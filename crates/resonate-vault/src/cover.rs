use std::io::Cursor;

use image::{
    DynamicImage, ExtendedColorType, ImageEncoder as _, RgbaImage,
    codecs::png::{CompressionType, FilterType, PngEncoder},
};
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
