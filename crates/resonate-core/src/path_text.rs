use std::{
    borrow::Cow,
    iter::Peekable,
    path::{Path, PathBuf},
    str::Chars,
};

use crate::os_bytes;

const ESCAPE: char = '\u{FDD0}';
const VERBATIM_SHARE: &str = r"\\?\UNC\";
const VERBATIM: &str = r"\\?\";
const SHARE: &str = r"\\";
const HEX_DIGITS: u32 = 2;
const HEX_RADIX: u32 = 16;

pub fn encoded(path: &Path) -> Cow<'_, str> {
    if let Some(text) = path.to_str()
        && !text.contains(ESCAPE)
    {
        return Cow::Borrowed(text);
    }

    let mut text = String::with_capacity(path.as_os_str().len());
    for chunk in os_bytes::bytes_of(path.as_os_str()).utf8_chunks() {
        for letter in chunk.valid().chars() {
            if letter == ESCAPE {
                text.push(ESCAPE);
            }
            text.push(letter);
        }
        for byte in chunk.invalid() {
            text.push(ESCAPE);
            text.push_str(&format!("{byte:02X}"));
        }
    }
    Cow::Owned(text)
}

pub fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    if cfg!(windows) {
        return without_verbatim(&text).into_owned();
    }
    text
}

fn without_verbatim(text: &str) -> Cow<'_, str> {
    if let Some(share) = text.strip_prefix(VERBATIM_SHARE) {
        return Cow::Owned(format!("{SHARE}{share}"));
    }
    text.strip_prefix(VERBATIM)
        .map_or(Cow::Borrowed(text), Cow::Borrowed)
}

pub fn decoded(text: &str) -> PathBuf {
    if !text.contains(ESCAPE) {
        return PathBuf::from(text);
    }

    let mut bytes = Vec::with_capacity(text.len());
    let mut letters = text.chars().peekable();
    while let Some(letter) = letters.next() {
        if letter != ESCAPE {
            push_letter(&mut bytes, letter);
        } else if letters.next_if_eq(&ESCAPE).is_some() {
            push_letter(&mut bytes, ESCAPE);
        } else if let Some(byte) = byte_after_escape(&mut letters) {
            bytes.push(byte);
        } else {
            push_letter(&mut bytes, ESCAPE);
        }
    }
    PathBuf::from(os_bytes::os_string_from(bytes))
}

fn push_letter(bytes: &mut Vec<u8>, letter: char) {
    let mut buffer = [0; 4];
    bytes.extend_from_slice(letter.encode_utf8(&mut buffer).as_bytes());
}

fn byte_after_escape(letters: &mut Peekable<Chars<'_>>) -> Option<u8> {
    let mut ahead = letters.clone();
    let mut value = 0;
    for _ in 0..HEX_DIGITS {
        value = value * HEX_RADIX + ahead.next()?.to_digit(HEX_RADIX)?;
    }
    *letters = ahead;
    u8::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_prefix_is_not_shown() {
        assert_eq!(
            without_verbatim(r"\\?\C:\Users\cb\Desktop"),
            r"C:\Users\cb\Desktop"
        );
        assert_eq!(without_verbatim(r"\\?\UNC\nas\music"), r"\\nas\music");
        assert_eq!(without_verbatim(r"C:\Music"), r"C:\Music");
        assert_eq!(without_verbatim("/home/music"), "/home/music");
    }

    #[cfg(unix)]
    fn named(bytes: &[u8]) -> PathBuf {
        PathBuf::from(os_bytes::os_string_from(bytes.to_vec()))
    }

    #[test]
    fn a_utf8_path_is_its_own_text() {
        let path = Path::new("/music/Échoes/01 Écho.flac");

        assert!(matches!(encoded(path), Cow::Borrowed(_)));
        assert_eq!(encoded(path), "/music/Échoes/01 Écho.flac");
        assert_eq!(decoded("/music/Échoes/01 Écho.flac"), path);
    }

    #[test]
    #[cfg(unix)]
    fn a_path_that_is_not_utf8_comes_back_byte_for_byte() {
        for bytes in [
            &b"/music/caf\xe9.flac"[..],
            b"/music/\xff\xfe/a.flac",
            b"/music/\xc3.flac",
            b"/music/\xf0\x9f.flac",
            b"\xed\xa0\x80",
        ] {
            let path = named(bytes);
            let text = encoded(&path);

            assert_eq!(
                os_bytes::bytes_of(decoded(&text).as_os_str()).as_ref(),
                bytes
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn two_paths_never_share_a_text() {
        let lookalike = PathBuf::from(format!("/music/{ESCAPE}E9.flac"));
        let real = named(b"/music/\xe9.flac");

        assert_ne!(encoded(&lookalike), encoded(&real));
        assert_eq!(decoded(&encoded(&lookalike)), lookalike);
        assert_eq!(decoded(&encoded(&real)), real);
    }

    #[test]
    #[cfg(unix)]
    fn the_text_of_a_folder_is_a_prefix_of_the_text_of_what_lies_in_it() {
        let folder = named(b"/music/caf\xe9");
        let file = folder.join("a.flac");

        assert!(encoded(&file).starts_with(&*encoded(&folder)));
    }

    #[test]
    #[cfg(windows)]
    fn a_path_with_a_lone_surrogate_comes_back_unit_for_unit() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt as _};

        let path = PathBuf::from(OsString::from_wide(&[0x43, 0x3A, 0x5C, 0xD800, 0x2E, 0x66]));
        let text = encoded(&path);

        assert_eq!(decoded(&text), path);
    }
}
