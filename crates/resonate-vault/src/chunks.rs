use std::{
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
};

use resonate_codec::MediaStream;

const CHUNKS_AT_MOST: usize = 4096;
const ANYTHING_PAST: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Layout {
    Riff,
    Aiff,
    Caf,
    Dsdiff,
}

impl Layout {
    const fn opening(self) -> &'static [u8; 4] {
        match self {
            Self::Riff => b"RIFF",
            Self::Aiff => b"FORM",
            Self::Caf => b"caff",
            Self::Dsdiff => b"FRM8",
        }
    }

    const fn head_bytes(self) -> u64 {
        match self {
            Self::Riff | Self::Aiff => 12,
            Self::Caf => 8,
            Self::Dsdiff => 16,
        }
    }

    const fn chunk_header_bytes(self) -> u64 {
        match self {
            Self::Riff | Self::Aiff => 8,
            Self::Caf | Self::Dsdiff => 12,
        }
    }

    const fn pads_to_even(self) -> bool {
        !matches!(self, Self::Caf)
    }

    fn kinds_it_holds(self, head: &[u8; 16]) -> bool {
        match self {
            Self::Riff => &head[8..12] == b"WAVE",
            Self::Aiff => matches!(&head[8..12], b"AIFF" | b"AIFC"),
            Self::Caf => head[4..6] == [0, 1],
            Self::Dsdiff => &head[12..16] == b"DSD ",
        }
    }

    fn declared_end(self, head: &[u8; 16]) -> Option<u64> {
        match self {
            Self::Riff => Some(u64::from(u32::from_le_bytes(head[4..8].try_into().ok()?)) + 8),
            Self::Aiff => Some(u64::from(u32::from_be_bytes(head[4..8].try_into().ok()?)) + 8),
            Self::Caf => None,
            Self::Dsdiff => u64::from_be_bytes(head[4..12].try_into().ok()?).checked_add(12),
        }
    }

    fn with_end(self, head: &[u8; 16], end: u64) -> Option<Vec<u8>> {
        let mut rewritten = head[..self.head_bytes() as usize].to_vec();
        match self {
            Self::Riff => {
                rewritten[4..8].copy_from_slice(&u32::try_from(end - 8).ok()?.to_le_bytes());
            }
            Self::Aiff => {
                rewritten[4..8].copy_from_slice(&u32::try_from(end - 8).ok()?.to_be_bytes());
            }
            Self::Caf => {}
            Self::Dsdiff => rewritten[4..12].copy_from_slice(&(end - 12).to_be_bytes()),
        }
        Some(rewritten)
    }

    fn sheds(self, id: &[u8; 4]) -> bool {
        match self {
            Self::Riff => matches!(
                id,
                b"LIST" | b"id3 " | b"ID3 " | b"bext" | b"iXML" | b"_PMX" | b"axml"
            ),
            Self::Aiff => matches!(
                id,
                b"NAME" | b"AUTH" | b"(c) " | b"ANNO" | b"COMT" | b"ID3 " | b"id3 "
            ),
            Self::Caf => matches!(id, b"info"),
            Self::Dsdiff => matches!(id, b"ID3 " | b"DIIN" | b"COMT"),
        }
    }

    fn declared_size(self, header: &[u8; 12]) -> Option<Declared> {
        match self {
            Self::Riff => Some(Declared::Bytes(u64::from(u32::from_le_bytes(
                header[4..8].try_into().ok()?,
            )))),
            Self::Aiff => Some(Declared::Bytes(u64::from(u32::from_be_bytes(
                header[4..8].try_into().ok()?,
            )))),
            Self::Caf => {
                let size = i64::from_be_bytes(header[4..12].try_into().ok()?);
                match u64::try_from(size) {
                    Ok(size) => Some(Declared::Bytes(size)),
                    Err(_) if size == -1 => Some(Declared::ToTheEnd),
                    Err(_) => None,
                }
            }
            Self::Dsdiff => Some(Declared::Bytes(u64::from_be_bytes(
                header[4..12].try_into().ok()?,
            ))),
        }
    }
}

