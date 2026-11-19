use std::borrow::Cow;

use crate::media::uri_escaped;

const BACKSLASH: u8 = b'\\';
const SLASH: u8 = b'/';
const DRIVE_MARK: u8 = b':';
const VERBATIM_UNC: &[u8] = br"\\?\UNC\";
const VERBATIM: &[u8] = br"\\?\";
const NETWORK: &[u8] = br"\\";
const NOT_A_SHARE: &[u8] = b"?.";
const DOT_SEGMENTS: [&[u8]; 2] = [b".", b".."];

pub fn uri_rest_of(path: &[u8]) -> String {
    if let Some(network) = path.strip_prefix(VERBATIM_UNC) {
        return uri_escaped(&forwards(network));
    }
    let path = path.strip_prefix(VERBATIM).unwrap_or(path);
    if let Some(network) = path.strip_prefix(NETWORK) {
        return uri_escaped(&forwards(network));
    }
    match (drive_of(path), path.get(2..)) {
        (Some(letter), Some(rest)) => {
            format!("/{}:{}", char::from(letter), uri_escaped(&forwards(rest)))
        }
        _ => uri_escaped(&forwards(path)),
    }
}

pub fn path_of_uri_rest(decoded: &[u8]) -> Option<Vec<u8>> {
    if decoded
        .split(|byte| *byte == SLASH || *byte == BACKSLASH)
        .any(|segment| DOT_SEGMENTS.contains(&segment))
    {
        return None;
    }
    let (&first, rooted) = decoded.split_first()?;
    if first != SLASH {
        let mut network = NETWORK.to_vec();
        network.extend(backwards(decoded));
        return Some(network);
    }
    match (drive_of(rooted), rooted.get(2)) {
        (Some(_), None | Some(&SLASH)) => Some(backwards(&drive_folded(rooted))),
        _ => Some(backwards(decoded)),
    }
}

pub(crate) fn verbatim(path: Vec<u8>) -> Vec<u8> {
    if let Some(prefixed) = path.strip_prefix(VERBATIM) {
        return match drive_folded(prefixed) {
            Cow::Owned(folded) => [VERBATIM, &folded].concat(),
            Cow::Borrowed(_) => path,
        };
    }
    if drive_of(&path).is_some() && path.get(2) == Some(&BACKSLASH) {
        return [VERBATIM, &drive_folded(&path)].concat();
    }
    match path.strip_prefix(NETWORK) {
        Some(share)
            if share
                .first()
                .is_some_and(|first| !NOT_A_SHARE.contains(first)) =>
        {
            [VERBATIM_UNC, share].concat()
        }
        _ => path,
    }
}

pub(crate) fn plain(path: &[u8]) -> Cow<'_, [u8]> {
    if let Some(share) = path.strip_prefix(VERBATIM_UNC) {
        return Cow::Owned([NETWORK, share].concat());
    }
    Cow::Borrowed(path.strip_prefix(VERBATIM).unwrap_or(path))
}

pub(crate) fn beneath(file: &[u8], folder: &[u8]) -> Option<Vec<u8>> {
    let file = backwards(&plain(file));
    let folder = backwards(&plain(folder));
    let folder = folder.strip_suffix(&[BACKSLASH]).unwrap_or(&folder);
    let (head, rest) = file.split_at_checked(folder.len())?;
    let rest = rest.strip_prefix(&[BACKSLASH])?;

    (drive_folded(head) == drive_folded(folder) && !rest.is_empty()).then(|| rest.to_vec())
}

fn drive_folded(path: &[u8]) -> Cow<'_, [u8]> {
    match (drive_of(path), path) {
        (Some(letter), [_, rest @ ..]) => {
            Cow::Owned([&[letter.to_ascii_uppercase()], rest].concat())
        }
        _ => Cow::Borrowed(path),
    }
}

fn drive_of(path: &[u8]) -> Option<u8> {
    match path {
        [letter, DRIVE_MARK, ..] if letter.is_ascii_alphabetic() => Some(*letter),
        _ => None,
    }
}

fn forwards(path: &[u8]) -> Vec<u8> {
    path.iter()
        .map(|byte| if *byte == BACKSLASH { SLASH } else { *byte })
        .collect()
}

