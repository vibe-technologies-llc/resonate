use std::{
    num::NonZeroU32,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_apple::{
    Component, Device, Direction, Format, Hog, PhysicalFormat, Shape, Stream, Unit, Word,
};
use resonate_audio::{
    AudioSource, Error, NodeName, Opener, Operation, SinkId, SinkResult, SinkStream, StreamRequest,
};
use resonate_core::StreamSpec;

use crate::{
    clock::Latencies,
    device::{backend_error, device_of_uid, enumerate, output_stream_of, physical_of},
    fill::Rendering,
    formats::{ClientWord, physical_for},
    remembered::Remembered,
    serving::{Hardware, Looks, Standing, launch},
    ways::{Way, device_of},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const A_DEVICE_SETTLES_WITHIN: Duration = Duration::from_millis(1_500);
const A_DEVICE_IS_ASKED_AGAIN_AFTER: Duration = Duration::from_millis(10);
const RATES_AGREE_WITHIN_HZ: f64 = 0.5;
const DONE: i32 = 0;

pub(crate) type Known = Arc<Mutex<Vec<(SinkId, NodeName)>>>;

pub(crate) struct Opens {
    pub(crate) known: Known,
    pub(crate) remembered: Arc<Mutex<Remembered>>,
}

enum Target {
    Default,
    Node(NodeName),
}

impl Opens {
    fn target_of(&self, chosen: Option<SinkId>) -> SinkResult<Target> {
        let Some(id) = chosen else {
            return Ok(Target::Default);
        };
        let named = |known: &Known| {
            known
                .lock()
                .iter()
                .find(|(held, _)| *held == id)
                .map(|(_, node)| node.clone())
        };
        if let Some(node) = named(&self.known) {
            return Ok(Target::Node(node));
        }
        let sinks = enumerate(&self.remembered)?;
        *self.known.lock() = sinks
            .iter()
            .map(|sink| (sink.id, sink.name.clone()))
            .collect();
        named(&self.known)
            .map(Target::Node)
            .ok_or(Error::SinkGone { node: id })
    }
}

impl Opener for Opens {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let target = self.target_of(request.target)?;
        let spec = request.spec;
        launch(
            request,
            source,
            OPEN_WITHIN,
            Looks::DEVICES,
            move |rendering| Playing::on(&target, spec, rendering),
        )
    }
}

fn unopened(error: resonate_apple::Error) -> Error {
    backend_error(Operation::Open, error)
}

fn unformatted(error: resonate_apple::Error) -> Error {
    backend_error(Operation::Format, error)
}

fn settled(mut is_there: impl FnMut() -> bool) -> bool {
    let asked = Instant::now();
    loop {
        if is_there() {
            return true;
        }
        if asked.elapsed() >= A_DEVICE_SETTLES_WITHIN {
            return false;
        }
        thread::sleep(A_DEVICE_IS_ASKED_AGAIN_AFTER);
    }
}

fn runs_at(device: Device, hz: f64) -> bool {
    device
        .nominal_rate()
        .is_ok_and(|rate| (rate - hz).abs() < RATES_AGREE_WITHIN_HZ)
}

struct Restoring {
    device: Device,
    stream: Stream,
    rate: Option<f64>,
    format: Option<PhysicalFormat>,
}

impl Drop for Restoring {
    fn drop(&mut self) {
        if let Some(format) = &self.format {
            let _ = self.stream.set_physical_format(format);
        }
        if let Some(rate) = self.rate.filter(|rate| !runs_at(self.device, *rate)) {
            let _ = self.device.set_nominal_rate(rate);
        }
    }
}

struct Hogged {
    _restoring: Restoring,
    _hog: Hog,
}

impl Hogged {
    fn taken(device: Device, spec: StreamSpec) -> SinkResult<Self> {
        let hog = Hog::taken(device).map_err(unopened)?;
        let stream = output_stream_of(device).ok_or(Error::FormatRefused)?;
        let restoring = Restoring {
            device,
            stream,
            rate: device.nominal_rate().ok(),
            format: stream.physical_format().ok(),
        };
        let offered = stream.physical_formats().map_err(unformatted)?;
        let carried: Vec<_> = offered
            .iter()
            .filter_map(|offered| Some((physical_of(offered)?, offered.format)))
            .collect();
        let physicals: Vec<_> = carried.iter().map(|(physical, _)| *physical).collect();
        let chosen = physical_for(spec, &physicals).ok_or(Error::FormatRefused)?;
        let (_, format) = carried
            .iter()
            .find(|(physical, _)| *physical == chosen)
            .ok_or(Error::FormatRefused)?;
        let hz = f64::from(spec.rate.hz());
        let format = format.at(hz);

        if !runs_at(device, hz) {
            device.set_nominal_rate(hz).map_err(unformatted)?;
        }
        if !settled(|| runs_at(device, hz)) {
            return Err(Error::FormatRefused);
        }
        stream.set_physical_format(&format).map_err(unformatted)?;
        let took = settled(|| {
            stream.physical_format().is_ok_and(|now| {
                now.depth() == format.depth() && (now.rate() - hz).abs() < RATES_AGREE_WITHIN_HZ
            })
        });
        if !took {
            return Err(Error::FormatRefused);
        }
        Ok(Self {
            _restoring: restoring,
            _hog: hog,
        })
    }
}

struct Playing {
    unit: Unit,
    _hogged: Option<Hogged>,
    device: Device,
}

const fn word_of(client: ClientWord) -> Word {
    match client {
        ClientWord::Float32 => Word::Float32,
        ClientWord::Signed16 => Word::Signed16,
        ClientWord::Signed32 => Word::Signed32,
    }
}

impl Playing {
    fn on(target: &Target, spec: StreamSpec, rendering: Box<Rendering>) -> SinkResult<Self> {
        let (device, way) = match target {
            Target::Default => {
                let device = Device::default_output().map_err(unopened)?;
                (device.ok_or(Error::NoSink)?, Way::Shared)
            }
            Target::Node(node) => {
                let (way, uid) = device_of(node).ok_or(Error::NoSink)?;
                (device_of_uid(uid).ok_or(Error::NoSink)?, way)
            }
        };
        let client = ClientWord::of(spec.format).ok_or(Error::FormatRefused)?;
        let channels =
            NonZeroU32::new(u32::from(spec.channel_count().get())).ok_or(Error::FormatRefused)?;
        let hz = f64::from(spec.rate.hz());
        let hogged = match way {
            Way::Shared => None,
            Way::Hog => Some(Hogged::taken(device, spec)?),
        };
        let shape = Shape {
            component: Component::HalOutput,
            device: Some(device.id()),
            format: Format::new(hz, channels, word_of(client)),
        };
        let unit = Unit::playing(shape, rendering).map_err(unopened)?;
        let playing = Self {
            unit,
            _hogged: hogged,
            device,
        };

        let converted = way == Way::Hog
            && !playing
                .unit
                .device_rate()
                .is_ok_and(|rate| (rate - hz).abs() < RATES_AGREE_WITHIN_HZ);
        if converted {
            return Err(Error::FormatRefused);
        }
        Ok(playing)
    }

    fn latencies(&self) -> Latencies {
        let output = Direction::Output;
        Latencies {
            device: self.device.latency(output).unwrap_or(0),
            safety_offset: self.device.safety_offset(output).unwrap_or(0),
            stream: output_stream_of(self.device)
                .and_then(|stream| stream.latency().ok())
                .unwrap_or(0),
            buffer: self.device.buffer_frames().unwrap_or(0),
        }
    }
}

impl Hardware for Playing {
    fn start(&self) -> i32 {
        self.unit
            .start()
            .map_or_else(|status| status.get(), |()| DONE)
    }

    fn stop(&self) -> i32 {
        self.unit
            .stop()
            .map_or_else(|status| status.get(), |()| DONE)
    }

    fn standing(&self) -> Standing {
        if !self.device.is_alive() {
            return Standing::Gone;
        }
        match self.device.nominal_rate() {
            Ok(rate) if rate.is_finite() && rate >= 1.0 => Standing::Here {
                device_hz: rate.round() as u32,
                latencies: self.latencies(),
            },
            _ => Standing::Gone,
        }
    }
}
