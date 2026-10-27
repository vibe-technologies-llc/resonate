use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use resonate_analysis::{Coding, Finding, Loudness, Verdict, Watch, study};
use resonate_codec::Sources;
use resonate_core::MediaLocation;

const STEREO: u32 = 0b11;
const FIVE_ONE_WITH_REARS: u32 = 0x3F;
const FIVE_ONE_WITH_SIDES: u32 = 0x60F;

const FLOAT_SUBFORMAT: [u8; 16] = [
    0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71,
];
const EXTENSIBLE: u16 = 0xFFFE;

const PROGRAMME_SECONDS: u32 = 30;
const PROGRAMME_STEP_SECONDS: u32 = 3;
const PROGRAMME_STEPS_DB: [f32; 7] = [-20.0, -30.0, -16.0, -26.0, -22.0, -34.0, -18.0];
const PROGRAMME_RATES: [u32; 5] = [44_100, 48_000, 88_200, 96_000, 192_000];

const INTEGRATED_AGREES_WITHIN: f32 = 0.1;
const RANGE_AGREES_WITHIN: f32 = 0.2;
const TRUE_PEAK_AGREES_WITHIN: f32 = 0.2;
const TONE_AGREES_WITHIN: f32 = 0.1;
const TONE_HZ: f32 = 997.0;

const PRINT_SECONDS: u32 = 20;
const PRINT_RATE: u32 = 11_025;
const PRINT_ALGORITHM: &str = "2";

const MUSIC_RATE: u32 = 44_100;
const MUSIC_SECONDS: u32 = 30;
const AAC_BITRATES: [&str; 2] = ["256k", "320k"];
const FRAMES_CUT_FROM_THE_DECODE: u32 = 1_000;
const SOURCE_LATTICE_BELOW: f32 = 0.01;
const MP3_ENCODES: [(&str, &[&str]); 2] = [
    ("320-open", &["-b", "320", "--lowpass", "-1"]),
    ("v0", &["-V0"]),
];
const VORBIS_QUALITIES: [&str; 2] = ["6", "10"];

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "resonate-references-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a scratch tree");
        Self { root }
    }

    fn wave(&self, name: &str, rate: u32, mask: u32, interleaved: &[f32]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, float_wave(rate, mask, interleaved)).expect("a written fixture");
        path
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

fn float_wave(rate: u32, mask: u32, interleaved: &[f32]) -> Vec<u8> {
    let channels = mask.count_ones() as u16;
    let block_align = channels * 4;

    let mut format = Vec::new();
    format.extend_from_slice(&EXTENSIBLE.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&rate.to_le_bytes());
    format.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&32_u16.to_le_bytes());
    format.extend_from_slice(&22_u16.to_le_bytes());
    format.extend_from_slice(&32_u16.to_le_bytes());
    format.extend_from_slice(&mask.to_le_bytes());
    format.extend_from_slice(&FLOAT_SUBFORMAT);

    let data: Vec<u8> = interleaved
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
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

struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1 << 23) as f32 - 1.0
    }
}

fn decibels(db: f32) -> f32 {
    10_f32.powf(db / 20.0)
}

fn programme(rate: u32, channels: usize) -> Vec<f32> {
    let mut noises: Vec<Noise> = (0..channels)
        .map(|channel| Noise(0x2545_f491 ^ (channel as u32).wrapping_mul(0x9e37_79b9)))
        .collect();
    let mut smoothed = vec![0.0_f32; channels];
    let step_frames = (rate * PROGRAMME_STEP_SECONDS) as usize;

    (0..(rate * PROGRAMME_SECONDS) as usize)
        .flat_map(|frame| {
            let gain = decibels(PROGRAMME_STEPS_DB[frame / step_frames % PROGRAMME_STEPS_DB.len()]);
            let time = frame as f32 / rate as f32;
            let mut samples = Vec::with_capacity(channels);
            for channel in 0..channels {
                smoothed[channel] = smoothed[channel] * 0.9 + noises[channel].next() * 0.1;
                let tone = (time * std::f32::consts::TAU * (220.0 * (channel + 1) as f32)).sin();
                samples.push(gain * (smoothed[channel] * 4.0 + tone * 0.5));
            }
            samples
        })
        .collect()
}

