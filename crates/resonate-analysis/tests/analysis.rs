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
    Applied, BandGains, BandLevels, Cut, Error, Finding, LossyGuess, Ramp, Study, Verdict, Watch,
    Watching, analyse, band_figures, loudest_through, peak_through, print, span_of_band, study,
};
use resonate_codec::Sources;
use resonate_core::{
    FrameSpan, Frames, MediaLocation, SampleRate,
    eq::{Band, BandGain, BandKind, Frequency, Preamp, Profile, Q},
};
use resonate_dsp::Impulse;
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

const FADE_SECONDS: f64 = 0.1;

fn faded(position: f64) -> f64 {
    let fade = (position / FADE_SECONDS).clamp(0.0, 1.0);
    0.5 - 0.5 * (std::f64::consts::PI * fade).cos()
}

fn tone(hertz: f64, amplitude: f64) -> Vec<i32> {
    let frames = (RATE * 3) as usize;
    (0..frames)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let left = frames as f64 / f64::from(RATE) - seconds;
            let envelope = faded(seconds).min(faded(left));
            let phase = std::f64::consts::TAU * hertz * seconds;
            let sample = (phase.sin() * amplitude * envelope * 32_767.0) as i32;
            [sample, sample]
        })
        .collect()
}

fn peak_db_through(profile: &Arc<Profile>, location: &MediaLocation) -> f64 {
    peak_db_through_chain(&Applied::from(Arc::clone(profile)), location)
}

fn peak_db_through_chain(applied: &Applied, location: &MediaLocation) -> f64 {
    let peak = peak_through(
        &Sources::local(),
        location,
        None,
        applied,
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

    let loudest = loudest_through(
        &Sources::local(),
        &cuts,
        &Applied::from(Arc::clone(&profile)),
        &Watch::default(),
    )
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
        loudest_through(
            &Sources::local(),
            &cuts,
            &Applied::from(Arc::clone(&profile)),
            &stopped
        ),
        Err(Error::Stopped)
    ));

    let nothing = loudest_through(
        &Sources::local(),
        &[],
        &Applied::from(profile),
        &Watch::default(),
    )
    .expect("an empty set");
    assert_eq!(nothing.fitted_preamp(), None);
}

fn peak_db_bounded_from_the_kept_bands(profile: &Arc<Profile>, location: &MediaLocation) -> f64 {
    let studied = study(&Sources::local(), location, None, &Watch::default()).expect("a study");
    decibels_of_the_bound(&gains_under(profile), &studied)
}

const STEPS_A_DECIBEL_OF_A_LEVEL: f64 = 2.0;

fn peak_db_estimated_from_the_kept_levels(profile: &Arc<Profile>, location: &MediaLocation) -> f64 {
    let studied = study(&Sources::local(), location, None, &Watch::default()).expect("a study");
    let levels = BandLevels::of(&studied.spectrum);
    let at_the_rate = profile.at_rate(SampleRate::HZ_44100);
    let (shaped, heard) =
        levels
            .as_bytes()
            .iter()
            .enumerate()
            .fold((0.0, 0.0), |(shaped, heard), (band, step)| {
                if *step == u8::MAX {
                    return (shaped, heard);
                }
                let amplitude = 10_f64.powf(-f64::from(*step) / STEPS_A_DECIBEL_OF_A_LEVEL / 20.0);
                let (from, to) = span_of_band(band);
                let decibels = at_the_rate.peak_db_between(from, to, SampleRate::HZ_44100);
                let gain = if decibels.is_finite() {
                    10_f64.powf(decibels / 20.0)
                } else {
                    1.0
                };
                (shaped + amplitude * gain, heard + amplitude)
            });
    20.0 * (f64::from(studied.loudness.true_peak) * shaped / heard).log10()
}

