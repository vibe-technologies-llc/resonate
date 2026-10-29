use std::sync::Arc;

use resonate_core::{
    SampleRate,
    eq::{Preamp, Profile, RESPONSE_POINTS, sweep},
};
use resonate_dsp::Impulse;

use crate::{Error, Result};

pub(crate) type Heard = (f64, f64);

#[derive(Clone, Debug)]
pub struct Applied {
    profile: Arc<Profile>,
    correction: Option<Arc<Impulse>>,
}

impl From<Arc<Profile>> for Applied {
    fn from(profile: Arc<Profile>) -> Self {
        Self {
            profile,
            correction: None,
        }
    }
}

impl Applied {
    #[must_use]
    pub fn corrected_by(self, correction: Option<Arc<Impulse>>) -> Self {
        Self { correction, ..self }
    }

    pub const fn profile(&self) -> &Arc<Profile> {
        &self.profile
    }

    pub fn correction(&self) -> Option<&Arc<Impulse>> {
        self.correction.as_ref()
    }

    pub fn at_rate(&self, rate: SampleRate) -> Self {
        Self {
            profile: self.profile.at_rate(rate),
            correction: self.correction.clone(),
        }
    }

    pub fn heard_at(&self, hertz: &[f64], rate: SampleRate) -> Result<Vec<Vec<Heard>>> {
        let profile = self.profile.heard_at(hertz, rate);
        let Some(correction) = &self.correction else {
            return Ok(profile);
        };
        let corrected = correction.heard_at(hertz, rate).map_err(corrects)?;
        Ok(profile
            .into_iter()
            .zip(corrected)
            .map(|(shaping, correcting)| {
                shaping
                    .iter()
                    .flat_map(|shaping| {
                        correcting
                            .iter()
                            .map(|correcting| product(*shaping, *correcting))
                    })
                    .collect()
            })
            .collect())
    }

