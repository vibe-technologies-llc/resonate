use std::{io::Cursor, num::NonZeroU32, sync::LazyLock};

use image::{DynamicImage, RgbImage, RgbaImage};

use crate::probe::{CoverArt, ImageFormat};

const CATMULL_ROM_SUPPORT: f32 = 2.0;
const CATMULL_ROM_B: f32 = 0.0;
const CATMULL_ROM_C: f32 = 0.5;

const SRGB_ENCODED_KNEE: f32 = 0.040_45;
const SRGB_LINEAR_KNEE: f32 = 0.003_130_8;
const SRGB_SLOPE: f32 = 12.92;
const SRGB_OFFSET: f32 = 0.055;
const SRGB_EXPONENT: f32 = 2.4;

const EIGHT_BIT_FULL_SCALE: f32 = 255.0;

const OPAQUE: f32 = 1.0;

const RGB: usize = 3;
const RGBA: usize = 4;

static LINEAR_FROM_EIGHT_BIT: LazyLock<[f32; 256]> = LazyLock::new(|| {
    std::array::from_fn(|encoded| linear_from_srgb(encoded as f32 / EIGHT_BIT_FULL_SCALE))
});

type Linear = [f32; RGBA];

pub struct Drawing(Decoded);

enum Decoded {
    Rgb(RgbImage),
    Rgba(RgbaImage),
}

impl Drawing {
    pub fn of(art: &CoverArt) -> Option<Self> {
        read_art(art).map(|read| match read {
            DynamicImage::ImageRgb8(rgb) => Self(Decoded::Rgb(rgb)),
            other => Self(Decoded::Rgba(other.into_rgba8())),
        })
    }

    pub fn no_larger_than(&self, side: NonZeroU32) -> Option<CoverArt> {
        let plane = self.plane();
        let (width, height) = sides_within(plane.width, plane.height, side.get())?;

        written_as_png(&drawn_smaller(plane, width, height))
    }

    pub fn squared(&self, side: NonZeroU32) -> Option<CoverArt> {
        let plane = self.plane();
        let square = plane.width.min(plane.height);
        if square == 0 {
            return None;
        }
        let cropped = plane.cropped(
            (plane.width - square) / 2,
            (plane.height - square) / 2,
            square,
        );
        let drawn = if square > side.get() {
            drawn_smaller(cropped, side.get(), side.get())
        } else {
            cropped.to_rgba()
        };

        written_as_png(&drawn)
    }

    fn plane(&self) -> Plane<'_> {
        match &self.0 {
            Decoded::Rgb(rgb) => Plane::whole(rgb.as_raw(), RGB, rgb.width(), rgb.height()),
            Decoded::Rgba(rgba) => Plane::whole(rgba.as_raw(), RGBA, rgba.width(), rgba.height()),
        }
    }
}

#[derive(Clone, Copy)]
struct Plane<'a> {
    samples: &'a [u8],
    channels: usize,
    stride: usize,
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

impl<'a> Plane<'a> {
    const fn whole(samples: &'a [u8], channels: usize, width: u32, height: u32) -> Self {
        Self {
            samples,
            channels,
            stride: width as usize * channels,
            left: 0,
            top: 0,
            width,
            height,
        }
    }

    const fn cropped(self, left: u32, top: u32, side: u32) -> Self {
        Self {
            left: self.left + left,
            top: self.top + top,
            width: side,
            height: side,
            ..self
        }
    }

    fn row(&self, y: u32) -> impl Iterator<Item = &'a [u8]> {
        let start = (self.top + y) as usize * self.stride + self.left as usize * self.channels;
        let end = start + self.width as usize * self.channels;
        self.samples[start..end].chunks_exact(self.channels)
    }

    fn linear(&self, y: u32, into: &mut [Linear]) {
        let linear = &*LINEAR_FROM_EIGHT_BIT;
        for (held, pixel) in into.iter_mut().zip(self.row(y)) {
            let alpha = match pixel.get(RGB) {
                Some(&alpha) => f32::from(alpha) / EIGHT_BIT_FULL_SCALE,
                None => OPAQUE,
            };
            *held = [
                linear[usize::from(pixel[0])],
                linear[usize::from(pixel[1])],
                linear[usize::from(pixel[2])],
                alpha,
            ];
        }
    }

    fn to_rgba(self) -> RgbaImage {
        let mut rgba = RgbaImage::new(self.width, self.height);
        for (y, into) in (0..self.height).zip(rgba.rows_mut()) {
            for (drawn, pixel) in into.zip(self.row(y)) {
                drawn.0[..RGB].copy_from_slice(&pixel[..RGB]);
                drawn.0[RGB] = pixel.get(RGB).copied().unwrap_or(u8::MAX);
            }
        }
        rgba
    }
}

