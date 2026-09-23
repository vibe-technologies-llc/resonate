use std::{
    env, fs,
    path::PathBuf,
    process,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use resonate_analysis::{
    Error, Finding, LossyGuess, Ramp, Verdict, Watch, Watching, analyse, study,
};
use resonate_codec::Sources;
use resonate_core::{FrameSpan, Frames, MediaLocation};
use rustfft::{FftPlanner, num_complex::Complex};

const RATE: u32 = 44_100;
const SECONDS: u32 = 12;
const PERIOD: usize = 65_536;

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "resonate-analysis-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a scratch tree");
        Self { root }
    }

    fn wave(&self, name: &str, rate: u32, bits: u16, samples: &[i32]) -> MediaLocation {
        let path = self.root.join(name);
        fs::write(&path, wave(rate, bits, samples)).expect("a written fixture");
        MediaLocation::local(path)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
}

fn wave(rate: u32, bits: u16, samples: &[i32]) -> Vec<u8> {
    let channels = 2_u16;
    let width = usize::from(bits / 8);
    let block_align = channels * (bits / 8);
    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&rate.to_le_bytes());
    format.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&bits.to_le_bytes());

    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes().into_iter().take(width))
        .collect();

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut whole = Vec::new();
    whole.extend_from_slice(b"RIFF");
    whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
    whole.extend_from_slice(&body);
    whole
}

fn noise_up_to(rate: u32, ceiling_hz: f32) -> Vec<f32> {
    let bin_hz = rate as f32 / PERIOD as f32;
    let mut state = 0x2545_f491_u32;
    let mut spectrum = vec![Complex::<f32>::default(); PERIOD];
    for bin in 1..PERIOD / 2 {
        let hz = bin as f32 * bin_hz;
        if hz > ceiling_hz {
            break;
        }
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let phase = (state >> 8) as f32 / (1 << 24) as f32 * std::f32::consts::TAU;
        let size = 1.0 / hz.sqrt();
        spectrum[bin] = Complex::from_polar(size, phase);
        spectrum[PERIOD - bin] = spectrum[bin].conj();
    }
    FftPlanner::new()
        .plan_fft_inverse(PERIOD)
        .process(&mut spectrum);

    let peak = spectrum
        .iter()
        .map(|sample| sample.re.abs())
        .fold(0.0, f32::max);
    spectrum
        .iter()
        .map(|sample| sample.re / peak * 0.5)
        .collect()
}

fn music(rate: u32, ceiling_hz: f32, scale: f32) -> Vec<i32> {
    let period = noise_up_to(rate, ceiling_hz);
    (0..(rate * SECONDS) as usize)
        .flat_map(|frame| {
            let left = period[frame % PERIOD];
            let right = period[(frame + PERIOD / 3) % PERIOD];
            [(left * scale) as i32, (right * scale) as i32]
        })
        .collect()
}

#[test]
fn band_limited_noise_reaching_the_top_is_genuine_and_everything_is_drawn() {
    let tree = Tree::new();
    let location = tree.wave("genuine.wav", RATE, 16, &music(RATE, 21_700.0, 32_767.0));
    let analysis =
        analyse(&Sources::local(), &location, None, &Watch::default()).expect("an analysis");
    let study = &analysis.study;

    assert_eq!(
        study.judgement.verdict,
        Verdict::Genuine,
        "{:?}",
        study.judgement
    );
    assert_eq!(study.examined.rate.hz(), RATE);
    assert_eq!(study.examined.channels, 2);
    assert_eq!(study.examined.declared_bits, Some(16));
    assert_eq!(
        study.examined.length,
        Duration::from_secs(u64::from(SECONDS))
    );
    assert_eq!(study.levels.bits_in_use, Some(16));
    assert!(
        (study.levels.peak - 0.5).abs() < 0.01,
        "{}",
        study.levels.peak
    );
    assert!(study.loudness.integrated.is_some());
    assert!(study.print.is_some());

    assert_eq!(analysis.envelope.lanes(), 2);
    assert_eq!(
        analysis.envelope.frames(),
        Frames(u64::from(RATE * SECONDS))
    );
    assert_eq!(analysis.envelope.condensed(1, 300).len(), 300);
    assert!(analysis.spectrogram.columns() > 100);
    assert_eq!(analysis.spectrogram.nyquist_hz(), RATE / 2);
    analysis
        .spectrogram
        .painted(&Ramp::through(&[[0, 0, 0], [255, 255, 255]]))
        .expect("a painted spectrogram");
}

#[test]
fn a_flac_that_was_once_a_128_kbps_mp3_is_fake() {
    let tree = Tree::new();
    let location = tree.wave("transcoded.wav", RATE, 16, &music(RATE, 16_000.0, 32_767.0));
    let study = study(&Sources::local(), &location, None, &Watch::default()).expect("a study");

    assert_eq!(
        study.judgement.verdict,
        Verdict::Fake,
        "{:?}",
        study.judgement
    );
    let cutoff = study.judgement.cutoff.expect("a wall");
    assert!((15_700..=16_300).contains(&cutoff.hz), "{}", cutoff.hz);
    assert_eq!(study.judgement.lossy_guess(), Some(LossyGuess::Near128));
}

#[test]
fn sixteen_bit_audio_padded_into_twenty_four_bits_is_fake() {
    let tree = Tree::new();
    let padded: Vec<i32> = music(RATE, 21_700.0, 32_767.0)
        .into_iter()
        .map(|sample| sample << 8)
        .collect();
    let location = tree.wave("padded.wav", RATE, 24, &padded);
    let study = study(&Sources::local(), &location, None, &Watch::default()).expect("a study");

    assert_eq!(study.examined.declared_bits, Some(24));
    assert_eq!(study.judgement.verdict, Verdict::Fake);
    assert!(study.judgement.findings.contains(&Finding::Padded {
        effective: 16,
        declared: 24
    }));
}

#[test]
fn a_cut_is_analysed_alone_and_a_stopped_watch_ends_the_pass() {
    let tree = Tree::new();
    let location = tree.wave("whole.wav", RATE, 16, &music(RATE, 21_700.0, 32_767.0));
    let span = FrameSpan::between(Frames(u64::from(RATE)), Frames(u64::from(RATE) * 3));
    let watch = Watch::default();
    let cut = study(&Sources::local(), &location, Some(span), &watch).expect("a study");
    assert_eq!(cut.examined.length, Duration::from_secs(2));
    assert_eq!(watch.share(), Some(1.0));

    let stopped = Watch::default();
    stopped.stop();
    assert!(matches!(
        study(&Sources::local(), &location, None, &stopped),
        Err(Error::Stopped)
    ));
    assert!(stopped.stopped());
}
