use resonate_audio::{SinkFormats, Words};
use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use super::formats::{OutputEncoding, OutputFormat, OutputPath};

pub(crate) const MIXED: StreamSpec = StreamSpec::new(
    SampleRate::HZ_48000,
    ChannelLayout::Stereo,
    SampleFormat::F32,
);

impl OutputFormat {
    pub(crate) fn spec(self) -> Option<StreamSpec> {
        let rate = SampleRate::new(self.rate).ok()?;
        let channels = match self.channels {
            1 => ChannelLayout::Mono,
            2 => ChannelLayout::Stereo,
            _ => return None,
        };
        let format = match self.encoding {
            OutputEncoding::Pcm16 => SampleFormat::S16,
            OutputEncoding::Pcm32 => SampleFormat::S32,
            OutputEncoding::Float => SampleFormat::F32,
        };
        Some(StreamSpec::new(rate, channels, format))
    }

    pub(crate) fn of(spec: StreamSpec) -> Option<Self> {
        let channels = match spec.channels {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
            _ => return None,
        };
        let encoding = match spec.format {
            SampleFormat::S16 => OutputEncoding::Pcm16,
            SampleFormat::S32 => OutputEncoding::Pcm32,
            SampleFormat::F32 => OutputEncoding::Float,
            SampleFormat::S24 => return None,
        };
        Some(Self {
            rate: spec.rate.hz(),
            encoding,
            channels,
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Offers {
    pub(crate) device: i32,
    pub(crate) bit_perfect: Vec<StreamSpec>,
    pub(crate) exclusive: Option<StreamSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Way {
    Mixed,
    Exclusive,
    BitPerfect { device: i32 },
}

impl Way {
    pub(crate) const fn path(self) -> OutputPath {
        match self {
            Self::Mixed => OutputPath::Mixed,
            Self::Exclusive => OutputPath::Exclusive,
            Self::BitPerfect { .. } => OutputPath::BitPerfect,
        }
    }
}

impl Offers {
    pub(crate) fn of(device: i32, formats: &[OutputFormat]) -> Self {
        let mut bit_perfect = Vec::new();
        for spec in formats.iter().filter_map(|format| format.spec()) {
            if !bit_perfect.contains(&spec) {
                bit_perfect.push(spec);
            }
        }
        Self {
            device,
            bit_perfect,
            exclusive: None,
        }
    }

    pub(crate) fn offered(&self, follows: bool) -> Vec<StreamSpec> {
        let mut specs = vec![MIXED];
        if !follows {
            return specs;
        }
        for spec in self.bit_perfect.iter().copied().chain(self.exclusive) {
            if !specs.contains(&spec) {
                specs.push(spec);
            }
        }
        specs
    }

    pub(crate) fn way_for(&self, spec: StreamSpec, follows: bool) -> Way {
        if !follows {
            return Way::Mixed;
        }
        if self.bit_perfect.contains(&spec) {
            return Way::BitPerfect {
                device: self.device,
            };
        }
        if self.exclusive == Some(spec) {
            return Way::Exclusive;
        }
        Way::Mixed
    }
}

pub(crate) fn sink_formats(specs: &[StreamSpec]) -> (Vec<SinkFormats>, Vec<SampleRate>) {
    let formats = specs
        .iter()
        .map(|spec| SinkFormats {
            format: spec.format,
            words: Words::Whole,
            rates: vec![spec.rate],
            channels: vec![spec.channels],
        })
        .collect();
    let mut rates: Vec<SampleRate> = specs.iter().map(|spec| spec.rate).collect();
    rates.sort_by_key(|rate| rate.hz());
    rates.dedup();
    (formats, rates)
}

#[cfg(test)]
mod tests {
    use resonate_audio::{HardwareVolume, NodeName, Plugged, SinkId, SinkInfo, SinkPort};

    use super::*;

    fn format(rate: u32, encoding: OutputEncoding, channels: u32) -> OutputFormat {
        OutputFormat {
            rate,
            encoding,
            channels,
        }
    }

    fn sink_offering(specs: &[StreamSpec]) -> SinkInfo {
        let (formats, allowed_rates) = sink_formats(specs);
        SinkInfo {
            id: SinkId::new(1),
            name: NodeName::new("android-route"),
            description: "Android audio output".into(),
            is_default: true,
            is_hardware: false,
            port: Some(SinkPort {
                description: "System route".into(),
                plugged: Plugged::Yes,
                hardware_volume: HardwareVolume::No,
                volume: None,
                muted: false,
            }),
            profile: None,
            formats,
            allowed_rates,
            current_rate: Some(MIXED.rate),
        }
    }

    const CD: StreamSpec = StreamSpec::new(
        SampleRate::HZ_44100,
        ChannelLayout::Stereo,
        SampleFormat::S16,
    );
    const STUDIO: StreamSpec = StreamSpec::new(
        SampleRate::HZ_96000,
        ChannelLayout::Stereo,
        SampleFormat::S24,
    );

    #[test]
    fn a_format_the_boundary_names_is_the_spec_the_engine_plans_for_and_back() {
        for format in [
            format(44_100, OutputEncoding::Pcm16, 2),
            format(96_000, OutputEncoding::Pcm32, 2),
            format(48_000, OutputEncoding::Float, 1),
        ] {
            let spec = format.spec().expect("a spec");
            assert_eq!(OutputFormat::of(spec), Some(format));
        }
        assert_eq!(OutputFormat::of(STUDIO), None);
    }

    #[test]
    fn a_format_no_stream_can_carry_is_left_out_of_what_is_offered() {
        let offers = Offers::of(
            7,
            &[
                format(0, OutputEncoding::Pcm16, 2),
                format(44_100, OutputEncoding::Pcm16, 6),
                format(44_100, OutputEncoding::Pcm16, 2),
                format(44_100, OutputEncoding::Pcm16, 2),
            ],
        );

        assert_eq!(offers.bit_perfect, vec![CD]);
    }

    #[test]
    fn without_following_the_song_only_the_shared_mixer_is_offered_and_every_stream_is_mixed() {
        let mut offers = Offers::of(7, &[format(44_100, OutputEncoding::Pcm16, 2)]);
        offers.exclusive = Some(MIXED);

        assert_eq!(offers.offered(false), vec![MIXED]);
        assert_eq!(offers.way_for(CD, false), Way::Mixed);
        assert_eq!(offers.way_for(MIXED, false), Way::Mixed);
    }

    #[test]
    fn following_the_song_offers_the_bit_perfect_and_exclusive_formats_beside_the_mixer() {
        let exclusive = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::S32,
        );
        let mut offers = Offers::of(7, &[format(44_100, OutputEncoding::Pcm16, 2)]);
        offers.exclusive = Some(exclusive);

        assert_eq!(offers.offered(true), vec![MIXED, CD, exclusive]);
        assert_eq!(offers.way_for(CD, true), Way::BitPerfect { device: 7 });
        assert_eq!(offers.way_for(exclusive, true), Way::Exclusive);
        assert_eq!(offers.way_for(MIXED, true), Way::Mixed);
        assert_eq!(Way::BitPerfect { device: 7 }.path(), OutputPath::BitPerfect);
        assert_eq!(Way::Exclusive.path(), OutputPath::Exclusive);
        assert_eq!(Way::Mixed.path(), OutputPath::Mixed);
    }

    #[test]
    fn a_cd_song_is_planned_bit_perfect_where_the_route_offers_its_format_and_resampled_where_not()
    {
        let offered = Offers::of(7, &[format(44_100, OutputEncoding::Pcm16, 2)]);

        let following = sink_offering(&offered.offered(true));
        assert_eq!(following.best_spec_for(CD), Some(CD));

        let mixed = sink_offering(&offered.offered(false));
        assert_eq!(mixed.best_spec_for(CD), Some(MIXED));
    }

    #[test]
    fn a_twenty_four_bit_song_is_carried_whole_in_a_wider_word_the_route_offers() {
        let offered = Offers::of(
            7,
            &[
                format(96_000, OutputEncoding::Pcm16, 2),
                format(96_000, OutputEncoding::Pcm32, 2),
            ],
        );
        let sink = sink_offering(&offered.offered(true));

        let chosen = sink.best_spec_for(STUDIO).expect("a spec");
        assert_eq!(chosen.rate, SampleRate::HZ_96000);
        assert_eq!(chosen.format, SampleFormat::S32);
        assert!(chosen.losslessly_holds(STUDIO));
    }

    #[test]
    fn every_spec_offered_is_its_own_entry_so_no_rate_and_word_are_crossed() {
        let specs = [
            CD,
            StreamSpec::new(
                SampleRate::HZ_96000,
                ChannelLayout::Stereo,
                SampleFormat::S32,
            ),
        ];
        let sink = sink_offering(&specs);

        assert!(sink.supports(specs[0]));
        assert!(sink.supports(specs[1]));
        assert!(!sink.supports(StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Stereo,
            SampleFormat::S16
        )));
        assert_eq!(
            sink.allowed_rates,
            vec![SampleRate::HZ_44100, SampleRate::HZ_96000]
        );
    }
}
