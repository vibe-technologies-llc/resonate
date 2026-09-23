#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicUsize, Ordering},
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

pub(crate) struct Recorder(pub(crate) Arc<Recording>);

impl AudioSink for Recorder {
    fn take(&mut self, bytes: &[u8]) {
        self.0.keep(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_keeps_what_it_is_handed_and_stops_when_full() {
        let recording = Recording::holding(3);
        let mut recorder = Recorder(Arc::clone(&recording));
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
}
