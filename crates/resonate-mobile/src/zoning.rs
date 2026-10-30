use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use parking_lot::Mutex;
use resonate_audio::SinkId;
use resonate_core::{Volume, ZoneId, ZoneName};
use resonate_engine::{
    Command, EngineConfig, Listening, NodeName, Opening, Opens, PlaybackState, Player, Zone, Zones,
};
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, SavedPlace, StoreOp, Transport,
    catalog::Catalog,
    documents::document_uri,
    output::{self, OutputFormat, OutputMixer},
    rendering, transport_of,
};

const SYSTEM_DEFAULT_DEVICE: i32 = 0;
const VOLUME_KEPT_WITHIN: f32 = 0.001;
const OUTPUTS_ARE_HELD_PER_DEVICE: bool = cfg!(not(target_os = "ios"));

pub(crate) type Engines = dyn Fn(ZoneId, EngineConfig, &Arc<output::Routing>) -> resonate_engine::Result<Player>
    + Send
    + Sync;

pub(crate) struct Zoned {
    pub(crate) routing: Arc<output::Routing>,
    pub(crate) failure: Mutex<Option<String>>,
    pub(crate) waiting: Mutex<Option<String>>,
    pub(crate) saved: Mutex<Option<SavedPlace>>,
    pub(crate) listening: Mutex<(Listening, Option<i64>)>,
    pub(crate) renderer: Mutex<Option<KeptRenderer>>,
    route: Mutex<String>,
    volume: Mutex<Volume>,
}

impl Zoned {
    fn new(
        routing: Arc<output::Routing>,
        route: String,
        volume: Volume,
        renderer: Option<KeptRenderer>,
    ) -> Self {
        Self {
            routing,
            failure: Mutex::new(None),
            waiting: Mutex::new(None),
            saved: Mutex::new(None),
            listening: Mutex::new((Listening::default(), None)),
            renderer: Mutex::new(renderer),
            route: Mutex::new(route),
            volume: Mutex::new(volume),
        }
    }

    pub(crate) fn kept(&self, id: ZoneId, name: &ZoneName) -> KeptZone {
        KeptZone {
            id,
            name: name.as_str().to_owned(),
            route: self.route.lock().clone(),
            volume: *self.volume.lock(),
            renderer: self.renderer.lock().clone(),
        }
    }

    pub(crate) fn wanted_sink(&self) -> Option<NodeName> {
        rendering::sink_of(self.renderer.lock().as_ref())
    }
}

#[derive(Default)]
pub(crate) struct Outputs {
    follows: AtomicBool,
    offers: Mutex<Option<(i32, Vec<OutputFormat>)>>,
    mixer: Mutex<Option<Arc<dyn OutputMixer>>>,
    route: Mutex<Option<(u32, f64)>>,
}

impl Outputs {
    fn followed_by(&self, routing: &output::Routing) {
        routing.follow_song_rate(self.follows.load(Ordering::Acquire));
        if let Some((device, formats)) = self.offers.lock().clone() {
            routing.offer(device, &formats);
        }
        if let Some(mixer) = self.mixer.lock().clone() {
            routing.watch_mixer(mixer);
        }
        if let Some((rate, latency_ms)) = *self.route.lock() {
            routing.route_changed(rate, latency_ms);
        }
    }
}

struct Opener {
    engines: Box<Engines>,
    zoned: Arc<Mutex<BTreeMap<ZoneId, Arc<Zoned>>>>,
    outputs: Arc<Outputs>,
    bound: Mutex<BTreeMap<ZoneId, Bound>>,
}

#[derive(Default)]
struct Bound {
    device: i32,
    route: String,
    renderer: Option<KeptRenderer>,
}

impl Opens for Opener {
    fn open(&self, opening: &Opening, alongside: &Player) -> resonate_engine::Result<Arc<Player>> {
        let Bound {
            device,
            route,
            renderer,
        } = self.bound.lock().remove(&opening.id).unwrap_or_default();
        let routing = Arc::new(output::Routing::serving(SinkId::new(opening.id.get())));
        routing.select(device);
        self.outputs.followed_by(&routing);
        let config = EngineConfig {
            sink: rendering::sink_of(renderer.as_ref()).or_else(|| opening.sink.clone()),
            volume: opening.volume,
            ..alongside
                .output_settings()
                .applied_to(EngineConfig::default())
        };

        let player = (self.engines)(opening.id, config, &routing)?;
        self.zoned.lock().insert(
            opening.id,
            Arc::new(Zoned::new(routing, route, opening.volume, renderer)),
        );
        Ok(Arc::new(player))
    }

