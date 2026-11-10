use std::{io, num::NonZeroU32, sync::Arc, time::Duration};

use crossbeam_channel::Receiver;
use resonate_apple::{Component, Format, OsStatus, Shape, Unit, Word};
use resonate_audio::{
    AudioSource, Backend, Error, HardwareVolume, NodeName, Opener, Operation, Plugged, Result,
    SinkChange, SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamRequest, Surveyor,
    Words,
};
use resonate_core::{ChannelLayout, Gain, SampleFormat, SampleRate};

use super::{
    remote_io_state::Routing,
    remote_io_stream::{Hardware, launch},
};

const STEREO: NonZeroU32 = NonZeroU32::MIN.saturating_add(1);
const UNIT_WITHIN: Duration = Duration::from_secs(3);

pub(crate) struct IosAudio {
    routing: Arc<Routing>,
}

impl IosAudio {
    pub(crate) fn new(routing: Arc<Routing>) -> Self {
        Self { routing }
    }
}

struct Device(Arc<Routing>);

impl Surveyor for Device {
    fn enumerate_sinks(&self, _: Duration) -> Result<Vec<SinkInfo>> {
        let rate = self.0.rate();
        Ok(vec![SinkInfo {
            id: self.0.sink(),
            name: NodeName::new("ios-route"),
            aliases: Vec::new(),
            description: "iOS audio route".into(),
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
            formats: vec![SinkFormats {
                format: SampleFormat::F32,
                words: Words::Whole,
                rates: vec![rate],
                channels: vec![ChannelLayout::Stereo],
            }],
            allowed_rates: vec![rate],
            current_rate: Some(rate),
        }])
    }
}

struct RemoteIo(Unit);

fn stereo_float(rate: SampleRate) -> Format {
    Format::new(f64::from(rate.hz()), STEREO, Word::Float32)
}

fn status_of(answered: std::result::Result<(), OsStatus>) -> i32 {
    answered.map_or_else(OsStatus::get, |()| 0)
}

fn unopened(error: resonate_apple::Error) -> Error {
    let source = match error {
        resonate_apple::Error::NoComponent { .. } => io::ErrorKind::NotFound.into(),
        refused @ resonate_apple::Error::Refused { .. } => io::Error::other(refused),
    };
    Error::Backend {
        op: Operation::Open,
        source,
    }
}

impl Hardware for RemoteIo {
    fn start(&self) -> i32 {
        status_of(self.0.start())
    }

    fn stop(&self) -> i32 {
        status_of(self.0.stop())
    }

    fn reformat(&self, rate: SampleRate) -> i32 {
        status_of(self.0.reformatted(stereo_float(rate)))
    }
}

impl Opener for Device {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Result<SinkStream> {
        launch(&self.0, request, source, UNIT_WITHIN, |rate, rendering| {
            let shape = Shape {
                component: Component::RemoteIo,
                device: None,
                format: stereo_float(rate),
            };
            Unit::playing(shape, rendering)
                .map(RemoteIo)
                .map_err(unopened)
        })
    }
}

impl Backend for IosAudio {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.routing.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Device(Arc::clone(&self.routing)))
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Device(Arc::clone(&self.routing)))
    }

    fn set_device_volume(&self, _: SinkId, _: Gain) -> Result<()> {
        Err(Error::Backend {
            op: Operation::Volume,
            source: io::ErrorKind::Unsupported.into(),
        })
    }

    fn set_device_mute(&self, _: SinkId, _: bool) -> Result<()> {
        Err(Error::Backend {
            op: Operation::Volume,
            source: io::ErrorKind::Unsupported.into(),
        })
    }

    fn shutdown(self: Box<Self>) -> Result<()> {
        Ok(())
    }
}
