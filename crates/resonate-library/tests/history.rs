use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::Ordering},
    time::{Duration, SystemTime},
};

use resonate_core::{
    FrameSpan, Frames, KeptZone, MediaLocation, Reordered, Resumable, Resumption, SourceId, Span,
    TrackId, Volume, ZoneId, ZoneName,
};
use resonate_library::{
    Aged, AlbumQuery, ArtistQuery, Error, Favoured, HistoryKept, Isrc, Library, Listen,
    ListeningService, LookupOp, Love, LovedNames, LovesTold, Mbid, PollOptions, Result,
    ScanOptions, Scrobbler as _, SortOrder, TrackQuery, Window,
};

use crate::support::{
    ALBUM, ARTIST, Delivering, HOURS, ISRC, MUSICBRAINZ, Offering, RECORDING, TITLE, TRACK, Told,
    Tree, Wav, album_titled, all, artist_named, beside, cancelled_once_a_batch_is_written,
    many_short_songs, matched, mbid, opened_with_a_vault, options, orbits_tree, scan,
    scanned_shapes, the_row_of, titles, wanted_san_tropez,
};

fn resumable(path: &str) -> Resumable {
    Resumable {
        location: MediaLocation::local(path),
        span: None,
        held: None,
    }
}

#[test]
fn a_play_is_counted_against_the_track_it_named() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    let once = library
        .track_played(&cold, None, Duration::ZERO)?
        .expect("the row the play counted");

    assert_eq!(the_row_of(&once).plays, 1);
    assert_eq!(the_row_of(&once).location, cold);
    assert!(the_row_of(&once).played.is_some());

    let twice = library
        .track_played(&cold, None, Duration::ZERO)?
        .expect("the row the play counted");

    assert_eq!(the_row_of(&twice).plays, 2);
    assert_eq!(the_row_of(&twice).id, the_row_of(&once).id);
    assert_ne!(twice.listen, once.listen);

    let played = library
        .track_at(tree.path().join("cold.wav").as_path(), None)?
        .expect("the track the scan stored");

    assert_eq!(Some(played), twice.track);

    let heat = library
        .track_at(tree.path().join("heat.wav").as_path(), None)?
        .expect("the track the scan stored");

    assert_eq!(heat.plays, 0);
    assert_eq!(heat.played, None);
    Ok(())
}

#[test]
fn a_play_of_something_the_catalog_does_not_hold_counts_against_no_row() -> Result<()> {
    let (tree, library) = scanned_shapes();

    let unheld = library
        .track_played(
            &MediaLocation::local(tree.path().join("nothing.wav")),
            None,
            Duration::ZERO,
        )?
        .expect("a local file is kept against its path");

    assert_eq!(unheld.track, None);
    assert!(
        library
            .tracks(&TrackQuery::default())?
            .iter()
            .all(|track| track.plays == 0)
    );
    Ok(())
}

#[test]
fn a_song_played_in_place_counts_as_a_listen_against_its_uri_and_never_as_a_row() -> Result<()> {
    let (_tree, library) = scanned_shapes();
    let served = MediaLocation::new(
        SourceId::new("tidal").expect("a lowercase name"),
        "track/11",
    );

    let counted = library
        .track_played(&served, None, Duration::from_secs(40))?
        .expect("a play kept against the song's uri");

    library.listened(counted.listen, Duration::from_secs(90))?;

    let statistics = library.statistics(Window::Everything)?;

    assert_eq!(counted.track, None);
    assert!(matches!(counted.listen, Listen::Unheld(_)));
    assert_eq!(statistics.plays, 1);
    assert_eq!(statistics.listened, Duration::from_secs(90));
    assert_eq!(statistics.tracks, 0);
    assert!(
        library
            .tracks(&TrackQuery::default())?
            .iter()
            .all(|track| track.plays == 0)
    );
    Ok(())
}

#[test]
fn a_saved_queue_keeps_a_song_played_in_place_by_its_uri_beside_a_held_one() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    let served = MediaLocation::from_uri("subsonic:track/so-1").expect("a served song");

    let resumption = Resumption {
        rows: [served.clone(), cold.clone()]
            .into_iter()
            .map(|location| Resumable {
                location,
                span: None,
                held: None,
            })
            .collect(),
        row: 0,
        at: Frames(4410),
        ..Resumption::default()
    };

    library.keep_resumption(ZoneId::FIRST, &resumption)?;

    let resumed = library.resumption()?.expect("the queue kept");

    assert_eq!(
        resumed
            .rows
            .iter()
            .map(|row| row.location.clone())
            .collect::<Vec<_>>(),
        [served, cold]
    );
    assert_eq!(resumed.rows[0].held, None);
    assert!(resumed.rows[1].held.is_some());
    assert_eq!(resumed.row, 0);
    assert_eq!(resumed.at, Frames(4410));
    Ok(())
}

