use std::{
    io,
    marker::PhantomData,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use oboe::{
    AudioFormat, AudioOutputCallback, AudioOutputStream, AudioStream, AudioStreamBase,
    AudioStreamBuilder, ChannelCount, ContentType, IsChannelCount, IsFormat, IsFrameType, Mono,
    PerformanceMode, SampleRateConversionQuality, SharingMode, Stereo, Usage,
};
use resonate_audio::{
    AudioSource, Backend, Error, GraphTime, HardwareVolume, NodeName, Opener, Operation, Plugged,
    Result, SinkChange, SinkId, SinkInfo, SinkPort, SinkStream, StreamClock, StreamCommand,
    StreamEvent, StreamRequest, StreamState, Surveyor, Words,
};
use resonate_core::{ChannelLayout, Frames, Gain, Ratio, SampleFormat, SampleRate, StreamSpec};

pub(crate) use super::oboe_state::Routing;
use super::{
    formats::{OutputFormat, OutputStream},
    handover::{Fed, fed_from},
    oboe_frames::{Frame, RENDER_BYTES},
    oboe_offers::{MIXED, Way, sink_formats},
    oboe_render::Rendering,
    oboe_reshape::{Opened, Order, controlled_by, reshaper},
    waits::{OPEN_WITHIN, between_commands, opened_within},
};

const SHORTEST_DRAIN: Duration = Duration::from_millis(20);
const MILLISECONDS_A_SECOND: f64 = 1000.0;

pub(crate) struct AndroidAudio {
    routing: Arc<Routing>,
}

impl AndroidAudio {
    pub(crate) fn new(routing: Arc<Routing>) -> Self {
        Self { routing }
    }
}

struct Device(Arc<Routing>);

impl Surveyor for Device {
    fn enumerate_sinks(&self, _: Duration) -> Result<Vec<SinkInfo>> {
        let (formats, allowed_rates) = sink_formats(&self.0.offered(exclusive_on));
        Ok(vec![SinkInfo {
            id: self.0.sink(),
            name: NodeName::new("android-default"),
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
        }])
    }
}

fn exclusive_on(device: i32) -> Option<StreamSpec> {
    let mut stream = AudioStreamBuilder::default()
        .set_stereo()
        .set_device_id(device)
        .set_performance_mode(PerformanceMode::LowLatency)
        .set_sharing_mode(SharingMode::Exclusive)
        .set_usage(Usage::Media)
        .set_content_type(ContentType::Music)
        .set_format_conversion_allowed(false)
        .set_channel_conversion_allowed(false)
        .set_sample_rate_conversion_quality(SampleRateConversionQuality::None)
        .open_stream()
        .ok()?;
    let native = exclusive_spec_of(&stream);
    let _ = stream.close();
    native
}

fn exclusive_spec_of(stream: &impl AudioStreamBase) -> Option<StreamSpec> {
    if stream.get_sharing_mode() != SharingMode::Exclusive {
        return None;
    }
    let rate = SampleRate::new(u32::try_from(stream.get_sample_rate()).ok()?).ok()?;
    let channels = match stream.get_channel_count() {
        ChannelCount::Mono => ChannelLayout::Mono,
        ChannelCount::Stereo => ChannelLayout::Stereo,
        ChannelCount::Unspecified => return None,
    };
    let format = match stream.get_format() {
        AudioFormat::I16 => SampleFormat::S16,
        AudioFormat::I32 => SampleFormat::S32,
        AudioFormat::F32 => SampleFormat::F32,
        AudioFormat::I24 | AudioFormat::Invalid | AudioFormat::Unspecified => return None,
    };
    Some(StreamSpec::new(rate, channels, format))
}

struct Opening {
    spec: StreamSpec,
    way: Way,
    routing: Arc<Routing>,
    fed: Fed,
    ready: Sender<Result<Opened>>,
    commands: Receiver<Order>,
    announced: Sender<StreamEvent>,
    clock: Arc<StreamClock>,
}

struct Asked {
    device: i32,
    sharing: SharingMode,
    performance: PerformanceMode,
    converts: bool,
}

impl Asked {
    fn of(way: Way, requested: i32) -> Self {
        match way {
            Way::Exclusive => Self {
                device: requested,
                sharing: SharingMode::Exclusive,
                performance: PerformanceMode::LowLatency,
                converts: false,
            },
            Way::BitPerfect { device } => Self {
                device,
                sharing: SharingMode::Shared,
                performance: PerformanceMode::PowerSaving,
                converts: false,
            },
            Way::Mixed => Self {
                device: requested,
                sharing: SharingMode::Shared,
                performance: PerformanceMode::PowerSaving,
                converts: true,
            },
        }
    }

    fn builder<T: IsFormat, C: IsChannelCount>(
        &self,
        spec: StreamSpec,
    ) -> AudioStreamBuilder<oboe::Output, C, T> {
        let quality = if self.converts {
            SampleRateConversionQuality::Best
        } else {
            SampleRateConversionQuality::None
        };
        AudioStreamBuilder::default()
            .set_format::<T>()
            .set_channel_count::<C>()
            .set_sample_rate(spec.rate.hz() as i32)
            .set_device_id(self.device)
            .set_performance_mode(self.performance)
            .set_sharing_mode(self.sharing)
            .set_usage(Usage::Media)
            .set_content_type(ContentType::Music)
            .set_format_conversion_allowed(self.converts)
            .set_channel_conversion_allowed(self.converts)
            .set_sample_rate_conversion_quality(quality)
    }
}

fn opens_as_asked<T: IsFormat, C: IsChannelCount>(asked: &Asked, spec: StreamSpec) -> bool {
    let Ok(mut probe) = asked.builder::<T, C>(spec).open_stream() else {
        return false;
    };
    let exact = exclusive_spec_of(&probe) == Some(spec);
    let _ = probe.close();
    exact
}

fn play<T, C>(opening: Opening)
where
    T: IsFormat + Send + 'static,
    C: IsChannelCount + Send + 'static,
    (T, C): IsFrameType,
    <(T, C) as IsFrameType>::Type: Frame,
    Rendering<(T, C)>: AudioOutputCallback<FrameType = (T, C)>,
{
    let Opening {
        spec,
        mut way,
        routing,
        fed,
        ready,
        commands,
        announced,
        clock,
    } = opening;
    let requested = routing.requested();
    if way == Way::Exclusive && !opens_as_asked::<T, C>(&Asked::of(way, requested), spec) {
        routing.exclusive_refused();
        way = Way::Mixed;
    }
    if let Way::BitPerfect { device } = way
        && !routing.prefer_bit_perfect(device, spec)
    {
        way = Way::Mixed;
    }
    if !matches!(way, Way::BitPerfect { .. }) {
        routing.mix_freely();
    }

    let empty = Arc::new(AtomicBool::new(false));
    let failed = Arc::new(AtomicBool::new(false));
    let rendering = Rendering::<(T, C)> {
        fed,
        empty: Arc::clone(&empty),
        failed: Arc::clone(&failed),
        bytes: vec![0; RENDER_BYTES].into_boxed_slice(),
        clock: Arc::clone(&clock),
        rate: spec.rate,
        shape: PhantomData,
    };
    let opened = Asked::of(way, requested)
        .builder::<T, C>(spec)
        .set_callback(rendering)
        .open_stream();
    let mut stream = match opened {
        Ok(stream)
            if stream.get_sample_rate() == spec.rate.hz() as i32
                && stream.get_format() == T::FORMAT =>
        {
            stream
        }
        Ok(mut stream) => {
            let _ = stream.close();
            let _ = ready.send(Err(Error::FormatRefused));
            return;
        }
        Err(source) => {
            let _ = ready.send(Err(backend_error(Operation::Open, source)));
            return;
        }
    };
    let path = if way == Way::Exclusive && stream.get_sharing_mode() != SharingMode::Exclusive {
        Way::Mixed.path()
    } else {
        way.path()
    };
    routing.opened_on(stream.get_device_id());
    let told = OutputFormat::of(spec).map(|format| routing.opened(OutputStream { format, path }));
    let _ = announced.send(StreamEvent::FormatChanged {
        spec,
        words: Words::Whole,
    });
    let _ = ready.send(Ok(Opened {
        spec,
        way,
        requested,
    }));

    serve(
        &mut stream,
        spec.rate,
        &commands,
        &announced,
        &clock,
        &empty,
        &failed,
    );
    let _ = stream.close();
    if let Some(told) = told {
        routing.closed(told);
    }
}

fn serve(
    stream: &mut impl AudioOutputStream,
    rate: SampleRate,
    commands: &Receiver<Order>,
    announced: &Sender<StreamEvent>,
    clock: &StreamClock,
    empty: &AtomicBool,
    failed: &AtomicBool,
) {
    let tick = Ratio {
        numer: NonZeroU32::MIN,
        denom: NonZeroU32::new(rate.hz()).unwrap_or(NonZeroU32::MIN),
    };
    let mut state = StreamState::Paused;
    let mut draining = false;
    let mut drain_after = None;
    loop {
        if failed.load(Ordering::Acquire) {
            let _ = announced.send(StreamEvent::StateChanged {
                from: state,
                to: StreamState::Failed,
            });
            return;
        }
        if let Ok(latency) = stream.calculate_latency_millis() {
            let frames = (latency.max(0.0) * f64::from(rate.hz()) / MILLISECONDS_A_SECOND) as u64;
            clock.note(
                GraphTime {
                    delay: frames.min(i64::MAX as u64) as i64,
                    buffered: 0,
                    queued: 0,
                    tick,
                },
                rate,
            );
        }
        if draining && empty.load(Ordering::Acquire) {
            let ready_at = *drain_after.get_or_insert_with(|| {
                Instant::now() + clock.latency().to_duration(rate).max(SHORTEST_DRAIN)
            });
            if Instant::now() >= ready_at {
                let _ = stream.request_stop();
                let _ = announced.send(StreamEvent::Drained);
                draining = false;
                drain_after = None;
            }
        } else {
            drain_after = None;
        }
        match commands.recv_timeout(between_commands(state, draining)) {
            Ok(Order::Command(StreamCommand::Close)) | Err(RecvTimeoutError::Disconnected) => {
                return;
            }
            Ok(Order::Command(StreamCommand::Drain)) => draining = true,
            Ok(Order::Reshaped) => {
                draining = false;
                drain_after = None;
            }
            Ok(Order::Command(StreamCommand::SetActive(active))) => {
                let result = if active {
                    stream.request_start()
                } else {
                    stream.request_pause()
                };
                let next = if result.is_err() {
                    StreamState::Failed
                } else if active {
                    StreamState::Streaming
                } else {
                    StreamState::Paused
                };
                let _ = announced.send(StreamEvent::StateChanged {
                    from: state,
                    to: next,
                });
                state = next;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

impl Opener for Device {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Result<SinkStream> {
        let spec = request.spec;
        let play_as: fn(Opening) = match (spec.format, spec.channels) {
            (SampleFormat::F32, ChannelLayout::Stereo) => play::<f32, Stereo>,
            (SampleFormat::F32, ChannelLayout::Mono) => play::<f32, Mono>,
            (SampleFormat::S16, ChannelLayout::Stereo) => play::<i16, Stereo>,
            (SampleFormat::S16, ChannelLayout::Mono) => play::<i16, Mono>,
            (SampleFormat::S32, ChannelLayout::Stereo) => play::<i32, Stereo>,
            (SampleFormat::S32, ChannelLayout::Mono) => play::<i32, Mono>,
            _ => return Err(Error::FormatRefused),
        };
        let (commands, asked) = unbounded();
        let (announced, events) = unbounded();
        let (ready, started) = bounded(1);
        let (fed, handing) = fed_from(source);
        let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
        let opening = Opening {
            spec,
            way: self.0.way_for(spec),
            routing: Arc::clone(&self.0),
            fed,
            ready,
            commands: asked,
            announced,
            clock: Arc::clone(&clock),
        };
        thread::Builder::new()
            .name("resonate-mobile-output".into())
            .spawn(move || play_as(opening))
            .map_err(|source| Error::Backend {
                op: Operation::Open,
                source,
            })?;
        let opened = opened_within(&started, OPEN_WITHIN)?;
        let reshape = reshaper(opened, Arc::clone(&self.0), handing, commands.clone());
        Ok(SinkStream::new(events, clock, controlled_by(commands)).reshaped_by(reshape))
    }
}

impl Backend for AndroidAudio {
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

fn backend_error(op: Operation, source: oboe::Error) -> Error {
    Error::Backend {
        op,
        source: io::Error::other(source),
    }
}
