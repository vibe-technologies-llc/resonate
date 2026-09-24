use std::fmt;

pub(crate) const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];

const WINDOWS_1252_FLOOR: u8 = 0x80;
const WINDOWS_1252_CEILING: u8 = 0x9F;
const WINDOWS_1252_ABOVE_LATIN1: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl TextEncoding {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
            Self::Windows1252 => "Windows-1252",
        }
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub fn decoded(bytes: &[u8]) -> (String, TextEncoding) {
    if let Some(rest) = bytes.strip_prefix(&UTF16_LE_BOM) {
        return (wide(rest, u16::from_le_bytes), TextEncoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(&UTF16_BE_BOM) {
        return (wide(rest, u16::from_be_bytes), TextEncoding::Utf16Be);
    }

    let body = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes);
    match std::str::from_utf8(body) {
        Ok(text) => (text.to_owned(), TextEncoding::Utf8),
        Err(_) => (legacy(body), TextEncoding::Windows1252),
    }
}

pub fn encoded(text: &str, encoding: TextEncoding) -> Option<Vec<u8>> {
    match encoding {
        TextEncoding::Utf8 => Some(text.as_bytes().to_vec()),
        TextEncoding::Utf16Le => Some(widened(text, u16::to_le_bytes)),
        TextEncoding::Utf16Be => Some(widened(text, u16::to_be_bytes)),
        TextEncoding::Windows1252 => text.chars().map(legacy_byte).collect(),
    }
}

fn widened(text: &str, order: fn(u16) -> [u8; 2]) -> Vec<u8> {
    text.encode_utf16().flat_map(order).collect()
}

fn legacy_byte(held: char) -> Option<u8> {
    if let Some(above) = WINDOWS_1252_ABOVE_LATIN1
        .iter()
        .position(|known| *known == held)
    {
        return Some(WINDOWS_1252_FLOOR + above as u8);
    }

    u8::try_from(u32::from(held))
        .ok()
        .filter(|byte| !(WINDOWS_1252_FLOOR..=WINDOWS_1252_CEILING).contains(byte))
}

pub(crate) fn wide(bytes: &[u8], order: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(order)
        .collect();

    String::from_utf16_lossy(&units)
}

pub(crate) fn legacy(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| match byte {
            WINDOWS_1252_FLOOR..=WINDOWS_1252_CEILING => {
                WINDOWS_1252_ABOVE_LATIN1[usize::from(byte - WINDOWS_1252_FLOOR)]
            }
            byte => char::from(*byte),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str, little: bool) -> Vec<u8> {
        let mut bytes = if little {
            UTF16_LE_BOM.to_vec()
        } else {
            UTF16_BE_BOM.to_vec()
        };
        for unit in text.encode_utf16() {
            let pair = if little {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            };
            bytes.extend_from_slice(&pair);
        }
        bytes
    }

    #[test]
    fn a_byte_order_mark_names_the_encoding_before_the_bytes_are_weighed() {
        assert_eq!(
            decoded(&utf16("Écoute", true)),
            ("Écoute".to_owned(), TextEncoding::Utf16Le)
        );
        assert_eq!(
            decoded(&utf16("Écoute", false)),
            ("Écoute".to_owned(), TextEncoding::Utf16Be)
        );

        let mut marked = UTF8_BOM.to_vec();
        marked.extend_from_slice("Écoute".as_bytes());
        assert_eq!(decoded(&marked), ("Écoute".to_owned(), TextEncoding::Utf8));
    }

    #[test]
    fn text_that_is_not_utf8_is_read_as_the_encoding_a_tagger_wrote_it_in() {
        assert_eq!(
            decoded(&[0x45, 0x63, 0x6F, 0x75, 0x74, 0xE9]),
            ("Ecout\u{e9}".to_owned(), TextEncoding::Windows1252)
        );
        assert_eq!(
            decoded(&[0x93, 0x45, 0x94]).0,
            "\u{201c}E\u{201d}".to_owned()
        );
    }

    #[test]
    fn plain_text_stays_utf8() {
        assert_eq!(
            decoded(b"TITLE \"Echoes\""),
            ("TITLE \"Echoes\"".to_owned(), TextEncoding::Utf8)
        );
        assert_eq!(decoded(b""), (String::new(), TextEncoding::Utf8));
    }

    #[test]
    fn what_was_decoded_is_written_back_in_the_encoding_it_was_read_in() {
        for held in ["Echoes", "Écoute", "Marcin Przybyłowicz", "“E”"] {
            for encoding in [
                TextEncoding::Utf8,
                TextEncoding::Utf16Le,
                TextEncoding::Utf16Be,
            ] {
                let written =
                    encoded(held, encoding).expect("every string is held by a Unicode encoding");
                let mut marked = match encoding {
                    TextEncoding::Utf16Le => UTF16_LE_BOM.to_vec(),
                    TextEncoding::Utf16Be => UTF16_BE_BOM.to_vec(),
                    _ => Vec::new(),
                };
                marked.extend_from_slice(&written);

                assert_eq!(decoded(&marked), (held.to_owned(), encoding));
            }
        }
    }

    #[test]
    fn a_legacy_encoding_answers_for_the_letters_it_holds_and_for_no_others() {
        assert_eq!(
            encoded("Ecouté", TextEncoding::Windows1252),
            Some(vec![0x45, 0x63, 0x6F, 0x75, 0x74, 0xE9])
        );
        assert_eq!(
            encoded("“E”", TextEncoding::Windows1252),
            Some(vec![0x93, 0x45, 0x94])
        );
        assert_eq!(encoded("Przybyłowicz", TextEncoding::Windows1252), None);
    }

    #[test]
    fn a_wide_encoding_missing_its_last_byte_keeps_what_it_holds() {
        let mut clipped = utf16("Echoes", true);
        clipped.pop();

        assert_eq!(decoded(&clipped).0, "Echoe");
    }
}