struct Taps {
    first: u32,
    weights: Vec<f32>,
}

fn taps(from: u32, to: u32) -> Vec<Taps> {
    let ratio = from as f32 / to as f32;
    let widened = if ratio < 1.0 { 1.0 } else { ratio };
    let reach = CATMULL_ROM_SUPPORT * widened;

    (0..to)
        .map(|out| {
            let centre = (out as f32 + 0.5) * ratio;
            let first = ((centre - reach).floor() as i64).clamp(0, i64::from(from) - 1) as u32;
            let last = ((centre + reach).ceil() as i64).clamp(i64::from(first) + 1, i64::from(from))
                as u32;
            let centre = centre - 0.5;

            let mut weights: Vec<f32> = (first..last)
                .map(|at| catmull_rom((at as f32 - centre) / widened))
                .collect();
            let sum: f32 = weights.iter().fold(0.0, |sum, weight| sum + weight);
            for weight in &mut weights {
                *weight /= sum;
            }
            Taps { first, weights }
        })
        .collect()
}

fn catmull_rom(x: f32) -> f32 {
    let (b, c) = (CATMULL_ROM_B, CATMULL_ROM_C);
    let a = x.abs();

    let k = if a < 1.0 {
        (12.0 - 9.0 * b - 6.0 * c) * a.powi(3)
            + (-18.0 + 12.0 * b + 6.0 * c) * a.powi(2)
            + (6.0 - 2.0 * b)
    } else if a < 2.0 {
        (-b - 6.0 * c) * a.powi(3)
            + (6.0 * b + 30.0 * c) * a.powi(2)
            + (-12.0 * b - 48.0 * c) * a
            + (8.0 * b + 24.0 * c)
    } else {
        0.0
    };

    k / 6.0
}

struct Held {
    rows: Vec<Vec<Linear>>,
    reached: u32,
}

impl Held {
    fn over(plane: &Plane<'_>, down: &[Taps]) -> Self {
        let deepest = down.iter().map(|row| row.weights.len()).max().unwrap_or(0);
        Self {
            rows: vec![vec![[0.0; RGBA]; plane.width as usize]; deepest],
            reached: 0,
        }
    }

    fn row(&mut self, plane: &Plane<'_>, y: u32) -> &[Linear] {
        let slot = y as usize % self.rows.len();
        if y >= self.reached {
            plane.linear(y, &mut self.rows[slot]);
            self.reached = y + 1;
        }
        &self.rows[slot]
    }
}

fn drawn_smaller(plane: Plane<'_>, width: u32, height: u32) -> RgbaImage {
    let across = taps(plane.width, width);
    let down = taps(plane.height, height);
    let mut held = Held::over(&plane, &down);
    let mut column = vec![[0.0; RGBA]; plane.width as usize];
    let mut drawn = RgbaImage::new(width, height);

    for (row, into) in down.iter().zip(drawn.rows_mut()) {
        column.fill([0.0; RGBA]);
        for (y, &weight) in (row.first..).zip(&row.weights) {
            for (sum, value) in column.iter_mut().zip(held.row(&plane, y)) {
                for (channel, value) in sum.iter_mut().zip(value) {
                    *channel += value * weight;
                }
            }
        }
        for (tap, pixel) in across.iter().zip(into) {
            let mut sum = [0.0; RGBA];
            for (value, &weight) in column[tap.first as usize..].iter().zip(&tap.weights) {
                for (channel, value) in sum.iter_mut().zip(value) {
                    *channel += value * weight;
                }
            }
            for (drawn, &value) in pixel.0.iter_mut().zip(&sum[..RGB]) {
                *drawn = eight_bit(srgb_from_linear(value.clamp(0.0, 1.0)));
            }
            pixel.0[RGB] = eight_bit(sum[RGB].clamp(0.0, 1.0));
        }
    }
    drawn
}

