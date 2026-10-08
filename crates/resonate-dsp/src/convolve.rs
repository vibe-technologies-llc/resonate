use std::{collections::VecDeque, sync::Arc};

use parking_lot::Mutex;
use resonate_core::{ChannelLayout, SampleRate, StreamSpec};
use rustfft::{Fft, FftPlanner, num_complex::Complex};

use crate::{FilterPhase, ProcessCount, Processor, Quality, Resampler, ResamplerConfig, Result};

const PARTITION_AT_48_KHZ: usize = 1_024;
const FRAMES_A_PARTITION_IS_FOR: u32 = 48_000;

pub fn partition_frames_at(rate: SampleRate) -> usize {
    let scaled = (rate.hz() / FRAMES_A_PARTITION_IS_FOR).max(1) as usize;
    PARTITION_AT_48_KHZ * scaled.next_power_of_two()
}
const RESAMPLED_A_BLOCK_AT_A_TIME: usize = 4_096;
const SPECTRUM_POINTS_A_TAP: usize = 4;

pub type Taps = Arc<[Vec<f64>]>;

#[derive(Debug)]
pub struct Impulse {
    rate: SampleRate,
    taps: Vec<Vec<f64>>,
    at_rates: Mutex<Vec<(SampleRate, Taps)>>,
}

impl PartialEq for Impulse {
    fn eq(&self, other: &Self) -> bool {
        self.rate == other.rate && self.taps == other.taps
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
            at_rates: Mutex::new(Vec::new()),
        })
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
        let longest = self.frames().max(1);
        let size = (longest * SPECTRUM_POINTS_A_TAP).next_power_of_two();
        let fft = FftPlanner::new().plan_fft_forward(size);
        let mut spectrum = vec![Complex::default(); size];
        let mut loudest = 0.0_f64;
        for channel in &self.taps {
            spectrum.fill(Complex::default());
            for (slot, tap) in spectrum.iter_mut().zip(channel) {
                *slot = Complex::new(*tap, 0.0);
            }
            fft.process(&mut spectrum);
            loudest = spectrum
                .iter()
                .take(size / 2 + 1)
                .map(|bin| bin.norm())
                .fold(loudest, f64::max);
        }
        loudest
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
            at_rates: Mutex::new(Vec::new()),
        }
    }

    pub fn at(&self, rate: SampleRate) -> Result<Taps> {
        if rate == self.rate {
            return Ok(Arc::from(self.taps.clone()));
        }
        if let Some((_, held)) = self.at_rates.lock().iter().find(|(held, _)| *held == rate) {
            return Ok(Arc::clone(held));
        }
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

struct Channel {
    partition: usize,
    filter: Vec<Vec<Complex<f64>>>,
    history: Vec<Vec<Complex<f64>>>,
    head: usize,
    previous: Vec<f64>,
    current: Vec<f64>,
    ready: VecDeque<f64>,
}

impl Channel {
    fn of(
        taps: &[f64],
        partition: usize,
        fft: &dyn Fft<f64>,
        scratch: &mut [Complex<f64>],
    ) -> Self {
        let partitions = taps.len().div_ceil(partition).max(1);
        let filter = (0..partitions)
            .map(|nth| {
                let mut spectrum = vec![Complex::default(); 2 * partition];
                for (slot, tap) in spectrum
                    .iter_mut()
                    .zip(taps.iter().skip(nth * partition).take(partition))
                {
                    *slot = Complex::new(*tap, 0.0);
                }
                fft.process_with_scratch(&mut spectrum, scratch);
                spectrum
            })
            .collect::<Vec<_>>();
        let mut channel = Self {
            partition,
            history: vec![vec![Complex::default(); 2 * partition]; filter.len()],
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
        let Convolving {
            fft,
            ifft,
            scratch,
            summed,
        } = convolving;
        let spectrum = &mut self.history[self.head];
        for (slot, sample) in spectrum
            .iter_mut()
            .zip(self.previous.iter().chain(self.current.iter()))
        {
            *slot = Complex::new(*sample, 0.0);
        }
        fft.process_with_scratch(spectrum, scratch);

        summed.fill(Complex::default());
        let partitions = self.filter.len();
        for (nth, filter) in self.filter.iter().enumerate() {
            let held = &self.history[(self.head + partitions - nth) % partitions];
            for ((sum, input), tap) in summed.iter_mut().zip(held).zip(filter) {
                *sum += input * tap;
            }
        }
        ifft.process_with_scratch(summed, scratch);
        let scale = 1.0 / (2 * self.partition) as f64;
        self.ready.extend(
            summed[self.partition..]
                .iter()
                .map(|sample| sample.re * scale),
        );

        std::mem::swap(&mut self.previous, &mut self.current);
        self.current.clear();
        self.head = (self.head + 1) % partitions;
    }
}

struct Convolving {
    fft: Arc<dyn Fft<f64>>,
    ifft: Arc<dyn Fft<f64>>,
    scratch: Vec<Complex<f64>>,
    summed: Vec<Complex<f64>>,
}

pub struct Convolver {
    impulse: Arc<Impulse>,
    partition: usize,
    convolving: Convolving,
    channels: Vec<Channel>,
    tail: usize,
}

impl Convolving {
    fn planned(partition: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(2 * partition);
        let ifft = planner.plan_fft_inverse(2 * partition);
        let scratch = vec![
            Complex::default();
            fft.get_inplace_scratch_len()
                .max(ifft.get_inplace_scratch_len())
        ];
        Self {
            fft,
            ifft,
            scratch,
            summed: vec![Complex::default(); 2 * partition],
        }
    }
}

impl Convolver {
    pub fn new(impulse: Arc<Impulse>) -> Self {
        let partition = PARTITION_AT_48_KHZ;
        Self {
            convolving: Convolving::planned(partition),
            partition,
            impulse,
            channels: Vec::new(),
            tail: 0,
        }
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
        let count = usize::from(spec.channel_count().get());
        let fft = Arc::clone(&self.convolving.fft);
        let scratch = &mut self.convolving.scratch;
        self.channels = (0..count)
            .map(|channel| {
                Channel::of(
                    &taps[channel % taps.len()],
                    partition,
                    fft.as_ref(),
                    scratch,
                )
            })
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
        let count = self.channels.len().max(1);
        let frames = input.len() / count;
        let room = output.len() / count;
        let frames = frames.min(room);
        for (frame, out) in input
            .chunks_exact(count)
            .zip(output.chunks_exact_mut(count))
            .take(frames)
        {
            for ((channel, sample), out) in self.channels.iter_mut().zip(frame).zip(out) {
                channel.take(*sample, &mut self.convolving);
                *out = channel.ready.pop_front().unwrap_or(0.0);
            }
        }
        ProcessCount {
            frames_in: frames,
            frames_out: frames,
        }
    }

    fn flush(&mut self, output: &mut [f64]) -> usize {
        let count = self.channels.len().max(1);
        let frames = (output.len() / count).min(self.tail);
        for out in output.chunks_exact_mut(count).take(frames) {
            for (channel, out) in self.channels.iter_mut().zip(out) {
                channel.take(0.0, &mut self.convolving);
                *out = channel.ready.pop_front().unwrap_or(0.0);
            }
        }
        self.tail -= frames;
        frames
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
}
