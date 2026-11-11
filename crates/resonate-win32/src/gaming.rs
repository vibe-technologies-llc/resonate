use std::sync::atomic::{AtomicBool, Ordering};

use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

const GAME_MODE: i32 = 5;

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub(crate) trait PowerModes {
    type Registration: Copy;

    fn register(&self) -> Raw<Self::Registration>;
    fn unregister(&self, registration: Self::Registration);
}

pub(crate) fn changed(mode: i32) {
    ACTIVE.store(mode == GAME_MODE, Ordering::Release);
}

pub(crate) struct Watching<T: PowerModes> {
    power: T,
    registration: T::Registration,
}

impl<T: PowerModes> Watching<T> {
    pub(crate) fn start(power: T) -> Result<Self> {
        let registration = power.register().map_err(refused(Op::WatchGameMode))?;
        Ok(Self {
            power,
            registration,
        })
    }

    pub(crate) fn active(&self) -> bool {
        ACTIVE.load(Ordering::Acquire)
    }
}

impl<T: PowerModes> Drop for Watching<T> {
    fn drop(&mut self) {
        self.power.unregister(self.registration);
    }
}

#[cfg(windows)]
pub struct GameModeWatch(Watching<Windows>);

#[cfg(windows)]
impl GameModeWatch {
    pub fn start() -> Result<Self> {
        Watching::start(Windows).map(Self)
    }

    pub fn active(&self) -> bool {
        self.0.active()
    }
}

#[cfg(windows)]
mod real {
    use std::{ffi::c_void, ptr};

    use windows::Win32::System::Power::{
        EFFECTIVE_POWER_MODE, EFFECTIVE_POWER_MODE_V2,
        PowerRegisterForEffectivePowerModeNotifications,
        PowerUnregisterFromEffectivePowerModeNotifications,
    };

    use super::{PowerModes, changed};
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

    unsafe extern "system" fn told(mode: EFFECTIVE_POWER_MODE, _: *const c_void) {
        changed(mode.0);
    }

    impl PowerModes for Windows {
        type Registration = *mut c_void;

        fn register(&self) -> Raw<*mut c_void> {
            let mut handle = ptr::null_mut();
            unsafe {
                PowerRegisterForEffectivePowerModeNotifications(
                    EFFECTIVE_POWER_MODE_V2,
                    Some(told),
                    None,
                    &mut handle,
                )
            }
            .map_err(hresult)?;
            Ok(handle)
        }

        fn unregister(&self, registration: *mut c_void) {
            let _ = unsafe { PowerUnregisterFromEffectivePowerModeNotifications(registration) };
        }
    }

    #[cfg(test)]
    mod tests {
        use windows::Win32::System::Power::EffectivePowerModeGameMode;

        use super::super::GAME_MODE;

        #[test]
        fn the_game_mode_constant_is_the_systems() {
            assert_eq!(GAME_MODE, EffectivePowerModeGameMode.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    static SERIAL: Mutex<()> = Mutex::new(());

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Register,
        Unregister(u8),
    }

    #[derive(Clone, Default)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        refusing: bool,
    }

    impl Fake {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }

    impl PowerModes for Fake {
        type Registration = u8;

        fn register(&self) -> Raw<u8> {
            self.calls.lock().push(Call::Register);
            if self.refusing {
                return Err(HResult(-7));
            }
            Ok(9)
        }

        fn unregister(&self, registration: u8) {
            self.calls.lock().push(Call::Unregister(registration));
        }
    }

    #[test]
    fn a_refused_registration_is_told_and_has_nothing_to_unregister() {
        let fake = Fake {
            refusing: true,
            ..Fake::default()
        };

        let watch = Watching::start(fake.clone());

        assert!(matches!(
            watch,
            Err(Error::Windows {
                op: Op::WatchGameMode,
                source: HResult(-7)
            })
        ));
        assert_eq!(fake.calls(), [Call::Register]);
    }

    #[test]
    fn a_watch_unregisters_its_registration_once_when_dropped() {
        let _serial = SERIAL.lock();
        let fake = Fake::default();

        drop(Watching::start(fake.clone()).expect("a watch"));

        assert_eq!(fake.calls(), [Call::Register, Call::Unregister(9)]);
    }

    #[test]
    fn the_watch_reads_game_mode_from_what_the_system_tells_it() {
        let _serial = SERIAL.lock();
        let watch = Watching::start(Fake::default()).expect("a watch");

        changed(GAME_MODE);
        let during = watch.active();
        changed(2);
        let after = watch.active();

        assert!(during);
        assert!(!after);
    }
}
