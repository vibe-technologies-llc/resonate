#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_codec::{CoverArt, Drawing, ImageFormat};

fuzz_target!(|bytes: &[u8]| {
    let Some(format) = ImageFormat::sniff(bytes) else {
        return;
    };
    let art = CoverArt {
        format,
        bytes: bytes.to_vec(),
    };
    let Some(drawing) = Drawing::of(&art) else {
        return;
    };
    let side = std::num::NonZeroU32::new(64).expect("a side");
    let _ = drawing.no_larger_than(side);
    let _ = drawing.squared(side);
    let _ = drawing.likeness();
});
