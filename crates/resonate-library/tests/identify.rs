use std::{fs, path::Path, sync::Arc};

use resonate_library::{
    AlbumQuery, Library, LookupOp, Mbid, RecordingAsked, RecordingMatch, ReleaseAsked,
    ReleaseMatch, ReleaseTrack, Result, ScanOptions, Wording,
};

use crate::support::{
    ADA, ALBUM, ALBUM_ARTIST, ANOTHER_CODE, ANOTHER_RECORDING, ARTIST, CODE, Called, Canned, Fake,
    Fault, HOURS, ISRC, MUSICBRAINZ, MUSICBRAINZ_ALBUM, MUSICBRAINZ_ARTIST,
    MUSICBRAINZ_RELEASE_GROUP, ORBITERS, ORBITS_TITLES, RECORDING, RELEASE, RELEASE_GROUP,
    RELEASE_TRACK, THE_FILES_LENGTH, TITLE, TRACK, Tree, Wav, all, artist_match, artist_named,
    asked_in_order, beside, credited, enrich, group_release, isrc, mbid, on_orbits, only_album,
    options, orbiters, orbits, orbits_asked, orbits_asked_in_words, orbits_group,
    orbits_group_asked, orbits_group_match, orbits_match, orbits_recording, orbits_rows,
    orbits_track, recorded, recording_match, release_row, scan, scanned_lone, scanned_orbits,
    scanned_orbits_on_disk, settled, stored, titles, write_orbits,
};

#[test]
fn an_isrc_that_names_no_release_is_followed_up_by_asking_where_the_recording_sits() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;

    let bare = orbits_recording(RECORDING, "One of These Days", CODE, Vec::new());
    let whole = orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(2));

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(isrc(CODE), bare)],
        recordings: vec![whole],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        1,
        "an ISRC answering no release was not followed up"
    );
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);

    assert_eq!(
        held.release_title.as_deref(),
        Some("Orbits"),
        "the release the follow-up named was not written"
    );
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let row = library
        .track_at(&file, None)?
        .expect("the catalog holds the track");

    assert_eq!(
        row.track_number,
        Some(2),
        "the place the follow-up named was not written"
    );
    assert_eq!(row.disc_number, Some(1));
    Ok(())
}

#[test]
fn a_track_with_no_recording_id_is_placed_through_its_isrc_and_else_through_a_strict_search()
-> Result<()> {
    let (_tree, coded, _, coded_file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;
    let (_other_tree, named, _, named_file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;

    let bare = orbits_recording(RECORDING, "One of These Days", CODE, Vec::new());
    let whole = orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(2));

    let fake = Fake::new(Canned {
        isrcs: vec![(isrc(CODE), bare)],
        recordings: vec![whole],
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        ..Canned::default()
    });
    let coded_track = coded
        .track_at(&coded_file, None)?
        .expect("the catalog holds the coded track");
    let named_track = named
        .track_at(&named_file, None)?
        .expect("the catalog holds the named track");

    let by_code = coded.recordings_of(coded_track.id, &fake)?;

    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);

    let [offered] = by_code.as_slice() else {
        panic!("the ISRC offered {} recordings, not one", by_code.len());
    };

    assert_eq!(offered.id, mbid(RECORDING));
    assert_eq!(
        offered.releases.len(),
        1,
        "the recording the ISRC named was not told where it sits"
    );
    assert!(coded.place_on(coded_track.id, &fake, offered, &mbid(RELEASE))?);
    assert_eq!(
        coded
            .track_at(&coded_file, None)?
            .expect("the catalog still holds the coded track")
            .track_number,
        Some(2),
        "the release chosen was not landed on the track"
    );

    let by_name = named.recordings_of(named_track.id, &fake)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);

    let [searched] = by_name.as_slice() else {
        panic!("the search offered {} recordings, not one", by_name.len());
    };

    assert_eq!(searched.id, mbid(RECORDING));
    assert_eq!(searched.releases.len(), 1);
    Ok(())
}

