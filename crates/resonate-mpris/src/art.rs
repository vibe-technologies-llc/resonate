use std::{
    collections::VecDeque,
    env,
    fs::{self, DirBuilder, OpenOptions},
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::{self, Write as _},
    os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, TrackId};
use resonate_engine::CoverArt;

const FOLDER: &str = "resonate-art";

const COVERS_KEPT: usize = 8;

const RUNNING_PROCESSES: &str = "/proc";

const RUNTIME_FOLDER: &str = "XDG_RUNTIME_DIR";

const OURS_ALONE: u32 = 0o700;

const READ_BY_US_ALONE: u32 = 0o600;

const STAGED: &str = ".laying";

#[derive(Default)]
pub(crate) struct Pictures {
    folder: Folder,
    drawn: Mutex<Laid>,
}

#[derive(Default)]
struct Laid {
    made: bool,
    drawn: VecDeque<Drawn>,
}

struct Drawn {
    playing: Option<Playing>,
    path: PathBuf,
    uri: String,
}

struct Playing {
    track: TrackId,
    art: Weak<CoverArt>,
}

impl Playing {
    fn is(&self, other: &Self) -> bool {
        self.track == other.track && Weak::ptr_eq(&self.art, &other.art)
    }
}

impl Pictures {
    pub(crate) fn uri(&self, track: TrackId, art: &Arc<CoverArt>) -> Option<String> {
        let playing = Playing {
            track,
            art: Arc::downgrade(art),
        };
        self.laid_down(Some(playing), art)
    }

    pub(crate) fn uri_of(&self, art: &CoverArt) -> Option<String> {
        self.laid_down(None, art)
    }

    fn laid_down(&self, playing: Option<Playing>, art: &CoverArt) -> Option<String> {
        let mut laid = self.drawn.lock();
        if !laid.made {
            if let Err(error) = made_ours(&self.folder.0) {
                tracing::debug!(
                    %error,
                    folder = %self.folder.0.display(),
                    "no folder of our own could be made for the covers the bus names"
                );
                return None;
            }
            laid.made = true;
        }
        let drawn = &mut laid.drawn;
        if let Some(playing) = playing.as_ref()
            && let Some(held) = drawn.iter().find(|held| {
                held.playing
                    .as_ref()
                    .is_some_and(|drawn_for| drawn_for.is(playing))
            })
        {
            return Some(held.uri.clone());
        }

        let path = self.folder.0.join(format!(
            "{:016x}.{}",
            digest_of(&art.bytes),
            art.format.extension()
        ));
        let already = drawn.iter().any(|held| held.path == path);
        if !already && let Err(error) = lay_down(&path, &art.bytes) {
            tracing::debug!(
                %error,
                path = %path.display(),
                "a cover could not be laid down for the bus to name"
            );
            return None;
        }

        let uri = MediaLocation::local(&path).to_uri();
        drawn.push_back(Drawn {
            playing,
            path,
            uri: uri.clone(),
        });
        while drawn.len() > COVERS_KEPT {
            let Some(gone) = drawn.pop_front() else {
                break;
            };
            if !drawn.iter().any(|held| held.path == gone.path) {
                let _ = fs::remove_file(&gone.path);
            }
        }
        Some(uri)
    }

    pub(crate) fn forget(&self) {
        let mut laid = self.drawn.lock();
        if laid.made {
            let _ = fs::remove_dir_all(&self.folder.0);
        }
        laid.made = false;
        laid.drawn.clear();
    }
}

fn digest_of(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

struct Folder(PathBuf);

impl Default for Folder {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let held = NEXT.fetch_add(1, Ordering::Relaxed);
        let beneath = env::var_os(RUNTIME_FOLDER)
            .map(PathBuf::from)
            .filter(|folder| folder.is_absolute())
            .unwrap_or_else(env::temp_dir);
        if held == 0 {
            sweep_the_departed(&beneath);
        }

        Self(beneath.join(format!("{FOLDER}-{}-{held}", process::id())))
    }
}

