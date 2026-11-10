use std::{
    ffi::c_void,
    ptr::{self, NonNull},
};

use objc2_core_audio::{
    AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectHasProperty,
    AudioObjectIsPropertySettable, AudioObjectPropertyAddress, AudioObjectSetPropertyData,
    AudioStreamRangedDescription, kAudioObjectPropertyElementMain, kAudioObjectPropertyScopeGlobal,
    kAudioObjectPropertyScopeInput, kAudioObjectPropertyScopeOutput,
};
use objc2_core_audio_types::{AudioStreamBasicDescription, AudioValueRange};
use objc2_core_foundation::{CFRetained, CFString};

use crate::{
    error::{Error, Op, Result},
    status::OsStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Output,
    Input,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Address(AudioObjectPropertyAddress);

impl Address {
    pub(crate) const fn global(selector: u32) -> Self {
        Self(AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMain,
        })
    }

    pub(crate) const fn towards(direction: Direction, selector: u32) -> Self {
        Self(AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: match direction {
                Direction::Output => kAudioObjectPropertyScopeOutput,
                Direction::Input => kAudioObjectPropertyScopeInput,
            },
            mElement: kAudioObjectPropertyElementMain,
        })
    }

    pub(crate) const fn element(self, element: u32) -> Self {
        Self(AudioObjectPropertyAddress {
            mElement: element,
            ..self.0
        })
    }

    pub(crate) const fn raw(&self) -> &AudioObjectPropertyAddress {
        &self.0
    }
}

pub(crate) trait Plain: Copy {
    const EMPTY: Self;
}

impl Plain for u32 {
    const EMPTY: Self = 0;
}

impl Plain for i32 {
    const EMPTY: Self = 0;
}

impl Plain for f32 {
    const EMPTY: Self = 0.0;
}

impl Plain for f64 {
    const EMPTY: Self = 0.0;
}

impl Plain for [u32; 2] {
    const EMPTY: Self = [0; 2];
}

impl Plain for AudioValueRange {
    const EMPTY: Self = Self {
        mMinimum: 0.0,
        mMaximum: 0.0,
    };
}

impl Plain for AudioStreamBasicDescription {
    const EMPTY: Self = Self {
        mSampleRate: 0.0,
        mFormatID: 0,
        mFormatFlags: 0,
        mBytesPerPacket: 0,
        mFramesPerPacket: 0,
        mBytesPerFrame: 0,
        mChannelsPerFrame: 0,
        mBitsPerChannel: 0,
        mReserved: 0,
    };
}

impl Plain for AudioStreamRangedDescription {
    const EMPTY: Self = Self {
        mFormat: AudioStreamBasicDescription::EMPTY,
        mSampleRateRange: AudioValueRange::EMPTY,
    };
}

fn checked(op: Op, status: i32) -> Result<()> {
    OsStatus::checked(status).map_err(Error::refused(op))
}

pub(crate) fn has(object: u32, address: Address) -> bool {
    unsafe { AudioObjectHasProperty(object, NonNull::from(address.raw())) }
}

pub(crate) fn settable(object: u32, address: Address) -> bool {
    let mut settable = 0_u8;
    let status = unsafe {
        AudioObjectIsPropertySettable(
            object,
            NonNull::from(address.raw()),
            NonNull::from(&mut settable),
        )
    };
    status == 0 && settable != 0
}

fn bytes_of(object: u32, address: Address, op: Op) -> Result<u32> {
    let mut bytes = 0_u32;
    let status = unsafe {
        AudioObjectGetPropertyDataSize(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            NonNull::from(&mut bytes),
        )
    };
    checked(op, status).map(|()| bytes)
}

pub(crate) fn read<T: Plain>(object: u32, address: Address, op: Op) -> Result<T> {
    let mut value = T::EMPTY;
    let mut bytes = size_of::<T>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            NonNull::from(&mut bytes),
            NonNull::from(&mut value).cast::<c_void>(),
        )
    };
    checked(op, status)?;
    if bytes as usize == size_of::<T>() {
        Ok(value)
    } else {
        Err(Error::Missized { op, bytes })
    }
}

pub(crate) fn read_all<T: Plain>(object: u32, address: Address, op: Op) -> Result<Vec<T>> {
    let each = size_of::<T>();
    let mut bytes = bytes_of(object, address, op)?;
    if !(bytes as usize).is_multiple_of(each) {
        return Err(Error::Missized { op, bytes });
    }
    let mut values = vec![T::EMPTY; bytes as usize / each];
    let Some(first) = NonNull::new(values.as_mut_ptr()) else {
        return Ok(values);
    };
    if values.is_empty() {
        return Ok(values);
    }
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            NonNull::from(&mut bytes),
            first.cast::<c_void>(),
        )
    };
    checked(op, status)?;
    values.truncate(bytes as usize / each);
    Ok(values)
}

pub(crate) fn write<T: Plain>(object: u32, address: Address, value: &T, op: Op) -> Result<()> {
    let status = unsafe {
        AudioObjectSetPropertyData(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            size_of::<T>() as u32,
            NonNull::from(value).cast::<c_void>(),
        )
    };
    checked(op, status)
}

pub(crate) fn write_no_pointer(object: u32, address: Address, op: Op) -> Result<()> {
    let nothing: *const c_void = ptr::null();
    let status = unsafe {
        AudioObjectSetPropertyData(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            size_of::<*const c_void>() as u32,
            NonNull::from(&nothing).cast::<c_void>(),
        )
    };
    checked(op, status)
}

pub(crate) fn read_text(object: u32, address: Address, op: Op) -> Result<String> {
    let mut text: *mut CFString = ptr::null_mut();
    let mut bytes = size_of::<*mut CFString>() as u32;
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(address.raw()),
            0,
            ptr::null(),
            NonNull::from(&mut bytes),
            NonNull::from(&mut text).cast::<c_void>(),
        )
    };
    checked(op, status)?;
    let Some(text) = NonNull::new(text) else {
        return Err(Error::Missized { op, bytes: 0 });
    };
    let owned = unsafe { CFRetained::from_raw(text) };
    Ok(owned.to_string())
}
