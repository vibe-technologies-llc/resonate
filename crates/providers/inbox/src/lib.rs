use std::{
    fs, io,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{SourceId, names_audio};
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp, Result};

const INBOX: &str = "inbox";
const SETTLES_FOR: Duration = Duration::from_secs(2);
const LISTING_TRUSTED_FOR: Duration = Duration::from_secs(30);
const LOSSLESS: [&str; 10] = [
    "aif", "aiff", "ape", "dff", "dsf", "flac", "rf64", "w64", "wav", "wave",
];
const LOSSLESS_OR_LOSSY: [&str; 6] = ["caf", "m4a", "mka", "mp4", "oga", "wv"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Fidelity {
    Lossless,
    LosslessOrLossy,
    Lossy,
}

impl Fidelity {
    fn of(path: &Path) -> Self {
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let among = |held: &[&str]| {
            held.iter()
                .any(|named| named.eq_ignore_ascii_case(extension))
        };
        if among(&LOSSLESS) {
            Self::Lossless
        } else if among(&LOSSLESS_OR_LOSSY) {
            Self::LosslessOrLossy
        } else {
            Self::Lossy
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    changed: Option<SystemTime>,
}

impl Stamp {
    fn of(metadata: &fs::Metadata) -> Self {
        Self {
            modified: metadata.modified().ok(),
            changed: changed_at(metadata),
        }
    }

    fn touched_within(self, settles_for: Duration, now: SystemTime) -> bool {
        [self.modified, self.changed]
            .into_iter()
            .flatten()
            .any(|touched| {
                now.duration_since(touched)
                    .is_ok_and(|quiet_for| quiet_for < settles_for)
            })
    }
}

fn changed_at(metadata: &fs::Metadata) -> Option<SystemTime> {
    let seconds = u64::try_from(metadata.ctime()).ok()?;
    let nanos = u32::try_from(metadata.ctime_nsec()).ok()?;
    SystemTime::UNIX_EPOCH.checked_add(Duration::new(seconds, nanos))
}

struct Listed {
    path: PathBuf,
    stem: String,
}

struct Listing {
    folder: Stamp,
    read_at: Instant,
    files: Vec<Listed>,
}

pub struct Inbox {
    source: SourceId,
    folder: PathBuf,
    settles_for: Duration,
    listing_trusted_for: Duration,
    listed: Mutex<Option<Listing>>,
}

impl Inbox {
    pub fn at(folder: impl Into<PathBuf>) -> Self {
        Self {
            source: SourceId::new(INBOX).unwrap_or_else(|_| SourceId::local()),
            folder: folder.into(),
            settles_for: SETTLES_FOR,
            listing_trusted_for: LISTING_TRUSTED_FOR,
            listed: Mutex::new(None),
        }
    }

    fn refused(&self, source: io::Error) -> Error {
        Error::Io {
            provider: self.source.clone(),
            op: ProviderOp::ReadFolder,
            source,
        }
    }

    fn read(&self, folder: Stamp) -> Result<Listing> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.folder).map_err(|source| self.refused(source))? {
            let path = entry.map_err(|source| self.refused(source))?.path();
            if let Some(stem) = names_audio(&path).then(|| stem_of(&path)).flatten() {
                files.push(Listed { path, stem });
            }
        }
        files.sort_by(|one, other| {
            (Fidelity::of(&one.path), &one.path).cmp(&(Fidelity::of(&other.path), &other.path))
        });
        Ok(Listing {
            folder,
            read_at: Instant::now(),
            files,
        })
    }

    fn named_by(&self, keys: &[String]) -> Result<Vec<PathBuf>> {
        let folder = fs::metadata(&self.folder)
            .map(|metadata| Stamp::of(&metadata))
            .map_err(|source| self.refused(source))?;
        let mut held = self.listed.lock();
        let still_true = held.as_ref().is_some_and(|listing| {
            listing.folder == folder && listing.read_at.elapsed() < self.listing_trusted_for
        });
        if !still_true {
            *held = Some(self.read(folder)?);
        }
        let files = held.as_ref().map_or(&[][..], |listing| &listing.files[..]);

        Ok(keys
            .iter()
            .flat_map(|key| {
                files
                    .iter()
                    .filter(move |file| file.stem == *key)
                    .map(|file| file.path.clone())
            })
            .collect())
    }

    fn settled(&self, file: PathBuf) -> Result<Option<PathBuf>> {
        let metadata = match fs::metadata(&file) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(self.refused(error)),
        };
        if !metadata.is_file() {
            return Ok(None);
        }
        if Stamp::of(&metadata).touched_within(self.settles_for, SystemTime::now()) {
            return Err(Error::StillArriving {
                provider: self.source.clone(),
                file,
            });
        }
        Ok(Some(file))
    }
}

