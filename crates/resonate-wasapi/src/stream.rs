use std::{
    num::{NonZeroU64, NonZeroUsize},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_audio::{
    AudioSource, Error, LatencyRequest, NodeName, Opener, Operation, SinkId, SinkResult,
    SinkStream, StreamRequest, sink_id_of,
};
use resonate_win32::PerformanceCounter;
use wasapi::{AudioClient, Device, Direction, StreamMode, WasapiError, WaveFormat};

use crate::{
    device::{backend_error, default_device, device_for, enter_the_apartment, status_of, wave_of},
    formats::wire_of,
    opening::{Initialize, Status, initialized, refused_at_open},
    priority::Priority,
    render::{Client, Cycle, Playing},
    serving::{Feeding, launch},
    surveying::Surveys,
    ways::{Way, endpoint_of, node_of},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const HNS_A_NANOSECOND_DIVISOR: u128 = 100;
const ALIGN_TO_BYTES: u32 = 128;

pub(crate) type Known = Arc<Mutex<Vec<(SinkId, NodeName)>>>;

pub(crate) struct Opens {
    pub(crate) known: Known,
    pub(crate) surveys: Arc<Surveys>,
}

enum Target {
    Default,
    Node(SinkId, NodeName),
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
        let named = || {
            self.known
                .lock()
                .iter()
                .find(|(held, _)| *held == id)
                .map(|(_, node)| Target::Node(id, node.clone()))
        };

        if let Some(target) = named() {
            return Ok(target);
        }

        self.surveys
            .within(OPEN_WITHIN)
            .ok_or(Error::OpenUnanswered)??;
        named().ok_or(Error::SinkGone { node: id })
    }
}

struct Opening<'a> {
    device: &'a Device,
    wave: &'a WaveFormat,
    way: Way,
}

impl Initialize for Opening<'_> {
    type Client = AudioClient;
    type Fault = WasapiError;

    fn activated(&mut self) -> Result<AudioClient, WasapiError> {
        self.device.get_iaudioclient()
    }

    fn initialized(
        &mut self,
        client: &mut AudioClient,
        period_hns: i64,
    ) -> Result<(), WasapiError> {
        let mode = match self.way {
            Way::Shared => StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: period_hns,
            },
            Way::Exclusive => StreamMode::EventsExclusive { period_hns },
        };
        client.initialize_client(self.wave, Direction::Render, mode, None)
    }

    fn buffer_frames(&self, client: &AudioClient) -> Result<u32, WasapiError> {
        client.get_buffer_size()
    }

    fn status_of(fault: &WasapiError) -> Option<Status> {
        status_of(fault)
    }
}

fn refused(node: Option<SinkId>, op: Operation) -> impl Fn(WasapiError) -> Error {
    move |error| match error {
        WasapiError::UnsupportedFormat => Error::FormatRefused,
        error => status_of(&error)
            .and_then(|status| refused_at_open(node, op, status))
            .unwrap_or_else(|| backend_error(op, error)),
    }
}

fn start(target: &Target, request: &StreamRequest, feeding: Feeding) -> SinkResult<Playing> {
    let (device, way, node) = match target {
        Target::Default => {
            let device = default_device().map_err(refused(None, Operation::Open))?;
            let endpoint = device.get_id().map_err(refused(None, Operation::Open))?;
            let node = sink_id_of(&node_of(Way::Shared, &endpoint));
            (device, Way::Shared, node)
        }
        Target::Node(id, node) => {
            let (way, endpoint) = endpoint_of(node).ok_or(Error::NoSink)?;
            let device = device_for(endpoint).map_err(refused(Some(*id), Operation::Open))?;
            (device, way, *id)
        }
    };
    let at_open = refused(Some(node), Operation::Open);
    let spec = request.spec;
    let wave = wave_of(spec.format, spec.rate, spec.channels);

    let mut opening = Opening {
        device: &device,
        wave: &wave,
        way,
    };

    let client = opening.activated().map_err(&at_open)?;

    let period = match way {
        Way::Shared => buffer_hns(request),
        Way::Exclusive => {
            let (default_period, _) = client.get_device_period().map_err(&at_open)?;
            let desired = match request.latency {
                LatencyRequest::Auto => default_period,
                LatencyRequest::Duration(_) => buffer_hns(request),
            };
            client
                .calculate_aligned_period_near(desired, Some(ALIGN_TO_BYTES), &wave)
                .map_err(&at_open)?
        }
    };
    let client = initialized(&mut opening, client, period, spec.rate)
        .map_err(refused(Some(node), Operation::Format))?;

    let event = client.set_get_eventhandle().map_err(&at_open)?;
    let render = client.get_audiorenderclient().map_err(&at_open)?;
    let buffer_frames = client.get_buffer_size().map_err(&at_open)?;
    let frame_bytes =
        wire_of(spec.format).store_bits / 8 * usize::from(spec.channels.count().get());
    let frame_bytes = NonZeroUsize::new(frame_bytes).ok_or(Error::FormatRefused)?;

    let scratch =
        vec![0_u8; frame_bytes.get() * usize::try_from(buffer_frames).unwrap_or_default()];
    let audio_clock = client.get_audioclock().ok().and_then(|audio_clock| {
        let frequency = NonZeroU64::new(audio_clock.get_frequency().ok()?)?;
        Some((audio_clock, frequency))
    });
    let counter = PerformanceCounter::of_this_machine().ok();

    Ok(Playing {
        client: Client {
            client,
            render,
            audio_clock,
            counter,
        },
        cycle: Cycle {
            fed: feeding.fed,
            scratch,
            buffer_frames,
            way,
            format: spec.format,
            rate: spec.rate,
            frame_bytes,
            clock: feeding.clock,
            written: 0,
            real: 0,
        },
        event,
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
