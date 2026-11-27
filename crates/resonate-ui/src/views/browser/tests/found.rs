use std::sync::Arc;

use gpui::{Point, TestAppContext};
use parking_lot::Mutex;
use resonate_core::{SourceId, ZoneId};
use resonate_library::Library;
use resonate_providers::Providers;

use super::{
    fixtures::{Delivers, HEROES_TONIGHT, HEROES_TONIGHT_GROUP, LIGHTS_GROUP, MusicBrainz, mbid},
    searching::typed,
};
use crate::{
    Supplying,
    driven::{Driven, Folder, Reaching},
    icons::Icon,
    models::Selection,
    views::{menu::Menu, root::Pane, search::SearchShows},
};

fn a_reach_for_an_album_not_held(cx: &mut TestAppContext) -> Driven {
    let folder = Folder::new();
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, library, &folder, reaching);

    typed(&mut driven, "Heroes Tonight");
    driven.until(|root, cx| !root.library.read(cx).albums_found().is_empty());
    driven
}

#[gpui::test]
fn an_album_not_held_opens_its_page_when_the_lookup_lands(cx: &mut TestAppContext) {
    let mut driven = a_reach_for_an_album_not_held(cx);

    driven.root.update(&mut driven.cx, |root, cx| {
        root.open_album_not_held(mbid(HEROES_TONIGHT_GROUP), cx);
    });
    driven.until(|root, cx| matches!(root.library.read(cx).selection(), Selection::Album(_)));

    assert_eq!(driven.read(|root, _| root.pane), Pane::Tracks);
}

#[gpui::test]
fn a_song_downloaded_on_an_album_opened_from_the_search_stays_on_it_as_the_held_row(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let music = Folder::new();
    let file = folder.tone("delivered.wav", 1);
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Delivers {
                source: SourceId::new("shop").expect("a source name"),
                file: file.clone(),
                positions: Arc::default(),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
    let music_folder = music.path().to_path_buf();

    driven.cx.update(|_, cx| {
        gpui::BorrowAppContext::update_global::<crate::ResonateApp, _>(cx, |global, _| {
            global.music_folder = Some(music_folder);
        });
    });

    typed(&mut driven, "Heroes Tonight");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());
    driven.root.update(&mut driven.cx, |root, cx| {
        root.open_album_not_held(mbid(LIGHTS_GROUP), cx);
    });
    driven.until(|root, cx| root.library.read(cx).release_tracks().len() == 2);

    let opened = driven.read(|root, cx| root.library.read(cx).selection());

    driven.click("unheld-title-1");
    driven.until(|root, cx| {
        root.library
            .read(cx)
            .release_tracks()
            .iter()
            .any(|row| row.title == "Skyline" && row.track.is_some())
    });

    let shown = driven.read(|root, cx| {
        let library = root.library.read(cx);
        (
            library.selection(),
            library
                .listing()
                .iter()
                .map(|track| track.title.clone())
                .collect::<Vec<_>>(),
            library.rows().len(),
        )
    });

    assert_eq!(
        shown,
        (opened, vec!["Skyline".to_owned()], 2),
        "the song downloaded left the album it was pressed on"
    );

    typed(&mut driven, "Heroes");
    driven.until(|root, cx| root.library.read(cx).listing().is_empty());
}

#[gpui::test]
fn an_album_not_held_does_not_open_over_a_pane_the_window_moved_to_while_it_was_asked(
    cx: &mut TestAppContext,
) {
    let mut driven = a_reach_for_an_album_not_held(cx);

    driven.root.update(&mut driven.cx, |root, cx| {
        root.open_album_not_held(mbid(HEROES_TONIGHT_GROUP), cx);
        root.set_pane(Pane::Queue, cx);
    });
    driven.settle();

    assert_eq!(driven.read(|root, _| root.pane), Pane::Queue);
    assert!(!matches!(
        driven.read(|root, cx| root.library.read(cx).selection()),
        Selection::Album(_)
    ));
}

#[gpui::test]
fn a_lookup_pressed_goes_stale_when_a_pane_a_menu_or_another_lookup_moves_the_window_on(
    cx: &mut TestAppContext,
) {
    let mut driven = a_reach_for_an_album_not_held(cx);

    let stale = driven.root.update(&mut driven.cx, |root, cx| {
        let pressed = root.press_a_lookup();

        assert!(!root.window_has_moved_on_from(pressed, cx));

        root.set_pane(Pane::Queue, cx);

        let after_the_pane = root.window_has_moved_on_from(pressed, cx);

        let pressed = root.press_a_lookup();

        root.open_a_menu(
            Menu::at(Point::default()).does(Icon::Play, "Play", |_, _, _| {}),
            cx,
        );

        let after_a_menu = root.window_has_moved_on_from(pressed, cx);

        let first = root.press_a_lookup();
        let second = root.press_a_lookup();
        (
            after_the_pane,
            after_a_menu,
            root.window_has_moved_on_from(first, cx),
            root.window_has_moved_on_from(second, cx),
        )
    });

    assert_eq!(stale, (true, true, true, false));
}

