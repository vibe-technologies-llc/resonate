use std::{fs, slice, thread, time::Duration};

use resonate_core::{AlbumId, MediaLocation, PlaylistId, QueueStamp, SourceId, Span};
use resonate_library::{
    AlbumQuery, Cut, Direction, Edit, Error, Kept, Library, Playing, PlaylistOrder, Result,
    RowOrder, SavedQuery, SortOrder,
};

use crate::support::{
    ALBUM, ALBUM_ARTIST, ARTIST, TITLE, Tree, Wav, all, beside, held, loaded, named, options, png,
    read_as, scan, scanned_playlist_tree, sortable_playlist_tree, stems,
};

fn named_as(library: &Library, named: Option<&str>) -> Result<Vec<String>> {
    Ok(library
        .playlists(PlaylistOrder::Name, Direction::Ascending, named)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect())
}

#[test]
fn a_playlist_keeps_the_order_rows_were_added_in() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let wanted: Vec<Cut> = ["b.wav", "a.wav", "c.wav"]
        .iter()
        .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
        .collect();

    let id = library.create_playlist("  Evening  ")?;

    assert_eq!(library.add_to_playlist(id, &wanted)?, 3);

    let listed = library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Evening", "the name was stored untrimmed");
    assert_eq!(listed[0].entries, 3);
    assert!(listed[0].duration.is_some_and(|played| !played.is_zero()));

    let entries = library.playlist_entries(id, None)?;
    let titles: Vec<Option<&str>> = entries
        .iter()
        .map(|entry| entry.track.as_ref().map(|track| track.title.as_str()))
        .collect();

    assert_eq!(titles, vec![Some("Dogs"), Some("Echoes"), Some("Sheep")]);
    assert_eq!(library.playlist_cuts(id)?, wanted);
    Ok(())
}

#[test]
fn playlists_are_read_a_page_at_a_time_in_the_order_they_are_listed_in() -> Result<()> {
    let library = Library::open_in_memory()?;

    for name in ["Echo", "Alpha", "Delta", "Bravo", "Charlie"] {
        library.create_playlist(name)?;
    }

    let names = |from, most| -> Result<Vec<String>> {
        Ok(library
            .playlists_within(PlaylistOrder::Name, Direction::Ascending, None, from, most)?
            .into_iter()
            .map(|playlist| playlist.name)
            .collect())
    };

    assert_eq!(names(0, 2)?, ["Alpha", "Bravo"]);
    assert_eq!(names(2, 2)?, ["Charlie", "Delta"]);
    assert_eq!(names(4, 2)?, ["Echo"]);
    assert!(names(5, 2)?.is_empty());
    Ok(())
}

#[test]
fn a_row_the_scan_has_never_seen_is_still_a_row() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let unscanned = MediaLocation::local(tree.path().join("nothing-scanned.wav"));

    let id = library.create_playlist("Mixed")?;

    library.add_to_playlist(
        id,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("a.wav"))),
            Cut::whole(unscanned.clone()),
        ],
    )?;

    let entries = library.playlist_entries(id, None)?;

    assert_eq!(entries.len(), 2);
    assert!(entries[0].track.is_some());
    assert_eq!(entries[1].location(), &unscanned);
    assert!(entries[1].track.is_none(), "an unscanned row grew a track");

    let listed = library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;

    assert_eq!(listed[0].entries, 2, "an unscanned row was not counted");
    Ok(())
}

#[test]
fn a_playlist_moves_and_loses_a_whole_span_of_rows_at_once() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let files = ["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"];

    for file in &files[3..] {
        tree.write(file, &Wav::new().text(TITLE, file).build());
    }
    library.add_to_playlist(
        id,
        &files
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert!(library.move_in_playlist(id, Span::between(0, 1), 3)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "a", "b", "e"],
        "a span moved down did not end on the row it landed on"
    );

    assert!(library.move_in_playlist(id, Span::between(2, 3), 1)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "a", "b", "d", "e"],
        "a span moved up did not start on the row it landed on"
    );

    assert!(
        !library.move_in_playlist(id, Span::between(1, 3), 2)?,
        "a span dropped on itself moved"
    );
    assert!(
        !library.move_in_playlist(id, Span::between(3, 5), 0)?,
        "a span running past the end moved"
    );

    assert!(library.remove_from_playlist(id, Span::between(1, 2))?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "e"],
        "a span removed did not close up behind it"
    );

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;

    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "e", "a"],
        "the positions left behind a span were not dense"
    );
    Ok(())
}

#[test]
fn a_playlist_gathers_and_loses_rows_chosen_apart_as_one_undo_step() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let files = ["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"];

    for file in &files[3..] {
        tree.write(file, &Wav::new().text(TITLE, file).build());
    }
    library.add_to_playlist(
        id,
        &files
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let apart = [Span::one(0), Span::one(2)];

    assert!(library.gather_in_playlist(id, &apart, 3)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "d", "a", "c", "e"],
        "rows chosen apart did not gather in order after the row they were dropped on"
    );

    assert!(library.undo()?.is_some());
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["a", "b", "c", "d", "e"],
        "gathering rows took more than one undo step"
    );

    assert!(!library.gather_in_playlist(id, &apart, 2)?);
    assert!(!library.gather_in_playlist(id, &apart, 9)?);

    assert_eq!(
        library.remove_runs_from_playlist(id, &[Span::one(0), Span::between(3, 4)])?,
        3
    );
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "c"]);

    assert!(library.undo()?.is_some());
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["a", "b", "c", "d", "e"],
        "removing rows chosen apart took more than one undo step"
    );

    assert_eq!(library.remove_runs_from_playlist(id, &[Span::one(9)])?, 0);
    Ok(())
}

#[test]
fn a_playlist_reorders_and_loses_the_row_it_is_told_to() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["a.wav", "b.wav", "c.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert!(library.move_in_playlist(id, Span::one(0), 2)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "c", "a"],
        "the moved row did not land where it was told"
    );

    assert!(library.move_in_playlist(id, Span::one(2), 1)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "a", "c"],
        "a row moved back up did not land where it was told"
    );

    assert!(library.remove_from_playlist(id, Span::one(1))?);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "c"]);

    assert!(!library.move_in_playlist(id, Span::one(0), 9)?);
    assert!(!library.move_in_playlist(id, Span::one(1), 1)?);
    assert!(!library.remove_from_playlist(id, Span::one(9))?);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "c"]);

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;

    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "c", "a"],
        "a row added after an edit did not land at the end"
    );
    Ok(())
}

