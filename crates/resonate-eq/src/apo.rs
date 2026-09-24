use std::fmt::Write;

use resonate_core::eq::{
    Band, BandGain, BandKind, ChannelSet, Frequency, MAX_BANDS, Preamp, Profile, Q,
};

use crate::{EqOp, Error, Result, graphic};

pub const LARGEST_PROFILE: usize = 64 * 1024;

const LINES_AT_MOST: usize = 4_096;
const PREAMP: &str = "preamp:";
const FILTER: &str = "filter";
const CHANNEL: &str = "channel:";
const EVERY_CHANNEL: &str = "all";
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

fn clamped_frequency(hertz: f64) -> Frequency {
    let held = hertz.clamp(
        f64::from(Frequency::LOWEST_CENTIHERTZ) / 100.0,
        f64::from(Frequency::HIGHEST_CENTIHERTZ) / 100.0,
    );
    Frequency::from_hertz(held).unwrap_or(Frequency::LOWEST)
}

fn clamped_gain(decibels: f64) -> BandGain {
    let widest = f64::from(BandGain::WIDEST_MILLIBELS) / 1_000.0;
    BandGain::from_decibels(decibels.clamp(-widest, widest)).unwrap_or(BandGain::FLAT)
}

fn clamped_q(units: f64) -> Q {
    let held = units.clamp(
        f64::from(Q::WIDEST_MILLI) / 1_000.0,
        f64::from(Q::NARROWEST_MILLI) / 1_000.0,
    );
    Q::from_units(held).unwrap_or(Q::BUTTERWORTH)
}

fn clamped_preamp(decibels: f64) -> Preamp {
    let widest = f64::from(BandGain::WIDEST_MILLIBELS) / 1_000.0;
    Preamp::from_decibels(decibels.clamp(-widest, widest)).unwrap_or(Preamp::NONE)
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

#[derive(Default)]
struct Written {
    hertz: Option<f64>,
    decibels: Option<f64>,
    q: Option<f64>,
    bandwidth: Option<f64>,
    slope: Option<f64>,
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
                "BW" if value.is_some() => self.bandwidth = value,
                "BW" => {
                    self.bandwidth = words.get(at + 2).copied().and_then(read_number);
                    taken = 3;
                }
                "S" => self.slope = value,
                _ => taken = 1,
            }
            at += taken;
        }
    }

    fn q_for(&self, kind: BandKind, decibels: f64) -> f64 {
        if let Some(q) = self.q {
            return q;
        }
        if let Some(octaves) = self.bandwidth {
            return q_from_bandwidth(octaves);
        }
        match self.slope {
            Some(slope) if kind.uses_gain() => q_from_slope(slope, decibels),
            _ => Q::BUTTERWORTH.units(),
        }
    }
}

fn band_of(line: &str) -> Option<Band> {
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
        BandGain::FLAT
    };

    Some(Band {
        on,
        ..Band::new(
            kind,
            clamped_frequency(hertz),
            gain,
            clamped_q(written.q_for(kind, decibels)),
        )
    })
}

fn preamp_of(line: &str) -> Option<Preamp> {
    let (_, tail) = line.split_once(':')?;
    let spelled = tail.split_whitespace().next()?;
    read_number(spelled).map(clamped_preamp)
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
    let mut lines = 0;
    let mut scope = Scope::Reaching(ChannelSet::EVERY);

    for line in text.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }

        lines += 1;
        if lines > LINES_AT_MOST {
            return Err(Error::TooLarge {
                op: EqOp::Parse,
                limit: LINES_AT_MOST,
            });
        }

        let folded = line.to_ascii_lowercase();
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
                Some(read) if reaching.is_every() => preamp = Some(read),
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
            Some(band) if bands.len() < MAX_BANDS => bands.push(Band {
                channels: reaching,
                ..band
            }),
            Some(_) => {
                return Err(Error::Domain(resonate_core::Error::TooManyBands(
                    bands.len() + 1,
                )));
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
        assert_eq!(reading.profile.preamp().millibels(), -6_100);

        let first = reading.profile.bands().first().copied().expect("ten bands");
        assert_eq!(first.kind, BandKind::LowShelf);
        assert_eq!(first.frequency.centihertz(), 10_500);
        assert_eq!(first.gain.millibels(), 6_400);
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

        assert_eq!(reading.profile.preamp().millibels(), -3_500);
        assert_eq!(reading.profile.bands().len(), 2);
        assert_eq!(reading.passed_over, 4);

        let shelf = reading.profile.bands().get(1).copied().expect("two bands");
        assert_eq!(shelf.kind, BandKind::HighShelf);
        assert_eq!(shelf.gain.millibels(), -2_500);
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
        assert_eq!(band.gain.millibels(), BandGain::WIDEST_MILLIBELS);
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
        assert_eq!(band.gain.millibels(), -4_000);
        assert_eq!(band.q.milli(), 2_500);
    }

    #[test]
    fn a_profile_past_the_ceilings_is_refused_rather_than_truncated() {
        let long = "Filter 1: ON PK Fc 1000 Hz Gain 1 dB Q 1\n".repeat(LINES_AT_MOST + 1);
        assert!(matches!(read(&long), Err(Error::TooLarge { .. })));

        let wide = "x".repeat(LARGEST_PROFILE + 1);
        assert!(matches!(read(&wide), Err(Error::TooLarge { .. })));

        let many = (1..=MAX_BANDS + 1)
            .map(|at| format!("Filter {at}: ON PK Fc 1000 Hz Gain 1 dB Q 1\n"))
            .collect::<String>();
        assert!(matches!(
            read(&many),
            Err(Error::Domain(resonate_core::Error::TooManyBands(_)))
        ));
    }
}
