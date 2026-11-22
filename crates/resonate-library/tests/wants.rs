use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, ReleaseTrackId, SourceId};
use resonate_library::{
    AlbumMatch, AlbumQuery, ArtistFound, Credit, Error, Isrc, Library, Link, PollOptions,
    PollProgress, RETRY_WAITS, RecordingMatch, ReleaseTrack, Result, Service,
    TRIES_BEFORE_GIVING_UP,
};
use resonate_providers::{Identity, Obtained, Provider, Providers, Result as ProvidedResult};

use crate::support::{
    ADA, ANOTHER_RECORDING, Called, Canned, Delivering, ECHOES, Fake, MEDDLE, ORBITERS, Offering,
    RECORDING, RELEASE, RELEASE_GROUP, RELEASE_TRACK, Tree, Unanswering,
    a_provider_offering_echoes, ada, credited, echoes_found, mbid, meddle_fake, meddle_release,
    only_album, opened_with_a_vault, options, orbiters_groups, orbits, orbits_rows, orbits_tree,
    png_art, recording_match, release_row, scan, scanned_orbits, scanned_orbits_on_disk,
    unreachable_server, wanted_san_tropez,
};

#[test]
fn a_want_is_one_per_release_row_and_carries_its_links() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(
                    4,
                    "San Tropez",
                    vec![Link::new(
                        "streaming",
                        "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
                    )],
                ),
            ],
            Vec::new(),
        ),
    )?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");

    let want = library.want(missing.id)?;

    assert_eq!(library.want(missing.id)?, want);
    assert!(matches!(
        library.want(ReleaseTrackId::MAX),
        Err(Error::UnknownReleaseTrack(_))
    ));

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].release_track, missing.id);
    assert_eq!(wants[0].album, album.id);
    assert_eq!(wants[0].album_title, "Orbits");
    assert_eq!(wants[0].title, "San Tropez");
    assert_eq!(wants[0].tried, None);
    assert_eq!(
        wants[0]
            .links
            .iter()
            .map(|link| link.service)
            .collect::<Vec<_>>(),
        vec![Service::Spotify]
    );

    let standing = library.wants_as_they_stand()?;

    assert_eq!(
        standing.iter().map(|want| want.id).collect::<Vec<_>>(),
        [want]
    );
    assert!(
        standing[0].links.is_empty(),
        "the wants read for how they stand read every link too"
    );

    assert!(library.unwant(want)?);
    assert!(!library.unwant(want)?);
    assert!(library.wants()?.is_empty());

    let again = library.want(missing.id)?;

    assert_eq!(library.wants()?.len(), 1);
    assert_eq!(library.wants()?[0].id, again);
    assert!(library.remove_root(tree.path())?);
    assert!(
        library.wants()?.is_empty(),
        "a want survived the root its album was scanned under"
    );
    Ok(())
}

#[test]
fn a_want_follows_the_track_it_was_for_across_a_release_the_reference_re_edited() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let recorded = |position: u32, title: &str, recording: &str| ReleaseTrack {
        recording: Some(mbid(recording)),
        ..release_row(position, title, Vec::new())
    };

    library.land_release(
        album.id,
        &orbits(
            vec![
                recorded(1, "One of These Days", RECORDING),
                recorded(2, "San Tropez", ANOTHER_RECORDING),
            ],
            Vec::new(),
        ),
    )?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");

    library.want(missing.id)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                recorded(1, "Seamus", RELEASE_GROUP),
                recorded(2, "One of These Days", RECORDING),
                recorded(3, "San Tropez", ANOTHER_RECORDING),
            ],
            Vec::new(),
        ),
    )?;

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(
        wants[0].title, "San Tropez",
        "the want moved to whatever row now sits where its own used to"
    );

    let moved = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the re-edited release still holds the row");

    assert_eq!(wants[0].release_track, moved.id);
    assert_eq!(moved.position, 3);
    Ok(())
}

