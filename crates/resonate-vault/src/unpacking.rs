use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const RIFF: &[u8; 4] = b"RIFF";
const RIFF_HEADER: u64 = 8;
const FRAME_MAGIC: u32 = 0xFD2F_B528;
const SKIPPABLE_MAGIC: u32 = 0x184D_2A50;
const SKIPPABLE_KINDS: u32 = 0xF;
const MAGIC_BYTES: u64 = 4;
const SKIPPABLE_SIZE_BYTES: u64 = 4;
const DESCRIPTOR_BYTES: u64 = 1;
const WINDOW_BYTES: u64 = 1;
const BLOCK_HEADER_BYTES: usize = 3;
const DICTIONARY_ID_BYTES: [u64; 4] = [0, 1, 2, 4];
const CHECKSUM_BYTES: u64 = 4;
const SINGLE_SEGMENT: u8 = 0x20;
const CHECKSUMMED: u8 = 0x04;
const DICTIONARY_FLAG: u8 = 0x03;
const CONTENT_FLAG_AT: u8 = 6;
const TWO_BYTE_CONTENT_OFFSET: u64 = 256;
const LAST_BLOCK: u32 = 1;
const RLE_BLOCK: u32 = 1;
const RESERVED_BLOCK: u32 = 3;
const FAR_AHEAD: u64 = 4 << 20;

type Unpacked = zstd::stream::read::Decoder<'static, BufReader<File>>;

pub(crate) struct Unpacking {
    path: PathBuf,
    unpacked: Unpacked,
    unpacked_to: u64,
    at: u64,
    len: u64,
    index: Index,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Framed {
    pub(crate) packed_at: u64,
    pub(crate) unpacked_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Index {
    Unread,
    Held(Vec<Framed>),
    Unheld,
}

impl Unpacking {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut unpacked = unpacked_from(path, 0)?;
        let mut head = [0_u8; RIFF_HEADER as usize];
        unpacked.read_exact(&mut head)?;
        let (named, declared) = head.split_at(RIFF.len());
        if named != RIFF {
            return Err(io::Error::from(io::ErrorKind::InvalidData));
        }
        let declared: [u8; 4] = declared
            .try_into()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;

        Ok(Self {
            path: path.to_path_buf(),
            unpacked: unpacked_from(path, 0)?,
            unpacked_to: 0,
            at: 0,
            len: u64::from(u32::from_le_bytes(declared)) + RIFF_HEADER,
            index: Index::Unread,
        })
    }

    fn catch_up(&mut self) -> io::Result<()> {
        let behind = self.at < self.unpacked_to;
        let far_ahead = self.at.saturating_sub(self.unpacked_to) > FAR_AHEAD;
        if behind || far_ahead {
            match self.frame_holding(self.at) {
                Some(framed) if behind || framed.unpacked_at > self.unpacked_to => {
                    self.unpacked = unpacked_from(&self.path, framed.packed_at)?;
                    self.unpacked_to = framed.unpacked_at;
                }
                Some(_) => {}
                None if behind => {
                    self.unpacked = unpacked_from(&self.path, 0)?;
                    self.unpacked_to = 0;
                }
                None => {}
            }
        }
        let behind = self.at - self.unpacked_to;
        self.unpacked_to += io::copy(&mut (&mut self.unpacked).take(behind), &mut io::sink())?;
        self.at = self.unpacked_to;
        Ok(())
    }

    fn frame_holding(&mut self, at: u64) -> Option<Framed> {
        if self.index == Index::Unread {
            self.index = frames_of(&self.path)
                .ok()
                .flatten()
                .map_or(Index::Unheld, Index::Held);
        }
        let Index::Held(framed) = &self.index else {
            return None;
        };
        let after = framed.partition_point(|framed| framed.unpacked_at <= at);
        after.checked_sub(1).and_then(|at| framed.get(at)).copied()
    }
}

fn unpacked_from(path: &Path, packed_at: u64) -> io::Result<Unpacked> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(packed_at))?;
    zstd::stream::read::Decoder::new(file)
}

struct Walking {
    file: BufReader<File>,
    at: u64,
}

