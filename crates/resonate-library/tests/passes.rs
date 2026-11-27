use std::{
    sync::{
        Arc,
        mpsc::{Receiver, Sender},
    },
    time::Duration,
};

use resonate_core::TrackId;
use resonate_library::{
    AlbumQuery, ArtistMatch, Breadth, Credit, EnrichOptions, EnrichSummary, Fingerprinters,
    Library, LookupOp, Pace, Reference, Release, Result, Sought, Stopped, TrackQuery, Waits,
};

use crate::support::{
    A_DAY, ALBUM, ARTIST, CODE, Called, Canned, Fake, Fault, HOURS, MUSICBRAINZ_ALBUM,
    MUSICBRAINZ_ARTIST, ORBITERS, ORBITERS_PICTURE, ORBITS_TITLES, RECORDING, RELEASE,
    RELEASE_GROUP, TITLE, Tree, WAITED, Wav, ada, album_titled, artist_match, artist_named, beside,
    enrich, group_release, hours, isrc, mbid, on_orbits, only_album, options, orbiters, orbits,
    orbits_group, orbits_group_match, orbits_match, orbits_recording, orbits_rows, orbits_track,
    png_art, scan, scanned_lone, scanned_orbits, stored, waited_for, write_hours, write_orbits,
};

#[test]
fn an_artist_named_in_a_release_group_credit_is_spared_a_search_the_same_way() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut group = orbits_group(vec![group_release(HOURS, "1996-05-06", None)], Vec::new());

    group.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![group],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindArtist), 0);

    let artists: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Artist)
        .collect();

    assert_eq!(artists, vec![Called::Artist(mbid(ORBITERS))]);
    assert_eq!(summary.stats.artists, 1);

    let artist = artist_named(&library, "The Orbiters")?;

    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    Ok(())
}

#[test]
fn an_artist_search_is_taken_only_at_a_perfect_score_with_the_same_folded_name() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let found = |score: u8, name: &str| ArtistMatch {
        mbid: mbid(ORBITERS),
        name: name.to_owned(),
        score,
        kind: Some("Group".to_owned()),
        disambiguation: None,
        aliases: Vec::new(),
    };

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(99, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindArtist), 1);
    assert_eq!(fake.called(LookupOp::Artist), 0);

    let artist = artist_named(&library, "The Orbiters")?;

    assert_eq!(artist.mbid, None);

    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");

    assert!(detail.asked.is_some());
    assert_eq!(detail.answered, None);

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(100, "Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, true)?;

    assert_eq!(fake.called(LookupOp::Artist), 0);
    assert_eq!(artist_named(&library, "The Orbiters")?.mbid, None);

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(100, "the orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, true)?;

    assert_eq!(fake.called(LookupOp::Artist), 1);

    let artist = artist_named(&library, "The Orbiters")?;

    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    assert!(
        library
            .artist_detail(artist.id)?
            .is_some_and(|detail| detail.answered.is_some())
    );
    Ok(())
}

#[test]
fn a_portrait_is_fetched_only_where_the_profile_names_an_image_and_none_is_held() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "Orbits/1.wav",
        &orbits_track("Small Mercies", 1)
            .described(MUSICBRAINZ_ARTIST, ORBITERS)
            .build(),
    );
    write_hours(&tree, None);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        artists: vec![orbiters(), ada()],
        portraits: vec![(ORBITERS_PICTURE.to_owned(), png_art(96))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let portraits: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Portrait)
        .collect();

    assert_eq!(
        portraits,
        vec![Called::Portrait(ORBITERS_PICTURE.to_owned())]
    );
    assert_eq!(summary.stats.artists, 2);
    assert_eq!(summary.stats.portraits, 1);
    assert_eq!(summary.stats.portraits_missing, 0);

    let orbiters = artist_named(&library, "The Orbiters")?;

    assert!(orbiters.has_portrait);
    assert_eq!(library.portrait(orbiters.id)?, Some(png_art(96)));

    let ada = artist_named(&library, "Ada")?;

    assert!(!ada.has_portrait);
    assert_eq!(library.portrait(ada.id)?, None);

    let summary = enrich(&library, &fake, true)?;

    assert_eq!(fake.called(LookupOp::Portrait), 1);
    assert_eq!(summary.stats.portraits, 0);
    Ok(())
}

