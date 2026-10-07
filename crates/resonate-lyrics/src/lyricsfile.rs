use std::time::Duration;

use resonate_core::SourceId;
use serde::Deserialize;
use serde_saphyr::{Budget, Options};

use crate::{
    LyricLine, Lyrics, SungWord, Voice,
    lrc::{Declared, lines_of},
    model::MOST_LINES,
};

const VERSION: &str = "1.0";

pub const LARGEST_LYRICSFILE: usize = 4 * 1024 * 1024;

const MOST_WORDS: usize = 200_000;

const MOST_NODES: usize = 1_000_000;

const DEEPEST: usize = 16;

#[derive(Deserialize)]
struct Document {
    version: String,
    #[serde(default)]
    metadata: Option<Metadata>,
    #[serde(default)]
    lines: Option<Vec<Line>>,
    #[serde(default)]
    plain: Option<String>,
}

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    instrumental: Option<bool>,
}

#[derive(Deserialize)]
struct Line {
    #[serde(default)]
    text: Option<String>,
    start_ms: u64,
    #[serde(default)]
    end_ms: Option<u64>,
    #[serde(default)]
    words: Option<Vec<Word>>,
}

#[derive(Deserialize)]
struct Word {
    #[serde(default)]
    text: Option<String>,
    start_ms: u64,
    #[serde(default)]
    end_ms: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unread {
    TooLarge,
    NotADocument,
    AnotherVersion,
    TooMany,
}

#[derive(Debug)]
pub struct Lyricsfile {
    pub lyrics: Option<Lyrics>,
    pub says_more_than_its_lines: bool,
    pub(crate) declared: Declared,
}

pub fn read_lyricsfile(source: SourceId, text: &str) -> Result<Lyricsfile, Unread> {
    if text.len() > LARGEST_LYRICSFILE {
        return Err(Unread::TooLarge);
    }
    let document: Document =
        serde_saphyr::from_str_with_options(text, options()).map_err(|error| {
            tracing::debug!(%error, "a lyricsfile did not parse");
            Unread::NotADocument
        })?;
    if document.version != VERSION {
        return Err(Unread::AnotherVersion);
    }
    let declared = document
        .metadata
        .as_ref()
        .map(Metadata::declared)
        .unwrap_or_default();
    if document
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.instrumental)
        .unwrap_or(false)
    {
        return Ok(Lyricsfile {
            lyrics: None,
            says_more_than_its_lines: false,
            declared,
        });
    }

    let mut lines = document.lines.unwrap_or_default();
    if lines.len() > MOST_LINES
        || lines
            .iter()
            .map(|line| line.words.as_ref().map_or(0, Vec::len))
            .sum::<usize>()
            > MOST_WORDS
    {
        return Err(Unread::TooMany);
    }
    lines.sort_by_key(|line| line.start_ms);
    let says_more_than_its_lines = says_more(&lines);
    let synced = Lyrics::synced(source.clone(), voiced(lines))
        .ok()
        .filter(|lyrics| !lyrics.is_empty());
    let lyrics = synced.or_else(|| {
        let plain = document.plain?;
        Some(Lyrics::plain(source, lines_of(&plain).map(str::to_owned)))
            .filter(|lyrics| !lyrics.is_empty())
    });

    Ok(Lyricsfile {
        says_more_than_its_lines: says_more_than_its_lines && lyrics.is_some(),
        lyrics,
        declared,
    })
}

impl Metadata {
    fn declared(&self) -> Declared {
        Declared::about(
            self.title.clone(),
            self.artist.clone(),
            self.duration_ms.map(moment),
        )
    }
}

fn options() -> Options {
    let mut budget = Budget::default();
    budget.max_documents = 1;
    budget.max_nodes = MOST_NODES;
    budget.max_depth = DEEPEST;
    let mut options = Options::default();
    options.budget = Some(budget);
    options.with_snippet = false;
    options
}

fn says_more(lines: &[Line]) -> bool {
    let worded = lines
        .iter()
        .any(|line| line.words.as_ref().is_some_and(|words| !words.is_empty()));
    let ended_apart = lines.iter().enumerate().any(|(index, line)| {
        line.end_ms
            .is_some_and(|end| lines.get(index + 1).is_none_or(|next| next.start_ms != end))
    });

    worded || ended_apart
}

