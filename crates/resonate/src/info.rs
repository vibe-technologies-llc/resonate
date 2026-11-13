use std::path::Path;

use resonate_codec::{
    BitRate, Codec, Container, CueFile, CueStart, Faststart, Percentiles, RawTag, Sources,
    StreamProfile, StreamReport, TagValue, WINDOW, probe, probe_stream, read_cue_media,
    the_file_a_cue_names,
};
use resonate_core::{AppliedGain, Decibels, Frames, MediaLocation, SampleRate};
use resonate_engine::{EngineConfig, resolve_replay_gain};
use unicode_width::UnicodeWidthStr as _;

use crate::{
    Result,
    table::{self, Table},
    words,
};

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

    heading(words::info::file());
    file(path, &report);

    heading(words::info::stream());
    stream(&report);

    if let Some(cut) = report.info.cue.as_ref() {
        heading(words::info::cue_sheet());
        cut_table(cut, Some((report.info.spec.rate, report.info.duration)));
    }

    heading(words::info::replaygain());
    replay_gain(&report, config);

    if let Some(layout) = report.layout.as_ref() {
        heading(words::info::container_layout());
        boxes(layout);
    }

    if let Some(profile) = report.profile.as_ref() {
        heading(words::info::bitrate());
        bitrate(profile);

        if graph {
            heading(words::info::bitrate_over_time());
            plot(profile);
        }
    }

    heading(words::info::tags());
    tags(&report.tags);
    Ok(())
}

pub(crate) fn heading(title: &str) {
    said!("\n{title}");
    said!("{}", "─".repeat(title.width()));
}

pub(crate) fn pairs(rows: Vec<(&str, String)>) {
    let width = rows.iter().map(|(name, _)| name.width()).max().unwrap_or(0);
    for (name, value) in rows {
        let padding = " ".repeat(width - name.width());
        said!("  {name}{padding}  {value}");
    }
}

