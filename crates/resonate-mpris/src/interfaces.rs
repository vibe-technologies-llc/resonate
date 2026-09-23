use std::{collections::HashMap, mem, sync::Arc, time::Duration};

use ahash::AHashSet;
use parking_lot::Mutex;
use resonate_core::{FrameSpan, Frames, MediaLocation, TrackId, Volume};
use resonate_engine::{
    Command, Player, PlayerState, QueueItem, RepeatMode, StreamDigest, TrackState,
};
use zbus::{
    fdo, interface,
    zvariant::{ObjectPath, OwnedValue},
};

use crate::{
    Heard, Host, Opened,
    art::Pictures,
    track::{
        PlaybackStatus, Sleep, SleepMode, frames, loop_status, metadata, micros, playing_digest,
        repeat_mode, sleep_status, track_path,
    },
};

pub(crate) const OWN_INTERFACE: &str = "org.resonate.Player1";
const RATE: f64 = 1.0;

const STANDS_STILL: f64 = 0.0;
const FILE_SCHEME: &str = "file";
const SETTLE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct Owed {
    rows: Mutex<AHashSet<TrackId>>,
}

impl Owed {
    pub(crate) fn note(&self, track: TrackId) {
        self.rows.lock().insert(track);
    }

    pub(crate) fn taken(&self) -> Vec<TrackId> {
        mem::take(&mut *self.rows.lock()).into_iter().collect()
    }

    pub(crate) fn owed_again(&self, rows: impl IntoIterator<Item = TrackId>) {
        self.rows.lock().extend(rows);
    }
}

pub(crate) struct Shared {
    pub(crate) player: Arc<Player>,
    pub(crate) host: Arc<dyn Host>,
    pub(crate) pictures: Pictures,
    pub(crate) owed: Owed,
}

impl Shared {
    pub(crate) fn located(&self, uri: &str) -> fdo::Result<(MediaLocation, Option<FrameSpan>)> {
        let refused =
            || fdo::Error::NotSupported("that URI names no source this build can open".to_owned());
        let (location, span) = MediaLocation::from_uri_within(uri).ok_or_else(refused)?;
        if !self.host.sources().contains(location.source()) {
            return Err(refused());
        }
        Ok((location, span))
    }

    pub(crate) fn art(
        &self,
        state: &PlayerState,
        digest: Option<&Arc<StreamDigest>>,
    ) -> Option<String> {
        let current = state.current?;
        let digest = playing_digest(state, digest)?;
        let art = self.player.art(&digest.location)?;

        self.pictures.uri(current.id, &art)
    }

    pub(crate) fn heard(&self, state: &PlayerState, queue: &[QueueItem]) -> Option<Heard> {
        let current = state.current?;
        let playing = queue.iter().find(|item| item.id == current.id)?;

        self.host.heard(&playing.location, playing.span)
    }

    pub(crate) fn settle(&self, command: Command) -> fdo::Result<()> {
        let kind = command.kind();
        let landing = self.player.settle(command).map_err(|error| {
            tracing::warn!(%error, "a command from the session bus did not reach the engine");
            fdo::Error::Failed("the engine is not accepting commands".to_owned())
        })?;

        if let Err(error) = landing.wait_for(SETTLE) {
            tracing::debug!(%error, ?kind, "the engine has not applied a command from the bus yet");
        }
        Ok(())
    }
}

pub(crate) struct Root {
    pub(crate) shared: Arc<Shared>,
}

#[interface(name = "org.mpris.MediaPlayer2")]
impl Root {
    fn raise(&self) {
        self.shared.host.raise();
    }

    fn quit(&self) {
        self.shared.host.quit();
    }

    #[zbus(property)]
    fn can_quit(&self) -> bool {
        self.shared.host.can_quit()
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        self.shared.host.can_raise()
    }

    #[zbus(property)]
    const fn has_track_list(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.shared.host.identity()
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> String {
        self.shared.host.desktop_entry().unwrap_or_default()
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        self.shared
            .host
            .sources()
            .iter()
            .map(|source| {
                if source.is_local() {
                    FILE_SCHEME.to_owned()
                } else {
                    source.to_string()
                }
            })
            .collect()
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        self.shared.host.mime_types()
    }
}

pub(crate) struct PlayerInterface {
    pub(crate) shared: Arc<Shared>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    fn next(&self) -> fdo::Result<()> {
        self.shared.settle(Command::Next)
    }

    fn previous(&self) -> fdo::Result<()> {
        self.shared.settle(Command::Previous)
    }

    fn pause(&self) -> fdo::Result<()> {
        self.shared.settle(Command::Pause)
    }

    fn play_pause(&self) -> fdo::Result<()> {
        self.shared.settle(Command::TogglePlayPause)
    }

    fn stop(&self) -> fdo::Result<()> {
        self.shared.settle(Command::Stop)
    }

    fn play(&self) -> fdo::Result<()> {
        self.shared.settle(Command::Play)
    }