#[test]
fn two_playlists_cannot_share_a_name_however_it_is_cased() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Evening")?;

    assert!(matches!(
        library.create_playlist("evening"),
        Err(Error::DuplicatePlaylist { .. })
    ));
    assert!(matches!(
        library.create_playlist("   "),
        Err(Error::UnnamedPlaylist)
    ));

    library.rename_playlist(id, "Evening")?;
    library.rename_playlist(id, "Morning")?;

    assert_eq!(
        library.playlist(id)?.map(|playlist| playlist.name),
        Some("Morning".to_owned())
    );
    assert_eq!(
        library.playlist_named("MORNING")?.map(|found| found.id),
        Some(id)
    );
    Ok(())
}

#[test]
fn a_name_is_one_name_whatever_alphabet_it_is_written_in() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Café")?;

    assert!(matches!(
        library.create_playlist("CAFÉ"),
        Err(Error::DuplicatePlaylist { .. })
    ));
    assert_eq!(
        library.playlist_named("café")?.map(|found| found.id),
        Some(id),
        "a name was addressed under a fold that reaches ASCII and nothing else"
    );
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, Some("CAFÉ"))?
            .len(),
        1,
        "the index narrowed under a fold that reaches ASCII and nothing else"
    );
    Ok(())
}

#[test]
fn a_name_is_one_name_however_its_accents_are_written() -> Result<()> {
    let library = Library::open_in_memory()?;
    let composed = "Caf\u{e9}";
    let combining = "Cafe\u{301}";

    let id = library.create_playlist(composed)?;

    assert_ne!(composed, combining, "the two spellings are the same bytes");
    assert!(
        matches!(
            library.create_playlist(combining),
            Err(Error::DuplicatePlaylist { .. })
        ),
        "a name written with a combining acute started a playlist of its own"
    );
    assert_eq!(
        library.playlist_named(combining)?.map(|found| found.id),
        Some(id),
        "a name written with a combining acute addressed nothing"
    );
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, Some(combining))?
            .len(),
        1,
        "the index narrowed on the spelling rather than the name"
    );

    let renamed = library.rename_playlist(id, "Cafe\u{301} Noir");

    assert!(renamed.is_ok(), "a rename in one spelling was refused");
    assert_eq!(
        library
            .playlist_named("caf\u{e9} noir")?
            .map(|found| found.id),
        Some(id),
        "a renamed playlist kept the fold of the spelling it was given"
    );
    Ok(())
}

#[test]
fn a_playlist_nothing_holds_is_never_the_one_in_play() -> Result<()> {
    let library = Library::open_in_memory()?;
    let unknown = PlaylistId::new(404)?;

    library.set_playing_playlist(loaded(unknown));

    assert_eq!(
        library.playing_playlist(QueueStamp::default()),
        None,
        "the cell reported a playlist the catalog does not hold"
    );
    Ok(())
}

#[test]
fn a_playlist_is_in_play_only_while_the_queue_is_the_one_it_was_loaded_as() -> Result<()> {
    let library = Library::open_in_memory()?;
    let evening = library.create_playlist("Evening")?;
    let as_loaded = QueueStamp::of(["a.wav", "b.wav"]);
    let after_an_edit = QueueStamp::of(["a.wav"]);

    library.set_playing_playlist(Some(Playing {
        playlist: evening,
        queue: as_loaded,
    }));

    assert_eq!(library.playing_playlist(as_loaded), Some(evening));
    assert_eq!(
        library.playing_playlist(after_an_edit),
        None,
        "a queue a row had left still read as the playlist it was loaded from"
    );
    assert_eq!(
        library.playing_playlist(as_loaded),
        Some(evening),
        "a queue put back the way it was loaded no longer read as the playlist"
    );
    Ok(())
}

#[test]
fn a_playlist_that_is_dropped_takes_its_rows_and_the_transport_with_it() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;
    library.set_playing_playlist(loaded(id));

    assert!(library.remove_playlist(id)?);
    assert_eq!(library.playing_playlist(QueueStamp::default()), None);
    assert!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .is_empty()
    );
    assert!(library.playlist_entries(id, None)?.is_empty());
    assert!(!library.remove_playlist(id)?);
    Ok(())
}

#[test]
fn forgetting_a_root_leaves_the_playlist_rows_that_came_from_it() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(id, slice::from_ref(&file))?;

    assert!(library.remove_root(tree.path())?);
    assert!(all(&library)?.is_empty());

    let entries = library.playlist_entries(id, None)?;

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].location(), &file.location);
    assert!(entries[0].track.is_none());
    Ok(())
}

#[test]
fn only_an_edit_moves_the_revision_a_bus_client_watches() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let created = library.playlists_revision();

    let id = library.create_playlist("Evening")?;
    let named = library.playlists_revision();

    assert_ne!(named, created, "creating a playlist moved nothing");

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;

    let filled = library.playlists_revision();

    assert_ne!(filled, named, "adding a row moved nothing");

    library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;
    library.playlist_entries(id, None)?;

    assert_eq!(library.playlists_revision(), filled, "a read moved it");

    assert!(!library.remove_from_playlist(id, Span::one(9))?);
    assert_eq!(
        library.playlists_revision(),
        filled,
        "an edit that changed nothing moved it"
    );
    Ok(())
}

#[test]
fn a_playlist_holds_local_files_only() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Evening")?;
    let remote = MediaLocation::new(
        SourceId::new("subsonic").expect("a lowercase name"),
        "track/1",
    );

    assert!(matches!(
        library.add_to_playlist(id, &[Cut::whole(remote)]),
        Err(Error::NotALocalFile { .. })
    ));
    assert!(library.playlist_entries(id, None)?.is_empty());
    Ok(())
}

#[test]
fn tidying_a_playlist_drops_only_the_rows_whose_files_have_gone() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["a.wav", "gone.wav", "b.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let filled = library.playlists_revision();

    assert_eq!(library.prune_playlist(id)?, 1);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["a", "b"]);
    assert_ne!(library.playlists_revision(), filled, "a tidy moved nothing");

    let tidied = library.playlists_revision();

    assert_eq!(library.prune_playlist(id)?, 0);
    assert_eq!(
        library.playlists_revision(),
        tidied,
        "a tidy that dropped nothing moved it"
    );
    Ok(())
}

