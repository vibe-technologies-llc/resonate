use std::{
    ffi::OsStr,
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::{FrameSpan, Frames, MediaLocation, TrackId};
use resonate_library::{
    AlbumQuery, Compacted, CoverArt, CoverSource, Cut, Encoding, Error, Form, ImageFormat,
    ImportOptions, Library, PollOptions, Pruned, Result, ScanOptions, Sources, Track, TrackQuery,
    Unwritten, Vault, VaultWeighingStats, Window,
};

use crate::support::{
    ALBUM, ARTIST, COMPILATION, Delivering, LABEL, MEDDLE_SHEET, Offering, TITLE, TRACK, Tree, Wav,
    a_real_picture, all, beside, link_to_directory, only_album, opened_with_a_vault, options,
    orbits, orbits_rows, orbits_tree, release_row, retagged, scan, titles, unlink_directory,
    vaulted, wanted_san_tropez,
};

#[test]
fn an_import_points_the_row_at_the_vault_and_leaves_the_file_where_it_stood() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM, "Meddle")
            .build(),
    );
    let bytes = fs::read(&path).expect("a source file");
    let stamp = fs::metadata(&path)
        .and_then(|held| held.modified())
        .expect("a stamped source file");

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.vaulted, 1);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(summary.plan.vaulted.len(), 1);
    assert_eq!(summary.plan.vaulted[0].wanted.form, Form::Flac);

    let rows = all(&library)?;

    assert_eq!(
        rows[0].location.as_path(),
        Some(path.as_path()),
        "a vaulted row stopped being named by its own file"
    );
    assert_eq!(rows[0].title, "Echoes");

    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault stands in for the row");
    let landed = stood.location.as_path().expect("a local object");

    assert!(landed.starts_with(vault.root()));
    assert_eq!(landed.extension().and_then(OsStr::to_str), Some("flac"));
    assert_eq!(stood.tags.title.as_deref(), Some("Echoes"));
    assert_eq!(stood.tags.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(stood.tags.album.as_deref(), Some("Meddle"));

    assert_eq!(fs::read(&path).expect("a source file"), bytes);
    assert_eq!(
        fs::metadata(&path)
            .and_then(|held| held.modified())
            .expect("a stamped source file"),
        stamp
    );

    let objects = library.vault_objects()?;

    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].form, Form::Flac);
    assert_eq!(objects[0].taken_from, path);
    assert!(objects[0].validated);
    Ok(())
}

#[test]
fn a_twenty_four_bit_master_kept_in_a_wider_wave_stands_in_as_twenty_four_bits() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .bits(24)
            .rate(192_000)
            .frames(19_200)
            .text(TITLE, "Echoes")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.vaulted, 1);
    assert_eq!(summary.plan.vaulted[0].wanted.form, Form::Wave);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault stands in for the row");

    assert_eq!(
        stood.bits,
        Some(24),
        "the object's width stood for the master's"
    );
    Ok(())
}

#[test]
fn a_vaulted_row_stands_in_under_the_credits_totals_and_notes_its_file_declared() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(TRACK, "6/6")
            .text(b"TCOM", "Roger Waters")
            .text(b"TCOP", "1971 Pink Floyd Music Ltd")
            .text(b"TIT1", "Meddle sessions")
            .text(LABEL, "Harvest")
            .text(COMPILATION, "1")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    assert_eq!(vaulted(&library, true)?.stats.vaulted, 1);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault stands in for the row");

    assert_eq!(stood.tags.title.as_deref(), Some("Echoes"));
    assert_eq!(stood.tags.track_number, Some(6));
    assert_eq!(stood.tags.track_total, Some(6), "the total was lost");
    assert_eq!(stood.tags.credits.composer.as_deref(), Some("Roger Waters"));
    assert_eq!(
        stood.tags.copyright.as_deref(),
        Some("1971 Pink Floyd Music Ltd")
    );
    assert_eq!(stood.tags.grouping.as_deref(), Some("Meddle sessions"));
    assert_eq!(stood.tags.label.as_deref(), Some("Harvest"));
    assert!(stood.tags.compilation, "the compilation mark was lost");

    let key = library.vault_objects()?[0].key;
    let released = library.release_the_rows_of(&[key])?;

    assert_eq!(released.released, 1);
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_none()
    );
    Ok(())
}

#[test]
fn a_rescan_after_an_import_leaves_the_row_in_the_vault_and_imports_nothing_again() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.added, 0);
    assert_eq!(stats.updated, 0);
    assert_eq!(stats.removed, 0);

    let again = vaulted(&library, true)?;

    assert_eq!(again.stats.walked, 0);
    assert_eq!(again.stats.vaulted, 0);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault still stands in for the row");

    assert!(
        stood
            .location
            .as_path()
            .expect("a local object")
            .starts_with(vault.root())
    );
    assert_eq!(library.vault_objects()?.len(), 1);
    Ok(())
}