    fn seek(&self, offset: i64) -> fdo::Result<()> {
        let Some(current) = self.shared.player.state().current else {
            return Ok(());
        };
        let delta = micros_to_frames_signed(offset, current.source.rate.hz());
        if reaches_past_the_end(&current, delta) {
            return self.shared.settle(Command::Next);
        }
        self.shared.settle(Command::SeekBy(delta))
    }

    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) -> fdo::Result<()> {
        let state = self.shared.player.state();
        let Some(current) = state.current else {
            return Ok(());
        };
        if track_id.as_str() != track_path(current.id).as_str() {
            return Ok(());
        }
        let Ok(position) = u64::try_from(position) else {
            return Ok(());
        };
        let wanted = frames(position, current.source.rate.hz());
        if current.duration.is_some_and(|duration| wanted > duration) {
            return Ok(());
        }
        self.shared.settle(Command::Seek(wanted))
    }

    fn open_uri(&self, uri: &str) -> fdo::Result<()> {
        let (location, span) = self.shared.located(uri)?;
        match self.shared.host.open(&location, span) {
            Opened::Accepted => Ok(()),
            Opened::Refused => Err(fdo::Error::NotSupported(
                "this front end does not open files from the bus".to_owned(),
            )),
        }
    }

    #[zbus(property)]
    fn playback_status(&self) -> String {
        PlaybackStatus::from(self.shared.player.state().playback)
            .as_str()
            .to_owned()
    }

    #[zbus(property)]
    fn loop_status(&self) -> String {
        loop_status(self.shared.player.state().repeat).to_owned()
    }

    #[zbus(property)]
    fn set_loop_status(&self, status: &str) -> fdo::Result<()> {
        let Some(repeat) = repeat_mode(status) else {
            return Err(fdo::Error::InvalidArgs(
                "LoopStatus is one of None, Track or Playlist".to_owned(),
            ));
        };
        self.shared.settle(Command::SetRepeat(repeat))
    }

    #[zbus(property)]
    const fn rate(&self) -> f64 {
        RATE
    }

    #[zbus(property)]
    fn set_rate(&self, rate: f64) -> fdo::Result<()> {
        if (rate - RATE).abs() < f64::EPSILON {
            return Ok(());
        }
        if (rate - STANDS_STILL).abs() < f64::EPSILON {
            return self.shared.settle(Command::Pause);
        }
        Err(fdo::Error::NotSupported(
            "resonate plays at the source rate only".to_owned(),
        ))
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.shared.player.state().shuffle
    }

    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) -> fdo::Result<()> {
        self.shared.settle(Command::SetShuffle(shuffle))
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let state = self.shared.player.state();
        let digest = self.shared.player.digest();
        let art = self.shared.art(&state, digest.as_ref());
        let heard = self.shared.heard(&state, &self.shared.player.queue());

        metadata(&state, digest.as_ref(), art, heard)
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        f64::from(self.shared.player.state().volume.get())
    }

    #[zbus(property)]
    fn set_volume(&self, volume: f64) -> fdo::Result<()> {
        let Ok(volume) = Volume::new(volume.clamp(0.0, 1.0) as f32) else {
            return Err(fdo::Error::InvalidArgs(
                "Volume is a fraction between 0 and 1".to_owned(),
            ));
        };
        self.shared.settle(Command::SetVolume(volume))
    }

    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        let state = self.shared.player.state();
        state
            .current
            .map_or(0, |track| micros(track.position, track.source.rate.hz()))
    }

    #[zbus(property)]
    const fn minimum_rate(&self) -> f64 {
        RATE
    }

    #[zbus(property)]
    const fn maximum_rate(&self) -> f64 {
        RATE
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        let state = self.shared.player.state();
        state.current.is_some()
            && (state.repeat != RepeatMode::Off
                || state
                    .queue_position
                    .is_some_and(|at| at + 1 < state.queue_len))
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        let state = self.shared.player.state();
        state.current.is_some()
            && (state.repeat == RepeatMode::Queue || state.queue_position.is_some_and(|at| at > 0))
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.shared.player.state().queue_len > 0
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.shared.player.state().current.is_some()
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        let state = self.shared.player.state();
        let Some(current) = state.current else {
            return false;
        };
        self.shared
            .player
            .digest()
            .is_some_and(|digest| digest.track == current.id && digest.info.is_seekable)
    }

    #[zbus(property)]
    const fn can_control(&self) -> bool {
        true
    }

    #[zbus(signal)]
    pub(crate) async fn seeked(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        position: i64,
    ) -> zbus::Result<()>;
}

pub(crate) struct OwnInterface {
    pub(crate) shared: Arc<Shared>,
}

#[interface(name = "org.resonate.Player1")]
impl OwnInterface {
    fn set_sleep(&self, mode: &str, seconds: u64) -> fdo::Result<()> {
        let Some(wanted) = SleepMode::read(mode) else {
            return Err(fdo::Error::InvalidArgs(format!(
                "{mode} is not one of {}",
                SleepMode::every_one()
            )));
        };
        self.shared
            .settle(Command::SleepUntil(wanted.until(seconds)))
    }

    #[zbus(property)]
    fn sleep(&self) -> Sleep {
        sleep_status(self.shared.player.state().sleeping)
    }
}

fn micros_to_frames_signed(offset: i64, rate: u32) -> i64 {
    let magnitude = frames(offset.unsigned_abs(), rate).get();
    let magnitude = i64::try_from(magnitude).unwrap_or(i64::MAX);
    if offset < 0 { -magnitude } else { magnitude }
}

fn reaches_past_the_end(current: &TrackState, delta: i64) -> bool {
    let Some(duration) = current.duration else {
        return false;
    };

    delta > 0
        && current
            .position
            .saturating_add(Frames(delta.unsigned_abs()))
            > duration
}
