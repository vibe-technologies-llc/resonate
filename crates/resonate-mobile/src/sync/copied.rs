use std::{
    io,
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
    Mirror, Mirrored, exported_after, mirror_in, received_amendments, received_copy, removed_copy,
};
use resonate_sync::{
    AskedForExport, DeviceId, ExportDeclined, ExportFetched, ExportHeld, ExportPart,
    ExportReceiving, ExportWanted,
};

use super::Running;
use crate::{Error, Result};

const COPY_RECEIVED: &str = "musicbrainz.sqlite.received";
const COPY_NOTED: &str = "musicbrainz.sqlite.received-from";
const AMENDMENTS_RECEIVED: &str = "musicbrainz.amended.sqlite.received";
const AMENDMENTS_NOTED: &str = "musicbrainz.amended.sqlite.received-from";
const REMOVED_BY_THE_LISTENER: &str = "musicbrainz.removed";
const ROOM_A_COPY_TAKES: u64 = 12_000_000_000;
const ROOM_LEFT_AT_LEAST: u64 = 2_000_000_000;
const LOOKED_AT_AGAIN_AFTER: Duration = Duration::from_secs(5 * 60);
const AMENDMENTS_ASKED_EVERY: Duration = Duration::from_secs(60 * 60);
const WAITED_ON_AN_ANSWER: Duration = Duration::from_secs(1);
const A_RECEIVE_CALLED_OFF_SETTLES_WITHIN: Duration = Duration::from_secs(10);

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
    removed: AtomicBool,
    standing: Mutex<MusicbrainzCopyStanding>,
    receiving: Mutex<Arc<ExportReceiving>>,
    outstanding: Mutex<Option<AskedForExport>>,
    settling: Mutex<()>,
    looking: Mutex<bool>,
    woken: Condvar,
    room_of: Mutex<RoomOf>,
}

type RoomOf = fn(&Path) -> Option<u64>;

pub(crate) struct Reaching {
    pub(crate) running: Weak<Mutex<Option<Running>>>,
    pub(crate) unmetered: Arc<AtomicBool>,
}

enum Settled {
    Answered(ExportFetched),
    Waiting,
    Idle,
}