fn file(path: &Path, report: &StreamReport) {
    let bytes = std::fs::metadata(path).map(|meta| meta.len()).ok();

    pairs(vec![
        (words::info::path(), path.display().to_string()),
        (
            words::info::container(),
            Container::from_id(report.info.container).to_string(),
        ),
        (words::info::size(), bytes.map_or_else(unknown, bytes_text)),
        (words::info::seekable(), yes_no(report.info.is_seekable)),
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

    heading(words::info::cue_sheet());
    pairs(vec![
        (words::info::path(), path.display().to_string()),
        (words::info::encoding(), held.encoding.to_string()),
        (words::info::files(), held.files.len().to_string()),
    ]);

    for cut in &held.files {
        heading(&words::info::file_named(cut.named.as_str()));
        let beside = path
            .parent()
            .and_then(|folder| the_file_a_cue_names(folder, &cut.named));
        let rate = beside
            .as_deref()
            .and_then(|file| probe(&sources, &MediaLocation::local(file)).ok())
            .map(|info| (info.spec.rate, info.duration));

        cut_table(cut, rate);
    }
    Ok(())
}

fn cut_table(cut: &CueFile, rate: Option<(SampleRate, Option<Frames>)>) {
    let mut table = Table::new(vec![
        words::info::number_heading(),
        words::info::title_heading(),
        words::info::artist_heading(),
        words::info::start_heading(),
        words::info::length_heading(),
    ]);
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
    said!("{}", table.render());
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
        .map_or_else(unknown, words::info::coded_bits);

    pairs(vec![
        (words::info::codec(), Codec::from_id(info.codec).to_string()),
        (words::info::sample_rate(), info.spec.rate.to_string()),
        (words::info::channels(), info.spec.channels.to_string()),
        (words::info::decoded_format(), info.spec.format.to_string()),
        (words::info::coded_depth(), coded),
        (words::info::duration(), duration),
        (
            words::info::frames(),
            info.duration
                .map_or_else(unknown, |frames| frames.to_string()),
        ),
        (
            words::info::encoder_delay(),
            words::info::counted_frames(info.encoder_delay),
        ),
        (
            words::info::encoder_padding(),
            words::info::counted_frames(info.encoder_padding),
        ),
    ]);
}

fn replay_gain(report: &StreamReport, config: &EngineConfig) {
    let gain = &report.info.tags.heard_gain();
    let applied = resolve_replay_gain(config.replay_gain, config.levelling, gain);

    let rows = vec![
        (words::info::mode(), format!("{:?}", config.replay_gain)),
        (words::info::pre_amp(), config.levelling.pre_amp.to_string()),
        (
            words::info::untagged(),
            config.levelling.untagged.to_string(),
        ),
        (
            words::info::track_gain(),
            gain.track_gain.map_or_else(unknown, decibels),
        ),
        (
            words::info::track_peak(),
            gain.track_peak.map_or_else(unknown, peak),
        ),
        (
            words::info::album_gain(),
            gain.album_gain.map_or_else(unknown, decibels),
        ),
        (
            words::info::album_peak(),
            gain.album_peak.map_or_else(unknown, peak),
        ),
        (words::info::applied(), applied_gain(applied)),
        (words::info::headroom(), headroom(applied)),
    ];

    pairs(rows);
}

fn applied_gain(gain: AppliedGain) -> String {
    let landed = rounded(gain.applied().to_decibels().get());
    match (gain.gain, gain.is_capped()) {
        (None, false) => words::info::none().to_owned(),
        (None, true) => words::info::under_full_scale(format!("{landed:+.2}")),
        (Some(_), false) => words::info::decibels(format!("{landed:+.2}")),
        (Some(requested), true) => words::info::capped_from(
            format!("{landed:+.2}"),
            format!("{:+.2}", rounded(requested.get())),
        ),
    }
}

fn headroom(gain: AppliedGain) -> String {
    gain.headroom().map_or_else(unknown, |margin| {
        words::info::decibels(format!("{:+.2}", rounded(margin.get())))
    })
}

fn boxes(layout: &resonate_codec::BoxLayout) {
    let verdict = layout.faststart.map_or_else(
        || words::info::no_moov().to_owned(),
        |state| match state {
            Faststart::Ready => words::info::faststart_ready(state.to_string()),
            Faststart::Trailing => words::info::faststart_trailing(state.to_string()),
        },
    );
    pairs(vec![("faststart", verdict)]);

    let mut table = Table::new(vec![
        "",
        words::info::box_heading(),
        words::info::offset_heading(),
        words::info::size_heading(),
    ]);
    for entry in &layout.boxes {
        table.push(vec![
            String::new(),
            entry.kind.to_string(),
            entry.at.to_string(),
            bytes_text(entry.bytes),
        ]);
    }
    said_on!("{}", table.render());
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
        (words::info::overall(), profile.overall.to_string()),
        (words::info::variability(), profile.variability.to_string()),
        (
            words::info::variation(),
            words::info::of_the_mean(format!("{:.1}", profile.variation * 100.0)),
        ),
        (words::info::mean(), profile.mean.to_string()),
        (words::info::std_deviation(), profile.deviation.to_string()),
        (words::info::min(), profile.min.to_string()),
        (words::info::max(), profile.max.to_string()),
        (words::info::packets(), profile.packets.to_string()),
        (
            words::info::packet_bytes(),
            words::info::smallest_and_largest(profile.largest_packet, profile.smallest_packet),
        ),
        (
            words::info::window(),
            words::info::window_points(profile.series.len(), format!("{WINDOW:.0}")),
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
    said_on!("\n{}", table.render());
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
        said!("  {:>9} │{line}", format!("{:.0}k", label / 1_000.0));
    }

    said!("  {:>9} └{}", "", "─".repeat(series.len()));
    said!(
        "  {:>9}  0s{:>width$}",
        "",
        format!("{:.0}s", profile.duration_seconds()),
        width = series.len().saturating_sub(2)
    );
}

fn tags(tags: &[RawTag]) {
    if tags.is_empty() {
        said!("  {}", words::info::none());
        return;
    }

    let mut table = Table::new(vec![
        "",
        words::info::key_heading(),
        words::info::value_heading(),
    ]);
    for tag in tags {
        table.push(vec![
            String::new(),
            tag.qualified_name(),
            on_one_line(&tag.value),
        ]);
    }
    said_on!("{}", table.render());
}

fn on_one_line(value: &TagValue) -> String {
    let folded = table::on_one_line(&value.to_string());

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
    words::info::decibels(format!("{:+.2}", gain.get()))
}

fn peak(value: f32) -> String {
    words::info::peak(
        format!("{:+.2}", 20.0 * value.max(f32::MIN_POSITIVE).log10()),
        format!("{value:.6}"),
    )
}

pub fn bytes_text(bytes: u64) -> String {
    const UNITS: [fn(String) -> String; 3] = [
        |value| words::info::kibibytes(value),
        |value| words::info::mebibytes(value),
        |value| words::info::gibibytes(value),
    ];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    match unit {
        0 => words::info::bytes(bytes),
        _ => UNITS[unit - 1](resonate_words::decimal(value, 1)),
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
    if value {
        words::info::yes()
    } else {
        words::info::no()
    }
    .to_owned()
}

fn unknown() -> String {
    words::info::unknown().to_owned()
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
