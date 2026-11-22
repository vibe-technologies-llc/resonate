use std::path::PathBuf;

use resonate_core::{MediaLocation, Span};
use resonate_library::{
    Cut, Direction, Edit, Error, Kept, Library, Maker, Result, RowOrder, SavedQuery, SortOrder,
};

use crate::support::{
    TITLE, Tree, Wav, held, loaded, options, scan, scanned_playlist_tree, sortable_playlist_tree,
};

#[test]
fn undoing_walks_back_one_edit_at_a_time() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.add_to_playlist(evening, &[file("b.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;
    library.remove_from_playlist(evening, Span::one(0))?;

    assert_eq!(held(&library, evening)?, vec!["b", "c"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Removed)
    );
    assert_eq!(held(&library, evening)?, vec!["a", "b", "c"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Ordered)
    );
    assert_eq!(held(&library, evening)?, vec!["c", "a", "b"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Added)
    );
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Added)
    );
    assert!(held(&library, evening)?.is_empty());

    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Started)
    );
    assert!(
        library.playlist(evening)?.is_none(),
        "undoing the start of a playlist left it behind"
    );
    assert_eq!(library.undo()?, None, "there was a step nothing had taken");
    Ok(())
}

#[test]
fn a_step_says_how_many_are_behind_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;

    assert_eq!(
        library.undoable().map(|undoable| undoable.behind),
        Some(2),
        "the step on offer did not count the start and the addition under it"
    );
    assert_eq!(library.undo()?.map(|put_back| put_back.behind), Some(2));
    assert_eq!(library.undoable().map(|undoable| undoable.behind), Some(1));
    assert_eq!(
        library.redoable().map(|again| again.behind),
        Some(0),
        "the one step walked back counted others behind it"
    );

    library.undo()?.expect("the addition is there to put back");
    library.undo()?.expect("the start is there to put back");

    assert_eq!(library.undoable(), None);
    assert_eq!(
        library.redoable().map(|again| again.behind),
        Some(2),
        "the steps walked back were not counted on the other stack"
    );
    Ok(())
}

#[test]
fn a_discarded_playlist_comes_back_under_the_id_it_had() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let kept = Kept {
        order: RowOrder::File,
        reading: Direction::Descending,
    };
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.keep_playlist_in_order(evening, Some(kept))?;

    let standing = library.playlist(evening)?.expect("the playlist is there");

    assert!(library.remove_playlist(evening)?);
    assert!(library.playlist(evening)?.is_none());

    let put_back = library.undo()?.expect("the discard is there to put back");

    assert_eq!(put_back.edit, Edit::Discarded);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        library.playlist(evening)?,
        Some(standing),
        "the playlist came back as something other than what went"
    );
    assert_eq!(held(&library, evening)?, vec!["c", "b", "a"]);
    Ok(())
}

#[test]
fn a_saved_query_comes_back_with_the_search_it_filled_itself_from() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let asked = SavedQuery {
        text: Some("dogs".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: None,
    };

    let floyd = library.save_query("Floyd", &asked)?;

    library.revise_query(
        floyd,
        "Barrett",
        &SavedQuery {
            text: Some("echoes".to_owned()),
            sort: SortOrder::Artist,
            reading: SortOrder::Artist.reads(),
            limit: Some(5),
        },
    )?;

    let put_back = library.undo()?.expect("the revision is there to put back");

    assert_eq!(put_back.edit, Edit::Revised);
    assert_eq!(
        put_back.name, "Floyd",
        "the name the revision took is not what came back"
    );

    let found = library.playlist(floyd)?.expect("the query is still there");

    assert_eq!(found.name, "Floyd");
    assert_eq!(found.query, Some(asked));
    Ok(())
}

#[test]
fn an_edit_that_moved_nothing_leaves_nothing_to_put_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav"), file("c.wav")])?;

    assert_eq!(
        library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?,
        0
    );
    assert_eq!(library.remove_matching(evening, "nothing at all")?, 0);
    assert_eq!(library.prune_playlist(evening)?, 0);
    assert!(!library.move_in_playlist(evening, Span::one(0), 0)?);
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "an edit that moved no row still left a step to walk back"
    );
    Ok(())
}

