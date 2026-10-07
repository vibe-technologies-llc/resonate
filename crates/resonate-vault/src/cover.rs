use std::io::Cursor;

use image::{
    DynamicImage, ExtendedColorType, ImageEncoder as _, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
use jxl_oxide::InitializeResult;
use resonate_codec::{CoverArt, ImageFormat};
use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
use zune_jpegxl::JxlSimpleEncoder;

use crate::error::{Error, PictureOp, Result};

const MOST_EFFORT: u8 = 127;
const OPAQUE: u8 = 255;
const RGB_CHANNELS: usize = 3;
const RGBA_CHANNELS: usize = 4;

pub(crate) struct Drawn {
    pub(crate) bytes: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn as_jxl(art: &CoverArt) -> Result<Drawn> {
    let picture = read(art)?;
    let (width, height) = (picture.width(), picture.height());
    let opaque = picture.pixels().all(|pixel| pixel.0[3] == OPAQUE);

    let (colorspace, samples) = if opaque {
        (ColorSpace::RGB, without_alpha(&picture))
    } else {
        (ColorSpace::RGBA, picture.into_raw())
    };

    let options = EncoderOptions::new(width as usize, height as usize, colorspace, BitDepth::Eight)
        .set_effort(MOST_EFFORT);

    let mut bytes = Vec::new();
    JxlSimpleEncoder::new(&samples, options)
        .encode(&mut bytes)
        .map_err(|source| Error::PictureWritten {
            source: Box::new(source),
        })?;

    Ok(Drawn {
        bytes,
        width,
        height,
    })
}

pub(crate) fn size_of_jxl(bytes: &[u8]) -> Result<(u32, u32)> {
    let unopened = || Error::PictureRead {
        op: PictureOp::Open,
    };
    let mut reading = jxl_oxide::JxlImage::builder().build_uninit();
    reading.feed_bytes(bytes).map_err(|source| {
        tracing::warn!(%source, "a JXL picture's head could not be read");
        unopened()
    })?;
    match reading.try_init().map_err(|source| {
        tracing::warn!(%source, "a JXL picture's head could not be read");
        unopened()
    })? {
        InitializeResult::Initialized(picture) => Ok((picture.width(), picture.height())),
        InitializeResult::NeedMoreData(_) => Err(unopened()),
    }
}

pub(crate) fn size_of(bytes: &[u8], format: Option<ImageFormat>) -> Result<(u32, u32)> {
    let Some(format) = format else {
        return size_of_jxl(bytes);
    };
    let mut reading = image::ImageReader::new(Cursor::new(bytes));
    reading.set_format(read_as(format));
    reading.into_dimensions().map_err(|source| Error::Picture {
        source: Box::new(source),
    })
}

pub(crate) fn pixels_of_jxl(bytes: &[u8]) -> Result<RgbaImage> {
    let picture = jxl_oxide::JxlImage::builder()
        .read(Cursor::new(bytes))
        .map_err(|source| {
            tracing::warn!(%source, "a JXL picture could not be opened");
            Error::PictureRead {
                op: PictureOp::Open,
            }
        })?;
    let render = picture.render_frame(0).map_err(|source| {
        tracing::warn!(%source, "a JXL picture could not be rendered");
        Error::PictureRead {
            op: PictureOp::Render,
        }
    })?;

    let mut stream = render.stream();
    let width = stream.width();
    let height = stream.height();
    let channels = stream.channels() as usize;

    let mut read = vec![0_u8; width as usize * height as usize * channels];
    stream.write_to_buffer(&mut read);

    let mut rgba = Vec::with_capacity(width as usize * height as usize * RGBA_CHANNELS);
    for pixel in read.chunks_exact(channels) {
        match channels {
            RGB_CHANNELS => {
                rgba.extend_from_slice(pixel);
                rgba.push(OPAQUE);
            }
            RGBA_CHANNELS => rgba.extend_from_slice(pixel),
            _ => {
                let grey = pixel.first().copied().unwrap_or_default();
                rgba.extend_from_slice(&[grey, grey, grey, OPAQUE]);
            }
        }
    }

    RgbaImage::from_raw(width, height, rgba).ok_or(Error::PictureUnsized { width, height })
}

pub(crate) fn drawable(bytes: &[u8]) -> Result<CoverArt> {
    let picture = pixels_of_jxl(bytes)?;
    let mut written = Vec::new();
    PngEncoder::new_with_quality(&mut written, CompressionType::Fast, FilterType::Adaptive)
        .write_image(
            picture.as_raw(),
            picture.width(),
            picture.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|source| Error::Picture {
            source: Box::new(source),
        })?;

    Ok(CoverArt {
        format: ImageFormat::Png,
        bytes: written,
    })
}

pub(crate) fn read(art: &CoverArt) -> Result<RgbaImage> {
    image::load_from_memory_with_format(&art.bytes, read_as(art.format))
        .map(DynamicImage::into_rgba8)
        .map_err(|source| Error::Picture {
            source: Box::new(source),
        })
}

fn without_alpha(picture: &RgbaImage) -> Vec<u8> {
    let mut samples = Vec::with_capacity(picture.as_raw().len() / RGBA_CHANNELS * RGB_CHANNELS);
    for pixel in picture.pixels() {
        samples.extend_from_slice(&pixel.0[..RGB_CHANNELS]);
    }
    samples
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
    use super::*;

    fn noisy(width: u32, height: u32) -> CoverArt {
        let mut state = 0x2545_f491_u32;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let [red, green, blue, _] = state.to_le_bytes();
            pixels.extend_from_slice(&[red, green, blue, OPAQUE]);
        }
        let drawn = RgbaImage::from_raw(width, height, pixels).expect("a drawn picture");
        let mut written = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(drawn)
            .write_to(&mut written, image::ImageFormat::Png)
            .expect("a written picture");
        CoverArt {
            format: ImageFormat::Png,
            bytes: written.into_inner(),
        }
    }

    #[test]
    fn a_jxl_is_sized_by_its_head_alone() {
        let drawn = as_jxl(&noisy(64, 48)).expect("a JXL");
        let head = &drawn.bytes[..drawn.bytes.len() / 4];

        assert_eq!(size_of_jxl(&drawn.bytes).expect("a size"), (64, 48));
        assert_eq!(size_of_jxl(head).expect("a size off the head"), (64, 48));
        assert!(pixels_of_jxl(head).is_err());
    }
}
