use std::io::{Cursor, Read, Seek};

use lofty::file::FileType;

use crate::{
    ogg::{self, Codec, Layout, Paged},
    prescan::{past_id3, read_exact},
};

const FLAC_MAGIC: [u8; 4] = *b"fLaC";
const BLOCK_HEADER_BYTES: u64 = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7F;
const PADDING: u8 = 1;
const MOST_BLOCKS: usize = 1_024;
const LARGEST_BLOCK: u64 = (1 << 24) - 1;

const ID3: &[u8; 3] = b"ID3";
const ID3_HEADER_BYTES: usize = 10;
const ID3_FLAGS_AT: usize = 5;
const ID3_SIZE_AT: usize = 6;
const ID3_EXTENDED_HEADER: u8 = 0x40;
const ID3_FOOTER: u8 = 0x10;
const LARGEST_SYNCHSAFE: u64 = (1 << 28) - 1;

const PADDING_LEFT_FOR_THE_NEXT_EDIT: u64 = 8 << 10;
const MOST_BLOCKS_INSERTED: u64 = 64;
const LEEWAY_PAST_THE_HEAD: u64 = 64 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Padded {
    Flac,
    Id3,
    Ogg(Layout),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Room {
    Held,
    Generous,
    Blocks(u64),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Rebuilt {
    pub(crate) head: Vec<u8>,
    pub(crate) renumbered_from: Option<u32>,
}

impl Rebuilt {
    const fn unnumbered(head: Vec<u8>) -> Self {
        Self {
            head,
            renumbered_from: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Head {
    pub(crate) padded: Padded,
    pub(crate) length: u64,
}

impl Head {
    pub(crate) fn of<S: Read + Seek + ?Sized>(kind: FileType, source: &mut S) -> Option<Self> {
        source.rewind().ok()?;
        let padded = match kind {
            FileType::Flac => Padded::Flac,
            FileType::Mpeg | FileType::Aac => Padded::Id3,
            FileType::Vorbis => return Self::ogg(Codec::Vorbis, source),
            FileType::Opus => return Self::ogg(Codec::Opus, source),
            _ => return None,
        };
        let length = match padded {
            Padded::Flac => flac_metadata_end(source)?,
            Padded::Id3 => past_id3(source)?,
            Padded::Ogg(_) => return None,
        };
        Some(Self { padded, length })
    }

    fn ogg<S: Read + ?Sized>(codec: Codec, source: &mut S) -> Option<Self> {
        let headers = ogg::headers(codec, source)?;
        Some(Self {
            padded: Padded::Ogg(headers.layout(codec)?),
            length: headers.length,
        })
    }

    pub(crate) fn tail_read(self, stood: u64) -> Option<u64> {
        match self.padded {
            Padded::Ogg(_) => Some(0),
            Padded::Flac | Padded::Id3 => {
                let tail = stood.saturating_sub(self.length).min(LEEWAY_PAST_THE_HEAD);
                (tail > 0).then_some(tail)
            }
        }
    }

    pub(crate) fn rebuilt(self, written: &[u8], room: Room) -> Option<Rebuilt> {
        match self.padded {
            Padded::Flac => flac_rebuilt(written, self.length, room),
            Padded::Id3 => id3_rebuilt(written, self.length, room),
            Padded::Ogg(layout) => ogg_rebuilt(layout, self.length, written, room),
        }
    }
}

fn room_in_blocks(content: u64, standing: u64, block: u64) -> Option<u64> {
    let wanted = content.checked_add(PADDING_LEFT_FOR_THE_NEXT_EDIT)?;
    let blocks = wanted
        .saturating_sub(standing)
        .div_ceil(block.max(1))
        .max(1);
    standing.checked_add(blocks.checked_mul(block)?)
}

fn flac_rebuilt(written: &[u8], standing: u64, room: Room) -> Option<Rebuilt> {
    let content = flac_content(written)?;
    let length = match room {
        Room::Held => standing,
        Room::Generous => content + BLOCK_HEADER_BYTES + PADDING_LEFT_FOR_THE_NEXT_EDIT,
        Room::Blocks(block) => room_in_blocks(content + BLOCK_HEADER_BYTES, standing, block)?,
    };
    flac_absorbed(written, length).map(Rebuilt::unnumbered)
}

fn id3_rebuilt(written: &[u8], standing: u64, room: Room) -> Option<Rebuilt> {
    let content = id3_content(written)?;
    let length = match room {
        Room::Held => standing,
        Room::Generous => content + PADDING_LEFT_FOR_THE_NEXT_EDIT,
        Room::Blocks(block) => room_in_blocks(content, standing, block)?,
    };
    id3_absorbed(written, length).map(Rebuilt::unnumbered)
}

fn ogg_rebuilt(layout: Layout, standing: u64, written: &[u8], room: Room) -> Option<Rebuilt> {
    let parsed = ogg::headers(layout.codec, &mut Cursor::new(written))?;
    if parsed.length != written.len() as u64 {
        return None;
    }
    let comment = parsed.packets.get(1)?;
    let others: Vec<&Vec<u8>> = parsed
        .packets
        .iter()
        .enumerate()
        .filter(|(at, _)| *at != 1)
        .map(|(_, packet)| packet)
        .collect();
    let fixed = others.iter().fold(Paged::NOTHING, |paged, packet| {
        paged.with(Paged::of_packet(packet.len()))
    });

    let padded_to = match room {
        Room::Held => {
            let length = usize::try_from(layout.comment).ok()?;
            (comment.len() <= length).then_some(length)?
        }
        Room::Generous => comment.len() + usize::try_from(PADDING_LEFT_FOR_THE_NEXT_EDIT).ok()?,
        Room::Blocks(block) => {
            let pages = usize::try_from(layout.pages).ok()?;
            (1..=MOST_BLOCKS_INSERTED).find_map(|blocks| {
                let target =
                    usize::try_from(standing.checked_add(blocks.checked_mul(block)?)?).ok()?;
                (comment.len()..)
                    .map_while(|length| {
                        let paged = fixed.with(Paged::of_packet(length));
                        (paged.pages <= pages).then_some((length, paged))
                    })
                    .find(|(_, paged)| paged.pages == pages && paged.bytes == target)
                    .map(|(length, _)| length)
            })?
        }
    };

    let mut padded = comment.clone();
    padded.resize(padded_to, 0);
    let mut packets: Vec<&[u8]> = parsed.packets.iter().map(Vec::as_slice).collect();
    packets[1] = &padded;
    let head = ogg::paginated(parsed.serial, &packets);
    let pages = fixed.with(Paged::of_packet(padded.len())).pages;

    let held = matches!(room, Room::Held);
    if (held || matches!(room, Room::Blocks(_)))
        && (pages != usize::try_from(layout.pages).ok()? || (held && head.len() as u64 != standing))
    {
        return None;
    }
    Some(Rebuilt {
        head,
        renumbered_from: (pages != usize::try_from(layout.pages).ok()?)
            .then(|| u32::try_from(pages).ok())
            .flatten(),
    })
}

fn flac_metadata_end<S: Read + Seek + ?Sized>(source: &mut S) -> Option<u64> {
    let mut at = past_id3(source)?;
    source.seek(std::io::SeekFrom::Start(at)).ok()?;
    if read_exact::<4, S>(source)? != FLAC_MAGIC {
        return None;
    }
    at += FLAC_MAGIC.len() as u64;

    for _ in 0..MOST_BLOCKS {
        let header = read_exact::<4, S>(source)?;
        let length = block_length(header);
        at = at.checked_add(BLOCK_HEADER_BYTES + length)?;
        if header[0] & LAST_BLOCK != 0 {
            return Some(at);
        }
        source.seek(std::io::SeekFrom::Start(at)).ok()?;
    }
    None
}

fn block_length(header: [u8; 4]) -> u64 {
    u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]))
}

struct FlacBlocks<'w> {
    body: usize,
    kept: Vec<&'w [u8]>,
}

impl FlacBlocks<'_> {
    fn content(&self) -> u64 {
        self.body as u64
            + self
                .kept
                .iter()
                .map(|block| block.len() as u64)
                .sum::<u64>()
    }
}