    fn closed(&self, zone: &Zone) {
        self.zoned.lock().remove(&zone.id);
    }
}

pub(crate) struct Playing {
    zones: Mutex<Option<Arc<Zones>>>,
    shown: Arc<AtomicU32>,
    zoned: Arc<Mutex<BTreeMap<ZoneId, Arc<Zoned>>>>,
    outputs: Arc<Outputs>,
    opener: Option<Arc<Opener>>,
}

impl Playing {
    pub(crate) fn start(
        first: Player,
        routing: Arc<output::Routing>,
        kept: Option<&KeptZone>,
        engines: Box<Engines>,
    ) -> Self {
        let route = kept.map(|kept| kept.route.clone()).unwrap_or_default();
        let volume = kept.map_or(Volume::MAX, |kept| kept.volume);
        let renderer = kept.and_then(|kept| kept.renderer.clone());
        let name = kept
            .and_then(|kept| ZoneName::given(&kept.name))
            .unwrap_or_else(|| ZoneName::first_given_to(ZoneId::FIRST));
        let zoned = Arc::new(Mutex::new(BTreeMap::from([(
            ZoneId::FIRST,
            Arc::new(Zoned::new(routing, route, volume, renderer)),
        )])));
        let outputs = Arc::new(Outputs::default());
        let opener = Arc::new(Opener {
            engines,
            zoned: Arc::clone(&zoned),
            outputs: Arc::clone(&outputs),
            bound: Mutex::new(BTreeMap::new()),
        });
        let zones = Zones::start(Arc::new(first), name, Arc::clone(&opener) as Arc<dyn Opens>);

        Self {
            zones: Mutex::new(Some(zones)),
            shown: Arc::new(AtomicU32::new(ZoneId::FIRST.get())),
            zoned,
            outputs,
            opener: Some(opener),
        }
    }

    #[cfg(test)]
    pub(crate) fn closed() -> Self {
        Self {
            zones: Mutex::new(None),
            shown: Arc::new(AtomicU32::new(ZoneId::FIRST.get())),
            zoned: Arc::new(Mutex::new(BTreeMap::new())),
            outputs: Arc::new(Outputs::default()),
            opener: None,
        }
    }

    pub(crate) fn zones(&self) -> Result<Arc<Zones>> {
        self.zones.lock().clone().ok_or(Error::Closed)
    }

    pub(crate) fn shown(&self) -> ZoneId {
        ZoneId::new(self.shown.load(Ordering::Acquire)).unwrap_or(ZoneId::FIRST)
    }

