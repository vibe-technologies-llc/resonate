use std::{sync::Arc, time::Duration};

use resonate_core::SourceId;
use resonate_library::{KeptLyrics, Library, LookupOp, LyricText, LyricsAsked};
use resonate_lyrics::{LyricOp, LyricProvider, Lyrics, Wanted, read_lyrics, read_lyricsfile};
use serde::Deserialize;

use crate::{Client, Error, Host, query::Params};

const LENGTH_MAY_DIFFER_BY: Duration = Duration::from_secs(30);
const LRCLIB: &str = "lrclib";

#[derive(Deserialize)]
struct Answer {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default, rename = "trackName")]
    track_name: Option<String>,
    #[serde(default, rename = "artistName")]
    artist_name: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    instrumental: bool,
    #[serde(default, rename = "plainLyrics")]
    plain: Option<String>,
    #[serde(default, rename = "syncedLyrics")]
    synced: Option<String>,
    #[serde(default)]
    lyricsfile: Option<String>,
}

impl Answer {
    fn told(self) -> Option<LyricText> {
        if self.instrumental {
            return None;
        }
        let read = present(self.lyricsfile).and_then(|document| {
            read_lyricsfile(source(), &document)
                .map_err(|unread| tracing::debug!(?unread, id = ?self.id, "lrclib answered with a lyricsfile this build cannot read"))
                .ok()
                .map(|read| (document, read))
        });
        let lyricsfile = read
            .as_ref()
            .and_then(|(document, read)| read.says_more_than_its_lines.then(|| document.clone()));
        if let Some(synced) = present(self.synced) {
            return Some(LyricText {
                text: synced,
                synced: true,
                lyricsfile,
            });
        }
        if let Some(plain) = present(self.plain) {
            return Some(LyricText {
                text: plain,
                synced: false,
                lyricsfile,
            });
        }
        let (_, read) = read?;
        let lines: Vec<String> = read
            .lyrics?
            .lines()
            .iter()
            .map(|line| line.text.clone())
            .collect();

        Some(LyricText {
            text: lines.join("\n"),
            synced: false,
            lyricsfile,
        })
    }

    fn names(&self, title: &str, artist: &str) -> bool {
        self.track_name
            .as_deref()
            .is_some_and(|named| folded(named) == folded(title))
            && self
                .artist_name
                .as_deref()
                .is_some_and(|named| folded(named) == folded(artist))
    }

    fn lasts_about(&self, wanted: Option<Duration>) -> bool {
        let Some(wanted) = wanted else {
            return true;
        };
        let Some(lasts) = self
            .duration
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        else {
            return true;
        };

        lasts.abs_diff(wanted) <= LENGTH_MAY_DIFFER_BY
    }
}

fn source() -> SourceId {
    SourceId::new(LRCLIB).unwrap_or_else(|_| SourceId::local())
}

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

