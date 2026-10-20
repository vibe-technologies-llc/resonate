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
const PAGE: usize = PAGE_BYTES as usize;
pub(crate) const MOST_BYTES_WRITTEN_IN_PLACE: u64 = 32 << 20;
const MOST_PAGES: usize = (MOST_BYTES_WRITTEN_IN_PLACE / PAGE_BYTES) as usize;
const PAGES_COMPARED_AT_ONCE: usize = 256;

type Page = Box<[u8; PAGE]>;

enum Slot {
    Written(Page),
    Stood,
}

struct Rewrite {
    start: u64,
    before: Vec<u8>,
    after: Vec<u8>,
}

pub(crate) struct Overlay<'f> {
    base: &'f File,
    stood: u64,
    readable: u64,
    length: u64,
    at: u64,
    pages: BTreeMap<u64, Slot>,
    held: usize,
    compared: Vec<u8>,
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
            held: 0,
            compared: Vec::new(),
            outgrown: false,
        })
    }

    pub(crate) const fn outgrown(&self) -> bool {
        self.outgrown
    }

    pub(crate) fn land(&self, track: &Path) -> io::Result<Landing> {
        if self.outgrown {
            return Ok(Landing::TooWide);
        }
        let Some(rewrites) = self.rewrites()? else {
            return Ok(Landing::TooWide);
        };
        if rewrites.is_empty() {
            return Ok(Landing::Unchanged);
        }

        let (befores, afters): (Vec<_>, Vec<_>) = rewrites
            .into_iter()
            .map(|rewrite| ((rewrite.start, rewrite.before), rewrite.after))
            .unzip();
        let changes: Vec<Change<'_>> = befores
            .into_iter()
            .zip(&afters)
            .map(|((start, before), after)| Change {
                start,
                before,
                after,
            })
            .collect();

        let undo = Undo::kept_beside(track, self.stood, self.length, &changes)
            .inspect_err(|error| {
                tracing::debug!(%error, path = %track.display(), "no undo could be kept beside the track, so its tag lands in place without one");
            })
            .ok();
        if undo.is_none() && self.length != self.stood {
            return Ok(Landing::TooWide);
        }

        match (self.laid(&changes), undo) {
            (Ok(()), Some(undo)) => undo.done(),
            (Ok(()), None) => {}
            (Err(error), Some(undo)) => {
                undo.roll_back(self.base, self.stood, &changes);
                return Err(error);
            }
            (Err(error), None) => return Err(error),
        }
        Ok(Landing::InPlace)
    }

    fn laid(&self, changes: &[Change<'_>]) -> io::Result<()> {
        changes
            .iter()
            .filter(|change| !change.after.is_empty())
            .try_for_each(|change| self.base.write_all_at(change.after, change.start))?;
        if self.length < self.stood {
            self.base.set_len(self.length)?;
        }
        self.base.sync_data()
    }

    fn rewrites(&self) -> io::Result<Option<Vec<Rewrite>>> {
        let first_unread = self.readable / PAGE_BYTES;
        let last = self.length.max(self.stood).div_ceil(PAGE_BYTES);
        let unwritten = (first_unread..last).filter(|index| !self.pages.contains_key(index));

        let mut rewrites = Vec::new();
        for index in self.pages.keys().copied().chain(unwritten) {
            let Some(rewrite) = self.rewrite_of(index)? else {
                continue;
            };
            if rewrites.len() >= MOST_PAGES {
                return Ok(None);
            }
            rewrites.push(rewrite);
        }
        rewrites.sort_by_key(|rewrite| rewrite.start);
        Ok(Some(rewrites))
    }

    fn rewrite_of(&self, index: u64) -> io::Result<Option<Rewrite>> {
        let start = index * PAGE_BYTES;
        let after_bytes =
            usize::try_from(self.length.saturating_sub(start).min(PAGE_BYTES)).unwrap_or(PAGE);
        let after = match self.pages.get(&index) {
            Some(Slot::Stood) => return Ok(None),
            Some(Slot::Written(bytes)) => bytes[..after_bytes].to_vec(),
            None => {
                let mut bytes = vec![0; after_bytes];
                self.read_at(&mut bytes, start)?;
                bytes
            }
        };

        let before_bytes =
            usize::try_from(self.stood.saturating_sub(start).min(PAGE_BYTES)).unwrap_or(PAGE);
        let mut before = vec![0; before_bytes];
        self.base.read_exact_at(&mut before, start)?;
        Ok((before != after).then_some(Rewrite {
            start,
            before,
            after,
        }))
    }

    fn loaded(&self, index: u64) -> io::Result<Page> {
        let start = index * PAGE_BYTES;
        let readable = match self.pages.get(&index) {
            Some(Slot::Stood) => PAGE_BYTES,
            _ => self.readable.saturating_sub(start).min(PAGE_BYTES),
        };
        let mut bytes: Page = Box::new([0; PAGE]);
        if readable > 0 {
            let readable = usize::try_from(readable).unwrap_or(PAGE);
            self.base.read_exact_at(&mut bytes[..readable], start)?;
        }
        Ok(bytes)
    }

    fn page(&mut self, index: u64) -> io::Result<&mut Page> {
        if !matches!(self.pages.get(&index), Some(Slot::Written(_))) {
            if self.held >= MOST_PAGES {
                self.outgrown = true;
                return Err(io::Error::from(io::ErrorKind::FileTooLarge));
            }
            let bytes = self.loaded(index)?;
            self.pages.insert(index, Slot::Written(bytes));
            self.held += 1;
        }
        let Some(Slot::Written(bytes)) = self.pages.get_mut(&index) else {
            unreachable!("a page this call has just put there");
        };
        Ok(bytes)
    }

    fn read_at(&self, into: &mut [u8], start: u64) -> io::Result<()> {
        let page = start / PAGE_BYTES;
        let offset = usize::try_from(start % PAGE_BYTES).unwrap_or_default();
        match self.pages.get(&page) {
            Some(Slot::Written(bytes)) => {
                into.copy_from_slice(&bytes[offset..offset + into.len()]);
            }
            Some(Slot::Stood) => self.base.read_exact_at(into, start)?,
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

    fn served_by_the_file(&self, start: u64, most: u64) -> u64 {
        let end = start.saturating_add(most);
        let mut at = start;
        while at < end {
            let index = at / PAGE_BYTES;
            let page_end = (index + 1) * PAGE_BYTES;
            let stops = page_end.min(end);
            match self.pages.get(&index) {
                Some(Slot::Stood) => at = stops,
                Some(Slot::Written(_)) => break,
                None => {
                    let backed = stops.min(self.readable);
                    if backed <= at {
                        break;
                    }
                    at = backed;
                    if backed < stops {
                        break;
                    }
                }
            }
        }
        at - start
    }

    fn stand_by_the_file(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.at.is_multiple_of(PAGE_BYTES) {
            return Ok(0);
        }
        let first = self.at / PAGE_BYTES;
        let whole_in_the_file = (self.stood / PAGE_BYTES).saturating_sub(first);
        let pages = (buf.len() / PAGE)
            .min(PAGES_COMPARED_AT_ONCE)
            .min(usize::try_from(whole_in_the_file).unwrap_or(usize::MAX));
        if pages == 0 {
            return Ok(0);
        }

        let mut compared = std::mem::take(&mut self.compared);
        compared.resize(pages * PAGE, 0);
        let read = self.base.read_exact_at(&mut compared, self.at);
        let alike = read.map(|()| {
            compared
                .as_chunks::<PAGE>()
                .0
                .iter()
                .zip(buf.as_chunks::<PAGE>().0)
                .take_while(|(file, written)| file == written)
                .count()
        });
        self.compared = compared;
        let alike = alike?;

        for index in first..first + alike as u64 {
            if matches!(
                self.pages.insert(index, Slot::Stood),
                Some(Slot::Written(_))
            ) {
                self.held -= 1;
            }
        }
        self.at += (alike * PAGE) as u64;
        self.length = self.length.max(self.at);
        Ok(alike * PAGE)
    }
}

impl Read for Overlay<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let left = self.length.saturating_sub(self.at);
        let wanted = left.min(buf.len() as u64);
        if wanted == 0 {
            return Ok(0);
        }

        let from_the_file = self.served_by_the_file(self.at, wanted);
        if from_the_file > 0 {
            let taking = usize::try_from(from_the_file).unwrap_or(buf.len());
            self.base.read_exact_at(&mut buf[..taking], self.at)?;
            self.at += from_the_file;
            return Ok(taking);
        }

        let within_page = PAGE_BYTES - self.at % PAGE_BYTES;
        let taking = usize::try_from(wanted.min(within_page)).unwrap_or(buf.len());
        self.read_at(&mut buf[..taking], self.at)?;
        self.at += taking as u64;
        Ok(taking)
    }
}

