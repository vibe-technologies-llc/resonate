use std::{f64::consts::SQRT_2, sync::Arc};

use resonate_analysis::Watch;
use resonate_core::{
    SampleRate,
    eq::{
        Band, BandGain, BandKind, ChannelSet, FITTED_AT, Frequency, MAX_BANDS, Preamp, Profile, Q,
    },
};

use crate::{Error, MobilePlayer, Result};

pub const EQUALISER_CENTRES: [f64; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];

pub const LOWEST_HZ: f32 = 20.0;
pub const HIGHEST_HZ: f32 = 20_000.0;
pub const GAIN_AT_MOST_DB: f32 = 24.0;
pub const PREAMP_AT_MOST_DB: f32 = 24.0;
pub const Q_AT_LEAST: f32 = 0.1;
pub const Q_AT_MOST: f32 = 40.0;

const OCTAVE_Q: f64 = SQRT_2;
const OCTAVE_Q_HELD_WITHIN: f32 = 0.001;
const RESPONSE_POINTS_AT_LEAST: u32 = 2;
const RESPONSE_POINTS_AT_MOST: u32 = 2_048;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, uniffi::Enum)]
pub enum EqualiserFilter {
    #[default]
    Peaking,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
    Notch,
    BandPass,
    AllPass,
}

impl From<EqualiserFilter> for BandKind {
    fn from(filter: EqualiserFilter) -> Self {
        match filter {
            EqualiserFilter::Peaking => Self::Peaking,
            EqualiserFilter::LowShelf => Self::LowShelf,
            EqualiserFilter::HighShelf => Self::HighShelf,
            EqualiserFilter::LowPass => Self::LowPass,
            EqualiserFilter::HighPass => Self::HighPass,
            EqualiserFilter::Notch => Self::Notch,
            EqualiserFilter::BandPass => Self::BandPass,
            EqualiserFilter::AllPass => Self::AllPass,
        }
    }
}