#[test]
fn a_reference_that_cannot_be_reached_ends_the_pass_and_stamps_nothing() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);
    write_hours(&tree, None);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 1, Fault::Unreachable),
    );
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        summary.stopped_by,
        Some(Stopped::Unreached(LookupOp::Release))
    );
    assert_eq!(fake.called(LookupOp::Release), 2);
    assert_eq!(fake.called(LookupOp::Artist), 0);
    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.refused, 0);

    let mut answered = 0;
    let mut untouched = 0;

    for album in library.albums(&AlbumQuery::default())? {
        let release = library.release_of(album.id)?.expect("the album is known");
        match (release.asked, release.answered) {
            (Some(_), Some(_)) => answered += 1,
            (None, None) => untouched += 1,
            other => panic!("an album was left half stamped: {other:?}"),
        }
    }

    assert_eq!((answered, untouched), (1, 1));
    Ok(())
}

#[test]
fn a_pass_a_shut_gate_or_a_switched_off_network_ends_says_so_and_not_unreached() -> Result<()> {
    for (fault, said) in [
        (Fault::Metered, Stopped::Metered),
        (Fault::Offline, Stopped::Offline),
    ] {
        let tree = Tree::new();

        write_orbits(&tree, true);
        write_hours(&tree, None);

        let library = Library::open_in_memory()?;

        scan(&library, &options(&tree))?;

        let fake = Arc::new(
            Fake::new(Canned {
                releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
                ..Canned::default()
            })
            .faulting(LookupOp::Release, 1, fault),
        );
        let summary = enrich(&library, &fake, false)?;

        assert_eq!(summary.stopped_by, Some(said));
        assert_eq!(summary.stats.releases, 1);
        assert!(
            library.unfinished_enrichment()?.is_some(),
            "a pass the gate ended is left for the next run"
        );
    }
    Ok(())
}

#[test]
fn a_refusal_counts_and_stamps_asked_and_the_pass_carries_on() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 0, Fault::Refused),
    );

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    assert_eq!(summary.stats.refused, 1);
    assert_eq!(summary.stats.releases, 0);
    assert_eq!(summary.stats.albums_unmatched, 0);
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(fake.called(LookupOp::Cover), 0);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, None);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert!(release.asked.is_some());
    assert_eq!(release.answered, None);
    Ok(())
}

#[test]
fn a_release_too_large_to_read_is_stamped_asked_and_counts_as_no_refusal() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 0, Fault::TooLarge),
    );

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    assert_eq!(summary.stats.refused, 0);
    assert_eq!(summary.stats.releases, 0);

    let release = library
        .release_of(only_album(&library)?.id)?
        .expect("the album is known");

    assert!(release.asked.is_some());
    assert_eq!(release.answered, None);
    Ok(())
}

#[test]
fn a_refused_release_search_stamps_the_refusal_rather_than_asking_down_the_ladder() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::FindRelease, 0, Fault::Refused),
    );

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.refused, 1);
    assert_eq!(fake.called(LookupOp::FindRelease), 1);
    assert_eq!(fake.called(LookupOp::FindReleaseGroup), 0);
    assert_eq!(fake.called(LookupOp::ReleaseGroup), 0);
    assert_eq!(fake.called(LookupOp::Release), 0);

    let refused_at_once = Waits {
        retry_after: A_DAY,
        refused_again_after: Duration::ZERO,
        refresh_after: A_DAY,
    };

    assert_eq!(library.albums_to_ask(refused_at_once, false)?.len(), 1);
    Ok(())
}

