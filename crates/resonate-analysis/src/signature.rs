use std::{f64::consts::PI, sync::Arc};

use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{FilterPhase, Processor, Quality, Resampler, ResamplerConfig};
use rustfft::{Fft, FftPlanner, num_complex::Complex};
use smallvec::SmallVec;

use crate::print::standard_base64;

const SIGNED_AT: SampleRate = SampleRate::HZ_16000;
const SIGNED_AT_ID: u32 = 3;
const SAMPLE_RATE_ID_SHIFT: u32 = 27;
const HEADER_MAGIC: u32 = 0xcafe_2580;
const HEADER_SECOND_MAGIC: u32 = 0x9411_9c00;
const HEADER_FIXED: u32 = (15 << 19) + 0x40000;
const HEADER_BYTES: usize = 48;
const CONTENTS_MARKER: u32 = 0x4000_0000;
const FIRST_BAND_TAG: u32 = 0x6003_0040;
const SAMPLES_ADDED_PER_HZ: f64 = 0.24;
const CHECKED_FROM: usize = 8;
const DATA_URI: &str = "data:audio/vnd.shazam.sig;base64,";

const WINDOW: usize = 2_048;
const HOP: usize = 128;
const PEAKS_A_FRAME_HELD_INLINE: usize = 16;
const BINS: usize = WINDOW / 2 + 1;
const HELD_FRAMES: usize = 256;
const POWER_SCALE: f64 = (1 << 17) as f64;
const QUIETEST_POWER: f64 = 1e-10;
const SIXTEEN_BIT_SCALE: f64 = 32_768.0;
const SIGNED_SECONDS_AT_MOST: f64 = 12.0;

const PEAKS_LOOKED_FOR_AFTER: u64 = 46;
const EXAMINED_FRAMES_AGO: usize = 46;
const SPREAD_EXAMINED_FRAMES_AGO: usize = 49;
const SPREAD_BACK_INTO: [usize; 3] = [1, 3, 6];
const FIRST_EXAMINED_BIN: usize = 10;
const LAST_EXAMINED_BIN: usize = 1_014;
const QUIETEST_PEAK: f64 = 1.0 / 64.0;
const NEIGHBOURING_BINS: [isize; 8] = [-10, -7, -4, -3, 1, 2, 5, 8];
const NEIGHBOURING_FRAMES: [isize; 14] = [
    -53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249,
];
const MAGNITUDE_SCALE: f64 = 1_477.3;
const MAGNITUDE_OFFSET: f64 = 6_144.0;
const SUB_BINS: f64 = 64.0;
const SUB_BIN_VARIATION_SCALE: f64 = 32.0;
const BANDS_HZ: [(f64, f64); 4] = [
    (250.0, 520.0),
    (520.0, 1_450.0),
    (1_450.0, 3_500.0),
    (3_500.0, 5_500.0),
];
const FRAME_DELTA_ESCAPE: u8 = 0xff;
const RESAMPLED_IN_BLOCKS: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    bytes: Vec<u8>,
    samples: u32,
}

impl Signature {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn uri(&self) -> String {
        format!("{DATA_URI}{}", standard_base64(&self.bytes))
    }