#[test]
fn a_song_is_offered_the_recordings_its_words_nearly_name_and_becomes_the_one_chosen() -> Result<()>
{
    let (_tree, library, _, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "1 of these")
            .text(ARTIST, "Somebody Else"),
    )?;

    let near = RecordingMatch {
        score: 61,
        ..recording_match(RECORDING, "One of These Days")
    };
    let far = RecordingMatch {
        score: 34,
        ..recording_match(ANOTHER_RECORDING, "One of These Nights")
    };

    let fake = Fake::new(Canned {
        found_recordings_in_words: vec![far, near],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(2),
        )],
        ..Canned::default()
    });
    let track = library
        .track_at(&file, None)?
        .expect("the catalog holds the track");

    let offered = library.recordings_like(track.id, &fake, "One of These Days")?;
    let ids: Vec<Mbid> = offered
        .iter()
        .map(|recording| recording.id.clone())
        .collect();

    assert_eq!(
        ids,
        [mbid(RECORDING), mbid(ANOTHER_RECORDING)],
        "a soft match below the strict rule was not offered, or not best first"
    );

    let [chosen, _] = offered.as_slice() else {
        panic!("two recordings were offered");
    };

    assert!(library.take_recording(track.id, &fake, chosen)?);

    let taken = library
        .track_at(&file, None)?
        .expect("the catalog still holds the track");

    assert_eq!(taken.title, "One of These Days");
    assert_eq!(taken.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(
        taken.track_number,
        Some(2),
        "the recording chosen was not placed on a release it is on"
    );
    Ok(())
}

#[test]
fn a_track_with_no_album_is_identified_by_the_isrc_the_file_carries() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;

    assert!(library.albums(&AlbumQuery::default())?.is_empty());

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
        )],
        found_recordings: vec![recording_match(ANOTHER_RECORDING, "One of These Days")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(fake.called(LookupOp::Recording), 0);
    assert_eq!(fake.called(LookupOp::Release), 0);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);
    assert_eq!(summary.stats.by_isrc, 1);
    assert_eq!(summary.stats.by_search, 0);
    assert_eq!(summary.stats.tracks_unmatched, 0);

    let held = stored(&database, &file);

    assert_eq!(held.title, "One of These Days");
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.asked.is_some() && held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let tracks = all(&library)?;

    assert_eq!(tracks[0].track_number, Some(1));
    assert_eq!(tracks[0].disc_number, Some(1));

    let artist = artist_named(&library, "The Orbiters")?;

    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    assert_eq!(tracks[0].artist_id, Some(artist.id));
    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    assert_eq!(fake.called(LookupOp::Artist), 1);
    Ok(())
}

#[test]
fn a_pass_tells_what_it_found_no_strict_match_for_apart_from_what_a_service_refused() -> Result<()>
{
    let (_tree, library, _database, _file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned::default()));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.tracks_unmatched, 1);
    assert_eq!(summary.stats.by_isrc, 0);
    assert_eq!(summary.stats.by_recording, 0);
    assert_eq!(summary.stats.by_search, 0);
    assert_eq!(summary.stats.by_fingerprint, 0);
    assert_eq!(summary.stats.artists_unmatched, 1);
    assert_eq!(summary.stats.profiles, 0);
    assert_eq!(summary.stats.refused, 0);

    let (_tree, library, _database, _file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake =
        Arc::new(Fake::new(Canned::default()).faulting(LookupOp::FindRecording, 0, Fault::Refused));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.refused, 1);
    assert_eq!(summary.stats.tracks_unmatched, 0);
    Ok(())
}

#[test]
fn a_track_is_identified_by_the_recording_id_the_file_carries_before_any_search() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .identified(MUSICBRAINZ, RECORDING),
    )?;

    assert_eq!(
        recorded(&database, &file).as_deref(),
        Some(RECORDING),
        "the scan did not read the recording id out of the file"
    );

    let fake = Arc::new(Fake::new(Canned {
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        found_recordings: vec![recording_match(ANOTHER_RECORDING, "One of These Days")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 0);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(
        fake.calls().first(),
        Some(&Called::Recording(mbid(RECORDING)))
    );
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);

    assert_eq!(held.isrc.as_deref(), Some("GBAYE7100195"));
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.answered.is_some());
    Ok(())
}

#[test]
fn an_artist_a_landing_names_for_the_first_time_is_asked_about_in_the_same_pass() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "An Orbiters Tribute")
            .identified(MUSICBRAINZ, RECORDING),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert!(
        fake.calls().contains(&Called::Artist(mbid(ORBITERS))),
        "the artist the recording credited was left for the next pass: {:?}",
        fake.calls()
    );

    let artist = artist_named(&library, "The Orbiters")?;
    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");

    assert!(detail.answered.is_some());
    Ok(())
}