fn tone(rate: u32, seconds: &[(u32, f32)]) -> Vec<f32> {
    let mut frame = 0_usize;
    seconds
        .iter()
        .flat_map(|(length, db)| {
            let gain = decibels(*db);
            let from = frame;
            frame += (rate * length) as usize;
            (from..frame).flat_map(move |at| {
                let sample =
                    gain * (at as f32 * std::f32::consts::TAU * TONE_HZ / rate as f32).sin();
                [sample, sample]
            })
        })
        .collect()
}

fn studied(path: &Path) -> resonate_analysis::Study {
    let location = MediaLocation::local(path);
    study(&Sources::local(), &location, None, &Watch::default()).expect("a study")
}

fn integrated(loudness: &Loudness) -> f32 {
    loudness.integrated.expect("an integrated loudness")
}

struct Reference {
    integrated: f32,
    range: f32,
    true_peak: f32,
}

fn ebur128_of(path: &Path) -> Option<Reference> {
    let output = Command::new("ffmpeg")
        .args(["-nostats", "-hide_banner", "-i"])
        .arg(path)
        .args(["-af", "ebur128=peak=true", "-f", "null", "-"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let report = String::from_utf8_lossy(&output.stderr);
    let summary = &report[report.rfind("Summary:")?..];
    let number_after = |label: &str| -> Option<f32> {
        let line = summary
            .lines()
            .find(|line| line.trim_start().starts_with(label))?;
        line.split_whitespace().nth(1)?.parse().ok()
    };

    Some(Reference {
        integrated: number_after("I:")?,
        range: number_after("LRA:")?,
        true_peak: number_after("Peak:")?,
    })
}

fn agrees(what: &str, ours: f32, reference: f32, within: f32) {
    assert!(
        (ours - reference).abs() <= within,
        "{what}: ours {ours}, the reference {reference}"
    );
}

#[test]
fn a_stereo_sine_reads_its_own_level_in_loudness_units_at_every_rate() {
    let tree = Tree::new();

    for rate in PROGRAMME_RATES {
        for level in [-23.0, -33.0] {
            let path = tree.wave("tone.wav", rate, STEREO, &tone(rate, &[(20, level)]));

            agrees(
                &format!("{level} dBFS at {rate} Hz"),
                integrated(&studied(&path).loudness),
                level,
                TONE_AGREES_WITHIN,
            );
        }
    }
}

#[test]
fn quiet_stretches_beside_a_loud_one_are_gated_out_as_the_tech_3341_cases_say() {
    let tree = Tree::new();
    let cases: [&[(u32, f32)]; 3] = [
        &[(10, -36.0), (60, -23.0), (10, -36.0)],
        &[
            (10, -72.0),
            (10, -36.0),
            (60, -23.0),
            (10, -36.0),
            (10, -72.0),
        ],
        &[(20, -26.0), (20, -20.0), (20, -26.0)],
    ];

    for rate in [44_100, 48_000] {
        for (number, case) in cases.iter().enumerate() {
            let path = tree.wave("gated.wav", rate, STEREO, &tone(rate, case));

            agrees(
                &format!("case {number} at {rate} Hz"),
                integrated(&studied(&path).loudness),
                -23.0,
                TONE_AGREES_WITHIN,
            );
        }
    }
}

#[test]
fn a_range_is_the_distance_between_the_levels_a_stretch_steps_between() {
    let tree = Tree::new();
    let cases: [(&[(u32, f32)], f32); 2] = [
        (&[(20, -20.0), (20, -30.0)], 10.0),
        (&[(20, -20.0), (20, -15.0)], 5.0),
    ];

    for (case, range) in cases {
        let path = tree.wave("range.wav", 48_000, STEREO, &tone(48_000, case));

        agrees(
            "the range",
            studied(&path).loudness.range.expect("a range"),
            range,
            1.0,
        );
    }
}

fn agrees_with_ebur128(tree: &Tree, name: &str, rate: u32, mask: u32) {
    let channels = mask.count_ones() as usize;
    let path = tree.wave(name, rate, mask, &programme(rate, channels));
    let Some(reference) = ebur128_of(&path) else {
        eprintln!("skipped: no ffmpeg to measure {name} against");
        return;
    };
    let loudness = studied(&path).loudness;

    agrees(
        &format!("{name} integrated"),
        integrated(&loudness),
        reference.integrated,
        INTEGRATED_AGREES_WITHIN,
    );
    agrees(
        &format!("{name} range"),
        loudness.range.expect("a range"),
        reference.range,
        RANGE_AGREES_WITHIN,
    );
    agrees(
        &format!("{name} true peak"),
        20.0 * loudness.true_peak.log10(),
        reference.true_peak,
        TRUE_PEAK_AGREES_WITHIN,
    );
}

#[test]
fn stereo_programme_reads_as_ffmpegs_ebur128_does_at_every_rate() {
    let tree = Tree::new();

    for rate in PROGRAMME_RATES {
        agrees_with_ebur128(&tree, &format!("stereo-{rate}.wav"), rate, STEREO);
    }
}

#[test]
fn five_one_programme_reads_as_ffmpegs_ebur128_does_with_rears_or_sides() {
    let tree = Tree::new();

    agrees_with_ebur128(&tree, "five-one-rears.wav", 48_000, FIVE_ONE_WITH_REARS);
    agrees_with_ebur128(&tree, "five-one-sides.wav", 48_000, FIVE_ONE_WITH_SIDES);
}

fn fpcalc_print_of(path: &Path) -> Option<String> {
    let output = Command::new("fpcalc")
        .args(["-algorithm", PRINT_ALGORITHM, "-length"])
        .arg(PRINT_SECONDS.to_string())
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("FINGERPRINT="))
        .map(str::to_owned)
}

#[test]
fn a_print_of_a_clip_at_chromaprints_own_rate_is_the_one_fpcalc_makes() {
    let tree = Tree::new();
    let mut noise = Noise(0x1234_5678);
    let mut smoothed = 0.0_f32;
    let mono: Vec<f32> = (0..PRINT_RATE * PRINT_SECONDS)
        .map(|frame| {
            smoothed = smoothed * 0.8 + noise.next() * 0.2;
            let time = frame as f32 / PRINT_RATE as f32;
            0.25 * smoothed + 0.3 * (time * std::f32::consts::TAU * (300.0 + 40.0 * time)).sin()
        })
        .collect();
    let interleaved: Vec<f32> = mono.iter().flat_map(|sample| [*sample, *sample]).collect();
    let path = tree.wave("print.wav", PRINT_RATE, STEREO, &interleaved);

    let Some(reference) = fpcalc_print_of(&path) else {
        eprintln!("skipped: no fpcalc to print against");
        return;
    };
    let printed = studied(&path).print.expect("a print");

    assert_eq!(
        printed.length(),
        Duration::from_secs(u64::from(PRINT_SECONDS))
    );
    assert_eq!(printed.encoded(), reference.trim_end_matches('='));
}

fn wideband_music(rate: u32, seconds: u32) -> Vec<f32> {
    let mut noises = [Noise(0x0bad_cafe), Noise(0x5eed_1e55)];
    let mut swell = Noise(0x0dd_ba11);
    let mut gain = 0.2_f32;
    let beat = rate as usize / 4;
    (0..(rate * seconds) as usize)
        .flat_map(|frame| {
            if frame % beat == 0 {
                gain = 0.05 + 0.15 * (swell.next() + 1.0);
            }
            let time = frame as f32 / rate as f32;
            let decay = (-((frame % beat) as f32) / beat as f32 * 3.0).exp();
            let melody =
                (time * std::f32::consts::TAU * (330.0 + 110.0 * (time * 0.7).sin())).sin();
            let shared = noises[0].next();
            let apart = noises[1].next();
            let left = gain * decay * (0.6 * shared + 0.4 * apart) + 0.2 * melody;
            let right = gain * decay * (0.6 * shared - 0.4 * apart) + 0.15 * melody;
            [left, right]
        })
        .collect()
}

fn ffmpeg(arguments: &[&OsStr]) -> Option<()> {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(arguments)
        .status()
        .ok()?;
    status.success().then_some(())
}

fn lame(arguments: &[&OsStr]) -> Option<()> {
    let status = Command::new("lame")
        .arg("--quiet")
        .args(arguments)
        .status()
        .ok()?;
    status.success().then_some(())
}

fn as_flac(from: &Path, to: &Path, filter: Option<String>) -> Option<()> {
    let mut arguments = vec![OsStr::new("-i"), from.as_os_str()];
    let filter = filter.map(OsString::from);
    if let Some(filter) = &filter {
        arguments.extend([OsStr::new("-af"), filter.as_os_str()]);
    }
    arguments.extend(["-sample_fmt", "s16", "-c:a", "flac"].map(OsStr::new));
    arguments.push(to.as_os_str());
    ffmpeg(&arguments)
}

fn as_wave(from: &Path, to: &Path) -> Option<()> {
    ffmpeg(&[
        "-i".as_ref(),
        from.as_os_str(),
        "-sample_fmt".as_ref(),
        "s16".as_ref(),
        to.as_os_str(),
    ])
}

fn quantiser_steps_of(study: &resonate_analysis::Study) -> Option<(Coding, Option<u32>)> {
    study
        .judgement
        .findings
        .iter()
        .find_map(|finding| match finding {
            Finding::QuantiserSteps {
                coding,
                resampled_from,
                ..
            } => Some((*coding, *resampled_from)),
            _ => None,
        })
}

fn assert_a_genuine_source(path: &Path) {
    let study = studied(path);

    assert_eq!(quantiser_steps_of(&study), None, "{study:?}");
    assert_ne!(study.judgement.verdict, Verdict::Fake, "{study:?}");
    assert!(!study.steps.is_empty(), "a lossless source is weighed");
    for steps in &study.steps {
        assert!(steps.lattice < SOURCE_LATTICE_BELOW, "{steps:?}");
    }
}

fn assert_decoded_from(path: &Path, coding: Coding, resampled_from: Option<u32>, what: &str) {
    let study = studied(path);

    assert_eq!(
        quantiser_steps_of(&study),
        Some((coding, resampled_from)),
        "{what}: {:?}",
        study.steps
    );
    assert_eq!(study.judgement.verdict, Verdict::Fake, "{what}");
}

struct Sourced {
    tree: Tree,
    source: PathBuf,
    wave: PathBuf,
}

fn music_source(rate: u32) -> Option<Sourced> {
    let tree = Tree::new();
    let made = tree.wave(
        "music.wav",
        rate,
        STEREO,
        &wideband_music(rate, MUSIC_SECONDS),
    );
    let source = tree.root.join("source.flac");
    let wave = tree.root.join("source.wav");
    as_flac(&made, &source, None)?;
    as_wave(&source, &wave)?;
    Some(Sourced { tree, source, wave })
}

#[test]
fn ffmpegs_aac_at_256_and_320_kbps_decoded_back_to_flac_lands_on_its_quantiser_steps_and_the_source_does_not()
 {
    let Some(Sourced { tree, source, .. }) = music_source(MUSIC_RATE) else {
        eprintln!("skipped: no ffmpeg to encode AAC with");
        return;
    };

    assert_a_genuine_source(&source);

    for bitrate in AAC_BITRATES {
        let encoded = tree.root.join(format!("{bitrate}.m4a"));
        ffmpeg(&[
            "-i".as_ref(),
            source.as_os_str(),
            "-c:a".as_ref(),
            "aac".as_ref(),
            "-b:a".as_ref(),
            bitrate.as_ref(),
            encoded.as_os_str(),
        ])
        .expect("an AAC encode");
        let whole = tree.root.join(format!("{bitrate}.flac"));
        let cut = tree.root.join(format!("{bitrate}-cut.flac"));
        as_flac(&encoded, &whole, None).expect("a decode");
        as_flac(
            &encoded,
            &cut,
            Some(format!("atrim=start_sample={FRAMES_CUT_FROM_THE_DECODE}")),
        )
        .expect("a cut decode");

        for decoded in [&whole, &cut] {
            assert_decoded_from(decoded, Coding::Aac, None, bitrate);
        }
    }
}

#[test]
fn an_aac_decode_resampled_to_the_other_cd_rate_lands_on_aacs_steps_once_brought_back() {
    for (rate, other) in [(MUSIC_RATE, 48_000), (48_000, MUSIC_RATE)] {
        let Some(Sourced { tree, source, .. }) = music_source(rate) else {
            eprintln!("skipped: no ffmpeg to encode AAC with");
            return;
        };
        let resampled_source = tree.root.join("resampled-source.flac");
        as_flac(
            &source,
            &resampled_source,
            Some(format!("aresample={other}")),
        )
        .expect("a resampled source");

        assert_a_genuine_source(&resampled_source);

        let encoded = tree.root.join("256k.m4a");
        ffmpeg(&[
            "-i".as_ref(),
            source.as_os_str(),
            "-c:a".as_ref(),
            "aac".as_ref(),
            "-b:a".as_ref(),
            "256k".as_ref(),
            encoded.as_os_str(),
        ])
        .expect("an AAC encode");
        let resampled = tree.root.join("resampled.flac");
        as_flac(&encoded, &resampled, Some(format!("aresample={other}"))).expect("a decode");

        assert_decoded_from(&resampled, Coding::Aac, Some(rate), "resampled AAC");
    }
}

#[test]
fn lames_mp3_with_no_lowpass_decoded_back_to_flac_lands_on_its_granules_and_the_source_does_not() {
    let Some(Sourced { tree, source, wave }) = music_source(MUSIC_RATE) else {
        eprintln!("skipped: no ffmpeg to encode MP3 with");
        return;
    };
    let probe = tree.root.join("probe.mp3");
    if lame(&[
        "-b".as_ref(),
        "128".as_ref(),
        wave.as_os_str(),
        probe.as_os_str(),
    ])
    .is_none()
    {
        eprintln!("skipped: no lame to encode MP3 with");
        return;
    }

    assert_a_genuine_source(&source);

    for (name, flags) in MP3_ENCODES {
        let encoded = tree.root.join(format!("{name}.mp3"));
        let mut arguments: Vec<&OsStr> = flags.iter().map(OsStr::new).collect();
        arguments.extend([wave.as_os_str(), encoded.as_os_str()]);
        lame(&arguments).expect("an MP3 encode");
        let whole = tree.root.join(format!("{name}.flac"));
        let cut = tree.root.join(format!("{name}-cut.flac"));
        let resampled = tree.root.join(format!("{name}-resampled.flac"));
        as_flac(&encoded, &whole, None).expect("a decode");
        as_flac(
            &encoded,
            &cut,
            Some(format!("atrim=start_sample={FRAMES_CUT_FROM_THE_DECODE}")),
        )
        .expect("a cut decode");
        as_flac(&encoded, &resampled, Some("aresample=48000".to_owned())).expect("a decode");

        for decoded in [&whole, &cut] {
            assert_decoded_from(decoded, Coding::Mp3, None, name);
        }
        assert_decoded_from(&resampled, Coding::Mp3, Some(MUSIC_RATE), name);
    }
}

#[test]
fn libvorbis_with_no_lowpass_decoded_back_to_flac_lands_on_its_floor_and_the_source_does_not() {
    let Some(Sourced { tree, source, wave }) = music_source(MUSIC_RATE) else {
        eprintln!("skipped: no ffmpeg to encode Vorbis with");
        return;
    };

    assert_a_genuine_source(&source);

    for quality in VORBIS_QUALITIES {
        let encoded = tree.root.join(format!("q{quality}.ogg"));
        if ffmpeg(&[
            "-i".as_ref(),
            wave.as_os_str(),
            "-c:a".as_ref(),
            "libvorbis".as_ref(),
            "-q:a".as_ref(),
            quality.as_ref(),
            encoded.as_os_str(),
        ])
        .is_none()
        {
            eprintln!("skipped: no libvorbis in ffmpeg");
            return;
        }
        let whole = tree.root.join(format!("q{quality}.flac"));
        let cut = tree.root.join(format!("q{quality}-cut.flac"));
        as_flac(&encoded, &whole, None).expect("a decode");
        as_flac(
            &encoded,
            &cut,
            Some(format!("atrim=start_sample={FRAMES_CUT_FROM_THE_DECODE}")),
        )
        .expect("a cut decode");

        for decoded in [&whole, &cut] {
            assert_decoded_from(decoded, Coding::Vorbis, None, quality);
        }
    }
}

#[test]
fn loud_noise_and_dense_music_at_every_cd_rate_land_on_no_codecs_steps() {
    for rate in [MUSIC_RATE, 48_000] {
        let tree = Tree::new();
        let mut noise = Noise(0x5eed_cafe);
        let loud: Vec<f32> = (0..(rate * MUSIC_SECONDS * 2) as usize)
            .map(|_| 0.7 * noise.next())
            .collect();
        let made = [
            tree.wave("noise.wav", rate, STEREO, &loud),
            tree.wave(
                "music.wav",
                rate,
                STEREO,
                &wideband_music(rate, MUSIC_SECONDS),
            ),
        ];
        for made in made {
            let flac = made.with_extension("flac");
            if as_flac(&made, &flac, None).is_none() {
                eprintln!("skipped: no ffmpeg to write FLAC with");
                return;
            }

            assert_a_genuine_source(&flac);
        }
    }
}
