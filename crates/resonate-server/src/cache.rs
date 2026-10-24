use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    process,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_core::writer::{Held, Writer, writer_of};

use crate::{CacheOp, Error, Result};

const KEPT_AS: &str = "flac";
const FILLING_AS: &str = "part";
const SHARE_OF_LIMIT_ONE_TRACK_MAY_TAKE: u64 = 2;

#[derive(Clone)]
pub struct Cache(Arc<Shared>);

struct Shared {
    directory: PathBuf,
    limit: u64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    kept: HashMap<u64, Kept>,
    filling: HashSet<u64>,
    bytes: u64,
    tick: u64,
}

struct Kept {
    bytes: u64,
    used: u64,
}

enum Name {
    Kept(u64),
    Filling(u32),
}

pub struct Hit {
    pub file: File,
    pub bytes: u64,
}

pub struct Fill {
    cache: Cache,
    id: u64,
    part: PathBuf,
    held: Held,
    reserved: u64,
    committed: bool,
}

impl Name {
    fn of(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        let mut parts = name.split('.');
        let id = number(parts.next()?)?;
        match (parts.next()?, parts.next(), parts.next()) {
            (KEPT_AS, None, None) => Some(Self::Kept(id)),
            (pid, Some(FILLING_AS), None) => Some(Self::Filling(number(pid)?.try_into().ok()?)),
            _ => None,
        }
    }
}

fn number(text: &str) -> Option<u64> {
    text.bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| text.parse().ok())
        .flatten()
}

fn failed(op: CacheOp, path: &Path, source: io::Error) -> Error {
    Error::Cache {
        op,
        path: path.to_path_buf(),
        source,
    }
}

impl Shared {
    fn kept_path(&self, id: u64) -> PathBuf {
        self.directory.join(format!("{id}.{KEPT_AS}"))
    }

    fn filling_path(&self, id: u64) -> PathBuf {
        self.directory
            .join(format!("{id}.{}.{FILLING_AS}", process::id()))
    }

    fn one_track_at_most(&self) -> u64 {
        self.limit / SHARE_OF_LIMIT_ONE_TRACK_MAY_TAKE
    }

    fn made_room_for(&self, state: &mut State, needed: u64) -> bool {
        while state.bytes.saturating_add(needed) > self.limit {
            let Some(oldest) = state
                .kept
                .iter()
                .min_by_key(|(_, kept)| kept.used)
                .map(|(id, _)| *id)
            else {
                return false;
            };
            if let Some(evicted) = state.kept.remove(&oldest) {
                state.bytes -= evicted.bytes;
            }
            if let Err(error) = fs::remove_file(self.kept_path(oldest)) {
                tracing::warn!(track = oldest, %error, "an evicted track could not be removed");
            }
        }
        true
    }
}

impl Cache {
    pub fn open(directory: &Path, limit: u64) -> Result<Self> {
        fs::create_dir_all(directory)
            .map_err(|source| failed(CacheOp::Create, directory, source))?;
        let listing =
            fs::read_dir(directory).map_err(|source| failed(CacheOp::List, directory, source))?;

        let mut found = Vec::new();
        for entry in listing {
            let entry = entry.map_err(|source| failed(CacheOp::List, directory, source))?;
            let path = entry.path();
            match Name::of(&path) {
                Some(Name::Kept(id)) => {
                    let metadata = entry
                        .metadata()
                        .map_err(|source| failed(CacheOp::Inspect, &path, source))?;
                    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
                    found.push((id, metadata.len(), modified));
                }
                Some(Name::Filling(pid)) if writer_of(&path, pid) == Writer::Gone => {
                    if let Err(error) = fs::remove_file(&path) {
                        tracing::warn!(path = %path.display(), %error, "a stale cache fill could not be removed");
                    }
                }
                Some(Name::Filling(_)) | None => {}
            }
        }
        found.sort_by_key(|(_, _, modified)| *modified);

        let mut state = State::default();
        for (id, bytes, _) in found {
            state.tick += 1;
            state.bytes += bytes;
            state.kept.insert(
                id,
                Kept {
                    bytes,
                    used: state.tick,
                },
            );
        }
        let shared = Shared {
            directory: directory.to_path_buf(),
            limit,
            state: Mutex::new(State::default()),
        };
        shared.made_room_for(&mut state, 0);
        *shared.state.lock() = state;
        Ok(Self(Arc::new(shared)))
    }

