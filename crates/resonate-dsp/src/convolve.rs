use std::{collections::VecDeque, sync::Arc};

use parking_lot::Mutex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex};
use resonate_core::{ChannelLayout, SampleRate, StreamSpec};

use crate::{
    Error, FilterPhase, ProcessCount, Processor, Quality, Resampler, ResamplerConfig, Result,
};

const PARTITION_AT_48_KHZ: usize = 1_024;
const FRAMES_A_PARTITION_IS_FOR: u32 = 48_000;

pub fn partition_frames_at(rate: SampleRate) -> usize {
    let scaled = (rate.hz() / FRAMES_A_PARTITION_IS_FOR).max(1) as usize;
    PARTITION_AT_48_KHZ * scaled.next_power_of_two()
}
const POINTS_A_BLOCK: usize = 128;
const RESAMPLED_A_BLOCK_AT_A_TIME: usize = 4_096;
const SPECTRUM_POINTS_A_TAP: usize = 4;
const BYTES_A_TAP: u64 = size_of::<f64>() as u64;
const BYTES_A_BIN: u64 = size_of::<Complex<f64>>() as u64;
pub const RESPONSE_BYTES_AT_MOST: u64 = 512 * 1_024 * 1_024;

fn smooth_length_from(least: usize) -> usize {
    (least.max(2).next_multiple_of(2)..)
        .step_by(2)
        .find(|length| {
            let mut rest = *length;
            for factor in [2, 3, 5] {
                while rest.is_multiple_of(factor) {
                    rest /= factor;
                }
            }
            rest == 1
        })
        .unwrap_or(least)
}

fn refused_over(bytes: u64) -> Result<()> {
    if bytes > RESPONSE_BYTES_AT_MOST {
        return Err(Error::ResponseTooLarge {
            bytes,
            at_most: RESPONSE_BYTES_AT_MOST,
        });
    }
    Ok(())
}

pub type Taps = Arc<[Vec<f64>]>;

#[derive(Debug)]
pub struct Impulse {
    rate: SampleRate,
    taps: Vec<Vec<f64>>,
    crossed: bool,
    at_rates: Mutex<Vec<(SampleRate, Taps)>>,
}

impl PartialEq for Impulse {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate && self.crossed == other.crossed && self.taps == other.taps
    }
}

pub const CROSSED_RESPONSES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Speaker {
    Left,
    Right,
}

impl std::fmt::Display for Speaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
        })
    }
}

impl Impulse {
    pub fn new(rate: SampleRate, taps: Vec<Vec<f64>>) -> Option<Self> {
        if taps.is_empty()
            || taps.iter().any(Vec::is_empty)
            || taps.iter().flatten().any(|tap| !tap.is_finite())
        {
            return None;
        }
        Some(Self {
            rate,
            taps,
            crossed: false,
            at_rates: Mutex::new(Vec::new()),
        })
    }

    pub fn crossed(rate: SampleRate, taps: Vec<Vec<f64>>) -> Option<Self> {
        if taps.len() != CROSSED_RESPONSES {
            return None;
        }
        Self::new(rate, taps).map(|impulse| Self {
            crossed: true,
            ..impulse
        })
    }

    pub fn binaural(left: &Self, right: &Self) -> Result<Self> {
        for (speaker, response) in [(Speaker::Left, left), (Speaker::Right, right)] {
            if response.crossed {
                return Err(Error::SpeakerAlreadyCrossed { speaker });
            }
            if response.taps.len() != 2 {
                return Err(Error::SpeakerNotStereo {
                    speaker,
                    channels: response.taps.len(),
                });
            }
        }
        if left.rate != right.rate {
            return Err(Error::SpeakerRatesDiffer {
                left: left.rate,
                right: right.rate,
            });
        }
        if left.frames() != right.frames() {
            return Err(Error::SpeakerLengthsDiffer {
                left: left.frames(),
                right: right.frames(),
            });
        }
        let taps = left.taps.iter().chain(&right.taps).cloned().collect();
        Self::crossed(left.rate, taps).ok_or(Error::SpeakersUnusable)
    }

    pub const fn is_crossed(&self) -> bool {
        self.crossed
    }

    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn channels(&self) -> usize {
        self.taps.len()
    }

