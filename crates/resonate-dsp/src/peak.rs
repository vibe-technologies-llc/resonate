use std::{collections::VecDeque, num::NonZeroUsize};

use resonate_core::StreamSpec;

use crate::{
    ProcessCount, Processor, Result,
    fused::{larger, multiply_add},
    resample::{SincParams, WindowedSinc},
};

const OVERSAMPLED: usize = 8;
const INTERPOLATOR: SincParams = SincParams {
    half_taps: 48,
    phases: OVERSAMPLED as u32,
    cutoff: 0.998,
    kaiser_beta: 10.0,
};
const REACH: usize = INTERPOLATOR.half_taps as usize;
const TAPS: usize = 2 * REACH;
const HELD_TWICE: usize = 2 * TAPS;
const LOOKAHEAD_SECONDS: f64 = 0.0015;
const RELEASE_SECONDS: f64 = 0.08;
const CEILING_DECIBELS: f64 = -0.1;
const DECIBELS_PER_DECADE: f64 = 20.0;
const BACK_AT_UNITY_WITHIN: f64 = 1.0 / 4_294_967_296.0;
const ROUNDING_MARGIN: f64 = 1.0 - 1e-9;

struct Oversampler {
    weights: [[f64; OVERSAMPLED]; TAPS],
    gain_at_most: f64,
    held: Vec<f64>,
    newest: usize,
    loud_for: usize,
    channels: usize,
}

impl Oversampler {
    fn new(channels: usize) -> Self {
        let windowed = WindowedSinc::new(INTERPOLATOR);
        let mut weights = [[0.0; OVERSAMPLED]; TAPS];
        if let Some(lane) = weights
            .get_mut(REACH - 1)
            .and_then(|lanes| lanes.first_mut())
        {
            *lane = 1.0;
        }
        for phase in 1..OVERSAMPLED {
            let fraction = phase as f64 / OVERSAMPLED as f64;
            let row: [f64; TAPS] = std::array::from_fn(|tap| {
                let distance = tap as f64 - (REACH - 1) as f64 - fraction;
                windowed.at(distance.abs())
            });
            let gain: f64 = row.iter().sum();
            for (lanes, weight) in weights.iter_mut().zip(row) {
                if let Some(lane) = lanes.get_mut(phase) {
                    *lane = weight / gain;
                }
            }
        }
        let gain_at_most = (0..OVERSAMPLED)
            .map(|phase| {
                weights
                    .iter()
                    .filter_map(|lanes| lanes.get(phase))
                    .map(|weight| weight.abs())
                    .sum::<f64>()
            })
            .fold(0.0, f64::max);
        Self {
            weights,
            gain_at_most,
            held: vec![0.0; channels * HELD_TWICE],
            newest: 0,
            loud_for: 0,
            channels,
        }
    }

    fn quiet_below(&self, ceiling: f64) -> f64 {
        ceiling / self.gain_at_most * ROUNDING_MARGIN
    }

    fn clear(&mut self) {
        self.held.fill(0.0);
        self.newest = 0;
        self.loud_for = 0;
    }

    fn loudest_after(&mut self, frame: &[f64]) -> f64 {
        self.hold(frame);
        let loudest = self.read();
        self.advance();
        loudest
    }

    fn loudest_that_could_pass(&mut self, frame: &[f64], quiet_below: f64) -> f64 {
        if self.hold(frame) > quiet_below {
            self.loud_for = TAPS;
        }
        let loudest = if self.loud_for == 0 {
            0.0
        } else {
            self.loud_for -= 1;
            self.read()
        };
        self.advance();
        loudest
    }

    fn hold(&mut self, frame: &[f64]) -> f64 {
        let slot = self.newest;
        let mut loudest: f64 = 0.0;
        for (channel, sample) in frame.iter().enumerate().take(self.channels) {
            let plane = channel * HELD_TWICE;
            for copy in [slot, slot + TAPS] {
                if let Some(held) = self.held.get_mut(plane + copy) {
                    *held = *sample;
                }
            }
            loudest = loudest.max(sample.abs());
        }
        loudest
    }

