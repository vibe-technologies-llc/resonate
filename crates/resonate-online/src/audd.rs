use std::sync::Arc;

use resonate_core::{Isrc, Mbid, SourceId};
use resonate_library::LookupOp;
use resonate_listen::{Clip, Heard, Picture, PictureFormat, Recogniser};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    Client, Host,
    client::{LARGEST_PICTURE, Posted},
};

const AUDD: &str = "audd";
const ANSWERED: &str = "success";
const ASKED_FOR: &str = "apple_music,musicbrainz";
const LARGEST_CLIP: usize = 10 * 1024 * 1024;
const ARTWORK_SIDE: &str = "600";
const ARTWORK_WIDTH: &str = "{w}";
const ARTWORK_HEIGHT: &str = "{h}";
const YEAR_DIGITS: usize = 4;
const SIXTEEN_BIT_SCALE: f32 = 32_767.0;
const WAVE_HEADER_BYTES: u32 = 36;
const PCM: u16 = 1;
const MONO: u16 = 1;
const SIXTEEN_BITS: u16 = 16;
const PICTURES_SERVED_BY: &str = ".mzstatic.com";
const SECURE: &str = "https://";

#[derive(Deserialize)]
struct Answer {
    status: String,
    #[serde(default)]
    result: Option<Found>,
    #[serde(default)]
    error: Option<Refusal>,
}

#[derive(Deserialize)]
struct Refusal {
    error_code: u16,
}

#[derive(Deserialize)]
struct Found {
    title: String,
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    song_link: Option<String>,
    #[serde(default)]
    apple_music: Option<AppleMusic>,
    #[serde(default)]
    musicbrainz: Vec<Recorded>,
}

#[derive(Deserialize)]
struct AppleMusic {
    #[serde(default)]
    isrc: Option<String>,
    #[serde(default)]
    artwork: Option<Artwork>,
}

#[derive(Deserialize)]
struct Artwork {
    url: String,
}

#[derive(Deserialize)]
struct Recorded {
    id: String,
    #[serde(default)]
    isrcs: Vec<String>,
}

pub struct Audd {
    client: Arc<Client>,
    token: String,
    service: SourceId,
}

impl Audd {
    pub fn new(client: Arc<Client>, token: String) -> Self {
        Self {
            client,
            token,
            service: SourceId::new(AUDD).unwrap_or_else(|_| SourceId::local()),
        }
    }

    fn picture(&self, url: &str) -> Option<Picture> {
        let served = url
            .strip_prefix(SECURE)
            .and_then(|rest| rest.split('/').next())
            .is_some_and(|host| host.ends_with(PICTURES_SERVED_BY));
        if !served {
            return None;
        }
        let bytes = self
            .client
            .bytes(Host::AppleArtwork, LookupOp::Cover, url, LARGEST_PICTURE)
            .ok()??;
        Some(Picture {
            format: PictureFormat::sniffed(&bytes)?,
            bytes,
        })
    }
}

impl Recogniser for Audd {
    fn service(&self) -> &SourceId {
        &self.service
    }

    fn recognise(&self, clip: &Clip) -> resonate_listen::Result<Option<Heard>> {
        let wave = wave_of(clip);
        if wave.len() > LARGEST_CLIP {
            return Err(resonate_listen::Error::TooLarge {
                service: self.service.clone(),
            });
        }
        let boundary = format!("resonate-{}", Uuid::new_v4().simple());
        let body = form(&boundary, &self.token, &wave);
        let answer: Option<Answer> = self
            .client
            .posted(
                Host::Audd,
                LookupOp::Recognise,
                &format!("{}/", Host::Audd.base()),
                &Posted {
                    content_type: format!("multipart/form-data; boundary={boundary}"),
                    bytes: body,
                },
            )
            .map_err(|error| error.into_listen_error(self.service.clone()))?;

        let (mut heard, artwork) = match heard_in(answer, &self.service) {
            Ok(Some(found)) => found,
            Ok(None) => return Ok(None),
            Err(status) => {
                return Err(resonate_listen::Error::Refused {
                    service: self.service.clone(),
                    status,
                });
            }
        };
        heard.picture = artwork.and_then(|url| self.picture(&url));
        Ok(Some(heard))
    }
}

