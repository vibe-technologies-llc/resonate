use std::{
    fs,
    sync::Arc,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, bounded};
use resonate_apple::{Answer, Asked, Clock, NowPlaying, Offered, Playback, Shown};
use resonate_core::MediaLocation;

use crate::{
    interfaces::{PlayerInterface, Shared},
    track::track_path,
    transport::{Controls, Displayed, Moved, Status, TIMELINE_EVERY, Timeline},
};

const PRESSES_WAITING: usize = 16;
const NANOS_PER_TICK: u64 = 100;
const MICROS_PER_SECOND: u128 = 1_000_000;
const PLAYING_RATE: f64 = 1.0;
const HELD_RATE: f64 = 0.0;

pub(crate) struct Transport {
    center: NowPlaying,
    presses: Receiver<Asked>,
    player: PlayerInterface,
    told: Controls,
    offered: Option<Offered>,
    timed: Instant,
}

impl Transport {
    pub(crate) fn open(shared: &Arc<Shared>) -> Self {
        let (pressed, presses) = bounded(PRESSES_WAITING);
        let center = NowPlaying::answered_by(move |asked| match pressed.try_send(asked) {
            Ok(()) => Answer::Done,
            Err(_) => Answer::Failed,
        });

        Self {
            center,
            presses,
            player: PlayerInterface {
                shared: Arc::clone(shared),
            },
            told: Controls::default(),
            offered: None,
            timed: Instant::now(),
        }
    }

    pub(crate) fn answer_within(&self, patience: Duration) {
        match self.presses.recv_timeout(patience) {
            Ok(asked) => {
                refused(self.answered(asked));
                for asked in self.presses.try_iter() {
                    refused(self.answered(asked));
                }
            }
            Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => {}
        }
    }

    pub(crate) fn tell(&mut self, now: Controls, seeked: bool) {
        let due = self.timed.elapsed() >= TIMELINE_EVERY;
        let moved = Moved::between(&self.told, &now, seeked, due);
        let offered = offered_by(&now);

        if self.offered != Some(offered) {
            self.center.offer(offered);
            self.offered = Some(offered);
        }
        if moved.status
            && let Some(status) = now.status
        {
            self.center.playback(playback_of(status));
        }
        if moved.displayed {
            self.center
                .show(now.displayed.as_ref().map(shown_of).as_ref());
        }
        if moved.timeline {
            self.center.clock(clock_of(now.timeline, now.status));
            self.timed = Instant::now();
        }

        self.told = now;
    }

    pub(crate) fn close(self) {
        self.center.playback(Playback::Stopped);
        self.center.show(None);
    }

    fn answered(&self, asked: Asked) -> zbus::fdo::Result<()> {
        match asked {
            Asked::Play => self.player.play(),
            Asked::Pause => self.player.pause(),
            Asked::Toggle => self.player.play_pause(),
            Asked::Stop => self.player.stop(),
            Asked::Next => self.player.next(),
            Asked::Previous => self.player.previous(),
            Asked::Seek(to) => self.moved_to(to),
        }
    }

    fn moved_to(&self, to: Duration) -> zbus::fdo::Result<()> {
        let Some(current) = self.player.shared.player.state().current else {
            return Ok(());
        };

        self.player
            .set_position(track_path(current.id).into_inner(), micros_of(to))
    }
}

fn offered_by(now: &Controls) -> Offered {
    Offered {
        play: now.buttons.play,
        pause: now.buttons.pause,
        stop: now.buttons.stop,
        next: now.buttons.next,
        previous: now.buttons.previous,
        seek: now.timeline.is_some_and(|timeline| timeline.seekable),
    }
}

const fn playback_of(status: Status) -> Playback {
    match status {
        Status::Playing => Playback::Playing,
        Status::Paused => Playback::Paused,
        Status::Stopped | Status::Closed => Playback::Stopped,
    }
}

fn shown_of(displayed: &Displayed) -> Shown {
    Shown {
        title: displayed.title.clone(),
        artist: displayed.artist.clone(),
        album: displayed.album.clone(),
        album_artist: displayed.album_artist.clone(),
        track_number: displayed.track_number,
        cover: displayed.art.as_deref().and_then(cover_of),
    }
}