#[test]
fn the_bands_measured_alone_are_the_bands_a_study_keeps() {
    let tree = Tree::new();
    let music = tree.wave("music.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));

    let alone = band_figures(&Sources::local(), &music, None, &Watch::default())
        .expect("bands measured alone");
    let studied = study(&Sources::local(), &music, None, &Watch::default()).expect("a study");

    assert_eq!(alone, studied.band_figures());
    assert_eq!(alone.levels, BandLevels::of(&studied.spectrum));
    assert!(!alone.levels.is_silent());
    assert!(!alone.peaks.is_silent());
}

#[test]
fn a_stopped_watch_ends_the_measure_of_the_bands() {
    let tree = Tree::new();
    let music = tree.wave("music.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));
    let stopped = Watch::default();
    stopped.stop();

    assert!(matches!(
        band_figures(&Sources::local(), &music, None, &stopped),
        Err(Error::Stopped)
    ));
}

#[test]
fn music_sounding_on_the_boost_is_bounded_by_its_bands_as_the_decode_measures_it() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let on_the_boost = tree.wave("on-the-boost.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));

    let bounded = peak_db_bounded_from_the_kept_bands(&profile, &on_the_boost);
    let decoded = peak_db_through(&profile, &on_the_boost);

    assert!(bounded >= decoded - 0.01, "{bounded} under {decoded}");
    assert!(bounded < decoded + 1.0, "{bounded} against {decoded}");
}

#[test]
fn music_sounding_away_from_the_boost_is_bounded_by_its_bands_as_the_decode_measures_it() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let elsewhere = tree.wave("elsewhere.wav", RATE, 16, &tone(SOUNDED_ELSEWHERE_HZ, 0.8));

    let bounded = peak_db_bounded_from_the_kept_bands(&profile, &elsewhere);
    let decoded = peak_db_through(&profile, &elsewhere);

    assert!(bounded >= decoded - 0.01, "{bounded} under {decoded}");
    assert!(bounded < decoded + 0.5, "{bounded} against {decoded}");
    assert!(bounded < 0.0, "{bounded}");
}

fn bass_bursts_over_a_steady_tone() -> Vec<i32> {
    let bursts_at = [0.5, 1.5, 2.5];
    let burst_seconds = 0.04;
    (0..(RATE * 3) as usize)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let steady = (std::f64::consts::TAU * SOUNDED_ELSEWHERE_HZ * seconds).sin() * 0.3;
            let burst: f64 = bursts_at
                .iter()
                .map(|at| {
                    let into = (seconds - at) / burst_seconds;
                    if !(0.0..1.0).contains(&into) {
                        return 0.0;
                    }
                    let swell = (std::f64::consts::PI * into).sin().powi(2);
                    swell * (std::f64::consts::TAU * 110.0 * seconds).sin() * 0.6
                })
                .sum();
            let sample = ((steady + burst) * 32_767.0) as i32;
            [sample, sample]
        })
        .collect()
}

#[test]
fn a_song_whose_peaks_gather_in_a_boosted_band_passes_the_estimate_and_not_the_bound() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let gathered = tree.wave("gathered.wav", RATE, 16, &bass_bursts_over_a_steady_tone());

    let decoded = peak_db_through(&profile, &gathered);
    let estimated = peak_db_estimated_from_the_kept_levels(&profile, &gathered);
    let bounded = peak_db_bounded_from_the_kept_bands(&profile, &gathered);

    assert!(
        estimated < decoded - 1.0,
        "{estimated} against {decoded}: the estimate was meant to fall short"
    );
    assert!(bounded >= decoded - 0.01, "{bounded} under {decoded}");
    assert!(
        Preamp::holding_under_full_scale(bounded).decibels()
            <= Preamp::holding_under_full_scale(decoded).decibels()
    );
    assert!(
        Preamp::holding_under_full_scale(estimated).decibels()
            > Preamp::holding_under_full_scale(decoded).decibels()
    );
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

