use std::fmt::Write as _;

use resonate_core::{
    TextEncoding,
    lrc::{escaped_for_lrc, unescaped},
    text::decoded_as,
};

use crate::mpa::FrameClock;

pub(crate) const FRAME_IDS: [&str; 2] = ["SYLT", "SLT"];
pub(crate) const UNKNOWN_LANGUAGE: [u8; LANGUAGE_BYTES] = *b"XXX";

const LANGUAGE_BYTES: usize = 3;
const CONTENT_AT: usize = 1 + LANGUAGE_BYTES + 1;
const MPEG_FRAMES: u8 = 1;
const MILLISECONDS: u8 = 2;
const STAMP_BYTES: usize = 4;
const MILLISECONDS_PER_MINUTE: u32 = 60_000;
const MILLISECONDS_PER_SECOND: u32 = 1_000;
const MOST_SYLLABLES: usize = 65_536;
const CONTENT_OF_LYRICS: u8 = 1;
const CONTENT_OF_A_TRANSCRIPTION: u8 = 2;

const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];
const LATIN1: u8 = 0;
const UTF16_WITH_A_MARK: u8 = 1;
const UTF16_BE: u8 = 2;
const UTF8: u8 = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Latin1,
    Utf16 { little: bool },
    Utf16Be,
    Utf8,
}

impl Encoding {
    fn of(byte: u8) -> Option<Self> {
        match byte {
            LATIN1 => Some(Self::Latin1),
            UTF16_WITH_A_MARK => Some(Self::Utf16 { little: false }),
            UTF16_BE => Some(Self::Utf16Be),
            UTF8 => Some(Self::Utf8),
            _ => None,
        }
    }

    fn encoded(self, text: &str) -> Option<Vec<u8>> {
        let mut bytes = match self {
            Self::Latin1 => text
                .chars()
                .map(|letter| u8::try_from(u32::from(letter)).ok())
                .collect::<Option<Vec<u8>>>()?,
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            Self::Utf16 { .. } => UTF16_LE_BOM
                .into_iter()
                .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
                .collect(),
        };

        bytes.extend_from_slice(if self.is_wide() { &[0, 0] } else { &[0] });
        Some(bytes)
    }

    fn is_wide(self) -> bool {
        matches!(self, Self::Utf16 { .. } | Self::Utf16Be)
    }

    fn text_at(self, bytes: &[u8]) -> Option<(usize, usize)> {
        if self.is_wide() {
            let end = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .position(|unit| *unit == [0, 0])?
                * 2;
            Some((end, end + 2))
        } else {
            let end = bytes.iter().position(|byte| *byte == 0)?;
            Some((end, end + 1))
        }
    }

    fn read(&mut self, bytes: &[u8]) -> String {
        match self {
            Self::Latin1 => decoded_as(bytes, TextEncoding::WINDOWS_1252),
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Utf16Be => decoded_as(bytes, TextEncoding::Utf16Be),
            Self::Utf16 { little } => {
                let body = if let Some(rest) = bytes.strip_prefix(&UTF16_LE_BOM) {
                    *little = true;
                    rest
                } else if let Some(rest) = bytes.strip_prefix(&UTF16_BE_BOM) {
                    *little = false;
                    rest
                } else {
                    bytes
                };
                if *little {
                    decoded_as(body, TextEncoding::Utf16Le)
                } else {
                    decoded_as(body, TextEncoding::Utf16Be)
                }
            }
        }
    }
}

struct Syllable {
    text: String,
    at: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Voiced {
    pub language: [u8; LANGUAGE_BYTES],
    pub encoding: u8,
}

impl Voiced {
    pub(crate) const UNSPOKEN: Self = Self {
        language: UNKNOWN_LANGUAGE,
        encoding: UTF16_WITH_A_MARK,
    };

