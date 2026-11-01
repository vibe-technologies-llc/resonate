use resonate_core::Frames;
use resonate_engine::{PlaybackState, RepeatMode, TagSet};

use crate::track::{PlaybackStatus, micros};

const TICKS_PER_MICRO: i64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Status {
    Closed,
    Stopped,
    Playing,
    Paused,
}

impl Status {
    pub(crate) fn of(playback: PlaybackState, queued: bool) -> Self {
        if !queued {
            return Self::Closed;
        }
        match PlaybackStatus::from(playback) {
            PlaybackStatus::Playing => Self::Playing,
            PlaybackStatus::Paused => Self::Paused,
            PlaybackStatus::Stopped => Self::Stopped,
        }
    }

    pub(crate) const fn code(self) -> i32 {
        match self {
            Self::Closed => 0,
            Self::Stopped => 2,
            Self::Playing => 3,
            Self::Paused => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Buttons {
    pub(crate) play: bool,
    pub(crate) pause: bool,
    pub(crate) stop: bool,
    pub(crate) next: bool,
    pub(crate) previous: bool,
}

impl Buttons {
    pub(crate) const fn of(
        can_play: bool,
        can_pause: bool,
        can_go_next: bool,
        can_go_previous: bool,
    ) -> Self {
        Self {
            play: can_play,
            pause: can_pause,
            stop: can_pause,
            next: can_go_next,
            previous: can_go_previous,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Pressed {
    Play,
    Pause,
    Stop,
    Next,
    Previous,
}

impl Pressed {
    pub(crate) const fn of(button: i32) -> Option<Self> {
        match button {
            0 => Some(Self::Play),
            1 => Some(Self::Pause),
            2 => Some(Self::Stop),
            6 => Some(Self::Next),
            7 => Some(Self::Previous),
            _ => None,
        }
    }
}

pub(crate) const fn repeat_code(repeat: RepeatMode) -> i32 {
    match repeat {
        RepeatMode::Off => 0,
        RepeatMode::Track => 1,
        RepeatMode::Queue => 2,
    }
}

pub(crate) const fn repeat_asked(code: i32) -> Option<RepeatMode> {
    match code {
        0 => Some(RepeatMode::Off),
        1 => Some(RepeatMode::Track),
        2 => Some(RepeatMode::Queue),
        _ => None,
    }
}

pub(crate) fn ticks(frames: Frames, rate: u32) -> i64 {
    micros(frames, rate).saturating_mul(TICKS_PER_MICRO)
}

pub(crate) const fn micros_of_ticks(ticks: i64) -> i64 {
    ticks / TICKS_PER_MICRO
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Timeline {
    pub(crate) position: i64,
    pub(crate) end: i64,
    pub(crate) seekable: bool,
}

impl Timeline {
    pub(crate) fn of(
        position: Frames,
        duration: Option<Frames>,
        rate: u32,
        seekable: bool,
    ) -> Self {
        let position = ticks(position, rate);
        let end = duration.map_or(position, |duration| ticks(duration, rate));

        Self {
            position,
            end: end.max(position),
            seekable: seekable && duration.is_some(),
        }
    }

    pub(crate) const fn seek_end(self) -> i64 {
        if self.seekable { self.end } else { 0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Displayed {
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) album: String,
    pub(crate) album_artist: String,
    pub(crate) track_number: u32,
    pub(crate) art: Option<String>,
}

impl Displayed {
    pub(crate) fn of(tags: &TagSet, title: &str, art: Option<String>) -> Self {
        let named =
            |tag: &Option<String>| tag.as_deref().map(str::trim).unwrap_or_default().to_owned();

        Self {
            title: title.to_owned(),
            artist: named(&tags.artist),
            album: named(&tags.album),
            album_artist: named(&tags.album_artist),
            track_number: tags.track_number.unwrap_or_default(),
            art,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Controls {
    pub(crate) status: Option<Status>,
    pub(crate) buttons: Buttons,
    pub(crate) shuffle: bool,
    pub(crate) repeat: Option<RepeatMode>,
    pub(crate) displayed: Option<Displayed>,
    pub(crate) timeline: Option<Timeline>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Moved {
    pub(crate) status: bool,
    pub(crate) buttons: bool,
    pub(crate) modes: bool,
    pub(crate) displayed: bool,
    pub(crate) timeline: bool,
}

impl Moved {
    pub(crate) fn between(before: &Controls, now: &Controls, seeked: bool, due: bool) -> Self {
        let displayed = before.displayed != now.displayed;
        let timeline = match (before.timeline, now.timeline) {
            (Some(before), Some(now)) => {
                seeked || due || before.end != now.end || before.seekable != now.seekable
            }
            (None, None) => false,
            _ => true,
        };

        Self {
            status: before.status != now.status,
            buttons: before.buttons != now.buttons,
            modes: before.shuffle != now.shuffle || before.repeat != now.repeat,
            displayed,
            timeline: timeline || displayed || before.status != now.status,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged() -> TagSet {
        TagSet {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            album_artist: Some("Pink Floyd".to_owned()),
            track_number: Some(6),
            ..TagSet::default()
        }
    }

    fn playing() -> Controls {
        Controls {
            status: Some(Status::Playing),
            buttons: Buttons {
                play: true,
                pause: true,
                stop: true,
                next: true,
                previous: false,
            },
            shuffle: false,
            repeat: Some(RepeatMode::Off),
            displayed: Some(Displayed::of(&tagged(), "Echoes", None)),
            timeline: Some(Timeline::of(Frames(0), Some(Frames(44_100)), 44_100, true)),
        }
    }

    #[test]
    fn the_status_windows_is_told_is_the_one_mpris_publishes_and_closed_with_nothing_queued() {
        assert_eq!(Status::of(PlaybackState::Playing, true), Status::Playing);
        assert_eq!(Status::of(PlaybackState::Buffering, true), Status::Playing);
        assert_eq!(Status::of(PlaybackState::Paused, true), Status::Paused);
        assert_eq!(Status::of(PlaybackState::Stopped, true), Status::Stopped);
        assert_eq!(Status::of(PlaybackState::Idle, true), Status::Stopped);
        assert_eq!(Status::of(PlaybackState::Playing, false), Status::Closed);

        assert_eq!(Status::Closed.code(), 0);
        assert_eq!(Status::Stopped.code(), 2);
        assert_eq!(Status::Playing.code(), 3);
        assert_eq!(Status::Paused.code(), 4);
    }

    #[test]
    fn a_button_is_offered_where_mpris_says_its_method_does_something_and_stop_beside_pause() {
        let alone = Buttons::of(true, true, false, false);
        let idle = Buttons::of(true, false, false, false);

        assert!(alone.play && alone.pause && alone.stop);
        assert!(!alone.next && !alone.previous);
        assert!(!idle.stop, "stop was offered with nothing playing");
        assert_eq!(Buttons::of(false, false, false, false), Buttons::default());
        assert!(Buttons::of(true, true, true, true).previous);
    }

    #[test]
    fn each_transport_button_windows_names_is_the_mpris_method_of_that_name() {
        assert_eq!(Pressed::of(0), Some(Pressed::Play));
        assert_eq!(Pressed::of(1), Some(Pressed::Pause));
        assert_eq!(Pressed::of(2), Some(Pressed::Stop));
        assert_eq!(Pressed::of(6), Some(Pressed::Next));
        assert_eq!(Pressed::of(7), Some(Pressed::Previous));

        for unoffered in [3, 4, 5, 8, 9, -1] {
            assert_eq!(Pressed::of(unoffered), None, "button {unoffered}");
        }
    }

    #[test]
    fn a_repeat_mode_crosses_to_windows_and_back_unchanged() {
        for repeat in [RepeatMode::Off, RepeatMode::Track, RepeatMode::Queue] {
            assert_eq!(repeat_asked(repeat_code(repeat)), Some(repeat));
        }

        assert_eq!(repeat_code(RepeatMode::Queue), 2);
        assert_eq!(repeat_asked(3), None);
    }

    #[test]
    fn a_position_is_told_in_hundreds_of_nanoseconds_and_read_back_in_micros() {
        assert_eq!(ticks(Frames(44_100), 44_100), 10_000_000);
        assert_eq!(ticks(Frames(22_050), 44_100), 5_000_000);
        assert_eq!(micros_of_ticks(10_000_000), 1_000_000);
        assert_eq!(
            micros_of_ticks(ticks(Frames(96_000 * 3), 96_000)),
            3_000_000
        );
    }

    #[test]
    fn a_stream_of_no_known_length_is_told_as_one_nothing_can_be_sought_in() {
        let open = Timeline::of(Frames(44_100), None, 44_100, true);
        let closed = Timeline::of(Frames(44_100), Some(Frames(88_200)), 44_100, false);
        let whole = Timeline::of(Frames(44_100), Some(Frames(88_200)), 44_100, true);

        assert!(!open.seekable);
        assert_eq!(open.end, open.position);
        assert_eq!(open.seek_end(), 0);
        assert_eq!(closed.seek_end(), 0);
        assert_eq!(whole.seek_end(), 20_000_000);
    }

    #[test]
    fn a_track_is_shown_by_the_title_its_notification_carries_and_the_rest_of_its_tags() {
        let shown = Displayed::of(&tagged(), "Echoes", Some("file:///C:/art.png".to_owned()));
        let untitled = Displayed::of(&TagSet::default(), "06 Echoes", None);

        assert_eq!(shown.title, "Echoes");
        assert_eq!(shown.artist, "Pink Floyd");
        assert_eq!(shown.album, "Meddle");
        assert_eq!(shown.album_artist, "Pink Floyd");
        assert_eq!(shown.track_number, 6);
        assert_eq!(shown.art.as_deref(), Some("file:///C:/art.png"));
        assert_eq!(untitled.title, "06 Echoes");
        assert_eq!(untitled.artist, "");
        assert_eq!(untitled.track_number, 0);
    }

    #[test]
    fn only_what_moved_is_told_again_and_the_clock_on_a_seek_or_when_due() {
        let before = playing();
        let mut paused = playing();
        paused.status = Some(Status::Paused);
        let mut moving = playing();
        moving.timeline = Some(Timeline::of(
            Frames(4_410),
            Some(Frames(44_100)),
            44_100,
            true,
        ));
        let mut covered = playing();
        if let Some(shown) = covered.displayed.as_mut() {
            shown.art = Some("file:///C:/art.png".to_owned());
        }

        let still = Moved::between(&before, &playing(), false, false);
        assert_eq!(
            still,
            Moved {
                status: false,
                buttons: false,
                modes: false,
                displayed: false,
                timeline: false,
            }
        );

        let flowing = Moved::between(&before, &moving, false, false);
        assert!(!flowing.timeline, "a playhead moving on its own was told");

        assert!(Moved::between(&before, &moving, true, false).timeline);
        assert!(Moved::between(&before, &moving, false, true).timeline);

        let halted = Moved::between(&before, &paused, false, false);
        assert!(halted.status);
        assert!(halted.timeline);

        let laid = Moved::between(&before, &covered, false, false);
        assert!(laid.displayed);
        assert!(laid.timeline);
    }
}
