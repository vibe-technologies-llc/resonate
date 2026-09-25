use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use notify::{
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _,
    event::{CreateKind, ModifyKind, RemoveKind, RenameMode},
};
use parking_lot::Mutex;

use crate::scan;

#[derive(Default)]
struct Moved {
    roots: BTreeSet<PathBuf>,
    last: Option<Instant>,
    gone: BTreeMap<PathBuf, Instant>,
}

enum Heard {
    Gone,
    Changed,
    Unread,
}

pub struct RootsWatch {
    moved: Arc<Mutex<Moved>>,
    _watcher: RecommendedWatcher,
}

impl RootsWatch {
    pub fn over(roots: &[PathBuf]) -> Option<Self> {
        let moved = Arc::new(Mutex::new(Moved::default()));
        let heard = Arc::clone(&moved);
        let under: Vec<PathBuf> = roots.to_vec();
        let watcher =
            notify::recommended_watcher(move |event: notify::Result<Event>| match event {
                Ok(event) => heard_under(&under, &event, &heard),
                Err(error) => tracing::debug!(%error, "a library folder's watch reported a fault"),
            });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                tracing::warn!(%error, "the library folders cannot be watched, so a file changed under them waits for a scan");
                return None;
            }
        };

        for root in roots {
            if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
                tracing::warn!(
                    %error,
                    root = %root.display(),
                    "a library folder cannot be watched, so a file changed under it waits for a scan"
                );
            }
        }

        Some(Self {
            moved,
            _watcher: watcher,
        })
    }

    pub fn settled(&self, quiet: Duration) -> Vec<PathBuf> {
        let mut moved = self.moved.lock();
        let still = moved.last.is_some_and(|last| last.elapsed() >= quiet);
        if !still {
            return Vec::new();
        }
        moved.last = None;
        std::mem::take(&mut moved.roots).into_iter().collect()
    }

    pub fn taken_away(&self, quiet: Duration) -> Vec<PathBuf> {
        let mut moved = self.moved.lock();
        if moved.last.is_some() {
            return Vec::new();
        }
        let still: Vec<PathBuf> = moved
            .gone
            .iter()
            .filter(|(_, heard)| heard.elapsed() >= quiet)
            .map(|(path, _)| path.clone())
            .collect();
        for path in &still {
            moved.gone.remove(path);
        }
        still
    }
}

fn heard_under(roots: &[PathBuf], event: &Event, into: &Mutex<Moved>) {
    if event.need_rescan() {
        let mut moved = into.lock();
        moved.roots.extend(roots.iter().cloned());
        moved.last = Some(Instant::now());
        return;
    }

    let heard = heard_as(event);
    if matches!(heard, Heard::Unread) {
        return;
    }
    let under: Vec<&PathBuf> = event
        .paths
        .iter()
        .filter(|path| roots.iter().any(|root| path.starts_with(root)))
        .collect();
    if under.is_empty() {
        return;
    }

    let now = Instant::now();
    let mut moved = into.lock();
    match heard {
        Heard::Gone => {
            for path in under {
                moved.gone.insert(path.clone(), now);
            }
        }
        Heard::Changed => {
            let touched = roots
                .iter()
                .filter(|root| under.iter().any(|path| path.starts_with(root)));
            moved.roots.extend(touched.cloned());
            moved.last = Some(now);
        }
        Heard::Unread => {}
    }
}

fn heard_as(event: &Event) -> Heard {
    let a_sheet = event.paths.iter().any(|path| scan::is_a_sheet(path));
    let audio_or_a_folder = event
        .paths
        .iter()
        .all(|path| scan::is_audio(path) || path.extension().is_none());
    let taken_away = match event.kind {
        EventKind::Remove(RemoveKind::Folder)
        | EventKind::Modify(ModifyKind::Name(RenameMode::From)) => true,
        EventKind::Remove(_) => audio_or_a_folder,
        _ => false,
    };

    if taken_away && !a_sheet {
        return Heard::Gone;
    }
    if moves_what_a_scan_reads(event) {
        return Heard::Changed;
    }
    Heard::Unread
}

fn moves_what_a_scan_reads(event: &Event) -> bool {
    let folder = matches!(
        event.kind,
        EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
    );
    let renamed = matches!(event.kind, EventKind::Modify(ModifyKind::Name(_)));
    let written = matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(_)
    );

    folder
        || (renamed && event.paths.iter().any(|path| names_a_folder(path)))
        || (written && event.paths.iter().any(|path| read_by_a_scan(path)))
}

fn names_a_folder(path: &Path) -> bool {
    path.extension().is_none() || path.is_dir()
}

