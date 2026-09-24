use std::{
    env,
    sync::{Arc, OnceLock},
    time::Duration,
};

use resonate_core::{MediaLocation, SampleRate};
use resonate_eq::{Corrections, DeviceId, suggest};
use resonate_library::{
    Error, LookupOp, Mbid, Reference, ReleaseAsked, Scrobble, Scrobbler, Wording,
};
use resonate_listen::{Clip, Recogniser};
use resonate_lyrics::{LyricProvider, Timing, Wanted};
use resonate_online::{AutoEq, Client, Identity, ListenBrainz, Lrclib, Online, Shazam};

const GATE: &str = "RESONATE_ONLINE_TESTS";
const MEDDLE: &str = "aadf62d6-d475-42e0-b622-e6da7a59fdf7";
const PINK_FLOYD: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const ECHOES_LASTS: Duration = Duration::from_secs(1412);

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
fn the_reference_answers_pink_floyd_with_every_album_and_ep_it_is_credited_on() {
    let Some(client) = reached() else {
        return;
    };
    let online = Online::with_client(client);

    let groups = online
        .release_groups_of(&mbid(PINK_FLOYD))
        .expect("musicbrainz answered");
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
        title: "Echoes".to_owned(),
        artist: "Pink Floyd".to_owned(),
        album: Some("Meddle".to_owned()),
        recording: None,
        release: Some(mbid(MEDDLE)),
        artist_mbid: Some(mbid(PINK_FLOYD)),
        number: Some(6),
        length: Some(ECHOES_LASTS),
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
