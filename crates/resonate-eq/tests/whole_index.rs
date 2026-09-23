use std::{env, fs};

use resonate_eq::{Catalogue, search, suggest};

#[test]
fn the_whole_index_reads_and_matches_the_way_the_sample_did() {
    let Some(path) = env::var_os("RESONATE_AUTOEQ_INDEX") else {
        eprintln!("skipped: set RESONATE_AUTOEQ_INDEX to a results/INDEX.md to walk it");
        return;
    };
    let text = fs::read_to_string(path).expect("the index reads");
    let catalogue = Catalogue::read(&text);
    eprintln!("catalogue holds {} devices", catalogue.len());
    assert!(catalogue.len() > 8_000);

    let named = |description: &str| {
        suggest(&catalogue, description)
            .and_then(|found| catalogue.device(found))
            .map(|device| device.label.clone())
    };

    for (description, wanted) in [
        ("Sony WH-1000XM4", "Sony WH-1000XM4"),
        ("WH-1000XM5", "Sony WH-1000XM5"),
        ("Sennheiser HD 650", "Sennheiser HD 650"),
        ("HD 600 Analog Stereo", "Sennheiser HD 600"),
        ("Bose QuietComfort 35", "Bose QuietComfort 35"),
        ("Sennheiser HD 800 S", "Sennheiser HD 800 S"),
        ("Moondrop Aria", "Moondrop Aria"),
        ("beyerdynamic DT 770 PRO", "Beyerdynamic DT 770 Pro"),
        ("Galaxy Buds2 Pro", "Samsung Galaxy Buds2 Pro"),
        ("Pixel Buds Pro", "Google Pixel Buds Pro"),
        ("Jabra Elite 75t", "Jabra Elite 75t"),
        ("HyperX Cloud II Wireless", "HyperX Cloud II Wireless"),
        ("LE-WH-1000XM4", "Sony WH-1000XM4"),
    ] {
        assert_eq!(named(description).as_deref(), Some(wanted), "{description}");
    }

    for description in [
        "USB-C to 3.5mm Headphone Jack Adapter Analog Stereo",
        "Built-in Audio Analog Stereo",
        "Topping E30 Analog Stereo",
        "Family 17h/19h HD Audio Controller Analog Stereo",
        "Scarlett 2i2 USB",
        "Navi 31 HDMI/DP Audio",
        "Starship/Matisse HD Audio",
    ] {
        assert_eq!(
            named(description),
            None,
            "{description} was taken for a headphone"
        );
    }

    let hits: Vec<&str> = search(&catalogue, "hd 650")
        .into_iter()
        .filter_map(|found| catalogue.device(found))
        .map(|device| device.label.as_str())
        .collect();
    assert_eq!(hits.first(), Some(&"Sennheiser HD 650"));
}
