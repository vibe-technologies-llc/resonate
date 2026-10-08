use std::fmt::Write;

use resonate_core::eq::{
    Band, BandGain, BandKind, ChannelSet, Frequency, MAX_BANDS, Preamp, Profile, Q,
};

use crate::{EqOp, Error, Result, graphic};

pub const LARGEST_PROFILE: usize = 64 * 1024;

const LINES_AT_MOST: usize = 4_096;
const PREAMP: &str = "preamp:";
const FILTER: &str = "filter";
const COMMENT: char = '#';
const CHANNEL: &str = "channel:";
const DEVICE: &str = "device:";
const EVERY_CHANNEL: &str = "all";
const EVERY_DEVICE: &str = "all";
const CHANNEL_NAMES: [&str; ChannelSet::NAMED_AT_MOST] =
    ["L", "R", "C", "SUB", "RL", "RR", "SL", "SR"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Reaching(ChannelSet),
    Unread,
}

fn scope_of(line: &str) -> Scope {
    let Some((_, tail)) = line.split_once(':') else {
        return Scope::Unread;
    };
    let mut named = Vec::new();
    for word in tail.split_whitespace() {
        if word.eq_ignore_ascii_case(EVERY_CHANNEL) {
            return Scope::Reaching(ChannelSet::EVERY);
        }
        let channel = CHANNEL_NAMES
            .iter()
            .position(|name| name.eq_ignore_ascii_case(word))
            .or_else(|| {
                word.parse::<usize>()
                    .ok()
                    .and_then(|number| number.checked_sub(1))
            });
        match channel {
            Some(channel) => named.push(channel),
            None => return Scope::Unread,
        }
    }
    ChannelSet::of(&named).map_or(Scope::Unread, Scope::Reaching)
}

fn spelled_channels(channels: ChannelSet) -> String {
    if channels.is_every() {
        return EVERY_CHANNEL.to_owned();
    }
    channels
        .held()
        .filter_map(|channel| CHANNEL_NAMES.get(channel).copied())
        .collect::<Vec<_>>()
        .join(" ")
}

pub struct Reading {
    pub profile: Profile,
    pub passed_over: usize,
    pub approximated: usize,
    pub clamped: usize,
}

pub fn read_number(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let spelled = if text.matches(',').count() == 1 && !text.contains('.') {
        text.replace(',', ".")
    } else {
        text.to_owned()
    };
    spelled
        .parse()
        .ok()
        .filter(|number: &f64| number.is_finite())
}

fn spelling_of(kind: BandKind) -> &'static str {
    match kind {
        BandKind::Peaking => "PK",
        BandKind::LowShelf => "LSC",
        BandKind::HighShelf => "HSC",
        BandKind::LowPass => "LPQ",
        BandKind::HighPass => "HPQ",
        BandKind::Notch => "NO",
        BandKind::BandPass => "BP",
        BandKind::AllPass => "AP",
    }
}

fn kind_of(code: &str) -> Option<BandKind> {
    match code.to_ascii_uppercase().as_str() {
        "PK" | "PEQ" | "MODAL" => Some(BandKind::Peaking),
        "LS" | "LSC" | "LSQ" => Some(BandKind::LowShelf),
        "HS" | "HSC" | "HSQ" => Some(BandKind::HighShelf),
        "LP" | "LPQ" => Some(BandKind::LowPass),
        "HP" | "HPQ" => Some(BandKind::HighPass),
        "NO" | "NOTCH" => Some(BandKind::Notch),
        "BP" => Some(BandKind::BandPass),
        "AP" => Some(BandKind::AllPass),
        _ => None,
    }
}

struct Held<T> {
    value: T,
    moved: bool,
}

fn held_within(value: f64, least: f64, most: f64) -> Held<f64> {
    let held = value.clamp(least, most);
    Held {
        value: held,
        moved: held != value,
    }
}

fn clamped_frequency(hertz: f64) -> Held<Frequency> {
    let held = held_within(
        hertz,
        f64::from(Frequency::LOWEST_CENTIHERTZ) / 100.0,
        f64::from(Frequency::HIGHEST_CENTIHERTZ) / 100.0,
    );
    Held {
        value: Frequency::from_hertz(held.value).unwrap_or(Frequency::LOWEST),
        moved: held.moved,
    }
}

