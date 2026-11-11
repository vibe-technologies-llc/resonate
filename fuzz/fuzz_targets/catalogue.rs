#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_eq::{Catalogue, DeviceId, search, suggest};

fn assert_stays_inside_a_folder(id: &DeviceId) {
    let path = id.as_str();
    assert!(!path.starts_with('/'), "{path}");
    assert!(!path.contains('\\'), "{path}");
    assert!(
        path.split('/')
            .all(|segment| !segment.is_empty() && segment != "." && segment != ".."),
        "{path}"
    );
}

fuzz_target!(|text: &str| {
    if let Ok(id) = DeviceId::new(text) {
        assert_stays_inside_a_folder(&id);
    }

    let catalogue = Catalogue::read(text);
    for device in catalogue.devices() {
        assert_stays_inside_a_folder(&device.id);
        assert!(catalogue.get(&device.id).is_some());
    }

    let (typed, described) = text.split_at(text.floor_char_boundary(text.len() / 2));
    let _ = search(&catalogue, typed);
    let _ = suggest(&catalogue, described);
});
