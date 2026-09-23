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
            at: 0,
            len: u64::from(u32::from_le_bytes(declared)) + RIFF_HEADER,
        })
    }

    fn skip(&mut self, bytes: u64) -> io::Result<()> {
        let skipped = io::copy(&mut (&mut self.unpacked).take(bytes), &mut io::sink())?;
        self.at += skipped;
        Ok(())
    }
}

fn unpacked(path: &Path) -> io::Result<Unpacked> {
    zstd::stream::read::Decoder::new(File::open(path)?)
}

impl Read for Unpacking {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let read = self.unpacked.read(into)?;
        self.at += read as u64;
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

        if target < self.at {
            self.unpacked = unpacked(&self.path)?;
            self.at = 0;
        }
        self.skip(target - self.at)?;
        Ok(self.at)
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
}