#[test]
fn a_want_follows_its_seat_only_to_the_song_it_was_for() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(2, "San Tropez", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");

    library.want(missing.id)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(2, "San Tropez!", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;

    assert_eq!(
        library.wants()?.len(),
        1,
        "a want for the song at a seat was lost to a title written with a mark"
    );

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(2, "Seamus", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;

    assert!(
        library.wants()?.is_empty(),
        "a want for one song was carried onto another that took its seat"
    );
    Ok(())
}

#[test]
fn forgetting_a_root_takes_the_release_rows_under_its_albums_with_it() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![release_row(1, "One of These Days", Vec::new())],
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;

    assert_eq!(library.rematch(album.id)?, 1);
    assert_eq!(library.release_tracks(album.id)?.len(), 1);

    assert!(library.remove_root(tree.path())?);
    assert_eq!(library.release_of(album.id)?, None);
    assert!(library.release_tracks(album.id)?.is_empty());
    assert!(library.albums(&AlbumQuery::default())?.is_empty());
    Ok(())
}

#[test]
fn the_stub_provider_delivers_nothing_and_a_poll_stamps_the_try() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let want = wanted_san_tropez(&library)?;

    let providers = Arc::new(Providers::none());

    assert!(!providers.has_a_source());
    assert_eq!(
        providers.names(),
        vec![SourceId::new("unprovided").expect("a nameable source")]
    );
    assert_eq!(library.last_tried()?, None);

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert!(!summary.cancelled);
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.nothing, 1);
    assert_eq!(summary.stats.offered, 0);
    assert_eq!(summary.stats.refused, 0);

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert!(wants[0].tried.is_some());
    assert_eq!(wants[0].offered, None);
    assert_eq!(library.last_tried()?, wants[0].tried);
    Ok(())
}

#[test]
fn a_file_delivered_with_no_vault_is_written_as_the_offer() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    wanted_san_tropez(&library)?;

    let delivered = tree.path().join("inbox").join("san-tropez.flac");
    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered.clone())));
    let providers = inbox.registered();

    assert!(providers.has_a_source());
    assert_eq!(
        providers.names(),
        vec![
            SourceId::new("unprovided").expect("a nameable source"),
            SourceId::new("inbox")?
        ]
    );

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert_eq!(summary.stats.nothing, 0);
    assert_eq!(inbox.asked().len(), 1);

    let wants = library.wants()?;

    assert_eq!(
        wants[0].offered,
        Some(MediaLocation::local(&delivered).to_uri())
    );
    assert!(wants[0].tried.is_some());
    Ok(())
}

#[test]
fn a_provider_is_handed_the_identity_and_the_service_links_the_catalog_holds() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(ReleaseTrack {
        recording: Some(mbid(ORBITERS)),
        track: Some(mbid(RELEASE_TRACK)),
        isrcs: vec!["GBN9Y1100089".to_owned()],
        ..release_row(
            4,
            "San Tropez",
            vec![Link::new(
                "free streaming",
                "https://tidal.com/track/55391743".to_owned(),
            )],
        )
    });
    library.land_release(
        album.id,
        &orbits(
            rows,
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");

    library.want(missing.id)?;

    let asked = Arc::new(Offering::new("asked", Delivering::Nothing));

    library
        .poll(asked.registered(), PollOptions::default())?
        .join()?;

    let identity = asked.asked().pop().expect("the want was asked about");

    assert_eq!(identity.title, "San Tropez");
    assert_eq!(identity.album.as_deref(), Some("Orbits"));
    assert_eq!(identity.recording, Some(mbid(ORBITERS)));
    assert_eq!(identity.track, Some(mbid(RELEASE_TRACK)));
    assert_eq!(identity.release, Some(mbid(RELEASE)));
    assert_eq!(identity.isrcs, [Isrc::new("GBN9Y1100089")?]);
    assert_eq!(identity.length, Some(Duration::from_millis(4_000)));
    assert_eq!(identity.disc, Some(1));
    assert_eq!(identity.position, Some(4));
    assert_eq!(
        identity.track_on(Service::Tidal),
        Some("https://tidal.com/track/55391743")
    );
    assert_eq!(
        identity.release_on(Service::Tidal),
        Some("https://tidal.com/album/55391740")
    );
    Ok(())
}

#[test]
fn a_want_tried_lately_is_not_asked_again_until_the_window_passes() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_san_tropez(&library)?;

    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let providers = quiet.registered();

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(quiet.asked().len(), 1);

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.asked, 0);
    assert_eq!(quiet.asked().len(), 1);

    let summary = library
        .poll(
            providers,
            PollOptions {
                every_want: true,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(quiet.asked().len(), 2);
    Ok(())
}

fn credited_to_ada(id: &str, title: &str) -> RecordingMatch {
    RecordingMatch {
        credit: vec![Credit {
            name: "Ada".to_owned(),
            joined_by: String::new(),
            mbid: Some(mbid(ADA)),
        }],
        ..recording_match(id, title)
    }
}

#[test]
fn an_artist_named_by_what_was_typed_is_offered_when_unheld_and_landed_with_its_releases()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Fake::new(Canned {
        artists: vec![ada()],
        artist_releases: vec![(mbid(ADA), orbiters_groups())],
        ..Canned::default()
    });
    let matches = vec![
        credited_to_ada(RECORDING, "Echoes"),
        credited_to_ada(ANOTHER_RECORDING, "One of These Days"),
        recording_match(ANOTHER_RECORDING, "Ada"),
    ];

    let offered = library.unheld_artists_among(&matches, "ada")?;

    assert_eq!(
        offered,
        vec![ArtistFound {
            mbid: mbid(ADA),
            name: "Ada".to_owned(),
        }],
        "one artist, once, for the credits that carry an id"
    );
    assert!(
        library
            .unheld_artists_among(&matches, "ada echoes")?
            .is_empty()
    );
    assert!(
        library
            .unheld_artists_among(&matches, "year:1971")?
            .is_empty()
    );

    let held = vec![RecordingMatch {
        credit: vec![Credit {
            name: "The Orbiters".to_owned(),
            joined_by: String::new(),
            mbid: Some(mbid(ORBITERS)),
        }],
        ..recording_match(RECORDING, "Echoes")
    }];

    assert!(
        library.unheld_artists_among(&held, "orbiters")?.is_empty(),
        "an artist the catalog holds is not offered"
    );

    let artist = library.open_artist_found(&fake, &offered[0])?;
    let detail = library.artist_detail(artist)?.expect("the artist landed");

    assert_eq!(detail.mbid, Some(mbid(ADA)));
    assert!(
        !library.albums_not_held_by(artist)?.is_empty(),
        "its releases are on its page"
    );
    assert_eq!(
        library.open_artist_found(&fake, &offered[0])?,
        artist,
        "landing it again lands the same artist"
    );

    library.settle_the_credits()?;

    assert!(
        library.artist_detail(artist)?.is_some(),
        "an artist found elsewhere outlives a sweep"
    );
    assert_eq!(
        library.unheld_artists_among(&matches, "ada")?,
        offered,
        "and is offered again until something of its own is held"
    );

    Ok(())
}

