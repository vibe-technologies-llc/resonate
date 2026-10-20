use std::{
    fs::File,
    io::{BufReader, Cursor, Read},
    path::Path,
};

use image::{
    AnimationDecoder as _, ColorType, DynamicImage, ExtendedColorType, ImageBuffer,
    ImageDecoder as _, ImageEncoder as _, Pixel, Primitive,
    codecs::{
        gif::GifDecoder,
        png::{CompressionType, FilterType, PngDecoder, PngEncoder},
        webp::WebPDecoder,
    },
};
use jxl_oxide::InitializeResult;
use resonate_codec::{CoverArt, ImageFormat};
use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
use zune_jpegxl::JxlSimpleEncoder;

use crate::error::{Error, PictureOp, Result, VaultOp};

const HEAD_READ_AT_A_TIME: usize = 16 << 10;
const MOST_EFFORT: u8 = 127;
const SMALLEST_JXL_SIDE: u32 = 2;
const FRAMES_THAT_MAKE_AN_ANIMATION: usize = 2;
const WIDEST_EIGHT_BIT_SAMPLE: u32 = 8;

pub(crate) struct Drawn {
    pub(crate) bytes: Vec<u8>,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pixels: DynamicImage,
}

impl Drawn {
    pub(crate) fn held_by(&self, written: &[u8]) -> Result<bool> {
        Ok(lossless_of_jxl(written)? == self.pixels)
    }
}

pub(crate) enum Drawing {
    Lossless(Drawn),
    AsItCame { width: u32, height: u32 },
}

pub(crate) fn as_jxl(art: &CoverArt) -> Result<Drawing> {
    let pixels = without_opaque_alpha(read(art)?);
    let (width, height) = (pixels.width(), pixels.height());

    let layout = layout_of(pixels.color());
    let too_narrow = width < SMALLEST_JXL_SIDE || height < SMALLEST_JXL_SIDE;
    let Some((colorspace, depth)) = layout.filter(|_| !too_narrow) else {
        return Ok(Drawing::AsItCame { width, height });
    };
    if carries_more_than_pixels(art)? {
        return Ok(Drawing::AsItCame { width, height });
    }

    let options = EncoderOptions::new(width as usize, height as usize, colorspace, depth)
        .set_effort(MOST_EFFORT);
    let mut bytes = Vec::new();
    JxlSimpleEncoder::new(pixels.as_bytes(), options)
        .encode(&mut bytes)
        .map_err(|source| Error::PictureWritten {
            source: Box::new(source),
        })?;

    Ok(Drawing::Lossless(Drawn {
        bytes,
        width,
        height,
        pixels,
    }))
}

fn carries_more_than_pixels(art: &CoverArt) -> Result<bool> {
    let held = || Cursor::new(art.bytes.as_slice());
    let animated = match art.format {
        ImageFormat::Png => PngDecoder::new(held()).and_then(|decoder| decoder.is_apng()),
        ImageFormat::Webp => WebPDecoder::new(held()).map(|decoder| decoder.has_animation()),
        ImageFormat::Gif => GifDecoder::new(held()).map(|decoder| {
            decoder
                .into_frames()
                .take(FRAMES_THAT_MAKE_AN_ANIMATION)
                .count()
                >= FRAMES_THAT_MAKE_AN_ANIMATION
        }),
        ImageFormat::Jpeg | ImageFormat::Bmp => Ok(false),
    }
    .map_err(unread)?;
    if animated {
        return Ok(true);
    }

    let profiled = image::ImageReader::with_format(held(), read_as(art.format))
        .into_decoder()
        .and_then(|mut decoder| decoder.icc_profile())
        .map_err(unread)?;
    Ok(profiled.is_some())
}

