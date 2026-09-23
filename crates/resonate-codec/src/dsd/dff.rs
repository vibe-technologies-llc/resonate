use std::io::SeekFrom;

use resonate_core::MediaLocation;

use crate::{
    Error, Result,
    dsd::{
        BitOrder, Container, DsdChunk, Edited, Interleave, Layout, MAX_DSD_CHANNELS, channels,
        rate::DsdRate,
    },
    prescan::read_exact,
    source::MediaStream,
    text::decoded,
};

const FORM_HEADER_BYTES: u64 = 12;
const FORM_TYPE_BYTES: u64 = 4;
const CHUNK_HEADER_BYTES: u64 = 12;
const MAX_CHUNKS: usize = 4_096;
const MAX_PROP_BYTES: u64 = 1 << 16;
const MAX_EDITED_TEXT_BYTES: u32 = 1 << 12;
const EDITED_COUNT_BYTES: u64 = 4;

pub(crate) fn read(bytes: &mut dyn MediaStream, location: &MediaLocation) -> Result<Layout> {
    bytes.seek(SeekFrom::Start(0)).map_err(|source| Error::Io {
        location: location.clone(),
        source,
    })?;

    let head = read_exact::<16, _>(bytes).ok_or_else(|| missing(location, DsdChunk::Format))?;
    if !head.starts_with(b"FRM8") || head.get(12..16) != Some(b"DSD ".as_slice()) {
        return Err(missing(location, DsdChunk::Format));
    }
    let form = u64::from_be_bytes(take8(&head, 4));
    let within = FORM_HEADER_BYTES.saturating_add(form.saturating_sub(FORM_TYPE_BYTES));

    let mut found = Found::default();
    let mut at = FORM_HEADER_BYTES + FORM_TYPE_BYTES;

    for _ in 0..MAX_CHUNKS {
        if at >= within {
            break;
        }
        let Some((id, size)) = header(bytes, at) else {
            break;
        };
        let body = at + CHUNK_HEADER_BYTES;

        match &id {
            b"PROP" => read_property(bytes, body, size, &mut found, location)?,
            b"DSD " => {
                found.data_at = Some(body);
                found.data_bytes = Some(size);
            }
            b"ID3 " | b"id3 " => found.metadata_at = Some(body),
            b"DIIN" => read_edited(bytes, body, size, &mut found.edited),
            b"DST " | b"DSTI" => {
                return Err(Error::DsdCompressed {
                    location: location.clone(),
                    compression: id,
                });
            }
            _ => {}
        }

        at = body.saturating_add(size).saturating_add(size % 2);
    }

    let rate = found
        .rate
        .ok_or_else(|| missing(location, DsdChunk::SampleRate))?;
    let count = found
        .channels
        .ok_or_else(|| missing(location, DsdChunk::Channels))?;
    let data_at = found
        .data_at
        .ok_or_else(|| missing(location, DsdChunk::Data))?;
    let declared = found.data_bytes.unwrap_or(0);

    let channels = channels(count, location)?;
    let lanes = u64::from(channels.count().get());
    let held = bytes
        .byte_len()
        .map_or(declared, |len| declared.min(len.saturating_sub(data_at)));
    let data_bytes = held - held % lanes.max(1);

    Ok(Layout {
        container: Container::Dff,
        rate,
        channels,
        samples: data_bytes / lanes * 8,
        data_at,
        data_bytes,
        order: Interleave::PerByte,
        bits: BitOrder::MostSignificantFirst,
        metadata_at: found.metadata_at,
        edited: found.edited,
    })
}

#[derive(Default)]
struct Found {
    rate: Option<DsdRate>,
    channels: Option<u32>,
    data_at: Option<u64>,
    data_bytes: Option<u64>,
    metadata_at: Option<u64>,
    edited: Edited,
}

fn read_edited(bytes: &mut dyn MediaStream, body: u64, size: u64, edited: &mut Edited) {
    let within = body.saturating_add(size.min(MAX_PROP_BYTES));
    let mut at = body;

    for _ in 0..MAX_CHUNKS {
        if at >= within {
            break;
        }
        let Some((id, held)) = header(bytes, at) else {
            break;
        };
        let inner = at + CHUNK_HEADER_BYTES;

        match &id {
            b"DIAR" => edited.artist = edited_text(bytes, held),
            b"DITI" => edited.title = edited_text(bytes, held),
            _ => {}
        }

        at = inner.saturating_add(held).saturating_add(held % 2);
    }
}