    pub fn impulse_responses(&self, frames: usize, rate: SampleRate) -> Result<Vec<Vec<f64>>> {
        let Some(correction) = &self.correction else {
            return Ok(self.profile.impulse_responses(frames, rate));
        };
        let taps = correction.at(rate).map_err(corrects)?;
        let mut by_correction: Vec<std::vec::IntoIter<Vec<f64>>> = taps
            .iter()
            .map(|channel| {
                let mut input = vec![0.0; frames];
                for (slot, tap) in input.iter_mut().zip(channel) {
                    *slot = *tap;
                }
                self.profile.filtered(&input, rate).into_iter()
            })
            .collect();
        let groups = by_correction.first().map_or(0, ExactSizeIterator::len);
        Ok((0..groups)
            .flat_map(|_| {
                by_correction
                    .iter_mut()
                    .filter_map(Iterator::next)
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub fn correction_frames(&self, rate: SampleRate) -> Result<usize> {
        let Some(correction) = &self.correction else {
            return Ok(0);
        };
        let taps = correction.at(rate).map_err(corrects)?;
        Ok(taps.iter().map(Vec::len).max().unwrap_or(0))
    }

    pub fn peak_db(&self, rate: SampleRate) -> Result<f64> {
        if self.correction.is_none() {
            return Ok(self.profile.peak_db(rate));
        }
        let centres = self
            .profile
            .bands()
            .iter()
            .filter(|band| band.applies_at(rate))
            .map(|band| band.frequency.hertz());
        let hertz: Vec<f64> = sweep(RESPONSE_POINTS).chain(centres).collect();
        let loudest = self
            .heard_at(&hertz, rate)?
            .iter()
            .flatten()
            .map(|(real, imaginary)| real.hypot(*imaginary))
            .fold(0.0, f64::max);
        Ok(20.0 * loudest.log10())
    }

    pub fn fitted_preamp(&self, rate: SampleRate) -> Result<Preamp> {
        self.peak_db(rate).map(Preamp::holding_under_full_scale)
    }
}

fn product(left: Heard, right: Heard) -> Heard {
    (
        left.0 * right.0 - left.1 * right.1,
        left.0 * right.1 + left.1 * right.0,
    )
}

fn corrects(source: resonate_dsp::Error) -> Error {
    Error::Correcting {
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Q};

    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;

    fn boosting(hertz: f64, decibels: f64) -> Arc<Profile> {
        let band = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(hertz).expect("in range"),
            BandGain::from_decibels(decibels).expect("in range"),
            Q::from_units(1.0).expect("in range"),
        );
        Arc::new(Profile::new(Preamp::NONE, vec![band]).expect("a profile in range"))
    }

    fn correcting_with(channels: Vec<Vec<f64>>) -> Option<Arc<Impulse>> {
        Some(Arc::new(Impulse::new(RATE, channels).expect("an impulse")))
    }

    #[test]
    fn a_profile_alone_is_heard_as_the_profile_is() {
        let profile = boosting(200.0, 6.0);
        let applied = Applied::from(Arc::clone(&profile));
        let hertz = [50.0, 200.0, 4_000.0];

        assert_eq!(
            applied.heard_at(&hertz, RATE).expect("a response"),
            profile.heard_at(&hertz, RATE)
        );
        assert_eq!(
            applied.impulse_responses(64, RATE).expect("an impulse"),
            profile.impulse_responses(64, RATE)
        );
        assert_eq!(
            applied.peak_db(RATE).expect("a peak"),
            profile.peak_db(RATE)
        );
    }

    #[test]
    fn the_response_of_a_correction_multiplies_into_the_response_of_the_profile() {
        let profile = boosting(200.0, 6.0);
        let taps = vec![0.5, 0.25, -0.125];
        let applied = Applied::from(Arc::clone(&profile))
            .corrected_by(correcting_with(vec![taps.clone(), vec![1.0]]));
        let hertz = [60.0, 200.0, 3_000.0];

        let heard = applied.heard_at(&hertz, RATE).expect("a response");
        let shaped = profile.heard_at(&hertz, RATE);

        for (at, hertz) in hertz.iter().enumerate() {
            assert_eq!(heard[at].len(), shaped[at].len() * 2);
            let angle = std::f64::consts::TAU * hertz / f64::from(RATE.hz());
            let corrected =
                taps.iter()
                    .enumerate()
                    .fold((0.0, 0.0), |(real, imaginary), (lag, tap)| {
                        (
                            real + tap * (angle * lag as f64).cos(),
                            imaginary + tap * (angle * lag as f64).sin(),
                        )
                    });
            let expected = product(shaped[at][0], corrected);
            assert!((heard[at][0].0 - expected.0).abs() < 1e-12);
            assert!((heard[at][0].1 - expected.1).abs() < 1e-12);
            assert_eq!(heard[at][1], shaped[at][0]);
        }
    }

    #[test]
    fn the_impulse_response_of_a_correction_convolves_into_the_ring_of_the_profile() {
        let profile = boosting(200.0, 6.0);
        let applied = Applied::from(Arc::clone(&profile))
            .corrected_by(correcting_with(vec![vec![1.0, 0.0, 0.5]]));
        let ring = profile.impulse_responses(200, RATE);

        let composed = applied.impulse_responses(200, RATE).expect("an impulse");

        assert_eq!(composed.len(), ring.len());
        for at in 0..200 {
            let expected = ring[0][at] + if at >= 2 { 0.5 * ring[0][at - 2] } else { 0.0 };
            assert!((composed[0][at] - expected).abs() < 1e-12, "frame {at}");
        }
    }

    #[test]
    fn a_correction_boosting_where_the_curve_does_deepens_the_modelled_fit() {
        let profile = boosting(100.0, 6.0);
        let alone = Applied::from(Arc::clone(&profile));
        let boosting_the_bass = alone
            .clone()
            .corrected_by(correcting_with(vec![vec![1.0, 1.0]]));
        let cutting_the_bass = alone
            .clone()
            .corrected_by(correcting_with(vec![vec![0.5, -0.5]]));

        let plain = alone.fitted_preamp(RATE).expect("a fit").decibels();
        let deeper = boosting_the_bass
            .fitted_preamp(RATE)
            .expect("a fit")
            .decibels();
        let shallower = cutting_the_bass
            .fitted_preamp(RATE)
            .expect("a fit")
            .decibels();

        assert!(deeper < plain - 4.0, "{deeper} against {plain}");
        assert!(shallower > plain + 4.0, "{shallower} against {plain}");
    }
}
