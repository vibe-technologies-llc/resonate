use std::f64::consts::{PI, TAU};

use resonate_core::{
    SampleRate,
    eq::{Band, BandGain, BandKind, Biquad, Frequency, Q},
};

use crate::bands::{BANDS, BandPeaks, edge_of};

const EDGES: usize = BANDS - 1;

const SECTIONS_AN_EDGE: usize = 2;

const SECTIONS_OF_THE_ORDER: usize = 2 * SECTIONS_AN_EDGE;

const CHUNKS_A_SECOND: usize = 4;

const MARGINS_A_SECOND: usize = 20;

const PROBE_OFFSETS: usize = 64;

const PROBE_FRAMES: isize = 4;

const LEAST_SEEN_OF_A_PEAK: f64 = 0.25;

const LAST_MIDPOINT_FRAMES: usize = 2;

const WIDEST_EDGE_OF_A_BAND_READ: f64 = 0.9;

const OCTAVES_BEYOND_A_BAND_READ: f64 = 2.0 / 3.0;

type Plane = [f64; EDGES];

type Lanes = [f64; BANDS];

struct Coefficients {
    b0: [Plane; SECTIONS_AN_EDGE],
    b1: [Plane; SECTIONS_AN_EDGE],
    b2: [Plane; SECTIONS_AN_EDGE],
    a1: [Plane; SECTIONS_AN_EDGE],
    a2: [Plane; SECTIONS_AN_EDGE],
}

impl Coefficients {
    fn at(rate: SampleRate) -> Self {
        let mut planes = Self {
            b0: [[1.0; EDGES]; SECTIONS_AN_EDGE],
            b1: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            b2: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            a1: [[0.0; EDGES]; SECTIONS_AN_EDGE],
            a2: [[0.0; EDGES]; SECTIONS_AN_EDGE],
        };
        for edge in 0..EDGES {
            let frequency = Frequency::from_hertz(edge_of(edge + 1)).unwrap_or(Frequency::HIGHEST);
            for section in 0..SECTIONS_AN_EDGE {
                let q = Q::from_units(butterworth_q(section)).unwrap_or(Q::BUTTERWORTH);
                let Biquad { b0, b1, b2, a1, a2 } =
                    Band::new(BandKind::LowPass, frequency, BandGain::FLAT, q).design(rate);
                planes.b0[section][edge] = b0;
                planes.b1[section][edge] = b1;
                planes.b2[section][edge] = b2;
                planes.a1[section][edge] = a1;
                planes.a2[section][edge] = a2;
            }
        }
        planes
    }
}

fn butterworth_q(section: usize) -> f64 {
    let angle = PI * (2 * section + 1) as f64 / (2 * SECTIONS_OF_THE_ORDER) as f64;
    1.0 / (2.0 * angle.cos())
}

struct Cascades {
    first: [Plane; SECTIONS_AN_EDGE],
    second: [Plane; SECTIONS_AN_EDGE],
}

impl Cascades {
    const SILENT: Self = Self {
        first: [[0.0; EDGES]; SECTIONS_AN_EDGE],
        second: [[0.0; EDGES]; SECTIONS_AN_EDGE],
    };

    fn step(&mut self, coefficients: &Coefficients, heard: &mut Plane) {
        for section in 0..SECTIONS_AN_EDGE {
            let b0 = &coefficients.b0[section];
            let b1 = &coefficients.b1[section];
            let b2 = &coefficients.b2[section];
            let a1 = &coefficients.a1[section];
            let a2 = &coefficients.a2[section];
            let first = &mut self.first[section];
            let second = &mut self.second[section];
            for edge in 0..EDGES {
                let input = heard[edge];
                let output = b0[edge] * input + first[edge];
                first[edge] = b1[edge] * input - a1[edge] * output + second[edge];
                second[edge] = b2[edge] * input - a2[edge] * output;
                heard[edge] = output;
            }
        }
    }
}

struct Seen {
    before: [Lanes; 3],
    loudest: Lanes,
}

impl Seen {
    const NOTHING: Self = Self {
        before: [[0.0; BANDS]; 3],
        loudest: [0.0; BANDS],
    };

