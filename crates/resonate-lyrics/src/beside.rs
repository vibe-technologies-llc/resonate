use std::{
    ffi::OsString,
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process,
};

use resonate_core::{
    naming::{NAME_BYTES_AT_MOST, named_within},
    text,
    writer::{self, Held, Writer},
};

use crate::{
    Error, Result,
    sidecar::{BESIDE, WITHIN},
};

const STAGED: &str = "resonate-staging";
const LYRICSFILE_ENDING: usize = 0;
const LRC_ENDING: usize = 2;
const READ_BACK_AT_MOST: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidecarOp {
    Look,
    Stage,
    Settle,
}

#[derive(Clone, Copy, Debug)]
pub struct KeptSheet<'a> {
    pub text: &'a str,
    pub document: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    LeftAlone,
    Replaced,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Beside {
    Written(PathBuf),
    AlreadySaid(PathBuf),
}

impl Beside {
    pub fn path(&self) -> &Path {
        match self {
            Self::Written(path) | Self::AlreadySaid(path) => path,
        }
    }
}

impl<'a> KeptSheet<'a> {
    fn form(self) -> (usize, &'a str) {
        match self.document {
            Some(document) if !a_ttml(document) => (LYRICSFILE_ENDING, document),
            _ => (LRC_ENDING, self.text),
        }
    }
}

pub fn plan_beside(track: &Path, sheet: KeptSheet<'_>, standing: Standing) -> Result<Beside> {
    Ok(Planned::of(track, sheet, standing)?.beside)
}

pub fn write_beside(track: &Path, sheet: KeptSheet<'_>, standing: Standing) -> Result<Beside> {
    let planned = Planned::of(track, sheet, standing)?;
    if let Beside::Written(target) = &planned.beside {
        sweep_what_a_dead_writer_staged(&planned.folder, &planned.name);
        settled(
            &planned.folder,
            &planned.name,
            target,
            planned.body,
            standing,
        )?;
    }

    Ok(planned.beside)
}

struct Planned<'a> {
    folder: PathBuf,
    name: OsString,
    body: &'a str,
    beside: Beside,
}

impl<'a> Planned<'a> {
    fn of(track: &Path, sheet: KeptSheet<'a>, standing: Standing) -> Result<Self> {
        let (ending, body) = sheet.form();
        let stem = track
            .file_stem()
            .filter(|stem| !stem.is_empty())
            .ok_or_else(|| Error::SidecarUnnamed {
                track: track.to_path_buf(),
            })?;
        let mut name = stem.to_os_string();
        name.push(BESIDE[ending].0);
        if name.len() > NAME_BYTES_AT_MOST {
            return Err(Error::SidecarNameTooLong {
                track: track.to_path_buf(),
            });
        }
        let folder = match track.parent() {
            Some(folder) if !folder.as_os_str().is_empty() => folder.to_path_buf(),
            _ => PathBuf::from("."),
        };

        let standing_beside = sheets_named_after(&folder, &stem.to_string_lossy())?;
        let same = standing_beside
            .iter()
            .find(|sheet| sheet.ending == ending && sheet.beside);
        let beside = if let Some(same) = same
            && says(&same.path, body)?
        {
            Beside::AlreadySaid(same.path.clone())
        } else if let Some(first) = standing_beside.first()
            && standing == Standing::LeftAlone
        {
            return Err(Error::SidecarStands {
                path: first.path.clone(),
            });
        } else if let Some(ahead) = standing_beside.iter().find(|sheet| sheet.ending < ending) {
            return Err(Error::SidecarOutranked {
                path: ahead.path.clone(),
            });
        } else {
            Beside::Written(same.map_or_else(|| folder.join(&name), |same| same.path.clone()))
        };

        Ok(Self {
            folder,
            name,
            body,
            beside,
        })
    }
}

struct Found {
    path: PathBuf,
    ending: usize,
    beside: bool,
}