fn keys_most_exact_first(identity: &Identity) -> Vec<String> {
    [
        identity.recording.as_ref().map(|mbid| mbid.as_str()),
        identity.track.as_ref().map(|mbid| mbid.as_str()),
        identity.isrc.as_ref().map(|isrc| isrc.as_str()),
    ]
    .into_iter()
    .flatten()
    .map(str::to_ascii_lowercase)
    .collect()
}

fn stem_of(path: &Path) -> Option<String> {
    path.file_stem()?.to_str().map(str::to_ascii_lowercase)
}

impl Provider for Inbox {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn find(&self, identity: &Identity) -> Result<Obtained> {
        let keys = keys_most_exact_first(identity);
        if keys.is_empty() {
            return Ok(Obtained::Nothing);
        }

        for file in self.named_by(&keys)? {
            if let Some(file) = self.settled(file)? {
                return Ok(Obtained::Found(Delivery::File(file)));
            }
        }
        Ok(Obtained::Nothing)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env,
        fs::File,
        process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use resonate_core::{Isrc, Mbid};

    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const ECHOES_TRACK: &str = "0a0bb0f4-3e3d-4e71-9b0a-6c3b1b1c7a11";
    const ECHOES_ISRC: &str = "GBN9Y1100089";
    const WANTS_IN_A_POLL: u64 = 500;
    const AN_HOUR: Duration = Duration::from_secs(3600);

    static FOLDERS: AtomicU64 = AtomicU64::new(0);

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "resonate-inbox-{}-{}",
                process::id(),
                FOLDERS.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("a scratch folder");
            Self(path)
        }

