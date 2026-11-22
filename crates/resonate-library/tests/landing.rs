use std::{
    fs,
    num::NonZeroUsize,
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use resonate_library::{
    AlbumQuery, Certainty, CoverArt, CoverSource, Credit, Error, Favoured, GroupAsked,
    GroupRelease, HeldMedium, ImageFormat, Issued, Library, Link, LookupOp, Medium, Recording,
    RecordingRelease, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, Result,
    TagField, TrackQuery, Wording,
};

use crate::support::{
    ADA, ALBUM, ARTIST, COMPILATION, Called, Canned, Fake, HOURS, MUSICBRAINZ_ALBUM,
    MUSICBRAINZ_RELEASE_GROUP, ORBITERS, ORBITS_TITLES, RECORDING, RELEASE, RELEASE_GROUP, TITLE,
    Tree, Wav, a_real_picture, album_titled, all, artist_named, asked_in_order, beside, credited,
    enrich, fields_of, group_release, hours, keys, mbid, only_album, opened_with_a_vault, options,
    orbiters, orbits, orbits_asked, orbits_asked_in_words, orbits_group, orbits_group_asked,
    orbits_group_match, orbits_match, orbits_rows, orbits_track, png, png_art, recorded,
    release_row, retagged, scan, scanned_orbits, scanned_orbits_on_disk, stored, write_hours,
    write_orbits,
};

fn orbits_group_asked_in_words() -> GroupAsked {
    GroupAsked {
        wording: Wording::Words,
        ..orbits_group_asked()
    }
}

#[test]
fn an_album_tagged_with_a_release_id_is_asked_for_by_that_id_and_never_searched() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    assert_eq!(fake.called(LookupOp::FindRelease), 0);
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(fake.calls()[0], Called::Release(mbid(RELEASE)));
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.groups, 0);
    assert_eq!(summary.stats.albums_unmatched, 0);
    assert_eq!(summary.stats.albums_rejected, 0);
    assert_eq!(only_album(&library)?.missing, 0);
    Ok(())
}

#[test]
fn a_release_the_reference_answers_fills_the_album_and_lists_every_track_it_names() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(
            rows,
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        )],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(ReleaseAsked {
                title: "Orbits".to_owned(),
                artist: Some("The Orbiters".to_owned()),
                artist_mbid: None,
                barcode: None,
                catalog_number: None,
                wording: Wording::Phrase,
            }),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(
        fake.called(LookupOp::Cover),
        1,
        "the picture the release names was not fetched beside the pass"
    );

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 1);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert!(release.asked.is_some() && release.answered.is_some());
    assert_eq!(release.links.len(), 1);

    let titles: Vec<String> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| row.title)
        .collect();

    assert_eq!(
        titles,
        vec![
            "One of These Days",
            "A Pillow of Winds",
            "Fearless",
            "San Tropez"
        ]
    );
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(summary.stats.artists, 1);
    assert_eq!(summary.stats.refused, 0);
    Ok(())
}

const DISC_TITLES: [[&str; 2]; 2] = [
    ["One of These Days", "A Pillow of Winds"],
    ["Fearless", "San Tropez"],
];

fn write_orbits_as_two_discs(tree: &Tree) {
    for (index, titles) in DISC_TITLES.into_iter().enumerate() {
        let disc = index + 1;
        for (index, title) in titles.into_iter().enumerate() {
            let number = index as u32 + 1;
            let track = orbits_track(title, number)
                .text(ALBUM, &format!("Orbits [Disc {disc}]"))
                .described(MUSICBRAINZ_ALBUM, RELEASE);

            tree.write(&format!("Orbits/CD{disc}/{number}.wav"), &track.build());
        }
    }
}

