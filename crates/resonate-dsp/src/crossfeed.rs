use std::f64::consts::TAU;

use resonate_core::{ChannelLayout, SampleRate, StreamSpec};

use crate::{Error, ProcessCount, Processor, Result};

const DELAY_SECONDS: f64 = 0.000_3;
const DENORMAL_FLOOR: f64 = 1e-30;
const CHANNELS: usize = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Crossfeed {
    #[default]
    Off,
    Light,
    Moderate,
    Strong,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Feed {
    cutoff_hz: f64,
    level_db: f64,
}

impl Crossfeed {
    pub const ALL: [Self; 4] = [Self::Off, Self::Light, Self::Moderate, Self::Strong];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Light => "light",
            Self::Moderate => "moderate",
            Self::Strong => "strong",
        }
    }

    pub fn named(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }

    pub const fn cutoff_hz(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Light => Some(650),
            Self::Moderate | Self::Strong => Some(700),
        }
    }

    pub const fn level_millibels(self) -> Option<u32> {
        match self {
            Self::Off => None,
            Self::Light => Some(9_500),
            Self::Moderate => Some(6_000),
            Self::Strong => Some(4_500),
        }
    }

    fn feed(self) -> Option<Feed> {
        Some(Feed {
            cutoff_hz: f64::from(self.cutoff_hz()?),
            level_db: f64::from(self.level_millibels()?) / 1_000.0,
        })
    }
}

#[derive(Clone, Debug)]
struct Side {
    line: Vec<f64>,
    low: f64,
}

impl Side {
    fn silent(delay: usize) -> Self {
        Self {
            line: vec![0.0; delay],
            low: 0.0,
        }
    }

    fn clear(&mut self) {
        self.line.fill(0.0);
        self.low = 0.0;
    }
}

pub struct Crossfeeder {
    crossfeed: Crossfeed,
    direct: f64,
    cross: f64,
    smoothing: f64,
    head: usize,
    sides: [Side; CHANNELS],
}

impl Crossfeeder {
    pub fn new(crossfeed: Crossfeed) -> Self {
        Self {
            crossfeed,
            direct: 1.0,
            cross: 0.0,
            smoothing: 1.0,
            head: 0,
            sides: [Side::silent(0), Side::silent(0)],
        }
    }

    pub const fn crossfeed(&self) -> Crossfeed {
        self.crossfeed
    }

    pub fn delay_frames_at(rate: SampleRate) -> usize {
        ((DELAY_SECONDS * f64::from(rate.hz())).round() as usize).max(1)
    }
}

impl Processor for Crossfeeder {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        if spec.channels != ChannelLayout::Stereo {
            return Err(Error::ChannelMismatch {
                expected: ChannelLayout::Stereo.count(),
                actual: spec.channel_count(),
            });
        }
        if let Some(feed) = self.crossfeed.feed() {
            let cross_over_direct = 10_f64.powf(-feed.level_db / 20.0);
            self.direct = 1.0 / (1.0 + cross_over_direct);
            self.cross = cross_over_direct / (1.0 + cross_over_direct);
            self.smoothing = 1.0 - (-TAU * feed.cutoff_hz / f64::from(spec.rate.hz())).exp();
        }
        let delay = Self::delay_frames_at(spec.rate);
        self.sides = [Side::silent(delay), Side::silent(delay)];
        self.head = 0;
        Ok(max_frames_in)
    }

    fn reset(&mut self) {
        for side in &mut self.sides {
            side.clear();
        }
        self.head = 0;
    }

    fn latency_frames(&self) -> f64 {
        0.0
    }

    fn is_transparent(&self) -> bool {
        self.crossfeed == Crossfeed::Off
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let (heard, _) = input.as_chunks::<CHANNELS>();
        let (written, _) = output.as_chunks_mut::<CHANNELS>();
        let frames = heard.len().min(written.len());
        let [left_side, right_side] = &mut self.sides;
        let delay = left_side.line.len();
        for (&[left, right], out) in heard.iter().zip(written.iter_mut()) {
            let late_left = swapped(&mut left_side.line, self.head, left);
            let late_right = swapped(&mut right_side.line, self.head, right);
            left_side.low = lowpassed(left_side.low, late_right, self.smoothing);
            right_side.low = lowpassed(right_side.low, late_left, self.smoothing);
            self.head = if self.head + 1 >= delay {
                0
            } else {
                self.head + 1
            };

            *out = [
                self.direct * left + self.cross * left_side.low,
                self.direct * right + self.cross * right_side.low,
            ];
        }
        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, _output: &mut [f64]) -> usize {
        0
    }
}

fn swapped(line: &mut [f64], head: usize, sample: f64) -> f64 {
    line.get_mut(head)
        .map_or(sample, |slot| std::mem::replace(slot, sample))
}

