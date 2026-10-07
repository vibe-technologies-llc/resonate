#![no_main]

mod held;

use libfuzzer_sys::fuzz_target;
use resonate_codec::{
    DecodeStatus, Decoder, Delivery, FormatHint, MediaInfo, Packing, probe, probe_cover_art,
    probe_span, probe_stream,
};
use resonate_core::{AUDIO_EXTENSIONS, AudioBuffer, FrameSpan, Frames, SampleFormat};

use crate::held::{Arrival, Shape};

const BLOCKS: usize = 64;
const BLOCKS_AFTER_A_SEEK: usize = 8;
const PIPED: u8 = 0b1;
const DSD_AS_SAMPLES: u8 = 0b10;
const HINT_SHIFT: u32 = 2;
const STEPS_OF_THE_LENGTH: u64 = 256;

struct Asked {
    shape: Shape,
    dsd_as_samples: bool,
    seeks: [u8; 2],
}

impl Asked {
    fn of(bytes: &[u8]) -> Self {
        let [.., ahead, behind, last] = bytes else {
            return Self {
                shape: Shape::FILE,
                dsd_as_samples: false,
                seeks: [0; 2],
            };
        };
        let arrival = if last & PIPED == 0 {
            Arrival::Seekable
        } else {
            Arrival::Piped
        };
        let hinted = usize::from(last >> HINT_SHIFT);
        let hint = hinted
            .checked_sub(1)
            .and_then(|at| AUDIO_EXTENSIONS.get(at % AUDIO_EXTENSIONS.len()))
            .map(|extension| FormatHint::Extension(Box::from(*extension)));

        Self {
            shape: Shape { arrival, hint },
            dsd_as_samples: last & DSD_AS_SAMPLES != 0,
            seeks: [*ahead, *behind],
        }
    }

    fn delivery(&self, info: &MediaInfo) -> Delivery {
        match info.packing {
            Packing::DopMarked(_) if !self.dsd_as_samples => Delivery {
                format: SampleFormat::S24,
                packing: info.packing,
            },
            _ => Delivery::samples(SampleFormat::F32),
        }
    }

    fn seek_to(step: u8, info: &MediaInfo) -> Frames {
        let length = info.duration.map_or(u64::from(u32::MAX), Frames::get);
        Frames(length / STEPS_OF_THE_LENGTH * u64::from(step))
    }
}

fn decode(decoder: &mut Decoder, out: &mut AudioBuffer, blocks: usize) {
    for _ in 0..blocks {
        match decoder.next_block(out) {
            Ok(DecodeStatus::EndOfStream) | Err(_) => break,
            Ok(_) => {}
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    let asked = Asked::of(bytes);
    let (sources, location) = held::held_as(bytes, asked.shape.clone());

    let _ = probe(&sources, &location);
    let _ = probe_stream(&sources, &location);
    let _ = probe_cover_art(&sources, &location);
    let _ = probe_span(
        &sources,
        &location,
        FrameSpan::between(Frames(0), Frames(1_024)),
    );

    if let Ok((mut decoder, info)) = Decoder::open(&sources, &location) {
        decoder.deliver(asked.delivery(&info));
        let mut out = AudioBuffer::empty(info.spec);
        decode(&mut decoder, &mut out, BLOCKS);

        for step in asked.seeks {
            if decoder.seek(Asked::seek_to(step, &info)).is_ok() {
                decode(&mut decoder, &mut out, BLOCKS_AFTER_A_SEEK);
            }
        }
        if decoder.seek(Frames::ZERO).is_ok() {
            decode(&mut decoder, &mut out, BLOCKS_AFTER_A_SEEK);
        }
        let _ = decoder.settle_the_spool();
    }
});