#[test]
fn a_playlist_that_is_played_records_when_and_how_often() -> Result<()> {
    let library = Library::open_in_memory()?;
    let morning = library.create_playlist("Morning")?;
    let evening = library.create_playlist("Evening")?;

    assert!(
        library
            .playlist(evening)?
            .is_some_and(|found| found.played.is_none() && found.plays == 0),
        "a playlist nothing has played named a last play or a count"
    );

    library.set_playing_playlist(loaded(evening));

    let played = library.playlist(evening)?.and_then(|found| found.played);

    assert!(played.is_some(), "playing a playlist recorded nothing");
    assert_eq!(
        library.playlist(evening)?.map(|found| found.plays),
        Some(1),
        "playing a playlist counted nothing"
    );

    let by_play: Vec<String> = library
        .playlists(PlaylistOrder::Played, Direction::Ascending, None)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect();

    assert_eq!(
        by_play,
        vec!["Morning", "Evening"],
        "one nothing has played did not come first"
    );

    library.set_playing_playlist(None);

    assert_eq!(
        library.playlist(evening)?.and_then(|found| found.played),
        played,
        "clearing what is in play rewrote a last play"
    );
    assert_eq!(
        library.playlist(evening)?.map(|found| found.plays),
        Some(1),
        "clearing what is in play counted a play"
    );
    assert_eq!(morning, PlaylistId::new(1)?);
    Ok(())
}

#[test]
fn the_playlists_most_played_this_month_are_ordered_by_what_the_month_played() -> Result<()> {
    const TWO_MONTHS: i64 = 60 * 86_400 * 1_000_000_000;

    let tree = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    let anthems = library.create_playlist("Anthems")?;
    let carols = library.create_playlist("Carols")?;

    for _ in 0..3 {
        library.set_playing_playlist(loaded(carols));
    }
    beside(&database)
        .execute_batch(&format!(
            "UPDATE playlist_plays SET at = at - {TWO_MONTHS};"
        ))
        .expect("the plays are moved back two months");
    library.set_playing_playlist(loaded(anthems));

    assert_eq!(
        read_as(&library, PlaylistOrder::Plays, Direction::Descending)?,
        vec!["Carols", "Anthems"]
    );
    assert_eq!(
        read_as(
            &library,
            PlaylistOrder::PlaysThisMonth,
            Direction::Descending
        )?,
        vec!["Anthems", "Carols"]
    );
    Ok(())
}

#[test]
fn a_playlist_taking_the_id_of_one_discarded_starts_with_no_plays_this_month() -> Result<()> {
    let library = Library::open_in_memory()?;
    let anthems = library.create_playlist("Anthems")?;
    let carols = library.create_playlist("Carols")?;

    library.set_playing_playlist(loaded(anthems));
    for _ in 0..3 {
        library.set_playing_playlist(loaded(carols));
    }
    library.remove_playlist(carols)?;

    let ballads = library.create_playlist("Ballads")?;

    assert_eq!(
        ballads, carols,
        "the discarded playlist's id was not taken again"
    );

    assert_eq!(
        read_as(
            &library,
            PlaylistOrder::PlaysThisMonth,
            Direction::Descending
        )?,
        vec!["Anthems", "Ballads"]
    );
    Ok(())
}

#[test]
fn the_playlists_are_listed_in_whatever_order_is_asked_for() -> Result<()> {
    let library = Library::open_in_memory()?;
    let anthems = library.create_playlist("Anthems")?;
    let ballads = library.create_playlist("Ballads")?;
    let carols = library.create_playlist("Carols")?;

    for _ in 0..3 {
        library.set_playing_playlist(loaded(carols));
    }
    library.set_playing_playlist(loaded(anthems));
    library.rename_playlist(ballads, "Ballads and laments")?;

    assert_eq!(
        named(&library, PlaylistOrder::Name)?,
        vec!["Anthems", "Ballads and laments", "Carols"]
    );
    assert_eq!(
        named(&library, PlaylistOrder::Created)?,
        vec!["Anthems", "Ballads and laments", "Carols"],
        "the oldest did not come first"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Modified)?,
        vec!["Anthems", "Carols", "Ballads and laments"],
        "the last one edited did not come last"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Played)?,
        vec!["Ballads and laments", "Carols", "Anthems"],
        "one nothing has played did not come first"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Plays)?,
        vec!["Ballads and laments", "Anthems", "Carols"],
        "the most played did not come last"
    );

    assert_eq!(
        PlaylistOrder::Name.reads(),
        Direction::Ascending,
        "a listing by name opens back to front"
    );
    assert!(
        PlaylistOrder::ALL
            .into_iter()
            .filter(|order| *order != PlaylistOrder::Name)
            .all(|order| order.reads() == Direction::Descending),
        "an order the newest or the most belongs at the top of opens front to back"
    );

    assert_eq!(
        read_as(&library, PlaylistOrder::Plays, Direction::Descending)?,
        vec!["Carols", "Anthems", "Ballads and laments"],
        "the most played did not come first when the listing was asked to read down"
    );
    assert_eq!(
        read_as(&library, PlaylistOrder::Played, Direction::Descending)?,
        vec!["Anthems", "Carols", "Ballads and laments"],
        "one nothing has played did not come last when the listing was asked to read down"
    );
    assert_eq!(
        read_as(&library, PlaylistOrder::Name, Direction::Descending)?,
        vec!["Carols", "Ballads and laments", "Anthems"],
        "a listing by name would not read Z to A"
    );
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_as_the_library_grows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.save_query(
        "Everything",
        &SavedQuery {
            text: None,
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    let found = library.playlist(id)?.expect("the playlist was saved");

    assert_eq!(found.entries, 3, "the query did not count what it matches");
    assert!(found.duration.is_some_and(|played| !played.is_zero()));
    assert_eq!(
        found.query.as_ref().map(|query| query.sort),
        Some(SortOrder::Title)
    );

    let entries = library.playlist_entries(id, None)?;
    let titles: Vec<&str> = entries
        .iter()
        .map(|entry| entry.track.as_ref().expect("a scanned row").title.as_str())
        .collect();

    assert_eq!(titles, vec!["Dogs", "Echoes", "Sheep"]);

    tree.write("d.wav", &Wav::new().text(TITLE, "Animals").build());
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(4),
        "a track the scan added did not reach the saved query"
    );
    Ok(())
}

#[test]
fn a_saved_query_takes_its_rows_from_the_text_and_the_limit() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();

    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);

    let capped = library.save_query(
        "One dog",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: Some(1),
        },
    )?;

    assert_eq!(library.playlist_entries(capped, None)?.len(), 1);
    assert_eq!(
        library.playlist(capped)?.map(|found| found.entries),
        Some(1),
        "the count went past the limit the query holds"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_revised_where_it_stands() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();

    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);

    let saved = library.playlists_revision();

    let revised = SavedQuery {
        text: Some("echoes".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(1),
    };

    library.revise_query(id, "Dogs Live", &revised)?;

    assert_ne!(
        library.playlists_revision(),
        saved,
        "revising a search moved nothing"
    );

    let found = library.playlist(id)?.expect("the playlist is still there");

    assert_eq!(found.name, "Dogs Live", "a revision left the name behind");
    assert_eq!(found.query, Some(revised), "the query read back as it was");
    assert_eq!(found.entries, 1, "the count did not follow the new search");
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["a"]);
    Ok(())
}

