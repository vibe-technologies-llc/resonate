use std::{borrow::Cow, fmt};

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::Encoding;

pub const UTF8_BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: [u8; 2] = [0xFF, 0xFE];
const UTF16_BE_BOM: [u8; 2] = [0xFE, 0xFF];

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct LegacyEncoding(&'static Encoding);

impl LegacyEncoding {
    pub const WINDOWS_1252: Self = Self(encoding_rs::WINDOWS_1252);
    pub const WINDOWS_1251: Self = Self(encoding_rs::WINDOWS_1251);
    pub const SHIFT_JIS: Self = Self(encoding_rs::SHIFT_JIS);
    pub const GBK: Self = Self(encoding_rs::GBK);
    pub const BIG5: Self = Self(encoding_rs::BIG5);

    pub fn name(self) -> &'static str {
        self.0.name()
    }

    pub fn is_ascii_compatible(self) -> bool {
        self.0.is_ascii_compatible()
    }
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Legacy(LegacyEncoding),
}

impl TextEncoding {
    pub const WINDOWS_1252: Self = Self::Legacy(LegacyEncoding::WINDOWS_1252);

    pub fn name(self) -> &'static str {
        match self {
            Self::Utf8 | Self::Utf8Bom => "UTF-8",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
            Self::Legacy(legacy) => legacy.name(),
        }
    }

    pub const fn byte_order_mark(self) -> &'static [u8] {
        match self {
            Self::Utf8Bom => &UTF8_BOM,
            Self::Utf16Le => &UTF16_LE_BOM,
            Self::Utf16Be => &UTF16_BE_BOM,
            Self::Utf8 | Self::Legacy(_) => &[],
        }
    }

    pub const fn unit_bytes(self) -> usize {
        match self {
            Self::Utf8 | Self::Utf8Bom | Self::Legacy(_) => 1,
            Self::Utf16Le | Self::Utf16Be => 2,
        }
    }
}

impl fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

pub fn decoded(bytes: &[u8]) -> (String, TextEncoding) {
    let encoding = detected(bytes);
    (decoded_as(bytes, encoding), encoding)
}

pub fn detected(bytes: &[u8]) -> TextEncoding {
    if bytes.starts_with(&UTF16_LE_BOM) {
        return TextEncoding::Utf16Le;
    }
    if bytes.starts_with(&UTF16_BE_BOM) {
        return TextEncoding::Utf16Be;
    }

    let marked = bytes.starts_with(&UTF8_BOM);
    let body = bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes);

    if std::str::from_utf8(body).is_ok() {
        return if marked {
            TextEncoding::Utf8Bom
        } else {
            TextEncoding::Utf8
        };
    }
    TextEncoding::Legacy(LegacyEncoding(guessed(body)))
}

fn guessed(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);

    detector.feed(bytes, true);
    detector.guess(None, Utf8Detection::Deny)
}

pub fn decoded_as(bytes: &[u8], encoding: TextEncoding) -> String {
    match encoding {
        TextEncoding::Utf16Le => wide(
            bytes.strip_prefix(&UTF16_LE_BOM).unwrap_or(bytes),
            u16::from_le_bytes,
        ),
        TextEncoding::Utf16Be => wide(
            bytes.strip_prefix(&UTF16_BE_BOM).unwrap_or(bytes),
            u16::from_be_bytes,
        ),
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => {
            String::from_utf8_lossy(bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes)).into_owned()
        }
        TextEncoding::Legacy(legacy) => legacy
            .0
            .decode_without_bom_handling(bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes))
            .0
            .into_owned(),
    }
}

pub fn restored(text: &str, encoding: TextEncoding, original: &[u8]) -> Option<Vec<u8>> {
    if decoded_as(original, encoding) == text {
        return Some(original.to_vec());
    }
    decoded_strictly(original, encoding)?;

    let written = [encoding.byte_order_mark(), &encoded(text, encoding)?].concat();

    (decoded_as(&written, encoding) == text).then_some(written)
}