    pub(crate) fn new(language: [u8; LANGUAGE_BYTES], encoding: u8) -> Option<Self> {
        Encoding::of(encoding)?;

        let language = if language.iter().all(u8::is_ascii_alphabetic) {
            language
        } else {
            UNKNOWN_LANGUAGE
        };
        Some(Self { language, encoding })
    }

    pub(crate) fn of_a_frame(frame: &[u8]) -> Option<Self> {
        let (&encoding, rest) = frame.split_first()?;
        Self::new(rest.get(..LANGUAGE_BYTES)?.try_into().ok()?, encoding)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Timed {
    pub frame: Vec<u8>,
    pub words: String,
}

pub(crate) fn holds_words(frame: &[u8]) -> bool {
    matches!(
        frame.get(CONTENT_AT),
        Some(&(CONTENT_OF_LYRICS | CONTENT_OF_A_TRANSCRIPTION))
    )
}

pub(crate) fn timed(sheet: &str, voiced: Voiced) -> Option<Timed> {
    let syllables = syllables_of_a_sheet(sheet)?;
    [voiced.encoding, UTF16_WITH_A_MARK]
        .into_iter()
        .find_map(|encoding| {
            let frame = framed(&syllables, voiced.language, encoding)?;
            (as_lrc(&frame, None).as_deref() == Some(sheet)).then_some(frame)
        })
        .map(|frame| Timed {
            frame,
            words: words_of(&syllables),
        })
}

fn framed(syllables: &[Syllable], language: [u8; LANGUAGE_BYTES], encoding: u8) -> Option<Vec<u8>> {
    let encoded_as = Encoding::of(encoding)?;
    let mut frame = vec![encoding];

    frame.extend_from_slice(&language);
    frame.extend_from_slice(&[MILLISECONDS, CONTENT_OF_LYRICS]);
    frame.extend(encoded_as.encoded("")?);
    for syllable in syllables {
        frame.extend(encoded_as.encoded(&syllable.text)?);
        frame.extend_from_slice(&syllable.at.to_be_bytes());
    }
    Some(frame)
}

fn syllables_of_a_sheet(sheet: &str) -> Option<Vec<Syllable>> {
    let mut read = Vec::new();

    for line in sheet.lines() {
        let (at, said) = line.strip_prefix('[')?.split_once(']')?;
        let Some(marked) = said.strip_prefix('<') else {
            read.push(Syllable {
                text: unescaped(said).to_owned(),
                at: stamped(at)?,
            });
            continue;
        };
        for (place, piece) in marked.split('<').enumerate() {
            let (at, text) = piece.split_once('>')?;
            let opening = if place == 0 { "\n" } else { "" };

            read.push(Syllable {
                text: format!("{opening}{text}"),
                at: stamped(at)?,
            });
        }
    }
    (!read.is_empty()).then_some(read)
}

fn stamped(stamp: &str) -> Option<u32> {
    let (minutes, rest) = stamp.split_once(':')?;
    let (seconds, milliseconds) = rest.split_once('.')?;
    let figure = |figures: &str| {
        figures
            .bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| figures.parse::<u32>().ok())
            .flatten()
    };
    figure(minutes)?
        .checked_mul(MILLISECONDS_PER_MINUTE)?
        .checked_add(figure(seconds)?.checked_mul(MILLISECONDS_PER_SECOND)?)?
        .checked_add(figure(milliseconds)?)
}

fn words_of(syllables: &[Syllable]) -> String {
    let marked = syllables
        .iter()
        .any(|syllable| starts_a_line(&syllable.text));
    let mut lines: Vec<String> = Vec::new();

    for syllable in syllables {
        match lines.last_mut() {
            Some(line) if marked && !starts_a_line(&syllable.text) => line.push_str(&syllable.text),
            _ => lines.push(syllable.text.trim_start_matches(['\n', '\r']).to_owned()),
        }
    }
    lines
        .iter()
        .map(|line| line.trim())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn as_lrc(frame: &[u8], mpeg: Option<FrameClock>) -> Option<String> {
    let (&encoding, rest) = frame.split_first()?;
    let mut encoding = Encoding::of(encoding)?;
    let rest = rest.get(LANGUAGE_BYTES..)?;
    let (&stamps, rest) = rest.split_first()?;
    let counted_in = match stamps {
        MILLISECONDS => None,
        MPEG_FRAMES => Some(mpeg?),
        _ => return None,
    };
    let (&content, rest) = rest.split_first()?;

    if content != CONTENT_OF_LYRICS && content != CONTENT_OF_A_TRANSCRIPTION {
        return None;
    }

    let (descriptor, after_descriptor) = encoding.text_at(rest)?;

    encoding.read(&rest[..descriptor]);

    let syllables = syllables(encoding, &rest[after_descriptor..], counted_in);
    written(&syllables)
}

fn syllables(
    mut encoding: Encoding,
    mut rest: &[u8],
    counted_in: Option<FrameClock>,
) -> Vec<Syllable> {
    let mut read = Vec::new();

    loop {
        if read.len() == MOST_SYLLABLES {
            tracing::warn!(
                kept = MOST_SYLLABLES,
                "a timed-lyrics frame holds more syllables than are read; the rest are passed over"
            );
            break;
        }

        let Some((end, after)) = encoding.text_at(rest) else {
            break;
        };
        let Some(stamp) = rest.get(after..after + STAMP_BYTES) else {
            break;
        };
        let Ok(stamp) = <[u8; STAMP_BYTES]>::try_from(stamp) else {
            break;
        };

        read.push(Syllable {
            text: encoding.read(&rest[..end]),
            at: milliseconds(u32::from_be_bytes(stamp), counted_in),
        });
        rest = &rest[after + STAMP_BYTES..];
    }
    read
}

fn milliseconds(stamp: u32, counted_in: Option<FrameClock>) -> u32 {
    counted_in.map_or(stamp, |clock| clock.milliseconds(stamp))
}

fn starts_a_line(text: &str) -> bool {
    text.starts_with(['\n', '\r'])
}

fn written(syllables: &[Syllable]) -> Option<String> {
    let marked = syllables
        .iter()
        .any(|syllable| starts_a_line(&syllable.text));
    let mut lines: Vec<(u32, String)> = Vec::new();

    for syllable in syllables {
        match lines.last_mut() {
            Some((_, line)) if marked && !starts_a_line(&syllable.text) => {
                let _ = write!(line, "<{}>{}", stamp(syllable.at), syllable.text);
            }
            _ if marked => lines.push((
                syllable.at,
                format!(
                    "<{}>{}",
                    stamp(syllable.at),
                    syllable.text.trim_start_matches(['\n', '\r'])
                ),
            )),
            _ => lines.push((
                syllable.at,
                escaped_for_lrc(syllable.text.trim()).into_owned(),
            )),
        }
    }

    let mut sheet = String::new();

    for (at, line) in &lines {
        let _ = writeln!(sheet, "[{}]{}", stamp(*at), line.trim());
    }

    let holds_words = syllables
        .iter()
        .any(|syllable| !syllable.text.trim().is_empty());
    holds_words.then_some(sheet)
}

fn stamp(at: u32) -> String {
    let minutes = at / MILLISECONDS_PER_MINUTE;
    let seconds = at % MILLISECONDS_PER_MINUTE / MILLISECONDS_PER_SECOND;
    let milliseconds = at % MILLISECONDS_PER_SECOND;

    format!("{minutes:02}:{seconds:02}.{milliseconds:03}")
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;
    use symphonia::core::codecs::audio::well_known::{CODEC_ID_FLAC, CODEC_ID_MP3};

    use super::*;

    fn frame(encoding: u8, stamps: u8, entries: &[(&[u8], u32)], terminator: &[u8]) -> Vec<u8> {
        framed_as(CONTENT_OF_LYRICS, encoding, stamps, entries, terminator)
    }

    fn framed_as(
        content: u8,
        encoding: u8,
        stamps: u8,
        entries: &[(&[u8], u32)],
        terminator: &[u8],
    ) -> Vec<u8> {
        let mut bytes = vec![encoding, b'e', b'n', b'g', stamps, content];

        bytes.extend_from_slice(terminator);
        for (text, at) in entries {
            bytes.extend_from_slice(text);
            bytes.extend_from_slice(terminator);
            bytes.extend_from_slice(&at.to_be_bytes());
        }
        bytes
    }

    fn utf16le(text: &str) -> Vec<u8> {
        UTF16_LE_BOM
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect()
    }

    #[test]
    fn a_line_per_entry_is_written_as_a_line_per_moment() {
        let bytes = frame(
            3,
            MILLISECONDS,
            &[
                (b"Sunlight the fireflies", 4_750),
                (b"Sways in silence", 71_020),
            ],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes, None).as_deref(),
            Some("[00:04.750]Sunlight the fireflies\n[01:11.020]Sways in silence\n"),
        );
    }

    #[test]
    fn syllables_are_gathered_into_the_line_a_newline_opens() {
        let bytes = frame(
            3,
            MILLISECONDS,
            &[
                (b"\nSun", 1_000),
                (b"light ", 1_400),
                (b"the fireflies", 1_900),
                (b"\nSways", 5_000),
                (b" in silence", 5_600),
            ],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes, None).as_deref(),
            Some(
                "[00:01.000]<00:01.000>Sun<00:01.400>light <00:01.900>the fireflies\n\
                 [00:05.000]<00:05.000>Sways<00:05.600> in silence\n"
            ),
        );
    }

