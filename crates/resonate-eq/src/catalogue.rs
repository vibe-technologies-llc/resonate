use crate::{Error, Result};

pub const FOUND_AT_MOST: usize = 24;
const PATH_AT_MOST: usize = 256;

const SAID_BY_THE_GRAPH: [&str; 37] = [
    "analog",
    "digital",
    "stereo",
    "mono",
    "surround",
    "audio",
    "output",
    "input",
    "sink",
    "source",
    "usb",
    "hdmi",
    "displayport",
    "spdif",
    "iec958",
    "built",
    "in",
    "device",
    "controller",
    "hd",
    "pro",
    "speaker",
    "speakers",
    "headphone",
    "headphones",
    "headset",
    "earphones",
    "a2dp",
    "sco",
    "handsfree",
    "bluetooth",
    "family",
    "generic",
    "default",
    "high",
    "definition",
    "wireless",
];

const NOT_A_DEVICE: [&str; 12] = [
    "adapter",
    "jack",
    "dock",
    "interface",
    "receiver",
    "dac",
    "amp",
    "amplifier",
    "hdmi",
    "displayport",
    "spdif",
    "iec958",
];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(Box<str>);

impl DeviceId {
    pub fn new(path: &str) -> Result<Self> {
        let path = path.trim().trim_start_matches("./");
        let usable = !path.is_empty()
            && path.len() <= PATH_AT_MOST
            && !path.starts_with('/')
            && !path.contains('\\')
            && !path.chars().any(char::is_control)
            && path
                .split('/')
                .all(|segment| !segment.is_empty() && segment != "." && segment != "..");

        if !usable {
            return Err(Error::NameNotUsable);
        }
        Ok(Self(path.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DeviceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    pub id: DeviceId,
    pub label: String,
    pub measured_by: String,
    pub rig: Option<String>,
}

impl Device {
    pub fn shown(&self) -> String {
        match self.rig.as_deref() {
            Some(rig) => format!("{} — {} on {rig}", self.label, self.measured_by),
            None => format!("{} — {}", self.label, self.measured_by),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Found {
    pub at: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Catalogue {
    devices: Vec<Device>,
    folded: Vec<String>,
    words: Vec<Vec<String>>,
}

fn folded(text: &str) -> String {
    let spelled: String = text
        .chars()
        .map(|glyph| if glyph.is_alphanumeric() { glyph } else { ' ' })
        .collect();
    spelled
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn words_of(text: &str) -> Vec<String> {
    folded(text).split_whitespace().map(str::to_owned).collect()
}

fn percent_decoded(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let raw: Vec<u8> = text.bytes().collect();
    let mut bytes = Vec::with_capacity(raw.len());
    let mut at = 0;
    while at < raw.len() {
        let byte = raw.get(at).copied().unwrap_or(b' ');
        let pair = (raw.get(at + 1).copied(), raw.get(at + 2).copied());
        match (byte, pair) {
            (b'%', (Some(high), Some(low))) => {
                match u8::from_str_radix(&format!("{}{}", high as char, low as char), 16) {
                    Ok(decoded) => {
                        bytes.push(decoded);
                        at += 3;
                    }
                    Err(_) => {
                        bytes.push(byte);
                        at += 1;
                    }
                }
            }
            _ => {
                bytes.push(byte);
                at += 1;
            }
        }
    }
    out.push_str(&String::from_utf8_lossy(&bytes));
    out
}

fn entry_of(line: &str) -> Option<Device> {
    let line = line.trim();
    let rest = line.strip_prefix("- [")?;
    let (label, rest) = rest.split_once("](")?;
    let (path, tail) = rest.split_once(") by ")?;
    let (measured_by, rig) = match tail.trim().split_once(" on ") {
        Some((measured_by, rig)) => (measured_by.trim(), Some(rig.trim().to_owned())),
        None => (tail.trim(), None),
    };

    let id = DeviceId::new(&percent_decoded(path)).ok()?;
    let tail_of_path = id.as_str().rsplit('/').next().unwrap_or_default();
    if tail_of_path != label {
        tracing::debug!(label, path = id.as_str(), "an index row names two devices");
        return None;
    }

    Some(Device {
        id,
        label: label.to_owned(),
        measured_by: measured_by.to_owned(),
        rig: rig.filter(|rig| !rig.is_empty()),
    })
}

impl Catalogue {
    pub fn read(index: &str) -> Self {
        let devices: Vec<Device> = index.lines().filter_map(entry_of).collect();
        Self::of(devices)
    }

    pub fn of(devices: Vec<Device>) -> Self {
        let folded: Vec<String> = devices.iter().map(|device| folded(&device.label)).collect();
        let words: Vec<Vec<String>> = devices
            .iter()
            .map(|device| words_of(&device.label))
            .collect();

        Self {
            devices,
            folded,
            words,
        }
    }

    pub fn empty() -> Self {
        Self::default()
    }

    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    pub fn len(&self) -> usize {
        self.devices.len()
    }

    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    pub fn device(&self, found: Found) -> Option<&Device> {
        self.devices.get(found.at)
    }

    pub fn get(&self, id: &DeviceId) -> Option<&Device> {
        self.devices.iter().find(|device| &device.id == id)
    }
}

pub fn search(catalogue: &Catalogue, typed: &str) -> Vec<Found> {
    let wanted = words_of(typed);
    if wanted.is_empty() {
        return Vec::new();
    }

    let mut hits: Vec<(usize, usize)> = catalogue
        .folded
        .iter()
        .enumerate()
        .filter(|(_, held)| wanted.iter().all(|word| held.contains(word.as_str())))
        .map(|(at, held)| (held.len(), at))
        .collect();

    hits.sort_by(|one, other| {
        one.0.cmp(&other.0).then_with(|| {
            catalogue
                .folded
                .get(one.1)
                .cmp(&catalogue.folded.get(other.1))
        })
    });
    hits.into_iter()
        .take(FOUND_AT_MOST)
        .map(|(_, at)| Found { at })
        .collect()
}

fn telling(word: &str) -> bool {
    !SAID_BY_THE_GRAPH.contains(&word)
}

fn names_a_model(words: &[String]) -> bool {
    let telling: Vec<&String> = words.iter().filter(|word| telling(word)).collect();
    if telling.len() >= 2 {
        return true;
    }
    telling.iter().any(|word| {
        let digits = word.chars().filter(char::is_ascii_digit).count();
        let letters = word.chars().any(char::is_alphabetic);
        (digits > 0 && letters) || (digits >= 3 && !letters)
    })
}

pub fn suggest(catalogue: &Catalogue, description: &str) -> Option<Found> {
    let said = words_of(description);
    if said
        .iter()
        .any(|word| NOT_A_DEVICE.contains(&word.as_str()))
    {
        return None;
    }

    let mut best: Vec<(u8, usize, usize)> = Vec::new();
    for (at, words) in catalogue.words.iter().enumerate() {
        if words.len() < 2 {
            continue;
        }
        if words.iter().all(|word| said.contains(word)) {
            best.push((2, words.len(), at));
            continue;
        }
        let Some((_, tail)) = words.split_first() else {
            continue;
        };
        if tail.is_empty() || !tail.iter().all(|word| said.contains(word)) {
            continue;
        }
        if names_a_model(tail) {
            best.push((1, words.len(), at));
        }
    }

    best.sort_by(|one, other| other.0.cmp(&one.0).then_with(|| other.1.cmp(&one.1)));
    let top = best.first().copied()?;
    let named = catalogue.folded.get(top.2);
    let rivals = best
        .iter()
        .filter(|held| (held.0, held.1) == (top.0, top.1) && catalogue.folded.get(held.2) != named)
        .count();
    (rivals == 0).then_some(Found { at: top.2 })
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDEX: &str = include_str!("../tests/fixtures/autoeq_index.md");

    fn catalogue() -> Catalogue {
        Catalogue::read(INDEX)
    }

    fn found(catalogue: &Catalogue, typed: &str) -> Vec<String> {
        search(catalogue, typed)
            .into_iter()
            .filter_map(|found| catalogue.device(found))
            .map(|device| device.label.clone())
            .collect()
    }

    fn suggested(catalogue: &Catalogue, description: &str) -> Option<String> {
        suggest(catalogue, description)
            .and_then(|found| catalogue.device(found))
            .map(|device| device.label.clone())
    }

    #[test]
    fn an_index_row_reads_back_as_the_device_it_names() {
        let catalogue = catalogue();
        let hd650 = catalogue
            .devices()
            .iter()
            .find(|device| device.label == "Sennheiser HD 650")
            .expect("the index names it");

        assert_eq!(hd650.id.as_str(), "oratory1990/over-ear/Sennheiser HD 650");
        assert_eq!(hd650.measured_by, "oratory1990");
        assert_eq!(hd650.rig, None);
        assert_eq!(hd650.shown(), "Sennheiser HD 650 — oratory1990");
    }

    #[test]
    fn a_row_that_is_not_a_device_is_passed_over_rather_than_read() {
        let catalogue = catalogue();

        assert!(
            !catalogue
                .devices()
                .iter()
                .any(|device| device.label == "A Name The Row Does Not Match"),
            "a row whose path names another device was taken"
        );
        assert!(
            !catalogue
                .devices()
                .iter()
                .any(|device| device.label == "No Attribution"),
            "a row naming nobody was taken"
        );
    }

    #[test]
    fn a_row_whose_path_walks_upward_is_refused() {
        let catalogue = catalogue();

        assert!(
            !catalogue
                .devices()
                .iter()
                .any(|device| device.label == "Walks Upward"),
            "a row walking out of the results folder was taken"
        );
        assert!(DeviceId::new("../../etc/passwd").is_err());
        assert!(DeviceId::new("/etc/passwd").is_err());
        assert!(DeviceId::new("a\\b").is_err());
        assert!(DeviceId::new("").is_err());
        assert!(DeviceId::new("./oratory1990/over-ear/Sennheiser HD 650").is_ok());
    }

    #[test]
    fn a_search_answers_the_devices_whose_name_holds_every_word_typed() {
        let catalogue = catalogue();

        assert_eq!(found(&catalogue, "hd 650"), ["Sennheiser HD 650"]);
        assert_eq!(found(&catalogue, "porta pro"), ["Koss Porta Pro"]);
        assert!(found(&catalogue, "xm4").contains(&"Sony WH-1000XM4".to_owned()));
        assert!(found(&catalogue, "").is_empty());
        assert!(found(&catalogue, "no such headphone").is_empty());
    }

    #[test]
    fn a_search_answers_the_shortest_name_that_holds_the_words_first() {
        let catalogue = catalogue();
        let hits = found(&catalogue, "koss");

        assert_eq!(hits.first().map(String::as_str), Some("Koss Porta Pro"));
    }

    #[test]
    fn a_sink_named_after_a_headphone_suggests_that_headphone() {
        let catalogue = catalogue();

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
            (
                "SteelSeries Arctis Nova Pro Wireless",
                "SteelSeries Arctis Nova Pro Wireless",
            ),
            ("LE-WH-1000XM4", "Sony WH-1000XM4"),
        ] {
            assert_eq!(
                suggested(&catalogue, description).as_deref(),
                Some(wanted),
                "{description}"
            );
        }
    }

    #[test]
    fn a_sink_that_names_a_socket_rather_than_a_headphone_suggests_nothing() {
        let catalogue = catalogue();

        for description in [
            "USB-C to 3.5mm Headphone Jack Adapter Analog Stereo",
            "Built-in Audio Analog Stereo",
            "Topping E30 Analog Stereo",
            "Family 17h/19h HD Audio Controller Analog Stereo",
            "Scarlett 2i2 USB",
            "Navi 31 HDMI/DP Audio",
            "Starship/Matisse HD Audio",
            "Dummy Output",
            "",
        ] {
            assert_eq!(
                suggested(&catalogue, description),
                None,
                "{description} was taken for a headphone"
            );
        }
    }

    #[test]
    fn a_description_two_devices_answer_to_equally_well_suggests_neither() {
        let catalogue = Catalogue::of(vec![
            Device {
                id: DeviceId::new("a/over-ear/Maker Thing One").expect("a path"),
                label: "Maker Thing One".to_owned(),
                measured_by: "a".to_owned(),
                rig: None,
            },
            Device {
                id: DeviceId::new("b/over-ear/Maker Thing Two").expect("a path"),
                label: "Maker Thing Two".to_owned(),
                measured_by: "b".to_owned(),
                rig: None,
            },
            Device {
                id: DeviceId::new("c/over-ear/Maker Else").expect("a path"),
                label: "Maker Else".to_owned(),
                measured_by: "c".to_owned(),
                rig: None,
            },
        ]);

        assert_eq!(
            suggest(&catalogue, "Maker Thing One Thing Two Analog Stereo"),
            None
        );
    }

    #[test]
    fn an_empty_catalogue_answers_nothing_to_anything() {
        let catalogue = Catalogue::empty();

        assert!(catalogue.is_empty());
        assert_eq!(catalogue.len(), 0);
        assert!(search(&catalogue, "hd 650").is_empty());
        assert_eq!(suggest(&catalogue, "Sennheiser HD 650"), None);
    }

    #[test]
    fn a_rig_is_carried_where_the_index_names_one() {
        let catalogue = Catalogue::of(vec![Device {
            id: DeviceId::new("crinacle/711 in-ear/Thing").expect("a path"),
            label: "Thing".to_owned(),
            measured_by: "crinacle".to_owned(),
            rig: Some("711".to_owned()),
        }]);
        let device = catalogue.devices().first().expect("one device");

        assert_eq!(device.shown(), "Thing — crinacle on 711");
    }
}
