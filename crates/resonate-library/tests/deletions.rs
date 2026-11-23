use std::{
    fs::{self, OpenOptions},
    io::{Seek as _, SeekFrom, Write as _},
    panic::{self, AssertUnwindSafe},
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{Recording, TrackId};
use resonate_library::{Favoured, Library, Removal, Result, Trashes};

use crate::support::{ALBUM, ARTIST, TITLE, TrashedByTheDesktop, Tree, Wav, beside, options, scan};

fn song(title: &str) -> Vec<u8> {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, "Ada")
        .text(ALBUM, "Winter")
        .build()
}

fn recording(title: &str) -> Recording {
    Recording {
        mbid: None,
        title: title.into(),
        artist: "Ada".into(),
        album: "Winter".into(),
        length: Some(Duration::from_millis(100)),
    }
}

fn row_of(library: &Library, path: &Path) -> Result<TrackId> {
    Ok(library
        .track_at(path, None)?
        .expect("the song was scanned")
        .id)
}

fn taken_by_the_desktop() -> Arc<TrashedByTheDesktop> {
    Arc::new(TrashedByTheDesktop {
        asked: Mutex::new(Vec::new()),
        answer: Removal::Removed,
    })
}

#[test]
fn a_song_deleted_on_a_drive_not_plugged_in_keeps_its_row_and_mark_and_one_gone_by_hand_is_settled()
-> Result<()> {
    let tree = Tree::new();
    let catalog = Tree::new();
    let far = tree.write("drive/far.wav", &song("Far"));
    let near = tree.write("near.wav", &song("Near"));
    let drive = far.parent().expect("a folder").to_path_buf();
    let database = catalog.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(catalog.path().join("data")));
    scan(&library, &options(&tree))?;
    beside(&database)
        .execute(
            "INSERT INTO volumes (path) VALUES (?1)",
            [drive.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the catalog takes a volume");

    let far_row = row_of(&library, &far)?;
    let near_row = row_of(&library, &near)?;

    fs::remove_dir_all(&drive).expect("the drive is emptied");
    fs::create_dir(&drive).expect("its mount point stands empty");

    let unplugged = library.delete_tracks(&[far_row])?;

    assert_eq!(
        (unplugged.tracks, unplugged.files, unplugged.kept),
        (0, 0, 1)
    );
    assert!(library.track_at(&far, None)?.is_some());
    assert!(!library.deleted_recording(&recording("Far"))?);

    fs::remove_dir(&drive).expect("the mount point goes too");

    let vanished = library.delete_tracks(&[far_row])?;

    assert_eq!((vanished.tracks, vanished.files, vanished.kept), (0, 0, 1));
    assert!(library.track_at(&far, None)?.is_some());
    assert!(!library.deleted_recording(&recording("Far"))?);

    fs::remove_file(&near).expect("the file goes by hand");

    let gone = library.delete_tracks(&[near_row])?;

    assert_eq!((gone.tracks, gone.files, gone.kept), (1, 0, 0));
    assert!(library.track_at(&near, None)?.is_none());
    assert!(library.deleted_recording(&recording("Near"))?);
    Ok(())
}

struct AnsweredGone;

impl Trashes for AnsweredGone {
    fn trash(&self, _: &Path) -> Removal {
        Removal::AlreadyGone
    }
}

#[cfg(unix)]
#[test]
fn a_file_the_trash_called_gone_though_its_stat_was_refused_keeps_its_row_and_mark() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let tree = Tree::new();
    let locked = tree.write("locked/song.wav", &song("Locked"));
    let folder = locked.parent().expect("a folder").to_path_buf();
    let library = Library::open_in_memory()?;

    library.trash_through(Some(Arc::new(AnsweredGone)));
    scan(&library, &options(&tree))?;

    let row = row_of(&library, &locked)?;

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o600))
        .expect("the folder takes a mode");

    let deleted = library.delete_tracks(&[row]);

    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755))
        .expect("the folder takes its mode back");

    let deleted = deleted?;

    assert_eq!((deleted.tracks, deleted.files, deleted.kept), (0, 0, 1));
    assert!(locked.exists());
    assert!(library.track_at(&locked, None)?.is_some());
    assert!(!library.deleted_recording(&recording("Locked"))?);
    Ok(())
}

