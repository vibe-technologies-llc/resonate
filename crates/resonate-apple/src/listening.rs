use std::{
    collections::BTreeMap,
    ffi::c_void,
    panic::{self, AssertUnwindSafe},
    ptr::{self, NonNull},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use objc2_core_audio::{
    AudioObjectAddPropertyListener, AudioObjectPropertyAddress, AudioObjectRemovePropertyListener,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyHogMode, kAudioDevicePropertyMute,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertyVolumeScalar,
    kAudioHardwarePropertyDefaultInputDevice, kAudioHardwarePropertyDefaultOutputDevice,
    kAudioHardwarePropertyDevices, kAudioHardwarePropertyServiceRestarted,
    kAudioObjectPropertyElementWildcard,
};
use parking_lot::Mutex;

use crate::{
    device::{Device, THE_SYSTEM},
    error::{Error, Op, Result},
    property::{self, Address, Direction},
    status::OsStatus,
};

const THE_RUN_LOOP_NOTIFICATIONS_ARRIVE_ON: u32 = u32::from_be_bytes(*b"rnlp");
const TOLD: i32 = 0;

type Nudge = Arc<dyn Fn() + Send + Sync>;

static NUDGES: Mutex<BTreeMap<usize, Nudge>> = Mutex::new(BTreeMap::new());
static NEXT_TOKEN: AtomicUsize = AtomicUsize::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Told {
    Devices,
    DefaultOutput,
    DefaultInput,
    ServiceRestarted,
    Rate,
    Alive,
    Hog,
    Volume,
    Mute,
}

impl Told {
    const fn address(self) -> Address {
        match self {
            Self::Devices => Address::global(kAudioHardwarePropertyDevices),
            Self::DefaultOutput => Address::global(kAudioHardwarePropertyDefaultOutputDevice),
            Self::DefaultInput => Address::global(kAudioHardwarePropertyDefaultInputDevice),
            Self::ServiceRestarted => Address::global(kAudioHardwarePropertyServiceRestarted),
            Self::Rate => Address::global(kAudioDevicePropertyNominalSampleRate),
            Self::Alive => Address::global(kAudioDevicePropertyDeviceIsAlive),
            Self::Hog => Address::global(kAudioDevicePropertyHogMode),
            Self::Volume => Address::towards(Direction::Output, kAudioDevicePropertyVolumeScalar)
                .element(kAudioObjectPropertyElementWildcard),
            Self::Mute => Address::towards(Direction::Output, kAudioDevicePropertyMute)
                .element(kAudioObjectPropertyElementWildcard),
        }
    }
}

unsafe extern "C-unwind" fn told(
    _: u32,
    _: u32,
    _: NonNull<AudioObjectPropertyAddress>,
    token: *mut c_void,
) -> i32 {
    let nudge = NUDGES.lock().get(&token.addr()).cloned();
    if let Some(nudge) = nudge {
        let _ = panic::catch_unwind(AssertUnwindSafe(|| nudge()));
    }
    TOLD
}

#[derive(Debug)]
pub struct Listening {
    object: u32,
    address: Address,
    token: usize,
}

impl Listening {
    fn to(object: u32, what: Told, nudge: Nudge) -> Result<Self> {
        let address = what.address();
        let token = NEXT_TOKEN.fetch_add(1, Ordering::Relaxed);
        NUDGES.lock().insert(token, nudge);
        let status = unsafe {
            AudioObjectAddPropertyListener(
                object,
                NonNull::from(address.raw()),
                Some(told),
                ptr::without_provenance_mut(token),
            )
        };
        match OsStatus::checked(status) {
            Ok(()) => Ok(Self {
                object,
                address,
                token,
            }),
            Err(source) => {
                NUDGES.lock().remove(&token);
                Err(Error::Refused {
                    op: Op::Listen,
                    source,
                })
            }
        }
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        unsafe {
            AudioObjectRemovePropertyListener(
                self.object,
                NonNull::from(self.address.raw()),
                Some(told),
                ptr::without_provenance_mut(self.token),
            )
        };
        NUDGES.lock().remove(&self.token);
    }
}

pub fn told_of_the_system(
    what: Told,
    nudge: impl Fn() + Send + Sync + 'static,
) -> Result<Listening> {
    Listening::to(THE_SYSTEM, what, Arc::new(nudge))
}

impl Device {
    pub fn told_of(
        self,
        what: Told,
        nudge: impl Fn() + Send + Sync + 'static,
    ) -> Result<Listening> {
        Listening::to(self.id(), what, Arc::new(nudge))
    }
}

pub fn notify_on_the_hals_own_thread() -> Result<()> {
    let run_loop = Address::global(THE_RUN_LOOP_NOTIFICATIONS_ARRIVE_ON);
    property::write_no_pointer(THE_SYSTEM, run_loop, Op::NotifyOnOwnThread)
}
