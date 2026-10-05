use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::FileExt as _,
    path::Path,
};

use lofty::io::{Length, Truncate};

use crate::journal::{Change, Undo};

const PAGE_BYTES: u64 = 4096;
pub(crate) const MOST_BYTES_WRITTEN_IN_PLACE: u64 = 32 << 20;
const MOST_PAGES: usize = (MOST_BYTES_WRITTEN_IN_PLACE / PAGE_BYTES) as usize;

type Page = Box<[u8; PAGE_BYTES as usize]>;

pub(crate) struct Overlay<'f> {
    base: &'f File,
    stood: u64,
    readable: u64,
    length: u64,
    at: u64,
    pages: BTreeMap<u64, Page>,
    outgrown: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Landing {
    InPlace,
    Unchanged,
    TooWide,
}

impl<'f> Overlay<'f> {
    pub(crate) fn over(base: &'f File) -> io::Result<Self> {
        let stood = base.metadata()?.len();
        Ok(Self {
            base,
            stood,
            readable: stood,
            length: stood,
            at: 0,
            pages: BTreeMap::new(),
            outgrown: false,
        })
    }

    pub(crate) const fn outgrown(&self) -> bool {
        self.outgrown
    }

    pub(crate) fn land(&self, track: &Path) -> io::Result<Landing> {
        if self.outgrown || self.length != self.stood {
            return Ok(Landing::TooWide);
        }

        let mut changed = Vec::new();
        let mut held = [0_u8; PAGE_BYTES as usize];
        for (&page, bytes) in &self.pages {
            let start = page * PAGE_BYTES;
            let within = usize::try_from(self.length.saturating_sub(start).min(PAGE_BYTES))
                .unwrap_or(PAGE_BYTES as usize);
            self.base.read_exact_at(&mut held[..within], start)?;
            if held[..within] != bytes[..within] {
                changed.push(Change {
                    start,
                    before: held[..within].to_vec(),
                    after: &bytes[..within],
                });
            }
        }
        if changed.is_empty() {
            return Ok(Landing::Unchanged);
        }

        let undo = Undo::kept_beside(track, self.stood, &changed)
            .inspect_err(|error| {
                tracing::debug!(%error, path = %track.display(), "no undo could be kept beside the track, so its tag lands in place without one");
            })
            .ok();
        let landed = changed
            .iter()
            .try_for_each(|change| self.base.write_all_at(change.after, change.start))
            .and_then(|()| self.base.sync_data());
        match (landed, undo) {
            (Ok(()), Some(undo)) => undo.done(),
            (Ok(()), None) => {}
            (Err(error), Some(undo)) => {
                undo.roll_back(self.base, &changed);
                return Err(error);
            }
            (Err(error), None) => return Err(error),
        }
        Ok(Landing::InPlace)
    }

    fn page(&mut self, page: u64) -> io::Result<&mut Page> {
        if !self.pages.contains_key(&page) {
            if self.pages.len() >= MOST_PAGES {
                self.outgrown = true;
                return Err(io::Error::from(io::ErrorKind::FileTooLarge));
            }
            let mut bytes: Page = Box::new([0; PAGE_BYTES as usize]);
            let start = page * PAGE_BYTES;
            let within = self.readable.saturating_sub(start).min(PAGE_BYTES);
            if within > 0 {
                let within = usize::try_from(within).unwrap_or(PAGE_BYTES as usize);
                self.base.read_exact_at(&mut bytes[..within], start)?;
            }
            self.pages.insert(page, bytes);
        }
        Ok(self
            .pages
            .get_mut(&page)
            .expect("a page this call has just put there"))
    }

    fn read_at(&self, into: &mut [u8], start: u64) -> io::Result<()> {
        let page = start / PAGE_BYTES;
        let offset = usize::try_from(start % PAGE_BYTES).unwrap_or_default();
        match self.pages.get(&page) {
            Some(bytes) => into.copy_from_slice(&bytes[offset..offset + into.len()]),
            None => {
                let readable = usize::try_from(self.readable.saturating_sub(start))
                    .unwrap_or(usize::MAX)
                    .min(into.len());
                self.base.read_exact_at(&mut into[..readable], start)?;
                into[readable..].fill(0);
            }
        }
        Ok(())
    }
}

impl Read for Overlay<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.length.saturating_sub(self.at);
        let within_page = PAGE_BYTES - self.at % PAGE_BYTES;
        let wanted = usize::try_from(left.min(within_page))
            .unwrap_or(usize::MAX)
            .min(buf.len());
        if wanted == 0 {
            return Ok(0);
        }

        self.read_at(&mut buf[..wanted], self.at)?;
        self.at += wanted as u64;
        Ok(wanted)
    }
}