fn album_match(group: &str, title: &str, artist: &str, kind: &str) -> AlbumMatch {
    AlbumMatch {
        group: mbid(group),
        score: 100,
        title: title.to_owned(),
        credit: credited(Some(artist), None),
        kind: Some(kind.to_owned()),
        secondary: Vec::new(),
        first_released: Some("1973".to_owned()),
    }
}

#[test]
fn an_album_the_words_name_is_offered_unless_the_catalog_holds_it() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let ada_group = "bbbbbbbb-1111-2222-3333-444444444444";
    let live_group = "cccccccc-1111-2222-3333-444444444444";
    let live = AlbumMatch {
        secondary: vec!["Live".to_owned()],
        ..album_match(live_group, "Orbits Live", "Ada", "Album")
    };

    let matches = vec![
        album_match(ada_group, "Orbits Again", "Ada", "Album"),
        album_match(ada_group, "Orbits Again", "Ada", "Album"),
        album_match(RELEASE_GROUP, "Orbits", "The Orbiters", "Album"),
        album_match(
            "dddddddd-1111-2222-3333-444444444444",
            "Orbits",
            "The Orbiters",
            "Album",
        ),
        album_match(
            "eeeeeeee-1111-2222-3333-444444444444",
            "Orbits (single)",
            "Ada",
            "Single",
        ),
        live,
        album_match(
            "ffffffff-1111-2222-3333-444444444444",
            "Meddle",
            "Pink Floyd",
            "Album",
        ),
    ];

    let offered = library.unheld_albums_among(&matches, "orbits")?;

    assert_eq!(
        offered
            .iter()
            .map(|found| found.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Orbits Again"],
        "an album held by group or by title and artist, a single, a live album, a repeat and \
         one the words never named are all left out"
    );
    assert_eq!(offered[0].artist, "Ada");
    assert_eq!(offered[0].group, mbid(ada_group));
    assert_eq!(offered[0].kind.as_deref(), Some("Album"));
    assert_eq!(offered[0].first_released.as_deref(), Some("1973"));
    assert!(
        library
            .unheld_albums_among(&matches, "year:1971")?
            .is_empty(),
        "a search naming no words names no album"
    );

    Ok(())
}