    fn read(&self) -> f64 {
        let mut loudest = [0.0; OVERSAMPLED];
        let mut channel = 0;
        while channel + 2 <= self.channels {
            let (left, right) = self.phases_of_a_pair(channel);
            for ((loudest, left), right) in loudest.iter_mut().zip(left).zip(right) {
                *loudest = larger(*loudest, larger(left.abs(), right.abs()));
            }
            channel += 2;
        }
        if channel < self.channels {
            for (loudest, phase) in loudest.iter_mut().zip(self.phases_of(channel)) {
                *loudest = larger(*loudest, phase.abs());
            }
        }
        loudest.into_iter().fold(0.0, larger)
    }

    fn window_of(&self, channel: usize) -> &[f64] {
        let from = channel * HELD_TWICE + self.newest + 1;
        self.held.get(from..from + TAPS).unwrap_or_default()
    }

    fn phases_of(&self, channel: usize) -> [f64; OVERSAMPLED] {
        let mut phases = [0.0; OVERSAMPLED];
        for (lanes, sample) in self.weights.iter().zip(self.window_of(channel)) {
            for (phase, weight) in phases.iter_mut().zip(lanes) {
                *phase = multiply_add(*weight, *sample, *phase);
            }
        }
        phases
    }

    fn phases_of_a_pair(&self, first: usize) -> ([f64; OVERSAMPLED], [f64; OVERSAMPLED]) {
        let mut left = [0.0; OVERSAMPLED];
        let mut right = [0.0; OVERSAMPLED];
        let pairs = self.window_of(first).iter().zip(self.window_of(first + 1));
        for (lanes, (sample, beside)) in self.weights.iter().zip(pairs) {
            for ((left, right), weight) in left.iter_mut().zip(right.iter_mut()).zip(lanes) {
                *left = multiply_add(*weight, *sample, *left);
                *right = multiply_add(*weight, *beside, *right);
            }
        }
        (left, right)
    }

    fn advance(&mut self) {
        self.newest = if self.newest + 1 == TAPS {
            0
        } else {
            self.newest + 1
        };
    }
}

pub struct TruePeakMeter {
    oversampler: Oversampler,
    loudest: f64,
    frame: Vec<f64>,
}

impl TruePeakMeter {
    pub fn new(channels: NonZeroUsize) -> Self {
        Self {
            oversampler: Oversampler::new(channels.get()),
            loudest: 0.0,
            frame: vec![0.0; channels.get()],
        }
    }

    pub fn note(&mut self, interleaved: &[f32]) {
        let channels = self.frame.len();
        for frame in interleaved.chunks_exact(channels) {
            for (slot, sample) in self.frame.iter_mut().zip(frame) {
                *slot = f64::from(*sample);
            }
            let loudest = self.oversampler.loudest_after(&self.frame);
            self.loudest = self.loudest.max(loudest);
        }
    }

    pub fn finish(mut self) -> f64 {
        let silence = vec![0.0; self.frame.len()];
        for _ in 0..TAPS {
            let loudest = self.oversampler.loudest_after(&silence);
            self.loudest = self.loudest.max(loudest);
        }
        self.loudest
    }
}

pub struct TruePeak {
    ceiling: f64,
    quiet_below: f64,
    channels: usize,
    oversampler: Option<Oversampler>,
    lookahead: usize,
    delay: usize,
    release: f64,
    delayed: Vec<f64>,
    write_at: usize,
    taken: u64,
    lows: VecDeque<Low>,
    smallest: Vec<f64>,
    smallest_under: usize,
    smallest_sum: f64,
    per_window: f64,
    ring_at: usize,
    applied: f64,
    silence: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Low {
    at: u64,
    wanted: f64,
}

impl TruePeak {
    pub fn new() -> Self {
        let ceiling = 10_f64.powf(CEILING_DECIBELS / DECIBELS_PER_DECADE);
        Self {
            ceiling,
            quiet_below: 0.0,
            channels: 1,
            oversampler: None,
            lookahead: 0,
            delay: 0,
            release: 1.0,
            delayed: Vec::new(),
            write_at: 0,
            taken: 0,
            lows: VecDeque::new(),
            smallest: Vec::new(),
            smallest_under: 0,
            smallest_sum: 0.0,
            per_window: 1.0,
            ring_at: 0,
            applied: 1.0,
            silence: Vec::new(),
        }
    }

