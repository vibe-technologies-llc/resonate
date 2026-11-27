use std::time::Duration;

use resonate_core::SourceId;
use roxmltree::{Document, Node, ParsingOptions};

use crate::{
    LyricLine, Lyrics, SungWord, Voice,
    lrc::Declared,
    lyricsfile::{LARGEST_LYRICSFILE, Lyricsfile, MOST_WORDS, Unread},
    model::MOST_LINES,
};

const TTML: &str = "http://www.w3.org/ns/ttml";
const TTML_METADATA: &str = "http://www.w3.org/ns/ttml#metadata";
const TTML_PARAMETER: &str = "http://www.w3.org/ns/ttml#parameter";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

const MOST_NODES: u32 = 1_000_000;
const MOST_DEPTH: usize = 64;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;
const MILLIS_PER_SECOND: u128 = 1_000;
const NANOS_PER_SECOND: u128 = 1_000_000_000;
const MILLIS_DIGITS: usize = 3;
const NANOS_DIGITS: usize = 9;
const FRAMES_PER_SECOND_UNLESS_SAID: u128 = 30;

pub fn read_document(source: SourceId, text: &str) -> Result<Lyricsfile, Unread> {
    if text.trim_start().starts_with('<') {
        read_ttml(source, text)
    } else {
        crate::read_lyricsfile(source, text)
    }
}

pub fn read_ttml(source: SourceId, text: &str) -> Result<Lyricsfile, Unread> {
    if text.len() > LARGEST_LYRICSFILE {
        return Err(Unread::TooLarge);
    }

    let options = ParsingOptions {
        allow_dtd: false,
        nodes_limit: MOST_NODES,
        ..ParsingOptions::default()
    };
    let document = Document::parse_with_options(text, options).map_err(|error| {
        tracing::debug!(%error, "a TTML document did not parse");
        Unread::NotADocument
    })?;
    let root = document.root_element();

    if !root.has_tag_name((TTML, "tt")) {
        return Err(Unread::NotADocument);
    }
    if nests_deeper_than(root, MOST_DEPTH) {
        return Err(Unread::TooDeep);
    }

    let clocks = Clocks::of(root);

    let body = root
        .children()
        .find(|node| node.has_tag_name((TTML, "body")));
    let declared = Declared::about(
        None,
        None,
        body.and_then(|body| body.attribute("dur"))
            .and_then(|dur| clocks.moment(dur)),
    );
    let Some(body) = body else {
        return Ok(Lyricsfile {
            lyrics: None,
            says_more_than_its_lines: false,
            declared,
        });
    };

    let singers = Singers::of(root);
    let paragraphs: Vec<Node<'_, '_>> = body
        .descendants()
        .filter(|node| node.has_tag_name((TTML, "p")))
        .collect();

    if paragraphs.len() > MOST_LINES {
        return Err(Unread::TooMany);
    }

    let lines: Vec<Paragraph> = paragraphs
        .into_iter()
        .map(|paragraph| Paragraph::of(paragraph, &singers, &clocks))
        .collect();

    if lines.iter().map(|line| line.words.len()).sum::<usize>() > MOST_WORDS {
        return Err(Unread::TooMany);
    }

    let says_more_than_its_lines = lines
        .iter()
        .any(|line| !line.words.is_empty() || line.until.is_some());
    let lyrics = if lines.iter().all(|line| line.at.is_some()) {
        Lyrics::synced(
            source,
            lines.into_iter().filter_map(Paragraph::into_line).collect(),
        )
        .ok()
    } else {
        Some(Lyrics::plain(
            source,
            lines.into_iter().map(Paragraph::into_text),
        ))
    }
    .filter(|lyrics| !lyrics.is_empty());

    Ok(Lyricsfile {
        says_more_than_its_lines: says_more_than_its_lines
            && lyrics
                .as_ref()
                .is_some_and(|lyrics| lyrics.timing() == crate::Timing::Synced),
        lyrics,
        declared,
    })
}