    pub(crate) fn shown_zone_cell(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.shown)
    }

    pub(crate) fn holder_of(&self, asking: ZoneId, device: i32) -> Option<ZoneId> {
        if !OUTPUTS_ARE_HELD_PER_DEVICE {
            return None;
        }
        let zones = self.zones.lock().clone()?;
        let routed = self.zoned.lock();

        zones
            .holding(asking)
            .into_iter()
            .map(|held| held.by)
            .find(|by| {
                routed.get(by).is_some_and(|zoned| {
                    reaches(zoned.routing.requested(), zoned.routing.actual(), device)
                })
            })
    }

    pub(crate) fn refusing_a_device_held(&self, asking: ZoneId, device: i32) -> Result<()> {
        match self.holder_of(asking, device) {
            Some(by) => Err(resonate_engine::Error::DeviceHeld { by }.into()),
            None => Ok(()),
        }
    }

    pub(crate) fn player_of(&self, zone: ZoneId) -> Result<Arc<Player>> {
        self.zones()?
            .zone(zone)
            .map(|zone| zone.player)
            .ok_or(Error::NoZone { zone: zone.get() })
    }

    pub(crate) fn player(&self) -> Result<Arc<Player>> {
        self.player_of(self.shown())
    }

    pub(crate) fn zoned(&self, zone: ZoneId) -> Result<Arc<Zoned>> {
        self.zoned
            .lock()
            .get(&zone)
            .cloned()
            .ok_or(Error::NoZone { zone: zone.get() })
    }

    pub(crate) fn every_zoned(&self) -> Vec<(ZoneId, Arc<Zoned>)> {
        self.zoned
            .lock()
            .iter()
            .map(|(id, zoned)| (*id, Arc::clone(zoned)))
            .collect()
    }

    pub(crate) fn listed(&self) -> Vec<Zone> {
        self.zones
            .lock()
            .as_ref()
            .map(|zones| zones.listed())
            .unwrap_or_default()
    }

    pub(crate) fn send_to(&self, zone: ZoneId, command: Command) -> Result<()> {
        Ok(self.zones()?.send(zone, command)?)
    }

    pub(crate) fn send(&self, command: Command) -> Result<()> {
        self.send_to(self.shown(), command)
    }

    pub(crate) fn take(&self) -> Option<Arc<Zones>> {
        self.zones.lock().take()
    }

    pub(crate) fn follow_song_rate(&self, follows: bool) -> Vec<ZoneId> {
        self.outputs.follows.store(follows, Ordering::Release);
        self.moved_by(|routing| routing.follow_song_rate(follows))
    }

    pub(crate) fn offer(&self, device: i32, bit_perfect: &[OutputFormat]) -> Vec<ZoneId> {
        *self.outputs.offers.lock() = Some((device, bit_perfect.to_vec()));
        self.moved_by(|routing| routing.offer(device, bit_perfect))
    }

    pub(crate) fn watch_mixer(&self, mixer: &Arc<dyn OutputMixer>) {
        *self.outputs.mixer.lock() = Some(Arc::clone(mixer));
        for (_, zoned) in self.every_zoned() {
            zoned.routing.watch_mixer(Arc::clone(mixer));
        }
    }

    pub(crate) fn route_changed(&self, rate: u32, latency_ms: f64) -> Vec<ZoneId> {
        *self.outputs.route.lock() = Some((rate, latency_ms));
        self.moved_by(|routing| routing.route_changed(rate, latency_ms))
    }

    fn moved_by(&self, change: impl Fn(&output::Routing) -> bool) -> Vec<ZoneId> {
        self.every_zoned()
            .into_iter()
            .filter(|(_, zoned)| change(&zoned.routing))
            .map(|(id, _)| id)
            .collect()
    }

    pub(crate) fn wanted_sink(&self, zone: ZoneId) -> Result<Option<NodeName>> {
        Ok(self.zoned(zone)?.wanted_sink())
    }

    fn bind_on_opening(&self, zone: ZoneId, device: i32, kept: &KeptZone) {
        if let Some(opener) = &self.opener {
            opener.bound.lock().insert(
                zone,
                Bound {
                    device,
                    route: kept.route.clone(),
                    renderer: kept.renderer.clone(),
                },
            );
        }
    }
}

const fn reaches(requested: i32, actual: i32, asked: i32) -> bool {
    requested == asked || (asked != SYSTEM_DEFAULT_DEVICE && actual == asked)
}

fn handover_clashes(held_by: ZoneId, from: ZoneId, sounding: bool) -> bool {
    held_by != from || sounding
}

pub(crate) fn first_zones_sink(kept: &[KeptZone]) -> Option<NodeName> {
    kept.iter()
        .find(|zone| zone.id.is_first())
        .and_then(|zone| rendering::sink_of(zone.renderer.as_ref()))
}