struct EndsTheRun {
    moves_first: bool,
    ended: AtomicBool,
}

impl Trashes for EndsTheRun {
    fn trash(&self, file: &Path) -> Removal {
        let first = !self.ended.swap(true, Ordering::AcqRel);

        if self.moves_first || !first {
            fs::remove_file(file).expect("the desktop takes the file");
        }

        assert!(!first, "the run ends while the file is being moved");
        Removal::Removed
    }
}

struct Interrupted {
    _tree: Tree,
    _catalog: Tree,
    database: PathBuf,
    cold: PathBuf,
}

fn deleted_everywhere_then_interrupted(moves_first: bool) -> Result<Interrupted> {
    let tree = Tree::new();
    let catalog = Tree::new();
    let cold = tree.write("cold.wav", &song("Cold"));
    let database = catalog.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_through(Some(Arc::new(EndsTheRun {
        moves_first,
        ended: AtomicBool::new(false),
    })));
    scan(&library, &options(&tree))?;

    let row = row_of(&library, &cold)?;
    let interrupted = panic::catch_unwind(AssertUnwindSafe(|| {
        library.delete_tracks_everywhere(&[row])
    }));

    assert!(interrupted.is_err());
    assert!(
        library.deleted_recording(&recording("Cold"))?,
        "the mark was not noted before the file went"
    );
    Ok(Interrupted {
        _tree: tree,
        _catalog: catalog,
        database,
        cold,
    })
}

#[test]
fn a_run_ended_before_the_file_moved_takes_its_mark_back_once_reopened_and_tells_no_device()
-> Result<()> {
    let left = deleted_everywhere_then_interrupted(false)?;

    for _ in 0..2 {
        let reopened = Library::open(&left.database)?;

        assert!(left.cold.exists());
        assert!(reopened.track_at(&left.cold, None)?.is_some());
        assert!(!reopened.deleted_recording(&recording("Cold"))?);
        assert!(reopened.deleted_everywhere_to_tell()?.is_empty());
    }
    Ok(())
}

#[test]
fn a_run_ended_once_the_file_moved_is_settled_once_reopened_and_tells_every_device_once()
-> Result<()> {
    let left = deleted_everywhere_then_interrupted(true)?;

    for _ in 0..2 {
        let reopened = Library::open(&left.database)?;
        let waiting = reopened.deleted_everywhere_to_tell()?;

        assert!(!left.cold.exists());
        assert!(reopened.track_at(&left.cold, None)?.is_none());
        assert!(reopened.deleted_recording(&recording("Cold"))?);
        assert_eq!(
            waiting
                .iter()
                .map(|told| told.recording.title.as_str())
                .collect::<Vec<_>>(),
            ["Cold"]
        );
    }
    Ok(())
}

#[test]
fn a_run_ended_once_the_file_moved_is_settled_before_the_next_deletion_of_the_same_catalog()
-> Result<()> {
    let tree = Tree::new();
    let cold = tree.write("cold.wav", &song("Cold"));
    let other = tree.write("other.wav", &song("Other"));
    let library = Library::open_in_memory()?;

    library.trash_through(Some(Arc::new(EndsTheRun {
        moves_first: true,
        ended: AtomicBool::new(false),
    })));
    scan(&library, &options(&tree))?;

    let row = row_of(&library, &cold)?;
    let next = row_of(&library, &other)?;
    let interrupted = panic::catch_unwind(AssertUnwindSafe(|| {
        library.delete_tracks_everywhere(&[row])
    }));

    assert!(interrupted.is_err());
    assert!(library.track_at(&cold, None)?.is_some());

    library.delete_tracks(&[next])?;

    let waiting = library.deleted_everywhere_to_tell()?;

    assert!(library.track_at(&cold, None)?.is_none());
    assert!(library.track_at(&other, None)?.is_none());
    assert!(library.deleted_recording(&recording("Cold"))?);
    assert!(library.deleted_recording(&recording("Other"))?);
    assert_eq!(
        waiting
            .iter()
            .map(|told| told.recording.title.as_str())
            .collect::<Vec<_>>(),
        ["Cold"]
    );
    Ok(())
}

