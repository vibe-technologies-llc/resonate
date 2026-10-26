use std::{error, fmt, io, mem::size_of, ptr::NonNull};

use objc2_core_audio_types::{
    AudioStreamBasicDescription, kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM,
};
use resonate_audio::{Error, Operation, Result};
use resonate_core::SampleRate;

use super::{remote_io_fill::Rendering, remote_io_stream::Hardware};

const STEREO_CHANNELS: u32 = 2;
const FLOAT_BYTES: u32 = size_of::<f32>() as u32;

pub(crate) trait Toolbox {
    type Instance: Copy;

    fn has_component(&self) -> bool;
    fn instantiate(&self) -> std::result::Result<Self::Instance, i32>;
    fn set_format(&self, unit: Self::Instance, format: &AudioStreamBasicDescription) -> i32;
    fn set_render(&self, unit: Self::Instance, rendering: NonNull<Rendering>) -> i32;
    fn initialize(&self, unit: Self::Instance) -> i32;
    fn start(&self, unit: Self::Instance) -> i32;
    fn stop(&self, unit: Self::Instance) -> i32;
    fn uninitialize(&self, unit: Self::Instance) -> i32;
    fn dispose(&self, unit: Self::Instance) -> i32;
}

#[derive(Debug)]
struct Status(i32);

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "OSStatus {}", self.0)
    }
}

impl error::Error for Status {}

fn refused(op: Operation, status: i32) -> Error {
    Error::Backend {
        op,
        source: io::Error::other(Status(status)),
    }
}

fn checked(op: Operation, status: i32) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(refused(op, status))
    }
}

fn stream_format(rate: SampleRate) -> AudioStreamBasicDescription {
    AudioStreamBasicDescription {
        mSampleRate: f64::from(rate.hz()),
        mFormatID: kAudioFormatLinearPCM,
        mFormatFlags: kAudioFormatFlagsNativeFloatPacked,
        mBytesPerPacket: STEREO_CHANNELS * FLOAT_BYTES,
        mFramesPerPacket: 1,
        mBytesPerFrame: STEREO_CHANNELS * FLOAT_BYTES,
        mChannelsPerFrame: STEREO_CHANNELS,
        mBitsPerChannel: FLOAT_BYTES * 8,
        mReserved: 0,
    }
}

struct Leaked(NonNull<Rendering>);

impl Leaked {
    fn new(rendering: Box<Rendering>) -> Self {
        Self(NonNull::from(Box::leak(rendering)))
    }
}

impl Drop for Leaked {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

pub(crate) struct Unit<T: Toolbox> {
    toolbox: T,
    unit: T::Instance,
    rendering: Leaked,
}

impl<T: Toolbox> Unit<T> {
    pub(crate) fn opened(toolbox: T, rate: SampleRate, rendering: Box<Rendering>) -> Result<Self> {
        let rendering = Leaked::new(rendering);
        if !toolbox.has_component() {
            return Err(Error::Backend {
                op: Operation::Open,
                source: io::ErrorKind::NotFound.into(),
            });
        }
        let unit = toolbox
            .instantiate()
            .map_err(|status| refused(Operation::Open, status))?;
        let opened = Self {
            toolbox,
            unit,
            rendering,
        };
        checked(
            Operation::Open,
            opened.toolbox.set_format(unit, &stream_format(rate)),
        )?;
        checked(
            Operation::Open,
            opened.toolbox.set_render(unit, opened.rendering.0),
        )?;
        checked(Operation::Open, opened.toolbox.initialize(unit))?;
        Ok(opened)
    }
}

impl<T: Toolbox> Hardware for Unit<T> {
    fn start(&self) -> i32 {
        self.toolbox.start(self.unit)
    }

    fn stop(&self) -> i32 {
        self.toolbox.stop(self.unit)
    }

    fn reformat(&self, rate: SampleRate) -> i32 {
        let format = stream_format(rate);
        let steps: [&dyn Fn() -> i32; 4] = [
            &|| self.toolbox.stop(self.unit),
            &|| self.toolbox.uninitialize(self.unit),
            &|| self.toolbox.set_format(self.unit, &format),
            &|| self.toolbox.initialize(self.unit),
        ];
        steps
            .into_iter()
            .map(|step| step())
            .find(|status| *status != 0)
            .unwrap_or(0)
    }
}

impl<T: Toolbox> Drop for Unit<T> {
    fn drop(&mut self) {
        self.toolbox.stop(self.unit);
        self.toolbox.uninitialize(self.unit);
        self.toolbox.dispose(self.unit);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    use parking_lot::Mutex;
    use resonate_audio::AudioSource;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Instantiate,
        SetFormat,
        SetRender,
        Initialize,
        Start,
        Stop,
        Uninitialize,
        Dispose,
    }

