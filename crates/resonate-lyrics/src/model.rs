use std::{ops::Range, time::Duration};

use resonate_core::SourceId;

use crate::{Error, Result};

pub const MOST_LINES: usize = 20_000;

const LIT_AT_MOST: Duration = Duration::from_secs(10);

const SUNG_AT_LEAST: Duration = Duration::from_secs(3);

const SUNG_BEFORE_THE_WORDS: Duration = Duration::from_millis(1_500);

const SUNG_PER_LETTER: Duration = Duration::from_millis(110);

const A_BREATH_AT_LEAST: Duration = Duration::from_secs(3);

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Detail {
    Unsynced,
    Lines,
    Words,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Sweep {
    pub sung: usize,
    pub singing: Option<Singing>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Singing {
    pub word: Range<usize>,
    pub through: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SungWord {
    pub at: Duration,
    pub until: Option<Duration>,
    pub text: String,
}

impl SungWord {
    pub fn sung(at: Duration, text: impl Into<String>) -> Self {
        Self {
            at,
            until: None,
            text: text.into(),
        }
    }

    #[must_use]
    pub fn ending(mut self, until: Duration) -> Self {
        self.until = (until >= self.at).then_some(until);
        self
    }

    fn shifted_back(self, by: Duration) -> Self {
        Self {
            at: self.at.saturating_sub(by),
            until: self.until.map(|until| until.saturating_sub(by)),
            text: self.text,
        }
    }
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
    pub until: Option<Duration>,
    pub text: String,
    pub voice: Voice,
    pub words: Vec<SungWord>,
}

impl LyricLine {
    pub fn sung(at: Duration, text: impl Into<String>) -> Self {
        Self {
            at: Some(at),
            until: None,
            text: text.into(),
            voice: Voice::One,
            words: Vec::new(),
        }
    }

    pub fn untimed(text: impl Into<String>) -> Self {
        Self {
            at: None,
            until: None,
            text: text.into(),
            voice: Voice::One,
            words: Vec::new(),
        }
    }

    pub fn worded(at: Duration, mut words: Vec<SungWord>) -> Self {
        words.sort_by_key(|word| word.at);
        let text = words.iter().map(|word| word.text.as_str()).collect();

        Self {
            at: Some(at),
            until: None,
            text,
            voice: Voice::One,
            words,
        }
    }

    #[must_use]
    pub fn voiced(mut self, voice: Voice) -> Self {
        self.voice = voice;
        self
    }

    #[must_use]
    pub fn ending(mut self, until: Duration) -> Self {
        self.until = self.at.is_some_and(|at| until >= at).then_some(until);
        self
    }

    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn sweep_at(&self, position: Duration) -> Option<Sweep> {
        if self.words.is_empty() {
            return None;
        }
        let mut sung = 0;
        let mut starts = 0;
        for (index, word) in self.words.iter().enumerate() {
            let ends = starts + word.text.len();
            if position < word.at {
                break;
            }
            let until = word
                .until
                .or_else(|| self.words.get(index + 1).map(|next| next.at))
                .or(self.until);
            match until {
                Some(until) if position < until => {
                    return Some(Sweep {
                        sung,
                        singing: Some(Singing {
                            word: starts..ends,
                            through: share(position.saturating_sub(word.at), until - word.at),
                        }),
                    });
                }
                _ => sung = ends,
            }
            starts = ends;
        }

        Some(Sweep {
            sung,
            singing: None,
        })
    }

    fn declared_until(&self) -> Option<Duration> {
        self.until
            .or_else(|| self.words.iter().filter_map(|word| word.until).max())
    }

    fn shifted_back(self, by: Duration) -> Self {
        Self {
            at: self.at.map(|at| at.saturating_sub(by)),
            until: self.until.map(|until| until.saturating_sub(by)),
            text: self.text,
            voice: self.voice,
            words: self
                .words
                .into_iter()
                .map(|word| word.shifted_back(by))
                .collect(),
        }
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

    pub fn plain(source: SourceId, lines: impl IntoIterator<Item = String>) -> Self {
        Self {
            source,
            timing: Timing::Unsynced,
            lines: lines
                .into_iter()
                .take(MOST_LINES)
                .map(LyricLine::untimed)
                .collect(),
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

    pub fn detail(&self) -> Detail {
        match self.timing {
            Timing::Unsynced => Detail::Unsynced,
            Timing::Synced if self.lines.iter().any(|line| !line.words.is_empty()) => Detail::Words,
            Timing::Synced => Detail::Lines,
        }
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
                inside.then(|| line.shifted_back(start))
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

        let passed = self.passed(position);
        let mut seen = [false; 2];
        for index in (0..passed).rev() {
            let line = &self.lines[index];
            let voice = line.voice.index();
            if seen[voice] {
                continue;
            }
            seen[voice] = true;
            let (_, until) = self.span_of(index).expect("a synced line has a span");
            if position < until && !line.is_blank() {
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

        let passed = self.passed(position);
        let next = passed
            + self.lines[passed..]
                .iter()
                .position(|line| !line.is_blank())?;
        let arrives = self.lines[next].at?;
        let Some(went_out) = self.went_out_before(passed) else {
            return Some(Waiting {
                next,
                through: share(position, arrives),
            });
        };
        let wait = arrives.saturating_sub(went_out);
        if wait < A_BREATH_AT_LEAST {
            return None;
        }

        Some(Waiting {
            next,
            through: share(position.saturating_sub(went_out), wait),
        })
    }

    pub fn breathes_before(&self, line: usize) -> bool {
        if self.timing == Timing::Unsynced {
            return false;
        }
        let Some(arrives) = self
            .lines
            .get(line)
            .filter(|next| !next.is_blank())
            .and_then(|next| next.at)
        else {
            return false;
        };
        let Some(went_out) = self.went_out_before(line) else {
            return true;
        };

        arrives.saturating_sub(went_out) >= A_BREATH_AT_LEAST
    }

    fn went_out_before(&self, line: usize) -> Option<Duration> {
        [Voice::One, Voice::Two]
            .into_iter()
            .filter_map(|voice| {
                self.lines[..line]
                    .iter()
                    .rposition(|earlier| earlier.voice == voice && !earlier.is_blank())
            })
            .filter_map(|sung| self.span_of(sung))
            .map(|(_, until)| until)
            .max()
    }

    pub fn has_ended(&self, position: Duration) -> bool {
        if self.timing == Timing::Unsynced || self.is_empty() {
            return false;
        }
        let passed = self.passed(position);

        self.lines[passed..].iter().all(LyricLine::is_blank)
            && self.line_in_play(position).is_none()
    }

    fn passed(&self, position: Duration) -> usize {
        self.lines
            .partition_point(|line| line.at.is_some_and(|at| at <= position))
    }

    fn span_of(&self, line: usize) -> Option<(Duration, Duration)> {
        let this = self.lines.get(line)?;
        let at = this.at?;
        if let Some(until) = this.declared_until() {
            return Some((at, until.max(at)));
        }
        let later = &self.lines[line + 1..];
        let held = later
            .iter()
            .find(|next| next.voice == this.voice)
            .and_then(|next| next.at)
            .unwrap_or(Duration::MAX);
        let sung = at.saturating_add(sung_for(&this.text));
        let answered = later
            .iter()
            .filter(|next| !next.is_blank())
            .filter_map(|next| next.at)
            .find(|next| *next >= sung)
            .unwrap_or(Duration::MAX);
        let until = [held, answered]
            .into_iter()
            .find(|next| next.saturating_sub(sung) < A_BREATH_AT_LEAST)
            .map_or(sung, |next| next.min(at.saturating_add(LIT_AT_MOST)));

        Some((at, until))
    }
}

fn sung_for(text: &str) -> Duration {
    let letters = u32::try_from(text.trim().chars().count()).unwrap_or(u32::MAX);

    SUNG_BEFORE_THE_WORDS
        .saturating_add(SUNG_PER_LETTER.saturating_mul(letters))
        .clamp(SUNG_AT_LEAST, LIT_AT_MOST)
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
    fn two_voices_stay_in_play_until_their_own_next_lines_or_until_they_have_been_sung() {
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
        assert_eq!(lyrics.voices_in_play(at(4)), [Some(0), Some(1)]);
        assert_eq!(lyrics.voices_in_play(at(6)), [Some(2), None]);
        assert_eq!(lyrics.voices_in_play(at(8)), [Some(2), Some(3)]);
        assert_eq!(lyrics.line_in_play(at(8)), Some(3));
        assert_eq!(lyrics.voices_in_play(at(20)), [None, None]);
    }

    #[test]
    fn a_line_waits_for_the_other_voice_rather_than_leaving_less_than_a_breath_unlit() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(4), "first singer opens the song"),
                LyricLine::sung(millis(9_500), "the second answers").voiced(Voice::Two),
                LyricLine::sung(at(17), "first singer again"),
                LyricLine::sung(at(30), "second singer again").voiced(Voice::Two),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.voices_in_play(millis(9_000)), [Some(0), None]);
        assert_eq!(lyrics.voices_in_play(millis(9_500)), [None, Some(1)]);
        assert_eq!(lyrics.waiting_at(millis(9_000)), None);
        assert_eq!(
            lyrics.voices_in_play(at(15)),
            [None, None],
            "a line was held across more than a breath"
        );
        assert_eq!(
            lyrics.waiting_at(at(15)).map(|waiting| waiting.next),
            Some(2)
        );
    }

    #[test]
    fn a_wait_counts_from_whichever_voice_went_out_last() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(10), "a first singer holds a long long line"),
                LyricLine::sung(at(11), "short").voiced(Voice::Two),
                LyricLine::sung(at(40), "and then the next"),
            ],
        )
        .expect("every line is timed");
        let went_out = at(10) + sung_for("a first singer holds a long long line");
        let waiting = lyrics
            .waiting_at(went_out)
            .expect("a pause after both voices went out");

        assert_eq!(waiting.next, 2);
        assert!(
            waiting.through.abs() < f32::EPSILON,
            "the dots began {} of the way through",
            waiting.through
        );
        assert!(lyrics.breathes_before(2));
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

    fn millis(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn stay_until_the_morning() -> LyricLine {
        LyricLine::worded(
            millis(4_200),
            vec![
                SungWord::sung(millis(4_200), "Stay ").ending(millis(4_800)),
                SungWord::sung(millis(4_800), "until ").ending(millis(5_400)),
                SungWord::sung(millis(5_400), "the ").ending(millis(5_750)),
                SungWord::sung(millis(5_750), "morning").ending(millis(6_800)),
            ],
        )
        .ending(millis(6_800))
    }

    #[test]
    fn a_worded_line_reads_as_its_words_joined_and_sweeps_through_them_as_they_are_sung() {
        let line = stay_until_the_morning();
        assert_eq!(line.text, "Stay until the morning");

        assert_eq!(
            line.sweep_at(millis(4_000)),
            Some(Sweep {
                sung: 0,
                singing: None
            })
        );
        assert_eq!(
            line.sweep_at(millis(4_500)),
            Some(Sweep {
                sung: 0,
                singing: Some(Singing {
                    word: 0..5,
                    through: 0.5
                })
            })
        );
        assert_eq!(
            line.sweep_at(millis(5_400)),
            Some(Sweep {
                sung: 11,
                singing: Some(Singing {
                    word: 11..15,
                    through: 0.0
                })
            })
        );
        assert_eq!(
            line.sweep_at(millis(9_000)),
            Some(Sweep {
                sung: 22,
                singing: None
            })
        );
        assert_eq!(
            LyricLine::sung(millis(0), "no words").sweep_at(millis(1)),
            None
        );
    }

    #[test]
    fn a_word_with_no_end_of_its_own_is_sung_until_the_next_word_or_the_line_ends() {
        let line = LyricLine::worded(
            at(0),
            vec![SungWord::sung(at(0), "all "), SungWord::sung(at(2), "that")],
        )
        .ending(at(6));

        let at_one = line.sweep_at(at(1)).expect("a worded line");
        assert_eq!(at_one.sung, 0);
        assert_eq!(
            at_one.singing,
            Some(Singing {
                word: 0..4,
                through: 0.5
            })
        );
        let at_four = line.sweep_at(at(4)).expect("a worded line");
        assert_eq!(at_four.sung, 4);
        assert_eq!(
            at_four.singing,
            Some(Singing {
                word: 4..8,
                through: 0.5
            })
        );
    }

    #[test]
    fn a_line_the_sheet_ends_is_lit_exactly_as_long_as_the_sheet_says() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "oh").ending(at(9)),
                LyricLine::sung(at(30), "all that you touch"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.line_in_play(at(8)), Some(0));
        assert_eq!(lyrics.line_in_play(at(9)), None);
        assert_eq!(lyrics.detail(), Detail::Lines);
        assert_eq!(
            LyricLine::sung(at(5), "backwards").ending(at(4)).until,
            None,
            "an end before the start was kept"
        );
    }

    #[test]
    fn cutting_a_cue_row_carries_its_words_and_its_end_onto_the_row_clock() {
        let lyrics = Lyrics::synced(source(), vec![stay_until_the_morning()])
            .expect("every line is timed")
            .within(at(4), None)
            .expect("the line is inside the cut");
        let line = &lyrics.lines()[0];

        assert_eq!(lyrics.detail(), Detail::Words);
        assert_eq!(line.at, Some(millis(200)));
        assert_eq!(line.until, Some(millis(2_800)));
        assert_eq!(line.words[3].at, millis(1_750));
        assert_eq!(line.words[3].until, Some(millis(2_800)));
        assert_eq!(line.text, "Stay until the morning");
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

        assert_eq!(lyrics.line_in_play(at(3)), Some(0));
        assert_eq!(lyrics.line_in_play(at(4)), None);
        assert_eq!(lyrics.line_in_play(at(119)), None);
        assert_eq!(lyrics.line_at(at(119)), Some(0));
        assert_eq!(lyrics.line_in_play(at(120)), Some(1));
    }

    #[test]
    fn the_last_line_goes_out_once_it_has_had_its_time() {
        let lyrics = synced();

        assert_eq!(lyrics.line_in_play(at(15)), Some(2));
        assert_eq!(lyrics.line_in_play(at(16)), None);
        assert!(!lyrics.has_ended(at(15)));
        assert!(lyrics.has_ended(at(16)));
        assert!(lyrics.has_ended(at(600)));
    }

    #[test]
    fn a_longer_line_is_held_lit_for_longer() {
        assert_eq!(sung_for("oh"), SUNG_AT_LEAST);
        assert!(sung_for("and everything under the sun is in tune") > sung_for("all that you see"));
        assert_eq!(sung_for(&"la ".repeat(200)), LIT_AT_MOST);
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
                LyricLine::sung(at(0), "touch"),
                LyricLine::sung(at(33), "all that you see"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.waiting_at(at(2)), None);
        assert_eq!(
            lyrics.waiting_at(at(3)),
            Some(Waiting {
                next: 1,
                through: 0.0
            })
        );
        assert_eq!(
            lyrics.waiting_at(at(18)),
            Some(Waiting {
                next: 1,
                through: 0.5
            })
        );
        assert_eq!(lyrics.waiting_at(at(33)), None);
    }

    #[test]
    fn a_pause_mid_song_breathes_where_a_short_one_does_not() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "touch"),
                LyricLine::sung(at(5), "see"),
                LyricLine::sung(at(60), "taste"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.line_in_play(at(4)), Some(0));
        assert_eq!(lyrics.waiting_at(at(4)), None);
        assert_eq!(lyrics.line_in_play(at(7)), Some(1));
        assert_eq!(lyrics.line_in_play(at(9)), None);
        assert_eq!(
            lyrics.waiting_at(at(9)).map(|waiting| waiting.next),
            Some(2),
            "a long pause between two verses did not breathe"
        );
    }

    #[test]
    fn a_blank_line_between_verses_is_a_pause_rather_than_a_line_in_play() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(0), "and everything under the sun is in tune"),
                LyricLine::sung(at(4), ""),
                LyricLine::sung(at(20), "but the sun is eclipsed by the moon"),
            ],
        )
        .expect("every line is timed");

        assert_eq!(lyrics.line_in_play(at(3)), Some(0));
        assert_eq!(lyrics.line_in_play(at(5)), None);
        assert_eq!(
            lyrics.waiting_at(at(12)),
            Some(Waiting {
                next: 2,
                through: 0.5
            }),
            "the pause a blank line marks did not count down to the verse after it"
        );
        assert!(!lyrics.has_ended(at(12)));
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
    fn a_line_breathes_before_it_exactly_where_a_wait_would_count_down_to_it() {
        let lyrics = Lyrics::synced(
            source(),
            vec![
                LyricLine::sung(at(20), "touch"),
                LyricLine::sung(at(24), "see"),
                LyricLine::sung(at(40), ""),
                LyricLine::sung(at(60), "taste"),
            ],
        )
        .expect("every line is timed");

        assert!(lyrics.breathes_before(0));
        assert!(!lyrics.breathes_before(1));
        assert!(!lyrics.breathes_before(2));
        assert!(lyrics.breathes_before(3));
        assert!(!lyrics.breathes_before(9));
        for (position, line) in [(at(5), 0), (at(50), 3)] {
            assert_eq!(
                lyrics.waiting_at(position).map(|waiting| waiting.next),
                Some(line)
            );
        }
        assert!(
            !Lyrics::plain(source(), vec!["touch".to_owned()]).breathes_before(0),
            "an unsynced set breathed"
        );
    }

    #[test]
    fn nothing_is_waited_on_once_the_last_line_has_had_its_time() {
        let lyrics = synced();

        assert_eq!(lyrics.waiting_at(at(15)), None);
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
        assert!(!lyrics.has_ended(at(30)));
        assert!(!lyrics.is_empty());
    }

    #[test]
    fn a_set_of_nothing_but_blank_lines_is_no_lyrics_at_all() {
        let lyrics = Lyrics::plain(source(), vec![String::new(), "   ".to_owned()]);

        assert!(lyrics.is_empty());
        assert!(Lyrics::plain(source(), Vec::new()).is_empty());
        assert_eq!(
            Lyrics::plain(
                source(),
                std::iter::repeat_n("la".to_owned(), MOST_LINES * 4)
            )
            .lines()
            .len(),
            MOST_LINES
        );
    }
}