enum Declared {
    Bytes(u64),
    ToTheEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shed {
    pub(crate) head: Vec<u8>,
    pub(crate) from: u64,
    pub(crate) left_out: Vec<Range<u64>>,
}

pub(crate) fn shed(
    stream: &mut Box<dyn MediaStream>,
    layout: Layout,
    at: u64,
    length: u64,
) -> Option<Shed> {
    let mut head = [0_u8; 16];
    stream.seek(SeekFrom::Start(at)).ok()?;
    stream.read_exact(&mut head).ok()?;
    if &head[..4] != layout.opening() || !layout.kinds_it_holds(&head) {
        return None;
    }

    let from = at.checked_add(layout.head_bytes())?;
    let end = match layout.declared_end(&head) {
        Some(declared) => at.checked_add(declared)?.min(length),
        None => length,
    };

    let mut left_out = Vec::new();
    let mut shed_within = 0_u64;
    let mut walked = from;
    for _ in 0..CHUNKS_AT_MOST {
        if walked >= end {
            break;
        }
        let header_end = walked.checked_add(layout.chunk_header_bytes())?;
        if header_end > end {
            return None;
        }
        let mut header = [0_u8; 12];
        stream.seek(SeekFrom::Start(walked)).ok()?;
        stream
            .read_exact(&mut header[..layout.chunk_header_bytes() as usize])
            .ok()?;
        let id: [u8; 4] = header[..4].try_into().ok()?;
        let past = match layout.declared_size(&header)? {
            Declared::Bytes(size) => {
                let padded = if layout.pads_to_even() {
                    size.checked_add(size & 1)?
                } else {
                    size
                };
                header_end.checked_add(padded)?.min(end)
            }
            Declared::ToTheEnd => end,
        };
        if layout.sheds(&id) {
            left_out.push(walked..past);
            shed_within += past - walked;
        }
        walked = past;
    }
    if walked < end {
        return None;
    }

    if end < length {
        left_out.push(end..ANYTHING_PAST);
    }
    if left_out.is_empty() && at == 0 {
        return None;
    }

    let kept_end = end.checked_sub(at)?.checked_sub(shed_within)?;
    Some(Shed {
        head: layout.with_end(&head, kept_end)?,
        from,
        left_out,
    })
}

pub(crate) struct Passing<R> {
    inner: R,
    at: u64,
    left_out: Vec<Range<u64>>,
    next: usize,
}

impl<R: Read + Seek> Passing<R> {
    pub(crate) const fn over(inner: R, at: u64, left_out: Vec<Range<u64>>) -> Self {
        Self {
            inner,
            at,
            left_out,
            next: 0,
        }
    }
}

impl<R: Read + Seek> Read for Passing<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        while let Some(range) = self.left_out.get(self.next) {
            if self.at < range.start {
                break;
            }
            if range.end == ANYTHING_PAST {
                return Ok(0);
            }
            self.at = self.at.max(range.end);
            self.inner.seek(SeekFrom::Start(self.at))?;
            self.next += 1;
        }

        let room = match self.left_out.get(self.next) {
            Some(range) => usize::try_from(range.start - self.at).unwrap_or(usize::MAX),
            None => usize::MAX,
        };
        let wanted = buffer.len().min(room);
        let Some(within) = buffer.get_mut(..wanted) else {
            return Ok(0);
        };
        let read = self.inner.read(within)?;
        self.at += read as u64;
        Ok(read)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_codec::Reading;

    use super::*;

