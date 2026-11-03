use crate::{
    contender::{Contender, Pair},
    error::Result,
    measure::{
        self, AUDIBLE_HZ, Impulse, LOWEST_HZ, MIDDLE_HZ, Tone, impulse, latency, null, phase_delays,
    },
};

const NULL_NARROW_HZ: f64 = 10_000.0;
const RIPPLE_UNTIL_HZ: f64 = 10_000.0;
const EDGE_SEARCH_FROM_HZ: f64 = 1_000.0;
const SLIGHT_DROP_DB: f64 = -0.1;
const HALF_POWER_DB: f64 = -3.0;

pub struct Measured {
    pub contender: Contender,
    pub pair: Pair,
    pub tones: Vec<Tone>,
    pub summary: Summary,
}

pub struct Summary {
    pub ripple_db: f64,
    pub deviation_db: f64,
    pub at_audible_db: f64,
    pub slight_drop_hz: Option<f64>,
    pub half_power_hz: Option<f64>,
    pub worst_rejection_db: f64,
    pub worst_audible_rejection_db: f64,
    pub snr_middle_db: f64,
    pub worst_snr_db: f64,
    pub null_wide_db: f64,
    pub null_narrow_db: f64,
    pub delay_middle_s: f64,
    pub delay_spread_s: f64,
    pub impulse: Impulse,
    pub latency_s: f64,
}

fn heard_within(tones: &[Tone]) -> impl Iterator<Item = (f64, measure::Heard)> + '_ {
    tones
        .iter()
        .filter(|tone| (LOWEST_HZ..=AUDIBLE_HZ).contains(&tone.hz))
        .filter_map(|tone| tone.heard.map(|heard| (tone.hz, heard)))
}

fn crossing(tones: &[Tone], level_db: f64) -> Option<f64> {
    let heard: Vec<(f64, f64)> = tones
        .iter()
        .filter_map(|tone| tone.heard.map(|heard| (tone.hz, heard.gain_db)))
        .filter(|&(hz, _)| hz >= EDGE_SEARCH_FROM_HZ)
        .collect();
    heard.windows(2).find_map(|pair| {
        let [(low_hz, low_db), (high_hz, high_db)] = [pair[0], pair[1]];
        (low_db >= level_db && high_db < level_db)
            .then(|| low_hz + (level_db - low_db) / (high_db - low_db) * (high_hz - low_hz))
    })
}

fn rejections(tones: &[Tone]) -> impl Iterator<Item = (f64, f64)> + '_ {
    tones.iter().filter_map(|tone| {
        tone.leak
            .map(|leak| (leak.lands_hz, leak.level_db))
            .or_else(|| tone.image.map(|image| (image.lands_hz, image.level_db)))
    })
}

fn loudest(levels: impl Iterator<Item = f64>) -> f64 {
    levels.fold(f64::NEG_INFINITY, f64::max)
}

pub fn measured(contender: Contender, pair: Pair) -> Result<Measured> {
    let tones = measure::sweep(pair)
        .into_iter()
        .map(|hz| measure::tone(contender, pair, hz))
        .collect::<Result<Vec<_>>>()?;
    let impulse = impulse(contender, pair)?;
    let delays = phase_delays(&tones, impulse.peak_offset_s);
    let delay_middle_s = delays
        .iter()
        .find(|&&(hz, _)| hz == MIDDLE_HZ)
        .map_or(impulse.peak_offset_s, |&(_, delay)| delay);

    let gains: Vec<f64> = heard_within(&tones)
        .map(|(_, heard)| heard.gain_db)
        .collect();
    let flat: Vec<f64> = heard_within(&tones)
        .filter(|&(hz, _)| hz <= RIPPLE_UNTIL_HZ)
        .map(|(_, heard)| heard.gain_db)
        .collect();
    let snrs: Vec<f64> = heard_within(&tones)
        .map(|(_, heard)| -heard.spurious_db)
        .collect();
    let snr_middle_db = heard_within(&tones)
        .find(|&(hz, _)| hz == MIDDLE_HZ)
        .map_or(f64::NAN, |(_, heard)| -heard.spurious_db);
    let at_audible_db = heard_within(&tones)
        .find(|&(hz, _)| hz == AUDIBLE_HZ)
        .map_or(f64::NAN, |(_, heard)| heard.gain_db);

    let summary = Summary {
        ripple_db: loudest(flat.iter().copied()) + loudest(flat.iter().map(|gain| -gain)),
        deviation_db: loudest(gains.iter().copied()) + loudest(gains.iter().map(|gain| -gain)),
        at_audible_db,
        slight_drop_hz: crossing(&tones, SLIGHT_DROP_DB),
        half_power_hz: crossing(&tones, HALF_POWER_DB),
        worst_rejection_db: loudest(rejections(&tones).map(|(_, level)| level)),
        worst_audible_rejection_db: loudest(
            rejections(&tones)
                .filter(|&(lands, _)| lands <= AUDIBLE_HZ)
                .map(|(_, level)| level),
        ),
        snr_middle_db,
        worst_snr_db: -loudest(snrs.iter().map(|snr| -snr)),
        null_wide_db: null(contender, pair, AUDIBLE_HZ, delay_middle_s)?,
        null_narrow_db: null(contender, pair, NULL_NARROW_HZ, delay_middle_s)?,
        delay_middle_s,
        delay_spread_s: loudest(delays.iter().map(|&(_, delay)| delay))
            + loudest(delays.iter().map(|&(_, delay)| -delay)),
        impulse,
        latency_s: latency(contender, pair)?,
    };

    Ok(Measured {
        contender,
        pair,
        tones,
        summary,
    })
}