fn backwards(path: &[u8]) -> Vec<u8> {
    path.iter()
        .map(|byte| if *byte == SLASH { BACKSLASH } else { *byte })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drive_path_is_a_rooted_uri_with_the_colon_kept() {
        assert_eq!(
            uri_rest_of(br"C:\Music\Meddle #1.flac"),
            "/C:/Music/Meddle%20%231.flac"
        );
        assert_eq!(
            path_of_uri_rest(b"/C:/Music/Meddle #1.flac"),
            Some(br"C:\Music\Meddle #1.flac".to_vec())
        );
    }

    #[test]
    fn a_drive_spelt_in_lower_case_is_read_back_in_the_case_canonicalising_writes() {
        assert_eq!(
            path_of_uri_rest(b"/c:/Music/a.flac"),
            Some(br"C:\Music\a.flac".to_vec())
        );
        assert_eq!(path_of_uri_rest(b"/d:"), Some(b"D:".to_vec()));
        assert_eq!(
            verbatim(br"c:\Music\a.flac".to_vec()),
            br"\\?\C:\Music\a.flac"
        );
        assert_eq!(verbatim(br"\\?\c:\a.flac".to_vec()), br"\\?\C:\a.flac");
        assert_eq!(
            verbatim(br"\\?\UNC\nas\music\a.flac".to_vec()),
            br"\\?\UNC\nas\music\a.flac"
        );
    }

    #[test]
    fn a_share_is_the_authority_of_the_uri() {
        assert_eq!(uri_rest_of(br"\\nas\music\a.flac"), "nas/music/a.flac");
        assert_eq!(
            path_of_uri_rest(b"nas/music/a.flac"),
            Some(br"\\nas\music\a.flac".to_vec())
        );
    }

    #[test]
    fn a_verbatim_path_is_the_path_it_prefixes() {
        assert_eq!(uri_rest_of(br"\\?\C:\a.flac"), "/C:/a.flac");
        assert_eq!(
            uri_rest_of(br"\\?\UNC\nas\music\a.flac"),
            "nas/music/a.flac"
        );
    }

    #[test]
    fn a_rooted_path_with_no_drive_stays_rooted() {
        assert_eq!(uri_rest_of(br"\music\a.flac"), "/music/a.flac");
        assert_eq!(
            path_of_uri_rest(b"/music/a.flac"),
            Some(br"\music\a.flac".to_vec())
        );
    }

    #[test]
    fn a_drive_or_share_path_is_written_verbatim_as_canonicalising_writes_it() {
        assert_eq!(
            verbatim(br"C:\Music\a.flac".to_vec()),
            br"\\?\C:\Music\a.flac"
        );
        assert_eq!(
            verbatim(br"\\nas\music\a.flac".to_vec()),
            br"\\?\UNC\nas\music\a.flac"
        );
        assert_eq!(verbatim(br"\\?\C:\a.flac".to_vec()), br"\\?\C:\a.flac");
        assert_eq!(
            verbatim(br"\\.\pipe\resonate".to_vec()),
            br"\\.\pipe\resonate"
        );
        assert_eq!(verbatim(br"\music\a.flac".to_vec()), br"\music\a.flac");
        assert_eq!(verbatim(b"C:a.flac".to_vec()), b"C:a.flac");
    }

    #[test]
    fn a_verbatim_path_is_plain_again_without_its_prefix() {
        assert_eq!(plain(br"\\?\C:\Music"), &br"C:\Music"[..]);
        assert_eq!(plain(br"\\?\UNC\nas\music"), &br"\\nas\music"[..]);
        assert_eq!(plain(br"C:\Music"), &br"C:\Music"[..]);
        assert_eq!(plain(b"/home/music"), &b"/home/music"[..]);
    }

    #[test]
    fn a_file_beneath_a_folder_is_named_from_it_whichever_prefix_either_carries() {
        let echoes = Some(br"Pink Floyd\Echoes.flac".to_vec());

        assert_eq!(
            beneath(br"\\?\C:\Music\Pink Floyd\Echoes.flac", br"C:\Music"),
            echoes
        );
        assert_eq!(
            beneath(br"C:\Music\Pink Floyd\Echoes.flac", br"\\?\C:\Music"),
            echoes
        );
        assert_eq!(
            beneath(br"\\?\C:\Music\Pink Floyd\Echoes.flac", br"\\?\C:\Music\"),
            echoes
        );
        assert_eq!(
            beneath(br"C:\Music\Pink Floyd\Echoes.flac", br"c:/Music"),
            echoes
        );
        assert_eq!(
            beneath(
                br"\\?\UNC\nas\music\Pink Floyd\Echoes.flac",
                br"\\nas\music"
            ),
            echoes
        );
        assert_eq!(
            beneath(
                br"\\nas\music\Pink Floyd\Echoes.flac",
                br"\\?\UNC\nas\music"
            ),
            echoes
        );
        assert_eq!(
            beneath(br"\\?\C:\Echoes.flac", br"C:\"),
            Some(b"Echoes.flac".to_vec())
        );
    }

    #[test]
    fn a_file_elsewhere_is_not_beneath_the_folder() {
        assert_eq!(beneath(br"\\?\D:\Music\a.flac", br"C:\Music"), None);
        assert_eq!(beneath(br"\\?\C:\Musical\a.flac", br"C:\Music"), None);
        assert_eq!(beneath(br"\\?\C:\a.flac", br"C:\Music"), None);
        assert_eq!(beneath(br"\\?\C:\Music", br"C:\Music"), None);
        assert_eq!(beneath(br"\\nas\other\a.flac", br"\\nas\music"), None);
        assert_eq!(beneath(br"\\?\C:\Music\a.flac", br"\\nas\music"), None);
    }

    #[test]
    fn nothing_is_no_path() {
        assert_eq!(path_of_uri_rest(b""), None);
    }

    #[test]
    fn a_dot_segment_names_no_path() {
        assert_eq!(path_of_uri_rest(b"relative/../a.mp3"), None);
        assert_eq!(path_of_uri_rest(b"/C:/Music/./a.flac"), None);
        assert_eq!(path_of_uri_rest(b"/C:/Music/..\\a.flac"), None);
        assert!(path_of_uri_rest(b"/C:/Music/a..flac").is_some());
    }
}
