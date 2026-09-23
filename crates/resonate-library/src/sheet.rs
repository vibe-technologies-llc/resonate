use std::{
    borrow::Cow,
    ffi::OsStr,
    fs::{self, File},
    io::Read as _,
    path::{self, Component, Path, PathBuf},
};

use resonate_core::MediaLocation;

use crate::{Error, PlaylistEntry, PlaylistFormat, Result, SheetEncoding, m3u, pls, store, xspf};

const LARGEST_PLAYLIST_FILE: u64 = 8 * 1024 * 1024;

const STAGING_SUFFIX: &str = ".new";

const SCHEME_SEPARATOR: &str = "://";

const WINDOWS_SEPARATOR: char = '\\';

const FILE_SCHEME: &str = "file://";

const LOCAL_AUTHORITY: &str = "localhost";

const BYTE_ORDER_MARK: char = '\u{feff}';

const NOTHING_TEXT_HOLDS: u8 = 0;

const A_COMMENT: char = '#';

const LINE_BREAKS: [char; 2] = ['\n', '\r'];

const UTF8_BY_DEFINITION: [&str; 2] = ["m3u8", "xspf"];

const UNRESERVED: [char; 4] = ['-', '.', '_', '~'];

const WINDOWS_1252_ABOVE_LATIN1: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

const WINDOWS_1252_FLOOR: u8 = 0x80;

const WINDOWS_1252_CEILING: u8 = 0x9f;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sheet {
    pub declared: Option<String>,
    pub locations: Vec<MediaLocation>,
    pub elsewhere: usize,
    pub promised: Option<usize>,
}

impl Sheet {
    pub fn short(&self) -> usize {
        let held = self.locations.len() + self.elsewhere;
        self.promised
            .map_or(0, |promised| promised.saturating_sub(held))
    }
}

pub struct Reading {
    pub sheet: Sheet,
    pub format: PlaylistFormat,
    pub encoding: SheetEncoding,
}

pub struct Described {
    pub seconds: Option<u64>,
    pub artist: Option<String>,
    pub title: String,
}

pub fn read(path: &Path) -> Result<Reading> {
    let (text, encoding) = decoded(path)?;
    let beside = path.parent().unwrap_or_else(|| Path::new("."));
    let (sheet, format) = parse(&text, beside);

    Ok(Reading {
        sheet,
        format,
        encoding,
    })
}

pub fn parse(text: &str, beside: &Path) -> (Sheet, PlaylistFormat) {
    let format = sniffed(text);
    let sheet = match format {
        PlaylistFormat::M3u => m3u::read(text, beside),
        PlaylistFormat::Pls => pls::read(text, beside),
        PlaylistFormat::Xspf => xspf::read(text, beside),
    };

    (sheet, format)
}

pub fn write(path: &Path, name: &str, entries: &[PlaylistEntry]) -> Result<PlaylistFormat> {
    let beside = path.parent().unwrap_or_else(|| Path::new("."));
    let format = PlaylistFormat::of(path);
    let text = match format {
        PlaylistFormat::M3u => m3u::write(name, entries, beside)?,
        PlaylistFormat::Pls => pls::write(name, entries, beside)?,
        PlaylistFormat::Xspf => xspf::write(name, entries, beside)?,
    };

    staged_over(path, &text)?;
    Ok(format)
}

pub fn describe(entry: &PlaylistEntry) -> Described {
    let Some(track) = entry.track.as_ref() else {
        return Described {
            seconds: None,
            artist: None,
            title: stem_of(entry.location()),
        };
    };

    Described {
        seconds: track
            .duration
            .map(|frames| frames.to_duration(track.spec.rate).as_secs()),
        artist: track.artist.clone(),
        title: track.title.clone(),
    }
}

pub fn located(named: &str, beside: &Path) -> Option<MediaLocation> {
    match scheme_of(named) {
        None => Some(resolved(
            PathBuf::from(forward_separated(named).as_ref()),
            beside,
        )),
        Some(_) => from_uri(named),
    }
}

fn forward_separated(named: &str) -> Cow<'_, str> {
    if named.contains(WINDOWS_SEPARATOR) && !named.contains('/') {
        return Cow::Owned(named.replace(WINDOWS_SEPARATOR, "/"));
    }
    Cow::Borrowed(named)
}

pub fn from_uri(named: &str) -> Option<MediaLocation> {
    Some(canonical(PathBuf::from(local_file(named)?)))
}

pub fn resolved(file: PathBuf, beside: &Path) -> MediaLocation {
    canonical(if file.is_absolute() {
        file
    } else {
        beside.join(file)
    })
}

pub fn canonical(file: PathBuf) -> MediaLocation {
    let found = file.canonicalize();

    MediaLocation::local(found.unwrap_or_else(|_| settled(&file)))
}

fn settled(file: &Path) -> PathBuf {
    let held = path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
    let mut whole = PathBuf::new();

    for part in held.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                whole.pop();
            }
            part => whole.push(part),
        }
    }
    whole
}

pub fn scheme_of(line: &str) -> Option<&str> {
    let (scheme, _) = line.split_once(SCHEME_SEPARATOR)?;
    let named = scheme.starts_with(|first: char| first.is_ascii_alphabetic())
        && scheme.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
        });

    named.then_some(scheme)
}

pub fn local_file(line: &str) -> Option<String> {
    let encoded = line.strip_prefix(FILE_SCHEME)?;
    let encoded = encoded.strip_prefix(LOCAL_AUTHORITY).unwrap_or(encoded);
    if !encoded.starts_with('/') {
        return None;
    }
    unescaped(encoded)
}