fn orbits_on_two_discs() -> Release {
    let disc = |index: usize, title: Option<&str>| Medium {
        position: index as u32 + 1,
        format: Some("CD".to_owned()),
        title: title.map(str::to_owned),
        tracks: DISC_TITLES[index]
            .into_iter()
            .enumerate()
            .map(|(at, named)| release_row(at as u32 + 1, named, Vec::new()))
            .collect(),
    };

    Release {
        media: vec![disc(0, Some("The Early Orbit")), disc(1, None)],
        ..orbits(Vec::new(), Vec::new())
    }
}

#[test]
fn a_set_of_two_discs_is_billed_by_the_release_and_keeps_a_medium_of_its_own_for_each_disc()
-> Result<()> {
    let tree = Tree::new();

    write_orbits_as_two_discs(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let scanned = only_album(&library)?.title;

    assert!(
        DISC_TITLES
            .iter()
            .enumerate()
            .any(|(index, _)| scanned == format!("Orbits [Disc {}]", index + 1)),
        "the scan bills the set by whichever disc it read first, and got {scanned}"
    );

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits_on_two_discs()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;

    assert_eq!(album.title, "Orbits");

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(
        release.media,
        vec![
            HeldMedium {
                position: 1,
                format: Some("CD".to_owned()),
                title: Some("The Early Orbit".to_owned()),
            },
            HeldMedium {
                position: 2,
                format: Some("CD".to_owned()),
                title: None,
            },
        ]
    );

    let placed: Vec<(u32, u32, String)> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| (row.disc, row.position, row.title))
        .collect();

    assert_eq!(
        placed,
        vec![
            (1, 1, "One of These Days".to_owned()),
            (1, 2, "A Pillow of Winds".to_owned()),
            (2, 1, "Fearless".to_owned()),
            (2, 2, "San Tropez".to_owned()),
        ]
    );

    scan(&library, &options(&tree))?;

    assert_eq!(
        only_album(&library)?.title,
        "Orbits",
        "a rescan read the set back under the disc tag the files carry"
    );
    Ok(())
}

#[test]
fn a_match_the_listener_forgets_is_taken_away_and_never_landed_again() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(
            rows,
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));

    assert!(library.forget_the_match(album.id)?);

    let forgotten = only_album(&library)?;

    assert_eq!(forgotten.mbid, None);
    assert_eq!(forgotten.missing, 0);
    assert!(library.release_tracks(album.id)?.is_empty());

    let release = library.release_of(album.id)?.expect("the album is known");

    assert!(release.links.is_empty());
    assert!(
        release.asked.is_none(),
        "the album is not due to be asked again"
    );
    assert!(library.refuses(album.id, &mbid(RELEASE))?);

    let again = enrich(&library, &fake, false)?;

    assert_eq!(again.stats.albums_rejected, 1);
    assert_eq!(again.stats.albums_unmatched, 0);
    assert_eq!(again.stats.releases, 0);
    assert_eq!(
        only_album(&library)?.mbid,
        None,
        "the release the listener refused was landed again"
    );
    assert!(library.release_tracks(album.id)?.is_empty());
    assert!(
        !library.forget_the_match(album.id)?,
        "an album matched to nothing forgot a match"
    );
    Ok(())
}

#[test]
fn forgetting_a_match_puts_back_the_names_the_files_gave_and_asks_about_the_tracks_again()
-> Result<()> {
    let (_tree, library, database) = scanned_orbits_on_disk()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;
    let track = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .next()
        .expect("a scanned track");
    let file = track
        .location
        .as_path()
        .expect("a local file")
        .to_path_buf();
    let tagged = stored(&database, &file);

    library.land_recording(
        track.id,
        &Recording {
            id: mbid("5f6a7b8c-9d0e-4f1a-8b2c-3d4e5f6a7b8c"),
            title: "Renamed By The Match".to_owned(),
            credit: Vec::new(),
            length: None,
            isrcs: Vec::new(),
            releases: Vec::new(),
        },
        Certainty::Exactly,
        None,
    )?;

    assert_eq!(stored(&database, &file).title, "Renamed By The Match");

    assert!(library.forget_the_match(album.id)?);

    let forgotten = stored(&database, &file);

    assert_eq!(
        Some(forgotten.title.as_str()),
        tagged.tagged_title.as_deref(),
        "the name the match wrote was left standing"
    );
    assert_eq!(forgotten.answered, None);
    assert_eq!(forgotten.asked, None);
    assert!(
        library.search("renamed", 10)?.tracks.is_empty(),
        "the index still finds the name the match wrote"
    );
    Ok(())
}

