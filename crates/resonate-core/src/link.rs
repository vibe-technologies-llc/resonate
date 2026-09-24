const SCHEME_END: &str = "://";
const BARE_WWW: &str = "www.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Relation {
    Streaming,
    FreeStreaming,
    PurchaseForDownload,
    PurchaseForMailOrder,
    Homepage,
    Wikipedia,
    Wikidata,
    Discogs,
    AllMusic,
    Image,
    SocialNetwork,
    Lyrics,
    Youtube,
    YoutubeMusic,
    Soundcloud,
    Bandcamp,
    LastFm,
    Other,
}

impl Relation {
    const TYPES: [(&'static str, Self); 17] = [
        ("streaming", Self::Streaming),
        ("free streaming", Self::FreeStreaming),
        ("purchase for download", Self::PurchaseForDownload),
        ("purchase for mail-order", Self::PurchaseForMailOrder),
        ("official homepage", Self::Homepage),
        ("wikipedia", Self::Wikipedia),
        ("wikidata", Self::Wikidata),
        ("discogs", Self::Discogs),
        ("allmusic", Self::AllMusic),
        ("image", Self::Image),
        ("social network", Self::SocialNetwork),
        ("lyrics", Self::Lyrics),
        ("youtube", Self::Youtube),
        ("youtube music", Self::YoutubeMusic),
        ("soundcloud", Self::Soundcloud),
        ("bandcamp", Self::Bandcamp),
        ("last.fm", Self::LastFm),
    ];

    pub fn of_type(kind: &str) -> Self {
        Self::TYPES
            .into_iter()
            .find(|(named, _)| *named == kind)
            .map_or(Self::Other, |(_, relation)| relation)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    Spotify,
    Tidal,
    AppleMusic,
    Deezer,
    Qobuz,
    AmazonMusic,
    Youtube,
    YoutubeMusic,
    Bandcamp,
    Soundcloud,
    Beatport,
    SevenDigital,
    Discogs,
    Wikipedia,
    Wikidata,
    AllMusic,
    LastFm,
    WikimediaCommons,
    Facebook,
    Instagram,
    Twitter,
    Other,
}

impl Service {
    const HOSTS: [(&'static str, Self); 21] = [
        ("spotify.com", Self::Spotify),
        ("tidal.com", Self::Tidal),
        ("music.apple.com", Self::AppleMusic),
        ("itunes.apple.com", Self::AppleMusic),
        ("deezer.com", Self::Deezer),
        ("qobuz.com", Self::Qobuz),
        ("music.youtube.com", Self::YoutubeMusic),
        ("youtube.com", Self::Youtube),
        ("youtu.be", Self::Youtube),
        ("bandcamp.com", Self::Bandcamp),
        ("soundcloud.com", Self::Soundcloud),
        ("beatport.com", Self::Beatport),
        ("7digital.com", Self::SevenDigital),
        ("discogs.com", Self::Discogs),
        ("wikipedia.org", Self::Wikipedia),
        ("wikidata.org", Self::Wikidata),
        ("allmusic.com", Self::AllMusic),
        ("last.fm", Self::LastFm),
        ("commons.wikimedia.org", Self::WikimediaCommons),
        ("facebook.com", Self::Facebook),
        ("instagram.com", Self::Instagram),
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::Tidal => "tidal",
            Self::AppleMusic => "apple music",
            Self::Deezer => "deezer",
            Self::Qobuz => "qobuz",
            Self::AmazonMusic => "amazon music",
            Self::Youtube => "youtube",
            Self::YoutubeMusic => "youtube music",
            Self::Bandcamp => "bandcamp",
            Self::Soundcloud => "soundcloud",
            Self::Beatport => "beatport",
            Self::SevenDigital => "7digital",
            Self::Discogs => "discogs",
            Self::Wikipedia => "wikipedia",
            Self::Wikidata => "wikidata",
            Self::AllMusic => "allmusic",
            Self::LastFm => "last.fm",
            Self::WikimediaCommons => "wikimedia commons",
            Self::Facebook => "facebook",
            Self::Instagram => "instagram",
            Self::Twitter => "twitter",
            Self::Other => "other",
        }
    }

    pub const fn title(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::Tidal => "Tidal",
            Self::AppleMusic => "Apple Music",
            Self::Deezer => "Deezer",
            Self::Qobuz => "Qobuz",
            Self::AmazonMusic => "Amazon Music",
            Self::Youtube => "YouTube",
            Self::YoutubeMusic => "YouTube Music",
            Self::Bandcamp => "Bandcamp",
            Self::Soundcloud => "SoundCloud",
            Self::Beatport => "Beatport",
            Self::SevenDigital => "7digital",
            Self::Discogs => "Discogs",
            Self::Wikipedia => "Wikipedia",
            Self::Wikidata => "Wikidata",
            Self::AllMusic => "AllMusic",
            Self::LastFm => "Last.fm",
            Self::WikimediaCommons => "Wikimedia Commons",
            Self::Facebook => "Facebook",
            Self::Instagram => "Instagram",
            Self::Twitter => "X",
            Self::Other => "elsewhere",
        }
    }

    const AMAZON: &'static str = "amazon";

    const TWITTER: [&'static str; 2] = ["twitter.com", "x.com"];

    pub fn of_url(url: &str) -> Self {
        let Some(host) = host_of(url) else {
            return Self::Other;
        };
        if let Some((_, service)) = Self::HOSTS.iter().find(|(named, _)| under(&host, named)) {
            return *service;
        }
        if Self::TWITTER.iter().any(|named| under(&host, named)) {
            return Self::Twitter;
        }
        if host.split('.').any(|label| label == Self::AMAZON) {
            return Self::AmazonMusic;
        }

        Self::Other
    }
}

fn host_of(url: &str) -> Option<String> {
    let (_, after_scheme) = url.split_once(SCHEME_END)?;
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.split(':').next().unwrap_or_default();
    if host.is_empty() {
        return None;
    }

    let lowered = host.to_ascii_lowercase();
    Some(
        lowered
            .strip_prefix(BARE_WWW)
            .map_or(lowered.as_str(), |bare| bare)
            .to_owned(),
    )
}

fn under(host: &str, named: &str) -> bool {
    host.strip_suffix(named)
        .is_some_and(|before| before.is_empty() || before.ends_with('.'))
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Link {
    pub relation: Relation,
    pub service: Service,
    pub url: String,
}

impl Link {
    pub fn new(relation_type: &str, url: String) -> Self {
        Self {
            relation: Relation::of_type(relation_type),
            service: Service::of_url(&url),
            url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_service_is_read_off_the_host_of_its_url() {
        for (url, service) in [
            (
                "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV",
                Service::Spotify,
            ),
            ("https://tidal.com/track/55391743", Service::Tidal),
            ("https://listen.tidal.com/album/55391740", Service::Tidal),
            (
                "https://music.apple.com/gb/song/1065973615",
                Service::AppleMusic,
            ),
            ("https://www.deezer.com/track/116913984", Service::Deezer),
            ("https://open.qobuz.com/artist/38324", Service::Qobuz),
            (
                "https://music.amazon.co.uk/albums/B0BX7Z1",
                Service::AmazonMusic,
            ),
            ("https://www.amazon.com/dp/B000002UAK", Service::AmazonMusic),
            (
                "https://www.youtube.com/watch?v=IluRBvnYMoY",
                Service::Youtube,
            ),
            ("https://youtu.be/IluRBvnYMoY", Service::Youtube),
            (
                "https://music.youtube.com/channel/UCY2qt3dw2TQJxvBrXwstEPQ",
                Service::YoutubeMusic,
            ),
            (
                "https://commons.wikimedia.org/wiki/File:PinkFloyd1973_retouched.jpg",
                Service::WikimediaCommons,
            ),
            (
                "https://en.wikipedia.org/wiki/Pink_Floyd",
                Service::Wikipedia,
            ),
            ("https://www.wikidata.org/wiki/Q2306", Service::Wikidata),
            ("https://pinkfloyd.bandcamp.com/", Service::Bandcamp),
            ("https://soundcloud.com/pinkfloyd", Service::Soundcloud),
            ("https://www.discogs.com/artist/45467", Service::Discogs),
            (
                "https://www.allmusic.com/artist/mn0000346336",
                Service::AllMusic,
            ),
            ("https://www.last.fm/music/Pink+Floyd", Service::LastFm),
            ("https://www.facebook.com/pinkfloyd", Service::Facebook),
            ("https://www.instagram.com/pinkfloyd/", Service::Instagram),
            ("https://twitter.com/pinkfloyd", Service::Twitter),
            ("https://x.com/pinkfloyd", Service::Twitter),
            (
                "HTTPS://WWW.7DIGITAL.COM/artist/pink-floyd",
                Service::SevenDigital,
            ),
            ("https://mora.jp/artist/579953/", Service::Other),
            ("https://notspotify.com/track/1", Service::Other),
            ("not a url at all", Service::Other),
        ] {
            assert_eq!(Service::of_url(url), service, "{url}");
        }
    }

    #[test]
    fn a_service_is_named_in_lowercase_and_every_name_is_its_own() {
        let named: Vec<&str> = Service::HOSTS
            .iter()
            .map(|(_, service)| service.name())
            .chain([
                Service::Twitter.name(),
                Service::AmazonMusic.name(),
                Service::Other.name(),
            ])
            .collect();

        for name in &named {
            assert_eq!(*name, name.to_lowercase(), "{name}");
            assert!(!name.trim().is_empty());
        }
        assert_eq!(Service::AppleMusic.name(), "apple music");
        assert_eq!(Service::SevenDigital.name(), "7digital");
        assert_eq!(Service::WikimediaCommons.name(), "wikimedia commons");

        let mut distinct = named.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 22);
    }

    #[test]
    fn a_service_is_titled_as_it_spells_itself_and_every_title_is_its_own() {
        let titled: Vec<&str> = Service::HOSTS
            .iter()
            .map(|(_, service)| service.title())
            .chain([
                Service::Twitter.title(),
                Service::AmazonMusic.title(),
                Service::Other.title(),
            ])
            .collect();

        for title in &titled {
            assert_eq!(title.to_lowercase(), title.to_lowercase().trim());
            assert!(!title.trim().is_empty());
        }
        assert_eq!(Service::AppleMusic.title(), "Apple Music");
        assert_eq!(Service::Soundcloud.title(), "SoundCloud");

        let mut distinct = titled.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(distinct.len(), 22);
    }

    #[test]
    fn a_relation_is_read_off_the_exact_type_musicbrainz_writes() {
        for (kind, relation) in [
            ("streaming", Relation::Streaming),
            ("free streaming", Relation::FreeStreaming),
            ("purchase for download", Relation::PurchaseForDownload),
            ("purchase for mail-order", Relation::PurchaseForMailOrder),
            ("official homepage", Relation::Homepage),
            ("wikipedia", Relation::Wikipedia),
            ("wikidata", Relation::Wikidata),
            ("discogs", Relation::Discogs),
            ("allmusic", Relation::AllMusic),
            ("image", Relation::Image),
            ("social network", Relation::SocialNetwork),
            ("lyrics", Relation::Lyrics),
            ("youtube", Relation::Youtube),
            ("youtube music", Relation::YoutubeMusic),
            ("soundcloud", Relation::Soundcloud),
            ("bandcamp", Relation::Bandcamp),
            ("last.fm", Relation::LastFm),
            ("Streaming", Relation::Other),
            ("get the music", Relation::Other),
        ] {
            assert_eq!(Relation::of_type(kind), relation, "{kind}");
        }
    }
}
