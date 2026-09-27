use std::{iter, time::Duration};

use resonate_core::SourceId;

use crate::{Credits, Error, LyricLine, LyricOp, Lyrics, Result, SungWord, Voice, Wanted};

const SECONDS_PER_MINUTE: u64 = 60;
const MINUTES_PER_HOUR: u64 = 60;
const COLONS_WHEN_HOURS_ARE_WRITTEN: usize = 2;
const NANOSECOND_DIGITS: usize = 9;
pub(crate) const LARGEST_SHEET: usize = 512 * 1024;
const LARGEST_SET: usize = 1024 * 1024;
const MOST_LINES: usize = 20_000;
const LENGTH_MAY_DIFFER_BY: Duration = Duration::from_secs(30);

pub(crate) struct Sheet {
    pub(crate) declared: Declared,
    pub(crate) lyrics: Option<Lyrics>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Identified {
    Title,
    Artist,
    Album,
    Author,
    Transcriber,
    Editor,
    Version,
    Length,
    Offset,
}

impl Identified {
    const NAMED: [(&'static str, Self); 10] = [
        ("ti", Self::Title),
        ("ar", Self::Artist),
        ("al", Self::Album),
        ("au", Self::Author),
        ("by", Self::Transcriber),
        ("re", Self::Editor),
        ("tool", Self::Editor),
        ("ve", Self::Version),
        ("length", Self::Length),
        ("offset", Self::Offset),
    ];

    fn read(name: &str) -> Option<Self> {
        Self::NAMED
            .iter()
            .find(|(tag, _)| name.eq_ignore_ascii_case(tag))
            .map(|(_, identified)| *identified)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Declared {
    title: Option<String>,
    artist: Option<String>,
    length: Option<Duration>,
}

impl Declared {
    pub(crate) fn names_another_track(&self, wanted: &Wanted) -> bool {
        disagree(self.title.as_deref(), wanted.title.as_deref())
            || disagree(self.artist.as_deref(), wanted.artist.as_deref())
            || runs_for_another_length(self.length, wanted.duration)
    }
}

fn disagree(declared: Option<&str>, wanted: Option<&str>) -> bool {
    let (Some(declared), Some(wanted)) = (declared, wanted) else {
        return false;
    };
    let (declared, wanted) = (folded(declared), folded(wanted));
    if declared.is_empty() || wanted.is_empty() {
        return false;
    }

    !declared.contains(&wanted) && !wanted.contains(&declared)
}

fn runs_for_another_length(declared: Option<Duration>, wanted: Option<Duration>) -> bool {
    let (Some(declared), Some(wanted)) = (declared, wanted) else {
        return false;
    };

    declared.abs_diff(wanted) > LENGTH_MAY_DIFFER_BY
}

pub(crate) fn folded(text: &str) -> String {
    text.chars()
        .filter(|glyph| glyph.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn read(source: SourceId, text: &str) -> Result<Sheet> {
    if text.len() > LARGEST_SHEET {
        return Err(beyond_what_a_sheet_holds(source));
    }

    let mut reading = Reading::new(source);
    for line in text.lines() {
        reading.absorb(line)?;
    }
    reading.finish()
}

fn beyond_what_a_sheet_holds(provider: SourceId) -> Error {
    Error::Unreadable {
        provider,
        op: LyricOp::Parse,
    }
}

struct Stamped {
    text: String,
    words: Vec<(Duration, String)>,
    ends: Option<Duration>,
}

impl Stamped {
    fn read(line: &str) -> Option<Self> {
        let mut words: Vec<(Duration, String)> = Vec::new();
        let mut lead = String::new();
        let mut gathering = String::new();
        let mut sung_from: Option<Duration> = None;
        let mut stamped = false;
        let mut rest = line;
        while let Some(opens) = rest.find('<') {
            let (before, after) = rest.split_at(opens);
            gathering.push_str(before);
            let stamp = after[1..]
                .split_once('>')
                .and_then(|(inside, tail)| Some((moment(inside).or_else(|| span(inside))?, tail)));
            let Some((at, tail)) = stamp else {
                gathering.push('<');
                rest = &after[1..];
                continue;
            };
            stamped = true;
            let word = std::mem::take(&mut gathering);
            match sung_from {
                Some(from) if !word.is_empty() => words.push((from, word)),
                Some(_) => {}
                None => lead.push_str(&word),
            }
            sung_from = Some(at);
            rest = tail;
        }
        if !stamped {
            return None;
        }
        gathering.push_str(rest);
        let mut ends = None;
        match sung_from {
            Some(from) if !gathering.trim().is_empty() => words.push((from, gathering)),
            Some(at) => ends = Some(at),
            None => {}
        }
        if let Some((_, first)) = words.first_mut() {
            *first = format!("{lead}{first}").trim_start().to_owned();
        }
        for at in 1..words.len() {
            if words[at - 1].1.ends_with(char::is_whitespace) {
                let spaced = words[at].1.trim_start().to_owned();
                words[at].1 = spaced;
            }
        }
        if let Some((_, last)) = words.last_mut() {
            last.truncate(last.trim_end().len());
        }
        let text = if words.is_empty() {
            lead.trim().to_owned()
        } else {
            words.iter().map(|(_, word)| word.as_str()).collect()
        };

        Some(Self { text, words, ends })
    }

    fn first_sung(&self) -> Option<Duration> {
        self.words.first().map(|(at, _)| *at)
    }
}

struct Timed {
    at: Duration,
    text: String,
    voice: Voice,
    words: Vec<(Duration, String)>,
    ends: Option<Duration>,
}

fn moved(at: Duration, from: Duration, to: Duration) -> Duration {
    if to >= from {
        at.saturating_add(to - from)
    } else {
        at.saturating_sub(from - to)
    }
}

struct Reading {
    source: SourceId,
    timed: Vec<Timed>,
    plain: Vec<String>,
    declared: Declared,
    credits: Credits,
    shift: i64,
    held: usize,
}

impl Reading {
    fn new(source: SourceId) -> Self {
        Self {
            source,
            timed: Vec::new(),
            plain: Vec::new(),
            declared: Declared::default(),
            credits: Credits::default(),
            shift: 0,
            held: 0,
        }
    }

    fn hold(&mut self, text: &str) -> Result<()> {
        self.held = self.held.saturating_add(text.len());
        let lines = self.timed.len() + self.plain.len();
        if self.held > LARGEST_SET || lines >= MOST_LINES {
            return Err(beyond_what_a_sheet_holds(self.source.clone()));
        }

        Ok(())
    }

    fn absorb(&mut self, line: &str) -> Result<()> {
        let mut rest = line.trim_start_matches('\u{feff}').trim();
        let mut moments = Vec::new();
        let mut identified = false;

        while let Some((inside, tail)) = brackets(rest) {
            match Bracket::read(inside) {
                Bracket::Moment(at) => moments.push(at),
                Bracket::Identifying { tag, value } => {
                    self.identify(tag, value);
                    identified = true;
                }
                Bracket::Text => break,
            }
            rest = tail.trim_start();
        }

        let text = rest.trim();
        let (voice, text) = voiced_text(text);
        let stamped = Stamped::read(text);
        if moments.is_empty()
            && let Some(first) = stamped.as_ref().and_then(Stamped::first_sung)
        {
            moments.push(first);
        }
        if !moments.is_empty() {
            let first = moments[0];
            let text = stamped
                .as_ref()
                .map_or(text, |stamped| stamped.text.as_str());
            for at in moments {
                self.hold(text)?;
                let (words, ends) = match &stamped {
                    Some(stamped) => (
                        stamped
                            .words
                            .iter()
                            .map(|(sung, word)| (moved(*sung, first, at), word.clone()))
                            .collect(),
                        stamped.ends.map(|ends| moved(ends, first, at)),
                    ),
                    None => (Vec::new(), None),
                };
                self.timed.push(Timed {
                    at,
                    text: text.to_owned(),
                    voice,
                    words,
                    ends,
                });
            }
            return Ok(());
        }
        let text = rest.trim();
        if !identified && !text.is_empty() {
            self.hold(text)?;
            self.plain.push(text.to_owned());
        }

        Ok(())
    }

    fn identify(&mut self, tag: Identified, value: &str) {
        let value = value.trim();
        if value.is_empty() {
            return;
        }
        match tag {
            Identified::Offset => {
                if let Ok(shift) = value.parse() {
                    self.shift = shift;
                }
            }
            Identified::Length => {
                if let Some(length) = span(value) {
                    self.declared.length = Some(length);
                }
            }
            Identified::Title => self.declared.title = Some(value.to_owned()),
            Identified::Artist => self.declared.artist = Some(value.to_owned()),
            Identified::Album => self.credits.album = Some(value.to_owned()),
            Identified::Author => self.credits.words_by = Some(value.to_owned()),
            Identified::Transcriber => self.credits.sheet_by = Some(value.to_owned()),
            Identified::Editor => self.credits.editor = Some(value.to_owned()),
            Identified::Version => self.credits.version = Some(value.to_owned()),
        }
    }

    fn finish(self) -> Result<Sheet> {
        let Self {
            source,
            timed,
            plain,
            declared,
            credits,
            shift,
            held: _,
        } = self;

        if timed.is_empty() {
            return Ok(Sheet {
                declared,
                lyrics: worth_drawing(Lyrics::plain(source, plain).credited(credits)),
            });
        }
        let lines = timed
            .into_iter()
            .map(|timed| {
                let at = shifted(timed.at, shift);
                let line = if timed.words.is_empty() {
                    LyricLine::sung(at, timed.text)
                } else {
                    LyricLine::worded(
                        at,
                        timed
                            .words
                            .into_iter()
                            .map(|(sung, word)| SungWord::sung(shifted(sung, shift), word))
                            .collect(),
                    )
                };
                let line = match timed.ends {
                    Some(ends) => line.ending(shifted(ends, shift)),
                    None => line,
                };

                line.voiced(timed.voice)
            })
            .collect();

        Ok(Sheet {
            declared,
            lyrics: worth_drawing(Lyrics::synced(source, lines)?.credited(credits)),
        })
    }
}

fn voiced_text(text: &str) -> (Voice, &str) {
    for (marker, voice) in [("[v1:", Voice::One), ("[v2:", Voice::Two)] {
        if let Some(words) = text
            .strip_prefix(marker)
            .and_then(|words| words.strip_suffix(']'))
        {
            return (voice, words.trim());
        }
    }

    (Voice::One, text)
}

fn worth_drawing(lyrics: Lyrics) -> Option<Lyrics> {
    (!lyrics.is_empty()).then_some(lyrics)
}

fn shifted(at: Duration, shift: i64) -> Duration {
    let by = Duration::from_millis(shift.unsigned_abs());

    if shift.is_negative() {
        at.saturating_add(by)
    } else {
        at.saturating_sub(by)
    }
}

enum Bracket<'a> {
    Moment(Duration),
    Identifying { tag: Identified, value: &'a str },
    Text,
}

impl<'a> Bracket<'a> {
    fn read(inside: &'a str) -> Self {
        if let Some(at) = moment(inside).or_else(|| span(inside)) {
            return Self::Moment(at);
        }
        let Some((name, value)) = inside.split_once(':') else {
            return Self::Text;
        };
        let Some(tag) = Identified::read(name.trim()) else {
            return Self::Text;
        };

        Self::Identifying { tag, value }
    }
}

fn brackets(line: &str) -> Option<(&str, &str)> {
    line.strip_prefix('[')?.split_once(']')
}

fn span(text: &str) -> Option<Duration> {
    let text = text.trim();
    let colons = text.bytes().filter(|byte| *byte == b':').count();
    if colons < COLONS_WHEN_HOURS_ARE_WRITTEN {
        return moment(text);
    }
    let (hours, rest) = text.split_once(':')?;
    let hours = whole(hours)?
        .checked_mul(MINUTES_PER_HOUR)?
        .checked_mul(SECONDS_PER_MINUTE)?;

    moment(rest)?.checked_add(Duration::from_secs(hours))
}

fn moment(inside: &str) -> Option<Duration> {
    let (minutes, rest) = inside.split_once(':')?;
    let (seconds, fraction) = match rest.split_once(['.', ':']) {
        Some((seconds, fraction)) => (seconds, Some(fraction)),
        None => (rest, None),
    };
    let nanoseconds = match fraction {
        Some(fraction) => sub_second(fraction)?,
        None => 0,
    };

    let seconds = whole(minutes)?
        .checked_mul(SECONDS_PER_MINUTE)?
        .checked_add(whole(seconds)?)?;

    Some(Duration::new(seconds, nanoseconds))
}

fn whole(text: &str) -> Option<u64> {
    decimal(text)?.parse().ok()
}

fn sub_second(text: &str) -> Option<u32> {
    let scaled: String = decimal(text)?
        .chars()
        .chain(iter::repeat('0'))
        .take(NANOSECOND_DIGITS)
        .collect();

    scaled.parse().ok()
}

fn decimal(text: &str) -> Option<&str> {
    let text = text.trim();
    let all_digits = !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());

    all_digits.then_some(text)
}

#[cfg(test)]
mod tests {
    use resonate_core::MediaLocation;

    use super::*;
    use crate::Timing;

    fn source() -> SourceId {
        SourceId::new("held").expect("a lowercase name")
    }

    fn sheet(text: &str) -> Sheet {
        read(source(), text).expect("nothing failed")
    }

    fn lyrics(text: &str) -> Option<Lyrics> {
        sheet(text).lyrics
    }

    fn about(title: Option<&str>, artist: Option<&str>) -> Wanted {
        Wanted {
            title: title.map(str::to_owned),
            artist: artist.map(str::to_owned),
            ..Wanted::for_media(MediaLocation::local("/music/Echoes.flac"))
        }
    }

    fn lasting(seconds: u64) -> Wanted {
        Wanted {
            duration: Some(Duration::from_secs(seconds)),
            ..Wanted::for_media(MediaLocation::local("/music/Echoes.flac"))
        }
    }

    fn credits(text: &str) -> Credits {
        sheet(text).lyrics.expect("a set").credits().clone()
    }

    fn plain(lyrics: &Lyrics) -> Vec<&str> {
        lyrics
            .lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    }

    fn timed(lyrics: &Lyrics) -> Vec<(Duration, &str)> {
        lyrics
            .lines()
            .iter()
            .map(|line| (line.at.expect("a timed line"), line.text.as_str()))
            .collect()
    }

    fn refused(text: &str) -> bool {
        matches!(read(source(), text), Err(Error::Unreadable { .. }))
    }

    fn millis(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    fn spelled(line: &LyricLine) -> Vec<(&str, Duration)> {
        line.words
            .iter()
            .map(|word| (word.text.as_str(), word.at))
            .collect()
    }

    #[test]
    fn a_line_stamped_word_by_word_is_read_as_its_words_and_ends_where_the_last_stamp_says() {
        let lyrics = lyrics(
            "[00:12.00]<00:12.00>Overhead, <00:12.60>the <00:12.90>albatross<00:14.10>\n\
             [00:15.00]Hangs motionless",
        )
        .expect("the sheet holds lines");
        let line = &lyrics.lines()[0];

        assert_eq!(lyrics.detail(), crate::Detail::Words);
        assert_eq!(line.text, "Overhead, the albatross");
        assert_eq!(
            spelled(line),
            [
                ("Overhead, ", millis(12_000)),
                ("the ", millis(12_600)),
                ("albatross", millis(12_900))
            ]
        );
        assert_eq!(line.until, Some(millis(14_100)));
        assert!(lyrics.lines()[1].words.is_empty());
        assert_eq!(lyrics.lines()[1].text, "Hangs motionless");
    }

    #[test]
    fn stamps_written_with_spaces_around_them_and_no_closing_stamp_still_read_as_words() {
        let lyrics = lyrics("[00:01.00] <00:01.00> all <00:01.50> that you touch")
            .expect("the sheet holds lines");
        let line = &lyrics.lines()[0];

        assert_eq!(line.text, "all that you touch");
        assert_eq!(
            spelled(line),
            [("all ", millis(1_000)), ("that you touch", millis(1_500))]
        );
        assert_eq!(line.until, None);
    }

    #[test]
    fn a_line_of_stamps_with_no_moment_of_its_own_is_sung_from_its_first_word() {
        let lyrics = lyrics("<00:03.00>all <00:03.40>that\n<00:05.00>you <00:05.30>see")
            .expect("the sheet holds lines");

        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(lyrics.lines()[0].at, Some(millis(3_000)));
        assert_eq!(lyrics.lines()[1].at, Some(millis(5_000)));
        assert_eq!(lyrics.lines()[1].text, "you see");
    }

    #[test]
    fn a_stamped_line_sung_twice_carries_its_words_to_the_second_moment_and_the_offset_moves_both()
    {
        let lyrics =
            lyrics("[offset:500]\n[00:10.00][01:10.00]<00:10.00>la <00:10.50>la<00:11.00>")
                .expect("the sheet holds lines");

        assert_eq!(
            spelled(&lyrics.lines()[0]),
            [("la ", millis(9_500)), ("la", millis(10_000))]
        );
        assert_eq!(
            spelled(&lyrics.lines()[1]),
            [("la ", millis(69_500)), ("la", millis(70_000))]
        );
        assert_eq!(lyrics.lines()[1].until, Some(millis(70_500)));
    }

    #[test]
    fn syllables_an_id3_sylt_frame_times_are_read_as_the_words_of_their_line() {
        let lyrics = lyrics(
            "[00:01.000]<00:01.000>Over<00:01.400>head <00:01.900>the albatross\n\
             [00:05.000]<00:05.000>Hangs<00:05.600> motionless\n",
        )
        .expect("the sheet holds lines");

        assert_eq!(lyrics.lines()[0].text, "Overhead the albatross");
        assert_eq!(
            spelled(&lyrics.lines()[0]),
            [
                ("Over", millis(1_000)),
                ("head ", millis(1_400)),
                ("the albatross", millis(1_900))
            ]
        );
        assert_eq!(lyrics.lines()[1].text, "Hangs motionless");
    }

    #[test]
    fn a_voiced_line_carries_its_stamps_and_an_angle_that_is_no_stamp_is_text() {
        let lyrics =
            lyrics("[00:02.00][v2: <00:02.00>second <00:02.40>singer]\n[00:05.00]I <3 you <b>")
                .expect("the sheet holds lines");

        assert_eq!(lyrics.lines()[0].voice, Voice::Two);
        assert_eq!(lyrics.lines()[0].text, "second singer");
        assert_eq!(lyrics.lines()[0].words.len(), 2);
        assert_eq!(lyrics.lines()[1].text, "I <3 you <b>");
        assert!(lyrics.lines()[1].words.is_empty());
    }

    #[test]
    fn a_sheet_larger_than_any_lyric_is_refused_before_a_line_of_it_is_read() {
        assert!(refused(&"all that you touch\n".repeat(64 * 1024)));
    }

    #[test]
    fn a_line_carrying_more_moments_than_a_set_holds_is_refused_rather_than_repeated() {
        let repeated = format!("{}all that you touch", "[00:00.00]".repeat(50_000));

        assert!(repeated.len() <= LARGEST_SHEET);
        assert!(refused(&repeated));
    }

    #[test]
    fn a_timestamped_file_is_read_as_a_synced_set_in_the_order_it_is_sung() {
        let lyrics = lyrics("[00:10.00]and everything under the sun\n[00:05.50]all that you see\n[00:00.00]all that you touch\n")
            .expect("a set");

        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(
            timed(&lyrics),
            [
                (Duration::ZERO, "all that you touch"),
                (Duration::from_millis(5_500), "all that you see"),
                (Duration::from_secs(10), "and everything under the sun"),
            ]
        );
    }

    #[test]
    fn voice_markers_are_read_without_becoming_lyric_text() {
        let lyrics = lyrics(
            "[00:01.00][v1: First singer]\n[00:02.00][v2: Second singer]\n[00:03.00]Together",
        )
        .expect("a set");

        assert_eq!(
            plain(&lyrics),
            ["First singer", "Second singer", "Together"]
        );
        assert_eq!(
            lyrics
                .lines()
                .iter()
                .map(|line| line.voice)
                .collect::<Vec<_>>(),
            [Voice::One, Voice::Two, Voice::One]
        );
    }

    #[test]
    fn several_moments_in_front_of_one_line_sing_it_at_each_of_them() {
        let lyrics = lyrics("[00:04.00][01:04.00]all that you touch").expect("a set");

        assert_eq!(
            timed(&lyrics),
            [
                (Duration::from_secs(4), "all that you touch"),
                (Duration::from_secs(64), "all that you touch"),
            ]
        );
    }

    #[test]
    fn a_moment_is_read_whether_its_fraction_is_hundredths_thousandths_or_absent() {
        let lyrics =
            lyrics("[00:01]one\n[00:02.5]two\n[00:03.25]three\n[00:04:75]four\n[00:05.125]five")
                .expect("a set");

        assert_eq!(
            timed(&lyrics).iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            [
                Duration::from_secs(1),
                Duration::from_millis(2_500),
                Duration::from_millis(3_250),
                Duration::from_millis(4_750),
                Duration::from_millis(5_125),
            ]
        );
    }

    #[test]
    fn a_moment_past_the_hour_is_read_with_its_hours_where_its_fraction_says_so() {
        let lyrics =
            lyrics("[59:59.50]before\n[1:02:03.45]after\n[1:02:03]hundredths").expect("a set");

        assert_eq!(
            timed(&lyrics),
            [
                (Duration::from_millis(62_030), "hundredths"),
                (Duration::from_millis(3_599_500), "before"),
                (Duration::from_millis(3_723_450), "after"),
            ]
        );
    }

    #[test]
    fn a_metadata_bracket_is_not_a_line_and_its_text_is_not_sung() {
        let lyrics = lyrics("[ti:Echoes]\n[ar:Pink Floyd]\n[al:Meddle: remastered]\n[00:01.00]overhead the albatross")
            .expect("a set");

        assert_eq!(
            timed(&lyrics),
            [(Duration::from_secs(1), "overhead the albatross")]
        );
    }

    #[test]
    fn an_offset_shifts_every_moment_and_a_positive_one_sings_the_line_earlier() {
        let earlier = lyrics("[offset:+500]\n[00:10.00]all that you touch").expect("a set");
        let later = lyrics("[offset:-500]\n[00:10.00]all that you touch").expect("a set");

        assert_eq!(timed(&earlier)[0].0, Duration::from_millis(9_500));
        assert_eq!(timed(&later)[0].0, Duration::from_millis(10_500));
    }

    #[test]
    fn an_offset_larger_than_the_first_moment_lands_on_zero_rather_than_wrapping() {
        let lyrics = lyrics("[offset:+9000]\n[00:01.00]all that you touch").expect("a set");

        assert_eq!(timed(&lyrics)[0].0, Duration::ZERO);
    }

    #[test]
    fn a_file_with_no_moments_at_all_is_read_as_an_unsynced_set() {
        let lyrics = lyrics("all that you touch\n\nall that you see\n").expect("a set");

        assert_eq!(lyrics.timing(), Timing::Unsynced);
        assert_eq!(plain(&lyrics), ["all that you touch", "all that you see"]);
    }

    #[test]
    fn a_moment_with_nothing_behind_it_is_a_break_rather_than_a_line() {
        let lyrics = lyrics("[00:01.00]all that you touch\n[00:20.00]\n[00:30.00]all that you see")
            .expect("a set");

        assert_eq!(lyrics.lines().len(), 3);
        assert!(lyrics.lines()[1].is_blank());
    }

    #[test]
    fn a_file_holding_nothing_worth_drawing_answers_with_nothing() {
        assert!(lyrics("").is_none());
        assert!(lyrics("[ti:Echoes]\n[ar:Pink Floyd]\n").is_none());
        assert!(lyrics("[00:01.00]\n[00:02.00]   \n").is_none());
    }

    #[test]
    fn a_bracket_that_is_neither_a_moment_nor_an_id_tag_is_the_line_it_was_written_on() {
        let lyrics = lyrics("[Chorus]\nall that you touch\n[Verse 2: Roger]\nall that you see")
            .expect("a set");

        assert_eq!(
            plain(&lyrics),
            [
                "[Chorus]",
                "all that you touch",
                "[Verse 2: Roger]",
                "all that you see",
            ]
        );
    }

    #[test]
    fn a_marker_in_front_of_a_sung_line_is_sung_with_it() {
        let lyrics = lyrics("[00:01.00][Chorus] all that you touch").expect("a set");

        assert_eq!(
            timed(&lyrics),
            [(Duration::from_secs(1), "[Chorus] all that you touch")]
        );
    }

    #[test]
    fn an_id_tag_the_reader_keeps_is_still_not_a_line_of_its_own() {
        let sheet = sheet(
            "[by:a stranger]\n[re:an editor]\n[ve:1.0]\n[length:03:20]\n[00:01.00]overhead the \
             albatross",
        );
        let lyrics = sheet.lyrics.expect("a set");

        assert_eq!(
            timed(&lyrics),
            [(Duration::from_secs(1), "overhead the albatross")]
        );
        assert_eq!(lyrics.credits().sheet_by.as_deref(), Some("a stranger"));
        assert_eq!(sheet.declared.length, Some(Duration::from_secs(200)));
    }

    #[test]
    fn a_sheet_of_nothing_but_id_tags_holds_nothing_worth_drawing() {
        assert!(lyrics("[by:a stranger]\n[re:an editor]\n[ve:1.0]\n[length:03:20]").is_none());
    }

    #[test]
    fn what_a_sheet_says_about_its_own_making_is_kept_beside_the_lines() {
        let credits = credits(
            "[al:Meddle]\n[au:Roger Waters]\n[by:a stranger]\n[re:LRCGET]\n[ve:0.5]\n\
             [00:01.00]overhead the albatross",
        );

        assert_eq!(credits.album.as_deref(), Some("Meddle"));
        assert_eq!(credits.words_by.as_deref(), Some("Roger Waters"));
        assert_eq!(credits.sheet_by.as_deref(), Some("a stranger"));
        assert_eq!(credits.editor.as_deref(), Some("LRCGET"));
        assert_eq!(credits.version.as_deref(), Some("0.5"));
    }

    #[test]
    fn the_tool_tag_names_the_editor_the_way_the_re_tag_does() {
        let credits = credits("[tool:LRCGET]\n[00:01.00]overhead the albatross");

        assert_eq!(credits.editor.as_deref(), Some("LRCGET"));
    }

    #[test]
    fn a_sheet_that_credits_nobody_credits_nobody() {
        assert_eq!(
            credits("[ti:Echoes]\n[00:01.00]overhead the albatross"),
            Credits::default()
        );
    }

    #[test]
    fn a_declared_length_is_read_however_it_is_written() {
        for (written, milliseconds) in [
            ("03:20", 200_000),
            ("3:20.50", 200_500),
            ("0:59", 59_000),
            ("1:03:20", 3_800_000),
        ] {
            let sheet = sheet(&format!(
                "[length:{written}]\n[00:01.00]overhead the albatross"
            ));

            assert_eq!(
                sheet.declared.length,
                Some(Duration::from_millis(milliseconds)),
                "{written}"
            );
        }
    }

    #[test]
    fn a_length_no_arithmetic_can_hold_leaves_the_sheet_declaring_none() {
        for written in [
            "",
            "   ",
            "the whole side",
            "200",
            "99999999999999999999:01",
        ] {
            let sheet = sheet(&format!(
                "[length:{written}]\n[00:01.00]overhead the albatross"
            ));

            assert_eq!(sheet.declared.length, None, "{written}");
        }
    }

    #[test]
    fn a_sheet_running_to_another_length_is_not_about_this_track() {
        let sheet = sheet("[length:03:20]\n[00:01.00]overhead the albatross");

        assert!(sheet.declared.names_another_track(&lasting(23 * 60 + 31)));
    }

    #[test]
    fn a_length_rounded_to_the_second_by_one_and_ripped_by_the_other_still_agrees() {
        let sheet = sheet("[length:23:31]\n[00:01.00]overhead the albatross");

        assert!(!sheet.declared.names_another_track(&lasting(23 * 60 + 33)));
    }

    #[test]
    fn a_length_is_weighed_to_the_tolerance_the_reader_names_and_no_further() {
        let sheet = sheet("[length:03:20]\n[00:01.00]overhead the albatross");
        let declared = Duration::from_secs(200);
        let at_the_edge = (declared + LENGTH_MAY_DIFFER_BY).as_secs();

        assert!(!sheet.declared.names_another_track(&lasting(at_the_edge)));
        assert!(
            sheet
                .declared
                .names_another_track(&lasting(at_the_edge + 1))
        );
    }

    #[test]
    fn a_sheet_that_declares_no_length_is_about_whatever_it_sits_beside() {
        let sheet = sheet("[00:01.00]overhead the albatross");

        assert!(!sheet.declared.names_another_track(&lasting(200)));
    }

    #[test]
    fn a_track_whose_length_is_unknown_is_not_something_a_sheet_can_contradict() {
        let sheet = sheet("[length:03:20]\n[00:01.00]overhead the albatross");

        assert!(!sheet.declared.names_another_track(&about(None, None)));
    }

    #[test]
    fn what_a_sheet_says_it_is_comes_off_its_id_tags() {
        let sheet = sheet("[ti:Echoes]\n[AR:Pink Floyd]\n[00:01.00]overhead the albatross");

        assert_eq!(sheet.declared.title.as_deref(), Some("Echoes"));
        assert_eq!(sheet.declared.artist.as_deref(), Some("Pink Floyd"));
    }

    #[test]
    fn a_sheet_declaring_the_track_it_sits_beside_is_the_one_it_names() {
        let sheet = sheet("[ti:Echoes]\n[ar:Pink Floyd]\n[00:01.00]overhead the albatross");

        assert!(
            !sheet
                .declared
                .names_another_track(&about(Some("Echoes"), Some("Pink Floyd")))
        );
    }

    #[test]
    fn a_sheet_naming_another_song_is_not_about_this_track() {
        let sheet = sheet("[ti:Echoes]\n[00:01.00]overhead the albatross");

        assert!(
            sheet
                .declared
                .names_another_track(&about(Some("Time"), None))
        );
    }

    #[test]
    fn a_sheet_naming_another_singer_of_the_same_song_is_not_about_this_track() {
        let sheet = sheet("[ti:Hallelujah]\n[ar:Leonard Cohen]\n[00:01.00]it goes like this");

        assert!(
            sheet
                .declared
                .names_another_track(&about(Some("Hallelujah"), Some("Jeff Buckley")))
        );
    }

    #[test]
    fn punctuation_case_and_a_longer_naming_of_the_same_track_all_still_agree() {
        let sheet = sheet("[ti:echoes]\n[ar:Pink Floyd]\n[00:01.00]overhead the albatross");

        assert!(!sheet.declared.names_another_track(&about(
            Some("Echoes (Live at Pompeii)"),
            Some("Pink Floyd feat. Roger Waters")
        )));
    }

    #[test]
    fn a_sheet_that_declares_nothing_is_about_whatever_it_sits_beside() {
        let sheet = sheet("[00:01.00]overhead the albatross");

        assert!(
            !sheet
                .declared
                .names_another_track(&about(Some("Time"), Some("Pink Floyd")))
        );
        assert!(!sheet.declared.names_another_track(&about(None, None)));
    }

    #[test]
    fn a_track_the_player_cannot_name_is_not_something_a_sheet_can_contradict() {
        let sheet = sheet("[ti:Echoes]\n[ar:Pink Floyd]\n[00:01.00]overhead the albatross");

        assert!(!sheet.declared.names_another_track(&about(None, None)));
    }

    #[test]
    fn a_moment_no_arithmetic_can_hold_is_not_a_moment() {
        for overflowing in [
            "[99999999999999999999:01.00]all that you touch",
            "[18446744073709551615:01.00]all that you touch",
        ] {
            let lyrics = lyrics(overflowing).expect("a set");

            assert_eq!(lyrics.timing(), Timing::Unsynced);
            assert_eq!(plain(&lyrics), [overflowing]);
        }
    }
}
