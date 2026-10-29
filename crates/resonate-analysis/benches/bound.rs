use std::{
    env,
    f64::consts::{PI, SQRT_2, TAU},
    fmt::Write as _,
    fs,
    hint::black_box,
    path::PathBuf,
    process,
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_analysis::{Applied, BandGains, BandPeaks, Loudest, Watch, band_figures};
use resonate_codec::Sources;
use resonate_core::{
    MediaLocation, SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use resonate_dsp::Impulse;

const RATE: u32 = 44_100;
const SONG_SECONDS: f64 = 4.0;
const FADE_SECONDS: f64 = 0.1;
const SONGS_FOLDED: usize = 1_000;
const RUNS: usize = 7;
const KEPT_BOUNDS: &str = "RESONATE_BENCH_BOUNDS";
const ROOM_SECONDS: usize = 10;

struct Noise(u32);

impl Noise {
    fn white(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0 >> 8) / f64::from(1_u32 << 24) - 0.5
    }
}

fn faded(position: f64) -> f64 {
    let fade = (position / FADE_SECONDS).clamp(0.0, 1.0);
    0.5 - 0.5 * (PI * fade).cos()
}

fn frames() -> usize {
    (f64::from(RATE) * SONG_SECONDS) as usize
}

fn enveloped(seconds: f64) -> f64 {
    faded(seconds).min(faded(SONG_SECONDS - seconds))
}

fn band(seed: u32, tempo: f64, level: f64) -> Vec<f64> {
    let mut noise = Noise(seed);
    let mut hiss = 0.0;
    let beat_seconds = 60.0 / tempo;
    let pads = [330.0, 415.0, 494.0, 1_250.0, 2_600.0]
        .map(|hertz| hertz * (1.0 + f64::from(seed % 7) * 0.03));
    (0..frames())
        .map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let beat = seconds % beat_seconds;
            let kick = (TAU * (50.0 + 60.0 * (-beat * 30.0).exp()) * beat).sin()
                * (-beat * 12.0).exp()
                * 0.5;
            let white = noise.white();
            let off = (seconds + beat_seconds / 2.0) % beat_seconds;
            let hat = (white - hiss) * (-off * 60.0).exp() * 0.4;
            hiss = white;
            let bass = (TAU * 82.0 * seconds).sin() * 0.15;
            let pad: f64 = pads
                .iter()
                .map(|hertz| (TAU * hertz * seconds).sin() * 0.06)
                .sum();
            (kick + hat + bass + pad) * enveloped(seconds) * level
        })
        .collect()
}

fn noise(seed: u32, smoothing: f64, level: f64) -> Vec<f64> {
    let mut noise = Noise(seed);
    let mut held = 0.0;
    (0..frames())
        .map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            held = held * smoothing + noise.white() * (1.0 - smoothing);
            held * enveloped(seconds) * level / (1.0 - smoothing).sqrt()
        })
        .collect()
}

fn beats(low: f64, high: f64, level: f64) -> Vec<f64> {
    (0..frames())
        .map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let sounded = (TAU * low * seconds).sin() * 0.55 + (TAU * high * seconds).sin() * 0.45;
            sounded * enveloped(seconds) * level
        })
        .collect()
}

fn wave(samples: &[f64]) -> Vec<u8> {
    let channels = 2_u16;
    let block_align = channels * 2;
    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&16_u16.to_le_bytes());
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| {
            let word = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
            [word, word]
        })
        .flat_map(i16::to_le_bytes)
        .collect();
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    for (id, payload) in [(b"fmt ", &format), (b"data", &data)] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
    }
    let mut whole = Vec::new();
    whole.extend_from_slice(b"RIFF");
    whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
    whole.extend_from_slice(&body);
    whole
}

fn songs() -> Vec<(String, Vec<f64>)> {
    let mut songs = Vec::new();
    for seed in 0..6_u32 {
        songs.push((
            format!("band {seed}"),
            band(
                0x1234_5678 + seed,
                100.0 + 12.0 * f64::from(seed),
                0.9 + 0.02 * f64::from(seed),
            ),
        ));
    }
    for (seed, smoothing) in [(1_u32, 0.0), (2, 0.5), (3, 0.9)] {
        songs.push((format!("noise {smoothing}"), noise(seed, smoothing, 0.3)));
    }
    for (low, high) in [(63.0, 2_500.0), (110.0, 5_000.0), (41.0, 900.0)] {
        songs.push((format!("beats {low} {high}"), beats(low, high, 0.9)));
    }
    songs
}

fn measured(songs: &[(String, Vec<f64>)]) -> Vec<(String, BandPeaks)> {
    let root = env::temp_dir().join(format!("resonate-bound-bench-{}", process::id()));
    fs::create_dir_all(&root).expect("a scratch folder");
    let peaks = songs
        .iter()
        .enumerate()
        .map(|(index, (name, samples))| {
            let path: PathBuf = root.join(format!("{index}.wav"));
            fs::write(&path, wave(samples)).expect("a written song");
            let figures = band_figures(
                &Sources::local(),
                &MediaLocation::local(path),
                None,
                &Watch::default(),
            )
            .expect("measured bands");
            (name.clone(), figures.peaks)
        })
        .collect();
    let _ = fs::remove_dir_all(&root);
    peaks
}

