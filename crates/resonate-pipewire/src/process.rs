#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

use std::{
    mem,
    num::{NonZeroU16, NonZeroU32, NonZeroUsize},
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
};

use libspa::utils::Fraction;
use pipewire::stream::Stream;
use resonate_core::{Ratio, SampleRate, StreamSpec};

use crate::{AudioSink, AudioSource, GraphTime, StreamClock, format::WireWord};

const PADDED_WORD: usize = 4;
const PACKED_WORD: usize = 3;
const FEEDS_HANDED_AT_ONCE: usize = 1;
const FEEDS_SPENT_AT_ONCE: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taken {
    AsAsked(WireWord),
    Otherwise { frame: NonZeroU16 },
}

#[derive(Debug, Default)]
pub(crate) struct Taking(AtomicU32);

const PADDED_AS_ASKED: u32 = 0;
const PACKED_AS_ASKED: u32 = 1;
const FRAME_HELD_ABOVE: u32 = 1;

impl Taking {
    pub(crate) fn store(&self, taken: Taken) {
        let held = match taken {
            Taken::AsAsked(WireWord::Padded) => PADDED_AS_ASKED,
            Taken::AsAsked(WireWord::Packed) => PACKED_AS_ASKED,
            Taken::Otherwise { frame } => u32::from(frame.get()) << FRAME_HELD_ABOVE,
        };
        self.0.store(held, Ordering::Release);
    }

    fn load(&self) -> Taken {
        match self.0.load(Ordering::Acquire) {
            PADDED_AS_ASKED => Taken::AsAsked(WireWord::Padded),
            PACKED_AS_ASKED => Taken::AsAsked(WireWord::Packed),
            held => u16::try_from(held >> FRAME_HELD_ABOVE)
                .ok()
                .and_then(NonZeroU16::new)
                .map_or(Taken::AsAsked(WireWord::Padded), |frame| Taken::Otherwise {
                    frame,
                }),
        }
    }
}

pub(crate) struct Hearing;

impl Hearing {
    pub(crate) fn run(stream: &Stream, sink: &mut dyn AudioSink) {
        let Some(mut buffer) = stream.dequeue_buffer() else {
            return;
        };
        let datas = buffer.datas_mut();
        let Some(data) = datas.first_mut() else {
            return;
        };
        let chunk = data.chunk();
        let offset = chunk.offset() as usize;
        let size = chunk.size() as usize;
        let Some(slice) = data.data() else {
            return;
        };
        let end = offset.saturating_add(size).min(slice.len());
        if let Some(heard) = slice.get(offset.min(end)..end) {
            sink.take(heard);
        }
    }
}

pub(crate) struct Feed {
    cycle: Cycle,
    source: Box<dyn AudioSource>,
}

impl Feed {
    pub(crate) fn new(cycle: Cycle, source: Box<dyn AudioSource>) -> Self {
        Self { cycle, source }
    }

    pub(crate) fn into_source(self) -> Box<dyn AudioSource> {
        self.source
    }
}

pub(crate) struct Fed {
    feed: Feed,
    handed: rtrb::Consumer<Feed>,
    spent: rtrb::Producer<Feed>,
}

impl Fed {
    pub(crate) fn run(&mut self, stream: &Stream) {
        self.take_what_was_handed();
        self.feed.cycle.run(stream, self.feed.source.as_mut());
    }

    fn take_what_was_handed(&mut self) {
        if self.spent.is_full() {
            return;
        }
        let Ok(handed) = self.handed.pop() else {
            return;
        };
        let spent = mem::replace(&mut self.feed, handed);
        let kept_off_this_thread = self.spent.push(spent);
        debug_assert!(kept_off_this_thread.is_ok());
    }
}

pub(crate) struct Handing {
    handing: rtrb::Producer<Feed>,
    spent: rtrb::Consumer<Feed>,
}

impl Handing {
    pub(crate) fn has_room(&self) -> bool {
        !self.handing.is_full()
    }

    pub(crate) fn hand(&mut self, feed: Feed) -> Result<(), Feed> {
        self.handing
            .push(feed)
            .map_err(|rtrb::PushError::Full(feed)| feed)
    }

    pub(crate) fn let_go_of_the_spent(&mut self) {
        while let Ok(spent) = self.spent.pop() {
            drop(spent);
        }
    }
}

pub(crate) fn fed_from(feed: Feed) -> (Fed, Handing) {
    let (handing, handed) = rtrb::RingBuffer::new(FEEDS_HANDED_AT_ONCE);
    let (spending, spent) = rtrb::RingBuffer::new(FEEDS_SPENT_AT_ONCE);
    (
        Fed {
            feed,
            handed,
            spent: spending,
        },
        Handing { handing, spent },
    )
}