    pub fn frames(&self) -> usize {
        self.taps.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub fn loudest_gain(&self) -> f64 {
        let size = smooth_length_from(self.frames().max(1) * SPECTRUM_POINTS_A_TAP);
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(size);
        let mut window = fft.make_input_vec();
        let mut scratch = fft.make_scratch_vec();
        let mut spectra: Vec<Vec<Complex<f64>>> = Vec::with_capacity(self.taps.len());
        for channel in &self.taps {
            window.fill(0.0);
            for (slot, tap) in window.iter_mut().zip(channel) {
                *slot = *tap;
            }
            let mut spectrum = fft.make_output_vec();
            if fft
                .process_with_scratch(&mut window, &mut spectrum, &mut scratch)
                .is_err()
            {
                spectrum.fill(Complex::default());
            }
            spectra.push(spectrum);
        }
        if self.crossed {
            return crossed_gain(&spectra);
        }
        spectra
            .iter()
            .flatten()
            .map(Complex::norm_sqr)
            .fold(0.0_f64, f64::max)
            .sqrt()
    }

    #[must_use]
    pub fn with_headroom(self) -> Self {
        let loudest = self.loudest_gain();
        if !loudest.is_finite() || loudest <= 1.0 {
            return self;
        }
        let scale = loudest.recip();
        Self {
            rate: self.rate,
            taps: self
                .taps
                .into_iter()
                .map(|channel| channel.into_iter().map(|tap| tap * scale).collect())
                .collect(),
            crossed: self.crossed,
            at_rates: Mutex::new(Vec::new()),
        }
    }

    pub fn heard_at(&self, hertz: &[f64], rate: SampleRate) -> Result<Vec<Vec<(f64, f64)>>> {
        let taps = self.at(rate)?;
        let turns: Vec<(f64, f64)> = hertz
            .iter()
            .map(|hertz| {
                let angle = std::f64::consts::TAU * hertz / f64::from(rate.hz());
                let (imaginary, real) = angle.sin_cos();
                (real, imaginary)
            })
            .collect();
        let mut heard = vec![Vec::with_capacity(taps.len()); hertz.len()];
        for channel in taps.iter() {
            for (block, turns) in turns.chunks(POINTS_A_BLOCK).enumerate() {
                let sums = summed_turns(channel, turns);
                for (at, sum) in sums.into_iter().enumerate() {
                    heard[block * POINTS_A_BLOCK + at].push(sum);
                }
            }
        }
        Ok(heard)
    }

    pub fn at(&self, rate: SampleRate) -> Result<Taps> {
        if rate == self.rate {
            return Ok(Arc::from(self.taps.clone()));
        }
        if let Some((_, held)) = self.at_rates.lock().iter().find(|(held, _)| *held == rate) {
            return Ok(Arc::clone(held));
        }
        let resampled_frames =
            (self.frames() as u64 * u64::from(rate.hz())).div_ceil(u64::from(self.rate.hz()));
        refused_over(resampled_frames * self.taps.len() as u64 * BYTES_A_TAP)?;
        let resampled: Taps = self
            .taps
            .iter()
            .map(|channel| resampled(channel, self.rate, rate))
            .collect::<Result<Vec<_>>>()?
            .into();
        self.at_rates.lock().push((rate, Arc::clone(&resampled)));
        Ok(resampled)
    }
}

fn crossed_gain(spectra: &[Vec<Complex<f64>>]) -> f64 {
    let [left_to_left, left_to_right, right_to_left, right_to_right] = spectra else {
        return 0.0;
    };
    left_to_left
        .iter()
        .zip(left_to_right)
        .zip(right_to_left.iter().zip(right_to_right))
        .map(|((ll, lr), (rl, rr))| (ll.norm() + rl.norm()).max(lr.norm() + rr.norm()))
        .fold(0.0_f64, f64::max)
}

fn summed_turns(taps: &[f64], steps: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let mut turned_real = vec![1.0; steps.len()];
    let mut turned_imaginary = vec![0.0; steps.len()];
    let mut real = vec![0.0; steps.len()];
    let mut imaginary = vec![0.0; steps.len()];
    let step_real: Vec<f64> = steps.iter().map(|step| step.0).collect();
    let step_imaginary: Vec<f64> = steps.iter().map(|step| step.1).collect();

    for tap in taps {
        for (((real, imaginary), turned_real), turned_imaginary) in real
            .iter_mut()
            .zip(imaginary.iter_mut())
            .zip(turned_real.iter_mut())
            .zip(turned_imaginary.iter_mut())
        {
            *real += tap * *turned_real;
            *imaginary += tap * *turned_imaginary;
        }
        for (((turned_real, turned_imaginary), step_real), step_imaginary) in turned_real
            .iter_mut()
            .zip(turned_imaginary.iter_mut())
            .zip(&step_real)
            .zip(&step_imaginary)
        {
            let next_real = *turned_real * step_real - *turned_imaginary * step_imaginary;
            *turned_imaginary = *turned_real * step_imaginary + *turned_imaginary * step_real;
            *turned_real = next_real;
        }
    }
    real.into_iter().zip(imaginary).collect()
}

fn resampled(taps: &[f64], from: SampleRate, to: SampleRate) -> Result<Vec<f64>> {
    let mut resampler = Resampler::new(ResamplerConfig {
        input_rate: from,
        output_rate: to,
        channels: ChannelLayout::Mono,
        quality: Quality::VeryHigh,
        phase: FilterPhase::Linear,
        max_frames_in: RESAMPLED_A_BLOCK_AT_A_TIME,
    })?;
    let spec = StreamSpec::new(from, ChannelLayout::Mono, resonate_core::SampleFormat::F32);
    let most = resampler.prepare(spec, RESAMPLED_A_BLOCK_AT_A_TIME)?;
    let mut out = vec![0.0; most.max(resampler.max_flush_frames())];
    let mut written = Vec::new();
    let trailing = std::iter::repeat_n(0.0, 2 * RESAMPLED_A_BLOCK_AT_A_TIME);
    let padded: Vec<f64> = taps.iter().copied().chain(trailing).collect();
    for block in padded.chunks(RESAMPLED_A_BLOCK_AT_A_TIME) {
        let counted = resampler.process(block, &mut out);
        written.extend_from_slice(&out[..counted.frames_out]);
    }
    let scale = f64::from(from.hz()) / f64::from(to.hz());
    let length = (taps.len() as u64 * u64::from(to.hz())).div_ceil(u64::from(from.hz())) as usize;
    Ok(written
        .into_iter()
        .take(length)
        .map(|tap| tap * scale)
        .collect())
}

type Spectra = Arc<[Vec<Complex<f64>>]>;

struct Channel {
    partition: usize,
    filter: Spectra,
    history: Vec<Vec<Complex<f64>>>,
    head: usize,
    previous: Vec<f64>,
    current: Vec<f64>,
    ready: VecDeque<f64>,
}

impl Channel {
    fn of(filter: Spectra, partition: usize) -> Self {
        let mut channel = Self {
            partition,
            history: vec![vec![Complex::default(); partition + 1]; filter.len()],
            filter,
            head: 0,
            previous: vec![0.0; partition],
            current: Vec::with_capacity(partition),
            ready: VecDeque::with_capacity(2 * partition),
        };
        channel.primed();
        channel
    }

