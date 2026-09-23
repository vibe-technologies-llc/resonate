#![no_main]

mod held;

use libfuzzer_sys::fuzz_target;
use resonate_codec::{DecodeStatus, Decoder, probe, probe_cover_art, probe_span, probe_stream};
use resonate_core::{AudioBuffer, FrameSpan, Frames};

const BLOCKS: usize = 64;

fuzz_target!(|bytes: &[u8]| {
    let (sources, location) = held::held(bytes);

    let _ = probe(&sources, &location);
    let _ = probe_stream(&sources, &location);
    let _ = probe_cover_art(&sources, &location);
    let _ = probe_span(
        &sources,
        &location,
        FrameSpan::between(Frames(0), Frames(1_024)),
    );

    if let Ok((mut decoder, info)) = Decoder::open(&sources, &location) {
        let mut out = AudioBuffer::empty(info.spec);
        for _ in 0..BLOCKS {
            match decoder.next_block(&mut out) {
                Ok(DecodeStatus::EndOfStream) | Err(_) => break,
                Ok(_) => {}
            }
        }
    }
});