fn clamped_gain(decibels: f64) -> Held<BandGain> {
    let widest = f64::from(BandGain::WIDEST_MILLI_DECIBELS) / 1_000.0;
    let held = held_within(decibels, -widest, widest);
    Held {
        value: BandGain::from_decibels(held.value).unwrap_or(BandGain::FLAT),
        moved: held.moved,
    }
}

fn clamped_q(units: f64) -> Held<Q> {
    let held = held_within(
        units,
        f64::from(Q::WIDEST_MILLI) / 1_000.0,
        f64::from(Q::NARROWEST_MILLI) / 1_000.0,
    );
    Held {
        value: Q::from_units(held.value).unwrap_or(Q::BUTTERWORTH),
        moved: held.moved,
    }
}

fn clamped_preamp(decibels: f64) -> Held<Preamp> {
    let widest = f64::from(BandGain::WIDEST_MILLI_DECIBELS) / 1_000.0;
    let held = held_within(decibels, -widest, widest);
    Held {
        value: Preamp::from_decibels(held.value).unwrap_or(Preamp::NONE),
        moved: held.moved,
    }
}

fn q_from_bandwidth(octaves: f64) -> f64 {
    let spread = 2.0_f64.powf(octaves);
    if spread <= 1.0 {
        return Q::BUTTERWORTH.units();
    }
    spread.sqrt() / (spread - 1.0)
}

fn q_from_slope(slope: f64, decibels: f64) -> f64 {
    if slope <= 0.0 {
        return Q::BUTTERWORTH.units();
    }
    let amplitude = 10.0_f64.powf(decibels / 40.0);
    let under = (amplitude + 1.0 / amplitude) * (1.0 / slope - 1.0) + 2.0;
    if under <= 0.0 {
        return Q::BUTTERWORTH.units();
    }
    1.0 / under.sqrt()
}

#[derive(Clone, Copy)]
enum Bandwidth {
    Octaves(f64),
    Hertz(f64),
}

impl Bandwidth {
    fn spelled(unit: &str, number: f64) -> Option<Self> {
        if unit.eq_ignore_ascii_case("oct") {
            Some(Self::Octaves(number))
        } else if unit.eq_ignore_ascii_case("hz") {
            Some(Self::Hertz(number))
        } else {
            None
        }
    }

    fn q_at(self, hertz: f64) -> f64 {
        match self {
            Self::Octaves(octaves) => q_from_bandwidth(octaves),
            Self::Hertz(wide) if wide > 0.0 => hertz / wide,
            Self::Hertz(_) => Q::BUTTERWORTH.units(),
        }
    }
}

#[derive(Default)]
struct Written {
    hertz: Option<f64>,
    decibels: Option<f64>,
    q: Option<f64>,
    bandwidth: Option<Bandwidth>,
    slope: Option<f64>,
    rolloff_db_an_octave: Option<u32>,
}

impl Written {
    fn read(&mut self, words: &[&str]) {
        let mut at = 0;
        while at < words.len() {
            let name = words.get(at).copied().unwrap_or_default();
            let value = words.get(at + 1).copied().and_then(read_number);
            let mut taken = 2;
            match name.to_ascii_uppercase().as_str() {
                "FC" | "FREQ" => {
                    let named_in_kilohertz = words
                        .get(at + 2)
                        .is_some_and(|unit| unit.eq_ignore_ascii_case("khz"));
                    let scale = if named_in_kilohertz { 1_000.0 } else { 1.0 };
                    self.hertz = value.map(|hertz| hertz * scale);
                }
                "GAIN" => self.decibels = value,
                "Q" => self.q = value,
                "BW" => {
                    let (bandwidth, read) = bandwidth_in(words.get(at + 1..).unwrap_or_default());
                    self.bandwidth = bandwidth;
                    taken = 1 + read;
                }
                "S" => self.slope = value,
                word => {
                    self.rolloff_db_an_octave = rolloff_in(word).or(self.rolloff_db_an_octave);
                    taken = 1;
                }
            }
            at += taken;
        }
    }

    fn q_for(&self, kind: BandKind, hertz: f64, decibels: f64) -> f64 {
        if let Some(q) = self.q {
            return q;
        }
        if let Some(bandwidth) = self.bandwidth {
            return bandwidth.q_at(hertz);
        }
        match self.slope {
            Some(slope) if kind.uses_gain() => q_from_slope(slope, decibels),
            _ => Q::BUTTERWORTH.units(),
        }
    }
}

