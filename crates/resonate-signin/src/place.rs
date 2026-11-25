pub const SIGN_IN_PAGE: &str =
    "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/";

const SECURE_SCHEME: &str = "https://";

const YOUTUBE_MUSIC: &str = "music.youtube.com";

const AUTHORITY_ENDS_AT: [char; 3] = ['/', '?', '#'];

const PORT_FOLLOWS: char = ':';

const SECURE_PORT: &str = "443";

const GOOGLE_AND_YOUTUBE: [&str; 2] = ["google.com", "youtube.com"];

fn host_of_secure(address: &str) -> Option<&str> {
    let rest = address.strip_prefix(SECURE_SCHEME)?;
    let authority = rest.split(AUTHORITY_ENDS_AT).next()?;
    let (host, port) = match authority.split_once(PORT_FOLLOWS) {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };

    let port_is_secure = port.is_none_or(|port| port == SECURE_PORT);
    let host_is_plain = !host.is_empty()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'.');

    (port_is_secure && host_is_plain).then_some(host)
}

pub fn is_youtube_music(address: &str) -> bool {
    host_of_secure(address).is_some_and(|host| host.eq_ignore_ascii_case(YOUTUBE_MUSIC))
}

pub fn may_lead_to(address: &str) -> bool {
    let Some(host) = host_of_secure(address) else {
        return false;
    };
    let host = host.to_ascii_lowercase();

    GOOGLE_AND_YOUTUBE.iter().any(|domain| {
        host == *domain
            || host
                .strip_suffix(domain)
                .is_some_and(|front| front.ends_with('.'))
    })
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
        assert!(!is_youtube_music("https://music.youtube.com:x@evil.net/"));
        assert!(!is_youtube_music("https://music.youtube.com:443@evil.net/"));
        assert!(!is_youtube_music("https://music.youtube.com:8443/"));
        assert!(!is_youtube_music("https://music.youtube.com:/x@y"));
        assert!(!is_youtube_music("https://evil.net\\@music.youtube.com/"));
        assert!(!is_youtube_music("https://[::1]:443@music.youtube.com/"));
        assert!(!is_youtube_music(
            "https://accounts.google.com/ServiceLogin?service=youtube&continue=https://music.youtube.com/"
        ));
    }

    #[test]
    fn a_navigation_may_lead_to_https_google_and_youtube_hosts_alone() {
        assert!(may_lead_to(SIGN_IN_PAGE));
        assert!(may_lead_to("https://accounts.google.com/v3/signin"));
        assert!(may_lead_to("https://consent.youtube.com/m?continue=x"));
        assert!(may_lead_to("https://consent.google.com/"));
        assert!(may_lead_to("https://accounts.youtube.com:443/"));
        assert!(may_lead_to("https://Music.YouTube.com/"));
        assert!(may_lead_to("https://google.com/"));
        assert!(may_lead_to("https://youtube.com"));
        assert!(!may_lead_to("https://evil.net/"));
        assert!(!may_lead_to("http://accounts.google.com/"));
        assert!(!may_lead_to("https://google.com.evil.net/"));
        assert!(!may_lead_to("https://notgoogle.com/"));
        assert!(!may_lead_to("https://evilyoutube.com/"));
        assert!(!may_lead_to("https://accounts.google.com@evil.net/"));
        assert!(!may_lead_to("https://accounts.google.com:x@evil.net/"));
        assert!(!may_lead_to("https://evil.net\\.accounts.google.com/"));
        assert!(!may_lead_to("https://evil.net/accounts.google.com/"));
        assert!(!may_lead_to("https://accounts.google.com:8443/"));
        assert!(!may_lead_to("https://accounts.google.com./"));
        assert!(!may_lead_to("https://%61ccounts.google.com/"));
        assert!(!may_lead_to("about:blank"));
        assert!(!may_lead_to("file:///etc/passwd"));
        assert!(!may_lead_to("data:text/html,x"));
        assert!(!may_lead_to("https://"));
    }
}
