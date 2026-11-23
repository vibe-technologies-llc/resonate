use std::io::{self, Read, Seek, SeekFrom};

use ring::digest::{Context, SHA256};

use crate::key::hex;

pub const REVISION_BYTES: usize = 32;
const EDGE_BYTES: u64 = 64 * 1024;
const SAMPLES: u64 = 16;
const SAMPLE_BYTES: u64 = 4 * 1024;
const READ_WHOLE_UP_TO: u64 = 2 * EDGE_BYTES + SAMPLES * SAMPLE_BYTES;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct Revision(pub [u8; REVISION_BYTES]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bound {
    Unsaid,
    Carried(Option<Revision>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Revised {
    pub revision: Revision,
    pub from: u64,
}

impl Revision {
    pub fn of<R: Read + Seek + ?Sized>(reader: &mut R, size: u64) -> io::Result<Self> {
        let mut context = Context::new(&SHA256);

        context.update(&size.to_be_bytes());
        for (start, length) in windows(size) {
            let mut window = vec![0; usize::try_from(length).map_err(io::Error::other)?];

            reader.seek(SeekFrom::Start(start))?;
            reader.read_exact(&mut window)?;
            context.update(&window);
        }

        let mut revision = [0; REVISION_BYTES];

        revision.copy_from_slice(context.finish().as_ref());
        Ok(Self(revision))
    }

    pub fn hex(&self) -> String {
        hex(&self.0)
    }

    pub fn from_hex(text: &str) -> Option<Self> {
        let (pairs, []) = text.as_bytes().as_chunks::<2>() else {
            return None;
        };

        if pairs.len() != REVISION_BYTES {
            return None;
        }

        let mut revision = [0; REVISION_BYTES];

        for (byte, [high, low]) in revision.iter_mut().zip(pairs) {
            *byte = (hex_digit(*high)? << 4) | hex_digit(*low)?;
        }
        Some(Self(revision))
    }
}

fn hex_digit(digit: u8) -> Option<u8> {
    char::from(digit)
        .to_digit(16)
        .and_then(|value| u8::try_from(value).ok())
}

fn windows(size: u64) -> Vec<(u64, u64)> {
    if size <= READ_WHOLE_UP_TO {
        return vec![(0, size)];
    }

    let between = size - 2 * EDGE_BYTES - SAMPLE_BYTES;
    let mut windows = vec![(0, EDGE_BYTES)];

    windows.extend(
        (0..SAMPLES).map(|index| (EDGE_BYTES + between * index / (SAMPLES - 1), SAMPLE_BYTES)),
    );
    windows.push((size - EDGE_BYTES, EDGE_BYTES));
    windows
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn revision_of(bytes: &[u8]) -> Revision {
        Revision::of(&mut Cursor::new(bytes), bytes.len() as u64).expect("a revision")
    }

    #[test]
    fn a_revision_reads_back_from_its_hex_and_nothing_else_reads_as_one() {
        let revision = revision_of(b"a song");

        assert_eq!(Revision::from_hex(&revision.hex()), Some(revision));
        assert_eq!(Revision::from_hex("abc"), None);
        assert_eq!(Revision::from_hex(&"zz".repeat(REVISION_BYTES)), None);
    }

    #[test]
    fn the_same_bytes_name_the_same_revision_and_a_change_at_either_end_or_between_another() {
        let song: Vec<u8> = (0..1024 * 1024_u32).map(|at| (at % 251) as u8).collect();

        let near_the_end = song.len() - 10;
        let first_sampled_between = usize::try_from(EDGE_BYTES).expect("a size");
        let mut head = song.clone();
        let mut tail = song.clone();
        let mut between = song.clone();

        head[10] ^= 1;
        tail[near_the_end] ^= 1;
        between[first_sampled_between] ^= 1;

        assert_eq!(revision_of(&song), revision_of(&song.clone()));
        assert_ne!(revision_of(&song), revision_of(&head));
        assert_ne!(revision_of(&song), revision_of(&tail));
        assert_ne!(revision_of(&song), revision_of(&between));
        assert_ne!(revision_of(&song), revision_of(&song[..song.len() - 1]));
    }

    #[test]
    fn a_file_shorter_than_the_size_it_was_offered_at_has_no_revision() {
        let offered = Revision::of(&mut Cursor::new(vec![0; 1000]), 4000);

        assert!(offered.is_err());
    }
}