#[test]
fn a_search_narrows_a_playlist_to_the_rows_it_matches() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["b.wav", "a.wav", "c.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let whole = library.playlist_entries(id, None)?;

    assert_eq!(
        whole.iter().map(|entry| entry.position).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "a row did not carry the place the list holds it in"
    );

    let narrowed = library.playlist_entries(id, Some("sheep"))?;

    assert_eq!(narrowed.len(), 1, "the search narrowed to the wrong rows");
    assert_eq!(
        narrowed[0].position, 2,
        "a narrowed row forgot where it stands in the list"
    );
    assert_eq!(stems(&[narrowed[0].cut.clone()]), vec!["c"]);

    assert_eq!(
        library.playlist_entries(id, Some("is:lossless"))?.len(),
        3,
        "a term did not reach a playlist's rows"
    );
    assert!(
        library
            .playlist_entries(id, Some("nothing here"))?
            .is_empty(),
        "a search that matches nothing still drew rows"
    );
    Ok(())
}

#[test]
fn a_playlist_row_no_scan_has_seen_matches_no_search() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let unscanned = tree.write("late.wav", &Wav::new().text(TITLE, "Sheep").build());

    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("c.wav"))),
            Cut::whole(MediaLocation::local(unscanned)),
        ],
    )?;

    assert_eq!(library.playlist_entries(id, None)?.len(), 2);

    let narrowed = library.playlist_entries(id, Some("sheep"))?;

    assert_eq!(
        stems(
            &narrowed
                .iter()
                .map(|entry| entry.cut.clone())
                .collect::<Vec<_>>()
        ),
        vec!["c"],
        "a row the catalog has never seen answered a search"
    );
    Ok(())
}

#[test]
fn a_search_narrows_the_index_by_the_words_it_holds() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();

    for name in ["Evening jazz", "Morning jazz", "Anthems"] {
        library.create_playlist(name)?;
    }

    assert_eq!(
        named_as(&library, Some("jazz"))?,
        vec!["Evening jazz", "Morning jazz"],
        "a word did not narrow the index by name"
    );
    assert_eq!(
        named_as(&library, Some("morning JAZZ"))?,
        vec!["Morning jazz"],
        "two words did not both have to hold, or the match was cased"
    );
    assert_eq!(
        named_as(&library, Some("is:hires"))?,
        vec!["Anthems", "Evening jazz", "Morning jazz"],
        "a term narrowed an index that has only a name to answer with"
    );
    assert!(
        named_as(&library, Some("nothing"))?.is_empty(),
        "a word nothing is named after still listed a playlist"
    );
    Ok(())
}

#[test]
fn a_listing_of_names_reads_the_same_order_as_the_listing_beside_it_and_stops_where_told()
-> Result<()> {
    let library = Library::open_in_memory()?;

    for name in ["Anthems", "Ballads", "Carols", "Dirges"] {
        library.create_playlist(name)?;
    }

    let names = |from, most| -> Result<Vec<String>> {
        Ok(library
            .playlist_names(PlaylistOrder::Name, Direction::Ascending, from, most)?
            .into_iter()
            .map(|playlist| playlist.name)
            .collect())
    };

    assert_eq!(names(0, None)?, named_as(&library, None)?);
    assert_eq!(names(1, Some(2))?, vec!["Ballads", "Carols"]);
    assert_eq!(names(0, Some(0))?, Vec::<String>::new());
    assert_eq!(names(4, None)?, Vec::<String>::new());
    assert_eq!(
        library
            .playlist_names(PlaylistOrder::Name, Direction::Descending, 0, Some(1))?
            .into_iter()
            .map(|playlist| playlist.name)
            .collect::<Vec<_>>(),
        vec!["Dirges"],
        "a listing read the other way round started at the same end"
    );
    Ok(())
}

#[test]
fn the_count_is_counted_rather_than_read_off_a_listing() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    assert_eq!(library.playlist_count(None)?, 0);

    let evening = library.create_playlist("Evening jazz")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.create_playlist("Morning jazz")?;
    library.save_query("Anthems", &SavedQuery::default())?;

    assert_eq!(
        library.playlist_count(None)?,
        3,
        "a playlist that fills itself was counted as its rows rather than as one playlist"
    );
    assert_eq!(
        library.playlist_count(Some("jazz"))?,
        2,
        "a count read past the narrowing the listing beside it takes"
    );
    assert_eq!(
        library.playlist_count(Some("nothing"))? as usize,
        named_as(&library, Some("nothing"))?.len()
    );
    assert_eq!(
        library.playlist_count(None)? as usize,
        named_as(&library, None)?.len(),
        "the count and the listing disagreed about how many playlists there are"
    );
    Ok(())
}

#[test]
fn the_lists_are_read_without_the_playlists_that_fill_themselves() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.create_playlist("Evening jazz")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.create_playlist("Morning jazz")?;
    library.save_query("Anthems", &SavedQuery::default())?;

    let lists = library.playlist_lists(PlaylistOrder::Name, Direction::Ascending)?;

    assert_eq!(
        lists
            .iter()
            .map(|list| list.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Evening jazz", "Morning jazz"],
        "a playlist that fills itself was offered as a list to put rows in"
    );
    assert!(
        lists.iter().all(|list| list.query.is_none()),
        "a list came back carrying a saved query"
    );
    assert_eq!(lists[0].entries, 2);
    assert_eq!(lists[1].entries, 0);

    let reversed = library.playlist_lists(PlaylistOrder::Name, Direction::Descending)?;

    assert_eq!(
        reversed
            .iter()
            .map(|list| list.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Morning jazz", "Evening jazz"]
    );
    Ok(())
}

#[test]
fn a_saved_query_and_a_search_both_have_to_hold() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();

    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);

    let narrowed = library.playlist_entries(id, Some("live"))?;

    assert_eq!(
        stems(
            &narrowed
                .iter()
                .map(|entry| entry.cut.clone())
                .collect::<Vec<_>>()
        ),
        vec!["d"],
        "the query's own text and the search did not both hold"
    );
    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(2),
        "narrowing the rows shown changed what the query says it holds"
    );
    Ok(())
}

