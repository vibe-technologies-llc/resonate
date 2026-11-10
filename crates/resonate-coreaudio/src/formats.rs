use std::num::NonZeroUsize;

use resonate_audio::{SinkFormats, Words};
use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use crate::fill::Laid;

pub const RATES_A_RANGE_IS_CROSSED_WITH: [SampleRate; 8] = [
    SampleRate::HZ_44100,
    SampleRate::HZ_48000,
    SampleRate::HZ_88200,
    SampleRate::HZ_96000,
    SampleRate::HZ_176400,
    SampleRate::HZ_192000,
    SampleRate::HZ_352800,
    SampleRate::HZ_384000,
];
const RATES_AGREE_WITHIN_HZ: f64 = 0.5;
const HOGGED_WORDS: [SampleFormat; 3] = [SampleFormat::S16, SampleFormat::S24, SampleFormat::F32];
const HOGGED_LAYOUTS: [ChannelLayout; 2] = [ChannelLayout::Stereo, ChannelLayout::Mono];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Depth {
    Sixteen,
    TwentyFour,
    ThirtyTwo,
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Physical {
    pub depth: Depth,
    pub channels: u32,
    pub lowest_hz: f64,
    pub highest_hz: f64,
}

impl Physical {
    fn is_one_rate(&self) -> bool {
        (self.highest_hz - self.lowest_hz).abs() < RATES_AGREE_WITHIN_HZ
    }

    fn runs_at(&self, rate: SampleRate) -> bool {
        let hz = f64::from(rate.hz());
        hz > self.lowest_hz - RATES_AGREE_WITHIN_HZ && hz < self.highest_hz + RATES_AGREE_WITHIN_HZ
    }

    fn rates(&self) -> Vec<SampleRate> {
        if self.is_one_rate() {
            return rate_of(self.lowest_hz).into_iter().collect();
        }
        RATES_A_RANGE_IS_CROSSED_WITH
            .into_iter()
            .filter(|rate| self.runs_at(*rate))
            .collect()
    }

    const fn holds(&self, format: SampleFormat) -> bool {
        match format {
            SampleFormat::S16 => true,
            SampleFormat::S24 => !matches!(self.depth, Depth::Sixteen),
            SampleFormat::F32 => matches!(self.depth, Depth::Float),
            SampleFormat::S32 => false,
        }
    }

    fn plays(&self, layout: ChannelLayout) -> bool {
        layout_of(self.channels) == Some(layout)
    }
}

pub fn rate_of(hz: f64) -> Option<SampleRate> {
    if !hz.is_finite() || hz < 0.0 || hz > f64::from(SampleRate::MAX_HZ) {
        return None;
    }
    SampleRate::new(hz.round() as u32).ok()
}

pub const fn layout_of(channels: u32) -> Option<ChannelLayout> {
    match channels {
        0 => None,
        1 => Some(ChannelLayout::Mono),
        _ => Some(ChannelLayout::Stereo),
    }
}

pub const fn words_of(format: SampleFormat) -> Words {
    match format {
        SampleFormat::S24 => Words::Padded,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientWord {
    Float32,
    Signed16,
    Signed32,
}

impl ClientWord {
    pub const fn of(format: SampleFormat) -> Option<Self> {
        match format {
            SampleFormat::F32 => Some(Self::Float32),
            SampleFormat::S16 => Some(Self::Signed16),
            SampleFormat::S24 => Some(Self::Signed32),
            SampleFormat::S32 => None,
        }
    }
}

pub const fn laid_of(format: SampleFormat) -> Laid {
    match format {
        SampleFormat::S24 => Laid::TwentyFourAgainstTheTop,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Laid::AsHeld,
    }
}

pub fn frame_of(spec: StreamSpec) -> NonZeroUsize {
    NonZeroUsize::new(spec.bytes_per_frame().get() as usize).unwrap_or(NonZeroUsize::MIN)
}

pub fn shared_formats(nominal: SampleRate, channels: u32) -> Vec<SinkFormats> {
    layout_of(channels)
        .map(|layout| SinkFormats {
            format: SampleFormat::F32,
            words: Words::Whole,
            rates: vec![nominal],
            channels: vec![layout],
        })
        .into_iter()
        .collect()
}

pub fn hog_formats(offered: &[Physical]) -> Vec<SinkFormats> {
    let mut formats = Vec::new();
    for format in HOGGED_WORDS {
        for layout in HOGGED_LAYOUTS {
            let mut rates: Vec<SampleRate> = offered
                .iter()
                .filter(|physical| physical.holds(format) && physical.plays(layout))
                .flat_map(Physical::rates)
                .collect();
            rates.sort_unstable();
            rates.dedup();
            if !rates.is_empty() {
                formats.push(SinkFormats {
                    format,
                    words: words_of(format),
                    rates,
                    channels: vec![layout],
                });
            }
        }
    }
    formats
}

pub fn physical_for(spec: StreamSpec, offered: &[Physical]) -> Option<Physical> {
    offered
        .iter()
        .filter(|physical| {
            physical.holds(spec.format)
                && physical.plays(spec.channels)
                && physical.runs_at(spec.rate)
        })
        .min_by_key(|physical| (physical.depth, physical.channels))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(depth: Depth, hz: f64) -> Physical {
        Physical {
            depth,
            channels: 2,
            lowest_hz: hz,
            highest_hz: hz,
        }
    }

    fn rates_of(formats: &[SinkFormats], format: SampleFormat) -> Vec<u32> {
        formats
            .iter()
            .filter(|offered| offered.format == format)
            .flat_map(|offered| offered.rates.iter().map(|rate| rate.hz()))
            .collect()
    }

    fn spec(hz: u32, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(
            SampleRate::new(hz).expect("a rate"),
            ChannelLayout::Stereo,
            format,
        )
    }

    #[test]
    fn each_physical_depth_offers_the_words_it_carries_whole_and_thirty_two_bits_never() {
        let sixteen = hog_formats(&[at(Depth::Sixteen, 44_100.0)]);
        let twenty_four = hog_formats(&[at(Depth::TwentyFour, 96_000.0)]);
        let float = hog_formats(&[at(Depth::Float, 48_000.0)]);

        assert_eq!(rates_of(&sixteen, SampleFormat::S16), [44_100]);
        assert!(rates_of(&sixteen, SampleFormat::S24).is_empty());
        assert_eq!(rates_of(&twenty_four, SampleFormat::S16), [96_000]);
        assert_eq!(rates_of(&twenty_four, SampleFormat::S24), [96_000]);
        assert!(rates_of(&twenty_four, SampleFormat::F32).is_empty());
        assert_eq!(rates_of(&float, SampleFormat::F32), [48_000]);
        assert_eq!(rates_of(&float, SampleFormat::S24), [48_000]);
        for formats in [&sixteen, &twenty_four, &float] {
            assert!(rates_of(formats, SampleFormat::S32).is_empty());
        }
    }

    #[test]
    fn a_word_is_offered_at_every_rate_some_format_carries_it_and_at_no_other() {
        let formats = hog_formats(&[
            at(Depth::Sixteen, 44_100.0),
            at(Depth::Sixteen, 48_000.0),
            at(Depth::TwentyFour, 48_000.0),
            at(Depth::TwentyFour, 192_000.0),
        ]);

        assert_eq!(
            rates_of(&formats, SampleFormat::S16),
            [44_100, 48_000, 192_000]
        );
        assert_eq!(rates_of(&formats, SampleFormat::S24), [48_000, 192_000]);
    }

    #[test]
    fn twenty_four_bits_travel_padded_and_the_rest_whole() {
        let formats = hog_formats(&[at(Depth::ThirtyTwo, 48_000.0)]);

        for offered in &formats {
            let expected = match offered.format {
                SampleFormat::S24 => Words::Padded,
                _ => Words::Whole,
            };
            assert_eq!(offered.words, expected);
            assert_eq!(offered.channels, [ChannelLayout::Stereo]);
        }
    }

    #[test]
    fn a_range_of_rates_is_crossed_with_the_rates_music_comes_in() {
        let ranged = Physical {
            depth: Depth::Float,
            channels: 2,
            lowest_hz: 8_000.0,
            highest_hz: 96_000.0,
        };

        let formats = hog_formats(&[ranged]);

        assert_eq!(
            rates_of(&formats, SampleFormat::F32),
            [44_100, 48_000, 88_200, 96_000]
        );
    }

    #[test]
    fn a_one_channel_device_is_offered_as_mono_and_one_with_more_as_stereo() {
        let mono = Physical {
            channels: 1,
            ..at(Depth::Sixteen, 48_000.0)
        };
        let surround = Physical {
            channels: 6,
            ..at(Depth::TwentyFour, 48_000.0)
        };

        let formats = hog_formats(&[mono, surround]);

        assert!(formats.iter().any(|offered| {
            offered.format == SampleFormat::S16 && offered.channels == [ChannelLayout::Mono]
        }));
        assert!(formats.iter().any(|offered| {
            offered.format == SampleFormat::S24 && offered.channels == [ChannelLayout::Stereo]
        }));
        assert_eq!(shared_formats(SampleRate::HZ_48000, 0), []);
    }

    #[test]
    fn the_shared_sink_is_float_at_the_devices_own_rate_alone() {
        let formats = shared_formats(SampleRate::HZ_44100, 2);

        assert_eq!(
            formats,
            [SinkFormats {
                format: SampleFormat::F32,
                words: Words::Whole,
                rates: vec![SampleRate::HZ_44100],
                channels: vec![ChannelLayout::Stereo],
            }]
        );
    }

    #[test]
    fn the_least_depth_that_holds_the_word_at_its_rate_is_chosen_and_float_only_failing_that() {
        let offered = [
            at(Depth::Float, 96_000.0),
            at(Depth::ThirtyTwo, 96_000.0),
            at(Depth::TwentyFour, 96_000.0),
            at(Depth::Sixteen, 96_000.0),
            at(Depth::Float, 44_100.0),
        ];

        let depth = |spec| physical_for(spec, &offered).map(|physical| physical.depth);

        assert_eq!(depth(spec(96_000, SampleFormat::S16)), Some(Depth::Sixteen));
        assert_eq!(
            depth(spec(96_000, SampleFormat::S24)),
            Some(Depth::TwentyFour)
        );
        assert_eq!(depth(spec(96_000, SampleFormat::F32)), Some(Depth::Float));
        assert_eq!(depth(spec(44_100, SampleFormat::S24)), Some(Depth::Float));
        assert_eq!(depth(spec(96_000, SampleFormat::S32)), None);
        assert_eq!(depth(spec(48_000, SampleFormat::S16)), None);
    }

    #[test]
    fn each_word_reaches_the_unit_in_a_client_format_that_carries_it_exactly() {
        assert_eq!(
            ClientWord::of(SampleFormat::S16),
            Some(ClientWord::Signed16)
        );
        assert_eq!(
            ClientWord::of(SampleFormat::S24),
            Some(ClientWord::Signed32)
        );
        assert_eq!(ClientWord::of(SampleFormat::F32), Some(ClientWord::Float32));
        assert_eq!(ClientWord::of(SampleFormat::S32), None);
        assert_eq!(laid_of(SampleFormat::S24), Laid::TwentyFourAgainstTheTop);
        assert_eq!(laid_of(SampleFormat::S16), Laid::AsHeld);
        assert_eq!(frame_of(spec(48_000, SampleFormat::S24)).get(), 8);
        assert_eq!(frame_of(spec(48_000, SampleFormat::S16)).get(), 4);
    }

    #[test]
    fn a_rate_no_stream_can_run_at_names_no_rate() {
        assert_eq!(rate_of(44_100.0), Some(SampleRate::HZ_44100));
        assert_eq!(rate_of(0.0), None);
        assert_eq!(rate_of(f64::NAN), None);
        assert_eq!(rate_of(4.0e9), None);
    }
}