fn flac_content(written: &[u8]) -> Option<u64> {
    Some(flac_blocks(written)?.content())
}

fn flac_blocks(written: &[u8]) -> Option<FlacBlocks<'_>> {
    let lead = usize::try_from(past_id3(&mut Cursor::new(written))?).ok()?;
    let body = lead.checked_add(FLAC_MAGIC.len())?;
    if written.get(lead..body)? != FLAC_MAGIC {
        return None;
    }

    let mut kept: Vec<&[u8]> = Vec::new();
    let mut at = body;
    loop {
        let header: [u8; 4] = written.get(at..at + 4)?.try_into().ok()?;
        let end = at.checked_add(4 + usize::try_from(block_length(header)).ok()?)?;
        let block = written.get(at..end)?;
        if header[0] & BLOCK_KIND != PADDING {
            kept.push(block);
        }
        at = end;
        if header[0] & LAST_BLOCK != 0 {
            break;
        }
        if kept.len() > MOST_BLOCKS {
            return None;
        }
    }
    if at != written.len() {
        return None;
    }
    Some(FlacBlocks { body, kept })
}

fn flac_absorbed(written: &[u8], length: u64) -> Option<Vec<u8>> {
    let blocks = flac_blocks(written)?;
    let content = blocks.content();
    let FlacBlocks { body, kept } = blocks;
    let padding = match length.checked_sub(content)? {
        0 => None,
        room => Some(
            room.checked_sub(BLOCK_HEADER_BYTES)
                .filter(|padding| *padding <= LARGEST_BLOCK)?,
        ),
    };

    let mut absorbed = Vec::with_capacity(usize::try_from(length).ok()?);
    absorbed.extend_from_slice(&written[..body]);
    let blocks = kept.len();
    for (index, block) in kept.into_iter().enumerate() {
        let last = index + 1 == blocks && padding.is_none();
        absorbed.push(match last {
            true => block[0] | LAST_BLOCK,
            false => block[0] & !LAST_BLOCK,
        });
        absorbed.extend_from_slice(&block[1..]);
    }
    if let Some(padding) = padding {
        let [_, high, middle, low] = u32::try_from(padding).ok()?.to_be_bytes();
        absorbed.extend_from_slice(&[PADDING | LAST_BLOCK, high, middle, low]);
        absorbed.resize(usize::try_from(length).ok()?, 0);
    }
    Some(absorbed)
}

