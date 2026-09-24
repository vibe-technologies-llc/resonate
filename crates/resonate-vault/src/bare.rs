use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use resonate_codec::{Container, MediaStream};

use crate::{
    error::{Error, Result, VaultOp},
    ogg::{self, Renumbering},
};

pub(crate) const FLAC_MAGIC: [u8; 4] = *b"fLaC";
const METADATA_HEADER_BYTES: usize = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7f;
const STREAM_INFO: u8 = 0;
const MOST_METADATA_BYTES: u64 = 64 << 20;

const ID3V2_MAGIC: &[u8; 3] = b"ID3";
const ID3V2_HEADER_BYTES: u64 = 10;
const ID3V2_FOOTER_BYTES: u64 = 10;
const ID3V2_HAS_A_FOOTER: u8 = 0x10;
const ID3V2_FOOTER_MAGIC: &[u8; 3] = b"3DI";
const ID3V1_MAGIC: &[u8; 3] = b"TAG";
const ID3V1_BYTES: u64 = 128;
const ENHANCED_ID3V1_MAGIC: &[u8; 4] = b"TAG+";
const ENHANCED_ID3V1_BYTES: u64 = 227;
const APE_MAGIC: &[u8; 8] = b"APETAGEX";
const APE_FOOTER_BYTES: u64 = 32;
const APE_SIZE_AT: usize = 12;
const APE_FLAGS_AT: usize = 20;
const APE_HAS_A_HEADER: u32 = 1 << 31;
const LYRICS3_MAGIC: &[u8; 9] = b"LYRICS200";
const LYRICS3_SIZE_DIGITS: usize = 6;
const LYRICS3_TRAILER_BYTES: u64 = 15;
const STACKED_TAGS_AT_MOST: usize = 8;
const SYNCSAFE_BITS: u32 = 7;
const SYNCSAFE_CEILING: u8 = 0x80;

const DSF_MAGIC: &[u8; 4] = b"DSD ";
const DSF_HEAD_BYTES: usize = 28;
const DSF_TOTAL_AT: usize = 12;
const DSF_METADATA_AT: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Bare {
    pub(crate) head: Vec<u8>,
    pub(crate) until: Option<u64>,
    pub(crate) renumbering: Option<Renumbering>,
}

pub(crate) fn bare(
    stream: &mut Box<dyn MediaStream>,
    container: Container,
    named: &Path,
) -> Result<Option<Bare>> {
    if !stream.is_seekable() {
        return Ok(None);
    }

    let found = match container {
        Container::Flac => bare_flac(stream),
        Container::Mpeg | Container::Adts => untagged_frames(stream),
        Container::Dsf => bare_dsf(stream),
        Container::Ogg => ogg::bare(stream).map(|bared| Bare {
            head: bared.head,
            until: None,
            renumbering: bared.renumbering,
        }),
        Container::Wave
        | Container::Aiff
        | Container::Caf
        | Container::Dff
        | Container::IsoMp4
        | Container::Matroska
        | Container::Unknown => None,
    };

    if found.is_none() {
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|source| Error::io(VaultOp::Read, named, source))?;
    }
    Ok(found)
}

fn bare_flac(stream: &mut Box<dyn MediaStream>) -> Option<Bare> {
    let mut magic = [0_u8; FLAC_MAGIC.len()];
    stream.read_exact(&mut magic).ok()?;
    if magic != FLAC_MAGIC {
        return None;
    }

    let mut stream_info = None;
    let mut walked = 0_u64;

    loop {
        let mut header = [0_u8; METADATA_HEADER_BYTES];
        stream.read_exact(&mut header).ok()?;
        let last = header[0] & LAST_BLOCK != 0;
        let kind = header[0] & BLOCK_KIND;
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]);

        walked += u64::from(length);
        if walked > MOST_METADATA_BYTES {
            return None;
        }

        if kind == STREAM_INFO {
            let mut held = vec![0_u8; length as usize];
            stream.read_exact(&mut held).ok()?;
            stream_info = Some(held);
        } else {
            stream.seek(SeekFrom::Current(i64::from(length))).ok()?;
        }

        if last {
            break;
        }
    }

    let held = stream_info?;
    let length = u32::try_from(held.len()).ok()?.to_be_bytes();
    let mut head = Vec::with_capacity(FLAC_MAGIC.len() + METADATA_HEADER_BYTES + held.len());
    head.extend_from_slice(&FLAC_MAGIC);
    head.push(LAST_BLOCK | STREAM_INFO);
    head.extend_from_slice(&length[1..]);
    head.extend_from_slice(&held);
    Some(Bare {
        head,
        until: None,
        renumbering: None,
    })
}

