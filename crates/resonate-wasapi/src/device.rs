use std::{collections::BTreeMap, io, sync::LazyLock};

use parking_lot::Mutex;
use resonate_audio::{
    Error, HardwareVolume, Operation, Plugged, SinkFormats, SinkInfo, SinkPort, SinkResult,
    gain_of, sink_id_of,
};
use resonate_core::{ChannelCount, ChannelLayout, SampleFormat, SampleRate};
use resonate_win32::EndpointVolume;
use wasapi::{
    AudioClient, Device, DeviceEnumerator, Direction, SampleType, ShareMode, WasapiError,
    WaveFormat,
};

use crate::{
    formats::{Wire, format_of, formats_taken, mask_of, rates_of, wire_of},
    ways::{Way, node_of},
};

const DEVICE_IN_USE: i32 = i32::from_ne_bytes(0x8889_000A_u32.to_ne_bytes());

static TAKEN_WHILE_FREE: LazyLock<Mutex<BTreeMap<String, Vec<SinkFormats>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

pub(crate) fn backend_error(op: Operation, source: WasapiError) -> Error {
    Error::Backend {
        op,
        source: io::Error::other(source),
    }
}

pub(crate) fn level_error(source: resonate_win32::Error) -> Error {
    Error::Backend {
        op: Operation::Volume,
        source: io::Error::other(source),
    }
}

pub(crate) fn enter_the_apartment() {
    let _ = wasapi::initialize_mta();
}

pub(crate) fn wave_of(format: SampleFormat, rate: SampleRate, layout: ChannelLayout) -> WaveFormat {
    let wire = wire_of(format);
    let sample = if wire.float {
        SampleType::Float
    } else {
        SampleType::Int
    };
    WaveFormat::new(
        wire.store_bits,
        wire.valid_bits,
        sample,
        rate.hz() as usize,
        usize::from(layout.count().get()),
        Some(mask_of(layout)),
    )
}

fn mix_of(client: &AudioClient) -> Option<(SampleFormat, SampleRate, ChannelLayout)> {
    let mix = client.get_mixformat().ok()?;
    let float = matches!(mix.get_subformat(), Ok(SampleType::Float));
    let wire = Wire {
        store_bits: usize::from(mix.get_bitspersample()),
        valid_bits: usize::from(mix.get_validbitspersample()),
        float,
    };
    let format = format_of(wire)?;
    let rate = SampleRate::new(mix.get_samplespersec()).ok()?;
    let channels = ChannelCount::new(mix.get_nchannels()).ok()?;
    Some((format, rate, ChannelLayout::from_count(channels)))
}

pub(crate) fn enumerate() -> SinkResult<Vec<SinkInfo>> {
    enter_the_apartment();
    let enumerator = DeviceEnumerator::new().map_err(|e| backend_error(Operation::Connect, e))?;
    let default = enumerator
        .get_default_device(Direction::Render)
        .and_then(|device| device.get_id())
        .ok();
    let collection = enumerator
        .get_device_collection(Direction::Render)
        .map_err(|e| backend_error(Operation::Connect, e))?;

    let mut sinks = Vec::new();
    for device in &collection {
        let Ok(device) = device else {
            continue;
        };
        let Ok(endpoint) = device.get_id() else {
            continue;
        };
        let is_default = default.as_deref() == Some(endpoint.as_str());
        let name = device
            .get_friendlyname()
            .unwrap_or_else(|_| endpoint.clone());
        let Ok(client) = device.get_iaudioclient() else {
            continue;
        };
        let port = port_of(&device, &endpoint);
        sinks.extend(shared_sink(
            &endpoint,
            &name,
            is_default,
            &client,
            port.clone(),
        ));
        sinks.extend(exclusive_sink(&endpoint, &name, &client, port));
    }
    Ok(sinks)
}

fn port_of(device: &Device, endpoint: &str) -> Option<SinkPort> {
    let level = EndpointVolume::of(endpoint).ok()?;
    let scalar = level.level().ok()?;
    let hardware_volume = device
        .get_audiometerinformation()
        .and_then(|meter| meter.query_hardware_support())
        .map_or(HardwareVolume::Unsaid, |support| {
            if support.volume {
                HardwareVolume::Yes
            } else {
                HardwareVolume::No
            }
        });
    Some(SinkPort {
        description: device.get_description().unwrap_or_default(),
        plugged: Plugged::Unsaid,
        hardware_volume,
        volume: Some(gain_of(scalar.get())),
        muted: level.is_muted().unwrap_or(false),
    })
}

fn shared_sink(
    endpoint: &str,
    name: &str,
    is_default: bool,
    client: &AudioClient,
    port: Option<SinkPort>,
) -> Option<SinkInfo> {
    let (format, rate, layout) = mix_of(client)?;
    let node = node_of(Way::Shared, endpoint);
    Some(SinkInfo {
        id: sink_id_of(&node),
        name: node,
        aliases: Vec::new(),
        description: format!("{name}{}", Way::Shared.described()),
        is_default,
        is_hardware: true,
        port,
        profile: None,
        formats: vec![resonate_audio::SinkFormats {
            format,
            words: resonate_audio::Words::Whole,
            rates: vec![rate],
            channels: vec![layout],
        }],
        allowed_rates: vec![rate],
        current_rate: Some(rate),
    })
}

fn exclusive_sink(
    endpoint: &str,
    name: &str,
    client: &AudioClient,
    port: Option<SinkPort>,
) -> Option<SinkInfo> {
    let mut held = false;
    let probed = formats_taken(|format, rate, layout| {
        if held {
            return false;
        }
        match client.is_supported(&wave_of(format, rate, layout), ShareMode::Exclusive) {
            Ok(None) => true,
            Err(WasapiError::Windows(error)) if error.code().0 == DEVICE_IN_USE => {
                held = true;
                false
            }
            _ => false,
        }
    });
    let formats = taken_while_free(endpoint, held, probed);
    if formats.is_empty() {
        return None;
    }
    let node = node_of(Way::Exclusive, endpoint);
    Some(SinkInfo {
        id: sink_id_of(&node),
        name: node,
        aliases: Vec::new(),
        description: format!("{name}{}", Way::Exclusive.described()),
        is_default: false,
        is_hardware: true,
        allowed_rates: rates_of(&formats),
        formats,
        port,
        profile: None,
        current_rate: None,
    })
}

fn taken_while_free(endpoint: &str, held: bool, probed: Vec<SinkFormats>) -> Vec<SinkFormats> {
    let mut remembered = TAKEN_WHILE_FREE.lock();
    if held {
        return remembered.get(endpoint).cloned().unwrap_or_default();
    }
    remembered.insert(endpoint.to_owned(), probed.clone());
    probed
}

pub(crate) fn device_for(endpoint: &str) -> SinkResult<Device> {
    let enumerator = DeviceEnumerator::new().map_err(|e| backend_error(Operation::Open, e))?;
    enumerator
        .get_device(endpoint)
        .map_err(|e| backend_error(Operation::Open, e))
}

pub(crate) fn default_device() -> SinkResult<Device> {
    let enumerator = DeviceEnumerator::new().map_err(|e| backend_error(Operation::Open, e))?;
    enumerator
        .get_default_device(Direction::Render)
        .map_err(|e| backend_error(Operation::Open, e))
}
