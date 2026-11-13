use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::{Condvar, Mutex};
use resonate_musicbrainz::{
    Mirror, Mirrored, mirror_in, received_amendments, received_copy, removed_copy,
};
use resonate_sync::{
    AskedForExport, DeviceId, ExportFetched, ExportHeld, ExportPart, ExportReceiving, ExportWanted,
};

use super::Running;
use crate::{Error, Result};

const COPY_RECEIVED: &str = "musicbrainz.sqlite.received";
const COPY_NOTED: &str = "musicbrainz.sqlite.received-from";
const AMENDMENTS_RECEIVED: &str = "musicbrainz.amended.sqlite.received";
const AMENDMENTS_NOTED: &str = "musicbrainz.amended.sqlite.received-from";
const ROOM_A_COPY_TAKES: u64 = 12_000_000_000;
const ROOM_LEFT_AT_LEAST: u64 = 2_000_000_000;
const LOOKED_AT_AGAIN_AFTER: Duration = Duration::from_secs(5 * 60);
const AMENDMENTS_ASKED_EVERY: Duration = Duration::from_secs(60 * 60);
const WAITED_ON_AN_ANSWER: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MusicbrainzCopyStanding {
    Off,
    WaitingForTransfers,
    Looking,
    NoDeviceHoldsOne,
    Receiving,
    Held,
    NoRoom,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MusicbrainzCopy {
    pub standing: MusicbrainzCopyStanding,
    pub dump: Option<String>,
    pub bytes: u64,
    pub amended_to: Option<u64>,
    pub amended_at: Option<u64>,
    pub received: u64,
    pub of: u64,
}

pub(crate) struct Copied {
    folder: PathBuf,
    pub(crate) mirrored: Arc<Mirrored>,
    keeping: AtomicBool,
    standing: Mutex<MusicbrainzCopyStanding>,
    receiving: Arc<ExportReceiving>,
    outstanding: Mutex<Option<AskedForExport>>,
    looking: Mutex<bool>,
    woken: Condvar,
}

pub(crate) struct Reaching {
    pub(crate) running: Weak<Mutex<Option<Running>>>,
    pub(crate) unmetered: Arc<AtomicBool>,
}

impl Copied {
    pub(crate) fn under(folder: PathBuf) -> Self {
        let mirrored = Arc::new(Mirrored::default());
        let path = mirror_in(&folder);
        if path.is_file() {
            match Mirror::open(&path) {
                Ok(mirror) => mirrored.hold(Some(Arc::new(mirror))),
                Err(error) => {
                    tracing::warn!(%error, "the copy of MusicBrainz kept here could not be opened")
                }
            }
        }
        Self {
            folder,
            mirrored,
            keeping: AtomicBool::new(false),
            standing: Mutex::new(MusicbrainzCopyStanding::Off),
            receiving: Arc::new(ExportReceiving::default()),
            outstanding: Mutex::new(None),
            looking: Mutex::new(false),
            woken: Condvar::new(),
        }
    }

    pub(crate) fn keep(self: &Arc<Self>, on: bool, reaching: Reaching) {
        let mut looking = self.looking.lock();
        self.keeping.store(on, Ordering::Release);
        if !on {
            *self.standing.lock() = MusicbrainzCopyStanding::Off;
            self.woken.notify_all();
            return;
        }
        if *looking {
            self.woken.notify_all();
            return;
        }
        *looking = true;
        drop(looking);
        let copied = Arc::clone(self);
        let spawned = thread::Builder::new()
            .name("resonate-musicbrainz-copy".to_owned())
            .spawn(move || {
                loop {
                    copied.follow(&reaching);
                    let mut looking = copied.looking.lock();
                    if !copied.keeps() || reaching.running.strong_count() == 0 {
                        *looking = false;
                        return;
                    }
                }
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "the copy of MusicBrainz could not begin to be kept");
            *self.looking.lock() = false;
        }
    }

    pub(crate) fn wake(&self) {
        self.woken.notify_all();
    }

    pub(crate) fn told(&self) -> MusicbrainzCopy {
        let held = self.mirrored.held();
        let (received, of) = self.receiving.so_far();
        let standing = *self.standing.lock();
        MusicbrainzCopy {
            standing,
            dump: held.as_ref().map(|mirror| mirror.made().dump.clone()),
            bytes: held
                .as_ref()
                .and_then(|mirror| std::fs::metadata(mirror.path()).ok())
                .map_or(0, |metadata| metadata.len()),
            amended_to: held
                .as_ref()
                .and_then(|mirror| mirror.amended_to().map(|to| to.sequence)),
            amended_at: held
                .as_ref()
                .and_then(|mirror| mirror.amended_to().map(|to| to.at)),
            received: if standing == MusicbrainzCopyStanding::Receiving {
                received
            } else {
                0
            },
            of: if standing == MusicbrainzCopyStanding::Receiving {
                of
            } else {
                0
            },
        }
    }

    pub(crate) fn remove(&self) -> Result<()> {
        self.mirrored.hold(None);
        for staged in [
            COPY_RECEIVED,
            COPY_NOTED,
            AMENDMENTS_RECEIVED,
            AMENDMENTS_NOTED,
        ] {
            match std::fs::remove_file(self.folder.join(staged)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(%error, "a part of MusicBrainz's copy received could not be removed");
                }
            }
        }
        removed_copy(&self.folder).map_err(|source| Error::MusicbrainzCopyNotRemoved {
            source: Box::new(source),
        })
    }

    fn keeps(&self) -> bool {
        self.keeping.load(Ordering::Acquire)
    }

    fn rest(&self, for_as_long: Duration) {
        let mut looking = self.looking.lock();
        if self.keeps() {
            let _ = self.woken.wait_for(&mut looking, for_as_long);
        }
    }

    fn stand(&self, standing: MusicbrainzCopyStanding) {
        if self.keeps() {
            *self.standing.lock() = standing;
        }
    }

    fn follow(&self, reaching: &Reaching) {
        let mut amendments_asked: Option<Instant> = None;
        while self.keeps() {
            let Some(running) = reaching.running.upgrade() else {
                return;
            };
            let waited = self.waited_on_the_last_ask();
            if !waited {
                self.rest(LOOKED_AT_AGAIN_AFTER);
                continue;
            }
            let devices = {
                let guard = running.lock();
                guard.as_ref().and_then(|held| {
                    let allowed =
                        reaching.unmetered.load(Ordering::Acquire) || held.metered_allowed;
                    allowed.then(|| devices_online(held))
                })
            };
            drop(running);
            let Some(devices) = devices else {
                self.stand(MusicbrainzCopyStanding::WaitingForTransfers);
                self.rest(LOOKED_AT_AGAIN_AFTER);
                continue;
            };
            let due_for_amendments =
                amendments_asked.is_none_or(|asked| asked.elapsed() >= AMENDMENTS_ASKED_EVERY);
            match self.looked_for(reaching, &devices, due_for_amendments) {
                Looked::Current => {
                    amendments_asked = Some(Instant::now());
                    self.stand(MusicbrainzCopyStanding::Held);
                    self.rest(AMENDMENTS_ASKED_EVERY);
                }
                Looked::Unheld => {
                    self.stand(if self.mirrored.held().is_some() {
                        MusicbrainzCopyStanding::Held
                    } else {
                        MusicbrainzCopyStanding::NoDeviceHoldsOne
                    });
                    self.rest(LOOKED_AT_AGAIN_AFTER);
                }
                Looked::NoRoom => {
                    self.stand(MusicbrainzCopyStanding::NoRoom);
                    self.rest(LOOKED_AT_AGAIN_AFTER);
                }
                Looked::Stopped => {}
            }
        }
    }

    fn waited_on_the_last_ask(&self) -> bool {
        let mut outstanding = self.outstanding.lock();
        let Some(asked) = outstanding.as_ref() else {
            return true;
        };
        let answered = asked.answer_within(WAITED_ON_AN_ANSWER).is_some();
        if answered {
            *outstanding = None;
        }
        answered
    }

    fn looked_for(&self, reaching: &Reaching, devices: &[DeviceId], amendments: bool) -> Looked {
        for device in devices {
            let held = self.mirrored.held().map(|mirror| ExportHeld {
                dump: mirror.made().dump.clone(),
                sequence: 0,
            });
            if held.is_none() && !self.has_room() {
                return Looked::NoRoom;
            }
            self.stand(MusicbrainzCopyStanding::Looking);
            let fetched = self.asked(reaching, *device, ExportPart::Copy, held);
            match fetched {
                Some(ExportFetched::Whole(_)) => {
                    let staged = self.folder.join(COPY_RECEIVED);
                    if let Err(error) = received_copy(&self.folder, &staged) {
                        tracing::warn!(%error, "the copy of MusicBrainz received could not be kept");
                        let _ = std::fs::remove_file(&staged);
                        continue;
                    }
                    self.reopened();
                    self.amended_from(reaching, *device);
                    return Looked::Current;
                }
                Some(ExportFetched::AlreadyHeld) => {
                    if amendments {
                        self.amended_from(reaching, *device);
                    }
                    return Looked::Current;
                }
                Some(ExportFetched::NotHeld | ExportFetched::Failed) => {}
                None => return Looked::Stopped,
            }
        }
        Looked::Unheld
    }

    fn amended_from(&self, reaching: &Reaching, device: DeviceId) {
        let held = self.mirrored.held().and_then(|mirror| {
            mirror.amended_to().map(|to| ExportHeld {
                dump: to.dump.clone(),
                sequence: to.sequence,
            })
        });
        let Some(ExportFetched::Whole(_)) =
            self.asked(reaching, device, ExportPart::Amendments, held)
        else {
            return;
        };
        let staged = self.folder.join(AMENDMENTS_RECEIVED);
        match received_amendments(&self.folder, &staged) {
            Ok(()) => self.reopened(),
            Err(error) => {
                tracing::warn!(%error, "the changes MusicBrainz's copy follows could not be kept");
                let _ = std::fs::remove_file(&staged);
            }
        }
    }

    fn asked(
        &self,
        reaching: &Reaching,
        device: DeviceId,
        part: ExportPart,
        held: Option<ExportHeld>,
    ) -> Option<ExportFetched> {
        let (staged, noted) = match part {
            ExportPart::Copy => (COPY_RECEIVED, COPY_NOTED),
            ExportPart::Amendments => (AMENDMENTS_RECEIVED, AMENDMENTS_NOTED),
        };
        let running = reaching.running.upgrade()?;
        let asked = {
            let guard = running.lock();
            let held_sync = guard.as_ref()?;
            held_sync.sync.ask_for_export(
                device,
                ExportWanted {
                    part,
                    held,
                    staged: self.folder.join(staged),
                    noted: self.folder.join(noted),
                },
                Arc::clone(&self.receiving),
            )
        };
        drop(running);
        if part == ExportPart::Copy {
            self.stand(MusicbrainzCopyStanding::Receiving);
        }
        loop {
            if let Some(fetched) = asked.answer_within(WAITED_ON_AN_ANSWER) {
                return Some(fetched);
            }
            if !self.keeps() {
                *self.outstanding.lock() = Some(asked);
                return None;
            }
        }
    }

    fn reopened(&self) {
        match Mirror::open(&mirror_in(&self.folder)) {
            Ok(mirror) => self.mirrored.hold(Some(Arc::new(mirror))),
            Err(error) => {
                tracing::warn!(%error, "the copy of MusicBrainz received could not be opened");
            }
        }
    }

    fn has_room(&self) -> bool {
        let partial =
            std::fs::metadata(self.folder.join(COPY_RECEIVED)).map_or(0, |metadata| metadata.len());
        let wanted = ROOM_A_COPY_TAKES.saturating_sub(partial) + ROOM_LEFT_AT_LEAST;
        room_in(&self.folder).is_none_or(|room| room >= wanted)
    }
}

