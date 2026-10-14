use std::{sync::Arc, time::Duration};

use resonate_core::eq::{Band, BandGain, Frequency, Preamp, Profile, Q};
use resonate_engine::{
    Command, Equalisation, FilterPhase, Quality, ReplayGainMode, Restoration, Until,
};

use crate::{Error, Result};

pub const EQUALISER_CENTRES: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum GainMode {
    #[default]
    Off,
    Track,
    Album,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum ResamplingQuality {
    Fast,
    Balanced,
    #[default]
    High,
    VeryHigh,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum ResamplingPhase {
    #[default]
    Linear,
    Intermediate,
    Minimum,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum LossyRestoration {
    #[default]
    Off,
    Repair,
    Extend,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PlaybackPreferences {
    pub equaliser_enabled: bool,
    pub preamp_db: f32,
    pub gains_db: Vec<f32>,
    pub replay_gain: GainMode,
    pub quality: ResamplingQuality,
    pub phase: ResamplingPhase,
    pub true_peak: bool,
    pub restoration: LossyRestoration,
    pub buffer_ms: u32,
    pub dsd_like_pcm: bool,
    pub profile_name: String,
    pub profile_text: String,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            equaliser_enabled: false,
            preamp_db: 0.0,
            gains_db: vec![0.0; EQUALISER_CENTRES.len()],
            replay_gain: GainMode::default(),
            quality: ResamplingQuality::default(),
            phase: ResamplingPhase::default(),
            true_peak: true,
            restoration: LossyRestoration::default(),
            buffer_ms: 500,
            dsd_like_pcm: false,
            profile_name: String::new(),
            profile_text: String::new(),
        }
    }
}

impl PlaybackPreferences {
    pub(crate) fn profile(&self) -> Result<Profile> {
        if self.gains_db.len() != EQUALISER_CENTRES.len() {
            return Err(Error::EqualiserBandCount {
                found: self.gains_db.len() as u64,
            });
        }
        let bands = EQUALISER_CENTRES
            .iter()
            .zip(&self.gains_db)
            .map(|(&frequency, &gain)| {
                Ok(Band::peaking(
                    Frequency::from_hertz(frequency)?,
                    BandGain::from_decibels(f64::from(gain))?,
                    Q::from_units(2.0_f64.sqrt())?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let profile = Profile::new(Preamp::from_decibels(f64::from(self.preamp_db))?, bands)?;
        if !self.profile_text.is_empty() {
            return Ok(resonate_eq::read_profile(&self.profile_text)?);
        }
        Ok(profile)
    }

    pub(crate) fn commands(&self, previous: &Self) -> Result<Vec<Command>> {
        let profile = self.profile()?;
        if !(50..=2000).contains(&self.buffer_ms) {
            return Err(Error::BufferRange {
                milliseconds: self.buffer_ms,
            });
        }
        let mut commands = Vec::new();
        if self.equaliser_enabled != previous.equaliser_enabled
            || self.preamp_db != previous.preamp_db
            || self.gains_db != previous.gains_db
            || self.profile_text != previous.profile_text
        {
            commands.push(Command::SetEqualisation(Arc::new(Equalisation {
                enabled: self.equaliser_enabled,
                fallback: Some(Arc::new(profile)),
                ..Equalisation::default()
            })));
        }
        if self.replay_gain != previous.replay_gain {
            commands.push(Command::SetReplayGain(match self.replay_gain {
                GainMode::Off => ReplayGainMode::Off,
                GainMode::Track => ReplayGainMode::Track,
                GainMode::Album => ReplayGainMode::Album,
            }));
        }
        if self.quality != previous.quality {
            commands.push(Command::SetQuality(match self.quality {
                ResamplingQuality::Fast => Quality::Fast,
                ResamplingQuality::Balanced => Quality::Balanced,
                ResamplingQuality::High => Quality::High,
                ResamplingQuality::VeryHigh => Quality::VeryHigh,
            }));
        }
        if self.phase != previous.phase {
            commands.push(Command::SetFilterPhase(match self.phase {
                ResamplingPhase::Linear => FilterPhase::Linear,
                ResamplingPhase::Intermediate => FilterPhase::Intermediate,
                ResamplingPhase::Minimum => FilterPhase::Minimum,
            }));
        }
        if self.true_peak != previous.true_peak {
            commands.push(Command::SetTruePeak(self.true_peak));
        }
        if self.restoration != previous.restoration {
            commands.push(Command::SetRestoration(match self.restoration {
                LossyRestoration::Off => Restoration::Off,
                LossyRestoration::Repair => Restoration::Repair,
                LossyRestoration::Extend => Restoration::Extend,
            }));
        }
        if self.buffer_ms != previous.buffer_ms {
            commands.push(Command::SetBuffer(Duration::from_millis(u64::from(
                self.buffer_ms,
            ))));
        }
        if self.dsd_like_pcm != previous.dsd_like_pcm {
            commands.push(Command::SetDsdLikePcm(self.dsd_like_pcm));
        }
        Ok(commands)
    }
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum SleepTimer {
    After { seconds: u32 },
    EndOfTrack,
    EndOfQueue,
}

impl From<SleepTimer> for Until {
    fn from(timer: SleepTimer) -> Self {
        match timer {
            SleepTimer::After { seconds } => Self::After(Duration::from_secs(u64::from(seconds))),
            SleepTimer::EndOfTrack => Self::EndOfTrack,
            SleepTimer::EndOfQueue => Self::EndOfQueue,
        }
    }
}

impl From<Until> for SleepTimer {
    fn from(until: Until) -> Self {
        match until {
            Until::After(delay) => Self::After {
                seconds: delay.as_secs().min(u64::from(u32::MAX)) as u32,
            },
            Until::EndOfTrack => Self::EndOfTrack,
            Until::EndOfQueue => Self::EndOfQueue,
        }
    }
}

#[uniffi::export]
pub fn default_playback_preferences() -> PlaybackPreferences {
    PlaybackPreferences::default()
}

#[uniffi::export]
pub fn equaliser_frequencies() -> Vec<f64> {
    EQUALISER_CENTRES.to_vec()
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;

    #[test]
    fn an_octave_band_shapes_the_shared_profile_and_only_sends_what_changed() {
        let previous = PlaybackPreferences::default();
        let mut preferences = previous.clone();
        preferences.equaliser_enabled = true;
        preferences.gains_db[5] = 6.0;
        preferences.preamp_db = -6.0;
        let profile = preferences.profile().expect("a valid ten-band curve");
        assert!((profile.magnitude_db(1000.0, SampleRate::HZ_48000)).abs() < 0.01);
        assert!(profile.magnitude_db(100.0, SampleRate::HZ_48000) < -5.0);
        let commands = preferences.commands(&previous).expect("a curve edit");
        assert!(matches!(commands.as_slice(), [Command::SetEqualisation(_)]));
        assert!(
            preferences
                .commands(&preferences)
                .expect("an unchanged curve")
                .is_empty()
        );
    }

    #[test]
    fn a_malformed_curve_is_refused_before_any_commands_are_sent() {
        let mut preferences = PlaybackPreferences {
            gains_db: vec![0.0; 9],
            ..PlaybackPreferences::default()
        };
        assert!(matches!(
            preferences.commands(&PlaybackPreferences::default()),
            Err(Error::EqualiserBandCount { found: 9 })
        ));
        preferences.gains_db = vec![0.0; 10];
        preferences.gains_db[0] = f32::NAN;
        assert!(
            preferences
                .commands(&PlaybackPreferences::default())
                .is_err()
        );
        preferences.gains_db[0] = 0.0;
        preferences.preamp_db = f32::INFINITY;
        assert!(
            preferences
                .commands(&PlaybackPreferences::default())
                .is_err()
        );
    }
}
