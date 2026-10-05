use std::{
    collections::BTreeSet,
    env, fs,
    num::NonZeroUsize,
    os,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    thread,
};

use resonate_core::{FrameSpan, Frames, MediaLocation, PlaylistId, Span};
use resonate_library::{
    Cut, Direction, Error, Library, PlaylistOrder, Result, ScanOptions, ScanStats,
};

const RATE: u32 = 44_100;

const CHANNELS: u16 = 2;

const BITS: u16 = 16;

const PAST_WHAT_THE_UNDO_STACK_HOLDS: usize = 50_001;

const DUPLICATING_THREADS: usize = 4;

const DUPLICATES_EACH: usize = 6;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-playlists-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");

        Self {
            root: root
                .canonicalize()
                .expect("the temporary directory has a canonical path"),
        }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, bytes).expect("a writable temporary file");
        path
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn wav(frames: u32) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let data = vec![0_u8; frames as usize * usize::from(block_align)];

    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn scan(library: &Library, root: &Path, follow_symlinks: bool) -> Result<ScanStats> {
    let summary = library
        .scan(ScanOptions {
            roots: vec![root.to_path_buf()],
            incremental: true,
            follow_symlinks,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?;

    assert!(!summary.cancelled, "the scan reported itself cancelled");
    Ok(summary.stats)
}

fn whole(path: impl AsRef<Path>) -> Cut {
    Cut::whole(MediaLocation::local(path.as_ref()))
}

fn paths(library: &Library, id: PlaylistId) -> Result<Vec<PathBuf>> {
    Ok(library
        .playlist_cuts(id)?
        .into_iter()
        .filter_map(|cut| cut.location.as_path().map(Path::to_path_buf))
        .collect())
}

fn held(library: &Library, id: PlaylistId) -> Result<u32> {
    Ok(library.playlist(id)?.map_or(0, |found| found.entries))
}

#[test]
fn an_edit_to_a_long_playlist_holds_only_the_rows_from_where_it_reached() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows: Vec<Cut> = (0..PAST_WHAT_THE_UNDO_STACK_HOLDS)
        .map(|row| whole(format!("/music/{row}.wav")))
        .collect();
    let id = library.start_playlist("Long", &rows)?;
    let before = library.playlist_cuts(id)?;

    library.add_to_playlist(id, &[whole("/music/added.wav")])?;
    let appended = library.playlist_cuts(id)?;
    let near_the_end = appended.len() - 20;
    library.remove_from_playlist(id, Span::between(near_the_end, near_the_end + 2))?;
    let removed = library.playlist_cuts(id)?;
    library.move_in_playlist(
        id,
        Span::between(near_the_end, near_the_end + 1),
        removed.len() - 1,
    )?;

    assert_eq!(
        library.undoable().map(|step| step.behind),
        Some(3),
        "a step holding the whole playlist pushed the ones under it off the stack"
    );

    let moved = library.playlist_cuts(id)?;
    assert_ne!(moved, removed);

    library.undo()?;
    assert_eq!(library.playlist_cuts(id)?, removed);
    library.undo()?;
    assert_eq!(library.playlist_cuts(id)?, appended);
    library.undo()?;
    assert_eq!(library.playlist_cuts(id)?, before);

    library.redo()?;
    assert_eq!(library.playlist_cuts(id)?, appended);
    library.redo()?;
    assert_eq!(library.playlist_cuts(id)?, removed);
    library.redo()?;
    assert_eq!(library.playlist_cuts(id)?, moved);
    Ok(())
}

#[test]
fn an_undo_refuses_a_playlist_another_catalog_changed_since_the_edit() -> Result<()> {
    let tree = Tree::new();
    let database = tree.path().join("library.db");
    let ours = Library::open(&database)?;
    let theirs = Library::open(&database)?;
    let id = ours.start_playlist("Evening", &[whole("/music/a.wav")])?;
    ours.add_to_playlist(id, &[whole("/music/b.wav")])?;

    theirs.add_to_playlist(id, &[whole("/music/c.wav")])?;

    assert!(matches!(ours.undo(), Err(Error::PlaylistChanged(changed)) if changed == id));
    assert_eq!(
        paths(&ours, id)?,
        ["/music/a.wav", "/music/b.wav", "/music/c.wav"].map(PathBuf::from),
        "an undo destroyed the row the other catalog added"
    );
    assert_eq!(ours.redoable(), None);
    Ok(())
}

#[test]
fn an_edit_near_the_top_of_a_long_playlist_holds_only_the_rows_it_crossed() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows: Vec<Cut> = (0..PAST_WHAT_THE_UNDO_STACK_HOLDS)
        .map(|row| whole(format!("/music/{row}.wav")))
        .collect();
    let id = library.start_playlist("Long", &rows)?;
    let before = library.playlist_cuts(id)?;

    library.remove_from_playlist(id, Span::between(2, 4))?;
    let removed = library.playlist_cuts(id)?;
    library.move_in_playlist(id, Span::between(1, 2), 6)?;
    let moved = library.playlist_cuts(id)?;
    library.move_in_playlist(id, Span::between(9, 10), 0)?;
    let raised = library.playlist_cuts(id)?;
    library.remove_from_playlist(id, Span::between(0, 0))?;
    let dropped = library.playlist_cuts(id)?;

    assert_eq!(
        library.undoable().map(|step| step.behind),
        Some(4),
        "a step holding the rest of the playlist pushed the ones under it off the stack"
    );
    assert_eq!(removed.len(), before.len() - 3);
    assert_eq!(&removed[..2], &before[..2]);
    assert_eq!(&removed[2..], &before[5..]);
    assert_ne!(moved, removed);
    assert_ne!(raised, moved);

    for standing in [&raised, &moved, &removed, &before] {
        library.undo()?;
        assert_eq!(library.playlist_cuts(id)?, *standing);
    }
    for standing in [&removed, &moved, &raised, &dropped] {
        library.redo()?;
        assert_eq!(library.playlist_cuts(id)?, *standing);
    }
    Ok(())
}

