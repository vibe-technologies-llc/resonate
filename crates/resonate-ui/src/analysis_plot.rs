use std::time::Duration;

use resonate_engine::{Envelope, Ramp, Reach, Spectrum};

pub(crate) const WAVEFORM_COLUMNS: usize = 720;
pub(crate) const SPECTRUM_COLUMNS: usize = 480;
pub(crate) const SPECTRUM_FLOOR_DB: f32 = -130.0;
pub(crate) const SPECTRUM_CEILING_DB: f32 = 0.0;
pub(crate) const SPECTRUM_MARKED_EVERY_DB: f32 = 20.0;

const FREQUENCY_STEPS_HZ: [u32; 6] = [2_000, 5_000, 10_000, 20_000, 50_000, 100_000];
const FREQUENCY_MARKS_AT_MOST: u32 = 6;
const TIME_STEPS_SECONDS: [u64; 9] = [5, 10, 15, 30, 60, 120, 300, 600, 1_800];
const TIME_MARKS_AT_MOST: u64 = 8;
const HZ_A_KILOHERTZ: u32 = 1_000;
const SECONDS_A_MINUTE: u64 = 60;
const CHANNEL_BITS: u32 = 8;
const CHANNEL_MASK: u32 = 0xff;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    pub(crate) share: f32,
    pub(crate) label: String,
}

pub(crate) fn lanes_of(envelope: &Envelope, columns: usize) -> Vec<Vec<Reach>> {
    (0..envelope.lanes())
        .map(|lane| envelope.condensed(lane, columns))
        .collect()
}

pub(crate) fn traced(spectrum: &Spectrum, columns: usize) -> Vec<f32> {
    let nyquist = spectrum.nyquist_hz();
    (0..columns)
        .map(|column| {
            let from = nyquist * column as f32 / columns as f32;
            let to = nyquist * (column + 1) as f32 / columns as f32;
            spectrum
                .band_db(from, to)
                .unwrap_or(SPECTRUM_FLOOR_DB)
                .clamp(SPECTRUM_FLOOR_DB, SPECTRUM_CEILING_DB)
        })
        .collect()
}

pub(crate) fn height_of(db: f32) -> f32 {
    ((db - SPECTRUM_FLOOR_DB) / (SPECTRUM_CEILING_DB - SPECTRUM_FLOOR_DB)).clamp(0.0, 1.0)
}

pub(crate) fn frequency_marks(nyquist_hz: u32) -> Vec<Mark> {
    if nyquist_hz == 0 {
        return Vec::new();
    }
    let step = FREQUENCY_STEPS_HZ
        .into_iter()
        .find(|step| nyquist_hz / step <= FREQUENCY_MARKS_AT_MOST)
        .unwrap_or(nyquist_hz);
    (1..)
        .map(|nth| nth * step)
        .take_while(|hz| *hz < nyquist_hz)
        .map(|hz| Mark {
            share: hz as f32 / nyquist_hz as f32,
            label: format!("{}k", hz / HZ_A_KILOHERTZ),
        })
        .collect()
}

pub(crate) fn time_marks(length: Duration) -> Vec<Mark> {
    let seconds = length.as_secs();
    if seconds == 0 {
        return Vec::new();
    }
    let step = TIME_STEPS_SECONDS
        .into_iter()
        .find(|step| seconds / step <= TIME_MARKS_AT_MOST)
        .unwrap_or(seconds);
    (1..)
        .map(|nth| nth * step)
        .take_while(|at| *at < seconds)
        .map(|at| Mark {
            share: at as f32 / length.as_secs_f32(),
            label: format!("{}:{:02}", at / SECONDS_A_MINUTE, at % SECONDS_A_MINUTE),
        })
        .collect()
}

pub(crate) const fn rgb_of(colour: u32) -> [u8; 3] {
    [
        ((colour >> (2 * CHANNEL_BITS)) & CHANNEL_MASK) as u8,
        ((colour >> CHANNEL_BITS) & CHANNEL_MASK) as u8,
        (colour & CHANNEL_MASK) as u8,
    ]
}

pub(crate) fn ramp_through(stops: &[u32]) -> Ramp {
    let stops: Vec<[u8; 3]> = stops.iter().map(|stop| rgb_of(*stop)).collect();
    Ramp::through(&stops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_colour_splits_into_its_three_channels() {
        assert_eq!(rgb_of(0x12_34_56), [0x12, 0x34, 0x56]);
        assert_eq!(rgb_of(0xff_00_80), [0xff, 0x00, 0x80]);
    }

    #[test]
    fn a_level_sits_between_the_floor_and_the_ceiling() {
        assert_eq!(height_of(SPECTRUM_FLOOR_DB), 0.0);
        assert_eq!(height_of(SPECTRUM_CEILING_DB), 1.0);
        assert_eq!(height_of(-500.0), 0.0);
        assert!((height_of(-65.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn frequencies_are_marked_at_round_steps_below_the_top() {
        let cd = frequency_marks(22_050);
        assert_eq!(
            cd.iter()
                .map(|mark| mark.label.as_str())
                .collect::<Vec<_>>(),
            vec!["5k", "10k", "15k", "20k"]
        );
        assert!((cd[1].share - 10_000.0 / 22_050.0).abs() < 1e-6);

        let hi_res = frequency_marks(96_000);
        assert!(hi_res.len() <= FREQUENCY_MARKS_AT_MOST as usize);
        assert_eq!(hi_res[0].label, "20k");
        assert!(frequency_marks(0).is_empty());
    }

    #[test]
    fn time_is_marked_in_minutes_and_seconds() {
        let marks = time_marks(Duration::from_secs(215));
        assert_eq!(marks[0].label, "0:30");
        assert_eq!(marks.last().map(|mark| mark.label.as_str()), Some("3:30"));
        assert!(marks.iter().all(|mark| mark.share < 1.0));
        assert!(time_marks(Duration::ZERO).is_empty());
        assert!(time_marks(Duration::from_secs(3_600)).len() <= TIME_MARKS_AT_MOST as usize);
    }
}
