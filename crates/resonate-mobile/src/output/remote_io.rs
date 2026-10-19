use std::{
    error,
    ffi::c_void,
    fmt, io,
    mem::size_of,
    num::NonZeroU32,
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, bounded, select, unbounded};
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

use super::{
    remote_io_fill::Rendering,
    remote_io_render::render,
    remote_io_state::{Drain, OUTPUT, Routing},
};

const REMOTE_IO: u32 = u32::from_be_bytes(*b"rioc");
const OUTPUT_BUS: u32 = 0;
const STEREO_CHANNELS: u32 = 2;
const FLOAT_BYTES: u32 = size_of::<f32>() as u32;
const COMMAND_POLL: Duration = Duration::from_millis(10);
const QUIET_WAIT: Duration = Duration::from_secs(24 * 60 * 60);
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

    fn start(&self) -> i32 {
        unsafe { AudioOutputUnitStart(self.unit) }
    }

    fn stop(&self) -> i32 {
        unsafe { AudioOutputUnitStop(self.unit) }
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
    let mut drain = Drain::new();
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
        let latency = clock.latency().to_duration(rate);
        if drain.settled(empty.load(Ordering::Acquire), latency, Instant::now()) {
            unit.stop();
            let _ = announced.send(StreamEvent::Drained);
        }
        let wait = if drain.is_requested() {
            COMMAND_POLL
        } else {
            QUIET_WAIT
        };
        let command = select! {
            recv(asked) -> command => Some(command),
            recv(routing.latency_moves) -> _ => None,
            default(wait) => None,
        };
        match command {
            Some(Ok(StreamCommand::Close) | Err(_)) => break,
            Some(Ok(StreamCommand::Drain)) => drain.request(),
            Some(Ok(StreamCommand::SetActive(active))) => {
                let status = if active { unit.start() } else { unit.stop() };
                if status != 0 {
                    let op = if active {
                        Operation::Start
                    } else {
                        Operation::Pause
                    };
                    let _ = announced.send(StreamEvent::Refused {
                        from: state,
                        op,
                        status,
                    });
                    state = StreamState::Failed;
                    continue;
                }
                let next = if active {
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
            None => {}
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
