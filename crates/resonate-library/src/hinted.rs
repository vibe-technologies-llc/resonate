use std::sync::Arc;

use resonate_codec::Hinting;
use resonate_core::{
    Decibels, FrameSpan, Gain, MeasuredGain, MediaLocation, TrackHints, eq::Frequency,
};

use crate::{db::Inner, studies};

const REPLAY_GAIN_REFERENCE_LUFS: f64 = -18.0;

pub(crate) struct Hinted {
    inner: Arc<Inner>,
}

impl Hinted {
    pub(crate) const fn over(inner: Arc<Inner>) -> Self {
        Self { inner }
    }
}

impl Hinting for Hinted {
    fn hints(&self, location: &MediaLocation, span: Option<FrameSpan>) -> TrackHints {
        let studied = self.inner.read(|connection| {
            let Some((track, studied)) = studies::study_of(connection, location, span)? else {
                return Ok(None);
            };
            let album = studies::album_loudness(connection, track)?;
            Ok(Some((studied, album)))
        });
        match studied {
            Ok(Some((studied, album))) => TrackHints {
                true_peak: Gain::new(studied.true_peak).ok(),
                lowpass: studied
                    .cutoff
                    .and_then(|cutoff| Frequency::from_hertz(f64::from(cutoff.hz)).ok()),
                measured: MeasuredGain {
                    track: studied
                        .loudness
                        .map(f64::from)
                        .and_then(gain_to_the_reference),
                    album: album.and_then(gain_to_the_reference),
                },
            },
            Ok(None) => TrackHints::default(),
            Err(error) => {
                tracing::debug!(%error, %location, "the catalog could not say what it studied of a row");
                TrackHints::default()
            }
        }
    }
}

fn gain_to_the_reference(loudness: f64) -> Option<Decibels> {
    Decibels::new((REPLAY_GAIN_REFERENCE_LUFS - loudness) as f32).ok()
}
