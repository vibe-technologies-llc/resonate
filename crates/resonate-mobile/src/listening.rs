use std::sync::Arc;

use resonate_core::{SampleRate, SourceId, folded_letters};
use resonate_listen::{Clip, Error as ListenError, Heard, Recognisers, Recognition};
use resonate_online::{Audd, Shazam};

use crate::{Error, MobilePlayer, Result};

const HEARD_IN_MONO: u16 = 1;
const CLIP_SAMPLES_AT_MOST: usize = 30 * 192_000;
const TITLE_APART_FROM_ARTIST: char = '\u{1f}';
const SERVICE_NAMES: [(&str, &str); 2] = [("shazam", "Shazam"), ("audd", "AudD")];

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct HeardSong {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<u16>,
    pub isrc: String,
    pub recording: String,
    pub picture: Option<Vec<u8>>,
    pub link: String,
    pub by: String,
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum HeardAnswer {
    Named { song: HeardSong },
    Missed,
    Silent,
    Unreached { service: String },
}

#[uniffi::export]
impl MobilePlayer {
    pub fn recognise_heard(
        &self,
        samples: Vec<f32>,
        rate: u32,
        audd_token: String,
    ) -> Result<HeardAnswer> {
        refuse_a_long_clip(samples.len())?;
        self.require_online()?;
        let clip = Clip {
            rate: SampleRate::new(rate)?,
            channels: HEARD_IN_MONO,
            samples,
        };
        answer_of(self.recognisers(audd_token).recognise(&clip))
    }
}

impl MobilePlayer {
    fn recognisers(&self, audd_token: String) -> Recognisers {
        let client = self.services.online.client();
        let shazam = Recognisers::none().and(Arc::new(Shazam::new(Arc::clone(client))));
        if audd_token.trim().is_empty() {
            shazam
        } else {
            shazam.and(Arc::new(Audd::new(Arc::clone(client), audd_token)))
        }
    }
}

fn refuse_a_long_clip(samples: usize) -> Result<()> {
    if samples > CLIP_SAMPLES_AT_MOST {
        return Err(Error::ClipTooLong {
            samples: samples as u64,
        });
    }
    Ok(())
}

fn answer_of(recognition: resonate_listen::Result<Recognition>) -> Result<HeardAnswer> {
    Ok(match recognition {
        Err(ListenError::NothingHeard) => HeardAnswer::Silent,
        Err(error) => return Err(error.into()),
        Ok(Recognition {
            heard: Some(heard), ..
        }) => HeardAnswer::Named {
            song: heard_song(heard),
        },
        Ok(Recognition { refused, .. }) => match refused.into_iter().next() {
            None => HeardAnswer::Missed,
            Some(service) => HeardAnswer::Unreached {
                service: service_name(&service),
            },
        },
    })
}

fn service_name(service: &SourceId) -> String {
    let id = service.to_string();
    SERVICE_NAMES
        .iter()
        .find(|(named, _)| *named == id)
        .map_or(id.clone(), |(_, name)| (*name).to_owned())
}

fn heard_song(heard: Heard) -> HeardSong {
    let artist = heard.artist.unwrap_or_default();
    let key = format!(
        "{}{TITLE_APART_FROM_ARTIST}{}",
        folded_letters(&heard.title),
        folded_letters(&artist)
    );
    HeardSong {
        title: heard.title,
        artist,
        album: heard.album.unwrap_or_default(),
        year: heard.year,
        isrc: heard.isrc.map(|isrc| isrc.to_string()).unwrap_or_default(),
        recording: heard
            .recording
            .map(|mbid| mbid.to_string())
            .unwrap_or_default(),
        picture: heard.picture.map(|picture| picture.bytes),
        link: heard.link.unwrap_or_default(),
        by: service_name(&heard.by),
        key,
    }
}

#[cfg(test)]
mod tests {
    use resonate_listen::Recogniser;

    use super::*;

    struct Canned {
        service: SourceId,
        answer: Option<(&'static str, &'static str)>,
        fails: bool,
    }

    impl Recogniser for Canned {
        fn service(&self) -> &SourceId {
            &self.service
        }

        fn recognise(&self, _clip: &Clip) -> resonate_listen::Result<Option<Heard>> {
            if self.fails {
                return Err(ListenError::Unreachable {
                    service: self.service.clone(),
                });
            }
            Ok(self.answer.map(|(title, artist)| Heard {
                title: title.to_owned(),
                artist: Some(artist.to_owned()),
                album: None,
                year: Some(1971),
                isrc: None,
                recording: None,
                picture: None,
                link: None,
                by: self.service.clone(),
            }))
        }
    }

    #[test]
    fn a_clip_past_thirty_seconds_at_the_highest_rate_is_refused() {
        assert!(refuse_a_long_clip(CLIP_SAMPLES_AT_MOST).is_ok());
        assert!(matches!(
            refuse_a_long_clip(CLIP_SAMPLES_AT_MOST + 1),
            Err(Error::ClipTooLong { .. })
        ));
    }

    fn asking(answer: Option<(&'static str, &'static str)>, fails: bool) -> Recognisers {
        Recognisers::none().and(Arc::new(Canned {
            service: SourceId::new("shazam").expect("a source name"),
            answer,
            fails,
        }))
    }

    fn sounding() -> Clip {
        Clip {
            rate: SampleRate::HZ_48000,
            channels: HEARD_IN_MONO,
            samples: (0..9_600).map(|n| (n as f32 * 0.05).sin() * 0.3).collect(),
        }
    }

    fn silence() -> Clip {
        Clip {
            rate: SampleRate::HZ_48000,
            channels: HEARD_IN_MONO,
            samples: vec![0.0; 9_600],
        }
    }

    fn key_of(answer: HeardAnswer) -> String {
        match answer {
            HeardAnswer::Named { song } => song.key,
            other => panic!("nothing was named: {other:?}"),
        }
    }

    #[test]
    fn a_named_song_carries_a_key_its_other_spellings_share() {
        let named = answer_of(asking(Some(("Echoes", "Pink Floyd")), false).recognise(&sounding()))
            .expect("an answer");
        let shouted =
            answer_of(asking(Some(("ECHOES", "pink floyd")), false).recognise(&sounding()))
                .expect("an answer");
        let other = answer_of(asking(Some(("Dogs", "Pink Floyd")), false).recognise(&sounding()))
            .expect("an answer");

        let key = key_of(named);

        assert_eq!(key, key_of(shouted));
        assert_ne!(key, key_of(other));
    }

    #[test]
    fn a_named_song_keeps_what_the_service_said_and_leaves_the_rest_blank() {
        let answer =
            answer_of(asking(Some(("Echoes", "Pink Floyd")), false).recognise(&sounding()))
                .expect("an answer");

        let HeardAnswer::Named { song } = answer else {
            panic!("nothing was named");
        };

        assert_eq!(song.title, "Echoes");
        assert_eq!(song.artist, "Pink Floyd");
        assert_eq!(song.year, Some(1971));
        assert_eq!(song.by, "Shazam");
        assert!(song.album.is_empty() && song.link.is_empty() && song.picture.is_none());
    }

    #[test]
    fn a_silent_clip_a_miss_and_a_service_out_of_reach_are_told_apart() {
        let silent = answer_of(asking(Some(("Echoes", "Pink Floyd")), false).recognise(&silence()));
        let missed = answer_of(asking(None, false).recognise(&sounding()));
        let away = answer_of(asking(None, true).recognise(&sounding()));

        assert_eq!(silent.expect("an answer"), HeardAnswer::Silent);
        assert_eq!(missed.expect("an answer"), HeardAnswer::Missed);
        assert_eq!(
            away.expect("an answer"),
            HeardAnswer::Unreached {
                service: "Shazam".to_owned()
            }
        );
    }
}
