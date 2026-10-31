use crate::media::uri_escaped;

const BACKSLASH: u8 = b'\\';
const SLASH: u8 = b'/';
const DRIVE_MARK: u8 = b':';
const VERBATIM_UNC: &[u8] = br"\\?\UNC\";
const VERBATIM: &[u8] = br"\\?\";
const NETWORK: &[u8] = br"\\";

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
    let (&first, rooted) = decoded.split_first()?;
    if first != SLASH {
        let mut network = NETWORK.to_vec();
        network.extend(backwards(decoded));
        return Some(network);
    }
    match (drive_of(rooted), rooted.get(2)) {
        (Some(_), None | Some(&SLASH)) => Some(backwards(rooted)),
        _ => Some(backwards(decoded)),
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
    fn nothing_is_no_path() {
        assert_eq!(path_of_uri_rest(b""), None);
    }
}