#[test]
fn a_removal_left_out_of_reach_waits_and_never_takes_back_a_mark_a_removed_copy_shares()
-> Result<()> {
    let tree = Tree::new();
    let catalog = Tree::new();
    let far = tree.write("drive/cold.wav", &song("Cold"));
    let near = tree.write("near/cold.wav", &song("Cold"));
    let drive = far.parent().expect("a folder").to_path_buf();
    let database = catalog.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_through(Some(Arc::new(EndsTheRun {
        moves_first: false,
        ended: AtomicBool::new(false),
    })));
    scan(&library, &options(&tree))?;
    beside(&database)
        .execute(
            "INSERT INTO volumes (path) VALUES (?1)",
            [drive.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the catalog takes a volume");

    let far_row = row_of(&library, &far)?;
    let near_row = row_of(&library, &near)?;
    let interrupted = panic::catch_unwind(AssertUnwindSafe(|| library.delete_tracks(&[far_row])));

    assert!(interrupted.is_err());

    fs::rename(&far, catalog.path().join("cold.wav")).expect("the drive's song is set aside");

    library.delete_tracks(&[near_row])?;

    assert!(library.track_at(&far, None)?.is_some());
    assert!(library.track_at(&near, None)?.is_none());

    drop(library);
    fs::rename(catalog.path().join("cold.wav"), &far).expect("the drive is plugged in again");

    let reopened = Library::open(&database)?;

    assert!(far.exists());
    assert!(reopened.track_at(&far, None)?.is_some());
    assert!(reopened.deleted_recording(&recording("Cold"))?);
    Ok(())
}

struct FollowedMeanwhile {
    library: Arc<OnceLock<Arc<Library>>>,
    recording: Recording,
    kept_its_mark: Arc<AtomicBool>,
}

impl Trashes for FollowedMeanwhile {
    fn trash(&self, file: &Path) -> Removal {
        let library = self.library.get().expect("the library is set");
        let lacked = |library: &Library| -> Result<()> {
            if library.deleted_recording(&self.recording)?
                && library.holds_recording(&self.recording)?
            {
                library.forgive_deleted_recording(&self.recording)?;
                library.forgive_deleted_recordings()?;
            }
            Ok(())
        };

        lacked(library).expect("the follow reads the catalog");
        self.kept_its_mark.store(
            library
                .deleted_recording(&self.recording)
                .expect("the mark is read"),
            Ordering::Release,
        );
        fs::remove_file(file).expect("the desktop takes the file");
        Removal::Removed
    }
}

#[test]
fn a_follow_while_the_file_moves_keeps_its_mark_so_the_song_is_not_fetched_back() -> Result<()> {
    let tree = Tree::new();
    let cold = tree.write("cold.wav", &song("Cold"));
    let library = Arc::new(Library::open_in_memory()?);
    let seat = Arc::new(OnceLock::new());
    let kept_its_mark = Arc::new(AtomicBool::new(false));

    library.trash_through(Some(Arc::new(FollowedMeanwhile {
        library: Arc::clone(&seat),
        recording: recording("Cold"),
        kept_its_mark: Arc::clone(&kept_its_mark),
    })));
    seat.set(Arc::clone(&library)).ok();
    scan(&library, &options(&tree))?;

    let row = row_of(&library, &cold)?;

    library.delete_tracks(&[row])?;

    assert!(kept_its_mark.load(Ordering::Acquire));
    assert!(library.deleted_recording(&recording("Cold"))?);
    assert!(!library.holds_recording(&recording("Cold"))?);

    library.forgive_deleted_recordings()?;

    assert!(!library.deleted_recording(&recording("Cold"))?);
    Ok(())
}

struct RefusesWhatEndsIn(&'static str);

impl Trashes for RefusesWhatEndsIn {
    fn trash(&self, file: &Path) -> Removal {
        if file.to_string_lossy().ends_with(self.0) {
            return Removal::Kept;
        }
        fs::remove_file(file).expect("the desktop takes the file");
        Removal::Removed
    }
}

#[test]
fn lyrics_the_trash_refused_beside_a_song_it_took_stay_and_are_told_as_kept() -> Result<()> {
    let tree = Tree::new();
    let cold = tree.write("cold.wav", &song("Cold"));
    let lyrics = tree.write("cold.lrc", b"[00:01.00]words");
    let shared = tree.write("both/song.lrc", b"[00:01.00]shared");

    tree.write("both/song.wav", &song("Song"));
    tree.write("both/song.flac", b"not a song the scan reads");

    let library = Library::open_in_memory()?;

    library.trash_through(Some(Arc::new(RefusesWhatEndsIn(".lrc"))));
    scan(&library, &options(&tree))?;

    let row = row_of(&library, &cold)?;
    let deleted = library.delete_tracks(&[row])?;

    assert_eq!((deleted.tracks, deleted.files, deleted.kept), (1, 1, 0));
    assert!(!cold.exists());
    assert!(lyrics.exists());
    assert_eq!(deleted.sidecars_kept, [lyrics]);

    let beside_another = row_of(&library, &tree.path().join("both/song.wav"))?;
    let shared_deleted = library.delete_tracks(&[beside_another])?;

    assert!(shared.exists());
    assert!(shared_deleted.sidecars_kept.is_empty());
    Ok(())
}

fn changed_in_place(path: &Path) {
    let mut file = OpenOptions::new()
        .write(true)
        .open(path)
        .expect("the file opens for writing");

    file.seek(SeekFrom::End(-1))
        .expect("the last byte is reached");
    file.write_all(&[1]).expect("the last byte is written");
}

fn grown(path: &Path) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("the file opens for writing");

    file.write_all(&[0; 4]).expect("the file grows");
}

#[test]
fn a_pair_changed_or_forgotten_since_it_was_found_is_not_folded_and_the_rest_still_are()
-> Result<()> {
    let tree = Tree::new();
    let mut pairs = Vec::new();

    for title in ["Dogs", "Echoes", "Sheep", "Time"] {
        let kept = tree.write(&format!("{title}.wav"), &song(title));
        let copy = tree.root.join(format!("{title} (1).wav"));

        fs::copy(&kept, &copy).expect("a copy");
        pairs.push((kept, copy));
    }

    let [
        (dogs, dogs_copy),
        (echoes, echoes_copy),
        (_, sheep_copy),
        (time, time_copy),
    ] = <[_; 4]>::try_from(pairs).expect("four pairs");
    let desktop = taken_by_the_desktop();
    let library = Library::open_in_memory()?;

    library.trash_through(Some(Arc::clone(&desktop) as Arc<dyn Trashes>));
    scan(&library, &options(&tree))?;

    let found = library.identical_copies()?;

    library.favour(Favoured::Track(row_of(&library, &echoes_copy)?), true)?;
    changed_in_place(&echoes_copy);
    grown(&dogs);
    fs::remove_file(&sheep_copy).expect("the copy goes by hand");
    library.forget_the_gone(std::slice::from_ref(&sheep_copy))?;

    let folded = library.fold_copies(&found.identical)?;

    assert_eq!(found.identical.len(), 4);
    assert_eq!(folded.files, 1);
    assert_eq!(
        folded.changed_since_found,
        [dogs_copy.clone(), echoes_copy.clone(), sheep_copy]
    );
    assert_eq!(*desktop.asked.lock(), std::slice::from_ref(&time_copy));
    assert!(!time_copy.exists() && time.exists());
    assert!(dogs.exists() && dogs_copy.exists());
    assert!(echoes.exists() && echoes_copy.exists());
    assert!(
        library
            .track_at(&echoes_copy, None)?
            .is_some_and(|row| row.favourite.is_some()),
        "the changed copy's favourite was handed over"
    );
    assert!(
        library
            .track_at(&echoes, None)?
            .is_some_and(|row| row.favourite.is_none())
    );
    Ok(())
}
