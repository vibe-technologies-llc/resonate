use std::{ops::Range, sync::Arc};

use parking_lot::Mutex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex, num_complex::Complex};

const PARTITION: usize = 256;
const WIDE_PARTITION: usize = 512;
const WIDE_PARTITION_FROM_HZ: u64 = 176_400;
const KEPT_SPECTRA_BYTES_AT_MOST: usize = 4 << 20;

pub(crate) const fn partition_at(oversampled_hz: u64) -> usize {
    if oversampled_hz >= WIDE_PARTITION_FROM_HZ {
        WIDE_PARTITION
    } else {
        PARTITION
    }
}

pub(crate) struct Spectra {
    real: Vec<f64>,
    imaginary: Vec<f64>,
    partition: usize,
    partitions: usize,
}

impl Spectra {
    fn of(kernel: &[f64], partition: usize) -> Self {
        let points = 2 * partition;
        let bins = partition + 1;
        let partitions = kernel.len().div_ceil(partition).max(1);
        let forward = RealFftPlanner::<f64>::new().plan_fft_forward(points);
        let mut time = vec![0.0; points];
        let mut spectrum = vec![Complex::new(0.0, 0.0); bins];
        let mut scratch = forward.make_scratch_vec();
        let mut real = vec![0.0; partitions * bins];
        let mut imaginary = vec![0.0; partitions * bins];
        let unscaled = (points as f64).recip();

        for ((taps, real), imaginary) in kernel
            .chunks(partition)
            .zip(real.chunks_exact_mut(bins))
            .zip(imaginary.chunks_exact_mut(bins))
        {
            time.fill(0.0);
            for (slot, tap) in time.iter_mut().zip(taps) {
                *slot = tap * unscaled;
            }
            if forward
                .process_with_scratch(&mut time, &mut spectrum, &mut scratch)
                .is_err()
            {
                spectrum.fill(Complex::new(0.0, 0.0));
            }
            for ((bin, real), imaginary) in spectrum.iter().zip(real).zip(imaginary) {
                *real = bin.re;
                *imaginary = bin.im;
            }
        }

        Self {
            real,
            imaginary,
            partition,
            partitions,
        }
    }

    fn bytes(&self) -> usize {
        (self.real.capacity() + self.imaginary.capacity()) * size_of::<f64>()
    }

    pub(crate) fn kept_for<Key: PartialEq + Copy>(
        cache: &Mutex<Option<(Key, Arc<Self>)>>,
        wanted: Key,
        partition: usize,
        kernel: impl FnOnce() -> Vec<f64>,
    ) -> Arc<Self> {
        if let Some((_, kept)) = cache
            .lock()
            .as_ref()
            .filter(|(held, kept)| *held == wanted && kept.partition == partition)
        {
            return Arc::clone(kept);
        }
        let built = Arc::new(Self::of(&kernel(), partition));
        let mut held = cache.lock();
        *held = (built.bytes() <= KEPT_SPECTRA_BYTES_AT_MOST).then(|| (wanted, Arc::clone(&built)));
        built
    }
}

struct Lane {
    recent: Vec<f64>,
    real: Vec<f64>,
    imaginary: Vec<f64>,
    ready: Vec<f64>,
}

impl Lane {
    fn new(spectra: &Spectra, up: usize) -> Self {
        let bins = spectra.partition + 1;
        Self {
            recent: vec![0.0; 2 * spectra.partition / up],
            real: vec![0.0; spectra.partitions * bins],
            imaginary: vec![0.0; spectra.partitions * bins],
            ready: vec![0.0; spectra.partition],
        }
    }

    fn reset(&mut self) {
        self.recent.fill(0.0);
        self.real.fill(0.0);
        self.imaginary.fill(0.0);
        self.ready.fill(0.0);
    }
}

pub(crate) struct Prefilter {
    spectra: Arc<Spectra>,
    up: usize,
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
    lanes: Vec<Lane>,
    compact: Vec<f64>,
    folded: Vec<Complex<f64>>,
    time: Vec<f64>,
    spectrum: Vec<Complex<f64>>,
    summed_real: Vec<f64>,
    summed_imaginary: Vec<f64>,
    forward_scratch: Vec<Complex<f64>>,
    inverse_scratch: Vec<Complex<f64>>,
    gathered: usize,
    newest: usize,
}

