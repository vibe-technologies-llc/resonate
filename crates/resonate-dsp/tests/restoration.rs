use std::{
    f64::consts::PI,
    io::Write,
    ops::Range,
    process::{Command, Stdio},
    thread,
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_dsp::{Processor, Restoration, Restore, RestoreConfig, Tuning};
use rustfft::{FftPlanner, num_complex::Complex};

const RATE: f64 = 44_100.0;
const FRAMES: usize = 4 * 44_100;
const BLOCK: usize = 1_024;
const MEASURED: usize = 8_192;
const HALF_A_SECOND: usize = 22_050;
const PATCHED_FROM_ABOVE_THE_WALL_HZ: f64 = 300.0;
const PATCHED_TO_ABOVE_THE_WALL_HZ: f64 = 2_000.0;
const RAISED_AT_LEAST_DB: f64 = 20.0;
const GRANULE_FRAMES: usize = 576;
const HOLE_FROM_HZ: f64 = 7_000.0;
const HOLE_TO_HZ: f64 = 9_000.0;
const SLICE_FRAMES: usize = 256;
const PRIMED_AT_MOST: usize = 2_400;
const HOLE_RAISED_AT_LEAST_DB: f64 = 10.0;

struct Encoding {
    name: &'static str,
    tuning: Tuning,
    arguments: &'static [&'static str],
    keeps_a_granule_hole: bool,
}

const ENCODINGS: [Encoding; 3] = [
    Encoding {
        name: "mp3",
        tuning: Tuning::Mp3,
        arguments: &["-c:a", "libmp3lame", "-b:a", "96k", "-f", "mp3"],
        keeps_a_granule_hole: true,
    },
    Encoding {
        name: "aac",
        tuning: Tuning::Aac,
        arguments: &["-c:a", "aac", "-b:a", "64k", "-f", "adts"],
        keeps_a_granule_hole: false,
    },
    Encoding {
        name: "vorbis",
        tuning: Tuning::Vorbis,
        arguments: &["-c:a", "libvorbis", "-q:a", "0", "-f", "ogg"],
        keeps_a_granule_hole: true,
    },
];

fn tones(lowest: f64, highest: f64, count: usize) -> Vec<f64> {
    let mut seed: u64 = 0x5265_736F_6E61_7465;
    let mut draw = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1_u64 << 53) as f64
    };
    let voices: Vec<(f64, f64)> = (0..count)
        .map(|_| (lowest + draw() * (highest - lowest), draw() * 2.0 * PI))
        .collect();
    let level = 0.5 / (count as f64).sqrt();
    (0..FRAMES)
        .flat_map(|n| {
            let t = n as f64 / RATE;
            let sample: f64 = voices
                .iter()
                .map(|(hz, phase)| (2.0 * PI * hz * t + phase).sin())
                .sum::<f64>()
                * level;
            [sample, sample]
        })
        .collect()
}

fn encoded_and_decoded(encoding: &Encoding, samples: &[f64]) -> Option<Vec<f64>> {
    let rate = RATE.to_string();
    let mut encoder = Command::new("ffmpeg")
        .args(["-v", "error", "-f", "f64le", "-ar", &rate, "-ac", "2"])
        .args(["-i", "pipe:0"])
        .args(encoding.arguments)
        .arg("pipe:1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut feed = encoder.stdin.take()?;
    let encoded = encoder.stdout.take()?;
    let decoder = Command::new("ffmpeg")
        .args([
            "-v", "error", "-i", "pipe:0", "-ac", "2", "-f", "f64le", "pipe:1",
        ])
        .stdin(Stdio::from(encoded))
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let feeding = thread::spawn(move || feed.write_all(&bytes).is_ok());
    let decoded = decoder.wait_with_output().ok()?;
    let fed = feeding.join().unwrap_or(false);
    let encoded_well = encoder.wait().ok()?.success();
    if !fed || !encoded_well || !decoded.status.success() || decoded.stdout.is_empty() {
        return None;
    }
    Some(
        decoded
            .stdout
            .as_chunks::<8>()
            .0
            .iter()
            .map(|sample| f64::from_le_bytes(*sample))
            .collect(),
    )
}

fn through(restoration: Restoration, tuning: Tuning, input: &[f64]) -> (Vec<f64>, Restore) {
    let mut stage = Restore::new(RestoreConfig {
        restoration,
        tuning,
        wall: None,
    });
    let most = stage
        .prepare(
            StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::F32,
            ),
            BLOCK,
        )
        .expect("the stage prepares");
    let mut scratch = vec![0.0; most * 2];
    let mut output = Vec::new();
    for block in input.chunks(BLOCK * 2) {
        let count = stage.process(block, &mut scratch);
        output.extend_from_slice(&scratch[..count.frames_out * 2]);
    }
    let mut tail = vec![0.0; stage.max_flush_frames() * 2];
    let drained = stage.flush(&mut tail);
    output.extend_from_slice(&tail[..drained * 2]);
    (output, stage)
}