#[test]
fn a_near_match_credited_to_another_artist_leaves_the_track_filed_under_its_own() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let track = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .next()
        .expect("a scanned track");
    let own = track.artist_id;

    library.land_recording(
        track.id,
        &Recording {
            id: mbid(RECORDING),
            title: track.title.clone(),
            credit: credited(Some("Someone Else"), None),
            length: None,
            isrcs: Vec::new(),
            releases: Vec::new(),
        },
        Certainty::Nearly,
        None,
    )?;

    let landed = library.track(track.id)?.expect("the track is held");

    assert_eq!(landed.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(
        landed.artist_id, own,
        "the row reads one artist and is listed under another"
    );
    Ok(())
}

#[test]
fn forgetting_a_match_takes_away_the_recording_ids_and_codes_it_stamped() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let mut rows = orbits_rows();

    rows[0].recording = Some(mbid(RECORDING));
    rows[0].isrcs = vec!["GBN9Y1100089".to_owned()];

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let file = tree.path().join("1.wav");

    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    assert_eq!(
        stored(&database, &file).isrc.as_deref(),
        Some("GBN9Y1100089")
    );

    assert!(library.forget_the_match(only_album(&library)?.id)?);

    assert_eq!(
        recorded(&database, &file),
        None,
        "the refused match's recording id was kept to be taken as tagged"
    );
    assert_eq!(stored(&database, &file).isrc, None);
    Ok(())
}

#[test]
fn a_pressing_the_listener_chooses_is_landed_in_place_of_the_one_the_lookup_took() -> Result<()> {
    const THE_JAPANESE_PRESSING: &str = "2d3e4f5a-6b7c-4d8e-9f0a-1b2c3d4e5f6a";

    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Bonus Track", Vec::new()));

    let japanese = Release {
        id: mbid(THE_JAPANESE_PRESSING),
        country: Some("JP".to_owned()),
        ..orbits(rows, Vec::new())
    };
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new()), japanese],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));

    assert!(library.take_pressing(album.id, fake.as_ref(), &mbid(THE_JAPANESE_PRESSING))?);

    let taken = only_album(&library)?;

    assert_eq!(taken.mbid, Some(mbid(THE_JAPANESE_PRESSING)));
    assert_eq!(
        taken.missing, 1,
        "the bonus track the pressing adds is not listed as missing"
    );

    assert!(library.forget_the_match(album.id)?);
    assert!(library.refuses(album.id, &mbid(THE_JAPANESE_PRESSING))?);
    assert!(library.take_pressing(album.id, fake.as_ref(), &mbid(THE_JAPANESE_PRESSING))?);
    assert!(
        !library.refuses(album.id, &mbid(THE_JAPANESE_PRESSING))?,
        "a pressing chosen by hand stayed refused"
    );
    assert_eq!(
        only_album(&library)?.mbid,
        Some(mbid(THE_JAPANESE_PRESSING))
    );
    Ok(())
}