fn untagged_frames(stream: &mut Box<dyn MediaStream>) -> Option<Bare> {
    let length = stream.seek(SeekFrom::End(0)).ok()?;
    let from = past_leading_tags(stream, length)?;
    let until = before_trailing_tags(stream, from, length)?;
    if from == 0 && until == length {
        return None;
    }
    if until <= from {
        return None;
    }

    stream.seek(SeekFrom::Start(from)).ok()?;
    Some(Bare {
        head: Vec::new(),
        until: Some(until),
        renumbering: None,
    })
}

fn past_leading_tags(stream: &mut Box<dyn MediaStream>, length: u64) -> Option<u64> {
    let mut at = 0_u64;
    for _ in 0..STACKED_TAGS_AT_MOST {
        let Some(header) = read_at::<10>(stream, at) else {
            break;
        };
        let Some(size) = id3v2_size(&header, ID3V2_MAGIC) else {
            break;
        };
        let footer = if header[5] & ID3V2_HAS_A_FOOTER != 0 {
            ID3V2_FOOTER_BYTES
        } else {
            0
        };
        let past = at
            .checked_add(ID3V2_HEADER_BYTES)?
            .checked_add(size)?
            .checked_add(footer)?;
        if past > length {
            return None;
        }
        at = past;
    }
    Some(at)
}

fn before_trailing_tags(stream: &mut Box<dyn MediaStream>, from: u64, length: u64) -> Option<u64> {
    let mut end = length;
    for _ in 0..STACKED_TAGS_AT_MOST {
        let Some(shed) = trailing_tag(stream, end) else {
            break;
        };
        end = end.checked_sub(shed).filter(|end| *end >= from)?;
    }
    Some(end)
}

fn trailing_tag(stream: &mut Box<dyn MediaStream>, end: u64) -> Option<u64> {
    if let Some(at) = end.checked_sub(ENHANCED_ID3V1_BYTES + ID3V1_BYTES)
        && read_at::<4>(stream, at).is_some_and(|magic| magic == *ENHANCED_ID3V1_MAGIC)
        && ends_in_id3v1(stream, end)
    {
        return Some(ENHANCED_ID3V1_BYTES + ID3V1_BYTES);
    }
    if ends_in_id3v1(stream, end) {
        return Some(ID3V1_BYTES);
    }
    if let Some(at) = end.checked_sub(APE_FOOTER_BYTES)
        && let Some(footer) = read_at::<32>(stream, at)
        && footer.starts_with(APE_MAGIC)
    {
        let size = u32::from_le_bytes(footer[APE_SIZE_AT..APE_SIZE_AT + 4].try_into().ok()?);
        let flags = u32::from_le_bytes(footer[APE_FLAGS_AT..APE_FLAGS_AT + 4].try_into().ok()?);
        let header = if flags & APE_HAS_A_HEADER != 0 {
            APE_FOOTER_BYTES
        } else {
            0
        };
        return u64::from(size)
            .checked_add(header)
            .filter(|shed| *shed >= APE_FOOTER_BYTES);
    }
    if let Some(at) = end.checked_sub(LYRICS3_TRAILER_BYTES)
        && let Some(trailer) = read_at::<15>(stream, at)
        && trailer.ends_with(LYRICS3_MAGIC)
    {
        let digits = std::str::from_utf8(&trailer[..LYRICS3_SIZE_DIGITS]).ok()?;
        return digits
            .parse::<u64>()
            .ok()?
            .checked_add(LYRICS3_TRAILER_BYTES);
    }
    if let Some(at) = end.checked_sub(ID3V2_FOOTER_BYTES)
        && let Some(footer) = read_at::<10>(stream, at)
        && let Some(size) = id3v2_size(&footer, ID3V2_FOOTER_MAGIC)
    {
        return size.checked_add(ID3V2_HEADER_BYTES + ID3V2_FOOTER_BYTES);
    }
    None
}