#[test]
fn a_favourite_with_a_recording_is_told_as_a_love_once_and_taken_back_when_unmarked() -> Result<()>
{
    let tree = Tree::new();

    tree.write(
        "named.wav",
        &Wav::new()
            .text(TITLE, "Named")
            .text(ARTIST, "Ada")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );
    tree.write(
        "bare.wav",
        &Wav::new().text(TITLE, "Bare").text(ARTIST, "Ada").build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let tracks = library.tracks(&TrackQuery::default())?;

    for track in &tracks {
        library.favour(Favoured::Track(track.id), true)?;
    }

    let named = tracks
        .iter()
        .find(|track| track.title == "Named")
        .expect("the named track was scanned");
    let told = Told::default();

    assert_eq!(
        library.tell_loves(&told)?,
        LovesTold {
            loved: 1,
            taken_back: 0,
            refused: 0,
        }
    );
    assert_eq!(*told.loves.lock(), vec![(mbid(RECORDING), Love::Loved)]);
    assert_eq!(library.tell_loves(&told)?, LovesTold::default());

    library.favour(Favoured::Track(named.id), false)?;

    assert_eq!(library.tell_loves(&told)?.taken_back, 1);
    assert_eq!(
        told.loves.lock().last(),
        Some(&(mbid(RECORDING), Love::TakenBack))
    );
    assert_eq!(library.tell_loves(&told)?, LovesTold::default());

    let ada_named = Some(LovedNames {
        title: "Named".to_owned(),
        artist: "Ada".to_owned(),
    });

    assert_eq!(*told.named.lock(), vec![ada_named.clone(), ada_named]);
    Ok(())
}

fn two_favourites_with_recordings() -> (Tree, Library) {
    let tree = Tree::new();

    for (name, recording) in [("first.wav", RECORDING), ("second.wav", HOURS)] {
        tree.write(
            name,
            &Wav::new()
                .text(TITLE, name)
                .text(ARTIST, "Ada")
                .identified(MUSICBRAINZ, recording)
                .build(),
        );
    }

    let library = Library::open_in_memory().expect("an in-memory library opens");

    scan(&library, &options(&tree)).expect("the tree scans");
    for track in library
        .tracks(&TrackQuery::default())
        .expect("the tracks list")
    {
        library
            .favour(Favoured::Track(track.id), true)
            .expect("a track is favoured");
    }
    (tree, library)
}

#[test]
fn a_love_the_service_refuses_outright_is_passed_over_and_the_others_are_told() -> Result<()> {
    let (_tree, library) = two_favourites_with_recordings();
    let told = Told::refusing_love(RECORDING, 404);

    let said = library.tell_loves(&told)?;

    assert_eq!(
        said,
        LovesTold {
            loved: 1,
            taken_back: 0,
            refused: 1,
        }
    );
    assert_eq!(*told.loves.lock(), vec![(mbid(HOURS), Love::Loved)]);
    assert_eq!(library.tell_loves(&told)?, LovesTold::default());
    Ok(())
}

#[test]
fn a_love_the_service_answers_with_a_rate_limit_is_asked_again_later() {
    let (_tree, library) = two_favourites_with_recordings();
    let told = Told::refusing_love(RECORDING, 429);

    for _ in 0..2 {
        assert!(matches!(
            library.tell_loves(&told),
            Err(Error::Refused {
                op: LookupOp::Love,
                status: 429,
            })
        ));
    }
}

fn scanned_listening() -> (Tree, Library, [MediaLocation; 3]) {
    let tree = Tree::new();
    let cold = tree.write(
        "cold.wav",
        &Wav::new()
            .text(TITLE, "Cold")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Winter")
            .text(TRACK, "3")
            .build(),
    );
    let heat = tree.write(
        "heat.wav",
        &Wav::new()
            .frames(44_100)
            .text(TITLE, "Heat")
            .text(ARTIST, "Ben")
            .build(),
    );
    let loose = tree.write("loose.wav", &Wav::new().text(TITLE, "Loose").build());

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library, [cold, heat, loose].map(MediaLocation::local))
}

#[test]
fn a_service_is_told_what_was_heard_after_it_was_first_asked_and_each_play_once() -> Result<()> {
    let (_tree, library, [cold, heat, loose]) = scanned_listening();

    library.track_played(&cold, None, Duration::ZERO)?;

    let told = Told::default();

    let started = library.submit_listens(&told)?;

    assert!(started.started);
    assert_eq!(started.submitted, 0);
    assert!(told.titles().is_empty());

    library.track_played(&heat, None, Duration::ZERO)?;
    library.track_played(&loose, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;

    let submitted = library.submit_listens(&told)?;

    assert_eq!(submitted.submitted, 2);
    assert_eq!(submitted.unnamed, 1);
    assert!(!submitted.started);
    assert_eq!(told.titles(), vec![vec!["Heat", "Cold"]]);

    let batch = told.batches.lock()[0].clone();

    assert_eq!(batch[0].billed.artist, "Ben");
    assert_eq!(batch[0].billed.album, None);
    assert_eq!(batch[0].billed.length, Some(Duration::from_secs(1)));
    assert_eq!(batch[1].billed.artist, "Ada");
    assert_eq!(batch[1].billed.album.as_deref(), Some("Winter"));
    assert_eq!(batch[1].billed.number, Some(3));
    assert!(batch[0].listen < batch[1].listen);
    assert!(batch[0].at <= batch[1].at);

    assert_eq!(library.submit_listens(&told)?.submitted, 0);
    assert_eq!(told.titles().len(), 1);
    Ok(())
}

#[test]
fn a_listen_is_told_as_when_it_began_rather_than_when_it_counted() -> Result<()> {
    const HEARD_FOR: Duration = Duration::from_secs(95);

    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::default();

    library.submit_listens(&told)?;

    library.track_played(&cold, None, HEARD_FOR)?;

    let counted = SystemTime::now();

    library.track_played(&heat, None, Duration::ZERO)?;
    library.submit_listens(&told)?;

    let batch = told.batches.lock()[0].clone();
    let began = counted
        .duration_since(batch[0].at)
        .expect("a listen told as beginning after it counted");

    assert!(
        began >= HEARD_FOR && began < HEARD_FOR + Duration::from_secs(5),
        "a listen heard for {HEARD_FOR:?} was told as beginning {began:?} before it counted"
    );
    assert!(batch[1].at >= batch[0].at + HEARD_FOR);
    Ok(())
}

#[test]
fn what_is_playing_is_billed_as_the_catalog_names_it_and_a_file_it_does_not_hold_is_not()
-> Result<()> {
    let (tree, library, [cold, _, loose]) = scanned_listening();

    let billed = library
        .billed_as(&cold, None)?
        .expect("a scanned track the service can be told of");

    assert_eq!(billed.title, "Cold");
    assert_eq!(billed.artist, "Ada");
    assert_eq!(library.billed_as(&loose, None)?, None);
    assert_eq!(
        library.billed_as(&MediaLocation::local(tree.path().join("nowhere.wav")), None)?,
        None
    );
    Ok(())
}

#[test]
fn plays_from_before_the_service_was_first_asked_are_told_once_and_only_when_asked_for()
-> Result<()> {
    const WELL_BEFORE_A_YEAR: i64 = 400 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let cold = MediaLocation::local(tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    ));
    let heat = MediaLocation::local(tree.write(
        "heat.wav",
        &Wav::new().text(TITLE, "Heat").text(ARTIST, "Ben").build(),
    ));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let told = Told::default();
    let a_year = HistoryKept::parse("365").expect("a span of days");

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&heat, None, Duration::ZERO)?;

    assert!(library.submit_listens(&told)?.started);

    library.track_played(&heat, None, Duration::ZERO)?;

    assert_eq!(library.submit_listens(&told)?.submitted, 1);
    assert!(!library.earlier_listens_owed(ListeningService::ListenBrainz)?);

    library.tell_earlier_listens(ListeningService::ListenBrainz)?;
    library.tell_earlier_listens(ListeningService::ListenBrainz)?;

    assert!(library.earlier_listens_owed(ListeningService::ListenBrainz)?);

    beside(&database)
        .execute_batch(&format!(
            "UPDATE listens SET at = at - {WELL_BEFORE_A_YEAR};"
        ))
        .expect("the history is moved back past a year");

    assert_eq!(
        library.age_the_history(a_year)?.listens,
        0,
        "a play asked to be told was forgotten before it was"
    );

    library.track_played(&cold, None, Duration::ZERO)?;

    let submitted = library.submit_listens(&told)?;

    assert_eq!(submitted.submitted, 3);
    assert_eq!(submitted.earlier, 2);
    assert_eq!(
        told.titles(),
        vec![vec!["Heat"], vec!["Cold"], vec!["Cold", "Heat"]]
    );
    assert!(!library.earlier_listens_owed(ListeningService::ListenBrainz)?);
    assert_eq!(library.submit_listens(&told)?.submitted, 0);
    assert_eq!(library.age_the_history(a_year)?.listens, 3);
    Ok(())
}