#[test]
fn a_play_counted_between_an_edit_and_its_undo_is_not_taken_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.remove_from_playlist(evening, Span::one(1))?;
    library.set_playing_playlist(loaded(evening));

    library.undo()?.expect("the removal is there to put back");

    let found = library.playlist(evening)?.expect("the playlist is there");

    assert_eq!(
        found.plays, 1,
        "the play the undo crossed was taken back with it"
    );
    assert!(found.played.is_some());
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn only_so_many_steps_are_held_to_walk_back() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let evening = library.create_playlist("hold 0")?;

    for step in 1..=40 {
        library.rename_playlist(evening, &format!("hold {step}"))?;
    }

    let mut walked = 0;

    while library.undo()?.is_some() {
        walked += 1;
    }

    assert_eq!(walked, 32, "the stack held a different number of steps");
    assert_eq!(
        library.playlist(evening)?.map(|found| found.name),
        Some("hold 8".to_owned()),
        "the steps that went were not the oldest ones"
    );
    Ok(())
}

#[test]
fn a_rename_is_a_step_that_holds_a_name_rather_than_the_rows_beneath_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let evening = library.create_playlist("hold 0")?;
    let rows: Vec<Cut> = (0..2_000)
        .map(|row| Cut::whole(MediaLocation::local(tree.path().join(format!("{row}.wav")))))
        .collect();

    library.add_to_playlist(evening, &rows)?;
    for step in 1..=32 {
        library.rename_playlist(evening, &format!("hold {step}"))?;
    }

    assert_eq!(
        library.undoable().map(|step| step.behind),
        Some(31),
        "the rows under a rename were counted against the stack's own bound"
    );

    let mut walked = 0;

    while library.undo()?.is_some() {
        walked += 1;
    }

    assert_eq!(walked, 32, "the stack held a different number of steps");

    let found = library.playlist(evening)?.expect("the playlist is there");

    assert_eq!(found.name, "hold 0");
    assert_eq!(
        found.entries, 2_000,
        "walking back a rename moved the rows under it"
    );
    Ok(())
}

#[test]
fn a_copy_and_an_import_are_each_one_step_to_walk_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;

    let morning = library.create_playlist("Morning")?;

    library.copy_playlist(evening, morning, None)?;

    assert_eq!(held(&library, morning)?, vec!["a", "b"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Copied)
    );
    assert!(
        held(&library, morning)?.is_empty(),
        "one copy took more than one step to walk back"
    );

    let sheet = tree.write(
        "night.m3u8",
        format!(
            "{}\n{}\n",
            tree.path().join("a.wav").display(),
            tree.path().join("c.wav").display()
        )
        .as_bytes(),
    );
    let read = library.import_playlist(&sheet, Some("Night"))?;

    assert_eq!(read.added, 2);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Imported)
    );
    assert!(
        library.playlist_named("Night")?.is_none(),
        "the sheet that made a playlist took more than one step to walk back"
    );
    Ok(())
}

#[test]
fn a_step_walked_back_is_put_back_again() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;
    library.remove_from_playlist(evening, Span::one(0))?;

    assert_eq!(held(&library, evening)?, vec!["b", "c"]);
    assert_eq!(
        library.redoable(),
        None,
        "there was a step nothing had walked"
    );
    assert_eq!(
        library.redo()?,
        None,
        "a step was put back that nothing took"
    );

    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Removed)
    );
    assert_eq!(held(&library, evening)?, vec!["a", "b", "c"]);
    assert_eq!(
        library.redoable().map(|again| again.edit),
        Some(Edit::Removed),
        "the step walked back was not there to put back again"
    );

    let again = library.redo()?.expect("the removal is there to do again");

    assert_eq!(again.edit, Edit::Removed);
    assert_eq!(again.playlist, evening);
    assert_eq!(again.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["b", "c"],
        "putting the edit back left the playlist somewhere else"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "the rows put back again were not dense"
    );
    assert_eq!(
        library.redo()?,
        None,
        "one redo put back more than one step"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Removed),
        "a step put back again was not there to walk back once more"
    );
    Ok(())
}

#[test]
fn walking_forward_crosses_every_step_walking_back_did() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.rename_playlist(evening, "Morning")?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;

    let mut walked = Vec::new();

    while let Some(put_back) = library.undo()? {
        walked.push(put_back.edit);
    }

    assert_eq!(
        walked,
        vec![Edit::Ordered, Edit::Renamed, Edit::Added, Edit::Started]
    );
    assert!(library.playlist(evening)?.is_none());

    let mut again = Vec::new();

    while let Some(done) = library.redo()? {
        again.push(done.edit);
    }

    assert_eq!(
        again,
        vec![Edit::Started, Edit::Added, Edit::Renamed, Edit::Ordered],
        "walking forward crossed other steps than walking back had"
    );

    let found = library.playlist(evening)?.expect("the playlist is back");

    assert_eq!(
        found.name, "Morning",
        "the name the edits left did not come back"
    );
    assert_eq!(held(&library, evening)?, vec!["a", "c"]);
    Ok(())
}

