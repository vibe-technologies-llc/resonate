use std::{
    env,
    sync::{Arc, OnceLock},
    time::Duration,
};

use resonate_core::{MediaLocation, SampleRate};
use resonate_eq::{Corrections, DeviceId, suggest};
use resonate_library::{
    AlbumLink, Billed, Error, Isrc, Link, LookupOp, Mbid, Reference, Relation, ReleaseAsked,
    Scrobble, Scrobbler, Service, SongLink, StreamAsked, TokenHeld, Wording, songs_asked,
    weighed_for,
};
use resonate_listen::{Clip, Recogniser};
use resonate_lyrics::{LyricProvider, Timing, Wanted};
use resonate_online::{AutoEq, Client, Identity, ListenBrainz, Lrclib, Online, Shazam};

const GATE: &str = "RESONATE_ONLINE_TESTS";
const MEDDLE: &str = "aadf62d6-d475-42e0-b622-e6da7a59fdf7";
const PINK_FLOYD: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const ECHOES_LASTS: Duration = Duration::from_secs(1412);
const ONE_OF_THESE_DAYS: &str = "8a313f74-75fe-40ff-a1a9-0b000216c555";
const ECHOES_ISRC: &str = "GBN9Y1100065";

static CLIENT: OnceLock<Arc<Client>> = OnceLock::new();

fn reached() -> Option<Arc<Client>> {
    if env::var_os(GATE).is_none() {
        eprintln!("skipped: set {GATE} to reach the services");
        return None;
    }

    Some(
        CLIENT
            .get_or_init(|| Arc::new(Client::new(Identity::of_this_build())))
            .clone(),
    )
}

fn mbid(text: &str) -> Mbid {
    Mbid::new(text).expect("a well-formed mbid")
}

#[test]
fn deezer_names_where_a_track_streams_by_its_isrc_and_by_its_names() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let by_code = online
        .streamed_at(&StreamAsked {
            title: "Easier to Run".to_owned(),
            artist: Some("Linkin Park".to_owned()),
            isrc: Some(Isrc::new("USWB10301869").expect("a well-formed isrc")),
            length: None,
        })
        .expect("deezer answered")
        .expect("deezer holds the track under its isrc");
    assert_eq!(by_code.service, Service::Deezer);
    assert_eq!(by_code.url, "https://www.deezer.com/track/677241");

    let by_name = online
        .streamed_at(&StreamAsked {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            isrc: None,
            length: Some(ECHOES_LASTS),
        })
        .expect("deezer answered")
        .expect("deezer holds echoes");
    assert!(by_name.url.starts_with("https://www.deezer.com/track/"));
}

#[test]
fn a_title_by_an_artist_is_found_however_the_title_is_spelt() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    for typed in [
        "You F O by stela cole",
        "you fo by stela cole",
        "You F.O. by Stela Cole",
        "Stela Cole - You F O",
    ] {
        let asked = songs_asked(typed).expect("words worth asking");
        let found = weighed_for(
            &asked,
            online.find_songs(&asked).expect("musicbrainz answered"),
        );
        let first = found
            .first()
            .unwrap_or_else(|| panic!("{typed} found nothing"));

        assert_eq!(first.title, "You F O", "{typed}");
        assert_eq!(first.credited_as(), "Stela Cole", "{typed}");
    }
}

#[test]
fn a_link_to_a_song_on_spotify_tidal_and_apple_music_names_its_isrc() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);
    let never_gonna = Isrc::new("GBARL9300135").expect("a well-formed isrc");

    for pasted in [
        "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT",
        "https://tidal.com/browse/track/491206012",
        "https://music.apple.com/us/album/x/1559523357?i=1559523359",
        "https://www.deezer.com/track/781592622",
    ] {
        let link = SongLink::read(pasted).expect("a song link");
        let named = online
            .song_linked(&link)
            .expect("the services answered")
            .unwrap_or_else(|| panic!("{pasted} named no song"));

        assert!(named.isrcs.contains(&never_gonna), "{pasted}: {named:?}");
    }
}

#[test]
fn a_link_to_an_album_names_a_barcode_musicbrainz_files_the_release_group_under() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);
    let whenever = mbid("082c6aff-a7cc-36e0-a960-35a578ecd937");

    let link = AlbumLink::read("https://open.spotify.com/album/6N9PS4QXF1D0OWPk0Sxtb4")
        .expect("an album link");
    let named = online
        .album_linked(&link)
        .expect("the services answered")
        .expect("the link named an album");
    let mut groups = Vec::new();
    for barcode in &named.barcodes {
        groups.extend(
            online
                .releases_by_barcode(barcode)
                .expect("musicbrainz answered")
                .into_iter()
                .filter(|found| {
                    found
                        .barcode
                        .as_deref()
                        .is_some_and(|held| barcode.names(held))
                })
                .filter_map(|found| found.group),
        );
    }

    assert!(groups.contains(&whenever), "{named:?} named {groups:?}");
}

