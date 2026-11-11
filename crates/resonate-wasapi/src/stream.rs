use std::{num::NonZeroUsize, sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, LatencyRequest, NodeName, Opener, Operation, SinkId, SinkResult,
    SinkStream, StreamRequest,
};
use resonate_win32::PerformanceCounter;
use wasapi::{Direction, ShareMode, StreamMode, WasapiError};

use crate::{
    device::{backend_error, default_device, device_for, enter_the_apartment, enumerate, wave_of},
    formats::wire_of,
    priority::Priority,
    render::Playing,
    serving::{Feeding, launch},
    ways::{Way, endpoint_of},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const HNS_A_NANOSECOND_DIVISOR: u128 = 100;
const ALIGN_TO_BYTES: u32 = 128;

pub(crate) type Known = Arc<Mutex<Vec<(SinkId, NodeName)>>>;

pub(crate) struct Opens {
    pub(crate) known: Known,
}

enum Target {
    Default,
    Node(NodeName),
}

impl Opener for Opens {
    fn open(
        &self,
        request: &StreamRequest,
        source: Box<dyn AudioSource>,
    ) -> SinkResult<SinkStream> {
        let target = self.target_of(request.target)?;
        let asked = request.clone();

        launch(request, source, OPEN_WITHIN, move |feeding| {
            enter_the_apartment();
            start(&target, &asked, feeding)
        })
    }
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
        let sinks = enumerate()?;
        *self.known.lock() = sinks
            .iter()
            .map(|sink| (sink.id, sink.name.clone()))
            .collect();
        named(&self.known)
            .map(Target::Node)
            .ok_or(Error::SinkGone { node: id })
    }
}

fn start(target: &Target, request: &StreamRequest, feeding: Feeding) -> SinkResult<Playing> {
    let (device, way) = match target {
        Target::Default => (default_device()?, Way::Shared),
        Target::Node(node) => {
            let (way, endpoint) = endpoint_of(node).ok_or(Error::NoSink)?;
            (device_for(endpoint)?, way)
        }
    };
    let spec = request.spec;
    let wave = wave_of(spec.format, spec.rate, spec.channels);
    let mut client = device
        .get_iaudioclient()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let (mode, share) = match way {
        Way::Shared => (
            StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: buffer_hns(request),
            },
            ShareMode::Shared,
        ),
        Way::Exclusive => {
            let (default_period, _) = client
                .get_device_period()
                .map_err(|error| backend_error(Operation::Open, error))?;
            let desired = match request.latency {
                LatencyRequest::Auto => default_period,
                LatencyRequest::Duration(_) => buffer_hns(request),
            };
            let period = client
                .calculate_aligned_period_near(desired, Some(ALIGN_TO_BYTES), &wave)
                .map_err(|error| backend_error(Operation::Open, error))?;
            (
                StreamMode::EventsExclusive { period_hns: period },
                ShareMode::Exclusive,
            )
        }
    };
    client
        .initialize_client(&wave, Direction::Render, mode, None)
        .map_err(initialize_error)?;
    let event = client
        .set_get_eventhandle()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let render = client
        .get_audiorenderclient()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let buffer_frames = client
        .get_buffer_size()
        .map_err(|error| backend_error(Operation::Open, error))?;
    let frame_bytes =
        wire_of(spec.format).store_bits / 8 * usize::from(spec.channels.count().get());
    let frame_bytes = NonZeroUsize::new(frame_bytes).ok_or(Error::FormatRefused)?;
    let scratch =
        vec![0_u8; frame_bytes.get() * usize::try_from(buffer_frames).unwrap_or_default()];
    let audio_clock = client.get_audioclock().ok().and_then(|audio_clock| {
        let frequency = std::num::NonZeroU64::new(audio_clock.get_frequency().ok()?)?;
        Some((audio_clock, frequency))
    });
    let counter = PerformanceCounter::of_this_machine().ok();

    Ok(Playing {
        client,
        render,
        event,
        fed: feeding.fed,
        scratch,
        buffer_frames,
        way: share,
        format: spec.format,
        rate: spec.rate,
        frame_bytes,
        clock: feeding.clock,
        audio_clock,
        counter,
        written: 0,
        real: 0,
        priority: Priority::ordinary(),
    })
}

fn buffer_hns(request: &StreamRequest) -> i64 {
    match request.latency {
        LatencyRequest::Auto => 0,
        LatencyRequest::Duration(wanted) => {
            i64::try_from(wanted.as_nanos() / HNS_A_NANOSECOND_DIVISOR).unwrap_or(0)
        }
    }
}

fn initialize_error(error: WasapiError) -> Error {
    match error {
        WasapiError::UnsupportedFormat => Error::FormatRefused,
        other => backend_error(Operation::Format, other),
    }
}