fn heard_in(
    answer: Option<Answer>,
    service: &SourceId,
) -> Result<Option<(Heard, Option<String>)>, u16> {
    let Some(answer) = answer else {
        return Ok(None);
    };
    if answer.status != ANSWERED {
        return Err(answer.error.map_or(0, |refusal| refusal.error_code));
    }
    let Some(found) = answer.result else {
        return Ok(None);
    };
    let recorded = found.musicbrainz.first();
    let isrc = found
        .apple_music
        .as_ref()
        .and_then(|apple| apple.isrc.clone())
        .or_else(|| recorded.and_then(|recorded| recorded.isrcs.first().cloned()));
    let artwork = found
        .apple_music
        .as_ref()
        .and_then(|apple| apple.artwork.as_ref())
        .map(|artwork| {
            artwork
                .url
                .replace(ARTWORK_WIDTH, ARTWORK_SIDE)
                .replace(ARTWORK_HEIGHT, ARTWORK_SIDE)
        });
    let heard = Heard {
        title: found.title,
        artist: found.artist.filter(|artist| !artist.is_empty()),
        album: found.album.filter(|album| !album.is_empty()),
        year: found
            .release_date
            .and_then(|date| date.get(..YEAR_DIGITS).and_then(|year| year.parse().ok())),
        isrc: isrc.as_deref().and_then(|isrc| Isrc::new(isrc).ok()),
        recording: recorded.and_then(|recorded| Mbid::new(&recorded.id).ok()),
        picture: None,
        link: found.song_link,
        by: service.clone(),
    };
    Ok(Some((heard, artwork)))
}

fn wave_of(clip: &Clip) -> Vec<u8> {
    let mono = clip.mono();
    let data = (mono.len() * 2) as u32;
    let rate = clip.rate.hz();
    let mut wave = Vec::with_capacity(data as usize + 44);
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(WAVE_HEADER_BYTES + data).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16_u32.to_le_bytes());
    wave.extend_from_slice(&PCM.to_le_bytes());
    wave.extend_from_slice(&MONO.to_le_bytes());
    wave.extend_from_slice(&rate.to_le_bytes());
    wave.extend_from_slice(&(rate * u32::from(SIXTEEN_BITS / 8)).to_le_bytes());
    wave.extend_from_slice(&(SIXTEEN_BITS / 8).to_le_bytes());
    wave.extend_from_slice(&SIXTEEN_BITS.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data.to_le_bytes());
    for sample in mono {
        let word = (sample * SIXTEEN_BIT_SCALE)
            .round()
            .clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16;
        wave.extend_from_slice(&word.to_le_bytes());
    }
    wave
}

fn form(boundary: &str, token: &str, wave: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(wave.len() + 512);
    for (name, value) in [("api_token", token), ("return", ASKED_FOR)] {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wave);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;

    const RECOGNISED: &str = include_str!("../tests/fixtures/audd_recognised.json");
    const NOTHING: &str = include_str!("../tests/fixtures/audd_nothing.json");
    const REFUSED: &str = include_str!("../tests/fixtures/audd_refused.json");

    fn service() -> SourceId {
        SourceId::new(AUDD).expect("a service name")
    }

    fn answered(text: &str) -> Result<Option<(Heard, Option<String>)>, u16> {
        heard_in(
            Some(serde_json::from_str(text).expect("an audd answer")),
            &service(),
        )
    }

    #[test]
    fn a_recognition_names_the_song_its_recording_and_where_its_artwork_is() {
        let (heard, artwork) = answered(RECOGNISED).expect("an answer").expect("a song");
        assert_eq!(heard.title, "Warriors");
        assert_eq!(heard.artist.as_deref(), Some("Imagine Dragons"));
        assert_eq!(heard.album.as_deref(), Some("Warriors"));
        assert_eq!(heard.year, Some(2014));
        assert_eq!(
            heard.isrc,
            Some(Isrc::new("USUM71414287").expect("an isrc"))
        );
        assert!(heard.recording.is_some());
        assert_eq!(heard.link.as_deref(), Some("https://lis.tn/Warriors"));
        let artwork = artwork.expect("artwork");
        assert!(artwork.ends_with("600x600bb.jpg"), "{artwork}");
    }

    #[test]
    fn nothing_found_is_no_song_and_a_refusal_is_its_code() {
        assert!(matches!(answered(NOTHING), Ok(None)));
        assert_eq!(answered(REFUSED).err(), Some(900));
    }

    #[test]
    fn the_clip_is_sent_as_a_mono_sixteen_bit_wave_inside_one_form() {
        let clip = Clip {
            rate: SampleRate::HZ_48000,
            channels: 2,
            samples: vec![0.5, -0.5, 1.0, 1.0],
        };
        let wave = wave_of(&clip);
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(wave.len(), 44 + 4);
        assert_eq!(i16::from_le_bytes([wave[44], wave[45]]), 0);
        assert_eq!(i16::from_le_bytes([wave[46], wave[47]]), i16::MAX);

        let body = form("edge", "token", &wave);
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with("--edge\r\nContent-Disposition: form-data; name=\"api_token\""));
        assert!(text.contains("name=\"return\"\r\n\r\napple_music,musicbrainz\r\n"));
        assert!(text.ends_with("\r\n--edge--\r\n"));
    }
}
