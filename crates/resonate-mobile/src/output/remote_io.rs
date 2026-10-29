use std::{
    ffi::c_void,
    io,
    ptr::{self, NonNull},
    sync::Arc,
    time::Duration,
};

use crossbeam_channel::Receiver;
use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioComponent, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioOutputUnitStart,
    AudioOutputUnitStop, AudioUnit, AudioUnitInitialize, AudioUnitSetProperty,
    AudioUnitUninitialize, kAudioUnitManufacturer_Apple, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Input, kAudioUnitType_Output,
};
use objc2_core_audio_types::AudioStreamBasicDescription;
use resonate_audio::{
    AudioSource, Backend, Error, HardwareVolume, NodeName, Opener, Operation, Plugged, Result,
    SinkChange, SinkFormats, SinkId, SinkInfo, SinkPort, SinkStream, StreamRequest, Surveyor,
    Words,
};
use resonate_core::{ChannelLayout, Gain, SampleFormat};

use super::{
    remote_io_fill::Rendering,
    remote_io_render::render,
    remote_io_state::Routing,
    remote_io_stream::launch,
    remote_io_unit::{Toolbox, Unit},
};

const REMOTE_IO: u32 = u32::from_be_bytes(*b"rioc");
const OUTPUT_BUS: u32 = 0;
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

struct CoreAudio;

impl CoreAudio {
    fn set<T>(unit: AudioUnit, property: u32, value: &T) -> i32 {
        unsafe {
            AudioUnitSetProperty(
                unit,
                property,
                kAudioUnitScope_Input,
                OUTPUT_BUS,
                ptr::from_ref(value).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        }
    }

    fn component() -> AudioComponent {
        let description = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: REMOTE_IO,
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(&description)) }
    }
}

impl Toolbox for CoreAudio {
    type Instance = AudioUnit;

    fn has_component(&self) -> bool {
        !Self::component().is_null()
    }

    fn instantiate(&self) -> std::result::Result<AudioUnit, i32> {
        let mut unit: AudioUnit = ptr::null_mut();
        match unsafe { AudioComponentInstanceNew(Self::component(), NonNull::from(&mut unit)) } {
            0 => Ok(unit),
            status => Err(status),
        }
    }

    fn set_format(&self, unit: AudioUnit, format: &AudioStreamBasicDescription) -> i32 {
        Self::set(unit, kAudioUnitProperty_StreamFormat, format)
    }

    fn set_render(&self, unit: AudioUnit, rendering: NonNull<Rendering>) -> i32 {
        let callback = AURenderCallbackStruct {
            inputProc: Some(render),
            inputProcRefCon: rendering.as_ptr().cast::<c_void>(),
        };
        Self::set(unit, kAudioUnitProperty_SetRenderCallback, &callback)
    }

    fn initialize(&self, unit: AudioUnit) -> i32 {
        unsafe { AudioUnitInitialize(unit) }
    }

    fn start(&self, unit: AudioUnit) -> i32 {
        unsafe { AudioOutputUnitStart(unit) }
    }

    fn stop(&self, unit: AudioUnit) -> i32 {
        unsafe { AudioOutputUnitStop(unit) }
    }

    fn uninitialize(&self, unit: AudioUnit) -> i32 {
        unsafe { AudioUnitUninitialize(unit) }
    }

    fn dispose(&self, unit: AudioUnit) -> i32 {
        unsafe { AudioComponentInstanceDispose(unit) }
    }
}

impl Opener for Device {
    fn open(&self, request: &StreamRequest, source: Box<dyn AudioSource>) -> Result<SinkStream> {
        launch(&self.0, request, source, UNIT_WITHIN, |rate, rendering| {
            Unit::opened(CoreAudio, rate, rendering)
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