#[test]
fn an_object_kept_under_an_older_encoder_is_weighed_again_and_stamped_with_this_one() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));

    let library = Library::open_with_vault(&database, Arc::clone(&vault))?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    assert_eq!(
        library.vault_objects()?[0].encoding,
        Encoding::OF_THIS_BUILD
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 0);

    beside(&database)
        .execute("UPDATE vault_objects SET encoding = 0", [])
        .expect("an object stamped as an older encoder's");

    let preview = vaulted(&library, false)?;

    assert_eq!(preview.stats.walked, 1);
    assert!(preview.plan.wanted[0].renewing);
    assert_eq!(preview.plan.wanted[0].form, Form::Flac);

    let renewed = vaulted(&library, true)?;

    assert_eq!(renewed.stats.vaulted, 1);
    assert_eq!(renewed.stats.passed, 0);

    let objects = library.vault_objects()?;

    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].encoding, Encoding::OF_THIS_BUILD);
    assert!(objects[0].path.is_file());

    let rows = all(&library)?;

    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_some(),
        "a renewed row no longer reaches its object"
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 0);
    Ok(())
}

#[test]
fn a_row_whose_source_has_gone_is_not_weighed_again_and_keeps_its_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));

    let library = Library::open_with_vault(&database, Arc::clone(&vault))?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&path).expect("the source goes");
    beside(&database)
        .execute("UPDATE vault_objects SET encoding = 0", [])
        .expect("an object stamped as an older encoder's");

    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.walked, 0);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(library.vault_objects()?.len(), 1);
    Ok(())
}

#[test]
fn an_import_on_several_workers_keeps_every_row_and_shared_audio_once() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    for index in 0..12 {
        tree.write(
            &format!("{index:02}.wav"),
            &Wav::new()
                .frames(4_410 + 500 * (index % 3))
                .text(TITLE, &format!("take {index}"))
                .build(),
        );
    }

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let asked = vaulted(&library, false)?;

    let summary = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                apply: true,
                workers: NonZeroUsize::new(4).expect("four workers"),
                ..ImportOptions::default()
            },
        )?
        .join()?;

    assert_eq!(summary.stats.vaulted, 12);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(
        summary
            .plan
            .vaulted
            .iter()
            .map(|kept| kept.wanted.from.clone())
            .collect::<Vec<_>>(),
        asked
            .plan
            .wanted
            .iter()
            .map(|wanted| wanted.from.clone())
            .collect::<Vec<_>>(),
        "the plan is not in the order the rows were asked for"
    );
    assert_eq!(library.vault_objects()?.len(), 3);
    assert_eq!(vault.holding().expect("a counted vault").objects, 3);

    for row in all(&library)? {
        let stood = library
            .stand_in()
            .stands_in(&row.location, row.span)
            .expect("the vault stands in for every row");

        assert!(stood.location.as_path().expect("a local object").is_file());
    }
    Ok(())
}

#[test]
fn a_file_a_vaulted_sheet_cuts_is_stood_in_for_only_by_the_cut_it_was_asked_as() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let file = tree.write("Meddle.wav", &Wav::new().frames(44_100).build());

    tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    assert_eq!(vaulted(&library, true)?.stats.vaulted, 3);

    let first = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.span.map(FrameSpan::start) == Some(Frames(0)))
        .expect("the sheet's first cut");
    let whole = MediaLocation::local(&file);
    let stand_in = library.stand_in();

    assert!(stand_in.stands_in(&first.location, first.span).is_some());
    assert!(
        stand_in.stands_in(&whole, None).is_none(),
        "the whole file was stood in for by its first cut"
    );
    assert!(
        stand_in
            .stands_in(&whole, Some(FrameSpan::starting(Frames(0))))
            .is_none(),
        "a span sharing the first cut's start alone was stood in for by it"
    );
    Ok(())
}

#[test]
fn a_vaulted_row_counts_its_plays_and_sits_in_a_playlist_as_the_file_it_came_from() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let row = all(&library)?.remove(0);
    let counted = library.track_played(&row.location, row.span, Duration::ZERO)?;

    assert!(counted.is_some(), "a play of a vaulted row was not counted");

    let playlist = library.create_playlist("Evening")?;

    library.add_to_playlist(playlist, &[Cut::of(&row)])?;

    let cuts = library.playlist_cuts(playlist)?;

    assert_eq!(cuts[0].location.as_path(), Some(path.as_path()));
    Ok(())
}

#[test]
fn a_vaulted_row_outlives_the_file_it_came_from_and_keeps_its_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&path).expect("the source goes");

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 0, "the only copy's row was pruned");

    let rows = all(&library)?;

    assert_eq!(rows.len(), 1);
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_some(),
        "the row no longer reaches its object"
    );
    assert_eq!(library.vault_objects_nothing_names()?.len(), 0);
    Ok(())
}

#[test]
fn forgetting_a_folder_leaves_its_vaulted_rows_and_keeps_the_listens_of_the_rows_it_forgets()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let echoes = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let dogs = tree.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    scan(&library, &options(&tree))?;
    fs::remove_file(&echoes).expect("the source goes");

    for path in [&echoes, &dogs, &dogs] {
        let counted = library
            .track_played(&MediaLocation::local(path), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }

    let before = library.statistics(Window::Everything)?;

    assert!(library.remove_root(tree.path())?);

    let rows = all(&library)?;
    let after = library.statistics(Window::Everything)?;

    assert_eq!(titles(&rows), vec!["Echoes"]);
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_some(),
        "the only copy's row no longer reaches its object"
    );
    assert!(library.vault_objects_nothing_names()?.is_empty());
    assert!(library.roots()?.is_empty());
    assert_eq!(before.plays, 3);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);

    library.add_root(tree.path())?;
    scan(&library, &options(&tree))?;

    assert_eq!(library.track_at(&dogs, None)?.expect("held again").plays, 2);
    assert_eq!(library.statistics(Window::Everything)?.plays, 3);
    Ok(())
}

