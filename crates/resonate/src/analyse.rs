use std::{ffi::OsStr, path::Path};

use resonate_codec::{Sources, probe, read_cue_media};
use resonate_core::{FrameSpan, Frames, MediaLocation};
use resonate_engine::{Analysis, Envelope, Spectrum, Watch, analyse};
use resonate_library::{Fingerprinters, HeardAs, Sounded};

use crate::{
    Error, Result, from_here,
    info::{BLOCKS, clock, heading, pairs},
    location_of_argument, names_a_sheet,
    table::Table,
};

const PLOT_COLUMNS: usize = 72;
const PLOT_ROWS: usize = 10;
const WAVEFORM_ROWS_A_SIDE: usize = 6;
const WAVEFORM_RMS: char = '█';
const WAVEFORM_PEAK: char = '▒';
const SPECTRUM_FLOOR_DB: f32 = -130.0;
const PRINT_SHOWN: usize = 48;
const HZ_A_KILOHERTZ: f32 = 1_000.0;
const NO_ARTIST: &str = "-";

pub struct Analysed {
    location: MediaLocation,
    span: Option<FrameSpan>,
}

impl Analysed {
    pub fn named(argument: &OsStr, track: Option<u32>, sources: &Sources) -> Result<Self> {
        let cut = argument
            .to_str()
            .and_then(MediaLocation::from_uri_within)
            .filter(|(location, span)| {
                span.is_some()
                    && (location.as_path().is_some()
                        || sources.provider(location.source()).is_some())
            });
        if let Some((location, span)) = cut {
            return match track {
                Some(_) => Err(Error::TrackOutsideASheet { location }),
                None => Ok(Self { location, span }),
            };
        }

        let location = location_of_argument(argument, sources);
        let sheet = names_a_sheet(&location)
            .then(|| location.as_path().map(Path::to_path_buf))
            .flatten();
        match (sheet, track) {
            (None, None) => Ok(Self {
                location,
                span: None,
            }),
            (None, Some(_)) => Err(Error::TrackOutsideASheet { location }),
            (Some(sheet), None) => Err(Error::SheetWithoutATrack { sheet }),
            (Some(sheet), Some(number)) => Self::cut_by(sources, &sheet, number),
        }
    }

    fn cut_by(sources: &Sources, sheet: &Path, number: u32) -> Result<Self> {
        let cues = read_cue_media(sources, &MediaLocation::local(sheet))?;
        for file in &cues.files {
            let Some((index, _)) = file
                .audio_tracks()
                .find(|(_, track)| track.number == number)
            else {
                continue;
            };
            let Some(named) = sheet.parent().map(|folder| folder.join(&file.named)) else {
                continue;
            };
            let location = MediaLocation::local(from_here(&named));
            let info = probe(sources, &location)?;
            if let Some(span) = file.span_of(index, info.spec.rate, info.duration) {
                return Ok(Self {
                    location,
                    span: Some(span),
                });
            }
        }
        Err(Error::NoSuchSheetTrack {
            sheet: sheet.to_path_buf(),
            track: number,
        })
    }
}

pub fn print(
    analysed: &Analysed,
    sources: &Sources,
    recognising: Option<Fingerprinters>,
) -> Result<()> {
    let Analysed { location, span } = analysed;
    let analysis = analyse(sources, location, *span, &Watch::default())?;

    heading("STREAM");
    stream(&analysis, *span);

    heading("VERDICT");
    verdict(&analysis);

    heading("LEVELS");
    levels(&analysis);

    heading("SPECTRUM");
    spectrum(
        &analysis.study.spectrum,
        analysis.study.judgement.cutoff.map(|cutoff| cutoff.hz),
    );

    heading("WAVEFORM");
    waveform(&analysis.envelope);

    heading("PRINT");
    printed(&analysis);

    if let Some(fingerprinters) = recognising {
        heading("RECOGNISED");
        recognised(&analysis, analysed, &fingerprinters);
    }
    Ok(())
}