    fn note(&mut self, lanes: &Lanes) {
        let [oldest, older, newer] = &self.before;
        for band in 0..BANDS {
            let midway = (9.0 * (older[band] + newer[band]) - oldest[band] - lanes[band]) / 16.0;
            let louder = midway.abs().max(lanes[band].abs());
            self.loudest[band] = self.loudest[band].max(louder);
        }
        self.before = [self.before[1], self.before[2], *lanes];
    }
}

pub(crate) struct Splitting {
    channels: usize,
    chunk: usize,
    margin: usize,
    coefficients: Coefficients,
    windows: Vec<Vec<f64>>,
    settled: usize,
    begun: bool,
    seen: Vec<Seen>,
    stored: Vec<Plane>,
    shortfalls: Lanes,
}

impl Splitting {
    pub(crate) fn new(rate: SampleRate, channels: usize) -> Self {
        let channels = channels.max(1);
        let hz = rate.hz() as usize;
        Self {
            channels,
            chunk: (hz / CHUNKS_A_SECOND).max(1),
            margin: (hz / MARGINS_A_SECOND).max(1),
            coefficients: Coefficients::at(rate),
            windows: vec![Vec::new(); channels],
            settled: 0,
            begun: false,
            seen: (0..channels).map(|_| Seen::NOTHING).collect(),
            stored: Vec::new(),
            shortfalls: shortfalls_at(rate),
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        for frame in interleaved.chunks_exact(self.channels) {
            for (window, sample) in self.windows.iter_mut().zip(frame) {
                window.push(f64::from(*sample));
            }
        }
        while self.frames_held() >= self.settled + self.chunk + self.margin {
            self.settle(self.chunk, false);
        }
    }

    pub(crate) fn finished(mut self) -> BandPeaks {
        let unsettled = self.frames_held() - self.settled;
        self.settle(unsettled, true);
        for seen in &mut self.seen {
            for _ in 0..LAST_MIDPOINT_FRAMES {
                seen.note(&[0.0; BANDS]);
            }
        }

        BandPeaks::of((0..BANDS).map(|band| {
            let loudest = self
                .seen
                .iter()
                .map(|seen| seen.loudest[band])
                .fold(0.0, f64::max);
            loudest / self.shortfalls[band]
        }))
    }

    fn frames_held(&self) -> usize {
        self.windows.first().map_or(0, Vec::len)
    }

    fn settle(&mut self, frames: usize, ending: bool) {
        if frames == 0 {
            return;
        }
        for channel in 0..self.channels {
            self.settle_channel(channel, frames, ending);
        }
        self.begun = true;
        self.settled += frames;
        if self.settled > self.margin {
            let behind = self.settled - self.margin;
            for window in &mut self.windows {
                window.drain(..behind);
            }
            self.settled = self.margin;
        }
    }

    fn settle_channel(&mut self, channel: usize, frames: usize, ending: bool) {
        let window = &self.windows[channel];
        let before = if self.begun {
            0
        } else {
            self.margin.min(window.len().saturating_sub(1))
        };
        let after = if ending {
            self.margin.min(window.len().saturating_sub(1))
        } else {
            0
        };
        let from = before + self.settled;
        let to = from + frames;

        self.stored.clear();
        let mut forward = Cascades::SILENT;
        let padded_before = (1..=before).rev().map(|back| window[back]);
        let padded_after = (1..=after).map(|ahead| window[window.len() - 1 - ahead]);
        for sample in padded_before
            .chain(window.iter().copied())
            .chain(padded_after)
        {
            let mut lanes = [sample; EDGES];
            forward.step(&self.coefficients, &mut lanes);
            self.stored.push(lanes);
        }

        let mut backward = Cascades::SILENT;
        for lanes in self.stored[from..].iter_mut().rev() {
            backward.step(&self.coefficients, lanes);
        }

        let seen = &mut self.seen[channel];
        for at in from..to {
            let mut bands = [0.0; BANDS];
            let mut below = 0.0;
            for (band, slot) in bands.iter_mut().enumerate() {
                let above = self.stored[at]
                    .get(band)
                    .copied()
                    .unwrap_or(window[at - before]);
                *slot = above - below;
                below = above;
            }
            seen.note(&bands);
        }
    }
}

fn shortfalls_at(rate: SampleRate) -> Lanes {
    let hz = f64::from(rate.hz());
    let nyquist = hz / 2.0;
    std::array::from_fn(|band| {
        let upper = if band + 1 < BANDS {
            edge_of(band + 1) * 2.0_f64.powf(OCTAVES_BEYOND_A_BAND_READ)
        } else {
            nyquist
        };
        let angle = TAU * upper.min(WIDEST_EDGE_OF_A_BAND_READ * nyquist) / hz;
        shortfall(angle).max(LEAST_SEEN_OF_A_PEAK)
    })
}

fn shortfall(angle: f64) -> f64 {
    (0..PROBE_OFFSETS)
        .map(|step| {
            let offset = angle * step as f64 / PROBE_OFFSETS as f64;
            let tone: Vec<f64> = (-PROBE_FRAMES..=PROBE_FRAMES)
                .map(|frame| (angle * frame as f64 - offset).cos())
                .collect();
            tone.windows(4)
                .flat_map(|four| {
                    let midway = (9.0 * (four[1] + four[2]) - four[0] - four[3]) / 16.0;
                    [four[3].abs(), midway.abs()]
                })
                .fold(0.0, f64::max)
        })
        .fold(f64::INFINITY, f64::min)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate::HZ_44100;

    fn faded_tone(hertz: f64, amplitude: f64, channels: usize) -> Vec<f32> {
        let frames = RATE.hz() as usize * 2;
        (0..frames)
            .flat_map(|frame| {
                let position = frame as f64 / frames as f64;
                let envelope = (PI * position).sin().powi(2);
                let sample = (TAU * hertz * frame as f64 / f64::from(RATE.hz())).sin()
                    * amplitude
                    * envelope;
                vec![sample as f32; channels]
            })
            .collect()
    }

    fn peaks_of(signal: &[f32], channels: usize, piece: usize) -> BandPeaks {
        let mut splitting = Splitting::new(RATE, channels);
        for part in signal.chunks(piece * channels) {
            splitting.note(part);
        }
        splitting.finished()
    }

    fn amplitudes(peaks: &BandPeaks) -> Vec<f64> {
        peaks.amplitudes().to_vec()
    }

    #[test]
    fn a_tone_is_held_between_the_bands_around_it_and_they_hold_no_more_than_it_does() {
        let peaks = amplitudes(&peaks_of(&faded_tone(1_000.0, 0.8, 2), 2, 4_096));

        let together: f64 = peaks.iter().sum();
        let loudest = peaks.iter().copied().fold(0.0, f64::max);

        assert!((0.8..0.8 * 1.1).contains(&together), "{together}");
        assert!(loudest < 0.8 * 0.6, "{loudest}");
    }

    #[test]
    fn a_stream_noted_in_pieces_has_the_peaks_it_has_noted_whole() {
        let signal = faded_tone(250.0, 0.6, 2);

        let whole = peaks_of(&signal, 2, signal.len() / 2);
        let in_blocks = peaks_of(&signal, 2, 1_000);
        let in_odd_blocks = peaks_of(&signal, 2, 7_919);

        assert_eq!(whole, in_blocks);
        assert_eq!(whole, in_odd_blocks);
    }

    #[test]
    fn the_louder_of_two_channels_sets_the_peak_of_each_band_and_silence_has_none() {
        let mut stereo = Vec::new();
        for (loud, quiet) in faded_tone(500.0, 0.7, 1)
            .into_iter()
            .zip(faded_tone(500.0, 0.1, 1))
        {
            stereo.extend([quiet, loud]);
        }
        let silence = vec![0.0_f32; RATE.hz() as usize * 2];

        let heard = amplitudes(&peaks_of(&stereo, 2, 4_096));
        let mono_of_the_louder = amplitudes(&peaks_of(&faded_tone(500.0, 0.7, 1), 1, 4_096));

        assert_eq!(heard, mono_of_the_louder);
        assert!(peaks_of(&silence, 1, 4_096).is_silent());
    }

    #[test]
    fn a_stream_shorter_than_the_margin_still_has_its_peaks() {
        let signal: Vec<f32> = (0..300)
            .map(|frame| {
                (TAU * 1_000.0 * f64::from(frame) / f64::from(RATE.hz())).sin() as f32 * 0.5
            })
            .collect();

        let peaks = peaks_of(&signal, 1, 100);

        assert!(!peaks.is_silent());
    }
}