const fn layout_of(color: ColorType) -> Option<(ColorSpace, BitDepth)> {
    Some(match color {
        ColorType::L8 => (ColorSpace::Luma, BitDepth::Eight),
        ColorType::La8 => (ColorSpace::LumaA, BitDepth::Eight),
        ColorType::Rgb8 => (ColorSpace::RGB, BitDepth::Eight),
        ColorType::Rgba8 => (ColorSpace::RGBA, BitDepth::Eight),
        ColorType::L16 => (ColorSpace::Luma, BitDepth::Sixteen),
        ColorType::La16 => (ColorSpace::LumaA, BitDepth::Sixteen),
        ColorType::Rgb16 => (ColorSpace::RGB, BitDepth::Sixteen),
        ColorType::Rgba16 => (ColorSpace::RGBA, BitDepth::Sixteen),
        _ => return None,
    })
}

fn without_opaque_alpha(picture: DynamicImage) -> DynamicImage {
    match picture {
        DynamicImage::ImageLumaA8(held) if all_opaque(&held) => {
            DynamicImage::ImageLuma8(DynamicImage::ImageLumaA8(held).into_luma8())
        }
        DynamicImage::ImageRgba8(held) if all_opaque(&held) => {
            DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(held).into_rgb8())
        }
        DynamicImage::ImageLumaA16(held) if all_opaque(&held) => {
            DynamicImage::ImageLuma16(DynamicImage::ImageLumaA16(held).into_luma16())
        }
        DynamicImage::ImageRgba16(held) if all_opaque(&held) => {
            DynamicImage::ImageRgb16(DynamicImage::ImageRgba16(held).into_rgb16())
        }
        held => held,
    }
}

fn all_opaque<P>(picture: &ImageBuffer<P, Vec<P::Subpixel>>) -> bool
where
    P: Pixel,
{
    let opaque = P::Subpixel::DEFAULT_MAX_VALUE;
    picture.pixels().all(|pixel| {
        pixel
            .channels()
            .last()
            .is_some_and(|alpha| *alpha == opaque)
    })
}

fn unread(source: image::ImageError) -> Error {
    Error::Picture {
        source: Box::new(source),
    }
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

pub(crate) fn size_of_file(path: &Path, format: Option<ImageFormat>) -> Result<(u32, u32)> {
    let opened = File::open(path).map_err(|source| Error::io(VaultOp::Read, path, source))?;
    let Some(format) = format else {
        return size_of_jxl_stream(path, opened);
    };
    let mut reading = image::ImageReader::new(BufReader::new(opened));
    reading.set_format(read_as(format));
    reading.into_dimensions().map_err(|source| Error::Picture {
        source: Box::new(source),
    })
}

fn size_of_jxl_stream(path: &Path, mut file: File) -> Result<(u32, u32)> {
    let unopened = || Error::PictureRead {
        op: PictureOp::Open,
    };
    let mut reading = jxl_oxide::JxlImage::builder().build_uninit();
    let mut chunk = vec![0_u8; HEAD_READ_AT_A_TIME];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|source| Error::io(VaultOp::Read, path, source))?;
        if read == 0 {
            return Err(unopened());
        }
        reading.feed_bytes(&chunk[..read]).map_err(|source| {
            tracing::warn!(%source, "a JXL picture's head could not be read");
            unopened()
        })?;
        match reading.try_init().map_err(|source| {
            tracing::warn!(%source, "a JXL picture's head could not be read");
            unopened()
        })? {
            InitializeResult::Initialized(picture) => {
                return Ok((picture.width(), picture.height()));
            }
            InitializeResult::NeedMoreData(unfinished) => reading = unfinished,
        }
    }
}