    fn primed(&mut self) {
        for spectrum in &mut self.history {
            spectrum.fill(Complex::default());
        }
        self.head = 0;
        self.previous.fill(0.0);
        self.current.clear();
        self.ready.clear();
        self.ready.extend(std::iter::repeat_n(0.0, self.partition));
    }

    fn take(&mut self, sample: f64, convolving: &mut Convolving) {
        self.current.push(sample);
        if self.current.len() == self.partition {
            self.convolved(convolving);
        }
    }

    fn convolved(&mut self, convolving: &mut Convolving) {
        let Some(spectrum) = self.history.get_mut(self.head) else {
            return;
        };
        convolving.forward(self.previous.iter().chain(&self.current).copied(), spectrum);

        convolving.summed.fill(Complex::default());
        let (newer, older) = self.history.split_at(self.head + 1);
        let newest_first = newer.iter().rev().chain(older.iter().rev());
        for (held, filter) in newest_first.zip(self.filter.iter()) {
            for ((sum, input), tap) in convolving.summed.iter_mut().zip(held).zip(filter) {
                *sum += input * tap;
            }
        }
        convolving.inverse_into(&mut self.ready, self.partition);

        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
        self.head = if self.head + 1 == self.history.len() {
            0
        } else {
            self.head + 1
        };
    }
}

struct Convolving {
    fft: Arc<dyn RealToComplex<f64>>,
    ifft: Arc<dyn ComplexToReal<f64>>,
    forward_scratch: Vec<Complex<f64>>,
    inverse_scratch: Vec<Complex<f64>>,
    window: Vec<f64>,
    summed: Vec<Complex<f64>>,
    time: Vec<f64>,
}

pub struct Convolver {
    impulse: Arc<Impulse>,
    partition: usize,
    convolving: Convolving,
    channels: Vec<Channel>,
    width: usize,
    crossed: bool,
    tail: usize,
}

impl Convolving {
    fn planned(partition: usize) -> Self {
        let mut planner = RealFftPlanner::new();
        let fft = planner.plan_fft_forward(2 * partition);
        let ifft = planner.plan_fft_inverse(2 * partition);
        Self {
            forward_scratch: fft.make_scratch_vec(),
            inverse_scratch: ifft.make_scratch_vec(),
            window: fft.make_input_vec(),
            summed: fft.make_output_vec(),
            time: ifft.make_output_vec(),
            fft,
            ifft,
        }
    }