#[test]
fn a_file_changed_where_it_stands_is_weighed_again_rather_than_played_from_its_old_object()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    fs::write(&path, Wav::new().text(TITLE, "Echoes (remastered)").build())
        .expect("the file is ripped again");

    let later = SystemTime::now() + Duration::from_secs(60);

    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(later))
        .expect("a later mtime");
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;

    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_none(),
        "a file ripped again went on playing its old object"
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 1);
    Ok(())
}

#[test]
fn a_release_and_a_prune_foretold_count_what_they_would_do_and_do_none_of_it() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let gone = tree.write(
        "dogs.wav",
        &Wav::new().text(TITLE, "Dogs").frames(8_820).build(),
    );
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&gone).expect("one source goes");

    let echoes_stands_in = |library: &Library| -> Result<bool> {
        let row = all(library)?
            .into_iter()
            .find(|row| row.title == "Echoes")
            .expect("the row is in the catalog");
        Ok(library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some())
    };

    let foretold = library.vault_release_foretold(&[])?;

    assert_eq!((foretold.released, foretold.stranded), (1, 1));
    assert!(
        echoes_stands_in(&library)?,
        "a release foretold already pointed a row back"
    );
    assert!(library.vault_objects_nothing_names()?.is_empty());

    assert_eq!(library.vault_prune_foretold()?, Pruned::default());

    let released = library.release_from_vault(&[])?;

    assert_eq!(released.released, foretold.released);
    assert!(!echoes_stands_in(&library)?);

    let objects = vault.holding().expect("a counted vault").objects;
    let foretold = library.vault_prune_foretold()?;

    assert_eq!(foretold.objects, 1);
    assert_eq!(
        vault.holding().expect("a counted vault").objects,
        objects,
        "a prune foretold took an object away"
    );
    assert_eq!(library.vault_objects_nothing_names()?.len(), 1);

    let pruned = library.prune_the_vault()?;

    assert_eq!(pruned.objects, foretold.objects);
    assert_eq!(library.vault_prune_foretold()?, Pruned::default());
    Ok(())
}

#[test]
fn a_release_points_a_row_back_at_its_own_file_and_leaves_the_only_copy_where_it_is() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let gone = tree.write(
        "dogs.wav",
        &Wav::new().text(TITLE, "Dogs").frames(8_820).build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&gone).expect("one source goes");

    let released = library.release_from_vault(&[])?;

    assert_eq!(released.released, 1);
    assert_eq!(
        released.stranded, 1,
        "the row the vault alone holds was released"
    );

    let stood_in = |title: &str| -> Result<bool> {
        let row = all(&library)?
            .into_iter()
            .find(|row| row.title == title)
            .expect("the row is still in the catalog");
        Ok(library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some())
    };

    assert!(
        !stood_in("Echoes")?,
        "a released row still played from the vault"
    );
    assert!(stood_in("Dogs")?, "the only copy stopped being reached");

    assert_eq!(
        library.vault_objects_nothing_names()?.len(),
        1,
        "the released row's object is not left for --prune"
    );
    assert_eq!(
        vaulted(&library, false)?.stats.walked,
        1,
        "a released row is not weighed again by the next import"
    );
    assert!(matches!(
        library.release_from_vault(&[PathBuf::from("/nowhere")]),
        Err(Error::NotARoot { .. })
    ));
    Ok(())
}

#[test]
fn a_vaulted_row_is_passed_over_by_a_retagging() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let summary = retagged(&library, false)?;

    assert_eq!(summary.retagging.passed_over.len(), 1);
    assert_eq!(summary.retagging.passed_over[0].why, Unwritten::Vaulted);
    assert!(summary.retagging.writes.is_empty());
    Ok(())
}

#[test]
fn an_albums_cover_moves_into_the_vault_and_reads_back_as_a_drawable_picture() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let album = library.albums(&AlbumQuery::default())?[0].id;

    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::File
    );

    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.covers, 1);
    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::Vault
    );
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);

    let drawn = library.cover_art(album)?.expect("a cover read back");

    assert_eq!(drawn.format, ImageFormat::Png);

    let went_in = image::load_from_memory(&art.bytes)
        .expect("a picture")
        .to_rgba8();
    let came_out = image::load_from_memory(&drawn.bytes)
        .expect("a picture")
        .to_rgba8();

    assert_eq!(went_in, came_out);
    Ok(())
}