struct Singers<'input> {
    first: Option<&'input str>,
    people: Vec<&'input str>,
}

impl<'input> Singers<'input> {
    fn of(root: Node<'input, '_>) -> Self {
        let people: Vec<&'input str> = root
            .children()
            .filter(|node| node.has_tag_name((TTML, "head")))
            .flat_map(|head| head.descendants())
            .filter(|node| node.has_tag_name((TTML_METADATA, "agent")))
            .filter(|agent| agent.attribute("type") == Some("person"))
            .filter_map(|agent| agent.attribute((XML, "id")))
            .collect();

        Self {
            first: people.first().copied(),
            people,
        }
    }

    fn voice_of(&self, agent: Option<&str>) -> Voice {
        match agent {
            Some(agent) if Some(agent) != self.first && self.people.contains(&agent) => Voice::Two,
            _ => Voice::One,
        }
    }
}

struct Paragraph {
    at: Option<Duration>,
    until: Option<Duration>,
    voice: Voice,
    lead: String,
    words: Vec<SungWord>,
}

impl Paragraph {
    fn of(paragraph: Node<'_, '_>, singers: &Singers<'_>, clocks: &Clocks) -> Self {
        let agent = paragraph
            .ancestors()
            .find_map(|node| node.attribute((TTML_METADATA, "agent")));
        let mut read = Self {
            at: paragraph
                .attribute("begin")
                .and_then(|begin| clocks.moment(begin)),
            until: paragraph
                .attribute("end")
                .and_then(|end| clocks.moment(end)),
            voice: singers.voice_of(agent),
            lead: String::new(),
            words: Vec::new(),
        };

        read.take_children_of(paragraph, clocks);
        if let Some(last) = read.words.last_mut() {
            last.text.truncate(last.text.trim_end().len());
        }
        read.lead.truncate(read.lead.trim_end().len());

        read
    }

    fn take_children_of(&mut self, parent: Node<'_, '_>, clocks: &Clocks) {
        for child in parent.children() {
            if let Some(text) = child.text().filter(|_| child.is_text()) {
                self.spoken(text);
                continue;
            }
            if !child.is_element() || is_beside_the_words(child) {
                continue;
            }
            match child
                .attribute("begin")
                .and_then(|begin| clocks.moment(begin))
            {
                Some(at) if child.has_tag_name((TTML, "span")) => {
                    let mut word = SungWord::sung(at, String::new());

                    spaced(&mut word.text, &spoken_within(child));
                    if let Some(until) = child.attribute("end").and_then(|end| clocks.moment(end)) {
                        word = word.ending(until);
                    }
                    self.words.push(word);
                }
                _ => self.take_children_of(child, clocks),
            }
        }
    }

    fn spoken(&mut self, text: &str) {
        match self.words.last_mut() {
            Some(word) => spaced(&mut word.text, text),
            None => spaced(&mut self.lead, text),
        }
    }

    fn into_line(self) -> Option<LyricLine> {
        let at = self.at?;
        let line = if self.words.is_empty() {
            LyricLine::sung(at, self.lead)
        } else {
            let mut words = self.words;

            if let Some(first) = words.first_mut()
                && !self.lead.is_empty()
            {
                first.text.insert_str(0, &format!("{} ", self.lead));
            }
            LyricLine::worded(at, words)
        };
        let line = line.voiced(self.voice);

        Some(match self.until {
            Some(until) => line.ending(until),
            None => line,
        })
    }

    fn into_text(self) -> String {
        if self.words.is_empty() {
            return self.lead;
        }

        let words: String = self.words.into_iter().map(|word| word.text).collect();
        if self.lead.is_empty() {
            words
        } else {
            format!("{} {words}", self.lead)
        }
    }
}

fn is_beside_the_words(node: Node<'_, '_>) -> bool {
    node.attribute((TTML_METADATA, "role")).is_some()
}

