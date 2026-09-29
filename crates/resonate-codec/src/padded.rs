use std::io::{Cursor, Read, Seek};

use lofty::file::FileType;

use crate::prescan::{past_id3, read_exact};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Padded {
    Flac,
    Id3,
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
            _ => return None,
        };
        let length = match padded {
            Padded::Flac => flac_metadata_end(source)?,
            Padded::Id3 => past_id3(source).filter(|end| *end > 0)?,
        };
        Some(Self { padded, length })
    }

    pub(crate) fn absorbed(self, written: &[u8]) -> Option<Vec<u8>> {
        match self.padded {
            Padded::Flac => flac_absorbed(written, self.length),
            Padded::Id3 => id3_absorbed(written, self.length),
        }
    }
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

fn flac_absorbed(written: &[u8], length: u64) -> Option<Vec<u8>> {
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

    let content = body as u64 + kept.iter().map(|block| block.len() as u64).sum::<u64>();
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

    let content = written[ID3_HEADER_BYTES..]
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(ID3_HEADER_BYTES, |last| ID3_HEADER_BYTES + last + 1);
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
}
