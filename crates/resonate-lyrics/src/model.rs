use std::time::Duration;

use resonate_core::SourceId;

use crate::{Error, Result};

const LIT_AT_MOST: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Waiting {
    pub next: usize,
    pub through: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timing {
    Synced,
    Unsynced,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Voice {
    #[default]
    One,
    Two,
}

impl Voice {
    pub const fn index(self) -> usize {
        match self {
            Self::One => 0,
            Self::Two => 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricLine {
    pub at: Option<Duration>,
    pub text: String,
    pub voice: Voice,
}

impl LyricLine {
    pub fn sung(at: Duration, text: impl Into<String>) -> Self {
        Self {
            at: Some(at),
            text: text.into(),
            voice: Voice::One,
        }
    }

    pub fn untimed(text: impl Into<String>) -> Self {
        Self {
            at: None,
            text: text.into(),
            voice: Voice::One,
        }
    }

    #[must_use]
    pub fn voiced(mut self, voice: Voice) -> Self {
        self.voice = voice;
        self
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credits {
    pub album: Option<String>,
    pub words_by: Option<String>,
    pub sheet_by: Option<String>,
    pub editor: Option<String>,
    pub version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    source: SourceId,
    timing: Timing,
    lines: Vec<LyricLine>,
    credits: Credits,
}

impl Lyrics {
    pub fn synced(source: SourceId, mut lines: Vec<LyricLine>) -> Result<Self> {
        if lines.iter().any(|line| line.at.is_none()) {
            return Err(Error::LineNotTimed);
        }
        lines.sort_by_key(|line| line.at);

        Ok(Self {
            source,
            timing: Timing::Synced,
            lines,
            credits: Credits::default(),
        })
    }

    pub fn plain(source: SourceId, lines: Vec<String>) -> Self {
        Self {
            source,
            timing: Timing::Unsynced,
            lines: lines.into_iter().map(LyricLine::untimed).collect(),
            credits: Credits::default(),
        }
    }

    #[must_use]
    pub fn credited(mut self, credits: Credits) -> Self {
        self.credits = credits;
        self
    }

    pub const fn credits(&self) -> &Credits {
        &self.credits
    }

    pub const fn source(&self) -> &SourceId {
        &self.source
    }

    pub const fn timing(&self) -> Timing {
        self.timing
    }

    pub fn lines(&self) -> &[LyricLine] {
        &self.lines
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(LyricLine::is_blank)
    }

    pub fn within(self, start: Duration, end: Option<Duration>) -> Option<Self> {
        if self.timing == Timing::Unsynced {
            return None;
        }
        let lines: Vec<LyricLine> = self
            .lines
            .into_iter()
            .filter_map(|line| {
                let at = line.at?;
                let inside = at >= start && end.is_none_or(|end| at < end);
                inside.then(|| LyricLine::sung(at - start, line.text).voiced(line.voice))
            })
            .collect();
        if lines.iter().all(LyricLine::is_blank) {
            return None;
        }

        Some(Self { lines, ..self })
    }

    pub fn line_at(&self, position: Duration) -> Option<usize> {
        if self.timing == Timing::Unsynced {
            return None;
        }
        let sung = self
            .lines
            .partition_point(|line| line.at.is_some_and(|at| at <= position));

        sung.checked_sub(1)
    }

    pub fn line_in_play(&self, position: Duration) -> Option<usize> {
        self.voices_in_play(position).into_iter().flatten().max()
    }

    pub fn voices_in_play(&self, position: Duration) -> [Option<usize>; 2] {
        let mut active = [None; 2];
        if self.timing == Timing::Unsynced {
            return active;
        }

        let passed = self
            .lines
            .partition_point(|line| line.at.is_some_and(|at| at <= position));
        let mut seen = [false; 2];
        for index in (0..passed).rev() {
            let line = &self.lines[index];
            let voice = line.voice.index();
            if seen[voice] {
                continue;
            }
            seen[voice] = true;
            let (_, until) = self.span_of(index).expect("a synced line has a span");
            if position < until {
                active[voice] = Some(index);
            }
            if seen.into_iter().all(|found| found) {
                break;
            }
        }

        active
    }

    pub fn waiting_at(&self, position: Duration) -> Option<Waiting> {
        if self.timing == Timing::Unsynced || self.line_in_play(position).is_some() {
            return None;
        }

        let Some(sung) = self.line_at(position) else {
            let arrives = self.lines.first()?.at?;

            return Some(Waiting {
                next: 0,
                through: share(position, arrives),
            });
        };
        let (_, until) = self.span_of(sung)?;
        let next = sung + 1;
        let arrives = self.lines.get(next)?.at?;

        Some(Waiting {
            next,
            through: share(
                position.saturating_sub(until),
                arrives.saturating_sub(until),
            ),
        })
    }

    fn span_of(&self, line: usize) -> Option<(Duration, Duration)> {
        let at = self.lines.get(line)?.at?;
        let next = self.lines[line + 1..]
            .iter()
            .find(|next| next.voice == self.lines[line].voice)
            .and_then(|line| line.at);
        let until = next
            .unwrap_or(Duration::MAX)
            .min(at.saturating_add(LIT_AT_MOST));

        Some((at, until))
    }
}

fn share(elapsed: Duration, whole: Duration) -> f32 {
    if whole.is_zero() {
        return 1.0;
    }

    (elapsed.as_secs_f32() / whole.as_secs_f32()).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source() -> SourceId {
        SourceId::new("held").expect("a lowercase name")
    }

    fn at(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn synced() -> Lyrics {
        Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(10), "and everything under the sun is in tune"),
                LyricLine::sung(at(0), "all that you touch"),
                LyricLine::sung(at(5), "all that you see"),
            ],
        )
        .expect("every line is timed")
    }

    #[test]
    fn synced_lines_are_held_in_the_order_they_are_sung_however_they_arrive() {
        let lyrics = synced();

        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(
            lyrics
                .lines()
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [
                "all that you touch",
                "all that you see",
                "and everything under the sun is in tune",
            ]
        );
    }

    #[test]
    fn a_line_that_carries_no_moment_cannot_be_part_of_a_synced_set() {
        let untimed = Lyrics::synced(source(), vec![LyricLine::untimed("all that you touch")]);

        assert!(matches!(untimed, Err(Error::LineNotTimed)));
    }

    #[test]
    fn the_line_in_play_is_the_last_one_whose_moment_has_passed() {
        let lyrics = synced();

        assert_eq!(lyrics.line_at(Duration::ZERO), Some(0));
        assert_eq!(lyrics.line_at(at(4)), Some(0));
        assert_eq!(lyrics.line_at(at(5)), Some(1));
        assert_eq!(lyrics.line_at(at(9)), Some(1));
        assert_eq!(lyrics.line_at(at(10)), Some(2));
        assert_eq!(lyrics.line_at(at(600)), Some(2));
    }

    #[test]
    fn a_position_ahead_of_the_first_line_is_between_lines_rather_than_on_one() {
        let lyrics = Lyrics::synced(source(), vec![LyricLine::sung(at(3), "all that you touch")])
            .expect("every line is timed");

        assert_eq!(lyrics.line_at(at(2)), None);
        assert_eq!(lyrics.line_at(at(3)), Some(0));
    }

    #[test]
    fn a_line_stays_lit_until_the_next_one_is_sung() {
        let lyrics = synced();

        assert_eq!(lyrics.line_in_play(at(4)), Some(0));
        assert_eq!(lyrics.line_in_play(at(5)), Some(1));
        assert_eq!(lyrics.line_in_play(at(9)), Some(1));
    }

    #[test]
    fn two_voices_stay_in_play_until_their_own_next_lines() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "first singer"),
                LyricLine::sung(at(2), "second singer").voiced(Voice::Two),
                LyricLine::sung(at(5), "first singer again"),
                LyricLine::sung(at(8), "second singer again").voiced(Voice::Two),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.voices_in_play(at(3)), [Some(0), Some(1)]);
        assert_eq!(lyrics.voices_in_play(at(6)), [Some(2), Some(1)]);
        assert_eq!(lyrics.voices_in_play(at(8)), [Some(2), Some(3)]);
        assert_eq!(lyrics.line_in_play(at(8)), Some(3));
        assert_eq!(lyrics.voices_in_play(at(20)), [None, None]);
    }

    #[test]
    fn cutting_a_cue_row_preserves_the_voice() {
        let lyrics = Lyrics::synced(
            source(),
            vec![LyricLine::sung(at(12), "second singer").voiced(Voice::Two)],
        )
        .expect("every line is timed")
        .within(at(10), Some(at(20)))
        .expect("a line is inside the cue row");

        assert_eq!(lyrics.lines()[0].at, Some(at(2)));
        assert_eq!(lyrics.lines()[0].voice, Voice::Two);
    }

    #[test]
    fn a_long_instrumental_puts_the_set_out_rather_than_leaving_the_last_line_lit() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "all that you touch"),
                LyricLine::sung(at(120), "all that you see"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.line_in_play(at(9)), Some(0));
        assert_eq!(lyrics.line_in_play(at(10)), None);
        assert_eq!(lyrics.line_in_play(at(119)), None);
        assert_eq!(lyrics.line_at(at(119)), Some(0));
        assert_eq!(lyrics.line_in_play(at(120)), Some(1));
    }

    #[test]
    fn the_last_line_goes_out_once_it_has_had_its_time() {
        let lyrics = synced();

        assert_eq!(lyrics.line_in_play(at(19)), Some(2));
        assert_eq!(lyrics.line_in_play(at(20)), None);
    }

    #[test]
    fn a_line_being_sung_is_not_a_gap_to_be_counted_down() {
        let lyrics = synced();

        assert_eq!(lyrics.waiting_at(at(0)), None);
        assert_eq!(lyrics.waiting_at(at(4)), None);
        assert_eq!(lyrics.waiting_at(at(9)), None);
    }

    #[test]
    fn a_gap_counts_down_to_the_line_it_is_waiting_on() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "all that you touch"),
                LyricLine::sung(at(30), "all that you see"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.waiting_at(at(9)), None);
        assert_eq!(
            lyrics.waiting_at(at(10)),
            Some(Waiting {
                next: 1,
                through: 0.0
            })
        );
        assert_eq!(
            lyrics.waiting_at(at(20)),
            Some(Waiting {
                next: 1,
                through: 0.5
            })
        );
        assert_eq!(lyrics.waiting_at(at(30)), None);
    }

    #[test]
    fn the_time_before_the_first_line_counts_down_to_it() {
        let lyrics = Lyrics::synced(
            source(),
            vec![LyricLine::sung(at(20), "all that you touch")],
        )
        .expect("every line is timed");

        assert_eq!(
            lyrics.waiting_at(at(5)),
            Some(Waiting {
                next: 0,
                through: 0.25
            })
        );
        assert_eq!(lyrics.waiting_at(at(20)), None);
    }

    #[test]
    fn nothing_is_waited_on_once_the_last_line_has_had_its_time() {
        let lyrics = synced();

        assert_eq!(lyrics.waiting_at(at(19)), None);
        assert_eq!(lyrics.waiting_at(at(20)), None);
        assert_eq!(lyrics.waiting_at(at(600)), None);
    }

    #[test]
    fn an_unsynced_set_never_claims_a_line_is_in_play() {
        let lyrics = Lyrics::plain(source(), vec!["all that you touch".to_owned()]);

        assert_eq!(lyrics.timing(), Timing::Unsynced);
        assert_eq!(lyrics.line_at(at(30)), None);
        assert_eq!(lyrics.line_in_play(at(30)), None);
        assert_eq!(lyrics.waiting_at(at(30)), None);
        assert!(!lyrics.is_empty());
    }

    #[test]
    fn a_set_of_nothing_but_blank_lines_is_no_lyrics_at_all() {
        let lyrics = Lyrics::plain(source(), vec![String::new(), "   ".to_owned()]);

        assert!(lyrics.is_empty());
        assert!(Lyrics::plain(source(), Vec::new()).is_empty());
    }
}