fn band_db(samples: &[f64], frames: Range<usize>, from_hz: f64, to_hz: f64) -> f64 {
    let length = frames.len();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(length);
    let mut spectrum: Vec<Complex<f64>> = (0..length)
        .map(|n| {
            let left = samples
                .get((frames.start + n) * 2)
                .copied()
                .unwrap_or_default();
            let hann = 0.5 - 0.5 * (2.0 * PI * n as f64 / length as f64).cos();
            Complex::new(left * hann, 0.0)
        })
        .collect();
    fft.process(&mut spectrum);
    let bin = |hz: f64| (hz * length as f64 / RATE) as usize;
    let power: f64 = spectrum[bin(from_hz)..bin(to_hz)]
        .iter()
        .map(|bin| bin.norm_sqr())
        .sum();
    10.0 * (power / ((bin(to_hz) - bin(from_hz)) * length) as f64)
        .max(1e-30)
        .log10()
}

#[test]
fn every_lossy_codec_is_extended_from_its_first_half_second() {
    let source = tones(100.0, 21_000.0, 400);
    for encoding in &ENCODINGS {
        let Some(decoded) = encoded_and_decoded(encoding, &source) else {
            eprintln!("skipped: no ffmpeg to encode {}", encoding.name);
            return;
        };
        let (extended, stage) = through(Restoration::Extend, encoding.tuning, &decoded);
        assert_eq!(
            extended.len(),
            decoded.len(),
            "{} changed length",
            encoding.name
        );
        let wall = f64::from(
            stage
                .wall()
                .unwrap_or_else(|| panic!("no wall was found in {}", encoding.name))
                .centihertz(),
        ) / 100.0;

        let from = wall + PATCHED_FROM_ABOVE_THE_WALL_HZ;
        let to = (wall + PATCHED_TO_ABOVE_THE_WALL_HZ).min(RATE / 2.0 * 0.95);
        let early = HALF_A_SECOND..HALF_A_SECOND + MEASURED;
        let raised =
            band_db(&extended, early.clone(), from, to) - band_db(&decoded, early, from, to);
        assert!(
            raised >= RAISED_AT_LEAST_DB,
            "{} with its wall at {wall} Hz was raised by {raised:.1} dB above it half a second in",
            encoding.name
        );
    }
}

#[test]
fn a_band_one_granule_missing_from_every_lossy_codec_is_filled() {
    let mut source = tones(500.0, 6_000.0, 200);
    let band = tones(HOLE_FROM_HZ, HOLE_TO_HZ, 60);
    let gap = 2 * 44_100..2 * 44_100 + GRANULE_FRAMES;
    for (frame, pair) in source.as_chunks_mut::<2>().0.iter_mut().enumerate() {
        if !gap.contains(&frame) {
            pair[0] += band[frame * 2];
            pair[1] += band[frame * 2 + 1];
        }
    }
    let within = HOLE_FROM_HZ + 300.0;
    let without = HOLE_TO_HZ - 300.0;
    for encoding in &ENCODINGS {
        let Some(decoded) = encoded_and_decoded(encoding, &source) else {
            eprintln!("skipped: no ffmpeg to encode {}", encoding.name);
            return;
        };
        let (repaired, _) = through(Restoration::Repair, encoding.tuning, &decoded);

        let deepest = (gap.start..gap.end + PRIMED_AT_MOST - SLICE_FRAMES)
            .step_by(16)
            .map(|at| at..at + SLICE_FRAMES)
            .min_by(|one, other| {
                band_db(&decoded, one.clone(), within, without).total_cmp(&band_db(
                    &decoded,
                    other.clone(),
                    within,
                    without,
                ))
            })
            .expect("the gap is longer than a slice");
        let taken = band_db(&decoded, deepest.clone(), within, without);
        let heard = band_db(&repaired, deepest.clone(), within, without);
        if encoding.keeps_a_granule_hole {
            assert!(
                heard - taken >= HOLE_RAISED_AT_LEAST_DB,
                "the hole {} kept was raised by {:.1} dB",
                encoding.name,
                heard - taken
            );
        } else {
            assert!(
                (heard - taken).abs() < 0.01,
                "the dropout {} smeared was moved by {:.1} dB",
                encoding.name,
                heard - taken
            );
        }
    }
}
