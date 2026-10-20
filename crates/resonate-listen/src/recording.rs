#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};

use resonate_pipewire::AudioSink;

const BYTES_PER_SAMPLE: usize = size_of::<f32>();

pub struct Recording {
    samples: Box<[AtomicU32]>,
    filled: AtomicUsize,
}

impl Recording {
    pub fn holding(samples: usize) -> Arc<Self> {
        Arc::new(Self {
            samples: (0..samples).map(|_| AtomicU32::new(0)).collect(),
            filled: AtomicUsize::new(0),
        })
    }

    pub fn filled(&self) -> usize {
        self.filled.load(Ordering::Acquire)
    }

    pub fn is_full(&self) -> bool {
        self.filled() >= self.samples.len()
    }

    pub fn capacity(&self) -> usize {
        self.samples.len()
    }

    pub fn taken(&self) -> Vec<f32> {
        let filled = self.filled();
        self.samples
            .iter()
            .take(filled)
            .map(|bits| f32::from_bits(bits.load(Ordering::Relaxed)))
            .collect()
    }

    pub fn silence_for(&self, frames: usize, channels: usize) {
        let channels = channels.max(1);
        let from = self
            .filled
            .load(Ordering::Relaxed)
            .next_multiple_of(channels);
        let to = from
            .saturating_add(frames.saturating_mul(channels))
            .min(self.samples.len());
        self.filled.store(to, Ordering::Release);
    }

    fn keep(&self, bytes: &[u8]) {
        let mut at = self.filled.load(Ordering::Relaxed);
        for word in bytes.as_chunks::<BYTES_PER_SAMPLE>().0 {
            let Some(slot) = self.samples.get(at) else {
                break;
            };
            slot.store(f32::from_le_bytes(*word).to_bits(), Ordering::Relaxed);
            at += 1;
        }
        self.filled.store(at, Ordering::Release);
    }
}

pub(crate) struct Recorder {
    recording: Arc<Recording>,
    admitting: Arc<AtomicBool>,
}

impl Recorder {
    pub(crate) fn admitting(recording: Arc<Recording>) -> Self {
        Self {
            recording,
            admitting: Arc::new(AtomicBool::new(true)),
        }
    }

    pub(crate) fn held_back(recording: Arc<Recording>) -> (Self, Arc<AtomicBool>) {
        let admitting = Arc::new(AtomicBool::new(false));
        let recorder = Self {
            recording,
            admitting: Arc::clone(&admitting),
        };
        (recorder, admitting)
    }
}

impl AudioSink for Recorder {
    fn take(&mut self, bytes: &[u8]) {
        if self.admitting.load(Ordering::Acquire) {
            self.recording.keep(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_keeps_what_it_is_handed_and_stops_when_full() {
        let recording = Recording::holding(3);
        let mut recorder = Recorder::admitting(Arc::clone(&recording));
        let bytes: Vec<u8> = [0.5_f32, -0.25, 1.0, 0.75]
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect();
        recorder.take(bytes.get(..8).unwrap_or_default());
        assert_eq!(recording.taken(), vec![0.5, -0.25]);
        recorder.take(bytes.get(8..).unwrap_or_default());
        assert!(recording.is_full());
        assert_eq!(recording.taken(), vec![0.5, -0.25, 1.0]);
    }

    fn bytes_of(samples: &[f32]) -> Vec<u8> {
        samples
            .iter()
            .flat_map(|sample| sample.to_le_bytes())
            .collect()
    }

    #[test]
    fn a_gap_is_silence_of_whole_frames_and_never_past_the_end() {
        let recording = Recording::holding(8);
        let mut recorder = Recorder::admitting(Arc::clone(&recording));
        recorder.take(&bytes_of(&[0.5, -0.5, 0.25]));

        recording.silence_for(1, 2);
        assert_eq!(recording.filled(), 6);
        assert_eq!(recording.taken(), vec![0.5, -0.5, 0.25, 0.0, 0.0, 0.0]);

        recording.silence_for(100, 2);
        assert!(recording.is_full());
        assert_eq!(recording.filled(), 8);
    }

    #[test]
    fn a_recorder_held_back_hears_nothing_until_it_is_admitted() {
        let recording = Recording::holding(4);
        let (mut recorder, admitting) = Recorder::held_back(Arc::clone(&recording));

        recorder.take(&bytes_of(&[1.0, 1.0]));
        assert_eq!(recording.filled(), 0);

        admitting.store(true, Ordering::Release);
        recorder.take(&bytes_of(&[0.5]));
        assert_eq!(recording.taken(), vec![0.5]);
    }
}
