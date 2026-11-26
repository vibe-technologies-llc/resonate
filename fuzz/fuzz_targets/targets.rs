#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_eq::BuiltIn;

fuzz_target!(|text: &str| {
    let Some(target) = BuiltIn::parse(text) else {
        return;
    };

    assert!(target.name().eq_ignore_ascii_case(text.trim()));
    assert_eq!(BuiltIn::parse(target.name()), Some(target));

    let response = target.response().expect("a built-in target reads");
    assert!(response.at(1_000.0).is_finite());
});