    fn step(&mut self, frame: &[f64], output: &mut [f64]) -> bool {
        let Some(oversampler) = self.oversampler.as_mut() else {
            return false;
        };
        let loudest = oversampler.loudest_that_could_pass(frame, self.quiet_below);
        let wanted = if loudest > self.ceiling {
            self.ceiling / loudest
        } else {
            1.0
        };

        let quietest = self.quietest_with(wanted);
        let smoothed = self.smoothed_with(quietest);
        let recovered = self.applied + (1.0 - self.applied) * self.release;
        let recovered = if 1.0 - recovered < BACK_AT_UNITY_WITHIN {
            1.0
        } else {
            recovered
        };
        self.applied = smoothed.min(recovered);

        let channels = self.channels;
        let into = self.write_at;
        let out = if into == self.delay { 0 } else { into + 1 };
        if let Some(slot) = self.delayed.get_mut(into * channels..(into + 1) * channels) {
            for (held, sample) in slot.iter_mut().zip(frame) {
                *held = *sample;
            }
        }
        self.write_at = out;
        self.taken += 1;
        if self.taken <= self.delay as u64 {
            return false;
        }
        let ready = self
            .delayed
            .get(out * channels..(out + 1) * channels)
            .unwrap_or_default();
        for (slot, sample) in output.iter_mut().zip(ready) {
            *slot = sample * self.applied;
        }
        true
    }

    fn quietest_with(&mut self, wanted: f64) -> f64 {
        let at = self.taken;
        while self.lows.back().is_some_and(|low| low.wanted >= wanted) {
            self.lows.pop_back();
        }
        self.lows.push_back(Low { at, wanted });
        let window = self.lookahead as u64 + 1;
        while self.lows.front().is_some_and(|low| low.at + window <= at) {
            self.lows.pop_front();
        }
        self.lows.front().map_or(1.0, |low| low.wanted)
    }

    fn smoothed_with(&mut self, quietest: f64) -> f64 {
        let at = self.ring_at;
        if let Some(slot) = self.smallest.get_mut(at) {
            if *slot < 1.0 {
                self.smallest_under -= 1;
            }
            if quietest < 1.0 {
                self.smallest_under += 1;
            }
            self.smallest_sum += quietest - *slot;
            *slot = quietest;
        }
        self.ring_at = if at == self.lookahead {
            self.smallest_sum = self.smallest.iter().sum();
            0
        } else {
            at + 1
        };
        if self.smallest_under == 0 {
            1.0
        } else {
            self.smallest_sum * self.per_window
        }
    }
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor for TruePeak {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        let channels = usize::from(spec.channel_count().get());
        let rate = f64::from(spec.rate.hz());
        self.channels = channels;
        let oversampler = Oversampler::new(channels);
        self.quiet_below = oversampler.quiet_below(self.ceiling);
        self.oversampler = Some(oversampler);
        self.lookahead = (LOOKAHEAD_SECONDS * rate).ceil() as usize;
        self.delay = self.lookahead + REACH;
        self.release = 1.0 - (-1.0 / (RELEASE_SECONDS * rate)).exp();
        self.delayed = vec![0.0; (self.delay + 1) * channels];
        self.lows = VecDeque::with_capacity(self.lookahead + 2);
        self.smallest = vec![1.0; self.lookahead + 1];
        self.per_window = 1.0 / (self.lookahead + 1) as f64;
        self.silence = vec![0.0; channels];
        self.reset();
        Ok(max_frames_in)
    }

    fn max_flush_frames(&self) -> usize {
        self.delay
    }

    fn reset(&mut self) {
        if let Some(oversampler) = self.oversampler.as_mut() {
            oversampler.clear();
        }
        self.delayed.fill(0.0);
        self.lows.clear();
        self.smallest.fill(1.0);
        self.smallest_under = 0;
        self.smallest_sum = self.smallest.len() as f64;
        self.write_at = 0;
        self.taken = 0;
        self.ring_at = 0;
        self.applied = 1.0;
    }