    #[derive(Clone)]
    struct Fake {
        calls: Arc<Mutex<Vec<Call>>>,
        found: bool,
        refusing: Option<(Call, i32)>,
        format: Arc<Mutex<Option<AudioStreamBasicDescription>>>,
        rendering: Arc<Mutex<Option<NonNull<Rendering>>>>,
    }

    unsafe impl Send for Fake {}

    impl Fake {
        fn new() -> Self {
            Self {
                calls: Arc::default(),
                found: true,
                refusing: None,
                format: Arc::default(),
                rendering: Arc::default(),
            }
        }

        fn refusing(call: Call, status: i32) -> Self {
            Self {
                refusing: Some((call, status)),
                ..Self::new()
            }
        }

        fn answer(&self, call: Call) -> i32 {
            self.calls.lock().push(call);
            self.refusing
                .filter(|(refused, _)| *refused == call)
                .map_or(0, |(_, status)| status)
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().clone()
        }
    }

    impl Toolbox for Fake {
        type Instance = u8;

        fn has_component(&self) -> bool {
            self.found
        }

        fn instantiate(&self) -> std::result::Result<u8, i32> {
            match self.answer(Call::Instantiate) {
                0 => Ok(7),
                status => Err(status),
            }
        }

        fn set_format(&self, _: u8, format: &AudioStreamBasicDescription) -> i32 {
            *self.format.lock() = Some(*format);
            self.answer(Call::SetFormat)
        }

        fn set_render(&self, _: u8, rendering: NonNull<Rendering>) -> i32 {
            *self.rendering.lock() = Some(rendering);
            self.answer(Call::SetRender)
        }

        fn initialize(&self, _: u8) -> i32 {
            self.answer(Call::Initialize)
        }

        fn start(&self, _: u8) -> i32 {
            self.answer(Call::Start)
        }

        fn stop(&self, _: u8) -> i32 {
            self.answer(Call::Stop)
        }

        fn uninitialize(&self, _: u8) -> i32 {
            self.answer(Call::Uninitialize)
        }

        fn dispose(&self, _: u8) -> i32 {
            self.answer(Call::Dispose)
        }
    }

    struct Supply {
        dropped: Arc<AtomicUsize>,
        given: Arc<AtomicBool>,
    }

    impl AudioSource for Supply {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            self.given.store(true, Ordering::Release);
            dst.fill(0xCD);
            dst.len()
        }
    }

    impl Drop for Supply {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct Held {
        dropped: Arc<AtomicUsize>,
        given: Arc<AtomicBool>,
    }

    fn rendering() -> (Box<Rendering>, Held) {
        let dropped = Arc::new(AtomicUsize::new(0));
        let given = Arc::new(AtomicBool::new(false));
        let supply = Supply {
            dropped: Arc::clone(&dropped),
            given: Arc::clone(&given),
        };
        let (rendering, _) = Rendering::fed_by(Box::new(supply), Arc::new(AtomicBool::new(false)));
        (Box::new(rendering), Held { dropped, given })
    }

    fn status_of(error: &Error) -> Option<String> {
        match error {
            Error::Backend { source, .. } => source.get_ref().map(ToString::to_string),
            _ => None,
        }
    }

    #[test]
    fn a_unit_is_made_configured_and_initialised_in_that_order() {
        let fake = Fake::new();
        let (rendering, _) = rendering();

        let unit = Unit::opened(fake.clone(), SampleRate::HZ_44100, rendering).expect("a unit");

        assert_eq!(
            fake.calls(),
            [
                Call::Instantiate,
                Call::SetFormat,
                Call::SetRender,
                Call::Initialize
            ]
        );
        drop(unit);
    }

    #[test]
    fn the_unit_is_asked_for_interleaved_float_stereo_at_the_rate_given() {
        let fake = Fake::new();
        let (rendering, _) = rendering();

        let _unit = Unit::opened(fake.clone(), SampleRate::HZ_44100, rendering).expect("a unit");

        let format = fake.format.lock().expect("a format was set");
        assert_eq!(format.mSampleRate, 44_100.0);
        assert_eq!(format.mFormatID, kAudioFormatLinearPCM);
        assert_eq!(format.mFormatFlags, kAudioFormatFlagsNativeFloatPacked);
        assert_eq!(format.mChannelsPerFrame, 2);
        assert_eq!(format.mBitsPerChannel, 32);
        assert_eq!(format.mBytesPerFrame, 8);
        assert_eq!(format.mBytesPerPacket, 8);
        assert_eq!(format.mFramesPerPacket, 1);
    }

    #[test]
    fn the_callback_is_handed_the_rendering_that_reaches_the_source() {
        let fake = Fake::new();
        let (rendering, held) = rendering();

        let _unit = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering).expect("a unit");

