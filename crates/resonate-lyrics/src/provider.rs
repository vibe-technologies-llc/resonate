use std::{sync::Arc, time::Duration};

use resonate_core::{FrameSpan, MediaLocation, SampleRate, SourceId};

use crate::{Embedded, Lyrics, Result, Sidecar, lrc};

const UNSOURCED: &str = "none";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub rate: Option<SampleRate>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    pub carried: Option<String>,
}

impl Wanted {
    pub fn for_media(location: MediaLocation) -> Self {
        Self {
            location,
            span: None,
            rate: None,
            title: None,
            artist: None,
            album: None,
            duration: None,
            carried: None,
        }
    }

    pub fn cut_of_the_file(&self, whole: Lyrics) -> Option<Lyrics> {
        let Some(span) = self.span else {
            return Some(whole);
        };
        let rate = self.rate?;
        whole.within(
            span.start().to_duration(rate),
            span.end().map(|end| end.to_duration(rate)),
        )
    }
}

pub fn read_lyrics(source: SourceId, text: &str) -> Result<Option<Lyrics>> {
    Ok(lrc::read(source, text)?.lyrics)
}

pub trait LyricProvider: Send + Sync {
    fn source(&self) -> &SourceId;

    fn lyrics(&self, wanted: &Wanted) -> Result<Option<Lyrics>>;
}

pub struct Unsourced {
    source: SourceId,
}

impl Default for Unsourced {
    fn default() -> Self {
        Self {
            source: SourceId::new(UNSOURCED).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl LyricProvider for Unsourced {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn lyrics(&self, _wanted: &Wanted) -> Result<Option<Lyrics>> {
        Ok(None)
    }
}

pub struct Lyricists {
    providers: Vec<Arc<dyn LyricProvider>>,
}

impl Lyricists {
    pub fn unsourced() -> Self {
        Self {
            providers: vec![Arc::new(Unsourced::default())],
        }
    }

    pub fn local() -> Self {
        Self::unsourced()
            .and(Arc::new(Sidecar::default()))
            .and(Arc::new(Embedded::default()))
    }

    #[must_use]
    pub fn and(mut self, provider: Arc<dyn LyricProvider>) -> Self {
        self.providers
            .retain(|held| held.source() != provider.source());
        self.providers.push(provider);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.providers
            .iter()
            .map(|provider| provider.source().clone())
            .collect()
    }

    pub fn has_a_source(&self) -> bool {
        self.providers
            .iter()
            .any(|provider| provider.source().as_str() != UNSOURCED)
    }

    pub fn find(&self, wanted: &Wanted) -> Result<Option<Lyrics>> {
        let mut refused = None;

        for provider in &self.providers {
            match provider.lyrics(wanted) {
                Ok(Some(lyrics)) if !lyrics.is_empty() => return Ok(Some(lyrics)),
                Ok(_) => {}
                Err(error) => {
                    tracing::debug!(%error, source = %provider.source(), "a lyric provider refused");
                    refused = refused.or(Some(error));
                }
            }
        }

        match refused {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }
}

impl Default for Lyricists {
    fn default() -> Self {
        Self::local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Error, LyricLine, LyricOp};

    struct Held {
        source: SourceId,
        lyrics: Option<Lyrics>,
    }

    impl LyricProvider for Held {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn lyrics(&self, _wanted: &Wanted) -> Result<Option<Lyrics>> {
            Ok(self.lyrics.clone())
        }
    }

    struct Refusing {
        source: SourceId,
    }

    impl LyricProvider for Refusing {
        fn source(&self) -> &SourceId {
            &self.source
        }

        fn lyrics(&self, _wanted: &Wanted) -> Result<Option<Lyrics>> {
            Err(Error::Unreadable {
                provider: self.source.clone(),
                op: LyricOp::Fetch,
            })
        }
    }

    fn named(name: &str) -> SourceId {
        SourceId::new(name).expect("a lowercase name")
    }

    fn wanted() -> Wanted {
        Wanted {
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            ..Wanted::for_media(MediaLocation::local("/music/Echoes.flac"))
        }
    }

    fn holding(name: &str, text: &str) -> Arc<Held> {
        Arc::new(Held {
            source: named(name),
            lyrics: Some(Lyrics::plain(named(name), vec![text.to_owned()])),
        })
    }

    #[test]
    fn a_build_with_nothing_behind_it_finds_no_lyrics_and_says_so() {
        let lyricists = Lyricists::unsourced();

        assert!(!lyricists.has_a_source());
        assert_eq!(lyricists.find(&wanted()).expect("nothing failed"), None);
        assert_eq!(lyricists.names(), vec![named(UNSOURCED)]);
    }

    #[test]
    fn a_local_build_looks_beside_the_file_before_it_reads_what_the_file_carries() {
        let lyricists = Lyricists::local();

        assert!(lyricists.has_a_source());
        assert_eq!(
            lyricists.names(),
            vec![named(UNSOURCED), named("sidecar"), named("embedded")]
        );
    }

    #[test]
    fn a_track_carrying_words_is_read_where_nothing_sits_beside_it() {
        let wanted = Wanted {
            carried: Some("all that you touch".to_owned()),
            ..Wanted::for_media(MediaLocation::local("/music/nothing-is-here/Echoes.flac"))
        };

        let found = Lyricists::local()
            .find(&wanted)
            .expect("nothing failed")
            .expect("the track carries them");

        assert_eq!(found.source(), &named("embedded"));
    }

    #[test]
    fn the_first_provider_that_answers_is_the_one_the_pane_draws() {
        let lyricists = Lyricists::unsourced()
            .and(holding("first", "all that you touch"))
            .and(holding("second", "all that you see"));

        let found = lyricists
            .find(&wanted())
            .expect("nothing failed")
            .expect("a provider answered");

        assert!(lyricists.has_a_source());
        assert_eq!(found.source(), &named("first"));
        assert_eq!(
            found.lines(),
            [LyricLine::untimed("all that you touch")].as_slice()
        );
    }

    #[test]
    fn a_provider_that_refuses_does_not_stop_the_one_behind_it() {
        let lyricists = Lyricists::unsourced()
            .and(Arc::new(Refusing {
                source: named("first"),
            }))
            .and(holding("second", "all that you see"));

        let found = lyricists
            .find(&wanted())
            .expect("the second provider answered")
            .expect("a provider answered");

        assert_eq!(found.source(), &named("second"));
    }

    #[test]
    fn a_refusal_is_only_reported_where_nothing_else_answered() {
        let lyricists = Lyricists::unsourced().and(Arc::new(Refusing {
            source: named("first"),
        }));

        assert!(matches!(
            lyricists.find(&wanted()),
            Err(Error::Unreadable { .. })
        ));
    }

    #[test]
    fn a_provider_holding_nothing_but_blank_lines_is_passed_over() {
        let empty = Arc::new(Held {
            source: named("first"),
            lyrics: Some(Lyrics::plain(named("first"), vec!["  ".to_owned()])),
        });
        let lyricists = Lyricists::unsourced()
            .and(empty)
            .and(holding("second", "all that you see"));

        let found = lyricists
            .find(&wanted())
            .expect("nothing failed")
            .expect("a provider answered");

        assert_eq!(found.source(), &named("second"));
    }

    #[test]
    fn a_track_with_no_tags_is_still_something_a_provider_can_be_asked_about() {
        let bare = Wanted::for_media(MediaLocation::local("/music/Echoes.flac"));

        assert!(bare.title.is_none() && bare.artist.is_none());
        assert!(wanted().title.is_some() && wanted().artist.is_some());
    }
}
