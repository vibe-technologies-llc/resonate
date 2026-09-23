use std::iter;

use libspa::{
    param::{
        audio::{AudioFormat, AudioInfoRaw},
        format::{MediaSubtype, MediaType},
        format_utils,
    },
    pod::{ChoiceValue, Pod, Value, ValueArray},
    sys,
    utils::{ChoiceEnum, Direction, Id},
};
use resonate_core::{ChannelCount, ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use crate::{HardwareVolume, Plugged, SinkPort};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum WireWord {
    #[default]
    Padded,
    Packed,
}

impl WireWord {
    pub(crate) const fn of(format: AudioFormat) -> Self {
        match format {
            AudioFormat::S24LE => Self::Packed,
            _ => Self::Padded,
        }
    }

    pub(crate) const fn is_packed(self) -> bool {
        matches!(self, Self::Packed)
    }
}

pub(crate) fn sample_format(raw: u32) -> Option<SampleFormat> {
    match AudioFormat(raw) {
        AudioFormat::S16LE => Some(SampleFormat::S16),
        AudioFormat::S24LE | AudioFormat::S24_32LE => Some(SampleFormat::S24),
        AudioFormat::S32LE => Some(SampleFormat::S32),
        AudioFormat::F32LE => Some(SampleFormat::F32),
        _ => None,
    }
}

pub(crate) const fn spa_format(format: SampleFormat, word: WireWord) -> AudioFormat {
    match (format, word) {
        (SampleFormat::S16, _) => AudioFormat::S16LE,
        (SampleFormat::S24, WireWord::Padded) => AudioFormat::S24_32LE,
        (SampleFormat::S24, WireWord::Packed) => AudioFormat::S24LE,
        (SampleFormat::S32, _) => AudioFormat::S32LE,
        (SampleFormat::F32, _) => AudioFormat::F32LE,
    }
}

pub(crate) const fn packs_narrower(format: SampleFormat) -> bool {
    matches!(format, SampleFormat::S24)
}

fn one_of_each(raw: impl Iterator<Item = u32>) -> Vec<SampleFormat> {
    let mut kept: Vec<SampleFormat> = Vec::new();
    for format in raw.filter_map(sample_format) {
        if !kept.contains(&format) {
            kept.push(format);
        }
    }
    kept
}

pub(crate) const fn spa_position(layout: ChannelLayout) -> [u32; 8] {
    match layout {
        ChannelLayout::Mono => [sys::SPA_AUDIO_CHANNEL_MONO, 0, 0, 0, 0, 0, 0, 0],
        ChannelLayout::Stereo => [
            sys::SPA_AUDIO_CHANNEL_FL,
            sys::SPA_AUDIO_CHANNEL_FR,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        ChannelLayout::Quad => [
            sys::SPA_AUDIO_CHANNEL_FL,
            sys::SPA_AUDIO_CHANNEL_FR,
            sys::SPA_AUDIO_CHANNEL_RL,
            sys::SPA_AUDIO_CHANNEL_RR,
            0,
            0,
            0,
            0,
        ],
        ChannelLayout::Surround51 => [
            sys::SPA_AUDIO_CHANNEL_FL,
            sys::SPA_AUDIO_CHANNEL_FR,
            sys::SPA_AUDIO_CHANNEL_FC,
            sys::SPA_AUDIO_CHANNEL_LFE,
            sys::SPA_AUDIO_CHANNEL_RL,
            sys::SPA_AUDIO_CHANNEL_RR,
            0,
            0,
        ],
        ChannelLayout::Surround71 => [
            sys::SPA_AUDIO_CHANNEL_FL,
            sys::SPA_AUDIO_CHANNEL_FR,
            sys::SPA_AUDIO_CHANNEL_FC,
            sys::SPA_AUDIO_CHANNEL_LFE,
            sys::SPA_AUDIO_CHANNEL_RL,
            sys::SPA_AUDIO_CHANNEL_RR,
            sys::SPA_AUDIO_CHANNEL_SL,
            sys::SPA_AUDIO_CHANNEL_SR,
        ],
        ChannelLayout::Discrete(_) => [0; 8],
    }
}

fn ids(value: &Value) -> Vec<u32> {
    match value {
        Value::Id(Id(raw)) => vec![*raw],
        Value::Choice(ChoiceValue::Id(choice)) => match &choice.1 {
            ChoiceEnum::None(Id(raw)) => vec![*raw],
            ChoiceEnum::Enum {
                default: Id(raw),
                alternatives,
            } => {
                let mut all = vec![*raw];
                all.extend(alternatives.iter().map(|Id(raw)| *raw));
                all
            }
            ChoiceEnum::Range {
                default: Id(raw), ..
            }
            | ChoiceEnum::Step {
                default: Id(raw), ..
            } => vec![*raw],
            ChoiceEnum::Flags {
                default: Id(raw), ..
            } => vec![*raw],
        },
        _ => Vec::new(),
    }
}

const RATES_A_CONTINUUM_IS_READ_AS: [i32; 16] = [
    8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 64_000, 88_200, 96_000, 176_400,
    192_000, 352_800, 384_000, 705_600, 768_000,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spanned {
    default: i32,
    min: i32,
    max: i32,
    step: i32,
}

impl Spanned {
    const fn holds(self, value: i32) -> bool {
        value >= self.min
            && value <= self.max
            && (self.step <= 1 || (value - self.min) % self.step == 0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Offered {
    Listed(Vec<i32>),
    Spanned(Spanned),
}

impl Offered {
    fn read_as(self, named: impl Iterator<Item = i32>) -> Vec<i32> {
        match self {
            Self::Listed(values) => values,
            Self::Spanned(span) => {
                let mut values: Vec<i32> = iter::once(span.default)
                    .chain(named.filter(|value| span.holds(*value)))
                    .collect();
                values.sort_unstable();
                values.dedup();
                values
            }
        }
    }
}

fn offered(value: &Value) -> Option<Offered> {
    match value {
        Value::Int(raw) => Some(Offered::Listed(vec![*raw])),
        Value::Choice(ChoiceValue::Int(choice)) => Some(match &choice.1 {
            ChoiceEnum::None(raw) => Offered::Listed(vec![*raw]),
            ChoiceEnum::Enum {
                default,
                alternatives,
            } => {
                let mut all = vec![*default];
                all.extend(alternatives.iter().copied());
                Offered::Listed(all)
            }
            ChoiceEnum::Range { default, min, max } => Offered::Spanned(Spanned {
                default: *default,
                min: *min,
                max: *max,
                step: 1,
            }),
            ChoiceEnum::Step {
                default,
                min,
                max,
                step,
            } => Offered::Spanned(Spanned {
                default: *default,
                min: *min,
                max: *max,
                step: *step,
            }),
            ChoiceEnum::Flags { default, .. } => Offered::Listed(vec![*default]),
        }),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AdvertisedFormat {
    pub formats: Vec<SampleFormat>,
    pub rates: Vec<SampleRate>,
    pub layouts: Vec<ChannelLayout>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Negotiated {
    pub spec: StreamSpec,
    pub word: WireWord,
}

pub(crate) fn negotiated(param: &Pod) -> Option<Negotiated> {
    let (media_type, media_subtype) = format_utils::parse_format(param)
        .inspect_err(|error| tracing::debug!(%error, "a stream format pod named no media type"))
        .ok()?;
    if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
        return None;
    }

    let mut info = AudioInfoRaw::default();
    info.parse(param)
        .inspect_err(|error| tracing::debug!(%error, "a raw audio format pod would not parse"))
        .ok()?;

    let raw = info.format();
    let format = sample_format(raw.as_raw())?;
    let rate = SampleRate::new(info.rate()).ok()?;
    let channels = ChannelCount::new(u16::try_from(info.channels()).ok()?).ok()?;

    Some(Negotiated {
        spec: StreamSpec::new(rate, ChannelLayout::from_count(channels), format),
        word: WireWord::of(raw),
    })
}

pub(crate) fn parse_enum_format(value: &Value) -> Option<AdvertisedFormat> {
    let Value::Object(object) = value else {
        return None;
    };
    if object.type_ != sys::SPA_TYPE_OBJECT_Format {
        return None;
    }

    let mut advertised = AdvertisedFormat::default();
    let mut media_type = None;
    let mut media_subtype = None;

    for property in &object.properties {
        match property.key {
            sys::SPA_FORMAT_mediaType => media_type = ids(&property.value).first().copied(),
            sys::SPA_FORMAT_mediaSubtype => media_subtype = ids(&property.value).first().copied(),
            sys::SPA_FORMAT_AUDIO_format => {
                advertised.formats = one_of_each(ids(&property.value).into_iter());
            }
            sys::SPA_FORMAT_AUDIO_rate => {
                advertised.rates = offered(&property.value)
                    .map(|choice| choice.read_as(RATES_A_CONTINUUM_IS_READ_AS.into_iter()))
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|hz| u32::try_from(hz).ok())
                    .filter_map(|hz| SampleRate::new(hz).ok())
                    .collect();
            }
            sys::SPA_FORMAT_AUDIO_channels => {
                advertised.layouts = offered(&property.value)
                    .map(|choice| choice.read_as(1..=i32::from(ChannelCount::MAX.get())))
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|count| u16::try_from(count).ok())
                    .filter_map(|count| ChannelCount::new(count).ok())
                    .map(ChannelLayout::from_count)
                    .collect();
            }
            _ => {}
        }
    }

    if media_type != Some(sys::SPA_MEDIA_TYPE_audio)
        || media_subtype != Some(sys::SPA_MEDIA_SUBTYPE_raw)
    {
        return None;
    }
    Some(advertised)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AdvertisedRoute {
    pub(crate) seats: Vec<i32>,
    pub(crate) port: SinkPort,
}

pub(crate) fn parse_route(value: &Value) -> Option<AdvertisedRoute> {
    let Value::Object(object) = value else {
        return None;
    };
    if object.type_ != sys::SPA_TYPE_OBJECT_ParamRoute {
        return None;
    }

    let mut seat = None;
    let mut seats = Vec::new();
    let mut description = None;
    let mut name = None;
    let mut plugged = Plugged::Unsaid;
    let mut hardware_volume = HardwareVolume::Unsaid;
    let mut direction = None;

    for property in &object.properties {
        match (property.key, &property.value) {
            (sys::SPA_PARAM_ROUTE_direction, value) => direction = ids(value).first().copied(),
            (sys::SPA_PARAM_ROUTE_device, Value::Int(index)) => seat = Some(*index),
            (sys::SPA_PARAM_ROUTE_devices, Value::ValueArray(ValueArray::Int(indexes))) => {
                seats = indexes.clone();
            }
            (sys::SPA_PARAM_ROUTE_name, Value::String(named)) => name = Some(named.clone()),
            (sys::SPA_PARAM_ROUTE_description, Value::String(drawn)) => {
                description = Some(drawn.clone());
            }
            (sys::SPA_PARAM_ROUTE_available, value) => {
                plugged = ids(value)
                    .first()
                    .copied()
                    .map_or(Plugged::Unsaid, plugged_as);
            }
            (sys::SPA_PARAM_ROUTE_info, Value::Struct(items)) => {
                hardware_volume = hardware_volume_of(items);
            }
            _ => {}
        }
    }

    if direction != Some(Direction::Output.as_raw()) {
        return None;
    }
    seats.extend(seat.filter(|held| !seats.contains(held)));
    if seats.is_empty() {
        return None;
    }

    Some(AdvertisedRoute {
        seats,
        port: SinkPort {
            description: description.or(name)?,
            plugged,
            hardware_volume,
        },
    })
}

const ROUTE_HARDWARE_VOLUME: &str = "route.hw-volume";

fn hardware_volume_of(items: &[Value]) -> HardwareVolume {
    let mut said = items.iter().filter_map(|item| match item {
        Value::String(text) => Some(text.as_str()),
        _ => None,
    });

    while let Some(key) = said.next() {
        let Some(value) = said.next() else {
            return HardwareVolume::Unsaid;
        };
        if key == ROUTE_HARDWARE_VOLUME {
            return match value {
                "true" => HardwareVolume::Yes,
                "false" => HardwareVolume::No,
                _ => HardwareVolume::Unsaid,
            };
        }
    }
    HardwareVolume::Unsaid
}

pub(crate) fn parse_profile(value: &Value) -> Option<String> {
    let Value::Object(object) = value else {
        return None;
    };
    if object.type_ != sys::SPA_TYPE_OBJECT_ParamProfile {
        return None;
    }

    let mut description = None;
    let mut name = None;

    for property in &object.properties {
        match (property.key, &property.value) {
            (sys::SPA_PARAM_PROFILE_description, Value::String(drawn)) => {
                description = Some(drawn.clone());
            }
            (sys::SPA_PARAM_PROFILE_name, Value::String(named)) => name = Some(named.clone()),
            _ => {}
        }
    }

    description.or(name)
}

const fn plugged_as(availability: u32) -> Plugged {
    match availability {
        sys::SPA_PARAM_AVAILABILITY_no => Plugged::No,
        sys::SPA_PARAM_AVAILABILITY_yes => Plugged::Yes,
        _ => Plugged::Unsaid,
    }
}

pub(crate) fn parse_allowed_rates(json: &str) -> Vec<SampleRate> {
    json.trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace())
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|piece| !piece.is_empty())
        .filter_map(|piece| piece.parse::<u32>().ok())
        .filter_map(|hz| SampleRate::new(hz).ok())
        .collect()
}

pub(crate) fn parse_rate(value: &str) -> Option<SampleRate> {
    let hz = value.trim().parse::<u32>().ok()?;
    SampleRate::new(hz).ok()
}

pub(crate) fn parse_default_sink(json: &str) -> Option<String> {
    let key = "\"name\"";
    let rest = json.split_once(key)?.1;
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let (name, _) = rest.split_once('"')?;
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use libspa::{
        pod::{Object, Property},
        utils::{Choice, ChoiceFlags},
    };

    use super::*;

    fn spanning(kind: ChoiceEnum<i32>) -> Value {
        Value::Choice(ChoiceValue::Int(Choice(ChoiceFlags::empty(), kind)))
    }

    fn advertising(key: u32, value: Value) -> Value {
        Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_Format,
            id: sys::SPA_PARAM_EnumFormat,
            properties: vec![
                Property::new(
                    sys::SPA_FORMAT_mediaType,
                    Value::Id(Id(sys::SPA_MEDIA_TYPE_audio)),
                ),
                Property::new(
                    sys::SPA_FORMAT_mediaSubtype,
                    Value::Id(Id(sys::SPA_MEDIA_SUBTYPE_raw)),
                ),
                Property::new(
                    sys::SPA_FORMAT_AUDIO_format,
                    Value::Id(Id(sys::SPA_AUDIO_FORMAT_S16_LE)),
                ),
                Property::new(key, value),
            ],
        })
    }

    fn routed(direction: u32, properties: Vec<Property>) -> Value {
        let mut held = vec![Property::new(
            sys::SPA_PARAM_ROUTE_direction,
            Value::Id(Id(direction)),
        )];
        held.extend(properties);

        Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_ParamRoute,
            id: sys::SPA_PARAM_Route,
            properties: held,
        })
    }

    fn a_headphone_jack(available: u32) -> Vec<Property> {
        vec![
            Property::new(sys::SPA_PARAM_ROUTE_device, Value::Int(1)),
            Property::new(
                sys::SPA_PARAM_ROUTE_name,
                Value::String("analog-output-headphones".to_owned()),
            ),
            Property::new(
                sys::SPA_PARAM_ROUTE_description,
                Value::String("Headphones".to_owned()),
            ),
            Property::new(sys::SPA_PARAM_ROUTE_available, Value::Id(Id(available))),
        ]
    }

    #[test]
    fn a_route_names_the_seat_it_serves_and_whether_anything_is_plugged_into_it() {
        let plugged = parse_route(&routed(
            Direction::Output.as_raw(),
            a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes),
        ))
        .expect("an output route");

        assert_eq!(plugged.seats, vec![1]);
        assert_eq!(plugged.port.description, "Headphones");
        assert_eq!(plugged.port.plugged, Plugged::Yes);

        for (available, read_as) in [
            (sys::SPA_PARAM_AVAILABILITY_no, Plugged::No),
            (sys::SPA_PARAM_AVAILABILITY_unknown, Plugged::Unsaid),
        ] {
            let route = parse_route(&routed(
                Direction::Output.as_raw(),
                a_headphone_jack(available),
            ))
            .expect("an output route");
            assert_eq!(route.port.plugged, read_as);
        }
    }

    fn told(pairs: &[(&str, &str)]) -> Property {
        let mut items = vec![Value::Int(pairs.len() as i32)];
        for (key, value) in pairs {
            items.push(Value::String((*key).to_owned()));
            items.push(Value::String((*value).to_owned()));
        }

        Property::new(sys::SPA_PARAM_ROUTE_info, Value::Struct(items))
    }

    #[test]
    fn a_route_says_whether_the_volume_on_its_port_is_the_hardwares_own() {
        for (said, read_as) in [
            ("true", HardwareVolume::Yes),
            ("false", HardwareVolume::No),
            ("neither", HardwareVolume::Unsaid),
        ] {
            let mut properties = a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes);
            properties.push(told(&[
                ("port.type", "headphones"),
                ("route.hw-mute", "true"),
                ("route.hw-volume", said),
            ]));

            let route = parse_route(&routed(Direction::Output.as_raw(), properties))
                .expect("an output route");
            assert_eq!(route.port.hardware_volume, read_as, "reading {said}");
        }
    }

    #[test]
    fn a_route_that_says_nothing_about_its_volume_is_unsaid_rather_than_software() {
        let bare = parse_route(&routed(
            Direction::Output.as_raw(),
            a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes),
        ))
        .expect("an output route");
        assert_eq!(bare.port.hardware_volume, HardwareVolume::Unsaid);

        let mut properties = a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes);
        properties.push(told(&[("port.type", "headphones")]));
        let quiet =
            parse_route(&routed(Direction::Output.as_raw(), properties)).expect("an output route");
        assert_eq!(quiet.port.hardware_volume, HardwareVolume::Unsaid);
    }

    #[test]
    fn a_profile_is_drawn_by_what_it_calls_itself_and_named_where_it_describes_nothing() {
        let described = Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_ParamProfile,
            id: sys::SPA_PARAM_Profile,
            properties: vec![
                Property::new(sys::SPA_PARAM_PROFILE_index, Value::Int(1)),
                Property::new(
                    sys::SPA_PARAM_PROFILE_name,
                    Value::String("output:analog-stereo".to_owned()),
                ),
                Property::new(
                    sys::SPA_PARAM_PROFILE_description,
                    Value::String("Analog Stereo Output".to_owned()),
                ),
            ],
        });
        assert_eq!(
            parse_profile(&described).as_deref(),
            Some("Analog Stereo Output")
        );

        let bare = Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_ParamProfile,
            id: sys::SPA_PARAM_Profile,
            properties: vec![Property::new(
                sys::SPA_PARAM_PROFILE_name,
                Value::String("off".to_owned()),
            )],
        });
        assert_eq!(parse_profile(&bare).as_deref(), Some("off"));
    }

    #[test]
    fn a_route_is_not_read_as_a_profile() {
        assert_eq!(
            parse_profile(&routed(
                Direction::Output.as_raw(),
                a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes),
            )),
            None
        );
    }

    #[test]
    fn a_route_that_is_not_an_output_or_names_no_seat_is_not_a_port_a_sink_comes_out_of() {
        assert_eq!(
            parse_route(&routed(
                Direction::Input.as_raw(),
                a_headphone_jack(sys::SPA_PARAM_AVAILABILITY_yes)
            )),
            None,
            "a microphone was read as a port a sink comes out of"
        );
        assert_eq!(
            parse_route(&routed(Direction::Output.as_raw(), Vec::new())),
            None
        );
    }

    #[test]
    fn a_route_the_card_offers_names_every_seat_it_would_serve() {
        let offered = parse_route(&routed(
            Direction::Output.as_raw(),
            vec![
                Property::new(
                    sys::SPA_PARAM_ROUTE_devices,
                    Value::ValueArray(ValueArray::Int(vec![7, 8, 9])),
                ),
                Property::new(
                    sys::SPA_PARAM_ROUTE_description,
                    Value::String("HDMI / DisplayPort 2".to_owned()),
                ),
                Property::new(
                    sys::SPA_PARAM_ROUTE_available,
                    Value::Id(Id(sys::SPA_PARAM_AVAILABILITY_no)),
                ),
            ],
        ))
        .expect("an output route");

        assert_eq!(offered.seats, vec![7, 8, 9]);
        assert_eq!(offered.port.plugged, Plugged::No);
    }

    #[test]
    fn a_route_that_describes_itself_with_nothing_but_a_name_is_drawn_by_that_name() {
        let named = parse_route(&routed(
            Direction::Output.as_raw(),
            vec![
                Property::new(sys::SPA_PARAM_ROUTE_device, Value::Int(4)),
                Property::new(
                    sys::SPA_PARAM_ROUTE_name,
                    Value::String("analog-output".to_owned()),
                ),
            ],
        ))
        .expect("an output route");

        assert_eq!(named.seats, vec![4]);
        assert_eq!(named.port.description, "analog-output");
        assert_eq!(named.port.plugged, Plugged::Unsaid);
    }

    fn rates_of(value: Value) -> Vec<SampleRate> {
        parse_enum_format(&advertising(sys::SPA_FORMAT_AUDIO_rate, value))
            .expect("an audio format")
            .rates
    }

    fn layouts_of(value: Value) -> Vec<ChannelLayout> {
        parse_enum_format(&advertising(sys::SPA_FORMAT_AUDIO_channels, value))
            .expect("an audio format")
            .layouts
    }

    #[test]
    fn a_rate_range_names_every_standard_rate_inside_it_rather_than_three() {
        let rates = rates_of(spanning(ChoiceEnum::Range {
            default: 48_000,
            min: 8_000,
            max: 192_000,
        }));

        assert_eq!(
            rates.iter().map(|rate| rate.hz()).collect::<Vec<_>>(),
            vec![
                8_000, 11_025, 16_000, 22_050, 32_000, 44_100, 48_000, 64_000, 88_200, 96_000,
                176_400, 192_000
            ]
        );
        assert!(
            rates.contains(&SampleRate::HZ_96000),
            "a rate the hardware would have taken natively was discarded"
        );
        assert!(
            !rates.contains(&SampleRate::HZ_352800),
            "a rate past the span was named"
        );
    }

    #[test]
    fn a_rate_range_keeps_the_rate_it_defaults_to_even_where_the_span_holds_nothing() {
        let rates = rates_of(spanning(ChoiceEnum::Range {
            default: 48_000,
            min: 1,
            max: -1,
        }));

        assert_eq!(rates, vec![SampleRate::HZ_48000]);
    }

    #[test]
    fn a_stepped_rate_names_only_the_values_the_step_lands_on() {
        let rates = rates_of(spanning(ChoiceEnum::Step {
            default: 48_000,
            min: 48_000,
            max: 192_000,
            step: 48_000,
        }));

        assert_eq!(
            rates,
            vec![
                SampleRate::HZ_48000,
                SampleRate::HZ_96000,
                SampleRate::HZ_192000
            ],
            "a step that skips 44.1 kHz still offered it"
        );
    }

    #[test]
    fn an_enumerated_rate_list_is_taken_exactly_as_the_device_gave_it() {
        let rates = rates_of(spanning(ChoiceEnum::Enum {
            default: 48_000,
            alternatives: vec![44_100, 96_000],
        }));

        assert_eq!(
            rates,
            vec![
                SampleRate::HZ_48000,
                SampleRate::HZ_44100,
                SampleRate::HZ_96000
            ]
        );
    }

    #[test]
    fn a_single_rate_is_the_whole_of_what_that_device_offers() {
        assert_eq!(rates_of(Value::Int(48_000)), vec![SampleRate::HZ_48000]);
        assert_eq!(
            rates_of(spanning(ChoiceEnum::None(44_100))),
            vec![SampleRate::HZ_44100]
        );
    }

    #[test]
    fn a_channel_range_names_every_count_inside_it_rather_than_its_ends() {
        let layouts = layouts_of(spanning(ChoiceEnum::Range {
            default: 2,
            min: 1,
            max: 8,
        }));

        assert_eq!(
            layouts,
            vec![
                ChannelLayout::Mono,
                ChannelLayout::Stereo,
                ChannelLayout::from_count(ChannelCount::new(3).expect("three channels")),
                ChannelLayout::Quad,
                ChannelLayout::from_count(ChannelCount::new(5).expect("five channels")),
                ChannelLayout::Surround51,
                ChannelLayout::from_count(ChannelCount::new(7).expect("seven channels")),
                ChannelLayout::Surround71,
            ]
        );
    }

    #[test]
    fn a_channel_range_is_capped_at_the_widest_count_the_domain_carries() {
        let layouts = layouts_of(spanning(ChoiceEnum::Range {
            default: 2,
            min: 1,
            max: i32::MAX,
        }));

        assert_eq!(
            layouts.len(),
            usize::from(ChannelCount::MAX.get()),
            "a range past the domain named a layout it cannot hold"
        );
        assert_eq!(layouts.first(), Some(&ChannelLayout::Mono));
    }

    #[test]
    fn allowed_rates_parse_the_daemons_json_array() {
        let rates = parse_allowed_rates("[ 44100, 48000, 96000 ]");
        assert_eq!(
            rates,
            vec![
                SampleRate::HZ_44100,
                SampleRate::HZ_48000,
                SampleRate::HZ_96000
            ]
        );
    }

    #[test]
    fn a_single_allowed_rate_parses() {
        assert_eq!(parse_allowed_rates("[ 48000 ]"), vec![SampleRate::HZ_48000]);
    }

    #[test]
    fn an_empty_allowed_rates_list_yields_nothing() {
        assert!(parse_allowed_rates("[ ]").is_empty());
        assert!(parse_allowed_rates("").is_empty());
    }

    #[test]
    fn rates_the_domain_rejects_are_dropped_rather_than_panicking() {
        assert_eq!(
            parse_allowed_rates("[ 0, 48000, 999999999 ]"),
            vec![SampleRate::HZ_48000]
        );
    }

    #[test]
    fn the_graph_rate_parses_as_a_bare_integer() {
        assert_eq!(parse_rate("48000"), Some(SampleRate::HZ_48000));
        assert_eq!(parse_rate(" 44100 "), Some(SampleRate::HZ_44100));
    }

    #[test]
    fn an_unforced_graph_rate_of_zero_is_not_a_rate() {
        assert_eq!(parse_rate("0"), None);
        assert_eq!(parse_rate(""), None);
        assert_eq!(parse_rate("1/48000"), None);
    }

    #[test]
    fn the_default_sink_name_is_lifted_out_of_its_json() {
        assert_eq!(
            parse_default_sink(r#"{"name":"alsa_output.pci-0000_00_1f.3.analog-stereo"}"#),
            Some("alsa_output.pci-0000_00_1f.3.analog-stereo".to_owned())
        );
        assert_eq!(
            parse_default_sink(r#"{ "name" : "auto_null" }"#),
            Some("auto_null".to_owned())
        );
    }

    #[test]
    fn a_default_sink_payload_without_a_name_is_not_a_sink() {
        assert_eq!(parse_default_sink("{}"), None);
        assert_eq!(parse_default_sink(""), None);
    }

    #[test]
    fn spa_sample_formats_map_only_to_the_four_we_carry() {
        assert_eq!(
            sample_format(sys::SPA_AUDIO_FORMAT_S16_LE),
            Some(SampleFormat::S16)
        );
        assert_eq!(
            sample_format(sys::SPA_AUDIO_FORMAT_S24_32_LE),
            Some(SampleFormat::S24)
        );
        assert_eq!(
            sample_format(sys::SPA_AUDIO_FORMAT_S32_LE),
            Some(SampleFormat::S32)
        );
        assert_eq!(
            sample_format(sys::SPA_AUDIO_FORMAT_F32_LE),
            Some(SampleFormat::F32)
        );
        assert_eq!(
            sample_format(sys::SPA_AUDIO_FORMAT_S24_LE),
            Some(SampleFormat::S24)
        );
        assert_eq!(sample_format(sys::SPA_AUDIO_FORMAT_UNKNOWN), None);
    }

    #[test]
    fn every_format_round_trips_through_its_spa_identifier() {
        for format in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            for word in [WireWord::Padded, WireWord::Packed] {
                assert_eq!(sample_format(spa_format(format, word).0), Some(format));
            }
        }
    }

    #[test]
    fn only_the_twenty_four_bit_word_is_narrower_on_the_wire_than_it_is_in_memory() {
        assert!(packs_narrower(SampleFormat::S24));
        for format in [SampleFormat::S16, SampleFormat::S32, SampleFormat::F32] {
            assert!(!packs_narrower(format));
            assert_eq!(
                spa_format(format, WireWord::Packed),
                spa_format(format, WireWord::Padded),
                "{format} named a packed word it does not have"
            );
        }
    }

    #[test]
    fn the_two_twenty_four_bit_words_are_told_apart_by_the_one_that_is_packed() {
        assert_eq!(
            WireWord::of(spa_format(SampleFormat::S24, WireWord::Packed)),
            WireWord::Packed
        );
        assert_eq!(
            WireWord::of(spa_format(SampleFormat::S24, WireWord::Padded)),
            WireWord::Padded
        );
        assert!(!WireWord::of(AudioFormat(sys::SPA_AUDIO_FORMAT_S32_LE)).is_packed());
    }

    #[test]
    fn a_device_advertising_both_twenty_four_bit_words_offers_the_depth_once() {
        let advertised = one_of_each(
            [
                sys::SPA_AUDIO_FORMAT_S24_LE,
                sys::SPA_AUDIO_FORMAT_S24_32_LE,
                sys::SPA_AUDIO_FORMAT_S16_LE,
                sys::SPA_AUDIO_FORMAT_UNKNOWN,
            ]
            .into_iter(),
        );

        assert_eq!(advertised, vec![SampleFormat::S24, SampleFormat::S16]);
    }

    #[test]
    fn named_layouts_declare_a_channel_position_each() {
        for layout in [
            ChannelLayout::Mono,
            ChannelLayout::Stereo,
            ChannelLayout::Quad,
            ChannelLayout::Surround51,
            ChannelLayout::Surround71,
        ] {
            let positions = spa_position(layout);
            let named = positions
                .iter()
                .take(layout.count().get() as usize)
                .filter(|channel| **channel != 0)
                .count();
            assert_eq!(
                named,
                layout.count().get() as usize,
                "{layout} is incomplete"
            );
        }
    }
}