pub(crate) struct Cycle {
    rate: SampleRate,
    padded_frame: NonZeroUsize,
    packed_frame: NonZeroUsize,
    taking: Arc<Taking>,
    clock: Arc<StreamClock>,
    last_filled: AtomicU64,
}

impl Cycle {
    pub(crate) fn new(spec: StreamSpec, taking: Arc<Taking>, clock: Arc<StreamClock>) -> Self {
        let bytes_per_frame = spec.bytes_per_frame().get() as usize;
        let packed_stride = packed_stride(spec);

        Self {
            rate: spec.rate,
            padded_frame: NonZeroUsize::new(bytes_per_frame).unwrap_or(NonZeroUsize::MIN),
            packed_frame: NonZeroUsize::new(packed_stride).unwrap_or(NonZeroUsize::MIN),
            taking,
            clock,
            last_filled: AtomicU64::new(0),
        }
    }

    pub(crate) fn run(&self, stream: &Stream, source: &mut dyn AudioSource) {
        self.note_latency(stream);

        let Some(mut buffer) = stream.dequeue_buffer() else {
            return;
        };
        let asked = buffer.requested();
        let datas = buffer.datas_mut();
        let Some(data) = datas.first_mut() else {
            return;
        };
        let taken = self.taking.load();
        let frame = self.frame_of(taken);
        let filled = match data.data() {
            Some(slice) => self.write(taken, asked, slice, source),
            None => 0,
        };

        self.note_filled(filled, frame);
        let chunk = data.chunk_mut();
        *chunk.offset_mut() = 0;
        *chunk.stride_mut() = frame.get() as i32;
        *chunk.size_mut() = filled as u32;
    }

    fn write(
        &self,
        taken: Taken,
        asked: u64,
        slice: &mut [u8],
        source: &mut dyn AudioSource,
    ) -> usize {
        let frame = self.ring_frame_of(taken);
        let Some(quantum) = slice.get_mut(..asked_for(asked, frame, slice.len())) else {
            return 0;
        };
        match taken {
            Taken::AsAsked(WireWord::Padded) => source.fill(quantum),
            Taken::AsAsked(WireWord::Packed) => {
                let written = source.fill(quantum);
                pack(quantum, written)
            }
            Taken::Otherwise { .. } => {
                quantum.fill(0);
                quantum.len()
            }
        }
    }

    fn ring_frame_of(&self, taken: Taken) -> NonZeroUsize {
        match taken {
            Taken::AsAsked(_) => self.padded_frame,
            Taken::Otherwise { frame } => NonZeroUsize::from(frame),
        }
    }

    fn frame_of(&self, taken: Taken) -> NonZeroUsize {
        match taken {
            Taken::AsAsked(WireWord::Padded) => self.padded_frame,
            Taken::AsAsked(WireWord::Packed) => self.packed_frame,
            Taken::Otherwise { frame } => NonZeroUsize::from(frame),
        }
    }

    fn note_filled(&self, filled: usize, frame: NonZeroUsize) {
        self.last_filled
            .store((filled / frame) as u64, Ordering::Relaxed);
    }

    fn queued_frames(&self, buffers: u32) -> u64 {
        u64::from(buffers).saturating_mul(self.last_filled.load(Ordering::Relaxed))
    }

    fn note_latency(&self, stream: &Stream) {
        let Ok(time) = stream.time() else {
            return;
        };
        let Some(tick) = ticking(time.rate()) else {
            return;
        };
        let ahead = GraphTime {
            delay: time.delay(),
            buffered: time.buffered(),
            queued: self.queued_frames(time.queued_buffers()),
            tick,
        };
        self.clock.note(ahead, self.rate);
    }
}

fn asked_for(frames: u64, frame: NonZeroUsize, room: usize) -> usize {
    let quantum = (frames as usize).saturating_mul(frame.get());
    let whole_room = room - room % frame;
    if quantum == 0 {
        return whole_room;
    }
    quantum.min(whole_room)
}

fn packed_stride(spec: StreamSpec) -> usize {
    let samples = usize::from(spec.channel_count().get());
    samples.saturating_mul(PACKED_WORD)
}

