use std::{
    io::{Read, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use resonate_engine::Player;
use zbus::{ObjectPath, fdo};

use crate::{
    Host, Opened, PlaybackStatus, Playlists, Result,
    art::Pictures,
    interfaces::{Owed, OwnInterface, PlayerInterface, Shared},
    remote::described,
    track::{no_track, track_of, track_path},
    tracklist::TrackList,
    wire::{Act, Fault, Reply, Request, WireFault, read_frame, write_frame},
};

pub struct Answering {
    player: PlayerInterface,
    tracks: TrackList,
    own: OwnInterface,
    shared: Arc<Shared>,
    playlists: Option<Arc<dyn Playlists>>,
    closed: AtomicBool,
}

impl Answering {
    pub fn new(
        player: Arc<Player>,
        host: Arc<dyn Host>,
        playlists: Option<Arc<dyn Playlists>>,
    ) -> Self {
        Self::over(
            Arc::new(Shared {
                player,
                host,
                pictures: Pictures::default(),
                owed: Owed::default(),
            }),
            playlists,
        )
    }

    pub(crate) fn over(shared: Arc<Shared>, playlists: Option<Arc<dyn Playlists>>) -> Self {
        Self {
            player: PlayerInterface {
                shared: Arc::clone(&shared),
            },
            tracks: TrackList {
                shared: Arc::clone(&shared),
            },
            own: OwnInterface {
                shared: Arc::clone(&shared),
            },
            shared,
            playlists,
            closed: AtomicBool::new(false),
        }
    }

    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.shared.pictures.forget();
    }

    pub fn serve<S: Read + Write>(&self, mut stream: S) -> Result<()> {
        while let Some(bytes) = read_frame(&mut stream)? {
            if self.closed.load(Ordering::Acquire) {
                return Ok(());
            }
            let reply = match Request::decode(&bytes) {
                Ok(request) => self.answer(request),
                Err(fault) => Self::unreadable(&fault),
            };
            write_frame(&mut stream, &reply.encode())?;
        }
        Ok(())
    }

    fn unreadable(fault: &WireFault) -> Reply {
        tracing::debug!(%fault, "a request on the pipe could not be read");
        match fault {
            WireFault::NoSuchRequest { .. } => Reply::Faulted(Fault::NotSupported),
            _ => Reply::Faulted(Fault::InvalidArgs),
        }
    }

    pub(crate) fn answer(&self, request: Request) -> Reply {
        match request {
            Request::CanRaise => Reply::Flag(self.shared.host.can_raise()),
            Request::Told(act) => self.told(act),
            Request::Seek(micros) => done(self.player.seek(micros)),
            Request::SetPosition { track, micros } => done(
                self.player
                    .set_position(ObjectPath::from(&track_path(track)), micros),
            ),
            Request::Playback => Reply::Text(
                PlaybackStatus::from(self.shared.player.state().playback)
                    .as_str()
                    .to_owned(),
            ),
            Request::Volume => Reply::Real(self.player.volume()),
            Request::SetVolume(volume) => done(self.player.set_volume(volume)),
            Request::Shuffle => Reply::Flag(self.player.shuffle()),
            Request::Metadata => Reply::Row(described(&self.player.metadata())),
            Request::Position => Reply::Micros(self.player.position()),
            Request::Queued => Reply::Tracks(
                self.tracks
                    .tracks()
                    .iter()
                    .filter_map(|path| track_of(path.as_str()))
                    .collect(),
            ),
            Request::PlayOrder => Reply::Tracks(
                self.own
                    .play_order()
                    .iter()
                    .filter_map(|path| track_of(path.as_str()))
                    .collect(),
            ),
            Request::Described(rows) => Reply::Rows(
                self.tracks
                    .get_tracks_metadata(rows.into_iter().map(track_path).collect())
                    .iter()
                    .filter_map(described)
                    .collect(),
            ),
            Request::RemoveTrack(track) => done(
                self.tracks
                    .remove_track(ObjectPath::from(&track_path(track))),
            ),
            Request::Playlists {
                order,
                reverse,
                from,
                most,
            } => self.with_playlists(|playlists| {
                Reply::Listed(playlists.listing(
                    order,
                    reverse,
                    usize::try_from(from).unwrap_or(usize::MAX),
                    Some(usize::try_from(most).unwrap_or(usize::MAX)),
                ))
            }),
            Request::ActivatePlaylist(playlist) => {
                self.with_playlists(|playlists| match playlists.activate(playlist) {
                    Opened::Accepted => Reply::Done,
                    Opened::Refused => Reply::Faulted(Fault::Failed),
                })
            }
            Request::Sleep => {
                let (mode, seconds) = self.own.sleep();
                Reply::Sleeping { mode, seconds }
            }
            Request::SetSleep { mode, seconds } => done(self.own.set_sleep(&mode, seconds)),
            Request::AddTracks { uris, after, play } => {
                let after = after.map_or_else(no_track, track_path);
                done(self.own.add_tracks(uris, ObjectPath::from(&after), play))
            }
            Request::Relocate(moved) => done(self.own.relocate(moved)),
            Request::Tune(settings) => done(self.own.tune(settings)),
            Request::Reload { group, settings } => done(self.own.reload(&group, &settings)),
            Request::HandOver(zone) => done(self.own.hand_over(zone)),
            Request::Zone => Reply::Text(self.own.zone()),
            Request::Waiting => Reply::Count(self.own.playing_next()),
            Request::Device => Reply::Text(self.own.device()),
        }
    }

    fn told(&self, act: Act) -> Reply {
        match act {
            Act::Raise => {
                self.shared.host.raise();
                Reply::Done
            }
            Act::Play => done(self.player.play()),
            Act::Pause => done(self.player.pause()),
            Act::PlayPause => done(self.player.play_pause()),
            Act::Stop => done(self.player.stop()),
            Act::Next => done(self.player.next()),
            Act::Previous => done(self.player.previous()),
        }
    }

    fn with_playlists(&self, ask: impl FnOnce(&Arc<dyn Playlists>) -> Reply) -> Reply {
        match &self.playlists {
            Some(playlists) => ask(playlists),
            None => Reply::Faulted(Fault::NotSupported),
        }
    }
}

fn done(result: fdo::Result<()>) -> Reply {
    match result {
        Ok(()) => Reply::Done,
        Err(error) => {
            tracing::debug!(%error, "a call on the pipe was refused");
            Reply::Faulted(match error {
                fdo::Error::InvalidArgs(_) => Fault::InvalidArgs,
                fdo::Error::NotSupported(_) => Fault::NotSupported,
                _ => Fault::Failed,
            })
        }
    }
}
