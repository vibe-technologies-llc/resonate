use std::io::Cursor;

use image::{
    ExtendedColorType, ImageEncoder,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use resonate_codec::{CoverArt, ImageFormat};
use resonate_core::Frames;

use crate::{
    Error, Result,
    reduce::{Absorbs, Doubling},
};

pub const SPECTROGRAM_COLUMNS: usize = 1_024;

pub const SPECTROGRAM_ROWS: usize = 256;

pub const SPECTROGRAM_FLOOR_DB: f32 = -120.0;

const LEVELS: usize = 256;

#[derive(Clone, Debug)]
struct Slice {
    power: [f32; SPECTROGRAM_ROWS],
    transforms: u32,
}

impl Absorbs for Slice {
    fn absorb(&mut self, later: &Self) {
        for (held, arrived) in self.power.iter_mut().zip(&later.power) {
            *held += arrived;
        }
        self.transforms += later.transforms;
    }
}

pub(crate) struct Spectrographing {
    columns: Doubling<Slice>,
    points: usize,
    rate: u32,
    frames: u64,
}

impl Spectrographing {
    pub(crate) fn new(points: usize, rate: u32) -> Self {
        Self {
            columns: Doubling::new(SPECTROGRAM_COLUMNS),
            points,
            rate,
            frames: 0,
        }
    }

    pub(crate) fn note(&mut self, powers: &[f32]) {
        let bins = powers.len();
        let power = std::array::from_fn(|row| {
            let first = row * bins / SPECTROGRAM_ROWS;
            let past = ((row + 1) * bins / SPECTROGRAM_ROWS)
                .max(first + 1)
                .min(bins);
            powers[first..past].iter().copied().fold(0.0, f32::max)
        });
        self.columns.push(Slice {
            power,
            transforms: 1,
        });
        self.frames += self.points as u64;
    }

    pub(crate) fn finished(self) -> Spectrogram {
        let frames_per_column = self.columns.per_column() * self.points as u64;
        let slices = self.columns.finished();
        let levels = slices
            .iter()
            .flat_map(|slice| {
                let transforms = slice.transforms.max(1) as f32;
                slice
                    .power
                    .iter()
                    .map(move |power| quantised(power / transforms))
            })
            .collect();

        Spectrogram {
            columns: slices.len(),
            levels,
            nyquist_hz: self.rate / 2,
            frames_per_column,
            frames: self.frames,
        }
    }
}

fn quantised(power: f32) -> u8 {
    let db = if power > 0.0 {
        10.0 * power.log10()
    } else {
        SPECTROGRAM_FLOOR_DB
    };
    let share = (db - SPECTROGRAM_FLOOR_DB) / -SPECTROGRAM_FLOOR_DB;
    (share * (LEVELS - 1) as f32)
        .round()
        .clamp(0.0, (LEVELS - 1) as f32) as u8
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ramp {
    colours: Vec<[u8; 3]>,
}

impl Ramp {
    pub fn through(stops: &[[u8; 3]]) -> Self {
        let colours = (0..LEVELS)
            .map(|level| match stops {
                [] => [0, 0, 0],
                [only] => *only,
                _ => {
                    let at = level as f32 / (LEVELS - 1) as f32 * (stops.len() - 1) as f32;
                    let lower = (at.floor() as usize).min(stops.len() - 2);
                    let share = at - lower as f32;
                    let (from, to) = (stops[lower], stops[lower + 1]);
                    [0, 1, 2].map(|channel| {
                        let from = f32::from(from[channel]);
                        let to = f32::from(to[channel]);
                        (from + (to - from) * share).round() as u8
                    })
                }
            })
            .collect();
        Self { colours }
    }

    fn colour(&self, level: u8) -> [u8; 3] {
        self.colours
            .get(usize::from(level))
            .copied()
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spectrogram {
    columns: usize,
    levels: Vec<u8>,
    nyquist_hz: u32,
    frames_per_column: u64,
    frames: u64,
}

impl Spectrogram {
    pub const fn columns(&self) -> usize {
        self.columns
    }

    pub const fn rows(&self) -> usize {
        SPECTROGRAM_ROWS
    }

    pub const fn nyquist_hz(&self) -> u32 {
        self.nyquist_hz
    }

    pub const fn frames(&self) -> Frames {
        Frames(self.frames)
    }

    pub const fn frames_per_column(&self) -> Frames {
        Frames(self.frames_per_column)
    }

    pub fn level(&self, column: usize, row: usize) -> Option<u8> {
        (row < SPECTROGRAM_ROWS)
            .then(|| self.levels.get(column * SPECTROGRAM_ROWS + row).copied())
            .flatten()
    }

    pub fn painted(&self, ramp: &Ramp) -> Result<CoverArt> {
        let width = self.columns.max(1);
        let mut pixels = Vec::with_capacity(width * SPECTROGRAM_ROWS * 3);
        for row in (0..SPECTROGRAM_ROWS).rev() {
            for column in 0..width {
                pixels.extend(ramp.colour(self.level(column, row).unwrap_or(0)));
            }
        }

        let mut bytes = Vec::new();
        PngEncoder::new_with_quality(
            Cursor::new(&mut bytes),
            CompressionType::Fast,
            FilterType::Adaptive,
        )
        .write_image(
            &pixels,
            width as u32,
            SPECTROGRAM_ROWS as u32,
            ExtendedColorType::Rgb8,
        )
        .map_err(|source| Error::Painted {
            source: Box::new(source),
        })?;

        Ok(CoverArt {
            format: ImageFormat::Png,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ramp_runs_through_its_stops_evenly() {
        let ramp = Ramp::through(&[[0, 0, 0], [255, 0, 0], [255, 255, 255]]);
        assert_eq!(ramp.colour(0), [0, 0, 0]);
        assert_eq!(ramp.colour(255), [255, 255, 255]);
        let middle = ramp.colour(128);
        assert!(middle[0] >= 254 && middle[1] <= 2);

        assert_eq!(Ramp::through(&[]).colour(200), [0, 0, 0]);
        assert_eq!(Ramp::through(&[[9, 8, 7]]).colour(3), [9, 8, 7]);
    }

    #[test]
    fn loud_bins_quantise_high_and_silence_to_the_floor() {
        assert_eq!(quantised(1.0), 255);
        assert_eq!(quantised(0.0), 0);
        assert_eq!(quantised(1e-13), 0);
        assert!((120..=135).contains(&quantised(1e-6)));
    }

    #[test]
    fn a_spectrogram_paints_one_pixel_per_column_and_row() {
        let mut graphing = Spectrographing::new(4_096, 48_000);
        for nth in 0..10 {
            let mut powers = vec![0.0; 2_049];
            powers[nth * 100] = 1.0;
            graphing.note(&powers);
        }
        let spectrogram = graphing.finished();
        assert_eq!(spectrogram.columns(), 10);
        assert_eq!(spectrogram.nyquist_hz(), 24_000);
        assert_eq!(spectrogram.frames(), Frames(40_960));
        assert_eq!(
            spectrogram.level(3, 300 * SPECTROGRAM_ROWS / 2_049),
            Some(255)
        );
        assert_eq!(spectrogram.level(3, 200), Some(0));
        assert_eq!(spectrogram.level(3, SPECTROGRAM_ROWS), None);

        let painted = spectrogram
            .painted(&Ramp::through(&[[0, 0, 0], [255, 255, 255]]))
            .expect("a picture");
        assert_eq!(painted.format, ImageFormat::Png);
        let read = image::load_from_memory(&painted.bytes).expect("a readable picture");
        assert_eq!((read.width(), read.height()), (10, SPECTROGRAM_ROWS as u32));
    }
}