pub(crate) fn first_zones_volume(kept: &[KeptZone]) -> Volume {
    kept.iter()
        .find(|zone| zone.id.is_first())
        .map_or(Volume::MAX, |zone| zone.volume)
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeptZone {
    pub(crate) id: ZoneId,
    pub(crate) name: String,
    pub(crate) route: String,
    pub(crate) volume: Volume,
    pub(crate) renderer: Option<KeptRenderer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptRenderer {
    pub(crate) node: String,
    pub(crate) name: String,
}

impl Catalog {
    pub(crate) fn kept_zones(&self) -> Result<Vec<KeptZone>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT id, name, route, volume, renderer, renderer_name FROM zones ORDER BY id",
            )
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f32>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, name, route, volume, node, renderer_name)| {
                Some(KeptZone {
                    id: ZoneId::new(id)?,
                    name,
                    route,
                    volume: Volume::new(volume).unwrap_or(Volume::MAX),
                    renderer: node.map(|node| KeptRenderer {
                        name: renderer_name.unwrap_or_else(|| node.clone()),
                        node,
                    }),
                })
            })
            .collect())
    }

    pub(crate) fn keep_zone(&self, zone: &KeptZone) -> Result<()> {
        self.0
            .lock()
            .execute(
                "INSERT INTO zones(id, name, route, volume, renderer, renderer_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET name = excluded.name, route = excluded.route, renderer = excluded.renderer, renderer_name = excluded.renderer_name",
                params![
                    zone.id.get(),
                    zone.name,
                    zone.route,
                    zone.volume.get(),
                    zone.renderer.as_ref().map(|renderer| renderer.node.as_str()),
                    zone.renderer.as_ref().map(|renderer| renderer.name.as_str()),
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn keep_volume(&self, zone: &KeptZone) -> Result<()> {
        self.0
            .lock()
            .execute(
                "INSERT INTO zones(id, name, route, volume, renderer, renderer_name) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(id) DO UPDATE SET volume = excluded.volume",
                params![
                    zone.id.get(),
                    zone.name,
                    zone.route,
                    zone.volume.get(),
                    zone.renderer.as_ref().map(|renderer| renderer.node.as_str()),
                    zone.renderer.as_ref().map(|renderer| renderer.name.as_str()),
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn forget_zone(&self, zone: ZoneId) -> Result<()> {
        let connection = self.0.lock();
        connection
            .execute("DELETE FROM zones WHERE id = ?1", [zone.get()])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        crate::catalog::forget_served_rows_of(&connection, zone)
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PlayerZone {
    pub id: u32,
    pub name: String,
    pub route: String,
    pub device: i32,
    pub shown: bool,
    pub volume: f32,
    pub transport: Transport,
    pub playing: Option<String>,
    #[uniffi(default)]
    pub renderer: Option<String>,
    #[uniffi(default)]
    pub renderer_name: Option<String>,
}

pub(crate) fn zone_named(zone: u32) -> Result<ZoneId> {
    ZoneId::new(zone).ok_or(Error::NoZone { zone })
}

#[uniffi::export]
impl MobilePlayer {
    pub fn zones(&self) -> Vec<PlayerZone> {
        let shown = self.playing.shown();
        self.playing
            .listed()
            .into_iter()
            .map(|zone| self.player_zone(&zone, shown))
            .collect()
    }

    pub fn shown_zone(&self) -> u32 {
        self.playing.shown().get()
    }

    pub fn show_zone(&self, zone: u32) -> Result<()> {
        let id = zone_named(zone)?;
        self.playing.player_of(id)?;
        self.playing.shown.store(id.get(), Ordering::Release);
        self.services.watching.tell();
        Ok(())
    }

    pub fn add_zone(
        self: Arc<Self>,
        name: String,
        device: i32,
        route: String,
    ) -> Result<PlayerZone> {
        let zone = self.zone_added(&name, device, route)?;
        crate::watching::keep(&self, zone.id);
        Ok(self.player_zone(&zone, self.playing.shown()))
    }

    pub fn rename_zone(&self, zone: u32, name: String) -> Result<()> {
        let id = zone_named(zone)?;
        let name = ZoneName::given(&name).ok_or(Error::ZoneUnnamed)?;
        let zoned = self.playing.zoned(id)?;

        self.playing.zones()?.rename(id, name.clone())?;
        self.catalog.keep_zone(&zoned.kept(id, &name))
    }

    pub fn remove_zone(&self, zone: u32) -> Result<()> {
        let id = zone_named(zone)?;
        if id.is_first() {
            return Err(Error::FirstZoneStays);
        }
        let zones = self.playing.zones()?;
        let listening = self.playing.zoned(id)?;

        if let Err(error) = self.settle_listening(&listening) {
            tracing::warn!(%error, zone, "the listen of a zone taken away was not kept");
        }
        zones.remove(id)?;
        if self.playing.shown() == id {
            self.playing
                .shown
                .store(ZoneId::FIRST.get(), Ordering::Release);
        }
        self.catalog.forget_zone(id)?;
        self.services.watching.tell();
        Ok(())
    }

    pub fn select_zone_output(&self, zone: u32, device: i32, route: String) -> Result<()> {
        let id = zone_named(zone)?;
        let zoned = self.playing.zoned(id)?;
        let name = self
            .playing
            .zones()?
            .zone(id)
            .ok_or(Error::NoZone { zone })?
            .name;

        if device < 0 {
            return Err(Error::OutputUnavailable);
        }
        self.playing.refusing_a_device_held(id, device)?;
        if !zoned.routing.select(device) {
            return Err(Error::OutputUnavailable);
        }
        *zoned.route.lock() = route;
        self.catalog.keep_zone(&zoned.kept(id, &name))?;
        self.playing
            .send_to(id, Command::SetSink(zoned.wanted_sink()))
    }

    pub fn hand_over_to(&self, zone: u32) -> Result<()> {
        let to = zone_named(zone)?;
        let from = self.playing.shown();
        let zones = self.playing.zones()?;

        self.refusing_a_handover_onto_a_held_device(from, to)?;
        zones.hand_over(from, to)?;
        self.playing.shown.store(to.get(), Ordering::Release);
        self.services.watching.tell();
        Ok(())
    }
}

impl MobilePlayer {
    fn refusing_a_handover_onto_a_held_device(&self, from: ZoneId, to: ZoneId) -> Result<()> {
        let device = self.playing.zoned(to)?.routing.requested();
        let Some(by) = self.playing.holder_of(to, device) else {
            return Ok(());
        };
        let sounding = matches!(
            self.playing.player_of(from)?.state().playback,
            PlaybackState::Playing | PlaybackState::Buffering
        );
        if handover_clashes(by, from, sounding) {
            return Err(resonate_engine::Error::DeviceHeld { by }.into());
        }
        Ok(())
    }

    fn zone_added(&self, name: &str, device: i32, route: String) -> Result<Zone> {
        let zones = self.playing.zones()?;
        let id = zones.next_id().ok_or(resonate_engine::Error::NoZoneLeft)?;
        let name = ZoneName::given(name).unwrap_or_else(|| ZoneName::first_given_to(id));
        let kept = KeptZone {
            id,
            name: name.as_str().to_owned(),
            route,
            volume: Volume::MAX,
            renderer: None,
        };

        self.playing.refusing_a_device_held(id, device)?;
        self.catalog.keep_zone(&kept)?;
        self.zone_opened(&zones, &kept, device).or_else(|error| {
            self.catalog.forget_zone(id)?;
            Err(error)
        })
    }

    fn player_zone(&self, zone: &Zone, shown: ZoneId) -> PlayerZone {
        let state = zone.player.state();
        let (route, device, renderer) = self
            .playing
            .zoned(zone.id)
            .map(|zoned| {
                (
                    zoned.route.lock().clone(),
                    zoned.routing.requested(),
                    zoned.renderer.lock().clone(),
                )
            })
            .unwrap_or_default();
        let volume = state.volume.get();
        let playing = state.current.and_then(|current| {
            let queued = zone.player.queued();
            let row = queued.rows.iter().find(|row| row.id == current.id)?;
            let uri = document_uri(&row.location).ok()?;
            self.catalog.track_uri(&uri).ok().map(|track| track.title)
        });

        let renderer_name = renderer.as_ref().map(|renderer| {
            self.rendering
                .named(&renderer.node)
                .unwrap_or_else(|| renderer.name.clone())
        });

        PlayerZone {
            id: zone.id.get(),
            name: zone.name.as_str().to_owned(),
            route,
            device,
            renderer: renderer.map(|renderer| renderer.node),
            renderer_name,
            shown: zone.id == shown,
            volume,
            transport: transport_of(state.playback),
            playing,
        }
    }

    pub(crate) fn zone_opened(
        &self,
        zones: &Arc<Zones>,
        kept: &KeptZone,
        device: i32,
    ) -> Result<Zone> {
        let name = ZoneName::given(&kept.name).unwrap_or_else(|| ZoneName::first_given_to(kept.id));
        self.playing.bind_on_opening(kept.id, device, kept);

        let zone = zones.open(&Opening {
            id: kept.id,
            name,
            sink: None,
            volume: kept.volume,
        })?;
        if let Some(impulse) = self.room.applied() {
            zone.player.send(Command::SetConvolution(Some(impulse)))?;
        }
        Ok(zone)
    }

    pub(crate) fn restore_zones(self: &Arc<Self>, kept: &[KeptZone]) -> Result<()> {
        let zones = self.playing.zones()?;
        for kept in kept.iter().filter(|kept| !kept.id.is_first()) {
            if let Err(error) = self.zone_opened(&zones, kept, 0) {
                tracing::warn!(%error, zone = %kept.id, "a zone kept from the last run did not open");
                continue;
            }
            if let Err(error) = self.resume_saved_in(kept.id) {
                tracing::warn!(%error, zone = %kept.id, "a zone's queue kept from the last run was not taken up");
            }
            crate::watching::keep(self, kept.id);
        }
        Ok(())
    }
}

impl MobilePlayer {
    pub(crate) fn keep_volume_of(&self, zone: &Zone) -> Result<()> {
        let zoned = self.playing.zoned(zone.id)?;
        let volume = zone.player.state().volume;
        if (volume.get() - zoned.volume.lock().get()).abs() < VOLUME_KEPT_WITHIN {
            return Ok(());
        }
        self.catalog.keep_volume(&KeptZone {
            volume,
            ..zoned.kept(zone.id, &zone.name)
        })?;
        *zoned.volume.lock() = volume;
        Ok(())
    }
}

pub(crate) fn shut_down(zones: Arc<Zones>) -> Result<()> {
    zones.leave();
    let players = zones
        .listed()
        .into_iter()
        .map(|zone| zone.player)
        .collect::<Vec<_>>();
    drop(zones);

    players
        .into_iter()
        .map(|player| match Arc::try_unwrap(player) {
            Ok(player) => player.shutdown().map_err(Error::from),
            Err(_) => Ok(()),
        })
        .fold(Ok(()), Result::and)
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Instant};

    use super::*;
    use crate::harness::{Harness, LOOKED_AGAIN_AFTER, SETTLED_WITHIN, name_of};

    fn names_queued(harness: &Harness) -> String {
        harness
            .player
            .queue()
            .expect("the queue")
            .iter()
            .map(name_of)
            .collect()
    }

    #[test]
    fn a_zone_added_keeps_a_queue_of_its_own_and_every_call_follows_the_zone_shown() {
        let harness = Harness::new();
        harness.resumed("ab", 0, 0);

        let kitchen = harness
            .player
            .zone_added("Kitchen", 3, "v1:usb:dac".into())
            .expect("a second zone");
        harness
            .player
            .show_zone(kitchen.id.get())
            .expect("the kitchen shown");
        harness
            .player
            .load(harness.ids("c"), 0, false)
            .expect("a queue for the kitchen");
        let kitchen_shown = harness.settled(|snapshot| snapshot.current.is_some());
        let kitchen_queue = names_queued(&harness);
        let listed = harness.player.zones();

        assert_eq!(kitchen_shown.zone, kitchen.id.get());
        assert_eq!(kitchen_queue, "c");
        assert_eq!(
            listed
                .iter()
                .map(|zone| (zone.id, zone.name.as_str(), zone.route.as_str(), zone.shown))
                .collect::<Vec<_>>(),
            vec![
                (1, "Zone 1", "", false),
                (kitchen.id.get(), "Kitchen", "v1:usb:dac", true),
            ]
        );

        harness.player.show_zone(1).expect("the first zone shown");
        let first_shown = harness.settled(|snapshot| snapshot.current.is_some());

        assert_eq!(first_shown.zone, 1);
        assert_eq!(names_queued(&harness), "ab");
        assert_ne!(first_shown.zones_revision, 0);
    }

    fn volume_near(player: &Player, wanted: f32) -> bool {
        (player.state().volume.get() - wanted).abs() < VOLUME_KEPT_WITHIN
    }

    fn settled_at(player: &Player, wanted: f32) {
        let began = Instant::now();
        while !volume_near(player, wanted) {
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "never heard at {wanted}: {}",
                player.state().volume
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    fn kept_volume(harness: &Harness, zone: ZoneId) -> Option<f32> {
        harness
            .player
            .catalog
            .kept_zones()
            .expect("read")
            .iter()
            .find(|kept| kept.id == zone)
            .map(|kept| kept.volume.get())
    }

    const HELD_DEVICE: i32 = 7;

    fn playing_bit_perfect() -> (Harness, Zone) {
        let harness = Harness::sounding();
        harness.resumed("abc", 1, 0);
        let kitchen = harness
            .player
            .zone_added("Kitchen", HELD_DEVICE, String::new())
            .expect("a second zone");
        harness
            .player
            .select_output(HELD_DEVICE)
            .expect("the device chosen");
        harness.player.play().expect("played");
        harness.settled(|snapshot| snapshot.transport == Transport::Playing);

        let asking = ZoneId::new(9).expect("a zone");
        let began = Instant::now();
        while harness
            .player
            .playing
            .holder_of(asking, HELD_DEVICE)
            .is_none()
        {
            assert!(
                began.elapsed() < SETTLED_WITHIN,
                "the first zone never held its device"
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
        (harness, kitchen)
    }

    #[test]
    fn each_zones_volume_is_kept_where_it_moved_and_a_zone_opens_at_the_volume_it_was_kept_at() {
        let harness = Harness::new();
        let kitchen = harness
            .player
            .zone_added("Kitchen", 0, String::new())
            .expect("a second zone");

        harness.player.save().expect("every zone saved");
        let untouched = harness.player.catalog.kept_zones().expect("read");

        harness.player.set_volume(0.5).expect("the first quieter");
        settled_at(
            &harness
                .player
                .playing
                .player_of(ZoneId::FIRST)
                .expect("a zone"),
            0.5,
        );
        harness
            .player
            .show_zone(kitchen.id.get())
            .expect("the kitchen shown");
        harness
            .player
            .set_volume(0.25)
            .expect("the kitchen quieter");
        settled_at(
            &harness
                .player
                .playing
                .player_of(kitchen.id)
                .expect("a zone"),
            0.25,
        );
        harness.player.save().expect("every zone saved");
        harness
            .player
            .rename_zone(kitchen.id.get(), "Den".into())
            .expect("renamed");

        assert_eq!(
            untouched.iter().map(|kept| kept.id).collect::<Vec<_>>(),
            vec![kitchen.id]
        );
        assert!((kept_volume(&harness, ZoneId::FIRST).expect("kept") - 0.5).abs() < 0.001);
        assert!((kept_volume(&harness, kitchen.id).expect("kept") - 0.25).abs() < 0.001);
        assert_eq!(
            harness
                .player
                .zones()
                .iter()
                .map(|zone| (zone.id, (zone.volume * 100.0).round() as i32))
                .collect::<Vec<_>>(),
            vec![(1, 50), (kitchen.id.get(), 25)]
        );

        let reopened = KeptZone {
            id: ZoneId::new(5).expect("a zone"),
            name: "Study".into(),
            route: String::new(),
            volume: Volume::new(0.4).expect("a volume"),
            renderer: None,
        };
        let zones = harness.player.playing.zones().expect("the zones");
        let study = harness
            .player
            .zone_opened(&zones, &reopened, 0)
            .expect("a zone opened from the kept row");

        settled_at(&study.player, 0.4);
        assert!(
            (harness
                .player
                .playing
                .zoned(study.id)
                .expect("a zone")
                .volume
                .lock()
                .get()
                - 0.4)
                .abs()
                < 0.001
        );
    }

    #[test]
    fn the_first_zone_reopens_at_the_volume_kept_beside_it_and_full_without_one() {
        let kept = |id: u32, volume: f32| KeptZone {
            id: ZoneId::new(id).expect("a zone"),
            name: "Zone".into(),
            route: String::new(),
            volume: Volume::new(volume).expect("a volume"),
            renderer: None,
        };

        assert_eq!(first_zones_volume(&[]), Volume::MAX);
        assert_eq!(first_zones_volume(&[kept(2, 0.3)]), Volume::MAX);
        assert_eq!(first_zones_volume(&[kept(1, 0.6), kept(2, 0.3)]).get(), 0.6);
    }

    #[test]
    fn a_device_is_reached_by_the_choice_made_or_the_one_the_system_gave() {
        assert!(reaches(31, 31, 31));
        assert!(reaches(0, 31, 31));
        assert!(reaches(0, 0, 0));
        assert!(!reaches(31, 31, 7));
        assert!(!reaches(31, 31, 0));
        assert!(!reaches(0, 0, 7));
    }

    #[test]
    fn a_handover_onto_a_held_device_is_refused_unless_the_holder_is_the_zone_handing_over_and_silent()
     {
        let first = ZoneId::FIRST;
        let second = ZoneId::new(2).expect("a zone");

        assert!(handover_clashes(second, first, false));
        assert!(handover_clashes(first, first, true));
        assert!(!handover_clashes(first, first, false));
    }

    #[test]
    fn a_device_another_zone_holds_bit_perfect_is_refused_to_every_way_of_binding_a_zone_to_it() {
        let (harness, kitchen) = playing_bit_perfect();

        let added = harness
            .player
            .zone_added("Study", HELD_DEVICE, String::new());
        let selected =
            harness
                .player
                .select_zone_output(kitchen.id.get(), HELD_DEVICE, String::new());
        harness
            .player
            .show_zone(kitchen.id.get())
            .expect("the kitchen shown");
        let chosen = harness.player.select_output(HELD_DEVICE);
        let handed = harness
            .player
            .show_zone(1)
            .and_then(|()| harness.player.hand_over_to(kitchen.id.get()));
        let free = harness
            .player
            .zone_added("Den", HELD_DEVICE + 1, String::new());

        for refused in [added.map(|_| ()), selected, chosen, handed] {
            assert!(
                matches!(
                    refused,
                    Err(Error::Player(resonate_engine::Error::DeviceHeld { by })) if by == ZoneId::FIRST
                ),
                "a held device was handed on: {refused:?}"
            );
        }
        assert!(free.is_ok());
        assert_eq!(harness.player.zones().len(), 3);
    }

    #[test]
    fn each_zones_queue_is_kept_under_its_id_and_goes_with_the_zone() {
        let harness = Harness::new();
        harness.resumed("ab", 1, 0);
        let study = harness
            .player
            .zone_added("Study", 0, String::new())
            .expect("a second zone");
        harness
            .player
            .show_zone(study.id.get())
            .expect("the study shown");
        harness
            .player
            .load(harness.ids("dc"), 1, false)
            .expect("a queue for the study");
        harness.settled(|snapshot| snapshot.current.is_some());

        harness.player.save().expect("every zone saved");

        assert_eq!(
            harness
                .player
                .catalog
                .resume_of(ZoneId::FIRST)
                .expect("read"),
            (harness.ids("ab"), 1, 0)
        );
        assert_eq!(
            harness.player.catalog.resume_of(study.id).expect("read"),
            (harness.ids("dc"), 1, 0)
        );
        assert_eq!(
            harness.player.catalog.kept_zones().expect("read"),
            vec![KeptZone {
                id: study.id,
                name: "Study".into(),
                route: String::new(),
                volume: Volume::MAX,
                renderer: None,
            }]
        );

        harness
            .player
            .remove_zone(study.id.get())
            .expect("the study taken away");

        assert_eq!(harness.player.shown_zone(), 1);
        assert_eq!(harness.player.zones().len(), 1);
        assert_eq!(
            harness.player.catalog.resume_of(study.id).expect("read"),
            (Vec::new(), 0, 0)
        );
        assert!(
            harness
                .player
                .catalog
                .kept_zones()
                .expect("read")
                .is_empty()
        );
    }

    #[test]
    fn a_zone_renamed_keeps_its_new_name_and_a_blank_name_or_the_first_zones_removal_is_refused() {
        let harness = Harness::new();
        let study = harness
            .player
            .zone_added("  ", 0, String::new())
            .expect("a second zone");

        harness
            .player
            .rename_zone(1, "Living room".into())
            .expect("the first renamed");

        assert_eq!(study.name.as_str(), "Zone 2");
        assert_eq!(harness.player.zones()[0].name, "Living room");
        assert!(matches!(
            harness.player.rename_zone(study.id.get(), " ".into()),
            Err(Error::ZoneUnnamed)
        ));
        assert!(matches!(
            harness.player.remove_zone(1),
            Err(Error::FirstZoneStays)
        ));
        assert!(matches!(
            harness.player.show_zone(9),
            Err(Error::NoZone { zone: 9 })
        ));
        assert!(matches!(
            harness.player.show_zone(0),
            Err(Error::NoZone { zone: 0 })
        ));
        assert_eq!(
            harness
                .player
                .catalog
                .kept_zones()
                .expect("read")
                .iter()
                .map(|kept| kept.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Living room", "Zone 2"]
        );
    }

    #[test]
    fn the_song_handed_to_another_zone_plays_there_and_leaves_the_zone_it_came_from() {
        let harness = Harness::sounding();
        harness.resumed("abc", 1, 0);
        harness.player.play().expect("played");
        harness.settled(|snapshot| snapshot.transport == Transport::Playing);
        let kitchen = harness
            .player
            .zone_added("Kitchen", 7, String::new())
            .expect("a second zone");

        harness
            .player
            .hand_over_to(kitchen.id.get())
            .expect("handed over");

        assert_eq!(harness.player.shown_zone(), kitchen.id.get());
        let began = Instant::now();
        loop {
            let left = harness
                .player
                .playing
                .player_of(ZoneId::FIRST)
                .expect("the first zone")
                .queued()
                .rows
                .len();
            let snapshot = harness.player.snapshot().expect("a snapshot");
            if left == 0 && snapshot.transport == Transport::Playing {
                assert_eq!(snapshot.queue_index, 1);
                break;
            }
            assert!(
                began.elapsed() < SETTLED_WITHIN * 2,
                "never handed over: {snapshot:?}"
            );
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
        assert_eq!(names_queued(&harness), "abc");
    }
}