#[test]
fn a_vault_opened_through_another_spelling_or_moved_keeps_every_object_and_cover() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let catalog = held.path().join("catalog.db");
    let first = held.path().join("vault");
    let through = held.path().join("through");

    fs::create_dir_all(&first).expect("a vault folder");
    if !link_to_directory(&first, &through) {
        return Ok(());
    }

    {
        let vault = Arc::new(Vault::make(&through).expect("a writable vault"));
        let library = Library::open_with_vault(&catalog, vault)?;

        scan(&library, &options(&tree))?;

        let summary = vaulted(&library, true)?;

        assert_eq!(summary.stats.vaulted, 1);
        assert_eq!(summary.stats.covers, 1);
    }

    let pruned = {
        let vault = Arc::new(Vault::open(&first).expect("a writable vault"));
        let library = Library::open_with_vault(&catalog, vault)?;
        library.prune_the_vault()?
    };

    assert_eq!(
        pruned,
        Pruned::default(),
        "a prune under another spelling took what the catalog names"
    );

    let moved = held.path().join("moved");

    unlink_directory(&through).expect("the second spelling taken away");
    fs::rename(&first, &moved).expect("a moved vault");

    let vault = Arc::new(Vault::open(&moved).expect("a writable vault"));
    let library = Library::open_with_vault(&catalog, Arc::clone(&vault))?;

    assert_eq!(library.prune_the_vault()?, Pruned::default());
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    assert_eq!(vault.holding().expect("a counted vault").objects, 1);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the moved vault still stands in for the row");

    assert!(
        stood
            .location
            .as_path()
            .expect("a local object")
            .starts_with(vault.root())
    );

    let album = library.albums(&AlbumQuery::default())?[0].id;

    assert!(library.cover_art(album)?.is_some());

    let objects = library.vault_objects()?;

    assert!(objects[0].path.starts_with(vault.root()));
    Ok(())
}

fn a_photographed_jpeg(width: u32, height: u32) -> CoverArt {
    let mut state = 0x9e37_79b9_u32;
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);

    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;

            let grain = (state >> 24) as f32 / 255.0 - 0.5;
            let (fx, fy) = (x as f32 / width as f32, y as f32 / height as f32);
            let sky = 0.55 + 0.35 * (fy * 3.1).cos();

            let hill = 0.5 + 0.4 * ((fx * 5.0).sin() * (fy * 4.0).cos());
            for sample in [
                sky * 0.9 + grain * 0.12,
                hill * 0.8 + grain * 0.12,
                sky + hill * 0.2 + grain * 0.12,
            ] {
                pixels.push((sample.clamp(0.0, 1.0) * 255.0) as u8);
            }
        }
    }

    let mut written = Vec::new();

    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut written, 85)
        .encode(&pixels, width, height, image::ExtendedColorType::Rgb8)
        .expect("a written landscape");

    CoverArt {
        format: ImageFormat::Jpeg,
        bytes: written,
    }
}

struct OlderCovers {
    library: Library,
    vault: Arc<Vault>,
    database: PathBuf,
    art: CoverArt,
    kept: PathBuf,
    _tree: Tree,
    _held: Tree,
}

impl OlderCovers {
    fn new() -> Result<Self> {
        let tree = Tree::new();
        let held = Tree::new();
        let art = a_photographed_jpeg(320, 240);

        for (file, title, album) in [
            ("echoes.wav", "Echoes", "Meddle"),
            ("fearless.wav", "Fearless", "Meddle"),
            ("time.wav", "Time", "The Dark Side of the Moon"),
        ] {
            tree.write(
                file,
                &Wav::new()
                    .text(TITLE, title)
                    .text(ALBUM, album)
                    .picture(&art.bytes)
                    .build(),
            );
        }

        let database = tree.path().join("library.db");
        let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));

        let library = Library::open_with_vault(&database, Arc::clone(&vault))?;

        scan(&library, &options(&tree))?;
        vaulted(&library, true)?;

        let named: String = beside(&database)
            .query_row("SELECT DISTINCT cover_path FROM albums", [], |row| {
                row.get(0)
            })
            .expect("one cover for every album");
        let transcoded = vault.at(Path::new(&named)).expect("a cover in the vault");

        assert_eq!(
            transcoded.extension().and_then(|held| held.to_str()),
            Some("jxl")
        );

        let kept = transcoded.with_extension("jpg");

        fs::remove_file(&transcoded).expect("the newer form taken away");
        fs::write(&kept, &art.bytes).expect("the JPEG an older build kept");

        let older = vault.within(&kept).expect("a cover in the vault");

        beside(&database)
            .execute(
                "UPDATE albums SET cover_path = ?1",
                [older.to_str().expect("a path the catalog can hold")],
            )
            .expect("the older cover named");

        Ok(Self {
            library,
            vault,
            database,
            art,
            kept,
            _tree: tree,
            _held: held,
        })
    }

    fn named(&self) -> Vec<String> {
        let database = beside(&self.database);
        let mut query = database
            .prepare("SELECT cover_path FROM albums ORDER BY id")
            .expect("a prepared query");
        query
            .query_map([], |row| row.get(0))
            .expect("the covers named")
            .map(|named| named.expect("a named cover"))
            .collect()
    }

    fn every_album_draws_the_very_jpeg(&self) -> Result<()> {
        let albums = self.library.albums(&AlbumQuery::default())?;

        assert_eq!(albums.len(), 2);

        for album in albums {
            assert_eq!(self.library.cover_art(album.id)?, Some(self.art.clone()));
        }
        Ok(())
    }

    fn every_album_names_a_file_that_is_there(&self) {
        for named in self.named() {
            let path = self
                .vault
                .at(Path::new(&named))
                .expect("a cover in the vault");

            assert!(path.is_file(), "an album names {named}, which is gone");
        }
    }
}

