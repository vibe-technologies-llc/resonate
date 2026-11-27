use std::{sync::Arc, time::Duration};

use gpui::TestAppContext;
use parking_lot::Mutex;
use resonate_core::{Isrc, SourceId};
use resonate_library::{Library, Track, TrackQuery};
use resonate_providers::Providers;

use super::{
    fixtures::{Asked, LANTERN_TONIGHT, LANTERN_TONIGHT_ISRC, MusicBrainz, Shop, mbid, ncs_cover},
    searching::typed,
};
use crate::{
    Supplying,
    downloads::Fetching,
    driven::{Driven, Folder, Reaching},
    models::Selection,
    toast,
};

#[gpui::test]
fn pressing_an_ncs_song_found_on_musicbrainz_wants_it_and_asks_the_providers_for_it(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&asked);
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Shop {
                source: SourceId::new("shop").expect("a source name"),
                asked: Arc::clone(&told),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.set_query("Lantern Tonight".to_owned(), cx);
        });
    });
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());

    driven.click("unheld-title-0");
    driven.until(|_, _| !asked.lock().is_empty());
    driven.until(|root, cx| {
        let library = root.library.read(cx);
        library.downloads().first().is_some_and(|download| {
            matches!(
                library.fetching(download),
                Fetching::Retrying { tries: 1, .. }
            )
        })
    });
    driven.click("downloads");
    driven.bounds_of("download-0");

    assert!(
        driven.read(|_, cx| !toast::is_showing(cx)),
        "the download was told in a toast rather than the sidebar"
    );

    let wants = library.wants().expect("the wants read");

    assert_eq!(wants.len(), 1);
    assert_eq!(
        wants[0].artist.as_deref(),
        Some("Another Band & Third Band")
    );
    assert_eq!(
        library.cover_art(wants[0].album).expect("the cover read"),
        Some(ncs_cover())
    );

    let expected = Asked {
        recording: Some(mbid(LANTERN_TONIGHT)),
        isrc: Some(Isrc::new(LANTERN_TONIGHT_ISRC).expect("an isrc")),
        artist: Some("Another Band & Third Band".to_owned()),
    };
    let heard = asked.lock().clone();

    assert!(
        heard.len() == 2 && heard.iter().all(|asked| *asked == expected),
        "the providers were asked once from what the search found and once by the poll: {heard:?}"
    );

    let recording = mbid(LANTERN_TONIGHT);

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.cancel_download(&recording, cx));
    });
    driven.until(|root, cx| root.library.read(cx).downloads().is_empty());

    assert!(library.wants().expect("the wants read").is_empty());
}

#[gpui::test]
fn a_right_press_on_a_song_found_on_musicbrainz_downloads_it_without_waiting_to_play_it(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&asked);
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Shop {
                source: SourceId::new("shop").expect("a source name"),
                asked: Arc::clone(&told),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
    let recording = mbid(LANTERN_TONIGHT);

    typed(&mut driven, "Lantern Tonight");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());
    driven.right_click("unheld-title-0");

    assert!(
        driven.read(|root, _| root.menu.is_some()),
        "a right press on a found song opened no menu"
    );

    driven.click("menu-entry-2");
    driven.until(|_, _| !asked.lock().is_empty());

    assert!(driven.read(|root, _| root.menu.is_none()));
    assert!(
        driven.read(|root, cx| !root.library.read(cx).is_awaited(&recording)),
        "the song pressed to download was waited on to play"
    );
    assert!(
        driven.read(|root, cx| {
            root.library
                .read(cx)
                .downloads()
                .iter()
                .any(|download| download.found.recording == recording)
        }),
        "the song pressed to download was not listed as a download"
    );
    assert_eq!(library.wants().expect("the wants read").len(), 1);
}