fn read_by_a_scan(path: &Path) -> bool {
    scan::is_a_sheet(path) || scan::is_audio(path)
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process, thread};

    use super::*;

    const QUIET: Duration = Duration::from_millis(100);
    const HEARD_WITHIN: Duration = Duration::from_secs(5);
    const LOOKED_EVERY: Duration = Duration::from_millis(25);

    struct Scratch {
        path: PathBuf,
    }

    impl Scratch {
        fn new(named: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-watch-{}-{named}", process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(path.join("album")).expect("a writable temporary directory");
            Self { path }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn settled_within(watch: &RootsWatch, wait: Duration) -> Vec<PathBuf> {
        let started = Instant::now();
        while started.elapsed() < wait {
            let settled = watch.settled(QUIET);
            if !settled.is_empty() {
                return settled;
            }
            thread::sleep(LOOKED_EVERY);
        }
        Vec::new()
    }

    #[test]
    fn a_file_dropped_under_a_root_names_that_root_once_the_folder_is_quiet() {
        let scratch = Scratch::new("taken");
        let file = scratch.path.join("album/echoes.flac");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::write(&file, b"fLaC").expect("a file");

        assert_eq!(
            settled_within(&watch, HEARD_WITHIN),
            vec![scratch.path.clone()]
        );
        assert!(
            watch.settled(QUIET).is_empty(),
            "a change was handed out twice"
        );
    }

    fn taken_away_within(watch: &RootsWatch, wait: Duration) -> Vec<PathBuf> {
        let started = Instant::now();
        while started.elapsed() < wait {
            let gone = watch.taken_away(QUIET);
            if !gone.is_empty() {
                return gone;
            }
            thread::sleep(LOOKED_EVERY);
        }
        Vec::new()
    }

    #[test]
    fn a_file_taken_away_is_named_on_its_own_without_waiting_on_the_whole_root() {
        let scratch = Scratch::new("gone");
        let first = scratch.path.join("album/echoes.flac");
        let second = scratch.path.join("album/time.flac");
        fs::write(&first, b"fLaC").expect("a file");
        fs::write(&second, b"fLaC").expect("a file");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::remove_file(&first).expect("the file taken away");
        fs::remove_file(&second).expect("the file taken away");

        let mut gone = taken_away_within(&watch, HEARD_WITHIN);
        gone.extend(watch.taken_away(Duration::ZERO));
        gone.sort();
        assert_eq!(gone, vec![first, second]);
        assert!(
            watch.settled(Duration::ZERO).is_empty(),
            "a file taken away set off a scan of the whole root"
        );
    }

    #[test]
    fn a_file_renamed_under_a_root_is_left_to_the_scan_rather_than_forgotten_first() {
        let scratch = Scratch::new("renamed");
        let before = scratch.path.join("album/echoes.flac");
        let after = scratch.path.join("album/06 Echoes.flac");
        fs::write(&before, b"fLaC").expect("a file");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::rename(&before, &after).expect("the file renamed");

        thread::sleep(QUIET * 2);
        assert!(
            watch.taken_away(Duration::ZERO).is_empty(),
            "a renamed file was forgotten before the scan could follow it"
        );
        assert_eq!(
            settled_within(&watch, HEARD_WITHIN),
            vec![scratch.path.clone()]
        );
        assert_eq!(watch.taken_away(Duration::ZERO), vec![before]);
    }

    #[test]
    fn a_folder_renamed_with_a_dot_in_its_name_is_left_to_the_scan_rather_than_forgotten_first() {
        let scratch = Scratch::new("dotted");
        let before = scratch.path.join("Dr. Dre");
        let after = scratch.path.join("Dr. Dre (2001)");
        fs::create_dir_all(&before).expect("a folder");
        fs::write(before.join("still.flac"), b"fLaC").expect("a file");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::rename(&before, &after).expect("the folder renamed");

        thread::sleep(QUIET * 2);
        assert!(
            watch.taken_away(Duration::ZERO).is_empty(),
            "a renamed folder was forgotten before the scan could follow it"
        );
        assert_eq!(
            settled_within(&watch, HEARD_WITHIN),
            vec![scratch.path.clone()]
        );
        assert_eq!(watch.taken_away(Duration::ZERO), vec![before]);
    }

    #[test]
    fn a_sheet_taken_away_is_scanned_for_rather_than_forgotten() {
        let scratch = Scratch::new("sheet");
        let sheet = scratch.path.join("album/rip.cue");
        fs::write(&sheet, b"FILE \"rip.flac\" WAVE").expect("a sheet");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::remove_file(&sheet).expect("the sheet taken away");

        assert_eq!(
            settled_within(&watch, HEARD_WITHIN),
            vec![scratch.path.clone()]
        );
        assert!(watch.taken_away(Duration::ZERO).is_empty());
    }

    #[test]
    fn a_file_no_scan_reads_moves_nothing() {
        let scratch = Scratch::new("unread");
        let watch = RootsWatch::over(std::slice::from_ref(&scratch.path)).expect("a watch");

        fs::write(scratch.path.join("album/notes.txt"), b"liner notes").expect("a file");

        assert!(settled_within(&watch, QUIET * 10).is_empty());
    }
}