        fn holding(self, names: &[&str]) -> Self {
            for name in names {
                fs::write(self.0.join(name), b"audio").expect("a scratch file");
            }
            self
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn settled_at(folder: &Folder) -> Inbox {
        Inbox {
            settles_for: Duration::ZERO,
            ..Inbox::at(&folder.0)
        }
    }

    fn echoes() -> Identity {
        Identity {
            recording: Some(Mbid::new(ECHOES).expect("an mbid")),
            track: Some(Mbid::new(ECHOES_TRACK).expect("an mbid")),
            isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
            ..Identity::named("Echoes")
        }
    }

    fn recording_numbered(nth: u64) -> Identity {
        Identity {
            recording: Some(
                Mbid::new(&format!("{nth:08x}-0000-4000-8000-{nth:012x}")).expect("an mbid"),
            ),
            ..Identity::named("Another")
        }
    }

    fn delivered(obtained: Obtained) -> Option<PathBuf> {
        match obtained {
            Obtained::Found(Delivery::File(path)) => Some(path),
            Obtained::Found(Delivery::Stream { .. }) | Obtained::Nothing => None,
        }
    }

    fn listed_at(inbox: &Inbox) -> Option<Instant> {
        inbox.listed.lock().as_ref().map(|listing| listing.read_at)
    }

    #[test]
    fn the_recording_is_the_most_exact_name_and_the_isrc_the_least() {
        let folder = Folder::new().holding(&[
            &format!("{}.mp3", ECHOES_ISRC.to_ascii_lowercase()),
            &format!("{ECHOES_TRACK}.flac"),
            &format!("{}.wav", ECHOES.to_ascii_uppercase()),
        ]);
        let inbox = settled_at(&folder);

        let found = delivered(inbox.find(&echoes()).expect("a readable inbox"));
        assert_eq!(
            found,
            Some(
                folder
                    .0
                    .join(format!("{}.wav", ECHOES.to_ascii_uppercase()))
            )
        );

        let without_recording = Identity {
            recording: None,
            ..echoes()
        };
        let found = delivered(inbox.find(&without_recording).expect("a readable inbox"));
        assert_eq!(found, Some(folder.0.join(format!("{ECHOES_TRACK}.flac"))));

        let isrc_alone = Identity {
            recording: None,
            track: None,
            ..echoes()
        };
        let found = delivered(inbox.find(&isrc_alone).expect("a readable inbox"));
        assert_eq!(
            found,
            Some(
                folder
                    .0
                    .join(format!("{}.mp3", ECHOES_ISRC.to_ascii_lowercase()))
            )
        );
    }

    #[test]
    fn the_lossless_file_one_name_matches_is_delivered_over_a_lossy_one_whatever_their_names_sort_as()
     {
        let folder = Folder::new().holding(&[
            &format!("{ECHOES}.aac"),
            &format!("{ECHOES}.m4a"),
            &format!("{ECHOES}.mp3"),
            &format!("{ECHOES}.WAV"),
        ]);

        let found = delivered(
            settled_at(&folder)
                .find(&echoes())
                .expect("a readable inbox"),
        );
        assert_eq!(found, Some(folder.0.join(format!("{ECHOES}.WAV"))));

        fs::remove_file(folder.0.join(format!("{ECHOES}.WAV"))).expect("a scratch file");
        let found = delivered(
            settled_at(&folder)
                .find(&echoes())
                .expect("a readable inbox"),
        );
        assert_eq!(found, Some(folder.0.join(format!("{ECHOES}.m4a"))));
    }

    #[test]
    fn a_more_exact_name_is_delivered_before_a_lossless_file_a_looser_one_matches() {
        let folder =
            Folder::new().holding(&[&format!("{ECHOES}.mp3"), &format!("{ECHOES_ISRC}.flac")]);

        let found = delivered(
            settled_at(&folder)
                .find(&echoes())
                .expect("a readable inbox"),
        );

        assert_eq!(found, Some(folder.0.join(format!("{ECHOES}.mp3"))));
    }

    #[test]
    fn a_poll_over_many_wants_reads_the_inbox_once() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.flac"), "unrelated.flac"]);
        let inbox = settled_at(&folder);

        let first = delivered(inbox.find(&echoes()).expect("a readable inbox"));
        let read_at = listed_at(&inbox);
        for nth in 0..WANTS_IN_A_POLL {
            let answered = inbox
                .find(&recording_numbered(nth))
                .expect("a readable inbox");
            assert!(delivered(answered).is_none());
        }
        let last = delivered(inbox.find(&echoes()).expect("a readable inbox"));

        assert!(first.is_some());
        assert_eq!(first, last);
        assert!(read_at.is_some());
        assert_eq!(listed_at(&inbox), read_at);
    }

    #[test]
    fn a_file_dropped_in_after_the_inbox_was_read_is_seen_by_the_next_want() {
        let folder = Folder::new().holding(&["unrelated.flac"]);
        let inbox = settled_at(&folder);

        let before = delivered(inbox.find(&echoes()).expect("a readable inbox"));
        fs::write(folder.0.join(format!("{ECHOES}.flac")), b"audio").expect("a scratch file");
        let after = delivered(inbox.find(&echoes()).expect("a readable inbox"));

        assert!(before.is_none());
        assert_eq!(after, Some(folder.0.join(format!("{ECHOES}.flac"))));
    }

    #[test]
    fn a_listing_older_than_it_is_trusted_for_is_read_again() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.flac")]);
        let inbox = Inbox {
            listing_trusted_for: Duration::ZERO,
            ..settled_at(&folder)
        };

        let _ = inbox.find(&echoes()).expect("a readable inbox");
        let first = listed_at(&inbox);
        let _ = inbox.find(&echoes()).expect("a readable inbox");