#[test]
fn an_edit_forgets_what_was_walked_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.remove_from_playlist(evening, Span::one(0))?;
    library.undo()?.expect("the removal is there to put back");

    assert!(library.redoable().is_some());

    library.rename_playlist(evening, "Morning")?;

    assert_eq!(
        library.redoable(),
        None,
        "an edit made after a step was walked back still offered to put it back"
    );
    assert_eq!(library.redo()?, None);
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn an_edit_that_moved_nothing_forgets_nothing_that_was_walked_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.undo()?.expect("the addition is there to put back");

    assert_eq!(library.prune_playlist(evening)?, 0);
    assert_eq!(
        library.redoable().map(|again| again.edit),
        Some(Edit::Added),
        "an edit that wrote nothing took away what was there to put back"
    );

    library.redo()?.expect("the addition is there to do again");

    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn a_discarded_playlist_goes_again_under_the_id_it_came_back_with() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;

    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    let standing = library.playlist(evening)?.expect("the playlist is there");

    assert!(library.remove_playlist(evening)?);

    library.undo()?.expect("the discard is there to put back");

    assert_eq!(library.playlist(evening)?, Some(standing));

    let again = library.redo()?.expect("the discard is there to do again");

    assert_eq!(again.edit, Edit::Discarded);
    assert_eq!(again.name, "Evening");
    assert!(
        library.playlist(evening)?.is_none(),
        "the playlist put back stayed after the discard was done again"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Discarded),
        "the discard done again left nothing to walk back"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_revised_again_after_the_revision_was_walked_back() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let revised = SavedQuery {
        text: Some("echoes".to_owned()),
        sort: SortOrder::Artist,
        reading: SortOrder::Artist.reads(),
        limit: Some(5),
    };
    let floyd = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    library.revise_query(floyd, "Barrett", &revised)?;
    library.undo()?.expect("the revision is there to put back");

    let again = library.redo()?.expect("the revision is there to do again");

    assert_eq!(again.edit, Edit::Revised);
    assert_eq!(
        again.name, "Barrett",
        "the revision put back named the query what it was before"
    );

    let found = library.playlist(floyd)?.expect("the query is still there");

    assert_eq!(found.name, "Barrett");
    assert_eq!(found.query, Some(revised));
    Ok(())
}

#[test]
fn rows_put_into_a_playlist_at_a_place_land_there_and_one_undo_takes_them_out() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.start_playlist("Evening", &[file("a.wav"), file("b.wav")])?;
    let names = |library: &Library| -> Result<Vec<String>> {
        Ok(library
            .playlist_entries(evening, None)?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .cut
                    .location
                    .as_path()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .collect())
    };

    assert_eq!(
        library.insert_into_playlist(evening, &[file("c.wav"), file("a.wav")], 1)?,
        2
    );
    assert_eq!(names(&library)?, ["a.wav", "c.wav", "a.wav", "b.wav"]);
    assert_eq!(
        library.insert_into_playlist(evening, &[file("b.wav")], 99)?,
        1
    );
    assert_eq!(
        names(&library)?,
        ["a.wav", "c.wav", "a.wav", "b.wav", "b.wav"]
    );

    library
        .undo()?
        .expect("the insert at the end is there to take back");
    library
        .undo()?
        .expect("the insert at a place is there to take back");

    assert_eq!(names(&library)?, ["a.wav", "b.wav"]);
    Ok(())
}

fn offered_playlist_tree() -> (Tree, PathBuf) {
    let tree = Tree::new();

    for (file, title) in [("a.wav", "Echoes"), ("b.wav", "Dogs"), ("c.wav", "Sheep")] {
        tree.write(file, &Wav::new().text(TITLE, title).build());
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database).expect("a catalog on disc");

    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, database)
}

