use std::{
    fs::{self, Metadata, ReadDir},
    io,
    path::Path,
};

pub(crate) fn directory(path: &Path) -> io::Result<ReadDir> {
    #[cfg(feature = "faults")]
    crate::faults::before_listing(path)?;

    fs::read_dir(path)
}

pub(crate) fn metadata(path: &Path) -> io::Result<Metadata> {
    #[cfg(feature = "faults")]
    crate::faults::before_stat(path)?;

    fs::metadata(path)
}
