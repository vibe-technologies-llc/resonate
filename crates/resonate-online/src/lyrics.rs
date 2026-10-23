use std::{sync::Arc, time::SystemTime};

use resonate_core::SourceId;
use resonate_library::{KeptLyrics, Library, LyricText, LyricsAsked};
use resonate_lyrics::{
    Detail, LyricOp, LyricProvider, Lyrics, Voice, Wanted, read_document, read_lyrics,
};

use crate::{Client, Error, betterlyrics, lrclib};

const ONLINE_LYRICS: &str = "online";

pub(crate) struct Failed {
    provider: SourceId,
    op: LyricOp,
    error: Error,
}

impl Failed {
    pub(crate) const fn new(provider: SourceId, op: LyricOp, error: Error) -> Self {
        Self {
            provider,
            op,
            error,
        }
    }

    fn into_lyric_error(self) -> resonate_lyrics::Error {
        self.error.into_lyric_error(self.provider, self.op)
    }
}

impl From<Failed> for resonate_library::Error {
    fn from(failed: Failed) -> Self {
        failed.error.into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Singers {
    One,
    Two,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Score {
    detail: Detail,
    singers: Singers,
    ended: bool,
}

impl Score {
    const BEST: Self = Self {
        detail: Detail::Words,
        singers: Singers::Two,
        ended: true,
    };

    fn of(lyrics: &Lyrics) -> Self {
        let lines = lyrics.lines();

        Self {
            detail: lyrics.detail(),
            singers: if lines.iter().any(|line| line.voice == Voice::Two) {
                Singers::Two
            } else {
                Singers::One
            },
            ended: lines.iter().any(|line| line.until.is_some()),
        }
    }
}

#[derive(Clone, Copy)]
enum Service {
    Lrclib,
    BetterLyrics,
}

impl Service {
    const ASKED_IN_TURN: [Self; 2] = [Self::Lrclib, Self::BetterLyrics];

    fn told(self, client: &Client, asked: &LyricsAsked) -> Result<Option<LyricText>, Failed> {
        match self {
            Self::Lrclib => lrclib::told(client, asked),
            Self::BetterLyrics => betterlyrics::told(client, asked),
        }
    }
}

pub(crate) fn score_of(told: &LyricText) -> Option<Score> {
    match set_of(told) {
        Ok(lyrics) => lyrics.as_ref().map(Score::of),
        Err(error) => {
            tracing::debug!(%error, "told lyrics did not read");
            None
        }
    }
}

pub(crate) fn told(client: &Client, asked: &LyricsAsked) -> Result<Option<LyricText>, Failed> {
    let mut best: Option<(Score, LyricText)> = None;
    let mut failed = None;

    for service in Service::ASKED_IN_TURN {
        if best
            .as_ref()
            .is_some_and(|(score, _)| *score == Score::BEST)
        {
            break;
        }
        match service.told(client, asked) {
            Ok(Some(told)) => {
                let Some(score) = score_of(&told) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(held, _)| score > *held) {
                    best = Some((score, told));
                }
            }
            Ok(None) => {}
            Err(refused) => {
                tracing::debug!(error = %refused.error, provider = %refused.provider, "a lyric service failed; the next is asked");
                failed.get_or_insert(refused);
            }
        }
    }
    tracing::debug!(score = ?best.as_ref().map(|(score, _)| score), title = asked.title, "lyric services weighed");

    match (best, failed) {
        (Some((_, told)), _) => Ok(Some(told)),
        (None, Some(failed)) => Err(failed),
        (None, None) => Ok(None),
    }
}

pub fn source_of(document: Option<&str>) -> SourceId {
    match document {
        Some(document) if document.trim_start().starts_with('<') => betterlyrics::source(),
        Some(_) | None => lrclib::source(),
    }
}

pub(crate) fn set_of(told: &LyricText) -> resonate_lyrics::Result<Option<Lyrics>> {
    let source = source_of(told.lyricsfile.as_deref());
    if let Some(document) = &told.lyricsfile {
        match read_document(source.clone(), document) {
            Ok(read) if read.lyrics.is_some() => return Ok(read.lyrics),
            Ok(_) => {}
            Err(unread) => {
                tracing::debug!(
                    ?unread,
                    "a kept document could not be read, and its lines are read instead"
                );
            }
        }
    }
    if told.synced {
        return read_lyrics(source, &told.text);
    }

    Ok(Some(Lyrics::plain(
        source,
        told.text.lines().map(str::to_owned),
    )))
}

fn richer(held: Option<LyricText>, told: Option<LyricText>) -> Option<LyricText> {
    match (held, told) {
        (Some(held), Some(told)) if score_of(&held) > score_of(&told) => Some(held),
        (held, None) => held,
        (_, told) => told,
    }
}

fn asked_of(wanted: &Wanted) -> Option<LyricsAsked> {
    Some(LyricsAsked {
        title: wanted.title.clone()?,
        artist: wanted.artist.clone()?,
        album: wanted.album.clone(),
        length: wanted.duration,
    })
}

pub struct OnlineLyrics {
    client: Arc<Client>,
    library: Option<Arc<Library>>,
    source: SourceId,
}

impl OnlineLyrics {
    pub fn new(client: Arc<Client>, library: Option<Arc<Library>>) -> Self {
        Self {
            client,
            library,
            source: SourceId::new(ONLINE_LYRICS).unwrap_or_else(|_| SourceId::local()),
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

impl LyricProvider for OnlineLyrics {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, wanted: &Wanted) -> resonate_lyrics::Result<Option<Lyrics>> {
        let Some(asked) = asked_of(wanted) else {
            return Ok(None);
        };

        let kept = self.remembered(wanted);
        if let Some(kept) = &kept
            && (!kept.is_due(SystemTime::now()) || !self.client.is_reaching())
        {
            return kept.sung.as_ref().map_or(Ok(None), set_of);
        }

        if !self.client.is_reaching() {
            return Ok(None);
        }
        match told(&self.client, &asked) {
            Ok(told) => {
                self.keep(wanted, told.as_ref());
                richer(kept.and_then(|kept| kept.sung), told)
                    .as_ref()
                    .map_or(Ok(None), set_of)
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
    use super::*;

    const WORDED: &str =
        include_str!("../../resonate-lyrics/tests/fixtures/lyricsfile_worded.yaml");
    const DUET: &str = include_str!("../../resonate-lyrics/tests/fixtures/ttml_duet.ttml");
    const SUNG_ALONE: &str = include_str!("../../resonate-lyrics/tests/fixtures/ttml_worded.ttml");

    fn lined(text: &str) -> LyricText {
        LyricText {
            text: text.to_owned(),
            synced: true,
            lyricsfile: None,
        }
    }

    fn documented(document: &str) -> LyricText {
        LyricText {
            lyricsfile: Some(document.to_owned()),
            ..lined("[00:01.00]la")
        }
    }

    fn scored(told: &LyricText) -> Score {
        score_of(told).expect("the words read")
    }

    #[test]
    fn two_singers_word_by_word_outscore_one_and_words_outscore_lines() {
        let plain = LyricText {
            text: "la la".to_owned(),
            synced: false,
            lyricsfile: None,
        };
        let lines = lined("[00:01.00]la\n[00:02.00]la");
        let voiced_lines = lined("[00:01.00]la\n[00:02.00][v2: la]");
        let words = documented(WORDED);
        let sung_alone = documented(SUNG_ALONE);
        let duet = documented(DUET);

        assert!(scored(&plain) < scored(&lines));
        assert!(scored(&lines) < scored(&voiced_lines));
        assert!(scored(&voiced_lines) < scored(&words));
        assert!(scored(&words) <= scored(&sung_alone));
        assert!(scored(&sung_alone) < scored(&duet));
        assert_eq!(scored(&duet), Score::BEST);
    }

    #[test]
    fn a_kept_set_is_given_up_only_for_one_scoring_at_least_as_well() {
        let duet = documented(DUET);
        let lines = lined("[00:01.00]la");

        assert_eq!(
            richer(Some(duet.clone()), Some(lines.clone())),
            Some(duet.clone())
        );
        assert_eq!(
            richer(Some(lines.clone()), Some(duet.clone())),
            Some(duet.clone())
        );
        assert_eq!(richer(Some(lines.clone()), None), Some(lines.clone()));
        let other = lined("[00:03.00]lo");
        assert_eq!(richer(Some(lines), Some(other.clone())), Some(other));
    }

    #[test]
    fn a_kept_document_names_the_service_that_wrote_it() {
        assert_eq!(source_of(Some(DUET)).as_str(), "betterlyrics");
        assert_eq!(source_of(Some(WORDED)).as_str(), "lrclib");
        assert_eq!(source_of(None).as_str(), "lrclib");

        let lyrics = set_of(&documented(DUET))
            .expect("the document reads")
            .expect("its lines are there");
        assert_eq!(lyrics.source().as_str(), "betterlyrics");
        assert_eq!(lyrics.detail(), Detail::Words);
    }

    #[test]
    fn the_provider_is_named_online_and_refuses_a_track_nothing_has_named() {
        let provider = OnlineLyrics::new(
            Arc::new(Client::new(crate::Identity::of_this_build())),
            None,
        );
        assert_eq!(provider.source().as_str(), ONLINE_LYRICS);

        let bare = Wanted::for_media(resonate_core::MediaLocation::local("/music/Echoes.flac"));
        assert!(provider.lyrics(&bare).expect("nothing was asked").is_none());
    }
}
