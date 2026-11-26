use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use resonate_audio::{Error as AudioError, Operation};
use resonate_core::SampleRate;

use crate::{
    coreaudio_conform::Conforming,
    recording::{Recorder, Recording},
    session::{HEARD_AT, HEARD_IN_CHANNELS, Held},
};

const OPENED_WITHIN: Duration = Duration::from_secs(5);
const CONFORMED_EVERY: Duration = Duration::from_millis(40);
const HEARD_FOR_LONGER_THAN_ASKED_SECONDS: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Heard {
    pub(crate) rate: SampleRate,
    pub(crate) channels: NonZeroU32,
}

impl Heard {
    pub(crate) fn kept_of(rate: SampleRate, device_channels: NonZeroU32) -> Self {
        let kept = device_channels.get().min(HEARD_IN_CHANNELS as u32);

        Self {
            rate,
            channels: NonZeroU32::new(kept).unwrap_or(NonZeroU32::MIN),
        }
    }

    fn channels(&self) -> NonZeroUsize {
        NonZeroUsize::new(self.channels.get() as usize).unwrap_or(NonZeroUsize::MIN)
    }

    pub(crate) fn raw_samples_for(&self, clip_samples: usize) -> usize {
        let clip_frames = clip_samples / HEARD_IN_CHANNELS;
        let device_hz = self.rate.hz() as usize;

        let frames = clip_frames.saturating_mul(device_hz) / HEARD_AT.hz() as usize;
        frames
            .saturating_add(device_hz.saturating_mul(HEARD_FOR_LONGER_THAN_ASKED_SECONDS))
            .saturating_mul(self.channels().get())
    }
}

pub(crate) trait Opened {
    fn is_alive(&self) -> bool;
    fn stop(&self);
}

pub(crate) struct Capture {
    stopping: Sender<()>,
    lost: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Carrying {
    raw: Arc<Recording>,
    carried: usize,
    dropped: u64,
    conforming: Conforming,
    recorder: Recorder,
}

impl Carrying {
    fn carry(&mut self) {
        let heard = self.raw.heard_since(self.carried);

        self.carried += heard.len();
        if !heard.is_empty() {
            self.recorder.admit(&self.conforming.conformed(&heard));
        }

        let dropped = self.raw.dropped_frames();

        if dropped > self.dropped {
            self.recorder.note_dropped(dropped - self.dropped);
            self.dropped = dropped;
        }
    }
}

fn carried<O: Opened>(
    heard: Heard,
    recorder: Recorder,
    open: impl FnOnce(Recorder) -> Result<O, AudioError>,
    ready: &Sender<Result<(), AudioError>>,
    stopped: &Receiver<()>,
    lost: &AtomicBool,
) {
    let raw = Recording::holding(heard.raw_samples_for(recorder.room()));
    let conforming = match Conforming::of(heard.rate, heard.channels()) {
        Ok(conforming) => conforming,
        Err(_) => {
            let _ = ready.send(Err(AudioError::FormatRefused));

            return;
        }
    };
    let opened = match open(Recorder::admitting(Arc::clone(&raw))) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));

            return;
        }
    };
    let _ = ready.send(Ok(()));

    let mut carrying = Carrying {
        raw,
        carried: 0,
        dropped: 0,
        conforming,
        recorder,
    };

    while stopped.recv_timeout(CONFORMED_EVERY) == Err(RecvTimeoutError::Timeout) {
        carrying.carry();
        if !opened.is_alive() {
            lost.store(true, Ordering::Release);
        }
    }

    opened.stop();

    carrying.carry();
}