    pub fn hit(&self, id: u64) -> Option<Hit> {
        let mut state = self.0.state.lock();
        state.tick += 1;
        let tick = state.tick;
        let kept = state.kept.get_mut(&id)?;
        kept.used = tick;
        let bytes = kept.bytes;

        match File::open(self.0.kept_path(id)) {
            Ok(file) => {
                let _ = file.set_modified(SystemTime::now());
                Some(Hit { file, bytes })
            }
            Err(error) => {
                tracing::warn!(track = id, %error, "a cached track could not be opened");
                state.kept.remove(&id);
                state.bytes -= bytes;
                None
            }
        }
    }

    pub fn fill(&self, id: u64, length: Option<u64>) -> Option<Fill> {
        if length.is_some_and(|length| length > self.0.one_track_at_most()) {
            return None;
        }
        {
            let mut state = self.0.state.lock();
            if state.kept.contains_key(&id) || !state.filling.insert(id) {
                return None;
            }
        }
        let part = self.0.filling_path(id);
        match Held::made(&part) {
            Ok(held) => Some(Fill {
                cache: self.clone(),
                id,
                part,
                held,
                reserved: 0,
                committed: false,
            }),
            Err(error) => {
                tracing::warn!(track = id, %error, "a track could not be staged for the cache");
                self.0.state.lock().filling.remove(&id);
                None
            }
        }
    }

    pub fn bytes(&self) -> u64 {
        self.0.state.lock().bytes
    }

    pub fn tracks(&self) -> usize {
        self.0.state.lock().kept.len()
    }
}

impl Fill {
    pub fn write(&mut self, chunk: &[u8]) -> io::Result<()> {
        let shared = &self.cache.0;
        let size = chunk.len() as u64;
        if self.reserved + size > shared.one_track_at_most() {
            return Err(io::Error::from(io::ErrorKind::StorageFull));
        }
        {
            let mut state = shared.state.lock();
            if !shared.made_room_for(&mut state, size) {
                return Err(io::Error::from(io::ErrorKind::StorageFull));
            }
            state.bytes += size;
        }
        self.reserved += size;
        let mut file = self.held.file();
        file.write_all(chunk)
    }

    pub fn commit(mut self) -> io::Result<()> {
        let shared = &self.cache.0;
        fs::rename(&self.part, shared.kept_path(self.id))?;
        let mut state = shared.state.lock();
        state.tick += 1;
        let used = state.tick;
        state.kept.insert(
            self.id,
            Kept {
                bytes: self.reserved,
                used,
            },
        );
        state.filling.remove(&self.id);
        self.committed = true;
        tracing::info!(track = self.id, bytes = self.reserved, "a track was cached");
        Ok(())
    }
}

impl Drop for Fill {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        if let Err(error) = fs::remove_file(&self.part) {
            tracing::warn!(path = %self.part.display(), %error, "an abandoned cache fill could not be removed");
        }
        let mut state = self.cache.0.state.lock();
        state.bytes -= self.reserved;
        state.filling.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use std::{env, io::Read};

    use super::*;

    const NEVER_A_PROCESS: u32 = 999_999_999;
    const LIMIT: u64 = 100;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-cache-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn filled(cache: &Cache, id: u64, bytes: usize) {
        let mut fill = cache.fill(id, Some(bytes as u64)).expect("a fill");
        fill.write(&vec![7; bytes]).expect("written");
        fill.commit().expect("committed");
    }

