use std::{
    ffi::{CStr, c_char, c_void},
    mem,
    ptr::{self, NonNull},
};

use objc2::{
    AnyThread, Message,
    rc::Retained,
    runtime::{AnyClass, AnyObject},
};
use objc2_core_audio::{
    AudioHardwareCreateAggregateDevice, AudioHardwareDestroyAggregateDevice, CATapDescription,
};
use objc2_core_foundation::CFDictionary;
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSString, NSUUID};

use crate::{
    Device,
    error::{Error, Op, Result},
    status::OsStatus,
};

const TAP_CLASS: &CStr = c"CATapDescription";
const CREATE_TAP: &CStr = c"AudioHardwareCreateProcessTap";
const DESTROY_TAP: &CStr = c"AudioHardwareDestroyProcessTap";
const EVERY_LOADED_IMAGE: *mut c_void = -2isize as *mut c_void;

const AGGREGATE_UID: &str = "uid";
const AGGREGATE_NAME: &str = "name";
const AGGREGATE_IS_PRIVATE: &str = "private";
const AGGREGATE_MAIN_SUB_DEVICE: &str = "master";
const AGGREGATE_SUB_DEVICES: &str = "subdevices";
const AGGREGATE_TAPS: &str = "taps";
const AGGREGATE_TAPS_START_THEMSELVES: &str = "tapautostart";
const SUB_DEVICE_UID: &str = "uid";
const SUB_TAP_UID: &str = "uid";
const SUB_TAP_DRIFT_COMPENSATION: &str = "drift";
const TAPPED_NAME: &str = "Resonate desktop";

type CreateTap = unsafe extern "C-unwind" fn(*const CATapDescription, *mut u32) -> i32;
type DestroyTap = unsafe extern "C-unwind" fn(u32) -> i32;

unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

struct Tapping {
    create: CreateTap,
    destroy: DestroyTap,
}

impl Tapping {
    fn of_this_system() -> Option<Self> {
        AnyClass::get(TAP_CLASS)?;
        let create = unsafe { dlsym(EVERY_LOADED_IMAGE, CREATE_TAP.as_ptr()) };
        let destroy = unsafe { dlsym(EVERY_LOADED_IMAGE, DESTROY_TAP.as_ptr()) };
        if create.is_null() || destroy.is_null() {
            return None;
        }

        Some(Self {
            create: unsafe { mem::transmute::<*mut c_void, CreateTap>(create) },
            destroy: unsafe { mem::transmute::<*mut c_void, DestroyTap>(destroy) },
        })
    }
}

pub struct DesktopTap {
    tap: u32,
    aggregate: Device,
    destroy: DestroyTap,
}

impl DesktopTap {
    pub fn open() -> Result<Self> {
        let tapping = Tapping::of_this_system().ok_or(Error::NoProcessTap)?;
        let output = Device::default_output()?.ok_or(Error::NoOutputToTap)?;
        let output_uid = output.uid()?;

        let described = unsafe {
            CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &NSArray::new(),
            )
        };
        unsafe {
            described.setName(&NSString::from_str(TAPPED_NAME));
            described.setPrivate(true);
        }
        let tap_uid = unsafe { described.UUID() }.UUIDString().to_string();

        let mut tap = 0;
        OsStatus::checked(unsafe { (tapping.create)(Retained::as_ptr(&described), &raw mut tap) })
            .map_err(Error::refused(Op::CreateTap))?;

        let description = aggregate_over(&output_uid, &tap_uid);
        let mut aggregate = 0;
        let made = OsStatus::checked(unsafe {
            AudioHardwareCreateAggregateDevice(as_cf(&description), NonNull::from(&mut aggregate))
        });
        if let Err(status) = made {
            unsafe { (tapping.destroy)(tap) };
            return Err(Error::refused(Op::CreateAggregate)(status));
        }

        Ok(Self {
            tap,
            aggregate: Device::of(aggregate),
            destroy: tapping.destroy,
        })
    }

    pub const fn device(&self) -> Device {
        self.aggregate
    }
}

impl Drop for DesktopTap {
    fn drop(&mut self) {
        unsafe { AudioHardwareDestroyAggregateDevice(self.aggregate.id()) };
        unsafe { (self.destroy)(self.tap) };
    }
}

fn aggregate_over(output_uid: &str, tap_uid: &str) -> Retained<NSDictionary<NSString, AnyObject>> {
    let sub_device = NSDictionary::from_retained_objects(
        &[&*NSString::from_str(SUB_DEVICE_UID)],
        &[object(NSString::from_str(output_uid))],
    );
    let sub_tap = NSDictionary::from_retained_objects(
        &[
            &*NSString::from_str(SUB_TAP_UID),
            &*NSString::from_str(SUB_TAP_DRIFT_COMPENSATION),
        ],
        &[
            object(NSString::from_str(tap_uid)),
            object(NSNumber::new_bool(true)),
        ],
    );

    NSDictionary::from_retained_objects(
        &[
            &*NSString::from_str(AGGREGATE_UID),
            &*NSString::from_str(AGGREGATE_NAME),
            &*NSString::from_str(AGGREGATE_IS_PRIVATE),
            &*NSString::from_str(AGGREGATE_MAIN_SUB_DEVICE),
            &*NSString::from_str(AGGREGATE_SUB_DEVICES),
            &*NSString::from_str(AGGREGATE_TAPS),
            &*NSString::from_str(AGGREGATE_TAPS_START_THEMSELVES),
        ],
        &[
            object(NSUUID::new().UUIDString()),
            object(NSString::from_str(TAPPED_NAME)),
            object(NSNumber::new_bool(true)),
            object(NSString::from_str(output_uid)),
            object(NSArray::from_retained_slice(&[sub_device])),
            object(NSArray::from_retained_slice(&[sub_tap])),
            object(NSNumber::new_bool(true)),
        ],
    )
}

fn object<T: Message>(value: Retained<T>) -> Retained<AnyObject> {
    unsafe { Retained::cast_unchecked(value) }
}

fn as_cf(dictionary: &NSDictionary<NSString, AnyObject>) -> &CFDictionary {
    unsafe { &*ptr::from_ref(dictionary).cast::<CFDictionary>() }
}