#[test]
fn a_refusal_earns_the_wait_a_service_is_given_rather_than_the_one_a_miss_earns() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 0, Fault::Refused),
    );

    assert_eq!(enrich(&library, &fake, false)?.stats.refused, 1);

    let refused_at_once = Waits {
        retry_after: A_DAY,
        refused_again_after: Duration::ZERO,
        refresh_after: A_DAY,
    };
    let missed_at_once = Waits {
        retry_after: Duration::ZERO,
        refused_again_after: A_DAY,
        refresh_after: A_DAY,
    };

    assert_eq!(
        library.albums_to_ask(refused_at_once, false)?.len(),
        1,
        "a refused album waits out the wait a refusal earns"
    );
    assert!(
        library.albums_to_ask(missed_at_once, false)?.is_empty(),
        "a refused album was counted as an album nobody has heard of"
    );
    Ok(())
}

#[test]
fn a_rescan_that_adds_a_track_takes_it_off_the_missing_count_without_asking_again() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "Orbits/1.wav",
        &orbits_track("Small Mercies", 1)
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 1);
    assert_eq!(only_album(&library)?.missing, 2);

    tree.write(
        "Orbits/2.wav",
        &orbits_track("A Quilt of Winds", 2)
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .build(),
    );

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.added, 1);
    assert_eq!(only_album(&library)?.missing, 2);

    let silent = Arc::new(Fake::new(Canned::default()));
    let summary = enrich(&library, &silent, false)?;

    assert!(
        silent.calls().is_empty(),
        "the reference was asked again: {:?}",
        silent.calls()
    );
    assert_eq!(summary.stats.albums, 0);
    assert_eq!(
        summary.stats.matched, 1,
        "the count says how many rows the rematch moved, not how many are paired"
    );
    assert_eq!(only_album(&library)?.missing, 1);
    Ok(())
}

#[test]
fn a_picture_is_fetched_beside_the_pass_rather_than_in_it() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        covers: vec![(mbid(RELEASE), png_art(256))],
        found_artists: vec![artist_match(100, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    })
    .gated_on(Some(LookupOp::Cover));
    let fake = Arc::new(fake);
    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started.recv().expect("the cover reaches the reference");

    let asked_while_held = waited_for(|| fake.called(LookupOp::FindArtist) == 1);

    go.send(()).expect("the reference is still waiting");

    let summary = handle.join()?;

    assert!(
        asked_while_held,
        "the pass waited for a picture rather than going on: {:?}",
        fake.calls()
    );
    assert_eq!(summary.stats.covers, 1);
    assert!(album_titled(&library, "Orbits")?.has_cover_art);
    Ok(())
}

#[test]
fn a_pass_the_reference_ended_is_left_for_the_next_run_to_pick_up() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);
    write_hours(&tree, None);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.unfinished_enrichment()?, None);

    let fake = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 1, Fault::Unreachable),
    );
    let summary = enrich(&library, &fake, true)?;

    assert_eq!(
        summary.stopped_by,
        Some(Stopped::Unreached(LookupOp::Release))
    );

    let left = library
        .unfinished_enrichment()?
        .expect("a pass the reference ended is left unfinished");

    assert!(
        left.refresh,
        "the next run would pick the pass up without the refresh it was asked for"
    );

    let answering = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        ..Canned::default()
    }));

    enrich(&library, &answering, true)?;

    assert_eq!(
        library.unfinished_enrichment()?,
        None,
        "a pass that reached the end of its queue is still there to be picked up"
    );
    Ok(())
}

fn tagged_orbits() -> Result<(Tree, Library)> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

#[test]
fn a_cover_the_archive_did_not_hand_over_is_asked_for_again_by_the_next_pass() -> Result<()> {
    let (_tree, library) = tagged_orbits()?;
    let faulted = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new())],
            covers: vec![(mbid(RELEASE), png_art(128))],
            ..Canned::default()
        })
        .faulting(LookupOp::Cover, 0, Fault::Refused),
    );

    let first = enrich(&library, &faulted, false)?;

    assert_eq!(first.stats.releases, 1);
    assert_eq!(first.stats.covers, 0);
    assert_eq!(library.cover_art(only_album(&library)?.id)?, None);

    let answering = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    let second = enrich(&library, &answering, false)?;

    assert_eq!(
        answering.calls(),
        vec![Called::Cover(mbid(RELEASE), Some(mbid(RELEASE_GROUP)))],
        "the pass asked for more than the cover it is still short of"
    );
    assert_eq!(second.stats.covers, 1);
    assert_eq!(
        library.cover_art(only_album(&library)?.id)?,
        Some(png_art(128))
    );
    Ok(())
}