#[test]
fn a_recording_says_what_kind_of_release_each_of_its_releases_is_and_an_isrc_is_answered() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let recording = online
        .recording(&mbid(ONE_OF_THESE_DAYS))
        .expect("musicbrainz answered")
        .expect("the recording is one it holds");
    assert!(
        recording
            .releases
            .iter()
            .any(|release| release.issued.kind.as_deref() == Some("Album")
                && release.issued.status.as_deref() == Some("Official")),
        "no release of the recording was read as an official album"
    );

    let takes = online
        .recordings_of_isrc(&Isrc::new(ECHOES_ISRC).expect("a well-formed isrc"))
        .expect("musicbrainz answered the isrc");
    assert!(!takes.is_empty());
}

#[test]
fn the_reference_answers_meddle_with_its_six_tracks_and_a_cover() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let release = online
        .release(&mbid(MEDDLE))
        .expect("musicbrainz answered")
        .expect("meddle is a release it holds");
    assert_eq!(release.title, "Meddle");
    assert_eq!(release.credited_as(), "Pink Floyd");
    assert_eq!(release.track_count(), 6);
    assert!(release.group.is_some());
    assert!(release.has_front_cover);
    assert!(
        release.media[0]
            .tracks
            .iter()
            .all(|track| track.recording.is_some() && track.length.is_some())
    );
    assert!(
        release.media[0]
            .tracks
            .iter()
            .any(|track| !track.links.is_empty())
    );

    let cover = online
        .cover(&release.id, release.group.as_ref())
        .expect("the archive answered")
        .expect("meddle has a front cover");
    assert!(cover.bytes.len() > 1024);

    let found = online
        .find_release(&ReleaseAsked {
            title: "Meddle".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_mbid: None,
            barcode: None,
            catalog_number: None,
            wording: Wording::Phrase,
        })
        .expect("musicbrainz answered");
    assert!(!found.is_empty());
    assert!(found.iter().all(|found| found.title == "Meddle"));
}

#[test]
fn the_reference_answers_pink_floyd_with_a_profile_and_a_portrait() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let profile = online
        .artist(&mbid(PINK_FLOYD))
        .expect("musicbrainz answered")
        .expect("pink floyd is an artist it holds");
    assert_eq!(profile.name, "Pink Floyd");
    assert_eq!(profile.kind.as_deref(), Some("Group"));
    assert_eq!(profile.span.begin.as_deref(), Some("1965"));
    assert!(!profile.genres.is_empty());
    assert!(profile.may_be_pictured());

    let portrait = online
        .portrait(&profile.links)
        .expect("commons answered")
        .expect("the profile names a portrait");
    assert!(portrait.bytes.len() > 1024);

    let found = online
        .find_artist("Pink Floyd")
        .expect("musicbrainz answered");
    assert!(found.iter().any(|found| found.mbid.as_str() == PINK_FLOYD));
}

#[test]
fn an_artist_linked_only_to_spotify_or_soundcloud_is_pictured_by_the_page_it_is_linked_to() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    for url in [
        "https://open.spotify.com/artist/47RTV4mRN9dDNbqxB1IMXF",
        "https://soundcloud.com/wierza",
    ] {
        let links = [Link {
            relation: Relation::Other,
            service: Service::of_url(url),
            url: url.to_owned(),
        }];
        let portrait = online
            .portrait(&links)
            .expect("the page answered")
            .unwrap_or_else(|| panic!("{url} named no picture of the artist"));
        assert!(portrait.bytes.len() > 1024);
    }
}

#[test]
fn the_reference_answers_pink_floyd_with_every_album_and_ep_it_is_credited_on() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let groups = online
        .release_groups_of(&mbid(PINK_FLOYD), 0)
        .expect("musicbrainz answered")
        .releases;
    assert!(
        groups.len() >= 100,
        "the browse answered only {} groups",
        groups.len()
    );
    assert!(groups.iter().any(|group| group.title == "Meddle"));
}