#[test]
fn a_song_found_elsewhere_is_wanted_by_landing_the_release_it_first_came_out_on() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let mut release = meddle_release();

    release.has_front_cover = true;

    let art = png_art(128);
    let fake = Fake::new(Canned {
        found_songs: vec![
            echoes_found(),
            recording_match(RECORDING, "Echoes"),
            recording_match(ANOTHER_RECORDING, "One of These Days"),
        ],
        releases: vec![release],
        covers: vec![(mbid(MEDDLE), art.clone())],
        ..Canned::default()
    });

    let found = library.found_elsewhere(&fake, "echoes year:1971")?;

    assert_eq!(
        fake.calls(),
        vec![Called::FindSongs("echoes".to_owned())],
        "only the words were sent"
    );
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].title, "Echoes");
    assert_eq!(found[0].artist, "Pink Floyd");
    assert_eq!(
        found[0].release.as_ref().map(|release| release.id.as_str()),
        Some(MEDDLE)
    );
    assert!(library.found_elsewhere(&fake, "year:1971")?.is_empty());

    let want = library.want_found(&fake, &found[0])?;
    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].title, "Echoes");
    assert_eq!(wants[0].album_title, "Meddle");
    assert_eq!(wants[0].artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(library.cover_art(wants[0].album)?, Some(art));
    assert_eq!(
        library.missing_tracks(None, None)?.len(),
        1,
        "a release the library holds nothing of lists only the row that was wanted"
    );
    assert_eq!(library.missing_counted(None)?.tracks, 1);

    let unheld = library.unheld_matching("echoes", None)?;

    assert_eq!(unheld.len(), 1);
    assert_eq!(unheld[0].want, Some(want));
    assert!(library.unheld_matching("seamus", None)?.is_empty());
    assert_eq!(
        library.albums(&AlbumQuery::default())?.len(),
        1,
        "an album holding no file is not browsed"
    );

    let again = library.found_elsewhere(&fake, "echoes")?;
    let offered: Vec<&str> = again.iter().map(|found| found.recording.as_str()).collect();

    assert!(
        offered.contains(&ECHOES) && offered.contains(&ANOTHER_RECORDING),
        "a recording the catalog names but holds no file of is still offered: {offered:?}"
    );
    assert_eq!(library.want_found(&fake, &found[0])?, want);

    scan(&library, &options(&tree))?;

    assert_eq!(library.wants()?.len(), 1, "a scan kept the wanted release");

    assert!(library.unwant(want)?);

    scan(&library, &options(&tree))?;

    assert!(library.unheld_matching("echoes", None)?.is_empty());
    Ok(())
}

#[test]
fn a_song_found_elsewhere_is_wanted_with_every_code_of_it_and_from_its_next_release_where_one_is_gone()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = meddle_release();

    release.media[0].tracks[1].isrcs = vec!["GBN9Y1100065".to_owned(), "GBN9Y1100066".to_owned()];

    let fake = Fake::new(Canned {
        releases: vec![release],
        ..Canned::default()
    });
    let matched = echoes_found();
    let gone = matched.releases[0].clone();

    let found = resonate_library::Found {
        recording: matched.recording,
        title: matched.title,
        artist: "Pink Floyd".to_owned(),
        length: matched.length,
        release: Some(gone),
        releases: matched.releases,
        performer: None,
        isrcs: vec![Isrc::new("GBN9Y1100066")?, Isrc::new("USX9P1100001")?],
    };

    library.want_found(&fake, &found)?;

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(
        wants[0].album_title, "Meddle",
        "the release that read was the next one"
    );
    assert_eq!(
        wants[0].identity().isrcs,
        [
            Isrc::new("GBN9Y1100065")?,
            Isrc::new("GBN9Y1100066")?,
            Isrc::new("USX9P1100001")?
        ],
        "the release's codes first, then those its takes carry"
    );
    Ok(())
}