impl Prefilter {
    pub(crate) fn new(spectra: Arc<Spectra>, channels: usize, up: usize) -> Self {
        let points = 2 * spectra.partition;
        let gathered_points = points / up;
        let bins = spectra.partition + 1;
        let mut planner = RealFftPlanner::<f64>::new();
        let forward = planner.plan_fft_forward(gathered_points);
        let inverse = planner.plan_fft_inverse(points);
        let forward_scratch = forward.make_scratch_vec();
        let inverse_scratch = inverse.make_scratch_vec();
        let lanes = (0..channels).map(|_| Lane::new(&spectra, up)).collect();

        Self {
            spectra,
            up,
            forward,
            inverse,
            lanes,
            compact: vec![0.0; gathered_points],
            folded: vec![Complex::new(0.0, 0.0); gathered_points / 2 + 1],
            time: vec![0.0; points],
            spectrum: vec![Complex::new(0.0, 0.0); bins],
            summed_real: vec![0.0; bins],
            summed_imaginary: vec![0.0; bins],
            forward_scratch,
            inverse_scratch,
            gathered: 0,
            newest: 0,
        }
    }

    pub(crate) fn partition(&self) -> usize {
        self.spectra.partition
    }

    pub(crate) fn reset(&mut self) {
        for lane in &mut self.lanes {
            lane.reset();
        }
        self.gathered = 0;
        self.newest = 0;
    }

    pub(crate) fn room(&self) -> usize {
        self.spectra.partition - self.gathered
    }

    pub(crate) fn gather(&mut self, input: &[f64], channels: usize, frames: usize) {
        let from = (self.spectra.partition + self.gathered) / self.up;
        for (channel, lane) in self.lanes.iter_mut().enumerate() {
            let Some(run) = lane.recent.get_mut(from..from + frames) else {
                continue;
            };
            for (slot, sample) in run
                .iter_mut()
                .zip(input.iter().skip(channel).step_by(channels))
            {
                *slot = *sample;
            }
        }
    }

    pub(crate) fn gather_silence(&mut self, samples: usize) {
        let from = (self.spectra.partition + self.gathered) / self.up;
        for lane in &mut self.lanes {
            if let Some(run) = lane.recent.get_mut(from..from + samples / self.up) {
                run.fill(0.0);
            }
        }
    }

    pub(crate) fn ready(&self, channel: usize, run: Range<usize>) -> &[f64] {
        let from = self.gathered + run.start;
        let to = self.gathered + run.end;
        self.lanes
            .get(channel)
            .and_then(|lane| lane.ready.get(from..to))
            .unwrap_or_default()
    }

    pub(crate) fn advance(&mut self, samples: usize) {
        self.gathered += samples;
        if self.gathered >= self.spectra.partition {
            self.gathered = 0;
            self.convolve();
        }
    }

    fn convolve(&mut self) {
        let Self {
            spectra,
            up,
            forward,
            inverse,
            lanes,
            compact,
            folded,
            time,
            spectrum,
            summed_real,
            summed_imaginary,
            forward_scratch,
            inverse_scratch,
            newest,
            ..
        } = self;
        let partition = spectra.partition;
        let bins = partition + 1;
        let partitions = spectra.partitions;
        *newest = (*newest + partitions - 1) % partitions;

        for lane in lanes {
            compact.copy_from_slice(&lane.recent);
            if forward
                .process_with_scratch(compact, folded, forward_scratch)
                .is_err()
            {
                folded.fill(Complex::new(0.0, 0.0));
            }
            unfolded(folded, spectrum);
            let at = *newest * bins..(*newest + 1) * bins;
            if let (Some(real), Some(imaginary)) =
                (lane.real.get_mut(at.clone()), lane.imaginary.get_mut(at))
            {
                for ((bin, real), imaginary) in spectrum.iter().zip(real).zip(imaginary) {
                    *real = bin.re;
                    *imaginary = bin.im;
                }
            }

            summed_real.fill(0.0);
            summed_imaginary.fill(0.0);
            let split = *newest * bins;
            let (wrapped_real, leading_real) = lane.real.split_at(split);
            let (wrapped_imaginary, leading_imaginary) = lane.imaginary.split_at(split);
            let past_real = leading_real
                .chunks_exact(bins)
                .chain(wrapped_real.chunks_exact(bins));
            let past_imaginary = leading_imaginary
                .chunks_exact(bins)
                .chain(wrapped_imaginary.chunks_exact(bins));
            let taps = spectra
                .real
                .chunks_exact(bins)
                .zip(spectra.imaginary.chunks_exact(bins));
            for ((heard_real, heard_imaginary), (tap_real, tap_imaginary)) in
                past_real.zip(past_imaginary).zip(taps)
            {
                multiply_into(
                    summed_real,
                    summed_imaginary,
                    [heard_real, heard_imaginary],
                    [tap_real, tap_imaginary],
                );
            }

            for ((bin, real), imaginary) in spectrum
                .iter_mut()
                .zip(summed_real.iter())
                .zip(summed_imaginary.iter())
            {
                *bin = Complex::new(*real, *imaginary);
            }
            if let Some(first) = spectrum.first_mut() {
                first.im = 0.0;
            }
            if let Some(last) = spectrum.last_mut() {
                last.im = 0.0;
            }

            if inverse
                .process_with_scratch(spectrum, time, inverse_scratch)
                .is_err()
            {
                time.fill(0.0);
            }
            lane.ready
                .copy_from_slice(time.get(partition..).unwrap_or_default());
            lane.recent.copy_within(partition / *up.., 0);
        }
    }
}