impl Copied {
    pub(crate) fn under(folder: PathBuf) -> Self {
        let mirrored = Arc::new(Mirrored::default());
        let path = mirror_in(&folder);
        let removed = folder.join(REMOVED_BY_THE_LISTENER).is_file();

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
            removed: AtomicBool::new(removed),
            standing: Mutex::new(MusicbrainzCopyStanding::Off),
            receiving: Mutex::new(Arc::new(ExportReceiving::default())),
            outstanding: Mutex::new(None),
            settling: Mutex::new(()),
            looking: Mutex::new(false),
            woken: Condvar::new(),
            room_of: Mutex::new(room_in),
        }
    }

    #[cfg(test)]
    pub(crate) fn measures_room_by(&self, room_of: RoomOf) {
        *self.room_of.lock() = room_of;
    }

    pub(crate) fn keep(self: &Arc<Self>, on: bool, reaching: Reaching) {
        let mut looking = self.looking.lock();

        self.keeping.store(on, Ordering::Release);
        if !self.keeps() {
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

    pub(crate) fn forget_removal(&self) {
        let _settling = self.settling.lock();

        self.removed.store(false, Ordering::Release);
        match std::fs::remove_file(self.folder.join(REMOVED_BY_THE_LISTENER)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, "the removal of MusicBrainz's copy could not be forgotten");
            }
        }
    }

    pub(crate) fn wake(&self) {
        self.woken.notify_all();
    }

    pub(crate) fn told(&self) -> MusicbrainzCopy {
        let held = self.mirrored.held();
        let (received, of) = self.receiving.lock().so_far();
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
        let _settling = self.settling.lock();

        self.removed.store(true, Ordering::Release);
        *self.standing.lock() = MusicbrainzCopyStanding::Off;
        self.receiving.lock().call_off();
        {
            let _looking = self.looking.lock();

            self.woken.notify_all();
        }

        let remembered = std::fs::create_dir_all(&self.folder)
            .and_then(|()| std::fs::write(self.folder.join(REMOVED_BY_THE_LISTENER), []));

        if !self.settled_by(Instant::now() + A_RECEIVE_CALLED_OFF_SETTLES_WITHIN) {
            tracing::warn!("a receive of MusicBrainz's copy called off had not settled");
        }
        self.mirrored.hold(None);
        for staged in [
            COPY_RECEIVED,
            COPY_NOTED,
            AMENDMENTS_RECEIVED,
            AMENDMENTS_NOTED,
        ] {
            match std::fs::remove_file(self.folder.join(staged)) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(%error, "a part of MusicBrainz's copy received could not be removed");
                }
            }
        }
        removed_copy(&self.folder).map_err(|source| Error::MusicbrainzCopyNotRemoved {
            source: Box::new(source),
        })?;
        remembered.map_err(|source| Error::MusicbrainzCopyRemovalNotKept { source })
    }

    fn keeps(&self) -> bool {
        self.keeping.load(Ordering::Acquire) && !self.removed.load(Ordering::Acquire)
    }

    fn rest(&self, for_as_long: Duration) {
        let mut looking = self.looking.lock();
        if self.keeps() {
            let _ = self.woken.wait_for(&mut looking, for_as_long);
        }
    }

    fn stand(&self, standing: MusicbrainzCopyStanding) {
        let mut stood = self.standing.lock();

        if self.keeps() {
            *stood = standing;
        }
    }

    fn was_removed(&self) -> bool {
        self.removed.load(Ordering::Acquire)
    }

    fn follow(&self, reaching: &Reaching) {
        let mut amendments = AmendmentTimer::default();
        while self.keeps() {
            let Some(running) = reaching.running.upgrade() else {
                return;
            };
            let waited = !matches!(self.settled_within(WAITED_ON_AN_ANSWER), Settled::Waiting);

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
            let due_for_amendments = amendments.due(Instant::now());
            match self.looked_for(reaching, &devices, due_for_amendments) {
                Looked::Current { amendments_asked } => {
                    if amendments_asked {
                        amendments.asked(Instant::now());
                    }
                    self.stand(MusicbrainzCopyStanding::Held);
                    self.rest(amendments.remaining(Instant::now()));
                }
                Looked::Unheld => {
                    self.stand(self.held_or(MusicbrainzCopyStanding::NoDeviceHoldsOne));
                    self.rest(LOOKED_AT_AGAIN_AFTER);
                }
                Looked::NoRoom => {
                    self.stand(self.held_or(MusicbrainzCopyStanding::NoRoom));
                    self.rest(LOOKED_AT_AGAIN_AFTER);
                }
                Looked::Stopped => {}
            }
        }
    }

    fn held_or(&self, unheld: MusicbrainzCopyStanding) -> MusicbrainzCopyStanding {
        if self.mirrored.held().is_some() {
            MusicbrainzCopyStanding::Held
        } else {
            unheld
        }
    }

    fn settled_within(&self, within: Duration) -> Settled {
        let mut outstanding = self.outstanding.lock();

        let Some(asked) = outstanding.as_ref() else {
            return Settled::Idle;
        };
        let Some(fetched) = asked.answer_within(within) else {
            return Settled::Waiting;
        };

        *outstanding = None;
        Settled::Answered(fetched)
    }

    fn settled_by(&self, deadline: Instant) -> bool {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.settled_within(left.min(WAITED_ON_AN_ANSWER)) {
                Settled::Answered(_) | Settled::Idle => return true,
                Settled::Waiting if left.is_zero() => return false,
                Settled::Waiting => {}
            }
        }
    }

    fn looked_for(&self, reaching: &Reaching, devices: &[DeviceId], amendments: bool) -> Looked {
        let mut no_room = false;

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
                    let settling = self.settling.lock();

                    if self.was_removed() {
                        return Looked::Stopped;
                    }
                    if let Err(error) = received_copy(&self.folder, &staged) {
                        tracing::warn!(%error, "the copy of MusicBrainz received could not be kept");

                        let _ = std::fs::remove_file(&staged);

                        continue;
                    }
                    self.reopened();
                    drop(settling);
                    self.amended_from(reaching, *device);
                    return Looked::Current {
                        amendments_asked: true,
                    };
                }
                Some(ExportFetched::AlreadyHeld) => {
                    if amendments {
                        self.amended_from(reaching, *device);
                    }
                    return Looked::Current {
                        amendments_asked: amendments,
                    };
                }
                Some(ExportFetched::Declined(ExportDeclined::NoRoom { .. })) => no_room = true,
                Some(
                    ExportFetched::Declined(ExportDeclined::NotNewer)
                    | ExportFetched::NotHeld
                    | ExportFetched::Failed,
                ) => {}
                Some(ExportFetched::CalledOff) | None => return Looked::Stopped,
            }
        }
        if no_room {
            Looked::NoRoom
        } else {
            Looked::Unheld
        }
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
        let _settling = self.settling.lock();

        if self.was_removed() {
            return;
        }
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

        {
            let _settling = self.settling.lock();

            if !self.keeps() {
                return None;
            }

            let receiving = Arc::new(ExportReceiving::judged_by(self.judge(part, held.clone())));
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
                    Arc::clone(&receiving),
                )
            };

            *self.receiving.lock() = receiving;
            *self.outstanding.lock() = Some(asked);
        }

        drop(running);
        if part == ExportPart::Copy {
            self.stand(MusicbrainzCopyStanding::Receiving);
        }
        loop {
            match self.settled_within(WAITED_ON_AN_ANSWER) {
                Settled::Answered(fetched) => return Some(fetched),
                Settled::Idle => return None,
                Settled::Waiting if !self.keeps() => return None,
                Settled::Waiting => {}
            }
        }
    }

    fn judge(
        &self,
        part: ExportPart,
        held: Option<ExportHeld>,
    ) -> impl Fn(&ExportHeld, u64) -> std::result::Result<(), ExportDeclined> + Send + Sync + 'static
    {
        let folder = self.folder.clone();
        let room_of = *self.room_of.lock();

        move |offered, remaining| {
            let newer = held.as_ref().is_none_or(|held| match part {
                ExportPart::Copy => exported_after(&offered.dump, &held.dump),
                ExportPart::Amendments => {
                    offered.dump != held.dump || offered.sequence > held.sequence
                }
            });

            if !newer {
                return Err(ExportDeclined::NotNewer);
            }

            let wanted = remaining.saturating_add(ROOM_LEFT_AT_LEAST);

            if room_of(&folder).is_some_and(|room| room < wanted) {
                return Err(ExportDeclined::NoRoom { remaining });
            }
            Ok(())
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
        let room_of = *self.room_of.lock();
        room_of(&self.folder).is_none_or(|room| room >= wanted)
    }
}