fn id3_content(written: &[u8]) -> Option<u64> {
    if !written.starts_with(ID3) || written.len() < ID3_HEADER_BYTES {
        return None;
    }
    Some(
        written[ID3_HEADER_BYTES..]
            .iter()
            .rposition(|byte| *byte != 0)
            .map_or(ID3_HEADER_BYTES, |last| ID3_HEADER_BYTES + last + 1) as u64,
    )
}

fn id3_absorbed(written: &[u8], length: u64) -> Option<Vec<u8>> {
    if !written.starts_with(ID3) || written.len() < ID3_HEADER_BYTES {
        return None;
    }
    let flags = written[ID3_FLAGS_AT];
    if flags & (ID3_EXTENDED_HEADER | ID3_FOOTER) != 0 {
        return None;
    }
    let declared = synchsafe(&written[ID3_SIZE_AT..ID3_HEADER_BYTES])?;
    if ID3_HEADER_BYTES as u64 + declared != written.len() as u64 {
        return None;
    }

    let content = usize::try_from(id3_content(written)?).ok()?;
    let size = length.checked_sub(ID3_HEADER_BYTES as u64)?;
    if content as u64 > length || size > LARGEST_SYNCHSAFE {
        return None;
    }

    let mut absorbed = Vec::with_capacity(usize::try_from(length).ok()?);
    absorbed.extend_from_slice(&written[..ID3_SIZE_AT]);
    absorbed.extend_from_slice(&synchsafe_bytes(size));
    absorbed.extend_from_slice(&written[ID3_HEADER_BYTES..content]);
    absorbed.resize(usize::try_from(length).ok()?, 0);
    Some(absorbed)
}

fn synchsafe(bytes: &[u8]) -> Option<u64> {
    bytes.iter().try_fold(0_u64, |value, byte| {
        (byte & 0x80 == 0).then(|| (value << 7) | u64::from(*byte))
    })
}