    #[test]
    fn wide_text_carries_its_order_from_the_mark_before_it() {
        let first = utf16le("Überall");
        let second: Vec<u8> = "wo".encode_utf16().flat_map(u16::to_le_bytes).collect();

        let bytes = frame(1, MILLISECONDS, &[(&first, 0), (&second, 2_000)], &[0, 0]);

        assert_eq!(
            as_lrc(&bytes, None).as_deref(),
            Some("[00:00.000]Überall\n[00:02.000]wo\n"),
        );
    }

    #[test]
    fn only_a_frame_of_lyrics_or_a_transcription_is_read_as_words() {
        let entries: &[(&[u8], u32)] = &[(b"Sunlight", 1_000)];
        let chords = framed_as(5, 3, MILLISECONDS, entries, &[0]);
        let events = framed_as(3, 3, MILLISECONDS, entries, &[0]);
        let transcribed = framed_as(CONTENT_OF_A_TRANSCRIPTION, 3, MILLISECONDS, entries, &[0]);

        assert_eq!(as_lrc(&chords, None), None);
        assert_eq!(as_lrc(&events, None), None);
        assert_eq!(
            as_lrc(&transcribed, None).as_deref(),
            Some("[00:01.000]Sunlight\n")
        );
    }

    #[test]
    fn a_frame_timed_in_mpeg_frames_is_written_in_the_time_those_frames_last() {
        let clock = FrameClock::of(CODEC_ID_MP3, SampleRate::HZ_44100);
        let bytes = frame(
            3,
            MPEG_FRAMES,
            &[(b"Sunlight", 0), (b"Sways", 1_000), (b"Still", 2_000_000)],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes, clock).as_deref(),
            Some("[00:00.000]Sunlight\n[00:26.122]Sways\n[870:44.897]Still\n"),
        );
    }

