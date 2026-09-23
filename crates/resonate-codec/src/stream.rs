use std::{cmp::Ordering, fmt};

use resonate_core::{Frames, MediaLocation, SampleRate};
use symphonia::core::{errors, formats::FormatReader, packet::Packet};

use crate::{
    CodecOp, Error, MediaInfo, RawTag, Result, StreamTrackId, boxes, container, source::Sources,
    tags, timeline::Timeline,
};

pub const WINDOW: f64 = 1.0;

const CONSTANT_VARIATION: f64 = 0.01;
const MAX_PROFILED_SECONDS: f64 = 86_400.0;
const MAX_WINDOWS: usize = (MAX_PROFILED_SECONDS / WINDOW) as usize;

#[derive(Clone, Copy, Debug, Default, PartialEq, PartialOrd)]
pub struct BitRate(f64);

impl BitRate {
    pub const ZERO: Self = Self(0.0);

    pub fn new(bits_per_second: f64) -> Option<Self> {
        (bits_per_second.is_finite() && bits_per_second >= 0.0).then_some(Self(bits_per_second))
    }

    pub fn over(bytes: u64, frames: Frames, rate: SampleRate) -> Option<Self> {
        let seconds = frames.get() as f64 / f64::from(rate.hz());
        if seconds <= 0.0 {
            return None;
        }
        Self::new((bytes as f64) * 8.0 / seconds)
    }

    pub const fn get(self) -> f64 {
        self.0
    }

    pub fn kilobits(self) -> f64 {
        self.0 / 1_000.0
    }
}

impl fmt::Display for BitRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.0} kbps", self.kilobits())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PacketSpan {
    pub at: Frames,
    pub frames: Frames,
    pub bytes: u32,
}

impl PacketSpan {
    pub(crate) fn of(packet: &Packet, timeline: &Timeline) -> Self {
        Self {
            at: timeline.frames(packet.pts),
            frames: timeline.span(packet.dur),
            bytes: u32::try_from(packet.data.len()).unwrap_or(u32::MAX),
        }
    }

