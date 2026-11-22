use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{ClipboardItem, TestAppContext};
use parking_lot::Mutex;
use resonate_core::SourceId;
use resonate_library::{EnrichOptions, Fingerprinters, Library, MissingTrack};
use resonate_providers::Providers;

use super::fixtures::{
    Asked, HEROES_TONIGHT, HEROES_TONIGHT_GROUP, MusicBrainz, NCS_PLAYLIST, Shop, heroes_tonight,
    mbid,
};
use crate::{
    Supplying,
    downloads::Fetching,
    driven::{Driven, Folder, Reaching},
    models::Selection,
    views::menu::Menu,
};

#[gpui::test]
fn a_missing_song_pressed_on_an_album_is_listed_among_the_downloads(cx: &mut TestAppContext) {
    let folder = Folder::new();

    folder.tagged(
        "other.wav",
        1,
        &[
            (b"IART", "Janji & Johnning"),
            (b"INAM", "Another Song"),
            (b"IPRD", "Heroes Tonight"),
        ],
    );

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

    Driven::scanned(&library, &folder);
    library
        .enrich(
            Arc::new(MusicBrainz::new()),
            Arc::new(Fingerprinters::none()),
            EnrichOptions::default(),
        )
        .expect("the enrichment starts")
        .join()
        .expect("the enrichment finishes");

    let mut driven = Driven::reaching(cx, Arc::clone(&library), &folder, reaching);

    let missing = library
        .missing_tracks(None, None)
        .expect("the missing tracks read")
        .remove(0);
    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.select(Selection::Album(missing.album), cx);
        });
    });
    driven.until(|root, cx| !root.library.read(cx).release_tracks().is_empty());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.want(missing.release_track, cx));
    });
    driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());
    driven.until(|_, _| !asked.lock().is_empty());

    let listed = driven.read(|root, cx| {
        root.library
            .read(cx)
            .downloads()
            .iter()
            .map(|download| (download.found.title.clone(), download.found.artist.clone()))
            .collect::<Vec<_>>()
    });

    assert_eq!(
        listed,
        [("Heroes Tonight".to_owned(), "Janji & Johnning".to_owned())]
    );
    assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
}

fn an_album_short_of_a_song(
    cx: &mut TestAppContext,
    asked: &Arc<Mutex<Vec<Asked>>>,
    folder: &Folder,
) -> (Driven, Arc<Library>, MissingTrack, &'static str) {
    folder.tagged(
        "other.wav",
        1,
        &[
            (b"IART", "Janji & Johnning"),
            (b"INAM", "Another Song"),
            (b"IPRD", "Heroes Tonight"),
        ],
    );

    let told = Arc::clone(asked);
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

    Driven::scanned(&library, folder);
    library
        .enrich(
            Arc::new(MusicBrainz::new()),
            Arc::new(Fingerprinters::none()),
            EnrichOptions::default(),
        )
        .expect("the enrichment starts")
        .join()
        .expect("the enrichment finishes");

    let mut driven = Driven::reaching(cx, Arc::clone(&library), folder, reaching);

    let missing = library
        .missing_tracks(None, None)
        .expect("the missing tracks read")
        .remove(0);
    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.select(Selection::Album(missing.album), cx);
        });
    });
    driven.until(|root, cx| !root.library.read(cx).release_tracks().is_empty());

    let at = driven.read(|root, cx| {
        root.library
            .read(cx)
            .release_tracks()
            .iter()
            .position(|row| row.id == missing.release_track)
            .expect("the missing row is on the album")
    });
    let title: &'static str = Box::leak(format!("unheld-title-{at}").into_boxed_str());

    (driven, library, missing, title)
}

#[gpui::test]
fn a_right_press_on_a_missing_song_of_an_album_downloads_it_without_waiting_to_play_it(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));

    let (mut driven, library, _, title) = an_album_short_of_a_song(cx, &asked, &folder);
    let recording = mbid(HEROES_TONIGHT);

    driven.right_click(title);

    assert!(
        driven.read(|root, _| root.menu.is_some()),
        "a right press on a missing song opened no menu"
    );
    assert_eq!(
        driven.read(|root, _| root.menu.as_ref().map(Menu::labels)),
        Some(vec![
            "Download and play".to_owned(),
            String::new(),
            "Download".to_owned()
        ])
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
fn a_right_press_on_a_missing_song_of_an_album_can_download_it_and_play_it_once_it_lands(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));

    let (mut driven, library, _, title) = an_album_short_of_a_song(cx, &asked, &folder);
    let recording = mbid(HEROES_TONIGHT);

    driven.right_click(title);
    driven.click("menu-entry-0");
    driven.until(|_, _| !asked.lock().is_empty());

    assert!(
        driven.read(|root, cx| root.library.read(cx).is_awaited(&recording)),
        "the song pressed to download and play was not waited on"
    );
    assert_eq!(library.wants().expect("the wants read").len(), 1);
}

#[gpui::test]
fn a_right_press_on_a_missing_song_already_wanted_offers_to_stop_wanting_it(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));

    let (mut driven, library, missing, title) = an_album_short_of_a_song(cx, &asked, &folder);

    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.want(missing.release_track, cx));
    });
    driven.until(|root, cx| {
        root.library
            .read(cx)
            .wanted(missing.release_track)
            .is_some()
    });

    driven.right_click(title);

    assert_eq!(
        driven.read(|root, _| root.menu.as_ref().map(Menu::labels)),
        Some(vec!["Stop wanting this track".to_owned()]),
        "a want already asked for offered more than to stop it"
    );

    driven.click("menu-entry-0");
    driven.until(|root, cx| {
        root.library
            .read(cx)
            .wanted(missing.release_track)
            .is_none()
    });

    assert!(library.wants().expect("the wants read").is_empty());
}

