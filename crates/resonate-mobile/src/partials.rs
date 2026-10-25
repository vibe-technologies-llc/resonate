use std::{
    ffi::OsStr,
    fs,
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use resonate_core::{
    naming::named_within,
    writer::{self, Held as Locked, Writer},
};
use resonate_providers::{Held, Validator};

use crate::{collections::wanted_key, downloads::file_stem_of, features::SearchSong};

const FOLDER: &str = "download-partials";
const DATA_SUFFIX: &str = "resonate-part";
const NOTES_SUFFIX: &str = "resonate-note";
const NOTES_VERSION: &str = "1";
const NO_STAMP: u32 = 0;
const REPLAY_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

pub(crate) struct Partials {
    folder: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
struct Notes {
    wanted: String,
    taken_from: String,
    total: Option<u64>,
    validator: Option<Validator>,
}

impl Notes {
    fn written(&self) -> String {
        let total = self.total.map(|total| total.to_string());
        let validator = self.validator.as_ref().map(Validator::as_str);
        [
            NOTES_VERSION,
            &self.wanted,
            &self.taken_from,
            total.as_deref().unwrap_or_default(),
            validator.unwrap_or_default(),
        ]
        .iter()
        .map(|field| format!("{}:{field}", field.len()))
        .collect()
    }

    fn read(text: &str) -> Option<Self> {
        let mut rest = text;
        let mut fields = Vec::new();
        while !rest.is_empty() {
            let (length, after) = rest.split_once(':')?;
            let length: usize = length.parse().ok()?;
            fields.push(after.get(..length)?);
            rest = &after[length..];
        }
        match fields.as_slice() {
            [NOTES_VERSION, wanted, taken_from, total, validator] => Some(Self {
                wanted: (*wanted).to_owned(),
                taken_from: (*taken_from).to_owned(),
                total: total.parse().ok(),
                validator: Validator::new(validator),
            }),
            _ => None,
        }
    }
}

pub(crate) struct Partial {
    locked: Locked,
    data: PathBuf,
    notes: PathBuf,
    held: Option<Held>,
    from: u64,
}

impl Partials {
    pub(crate) fn under(folder: &Path) -> Self {
        Self {
            folder: folder.join(FOLDER),
        }
    }

    fn paths(&self, song: &SearchSong) -> (PathBuf, PathBuf) {
        let stem = file_stem_of(song);
        let key = crc32fast::hash(wanted_key(song).as_bytes());
        let named = |suffix: &str| {
            self.folder.join(named_within(
                "",
                OsStr::new(&stem),
                format!(".{key:08x}.{suffix}"),
            ))
        };
        (named(DATA_SUFFIX), named(NOTES_SUFFIX))
    }

    pub(crate) fn claim(&self, song: &SearchSong, taken_from: &str) -> Option<Partial> {
        if let Err(error) = fs::create_dir_all(&self.folder) {
            tracing::warn!(%error, "the folder for partial downloads could not be made");
            return None;
        }
        let (data, notes) = self.paths(song);
        let locked = match Locked::resumed(&data) {
            Ok(locked) => locked,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Locked::made(&data)
                .map_err(|error| claim_refused(&error))
                .ok()?,
            Err(error) => {
                claim_refused(&error);
                return None;
            }
        };
        let bytes = locked.file().metadata().ok()?.len();
        let wanted = wanted_key(song);
        let held = fs::read(&notes)
            .ok()
            .and_then(|text| Notes::read(&String::from_utf8_lossy(&text)))
            .filter(|noted| noted.wanted == wanted && noted.taken_from == taken_from)
            .filter(|_| bytes > 0)
            .map(|noted| Held {
                bytes,
                total: noted.total,
                validator: noted.validator,
            });
        Some(Partial {
            locked,
            data,
            notes,
            held,
            from: 0,
        })
    }

    pub(crate) fn sweep(&self, now: SystemTime) {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            if let Some(stem) = name.strip_suffix(DATA_SUFFIX) {
                let stale = entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .ok()
                    .and_then(|modified| now.duration_since(modified).ok())
                    .is_some_and(|untouched| untouched >= STALE_AFTER);
                if stale && writer::writer_of(&path, NO_STAMP) == Writer::Gone {
                    let _ = fs::remove_file(&path);
                    let _ = fs::remove_file(path.with_file_name(format!("{stem}{NOTES_SUFFIX}")));
                }
            } else if let Some(stem) = name.strip_suffix(NOTES_SUFFIX)
                && !path.with_file_name(format!("{stem}{DATA_SUFFIX}")).exists()
            {
                let _ = fs::remove_file(&path);
            }
        }
    }
}

fn claim_refused(error: &io::Error) {
    tracing::warn!(%error, "a partial download could not be claimed");
}

impl Partial {
    pub(crate) fn held(&self) -> Option<Held> {
        self.held.clone()
    }

    pub(crate) const fn from(&self) -> u64 {
        self.from
    }

    pub(crate) fn begun(
        mut self,
        song: &SearchSong,
        taken_from: &str,
        from: u64,
        total: Option<u64>,
        validator: Option<Validator>,
    ) -> io::Result<Self> {
        let kept = self.held.take().map_or(0, |held| held.bytes);
        if from > kept {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut file = self.locked.file();
        file.set_len(from)?;
        file.seek(SeekFrom::Start(from))?;
        let notes = Notes {
            wanted: wanted_key(song),
            taken_from: taken_from.to_owned(),
            total,
            validator,
        };
        fs::write(&self.notes, notes.written())?;
        self.from = from;
        Ok(self)
    }

    pub(crate) fn append(&self, chunk: &[u8]) -> io::Result<()> {
        let mut file = self.locked.file();
        file.write_all(chunk)
    }

    pub(crate) fn kept(&self) -> io::Result<()> {
        self.locked.file().sync_data()
    }

    pub(crate) fn replay(
        &self,
        into: &mut dyn FnMut(&[u8]) -> crate::Result<()>,
    ) -> crate::Result<()> {
        use crate::{Error, error::DownloadOp};

        let mut file = self.locked.file();
        file.seek(SeekFrom::Start(0))
            .map_err(|source| Error::DownloadIo {
                op: DownloadOp::Read,
                source,
            })?;
        let mut buffer = vec![0_u8; REPLAY_CHUNK_BYTES];
        loop {
            let count = file.read(&mut buffer).map_err(|source| Error::DownloadIo {
                op: DownloadOp::Read,
                source,
            })?;
            if count == 0 {
                return Ok(());
            }
            into(&buffer[..count])?;
        }
    }

    pub(crate) fn finished(self) {
        let _ = fs::remove_file(&self.data);
        let _ = fs::remove_file(&self.notes);
    }
}

#[cfg(test)]
mod tests {
    use std::{env, process};

    use super::*;

    const TAKEN_FROM: &str = "monochrome:track/42";

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = env::temp_dir().join(format!("resonate-partials-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn echoes() -> SearchSong {
        SearchSong {
            recording: "8f2eb3e7-4ff2-4b14-8c0e-8b7c1e7c2b7d".to_owned(),
            title: "Echoes".to_owned(),
            artist: "Pink Floyd".to_owned(),
            ..SearchSong::default()
        }
    }

    fn validator(text: &str) -> Option<Validator> {
        Validator::new(text)
    }

    fn partial_of(partials: &Partials, from: u64, chunk: &[u8]) -> Partial {
        let partial = partials
            .claim(&echoes(), TAKEN_FROM)
            .expect("a claimed partial")
            .begun(&echoes(), TAKEN_FROM, from, Some(100), validator("\"v1\""))
            .expect("a begun partial");
        partial.append(chunk).expect("written");
        partial
    }

    #[test]
    fn notes_survive_being_written_whatever_they_hold() {
        let notes = Notes {
            wanted: "named:4:a:b\n5:c:".to_owned(),
            taken_from: "tidal:track/1".to_owned(),
            total: Some(7),
            validator: validator("W/\"x:y\""),
        };
        let bare = Notes {
            wanted: String::new(),
            taken_from: String::new(),
            total: None,
            validator: None,
        };

        assert_eq!(Notes::read(&notes.written()), Some(notes));
        assert_eq!(Notes::read(&bare.written()), Some(bare));
        assert_eq!(Notes::read("garbage"), None);
        assert_eq!(Notes::read("9:1"), None);
        assert_eq!(Notes::read("1:2"), None);
    }

    #[test]
    fn a_claim_on_nothing_held_is_a_fresh_partial() {
        let scratch = Scratch::new("fresh");
        let partials = Partials::under(&scratch.0);

        let claimed = partials.claim(&echoes(), TAKEN_FROM).expect("a claim");

        assert_eq!(claimed.held(), None);
    }

    #[test]
    fn what_a_stopped_download_wrote_is_held_for_the_same_offer_and_for_none_other() {
        let scratch = Scratch::new("same-offer");
        let partials = Partials::under(&scratch.0);
        drop(partial_of(&partials, 0, b"abcde"));

        let same = partials.claim(&echoes(), TAKEN_FROM).expect("a claim");
        let held = same.held();
        drop(same);
        let other = partials
            .claim(&echoes(), "monochrome:track/43")
            .expect("a claim");

        assert_eq!(
            held,
            Some(Held {
                bytes: 5,
                total: Some(100),
                validator: validator("\"v1\""),
            })
        );
        assert_eq!(other.held(), None);
    }

    #[test]
    fn a_partial_being_written_is_claimed_by_no_other_job() {
        let scratch = Scratch::new("living");
        let partials = Partials::under(&scratch.0);
        let writing = partial_of(&partials, 0, b"abc");

        let second = partials.claim(&echoes(), TAKEN_FROM);

        assert!(second.is_none());
        drop(writing);
        assert!(partials.claim(&echoes(), TAKEN_FROM).is_some());
    }

    #[test]
    fn a_resumed_partial_is_cut_to_where_the_server_continues_and_replays_whole() {
        let scratch = Scratch::new("resumed");
        let partials = Partials::under(&scratch.0);
        drop(partial_of(&partials, 0, b"abcdef"));

        let resumed = partials
            .claim(&echoes(), TAKEN_FROM)
            .expect("a claim")
            .begun(&echoes(), TAKEN_FROM, 6, Some(9), validator("\"v1\""))
            .expect("resumed");
        resumed.append(b"ghi").expect("written");
        let mut replayed = Vec::new();
        resumed
            .replay(&mut |chunk| {
                replayed.extend_from_slice(chunk);
                Ok(())
            })
            .expect("replayed");

        assert_eq!(resumed.from(), 6);
        assert_eq!(replayed, b"abcdefghi");
    }

    #[test]
    fn a_server_restarting_from_the_start_empties_the_partial() {
        let scratch = Scratch::new("restart");
        let partials = Partials::under(&scratch.0);
        drop(partial_of(&partials, 0, b"abcdef"));

        let restarted = partials
            .claim(&echoes(), TAKEN_FROM)
            .expect("a claim")
            .begun(&echoes(), TAKEN_FROM, 0, Some(3), validator("\"v2\""))
            .expect("restarted");
        restarted.append(b"xyz").expect("written");
        let mut replayed = Vec::new();
        restarted
            .replay(&mut |chunk| {
                replayed.extend_from_slice(chunk);
                Ok(())
            })
            .expect("replayed");

        assert_eq!(replayed, b"xyz");
    }

    #[test]
    fn a_server_continuing_from_beyond_what_is_held_is_refused() {
        let scratch = Scratch::new("beyond");
        let partials = Partials::under(&scratch.0);
        drop(partial_of(&partials, 0, b"abc"));

        let refused = partials
            .claim(&echoes(), TAKEN_FROM)
            .expect("a claim")
            .begun(&echoes(), TAKEN_FROM, 9, Some(20), None)
            .map(drop);

        assert_eq!(
            refused.map_err(|error| error.kind()),
            Err(io::ErrorKind::InvalidData)
        );
    }

    #[test]
    fn a_finished_partial_leaves_nothing_behind() {
        let scratch = Scratch::new("finished");
        let partials = Partials::under(&scratch.0);

        partial_of(&partials, 0, b"abc").finished();

        assert_eq!(
            fs::read_dir(&partials.folder).expect("the folder").count(),
            0
        );
    }

    #[test]
    fn a_sweep_takes_a_partial_untouched_for_long_unless_a_writer_holds_it() {
        let scratch = Scratch::new("sweep");
        let partials = Partials::under(&scratch.0);
        let later = SystemTime::now() + STALE_AFTER * 2;
        let writing = partial_of(&partials, 0, b"abc");

        partials.sweep(SystemTime::now());
        let young = fs::read_dir(&partials.folder).expect("the folder").count();
        partials.sweep(later);
        let held = fs::read_dir(&partials.folder).expect("the folder").count();
        drop(writing);
        partials.sweep(later);
        let left = fs::read_dir(&partials.folder).expect("the folder").count();

        assert_eq!((young, held, left), (2, 2, 0));
    }

    #[test]
    fn a_note_whose_partial_is_gone_is_swept_with_no_age_to_wait_for() {
        let scratch = Scratch::new("orphan");
        let partials = Partials::under(&scratch.0);
        let (data, notes) = partials.paths(&echoes());
        fs::create_dir_all(&partials.folder).expect("a folder");
        fs::write(&notes, "left").expect("a note");

        partials.sweep(SystemTime::now());

        assert!(!notes.exists());
        assert!(!data.exists());
    }
}
