use resonate_core::{FrameSpan, Frames, MediaLocation};
use symphonia::core::{
    formats::{Attachment, FormatReader},
    meta::{StandardVisualKey, Visual},
};

use crate::{
    MediaInfo, Result,
    container::{self, Pictures},
    cue,
    source::Sources,
};

const MAX_COVER_BYTES: usize = 24 * 1024 * 1024;
const COVER_PREFIX: &[u8] = b"cover";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Webp,
    Gif,
    Bmp,
}

impl ImageFormat {
    pub fn sniff(data: &[u8]) -> Option<Self> {
        from_magic(data)
    }

    pub const fn extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
        }
    }

    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Jpeg => "image/jpeg",
            Self::Png => "image/png",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverArt {
    pub format: ImageFormat,
    pub bytes: Vec<u8>,
}

pub fn probe(sources: &Sources, location: &MediaLocation) -> Result<MediaInfo> {
    if let Some(info) = stood_in(sources, location, None) {
        return Ok(info);
    }
    container::open_media(sources, location)?.media_info(location)
}

fn stood_in(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Option<MediaInfo> {
    let stood = sources.stood_in(location, span)?;
    let opened = container::open_media(sources, &stood.location)
        .and_then(|opened| opened.media_info(&stood.location));
    match opened {
        Ok(mut info) => {
            info.tags = stood.tags;
            info.cue = None;
            Some(info)
        }
        Err(error) => {
            tracing::debug!(%error, %location, "what stands in for a row would not open; reading the row itself");
            None
        }
    }
}

pub fn probe_span(
    sources: &Sources,
    location: &MediaLocation,
    span: FrameSpan,
) -> Result<MediaInfo> {
    if let Some(info) = stood_in(sources, location, Some(span)) {
        return Ok(info);
    }
    let mut info = container::open_media(sources, location)?.media_info(location)?;
    let held = span.within(info.duration.unwrap_or(Frames(u64::MAX)));

    let cut = cue::cut_for(sources, location, &info);
    if let Some(track) = cut
        .as_ref()
        .and_then(|cut| cut.cut_at(held.start(), info.spec.rate))
    {
        info.tags = track.titled();
    }
    info.duration = held.frames();

    Ok(info)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picturing {
    Whether,
    Copied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pictured {
    StoodIn,
    Bare,
    Carried,
    Copied(CoverArt),
}

impl Pictured {
    pub const fn carries_one(&self) -> bool {
        matches!(self, Self::Carried | Self::Copied(_))
    }

    pub fn into_copied(self) -> Option<CoverArt> {
        match self {
            Self::Copied(art) => Some(art),
            Self::StoodIn | Self::Bare | Self::Carried => None,
        }
    }
}

pub fn probe_pictured(
    sources: &Sources,
    location: &MediaLocation,
    picturing: Picturing,
) -> Result<(MediaInfo, Pictured)> {
    if let Some(info) = stood_in(sources, location, None) {
        return Ok((info, Pictured::StoodIn));
    }
    let mut opened = container::open_media(sources, location)?;
    let info = opened.media_info(location)?;
    let pictured = match picturing {
        Picturing::Whether if carries_cover_art(opened.pictures_mut()) => Pictured::Carried,
        Picturing::Whether => Pictured::Bare,
        Picturing::Copied => {
            cover_art(opened.pictures_mut()).map_or(Pictured::Bare, Pictured::Copied)
        }
    };

    Ok((info, pictured))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PacketDigest(pub u64);

#[derive(Clone, Debug, PartialEq)]
pub struct Scanned {
    pub info: MediaInfo,
    pub carries_a_picture: bool,
    pub packets: Option<PacketDigest>,
}

const PACKETS_DIGESTED: usize = 48;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn probe_scanned(sources: &Sources, location: &MediaLocation) -> Result<Scanned> {
    let mut opened = container::open_media(sources, location)?;
    let info = opened.media_info(location)?;
    let carries_a_picture = carries_cover_art(opened.pictures_mut());
    let packets = opened
        .into_coded()
        .and_then(|coded| digest_of_the_first_packets(coded.reader, location));

    Ok(Scanned {
        info,
        carries_a_picture,
        packets,
    })
}

fn digest_of_the_first_packets(
    mut reader: Box<dyn FormatReader>,
    location: &MediaLocation,
) -> Option<PacketDigest> {
    let (track, _) = container::audio_track(reader.as_ref(), location).ok()?;
    let track = track.id;
    let mut digest = FNV_OFFSET_BASIS;
    let mut digested = 0;
    while digested < PACKETS_DIGESTED {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(error) => {
                tracing::debug!(%error, %location, "the packets a scan digests stopped early");
                break;
            }
        };
        if packet.track_id != track {
            continue;
        }
        for byte in packet.data.iter() {
            digest = (digest ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
        }
        digested += 1;
    }
    (digested > 0).then_some(PacketDigest(digest))
}

pub fn probe_cover_art(sources: &Sources, location: &MediaLocation) -> Result<Option<CoverArt>> {
    let mut opened = container::open_media(sources, location)?;
    Ok(cover_art(opened.pictures_mut()))
}

fn cover_art(pictures: Pictures<'_>) -> Option<CoverArt> {
    picture(pictures, &copied)
}

fn carries_cover_art(pictures: Pictures<'_>) -> bool {
    picture(pictures, &|_, _| ()).is_some()
}

fn copied(format: ImageFormat, data: &[u8]) -> CoverArt {
    CoverArt {
        format,
        bytes: data.to_vec(),
    }
}

fn picture<T>(pictures: Pictures<'_>, taken: &dyn Fn(ImageFormat, &[u8]) -> T) -> Option<T> {
    let (reader, chunk) = match pictures {
        Pictures::Read { reader, chunk } => (reader, chunk),
        Pictures::Held(visuals) => return choose(visuals, taken),
    };
    if let Some(art) = attached_cover(reader.attachments(), taken) {
        return Some(art);
    }
    if let Some(art) = choose(chunk, taken) {
        return Some(art);
    }

    let mut metadata = reader.metadata();
    choose(&metadata.skip_to_latest()?.media.visuals, taken)
}

fn choose<T>(visuals: &[Visual], taken: &dyn Fn(ImageFormat, &[u8]) -> T) -> Option<T> {
    let front = visuals
        .iter()
        .filter(|visual| matches!(visual.usage, Some(StandardVisualKey::FrontCover)));

    front
        .chain(visuals.iter())
        .find_map(|visual| decode_visual(visual, taken))
}

fn decode_visual<T>(visual: &Visual, taken: &dyn Fn(ImageFormat, &[u8]) -> T) -> Option<T> {
    let format = visual
        .media_type
        .as_deref()
        .and_then(from_media_type)
        .or_else(|| from_magic(&visual.data))?;

    held(format, &visual.data, taken)
}

fn held<T>(format: ImageFormat, data: &[u8], taken: &dyn Fn(ImageFormat, &[u8]) -> T) -> Option<T> {
    if data.len() > MAX_COVER_BYTES {
        tracing::debug!(
            bytes = data.len(),
            limit = MAX_COVER_BYTES,
            "declining a picture larger than a cover is held at"
        );
        return None;
    }

    Some(taken(format, data))
}

fn attached_cover<T>(
    attachments: &[Attachment],
    taken: &dyn Fn(ImageFormat, &[u8]) -> T,
) -> Option<T> {
    attachments.iter().find_map(|attachment| {
        let Attachment::File(file) = attachment else {
            return None;
        };
        if !named_as_a_cover(&file.name) {
            return None;
        }
        let format = file
            .media_type
            .as_deref()
            .and_then(from_media_type)
            .or_else(|| from_magic(&file.data))?;

        held(format, &file.data, taken)
    })
}

fn named_as_a_cover(name: &str) -> bool {
    name.as_bytes()
        .get(..COVER_PREFIX.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(COVER_PREFIX))
}

fn from_media_type(media_type: &str) -> Option<ImageFormat> {
    let media_type = media_type.trim();
    let subtype = media_type
        .strip_prefix("image/")
        .or_else(|| media_type.strip_prefix("IMAGE/"))
        .unwrap_or(media_type);

    if subtype.eq_ignore_ascii_case("jpeg") || subtype.eq_ignore_ascii_case("jpg") {
        Some(ImageFormat::Jpeg)
    } else if subtype.eq_ignore_ascii_case("png") {
        Some(ImageFormat::Png)
    } else if subtype.eq_ignore_ascii_case("webp") {
        Some(ImageFormat::Webp)
    } else if subtype.eq_ignore_ascii_case("gif") {
        Some(ImageFormat::Gif)
    } else if subtype.eq_ignore_ascii_case("bmp") || subtype.eq_ignore_ascii_case("x-ms-bmp") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn from_magic(data: &[u8]) -> Option<ImageFormat> {
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff];
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const GIF: &[u8] = b"GIF8";
    const RIFF: &[u8] = b"RIFF";
    const WEBP: &[u8] = b"WEBP";
    const BMP: &[u8] = b"BM";

    if data.starts_with(JPEG) {
        Some(ImageFormat::Jpeg)
    } else if data.starts_with(PNG) {
        Some(ImageFormat::Png)
    } else if data.starts_with(GIF) {
        Some(ImageFormat::Gif)
    } else if data.starts_with(RIFF) && data.get(8..12) == Some(WEBP) {
        Some(ImageFormat::Webp)
    } else if data.starts_with(BMP) {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visual(usage: Option<StandardVisualKey>, bytes: usize) -> Visual {
        let mut data = vec![0_u8; bytes];
        data.splice(..PNG_MAGIC.len(), PNG_MAGIC.iter().copied());

        Visual {
            media_type: Some("image/png".to_owned()),
            dimensions: None,
            color_mode: None,
            usage,
            tags: Vec::new(),
            data: data.into_boxed_slice(),
        }
    }

    const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

    #[test]
    fn a_picture_larger_than_the_bound_is_declined_before_it_is_copied() {
        assert_eq!(
            decode_visual(&visual(None, MAX_COVER_BYTES + 1), &copied),
            None
        );
        assert!(decode_visual(&visual(None, MAX_COVER_BYTES), &copied).is_some());
    }

    #[test]
    fn an_oversized_front_cover_gives_way_to_a_picture_that_fits() {
        let visuals = [
            visual(Some(StandardVisualKey::FrontCover), MAX_COVER_BYTES + 1),
            visual(None, 64),
        ];

        assert_eq!(
            choose(&visuals, &copied)
                .expect("the smaller picture")
                .bytes
                .len(),
            64
        );
    }

    #[test]
    fn a_file_whose_every_picture_is_oversized_answers_with_none() {
        let visuals = [
            visual(Some(StandardVisualKey::FrontCover), MAX_COVER_BYTES + 1),
            visual(None, MAX_COVER_BYTES + 2),
        ];

        assert_eq!(choose(&visuals, &copied), None);
    }

    #[test]
    fn a_media_type_names_the_image_format() {
        assert_eq!(from_media_type("image/JPEG"), Some(ImageFormat::Jpeg));
        assert_eq!(from_media_type(" image/png "), Some(ImageFormat::Png));
        assert_eq!(from_media_type("jpg"), Some(ImageFormat::Jpeg));
        assert_eq!(from_media_type("application/octet-stream"), None);
    }

    #[test]
    fn magic_bytes_identify_a_picture_whose_media_type_is_missing() {
        assert_eq!(
            from_magic(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(ImageFormat::Jpeg)
        );
        assert_eq!(
            from_magic(b"\x89PNG\r\n\x1a\n\x00\x00"),
            Some(ImageFormat::Png)
        );
        assert_eq!(
            from_magic(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::Webp)
        );
        assert_eq!(from_magic(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
        assert_eq!(from_magic(b"not a picture"), None);
    }
}