fn moment(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

fn voiced(lines: Vec<Line>) -> Vec<LyricLine> {
    let mut sung_until: [Option<u64>; 2] = [None; 2];

    lines
        .into_iter()
        .map(|line| {
            let starts = line.start_ms;
            let ends = line.end_ms.or_else(|| {
                line.words
                    .iter()
                    .flatten()
                    .filter_map(|word| word.end_ms)
                    .max()
            });
            let lyric = lyric_of(line);
            if lyric.is_blank() {
                return lyric;
            }
            let free = |voice: usize| sung_until[voice].is_none_or(|until| until <= starts);
            let voice = if free(0) {
                Voice::One
            } else if free(1) || sung_until[1] < sung_until[0] {
                Voice::Two
            } else {
                Voice::One
            };
            sung_until[voice.index()] = Some(ends.unwrap_or(starts).max(starts));

            lyric.voiced(voice)
        })
        .collect()
}

fn lyric_of(line: Line) -> LyricLine {
    let at = moment(line.start_ms);
    let text = line.text.unwrap_or_default();
    let words = line.words.unwrap_or_default();
    let lyric = if words.is_empty() {
        LyricLine::sung(at, text)
    } else {
        LyricLine::worded(at, worded(&text, words))
    };

    match line.end_ms {
        Some(end) => lyric.ending(moment(end)),
        None => lyric,
    }
}

fn worded(text: &str, words: Vec<Word>) -> Vec<SungWord> {
    let spelled: Vec<String> = match laid_over(text, &words) {
        Some(spelled) => spelled,
        None => words
            .iter()
            .map(|word| word.text.clone().unwrap_or_default())
            .collect(),
    };

    words
        .into_iter()
        .zip(spelled)
        .map(|(word, spelled)| {
            let sung = SungWord::sung(moment(word.start_ms), spelled);
            match word.end_ms {
                Some(end) => sung.ending(moment(end)),
                None => sung,
            }
        })
        .collect()
}

fn laid_over(text: &str, words: &[Word]) -> Option<Vec<String>> {
    let mut starts = Vec::with_capacity(words.len());
    let mut cursor = 0;
    for word in words {
        let needle = word.text.as_deref().unwrap_or_default().trim();
        let found = if needle.is_empty() {
            cursor
        } else {
            cursor + text.get(cursor..)?.find(needle)?
        };
        starts.push(found);
        cursor = found + needle.len();
    }
    if let Some(first) = starts.first_mut() {
        *first = 0;
    }

    Some(
        starts
            .iter()
            .enumerate()
            .map(|(index, &start)| {
                let end = starts.get(index + 1).copied().unwrap_or(text.len());
                text[start..end].to_owned()
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Detail, Timing};

    const WORDED: &str = include_str!("../tests/fixtures/lyricsfile_worded.yaml");
    const OVERLAPPING: &str = include_str!("../tests/fixtures/lyricsfile_overlapping.yaml");
    const LINED: &str = include_str!("../tests/fixtures/lyricsfile_lined.yaml");

    fn source() -> SourceId {
        SourceId::new("lrclib").expect("a lowercase name")
    }

    fn read_whole(text: &str) -> Lyricsfile {
        read_lyricsfile(source(), text).expect("the document reads")
    }

    fn read(source: SourceId, text: &str) -> Result<Lyricsfile, Unread> {
        read_lyricsfile(source, text)
    }

    #[test]
    fn a_word_synced_lyricsfile_is_read_word_by_word_under_the_line_it_sings() {
        let read = read_whole(WORDED);
        let lyrics = read.lyrics.expect("the document holds lines");

        assert!(read.says_more_than_its_lines);
        assert_eq!(lyrics.detail(), Detail::Words);
        let line = &lyrics.lines()[0];
        assert_eq!(line.text, "Stay until the morning");
        assert_eq!(line.at, Some(moment(4_200)));
        assert_eq!(line.until, Some(moment(6_800)));
        assert_eq!(
            line.words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["Stay ", "until ", "the ", "morning"]
        );
        assert_eq!(line.words[3].until, Some(moment(6_800)));
    }

    #[test]
    fn words_written_without_their_spaces_are_laid_over_the_line_they_came_from() {
        let read = read_whole(
            "version: '1.0'
metadata: {title: T, artist: A}
lines:
  - text: 'all that you touch'
    start_ms: 1000
    words:
      - {text: all, start_ms: 1000}
      - {text: that, start_ms: 1300}
      - {text: you, start_ms: 1600}
      - {text: touch, start_ms: 1900, end_ms: 2400}
",
        );
        let lyrics = read.lyrics.expect("the document holds lines");
        let line = &lyrics.lines()[0];

        assert_eq!(line.text, "all that you touch");
        assert_eq!(
            line.words
                .iter()
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>(),
            ["all ", "that ", "you ", "touch"]
        );
    }

    #[test]
    fn lines_that_overlap_are_sung_by_two_voices() {
        let lyrics = read_whole(OVERLAPPING)
            .lyrics
            .expect("the document holds lines");

        assert_eq!(
            lyrics
                .lines()
                .iter()
                .map(|line| line.voice)
                .collect::<Vec<_>>(),
            [Voice::One, Voice::Two, Voice::One]
        );
        assert_eq!(lyrics.voices_in_play(moment(13_000)), [Some(0), Some(1)]);
        assert_eq!(lyrics.voices_in_play(moment(15_500)), [None, Some(1)]);
    }

    #[test]
    fn a_lyricsfile_written_from_an_lrc_says_nothing_its_lines_do_not() {
        let read = read_whole(LINED);
        let lyrics = read.lyrics.expect("the document holds lines");

        assert!(!read.says_more_than_its_lines);
        assert_eq!(lyrics.timing(), Timing::Synced);
        assert_eq!(lyrics.detail(), Detail::Lines);
        assert!(lyrics.lines().iter().all(|line| line.voice == Voice::One));
    }

    #[test]
    fn an_instrumental_holds_no_lyrics_and_plain_words_are_read_where_no_line_is_timed() {
        let instrumental = read_whole(
            "version: '1.0'\nmetadata:\n  title: Quiet\n  artist: E\n  instrumental: true\nplain: |\n  la\n",
        );
        assert!(instrumental.lyrics.is_none());

        let plain = read_whole(
            "version: '1.0'\nmetadata: {title: M, artist: E}\nplain: |\n  Morning light\n  A new day begins\n",
        )
        .lyrics
        .expect("the plain words are read");
        assert_eq!(plain.timing(), Timing::Unsynced);
        assert_eq!(plain.lines().len(), 2);

        let blank = read_whole(
            "version: '1.0'\nmetadata: {title: M, artist: E}\nlines:\n  - {text: '', start_ms: 10}\nplain: 'only this'\n",
        );
        assert_eq!(
            blank.lyrics.map(|lyrics| lyrics.timing()),
            Some(Timing::Unsynced)
        );
    }

    #[test]
    fn a_document_of_another_version_a_duplicated_key_or_too_much_is_refused() {
        assert_eq!(
            read(
                source(),
                "version: '2.0'\nmetadata: {title: T, artist: A}\n"
            )
            .err(),
            Some(Unread::AnotherVersion)
        );
        assert_eq!(
            read(
                source(),
                "version: '1.0'\nversion: '1.0'\nmetadata: {title: T, artist: A}\n"
            )
            .err(),
            Some(Unread::NotADocument)
        );
        assert_eq!(
            read(source(), "- not a mapping\n").err(),
            Some(Unread::NotADocument)
        );
        assert_eq!(
            read(source(), &" ".repeat(LARGEST_LYRICSFILE + 1)).err(),
            Some(Unread::TooLarge)
        );
        let crowded = format!(
            "version: '1.0'\nlines:\n{}",
            "  - {text: la, start_ms: 1}\n".repeat(MOST_LINES + 1)
        );
        assert_eq!(read(source(), &crowded).err(), Some(Unread::TooMany));
    }
}
