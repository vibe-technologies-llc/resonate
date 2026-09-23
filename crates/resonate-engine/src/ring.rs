#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
};

use resonate_core::{AudioBuffer, Frames, RtFault, SampleFormat, Silence, StreamSpec};
use resonate_pipewire::AudioSource;

const FAULT_SLOTS: usize = 64;

pub fn ring(
    spec: StreamSpec,
    capacity: Frames,
    silence: Silence,
) -> (RingProducer, RingConsumer, RingMonitor) {
    let bytes_per_frame = spec.bytes_per_frame();
    let bytes = spec.frames_to_bytes(capacity) as usize;
    let (producer, consumer) = rtrb::RingBuffer::new(bytes);
    let (faults, drained) = rtrb::RingBuffer::new(FAULT_SLOTS);
    let dropped = Arc::new(AtomicU32::new(0));
    let starved = Arc::new(AtomicU64::new(0));
    let discard = Discard::default();
    let prime = bytes / bytes_per_frame.get() as usize / 2;

    (
        RingProducer {
            inner: producer,
            bytes_per_frame,
            discard: discard.clone(),
            requested: 0,
        },
        RingConsumer {
            inner: consumer,
            bytes_per_frame,
            discard,
            seen: 0,
            silent: false,
            silence,
            padded: false,
            prime,
            faults: FaultSink {
                queue: faults,
                dropped: Arc::clone(&dropped),
            },
            starved: Arc::clone(&starved),
        },
        RingMonitor {
            queue: drained,
            dropped,
            starved,
        },
    )
}

#[derive(Clone, Default)]
struct Discard {
    requested: Arc<AtomicU64>,
    applied: Arc<AtomicU64>,
    finished: Arc<AtomicBool>,
}

pub struct RingProducer {
    inner: rtrb::Producer<u8>,
    bytes_per_frame: NonZeroU32,
    discard: Discard,
    requested: u64,
}

impl RingProducer {
    pub fn discard_buffered(&mut self) {
        self.requested = self
            .discard
            .requested
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
    }

    pub fn is_discarding(&self) -> bool {
        self.discard.applied.load(Ordering::Acquire) < self.requested
    }

    pub fn finish(&mut self) {
        self.discard.finished.store(true, Ordering::Release);
    }

    pub fn write(&mut self, buffer: &AudioBuffer) -> usize {
        self.write_from(buffer, 0)
    }

    pub fn write_from(&mut self, buffer: &AudioBuffer, start: usize) -> usize {
        if self.is_discarding() {
            return 0;
        }
        let stride = self.bytes_per_frame.get() as usize;
        let Some(tail) = buffer.as_bytes().get(start.saturating_mul(stride)..) else {
            return 0;
        };
        let frames = (tail.len() / stride).min(self.free_frames());
        let Some(whole) = tail.get(..frames.saturating_mul(stride)) else {
            return 0;
        };
        let (pushed, _) = self.inner.push_partial_slice(whole);
        pushed.len() / stride
    }

    pub fn free_frames(&self) -> usize {
        self.inner.slots() / self.bytes_per_frame.get() as usize
    }

    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

pub struct RingConsumer {
    inner: rtrb::Consumer<u8>,
    bytes_per_frame: NonZeroU32,
    discard: Discard,
    seen: u64,
    silent: bool,
    silence: Silence,
    padded: bool,
    prime: usize,
    faults: FaultSink,
    starved: Arc<AtomicU64>,
}

impl RingConsumer {
    pub fn available_frames(&self) -> usize {
        self.inner.slots() / self.bytes_per_frame.get() as usize
    }

    fn apply_discard(&mut self) {
        let requested = self.discard.requested.load(Ordering::Acquire);
        if requested == self.seen {
            return;
        }
        self.seen = requested;
        self.silent = true;

        if let Ok(stale) = self.inner.read_chunk(self.inner.slots()) {
            stale.commit_all();
        }
        self.discard.applied.store(requested, Ordering::Release);
    }

    fn refilling(&mut self) -> bool {
        if !self.silent {
            return false;
        }
        if self.available_frames() < self.prime && !self.discard.finished.load(Ordering::Acquire) {
            return true;
        }
        self.silent = false;
        false
    }

