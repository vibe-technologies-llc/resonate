use std::time::Duration;

use resonate_codec::Codec;

use crate::{Levels, Spectrum};

pub const JUDGED_UNDER: u32 = 1;

const BAND_HZ: f32 = 100.0;
const WALL_DB: f32 = 30.0;
const WALL_FLOOR_SLACK_DB: f32 = 15.0;
const BELOW_WALL_FROM_HZ: f32 = 1_000.0;
const BELOW_WALL_TO_HZ: f32 = 200.0;
const ABOVE_WALL_FROM_HZ: f32 = 200.0;
const BANDS_ABOVE_A_WALL: usize = 3;
const LOWEST_WALL_HZ: f32 = 4_000.0;
const TOP_GUARD: f32 = 0.995;
const REFERENCE_FROM_HZ: f32 = 1_000.0;
const REFERENCE_TO_HZ: f32 = 4_000.0;
const CONTENT_WITHIN_DB: f32 = 65.0;
const HEARD_AT_LEAST: Duration = Duration::from_secs(5);

const LOSSY_CEILING_HZ: u32 = 19_500;
const SUSPECT_CEILING_HZ: u32 = 20_700;
const HIGHEST_PLAIN_RATE: u32 = 48_000;
const UPSAMPLE_SLACK_HZ: u32 = 300;
const UPSAMPLED_FROM: [u32; 4] = [44_100, 48_000, 88_200, 96_000];
const DC_OFFSET_FROM: f32 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Verdict {
    Genuine,
    Suspect,
    Fake,
    Lossy,
    NotJudged,
}

