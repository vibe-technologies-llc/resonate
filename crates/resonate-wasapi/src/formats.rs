use resonate_audio::{SinkFormats, Words};
use resonate_core::{ChannelLayout, SampleFormat, SampleRate};

pub const PROBED_RATES: [u32; 8] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000,
];

pub const PROBED_LAYOUTS: [ChannelLayout; 4] = [
    ChannelLayout::Stereo,
    ChannelLayout::Mono,
    ChannelLayout::Surround51,
    ChannelLayout::Surround71,
];

const SPEAKER_FRONT_LEFT: u32 = 0x1;
const SPEAKER_FRONT_RIGHT: u32 = 0x2;
const SPEAKER_FRONT_CENTER: u32 = 0x4;
const SPEAKER_LOW_FREQUENCY: u32 = 0x8;
const SPEAKER_BACK_LEFT: u32 = 0x10;
const SPEAKER_BACK_RIGHT: u32 = 0x20;
const SPEAKER_SIDE_LEFT: u32 = 0x200;
const SPEAKER_SIDE_RIGHT: u32 = 0x400;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wire {
    pub store_bits: usize,
    pub valid_bits: usize,
    pub float: bool,
}

pub const fn wire_of(format: SampleFormat) -> Wire {
    match format {
        SampleFormat::S16 => Wire {
            store_bits: 16,
            valid_bits: 16,
            float: false,
        },
        SampleFormat::S24 => Wire {
            store_bits: 32,
            valid_bits: 24,
            float: false,
        },
        SampleFormat::S32 => Wire {
            store_bits: 32,
            valid_bits: 32,
            float: false,
        },
        SampleFormat::F32 => Wire {
            store_bits: 32,
            valid_bits: 32,
            float: true,
        },
    }
}

pub const fn format_of(wire: Wire) -> Option<SampleFormat> {
    match (wire.store_bits, wire.valid_bits, wire.float) {
        (16, 16, false) => Some(SampleFormat::S16),
        (32, 24, false) => Some(SampleFormat::S24),
        (32, 32, false) => Some(SampleFormat::S32),
        (32, 32, true) => Some(SampleFormat::F32),
        _ => None,
    }
}

pub const fn mask_of(layout: ChannelLayout) -> u32 {
    match layout {
        ChannelLayout::Mono => SPEAKER_FRONT_CENTER,
        ChannelLayout::Stereo => SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT,
        ChannelLayout::Quad => {
            SPEAKER_FRONT_LEFT | SPEAKER_FRONT_RIGHT | SPEAKER_BACK_LEFT | SPEAKER_BACK_RIGHT
        }
        ChannelLayout::Surround50 => {
            SPEAKER_FRONT_LEFT
                | SPEAKER_FRONT_RIGHT
                | SPEAKER_FRONT_CENTER
                | SPEAKER_BACK_LEFT
                | SPEAKER_BACK_RIGHT
        }
        ChannelLayout::Surround51 => {
            SPEAKER_FRONT_LEFT
                | SPEAKER_FRONT_RIGHT
                | SPEAKER_FRONT_CENTER
                | SPEAKER_LOW_FREQUENCY
                | SPEAKER_BACK_LEFT
                | SPEAKER_BACK_RIGHT
        }
        ChannelLayout::Surround61 => 0,
        ChannelLayout::Surround71 => {
            SPEAKER_FRONT_LEFT
                | SPEAKER_FRONT_RIGHT
                | SPEAKER_FRONT_CENTER
                | SPEAKER_LOW_FREQUENCY
                | SPEAKER_BACK_LEFT
                | SPEAKER_BACK_RIGHT
                | SPEAKER_SIDE_LEFT
                | SPEAKER_SIDE_RIGHT
        }
        ChannelLayout::Discrete(_) => 0,
    }
}

pub fn formats_taken(
    mut probe: impl FnMut(SampleFormat, SampleRate, ChannelLayout) -> bool,
) -> Vec<SinkFormats> {
    let mut taken = Vec::new();
    for format in SampleFormat::ALL {
        for layout in PROBED_LAYOUTS {
            let rates: Vec<SampleRate> = PROBED_RATES
                .iter()
                .filter_map(|hz| SampleRate::new(*hz).ok())
                .filter(|rate| probe(format, *rate, layout))
                .collect();
            if rates.is_empty() {
                continue;
            }
            taken.push(SinkFormats {
                format,
                words: words_of(format),
                rates,
                channels: vec![layout],
            });
        }
    }
    taken
}

pub fn rates_of(formats: &[SinkFormats]) -> Vec<SampleRate> {
    let mut rates: Vec<SampleRate> = formats
        .iter()
        .flat_map(|entry| entry.rates.iter().copied())
        .collect();
    rates.sort();
    rates.dedup();
    rates
}

const fn words_of(format: SampleFormat) -> Words {
    match format {
        SampleFormat::S24 => Words::Padded,
        SampleFormat::S16 | SampleFormat::S32 | SampleFormat::F32 => Words::Whole,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_is_read_back_from_the_wire_it_is_written_as() {
        for format in SampleFormat::ALL {
            assert_eq!(format_of(wire_of(format)), Some(format));
        }
        assert_eq!(
            format_of(Wire {
                store_bits: 24,
                valid_bits: 24,
                float: false
            }),
            None
        );
    }

    #[test]
    fn a_device_taking_only_some_combinations_is_listed_as_exactly_those() {
        let taken = formats_taken(|format, rate, layout| {
            layout == ChannelLayout::Stereo
                && match format {
                    SampleFormat::S16 => rate.hz() <= 48_000,
                    SampleFormat::S24 => true,
                    SampleFormat::S32 | SampleFormat::F32 => false,
                }
        });

        assert_eq!(taken.len(), 2);
        let sixteen = taken
            .iter()
            .find(|entry| entry.format == SampleFormat::S16)
            .expect("sixteen bits listed");
        assert_eq!(sixteen.rates.len(), 2);
        assert_eq!(sixteen.channels, [ChannelLayout::Stereo]);
        let twenty_four = taken
            .iter()
            .find(|entry| entry.format == SampleFormat::S24)
            .expect("twenty-four bits listed");
        assert_eq!(twenty_four.words, Words::Padded);
        assert_eq!(twenty_four.rates.len(), PROBED_RATES.len());
        assert_eq!(rates_of(&taken).len(), PROBED_RATES.len());
    }

    #[test]
    fn a_device_taking_nothing_lists_no_format() {
        assert!(formats_taken(|_, _, _| false).is_empty());
    }

    #[test]
    fn the_masks_place_the_channels_in_the_order_the_workspace_lays_them() {
        assert_eq!(mask_of(ChannelLayout::Stereo), 0x3);
        assert_eq!(mask_of(ChannelLayout::Surround51), 0x3F);
        assert_eq!(mask_of(ChannelLayout::Surround71), 0x63F);
        assert_eq!(mask_of(ChannelLayout::Mono), 0x4);
    }
}
