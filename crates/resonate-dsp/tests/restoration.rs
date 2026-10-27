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
const MUSIC_SECONDS: f64 = 6.0;
const MUSIC_SEED: u64 = 0x0123_4567_89AB_CDEF;
const TRANSIENT_MUSIC_SECONDS: f64 = 8.0;
const TRANSIENT_MUSIC_SEED: u64 = 0xFEED_FACE_CAFE_BEEF;
const ABOVE_THE_WALL_FROM_HZ: f64 = 300.0;
const ABOVE_THE_WALL_TO_HZ: f64 = 2_000.0;
const BELOW_THE_WALL_FROM_HZ: f64 = 3_000.0;
const BELOW_THE_WALL_TO_HZ: f64 = 500.0;
const EXTENSION_AT_MOST_DB: f64 = -3.0;
const EXTENSION_AT_LEAST_DB: f64 = -60.0;
const NEVER_LOWERED_BY_MORE_THAN_DB: f64 = 0.5;
const DEEP_WALL_RAISED_AT_LEAST_DB: f64 = 20.0;
const HOLE_KEPT_DEEPER_THAN_DB: f64 = 45.0;
const SHORT_BLOCK_FRAMES: usize = 128;
const SHORT_BLOCK_STRIDE_FRAMES: usize = 32;
const SHORT_BLOCK_NEIGHBOURS: usize = 16;
const SHORT_BLOCK_FROM_HZ: f64 = 5_000.0;
const SHORT_BLOCK_TO_HZ: f64 = 12_000.0;
const SHORT_HOLE_UNDER_ITS_NEIGHBOURS_DB: f64 = 20.0;
const SHORT_HOLE_RAISED_AT_LEAST_DB: f64 = 6.0;
const SHORT_HOLES_FILLED_AT_LEAST: usize = 20;
const HARMONICS_TO_HZ: f64 = 20_000.0;
const NOTE_SECONDS: f64 = 0.25;
const ATTACK_SECONDS: f64 = 0.004;
const DECAY_SECONDS: f64 = 0.3;
const NOTE_LEVEL: f64 = 0.12;
const STRIKE_SECONDS: f64 = 0.125;
const STRIKE_RINGS_SECONDS: f64 = 0.04;
const STRIKE_DECAY_SECONDS: f64 = 0.008;
const STRIKE_LEVEL: f64 = 0.6;

struct Encoding {
    name: &'static str,
    tuning: Tuning,
    arguments: &'static [&'static str],
    keeps_a_granule_hole: bool,
    cuts_a_deep_wall: bool,
}

const fn encoding(
    name: &'static str,
    tuning: Tuning,
    arguments: &'static [&'static str],
    cuts_a_deep_wall: bool,
) -> Encoding {
    Encoding {
        name,
        tuning,
        arguments,
        keeps_a_granule_hole: false,
        cuts_a_deep_wall,
    }
}

const ENCODINGS: [Encoding; 3] = [
    Encoding {
        name: "mp3",
        tuning: Tuning::Mp3,
        arguments: &["-c:a", "libmp3lame", "-b:a", "96k", "-f", "mp3"],
        keeps_a_granule_hole: true,
        cuts_a_deep_wall: true,
    },
    Encoding {
        name: "aac",
        tuning: Tuning::Aac,
        arguments: &["-c:a", "aac", "-b:a", "64k", "-f", "adts"],
        keeps_a_granule_hole: false,
        cuts_a_deep_wall: false,
    },
    Encoding {
        name: "vorbis",
        tuning: Tuning::Vorbis,
        arguments: &["-c:a", "libvorbis", "-q:a", "0", "-f", "ogg"],
        keeps_a_granule_hole: true,
        cuts_a_deep_wall: false,
    },
];