        let handed = fake.rendering.lock().expect("a rendering was handed over");
        let mut bytes = [0_u8; 8];
        unsafe { &mut *handed.as_ptr() }.fill(&mut bytes);
        assert_eq!(bytes, [0xCD; 8]);
        assert!(held.given.load(Ordering::Acquire));
    }

    #[test]
    fn a_missing_component_is_not_found_and_nothing_is_made_or_leaked() {
        let fake = Fake {
            found: false,
            ..Fake::new()
        };
        let (rendering, held) = rendering();

        let Err(error) = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering) else {
            panic!("a unit was made without a component");
        };

        assert!(matches!(
            error,
            Error::Backend {
                op: Operation::Open,
                ref source
            } if source.kind() == io::ErrorKind::NotFound
        ));
        assert_eq!(fake.calls(), []);
        assert_eq!(held.dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_refused_instance_names_the_status_and_disposes_nothing() {
        let fake = Fake::refusing(Call::Instantiate, -50);
        let (rendering, held) = rendering();

        let Err(error) = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering) else {
            panic!("a unit was made from a refused instance");
        };

        assert_eq!(status_of(&error).as_deref(), Some("OSStatus -50"));
        assert_eq!(fake.calls(), [Call::Instantiate]);
        assert_eq!(held.dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_step_refused_after_the_instance_took_the_unit_down_and_frees_the_source_once() {
        for refused in [Call::SetFormat, Call::SetRender, Call::Initialize] {
            let fake = Fake::refusing(refused, 561_145_187);
            let (rendering, held) = rendering();

            let Err(error) = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering) else {
                panic!("a unit was made past a refused {refused:?}");
            };

            assert_eq!(status_of(&error).as_deref(), Some("OSStatus 561145187"));
            assert!(matches!(
                error,
                Error::Backend {
                    op: Operation::Open,
                    ..
                }
            ));
            let calls = fake.calls();
            assert_eq!(
                calls[calls.len() - 3..],
                [Call::Stop, Call::Uninitialize, Call::Dispose],
                "after {refused:?}"
            );
            assert_eq!(calls.iter().filter(|call| **call == refused).count(), 1);
            assert_eq!(held.dropped.load(Ordering::Acquire), 1, "after {refused:?}");
        }
    }

    #[test]
    fn a_unit_dropped_is_stopped_uninitialised_and_disposed_and_frees_the_source_once() {
        let fake = Fake::new();
        let (rendering, held) = rendering();
        let unit = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering).expect("a unit");
        assert_eq!(held.dropped.load(Ordering::Acquire), 0);

        drop(unit);

        assert_eq!(
            fake.calls()[4..],
            [Call::Stop, Call::Uninitialize, Call::Dispose]
        );
        assert_eq!(held.dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_unit_reformatted_is_stopped_uninitialised_set_and_initialised_again_at_the_new_rate() {
        let fake = Fake::new();
        let (rendering, _) = rendering();
        let unit = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering).expect("a unit");

        assert_eq!(unit.reformat(SampleRate::HZ_44100), 0);

        assert_eq!(
            fake.calls()[4..],
            [
                Call::Stop,
                Call::Uninitialize,
                Call::SetFormat,
                Call::Initialize
            ]
        );
        let format = fake.format.lock().expect("a format was set");
        assert_eq!(format.mSampleRate, 44_100.0);
        assert_eq!(format.mChannelsPerFrame, 2);
        assert_eq!(format.mFormatFlags, kAudioFormatFlagsNativeFloatPacked);
    }

    #[test]
    fn a_reformat_stops_at_the_step_the_system_refused_and_hands_back_its_status() {
        for (refused, after) in [
            (Call::Stop, 1),
            (Call::Uninitialize, 2),
            (Call::SetFormat, 3),
            (Call::Initialize, 4),
        ] {
            let fake = Fake::new();
            let (rendering, _) = rendering();
            let mut unit =
                Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering).expect("a unit");
            unit.toolbox.refusing = Some((refused, 561_145_187));
            let before = fake.calls().len();

            assert_eq!(
                unit.reformat(SampleRate::HZ_44100),
                561_145_187,
                "after {refused:?}"
            );

            assert_eq!(fake.calls().len() - before, after, "after {refused:?}");
        }
    }

    #[test]
    fn start_and_stop_reach_the_unit_and_hand_back_its_status() {
        let fake = Fake::refusing(Call::Start, 560_557_684);
        let (rendering, _) = rendering();
        let unit = Unit::opened(fake.clone(), SampleRate::HZ_48000, rendering).expect("a unit");

        assert_eq!(unit.start(), 560_557_684);
        assert_eq!(unit.stop(), 0);
    }
}
