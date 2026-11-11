use crate::error::{Op, Raw, Result, refused};
#[cfg(windows)]
use crate::system::Windows;

pub(crate) type Nudge = Box<dyn Fn() + Send + Sync>;

pub(crate) trait Endpoints {
    type Enumerator;
    type Device;
    type Volume;
    type Callback;

    fn enumerator(&self) -> Raw<Self::Enumerator>;
    fn device(&self, enumerator: &Self::Enumerator, endpoint: &str) -> Raw<Self::Device>;
    fn volume(&self, device: &Self::Device) -> Raw<Self::Volume>;
    fn level(&self, volume: &Self::Volume) -> Raw<f32>;
    fn set_level(&self, volume: &Self::Volume, scalar: f32) -> Raw<()>;
    fn muted(&self, volume: &Self::Volume) -> Raw<bool>;
    fn set_muted(&self, volume: &Self::Volume, muted: bool) -> Raw<()>;
    fn watch(&self, volume: &Self::Volume, nudge: Nudge) -> Raw<Self::Callback>;
    fn unwatch(&self, volume: &Self::Volume, callback: &Self::Callback) -> Raw<()>;
}

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

pub(crate) struct Opened<T: Endpoints> {
    endpoints: T,
    volume: T::Volume,
}

impl<T: Endpoints> Opened<T> {
    pub(crate) fn of(endpoints: T, endpoint: &str) -> Result<Self> {
        let enumerator = endpoints
            .enumerator()
            .map_err(refused(Op::ReachEndpoints))?;
        let device = endpoints
            .device(&enumerator, endpoint)
            .map_err(refused(Op::FindEndpoint))?;
        let volume = endpoints
            .volume(&device)
            .map_err(refused(Op::ActivateEndpointVolume))?;
        Ok(Self { endpoints, volume })
    }

    pub(crate) fn level(&self) -> Result<Level> {
        let scalar = self
            .endpoints
            .level(&self.volume)
            .map_err(refused(Op::ReadLevel))?;
        Ok(Level::new(scalar))
    }

    pub(crate) fn set_level(&self, level: Level) -> Result<()> {
        self.endpoints
            .set_level(&self.volume, level.get())
            .map_err(refused(Op::SetLevel))
    }

    pub(crate) fn is_muted(&self) -> Result<bool> {
        self.endpoints
            .muted(&self.volume)
            .map_err(refused(Op::ReadMute))
    }

    pub(crate) fn set_muted(&self, muted: bool) -> Result<()> {
        self.endpoints
            .set_muted(&self.volume, muted)
            .map_err(refused(Op::SetMute))
    }

    pub(crate) fn watched(self, nudge: Nudge) -> Result<Watched<T>> {
        let callback = self
            .endpoints
            .watch(&self.volume, nudge)
            .map_err(refused(Op::WatchVolume))?;
        Ok(Watched {
            endpoints: self.endpoints,
            volume: self.volume,
            callback,
        })
    }
}

pub(crate) struct Watched<T: Endpoints> {
    endpoints: T,
    volume: T::Volume,
    callback: T::Callback,
}

impl<T: Endpoints> Drop for Watched<T> {
    fn drop(&mut self) {
        let _ = self.endpoints.unwatch(&self.volume, &self.callback);
    }
}

#[cfg(windows)]
pub struct EndpointVolume(Opened<Windows>);

#[cfg(windows)]
impl EndpointVolume {
    pub fn of(endpoint: &str) -> Result<Self> {
        Opened::of(Windows, endpoint).map(Self)
    }

    pub fn level(&self) -> Result<Level> {
        self.0.level()
    }

    pub fn set_level(&self, level: Level) -> Result<()> {
        self.0.set_level(level)
    }

    pub fn is_muted(&self) -> Result<bool> {
        self.0.is_muted()
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.0.set_muted(muted)
    }

    pub fn watched(self, nudge: impl Fn() + Send + Sync + 'static) -> Result<VolumeWatch> {
        self.0
            .watched(Box::new(nudge))
            .map(|registered| VolumeWatch {
                _registered: registered,
            })
    }
}