impl Write for Overlay<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let at = self.at;
        let offset = usize::try_from(at % PAGE_BYTES).unwrap_or_default();
        let wanted = buf.len().min(PAGE_BYTES as usize - offset);

        let page = self.page(at / PAGE_BYTES)?;
        page[offset..offset + wanted].copy_from_slice(&buf[..wanted]);
        self.at += wanted as u64;
        self.length = self.length.max(self.at);
        Ok(wanted)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Seek for Overlay<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let to = match pos {
            SeekFrom::Start(to) => Some(to),
            SeekFrom::End(by) => self.length.checked_add_signed(by),
            SeekFrom::Current(by) => self.at.checked_add_signed(by),
        };
        self.at = to.ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        Ok(self.at)
    }
}

impl Truncate for Overlay<'_> {
    fn truncate(&mut self, new_len: u64) -> io::Result<()> {
        if new_len < self.length {
            let kept = new_len.div_ceil(PAGE_BYTES);
            self.pages.split_off(&kept);
            let offset = usize::try_from(new_len % PAGE_BYTES).unwrap_or_default();
            if offset > 0
                && let Some(last) = self.pages.get_mut(&(new_len / PAGE_BYTES))
            {
                last[offset..].fill(0);
            }
            self.readable = self.readable.min(new_len);
        }
        self.length = new_len;
        Ok(())
    }
}

impl Length for Overlay<'_> {
    fn len(&self) -> io::Result<u64> {
        Ok(self.length)
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf, process};

    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn holding(name: &str, bytes: &[u8]) -> Self {
            let path = env::temp_dir().join(format!("resonate-overlay-{}-{name}", process::id()));
            fs::write(&path, bytes).expect("a scratch file");
            Self(path)
        }

        fn opened(&self) -> File {
            File::options()
                .read(true)
                .write(true)
                .open(&self.0)
                .expect("the scratch file opens")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn counted(length: usize) -> Vec<u8> {
        (0..length).map(|at| (at % 251) as u8).collect()
    }

    #[test]
    fn a_write_of_the_same_length_lands_only_the_pages_it_changed() {
        let whole = counted(3 * PAGE_BYTES as usize + 100);
        let scratch = Scratch::holding("same", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.seek(SeekFrom::Start(5000)).expect("a seek");
        overlay.write_all(b"written").expect("a write");
        overlay.seek(SeekFrom::Start(0)).expect("a seek");
        let mut read = Vec::new();
        overlay.read_to_end(&mut read).expect("a read");

        let mut wanted = whole.clone();
        wanted[5000..5007].copy_from_slice(b"written");
        assert_eq!(read, wanted, "the overlay did not read its own write");
        assert_eq!(
            fs::read(&scratch.0).expect("the file"),
            whole,
            "a write reached the file before it landed"
        );

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::InPlace
        );
        assert_eq!(fs::read(&scratch.0).expect("the file"), wanted);
    }

    #[test]
    fn a_write_of_what_was_there_already_lands_nothing() {
        let whole = counted(PAGE_BYTES as usize * 2);
        let scratch = Scratch::holding("unchanged", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.seek(SeekFrom::Start(10)).expect("a seek");
        overlay.write_all(&whole[10..20]).expect("a write");

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::Unchanged
        );
    }

    #[test]
    fn a_write_that_moves_the_end_of_the_file_is_not_landed_in_place() {
        let whole = counted(PAGE_BYTES as usize + 10);
        let scratch = Scratch::holding("grown", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.seek(SeekFrom::End(0)).expect("a seek");
        overlay.write_all(b"tail").expect("a write");
        assert_eq!(overlay.len().expect("a length"), whole.len() as u64 + 4);
        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::TooWide
        );

        let mut shrunk = Overlay::over(&file).expect("an overlay");
        shrunk.truncate(100).expect("a truncation");
        assert_eq!(
            shrunk.land(&scratch.0).expect("a landing"),
            Landing::TooWide
        );
        assert_eq!(fs::read(&scratch.0).expect("the file"), whole);
    }

    #[test]
    fn a_file_cut_short_and_grown_again_reads_zeros_where_it_was_cut() {
        let whole = counted(PAGE_BYTES as usize * 2);
        let scratch = Scratch::holding("regrown", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.truncate(10).expect("a truncation");
        overlay.truncate(PAGE_BYTES * 2).expect("a regrowth");
        overlay.seek(SeekFrom::Start(0)).expect("a seek");
        let mut read = Vec::new();
        overlay.read_to_end(&mut read).expect("a read");

        assert_eq!(read[..10], whole[..10]);
        assert!(read[10..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn a_write_wider_than_the_ceiling_is_refused_and_says_so() {
        let scratch = Scratch::holding("wide", &[0; 16]);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        let page = vec![1_u8; PAGE_BYTES as usize];
        let refused = (0..=MOST_PAGES).try_for_each(|_| overlay.write_all(&page));

        assert!(refused.is_err());
        assert!(overlay.outgrown());
        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::TooWide
        );
    }
}