    fn forward(&mut self, samples: impl Iterator<Item = f64>, spectrum: &mut [Complex<f64>]) {
        self.window.fill(0.0);
        for (slot, sample) in self.window.iter_mut().zip(samples) {
            *slot = sample;
        }
        if self
            .fft
            .process_with_scratch(&mut self.window, spectrum, &mut self.forward_scratch)
            .is_err()
        {
            spectrum.fill(Complex::default());
        }
    }

    fn inverse_into(&mut self, ready: &mut VecDeque<f64>, partition: usize) {
        let scale = 1.0 / (2 * partition) as f64;
        if self
            .ifft
            .process_with_scratch(&mut self.summed, &mut self.time, &mut self.inverse_scratch)
            .is_err()
        {
            self.time.fill(0.0);
        }
        ready.extend(
            self.time
                .iter()
                .skip(partition)
                .map(|sample| sample * scale),
        );
    }

    fn filter_of(&mut self, taps: &[f64], partition: usize) -> Spectra {
        let partitions = taps.len().div_ceil(partition).max(1);
        (0..partitions)
            .map(|nth| {
                let mut spectrum = vec![Complex::default(); partition + 1];
                self.forward(
                    taps.iter().skip(nth * partition).take(partition).copied(),
                    &mut spectrum,
                );
                spectrum
            })
            .collect()
    }
}

fn spectra_bytes(taps: &[Vec<f64>], channels: usize, partition: usize) -> u64 {
    let longest = taps.iter().map(Vec::len).max().unwrap_or(0);
    let partitions = longest.div_ceil(partition).max(1) as u64;
    let held = taps.len().min(channels) as u64 + channels as u64;
    held.saturating_mul(partitions)
        .saturating_mul(partition as u64 + 1)
        .saturating_mul(BYTES_A_BIN)
}

impl Convolver {
    pub fn new(impulse: Arc<Impulse>) -> Self {
        let partition = PARTITION_AT_48_KHZ;
        Self {
            convolving: Convolving::planned(partition),
            partition,
            impulse,
            channels: Vec::new(),
            width: 0,
            crossed: false,
            tail: 0,
        }
    }

    fn crosses(&self, spec: StreamSpec) -> bool {
        self.impulse.is_crossed() && spec.channels == ChannelLayout::Stereo
    }
}

impl Processor for Convolver {
    fn prepare(&mut self, spec: StreamSpec, max_frames_in: usize) -> Result<usize> {
        let taps = self.impulse.at(spec.rate)?;
        let partition = partition_frames_at(spec.rate);
        if partition != self.partition {
            self.partition = partition;
            self.convolving = Convolving::planned(partition);
        }
        self.width = usize::from(spec.channel_count().get());
        self.crossed = self.crosses(spec);
        let count = if self.crossed {
            CROSSED_RESPONSES
        } else {
            self.width
        };
        refused_over(spectra_bytes(&taps, count, partition))?;
        let filters: Vec<Spectra> = taps
            .iter()
            .take(count)
            .map(|channel| self.convolving.filter_of(channel, partition))
            .collect();
        self.channels = (0..count)
            .filter_map(|channel| filters.get(channel % filters.len().max(1)))
            .map(|filter| Channel::of(Arc::clone(filter), partition))
            .collect();
        self.tail = taps.iter().map(Vec::len).max().unwrap_or(0) + partition;
        Ok(max_frames_in)
    }

    fn max_flush_frames(&self) -> usize {
        self.tail
    }

    fn reset(&mut self) {
        for channel in &mut self.channels {
            channel.primed();
        }
    }

    fn latency_frames(&self) -> f64 {
        self.partition as f64
    }

    fn is_transparent(&self) -> bool {
        false
    }

    fn is_carried_across_a_reshape(&self) -> bool {
        true
    }