impl Walking {
    fn to(&mut self, at: u64) -> io::Result<()> {
        let by = i64::try_from(at).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?
            - i64::try_from(self.at).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        self.file.seek_relative(by)?;
        self.at = at;
        Ok(())
    }

    fn read<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut bytes = [0_u8; N];
        self.file.read_exact(&mut bytes)?;
        self.at += N as u64;
        Ok(bytes)
    }

    fn little_endian(&mut self, bytes: u64) -> io::Result<u64> {
        let mut value = 0_u64;
        for shift in 0..bytes {
            let [byte] = self.read::<1>()?;
            value |= u64::from(byte) << (8 * shift);
        }
        Ok(value)
    }
}

pub(crate) fn frames_of(path: &Path) -> io::Result<Option<Vec<Framed>>> {
    let file = File::open(path)?;
    let length = file.metadata()?.len();
    let mut walking = Walking {
        file: BufReader::new(file),
        at: 0,
    };
    let mut framed = Vec::new();
    let mut unpacked_at = 0_u64;

    while walking.at < length {
        let packed_at = walking.at;
        let magic = u32::from_le_bytes(walking.read::<4>()?);
        if magic & !SKIPPABLE_KINDS == SKIPPABLE_MAGIC {
            let size = walking.little_endian(SKIPPABLE_SIZE_BYTES)?;
            walking.to(packed_at + MAGIC_BYTES + SKIPPABLE_SIZE_BYTES + size)?;
            continue;
        }
        if magic != FRAME_MAGIC {
            return Ok(None);
        }

        let [descriptor] = walking.read::<1>()?;
        let single_segment = descriptor & SINGLE_SEGMENT != 0;
        let dictionary_bytes = DICTIONARY_ID_BYTES[usize::from(descriptor & DICTIONARY_FLAG)];
        let content_bytes = match (descriptor >> CONTENT_FLAG_AT, single_segment) {
            (0, false) => return Ok(None),
            (0, true) => 1,
            (1, _) => 2,
            (2, _) => 4,
            _ => 8,
        };
        let window_bytes = if single_segment { 0 } else { WINDOW_BYTES };
        walking.to(packed_at + MAGIC_BYTES + DESCRIPTOR_BYTES + window_bytes + dictionary_bytes)?;
        let mut content = walking.little_endian(content_bytes)?;
        if content_bytes == 2 {
            content += TWO_BYTE_CONTENT_OFFSET;
        }

        loop {
            let [low, middle, high] = walking.read::<BLOCK_HEADER_BYTES>()?;
            let header = u32::from_le_bytes([low, middle, high, 0]);
            let kind = (header >> 1) & 0b11;
            let carried = match kind {
                RESERVED_BLOCK => return Ok(None),
                RLE_BLOCK => 1,
                _ => u64::from(header >> 3),
            };
            walking.to(walking.at + carried)?;
            if header & LAST_BLOCK != 0 {
                break;
            }
        }
        if descriptor & CHECKSUMMED != 0 {
            walking.to(walking.at + CHECKSUM_BYTES)?;
        }

        framed.push(Framed {
            packed_at,
            unpacked_at,
        });
        unpacked_at += content;
    }
    Ok(Some(framed))
}

impl Read for Unpacking {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if self.at >= self.len {
            return Ok(0);
        }
        self.catch_up()?;
        let read = self.unpacked.read(into)?;
        self.unpacked_to += read as u64;
        self.at = self.unpacked_to;
        Ok(read)
    }
}

