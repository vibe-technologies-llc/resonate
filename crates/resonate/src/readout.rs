use std::{
    io::{self, Write as _},
    time::Duration,
};

use resonate_engine::{Asleep, PlaybackState, PlayerState, RepeatMode, TrackState, Until};

const CLEAR_THE_LINE: &str = "\r\x1b[K";
const SECONDS_A_MINUTE: u64 = 60;
const MINUTES_AN_HOUR: u64 = 60;
const WHOLE: f32 = 100.0;

pub struct Readout {
    live: bool,
    drawn: bool,
    typing: String,
}

impl Readout {
    pub const fn over(live: bool) -> Self {
        Self {
            live,
            drawn: false,
            typing: String::new(),
        }
    }

    pub fn typing(&mut self, typed: String) {
        self.typing = typed;
    }

    pub fn clear(&mut self) {
        if self.drawn {
            print!("{CLEAR_THE_LINE}");
            let _ = io::stdout().flush();
            self.drawn = false;
        }
    }

    pub fn draw(&mut self, state: &PlayerState) {
        if !self.live {
            return;
        }
        print!("{CLEAR_THE_LINE}{}", line_of(state, &self.typing));
        let _ = io::stdout().flush();
        self.drawn = true;
    }
}

pub fn line_of(state: &PlayerState, typing: &str) -> String {
    let mut line = format!(
        "{} {}  vol {:.0}%",
        glyph(state.playback),
        where_in(state.current),
        state.volume.get() * WHOLE
    );
    if state.shuffle {
        line.push_str("  shuffle");
    }
    match state.repeat {
        RepeatMode::Off => {}
        RepeatMode::Queue => line.push_str("  repeat queue"),
        RepeatMode::Track => line.push_str("  repeat track"),
    }
    if let Some(asleep) = state.sleeping {
        line.push_str("  ");
        line.push_str(&sleeping(asleep));
    }
    if !typing.is_empty() {
        line.push_str("  > ");
        line.push_str(typing);
    }
    line
}

const fn glyph(playback: PlaybackState) -> &'static str {
    match playback {
        PlaybackState::Playing => "▶",
        PlaybackState::Paused => "⏸",
        PlaybackState::Buffering => "…",
        PlaybackState::Idle | PlaybackState::Stopped => "■",
    }
}

fn where_in(current: Option<TrackState>) -> String {
    let Some(track) = current else {
        return "-:--".to_owned();
    };
    let at = ticked(track.position.to_duration(track.source.rate));
    match track.duration {
        Some(length) => format!("{at} / {}", ticked(length.to_duration(track.source.rate))),
        None => at,
    }
}

fn sleeping(asleep: Asleep) -> String {
    match (asleep.until, asleep.left) {
        (Until::After(_), Some(left)) => format!("sleep in {}", ticked(left)),
        (Until::After(after), None) => format!("sleep in {}", ticked(after)),
        (Until::EndOfTrack, _) => "sleep at the end of the track".to_owned(),
        (Until::EndOfQueue, _) => "sleep at the end of the queue".to_owned(),
    }
}

fn ticked(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / SECONDS_A_MINUTE;
    match minutes / MINUTES_AN_HOUR {
        0 => format!("{minutes}:{:02}", seconds % SECONDS_A_MINUTE),
        hours => format!(
            "{hours}:{:02}:{:02}",
            minutes % MINUTES_AN_HOUR,
            seconds % SECONDS_A_MINUTE
        ),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{
        ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec, TrackId, Volume,
    };

    use super::*;

    fn playing(at_seconds: u64, of_seconds: u64) -> PlayerState {
        let rate = SampleRate::HZ_44100;
        PlayerState {
            playback: PlaybackState::Playing,
            current: Some(TrackState {
                id: TrackId::new(1).expect("an id"),
                source: StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::S16),
                position: Frames::from_duration(Duration::from_secs(at_seconds), rate),
                duration: Some(Frames::from_duration(Duration::from_secs(of_seconds), rate)),
            }),
            volume: Volume::new(0.8).expect("a volume"),
            ..PlayerState::default()
        }
    }

    #[test]
    fn the_readout_says_where_the_track_is_and_how_loud() {
        assert_eq!(line_of(&playing(83, 296), ""), "▶ 1:23 / 4:56  vol 80%");
        assert_eq!(
            line_of(&playing(3_725, 4_000), ""),
            "▶ 1:02:05 / 1:06:40  vol 80%"
        );
        assert_eq!(line_of(&PlayerState::default(), ""), "■ -:--  vol 100%");
    }

    #[test]
    fn the_readout_names_what_is_switched_on_and_what_is_being_typed() {
        let state = PlayerState {
            shuffle: true,
            repeat: RepeatMode::Queue,
            sleeping: Some(Asleep {
                until: Until::After(Duration::from_secs(1_800)),
                left: Some(Duration::from_secs(754)),
            }),
            ..playing(0, 60)
        };

        assert_eq!(
            line_of(&state, ":z tr"),
            "▶ 0:00 / 1:00  vol 80%  shuffle  repeat queue  sleep in 12:34  > :z tr"
        );
    }
}