    fn process(&mut self, input: &[f64], output: &mut [f64]) -> ProcessCount {
        let count = self.width.max(1);
        let frames = input.len() / count;
        let room = output.len() / count;
        let frames = frames.min(room);
        for (frame, out) in input
            .chunks_exact(count)
            .zip(output.chunks_exact_mut(count))
            .take(frames)
        {
            if self.crossed {
                self.crossed_frame(frame, out);
            } else {
                for ((channel, sample), out) in self.channels.iter_mut().zip(frame).zip(out) {
                    channel.take(*sample, &mut self.convolving);
                    *out = channel.ready.pop_front().unwrap_or(0.0);
                }
            }
        }
        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let count = self.width.max(1);
        let frames = (output.len() / count).min(self.tail);
        for out in output.chunks_exact_mut(count).take(frames) {
            if self.crossed {
                self.crossed_frame(&[0.0, 0.0], out);
            } else {
                for (channel, out) in self.channels.iter_mut().zip(out) {
                    channel.take(0.0, &mut self.convolving);
                    *out = channel.ready.pop_front().unwrap_or(0.0);
                }
            }
        }
        self.tail -= frames;
        frames
    }
}

impl Convolver {
    fn crossed_frame(&mut self, frame: &[f64], out: &mut [f64]) {
        out.fill(0.0);
        for (path, channel) in self.channels.iter_mut().enumerate() {
            let heard = frame.get(path / 2).copied().unwrap_or(0.0);
            channel.take(heard, &mut self.convolving);
            let produced = channel.ready.pop_front().unwrap_or(0.0);
            if let Some(slot) = out.get_mut(path % 2) {
                *slot += produced;
            }
        }
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

    fn convolved(taps: Vec<Vec<f64>>, input: &[f64], block: usize) -> Vec<f64> {
        let impulse = Arc::new(Impulse::new(RATE, taps).expect("an impulse"));
        let mut stage = Convolver::new(impulse);
        stage.prepare(stereo(), block).expect("a prepared stage");
        let mut written = Vec::new();
        let mut out = vec![0.0; block * 2];
        for chunk in input.chunks(block * 2) {
            let counted = stage.process(chunk, &mut out);
            written.extend_from_slice(&out[..counted.frames_out * 2]);
        }
        let mut tail = vec![0.0; stage.max_flush_frames() * 2];
        let flushed = stage.flush(&mut tail);
        written.extend_from_slice(&tail[..flushed * 2]);
        written
    }

    fn noise(frames: usize) -> Vec<f64> {
        let mut seed = 0x5eed_u64;
        (0..frames * 2)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                (seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5
            })
            .collect()
    }

    #[test]
    fn a_unit_impulse_hands_the_input_back_a_partition_late() {
        let input = noise(5_000);
        let written = convolved(vec![vec![1.0]], &input, 700);

        let late = partition_frames_at(RATE) * 2;
        for (at, sample) in input.iter().enumerate() {
            assert!((written[late + at] - sample).abs() < 1e-9, "sample {at}");
        }
        assert!(written[..late].iter().all(|sample| sample.abs() < 1e-12));
    }

    #[test]
    fn a_long_response_convolves_as_the_direct_sum_does_and_each_channel_takes_its_own() {
        let frames = 6_000;
        let input = noise(frames);
        let left: Vec<f64> = (0..3_000)
            .map(|at| (-(at as f64) / 400.0).exp() * 0.3)
            .collect();
        let right: Vec<f64> = vec![0.0, 0.0, 0.5, -0.25];
        let written = convolved(vec![left.clone(), right.clone()], &input, 1_024);

        for (channel, taps) in [(0_usize, &left), (1, &right)] {
            for at in (0..frames).step_by(97) {
                let direct: f64 = taps
                    .iter()
                    .enumerate()
                    .filter(|(lag, _)| *lag <= at)
                    .map(|(lag, tap)| tap * input[(at - lag) * 2 + channel])
                    .sum();
                let heard = written[(partition_frames_at(RATE) + at) * 2 + channel];
                assert!(
                    (heard - direct).abs() < 1e-9,
                    "channel {channel} frame {at}: {heard} against {direct}"
                );
            }
        }
    }

    #[test]
    fn the_response_heard_at_a_frequency_is_the_sum_of_its_taps_turned_by_the_frequency() {
        let taps = vec![0.5, 0.25, -0.125, 0.0625];
        let impulse = Impulse::new(RATE, vec![taps.clone(), vec![1.0]]).expect("an impulse");
        let hertz = [0.0, 100.0, 5_000.0, 23_000.0];

        let heard = impulse.heard_at(&hertz, RATE).expect("a response");

        assert_eq!(heard.len(), hertz.len());
        for (at, hertz) in hertz.iter().enumerate() {
            let angle = std::f64::consts::TAU * hertz / f64::from(RATE.hz());
            let direct =
                taps.iter()
                    .enumerate()
                    .fold((0.0, 0.0), |(real, imaginary), (lag, tap)| {
                        (
                            real + tap * (angle * lag as f64).cos(),
                            imaginary + tap * (angle * lag as f64).sin(),
                        )
                    });
            assert!((heard[at][0].0 - direct.0).abs() < 1e-12, "{hertz} Hz");
            assert!((heard[at][0].1 - direct.1).abs() < 1e-12, "{hertz} Hz");
            assert_eq!(heard[at][1], (1.0, 0.0));
        }
    }