#[test]
fn a_cover_the_archive_answered_it_does_not_hold_is_not_asked_for_again_within_the_month()
-> Result<()> {
    let (_tree, library) = tagged_orbits()?;
    let empty_handed = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &empty_handed, false)?;

    assert_eq!(empty_handed.called(LookupOp::Cover), 1);

    let again = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));

    enrich(&library, &again, false)?;

    assert_eq!(again.called(LookupOp::Cover), 0);
    assert_eq!(library.cover_art(only_album(&library)?.id)?, None);

    assert_eq!(library.ask_again_for_covers()?, 1);

    let asked_again = enrich(&library, &again, false)?;

    assert_eq!(again.called(LookupOp::Cover), 1);
    assert_eq!(asked_again.stats.covers, 1);
    assert_eq!(library.ask_again_for_covers()?, 0);
    Ok(())
}

#[test]
fn a_cover_the_archive_said_it_lacked_a_month_ago_is_asked_for_again() -> Result<()> {
    const A_YEAR_AGO_IN_NANOS: i64 = 365 * 24 * 60 * 60 * 1_000_000_000;

    let tree = Tree::new();

    write_orbits(&tree, true);

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let empty_handed = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &empty_handed, false)?;

    assert_eq!(empty_handed.called(LookupOp::Cover), 1);

    let stamped: i64 = beside(&database)
        .query_row("SELECT cover_asked FROM albums", [], |row| row.get(0))
        .expect("the answer was stamped");

    beside(&database)
        .execute(
            "UPDATE albums SET cover_asked = ?1",
            [stamped - A_YEAR_AGO_IN_NANOS],
        )
        .expect("the stamp is put back a year");

    let later = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    let asked_again = enrich(&library, &later, false)?;

    assert_eq!(later.called(LookupOp::Cover), 1);
    assert_eq!(asked_again.stats.covers, 1);
    assert_eq!(
        library.cover_art(only_album(&library)?.id)?,
        Some(png_art(128))
    );
    Ok(())
}

#[test]
fn cancelling_an_enrichment_stops_it_between_requests() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);
    write_hours(&tree, None);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        covers: vec![(mbid(RELEASE), png_art(256)), (mbid(HOURS), png_art(128))],
        ..Canned::default()
    })
    .gated();
    let fake = Arc::new(fake);
    let reference = Arc::clone(&fake) as Arc<dyn Reference>;
    let handle = library.enrich(
        reference,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started
        .recv()
        .expect("the first request reaches the reference");
    handle.cancel();

    assert!(handle.progress().is_cancelled());

    go.send(()).expect("the reference is still waiting");

    let summary = handle.join()?;

    assert!(summary.cancelled);
    assert_eq!(summary.stopped_by, None);
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(
        library.unfinished_enrichment()?,
        None,
        "a pass the listener stopped was left for the next run to pick up"
    );

    let answered = library
        .albums(&AlbumQuery::default())?
        .into_iter()
        .filter(|album| {
            library
                .release_of(album.id)
                .ok()
                .flatten()
                .is_some_and(|release| release.answered.is_some())
        })
        .count();

    assert_eq!(answered, 1);
    Ok(())
}

#[test]
fn an_album_gone_while_a_lookup_asks_about_it_is_passed_over_and_the_pass_goes_on() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);
    write_hours(&tree, None);

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        ..Canned::default()
    })
    .gated_on(Some(LookupOp::Release));
    let fake = Arc::new(fake);
    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started
        .recv()
        .expect("the release lookup reaches the reference");

    let gone = beside(&database)
        .execute("DELETE FROM albums WHERE title = 'Orbits'", [])
        .expect("the album is removed under the lookup");

    assert_eq!(gone, 1);

    go.send(()).expect("the reference is still waiting");

    let summary = handle.join()?;

    assert!(!summary.cancelled);
    assert_eq!(summary.stopped_by, None);
    assert!(
        fake.calls().len() > 1,
        "the pass ended at the album that went"
    );
    Ok(())
}