#[test]
fn the_jpeg_covers_an_older_build_kept_are_swapped_for_smaller_forms_every_album_follows()
-> Result<()> {
    let older = OlderCovers::new()?;
    let before = older.vault.holding().expect("a counted vault");

    assert_eq!(before.covers, 1);

    older.every_album_draws_the_very_jpeg()?;

    let foretold = older.library.vault_covers_compaction_foretold()?;

    assert_eq!(foretold.covers, 1);
    assert!(foretold.now < foretold.was);
    assert!(older.named().iter().all(|named| named.ends_with(".jpg")));
    assert!(older.kept.is_file());
    assert_eq!(older.vault.holding().expect("a counted vault"), before);

    let compacted = older.library.compact_the_vault_covers()?;

    assert_eq!(compacted.covers, 1);
    assert_eq!((compacted.was, compacted.now), (foretold.was, foretold.now));
    assert_eq!((compacted.kept, compacted.left), (0, 0));
    assert!(older.named().iter().all(|named| named.ends_with(".jxl")));
    assert!(!older.kept.exists());

    older.every_album_names_a_file_that_is_there();
    older.every_album_draws_the_very_jpeg()?;

    let after = older.vault.holding().expect("a counted vault");

    assert_eq!(after.covers, 1);
    assert_eq!(
        after.cover_bytes,
        before.cover_bytes - (compacted.was - compacted.now)
    );
    assert!(
        older
            .vault
            .verify_cover(&older.kept.with_extension("jxl"))
            .expect("a readable cover")
    );

    assert_eq!(
        older.library.compact_the_vault_covers()?,
        Compacted::default()
    );
    assert_eq!(older.library.prune_the_vault()?, Pruned::default());
    older.every_album_draws_the_very_jpeg()
}

#[test]
fn a_pass_stopped_between_landing_the_smaller_form_and_naming_it_is_finished_by_the_next()
-> Result<()> {
    let older = OlderCovers::new()?;

    older
        .vault
        .compact_cover(&older.kept)
        .expect("a landed form");

    let jxl = older.kept.with_extension("jxl");

    assert!(jxl.is_file() && older.kept.is_file());

    assert!(older.named().iter().all(|named| named.ends_with(".jpg")));

    older.every_album_names_a_file_that_is_there();
    older.every_album_draws_the_very_jpeg()?;

    assert_eq!(older.library.prune_the_vault()?, Pruned::default());

    let compacted = older.library.compact_the_vault_covers()?;

    assert_eq!((compacted.covers, compacted.left), (1, 0));
    assert!(older.named().iter().all(|named| named.ends_with(".jxl")));
    assert!(!older.kept.exists());

    older.every_album_names_a_file_that_is_there();
    older.every_album_draws_the_very_jpeg()
}

#[test]
fn a_pass_stopped_between_naming_the_smaller_form_and_taking_the_jpeg_away_is_finished_by_the_next()
-> Result<()> {
    let older = OlderCovers::new()?;

    older
        .vault
        .compact_cover(&older.kept)
        .expect("a landed form");

    let jxl = older.kept.with_extension("jxl");
    let named = older.vault.within(&jxl).expect("a cover in the vault");

    beside(&older.database)
        .execute(
            "UPDATE albums SET cover_path = ?1",
            [named.to_str().expect("a path the catalog can hold")],
        )
        .expect("the smaller form named");

    assert!(older.kept.is_file());

    older.every_album_names_a_file_that_is_there();
    older.every_album_draws_the_very_jpeg()?;

    assert_eq!(older.library.prune_the_vault()?, Pruned::default());
    assert!(older.kept.is_file());

    let compacted = older.library.compact_the_vault_covers()?;

    assert_eq!((compacted.covers, compacted.left), (1, 0));
    assert!(!older.kept.exists());

    older.every_album_names_a_file_that_is_there();
    older.every_album_draws_the_very_jpeg()
}

#[test]
fn a_prune_takes_away_a_cover_nothing_names_and_leaves_the_rest() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let stray = vault
        .keep_cover(&a_real_picture(8, 8))
        .expect("a cover nothing names");

    let pruned = library.prune_the_vault()?;

    assert_eq!(pruned.covers, 1);
    assert_eq!(pruned.objects, 0);
    assert!(!stray.path.exists());
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    Ok(())
}

#[test]
fn a_rescan_leaves_an_album_the_vault_covers_with_the_vaults_cover_alone() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    let track = |title: &str| {
        Wav::new()
            .text(TITLE, title)
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build()
    };

    tree.write("echoes.wav", &track("Echoes"));

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    tree.write("pillow.wav", &track("A Pillow of Winds"));
    scan(&library, &options(&tree))?;

    let album = library.albums(&AlbumQuery::default())?[0].id;

    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::Vault,
        "a rescan wrote the file's cover over the vault's"
    );
    Ok(())
}

