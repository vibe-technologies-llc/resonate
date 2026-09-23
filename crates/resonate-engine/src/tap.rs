use std::{
    hint,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering, fence},
    },
    time::{Duration, Instant},
};

use resonate_core::{AudioBuffer, Frames, SampleData, SampleRate};

const TAPPED_CHANNELS: usize = 2;
const HEARD_SLACK: Duration = Duration::from_millis(500);
const WIDEST_LOOK: usize = 32_768;
const LARGEST_TAP: usize = 1 << 20;
const RUNS_AHEAD_AT_MOST: Duration = Duration::from_millis(100);
const ANCHOR_TRIES: usize = 64;
const NOTHING_VALID: u64 = u64::MAX;

#[derive(Clone, Default)]
pub enum Tapped {
    #[default]
    Nothing,
    Samples(Arc<Tap>),
    Markers,
}

impl Tapped {
    pub(crate) fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Samples(held), Self::Samples(other)) => Arc::ptr_eq(held, other),
            (Self::Nothing, Self::Nothing) | (Self::Markers, Self::Markers) => true,
            (Self::Nothing | Self::Samples(_) | Self::Markers, _) => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caught {
    pub tapped: usize,
}

pub struct Tap {
    rate: SampleRate,
    mask: usize,
    slots: Box<[AtomicU32]>,
    claimed: AtomicU64,
    written: AtomicU64,
    valid_from: AtomicU64,
    born: Instant,
    anchor: Anchor,
}

#[derive(Default)]
struct Anchor {
    turn: AtomicU64,
    frame: AtomicU64,
    at: AtomicU64,
    moving: AtomicBool,
}

#[derive(Clone, Copy)]
struct Fixed {
    frame: u64,
    at: Duration,
    moving: bool,
}

impl Anchor {
    fn fix(&self, frame: u64, at: Duration, moving: bool) {
        let turn = self.turn.load(Ordering::Relaxed);
        self.turn.store(turn.wrapping_add(1), Ordering::Relaxed);
        fence(Ordering::Release);
        self.frame.store(frame, Ordering::Relaxed);
        self.at.store(nanos(at), Ordering::Relaxed);
        self.moving.store(moving, Ordering::Relaxed);
        self.turn.store(turn.wrapping_add(2), Ordering::Release);
    }

    fn fixed(&self) -> Option<Fixed> {
        for _ in 0..ANCHOR_TRIES {
            let before = self.turn.load(Ordering::Acquire);
            if before % 2 == 1 {
                hint::spin_loop();
                continue;
            }
            let frame = self.frame.load(Ordering::Relaxed);
            let at = self.at.load(Ordering::Relaxed);
            let moving = self.moving.load(Ordering::Relaxed);
            fence(Ordering::Acquire);
            if self.turn.load(Ordering::Relaxed) == before {
                return Some(Fixed {
                    frame,
                    at: Duration::from_nanos(at),
                    moving,
                });
            }
            hint::spin_loop();
        }
        None
    }
}

fn nanos(at: Duration) -> u64 {
    u64::try_from(at.as_nanos()).unwrap_or(u64::MAX)
}

impl Tap {
    fn new(rate: SampleRate, ring: usize) -> Self {
        let capacity = capacity_for(ring, rate);
        Self {
            rate,
            mask: capacity - 1,
            slots: (0..capacity * TAPPED_CHANNELS)
                .map(|_| AtomicU32::new(0))
                .collect(),
            claimed: AtomicU64::new(0),
            written: AtomicU64::new(0),
            valid_from: AtomicU64::new(0),
            born: Instant::now(),
            anchor: Anchor::default(),
        }
    }

    pub const fn rate(&self) -> SampleRate {
        self.rate
    }