#[test]
fn a_list_has_no_query_to_revise() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;

    assert!(matches!(
        library.revise_query(id, "Evening", &SavedQuery::default()),
        Err(Error::NotAQuery { .. })
    ));
    assert!(matches!(
        library.revise_query(PlaylistId::new(404)?, "Nothing", &SavedQuery::default()),
        Err(Error::UnknownPlaylist(_))
    ));
    assert_eq!(
        library.playlist_entries(id, None)?.len(),
        1,
        "a refused revision changed what the list holds"
    );
    Ok(())
}

#[test]
fn a_playlist_is_put_in_order_rather_than_moved_a_row_at_a_time() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["c.wav", "a.wav", "b.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["a", "c", "b"]);
    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?,
        0,
        "a list already in that order was written again"
    );

    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Descending)?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["b", "c", "a"]);

    library.sort_playlist(id, RowOrder::Artist, Direction::Ascending)?;

    assert_eq!(held(&library, id)?, vec!["a", "b", "c"]);

    library.sort_playlist(id, RowOrder::Length, Direction::Ascending)?;

    assert_eq!(held(&library, id)?, vec!["b", "a", "c"]);

    library.sort_playlist(id, RowOrder::File, Direction::Descending)?;

    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);
    Ok(())
}

#[test]
fn putting_a_list_in_order_leaves_a_row_no_scan_has_seen_at_the_end() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Mixed")?;

    library.add_to_playlist(
        id,
        &["c.wav", "d.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;

    assert_eq!(held(&library, id)?, vec!["a", "c", "d"]);

    library.sort_playlist(id, RowOrder::Title, Direction::Descending)?;

    assert_eq!(
        held(&library, id)?,
        vec!["c", "a", "d"],
        "an unscanned row did not stay at the end"
    );

    library.sort_playlist(id, RowOrder::File, Direction::Ascending)?;

    assert_eq!(
        held(&library, id)?,
        vec!["a", "c", "d"],
        "the file order left a row out"
    );
    Ok(())
}

#[test]
fn putting_a_list_in_order_moves_what_a_pane_redraws_on() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["c.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let added = library.playlists_revision();

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;

    let sorted = library.playlists_revision();

    assert_ne!(sorted, added, "putting a list in order redrew nothing");

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;

    assert_eq!(
        library.playlists_revision(),
        sorted,
        "a list already in that order redrew the pane"
    );
    Ok(())
}

#[test]
fn a_list_kept_in_an_order_puts_a_row_added_later_where_the_order_wants_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("b.wav"), file("c.wav")])?;

    assert_eq!(
        library.keep_playlist_in_order(
            id,
            Some(Kept {
                order: RowOrder::Title,
                reading: Direction::Ascending,
            })
        )?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["c", "b"]);

    library.add_to_playlist(id, slice::from_ref(&file("a.wav")))?;

    assert_eq!(
        held(&library, id)?,
        vec!["a", "c", "b"],
        "an added row landed at the end of a list kept in order"
    );

    let kept = library.playlist(id)?.expect("the playlist is there").kept;

    assert_eq!(
        kept,
        Some(Kept {
            order: RowOrder::Title,
            reading: Direction::Ascending,
        })
    );
    Ok(())
}

#[test]
fn a_list_kept_in_an_order_is_not_one_to_place_by_hand() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert!(matches!(
        library.move_in_playlist(id, Span::one(0), 2),
        Err(Error::KeptInOrder { .. })
    ));
    assert!(matches!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending),
        Err(Error::KeptInOrder { .. })
    ));
    assert_eq!(held(&library, id)?, vec!["a", "b", "c"]);

    assert!(library.remove_from_playlist(id, Span::one(1))?);
    assert_eq!(
        held(&library, id)?,
        vec!["a", "c"],
        "a kept order refused the edits that do not place a row"
    );
    Ok(())
}

#[test]
fn putting_a_list_that_is_already_in_hand_back_in_hand_writes_nothing() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("c.wav"), file("b.wav")])?;

    let unchanged = library.playlist(id)?.expect("the playlist").modified;

    assert_eq!(library.keep_playlist_in_order(id, None)?, 0);
    assert_eq!(
        library.playlist(id)?.expect("the playlist").modified,
        unchanged,
        "a list put back in hand it was already in was marked as changed"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "a list put back in hand it was already in left a step to walk back"
    );
    Ok(())
}

#[test]
fn a_span_past_the_end_of_a_list_takes_only_the_rows_there_are() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;

    assert!(!library.move_in_playlist(id, Span::one(usize::MAX), 0)?);
    assert!(!library.move_in_playlist(id, Span::one(0), usize::MAX)?);
    assert_eq!(positions(&library, id)?, vec![0, 1, 2]);
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);

    assert!(library.remove_from_playlist(id, Span::between(1, usize::MAX))?);
    assert_eq!(held(&library, id)?, vec!["c"]);
    assert_eq!(positions(&library, id)?, vec![0]);

    assert!(!library.remove_from_playlist(id, Span::between(1, usize::MAX))?);
    assert_eq!(positions(&library, id)?, vec![0]);
    Ok(())
}

fn positions(library: &Library, id: PlaylistId) -> Result<Vec<usize>> {
    Ok(library
        .playlist_entries(id, None)?
        .iter()
        .map(|entry| entry.position)
        .collect())
}

#[test]
fn letting_a_kept_order_go_leaves_the_rows_where_they_stand() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Descending,
        }),
    )?;

    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);

    assert_eq!(library.keep_playlist_in_order(id, None)?, 0);
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);
    assert_eq!(
        library.playlist(id)?.expect("the playlist is there").kept,
        None
    );

    assert!(library.move_in_playlist(id, Span::one(0), 2)?);
    assert_eq!(held(&library, id)?, vec!["b", "a", "c"]);

    library.add_to_playlist(id, slice::from_ref(&file("d.wav")))?;

    assert_eq!(
        held(&library, id)?,
        vec!["b", "a", "c", "d"],
        "a list back in hand put an added row somewhere other than the end"
    );
    Ok(())
}

#[test]
fn keeping_a_list_in_an_order_moves_what_a_pane_redraws_on() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.add_to_playlist(id, &[file("a.wav"), file("b.wav")])?;

    let added = library.playlists_revision();

    let kept = Kept {
        order: RowOrder::File,
        reading: Direction::Ascending,
    };

    assert_eq!(library.keep_playlist_in_order(id, Some(kept))?, 0);
    assert_ne!(
        library.playlists_revision(),
        added,
        "a list that took an order it was already in redrew nothing"
    );
    Ok(())
}