fn shop_reaching(asked: &Arc<Mutex<Vec<Asked>>>, open: &Arc<AtomicBool>) -> Reaching {
    let told = Arc::clone(asked);
    let opened = Arc::clone(open);
    Reaching {
        reference: Arc::new(MusicBrainz::new()),
        register: Arc::new(move |_: &Supplying<'_>| {
            if !opened.load(Ordering::Relaxed) {
                return Providers::none();
            }
            Providers::none().and(Arc::new(Shop {
                source: SourceId::new("shop").expect("a source name"),
                asked: Arc::clone(&told),
            }))
        }),
    }
}

fn heroes_tonight_found() -> resonate_library::Found {
    let matched = heroes_tonight();
    resonate_library::Found {
        recording: matched.recording,
        title: matched.title,
        artist: "Janji & Johnning".to_owned(),
        length: None,
        release: matched.releases.first().cloned(),
        releases: matched.releases,
        performer: None,
        isrcs: Vec::new(),
    }
}

#[gpui::test]
fn a_song_still_wanted_is_listed_among_the_downloads_when_the_window_opens_again(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let open = Arc::new(AtomicBool::new(true));
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    library
        .want_found(&MusicBrainz::new(), &heroes_tonight_found())
        .expect("the song is wanted");

    let mut driven = Driven::reaching(cx, library, &folder, shop_reaching(&asked, &open));

    driven.until(|root, cx| !root.library.read(cx).downloads().is_empty());

    let listed = driven.read(|root, cx| {
        let library = root.library.read(cx);
        library
            .downloads()
            .iter()
            .map(|download| {
                (
                    download.found.title.clone(),
                    library.fetching(download).is_underway(),
                )
            })
            .collect::<Vec<_>>()
    });

    assert_eq!(listed, [("Heroes Tonight".to_owned(), true)]);
}

#[gpui::test]
fn a_song_asked_for_before_any_provider_was_set_up_is_fetched_once_one_is(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let asked = Arc::new(Mutex::new(Vec::new()));
    let open = Arc::new(AtomicBool::new(false));
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    let mut driven = Driven::reaching(cx, library, &folder, shop_reaching(&asked, &open));

    let model = driven.read(|root, _| root.library.clone());

    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| {
            library.want_found(heroes_tonight_found(), cx);
        });
    });
    driven.until(|root, cx| {
        let library = root.library.read(cx);
        library
            .downloads()
            .first()
            .is_some_and(|download| library.fetching(download) == Fetching::NoProvider)
    });

    assert!(asked.lock().is_empty());

    open.store(true, Ordering::Relaxed);
    driven.cx.update(|_, cx| {
        model.update(cx, |library, cx| library.sources_moved(cx));
    });
    driven.until(|_, _| !asked.lock().is_empty());

    assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
}

#[gpui::test]
fn a_playlist_link_pasted_into_the_search_wants_every_song_it_holds_that_is_not_held(
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

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "https://www.deezer.com/en/playlist/{NCS_PLAYLIST}"
        )));
        search.update(cx, |search, cx| search.pastes(window, cx));
    });
    driven.until(|_, _| !asked.lock().is_empty());

    assert_eq!(
        driven.read(|root, cx| root.typed.search.read(cx).text().to_owned()),
        ""
    );
    assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));
    assert_eq!(library.wants().expect("the wants read").len(), 1);
    assert!(
        library
            .playlist_named("NCS")
            .expect("the playlists read")
            .is_none(),
        "a playlist was made of songs none of which is held"
    );
}

#[gpui::test]
fn a_song_link_pasted_into_the_search_is_downloaded_and_leaves_the_box_as_it_was(
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

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(
            "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT?si=a1".to_owned(),
        ));
        search.update(cx, |search, cx| search.pastes(window, cx));
    });
    driven.until(|_, _| !asked.lock().is_empty());

    assert_eq!(
        driven.read(|root, cx| root.typed.search.read(cx).text().to_owned()),
        "",
        "the link was neither kept in the box nor swapped for the song's name"
    );
    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).downloads().len()),
        1
    );
    assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));

    let wants = library.wants().expect("the wants read");

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].title, "Heroes Tonight");
}

#[gpui::test]
fn an_album_link_pasted_into_the_search_wants_every_song_of_the_album_it_names(
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

    let search = driven.read(|root, _| root.typed.search.clone());

    driven.cx.update(|window, cx| {
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "https://musicbrainz.org/release-group/{HEROES_TONIGHT_GROUP}"
        )));
        search.update(cx, |search, cx| search.pastes(window, cx));
    });
    driven.until(|_, _| !asked.lock().is_empty());

    assert_eq!(
        driven.read(|root, cx| root.typed.search.read(cx).text().to_owned()),
        "",
        "the album link was taken for words to search"
    );
    assert_eq!(
        driven.read(|root, cx| root.library.read(cx).downloads().len()),
        1
    );
    assert_eq!(asked.lock()[0].recording, Some(mbid(HEROES_TONIGHT)));

    let wants = library.wants().expect("the wants read");

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].album_title, "Heroes Tonight");
}
