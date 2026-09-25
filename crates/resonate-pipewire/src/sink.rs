use std::fmt;

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use crate::format::WireWord;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SinkId(u32);

impl SinkId {
    pub const fn new(global: u32) -> Self {
        Self(global)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SinkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeName(Box<str>);

impl NodeName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NodeName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Words {
    #[default]
    Whole,
    Packed,
    Padded,
    PackedAndPadded,
}

impl Words {
    pub(crate) const fn of(format: SampleFormat, word: WireWord) -> Self {
        match (format, word) {
            (SampleFormat::S24, WireWord::Packed) => Self::Packed,
            (SampleFormat::S24, WireWord::Padded) => Self::Padded,
            (SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32, _) => Self::Whole,
        }
    }

    pub(crate) const fn and(self, other: Self) -> Self {
        match (self, other) {
            (held, Self::Whole) | (Self::Whole, held) => held,
            (Self::Packed, Self::Packed) => Self::Packed,
            (Self::Padded, Self::Padded) => Self::Padded,
            _ => Self::PackedAndPadded,
        }
    }

    pub const fn offered_for(format: SampleFormat) -> Self {
        match format {
            SampleFormat::S24 => Self::PackedAndPadded,
            SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Self::Whole,
        }
    }

    pub fn spelled(self, format: SampleFormat) -> String {
        match (format, self) {
            (SampleFormat::S24, Self::Packed) => "S24LE".to_owned(),
            (SampleFormat::S24, Self::Padded) => "S24_32LE".to_owned(),
            (SampleFormat::S24, Self::PackedAndPadded) => "S24LE, S24_32LE".to_owned(),
            (format, _) => format.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkFormats {
    pub format: SampleFormat,
    pub words: Words,
    pub rates: Vec<SampleRate>,
    pub channels: Vec<ChannelLayout>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Plugged {
    #[default]
    Unsaid,
    No,
    Yes,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HardwareVolume {
    #[default]
    Unsaid,
    No,
    Yes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkPort {
    pub description: String,
    pub plugged: Plugged,
    pub hardware_volume: HardwareVolume,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SinkInfo {
    pub id: SinkId,
    pub name: NodeName,
    pub description: String,
    pub is_default: bool,
    pub is_hardware: bool,
    pub port: Option<SinkPort>,
    pub profile: Option<String>,
    pub formats: Vec<SinkFormats>,
    pub allowed_rates: Vec<SampleRate>,
    pub current_rate: Option<SampleRate>,
}

const BLUETOOTH_NODES: [&str; 2] = ["bluez_output.", "bluez_sink."];

impl SinkFormats {
    pub fn spelled(&self) -> String {
        self.words.spelled(self.format)
    }
}

impl SinkInfo {
    pub fn words_for(&self, format: SampleFormat) -> Option<Words> {
        self.formats
            .iter()
            .find(|entry| entry.format == format)
            .map(|entry| entry.words)
    }

    pub fn is_bluetooth(&self) -> bool {
        BLUETOOTH_NODES
            .iter()
            .any(|prefix| self.name.as_str().starts_with(prefix))
    }

    pub fn supports(&self, spec: StreamSpec) -> bool {
        self.allowed_rates.contains(&spec.rate)
            && self.formats.iter().any(|entry| {
                entry.format == spec.format
                    && entry.rates.contains(&spec.rate)
                    && entry.channels.contains(&spec.channels)
            })
    }

    pub fn best_spec_for(&self, source: StreamSpec) -> Option<StreamSpec> {
        self.candidates(source)
            .into_iter()
            .min_by_key(|candidate| fit(*candidate, source))
    }

    pub fn best_spec_on(&self, source: StreamSpec, rate: SampleRate) -> Option<StreamSpec> {
        self.candidates(source)
            .into_iter()
            .filter(|candidate| candidate.rate == rate)
            .min_by_key(|candidate| fit(*candidate, source))
    }

    fn candidates(&self, source: StreamSpec) -> Vec<StreamSpec> {
        if self.formats.is_empty() {
            return self
                .allowed_rates
                .iter()
                .map(|rate| StreamSpec::new(*rate, source.channels, source.format))
                .collect();
        }

        let mut specs = Vec::new();
        for entry in &self.formats {
            for rate in entry
                .rates
                .iter()
                .filter(|rate| self.allowed_rates.contains(rate))
            {
                offered_at(&mut specs, entry, *rate, source);
            }
        }
        if specs.is_empty() {
            for entry in &self.formats {
                for rate in &self.allowed_rates {
                    offered_at(&mut specs, entry, *rate, source);
                }
            }
        }
        specs
    }
}

fn offered_at(
    specs: &mut Vec<StreamSpec>,
    entry: &SinkFormats,
    rate: SampleRate,
    source: StreamSpec,
) {
    if entry.channels.is_empty() {
        specs.push(StreamSpec::new(rate, source.channels, entry.format));
        return;
    }
    for channels in &entry.channels {
        specs.push(StreamSpec::new(rate, *channels, entry.format));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Fit {
    rate: (u8, u32),
    channels: (u8, u16),
    format: (u8, u16),
}

fn fit(candidate: StreamSpec, source: StreamSpec) -> Fit {
    Fit {
        rate: rate_fit(candidate.rate, source.rate),
        channels: channel_fit(candidate.channels, source.channels),
        format: format_fit(candidate.format, source.format),
    }
}

fn rate_fit(candidate: SampleRate, source: SampleRate) -> (u8, u32) {
    const EXACT: u8 = 0;
    const MULTIPLE_IN_FAMILY: u8 = 1;
    const ABOVE_IN_FAMILY: u8 = 2;
    const BELOW_IN_FAMILY: u8 = 3;
    const ABOVE: u8 = 4;
    const BELOW: u8 = 5;

    let lowest_first = candidate.hz();
    let highest_first = u32::MAX - candidate.hz();

    if candidate == source {
        (EXACT, 0)
    } else if candidate.family() == source.family() {
        if candidate.is_integer_multiple_of(source) {
            (MULTIPLE_IN_FAMILY, lowest_first)
        } else if candidate > source {
            (ABOVE_IN_FAMILY, lowest_first)
        } else {
            (BELOW_IN_FAMILY, highest_first)
        }
    } else if candidate > source {
        (ABOVE, lowest_first)
    } else {
        (BELOW, highest_first)
    }
}

fn channel_fit(candidate: ChannelLayout, source: ChannelLayout) -> (u8, u16) {
    const EXACT: u8 = 0;
    const HOLDS_THEM_ALL: u8 = 1;
    const FOLDS_THEM_DOWN: u8 = 2;

    let width = u16::from(candidate.count().get());
    if candidate == source {
        (EXACT, 0)
    } else if candidate.count() >= source.count() {
        (HOLDS_THEM_ALL, width)
    } else {
        (FOLDS_THEM_DOWN, u16::MAX - width)
    }
}

fn format_fit(candidate: SampleFormat, source: SampleFormat) -> (u8, u16) {
    const EXACT: u8 = 0;
    const HOLDS_IT_LOSSLESSLY: u8 = 1;
    const NARROWS_IT: u8 = 2;

    let bits = u16::from(candidate.valid_bits());
    if candidate == source {
        (EXACT, 0)
    } else if candidate.losslessly_holds(source) {
        (HOLDS_IT_LOSSLESSLY, bits)
    } else {
        (NARROWS_IT, u16::MAX - bits)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SinkChange {
    Added(SinkId),
    Removed(SinkId),
    DefaultChanged,
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelCount, ChannelLayout};

    use super::*;

    fn sink(allowed: &[SampleRate], formats: &[SampleFormat]) -> SinkInfo {
        let entries = formats
            .iter()
            .map(|format| SinkFormats {
                format: *format,
                words: Words::offered_for(*format),
                rates: allowed.to_vec(),
                channels: vec![ChannelLayout::Stereo],
            })
            .collect();
        of(allowed, entries)
    }

    fn of(allowed: &[SampleRate], formats: Vec<SinkFormats>) -> SinkInfo {
        SinkInfo {
            id: SinkId::new(42),
            name: NodeName::new("alsa_output.test"),
            description: "Test DAC".to_owned(),
            is_default: true,
            is_hardware: true,
            port: None,
            profile: None,
            formats,
            allowed_rates: allowed.to_vec(),
            current_rate: allowed.first().copied(),
        }
    }

    fn entry(
        format: SampleFormat,
        rates: &[SampleRate],
        channels: &[ChannelLayout],
    ) -> SinkFormats {
        SinkFormats {
            format,
            words: Words::offered_for(format),
            rates: rates.to_vec(),
            channels: channels.to_vec(),
        }
    }

    fn stereo(rate: SampleRate, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(rate, ChannelLayout::Stereo, format)
    }

    #[test]
    fn an_exactly_supported_rate_is_always_chosen() {
        let sink = sink(
            &[
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                SampleRate::HZ_96000,
            ],
            &[SampleFormat::S32],
        );
        let chosen = sink
            .best_spec_for(stereo(SampleRate::HZ_44100, SampleFormat::S32))
            .expect("a sink with formats offers a candidate");

        assert_eq!(chosen.rate, SampleRate::HZ_44100);
    }

    #[test]
    fn an_unsupported_rate_prefers_an_integer_multiple_in_its_own_family() {
        let sink = sink(
            &[
                SampleRate::HZ_48000,
                SampleRate::HZ_88200,
                SampleRate::HZ_96000,
            ],
            &[SampleFormat::S32],
        );
        let chosen = sink
            .best_spec_for(stereo(SampleRate::HZ_44100, SampleFormat::S32))
            .expect("a sink with formats offers a candidate");

        assert_eq!(chosen.rate, SampleRate::HZ_88200);
    }

    #[test]
    fn crossing_families_is_the_last_resort() {
        let sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
        let chosen = sink
            .best_spec_for(stereo(SampleRate::HZ_44100, SampleFormat::S32))
            .expect("a sink with formats offers a candidate");

        assert_eq!(chosen.rate, SampleRate::HZ_48000);
    }

    #[test]
    fn a_rate_the_device_advertises_but_the_graph_forbids_is_not_support() {
        let mut sink = sink(&[SampleRate::HZ_48000], &[SampleFormat::S32]);
        sink.formats
            .first_mut()
            .expect("the fixture carries one entry")
            .rates
            .push(SampleRate::HZ_192000);
        let spec = stereo(SampleRate::HZ_192000, SampleFormat::S32);

        assert!(!sink.supports(spec));
        assert_eq!(
            sink.best_spec_for(spec).map(|chosen| chosen.rate),
            Some(SampleRate::HZ_48000)
        );
    }

    #[test]
    fn format_selection_widens_rather_than_narrows() {
        let sink = sink(
            &[SampleRate::HZ_44100],
            &[SampleFormat::S16, SampleFormat::S32],
        );

        for (source, expected) in [
            (SampleFormat::S24, SampleFormat::S32),
            (SampleFormat::S16, SampleFormat::S16),
        ] {
            let chosen = sink
                .best_spec_for(stereo(SampleRate::HZ_44100, source))
                .expect("a sink with formats offers a candidate");
            assert_eq!(chosen.format, expected);
        }
    }

    #[test]
    fn a_sink_that_cannot_hold_the_source_falls_back_to_its_widest_format() {
        let sink = sink(&[SampleRate::HZ_44100], &[SampleFormat::S16]);
        let chosen = sink
            .best_spec_for(stereo(SampleRate::HZ_44100, SampleFormat::S24))
            .expect("a sink with formats offers a candidate");

        assert_eq!(chosen.format, SampleFormat::S16);
    }

    #[test]
    fn every_spec_the_chooser_names_is_one_the_sink_advertised() {
        let sink = of(
            &[SampleRate::HZ_44100, SampleRate::HZ_48000],
            vec![
                entry(
                    SampleFormat::S32,
                    &[SampleRate::HZ_44100],
                    &[ChannelLayout::Stereo],
                ),
                entry(
                    SampleFormat::S16,
                    &[SampleRate::HZ_48000],
                    &[ChannelLayout::Stereo, ChannelLayout::Surround51],
                ),
            ],
        );

        for rate in [SampleRate::HZ_44100, SampleRate::HZ_48000] {
            for format in [SampleFormat::S16, SampleFormat::S24, SampleFormat::S32] {
                for channels in [ChannelLayout::Stereo, ChannelLayout::Surround51] {
                    let source = StreamSpec::new(rate, channels, format);
                    let chosen = sink
                        .best_spec_for(source)
                        .expect("a sink with formats offers a candidate");
                    assert!(
                        sink.supports(chosen),
                        "{source} was planned onto {chosen}, a pair the sink never advertised"
                    );
                }
            }
        }
    }

    #[test]
    fn a_sink_that_takes_the_sources_channels_keeps_them() {
        let sink = of(
            &[SampleRate::HZ_48000],
            vec![entry(
                SampleFormat::S32,
                &[SampleRate::HZ_48000],
                &[ChannelLayout::Stereo, ChannelLayout::Surround51],
            )],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );

        assert_eq!(sink.best_spec_for(source), Some(source));
    }

    #[test]
    fn a_stereo_sink_folds_a_surround_source_down_to_what_it_takes() {
        let sink = of(
            &[SampleRate::HZ_48000],
            vec![entry(
                SampleFormat::S32,
                &[SampleRate::HZ_48000],
                &[ChannelLayout::Mono, ChannelLayout::Stereo],
            )],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );

        assert_eq!(
            sink.best_spec_for(source).map(|chosen| chosen.channels),
            Some(ChannelLayout::Stereo)
        );
    }

    #[test]
    fn the_narrowest_layout_that_holds_every_channel_wins_over_a_wider_one() {
        let sink = of(
            &[SampleRate::HZ_48000],
            vec![entry(
                SampleFormat::S32,
                &[SampleRate::HZ_48000],
                &[ChannelLayout::Surround51, ChannelLayout::Surround71],
            )],
        );
        let source = StreamSpec::new(SampleRate::HZ_48000, ChannelLayout::Quad, SampleFormat::S32);

        assert_eq!(
            sink.best_spec_for(source).map(|chosen| chosen.channels),
            Some(ChannelLayout::Surround51)
        );
    }

    #[test]
    fn keeping_the_channels_outranks_keeping_the_depth() {
        let sink = of(
            &[SampleRate::HZ_48000],
            vec![
                entry(
                    SampleFormat::S32,
                    &[SampleRate::HZ_48000],
                    &[ChannelLayout::Stereo],
                ),
                entry(
                    SampleFormat::S16,
                    &[SampleRate::HZ_48000],
                    &[ChannelLayout::Surround51],
                ),
            ],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Surround51,
            SampleFormat::S32,
        );
        let chosen = sink
            .best_spec_for(source)
            .expect("a sink with formats offers a candidate");

        assert_eq!(chosen.channels, ChannelLayout::Surround51);
        assert_eq!(chosen.format, SampleFormat::S16);
    }

    #[test]
    fn pinning_the_rate_chooses_the_format_and_channels_offered_at_it() {
        let sink = of(
            &[SampleRate::HZ_44100, SampleRate::HZ_48000],
            vec![
                entry(
                    SampleFormat::S32,
                    &[SampleRate::HZ_44100],
                    &[ChannelLayout::Stereo],
                ),
                entry(
                    SampleFormat::S16,
                    &[SampleRate::HZ_48000],
                    &[ChannelLayout::Stereo],
                ),
            ],
        );
        let source = stereo(SampleRate::HZ_44100, SampleFormat::S32);

        assert_eq!(
            sink.best_spec_on(source, SampleRate::HZ_48000),
            Some(stereo(SampleRate::HZ_48000, SampleFormat::S16))
        );
        assert_eq!(sink.best_spec_on(source, SampleRate::HZ_96000), None);
    }

    #[test]
    fn a_sink_that_advertised_no_channels_is_believed_about_its_rates_alone() {
        let three = ChannelCount::new(3).expect("3 is in range");
        let sink = of(
            &[SampleRate::HZ_48000],
            vec![entry(SampleFormat::S32, &[SampleRate::HZ_48000], &[])],
        );
        let source = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Discrete(three),
            SampleFormat::S32,
        );

        assert_eq!(sink.best_spec_for(source), Some(source));
    }

    #[test]
    fn a_sink_that_advertised_no_formats_is_believed_about_its_rates_alone() {
        let sink = of(&[SampleRate::HZ_48000], Vec::new());
        let source = stereo(SampleRate::HZ_44100, SampleFormat::S24);

        assert_eq!(
            sink.best_spec_for(source),
            Some(stereo(SampleRate::HZ_48000, SampleFormat::S24))
        );
    }

    #[test]
    fn a_sink_whose_rates_the_graph_forbids_still_takes_only_the_formats_it_advertised() {
        let sink = SinkInfo {
            allowed_rates: vec![SampleRate::HZ_48000],
            current_rate: Some(SampleRate::HZ_48000),
            ..of(
                &[],
                vec![entry(
                    SampleFormat::S16,
                    &[SampleRate::HZ_44100],
                    &[ChannelLayout::Stereo],
                )],
            )
        };
        let source = stereo(SampleRate::HZ_44100, SampleFormat::S24);

        assert_eq!(
            sink.best_spec_for(source),
            Some(stereo(SampleRate::HZ_48000, SampleFormat::S16))
        );
        assert_eq!(
            sink.best_spec_on(source, SampleRate::HZ_48000),
            Some(stereo(SampleRate::HZ_48000, SampleFormat::S16))
        );
    }

    #[test]
    fn a_sink_that_advertised_nothing_at_all_names_no_spec() {
        let sink = of(&[], Vec::new());
        assert_eq!(
            sink.best_spec_for(stereo(SampleRate::HZ_44100, SampleFormat::S24)),
            None
        );
    }
}