    const fn stride(&self) -> usize {
        self.bytes_per_frame.get() as usize
    }

    fn pad(&mut self, gap: &mut [u8]) -> usize {
        let written = self.silence.write(gap, self.bytes_per_frame);
        self.padded |= written > 0;
        written
    }

    fn realigned(&mut self, dst: &mut [u8]) -> usize {
        if !self.padded || self.available_frames() == 0 {
            return 0;
        }
        self.padded = false;
        if self.splices_cleanly() {
            return 0;
        }
        match dst.get_mut(..self.stride()) {
            Some(frame) => self.silence.write(frame, self.bytes_per_frame),
            None => 0,
        }
    }

    fn splices_cleanly(&mut self) -> bool {
        let stride = self.stride();
        let silence = self.silence;
        let Ok(waiting) = self.inner.read_chunk(stride) else {
            return true;
        };
        let (head, tail) = waiting.as_slices();
        let mut sample = [0_u8; SampleFormat::MAX_BYTES];
        for (slot, byte) in sample.iter_mut().zip(head.iter().chain(tail)) {
            *slot = *byte;
        }

        silence.would_write(&sample)
    }
}

impl AudioSource for RingConsumer {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        self.apply_discard();
        if self.refilling() {
            return self.pad(dst);
        }

        let stride = self.stride();
        let spliced = self.realigned(dst);
        let Some(rest) = dst.get_mut(spliced..) else {
            return spliced;
        };

        let wanted = rest.len() / stride;
        let frames = wanted.min(self.available_frames());
        let Some(whole) = rest.get_mut(..frames.saturating_mul(stride)) else {
            return spliced;
        };

        let (popped, _) = self.inner.pop_partial_slice(whole);
        let taken = popped.len() / stride;
        let filled = spliced.saturating_add(popped.len());
        if let Some(last) = popped
            .len()
            .checked_sub(stride)
            .and_then(|from| popped.get(from..))
        {
            self.silence.follows(last);
        }

        if taken == wanted {
            return filled;
        }

        self.faults.raise(RtFault::Underrun);

        if self.discard.finished.load(Ordering::Acquire) {
            return filled;
        }
        self.starved
            .fetch_add(wanted.saturating_sub(taken) as u64, Ordering::Relaxed);
        match dst.get_mut(filled..) {
            Some(gap) => filled.saturating_add(self.pad(gap)),
            None => filled,
        }
    }
}

struct FaultSink {
    queue: rtrb::Producer<RtFault>,
    dropped: Arc<AtomicU32>,
}