fn under_a_gated_pass(
    library: &Library,
    fake: Fake,
    has_started: &Receiver<()>,
    go: &Sender<()>,
    refresh: bool,
    reached: impl FnOnce(&Sought),
) -> Result<(Arc<Fake>, EnrichSummary)> {
    let fake = Arc::new(fake);
    let sought = Arc::new(Sought::default());

    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions {
            refresh,
            sought: Arc::clone(&sought),
            ..EnrichOptions::default()
        },
    )?;

    has_started
        .recv()
        .expect("the first request reaches the reference");
    reached(&sought);
    go.send(()).expect("the reference is still waiting");

    let summary = handle.join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");

    Ok((fake, summary))
}

fn asked_at(fake: &Fake, wanted: &Called) -> Option<usize> {
    fake.calls().iter().position(|called| called == wanted)
}

#[test]
fn an_album_that_became_due_under_a_running_pass_is_asked_next_when_it_is_sought() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        artists: vec![orbiters(), ada()],
        found_artists: vec![artist_match(100, "The Orbiters"), artist_match(100, "Ada")],
        ..Canned::default()
    })
    .gated();

    let (fake, _) = under_a_gated_pass(&library, fake, &has_started, &go, false, |sought| {
        write_hours(&tree, None);
        scan(&library, &options(&tree)).expect("a second scan lands the new album");
        sought.album(
            album_titled(&library, "Hours")
                .expect("the new album was scanned")
                .id,
        );
    })?;

    let hours = asked_at(&fake, &Called::Release(mbid(HOURS)));

    assert!(
        hours.is_some(),
        "an album scanned under the pass was never asked about: {:?}",
        fake.calls()
    );

    let artist = fake
        .calls()
        .iter()
        .position(|called| called.op() == LookupOp::FindArtist || called.op() == LookupOp::Artist);

    assert!(
        hours < artist,
        "the sought album was asked after the artists the queue already held: {:?}",
        fake.calls()
    );
    Ok(())
}

#[test]
fn a_seek_naming_an_album_the_reference_has_already_answered_asks_nothing() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let answering = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    }));

    enrich(&library, &answering, false)?;

    let orbits = album_titled(&library, "Orbits")?.id;
    let silent = Arc::new(Fake::new(Canned::default()));
    let sought = Arc::new(Sought::default());

    sought.album(orbits);

    let summary = library
        .enrich(
            Arc::clone(&silent) as Arc<dyn Reference>,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                sought,
                ..EnrichOptions::default()
            },
        )?
        .join()?;

    let asked = queued(&silent);

    assert!(
        asked.is_empty(),
        "a seek asked again about an album that was not due: {asked:?}"
    );
    assert_eq!(summary.stats.albums, 0);
    Ok(())
}

fn queued(fake: &Fake) -> Vec<Called> {
    fake.calls()
        .into_iter()
        .filter(|called| called.op() != LookupOp::Portrait)
        .collect()
}

#[test]
fn a_portrait_that_never_landed_is_looked_for_again_without_asking_the_reference_twice()
-> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let refusing = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new())],
            artists: vec![orbiters()],
            found_artists: vec![artist_match(100, "The Orbiters")],
            ..Canned::default()
        })
        .faulting(LookupOp::Portrait, 0, Fault::Refused),
    );

    enrich(&library, &refusing, false)?;

    assert_eq!(
        refusing.called(LookupOp::Portrait),
        1,
        "the picture was not asked for once while the artist was being landed"
    );

    let artist = artist_named(&library, "The Orbiters")?;

    assert!(!artist.has_portrait, "the fake answered with a portrait");

    let answering = Arc::new(Fake::new(Canned {
        portraits: vec![(ORBITERS_PICTURE.to_owned(), png_art(96))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &answering, false)?;

    assert_eq!(
        queued(&answering),
        Vec::new(),
        "the retry asked the reference about an album or an artist that was not due"
    );
    assert_eq!(
        answering.called(LookupOp::Portrait),
        1,
        "the picture was not looked for again out of the links already held"
    );
    assert_eq!(summary.stats.portraits, 1);
    assert!(artist_named(&library, "The Orbiters")?.has_portrait);

    let settled = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &settled, false)?;

    assert_eq!(
        settled.called(LookupOp::Portrait),
        0,
        "a portrait that has landed was looked for again"
    );
    Ok(())
}

#[test]
fn a_seek_naming_the_album_the_pass_is_asking_about_does_not_ask_twice() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = album_titled(&library, "Orbits")?.id;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    })
    .gated();

    let (fake, _) = under_a_gated_pass(&library, fake, &has_started, &go, true, |sought| {
        sought.album(album);
    })?;

    assert_eq!(
        fake.called(LookupOp::Release),
        1,
        "the album being asked about was asked about again: {:?}",
        fake.calls()
    );
    Ok(())
}