    fn latency_frames(&self) -> f64 {
        self.delay as f64
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let channels = self.channels;
        let frames = input.len() / channels;
        let room = output.len() / channels;
        let mut produced = 0;
        let mut taken = 0;
        for frame in input.chunks_exact(channels) {
            let held_back = self.taken < self.delay as u64;
            if !held_back && produced >= room {
                break;
            }
            let slot = output
                .get_mut(produced * channels..(produced + 1) * channels)
                .unwrap_or_default();
            if self.step(frame, slot) {
                produced += 1;
            }
            taken += 1;
        }
        ProcessCount {
            frames_in: taken.min(frames),
            frames_out: produced,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let channels = self.channels;
        let owed = self.taken.min(self.delay as u64) as usize;
        let silence = std::mem::take(&mut self.silence);
        let mut produced = 0;
        for _ in 0..self.delay {
            if produced == owed {
                break;
            }
            let Some(slot) = output.get_mut(produced * channels..(produced + 1) * channels) else {
                break;
            };
            if self.step(&silence, slot) {
                produced += 1;
            }
        }
        self.silence = silence;
        produced
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_4, PI, TAU};

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

    use super::*;

    const BLOCK: usize = 512;
    const READS_WITHIN_DB: f64 = 0.05;
    const NEAR_NYQUIST_READS_WITHIN_DB: f64 = 0.1;
    const FADE_FRAMES: usize = 1_200;

    fn decibels(ratio: f64) -> f64 {
        20.0 * ratio.log10()
    }

    fn sine(hz: f64, amplitude: f64, phase: f64, frames: usize) -> Vec<f64> {
        let step = TAU * hz / 48_000.0;
        (0..frames)
            .flat_map(|n| {
                let sample = amplitude * (step * n as f64 + phase).sin();
                [sample, sample]
            })
            .collect()
    }

    fn faded(mut samples: Vec<f64>) -> Vec<f64> {
        let frames = samples.len() / 2;
        for (frame, pair) in samples.as_chunks_mut::<2>().0.iter_mut().enumerate() {
            let from_an_end = frame.min(frames - 1 - frame);
            if from_an_end < FADE_FRAMES {
                let rising = 0.5 - 0.5 * (PI * from_an_end as f64 / FADE_FRAMES as f64).cos();
                for sample in pair {
                    *sample *= rising;
                }
            }
        }
        samples
    }

    fn metered(samples: &[f64]) -> f64 {
        let mut meter = TruePeakMeter::new(NonZeroUsize::new(2).expect("two channels"));
        let narrow: Vec<f32> = samples.iter().map(|sample| *sample as f32).collect();
        meter.note(&narrow);
        meter.finish()
    }

    fn guarded(input: &[f64]) -> (Vec<f64>, TruePeak) {
        let mut stage = TruePeak::new();
        stage
            .prepare(
                StreamSpec::new(
                    SampleRate::HZ_48000,
                    ChannelLayout::Stereo,
                    SampleFormat::F32,
                ),
                BLOCK,
            )
            .expect("the guard prepares");
        let mut output = Vec::new();
        let mut scratch = vec![0.0; BLOCK * 2];
        for block in input.chunks(BLOCK * 2) {
            let count = stage.process(block, &mut scratch);
            assert_eq!(count.frames_in, block.len() / 2);
            output.extend_from_slice(&scratch[..count.frames_out * 2]);
        }
        let mut tail = vec![0.0; stage.max_flush_frames() * 2];
        let drained = stage.flush(&mut tail);
        output.extend_from_slice(&tail[..drained * 2]);
        (output, stage)
    }

    #[test]
    fn the_meter_reads_the_peak_between_the_samples() {
        let straddled = faded(sine(12_000.0, 1.0, FRAC_PI_4, 4_800));
        let sampled = straddled
            .iter()
            .fold(0.0_f64, |loudest, s| loudest.max(s.abs()));
        assert!(
            decibels(sampled) < -2.9,
            "the samples already reached {sampled}"
        );

        let read = decibels(metered(&straddled));
        assert!(
            read.abs() < READS_WITHIN_DB,
            "the meter read {read:.3} dBTP"
        );
    }

    #[test]
    fn a_tone_at_nine_tenths_of_nyquist_is_read_at_its_peak() {
        let high = faded(sine(21_600.0, 1.0, 0.0, 48_000));
        let read = decibels(metered(&high));
        assert!(
            read.abs() < NEAR_NYQUIST_READS_WITHIN_DB,
            "the meter read {read:.3} dBTP"
        );
    }

    #[test]
    fn every_phase_passes_a_tone_at_nineteen_twentieths_of_nyquist_within_a_tenth_of_a_decibel() {
        const FRACTION_OF_NYQUIST: f64 = 0.95;
        let omega = PI * FRACTION_OF_NYQUIST;
        let weights = Oversampler::new(1).weights;
        for phase in 1..OVERSAMPLED {
            let (real, imaginary) =
                weights
                    .iter()
                    .enumerate()
                    .fold((0.0, 0.0), |(real, imaginary), (tap, lanes)| {
                        let weight = lanes[phase];
                        let turned = omega * tap as f64;
                        (
                            real + weight * turned.cos(),
                            imaginary - weight * turned.sin(),
                        )
                    });
            let passed = decibels(real.hypot(imaginary));
            assert!(
                passed.abs() < NEAR_NYQUIST_READS_WITHIN_DB,
                "phase {phase} passed the tone at {passed:.3} dB"
            );
        }
    }

    #[test]
    fn the_detector_skips_only_frames_no_phase_could_carry_over_the_ceiling() {
        let ceiling = 10_f64.powf(CEILING_DECIBELS / DECIBELS_PER_DECADE);
        let mut exact = Oversampler::new(2);
        let mut skipping = Oversampler::new(2);
        let quiet_below = skipping.quiet_below(ceiling);
        let quiet = sine(18_000.0, quiet_below, 0.4, 3_000);
        let loud = sine(18_000.0, 1.3, 0.4, 3_000);
        let mut skipped = 0;
        let frames = [&quiet, &loud, &quiet]
            .into_iter()
            .flat_map(|run| run.as_chunks::<2>().0);
        for frame in frames {
            let read = exact.loudest_after(frame);
            let guarded = skipping.loudest_that_could_pass(frame, quiet_below);
            if guarded == 0.0 {
                skipped += 1;
                assert!(read <= ceiling, "a skipped frame read {read}");
            } else {
                assert_eq!(guarded, read);
            }
        }
        assert!(skipped > 5_000, "only {skipped} frames were skipped");
    }

    #[test]
    fn a_stream_that_never_nears_full_scale_passes_through_exactly_and_whole() {
        let quiet = sine(997.0, 0.5, 0.3, 9_000);
        let (output, stage) = guarded(&quiet);
        assert_eq!(
            output.len(),
            quiet.len(),
            "the guard changed the stream's length"
        );
        assert_eq!(output, quiet, "a stream under the ceiling was touched");
        assert!(stage.latency_frames() > 0.0);
    }

    #[test]
    fn a_stream_shorter_than_the_lookahead_is_flushed_whole() {
        for frames in [1, 40, 50, 95, 96, 97] {
            let short = sine(997.0, 0.5, 0.3, frames);
            let (output, _) = guarded(&short);
            assert_eq!(output, short, "{frames} frames came back otherwise");
        }
    }

    #[test]
    fn an_over_between_the_samples_is_ridden_down_under_the_ceiling() {
        let hot = sine(12_000.0, 1.25, FRAC_PI_4, 48_000);
        let before = decibels(metered(&hot));
        assert!(before > 1.8, "the test tone read only {before:.2} dBTP");

        let (output, _) = guarded(&hot);
        let after = decibels(metered(&output));
        assert!(
            after <= CEILING_DECIBELS + READS_WITHIN_DB,
            "the guard let {after:.3} dBTP through"
        );
    }

    #[test]
    fn a_lone_transient_is_attenuated_ahead_of_itself_and_released_after() {
        let mut signal = sine(440.0, 0.3, 0.0, 4 * 48_000);
        for frame in 12_000..12_004 {
            signal[frame * 2] = 1.4;
            signal[frame * 2 + 1] = 1.4;
        }
        let (output, _) = guarded(&signal);
        let loudest = output
            .iter()
            .fold(0.0_f64, |loudest, s| loudest.max(s.abs()));
        assert!(
            decibels(loudest) <= CEILING_DECIBELS + 1e-9,
            "a sample reached {loudest}"
        );
        let settled = &output[output.len() - 2_000..];
        let original = &signal[signal.len() - 2_000..];
        assert_eq!(
            settled, original,
            "the guard never let go after the transient"
        );
    }
}