#[test]
fn an_imported_sheet_lands_in_the_order_a_list_is_kept_in() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::Title,
            reading: Direction::Ascending,
        }),
    )?;

    let sheet = tree.path().join("evening.m3u8");

    fs::write(
        &sheet,
        format!(
            "{}\n{}\n{}\n",
            tree.path().join("c.wav").display(),
            tree.path().join("b.wav").display(),
            tree.path().join("a.wav").display()
        ),
    )
    .expect("a writable sheet");

    assert_eq!(library.import_playlist(&sheet, Some("Evening"))?.added, 3);
    assert_eq!(held(&library, id)?, vec!["a", "c", "b"]);
    Ok(())
}

#[test]
fn a_saved_query_is_kept_in_the_order_it_asked_for_and_no_other() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();
    let id = library.save_query("Everything", &SavedQuery::default())?;

    assert!(matches!(
        library.keep_playlist_in_order(
            id,
            Some(Kept {
                order: RowOrder::Title,
                reading: Direction::Ascending,
            })
        ),
        Err(Error::NotAList { .. })
    ));
    assert_eq!(
        library.playlist(id)?.expect("the playlist is there").kept,
        None
    );
    Ok(())
}

#[test]
fn a_saved_query_is_not_a_list_to_edit() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let id = library.save_query("Everything", &SavedQuery::default())?;

    assert!(matches!(
        library.add_to_playlist(id, slice::from_ref(&file)),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.remove_from_playlist(id, Span::one(0)),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.move_in_playlist(id, Span::one(0), 1),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.prune_playlist(id),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending),
        Err(Error::NotAList { .. })
    ));
    assert_eq!(
        library.playlist_entries(id, None)?.len(),
        3,
        "a refused edit changed what the query answers"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_renamed_dropped_and_written_out_like_any_other() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.save_query(
        "Evening",
        &SavedQuery {
            text: Some("echoes".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert!(matches!(
        library.save_query("evening", &SavedQuery::default()),
        Err(Error::DuplicatePlaylist { .. })
    ));

    library.rename_playlist(id, "Morning")?;

    let sheet = tree.path().join("morning.m3u8");

    assert_eq!(library.export_playlist(id, &sheet)?.rows, 1);

    let written = fs::read_to_string(&sheet).expect("the sheet was written");

    assert!(written.contains("a.wav\n"), "{written}");
    assert!(written.contains(",Echoes\n"), "{written}");

    let read_back = library.import_playlist(&sheet, Some("A list"))?;

    assert!(
        library
            .playlist(read_back.id)?
            .is_some_and(|found| found.query.is_none()),
        "a sheet read back in became a query"
    );

    assert!(library.remove_playlist(id)?);
    assert!(library.playlist(id)?.is_none());
    Ok(())
}

#[test]
fn a_playlist_is_copied_into_another_rather_than_moved_out_of() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.add_to_playlist(anthems, slice::from_ref(&file("b.wav")))?;

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["b", "c", "a"],
        "a copy did not land at the end in the order it was taken"
    );
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a"],
        "a copy took the rows out of the playlist it came from"
    );

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["b", "c", "a", "c", "a"],
        "a second copy reconciled rather than appending"
    );
    Ok(())
}

#[test]
fn a_copy_refuses_the_playlist_it_came_from() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, slice::from_ref(&file))?;

    assert!(matches!(
        library.copy_playlist(evening, evening, None),
        Err(Error::IntoItself { .. })
    ));
    assert_eq!(
        held(&library, evening)?,
        vec!["a"],
        "a playlist copied into itself doubled"
    );

    let unknown = PlaylistId::new(404)?;

    assert!(matches!(
        library.copy_playlist(evening, unknown, None),
        Err(Error::UnknownPlaylist(_))
    ));
    assert!(matches!(
        library.copy_playlist(unknown, evening, None),
        Err(Error::UnknownPlaylist(_))
    ));

    let query = library.save_query("Everything", &SavedQuery::default())?;

    assert!(matches!(
        library.copy_playlist(evening, query, None),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn a_search_narrowing_a_saved_query_is_read_beside_it_rather_than_into_it() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let floyd = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("\"pink floyd".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(
        narrowed_to(&library, floyd, None)?,
        vec!["Dogs"],
        "an unbalanced quote did not close at the end of the text it was written in"
    );
    assert_eq!(
        narrowed_to(&library, floyd, Some("dogs"))?,
        vec!["Dogs"],
        "the typed words were swallowed into the phrase the saved text left open"
    );

    let dangling = library.save_query(
        "Dangling",
        &SavedQuery {
            text: Some("dogs or".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert!(
        narrowed_to(&library, dangling, Some("sheep"))?.is_empty(),
        "a trailing or alternated with a word the saved text never held"
    );
    Ok(())
}

fn narrowed_to(library: &Library, id: PlaylistId, matching: Option<&str>) -> Result<Vec<String>> {
    Ok(library
        .playlist_entries(id, matching)?
        .into_iter()
        .filter_map(|entry| entry.track.map(|track| track.title))
        .collect())
}

#[test]
fn a_saved_query_is_frozen_into_a_list_by_copying_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let query = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("artist:floyd".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    let kept = library.create_playlist("Anthems")?;

    assert_eq!(library.copy_playlist(query, kept, None)?, 1);
    assert_eq!(held(&library, kept)?, vec!["a"]);

    tree.write(
        "d.wav",
        &Wav::new()
            .text(TITLE, "Pigs")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.playlist(query)?.map(|found| found.entries),
        Some(2),
        "a scan did not reach the query the copy was taken from"
    );
    assert_eq!(
        held(&library, kept)?,
        vec!["a"],
        "the list a query was copied into followed the query afterwards"
    );
    Ok(())
}

#[test]
fn a_search_narrows_what_a_copy_takes() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.copy_playlist(evening, anthems, Some("dogs"))?, 1);
    assert_eq!(
        held(&library, anthems)?,
        vec!["a"],
        "a copy took rows the search did not match"
    );
    assert_eq!(
        library.copy_playlist(evening, anthems, Some("nothing at all"))?,
        0
    );
    assert_eq!(held(&library, anthems)?, vec!["a"]);
    Ok(())
}

#[test]
fn a_copy_into_a_kept_list_lands_where_the_order_puts_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("b.wav")])?;
    library.add_to_playlist(anthems, slice::from_ref(&file("a.wav")))?;
    library.keep_playlist_in_order(
        anthems,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["a", "b", "c"],
        "a copy into a kept list landed at the end rather than in its order"
    );
    Ok(())
}

#[test]
fn a_search_takes_the_rows_it_matched_out_of_a_playlist() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "b"],
        "a drop took the rows the search did not match"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "the rows left behind were not closed up"
    );

    assert_eq!(library.remove_matching(evening, "nothing at all")?, 0);
    assert_eq!(held(&library, evening)?, vec!["c", "b"]);

    assert_eq!(library.remove_matching(evening, "is:lossless")?, 2);
    assert!(
        held(&library, evening)?.is_empty(),
        "a search matching every row left one behind"
    );
    Ok(())
}

