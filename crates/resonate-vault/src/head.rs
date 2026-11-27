use std::io::{self, Read, Seek, SeekFrom};

use resonate_core::Frames;

use crate::key::{KEY_BYTES, VaultKey};

const MAGIC: [u8; 4] = *b"fLaC";
const ID3V2_MAGIC: [u8; 3] = *b"ID3";
const BLOCK_HEADER_BYTES: usize = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7f;
const STREAM_INFO_BYTES: usize = 34;
const SAMPLES_PACKED_AT: usize = 10;
const DIGEST_AT: usize = STREAM_INFO_BYTES - KEY_BYTES;
const BLOCKS_AT_MOST: usize = 1_024;
const HEAD_BYTES_AT_MOST: u64 = 64 << 20;
const ID3V1_BYTES: u64 = 128;
const ID3V1_MAGIC: [u8; 3] = *b"TAG";
const APE_FOOTER_BYTES: u64 = 32;
const APE_MAGIC: [u8; 8] = *b"APETAGEX";

const STREAM_INFO: u8 = 0;
const PADDING: u8 = 1;
const APPLICATION: u8 = 2;
const SEEK_TABLE: u8 = 3;
const VORBIS_COMMENT: u8 = 4;
const CUE_SHEET: u8 = 5;
const PICTURE: u8 = 6;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum BlockKind {
    Padding,
    Application,
    SeekTable,
    VorbisComment,
    CueSheet,
    Picture,
    Reserved(u8),
}

impl BlockKind {
    const fn of(tag: u8) -> Self {
        match tag {
            PADDING => Self::Padding,
            APPLICATION => Self::Application,
            SEEK_TABLE => Self::SeekTable,
            VORBIS_COMMENT => Self::VorbisComment,
            CUE_SHEET => Self::CueSheet,
            PICTURE => Self::Picture,
            other => Self::Reserved(other),
        }
    }

    pub const fn tag(self) -> u8 {
        match self {
            Self::Padding => PADDING,
            Self::Application => APPLICATION,
            Self::SeekTable => SEEK_TABLE,
            Self::VorbisComment => VORBIS_COMMENT,
            Self::CueSheet => CUE_SHEET,
            Self::Picture => PICTURE,
            Self::Reserved(tag) => tag,
        }
    }