    pub fn rate(&self, rate: SampleRate) -> Option<BitRate> {
        BitRate::over(u64::from(self.bytes), self.frames, rate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Variability {
    Constant,
    Variable,
}

impl fmt::Display for Variability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Constant => "constant",
            Self::Variable => "variable",
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Percentiles {
    pub p05: BitRate,
    pub p25: BitRate,
    pub p50: BitRate,
    pub p75: BitRate,
    pub p95: BitRate,
    pub p99: BitRate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamProfile {
    pub rate: SampleRate,
    pub packets: u64,
    pub bytes: u64,
    pub frames: Frames,
    pub overall: BitRate,
    pub smallest_packet: u32,
    pub largest_packet: u32,
    pub series: Vec<BitRate>,
    pub min: BitRate,
    pub max: BitRate,
    pub mean: BitRate,
    pub deviation: BitRate,
    pub variation: f64,
    pub percentiles: Percentiles,
    pub variability: Variability,
}

impl StreamProfile {
    pub fn duration_seconds(&self) -> f64 {
        self.frames.get() as f64 / f64::from(self.rate.hz())
    }

    pub fn condensed(&self, columns: usize) -> Vec<BitRate> {
        if self.series.len() <= columns || columns == 0 {
            return self.series.clone();
        }

        (0..columns)
            .map(|column| {
                let from = column * self.series.len() / columns;
                let to = ((column + 1) * self.series.len() / columns).max(from + 1);
                let bucket = self
                    .series
                    .get(from..to.min(self.series.len()))
                    .unwrap_or_default();
                BitRate::new(mean(bucket)).unwrap_or_default()
            })
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct ProfileBuilder {
    rate: SampleRate,
    window_frames: u64,
    packets: u64,
    bytes: u64,
    frames: u64,
    smallest: u32,
    largest: u32,
    window_bytes: u64,
    window_filled: u64,
    series: Vec<BitRate>,
}

impl ProfileBuilder {
    pub fn new(rate: SampleRate) -> Self {
        Self {
            rate,
            window_frames: (f64::from(rate.hz()) * WINDOW).round() as u64,
            packets: 0,
            bytes: 0,
            frames: 0,
            smallest: u32::MAX,
            largest: 0,
            window_bytes: 0,
            window_filled: 0,
            series: Vec::new(),
        }
    }

    pub fn points(&self) -> usize {
        self.series.len()
    }

    pub fn push(&mut self, span: PacketSpan) {
        self.packets = self.packets.saturating_add(1);
        self.bytes = self.bytes.saturating_add(u64::from(span.bytes));
        self.frames = self.frames.saturating_add(span.frames.get());
        self.smallest = self.smallest.min(span.bytes);
        self.largest = self.largest.max(span.bytes);

        self.window_bytes = self.window_bytes.saturating_add(u64::from(span.bytes));
        self.window_filled = self.window_filled.saturating_add(span.frames.get());

        if self.window_frames == 0 {
            return;
        }
        for _ in 0..self.closable_windows() {
            self.close_window();
        }
        if self.window_filled >= self.window_frames {
            self.abandon_window();
        }
    }

    fn closable_windows(&self) -> u64 {
        let spanned = self.window_filled / self.window_frames;
        let room = MAX_WINDOWS.saturating_sub(self.series.len()) as u64;
        spanned.min(room)
    }

    fn abandon_window(&mut self) {
        self.window_bytes = 0;
        self.window_filled = 0;
    }

    fn close_window(&mut self) {
        let share = self.window_frames as f64 / self.window_filled as f64;
        let bytes = self.window_bytes as f64 * share;

        if let Some(rate) = BitRate::new(bytes * 8.0 / WINDOW) {
            self.series.push(rate);
        }
        self.window_bytes = (self.window_bytes as f64 * (1.0 - share)).round() as u64;
        self.window_filled -= self.window_frames;
    }

    pub fn finish(&self) -> Option<StreamProfile> {
        if self.packets == 0 {
            return None;
        }
        let frames = Frames(self.frames);
        let overall = BitRate::over(self.bytes, frames, self.rate)?;

        let mut series = self.series.clone();
        if series.is_empty() {
            series.push(overall);
        }

        let mut sorted = series.clone();
        sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));

        let mean = mean(&series);
        let deviation = deviation(&series, mean);
        let variation = if mean > 0.0 { deviation / mean } else { 0.0 };

        Some(StreamProfile {
            rate: self.rate,
            packets: self.packets,
            bytes: self.bytes,
            frames,
            overall,
            smallest_packet: self.smallest,
            largest_packet: self.largest,
            min: sorted.first().copied().unwrap_or_default(),
            max: sorted.last().copied().unwrap_or_default(),
            mean: BitRate::new(mean).unwrap_or_default(),
            deviation: BitRate::new(deviation).unwrap_or_default(),
            variation,
            percentiles: Percentiles {
                p05: percentile(&sorted, 0.05),
                p25: percentile(&sorted, 0.25),
                p50: percentile(&sorted, 0.50),
                p75: percentile(&sorted, 0.75),
                p95: percentile(&sorted, 0.95),
                p99: percentile(&sorted, 0.99),
            },
            variability: if variation <= CONSTANT_VARIATION {
                Variability::Constant
            } else {
                Variability::Variable
            },
            series,
        })
    }
}

fn mean(series: &[BitRate]) -> f64 {
    if series.is_empty() {
        return 0.0;
    }
    series.iter().map(|rate| rate.get()).sum::<f64>() / series.len() as f64
}

fn deviation(series: &[BitRate], mean: f64) -> f64 {
    if series.len() < 2 {
        return 0.0;
    }
    let total: f64 = series
        .iter()
        .map(|rate| (rate.get() - mean).powi(2))
        .sum::<f64>();
    (total / series.len() as f64).sqrt()
}

fn percentile(sorted: &[BitRate], fraction: f64) -> BitRate {
    if sorted.is_empty() {
        return BitRate::ZERO;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    let index = rank.max(1).min(sorted.len()) - 1;
    sorted.get(index).copied().unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq)]
pub struct StreamReport {
    pub info: MediaInfo,
    pub tags: Vec<RawTag>,
    pub profile: Option<StreamProfile>,
    pub layout: Option<boxes::BoxLayout>,
}

pub fn probe_stream(sources: &Sources, location: &MediaLocation) -> Result<StreamReport> {
    let opened = container::open_media(sources, location)?;
    let info = opened.media_info(location)?;
    let Some(mut coded) = opened.into_coded() else {
        return Ok(StreamReport {
            info,
            tags: Vec::new(),
            profile: None,
            layout: None,
        });
    };
    let (track, _) = container::audio_track(coded.reader.as_ref(), location)?;
    let id = StreamTrackId(track.id);
    let raw = tags::read_raw(&coded.revisions, track.id, &coded.prescan);
    let timeline = container::timeline(track, &info);

    let profile = walk(
        coded.reader.as_mut(),
        id,
        &timeline,
        info.spec.rate,
        location,
    )?;

    Ok(StreamReport {
        info,
        tags: raw,
        profile,
        layout: boxes::read(sources, location)?,
    })
}

fn walk(
    reader: &mut dyn FormatReader,
    track: StreamTrackId,
    timeline: &Timeline,
    rate: SampleRate,
    location: &MediaLocation,
) -> Result<Option<StreamProfile>> {
    let mut builder = ProfileBuilder::new(rate);

    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(errors::Error::IoError(source))
                if source.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                tracing::debug!("stream ends mid-packet; the profile covers what was readable");
                break;
            }
            Err(source) => {
                return Err(Error::from_symphonia(source, CodecOp::ReadPacket, location));
            }
        };

        if packet.track_id == track.0 {
            builder.push(PacketSpan::of(&packet, timeline));
        }
    }

    Ok(builder.finish())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: SampleRate = SampleRate::HZ_44100;

    fn constant(packets: usize, bytes: u32, frames: u64) -> StreamProfile {
        let mut builder = ProfileBuilder::new(RATE);
        for index in 0..packets {
            builder.push(PacketSpan {
                at: Frames(index as u64 * frames),
                frames: Frames(frames),
                bytes,
            });
        }
        builder.finish().expect("packets were pushed")
    }

    #[test]
    fn an_empty_stream_yields_no_profile() {
        assert!(ProfileBuilder::new(RATE).finish().is_none());
    }

    #[test]
    fn a_fixed_packet_size_reads_as_a_constant_bitrate() {
        let profile = constant(400, 417, 1_152);

        assert_eq!(profile.variability, Variability::Constant);
        assert!(
            profile.variation < CONSTANT_VARIATION,
            "{}",
            profile.variation
        );
        assert!(
            (profile.overall.kilobits() - 128.0).abs() < 1.0,
            "{}",
            profile.overall
        );
        assert_eq!(profile.smallest_packet, 417);
        assert_eq!(profile.largest_packet, 417);
    }

    #[test]
    fn the_overall_rate_is_the_whole_file_rather_than_the_mean_of_its_windows() {
        let profile = constant(400, 417, 1_152);
        let seconds = profile.duration_seconds();

        assert!(
            (seconds - 400.0 * 1_152.0 / 44_100.0).abs() < 1e-9,
            "{seconds}"
        );
        assert!((profile.overall.get() - profile.mean.get()).abs() < profile.overall.get() * 0.02);
    }

    fn swinging(period: u64, packets: u64) -> StreamProfile {
        let mut builder = ProfileBuilder::new(RATE);
        for index in 0..packets {
            let bytes = if index % (2 * period) < period {
                200
            } else {
                800
            };
            builder.push(PacketSpan {
                at: Frames(index * 1_152),
                frames: Frames(1_152),
                bytes,
            });
        }
        builder.finish().expect("packets were pushed")
    }

    #[test]
    fn a_swinging_packet_size_reads_as_a_variable_bitrate() {
        let profile = swinging(100, 800);

        assert_eq!(profile.variability, Variability::Variable);
        assert!(profile.max > profile.min);
        assert!(profile.deviation > BitRate::ZERO);
        assert_eq!(profile.smallest_packet, 200);
        assert_eq!(profile.largest_packet, 800);
    }

    #[test]
    fn a_swing_faster_than_the_window_is_flattened_by_it() {
        let fast = swinging(4, 800);
        let slow = swinging(100, 800);

        assert!(
            fast.variation < slow.variation / 4.0,
            "fast swung {} against the slow swing's {}",
            fast.variation,
            slow.variation
        );
        assert_eq!(fast.smallest_packet, 200);
        assert_eq!(fast.largest_packet, 800);
    }

    #[test]
    fn the_series_carries_one_point_per_window_of_audio() {
        let profile = constant(400, 417, 1_152);
        let seconds = profile.duration_seconds();

        let expected = (seconds / WINDOW).floor() as usize;
        assert!(
            profile.series.len().abs_diff(expected) <= 1,
            "{} points for {seconds:.1}s",
            profile.series.len()
        );
    }

    #[test]
    fn a_file_shorter_than_one_window_still_reports_a_rate() {
        let profile = constant(2, 417, 1_152);

        assert_eq!(profile.series.len(), 1);
        assert_eq!(profile.series.first().copied(), Some(profile.overall));
    }

    #[test]
    fn a_stream_longer_than_the_bound_stops_gaining_points_and_still_reports() {
        let mut builder = ProfileBuilder::new(RATE);
        let window = Frames(u64::from(RATE.hz()));

        for _ in 0..MAX_WINDOWS + 16 {
            builder.push(PacketSpan {
                at: Frames(0),
                frames: window,
                bytes: 417,
            });
        }
        let profile = builder.finish().expect("packets were pushed");

        assert_eq!(profile.series.len(), MAX_WINDOWS);
        assert_eq!(profile.packets, MAX_WINDOWS as u64 + 16);
    }

    #[test]
    fn a_packet_declaring_more_frames_than_a_stream_could_hold_ends_rather_than_spins() {
        let mut builder = ProfileBuilder::new(RATE);
        let absurd = PacketSpan {
            at: Frames(0),
            frames: Frames(u64::MAX),
            bytes: 417,
        };

        builder.push(absurd);
        builder.push(absurd);

        assert_eq!(builder.points(), MAX_WINDOWS);
        assert_eq!(builder.finish().expect("packets were pushed").packets, 2);
    }

    #[test]
    fn the_percentiles_rise_with_their_rank_and_stay_inside_the_range() {
        let mut builder = ProfileBuilder::new(RATE);
        for index in 0..400_u64 {
            builder.push(PacketSpan {
                at: Frames(index * 1_152),
                frames: Frames(1_152),
                bytes: 100 + (index as u32 * 3),
            });
        }
        let profile = builder.finish().expect("packets were pushed");
        let Percentiles {
            p05,
            p25,
            p50,
            p75,
            p95,
            p99,
        } = profile.percentiles;

        assert!(p05 <= p25 && p25 <= p50 && p50 <= p75 && p75 <= p95 && p95 <= p99);
        assert!(profile.min <= p05 && p99 <= profile.max);
    }

    #[test]
    fn condensing_keeps_the_shape_and_the_level_of_the_series() {
        let profile = swinging(100, 4_000);
        let condensed = profile.condensed(32);

        assert_eq!(condensed.len(), 32);
        assert!(condensed.iter().all(|rate| *rate >= profile.min));
        assert!(condensed.iter().all(|rate| *rate <= profile.max));

        let level = mean(&condensed);
        assert!(
            (level - profile.mean.get()).abs() < profile.mean.get() * 0.05,
            "{level} against {}",
            profile.mean
        );
    }

    #[test]
    fn condensing_a_series_shorter_than_the_columns_hands_it_back_whole() {
        let profile = constant(400, 417, 1_152);

        assert_eq!(profile.condensed(4_096), profile.series);
    }

    #[test]
    fn the_builder_counts_the_windows_it_has_closed() {
        let mut builder = ProfileBuilder::new(RATE);
        assert_eq!(builder.points(), 0);

        for index in 0..400_u64 {
            builder.push(PacketSpan {
                at: Frames(index * 1_152),
                frames: Frames(1_152),
                bytes: 417,
            });
        }
        let profile = builder.finish().expect("packets were pushed");

        assert_eq!(builder.points(), profile.series.len());
    }

    #[test]
    fn a_packet_reports_its_own_instantaneous_rate() {
        let span = PacketSpan {
            at: Frames::ZERO,
            frames: Frames(1_152),
            bytes: 417,
        };
        let rate = span
            .rate(RATE)
            .expect("a packet covering frames has a rate");

        assert!((rate.kilobits() - 127.7).abs() < 0.5, "{rate}");
    }

    #[test]
    fn a_packet_covering_no_frames_has_no_rate() {
        let span = PacketSpan {
            at: Frames::ZERO,
            frames: Frames::ZERO,
            bytes: 417,
        };
        assert_eq!(span.rate(RATE), None);
    }
}