fn bandwidth_in(words: &[&str]) -> (Option<Bandwidth>, usize) {
    let first = words.first().copied().unwrap_or_default();
    let second = words.get(1).copied().unwrap_or_default();
    match read_number(first) {
        Some(number) => match Bandwidth::spelled(second, number) {
            Some(bandwidth) => (Some(bandwidth), 2),
            None => (Some(Bandwidth::Octaves(number)), 1),
        },
        None => match read_number(second) {
            Some(number) => (Bandwidth::spelled(first, number), 2),
            None => (None, 1),
        },
    }
}

const SECOND_ORDER_DB_AN_OCTAVE: u32 = 12;

fn rolloff_in(word: &str) -> Option<u32> {
    word.strip_suffix("DB")?.parse().ok()
}

struct Taken {
    band: Band,
    approximated: bool,
    clamped: bool,
}

fn band_of(line: &str) -> Option<Taken> {
    let (_, tail) = line.split_once(':')?;
    let words: Vec<&str> = tail.split_whitespace().collect();

    let switched = words.first()?;
    let on = switched.eq_ignore_ascii_case("on");
    if !on && !switched.eq_ignore_ascii_case("off") {
        return None;
    }

    let kind = kind_of(words.get(1)?)?;
    let mut written = Written::default();
    written.read(words.get(2..).unwrap_or_default());

    let hertz = written.hertz?;
    let decibels = written.decibels.unwrap_or_default();
    let gain = if kind.uses_gain() {
        clamped_gain(decibels)
    } else {
        Held {
            value: BandGain::FLAT,
            moved: false,
        }
    };
    let frequency = clamped_frequency(hertz);
    let q = clamped_q(written.q_for(kind, hertz, decibels));

    let approximated = written
        .rolloff_db_an_octave
        .is_some_and(|rolloff| rolloff != SECOND_ORDER_DB_AN_OCTAVE);

    Some(Taken {
        band: Band {
            on,
            ..Band::new(kind, frequency.value, gain.value, q.value)
        },
        approximated,
        clamped: frequency.moved || gain.moved || q.moved,
    })
}

fn preamp_of(line: &str) -> Option<Held<Preamp>> {
    let (_, tail) = line.split_once(':')?;
    let spelled = tail.split_whitespace().next()?;
    read_number(spelled).map(clamped_preamp)
}

enum DeviceScope {
    Every,
    TheFirstNamed,
    Another,
}

fn device_scope(line: &str, first_named: &mut Option<String>) -> DeviceScope {
    let named = line
        .split_once(':')
        .map(|(_, tail)| tail.trim())
        .unwrap_or_default();
    if named.is_empty() || named.eq_ignore_ascii_case(EVERY_DEVICE) {
        return DeviceScope::Every;
    }
    let folded = named.to_lowercase();
    match first_named {
        Some(first) if *first == folded => DeviceScope::TheFirstNamed,
        Some(_) => DeviceScope::Another,
        None => {
            *first_named = Some(folded);
            DeviceScope::TheFirstNamed
        }
    }
}