fn stream(analysis: &Analysis, span: Option<FrameSpan>) {
    let examined = &analysis.study.examined;
    let frames = analysis.envelope.frames();
    let cut = span.map(|span| {
        let end = span
            .end()
            .map_or_else(|| "the end".to_owned(), |end| clock(end, examined.rate));
        (
            "cut",
            format!("{} to {end}", clock(span.start(), examined.rate)),
        )
    });
    pairs(
        cut.into_iter()
            .chain([
                ("codec", examined.codec.to_string()),
                ("rate", format!("{} Hz", examined.rate.hz())),
                ("channels", examined.channels.to_string()),
                (
                    "declared depth",
                    examined
                        .declared_bits
                        .map_or_else(|| "none".to_owned(), |bits| format!("{bits} bits")),
                ),
                ("decoded as", examined.format.to_string()),
                ("length", clock(frames, examined.rate)),
            ])
            .collect(),
    );
}

fn verdict(analysis: &Analysis) {
    let judgement = &analysis.study.judgement;
    println!("  {}", judgement.verdict.told());
    for finding in &judgement.findings {
        println!("  · {}", finding.told());
    }
}

fn true_peak_of(level: f32) -> String {
    if level <= 0.0 {
        return "silent".to_owned();
    }
    format!("{:+.2} dBTP", 20.0 * level.log10())
}

fn lufs_of(lufs: Option<f32>) -> String {
    lufs.map_or_else(
        || "too quiet to measure".to_owned(),
        |lufs| format!("{lufs:.1} LUFS"),
    )
}

fn lu_of(range: Option<f32>) -> String {
    range.map_or_else(
        || "too quiet to measure".to_owned(),
        |lu| format!("{lu:.1} LU"),
    )
}

fn decibels_of(level: f32) -> String {
    if level <= 0.0 {
        return "silent".to_owned();
    }
    format!("{:+.2} dBFS", 20.0 * level.log10())
}

fn levels(analysis: &Analysis) {
    let study = &analysis.study;
    let levels = &study.levels;
    pairs(vec![
        ("peak", decibels_of(levels.peak)),
        ("rms", decibels_of(levels.rms)),
        ("true peak", true_peak_of(study.loudness.true_peak)),
        (
            "loudness",
            study.loudness.integrated.map_or_else(
                || "too quiet to measure".to_owned(),
                |lufs| format!("{lufs:.1} LUFS"),
            ),
        ),
        ("loudness range", lu_of(study.loudness.range)),
        ("momentary max", lufs_of(study.loudness.momentary_max)),
        ("short-term max", lufs_of(study.loudness.short_term_max)),
        (
            "dynamic range",
            study
                .loudness
                .dynamic_range
                .map_or_else(|| "unmeasured".to_owned(), |dr| format!("DR{dr}")),
        ),
        (
            "clipped",
            format!("{} samples in {} runs", levels.clipped, levels.clipped_runs),
        ),
        ("dc offset", decibels_of(levels.dc)),
        (
            "bits in use",
            levels
                .bits_in_use
                .map_or_else(|| "not measured".to_owned(), |bits| bits.to_string()),
        ),
        (
            "channels",
            levels.stereo.map_or_else(
                || "not stereo".to_owned(),
                |stereo| match (stereo.identical, stereo.correlation) {
                    (true, _) => "identical".to_owned(),
                    (false, Some(correlation)) => format!("correlated {correlation:+.2}"),
                    (false, None) => "silent".to_owned(),
                },
            ),
        ),
    ]);
}

fn bars(series: &[f32], floor: f32, top: f32, label: impl Fn(f32) -> String) {
    let span = (top - floor).max(f32::EPSILON);
    for row in (0..PLOT_ROWS).rev() {
        let at = floor + span * (row as f32 + 0.5) / PLOT_ROWS as f32;
        let line: String = series
            .iter()
            .map(|value| {
                let filled = (value - floor) / span * PLOT_ROWS as f32 - row as f32;
                BLOCKS[(filled.clamp(0.0, 1.0) * 8.0).round() as usize]
            })
            .collect();
        println!("  {:>9} │{line}", label(at));
    }
    println!("  {:>9} └{}", "", "─".repeat(series.len()));
}