#[test]
fn a_portrait_the_reference_answered_it_lacks_is_not_looked_for_again_the_next_pass() -> Result<()>
{
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let empty_handed = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    }));

    enrich(&library, &empty_handed, false)?;

    assert_eq!(empty_handed.called(LookupOp::Portrait), 1);

    let later = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &later, false)?;

    assert_eq!(
        later.called(LookupOp::Portrait),
        0,
        "a portrait the reference said it lacks was asked for again on the very next pass"
    );
    Ok(())
}

#[test]
fn a_release_said_to_have_no_front_cover_is_pictured_by_its_group_and_never_by_itself() -> Result<()>
{
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![Release {
            has_front_cover: false,
            ..orbits(orbits_rows(), Vec::new())
        }],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let pictures: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Cover)
        .collect();

    assert_eq!(pictures, vec![Called::GroupCover(mbid(RELEASE_GROUP))]);
    Ok(())
}

#[test]
fn a_release_said_to_have_no_front_cover_and_no_group_is_never_asked_for_one() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![Release {
            has_front_cover: false,
            group: None,
            ..orbits(orbits_rows(), Vec::new())
        }],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert!(
        only_album(&library)?.mbid.is_some(),
        "the release did not land"
    );
    assert_eq!(fake.called(LookupOp::Cover), 0);

    let later = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &later, false)?;

    assert_eq!(
        later.called(LookupOp::Cover),
        0,
        "the sweep asked the archive for a cover the release said it lacks"
    );
    Ok(())
}

#[test]
fn a_discography_refused_as_its_artist_landed_is_asked_for_again_within_the_hour() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new())],
            artists: vec![orbiters()],
            found_artists: vec![artist_match(100, "The Orbiters")],
            ..Canned::default()
        })
        .faulting(LookupOp::ReleaseGroupsOfArtist, 0, Fault::Refused),
    );

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::ReleaseGroupsOfArtist), 1);

    let artist = artist_named(&library, "The Orbiters")?;

    assert!(artist.mbid.is_some(), "the profile did not land");

    let an_hour_on = Waits {
        refused_again_after: Duration::ZERO,
        ..WAITED
    };

    assert!(!library.artist_is_due(artist.id, WAITED, false)?);
    assert!(
        library.artist_is_due(artist.id, an_hour_on, false)?,
        "an artist whose discography was refused waits out the month"
    );
    Ok(())
}

#[test]
fn a_reference_refusing_lookup_after_lookup_ends_the_pass() -> Result<()> {
    const ALBUMS: usize = 12;
    const ENDS_AFTER: u64 = 10;

    let tree = Tree::new();

    for nth in 0..ALBUMS {
        tree.write(
            &format!("{nth}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Song {nth}"))
                .text(ARTIST, &format!("Singer {nth}"))
                .text(ALBUM, &format!("Album {nth}"))
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let mut fake = Fake::new(Canned::default());

    for nth in 0..ALBUMS * 4 {
        for op in [
            LookupOp::FindRelease,
            LookupOp::FindReleaseGroup,
            LookupOp::FindRecording,
            LookupOp::FindArtist,
        ] {
            fake = fake.faulting(op, nth, Fault::Refused);
        }
    }

    let fake = Arc::new(fake);
    let summary = enrich(&library, &fake, false)?;

    assert!(
        matches!(summary.stopped_by, Some(Stopped::RefusedInARow(_))),
        "the pass refused to the end"
    );
    assert_eq!(summary.stats.refused, ENDS_AFTER);
    assert_eq!(fake.calls().len() as u64, ENDS_AFTER);
    Ok(())
}

#[test]
fn a_lyric_the_service_refused_is_not_asked_for_again_on_the_next_pass() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned::default()).faulting(LookupOp::Lyrics, 0, Fault::Refused));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.sung().len(), ORBITS_TITLES.len());

    assert_eq!(
        library.lyrics_to_ask(false)?,
        Vec::new(),
        "a refused lyric is due again at once"
    );
    assert_eq!(library.lyrics_to_ask(true)?.len(), ORBITS_TITLES.len());
    Ok(())
}

