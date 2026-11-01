use std::ptr;

use windows::Win32::{
    Media::Audio::{
        AUDIO_VOLUME_NOTIFICATION_DATA,
        Endpoints::{
            IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
        },
        IMMDeviceEnumerator, MMDeviceEnumerator,
    },
    System::Com::{CLSCTX_ALL, CoCreateInstance},
};
use windows_core::{HSTRING, implement};

use crate::error::{Op, Result, refused};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Level(f32);

impl Level {
    pub const SILENT: Self = Self(0.0);

    pub fn new(scalar: f32) -> Self {
        if scalar.is_nan() {
            return Self::SILENT;
        }
        Self(scalar.clamp(0.0, 1.0))
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

pub struct EndpointVolume {
    volume: IAudioEndpointVolume,
}

impl EndpointVolume {
    pub fn of(endpoint: &str) -> Result<Self> {
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(refused(Op::ReachEndpoints))?;
        let id = HSTRING::from(endpoint);
        let device = unsafe { enumerator.GetDevice(&id) }.map_err(refused(Op::FindEndpoint))?;
        let volume = unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }
            .map_err(refused(Op::ActivateEndpointVolume))?;
        Ok(Self { volume })
    }

    pub fn level(&self) -> Result<Level> {
        let scalar =
            unsafe { self.volume.GetMasterVolumeLevelScalar() }.map_err(refused(Op::ReadLevel))?;
        Ok(Level::new(scalar))
    }

    pub fn set_level(&self, level: Level) -> Result<()> {
        unsafe {
            self.volume
                .SetMasterVolumeLevelScalar(level.get(), ptr::null())
        }
        .map_err(refused(Op::SetLevel))
    }

    pub fn is_muted(&self) -> Result<bool> {
        let muted = unsafe { self.volume.GetMute() }.map_err(refused(Op::ReadMute))?;
        Ok(muted.as_bool())
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        unsafe { self.volume.SetMute(muted, ptr::null()) }.map_err(refused(Op::SetMute))
    }

    pub fn watched(self, nudge: impl Fn() + Send + Sync + 'static) -> Result<VolumeWatch> {
        let callback: IAudioEndpointVolumeCallback = Nudging {
            nudge: Box::new(nudge),
        }
        .into();
        unsafe { self.volume.RegisterControlChangeNotify(&callback) }
            .map_err(refused(Op::WatchVolume))?;
        Ok(VolumeWatch {
            volume: self.volume,
            callback,
        })
    }
}

pub struct VolumeWatch {
    volume: IAudioEndpointVolume,
    callback: IAudioEndpointVolumeCallback,
}

impl Drop for VolumeWatch {
    fn drop(&mut self) {
        let _ = unsafe { self.volume.UnregisterControlChangeNotify(&self.callback) };
    }
}

type Nudge = Box<dyn Fn() + Send + Sync>;

#[implement(IAudioEndpointVolumeCallback)]
struct Nudging {
    nudge: Nudge,
}

impl IAudioEndpointVolumeCallback_Impl for Nudging_Impl {
    fn OnNotify(&self, _: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows_core::Result<()> {
        (self.nudge)();
        Ok(())
    }
}