#[cfg(windows)]
pub struct VolumeWatch {
    _registered: Watched<Windows>,
}

#[cfg(windows)]
mod real {
    use std::ptr;

    use windows::Win32::{
        Media::Audio::{
            AUDIO_VOLUME_NOTIFICATION_DATA,
            Endpoints::{
                IAudioEndpointVolume, IAudioEndpointVolumeCallback,
                IAudioEndpointVolumeCallback_Impl,
            },
            IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        },
        System::Com::{CLSCTX_ALL, CoCreateInstance},
    };
    use windows_core::{HSTRING, implement};

    use super::{Endpoints, Nudge};
    use crate::{
        error::Raw,
        system::{Windows, hresult},
    };

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

    impl Endpoints for Windows {
        type Enumerator = IMMDeviceEnumerator;
        type Device = IMMDevice;
        type Volume = IAudioEndpointVolume;
        type Callback = IAudioEndpointVolumeCallback;

        fn enumerator(&self) -> Raw<IMMDeviceEnumerator> {
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(hresult)
        }

        fn device(&self, enumerator: &IMMDeviceEnumerator, endpoint: &str) -> Raw<IMMDevice> {
            let id = HSTRING::from(endpoint);
            unsafe { enumerator.GetDevice(&id) }.map_err(hresult)
        }

        fn volume(&self, device: &IMMDevice) -> Raw<IAudioEndpointVolume> {
            unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.map_err(hresult)
        }

        fn level(&self, volume: &IAudioEndpointVolume) -> Raw<f32> {
            unsafe { volume.GetMasterVolumeLevelScalar() }.map_err(hresult)
        }

        fn set_level(&self, volume: &IAudioEndpointVolume, scalar: f32) -> Raw<()> {
            unsafe { volume.SetMasterVolumeLevelScalar(scalar, ptr::null()) }.map_err(hresult)
        }

        fn muted(&self, volume: &IAudioEndpointVolume) -> Raw<bool> {
            unsafe { volume.GetMute() }
                .map(|muted| muted.as_bool())
                .map_err(hresult)
        }

        fn set_muted(&self, volume: &IAudioEndpointVolume, muted: bool) -> Raw<()> {
            unsafe { volume.SetMute(muted, ptr::null()) }.map_err(hresult)
        }

        fn watch(
            &self,
            volume: &IAudioEndpointVolume,
            nudge: Nudge,
        ) -> Raw<IAudioEndpointVolumeCallback> {
            let callback: IAudioEndpointVolumeCallback = Nudging { nudge }.into();
            unsafe { volume.RegisterControlChangeNotify(&callback) }.map_err(hresult)?;
            Ok(callback)
        }

        fn unwatch(
            &self,
            volume: &IAudioEndpointVolume,
            callback: &IAudioEndpointVolumeCallback,
        ) -> Raw<()> {
            unsafe { volume.UnregisterControlChangeNotify(callback) }.map_err(hresult)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;

    use super::*;
    use crate::error::{Error, HResult};

    const CO_E_NOTINITIALIZED: HResult = HResult(0x8004_01F0_u32.cast_signed());

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Object {
        Enumerator,
        Device,
        Volume,
        Callback,
    }

    #[derive(Clone, PartialEq, Debug)]
    enum Call {
        Enumerator,
        Device(String),
        Volume,
        Level,
        SetLevel(f32),
        Muted,
        SetMuted(bool),
        Watch,
        Unwatch,
        Released(Object),
    }

    type Calls = Arc<Mutex<Vec<Call>>>;

    struct Held {
        object: Object,
        calls: Calls,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            self.calls.lock().push(Call::Released(self.object));
        }
    }

    #[derive(Clone, Default)]
    struct Fake {
        calls: Calls,
        refusing: Option<&'static str>,
        scalar: f32,
        muted: bool,
        nudges: Arc<Mutex<Vec<Nudge>>>,
    }