fn spectrum(spectrum: &Spectrum, cutoff_hz: Option<u32>) {
    let nyquist = spectrum.nyquist_hz();
    let series: Vec<f32> = (0..PLOT_COLUMNS)
        .map(|column| {
            let from = nyquist * column as f32 / PLOT_COLUMNS as f32;
            let to = nyquist * (column + 1) as f32 / PLOT_COLUMNS as f32;
            spectrum
                .band_db(from, to)
                .unwrap_or(SPECTRUM_FLOOR_DB)
                .max(SPECTRUM_FLOOR_DB)
        })
        .collect();
    bars(&series, SPECTRUM_FLOOR_DB, 0.0, |db| format!("{db:.0} dB"));

    let marked: String = (0..PLOT_COLUMNS)
        .map(|column| {
            let hz = nyquist * (column as f32 + 0.5) / PLOT_COLUMNS as f32;
            let width = nyquist / PLOT_COLUMNS as f32;
            match cutoff_hz {
                Some(cutoff) if (hz - cutoff as f32).abs() <= width / 2.0 => '▲',
                _ => ' ',
            }
        })
        .collect();
    println!("  {:>9}  {marked}", "");
    println!(
        "  {:>9}  0 kHz{:>width$}",
        "",
        format!("{:.1} kHz", nyquist / HZ_A_KILOHERTZ),
        width = PLOT_COLUMNS.saturating_sub(5)
    );
}

fn waveform(envelope: &Envelope) {
    let lanes = envelope.lanes();
    let condensed: Vec<_> = (0..lanes)
        .map(|lane| envelope.condensed(lane, PLOT_COLUMNS))
        .collect();
    let columns: Vec<(f32, f32, f32)> = (0..PLOT_COLUMNS)
        .map(|column| {
            condensed.iter().filter_map(|lane| lane.get(column)).fold(
                (0.0, 0.0, 0.0),
                |(high, low, rms), reach| {
                    (
                        f32::max(high, reach.high),
                        f32::max(low, -reach.low),
                        f32::max(rms, reach.rms),
                    )
                },
            )
        })
        .collect();
    if columns.is_empty() || envelope.frames() == Frames::ZERO {
        println!("  nothing was heard");
        return;
    }

    let side = WAVEFORM_ROWS_A_SIDE as f32;
    let cell = |reached: f32, rms: f32, at: f32| {
        if rms >= at {
            WAVEFORM_RMS
        } else if reached >= at {
            WAVEFORM_PEAK
        } else {
            ' '
        }
    };
    for row in (0..WAVEFORM_ROWS_A_SIDE).rev() {
        let at = (row as f32 + 0.5) / side;
        let line: String = columns
            .iter()
            .map(|(high, _, rms)| cell(*high, *rms, at))
            .collect();
        println!(
            "  {:>9} │{line}",
            format!("{:+.2}", (row + 1) as f32 / side)
        );
    }
    println!("  {:>9} ┼{}", "0", "─".repeat(columns.len()));
    for row in 0..WAVEFORM_ROWS_A_SIDE {
        let at = (row as f32 + 0.5) / side;
        let line: String = columns
            .iter()
            .map(|(_, low, rms)| cell(*low, *rms, at))
            .collect();
        println!(
            "  {:>9} │{line}",
            format!("{:+.2}", -((row + 1) as f32) / side)
        );
    }
    println!("  {:>9}  {WAVEFORM_RMS} rms  {WAVEFORM_PEAK} peak", "");
}

fn printed(analysis: &Analysis) {
    let Some(print) = analysis.study.print.as_ref() else {
        println!("  the stream could not be fingerprinted");
        return;
    };
    let encoded = print.encoded();
    let shown: String = encoded.chars().take(PRINT_SHOWN).collect();
    let more = if encoded.len() > PRINT_SHOWN {
        "…"
    } else {
        ""
    };
    pairs(vec![
        ("chromaprint", format!("{shown}{more}")),
        ("characters", encoded.len().to_string()),
        ("length", format!("{} s", print.length().as_secs())),
    ]);
}