impl Capture {
    pub(crate) fn spawn<O: Opened>(
        heard: Heard,
        recorder: Recorder,
        open: impl FnOnce(Recorder) -> Result<O, AudioError> + Send + 'static,
    ) -> Result<Self, AudioError> {
        let (stopping, stopped) = mpsc::channel();
        let (ready, opened) = mpsc::channel();
        let lost = Arc::new(AtomicBool::new(false));

        let thread = thread::Builder::new()
            .name("resonate-listen-capture".into())
            .spawn({
                let lost = Arc::clone(&lost);
                move || carried(heard, recorder, open, &ready, &stopped, &lost)
            })
            .map_err(|source| AudioError::Backend {
                op: Operation::Open,
                source,
            })?;
        match opened.recv_timeout(OPENED_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                stopping,
                lost,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(AudioError::OpenUnanswered),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(AudioError::LoopStopped),
        }
    }

    pub(crate) fn stop(mut self) -> Result<(), AudioError> {
        let _ = self.stopping.send(());
        match self.thread.take().map(JoinHandle::join) {
            Some(Err(_)) => Err(AudioError::LoopStopped),
            _ => Ok(()),
        }
    }
}

impl Held for Capture {
    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.stopping.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::atomic::AtomicUsize, time::Instant};

    use parking_lot::Mutex;

    use super::*;

    const THE_CLIP_SECONDS: usize = 60;
    const A_THIRTY_TWO_CHANNEL_INTERFACE: u32 = 32;

    fn heard_at(hz: u32, channels: u32) -> Heard {
        Heard::kept_of(
            SampleRate::new(hz).expect("a rate"),
            NonZeroU32::new(channels).expect("channels"),
        )
    }

    fn bytes_of(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    type Shared = Arc<Mutex<Option<Recorder>>>;

    #[derive(Clone, Default)]
    struct Observed {
        alive: Arc<AtomicBool>,
        stopped: Arc<AtomicUsize>,
        disposed: Arc<AtomicBool>,
        raw: Shared,
        slow_to_stop: Duration,
    }

    struct Unit {
        observed: Observed,
    }

    impl Opened for Unit {
        fn is_alive(&self) -> bool {
            self.observed.alive.load(Ordering::Acquire)
        }

        fn stop(&self) {
            self.observed.stopped.fetch_add(1, Ordering::AcqRel);
            thread::sleep(self.observed.slow_to_stop);
        }
    }

    impl Drop for Unit {
        fn drop(&mut self) {
            self.observed.disposed.store(true, Ordering::Release);
        }
    }

    impl Observed {
        fn new() -> Self {
            Self {
                alive: Arc::new(AtomicBool::new(true)),
                ..Self::default()
            }
        }

        fn slow_to_stop(self, within: Duration) -> Self {
            Self {
                slow_to_stop: within,
                ..self
            }
        }

        fn open(&self) -> impl FnOnce(Recorder) -> Result<Unit, AudioError> + Send + 'static {
            let observed = self.clone();

            move |raw| {
                *observed.raw.lock() = Some(raw);
                Ok(Unit { observed })
            }
        }

        fn hears(&self, samples: &[f32]) {
            let mut raw = self.raw.lock();

            raw.as_mut()
                .expect("a raw recorder")
                .admit(&bytes_of(samples));
        }

        fn drops(&self, frames: u64) {
            let mut raw = self.raw.lock();

            raw.as_mut().expect("a raw recorder").note_dropped(frames);
        }
    }

    fn stereo_48k() -> Heard {
        heard_at(48_000, 2)
    }

    #[test]
    fn a_device_of_thirty_two_channels_at_192_khz_is_asked_for_the_clips_two() {
        let heard = heard_at(192_000, A_THIRTY_TWO_CHANNEL_INTERFACE);
        let clip_samples = THE_CLIP_SECONDS * HEARD_AT.hz() as usize * HEARD_IN_CHANNELS;

        assert_eq!(heard.channels.get(), 2);

        let raw = heard.raw_samples_for(clip_samples);
        let frames = (THE_CLIP_SECONDS + HEARD_FOR_LONGER_THAN_ASKED_SECONDS) * 192_000;

        assert_eq!(raw, frames * HEARD_IN_CHANNELS);
        assert!(raw * size_of::<f32>() < 100_000_000);
    }

    #[test]
    fn a_microphone_of_one_channel_keeps_its_one_and_a_pair_its_two() {
        assert_eq!(heard_at(48_000, 1).channels.get(), 1);
        assert_eq!(heard_at(48_000, 2).channels.get(), 2);
        assert_eq!(heard_at(48_000, 3).channels.get(), 2);
    }

    #[test]
    fn a_capture_dropped_is_joined_so_its_last_carry_is_in_before_another_one_opens() {
        let recording = Recording::holding(64);
        let observed = Observed::new().slow_to_stop(Duration::from_millis(60));

        let capture = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(Arc::clone(&recording)),
            observed.open(),
        )
        .expect("a capture");

        observed.hears(&[0.25, -0.25, 0.5, -0.5]);

        drop(capture);

        assert!(observed.disposed.load(Ordering::Acquire));
        assert_eq!(observed.stopped.load(Ordering::Acquire), 1);
        assert_eq!(recording.taken(), vec![0.25, -0.25, 0.5, -0.5]);

        recording.silence_for(3, 2);

        thread::sleep(Duration::from_millis(120));

        assert_eq!(recording.filled(), 10);
        assert_eq!(
            recording.taken(),
            vec![0.25, -0.25, 0.5, -0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
        );
    }

    #[test]
    fn a_capture_opened_on_the_same_recording_after_a_drop_carries_on_where_it_left_off() {
        let recording = Recording::holding(64);
        let first = Observed::new();
        let second = Observed::new();

        let lost = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(Arc::clone(&recording)),
            first.open(),
        )
        .expect("a capture");

        first.hears(&[0.25, -0.25]);
        drop(lost);

        recording.silence_for(1, 2);

        let (recorder, admitting) = Recorder::held_back(Arc::clone(&recording));
        let reopened = Capture::spawn(stereo_48k(), recorder, second.open()).expect("a capture");

        admitting.store(true, Ordering::Release);
        second.hears(&[0.75, -0.75]);

        reopened.stop().expect("a stop");

        assert_eq!(recording.taken(), vec![0.25, -0.25, 0.0, 0.0, 0.75, -0.75]);
    }

    #[test]
    fn a_stopped_capture_stops_its_unit_once_and_disposes_it() {
        let recording = Recording::holding(16);
        let observed = Observed::new();

        let capture = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(Arc::clone(&recording)),
            observed.open(),
        )
        .expect("a capture");

        observed.hears(&[0.5, 0.5]);

        capture.stop().expect("a stop");

        assert_eq!(observed.stopped.load(Ordering::Acquire), 1);
        assert!(observed.disposed.load(Ordering::Acquire));
        assert_eq!(recording.taken(), vec![0.5, 0.5]);
    }

    #[test]
    fn a_device_that_is_no_longer_alive_makes_the_capture_lost() {
        let recording = Recording::holding(16);
        let observed = Observed::new();

        let capture = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(Arc::clone(&recording)),
            observed.open(),
        )
        .expect("a capture");

        assert!(!capture.is_lost());

        observed.alive.store(false, Ordering::Release);

        let asked = Instant::now();

        while !capture.is_lost() && asked.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(10));
        }

        assert!(capture.is_lost());
    }

    #[test]
    fn a_unit_that_would_not_open_is_the_error_it_gave() {
        let recording = Recording::holding(16);

        let Err(error) = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(recording),
            |_| -> Result<Unit, AudioError> { Err(AudioError::NoSink) },
        ) else {
            panic!("a capture opened without a unit");
        };

        assert!(matches!(error, AudioError::NoSink));
    }

    #[test]
    fn frames_the_unit_dropped_reach_the_recording_the_sitting_reads() {
        let recording = Recording::holding(64);
        let observed = Observed::new();

        let capture = Capture::spawn(
            stereo_48k(),
            Recorder::admitting(Arc::clone(&recording)),
            observed.open(),
        )
        .expect("a capture");

        observed.drops(16_384);
        observed.drops(16_384);

        drop(capture);

        assert_eq!(recording.dropped_frames(), 32_768);
        assert_eq!(recording.filled(), 0);
    }
}