    impl Fake {
        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }

        fn refuses(&self, what: &'static str) -> Raw<()> {
            if self.refusing == Some(what) {
                return Err(HResult(-11));
            }
            Ok(())
        }

        fn held(&self, object: Object) -> Held {
            Held {
                object,
                calls: Arc::clone(&self.calls),
            }
        }

        fn refusing(what: &'static str) -> Self {
            Self {
                refusing: Some(what),
                ..Self::default()
            }
        }
    }

    impl Endpoints for Fake {
        type Enumerator = Held;
        type Device = Held;
        type Volume = Held;
        type Callback = Held;

        fn enumerator(&self) -> Raw<Held> {
            self.calls.lock().push(Call::Enumerator);
            if self.refusing == Some("enumerator") {
                return Err(CO_E_NOTINITIALIZED);
            }
            Ok(self.held(Object::Enumerator))
        }

        fn device(&self, _: &Held, endpoint: &str) -> Raw<Held> {
            self.calls.lock().push(Call::Device(endpoint.to_owned()));
            self.refuses("device")?;
            Ok(self.held(Object::Device))
        }

        fn volume(&self, _: &Held) -> Raw<Held> {
            self.calls.lock().push(Call::Volume);
            self.refuses("volume")?;
            Ok(self.held(Object::Volume))
        }

        fn level(&self, _: &Held) -> Raw<f32> {
            self.calls.lock().push(Call::Level);
            self.refuses("level")?;
            Ok(self.scalar)
        }

        fn set_level(&self, _: &Held, scalar: f32) -> Raw<()> {
            self.calls.lock().push(Call::SetLevel(scalar));
            self.refuses("set_level")
        }

        fn muted(&self, _: &Held) -> Raw<bool> {
            self.calls.lock().push(Call::Muted);
            self.refuses("muted")?;
            Ok(self.muted)
        }

        fn set_muted(&self, _: &Held, muted: bool) -> Raw<()> {
            self.calls.lock().push(Call::SetMuted(muted));
            self.refuses("set_muted")
        }

        fn watch(&self, _: &Held, nudge: Nudge) -> Raw<Held> {
            self.calls.lock().push(Call::Watch);
            self.refuses("watch")?;
            self.nudges.lock().push(nudge);
            Ok(self.held(Object::Callback))
        }

        fn unwatch(&self, _: &Held, _: &Held) -> Raw<()> {
            self.calls.lock().push(Call::Unwatch);
            self.refuses("unwatch")
        }
    }

    #[test]
    fn a_thread_in_no_apartment_is_refused_at_the_enumerator() {
        let fake = Fake::refusing("enumerator");

        let opened = Opened::of(fake.clone(), "{endpoint}");

        assert!(matches!(
            opened,
            Err(Error::Windows {
                op: Op::ReachEndpoints,
                source: CO_E_NOTINITIALIZED
            })
        ));
        assert_eq!(fake.calls(), [Call::Enumerator]);
    }

    #[test]
    fn an_endpoint_that_is_not_found_releases_the_enumerator_and_names_the_step() {
        let fake = Fake::refusing("device");

        let opened = Opened::of(fake.clone(), "{gone}");

        assert!(matches!(
            opened,
            Err(Error::Windows {
                op: Op::FindEndpoint,
                ..
            })
        ));
        assert_eq!(
            fake.calls(),
            [
                Call::Enumerator,
                Call::Device("{gone}".into()),
                Call::Released(Object::Enumerator)
            ]
        );
    }

    #[test]
    fn a_volume_that_will_not_activate_releases_the_device_then_the_enumerator() {
        let fake = Fake::refusing("volume");

        let opened = Opened::of(fake.clone(), "{id}");

        assert!(matches!(
            opened,
            Err(Error::Windows {
                op: Op::ActivateEndpointVolume,
                ..
            })
        ));
        assert_eq!(
            fake.calls(),
            [
                Call::Enumerator,
                Call::Device("{id}".into()),
                Call::Volume,
                Call::Released(Object::Device),
                Call::Released(Object::Enumerator)
            ]
        );
    }