fn cover_of(art: &str) -> Option<Arc<[u8]>> {
    let location = MediaLocation::from_uri(art)?;
    let path = location.as_path()?;

    match fs::read(path) {
        Ok(bytes) => Some(Arc::from(bytes)),
        Err(error) => {
            tracing::debug!(%error, "the cover did not reach Now Playing");
            None
        }
    }
}

fn clock_of(timeline: Option<Timeline>, status: Option<Status>) -> Option<Clock> {
    let timeline = timeline?;
    let rate = match status {
        Some(Status::Playing) => PLAYING_RATE,
        _ => HELD_RATE,
    };

    Some(Clock {
        elapsed: since_the_start(timeline.position),
        length: timeline.seekable.then(|| since_the_start(timeline.end)),
        rate,
    })
}

fn since_the_start(ticks: i64) -> Duration {
    Duration::from_nanos(
        u64::try_from(ticks)
            .unwrap_or_default()
            .saturating_mul(NANOS_PER_TICK),
    )
}

fn micros_of(to: Duration) -> i64 {
    i64::try_from(
        to.as_micros()
            .min(MICROS_PER_SECOND.saturating_mul(u128::from(u32::MAX))),
    )
    .unwrap_or(i64::MAX)
}

fn refused(outcome: zbus::fdo::Result<()>) {
    if let Err(error) = outcome {
        tracing::debug!(%error, "Now Playing asked for what the player refused");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::Buttons;

    fn playing() -> Controls {
        Controls {
            status: Some(Status::Playing),
            buttons: Buttons::of(true, true, true, false),
            timeline: Some(Timeline {
                position: 10_000_000,
                end: 2_400_000_000,
                seekable: true,
            }),
            ..Controls::default()
        }
    }

    #[test]
    fn a_playing_song_runs_at_one_and_a_held_one_stands_still() {
        let now = playing();

        let running = clock_of(now.timeline, Some(Status::Playing)).unwrap();
        let held = clock_of(now.timeline, Some(Status::Paused)).unwrap();

        assert_eq!(running.elapsed, Duration::from_secs(1));
        assert_eq!(running.length, Some(Duration::from_secs(240)));
        assert!((running.rate - PLAYING_RATE).abs() < f64::EPSILON);
        assert!((held.rate - HELD_RATE).abs() < f64::EPSILON);
        assert_eq!(clock_of(None, Some(Status::Playing)), None);
    }

    #[test]
    fn a_stream_of_no_known_length_is_told_without_one_and_cannot_be_sought() {
        let open = Controls {
            timeline: Some(Timeline {
                position: 10_000_000,
                end: 10_000_000,
                seekable: false,
            }),
            ..playing()
        };

        let clock = clock_of(open.timeline, open.status).unwrap();

        assert_eq!(clock.length, None);
        assert!(!offered_by(&open).seek);
        assert!(offered_by(&playing()).seek);
    }

    #[test]
    fn what_is_offered_is_what_the_buttons_say() {
        let offered = offered_by(&playing());

        assert!(offered.play && offered.pause && offered.stop && offered.next);
        assert!(!offered.previous);
    }

    #[test]
    fn nothing_queued_is_told_as_stopped() {
        assert_eq!(playback_of(Status::Closed), Playback::Stopped);
        assert_eq!(playback_of(Status::Stopped), Playback::Stopped);
        assert_eq!(playback_of(Status::Paused), Playback::Paused);
        assert_eq!(playback_of(Status::Playing), Playback::Playing);
    }

    #[test]
    fn a_place_asked_for_crosses_to_the_player_in_micros() {
        assert_eq!(micros_of(Duration::from_millis(1500)), 1_500_000);
        assert_eq!(micros_of(Duration::ZERO), 0);
        assert_eq!(since_the_start(-5), Duration::ZERO);
    }

    #[test]
    fn a_cover_that_is_not_a_file_here_is_left_out() {
        assert_eq!(cover_of("https://example.invalid/cover.jpg"), None);
        assert_eq!(cover_of("file:///nowhere/at/all/cover.jpg"), None);
    }
}
