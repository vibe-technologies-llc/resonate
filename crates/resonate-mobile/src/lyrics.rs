use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_core::SourceId;
use resonate_library::{LookupOp, LyricsAsked, Reference};
use resonate_lyrics::{Detail, LyricLine, Lyrics, Timing, Voice, read_lyrics, read_lyricsfile};

use crate::{Error, MobilePlayer, Result, documents::location, features::milliseconds};

const SHEETS_KEPT: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LyricDetail {
    Unsynced,
    Lines,
    Words,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LyricWord {
    pub text: String,
    pub at_ms: u64,
    pub until_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LyricRow {
    pub text: String,
    pub at_ms: Option<u64>,
    pub until_ms: Option<u64>,
    pub voice: u32,
    pub words: Vec<LyricWord>,
    pub blank: bool,
    pub written: u32,
    pub breathes: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LyricSheet {
    pub track_id: u64,
    pub origin: String,
    pub detail: LyricDetail,
    pub two_voices: bool,
    pub end: u32,
    pub written_end: u32,
    pub lines: Vec<LyricRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum LyricReads {
    At { line: u32 },
    Spent { line: Option<u32> },
    Evenly,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LyricWait {
    pub next: u32,
    pub through: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LyricLetter {
    pub start_utf16: u32,
    pub end_utf16: u32,
    pub share: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LyricWipe {
    pub line: u32,
    pub sung_utf16: u32,
    pub blend: Option<LyricLetter>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LyricFrame {
    pub line: Option<u32>,
    pub active: Vec<u32>,
    pub voice_one: Option<u32>,
    pub voice_two: Option<u32>,
    pub read_at: Option<u32>,
    pub reads: LyricReads,
    pub waiting: Option<LyricWait>,
    pub ended: bool,
    pub wipes: Vec<LyricWipe>,
}

#[derive(Clone)]
pub(crate) enum Kept {
    Sheet(Arc<Lyrics>),
    Wordless,
}

#[derive(Default)]
pub(crate) struct KeptSheets(Mutex<Vec<(u64, Kept)>>);

impl KeptSheets {
    fn read(&self, id: u64) -> Option<Kept> {
        let mut held = self.0.lock();
        let at = held.iter().position(|(kept, _)| *kept == id)?;
        let read = held.remove(at);
        let kept = read.1.clone();
        held.push(read);

        Some(kept)
    }

    fn keep(&self, id: u64, kept: Kept) {
        let mut held = self.0.lock();
        held.retain(|(held, _)| *held != id);
        if held.len() >= SHEETS_KEPT {
            held.remove(0);
        }
        held.push((id, kept));
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn keep_lyric_text(&self, id: u64, text: String) -> Result<()> {
        let parsed = read_lyrics(SourceId::new("embedded")?, &text)?;
        if let Some(parsed) = parsed {
            let previous = self.cached_lyrics(id)?;
            if previous
                .as_ref()
                .is_none_or(|held| parsed.detail() > held.detail())
            {
                self.catalog
                    .keep_lyrics(id, Some(&text), None, "embedded")?;
                self.remember_lyrics(id, Some(parsed));
            }
        }
        Ok(())
    }

    pub fn keep_sidecar_lyrics(&self, id: u64, bytes: Vec<u8>) -> Result<()> {
        let track = self.catalog.track(id)?;
        let mut wanted = resonate_lyrics::Wanted::for_media(location(&track.uri)?);
        wanted.title = Some(track.title);
        wanted.artist = Some(track.artist);
        wanted.duration =
            (track.duration_ms > 0).then_some(Duration::from_millis(track.duration_ms));
        if let Some(lyrics) =
            resonate_lyrics::read_sidecar(SourceId::new("sidecar")?, &bytes, &wanted)?
        {
            let previous = self.cached_lyrics(id)?;
            if previous
                .as_ref()
                .is_none_or(|held| lyrics.detail() >= held.detail())
            {
                self.catalog.keep_lyrics(
                    id,
                    Some(&resonate_core::text::decoded(&bytes).0),
                    None,
                    "sidecar",
                )?;
                self.remember_lyrics(id, Some(lyrics));
            }
        }
        Ok(())
    }

    pub fn lyrics(&self, id: u64, refresh: bool) -> Result<LyricSheet> {
        let track = self.catalog.track(id)?;
        let cached = self.cached_lyrics(id)?;
        if self.services.online.client().is_reaching()
            && (refresh || self.catalog.lyric_text(id)?.is_none())
        {
            let answer = self
                .services
                .online
                .lyrics(&LyricsAsked {
                    title: track.title,
                    artist: track.artist,
                    album: (!track.album.is_empty()).then_some(track.album),
                    length: (track.duration_ms > 0)
                        .then_some(Duration::from_millis(track.duration_ms)),
                })
                .map_err(|source| Error::reference(LookupOp::Lyrics, source));
            match answer {
                Ok(Some(answer)) => {
                    let parsed =
                        parse_lyrics(&answer.text, answer.lyricsfile.as_deref(), "lrclib")?;
                    if let Some(parsed) = parsed
                        && cached
                            .as_ref()
                            .is_none_or(|held| parsed.detail() >= held.detail())
                    {
                        self.catalog.keep_lyrics(
                            id,
                            Some(&answer.text),
                            answer.lyricsfile.as_deref(),
                            "lrclib",
                        )?;
                        self.remember_lyrics(id, Some(parsed));
                    }
                }
                Ok(None) if cached.is_none() => {
                    self.catalog.keep_lyrics(id, None, None, "lrclib")?
                }
                Ok(None) => {}
                Err(error) if cached.is_none() => return Err(error),
                Err(error) => tracing::debug!(%error, "cached lyrics cover a refused lookup"),
            }
        }
        let lyrics = self.cached_lyrics(id)?;

        Ok(sheet_of(id, lyrics.as_deref()))
    }

    pub fn lyric_frame(&self, id: u64, position_ms: u64) -> LyricFrame {
        let lyrics = match self.services.lyrics.read(id) {
            Some(Kept::Sheet(lyrics)) => Some(lyrics),
            Some(Kept::Wordless) => None,
            None => self.lyrics_from_the_catalog(id).unwrap_or_else(|error| {
                tracing::debug!(%error, "kept lyrics could not be read again");
                self.services.lyrics.keep(id, Kept::Wordless);
                None
            }),
        };

        lyrics.map_or_else(unlit, |lyrics| {
            frame_of(&lyrics, Duration::from_millis(position_ms))
        })
    }
}

impl MobilePlayer {
    fn cached_lyrics(&self, id: u64) -> Result<Option<Arc<Lyrics>>> {
        if let Some(Kept::Sheet(lyrics)) = self.services.lyrics.read(id) {
            return Ok(Some(lyrics));
        }
        self.lyrics_from_the_catalog(id)
    }

    fn lyrics_from_the_catalog(&self, id: u64) -> Result<Option<Arc<Lyrics>>> {
        let lyrics = match self.catalog.lyric_text(id)? {
            Some((Some(text), document, origin)) => {
                parse_lyrics(&text, document.as_deref(), &origin)?
            }
            _ => None,
        };

        Ok(self.remember_lyrics(id, lyrics))
    }

    fn remember_lyrics(&self, id: u64, lyrics: Option<Lyrics>) -> Option<Arc<Lyrics>> {
        let lyrics = lyrics.map(Arc::new);
        let kept = lyrics.clone().map_or(Kept::Wordless, Kept::Sheet);
        self.services.lyrics.keep(id, kept);

        lyrics
    }
}

fn parse_lyrics(text: &str, document: Option<&str>, origin: &str) -> Result<Option<Lyrics>> {
    let source = SourceId::new(origin)?;
    if let Some(document) = document
        && let Ok(read) = read_lyricsfile(source.clone(), document)
        && read.lyrics.is_some()
    {
        return Ok(read.lyrics);
    }
    Ok(read_lyrics(source, text)?)
}

fn row(index: usize) -> u32 {
    u32::try_from(index).unwrap_or(u32::MAX)
}

fn sheet_of(id: u64, lyrics: Option<&Lyrics>) -> LyricSheet {
    let Some(lyrics) = lyrics else {
        return LyricSheet {
            track_id: id,
            origin: String::new(),
            detail: LyricDetail::Unsynced,
            two_voices: false,
            end: 0,
            written_end: 0,
            lines: Vec::new(),
        };
    };
    let mut written = 0;
    let lines: Vec<LyricRow> = lyrics
        .lines()
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let ordinal = written;
            if !line.is_blank() {
                written += 1;
            }

            row_of(line, ordinal, lyrics.breathes_before(index))
        })
        .collect();

    LyricSheet {
        track_id: id,
        origin: lyrics.source().to_string(),
        detail: match lyrics.detail() {
            Detail::Unsynced => LyricDetail::Unsynced,
            Detail::Lines => LyricDetail::Lines,
            Detail::Words => LyricDetail::Words,
        },
        two_voices: lyrics.lines().iter().any(|line| line.voice == Voice::Two),
        end: row(lines.len()),
        written_end: written,
        lines,
    }
}

fn row_of(line: &LyricLine, written: u32, breathes: bool) -> LyricRow {
    LyricRow {
        text: line.text.clone(),
        at_ms: line.at.map(milliseconds),
        until_ms: line.until.map(milliseconds),
        voice: row(line.voice.index()),
        words: line
            .words
            .iter()
            .map(|word| LyricWord {
                text: word.text.clone(),
                at_ms: milliseconds(word.at),
                until_ms: word.until.map(milliseconds),
            })
            .collect(),
        blank: line.is_blank(),
        written,
        breathes,
    }
}

fn unlit() -> LyricFrame {
    LyricFrame {
        line: None,
        active: Vec::new(),
        voice_one: None,
        voice_two: None,
        read_at: None,
        reads: LyricReads::Evenly,
        waiting: None,
        ended: false,
        wipes: Vec::new(),
    }
}

fn frame_of(lyrics: &Lyrics, position: Duration) -> LyricFrame {
    let line = lyrics.line_at(position);
    let lit = lyrics.voices_in_play(position);
    let waiting = lyrics.waiting_at(position);
    let ended = lyrics.has_ended(position).then_some(lyrics.lines().len());
    let read_at = waiting.map(|waiting| waiting.next).or(ended).or(line);
    let reads = if lyrics.timing() == Timing::Unsynced {
        LyricReads::Evenly
    } else {
        lit.into_iter()
            .flatten()
            .max()
            .or(ended)
            .or_else(|| waiting.is_none().then_some(read_at).flatten())
            .map_or(
                LyricReads::Spent {
                    line: read_at.map(row),
                },
                |line| LyricReads::At { line: row(line) },
            )
    };
    let wipes = lit
        .into_iter()
        .flatten()
        .filter_map(|index| wipe_of(&lyrics.lines()[index], index, position))
        .collect();

    LyricFrame {
        line: line.map(row),
        active: lit.into_iter().flatten().map(row).collect(),
        voice_one: lit[Voice::One.index()].map(row),
        voice_two: lit[Voice::Two.index()].map(row),
        read_at: read_at.map(row),
        reads,
        waiting: waiting.map(|waiting| LyricWait {
            next: row(waiting.next),
            through: waiting.through,
        }),
        ended: ended.is_some(),
        wipes,
    }
}

fn wipe_of(line: &LyricLine, index: usize, position: Duration) -> Option<LyricWipe> {
    let wiped = line.sweep_at(position)?.wiped(&line.text);

    Some(LyricWipe {
        line: row(index),
        sung_utf16: utf16_within(&line.text, wiped.sung_to),
        blend: wiped.blending.map(|blending| LyricLetter {
            start_utf16: utf16_within(&line.text, blending.letter.start),
            end_utf16: utf16_within(&line.text, blending.letter.end),
            share: blending.share,
        }),
    })
}

fn utf16_within(text: &str, bytes: usize) -> u32 {
    let units: usize = text
        .char_indices()
        .take_while(|&(at, _)| at < bytes)
        .map(|(_, letter)| letter.len_utf16())
        .sum();

    u32::try_from(units).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use std::{env, path::Path};

    use super::*;
    use crate::{PlaybackPreferences, Track, catalog::Catalog, features::Services};

    fn player() -> MobilePlayer {
        MobilePlayer {
            player: Mutex::new(None),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(resonate_codec::Sources::local()),
            failure: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: crate::sync::Syncing::under(env::temp_dir()),
            routing: Arc::new(crate::output::Routing::new()),
        }
    }

    fn song(player: &MobilePlayer, name: &str) -> u64 {
        player
            .catalog
            .upsert(Track {
                uri: format!("content://test/{name}.flac"),
                title: name.into(),
                ..Track::default()
            })
            .expect("a song")
            .id
    }

    fn sung(player: &MobilePlayer, sheet: &str) -> u64 {
        let id = song(player, "sung");
        player
            .keep_lyric_text(id, sheet.into())
            .expect("a readable sheet");

        id
    }

    fn sheet(lines: &[&str]) -> Lyrics {
        Lyrics::synced(
            SourceId::new("held").expect("a lowercase name"),
            lines
                .iter()
                .enumerate()
                .map(|(at, text)| LyricLine::sung(Duration::from_secs(at as u64), *text))
                .collect(),
        )
        .expect("every line is timed")
    }

    fn near(share: f32, expected: f32) -> bool {
        (share - expected).abs() < 1e-4
    }

    #[test]
    fn a_word_being_sung_is_wiped_to_the_letter_it_has_reached() {
        let player = player();
        let id = sung(
            &player,
            "[00:01.00]<00:01.00>Hello <00:02.00>world<00:03.00>\n[00:20.00]Next",
        );

        let sheet = player.lyrics(id, false).expect("the kept sheet");
        let frame = player.lyric_frame(id, 1_500);

        assert_eq!(sheet.detail, LyricDetail::Words);
        assert_eq!(sheet.end, 2);
        assert_eq!(frame.reads, LyricReads::At { line: 0 });
        assert_eq!(frame.read_at, Some(0));
        assert_eq!(frame.voice_one, Some(0));
        assert_eq!(frame.active, vec![0]);
        assert_eq!(
            frame.wipes,
            vec![LyricWipe {
                line: 0,
                sung_utf16: 2,
                blend: Some(LyricLetter {
                    start_utf16: 2,
                    end_utf16: 3,
                    share: 0.5
                }),
            }]
        );
    }

    #[test]
    fn two_singers_are_each_lit_and_the_later_line_is_read() {
        let player = player();
        let id = sung(
            &player,
            "[00:01.00][v1: First singer line]\n[00:02.00][v2: Second singer]\n[00:30.00]Together",
        );

        let sheet = player.lyrics(id, false).expect("the kept sheet");
        let frame = player.lyric_frame(id, 2_500);

        assert!(sheet.two_voices);
        assert_eq!(sheet.lines[1].voice, 1);
        assert_eq!(sheet.detail, LyricDetail::Lines);
        assert_eq!(frame.voice_one, Some(0));
        assert_eq!(frame.voice_two, Some(1));
        assert_eq!(frame.active, vec![0, 1]);
        assert_eq!(frame.reads, LyricReads::At { line: 1 });
        assert_eq!(frame.read_at, Some(1));
        assert!(frame.wipes.is_empty(), "a line-timed sheet wipes nothing");
    }

    #[test]
    fn a_pause_counts_down_to_the_next_written_line_and_the_light_is_spent() {
        let player = player();
        let id = sung(&player, "[00:01.00]one\n[00:05.00]\n[00:30.00]two");

        let sheet = player.lyrics(id, false).expect("the kept sheet");
        let frame = player.lyric_frame(id, 10_000);
        let waiting = frame.waiting.expect("a pause between verses");

        assert_eq!(
            sheet
                .lines
                .iter()
                .map(|line| (line.blank, line.written))
                .collect::<Vec<_>>(),
            vec![(false, 0), (true, 1), (false, 1)]
        );
        assert_eq!((sheet.end, sheet.written_end), (3, 2));
        assert!(sheet.lines[0].breathes && sheet.lines[2].breathes);
        assert_eq!(waiting.next, 2);
        assert!(
            waiting.through > 0.0 && waiting.through < 1.0,
            "the pause was {} of the way through",
            waiting.through
        );
        assert_eq!(frame.reads, LyricReads::Spent { line: Some(2) });
        assert_eq!(frame.read_at, Some(2));
        assert_eq!(frame.line, Some(1));
        assert!(frame.active.is_empty() && !frame.ended);
    }

    #[test]
    fn past_the_last_line_the_end_of_the_sheet_is_read() {
        let player = player();
        let id = sung(&player, "[00:01.00]one\n[00:05.00]two");

        let sheet = player.lyrics(id, false).expect("the kept sheet");
        let frame = player.lyric_frame(id, 100_000);

        assert!(frame.ended);
        assert_eq!(frame.read_at, Some(sheet.end));
        assert_eq!(frame.reads, LyricReads::At { line: sheet.end });
        assert_eq!(frame.waiting, None);
    }

    #[test]
    fn a_wipe_counts_in_utf16_across_letters_of_three_bytes() {
        let player = player();
        let id = sung(
            &player,
            "[00:01.00]<00:01.00>你好<00:02.00>世界<00:03.00>\n[00:20.00]下一行",
        );

        let frame = player.lyric_frame(id, 2_750);
        let wipe = frame.wipes.first().expect("a line being sung");
        let blend = wipe.blend.expect("a letter half sung");

        assert_eq!(wipe.sung_utf16, 3);
        assert_eq!((blend.start_utf16, blend.end_utf16), (3, 4));
        assert!(near(blend.share, 0.5));
    }

    #[test]
    fn a_wipe_counts_both_halves_of_a_surrogate_pair() {
        let player = player();
        let id = sung(
            &player,
            "[00:01.00]<00:01.00>I 😀 <00:02.00>you<00:03.00>\n[00:20.00]Next",
        );

        let on_the_face = player.lyric_frame(id, 1_750);
        let after_it = player.lyric_frame(id, 2_500);
        let face = on_the_face.wipes[0].blend.expect("the face half sung");
        let letter = after_it.wipes[0].blend.expect("a letter half sung");

        assert_eq!(on_the_face.wipes[0].sung_utf16, 2);
        assert_eq!((face.start_utf16, face.end_utf16), (2, 4));
        assert!(near(face.share, 0.25));
        assert_eq!(after_it.wipes[0].sung_utf16, 6);
        assert_eq!((letter.start_utf16, letter.end_utf16), (6, 7));
    }

    #[test]
    fn a_sheet_read_lately_outlasts_the_sheets_remembered_after_it() {
        let player = player();

        for id in 1..=16 {
            player.remember_lyrics(id, Some(sheet(&["a line"])));
        }
        let read = player.lyric_frame(1, 500);
        player.remember_lyrics(17, Some(sheet(&["another line"])));

        assert_eq!(read.active, vec![0]);
        assert!(matches!(
            player.services.lyrics.read(1),
            Some(Kept::Sheet(_))
        ));
        assert!(player.services.lyrics.read(2).is_none());
        assert!(matches!(
            player.services.lyrics.read(17),
            Some(Kept::Sheet(_))
        ));
    }

    #[test]
    fn a_frame_for_a_sheet_no_longer_remembered_reads_it_from_the_catalog() {
        let player = player();
        let id = sung(&player, "[00:01.00]one\n[00:30.00]two");

        for other in 1_000..1_016 {
            player.remember_lyrics(other, Some(sheet(&["a line"])));
        }
        let forgotten = player.services.lyrics.read(id).is_none();
        let frame = player.lyric_frame(id, 1_500);

        assert!(forgotten);
        assert_eq!(frame.active, vec![0]);
        assert_eq!(frame.reads, LyricReads::At { line: 0 });
    }

    #[test]
    fn a_song_without_words_answers_an_unlit_frame_and_is_remembered_so() {
        let player = player();
        let id = song(&player, "instrumental");

        let frame = player.lyric_frame(id, 1_000);
        let sheet = player.lyrics(id, false).expect("an empty sheet");

        assert_eq!(frame, unlit());
        assert!(matches!(
            player.services.lyrics.read(id),
            Some(Kept::Wordless)
        ));
        assert!(sheet.lines.is_empty());
        assert_eq!((sheet.end, sheet.written_end), (0, 0));
    }

    #[test]
    fn words_kept_after_a_song_was_remembered_wordless_are_read() {
        let player = player();
        let id = song(&player, "late");

        let before = player.lyric_frame(id, 1_500);
        player
            .keep_lyric_text(id, "[00:01.00]one\n[00:30.00]two".into())
            .expect("a readable sheet");
        let after = player.lyric_frame(id, 1_500);

        assert_eq!(before, unlit());
        assert_eq!(after.active, vec![0]);
    }
}
