use std::io;

use parking_lot::Mutex;
use resonate_apple::{
    Depth as Carried, Device, Direction, Mute, Offered, Stream, Transport, Volume,
};
use resonate_audio::{Error, Operation, Plugged, SinkInfo, SinkResult, gain_of};

use crate::{
    formats::{Depth, Physical, rate_of},
    remembered::Remembered,
    survey::{Level, Seen, sinks_of},
};

const CHANNELS_OF_A_DEVICE_THAT_DOES_NOT_SAY: u32 = 2;

pub(crate) fn backend_error(op: Operation, source: resonate_apple::Error) -> Error {
    Error::Backend {
        op,
        source: io::Error::other(source),
    }
}

const fn depth_of(carried: Carried) -> Depth {
    match carried {
        Carried::Sixteen => Depth::Sixteen,
        Carried::TwentyFour => Depth::TwentyFour,
        Carried::ThirtyTwo => Depth::ThirtyTwo,
        Carried::Float => Depth::Float,
    }
}

pub(crate) fn physical_of(offered: &Offered) -> Option<Physical> {
    let depth = offered.format.depth()?;
    Some(Physical {
        depth: depth_of(depth),
        channels: offered.format.channels(),
        lowest_hz: offered.rates.lowest,
        highest_hz: offered.rates.highest,
    })
}

pub(crate) fn output_stream_of(device: Device) -> Option<Stream> {
    device.streams(Direction::Output).ok()?.into_iter().next()
}

pub(crate) fn device_of_uid(uid: &str) -> Option<Device> {
    Device::all()
        .ok()?
        .into_iter()
        .find(|device| device.uid().is_ok_and(|held| held == uid))
}

fn seen(device: Device, remembered: &Mutex<Remembered>) -> Option<Seen> {
    let stream = output_stream_of(device)?;
    let uid = device.uid().ok()?;
    let name = device.name().unwrap_or_else(|_| uid.clone());
    let transport = device.transport();
    let is_hardware = !matches!(transport, Transport::Virtual | Transport::Aggregate);
    let read = stream
        .physical_formats()
        .ok()
        .map(|offered| offered.iter().filter_map(physical_of).collect());
    let level = Volume::of(device).and_then(|volume| {
        let scalar = volume.level().ok()?;
        Some(Level {
            gain: gain_of(scalar.get()),
            is_the_devices_own: is_hardware && volume.is_settable(),
        })
    });
    Some(Seen {
        offered: remembered.lock().read_or_recalled(&uid, read),
        uid,
        name,
        is_hardware,
        can_be_hogged: transport != Transport::Aggregate,
        nominal: device.nominal_rate().ok().and_then(rate_of),
        channels: stream
            .physical_format()
            .map_or(CHANNELS_OF_A_DEVICE_THAT_DOES_NOT_SAY, |format| {
                format.channels()
            }),
        plugged: match device.jack_connected(Direction::Output) {
            Some(true) => Plugged::Yes,
            Some(false) => Plugged::No,
            None => Plugged::Unsaid,
        },
        level,
        muted: Mute::of(device)
            .and_then(|mute| mute.is_muted().ok())
            .unwrap_or(false),
    })
}

pub(crate) fn enumerate(remembered: &Mutex<Remembered>) -> SinkResult<Vec<SinkInfo>> {
    let devices = Device::all().map_err(|error| backend_error(Operation::Connect, error))?;
    let seen: Vec<Seen> = devices
        .into_iter()
        .filter_map(|device| seen(device, remembered))
        .collect();
    let present: Vec<&str> = seen.iter().map(|device| device.uid.as_str()).collect();
    remembered.lock().keep_only(&present);
    let default = Device::default_output()
        .ok()
        .flatten()
        .and_then(|device| device.uid().ok());
    Ok(sinks_of(&seen, default.as_deref()))
}