pub(crate) fn folded(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn pick(found: Vec<Answer>, asked: &LyricsAsked) -> Option<Answer> {
    found.into_iter().find(|answer| {
        answer.names(&asked.title, &asked.artist) && answer.lasts_about(asked.length)
    })
}

pub(crate) struct Failed {
    op: LyricOp,
    error: Error,
}

impl Failed {
    fn into_lyric_error(self) -> resonate_lyrics::Error {
        self.error.into_lyric_error(source(), self.op)
    }
}

impl From<Failed> for resonate_library::Error {
    fn from(failed: Failed) -> Self {
        failed.error.into()
    }
}

fn ask(client: &Client, asked: &LyricsAsked) -> Result<Option<Answer>, Failed> {
    let seconds = asked.length.map(|length| length.as_secs().to_string());
    let get = Params::new()
        .with("track_name", &asked.title)
        .with("artist_name", &asked.artist)
        .maybe("album_name", asked.album.as_deref())
        .maybe("duration", seconds.as_deref())
        .finish();
    let got: Option<Answer> = client
        .json(Host::Lrclib, LookupOp::Lyrics, &format!("/get{get}"))
        .map_err(|error| Failed {
            op: LyricOp::Fetch,
            error,
        })?;
    if let Some(answer) = got {
        return Ok(Some(answer));
    }

    let search = Params::new()
        .with("track_name", &asked.title)
        .with("artist_name", &asked.artist)
        .finish();
    let found: Vec<Answer> = client
        .json(Host::Lrclib, LookupOp::Lyrics, &format!("/search{search}"))
        .map_err(|error| Failed {
            op: LyricOp::Search,
            error,
        })?
        .unwrap_or_default();

    Ok(pick(found, asked))
}

pub(crate) fn told(client: &Client, asked: &LyricsAsked) -> Result<Option<LyricText>, Failed> {
    let answer = ask(client, asked)?;
    let id = answer.as_ref().and_then(|answer| answer.id);
    let told = answer.and_then(Answer::told);
    tracing::debug!(
        ?id,
        title = asked.title,
        artist = asked.artist,
        synced = told.as_ref().map(|told| told.synced),
        lyricsfile = told.as_ref().is_some_and(|told| told.lyricsfile.is_some()),
        "lrclib answered"
    );

    Ok(told)
}

fn set_of(told: &LyricText) -> resonate_lyrics::Result<Option<Lyrics>> {
    if let Some(document) = &told.lyricsfile {
        match read_lyricsfile(source(), document) {
            Ok(read) if read.lyrics.is_some() => return Ok(read.lyrics),
            Ok(_) => {}
            Err(unread) => {
                tracing::debug!(
                    ?unread,
                    "a kept lyricsfile could not be read, and its lines are read instead"
                );
            }
        }
    }
    if told.synced {
        return read_lyrics(source(), &told.text);
    }

    Ok(Some(Lyrics::plain(
        source(),
        told.text.lines().map(str::to_owned).collect(),
    )))
}

fn asked_of(wanted: &Wanted) -> Option<LyricsAsked> {
    Some(LyricsAsked {
        title: wanted.title.clone()?,
        artist: wanted.artist.clone()?,
        album: wanted.album.clone(),
        length: wanted.duration,
    })
}

pub struct Lrclib {
    client: Arc<Client>,
    library: Option<Arc<Library>>,
    source: SourceId,
}

impl Lrclib {
    pub fn new(client: Arc<Client>, library: Option<Arc<Library>>) -> Self {
        Self {
            client,
            library,
            source: source(),
        }
    }

    fn remembered(&self, wanted: &Wanted) -> Option<KeptLyrics> {
        let library = self.library.as_ref()?;
        match library.kept_lyrics(&wanted.location, wanted.span) {
            Ok(kept) => kept,
            Err(error) => {
                tracing::debug!(%error, location = %wanted.location, "the catalog could not say what it kept");
                None
            }
        }
    }

    fn keep(&self, wanted: &Wanted, told: Option<&LyricText>) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        if let Err(error) = library.keep_lyrics(&wanted.location, wanted.span, told) {
            tracing::debug!(%error, location = %wanted.location, "the catalog could not keep what was told");
        }
    }
}

impl LyricProvider for Lrclib {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, wanted: &Wanted) -> resonate_lyrics::Result<Option<Lyrics>> {
        let Some(asked) = asked_of(wanted) else {
            return Ok(None);
        };

        let kept = self.remembered(wanted);
        if let Some(kept) = &kept
            && !kept.is_due(std::time::SystemTime::now())
        {
            return kept.sung.as_ref().map_or(Ok(None), set_of);
        }

