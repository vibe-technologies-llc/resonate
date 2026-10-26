use std::{
    env, fs,
    path::PathBuf,
    process,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use resonate_analysis::{
    BandGains, BandLevels, Cut, Error, Finding, LossyGuess, Ramp, Verdict, Watch, Watching,
    analyse, band_levels, loudest_through, peak_through, print, study,
};
use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, Frames, MediaLocation, SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use rustfft::{FftPlanner, num_complex::Complex};

const RATE: u32 = 44_100;
const SECONDS: u32 = 12;
const PERIOD: usize = 65_536;

const RIFF_LENGTH_AT: usize = 4;
const DATA_LENGTH_AT: usize = 40;
const UNKNOWN_LENGTH: [u8; 4] = u32::MAX.to_le_bytes();

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

    fn streamed_wave(&self, name: &str, rate: u32, samples: &[i32]) -> MediaLocation {
        let mut streamed = wave(rate, 16, samples);
        streamed[RIFF_LENGTH_AT..RIFF_LENGTH_AT + 4].copy_from_slice(&UNKNOWN_LENGTH);
        streamed[DATA_LENGTH_AT..DATA_LENGTH_AT + 4].copy_from_slice(&UNKNOWN_LENGTH);
        let path = self.root.join(name);
        fs::write(&path, streamed).expect("a written fixture");
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
    let painted = analysis
        .spectrogram
        .painted(&Ramp::through(&[[0, 0, 0], [255, 255, 255]]));
    assert_eq!(painted.width as usize, analysis.spectrogram.columns());
}

#[test]
fn a_stereo_file_in_opposite_phase_is_judged_by_what_its_channels_hold() {
    let tree = Tree::new();
    let period = noise_up_to(RATE, 21_700.0);
    let opposed: Vec<i32> = (0..(RATE * SECONDS) as usize)
        .flat_map(|frame| {
            let sample = (period[frame % PERIOD] * 32_767.0) as i32;
            [sample, -sample]
        })
        .collect();
    let location = tree.wave("opposed.wav", RATE, 16, &opposed);

    let study = study(&Sources::local(), &location, None, &Watch::default()).expect("a study");

    assert_eq!(
        study.judgement.verdict,
        Verdict::Genuine,
        "{:?}",
        study.judgement
    );
    assert!(study.spectrum.heard() > Duration::from_secs(10));
}

#[test]
fn a_print_of_the_head_alone_is_the_print_the_whole_study_took() {
    let tree = Tree::new();
    let location = tree.wave("printed.wav", RATE, 16, &music(RATE, 16_000.0, 20_000.0));
    let studied = study(&Sources::local(), &location, None, &Watch::default())
        .expect("a study")
        .print
        .expect("a studied print");
    let printed = print(&Sources::local(), &location, None, &Watch::default())
        .expect("a print")
        .expect("a printed head");

    assert_eq!(printed.encoded(), studied.encoded());
}

#[test]
fn a_stream_that_declares_no_length_is_printed_as_long_as_it_plays() {
    let tree = Tree::new();
    let rate = 11_025;
    let seconds = 150;
    let period = noise_up_to(rate, 4_000.0);
    let samples: Vec<i32> = (0..(rate * seconds) as usize)
        .flat_map(|frame| {
            let sample = (period[frame % PERIOD] * 20_000.0) as i32;
            [sample, sample]
        })
        .collect();
    let location = tree.streamed_wave("streamed.wav", rate, &samples);

    let printed = print(&Sources::local(), &location, None, &Watch::default())
        .expect("a print")
        .expect("a printed stream");

    assert_eq!(printed.length(), Duration::from_secs(u64::from(seconds)));
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
fn a_transcode_splattered_above_its_wall_by_clipping_is_still_fake() {
    const SPLATTERED_EVERY_THIRD_WINDOW: usize = 3;
    const WINDOW: usize = 4_096;
    const SPLATTER: f32 = 900.0;
    let transcoded = music(RATE, 16_000.0, 20_000.0);
    let mut state = 0x9e37_79b9_u32;
    let splattered: Vec<i32> = transcoded
        .chunks(2)
        .enumerate()
        .flat_map(|(frame, pair)| {
            let burst = (frame / WINDOW).is_multiple_of(SPLATTERED_EVERY_THIRD_WINDOW);
            pair.iter()
                .map(|sample| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    let noise = (state as f32 / u32::MAX as f32 - 0.5) * 2.0 * SPLATTER;
                    if burst {
                        (*sample as f32 + noise) as i32
                    } else {
                        *sample
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let tree = Tree::new();
    let location = tree.wave("splattered.wav", RATE, 16, &splattered);
    let study = study(&Sources::local(), &location, None, &Watch::default()).expect("a study");

    assert_eq!(
        study.judgement.verdict,
        Verdict::Fake,
        "{:?}",
        study.judgement
    );
    let cutoff = study.judgement.cutoff.expect("a wall");
    assert!((15_700..=16_300).contains(&cutoff.hz), "{}", cutoff.hz);
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

const BOOSTED_AT_HZ: f64 = 100.0;
const BOOST_DB: f64 = 6.0;
const SOUNDED_ELSEWHERE_HZ: f64 = 5_000.0;
const WITHIN_DB: f64 = 0.4;

fn boosting_the_bass() -> Arc<Profile> {
    let boost = Band::new(
        BandKind::Peaking,
        Frequency::from_hertz(BOOSTED_AT_HZ).expect("in range"),
        BandGain::from_decibels(BOOST_DB).expect("in range"),
        Q::from_units(1.0).expect("in range"),
    );
    Arc::new(Profile::new(Preamp::NONE, vec![boost]).expect("a profile in range"))
}

fn tone(hertz: f64, amplitude: f64) -> Vec<i32> {
    (0..(RATE * 3) as usize)
        .flat_map(|frame| {
            let phase = std::f64::consts::TAU * hertz * frame as f64 / f64::from(RATE);
            let sample = (phase.sin() * amplitude * 32_767.0) as i32;
            [sample, sample]
        })
        .collect()
}

fn peak_db_through(profile: &Arc<Profile>, location: &MediaLocation) -> f64 {
    let peak = peak_through(
        &Sources::local(),
        location,
        None,
        profile,
        &Watch::default(),
    )
    .expect("a measured peak");
    20.0 * peak.log10()
}

#[test]
fn a_boost_where_the_music_has_no_energy_needs_less_preamp_than_the_curve_alone_says() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let modelled = profile.fitted_preamp(SampleRate::HZ_44100).decibels();
    let elsewhere = tree.wave("elsewhere.wav", RATE, 16, &tone(SOUNDED_ELSEWHERE_HZ, 0.8));

    let peak = peak_db_through(&profile, &elsewhere);

    assert!(modelled < -BOOST_DB + WITHIN_DB, "{modelled}");
    assert!((peak - 20.0 * 0.8_f64.log10()).abs() < WITHIN_DB, "{peak}");
    assert!(peak < 0.0, "{peak}");
}

#[test]
fn a_boost_where_the_music_has_energy_costs_the_headroom_the_boost_takes() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let on_the_boost = tree.wave("on-the-boost.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));

    let peak = peak_db_through(&profile, &on_the_boost);

    let expected = 20.0 * 0.8_f64.log10() + BOOST_DB;
    assert!(
        (peak - expected).abs() < WITHIN_DB,
        "{peak} against {expected}"
    );
}

#[test]
fn music_peaking_at_full_scale_on_the_boost_is_fitted_as_the_curve_alone_is() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let modelled = profile.fitted_preamp(SampleRate::HZ_44100).decibels();
    let loud = tree.wave("loud.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.9999));

    let measured = Preamp::holding_under_full_scale(peak_db_through(&profile, &loud)).decibels();

    assert!(
        (measured - modelled).abs() < WITHIN_DB,
        "{measured} against {modelled}"
    );
}

#[test]
fn the_fit_over_several_cuts_holds_the_loudest_and_counts_what_could_not_be_read() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let quiet = tree.wave("quiet.wav", RATE, 16, &tone(SOUNDED_ELSEWHERE_HZ, 0.3));
    let loud = tree.wave("loud.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));
    let missing = MediaLocation::local(tree.root.join("missing.wav"));
    let cuts: Vec<Cut> = [quiet, loud.clone(), missing]
        .into_iter()
        .map(|location| Cut {
            location,
            span: None,
        })
        .collect();

    let loudest = loudest_through(&Sources::local(), &cuts, &profile, &Watch::default())
        .expect("a measured set");

    assert_eq!(loudest.measured, 2);
    assert_eq!(loudest.passed_over, 1);
    let alone = peak_db_through(&profile, &loud);
    assert!((loudest.peak_db().expect("a peak") - alone).abs() < 0.01);
    assert_eq!(
        loudest.fitted_preamp(),
        Some(Preamp::holding_under_full_scale(alone))
    );
}

#[test]
fn a_stopped_watch_ends_the_fit_and_nothing_measured_fits_nothing() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let loud = tree.wave("loud.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));
    let cuts = [Cut {
        location: loud,
        span: None,
    }];

    let stopped = Watch::default();
    stopped.stop();
    assert!(matches!(
        loudest_through(&Sources::local(), &cuts, &profile, &stopped),
        Err(Error::Stopped)
    ));

    let nothing =
        loudest_through(&Sources::local(), &[], &profile, &Watch::default()).expect("an empty set");
    assert_eq!(nothing.fitted_preamp(), None);
}

fn peak_db_estimated_from_the_kept_bands(profile: &Arc<Profile>, location: &MediaLocation) -> f64 {
    let studied = study(&Sources::local(), location, None, &Watch::default()).expect("a study");
    let gains = BandGains::of(profile, SampleRate::HZ_44100);
    let peak = gains.peak(
        &BandLevels::of(&studied.spectrum),
        studied.loudness.true_peak,
    );
    20.0 * peak.log10()
}

#[test]
fn the_bands_measured_alone_are_the_bands_a_study_keeps() {
    let tree = Tree::new();
    let music = tree.wave("music.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));

    let alone = band_levels(&Sources::local(), &music, None, &Watch::default())
        .expect("bands measured alone");
    let studied = study(&Sources::local(), &music, None, &Watch::default()).expect("a study");

    assert_eq!(alone, BandLevels::of(&studied.spectrum));
    assert!(!alone.is_silent());
}

#[test]
fn a_stopped_watch_ends_the_measure_of_the_bands() {
    let tree = Tree::new();
    let music = tree.wave("music.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));
    let stopped = Watch::default();
    stopped.stop();

    assert!(matches!(
        band_levels(&Sources::local(), &music, None, &stopped),
        Err(Error::Stopped)
    ));
}

#[test]
fn music_sounding_on_the_boost_is_estimated_from_its_bands_as_the_decode_measures_it() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let on_the_boost = tree.wave("on-the-boost.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));

    let estimated = peak_db_estimated_from_the_kept_bands(&profile, &on_the_boost);
    let decoded = peak_db_through(&profile, &on_the_boost);

    assert!(
        (estimated - decoded).abs() < WITHIN_DB,
        "{estimated} against {decoded}"
    );
}

#[test]
fn music_sounding_away_from_the_boost_is_estimated_from_its_bands_as_the_decode_measures_it() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let elsewhere = tree.wave("elsewhere.wav", RATE, 16, &tone(SOUNDED_ELSEWHERE_HZ, 0.8));

    let estimated = peak_db_estimated_from_the_kept_bands(&profile, &elsewhere);
    let decoded = peak_db_through(&profile, &elsewhere);

    assert!(
        (estimated - decoded).abs() < WITHIN_DB,
        "{estimated} against {decoded}"
    );
    assert!(estimated < 0.0, "{estimated}");
}
