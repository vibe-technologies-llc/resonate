use std::sync::Arc;

use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, MediaLocation, SampleRate, StreamSpec,
    eq::{Frequency, Profile, RESPONSE_FROM_HZ},
};

use crate::{
    Error, Result, Spectrum, Watching,
    analysis::{native_format, opened},
    conform::Walking,
    peaks::{BandPeaks, Heights, NODES, TILES_REACHED_EACH_WAY, TREE, WHOLE},
    spectrum::{FLOOR_DB, Transforming},
    split::{Splitting, TILES_A_SECOND, passed_below},
};

pub const BANDS: usize = 29;

const LOWEST_EDGE_HZ: f64 = 25.0;

const BANDS_AN_OCTAVE: f64 = 3.0;

const STEPS_A_DECIBEL: f64 = 2.0;

const SILENT: u8 = u8::MAX;

const LOWEST_REACH_HZ: f64 = Frequency::LOWEST_CENTIHERTZ as f64 / 100.0;

const POINTS_ACROSS_THE_RANGE: usize = 2_048;

const SHARE_HELD_WHOLE_FROM: f64 = 0.25;

const LEVELS_BETWEEN: usize = 8;

const QUIETEST_LEVEL: f64 = 0.25;

const LEVELS_ALIKE_WITHIN: f64 = 1e-4;

fn band_of(hertz: f64) -> usize {
    if hertz <= LOWEST_EDGE_HZ {
        return 0;
    }
    let band = ((hertz / LOWEST_EDGE_HZ).log2() * BANDS_AN_OCTAVE).floor();
    (band as usize).min(BANDS - 1)
}

pub(crate) fn edge_of(band: usize) -> f64 {
    LOWEST_EDGE_HZ * 2.0_f64.powf(band as f64 / BANDS_AN_OCTAVE)
}

pub fn span_of_band(band: usize) -> (f64, f64) {
    let from = if band == 0 {
        RESPONSE_FROM_HZ
    } else {
        edge_of(band)
    };
    let to = if band + 1 >= BANDS {
        f64::INFINITY
    } else {
        edge_of(band + 1)
    };
    (from, to)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandLevels {
    steps: [u8; BANDS],
}

impl BandLevels {
    pub const SILENT: Self = Self {
        steps: [SILENT; BANDS],
    };

    pub fn of(spectrum: &Spectrum) -> Self {
        let mut powers = [0.0_f64; BANDS];
        let bin_hz = f64::from(spectrum.bin_hz());
        for (bin, level) in spectrum.levels().iter().enumerate().skip(1) {
            if *level > FLOOR_DB {
                powers[band_of(bin as f64 * bin_hz)] += 10_f64.powf(f64::from(*level) / 10.0);
            }
        }

        let loudest = powers.iter().copied().fold(0.0, f64::max);
        if loudest <= 0.0 {
            return Self::SILENT;
        }
        Self {
            steps: powers.map(|power| steps_below(loudest, power)),
        }
    }

    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        <[u8; BANDS]>::try_from(bytes)
            .ok()
            .map(|steps| Self { steps })
    }

    pub const fn as_bytes(&self) -> &[u8] {
        &self.steps
    }

    pub fn is_silent(&self) -> bool {
        self.steps.iter().all(|step| *step == SILENT)
    }
}

