use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use windows::Win32::System::Power::{
    EFFECTIVE_POWER_MODE, EFFECTIVE_POWER_MODE_V2, EffectivePowerModeGameMode,
    PowerRegisterForEffectivePowerModeNotifications,
    PowerUnregisterFromEffectivePowerModeNotifications,
};

use crate::{Op, Result, error::refused};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct GameModeWatch(*mut c_void);

impl GameModeWatch {
    pub fn start() -> Result<Self> {
        let mut handle = ptr::null_mut();
        unsafe {
            PowerRegisterForEffectivePowerModeNotifications(
                EFFECTIVE_POWER_MODE_V2,
                Some(changed),
                None,
                &mut handle,
            )
        }
        .map_err(refused(Op::WatchGameMode))?;
        Ok(Self(handle))
    }

    pub fn active(&self) -> bool {
        ACTIVE.load(Ordering::Acquire)
    }
}

impl Drop for GameModeWatch {
    fn drop(&mut self) {
        let _ = unsafe { PowerUnregisterFromEffectivePowerModeNotifications(self.0) };
    }
}

unsafe extern "system" fn changed(mode: EFFECTIVE_POWER_MODE, _: *const c_void) {
    ACTIVE.store(mode == EffectivePowerModeGameMode, Ordering::Release);
}