#[test]
fn a_history_kept_for_a_span_forgets_what_is_older_once_every_service_was_told() -> Result<()> {
    const WELL_BEFORE_A_YEAR: i64 = 400 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let cold = MediaLocation::local(tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    ));
    let heat = MediaLocation::local(tree.write(
        "heat.wav",
        &Wav::new().text(TITLE, "Heat").text(ARTIST, "Ben").build(),
    ));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    library.track_played(&cold, None, Duration::ZERO)?;

    let told = Told::default();

    assert!(library.submit_listens(&told)?.started);

    library.track_played(&heat, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;
    library.passed(Duration::from_secs(20))?;
    beside(&database)
        .execute_batch(&format!(
            "UPDATE listens SET at = at - {WELL_BEFORE_A_YEAR};
             UPDATE passes SET at = at - {WELL_BEFORE_A_YEAR};"
        ))
        .expect("the history is moved back past a year");

    assert_eq!(
        library.age_the_history(HistoryKept::Forever)?,
        Aged::default()
    );

    let a_year = HistoryKept::parse("365").expect("a span of days");

    assert_eq!(
        library.age_the_history(a_year)?,
        Aged {
            listens: 1,
            passes: 1
        },
        "a listen no service had been told of yet was forgotten"
    );
    assert_eq!(library.statistics(Window::Everything)?.plays, 2);

    assert_eq!(library.submit_listens(&told)?.submitted, 2);
    assert_eq!(library.age_the_history(a_year)?.listens, 2);
    assert_eq!(library.statistics(Window::Everything)?.plays, 0);
    assert_eq!(
        library
            .track_at(cold.as_path().expect("a local file"), None)?
            .expect("the scanned track")
            .plays,
        2,
        "forgetting the history took the count with it"
    );
    Ok(())
}

#[test]
fn a_service_no_longer_told_holds_the_history_back_no_longer() -> Result<()> {
    const WELL_BEFORE_A_YEAR: i64 = 400 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let cold = MediaLocation::local(tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    ));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let told = Told::default();

    assert!(library.submit_listens(&told)?.started);

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;
    beside(&database)
        .execute_batch(&format!(
            "UPDATE listens SET at = at - {WELL_BEFORE_A_YEAR};"
        ))
        .expect("the history is moved back past a year");

    let a_year = HistoryKept::parse("365").expect("a span of days");

    assert_eq!(library.age_the_history(a_year)?.listens, 0);
    assert!(library.stop_telling(told.service())?);
    assert!(!library.stop_telling(told.service())?);
    assert_eq!(library.age_the_history(a_year)?.listens, 2);
    Ok(())
}

#[test]
fn a_play_counted_after_the_newest_listens_were_forgotten_is_still_told() -> Result<()> {
    const WELL_BEFORE_A_YEAR: i64 = 400 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let cold = MediaLocation::local(tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    ));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let told = Told::default();

    assert!(library.submit_listens(&told)?.started);

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;

    assert_eq!(library.submit_listens(&told)?.submitted, 2);

    beside(&database)
        .execute_batch(&format!(
            "UPDATE listens SET at = at - {WELL_BEFORE_A_YEAR};"
        ))
        .expect("the history is moved back past a year");

    let a_year = HistoryKept::parse("365").expect("a span of days");

    assert_eq!(library.age_the_history(a_year)?.listens, 2);

    library.track_played(&cold, None, Duration::ZERO)?;

    assert_eq!(library.submit_listens(&told)?.submitted, 1);
    Ok(())
}