#[test]
fn a_track_that_names_no_artist_is_never_searched_for() -> Result<()> {
    let (_tree, library, database, file) =
        scanned_lone(Wav::new().text(TITLE, "One of These Days"))?;

    assert!(
        library.albums(&AlbumQuery::default())?.is_empty(),
        "a file naming no album was filed under one"
    );

    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert!(
        fake.calls().is_empty(),
        "a track naming nobody was asked about: {:?}",
        fake.calls()
    );
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);

    let held = stored(&database, &file);

    assert!(held.asked.is_some());
    assert_eq!(held.answered, None);
    assert_eq!(recorded(&database, &file), None);

    let (_tree, library, ..) = scanned_lone(Wav::new().text(ARTIST, "The Orbiters"))?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "1")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(
        fake.called(LookupOp::FindRecording),
        0,
        "a track whose title is only its file name was searched for"
    );
    Ok(())
}

#[test]
fn a_file_named_like_a_song_by_an_artist_it_names_is_searched_for_by_its_file_name() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "One of These Days.wav",
        &Wav::new().text(ARTIST, "The Orbiters").build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert_eq!(stored(&database, &file).tagged_title, None);

    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    assert!(stored(&database, &file).answered.is_some());
    Ok(())
}

#[test]
fn a_track_naming_no_artist_under_an_unowned_album_is_searched_with_its_release() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ALBUM, "Orbits"),
    )?;

    assert_eq!(only_album(&library)?.artist, None);

    let fake = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    let asked = RecordingAsked {
        title: "One of These Days".to_owned(),
        artist: None,
        artist_mbid: None,
        release: Some("Orbits".to_owned()),
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };

    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    Ok(())
}

#[test]
fn a_track_naming_no_artist_under_an_owned_album_is_searched_with_the_owner_and_lands() -> Result<()>
{
    let tree = Tree::new();

    tree.write(
        "Orbits/1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "1")
            .build(),
    );

    let unnamed = tree.write(
        "Orbits/2.wav",
        &Wav::new()
            .text(TITLE, "A Pillow of Winds")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(album.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(stored(&database, &unnamed).artist, None);

    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "A Pillow of Winds")],
        recordings: vec![orbits_recording(
            RECORDING,
            "A Pillow of Winds",
            CODE,
            on_orbits(2),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();

    assert!(
        searched.contains(&Called::FindRecording(RecordingAsked {
            title: "A Pillow of Winds".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            artist_mbid: None,
            release: None,
            length: Some(THE_FILES_LENGTH),
            wording: Wording::Phrase,
        })),
        "the track naming no artist was not searched for with its album's owner: {searched:?}"
    );
    assert_eq!(summary.stats.named, 1);
    assert_eq!(recorded(&database, &unnamed).as_deref(), Some(RECORDING));

    let held = stored(&database, &unnamed);

    assert_eq!(held.artist.as_deref(), Some("The Orbiters"));
    assert!(held.answered.is_some());
    Ok(())
}

#[test]
fn a_tagged_release_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands() -> Result<()>
{
    let tree = Tree::new();

    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;

        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_ALBUM, HOURS)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(only_album(&library)?.mbid, Some(mbid(HOURS)));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::Release(mbid(HOURS)),
            Called::FindRelease(orbits_asked()),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 0);
    Ok(())
}

#[test]
fn a_tagged_release_group_id_the_reference_does_not_hold_falls_back_to_a_group_search() -> Result<()>
{
    let tree = Tree::new();

    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;

        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_RELEASE_GROUP, HOURS)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::ReleaseGroup(mbid(HOURS)),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    Ok(())
}

#[test]
fn a_tagged_artist_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands() -> Result<()>
{
    let tree = Tree::new();

    tree.write(
        "Orbits/1.wav",
        &orbits_track("One of These Days", 1)
            .described(MUSICBRAINZ_ARTIST, ADA)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ADA))
    );

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![artist_match(100, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let of_the_artist: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| {
            matches!(
                called.op(),
                LookupOp::Artist | LookupOp::FindArtist | LookupOp::Portrait
            )
        })
        .collect();

    assert_eq!(
        of_the_artist,
        vec![
            Called::Artist(mbid(ADA)),
            Called::FindArtist("The Orbiters".to_owned()),
            Called::Artist(mbid(ORBITERS)),
        ]
    );
    assert_eq!(summary.stats.artists, 1);

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
fn a_hit_one_track_short_lands_the_pressing_its_group_names_and_lists_the_missing_row() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            group: Some(mbid(RELEASE_GROUP)),
            ..orbits_match(100, Some("The Orbiters"), Some(4))
        }],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(2)),
                group_release(RELEASE, "1971-10-30", Some(4)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::FindReleaseGroup), 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 1);
    assert_eq!(library.release_tracks(album.id)?.len(), 4);
    Ok(())
}