fn shaped(bands: &[(BandKind, f64, f64, f64)]) -> Arc<Profile> {
    let bands = bands
        .iter()
        .map(|(kind, hertz, decibels, q)| {
            Band::new(
                *kind,
                Frequency::from_hertz(*hertz).expect("in range"),
                BandGain::from_decibels(*decibels).expect("in range"),
                Q::from_units(*q).expect("in range"),
            )
        })
        .collect();
    Arc::new(Profile::new(Preamp::NONE, bands).expect("a profile in range"))
}

fn octaves(gains: [f64; 10]) -> Arc<Profile> {
    let centres = [
        31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ];
    shaped(
        &centres
            .iter()
            .zip(gains)
            .map(|(hertz, gain)| (BandKind::Peaking, *hertz, gain, SQRT_2))
            .collect::<Vec<_>>(),
    )
}

fn curves() -> Vec<(&'static str, Arc<Profile>)> {
    vec![
        (
            "bass boost",
            shaped(&[(BandKind::Peaking, 110.0, 6.0, 1.0)]),
        ),
        (
            "boosts and cuts",
            shaped(&[
                (BandKind::Peaking, 80.0, 5.0, 1.0),
                (BandKind::Peaking, 800.0, -4.0, 1.5),
                (BandKind::Peaking, 2_500.0, 6.0, 2.0),
                (BandKind::HighPass, 30.0, 0.0, 0.7),
            ]),
        ),
        (
            "smile",
            octaves([6.0, 5.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 5.0, 6.0]),
        ),
        (
            "ragged",
            octaves([4.0, -3.0, 5.0, -2.0, 3.0, -4.0, 6.0, -1.0, 4.0, -3.0]),
        ),
        (
            "tilt",
            octaves([-4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
        ),
        (
            "turned phase",
            shaped(&[
                (BandKind::AllPass, 300.0, 0.0, 4.0),
                (BandKind::AllPass, 3_000.0, 0.0, 4.0),
                (BandKind::Peaking, 300.0, 4.0, 1.0),
            ]),
        ),
    ]
}

fn reverberant() -> Arc<Impulse> {
    let mut noise = Noise(0x7a11);
    let frames = RATE as usize * ROOM_SECONDS;
    let taps = (0..frames)
        .map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let direct = if frame == 0 { 0.8 } else { 0.0 };
            direct + noise.white() * 0.05 * (-seconds * 1.5).exp()
        })
        .collect();
    Arc::new(Impulse::new(SampleRate::HZ_44100, vec![taps]).expect("a response"))
}

fn per_song(elapsed: Duration, songs: usize) -> f64 {
    elapsed.as_secs_f64() * 1e6 / songs as f64
}

fn fastest<T>(runs: usize, mut run: impl FnMut() -> T) -> (Duration, T) {
    let mut fastest = Duration::MAX;
    let mut last = None;
    for _ in 0..runs {
        let started = Instant::now();
        let made = run();
        fastest = fastest.min(started.elapsed());
        last = Some(made);
    }
    (fastest, last.expect("at least one run"))
}

fn main() {
    let songs = measured(&songs());
    let mut kept = String::new();
    let mut bounding = Duration::ZERO;
    let mut folding = Duration::ZERO;
    for (curve, profile) in curves() {
        let (made, gains) = fastest(1, || {
            BandGains::of(&Applied::from(Arc::clone(&profile)), SampleRate::HZ_44100)
                .expect("gains under a curve")
        });
        let (alone, bounds) = fastest(RUNS, || {
            songs
                .iter()
                .map(|(_, peaks)| black_box(gains.bound(peaks)))
                .collect::<Vec<f64>>()
        });
        let (folded, loudest) = fastest(RUNS, || {
            let mut loudest = Loudest::default();
            for (_, peaks) in songs.iter().cycle().take(SONGS_FOLDED) {
                loudest.weigh(&gains, peaks);
            }
            loudest
        });

        bounding += alone;
        folding += folded;
        println!(
            "{curve:>16}: gains {:7.1} ms, bound {:8.1} us a song, fold {:8.1} us a song",
            made.as_secs_f64() * 1e3,
            per_song(alone, songs.len()),
            per_song(folded, SONGS_FOLDED),
        );
        for ((song, _), bound) in songs.iter().zip(&bounds) {
            writeln!(kept, "{curve}\t{song}\t{:016x}\t{bound}", bound.to_bits()).expect("kept");
        }
        writeln!(
            kept,
            "{curve}\tfold\t{:016x}\t{}",
            loudest.peak.to_bits(),
            loudest.peak
        )
        .expect("kept");
    }
    println!(
        "{:>16}: bound {:8.1} us a song, fold {:8.1} us a song",
        "all",
        per_song(bounding, songs.len() * curves().len()),
        per_song(folding, SONGS_FOLDED * curves().len()),
    );
    let (heard, _) = fastest(1, || {
        BandGains::of(
            &Applied::from(octaves([
                6.0, 5.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 5.0, 6.0,
            ]))
            .corrected_by(Some(reverberant())),
            SampleRate::HZ_44100,
        )
        .expect("gains under a curve and a response")
    });
    println!(
        "{:>16}: gains {:7.1} ms",
        "ten-second room",
        heard.as_secs_f64() * 1e3
    );
    if let Some(path) = env::var_os(KEPT_BOUNDS) {
        fs::write(path, kept).expect("bounds written");
    }
}