#[test]
fn a_held_track_is_placed_on_the_release_the_listener_chooses_rather_than_the_one_a_rule_would()
-> Result<()> {
    const THE_JAPANESE_PRESSING: &str = "2d3e4f5a-6b7c-4d8e-9f0a-1b2c3d4e5f6a";
    const THE_RECORDING: &str = "6a7b8c9d-0e1f-4a2b-8c3d-4e5f6a7b8c9d";

    let (_tree, library) = scanned_orbits()?;
    let japanese = Release {
        id: mbid(THE_JAPANESE_PRESSING),
        title: "Orbits (Japan)".to_owned(),
        country: Some("JP".to_owned()),
        ..orbits(orbits_rows(), Vec::new())
    };
    let on = |id: &str, title: &str, kind: Option<&str>| RecordingRelease {
        id: mbid(id),
        title: title.to_owned(),
        date: Some("1971-10-30".to_owned()),
        disc: Some(1),
        position: Some(1),
        issued: Issued {
            kind: kind.map(str::to_owned),
            secondary: Vec::new(),
            status: Some("Official".to_owned()),
        },
    };
    let recording = Recording {
        id: mbid(THE_RECORDING),
        title: "Launch".to_owned(),
        credit: Vec::new(),
        length: None,
        isrcs: Vec::new(),
        releases: vec![
            on(THE_JAPANESE_PRESSING, "Orbits (Japan)", Some("Single")),
            on(RELEASE, "Orbits", Some("Album")),
        ],
    };

    let fake = Fake::new(Canned {
        recordings: vec![recording.clone()],
        releases: vec![orbits(orbits_rows(), Vec::new()), japanese],
        ..Canned::default()
    });
    let track = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .next()
        .expect("a scanned track");

    assert!(
        library.recordings_of(track.id, &fake)?.is_empty(),
        "a track nothing identified offered releases"
    );

    library.land_recording(track.id, &recording, Certainty::Nearly, None)?;

    let offered = library
        .recordings_of(track.id, &fake)?
        .into_iter()
        .next()
        .expect("the recording the track is identified as");
    let order: Vec<&str> = resonate_library::in_the_order_worth_offering(&offered.releases)
        .into_iter()
        .map(|release| release.title.as_str())
        .collect();

    assert_eq!(order, ["Orbits", "Orbits (Japan)"]);

    assert!(library.place_on(track.id, &fake, &offered, &mbid(THE_JAPANESE_PRESSING))?);
    assert_eq!(
        only_album(&library)?.mbid,
        Some(mbid(THE_JAPANESE_PRESSING)),
        "the album was not placed on the release chosen"
    );
    assert!(
        !library.place_on(track.id, &fake, &offered, &mbid(ADA))?,
        "a release the recording is not on was taken"
    );
    Ok(())
}

#[test]
fn a_search_match_is_taken_only_under_the_strict_rule_and_a_near_miss_stamps_asked_alone()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        fake.called(LookupOp::FindRelease),
        2,
        "a near miss was not asked again in words"
    );
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.albums_unmatched, 1);
    assert_eq!(summary.stats.releases, 0);
    assert_eq!(summary.stats.refused, 0);
    assert_eq!(fake.called(LookupOp::Release), 0);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, None);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert!(release.asked.is_some());
    assert_eq!(release.answered, None);

    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("the orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));

    let tree = Tree::new();

    for (file, title, artist) in [
        ("dawn.wav", "Dawn", "Ada"),
        ("noon.wav", "Noon", "Ben"),
        ("dusk.wav", "Dusk", "Cleo"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(album.artist_id, None);

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            release: mbid(HOURS),
            group: None,
            score: 95,
            title: "Hours".to_owned(),
            credit: credited(Some("Various Artists"), None),
            track_count: Some(3),
            date: None,
        }],
        releases: vec![hours()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(HOURS)));
    Ok(())
}