fn two_tones_beating() -> Vec<i32> {
    (0..(RATE * 3) as usize)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let envelope = faded(seconds).min(faded(3.0 - seconds));
            let low = (std::f64::consts::TAU * 63.0 * seconds).sin() * 0.45;
            let high = (std::f64::consts::TAU * 2_500.0 * seconds).sin() * 0.4;
            let sample = ((low + high) * envelope * 32_767.0) as i32;
            [sample, sample]
        })
        .collect()
}

fn noise_between_fades() -> Vec<i32> {
    let mut samples = music(RATE, 18_000.0, 14_000.0);
    let frames = samples.len() / 2;
    for frame in 0..frames {
        let seconds = frame as f64 / f64::from(RATE);
        let envelope = faded(seconds).min(faded(SECONDS as f64 - seconds));
        for sample in &mut samples[frame * 2..frame * 2 + 2] {
            *sample = (f64::from(*sample) * envelope) as i32;
        }
    }
    samples
}

fn kick_hat_bass_and_pads() -> Vec<i32> {
    let mut state = 0x1234_5678_u32;
    let mut hiss = 0.0_f64;
    (0..(RATE * 4) as usize)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(RATE);
            let beat = seconds % 0.5;
            let kick = (std::f64::consts::TAU * (50.0 + 60.0 * (-beat * 30.0).exp()) * beat).sin()
                * (-beat * 12.0).exp()
                * 0.5;
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = f64::from(state >> 8) / f64::from(1_u32 << 24) - 0.5;
            let off = (seconds + 0.25) % 0.5;
            let hat = (white - hiss) * (-off * 60.0).exp() * 0.4;
            hiss = white;
            let bass = (std::f64::consts::TAU * 82.0 * seconds).sin() * 0.15;
            let pad = [330.0, 415.0, 494.0, 1_250.0, 2_600.0]
                .iter()
                .map(|hertz| (std::f64::consts::TAU * hertz * seconds).sin() * 0.06)
                .sum::<f64>();
            let envelope = faded(seconds).min(faded(4.0 - seconds));
            let sample = ((kick + hat + bass + pad) * envelope * 32_767.0) as i32;
            [sample, sample]
        })
        .collect()
}

fn clicks() -> Vec<i32> {
    (0..(RATE * 2) as usize)
        .flat_map(|frame| {
            let sample = match frame % 11_025 {
                0 => 26_000,
                1 => -20_000,
                _ => 0,
            };
            [sample, sample]
        })
        .collect()
}

fn octaves(gains: [f64; 10]) -> Arc<Profile> {
    let centres = [
        31.25, 62.5, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0,
    ];
    shaped(
        &centres
            .iter()
            .zip(gains)
            .map(|(hertz, gain)| (BandKind::Peaking, *hertz, gain, std::f64::consts::SQRT_2))
            .collect::<Vec<_>>(),
    )
}

fn bounded_under(applied: &Applied) -> BandGains {
    BandGains::of(applied, SampleRate::HZ_44100).expect("a bound under the chain")
}

fn gains_under(profile: &Arc<Profile>) -> BandGains {
    bounded_under(&Applied::from(Arc::clone(profile)))
}

fn decibels_of_the_bound(gains: &BandGains, studied: &Study) -> f64 {
    20.0 * gains.bound(&studied.band_peaks).log10()
}

