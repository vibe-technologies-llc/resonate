use std::{
    collections::HashMap,
    mem::ManuallyDrop,
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use ahash::AHashMap;
use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::RwLock;
use resonate_core::{Frames, TrackId, Volume};
use resonate_engine::{
    CoverArt, MediaInfo, PlaybackState, Player, PlayerState, QueueItem, RepeatMode, Seeks,
    TagsRead, TrackState,
};
use zbus::{
    blocking::{
        Connection, connection,
        fdo::{DBusProxy, NameLostIterator},
        object_server::InterfaceRef,
    },
    fdo::{RequestNameFlags, RequestNameReply},
    names::WellKnownName,
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::{
    BusOp, Error, Heard, Host, PlaylistInfo, PlaylistOrder, Playlists, Result,
    art::Pictures,
    desktop::Errands,
    interfaces::{Owed, OwnInterface, PlayerInterface, Root, Shared},
    notify::{Shown, shown},
    playlists::{PlaylistsInterface, listed, unheard_of},
    track::{
        PlaybackStatus, Sleep, metadata, micros, no_track, playing_digest, queued_metadata,
        sleep_status, sounding, track_path,
    },
    tracklist::{Change, TrackList, after_row, change},
};

pub(crate) const OBJECT_PATH: &str = "/org/mpris/MediaPlayer2";
pub(crate) const BUS_NAME: &str = "org.mpris.MediaPlayer2.resonate";
const POLL: Duration = Duration::from_millis(200);
const HEARD_READ_EVERY: Duration = Duration::from_secs(1);
const WAITING_TELLS: usize = 4;
const ANNOUNCE_BUDGET: Duration = Duration::from_millis(100);
const INSTANCES: u32 = 16;

struct Claimed {
    name: RwLock<WellKnownName<'static>>,
}

impl Claimed {
    fn new(name: WellKnownName<'static>) -> Self {
        Self {
            name: RwLock::new(name),
        }
    }

    fn name(&self) -> WellKnownName<'static> {
        self.name.read().clone()
    }

    fn moved_to(&self, name: WellKnownName<'static>) {
        *self.name.write() = name;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub summary: String,
    pub body: String,
    pub picture: Option<CoverArt>,
}

#[derive(Clone)]
pub struct Teller {
    tells: Sender<Told>,
}

impl Teller {
    pub fn tell(&self, told: Told) {
        if self.tells.try_send(told).is_err() {
            tracing::debug!("the desktop is still being told the last thing it was");
        }
    }
}

pub struct Mpris {
    tells: Sender<Told>,
    claimed: Arc<Claimed>,
    connection: Option<Connection>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Mpris {
    pub fn start(
        player: Arc<Player>,
        host: Arc<dyn Host>,
        playlists: Option<Arc<dyn Playlists>>,
    ) -> Result<Self> {
        let shared = Arc::new(Shared {
            player,
            host,
            pictures: Pictures::default(),
            owed: Owed::default(),
        });
        let mut builder = connection::Builder::session()
            .map_err(|source| Error::bus(BusOp::Connect, source))?
            .serve_at(
                OBJECT_PATH,
                Root {
                    shared: Arc::clone(&shared),
                },
            )
            .map_err(|source| Error::bus(BusOp::Serve, source))?
            .serve_at(
                OBJECT_PATH,
                PlayerInterface {
                    shared: Arc::clone(&shared),
                },
            )
            .map_err(|source| Error::bus(BusOp::Serve, source))?
            .serve_at(
                OBJECT_PATH,
                TrackList {
                    shared: Arc::clone(&shared),
                },
            )
            .map_err(|source| Error::bus(BusOp::Serve, source))?
            .serve_at(
                OBJECT_PATH,
                OwnInterface {
                    shared: Arc::clone(&shared),
                },
            )
            .map_err(|source| Error::bus(BusOp::Serve, source))?;

        if let Some(playlists) = playlists.clone() {
            builder = builder
                .serve_at(OBJECT_PATH, PlaylistsInterface { playlists })
                .map_err(|source| Error::bus(BusOp::Serve, source))?;
        }
        let connection = builder
            .build()
            .map_err(|source| Error::bus(BusOp::Connect, source))?;

        let losses = name_losses(&connection);
        let claimed = Arc::new(Claimed::new(claim(&connection)?));
        let stop = Arc::new(AtomicBool::new(false));
        let (tells, told) = bounded(WAITING_TELLS);
        let thread = {
            let stop = Arc::clone(&stop);
            let shared = Arc::clone(&shared);
            let connection = connection.clone();
            thread::Builder::new()
                .name("resonate-mpris".to_owned())
                .spawn(move || announce(&connection, &shared, playlists.as_ref(), &told, &stop))
                .map_err(|_| Error::ThreadStopped)?
        };
        if let Some(losses) = losses {
            watch_the_name(
                connection.clone(),
                Arc::clone(&claimed),
                Arc::clone(&stop),
                losses,
            );
        }

        tracing::info!(name = %claimed.name(), "the MPRIS interface is on the session bus");
        Ok(Self {
            tells,
            claimed,
            connection: Some(connection),
            stop,
            thread: Some(thread),
            shared,
        })
    }

    pub fn name(&self) -> WellKnownName<'static> {
        self.claimed.name()
    }

    pub fn teller(&self) -> Teller {
        Teller {
            tells: self.tells.clone(),
        }
    }

    pub fn shutdown(mut self) {
        self.stop_thread();
    }

    fn stop_thread(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        if let Some(connection) = self.connection.take() {
            hand_back(&connection, &self.claimed.name());
            let _ = connection.close();
        }
        self.shared.pictures.forget();
    }
}

impl Drop for Mpris {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

fn claim(connection: &Connection) -> Result<WellKnownName<'static>> {
    let preferred = WellKnownName::try_from(BUS_NAME)
        .map_err(|source| Error::bus(BusOp::ClaimName, source.into()))?;
    if owned(connection, &preferred)? {
        return Ok(preferred);
    }

    tracing::debug!("another player already owns the preferred bus name");
    instances(connection, preferred)
}

fn instances(
    connection: &Connection,
    fallback: WellKnownName<'static>,
) -> Result<WellKnownName<'static>> {
    let mut taken = fallback;
    for attempt in 1..=INSTANCES {
        let name = WellKnownName::try_from(instance_name(attempt))
            .map_err(|source| Error::bus(BusOp::ClaimName, source.into()))?;
        if owned(connection, &name)? {
            return Ok(name);
        }
        taken = name;
    }

    Err(Error::NameTaken { name: taken.into() })
}

fn name_losses(connection: &Connection) -> Option<NameLostIterator> {
    let subscribed = DBusProxy::new(connection)
        .map_err(|source| Error::bus(BusOp::Connect, source))
        .and_then(|bus| {
            bus.receive_name_lost()
                .map_err(|source| Error::bus(BusOp::Listen, source))
        });

    match subscribed {
        Ok(losses) => Some(losses),
        Err(error) => {
            tracing::warn!(%error, "a bus name taken away by another player will go unnoticed");
            None
        }
    }
}

fn watch_the_name(
    connection: Connection,
    claimed: Arc<Claimed>,
    stop: Arc<AtomicBool>,
    losses: NameLostIterator,
) {
    let watching = thread::Builder::new()
        .name("resonate-mpris-name".to_owned())
        .spawn(move || {
            let mut losses = ManuallyDrop::new(losses);
            for loss in losses.by_ref() {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                let Ok(args) = loss.args() else {
                    continue;
                };
                let lost = claimed.name();
                if args.name().as_str() != lost.as_str() {
                    continue;
                }
                match instances(&connection, lost.clone()) {
                    Ok(name) => {
                        tracing::warn!(%lost, %name, "another player took the bus name; serving under an instance name instead");
                        claimed.moved_to(name);
                    }
                    Err(error) => {
                        tracing::warn!(%error, %lost, "the bus name was taken away and no instance name was free");
                        break;
                    }
                }
            }
        });

    if watching.is_err() {
        tracing::warn!("a bus name taken away by another player will go unnoticed");
    }
}

fn hand_back(connection: &Connection, name: &WellKnownName<'static>) {
    if let Err(source) = connection.release_name(name.clone()) {
        let error = Error::bus(BusOp::ReleaseName, source);
        tracing::debug!(%error, %name, "the bus name was left to the connection closing rather than handed back");
    }
}

fn instance_name(attempt: u32) -> String {
    match attempt {
        1 => format!("{BUS_NAME}.instance{}", process::id()),
        other => format!("{BUS_NAME}.instance{}-{other}", process::id()),
    }
}

fn owned(connection: &Connection, name: &WellKnownName<'static>) -> Result<bool> {
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::AllowReplacement;
    match connection.request_name_with_flags(name.clone(), flags) {
        Ok(reply) => Ok(reply == RequestNameReply::PrimaryOwner),
        Err(zbus::Error::NameTaken) => Ok(false),
        Err(source) => Err(Error::bus(BusOp::ClaimName, source)),
    }
}

#[derive(Default)]
struct Collection {
    revision: Option<u64>,
    rows: Arc<[PlaylistInfo]>,
    playing: Option<PlaylistInfo>,
}

struct Watched {
    queue: Arc<Vec<QueueItem>>,
    playlists: Collection,
    playback: PlaybackState,
    repeat: RepeatMode,
    shuffle: bool,
    volume: Volume,
    track: Option<TrackId>,
    metadata: Arc<HashMap<String, OwnedValue>>,
    described: Described,
    heard: Reading,
    shown: Option<Shown>,
    can_go_next: bool,
    can_go_previous: bool,
    can_play: bool,
    can_pause: bool,
    can_seek: bool,
    seeks: Seeks,
    sleep: Sleep,
    playhead: Option<Playhead>,
    reads: u64,
}

#[derive(Clone, Copy)]
struct Playhead {
    position: Frames,
    rate: u32,
}

fn announce(
    connection: &Connection,
    shared: &Arc<Shared>,
    playlists: Option<&Arc<dyn Playlists>>,
    told: &Receiver<Told>,
    stop: &AtomicBool,
) {
    let server = connection.object_server();
    let Ok(player) = server.interface::<_, PlayerInterface>(OBJECT_PATH) else {
        tracing::warn!("the MPRIS player interface vanished from the object server");
        return;
    };
    let Ok(tracks) = server.interface::<_, TrackList>(OBJECT_PATH) else {
        tracing::warn!("the MPRIS track list vanished from the object server");
        return;
    };
    let Ok(ours) = server.interface::<_, OwnInterface>(OBJECT_PATH) else {
        tracing::warn!("the sleep timer interface vanished from the object server");
        return;
    };
    let collected = playlists.and_then(|_| {
        server
            .interface::<_, PlaylistsInterface>(OBJECT_PATH)
            .map_err(|_| tracing::warn!("the MPRIS playlists vanished from the object server"))
            .ok()
    });

    let errands = Errands::start(shared);
    let heard_from = told;
    let mut told = None;

    let mut watched = snapshot(shared, playlists, None);

    while !stop.load(Ordering::Acquire) {
        thread::sleep(POLL);

        let next = snapshot(shared, playlists, Some(&watched));
        publish(&player, &watched, &next);
        publish_sleep(&ours, &watched, &next);
        publish_tracks(&tracks, shared, &watched, &next);
        if let Some(collected) = collected.as_ref() {
            publish_playlists(collected, &watched.playlists, &next.playlists);
        }
        if let Some(errands) = errands.as_ref() {
            errands.follow(next.playback);
        }
        let wanted = Telling::of(shared.host.attended(), shared.host.notifies());
        told = tell(errands.as_ref(), &next, told, wanted);
        for heard in heard_from.try_iter() {
            if let (Telling::Raise, Some(errands)) = (wanted, errands.as_ref()) {
                let art = heard
                    .picture
                    .as_ref()
                    .and_then(|picture| shared.pictures.uri_of(picture));
                errands.tell(Shown::told(heard.summary, heard.body, art));
            }
        }
        if watched.seeks != next.seeks
            && let Some(playhead) = next.playhead
        {
            emit_seeked(&player, micros(playhead.position, playhead.rate));
        }
        watched = next;
    }

    if let Some(errands) = errands {
        errands.rest();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Telling {
    Raise,
    Note,
    Hold,
}

impl Telling {
    const fn of(attended: bool, notifies: bool) -> Self {
        if notifies && !attended {
            Self::Raise
        } else {
            Self::Note
        }
    }
}

fn telling(
    shown: Option<&Shown>,
    told: Option<&Shown>,
    playback: PlaybackState,
    wanted: Telling,
) -> Telling {
    match shown {
        None => Telling::Hold,
        Some(shown) if told == Some(shown) || !sounding(playback) => Telling::Hold,
        Some(_) => wanted,
    }
}

fn tell(
    errands: Option<&Errands>,
    now: &Watched,
    told: Option<Shown>,
    wanted: Telling,
) -> Option<Shown> {
    match telling(now.shown.as_ref(), told.as_ref(), now.playback, wanted) {
        Telling::Hold => told,
        Telling::Note => now.shown.clone(),
        Telling::Raise => {
            if let (Some(errands), Some(shown)) = (errands, now.shown.as_ref()) {
                errands.show(shown);
            }
            now.shown.clone()
        }
    }
}

#[derive(Clone, Default)]
struct Described {
    track: Option<TrackState>,
    info: Option<Arc<MediaInfo>>,
    art: Option<String>,
    heard: Option<Heard>,
}

impl PartialEq for Described {
    fn eq(&self, other: &Self) -> bool {
        self.track == other.track
            && self.art == other.art
            && self.heard == other.heard
            && match (&self.info, &other.info) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
    }
}

#[derive(Clone, Copy)]
struct Reading {
    heard: Option<Heard>,
    of: Option<TrackId>,
    at: Instant,
}

impl Reading {
    fn again(
        shared: &Shared,
        state: &PlayerState,
        queue: &[QueueItem],
        before: Option<&Self>,
    ) -> Self {
        let of = state.current.map(|current| current.id);
        let now = Instant::now();
        match before {
            Some(held)
                if held.of == of && now.saturating_duration_since(held.at) < HEARD_READ_EVERY =>
            {
                *held
            }
            _ => Self {
                heard: shared.heard(state, queue),
                of,
                at: now,
            },
        }
    }
}

fn described_otherwise(before: &Watched, now: &Watched) -> bool {
    !Arc::ptr_eq(&before.metadata, &now.metadata) && before.metadata != now.metadata
}

fn snapshot(
    shared: &Arc<Shared>,
    playlists: Option<&Arc<dyn Playlists>>,
    before: Option<&Watched>,
) -> Watched {
    let state = shared.player.state();
    let digest = shared.player.digest();
    let current = state.current;
    let art = shared.art(&state, digest.as_ref());
    let queue = shared.player.queue();
    let heard = Reading::again(shared, &state, &queue, before.map(|held| &held.heard));
    let described = Described {
        track: current.map(|track| TrackState {
            position: Frames::ZERO,
            ..track
        }),
        info: playing_digest(&state, digest.as_ref()).map(|playing| Arc::clone(&playing.info)),
        art: art.clone(),
        heard: heard.heard,
    };
    let metadata = match before {
        Some(held) if held.described == described => Arc::clone(&held.metadata),
        _ => Arc::new(metadata(&state, digest.as_ref(), art.clone(), heard.heard)),
    };
    let held_playlists = before.map(|held| &held.playlists);

    Watched {
        queue,
        playlists: collect(playlists, held_playlists),
        playback: state.playback,
        repeat: state.repeat,
        shuffle: state.shuffle,
        volume: state.volume,
        track: current.map(|track| track.id),
        metadata,
        described,
        heard,
        shown: playing_digest(&state, digest.as_ref())
            .and_then(|playing| shown(&playing.info.tags, &playing.location, art)),
        can_go_next: current.is_some()
            && (state.repeat != RepeatMode::Off
                || state
                    .queue_position
                    .is_some_and(|at| at + 1 < state.queue_len)),
        can_go_previous: current.is_some()
            && (state.repeat == RepeatMode::Queue || state.queue_position.is_some_and(|at| at > 0)),
        can_play: state.queue_len > 0,
        can_pause: current.is_some(),
        can_seek: current.is_some_and(|current| {
            digest.is_some_and(|digest| digest.track == current.id && digest.info.is_seekable)
        }),
        seeks: state.seeks,
        sleep: sleep_status(state.sleeping),
        playhead: current.map(|current| Playhead {
            position: current.position,
            rate: current.source.rate.hz(),
        }),
        reads: shared.player.media_revision(),
    }
}

fn publish(player: &InterfaceRef<PlayerInterface>, before: &Watched, now: &Watched) {
    let emitter = player.signal_emitter();
    let iface = player.get();

    if status_moved(before.playback, now.playback) {
        report(zbus::block_on(iface.playback_status_changed(emitter)));
    }
    if before.repeat != now.repeat {
        report(zbus::block_on(iface.loop_status_changed(emitter)));
    }
    if before.shuffle != now.shuffle {
        report(zbus::block_on(iface.shuffle_changed(emitter)));
    }
    if before.volume != now.volume {
        report(zbus::block_on(iface.volume_changed(emitter)));
    }
    if before.track != now.track || described_otherwise(before, now) {
        report(zbus::block_on(iface.metadata_changed(emitter)));
    }
    if before.can_go_next != now.can_go_next {
        report(zbus::block_on(iface.can_go_next_changed(emitter)));
    }
    if before.can_go_previous != now.can_go_previous {
        report(zbus::block_on(iface.can_go_previous_changed(emitter)));
    }
    if before.can_play != now.can_play {
        report(zbus::block_on(iface.can_play_changed(emitter)));
    }
    if before.can_pause != now.can_pause {
        report(zbus::block_on(iface.can_pause_changed(emitter)));
    }
    if before.can_seek != now.can_seek {
        report(zbus::block_on(iface.can_seek_changed(emitter)));
    }
}

fn status_moved(before: PlaybackState, now: PlaybackState) -> bool {
    PlaybackStatus::from(before) != PlaybackStatus::from(now)
}

fn publish_sleep(ours: &InterfaceRef<OwnInterface>, before: &Watched, now: &Watched) {
    if before.sleep == now.sleep {
        return;
    }
    report(zbus::block_on(
        ours.get().sleep_changed(ours.signal_emitter()),
    ));
}

fn publish_tracks(
    tracks: &InterfaceRef<TrackList>,
    shared: &Arc<Shared>,
    before: &Watched,
    now: &Watched,
) {
    let emitter = tracks.signal_emitter();

    if !Arc::ptr_eq(&before.queue, &now.queue) {
        publish_queue_moves(tracks, shared, before, now);
    }

    if described_otherwise(before, now)
        && let Some(track) = now.track
        && now.queue.iter().any(|item| item.id == track)
    {
        report(zbus::block_on(TrackList::track_metadata_changed(
            emitter,
            track_path(track),
            (*now.metadata).clone(),
        )));
    }
    if before.reads != now.reads {
        publish_late_reads(tracks, shared, now);
    }
}

fn publish_queue_moves(
    tracks: &InterfaceRef<TrackList>,
    shared: &Arc<Shared>,
    before: &Watched,
    now: &Watched,
) {
    let emitter = tracks.signal_emitter();

    match change(&before.queue, &now.queue) {
        Change::Unchanged => {}
        Change::Added { at } => {
            if let Some(item) = now.queue.get(at) {
                let media = shared
                    .player
                    .media_within(&item.location, item.span, ANNOUNCE_BUDGET);
                let state = shared.player.state();
                let digest = shared.player.digest();
                let metadata = queued_metadata(
                    item,
                    &state,
                    digest.as_ref(),
                    media.as_deref(),
                    shared.art(&state, digest.as_ref()),
                    shared.host.heard(&item.location, item.span),
                );
                report(zbus::block_on(TrackList::track_added(
                    emitter,
                    metadata,
                    after_row(&now.queue, at),
                )));
            }
        }
        Change::Removed { at } => {
            if let Some(item) = before.queue.get(at) {
                report(zbus::block_on(TrackList::track_removed(
                    emitter,
                    track_path(item.id),
                )));
            }
        }
        Change::Replaced => {
            let rows = now.queue.iter().map(|item| track_path(item.id)).collect();
            report(zbus::block_on(TrackList::track_list_replaced(
                emitter,
                rows,
                playing_row(now),
            )));
        }
    }
    if !before
        .queue
        .iter()
        .map(|item| item.id)
        .eq(now.queue.iter().map(|item| item.id))
    {
        report(zbus::block_on(tracks.get().tracks_invalidate(emitter)));
    }
}

fn publish_late_reads(tracks: &InterfaceRef<TrackList>, shared: &Arc<Shared>, now: &Watched) {
    let owed = shared.owed.taken();
    if owed.is_empty() {
        return;
    }
    let rows: AHashMap<TrackId, &QueueItem> =
        now.queue.iter().map(|item| (item.id, item)).collect();
    let mut landed = Vec::new();
    let mut waiting = Vec::new();
    for track in owed {
        let Some(item) = rows.get(&track).copied() else {
            continue;
        };
        match shared.player.tags_read(&item.location, item.span) {
            TagsRead::Answered(media) => landed.push((track, item, Some(media))),
            TagsRead::Nothing => landed.push((track, item, None)),
            TagsRead::NotYet => waiting.push(track),
        }
    }
    shared.owed.owed_again(waiting);

    let emitter = tracks.signal_emitter();
    let state = shared.player.state();
    let digest = shared.player.digest();
    let art = shared.art(&state, digest.as_ref());
    for (track, item, media) in landed {
        let metadata = queued_metadata(
            item,
            &state,
            digest.as_ref(),
            media.as_deref(),
            art.clone(),
            shared.host.heard(&item.location, item.span),
        );
        report(zbus::block_on(TrackList::track_metadata_changed(
            emitter,
            track_path(track),
            metadata,
        )));
    }
}

fn collect(playlists: Option<&Arc<dyn Playlists>>, held: Option<&Collection>) -> Collection {
    let Some(playlists) = playlists else {
        return Collection::default();
    };
    let revision = playlists.revision();

    Collection {
        revision: Some(revision),
        rows: match held {
            Some(held) if held.revision == Some(revision) => Arc::clone(&held.rows),
            _ => playlists
                .listing(PlaylistOrder::default(), false, 0, None)
                .into(),
        },
        playing: playlists.playing(),
    }
}

fn publish_playlists(
    collected: &InterfaceRef<PlaylistsInterface>,
    before: &Collection,
    now: &Collection,
) {
    let emitter = collected.signal_emitter();
    let iface = collected.get();

    if before.rows.len() != now.rows.len() {
        report(zbus::block_on(iface.playlist_count_changed(emitter)));
    }
    if before.playing != now.playing {
        report(zbus::block_on(iface.active_playlist_changed(emitter)));
    }

    for row in now.rows.iter() {
        if unheard_of(&before.rows, row) {
            report(zbus::block_on(PlaylistsInterface::playlist_changed(
                emitter,
                listed(row.clone()),
            )));
        }
    }
}

fn playing_row(now: &Watched) -> OwnedObjectPath {
    now.track.map_or_else(no_track, track_path)
}

fn emit_seeked(player: &InterfaceRef<PlayerInterface>, position: i64) {
    report(zbus::block_on(PlayerInterface::seeked(
        player.signal_emitter(),
        position,
    )));
}

fn report(outcome: zbus::Result<()>) {
    if let Err(source) = outcome {
        let error = Error::bus(BusOp::Emit, source);
        tracing::debug!(%error, "a property change did not reach the session bus");
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;
    use resonate_engine::TagSet;

    use super::*;

    fn echoes() -> Shown {
        shown(
            &TagSet {
                title: Some("Echoes".to_owned()),
                ..TagSet::default()
            },
            &MediaLocation::local("/music/Pink Floyd/Echoes.flac"),
            None,
        )
        .expect("a titled track is worth telling")
    }

    const UNATTENDED: Telling = Telling::of(false, true);

    #[test]
    fn only_a_move_the_bus_can_see_announces_the_playback_status() {
        assert!(!status_moved(
            PlaybackState::Buffering,
            PlaybackState::Playing
        ));
        assert!(!status_moved(PlaybackState::Idle, PlaybackState::Stopped));
        assert!(status_moved(PlaybackState::Playing, PlaybackState::Paused));
        assert!(status_moved(
            PlaybackState::Buffering,
            PlaybackState::Stopped
        ));
    }

    const ATTENDED: Telling = Telling::of(true, true);

    #[test]
    fn a_track_is_told_once_and_only_while_it_is_sounding() {
        let echoes = echoes();

        assert_eq!(
            telling(Some(&echoes), None, PlaybackState::Playing, UNATTENDED),
            Telling::Raise
        );
        assert_eq!(
            telling(
                Some(&echoes),
                Some(&echoes),
                PlaybackState::Playing,
                UNATTENDED
            ),
            Telling::Hold
        );
        assert_eq!(
            telling(Some(&echoes), None, PlaybackState::Paused, UNATTENDED),
            Telling::Hold
        );
        assert_eq!(
            telling(None, None, PlaybackState::Playing, UNATTENDED),
            Telling::Hold
        );
    }

    #[test]
    fn a_track_the_window_is_already_showing_is_noted_rather_than_raised() {
        let echoes = echoes();

        assert_eq!(
            telling(Some(&echoes), None, PlaybackState::Playing, ATTENDED),
            Telling::Note
        );
        assert_eq!(
            telling(
                Some(&echoes),
                Some(&echoes),
                PlaybackState::Playing,
                ATTENDED
            ),
            Telling::Hold
        );
    }

    #[test]
    fn a_host_that_has_notifications_turned_off_notes_a_track_rather_than_raising_it() {
        let echoes = echoes();
        let hushed = Telling::of(false, false);

        assert_eq!(
            telling(Some(&echoes), None, PlaybackState::Playing, hushed),
            Telling::Note,
            "a track started with notifications off raised one anyway"
        );
        assert_eq!(
            telling(Some(&echoes), Some(&echoes), PlaybackState::Playing, hushed),
            Telling::Hold
        );
        assert_eq!(
            Telling::of(true, false),
            Telling::Note,
            "an attended window with notifications off is still only noted"
        );
    }
}