    fn chunk_le(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        held.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            held.push(0);
        }
        held
    }

    fn chunk_be(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        held.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            held.push(0);
        }
        held
    }

    fn chunk_caf(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(payload.len() as i64).to_be_bytes());
        held.extend_from_slice(payload);
        held
    }

    fn chunk_dff(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(payload.len() as u64).to_be_bytes());
        held.extend_from_slice(payload);
        if payload.len() % 2 == 1 {
            held.push(0);
        }
        held
    }

    fn riff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut whole = b"RIFF".to_vec();
        whole.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
        whole.extend_from_slice(b"WAVE");
        whole.extend_from_slice(&body);
        whole
    }

    fn aiff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut whole = b"FORM".to_vec();
        whole.extend_from_slice(&(body.len() as u32 + 4).to_be_bytes());
        whole.extend_from_slice(b"AIFF");
        whole.extend_from_slice(&body);
        whole
    }

    fn caf(chunks: &[Vec<u8>]) -> Vec<u8> {
        let mut whole = b"caff".to_vec();
        whole.extend_from_slice(&[0, 1, 0, 0]);
        whole.extend_from_slice(&chunks.concat());
        whole
    }

    fn dsdiff(chunks: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = chunks.concat();
        let mut whole = b"FRM8".to_vec();
        whole.extend_from_slice(&(body.len() as u64 + 4).to_be_bytes());
        whole.extend_from_slice(b"DSD ");
        whole.extend_from_slice(&body);
        whole
    }

    fn written(whole: Vec<u8>, layout: Layout) -> Option<Vec<u8>> {
        let length = whole.len() as u64;
        let mut stream: Box<dyn MediaStream> = Box::new(Reading::new(Cursor::new(whole)));
        let shed = shed(&mut stream, layout, 0, length)?;
        stream.seek(SeekFrom::Start(shed.from)).expect("a seek");
        let mut rest = Vec::new();
        Passing::over(&mut stream, shed.from, shed.left_out)
            .read_to_end(&mut rest)
            .expect("the rest");
        Some([shed.head, rest].concat())
    }

    #[test]
    fn a_wave_sheds_its_list_and_id3_chunks_and_says_how_long_it_now_is() {
        let fmt = chunk_le(b"fmt ", &[1; 16]);
        let data = chunk_le(b"data", &[7; 9]);
        let whole = riff(&[
            fmt.clone(),
            chunk_le(b"LIST", b"INFOINAM\x05\0\0\0title"),
            data.clone(),
            chunk_le(b"id3 ", b"ID3 a tag"),
        ]);

        assert_eq!(written(whole, Layout::Riff), Some(riff(&[fmt, data])),);
    }

    #[test]
    fn an_aiff_sheds_its_text_chunks() {
        let comm = chunk_be(b"COMM", &[2; 18]);
        let ssnd = chunk_be(b"SSND", &[3; 21]);
        let whole = aiff(&[
            chunk_be(b"NAME", b"a name"),
            comm.clone(),
            chunk_be(b"ANNO", b"a note"),
            ssnd.clone(),
            chunk_be(b"ID3 ", b"ID3 more"),
        ]);

        assert_eq!(written(whole, Layout::Aiff), Some(aiff(&[comm, ssnd])));
    }

    #[test]
    fn a_caf_sheds_its_info_chunk_and_reads_a_data_chunk_to_the_end() {
        let desc = chunk_caf(b"desc", &[4; 32]);
        let mut data = b"data".to_vec();
        data.extend_from_slice(&(-1_i64).to_be_bytes());
        data.extend_from_slice(&[5; 13]);
        let whole = caf(&[
            desc.clone(),
            chunk_caf(b"info", b"\0\0\0\x01title\0x\0"),
            data.clone(),
        ]);

        assert_eq!(written(whole, Layout::Caf), Some(caf(&[desc, data])));
    }

    #[test]
    fn a_dsdiff_sheds_its_edited_master_and_id3_chunks() {
        let fver = chunk_dff(b"FVER", &[1, 5, 0, 0]);
        let prop = chunk_dff(b"PROP", &[6; 30]);
        let dsd = chunk_dff(b"DSD ", &[0x69; 17]);
        let whole = dsdiff(&[
            fver.clone(),
            prop.clone(),
            chunk_dff(b"DIIN", b"DITI an edit"),
            dsd.clone(),
            chunk_dff(b"ID3 ", b"ID3 a tag"),
        ]);

        assert_eq!(
            written(whole, Layout::Dsdiff),
            Some(dsdiff(&[fver, prop, dsd]))
        );
    }

    #[test]
    fn bytes_past_the_declared_end_are_left_behind() {
        let fmt = chunk_le(b"fmt ", &[1; 16]);
        let data = chunk_le(b"data", &[7; 8]);
        let mut whole = riff(&[fmt.clone(), data.clone()]);
        whole.extend_from_slice(b"TAG and the rest of an id3v1 tag");

        assert_eq!(written(whole, Layout::Riff), Some(riff(&[fmt, data])));
    }

    #[test]
    fn a_file_with_nothing_to_shed_or_a_chunk_that_runs_past_its_end_is_left_whole() {
        let fmt = chunk_le(b"fmt ", &[1; 16]);
        let data = chunk_le(b"data", &[7; 8]);
        assert_eq!(written(riff(&[fmt.clone(), data]), Layout::Riff), None);

        let mut broken = chunk_le(b"data", &[7; 8]);
        broken[4..8].copy_from_slice(&1000_u32.to_le_bytes());
        let mut whole = riff(&[fmt, broken]);
        whole.extend_from_slice(&chunk_le(b"LIST", b"INFO"));
        let declared = (whole.len() - 8) as u32;
        whole[4..8].copy_from_slice(&declared.to_le_bytes());
        assert_eq!(written(whole, Layout::Riff), None);
    }
}
