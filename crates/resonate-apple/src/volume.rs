use objc2_core_audio::{
    kAudioDevicePropertyMute, kAudioDevicePropertyPreferredChannelsForStereo,
    kAudioDevicePropertyVolumeScalar, kAudioObjectPropertyElementMain,
};

use crate::{
    device::Device,
    error::{Op, Result},
    property::{self, Address, Direction, Plain},
};

const LEFT_AND_RIGHT_BY_DEFAULT: [u32; 2] = [1, 2];

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Scalar(f32);

impl Scalar {
    pub fn new(position: f32) -> Self {
        Self(if position.is_nan() {
            0.0
        } else {
            position.clamp(0.0, 1.0)
        })
    }

    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Elements {
    Main,
    Pair([u32; 2]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Control {
    device: Device,
    selector: u32,
    elements: Elements,
}

impl Control {
    fn of(device: Device, selector: u32) -> Option<Self> {
        let main = Address::towards(Direction::Output, selector);
        if property::has(device.id(), main) {
            return Some(Self {
                device,
                selector,
                elements: Elements::Main,
            });
        }
        let preferred = Address::towards(
            Direction::Output,
            kAudioDevicePropertyPreferredChannelsForStereo,
        );
        let pair = property::read::<[u32; 2]>(device.id(), preferred, Op::ReadVolume)
            .unwrap_or(LEFT_AND_RIGHT_BY_DEFAULT);
        pair.iter()
            .all(|channel| property::has(device.id(), main.element(*channel)))
            .then_some(Self {
                device,
                selector,
                elements: Elements::Pair(pair),
            })
    }

    fn places(&self) -> impl Iterator<Item = Address> {
        let main = Address::towards(Direction::Output, self.selector);
        let (first, second) = match self.elements {
            Elements::Main => (main.element(kAudioObjectPropertyElementMain), None),
            Elements::Pair([left, right]) => (main.element(left), Some(main.element(right))),
        };
        [Some(first), second].into_iter().flatten()
    }

    fn is_settable(&self) -> bool {
        self.places()
            .all(|place| property::settable(self.device.id(), place))
    }

    fn read<T: Plain>(&self, op: Op) -> Result<Vec<T>> {
        self.places()
            .map(|place| property::read(self.device.id(), place, op))
            .collect()
    }

    fn write<T: Plain>(&self, value: &T, op: Op) -> Result<()> {
        self.places()
            .try_for_each(|place| property::write(self.device.id(), place, value, op))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume(Control);

impl Volume {
    pub fn of(device: Device) -> Option<Self> {
        Control::of(device, kAudioDevicePropertyVolumeScalar).map(Self)
    }

    pub fn is_settable(&self) -> bool {
        self.0.is_settable()
    }

    pub fn level(&self) -> Result<Scalar> {
        let levels = self.0.read::<f32>(Op::ReadVolume)?;
        Ok(Scalar::new(levels.into_iter().fold(0.0, f32::max)))
    }

    pub fn set_level(&self, level: Scalar) -> Result<()> {
        self.0.write(&level.get(), Op::SetVolume)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mute(Control);

impl Mute {
    pub fn of(device: Device) -> Option<Self> {
        Control::of(device, kAudioDevicePropertyMute).map(Self)
    }

    pub fn is_muted(&self) -> Result<bool> {
        let muted = self.0.read::<u32>(Op::ReadMute)?;
        Ok(!muted.is_empty() && muted.into_iter().all(|muted| muted != 0))
    }

    pub fn set_muted(&self, muted: bool) -> Result<()> {
        self.0.write(&u32::from(muted), Op::SetMute)
    }
}
