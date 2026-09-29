use std::{
    env, fs,
    num::NonZeroUsize,
    os,
    path::{Path, PathBuf},
    process, slice,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{Frames, MediaLocation, Resumable, Resumption, Span};
use resonate_library::{Library, Result, ScanOptions, SortOrder, TrackQuery};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const FRAMES: usize = 4_410;
const PLAYED_INTO: Frames = Frames(22_050);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-scanned-{}-{}",
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

    fn wav(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, silence()).expect("a writable temporary file");
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

fn silence() -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let data = vec![0_u8; FRAMES * usize::from(block_align)];

    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1_u16.to_le_bytes());
    fmt.extend_from_slice(&CHANNELS.to_le_bytes());
    fmt.extend_from_slice(&RATE.to_le_bytes());
    fmt.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    fmt.extend_from_slice(&block_align.to_le_bytes());
    fmt.extend_from_slice(&BITS.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    for (id, payload) in [(b"fmt ", fmt.as_slice()), (b"data", data.as_slice())] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
    }

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn following_links(root: &Path) -> ScanOptions {
    ScanOptions {
        roots: vec![root.to_path_buf()],
        incremental: true,
        follow_symlinks: true,
        extract_cover_art: false,
        workers: NonZeroUsize::MIN,
    }
}

fn scan(library: &Library, options: ScanOptions) -> Result<()> {
    let summary = library.scan(options)?.join()?;
    assert!(!summary.cancelled, "the scan reported itself cancelled");
    Ok(())
}

fn paths(library: &Library) -> Result<Vec<PathBuf>> {
    let mut held: Vec<PathBuf> = library
        .tracks(&TrackQuery {
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            ..TrackQuery::default()
        })?
        .into_iter()
        .filter_map(|track| track.location.as_path().map(Path::to_path_buf))
        .collect();
    held.sort();
    Ok(held)
}

fn beside(database: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(database).expect("the catalog file opens for a second reader")
}

fn row(path: &str) -> Resumable {
    Resumable {
        location: MediaLocation::local(path),
        span: None,
        held: None,
    }
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_inside_the_same_root_is_not_walked_a_second_time() -> Result<()> {
    let tree = Tree::new();
    let real = tree.wav("albums/meddle/echoes.wav");
    for named in ["a link", "z link"] {
        os::unix::fs::symlink(tree.path().join("albums/meddle"), tree.path().join(named))
            .expect("the fixture tree takes a link");
    }

    let library = Library::open_in_memory()?;
    scan(&library, following_links(tree.path()))?;

    assert_eq!(paths(&library)?, vec![real]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_link_to_a_folder_holding_the_root_is_not_walked_back_into_it() -> Result<()> {
    let tree = Tree::new();
    let real = tree.wav("music/echoes.wav");
    os::unix::fs::symlink(tree.path(), tree.path().join("music/everything"))
        .expect("the fixture tree takes a link");

    let library = Library::open_in_memory()?;
    scan(&library, following_links(&tree.path().join("music")))?;

    assert_eq!(paths(&library)?, vec![real]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_link_into_another_root_leaves_its_files_to_that_root() -> Result<()> {
    let tree = Tree::new();
    let kept = tree.wav("first/kept.wav");
    let elsewhere = tree.wav("second/elsewhere.wav");
    os::unix::fs::symlink(tree.path().join("second"), tree.path().join("first/second"))
        .expect("the fixture tree takes a link");

    let library = Library::open_in_memory()?;
    library.add_root(&tree.path().join("second"))?;
    scan(&library, following_links(&tree.path().join("first")))?;
    scan(&library, following_links(&tree.path().join("second")))?;

    assert_eq!(paths(&library)?, vec![kept, elsewhere]);
    Ok(())
}

#[test]
fn a_volume_retired_for_good_forgets_its_rows_and_is_no_longer_remembered() -> Result<()> {
    let tree = Tree::new();
    let home = tree.wav("music/home.wav");
    tree.wav("music/drive/one.wav");
    tree.wav("music/drive/two.wav");
    let drive = tree.path().join("music/drive");

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, following_links(&tree.path().join("music")))?;
    beside(&database)
        .execute(
            "INSERT INTO volumes (path) VALUES (?1)",
            [drive.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the catalog takes a volume");
    fs::remove_dir_all(&drive).expect("the fixture drive can be emptied");
    fs::create_dir(&drive).expect("the fixture mount point can be made");

    assert_eq!(library.forget_the_gone(slice::from_ref(&drive))?, 0);
    assert_eq!(paths(&library)?.len(), 3);

    let retired = library.retire(&drive)?;
    let volumes: i64 = beside(&database)
        .query_row("SELECT count(*) FROM volumes", [], |row| row.get(0))
        .expect("the catalog counts its volumes");

    assert_eq!(retired, 2);
    assert_eq!(paths(&library)?, vec![home]);
    assert_eq!(volumes, 0);
    Ok(())
}

#[test]
fn a_folder_whose_files_are_still_there_is_not_retired() -> Result<()> {
    let tree = Tree::new();
    let held = tree.wav("music/drive/one.wav");

    let library = Library::open_in_memory()?;
    scan(&library, following_links(&tree.path().join("music")))?;

    assert_eq!(library.retire(&tree.path().join("music/drive"))?, 0);
    assert_eq!(paths(&library)?, vec![held]);
    Ok(())
}

#[test]
fn a_kept_queue_row_that_will_not_open_is_dropped_and_the_rest_resumed() -> Result<()> {
    let tree = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    library.keep_resumption(&Resumption {
        rows: vec![
            row("/music/0.flac"),
            row("/music/1.flac"),
            row("/music/2.flac"),
        ],
        order: vec![2, 1, 0],
        row: 2,
        at: PLAYED_INTO,
        shuffle: true,
        next: Some(Span::one(1)),
    })?;
    beside(&database)
        .execute(
            "UPDATE resume_rows SET uri = 'nothing openable' WHERE position = 1",
            [],
        )
        .expect("the kept queue takes a row naming nothing");

    let resumed = library
        .resumption()?
        .expect("a queue one row short is resumed");
    let drawn: Vec<MediaLocation> = resumed
        .in_play_order()
        .into_iter()
        .map(|loaded| resumed.rows[loaded].location.clone())
        .collect();

    assert_eq!(
        drawn,
        vec![
            MediaLocation::local("/music/2.flac"),
            MediaLocation::local("/music/0.flac")
        ]
    );
    assert_eq!(
        drawn[resumed.landing()],
        MediaLocation::local("/music/0.flac")
    );
    assert_eq!(resumed.at, PLAYED_INTO);
    assert_eq!(resumed.next, None);
    assert!(resumed.shuffle);
    Ok(())
}