impl FaultSink {
    fn raise(&mut self, fault: RtFault) {
        if self.queue.push(fault).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub struct RingMonitor {
    queue: rtrb::Consumer<RtFault>,
    dropped: Arc<AtomicU32>,
    starved: Arc<AtomicU64>,
}

impl RingMonitor {
    pub fn went_without(&self) -> Frames {
        Frames(self.starved.swap(0, Ordering::Relaxed))
    }

    pub fn next_fault(&mut self) -> Option<RtFault> {
        if let Ok(fault) = self.queue.pop() {
            return Some(fault);
        }
        match self.dropped.swap(0, Ordering::Relaxed) {
            0 => None,
            count => Some(RtFault::Dropped { count }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use resonate_codec::{DsdRate, Packing};
    use resonate_core::{ChannelLayout, SampleData, SampleFormat, SampleRate};

    use super::*;

    const DOP_SILENT_PAIR: [u8; 2] = [0x69, 0x69];

    fn spec(format: SampleFormat) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Stereo, format)
    }

    fn marked() -> Silence {
        Packing::DopMarked(DsdRate::new(2_822_400).expect("dsd64")).silence(SampleFormat::S24)
    }

    fn ramp(format: SampleFormat, frames: usize) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(spec(format), frames);
        match buffer.data_mut() {
            SampleData::S16(store) => {
                for (index, slot) in store.iter_mut().enumerate() {
                    *slot = index as i16;
                }
            }
            SampleData::S24(store) | SampleData::S32(store) => {
                for (index, slot) in store.iter_mut().enumerate() {
                    *slot = index as i32;
                }
            }
            SampleData::F32(store) => {
                for (index, slot) in store.iter_mut().enumerate() {
                    *slot = index as f32;
                }
            }
        }
        buffer
    }

    #[test]
    fn capacity_is_reported_in_frames_not_bytes() {
        let (producer, consumer, _) =
            ring(spec(SampleFormat::S24), Frames(1024), Silence::Unmarked);

        assert_eq!(producer.free_frames(), 1024);
        assert_eq!(consumer.available_frames(), 0);
        assert!(!producer.is_abandoned());
    }

    #[test]
    fn dropping_one_end_is_visible_to_the_other() {
        let spec = StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::F32,
        );
        let (producer, consumer, _) = ring(spec, Frames(256), Silence::Unmarked);
        drop(consumer);
        assert!(producer.is_abandoned());
    }

    #[test]
    fn every_byte_survives_the_round_trip_for_all_formats() {
        for format in [
            SampleFormat::S16,
            SampleFormat::S24,
            SampleFormat::S32,
            SampleFormat::F32,
        ] {
            let source = ramp(format, 128);
            let (mut producer, mut consumer, _) =
                ring(spec(format), Frames(256), Silence::Unmarked);

            assert_eq!(producer.write(&source), 128);

            let mut sunk = vec![0_u8; source.as_bytes().len()];
            assert_eq!(consumer.fill(&mut sunk), sunk.len());
            assert_eq!(sunk, source.as_bytes(), "{format} did not survive the ring");
        }
    }

    #[test]
    fn a_write_that_does_not_fit_stops_on_a_frame_boundary() {
        let source = ramp(SampleFormat::S16, 100);
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S16), Frames(64), Silence::Unmarked);

        let written = producer.write(&source);
        assert!(written < 100);
        assert_eq!(consumer.available_frames(), written);

        let mut sunk = vec![0_u8; written * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(sunk, &source.as_bytes()[..written * 4]);
    }