#[test]
fn a_cover_the_vault_cannot_read_is_passed_over_and_the_import_carries_on() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(b"\x89PNG\r\n\x1a\nnot a picture at all")
            .build(),
    );
    tree.write(
        "time.wav",
        &Wav::new()
            .text(TITLE, "Time")
            .text(ALBUM, "The Dark Side of the Moon")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;

    assert_eq!(
        summary.stats.vaulted, 2,
        "one unreadable cover ended the import"
    );
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(summary.stats.covers_passed, 1);
    Ok(())
}

#[test]
fn a_cover_naming_no_format_the_vault_knows_is_passed_over_and_the_import_carries_on() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));

    let library = Library::open_with_vault(&database, vault)?;

    scan(&library, &options(&tree))?;
    beside(&database)
        .execute("UPDATE albums SET cover_format = 99", [])
        .expect("the catalog takes a format code");

    let summary = vaulted(&library, true)?;

    assert_eq!(
        summary.stats.vaulted, 1,
        "a cover of no known format ended the import"
    );
    assert_eq!(summary.stats.covers_passed, 1);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_vault_that_cannot_write_ends_the_import_rather_than_billing_every_row() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let tree = Tree::new();
    let held = Tree::new();

    for title in ["Echoes", "Time", "Money"] {
        tree.write(
            &format!("{title}.wav"),
            &Wav::new().text(TITLE, title).build(),
        );
    }

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let staging = held.path().join("staging");

    fs::set_permissions(&staging, fs::Permissions::from_mode(0o555))
        .expect("the staging folder takes a mode");
    if fs::write(staging.join("probe"), b"").is_ok() {
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o755))
            .expect("the staging folder takes a mode");
        eprintln!("skipping: this user writes a folder whatever its mode");
        return Ok(());
    }

    let refused = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                roots: Vec::new(),
                apply: true,
                ..ImportOptions::default()
            },
        )?
        .join();

    fs::set_permissions(&staging, fs::Permissions::from_mode(0o755))
        .expect("the staging folder takes a mode");

    assert!(
        matches!(refused, Err(Error::Vault { .. })),
        "a vault that could not write answered {refused:?}"
    );
    Ok(())
}

#[test]
fn a_cover_the_vault_already_holds_is_not_kept_a_second_time() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    let track = |title: &str| {
        Wav::new()
            .text(TITLE, title)
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build()
    };

    tree.write("echoes.wav", &track("Echoes"));

    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    tree.write("pillow.wav", &track("A Pillow of Winds"));
    scan(&library, &options(&tree))?;

    let again = vaulted(&library, true)?;

    assert_eq!(again.stats.vaulted, 1);
    assert_eq!(
        vault.holding().expect("a counted vault").covers,
        1,
        "the vault's own cover was kept again under another name"
    );
    Ok(())
}

#[test]
fn the_rows_a_sheet_cuts_are_weighed_against_their_share_of_the_file_and_play_as_themselves()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let file = tree.write("Meddle.wav", &Wav::new().frames(44_100).build());

    tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());

    let size = fs::metadata(&file).expect("a sized file").len();

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.walked, 3);
    assert!(
        summary.stats.was_bytes <= size,
        "three rows of one file counted {} bytes of a {size}-byte file",
        summary.stats.was_bytes
    );

    for row in all(&library)? {
        assert_eq!(row.location.as_path(), Some(file.as_path()));
        if let Some(stood) = library.stand_in().stands_in(&row.location, row.span) {
            assert_eq!(stood.tags.title.as_deref(), Some(row.title.as_str()));
        }
    }
    Ok(())
}

#[test]
fn an_object_nothing_names_any_more_is_what_a_prune_reads_back() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let echoes = tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    assert!(library.vault_objects_nothing_names()?.is_empty());

    assert!(library.remove_root(tree.path())?);
    assert!(library.vault_objects_nothing_names()?.is_empty());

    assert!(library.forget_delivered(&echoes)?);

    let loose = library.vault_objects_nothing_names()?;

    assert_eq!(loose.len(), 1);
    assert!(library.forget_vault_object(&loose[0].key)?);
    assert!(library.vault_objects()?.is_empty());
    Ok(())
}

#[test]
fn a_prune_takes_away_an_object_no_row_names_once_it_has_stood_a_while() -> Result<()> {
    const AN_HOUR: Duration = Duration::from_secs(60 * 60);

    let held = Tree::new();
    let (library, _vault) = opened_with_a_vault(&held)?;
    let stood = held.write("audio/ab/ab000000000000000000000000000001.flac", b"fLaC");
    let landing = held.write("audio/ab/ab000000000000000000000000000002.flac", b"fLaC");

    fs::File::options()
        .write(true)
        .open(&stood)
        .and_then(|file| file.set_modified(SystemTime::now() - AN_HOUR))
        .expect("the fixture object takes a time");

    let pruned = library.prune_the_vault()?;

    assert_eq!(pruned.objects, 1);
    assert!(
        !stood.exists(),
        "an object nothing names stood through a prune"
    );
    assert!(
        landing.exists(),
        "an object a poll may be about to note was taken from under it"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn an_object_a_prune_could_not_take_away_keeps_its_row_for_the_next() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let tree = Tree::new();
    let held = Tree::new();
    let echoes = tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    assert!(library.remove_root(tree.path())?);
    assert!(library.forget_delivered(&echoes)?);

    let loose = library.vault_objects_nothing_names()?;
    let folder = vault
        .root()
        .join(&loose[0].path)
        .parent()
        .expect("an object sits in a folder")
        .to_path_buf();

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555))
        .expect("the vault folder takes a mode");
    if fs::write(folder.join("probe"), b"").is_ok() {
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755))
            .expect("the vault folder takes a mode");
        eprintln!("skipping: this user writes into a folder whatever its mode");
        return Ok(());
    }

    let pruned = library.prune_the_vault();

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755))
        .expect("the vault folder takes a mode");

    let pruned = pruned?;

    assert_eq!((pruned.objects, pruned.left), (0, 1));
    assert_eq!(library.vault_objects()?.len(), 1);

    let pruned = library.prune_the_vault()?;

    assert_eq!((pruned.objects, pruned.left), (1, 0));
    assert!(library.vault_objects()?.is_empty());
    Ok(())
}

