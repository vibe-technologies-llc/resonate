use std::{
    error,
    ffi::c_void,
    fmt, io,
    mem::size_of,
    num::NonZeroU32,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, unbounded};
use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioOutputUnitStart,
    AudioOutputUnitStop, AudioUnit, AudioUnitInitialize, AudioUnitSetProperty,
    AudioUnitUninitialize, kAudioUnitManufacturer_Apple, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, kAudioUnitType_Output,
};
use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM,
};
use resonate_audio::{
    AudioSource, Backend, Error, GraphTime, HardwareVolume, NodeName, Opener, Operation, Plugged,
    Result, SinkChange, SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamClock,
    StreamCommand, StreamEvent, StreamRequest, StreamState, Surveyor, Words,
};
use resonate_core::{ChannelLayout, Frames, Gain, Ratio, SampleFormat, SampleRate, StreamSpec};

use super::remote_io_render::{Rendering, render};

const OUTPUT: SinkId = SinkId::new(1);
const REMOTE_IO: u32 = u32::from_be_bytes(*b"rioc");
const OUTPUT_BUS: u32 = 0;
const STEREO_CHANNELS: u32 = 2;
const FLOAT_BYTES: u32 = size_of::<f32>() as u32;
const COMMAND_POLL: Duration = Duration::from_millis(10);
const UNIT_WITHIN: Duration = Duration::from_secs(3);
const SHORTEST_DRAIN: Duration = Duration::from_millis(20);
const MILLISECONDS_A_SECOND: f64 = 1000.0;

pub(crate) struct Routing {
    rate: AtomicU32,
    latency: AtomicU64,
    announce: Sender<SinkChange>,
    changes: Receiver<SinkChange>,
}

impl Routing {
    pub(crate) fn new() -> Self {
        let (announce, changes) = unbounded();
        Self {
            rate: AtomicU32::new(SampleRate::HZ_48000.hz()),
            latency: AtomicU64::new(0),
            announce,
            changes,
        }
    }

    pub(crate) fn select(&self, _: i32) -> bool {
        false
    }

    pub(crate) fn actual(&self) -> i32 {
        0
    }

    pub(crate) fn route_changed(&self, rate: u32, latency_ms: f64) -> bool {
        let Ok(rate) = SampleRate::new(rate) else {
            return false;
        };
        let latency = latency_ms.max(0.0) * f64::from(rate.hz()) / MILLISECONDS_A_SECOND;
        self.latency.store(latency as u64, Ordering::Relaxed);
        let moved = self.rate.swap(rate.hz(), Ordering::Relaxed) != rate.hz();
        if moved {
            let _ = self.announce.send(SinkChange::Reformatted(OUTPUT));
        }
        moved
    }

    fn rate(&self) -> SampleRate {
        SampleRate::new(self.rate.load(Ordering::Relaxed)).unwrap_or(SampleRate::HZ_48000)
    }

    fn latency(&self) -> Frames {
        Frames(self.latency.load(Ordering::Relaxed))
    }
}

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
            id: OUTPUT,
            name: NodeName::new("ios-route"),
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

#[derive(Debug)]
struct Status(i32);

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "OSStatus {}", self.0)
    }
}

impl error::Error for Status {}

fn checked(op: Operation, status: i32) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(Error::Backend {
            op,
            source: io::Error::other(Status(status)),
        })
    }
}

struct Unit {
    unit: AudioUnit,
    rendering: NonNull<Rendering>,
}

impl Unit {
    fn opened(rate: SampleRate, rendering: Box<Rendering>) -> Result<Self> {
        let mut opened = Self {
            unit: ptr::null_mut(),
            rendering: NonNull::from(Box::leak(rendering)),
        };
        let description = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: REMOTE_IO,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        let component =
            unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(&description)) };
        if component.is_null() {
            return Err(Error::Backend {
                op: Operation::Open,
                source: io::ErrorKind::NotFound.into(),
            });
        }
        checked(Operation::Open, unsafe {
            AudioComponentInstanceNew(component, NonNull::from(&mut opened.unit))
        })?;
        let format = AudioStreamBasicDescription {
            mSampleRate: f64::from(rate.hz()),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagsNativeFloatPacked,
            mBytesPerPacket: STEREO_CHANNELS * FLOAT_BYTES,
            mFramesPerPacket: 1,
            mBytesPerFrame: STEREO_CHANNELS * FLOAT_BYTES,
            mChannelsPerFrame: STEREO_CHANNELS,
            mBitsPerChannel: FLOAT_BYTES * 8,
            mReserved: 0,
        };
        opened.set(kAudioUnitProperty_StreamFormat, &format)?;
        let callback = AURenderCallbackStruct {
            inputProc: Some(render),
            inputProcRefCon: opened.rendering.as_ptr().cast::<c_void>(),
        };
        opened.set(kAudioUnitProperty_SetRenderCallback, &callback)?;
        checked(Operation::Open, unsafe { AudioUnitInitialize(opened.unit) })?;
        Ok(opened)
    }

    fn set<T>(&self, property: u32, value: &T) -> Result<()> {
        checked(Operation::Open, unsafe {
            AudioUnitSetProperty(
                self.unit,
                property,
                kAudioUnitScope_Input,
                OUTPUT_BUS,
                ptr::from_ref(value).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        })
    }

    fn start(&self) -> Result<()> {
        checked(Operation::Open, unsafe { AudioOutputUnitStart(self.unit) })
    }

    fn stop(&self) -> Result<()> {
        checked(Operation::Open, unsafe { AudioOutputUnitStop(self.unit) })
    }
}