fn recognised(analysis: &Analysis, analysed: &Analysed, fingerprinters: &Fingerprinters) {
    if !fingerprinters.has_a_source() {
        println!(
            "  nothing to ask: set `acoustid-key` in config.toml, and leave `online` on, to \
             recognise what a file is"
        );
        return;
    }
    let Some(print) = analysis.study.print.clone() else {
        println!("  there is no print to ask with");
        return;
    };

    let recognition = fingerprinters.recognise(&Sounded {
        location: analysed.location.clone(),
        span: analysed.span,
        length: Some(analysis.study.examined.length),
        title: None,
        artist: None,
        print,
    });
    if recognition.refused && recognition.matches.is_empty() {
        println!("  the recognition service refused or could not be reached");
        return;
    }
    if recognition.matches.is_empty() {
        println!("  nothing the service holds sounds like this");
        return;
    }

    let mut table = Table::new(vec!["SCORE", "TITLE", "ARTIST", "RECORDING"]);
    for found in &recognition.matches {
        let heard = HeardAs::of(found);
        table.push(vec![
            heard.score.to_string(),
            heard.title,
            heard.artist.unwrap_or_else(|| NO_ARTIST.to_owned()),
            heard.recording.to_string(),
        ]);
    }
    print!("{}", table.render());
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsString, fs, path::PathBuf};

    use super::*;

    const RATE: u32 = 8_000;
    const FRAMES: u32 = RATE;
    const CD_FRAMES_A_SECOND: u32 = 75;
    const SECOND_TRACK_AT_CD_FRAMES: u32 = 30;

    fn silent_wave() -> Vec<u8> {
        let data = FRAMES * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&RATE.to_le_bytes());
        bytes.extend_from_slice(&(RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data.to_le_bytes());
        bytes.resize(bytes.len() + data as usize, 0);
        bytes
    }

    fn a_rip() -> PathBuf {
        let folder = std::env::temp_dir().join(format!("resonate-analyse-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        fs::write(folder.join("whole.wav"), silent_wave()).expect("a wave file");
        fs::write(
            folder.join("sheet.cue"),
            format!(
                "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 \
                 AUDIO\n    INDEX 01 00:00:{SECOND_TRACK_AT_CD_FRAMES}\n"
            ),
        )
        .expect("a sheet");
        folder
    }

    #[test]
    fn a_sheet_and_a_uri_each_name_one_cut_and_a_track_is_asked_of_a_sheet_alone() {
        let folder = a_rip();
        let sources = Sources::local();
        let sheet = folder.join("sheet.cue");

        let second = Analysed::named(sheet.as_os_str(), Some(2), &sources).expect("a cut");
        assert_eq!(
            second.span.map(FrameSpan::start),
            Some(Frames(u64::from(
                RATE * SECOND_TRACK_AT_CD_FRAMES / CD_FRAMES_A_SECOND
            )))
        );
        assert_eq!(
            second.location.as_path(),
            Some(from_here(&folder.join("whole.wav")).as_path())
        );

        assert!(matches!(
            Analysed::named(sheet.as_os_str(), None, &sources),
            Err(Error::SheetWithoutATrack { .. })
        ));
        assert!(matches!(
            Analysed::named(sheet.as_os_str(), Some(9), &sources),
            Err(Error::NoSuchSheetTrack { track: 9, .. })
        ));

        let whole = folder.join("whole.wav");
        assert!(matches!(
            Analysed::named(whole.as_os_str(), Some(1), &sources),
            Err(Error::TrackOutsideASheet { .. })
        ));

        let uri = OsString::from(
            MediaLocation::local(&whole)
                .to_uri_within(Some(FrameSpan::between(Frames(10), Frames(20)))),
        );
        let cut = Analysed::named(&uri, None, &sources).expect("a cut");
        assert_eq!(cut.span, Some(FrameSpan::between(Frames(10), Frames(20))));

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }
}
