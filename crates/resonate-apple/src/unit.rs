use std::{
    fmt,
    num::{NonZeroU32, NonZeroUsize},
    ptr::NonNull,
    result,
};

use objc2_core_audio_types::AudioStreamBasicDescription;

use crate::{
    asbd::Format,
    clock::HostClock,
    error::{Error, Op, Result},
    status::OsStatus,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostTime {
    pub now: u64,
    pub of_the_buffer: u64,
}

pub trait Render: Send {
    fn render(&mut self, bytes: &mut [u8], host: Option<HostTime>);
}

pub trait Hear: Send {
    fn hear(&mut self, bytes: &[u8]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Component {
    RemoteIo,
    HalOutput,
}

impl fmt::Display for Component {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RemoteIo => "RemoteIO",
            Self::HalOutput => "HAL output",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bus {
    Output,
    Input,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub component: Component,
    pub device: Option<u32>,
    pub format: Format,
}

pub(crate) struct Rendered {
    pub(crate) render: Box<dyn Render>,
    pub(crate) clock: HostClock,
}

pub(crate) struct Hearing<I> {
    pub(crate) unit: I,
    pub(crate) scratch: Box<[u8]>,
    pub(crate) frame: NonZeroUsize,
    pub(crate) channels: u32,
    pub(crate) hear: Box<dyn Hear>,
}

pub(crate) trait Toolbox {
    type Instance: Copy;

    fn host_clock(&self) -> HostClock;
    fn has_component(&self, component: Component) -> bool;
    fn instantiate(&self, component: Component) -> result::Result<Self::Instance, i32>;
    fn enable_io(&self, unit: Self::Instance, bus: Bus, enabled: bool) -> i32;
    fn set_device(&self, unit: Self::Instance, device: u32) -> i32;
    fn set_format(
        &self,
        unit: Self::Instance,
        bus: Bus,
        format: &AudioStreamBasicDescription,
    ) -> i32;
    fn format_of_the_device(
        &self,
        unit: Self::Instance,
        bus: Bus,
    ) -> result::Result<AudioStreamBasicDescription, i32>;
    fn set_render(&self, unit: Self::Instance, rendered: NonNull<Rendered>) -> i32;
    fn set_hearing(&self, unit: Self::Instance, hearing: NonNull<Hearing<Self::Instance>>) -> i32;
    fn initialize(&self, unit: Self::Instance) -> i32;
    fn start(&self, unit: Self::Instance) -> i32;
    fn stop(&self, unit: Self::Instance) -> i32;
    fn uninitialize(&self, unit: Self::Instance) -> i32;
    fn dispose(&self, unit: Self::Instance) -> i32;
}

struct Leaked<V>(NonNull<V>);

impl<V> Leaked<V> {
    fn new(value: Box<V>) -> Self {
        Self(NonNull::from(Box::leak(value)))
    }
}

impl<V> Drop for Leaked<V> {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

pub(crate) struct Sequenced<T: Toolbox> {
    toolbox: T,
    unit: T::Instance,
    bus: Bus,
    _rendered: Option<Leaked<Rendered>>,
    _hearing: Option<Leaked<Hearing<T::Instance>>>,
}

fn checked(op: Op, status: i32) -> Result<()> {
    OsStatus::checked(status).map_err(Error::refused(op))
}

impl<T: Toolbox> Sequenced<T> {
    fn instance_of(toolbox: &T, component: Component) -> Result<T::Instance> {
        if !toolbox.has_component(component) {
            return Err(Error::NoComponent { component });
        }
        toolbox
            .instantiate(component)
            .map_err(|status| Error::Refused {
                op: Op::MakeUnit,
                source: OsStatus::of(status),
            })
    }

    pub(crate) fn playing(toolbox: T, shape: Shape, render: Box<dyn Render>) -> Result<Self> {
        let unit = Self::instance_of(&toolbox, shape.component)?;
        let rendered = Leaked::new(Box::new(Rendered {
            render,
            clock: toolbox.host_clock(),
        }));
        let handed = rendered.0;
        let opened = Self {
            toolbox,
            unit,
            bus: Bus::Output,
            _rendered: Some(rendered),
            _hearing: None,
        };

        if let Some(device) = shape.device {
            checked(Op::SetDevice, opened.toolbox.set_device(unit, device))?;
        }
        checked(
            Op::SetFormat,
            opened
                .toolbox
                .set_format(unit, Bus::Output, &shape.format.described()),
        )?;
        checked(Op::SetCallback, opened.toolbox.set_render(unit, handed))?;
        checked(Op::Initialize, opened.toolbox.initialize(unit))?;
        Ok(opened)
    }

    pub(crate) fn hearing(
        toolbox: T,
        shape: Shape,
        hear: Box<dyn Hear>,
        frames_a_slice_at_most: NonZeroU32,
    ) -> Result<Self> {
        let unit = Self::instance_of(&toolbox, shape.component)?;
        let frame = shape.format.frame_bytes();
        let scratch = frames_a_slice_at_most.get() as usize * frame.get() as usize;
        let hearing = Leaked::new(Box::new(Hearing {
            unit,
            scratch: vec![0; scratch].into_boxed_slice(),
            frame: NonZeroUsize::new(frame.get() as usize).unwrap_or(NonZeroUsize::MIN),
            channels: shape.format.channels().get(),
            hear,
        }));
        let handed = hearing.0;
        let opened = Self {
            toolbox,
            unit,
            bus: Bus::Input,
            _rendered: None,
            _hearing: Some(hearing),
        };

        checked(
            Op::EnableIo,
            opened.toolbox.enable_io(unit, Bus::Input, true),
        )?;
        checked(
            Op::EnableIo,
            opened.toolbox.enable_io(unit, Bus::Output, false),
        )?;
        if let Some(device) = shape.device {
            checked(Op::SetDevice, opened.toolbox.set_device(unit, device))?;
        }
        checked(
            Op::SetFormat,
            opened
                .toolbox
                .set_format(unit, Bus::Input, &shape.format.described()),
        )?;
        checked(Op::SetCallback, opened.toolbox.set_hearing(unit, handed))?;
        checked(Op::Initialize, opened.toolbox.initialize(unit))?;
        Ok(opened)
    }

    pub(crate) fn start(&self) -> result::Result<(), OsStatus> {
        OsStatus::checked(self.toolbox.start(self.unit))
    }

    pub(crate) fn stop(&self) -> result::Result<(), OsStatus> {
        OsStatus::checked(self.toolbox.stop(self.unit))
    }

    pub(crate) fn reformatted(&self, format: Format) -> result::Result<(), OsStatus> {
        let described = format.described();
        let steps: [&dyn Fn() -> i32; 4] = [
            &|| self.toolbox.stop(self.unit),
            &|| self.toolbox.uninitialize(self.unit),
            &|| self.toolbox.set_format(self.unit, self.bus, &described),
            &|| self.toolbox.initialize(self.unit),
        ];
        steps
            .into_iter()
            .try_for_each(|step| OsStatus::checked(step()))
    }

    pub(crate) fn device_rate(&self) -> Result<f64> {
        self.toolbox
            .format_of_the_device(self.unit, self.bus)
            .map(|format| format.mSampleRate)
            .map_err(|status| Error::Refused {
                op: Op::ReadFormat,
                source: OsStatus::of(status),
            })
    }
}

impl<T: Toolbox> Drop for Sequenced<T> {
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

    use objc2_core_audio_types::{kAudioFormatFlagsNativeFloatPacked, kAudioFormatLinearPCM};
    use parking_lot::Mutex;

    use super::*;
    use crate::asbd::Word;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        Instantiate,
        EnableIo(Bus, bool),
        SetDevice(u32),
        SetFormat(Bus),
        SetRender,
        SetHearing,
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
        rendered: Arc<Mutex<Option<NonNull<Rendered>>>>,
        hearing: Arc<Mutex<Option<NonNull<Hearing<u8>>>>>,
    }

    unsafe impl Send for Fake {}

    impl Fake {
        fn new() -> Self {
            Self {
                calls: Arc::default(),
                found: true,
                refusing: None,
                format: Arc::default(),
                rendered: Arc::default(),
                hearing: Arc::default(),
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

        fn host_clock(&self) -> HostClock {
            HostClock::NANOSECONDS
        }

        fn has_component(&self, _: Component) -> bool {
            self.found
        }

        fn instantiate(&self, _: Component) -> result::Result<u8, i32> {
            match self.answer(Call::Instantiate) {
                0 => Ok(7),
                status => Err(status),
            }
        }

        fn enable_io(&self, _: u8, bus: Bus, enabled: bool) -> i32 {
            self.answer(Call::EnableIo(bus, enabled))
        }

        fn set_device(&self, _: u8, device: u32) -> i32 {
            self.answer(Call::SetDevice(device))
        }

        fn set_format(&self, _: u8, bus: Bus, format: &AudioStreamBasicDescription) -> i32 {
            *self.format.lock() = Some(*format);
            self.answer(Call::SetFormat(bus))
        }

        fn format_of_the_device(
            &self,
            _: u8,
            _: Bus,
        ) -> result::Result<AudioStreamBasicDescription, i32> {
            self.format.lock().ok_or(-50)
        }

        fn set_render(&self, _: u8, rendered: NonNull<Rendered>) -> i32 {
            *self.rendered.lock() = Some(rendered);
            self.answer(Call::SetRender)
        }

        fn set_hearing(&self, _: u8, hearing: NonNull<Hearing<u8>>) -> i32 {
            *self.hearing.lock() = Some(hearing);
            self.answer(Call::SetHearing)
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

    impl Render for Supply {
        fn render(&mut self, bytes: &mut [u8], _: Option<HostTime>) {
            self.given.store(true, Ordering::Release);
            bytes.fill(0xCD);
        }
    }

    impl Hear for Supply {
        fn hear(&mut self, _: &[u8]) {
            self.given.store(true, Ordering::Release);
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

    fn supply() -> (Box<Supply>, Held) {
        let dropped = Arc::new(AtomicUsize::new(0));
        let given = Arc::new(AtomicBool::new(false));
        let supply = Supply {
            dropped: Arc::clone(&dropped),
            given: Arc::clone(&given),
        };
        (Box::new(supply), Held { dropped, given })
    }

    fn stereo_float(hz: f64) -> Format {
        Format::new(hz, NonZeroU32::MIN.saturating_add(1), Word::Float32)
    }

    fn remote_io(hz: f64) -> Shape {
        Shape {
            component: Component::RemoteIo,
            device: None,
            format: stereo_float(hz),
        }
    }

    fn hal(device: u32, hz: f64) -> Shape {
        Shape {
            component: Component::HalOutput,
            device: Some(device),
            format: stereo_float(hz),
        }
    }

    fn a_slice() -> NonZeroU32 {
        NonZeroU32::MIN.saturating_add(511)
    }

    fn status_of(error: &Error) -> Option<(Op, i32)> {
        match error {
            Error::Refused { op, source } => Some((*op, source.get())),
            _ => None,
        }
    }

    #[test]
    fn a_unit_is_made_configured_and_initialised_in_that_order() {
        let fake = Fake::new();
        let (render, _) = supply();

        let unit = Sequenced::playing(fake.clone(), remote_io(44_100.0), render).expect("a unit");

        assert_eq!(
            fake.calls(),
            [
                Call::Instantiate,
                Call::SetFormat(Bus::Output),
                Call::SetRender,
                Call::Initialize
            ]
        );
        drop(unit);
    }

    #[test]
    fn a_unit_on_a_device_is_given_the_device_before_its_format() {
        let fake = Fake::new();
        let (render, _) = supply();

        let _unit = Sequenced::playing(fake.clone(), hal(42, 96_000.0), render).expect("a unit");

        assert_eq!(
            fake.calls(),
            [
                Call::Instantiate,
                Call::SetDevice(42),
                Call::SetFormat(Bus::Output),
                Call::SetRender,
                Call::Initialize
            ]
        );
    }

    #[test]
    fn a_unit_that_hears_turns_its_input_on_and_its_output_off_before_it_takes_a_device() {
        let fake = Fake::new();
        let (hear, _) = supply();

        let _unit =
            Sequenced::hearing(fake.clone(), hal(9, 48_000.0), hear, a_slice()).expect("a unit");

        assert_eq!(
            fake.calls(),
            [
                Call::Instantiate,
                Call::EnableIo(Bus::Input, true),
                Call::EnableIo(Bus::Output, false),
                Call::SetDevice(9),
                Call::SetFormat(Bus::Input),
                Call::SetHearing,
                Call::Initialize
            ]
        );
        let hearing = fake.hearing.lock().expect("a hearing was handed over");
        let hearing = unsafe { hearing.as_ref() };
        assert_eq!(hearing.unit, 7);
        assert_eq!(hearing.frame.get(), 8);
        assert_eq!(hearing.channels, 2);
        assert_eq!(hearing.scratch.len(), 512 * 8);
    }

    #[test]
    fn a_step_refused_while_a_hearing_unit_is_made_takes_it_down_and_frees_the_listener_once() {
        for refused in [
            Call::EnableIo(Bus::Input, true),
            Call::EnableIo(Bus::Output, false),
            Call::SetDevice(9),
            Call::SetFormat(Bus::Input),
            Call::SetHearing,
            Call::Initialize,
        ] {
            let fake = Fake::refusing(refused, -10_851);
            let (hear, held) = supply();

            let Err(error) = Sequenced::hearing(fake.clone(), hal(9, 48_000.0), hear, a_slice())
            else {
                panic!("a unit was made past a refused {refused:?}");
            };

            assert_eq!(
                status_of(&error).map(|(_, status)| status),
                Some(-10_851),
                "after {refused:?}"
            );
            let calls = fake.calls();
            assert_eq!(
                calls[calls.len() - 3..],
                [Call::Stop, Call::Uninitialize, Call::Dispose],
                "after {refused:?}"
            );
            assert_eq!(held.dropped.load(Ordering::Acquire), 1, "after {refused:?}");
        }
    }

    #[test]
    fn the_unit_is_asked_for_the_format_of_its_shape() {
        let fake = Fake::new();
        let (render, _) = supply();

        let _unit = Sequenced::playing(fake.clone(), remote_io(44_100.0), render).expect("a unit");

        let format = fake.format.lock().expect("a format was set");
        assert_eq!(format.mSampleRate, 44_100.0);
        assert_eq!(format.mFormatID, kAudioFormatLinearPCM);
        assert_eq!(format.mFormatFlags, kAudioFormatFlagsNativeFloatPacked);
        assert_eq!(format.mChannelsPerFrame, 2);
        assert_eq!(format.mBytesPerFrame, 8);
    }

    #[test]
    fn the_callback_is_handed_what_reaches_the_render() {
        let fake = Fake::new();
        let (render, held) = supply();

        let _unit = Sequenced::playing(fake.clone(), remote_io(48_000.0), render).expect("a unit");

        let handed = fake.rendered.lock().expect("a render was handed over");
        let mut bytes = [0_u8; 8];
        unsafe { &mut *handed.as_ptr() }
            .render
            .render(&mut bytes, None);
        assert_eq!(bytes, [0xCD; 8]);
        assert!(held.given.load(Ordering::Acquire));
    }

    #[test]
    fn a_missing_component_is_named_and_nothing_is_made_or_leaked() {
        let fake = Fake {
            found: false,
            ..Fake::new()
        };
        let (render, held) = supply();

        let Err(error) = Sequenced::playing(fake.clone(), remote_io(48_000.0), render) else {
            panic!("a unit was made without a component");
        };

        assert!(matches!(
            error,
            Error::NoComponent {
                component: Component::RemoteIo
            }
        ));
        assert_eq!(fake.calls(), []);
        assert_eq!(held.dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_refused_instance_names_the_status_and_disposes_nothing() {
        let fake = Fake::refusing(Call::Instantiate, -50);
        let (render, held) = supply();

        let Err(error) = Sequenced::playing(fake.clone(), remote_io(48_000.0), render) else {
            panic!("a unit was made from a refused instance");
        };

        assert_eq!(status_of(&error), Some((Op::MakeUnit, -50)));
        assert_eq!(fake.calls(), [Call::Instantiate]);
        assert_eq!(held.dropped.load(Ordering::Acquire), 1);
    }

    #[test]
    fn a_step_refused_after_the_instance_took_the_unit_down_and_frees_the_render_once() {
        for (refused, op) in [
            (Call::SetFormat(Bus::Output), Op::SetFormat),
            (Call::SetRender, Op::SetCallback),
            (Call::Initialize, Op::Initialize),
        ] {
            let fake = Fake::refusing(refused, 561_145_187);
            let (render, held) = supply();

            let Err(error) = Sequenced::playing(fake.clone(), remote_io(48_000.0), render) else {
                panic!("a unit was made past a refused {refused:?}");
            };

            assert_eq!(status_of(&error), Some((op, 561_145_187)));
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
    fn a_unit_dropped_is_stopped_uninitialised_and_disposed_and_frees_the_render_once() {
        let fake = Fake::new();
        let (render, held) = supply();
        let unit = Sequenced::playing(fake.clone(), remote_io(48_000.0), render).expect("a unit");
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
        let (render, _) = supply();
        let unit = Sequenced::playing(fake.clone(), remote_io(48_000.0), render).expect("a unit");

        assert_eq!(unit.reformatted(stereo_float(44_100.0)), Ok(()));

        assert_eq!(
            fake.calls()[4..],
            [
                Call::Stop,
                Call::Uninitialize,
                Call::SetFormat(Bus::Output),
                Call::Initialize
            ]
        );
        let format = fake.format.lock().expect("a format was set");
        assert_eq!(format.mSampleRate, 44_100.0);
        assert_eq!(format.mChannelsPerFrame, 2);
        assert_eq!(unit.device_rate().ok(), Some(44_100.0));
    }

    #[test]
    fn a_reformat_stops_at_the_step_the_system_refused_and_hands_back_its_status() {
        for (refused, after) in [
            (Call::Stop, 1),
            (Call::Uninitialize, 2),
            (Call::SetFormat(Bus::Output), 3),
            (Call::Initialize, 4),
        ] {
            let fake = Fake::new();
            let (render, _) = supply();
            let mut unit =
                Sequenced::playing(fake.clone(), remote_io(48_000.0), render).expect("a unit");
            unit.toolbox.refusing = Some((refused, 561_145_187));
            let before = fake.calls().len();

            assert_eq!(
                unit.reformatted(stereo_float(44_100.0))
                    .map_err(OsStatus::get),
                Err(561_145_187),
                "after {refused:?}"
            );

            assert_eq!(fake.calls().len() - before, after, "after {refused:?}");
        }
    }

    #[test]
    fn start_and_stop_reach_the_unit_and_hand_back_its_status() {
        let fake = Fake::refusing(Call::Start, 560_557_684);
        let (render, _) = supply();
        let unit = Sequenced::playing(fake.clone(), remote_io(48_000.0), render).expect("a unit");

        assert_eq!(unit.start().map_err(OsStatus::get), Err(560_557_684));
        assert_eq!(unit.stop(), Ok(()));
    }
}
