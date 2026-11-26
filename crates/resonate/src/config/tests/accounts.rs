use super::*;

#[test]
fn a_cookie_setting_is_withheld_from_the_configs_debug() {
    let config =
        read("youtube-music-cookies = \"firefox:/home/a/zen\"\nyt-dlp = \"/opt/yt-dlp\"\n")
            .expect("a well formed document");

    let printed = format!("{config:?}");

    assert!(printed.contains("<withheld>"));
    assert!(printed.contains("/opt/yt-dlp"));
    assert!(!printed.contains("/home/a/zen"));
}

#[test]
fn the_configs_debug_names_each_field_once() {
    let config = read("").expect("a well formed document");

    let printed = format!("{config:#?}");
    let named: Vec<&str> = printed
        .lines()
        .filter_map(|line| line.strip_prefix("    "))
        .filter(|line| !line.starts_with(' '))
        .filter_map(|line| line.split_once(": ").map(|(name, _)| name))
        .collect();
    let distinct: std::collections::BTreeSet<&str> = named.iter().copied().collect();

    assert!(named.len() > 50);
    assert_eq!(named.len(), distinct.len(), "{named:?}");
}

#[test]
fn no_key_holding_a_secret_is_printed_by_debug() {
    let config = read(
        "subsonic = \"https://music.home.arpa\"\nsubsonic-user = \"listener\"\n\
         subsonic-password = \"subsonic-sesame\"\njellyfin = \"https://jelly.home.arpa\"\n\
         jellyfin-user = \"listener\"\njellyfin-password = \"jellyfin-sesame\"\ntidal-client-id = \"client\"\n\
         tidal-client-secret = \"tidal-hush\"\ntidal-refresh-token = \"tidal-sesame\"\n\
         spotify-refresh-token = \"spotify-sesame\"\n\
         audd-token = \"audd-sesame\"\nlistenbrainz-token = \"listenbrainz-sesame\"\n\
         acoustid-key = \"acoustid-sesame\"\n\
         sync-key = \"sync-sesame\"\nmusicbrainz-replication-token = \"metabrainz-sesame\"\n",
    )
    .expect("a well formed document");

    let printed = format!("{config:?}");

    assert!(printed.contains("music.home.arpa"));
    assert!(printed.contains("listener"));
    assert!(printed.contains("client"));
    assert!(printed.contains("<withheld>"));
    for secret in [
        "subsonic-sesame",
        "jellyfin-sesame",
        "tidal-hush",
        "tidal-sesame",
        "spotify-sesame",
        "audd-sesame",
        "listenbrainz-sesame",
        "acoustid-sesame",
        "metabrainz-sesame",
        "sync-sesame",
    ] {
        assert!(!printed.contains(secret), "{secret} was printed");
    }
}

#[test]
#[cfg(feature = "online")]
fn a_listenbrainz_token_is_submitted_to_only_while_online_is_on() {
    let config = read("listenbrainz-token = \" a token \"").expect("a well formed document");

    assert_eq!(config.listenbrainz_token.as_deref(), Some("a token"));
    assert_eq!(config.submits_to().listenbrainz.as_deref(), Some("a token"));

    let off = read("online = false\nlistenbrainz-token = \"a token\"").expect("well formed");

    assert_eq!(off.submits_to(), Accounts::default());

    let blank = read("listenbrainz-token = \"  \"").expect("well formed");

    assert_eq!(blank.listenbrainz_token, None);
    assert_eq!(Config::default().submits_to(), Accounts::default());
}

#[test]
#[cfg(feature = "online")]
fn a_lastfm_account_is_submitted_to_only_where_its_key_secret_and_session_are_all_given() {
    let whole =
        read("lastfm-key = \"key\"\nlastfm-secret = \"secret\"\nlastfm-session = \"session\"")
            .expect("a well formed document");
    let unsigned = read("lastfm-key = \"key\"\nlastfm-secret = \"secret\"").expect("well formed");

    assert_eq!(
        whole.submits_to().lastfm,
        Some(LastfmAccount {
            key: "key".to_owned(),
            secret: "secret".to_owned(),
            session: "session".to_owned(),
        })
    );
    assert_eq!(unsigned.submits_to().lastfm, None);
    assert!(!format!("{whole:?}").contains("secret\""));
    assert!(!format!("{:?}", whole.submits_to()).contains("session"));
}

#[test]
fn an_audd_token_is_read_and_a_blank_one_is_none() {
    let config = read("audd-token = \"a token\"").expect("a well formed document");

    assert_eq!(config.audd_token.as_deref(), Some("a token"));
    assert_eq!(
        read("audd-token = \"\"").expect("well formed").audd_token,
        None
    );
    assert_eq!(Config::default().audd_token, None);
}

#[test]
fn an_acoustid_key_is_read_and_a_blank_one_is_none() {
    let config = read("acoustid-key = \"a key the user typed\"").expect("a well formed document");

    assert_eq!(config.acoustid_key.as_deref(), Some("a key the user typed"));

    let config = read("acoustid-key = \"  \"").expect("a well formed document");

    assert_eq!(config.acoustid_key, None);

    assert_eq!(Config::default().acoustid_key, None);
}

#[cfg(feature = "online")]
#[test]
fn a_spotify_account_is_read_out_of_the_file_only_where_both_keys_are_given() {
    let scratch = Scratch::new().seed("spotify-client-id = \"registered\"\n");

    assert_eq!(spotify_in(&scratch.path).expect("it reads"), None);

    let scratch =
        scratch.seed("spotify-client-id = \" registered \"\nspotify-refresh-token = \"sesame\"\n");

    assert_eq!(
        spotify_in(&scratch.path).expect("it reads"),
        Some(("registered".to_owned(), "sesame".to_owned()))
    );

    let _ = fs::remove_file(&scratch.path);
}

#[test]
fn online_and_metered_switches_are_read() {
    let config = read("online = false").expect("a well formed document");

    assert_eq!(config.switches.said(Switch::Online), Some(false));
    assert!(!config.is_on(Switch::Online));

    let config = read("online = true").expect("a well formed document");

    assert_eq!(config.switches.said(Switch::Online), Some(true));
    assert!(config.is_on(Switch::Online));

    let config = read("").expect("empty is valid");

    assert!(config.is_on(Switch::Online));
    assert!(config.is_on(Switch::EnrichAfterScan));

    let config = read("enrich-after-scan = false").expect("a well formed document");

    assert_eq!(config.switches.said(Switch::EnrichAfterScan), Some(false));
    assert!(!config.is_on(Switch::EnrichAfterScan));

    assert!(!config.is_on(Switch::EnrichOnMetered));
    assert!(config.is_on(Switch::DownloadOnMetered));

    let config = read("enrich-on-metered = true\ndownload-on-metered = false")
        .expect("a well formed document");

    assert!(config.is_on(Switch::EnrichOnMetered));
    assert!(!config.is_on(Switch::DownloadOnMetered));

    let error = read("online = \"yes\"").expect_err("a string is not a boolean");

    assert!(
        matches!(
            error,
            Error::ConfigType {
                key: ConfigKey::Online,
                expected: ValueKind::Boolean,
                ..
            }
        ),
        "{error:?}"
    );
}