impl Seek for Unpacking {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(by) => self.len.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        }
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        self.at = target;
        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use super::*;

    #[test]
    fn a_packed_wave_reads_and_seeks_both_ways_like_the_bytes_it_holds() {
        let folder = env::temp_dir().join(format!("resonate-unpacking-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let path = folder.join("held.wav.zst");

        let body: Vec<u8> = (0..300_000_u32).map(|at| (at % 251) as u8).collect();
        let mut whole = RIFF.to_vec();
        whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
        whole.extend_from_slice(&body);
        fs::write(
            &path,
            zstd::encode_all(whole.as_slice(), 3).expect("packed"),
        )
        .expect("written");

        let mut reading = Unpacking::open(&path).expect("an unpacking");
        assert_eq!(
            reading.seek(SeekFrom::End(0)).expect("the end"),
            whole.len() as u64
        );

        for at in [200_000_u64, 10, 150_000, 0] {
            reading.seek(SeekFrom::Start(at)).expect("a seek");
            let mut read = [0_u8; 64];
            reading.read_exact(&mut read).expect("a read");
            assert_eq!(&read[..], &whole[at as usize..at as usize + 64]);
        }
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn measuring_a_packed_wave_unpacks_none_of_it() {
        let folder = env::temp_dir().join(format!("resonate-measuring-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let path = folder.join("held.wav.zst");

        let body = vec![7_u8; 1 << 20];
        let mut whole = RIFF.to_vec();
        whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
        whole.extend_from_slice(&body);
        fs::write(
            &path,
            zstd::encode_all(whole.as_slice(), 3).expect("packed"),
        )
        .expect("written");

        let mut reading = Unpacking::open(&path).expect("an unpacking");
        assert_eq!(
            reading.seek(SeekFrom::End(0)).expect("the end"),
            whole.len() as u64
        );
        assert_eq!(reading.read(&mut [0_u8; 16]).expect("a read at the end"), 0);
        assert_eq!(reading.seek(SeekFrom::Start(0)).expect("the start"), 0);
        assert_eq!(
            reading.unpacked_to, 0,
            "a seek there and back unpacked the object"
        );

        let mut head = [0_u8; 4];
        reading.read_exact(&mut head).expect("a read");
        assert_eq!(&head, RIFF);
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_wave_packed_in_frames_is_read_back_from_the_frame_a_seek_lands_in() {
        const FRAME_BYTES: u64 = 64 << 10;

        let folder = env::temp_dir().join(format!("resonate-unpacking-framed-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let from = folder.join("held.wav");
        let path = folder.join("held.wav.zst");

        let body: Vec<u8> = (0..300_000_u32).map(|at| (at % 251) as u8).collect();
        let mut whole = RIFF.to_vec();
        whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
        whole.extend_from_slice(&body);
        fs::write(&from, &whole).expect("written");
        crate::wave::compressed_in_frames(&from, &path, None, FRAME_BYTES, crate::Halt::NEVER)
            .expect("packed");

        let framed = frames_of(&path)
            .expect("a walk")
            .expect("frames it can name");
        let starts: Vec<u64> = framed.iter().map(|framed| framed.unpacked_at).collect();
        let mut reading = Unpacking::open(&path).expect("an unpacking");
        reading.seek(SeekFrom::Start(290_000)).expect("a seek");
        let mut late = [0_u8; 64];
        reading.read_exact(&mut late).expect("a read");
        reading.seek(SeekFrom::Start(200_000)).expect("a seek back");
        let mut earlier = [0_u8; 64];
        reading.read_exact(&mut earlier).expect("a read");
        let held = reading.frame_holding(200_000);
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(
            starts,
            (0..whole.len() as u64)
                .step_by(FRAME_BYTES as usize)
                .collect::<Vec<_>>()
        );
        assert_eq!(&late[..], &whole[290_000..290_064]);
        assert_eq!(&earlier[..], &whole[200_000..200_064]);
        assert_eq!(held.map(|held| held.unpacked_at), Some(3 * FRAME_BYTES));
        assert!(held.is_some_and(|held| held.packed_at > 0));
    }

    #[test]
    fn a_wave_packed_as_one_frame_naming_no_length_is_read_back_from_its_start() {
        let folder = env::temp_dir().join(format!("resonate-unpacking-whole-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        let path = folder.join("held.wav.zst");

        let mut whole = RIFF.to_vec();
        whole.extend_from_slice(&(1_u32 << 16).to_le_bytes());
        whole.extend_from_slice(&[9_u8; 1 << 16]);
        fs::write(
            &path,
            zstd::encode_all(whole.as_slice(), 3).expect("packed"),
        )
        .expect("written");

        let framed = frames_of(&path).expect("a walk");
        let _ = fs::remove_dir_all(&folder);

        assert_eq!(framed, None);
    }
}