#[test]
fn a_row_no_scan_has_seen_survives_every_search_that_drops() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let unscanned = tree.write("late.wav", &Wav::new().text(TITLE, "Dogs").build());
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(
        evening,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("a.wav"))),
            Cut::whole(MediaLocation::local(unscanned)),
        ],
    )?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["late"],
        "a row the catalog has never seen answered a search that drops"
    );
    Ok(())
}

#[test]
fn a_saved_query_has_no_rows_to_drop() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let query = library.save_query("Floyd", &SavedQuery::default())?;

    assert!(matches!(
        library.remove_matching(query, "dogs"),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn a_kept_list_is_dropped_out_of_and_stays_in_its_order() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["b", "c"],
        "a kept list lost its order when a search dropped a row"
    );
    Ok(())
}

#[test]
fn what_a_search_dropped_is_put_back_where_it_stood() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(held(&library, evening)?, vec!["c", "b"]);
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Dropped),
        "the drop left nothing to put back"
    );

    let put_back = library.undo()?.expect("the drop is there to put back");

    assert_eq!(put_back.edit, Edit::Dropped);
    assert_eq!(put_back.playlist, evening);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "b"],
        "the row came back somewhere other than where it went from"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the rows put back were not dense again"
    );
    Ok(())
}

#[test]
fn folding_keeps_the_first_of_every_doubled_row_and_drops_the_rest() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(
        evening,
        &[
            file("c.wav"),
            file("a.wav"),
            file("c.wav"),
            file("b.wav"),
            file("c.wav"),
            file("a.wav"),
        ],
    )?;

    assert_eq!(library.fold_doubles(evening)?, 3);
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "b"],
        "folding kept a row other than the first of each file"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the rows left behind were not closed up"
    );
    Ok(())
}

#[test]
fn folding_a_playlist_that_holds_each_file_once_writes_nothing() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    let unchanged = library.playlist(evening)?.expect("the playlist").modified;

    assert_eq!(library.fold_doubles(evening)?, 0);
    assert_eq!(held(&library, evening)?, vec!["c", "a", "b"]);
    assert_eq!(
        library.playlist(evening)?.expect("the playlist").modified,
        unchanged,
        "a fold that dropped nothing still marked the playlist as changed"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "a fold that dropped nothing still left a step to walk back"
    );
    Ok(())
}

#[test]
fn tidying_drops_gone_and_doubled_rows_in_one_undoable_edit() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(
        evening,
        &[
            file("a.wav"),
            file("a.wav"),
            file("b.wav"),
            file("c.wav"),
            file("c.wav"),
        ],
    )?;
    fs::remove_file(tree.path().join("c.wav")).expect("the file can be removed");

    assert_eq!(library.tidy_playlist(evening)?, 3);
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);

    let put_back = library.undo()?.expect("the tidy is there to put back");

    assert_eq!(put_back.edit, Edit::Tidied);
    assert_eq!(held(&library, evening)?, vec!["a", "a", "b", "c", "c"]);

    assert_eq!(library.redo()?.map(|again| again.edit), Some(Edit::Tidied));
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn what_a_fold_took_out_is_put_back_where_it_stood() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("c.wav")])?;

    assert_eq!(library.fold_doubles(evening)?, 1);
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);

    let put_back = library.undo()?.expect("the fold is there to put back");

    assert_eq!(put_back.edit, Edit::Folded);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "c"],
        "the double came back somewhere other than where it went from"
    );

    assert_eq!(library.redo()?.map(|again| again.edit), Some(Edit::Folded));
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);
    Ok(())
}

#[test]
fn a_kept_list_is_folded_and_stays_in_its_order() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("c.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.fold_doubles(evening)?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["a", "c"],
        "a kept list lost its order when a fold dropped a row"
    );
    Ok(())
}

#[test]
fn a_saved_query_has_no_doubles_to_fold() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let query = library.save_query("Floyd", &SavedQuery::default())?;

    assert!(matches!(
        library.fold_doubles(query),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

fn listed_playlists(library: &Library) -> Result<Vec<Vec<String>>> {
    let mut every = Vec::new();

    for order in PlaylistOrder::ALL {
        for reading in Direction::ALL {
            every.push(
                library
                    .playlists(order, reading, None)?
                    .into_iter()
                    .map(|found| found.name)
                    .collect(),
            );
        }
    }

    Ok(every)
}

#[test]
fn a_pinned_playlist_leads_every_order() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    library.start_playlist("Morning", &[file("a.wav")])?;
    library.start_playlist("Evening", &[file("b.wav"), file("c.wav")])?;

    let night = library.start_playlist("Night", &[file("c.wav")])?;

    assert!(
        listed_playlists(&library)?
            .iter()
            .any(|listed| listed.first().map(String::as_str) != Some("Night")),
        "the listings led with Night before anything was pinned, so the pin proves nothing"
    );

    assert!(library.pin_playlist(night, true)?);
    assert!(
        library
            .playlist(night)?
            .and_then(|found| found.pinned)
            .is_some()
    );

    for listed in listed_playlists(&library)? {
        assert_eq!(
            listed.first().map(String::as_str),
            Some("Night"),
            "a pinned playlist did not lead the listing: {listed:?}"
        );
    }

    assert!(library.pin_playlist(night, false)?);
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .into_iter()
            .map(|found| found.name)
            .collect::<Vec<String>>(),
        vec!["Evening", "Morning", "Night"],
        "an unpinned playlist kept the front of the listing"
    );
    assert!(
        !library.pin_playlist(PlaylistId::MAX, true)?,
        "an id the catalog holds no playlist for answered as though a row had changed"
    );
    Ok(())
}

#[test]
fn a_smart_playlists_count_follows_the_catalog_it_is_counted_in() -> Result<()> {
    let tree = Tree::new();

    tree.write("a.wav", &Wav::new().text(TITLE, "Echoes").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let everything = library.save_query("Everything", &SavedQuery::default())?;

    let before = library
        .playlist(everything)?
        .expect("the playlist is there");

    assert_eq!(before.entries, 1);

    tree.write("b.wav", &Wav::new().text(TITLE, "Dogs").build());
    scan(&library, &options(&tree))?;

    let after = library
        .playlist(everything)?
        .expect("the playlist is there");

    assert_eq!(
        after.entries, 2,
        "a count kept through a scan that added a track"
    );
    Ok(())
}

#[test]
fn a_saved_query_reads_back_the_order_it_was_saved_with() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();

    for sort in SortOrder::ALL {
        for reading in Direction::ALL {
            let query = SavedQuery {
                text: None,
                sort,
                reading,
                limit: None,
            };
            let id = library.save_query(&format!("{sort:?} read {reading:?}"), &query)?;

            assert_eq!(
                library.playlist(id)?.and_then(|found| found.query),
                Some(query),
                "{sort:?} read {reading:?} did not come back as the order it was saved with"
            );
        }
    }
    Ok(())
}