        match told(&self.client, &asked) {
            Ok(told) => {
                self.keep(wanted, told.as_ref());
                let best = match kept {
                    Some(kept) => kept.richer_of(told),
                    None => told,
                };
                best.as_ref().map_or(Ok(None), set_of)
            }
            Err(failed) => match kept.and_then(|kept| kept.sung) {
                Some(held) => set_of(&held),
                None => Err(failed.into_lyric_error()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_lyrics::{Detail, Timing};

    use super::*;

    const GET: &str = include_str!("../tests/fixtures/lrclib_get.json");
    const SEARCH: &str = include_str!("../tests/fixtures/lrclib_search.json");
    const WORDED: &str =
        include_str!("../../resonate-lyrics/tests/fixtures/lyricsfile_worded.yaml");
    const ECHOES_LASTS: Duration = Duration::from_secs(1412);

    fn found() -> Vec<Answer> {
        serde_json::from_str(SEARCH).expect("the fixture parses")
    }

    fn asked(title: &str, artist: &str, length: Option<Duration>) -> LyricsAsked {
        LyricsAsked {
            title: title.to_owned(),
            artist: artist.to_owned(),
            album: None,
            length,
        }
    }

    #[test]
    fn a_length_past_what_a_duration_holds_is_read_as_no_length_rather_than_a_panic() {
        let answer: Answer = serde_json::from_str(
            r#"{"trackName": "Echoes", "artistName": "Pink Floyd", "duration": 1e20}"#,
        )
        .expect("an answer");

        assert!(answer.lasts_about(Some(ECHOES_LASTS)));
    }

    #[test]
    fn an_lrclib_answer_is_synced_where_it_carries_timestamps() {
        let answer: Answer = serde_json::from_str(GET).expect("the fixture parses");
        assert_eq!(answer.id, Some(488_884));
        assert!(answer.names("Echoes", "Pink Floyd"));
        assert!(answer.lasts_about(Some(ECHOES_LASTS)));

        let told = answer.told().expect("the answer carries words");
        assert!(told.synced);
        let lyrics = set_of(&told)
            .expect("the sheet reads")
            .expect("the sheet holds lines");
        assert_eq!(lyrics.timing(), Timing::Synced);
        assert!(lyrics.lines().len() > 30);
        assert!(
            lyrics
                .lines()
                .iter()
                .any(|line| line.text == "Overhead, the albatross")
        );

        let plain_only: Answer = serde_json::from_str(
            r#"{"instrumental":false,"plainLyrics":"all that you touch\nall that you see","syncedLyrics":""}"#,
        )
        .expect("the document parses");
        let told = plain_only.told().expect("the answer carries words");
        assert!(!told.synced);
        let lyrics = set_of(&told)
            .expect("the words read")
            .expect("the words are there");
        assert_eq!(lyrics.timing(), Timing::Unsynced);
        assert_eq!(lyrics.lines().len(), 2);

        let instrumental: Answer = serde_json::from_str(
            r#"{"instrumental":true,"plainLyrics":"la la","syncedLyrics":"[00:01.00]la la"}"#,
        )
        .expect("the document parses");
        assert!(instrumental.told().is_none());

        let bare: Answer = serde_json::from_str(r"{}").expect("the document parses");
        assert!(bare.told().is_none());
    }

    #[test]
    fn a_lyricsfile_an_lrc_was_turned_into_is_not_kept_beside_the_lrc() {
        let answer: Answer = serde_json::from_str(GET).expect("the fixture parses");
        assert!(answer.lyricsfile.is_some());

        let told = answer.told().expect("the answer carries words");
        assert_eq!(told.lyricsfile, None);
    }

    #[test]
    fn a_word_synced_lyricsfile_is_kept_and_read_back_word_by_word() {
        let answer = Answer {
            id: Some(1),
            track_name: Some("Small Hours".to_owned()),
            artist_name: Some("Example Artist".to_owned()),
            duration: Some(10.0),
            instrumental: false,
            plain: Some("Stay until the morning".to_owned()),
            synced: Some("[00:04.20] Stay until the morning".to_owned()),
            lyricsfile: Some(WORDED.to_owned()),
        };

        let told = answer.told().expect("the answer carries words");
        assert!(told.synced);
        assert_eq!(told.lyricsfile.as_deref(), Some(WORDED));
        let lyrics = set_of(&told)
            .expect("the document reads")
            .expect("the document holds lines");
        assert_eq!(lyrics.detail(), Detail::Words);

        let unreadable = LyricText {
            lyricsfile: Some("version: '9'".to_owned()),
            ..told
        };
        let lyrics = set_of(&unreadable)
            .expect("the lines read")
            .expect("the lines are there");
        assert_eq!(lyrics.detail(), Detail::Lines);
    }

    #[test]
    fn a_search_answer_is_taken_only_where_it_names_the_track_and_lasts_about_as_long() {
        assert_eq!(found().len(), 5);

        assert!(pick(found(), &asked("Echoes", "Pink Floyd", Some(ECHOES_LASTS))).is_none());
        assert!(pick(found(), &asked("Echoes", "Pink Floyd", None)).is_none());

        let taken = pick(found(), &asked("echoes - ECHOES", "pink floyd", None))
            .expect("the first is named so");
        assert_eq!(taken.id, Some(18_688_320));

        let taken = pick(
            found(),
            &asked(
                "Echoes - Echoes",
                "Pink Floyd",
                Some(Duration::from_secs(1000)),
            ),
        )
        .expect("the first lasts about as long");
        assert_eq!(taken.id, Some(18_688_320));

        let taken = pick(
            found(),
            &asked("Echoes - Echoes", "Pink Floyd", Some(ECHOES_LASTS)),
        )
        .expect("the last folds to the same name and lasts as long");
        assert_eq!(taken.id, Some(22_369_384));

        assert!(pick(found(), &asked("Echoes - Echoes", "Roger Waters", None)).is_none());
        assert!(
            pick(
                found(),
                &asked(
                    "Echoes - Echoes",
                    "Pink Floyd",
                    Some(Duration::from_secs(5000))
                )
            )
            .is_none()
        );
    }

    #[test]
    fn the_provider_is_named_lrclib_and_refuses_a_track_nothing_has_named() {
        let provider = Lrclib::new(
            Arc::new(Client::new(crate::Identity::of_this_build())),
            None,
        );
        assert_eq!(provider.source().as_str(), LRCLIB);

        let bare = Wanted::for_media(resonate_core::MediaLocation::local("/music/Echoes.flac"));
        assert!(provider.lyrics(&bare).expect("nothing was asked").is_none());
    }
}