    pub fn sample_ms(&self) -> u64 {
        u64::from(self.samples) * 1_000 / u64::from(SIGNED_AT.hz())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Peak {
    frame: u32,
    magnitude: u16,
    bin: u16,
}

pub fn signature_of(mono: &[f32], rate: SampleRate) -> Signature {
    let resampled = at_sixteen_kilohertz(mono, rate);
    let kept = middle_of(&resampled);
    let mut signing = Signing::new();
    for hop in kept.as_chunks::<HOP>().0 {
        signing.hear(hop);
    }
    serialised(&signing.bands, kept.len() as u32)
}

fn at_sixteen_kilohertz(mono: &[f32], rate: SampleRate) -> Vec<f64> {
    let widened: Vec<f64> = mono.iter().map(|sample| f64::from(*sample)).collect();
    if rate == SIGNED_AT {
        return widened;
    }
    let Ok(mut resampler) = Resampler::new(ResamplerConfig {
        input_rate: rate,
        output_rate: SIGNED_AT,
        channels: ChannelLayout::Mono,
        quality: Quality::Balanced,
        phase: FilterPhase::Linear,
        max_frames_in: RESAMPLED_IN_BLOCKS,
    }) else {
        return Vec::new();
    };
    let mut resampled = Vec::with_capacity(widened.len() / 2);
    let mut scratch = vec![0.0; resampler.max_output_frames(RESAMPLED_IN_BLOCKS)];
    for block in widened.chunks(RESAMPLED_IN_BLOCKS) {
        let made = resampler.process(block, &mut scratch).frames_out;
        resampled.extend_from_slice(scratch.get(..made).unwrap_or_default());
    }
    let mut tail = vec![0.0; resampler.max_flush_frames()];
    let drained = resampler.flush(&mut tail);
    resampled.extend_from_slice(tail.get(..drained).unwrap_or_default());
    resampled
}

fn middle_of(samples: &[f64]) -> Vec<f64> {
    let most = (SIGNED_SECONDS_AT_MOST * f64::from(SIGNED_AT.hz())) as usize;
    let skipped = samples.len().saturating_sub(most) / 2;
    samples
        .iter()
        .skip(skipped)
        .take(most)
        .map(|sample| {
            (sample * SIXTEEN_BIT_SCALE)
                .round()
                .clamp(-SIXTEEN_BIT_SCALE, SIXTEEN_BIT_SCALE - 1.0)
        })
        .collect()
}

struct Signing {
    fft: Arc<dyn Fft<f64>>,
    window: Vec<f64>,
    held: Vec<f64>,
    held_at: usize,
    spectrum: Vec<Complex<f64>>,
    powers: Vec<Vec<f64>>,
    spread: Vec<Vec<f64>>,
    frames_done: u64,
    bands: [Vec<Peak>; 4],
}

impl Signing {
    fn new() -> Self {
        let mut planner = FftPlanner::<f64>::new();
        Self {
            fft: planner.plan_fft_forward(WINDOW),
            window: (0..WINDOW)
                .map(|n| 0.5 - 0.5 * (2.0 * PI * (n + 1) as f64 / (WINDOW + 1) as f64).cos())
                .collect(),
            held: vec![0.0; WINDOW],
            held_at: 0,
            spectrum: vec![Complex::new(0.0, 0.0); WINDOW],
            powers: vec![vec![QUIETEST_POWER; BINS]; HELD_FRAMES],
            spread: vec![vec![0.0; BINS]; HELD_FRAMES],
            frames_done: 0,
            bands: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        }
    }

    fn hear(&mut self, hop: &[f64]) {
        for sample in hop {
            if let Some(slot) = self.held.get_mut(self.held_at) {
                *slot = *sample;
            }
            self.held_at = (self.held_at + 1) % WINDOW;
        }
        self.transform();
        self.spread_the_latest();
        if self.frames_done >= PEAKS_LOOKED_FOR_AFTER {
            self.look_for_peaks();
        }
    }

    fn slot(&self, frames_ago: isize) -> usize {
        let current = (self.frames_done % HELD_FRAMES as u64) as isize;
        (current + frames_ago).rem_euclid(HELD_FRAMES as isize) as usize
    }

    fn transform(&mut self) {
        for (offset, (bin, weight)) in self.spectrum.iter_mut().zip(&self.window).enumerate() {
            let sample = self
                .held
                .get((self.held_at + offset) % WINDOW)
                .copied()
                .unwrap_or_default();
            *bin = Complex::new(sample * weight, 0.0);
        }
        self.fft.process(&mut self.spectrum);
        let slot = self.slot(0);
        if let Some(powers) = self.powers.get_mut(slot) {
            for (power, bin) in powers.iter_mut().zip(&self.spectrum) {
                *power = (bin.norm_sqr() / POWER_SCALE).max(QUIETEST_POWER);
            }
        }
    }

    fn spread_the_latest(&mut self) {
        let slot = self.slot(0);
        let mut spread = self.powers.get(slot).cloned().unwrap_or_default();
        for bin in 0..BINS.saturating_sub(2) {
            let widest = spread
                .get(bin..bin + 3)
                .unwrap_or_default()
                .iter()
                .copied()
                .fold(f64::MIN, f64::max);
            if let Some(slot) = spread.get_mut(bin) {
                *slot = widest;
            }
        }
        for back in SPREAD_BACK_INTO {
            let earlier = self.slot(-(back as isize));
            if let Some(held) = self.spread.get_mut(earlier) {
                for (kept, latest) in held.iter_mut().zip(&spread) {
                    *kept = kept.max(*latest);
                }
            }
        }
        if let Some(held) = self.spread.get_mut(slot) {
            *held = spread;
        }
        self.frames_done += 1;
    }

    fn look_for_peaks(&mut self) {
        let examined = self.slot(-(EXAMINED_FRAMES_AGO as isize));
        let spread_examined = self.slot(-(SPREAD_EXAMINED_FRAMES_AGO as isize));
        let (Some(powers), Some(spread)) =
            (self.powers.get(examined), self.spread.get(spread_examined))
        else {
            return;
        };
        let frame = (self.frames_done - PEAKS_LOOKED_FOR_AFTER) as u32;
        let mut found = SmallVec::<[Peak; PEAKS_A_FRAME_HELD_INLINE]>::new();

        for bin in FIRST_EXAMINED_BIN..=LAST_EXAMINED_BIN {
            let power = powers.get(bin).copied().unwrap_or_default();
            let below = spread.get(bin - 1).copied().unwrap_or_default();
            if power < QUIETEST_PEAK || power < below {
                continue;
            }
            let loudest_near = NEIGHBOURING_BINS
                .iter()
                .filter_map(|offset| spread.get(bin.checked_add_signed(*offset)?))
                .copied()
                .fold(0.0, f64::max);
            if power <= loudest_near {
                continue;
            }
            let loudest_around = NEIGHBOURING_FRAMES
                .iter()
                .filter_map(|offset| {
                    let slot = self.slot(*offset);
                    self.spread.get(slot)?.get(bin - 1)
                })
                .copied()
                .fold(0.0, f64::max);
            if power <= loudest_around {
                continue;
            }
            found.push(peak_at(powers, bin, frame));
        }

        for peak in found {
            let hertz =
                f64::from(peak.bin) * (f64::from(SIGNED_AT.hz()) / 2.0 / 1_024.0 / SUB_BINS);
            if let Some(band) = BANDS_HZ
                .iter()
                .position(|(from, to)| (*from..*to).contains(&hertz))
                .and_then(|band| self.bands.get_mut(band))
            {
                band.push(peak);
            }
        }
    }
}

fn logged(power: f64) -> f64 {
    power.ln().max(QUIETEST_PEAK) * MAGNITUDE_SCALE + MAGNITUDE_OFFSET
}

fn peak_at(powers: &[f64], bin: usize, frame: u32) -> Peak {
    let at = |bin: usize| logged(powers.get(bin).copied().unwrap_or(QUIETEST_POWER));
    let (before, peak, after) = (at(bin - 1), at(bin), at(bin + 1));
    let curvature = peak * 2.0 - before - after;
    let variation = if curvature == 0.0 {
        0.0
    } else {
        (after - before) * SUB_BIN_VARIATION_SCALE / curvature
    };
    Peak {
        frame,
        magnitude: peak as u16,
        bin: (bin as i32 * SUB_BINS as i32 + variation as i32) as u16,
    }
}

fn serialised(bands: &[Vec<Peak>; 4], samples: u32) -> Signature {
    let mut contents = Vec::new();
    for (band, peaks) in bands.iter().enumerate() {
        if peaks.is_empty() {
            continue;
        }
        let mut written = Vec::new();
        let mut last = 0_u32;
        for peak in peaks {
            if peak.frame - last >= u32::from(FRAME_DELTA_ESCAPE) {
                written.push(FRAME_DELTA_ESCAPE);
                written.extend_from_slice(&peak.frame.to_le_bytes());
                last = peak.frame;
            }
            written.push((peak.frame - last) as u8);
            written.extend_from_slice(&peak.magnitude.to_le_bytes());
            written.extend_from_slice(&peak.bin.to_le_bytes());
            last = peak.frame;
        }
        contents.extend_from_slice(&(FIRST_BAND_TAG + band as u32).to_le_bytes());
        contents.extend_from_slice(&(written.len() as u32).to_le_bytes());
        let padding = (4 - written.len() % 4) % 4;
        contents.extend_from_slice(&written);
        contents.extend(std::iter::repeat_n(0, padding));
    }

    let size_minus_header = (contents.len() + 8) as u32;
    let mut bytes = Vec::with_capacity(HEADER_BYTES + contents.len() + 8);
    for word in [
        HEADER_MAGIC,
        0,
        size_minus_header,
        HEADER_SECOND_MAGIC,
        0,
        0,
        0,
        SIGNED_AT_ID << SAMPLE_RATE_ID_SHIFT,
        0,
        0,
        samples + (f64::from(SIGNED_AT.hz()) * SAMPLES_ADDED_PER_HZ) as u32,
        HEADER_FIXED,
        CONTENTS_MARKER,
        size_minus_header,
    ] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes.extend_from_slice(&contents);
    let checksum = crc32(bytes.get(CHECKED_FROM..).unwrap_or_default());
    if let Some(slot) = bytes.get_mut(4..8) {
        slot.copy_from_slice(&checksum.to_le_bytes());
    }
    Signature { bytes, samples }
}

const CRC32_POLYNOMIAL: u32 = 0xedb8_8320;

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let low = crc & 1;
            crc >>= 1;
            if low == 1 {
                crc ^= CRC32_POLYNOMIAL;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_clip;

    fn word(bytes: &[u8], at: usize) -> u32 {
        u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
    }

    fn tone(hertz: &[f64], rate: SampleRate, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(rate.hz())) as usize;
        (0..frames)
            .map(|n| {
                let t = n as f64 / f64::from(rate.hz());
                let summed: f64 = hertz.iter().map(|hz| (2.0 * PI * hz * t).sin()).sum();
                (0.4 * summed / hertz.len() as f64) as f32
            })
            .collect()
    }

    fn notes(hertz: &[f64], rate: SampleRate, seconds: f64) -> Vec<f32> {
        let frames = (seconds * f64::from(rate.hz())) as usize;
        let note = f64::from(rate.hz()) * NOTE_SECONDS;
        (0..frames)
            .map(|n| {
                let at = n as f64 / note;
                let which = hertz[(at as usize) % hertz.len()];
                let within = at.fract();
                let envelope = if within < NOTE_SOUNDS_FOR { 1.0 } else { 0.0 };
                let t = n as f64 / f64::from(rate.hz());
                (0.4 * envelope * (2.0 * PI * which * t).sin()) as f32
            })
            .collect()
    }

    const NOTE_SECONDS: f64 = 0.25;
    const NOTE_SOUNDS_FOR: f64 = 0.6;

    fn bands_of(signature: &Signature) -> Vec<(u32, Vec<Peak>)> {
        let bytes = signature.bytes();
        let mut at = HEADER_BYTES + 8;
        let mut bands = Vec::new();
        while at < bytes.len() {
            let tag = word(bytes, at);
            let length = word(bytes, at + 4) as usize;
            let mut peaks = Vec::new();
            let (mut read, mut frame) = (at + 8, 0_u32);
            while read < at + 8 + length {
                if bytes[read] == FRAME_DELTA_ESCAPE {
                    frame = word(bytes, read + 1);
                    read += 5;
                }
                frame += u32::from(bytes[read]);
                let magnitude = u16::from_le_bytes([bytes[read + 1], bytes[read + 2]]);
                let bin = u16::from_le_bytes([bytes[read + 3], bytes[read + 4]]);
                peaks.push(Peak {
                    frame,
                    magnitude,
                    bin,
                });
                read += 5;
            }
            bands.push((tag, peaks));
            at += 8 + length.next_multiple_of(4);
        }
        bands
    }

    #[test]
    fn a_signature_carries_the_header_the_service_reads_and_checks_its_own_crc() {
        let signature = signature_of(
            &tone(&[440.0, 2_000.0], SampleRate::HZ_48000, 6.0),
            SampleRate::HZ_48000,
        );
        let bytes = signature.bytes();
        assert_eq!(word(bytes, 0), HEADER_MAGIC);
        assert_eq!(word(bytes, 4), crc32(&bytes[8..]));
        assert_eq!(word(bytes, 8) as usize, bytes.len() - HEADER_BYTES);
        assert_eq!(word(bytes, 12), HEADER_SECOND_MAGIC);
        assert_eq!(word(bytes, 28) >> SAMPLE_RATE_ID_SHIFT, SIGNED_AT_ID);
        assert_eq!(word(bytes, 40) - 3_840, 6 * 16_000);
        assert_eq!(word(bytes, 44), 0x007c_0000);
        assert_eq!(word(bytes, 48), CONTENTS_MARKER);
        assert_eq!(signature.sample_ms(), 6_000);
        assert!(signature.uri().starts_with(DATA_URI));
    }

    #[test]
    fn a_tone_leaves_its_peaks_in_the_band_it_sounds_in() {
        let signature = signature_of(
            &notes(&[440.0, 2_000.0], SampleRate::HZ_44100, 6.0),
            SampleRate::HZ_44100,
        );
        let bands = bands_of(&signature);
        let tags: Vec<u32> = bands.iter().map(|(tag, _)| *tag - FIRST_BAND_TAG).collect();
        assert!(
            tags.contains(&0),
            "440 Hz left nothing in the lowest band: {tags:?}"
        );
        assert!(
            tags.contains(&2),
            "2 kHz left nothing in the third band: {tags:?}"
        );
        for (tag, peaks) in &bands {
            let (from, to) = BANDS_HZ[(*tag - FIRST_BAND_TAG) as usize];
            for peak in peaks {
                let hertz = f64::from(peak.bin) * 16_000.0 / 2.0 / 1_024.0 / SUB_BINS;
                assert!(
                    (from..to).contains(&hertz),
                    "{hertz} Hz filed under {from}..{to}"
                );
            }
            assert!(peaks.windows(2).all(|pair| pair[0].frame <= pair[1].frame));
        }
        let near_440 = bands[0].1.iter().any(|peak| {
            let hertz = f64::from(peak.bin) * 16_000.0 / 2.0 / 1_024.0 / SUB_BINS;
            (hertz - 440.0).abs() < 10.0
        });
        assert!(near_440, "no peak landed near 440 Hz");
    }

    #[test]
    fn the_checksum_is_the_standard_crc_32() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn the_uri_is_standard_base64_with_its_padding() {
        use crate::print::standard_base64;
        assert_eq!(standard_base64(b"Man"), "TWFu");
        assert_eq!(standard_base64(b"Ma"), "TWE=");
        assert_eq!(standard_base64(b"M"), "TQ==");
        assert_eq!(standard_base64(&[0xfb, 0xff]), "+/8=");
    }

    #[test]
    fn a_clip_can_be_printed_the_way_a_track_is() {
        let clip = tone(&[330.0, 1_250.0, 2_900.0], SampleRate::HZ_48000, 8.0);
        assert!(print_clip(&clip, 48_000, 1).is_some());
    }
}