#[test]
fn a_duplicated_playlist_holds_what_the_source_holds_under_a_free_name() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening =
        library.start_playlist("Evening", &[file("b.wav"), file("a.wav"), file("b.wav")])?;

    let copy = library.duplicate_playlist(evening)?;
    let second = library.duplicate_playlist(evening)?;

    let copied = library.playlist(copy)?.expect("the copy is there");

    assert_eq!(copied.name, "Evening (copy)");
    assert_eq!(
        library
            .playlist(second)?
            .expect("the second copy is there")
            .name,
        "Evening (copy 2)"
    );
    assert_eq!(
        library.playlist_cuts(copy)?,
        library.playlist_cuts(evening)?,
        "the copy holds other rows than the source, or in another order"
    );
    assert_eq!(
        library
            .playlist(evening)?
            .expect("the source is there")
            .entries,
        3,
        "duplicating took rows out of the source"
    );

    library
        .undo()?
        .expect("the second copy is there to take back");

    assert!(
        library.playlist(second)?.is_none(),
        "undo left the copy standing"
    );
    Ok(())
}

#[test]
fn a_duplicated_playlist_keeps_the_order_or_the_search_the_source_had() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let kept = Kept {
        order: RowOrder::Title,
        reading: Direction::Ascending,
    };

    let evening = library.start_playlist("Evening", &[file("a.wav"), file("c.wav")])?;

    library.keep_playlist_in_order(evening, Some(kept))?;

    let wanted = SavedQuery {
        text: Some("dogs".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(10),
    };
    let dogs = library.save_query("Dogs", &wanted)?;

    let kept_copy = library.duplicate_playlist(evening)?;
    let query_copy = library.duplicate_playlist(dogs)?;

    assert_eq!(
        library
            .playlist(kept_copy)?
            .expect("the copy is there")
            .kept,
        Some(kept),
        "the copy was left in hand"
    );

    library.add_to_playlist(kept_copy, &[file("b.wav")])?;

    let titles: Vec<String> = library
        .playlist_entries(kept_copy, None)?
        .into_iter()
        .filter_map(|entry| entry.track.map(|track| track.title))
        .collect();

    assert_eq!(titles, ["Dogs", "Echoes", "Sheep"]);

    let searched = library.playlist(query_copy)?.expect("the copy is there");

    assert_eq!(
        searched.query,
        Some(wanted),
        "the copy fills itself from another search"
    );
    assert!(matches!(
        library.add_to_playlist(query_copy, &[file("a.wav")]),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn duplicating_a_playlist_nothing_holds_is_refused() {
    let (_tree, library) = scanned_playlist_tree();
    let gone = PlaylistId::new(99).expect("an id above zero");

    assert!(matches!(
        library.duplicate_playlist(gone),
        Err(Error::UnknownPlaylist(id)) if id == gone
    ));
}

fn four_albums_three_of_them_covered() -> (Tree, Library) {
    let tree = Tree::new();

    for (album, art) in [
        ("Alpha", Some(64)),
        ("Beta", Some(65)),
        ("Gamma", Some(66)),
        ("Delta", None),
    ] {
        for index in 0..2 {
            let mut file = Wav::new()
                .text(TITLE, &format!("{album} {index}"))
                .text(ALBUM, album)
                .text(ALBUM_ARTIST, "Someone");

            if let Some(size) = art {
                file = file.picture(&png(size));
            }
            tree.write(&format!("{album}-{index}.wav"), &file.build());
        }
    }

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of eight files");
    (tree, library)
}

fn album_named(library: &Library, title: &str) -> AlbumId {
    library
        .albums(&AlbumQuery::default())
        .expect("the albums read back")
        .into_iter()
        .find(|album| album.title == title)
        .expect("the album was grouped")
        .id
}

#[test]
fn a_playlist_is_pictured_by_the_covered_albums_its_rows_reach_first() -> Result<()> {
    let (tree, library) = four_albums_three_of_them_covered();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.start_playlist(
        "Evening",
        &[
            file("Gamma-0.wav"),
            file("Delta-0.wav"),
            file("Alpha-0.wav"),
            file("Gamma-1.wav"),
            file("Beta-0.wav"),
        ],
    )?;
    let (alpha, beta, gamma) = (
        album_named(&library, "Alpha"),
        album_named(&library, "Beta"),
        album_named(&library, "Gamma"),
    );

    assert_eq!(
        library.playlist_pictures(evening, 4)?,
        vec![gamma, alpha, beta]
    );
    assert_eq!(library.playlist_pictures(evening, 2)?, vec![gamma, alpha]);

    let empty = library.start_playlist("Nothing yet", &[])?;

    assert!(library.playlist_pictures(empty, 4)?.is_empty());
    Ok(())
}

#[test]
fn a_capped_search_is_pictured_by_the_rows_it_holds_and_no_others() -> Result<()> {
    let (_tree, library) = four_albums_three_of_them_covered();
    let first = library.save_query(
        "First",
        &SavedQuery {
            text: None,
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: Some(2),
        },
    )?;
    let every = library.save_query("Every", &SavedQuery::default())?;

    assert_eq!(
        library.playlist_pictures(first, 4)?,
        vec![album_named(&library, "Alpha")],
        "the cap did not bound what pictures a search"
    );
    assert_eq!(library.playlist_pictures(every, 4)?.len(), 3);
    Ok(())
}

#[test]
fn the_pinned_playlists_are_listed_most_lately_pinned_first() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();
    let morning = library.create_playlist("Morning")?;
    let evening = library.create_playlist("Evening")?;

    library.create_playlist("Unpinned")?;

    assert!(library.pinned_playlists(8)?.is_empty());

    library.pin_playlist(morning, true)?;
    thread::sleep(Duration::from_millis(2));
    library.pin_playlist(evening, true)?;

    let named = |most| -> Result<Vec<String>> {
        Ok(library
            .pinned_playlists(most)?
            .into_iter()
            .map(|pinned| pinned.name)
            .collect())
    };

    assert_eq!(named(8)?, ["Evening", "Morning"]);
    assert_eq!(named(1)?, ["Evening"]);

    library.pin_playlist(evening, false)?;

    assert_eq!(named(8)?, ["Morning"]);
    Ok(())
}