fn ends_in_id3v1(stream: &mut Box<dyn MediaStream>, end: u64) -> bool {
    end.checked_sub(ID3V1_BYTES)
        .and_then(|at| read_at::<3>(stream, at))
        .is_some_and(|magic| magic == *ID3V1_MAGIC)
}

fn id3v2_size(header: &[u8; 10], magic: &[u8; 3]) -> Option<u64> {
    let [a, b, c, major, minor, _, sizes @ ..] = *header;
    if [a, b, c] != *magic || major == 0xFF || minor == 0xFF {
        return None;
    }
    sizes.iter().try_fold(0_u64, |size, byte| {
        (*byte < SYNCSAFE_CEILING).then(|| (size << SYNCSAFE_BITS) | u64::from(*byte))
    })
}

fn bare_dsf(stream: &mut Box<dyn MediaStream>) -> Option<Bare> {
    let head = read_at::<DSF_HEAD_BYTES>(stream, 0)?;
    if !head.starts_with(DSF_MAGIC) {
        return None;
    }
    let length = stream.seek(SeekFrom::End(0)).ok()?;
    let metadata = u64::from_le_bytes(head[DSF_METADATA_AT..].try_into().ok()?);
    if metadata == 0 || metadata <= DSF_HEAD_BYTES as u64 || metadata > length {
        return None;
    }

    let mut bared = head.to_vec();
    bared[DSF_TOTAL_AT..DSF_METADATA_AT].copy_from_slice(&metadata.to_le_bytes());
    bared[DSF_METADATA_AT..].copy_from_slice(&0_u64.to_le_bytes());
    stream.seek(SeekFrom::Start(DSF_HEAD_BYTES as u64)).ok()?;
    Some(Bare {
        head: bared,
        until: Some(metadata),
        renumbering: None,
    })
}