#[test]
fn the_tracks_most_played_this_month_are_ordered_by_what_the_month_heard() -> Result<()> {
    const TWO_MONTHS: i64 = 60 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let cold = MediaLocation::local(tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    ));
    let heat = MediaLocation::local(tree.write(
        "heat.wav",
        &Wav::new().text(TITLE, "Heat").text(ARTIST, "Ben").build(),
    ));
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    for _ in 0..3 {
        library.track_played(&cold, None, Duration::ZERO)?;
    }
    beside(&database)
        .execute_batch(&format!("UPDATE listens SET at = at - {TWO_MONTHS};"))
        .expect("the plays are moved back two months");
    library.track_played(&heat, None, Duration::ZERO)?;

    let titles = |sort| -> Result<Vec<String>> {
        Ok(library
            .tracks(&TrackQuery {
                sort,
                reading: sort.reads(),
                ..TrackQuery::default()
            })?
            .into_iter()
            .map(|track| track.title)
            .collect())
    };

    assert_eq!(titles(SortOrder::Plays)?, vec!["Cold", "Heat"]);
    assert_eq!(titles(SortOrder::PlaysThisMonth)?, vec!["Heat", "Cold"]);
    Ok(())
}

#[test]
fn a_play_the_service_refuses_as_malformed_is_passed_over_and_the_rest_are_told() -> Result<()> {
    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::refusing("Heat");

    library.submit_listens(&told)?;

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&heat, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;

    let submitted = library.submit_listens(&told)?;

    assert_eq!(submitted.submitted, 2);
    assert_eq!(submitted.refused, 1);
    assert_eq!(
        told.titles(),
        vec![
            vec!["Cold", "Heat", "Cold"],
            vec!["Cold"],
            vec!["Heat"],
            vec!["Cold"],
        ]
    );

    assert_eq!(library.submit_listens(&told)?, Default::default());
    assert_eq!(told.titles().len(), 4);
    Ok(())
}

#[test]
fn a_play_a_service_could_not_be_reached_for_is_told_the_next_time() -> Result<()> {
    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::default();

    library.submit_listens(&told)?;
    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&heat, None, Duration::ZERO)?;

    told.unreachable.store(true, Ordering::Relaxed);

    assert!(matches!(
        library.submit_listens(&told),
        Err(Error::Unreachable {
            op: LookupOp::Submit,
            ..
        })
    ));

    told.unreachable.store(false, Ordering::Relaxed);

    assert_eq!(library.submit_listens(&told)?.submitted, 2);
    assert_eq!(told.titles(), vec![vec!["Cold", "Heat"]]);
    Ok(())
}

#[test]
fn a_batch_a_service_takes_in_pieces_is_marked_a_piece_at_a_time() -> Result<()> {
    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::taking(2, 1);

    library.submit_listens(&told)?;
    for track in [&cold, &heat, &cold, &heat, &cold] {
        library.track_played(track, None, Duration::ZERO)?;
    }

    assert!(matches!(
        library.submit_listens(&told),
        Err(Error::Unreachable { .. })
    ));
    assert_eq!(told.titles(), vec![vec!["Cold", "Heat"]]);

    let again = Told::taking(2, usize::MAX);
    let submitted = library.submit_listens(&again)?;

    assert_eq!(submitted.submitted, 3);
    assert_eq!(again.titles(), vec![vec!["Cold", "Heat"], vec!["Cold"]]);
    Ok(())
}