fn decoded_strictly(bytes: &[u8], encoding: TextEncoding) -> Option<String> {
    match encoding {
        TextEncoding::Utf16Le => wide_strictly(
            bytes.strip_prefix(&UTF16_LE_BOM).unwrap_or(bytes),
            u16::from_le_bytes,
        ),
        TextEncoding::Utf16Be => wide_strictly(
            bytes.strip_prefix(&UTF16_BE_BOM).unwrap_or(bytes),
            u16::from_be_bytes,
        ),
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => {
            String::from_utf8(bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes).to_vec()).ok()
        }
        TextEncoding::Legacy(legacy) => legacy
            .0
            .decode_without_bom_handling_and_without_replacement(
                bytes.strip_prefix(&UTF8_BOM).unwrap_or(bytes),
            )
            .map(Cow::into_owned),
    }
}

pub fn encoded(text: &str, encoding: TextEncoding) -> Option<Vec<u8>> {
    match encoding {
        TextEncoding::Utf8 | TextEncoding::Utf8Bom => Some(text.as_bytes().to_vec()),
        TextEncoding::Utf16Le => Some(widened(text, u16::to_le_bytes)),
        TextEncoding::Utf16Be => Some(widened(text, u16::to_be_bytes)),
        TextEncoding::Legacy(legacy) => {
            let (bytes, _, unmappable) = legacy.0.encode(text);
            (!unmappable).then(|| bytes.into_owned())
        }
    }
}

fn widened(text: &str, order: fn(u16) -> [u8; 2]) -> Vec<u8> {
    text.encode_utf16().flat_map(order).collect()
}

fn units_of(bytes: &[u8], order: fn([u8; 2]) -> u16) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .copied()
        .map(order)
        .collect()
}

fn wide(bytes: &[u8], order: fn([u8; 2]) -> u16) -> String {
    String::from_utf16_lossy(&units_of(bytes, order))
}

