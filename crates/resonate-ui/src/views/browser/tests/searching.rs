use std::sync::{Arc, atomic::Ordering};

use gpui::TestAppContext;
use resonate_library::Library;
use resonate_providers::Providers;

use super::fixtures::MusicBrainz;
use crate::{
    Beyond, Supplying,
    driven::{Driven, Folder, Reaching},
    models::Selection,
    views::{
        reorder::{Listed, Shift},
        search::{TopEntry, TopRun},
    },
};

pub(super) fn searching(musicbrainz: &MusicBrainz, cx: &mut TestAppContext) -> Driven {
    let folder = Folder::new();
    let reaching = Reaching {
        reference: Arc::new(musicbrainz.sharing()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::reaching(cx, library, &folder, reaching)
}

pub(super) fn typed(driven: &mut Driven, query: &str) {
    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.set_query(query.to_owned(), cx));
    });
}

pub(super) fn answered(driven: &mut Driven) {
    driven.until(|root, cx| !root.library.read(cx).is_asking_elsewhere());
}

#[gpui::test]
fn words_searched_again_are_answered_from_memory_rather_than_asked_twice(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "Heroes Tonight");
    answered(&mut driven);
    typed(&mut driven, "janji heroes");
    answered(&mut driven);
    typed(&mut driven, "heroes   TONIGHT");

    let at_once = driven.read(|root, cx| root.library.read(cx).found().len());

    answered(&mut driven);

    assert_eq!(at_once, 1, "an answer held in memory waited to be drawn");
    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).found().len()),
        1
    );
    assert_eq!(
        musicbrainz.searched.lock().clone(),
        ["heroes tonight", "janji heroes"]
    );
}

#[gpui::test]
fn songs_found_for_fewer_words_stay_listed_while_more_are_asked_for(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "heroes");
    answered(&mut driven);
    typed(&mut driven, "heroes ton");

    let narrowed = driven.read(|root, cx| {
        let library = root.library.read(cx);
        (library.found().len(), library.is_asking_elsewhere())
    });

    typed(&mut driven, "radiohead");

    let unrelated = driven.read(|root, cx| root.library.read(cx).found().len());

    assert_eq!(narrowed, (1, true));
    assert_eq!(unrelated, 0);
}

#[gpui::test]
fn a_search_musicbrainz_refused_says_so_and_is_asked_again_on_a_press(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();

    musicbrainz.refusing.store(true, Ordering::Relaxed);

    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "heroes tonight");
    driven.until(|root, cx| root.library.read(cx).elsewhere() == Some(Beyond::Unreached(0)));
    musicbrainz.refusing.store(false, Ordering::Relaxed);
    driven.click("ask-elsewhere-again");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());

    assert_eq!(
        musicbrainz.searched.lock().clone(),
        ["heroes tonight", "heroes tonight"]
    );
}

fn asked_now(driven: &mut Driven) {
    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.ask_elsewhere_now(cx));
    });
}

#[gpui::test]
fn songs_found_stand_where_musicbrainz_refused_the_albums_which_are_asked_for_again(
    cx: &mut TestAppContext,
) {
    let musicbrainz = MusicBrainz::new();

    musicbrainz.refusing_albums.store(true, Ordering::Relaxed);

    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "heroes tonight");
    answered(&mut driven);

    let refused = driven.read(|root, cx| {
        let library = root.library.read(cx);
        (library.found().len(), library.albums_found().len())
    });

    musicbrainz.refusing_albums.store(false, Ordering::Relaxed);
    typed(&mut driven, "janji");
    answered(&mut driven);
    typed(&mut driven, "heroes tonight");
    answered(&mut driven);

    assert_eq!(refused, (1, 0));
    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).albums_found().len()),
        1
    );
    assert_eq!(
        musicbrainz.albums_searched.lock().clone(),
        ["heroes tonight", "janji", "heroes tonight"]
    );
}

#[gpui::test]
fn a_search_typed_past_does_not_ask_musicbrainz_for_its_albums(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "heroes tonight");
    asked_now(&mut driven);
    typed(&mut driven, "janji heroes");
    asked_now(&mut driven);
    answered(&mut driven);

    assert_eq!(
        musicbrainz.searched.lock().clone(),
        ["heroes tonight", "janji heroes"]
    );
    assert_eq!(musicbrainz.albums_searched.lock().clone(), ["janji heroes"]);
}