#[test]
fn an_isrc_typed_for_a_track_is_asked_by_the_next_lookup_though_it_was_asked_in_vain_before()
-> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "Small Mercies")
            .text(ARTIST, "The Orbiters"),
    )?;

    let track = library
        .track_at(&file, None)?
        .expect("the catalog holds the track");

    let whole = orbits_recording(RECORDING, "Small Mercies", CODE, on_orbits(2));
    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(isrc(CODE), whole.clone())],
        recordings: vec![whole],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    let first = enrich(&library, &fake, false)?;

    assert_eq!(first.stats.tracks, 1);
    assert_eq!(fake.called(LookupOp::Isrc), 0);

    let waiting = enrich(&library, &fake, false)?;

    assert_eq!(waiting.stats.tracks, 0);

    library.set_isrc(track.id, &isrc(CODE))?;

    let typed = enrich(&library, &fake, false)?;

    assert_eq!(typed.stats.tracks, 1);
    assert_eq!(typed.stats.by_isrc, 1);
    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(
        stored(&database, &file).isrc.as_deref(),
        Some(isrc(CODE).as_str())
    );
    Ok(())
}

fn look_up_the_song(library: &Library, fake: &Arc<Fake>, song: TrackId) -> Result<EnrichSummary> {
    let reference = Arc::clone(fake) as Arc<dyn Reference>;
    let summary = library
        .enrich(
            reference,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                pace: Pace::Hurried,
                breadth: Breadth::TheSong(song),
                ..EnrichOptions::default()
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

#[test]
fn a_song_looked_up_alone_is_asked_at_once_though_asked_in_vain_and_nothing_else_is() -> Result<()>
{
    let tree = Tree::new();

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "Small Mercies")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    tree.write(
        "2.wav",
        &Wav::new()
            .text(TITLE, "Paper Boats")
            .text(ARTIST, "The Orbiters")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned::default()));

    let first = enrich(&library, &fake, false)?;
    let song = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|track| track.title == "Paper Boats")
        .expect("the catalog holds the song")
        .id;
    let looked_up = look_up_the_song(&library, &fake, song)?;
    let waiting = enrich(&library, &fake, false)?;

    assert_eq!(first.stats.tracks, 2);
    assert_eq!(looked_up.stats.tracks, 1);
    assert_eq!(looked_up.stats.tracks_unmatched, 1);
    assert_eq!(looked_up.stats.albums, 0);
    assert_eq!(
        waiting.stats.tracks, 0,
        "the song left waiting was asked again"
    );
    Ok(())
}

#[test]
fn a_song_on_an_answered_album_is_looked_up_through_its_album_again() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    let first = enrich(&library, &fake, false)?;
    let song = library.tracks(&TrackQuery::default())?[0].id;
    let asked_before = fake.called(LookupOp::Release);
    let looked_up = look_up_the_song(&library, &fake, song)?;

    assert_eq!(first.stats.releases, 1);
    assert_eq!(fake.called(LookupOp::Release), asked_before + 1);
    assert_eq!(looked_up.stats.albums, 1);
    assert_eq!(looked_up.stats.releases, 1);
    assert_eq!(
        looked_up.stats.tracks, 0,
        "a song its album answers for was asked alone"
    );
    assert_eq!(looked_up.stopped_by, None);
    Ok(())
}
