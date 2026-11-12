use objc2_core_audio::{
    AudioStreamRangedDescription, kAudioDevicePropertyAvailableNominalSampleRates,
    kAudioDevicePropertyBufferFrameSize, kAudioDevicePropertyBufferFrameSizeRange,
    kAudioDevicePropertyDeviceIsAlive, kAudioDevicePropertyDeviceUID, kAudioDevicePropertyHogMode,
    kAudioDevicePropertyJackIsConnected, kAudioDevicePropertyLatency,
    kAudioDevicePropertyNominalSampleRate, kAudioDevicePropertySafetyOffset,
    kAudioDevicePropertyStreams, kAudioDevicePropertyTransportType, kAudioDeviceTransportTypeAVB,
    kAudioDeviceTransportTypeAggregate, kAudioDeviceTransportTypeAirPlay,
    kAudioDeviceTransportTypeBluetooth, kAudioDeviceTransportTypeBluetoothLE,
    kAudioDeviceTransportTypeBuiltIn, kAudioDeviceTransportTypeDisplayPort,
    kAudioDeviceTransportTypeFireWire, kAudioDeviceTransportTypeHDMI, kAudioDeviceTransportTypePCI,
    kAudioDeviceTransportTypeThunderbolt, kAudioDeviceTransportTypeUSB,
    kAudioDeviceTransportTypeVirtual, kAudioHardwarePropertyDefaultInputDevice,
    kAudioHardwarePropertyDefaultOutputDevice, kAudioHardwarePropertyDevices,
    kAudioObjectPropertyName, kAudioObjectSystemObject,
    kAudioStreamPropertyAvailablePhysicalFormats, kAudioStreamPropertyLatency,
    kAudioStreamPropertyPhysicalFormat,
};
use objc2_core_audio_types::{AudioStreamBasicDescription, AudioValueRange};

use crate::{
    error::{Error, Op, Result},
    hog::Pid,
    physical::PhysicalFormat,
    property::{self, Address, Direction},
};

