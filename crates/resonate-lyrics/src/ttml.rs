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
const XML: &str = "http://www.w3.org/XML/1998/namespace";

const MOST_NODES: u32 = 1_000_000;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;
const MILLIS_DIGITS: usize = 3;

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

    let body = root
        .children()
        .find(|node| node.has_tag_name((TTML, "body")));
    let declared = Declared::about(
        None,
        None,
        body.and_then(|body| body.attribute("dur")).and_then(moment),
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
        .map(|paragraph| Paragraph::of(paragraph, &singers))
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
    fn of(paragraph: Node<'_, '_>, singers: &Singers<'_>) -> Self {
        let agent = paragraph
            .ancestors()
            .find_map(|node| node.attribute((TTML_METADATA, "agent")));
        let mut read = Self {
            at: paragraph.attribute("begin").and_then(moment),
            until: paragraph.attribute("end").and_then(moment),
            voice: singers.voice_of(agent),
            lead: String::new(),
            words: Vec::new(),
        };
        read.take_children_of(paragraph);
        if let Some(last) = read.words.last_mut() {
            last.text.truncate(last.text.trim_end().len());
        }
        read.lead.truncate(read.lead.trim_end().len());

        read
    }

    fn take_children_of(&mut self, parent: Node<'_, '_>) {
        for child in parent.children() {
            if let Some(text) = child.text().filter(|_| child.is_text()) {
                self.spoken(text);
                continue;
            }
            if !child.is_element() || is_beside_the_words(child) {
                continue;
            }
            match child.attribute("begin").and_then(moment) {
                Some(at) if child.has_tag_name((TTML, "span")) => {
                    let mut word = SungWord::sung(at, String::new());
                    spaced(&mut word.text, &spoken_within(child));
                    if let Some(until) = child.attribute("end").and_then(moment) {
                        word = word.ending(until);
                    }
                    self.words.push(word);
                }
                _ => self.take_children_of(child),
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

fn moment(value: &str) -> Option<Duration> {
    let value = value.trim();
    let value = value.strip_suffix('s').unwrap_or(value);
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
    let (whole_seconds, fraction) = seconds.split_once('.').unwrap_or((seconds, ""));

    let total = hours
        .checked_mul(SECONDS_PER_HOUR)?
        .checked_add(minutes.checked_mul(SECONDS_PER_MINUTE)?)?
        .checked_add(whole(whole_seconds)?)?;
    Some(Duration::from_secs(total) + Duration::from_millis(millis(fraction)?))
}

fn whole(digits: &str) -> Option<u64> {
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn millis(fraction: &str) -> Option<u64> {
    if fraction.is_empty() {
        return Some(0);
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let kept: String = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(MILLIS_DIGITS)
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

    #[test]
    fn a_ttml_clock_is_read_in_seconds_minutes_or_hours() {
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
        assert_eq!(line.text, "I used to rule the world");
        assert_eq!(
            texts(line),
            ["I ", "used ", "to ", "rule ", "the ", "world"]
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
            .find(|line| line.text == "'Cause you already know what you mean to me")
            .expect("a line sung together");
        assert_eq!(together.voice, Voice::One);
        assert!(lyrics.lines().iter().any(|line| line.voice == Voice::One));
        let goodbye = lyrics
            .lines()
            .iter()
            .find(|line| line.text.ends_with("say goodbye"))
            .expect("a line with a word sung in two syllables");
        assert!(texts(goodbye).ends_with(&["say ", "good", "bye"]));
    }

    #[test]
    fn a_line_timed_ttml_reads_its_lines_and_an_untimed_one_reads_as_words_alone() {
        let lined = read_whole(
            r#"<tt xmlns="http://www.w3.org/ns/ttml" xmlns:itunes="http://music.apple.com/lyric-ttml-internal" itunes:timing="Line"><body><div>
                <p begin="00:01.000" end="00:02.500">All that   you touch</p>
                <p begin="00:03.000">All that you see</p>
            </div></body></tt>"#,
        );
        assert_eq!(lined.detail(), Detail::Lines);
        assert_eq!(lined.lines()[0].text, "All that you touch");
        assert_eq!(lined.lines()[0].until, Some(Duration::from_millis(2_500)));
        assert_eq!(lined.lines()[1].until, None);

        let untimed = read_ttml(
            source(),
            r#"<tt xmlns="http://www.w3.org/ns/ttml"><body><div><p>All that you touch</p><p>All that you see</p></div></body></tt>"#,
        )
        .expect("the document reads");
        assert!(!untimed.says_more_than_its_lines);
        let untimed = untimed.lyrics.expect("the words are there");
        assert_eq!(untimed.timing(), Timing::Unsynced);
        assert_eq!(untimed.lines()[1].text, "All that you see");
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