fn sheets_named_after(folder: &Path, stem: &str) -> Result<Vec<Found>> {
    let stem = stem.to_lowercase();
    let mut found = Vec::new();
    for (name, path) in names_in(folder)? {
        if let Some(ending) = ending_after(&name, &stem) {
            found.push(Found {
                path,
                ending,
                beside: true,
            });
        } else if WITHIN.contains(&name.as_str()) && path.is_dir() {
            for (name, path) in names_in(&path)? {
                if let Some(ending) = ending_after(&name, &stem) {
                    found.push(Found {
                        path,
                        ending,
                        beside: false,
                    });
                }
            }
        }
    }
    found.sort_by_key(|sheet| (sheet.ending, !sheet.beside));

    Ok(found)
}

fn names_in(folder: &Path) -> Result<Vec<(String, PathBuf)>> {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(cause) => return Err(unwritten(folder, SidecarOp::Look, cause)),
    };

    Ok(entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_lowercase();
            Some((name, entry.path()))
        })
        .collect())
}

fn ending_after(name: &str, stem: &str) -> Option<usize> {
    BESIDE
        .iter()
        .position(|(ending, _)| name.strip_suffix(ending) == Some(stem))
}

fn says(path: &Path, body: &str) -> Result<bool> {
    let mut held = Vec::new();
    File::open(path)
        .and_then(|file| file.take(READ_BACK_AT_MOST).read_to_end(&mut held))
        .map_err(|cause| unwritten(path, SidecarOp::Look, cause))?;

    Ok(text::decoded(&held).0 == body)
}

fn settled(
    folder: &Path,
    name: &OsString,
    target: &Path,
    body: &str,
    standing: Standing,
) -> Result<()> {
    let staged = folder.join(named_within(
        ".",
        name,
        format!(".{}.{STAGED}", process::id()),
    ));
    let held = Held::made(&staged).map_err(|cause| unwritten(&staged, SidecarOp::Stage, cause))?;
    let landed = written_into(held.file(), body)
        .map_err(|cause| unwritten(&staged, SidecarOp::Stage, cause))
        .and_then(|()| {
            if standing == Standing::LeftAlone && target.exists() {
                return Err(Error::SidecarStands {
                    path: target.to_path_buf(),
                });
            }
            fs::rename(&staged, target).map_err(|cause| unwritten(target, SidecarOp::Settle, cause))
        });
    if landed.is_err() {
        let _ = fs::remove_file(&staged);
    }
    drop(held);

    landed
}

fn written_into(mut file: &File, body: &str) -> io::Result<()> {
    file.write_all(body.as_bytes())?;
    file.sync_all()
}

fn sweep_what_a_dead_writer_staged(folder: &Path, name: &OsString) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    let name = name.to_string_lossy();
    for entry in entries.flatten() {
        let held = entry.file_name();
        let Some(pid) = staged_for(&held.to_string_lossy(), &name) else {
            continue;
        };
        let path = entry.path();
        if writer::writer_of(&path, pid) == Writer::Gone {
            tracing::info!(
                staged = %path.display(),
                "taking away a lyric sheet a run that did not finish staged"
            );
            let _ = fs::remove_file(path);
        }
    }
}

fn staged_for(held: &str, name: &str) -> Option<u32> {
    let rest = held.strip_suffix(STAGED)?.strip_suffix('.')?;
    let (named, pid) = rest.rsplit_once('.')?;
    let named = named.strip_prefix('.')?;

    (!named.is_empty() && name.starts_with(named))
        .then(|| pid.parse().ok())
        .flatten()
}

fn a_ttml(document: &str) -> bool {
    document.trim_start().starts_with('<')
}