pub fn unescaped(encoded: &str) -> Option<String> {
    let mut decoded = Vec::with_capacity(encoded.len());
    let mut bytes = encoded.bytes();

    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            decoded.push(byte);
            continue;
        }
        let high = hex(bytes.next()?)?;
        let low = hex(bytes.next()?)?;
        decoded.push(high << 4 | low);
    }
    if decoded.contains(&NOTHING_TEXT_HOLDS) {
        return None;
    }

    String::from_utf8(decoded).ok()
}

pub fn escaped(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());

    for byte in path.bytes() {
        let character = byte as char;
        if character.is_ascii_alphanumeric() || UNRESERVED.contains(&character) || character == '/'
        {
            encoded.push(character);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

pub fn beside_or_absolute(file: &Path, beside: &Path) -> PathBuf {
    file.strip_prefix(beside)
        .map_or_else(|_| file.to_path_buf(), Path::to_path_buf)
}

pub fn as_a_row(file: &Path, beside: &Path) -> Result<String> {
    let named = beside_or_absolute(file, beside);
    let row = store::path_text(&named)?;
    if reads_back_as_itself(row) {
        return Ok(row.to_owned());
    }

    Ok(format!("{FILE_SCHEME}{}", escaped(store::path_text(file)?)))
}

fn reads_back_as_itself(row: &str) -> bool {
    !row.is_empty()
        && row.trim() == row
        && !row.starts_with(A_COMMENT)
        && !row.contains(LINE_BREAKS)
        && scheme_of(row).is_none()
        && matches!(forward_separated(row), Cow::Borrowed(_))
}

pub fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn stem_of(location: &MediaLocation) -> String {
    location
        .stem()
        .map_or_else(|| location.to_string(), |stem| stem.into_owned())
}

fn sniffed(text: &str) -> PlaylistFormat {
    let Some(first) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return PlaylistFormat::M3u;
    };

    if first.starts_with('<') {
        return PlaylistFormat::Xspf;
    }
    if first.eq_ignore_ascii_case(pls::HEADER) {
        return PlaylistFormat::Pls;
    }
    PlaylistFormat::M3u
}

fn decoded(path: &Path) -> Result<(String, SheetEncoding)> {
    let bytes = within_the_limit(path)?;

    match String::from_utf8(bytes) {
        Ok(text) => Ok((without_a_mark(text), SheetEncoding::Utf8)),
        Err(refused) => Ok((
            legacy_text(path, refused.as_bytes())?,
            SheetEncoding::Windows1252,
        )),
    }
}

fn legacy_text(path: &Path, bytes: &[u8]) -> Result<String> {
    if declares_utf8(path) || bytes.contains(&NOTHING_TEXT_HOLDS) {
        return Err(Error::NonUtf8PlaylistFile {
            path: path.to_path_buf(),
        });
    }

    Ok(bytes
        .iter()
        .map(|byte| match byte {
            WINDOWS_1252_FLOOR..=WINDOWS_1252_CEILING => {
                WINDOWS_1252_ABOVE_LATIN1[usize::from(byte - WINDOWS_1252_FLOOR)]
            }
            byte => char::from(*byte),
        })
        .collect())
}

fn declares_utf8(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| {
            UTF8_BY_DEFINITION
                .iter()
                .any(|named| named.eq_ignore_ascii_case(extension))
        })
}

fn without_a_mark(text: String) -> String {
    match text.strip_prefix(BYTE_ORDER_MARK) {
        Some(stripped) => stripped.to_owned(),
        None => text,
    }
}

fn within_the_limit(path: &Path) -> Result<Vec<u8>> {
    let declared = fs::metadata(path)
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?
        .len();
    if declared > LARGEST_PLAYLIST_FILE {
        return Err(too_large(path, declared));
    }

    let mut taken = Vec::new();
    File::open(path)
        .and_then(|sheet| {
            sheet
                .take(LARGEST_PLAYLIST_FILE + 1)
                .read_to_end(&mut taken)
        })
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;

    let read = taken.len() as u64;
    if read > LARGEST_PLAYLIST_FILE {
        return Err(too_large(path, declared.max(read)));
    }
    Ok(taken)
}

fn too_large(path: &Path, held: u64) -> Error {
    Error::PlaylistFileTooLarge {
        path: path.to_path_buf(),
        held,
        limit: LARGEST_PLAYLIST_FILE,
    }
}

fn staged_over(path: &Path, text: &str) -> Result<()> {
    let Some(name) = path.file_name() else {
        return Err(Error::NotAPlaylistFile {
            path: path.to_path_buf(),
        });
    };

    let mut staging = name.to_os_string();
    staging.push(STAGING_SUFFIX);
    let staged = path.with_file_name(staging);

    fs::write(&staged, text)
        .and_then(|()| fs::rename(&staged, path))
        .map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })
}

const fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_whose_name_holds_a_backslash_is_written_as_a_row_that_reads_back_as_it() {
        let beside = Path::new("/music");
        let file = Path::new("/music/AC\\DC.wav");

        let row = as_a_row(file, beside).expect("a row");
        assert!(
            row.starts_with(FILE_SCHEME),
            "{row} reads back as a Windows path"
        );
        assert_eq!(
            located(&row, beside).and_then(|held| held.as_path().map(Path::to_path_buf)),
            Some(file.to_path_buf())
        );
    }

    #[test]
    fn a_row_a_windows_player_wrote_still_reads_with_its_separators_turned() {
        let beside = Path::new("/music");

        assert_eq!(
            located("Pink Floyd\\Echoes.flac", beside)
                .and_then(|held| held.as_path().map(Path::to_path_buf)),
            Some(PathBuf::from("/music/Pink Floyd/Echoes.flac"))
        );
    }
}