fn pack(slice: &mut [u8], written: usize) -> usize {
    let words = written / PADDED_WORD;
    let mut filled = 0_usize;

    for word in 0..words {
        let held = word.saturating_mul(PADDED_WORD);
        let onto = word.saturating_mul(PACKED_WORD);
        let Some(low) = slice.get(held..held.saturating_add(PACKED_WORD)) else {
            break;
        };
        let Ok(carried) = <[u8; PACKED_WORD]>::try_from(low) else {
            break;
        };
        let Some(place) = slice.get_mut(onto..onto.saturating_add(PACKED_WORD)) else {
            break;
        };
        place.copy_from_slice(&carried);
        filled = filled.saturating_add(PACKED_WORD);
    }

    filled
}

fn ticking(rate: Fraction) -> Option<Ratio> {
    Some(Ratio {
        numer: NonZeroU32::new(rate.num)?,
        denom: NonZeroU32::new(rate.denom)?,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::expect_used)]
mod tests {
    use resonate_core::{ChannelLayout, SampleFormat};

    use super::*;

    const DOP_MARKERS: [u8; 2] = [0x05, 0xFA];

    fn words(count: usize) -> Vec<u8> {
        (0..count)
            .flat_map(|word| {
                let value = word as u32;
                [
                    value as u8,
                    (value >> 8) as u8,
                    (value >> 16) as u8,
                    if value & 0x0080_0000 == 0 { 0x00 } else { 0xFF },
                ]
            })
            .collect()
    }

    fn stereo(format: SampleFormat) -> StreamSpec {
        StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Stereo, format)
    }

    fn cycling(format: SampleFormat) -> Cycle {
        Cycle::new(
            stereo(format),
            Arc::new(Taking::default()),
            Arc::new(StreamClock::default()),
        )
    }

    struct Counting {
        handed: Vec<u8>,
        asked: usize,
    }

    impl AudioSource for Counting {
        fn fill(&mut self, dst: &mut [u8]) -> usize {
            self.asked += dst.len();
            let written = dst.len().min(self.handed.len());
            dst[..written].copy_from_slice(&self.handed[..written]);
            self.handed.drain(..written);
            written
        }
    }

    fn whole(bytes: usize) -> NonZeroUsize {
        NonZeroUsize::new(bytes).expect("a frame holds a byte")
    }

    #[test]
    fn each_queued_buffer_is_reckoned_to_hold_what_the_last_cycle_filled() {
        let cycle = cycling(SampleFormat::S24);

        assert_eq!(
            cycle.queued_frames(2),
            0,
            "nothing filled was reckoned queued"
        );

        cycle.note_filled(1_024 * 8, cycle.frame_of(Taken::AsAsked(WireWord::Padded)));
        assert_eq!(cycle.queued_frames(2), 2_048);

        cycle.note_filled(1_024 * 6, cycle.frame_of(Taken::AsAsked(WireWord::Packed)));
        assert_eq!(
            cycle.queued_frames(1),
            1_024,
            "a packed buffer was counted in padded frames"
        );
    }

    #[test]
    fn a_cycle_takes_the_frames_the_graph_asked_for_rather_than_the_pool_it_was_handed() {
        let frame = whole(4);
        let room = 8_192;

        assert_eq!(
            asked_for(256, frame, room),
            1_024,
            "the cycle read past the quantum the graph asked for"
        );
        assert_eq!(
            asked_for(0, frame, room),
            room,
            "a graph that named no quantum left the buffer unfilled"
        );
        assert_eq!(
            asked_for(4_096, frame, room),
            room,
            "a quantum larger than the buffer was written past its end"
        );
        assert_eq!(
            asked_for(0, whole(20), 8_190),
            8_180,
            "a pool that is not whole frames was handed out to its last byte"
        );
    }

    #[test]
    fn what_was_taken_reads_back_as_it_was_stored() {
        let taking = Taking::default();

        assert_eq!(taking.load(), Taken::AsAsked(WireWord::Padded));

        for taken in [
            Taken::AsAsked(WireWord::Packed),
            Taken::Otherwise {
                frame: NonZeroU16::MIN,
            },
            Taken::Otherwise {
                frame: NonZeroU16::new(64 * 4).expect("a wide frame"),
            },
            Taken::AsAsked(WireWord::Padded),
        ] {
            taking.store(taken);
            assert_eq!(taking.load(), taken);
        }
    }

    #[test]
    fn a_packed_quantum_asks_the_ring_for_every_padded_frame_the_graph_wants() {
        let cycle = cycling(SampleFormat::S24);
        let frames = 256;
        let mut source = Counting {
            handed: vec![0x5A; 4 * 2 * 1_024],
            asked: 0,
        };
        let mut buffer = vec![0; 4 * 2 * 1_024];

        let filled = cycle.write(
            Taken::AsAsked(WireWord::Packed),
            frames as u64,
            &mut buffer,
            &mut source,
        );

        assert_eq!(
            source.asked,
            frames * 2 * PADDED_WORD,
            "the ring was asked for fewer frames than the graph wanted"
        );
        assert_eq!(
            filled,
            frames * 2 * PACKED_WORD,
            "the graph was handed fewer packed frames than it asked for"
        );
    }

    #[test]
    fn a_ring_laid_out_for_another_layout_is_never_written_into_the_graphs_buffer() {
        let cycle = cycling(SampleFormat::S16);
        let five_wide = NonZeroU16::new(5 * 2).expect("a five-channel frame");
        let mut source = Counting {
            handed: vec![0x5A; 4_096],
            asked: 0,
        };
        let mut buffer = vec![0xFF; 4_096];

        let filled = cycle.write(
            Taken::Otherwise { frame: five_wide },
            64,
            &mut buffer,
            &mut source,
        );

        assert_eq!(
            filled,
            64 * 10,
            "the quantum was not counted in the graph's frames"
        );
        assert!(
            buffer[..filled].iter().all(|byte| *byte == 0),
            "the graph was handed something other than silence"
        );
        assert_eq!(
            source.asked, 0,
            "the ring was read while it could not be heard"
        );
        assert_eq!(source.handed.len(), 4_096);
        assert_eq!(
            cycle.frame_of(Taken::Otherwise { frame: five_wide }).get(),
            10,
            "the chunk's stride is not the graph's frame"
        );
    }

    #[test]
    fn a_ring_laid_out_as_the_graph_took_it_is_handed_over_as_ever() {
        let cycle = cycling(SampleFormat::S16);
        let mut source = Counting {
            handed: (0..=255).collect(),
            asked: 0,
        };
        let mut buffer = vec![0; 4_096];

        let filled = cycle.write(
            Taken::AsAsked(WireWord::Padded),
            16,
            &mut buffer,
            &mut source,
        );

        assert_eq!(filled, 64);
        assert_eq!(source.asked, 64);
        assert_eq!(buffer[..filled], (0..64).collect::<Vec<u8>>());
    }

    #[test]
    fn a_packed_word_keeps_the_three_bytes_the_sample_lives_in_and_drops_the_padding() {
        let mut buffer = words(4);
        let filled = pack(&mut buffer, 16);

        assert_eq!(filled, 12);
        assert_eq!(
            &buffer[..filled],
            &[0, 0, 0, 1, 0, 0, 2, 0, 0, 3, 0, 0],
            "the low three bytes of each word did not survive the pack"
        );
    }

    #[test]
    fn packing_is_three_quarters_of_what_the_ring_handed_over() {
        for frames in 0..64_usize {
            let samples = frames.saturating_mul(2);
            let mut buffer = words(samples);
            let written = samples.saturating_mul(PADDED_WORD);

            assert_eq!(pack(&mut buffer, written), samples * PACKED_WORD);
        }
    }

    #[test]
    fn a_read_that_stopped_part_way_through_a_word_packs_only_the_whole_ones() {
        let mut buffer = words(4);
        let filled = pack(&mut buffer, 14);

        assert_eq!(
            filled, 9,
            "a word the ring did not finish was put on the wire"
        );
        assert_eq!(&buffer[..filled], &[0, 0, 0, 1, 0, 0, 2, 0, 0]);
    }

    #[test]
    fn a_dop_marker_rides_the_top_byte_of_the_word_the_pack_keeps() {
        let mut buffer = Vec::new();
        for marker in DOP_MARKERS {
            buffer.extend_from_slice(&[0x34, 0x12, marker, 0x00]);
        }
        let written = buffer.len();
        let filled = pack(&mut buffer, written);

        assert_eq!(filled, 6);
        assert_eq!(&buffer[..filled], &[0x34, 0x12, 0x05, 0x34, 0x12, 0xFA]);
    }

    #[test]
    fn nothing_written_packs_to_nothing() {
        let mut buffer = words(4);

        assert_eq!(pack(&mut buffer, 0), 0);
        assert_eq!(pack(&mut buffer, 3), 0);
    }

    #[test]
    fn the_packed_stride_is_the_frame_a_device_taking_three_byte_words_expects() {
        assert_eq!(packed_stride(stereo(SampleFormat::S24)), 6);
        assert_eq!(stereo(SampleFormat::S24).bytes_per_frame().get(), 8);
        assert_eq!(
            packed_stride(StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Surround51,
                SampleFormat::S24,
            )),
            18
        );
    }
}
