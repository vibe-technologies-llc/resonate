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

use resonate_audio::AudioSource;
use resonate_core::{AudioBuffer, Frames, RtFault, SampleFormat, Silence, StreamSpec};

const FAULT_SLOTS: usize = 64;
const RENDERED_SLOTS: usize = 1024;
const RINGS_OF_ROOM_A_SPLICE_MAY_TAKE: usize = 1;

pub const FADED_OVER: Duration = Duration::from_millis(10);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entering {
    Whole,
    FadedIn,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Level {
    pub rendered: f32,
    pub heard: f32,
}

impl Level {
    pub const WHOLE: Self = Self {
        rendered: 1.0,
        heard: 1.0,
    };
}

#[derive(Clone, Copy, Debug)]
struct Rendered {
    epoch: u64,
    from: u64,
    amplitude: f32,
}

pub fn ring(
    spec: StreamSpec,
    capacity: Frames,
    silence: Silence,
    fade: Frames,
    entering: Entering,
    level: Level,
) -> (RingProducer, RingConsumer, RingMonitor) {
    let bytes_per_frame = spec.bytes_per_frame();
    let bytes = spec.frames_to_bytes(capacity) as usize;
    let room = bytes.saturating_mul(1 + RINGS_OF_ROOM_A_SPLICE_MAY_TAKE);
    let (producer, consumer) = rtrb::RingBuffer::new(room);
    let (faults, drained) = rtrb::RingBuffer::new(FAULT_SLOTS);
    let (announcing, announced) = rtrb::RingBuffer::new(RENDERED_SLOTS);
    let heard = Arc::new(AtomicU32::new(level.heard.to_bits()));
    let dropped = Arc::new(AtomicU32::new(0));
    let starved = Arc::new(AtomicU64::new(0));
    let discard = Discard::default();
    let splice = Splice::default();
    let hold = Hold::default();
    let fader = Fader::default();
    fader.over.store(fade.get().max(1), Ordering::Release);
    let prime = bytes / bytes_per_frame.get() as usize / 2;
    let fades = matches!(silence, Silence::Unmarked) && fade > Frames::ZERO;
    let faded = match entering {
        Entering::FadedIn if fades => SILENT,
        Entering::Whole | Entering::FadedIn => WHOLE,
    };

    (
        RingProducer {
            inner: producer,
            bytes_per_frame,
            format: spec.format,
            capacity: capacity.get() as usize,
            mirror: vec![0; room],
            splice: splice.clone(),
            splices_asked: 0,
            splicing: None,
            spliced_away: 0,
            discard: discard.clone(),
            hold: hold.clone(),
            fader: fader.clone(),
            requested: 0,
            written: 0,
            announcing,
            owed: None,
            rendering: level.rendered,
            heard: Arc::clone(&heard),
        },
        RingConsumer {
            inner: consumer,
            bytes_per_frame,
            discard,
            splice,
            splices_seen: 0,
            pending: None,
            hold,
            fader,
            format: spec.format,
            fades,
            level: faded,
            announced,
            heard,
            read: 0,
            rendered_at: f64::from(level.rendered),
            heard_now: f64::from(level.heard),
            trim_step: WHOLE / fade.get().max(1) as f64,
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
struct Splice {
    asked: Arc<AtomicU64>,
    at: Arc<AtomicU64>,
    to: Arc<AtomicU64>,
    taken: Arc<AtomicU64>,
    pulled: Arc<AtomicU64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Spliced {
    at: u64,
    to: u64,
}

impl Spliced {
    const fn stale(self) -> u64 {
        self.to.saturating_sub(self.at)
    }
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
    whole: Arc<AtomicBool>,
    pulls: Arc<AtomicU64>,
}

const WHOLE: f64 = 1.0;
const SILENT: f64 = 0.0;
const S24_LEAST: f64 = -8_388_608.0;
const S24_GREATEST: f64 = 8_388_607.0;

pub struct RingProducer {
    inner: rtrb::Producer<u8>,
    bytes_per_frame: NonZeroU32,
    format: SampleFormat,
    capacity: usize,
    mirror: Vec<u8>,
    splice: Splice,
    splices_asked: u64,
    splicing: Option<Spliced>,
    spliced_away: u64,
    discard: Discard,
    hold: Hold,
    fader: Fader,
    requested: u64,
    written: u64,
    announcing: rtrb::Producer<Rendered>,
    owed: Option<Rendered>,
    rendering: f32,
    heard: Arc<AtomicU32>,
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

    pub fn is_audible(&self) -> bool {
        self.fader.whole.load(Ordering::Acquire) && !self.is_discarding()
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
        self.written = 0;
        self.splicing = None;
        self.spliced_away = 0;
        self.owed = Some(Rendered {
            epoch: self.requested,
            from: 0,
            amplitude: self.rendering,
        });
        self.hand_over_what_is_owed();
    }

    pub const fn rendering(&self) -> f32 {
        self.rendering
    }

    pub fn heard(&self) -> Arc<AtomicU32> {
        Arc::clone(&self.heard)
    }

    pub fn hear_at(&self, amplitude: f32) {
        self.heard.store(amplitude.to_bits(), Ordering::Release);
    }

    pub fn render_at(&mut self, amplitude: f32, ahead: usize) {
        if amplitude.to_bits() == self.rendering.to_bits() {
            return;
        }
        self.rendering = amplitude;
        self.owed = Some(Rendered {
            epoch: self.requested,
            from: self.written.saturating_add(ahead as u64),
            amplitude,
        });
        self.hand_over_what_is_owed();
    }

    fn hand_over_what_is_owed(&mut self) {
        let Some(owed) = self.owed else {
            return;
        };
        if self.announcing.push(owed).is_ok() {
            self.owed = None;
        }
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
        self.hand_over_what_is_owed();
        if self.is_discarding() {
            return 0;
        }
        let stride = self.bytes_per_frame.get() as usize;
        let Some(tail) = buffer.as_bytes().get(start.saturating_mul(stride)..) else {
            return 0;
        };
        let frames = (tail.len() / stride).min(self.free_frames());
        self.push(tail, frames)
    }

    fn push(&mut self, tail: &[u8], frames: usize) -> usize {
        let stride = self.bytes_per_frame.get() as usize;
        let Some(whole) = tail.get(..frames.saturating_mul(stride)) else {
            return 0;
        };
        let (pushed, _) = self.inner.push_partial_slice(whole);
        let frames = pushed.len() / stride;
        if let Some(laid) = whole.get(..frames * stride) {
            self.remember(laid);
        }
        self.written = self.written.saturating_add(frames as u64);
        frames
    }

    fn remember(&mut self, laid: &[u8]) {
        let room = self.mirror.len();
        if room == 0 {
            return;
        }
        let stride = self.bytes_per_frame.get() as usize;
        let mut at = (self.written.saturating_mul(stride as u64) % room as u64) as usize;
        let mut from = laid;
        while !from.is_empty() {
            let run = from.len().min(room - at);
            if let (Some(into), Some(taken)) = (self.mirror.get_mut(at..at + run), from.get(..run))
            {
                into.copy_from_slice(taken);
            }
            from = from.get(run..).unwrap_or_default();
            at = 0;
        }
    }

    fn room_frames(&self) -> usize {
        self.inner.slots() / self.bytes_per_frame.get() as usize
    }

    fn occupied_frames(&self) -> usize {
        self.inner
            .buffer()
            .capacity()
            .saturating_sub(self.inner.slots())
            / self.bytes_per_frame.get() as usize
    }

    pub fn is_splicing(&self) -> bool {
        self.splicing.is_some() && self.splice.taken.load(Ordering::Acquire) != self.splices_asked
    }

    fn stale_frames(&self) -> usize {
        match self.splicing {
            Some(spliced) if self.is_splicing() => spliced.stale() as usize,
            Some(_) | None => 0,
        }
    }

    pub fn held_frames(&self) -> usize {
        self.occupied_frames().saturating_sub(self.stale_frames())
    }

    pub fn free_frames(&self) -> usize {
        self.capacity
            .saturating_sub(self.held_frames())
            .min(self.room_frames())
    }

    pub fn written_ahead_of_the_graph(&self) -> usize {
        self.occupied_frames()
    }

    pub fn splice_point(&self, ahead: Frames, blended: Frames) -> Option<u64> {
        if self.is_discarding() || self.is_splicing() {
            return None;
        }
        let read = self.written.saturating_sub(self.occupied_frames() as u64);
        let pulled = self.splice.pulled.load(Ordering::Relaxed).saturating_mul(2);
        let at = read.saturating_add(ahead.get().max(pulled));
        let mirrored = (self.mirror.len() / self.bytes_per_frame.get() as usize) as u64;
        let remembered = self.written.saturating_sub(mirrored);
        (at >= remembered && at.saturating_add(blended.get()) <= self.written).then_some(at)
    }

    pub const fn written(&self) -> u64 {
        self.written
    }

    pub const fn heard_as(&self, at: u64) -> u64 {
        at.saturating_sub(self.spliced_away)
    }

    pub fn room_to_splice(&self) -> usize {
        self.room_frames()
    }

    pub fn widen_what_was_written(&self, at: u64, into: &mut [f64]) -> usize {
        let stride = self.bytes_per_frame.get() as usize;
        let room = self.mirror.len();
        let mirrored = (room / stride) as u64;
        if room == 0 || at < self.written.saturating_sub(mirrored) || at >= self.written {
            return 0;
        }
        let width = usize::from(self.format.bytes_per_sample().get());
        let samples_a_frame = (stride / width).max(1);
        let frames = (into.len() / samples_a_frame).min((self.written - at) as usize);
        let first = (at.saturating_mul(stride as u64) % room as u64) as usize;
        for (index, slot) in into.iter_mut().take(frames * samples_a_frame).enumerate() {
            let byte = (first + index * width) % room;
            let mut word = [0_u8; SampleFormat::MAX_BYTES];
            for (offset, held) in word.iter_mut().take(width).enumerate() {
                *held = self
                    .mirror
                    .get((byte + offset) % room)
                    .copied()
                    .unwrap_or(0);
            }
            *slot = widened(self.format, word);
        }
        frames
    }

    pub fn write_spliced(&mut self, buffer: &AudioBuffer) -> usize {
        self.hand_over_what_is_owed();
        let stride = self.bytes_per_frame.get() as usize;
        let bytes = buffer.as_bytes();
        let frames = (bytes.len() / stride).min(self.room_frames());
        self.push(bytes, frames)
    }

    pub fn splice(&mut self, at: u64, to: u64) {
        let spliced = Spliced { at, to };
        self.splice.at.store(at, Ordering::Relaxed);
        self.splice.to.store(to, Ordering::Relaxed);
        self.splices_asked = self.splices_asked.wrapping_add(1);
        self.splice
            .asked
            .store(self.splices_asked, Ordering::Release);
        self.splicing = Some(spliced);
        self.spliced_away = self.spliced_away.saturating_add(spliced.stale());
    }

    pub fn is_abandoned(&self) -> bool {
        self.inner.is_abandoned()
    }
}

pub struct RingConsumer {
    inner: rtrb::Consumer<u8>,
    bytes_per_frame: NonZeroU32,
    discard: Discard,
    splice: Splice,
    splices_seen: u64,
    pending: Option<Spliced>,
    hold: Hold,
    fader: Fader,
    format: SampleFormat,
    fades: bool,
    level: f64,
    announced: rtrb::Consumer<Rendered>,
    heard: Arc<AtomicU32>,
    read: u64,
    rendered_at: f64,
    heard_now: f64,
    trim_step: f64,
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
        let stale = self.pending.map_or(0, Spliced::stale) as usize;
        (self.inner.slots() / self.bytes_per_frame.get() as usize).saturating_sub(stale)
    }

    fn look_for_a_splice(&mut self) {
        let asked = self.splice.asked.load(Ordering::Acquire);
        if asked == self.splices_seen {
            return;
        }
        self.splices_seen = asked;
        self.pending = Some(Spliced {
            at: self.splice.at.load(Ordering::Relaxed),
            to: self.splice.to.load(Ordering::Relaxed),
        });
    }

    fn pass_over_what_was_spliced_away(&mut self) {
        let Some(spliced) = self.pending else {
            return;
        };
        if self.read < spliced.at {
            return;
        }
        let stride = self.stride();
        let stale = (spliced.stale() as usize).min(self.inner.slots() / stride);
        if let Ok(passed) = self.inner.read_chunk(stale.saturating_mul(stride)) {
            passed.commit_all();
        }
        self.read = self.read.saturating_add(stale as u64);
        self.pending = None;
        self.splice
            .taken
            .store(self.splices_seen, Ordering::Release);
    }

    fn frames_before_the_splice(&self, room: usize) -> usize {
        self.pending.map_or(room, |spliced| {
            (spliced.at.saturating_sub(self.read) as usize).min(room)
        })
    }

    fn take_into(&mut self, into: &mut [u8], target: f64) -> usize {
        let stride = self.stride();
        let mut filled = 0;
        loop {
            self.pass_over_what_was_spliced_away();
            let room = into.len().saturating_sub(filled) / stride;
            let frames = self.frames_before_the_splice(room);
            if frames == 0 {
                return filled;
            }
            let Some(part) = into.get_mut(filled..filled.saturating_add(frames * stride)) else {
                return filled;
            };
            let (popped, _) = self.inner.pop_partial_slice(part);
            let taken = popped.len();
            self.shape(popped, target);
            filled = filled.saturating_add(taken);
            if taken < frames * stride {
                return filled;
            }
        }
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
        self.read = 0;
        self.heard_now = self.wanted();
        self.splices_seen = self.splice.asked.load(Ordering::Acquire);
        self.pending = None;
        self.splice
            .taken
            .store(self.splices_seen, Ordering::Release);
        self.fader.whole.store(false, Ordering::Release);
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
        let filled = self.pulled(dst);
        self.fader
            .whole
            .store(self.level >= WHOLE, Ordering::Release);
        filled
    }
}

impl RingConsumer {
    fn pulled(&mut self, dst: &mut [u8]) -> usize {
        self.fader.pulls.fetch_add(1, Ordering::Relaxed);
        self.splice
            .pulled
            .store((dst.len() / self.stride()) as u64, Ordering::Relaxed);
        self.look_for_a_splice();
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

        let target = if silenced { SILENT } else { WHOLE };
        let popped = self.take_into(whole, target);
        let taken = popped / stride;
        let filled = spliced.saturating_add(popped);
        if let Some(last) = popped
            .checked_sub(stride)
            .and_then(|from| whole.get(from..popped))
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
            Some(whole) => self.take_into(whole, SILENT),
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

    fn wanted(&self) -> f64 {
        f64::from(f32::from_bits(self.heard.load(Ordering::Acquire)))
    }

    fn trim(&self) -> f64 {
        if self.rendered_at > SILENT {
            self.heard_now / self.rendered_at
        } else {
            WHOLE
        }
    }

    fn take_what_was_rendered(&mut self) {
        while let Ok(next) = self.announced.peek() {
            let reached =
                next.epoch < self.seen || (next.epoch == self.seen && next.from <= self.read);
            if !reached {
                return;
            }
            self.rendered_at = f64::from(next.amplitude);
            let _ = self.announced.pop();
        }
    }

    fn renders_nothing_new_within(&self, frames: usize) -> bool {
        let until = self.read.saturating_add(frames as u64);
        self.announced.peek().map_or(true, |next| {
            next.epoch > self.seen || (next.epoch == self.seen && next.from >= until)
        })
    }

    fn is_steady(&self, target: f64, wanted: f64, frames: usize) -> bool {
        self.level >= WHOLE
            && target >= WHOLE
            && self.heard_now == wanted
            && self.heard_now == self.rendered_at
            && self.renders_nothing_new_within(frames)
    }

    fn shape(&mut self, frames: &mut [u8], target: f64) {
        let stride = self.stride();
        let count = frames.len() / stride;
        let wanted = self.wanted();
        if !self.fades {
            self.read = self.read.saturating_add(count as u64);
            self.take_what_was_rendered();
            if wanted <= SILENT {
                self.pad(frames);
            }
            return;
        }
        if self.is_steady(target, wanted, count) {
            self.read = self.read.saturating_add(count as u64);
            return;
        }
        let step = self.step();
        for frame in frames.chunks_exact_mut(stride) {
            self.take_what_was_rendered();
            self.read = self.read.saturating_add(1);
            self.heard_now = toward(self.heard_now, wanted, self.trim_step);
            if self.level < WHOLE || target < WHOLE {
                self.level = toward(self.level, target, step);
            }
            let factor = self.level * self.trim();
            if factor != WHOLE {
                scale(frame, self.format, factor);
            }
        }
    }
}

fn widened(format: SampleFormat, word: [u8; SampleFormat::MAX_BYTES]) -> f64 {
    let [first, second, third, fourth] = word;
    let full_scale = f64::from(format.full_scale());
    match format {
        SampleFormat::S16 => f64::from(i16::from_ne_bytes([first, second])) / full_scale,
        SampleFormat::S24 | SampleFormat::S32 => {
            f64::from(i32::from_ne_bytes([first, second, third, fourth])) / full_scale
        }
        SampleFormat::F32 => f64::from(f32::from_ne_bytes([first, second, third, fourth])),
    }
}

fn toward(from: f64, to: f64, step: f64) -> f64 {
    if to > from {
        (from + step).min(to)
    } else {
        (from - step).max(to)
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
        SampleFormat::S24 => {
            for sample in frame.as_chunks_mut::<4>().0 {
                let scaled = (f64::from(i32::from_ne_bytes(*sample)) * level)
                    .round()
                    .clamp(S24_LEAST, S24_GREATEST) as i32;
                *sample = scaled.to_ne_bytes();
            }
        }
        SampleFormat::S32 => {
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
        steady_at(1_000, frames)
    }

    fn steady_at(level: i16, frames: usize) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S16), frames);
        if let SampleData::S16(store) = buffer.data_mut() {
            store.fill(level);
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
            Level::WHOLE,
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
            Level::WHOLE,
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
                Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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

    fn counted(from: i16, frames: usize) -> AudioBuffer {
        let mut buffer = AudioBuffer::silence(spec(SampleFormat::S16), frames);
        if let SampleData::S16(store) = buffer.data_mut() {
            for (index, frame) in store.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                frame.fill(from + index as i16);
            }
        }
        buffer
    }

    fn whole_ring(frames: u64) -> (RingProducer, RingConsumer, RingMonitor) {
        ring(
            spec(SampleFormat::S16),
            Frames(frames),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
            Level::WHOLE,
        )
    }

    #[test]
    fn a_splice_plays_up_to_its_point_and_then_what_was_written_for_it_past_the_stale_frames() {
        let (mut producer, mut consumer, _) = whole_ring(64);
        producer.write(&counted(0, 48));
        let mut sunk = vec![0_u8; 10 * 4];
        consumer.fill(&mut sunk);

        let at = producer
            .splice_point(Frames(8), Frames(4))
            .expect("a full ring has room to splice ahead of the graph");
        assert_eq!(
            at, 30,
            "a splice was offered closer than two of the graph's pulls"
        );
        let stale = producer.written();
        assert_eq!(producer.write_spliced(&counted(1_000, 40)), 40);
        producer.splice(at, stale);
        assert!(producer.is_splicing());
        assert_eq!(producer.held_frames(), 20 + 40);
        assert!(
            producer.splice_point(Frames(8), Frames(4)).is_none(),
            "a second splice was offered over one the graph had not reached"
        );

        let mut sunk = vec![0_u8; 24 * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());
        assert_eq!(consumer.available_frames(), 36);

        let mut heard: Vec<i16> = (10..30).collect();
        heard.extend(1_000..1_004);
        assert_eq!(left_of(&sunk), heard);
        assert!(!producer.is_splicing());
        assert_eq!(producer.held_frames(), 36);
        assert_eq!(producer.heard_as(producer.written()), 48 + 40 - 18);
    }

    #[test]
    fn a_splice_the_graph_reached_late_keeps_time_rather_than_playing_frames_again() {
        let (mut producer, mut consumer, _) = whole_ring(64);
        producer.write(&counted(0, 48));
        let mut sunk = vec![0_u8; 10 * 4];
        consumer.fill(&mut sunk);

        let stale = producer.written();
        producer.write_spliced(&counted(1_005, 43));
        producer.splice(5, stale);

        let mut sunk = vec![0_u8; 4 * 4];
        consumer.fill(&mut sunk);
        assert_eq!(left_of(&sunk), vec![1_010, 1_011, 1_012, 1_013]);
    }

    #[test]
    fn a_discard_lets_go_of_a_splice_the_graph_had_not_reached() {
        let (mut producer, mut consumer, _) = whole_ring(64);
        producer.write(&counted(0, 48));
        let stale = producer.written();
        producer.write_spliced(&counted(1_000, 40));
        producer.splice(30, stale);
        producer.discard_buffered();

        let mut sunk = vec![0_u8; 4];
        consumer.fill(&mut sunk);
        assert!(!producer.is_splicing());
        assert!(!producer.is_discarding());
        assert_eq!(producer.held_frames(), 0);
        assert_eq!(producer.heard_as(10), 10);
    }

    #[test]
    fn what_was_written_is_widened_back_from_where_a_splice_would_blend_it() {
        let (mut producer, mut consumer, _) = whole_ring(16);
        let mut sunk = vec![0_u8; 12 * 4];
        for _ in 0..3 {
            producer.write(&counted(100, 12));
            consumer.fill(&mut sunk);
        }

        let mut widened = vec![0.0; 2 * 3];
        assert_eq!(producer.widen_what_was_written(30, &mut widened), 3);
        let full_scale = f64::from(SampleFormat::S16.full_scale());
        assert_eq!(
            widened,
            [106.0, 106.0, 107.0, 107.0, 108.0, 108.0].map(|word| word / full_scale)
        );
        assert_eq!(producer.widen_what_was_written(36, &mut widened), 0);
    }

    #[test]
    fn a_ring_refilling_after_a_discard_plays_silence_rather_than_a_stutter() {
        let (mut producer, mut consumer, mut faults) = ring(
            spec(SampleFormat::S16),
            Frames(256),
            Silence::Unmarked,
            UNFADED,
            Entering::Whole,
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
                Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
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
            Level::WHOLE,
        );
        producer.discard_buffered();

        let mut sunk = vec![0xAA_u8; 32 * 8];
        assert_eq!(consumer.fill(&mut sunk), 0);
        assert!(sunk.iter().all(|byte| *byte == 0xAA));
    }

    fn faded() -> (RingProducer, RingConsumer) {
        let (producer, consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            FADED_OVER,
            Entering::Whole,
            Level::WHOLE,
        );
        (producer, consumer)
    }

    #[test]
    fn a_level_turned_down_is_heard_on_what_the_ring_already_holds() {
        let (mut producer, mut consumer) = faded();
        producer.write(&steady(8));
        producer.hear_at(0.5);

        let mut sunk = vec![0_u8; 8 * 4];
        consumer.fill(&mut sunk);

        assert_eq!(left_of(&sunk), vec![750, 500, 500, 500, 500, 500, 500, 500]);
    }

    #[test]
    fn frames_rendered_at_the_level_heard_pass_through_untouched_once_the_held_ones_are_out() {
        let (mut producer, mut consumer) = faded();
        producer.write(&steady(4));
        producer.hear_at(0.5);
        producer.render_at(0.5, 2);
        producer.write(&steady(2));
        let rendered = steady_at(500, 4);
        producer.write(&rendered);

        let mut sunk = vec![0_u8; 10 * 4];
        consumer.fill(&mut sunk);

        assert_eq!(
            left_of(&sunk),
            vec![750, 500, 500, 500, 500, 500, 500, 500, 500, 500]
        );
        assert_eq!(sunk[6 * 4..], rendered.as_bytes()[..]);
    }

    #[test]
    fn a_level_turned_back_up_raises_what_was_rendered_quieter() {
        let (mut producer, mut consumer) = faded();
        producer.hear_at(0.5);
        producer.render_at(0.5, 0);
        producer.write(&steady_at(500, 12));
        let mut settling = vec![0_u8; 4 * 4];
        consumer.fill(&mut settling);
        assert_eq!(left_of(&settling), vec![750, 500, 500, 500]);
        producer.hear_at(1.0);

        let mut sunk = vec![0_u8; 8 * 4];
        consumer.fill(&mut sunk);

        assert_eq!(
            left_of(&sunk),
            vec![750, 1_000, 1_000, 1_000, 1_000, 1_000, 1_000, 1_000]
        );
    }

    #[test]
    fn a_ring_opened_muted_is_silent_from_its_first_frame_and_still_plays_on() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S16),
            Frames(64),
            Silence::Unmarked,
            FADED_OVER,
            Entering::Whole,
            Level {
                rendered: 1.0,
                heard: 0.0,
            },
        );
        producer.write(&steady(16));

        let mut sunk = vec![0xAA_u8; 8 * 4];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());

        assert!(sunk.iter().all(|byte| *byte == 0));
        assert_eq!(consumer.available_frames(), 8);
    }

    #[test]
    fn a_discard_counts_the_frames_again_from_the_level_rendered_last() {
        let (mut producer, mut consumer) = faded();
        producer.write(&steady(8));
        producer.hear_at(0.5);
        producer.render_at(0.25, 30);
        producer.discard_buffered();

        let mut fading = vec![0_u8; 8 * 4];
        consumer.fill(&mut fading);
        assert!(!producer.is_discarding());
        producer.write(&steady_at(250, 40));

        let mut sunk = vec![0_u8; 40 * 4];
        consumer.fill(&mut sunk);

        let left = left_of(&sunk);
        assert_eq!(left[..4], [125, 250, 375, 500]);
        assert!(left[4..].iter().all(|sample| *sample == 500), "{left:?}");
    }

    #[test]
    fn a_muted_marked_ring_feeds_its_silence_and_still_takes_the_frames() {
        let (mut producer, mut consumer, _) = ring(
            spec(SampleFormat::S24),
            Frames(256),
            marked(),
            UNFADED,
            Entering::Whole,
            Level::WHOLE,
        );
        let carried = dop(16, 0);
        producer.write(&carried);
        producer.hear_at(0.0);

        let mut sunk = vec![0_u8; 8 * 8];
        assert_eq!(consumer.fill(&mut sunk), sunk.len());

        assert_eq!(
            consumer.available_frames(),
            8,
            "a muted marked ring stopped playing"
        );
        for frame in sunk.as_chunks::<8>().0 {
            assert_eq!(
                frame[..2],
                DOP_SILENT_PAIR,
                "a muted marked ring let its music through"
            );
        }
        assert!(
            markers(&sunk).windows(2).all(|pair| pair[0] != pair[1]),
            "the silence broke the markers' alternation"
        );

        producer.hear_at(1.0);
        let mut heard = vec![0_u8; 8 * 8];
        consumer.fill(&mut heard);
        assert!(
            heard
                .as_chunks::<8>()
                .0
                .iter()
                .any(|frame| frame[..] == carried.as_bytes()[8 * 15..]),
            "unmuting a marked ring did not bring its music back"
        );
    }

    #[test]
    fn a_trim_above_one_holds_a_twenty_four_bit_word_inside_its_twenty_four_bits() {
        let mut frame: Vec<u8> = [8_000_000_i32, -8_000_000, 1_000]
            .iter()
            .flat_map(|sample| sample.to_ne_bytes())
            .collect();

        scale(&mut frame, SampleFormat::S24, 2.0);

        let words: Vec<i32> = frame
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_ne_bytes(*word))
            .collect();
        assert_eq!(words, vec![8_388_607, -8_388_608, 2_000]);
    }
}