#[gpui::test]
fn a_found_song_pressed_lands_once_as_the_held_track_and_plays(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let music = Folder::new();
    let file = folder.tone("delivered.wav", 1);
    let positions = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&positions);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Delivers {
                source: SourceId::new("shop").expect("a source name"),
                file: file.clone(),
                positions: Arc::clone(&heard),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
    let music_folder = music.path().to_path_buf();

    driven.cx.update(|_, cx| {
        gpui::BorrowAppContext::update_global::<crate::ResonateApp, _>(cx, |global, _| {
            global.music_folder = Some(music_folder);
        });
    });

    typed(&mut driven, "Heroes Tonight");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());
    driven.click("unheld-title-0");
    driven.until(|root, cx| {
        let library = root.library.read(cx);
        !library.listing().is_empty() && library.found().is_empty()
    });

    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).listing().len()),
        1
    );

    driven.until(|root, cx| root.player.read(cx).state().current.is_some());

    assert!(
        driven.read(|root, cx| !root.library.read(cx).is_awaited(&mbid(HEROES_TONIGHT))),
        "a song pressed and landed within half a minute plays once and is awaited no more"
    );
    assert_eq!(
        positions.lock().clone(),
        vec![None],
        "the one ask began from what the search found, before the release was read"
    );
}

#[gpui::test]
fn a_found_song_pressed_on_one_zone_plays_there_though_another_is_shown_when_it_lands(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let music = Folder::new();
    let file = folder.tone("delivered.wav", 1);
    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            Providers::none().and(Arc::new(Delivers {
                source: SourceId::new("shop").expect("a source name"),
                file: file.clone(),
                positions: Arc::default(),
            }))
        }),
    };
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);
    let music_folder = music.path().to_path_buf();

    driven.cx.update(|_, cx| {
        gpui::BorrowAppContext::update_global::<crate::ResonateApp, _>(cx, |global, _| {
            global.music_folder = Some(music_folder);
        });
    });
    driven
        .root
        .update(&mut driven.cx, |root, cx| root.add_a_zone(cx));
    driven.until(|root, _| root.zones.len() == 2);

    let second = driven.read(|root, _| root.switcher.selected());

    driven.root.update(&mut driven.cx, |root, cx| {
        root.show_zone(ZoneId::FIRST, cx);
    });
    driven.settle();

    typed(&mut driven, "Heroes Tonight");
    driven.until(|root, cx| !root.library.read(cx).found().is_empty());
    driven.root.update(&mut driven.cx, |root, cx| {
        let found = root
            .library
            .read(cx)
            .found()
            .first()
            .cloned()
            .expect("a song found on MusicBrainz");
        let zone = root.zone_acted_on();

        root.library
            .update(cx, |library, cx| library.fetch_and_play(found, zone, cx));
        root.show_zone(second, cx);
    });
    driven.until(|root, cx| {
        root.zones
            .iter()
            .any(|shown| !shown.player.read(cx).queue().is_empty())
    });

    let queued_on = |driven: &mut Driven, zone: ZoneId| {
        driven.read(|root, cx| {
            root.zones
                .iter()
                .find(|shown| shown.id == zone)
                .map_or(0, |shown| shown.player.read(cx).queue().len())
        })
    };

    assert_eq!(driven.read(|root, _| root.switcher.selected()), second);
    assert_eq!(queued_on(&mut driven, ZoneId::FIRST), 1);
    assert_eq!(
        queued_on(&mut driven, second),
        0,
        "the zone turned to while it downloaded took the play"
    );
}

#[gpui::test]
fn a_title_by_an_artist_is_searched_as_that_title_by_that_artist(cx: &mut TestAppContext) {
    let folder = Folder::new();

    folder.tagged("1.wav", 1, &[(b"IART", "Mira Vale"), (b"INAM", "Hey A.B.")]);
    folder.tagged(
        "2.wav",
        1,
        &[(b"IART", "Somebody Else"), (b"INAM", "Hey A.B.")],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let mut driven = Driven::reaching(cx, library, &folder, reaching);

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|_, cx| {
        search.update(cx, |search, cx| {
            search.set_text("Hey A B by mira vale".to_owned(), cx);
        });
    });
    driven.until(|root, cx| root.library.read(cx).meant().is_some());

    let artists = |driven: &mut Driven| {
        driven.read(|root, cx| {
            root.library
                .read(cx)
                .listing()
                .iter()
                .map(|track| track.artist.clone().unwrap_or_default())
                .collect::<Vec<_>>()
        })
    };

    assert_eq!(artists(&mut driven), ["Mira Vale"]);

    driven.click("search-as-typed");
    driven.until(|root, cx| root.library.read(cx).meant().is_none());

    assert!(
        artists(&mut driven).is_empty(),
        "the words as typed hold the word by and a misspelt name"
    );
}