    pub fn around(&self, now: Instant, left: &mut [f32], right: &mut [f32]) -> Caught {
        let wanted = left.len().min(right.len());
        let written = self.written.load(Ordering::Acquire);
        let valid_from = self.valid_from.load(Ordering::Relaxed);
        let Some(fixed) = self.anchor.fixed() else {
            silence(left, right);
            return Caught { tapped: 0 };
        };

        let heard = fixed.frame.saturating_add(self.run_on(fixed, now));
        let end = heard.saturating_add((wanted / 2) as u64).min(written);
        let start = end.saturating_sub(wanted as u64);
        let before = wanted.saturating_sub((end - start) as usize);

        silence(left, right);
        let frames = left
            .iter_mut()
            .zip(right.iter_mut())
            .skip(before)
            .zip(start..end);
        for ((on_the_left, on_the_right), frame) in frames {
            let slot = (frame as usize & self.mask) * TAPPED_CHANNELS;
            *on_the_left = self.level_at(slot);
            *on_the_right = self.level_at(slot + 1);
        }

        fence(Ordering::Acquire);
        let overwritten = self
            .claimed
            .load(Ordering::Relaxed)
            .saturating_sub(self.slots.len() as u64 / TAPPED_CHANNELS as u64);
        let first_sound = overwritten.max(valid_from).clamp(start, end);
        let lost = (first_sound - start) as usize;
        for (on_the_left, on_the_right) in left
            .iter_mut()
            .zip(right.iter_mut())
            .skip(before)
            .take(lost)
        {
            *on_the_left = 0.0;
            *on_the_right = 0.0;
        }

        Caught {
            tapped: (end - first_sound) as usize,
        }
    }

    fn run_on(&self, fixed: Fixed, now: Instant) -> u64 {
        if !fixed.moving {
            return 0;
        }
        let since = now
            .saturating_duration_since(self.born + fixed.at)
            .min(RUNS_AHEAD_AT_MOST);
        Frames::from_duration(since, self.rate).get()
    }

    fn level_at(&self, slot: usize) -> f32 {
        self.slots
            .get(slot)
            .map_or(0.0, |held| f32::from_bits(held.load(Ordering::Relaxed)))
    }

    fn lay(&self, slot: usize, level: f32) {
        if let Some(held) = self.slots.get(slot) {
            held.store(level.to_bits(), Ordering::Relaxed);
        }
    }
}

fn silence(left: &mut [f32], right: &mut [f32]) {
    left.fill(0.0);
    right.fill(0.0);
}

fn capacity_for(ring: usize, rate: SampleRate) -> usize {
    let slack = usize::try_from(Frames::from_duration(HEARD_SLACK, rate).get()).unwrap_or(0);
    ring.saturating_add(slack)
        .saturating_add(WIDEST_LOOK)
        .checked_next_power_of_two()
        .map_or(LARGEST_TAP, |capacity| capacity.min(LARGEST_TAP))
}

pub(crate) struct Tapping {
    tap: Arc<Tap>,
    listening: Arc<AtomicBool>,
    written: u64,
    skipping: bool,
}

impl Tapping {
    pub(crate) fn new(rate: SampleRate, ring: usize, listening: &Arc<AtomicBool>) -> Self {
        Self {
            tap: Arc::new(Tap::new(rate, ring)),
            listening: Arc::clone(listening),
            written: 0,
            skipping: false,
        }
    }

    pub(crate) fn tapped(&self) -> Tapped {
        Tapped::Samples(Arc::clone(&self.tap))
    }

    pub(crate) fn record(&mut self, buffer: &AudioBuffer, from: usize, frames: usize) {
        if frames == 0 {
            return;
        }
        let upto = self.written.saturating_add(frames as u64);
        if !self.listening.load(Ordering::Relaxed) {
            if !self.skipping {
                self.tap.valid_from.store(NOTHING_VALID, Ordering::Relaxed);
                self.skipping = true;
            }
            self.published(upto);
            return;
        }
        if self.skipping {
            self.tap.valid_from.store(self.written, Ordering::Relaxed);
            self.skipping = false;
        }

        self.tap.claimed.store(upto, Ordering::Relaxed);
        fence(Ordering::Release);
        let channels = buffer.spec().channel_count().get() as usize;
        let scale = buffer.spec().format.full_scale().recip();
        let run =
            from.saturating_mul(channels)..from.saturating_add(frames).saturating_mul(channels);
        match buffer.data() {
            SampleData::S16(samples) => {
                self.lay(samples.get(run), channels, |sample| {
                    f32::from(sample) * scale
                });
            }
            SampleData::S24(samples) | SampleData::S32(samples) => {
                self.lay(samples.get(run), channels, |sample| sample as f32 * scale);
            }
            SampleData::F32(samples) => self.lay(samples.get(run), channels, |sample| sample),
        }
        self.published(upto);
    }

