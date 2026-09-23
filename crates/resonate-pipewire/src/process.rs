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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use libspa::utils::Fraction;
use pipewire::stream::Stream;
use resonate_core::{Ratio, SampleRate, StreamSpec};

use crate::{AudioSink, AudioSource, GraphTime};

const PADDED_WORD: usize = 4;
const PACKED_WORD: usize = 3;

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

pub(crate) struct Cycle {
    rate: SampleRate,
    stride: i32,
    packed_stride: i32,
    bytes_per_frame: usize,
    packed: Arc<AtomicBool>,
    latency: Arc<AtomicU64>,
}

impl Cycle {
    pub(crate) fn new(spec: StreamSpec, packed: Arc<AtomicBool>, latency: Arc<AtomicU64>) -> Self {
        Self {
            rate: spec.rate,
            stride: spec.bytes_per_frame().get() as i32,
            packed_stride: packed_stride(spec),
            bytes_per_frame: spec.bytes_per_frame().get() as usize,
            packed,
            latency,
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
        let packed = self.packed.load(Ordering::Relaxed);
        let filled = match data.data() {
            Some(slice) => match slice.get_mut(..self.asked_for(asked, slice.len())) {
                Some(quantum) => {
                    let written = Self::fill(source, quantum);
                    if packed {
                        pack(quantum, written)
                    } else {
                        written
                    }
                }
                None => 0,
            },
            None => 0,
        };

        let chunk = data.chunk_mut();
        *chunk.offset_mut() = 0;
        *chunk.stride_mut() = if packed {
            self.packed_stride
        } else {
            self.stride
        };
        *chunk.size_mut() = filled as u32;
    }

    fn asked_for(&self, frames: u64, room: usize) -> usize {
        let quantum = (frames as usize).saturating_mul(self.bytes_per_frame);
        if quantum == 0 {
            return room;
        }
        quantum.min(room)
    }

    fn fill(source: &mut dyn AudioSource, slice: &mut [u8]) -> usize {
        let wanted = slice.len();
        let written = source.fill(slice);
        if written < wanted {
            source.on_underrun(wanted - written);
        }
        written
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
            tick,
        };
        self.latency
            .store(ahead.downstream(self.rate).get(), Ordering::Relaxed);
    }
}

fn packed_stride(spec: StreamSpec) -> i32 {
    let samples = usize::from(spec.channel_count().get());
    samples.saturating_mul(PACKED_WORD) as i32
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
#[allow(clippy::indexing_slicing)]
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
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        )
    }

    #[test]
    fn a_cycle_takes_the_frames_the_graph_asked_for_rather_than_the_pool_it_was_handed() {
        let cycle = cycling(SampleFormat::S16);
        let room = 8_192;

        assert_eq!(
            cycle.asked_for(256, room),
            1_024,
            "the cycle read past the quantum the graph asked for"
        );
        assert_eq!(
            cycle.asked_for(0, room),
            room,
            "a graph that named no quantum left the buffer unfilled"
        );
        assert_eq!(
            cycle.asked_for(4_096, room),
            room,
            "a quantum larger than the buffer was written past its end"
        );
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
