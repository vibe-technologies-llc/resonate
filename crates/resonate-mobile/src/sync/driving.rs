use std::{
    collections::BTreeMap,
    slice,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{RemoteTrust, ZoneId};
use resonate_driven::{Driven, RowNames, unnamed};
use resonate_engine::{Player, QueueItem};
use resonate_sync::{
    Controlling, Mirror, Permille, RemoteCommand, RemotePlayback, RemoteRow, RemoteZone, Watched,
};

use super::the_device;
use crate::{
    Error, MobilePlayer, Result, Track, catalog::Catalog, documents::document_uri, served::Served,
    words::driving as said,
};

const ROWS_KEPT_AT_MOST: usize = 8_192;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum SyncControl {
    #[default]
    Everything,
    Transport,
    Nothing,
}

impl From<SyncControl> for RemoteTrust {
    fn from(control: SyncControl) -> Self {
        match control {
            SyncControl::Everything => Self::Everything,
            SyncControl::Transport => Self::Transport,
            SyncControl::Nothing => Self::Nothing,
        }
    }
}

impl From<RemoteTrust> for SyncControl {
    fn from(trust: RemoteTrust) -> Self {
        match trust {
            RemoteTrust::Everything => Self::Everything,
            RemoteTrust::Transport => Self::Transport,
            RemoteTrust::Nothing => Self::Nothing,
        }
    }
}

impl SyncControl {
    fn choice(self) -> SyncControlChoice {
        let (label, meaning) = match self {
            Self::Everything => (said::everything(), said::everything_means()),
            Self::Transport => (said::transport(), said::transport_means()),
            Self::Nothing => (said::nothing(), said::nothing_means()),
        };

        SyncControlChoice {
            control: self,
            label: label.to_owned(),
            meaning: meaning.to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SyncControlChoice {
    pub control: SyncControl,
    pub label: String,
    pub meaning: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SyncControlOffered {
    pub title: String,
    pub asked: String,
    pub told: String,
    pub choices: Vec<SyncControlChoice>,
}

#[uniffi::export]
pub fn sync_control_offered() -> SyncControlOffered {
    SyncControlOffered {
        title: said::title().to_owned(),
        asked: said::asked().to_owned(),
        told: said::told().to_owned(),
        choices: RemoteTrust::ALL
            .into_iter()
            .map(|trust| SyncControl::from(trust).choice())
            .collect(),
    }
}

#[derive(Default)]
struct KeptRows {
    revision: u64,
    rows: BTreeMap<String, RemoteRow>,
}

struct NamedByTheCatalog {
    catalog: Arc<Catalog>,
    served: Arc<Served>,
    kept: Mutex<KeptRows>,
}

fn row_of(track: &Track) -> RemoteRow {
    RemoteRow {
        title: if track.title.is_empty() {
            track.name.clone()
        } else {
            track.title.clone()
        },
        artist: track.artist.clone(),
        length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
    }
}

impl NamedByTheCatalog {
    fn over(catalog: Arc<Catalog>, served: Arc<Served>) -> Self {
        Self {
            catalog,
            served,
            kept: Mutex::new(KeptRows::default()),
        }
    }

    fn read_outside_the_lock(&self, uris: Vec<String>) -> BTreeMap<String, RemoteRow> {
        uris.into_iter()
            .filter_map(|uri| {
                let tracks = self.catalog.tracks_at(slice::from_ref(&uri));
                let row = tracks.ok()?.first().map(row_of)?;
                Some((uri, row))
            })
            .collect()
    }

    fn documents(&self, items: &[Arc<QueueItem>]) -> BTreeMap<String, RemoteRow> {
        let revision = self.catalog.revision();
        let uris = items
            .iter()
            .filter(|item| !Served::names(&item.location))
            .filter_map(|item| document_uri(&item.location).ok());
        let (mut named, unread) = {
            let mut kept = self.kept.lock();
            if kept.revision != revision || kept.rows.len() > ROWS_KEPT_AT_MOST {
                kept.revision = revision;
                kept.rows.clear();
            }
            let mut named = BTreeMap::new();
            let mut unread = Vec::new();
            for uri in uris {
                match kept.rows.get(&uri) {
                    Some(row) => {
                        named.insert(uri, row.clone());
                    }
                    None => unread.push(uri),
                }
            }
            (named, unread)
        };
        let read = self.read_outside_the_lock(unread);
        let mut kept = self.kept.lock();
        if kept.revision == revision {
            kept.rows
                .extend(read.iter().map(|(uri, row)| (uri.clone(), row.clone())));
        }
        named.extend(read);
        named
    }
}

impl RowNames for NamedByTheCatalog {
    fn rows(&self, _: &Player, items: &[Arc<QueueItem>]) -> Vec<RemoteRow> {
        let documents = self.documents(items);

        items
            .iter()
            .map(|item| {
                if Served::names(&item.location) {
                    return row_of(&self.served.track_of(item));
                }
                document_uri(&item.location)
                    .ok()
                    .and_then(|uri| documents.get(&uri).cloned())
                    .unwrap_or_else(|| RemoteRow {
                        title: unnamed(item),
                        artist: String::new(),
                        length: None,
                    })
            })
            .collect()
    }
}

impl MobilePlayer {
    pub(super) fn driven(&self) -> Option<Driven> {
        let zones = self.playing.zones().ok()?;
        let names = NamedByTheCatalog::over(Arc::clone(&self.catalog), Arc::clone(&self.served));

        Some(Driven::zones(&zones, names))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteControl {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Seek { millis: u64 },
    Volume { permille: u16 },
    Move { from: u32, to: u32 },
    Remove { row: u32 },
    Jump { row: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteAnswer {
    Done,
    Untrusted,
    Uncontrollable,
    NoSuchZone,
    NoSuchRow,
    Failed,
    Unanswered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoteStanding {
    Waiting,
    Mirrored,
    Refused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemotePlaying {
    Idle,
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteSong {
    pub title: String,
    pub artist: String,
    pub length_millis: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteZoneState {
    pub zone: u32,
    pub name: String,
    pub playing: RemotePlaying,
    pub playing_row: Option<u32>,
    pub position_millis: u64,
    pub length_millis: Option<u64>,
    pub volume_permille: u16,
    pub queue_length: u32,
    pub first_row: u32,
    pub queue_revision: Option<u64>,
    pub rows: Vec<RemoteSong>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RemoteDevice {
    pub standing: RemoteStanding,
    pub zones: Vec<RemoteZoneState>,
}

fn millis(length: Duration) -> u64 {
    u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

fn row(given: u32) -> usize {
    usize::try_from(given).unwrap_or(usize::MAX)
}

const fn playing_of(playback: RemotePlayback) -> RemotePlaying {
    match playback {
        RemotePlayback::Idle => RemotePlaying::Idle,
        RemotePlayback::Playing => RemotePlaying::Playing,
        RemotePlayback::Paused => RemotePlaying::Paused,
        RemotePlayback::Stopped => RemotePlaying::Stopped,
    }
}

const fn answer_of(controlling: Controlling) -> RemoteAnswer {
    match controlling {
        Controlling::Done => RemoteAnswer::Done,
        Controlling::Untrusted => RemoteAnswer::Untrusted,
        Controlling::Uncontrollable => RemoteAnswer::Uncontrollable,
        Controlling::NoSuchZone => RemoteAnswer::NoSuchZone,
        Controlling::NoSuchRow => RemoteAnswer::NoSuchRow,
        Controlling::Failed => RemoteAnswer::Failed,
        Controlling::Unanswered => RemoteAnswer::Unanswered,
    }
}

pub(crate) fn command_of(control: RemoteControl) -> RemoteCommand {
    match control {
        RemoteControl::Play => RemoteCommand::Play,
        RemoteControl::Pause => RemoteCommand::Pause,
        RemoteControl::Toggle => RemoteCommand::Toggle,
        RemoteControl::Next => RemoteCommand::Next,
        RemoteControl::Previous => RemoteCommand::Previous,
        RemoteControl::Seek { millis } => RemoteCommand::Seek(Duration::from_millis(millis)),
        RemoteControl::Volume { permille } => RemoteCommand::Volume(Permille::new(permille)),
        RemoteControl::Move { from, to } => RemoteCommand::Move {
            from: row(from),
            to: row(to),
        },
        RemoteControl::Remove { row: at } => RemoteCommand::Remove(row(at)),
        RemoteControl::Jump { row: at } => RemoteCommand::Jump(row(at)),
    }
}

fn song_of(row: &RemoteRow) -> RemoteSong {
    RemoteSong {
        title: row.title.clone(),
        artist: row.artist.clone(),
        length_millis: row.length.map(millis),
    }
}

fn zone_state(mirror: &Mirror, zone: &RemoteZone, now: Instant) -> RemoteZoneState {
    RemoteZoneState {
        zone: zone.zone.get(),
        name: zone.name.clone(),
        playing: playing_of(zone.playback),
        playing_row: zone.playing.map(counted),
        position_millis: millis(mirror.position_of(zone, now)),
        length_millis: zone.length.map(millis),
        volume_permille: zone.volume.get(),
        queue_length: counted(zone.queue_length),
        first_row: counted(zone.first_row),
        queue_revision: zone.queue_revision,
        rows: zone.rows.iter().map(song_of).collect(),
    }
}

pub(crate) fn device_of(watched: &Watched, now: Instant) -> RemoteDevice {
    match watched {
        Watched::Waiting => RemoteDevice {
            standing: RemoteStanding::Waiting,
            zones: Vec::new(),
        },
        Watched::Refused(_) => RemoteDevice {
            standing: RemoteStanding::Refused,
            zones: Vec::new(),
        },
        Watched::Mirrored(mirror) => RemoteDevice {
            standing: RemoteStanding::Mirrored,
            zones: mirror
                .zones
                .iter()
                .map(|zone| zone_state(mirror, zone, now))
                .collect(),
        },
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn sync_watch(&self, device: String) -> Result<RemoteDevice> {
        let running = self.syncing.running.lock();
        let running = running.as_ref().ok_or(Error::SyncOff)?;
        let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
        running.sync.watch(device.id);
        Ok(device_of(&running.sync.watched(device.id), Instant::now()))
    }

    pub fn sync_unwatch(&self, device: String) {
        let running = self.syncing.running.lock();
        if let Some(running) = running.as_ref()
            && let Some(device) = the_device(&running.sync, &device)
        {
            running.sync.unwatch(device.id);
        }
    }

    pub fn sync_control(
        &self,
        device: String,
        zone: u32,
        control: RemoteControl,
        queue_revision: Option<u64>,
    ) -> Result<RemoteAnswer> {
        let asked = {
            let running = self.syncing.running.lock();
            let running = running.as_ref().ok_or(Error::SyncOff)?;
            let device = the_device(&running.sync, &device).ok_or(Error::UnknownSyncDevice)?;
            let Some(zone) = ZoneId::new(zone) else {
                return Ok(RemoteAnswer::NoSuchZone);
            };
            running
                .sync
                .ask_to_control(device.id, zone, command_of(control), queue_revision)
        };
        Ok(answer_of(asked.answer()))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        net::{Ipv4Addr, SocketAddr},
        path::Path,
        process, thread,
    };

    use resonate_core::Recording;
    use resonate_sync::{
        Broadcast, Catalog as SyncCatalog, Controlled, Follow, Held, LocalListen, LocalPlaylist,
        LocalPlaylistId, Me, Nearby, Opened, Platform, Refused, RemoteListen, Resumed, Server,
        Settings, SongTags, Sync, SyncKey, Uncontrolled,
    };

    use super::*;
    use crate::{
        harness::{Harness, SECONDS_A_SONG},
        sync::SyncConfig,
    };

    type Answer<T> = std::result::Result<T, Refused>;

    const SETTLES_WITHIN: Duration = Duration::from_secs(40);
    const LOOKED_AGAIN_AFTER: Duration = Duration::from_millis(50);

    fn eventually(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + SETTLES_WITHIN;
        while !done() {
            assert!(Instant::now() < deadline, "{what} never happened");
            thread::sleep(LOOKED_AGAIN_AFTER);
        }
    }

    fn titles(zone: &RemoteZone) -> Vec<String> {
        zone.rows.iter().map(|row| row.title.clone()).collect()
    }

    #[test]
    fn a_phones_zones_are_mirrored_by_the_catalogs_names_and_driven_by_row() {
        let harness = Harness::sounding();
        let player = &harness.player;
        let driven = player.driven().expect("the zones");
        let rows: Vec<String> = "abc"
            .chars()
            .map(|name| harness.song(name).title.clone())
            .collect();

        player
            .load(harness.ids("abc"), 0, false)
            .expect("the queue loaded");

        eventually("the rows named by the catalog", || {
            let zones = driven.zones();
            zones.len() == 1 && zones[0].queue_length == 3 && titles(&zones[0]) == rows
        });
        assert_eq!(
            driven.zones()[0].rows[0].length,
            Some(Duration::from_secs(u64::from(SECONDS_A_SONG)))
        );

        driven
            .control(ZoneId::FIRST, RemoteCommand::Move { from: 2, to: 0 })
            .expect("a move");
        eventually("the move shown", || {
            titles(&driven.zones()[0]) == [rows[2].clone(), rows[0].clone(), rows[1].clone()]
        });

        driven
            .control(ZoneId::FIRST, RemoteCommand::Remove(1))
            .expect("a removal");
        eventually("the row gone from the zone shown", || {
            player.queue().map(|queue| queue.len()).unwrap_or_default() == 2
        });

        driven
            .control(ZoneId::FIRST, RemoteCommand::Jump(1))
            .expect("a jump");
        eventually("the row jumped to is the one a snapshot names", || {
            player
                .snapshot()
                .ok()
                .and_then(|snapshot| snapshot.current)
                .is_some_and(|track| track.title == rows[1])
        });

        driven
            .control(ZoneId::FIRST, RemoteCommand::Volume(Permille::new(250)))
            .expect("the volume");
        eventually("the volume taken", || {
            driven.zones()[0].volume == Permille::new(250)
        });

        assert_eq!(
            driven.control(ZoneId::FIRST, RemoteCommand::Remove(7)),
            Err(Uncontrolled::NoSuchRow)
        );
        assert_eq!(
            driven.control(ZoneId::new(9).expect("a zone"), RemoteCommand::Play),
            Err(Uncontrolled::NoSuchZone)
        );
    }

    #[test]
    fn a_row_retitled_in_the_catalog_is_mirrored_by_its_new_name() {
        let harness = Harness::sounding();
        let player = &harness.player;
        let driven = player.driven().expect("the zones");
        let song = harness.song('a').clone();

        player
            .load(vec![song.id], 0, false)
            .expect("the queue loaded");
        eventually("the row named", || {
            driven.zones()[0].rows.first().map(|row| &row.title) == Some(&song.title)
        });

        player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET title = 'Renamed' WHERE id = ?1",
                [song.id as i64],
            )
            .expect("the row retitled");

        eventually("the new name mirrored", || {
            driven.zones()[0].rows.first().map(|row| row.title.as_str()) == Some("Renamed")
        });
    }

    #[test]
    fn what_a_device_may_do_here_is_worded_once_and_everything_unless_told() {
        let offered = sync_control_offered();

        assert_eq!(SyncControl::default(), SyncControl::Everything);
        assert_eq!(
            RemoteTrust::default(),
            RemoteTrust::from(SyncControl::default())
        );
        assert_eq!(offered.asked, "What your other devices may do here");
        assert_eq!(
            offered
                .choices
                .iter()
                .map(|choice| (choice.control, choice.label.as_str()))
                .collect::<Vec<_>>(),
            [
                (SyncControl::Everything, "Everything"),
                (SyncControl::Transport, "Playback only"),
                (SyncControl::Nothing, "Nothing"),
            ]
        );
    }

    struct NothingHeld;

    impl SyncCatalog for NothingHeld {
        fn playlists(&self) -> Answer<Vec<LocalPlaylist>> {
            Ok(Vec::new())
        }

        fn write_playlist(
            &self,
            _: Option<LocalPlaylistId>,
            _: &str,
            _: &[Recording],
        ) -> Answer<LocalPlaylist> {
            Err(Refused::Unwritable)
        }

        fn remove_playlist(&self, _: LocalPlaylistId) -> Answer<()> {
            Ok(())
        }

        fn favourites(&self) -> Answer<Vec<Recording>> {
            Ok(Vec::new())
        }

        fn favour(&self, _: &Recording, _: bool) -> Answer<bool> {
            Ok(false)
        }

        fn listens_after(&self, _: u64, _: usize) -> Answer<Vec<LocalListen>> {
            Ok(Vec::new())
        }

        fn credit(&self, _: &RemoteListen) -> Answer<bool> {
            Ok(false)
        }

        fn resumption(&self) -> Answer<Option<Resumed>> {
            Ok(None)
        }

        fn holds(&self, _: &Recording) -> Answer<bool> {
            Ok(false)
        }

        fn holdings(&self) -> Answer<Vec<Held>> {
            Ok(Vec::new())
        }

        fn open(&self, _: &Recording) -> Answer<Option<Opened>> {
            Ok(None)
        }

        fn land(&self, _: &Recording, _: &SongTags, _: &str, _: &Path) -> Answer<bool> {
            Ok(false)
        }

        fn revision(&self) -> u64 {
            0
        }
    }

    #[test]
    fn a_phone_is_driven_across_the_wire_as_far_as_its_setting_trusts() {
        let harness = Harness::sounding();
        let player = &harness.player;
        let folder = env::temp_dir().join(format!("resonate-driven-phone-{}", process::id()));
        fs::create_dir_all(&folder).expect("a test folder");
        let key = SyncKey::minted().expect("a key");

        player
            .load(harness.ids("ab"), 0, false)
            .expect("the queue loaded");
        player
            .sync_start(SyncConfig {
                enabled: true,
                nearby_only: true,
                server: String::new(),
                key: key.code().to_owned(),
                device: "Phone".to_owned(),
                favourites: false,
                playlists: false,
                everything: false,
                metered_allowed: true,
                control: SyncControl::Transport,
            })
            .expect("sync started");
        let told = player
            .syncing
            .running
            .lock()
            .as_ref()
            .expect("running")
            .sync
            .nearby_port()
            .map(|port| SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
        let desk = Sync::start(
            Settings {
                server: Server::Nowhere,
                trusted: None,
                stun: None,
                key,
                me: Me {
                    name: "Desk".into(),
                    platform: Platform::Linux,
                },
                follow: Follow::default(),
                ledger: folder.join("desk.sqlite"),
                staging: folder.join("desk-staging"),
                transfers_allowed: true,
                relay_only: false,
                nearby: Some(Nearby {
                    broadcast: Some(Broadcast {
                        port: 0,
                        told: told.into_iter().collect(),
                    }),
                    bonjour: false,
                }),
            },
            Arc::new(NothingHeld),
        )
        .expect("the desk started");
        let the_phone = || {
            desk.shown()
                .devices
                .into_iter()
                .find(|known| known.name == "Phone" && known.nearby)
                .map(|known| known.id)
        };
        eventually("the desk meeting the phone", || the_phone().is_some());
        let phone = the_phone().expect("the phone");

        eventually("the phone's queue mirrored on the desk", || {
            desk.watch(phone);
            desk.watched(phone)
                .mirror()
                .is_some_and(|mirror| mirror.zones[0].queue_length == 2)
        });

        assert_eq!(
            desk.control(phone, ZoneId::FIRST, RemoteCommand::Jump(1)),
            Controlling::Done
        );
        assert_eq!(
            desk.control(phone, ZoneId::FIRST, RemoteCommand::Remove(0)),
            Controlling::Untrusted
        );
        eventually("the jump reached the engine", || {
            player
                .playing
                .player()
                .is_ok_and(|player| player.state().queue_position == Some(1))
        });
        assert_eq!(player.queue().expect("the queue").len(), 2);

        drop(desk);
        player.sync_stop();
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_mirrored_zone_reaches_the_phones_with_its_place_played_on_and_its_rows_numbered() {
        let heard_at = Instant::now();
        let zone = RemoteZone {
            zone: ZoneId::FIRST,
            name: "Living room".into(),
            playback: RemotePlayback::Playing,
            playing: Some(3),
            position: Duration::from_secs(10),
            length: Some(Duration::from_secs(240)),
            volume: Permille::new(700),
            queue_length: 9,
            first_row: 2,
            rows: vec![RemoteRow {
                title: "Echoes".into(),
                artist: "Pink Floyd".into(),
                length: None,
            }],
            queue_revision: Some(4),
        };
        let watched = Watched::Mirrored(Mirror {
            zones: vec![zone],
            heard_at,
        });

        let shown = device_of(&watched, heard_at + Duration::from_secs(2));

        assert_eq!(shown.standing, RemoteStanding::Mirrored);
        assert_eq!(shown.zones[0].position_millis, 12_000);
        assert_eq!(shown.zones[0].playing_row, Some(3));
        assert_eq!(shown.zones[0].first_row, 2);
        assert_eq!(shown.zones[0].queue_revision, Some(4));
        assert_eq!(shown.zones[0].rows[0].title, "Echoes");
        assert_eq!(
            command_of(RemoteControl::Move { from: 4, to: 1 }),
            RemoteCommand::Move { from: 4, to: 1 }
        );
    }
}
