use std::{
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::Duration,
};

use notify::{
    Event, EventKind,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use resonate_library::{Favoured, Library, Overheard, Result, RootsWatch, ScanOptions};

use crate::support::{ARTIST, TITLE, Tree, Wav, moved, options, scan};

fn renamed(mode: RenameMode, path: &Path) -> Event {
    Event::new(EventKind::Modify(ModifyKind::Name(mode))).add_path(path.to_path_buf())
}

fn scanned_as_the_window_would(library: &Library, owed: Vec<PathBuf>) -> Result<()> {
    if owed.is_empty() {
        return Ok(());
    }

    let held = library.scan_what_is_held(ScanOptions {
        roots: owed,
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: true,
        workers: NonZeroUsize::MIN,
    })?;

    if let Some(handle) = held {
        let summary = handle.join()?;

        assert!(
            !summary.cancelled,
            "the watch's scan reported itself cancelled"
        );
    }
    Ok(())
}

fn followed_across_roots_as_heard(heard: impl Fn(&Path, &Path) -> Vec<Event>) -> Result<()> {
    let catalog = Tree::new();
    let source = Tree::new();
    let destination = Tree::new();
    let before = source.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Example Band")
            .build(),
    );

    source.write("marlow.wav", &Wav::new().text(TITLE, "Marlow").build());
    destination.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let library = Library::open(&catalog.path().join("library.db"))?;

    for tree in [&source, &destination] {
        scan(&library, &options(tree))?;
    }

    let slow_water = library
        .track_at(&before, None)?
        .expect("the scan stored the song");

    library.favour(Favoured::Track(slow_water.id), true)?;

    let after = destination.path().join("Example Band/Lantern.wav");

    moved(&before, &after);

    let overheard = Overheard::over(&[
        source.path().to_path_buf(),
        destination.path().to_path_buf(),
    ]);

    for event in heard(&before, &after) {
        overheard.hear(&event);
    }

    let forgotten_while_settling = overheard.taken_away(Duration::ZERO);
    let owed = overheard.settled(Duration::ZERO);
    let forgotten = overheard.taken_away(Duration::ZERO);

    library.forget_the_gone(&forgotten_while_settling)?;
    library.forget_the_gone(&forgotten)?;
    scanned_as_the_window_would(&library, owed.clone())?;

    let now = library
        .track_at(&after, None)?
        .expect("the row followed the file");

    assert_eq!(
        now.id, slow_water.id,
        "the file was catalogued as a new song"
    );
    assert!(now.favourite.is_some(), "the favourite stayed behind");
    assert!(library.track_at(&before, None)?.is_none());
    assert_eq!(owed.len(), 2, "both roots of the move were not owed a scan");
    assert!(forgotten_while_settling.is_empty());
    assert!(
        forgotten.is_empty(),
        "the side the file left was forgotten beside the scan that follows it"
    );
    Ok(())
}

#[test]
fn a_file_renamed_across_roots_with_each_side_heard_apart_keeps_its_id_and_favourite() -> Result<()>
{
    followed_across_roots_as_heard(|before, after| {
        vec![
            renamed(RenameMode::From, before),
            renamed(RenameMode::To, after),
        ]
    })
}

#[test]
fn a_file_renamed_across_roots_with_neither_side_named_keeps_its_id_and_favourite() -> Result<()> {
    followed_across_roots_as_heard(|before, after| {
        vec![
            renamed(RenameMode::Any, before),
            renamed(RenameMode::Any, after),
        ]
    })
}

#[test]
fn a_file_moved_across_roots_heard_as_taken_away_and_made_keeps_its_id_and_favourite() -> Result<()>
{
    followed_across_roots_as_heard(|before, after| {
        vec![
            Event::new(EventKind::Remove(RemoveKind::File)).add_path(before.to_path_buf()),
            Event::new(EventKind::Create(CreateKind::File)).add_path(after.to_path_buf()),
        ]
    })
}

#[test]
fn a_file_taken_away_with_nothing_arriving_is_forgotten_without_a_scan() -> Result<()> {
    let tree = Tree::new();
    let gone = tree.write("lantern.wav", &Wav::new().text(TITLE, "Lantern").build());
    let overheard = Overheard::over(&[tree.path().to_path_buf()]);

    fs::remove_file(&gone).expect("the file taken away");
    overheard.hear(&Event::new(EventKind::Remove(RemoveKind::File)).add_path(gone.clone()));

    assert!(overheard.settled(Duration::ZERO).is_empty());
    assert_eq!(overheard.taken_away(Duration::ZERO), vec![gone]);
    Ok(())
}

#[test]
fn a_song_added_while_its_root_was_uncovered_is_catalogued_once_the_watch_covers_it_again()
-> Result<()> {
    let catalog = Tree::new();
    let root = Tree::new();

    root.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let library = Library::open(&catalog.path().join("library.db"))?;

    scan(&library, &options(&root))?;

    let away = catalog.path().join("away");

    fs::rename(root.path(), &away).expect("the root taken away for a while");

    let mut watch = RootsWatch::over(&[root.path().to_path_buf()]).expect("a watch");

    assert!(watch.leaves_a_root_uncovered());

    fs::write(
        away.join("lantern.wav"),
        Wav::new().text(TITLE, "Lantern").build(),
    )
    .expect("a song added while nothing watched");
    fs::rename(&away, root.path()).expect("the root brought back");

    watch.cover_again();

    assert!(!watch.leaves_a_root_uncovered());

    let owed = watch.settled(Duration::ZERO);

    assert_eq!(owed, vec![root.path().to_path_buf()]);

    scanned_as_the_window_would(&library, owed)?;

    assert!(
        library
            .track_at(&root.path().join("lantern.wav"), None)?
            .is_some(),
        "the song added while the root was uncovered waits for another file to change"
    );
    Ok(())
}