#[test]
fn an_edit_a_model_made_is_taken_onto_the_windows_undo_and_walked_back_there() -> Result<()> {
    let (tree, database) = offered_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let model = Library::open(&database)?;
    let window = Library::open(&database)?;

    model.offer_edits_as_a_model();

    let evening = model.start_playlist("Evening", &[file("a.wav"), file("b.wav")])?;

    assert!(model.remove_from_playlist(evening, Span::one(0))?);

    model.rename_playlist(evening, "Night")?;

    assert_eq!(model.take_what_a_model_offered()?, 0);
    assert!(window.undoable().is_none());
    assert_eq!(window.take_what_a_model_offered()?, 3);
    assert_eq!(
        window.take_what_a_model_offered()?,
        0,
        "an edit taken once was offered again"
    );

    let offered = window
        .undoable()
        .expect("the rename is on the window's undo");

    assert_eq!(offered.edit, Edit::Renamed);
    assert_eq!(offered.made_by, Maker::AModel);
    assert_eq!(offered.behind, 2);

    window.undo()?.expect("the rename is put back");

    assert_eq!(
        window.redoable().map(|redone| redone.made_by),
        Some(Maker::AModel),
        "the redo of a model's edit was not said to be the model's"
    );

    window.undo()?.expect("the dropped row is put back");

    let named = window.playlist(evening)?.expect("the playlist stands");

    assert_eq!(named.name, "Evening");
    assert_eq!(named.entries, 2);

    window
        .undo()?
        .expect("the playlist the model started is taken away");

    assert!(window.playlist(evening)?.is_none());
    assert!(
        matches!(model.undo(), Err(Error::PlaylistChanged(id)) if id == evening),
        "the model walked back an edit the window had already walked back"
    );
    Ok(())
}

#[test]
fn an_edit_a_model_undid_or_another_process_wrote_over_is_not_taken() -> Result<()> {
    let (tree, database) = offered_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let model = Library::open(&database)?;
    let window = Library::open(&database)?;
    let command_line = Library::open(&database)?;

    model.offer_edits_as_a_model();

    let evening = model.start_playlist("Evening", &[file("a.wav")])?;

    assert_eq!(model.add_to_playlist(evening, &[file("b.wav")])?, 1);

    model.undo()?.expect("the model takes back its own add");
    model.rename_playlist(evening, "Night")?;
    model.undo()?.expect("the model takes back its rename");
    model.redo()?.expect("the model makes its rename again");

    assert_eq!(window.take_what_a_model_offered()?, 2);

    let offered = window
        .undoable()
        .expect("the rename made again is on the window's undo");

    assert_eq!(offered.edit, Edit::Renamed);
    assert_eq!(offered.behind, 1, "the add the model took back was offered");

    assert_eq!(model.add_to_playlist(evening, &[file("c.wav")])?, 1);
    assert_eq!(command_line.add_to_playlist(evening, &[file("b.wav")])?, 1);
    assert_eq!(
        window.take_what_a_model_offered()?,
        0,
        "an edit another process wrote over was taken"
    );
    assert_eq!(window.undoable().map(|step| step.edit), Some(Edit::Renamed));
    Ok(())
}

#[test]
fn undoing_an_edit_keeps_a_playlist_pinned() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.start_playlist("Evening", &[file("a.wav"), file("b.wav")])?;

    assert!(library.pin_playlist(evening, true)?);

    let pinned = library
        .playlist(evening)?
        .expect("the playlist is there")
        .pinned;

    assert!(pinned.is_some());

    assert!(library.remove_from_playlist(evening, Span::one(0))?);

    let put_back = library.undo()?.expect("the drop is there to put back");

    assert_eq!(put_back.playlist, evening);

    let after = library.playlist(evening)?.expect("the playlist is there");

    assert_eq!(after.entries, 2, "the row did not come back");
    assert_eq!(
        after.pinned, pinned,
        "undoing an edit wrote the playlist row back without what pinned it"
    );
    Ok(())
}

#[test]
fn a_pin_made_after_an_edit_survives_walking_the_edit_back() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.start_playlist("Evening", &[file("a.wav")])?;

    library.add_to_playlist(evening, &[file("b.wav")])?;

    assert!(library.pin_playlist(evening, true)?);

    library.undo()?.expect("the add is there to walk back");

    let after = library.playlist(evening)?.expect("the playlist is there");

    assert_eq!(after.entries, 1);
    assert!(
        after.pinned.is_some(),
        "walking back an add took a later pin away"
    );

    library.add_to_playlist(evening, &[file("b.wav")])?;

    assert!(library.pin_playlist(evening, false)?);

    library.undo()?.expect("the add is there to walk back");

    let after = library.playlist(evening)?.expect("the playlist is there");

    assert!(
        after.pinned.is_none(),
        "walking back an add put a pin taken off back"
    );
    Ok(())
}
