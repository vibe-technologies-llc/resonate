use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::SourceId;
use resonate_library::{KeptLyrics, Library, LookupOp};
use resonate_lyrics::{LyricOp, LyricProvider, Lyrics, Wanted, read_lyrics};
use serde::Deserialize;

use crate::{Client, Host, query::Params};

pub(crate) const ASK_AGAIN_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);
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
}

enum Told {
    Synced(String),
    Plain(String),
    Nothing,
}

impl Answer {
    fn told(self) -> Told {
        if self.instrumental {
            return Told::Nothing;
        }
        if let Some(synced) = present(self.synced) {
            return Told::Synced(synced);
        }
        if let Some(plain) = present(self.plain) {
            return Told::Plain(plain);
        }

        Told::Nothing
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

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

pub(crate) fn folded(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn pick(
    found: Vec<Answer>,
    title: &str,
    artist: &str,
    duration: Option<Duration>,
) -> Option<Answer> {
    found
        .into_iter()
        .find(|answer| answer.names(title, artist) && answer.lasts_about(duration))
}

fn still_fresh(taken: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(taken)
        .is_ok_and(|age| age < ASK_AGAIN_AFTER)
        || taken > SystemTime::now()
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
            source: SourceId::new(LRCLIB).unwrap_or_else(|_| SourceId::local()),
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

    fn keep(&self, wanted: &Wanted, text: Option<&str>, synced: bool) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        if let Err(error) = library.keep_lyrics(&wanted.location, wanted.span, text, synced) {
            tracing::debug!(%error, location = %wanted.location, "the catalog could not keep what was told");
        }
    }

    fn set_of(&self, text: &str, synced: bool) -> resonate_lyrics::Result<Option<Lyrics>> {
        if synced {
            return read_lyrics(self.source.clone(), text);
        }

        Ok(Some(Lyrics::plain(
            self.source.clone(),
            text.lines().map(str::to_owned).collect(),
        )))
    }

    fn ask(
        &self,
        wanted: &Wanted,
        title: &str,
        artist: &str,
    ) -> resonate_lyrics::Result<Option<Answer>> {
        let seconds = wanted
            .duration
            .map(|duration| duration.as_secs().to_string());
        let get = Params::new()
            .with("track_name", title)
            .with("artist_name", artist)
            .maybe("album_name", wanted.album.as_deref())
            .maybe("duration", seconds.as_deref())
            .finish();
        let got: Option<Answer> = self
            .client
            .json(Host::Lrclib, LookupOp::Lyrics, &format!("/get{get}"))
            .map_err(|error| error.into_lyric_error(self.source.clone(), LyricOp::Fetch))?;
        if let Some(answer) = got {
            return Ok(Some(answer));
        }

        let search = Params::new()
            .with("track_name", title)
            .with("artist_name", artist)
            .finish();
        let found: Vec<Answer> = self
            .client
            .json(Host::Lrclib, LookupOp::Lyrics, &format!("/search{search}"))
            .map_err(|error| error.into_lyric_error(self.source.clone(), LyricOp::Search))?
            .unwrap_or_default();

        Ok(pick(found, title, artist, wanted.duration))
    }
}

impl LyricProvider for Lrclib {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, wanted: &Wanted) -> resonate_lyrics::Result<Option<Lyrics>> {
        let (Some(title), Some(artist)) = (wanted.title.as_deref(), wanted.artist.as_deref())
        else {
            return Ok(None);
        };

        match self.remembered(wanted) {
            Some(KeptLyrics {
                text: Some(text),
                synced,
                ..
            }) => return self.set_of(&text, synced),
            Some(KeptLyrics {
                text: None, taken, ..
            }) if still_fresh(taken) => return Ok(None),
            _ => {}
        }

        let answer = self.ask(wanted, title, artist)?;
        let id = answer.as_ref().and_then(|answer| answer.id);
        match answer.map_or(Told::Nothing, Answer::told) {
            Told::Synced(text) => {
                tracing::debug!(?id, title, artist, "lrclib answered with a synced set");
                let lyrics = self.set_of(&text, true)?;
                self.keep(wanted, Some(&text), true);
                Ok(lyrics)
            }
            Told::Plain(text) => {
                tracing::debug!(?id, title, artist, "lrclib answered with plain words");
                let lyrics = self.set_of(&text, false)?;
                self.keep(wanted, Some(&text), false);
                Ok(lyrics)
            }
            Told::Nothing => {
                tracing::debug!(?id, title, artist, "lrclib holds nothing for the track");
                self.keep(wanted, None, false);
                Ok(None)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use resonate_lyrics::Timing;

    use super::*;

    const GET: &str = include_str!("../tests/fixtures/lrclib_get.json");
    const SEARCH: &str = include_str!("../tests/fixtures/lrclib_search.json");
    const ECHOES_LASTS: Duration = Duration::from_secs(1412);

    fn source() -> SourceId {
        SourceId::new(LRCLIB).expect("a lowercase name")
    }

    fn found() -> Vec<Answer> {
        serde_json::from_str(SEARCH).expect("the fixture parses")
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

        let Told::Synced(text) = answer.told() else {
            panic!("the answer carries timestamps");
        };
        let lyrics = read_lyrics(source(), &text)
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
        let Told::Plain(text) = plain_only.told() else {
            panic!("the answer carries words with no timestamps");
        };
        let lyrics = Lyrics::plain(source(), text.lines().map(str::to_owned).collect());
        assert_eq!(lyrics.timing(), Timing::Unsynced);
        assert_eq!(lyrics.lines().len(), 2);

        let instrumental: Answer = serde_json::from_str(
            r#"{"instrumental":true,"plainLyrics":"la la","syncedLyrics":"[00:01.00]la la"}"#,
        )
        .expect("the document parses");
        assert!(matches!(instrumental.told(), Told::Nothing));

        let bare: Answer = serde_json::from_str(r"{}").expect("the document parses");
        assert!(matches!(bare.told(), Told::Nothing));
    }

    #[test]
    fn a_search_answer_is_taken_only_where_it_names_the_track_and_lasts_about_as_long() {
        assert_eq!(found().len(), 5);

        assert!(pick(found(), "Echoes", "Pink Floyd", Some(ECHOES_LASTS)).is_none());
        assert!(pick(found(), "Echoes", "Pink Floyd", None).is_none());

        let taken =
            pick(found(), "echoes - ECHOES", "pink floyd", None).expect("the first is named so");
        assert_eq!(taken.id, Some(18_688_320));

        let taken = pick(
            found(),
            "Echoes - Echoes",
            "Pink Floyd",
            Some(Duration::from_secs(1000)),
        )
        .expect("the first lasts about as long");
        assert_eq!(taken.id, Some(18_688_320));

        let taken = pick(found(), "Echoes - Echoes", "Pink Floyd", Some(ECHOES_LASTS))
            .expect("the last folds to the same name and lasts as long");
        assert_eq!(taken.id, Some(22_369_384));

        assert!(pick(found(), "Echoes - Echoes", "Roger Waters", None).is_none());
        assert!(
            pick(
                found(),
                "Echoes - Echoes",
                "Pink Floyd",
                Some(Duration::from_secs(5000))
            )
            .is_none()
        );
    }

    #[test]
    fn a_kept_miss_is_fresh_for_a_week_and_stale_after() {
        let now = SystemTime::now();
        assert!(still_fresh(now));
        assert!(still_fresh(now - Duration::from_secs(60)));
        assert!(still_fresh(now + Duration::from_secs(60)));
        assert!(!still_fresh(now - ASK_AGAIN_AFTER - Duration::from_secs(1)));
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