fn identifiers(database: &Path, file: &Path) -> (Option<String>, Option<String>, Option<String>) {
    beside(database)
        .query_row(
            "SELECT mbid, release_track_mbid, isrc FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the scan stored the track")
}

#[test]
fn a_paired_track_receives_the_identifiers_its_release_row_holds() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let file = tree.path().join("1.wav");

    assert_eq!(identifiers(&database, &file), (None, None, None));

    let mut rows = orbits_rows();

    rows[0] = ReleaseTrack {
        recording: Some(mbid(RECORDING)),
        track: Some(mbid(RELEASE_TRACK)),
        isrcs: vec!["GBAYE7100195".to_owned()],
        ..rows[0].clone()
    };

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 3);
    assert_eq!(
        identifiers(&database, &file),
        (
            Some(RECORDING.to_owned()),
            Some(RELEASE_TRACK.to_owned()),
            Some("GBAYE7100195".to_owned())
        )
    );
    assert_eq!(
        identifiers(&database, &tree.path().join("2.wav")),
        (None, None, None)
    );
    Ok(())
}

#[test]
fn an_album_whose_owner_holds_an_id_is_searched_for_by_that_id_and_agrees_by_it() -> Result<()> {
    let tree = Tree::new();

    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;

        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_ARTIST, ORBITERS)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ORBITERS))
    );

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            credit: credited(Some("Orbiters"), Some(ORBITERS)),
            ..orbits_match(100, None, Some(3))
        }],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(ReleaseAsked {
                artist_mbid: Some(mbid(ORBITERS)),
                ..orbits_asked()
            }),
            Called::Release(mbid(RELEASE)),
            Called::Artist(mbid(ORBITERS)),
            Called::ReleaseGroupsOf(mbid(ORBITERS)),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));
    Ok(())
}

#[test]
fn a_track_the_album_pass_already_paired_is_never_asked_about() -> Result<()> {
    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.tracks, 0);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(fake.called(LookupOp::Isrc), 0);

    let tree = Tree::new();

    write_orbits(&tree, true);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let two_of_the_three: Vec<ReleaseTrack> = orbits_rows().into_iter().take(2).collect();
    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(two_of_the_three, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 2);
    assert_eq!(summary.stats.tracks, 1);

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    let asked = RecordingAsked {
        title: "Fearless".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        release: None,
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };

    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    Ok(())
}