fn unwritten(path: &Path, op: SidecarOp, cause: io::Error) -> Error {
    Error::SidecarUnwritten {
        path: path.to_path_buf(),
        op,
        cause,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime},
    };

    use resonate_core::{MediaLocation, SourceId};

    use super::*;
    use crate::{Detail, LyricProvider, Lyrics, Sidecar, Wanted, read_document, read_lyrics};

    const SYNCED: &str = "[00:01.00]all that you touch\n[00:05.00]all that you see\n";
    const WORDED: &str = include_str!("../tests/fixtures/lyricsfile_worded.yaml");
    const TTML: &str = include_str!("../tests/fixtures/ttml_worded.ttml");

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = env::temp_dir().join(format!(
                "resonate-beside-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).expect("a writable temporary directory");
            Self(root)
        }

        fn track(&self) -> PathBuf {
            let track = self.0.join("Echoes.flac");
            fs::write(&track, b"fLaC").expect("a track");
            track
        }

        fn holding(&self, name: &str) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(&self.0)
                .expect("a folder")
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name != "Echoes.flac")
                .collect();
            names.sort();
            names.retain(|held| held.contains(name));
            names
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn read_beside(track: &Path) -> Lyrics {
        Sidecar::default()
            .lyrics(&Wanted::for_media(MediaLocation::local(track)))
            .expect("nothing failed")
            .expect("a sheet beside the track")
    }

    fn lines_of(lyrics: &Lyrics) -> Vec<(Option<Duration>, String, usize)> {
        lyrics
            .lines()
            .iter()
            .map(|line| (line.at, line.text.clone(), line.words.len()))
            .collect()
    }

    fn kept(text: &str) -> KeptSheet<'_> {
        KeptSheet {
            text,
            document: None,
        }
    }

    #[test]
    fn a_synced_sheet_kept_is_written_beside_the_track_and_read_back_as_it_was_kept() {
        let folder = Folder::new();
        let track = folder.track();

        let written =
            write_beside(&track, kept(SYNCED), Standing::LeftAlone).expect("a sheet written");
        let read = read_beside(&track);
        let as_kept = read_lyrics(SourceId::local(), SYNCED)
            .expect("a readable sheet")
            .expect("its lines");

        assert_eq!(written, Beside::Written(folder.0.join("Echoes.lrc")));
        assert_eq!(
            fs::read_to_string(written.path()).expect("its text"),
            SYNCED
        );
        assert_eq!(lines_of(&read), lines_of(&as_kept));
        assert_eq!(folder.holding("resonate-staging"), Vec::<String>::new());
    }

    #[test]
    fn a_plain_sheet_kept_is_written_as_an_lrc_that_reads_back_unsynced() {
        let folder = Folder::new();
        let track = folder.track();
        let plain = "all that you touch\nall that you see\n";

        write_beside(&track, kept(plain), Standing::LeftAlone).expect("a sheet written");
        let read = read_beside(&track);

        assert_eq!(read.detail(), Detail::Unsynced);
        assert_eq!(
            lines_of(&read),
            lines_of(
                &read_lyrics(SourceId::local(), plain)
                    .expect("a readable sheet")
                    .expect("its lines")
            )
        );
    }

    #[test]
    fn a_lyricsfile_kept_is_written_as_the_richest_sidecar_and_read_back_word_by_word() {
        let folder = Folder::new();
        let track = folder.track();
        let sheet = KeptSheet {
            text: SYNCED,
            document: Some(WORDED),
        };

        let written = write_beside(&track, sheet, Standing::LeftAlone).expect("a sheet written");
        let read = read_beside(&track);
        let as_kept = read_document(SourceId::local(), WORDED)
            .expect("a readable document")
            .lyrics
            .expect("its lines");

        assert_eq!(
            written,
            Beside::Written(folder.0.join("Echoes.lyricsfile.yaml"))
        );
        assert_eq!(read.detail(), Detail::Words);
        assert_eq!(lines_of(&read), lines_of(&as_kept));
    }

    #[test]
    fn a_ttml_kept_is_written_as_the_lines_the_catalog_keeps_beside_it() {
        let folder = Folder::new();
        let track = folder.track();
        let sheet = KeptSheet {
            text: SYNCED,
            document: Some(TTML),
        };

        let written = write_beside(&track, sheet, Standing::LeftAlone).expect("a sheet written");

        assert_eq!(written, Beside::Written(folder.0.join("Echoes.lrc")));
        assert_eq!(
            fs::read_to_string(written.path()).expect("its text"),
            SYNCED
        );
    }

    #[test]
    fn a_sheet_standing_beside_the_track_is_not_overwritten_unless_the_listener_replaces_it() {
        let folder = Folder::new();
        let track = folder.track();
        let theirs = "[00:02.00]a sheet of the listener's own\n";
        fs::write(folder.0.join("Echoes.lrc"), theirs).expect("a sheet of their own");

        let refused = write_beside(&track, kept(SYNCED), Standing::LeftAlone);
        let after_refusal = fs::read_to_string(folder.0.join("Echoes.lrc")).expect("their text");
        let replaced =
            write_beside(&track, kept(SYNCED), Standing::Replaced).expect("a sheet replaced");

        assert!(
            matches!(&refused, Err(Error::SidecarStands { path }) if *path == folder.0.join("Echoes.lrc")),
            "{refused:?}"
        );
        assert_eq!(after_refusal, theirs);
        assert_eq!(replaced, Beside::Written(folder.0.join("Echoes.lrc")));
        assert_eq!(
            fs::read_to_string(folder.0.join("Echoes.lrc")).expect("our text"),
            SYNCED
        );
    }

    #[test]
    fn a_sheet_standing_in_a_lyrics_folder_or_spelled_otherwise_is_left_alone_unasked() {
        let folder = Folder::new();
        let track = folder.track();
        fs::create_dir_all(folder.0.join("Lyrics")).expect("a lyrics folder");
        fs::write(
            folder.0.join("Lyrics").join("echoes.LRC"),
            "[00:01.00]theirs",
        )
        .expect("a sheet of their own");

        let refused = write_beside(&track, kept(SYNCED), Standing::LeftAlone);

        assert!(
            matches!(refused, Err(Error::SidecarStands { .. })),
            "{refused:?}"
        );
        assert_eq!(folder.holding(".lrc"), Vec::<String>::new());
    }

    #[test]
    fn a_plan_says_where_the_sheet_would_go_and_writes_nothing() {
        let folder = Folder::new();
        let track = folder.track();

        let planned = plan_beside(&track, kept(SYNCED), Standing::LeftAlone);

        assert_eq!(
            planned.ok(),
            Some(Beside::Written(folder.0.join("Echoes.lrc")))
        );
        assert_eq!(folder.holding(".lrc"), Vec::<String>::new());
    }

    #[test]
    fn the_same_sheet_written_again_is_already_said_and_left_as_it_is() {
        let folder = Folder::new();
        let track = folder.track();

        write_beside(&track, kept(SYNCED), Standing::LeftAlone).expect("a sheet written");
        let again = write_beside(&track, kept(SYNCED), Standing::LeftAlone);

        assert_eq!(
            again.ok(),
            Some(Beside::AlreadySaid(folder.0.join("Echoes.lrc")))
        );
    }

    #[test]
    fn a_richer_sheet_beside_the_track_is_never_hidden_behind_a_plainer_one() {
        let folder = Folder::new();
        let track = folder.track();
        fs::write(folder.0.join("Echoes.lyricsfile.yaml"), WORDED).expect("a richer sheet");

        let refused = write_beside(&track, kept(SYNCED), Standing::Replaced);

        assert!(
            matches!(refused, Err(Error::SidecarOutranked { .. })),
            "{refused:?}"
        );
        assert_eq!(folder.holding(".lrc"), Vec::<String>::new());
    }

    #[test]
    fn a_staged_sheet_a_dead_writer_left_is_swept_by_the_next_write() {
        let folder = Folder::new();
        let track = folder.track();
        let left = folder.0.join(".Echoes.lrc.4194999.resonate-staging");
        let file = File::create(&left).expect("a staged sheet left behind");
        file.set_modified(SystemTime::now() - Duration::from_secs(60))
            .expect("an old stamp");
        drop(file);

        write_beside(&track, kept(SYNCED), Standing::LeftAlone).expect("a sheet written");

        assert!(!left.exists());
    }

    #[test]
    fn only_a_staged_name_for_the_sheet_being_written_is_taken_for_its_staging() {
        assert_eq!(
            staged_for(".Echoes.lrc.42.resonate-staging", "Echoes.lrc"),
            Some(42)
        );
        assert_eq!(
            staged_for(".Echo.42.resonate-staging", "Echoes.lrc"),
            Some(42)
        );
        assert_eq!(
            staged_for(".Other.lrc.42.resonate-staging", "Echoes.lrc"),
            None
        );
        assert_eq!(staged_for("Echoes.lrc", "Echoes.lrc"), None);
        assert_eq!(staged_for("..42.resonate-staging", "Echoes.lrc"), None);
    }
}