#[test]
fn a_rescan_keeps_how_often_a_track_was_played() -> Result<()> {
    let tree = Tree::new();

    tree.write("one.wav", &Wav::new().text(TITLE, "before").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert!(
        library
            .track_played(
                &MediaLocation::local(tree.path().join("one.wav")),
                None,
                Duration::ZERO
            )?
            .is_some()
    );

    tree.write(
        "one.wav",
        &Wav::new().frames(8_820).text(TITLE, "after").build(),
    );
    scan(&library, &options(&tree))?;

    let tracks = all(&library)?;
    let track = tracks.first().expect("the track is still there");

    assert_eq!(track.title, "after");
    assert_eq!(track.plays, 1);
    assert!(track.played.is_some());
    Ok(())
}

#[test]
fn a_play_counted_against_a_track_is_a_listen_a_window_can_narrow_on() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    assert!(matched(&library, "plays:>0@30d")?.is_empty());

    library.track_played(&cold, None, Duration::ZERO)?;

    assert_eq!(matched(&library, "plays:>0@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:0@30d")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn forgetting_a_root_keeps_the_listens_counted_under_it_for_the_rows_that_come_back() -> Result<()>
{
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;

    assert_eq!(matched(&library, "plays:>1@30d")?, vec!["Cold"]);

    assert!(library.remove_root(tree.path())?);
    assert!(all(&library)?.is_empty());

    scan(&library, &options(&tree))?;

    assert_eq!(titles(&all(&library)?), vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "plays:0")?, vec!["Heat"]);
    assert_eq!(matched(&library, "plays:>1@30d")?, vec!["Cold"]);
    Ok(())
}

#[test]
fn a_catalog_nothing_has_played_has_nothing_to_resume() -> Result<()> {
    let library = Library::open_in_memory()?;

    assert_eq!(library.resumption()?, None);
    Ok(())
}

#[test]
fn a_kept_queue_comes_back_as_it_was_left() -> Result<()> {
    let library = Library::open_in_memory()?;
    let cut = FrameSpan::between(Frames(44_100), Frames(88_200));
    let kept = Resumption {
        rows: vec![
            resumable("/music/first.flac"),
            Resumable {
                location: MediaLocation::local("/music/whole.flac"),
                span: Some(cut),
                held: None,
            },
        ],
        order: vec![0, 1],
        row: 1,
        at: Frames(12_345),
        shuffle: false,
        next: None,
    };

    library.keep_resumption(ZoneId::FIRST, &kept)?;

    assert_eq!(library.resumption()?, Some(kept));
    Ok(())
}

#[test]
fn what_was_queued_to_play_next_comes_back_and_goes_when_nothing_is() -> Result<()> {
    let library = Library::open_in_memory()?;
    let kept = Resumption {
        rows: vec![
            resumable("/music/first.flac"),
            resumable("/music/queued.flac"),
            resumable("/music/also-queued.flac"),
            resumable("/music/second.flac"),
        ],
        order: vec![0, 1, 2, 3],
        row: 0,
        at: Frames(4_410),
        shuffle: false,
        next: Some(Span::between(1, 2)),
    };

    library.keep_resumption(ZoneId::FIRST, &kept)?;

    assert_eq!(library.resumption()?, Some(kept.clone()));

    library.keep_order(
        ZoneId::FIRST,
        &Reordered {
            order: vec![0, 1, 2, 3],
            row: 3,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        },
    )?;

    assert_eq!(library.resumption()?.and_then(|kept| kept.next), None);
    Ok(())
}

#[test]
fn a_shuffled_queue_comes_back_saying_where_each_row_was_loaded() -> Result<()> {
    let library = Library::open_in_memory()?;
    let kept = Resumption {
        rows: vec![
            resumable("/music/first.flac"),
            resumable("/music/second.flac"),
            resumable("/music/third.flac"),
        ],
        order: vec![2, 0, 1],
        row: 1,
        at: Frames(4_410),
        shuffle: true,
        next: None,
    };

    library.keep_resumption(ZoneId::FIRST, &kept)?;

    assert_eq!(library.resumption()?, Some(kept));
    Ok(())
}

#[test]
fn a_row_from_a_source_that_is_not_the_filesystem_is_kept_by_its_uri() -> Result<()> {
    let library = Library::open_in_memory()?;
    let elsewhere = MediaLocation::new(SourceId::new("subsonic")?, "track/1.flac");
    let kept = Resumption {
        rows: vec![Resumable {
            location: elsewhere.clone(),
            span: None,
            held: None,
        }],
        order: vec![0],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
        next: None,
    };

    library.keep_resumption(ZoneId::FIRST, &kept)?;

    assert_eq!(
        library.resumption()?.map(|kept| kept.rows),
        Some(vec![Resumable {
            location: elsewhere,
            span: None,
            held: None,
        }])
    );
    Ok(())
}

#[test]
fn keeping_the_place_moves_it_without_rewriting_the_rows_or_the_shuffle() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows = vec![
        resumable("/music/first.flac"),
        resumable("/music/second.flac"),
    ];

    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: rows.clone(),
            order: vec![1, 0],
            row: 0,
            at: Frames::ZERO,
            shuffle: true,
            next: None,
        },
    )?;
    library.keep_place(ZoneId::FIRST, 1, Frames(999))?;

    assert_eq!(
        library.resumption()?,
        Some(Resumption {
            rows,
            order: vec![1, 0],
            row: 1,
            at: Frames(999),
            shuffle: true,
            next: None,
        })
    );
    Ok(())
}

#[test]
fn an_order_kept_on_its_own_leaves_the_rows_where_they_were() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows = vec![
        resumable("/music/first.flac"),
        resumable("/music/second.flac"),
        resumable("/music/third.flac"),
    ];

    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: rows.clone(),
            order: vec![0, 1, 2],
            row: 0,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        },
    )?;

    library.keep_order(
        ZoneId::FIRST,
        &Reordered {
            order: vec![2, 0, 1],
            row: 1,
            at: Frames(4_410),
            shuffle: true,
            next: None,
        },
    )?;

    assert_eq!(
        library.resumption()?,
        Some(Resumption {
            rows,
            order: vec![2, 0, 1],
            row: 1,
            at: Frames(4_410),
            shuffle: true,
            next: None,
        }),
        "keeping the order alone did not leave the rows as they were"
    );
    Ok(())
}

#[test]
fn a_queue_kept_again_replaces_the_rows_rather_than_adding_to_them() -> Result<()> {
    let library = Library::open_in_memory()?;
    let longer = Resumption {
        rows: (0..3)
            .map(|number| resumable(&format!("/music/{number}.flac")))
            .collect(),
        order: vec![0, 1, 2],
        row: 2,
        at: Frames::ZERO,
        shuffle: true,
        next: None,
    };
    let shorter = Resumption {
        rows: vec![resumable("/music/only.flac")],
        order: vec![0],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
        next: None,
    };

    library.keep_resumption(ZoneId::FIRST, &longer)?;
    library.keep_resumption(ZoneId::FIRST, &shorter)?;

    assert_eq!(library.resumption()?, Some(shorter));
    Ok(())
}

#[test]
fn a_queue_kept_as_nothing_leaves_nothing_to_resume() -> Result<()> {
    let library = Library::open_in_memory()?;

    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: vec![resumable("/music/only.flac")],
            order: vec![0],
            row: 0,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        },
    )?;
    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: Vec::new(),
            order: Vec::new(),
            row: 0,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        },
    )?;

    assert_eq!(library.resumption()?, None);
    Ok(())
}

#[test]
fn forgetting_a_kept_queue_takes_the_place_with_it() -> Result<()> {
    let library = Library::open_in_memory()?;

    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: vec![resumable("/music/only.flac")],
            order: vec![0],
            row: 0,
            at: Frames(500),
            shuffle: false,
            next: None,
        },
    )?;
    library.forget_resumption()?;

    assert_eq!(library.resumption()?, None);

    library.forget_resumption()?;
    Ok(())
}

