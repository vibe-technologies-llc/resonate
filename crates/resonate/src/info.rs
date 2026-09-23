use std::path::Path;

use resonate_codec::{
    BitRate, Codec, Container, CueFile, CueStart, Faststart, Percentiles, RawTag, Sources,
    StreamProfile, StreamReport, TagValue, WINDOW, probe, probe_stream, read_cue_media,
};
use resonate_core::{AppliedGain, Decibels, Frames, MediaLocation, SampleRate};
use resonate_engine::{EngineConfig, resolve_replay_gain};

use crate::{Result, table::Table};

const NANOS_A_MILLI: u128 = 1_000_000;
const MILLIS_A_SECOND: u64 = 1_000;
const SECONDS_A_MINUTE: u64 = 60;
const MINUTES_AN_HOUR: u64 = 60;

const WIDEST_TAG_VALUE: usize = 72;
const ELLIPSIS: char = '\u{2026}';
const GRAPH_COLUMNS: usize = 72;
const GRAPH_ROWS: usize = 12;
pub(crate) const BLOCKS: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn print(path: &Path, sources: &Sources, config: &EngineConfig, graph: bool) -> Result<()> {
    if names_a_sheet(path) {
        return sheet(path);
    }
    let report = probe_stream(sources, &MediaLocation::local(path))?;

    heading("FILE");
    file(path, &report);

    heading("STREAM");
    stream(&report);

    if let Some(cut) = report.info.cue.as_ref() {
        heading("CUE SHEET");
        cut_table(cut, Some((report.info.spec.rate, report.info.duration)));
    }

    heading("REPLAYGAIN");
    replay_gain(&report, config);

    if let Some(layout) = report.layout.as_ref() {
        heading("CONTAINER LAYOUT");
        boxes(layout);
    }

    if let Some(profile) = report.profile.as_ref() {
        heading("BITRATE");
        bitrate(profile);

        if graph {
            heading("BITRATE OVER TIME");
            plot(profile);
        }
    }

    heading("TAGS");
    tags(&report.tags);
    Ok(())
}

pub(crate) fn heading(title: &str) {
    println!("\n{title}");
    println!("{}", "─".repeat(title.len()));
}

pub(crate) fn pairs(rows: Vec<(&str, String)>) {
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for (name, value) in rows {
        println!("  {name:<width$}  {value}");
    }
}

fn file(path: &Path, report: &StreamReport) {
    let bytes = std::fs::metadata(path).map(|meta| meta.len()).ok();

    pairs(vec![
        ("path", path.display().to_string()),
        (
            "container",
            Container::from_id(report.info.container).to_string(),
        ),
        ("size", bytes.map_or_else(unknown, bytes_text)),
        ("seekable", yes_no(report.info.is_seekable)),
    ]);
}

const SHEET_EXTENSION: &str = "cue";