fn read_art(art: &CoverArt) -> Option<DynamicImage> {
    match image::load_from_memory_with_format(&art.bytes, read_as(art.format)) {
        Ok(read) => Some(read),
        Err(error) => {
            tracing::warn!(%error, "cover art could not be read for drawing");
            None
        }
    }
}

const fn sides_within(width: u32, height: u32, side: u32) -> Option<(u32, u32)> {
    let longest = if width > height { width } else { height };
    if longest <= side || width == 0 || height == 0 {
        return None;
    }

    Some((
        shorter_side(width, side, longest),
        shorter_side(height, side, longest),
    ))
}

const fn shorter_side(length: u32, side: u32, longest: u32) -> u32 {
    match (length as u64 * side as u64).div_ceil(longest as u64) as u32 {
        0 => 1,
        shortened => shortened,
    }
}

fn linear_from_srgb(value: f32) -> f32 {
    if value <= SRGB_ENCODED_KNEE {
        value / SRGB_SLOPE
    } else {
        ((value + SRGB_OFFSET) / (1.0 + SRGB_OFFSET)).powf(SRGB_EXPONENT)
    }
}

fn srgb_from_linear(value: f32) -> f32 {
    if value <= SRGB_LINEAR_KNEE {
        value * SRGB_SLOPE
    } else {
        (1.0 + SRGB_OFFSET) * value.powf(1.0 / SRGB_EXPONENT) - SRGB_OFFSET
    }
}

fn eight_bit(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * EIGHT_BIT_FULL_SCALE).round() as u8
}

fn written_as_png(drawn: &RgbaImage) -> Option<CoverArt> {
    let mut bytes = Vec::new();
    match drawn.write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png) {
        Ok(()) => Some(CoverArt {
            format: ImageFormat::Png,
            bytes,
        }),
        Err(error) => {
            tracing::warn!(%error, "cover art could not be written for drawing");
            None
        }
    }
}