#[test]
fn a_song_foreseen_from_what_the_search_found_is_not_asked_of_the_provider_again_by_the_poll()
-> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let fake = meddle_fake();
    let found = library.found_elsewhere(&fake, "echoes")?;
    let shop = a_provider_offering_echoes("shop");

    assert!(library.foresee(&shop.registered(), &found[0]));

    let asked = shop.asked();

    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].title, "Echoes");
    assert_eq!(asked[0].artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(asked[0].recording, Some(mbid(ECHOES)));
    assert_eq!(asked[0].album.as_deref(), Some("Meddle"));
    assert_eq!(asked[0].length, Some(Duration::from_secs(1_411)));

    library.want_found(&fake, &found[0])?;

    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(shop.asked().len(), 1, "the poll asked the provider again");
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(shop.opened(), 1);
    assert!(library.wants()?[0].held.is_some());
    Ok(())
}

#[test]
fn a_foresight_is_spent_by_the_poll_that_takes_it_and_asked_afresh_by_the_next() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = meddle_fake();

    let found = library.found_elsewhere(&fake, "echoes")?;
    let shop = a_provider_offering_echoes("shop");

    assert!(library.foresee(&shop.registered(), &found[0]));

    library.want_found(&fake, &found[0])?;
    library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(shop.asked().len(), 1);

    library
        .poll(shop.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(
        shop.asked().len(),
        2,
        "a second ask was answered from a foresight already spent"
    );
    Ok(())
}

#[test]
fn a_provider_offering_nothing_foresees_nothing_and_the_poll_asks_for_itself() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = meddle_fake();

    let found = library.found_elsewhere(&fake, "echoes")?;
    let empty = Arc::new(Offering::new("shop", Delivering::Nothing));

    assert!(!library.foresee(&empty.registered(), &found[0]));

    library.want_found(&fake, &found[0])?;

    let summary = library
        .poll(empty.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(empty.asked().len(), 2);
    assert_eq!(summary.stats.nothing, 1);
    Ok(())
}

#[test]
fn an_offer_foreseen_of_a_provider_the_poll_does_not_register_is_not_taken() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = meddle_fake();

    let found = library.found_elsewhere(&fake, "echoes")?;
    let gone = a_provider_offering_echoes("old");
    let current = Arc::new(Offering::new("current", Delivering::Nothing));

    assert!(library.foresee(&gone.registered(), &found[0]));

    library.want_found(&fake, &found[0])?;
    library
        .poll(current.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(current.asked().len(), 1);
    assert_eq!(gone.opened(), 0);
    Ok(())
}

#[test]
fn a_foresight_let_go_of_is_asked_afresh_by_the_poll() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = meddle_fake();

    let found = library.found_elsewhere(&fake, "echoes")?;
    let shop = a_provider_offering_echoes("shop");

    assert!(library.foresee(&shop.registered(), &found[0]));

    library.forget_foresight(&found[0].recording);
    library.want_found(&fake, &found[0])?;
    library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(shop.asked().len(), 2);
    Ok(())
}

struct Withdrawing {
    source: SourceId,
    library: Library,
    asked: Mutex<usize>,
}

impl Provider for Withdrawing {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        *self.asked.lock() += 1;

        let asked_about = self
            .library
            .wants()
            .expect("the wants read")
            .into_iter()
            .find(|want| want.identity().title == identity.title);

        if let Some(want) = asked_about {
            self.library.unwant(want.id).expect("the want is withdrawn");
        }
        Ok(Obtained::Nothing)
    }
}

#[test]
fn a_want_withdrawn_while_a_poll_asks_about_it_is_passed_over_and_the_poll_goes_on() -> Result<()> {
    let (_tree, library, database) = scanned_orbits_on_disk()?;

    wanted_every_missing_row(&library)?;

    let wanted = library.wants()?.len();

    assert!(wanted > 1);

    let withdrawing = Arc::new(Withdrawing {
        source: SourceId::new("shop").expect("a nameable source"),
        library: Library::open(&database)?,
        asked: Mutex::new(0),
    });
    let providers = Arc::new(Providers::none().and(Arc::clone(&withdrawing) as Arc<dyn Provider>));

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.asked, wanted as u64);
    assert_eq!(*withdrawing.asked.lock(), wanted);
    assert!(library.wants()?.is_empty());
    Ok(())
}

fn wrong_password(provider: SourceId) -> resonate_providers::Error {
    resonate_providers::Error::Unwelcome {
        provider,
        op: resonate_providers::ProviderOp::Search,
        code: 40,
    }
}

