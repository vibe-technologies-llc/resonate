use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use resonate_audio::AudioSource;
use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use crate::{
    Clip, Error, Hearing, Result,
    recording::{Recorder, Recording},
};

pub(crate) const HEARD_AT: SampleRate = SampleRate::HZ_48000;
pub(crate) const HEARD_IN: ChannelLayout = ChannelLayout::Stereo;
pub(crate) const HEARD_IN_CHANNELS: usize = 2;
pub(crate) const BYTES_PER_SAMPLE: usize = size_of::<f32>();
pub(crate) const BYTES_PER_FRAME: usize = HEARD_IN_CHANNELS * BYTES_PER_SAMPLE;

const LOOKS_EVERY: Duration = Duration::from_millis(40);
const GIVES_UP_AFTER_MORE_THAN_ASKED: Duration = Duration::from_secs(4);

pub(crate) const fn heard_spec() -> StreamSpec {
    StreamSpec::new(HEARD_AT, HEARD_IN, SampleFormat::F32)
}

pub(crate) trait Held {
    fn is_lost(&self) -> bool;
}

pub(crate) struct Sitting {
    pub(crate) spec: StreamSpec,
    pub(crate) channels: usize,
    pub(crate) recording: Arc<Recording>,
}

impl Sitting {
    pub(crate) fn begin(length: Duration, hearing: &Hearing) -> Self {
        let spec = heard_spec();
        let channels = usize::from(spec.channel_count().get());
        let frames = frames_in(length);
        hearing.wanted.store(frames as u64, Ordering::Relaxed);
        hearing.heard.store(0, Ordering::Relaxed);
        Self {
            spec,
            channels,
            recording: Recording::holding(frames * channels),
        }
    }

    pub(crate) fn until_full<S: Held>(
        &self,
        opened: S,
        length: Duration,
        hearing: &Hearing,
        mut again: impl FnMut(Recorder) -> Option<S>,
    ) -> Option<S> {
        let started = Instant::now();
        let deadline = length + GIVES_UP_AFTER_MORE_THAN_ASKED;
        let mut stream = Some(opened);
        let mut lost_since = None;
        let mut filled_at_reopen = None;
        while !self.recording.is_full() && !hearing.stopped() && started.elapsed() < deadline {
            thread::sleep(LOOKS_EVERY);
            hearing.heard.store(
                (self.recording.filled() / self.channels) as u64,
                Ordering::Relaxed,
            );

            if stream.as_ref().is_some_and(Held::is_lost) {
                if filled_at_reopen.take() == Some(self.recording.filled()) {
                    tracing::debug!("the capture reopened was lost before it heard anything");
                } else {
                    tracing::warn!("the capture was lost; opening it again");
                }
                stream = None;
                lost_since.get_or_insert_with(Instant::now);
            }
            if stream.is_none() {
                let since = lost_since.unwrap_or_else(Instant::now);
                stream = self.reopened(since, &mut again);
                if stream.is_some() {
                    lost_since = None;
                    filled_at_reopen = Some(self.recording.filled());
                }
            }
        }
        stream
    }

    fn reopened<S>(
        &self,
        lost_since: Instant,
        again: &mut impl FnMut(Recorder) -> Option<S>,
    ) -> Option<S> {
        let (recorder, admitting) = Recorder::held_back(Arc::clone(&self.recording));
        let stream = again(recorder)?;
        self.recording
            .silence_for(frames_in(lost_since.elapsed()), self.channels);
        admitting.store(true, Ordering::Release);
        tracing::info!("the capture carries on after a gap of silence");
        Some(stream)
    }
}

pub(crate) struct Played {
    mono: Arc<[f32]>,
    at: usize,
}

impl Played {
    pub(crate) fn of(samples: &[f32]) -> Self {
        Self {
            mono: samples.into(),
            at: 0,
        }
    }
}

impl AudioSource for Played {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let whole = dst.len() / BYTES_PER_FRAME * BYTES_PER_FRAME;
        let frames = dst.get_mut(..whole).unwrap_or_default();
        for frame in frames.as_chunks_mut::<BYTES_PER_FRAME>().0 {
            let sample = self.mono.get(self.at).copied().unwrap_or(0.0);
            self.at = self.at.saturating_add(1);
            for word in frame.as_chunks_mut::<BYTES_PER_SAMPLE>().0 {
                *word = sample.to_le_bytes();
            }
        }
        whole
    }
}

pub(crate) fn finished(
    stopped: Result<Option<()>>,
    asked_to_stop: bool,
    samples: Vec<f32>,
    spec: StreamSpec,
) -> Result<Clip> {
    if asked_to_stop {
        return Err(Error::Stopped);
    }
    if samples.is_empty() {
        stopped?;
        return Err(Error::NothingHeard);
    }
    if let Err(error) = stopped {
        tracing::warn!(%error, "the capture would not stop cleanly; the clip heard is kept");
    }
    Ok(Clip {
        rate: spec.rate,
        channels: u16::from(spec.channel_count().get()),
        samples,
    })
}

pub(crate) fn frames_in(elapsed: Duration) -> usize {
    (elapsed.as_secs_f64() * f64::from(HEARD_AT.hz())).round() as usize
}