    fn lay<S: Copy>(&self, run: Option<&[S]>, channels: usize, level: impl Fn(S) -> f32) {
        let Some(run) = run else {
            return;
        };
        let first = self.written as usize;
        for (offset, frame) in run.chunks_exact(channels).enumerate() {
            let left = frame.first().map_or(0.0, |sample| level(*sample));
            let right = frame.get(1).map_or(left, |sample| level(*sample));
            let slot = (first.wrapping_add(offset) & self.tap.mask) * TAPPED_CHANNELS;
            self.tap.lay(slot, left);
            self.tap.lay(slot + 1, right);
        }
    }

    fn published(&mut self, upto: u64) {
        self.written = upto;
        self.tap.written.store(upto, Ordering::Release);
    }

    pub(crate) fn forget(&self) {
        if !self.skipping {
            self.tap.valid_from.store(self.written, Ordering::Relaxed);
        }
    }

    pub(crate) fn hear(&self, downstream: u64, moving: bool) {
        self.tap.anchor.fix(
            self.written.saturating_sub(downstream),
            self.tap.born.elapsed(),
            moving,
        );
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, SampleFormat, StreamSpec};

    use super::*;

    const RATE: SampleRate = SampleRate::HZ_48000;
    const RING: usize = 4_096;

    fn ramp(frames: usize, from: usize) -> AudioBuffer {
        let spec = StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::F32);
        let mut buffer = AudioBuffer::silence(spec, frames);
        if let Some(samples) = buffer.as_f32_mut() {
            for (at, [left, right]) in samples.as_chunks_mut::<2>().0.iter_mut().enumerate() {
                let level = (from + at) as f32 / 1_000_000.0;
                *left = level;
                *right = -level;
            }
        }
        buffer
    }

    fn listening(on: bool) -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(on))
    }

    fn read(tap: &Tap, frames: usize) -> (Vec<f32>, Vec<f32>, Caught) {
        let mut left = vec![f32::NAN; frames];
        let mut right = vec![f32::NAN; frames];
        let caught = tap.around(Instant::now(), &mut left, &mut right);
        (left, right, caught)
    }

    fn level_of(frame: usize) -> f32 {
        frame as f32 / 1_000_000.0
    }

    #[test]
    fn what_is_read_is_centred_on_the_frame_the_graph_is_playing() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(3_000, 0), 0, 3_000);
        tapping.hear(1_000, false);

        let (left, right, caught) = read(&tapping.tap, 100);

        assert_eq!(caught, Caught { tapped: 100 });
        assert_eq!(left.first().copied(), Some(level_of(1_950)));
        assert_eq!(left.last().copied(), Some(level_of(2_049)));
        assert_eq!(right.first().copied(), Some(-level_of(1_950)));
    }

    #[test]
    fn a_look_past_what_has_been_written_ends_at_the_newest_frame() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(500, 0), 0, 500);
        tapping.hear(0, false);

        let (left, _, caught) = read(&tapping.tap, 100);

        assert_eq!(caught.tapped, 100);
        assert_eq!(left.last().copied(), Some(level_of(499)));
    }

    #[test]
    fn a_look_reaching_before_the_stream_began_is_silence_there() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(40, 0), 0, 40);
        tapping.hear(0, false);

        let (left, _, caught) = read(&tapping.tap, 100);

        assert_eq!(caught.tapped, 40);
        assert!(left.iter().take(60).all(|level| *level == 0.0));
        assert_eq!(left.get(60).copied(), Some(level_of(0)));
    }

    #[test]
    fn nothing_is_recorded_while_nobody_listens_and_what_was_passed_over_reads_as_silence() {
        let ear = listening(false);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(1_000, 0), 0, 1_000);
        tapping.hear(0, false);

        let (left, _, caught) = read(&tapping.tap, 100);
        assert_eq!(caught.tapped, 0);
        assert!(left.iter().all(|level| *level == 0.0));

        ear.store(true, Ordering::Relaxed);
        tapping.record(&ramp(1_000, 1_000), 0, 1_000);
        tapping.hear(0, false);

        let (left, _, caught) = read(&tapping.tap, 100);
        assert_eq!(caught.tapped, 100);
        assert_eq!(left.last().copied(), Some(level_of(1_999)));

        tapping.hear(990, false);
        let (left, _, caught) = read(&tapping.tap, 100);
        assert_eq!(
            caught.tapped, 60,
            "only what was written since the listener came is sound"
        );
        assert!(left.iter().take(40).all(|level| *level == 0.0));
        assert_eq!(left.get(40).copied(), Some(level_of(1_000)));
    }

    #[test]
    fn a_seek_forgets_what_was_tapped_before_it() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(2_000, 0), 0, 2_000);
        tapping.forget();
        tapping.hear(500, false);

        let (left, _, caught) = read(&tapping.tap, 100);

        assert_eq!(caught.tapped, 0);
        assert!(left.iter().all(|level| *level == 0.0));
    }

    #[test]
    fn a_moving_transport_runs_on_from_its_anchor_but_never_past_what_is_written() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&ramp(48_000, 0), 0, 48_000);
        tapping.hear(40_000, true);
        let fixed = tapping
            .tap
            .anchor
            .fixed()
            .expect("an anchor nobody is writing");

        let later = tapping.tap.born + fixed.at + Duration::from_millis(10);
        assert_eq!(tapping.tap.run_on(fixed, later), 480);

        let much_later = tapping.tap.born + fixed.at + Duration::from_secs(5);
        assert_eq!(
            tapping.tap.run_on(fixed, much_later),
            Frames::from_duration(RUNS_AHEAD_AT_MOST, RATE).get()
        );

        tapping.hear(0, true);
        let (left, _, _) = read(&tapping.tap, 100);
        assert_eq!(left.last().copied(), Some(level_of(47_999)));
    }

    #[test]
    fn a_frame_the_ring_has_gone_round_on_reads_as_silence_rather_than_as_the_frame_after_it() {
        let ear = listening(true);
        let mut tapping = Tapping::new(RATE, RING, &ear);
        let capacity = tapping.tap.mask + 1;
        let frames = capacity + 1_000;
        tapping.record(&ramp(frames, 0), 0, frames);
        tapping.hear(0, false);

        let wide = capacity + 200;
        let (left, _, caught) = read(&tapping.tap, wide);

        assert_eq!(caught.tapped, capacity);
        assert!(left.iter().take(200).all(|level| *level == 0.0));
        assert_eq!(left.get(200).copied(), Some(level_of(1_000)));
    }

    #[test]
    fn a_mono_integer_stream_is_read_on_both_sides_at_full_scale() {
        let ear = listening(true);
        let spec = StreamSpec::new(RATE, ChannelLayout::Mono, SampleFormat::S16);
        let mut buffer = AudioBuffer::silence(spec, 4);
        if let SampleData::S16(samples) = buffer.data_mut() {
            samples.copy_from_slice(&[i16::MIN, -16_384, 0, 16_384]);
        }
        let mut tapping = Tapping::new(RATE, RING, &ear);
        tapping.record(&buffer, 1, 3);
        tapping.hear(0, false);

        let (left, right, _) = read(&tapping.tap, 3);

        assert_eq!(left, vec![-0.5, 0.0, 0.5]);
        assert_eq!(right, left);
    }

    #[test]
    fn the_tap_holds_the_ring_the_slack_and_the_widest_look_and_no_more_than_its_ceiling() {
        let held = capacity_for(24_000, RATE);
        assert!(held.is_power_of_two());
        assert!(held >= 24_000 + 24_000 + WIDEST_LOOK);
        assert_eq!(capacity_for(usize::MAX, RATE), LARGEST_TAP);
    }

    #[test]
    fn a_tap_published_twice_is_the_same_tap_and_a_new_one_is_not() {
        let ear = listening(true);
        let one = Tapping::new(RATE, RING, &ear);
        let another = Tapping::new(RATE, RING, &ear);

        assert!(one.tapped().is(&one.tapped()));
        assert!(!one.tapped().is(&another.tapped()));
        assert!(Tapped::Markers.is(&Tapped::Markers));
        assert!(!Tapped::Nothing.is(&Tapped::Markers));
    }
}