fn wanted_every_missing_row(library: &Library) -> Result<()> {
    let album = only_album(library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));
    rows.push(release_row(5, "Seamus", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    for missing in library
        .release_tracks(album.id)?
        .into_iter()
        .filter(|row| row.track.is_none())
    {
        library.want(missing.id)?;
    }
    Ok(())
}

#[test]
fn a_want_no_provider_could_answer_is_left_untried_and_asked_again_by_the_next_poll() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;

    wanted_san_tropez(&library)?;

    let server = Unanswering::new("server", wrong_password);
    let providers = Arc::new(Providers::none().and(Arc::clone(&server) as Arc<dyn Provider>));

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.refused, 1);
    assert_eq!(summary.stats.nothing, 0);
    assert_eq!(library.wants()?[0].tried, None);
    assert!(library.is_a_want_due(PollOptions::default())?);

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.asked, 1);
    assert_eq!(server.asked(), 2);
    Ok(())
}

#[test]
fn a_want_asked_for_again_is_due_at_once_however_lately_it_was_tried() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let want = wanted_san_tropez(&library)?;
    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let providers = Arc::new(Providers::none().and(Arc::clone(&quiet) as Arc<dyn Provider>));

    library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    assert!(library.wants()?[0].tried.is_some());
    assert!(!library.is_a_want_due(PollOptions::default())?);

    assert_eq!(wanted_san_tropez(&library)?, want);

    assert_eq!(library.wants()?[0].tried, None);
    assert!(library.is_a_want_due(PollOptions::default())?);

    library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(quiet.asked().len(), 2);
    Ok(())
}

#[test]
fn a_want_tried_in_vain_is_retried_after_longer_waits_then_given_up_until_asked_for_again()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let want = wanted_san_tropez(&library)?;
    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let providers = Arc::new(Providers::none().and(Arc::clone(&quiet) as Arc<dyn Provider>));

    library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;

    let tried = library.wants()?[0].clone();

    assert_eq!(tried.misses, 1);
    assert!(!tried.gave_up());
    assert_eq!(
        tried.due_at(),
        tried.tried.map(|tried| tried + RETRY_WAITS[0]),
        "the first retry waits the shortest"
    );
    assert!(!library.is_a_want_due(PollOptions::default())?);

    for _ in 1..TRIES_BEFORE_GIVING_UP {
        library
            .poll(Arc::clone(&providers), PollOptions::ASKING_EVERY_WANT)?
            .join()?;
    }

    let given_up = library.wants()?[0].clone();

    assert_eq!(given_up.misses, TRIES_BEFORE_GIVING_UP);
    assert!(given_up.gave_up());
    assert_eq!(given_up.due_at(), None);
    assert_eq!(library.next_want_due()?, None);

    assert_eq!(wanted_san_tropez(&library)?, want);

    let asked_again = library.wants()?[0].clone();

    assert_eq!(asked_again.misses, 0);
    assert!(!asked_again.gave_up());
    assert!(library.is_a_want_due(PollOptions::default())?);
    Ok(())
}

#[test]
fn a_want_one_provider_answered_and_another_refused_stays_due() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_san_tropez(&library)?;

    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let server = Unanswering::new("server", wrong_password);

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&quiet) as Arc<dyn Provider>)
            .and(Arc::clone(&server) as Arc<dyn Provider>),
    );

    library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(quiet.asked().len(), 1);
    assert_eq!(library.wants()?[0].tried, None);
    Ok(())
}

#[test]
fn a_provider_that_cannot_be_reached_is_asked_once_a_poll_rather_than_once_a_want() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_every_missing_row(&library)?;

    let wanted = library.wants()?.len();

    assert!(
        wanted > 1,
        "the poll is to be asked about more than one want"
    );

    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let server = Unanswering::new("server", unreachable_server);

    let providers = Arc::new(
        Providers::none()
            .and(Arc::clone(&server) as Arc<dyn Provider>)
            .and(Arc::clone(&quiet) as Arc<dyn Provider>),
    );

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.asked, wanted as u64);
    assert_eq!(server.asked(), 1);
    assert_eq!(quiet.asked().len(), wanted);
    assert!(library.wants()?.iter().all(|want| want.tried.is_none()));
    Ok(())
}

#[test]
fn a_wrong_password_is_tried_once_a_poll_rather_than_once_a_want() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_every_missing_row(&library)?;

    let wanted = library.wants()?.len();

    assert!(wanted > 1);

    let server = Unanswering::new("server", wrong_password);
    let providers = Arc::new(Providers::none().and(Arc::clone(&server) as Arc<dyn Provider>));

    library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(
        server.asked(),
        1,
        "a refused login was tried for every want"
    );
    assert!(library.wants()?.iter().all(|want| want.tried.is_none()));
    Ok(())
}