const fn read_as(format: ImageFormat) -> image::ImageFormat {
    match format {
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use image::{RgbImage, Rgba, Rgba32FImage, RgbaImage, imageops::FilterType};

    use super::{
        CoverArt, Decoded, Drawing, EIGHT_BIT_FULL_SCALE, ImageFormat, LINEAR_FROM_EIGHT_BIT, RGB,
        drawn_smaller, eight_bit, linear_from_srgb, sides_within, srgb_from_linear, written_as_png,
    };

    fn speckled(width: u32, height: u32) -> RgbaImage {
        let mut state = 0x2545_f491_u32;
        RgbaImage::from_fn(width, height, |_, _| {
            let mut next = || {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state.to_le_bytes()[0]
            };
            Rgba([next(), next(), next(), next()])
        })
    }

    fn drawn_by_the_image_crate(read: &RgbaImage, width: u32, height: u32) -> RgbaImage {
        let linear = &*LINEAR_FROM_EIGHT_BIT;
        let held = Rgba32FImage::from_fn(read.width(), read.height(), |x, y| {
            let Rgba([red, green, blue, alpha]) = *read.get_pixel(x, y);
            Rgba([
                linear[usize::from(red)],
                linear[usize::from(green)],
                linear[usize::from(blue)],
                f32::from(alpha) / EIGHT_BIT_FULL_SCALE,
            ])
        });
        let resized = image::imageops::resize(&held, width, height, FilterType::CatmullRom);
        RgbaImage::from_fn(width, height, |x, y| {
            let Rgba(from) = *resized.get_pixel(x, y);
            let mut into = [0; 4];
            for (drawn, value) in into.iter_mut().zip(&from[..RGB]) {
                *drawn = eight_bit(srgb_from_linear(*value));
            }
            into[RGB] = eight_bit(from[RGB]);
            Rgba(into)
        })
    }

    fn painted(width: u32, height: u32, shade: u8) -> CoverArt {
        let drawn = RgbaImage::from_pixel(width, height, image::Rgba([shade, shade, shade, 255]));

        written_as_png(&drawn).expect("a flat image is written as PNG")
    }

    #[test]
    fn a_cover_already_small_enough_is_left_alone() {
        assert_eq!(sides_within(128, 96, 128), None);
        assert_eq!(sides_within(64, 64, 128), None);
        assert_eq!(sides_within(0, 10, 128), None);
    }

    #[test]
    fn a_cover_keeps_its_shape_as_it_shrinks() {
        assert_eq!(sides_within(1000, 1000, 128), Some((128, 128)));
        assert_eq!(sides_within(512, 256, 128), Some((128, 64)));
        assert_eq!(sides_within(256, 512, 128), Some((64, 128)));
        assert_eq!(sides_within(4000, 3, 128), Some((128, 1)));
    }

    #[test]
    fn a_drawn_cover_is_the_side_it_was_asked_for() {
        let side = NonZeroU32::new(128).expect("128 is not zero");
        let drawn = Drawing::of(&painted(512, 256, 200))
            .expect("a flat image reads back")
            .no_larger_than(side)
            .expect("the cover shrinks");

        assert_eq!(drawn.format, ImageFormat::Png);

        let read = image::load_from_memory_with_format(&drawn.bytes, image::ImageFormat::Png)
            .expect("the cover reads back");

        assert_eq!((read.width(), read.height()), (128, 64));
    }

    #[test]
    fn a_squared_picture_is_the_middle_of_the_longer_side_at_the_side_asked_for() {
        let side = NonZeroU32::new(64).expect("64 is not zero");
        let drawn = Drawing::of(&painted(512, 256, 200))
            .expect("a flat image reads back")
            .squared(side)
            .expect("the picture is cropped");
        let read = image::load_from_memory_with_format(&drawn.bytes, image::ImageFormat::Png)
            .expect("the picture reads back");

        assert_eq!((read.width(), read.height()), (64, 64));
    }

    #[test]
    fn a_squared_picture_already_small_enough_is_cropped_and_not_grown() {
        let side = NonZeroU32::new(128).expect("128 is not zero");
        let drawn = Drawing::of(&painted(40, 90, 200))
            .expect("a flat image reads back")
            .squared(side)
            .expect("the picture is cropped");
        let read = image::load_from_memory_with_format(&drawn.bytes, image::ImageFormat::Png)
            .expect("the picture reads back");

        assert_eq!((read.width(), read.height()), (40, 40));
    }

    #[test]
    fn a_flat_cover_keeps_its_shade_through_linear_light() {
        let side = NonZeroU32::new(64).expect("64 is not zero");
        let drawn = Drawing::of(&painted(400, 400, 137))
            .expect("a flat image reads back")
            .no_larger_than(side)
            .expect("the cover shrinks");
        let read = image::load_from_memory_with_format(&drawn.bytes, image::ImageFormat::Png)
            .expect("the cover reads back")
            .to_rgba8();

        for pixel in read.pixels() {
            assert_eq!(pixel.0, [137, 137, 137, 255]);
        }
    }

    #[test]
    fn the_transfer_function_is_its_own_inverse() {
        for step in 0..=255u16 {
            let value = f32::from(step) / 255.0;

            assert_eq!(
                eight_bit(srgb_from_linear(linear_from_srgb(value))),
                step as u8
            );
        }
    }

    #[test]
    fn a_cover_is_drawn_exactly_as_the_image_crate_would_draw_it() {
        let read = speckled(203, 117);
        let drawing = Drawing(Decoded::Rgba(read.clone()));

        for (width, height) in [(52, 30), (128, 74), (17, 117), (203, 9)] {
            assert_eq!(
                drawn_smaller(drawing.plane(), width, height),
                drawn_by_the_image_crate(&read, width, height),
                "{width}x{height}"
            );
        }
    }

    #[test]
    fn an_opaque_cover_is_drawn_alike_with_or_without_its_alpha() {
        let read = speckled(160, 90);
        let opaque = RgbImage::from_fn(160, 90, |x, y| {
            let Rgba([red, green, blue, _]) = *read.get_pixel(x, y);
            image::Rgb([red, green, blue])
        });
        let side = NonZeroU32::new(64).expect("64 is not zero");
        let with_alpha = Drawing(Decoded::Rgba(
            image::DynamicImage::ImageRgb8(opaque.clone()).into_rgba8(),
        ));
        let without = Drawing(Decoded::Rgb(opaque));

        assert_eq!(
            with_alpha.no_larger_than(side),
            without.no_larger_than(side)
        );
        assert_eq!(with_alpha.squared(side), without.squared(side));
    }
}