fn spoken_within(node: Node<'_, '_>) -> String {
    let mut text = String::new();

    for part in node.descendants().filter(|part| part.is_text()) {
        let aside = part
            .ancestors()
            .take_while(|ancestor| *ancestor != node)
            .any(is_beside_the_words);
        if !aside {
            text.push_str(part.text().unwrap_or_default());
        }
    }
    text
}

fn spaced(into: &mut String, text: &str) {
    for letter in text.chars() {
        if letter.is_whitespace() {
            if !into.is_empty() && !into.ends_with(' ') {
                into.push(' ');
            }
        } else {
            into.push(letter);
        }
    }
}

fn nests_deeper_than(root: Node<'_, '_>, most: usize) -> bool {
    let mut open: Vec<Node<'_, '_>> = Vec::new();

    for node in root.descendants().filter(Node::is_element) {
        while open.last().is_some_and(|last| Some(*last) != node.parent()) {
            open.pop();
        }
        open.push(node);
        if open.len() > most {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
enum Metric {
    Hours,
    Minutes,
    Seconds,
    Milliseconds,
    Frames,
    Ticks,
}

impl Metric {
    const SUFFIXES: [(&'static str, Self); 6] = [
        ("ms", Self::Milliseconds),
        ("h", Self::Hours),
        ("m", Self::Minutes),
        ("s", Self::Seconds),
        ("f", Self::Frames),
        ("t", Self::Ticks),
    ];

    fn ending(value: &str) -> Option<(&str, Self)> {
        Self::SUFFIXES
            .iter()
            .find_map(|(suffix, metric)| Some((value.strip_suffix(suffix)?, *metric)))
    }
}

struct Clocks {
    frames_per_second: Ratio,
    ticks_per_second: u128,
}

#[derive(Clone, Copy)]
struct Ratio {
    over: u128,
    under: u128,
}

impl Clocks {
    fn of(root: Node<'_, '_>) -> Self {
        let parameter = |name: &str| {
            root.attribute((TTML_PARAMETER, name))
                .and_then(|value| whole(value.trim()))
                .filter(|value| *value > 0)
                .map(u128::from)
        };
        let frame_rate = parameter("frameRate");
        let sub_frames = parameter("subFrameRate").unwrap_or(1);
        let multiplier = root
            .attribute((TTML_PARAMETER, "frameRateMultiplier"))
            .and_then(|value| {
                let mut parts = value.split_whitespace();

                let over = whole(parts.next()?)?;
                let under = whole(parts.next()?)?;

                (parts.next().is_none() && over > 0 && under > 0)
                    .then_some((u128::from(over), u128::from(under)))
            })
            .unwrap_or((1, 1));

        Self {
            frames_per_second: Ratio {
                over: frame_rate.unwrap_or(FRAMES_PER_SECOND_UNLESS_SAID) * multiplier.0,
                under: multiplier.1,
            },
            ticks_per_second: parameter("tickRate")
                .or_else(|| frame_rate.map(|rate| rate * sub_frames))
                .unwrap_or(1),
        }
    }

    fn moment(&self, value: &str) -> Option<Duration> {
        let value = value.trim();

        match Metric::ending(value) {
            Some((count, Metric::Seconds)) if count.contains(':') => clock(count),
            Some((count, metric)) => self.counted(count, metric),
            None => clock(value),
        }
    }

    fn counted(&self, count: &str, metric: Metric) -> Option<Duration> {
        let (whole_count, fraction_digits) = count.split_once('.').unwrap_or((count, ""));

        let scaled = u128::from(whole(whole_count)?)
            .checked_mul(NANOS_PER_SECOND)?
            .checked_add(u128::from(fraction(fraction_digits, NANOS_DIGITS)?))?;

        let (times, per) = match metric {
            Metric::Hours => (u128::from(SECONDS_PER_HOUR), 1),
            Metric::Minutes => (u128::from(SECONDS_PER_MINUTE), 1),
            Metric::Seconds => (1, 1),
            Metric::Milliseconds => (1, MILLIS_PER_SECOND),
            Metric::Frames => (self.frames_per_second.under, self.frames_per_second.over),
            Metric::Ticks => (1, self.ticks_per_second),
        };

        let nanos = scaled.checked_mul(times)? / per;

        Some(Duration::new(
            u64::try_from(nanos / NANOS_PER_SECOND).ok()?,
            u32::try_from(nanos % NANOS_PER_SECOND).ok()?,
        ))
    }
}

fn clock(value: &str) -> Option<Duration> {
    let mut parts = value.rsplit(':');

    let seconds = parts.next()?;
    let minutes = match parts.next() {
        Some(minutes) => whole(minutes)?,
        None => 0,
    };
    let hours = match parts.next() {
        Some(hours) => whole(hours)?,
        None => 0,
    };

    if parts.next().is_some() {
        return None;
    }

    let (whole_seconds, digits) = seconds.split_once('.').unwrap_or((seconds, ""));

    let total = hours
        .checked_mul(SECONDS_PER_HOUR)?
        .checked_add(minutes.checked_mul(SECONDS_PER_MINUTE)?)?
        .checked_add(whole(whole_seconds)?)?;
    Some(Duration::from_secs(total) + Duration::from_millis(fraction(digits, MILLIS_DIGITS)?))
}

fn whole(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn fraction(digits: &str, places: usize) -> Option<u64> {
    if digits.is_empty() {
        return Some(0);
    }
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    let kept: String = digits
        .chars()
        .chain(std::iter::repeat('0'))
        .take(places)
        .collect();

    kept.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Detail, Timing};

    const WORDED: &str = include_str!("../tests/fixtures/ttml_worded.ttml");
    const DUET: &str = include_str!("../tests/fixtures/ttml_duet.ttml");

    fn source() -> SourceId {
        SourceId::new("betterlyrics").expect("a lowercase name")
    }

    fn read_whole(text: &str) -> Lyrics {
        read_ttml(source(), text)
            .expect("the document reads")
            .lyrics
            .expect("the document holds lines")
    }

    fn texts(line: &LyricLine) -> Vec<&str> {
        line.words.iter().map(|word| word.text.as_str()).collect()
    }

    fn unspecified() -> Clocks {
        Clocks {
            frames_per_second: Ratio {
                over: FRAMES_PER_SECOND_UNLESS_SAID,
                under: 1,
            },
            ticks_per_second: 1,
        }
    }

    #[test]
    fn a_ttml_clock_is_read_in_seconds_minutes_or_hours() {
        let moment = |value| unspecified().moment(value);

        assert_eq!(moment("12.741"), Some(Duration::from_millis(12_741)));
        assert_eq!(moment("4:01.445"), Some(Duration::from_millis(241_445)));
        assert_eq!(moment("1:02:03.5"), Some(Duration::from_millis(3_723_500)));
        assert_eq!(moment("7.25s"), Some(Duration::from_millis(7_250)));
        assert_eq!(moment("3"), Some(Duration::from_secs(3)));
        assert_eq!(moment("1:2:3:4"), None);
        assert_eq!(moment("-1.0"), None);
        assert_eq!(moment("1.x"), None);
        assert_eq!(moment(""), None);
    }

    #[test]
    fn every_offset_time_metric_ttml_defines_is_read() {
        let moment = |value| unspecified().moment(value);

        assert_eq!(moment("1500ms"), Some(Duration::from_millis(1_500)));
        assert_eq!(moment("2.5ms"), Some(Duration::from_micros(2_500)));
        assert_eq!(moment("2m"), Some(Duration::from_secs(120)));
        assert_eq!(moment("1.5m"), Some(Duration::from_secs(90)));
        assert_eq!(moment("1h"), Some(Duration::from_secs(3_600)));
        assert_eq!(moment("0.25h"), Some(Duration::from_secs(900)));
        assert_eq!(moment("45f"), Some(Duration::from_millis(1_500)));
        assert_eq!(moment("9t"), Some(Duration::from_secs(9)));
        assert_eq!(moment("-4ms"), None);
        assert_eq!(moment("ms"), None);
        assert_eq!(moment("1:30s"), Some(Duration::from_secs(90)));
        assert_eq!(moment("99999999999999999999h"), None);
    }

    #[test]
    fn frames_and_ticks_are_counted_against_the_rates_the_document_declares() {
        let lyrics = read_whole(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttp="http://www.w3.org/ns/ttml#parameter" ttp:frameRate="24" ttp:frameRateMultiplier="1000 1001" ttp:tickRate="10000000"><body><div>
                <p begin="24f" end="20000000t">frames</p>
                <p begin="5000ms" end="1m">units</p>
            </div></body></tt>"#,
        );

        assert_eq!(
            lyrics.lines()[0].at,
            Some(Duration::from_nanos(1_001_000_000))
        );
        assert_eq!(lyrics.lines()[0].until, Some(Duration::from_secs(2)));
        assert_eq!(lyrics.lines()[1].at, Some(Duration::from_secs(5)));
        assert_eq!(lyrics.lines()[1].until, Some(Duration::from_secs(60)));

        let by_default = read_whole(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttp="http://www.w3.org/ns/ttml#parameter" ttp:frameRate="25" ttp:subFrameRate="2"><body><div>
                <p begin="50t" end="30f">ticks</p>
            </div></body></tt>"#,
        );

        assert_eq!(by_default.lines()[0].at, Some(Duration::from_secs(1)));
        assert_eq!(
            by_default.lines()[0].until,
            Some(Duration::from_millis(1_200))
        );
    }

    #[test]
    fn a_document_nested_past_the_bound_is_unreadable_rather_than_overflowing_the_stack() {
        let depth = 100_000;
        let document = format!(
            r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="1.0">{}x{}</p></div></body></tt>"#,
            "<span>".repeat(depth),
            "</span>".repeat(depth)
        );

        assert_eq!(
            read_ttml(source(), &document).map(|read| read.lyrics.is_some()),
            Err(Unread::TooDeep)
        );
    }

    #[test]
    fn a_document_nested_within_the_bound_is_still_read() {
        let depth = MOST_DEPTH - 5;
        let document = format!(
            r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p begin="1.0">{}deep{}</p></div></body></tt>"#,
            "<span>".repeat(depth),
            "</span>".repeat(depth)
        );

        assert_eq!(read_whole(&document).lines()[0].text, "deep");
    }

    #[test]
    fn a_word_timed_ttml_is_read_word_by_word_with_each_line_ending_where_it_says() {
        let read = read_ttml(source(), WORDED).expect("the document reads");

        assert!(read.says_more_than_its_lines);
        assert_eq!(
            read.declared,
            Declared::about(None, None, Some(Duration::from_millis(241_445)))
        );

        let lyrics = read.lyrics.expect("the document holds lines");

        assert_eq!(lyrics.detail(), Detail::Words);

        let line = &lyrics.lines()[0];

        assert_eq!(line.text, "We used to chart the stars");
        assert_eq!(
            texts(line),
            ["We ", "used ", "to ", "chart ", "the ", "stars"]
        );
        assert_eq!(line.at, Some(Duration::from_millis(12_741)));
        assert_eq!(line.until, Some(Duration::from_millis(16_514)));
        assert_eq!(line.words[5].until, Some(Duration::from_millis(16_514)));
        assert!(lyrics.lines().iter().all(|line| line.voice == Voice::One));
    }

    #[test]
    fn the_second_singer_declared_is_the_second_voice_whoever_sings_first() {
        let lyrics = read_whole(DUET);

        assert_eq!(lyrics.lines()[0].text, "Ooh, ooh");
        assert_eq!(lyrics.lines()[0].voice, Voice::Two);

        let together = lyrics
            .lines()
            .iter()
            .find(|line| line.text == "Since you already know the way that I feel")
            .expect("a line sung together");

        assert_eq!(together.voice, Voice::One);
        assert!(lyrics.lines().iter().any(|line| line.voice == Voice::One));

        let goodbye = lyrics
            .lines()
            .iter()
            .find(|line| line.text.ends_with("drift apart"))
            .expect("a line with a word sung in two syllables");

        assert!(texts(goodbye).ends_with(&["drift ", "a", "part"]));
    }

    #[test]
    fn a_line_timed_ttml_reads_its_lines_and_an_untimed_one_reads_as_words_alone() {
        let lined = read_whole(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal" itunes:timing="Line"><body><div>
                <p begin="00:01.000" end="00:02.500">Every lamp   you lit</p>
                <p begin="00:03.000">Each road I know</p>
            </div></body></tt>"#,
        );

        assert_eq!(lined.detail(), Detail::Lines);
        assert_eq!(lined.lines()[0].text, "Every lamp you lit");
        assert_eq!(lined.lines()[0].until, Some(Duration::from_millis(2_500)));
        assert_eq!(lined.lines()[1].until, None);

        let untimed = read_ttml(
            source(),
            r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p>Every lamp you lit</p><p>Each road I know</p></div></body></tt>"#,
        )
        .expect("the document reads");

        assert!(!untimed.says_more_than_its_lines);

        let untimed = untimed.lyrics.expect("the words are there");

        assert_eq!(untimed.timing(), Timing::Unsynced);
        assert_eq!(untimed.lines()[1].text, "Each road I know");
    }

    #[test]
    fn background_and_translated_words_are_left_beside_the_line() {
        let lyrics = read_whole(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:ttm="http://www.w3.org/ns/ttml#metadata"><body><div>
                <p begin="1.0" end="3.0"><span begin="1.0" end="1.5">Hello</span> <span begin="1.5" end="2.0">there</span><span ttm:role="x-bg"><span begin="2.0" end="3.0">(hey)</span></span><span ttm:role="x-translation">Hallo</span></p>
            </div></body></tt>"#,
        );

        assert_eq!(lyrics.lines()[0].text, "Hello there");
        assert_eq!(texts(&lyrics.lines()[0]), ["Hello ", "there"]);
    }

    #[test]
    fn what_is_not_a_ttml_document_is_refused_and_a_lyricsfile_is_still_read_as_one() {
        assert_eq!(
            read_ttml(source(), "<html><body/></html>").map(|read| read.lyrics.is_some()),
            Err(Unread::NotADocument)
        );
        assert_eq!(
            read_ttml(source(), "<tt").map(|read| read.lyrics.is_some()),
            Err(Unread::NotADocument)
        );
        assert_eq!(
            read_ttml(
                source(),
                r#"<!DOCTYPE tt [<!ENTITY a "aaaa">]><tt xmlns="http://www.w3.org/ns/ttml"/>"#
            )
            .map(|read| read.lyrics.is_some()),
            Err(Unread::NotADocument)
        );

        let lyricsfile = read_document(
            source(),
            "version: '1.0'\nlines:\n  - {text: 'Stay', start_ms: 4200}\n",
        )
        .expect("the lyricsfile reads");

        assert_eq!(lyricsfile.lyrics.expect("a line").lines()[0].text, "Stay");
        assert!(read_document(source(), DUET).is_ok());
    }

    #[test]
    fn a_document_past_the_bound_is_refused_whole() {
        let lines = "<p begin=\"1.0\">la</p>".repeat(MOST_LINES + 1);
        let document = format!(
            r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div>{lines}</div></body></tt>"#
        );

        assert_eq!(
            read_ttml(source(), &document).map(|read| read.lyrics.is_some()),
            Err(Unread::TooMany)
        );
    }
}
