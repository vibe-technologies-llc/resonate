use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, bounded, unbounded};
use oboe::{
    AudioOutputStream, AudioStream, AudioStreamBase, AudioStreamBuilder, AudioStreamSafe,
    PerformanceMode, SharingMode, Stereo, Usage,
};
use resonate_audio::{
    AudioSource, Backend, Error, HardwareVolume, NodeName, Opener, Operation, Result, SinkChange,
    SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamClock, StreamCommand, StreamEvent,
    StreamRequest, StreamState, Surveyor, Words,
};
use resonate_core::{ChannelLayout, Frames, Gain, SampleFormat, SampleRate, StreamSpec};

use super::oboe_render::{RENDER_BYTES, Rendering};

const OUTPUT: SinkId = SinkId::new(1);
const RATE: SampleRate = SampleRate::HZ_48000;

pub(crate) struct AndroidAudio {
    changes: Receiver<SinkChange>,
    _announce: crossbeam_channel::Sender<SinkChange>,
    routing: Arc<Routing>,
}

impl AndroidAudio {
    pub(crate) fn new(routing: Arc<Routing>) -> Self {
        let (announce, changes) = unbounded();
        Self {
            changes,
            _announce: announce,
            routing,
        }
    }
}

#[derive(Default)]
pub(crate) struct Routing {
    pub(crate) requested: AtomicI32,
    pub(crate) actual: AtomicI32,
}

impl Routing {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn select(&self, device: i32) -> bool {
        self.requested.store(device, Ordering::Relaxed);
        true
    }

    pub(crate) fn actual(&self) -> i32 {
        self.actual.load(Ordering::Relaxed)
    }

    pub(crate) fn route_changed(&self, _: u32, _: f64) -> bool {
        false
    }
}

struct Device(Arc<Routing>);

impl Surveyor for Device {
    fn enumerate_sinks(&self, _: Duration) -> Result<Vec<SinkInfo>> {
        Ok(vec![SinkInfo {
            id: OUTPUT,
            name: NodeName::new("android-default"),
            description: "Android audio output".into(),
            is_default: true,
            is_hardware: false,
            port: Some(SinkPort {
                description: "System route".into(),
                plugged: resonate_audio::Plugged::Yes,
                hardware_volume: HardwareVolume::No,
                volume: None,
                muted: false,
            }),
            profile: None,
            formats: vec![SinkFormats {
                format: SampleFormat::F32,
                words: Words::Whole,
                rates: vec![RATE],
                channels: vec![ChannelLayout::Stereo],
            }],
            allowed_rates: vec![RATE],
            current_rate: Some(RATE),
        }])
    }
}

impl Opener for Device {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Result<SinkStream> {
        let spec = StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::F32);
        if request.spec != spec {
            return Err(Error::FormatRefused);
        }
        let (commands, asked) = unbounded();
        let (announced, events) = unbounded();
        let (ready, started) = bounded(1);
        let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
        let empty = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let thread_clock = Arc::clone(&clock);
        let routing = Arc::clone(&self.0);
        let requested_device = routing.requested.load(Ordering::Relaxed);
        thread::Builder::new()
            .name("resonate-mobile-output".into())
            .spawn(move || {
                let callback = Rendering {
                    source,
                    empty: Arc::clone(&empty),
                    failed: Arc::clone(&failed),
                    bytes: vec![0; RENDER_BYTES].into_boxed_slice(),
                };
                let opened = AudioStreamBuilder::default()
                    .set_f32()
                    .set_channel_count::<Stereo>()
                    .set_sample_rate(RATE.hz() as i32)
                    .set_device_id(requested_device)
                    .set_performance_mode(PerformanceMode::LowLatency)
                    .set_sharing_mode(SharingMode::Shared)
                    .set_usage(Usage::Media)
                    .set_callback(callback)
                    .open_stream();
                let mut stream = match opened {
                    Ok(stream) if stream.get_sample_rate() == RATE.hz() as i32 => stream,
                    Ok(_) => {
                        let _ = ready.send(Err(Error::FormatRefused));
                        return;
                    }
                    Err(source) => {
                        let _ = ready.send(Err(backend_error(Operation::Open, source)));
                        return;
                    }
                };
                routing
                    .actual
                    .store(stream.get_device_id(), Ordering::Relaxed);
                let _ = announced.send(StreamEvent::FormatChanged {
                    spec,
                    words: Words::Whole,
                });
                let _ = ready.send(Ok(()));
                let mut state = StreamState::Paused;
                let mut draining = false;
                let mut drain_after = None;
                loop {
                    if failed.load(Ordering::Acquire) {
                        let _ = announced.send(StreamEvent::StateChanged {
                            from: state,
                            to: StreamState::Failed,
                        });
                        break;
                    }
                    if let Ok(latency) = stream.calculate_latency_millis() {
                        let frames =
                            Frames((latency.max(0.0) * f64::from(RATE.hz()) / 1000.0) as u64);
                        thread_clock.note(
                            resonate_audio::GraphTime {
                                delay: frames.get().min(i64::MAX as u64) as i64,
                                buffered: 0,
                                queued: 0,
                                tick: resonate_core::Ratio {
                                    numer: std::num::NonZeroU32::MIN,
                                    denom: std::num::NonZeroU32::new(RATE.hz())
                                        .unwrap_or(std::num::NonZeroU32::MIN),
                                },
                            },
                            RATE,
                        );
                    }
                    if draining && empty.load(Ordering::Acquire) {
                        let ready_at = *drain_after.get_or_insert_with(|| {
                            Instant::now()
                                + thread_clock
                                    .latency()
                                    .to_duration(RATE)
                                    .max(Duration::from_millis(20))
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
                    match asked.recv_timeout(Duration::from_millis(10)) {
                        Ok(StreamCommand::Close)
                        | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                            let _ = stream.close();
                            break;
                        }
                        Ok(StreamCommand::Drain) => draining = true,
                        Ok(StreamCommand::SetActive(active)) => {
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
                        Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .map_err(|source| Error::Backend {
                op: Operation::Open,
                source,
            })?;
        started.recv().map_err(|_| Error::LoopStopped)??;
        Ok(SinkStream::new(
            events,
            clock,
            Box::new(move |command| commands.send(command).map_err(|_| Error::Disconnected)),
        ))
    }
}

impl Backend for AndroidAudio {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
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