    pub const fn names_places_in_the_frames(self) -> bool {
        matches!(self, Self::SeekTable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadBlock {
    pub kind: BlockKind,
    pub body: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    pub rate: u32,
    pub channels: u8,
    pub bits: u8,
    pub frames: Option<Frames>,
    pub digest: Option<VaultKey>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum TrailingTag {
    Id3v1,
    Ape,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum HeadFault {
    NotAFlac,
    TagBeforeTheStream,
    NoStreamInfo,
    CutShort,
    TooLarge,
    Unread(io::ErrorKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlacHead {
    pub info: StreamInfo,
    pub blocks: Vec<HeadBlock>,
    pub trailing: Option<TrailingTag>,
}

impl FlacHead {
    pub fn read<S: Read + Seek + ?Sized>(stream: &mut S) -> Result<Self, HeadFault> {
        stream.seek(SeekFrom::Start(0)).map_err(unread)?;

        let mut magic = [0_u8; MAGIC.len()];

        stream.read_exact(&mut magic).map_err(unread)?;
        if magic[..ID3V2_MAGIC.len()] == ID3V2_MAGIC {
            return Err(HeadFault::TagBeforeTheStream);
        }
        if magic != MAGIC {
            return Err(HeadFault::NotAFlac);
        }

        let mut info = None;
        let mut blocks = Vec::new();
        let mut held = 0_u64;

        loop {
            let mut header = [0_u8; BLOCK_HEADER_BYTES];

            stream.read_exact(&mut header).map_err(unread)?;

            let tag = header[0] & BLOCK_KIND;
            let length = u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]));

            held += length;
            if held > HEAD_BYTES_AT_MOST {
                return Err(HeadFault::TooLarge);
            }

            let mut body = Vec::new();

            Read::take(&mut *stream, length)
                .read_to_end(&mut body)
                .map_err(unread)?;
            if body.len() as u64 != length {
                return Err(HeadFault::CutShort);
            }
            match (tag, &info) {
                (STREAM_INFO, None) => info = Some(stream_info(&body)?),
                (_, None) => return Err(HeadFault::NoStreamInfo),
                (STREAM_INFO, Some(_)) => return Err(HeadFault::NotAFlac),
                (tag, Some(_)) => blocks.push(HeadBlock {
                    kind: BlockKind::of(tag),
                    body,
                }),
            }
            if blocks.len() > BLOCKS_AT_MOST {
                return Err(HeadFault::TooLarge);
            }
            if header[0] & LAST_BLOCK != 0 {
                break;
            }
        }

        let info = info.ok_or(HeadFault::NoStreamInfo)?;
        let trailing = trailing_tag(stream).map_err(unread)?;

        Ok(Self {
            info,
            blocks,
            trailing,
        })
    }

    pub fn carried(&self) -> Vec<HeadBlock> {
        self.blocks
            .iter()
            .filter(|block| !block.kind.names_places_in_the_frames())
            .cloned()
            .collect()
    }
}

fn unread(error: io::Error) -> HeadFault {
    match error.kind() {
        io::ErrorKind::UnexpectedEof => HeadFault::CutShort,
        kind => HeadFault::Unread(kind),
    }
}

fn stream_info(body: &[u8]) -> Result<StreamInfo, HeadFault> {
    let packed: [u8; 8] = body
        .get(SAMPLES_PACKED_AT..SAMPLES_PACKED_AT + 8)
        .and_then(|packed| packed.try_into().ok())
        .ok_or(HeadFault::NoStreamInfo)?;
    let digest: [u8; KEY_BYTES] = body
        .get(DIGEST_AT..STREAM_INFO_BYTES)
        .and_then(|digest| digest.try_into().ok())
        .ok_or(HeadFault::NoStreamInfo)?;
    let packed = u64::from_be_bytes(packed);
    let frames = packed & ((1 << 36) - 1);

    Ok(StreamInfo {
        rate: (packed >> 44) as u32,
        channels: ((packed >> 41) & 0b111) as u8 + 1,
        bits: ((packed >> 36) & 0b1_1111) as u8 + 1,
        frames: (frames != 0).then_some(Frames(frames)),
        digest: (digest != [0; KEY_BYTES]).then(|| VaultKey::of(digest)),
    })
}

fn trailing_tag<S: Read + Seek + ?Sized>(stream: &mut S) -> io::Result<Option<TrailingTag>> {
    let length = stream.seek(SeekFrom::End(0))?;

    if length >= ID3V1_BYTES {
        stream.seek(SeekFrom::Start(length - ID3V1_BYTES))?;

        let mut magic = [0_u8; ID3V1_MAGIC.len()];

        stream.read_exact(&mut magic)?;
        if magic == ID3V1_MAGIC {
            return Ok(Some(TrailingTag::Id3v1));
        }
    }
    if length >= APE_FOOTER_BYTES {
        stream.seek(SeekFrom::Start(length - APE_FOOTER_BYTES))?;

        let mut magic = [0_u8; APE_MAGIC.len()];

        stream.read_exact(&mut magic)?;
        if magic == APE_MAGIC {
            return Ok(Some(TrailingTag::Ape));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn block(tag: u8, last: bool, body: &[u8]) -> Vec<u8> {
        let length = (body.len() as u32).to_be_bytes();
        let mut bytes = vec![tag | if last { LAST_BLOCK } else { 0 }];

        bytes.extend_from_slice(&length[1..]);
        bytes.extend_from_slice(body);
        bytes
    }

    fn stream_info_of(rate: u32, channels: u8, bits: u8, frames: u64) -> Vec<u8> {
        let mut body = vec![0_u8; STREAM_INFO_BYTES];
        let packed = (u64::from(rate) << 44)
            | (u64::from(channels - 1) << 41)
            | (u64::from(bits - 1) << 36)
            | frames;

        body[SAMPLES_PACKED_AT..SAMPLES_PACKED_AT + 8].copy_from_slice(&packed.to_be_bytes());
        body[DIGEST_AT] = 7;
        body
    }

    #[test]
    fn a_head_names_its_stream_and_every_block_after_it_and_a_tag_at_the_end() {
        let mut file = MAGIC.to_vec();

        file.extend(block(
            STREAM_INFO,
            false,
            &stream_info_of(96_000, 2, 24, 1_234),
        ));
        file.extend(block(SEEK_TABLE, false, &[0; 18]));
        file.extend(block(VORBIS_COMMENT, false, b"comments"));
        file.extend(block(PADDING, true, &[0; 4]));
        file.extend_from_slice(&[0xff, 0xf8, 0, 0]);
        file.extend_from_slice(&APE_MAGIC);
        file.extend_from_slice(&[0; 24]);

        let head = FlacHead::read(&mut Cursor::new(file)).expect("a head");

        assert_eq!(head.info.rate, 96_000);
        assert_eq!(head.info.channels, 2);
        assert_eq!(head.info.bits, 24);
        assert_eq!(head.info.frames, Some(Frames(1_234)));
        assert!(head.info.digest.is_some());
        assert_eq!(
            head.blocks
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![
                BlockKind::SeekTable,
                BlockKind::VorbisComment,
                BlockKind::Padding
            ]
        );
        assert_eq!(
            head.carried()
                .iter()
                .map(|block| block.kind)
                .collect::<Vec<_>>(),
            vec![BlockKind::VorbisComment, BlockKind::Padding]
        );
        assert_eq!(head.trailing, Some(TrailingTag::Ape));
    }

    #[test]
    fn a_file_that_opens_on_a_tag_or_ends_short_is_no_head_to_carry() {
        let mut tagged = ID3V2_MAGIC.to_vec();

        tagged.extend_from_slice(&[4, 0, 0, 0, 0, 0, 0]);

        let mut cut = MAGIC.to_vec();

        cut.extend(block(STREAM_INFO, true, &stream_info_of(44_100, 2, 16, 9)));
        cut.truncate(cut.len() - 3);

        assert_eq!(
            FlacHead::read(&mut Cursor::new(tagged)),
            Err(HeadFault::TagBeforeTheStream)
        );
        assert_eq!(
            FlacHead::read(&mut Cursor::new(cut)),
            Err(HeadFault::CutShort)
        );
        assert_eq!(
            FlacHead::read(&mut Cursor::new(b"RIFF....".to_vec())),
            Err(HeadFault::NotAFlac)
        );
    }
}