#[derive(Default)]
struct AmendmentTimer {
    last_asked: Option<Instant>,
}

impl AmendmentTimer {
    fn due(&self, now: Instant) -> bool {
        self.remaining(now).is_zero()
    }

    fn asked(&mut self, now: Instant) {
        self.last_asked = Some(now);
    }

    fn remaining(&self, now: Instant) -> Duration {
        self.last_asked.map_or(Duration::ZERO, |asked| {
            AMENDMENTS_ASKED_EVERY.saturating_sub(now.saturating_duration_since(asked))
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Looked {
    Current { amendments_asked: bool },
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
        io::{Cursor, Read, Seek, SeekFrom},
        net::{Ipv4Addr, SocketAddr},
        process,
        sync::{
            atomic::AtomicUsize,
            mpsc::{self as std_mpsc, Receiver, Sender},
        },
    };

    use resonate_sync::{
        Broadcast, Consultable, ConsultedAnswer, Follow, KeptAnswer, Me, Nearby, OfferedExport,
        Platform, Server, Settings, Sync, SyncKey,
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
    const EARLIER_DUMP: &str = "20261104-002222";
    const RECEIVED_BEFORE_THE_GATE: u64 = 4096;
    const LOOKED_AT_FOR: Duration = Duration::from_secs(2);

    struct OffersACopy {
        dump: &'static str,
        copy: Vec<u8>,
        gate: Arc<Mutex<Option<Receiver<()>>>>,
        asked: AtomicUsize,
    }

    impl Consultable for OffersACopy {
        fn consult(&self, _: &[u8]) -> Option<ConsultedAnswer> {
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
            (part == ExportPart::Copy).then(|| {
                self.asked.fetch_add(1, Ordering::AcqRel);
                OfferedExport {
                    dump: self.dump.to_owned(),
                    sequence: 0,
                    size: self.copy.len() as u64,
                    reader: Box::new(Gated {
                        inner: Cursor::new(self.copy.clone()),
                        gate: Arc::clone(&self.gate),
                    }),
                }
            })
        }
    }

    struct Gated {
        inner: Cursor<Vec<u8>>,
        gate: Arc<Mutex<Option<Receiver<()>>>>,
    }

    impl Read for Gated {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let at = self.inner.position();

            if at < RECEIVED_BEFORE_THE_GATE {
                let most = usize::try_from(RECEIVED_BEFORE_THE_GATE - at)
                    .unwrap_or(usize::MAX)
                    .min(buffer.len());

                return self.inner.read(&mut buffer[..most]);
            }

            let mut gate = self.gate.lock();

            if gate.as_ref().is_some_and(|waiting| waiting.recv().is_err()) {
                *gate = None;
            }
            drop(gate);
            self.inner.read(buffer)
        }
    }

    impl Seek for Gated {
        fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
            self.inner.seek(to)
        }
    }

    fn a_copy_of(folder: &Path, dump: &str) -> Vec<u8> {
        let path = folder.join(format!("{dump}-musicbrainz.sqlite"));
        let connection = Connection::open(&path).expect("a copy");

        connection
            .execute_batch(&format!(
                "CREATE TABLE made (format INTEGER, dump TEXT, built INTEGER);
                 CREATE TABLE dictionaries (kind INTEGER, bytes BLOB);
                 INSERT INTO made VALUES ({}, '{dump}', 1);",
                resonate_musicbrainz::FORMAT
            ))
            .expect("the copy laid out");
        drop(connection);
        fs::read(&path).expect("the copy read")
    }

    fn roomy(_: &Path) -> Option<u64> {
        Some(u64::MAX)
    }

    fn full(_: &Path) -> Option<u64> {
        Some(ROOM_LEFT_AT_LEAST)
    }

    struct Offer {
        dump: &'static str,
        gated: bool,
    }

    struct Phone {
        holds: Option<&'static str>,
        room_of: RoomOf,
        keeps: bool,
    }

    struct Offered {
        harness: Harness,
        gate: Option<Sender<()>>,
        _desk: Sync,
        folder: PathBuf,
        copy: Vec<u8>,
        held_before: Option<Vec<u8>>,
        offers: Arc<OffersACopy>,
    }

    impl Offered {
        fn kept_here(&self) -> PathBuf {
            self.harness.folder.join("musicbrainz")
        }

        fn devices_met(&self) -> Vec<DeviceId> {
            let player = &self.harness.player;

            eventually("the phone meeting the desk", || {
                let running = player.syncing.running.lock();
                running
                    .as_ref()
                    .is_some_and(|running| !devices_online(running).is_empty())
            });

            let running = player.syncing.running.lock();
            devices_online(running.as_ref().expect("running"))
        }

        fn held_dump(&self) -> Option<String> {
            self.harness
                .player
                .syncing
                .copied
                .mirrored
                .held()
                .map(|mirror| mirror.made().dump.clone())
        }
    }

    impl Drop for Offered {
        fn drop(&mut self) {
            self.gate = None;

            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    fn a_desk_offers(offer: Offer, phone: Phone, name: &str) -> Offered {
        let harness = Harness::new();
        let player = &harness.player;
        let folder = env::temp_dir().join(format!("resonate-copied-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);

        fs::create_dir_all(&folder).expect("a test folder");

        let key = SyncKey::minted().expect("a key");
        let copy = a_copy_of(&folder, offer.dump);
        let (gate, gated) = std_mpsc::channel();

        let offers = Arc::new(OffersACopy {
            dump: offer.dump,
            copy: copy.clone(),
            gate: Arc::new(Mutex::new(offer.gated.then_some(gated))),
            asked: AtomicUsize::new(0),
        });
        let kept_here = harness.folder.join("musicbrainz");
        let held_before = phone.holds.map(|dump| a_copy_of(&folder, dump));

        if let Some(held) = &held_before {
            fs::create_dir_all(&kept_here).expect("the phone's folder");
            fs::write(mirror_in(&kept_here), held).expect("the phone's copy");
            player.syncing.copied.reopened();
        }
        player.syncing.copied.measures_room_by(phone.room_of);
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
                musicbrainz_copy: phone.keeps,
                takes_deletions: true,
            })
            .expect("sync started");
        if !phone.keeps {
            player.syncing.copied.keeping.store(true, Ordering::Release);
        }

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

        desk.consult_with(Arc::clone(&offers) as Arc<dyn Consultable>);
        player.syncing.copied.wake();

        Offered {
            harness,
            gate: Some(gate),
            _desk: desk,
            folder,
            copy,
            held_before,
            offers,
        }
    }

    fn kept_by_a_phone_with(room_of: RoomOf) -> Phone {
        Phone {
            holds: None,
            room_of,
            keeps: true,
        }
    }

    fn looked_at_by_a_phone_holding(dump: &'static str, room_of: RoomOf) -> Phone {
        Phone {
            holds: Some(dump),
            room_of,
            keeps: false,
        }
    }

    fn whole(dump: &'static str) -> Offer {
        Offer { dump, gated: false }
    }

    #[test]
    fn a_phone_keeping_a_copy_fetches_it_from_a_desk_and_serves_lookups_from_it() {
        let offered = a_desk_offers(whole(DUMP), kept_by_a_phone_with(roomy), "roomy");
        let player = &offered.harness.player;

        eventually("the copy kept on the phone", || {
            player.syncing.copied.wake();
            player.musicbrainz_copy().standing == MusicbrainzCopyStanding::Held
        });

        let told = player.musicbrainz_copy();
        let kept = fs::read(mirror_in(&offered.kept_here())).expect("the copy");

        assert_eq!(told.dump.as_deref(), Some(DUMP));
        assert_eq!(told.bytes, offered.copy.len() as u64);
        assert_eq!(kept, offered.copy);
        assert!(player.syncing.copied.mirrored.held().is_some());
        assert!(!offered.kept_here().join(COPY_RECEIVED).exists());

        player.sync_stop();
        player.remove_musicbrainz_copy().expect("the copy removed");

        let removed = player.musicbrainz_copy();

        assert_eq!(removed.standing, MusicbrainzCopyStanding::Off);
        assert_eq!(removed.dump, None);
        assert!(player.syncing.copied.mirrored.held().is_none());
    }

    #[test]
    fn a_phone_without_room_for_a_copy_asks_no_desk_for_one() {
        let offered = a_desk_offers(whole(DUMP), kept_by_a_phone_with(full), "full");
        let player = &offered.harness.player;

        eventually("the phone to say it has no room", || {
            player.syncing.copied.wake();
            player.musicbrainz_copy().standing == MusicbrainzCopyStanding::NoRoom
        });

        assert!(player.syncing.copied.mirrored.held().is_none());
        assert!(!offered.kept_here().join(COPY_RECEIVED).exists());

        player.sync_stop();
    }

    #[test]
    fn a_look_not_yet_due_for_amendments_asks_for_none_and_one_due_asks() {
        let offered = a_desk_offers(whole(DUMP), kept_by_a_phone_with(roomy), "amending");
        let player = &offered.harness.player;

        eventually("the copy kept on the phone", || {
            player.syncing.copied.wake();
            player.musicbrainz_copy().standing == MusicbrainzCopyStanding::Held
        });

        let reaching = player.copy_reaching();
        let devices = offered.devices_met();

        let hurried = player.syncing.copied.looked_for(&reaching, &devices, false);
        let due = player.syncing.copied.looked_for(&reaching, &devices, true);

        player.sync_stop();

        assert!(!devices.is_empty());
        assert_eq!(
            hurried,
            Looked::Current {
                amendments_asked: false
            }
        );
        assert_eq!(
            due,
            Looked::Current {
                amendments_asked: true
            }
        );
    }

    #[test]
    fn an_earlier_export_offered_leaves_the_copy_held_as_it_was() {
        let offered = a_desk_offers(
            whole(EARLIER_DUMP),
            looked_at_by_a_phone_holding(DUMP, roomy),
            "earlier",
        );
        let player = &offered.harness.player;
        let reaching = player.copy_reaching();
        let devices = offered.devices_met();

        let looked = player.syncing.copied.looked_for(&reaching, &devices, false);
        let kept = fs::read(mirror_in(&offered.kept_here())).expect("the copy held");

        player.sync_stop();

        assert_eq!(looked, Looked::Unheld);
        assert_eq!(offered.offers.asked.load(Ordering::Acquire), 1);
        assert_eq!(offered.held_dump().as_deref(), Some(DUMP));
        assert_eq!(Some(kept), offered.held_before.clone());
        assert!(!offered.kept_here().join(COPY_RECEIVED).exists());
        assert!(!offered.kept_here().join(COPY_NOTED).exists());
    }

    #[test]
    fn a_later_export_with_no_room_beside_the_copy_leaves_the_copy_held_as_it_was() {
        let offered = a_desk_offers(
            whole(DUMP),
            looked_at_by_a_phone_holding(EARLIER_DUMP, full),
            "later-full",
        );
        let player = &offered.harness.player;
        let reaching = player.copy_reaching();
        let devices = offered.devices_met();

        let looked = player.syncing.copied.looked_for(&reaching, &devices, false);
        let kept = fs::read(mirror_in(&offered.kept_here())).expect("the copy held");

        player.sync_stop();

        assert_eq!(looked, Looked::NoRoom);
        assert_eq!(offered.offers.asked.load(Ordering::Acquire), 1);
        assert_eq!(offered.held_dump().as_deref(), Some(EARLIER_DUMP));
        assert_eq!(Some(kept), offered.held_before.clone());
        assert!(!offered.kept_here().join(COPY_RECEIVED).exists());
        assert!(!offered.kept_here().join(COPY_NOTED).exists());
    }

    #[test]
    fn a_later_export_with_room_beside_the_copy_replaces_it() {
        let offered = a_desk_offers(
            whole(DUMP),
            looked_at_by_a_phone_holding(EARLIER_DUMP, roomy),
            "later-roomy",
        );
        let player = &offered.harness.player;
        let reaching = player.copy_reaching();
        let devices = offered.devices_met();

        let looked = player.syncing.copied.looked_for(&reaching, &devices, false);
        let kept = fs::read(mirror_in(&offered.kept_here())).expect("the copy received");

        player.sync_stop();

        assert_eq!(
            looked,
            Looked::Current {
                amendments_asked: true
            }
        );
        assert_eq!(offered.held_dump().as_deref(), Some(DUMP));
        assert_eq!(kept, offered.copy);
        assert!(!offered.kept_here().join(COPY_RECEIVED).exists());
    }

    #[test]
    fn a_copy_removed_while_received_calls_the_receive_off_and_is_fetched_again_only_once_turned_back_on()
     {
        let mut offered = a_desk_offers(
            Offer {
                dump: DUMP,
                gated: true,
            },
            kept_by_a_phone_with(roomy),
            "removed",
        );

        let player = Arc::clone(&offered.harness.player);
        let copied = &player.syncing.copied;
        let kept_here = offered.kept_here();

        assert!(offered.copy.len() as u64 > RECEIVED_BEFORE_THE_GATE);

        eventually("a part of the copy received", || {
            copied.wake();
            fs::metadata(kept_here.join(COPY_RECEIVED))
                .is_ok_and(|metadata| metadata.len() >= RECEIVED_BEFORE_THE_GATE)
        });

        let receiving = player.musicbrainz_copy();
        let began = Instant::now();

        player.remove_musicbrainz_copy().expect("the copy removed");

        let settled_in = began.elapsed();
        let removed = player.musicbrainz_copy();
        let asked_before = offered.offers.asked.load(Ordering::Acquire);

        assert_eq!(receiving.standing, MusicbrainzCopyStanding::Receiving);
        assert!(settled_in < A_RECEIVE_CALLED_OFF_SETTLES_WITHIN);
        assert_eq!(removed.standing, MusicbrainzCopyStanding::Off);
        assert_eq!(removed.dump, None);
        assert!(!kept_here.join(COPY_RECEIVED).exists());
        assert!(!kept_here.join(COPY_NOTED).exists());
        assert!(!mirror_in(&kept_here).exists());
        assert!(kept_here.join(REMOVED_BY_THE_LISTENER).exists());
        assert!(Copied::under(kept_here.clone()).was_removed());

        offered.gate = None;
        copied.keep(true, player.copy_reaching());

        let looked_until = Instant::now() + LOOKED_AT_FOR;

        while Instant::now() < looked_until {
            copied.wake();
            thread::sleep(Duration::from_millis(100));
        }

        assert_eq!(offered.offers.asked.load(Ordering::Acquire), asked_before);
        assert_eq!(
            player.musicbrainz_copy().standing,
            MusicbrainzCopyStanding::Off
        );
        assert!(!kept_here.join(COPY_RECEIVED).exists());
        assert!(!mirror_in(&kept_here).exists());

        copied.keep(false, player.copy_reaching());
        copied.forget_removal();
        copied.keep(true, player.copy_reaching());

        eventually("the copy fetched again once turned back on", || {
            copied.wake();
            player.musicbrainz_copy().standing == MusicbrainzCopyStanding::Held
        });

        let fetched = fs::read(mirror_in(&kept_here)).expect("the copy fetched again");

        player.sync_stop();

        assert_eq!(fetched, offered.copy);
        assert!(!kept_here.join(REMOVED_BY_THE_LISTENER).exists());
    }

    #[test]
    fn a_copy_turned_off_in_the_settings_forgets_that_it_was_removed() {
        let harness = Harness::new();
        let player = &harness.player;
        let kept_here = harness.folder.join("musicbrainz");

        player.remove_musicbrainz_copy().expect("the copy removed");

        let remembered = kept_here.join(REMOVED_BY_THE_LISTENER).exists();
        let config = |musicbrainz_copy| SyncConfig {
            enabled: false,
            nearby_only: true,
            server: String::new(),
            key: String::new(),
            device: "Phone".to_owned(),
            favourites: false,
            playlists: false,
            everything: false,
            metered_allowed: true,
            control: SyncControl::Everything,
            stun: String::new(),
            certificate: String::new(),
            musicbrainz_copy,
            takes_deletions: true,
        };

        player.sync_start(config(true)).expect("sync left off");

        let kept_on = kept_here.join(REMOVED_BY_THE_LISTENER).exists();

        player.sync_start(config(false)).expect("sync left off");

        assert!(remembered);
        assert!(kept_on);
        assert!(!kept_here.join(REMOVED_BY_THE_LISTENER).exists());
        assert!(!player.syncing.copied.was_removed());
    }

    #[test]
    fn a_timer_never_asked_is_due_and_rests_for_nothing() {
        let now = Instant::now();
        let timer = AmendmentTimer::default();

        assert!(timer.due(now));
        assert_eq!(timer.remaining(now), Duration::ZERO);
    }

    #[test]
    fn wakes_within_the_hour_that_ask_for_nothing_leave_the_next_due_tick_asking() {
        let start = Instant::now();
        let mut timer = AmendmentTimer::default();

        timer.asked(start);

        let wakes = (1..60).map(|minute| start + Duration::from_secs(minute * 60));

        for wake in wakes {
            assert!(!timer.due(wake));
        }

        let due = start + AMENDMENTS_ASKED_EVERY;

        assert!(timer.due(due));
        assert_eq!(
            timer.remaining(start + Duration::from_secs(10 * 60)),
            Duration::from_secs(50 * 60)
        );
    }

    #[test]
    fn an_amendment_request_restarts_the_hour() {
        let start = Instant::now();
        let mut timer = AmendmentTimer::default();

        timer.asked(start);

        let again = start + AMENDMENTS_ASKED_EVERY;

        timer.asked(again);

        assert!(!timer.due(again + Duration::from_secs(60)));
        assert!(timer.due(again + AMENDMENTS_ASKED_EVERY));
    }
}
