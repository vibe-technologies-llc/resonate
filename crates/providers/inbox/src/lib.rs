use std::{
    fs,
    path::{Path, PathBuf},
};

use resonate_core::SourceId;
use resonate_providers::{Delivery, Error, Identity, Obtained, Provider, ProviderOp, Result};

const INBOX: &str = "inbox";

pub struct Inbox {
    source: SourceId,
    folder: PathBuf,
}

impl Inbox {
    pub fn at(folder: impl Into<PathBuf>) -> Self {
        Self {
            source: SourceId::new(INBOX).unwrap_or_else(|_| SourceId::local()),
            folder: folder.into(),
        }
    }

    fn refused(&self, source: std::io::Error) -> Error {
        Error::Io {
            provider: self.source.clone(),
            op: ProviderOp::ReadFolder,
            source,
        }
    }

    fn files(&self) -> Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in fs::read_dir(&self.folder).map_err(|source| self.refused(source))? {
            let path = entry.map_err(|source| self.refused(source))?.path();
            if path.is_file() {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
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

    fn obtain(&self, identity: &Identity) -> Result<Obtained> {
        let keys = keys_most_exact_first(identity);
        if keys.is_empty() {
            return Ok(Obtained::Nothing);
        }

        let files = self.files()?;
        let named = keys.iter().find_map(|key| {
            files
                .iter()
                .find(|file| stem_of(file).as_deref() == Some(key.as_str()))
        });

        Ok(named.map_or(Obtained::Nothing, |file| {
            Obtained::Found(Delivery::File(file.clone()))
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use resonate_core::{Isrc, Mbid};

    use super::*;

    const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const ECHOES_TRACK: &str = "0a0bb0f4-3e3d-4e71-9b0a-6c3b1b1c7a11";
    const ECHOES_ISRC: &str = "GBN9Y1100089";

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

    fn echoes() -> Identity {
        Identity {
            recording: Some(Mbid::new(ECHOES).expect("an mbid")),
            track: Some(Mbid::new(ECHOES_TRACK).expect("an mbid")),
            isrc: Some(Isrc::new(ECHOES_ISRC).expect("an isrc")),
            ..Identity::named("Echoes")
        }
    }

    fn delivered(obtained: Obtained) -> Option<PathBuf> {
        match obtained {
            Obtained::Found(Delivery::File(path)) => Some(path),
            Obtained::Found(Delivery::Stream { .. }) | Obtained::Nothing => None,
        }
    }

    #[test]
    fn the_recording_is_the_most_exact_name_and_the_isrc_the_least() {
        let folder = Folder::new().holding(&[
            &format!("{}.mp3", ECHOES_ISRC.to_ascii_lowercase()),
            &format!("{ECHOES_TRACK}.flac"),
            &format!("{}.wav", ECHOES.to_ascii_uppercase()),
        ]);
        let inbox = Inbox::at(&folder.0);

        let found = delivered(inbox.obtain(&echoes()).expect("a readable inbox"));
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
        let found = delivered(inbox.obtain(&without_recording).expect("a readable inbox"));
        assert_eq!(found, Some(folder.0.join(format!("{ECHOES_TRACK}.flac"))));

        let isrc_alone = Identity {
            recording: None,
            track: None,
            ..echoes()
        };
        let found = delivered(inbox.obtain(&isrc_alone).expect("a readable inbox"));
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
    fn a_title_is_never_matched_and_nothing_named_is_nothing_delivered() {
        let folder = Folder::new().holding(&["Echoes.flac", "unrelated.flac"]);
        let inbox = Inbox::at(&folder.0);

        assert!(delivered(inbox.obtain(&Identity::named("Echoes")).expect("no read")).is_none());
        assert!(delivered(inbox.obtain(&echoes()).expect("a readable inbox")).is_none());
    }

    #[test]
    fn a_folder_nested_in_the_inbox_is_not_walked() {
        let folder = Folder::new();
        fs::create_dir_all(folder.0.join("deeper")).expect("a nested folder");
        fs::write(
            folder.0.join("deeper").join(format!("{ECHOES}.flac")),
            b"audio",
        )
        .expect("a nested file");

        assert!(
            delivered(
                Inbox::at(&folder.0)
                    .obtain(&echoes())
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
            Inbox::at(&missing).obtain(&echoes()),
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

        let _ = Inbox::at(&folder.0)
            .obtain(&echoes())
            .expect("a readable inbox");

        let after = fs::read(folder.0.join(format!("{ECHOES}.flac"))).expect("the file");
        assert_eq!(before, after);
        assert_eq!(fs::read_dir(&folder.0).expect("the inbox").count(), 1);
    }
}
