use std::time::Duration;

use resonate_core::{FrameSpan, Frames, MediaLocation, TrackId};

use crate::{PlaybackState, PlayerState, QueueItem, TrackState};

pub const COUNTS_AS_HEARD: Duration = Duration::from_secs(240);

pub const A_SEEK: Duration = Duration::from_secs(5);

pub const TOLD_EVERY: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Listened {
    track: TrackId,
    at: Frames,
    heard: Frames,
    told: Frames,
    counted: Option<Played>,
}

impl Listened {
    const fn starting(track: &TrackState) -> Self {
        Self {
            track: track.id,
            at: track.position,
            heard: Frames::ZERO,
            told: Frames::ZERO,
            counted: None,
        }
    }

    fn resumes(&self, track: &TrackState) -> bool {
        self.track == track.id && self.at <= track.position
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Played {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub heard: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Counting {
    Counts(Played),
    Hears(Played),
    Settles(Played),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listening {
    held: Option<Listened>,
}

impl Listening {
    pub fn heard(&mut self, state: &PlayerState, queue: &[QueueItem]) -> Option<Counting> {
        let Some(track) = state.current.as_ref() else {
            return self.ends();
        };

        let settled = match self.held.as_ref() {
            Some(held) if held.resumes(track) => None,
            _ => self.ends(),
        };
        let mut held = self
            .held
            .take()
            .unwrap_or_else(|| Listened::starting(track));

        let step = track.position.saturating_sub(held.at);
        held.at = track.position;
        if state.playback == PlaybackState::Playing && step <= listened(track) {
            held.heard = held.heard.saturating_add(step);
        }

        let so_far = held.heard.to_duration(track.source.rate);
        let counted = (held.counted.is_none() && held.heard >= enough(track))
            .then(|| playing(state, queue, track.id, so_far))
            .flatten();
        let mut hears = None;
        match held.counted.as_mut() {
            Some(played) => {
                played.heard = so_far;
                let untold = held.heard.saturating_sub(held.told);
                if untold >= Frames::from_duration(TOLD_EVERY, track.source.rate) {
                    held.told = held.heard;
                    hears = Some(Counting::Hears(played.clone()));
                }
            }
            None => {
                held.counted.clone_from(&counted);
                held.told = held.heard;
            }
        }
        self.held = Some(held);

        settled.or_else(|| counted.map(Counting::Counts)).or(hears)
    }

    pub fn leaves(&mut self) -> Option<Counting> {
        self.ends()
    }

    fn ends(&mut self) -> Option<Counting> {
        self.held.take()?.counted.map(Counting::Settles)
    }
}

fn playing(
    state: &PlayerState,
    queue: &[QueueItem],
    track: TrackId,
    heard: Duration,
) -> Option<Played> {
    queue
        .get(state.queue_position?)
        .filter(|item| item.id == track)
        .map(|item| Played {
            location: item.location.clone(),
            span: item.span,
            heard,
        })
}

fn enough(track: &TrackState) -> Frames {
    let capped = Frames::from_duration(COUNTS_AS_HEARD, track.source.rate);

    track
        .duration
        .map_or(capped, |whole| half(whole).min(capped))
}

fn listened(track: &TrackState) -> Frames {
    Frames::from_duration(A_SEEK, track.source.rate)
}

const fn half(frames: Frames) -> Frames {
    Frames(frames.get() / 2)
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, QueueStamp, SampleFormat, SampleRate, StreamSpec, Volume};

    use super::*;
    use crate::{RepeatMode, Seeks};

    const RATE: SampleRate = SampleRate::HZ_44100;

    fn seconds(count: u64) -> Frames {
        Frames(RATE.hz() as u64 * count)
    }

    fn track(number: u64) -> TrackId {
        TrackId::new(number).expect("track ids start at one")
    }

    fn queued(count: u64) -> Vec<QueueItem> {
        (1..=count)
            .map(|number| QueueItem {
                id: track(number),
                location: MediaLocation::local(format!("/music/{number}.flac")),
                span: None,
            })
            .collect()
    }

    fn playing_at(number: u64, position: Frames, whole: Option<Frames>) -> PlayerState {
        PlayerState {
            playback: PlaybackState::Playing,
            current: Some(TrackState {
                id: track(number),
                source: StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::S16),
                position,
                duration: whole,
            }),
            volume: Volume::MAX,
            repeat: RepeatMode::Off,
            skip_under_repeat: crate::SkipUnderRepeat::default(),
            previous_restarts: crate::PreviousRestarts::default(),
            shuffle: false,
            queue_position: Some(number as usize - 1),
            loaded_position: Some(number as usize - 1),
            queue_len: 1,
            queue_stamp: QueueStamp::default(),
            seeks: Seeks::default(),
            sleeping: None,
            output: None,
        }
    }

    fn counted(heard: Option<Counting>) -> Option<Played> {
        match heard {
            Some(Counting::Counts(played)) => Some(played),
            _ => None,
        }
    }

    fn settled(heard: Option<Counting>) -> Option<Played> {
        match heard {
            Some(Counting::Settles(played)) => Some(played),
            _ => None,
        }
    }

    fn walked(listening: &mut Listening, number: u64, whole: Option<Frames>, to: u64) -> usize {
        let queue = queued(number.max(1));
        let mut counts = 0;

        for second in 0..=to {
            let state = playing_at(number, seconds(second), whole);
            if counted(listening.heard(&state, &queue)).is_some() {
                counts += 1;
            }
        }

        counts
    }

    #[test]
    fn half_a_track_counts_as_hearing_it() {
        let mut listening = Listening::default();
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 99), 0);
        assert_eq!(walked(&mut listening, 1, whole, 100), 1);
    }