#[gpui::test]
fn albums_found_stay_listed_while_the_words_are_edited(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let mut driven = searching(&musicbrainz, cx);

    typed(&mut driven, "heroes tonight");
    answered(&mut driven);
    typed(&mut driven, "heroes tonigh");

    let while_asked = driven.read(|root, cx| {
        let library = root.library.read(cx);
        (library.albums_found().len(), library.is_asking_elsewhere())
    });

    assert_eq!(while_asked, (1, true));
}

#[gpui::test]
fn every_found_song_below_the_held_ones_can_be_reached_from_the_keyboard(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let folder = Folder::new();

    folder.tone("Heroes Tonight.wav", 1);

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let reaching = Reaching {
        reference: Arc::new(musicbrainz.sharing()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let mut driven = Driven::reaching(cx, library, &folder, reaching);

    typed(&mut driven, "heroes tonight");
    answered(&mut driven);
    driven.until(|root, cx| {
        let library = root.library.read(cx);
        !library.listing().is_empty() && !library.found().is_empty()
    });

    let found =
        driven.read(|root, cx| TopRun::of(root.library.read(cx)).row_of(TopEntry::Found(0)));

    driven.focus(|root| &root.typed.search);
    for _ in 0..=found {
        driven.cx.simulate_keystrokes("down");
    }
    driven.settle();

    assert!(driven.read(|root, cx| root.reaches(Shift::Listing(Listed::Top), found, cx)));

    driven.cx.simulate_keystrokes("enter");
    driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
}

#[gpui::test]
fn the_artists_songs_and_albums_on_the_top_results_are_one_run_the_keys_walk_and_press(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();

    folder.tagged(
        "echoes.wav",
        1,
        &[
            (b"INAM", "Echoes"),
            (b"IART", "Echoes"),
            (b"IPRD", "Echoes"),
        ],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::opened_in(cx, library, &folder);

    driven.click("tab-tracks");

    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.set_query("echoes".to_owned(), cx));
    });
    driven.until(|_, cx| {
        let library = model.read(cx);
        !library.listing().is_empty()
            && !library.albums().is_empty()
            && !library.artists().is_empty()
    });

    let run = driven.read(|root, cx| TopRun::of(root.library.read(cx)));
    let top = Shift::Listing(Listed::Top);

    assert_eq!(run.len(), 3);

    driven.focus(|root| &root.typed.search);
    driven.cx.simulate_keystrokes("down");
    driven.settle();

    assert!(
        driven.read(|root, cx| root.reaches(top, 0, cx)),
        "the song is not reached first"
    );

    driven.cx.simulate_keystrokes("down down");
    driven.settle();

    assert!(
        driven.read(|root, cx| root.reaches(top, 2, cx)),
        "the album is not reached last"
    );

    driven.cx.simulate_keystrokes("enter");
    driven.settle();

    assert!(matches!(
        driven.read(|_, cx| model.read(cx).selection()),
        Selection::Album(_)
    ));
}

#[gpui::test]
fn enter_straight_after_the_last_letter_reaches_the_results_typed_not_the_ones_before(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();

    folder.tagged(
        "alpha.wav",
        1,
        &[(b"INAM", "Alpha"), (b"IART", "Pink Floyd")],
    );
    folder.tagged(
        "zebra.wav",
        1,
        &[(b"INAM", "Zebra"), (b"IART", "Pink Floyd")],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::opened_in(cx, library, &folder);

    driven.click("tab-tracks");

    let top = Shift::Listing(Listed::Top);

    driven.focus(|root| &root.typed.search);
    driven.cx.simulate_input("zebra");

    assert!(
        driven.read(|root, cx| root.library.read(cx).is_reading_the_search()),
        "the read had settled before enter was pressed"
    );

    driven.cx.simulate_keystrokes("enter");
    driven.until(|root, cx| root.reaches(top, 0, cx));

    let titles = driven.read(|root, cx| {
        root.library
            .read(cx)
            .listing()
            .iter()
            .map(|track| track.title.to_string())
            .collect::<Vec<_>>()
    });

    assert_eq!(titles, ["Zebra"]);
}

#[gpui::test]
fn a_search_inside_an_artist_asks_musicbrainz_nothing(cx: &mut TestAppContext) {
    let musicbrainz = MusicBrainz::new();
    let mut driven = searching(&musicbrainz, cx);

    let model = driven.read(|root, _| root.library.clone());
    let artist = resonate_core::ArtistId::new(1).expect("a non-zero id");

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.select(Selection::Artist(artist), cx)
        });
    });
    typed(&mut driven, "heroes tonight");
    asked_now(&mut driven);
    driven.settle();

    assert!(musicbrainz.searched.lock().is_empty());
}