#[test]
fn a_vault_kept_inside_a_root_is_never_scanned_as_tracks_of_its_own() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let vault = Arc::new(Vault::make(tree.path().join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(vault)?;

    scan(&library, &options(&tree))?;

    assert_eq!(vaulted(&library, true)?.stats.vaulted, 1);

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    assert_eq!(titles(&all(&library)?), vec!["Echoes"]);
    assert!(all(&library)?.iter().all(|track| !track.delivered));
    Ok(())
}

#[test]
fn a_delivered_row_a_scan_walks_over_keeps_its_name_and_belongs_to_no_root() -> Result<()> {
    let tree = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );
    let orbits = orbits_tree();
    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::make(orbits.path().join("vault")).expect("a writable vault"));

    {
        let library = Library::open_with_vault(&database, vault)?;

        scan(&library, &options(&orbits))?;
        wanted_san_tropez(&library)?;

        let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
        let summary = library
            .poll(inbox.registered(), PollOptions::default())?
            .join()?;

        assert_eq!(summary.stats.kept, 1);
    }

    let library = Library::open(&database)?;

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&orbits)
        },
    )?;

    let rows = all(&library)?;
    let landed: Vec<Track> = rows
        .iter()
        .filter(|track| track.delivered)
        .cloned()
        .collect();

    assert_eq!(titles(&landed), vec!["San Tropez"]);
    assert_eq!(rows.len(), 4);
    Ok(())
}

#[test]
fn a_delivered_file_lands_in_the_vault_and_the_want_names_where_it_went() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let want = wanted_san_tropez(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered.clone())));
    let summary = library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);

    let objects = library.vault_objects()?;

    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].form, Form::Flac);
    assert!(objects[0].path.starts_with(vault.root()));
    assert!(
        delivered.is_file(),
        "the delivered file is left where it stood"
    );

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&objects[0].path).to_uri().as_str())
    );

    let album = only_album(&library)?;
    let row = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    let held = row
        .track
        .expect("the delivery is paired with the row it was wanted for");

    assert_eq!(wants[0].held, Some(held));
    assert!(
        library
            .missing_tracks(None, None)?
            .iter()
            .all(|missing| missing.title != "San Tropez")
    );

    let track = library.track(held)?.expect("the delivery is a track row");

    assert_eq!(track.title, "San Tropez");
    assert_eq!(track.album_id, Some(album.id));
    assert_eq!(track.location.as_path(), Some(objects[0].path.as_path()));
    assert!(
        track.delivered,
        "a row a provider delivered read back as scanned"
    );
    assert_eq!(
        titles(&library.search("Tropez", 10)?.tracks),
        vec!["San Tropez"]
    );

    let stood = library
        .stand_in()
        .stands_in(&track.location, track.span)
        .expect("the vault stands in for the delivered row");

    assert_eq!(stood.location.as_path(), Some(objects[0].path.as_path()));
    assert_eq!(stood.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(stood.tags.album.as_deref(), Some("Orbits"));

    scan(&library, &options(&orbits))?;

    assert!(
        library.track(held)?.is_some(),
        "a scan of the roots took away a row that belongs to none"
    );

    let summary = library
        .poll(
            inbox.registered(),
            PollOptions {
                every_want: true,
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert_eq!(summary.stats.asked, 0, "a held want was asked about again");

    library.unwant(want)?;

    assert_eq!(library.prune_the_vault()?.objects, 0);
    assert!(
        objects[0].path.is_file(),
        "a prune took away the object a delivered row is played from"
    );
    Ok(())
}

#[test]
fn a_delivered_row_is_forgotten_by_its_path_and_its_want_is_due_again() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&orbits))?;

    let want = wanted_san_tropez(&library)?;
    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));

    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let object = library.vault_objects()?[0].path.clone();
    let scanned = all(&library)?
        .into_iter()
        .find(|track| track.title != "San Tropez")
        .expect("a scanned row");

    assert!(
        !library.forget_delivered(scanned.location.as_path().expect("a local row"))?,
        "a row scanned from a root was forgotten as a delivery"
    );
    assert!(library.forget_delivered(&object)?);
    assert!(!library.forget_delivered(&object)?);

    assert!(
        all(&library)?
            .iter()
            .all(|track| track.title != "San Tropez")
    );
    assert!(library.track(scanned.id)?.is_some());

    let wants = library.wants()?;

    assert_eq!(wants[0].id, want);
    assert_eq!((wants[0].held, wants[0].offered.as_deref()), (None, None));
    assert!(library.is_a_want_due(PollOptions::default())?);
    assert_eq!(library.prune_the_vault()?.objects, 1);
    Ok(())
}