#[test]
fn an_album_naming_no_owner_is_not_taken_for_a_release_by_somebody_none_of_its_tracks_bill()
-> Result<()> {
    let tree = Tree::new();

    for (file, title, artist) in [
        ("dawn.wav", "Dawn", "Ada"),
        ("noon.wav", "Noon", "Ben"),
        ("dusk.wav", "Dusk", "Cleo"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let hit = |artist| ReleaseMatch {
        release: mbid(HOURS),
        group: None,
        score: 100,
        title: "Hours".to_owned(),
        credit: credited(Some(artist), None),
        track_count: Some(3),
        date: None,
    };
    let elsewhere = Arc::new(Fake::new(Canned {
        found_releases: vec![hit("The Orbiters")],
        releases: vec![hours()],
        ..Canned::default()
    }));
    let performed = Arc::new(Fake::new(Canned {
        found_releases: vec![hit("Ben")],
        releases: vec![hours()],
        ..Canned::default()
    }));

    enrich(&library, &elsewhere, false)?;

    assert_eq!(elsewhere.called(LookupOp::Release), 0);
    assert_eq!(only_album(&library)?.mbid, None);

    enrich(&library, &performed, true)?;

    assert_eq!(performed.called(LookupOp::Release), 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(HOURS)));
    Ok(())
}

#[test]
fn a_phrase_that_answers_nothing_is_asked_again_in_words_and_lands_under_the_strict_rule()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases_in_words: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));

    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_groups_in_words: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::FindReleaseGroup(orbits_group_asked_in_words()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));
    Ok(())
}

fn orbits_in_two_folders() -> Tree {
    let tree = Tree::new();

    for (folder, frames) in [("ripped", 4_410), ("ripped again", 44_100 * 4)] {
        for (file, title) in [
            ("1.wav", "One of These Days"),
            ("2.wav", "A Pillow of Winds"),
            ("3.wav", "Fearless!"),
        ] {
            tree.write(
                &format!("{folder}/{file}"),
                &Wav::new()
                    .frames(frames)
                    .text(TITLE, title)
                    .text(ARTIST, "The Orbiters")
                    .text(ALBUM, "Orbits")
                    .build(),
            );
        }
    }

    tree
}