fn names_a_sheet(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

fn sheet(path: &Path) -> Result<()> {
    let sources = Sources::local();
    let held = read_cue_media(&sources, &MediaLocation::local(path))?;

    heading("CUE SHEET");
    pairs(vec![
        ("path", path.display().to_string()),
        ("encoding", held.encoding.to_string()),
        ("files", held.files.len().to_string()),
    ]);

    for cut in &held.files {
        heading(&format!("FILE {}", cut.named));
        let beside = path.parent().map(|folder| folder.join(&cut.named));
        let rate = beside
            .as_deref()
            .and_then(|file| probe(&sources, &MediaLocation::local(file)).ok())
            .map(|info| (info.spec.rate, info.duration));

        cut_table(cut, rate);
    }
    Ok(())
}

fn cut_table(cut: &CueFile, rate: Option<(SampleRate, Option<Frames>)>) {
    let mut table = Table::new(vec!["#", "TITLE", "ARTIST", "START", "LENGTH"]);
    for (index, track) in cut.audio_tracks() {
        let span = rate.and_then(|(rate, whole)| cut.span_of(index, rate, whole));
        let length = match (span.and_then(|span| span.frames()), rate) {
            (Some(frames), Some((rate, _))) => clock(frames, rate),
            _ => unknown(),
        };
        table.push(vec![
            track.number.to_string(),
            track.tags.title.clone().unwrap_or_else(unknown),
            track.tags.artist.clone().unwrap_or_else(unknown),
            started(track.start, rate),
            length,
        ]);
    }
    println!("{}", table.render());
}

fn started(start: CueStart, rate: Option<(SampleRate, Option<Frames>)>) -> String {
    match rate {
        Some((rate, _)) => clock(start.at(rate), rate),
        None => start.to_string(),
    }
}

fn stream(report: &StreamReport) {
    let info = &report.info;
    let duration = info
        .duration
        .map_or_else(unknown, |frames| clock(frames, info.spec.rate));
    let coded = info
        .bits_per_coded_sample
        .map_or_else(unknown, |bits| format!("{bits} bit"));

    pairs(vec![
        ("codec", Codec::from_id(info.codec).to_string()),
        ("sample rate", info.spec.rate.to_string()),
        ("channels", info.spec.channels.to_string()),
        ("decoded format", info.spec.format.to_string()),
        ("coded depth", coded),
        ("duration", duration),
        (
            "frames",
            info.duration
                .map_or_else(unknown, |frames| frames.to_string()),
        ),
        ("encoder delay", format!("{} frames", info.encoder_delay)),
        (
            "encoder padding",
            format!("{} frames", info.encoder_padding),
        ),
    ]);
}

fn replay_gain(report: &StreamReport, config: &EngineConfig) {
    let gain = &report.info.tags.replay_gain;
    let applied = resolve_replay_gain(config.replay_gain, config.levelling, gain);

    let rows = vec![
        ("mode", format!("{:?}", config.replay_gain)),
        ("pre-amp", config.levelling.pre_amp.to_string()),
        ("untagged", config.levelling.untagged.to_string()),
        ("track gain", gain.track_gain.map_or_else(unknown, decibels)),
        ("track peak", gain.track_peak.map_or_else(unknown, peak)),
        ("album gain", gain.album_gain.map_or_else(unknown, decibels)),
        ("album peak", gain.album_peak.map_or_else(unknown, peak)),
        ("applied", applied_gain(applied)),
        ("headroom", headroom(applied)),
    ];

    pairs(rows);
}

fn applied_gain(gain: AppliedGain) -> String {
    let landed = rounded(gain.applied().to_decibels().get());
    match (gain.gain, gain.is_capped()) {
        (None, false) => "none".to_owned(),
        (None, true) => format!("{landed:+.2} dB, to keep the peak under full scale"),
        (Some(_), false) => format!("{landed:+.2} dB"),
        (Some(requested), true) => {
            format!(
                "{landed:+.2} dB, capped from {:+.2} dB",
                rounded(requested.get())
            )
        }
    }
}

fn headroom(gain: AppliedGain) -> String {
    gain.headroom().map_or_else(unknown, |margin| {
        format!("{:+.2} dB", rounded(margin.get()))
    })
}

fn boxes(layout: &resonate_codec::BoxLayout) {
    let verdict = layout.faststart.map_or_else(
        || "no moov/mdat pair".to_owned(),
        |state| match state {
            Faststart::Ready => format!("{state} — plays before the whole file arrives"),
            Faststart::Trailing => format!("{state} — the index is at the end"),
        },
    );
    pairs(vec![("faststart", verdict)]);

    let mut table = Table::new(vec!["", "BOX", "OFFSET", "SIZE"]);
    for entry in &layout.boxes {
        table.push(vec![
            String::new(),
            entry.kind.to_string(),
            entry.at.to_string(),
            bytes_text(entry.bytes),
        ]);
    }
    print!("{}", table.render());
}

fn bitrate(profile: &StreamProfile) {
    let Percentiles {
        p05,
        p25,
        p50,
        p75,
        p95,
        p99,
    } = profile.percentiles;

    pairs(vec![
        ("overall", profile.overall.to_string()),
        ("variability", profile.variability.to_string()),
        (
            "variation",
            format!("{:.1}% of the mean", profile.variation * 100.0),
        ),
        ("mean", profile.mean.to_string()),
        ("std deviation", profile.deviation.to_string()),
        ("min", profile.min.to_string()),
        ("max", profile.max.to_string()),
        ("packets", profile.packets.to_string()),
        (
            "packet bytes",
            format!(
                "{} min / {} max",
                profile.smallest_packet, profile.largest_packet
            ),
        ),
        (
            "window",
            format!("{WINDOW:.0}s, {} points", profile.series.len()),
        ),
    ]);

    let mut table = Table::new(vec!["", "P05", "P25", "P50", "P75", "P95", "P99"]);
    table.push(vec![
        String::new(),
        kbps(p05),
        kbps(p25),
        kbps(p50),
        kbps(p75),
        kbps(p95),
        kbps(p99),
    ]);
    print!("\n{}", table.render());
}

fn plot(profile: &StreamProfile) {
    let series: Vec<f64> = profile
        .condensed(GRAPH_COLUMNS)
        .iter()
        .map(|rate| rate.get())
        .collect();
    let Some(top) = series.iter().copied().reduce(f64::max) else {
        return;
    };
    let floor = series.iter().copied().reduce(f64::min).unwrap_or(0.0);
    let span = (top - floor).max(f64::EPSILON);

    for row in (0..GRAPH_ROWS).rev() {
        let label = floor + span * (row as f64 + 0.5) / GRAPH_ROWS as f64;
        let line: String = series
            .iter()
            .map(|value| {
                let filled = ((value - floor) / span * GRAPH_ROWS as f64) - row as f64;
                BLOCKS[(filled.clamp(0.0, 1.0) * 8.0).round() as usize]
            })
            .collect();
        println!("  {:>9} │{line}", format!("{:.0}k", label / 1_000.0));
    }

    println!("  {:>9} └{}", "", "─".repeat(series.len()));
    println!(
        "  {:>9}  0s{:>width$}",
        "",
        format!("{:.0}s", profile.duration_seconds()),
        width = series.len().saturating_sub(2)
    );
}

fn tags(tags: &[RawTag]) {
    if tags.is_empty() {
        println!("  none");
        return;
    }

    let mut table = Table::new(vec!["", "KEY", "VALUE"]);
    for tag in tags {
        table.push(vec![
            String::new(),
            tag.qualified_name(),
            on_one_line(&tag.value),
        ]);
    }
    print!("{}", table.render());
}

fn on_one_line(value: &TagValue) -> String {
    let folded: String = value
        .to_string()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();

    let mut kept: String = folded.chars().take(WIDEST_TAG_VALUE).collect();
    if folded.chars().nth(WIDEST_TAG_VALUE).is_some() {
        kept.push(ELLIPSIS);
    }
    kept
}

fn kbps(rate: BitRate) -> String {
    format!("{:.0}", rate.kilobits())
}

fn decibels(gain: Decibels) -> String {
    format!("{:+.2} dB", gain.get())
}

fn peak(value: f32) -> String {
    format!(
        "{value:.6} ({:+.2} dBFS)",
        20.0 * value.max(f32::MIN_POSITIVE).log10()
    )
}

pub fn bytes_text(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => format!("{bytes} B"),
        _ => format!("{value:.1} {}", UNITS[unit]),
    }
}