#[test]
fn a_row_on_a_drive_that_is_not_mounted_is_kept_by_a_tidy_and_a_deleted_one_is_not() -> Result<()> {
    let tree = Tree::new();
    let on_the_drive = tree.write("drive/album/one.wav", &wav(4_410));
    let deleted = tree.write("home/gone.wav", &wav(4_410));
    let staying = tree.write("home/stays.wav", &wav(4_410));
    let never_scanned = tree.write("usb/Artist/Album/far.wav", &wav(4_410));
    let alone = tree.write("home/emptied/alone.wav", &wav(4_410));
    let database = tree.path().join("library.db");

    let library = Library::open(&database)?;
    scan(&library, &tree.path().join("home"), false)?;
    scan(&library, &tree.path().join("drive"), false)?;
    rusqlite::Connection::open(&database)
        .and_then(|catalog| {
            catalog.execute(
                "INSERT INTO volumes (path) VALUES (?1)",
                [tree.path().join("drive").to_str()],
            )
        })
        .expect("the catalog takes a volume");
    let id = library.start_playlist(
        "Evening",
        &[&on_the_drive, &deleted, &staying, &never_scanned, &alone].map(whole),
    )?;

    fs::remove_dir_all(tree.path().join("drive")).expect("the fixture drive can be emptied");
    fs::create_dir(tree.path().join("drive")).expect("the fixture mount point can be made");
    fs::remove_dir_all(tree.path().join("usb/Artist")).expect("the fixture stick can be emptied");
    fs::remove_file(&deleted).expect("the fixture file can be deleted");
    fs::remove_file(&alone).expect("the last file of a folder can be deleted");

    assert_eq!(library.tidy_playlist(id)?, 2);
    assert_eq!(
        paths(&library, id)?,
        vec![on_the_drive, staying, never_scanned],
        "a tidy dropped a row whose file only sat on a drive that was not there"
    );
    Ok(())
}

#[test]
fn a_prune_keeps_the_rows_under_a_root_that_is_not_there() -> Result<()> {
    let tree = Tree::new();
    let root = tree.path().join("root");
    let inside = tree.write("root/album/one.wav", &wav(4_410));
    tree.write("beside.wav", &wav(4_410));

    let library = Library::open(&tree.path().join("library.db"))?;
    scan(&library, &root, false)?;
    let id = library.start_playlist("Evening", &[whole(&inside)])?;
    fs::remove_dir_all(&root).expect("the fixture root can be taken away");

    assert_eq!(library.prune_playlist(id)?, 0);
    assert_eq!(paths(&library, id)?, vec![inside]);
    Ok(())
}

#[test]
fn a_sheet_title_carrying_a_character_xml_forbids_is_written_without_it() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let id = library.start_playlist("Bell\u{1}s \u{8}and Whistles", &[whole("/music/a.wav")])?;
    let sheet = tree.path().join("bells.xspf");

    library.export_playlist(id, &sheet)?;
    let written = fs::read_to_string(&sheet).expect("the sheet was written");

    assert!(
        !written
            .chars()
            .any(|character| character.is_control() && !character.is_whitespace()),
        "the sheet carries a control character: {written:?}"
    );
    assert!(
        written.contains("<title>Bells and Whistles</title>"),
        "{written}"
    );
    Ok(())
}