#[test]
fn a_rescan_writes_no_row_whose_file_it_found_unchanged_and_still_prunes_the_one_gone() -> Result<()>
{
    let tree = Tree::new();
    let tone = Wav::new().frames(16).build();

    for nth in 0..3 {
        tree.write(&format!("{nth}.wav"), &tone);
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    beside(&database)
        .execute_batch(
            "CREATE TABLE written (id INTEGER NOT NULL);
             CREATE TRIGGER note_a_write AFTER UPDATE ON tracks
             BEGIN INSERT INTO written (id) VALUES (new.id); END;",
        )
        .expect("a trigger noting every row written");

    fs::remove_file(tree.path().join("2.wav")).expect("the file is taken away");

    let summary = scan(&library, &options(&tree))?;

    let written: i64 = beside(&database)
        .query_row("SELECT count(DISTINCT id) FROM written", [], |row| {
            row.get(0)
        })
        .expect("the notes read back");

    assert_eq!(summary.removed, 1);
    assert_eq!(all(&library)?.len(), 2);
    assert_eq!(written, 1, "a row whose file was unchanged was written");
    Ok(())
}

#[test]
fn a_scan_whose_write_fails_answers_the_failure_and_hands_the_tree_back() -> Result<()> {
    const FILES: usize = 2_400;
    const WAITS_AT_MOST: Duration = Duration::from_secs(60);

    let tree = Tree::new();
    let tone = Wav::new().frames(16).build();

    for nth in 0..FILES {
        tree.write(&format!("{nth:04}.wav"), &tone);
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    let mut wide = options(&tree);

    wide.workers = NonZeroUsize::new(4).expect("four workers");
    scan(&library, &wide)?;

    beside(&database)
        .execute_batch(
            "UPDATE tracks SET probe_again = 1;
             CREATE TRIGGER refuse_a_write BEFORE UPDATE OF seen ON tracks
             BEGIN SELECT RAISE(ABORT, 'the disc is full'); END;",
        )
        .expect("a trigger that refuses every write");

    let handle = library.scan(wide.clone())?;
    let (told, heard) = mpsc::channel();

    thread::spawn(move || {
        let _ = told.send(handle.join());
    });

    let answered = heard
        .recv_timeout(WAITS_AT_MOST)
        .expect("a scan whose write failed never finished");

    assert!(matches!(answered, Err(Error::Store { .. })));

    beside(&database)
        .execute_batch("DROP TRIGGER refuse_a_write")
        .expect("the trigger taken away");
    scan(&library, &wide)?;
    Ok(())
}

#[test]
fn two_albums_that_turn_out_to_share_a_release_are_gathered_into_one() -> Result<()> {
    let tree = orbits_in_two_folders();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert_eq!(
        library.albums(&AlbumQuery::default())?.len(),
        2,
        "the two folders were not scanned as two albums to gather"
    );

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(
        album.track_count, 6,
        "the gathered album does not hold both folders' tracks"
    );
    assert_eq!(all(&library)?.len(), 6, "a track was lost to the gathering");
    assert_eq!(
        keys(&database).len(),
        3,
        "the album is not named by both folders' keys and the release's own"
    );

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(
        album.mbid,
        Some(mbid(RELEASE)),
        "a rescan split the gathered album and lost what the release wrote"
    );
    assert_eq!(album.track_count, 6);
    Ok(())
}

#[test]
fn a_gathered_album_brings_its_favourite_and_its_vaulted_cover_with_it() -> Result<()> {
    for marked in 0..2 {
        let tree = orbits_in_two_folders();
        let held = Tree::new();
        let (library, vault) = opened_with_a_vault(&held)?;

        scan(&library, &options(&tree))?;

        let albums = library.albums(&AlbumQuery::default())?;

        assert_eq!(albums.len(), 2);

        let chosen = albums[marked].id;

        library.favour(Favoured::Album(chosen), true)?;

        assert!(library.land_archive_cover(chosen, &a_real_picture(12, 12))?);
        assert!(library.albums(&AlbumQuery::default())?[marked].has_cover_art);

        let fake = Arc::new(Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        }));

        enrich(&library, &fake, false)?;

        let album = only_album(&library)?;

        assert!(
            album.favourite.is_some(),
            "the favourite was lost gathering album {marked} into the other"
        );
        assert!(
            album.has_cover_art,
            "the vaulted cover of album {marked} was lost"
        );
        assert!(library.cover_art(album.id)?.is_some());
        assert_eq!(library.prune_the_vault()?.covers, 0);
        assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    }
    Ok(())
}

#[test]
fn a_phrase_that_answers_a_near_miss_is_asked_again_in_words_and_lands_there() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_releases_in_words: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        found_groups: vec![orbits_group_match(100, Some("Someone Else"))],
        found_groups_in_words: vec![orbits_group_match(100, Some("The Orbiters"))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let searches: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| {
            matches!(
                called.op(),
                LookupOp::FindRelease | LookupOp::FindReleaseGroup
            )
        })
        .collect();

    assert_eq!(
        searches,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
        ],
        "a phrase that landed nothing was not asked again in words, or the words were \
         not enough to spare the group search"
    );

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));

    let release = library.release_of(album.id)?.expect("the album is known");

    assert!(release.answered.is_some());
    Ok(())
}

