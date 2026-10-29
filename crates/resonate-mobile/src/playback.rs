use std::{sync::Arc, time::Duration};

use resonate_core::eq::Profile;
use resonate_engine::{
    Command, Crossfeed, Equalisation, FilterPhase, Quality, ReplayGainMode, Restoration, Until,
};

use crate::{Error, Result, equaliser::EqualiserCurve};

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
pub enum CrossfeedLevel {
    #[default]
    Off,
    Light,
    Moderate,
    Strong,
}

impl From<CrossfeedLevel> for Crossfeed {
    fn from(level: CrossfeedLevel) -> Self {
        match level {
            CrossfeedLevel::Off => Self::Off,
            CrossfeedLevel::Light => Self::Light,
            CrossfeedLevel::Moderate => Self::Moderate,
            CrossfeedLevel::Strong => Self::Strong,
        }
    }
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
    pub equaliser: EqualiserCurve,
    pub replay_gain: GainMode,
    pub quality: ResamplingQuality,
    pub phase: ResamplingPhase,
    pub true_peak: bool,
    pub restoration: LossyRestoration,
    pub buffer_ms: u32,
    pub dsd_like_pcm: bool,
    pub crossfeed: CrossfeedLevel,
}

impl Default for PlaybackPreferences {
    fn default() -> Self {
        Self {
            equaliser_enabled: false,
            equaliser: EqualiserCurve::default(),
            replay_gain: GainMode::default(),
            quality: ResamplingQuality::default(),
            phase: ResamplingPhase::default(),
            true_peak: true,
            restoration: LossyRestoration::default(),
            buffer_ms: 500,
            dsd_like_pcm: false,
            crossfeed: CrossfeedLevel::Off,
        }
    }
}

impl PlaybackPreferences {
    pub(crate) fn equalisation(&self, profile: Profile) -> Equalisation {
        Equalisation {
            enabled: self.equaliser_enabled,
            fallback: Some(Arc::new(profile)),
            ..Equalisation::default()
        }
    }

    pub(crate) fn commands(&self, previous: &Self) -> Result<Vec<Command>> {
        let profile = self.equaliser.profile()?;
        if !(50..=2000).contains(&self.buffer_ms) {
            return Err(Error::BufferRange {
                milliseconds: self.buffer_ms,
            });
        }
        let mut commands = Vec::new();
        if self.equaliser_enabled != previous.equaliser_enabled
            || self.equaliser.sounds_unlike(&previous.equaliser)
        {
            commands.push(Command::SetEqualisation(Arc::new(
                self.equalisation(profile),
            )));
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
        if self.crossfeed != previous.crossfeed {
            commands.push(Command::SetCrossfeed(self.crossfeed.into()));
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

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;
    use crate::equaliser::{EqualiserBand, EqualiserFilter, ten_band_curve};

    #[test]
    fn a_shaped_curve_reaches_the_engine_and_only_what_changed_is_sent() {
        let previous = PlaybackPreferences::default();
        let mut preferences = previous.clone();
        preferences.equaliser_enabled = true;
        preferences.equaliser = ten_band_curve(-6.0, vec![0.0, 0.0, 0.0, 0.0, 0.0, 6.0]);

        let profile = preferences.equaliser.profile().expect("a valid curve");
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

        let mut renamed = preferences.clone();
        renamed.equaliser.name = "Renamed".into();
        assert!(
            renamed
                .commands(&preferences)
                .expect("a renamed curve")
                .is_empty()
        );
    }

    #[test]
    fn a_crossfeed_level_is_sent_once_and_survives_a_round_trip_of_every_level() {
        let previous = PlaybackPreferences::default();
        assert_eq!(previous.crossfeed, CrossfeedLevel::Off);

        for (level, engine) in [
            (CrossfeedLevel::Light, Crossfeed::Light),
            (CrossfeedLevel::Moderate, Crossfeed::Moderate),
            (CrossfeedLevel::Strong, Crossfeed::Strong),
        ] {
            let mut preferences = previous.clone();
            preferences.crossfeed = level;

            let commands = preferences.commands(&previous).expect("a level");
            assert!(matches!(
                commands.as_slice(),
                [Command::SetCrossfeed(sent)] if *sent == engine
            ));
            assert!(preferences.commands(&preferences).expect("same").is_empty());
            assert_eq!(preferences.clone(), preferences);
        }

        let mut back = previous.clone();
        back.crossfeed = CrossfeedLevel::Strong;
        let commands = previous.commands(&back).expect("switched off");
        assert!(matches!(
            commands.as_slice(),
            [Command::SetCrossfeed(Crossfeed::Off)]
        ));
    }

    #[test]
    fn a_curve_of_any_shape_up_to_the_bound_is_sent_whole() {
        let previous = PlaybackPreferences::default();
        let mut preferences = previous.clone();
        preferences.equaliser.bands = (0..20)
            .map(|at| EqualiserBand {
                filter: EqualiserFilter::Peaking,
                frequency_hz: 40.0 * (at + 1) as f32,
                gain_db: 1.0,
                q: 2.0,
                channels: u8::MAX,
            })
            .collect();

        let commands = preferences.commands(&previous).expect("twenty bands");
        let [Command::SetEqualisation(equalisation)] = commands.as_slice() else {
            panic!("one equalisation command");
        };
        assert_eq!(
            equalisation
                .fallback
                .as_ref()
                .expect("one curve for every route")
                .bands()
                .len(),
            20
        );
    }

    #[test]
    fn a_malformed_curve_is_refused_before_any_commands_are_sent() {
        let mut preferences = PlaybackPreferences::default();
        preferences.equaliser.bands.push(EqualiserBand {
            filter: EqualiserFilter::Peaking,
            frequency_hz: f32::NAN,
            gain_db: 0.0,
            q: 1.0,
            channels: u8::MAX,
        });
        preferences.buffer_ms = 100;
        assert!(matches!(
            preferences.commands(&PlaybackPreferences::default()),
            Err(Error::BandFrequency { band: 0, .. })
        ));

        preferences.equaliser.bands.clear();
        preferences.equaliser.preamp_db = f32::INFINITY;
        assert!(matches!(
            preferences.commands(&PlaybackPreferences::default()),
            Err(Error::PreampRange { .. })
        ));
    }
}