const BREADTH: [Encoding; 11] = [
    encoding(
        "mp3 64k",
        Tuning::Mp3,
        &["-c:a", "libmp3lame", "-b:a", "64k", "-f", "mp3"],
        true,
    ),
    encoding(
        "mp3 128k",
        Tuning::Mp3,
        &["-c:a", "libmp3lame", "-b:a", "128k", "-f", "mp3"],
        true,
    ),
    encoding(
        "mp3 192k",
        Tuning::Mp3,
        &["-c:a", "libmp3lame", "-b:a", "192k", "-f", "mp3"],
        true,
    ),
    encoding(
        "mp3 v4",
        Tuning::Mp3,
        &["-c:a", "libmp3lame", "-q:a", "4", "-f", "mp3"],
        true,
    ),
    encoding(
        "aac 64k",
        Tuning::Aac,
        &["-c:a", "aac", "-b:a", "64k", "-f", "adts"],
        false,
    ),
    encoding(
        "aac 96k",
        Tuning::Aac,
        &["-c:a", "aac", "-b:a", "96k", "-f", "adts"],
        false,
    ),
    encoding(
        "aac 128k",
        Tuning::Aac,
        &["-c:a", "aac", "-b:a", "128k", "-f", "adts"],
        false,
    ),
    encoding(
        "aac 192k",
        Tuning::Aac,
        &["-c:a", "aac", "-b:a", "192k", "-f", "adts"],
        false,
    ),
    encoding(
        "vorbis q0",
        Tuning::Vorbis,
        &["-c:a", "libvorbis", "-q:a", "0", "-f", "ogg"],
        false,
    ),
    encoding(
        "vorbis q3",
        Tuning::Vorbis,
        &["-c:a", "libvorbis", "-q:a", "3", "-f", "ogg"],
        false,
    ),
    encoding(
        "vorbis q6",
        Tuning::Vorbis,
        &["-c:a", "libvorbis", "-q:a", "6", "-f", "ogg"],
        false,
    ),
];

const SHORT_BLOCK_ENCODING: Encoding = encoding(
    "aac 32k",
    Tuning::Aac,
    &["-c:a", "aac", "-b:a", "32k", "-f", "adts"],
    false,
);

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

fn music(seconds: f64, seed: u64) -> Vec<f64> {
    let frames = (seconds * RATE) as usize;
    let mut state = seed;
    let mut draw = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1_u64 << 53) as f64
    };
    let mut channels = [vec![0.0; frames], vec![0.0; frames]];
    for samples in &mut channels {
        let notes = (seconds / NOTE_SECONDS) as usize;
        for note in 0..notes {
            let from = (note as f64 * NOTE_SECONDS * RATE) as usize;
            let fundamental = 110.0 * 2.0_f64.powf((draw() * 36.0).floor() / 12.0);
            let harmonics = ((HARMONICS_TO_HZ / fundamental) as usize).max(1);
            let phase = draw() * 2.0 * PI;
            let rings = (NOTE_SECONDS * 2.0 * RATE) as usize;
            for (offset, slot) in samples.iter_mut().skip(from).take(rings).enumerate() {
                let t = offset as f64 / RATE;
                let envelope = (1.0 - (-t / ATTACK_SECONDS).exp()) * (-t / DECAY_SECONDS).exp();
                let tone: f64 = (1..=harmonics)
                    .map(|h| {
                        (2.0 * PI * fundamental * h as f64 * t + phase * h as f64).sin()
                            / (h as f64).powf(1.1)
                    })
                    .sum();
                *slot += tone * envelope * NOTE_LEVEL;
            }
        }
        let strikes = (seconds / STRIKE_SECONDS) as usize;
        for strike in 0..strikes {
            let from = (strike as f64 * STRIKE_SECONDS * RATE) as usize;
            let rings = (STRIKE_RINGS_SECONDS * RATE) as usize;
            for (offset, slot) in samples.iter_mut().skip(from).take(rings).enumerate() {
                let t = offset as f64 / RATE;
                *slot += (draw() - 0.5) * (-t / STRIKE_DECAY_SECONDS).exp() * STRIKE_LEVEL;
            }
        }
    }
    channels[0]
        .iter()
        .zip(&channels[1])
        .flat_map(|(left, right)| [*left, *right])
        .collect()
}

