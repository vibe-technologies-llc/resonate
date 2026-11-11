use resonate_core::uri_unescaped;

const PLAIN_LINK: &str = "resonate://";

const SHARED_HOST: &str = "shared/";

const PARTY_HOST: &str = "party/";

const SHARED_PAGE: &str = "/s/#";

const PARTY_PAGE: &str = "/p/#";

const SERVER_QUERY: &str = "server=";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invited {
    Playlist,
    Party,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invitation {
    pub to: Invited,
    pub code: String,
    pub server: Option<String>,
}

impl Invitation {
    pub fn naming_a_server_other_than(&self, own: &str) -> Option<&str> {
        let named = self.server.as_deref()?;

        (!same_server(named, own)).then_some(named)
    }
}

fn same_server(one: &str, other: &str) -> bool {
    let bare = |server: &str| server.trim().trim_end_matches('/').to_ascii_lowercase();

    bare(one) == bare(other)
}

pub fn invitation_in(text: &str) -> Option<Invitation> {
    let text = text.trim();

    if let Some(rest) = stripped_ignoring_case(text, PLAIN_LINK) {
        let (to, rest) = if let Some(rest) = stripped_ignoring_case(rest, SHARED_HOST) {
            (Invited::Playlist, rest)
        } else {
            (Invited::Party, stripped_ignoring_case(rest, PARTY_HOST)?)
        };
        let (code, query) = rest.split_once('?').unwrap_or((rest, ""));

        return Some(Invitation {
            to,
            code: given_code(code)?,
            server: server_in(query),
        });
    }

    let (origin, to, code) = if let Some((origin, code)) = text.split_once(SHARED_PAGE) {
        (origin, Invited::Playlist, code)
    } else {
        let (origin, code) = text.split_once(PARTY_PAGE)?;
        (origin, Invited::Party, code)
    };
    let host = origin
        .strip_prefix("https://")
        .map(|host| format!("wss://{host}/"))
        .or_else(|| {
            origin
                .strip_prefix("http://")
                .map(|host| format!("ws://{host}/"))
        })?;

    Some(Invitation {
        to,
        code: given_code(code)?,
        server: Some(host),
    })
}

fn stripped_ignoring_case<'a>(text: &'a str, start: &str) -> Option<&'a str> {
    let (head, rest) = text.split_at_checked(start.len())?;

    head.eq_ignore_ascii_case(start).then_some(rest)
}

fn given_code(code: &str) -> Option<String> {
    let code = code.trim_end_matches('/').trim();

    (!code.is_empty()).then(|| code.to_owned())
}

fn server_in(query: &str) -> Option<String> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(SERVER_QUERY))
        .and_then(uri_unescaped)
        .and_then(|server| String::from_utf8(server).ok())
        .filter(|server| !server.trim().is_empty())
}

pub fn link_to(to: Invited, code: &str, server: Option<&str>) -> String {
    let host = match to {
        Invited::Playlist => SHARED_HOST,
        Invited::Party => PARTY_HOST,
    };

    match server {
        Some(server) => format!(
            "{PLAIN_LINK}{host}{code}?{SERVER_QUERY}{}",
            percent_encoded(server)
        ),
        None => format!("{PLAIN_LINK}{host}{code}"),
    }
}

pub fn web_link_to(to: Invited, code: &str, server: &str) -> Option<String> {
    let host = server
        .trim()
        .strip_prefix("wss://")
        .map(|host| format!("https://{host}"))
        .or_else(|| {
            server
                .trim()
                .strip_prefix("ws://")
                .map(|host| format!("http://{host}"))
        })?;
    let page = match to {
        Invited::Playlist => SHARED_PAGE,
        Invited::Party => PARTY_PAGE,
    };

    Some(format!("{}{page}{code}", host.trim_end_matches('/')))
}

fn percent_encoded(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }

    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODE: &str = "E-ABCDE-12345";
    const OWN_SERVER: &str = "wss://music.example:8443/";

    #[test]
    fn a_link_names_what_it_invites_to_and_reads_back_with_its_server() {
        let link = link_to(Invited::Playlist, CODE, Some(OWN_SERVER));

        assert_eq!(
            invitation_in(&link),
            Some(Invitation {
                to: Invited::Playlist,
                code: CODE.to_owned(),
                server: Some(OWN_SERVER.to_owned()),
            })
        );
        assert_eq!(
            invitation_in(&link_to(Invited::Party, CODE, None)),
            Some(Invitation {
                to: Invited::Party,
                code: CODE.to_owned(),
                server: None,
            })
        );
    }

    #[test]
    fn a_web_link_reads_as_the_invitation_it_holds_and_names_its_own_server() {
        let link = web_link_to(Invited::Party, CODE, OWN_SERVER).expect("a web server");

        assert_eq!(link, format!("https://music.example:8443/p/#{CODE}"));
        assert_eq!(
            invitation_in(&link),
            Some(Invitation {
                to: Invited::Party,
                code: CODE.to_owned(),
                server: Some(OWN_SERVER.to_owned()),
            })
        );
    }

    #[test]
    fn a_server_no_browser_can_reach_has_no_web_link() {
        assert_eq!(web_link_to(Invited::Party, CODE, "music.example:443"), None);
    }

    #[test]
    fn a_link_names_another_server_only_where_it_is_not_the_one_given() {
        let named = |server: Option<&str>| Invitation {
            to: Invited::Playlist,
            code: CODE.to_owned(),
            server: server.map(str::to_owned),
        };

        assert_eq!(named(None).naming_a_server_other_than(OWN_SERVER), None);
        assert_eq!(
            named(Some(OWN_SERVER)).naming_a_server_other_than("WSS://music.example:8443"),
            None
        );
        assert_eq!(
            named(Some("wss://stranger.example/")).naming_a_server_other_than(OWN_SERVER),
            Some("wss://stranger.example/")
        );
    }

    #[test]
    fn anything_else_is_no_invitation() {
        assert_eq!(invitation_in("resonate://pair/ABCDE"), None);
        assert_eq!(invitation_in("resonate://shared/"), None);
        assert_eq!(invitation_in("resonate:track/12"), None);
        assert_eq!(invitation_in("https://example.org/other/#x"), None);
        assert_eq!(invitation_in(""), None);
    }

    #[test]
    fn a_link_is_read_whatever_case_its_scheme_is_in_and_with_a_trailing_slash() {
        assert_eq!(
            invitation_in(&format!("RESONATE://SHARED/{CODE}/")).map(|link| link.code),
            Some(CODE.to_owned())
        );
    }
}