pub fn clock(frames: Frames, rate: SampleRate) -> String {
    let nanos = frames.to_duration(rate).as_nanos();
    let millis = ((nanos + NANOS_A_MILLI / 2) / NANOS_A_MILLI) as u64;
    let seconds = millis / MILLIS_A_SECOND;
    let minutes = seconds / SECONDS_A_MINUTE;

    match minutes / MINUTES_AN_HOUR {
        0 => format!(
            "{minutes}:{:02}.{:03}",
            seconds % SECONDS_A_MINUTE,
            millis % MILLIS_A_SECOND
        ),
        hours => format!(
            "{hours}:{:02}:{:02}.{:03}",
            minutes % MINUTES_AN_HOUR,
            seconds % SECONDS_A_MINUTE,
            millis % MILLIS_A_SECOND
        ),
    }
}

fn yes_no(value: bool) -> String {
    if value { "yes" } else { "no" }.to_owned()
}

fn unknown() -> String {
    "unknown".to_owned()
}

fn rounded(db: f32) -> f32 {
    (db * 100.0).round() / 100.0 + 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rate: SampleRate, frames: u64) -> String {
        clock(Frames(frames), rate)
    }

    #[test]
    fn a_length_one_frame_short_of_a_minute_rolls_over_rather_than_reading_sixty() {
        assert_eq!(at(SampleRate::HZ_44100, 44_100 * 120 - 1), "2:00.000");
        assert_eq!(at(SampleRate::HZ_44100, 44_100 * 120), "2:00.000");
        assert_eq!(at(SampleRate::HZ_44100, 44_100 * 119), "1:59.000");
    }

    #[test]
    fn a_length_past_an_hour_reads_in_hours() {
        assert_eq!(at(SampleRate::HZ_44100, 44_100 * 3_661), "1:01:01.000");
        assert_eq!(at(SampleRate::HZ_44100, 44_100 * 3_599), "59:59.000");
    }
}