pub(crate) fn lossless_of_jxl(bytes: &[u8]) -> Result<DynamicImage> {
    let picture = jxl_oxide::JxlImage::builder()
        .read(Cursor::new(bytes))
        .map_err(|source| {
            tracing::warn!(%source, "a JXL picture could not be opened");
            Error::PictureRead {
                op: PictureOp::Open,
            }
        })?;
    let wide =
        picture.image_header().metadata.bit_depth.bits_per_sample() > WIDEST_EIGHT_BIT_SAMPLE;
    let render = picture.render_frame(0).map_err(|source| {
        tracing::warn!(%source, "a JXL picture could not be rendered");
        Error::PictureRead {
            op: PictureOp::Render,
        }
    })?;

    let mut stream = render.stream();
    let (width, height) = (stream.width(), stream.height());
    let channels = stream.channels() as usize;
    let samples = width as usize * height as usize * channels;

    let drawn = if wide {
        let mut read = vec![0_u16; samples];
        stream.write_to_buffer(&mut read);
        match channels {
            1 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageLuma16),
            2 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageLumaA16),
            3 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageRgb16),
            4 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageRgba16),
            _ => None,
        }
    } else {
        let mut read = vec![0_u8; samples];
        stream.write_to_buffer(&mut read);
        match channels {
            1 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageLuma8),
            2 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageLumaA8),
            3 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageRgb8),
            4 => ImageBuffer::from_raw(width, height, read).map(DynamicImage::ImageRgba8),
            _ => None,
        }
    };
    drawn.ok_or(Error::PictureUnsized { width, height })
}