    #[test]
    fn a_frame_timed_in_mpeg_frames_counts_from_the_first_sample_heard() {
        let clock = FrameClock::of(CODEC_ID_MP3, SampleRate::HZ_44100)
            .expect("a clock")
            .heard_from(1_105);
        let bytes = frame(
            3,
            MPEG_FRAMES,
            &[(b"Sunlight", 0), (b"Sways", 1), (b"Still", 1_000)],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes, Some(clock)).as_deref(),
            Some("[00:00.000]Sunlight\n[00:00.001]Sways\n[00:26.097]Still\n"),
        );
    }

    #[test]
    fn a_frame_timed_in_mpeg_frames_of_a_stream_with_no_frames_is_left_unread() {
        let bytes = frame(3, MPEG_FRAMES, &[(b"Sunlight", 12)], &[0]);

        assert_eq!(as_lrc(&bytes, None), None);
        assert_eq!(FrameClock::of(CODEC_ID_FLAC, SampleRate::HZ_44100), None);
    }

    #[test]
    fn a_frame_of_mpeg_two_lasts_as_long_as_one_of_mpeg_one_at_twice_the_rate() {
        let full = FrameClock::of(CODEC_ID_MP3, SampleRate::HZ_44100).expect("a clock");
        let half = FrameClock::of(CODEC_ID_MP3, SampleRate::HZ_22050).expect("a clock");

        assert_eq!(full.milliseconds(1_000), 26_122);
        assert_eq!(half.milliseconds(1_000), 26_122);
    }

    #[test]
    fn wide_text_with_no_mark_is_big_endian() {
        let units: Vec<u8> = "Überall"
            .encode_utf16()
            .flat_map(u16::to_be_bytes)
            .collect();
        let bytes = frame(1, MILLISECONDS, &[(&units, 0)], &[0, 0]);

        assert_eq!(
            as_lrc(&bytes, None).as_deref(),
            Some("[00:00.000]Überall\n")
        );
    }

    #[test]
    fn a_frame_cut_short_keeps_the_entries_it_holds_whole() {
        let mut bytes = frame(3, MILLISECONDS, &[(b"Sunlight", 1_000)], &[0]);

        bytes.extend_from_slice(b"Sways\0\0\0");

        assert_eq!(
            as_lrc(&bytes, None).as_deref(),
            Some("[00:01.000]Sunlight\n")
        );
    }

    const ENGLISH: Voiced = Voiced {
        language: *b"eng",
        encoding: UTF8,
    };

    #[test]
    fn a_sheet_a_frame_reads_as_is_framed_back_into_the_same_frame() {
        let lines = frame(
            UTF8,
            MILLISECONDS,
            &[
                (b"Sunlight the fireflies", 4_750),
                (b"Sways in silence", 71_020),
            ],
            &[0],
        );
        let syllables = frame(
            UTF8,
            MILLISECONDS,
            &[
                (b"\nSun", 1_000),
                (b"light ", 1_400),
                (b"the fireflies", 1_900),
                (b"\nSways", 5_000),
                (b" in silence", 5_600),
            ],
            &[0],
        );

        for (bytes, words) in [
            (lines, "Sunlight the fireflies\nSways in silence"),
            (syllables, "Sunlight the fireflies\nSways in silence"),
        ] {
            let sheet = as_lrc(&bytes, None).expect("a sheet");
            let timed = timed(&sheet, ENGLISH).expect("a sheet a frame can hold");

            assert_eq!(timed.frame, bytes);
            assert_eq!(timed.words, words);
        }
    }

    #[test]
    fn a_line_led_by_a_bracket_the_reader_would_take_is_escaped_and_framed_back_as_written() {
        let bytes = frame(
            UTF8,
            MILLISECONDS,
            &[
                (b"[offset:5] sunlight", 1_000),
                (b"[00:09.00] the fireflies", 9_000),
                (b"[Chorus] sways", 12_000),
            ],
            &[0],
        );

        let sheet = as_lrc(&bytes, None).expect("a sheet");
        let timed = timed(&sheet, ENGLISH).expect("a sheet a frame can hold");

        assert_eq!(
            sheet,
            "[00:01.000]\\[offset:5] sunlight\n\
             [00:09.000]\\[00:09.00] the fireflies\n\
             [00:12.000][Chorus] sways\n",
        );
        assert_eq!(timed.frame, bytes);
        assert_eq!(
            timed.words,
            "[offset:5] sunlight\n[00:09.00] the fireflies\n[Chorus] sways"
        );
    }

    #[test]
    fn a_marked_line_opens_on_its_word_stamp_and_needs_no_escape() {
        let bytes = frame(
            UTF8,
            MILLISECONDS,
            &[(b"\n[offset:5] sun", 1_000), (b"light", 1_400)],
            &[0],
        );

        let sheet = as_lrc(&bytes, None).expect("a sheet");

        assert_eq!(
            sheet,
            "[00:01.000]<00:01.000>[offset:5] sun<00:01.400>light\n"
        );
        assert_eq!(timed(&sheet, ENGLISH).map(|timed| timed.frame), Some(bytes));
    }

    #[test]
    fn a_sheet_no_frame_reads_as_is_not_framed() {
        for sheet in [
            "[00:01.00]Sunlight the fireflies\n",
            "[00:01.000]Sunlight the fireflies",
            "[00:4294967295.000]Sunlight the fireflies\n",
            "[00:4294968.000]Sunlight the fireflies\n",
            "[71582:48.000]Sunlight the fireflies\n",
            "[ar:Example Band]\n[00:01.000]Sunlight the fireflies\n",
            "Sunlight the fireflies\n",
            "",
        ] {
            assert_eq!(timed(sheet, ENGLISH), None, "{sheet:?}");
        }
    }

    #[test]
    fn a_sheet_its_frames_text_cannot_hold_is_framed_as_wide_text_with_its_mark() {
        let latin = Voiced {
            language: *b"deu",
            encoding: LATIN1,
        };

        let plain = timed("[00:00.000]Uberall\n", latin).expect("a sheet");
        let wide = timed("[00:00.000]Überall ☂\n", latin).expect("a sheet");

        assert_eq!(plain.frame[0], LATIN1);
        assert_eq!(wide.frame[0], UTF16_WITH_A_MARK);
        assert_eq!(&wide.frame[1..4], b"deu");
        assert_eq!(
            as_lrc(&wide.frame, None).as_deref(),
            Some("[00:00.000]Überall ☂\n")
        );
    }

    #[test]
    fn a_language_no_frame_may_be_written_in_is_written_unknown() {
        let spoken = Voiced::of_a_frame(&frame(UTF8, MILLISECONDS, &[], &[0]));
        let unspelt = Voiced::new([0, b'e', b' '], UTF8);

        assert_eq!(spoken.map(|voiced| voiced.language), Some(*b"eng"));
        assert_eq!(
            unspelt.map(|voiced| voiced.language),
            Some(UNKNOWN_LANGUAGE)
        );
        assert_eq!(Voiced::new(*b"eng", 9), None);
    }

    #[test]
    fn only_a_frame_of_words_is_one_lyrics_take_the_place_of() {
        let entries: &[(&[u8], u32)] = &[(b"Sunlight", 1_000)];

        assert!(holds_words(&frame(UTF8, MILLISECONDS, entries, &[0])));
        assert!(holds_words(&framed_as(
            CONTENT_OF_A_TRANSCRIPTION,
            UTF8,
            MILLISECONDS,
            entries,
            &[0]
        )));
        assert!(!holds_words(&framed_as(
            5,
            UTF8,
            MILLISECONDS,
            entries,
            &[0]
        )));
        assert!(!holds_words(&[UTF8]));
    }

    #[test]
    fn a_frame_holding_no_words_is_none() {
        let bytes = frame(3, MILLISECONDS, &[(b"", 0), (b"  ", 1_000)], &[0]);

        assert_eq!(as_lrc(&bytes, None), None);
        assert_eq!(as_lrc(&[3, b'e'], None), None);
    }
}