    #[test]
    fn write_from_resumes_where_the_previous_write_stopped() {
        let source = ramp(SampleFormat::S32, 200);
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S32), Frames(64), Silence::Unmarked);

        let mut written = 0;
        let mut sunk = Vec::new();
        while written < 200 {
            written += producer.write_from(&source, written);
            let mut block = vec![0_u8; consumer.available_frames() * 8];
            consumer.fill(&mut block);
            sunk.extend_from_slice(&block);
        }

        assert_eq!(sunk, source.as_bytes());
    }

    #[test]
    fn a_short_read_raises_exactly_one_underrun_and_counts_what_was_missing() {
        let (mut producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S16), Frames(64), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 10));

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 10 * 4);

        assert_eq!(faults.next_fault(), Some(RtFault::Underrun));
        assert_eq!(faults.next_fault(), None);
        assert_eq!(faults.went_without(), Frames(22));
    }

    #[test]
    fn a_full_read_raises_nothing() {
        let (mut producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S16), Frames(64), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 32));

        let mut sunk = vec![0_u8; 32 * 4];
        consumer.fill(&mut sunk);

        assert_eq!(faults.next_fault(), None);
    }

    #[test]
    fn faults_beyond_the_queues_depth_are_counted_rather_than_lost() {
        let (_producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S16), Frames(64), Silence::Unmarked);

        let mut sunk = vec![0_u8; 4];
        for _ in 0..FAULT_SLOTS + 10 {
            consumer.fill(&mut sunk);
        }

        let mut seen = 0;
        let mut dropped = 0;
        while let Some(fault) = faults.next_fault() {
            match fault {
                RtFault::Dropped { count } => dropped += count,
                _ => seen += 1,
            }
        }

        assert_eq!(seen + dropped as usize, FAULT_SLOTS + 10);
        assert_eq!(dropped, 10);
        assert_eq!(
            faults.went_without(),
            Frames((FAULT_SLOTS + 10) as u64),
            "a frame the graph went without was lost with the fault that carried it"
        );
    }

    #[test]
    fn what_a_starve_cost_is_counted_and_the_end_of_a_track_is_not_a_starve() {
        let (mut producer, mut consumer, faults) =
            ring(spec(SampleFormat::S16), Frames(64), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 10));

        let mut sunk = vec![0_u8; 32 * 4];
        consumer.fill(&mut sunk);
        assert_eq!(faults.went_without(), Frames(22));
        assert_eq!(
            faults.went_without(),
            Frames::ZERO,
            "a starve was handed over twice"
        );

        producer.write(&ramp(SampleFormat::S16, 4));
        producer.finish();
        consumer.fill(&mut sunk);
        assert_eq!(
            faults.went_without(),
            Frames::ZERO,
            "the short chunk a track ends on was counted as a starve"
        );
    }

    #[test]
    fn a_discard_drops_what_the_consumer_has_not_read_and_nothing_the_producer_writes_after() {
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S16), Frames(256), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 128));

        producer.discard_buffered();
        assert!(producer.is_discarding());
        assert_eq!(producer.write(&ramp(SampleFormat::S16, 8)), 0);

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 0);
        assert!(!producer.is_discarding());

        let fresh = ramp(SampleFormat::S16, 128);
        assert_eq!(producer.write(&fresh), 128);
        let mut sunk = vec![0_u8; 128 * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(sunk, fresh.as_bytes());
    }

    #[test]
    fn a_ring_refilling_after_a_discard_plays_silence_rather_than_a_stutter() {
        let (mut producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S16), Frames(256), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 200));
        producer.discard_buffered();

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 0);

        producer.write(&ramp(SampleFormat::S16, 127));
        assert_eq!(
            consumer.fill(&mut sunk),
            0,
            "the ring played before it primed"
        );

        producer.write(&ramp(SampleFormat::S16, 1));
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(faults.next_fault(), None);
    }

    #[test]
    fn a_tail_shorter_than_the_priming_mark_still_reaches_the_graph() {
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S16), Frames(256), Silence::Unmarked);
        producer.discard_buffered();

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 0);

        producer.write(&ramp(SampleFormat::S16, 8));
        producer.finish();

        assert_eq!(consumer.fill(&mut sunk), 8 * 4);
    }

    #[test]
    fn a_marked_ring_refilling_after_a_discard_carries_its_markers_through_the_gap() {
        let (mut producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S24), Frames(256), marked());
        producer.write(&ramp(SampleFormat::S24, 200));
        producer.discard_buffered();

        let mut sunk = vec![0_u8; 32 * 8];
        assert_eq!(
            consumer.fill(&mut sunk),
            sunk.len(),
            "the prime window handed the graph a gap rather than silence"
        );
        let lanes = usize::from(spec(SampleFormat::S24).channel_count().get());
        for (index, word) in sunk.as_chunks::<4>().0.iter().enumerate() {
            let frame = index / lanes;
            let wanted = if frame % 2 == 0 { 0x05 } else { 0xFA };
            assert_eq!(
                word[2], wanted,
                "word {index} left the marker a DAC locks onto"
            );
            assert_eq!(&word[..2], &DOP_SILENT_PAIR);
        }

        producer.write(&ramp(SampleFormat::S24, 128));
        let fresh = ramp(SampleFormat::S24, 32);
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(
            sunk,
            fresh.as_bytes(),
            "the ring went on marking once it had primed"
        );
        assert_eq!(
            faults.next_fault(),
            None,
            "the marked gap was reported as a fault"
        );
    }

    #[test]
    fn a_marked_ring_keeps_its_parity_across_the_callbacks_of_one_gap() {
        let (mut producer, mut consumer, _) = ring(spec(SampleFormat::S24), Frames(256), marked());
        producer.discard_buffered();

        let mut first = vec![0_u8; 8];
        let mut second = vec![0_u8; 8];
        assert_eq!(consumer.fill(&mut first), first.len());
        assert_eq!(consumer.fill(&mut second), second.len());

        assert_eq!(first[2], 0x05);
        assert_eq!(
            second[2], 0xFA,
            "the second callback started the marker again"
        );
    }

    fn dop(frames: usize, from: u64) -> AudioBuffer {
        const HELD: u32 = 0x1234;
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S24), frames);
        let lanes = usize::from(spec(SampleFormat::S24).channel_count().get());
        if let SampleData::S24(store) = buffer.data_mut() {
            for (index, slot) in store.iter_mut().enumerate() {
                let frame = from.saturating_add((index / lanes) as u64);
                let marker = if frame.is_multiple_of(2) {
                    0x05_u32
                } else {
                    0xFA
                };
                *slot = (((marker << 16 | HELD) << 8) as i32) >> 8;
            }
        }
        buffer
    }

    fn markers(sunk: &[u8]) -> Vec<u8> {
        let lanes = usize::from(spec(SampleFormat::S24).channel_count().get());
        sunk.as_chunks::<4>()
            .0
            .iter()
            .step_by(lanes)
            .map(|word| word[2])
            .collect()
    }

    fn alternating(from: u8, frames: usize) -> Vec<u8> {
        (0..frames)
            .map(|frame| {
                if frame.is_multiple_of(2) {
                    from
                } else if from == 0x05 {
                    0xFA
                } else {
                    0x05
                }
            })
            .collect()
    }

    #[test]
    fn a_marked_ring_starved_mid_track_covers_the_gap_rather_than_handing_over_a_short_chunk() {
        let (mut producer, mut consumer, mut faults) =
            ring(spec(SampleFormat::S24), Frames(256), marked());
        producer.write(&dop(8, 0));

        let mut sunk = vec![0_u8; 16 * 8];
        assert_eq!(
            consumer.fill(&mut sunk),
            sunk.len(),
            "a starved read handed the graph a short chunk"
        );
        assert_eq!(markers(&sunk), alternating(0x05, 16));
        assert_eq!(
            faults.next_fault(),
            Some(RtFault::Underrun),
            "the starve was covered without being reported"
        );
        assert_eq!(faults.went_without(), Frames(8));
    }

    #[test]
    fn a_marked_ring_at_the_end_of_a_track_hands_over_the_short_chunk_it_holds() {
        let (mut producer, mut consumer, _) = ring(spec(SampleFormat::S24), Frames(256), marked());
        producer.write(&dop(8, 0));
        producer.finish();

        let mut sunk = vec![0_u8; 16 * 8];
        assert_eq!(
            consumer.fill(&mut sunk),
            8 * 8,
            "the tail of a track was padded rather than let run out"
        );
    }

    #[test]
    fn audio_spliced_back_after_a_starve_lands_on_the_marker_that_follows() {
        let (mut producer, mut consumer, _) = ring(spec(SampleFormat::S24), Frames(256), marked());
        producer.write(&dop(8, 0));

        let mut first = vec![0_u8; 15 * 8];
        assert_eq!(consumer.fill(&mut first), first.len());

        producer.write(&dop(15, 8));
        let mut second = vec![0_u8; 15 * 8];
        assert_eq!(consumer.fill(&mut second), second.len());

        let mut heard = markers(&first);
        heard.extend(markers(&second));
        assert_eq!(
            heard,
            alternating(0x05, 30),
            "the splice put two like markers together"
        );

        assert_eq!(
            &second[8..],
            dop(14, 8).as_bytes(),
            "a frame of the audio was lost to the splice"
        );
    }

    #[test]
    fn an_unmarked_ring_starved_mid_track_is_handed_over_exactly_as_it_was() {
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S16), Frames(256), Silence::Unmarked);
        producer.write(&ramp(SampleFormat::S16, 8));

        let mut sunk = vec![0xAA_u8; 16 * 4];
        assert_eq!(consumer.fill(&mut sunk), 8 * 4);
        assert!(
            sunk[8 * 4..].iter().all(|byte| *byte == 0xAA),
            "an unmarked starve wrote into the graph's buffer"
        );
    }

    #[test]
    fn an_unmarked_ring_leaves_the_prime_window_to_the_graph() {
        let (mut producer, mut consumer, _) =
            ring(spec(SampleFormat::S24), Frames(256), Silence::Unmarked);
        producer.discard_buffered();

        let mut sunk = vec![0xAA_u8; 32 * 8];
        assert_eq!(consumer.fill(&mut sunk), 0);
        assert!(sunk.iter().all(|byte| *byte == 0xAA));
    }
}