impl Write for Overlay<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let standing = self.stand_by_the_file(buf)?;
        if standing > 0 {
            return Ok(standing);
        }

        let at = self.at;
        let offset = usize::try_from(at % PAGE_BYTES).unwrap_or_default();
        let wanted = buf.len().min(PAGE - offset);

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
            self.held = self
                .pages
                .values()
                .filter(|slot| matches!(slot, Slot::Written(_)))
                .count();
            self.readable = self.readable.min(new_len);

            let offset = usize::try_from(new_len % PAGE_BYTES).unwrap_or_default();
            if offset > 0 && self.pages.contains_key(&(new_len / PAGE_BYTES)) {
                let last = self.page(new_len / PAGE_BYTES)?;
                last[offset..].fill(0);
            }
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
    fn a_tail_that_grows_is_landed_in_place_and_the_file_grows_with_it() {
        let whole = counted(PAGE * 3 + 10);
        let scratch = Scratch::holding("grown", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.seek(SeekFrom::End(0)).expect("a seek");
        overlay.write_all(&[7; PAGE * 2]).expect("a write");
        assert_eq!(
            overlay.len().expect("a length"),
            (whole.len() + PAGE * 2) as u64
        );

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::InPlace
        );
        let mut wanted = whole.clone();
        wanted.extend_from_slice(&[7; PAGE * 2]);
        assert_eq!(fs::read(&scratch.0).expect("the file"), wanted);
    }

    #[test]
    fn a_tail_that_shrinks_is_landed_in_place_and_the_file_is_cut() {
        let whole = counted(PAGE * 5 + 10);
        let scratch = Scratch::holding("shrunk", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay
            .truncate(PAGE as u64 * 2 + 100)
            .expect("a truncation");
        overlay
            .seek(SeekFrom::Start(PAGE as u64 * 2))
            .expect("a seek");
        overlay.write_all(&[9; 100]).expect("a write");

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::InPlace
        );
        let mut wanted = whole[..PAGE * 2 + 100].to_vec();
        wanted[PAGE * 2..].fill(9);
        assert_eq!(fs::read(&scratch.0).expect("the file"), wanted);
    }

    #[test]
    fn a_file_written_back_whole_keeps_only_the_pages_that_differ() {
        let whole = counted(PAGE * 300 + 123);
        let scratch = Scratch::holding("whole", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        let mut read = Vec::new();
        overlay.read_to_end(&mut read).expect("a read");
        assert_eq!(read, whole);

        read.truncate(whole.len() - 50);
        read.extend_from_slice(&[5; PAGE + 500]);
        overlay.rewind().expect("a rewind");
        overlay.truncate(0).expect("a truncation");
        overlay.write_all(&read).expect("a write");

        assert!(overlay.held <= 3, "{} pages held", overlay.held);
        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::InPlace
        );
        assert_eq!(fs::read(&scratch.0).expect("the file"), read);
    }

    #[test]
    fn a_file_cut_short_and_grown_to_its_length_again_lands_the_zeros() {
        let whole = counted(PAGE * 2);
        let scratch = Scratch::holding("zeroed", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        overlay.truncate(10).expect("a truncation");
        overlay.truncate(PAGE as u64 * 2).expect("a regrowth");

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::InPlace
        );
        let mut wanted = whole.clone();
        wanted[10..].fill(0);
        assert_eq!(fs::read(&scratch.0).expect("the file"), wanted);
    }

    #[test]
    fn a_file_that_ends_up_as_it_was_lands_nothing() {
        let whole = counted(PAGE * 20 + 7);
        let scratch = Scratch::holding("alike", &whole);
        let file = scratch.opened();

        let mut overlay = Overlay::over(&file).expect("an overlay");
        let mut read = Vec::new();
        overlay.read_to_end(&mut read).expect("a read");
        overlay.rewind().expect("a rewind");
        overlay.truncate(0).expect("a truncation");
        overlay.write_all(&read).expect("a write");

        assert_eq!(
            overlay.land(&scratch.0).expect("a landing"),
            Landing::Unchanged
        );
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