impl Drop for Unit {
    fn drop(&mut self) {
        if !self.unit.is_null() {
            unsafe {
                AudioOutputUnitStop(self.unit);
                AudioUnitUninitialize(self.unit);
                AudioComponentInstanceDispose(self.unit);
            }
        }
        drop(unsafe { Box::from_raw(self.rendering.as_ptr()) });
    }
}

impl Opener for Device {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Result<SinkStream> {
        let rate = self.0.rate();
        let spec = StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32);
        if request.spec != spec {
            return Err(Error::FormatRefused);
        }
        let (commands, asked) = unbounded();
        let (announced, events) = unbounded();
        let (ready, started) = bounded(1);
        let (disposed, closed) = bounded(1);
        let clock = Arc::new(StreamClock::behind_by(Frames::ZERO));
        let empty = Arc::new(AtomicBool::new(false));
        let thread_clock = Arc::clone(&clock);
        let routing = Arc::clone(&self.0);
        thread::Builder::new()
            .name("resonate-mobile-output".into())
            .spawn(move || {
                let rendering = Box::new(Rendering {
                    source,
                    empty: Arc::clone(&empty),
                });
                let unit = match Unit::opened(rate, rendering) {
                    Ok(unit) => unit,
                    Err(refused) => {
                        let _ = ready.send(Err(refused));
                        return;
                    }
                };
                let _ = announced.send(StreamEvent::FormatChanged {
                    spec,
                    words: Words::Whole,
                });
                let _ = ready.send(Ok(()));
                serve(
                    &unit,
                    rate,
                    &routing,
                    &thread_clock,
                    &empty,
                    &asked,
                    &announced,
                );
                drop(unit);
                let _ = disposed.send(());
            })
            .map_err(|source| Error::Backend {
                op: Operation::Open,
                source,
            })?;
        match started.recv_timeout(UNIT_WITHIN) {
            Ok(opened) => opened?,
            Err(RecvTimeoutError::Timeout) => {
                return Err(Error::Backend {
                    op: Operation::Open,
                    source: io::ErrorKind::TimedOut.into(),
                });
            }
            Err(RecvTimeoutError::Disconnected) => return Err(Error::LoopStopped),
        }
        Ok(SinkStream::new(
            events,
            clock,
            Box::new(move |command| {
                let closing = matches!(command, StreamCommand::Close);
                commands.send(command).map_err(|_| Error::Disconnected)?;
                if closing {
                    closed
                        .recv_timeout(UNIT_WITHIN)
                        .map_err(|_| Error::Backend {
                            op: Operation::Close,
                            source: io::ErrorKind::TimedOut.into(),
                        })?;
                }
                Ok(())
            }),
        ))
    }
}

fn serve(
    unit: &Unit,
    rate: SampleRate,
    routing: &Routing,
    clock: &StreamClock,
    empty: &AtomicBool,
    asked: &Receiver<StreamCommand>,
    announced: &Sender<StreamEvent>,
) {
    let tick = Ratio {
        numer: NonZeroU32::MIN,
        denom: NonZeroU32::new(rate.hz()).unwrap_or(NonZeroU32::MIN),
    };
    let mut state = StreamState::Paused;
    let mut draining = false;
    let mut drain_after = None;
    loop {
        clock.note(
            GraphTime {
                delay: routing.latency().get().min(i64::MAX as u64) as i64,
                buffered: 0,
                queued: 0,
                tick,
            },
            rate,
        );
        if draining && empty.load(Ordering::Acquire) {
            let ready_at = *drain_after.get_or_insert_with(|| {
                Instant::now() + clock.latency().to_duration(rate).max(SHORTEST_DRAIN)
            });
            if Instant::now() >= ready_at {
                let _ = unit.stop();
                let _ = announced.send(StreamEvent::Drained);
                draining = false;
                drain_after = None;
            }
        } else {
            drain_after = None;
        }
        match asked.recv_timeout(COMMAND_POLL) {
            Ok(StreamCommand::Close) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(StreamCommand::Drain) => draining = true,
            Ok(StreamCommand::SetActive(active)) => {
                let result = if active { unit.start() } else { unit.stop() };
                let next = match (result, active) {
                    (Err(_), _) => StreamState::Failed,
                    (Ok(()), true) => StreamState::Streaming,
                    (Ok(()), false) => StreamState::Paused,
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