#[test]
fn one_object_delivered_for_two_wants_is_searched_for_by_the_row_it_stayed() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let scanned = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&scanned))?;

    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "San Tropez", Vec::new()));
    rows.push(release_row(5, "Seamus", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    for missing in library.release_tracks(album.id)? {
        if missing.title == "San Tropez" || missing.title == "Seamus" {
            library.want(missing.id)?;
        }
    }

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));

    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let landed: Vec<TrackId> = library
        .wants()?
        .into_iter()
        .filter_map(|want| want.held)
        .collect();

    assert_eq!(landed.len(), 2);
    assert_eq!(landed[0], landed[1], "one object became two track rows");

    let track = library
        .track(landed[0])?
        .expect("the delivery is a track row");
    let other = if track.title == "Seamus" {
        "San Tropez"
    } else {
        "Seamus"
    };

    assert_eq!(
        titles(&library.search(&track.title, 10)?.tracks),
        vec![track.title.clone()]
    );
    assert!(
        library.search(other, 10)?.tracks.is_empty(),
        "the row was indexed under the later want's names"
    );
    Ok(())
}

#[test]
fn weighing_a_vault_reads_every_object_back_and_notes_it_validated() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());
    tree.write(
        "dogs.wav",
        &Wav::new().text(TITLE, "Dogs").frames(8_820).build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let weighed = library.weigh_the_vault(false)?.join()?;

    assert_eq!(weighed.objects, 2);
    assert_eq!(weighed.objects_held, 2);
    assert!(weighed.objects_moved.is_empty());
    assert!(weighed.unreached.is_empty());
    assert!(weighed.mended.is_none());
    assert!(!weighed.has_something_to_mend());
    assert!(
        library
            .vault_objects()?
            .iter()
            .all(|object| object.validated)
    );
    Ok(())
}

#[test]
fn a_weighing_counts_what_it_has_read_back_against_everything_it_will() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());
    tree.write(
        "dogs.wav",
        &Wav::new().text(TITLE, "Dogs").frames(8_820).build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let handle = library.weigh_the_vault(false)?;
    let watched = Arc::clone(handle.progress());
    let weighed = handle.join()?;

    assert!(!weighed.cancelled);
    assert!(!watched.is_cancelled());
    assert_eq!(
        watched.snapshot(),
        VaultWeighingStats {
            to_weigh: 2,
            weighed: 2
        }
    );
    Ok(())
}

#[test]
fn a_weighing_stopped_early_says_so_and_mends_nothing() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    for index in 0..40 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Song {index}"))
                .frames(4_410 + index)
                .build(),
        );
    }

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let broken = library.vault_objects()?.remove(0);

    fs::write(&broken.path, b"no longer what went in").expect("an object overwritten");

    let handle = library.weigh_the_vault(true)?;
    let watched = Arc::clone(handle.progress());

    handle.cancel();

    let weighed = handle.join()?;

    assert!(watched.is_cancelled());
    assert!(weighed.cancelled, "the weighing outran the cancel");
    assert!(watched.snapshot().weighed < watched.snapshot().to_weigh);
    assert_eq!(weighed.objects, 40);
    assert!(weighed.objects_held + (weighed.objects_moved.len() as u64) < weighed.objects);
    assert!(weighed.mended.is_none());
    assert!(weighed.reframed.is_none());

    let row = all(&library)?.remove(0);

    assert!(
        library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some(),
        "a stopped mend pointed a row back"
    );
    Ok(())
}

#[test]
fn weighing_an_empty_vault_finds_nothing_to_weigh() -> Result<()> {
    let held = Tree::new();
    let (library, _vault) = opened_with_a_vault(&held)?;

    let weighed = library.weigh_the_vault(true)?.join()?;

    assert!(weighed.held_nothing());
    assert!(weighed.mended.is_none());
    Ok(())
}

#[test]
fn weighing_a_vault_without_a_vault_is_refused() -> Result<()> {
    let library = Library::open_in_memory()?;

    assert!(matches!(
        library.weigh_the_vault(false),
        Err(resonate_library::Error::NoVault)
    ));
    Ok(())
}

#[test]
fn an_object_that_no_longer_reads_back_is_found_and_only_mended_when_asked() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();

    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let object = library.vault_objects()?.remove(0);

    fs::write(&object.path, b"no longer what went in").expect("an object overwritten");

    let found = library.weigh_the_vault(false)?.join()?;

    assert_eq!(found.objects_moved, std::slice::from_ref(&object.path));
    assert_eq!(found.objects_held, 0);
    assert!(found.has_something_to_mend());
    assert!(found.mended.is_none());

    let row = all(&library)?.remove(0);

    assert!(
        library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some(),
        "a check that only looked pointed a row back"
    );

    let mended = library
        .weigh_the_vault(true)?
        .join()?
        .mended
        .expect("the mending is told");

    assert_eq!(mended.released.released, 1);
    assert_eq!(mended.released.stranded, 0);
    assert!(
        library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_none()
    );
    Ok(())
}