#[test]
fn every_zone_keeps_its_own_name_device_volume_and_queue_and_comes_back_as_it_was_left()
-> Result<()> {
    let library = Library::open_in_memory()?;
    let second = ZoneId::new(2).expect("a second zone");
    let first_queue = Resumption {
        rows: vec![resumable("/music/kitchen.flac")],
        order: vec![0],
        row: 0,
        at: Frames(100),
        shuffle: false,
        next: None,
    };
    let second_queue = Resumption {
        rows: vec![
            resumable("/music/study-one.flac"),
            resumable("/music/study-two.flac"),
        ],
        order: vec![1, 0],
        row: 1,
        at: Frames(4_410),
        shuffle: true,
        next: None,
    };
    let study = KeptZone {
        id: second,
        name: ZoneName::given("Study").expect("a name"),
        device: Some("alsa_output.usb".into()),
        volume: Some(Volume::new(0.5).expect("half way")),
    };
    let kitchen = KeptZone {
        id: ZoneId::FIRST,
        name: ZoneName::given("Kitchen").expect("a name"),
        device: None,
        volume: None,
    };

    library.keep_zone(&kitchen)?;
    library.keep_zone(&study)?;
    library.keep_resumption(ZoneId::FIRST, &first_queue)?;
    library.keep_resumption(second, &second_queue)?;
    library.keep_place(second, 0, Frames(9))?;

    assert_eq!(library.zones()?, vec![kitchen.clone(), study.clone()]);
    assert_eq!(library.resumption()?, Some(first_queue.clone()));
    assert_eq!(
        library.resumption_of(second)?,
        Some(Resumption {
            row: 0,
            at: Frames(9),
            ..second_queue
        })
    );

    library.forget_zone(second)?;

    assert_eq!(library.zones()?, vec![kitchen]);
    assert_eq!(library.resumption_of(second)?, None);
    assert_eq!(library.resumption()?, Some(first_queue));

    library.forget_resumption()?;

    assert_eq!(library.zones()?, Vec::new());
    Ok(())
}

