#[cfg(unix)]
pub use std::os::unix::fs::FileExt;

#[cfg(windows)]
pub use self::windows::FileExt;

#[cfg(windows)]
mod windows {
    use std::{fs::File, io, os::windows::fs::FileExt as Native};

    pub trait FileExt {
        fn read_at(&self, into: &mut [u8], offset: u64) -> io::Result<usize>;

        fn write_at(&self, from: &[u8], offset: u64) -> io::Result<usize>;

        fn read_exact_at(&self, mut into: &mut [u8], mut offset: u64) -> io::Result<()> {
            while !into.is_empty() {
                match self.read_at(into, offset) {
                    Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                    Ok(read) => {
                        into = into.split_at_mut(read).1;
                        offset += read as u64;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }

        fn write_all_at(&self, mut from: &[u8], mut offset: u64) -> io::Result<()> {
            while !from.is_empty() {
                match self.write_at(from, offset) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(written) => {
                        from = from.split_at(written).1;
                        offset += written as u64;
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(())
        }
    }

    impl FileExt for File {
        fn read_at(&self, into: &mut [u8], offset: u64) -> io::Result<usize> {
            self.seek_read(into, offset)
        }

        fn write_at(&self, from: &[u8], offset: u64) -> io::Result<usize> {
            self.seek_write(from, offset)
        }
    }
}
