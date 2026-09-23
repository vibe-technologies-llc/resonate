use std::time::Duration;

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, TrackId, Volume};
use resonate_engine::{Asleep, Until};
use resonate_mpris::{
    Described, PlaybackStatus, PlayerName, PlaylistInfo, PlaylistOrder, Queueing, Running, Seeking,
};

use crate::{Error, Result};

pub type Row = (MediaLocation, Option<FrameSpan>);

pub trait Controlling {
    fn name(&self) -> String;

    fn playback(&self) -> resonate_mpris::Result<Option<PlaybackStatus>>;

    fn play(&self) -> resonate_mpris::Result<()>;

    fn pause(&self) -> resonate_mpris::Result<()>;

    fn play_pause(&self) -> resonate_mpris::Result<()>;

    fn stop(&self) -> resonate_mpris::Result<()>;

    fn next(&self) -> resonate_mpris::Result<()>;

    fn previous(&self) -> resonate_mpris::Result<()>;

    fn seek(&self, by: Seeking) -> resonate_mpris::Result<()>;

    fn volume(&self) -> resonate_mpris::Result<Volume>;

    fn set_volume(&self, volume: Volume) -> resonate_mpris::Result<()>;

    fn metadata(&self) -> resonate_mpris::Result<Option<Described>>;

    fn position(&self) -> resonate_mpris::Result<Duration>;

    fn queued(&self) -> resonate_mpris::Result<Vec<TrackId>>;

    fn described(&self, rows: &[TrackId]) -> resonate_mpris::Result<Vec<Described>>;

    fn queue(&self, rows: &[Row], queueing: Queueing) -> resonate_mpris::Result<usize>;

    fn remove_track(&self, track: TrackId) -> resonate_mpris::Result<()>;

    fn playlists(&self) -> resonate_mpris::Result<Vec<PlaylistInfo>>;

    fn activate_playlist(&self, playlist: PlaylistId) -> resonate_mpris::Result<()>;

    fn sleep(&self) -> resonate_mpris::Result<Option<Asleep>>;

    fn set_sleep(&self, until: Option<Until>) -> resonate_mpris::Result<()>;
}

pub trait Reach {
    fn player(&self) -> Result<Box<dyn Controlling>>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OnTheBus {
    named: Option<PlayerName>,
}

impl OnTheBus {
    pub const fn named(named: Option<PlayerName>) -> Self {
        Self { named }
    }
}

impl Reach for OnTheBus {
    fn player(&self) -> Result<Box<dyn Controlling>> {
        let found = match &self.named {
            Some(name) => {
                Running::named(name)?.ok_or_else(|| Error::NoSuchPlayer { name: name.clone() })?
            }
            None => Running::found()?.ok_or(Error::NothingRunning)?,
        };
        Ok(Box::new(found))
    }
}

impl Controlling for Running {
    fn name(&self) -> String {
        Running::name(self).to_string()
    }

    fn playback(&self) -> resonate_mpris::Result<Option<PlaybackStatus>> {
        Running::playback(self)
    }

    fn play(&self) -> resonate_mpris::Result<()> {
        Running::play(self)
    }

    fn pause(&self) -> resonate_mpris::Result<()> {
        Running::pause(self)
    }

    fn play_pause(&self) -> resonate_mpris::Result<()> {
        Running::play_pause(self)
    }

    fn stop(&self) -> resonate_mpris::Result<()> {
        Running::stop(self)
    }

    fn next(&self) -> resonate_mpris::Result<()> {
        Running::next(self)
    }

    fn previous(&self) -> resonate_mpris::Result<()> {
        Running::previous(self)
    }

    fn seek(&self, by: Seeking) -> resonate_mpris::Result<()> {
        Running::seek(self, by)
    }

    fn volume(&self) -> resonate_mpris::Result<Volume> {
        Running::volume(self)
    }

    fn set_volume(&self, volume: Volume) -> resonate_mpris::Result<()> {
        Running::set_volume(self, volume)
    }

    fn metadata(&self) -> resonate_mpris::Result<Option<Described>> {
        Running::metadata(self)
    }

    fn position(&self) -> resonate_mpris::Result<Duration> {
        Running::position(self)
    }

    fn queued(&self) -> resonate_mpris::Result<Vec<TrackId>> {
        Running::queued(self)
    }

    fn described(&self, rows: &[TrackId]) -> resonate_mpris::Result<Vec<Described>> {
        Running::described(self, rows)
    }

    fn queue(&self, rows: &[Row], queueing: Queueing) -> resonate_mpris::Result<usize> {
        Running::queue(self, rows, queueing)
    }

    fn remove_track(&self, track: TrackId) -> resonate_mpris::Result<()> {
        Running::remove_track(self, track)
    }

    fn playlists(&self) -> resonate_mpris::Result<Vec<PlaylistInfo>> {
        Running::playlists(self, PlaylistOrder::Alphabetical, false, 0, None)
    }

    fn activate_playlist(&self, playlist: PlaylistId) -> resonate_mpris::Result<()> {
        Running::activate_playlist(self, playlist)
    }

    fn sleep(&self) -> resonate_mpris::Result<Option<Asleep>> {
        Running::sleep(self)
    }

    fn set_sleep(&self, until: Option<Until>) -> resonate_mpris::Result<()> {
        Running::set_sleep(self, until)
    }
}
