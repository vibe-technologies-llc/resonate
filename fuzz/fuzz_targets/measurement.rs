#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_core::eq::MAX_BANDS;
use resonate_eq::{BuiltIn, Fitting, POINTS_AT_LEAST, fit, read_response};

mod stage;

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

    for target in BuiltIn::ALL {
        let goal = target.response().expect("a built-in target reads");

        if let Ok(fitted) = fit(&reading.response, &goal, &Fitting::default()) {
            assert!(fitted.profile.bands().len() <= MAX_BANDS);
            assert!(fitted.off_by_db_before.is_finite());
            assert!(fitted.off_by_db_after.is_finite());

            stage::holds_under_full_scale(&fitted.profile);
        }
    }
});
