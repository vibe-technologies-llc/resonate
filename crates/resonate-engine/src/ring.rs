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
    time::Duration,
};

use resonate_core::{AudioBuffer, Frames, RtFault, SampleFormat, Silence, StreamSpec};
use resonate_pipewire::AudioSource;

const FAULT_SLOTS: usize = 64;

pub const FADED_OVER: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entering {
    Whole,
    FadedIn,
}

pub fn ring(
    spec: StreamSpec,
    capacity: Frames,
    silence: Silence,
    fade: Frames,
    entering: Entering,
) -> (RingProducer, RingConsumer, RingMonitor) {
    let bytes_per_frame = spec.bytes_per_frame();
    let bytes = spec.frames_to_bytes(capacity) as usize;
    let (producer, consumer) = rtrb::RingBuffer::new(bytes);
    let (faults, drained) = rtrb::RingBuffer::new(FAULT_SLOTS);
    let dropped = Arc::new(AtomicU32::new(0));
    let starved = Arc::new(AtomicU64::new(0));
    let discard = Discard::default();
    let hold = Hold::default();
    let fader = Fader::default();
    fader.over.store(fade.get().max(1), Ordering::Release);
    let prime = bytes / bytes_per_frame.get() as usize / 2;
    let fades = matches!(silence, Silence::Unmarked) && fade > Frames::ZERO;
    let level = match entering {
        Entering::FadedIn if fades => SILENT,
        Entering::Whole | Entering::FadedIn => WHOLE,
    };

    (
        RingProducer {
            inner: producer,
            bytes_per_frame,
            discard: discard.clone(),
            hold: hold.clone(),
            fader: fader.clone(),
            requested: 0,
        },
        RingConsumer {
            inner: consumer,
            bytes_per_frame,
            discard,
            hold,
            fader,
            format: spec.format,
            fades,
            level,
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

#[derive(Clone, Default)]
struct Hold {
    lead_in: Arc<AtomicU64>,
}

#[derive(Clone, Default)]
struct Fader {
    silenced: Arc<AtomicBool>,
    over: Arc<AtomicU64>,
    quiet: Arc<AtomicBool>,
    pulls: Arc<AtomicU64>,
}

const WHOLE: f64 = 1.0;
const SILENT: f64 = 0.0;

pub struct RingProducer {
    inner: rtrb::Producer<u8>,
    bytes_per_frame: NonZeroU32,
    discard: Discard,
    hold: Hold,
    fader: Fader,
    requested: u64,
}

impl RingProducer {
    pub fn fade_out(&self, over: Frames) {
        self.fader.over.store(over.get().max(1), Ordering::Release);
        self.fader.silenced.store(true, Ordering::Release);
    }

    pub fn fade_in(&self, over: Frames) {
        self.fader.over.store(over.get().max(1), Ordering::Release);
        self.fader.quiet.store(false, Ordering::Release);
        self.fader.silenced.store(false, Ordering::Release);
    }

    pub fn is_fading_out(&self) -> bool {
        self.fader.silenced.load(Ordering::Acquire)
    }

    pub fn is_quiet(&self) -> bool {
        self.fader.quiet.load(Ordering::Acquire)
    }

    pub fn pulls(&self) -> u64 {
        self.fader.pulls.load(Ordering::Relaxed)
    }

    pub fn lead_in(&self, frames: Frames) {
        self.hold.lead_in.store(frames.0, Ordering::Release);
    }

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
    hold: Hold,
    fader: Fader,
    format: SampleFormat,
    fades: bool,
    level: f64,
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

    fn discard_is_asked(&self) -> bool {
        self.discard.requested.load(Ordering::Acquire) != self.seen
    }

    fn fades_before_the_discard(&self) -> bool {
        self.fades && !self.silent && self.level > SILENT && self.available_frames() > 0
    }

    fn apply_discard(&mut self) {
        let requested = self.discard.requested.load(Ordering::Acquire);
        if requested == self.seen {
            return;
        }
        self.seen = requested;
        if self.fades {
            self.level = SILENT;
        }
        self.silent = true;

        if let Ok(stale) = self.inner.read_chunk(self.inner.slots()) {
            stale.commit_all();
        }
        self.discard.applied.store(requested, Ordering::Release);
    }

    fn held(&self, frames: usize) -> bool {
        let lead = self.hold.lead_in.load(Ordering::Acquire);
        if lead == 0 {
            return false;
        }
        self.hold
            .lead_in
            .store(lead.saturating_sub(frames as u64), Ordering::Release);
        true
    }

    fn hush(&mut self, dst: &mut [u8]) -> usize {
        if matches!(self.silence, Silence::Unmarked) {
            let whole = dst.len() / self.stride() * self.stride();
            if let Some(frames) = dst.get_mut(..whole) {
                frames.fill(0);
            }
            return whole;
        }
        self.pad(dst)
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
        self.fader.pulls.fetch_add(1, Ordering::Relaxed);
        if self.discard_is_asked() && self.fades_before_the_discard() {
            return self.fade_into_the_discard(dst);
        }
        self.apply_discard();
        if self.held(dst.len() / self.stride()) {
            return self.hush(dst);
        }
        if self.refilling() {
            return self.pad(dst);
        }
        let silenced = self.fader.silenced.load(Ordering::Acquire);
        if silenced && (!self.fades || self.level <= SILENT) {
            self.fader.quiet.store(true, Ordering::Release);
            return self.hush(dst);
        }

        let stride = self.stride();
        let spliced = self.realigned(dst);
        let Some(rest) = dst.get_mut(spliced..) else {
            return spliced;
        };

        let wanted = rest.len() / stride;
        let most = if silenced {
            self.frames_until_silent()
        } else {
            wanted
        };
        let frames = wanted.min(self.available_frames()).min(most);
        let Some(whole) = rest.get_mut(..frames.saturating_mul(stride)) else {
            return spliced;
        };

        let (popped, _) = self.inner.pop_partial_slice(whole);
        let target = if silenced { SILENT } else { WHOLE };
        self.ramp(popped, target);
        let taken = popped.len() / stride;
        let filled = spliced.saturating_add(popped.len());
        if let Some(last) = popped
            .len()
            .checked_sub(stride)
            .and_then(|from| popped.get(from..))
        {
            self.silence.follows(last);
        }

        if silenced && self.level <= SILENT {
            self.fader.quiet.store(true, Ordering::Release);
            return match dst.get_mut(filled..) {
                Some(gap) => filled.saturating_add(self.hush(gap)),
                None => filled,
            };
        }
        if taken == wanted {
            return filled;
        }

        if self.discard.finished.load(Ordering::Acquire) {
            self.faults.raise(RtFault::Underrun);
            return filled;
        }
        self.starved
            .fetch_add(wanted.saturating_sub(taken) as u64, Ordering::Release);
        self.faults.raise(RtFault::Underrun);
        match dst.get_mut(filled..) {
            Some(gap) => filled.saturating_add(self.pad(gap)),
            None => filled,
        }
    }
}

impl RingConsumer {
    fn step(&self) -> f64 {
        WHOLE / self.fader.over.load(Ordering::Acquire).max(1) as f64
    }

    fn frames_until_silent(&self) -> usize {
        (self.level / self.step()).ceil() as usize
    }

    fn fade_into_the_discard(&mut self, dst: &mut [u8]) -> usize {
        let stride = self.stride();
        let frames = (dst.len() / stride)
            .min(self.available_frames())
            .min(self.frames_until_silent());
        let faded = match dst.get_mut(..frames.saturating_mul(stride)) {
            Some(whole) => {
                let (popped, _) = self.inner.pop_partial_slice(whole);
                self.ramp(popped, SILENT);
                popped.len()
            }
            None => 0,
        };
        if self.level <= SILENT || self.available_frames() == 0 {
            self.apply_discard();
        }
        match dst.get_mut(faded..) {
            Some(gap) => faded.saturating_add(self.hush(gap)),
            None => faded,
        }
    }

    fn ramp(&mut self, frames: &mut [u8], target: f64) {
        if !self.fades {
            return;
        }
        let step = self.step();
        let stride = self.stride();
        for frame in frames.chunks_exact_mut(stride) {
            if self.level >= WHOLE && target >= WHOLE {
                return;
            }
            self.level = if target > self.level {
                (self.level + step).min(target)
            } else {
                (self.level - step).max(target)
            };
            scale(frame, self.format, self.level);
        }
    }
}

fn scale(frame: &mut [u8], format: SampleFormat, level: f64) {
    match format {
        SampleFormat::S16 => {
            for sample in frame.as_chunks_mut::<2>().0 {
                let scaled = (f64::from(i16::from_ne_bytes(*sample)) * level).round() as i16;
                *sample = scaled.to_ne_bytes();
            }
        }
        SampleFormat::S24 | SampleFormat::S32 => {
            for sample in frame.as_chunks_mut::<4>().0 {
                let scaled = (f64::from(i32::from_ne_bytes(*sample)) * level).round() as i32;
                *sample = scaled.to_ne_bytes();
            }
        }
        SampleFormat::F32 => {
            for sample in frame.as_chunks_mut::<4>().0 {
                let scaled = (f64::from(f32::from_ne_bytes(*sample)) * level) as f32;
                *sample = scaled.to_ne_bytes();
            }
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
        Frames(self.starved.swap(0, Ordering::Acquire))
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
    const UNFADED: Frames = Frames::ZERO;
    const FADED_OVER: Frames = Frames(4);

    fn steady(frames: usize) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S16), frames);
        if let SampleData::S16(store) = buffer.data_mut() {
            store.fill(1_000);
        }
        buffer
    }

    fn left_of(sunk: &[u8]) -> Vec<i16> {
        sunk.as_chunks::<4>()
            .0
            .iter()
            .map(|frame| i16::from_ne_bytes([frame[0], frame[1]]))
            .collect()
    }

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
        let (producer, consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(1024),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );

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
        let (producer, consumer, _) = ring(
            spec,
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
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
            let (mut producer, mut consumer, _) = ring(
                spec(format),
                Frames(256),
                Silence::Unmarked,
                UNFADED,
                Entering::Whole,
            );

            assert_eq!(producer.write(&source), 128);

            let mut sunk = vec![0_u8; source.as_bytes().len()];
            assert_eq!(consumer.fill(&mut sunk), sunk.len());
            assert_eq!(sunk, source.as_bytes(), "{format} did not survive the ring");
        }
    }

    #[test]
    fn a_write_that_does_not_fit_stops_on_a_frame_boundary() {
        let source = ramp(SampleFormat::S16, 100);
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );

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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S32),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );

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
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
        producer.write(&ramp(SampleFormat::S16, 10));

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 10 * 4);

        assert_eq!(faults.next_fault(), Some(RtFault::Underrun));
        assert_eq!(faults.next_fault(), None);
        assert_eq!(faults.went_without(), Frames(22));
    }

    #[test]
    fn a_full_read_raises_nothing() {
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
        producer.write(&ramp(SampleFormat::S16, 32));

        let mut sunk = vec![0_u8; 32 * 4];
        consumer.fill(&mut sunk);

        assert_eq!(faults.next_fault(), None);
    }

    #[test]
    fn faults_beyond_the_queues_depth_are_counted_rather_than_lost() {
        let (_producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );

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
        let (mut producer, mut consumer, faults) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
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
    fn a_ring_faded_out_ramps_to_silence_then_keeps_every_frame_it_still_holds() {
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            FADED_OVER,
            Entering::Whole,
        );
        producer.write(&steady(64));
        producer.fade_out(FADED_OVER);

        let mut sunk = vec![0xff_u8; 8 * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(left_of(&sunk), vec![750, 500, 250, 0, 0, 0, 0, 0]);
        assert!(producer.is_quiet());
        assert_eq!(consumer.available_frames(), 60);

        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert!(left_of(&sunk).iter().all(|sample| *sample == 0));
        assert_eq!(consumer.available_frames(), 60, "a quiet ring played on");

        producer.fade_in(FADED_OVER);
        assert!(!producer.is_quiet());
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(
            left_of(&sunk),
            vec![250, 500, 750, 1_000, 1_000, 1_000, 1_000, 1_000]
        );
        assert_eq!(faults.next_fault(), None);
    }

    #[test]
    fn a_seek_fades_out_what_the_ring_held_and_fades_in_what_follows() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            FADED_OVER,
            Entering::Whole,
        );
        producer.write(&steady(128));
        producer.discard_buffered();

        let mut sunk = vec![0xff_u8; 8 * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(left_of(&sunk), vec![750, 500, 250, 0, 0, 0, 0, 0]);
        assert!(!producer.is_discarding());

        assert_eq!(producer.write(&steady(128)), 128);
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(
            left_of(&sunk),
            vec![250, 500, 750, 1_000, 1_000, 1_000, 1_000, 1_000]
        );
    }

    #[test]
    fn a_ring_entering_mid_track_fades_in_and_one_entering_whole_does_not() {
        for (entering, opens_on) in [(Entering::FadedIn, 250), (Entering::Whole, 1_000)] {
            let (mut producer, mut consumer, _) = ring(
                spec(SampleFormat::S16),
                Frames(64),
                Silence::Unmarked,
                FADED_OVER,
                entering,
            );
            producer.write(&steady(16));

            let mut sunk = vec![0_u8; 4 * 4];
            consumer.fill(&mut sunk);
            assert_eq!(left_of(&sunk).first().copied(), Some(opens_on));
        }
    }

    #[test]
    fn a_marked_ring_is_never_scaled_and_goes_quiet_on_its_markers() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            FADED_OVER,
            Entering::FadedIn,
        );
        let carried = dop(16, 0);
        producer.write(&carried);

        let mut sunk = vec![0_u8; 8 * 8];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(sunk, carried.as_bytes()[..sunk.len()]);

        producer.fade_out(FADED_OVER);
        let mut hushed = vec![0_u8; 4 * 8];
        assert_eq!(consumer.fill(&mut hushed), hushed.len());
        assert!(producer.is_quiet());
        assert_eq!(
            consumer.available_frames(),
            8,
            "a marked ring played on when faded"
        );
        assert!(
            hushed.iter().any(|byte| *byte != 0),
            "a marked ring went quiet on zeros"
        );
    }

    #[test]
    fn a_lead_in_is_silence_before_the_first_frame_and_then_the_ring_plays() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
        producer.write(&ramp(SampleFormat::S16, 64));
        producer.lead_in(Frames(48));

        let mut sunk = vec![0xff_u8; 32 * 4];
        consumer.fill(&mut sunk);
        assert!(sunk.iter().all(|byte| *byte == 0));
        consumer.fill(&mut sunk);
        assert!(sunk.iter().all(|byte| *byte == 0));
        assert_eq!(
            consumer.available_frames(),
            64,
            "the lead-in spent the music"
        );

        consumer.fill(&mut sunk);
        assert_eq!(consumer.available_frames(), 32);
    }

    #[test]
    fn a_tail_shorter_than_the_priming_mark_still_reaches_the_graph() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
        producer.discard_buffered();

        let mut sunk = vec![0_u8; 32 * 4];
        assert_eq!(consumer.fill(&mut sunk), 0);

        producer.write(&ramp(SampleFormat::S16, 8));
        producer.finish();

        assert_eq!(consumer.fill(&mut sunk), 8 * 4);
    }

    #[test]
    fn a_marked_ring_refilling_after_a_discard_carries_its_markers_through_the_gap() {
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
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
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
        );
        producer.discard_buffered();

        let mut sunk = vec![0xAA_u8; 32 * 8];
        assert_eq!(consumer.fill(&mut sunk), 0);
        assert!(sunk.iter().all(|byte| *byte == 0xAA));
    }
}
