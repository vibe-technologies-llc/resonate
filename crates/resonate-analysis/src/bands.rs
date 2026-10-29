use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, MediaLocation, SampleRate, StreamSpec,
    eq::{Frequency, Profile, RESPONSE_FROM_HZ},
};
use resonate_dsp::response_of;
use rustfft::{FftPlanner, num_complex::Complex};

use crate::{
    Error, Result, Spectrum, Watching,
    analysis::{native_format, opened},
    applied::{Applied, Heard},
    conform::Walking,
    peaks::{BandPeaks, Heights, NODES, TILES_REACHED_EACH_WAY, TREE, WHOLE},
    spectrum::{FLOOR_DB, Transforming},
    split::{Splitting, TILES_A_SECOND, passed_below},
    tones::{Solving, Tones},
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

const TAPERED_SHARE: usize = 4;

const ENVELOPE_BINS_A_TAP: usize = 32;

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
    apart: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BandGains {
    anchors: Vec<Anchor>,
    ringing: Heights,
    tones: Tones,
}

impl BandGains {
    pub fn of(applied: &Applied, rate: SampleRate) -> Result<Self> {
        let at_the_rate = applied.at_rate(rate);
        let hertz = points_across(at_the_rate.profile(), rate);
        let shares: Vec<Heights> = hertz.iter().map(|hertz| shares_at(*hertz, rate)).collect();
        let heard = at_the_rate.heard_at(&hertz, rate)?;
        let reached = TILES_REACHED_EACH_WAY * (rate.hz() as usize / TILES_A_SECOND).max(1);
        let lasting = at_the_rate.correction_frames(rate)? + 2 * reached + 1;
        let soon = Soon::within(&at_the_rate.impulse_responses(lasting, rate)?, reached);
        let (near, held) = soon.heard_at(&hertz, rate);
        let later: Vec<Vec<Heard>> = heard
            .iter()
            .zip(&held)
            .map(|(whole, soon)| {
                whole
                    .iter()
                    .zip(soon)
                    .map(|(whole, soon)| (whole.0 - soon.0, whole.1 - soon.1))
                    .collect()
            })
            .collect();

        let tones = Tones::across(rate);
        let levels = levels_of(&heard);
        let spread = soon.spread_over(&levels, tones.spans(), rate);

        Ok(Self {
            anchors: levels
                .into_iter()
                .zip(spread)
                .map(|(level, apart)| Anchor {
                    level,
                    reach: reach_of(&near, &shares, level),
                    apart,
                })
                .collect(),
            ringing: reach_of(&later, &shares, 0.0),
            tones,
        })
    }

    pub fn bound(&self, peaks: &BandPeaks) -> f64 {
        self.bound_beside(peaks, 0.0)
    }

    pub fn bound_beside(&self, peaks: &BandPeaks, loudest_yet: f64) -> f64 {
        let moments: Vec<Heights> = peaks.moments().collect();
        let whole: Heights = std::array::from_fn(|node| {
            moments
                .iter()
                .fold(0.0, |loudest, heights| f64::max(loudest, heights[node]))
        });
        let mut ranked: Vec<(f64, &Heights)> = moments
            .iter()
            .map(|heights| (self.partitioned(heights), heights))
            .collect();
        ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
        let ring = cheapest(&self.ringing, &whole);
        let floor = loudest_yet - ring;
        let mut held: f64 = 0.0;
        for (partitioned, heights) in ranked {
            if partitioned <= held.max(floor) {
                held = held.max(partitioned);
                break;
            }
            held = held.max(self.solved(heights, partitioned, held.max(floor)));
        }
        held + ring
    }

    fn partitioned(&self, heights: &Heights) -> f64 {
        self.anchors
            .iter()
            .map(|anchor| anchor.level.abs() * heights[WHOLE] + cheapest(&anchor.reach, heights))
            .fold(f64::INFINITY, f64::min)
    }

    fn solved(&self, heights: &Heights, partitioned: f64, floor: f64) -> f64 {
        let mut solving = Solving::over(&self.tones, heights);
        let mut ranked: Vec<(f64, &Anchor)> = self
            .anchors
            .iter()
            .map(|anchor| {
                let level = anchor.level.abs() * heights[WHOLE];
                (level + solving.heard_alone(&anchor.apart), anchor)
            })
            .collect();
        ranked.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut least = partitioned;
        for (at_least, anchor) in ranked {
            if least <= floor || at_least >= least {
                break;
            }
            let level = anchor.level.abs() * heights[WHOLE];
            if let Some(spread) = solving.bound(&anchor.apart, least - level) {
                least = least.min(level + spread);
            }
        }
        least
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

struct Soon {
    centre: usize,
    from: usize,
    taps: Vec<Vec<f64>>,
}

impl Soon {
    fn within(responses: &[Vec<f64>], reached: usize) -> Self {
        let span = 2 * reached;
        let frames = responses.iter().map(Vec::len).max().unwrap_or(0);
        let loudest_at = |frame: usize| {
            responses
                .iter()
                .filter_map(|response| response.get(frame))
                .fold(0.0_f64, |loudest, tap| loudest.max(tap.abs()))
        };
        let centre = (0..frames).fold(0, |loudest, frame| {
            if loudest_at(frame) > loudest_at(loudest) {
                frame
            } else {
                loudest
            }
        });
        let mut energy = vec![0.0; frames + 1];
        for frame in 0..frames {
            let here: f64 = responses
                .iter()
                .filter_map(|response| response.get(frame))
                .map(|tap| tap * tap)
                .sum();
            energy[frame + 1] = energy[frame] + here;
        }
        let held_from =
            |from: usize| energy[(from + span + 1).min(frames)] - energy[from.min(frames)];
        let earliest = centre.saturating_sub(span - span / TAPERED_SHARE);
        let latest = centre.saturating_sub(span / TAPERED_SHARE);
        let from = (earliest..=latest).fold(earliest, |best, from| {
            if held_from(from) > held_from(best) {
                from
            } else {
                best
            }
        });
        let taps = responses
            .iter()
            .map(|response| {
                let mut taps: Vec<f64> =
                    response.iter().skip(from).take(span + 1).copied().collect();
                faded(&mut taps, from > 0);
                taps
            })
            .collect();
        Self { centre, from, taps }
    }

    fn spread_over(&self, levels: &[f64], spans: &[(f64, f64)], rate: SampleRate) -> Vec<Vec<f64>> {
        let longest = self.taps.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let size = (longest * ENVELOPE_BINS_A_TAP).next_power_of_two();
        let fft = FftPlanner::<f64>::new().plan_fft_forward(size);
        let half_bin = std::f64::consts::PI / size as f64;
        let bins_a_hertz = size as f64 / f64::from(rate.hz());
        let last_bin = size / 2;
        let ranges: Vec<(usize, usize)> = spans
            .iter()
            .map(|(from, to)| {
                let first = ((from * bins_a_hertz).round() as usize).min(last_bin);
                let last = ((to * bins_a_hertz).round() as usize).clamp(first, last_bin);
                (first, last)
            })
            .collect();
        let mut spread = vec![vec![0.0_f64; spans.len()]; levels.len()];
        for taps in &self.taps {
            let mut heard = vec![Complex::new(0.0, 0.0); size];
            let mut turning = vec![Complex::new(0.0, 0.0); size];
            let mut bending = 0.0;
            for (into, tap) in taps.iter().enumerate() {
                let offset = (self.from + into) as f64 - self.centre as f64;
                let at = (self.from + into + size - self.centre % size) % size;
                heard[at] = Complex::new(*tap, 0.0);
                turning[at] = Complex::new(offset * tap, 0.0);
                bending += offset * offset * tap.abs();
            }
            fft.process(&mut heard);
            fft.process(&mut turning);
            let bent = half_bin * half_bin / 2.0 * bending;
            let margins: Vec<f64> = turning[..=last_bin]
                .iter()
                .map(|turned| half_bin * turned.norm() + bent)
                .collect();
            for (level, spread) in levels.iter().zip(&mut spread) {
                let apart: Vec<f64> = heard[..=last_bin]
                    .iter()
                    .zip(&margins)
                    .map(|(heard, margin)| (heard.re - level).hypot(heard.im) + margin)
                    .collect();
                for ((first, last), widest) in ranges.iter().zip(spread.iter_mut()) {
                    let here = apart[*first..=*last].iter().copied().fold(0.0, f64::max);
                    *widest = widest.max(here);
                }
            }
        }
        spread
    }

    fn heard_at(&self, hertz: &[f64], rate: SampleRate) -> (Vec<Vec<Heard>>, Vec<Vec<Heard>>) {
        let summed: Vec<Vec<Heard>> = self
            .taps
            .iter()
            .map(|taps| response_of(taps, hertz, rate))
            .collect();
        let turned_by = |hertz: f64, frames: f64| {
            let angle = std::f64::consts::TAU * hertz / f64::from(rate.hz()) * frames;
            let (imaginary, real) = angle.sin_cos();
            (real, imaginary)
        };
        let (near, held) = hertz
            .iter()
            .enumerate()
            .map(|(at, hertz)| {
                let from_the_centre = turned_by(*hertz, self.from as f64 - self.centre as f64);
                let from_the_start = turned_by(*hertz, self.from as f64);
                summed
                    .iter()
                    .map(|sums| {
                        let sum = sums[at];
                        (product(sum, from_the_centre), product(sum, from_the_start))
                    })
                    .unzip::<Heard, Heard, Vec<Heard>, Vec<Heard>>()
            })
            .unzip();
        (near, held)
    }
}

fn product(left: Heard, right: Heard) -> Heard {
    (
        left.0 * right.0 - left.1 * right.1,
        left.0 * right.1 + left.1 * right.0,
    )
}

fn faded(taps: &mut [f64], cut_before: bool) {
    let frames = taps.len();
    let across = (frames / TAPERED_SHARE).max(1);
    let fade = |into: usize| 0.5 - 0.5 * (std::f64::consts::PI * into as f64 / across as f64).cos();
    for (into, tap) in taps.iter_mut().rev().take(across).enumerate() {
        *tap *= fade(into);
    }
    if cut_before {
        for (into, tap) in taps.iter_mut().take(across).enumerate() {
            *tap *= fade(into);
        }
    }
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
    use std::{sync::Arc, time::Duration};

    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Q};
    use resonate_dsp::Impulse;

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

    fn gains_under(profile: &Arc<Profile>) -> BandGains {
        BandGains::of(&Applied::from(Arc::clone(profile)), SampleRate::HZ_44100)
            .expect("gains under a profile")
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
            gains_under(&boosting(100.0, 6.0)).bound(&BandPeaks::SILENT),
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
        let gains = gains_under(&boosting(110.0, 6.0));

        let raised = gains.bound(&tone_of(110.0, 0.5));
        let left = gains.bound(&tone_of(5_000.0, 0.5));

        assert!(
            (0.0..0.3).contains(&(decibels_between(raised, 0.5) - 6.0)),
            "{raised}"
        );
        assert!((0.0..0.25).contains(&decibels_between(left, 0.5)), "{left}");
    }

    #[test]
    fn a_cut_lowers_the_bound_and_a_flat_curve_leaves_the_peak_as_it_was() {
        let peaks = tone_of(110.0, 0.8);
        let flat = gains_under(&Arc::new(Profile::flat()));
        let cutting = gains_under(&bell(110.0, -6.0, 0.2));

        assert!((flat.bound(&peaks) - 0.8).abs() < 0.8 * 0.01);
        assert!(cutting.bound(&peaks) < 0.8 * 0.7);
    }

    #[test]
    fn the_loudest_moment_sets_the_bound_and_a_quiet_one_on_the_boost_need_not() {
        let gains = gains_under(&boosting(110.0, 6.0));
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
        let ringing = gains_under(&bell(25.0, 12.0, 20.0));
        let settled = gains_under(&bell(25.0, 12.0, 1.0));
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

    fn corrected_by(profile: &Arc<Profile>, taps: Vec<f64>) -> BandGains {
        let correction = Impulse::new(SampleRate::HZ_44100, vec![taps]).expect("an impulse");
        BandGains::of(
            &Applied::from(Arc::clone(profile)).corrected_by(Some(Arc::new(correction))),
            SampleRate::HZ_44100,
        )
        .expect("gains under a profile and a correction")
    }

    #[test]
    fn a_correction_scales_the_bound_by_the_gain_it_holds_where_the_music_sounds() {
        let flat = Arc::new(Profile::flat());
        let peaks = tone_of(1_000.0, 0.5);

        let halved = corrected_by(&flat, vec![0.5]).bound(&peaks);
        let doubled = corrected_by(&flat, vec![2.0]).bound(&peaks);

        assert!((halved - 0.25).abs() < 0.25 * 0.02, "{halved}");
        assert!((doubled - 1.0).abs() < 0.02, "{doubled}");
    }

    #[test]
    fn a_correction_boosting_the_bass_raises_the_bound_of_bass_and_a_lowpass_lowers_that_of_treble()
    {
        let flat = Arc::new(Profile::flat());
        let boosting = corrected_by(&flat, vec![1.0, 1.0]);
        let lowpassing = corrected_by(&flat, vec![0.5, 0.5]);

        let bass = boosting.bound(&tone_of(60.0, 0.4));
        let treble = lowpassing.bound(&tone_of(15_000.0, 0.4));
        let bass_left_alone = lowpassing.bound(&tone_of(60.0, 0.4));

        assert!(
            (0.0..0.3).contains(&(decibels_between(bass, 0.4) - 6.0)),
            "{bass}"
        );
        assert!(decibels_between(treble, 0.4) < -3.0, "{treble}");
        assert!(
            (0.0..0.3).contains(&decibels_between(bass_left_alone, 0.4)),
            "{bass_left_alone}"
        );
    }

    #[test]
    fn a_correction_whose_energy_comes_late_is_bounded_where_its_main_lobe_meets_the_music() {
        let flat = Arc::new(Profile::flat());
        let mut late = vec![0.0; 22_052];
        late[22_050] = 1.0;
        late[22_051] = 1.0;
        let gains = corrected_by(&flat, late);
        let bass = tone_of(60.0, 0.45);
        let treble = tone_of(5_000.0, 0.9);
        let moments: Vec<Heights> = bass.moments().chain(treble.moments()).collect();
        let apart = BandPeaks::of_moments(&moments);
        let treble_raised =
            2.0 * (std::f64::consts::PI * 5_000.0 / f64::from(SampleRate::HZ_44100.hz())).cos();

        let alone = gains.bound(&tone_of(1_000.0, 0.5));
        let bound = gains.bound(&apart);

        assert!(alone > 0.5 * 1.9 && alone < 0.5 * 2.0 * 1.05, "{alone}");
        let heard = (0.9 * treble_raised).max(0.45 * 2.0);
        assert!(bound >= heard * 0.99, "{bound} under {heard}");
        assert!(bound < heard * 1.05, "{bound} against {heard}");
    }

    #[test]
    fn a_quiet_band_beside_a_narrow_boost_carries_it_and_the_loud_band_pays_none_of_it() {
        let gains = gains_under(&bell(63.0, 8.0, 4.0));

        let bound = gains.bound(&tone_of(100.0, 0.8));

        assert!(bound >= 0.8 * 0.999, "{bound}");
        assert!(decibels_between(bound, 0.8) < 1.0, "{bound}");
    }

    fn alone_in_the_band_holding(hertz: f64, peak: f64) -> BandPeaks {
        let sounding = band_of(hertz);
        BandPeaks::at_once((0..BANDS).map(|band| if band == sounding { peak } else { 0.0 }))
    }

    #[test]
    fn a_lone_band_under_a_boost_on_it_is_bounded_at_least_at_its_boosted_peak() {
        let boost_db = 6.0;
        let raised = 10_f64.powf(boost_db / 20.0);
        let averaged = corrected_by(&bell(1_000.0, boost_db, 1.0), vec![1.0 / 24.0; 24]);
        for hertz in [100.0, 1_000.0, 5_000.0] {
            let boosted = gains_under(&bell(hertz, boost_db, 1.0));
            for peak in [0.5, 0.9] {
                let bound = boosted.bound(&alone_in_the_band_holding(hertz, peak));

                assert!(
                    bound >= peak * raised * 0.999,
                    "{bound} under {} at {hertz} Hz",
                    peak * raised
                );
            }
        }

        let turn = std::f64::consts::PI * 1_000.0 / f64::from(SampleRate::HZ_44100.hz());
        let averaged_at_the_boost = (24.0 * turn).sin() / (24.0 * turn.sin());
        let heard = averaged.bound(&alone_in_the_band_holding(1_000.0, 0.9));
        let truth = 0.9 * raised * averaged_at_the_boost;

        assert!(heard >= truth * 0.999, "{heard} under {truth}");
    }

    #[test]
    fn the_solved_bound_never_passes_the_partition_and_never_falls_under_a_tone_its_peaks_allow() {
        let profiles = [
            boosting(110.0, 6.0),
            bell(63.0, 8.0, 4.0),
            bell(25.0, 12.0, 20.0),
            bell(3_000.0, -9.0, 0.7),
        ];
        let songs = [
            tone_of(110.0, 0.5),
            tone_of(5_000.0, 0.9),
            BandPeaks::at_once((0..BANDS).map(|band| 0.1 + 0.02 * band as f64)),
        ];
        for profile in &profiles {
            let gains = gains_under(profile);
            for song in &songs {
                for heights in song.moments() {
                    let partitioned = gains.partitioned(&heights);
                    let solved = gains.solved(&heights, partitioned, 0.0);
                    let alone = gains
                        .anchors
                        .iter()
                        .map(|anchor| {
                            anchor.level.abs() * heights[WHOLE]
                                + Solving::over(&gains.tones, &heights).heard_alone(&anchor.apart)
                        })
                        .fold(f64::INFINITY, f64::min);

                    assert!(solved <= partitioned, "{solved} over {partitioned}");
                    assert!(
                        solved == partitioned || solved >= alone * 0.999,
                        "{solved} under {alone}"
                    );
                }
            }
        }
    }
}