#[gpui::test]
fn a_search_begun_on_any_pane_opens_the_top_results(cx: &mut TestAppContext) {
    let folder = Folder::new();

    folder.tagged(
        "one.wav",
        1,
        &[(b"IART", "Janji"), (b"INAM", "Heroes"), (b"IPRD", "Album")],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::opened_in(cx, library, &folder);

    let search = driven.read(|root, _| root.typed.search.clone());

    for sidebar in [
        "queue",
        "tab-albums",
        "tab-artists",
        "tab-tracks",
        "tab-settings",
    ] {
        driven.click(sidebar);
        driven.cx.update(|_, cx| {
            search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
        });
        driven.until(|root, cx| root.search_in_front(cx).is_some());

        let (pane, shows) = driven.read(|root, cx| (root.pane, root.search_in_front(cx)));

        assert_eq!(shows, Some(SearchShows::Top), "searching from {sidebar}");
        assert!(
            matches!(pane, Pane::Albums | Pane::Artists | Pane::Tracks),
            "searching from {sidebar} left the pane at {pane:?}"
        );

        driven.cx.update(|_, cx| {
            search.update(cx, |search, cx| search.clear(cx));
        });
        driven.until(|root, cx| root.search_in_front(cx).is_none());
    }
}

#[gpui::test]
fn songs_not_in_the_library_stand_on_the_first_page_however_many_it_holds(cx: &mut TestAppContext) {
    let folder = Folder::new();

    for number in 0..40 {
        let title = format!("Heroes {number}");

        folder.tagged(
            &format!("{number:02}.wav"),
            1,
            &[(b"IART", "Janji"), (b"INAM", title.as_str())],
        );
    }

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let mut driven = Driven::reaching(cx, library, &folder, reaching);

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|_, cx| {
        search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
    });
    driven.until(|root, cx| {
        let library = root.library.read(cx);
        library.listing().len() == 40 && !library.found().is_empty()
    });

    let found = driven.bounds_of("found-0");
    let seen = driven.cx.update(|window, _| window.viewport_size());

    assert!(
        found.bottom() <= seen.height,
        "the song found elsewhere was drawn below the window, at {found:?}"
    );
    assert_eq!(
        driven.read(|root, cx| root.search_in_front(cx)),
        Some(SearchShows::Top)
    );

    driven.click("search-songs");

    assert_eq!(
        driven.read(|root, cx| (root.pane, root.search_in_front(cx))),
        (Pane::Tracks, Some(SearchShows::Songs))
    );

    let (held, found) = driven.read(|root, cx| {
        let library = root.library.read(cx);
        (library.listing().len(), library.found().len())
    });
    let rows = driven.read(|root, cx| root.library.read(cx).rows());

    assert_eq!(rows.len(), held + 1 + found);
    assert_eq!(rows[held], crate::ListedRow::NotHeld(found));
    assert!(
        rows.iter()
            .skip(held + 1)
            .enumerate()
            .all(|(at, row)| *row == crate::ListedRow::Found(at)),
        "the songs found follow the songs held, under their own heading"
    );
    assert!(driven.read(|root, cx| root.library.read(cx).found_at(held + 1).is_some()));

    driven.click("search-top");
    driven.bounds_of("found-0");

    driven.click("unheld-title-0");
    driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
}

#[gpui::test]
fn an_album_found_elsewhere_stands_under_the_albums_held_and_in_the_top_results(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();

    folder.tagged(
        "heroes.wav",
        1,
        &[
            (b"IART", "Janji"),
            (b"INAM", "Heroes Again"),
            (b"IPRD", "Heroes"),
        ],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let reaching = Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(|_: &Supplying<'_>| Providers::none()),
    };
    let mut driven = Driven::reaching(cx, library, &folder, reaching);
    let cell: &'static str =
        Box::leak(format!("album-found-{HEROES_TONIGHT_GROUP}").into_boxed_str());

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|_, cx| {
        search.update(cx, |search, cx| search.set_text("heroes".to_owned(), cx));
    });
    driven.until(|root, cx| !root.library.read(cx).albums_found().is_empty());

    let seen = driven.cx.update(|window, _| window.viewport_size());

    assert!(driven.bounds_of(cell).bottom() <= seen.height);

    driven.click("search-albums");

    assert_eq!(
        driven.read(|root, cx| (root.pane, root.search_in_front(cx))),
        (Pane::Albums, Some(SearchShows::Albums))
    );
    assert!(driven.bounds_of(cell).bottom() <= seen.height);
    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).albums().len()),
        1,
        "the album held is still the grid"
    );
}
