mod enriching;
mod fixtures;
mod found;
mod searching;
mod wanting;

use std::{sync::Arc, time::Duration};

use gpui::TestAppContext;
use resonate_core::ReleaseTrackId;
use resonate_library::{CoverSource, HeldMedium, Library, Link, ReleaseDetail, Track, TrackQuery};

use crate::{
    driven::{Driven, Folder},
    models::Selection,
    theme,
    views::{
        browser::{
            grid::{A_CELL_GROWS_AT_MOST, GRID_PADDING, columns_across, side_across},
            missing_rows::Unheld,
            record::record_of,
        },
        kit,
        root::{Deleting, Pane},
    },
};

#[test]
fn a_missing_song_credited_as_its_album_names_the_albums_artist_like_the_songs_held() {
    let release_track = ReleaseTrackId::new(1).expect("an id");
    let credited_alike = Unheld::of(release_track, None, 1, "Yeni Bir Flow", None, None);
    let credited_apart = Unheld::of(
        release_track,
        None,
        5,
        "Hani Nerdeler",
        Some("Diyar Pala & Emir"),
        Some(Duration::from_secs(200)),
    );

    let named = credited_alike.credited_as(Some("Diyar Pala"));
    let kept = credited_apart.credited_as(Some("Diyar Pala"));
    let unowned = Unheld::of(release_track, None, 1, "Yeni Bir Flow", None, None).credited_as(None);

    assert_eq!(named.artist.as_ref(), "Diyar Pala");
    assert_eq!(kept.artist.as_ref(), "Diyar Pala & Emir");
    assert_eq!(unowned.artist.as_ref(), "");
}

#[test]
fn the_cells_of_a_grid_fill_its_width_and_never_shrink_under_a_cover() {
    let _worn = crate::theme::alongside();

    for width in [520.0_f32, 700.0, 861.0, 1_283.0, 1_921.0, 2_560.0] {
        let columns = columns_across(width) as f32;
        let side = side_across(width);

        let filled = columns * side + (columns - 1.0) * theme::grid_gap();
        let room = width - GRID_PADDING;

        assert!(
            side >= theme::grid_cover(),
            "a cell under a cover at {width}"
        );
        assert!(filled <= room, "the grid overflows a pane {width} wide");
        assert!(
            room - filled < columns,
            "the grid stops {} short of a pane {width} wide",
            room - filled
        );
    }
}

#[test]
fn one_cell_in_a_wide_pane_grows_no_further_than_its_picture_stays_sharp() {
    let _worn = crate::theme::alongside();

    let narrow = theme::grid_cover() + GRID_PADDING;

    assert_eq!(columns_across(narrow), 1);
    assert_eq!(side_across(narrow), theme::grid_cover());
    assert!(side_across(narrow + 180.0) <= theme::grid_cover() * A_CELL_GROWS_AT_MOST);
}

fn unreleased() -> ReleaseDetail {
    ReleaseDetail {
        mbid: None,
        group: None,
        date: None,
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: None,
        disambiguation: None,
        cover_source: CoverSource::File,
        may_have_a_front: true,
        asked: None,
        answered: None,
        links: Vec::new(),
        media: Vec::new(),
    }
}

#[test]
fn a_record_names_every_fact_the_release_holds_and_an_empty_one_names_nothing() {
    assert!(record_of(&unreleased()).is_none());

    let mut release = unreleased();

    release.date = Some("1973-03-01".to_owned());
    release.country = Some("GB".to_owned());
    release.label = Some("Harvest".to_owned());
    release.catalog_number = Some("SHVL 804".to_owned());
    release.barcode = Some("077774638220".to_owned());
    release.kind = Some("Album".to_owned());
    release.disambiguation = Some("remaster".to_owned());
    release.media = vec![HeldMedium {
        position: 1,
        format: Some("CD".to_owned()),
        title: None,
    }];
    release.links = vec![
        Link::new(
            "free streaming",
            "https://music.apple.com/us/album/1".to_owned(),
        ),
        Link::new("free streaming", "https://bandcamp.com/album/1".to_owned()),
    ];

    let record = record_of(&release).expect("a full release is a record");
    let facts: Vec<(&str, &str)> = record
        .facts
        .iter()
        .map(|fact| (fact.label, fact.value.as_str()))
        .collect();

    assert_eq!(
        facts,
        [
            ("Released", "1973-03-01"),
            ("Format", "CD"),
            ("Label", "Harvest"),
            ("Catalogue number", "SHVL 804"),
            ("Barcode", "077774638220"),
            ("Country", "GB"),
            ("Kind", "Album"),
        ]
    );
    assert_eq!(record.note.as_deref(), Some("remaster"));
    assert_eq!(
        record.on.iter().map(|heard| heard.name).collect::<Vec<_>>(),
        ["Apple Music", "Bandcamp"]
    );
}