enum Looked {
    Current,
    Unheld,
    NoRoom,
    Stopped,
}

fn devices_online(running: &Running) -> Vec<DeviceId> {
    let mut online = running
        .sync
        .shown()
        .devices
        .into_iter()
        .filter(|device| device.online && !device.this)
        .collect::<Vec<_>>();
    online.sort_by_key(|device| !device.nearby);
    online.into_iter().map(|device| device.id).collect()
}

fn room_in(folder: &Path) -> Option<u64> {
    let _ = std::fs::create_dir_all(folder);
    fs4::available_space(folder).ok()
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        io::Cursor,
        net::{Ipv4Addr, SocketAddr},
        process,
    };

    use resonate_sync::{
        Broadcast, Consultable, Follow, KeptAnswer, Me, Nearby, OfferedExport, Platform, Server,
        Settings, Sync, SyncKey,
    };
    use rusqlite::Connection;

    use super::*;
    use crate::{
        harness::Harness,
        sync::{
            SyncConfig,
            driving::{
                SyncControl,
                tests::{NothingHeld, eventually},
            },
        },
    };

    const DUMP: &str = "20261111-002335";

    struct OffersACopy(Vec<u8>);

    impl Consultable for OffersACopy {
        fn consult(&self, _: &[u8]) -> Option<Vec<u8>> {
            None
        }

        fn kept_after(&self, _: u64, _: usize, _: DeviceId) -> Vec<KeptAnswer> {
            Vec::new()
        }

        fn kept_mark(&self, _: DeviceId) -> u64 {
            0
        }

        fn take_kept(&self, _: DeviceId, _: Vec<KeptAnswer>, _: u64) {}

        fn export(&self, part: ExportPart) -> Option<OfferedExport> {
            (part == ExportPart::Copy).then(|| OfferedExport {
                dump: DUMP.to_owned(),
                sequence: 0,
                size: self.0.len() as u64,
                reader: Box::new(Cursor::new(self.0.clone())),
            })
        }
    }

    fn a_copy_of_nothing(folder: &Path) -> Vec<u8> {
        let path = folder.join("desk-musicbrainz.sqlite");
        let connection = Connection::open(&path).expect("a copy");
        connection
            .execute_batch(&format!(
                "CREATE TABLE made (format INTEGER, dump TEXT, built INTEGER);
                 CREATE TABLE dictionaries (kind INTEGER, bytes BLOB);
                 INSERT INTO made VALUES ({}, '{DUMP}', 1);",
                resonate_musicbrainz::FORMAT
            ))
            .expect("the copy laid out");
        drop(connection);
        fs::read(&path).expect("the copy read")
    }

    #[test]
    fn a_phone_keeping_a_copy_fetches_it_from_a_desk_and_serves_lookups_from_it() {
        let harness = Harness::new();
        let player = &harness.player;
        let folder = env::temp_dir().join(format!("resonate-copied-phone-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a test folder");
        let key = SyncKey::minted().expect("a key");
        let copy = a_copy_of_nothing(&folder);

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
                control: SyncControl::Everything,
                stun: String::new(),
                certificate: String::new(),
                musicbrainz_copy: true,
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
        desk.consult_with(Arc::new(OffersACopy(copy.clone())));
        player.syncing.copied.wake();

        eventually("the copy kept on the phone", || {
            player.syncing.copied.wake();
            player.musicbrainz_copy().standing == MusicbrainzCopyStanding::Held
        });
        let told = player.musicbrainz_copy();
        let kept = fs::read(mirror_in(&harness.folder.join("musicbrainz"))).expect("the copy");

        assert_eq!(told.dump.as_deref(), Some(DUMP));
        assert_eq!(told.bytes, copy.len() as u64);
        assert_eq!(kept, copy);
        assert!(player.syncing.copied.mirrored.held().is_some());
        assert!(
            !harness
                .folder
                .join("musicbrainz")
                .join(COPY_RECEIVED)
                .exists()
        );

        player.sync_stop();
        player.remove_musicbrainz_copy().expect("the copy removed");
        let removed = player.musicbrainz_copy();

        assert_eq!(removed.standing, MusicbrainzCopyStanding::Off);
        assert_eq!(removed.dump, None);
        assert!(player.syncing.copied.mirrored.held().is_none());

        drop(desk);
        let _ = fs::remove_dir_all(&folder);
    }
}