#[test]
fn an_album_no_release_matches_is_landed_as_its_release_group_with_a_cover() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(HOURS, "1996-05-06", None)],
            vec![Link::new(
                "free streaming",
                "https://example.invalid/orbits".to_owned(),
            )],
        )],
        group_covers: vec![(mbid(RELEASE_GROUP), png_art(64))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);

    let of_the_album: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| called.op() != LookupOp::FindRecording)
        .collect();

    assert_eq!(
        of_the_album,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::Cover), 1);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, None);
    assert_eq!(album.year, Some(1971));
    assert!(album.has_cover_art);
    assert!(library.release_tracks(album.id)?.is_empty());

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.mbid, None);
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.kind.as_deref(), Some("Album"));
    assert_eq!(release.date.as_deref(), Some("1971-10-30"));
    assert_eq!(release.disambiguation.as_deref(), Some("the original run"));
    assert!(release.asked.is_some() && release.answered.is_some());
    assert_eq!(release.cover_source, CoverSource::Archive);
    assert_eq!(
        release.links,
        vec![Link::new(
            "free streaming",
            "https://example.invalid/orbits".to_owned()
        )]
    );

    assert_eq!(summary.stats.releases, 0);
    assert_eq!(summary.stats.groups, 1);
    assert_eq!(summary.stats.albums_unmatched, 0);
    assert_eq!(summary.stats.matched, 0);
    assert_eq!(summary.stats.covers, 1);
    assert_eq!(summary.stats.covers_missing, 0);
    assert_eq!(summary.stats.refused, 0);
    Ok(())
}

#[test]
fn a_group_whose_release_matches_the_album_lands_that_release_and_its_rows() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(5)),
                group_release(RELEASE, "1971-10-30", Some(3)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::Cover), 1);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(library.release_tracks(album.id)?.len(), 3);
    assert_eq!(album.missing, 0);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert_eq!(release.date.as_deref(), Some("1998-03-02"));
    assert!(release.answered.is_some());

    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.covers, 1);
    Ok(())
}

#[test]
fn an_album_wider_than_every_pressing_of_its_group_lands_the_widest_one() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let two_of_the_three: Vec<ReleaseTrack> = orbits_rows().into_iter().take(2).collect();
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(1)),
                group_release(RELEASE, "1971-10-30", Some(2)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(two_of_the_three, Vec::new())],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    let of_the_album: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| called.op() != LookupOp::FindRecording)
        .collect();

    assert_eq!(
        of_the_album,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(library.release_tracks(album.id)?.len(), 2);
    assert_eq!(album.missing, 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 2);

    let landed = retagged(&library, false)?;

    enrich(&library, &fake, true)?;

    let landed_again = retagged(&library, false)?;

    for preview in [&landed, &landed_again] {
        assert!(!preview.retagging.writes.is_empty());
        for write in &preview.retagging.writes {
            let fields = fields_of(write);
            for withheld in [TagField::TrackTotal, TagField::MusicBrainzAlbumId] {
                assert!(
                    !fields.contains(&withheld),
                    "the closest pressing was written as a match: {fields:?}"
                );
            }
        }
    }
    Ok(())
}

fn five_track_pressing_rows() -> Vec<ReleaseTrack> {
    orbits_rows()
        .into_iter()
        .chain([
            release_row(4, "Careful with That Axe", Vec::new()),
            release_row(5, "Set the Controls", Vec::new()),
        ])
        .collect()
}

fn group_of_pressings_laid_out(layouts: [&[u32]; 2]) -> ReleaseGroup {
    let [earlier, later] = layouts;
    orbits_group(
        vec![
            GroupRelease {
                discs: later.to_vec(),
                ..group_release(HOURS, "1996-05-06", Some(later.iter().sum()))
            },
            GroupRelease {
                discs: earlier.to_vec(),
                ..group_release(RELEASE, "1971-10-30", Some(earlier.iter().sum()))
            },
        ],
        Vec::new(),
    )
}