#[gpui::test]
fn an_artists_picture_stands_at_its_side_from_the_first_frame_its_page_draws(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();

    folder.tagged(
        "short.wav",
        1,
        &[(b"INAM", "One"), (b"IART", "Air"), (b"IPRD", "Moon Safari")],
    );
    folder.tagged(
        "long.wav",
        1,
        &[
            (b"INAM", "Two"),
            (
                b"IART",
                "An Artist Whose Name Runs Long Enough To Wrap Across The Title",
            ),
            (b"IPRD", "Another"),
        ],
    );

    let library = Arc::new(Library::open_in_memory().expect("a catalog"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::open(cx, Arc::clone(&library));
    let side = gpui::px(kit::hero_side());

    for at in [0, 1, 0] {
        driven
            .root
            .update(&mut driven.cx, |root, cx| root.set_pane(Pane::Artists, cx));
        driven.settle();

        let artist = driven
            .read(|root, cx| root.library.read(cx).artists().get(at).map(|held| held.id))
            .expect("an artist held");

        driven.root.update(&mut driven.cx, |root, cx| {
            root.opened(Selection::Artist(artist), cx);
        });
        driven.cx.update(|window, cx| window.draw(cx).clear(cx));

        let first = driven.cx.debug_bounds(kit::HERO_ART);

        driven.settle();

        let settled = driven.cx.debug_bounds(kit::HERO_ART);

        assert_eq!(first.map(|drawn| drawn.size), Some(gpui::size(side, side)));
        assert_eq!(
            settled.map(|drawn| drawn.size),
            Some(gpui::size(side, side))
        );
        assert_eq!(
            first.map(|drawn| drawn.origin.x),
            settled.map(|drawn| drawn.origin.x)
        );
    }
}

fn asked_to_delete(driven: &mut Driven, track: &Track) {
    let root = driven.root.clone();
    let deleting = Deleting::of(track);

    driven.cx.update(|_, cx| {
        root.update(cx, |root, cx| root.ask_to_delete(deleting, cx));
    });
    driven.settle();
}

#[gpui::test]
fn deleting_a_track_asks_first_and_takes_its_file_only_once_confirmed(cx: &mut TestAppContext) {
    let folder = Folder::new();
    let doomed = folder.tone("doomed.wav", 1);
    let kept = folder.tone("kept.wav", 1);
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
    let track = library
        .tracks(&TrackQuery::default())
        .expect("the tracks read")
        .into_iter()
        .find(|track| {
            track
                .location
                .as_path()
                .is_some_and(|path| path.ends_with("doomed.wav"))
        })
        .expect("the track was scanned");

    asked_to_delete(&mut driven, &track);
    driven.click("keep-the-track");

    let kept_on_cancel = doomed.exists();

    asked_to_delete(&mut driven, &track);
    driven.click("delete-the-track");
    driven.until(|_, _| !doomed.exists());
    driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

    assert!(
        kept_on_cancel,
        "the file went before the delete was confirmed"
    );
    assert!(kept.exists());
    assert!(library.track(track.id).expect("the track reads").is_none());
    assert!(
        library
            .deleted_everywhere_to_tell()
            .expect("the queue reads")
            .is_empty()
    );
}

#[gpui::test]
fn deleting_a_track_from_every_device_takes_its_file_and_waits_to_tell_the_others(
    cx: &mut TestAppContext,
) {
    let folder = Folder::new();
    let doomed = folder.tone("doomed.wav", 1);
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

    Driven::scanned(&library, &folder);

    let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
    let track = library
        .tracks(&TrackQuery::default())
        .expect("the tracks read")
        .into_iter()
        .next()
        .expect("the track was scanned");
    let root = driven.root.clone();
    let deleting = Deleting::of(&track).on_every_device();

    driven.cx.update(|_, cx| {
        root.update(cx, |root, cx| root.ask_to_delete(deleting, cx));
    });
    driven.settle();
    driven.click("delete-the-track");
    driven.until(|_, _| !doomed.exists());
    driven.until(|root, cx| root.library.read(cx).tracks_counted() == 0);

    assert_eq!(
        library
            .deleted_everywhere_to_tell()
            .expect("the queue reads")
            .len(),
        1
    );
}
