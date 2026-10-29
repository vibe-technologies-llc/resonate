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
use resonate_engine::{Command, EngineConfig, Listening, Opening, Opens, Player, Zone, Zones};
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, SavedPlace, StoreOp, Transport,
    catalog::Catalog,
    documents::document_uri,
    output::{self, OutputFormat, OutputMixer},
    transport_of,
};

pub(crate) type Engines = dyn Fn(ZoneId, EngineConfig, &Arc<output::Routing>) -> resonate_engine::Result<Player>
    + Send
    + Sync;

pub(crate) struct Zoned {
    pub(crate) routing: Arc<output::Routing>,
    pub(crate) failure: Mutex<Option<String>>,
    pub(crate) waiting: Mutex<Option<String>>,
    pub(crate) saved: Mutex<Option<SavedPlace>>,
    pub(crate) listening: Mutex<(Listening, Option<i64>)>,
    route: Mutex<String>,
}

impl Zoned {
    fn new(routing: Arc<output::Routing>, route: String) -> Self {
        Self {
            routing,
            failure: Mutex::new(None),
            waiting: Mutex::new(None),
            saved: Mutex::new(None),
            listening: Mutex::new((Listening::default(), None)),
            route: Mutex::new(route),
        }
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
    bound: Mutex<BTreeMap<ZoneId, (i32, String)>>,
}

impl Opens for Opener {
    fn open(&self, opening: &Opening, alongside: &Player) -> resonate_engine::Result<Arc<Player>> {
        let (device, route) = self.bound.lock().remove(&opening.id).unwrap_or_default();
        let routing = Arc::new(output::Routing::serving(SinkId::new(opening.id.get())));
        routing.select(device);
        self.outputs.followed_by(&routing);
        let config = EngineConfig {
            volume: opening.volume,
            ..alongside
                .output_settings()
                .applied_to(EngineConfig::default())
        };

        let player = (self.engines)(opening.id, config, &routing)?;
        self.zoned
            .lock()
            .insert(opening.id, Arc::new(Zoned::new(routing, route)));
        Ok(Arc::new(player))
    }

    fn closed(&self, zone: &Zone) {
        self.zoned.lock().remove(&zone.id);
    }
}

pub(crate) struct Playing {
    zones: Mutex<Option<Arc<Zones>>>,
    shown: AtomicU32,
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
        let name = kept
            .and_then(|kept| ZoneName::given(&kept.name))
            .unwrap_or_else(|| ZoneName::first_given_to(ZoneId::FIRST));
        let zoned = Arc::new(Mutex::new(BTreeMap::from([(
            ZoneId::FIRST,
            Arc::new(Zoned::new(routing, route)),
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
            shown: AtomicU32::new(ZoneId::FIRST.get()),
            zoned,
            outputs,
            opener: Some(opener),
        }
    }

    #[cfg(test)]
    pub(crate) fn closed() -> Self {
        Self {
            zones: Mutex::new(None),
            shown: AtomicU32::new(ZoneId::FIRST.get()),
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

    fn bind_on_opening(&self, zone: ZoneId, device: i32, route: String) {
        if let Some(opener) = &self.opener {
            opener.bound.lock().insert(zone, (device, route));
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KeptZone {
    pub(crate) id: ZoneId,
    pub(crate) name: String,
    pub(crate) route: String,
}

impl Catalog {
    pub(crate) fn kept_zones(&self) -> Result<Vec<KeptZone>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare("SELECT id, name, route FROM zones ORDER BY id")
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = query
            .query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(rows
            .into_iter()
            .filter_map(|(id, name, route)| {
                Some(KeptZone {
                    id: ZoneId::new(id)?,
                    name,
                    route,
                })
            })
            .collect())
    }

    pub(crate) fn keep_zone(&self, zone: &KeptZone) -> Result<()> {
        self.0
            .lock()
            .execute(
                "INSERT INTO zones VALUES (?1, ?2, ?3) ON CONFLICT(id) DO UPDATE SET name = excluded.name, route = excluded.route",
                params![zone.id.get(), zone.name, zone.route],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn forget_zone(&self, zone: ZoneId) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM zones WHERE id = ?1", [zone.get()])
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
    pub transport: Transport,
    pub playing: Option<String>,
}

fn zone_named(zone: u32) -> Result<ZoneId> {
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
        let route = self.playing.zoned(id)?.route.lock().clone();

        self.playing.zones()?.rename(id, name.clone())?;
        self.catalog.keep_zone(&KeptZone {
            id,
            name: name.as_str().to_owned(),
            route,
        })
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

        if device < 0 || !zoned.routing.select(device) {
            return Err(Error::OutputUnavailable);
        }
        *zoned.route.lock() = route.clone();
        self.catalog.keep_zone(&KeptZone {
            id,
            name: name.as_str().to_owned(),
            route,
        })?;
        self.playing.send_to(id, Command::SetSink(None))
    }

    pub fn hand_over_to(&self, zone: u32) -> Result<()> {
        let to = zone_named(zone)?;
        let from = self.playing.shown();
        let zones = self.playing.zones()?;

        zones.hand_over(from, to)?;
        self.playing.shown.store(to.get(), Ordering::Release);
        self.services.watching.tell();
        Ok(())
    }
}

impl MobilePlayer {
    fn zone_added(&self, name: &str, device: i32, route: String) -> Result<Zone> {
        let zones = self.playing.zones()?;
        let id = zones.next_id().ok_or(resonate_engine::Error::NoZoneLeft)?;
        let name = ZoneName::given(name).unwrap_or_else(|| ZoneName::first_given_to(id));
        let kept = KeptZone {
            id,
            name: name.as_str().to_owned(),
            route,
        };

        self.catalog.keep_zone(&kept)?;
        self.zone_opened(&zones, &kept, device).or_else(|error| {
            self.catalog.forget_zone(id)?;
            Err(error)
        })
    }

    fn player_zone(&self, zone: &Zone, shown: ZoneId) -> PlayerZone {
        let state = zone.player.state();
        let route = self
            .playing
            .zoned(zone.id)
            .map(|zoned| (zoned.route.lock().clone(), zoned.routing.requested()))
            .unwrap_or_default();
        let playing = state.current.and_then(|current| {
            let queued = zone.player.queued();
            let row = queued.rows.iter().find(|row| row.id == current.id)?;
            let uri = document_uri(&row.location).ok()?;
            self.catalog.track_uri(&uri).ok().map(|track| track.title)
        });

        PlayerZone {
            id: zone.id.get(),
            name: zone.name.as_str().to_owned(),
            route: route.0,
            device: route.1,
            shown: zone.id == shown,
            transport: transport_of(state.playback),
            playing,
        }
    }

    fn zone_opened(&self, zones: &Arc<Zones>, kept: &KeptZone, device: i32) -> Result<Zone> {
        let name = ZoneName::given(&kept.name).unwrap_or_else(|| ZoneName::first_given_to(kept.id));
        self.playing
            .bind_on_opening(kept.id, device, kept.route.clone());

        let zone = zones.open(&Opening {
            id: kept.id,
            name,
            sink: None,
            volume: Volume::MAX,
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
            .zone_added("Kitchen", 0, String::new())
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