#[test]
fn a_timed_row_takes_its_rate_from_the_catalog_rather_than_probing_the_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("side.wav", &wav(RATE));
    let library = Library::open_in_memory()?;
    scan(&library, tree.path(), false)?;
    fs::write(&file, b"no longer a wave").expect("the fixture file can be spoiled");
    let sheet = tree.write(
        "cut.m3u",
        format!(
            "#EXTM3U\n#EXTVLCOPT:start-time=0.5\n{path}\n#EXTVLCOPT:start-time=0.25\n\
             #EXTVLCOPT:stop-time=0.5\n{path}\n",
            path = file.display()
        )
        .as_bytes(),
    );

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(
        library.playlist_cuts(imported.id)?,
        vec![
            Cut {
                location: MediaLocation::local(&file),
                span: Some(FrameSpan::starting(Frames(u64::from(RATE / 2)))),
            },
            Cut {
                location: MediaLocation::local(&file),
                span: Some(FrameSpan::between(
                    Frames(u64::from(RATE / 4)),
                    Frames(u64::from(RATE / 2))
                )),
            },
        ]
    );
    Ok(())
}

#[test]
fn an_export_that_fails_leaves_nothing_staged_beside_its_target() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let id = library.start_playlist("Evening", &[whole("/music/a.wav")])?;
    let folder = tree.path().join("sheets");
    let taken = folder.join("evening.m3u");
    fs::create_dir_all(&taken).expect("a folder stands where the sheet would go");
    let kept = folder.join("kept.m3u");

    assert!(matches!(
        library.export_playlist(id, &taken),
        Err(Error::Io { .. })
    ));
    library.export_playlist(id, &kept)?;
    library.export_playlist(id, &kept)?;

    let left: BTreeSet<PathBuf> = fs::read_dir(&folder)
        .expect("the sheets folder reads")
        .map(|entry| entry.expect("an entry reads").path())
        .collect();
    assert_eq!(left, BTreeSet::from([taken, kept]));
    Ok(())
}

#[test]
fn two_catalogs_duplicating_one_playlist_at_once_each_find_a_free_name() -> Result<()> {
    let tree = Tree::new();
    let database = tree.path().join("library.db");
    let first = Library::open(&database)?;
    let id = first.start_playlist("Evening", &[whole("/music/a.wav")])?;

    let copies: Vec<PlaylistId> = thread::scope(|scope| {
        let running: Vec<_> = (0..DUPLICATING_THREADS)
            .map(|_| {
                scope.spawn(|| -> Result<Vec<PlaylistId>> {
                    let catalog = Library::open(&database)?;
                    (0..DUPLICATES_EACH)
                        .map(|_| catalog.duplicate_playlist(id))
                        .collect()
                })
            })
            .collect();

        running
            .into_iter()
            .map(|thread| thread.join().expect("a duplicating thread finished"))
            .collect::<Result<Vec<_>>>()
            .map(|each| each.into_iter().flatten().collect())
    })?;

    let names: BTreeSet<String> = first
        .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
        .into_iter()
        .map(|found| found.name)
        .collect();
    assert_eq!(copies.len(), DUPLICATING_THREADS * DUPLICATES_EACH);
    assert_eq!(names.len(), DUPLICATING_THREADS * DUPLICATES_EACH + 1);
    assert!(names.contains("Evening (copy)"));
    Ok(())
}

#[test]
fn a_sheet_naming_a_file_through_a_link_the_scan_followed_lands_on_the_catalog_row() -> Result<()> {
    let tree = Tree::new();
    let elsewhere = Tree::new();
    let root = tree.path().join("root");
    tree.write("root/home.wav", &wav(4_410));
    elsewhere.write("far.wav", &wav(4_410));
    os::unix::fs::symlink(elsewhere.path(), root.join("linked"))
        .expect("the fixture tree takes a link");
    os::unix::fs::symlink(&root, tree.path().join("alias")).expect("the fixture tree takes a link");

    let library = Library::open_in_memory()?;
    scan(&library, &root, true)?;
    let catalogued = root.join("linked/far.wav");
    let sheet = tree.write(
        "list.m3u",
        format!(
            "#EXTM3U\n{}\n{}\n",
            catalogued.display(),
            tree.path().join("alias/linked/far.wav").display()
        )
        .as_bytes(),
    );

    let imported = library.import_playlist(&sheet, None)?;
    let entries = library.playlist_entries(imported.id, None)?;

    assert_eq!(
        paths(&library, imported.id)?,
        vec![catalogued.clone(), catalogued],
        "a row was stored where the link points rather than where the scan put it"
    );
    assert!(entries.iter().all(|entry| entry.track.is_some()));
    assert_eq!(held(&library, imported.id)?, 2);
    Ok(())
}