#[test]
fn lrclib_answers_echoes_with_a_synced_set() {
    let Some(client) = reached() else {
        return;
    };
    let provider = Lrclib::new(client, None);

    let wanted = Wanted {
        title: Some("Echoes".to_owned()),
        artist: Some("Pink Floyd".to_owned()),
        album: Some("Meddle".to_owned()),
        duration: Some(ECHOES_LASTS),
        ..Wanted::for_media(MediaLocation::local(
            "/music/Pink Floyd/Meddle/06 Echoes.flac",
        ))
    };
    let lyrics = provider
        .lyrics(&wanted)
        .expect("lrclib answered")
        .expect("lrclib holds echoes");

    assert_eq!(lyrics.timing(), Timing::Synced);
    assert!(
        lyrics
            .lines()
            .iter()
            .any(|line| line.text.contains("albatross"))
    );
}

#[test]
fn autoeq_answers_with_its_whole_index_and_one_measured_correction() {
    let Some(client) = reached() else {
        return;
    };
    let source = AutoEq::new(client, None);

    let catalogue = source.catalogue().expect("autoeq answered");
    assert!(
        catalogue.len() > 8_000,
        "the index named only {} devices",
        catalogue.len()
    );

    for device in catalogue.devices() {
        let tail = device.id.as_str().rsplit('/').next().unwrap_or_default();
        assert_eq!(tail, device.label, "a row names two devices");
    }

    let named = |description: &str| {
        suggest(&catalogue, description)
            .and_then(|found| catalogue.device(found))
            .map(|device| device.label.clone())
    };
    assert_eq!(
        named("Sony WH-1000XM4 Analog Stereo").as_deref(),
        Some("Sony WH-1000XM4")
    );
    assert_eq!(
        named("USB-C to 3.5mm Headphone Jack Adapter Analog Stereo"),
        None
    );

    let hd650 = DeviceId::new("oratory1990/over-ear/Sennheiser HD 650").expect("a path");
    let profile = source
        .profile(&hd650)
        .expect("autoeq answered")
        .expect("autoeq measured it");

    assert!(profile.bands().len() >= 5);
    assert!(profile.preamp().decibels() < 0.0);
}

#[test]
fn shazam_answers_a_clip_it_does_not_know_with_nothing_rather_than_a_refusal() {
    let Some(client) = reached() else {
        return;
    };
    let rate = SampleRate::HZ_48000;
    let notes: Vec<f32> = (0..rate.hz() as usize * 12)
        .map(|n| {
            let at = n as f64 / f64::from(rate.hz()) * 4.0;
            let hertz = [523.0, 1_760.0, 698.0, 2_637.0, 880.0][(at as usize) % 5];
            let sounding = if at.fract() < 0.6 { 0.3 } else { 0.0 };
            (sounding * (std::f64::consts::TAU * hertz * n as f64 / f64::from(rate.hz())).sin())
                as f32
        })
        .collect();
    let clip = Clip {
        rate,
        channels: 1,
        samples: notes,
    };
    let heard = Shazam::new(client)
        .recognise(&clip)
        .expect("the service answers this build's own user agent");
    assert_eq!(heard, None, "a run of synthetic notes was named");
}

#[test]
fn listenbrainz_refuses_a_token_it_never_issued_and_says_so_by_its_status() {
    let Some(client) = reached() else {
        return;
    };
    let listenbrainz = ListenBrainz::new(client, "not-a-token-anyone-was-given".to_owned());

    let refused = listenbrainz.submit(&[Scrobble {
        listen: resonate_core::ListenId::new(1).expect("a listen id is not zero"),
        at: std::time::SystemTime::now(),
        billed: Billed {
            title: "Echoes".to_owned(),
            artist: "Pink Floyd".to_owned(),
            album: Some("Meddle".to_owned()),
            recording: None,
            release: Some(mbid(MEDDLE)),
            release_group: None,
            artist_mbid: Some(mbid(PINK_FLOYD)),
            isrc: Some(Isrc::new(ECHOES_ISRC).expect("a well-formed isrc")),
            number: Some(6),
            length: Some(ECHOES_LASTS),
        },
    }]);

    assert!(
        matches!(
            refused,
            Err(Error::Refused {
                op: LookupOp::Submit,
                status: 401
            })
        ),
        "{refused:?}"
    );
}

#[test]
fn listenbrainz_says_a_token_it_never_issued_is_held_by_nobody() {
    let Some(client) = reached() else {
        return;
    };
    let listenbrainz = ListenBrainz::new(client, "not-a-token-anyone-was-given".to_owned());

    assert_eq!(
        listenbrainz.token_held().expect("the service answered"),
        TokenHeld::Unknown
    );
}