pub fn read(text: &str) -> Result<Reading> {
    if text.len() > LARGEST_PROFILE {
        return Err(Error::TooLarge {
            op: EqOp::Parse,
            limit: LARGEST_PROFILE,
        });
    }

    let mut preamp = None;
    let mut target = None;
    let mut bands = Vec::new();
    let mut passed_over = 0;
    let mut approximated = 0;
    let mut clamped = 0;
    let mut lines = 0;
    let mut scope = Scope::Reaching(ChannelSet::EVERY);
    let mut first_device = None;
    let mut another_device = false;

    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with(COMMENT) {
            continue;
        }

        lines += 1;
        if lines > LINES_AT_MOST {
            return Err(Error::TooManyLines {
                limit: LINES_AT_MOST,
            });
        }

        let folded = line.to_ascii_lowercase();
        if folded.starts_with(DEVICE) {
            another_device = match device_scope(line, &mut first_device) {
                DeviceScope::Every | DeviceScope::TheFirstNamed => false,
                DeviceScope::Another => {
                    tracing::debug!(line, "a second device's correction is passed over");
                    true
                }
            };
            continue;
        }
        if another_device {
            passed_over += 1;
            continue;
        }
        if folded.starts_with(CHANNEL) {
            scope = scope_of(line);
            if scope == Scope::Unread {
                tracing::debug!(
                    line,
                    "a channel line naming a channel this build does not know"
                );
                passed_over += 1;
            }
            continue;
        }
        let reaching = match scope {
            Scope::Reaching(channels) => channels,
            Scope::Unread => {
                passed_over += 1;
                continue;
            }
        };

        if folded.starts_with(PREAMP) {
            match preamp_of(line) {
                Some(read) if reaching.is_every() => {
                    if read.moved {
                        tracing::debug!(line, "a preamp past what this build holds");
                        clamped += 1;
                    }
                    preamp = Some(read.value);
                }
                Some(_) => {
                    tracing::debug!(line, "a preamp for some channels and not others");
                    passed_over += 1;
                }
                None => passed_over += 1,
            }
            continue;
        }

        if folded.starts_with(graphic::LEADER) {
            match graphic::target_of(line) {
                Some(read) if reaching.is_every() && target.is_none() => target = Some(read),
                _ => {
                    tracing::debug!(line, "a graphic curve this build could not take");
                    passed_over += 1;
                }
            }
            continue;
        }

        if !folded.starts_with(FILTER) {
            tracing::debug!(line, "a line the parametric grammar does not name");
            passed_over += 1;
            continue;
        }

        if folded.contains(" none") {
            continue;
        }

        match band_of(line) {
            Some(taken) if bands.len() < MAX_BANDS => {
                if taken.approximated {
                    tracing::debug!(line, "a rolloff this build reads as a second-order filter");
                    approximated += 1;
                }
                if taken.clamped {
                    tracing::debug!(line, "a filter past what this build holds, held to it");
                    clamped += 1;
                }
                bands.push(Band {
                    channels: reaching,
                    ..taken.band
                });
            }
            Some(_) => {
                tracing::debug!(line, "a filter past the bands a profile holds");
                passed_over += 1;
            }
            None => {
                tracing::debug!(line, "a filter line this build could not read");
                passed_over += 1;
            }
        }
    }

    let profile = match target {
        Some(target) => {
            passed_over += bands.len();
            let mut fitted = Profile::fitted_to(target);
            if let Some(preamp) = preamp {
                fitted.set_preamp(preamp);
            }
            fitted
        }
        None => Profile::new(preamp.unwrap_or(Preamp::NONE), bands)?,
    };
    Ok(Reading {
        profile,
        passed_over,
        approximated,
        clamped,
    })
}

pub fn read_profile(text: &str) -> Result<Profile> {
    Ok(read(text)?.profile)
}

pub(crate) fn spelled_hertz(frequency: Frequency) -> String {
    if frequency.centihertz().is_multiple_of(100) {
        format!("{}", frequency.centihertz() / 100)
    } else {
        format!("{:.2}", frequency.hertz())
    }
}

pub fn write(profile: &Profile) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Preamp: {:.3} dB", profile.preamp().decibels());
    if let Some(target) = profile.target() {
        let _ = writeln!(text, "{}", graphic::spelled(target));
        return text;
    }

    let mut scope = ChannelSet::EVERY;
    for (position, band) in profile.bands().iter().enumerate() {
        if band.channels != scope {
            scope = band.channels;
            let _ = writeln!(text, "Channel: {}", spelled_channels(scope));
        }
        let _ = writeln!(
            text,
            "Filter {}: {} {} Fc {} Hz Gain {:.3} dB Q {:.3}",
            position + 1,
            if band.on { "ON" } else { "OFF" },
            spelling_of(band.kind),
            spelled_hertz(band.frequency),
            band.gain.decibels(),
            band.q.units(),
        );
    }
    text
}

pub fn write_profile(profile: &Profile) -> String {
    write(profile)
}

#[cfg(test)]
mod tests {
    use resonate_core::SampleRate;

    use super::*;

    const AUTOEQ: &str = include_str!("../tests/fixtures/autoeq_parametric.txt");

    fn band(kind: BandKind, at: f64, gain: f64, q: f64) -> Band {
        Band::new(
            kind,
            Frequency::from_hertz(at).expect("in range"),
            BandGain::from_decibels(gain).expect("in range"),
            Q::from_units(q).expect("in range"),
        )
    }

