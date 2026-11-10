use std::{
    ffi::c_void,
    mem::MaybeUninit,
    num::NonZeroU32,
    ptr::{self, NonNull},
    result,
};

use objc2_audio_toolbox::{
    AURenderCallbackStruct, AudioComponent, AudioComponentDescription, AudioComponentFindNext,
    AudioComponentInstanceDispose, AudioComponentInstanceNew, AudioOutputUnitStart,
    AudioOutputUnitStop, AudioUnit, AudioUnitGetProperty, AudioUnitInitialize,
    AudioUnitSetProperty, AudioUnitUninitialize, kAudioOutputUnitProperty_CurrentDevice,
    kAudioOutputUnitProperty_EnableIO, kAudioOutputUnitProperty_SetInputCallback,
    kAudioUnitManufacturer_Apple, kAudioUnitProperty_SetRenderCallback,
    kAudioUnitProperty_StreamFormat, kAudioUnitScope_Global, kAudioUnitScope_Input,
    kAudioUnitScope_Output, kAudioUnitSubType_HALOutput, kAudioUnitType_Output,
};
use objc2_core_audio_types::AudioStreamBasicDescription;

use crate::{
    asbd::Format,
    clock::HostClock,
    error::Result,
    render::{hear, render},
    status::OsStatus,
    unit::{Bus, Component, Hear, Hearing, Render, Rendered, Sequenced, Shape, Toolbox},
};

const REMOTE_IO: u32 = u32::from_be_bytes(*b"rioc");
const OUTPUT_ELEMENT: u32 = 0;
const INPUT_ELEMENT: u32 = 1;

#[derive(Clone, Copy)]
struct Place {
    scope: u32,
    element: u32,
}

impl Place {
    const OF_THE_UNIT: Self = Self {
        scope: kAudioUnitScope_Global,
        element: OUTPUT_ELEMENT,
    };

    const fn of_our_side(bus: Bus) -> Self {
        match bus {
            Bus::Output => Self {
                scope: kAudioUnitScope_Input,
                element: OUTPUT_ELEMENT,
            },
            Bus::Input => Self {
                scope: kAudioUnitScope_Output,
                element: INPUT_ELEMENT,
            },
        }
    }

    const fn of_the_devices_side(bus: Bus) -> Self {
        match bus {
            Bus::Output => Self {
                scope: kAudioUnitScope_Output,
                element: OUTPUT_ELEMENT,
            },
            Bus::Input => Self {
                scope: kAudioUnitScope_Input,
                element: INPUT_ELEMENT,
            },
        }
    }
}

pub(crate) struct AudioToolbox;

impl AudioToolbox {
    fn set<T>(unit: AudioUnit, property: u32, place: Place, value: &T) -> i32 {
        unsafe {
            AudioUnitSetProperty(
                unit,
                property,
                place.scope,
                place.element,
                ptr::from_ref(value).cast::<c_void>(),
                size_of::<T>() as u32,
            )
        }
    }

    fn component(component: Component) -> AudioComponent {
        let description = AudioComponentDescription {
            componentType: kAudioUnitType_Output,
            componentSubType: match component {
                Component::RemoteIo => REMOTE_IO,
                Component::HalOutput => kAudioUnitSubType_HALOutput,
            },
            componentManufacturer: kAudioUnitManufacturer_Apple,
            componentFlags: 0,
            componentFlagsMask: 0,
        };
        unsafe { AudioComponentFindNext(ptr::null_mut(), NonNull::from(&description)) }
    }
}

impl Toolbox for AudioToolbox {
    type Instance = AudioUnit;

    fn host_clock(&self) -> HostClock {
        HostClock::of_this_machine()
    }

    fn has_component(&self, component: Component) -> bool {
        !Self::component(component).is_null()
    }

    fn instantiate(&self, component: Component) -> result::Result<AudioUnit, i32> {
        let mut unit: AudioUnit = ptr::null_mut();
        let found = Self::component(component);
        match unsafe { AudioComponentInstanceNew(found, NonNull::from(&mut unit)) } {
            0 => Ok(unit),
            status => Err(status),
        }
    }

    fn enable_io(&self, unit: AudioUnit, bus: Bus, enabled: bool) -> i32 {
        Self::set(
            unit,
            kAudioOutputUnitProperty_EnableIO,
            Place::of_the_devices_side(bus),
            &u32::from(enabled),
        )
    }

    fn set_device(&self, unit: AudioUnit, device: u32) -> i32 {
        Self::set(
            unit,
            kAudioOutputUnitProperty_CurrentDevice,
            Place::OF_THE_UNIT,
            &device,
        )
    }

    fn set_format(&self, unit: AudioUnit, bus: Bus, format: &AudioStreamBasicDescription) -> i32 {
        Self::set(
            unit,
            kAudioUnitProperty_StreamFormat,
            Place::of_our_side(bus),
            format,
        )
    }

    fn format_of_the_device(
        &self,
        unit: AudioUnit,
        bus: Bus,
    ) -> result::Result<AudioStreamBasicDescription, i32> {
        let place = Place::of_the_devices_side(bus);
        let mut format = MaybeUninit::<AudioStreamBasicDescription>::zeroed();
        let mut bytes = size_of::<AudioStreamBasicDescription>() as u32;
        let status = unsafe {
            AudioUnitGetProperty(
                unit,
                kAudioUnitProperty_StreamFormat,
                place.scope,
                place.element,
                NonNull::from(&mut format).cast::<c_void>(),
                NonNull::from(&mut bytes),
            )
        };
        match status {
            0 => Ok(unsafe { format.assume_init() }),
            refused => Err(refused),
        }
    }

    fn set_render(&self, unit: AudioUnit, rendered: NonNull<Rendered>) -> i32 {
        let callback = AURenderCallbackStruct {
            inputProc: Some(render),
            inputProcRefCon: rendered.as_ptr().cast::<c_void>(),
        };
        Self::set(
            unit,
            kAudioUnitProperty_SetRenderCallback,
            Place::of_our_side(Bus::Output),
            &callback,
        )
    }

    fn set_hearing(&self, unit: AudioUnit, hearing: NonNull<Hearing<AudioUnit>>) -> i32 {
        let callback = AURenderCallbackStruct {
            inputProc: Some(hear),
            inputProcRefCon: hearing.as_ptr().cast::<c_void>(),
        };
        Self::set(
            unit,
            kAudioOutputUnitProperty_SetInputCallback,
            Place::OF_THE_UNIT,
            &callback,
        )
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

pub struct Unit(Sequenced<AudioToolbox>);

impl Unit {
    pub fn playing(shape: Shape, render: Box<dyn Render>) -> Result<Self> {
        Sequenced::playing(AudioToolbox, shape, render).map(Self)
    }

    pub fn hearing(
        shape: Shape,
        hear: Box<dyn Hear>,
        frames_a_slice_at_most: NonZeroU32,
    ) -> Result<Self> {
        Sequenced::hearing(AudioToolbox, shape, hear, frames_a_slice_at_most).map(Self)
    }

    pub fn start(&self) -> result::Result<(), OsStatus> {
        self.0.start()
    }

    pub fn stop(&self) -> result::Result<(), OsStatus> {
        self.0.stop()
    }

    pub fn reformatted(&self, format: Format) -> result::Result<(), OsStatus> {
        self.0.reformatted(format)
    }

    pub fn device_rate(&self) -> Result<f64> {
        self.0.device_rate()
    }
}
