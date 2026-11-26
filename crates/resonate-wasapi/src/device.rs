use std::io;

use resonate_audio::{Error, HardwareVolume, Operation, Plugged, SinkPort, SinkResult, gain_of};
use resonate_core::{ChannelCount, ChannelLayout, SampleFormat, SampleRate};
use resonate_win32::EndpointVolume;
use wasapi::{
    AudioClient, Device, DeviceEnumerator, Direction, SampleType, ShareMode, WasapiError,
    WaveFormat,
};

use crate::{
    formats::{Wire, format_of, mask_of, wire_of},
    opening::Status,
    survey::{Endpoint, Endpoints, Mix, Probe},
};

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

pub(crate) fn status_of(error: &WasapiError) -> Option<Status> {
    match error {
        WasapiError::Windows(error) => Some(Status(error.code().0)),
        _ => None,
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

fn mix_of(client: &AudioClient) -> Option<Mix> {
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
    Some(Mix {
        format,
        rate,
        layout: ChannelLayout::from_count(channels),
    })
}

pub(crate) struct Devices;

impl Endpoints for Devices {
    type Client = AudioClient;

    fn listed(&mut self) -> SinkResult<Vec<Endpoint<AudioClient>>> {
        enter_the_apartment();

        let enumerator =
            DeviceEnumerator::new().map_err(|e| backend_error(Operation::Connect, e))?;
        let default = enumerator
            .get_default_device(Direction::Render)
            .and_then(|device| device.get_id())
            .ok();
        let collection = enumerator
            .get_device_collection(Direction::Render)
            .map_err(|e| backend_error(Operation::Connect, e))?;

        let mut listed = Vec::new();

        for device in &collection {
            let Ok(device) = device else {
                continue;
            };
            let Ok(id) = device.get_id() else {
                continue;
            };
            let Ok(client) = device.get_iaudioclient() else {
                continue;
            };

            listed.push(Endpoint {
                is_default: default.as_deref() == Some(id.as_str()),
                name: device.get_friendlyname().unwrap_or_else(|_| id.clone()),
                mix: mix_of(&client),
                port: port_of(&device, &id),
                client,
                id,
            });
        }
        Ok(listed)
    }

    fn exclusive(
        &self,
        client: &AudioClient,
        format: SampleFormat,
        rate: SampleRate,
        layout: ChannelLayout,
    ) -> Probe {
        match client.is_supported(&wave_of(format, rate, layout), ShareMode::Exclusive) {
            Ok(None) => Probe::Taken,
            Ok(Some(_)) => Probe::Refused,
            Err(error) => match status_of(&error) {
                Some(Status::DEVICE_IN_USE) => Probe::InUse,
                Some(Status::EXCLUSIVE_MODE_NOT_ALLOWED) => Probe::Barred,
                _ => Probe::Refused,
            },
        }
    }
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

pub(crate) fn device_for(endpoint: &str) -> Result<Device, WasapiError> {
    DeviceEnumerator::new()?.get_device(endpoint)
}

pub(crate) fn default_device() -> Result<Device, WasapiError> {
    DeviceEnumerator::new()?.get_default_device(Direction::Render)
}