    #[test]
    fn a_measured_profile_reads_back_as_the_bands_it_names() {
        let reading = read(AUTOEQ).expect("a well formed profile");

        assert_eq!(reading.passed_over, 0);
        assert_eq!(reading.profile.bands().len(), 10);
        assert_eq!(reading.profile.preamp().milli_decibels(), -6_100);

        let first = reading.profile.bands().first().copied().expect("ten bands");
        assert_eq!(first.kind, BandKind::LowShelf);
        assert_eq!(first.frequency.centihertz(), 10_500);
        assert_eq!(first.gain.milli_decibels(), 6_400);
        assert_eq!(first.q.milli(), 700);
        assert!(first.on);

        let shelf = reading
            .profile
            .bands()
            .iter()
            .find(|band| band.kind == BandKind::HighShelf)
            .copied()
            .expect("a high shelf");
        assert_eq!(shelf.frequency.centihertz(), 1_000_000);
    }

    #[test]
    fn the_preamp_a_measurement_declares_is_the_peak_the_bands_reach() {
        let profile = read_profile(AUTOEQ).expect("a well formed profile");
        let peak = profile.peak_db(SampleRate::HZ_48000);

        assert!(
            (peak + profile.preamp().decibels()).abs() < 0.1,
            "a declared preamp of {} against a computed peak of {peak} dB",
            profile.preamp()
        );
    }