    #[test]
    fn a_response_longer_than_a_block_of_points_is_heard_at_every_one_of_them() {
        let taps: Vec<f64> = (0..3_000)
            .map(|at| (-(at as f64) / 500.0).exp() * (at as f64 * 0.01).sin())
            .collect();
        let impulse = Impulse::new(RATE, vec![taps.clone()]).expect("an impulse");
        let hertz: Vec<f64> = (0..300).map(|at| 20.0 + at as f64 * 70.0).collect();

        let heard = impulse.heard_at(&hertz, RATE).expect("a response");

        for at in [0, 127, 128, 129, 255, 299] {
            let angle = std::f64::consts::TAU * hertz[at] / f64::from(RATE.hz());
            let direct =
                taps.iter()
                    .enumerate()
                    .fold((0.0, 0.0), |(real, imaginary), (lag, tap)| {
                        (
                            real + tap * (angle * lag as f64).cos(),
                            imaginary + tap * (angle * lag as f64).sin(),
                        )
                    });
            assert!((heard[at][0].0 - direct.0).abs() < 1e-9, "point {at}");
            assert!((heard[at][0].1 - direct.1).abs() < 1e-9, "point {at}");
        }
    }

    #[test]
    fn a_response_holding_a_tap_that_is_no_number_is_no_response() {
        assert!(Impulse::new(RATE, vec![vec![0.5, f64::NAN, 0.25]]).is_none());
        assert!(Impulse::new(RATE, vec![vec![1.0], vec![f64::INFINITY]]).is_none());
    }

    #[test]
    fn headroom_brings_a_boosting_response_down_to_unity_and_leaves_a_quiet_one_alone() {
        let boosted = Impulse::new(RATE, vec![vec![0.5, 1.0, 0.5], vec![0.25]])
            .expect("an impulse")
            .with_headroom();
        let quiet = Impulse::new(RATE, vec![vec![0.5, -0.25]])
            .expect("an impulse")
            .with_headroom();

        assert!((boosted.loudest_gain() - 1.0).abs() < 1e-9);
        let taps = boosted.at(RATE).expect("the response at its own rate");
        assert!((taps[0][1] - 0.5).abs() < 1e-9, "{:?}", taps[0]);
        assert!(
            (taps[1][0] - 0.125).abs() < 1e-9,
            "the channels lost their balance"
        );
        assert_eq!(quiet.at(RATE).expect("the response")[0], vec![0.5, -0.25]);
    }

    #[test]
    fn a_partition_grows_with_the_rate_so_its_latency_stays_near_a_fiftieth_of_a_second() {
        assert_eq!(partition_frames_at(SampleRate::HZ_44100), 1_024);
        assert_eq!(partition_frames_at(RATE), 1_024);
        assert_eq!(partition_frames_at(SampleRate::HZ_96000), 2_048);
        assert_eq!(partition_frames_at(SampleRate::HZ_192000), 4_096);
    }

    #[test]
    fn a_response_taken_at_another_rate_keeps_its_peak_where_it_was_in_time() {
        let mut taps = vec![0.0; 4_410];
        taps[441] = 1.0;
        let impulse = Impulse::new(SampleRate::HZ_44100, vec![taps]).expect("an impulse");

        let at_ours = impulse.at(RATE).expect("a response resampled");
        let peak = at_ours[0]
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.abs().total_cmp(&b.abs()))
            .map(|(at, _)| at);
        assert_eq!(peak, Some(480), "the response moved in time");
    }

    #[test]
    fn a_response_taken_at_another_rate_is_drawn_again_at_the_streams() {
        let impulse =
            Impulse::new(SampleRate::HZ_96000, vec![vec![1.0; 9_600]]).expect("an impulse");
        let at_ours = impulse.at(RATE).expect("a response resampled");
        let length = at_ours[0].len();
        assert!((4_700..=4_900).contains(&length), "{length}");
        assert!(Arc::ptr_eq(
            &at_ours,
            &impulse.at(RATE).expect("the same response")
        ));
    }

    #[test]
    fn a_mono_response_holds_one_spectrum_for_every_channel() {
        let impulse = Arc::new(Impulse::new(RATE, vec![vec![0.5; 3_000]]).expect("an impulse"));
        let mut stage = Convolver::new(impulse);
        stage.prepare(stereo(), 512).expect("a prepared stage");

        assert_eq!(stage.channels.len(), 2);
        assert!(Arc::ptr_eq(
            &stage.channels[0].filter,
            &stage.channels[1].filter
        ));
    }

    #[test]
    fn a_stereo_response_keeps_a_spectrum_a_channel() {
        let impulse = Arc::new(
            Impulse::new(RATE, vec![vec![0.5; 3_000], vec![0.25; 3_000]]).expect("an impulse"),
        );
        let mut stage = Convolver::new(impulse);
        stage.prepare(stereo(), 512).expect("a prepared stage");

        assert!(!Arc::ptr_eq(
            &stage.channels[0].filter,
            &stage.channels[1].filter
        ));
    }