#[test]
fn an_album_every_pressing_of_its_group_counts_alike_is_written_the_totals_but_not_the_pressing()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![group_of_pressings_laid_out([&[5], &[5]])],
        releases: vec![orbits(five_track_pressing_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let landed = retagged(&library, false)?;

    enrich(&library, &fake, true)?;

    let landed_again = retagged(&library, false)?;

    for preview in [&landed, &landed_again] {
        assert!(!preview.retagging.writes.is_empty());
        for write in &preview.retagging.writes {
            let fields = fields_of(write);

            for written in [TagField::TrackTotal, TagField::DiscTotal] {
                assert!(
                    fields.contains(&written),
                    "the count every pressing agrees on was withheld: {fields:?}"
                );
            }
            for withheld in [
                TagField::MusicBrainzAlbumId,
                TagField::CatalogNumber,
                TagField::Barcode,
            ] {
                assert!(
                    !fields.contains(&withheld),
                    "the closest pressing was written as a match: {fields:?}"
                );
            }

            let total = write
                .edits
                .iter()
                .find(|edit| edit.field == TagField::TrackTotal)
                .map(|edit| edit.value.as_str());

            assert_eq!(total, Some("5"));
        }
    }
    Ok(())
}

#[test]
fn an_album_the_pressings_of_its_group_count_apart_is_written_no_totals() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![group_of_pressings_laid_out([&[5], &[3, 2]])],
        releases: vec![orbits(five_track_pressing_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let preview = retagged(&library, false)?;

    assert!(!preview.retagging.writes.is_empty());

    for write in &preview.retagging.writes {
        let fields = fields_of(write);
        for withheld in [TagField::TrackTotal, TagField::DiscTotal] {
            assert!(
                !fields.contains(&withheld),
                "a count the pressings disagree on was written: {fields:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn an_album_tagged_with_a_release_group_id_is_asked_for_that_group_and_never_searched_for_one()
-> Result<()> {
    let tree = Tree::new();

    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;

        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_RELEASE_GROUP, RELEASE_GROUP)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(Vec::new(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindReleaseGroup), 0);
    assert_eq!(fake.called(LookupOp::ReleaseGroup), 1);
    assert_eq!(fake.called(LookupOp::Release), 0);
    assert_eq!(
        fake.calls()[..3],
        [
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
        ]
    );

    let album = only_album(&library)?;

    assert_eq!(album.mbid, None);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.kind.as_deref(), Some("Album"));
    assert!(release.answered.is_some());
    Ok(())
}

#[test]
fn an_album_asked_within_the_retry_window_is_not_asked_again_and_refresh_asks_anyway() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRelease), 2);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);

    let album = only_album(&library)?;

    assert!(
        library
            .release_of(album.id)?
            .is_some_and(|release| release.asked.is_some())
    );

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRelease), 2);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);
    assert_eq!(summary.stats.albums, 0);
    assert_eq!(summary.stats.artists, 0);

    let summary = enrich(&library, &fake, true)?;

    assert_eq!(fake.called(LookupOp::FindRelease), 4);
    assert_eq!(fake.called(LookupOp::FindArtist), 2);
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.artists, 1);
    Ok(())
}

#[test]
fn a_cover_the_archive_holds_lands_only_where_the_files_embedded_none() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let embedded = png(64);

    write_hours(&tree, Some(&embedded));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        covers: vec![(mbid(RELEASE), png_art(256)), (mbid(HOURS), png_art(128))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let covers: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Cover)
        .collect();

    assert_eq!(
        covers,
        vec![Called::Cover(mbid(RELEASE), Some(mbid(RELEASE_GROUP)))]
    );
    assert_eq!(summary.stats.covers, 1);

    let orbits = album_titled(&library, "Orbits")?;

    assert_eq!(library.cover_art(orbits.id)?, Some(png_art(256)));
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.cover_source),
        Some(CoverSource::Archive)
    );

    let hours = album_titled(&library, "Hours")?;

    assert_eq!(
        library.cover_art(hours.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: embedded,
        })
    );
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.cover_source),
        Some(CoverSource::File)
    );
    Ok(())
}

#[test]
fn an_artist_named_in_a_release_credit_is_looked_up_by_the_credit_id_and_never_searched()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
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

    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");

    assert!(detail.answered.is_some());
    assert_eq!(detail.sort_name.as_deref(), Some("Orbiters, The"));
    Ok(())
}