    #[test]
    fn a_committed_track_is_found_again_whole() {
        let scratch = Scratch::new("found");
        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");

        let missed = cache.hit(5).is_none();
        let mut fill = cache.fill(5, Some(6)).expect("a fill");
        fill.write(b"fLaC").expect("written");
        fill.write(b"-x").expect("written");
        fill.commit().expect("committed");
        let hit = cache.hit(5).expect("a hit");

        let mut read = Vec::new();
        (&hit.file).read_to_end(&mut read).expect("read");
        assert!(missed);
        assert_eq!(hit.bytes, 6);
        assert_eq!(read, b"fLaC-x");
        assert_eq!(cache.bytes(), 6);
    }

    #[test]
    fn a_fill_dropped_before_its_commit_leaves_nothing_behind() {
        let scratch = Scratch::new("dropped");
        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");

        let mut fill = cache.fill(5, None).expect("a fill");
        fill.write(b"half").expect("written");
        drop(fill);

        assert!(cache.hit(5).is_none());
        assert_eq!(cache.bytes(), 0);
        assert_eq!(fs::read_dir(&scratch.0).expect("a listing").count(), 0);
        assert!(cache.fill(5, None).is_some());
    }

    #[test]
    fn one_track_is_filled_by_one_download_at_a_time_and_a_kept_one_not_again() {
        let scratch = Scratch::new("once");
        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");

        let first = cache.fill(5, None).expect("a fill");
        let second = cache.fill(5, None);
        drop(first);
        filled(&cache, 5, 10);
        let again = cache.fill(5, None);

        assert!(second.is_none());
        assert!(again.is_none());
    }

    #[test]
    fn the_least_recently_used_track_goes_first_when_the_limit_is_reached() {
        let scratch = Scratch::new("evicted");
        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");

        filled(&cache, 1, 40);
        filled(&cache, 2, 40);
        let touched = cache.hit(1).is_some();
        filled(&cache, 3, 40);

        assert!(touched);
        assert!(cache.hit(1).is_some());
        assert!(cache.hit(2).is_none());
        assert!(cache.hit(3).is_some());
        assert_eq!(cache.bytes(), 80);
        assert!(!scratch.0.join("2.flac").exists());
    }

    #[test]
    fn a_track_over_half_the_limit_is_never_kept_and_costs_the_others_nothing() {
        let scratch = Scratch::new("large");
        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");
        filled(&cache, 1, 40);

        let refused_up_front = cache.fill(2, Some(LIMIT / 2 + 1)).is_none();
        let mut growing = cache.fill(3, None).expect("a fill");
        let outgrown = growing.write(&vec![7; LIMIT as usize / 2 + 1]);
        drop(growing);

        assert!(refused_up_front);
        assert_eq!(
            outgrown.expect_err("over the share").kind(),
            io::ErrorKind::StorageFull
        );
        assert!(cache.hit(1).is_some());
        assert_eq!(cache.bytes(), 40);
    }

    #[test]
    fn what_a_directory_kept_is_found_after_a_restart_and_what_nobody_writes_is_swept() {
        let scratch = Scratch::new("restart");
        {
            let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");
            filled(&cache, 1, 30);
            filled(&cache, 2, 30);
        }
        let stale = scratch.0.join(format!("9.{NEVER_A_PROCESS}.part"));
        fs::write(&stale, b"left").expect("a stale fill");
        fs::write(scratch.0.join("notes.txt"), b"not ours").expect("another file");

        let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");

        assert_eq!(cache.tracks(), 2);
        assert_eq!(cache.bytes(), 60);
        assert!(!stale.exists());
        assert!(scratch.0.join("notes.txt").exists());
    }

    #[test]
    fn a_directory_over_a_lowered_limit_is_trimmed_when_opened() {
        let scratch = Scratch::new("lowered");
        {
            let cache = Cache::open(&scratch.0, LIMIT).expect("a cache");
            filled(&cache, 1, 40);
            filled(&cache, 2, 40);
        }

        let cache = Cache::open(&scratch.0, 50).expect("a cache");

        assert_eq!(cache.tracks(), 1);
        assert!(cache.bytes() <= 50);
    }
}