fn sweep_the_departed(temporary: &Path) {
    let Ok(entries) = fs::read_dir(temporary) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str().and_then(laid_down_by) else {
            continue;
        };
        if pid != process::id() && !Path::new(RUNNING_PROCESSES).join(pid.to_string()).exists() {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

fn laid_down_by(name: &str) -> Option<u32> {
    let rest = name.strip_prefix(FOLDER)?.strip_prefix('-')?;
    let (pid, count) = rest.split_once('-')?;
    count.parse::<u64>().ok()?;
    pid.parse().ok()
}

fn made_ours(folder: &Path) -> io::Result<()> {
    DirBuilder::new().mode(OURS_ALONE).create(folder)
}

fn lay_down(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if already_laid(path, bytes) {
        return Ok(());
    }
    let staged = staging_for(path);
    let laid = write_staged(&staged, bytes).and_then(|()| fs::rename(&staged, path));
    if laid.is_err() {
        let _ = fs::remove_file(&staged);
    }
    laid
}

fn already_laid(path: &Path, bytes: &[u8]) -> bool {
    fs::symlink_metadata(path).is_ok_and(|held| held.is_file() && held.len() == bytes.len() as u64)
}

fn staging_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(STAGED);
    path.with_file_name(name)
}

fn write_staged(staged: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(READ_BY_US_ALONE)
        .open(staged)?;
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use resonate_engine::ImageFormat;

    use super::*;

    fn art() -> Arc<CoverArt> {
        Arc::new(CoverArt {
            format: ImageFormat::Png,
            bytes: b"\x89PNG\r\n\x1a\nthe cover".to_vec(),
        })
    }

    fn track(id: u64) -> TrackId {
        TrackId::new(id).expect("a non-zero track id")
    }

    #[test]
    fn a_cover_is_laid_down_once_and_named_by_the_uri_that_reads_it_back() {
        let pictures = Pictures::default();
        let art = art();

        let uri = pictures.uri(track(1), &art).expect("a cover was laid down");
        assert_eq!(pictures.uri(track(1), &art).as_deref(), Some(uri.as_str()));

        let laid = MediaLocation::from_uri(&uri).expect("a local location");
        let path = laid.as_path().expect("a path");
        assert_eq!(fs::read(path).expect("the cover reads back"), art.bytes);
        assert_eq!(path.extension().and_then(|held| held.to_str()), Some("png"));

        pictures.forget();
        assert!(!path.exists(), "a cover outlived the service that laid it");
    }

    fn cover(number: u64) -> Arc<CoverArt> {
        Arc::new(CoverArt {
            format: ImageFormat::Png,
            bytes: format!("\u{89}PNG cover {number}").into_bytes(),
        })
    }

    fn laid(uri: &str) -> PathBuf {
        MediaLocation::from_uri(uri)
            .and_then(|location| location.as_path().map(Path::to_path_buf))
            .expect("a local path")
    }

    #[test]
    fn two_tracks_of_one_album_share_one_cover_and_two_albums_do_not() {
        let pictures = Pictures::default();

        let first = pictures.uri(track(1), &cover(1)).expect("the first cover");
        let again = pictures.uri(track(2), &cover(1)).expect("the same cover");
        let second = pictures.uri(track(3), &cover(2)).expect("the second cover");

        assert_eq!(first, again);
        assert_ne!(first, second);
        pictures.forget();
    }

    #[test]
    fn a_track_id_minted_again_for_another_file_is_drawn_with_that_files_cover() {
        let pictures = Pictures::default();

        let first = pictures.uri(track(u64::MAX), &cover(1)).expect("a cover");
        let next = pictures
            .uri(track(u64::MAX), &cover(2))
            .expect("another cover");

        assert_ne!(
            first, next,
            "a reused id was answered with the last picture"
        );
        assert_eq!(
            fs::read(laid(&next)).expect("the cover reads back"),
            cover(2).bytes
        );
        pictures.forget();
    }

    #[test]
    fn only_the_last_few_covers_are_kept_on_disc() {
        let pictures = Pictures::default();

        let oldest = pictures.uri(track(1), &cover(1)).expect("the first cover");
        for number in 2..=(COVERS_KEPT as u64 + 1) {
            pictures
                .uri(track(number), &cover(number))
                .expect("a cover");
        }

        assert!(!laid(&oldest).exists(), "a cover outlived the ones kept");
        let held = fs::read_dir(&pictures.folder.0)
            .expect("the folder")
            .count();
        assert_eq!(held, COVERS_KEPT);
        pictures.forget();
    }

    #[test]
    fn a_folder_someone_else_made_first_is_never_laid_a_cover_in() {
        let pictures = Pictures::default();
        fs::create_dir_all(&pictures.folder.0).expect("a folder standing where ours would go");

        assert_eq!(pictures.uri(track(1), &art()), None);
        assert_eq!(
            fs::read_dir(&pictures.folder.0)
                .expect("the folder")
                .count(),
            0,
            "a cover was written into a folder this process did not make"
        );
        fs::remove_dir(&pictures.folder.0).expect("the folder taken away");
    }

    #[test]
    fn the_folder_a_cover_is_laid_in_is_readable_by_this_user_alone() {
        use std::os::unix::fs::PermissionsExt as _;

        let pictures = Pictures::default();
        pictures
            .uri(track(1), &art())
            .expect("a cover was laid down");
        let mode = fs::metadata(&pictures.folder.0)
            .expect("the folder")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, OURS_ALONE);
        pictures.forget();
    }

    fn laid_for(pictures: &Pictures, art: &CoverArt) -> PathBuf {
        pictures.folder.0.join(format!(
            "{:016x}.{}",
            digest_of(&art.bytes),
            art.format.extension()
        ))
    }

    #[test]
    fn a_cover_that_could_not_be_written_is_never_named_and_leaves_nothing_behind() {
        let pictures = Pictures::default();
        pictures.uri(track(1), &cover(1)).expect("the folder made");
        let unwritten = cover(2);
        let path = laid_for(&pictures, &unwritten);
        let staged = staging_for(&path);
        fs::create_dir(&staged).expect("something standing where the staging file goes");

        assert_eq!(pictures.uri(track(2), &unwritten), None);
        assert!(
            !path.exists(),
            "a cover that failed was left where the bus reads it"
        );

        fs::remove_dir(&staged).expect("the obstacle taken away");
        let uri = pictures
            .uri(track(2), &unwritten)
            .expect("the cover laid down the second time");
        assert_eq!(fs::read(laid(&uri)).expect("the cover"), unwritten.bytes);
        assert!(!staged.exists(), "the staging file outlived the rename");
        pictures.forget();
    }

    #[test]
    fn a_cover_cut_short_on_disc_is_written_again_rather_than_named() {
        let pictures = Pictures::default();
        pictures.uri(track(1), &cover(1)).expect("the folder made");
        let art = cover(2);
        let path = laid_for(&pictures, &art);
        fs::write(&path, &art.bytes[..3]).expect("a cover cut short");

        let uri = pictures.uri(track(2), &art).expect("the cover laid down");
        assert_eq!(fs::read(laid(&uri)).expect("the cover"), art.bytes);
        pictures.forget();
    }

    #[test]
    fn a_folder_is_named_by_the_process_that_laid_it_down() {
        assert_eq!(laid_down_by("resonate-art-4242-0"), Some(4242));
        assert_eq!(laid_down_by("resonate-art-4242"), None);
        assert_eq!(laid_down_by("resonate-config-4242-0"), None);
    }
}