        assert_ne!(listed_at(&inbox), first);
    }

    #[test]
    fn a_file_still_being_copied_in_is_not_delivered_and_the_inbox_is_not_away() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.flac")]);
        let arriving = folder.0.join(format!("{ECHOES}.flac"));
        let inbox = Inbox {
            settles_for: AN_HOUR,
            ..Inbox::at(&folder.0)
        };

        let Err(error) = inbox.find(&echoes()) else {
            panic!("a file still arriving was delivered");
        };

        assert!(matches!(
            &error,
            Error::StillArriving { file, .. } if *file == arriving
        ));
        assert!(!error.is_the_provider_away());
        assert_eq!(
            delivered(
                settled_at(&folder)
                    .find(&echoes())
                    .expect("a readable inbox")
            ),
            Some(arriving)
        );
    }

    #[test]
    fn a_copy_that_kept_its_old_modification_time_is_still_arriving_by_its_change_time() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.flac")]);
        File::options()
            .write(true)
            .open(folder.0.join(format!("{ECHOES}.flac")))
            .and_then(|file| file.set_modified(SystemTime::now() - AN_HOUR * 24))
            .expect("an old modification time");
        let inbox = Inbox {
            settles_for: AN_HOUR,
            ..Inbox::at(&folder.0)
        };

        assert!(matches!(
            inbox.find(&echoes()),
            Err(Error::StillArriving { .. })
        ));
    }

    #[test]
    fn a_file_untouched_for_longer_than_it_settles_for_has_settled() {
        let settled = Stamp {
            modified: Some(SystemTime::now() - SETTLES_FOR * 2),
            changed: Some(SystemTime::now() - SETTLES_FOR * 2),
        };
        let arriving = Stamp {
            changed: Some(SystemTime::now()),
            ..settled
        };

        assert!(!settled.touched_within(SETTLES_FOR, SystemTime::now()));
        assert!(arriving.touched_within(SETTLES_FOR, SystemTime::now()));
    }

    #[test]
    fn a_time_in_the_future_holds_nothing_back() {
        let ahead = Stamp {
            modified: Some(SystemTime::now() + AN_HOUR),
            changed: None,
        };

        assert!(!ahead.touched_within(SETTLES_FOR, SystemTime::now()));
    }

    #[test]
    fn a_title_is_never_matched_and_nothing_named_is_nothing_delivered() {
        let folder = Folder::new().holding(&["Echoes.flac", "unrelated.flac"]);
        let inbox = settled_at(&folder);

        assert!(delivered(inbox.find(&Identity::named("Echoes")).expect("no read")).is_none());
        assert!(delivered(inbox.find(&echoes()).expect("a readable inbox")).is_none());
    }

    #[test]
    fn only_audio_is_delivered_whatever_else_shares_its_name() {
        let folder = Folder::new().holding(&[
            &format!("{ECHOES}.cue"),
            &format!("{ECHOES}.jpg"),
            &format!("{ECHOES}.LOG"),
            &format!("{ECHOES}.FLAC"),
        ]);

        let found = delivered(
            settled_at(&folder)
                .find(&echoes())
                .expect("a readable inbox"),
        );

        assert_eq!(found, Some(folder.0.join(format!("{ECHOES}.FLAC"))));
    }

    #[test]
    fn a_name_matched_by_no_audio_file_delivers_nothing() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.cue"), &format!("{ECHOES}.jpg")]);

        assert!(
            delivered(
                settled_at(&folder)
                    .find(&echoes())
                    .expect("a readable inbox")
            )
            .is_none()
        );
    }

    #[test]
    fn a_folder_nested_in_the_inbox_is_not_walked() {
        let folder = Folder::new();
        fs::create_dir_all(folder.0.join("deeper")).expect("a nested folder");
        fs::create_dir_all(folder.0.join(format!("{ECHOES}.flac"))).expect("a folder named so");
        fs::write(
            folder.0.join("deeper").join(format!("{ECHOES}.flac")),
            b"audio",
        )
        .expect("a nested file");

        assert!(
            delivered(
                settled_at(&folder)
                    .find(&echoes())
                    .expect("a readable inbox")
            )
            .is_none()
        );
    }

    #[test]
    fn an_inbox_that_is_not_there_refuses_rather_than_answering_nothing() {
        let folder = Folder::new();
        let missing = folder.0.join("not-there");

        assert!(matches!(
            Inbox::at(&missing).find(&echoes()),
            Err(Error::Io {
                op: ProviderOp::ReadFolder,
                ..
            })
        ));
    }

    #[test]
    fn the_inbox_is_left_exactly_as_it_was() {
        let folder = Folder::new().holding(&[&format!("{ECHOES}.flac")]);
        let before = fs::read(folder.0.join(format!("{ECHOES}.flac"))).expect("the file");

        let _ = settled_at(&folder)
            .find(&echoes())
            .expect("a readable inbox");

        let after = fs::read(folder.0.join(format!("{ECHOES}.flac"))).expect("the file");
        assert_eq!(before, after);
        assert_eq!(fs::read_dir(&folder.0).expect("the inbox").count(), 1);
    }
}
