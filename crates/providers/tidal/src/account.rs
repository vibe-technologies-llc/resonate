use std::fmt;

const AUTH: &str = "https://auth.tidal.com/v1/oauth2/token";
const API: &str = "https://api.tidal.com/v1/";
const OPENAPI: &str = "https://openapi.tidal.com/v2/";
const MEDIA_SCHEME: &str = "https";
const MEDIA_DOMAIN: &str = "audio.tidal.com";

#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: String,
}

struct Withheld;

impl fmt::Debug for Withheld {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<withheld>")
    }
}

impl fmt::Debug for Account {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Account")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| Withheld),
            )
            .field("refresh_token", &Withheld)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub auth: String,
    pub api: String,
    pub openapi: String,
    pub media: MediaHosts,
}

impl Endpoints {
    pub fn tidal() -> Self {
        Self {
            auth: AUTH.to_owned(),
            api: API.to_owned(),
            openapi: OPENAPI.to_owned(),
            media: MediaHosts {
                scheme: MEDIA_SCHEME.to_owned(),
                domain: MEDIA_DOMAIN.to_owned(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaHosts {
    pub scheme: String,
    pub domain: String,
}

impl MediaHosts {
    pub fn holds(&self, url: &str) -> bool {
        let Some((scheme, rest)) = url.split_once("://") else {
            return false;
        };
        if !scheme.eq_ignore_ascii_case(&self.scheme) {
            return false;
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        if authority.contains(['@', '[', '\\']) {
            return false;
        }
        let host = authority
            .rsplit_once(':')
            .map_or(authority, |(host, _)| host)
            .to_ascii_lowercase();
        let domain = self.domain.to_ascii_lowercase();
        host == domain
            || host
                .strip_suffix(&domain)
                .is_some_and(|below| below.ends_with('.') && below.len() > 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_is_taken_only_from_the_audio_hosts_over_https() {
        let hosts = Endpoints::tidal().media;

        for held in [
            "https://sp-ad-fa.audio.tidal.com/mediatracks/1/0.mp4?token=a",
            "https://sp-ad-cf.audio.tidal.com/mediatracks/1/1.mp4",
            "https://lgf.audio.tidal.com/mediatracks/1.flac",
            "HTTPS://SP-AD-FA.AUDIO.TIDAL.COM/x",
            "https://audio.tidal.com:443/x",
        ] {
            assert!(hosts.holds(held), "{held} was refused");
        }

        for refused in [
            "http://sp-ad-fa.audio.tidal.com/x",
            "https://evil.example/x",
            "https://audio.tidal.com.evil.example/x",
            "https://notaudio.tidal.com/x",
            "https://xaudio.tidal.com/x",
            "https://.audio.tidal.com/x",
            "https://sp-ad-fa.audio.tidal.com@evil.example/x",
            "https://[::1]/x",
            "sp-ad-fa.audio.tidal.com/x",
        ] {
            assert!(!hosts.holds(refused), "{refused} was taken");
        }
    }

    #[test]
    fn an_account_never_prints_its_secrets() {
        let account = Account {
            client_id: "client".to_owned(),
            client_secret: Some("hush".to_owned()),
            refresh_token: "sesame".to_owned(),
        };
        let printed = format!("{account:?}");

        assert!(printed.contains("client"));
        assert!(!printed.contains("hush"));
        assert!(!printed.contains("sesame"));
    }
}