fn unfolded(folded: &[Complex<f64>], spectrum: &mut [Complex<f64>]) {
    let period = 2 * (folded.len() - 1);
    for (bin, slot) in spectrum.iter_mut().enumerate() {
        let wrapped = bin % period;
        *slot = match folded.get(wrapped) {
            Some(value) => *value,
            None => folded
                .get(period - wrapped)
                .map_or(Complex::new(0.0, 0.0), Complex::conj),
        };
    }
}

fn multiply_into(
    real: &mut [f64],
    imaginary: &mut [f64],
    [heard_real, heard_imaginary]: [&[f64]; 2],
    [tap_real, tap_imaginary]: [&[f64]; 2],
) {
    for ((((real, imaginary), heard_real), heard_imaginary), (tap_real, tap_imaginary)) in real
        .iter_mut()
        .zip(imaginary.iter_mut())
        .zip(heard_real)
        .zip(heard_imaginary)
        .zip(tap_real.iter().zip(tap_imaginary))
    {
        *real += heard_real * tap_real - heard_imaginary * tap_imaginary;
        *imaginary += heard_real * tap_imaginary + heard_imaginary * tap_real;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct(kernel: &[f64], input: &[f64]) -> Vec<f64> {
        (0..input.len())
            .map(|at| {
                kernel
                    .iter()
                    .enumerate()
                    .filter(|(lag, _)| *lag <= at)
                    .map(|(lag, tap)| tap * input[at - lag])
                    .sum()
            })
            .collect()
    }

    #[test]
    fn the_partitioned_convolution_is_the_direct_one_a_partition_late() {
        let kernel: Vec<f64> = (0..1_300)
            .map(|tap| (f64::from(tap) * 0.013).cos() / 1_300.0)
            .collect();
        let frames = 3_000;
        let input: Vec<f64> = (0..frames)
            .flat_map(|at: u32| {
                let sample = (f64::from(at) * 0.21).sin() + 0.3 * (f64::from(at) * 1.7).cos();
                [sample, -sample]
            })
            .collect();

        for (partition, up) in [(PARTITION, 1), (PARTITION, 2), (WIDE_PARTITION, 2)] {
            let cache = Mutex::new(None);
            let spectra = Spectra::kept_for(&cache, 0_u8, partition, || kernel.clone());
            let mut stereo = Prefilter::new(spectra, 2, up);
            let mut released = [Vec::new(), Vec::new()];
            let mut rest = input.as_slice();
            let mut chunk = 1;
            while !rest.is_empty() {
                let frames = chunk.min(rest.len() / 2).min(stereo.room() / up);
                stereo.gather(rest, 2, frames);
                for (channel, out) in released.iter_mut().enumerate() {
                    out.extend_from_slice(stereo.ready(channel, 0..frames * up));
                }
                stereo.advance(frames * up);
                rest = &rest[frames * 2..];
                chunk = chunk % 97 + 13;
            }

            let stuffed: Vec<f64> = input
                .iter()
                .step_by(2)
                .flat_map(|sample| std::iter::once(*sample).chain(std::iter::repeat_n(0.0, up - 1)))
                .collect();
            let exact = direct(&kernel, &stuffed);
            for (at, (left, right)) in released[0].iter().zip(&released[1]).enumerate() {
                let wanted = at.checked_sub(partition).map_or(0.0, |from| exact[from]);
                assert!(
                    (left - wanted).abs() < 1e-13 && (right + wanted).abs() < 1e-13,
                    "{partition} up {up} at {at}: {left} and {right} against {wanted}"
                );
            }
        }
    }
}