impl From<BandKind> for EqualiserFilter {
    fn from(kind: BandKind) -> Self {
        match kind {
            BandKind::Peaking => Self::Peaking,
            BandKind::LowShelf => Self::LowShelf,
            BandKind::HighShelf => Self::HighShelf,
            BandKind::LowPass => Self::LowPass,
            BandKind::HighPass => Self::HighPass,
            BandKind::Notch => Self::Notch,
            BandKind::BandPass => Self::BandPass,
            BandKind::AllPass => Self::AllPass,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct EqualiserBand {
    pub filter: EqualiserFilter,
    pub frequency_hz: f32,
    pub gain_db: f32,
    pub q: f32,
    #[uniffi(default = 255)]
    pub channels: u8,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct EqualiserCurve {
    pub name: String,
    pub preamp_db: f32,
    pub bands: Vec<EqualiserBand>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EqualiserPreset {
    pub name: String,
    pub detail: String,
    pub curve: EqualiserCurve,
}

type OctaveGains = [f32; EQUALISER_CENTRES.len()];

struct Preset {
    name: &'static str,
    detail: &'static str,
    preamp_db: f32,
    octaves: Option<OctaveGains>,
}

const PRESETS: [Preset; 10] = [
    Preset {
        name: "Flat",
        detail: "An uncoloured starting point.",
        preamp_db: 0.0,
        octaves: None,
    },
    Preset {
        name: "Bass boost",
        detail: "More weight and punch in the low end.",
        preamp_db: -6.0,
        octaves: Some([4.0, 4.0, 2.0, 0.0, -1.0, -1.0, 0.0, 0.0, 0.0, 0.0]),
    },
    Preset {
        name: "Bass only",
        detail: "More bass. Mids, treble and output gain stay unchanged.",
        preamp_db: 0.0,
        octaves: Some([4.0, 4.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    },
    Preset {
        name: "Bass cut",
        detail: "Less rumble, with the rest left clear.",
        preamp_db: 0.0,
        octaves: Some([-5.0, -4.0, -3.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    },
    Preset {
        name: "Vocals",
        detail: "Bring voices and lead instruments forward.",
        preamp_db: -5.0,
        octaves: Some([-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 3.0, 1.0, 0.0, -1.0]),
    },
    Preset {
        name: "Warm",
        detail: "Fuller lows and a softer top end.",
        preamp_db: -5.0,
        octaves: Some([2.0, 3.0, 2.0, 1.0, 0.0, 0.0, -1.0, -2.0, -2.0, -2.0]),
    },
    Preset {
        name: "Bright",
        detail: "A little more sparkle and detail.",
        preamp_db: -5.0,
        octaves: Some([-1.0, -1.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 2.0]),
    },
    Preset {
        name: "Treble cut",
        detail: "Softer highs for harsh recordings, with the rest left clear.",
        preamp_db: 0.0,
        octaves: Some([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -3.0, -4.0, -5.0]),
    },
    Preset {
        name: "Rock",
        detail: "Punchy lows, crisp guitars and drums.",
        preamp_db: -5.0,
        octaves: Some([3.0, 2.0, 0.0, -1.0, -2.0, 0.0, 2.0, 3.0, 3.0, 2.0]),
    },
    Preset {
        name: "Electronic",
        detail: "Deep bass with a lively, open top end.",
        preamp_db: -6.0,
        octaves: Some([4.0, 3.0, 1.0, 0.0, -2.0, -1.0, 1.0, 2.0, 3.0, 3.0]),
    },
];

impl Preset {
    fn offered(&self) -> EqualiserPreset {
        let curve = self
            .octaves
            .map_or_else(EqualiserCurve::default, |gains| EqualiserCurve {
                name: self.name.to_owned(),
                ..EqualiserCurve::ten_bands(self.preamp_db, &gains)
            });
        EqualiserPreset {
            name: self.name.to_owned(),
            detail: self.detail.to_owned(),
            curve,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EqualiserImport {
    pub curve: EqualiserCurve,
    pub passed_over: u32,
    pub approximated: u32,
    pub clamped: u32,
    pub converted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct EqualiserLimits {
    pub bands_at_most: u32,
    pub lowest_hz: f32,
    pub highest_hz: f32,
    pub gain_at_most_db: f32,
    pub preamp_at_most_db: f32,
    pub q_at_least: f32,
    pub q_at_most: f32,
}

impl EqualiserBand {
    fn band(self, at: usize) -> Result<Band> {
        let place = u32::try_from(at).unwrap_or(u32::MAX);
        if !(LOWEST_HZ..=HIGHEST_HZ).contains(&self.frequency_hz) {
            return Err(Error::BandFrequency {
                band: place,
                hertz: self.frequency_hz,
            });
        }
        if !(-GAIN_AT_MOST_DB..=GAIN_AT_MOST_DB).contains(&self.gain_db) {
            return Err(Error::BandGain {
                band: place,
                decibels: self.gain_db,
            });
        }
        if !(Q_AT_LEAST..=Q_AT_MOST).contains(&self.q) {
            return Err(Error::BandQ {
                band: place,
                q: self.q,
            });
        }

        let channels =
            ChannelSet::from_bits(self.channels).ok_or(Error::BandChannels { band: place })?;

        Ok(Band {
            channels,
            ..Band::new(
                self.filter.into(),
                Frequency::from_hertz(f64::from(self.frequency_hz))?,
                BandGain::from_decibels(f64::from(self.gain_db))?,
                Q::from_units(f64::from(self.q))?,
            )
        })
    }

    fn of(band: &Band) -> Self {
        Self {
            filter: band.kind.into(),
            frequency_hz: band.frequency.hertz() as f32,
            gain_db: band.gain.decibels() as f32,
            q: band.q.units() as f32,
            channels: band.channels.bits(),
        }
    }

    fn held_within_limits(self) -> (Self, bool) {
        let held = Self {
            filter: self.filter,
            frequency_hz: self.frequency_hz.clamp(LOWEST_HZ, HIGHEST_HZ),
            gain_db: self.gain_db.clamp(-GAIN_AT_MOST_DB, GAIN_AT_MOST_DB),
            q: self.q.clamp(Q_AT_LEAST, Q_AT_MOST),
            channels: self.channels,
        };
        (held, held != self)
    }
}

impl EqualiserCurve {
    pub(crate) fn profile(&self) -> Result<Profile> {
        if self.bands.len() > MAX_BANDS {
            return Err(Error::TooManyBands {
                found: u32::try_from(self.bands.len()).unwrap_or(u32::MAX),
            });
        }
        if !(-PREAMP_AT_MOST_DB..=PREAMP_AT_MOST_DB).contains(&self.preamp_db) {
            return Err(Error::PreampRange {
                decibels: self.preamp_db,
            });
        }

        let bands = self
            .bands
            .iter()
            .enumerate()
            .map(|(at, band)| band.band(at))
            .collect::<Result<Vec<_>>>()?;
        Ok(Profile::new(
            Preamp::from_decibels(f64::from(self.preamp_db))?,
            bands,
        )?)
    }

    pub(crate) fn sounds_unlike(&self, other: &Self) -> bool {
        self.preamp_db != other.preamp_db || self.bands != other.bands
    }

    fn ten_bands(preamp_db: f32, gains_db: &[f32]) -> Self {
        let level = |decibels: f32, most: f32| {
            if decibels.is_finite() {
                decibels.clamp(-most, most)
            } else {
                0.0
            }
        };
        Self {
            name: String::new(),
            preamp_db: level(preamp_db, PREAMP_AT_MOST_DB),
            bands: EQUALISER_CENTRES
                .iter()
                .enumerate()
                .map(|(at, &centre)| EqualiserBand {
                    filter: EqualiserFilter::Peaking,
                    frequency_hz: centre as f32,
                    gain_db: level(
                        gains_db.get(at).copied().unwrap_or_default(),
                        GAIN_AT_MOST_DB,
                    ),
                    q: OCTAVE_Q as f32,
                    channels: ChannelSet::EVERY.bits(),
                })
                .collect(),
        }
    }

    fn octave_gains(&self) -> Option<Vec<f32>> {
        if self.bands.len() != EQUALISER_CENTRES.len() {
            return None;
        }
        self.bands
            .iter()
            .zip(EQUALISER_CENTRES)
            .map(|(band, centre)| {
                let on_the_octave = band.filter == EqualiserFilter::Peaking
                    && band.frequency_hz == centre as f32
                    && (band.q - OCTAVE_Q as f32).abs() < OCTAVE_Q_HELD_WITHIN;
                on_the_octave.then_some(band.gain_db)
            })
            .collect()
    }
}

pub(crate) fn imported(profile: &Profile, name: String) -> EqualiserImport {
    let mut clamped = 0;

    let bands = profile
        .bands()
        .iter()
        .filter(|band| band.on)
        .map(|band| {
            let (held, was_held) = EqualiserBand::of(band).held_within_limits();
            clamped += u32::from(was_held);
            held
        })
        .collect();

    let preamp = profile.preamp().decibels() as f32;
    let preamp_db = preamp.clamp(-PREAMP_AT_MOST_DB, PREAMP_AT_MOST_DB);
    clamped += u32::from(preamp_db != preamp);

    EqualiserImport {
        curve: EqualiserCurve {
            name,
            preamp_db,
            bands,
        },
        passed_over: 0,
        approximated: 0,
        clamped,
        converted: profile.target().is_some(),
    }
}

fn counted(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[uniffi::export]
pub fn equaliser_limits() -> EqualiserLimits {
    EqualiserLimits {
        bands_at_most: counted(MAX_BANDS),
        lowest_hz: LOWEST_HZ,
        highest_hz: HIGHEST_HZ,
        gain_at_most_db: GAIN_AT_MOST_DB,
        preamp_at_most_db: PREAMP_AT_MOST_DB,
        q_at_least: Q_AT_LEAST,
        q_at_most: Q_AT_MOST,
    }
}

#[uniffi::export]
pub fn equaliser_frequencies() -> Vec<f64> {
    EQUALISER_CENTRES.to_vec()
}

#[uniffi::export]
pub fn equaliser_presets() -> Vec<EqualiserPreset> {
    PRESETS.iter().map(Preset::offered).collect()
}

#[uniffi::export]
pub fn ten_band_curve(preamp_db: f32, gains_db: Vec<f32>) -> EqualiserCurve {
    EqualiserCurve::ten_bands(preamp_db, &gains_db)
}

#[uniffi::export]
pub fn ten_band_gains(curve: EqualiserCurve) -> Option<Vec<f32>> {
    curve.octave_gains()
}

#[uniffi::export]
pub fn response_curve(
    curve: EqualiserCurve,
    points: u32,
    rate_hz: Option<u32>,
) -> Result<Vec<f32>> {
    let profile = curve.profile()?;
    let rate = rate_hz.map_or(Ok(FITTED_AT), SampleRate::new)?;
    let points = points.clamp(RESPONSE_POINTS_AT_LEAST, RESPONSE_POINTS_AT_MOST);

    Ok(profile
        .response(rate, points as usize)
        .into_iter()
        .map(|decibels| decibels as f32)
        .collect())
}

#[uniffi::export]
pub fn fitted_preamp(curve: EqualiserCurve, rate_hz: Option<u32>) -> Result<f32> {
    let profile = curve.profile()?;
    let rate = rate_hz.map_or(Ok(FITTED_AT), SampleRate::new)?;
    let fitted = profile.fitted_preamp(rate).decibels() as f32;

    Ok(fitted.max(-PREAMP_AT_MOST_DB))
}

#[uniffi::export]
impl MobilePlayer {
    pub fn fitted_preamp_to_the_queue(&self, curve: EqualiserCurve) -> Result<Option<f32>> {
        let profile = Arc::new(curve.profile()?);
        let (rows, playing) = {
            let held = self.player.lock();
            let player = held.as_ref().ok_or(Error::Closed)?;
            (player.queue(), player.state().queue_position)
        };

        let watch = Arc::new(Watch::default());
        if let Some(previous) = self.services.fitting.lock().replace(Arc::clone(&watch)) {
            previous.stop();
        }
        let cuts = resonate_engine::cuts_to_fit(&rows, playing);
        let loudest =
            resonate_analysis::loudest_through(&self.sources, &cuts, &profile, watch.as_ref())?;

        Ok(loudest
            .fitted_preamp()
            .map(|fitted| (fitted.decibels() as f32).max(-PREAMP_AT_MOST_DB)))
    }
}

#[uniffi::export]
pub fn equaliser_text(curve: EqualiserCurve) -> Result<String> {
    Ok(resonate_eq::write_profile(&curve.profile()?))
}

#[uniffi::export]
pub fn parse_eq_profile(bytes: Vec<u8>, name: String) -> Result<EqualiserImport> {
    let reading = resonate_eq::reading(&resonate_core::text::decoded(&bytes).0)?;
    let mut import = imported(&reading.profile, name);

    import.passed_over += counted(reading.passed_over);
    import.approximated += counted(reading.approximated);
    import.clamped += counted(reading.clamped);
    Ok(import)
}

#[cfg(test)]
mod tests {
    use resonate_core::eq::sweep;

    use super::*;

    const AUTOEQ_PARAMETRIC: &str =
        include_str!("../../resonate-eq/tests/fixtures/autoeq_parametric.txt");
    const AUTOEQ_GRAPHIC: &str =
        include_str!("../../resonate-eq/tests/fixtures/autoeq_graphic.txt");

    fn peaking(frequency_hz: f32, gain_db: f32, q: f32) -> EqualiserBand {
        EqualiserBand {
            filter: EqualiserFilter::Peaking,
            frequency_hz,
            gain_db,
            q,
            channels: ChannelSet::EVERY.bits(),
        }
    }

    fn curve(bands: Vec<EqualiserBand>) -> EqualiserCurve {
        EqualiserCurve {
            name: String::new(),
            preamp_db: 0.0,
            bands,
        }
    }

    #[test]
    fn the_drawn_response_of_one_band_is_the_biquad_the_stage_runs() {
        let points = 256;
        let centre = sweep(points).nth(points / 2).expect("a middle point") as f32;
        let shaped = curve(vec![peaking(centre, 6.0, 1.0)]);

        let drawn = response_curve(shaped.clone(), points as u32, None).expect("a valid curve");
        let designed = shaped.profile().expect("a valid curve").bands()[0].design(FITTED_AT);

        assert_eq!(drawn.len(), points);
        assert!(
            (drawn[points / 2] - 6.0).abs() < 0.05,
            "{} dB at the centre",
            drawn[points / 2]
        );
        for (hertz, decibels) in sweep(points).zip(&drawn) {
            let expected = designed.magnitude_db(hertz, FITTED_AT) as f32;
            assert!(
                (decibels - expected).abs() < 0.001,
                "{decibels} against {expected} dB at {hertz} Hz"
            );
        }
    }

    #[test]
    fn the_response_is_drawn_at_the_rate_asked_and_with_the_preamp() {
        let mut shaped = curve(vec![peaking(15_000.0, 6.0, 1.0)]);
        shaped.preamp_db = -3.0;

        let at_48 = response_curve(shaped.clone(), 64, None).expect("a valid curve");
        let at_44 = response_curve(shaped.clone(), 64, Some(44_100)).expect("a valid curve");

        assert!((at_48[0] + 3.0).abs() < 0.05);
        assert_ne!(at_48, at_44);
        assert_eq!(
            response_curve(shaped.clone(), 0, None)
                .expect("held to two points")
                .len(),
            2
        );
        assert!(matches!(
            response_curve(shaped, 64, Some(7)),
            Err(Error::Domain(_))
        ));
    }

    #[test]
    fn the_fitted_preamp_holds_the_loudest_band_under_full_scale_as_the_desktop_does() {
        let boosted = EqualiserCurve {
            name: String::new(),
            preamp_db: 3.0,
            bands: vec![peaking(1000.0, 6.0, 1.0), peaking(8000.0, -4.0, 2.0)],
        };
        let narrow = curve(vec![peaking(5_000.0, 12.0, 40.0)]);
        let cut = curve(vec![peaking(1000.0, -6.0, 1.0)]);
        let towering = curve(vec![peaking(1000.0, 24.0, 0.5), peaking(1100.0, 24.0, 0.5)]);

        let fitted = fitted_preamp(boosted.clone(), None).expect("a valid curve");
        let at_96 = fitted_preamp(boosted.clone(), Some(96_000)).expect("a valid curve");
        let desktop = boosted
            .profile()
            .expect("a valid curve")
            .fitted_preamp(SampleRate::new(96_000).expect("a rate"));

        assert!((fitted + 6.0).abs() < 0.05, "fitted at {fitted} dB");
        assert_eq!(at_96, desktop.decibels() as f32);
        assert!(
            (fitted_preamp(narrow, None).expect("a valid curve") + 12.0).abs() < 0.05,
            "a narrow peak between the drawn points is still weighed"
        );
        assert_eq!(fitted_preamp(cut, None).expect("a valid curve"), 0.0);
        assert_eq!(
            fitted_preamp(towering, None).expect("a valid curve"),
            -PREAMP_AT_MOST_DB
        );
        assert_eq!(
            fitted_preamp(EqualiserCurve::default(), None).expect("a flat curve"),
            0.0
        );
        assert!(matches!(
            fitted_preamp(boosted, Some(7)),
            Err(Error::Domain(_))
        ));
    }

    #[test]
    fn a_queue_of_silence_fits_nothing_and_an_empty_one_or_a_bad_curve_is_told_apart() {
        use crate::harness::Harness;

        let harness = Harness::new();
        let boosted = curve(vec![peaking(1000.0, 6.0, 1.0)]);

        assert_eq!(
            harness
                .player
                .fitted_preamp_to_the_queue(boosted.clone())
                .expect("an empty queue is measured"),
            None
        );

        harness
            .player
            .load(harness.ids("ab"), 0, false)
            .expect("a queue");
        assert_eq!(
            harness
                .player
                .fitted_preamp_to_the_queue(boosted.clone())
                .expect("a queue of silence is measured"),
            None
        );

        let broken = curve(vec![peaking(1000.0, 60.0, 1.0)]);
        assert!(matches!(
            harness.player.fitted_preamp_to_the_queue(broken),
            Err(Error::BandGain { .. })
        ));
    }

    #[test]
    fn an_autoeq_profile_loads_every_filter_as_an_editable_band() {
        let import = parse_eq_profile(AUTOEQ_PARAMETRIC.as_bytes().to_vec(), "HD 650".into())
            .expect("a well formed profile");
        let profile = resonate_eq::read_profile(AUTOEQ_PARAMETRIC).expect("a well formed profile");
        let fetched = imported(&profile, "HD 650".into());

        assert_eq!(import, fetched);
        assert_eq!(import.curve.name, "HD 650");
        assert_eq!(import.curve.bands.len(), 10);
        assert_eq!(import.curve.preamp_db, -6.1);
        assert_eq!(import.passed_over + import.clamped + import.approximated, 0);
        assert!(!import.converted);
        assert_eq!(import.curve.bands[0].filter, EqualiserFilter::LowShelf);
        assert_eq!(import.curve.bands[0].frequency_hz, 105.0);
        assert_eq!(import.curve.bands[5].filter, EqualiserFilter::HighShelf);
        assert_eq!(import.curve.bands[9], peaking(5332.0, -1.1, 5.75));
        assert_eq!(
            import.curve.profile().expect("a valid curve"),
            profile,
            "the editable bands sound as the profile does"
        );
    }

    #[test]
    fn a_graphic_curve_is_fitted_onto_bands_and_says_it_was_converted() {
        let import = parse_eq_profile(AUTOEQ_GRAPHIC.as_bytes().to_vec(), "Graphic".into())
            .expect("a well formed profile");

        assert!(import.converted);
        assert!(!import.curve.bands.is_empty());
        assert!(import.curve.bands.len() <= MAX_BANDS);
        assert!(import.curve.profile().is_ok());
    }

    #[test]
    fn values_past_what_a_phone_holds_are_held_to_it_and_counted() {
        let text = "Preamp: -30 dB\n\
                    Filter 1: ON PK Fc 10 Hz Gain 3 dB Q 1\n\
                    Filter 2: ON PK Fc 1000 Hz Gain 30 dB Q 1\n\
                    Filter 3: OFF PK Fc 2000 Hz Gain 3 dB Q 1\n\
                    Channel: L\n\
                    Filter 4: ON PK Fc 3000 Hz Gain 3 dB Q 1\n";

        let import = parse_eq_profile(text.as_bytes().to_vec(), String::new()).expect("a profile");

        assert_eq!(import.curve.bands.len(), 3);
        assert_eq!(import.curve.bands[0].frequency_hz, LOWEST_HZ);
        assert_eq!(import.curve.bands[1].gain_db, GAIN_AT_MOST_DB);
        assert_eq!(import.curve.preamp_db, -PREAMP_AT_MOST_DB);
        assert_eq!(import.clamped, 3);
        assert_eq!(import.passed_over, 0);
        assert_eq!(import.curve.bands[2].channels, 0b1);
        assert!(import.curve.profile().is_ok());
    }

    #[test]
    fn a_band_limited_to_some_channels_is_kept_on_them_and_played_on_them_alone() {
        let text = "Channel: R\nFilter 1: ON PK Fc 1000 Hz Gain 4 dB Q 1\n";

        let import = parse_eq_profile(text.as_bytes().to_vec(), String::new()).expect("a profile");
        let profile = import.curve.profile().expect("a valid curve");
        let kept = equaliser_text(import.curve.clone()).expect("text");
        let again = parse_eq_profile(kept.into_bytes(), String::new()).expect("a profile");

        assert_eq!(import.passed_over, 0);
        assert_eq!(import.curve.bands[0].channels, 0b10);
        assert_eq!(profile.channels_apart(), vec![1]);
        assert_eq!(again.curve.bands, import.curve.bands);
    }

    #[test]
    fn a_band_played_on_no_channel_is_refused_naming_it() {
        let mut silent = peaking(1000.0, 0.0, 1.0);
        silent.channels = 0;

        assert!(matches!(
            curve(vec![silent]).profile(),
            Err(Error::BandChannels { band: 0 })
        ));
    }

    #[test]
    fn a_curve_past_its_bounds_is_refused_naming_what_is_wrong() {
        let refused = |band: EqualiserBand| curve(vec![peaking(1000.0, 0.0, 1.0), band]).profile();

        assert!(matches!(
            refused(peaking(19.0, 0.0, 1.0)),
            Err(Error::BandFrequency { band: 1, .. })
        ));
        assert!(matches!(
            refused(peaking(20_001.0, 0.0, 1.0)),
            Err(Error::BandFrequency { .. })
        ));
        assert!(matches!(
            refused(peaking(f32::NAN, 0.0, 1.0)),
            Err(Error::BandFrequency { .. })
        ));
        assert!(matches!(
            refused(peaking(1000.0, 24.5, 1.0)),
            Err(Error::BandGain { band: 1, .. })
        ));
        assert!(matches!(
            refused(peaking(1000.0, f32::INFINITY, 1.0)),
            Err(Error::BandGain { .. })
        ));
        assert!(matches!(
            refused(peaking(1000.0, 0.0, 0.05)),
            Err(Error::BandQ { band: 1, .. })
        ));
        assert!(matches!(
            refused(peaking(1000.0, 0.0, 41.0)),
            Err(Error::BandQ { .. })
        ));
        assert!(matches!(
            curve(vec![peaking(1000.0, 0.0, 1.0); MAX_BANDS + 1]).profile(),
            Err(Error::TooManyBands { found: 33 })
        ));
        assert!(
            curve(vec![peaking(1000.0, 0.0, 1.0); MAX_BANDS])
                .profile()
                .is_ok()
        );

        let mut loud = curve(Vec::new());
        loud.preamp_db = f32::NAN;
        assert!(matches!(loud.profile(), Err(Error::PreampRange { .. })));
    }

    #[test]
    fn ten_octave_gains_make_ten_peaking_bands_at_the_octave_centres() {
        let gains = vec![1.0, -2.0, 3.5, 0.0, 0.0, 6.0, -12.0, 0.5, 0.0, 12.0];

        let octaves = ten_band_curve(-6.0, gains.clone());
        let before = Profile::new(
            Preamp::from_decibels(-6.0).expect("in range"),
            EQUALISER_CENTRES
                .iter()
                .zip(&gains)
                .map(|(&centre, &gain)| {
                    Band::peaking(
                        Frequency::from_hertz(centre).expect("in range"),
                        BandGain::from_decibels(f64::from(gain)).expect("in range"),
                        Q::from_units(SQRT_2).expect("in range"),
                    )
                })
                .collect(),
        )
        .expect("ten bands");

        assert_eq!(octaves.bands.len(), 10);
        assert_eq!(octaves.profile().expect("a valid curve"), before);
        assert_eq!(ten_band_gains(octaves.clone()), Some(gains));
        assert_eq!(ten_band_gains(curve(vec![peaking(1000.0, 1.0, 1.0)])), None);
    }

    #[test]
    fn a_ten_band_curve_holds_what_no_band_can_hold() {
        let octaves = ten_band_curve(f32::NAN, vec![f32::INFINITY, 99.0]);

        assert_eq!(octaves.preamp_db, 0.0);
        assert_eq!(octaves.bands[0].gain_db, 0.0);
        assert_eq!(octaves.bands[1].gain_db, GAIN_AT_MOST_DB);
        assert_eq!(octaves.bands[9].gain_db, 0.0);
        assert!(octaves.profile().is_ok());
    }

    #[test]
    fn every_preset_is_a_named_octave_curve_and_only_bass_only_boosts_without_lowering_the_preamp()
    {
        let presets = equaliser_presets();
        let names: Vec<&str> = presets.iter().map(|preset| preset.name.as_str()).collect();
        let flat = &presets[0];
        let bass_only = presets
            .iter()
            .find(|preset| preset.name == "Bass only")
            .expect("a Bass only preset");
        let bass_only_gains = ten_band_gains(bass_only.curve.clone()).expect("octave bands");

        assert_eq!(
            names,
            [
                "Flat",
                "Bass boost",
                "Bass only",
                "Bass cut",
                "Vocals",
                "Warm",
                "Bright",
                "Treble cut",
                "Rock",
                "Electronic"
            ]
        );
        assert_eq!(flat.curve, EqualiserCurve::default());
        assert_eq!(bass_only.curve.preamp_db, 0.0);
        assert!(bass_only_gains[..3].iter().all(|&gain| gain > 0.0));
        assert!(bass_only_gains[3..].iter().all(|&gain| gain == 0.0));
        for preset in &presets[1..] {
            let gains = ten_band_gains(preset.curve.clone()).expect("octave bands");
            let boosts = gains.iter().any(|&gain| gain > 0.0);

            assert_eq!(preset.curve.name, preset.name);
            assert!(
                preset.curve.profile().is_ok(),
                "{} is a valid curve",
                preset.name
            );
            assert!(!preset.detail.is_empty());
            if boosts && preset.name != "Bass only" {
                assert!(
                    preset.curve.preamp_db < 0.0,
                    "{} boosts without headroom",
                    preset.name
                );
            }
        }
    }

    #[test]
    fn the_presets_come_in_pairs_so_a_two_column_grid_leaves_no_gap() {
        assert_eq!(equaliser_presets().len() % 2, 0);
    }

    #[test]
    fn a_curve_kept_as_text_reads_back_as_the_same_curve() {
        let kept = EqualiserCurve {
            name: "Kept".into(),
            preamp_db: -4.5,
            bands: vec![
                peaking(31.25, 3.0, 1.414),
                EqualiserBand {
                    filter: EqualiserFilter::HighShelf,
                    frequency_hz: 9_000.5,
                    gain_db: -2.25,
                    q: 0.707,
                    channels: ChannelSet::EVERY.bits(),
                },
                EqualiserBand {
                    filter: EqualiserFilter::HighPass,
                    frequency_hz: 25.0,
                    gain_db: 0.0,
                    q: 0.5,
                    channels: ChannelSet::EVERY.bits(),
                },
            ],
        };

        let text = equaliser_text(kept.clone()).expect("a valid curve");
        let read = parse_eq_profile(text.into_bytes(), "Kept".into()).expect("our own text");

        assert_eq!(read.curve, kept);
        assert_eq!(
            parse_eq_profile(
                equaliser_text(EqualiserCurve::default())
                    .expect("a flat curve")
                    .into_bytes(),
                String::new()
            )
            .expect("our own text")
            .curve,
            EqualiserCurve::default()
        );
    }
}