pub(crate) const THE_SYSTEM: u32 = kAudioObjectSystemObject as u32;
const NO_DEVICE: u32 = 0;
const AN_AGGREGATE_THE_SYSTEM_MADE: u32 = u32::from_be_bytes(*b"fgrp");
const HELD_BY_NOBODY: i32 = -1;
const SUPPORTS_MIXING: u32 = u32::from_be_bytes(*b"mix?");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Device(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Stream(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
    BuiltIn,
    Usb,
    Bluetooth,
    Hdmi,
    DisplayPort,
    AirPlay,
    Thunderbolt,
    FireWire,
    Pci,
    Avb,
    Virtual,
    Aggregate,
    Unknown,
}

const TRANSPORTS: [(u32, Transport); 14] = [
    (kAudioDeviceTransportTypeBuiltIn, Transport::BuiltIn),
    (kAudioDeviceTransportTypeUSB, Transport::Usb),
    (kAudioDeviceTransportTypeBluetooth, Transport::Bluetooth),
    (kAudioDeviceTransportTypeBluetoothLE, Transport::Bluetooth),
    (kAudioDeviceTransportTypeHDMI, Transport::Hdmi),
    (kAudioDeviceTransportTypeDisplayPort, Transport::DisplayPort),
    (kAudioDeviceTransportTypeAirPlay, Transport::AirPlay),
    (kAudioDeviceTransportTypeThunderbolt, Transport::Thunderbolt),
    (kAudioDeviceTransportTypeFireWire, Transport::FireWire),
    (kAudioDeviceTransportTypePCI, Transport::Pci),
    (kAudioDeviceTransportTypeAVB, Transport::Avb),
    (kAudioDeviceTransportTypeVirtual, Transport::Virtual),
    (kAudioDeviceTransportTypeAggregate, Transport::Aggregate),
    (AN_AGGREGATE_THE_SYSTEM_MADE, Transport::Aggregate),
];

impl Transport {
    fn of(code: u32) -> Self {
        TRANSPORTS
            .iter()
            .find(|(named, _)| *named == code)
            .map_or(Self::Unknown, |(_, transport)| *transport)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RateRange {
    pub lowest: f64,
    pub highest: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramesRange {
    pub fewest: u32,
    pub most: u32,
}

impl FramesRange {
    fn of(range: AudioValueRange) -> Self {
        let frames = |bound: f64| {
            if bound.is_finite() && bound >= 0.0 {
                bound.min(f64::from(u32::MAX)).round() as u32
            } else {
                0
            }
        };
        Self {
            fewest: frames(range.mMinimum),
            most: frames(range.mMaximum),
        }
    }

    pub fn held_to(self, frames: u32) -> u32 {
        frames.clamp(self.fewest, self.most.max(self.fewest))
    }
}

impl RateRange {
    const fn of(range: AudioValueRange) -> Self {
        Self {
            lowest: range.mMinimum,
            highest: range.mMaximum,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OfferedFormat {
    pub format: PhysicalFormat,
    pub rates: RateRange,
}

impl Device {
    pub fn all() -> Result<Vec<Self>> {
        let devices = Address::global(kAudioHardwarePropertyDevices);
        let ids = property::read_all::<u32>(THE_SYSTEM, devices, Op::FindDevices)?;
        Ok(ids.into_iter().map(Self).collect())
    }

    fn default_of(selector: u32, op: Op) -> Result<Option<Self>> {
        let id = property::read::<u32>(THE_SYSTEM, Address::global(selector), op)?;
        Ok((id != NO_DEVICE).then_some(Self(id)))
    }

    pub fn default_output() -> Result<Option<Self>> {
        Self::default_of(
            kAudioHardwarePropertyDefaultOutputDevice,
            Op::ReadDefaultOutput,
        )
    }

    pub fn default_input() -> Result<Option<Self>> {
        Self::default_of(
            kAudioHardwarePropertyDefaultInputDevice,
            Op::ReadDefaultInput,
        )
    }

    pub(crate) const fn of(id: u32) -> Self {
        Self(id)
    }

    pub const fn id(self) -> u32 {
        self.0
    }

    pub fn uid(self) -> Result<String> {
        let uid = Address::global(kAudioDevicePropertyDeviceUID);
        property::read_text(self.0, uid, Op::ReadUid)
    }

    pub fn name(self) -> Result<String> {
        let name = Address::global(kAudioObjectPropertyName);
        property::read_text(self.0, name, Op::ReadName)
    }

    pub fn transport(self) -> Transport {
        let transport = Address::global(kAudioDevicePropertyTransportType);
        property::read::<u32>(self.0, transport, Op::ReadTransport)
            .map_or(Transport::Unknown, Transport::of)
    }

    pub fn is_alive(self) -> bool {
        let alive = Address::global(kAudioDevicePropertyDeviceIsAlive);
        property::read::<u32>(self.0, alive, Op::ReadAlive).is_ok_and(|alive| alive != 0)
    }

    pub fn streams(self, direction: Direction) -> Result<Vec<Stream>> {
        let streams = Address::towards(direction, kAudioDevicePropertyStreams);
        let ids = property::read_all::<u32>(self.0, streams, Op::ReadStreams)?;
        Ok(ids.into_iter().map(Stream).collect())
    }

    pub fn nominal_rate(self) -> Result<f64> {
        let rate = Address::global(kAudioDevicePropertyNominalSampleRate);
        property::read(self.0, rate, Op::ReadRate)
    }

    pub fn set_nominal_rate(self, hz: f64) -> Result<()> {
        let rate = Address::global(kAudioDevicePropertyNominalSampleRate);
        property::write(self.0, rate, &hz, Op::SetRate)
    }

    pub fn nominal_rates(self) -> Result<Vec<RateRange>> {
        let rates = Address::global(kAudioDevicePropertyAvailableNominalSampleRates);
        let ranges = property::read_all::<AudioValueRange>(self.0, rates, Op::ReadRates)?;
        Ok(ranges.into_iter().map(RateRange::of).collect())
    }

    pub fn latency(self, direction: Direction) -> Result<u32> {
        let latency = Address::towards(direction, kAudioDevicePropertyLatency);
        property::read(self.0, latency, Op::ReadLatency)
    }

    pub fn safety_offset(self, direction: Direction) -> Result<u32> {
        let offset = Address::towards(direction, kAudioDevicePropertySafetyOffset);
        property::read(self.0, offset, Op::ReadLatency)
    }

    pub fn buffer_frames(self) -> Result<u32> {
        let frames = Address::global(kAudioDevicePropertyBufferFrameSize);
        property::read(self.0, frames, Op::ReadLatency)
    }

    pub fn buffer_frames_allowed(self) -> Result<FramesRange> {
        let range = Address::global(kAudioDevicePropertyBufferFrameSizeRange);
        property::read::<AudioValueRange>(self.0, range, Op::ReadBufferFrames).map(FramesRange::of)
    }

    pub fn set_buffer_frames(self, frames: u32) -> Result<()> {
        let size = Address::global(kAudioDevicePropertyBufferFrameSize);
        property::write(self.0, size, &frames, Op::SetBufferFrames)
    }

    pub fn mixes(self) -> Option<bool> {
        let mixing = Address::global(SUPPORTS_MIXING);
        property::read::<u32>(self.0, mixing, Op::ReadMixing)
            .ok()
            .map(|mixes| mixes != 0)
    }

    pub fn set_mixing(self, mixes: bool) -> Result<()> {
        let mixing = Address::global(SUPPORTS_MIXING);
        if !property::settable(self.0, mixing) {
            return Err(Error::Unsettable { op: Op::SetMixing });
        }
        property::write(self.0, mixing, &u32::from(mixes), Op::SetMixing)
    }

    pub fn jack_connected(self, direction: Direction) -> Option<bool> {
        let jack = Address::towards(direction, kAudioDevicePropertyJackIsConnected);
        property::read::<u32>(self.0, jack, Op::ReadJack)
            .ok()
            .map(|connected| connected != 0)
    }

    pub fn hogged_by(self) -> Result<Option<Pid>> {
        let hog = Address::global(kAudioDevicePropertyHogMode);
        if !property::has(self.0, hog) {
            return Ok(None);
        }
        let holder = property::read::<i32>(self.0, hog, Op::ReadHog)?;
        Ok((holder != HELD_BY_NOBODY).then_some(Pid::of(holder)))
    }

    pub(crate) fn toggle_the_hog(self) -> Result<()> {
        let hog = Address::global(kAudioDevicePropertyHogMode);
        property::write(self.0, hog, &Pid::of_this_process().get(), Op::TakeHog)
    }
}

impl Stream {
    pub fn physical_format(self) -> Result<PhysicalFormat> {
        let format = Address::global(kAudioStreamPropertyPhysicalFormat);
        property::read::<AudioStreamBasicDescription>(self.0, format, Op::ReadPhysicalFormat)
            .map(PhysicalFormat::of)
    }

    pub fn set_physical_format(self, format: &PhysicalFormat) -> Result<()> {
        let physical = Address::global(kAudioStreamPropertyPhysicalFormat);
        property::write(self.0, physical, format.described(), Op::SetPhysicalFormat)
    }

    pub fn physical_formats(self) -> Result<Vec<OfferedFormat>> {
        let formats = Address::global(kAudioStreamPropertyAvailablePhysicalFormats);
        let offered = property::read_all::<AudioStreamRangedDescription>(
            self.0,
            formats,
            Op::ReadPhysicalFormats,
        )?;
        Ok(offered
            .into_iter()
            .map(|offered| OfferedFormat {
                format: PhysicalFormat::of(offered.mFormat),
                rates: RateRange::of(offered.mSampleRateRange),
            })
            .collect())
    }

    pub fn latency(self) -> Result<u32> {
        let latency = Address::global(kAudioStreamPropertyLatency);
        property::read(self.0, latency, Op::ReadLatency)
    }
}