fn synchsafe_bytes(size: u64) -> [u8; 4] {
    [21, 14, 7, 0].map(|shift| ((size >> shift) & 0x7F) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(kind: u8, last: bool, payload: &[u8]) -> Vec<u8> {
        let [_, high, middle, low] = u32::try_from(payload.len())
            .expect("a small block")
            .to_be_bytes();
        let mut bytes = vec![kind | if last { LAST_BLOCK } else { 0 }, high, middle, low];
        bytes.extend_from_slice(payload);
        bytes
    }

    fn flac_head(blocks: &[Vec<u8>]) -> Vec<u8> {
        let mut head = FLAC_MAGIC.to_vec();
        for block in blocks {
            head.extend_from_slice(block);
        }
        head
    }

    fn blocks_of(head: &[u8]) -> Vec<(u8, bool, usize)> {
        let mut at = FLAC_MAGIC.len();
        let mut blocks = Vec::new();
        while at < head.len() {
            let header: [u8; 4] = head[at..at + 4].try_into().expect("a header");
            let length = block_length(header) as usize;
            blocks.push((header[0] & BLOCK_KIND, header[0] & LAST_BLOCK != 0, length));
            at += 4 + length;
        }
        assert_eq!(at, head.len(), "the blocks did not end where the head does");
        blocks
    }

    #[test]
    fn a_grown_comment_takes_its_room_out_of_the_padding() {
        let standing = flac_head(&[
            block(0, false, &[0; 34]),
            block(4, false, &[1; 40]),
            block(PADDING, true, &[0; 1000]),
        ]);
        let written = flac_head(&[
            block(0, false, &[0; 34]),
            block(PADDING, false, &[0; 1000]),
            block(4, true, &[2; 140]),
        ]);

        let absorbed = flac_absorbed(&written, standing.len() as u64).expect("room to spare");

        assert_eq!(absorbed.len(), standing.len());
        assert_eq!(
            blocks_of(&absorbed),
            vec![(0, false, 34), (4, false, 140), (PADDING, true, 900)]
        );
        assert_eq!(
            flac_metadata_end(&mut Cursor::new(&absorbed)),
            Some(standing.len() as u64)
        );
    }

    #[test]
    fn a_comment_filling_the_padding_exactly_leaves_none() {
        let written = flac_head(&[block(0, false, &[0; 34]), block(4, true, &[1; 60])]);

        let absorbed = flac_absorbed(&written, written.len() as u64).expect("an exact fit");
        assert_eq!(blocks_of(&absorbed), vec![(0, false, 34), (4, true, 60)]);

        assert_eq!(
            flac_absorbed(&written, written.len() as u64 + 2),
            None,
            "a gap too small for a padding header was left"
        );
        assert_eq!(flac_absorbed(&written, written.len() as u64 - 1), None);
    }

    #[test]
    fn a_leading_id3_tag_of_its_own_is_kept_ahead_of_the_blocks() {
        let mut written = b"ID3\x04\x00\x00\x00\x00\x00\x02\x00\x00".to_vec();
        written.extend_from_slice(&flac_head(&[
            block(0, false, &[0; 34]),
            block(4, true, &[1; 10]),
        ]));

        let absorbed = flac_absorbed(&written, written.len() as u64 + 20).expect("room");
        assert_eq!(absorbed[..12], written[..12]);
        assert_eq!(
            flac_metadata_end(&mut Cursor::new(&absorbed)),
            Some(absorbed.len() as u64)
        );
    }

    fn id3(size: u64, flags: u8, frames: &[u8]) -> Vec<u8> {
        let mut tag = b"ID3\x04\x00".to_vec();
        tag.push(flags);
        tag.extend_from_slice(&synchsafe_bytes(size));
        tag.extend_from_slice(frames);
        tag.resize(ID3_HEADER_BYTES + size as usize, 0);
        tag
    }

    #[test]
    fn a_rewritten_id3_tag_is_padded_to_the_size_it_stood_at() {
        let written = id3(1200, 0, &[b'T'; 300]);

        let absorbed = id3_absorbed(&written, 800).expect("room to spare");

        assert_eq!(absorbed.len(), 800);
        assert_eq!(synchsafe(&absorbed[6..10]), Some(790));
        assert_eq!(absorbed[10..310], [b'T'; 300]);
        assert!(absorbed[310..].iter().all(|byte| *byte == 0));
        assert_eq!(
            past_id3(&mut Cursor::new(&absorbed)),
            Some(800),
            "the absorbed tag did not end where the old one did"
        );
    }

    #[test]
    fn an_id3_tag_that_will_not_fit_or_carries_a_footer_is_not_absorbed() {
        assert_eq!(id3_absorbed(&id3(1200, 0, &[b'T'; 300]), 200), None);
        assert_eq!(id3_absorbed(&id3(400, ID3_FOOTER, &[b'T'; 30]), 800), None);
        assert_eq!(
            id3_absorbed(&id3(400, ID3_EXTENDED_HEADER, &[b'T'; 30]), 800),
            None
        );
    }

    fn opus_head(comment_bytes: usize) -> Vec<u8> {
        let mut identification = b"OpusHead".to_vec();
        identification.extend_from_slice(&[1, 2, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
        let mut comments = b"OpusTags".to_vec();
        comments.extend_from_slice(&4_u32.to_le_bytes());
        comments.extend_from_slice(b"test");
        comments.extend_from_slice(&0_u32.to_le_bytes());
        comments.resize(comment_bytes.max(comments.len()), 0);
        ogg::paginated(7, &[&identification, &comments])
    }

    fn opus_head_of(bytes: &[u8]) -> Head {
        Head::of(FileType::Opus, &mut Cursor::new(bytes)).expect("an Opus head")
    }

    fn comment_length_of(head: &[u8]) -> usize {
        ogg::headers(Codec::Opus, &mut Cursor::new(head))
            .expect("a head")
            .packets[1]
            .len()
    }

    #[test]
    fn an_ogg_comment_that_fits_its_packet_is_padded_to_the_length_it_stood_at() {
        let standing = opus_head(3_000);
        let head = opus_head_of(&standing);
        let written = opus_head(400);

        let rebuilt = head.rebuilt(&written, Room::Held).expect("room to spare");

        assert_eq!(rebuilt.head.len(), standing.len());
        assert_eq!(comment_length_of(&rebuilt.head), 3_000);
        assert_eq!(rebuilt.renumbered_from, None);
    }

    #[test]
    fn an_ogg_comment_longer_than_its_packet_is_not_held() {
        let head = opus_head_of(&opus_head(3_000));

        assert_eq!(head.rebuilt(&opus_head(3_001), Room::Held), None);
    }

    #[test]
    fn an_ogg_comment_given_generous_room_may_need_another_page_and_says_so() {
        let standing = opus_head(7_000);
        let head = opus_head_of(&standing);

        let rebuilt = head
            .rebuilt(&opus_head(7_100), Room::Generous)
            .expect("a rebuilt head");

        assert_eq!(
            comment_length_of(&rebuilt.head),
            7_100 + PADDING_LEFT_FOR_THE_NEXT_EDIT as usize
        );
        assert_eq!(rebuilt.renumbered_from, Some(3));
        assert_eq!(
            head.rebuilt(&opus_head(100), Room::Generous)
                .map(|rebuilt| rebuilt.renumbered_from),
            Some(Some(3)),
        );
    }

    #[test]
    fn an_ogg_comment_given_whole_blocks_grows_by_exactly_them_inside_the_pages_it_had() {
        let standing = opus_head(500);
        let head = opus_head_of(&standing);

        let rebuilt = head
            .rebuilt(&opus_head(900), Room::Blocks(4096))
            .expect("room within a page");

        assert_eq!(rebuilt.head.len(), standing.len() + 4096);
        assert_eq!(rebuilt.renumbered_from, None);
        assert_eq!(
            ogg::headers(Codec::Opus, &mut Cursor::new(&rebuilt.head))
                .expect("a head")
                .pages,
            2
        );
        assert_eq!(
            head.rebuilt(&opus_head(9_000), Room::Blocks(4096)),
            None,
            "a comment past its page was inserted over"
        );
    }

    #[test]
    fn a_head_given_whole_blocks_grows_by_exactly_them_and_keeps_the_padding_it_would_want() {
        let standing = flac_head(&[block(0, false, &[0; 34]), block(4, true, &[1; 40])]);
        let head = Head {
            padded: Padded::Flac,
            length: standing.len() as u64,
        };
        let written = flac_head(&[block(0, false, &[0; 34]), block(4, true, &[2; 9_000])]);

        let rebuilt = head.rebuilt(&written, Room::Blocks(4096)).expect("blocks");

        let grown = rebuilt.head.len() - standing.len();
        assert!(grown > 0 && grown.is_multiple_of(4096));
        let blocks = blocks_of(&rebuilt.head);
        assert_eq!(blocks.last().map(|block| block.0), Some(PADDING));
        assert!(blocks.last().map_or(0, |block| block.2) as u64 >= PADDING_LEFT_FOR_THE_NEXT_EDIT);

        let tag = Head {
            padded: Padded::Id3,
            length: 0,
        };
        let id3_rebuilt = tag
            .rebuilt(&id3(300, 0, &[b'T'; 300]), Room::Blocks(4096))
            .expect("blocks");
        assert!(id3_rebuilt.head.len().is_multiple_of(4096));
        assert!(id3_rebuilt.head.len() as u64 >= 310 + PADDING_LEFT_FOR_THE_NEXT_EDIT);
    }

    #[test]
    fn a_head_given_generous_room_ends_with_the_padding_the_next_edit_needs() {
        let written = flac_head(&[block(0, false, &[0; 34]), block(4, true, &[2; 600])]);
        let head = Head {
            padded: Padded::Flac,
            length: 100,
        };

        let rebuilt = head.rebuilt(&written, Room::Generous).expect("room");

        assert_eq!(
            rebuilt.head.len() as u64,
            written.len() as u64 + BLOCK_HEADER_BYTES + PADDING_LEFT_FOR_THE_NEXT_EDIT
        );
    }
}