fn heard_in(database: &Path) -> Vec<i64> {
    let connection = beside(database);
    let mut statement = connection
        .prepare("SELECT heard FROM listens ORDER BY id")
        .expect("the listens read back");
    statement
        .query_map([], |row| row.get::<_, i64>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .expect("the listens read back")
}

#[test]
fn a_play_records_how_long_of_it_was_heard() -> Result<()> {
    let tree = Tree::new();

    tree.write("cold.wav", &Wav::new().text(TITLE, "Cold").build());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    let counted = library
        .track_played(&cold, None, Duration::ZERO)?
        .expect("the row the play counted");

    assert_eq!(
        heard_in(&database),
        vec![0],
        "a play that has not been heard out yet was counted as though it had"
    );

    library.listened(counted.listen, Duration::from_secs(90))?;

    assert_eq!(heard_in(&database), vec![90_000_000_000]);

    let again = library
        .track_played(&cold, None, Duration::ZERO)?
        .expect("the row the play counted");

    assert_ne!(again.listen, counted.listen);

    library.listened(again.listen, Duration::from_millis(1_500))?;

    assert_eq!(
        heard_in(&database),
        vec![90_000_000_000, 1_500_000_000],
        "one visit's length was written over another's"
    );
    Ok(())
}

fn favoured_tracks(library: &Library) -> Result<Vec<String>> {
    Ok(library
        .favourite_tracks(&TrackQuery {
            sort: SortOrder::Favourited,
            reading: SortOrder::Favourited.reads(),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

#[test]
fn a_favourite_track_is_found_by_is_favourite() -> Result<()> {
    let (tree, library) = scanned_shapes();

    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");

    assert_eq!(cold.favourite, None);
    assert!(favoured_tracks(&library)?.is_empty());

    assert!(library.favour(Favoured::Track(cold.id), true)?);

    let narrowed = library.tracks(&TrackQuery {
        text: Some("is:favourite".to_owned()),
        sort: SortOrder::Title,
        ..TrackQuery::default()
    })?;

    assert_eq!(titles(&narrowed), vec!["Cold"]);
    assert!(narrowed[0].favourite.is_some());
    assert_eq!(
        favoured_tracks(&library)?,
        vec!["Cold"],
        "the favourites listing and is:favourite do not agree"
    );
    Ok(())
}

#[test]
fn favouriting_an_album_leaves_its_tracks_alone() -> Result<()> {
    let (_tree, library) = scanned_shapes();
    let winter = album_titled(&library, "Winter")?;

    assert_eq!(winter.favourite, None);

    assert!(library.favour(Favoured::Album(winter.id), true)?);

    assert!(
        library
            .album(winter.id)?
            .and_then(|album| album.favourite)
            .is_some()
    );
    assert_eq!(
        library
            .favourite_albums(&AlbumQuery::default())?
            .into_iter()
            .map(|album| album.title)
            .collect::<Vec<String>>(),
        vec!["Winter"]
    );
    assert!(
        favoured_tracks(&library)?.is_empty(),
        "favouriting an album favourited the tracks under it"
    );

    let ada = artist_named(&library, "Ada")?;

    assert_eq!(ada.favourite, None);
    assert!(library.favour(Favoured::Artist(ada.id), true)?);
    assert_eq!(
        library
            .favourite_artists(&ArtistQuery::default())?
            .into_iter()
            .map(|artist| artist.name)
            .collect::<Vec<String>>(),
        vec!["Ada"]
    );
    assert!(favoured_tracks(&library)?.is_empty());
    Ok(())
}

#[test]
fn unfavouriting_puts_a_row_back_the_way_it_was() -> Result<()> {
    let (tree, library) = scanned_shapes();

    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");

    assert!(library.favour(Favoured::Track(cold.id), true)?);
    assert!(library.favour(Favoured::Track(cold.id), false)?);

    assert_eq!(
        library.track(cold.id)?,
        Some(cold),
        "unfavouriting left the row somewhere other than where it started"
    );
    assert!(favoured_tracks(&library)?.is_empty());
    assert!(
        !library.favour(Favoured::Track(TrackId::MAX), false)?,
        "an id the catalog holds no row for answered as though a row had changed"
    );
    Ok(())
}

#[test]
fn favouring_a_favourite_keeps_the_stamp_it_was_marked_with_and_answers_that_nothing_moved()
-> Result<()> {
    let (tree, library) = scanned_shapes();

    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");

    assert!(library.favour(Favoured::Track(cold.id), true)?);

    let marked = library
        .track(cold.id)?
        .expect("the row is still there")
        .favourite;

    assert!(marked.is_some(), "favouring left no stamp");

    assert!(
        !library.favour(Favoured::Track(cold.id), true)?,
        "favouring a favourite answered as though it had moved"
    );
    assert_eq!(
        library
            .track(cold.id)?
            .expect("the row is still there")
            .favourite,
        marked,
        "favouring a favourite again restamped it"
    );

    assert!(library.favour(Favoured::Track(cold.id), false)?);
    assert!(
        !library.favour(Favoured::Track(cold.id), false)?,
        "unfavouring a row that is no favourite answered as though it had moved"
    );
    Ok(())
}

const HEARD_FOR: Duration = Duration::from_secs(180);

#[test]
fn listening_time_is_the_time_that_was_heard() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let track = all(&library)?.remove(0);

    let counted = library
        .track_played(&track.location, track.span, Duration::ZERO)?
        .expect("the catalog counted the play against the row it scanned");

    library.listened(counted.listen, HEARD_FOR)?;

    let heard = library.statistics(Window::Week)?;

    assert_eq!(heard.plays, 1);
    assert_eq!(heard.listened, HEARD_FOR);
    assert_eq!(heard.tracks, 1);
    assert_eq!(heard.albums, 1);
    assert_eq!(heard.artists, 1);

    let most = library.most_listened(Window::Week, 5)?;

    assert_eq!(
        most.tracks
            .first()
            .map(|row| (row.name.as_str(), row.listened)),
        Some(("Echoes", HEARD_FOR)),
        "the track heard for three minutes is not the one most listened to"
    );
    assert_eq!(
        most.albums.first().map(|row| row.name.as_str()),
        Some("Orbits")
    );
    assert_eq!(
        most.artists.first().map(|row| row.name.as_str()),
        Some("The Orbiters")
    );

    let drawn = library.listening_by_day(Window::Week)?;

    assert_eq!(
        drawn.last().map(|day| (day.plays, day.listened)),
        Some((1, HEARD_FOR)),
        "the day the play was counted on does not hold the time it was heard for"
    );
    Ok(())
}

#[test]
fn a_file_played_before_any_scan_saw_it_is_credited_with_the_play_and_the_time_heard_once_one_does()
-> Result<()> {
    const HEARD_FOR: Duration = Duration::from_secs(90);

    let tree = Tree::new();
    let unheld = tree.write(
        "later/echoes.wav",
        &Wav::new().text(TITLE, "Echoes").build(),
    );
    let library = Library::open_in_memory()?;

    tree.write(
        "scanned/seamus.wav",
        &Wav::new().text(TITLE, "Seamus").build(),
    );
    scan(
        &library,
        &ScanOptions {
            roots: vec![tree.path().join("scanned")],
            ..options(&tree)
        },
    )?;

    let location = MediaLocation::local(&unheld);
    let first = library
        .track_played(&location, None, Duration::ZERO)?
        .expect("an unheld play is still kept");

    assert_eq!(first.track, None);
    assert!(matches!(first.listen, resonate_library::Listen::Unheld(_)));

    library.listened(first.listen, HEARD_FOR)?;

    let second = library
        .track_played(&location, None, Duration::ZERO)?
        .expect("an unheld play is still kept");

    assert_ne!(second.listen, first.listen);

    let before = library.statistics(Window::Everything)?;

    assert_eq!(before.plays, 2);
    assert_eq!(before.tracks, 0);
    assert_eq!(
        before.listened, HEARD_FOR,
        "the time an unheld play was heard for was not kept"
    );

    scan(&library, &options(&tree))?;

    let row = library
        .track_at(&unheld, None)?
        .expect("the file a later scan found");

    assert_eq!(
        row.plays, 2,
        "the plays heard before the scan were not credited"
    );
    assert!(row.played.is_some());

    let after = library.statistics(Window::Everything)?;

    assert_eq!(after.plays, 2);
    assert_eq!(after.tracks, 1);
    assert_eq!(
        after.listened, HEARD_FOR,
        "the time heard was not carried onto the row"
    );

    scan(&library, &options(&tree))?;

    assert_eq!(
        library
            .track_at(&unheld, None)?
            .expect("the same row")
            .plays,
        2,
        "a play was credited twice"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_held_listen_whose_song_is_deleted_mid_play_is_still_spent_and_survives_a_restore() -> Result<()>
{
    const HEARD_AT_THE_START: Duration = Duration::from_secs(2);
    const HEARD_BY_THE_END: Duration = Duration::from_secs(120);

    let tree = Tree::new();
    let data = Tree::new();
    let cold = tree.write(
        "cold.wav",
        &Wav::new().text(TITLE, "Cold").text(ARTIST, "Ada").build(),
    );
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let counted = library
        .track_played(&MediaLocation::local(&cold), None, HEARD_AT_THE_START)?
        .expect("a counted play");

    assert!(matches!(counted.listen, resonate_library::Listen::Held(_)));

    let held = library.track_at(&cold, None)?.expect("held");

    library.delete_tracks(&[held.id])?;
    library.listened(counted.listen, HEARD_BY_THE_END)?;

    assert_eq!(
        library.statistics(Window::Everything)?.listened,
        HEARD_BY_THE_END,
        "the time heard after the delete did not reach the listen the delete kept"
    );

    fs::rename(data.path().join("Trash/files/cold.wav"), &cold).expect("a restored file");
    scan(&library, &options(&tree))?;

    let heard = library.statistics(Window::Everything)?;

    assert_eq!(heard.plays, 1);
    assert_eq!(heard.listened, HEARD_BY_THE_END);
    Ok(())
}

#[test]
fn an_unheld_listen_credited_mid_song_is_still_spent_and_never_spends_a_later_one() -> Result<()> {
    const HEARD_FIRST: Duration = Duration::from_secs(90);
    const HEARD_SECOND: Duration = Duration::from_secs(5);

    let tree = Tree::new();
    let credited = tree.write(
        "scanned/echoes.wav",
        &Wav::new().text(TITLE, "Echoes").build(),
    );
    let still_unheld = tree.write(
        "later/seamus.wav",
        &Wav::new().text(TITLE, "Seamus").build(),
    );
    let library = Library::open_in_memory()?;

    let first = library
        .track_played(&MediaLocation::local(&credited), None, Duration::ZERO)?
        .expect("an unheld play is still kept");

    scan(
        &library,
        &ScanOptions {
            roots: vec![tree.path().join("scanned")],
            ..options(&tree)
        },
    )?;

    let second = library
        .track_played(&MediaLocation::local(&still_unheld), None, Duration::ZERO)?
        .expect("an unheld play is still kept");

    assert!(matches!(first.listen, resonate_library::Listen::Unheld(_)));
    assert!(matches!(second.listen, resonate_library::Listen::Unheld(_)));
    assert_ne!(
        second.listen, first.listen,
        "a credited unheld listen's id was handed out again"
    );

    library.listened(first.listen, HEARD_FIRST)?;
    library.listened(second.listen, HEARD_SECOND)?;

    let heard = library.statistics(Window::Everything)?;
    let row = library
        .track_at(&credited, None)?
        .expect("the row the scan credited");

    assert_eq!(row.plays, 1);
    assert_eq!(heard.plays, 2);
    assert_eq!(
        heard.listened,
        HEARD_FIRST + HEARD_SECOND,
        "the time heard after the scan did not reach the credited listen"
    );
    Ok(())
}

#[test]
fn what_is_playing_is_billed_with_the_code_and_the_release_group_the_catalog_holds() -> Result<()> {
    const GROUP: &str = "2a9b3ef6-6e5f-3b51-9d4a-4f29fd2b6c41";

    let tree = Tree::new();
    let heard = tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Winter")
            .text(ISRC, "gb-aye-71-00195")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let grouped = beside(&database)
        .execute(
            "UPDATE albums SET release_group = ?1 WHERE title = 'Winter'",
            [GROUP],
        )
        .expect("the album row is writable");
    let billed = library
        .billed_as(&MediaLocation::local(heard), None)?
        .expect("a scanned track the service can be told of");

    assert_eq!(grouped, 1);
    assert_eq!(billed.isrc, Some(Isrc::new("GBAYE7100195")?));
    assert_eq!(billed.release_group, Some(Mbid::new(GROUP)?));
    Ok(())
}

#[test]
fn a_file_the_scan_prunes_keeps_its_listens_and_has_them_back_when_it_returns() -> Result<()> {
    let tree = Tree::new();
    let bytes = Wav::new().text(TITLE, "Echoes").build();

    let echoes = tree.write("echoes.wav", &bytes);

    tree.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&echoes), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }

    let before = library.statistics(Window::Everything)?;

    fs::remove_file(&echoes).expect("the file goes");
    scan(&library, &options(&tree))?;

    let after = library.statistics(Window::Everything)?;

    assert_eq!(titles(&all(&library)?), vec!["Dogs"]);
    assert_eq!(before.plays, 2);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);

    tree.write("echoes.wav", &bytes);
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.track_at(&echoes, None)?.expect("held again").plays,
        2
    );
    assert_eq!(library.statistics(Window::Everything)?.plays, 2);
    Ok(())
}

#[test]
fn a_song_back_in_a_scan_cancelled_partway_is_credited_its_listens_once_by_an_unchanged_rescan()
-> Result<()> {
    let tree = Tree::new();
    let bytes = Wav::new().text(TITLE, "Echoes").build();

    let echoes = tree.write("echoes.wav", &bytes);

    many_short_songs(&tree.path().join("many"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&echoes), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }
    fs::remove_file(&echoes).expect("the file goes");
    scan(&library, &options(&tree))?;
    tree.write("echoes.wav", &bytes);
    cancelled_once_a_batch_is_written(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    let arrived = library
        .track_at(&echoes, None)?
        .expect("the cancelled scan wrote the song back");

    assert_eq!(arrived.plays, 0, "the cancelled scan credited the listens");

    let unchanged = scan(&library, &options(&tree))?;

    assert_eq!(unchanged.added, 0);
    assert_eq!(unchanged.updated, 0);
    assert_eq!(
        library.track_at(&echoes, None)?.expect("held").plays,
        2,
        "the listens kept against the song were not credited"
    );
    assert_eq!(library.statistics(Window::Everything)?.plays, 2);

    scan(&library, &options(&tree))?;

    assert_eq!(library.track_at(&echoes, None)?.expect("held").plays, 2);
    assert_eq!(library.statistics(Window::Everything)?.plays, 2);
    Ok(())
}

#[test]
fn a_delivered_row_forgotten_keeps_its_listens() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;
    wanted_san_tropez(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));

    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let object = library.vault_objects()?[0].path.clone();

    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&object), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }

    let before = library.statistics(Window::Everything)?;

    assert!(library.forget_delivered(&object)?);

    let after = library.statistics(Window::Everything)?;

    assert_eq!(before.plays, 2);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);
    Ok(())
}