#[test]
fn every_bitrate_of_every_lossy_codec_is_extended_only_where_the_encoder_cut() {
    let source = music(MUSIC_SECONDS, MUSIC_SEED);
    for encoding in &BREADTH {
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
        let wall = stage
            .wall()
            .map(|wall| f64::from(wall.centihertz()) / 100.0);
        if encoding.cuts_a_deep_wall {
            assert!(wall.is_some(), "no wall was found in {}", encoding.name);
        }
        let Some(wall) = wall else {
            continue;
        };

        let early = HALF_A_SECOND..HALF_A_SECOND + MEASURED;
        let from = wall + ABOVE_THE_WALL_FROM_HZ;
        let to = (wall + ABOVE_THE_WALL_TO_HZ).min(RATE / 2.0 * 0.95);
        let below = band_db(
            &extended,
            early.clone(),
            wall - BELOW_THE_WALL_FROM_HZ,
            wall - BELOW_THE_WALL_TO_HZ,
        );
        let before = band_db(&decoded, early.clone(), from, to) - below;
        let after = band_db(&extended, early, from, to) - below;
        assert!(
            (EXTENSION_AT_LEAST_DB..=EXTENSION_AT_MOST_DB).contains(&after),
            "{} with its wall at {wall} Hz sits at {after:.1} dB against the band below it",
            encoding.name
        );
        assert!(
            after >= before - NEVER_LOWERED_BY_MORE_THAN_DB,
            "{} was lowered from {before:.1} to {after:.1} dB above its wall",
            encoding.name
        );
        if encoding.cuts_a_deep_wall {
            assert!(
                after - before >= DEEP_WALL_RAISED_AT_LEAST_DB,
                "{} with its wall at {wall} Hz was raised by only {:.1} dB above it",
                encoding.name,
                after - before
            );
        }
    }
}

#[test]
fn a_hole_an_encoder_kept_deep_is_filled_at_every_bitrate() {
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
    for encoding in &BREADTH {
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
        let steady = gap.start - MEASURED..gap.start - MEASURED / 2;
        let taken = band_db(&decoded, deepest.clone(), within, without);
        let kept = band_db(&decoded, steady, within, without) - taken;
        let raised = band_db(&repaired, deepest, within, without) - taken;
        assert!(
            raised >= -0.01,
            "the hole {} kept was lowered by {raised:.1} dB",
            encoding.name
        );
        if kept >= HOLE_KEPT_DEEPER_THAN_DB {
            assert!(
                raised >= HOLE_RAISED_AT_LEAST_DB,
                "the hole {} kept {kept:.1} dB deep was raised by only {raised:.1} dB",
                encoding.name
            );
        }
    }
}

fn level_in_a_short_block(samples: &[f64], at: usize, to_hz: f64) -> f64 {
    band_db(
        samples,
        at..at + SHORT_BLOCK_FRAMES,
        SHORT_BLOCK_FROM_HZ,
        to_hz,
    )
}

#[test]
fn holes_an_aac_encoder_left_in_its_short_blocks_are_filled() {
    let source = music(TRANSIENT_MUSIC_SECONDS, TRANSIENT_MUSIC_SEED);
    let Some(decoded) = encoded_and_decoded(&SHORT_BLOCK_ENCODING, &source) else {
        eprintln!("skipped: no ffmpeg to encode {}", SHORT_BLOCK_ENCODING.name);
        return;
    };
    let (repaired, stage) = through(Restoration::Repair, Tuning::Aac, &decoded);
    assert_eq!(repaired.len(), decoded.len(), "the stage changed length");
    assert!(
        repaired.iter().all(|sample| sample.is_finite()),
        "the stage made a sample that is not a number"
    );

    let to_hz = stage.wall().map_or(SHORT_BLOCK_TO_HZ, |wall| {
        (f64::from(wall.centihertz()) / 100.0 - BELOW_THE_WALL_TO_HZ).min(SHORT_BLOCK_TO_HZ)
    });
    let frames = decoded.len() / 2;
    let before = SHORT_BLOCK_NEIGHBOURS / 2 * SHORT_BLOCK_FRAMES;
    let mut filled = 0;
    let mut at = before;
    while at + before < frames {
        let mut around: Vec<f64> = (0..SHORT_BLOCK_NEIGHBOURS)
            .map(|neighbour| {
                level_in_a_short_block(
                    &decoded,
                    at - before + neighbour * SHORT_BLOCK_FRAMES,
                    to_hz,
                )
            })
            .collect();
        around.sort_by(f64::total_cmp);
        let taken = level_in_a_short_block(&decoded, at, to_hz);
        let heard = level_in_a_short_block(&repaired, at, to_hz);
        let is_a_hole =
            around[SHORT_BLOCK_NEIGHBOURS / 2] - taken > SHORT_HOLE_UNDER_ITS_NEIGHBOURS_DB;
        if is_a_hole && heard - taken > SHORT_HOLE_RAISED_AT_LEAST_DB {
            filled += 1;
        }
        at += SHORT_BLOCK_STRIDE_FRAMES;
    }
    assert!(
        filled >= SHORT_HOLES_FILLED_AT_LEAST,
        "only {filled} windows one short block long were filled out of the holes the encoder left"
    );
}
