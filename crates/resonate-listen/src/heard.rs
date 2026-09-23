use std::{sync::Arc, time::Duration};

use resonate_core::{Isrc, Mbid, SampleRate, SourceId};

use crate::{Error, Result};

const SILENT_BELOW: f32 = 1e-4;

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub rate: SampleRate,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    pub fn length(&self) -> Duration {
        Duration::from_secs_f64(self.frames() as f64 / f64::from(self.rate.hz()))
    }

    pub fn mono(&self) -> Vec<f32> {
        let channels = usize::from(self.channels.max(1));
        self.samples
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    }

    pub fn is_silent(&self) -> bool {
        let squares: f64 = self
            .samples
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum();
        let rms = (squares / self.samples.len().max(1) as f64).sqrt();
        rms < f64::from(SILENT_BELOW)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PictureFormat {
    Jpeg,
    Png,
}

impl PictureFormat {
    const JPEG_STARTS: [u8; 3] = [0xff, 0xd8, 0xff];
    const PNG_STARTS: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    pub fn sniffed(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&Self::JPEG_STARTS) {
            Some(Self::Jpeg)
        } else if bytes.starts_with(&Self::PNG_STARTS) {
            Some(Self::Png)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub format: PictureFormat,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Heard {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub year: Option<u16>,
    pub isrc: Option<Isrc>,
    pub recording: Option<Mbid>,
    pub picture: Option<Picture>,
    pub link: Option<String>,
    pub by: SourceId,
}

pub trait Recogniser: Send + Sync {
    fn service(&self) -> &SourceId;

    fn recognise(&self, clip: &Clip) -> Result<Option<Heard>>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recognition {
    pub heard: Option<Heard>,
    pub refused: Vec<SourceId>,
}

#[derive(Clone, Default)]
pub struct Recognisers {
    held: Vec<Arc<dyn Recogniser>>,
}

impl Recognisers {
    pub fn none() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn and(mut self, recogniser: Arc<dyn Recogniser>) -> Self {
        self.held
            .retain(|held| held.service() != recogniser.service());
        self.held.push(recogniser);
        self
    }

    pub fn names(&self) -> Vec<SourceId> {
        self.held
            .iter()
            .map(|recogniser| recogniser.service().clone())
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    pub fn recognise(&self, clip: &Clip) -> Result<Recognition> {
        if clip.is_silent() {
            return Err(Error::NothingHeard);
        }
        let mut refused = Vec::new();
        for recogniser in &self.held {
            match recogniser.recognise(clip) {
                Ok(Some(heard)) => {
                    return Ok(Recognition {
                        heard: Some(heard),
                        refused,
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, service = %recogniser.service(), "a recogniser refused");
                    refused.push(recogniser.service().clone());
                }
            }
        }
        Ok(Recognition {
            heard: None,
            refused,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Canned {
        service: SourceId,
        answer: Option<&'static str>,
        fails: bool,
    }

    impl Recogniser for Canned {
        fn service(&self) -> &SourceId {
            &self.service
        }

        fn recognise(&self, _clip: &Clip) -> Result<Option<Heard>> {
            if self.fails {
                return Err(Error::Unreachable {
                    service: self.service.clone(),
                });
            }
            Ok(self.answer.map(|title| Heard {
                title: title.to_owned(),
                artist: None,
                album: None,
                year: None,
                isrc: None,
                recording: None,
                picture: None,
                link: None,
                by: self.service.clone(),
            }))
        }
    }

    fn canned(name: &str, answer: Option<&'static str>, fails: bool) -> Arc<dyn Recogniser> {
        Arc::new(Canned {
            service: SourceId::new(name).expect("a source name"),
            answer,
            fails,
        })
    }

    fn sounding() -> Clip {
        Clip {
            rate: SampleRate::HZ_48000,
            channels: 2,
            samples: (0..9_600).map(|n| (n as f32 * 0.05).sin() * 0.3).collect(),
        }
    }

    #[test]
    fn the_first_recogniser_that_knows_the_song_answers_and_a_refusal_is_counted() {
        let recognisers = Recognisers::none()
            .and(canned("down", None, true))
            .and(canned("blank", None, false))
            .and(canned("knows", Some("Tuesday"), false))
            .and(canned("later", Some("Wednesday"), false));
        let recognition = recognisers
            .recognise(&sounding())
            .expect("a clip that sounds");
        assert_eq!(
            recognition.heard.map(|heard| heard.title).as_deref(),
            Some("Tuesday")
        );
        assert_eq!(recognition.refused.len(), 1);
    }

    #[test]
    fn a_silent_clip_is_not_sent_anywhere() {
        let silent = Clip {
            samples: vec![0.0; 9_600],
            ..sounding()
        };
        assert!(matches!(
            Recognisers::none()
                .and(canned("knows", Some("Tuesday"), false))
                .recognise(&silent),
            Err(Error::NothingHeard)
        ));
    }

    #[test]
    fn a_second_recogniser_of_the_same_name_replaces_the_first() {
        let recognisers = Recognisers::none()
            .and(canned("knows", Some("Tuesday"), false))
            .and(canned("knows", Some("Wednesday"), false));
        assert_eq!(recognisers.names().len(), 1);
        let heard = recognisers
            .recognise(&sounding())
            .expect("a clip that sounds")
            .heard;
        assert_eq!(heard.map(|heard| heard.title).as_deref(), Some("Wednesday"));
    }
}