    #[test]
    fn a_long_track_counts_once_four_minutes_have_gone_by() {
        let mut listening = Listening::default();
        let whole = Some(seconds(3_600));

        assert_eq!(walked(&mut listening, 1, whole, 239), 0);
        assert_eq!(walked(&mut listening, 1, whole, 240), 1);
    }

    #[test]
    fn a_track_of_no_declared_length_counts_at_four_minutes() {
        let mut listening = Listening::default();

        assert_eq!(walked(&mut listening, 1, None, 239), 0);
        assert_eq!(walked(&mut listening, 1, None, 240), 1);
    }

    #[test]
    fn hearing_the_rest_of_a_track_counts_it_no_second_time() {
        let mut listening = Listening::default();
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 200), 1);
    }

    #[test]
    fn a_seek_past_the_mark_counts_nothing_by_itself() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        for position in [Frames::ZERO, seconds(1), seconds(180), seconds(190)] {
            let state = playing_at(1, position, whole);
            assert_eq!(listening.heard(&state, &queue), None);
        }
    }

    #[test]
    fn a_paused_transport_hears_nothing() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        for second in 0..=200 {
            let mut state = playing_at(1, seconds(second), whole);
            state.playback = PlaybackState::Paused;
            assert_eq!(listening.heard(&state, &queue), None);
        }
    }

    #[test]
    fn a_track_heard_again_from_the_top_counts_again() {
        let mut listening = Listening::default();
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 200), 1);
        assert_eq!(walked(&mut listening, 1, whole, 200), 1);
    }

    #[test]
    fn the_row_the_queue_is_on_is_what_a_play_names() {
        let mut listening = Listening::default();
        let queue = queued(2);
        let whole = Some(seconds(10));

        for second in 0..=5 {
            let state = playing_at(2, seconds(second), whole);
            let heard = listening.heard(&state, &queue);
            if second == 5 {
                assert_eq!(
                    heard,
                    Some(Counting::Counts(Played {
                        location: MediaLocation::local("/music/2.flac"),
                        span: None,
                        heard: Duration::from_secs(5),
                    }))
                );
            } else {
                assert_eq!(heard, None);
            }
        }
    }

    #[test]
    fn a_queue_the_playing_row_has_left_names_nothing() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(10));

        for second in 0..=5 {
            let state = playing_at(2, seconds(second), whole);
            assert_eq!(listening.heard(&state, &queue), None);
        }
    }

    #[test]
    fn an_idle_transport_forgets_what_it_was_hearing() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        for second in 0..=99 {
            let state = playing_at(1, seconds(second), whole);
            assert_eq!(listening.heard(&state, &queue), None);
        }

        let stopped = PlayerState::default();
        assert_eq!(listening.heard(&stopped, &queue), None);

        for second in 100..=101 {
            let state = playing_at(1, seconds(second), whole);
            assert_eq!(listening.heard(&state, &queue), None);
        }
    }

    #[test]
    fn a_visit_settles_with_the_whole_time_it_was_heard_for() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 150), 1);

        let stopped = PlayerState::default();
        let ended = settled(listening.heard(&stopped, &queue)).expect("a counted visit settles");
        assert_eq!(ended.location, MediaLocation::local("/music/1.flac"));
        assert_eq!(ended.heard, Duration::from_secs(150));
    }

    #[test]
    fn a_visit_that_never_counted_settles_nothing() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 50), 0);

        let stopped = PlayerState::default();
        assert_eq!(listening.heard(&stopped, &queue), None);
    }

    #[test]
    fn a_visit_settles_once_and_not_again() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));

        assert_eq!(walked(&mut listening, 1, whole, 150), 1);

        let stopped = PlayerState::default();
        assert!(settled(listening.heard(&stopped, &queue)).is_some());
        assert_eq!(listening.heard(&stopped, &queue), None);
    }

    #[test]
    fn a_track_played_from_the_top_again_counts_and_settles_twice() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(200));
        let mut counts = 0;
        let mut settles = Vec::new();

        for _ in 0..2 {
            for second in 0..=120 {
                match listening.heard(&playing_at(1, seconds(second), whole), &queue) {
                    Some(Counting::Counts(_)) => counts += 1,
                    Some(Counting::Settles(played)) => settles.push(played.heard),
                    Some(Counting::Hears(_)) | None => {}
                }
            }
        }
        if let Some(Counting::Settles(played)) = listening.heard(&PlayerState::default(), &queue) {
            settles.push(played.heard);
        }

        assert_eq!(counts, 2);
        assert_eq!(
            settles,
            [Duration::from_secs(120), Duration::from_secs(120)]
        );
    }

    #[test]
    fn a_counted_visit_says_how_long_it_has_been_heard_every_so_often() {
        let mut listening = Listening::default();
        let queue = queued(1);
        let whole = Some(seconds(600));
        let mut told = Vec::new();

        for second in 0..=300 {
            if let Some(Counting::Hears(played)) =
                listening.heard(&playing_at(1, seconds(second), whole), &queue)
            {
                told.push(played.heard.as_secs());
            }
        }

        assert_eq!(told, [270, 300]);
    }

    #[test]
    fn a_visit_left_mid_track_settles_with_what_was_heard() {
        let mut listening = Listening::default();
        let whole = Some(seconds(600));

        assert_eq!(walked(&mut listening, 1, whole, 250), 1);

        let left = settled(listening.leaves()).expect("a counted visit settles when it is left");
        assert_eq!(left.heard, Duration::from_secs(250));
        assert_eq!(listening.leaves(), None);
    }
}
