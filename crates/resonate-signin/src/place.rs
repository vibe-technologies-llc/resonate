pub const SIGN_IN_PAGE: &str =
    "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/";

const SECURE_SCHEME: &str = "https://";

const YOUTUBE_MUSIC: &str = "music.youtube.com";

const AUTHORITY_ENDS_AT: [char; 3] = ['/', '?', '#'];

const PORT_FOLLOWS: char = ':';

pub fn is_youtube_music(address: &str) -> bool {
    let Some(rest) = address.strip_prefix(SECURE_SCHEME) else {
        return false;
    };
    let authority = rest.split(AUTHORITY_ENDS_AT).next().unwrap_or_default();
    let host = authority.split(PORT_FOLLOWS).next().unwrap_or_default();
    host.eq_ignore_ascii_case(YOUTUBE_MUSIC)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_page_of_youtube_music_itself_is_youtube_music() {
        assert!(is_youtube_music("https://music.youtube.com/"));
        assert!(is_youtube_music("https://music.youtube.com"));
        assert!(is_youtube_music(
            "https://MUSIC.youtube.com:443/library?x=1#y"
        ));
        assert!(!is_youtube_music("http://music.youtube.com/"));
        assert!(!is_youtube_music("https://music.youtube.com.evil.net/"));
        assert!(!is_youtube_music("https://evil.net/music.youtube.com/"));
        assert!(!is_youtube_music("https://user@music.youtube.com/"));
        assert!(!is_youtube_music(
            "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/"
        ));
    }
}
