use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
};

#[cfg(unix)]
pub fn bytes_of(name: &OsStr) -> Cow<'_, [u8]> {
    use std::os::unix::ffi::OsStrExt as _;

    Cow::Borrowed(name.as_bytes())
}

#[cfg(unix)]
pub fn os_string_from(bytes: Vec<u8>) -> OsString {
    use std::os::unix::ffi::OsStringExt as _;

    OsString::from_vec(bytes)
}

#[cfg(windows)]
pub fn bytes_of(name: &OsStr) -> Cow<'_, [u8]> {
    use std::os::windows::ffi::OsStrExt as _;

    match name.to_str() {
        Some(text) => Cow::Borrowed(text.as_bytes()),
        None => Cow::Owned(wtf8_of_wide(name.encode_wide())),
    }
}

#[cfg(windows)]
pub fn os_string_from(bytes: Vec<u8>) -> OsString {
    use std::os::windows::ffi::OsStringExt as _;

    match String::from_utf8(bytes) {
        Ok(text) => OsString::from(text),
        Err(refused) => OsString::from_wide(&wide_of_wtf8(refused.as_bytes())),
    }
}

#[cfg(any(windows, test))]
const CONTINUATION_MARK: u8 = 0x80;
#[cfg(any(windows, test))]
const CONTINUATION_BITS: u32 = 6;
#[cfg(any(windows, test))]
const CONTINUATION_MASK: u32 = 0x3F;
#[cfg(any(windows, test))]
const REPLACEMENT: u16 = 0xFFFD;
#[cfg(any(windows, test))]
const ASTRAL_BASE: u32 = 0x1_0000;
#[cfg(any(windows, test))]
const HIGH_SURROGATE_BASE: u32 = 0xD800;
#[cfg(any(windows, test))]
const LOW_SURROGATE_BASE: u32 = 0xDC00;
#[cfg(any(windows, test))]
const SURROGATE_BITS: u32 = 10;
#[cfg(any(windows, test))]
const SURROGATE_MASK: u32 = 0x3FF;

#[cfg(any(windows, test))]
pub fn wtf8_of_wide(units: impl Iterator<Item = u16>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for unit in char::decode_utf16(units) {
        match unit {
            Ok(letter) => {
                let mut buffer = [0; 4];
                bytes.extend_from_slice(letter.encode_utf8(&mut buffer).as_bytes());
            }
            Err(lone) => push_three_bytes(&mut bytes, u32::from(lone.unpaired_surrogate())),
        }
    }
    bytes
}

#[cfg(any(windows, test))]
fn push_three_bytes(bytes: &mut Vec<u8>, code: u32) {
    let lead = 0xE0 | (code >> (2 * CONTINUATION_BITS));
    let middle = u32::from(CONTINUATION_MARK) | ((code >> CONTINUATION_BITS) & CONTINUATION_MASK);
    let last = u32::from(CONTINUATION_MARK) | (code & CONTINUATION_MASK);
    for byte in [lead, middle, last] {
        bytes.push(u8::try_from(byte).unwrap_or_default());
    }
}

#[cfg(any(windows, test))]
pub fn wide_of_wtf8(bytes: &[u8]) -> Vec<u16> {
    let mut units = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(&lead) = bytes.get(at) {
        let (extra, first_bits) = match lead {
            0x00..=0x7F => (0, u32::from(lead)),
            0xC2..=0xDF => (1, u32::from(lead & 0x1F)),
            0xE0..=0xEF => (2, u32::from(lead & 0x0F)),
            0xF0..=0xF4 => (3, u32::from(lead & 0x07)),
            _ => {
                units.push(REPLACEMENT);
                at += 1;
                continue;
            }
        };
        let tail = bytes.get(at + 1..at + 1 + extra);
        let code = tail.and_then(|tail| {
            tail.iter().try_fold(first_bits, |code, byte| {
                (byte & 0xC0 == CONTINUATION_MARK)
                    .then(|| (code << CONTINUATION_BITS) | (u32::from(*byte) & CONTINUATION_MASK))
            })
        });
        let Some(code) = code else {
            units.push(REPLACEMENT);
            at += 1;
            continue;
        };
        at += 1 + extra;
        push_units(&mut units, code);
    }
    units
}

#[cfg(any(windows, test))]
fn push_units(units: &mut Vec<u16>, code: u32) {
    match code.checked_sub(ASTRAL_BASE) {
        Some(astral) => {
            let high = HIGH_SURROGATE_BASE + (astral >> SURROGATE_BITS);
            let low = LOW_SURROGATE_BASE + (astral & SURROGATE_MASK);
            units.extend([high, low].map(|unit| u16::try_from(unit).unwrap_or(REPLACEMENT)));
        }
        None => units.push(u16::try_from(code).unwrap_or(REPLACEMENT)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(units: &[u16]) -> Vec<u16> {
        wide_of_wtf8(&wtf8_of_wide(units.iter().copied()))
    }

    #[test]
    fn a_plain_name_is_its_own_bytes() {
        let name = OsStr::new("Échoes/01.flac");

        assert_eq!(bytes_of(name).as_ref(), "Échoes/01.flac".as_bytes());
        assert_eq!(os_string_from("Échoes/01.flac".into()), name);
    }

    #[test]
    fn a_lone_surrogate_survives_the_bytes_it_is_written_as() {
        let units = [0x0061, 0xD800, 0x0062, 0xDFFF, 0x0063];

        assert_eq!(round_trip(&units), units);
        assert_eq!(
            wtf8_of_wide(units.iter().copied()),
            [b'a', 0xED, 0xA0, 0x80, b'b', 0xED, 0xBF, 0xBF, b'c']
        );
    }

    #[test]
    fn a_pair_of_surrogates_is_one_letter_of_four_bytes() {
        let units = [0xD83C, 0xDFB5];

        assert_eq!(wtf8_of_wide(units.iter().copied()), "🎵".as_bytes());
        assert_eq!(round_trip(&units), units);
    }

    #[test]
    fn bytes_that_are_no_text_become_the_replacement_letter() {
        assert_eq!(wide_of_wtf8(b"a\xffb"), [0x61, 0xFFFD, 0x62]);
        assert_eq!(wide_of_wtf8(b"\xe2\x82"), [0xFFFD, 0xFFFD]);
    }
}