#[cfg(any(windows, test))]
pub(crate) fn zeroed_if_silent(bytes: &mut [u8], silent: bool) {
    if silent {
        bytes.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use super::*;
    use crate::CaptureOp;

    fn stop_failed() -> Result<Option<()>> {
        Err(Error::capture(
            CaptureOp::Stop,
            resonate_audio::Error::Disconnected,
        ))
    }

    #[test]
    fn a_clip_survives_a_stop_that_failed() {
        let clip =
            finished(stop_failed(), false, vec![0.5, -0.5], heard_spec()).expect("the clip heard");

        assert_eq!(clip.samples, vec![0.5, -0.5]);
        assert_eq!(clip.channels, 2);
    }

    #[test]
    fn a_stop_that_failed_with_nothing_heard_is_the_error_it_was() {
        assert!(matches!(
            finished(stop_failed(), false, Vec::new(), heard_spec()),
            Err(Error::Capture {
                op: CaptureOp::Stop,
                ..
            })
        ));
        assert!(matches!(
            finished(Ok(Some(())), false, Vec::new(), heard_spec()),
            Err(Error::NothingHeard)
        ));
    }

    #[test]
    fn a_listener_who_stopped_it_hears_no_clip() {
        assert!(matches!(
            finished(Ok(Some(())), true, vec![0.5, 0.5], heard_spec()),
            Err(Error::Stopped)
        ));
    }

    #[test]
    fn what_is_played_fills_both_channels_in_whole_frames_and_then_falls_silent() {
        let mut played = Played::of(&[0.5, -0.25]);
        let mut block = [0xff_u8; 3 * BYTES_PER_FRAME + 3];

        let filled = played.fill(&mut block);

        assert_eq!(filled, 3 * BYTES_PER_FRAME);
        let words: Vec<f32> = block
            .get(..filled)
            .unwrap_or_default()
            .as_chunks::<BYTES_PER_SAMPLE>()
            .0
            .iter()
            .map(|word| f32::from_le_bytes(*word))
            .collect();
        assert_eq!(words, vec![0.5, 0.5, -0.25, -0.25, 0.0, 0.0]);
    }

    #[test]
    fn a_gap_counts_the_frames_of_the_time_that_passed() {
        assert_eq!(frames_in(Duration::from_millis(500)), 24_000);
        assert_eq!(frames_in(Duration::ZERO), 0);
    }

    #[test]
    fn a_buffer_flagged_silent_is_zeroed_and_any_other_is_left() {
        let mut flagged = [1_u8, 2, 3];
        let mut plain = [1_u8, 2, 3];

        zeroed_if_silent(&mut flagged, true);
        zeroed_if_silent(&mut plain, false);

        assert_eq!(flagged, [0, 0, 0]);
        assert_eq!(plain, [1, 2, 3]);
    }

    #[test]
    fn beginning_sizes_the_recording_to_the_length_and_resets_the_count() {
        let hearing = Hearing::new();
        hearing.heard.store(9, Ordering::Relaxed);

        let sitting = Sitting::begin(Duration::from_millis(500), &hearing);

        assert_eq!(sitting.recording.capacity(), 24_000 * 2);
        assert_eq!(sitting.channels, 2);
        assert_eq!(hearing.wanted.load(Ordering::Relaxed), 24_000);
        assert_eq!(hearing.heard.load(Ordering::Relaxed), 0);
    }

    struct Tap {
        lost: Arc<AtomicBool>,
    }

    impl Tap {
        fn lost() -> Self {
            Self {
                lost: Arc::new(AtomicBool::new(true)),
            }
        }

        fn kept() -> Self {
            Self {
                lost: Arc::new(AtomicBool::new(false)),
            }
        }
    }

    impl Held for Tap {
        fn is_lost(&self) -> bool {
            self.lost.load(Ordering::Acquire)
        }
    }

    #[test]
    fn a_capture_lost_is_opened_again_and_the_gap_is_silence_of_whole_frames() {
        let hearing = Hearing::new();
        let sitting = Sitting::begin(Duration::from_secs(5), &hearing);
        let mut reopened = 0;

        let left = sitting.until_full(Tap::lost(), Duration::from_secs(5), &hearing, |_| {
            reopened += 1;
            thread::sleep(Duration::from_millis(30));
            hearing.stop();
            Some(Tap::kept())
        });

        assert_eq!(reopened, 1);
        assert!(left.is_some());
        assert!(sitting.recording.filled() > 0);
        assert_eq!(sitting.recording.filled() % sitting.channels, 0);
        assert!(
            sitting
                .recording
                .taken()
                .iter()
                .all(|sample| *sample == 0.0)
        );
    }

    #[test]
    fn a_capture_that_cannot_open_again_leaves_the_recording_alone() {
        let hearing = Hearing::new();
        let sitting = Sitting::begin(Duration::from_secs(5), &hearing);
        let mut attempts = 0;

        let left = sitting.until_full(Tap::lost(), Duration::from_secs(5), &hearing, |_| {
            attempts += 1;
            if attempts == 2 {
                hearing.stop();
            }
            None::<Tap>
        });

        assert_eq!(attempts, 2);
        assert!(left.is_none());
        assert_eq!(sitting.recording.filled(), 0);
    }
}
