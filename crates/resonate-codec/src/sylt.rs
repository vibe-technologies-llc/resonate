use std::fmt::Write as _;

use crate::text::{legacy, wide};

pub(crate) const FRAME_IDS: [&str; 2] = ["SYLT", "SLT"];

const LANGUAGE_BYTES: usize = 3;
const MILLISECONDS: u8 = 2;
const STAMP_BYTES: usize = 4;
const MILLISECONDS_PER_MINUTE: u32 = 60_000;
const MILLISECONDS_PER_SECOND: u32 = 1_000;
const MOST_LINES: usize = 4_096;

const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];

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
            0 => Some(Self::Latin1),
            1 => Some(Self::Utf16 { little: true }),
            2 => Some(Self::Utf16Be),
            3 => Some(Self::Utf8),
            _ => None,
        }
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
            Self::Latin1 => legacy(bytes),
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Utf16Be => wide(bytes, u16::from_be_bytes),
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
                    wide(body, u16::from_le_bytes)
                } else {
                    wide(body, u16::from_be_bytes)
                }
            }
        }
    }
}

struct Syllable {
    text: String,
    at: u32,
}

pub(crate) fn as_lrc(frame: &[u8]) -> Option<String> {
    let (&encoding, rest) = frame.split_first()?;
    let mut encoding = Encoding::of(encoding)?;
    let rest = rest.get(LANGUAGE_BYTES..)?;
    let (&stamps, rest) = rest.split_first()?;
    if stamps != MILLISECONDS {
        return None;
    }
    let (_, rest) = rest.split_first()?;
    let (descriptor, after_descriptor) = encoding.text_at(rest)?;
    encoding.read(&rest[..descriptor]);

    let syllables = syllables(encoding, &rest[after_descriptor..]);
    written(&syllables)
}

fn syllables(mut encoding: Encoding, mut rest: &[u8]) -> Vec<Syllable> {
    let mut read = Vec::new();
    while read.len() < MOST_LINES {
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
            at: u32::from_be_bytes(stamp),
        });
        rest = &rest[after + STAMP_BYTES..];
    }
    read
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
                line.push_str(&syllable.text);
            }
            _ => lines.push((syllable.at, syllable.text.clone())),
        }
    }

    let mut sheet = String::new();
    for (at, line) in &lines {
        let line = line.trim();
        let minutes = at / MILLISECONDS_PER_MINUTE;
        let seconds = at % MILLISECONDS_PER_MINUTE / MILLISECONDS_PER_SECOND;
        let milliseconds = at % MILLISECONDS_PER_SECOND;
        let _ = writeln!(sheet, "[{minutes:02}:{seconds:02}.{milliseconds:03}]{line}");
    }

    let holds_words = lines.iter().any(|(_, line)| !line.trim().is_empty());
    holds_words.then_some(sheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(encoding: u8, stamps: u8, entries: &[(&[u8], u32)], terminator: &[u8]) -> Vec<u8> {
        let mut bytes = vec![encoding, b'e', b'n', b'g', stamps, 1];
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
                (b"Overhead the albatross", 4_750),
                (b"Hangs motionless", 71_020),
            ],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes).as_deref(),
            Some("[00:04.750]Overhead the albatross\n[01:11.020]Hangs motionless\n"),
        );
    }

    #[test]
    fn syllables_are_gathered_into_the_line_a_newline_opens() {
        let bytes = frame(
            3,
            MILLISECONDS,
            &[
                (b"\nOver", 1_000),
                (b"head ", 1_400),
                (b"the albatross", 1_900),
                (b"\nHangs", 5_000),
                (b" motionless", 5_600),
            ],
            &[0],
        );

        assert_eq!(
            as_lrc(&bytes).as_deref(),
            Some("[00:01.000]Overhead the albatross\n[00:05.000]Hangs motionless\n"),
        );
    }

    #[test]
    fn wide_text_carries_its_order_from_the_mark_before_it() {
        let first = utf16le("Überall");
        let second: Vec<u8> = "wo".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let bytes = frame(1, MILLISECONDS, &[(&first, 0), (&second, 2_000)], &[0, 0]);

        assert_eq!(
            as_lrc(&bytes).as_deref(),
            Some("[00:00.000]Überall\n[00:02.000]wo\n"),
        );
    }

    #[test]
    fn a_frame_timed_in_mpeg_frames_is_left_unread() {
        let bytes = frame(3, 1, &[(b"Overhead", 12)], &[0]);

        assert_eq!(as_lrc(&bytes), None);
    }

    #[test]
    fn a_frame_cut_short_keeps_the_entries_it_holds_whole() {
        let mut bytes = frame(3, MILLISECONDS, &[(b"Overhead", 1_000)], &[0]);
        bytes.extend_from_slice(b"Hangs\0\0\0");

        assert_eq!(as_lrc(&bytes).as_deref(), Some("[00:01.000]Overhead\n"));
    }

    #[test]
    fn a_frame_holding_no_words_is_none() {
        let bytes = frame(3, MILLISECONDS, &[(b"", 0), (b"  ", 1_000)], &[0]);

        assert_eq!(as_lrc(&bytes), None);
        assert_eq!(as_lrc(&[3, b'e']), None);
    }
}
