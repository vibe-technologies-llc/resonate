use resonate_audio::{HardwareVolume, Plugged, SinkInfo, SinkPort, sink_id_of};
use resonate_core::{Gain, SampleRate};

use crate::{
    formats::{Physical, hog_formats, shared_formats},
    ways::{Way, node_of},
    words,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub gain: Gain,
    pub is_the_devices_own: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub uid: String,
    pub name: String,
    pub is_hardware: bool,
    pub can_be_hogged: bool,
    pub nominal: Option<SampleRate>,
    pub channels: u32,
    pub offered: Vec<Physical>,
    pub plugged: Plugged,
    pub level: Option<Level>,
    pub muted: bool,
}

impl Seen {
    fn port(&self) -> SinkPort {
        SinkPort {
            description: self.name.clone(),
            plugged: self.plugged,
            hardware_volume: match self.level {
                Some(level) if level.is_the_devices_own => HardwareVolume::Yes,
                _ => HardwareVolume::No,
            },
            volume: self.level.map(|level| level.gain),
            muted: self.muted,
        }
    }

    fn shared(&self, is_default: bool) -> Option<SinkInfo> {
        let nominal = self.nominal?;
        let formats = shared_formats(nominal, self.channels);
        if formats.is_empty() {
            return None;
        }
        let name = node_of(Way::Shared, &self.uid);
        Some(SinkInfo {
            id: sink_id_of(&name),
            name,
            aliases: Vec::new(),
            description: self.name.clone(),
            is_default,
            is_hardware: self.is_hardware,
            port: Some(self.port()),
            profile: None,
            formats,
            allowed_rates: vec![nominal],
            current_rate: Some(nominal),
        })
    }

    fn hogged(&self) -> Option<SinkInfo> {
        let formats = hog_formats(&self.offered);
        if !self.can_be_hogged || formats.is_empty() {
            return None;
        }
        let mut allowed_rates: Vec<SampleRate> = formats
            .iter()
            .flat_map(|offered| offered.rates.iter().copied())
            .collect();
        allowed_rates.sort_unstable();
        allowed_rates.dedup();
        let name = node_of(Way::Hog, &self.uid);
        Some(SinkInfo {
            id: sink_id_of(&name),
            name,
            aliases: Vec::new(),
            description: words::sinks::hogged(self.name.as_str()),
            is_default: false,
            is_hardware: self.is_hardware,
            port: Some(self.port()),
            profile: None,
            formats,
            allowed_rates,
            current_rate: None,
        })
    }
}

pub fn sinks_of(seen: &[Seen], default_uid: Option<&str>) -> Vec<SinkInfo> {
    seen.iter()
        .flat_map(|device| {
            let is_default = default_uid == Some(device.uid.as_str());
            [device.shared(is_default), device.hogged()]
        })
        .flatten()
        .collect()
}

#[cfg(test)]
mod tests {
    use resonate_audio::NodeName;
    use resonate_core::SampleFormat;

    use super::*;
    use crate::formats::Depth;

    fn dac() -> Seen {
        Seen {
            uid: "AppleUSBAudioEngine:DAC".to_owned(),
            name: "Desk DAC".to_owned(),
            is_hardware: true,
            can_be_hogged: true,
            nominal: Some(SampleRate::HZ_48000),
            channels: 2,
            offered: vec![
                Physical {
                    depth: Depth::TwentyFour,
                    channels: 2,
                    lowest_hz: 44_100.0,
                    highest_hz: 44_100.0,
                },
                Physical {
                    depth: Depth::TwentyFour,
                    channels: 2,
                    lowest_hz: 96_000.0,
                    highest_hz: 96_000.0,
                },
            ],
            plugged: Plugged::Unsaid,
            level: Some(Level {
                gain: Gain::UNITY,
                is_the_devices_own: true,
            }),
            muted: false,
        }
    }

    fn named(sinks: &[SinkInfo], name: &str) -> SinkInfo {
        sinks
            .iter()
            .find(|sink| sink.name == NodeName::new(name))
            .cloned()
            .expect("the sink is listed")
    }

    #[test]
    fn a_device_is_two_sinks_the_shared_one_at_its_own_rate_and_the_hogged_one_at_every_rate() {
        let sinks = sinks_of(&[dac()], Some("AppleUSBAudioEngine:DAC"));

        let shared = named(&sinks, "coreaudio:AppleUSBAudioEngine:DAC");
        let hogged = named(&sinks, "coreaudio-hog:AppleUSBAudioEngine:DAC");

        assert_eq!(sinks.len(), 2);
        assert!(shared.is_default);
        assert_eq!(shared.description, "Desk DAC");
        assert_eq!(shared.current_rate, Some(SampleRate::HZ_48000));
        assert_eq!(shared.allowed_rates, [SampleRate::HZ_48000]);
        assert_eq!(shared.formats.len(), 1);
        assert_eq!(shared.formats[0].format, SampleFormat::F32);
        assert!(!hogged.is_default);
        assert_eq!(hogged.current_rate, None);
        assert_eq!(
            hogged.allowed_rates,
            [SampleRate::HZ_44100, SampleRate::HZ_96000]
        );
        assert_ne!(hogged.description, shared.description);
        assert!(hogged.description.contains("Desk DAC"));
        assert_ne!(shared.id, hogged.id);
    }

    #[test]
    fn only_the_systems_default_device_is_the_default_sink() {
        let other = Seen {
            uid: "BuiltInSpeakerDevice".to_owned(),
            ..dac()
        };

        let sinks = sinks_of(&[dac(), other], Some("BuiltInSpeakerDevice"));

        assert!(named(&sinks, "coreaudio:BuiltInSpeakerDevice").is_default);
        assert!(!named(&sinks, "coreaudio:AppleUSBAudioEngine:DAC").is_default);
    }

    #[test]
    fn an_aggregate_and_a_device_offering_no_format_have_no_hogged_sink() {
        let aggregate = Seen {
            can_be_hogged: false,
            ..dac()
        };
        let unread = Seen {
            uid: "Unread".to_owned(),
            offered: Vec::new(),
            ..dac()
        };

        let sinks = sinks_of(&[aggregate, unread], None);

        assert_eq!(sinks.len(), 2);
        assert!(
            sinks
                .iter()
                .all(|sink| sink.name.as_str().starts_with("coreaudio:"))
        );
    }

    #[test]
    fn a_device_whose_rate_cannot_be_read_is_not_offered_shared() {
        let unrated = Seen {
            nominal: None,
            ..dac()
        };

        let sinks = sinks_of(&[unrated], None);

        assert_eq!(sinks.len(), 1);
        assert!(sinks[0].name.as_str().starts_with("coreaudio-hog:"));
    }

    #[test]
    fn the_port_says_whose_volume_it_is_and_a_device_with_no_control_has_none() {
        let softened = Seen {
            level: Some(Level {
                gain: Gain::UNITY,
                is_the_devices_own: false,
            }),
            muted: true,
            ..dac()
        };
        let fixed = Seen {
            uid: "Fixed".to_owned(),
            level: None,
            ..dac()
        };

        let sinks = sinks_of(&[dac(), softened.clone(), fixed], None);
        let port = |index: usize| sinks[index].port.clone().expect("a port");

        assert_eq!(port(0).hardware_volume, HardwareVolume::Yes);
        assert_eq!(port(0).volume, Some(Gain::UNITY));
        assert_eq!(port(2).hardware_volume, HardwareVolume::No);
        assert!(port(2).muted);
        assert_eq!(port(4).hardware_volume, HardwareVolume::No);
        assert_eq!(port(4).volume, None);
    }
}