#[gpui::test]
fn a_pressed_song_no_provider_delivers_is_still_found_when_the_search_is_asked_again(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let told = Arc::clone(&asked);
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Shop {
                source: SourceId::new("shop").expect("a source name"),
                asked: Arc::clone(&told),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
    let recording = mbid(LANTERN_TONIGHT);
    let found_now = |root: &crate::views::root::RootView, cx: &gpui::App| {
        root.library
            .read(cx)
            .found()
            .iter()
            .any(|found| found.recording == recording)
    };

    typed(&mut driven, "Lantern Tonight");
    driven.until(|root, cx| found_now(root, cx));
    driven.click("unheld-title-0");
    driven.until(|_, _| !asked.lock().is_empty());

    assert!(
        driven.read(|root, cx| found_now(root, cx)),
        "the pressed song left the search"
    );

    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.ask_elsewhere_again(cx));
    });
    driven.until(|root, cx| !root.library.read(cx).is_asking_elsewhere());

    assert!(
        driven.read(|root, cx| found_now(root, cx)),
        "a song no provider delivered could not be found again"
    );
}

pub(super) const LANTERN_MISSPELT: &str = "Lantren tonite";

#[gpui::test]
fn a_song_matched_to_a_recording_its_name_only_nearly_spells_takes_that_recording(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();

    folder.tone(&format!("{LANTERN_MISSPELT}.wav"), 1);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let track = library
        .tracks(&TrackQuery::default())
        .expect("the tracks read")[0]
        .id;
    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

    let root = driven.root.clone();

    driven.cx.update(|window, cx| {
        root.update(cx, |root, cx| {
            root.ask_to_match(track, LANTERN_MISSPELT.into(), window, cx);
        });
    });

    assert!(
        driven.read(|root, _| root.matching.is_some()),
        "the matching sheet did not open"
    );

    driven.settle();
    driven.click("likely-recording-0");

    assert!(
        driven.read(|root, _| root.matching.is_none()),
        "the sheet stayed open once a recording was chosen"
    );

    driven.until(|_, _| {
        library
            .tracks(&TrackQuery::default())
            .expect("the tracks read")
            .first()
            .is_some_and(|held| held.title == "Lantern Tonight")
    });

    let taken = &library
        .tracks(&TrackQuery::default())
        .expect("the tracks read")[0];

    assert_eq!(taken.artist.as_deref(), Some("Another Band & Third Band"));
    assert_eq!(
        library
            .track_to_ask(taken.id)
            .expect("the track read")
            .and_then(|asked| asked.mbid),
        Some(mbid(LANTERN_TONIGHT)),
        "the song was not given the recording chosen"
    );
}

fn a_song_held_alone(
    cx: &mut TestAppContext,
    folder: &Folder,
    named: &str,
) -> (Driven, Arc<Library>, Track) {
    folder.tone(&format!("{named}.wav"), 1);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, folder);

    let mut driven = Driven::reaching(cx, Arc::clone(&library), folder, reaching);

    driven.until(|root, cx| {
        let library = root.library.read(cx);
        !library.listing().is_empty() && !library.is_enriching()
    });

    let track = library
        .tracks(&TrackQuery::default())
        .expect("the tracks read")
        .remove(0);

    (driven, library, track)
}

fn enrich_this_song_pressed(driven: &mut Driven, track: &Track) {
    identified_by(driven, track, "Enrich this song");
}

fn identified_by(driven: &mut Driven, track: &Track, label: &str) {
    let row: &'static str = Box::leak(format!("track-{}", track.id.get()).into_boxed_str());

    driven.right_click(row);

    let offered = driven
        .read(|root, _| root.menu.as_ref().and_then(|menu| menu.offering(label)))
        .expect("the song's menu offers it");
    let crate::views::menu::Offered::Beneath(at, under) = offered else {
        panic!("{label} stands outside the Identify nest");
    };
    let nest: &'static str = Box::leak(format!("menu-entry-{at}").into_boxed_str());
    let entry: &'static str = Box::leak(format!("menu-beneath-{at}-{under}").into_boxed_str());

    driven.click(nest);
    driven.click(entry);
}

pub(super) fn told(driven: &mut Driven) -> Option<String> {
    driven.read(|_, cx| toast::showing_text(cx))
}

fn told_in_turn(driven: &mut Driven, said: &str) {
    driven.until_ticking(Duration::from_secs(1), |_, cx| {
        toast::showing_text(cx).as_deref() == Some(said)
    });
}

