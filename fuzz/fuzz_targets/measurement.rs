#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_eq::{POINTS_AT_LEAST, read_response};

const PROBED_HERTZ: [f64; 5] = [1.0, 20.0, 1_000.0, 20_000.0, 1_000_000.0];

fuzz_target!(|bytes: &[u8]| {
    let Ok(reading) = read_response(bytes) else {
        return;
    };

    let points = reading.response.points();
    assert!(points.len() >= POINTS_AT_LEAST);
    assert!(points.windows(2).all(|pair| pair[0].hertz < pair[1].hertz));

    for hertz in PROBED_HERTZ {
        assert!(reading.response.at(hertz).is_finite());
        assert!(reading.response.smoothed_at(hertz, 1.0 / 3.0).is_finite());
    }
    let _ = reading.response.calibrated_by(&reading.response);
});