struct Slow {
    source: SourceId,
    in_flight: AtomicUsize,
    most_at_once: AtomicUsize,
    held_for: Duration,
}

impl Slow {
    fn new(held_for: Duration) -> Arc<Self> {
        Arc::new(Self {
            source: SourceId::new("slow").expect("a nameable source"),
            in_flight: AtomicUsize::new(0),
            most_at_once: AtomicUsize::new(0),
            held_for,
        })
    }

    fn most_at_once(&self) -> usize {
        self.most_at_once.load(Ordering::SeqCst)
    }
}

impl Provider for Slow {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, _identity: &Identity) -> ProvidedResult<Obtained> {
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;

        self.most_at_once.fetch_max(now, Ordering::SeqCst);
        thread::sleep(self.held_for);
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(Obtained::Nothing)
    }
}

#[test]
fn wants_are_asked_about_side_by_side_once_the_first_has_been_answered() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_every_missing_row(&library)?;

    let wanted = library.wants()?.len();

    assert!(wanted > 2);

    let slow = Slow::new(Duration::from_millis(150));
    let providers = Arc::new(Providers::none().and(Arc::clone(&slow) as Arc<dyn Provider>));

    let summary = library.poll(providers, PollOptions::default())?.join()?;

    assert_eq!(summary.stats.asked, wanted as u64);
    assert_eq!(summary.stats.nothing, wanted as u64);
    assert!(
        slow.most_at_once() > 1,
        "the wants were asked about one after another"
    );
    assert!(slow.most_at_once() <= PollOptions::default().lanes.get());
    Ok(())
}

#[test]
fn a_poll_with_one_lane_asks_about_one_want_at_a_time() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_every_missing_row(&library)?;

    let slow = Slow::new(Duration::from_millis(20));
    let providers = Arc::new(Providers::none().and(Arc::clone(&slow) as Arc<dyn Provider>));

    library
        .poll(
            providers,
            PollOptions {
                lanes: NonZeroUsize::MIN,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert_eq!(slow.most_at_once(), 1);
    Ok(())
}

struct WantsOneMore {
    source: SourceId,
    library: Arc<Library>,
    progress: Mutex<Option<Arc<PollProgress>>>,
    pressed: AtomicBool,
    asked: Mutex<Vec<String>>,
}

impl Provider for WantsOneMore {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.asked.lock().push(identity.title.clone());
        if !self.pressed.swap(true, Ordering::SeqCst) {
            let album = only_album(&self.library).expect("one album");
            let late = self
                .library
                .release_tracks(album.id)
                .expect("the release rows")
                .into_iter()
                .find(|row| row.title == "Seamus")
                .expect("a row nobody wanted yet");

            self.library.want(late.id).expect("a want");

            let progress = self.progress.lock().clone();
            let progress = progress.expect("the poll's progress");

            assert!(progress.wants_changed(), "the poll had already closed");
        }
        thread::sleep(Duration::from_millis(30));
        Ok(Obtained::Nothing)
    }
}

#[test]
fn a_want_pressed_while_a_poll_runs_is_asked_by_that_poll_and_not_the_next() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let library = Arc::new(library);
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));
    rows.push(release_row(5, "Seamus", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    let first = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("a row to want first");

    library.want(first.id)?;

    let provider = Arc::new(WantsOneMore {
        source: SourceId::new("shop").expect("a nameable source"),
        library: Arc::clone(&library),
        progress: Mutex::new(None),
        pressed: AtomicBool::new(false),
        asked: Mutex::new(Vec::new()),
    });
    let handle = library.poll(
        Arc::new(Providers::none().and(Arc::clone(&provider) as Arc<dyn Provider>)),
        PollOptions::default(),
    )?;

    *provider.progress.lock() = Some(Arc::clone(handle.progress()));

    let summary = handle.join()?;

    assert_eq!(summary.stats.asked, 2);
    assert_eq!(*provider.asked.lock(), ["San Tropez", "Seamus"]);
    Ok(())
}

#[test]
fn a_poll_that_has_finished_is_not_nudged() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    wanted_san_tropez(&library)?;

    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));

    let handle = library.poll(quiet.registered(), PollOptions::default())?;
    let progress = Arc::clone(handle.progress());

    handle.join()?;

    assert!(!progress.wants_changed());
    Ok(())
}