fn edited_text(bytes: &mut dyn MediaStream, held: u64) -> Option<String> {
    let count = read_exact::<4, _>(bytes).map(u32::from_be_bytes)?;
    if count > MAX_EDITED_TEXT_BYTES || u64::from(count) > held.saturating_sub(EDITED_COUNT_BYTES) {
        return None;
    }
    let mut text = vec![0; count as usize];
    bytes.read_exact(&mut text).ok()?;
    let (text, _) = decoded(&text);
    let text = text.trim_matches(char::from(0)).trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn read_property(
    bytes: &mut dyn MediaStream,
    body: u64,
    size: u64,
    found: &mut Found,
    location: &MediaLocation,
) -> Result<()> {
    if bytes.seek(SeekFrom::Start(body)).is_err() {
        return Ok(());
    }
    let Some(kind) = read_exact::<4, _>(bytes) else {
        return Ok(());
    };
    if &kind != b"SND " {
        return Ok(());
    }

    let within = body.saturating_add(size.min(MAX_PROP_BYTES));
    let mut at = body + FORM_TYPE_BYTES;

    for _ in 0..MAX_CHUNKS {
        if at >= within {
            break;
        }
        let Some((id, held)) = header(bytes, at) else {
            break;
        };
        let inner = at + CHUNK_HEADER_BYTES;

        match &id {
            b"FS  " => {
                if let Some(hz) = read_exact::<4, _>(bytes).map(u32::from_be_bytes) {
                    found.rate = DsdRate::new(hz);
                    if found.rate.is_none() {
                        return Err(Error::RateNotRepresentable {
                            location: location.clone(),
                            track: crate::StreamTrackId(0),
                            rate: hz,
                        });
                    }
                }
            }
            b"CHNL" => {
                let count = read_exact::<2, _>(bytes)
                    .map(u16::from_be_bytes)
                    .unwrap_or(0);
                found.channels = (count > 0 && count <= MAX_DSD_CHANNELS).then_some(count.into());
            }
            b"CMPR" => {
                if let Some(kind) = read_exact::<4, _>(bytes)
                    && &kind != b"DSD "
                {
                    return Err(Error::DsdCompressed {
                        location: location.clone(),
                        compression: kind,
                    });
                }
            }
            _ => {}
        }

        at = inner.saturating_add(held).saturating_add(held % 2);
    }
    Ok(())
}

fn header(bytes: &mut dyn MediaStream, at: u64) -> Option<([u8; 4], u64)> {
    bytes.seek(SeekFrom::Start(at)).ok()?;
    let held = read_exact::<12, _>(bytes)?;
    let id: [u8; 4] = held.get(..4)?.try_into().ok()?;
    Some((id, u64::from_be_bytes(take8(&held, 4))))
}

fn take8(held: &[u8], at: usize) -> [u8; 8] {
    held.get(at..at + 8)
        .and_then(|slice| slice.try_into().ok())
        .unwrap_or([0; 8])
}

fn missing(location: &MediaLocation, chunk: DsdChunk) -> Error {
    Error::DsdChunkMissing {
        location: location.clone(),
        chunk,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{dsd::tags, source::Reading};

    const DSD64: u32 = 2_822_400;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(body.len() as u64).to_be_bytes());
        held.extend_from_slice(body);
        if body.len() % 2 == 1 {
            held.push(0);
        }
        held
    }

    fn counted(text: &str) -> Vec<u8> {
        let mut held = (text.len() as u32).to_be_bytes().to_vec();
        held.extend_from_slice(text.as_bytes());
        held
    }

    fn id3_titled(title: &str) -> Vec<u8> {
        let mut frame = b"TIT2".to_vec();
        frame.extend_from_slice(&(title.len() as u32 + 1).to_be_bytes());
        frame.extend_from_slice(&[0, 0, 0]);
        frame.extend_from_slice(title.as_bytes());

        let size = frame.len() as u32;
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&[
            ((size >> 21) & 0x7F) as u8,
            ((size >> 14) & 0x7F) as u8,
            ((size >> 7) & 0x7F) as u8,
            (size & 0x7F) as u8,
        ]);
        tag.extend_from_slice(&frame);
        tag
    }

    fn dff(trailing: &[Vec<u8>]) -> Vec<u8> {
        let mut sound = b"SND ".to_vec();
        sound.extend(chunk(b"FS  ", &DSD64.to_be_bytes()));
        sound.extend(chunk(
            b"CHNL",
            &[0, 2, b'S', b'L', b'F', b'T', b'S', b'R', b'G', b'T'],
        ));
        sound.extend(chunk(b"CMPR", b"DSD \x0enot compressed\x00"));

        let mut form = b"DSD ".to_vec();
        form.extend(chunk(b"FVER", &[1, 5, 0, 0]));
        form.extend(chunk(b"PROP", &sound));
        form.extend(chunk(b"DSD ", &[0x69; 64]));
        for held in trailing {
            form.extend_from_slice(held);
        }
        chunk(b"FRM8", &form)
    }

    fn tags_of(bytes: Vec<u8>) -> crate::TagSet {
        let mut stream = Reading::new(Cursor::new(bytes));
        let layout = read(&mut stream, &MediaLocation::local("/music/a.dff")).expect("a dff reads");
        tags(&mut stream, &layout)
    }

    #[test]
    fn an_id3_chunk_after_the_sound_names_the_track() {
        let tags = tags_of(dff(&[chunk(b"ID3 ", &id3_titled("Pulse"))]));

        assert_eq!(tags.title.as_deref(), Some("Pulse"));
    }

    #[test]
    fn the_edited_master_names_what_no_id3_chunk_does() {
        let mut edited = chunk(b"DIAR", &counted("The Artist"));
        edited.extend(chunk(b"DITI", &counted("The Title")));

        let tags = tags_of(dff(&[
            chunk(b"DIIN", &edited),
            chunk(b"ID3 ", &id3_titled("Tagged")),
        ]));

        assert_eq!(tags.title.as_deref(), Some("Tagged"));
        assert_eq!(tags.artist.as_deref(), Some("The Artist"));
    }

    #[test]
    fn an_edited_text_longer_than_its_chunk_is_passed_over() {
        let mut lying = 4_000_u32.to_be_bytes().to_vec();
        lying.extend_from_slice(b"short");

        let tags = tags_of(dff(&[chunk(b"DIIN", &chunk(b"DITI", &lying))]));

        assert_eq!(tags.title, None);
    }
}