pub(crate) fn drawable(bytes: &[u8]) -> Result<CoverArt> {
    let picture = lossless_of_jxl(bytes)?.into_rgba8();
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

fn read(art: &CoverArt) -> Result<DynamicImage> {
    resonate_codec::decode_cover(art).map_err(unread)
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
            pixels.extend_from_slice(&[red, green, blue, u8::MAX]);
        }
        let drawn = image::RgbaImage::from_raw(width, height, pixels).expect("a drawn picture");
        let mut written = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(drawn)
            .write_to(&mut written, image::ImageFormat::Png)
            .expect("a written picture");
        CoverArt {
            format: ImageFormat::Png,
            bytes: written.into_inner(),
        }
    }

    fn lossless(art: &CoverArt) -> Drawn {
        match as_jxl(art).expect("a drawing") {
            Drawing::Lossless(drawn) => drawn,
            Drawing::AsItCame { .. } => panic!("a plain picture was kept as it came"),
        }
    }

    fn written_png(picture: DynamicImage) -> Vec<u8> {
        let mut written = Cursor::new(Vec::new());
        picture
            .write_to(&mut written, image::ImageFormat::Png)
            .expect("a written picture");
        written.into_inner()
    }

    fn sixteen_bit(width: u32, height: u32) -> DynamicImage {
        let mut samples = Vec::with_capacity((width * height * 3) as usize);
        for at in 0..width * height * 3 {
            samples.push((at.wrapping_mul(2_654_435_761) >> 7) as u16);
        }
        DynamicImage::ImageRgb16(
            ImageBuffer::from_raw(width, height, samples).expect("a sixteen-bit picture"),
        )
    }

    #[test]
    fn a_sixteen_bit_picture_reads_back_at_sixteen_bits_and_is_not_held_by_its_eight_bit_shadow() {
        let picture = sixteen_bit(24, 18);
        let art = CoverArt {
            format: ImageFormat::Png,
            bytes: written_png(picture.clone()),
        };

        let drawn = lossless(&art);
        let read = lossless_of_jxl(&drawn.bytes).expect("a JXL read back");
        let shadow = match as_jxl(&CoverArt {
            format: ImageFormat::Png,
            bytes: written_png(DynamicImage::ImageRgb8(picture.to_rgb8())),
        })
        .expect("a drawing")
        {
            Drawing::Lossless(shadow) => shadow,
            Drawing::AsItCame { .. } => panic!("a plain picture was kept as it came"),
        };

        assert_eq!(read.color(), ColorType::Rgb16);
        assert_eq!(read, picture);
        assert!(drawn.held_by(&drawn.bytes).expect("a JXL read back"));
        assert!(!drawn.held_by(&shadow.bytes).expect("a JXL read back"));
    }

    #[test]
    fn a_grey_picture_with_a_seen_through_alpha_reads_back_as_it_went_in() {
        let mut samples = Vec::new();
        for at in 0..20_u32 * 12 {
            samples.extend_from_slice(&[(at * 7) as u8, (at * 13) as u8]);
        }
        let picture = DynamicImage::ImageLumaA8(
            ImageBuffer::from_raw(20, 12, samples).expect("a grey picture"),
        );
        let art = CoverArt {
            format: ImageFormat::Png,
            bytes: written_png(picture.clone()),
        };

        let drawn = lossless(&art);

        assert_eq!(lossless_of_jxl(&drawn.bytes).expect("a read back"), picture);
    }

    #[test]
    fn a_picture_carrying_a_colour_profile_or_more_than_one_frame_is_kept_as_it_came() {
        let picture = DynamicImage::ImageRgb8(sixteen_bit(16, 16).to_rgb8());
        let mut profiled = Vec::new();
        let mut encoder = PngEncoder::new(&mut profiled);
        encoder
            .set_icc_profile(vec![0_u8; 132])
            .expect("a PNG that takes a profile");
        encoder
            .write_image(picture.as_bytes(), 16, 16, ExtendedColorType::Rgb8)
            .expect("a profiled picture");

        let mut animated = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut animated);
            for shade in [0_u8, 200] {
                let frame = image::RgbaImage::from_pixel(8, 8, image::Rgba([shade, 0, 0, 255]));
                encoder
                    .encode_frame(image::Frame::new(frame))
                    .expect("a written frame");
            }
        }
        let mut still = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut still);
            let frame = image::RgbaImage::from_pixel(8, 8, image::Rgba([9, 0, 0, 255]));
            encoder
                .encode_frame(image::Frame::new(frame))
                .expect("a written frame");
        }

        for (format, bytes) in [(ImageFormat::Png, profiled), (ImageFormat::Gif, animated)] {
            assert!(matches!(
                as_jxl(&CoverArt { format, bytes }).expect("a drawing"),
                Drawing::AsItCame { .. }
            ));
        }
        assert!(matches!(
            as_jxl(&CoverArt {
                format: ImageFormat::Gif,
                bytes: still,
            })
            .expect("a drawing"),
            Drawing::Lossless(_)
        ));
    }

    #[test]
    fn a_jxl_is_sized_by_its_head_alone() {
        let drawn = lossless(&noisy(64, 48));
        let head = &drawn.bytes[..drawn.bytes.len() / 4];

        assert_eq!(size_of_jxl(&drawn.bytes).expect("a size"), (64, 48));
        assert_eq!(size_of_jxl(head).expect("a size off the head"), (64, 48));
        assert!(lossless_of_jxl(head).is_err());
    }

    #[test]
    fn a_standing_file_is_sized_off_its_head_in_chunks() {
        let folder =
            std::env::temp_dir().join(format!("resonate-cover-sized-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("a folder");
        let art = noisy(160, 120);
        let drawn = lossless(&art);
        assert!(drawn.bytes.len() > 2 * HEAD_READ_AT_A_TIME);

        let jxl = folder.join("cover.jxl");
        let png = folder.join("cover.png");
        let empty = folder.join("empty.jxl");
        std::fs::write(&jxl, &drawn.bytes).expect("a written JXL");
        std::fs::write(&png, &art.bytes).expect("a written PNG");
        std::fs::write(&empty, []).expect("an empty file");

        let sized_jxl = size_of_file(&jxl, None);
        let sized_png = size_of_file(&png, Some(ImageFormat::Png));
        let sized_empty = size_of_file(&empty, None);
        std::fs::remove_dir_all(&folder).expect("a removed folder");

        assert_eq!(sized_jxl.expect("a JXL size"), (160, 120));
        assert_eq!(sized_png.expect("a PNG size"), (160, 120));
        assert!(sized_empty.is_err());
    }
}