fn lowpassed(state: f64, sample: f64, smoothing: f64) -> f64 {
    let next = state + smoothing * (sample - state);
    if next.abs() < DENORMAL_FLOOR {
        0.0
    } else {
        next
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleFormat;

    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;

    fn stereo() -> StreamSpec {
        StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::F32)
    }

    fn fed(crossfeed: Crossfeed, input: &[f64]) -> Vec<f64> {
        let mut stage = Crossfeeder::new(crossfeed);
        stage.prepare(stereo(), input.len()).expect("a stage");
        let mut out = vec![0.0; input.len()];
        let counted = stage.process(input, &mut out);
        assert_eq!(counted.frames_out * 2, input.len());
        out
    }

    fn tone(hz: f64, frames: usize, channels: [f64; 2]) -> Vec<f64> {
        (0..frames)
            .flat_map(|at| {
                let wave = (TAU * hz * at as f64 / 48_000.0).sin();
                [wave * channels[0], wave * channels[1]]
            })
            .collect()
    }

    fn level(signal: &[f64], channel: usize) -> f64 {
        signal[signal.len() / 2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|frame| frame[channel].abs())
            .fold(0.0, f64::max)
    }

    #[test]
    fn a_hard_panned_left_reaches_the_right_delayed() {
        let mut input = vec![0.0; 400];
        input[0] = 1.0;

        let out = fed(Crossfeed::Moderate, &input);

        let delay = Crossfeeder::delay_frames_at(RATE);
        assert!(
            out[..delay * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .all(|frame| frame[1] == 0.0)
        );
        assert!(out[delay * 2 + 1] > 0.0);
    }

    #[test]
    fn a_hard_panned_left_reaches_the_right_lowpassed_and_attenuated() {
        let low = fed(Crossfeed::Moderate, &tone(100.0, 9_600, [1.0, 0.0]));
        let high = fed(Crossfeed::Moderate, &tone(8_000.0, 9_600, [1.0, 0.0]));

        let bass_across = level(&low, 1);
        let treble_across = level(&high, 1);

        assert!(bass_across < level(&low, 0), "{bass_across}");
        assert!(bass_across > 0.2, "{bass_across}");
        assert!(treble_across < bass_across / 4.0, "{treble_across}");
    }

    #[test]
    fn a_stronger_setting_feeds_more_across() {
        let across = |crossfeed| level(&fed(crossfeed, &tone(100.0, 9_600, [1.0, 0.0])), 1);

        assert!(across(Crossfeed::Light) < across(Crossfeed::Moderate));
        assert!(across(Crossfeed::Moderate) < across(Crossfeed::Strong));
    }

    #[test]
    fn a_steady_mono_signal_keeps_its_level() {
        let input = vec![0.5; 4_000];

        let out = fed(Crossfeed::Strong, &input);

        for frame in out[out.len() - 200..].as_chunks::<2>().0 {
            assert!((frame[0] - 0.5).abs() < 1e-6 && (frame[1] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn a_low_mono_tone_keeps_its_level() {
        let out = fed(Crossfeed::Moderate, &tone(60.0, 19_200, [1.0, 1.0]));

        let heard = level(&out, 0);

        assert!((heard - 1.0).abs() < 0.03, "{heard}");
    }

    #[test]
    fn nothing_the_stage_makes_passes_the_largest_sample_it_was_given() {
        let mut seed = 0x5eed_u64;
        let input: Vec<f64> = (0..40_000)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed & 1 == 0 { 1.0 } else { -1.0 }
            })
            .collect();

        for crossfeed in [Crossfeed::Light, Crossfeed::Moderate, Crossfeed::Strong] {
            let out = fed(crossfeed, &input);
            assert!(out.iter().all(|sample| sample.abs() <= 1.0 + 1e-12));
        }
    }

    #[test]
    fn off_is_transparent_and_names_round_trip() {
        assert!(Crossfeeder::new(Crossfeed::Off).is_transparent());
        assert!(!Crossfeeder::new(Crossfeed::Light).is_transparent());
        for crossfeed in Crossfeed::ALL {
            assert_eq!(Crossfeed::named(crossfeed.as_str()), Some(crossfeed));
        }
    }

    #[test]
    fn a_stage_given_anything_but_stereo_is_refused() {
        let mono = StreamSpec::new(RATE, ChannelLayout::Mono, SampleFormat::F32);

        let refused = Crossfeeder::new(Crossfeed::Light).prepare(mono, 64);

        assert!(matches!(refused, Err(Error::ChannelMismatch { .. })));
    }

    #[test]
    fn reset_forgets_what_was_heard() {
        let mut stage = Crossfeeder::new(Crossfeed::Strong);
        stage.prepare(stereo(), 1_000).expect("a stage");
        let mut out = vec![0.0; 2_000];
        stage.process(&tone(100.0, 1_000, [1.0, 0.0]), &mut out);

        stage.reset();
        let mut quiet = vec![1.0; 2_000];
        stage.process(&[0.0; 2_000], &mut quiet);

        assert!(quiet.iter().all(|sample| *sample == 0.0));
    }
}