impl Verdict {
    pub const ALL: [Self; 5] = [
        Self::Genuine,
        Self::Suspect,
        Self::Fake,
        Self::Lossy,
        Self::NotJudged,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Genuine => "genuine",
            Self::Suspect => "suspect",
            Self::Fake => "fake",
            Self::Lossy => "lossy",
            Self::NotJudged => "not-judged",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|verdict| verdict.as_str() == name)
    }

    pub const fn told(self) -> &'static str {
        match self {
            Self::Genuine => "Consistent with lossless",
            Self::Suspect => "Suspect: it may be a high-bitrate lossy encode",
            Self::Fake => "Fake: it is not the lossless master it claims to be",
            Self::Lossy => "Lossy by its codec",
            Self::NotJudged => "Not judged",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LossyGuess {
    UpTo64,
    Near96,
    Near128,
    Near160,
    Near192,
    Near256,
    Near320,
}

impl LossyGuess {
    pub const ALL: [Self; 7] = [
        Self::UpTo64,
        Self::Near96,
        Self::Near128,
        Self::Near160,
        Self::Near192,
        Self::Near256,
        Self::Near320,
    ];

    const CEILINGS_HZ: [(u32, Self); 6] = [
        (11_500, Self::UpTo64),
        (15_500, Self::Near96),
        (17_200, Self::Near128),
        (17_800, Self::Near160),
        (19_000, Self::Near192),
        (19_800, Self::Near256),
    ];

    pub fn of(wall_hz: u32) -> Self {
        Self::CEILINGS_HZ
            .into_iter()
            .find(|(ceiling, _)| wall_hz <= *ceiling)
            .map_or(Self::Near320, |(_, guess)| guess)
    }

    pub const fn kbps(self) -> u16 {
        match self {
            Self::UpTo64 => 64,
            Self::Near96 => 96,
            Self::Near128 => 128,
            Self::Near160 => 160,
            Self::Near192 => 192,
            Self::Near256 => 256,
            Self::Near320 => 320,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UpTo64 => "64",
            Self::Near96 => "96",
            Self::Near128 => "128",
            Self::Near160 => "160",
            Self::Near192 => "192",
            Self::Near256 => "256",
            Self::Near320 => "320",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|guess| guess.as_str() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cutoff {
    pub hz: u32,
    pub drop_db: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Finding {
    Wall {
        cutoff: Cutoff,
        lossy: Option<LossyGuess>,
    },
    Rolloff {
        extent_hz: u32,
    },
    Upsampled {
        from_hz: u32,
    },
    Padded {
        effective: u8,
        declared: u8,
    },
    TooLittleHeard,
    FromDsd,
    MonoAsStereo,
    Clipped {
        samples: u64,
        runs: u64,
    },
    DcOffset {
        level: f32,
    },
}

const HZ_A_KILOHERTZ: f32 = 1_000.0;

fn kilohertz(hz: u32) -> f32 {
    hz as f32 / HZ_A_KILOHERTZ
}

impl Finding {
    pub fn told(&self) -> String {
        match self {
            Self::Wall {
                cutoff,
                lossy: Some(guess),
            } => format!(
                "The spectrum ends in a wall at {:.1} kHz, {:.0} dB deep, where a lossy encode near {} kbps cuts",
                kilohertz(cutoff.hz),
                cutoff.drop_db,
                guess.kbps()
            ),
            Self::Wall {
                cutoff,
                lossy: None,
            } if cutoff.hz <= SUSPECT_CEILING_HZ => format!(
                "The spectrum ends in a wall at {:.1} kHz, {:.0} dB deep, where a high-bitrate lossy encode cuts, or an unusually low anti-alias filter",
                kilohertz(cutoff.hz),
                cutoff.drop_db
            ),
            Self::Wall {
                cutoff,
                lossy: None,
            } => format!(
                "The spectrum ends at {:.1} kHz, {:.0} dB deep, where an anti-alias filter would stand",
                kilohertz(cutoff.hz),
                cutoff.drop_db
            ),
            Self::Rolloff { extent_hz } => format!(
                "The spectrum fades out by {:.1} kHz with no wall to judge by",
                kilohertz(*extent_hz)
            ),
            Self::Upsampled { from_hz } => format!(
                "Nothing stands above {} kHz, where {:.1} kHz audio ends, so it was upsampled from it",
                kilohertz(*from_hz) / 2.0,
                kilohertz(*from_hz)
            ),
            Self::Padded {
                effective,
                declared,
            } => {
                format!("Only {effective} of its {declared} bits carry audio; the rest are padding")
            }
            Self::TooLittleHeard => "Too little of it is loud enough to judge".to_owned(),
            Self::FromDsd => {
                "A DSD stream is not judged by the spectrum its decimation leaves".to_owned()
            }
            Self::MonoAsStereo => "Both channels carry the same samples".to_owned(),
            Self::Clipped { samples, runs } => {
                format!("{samples} samples sit at full scale in {runs} runs")
            }
            Self::DcOffset { level } => format!(
                "The signal sits off centre by {:.1} dBFS",
                20.0 * level.max(f32::MIN_POSITIVE).log10()
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Judgement {
    pub verdict: Verdict,
    pub cutoff: Option<Cutoff>,
    pub extent_hz: Option<u32>,
    pub findings: Vec<Finding>,
}

impl Judgement {
    pub fn lossy_guess(&self) -> Option<LossyGuess> {
        self.findings.iter().find_map(|finding| match finding {
            Finding::Wall { lossy, .. } => *lossy,
            _ => None,
        })
    }

    pub fn upsampled_from(&self) -> Option<u32> {
        self.findings.iter().find_map(|finding| match finding {
            Finding::Upsampled { from_hz } => Some(*from_hz),
            _ => None,
        })
    }

    pub fn padded(&self) -> Option<(u8, u8)> {
        self.findings.iter().find_map(|finding| match finding {
            Finding::Padded {
                effective,
                declared,
            } => Some((*effective, *declared)),
            _ => None,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Weighed<'a> {
    pub(crate) codec: Codec,
    pub(crate) rate: u32,
    pub(crate) declared_bits: Option<u8>,
    pub(crate) float: bool,
    pub(crate) spectrum: &'a Spectrum,
    pub(crate) levels: &'a Levels,
}

pub(crate) fn judged(weighed: Weighed<'_>) -> Judgement {
    let spectrum = weighed.spectrum;
    let bands = banded(spectrum);
    let cutoff = wall_in(&bands);
    let extent_hz = extent_of(spectrum, &bands);
    let mut findings = Vec::new();

    let lossy_wall = cutoff
        .filter(|cutoff| cutoff.hz <= LOSSY_CEILING_HZ)
        .map(|cutoff| LossyGuess::of(cutoff.hz));
    match cutoff {
        Some(cutoff) => findings.push(Finding::Wall {
            cutoff,
            lossy: lossy_wall,
        }),
        None => findings.extend(extent_hz.map(|extent_hz| Finding::Rolloff { extent_hz })),
    }

    let upsampled = cutoff.and_then(|cutoff| upsampled_from(cutoff.hz, weighed.rate));
    findings.extend(upsampled.map(|from_hz| Finding::Upsampled { from_hz }));

    let padded = padding(weighed);
    findings.extend(padded);
    findings.extend(incidentals(weighed.levels));

    let heard_enough = spectrum.heard() >= HEARD_AT_LEAST;
    if !heard_enough {
        findings.push(Finding::TooLittleHeard);
    }
    let from_dsd = weighed.codec == Codec::Dsd;
    if from_dsd {
        findings.push(Finding::FromDsd);
    }

    let verdict = if from_dsd || weighed.codec == Codec::Unknown {
        Verdict::NotJudged
    } else if !weighed.codec.is_lossless() {
        Verdict::Lossy
    } else if padded.is_some() || (heard_enough && (lossy_wall.is_some() || upsampled.is_some())) {
        Verdict::Fake
    } else if !heard_enough {
        Verdict::NotJudged
    } else {
        spectral_verdict(weighed.rate, cutoff, extent_hz)
    };

    Judgement {
        verdict,
        cutoff,
        extent_hz,
        findings,
    }
}

fn spectral_verdict(rate: u32, cutoff: Option<Cutoff>, extent_hz: Option<u32>) -> Verdict {
    let genuine_from = sources_of(rate)
        .map(|from| from / 2 + UPSAMPLE_SLACK_HZ)
        .max()
        .unwrap_or(SUSPECT_CEILING_HZ)
        .max(SUSPECT_CEILING_HZ);
    match (cutoff, extent_hz) {
        (Some(cutoff), _) if cutoff.hz <= SUSPECT_CEILING_HZ => Verdict::Suspect,
        (Some(cutoff), _) if cutoff.hz > genuine_from => Verdict::Genuine,
        (None, Some(extent)) if extent > genuine_from => Verdict::Genuine,
        _ => Verdict::NotJudged,
    }
}

fn banded(spectrum: &Spectrum) -> Vec<f32> {
    let top = spectrum.nyquist_hz() * TOP_GUARD;
    let count = (top / BAND_HZ).floor() as usize;
    (0..count)
        .map(|band| {
            let from = band as f32 * BAND_HZ;
            spectrum
                .band_db(from, from + BAND_HZ)
                .unwrap_or(crate::spectrum::FLOOR_DB)
        })
        .collect()
}

fn mean(levels: &[f32]) -> Option<f32> {
    (!levels.is_empty()).then(|| levels.iter().sum::<f32>() / levels.len() as f32)
}

fn drop_at(bands: &[f32], edge: usize) -> Option<f32> {
    let below_from = edge.checked_sub((BELOW_WALL_FROM_HZ / BAND_HZ) as usize)?;
    let below_to = edge.checked_sub((BELOW_WALL_TO_HZ / BAND_HZ) as usize)?;
    let above_from = edge + (ABOVE_WALL_FROM_HZ / BAND_HZ) as usize;
    let above = bands.get(above_from..)?;
    if above.len() < BANDS_ABOVE_A_WALL {
        return None;
    }

    let below = mean(&bands[below_from..below_to])?;
    let above_mean = mean(above)?;
    let above_most = above.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let drop = below - above_mean;
    (drop >= WALL_DB && below - above_most >= WALL_DB - WALL_FLOOR_SLACK_DB).then_some(drop)
}

fn wall_in(bands: &[f32]) -> Option<Cutoff> {
    let lowest = (LOWEST_WALL_HZ / BAND_HZ) as usize;
    let mut steepest: Option<(usize, f32)> = None;
    for edge in (lowest..bands.len()).rev() {
        match (drop_at(bands, edge), steepest) {
            (Some(drop), Some((_, held))) if drop > held => steepest = Some((edge, drop)),
            (Some(drop), None) => steepest = Some((edge, drop)),
            (Some(_), Some(_)) => {}
            (None, Some(_)) => break,
            (None, None) => {}
        }
    }

    steepest.map(|(edge, drop_db)| Cutoff {
        hz: (edge as f32 * BAND_HZ) as u32,
        drop_db,
    })
}

fn extent_of(spectrum: &Spectrum, bands: &[f32]) -> Option<u32> {
    let reference = spectrum.band_db(REFERENCE_FROM_HZ, REFERENCE_TO_HZ)?;
    let threshold = reference - CONTENT_WITHIN_DB;
    bands
        .iter()
        .rposition(|level| *level >= threshold)
        .map(|band| ((band + 1) as f32 * BAND_HZ) as u32)
}

fn sources_of(rate: u32) -> impl Iterator<Item = u32> {
    UPSAMPLED_FROM
        .into_iter()
        .filter(move |from| rate > HIGHEST_PLAIN_RATE && *from <= rate / 2)
}

fn upsampled_from(wall_hz: u32, rate: u32) -> Option<u32> {
    sources_of(rate).find(|from| wall_hz <= from / 2 + UPSAMPLE_SLACK_HZ)
}

fn padding(weighed: Weighed<'_>) -> Option<Finding> {
    let effective = weighed.levels.bits_in_use?;
    let declared = if weighed.float {
        weighed.declared_bits.unwrap_or(32).max(32)
    } else {
        weighed.declared_bits?
    };
    (weighed.codec.is_lossless() && weighed.codec != Codec::Dsd && effective < declared).then_some(
        Finding::Padded {
            effective,
            declared,
        },
    )
}

fn incidentals(levels: &Levels) -> impl Iterator<Item = Finding> {
    let mono = levels
        .stereo
        .is_some_and(|stereo| stereo.identical)
        .then_some(Finding::MonoAsStereo);
    let clipped = (levels.clipped_runs > 0).then_some(Finding::Clipped {
        samples: levels.clipped,
        runs: levels.clipped_runs,
    });
    let dc = (levels.dc >= DC_OFFSET_FROM).then_some(Finding::DcOffset { level: levels.dc });
    mono.into_iter().chain(clipped).chain(dc)
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Levels = Levels {
        peak: 0.5,
        rms: 0.1,
        dc: 0.0,
        clipped: 0,
        clipped_runs: 0,
        bits_in_use: None,
        stereo: None,
    };

    fn spectrum_of(rate: u32, level_at: impl Fn(f32) -> f32) -> Spectrum {
        let points = crate::spectrum::points_for(rate);
        let bin_hz = rate as f32 / points as f32;
        let levels = (0..=points / 2)
            .map(|bin| level_at(bin as f32 * bin_hz))
            .collect();
        Spectrum::new(bin_hz, levels, Duration::from_secs(180))
    }

    fn music_up_to(wall_hz: f32) -> impl Fn(f32) -> f32 {
        move |hz| {
            if hz <= wall_hz {
                -30.0 - 20.0 * (hz.max(100.0) / 1_000.0).log10()
            } else {
                -135.0
            }
        }
    }

    fn judged_as(codec: Codec, rate: u32, spectrum: &Spectrum, levels: &Levels) -> Judgement {
        judged(Weighed {
            codec,
            rate,
            declared_bits: Some(16),
            float: false,
            spectrum,
            levels,
        })
    }

    #[test]
    fn a_wall_at_sixteen_kilohertz_in_a_flac_is_a_128_kbps_transcode() {
        let spectrum = spectrum_of(44_100, music_up_to(16_000.0));
        let judgement = judged_as(Codec::Flac, 44_100, &spectrum, &QUIET);

        assert_eq!(judgement.verdict, Verdict::Fake);
        let cutoff = judgement.cutoff.expect("a wall");
        assert!((15_800..=16_200).contains(&cutoff.hz), "{}", cutoff.hz);
        assert!(cutoff.drop_db >= WALL_DB);
        assert_eq!(judgement.lossy_guess(), Some(LossyGuess::Near128));
    }

    #[test]
    fn a_wall_at_twenty_kilohertz_is_only_suspect_and_one_at_the_top_is_genuine() {
        let suspect = spectrum_of(44_100, music_up_to(20_300.0));
        assert_eq!(
            judged_as(Codec::Flac, 44_100, &suspect, &QUIET).verdict,
            Verdict::Suspect
        );

        let genuine = spectrum_of(44_100, music_up_to(21_600.0));
        let judgement = judged_as(Codec::Flac, 44_100, &genuine, &QUIET);
        assert_eq!(judgement.verdict, Verdict::Genuine);
        assert_eq!(judgement.lossy_guess(), None);
    }

    #[test]
    fn music_reaching_the_top_with_no_wall_is_genuine() {
        let spectrum = spectrum_of(44_100, |hz| {
            -30.0 - 20.0 * (hz.max(100.0) / 1_000.0).log10()
        });
        let judgement = judged_as(Codec::Flac, 44_100, &spectrum, &QUIET);
        assert_eq!(judgement.cutoff, None);
        assert_eq!(judgement.verdict, Verdict::Genuine);
    }

    #[test]
    fn a_gentle_rolloff_that_dies_early_cannot_be_judged() {
        let spectrum = spectrum_of(44_100, |hz| -20.0 - hz / 150.0);
        let judgement = judged_as(Codec::Flac, 44_100, &spectrum, &QUIET);
        assert_eq!(judgement.cutoff, None);
        assert_eq!(judgement.verdict, Verdict::NotJudged);
        assert!(matches!(
            judgement.findings.first(),
            Some(Finding::Rolloff { .. })
        ));
    }

    #[test]
    fn a_hi_res_stream_walled_where_cd_audio_ends_was_upsampled() {
        let spectrum = spectrum_of(96_000, music_up_to(21_800.0));
        let judgement = judged_as(Codec::Flac, 96_000, &spectrum, &QUIET);
        assert_eq!(judgement.verdict, Verdict::Fake);
        assert_eq!(judgement.upsampled_from(), Some(44_100));

        let from_48 = spectrum_of(192_000, music_up_to(23_900.0));
        assert_eq!(
            judged_as(Codec::Flac, 192_000, &from_48, &QUIET).upsampled_from(),
            Some(48_000)
        );

        let genuine = spectrum_of(96_000, music_up_to(40_000.0));
        let judgement = judged_as(Codec::Flac, 96_000, &genuine, &QUIET);
        assert_eq!(judgement.verdict, Verdict::Genuine);
        assert_eq!(judgement.upsampled_from(), None);
    }

    #[test]
    fn a_lossy_codec_is_lossy_whatever_its_spectrum_says() {
        let spectrum = spectrum_of(44_100, music_up_to(16_000.0));
        let judgement = judged_as(Codec::Mp3, 44_100, &spectrum, &QUIET);
        assert_eq!(judgement.verdict, Verdict::Lossy);
        assert_eq!(judgement.lossy_guess(), Some(LossyGuess::Near128));
    }

    #[test]
    fn sixteen_bits_in_a_twenty_four_bit_stream_are_padding() {
        let spectrum = spectrum_of(96_000, music_up_to(40_000.0));
        let levels = Levels {
            bits_in_use: Some(16),
            ..QUIET
        };
        let judgement = judged(Weighed {
            codec: Codec::Flac,
            rate: 96_000,
            declared_bits: Some(24),
            float: false,
            spectrum: &spectrum,
            levels: &levels,
        });
        assert_eq!(judgement.verdict, Verdict::Fake);
        assert_eq!(judgement.padded(), Some((16, 24)));
    }

    #[test]
    fn a_few_seconds_are_not_enough_to_judge_a_spectrum() {
        let points = crate::spectrum::points_for(44_100);
        let bin_hz = 44_100.0 / points as f32;
        let levels = (0..=points / 2)
            .map(|bin| music_up_to(16_000.0)(bin as f32 * bin_hz))
            .collect();
        let spectrum = Spectrum::new(bin_hz, levels, Duration::from_secs(2));
        let judgement = judged_as(Codec::Flac, 44_100, &spectrum, &QUIET);
        assert_eq!(judgement.verdict, Verdict::NotJudged);
        assert!(judgement.findings.contains(&Finding::TooLittleHeard));
    }

    #[test]
    fn a_dsd_stream_is_never_judged_by_its_decimated_spectrum() {
        let spectrum = spectrum_of(176_400, music_up_to(45_000.0));
        let judgement = judged_as(Codec::Dsd, 176_400, &spectrum, &QUIET);
        assert_eq!(judgement.verdict, Verdict::NotJudged);
        assert!(judgement.findings.contains(&Finding::FromDsd));
    }

    #[test]
    fn the_bitrate_is_guessed_from_where_the_wall_stands() {
        assert_eq!(LossyGuess::of(11_000), LossyGuess::UpTo64);
        assert_eq!(LossyGuess::of(16_000), LossyGuess::Near128);
        assert_eq!(LossyGuess::of(18_800), LossyGuess::Near192);
        assert_eq!(LossyGuess::of(19_600), LossyGuess::Near256);
        assert_eq!(LossyGuess::of(20_400), LossyGuess::Near320);
        for guess in LossyGuess::ALL {
            assert_eq!(LossyGuess::named(guess.as_str()), Some(guess));
        }
        for verdict in Verdict::ALL {
            assert_eq!(Verdict::named(verdict.as_str()), Some(verdict));
        }
    }

    #[test]
    fn every_finding_and_verdict_says_something() {
        let cutoff = Cutoff {
            hz: 16_100,
            drop_db: 60.0,
        };
        let findings = [
            Finding::Wall {
                cutoff,
                lossy: Some(LossyGuess::Near128),
            },
            Finding::Wall {
                cutoff,
                lossy: None,
            },
            Finding::Rolloff { extent_hz: 15_000 },
            Finding::Upsampled { from_hz: 44_100 },
            Finding::Padded {
                effective: 16,
                declared: 24,
            },
            Finding::TooLittleHeard,
            Finding::FromDsd,
            Finding::MonoAsStereo,
            Finding::Clipped {
                samples: 12,
                runs: 3,
            },
            Finding::DcOffset { level: 0.1 },
        ];
        assert!(findings.iter().all(|finding| !finding.told().is_empty()));
        assert_eq!(
            Finding::Upsampled { from_hz: 44_100 }.told(),
            "Nothing stands above 22.05 kHz, where 44.1 kHz audio ends, so it was upsampled from it"
        );
        assert!(findings[0].told().contains("16.1 kHz"));
        assert!(findings[0].told().contains("128 kbps"));
        assert!(findings[3].told().contains("44.1 kHz"));
        assert!(
            Verdict::ALL
                .iter()
                .all(|verdict| !verdict.told().is_empty())
        );
    }

    #[test]
    fn identical_channels_clipping_and_an_offset_are_noted() {
        let spectrum = spectrum_of(44_100, music_up_to(21_600.0));
        let levels = Levels {
            clipped: 12,
            clipped_runs: 3,
            dc: 0.05,
            stereo: Some(crate::Stereo {
                identical: true,
                correlation: Some(1.0),
            }),
            ..QUIET
        };
        let judgement = judged_as(Codec::Flac, 44_100, &spectrum, &levels);
        assert_eq!(judgement.verdict, Verdict::Genuine);
        assert!(judgement.findings.contains(&Finding::MonoAsStereo));
        assert!(judgement.findings.contains(&Finding::Clipped {
            samples: 12,
            runs: 3
        }));
        assert!(
            judgement
                .findings
                .contains(&Finding::DcOffset { level: 0.05 })
        );
    }
}