    #[test]
    fn an_opened_volume_keeps_only_the_volume_and_releases_it_once_dropped() {
        let fake = Fake::default();

        drop(Opened::of(fake.clone(), "{id}").expect("opened"));

        assert_eq!(
            fake.calls(),
            [
                Call::Enumerator,
                Call::Device("{id}".into()),
                Call::Volume,
                Call::Released(Object::Device),
                Call::Released(Object::Enumerator),
                Call::Released(Object::Volume)
            ]
        );
    }

    #[test]
    fn a_level_is_clamped_whatever_the_endpoint_answers() {
        for (answered, expected) in [(0.25, 0.25), (1.5, 1.0), (-0.5, 0.0), (f32::NAN, 0.0)] {
            let fake = Fake {
                scalar: answered,
                ..Fake::default()
            };
            let opened = Opened::of(fake, "{id}").expect("opened");

            assert_eq!(opened.level().expect("a level").get(), expected);
        }
    }

    #[test]
    fn a_level_is_set_as_given_and_mute_goes_both_ways() {
        let fake = Fake {
            muted: true,
            ..Fake::default()
        };
        let opened = Opened::of(fake.clone(), "{id}").expect("opened");
        fake.calls.lock().clear();

        opened.set_level(Level::new(0.5)).expect("set");
        let muted = opened.is_muted().expect("read");
        opened.set_muted(false).expect("unmuted");

        assert!(muted);
        assert_eq!(
            fake.calls(),
            [Call::SetLevel(0.5), Call::Muted, Call::SetMuted(false)]
        );
    }

    #[test]
    fn each_refused_call_names_itself() {
        for (what, op) in [
            ("level", Op::ReadLevel),
            ("set_level", Op::SetLevel),
            ("muted", Op::ReadMute),
            ("set_muted", Op::SetMute),
        ] {
            let opened = Opened::of(Fake::refusing(what), "{id}").expect("opened");

            let failed = match what {
                "level" => opened.level().err(),
                "set_level" => opened.set_level(Level::SILENT).err(),
                "muted" => opened.is_muted().err(),
                _ => opened.set_muted(true).err(),
            };

            assert!(
                matches!(failed, Some(Error::Windows { op: seen, .. }) if seen == op),
                "{what}"
            );
        }
    }

    #[test]
    fn a_refused_watch_releases_the_volume_and_the_watch_unregisters_once_when_dropped() {
        let refusing = Fake::refusing("watch");
        let opened = Opened::of(refusing.clone(), "{id}").expect("opened");
        refusing.calls.lock().clear();

        let refused = opened.watched(Box::new(|| {}));

        assert!(matches!(
            refused,
            Err(Error::Windows {
                op: Op::WatchVolume,
                ..
            })
        ));
        assert_eq!(
            refusing.calls(),
            [Call::Watch, Call::Released(Object::Volume)]
        );

        let fake = Fake::default();
        let watched = Opened::of(fake.clone(), "{id}")
            .expect("opened")
            .watched(Box::new(|| {}))
            .expect("watched");
        fake.calls.lock().clear();
        drop(watched);

        assert_eq!(
            fake.calls(),
            [
                Call::Unwatch,
                Call::Released(Object::Volume),
                Call::Released(Object::Callback)
            ]
        );
    }

    #[test]
    fn the_closure_handed_to_a_watch_is_the_one_the_endpoint_calls() {
        let fake = Fake::default();
        let told = Arc::new(Mutex::new(0_u32));
        let counting = Arc::clone(&told);

        let _watched = Opened::of(fake.clone(), "{id}")
            .expect("opened")
            .watched(Box::new(move || *counting.lock() += 1))
            .expect("watched");
        let nudge = fake.nudges.lock().pop().expect("a registered closure");
        nudge();
        nudge();

        assert_eq!(*told.lock(), 2);
    }
}