    #[test]
    fn a_response_whose_spectra_outgrow_the_bound_is_refused_before_any_is_made() {
        let frames = RESPONSE_BYTES_AT_MOST as usize / (3 * 1_025 * 16) * 1_024 * 6 / 5;
        let impulse = Arc::new(Impulse::new(RATE, vec![vec![0.0; frames]]).expect("an impulse"));
        let mut stage = Convolver::new(impulse);

        let refused = stage
            .prepare(stereo(), 512)
            .expect_err("a response too large");
        assert!(matches!(
            refused,
            Error::ResponseTooLarge { bytes, at_most }
                if bytes > at_most && at_most == RESPONSE_BYTES_AT_MOST
        ));
    }

    #[test]
    fn a_response_redrawn_beyond_the_bound_is_refused_before_it_is_resampled() {
        let impulse =
            Impulse::new(SampleRate::HZ_48000, vec![vec![0.0; 5_000_000]; 4]).expect("an impulse");

        let refused = impulse.at(SampleRate::HZ_192000).expect_err("too large");
        assert!(matches!(refused, Error::ResponseTooLarge { .. }));
    }

    #[test]
    fn the_loudest_gain_is_the_peak_of_the_magnitude_response() {
        let flat = Impulse::new(RATE, vec![vec![1.0]]).expect("an impulse");
        let doubled = Impulse::new(RATE, vec![vec![0.5, 0.5]]).expect("an impulse");
        let alternating =
            Impulse::new(RATE, vec![vec![1.0, -1.0], vec![0.25]]).expect("an impulse");

        assert!((flat.loudest_gain() - 1.0).abs() < 1e-9);
        assert!((doubled.loudest_gain() - 1.0).abs() < 1e-9);
        assert!((alternating.loudest_gain() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_spectrum_grid_is_a_short_even_length_of_small_factors() {
        for least in [1, 2, 7, 4_001, 1_000_003, 7_680_000] {
            let length = smooth_length_from(least);
            let mut rest = length;
            for factor in [2, 3, 5] {
                while rest.is_multiple_of(factor) {
                    rest /= factor;
                }
            }
            assert!(
                length >= least && length.is_multiple_of(2) && rest == 1,
                "{least}: {length}"
            );
            assert!(length <= least.max(2) * 11 / 10 + 4, "{least}: {length}");
        }
    }

    fn crossed(taps: [Vec<f64>; 4], input: &[f64], block: usize) -> Vec<f64> {
        let impulse = Arc::new(Impulse::crossed(RATE, taps.to_vec()).expect("a crossed impulse"));
        let mut stage = Convolver::new(impulse);
        stage.prepare(stereo(), block).expect("a prepared stage");
        let mut written = Vec::new();
        let mut out = vec![0.0; block * 2];
        for chunk in input.chunks(block * 2) {
            let counted = stage.process(chunk, &mut out);
            written.extend_from_slice(&out[..counted.frames_out * 2]);
        }
        let mut tail = vec![0.0; stage.max_flush_frames() * 2];
        let flushed = stage.flush(&mut tail);
        written.extend_from_slice(&tail[..flushed * 2]);
        written
    }

    #[test]
    fn a_two_by_two_identity_response_is_transparent() {
        let input = noise(5_000);
        let written = crossed([vec![1.0], vec![0.0], vec![0.0], vec![1.0]], &input, 700);

        let late = partition_frames_at(RATE) * 2;
        for (at, sample) in input.iter().enumerate() {
            assert!((written[late + at] - sample).abs() < 1e-9, "sample {at}");
        }
    }

    fn speaker(rate: SampleRate, near: Vec<f64>, far: Vec<f64>) -> Impulse {
        Impulse::new(rate, vec![near, far]).expect("a stereo response")
    }

    #[test]
    fn two_stereo_responses_are_assembled_left_speaker_first() {
        let left = speaker(RATE, vec![0.9, 0.1], vec![0.3, 0.0]);
        let right = speaker(RATE, vec![0.2, 0.0], vec![0.8, 0.05]);

        let pair = Impulse::binaural(&left, &right).expect("a pair");
        let whole = Impulse::crossed(
            RATE,
            vec![
                vec![0.9, 0.1],
                vec![0.3, 0.0],
                vec![0.2, 0.0],
                vec![0.8, 0.05],
            ],
        )
        .expect("four responses");

        assert!(pair.is_crossed());
        assert_eq!(pair, whole);
    }

    #[test]
    fn a_pair_that_is_not_two_stereo_responses_of_one_rate_and_length_is_refused() {
        let stereo = speaker(RATE, vec![1.0, 0.5], vec![0.0, 0.0]);
        let mono = Impulse::new(RATE, vec![vec![1.0, 0.5]]).expect("a mono response");
        let other_rate = speaker(SampleRate::HZ_44100, vec![1.0, 0.5], vec![0.0, 0.0]);
        let longer = speaker(RATE, vec![1.0, 0.5, 0.25], vec![0.0, 0.0, 0.0]);
        let crossed = Impulse::crossed(RATE, vec![vec![1.0]; 4]).expect("four responses");

        assert_eq!(
            Impulse::binaural(&mono, &stereo),
            Err(Error::SpeakerNotStereo {
                speaker: Speaker::Left,
                channels: 1
            })
        );
        assert_eq!(
            Impulse::binaural(&stereo, &mono),
            Err(Error::SpeakerNotStereo {
                speaker: Speaker::Right,
                channels: 1
            })
        );
        assert_eq!(
            Impulse::binaural(&stereo, &other_rate),
            Err(Error::SpeakerRatesDiffer {
                left: RATE,
                right: SampleRate::HZ_44100
            })
        );
        assert_eq!(
            Impulse::binaural(&stereo, &longer),
            Err(Error::SpeakerLengthsDiffer { left: 2, right: 3 })
        );
        assert_eq!(
            Impulse::binaural(&stereo, &crossed),
            Err(Error::SpeakerAlreadyCrossed {
                speaker: Speaker::Right
            })
        );
    }

    #[test]
    fn a_pure_cross_response_swaps_the_channels() {
        let input = noise(5_000);
        let written = crossed([vec![0.0], vec![1.0], vec![1.0], vec![0.0]], &input, 700);

        let late = partition_frames_at(RATE) * 2;
        for frame in 0..5_000 {
            assert!((written[late + frame * 2] - input[frame * 2 + 1]).abs() < 1e-9);
            assert!((written[late + frame * 2 + 1] - input[frame * 2]).abs() < 1e-9);
        }
    }

    #[test]
    fn a_two_by_two_response_sums_each_ear_from_both_inputs_as_the_direct_sum_does() {
        let frames = 4_000;
        let input = noise(frames);
        let taps = [
            vec![0.9, 0.1, 0.05],
            vec![0.0, 0.0, 0.3, -0.1],
            vec![0.0, 0.2, 0.0, 0.0, 0.1],
            vec![0.8, 0.0, -0.05],
        ];
        let written = crossed(taps.clone(), &input, 512);

        for at in (0..frames).step_by(61) {
            for ear in 0..2 {
                let direct: f64 = (0..2)
                    .map(|heard| {
                        taps[heard * 2 + ear]
                            .iter()
                            .enumerate()
                            .filter(|(lag, _)| *lag <= at)
                            .map(|(lag, tap)| tap * input[(at - lag) * 2 + heard])
                            .sum::<f64>()
                    })
                    .sum();
                let heard = written[(partition_frames_at(RATE) + at) * 2 + ear];
                assert!((heard - direct).abs() < 1e-9, "ear {ear} frame {at}");
            }
        }
    }

    #[test]
    fn headroom_holds_a_crossed_response_to_what_both_inputs_can_add_up_to() {
        let summing = Impulse::crossed(RATE, vec![vec![1.0], vec![0.0], vec![1.0], vec![0.0]])
            .expect("an impulse")
            .with_headroom();

        assert!((summing.loudest_gain() - 1.0).abs() < 1e-9);
        assert!(summing.is_crossed());
        let taps = summing.at(RATE).expect("the response");
        assert!((taps[0][0] - 0.5).abs() < 1e-9);
    }

    #[test]
    fn only_four_responses_make_a_crossed_one() {
        assert!(Impulse::crossed(RATE, vec![vec![1.0]; 3]).is_none());
        assert!(Impulse::crossed(RATE, vec![vec![1.0]; 5]).is_none());
        assert!(
            !Impulse::new(RATE, vec![vec![1.0]; 4])
                .expect("an impulse")
                .is_crossed()
        );
    }

    #[test]
    fn a_crossed_response_on_a_stream_that_is_not_stereo_is_taken_a_channel_each() {
        let impulse = Arc::new(Impulse::crossed(RATE, vec![vec![1.0]; 4]).expect("an impulse"));
        let mut stage = Convolver::new(impulse);
        let quad = StreamSpec::new(RATE, ChannelLayout::Quad, resonate_core::SampleFormat::F32);

        stage.prepare(quad, 512).expect("a prepared stage");

        assert_eq!(stage.channels.len(), 4);
        assert!(!stage.crossed);
    }
}