fn steps_below(loudest: f64, power: f64) -> u8 {
    if power <= 0.0 {
        return SILENT;
    }
    let steps = (10.0 * (loudest / power).log10() * STEPS_A_DECIBEL).round();
    if steps >= f64::from(SILENT) {
        SILENT
    } else {
        steps as u8
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandFigures {
    pub levels: BandLevels,
    pub peaks: BandPeaks,
}

#[derive(Clone, Debug, PartialEq)]
struct Anchor {
    level: f64,
    reach: Heights,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandGains {
    anchors: Vec<Anchor>,
    ringing: Heights,
}

type Heard = (f64, f64);

impl BandGains {
    pub fn of(profile: &Arc<Profile>, rate: SampleRate) -> Self {
        let at_the_rate = profile.at_rate(rate);
        let hertz = points_across(&at_the_rate, rate);
        let shares: Vec<Heights> = hertz.iter().map(|hertz| shares_at(*hertz, rate)).collect();
        let heard = at_the_rate.heard_at(&hertz, rate);
        let reached = TILES_REACHED_EACH_WAY * (rate.hz() as usize / TILES_A_SECOND).max(1);
        let soon = soon_heard_at(
            &tapered(at_the_rate.impulse_responses(reached, rate)),
            &hertz,
            rate,
        );
        let later: Vec<Vec<Heard>> = heard
            .iter()
            .zip(&soon)
            .map(|(whole, soon)| {
                whole
                    .iter()
                    .zip(soon)
                    .map(|(whole, soon)| (whole.0 - soon.0, whole.1 - soon.1))
                    .collect()
            })
            .collect();

        Self {
            anchors: levels_of(&heard)
                .into_iter()
                .map(|level| Anchor {
                    level,
                    reach: reach_of(&soon, &shares, level),
                })
                .collect(),
            ringing: reach_of(&later, &shares, 0.0),
        }
    }

    pub fn bound(&self, peaks: &BandPeaks) -> f64 {
        let (held, whole) = peaks
            .moments()
            .fold((0.0, [0.0; NODES]), |(held, whole), heights| {
                let bound = self.bound_of(&heights);
                let louder = std::array::from_fn(|node| f64::max(whole[node], heights[node]));
                (f64::max(held, bound), louder)
            });
        held + cheapest(&self.ringing, &whole)
    }

    fn bound_of(&self, heights: &Heights) -> f64 {
        self.anchors
            .iter()
            .map(|anchor| anchor.level.abs() * heights[WHOLE] + cheapest(&anchor.reach, heights))
            .fold(f64::INFINITY, f64::min)
    }
}

fn levels_of(heard: &[Vec<Heard>]) -> Vec<f64> {
    let (quietest, loudest) = heard
        .iter()
        .flatten()
        .map(|(real, imaginary)| real.hypot(*imaginary))
        .fold((f64::INFINITY, 0.0_f64), |(low, high), gain| {
            (low.min(gain), high.max(gain))
        });
    let mut levels = vec![0.0, 1.0];
    if loudest.is_finite() && loudest > 0.0 {
        let from = quietest.clamp(QUIETEST_LEVEL, loudest);
        let ratio = loudest / from;
        levels.extend(
            (0..LEVELS_BETWEEN)
                .map(|step| from * ratio.powf(step as f64 / (LEVELS_BETWEEN - 1) as f64)),
        );
    }
    levels.sort_by(f64::total_cmp);
    levels.dedup_by(|later, earlier| (*later - *earlier).abs() < LEVELS_ALIKE_WITHIN);
    levels
}

fn reach_of(heard: &[Vec<Heard>], shares: &[Heights], level: f64) -> Heights {
    heard
        .iter()
        .zip(shares)
        .fold([0.0; NODES], |mut reach, (responses, shares)| {
            let apart = responses
                .iter()
                .map(|(real, imaginary)| (real - level).hypot(*imaginary))
                .fold(0.0, f64::max);
            for (reach, share) in reach.iter_mut().zip(shares) {
                *reach = reach.max(share * apart);
            }
            reach
        })
}

fn tapered(mut responses: Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    for response in &mut responses {
        let frames = response.len();
        let from = frames * 3 / 4;
        let across = (frames - from).max(1) as f64;
        for (frame, sample) in response.iter_mut().enumerate().skip(from) {
            let into = (frame - from) as f64 / across;
            *sample *= 0.5 + 0.5 * (std::f64::consts::PI * into).cos();
        }
    }
    responses
}

fn soon_heard_at(responses: &[Vec<f64>], hertz: &[f64], rate: SampleRate) -> Vec<Vec<Heard>> {
    hertz
        .iter()
        .map(|hertz| {
            let angle = std::f64::consts::TAU * hertz / f64::from(rate.hz());
            let (turn_imaginary, turn_real) = angle.sin_cos();
            responses
                .iter()
                .map(|response| {
                    let mut turned = (1.0, 0.0);
                    let mut heard = (0.0, 0.0);
                    for sample in response {
                        heard.0 += sample * turned.0;
                        heard.1 += sample * turned.1;
                        turned = (
                            turned.0 * turn_real - turned.1 * turn_imaginary,
                            turned.0 * turn_imaginary + turned.1 * turn_real,
                        );
                    }
                    heard
                })
                .collect()
        })
        .collect()
}

fn cheapest(reach: &Heights, heights: &Heights) -> f64 {
    let mut costs = [0.0; NODES];
    for index in (0..NODES).rev() {
        let own = reach[index] * heights[index];
        costs[index] = match TREE[index].halves {
            Some((lower, upper)) => own.min(costs[lower] + costs[upper]),
            None => own,
        };
    }
    costs[WHOLE]
}

fn points_across(profile: &Profile, rate: SampleRate) -> Vec<f64> {
    let nyquist = f64::from(rate.hz()) / 2.0;
    let ratio = nyquist / LOWEST_REACH_HZ;
    let last = (POINTS_ACROSS_THE_RANGE - 1) as f64;
    let centres = profile
        .bands()
        .iter()
        .filter(|band| band.applies_at(rate))
        .map(|band| band.frequency.hertz());
    (0..POINTS_ACROSS_THE_RANGE)
        .map(|point| LOWEST_REACH_HZ * ratio.powf(point as f64 / last))
        .chain(centres)
        .collect()
}

fn shares_at(hertz: f64, rate: SampleRate) -> Heights {
    let passed: [f64; BANDS + 1] = std::array::from_fn(|edge| passed_below(edge, hertz, rate));
    let held_whole = |share: f64| (share / SHARE_HELD_WHOLE_FROM).min(1.0);
    let weighed: [f64; BANDS] = std::array::from_fn(|band| {
        let share = (passed[band + 1] - passed[band]).max(0.0);
        held_whole(share) * share
    });
    let all_weighed: f64 = weighed.iter().sum();
    std::array::from_fn(|index| {
        let node = TREE[index];
        let share = (passed[node.to] - passed[node.from]).max(0.0);
        let own = held_whole(share);
        let elsewhere = all_weighed - weighed[node.from..node.to].iter().sum::<f64>();
        let spread = own * share + elsewhere;
        if spread > 0.0 { own / spread } else { 0.0 }
    })
}

pub fn band_figures(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    watch: &dyn Watching,
) -> Result<BandFigures> {
    let (mut decoder, info) = opened(sources, location, span)?;
    let codec = resonate_codec::Codec::from_id(info.codec);
    let format = native_format(&info, codec);
    decoder.set_output_format(format);
    decoder.follow_format_changes();

    let rate = info.spec.rate;
    let placements = info.speakers.placements(info.spec.channels.count().get());
    let mut transforming = Transforming::new(rate.hz(), &placements);
    let mut splitting = Splitting::new(rate, usize::from(info.spec.channels.count().get()));
    let mut walking = Walking::new(StreamSpec::new(rate, info.spec.channels, format));
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let Some(heard) = walking.step(&mut decoder)? else {
            break;
        };
        transforming.note(heard.samples, |_| {});
        splitting.note(heard.samples);
    }

    Ok(BandFigures {
        levels: BandLevels::of(&transforming.finished()),
        peaks: splitting.finished(),
    })
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Q};

    use super::*;

    const BIN_HZ: f32 = 10.0;

    fn spectrum_sounding_at(hertz: f32, level_db: f32) -> Spectrum {
        let mut levels = vec![FLOOR_DB; 2_049];
        levels[(hertz / BIN_HZ) as usize] = level_db;
        Spectrum::new(BIN_HZ, levels, Duration::from_secs(10))
    }

    fn bell(hertz: f64, decibels: f64, q: f64) -> Arc<Profile> {
        let band = Band::new(
            BandKind::Peaking,
            Frequency::from_hertz(hertz).expect("in range"),
            BandGain::from_decibels(decibels).expect("in range"),
            Q::from_units(q).expect("in range"),
        );
        Arc::new(Profile::new(Preamp::NONE, vec![band]).expect("a profile in range"))
    }

    fn boosting(hertz: f64, decibels: f64) -> Arc<Profile> {
        bell(hertz, decibels, 2.0)
    }

    #[test]
    fn every_frequency_falls_in_the_band_whose_span_holds_it() {
        for band in 0..BANDS {
            let (from, to) = span_of_band(band);
            let inside = (from * to.min(from * 2.0)).sqrt();
            assert_eq!(band_of(inside), band, "{from} to {to} Hz");
        }
        assert_eq!(band_of(0.0), 0);
        assert_eq!(band_of(96_000.0), BANDS - 1);
    }

    #[test]
    fn a_band_the_music_sounds_in_is_the_loudest_and_the_rest_are_steps_below_it() {
        let levels = BandLevels::of(&spectrum_sounding_at(1_000.0, -6.0));
        let sounding = band_of(1_000.0);

        let steps = levels.as_bytes();

        assert_eq!(steps[sounding], 0);
        assert!(
            steps
                .iter()
                .enumerate()
                .all(|(band, step)| band == sounding || *step == SILENT)
        );
        assert!(!levels.is_silent());
    }

    #[test]
    fn music_that_never_rose_above_the_floor_is_silent_in_every_band() {
        let levels = BandLevels::of(&Spectrum::new(
            BIN_HZ,
            vec![FLOOR_DB; 2_049],
            Duration::ZERO,
        ));

        assert!(levels.is_silent());
        assert_eq!(levels, BandLevels::SILENT);
        assert_eq!(
            BandGains::of(&boosting(100.0, 6.0), SampleRate::HZ_44100).bound(&BandPeaks::SILENT),
            0.0
        );
    }

    #[test]
    fn levels_read_back_from_their_bytes_and_a_wrong_length_reads_as_nothing() {
        let levels = BandLevels::of(&spectrum_sounding_at(440.0, -3.0));

        assert_eq!(BandLevels::from_bytes(levels.as_bytes()), Some(levels));
        assert_eq!(BandLevels::from_bytes(&levels.as_bytes()[1..]), None);
        assert_eq!(BandLevels::from_bytes(&[]), None);
    }

    fn tone_of(hertz: f64, peak: f64) -> BandPeaks {
        let rate = SampleRate::HZ_44100;
        let shares: [f64; BANDS] = std::array::from_fn(|band| {
            passed_below(band + 1, hertz, rate) - passed_below(band, hertz, rate)
        });
        let heights: Heights = std::array::from_fn(|index| {
            let node = TREE[index];
            shares[node.from..node.to].iter().sum::<f64>() * peak
        });
        BandPeaks::of_moments(&[heights])
    }

    fn decibels_between(bound: f64, peak: f64) -> f64 {
        20.0 * (bound / peak).log10()
    }

    #[test]
    fn a_tone_on_a_boost_is_raised_by_it_and_a_tone_far_from_it_is_not() {
        let gains = BandGains::of(&boosting(110.0, 6.0), SampleRate::HZ_44100);

        let raised = gains.bound(&tone_of(110.0, 0.5));
        let left = gains.bound(&tone_of(5_000.0, 0.5));

        assert!(
            (0.0..0.3).contains(&(decibels_between(raised, 0.5) - 6.0)),
            "{raised}"
        );
        assert!((0.0..0.25).contains(&decibels_between(left, 0.5)), "{left}");
    }

    #[test]
    fn a_tone_beside_a_narrow_boost_pays_little_of_its_height() {
        let gains = BandGains::of(&bell(63.0, 8.0, 4.0), SampleRate::HZ_44100);

        let bound = gains.bound(&tone_of(100.0, 0.8));

        let height = 8.0;
        assert!(decibels_between(bound, 0.8) < height / 2.0, "{bound}");
    }

    #[test]
    fn a_cut_lowers_the_bound_and_a_flat_curve_leaves_the_peak_as_it_was() {
        let peaks = tone_of(110.0, 0.8);
        let flat = BandGains::of(&Arc::new(Profile::flat()), SampleRate::HZ_44100);
        let cutting = BandGains::of(&bell(110.0, -6.0, 0.2), SampleRate::HZ_44100);

        assert!((flat.bound(&peaks) - 0.8).abs() < 0.8 * 0.01);
        assert!(cutting.bound(&peaks) < 0.8 * 0.7);
    }

    #[test]
    fn the_loudest_moment_sets_the_bound_and_a_quiet_one_on_the_boost_need_not() {
        let gains = BandGains::of(&boosting(110.0, 6.0), SampleRate::HZ_44100);
        let loud_elsewhere = tone_of(5_000.0, 0.9);
        let quiet_on_the_boost = tone_of(110.0, 0.3);
        let moments: Vec<Heights> = loud_elsewhere
            .moments()
            .chain(quiet_on_the_boost.moments())
            .collect();
        let apart = BandPeaks::of_moments(&moments);
        let together = BandPeaks::of_moments(&[std::array::from_fn(|node| {
            moments[0][node] + moments[1][node]
        })]);

        let bound_apart = gains.bound(&apart);
        let bound_together = gains.bound(&together);

        assert!(decibels_between(bound_apart, 0.9) < 0.2, "{bound_apart}");
        assert!(bound_together > bound_apart + 0.2, "{bound_together}");
    }

    #[test]
    fn a_curve_ringing_past_the_moments_kept_bounds_its_ring_by_the_whole_song() {
        let ringing = BandGains::of(&bell(25.0, 12.0, 20.0), SampleRate::HZ_44100);
        let settled = BandGains::of(&bell(25.0, 12.0, 1.0), SampleRate::HZ_44100);
        let on_the_ring = tone_of(25.0, 0.5);
        let away_from_it = tone_of(5_000.0, 0.5);
        let louder = |peaks: &BandPeaks| peaks.moments().next().expect("a moment");

        let rung = cheapest(&ringing.ringing, &louder(&on_the_ring));
        let rung_away = cheapest(&ringing.ringing, &louder(&away_from_it));
        let settled_ring = cheapest(&settled.ringing, &louder(&on_the_ring));

        assert!(rung > 0.5, "{rung}");
        assert!(rung_away < 0.02, "{rung_away}");
        assert!(settled_ring < rung / 10.0, "{settled_ring}");
        assert!(ringing.bound(&on_the_ring) >= 0.5 * 10_f64.powf(12.0 / 20.0) * 0.99);
    }
}