    #[test]
    fn what_is_written_reads_back_as_what_was_written() {
        for kind in BandKind::ALL {
            for hertz in [20.0, 105.0, 1_227.5, 8_800.0, 20_000.0] {
                for gain in [-24.0, -6.4, 0.0, 0.7, 18.5] {
                    for q in [0.1, 0.707, 1.42, 5.75, 40.0] {
                        let mut written = band(kind, hertz, gain, q);
                        written.on = q > 0.5;
                        let profile = Profile::new(
                            Preamp::from_decibels(-6.1).expect("in range"),
                            vec![written],
                        )
                        .expect("one band");

                        let read = read_profile(&write(&profile)).expect("what we wrote");
                        assert_eq!(read, profile, "{kind} at {hertz} Hz, {gain} dB, Q {q}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_filter_set_per_channel_is_read_as_bands_for_those_channels_and_written_back_so() {
        let text = "Preamp: -6 dB\n\
                    Channel: L\n\
                    Filter 1: ON PK Fc 100 Hz Gain 3 dB Q 1\n\
                    Channel: R\n\
                    Filter 2: ON PK Fc 200 Hz Gain -2 dB Q 1\n\
                    Channel: 1 2\n\
                    Filter 3: ON PK Fc 1000 Hz Gain 1 dB Q 1\n\
                    Channel: all\n\
                    Filter 4: ON PK Fc 5000 Hz Gain 1 dB Q 1\n";

        let reading = read(text).expect("a readable profile");
        let reaches: Vec<Vec<usize>> = reading
            .profile
            .bands()
            .iter()
            .map(|band| band.channels.held().collect())
            .collect();
        assert_eq!(
            reaches,
            [
                vec![0],
                vec![1],
                vec![0, 1],
                (0..ChannelSet::NAMED_AT_MOST).collect()
            ]
        );
        assert_eq!(reading.passed_over, 0);
        assert_eq!(
            read_profile(&write(&reading.profile)).ok(),
            Some(reading.profile)
        );
    }

    #[test]
    fn a_channel_this_build_cannot_name_passes_its_filters_over_rather_than_widening_them() {
        let text = "Channel: TOP\nFilter 1: ON PK Fc 100 Hz Gain 3 dB Q 1\n\
                    Channel: L\nPreamp: -3 dB\n";

        let reading = read(text).expect("a readable profile");
        assert!(reading.profile.bands().is_empty());
        assert_eq!(reading.profile.preamp(), Preamp::NONE);
        assert_eq!(reading.passed_over, 3);
    }

    #[test]
    fn a_profile_is_read_as_far_as_it_parses_and_never_fails_on_a_line() {
        let text = "\u{feff}# written by hand\r\n\
                    Preamp: -3.5 dB\r\n\
                    Filter 1: ON PK Fc 1000 Hz Gain 3.0 dB Q 1.41\r\n\
                    Filter 2: OFF None\r\n\
                    Filter 3: ON WAT Fc 200 Hz Gain 1 dB Q 1\r\n\
                    Filter 4: ON PK Gain 1 dB Q 1\r\n\
                    what is this line\r\n\
                    Filter 5: ON HS Fc 8000 Hz Gain -2,5 dB\r\n";

        let reading = read(text).expect("a file of rubbish still reads");

        assert_eq!(reading.profile.preamp().milli_decibels(), -3_500);
        assert_eq!(reading.profile.bands().len(), 2);
        assert_eq!(reading.passed_over, 3);

        let shelf = reading.profile.bands().get(1).copied().expect("two bands");
        assert_eq!(shelf.kind, BandKind::HighShelf);
        assert_eq!(shelf.gain.milli_decibels(), -2_500);
        assert_eq!(shelf.q, Q::BUTTERWORTH);
    }

    #[test]
    fn a_file_of_nothing_the_grammar_names_reads_as_a_profile_of_no_bands() {
        let reading = read("the quick brown fox\njumped over\n").expect("rubbish reads");

        assert!(reading.profile.bands().is_empty());
        assert_eq!(reading.profile.preamp(), Preamp::NONE);
        assert_eq!(reading.passed_over, 2);
    }

    #[test]
    fn a_value_past_what_a_band_holds_is_clamped_rather_than_dropped() {
        let reading = read("Filter 1: ON PK Fc 90000 Hz Gain 90 dB Q 900").expect("it reads");
        let band = reading.profile.bands().first().copied().expect("one band");

        assert_eq!(band.frequency, Frequency::HIGHEST);
        assert_eq!(band.gain.milli_decibels(), BandGain::WIDEST_MILLI_DECIBELS);
        assert_eq!(band.q.milli(), Q::NARROWEST_MILLI);
    }

    #[test]
    fn a_bandwidth_and_a_slope_are_read_as_the_q_they_stand_for() {
        let wide = read("Filter 1: ON PK Fc 1000 Hz Gain 3 dB BW Oct 1.0").expect("it reads");
        let band = wide.profile.bands().first().copied().expect("one band");
        assert!(
            (band.q.units() - std::f64::consts::SQRT_2).abs() < 0.01,
            "one octave read as Q {}",
            band.q
        );

        let shelf = read("Filter 1: ON LS Fc 100 Hz Gain 6 dB S 1.0").expect("it reads");
        let band = shelf.profile.bands().first().copied().expect("one band");
        assert!(
            (band.q.units() - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "a slope of one read as Q {}",
            band.q
        );
    }

    #[test]
    fn a_bandwidth_named_without_its_unit_leaves_the_gain_after_it_read() {
        let bare = read("Filter 1: ON PK Fc 1000 Hz BW 1.0 Gain 3 dB").expect("it reads");
        let band = bare.profile.bands().first().copied().expect("one band");
        assert!(
            (band.gain.decibels() - 3.0).abs() < 1e-9,
            "the gain read as {}",
            band.gain.decibels()
        );
        assert!(
            (band.q.units() - std::f64::consts::SQRT_2).abs() < 0.01,
            "one octave read as Q {}",
            band.q
        );
    }

    #[test]
    fn a_band_whose_kind_ignores_the_gain_is_written_without_one() {
        let reading = read("Filter 1: ON NO Fc 1000 Hz Gain 9 dB Q 4").expect("it reads");
        let band = reading.profile.bands().first().copied().expect("one band");

        assert_eq!(band.kind, BandKind::Notch);
        assert_eq!(band.gain, BandGain::FLAT);
    }

    #[test]
    fn the_parameters_are_read_by_name_rather_than_by_position() {
        let ordered = read("Filter 1: ON PK Q 2.5 Gain -4 dB Fc 3.15 kHz").expect("it reads");
        let band = ordered.profile.bands().first().copied().expect("one band");

        assert_eq!(band.frequency.centihertz(), 315_000);
        assert_eq!(band.gain.milli_decibels(), -4_000);
        assert_eq!(band.q.milli(), 2_500);
    }

    #[test]
    fn a_file_correcting_two_devices_is_read_as_the_first_of_them() {
        let text = "Device: Headphones One\n\
                    Filter 1: ON PK Fc 100 Hz Gain 3 dB Q 1\n\
                    Device: Speakers Two\n\
                    Filter 2: ON PK Fc 200 Hz Gain -6 dB Q 1\n\
                    Filter 3: ON PK Fc 300 Hz Gain -6 dB Q 1\n\
                    Device: all\n\
                    Filter 4: ON PK Fc 400 Hz Gain 2 dB Q 1\n";
        let reading = read(text).expect("it reads");

        let centres: Vec<u32> = reading
            .profile
            .bands()
            .iter()
            .map(|band| band.frequency.centihertz() / 100)
            .collect();
        assert_eq!(centres, vec![100, 400]);
        assert_eq!(reading.passed_over, 2);
    }

    #[test]
    fn a_bandwidth_is_read_in_the_unit_it_names() {
        let in_hertz = read("Filter 1: ON PK Fc 1000 Hz Gain 3 dB BW Hz 250").expect("it reads");
        let after = read("Filter 1: ON PK Fc 1000 Hz Gain 3 dB BW 250 Hz").expect("it reads");
        let in_octaves = read("Filter 1: ON PK Fc 1000 Hz Gain 3 dB BW Oct 1").expect("it reads");

        let q = |reading: &Reading| reading.profile.bands()[0].q.milli();
        assert_eq!(q(&in_hertz), 4_000);
        assert_eq!(q(&after), 4_000);
        assert_eq!(q(&in_octaves), 1_414);
    }

    #[test]
    fn a_value_past_what_a_band_holds_is_held_to_it_and_counted() {
        let reading = read(
            "Preamp: -400 dB\n\
             Filter 1: ON PK Fc 1000 Hz Gain 300 dB Q 1\n\
             Filter 2: ON PK Fc 1000 Hz Gain 3 dB Q 1\n",
        )
        .expect("it reads");

        assert_eq!(reading.clamped, 2);
        assert_eq!(reading.profile.bands().len(), 2);
    }

    #[test]
    fn a_profile_past_the_ceilings_is_refused_rather_than_truncated() {
        let long = "x\n".repeat(LINES_AT_MOST + 1);
        assert!(matches!(read(&long), Err(Error::TooManyLines { .. })));

        let wide = "x".repeat(LARGEST_PROFILE + 1);
        assert!(matches!(read(&wide), Err(Error::TooLarge { .. })));
    }

    #[test]
    fn a_rolloff_word_other_than_second_order_is_read_and_counted_as_approximated() {
        let reading = read(
            "Filter 1: ON LS 6dB Fc 105 Hz Gain 4 dB\n\
             Filter 2: ON HP 12dB Fc 30 Hz\n\
             Filter 3: ON LP 24dB Fc 18000 Hz\n\
             Filter 4: ON PK Fc 1000 Hz Gain 2 dB Q 1",
        )
        .expect("it reads");

        assert_eq!(reading.profile.bands().len(), 4);
        assert_eq!(reading.approximated, 2);
        assert_eq!(reading.passed_over, 0);
    }

    #[test]
    fn a_filter_past_the_bands_a_profile_holds_is_passed_over_like_any_unreadable_line() {
        let many = (1..=MAX_BANDS + 2)
            .map(|at| format!("Filter {at}: ON PK Fc {} Hz Gain 1 dB Q 1\n", 100 * at))
            .collect::<String>();

        let reading = read(&many).expect("the first bands still read");
        let last = reading.profile.bands().last().copied().expect("bands");

        assert_eq!(reading.profile.bands().len(), MAX_BANDS);
        assert_eq!(reading.passed_over, 2);
        assert_eq!(last.frequency.centihertz(), 10_000 * MAX_BANDS as u32);
    }

    #[test]
    fn a_comment_line_is_no_line_passed_over_and_a_filter_inside_one_is_not_read() {
        let text = "# measured on a rig\nPreamp: -3 dB\n# Filter 2: ON PK Fc 900 Hz Gain 4 dB Q 1\n\
                    Filter 1: ON PK Fc 100 Hz Gain 1 dB Q 1\n";

        let reading = read(text).expect("it reads");

        assert_eq!(reading.passed_over, 0);
        assert_eq!(reading.profile.bands().len(), 1);
        assert_eq!(reading.profile.preamp().decibels(), -3.0);
    }
}