#[test]
fn the_bound_holds_over_what_the_decode_measures_whatever_the_curve_and_where_the_music_sounds() {
    let tree = Tree::new();
    let songs = [
        (
            "tone",
            tree.wave("tone.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8)),
        ),
        (
            "high tone",
            tree.wave("high.wav", RATE, 16, &tone(SOUNDED_ELSEWHERE_HZ, 0.8)),
        ),
        (
            "bursts",
            tree.wave("bursts.wav", RATE, 16, &bass_bursts_over_a_steady_tone()),
        ),
        (
            "beats",
            tree.wave("beats.wav", RATE, 16, &two_tones_beating()),
        ),
        (
            "noise",
            tree.wave("noise.wav", RATE, 16, &noise_between_fades()),
        ),
        (
            "a band",
            tree.wave("band.wav", RATE, 16, &kick_hat_bass_and_pads()),
        ),
        ("clicks", tree.wave("clicks.wav", RATE, 16, &clicks())),
    ];
    let curves = [
        ("a bass boost", boosting_the_bass()),
        (
            "a narrow boost",
            shaped(&[(BandKind::Peaking, 63.0, 8.0, 4.0)]),
        ),
        (
            "a low shelf",
            shaped(&[(BandKind::LowShelf, 200.0, 6.0, 0.7)]),
        ),
        (
            "a high shelf",
            shaped(&[(BandKind::HighShelf, 4_000.0, 5.0, 0.7)]),
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
            "a smile",
            octaves([6.0, 5.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 5.0, 6.0]),
        ),
        (
            "a ragged curve",
            octaves([4.0, -3.0, 5.0, -2.0, 3.0, -4.0, 6.0, -1.0, 4.0, -3.0]),
        ),
        (
            "a tilt",
            octaves([-4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
        ),
        (
            "a ringing low boost",
            shaped(&[(BandKind::Peaking, 40.0, 10.0, 12.0)]),
        ),
        (
            "a narrow treble boost",
            shaped(&[(BandKind::Peaking, 9_000.0, 9.0, 6.0)]),
        ),
        (
            "a deep notch beside a boost",
            shaped(&[
                (BandKind::Notch, 1_000.0, 0.0, 2.0),
                (BandKind::Peaking, 1_400.0, 6.0, 3.0),
            ]),
        ),
        (
            "a turned phase",
            shaped(&[
                (BandKind::AllPass, 300.0, 0.0, 4.0),
                (BandKind::AllPass, 3_000.0, 0.0, 4.0),
                (BandKind::Peaking, 300.0, 4.0, 1.0),
            ]),
        ),
        (
            "a low pass",
            shaped(&[(BandKind::LowPass, 2_000.0, 0.0, 2.0)]),
        ),
    ];

    for (song, location) in &songs {
        let studied = study(&Sources::local(), location, None, &Watch::default()).expect("a study");
        for (curve, profile) in &curves {
            let gains = gains_under(profile);
            let decoded = peak_db_through(profile, location);
            let bounded = decibels_of_the_bound(&gains, &studied);
            assert!(
                bounded >= decoded - 0.02,
                "{song} under {curve}: {bounded} under {decoded}"
            );
        }
    }
}

fn corrections() -> Vec<(&'static str, Arc<Impulse>)> {
    let rate = SampleRate::HZ_44100;
    let impulse = |taps: Vec<f64>| Arc::new(Impulse::new(rate, vec![taps]).expect("an impulse"));
    let mut seed = 0x5eed_u64;
    let dispersive: Vec<f64> = (0..2_000)
        .map(|at| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let noise = (seed >> 11) as f64 / (1_u64 << 53) as f64 - 0.5;
            noise * (-(at as f64) / 300.0).exp() * 0.6
        })
        .collect();
    let mut echoing = vec![0.0; 2_206];
    echoing[0] = 0.7;
    echoing[2_205] = 0.5;
    let mut held_back = vec![0.0; 13_231];
    held_back[0] = 0.4;
    held_back[13_230] = 0.9;
    vec![
        ("a pair of taps boosting the bass", impulse(vec![1.0, 1.0])),
        ("a pair of taps cutting the treble", impulse(vec![0.5, 0.5])),
        ("an echo", impulse(echoing)),
        ("a dispersive burst", impulse(dispersive)),
        ("a response held back past a moment", impulse(held_back)),
        (
            "a linear phase low pass",
            impulse(low_pass_taps(2_000.0, 255)),
        ),
    ]
}

fn low_pass_taps(cutoff_hz: f64, taps: usize) -> Vec<f64> {
    let centre = (taps / 2) as f64;
    let cutoff = cutoff_hz / f64::from(RATE);
    (0..taps)
        .map(|at| {
            let offset = at as f64 - centre;
            let sinc = if offset == 0.0 {
                2.0 * cutoff
            } else {
                (std::f64::consts::TAU * cutoff * offset).sin() / (std::f64::consts::PI * offset)
            };
            let window = 0.5 - 0.5 * (std::f64::consts::TAU * at as f64 / (taps - 1) as f64).cos();
            sinc * window
        })
        .collect()
}

#[test]
fn the_bound_still_holds_over_what_the_decode_measures_with_a_correction_in_the_chain() {
    let tree = Tree::new();
    let songs = [
        (
            "tone",
            tree.wave("tone.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8)),
        ),
        (
            "bursts",
            tree.wave("bursts.wav", RATE, 16, &bass_bursts_over_a_steady_tone()),
        ),
        (
            "noise",
            tree.wave("noise.wav", RATE, 16, &noise_between_fades()),
        ),
        (
            "a band",
            tree.wave("band.wav", RATE, 16, &kick_hat_bass_and_pads()),
        ),
        ("clicks", tree.wave("clicks.wav", RATE, 16, &clicks())),
    ];
    let curves = [
        ("no curve", Arc::new(Profile::flat())),
        ("a bass boost", boosting_the_bass()),
        (
            "boosts and cuts",
            shaped(&[
                (BandKind::Peaking, 80.0, 5.0, 1.0),
                (BandKind::Peaking, 800.0, -4.0, 1.5),
                (BandKind::Peaking, 2_500.0, 6.0, 2.0),
            ]),
        ),
        (
            "a ringing low boost",
            shaped(&[(BandKind::Peaking, 40.0, 10.0, 12.0)]),
        ),
    ];

    for (song, location) in &songs {
        let studied = study(&Sources::local(), location, None, &Watch::default()).expect("a study");
        for (correction, impulse) in corrections() {
            for (curve, profile) in &curves {
                let applied =
                    Applied::from(Arc::clone(profile)).corrected_by(Some(impulse.clone()));
                let decoded = peak_db_through_chain(&applied, location);
                let bounded = decibels_of_the_bound(&bounded_under(&applied), &studied);
                assert!(
                    bounded >= decoded - 0.02,
                    "{song} under {curve} and {correction}: {bounded} under {decoded}"
                );
            }
        }
    }
}

fn boosting_the_bass_twice_over() -> Arc<Impulse> {
    Arc::new(Impulse::new(SampleRate::HZ_44100, vec![vec![1.0, 1.0]]).expect("an impulse"))
}

#[test]
fn a_room_correction_that_boosts_where_the_music_sits_makes_the_fit_deeper_than_the_profile_alone()
{
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let on_the_boost = tree.wave("on-the-boost.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.4));
    let alone = Applied::from(Arc::clone(&profile));
    let corrected = alone
        .clone()
        .corrected_by(Some(boosting_the_bass_twice_over()));

    let without = peak_db_through_chain(&alone, &on_the_boost);
    let with = peak_db_through_chain(&corrected, &on_the_boost);

    assert!(
        (with - without - 6.0).abs() < WITHIN_DB,
        "{with} against {without}"
    );
    assert!(
        Preamp::holding_under_full_scale(with) < Preamp::holding_under_full_scale(without),
        "{with} against {without}"
    );

    let modelled = alone.fitted_preamp(SampleRate::HZ_44100).expect("a model");
    let modelled_through = corrected
        .fitted_preamp(SampleRate::HZ_44100)
        .expect("a model");
    assert!(
        modelled_through.decibels() < modelled.decibels() - 5.0,
        "{modelled_through:?} against {modelled:?}"
    );

    let studied =
        study(&Sources::local(), &on_the_boost, None, &Watch::default()).expect("a study");
    let bound_alone = decibels_of_the_bound(&bounded_under(&alone), &studied);
    let bound_through = decibels_of_the_bound(&bounded_under(&corrected), &studied);
    assert!(
        bound_through > bound_alone + 5.0,
        "{bound_through} against {bound_alone}"
    );
}

#[test]
fn a_room_correction_that_cuts_where_the_music_sits_needs_less_preamp_than_the_profile_alone() {
    let tree = Tree::new();
    let profile = boosting_the_bass();
    let loud = tree.wave("loud.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.9));
    let cutting =
        Arc::new(Impulse::new(SampleRate::HZ_44100, vec![vec![0.25, 0.25]]).expect("an impulse"));
    let alone = Applied::from(Arc::clone(&profile));
    let corrected = alone.clone().corrected_by(Some(cutting));

    let without = peak_db_through_chain(&alone, &loud);
    let with = peak_db_through_chain(&corrected, &loud);

    assert!(
        (without - with - 6.0).abs() < WITHIN_DB,
        "{with} against {without}"
    );
}

#[test]
fn a_response_holding_the_music_back_past_its_end_is_measured_in_the_tail_it_flushes() {
    let tree = Tree::new();
    let frames = 5_000;
    let at_the_end: Vec<i32> = (0..frames)
        .flat_map(|frame| {
            let burst = frame >= frames - 200;
            let seconds = frame as f64 / f64::from(RATE);
            let phase = std::f64::consts::TAU * 1_000.0 * seconds;
            let sample = if burst {
                (phase.sin() * 0.5 * 32_767.0) as i32
            } else {
                0
            };
            [sample, sample]
        })
        .collect();
    let song = tree.wave("at-the-end.wav", RATE, 16, &at_the_end);
    let mut held_back = vec![0.0; 3_001];
    held_back[3_000] = 2.0;
    let delayed =
        Arc::new(Impulse::new(SampleRate::HZ_44100, vec![held_back]).expect("an impulse"));
    let flat = Applied::from(Arc::new(Profile::flat()));

    let without = peak_db_through_chain(&flat, &song);
    let with = peak_db_through_chain(&flat.clone().corrected_by(Some(delayed)), &song);

    assert!(
        (with - without - 6.0).abs() < WITHIN_DB,
        "{with} against {without}"
    );
}

#[test]
fn broadband_music_under_a_curve_moving_every_octave_is_bounded_within_a_few_decibels_of_its_decode()
 {
    let tree = Tree::new();
    let band = tree.wave("band.wav", RATE, 16, &kick_hat_bass_and_pads());
    let studied = study(&Sources::local(), &band, None, &Watch::default()).expect("a study");

    for curve in [
        octaves([6.0, 5.0, 3.0, 1.0, -1.0, -1.0, 1.0, 3.0, 5.0, 6.0]),
        octaves([4.0, -3.0, 5.0, -2.0, 3.0, -4.0, 6.0, -1.0, 4.0, -3.0]),
        octaves([-4.0, -3.0, -2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0]),
    ] {
        let decoded = peak_db_through(&curve, &band);
        let bounded = decibels_of_the_bound(&gains_under(&curve), &studied);

        assert!(bounded >= decoded - 0.02, "{bounded} under {decoded}");
        assert!(bounded < decoded + 4.5, "{bounded} against {decoded}");
    }
}

#[test]
fn a_tone_beside_a_narrow_boost_is_bounded_near_its_decode() {
    let tree = Tree::new();
    let beside = tree.wave("beside.wav", RATE, 16, &tone(BOOSTED_AT_HZ, 0.8));
    let narrow = shaped(&[(BandKind::Peaking, 63.0, 8.0, 4.0)]);
    let studied = study(&Sources::local(), &beside, None, &Watch::default()).expect("a study");

    let decoded = peak_db_through(&narrow, &beside);
    let bounded = decibels_of_the_bound(&gains_under(&narrow), &studied);

    assert!(bounded >= decoded - 0.02, "{bounded} under {decoded}");
    assert!(bounded < decoded + 3.0, "{bounded} against {decoded}");
    assert!(bounded < 1.5, "{bounded}");
}
