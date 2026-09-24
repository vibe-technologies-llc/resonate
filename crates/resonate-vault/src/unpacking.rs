use std::{
    fs::File,
    io::{self, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const RIFF: &[u8; 4] = b"RIFF";
const RIFF_HEADER: u64 = 8;

type Unpacked = zstd::stream::read::Decoder<'static, BufReader<File>>;

pub(crate) struct Unpacking {
    path: PathBuf,
    unpacked: Unpacked,
    unpacked_to: u64,
    at: u64,
    len: u64,
}

impl Unpacking {
    pub(crate) fn open(path: &Path) -> io::Result<Self> {
        let mut unpacked = unpacked(path)?;
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
            unpacked: self::unpacked(path)?,
            unpacked_to: 0,
            at: 0,
            len: u64::from(u32::from_le_bytes(declared)) + RIFF_HEADER,
        })
    }

    fn catch_up(&mut self) -> io::Result<()> {
        if self.at < self.unpacked_to {
            self.unpacked = unpacked(&self.path)?;
            self.unpacked_to = 0;
        }
        let behind = self.at - self.unpacked_to;
        self.unpacked_to += io::copy(&mut (&mut self.unpacked).take(behind), &mut io::sink())?;
        self.at = self.unpacked_to;
        Ok(())
    }
}

fn unpacked(path: &Path) -> io::Result<Unpacked> {
    zstd::stream::read::Decoder::new(File::open(path)?)
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
}