fn read_at<const N: usize>(stream: &mut Box<dyn MediaStream>, at: u64) -> Option<[u8; N]> {
    stream.seek(SeekFrom::Start(at)).ok()?;
    let mut bytes = [0_u8; N];
    stream.read_exact(&mut bytes).ok()?;
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_codec::Reading;

    use super::*;

    const FRAMES: &[u8] = b"\xFF\xFB\x90\x00 the frames of an mpeg stream";

    fn block(kind: u8, last: bool, payload: &[u8]) -> Vec<u8> {
        let length = payload.len() as u32;
        let mut held = vec![
            kind | if last { LAST_BLOCK } else { 0 },
            (length >> 16) as u8,
            (length >> 8) as u8,
            length as u8,
        ];
        held.extend_from_slice(payload);
        held
    }

    fn streamed(bytes: Vec<u8>) -> Box<dyn MediaStream> {
        Box::new(Reading::new(Cursor::new(bytes)))
    }

    fn bared(bytes: Vec<u8>, container: Container) -> (Option<Bare>, Vec<u8>) {
        let mut stream = streamed(bytes);
        let bare = bare(&mut stream, container, Path::new("a.file")).expect("a readable stream");
        let mut rest = Vec::new();
        stream.read_to_end(&mut rest).expect("the rest");
        if let Some(until) = bare.as_ref().and_then(|bare| bare.until) {
            let at = stream.stream_position().expect("a position") - rest.len() as u64;
            rest.truncate((until - at) as usize);
        }
        (bare, rest)
    }

    fn id3v2(payload: &[u8], footer: bool) -> Vec<u8> {
        let size = payload.len() as u32;
        let syncsafe = [
            (size >> 21) as u8 & 0x7F,
            (size >> 14) as u8 & 0x7F,
            (size >> 7) as u8 & 0x7F,
            size as u8 & 0x7F,
        ];
        let flags = if footer { ID3V2_HAS_A_FOOTER } else { 0 };
        let mut tag = b"ID3\x04\x00".to_vec();
        tag.push(flags);
        tag.extend_from_slice(&syncsafe);
        tag.extend_from_slice(payload);
        if footer {
            tag.extend_from_slice(b"3DI\x04\x00");
            tag.push(flags);
            tag.extend_from_slice(&syncsafe);
        }
        tag
    }

    fn id3v1() -> Vec<u8> {
        let mut tag = b"TAG".to_vec();
        tag.resize(ID3V1_BYTES as usize, b' ');
        tag
    }

    fn ape(items: &[u8], header: bool) -> Vec<u8> {
        let size = (items.len() as u32 + APE_FOOTER_BYTES as u32).to_le_bytes();
        let flags = if header { APE_HAS_A_HEADER } else { 0 }.to_le_bytes();
        let framing = |mut part: Vec<u8>| {
            part.extend_from_slice(&2_000_u32.to_le_bytes());
            part.extend_from_slice(&size);
            part.extend_from_slice(&1_u32.to_le_bytes());
            part.extend_from_slice(&flags);
            part.resize(APE_FOOTER_BYTES as usize, 0);
            part
        };
        let mut tag = Vec::new();
        if header {
            tag.extend(framing(APE_MAGIC.to_vec()));
        }
        tag.extend_from_slice(items);
        tag.extend(framing(APE_MAGIC.to_vec()));
        tag
    }

    fn lyrics3(body: &[u8]) -> Vec<u8> {
        let mut tag = b"LYRICSBEGIN".to_vec();
        tag.extend_from_slice(body);
        let size = format!("{:06}", tag.len());
        tag.extend_from_slice(size.as_bytes());
        tag.extend_from_slice(LYRICS3_MAGIC);
        tag
    }

    #[test]
    fn a_head_that_never_reaches_its_last_block_is_copied_whole_rather_than_cut_short() {
        const PADDING: u8 = 1;

        let mut whole = FLAC_MAGIC.to_vec();
        whole.extend(block(STREAM_INFO, false, &[7_u8; 34]));
        whole.extend(block(PADDING, false, &[0_u8; 16]));

        let (bare, rest) = bared(whole.clone(), Container::Flac);

        assert_eq!(
            bare, None,
            "a STREAMINFO was marked last ahead of blocks it did not walk"
        );
        assert_eq!(rest, whole, "the file was not handed back from its start");
    }

    #[test]
    fn a_flac_head_is_rewritten_as_its_stream_info_alone() {
        const VORBIS_COMMENT: u8 = 4;
        const PICTURE: u8 = 6;

        let stream_info = vec![7_u8; 34];
        let mut whole = FLAC_MAGIC.to_vec();
        whole.extend(block(STREAM_INFO, false, &stream_info));
        whole.extend(block(VORBIS_COMMENT, false, b"a title nobody asked for"));
        whole.extend(block(PICTURE, true, &vec![9_u8; 4096]));
        whole.extend_from_slice(b"the frames that follow");

        let (bare, rest) = bared(whole, Container::Flac);

        let mut wanted = FLAC_MAGIC.to_vec();
        wanted.extend(block(STREAM_INFO, true, &stream_info));
        assert_eq!(bare.expect("a FLAC head").head, wanted);
        assert_eq!(rest, b"the frames that follow");
    }

    #[test]
    fn a_stream_that_is_not_what_its_container_says_is_left_where_it_was_found() {
        let (bare, rest) = bared(b"RIFF....WAVEfmt ".to_vec(), Container::Flac);

        assert!(bare.is_none());
        assert_eq!(rest, b"RIFF....WAVEfmt ");
    }

    #[test]
    fn an_mpeg_stream_sheds_the_tags_in_front_of_it_and_behind_it() {
        let mut whole = id3v2(&[0_u8; 700], false);
        whole.extend(id3v2(b"a second tag stacked on the first", true));
        whole.extend_from_slice(FRAMES);
        whole.extend(ape(b"an item or two", true));
        whole.extend(lyrics3(b"[00:01]a line"));
        whole.extend(id3v1());

        let (bare, rest) = bared(whole, Container::Mpeg);

        assert!(bare.expect("tags to shed").head.is_empty());
        assert_eq!(rest, FRAMES);
    }

    #[test]
    fn an_ape_tag_with_no_header_and_an_id3v2_footer_are_shed_from_the_tail() {
        let mut whole = FRAMES.to_vec();
        whole.extend(ape(b"items", false));
        whole.extend(id3v2(b"appended", true));

        let (_, rest) = bared(whole, Container::Adts);

        assert_eq!(rest, FRAMES);
    }

    #[test]
    fn an_mpeg_stream_carrying_no_tags_is_copied_as_it_stands() {
        let (bare, rest) = bared(FRAMES.to_vec(), Container::Mpeg);

        assert_eq!(bare, None);
        assert_eq!(rest, FRAMES);
    }

    #[test]
    fn a_tag_that_claims_more_than_the_file_holds_leaves_the_file_whole() {
        let mut whole = id3v2(&[0_u8; 64], false);
        whole[9] = 0x7F;
        whole.extend_from_slice(FRAMES);

        let (bare, rest) = bared(whole.clone(), Container::Mpeg);

        assert_eq!(bare, None);
        assert_eq!(rest, whole);
    }

    #[test]
    fn tags_that_would_leave_no_frames_leave_the_file_whole() {
        let mut whole = id3v2(b"all there is", false);
        whole.extend(id3v1());

        let (bare, rest) = bared(whole.clone(), Container::Mpeg);

        assert_eq!(bare, None);
        assert_eq!(rest, whole);
    }

    fn dsf(metadata: u64, body: &[u8], tag: &[u8]) -> Vec<u8> {
        let total = DSF_HEAD_BYTES as u64 + body.len() as u64 + tag.len() as u64;
        let mut whole = DSF_MAGIC.to_vec();
        whole.extend_from_slice(&(DSF_HEAD_BYTES as u64).to_le_bytes());
        whole.extend_from_slice(&total.to_le_bytes());
        whole.extend_from_slice(&metadata.to_le_bytes());
        whole.extend_from_slice(body);
        whole.extend_from_slice(tag);
        whole
    }

    #[test]
    fn a_dsf_ends_where_its_metadata_began_and_says_it_has_none() {
        let body = b"fmt and data chunks";
        let metadata = (DSF_HEAD_BYTES + body.len()) as u64;
        let whole = dsf(metadata, body, &id3v2(b"a title", false));

        let (bare, rest) = bared(whole, Container::Dsf);

        let mut head = DSF_MAGIC.to_vec();
        head.extend_from_slice(&(DSF_HEAD_BYTES as u64).to_le_bytes());
        head.extend_from_slice(&metadata.to_le_bytes());
        head.extend_from_slice(&0_u64.to_le_bytes());
        assert_eq!(bare.expect("a metadata chunk to shed").head, head);
        assert_eq!(rest, body);
    }

    #[test]
    fn a_dsf_that_points_at_no_metadata_or_past_its_end_is_copied_as_it_stands() {
        let body = b"fmt and data chunks";

        assert_eq!(bared(dsf(0, body, &[]), Container::Dsf).0, None);
        assert_eq!(bared(dsf(1 << 40, body, &[]), Container::Dsf).0, None);
    }
}