#[gpui::test]
fn enrich_this_song_finds_the_song_by_its_code_and_says_so(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let (mut driven, library, track) = a_song_held_alone(cx, &folder, "Lantern Tonight");

    library
        .set_isrc(track.id, &Isrc::new(LANTERN_TONIGHT_ISRC).expect("an isrc"))
        .expect("the code is given");

    enrich_this_song_pressed(&mut driven, &track);

    assert_eq!(
        told(&mut driven).as_deref(),
        Some("Looking up Lantern Tonight on MusicBrainz")
    );

    told_in_turn(
        &mut driven,
        "Found Lantern Tonight on MusicBrainz and filled in what it says",
    );

    assert_eq!(
        library
            .track_to_ask(track.id)
            .expect("the track read")
            .and_then(|asked| asked.mbid),
        Some(mbid(LANTERN_TONIGHT)),
        "the song was not given the recording its code names"
    );
}

#[gpui::test]
fn clearing_what_was_fetched_for_a_song_looks_it_up_again_and_fills_it_in_afresh(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let (mut driven, library, track) = a_song_held_alone(cx, &folder, "Lantern Tonight");

    library
        .set_isrc(track.id, &Isrc::new(LANTERN_TONIGHT_ISRC).expect("an isrc"))
        .expect("the code is given");

    enrich_this_song_pressed(&mut driven, &track);
    told_in_turn(
        &mut driven,
        "Found Lantern Tonight on MusicBrainz and filled in what it says",
    );
    driven.until(|root, cx| !root.library.read(cx).is_enriching());

    identified_by(&mut driven, &track, "Clear and fetch again");
    driven.until(|root, cx| root.library.read(cx).is_enriching());
    told_in_turn(
        &mut driven,
        "Found Lantern Tonight on MusicBrainz and filled in what it says",
    );

    assert_eq!(
        library
            .track_to_ask(track.id)
            .expect("the track read")
            .and_then(|asked| asked.mbid),
        Some(mbid(LANTERN_TONIGHT)),
        "the song fetched again was not given its recording"
    );
}

#[gpui::test]
fn clearing_what_was_fetched_on_a_metered_connection_clears_nothing_and_says_why(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let (mut driven, _library, track) = a_song_held_alone(cx, &folder, "Lantern Tonight");

    driven.metering.report(resonate_core::Meter::Metered);

    identified_by(&mut driven, &track, "Clear and fetch again");

    assert!(driven.read(|root, cx| !root.library.read(cx).is_enriching()));
    assert_eq!(
        told(&mut driven).as_deref(),
        Some(
            "Lantern Tonight wasn't looked up: the connection is metered, and Settings › Online holds lookups on it"
        )
    );
}

#[gpui::test]
fn enrich_this_song_says_plainly_when_nothing_strictly_matches(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let (mut driven, _library, track) = a_song_held_alone(cx, &folder, LANTERN_MISSPELT);

    enrich_this_song_pressed(&mut driven, &track);

    told_in_turn(
        &mut driven,
        "MusicBrainz has no strict match for Lantren tonite, so nothing was changed",
    );
}

#[gpui::test]
fn enrich_this_song_on_a_metered_connection_asks_nothing_and_says_why(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let (mut driven, _library, track) = a_song_held_alone(cx, &folder, "Lantern Tonight");

    driven.metering.report(resonate_core::Meter::Metered);

    enrich_this_song_pressed(&mut driven, &track);

    assert!(driven.read(|root, cx| !root.library.read(cx).is_enriching()));
    assert_eq!(
        told(&mut driven).as_deref(),
        Some(
            "Lantern Tonight wasn't looked up: the connection is metered, and Settings › Online holds lookups on it"
        )
    );
}

#[gpui::test]
fn pressing_the_artist_of_a_song_found_on_musicbrainz_opens_the_artist_and_wants_nothing(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

    typed(&mut driven, "Lantern Tonight");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());
    driven.click("unheld-artist-0");
    driven.until(|root, cx| matches!(root.library.read(cx).selection(), Selection::Artist(_)));

    let another_band = library
        .artist_named("Another Band")
        .expect("the artist read")
        .expect("the artist landed");

    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).selection()),
        Selection::Artist(another_band)
    );
    assert!(
        library.wants().expect("the wants read").is_empty(),
        "pressing the artist wanted the song"
    );
}