#[test]
fn a_search_match_leaves_the_title_the_file_carried_and_writes_the_identifiers_alone() -> Result<()>
{
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days (2011 Remaster)",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(fake.called(LookupOp::Recording), 1);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);

    let held = stored(&database, &file);

    assert_eq!(held.title, "One of These Days");
    assert_eq!(held.tagged_title.as_deref(), Some("One of These Days"));
    assert_eq!(held.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    Ok(())
}

#[test]
fn a_whole_rescan_keeps_the_recording_a_lookup_identified_a_file_by_until_it_is_retagged()
-> Result<()> {
    let (tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    fs::write(
        &file,
        Wav::new()
            .text(TITLE, "Fearless")
            .text(ARTIST, "The Orbiters")
            .build(),
    )
    .expect("a writable temporary file");
    scan(&library, &options(&tree))?;

    assert_eq!(recorded(&database, &file), None);
    Ok(())
}

#[test]
fn a_track_the_phrase_answered_nothing_for_is_searched_again_in_words() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings_in_words: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    let asked = RecordingAsked {
        title: "One of These Days".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        release: None,
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };
    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();

    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    assert!(stored(&database, &file).answered.is_some());
    Ok(())
}

#[test]
fn a_recording_the_search_placed_is_landed_without_asking_for_it_again() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![RecordingMatch {
            releases: on_orbits(4),
            ..recording_match(RECORDING, "One of These Days")
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        0,
        "a search answer that says where the recording sits was asked for a second time"
    );
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);

    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert_eq!(
        held.isrc, None,
        "the answer named no code, so none was to be written"
    );
    assert!(held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let tracks = all(&library)?;

    assert_eq!(tracks[0].track_number, Some(4));
    assert_eq!(tracks[0].disc_number, Some(1));
    Ok(())
}

#[test]
fn a_code_the_search_answer_named_is_written_without_asking_for_the_recording_again() -> Result<()>
{
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![RecordingMatch {
            releases: on_orbits(4),
            isrcs: vec![isrc(CODE)],
            ..recording_match(RECORDING, "One of These Days")
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        0,
        "the code was in the search answer, so nothing was to be asked twice"
    );
    assert_eq!(
        stored(&database, &file).isrc.as_deref(),
        Some("GBAYE7100195")
    );
    Ok(())
}

#[test]
fn an_exact_identification_corrects_a_title_the_file_carried() -> Result<()> {
    let tree = Tree::new();
    let carried = tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "one of these days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE)
            .build(),
    );
    let bare = tree.write("2.wav", &Wav::new().text(ISRC, ANOTHER_CODE).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert_eq!(stored(&database, &bare).title, "2");

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![
            (
                isrc(CODE),
                orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
            ),
            (
                isrc(ANOTHER_CODE),
                orbits_recording(
                    ANOTHER_RECORDING,
                    "A Pillow of Winds",
                    ANOTHER_CODE,
                    on_orbits(2),
                ),
            ),
        ],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.tracks, 2);
    assert_eq!(summary.stats.named, 2);

    let corrected = stored(&database, &carried);

    assert_eq!(corrected.title, "One of These Days");
    assert_eq!(corrected.tagged_title.as_deref(), Some("one of these days"));
    assert_eq!(corrected.artist.as_deref(), Some("The Orbiters"));

    let filled = stored(&database, &bare);

    assert_eq!(filled.title, "A Pillow of Winds");
    assert_eq!(filled.tagged_title, None);
    assert_eq!(filled.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(filled.tagged_artist, None);
    Ok(())
}

#[test]
fn a_track_whose_album_never_answered_lands_the_release_its_recording_names() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(ISRC, CODE),
    )?;

    assert_eq!(only_album(&library)?.mbid, None);

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        fake.called(LookupOp::FindRelease),
        2,
        "the album's near miss was not asked again in words"
    );
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.matched, 1);

    let album = only_album(&library)?;

    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 2);
    assert_eq!(library.release_tracks(album.id)?.len(), 3);

    let release = library.release_of(album.id)?.expect("the album is known");

    assert!(release.answered.is_some());
    assert_eq!(
        stored(&database, &file).release_title.as_deref(),
        Some("Orbits")
    );
    Ok(())
}

#[test]
fn a_corrected_title_is_what_the_search_index_finds_the_track_by() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;

    assert_eq!(titles(&library.search("Echos", 10)?.tracks), vec!["Echos"]);

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "Echoes", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.named, 1);

    assert_eq!(
        titles(&library.search("Echoes", 10)?.tracks),
        vec!["Echoes"]
    );
    assert!(
        library.search("Echos", 10)?.tracks.is_empty(),
        "the title the file carried still finds the row the enrichment renamed"
    );
    Ok(())
}

#[test]
fn an_album_artist_still_finds_its_tracks_once_the_enrichment_indexes_them_again() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "Zephyr Collective")
            .text(ALBUM, "Orbits")
            .text(ISRC, CODE),
    )?;

    assert_eq!(titles(&library.search("Zephyr", 10)?.tracks), vec!["Echos"]);

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "Echoes", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    assert_eq!(enrich(&library, &fake, false)?.stats.named, 1);

    assert_eq!(
        titles(&library.search("Zephyr", 10)?.tracks),
        vec!["Echoes"],
        "the album artist no longer finds the track the enrichment indexed again"
    );
    Ok(())
}

#[test]
fn a_rescan_indexes_and_bills_the_names_the_lookup_kept() -> Result<()> {
    let tagged = || {
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "Orbiterz")
            .text(ISRC, CODE)
    };
    let (tree, library, ..) = scanned_lone(tagged())?;

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "Echoes", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    assert_eq!(enrich(&library, &fake, false)?.stats.named, 1);

    let answered = library.search("Echoes", 10)?.tracks;

    assert_eq!(titles(&answered), vec!["Echoes"]);

    tree.write("1.wav", &tagged().build());

    assert_eq!(scan(&library, &options(&tree))?.updated, 1);

    let rescanned = library.search("Echoes", 10)?.tracks;

    assert_eq!(
        titles(&rescanned),
        vec!["Echoes"],
        "a rescan indexed the file's title over the one the row still shows"
    );
    assert!(library.search("Echos", 10)?.tracks.is_empty());
    assert_eq!(rescanned[0].artist.as_deref(), Some("The Orbiters"));
    assert_eq!(
        rescanned[0].artist_id, answered[0].artist_id,
        "a rescan pointed the row back at the artist the file names"
    );
    Ok(())
}