fn wide_strictly(bytes: &[u8], order: fn([u8; 2]) -> u16) -> Option<String> {
    let whole = bytes
        .len()
        .is_multiple_of(TextEncoding::Utf16Le.unit_bytes());

    whole
        .then(|| String::from_utf16(&units_of(bytes, order)).ok())
        .flatten()
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

    fn legacy(text: &str, encoding: LegacyEncoding) -> Vec<u8> {
        encoded(text, TextEncoding::Legacy(encoding)).expect("the text has letters there")
    }

    #[test]
    fn a_wide_encoding_names_the_mark_that_makes_its_bytes_read_back_as_it() {
        for (encoding, little) in [
            (TextEncoding::Utf16Le, true),
            (TextEncoding::Utf16Be, false),
        ] {
            let mut written = encoding.byte_order_mark().to_vec();

            written.extend(encoded("Écoute", encoding).expect("every letter fits"));

            assert_eq!(written, utf16("Écoute", little));
            assert_eq!(decoded(&written), ("Écoute".to_owned(), encoding));
        }

        assert!(TextEncoding::Utf8.byte_order_mark().is_empty());
        assert_eq!(TextEncoding::Utf8Bom.byte_order_mark(), UTF8_BOM);
        assert!(TextEncoding::WINDOWS_1252.byte_order_mark().is_empty());
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

        assert_eq!(
            decoded(&marked),
            ("Écoute".to_owned(), TextEncoding::Utf8Bom)
        );
    }

    #[test]
    fn a_utf8_mark_ahead_of_text_that_is_not_utf8_is_dropped_with_the_code_page_read() {
        let mut marked = UTF8_BOM.to_vec();

        marked.extend_from_slice(b"FILE \"Caf\xe9.flac\" WAVE\n");

        let (text, encoding) = decoded(&marked);

        assert_eq!(text, "FILE \"Café.flac\" WAVE\n");
        assert_eq!(encoding, TextEncoding::WINDOWS_1252);
    }

    #[test]
    fn text_that_is_not_utf8_is_read_in_the_code_page_its_letters_belong_to() {
        let cases = [
            (
                "TITLE \"Кино — Группа крови\"\nPERFORMER \"Виктор Цой\"\n",
                LegacyEncoding::WINDOWS_1251,
            ),
            (
                "TITLE \"ファイナルファンタジー オリジナル・サウンドトラック\"\nPERFORMER \"植松伸夫\"\n",
                LegacyEncoding::SHIFT_JIS,
            ),
            (
                "TITLE \"周杰伦 七里香 我的地盘 借口 外婆\"\nPERFORMER \"周杰伦\"\n",
                LegacyEncoding::GBK,
            ),
            (
                "TITLE \"周杰倫 七里香 我的地盤 藉口 外婆\"\nPERFORMER \"周杰倫\"\n",
                LegacyEncoding::BIG5,
            ),
            (
                "TITLE \"Écoute — “Déjà vu” à l’été\"\n",
                LegacyEncoding::WINDOWS_1252,
            ),
        ];

        for (text, encoding) in cases {
            assert_eq!(
                decoded(&legacy(text, encoding)),
                (text.to_owned(), TextEncoding::Legacy(encoding)),
                "{}",
                encoding.name()
            );
        }
    }

    #[test]
    fn text_is_written_back_in_the_encoding_it_was_read_in_where_that_holds_its_letters() {
        let wide = TextEncoding::Utf16Le;

        assert_eq!(
            decoded_as(&encoded("Écoute", wide).expect("any letter"), wide),
            "Écoute"
        );

        let cyrillic = TextEncoding::Legacy(LegacyEncoding::WINDOWS_1251);

        assert_eq!(encoded("Цой", cyrillic), Some(vec![0xD6, 0xEE, 0xE9]));
        assert_eq!(encoded("植松", cyrillic), None);
    }

    #[test]
    fn duplicate_shift_jis_codes_are_written_back_as_they_were_read() {
        let japanese = TextEncoding::Legacy(LegacyEncoding::SHIFT_JIS);
        let original = b"\x87\x90";

        let text = decoded_as(original, japanese);

        assert_eq!(text, "\u{2252}");
        assert_eq!(encoded(&text, japanese), Some(vec![0x81, 0xE0]));
        assert_eq!(restored(&text, japanese, original), Some(original.to_vec()));
    }

    #[test]
    fn an_odd_utf16_tail_is_kept_by_an_unchanged_text_and_refused_by_a_changed_one() {
        let mut original = utf16("Écoute", true);

        original.push(0x41);

        let (text, encoding) = decoded(&original);

        assert_eq!(restored(&text, encoding, &original), Some(original.clone()));
        assert_eq!(restored("Autre", encoding, &original), None);
    }

    #[test]
    fn the_utf8_mark_is_written_back_with_the_text_that_carried_it() {
        let mut marked = UTF8_BOM.to_vec();

        marked.extend_from_slice("Écoute".as_bytes());

        let (text, encoding) = decoded(&marked);

        assert_eq!(
            restored("Autre", encoding, &marked).map(|bytes| bytes.starts_with(&UTF8_BOM)),
            Some(true)
        );
        assert_eq!(restored(&text, encoding, &marked), Some(marked.clone()));
        assert_eq!(
            restored("Autre", TextEncoding::Utf8, b"Ecoute"),
            Some(b"Autre".to_vec())
        );
    }

    #[test]
    fn text_read_with_replacements_is_not_written_back_as_another_text() {
        let broken = b"Caf\xe9 \xff";

        assert_eq!(restored("Café", TextEncoding::Utf8, broken), None);
        assert_eq!(
            restored(
                "Autre",
                TextEncoding::Legacy(LegacyEncoding::SHIFT_JIS),
                b"\x81"
            ),
            None
        );
    }
}
