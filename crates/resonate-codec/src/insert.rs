use std::{fs::File, io, os::unix::fs::FileExt as _, path::Path};

use crate::journal::{Change, Undo};

pub(crate) trait Shifting {
    fn block_of(&self, file: &File) -> Option<u64>;
    fn inserted(&self, file: &File, bytes: u64) -> io::Result<()>;
    fn collapsed(&self, file: &File, bytes: u64) -> io::Result<()>;
}

pub(crate) struct Fallocate;

#[cfg(any(target_os = "linux", target_os = "android"))]
mod fallocating {
    use std::{fs::File, io};

    use rustix::fs::{FallocateFlags, fallocate, fstatfs, fstatvfs};

    use super::{Fallocate, Shifting};

    const EXT_MAGIC: u64 = 0xEF53;
    const XFS_MAGIC: u64 = 0x5846_5342;
    const F2FS_MAGIC: u64 = 0xF2F5_2010;
    const FROM_THE_START: u64 = 0;

    impl Shifting for Fallocate {
        fn block_of(&self, file: &File) -> Option<u64> {
            let kind = u64::try_from(fstatfs(file).ok()?.f_type).ok()?;
            if ![EXT_MAGIC, XFS_MAGIC, F2FS_MAGIC].contains(&kind) {
                return None;
            }
            Some(fstatvfs(file).ok()?.f_bsize).filter(|block| *block > 0)
        }

        fn inserted(&self, file: &File, bytes: u64) -> io::Result<()> {
            fallocate(file, FallocateFlags::INSERT_RANGE, FROM_THE_START, bytes)?;
            Ok(())
        }

        fn collapsed(&self, file: &File, bytes: u64) -> io::Result<()> {
            fallocate(file, FallocateFlags::COLLAPSE_RANGE, FROM_THE_START, bytes)?;
            Ok(())
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
impl Shifting for Fallocate {
    fn block_of(&self, _: &File) -> Option<u64> {
        None
    }

    fn inserted(&self, _: &File, _: u64) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn collapsed(&self, _: &File, _: u64) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

pub(crate) fn lengthened(
    shifting: &dyn Shifting,
    file: &File,
    track: &Path,
    stood: u64,
    standing_head: u64,
    head: &[u8],
) -> io::Result<bool> {
    let shifted = (head.len() as u64)
        .checked_sub(standing_head)
        .filter(|shifted| *shifted > 0)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let mut before = vec![0; usize::try_from(standing_head).map_err(io::Error::other)?];
    file.read_exact_at(&mut before, 0)?;

    let changes = [Change {
        start: 0,
        before,
        after: head,
    }];
    let undo = match Undo::kept_shifting(track, stood, shifted, &changes) {
        Ok(undo) => undo,
        Err(error) => {
            tracing::debug!(%error, path = %track.display(), "no undo could be kept beside the track, so its head is not lengthened in place");
            return Ok(false);
        }
    };

    if let Err(error) = shifting.inserted(file, shifted) {
        tracing::debug!(%error, path = %track.display(), "the filesystem would not open room at the head of the track");
        undo.done();
        return Ok(false);
    }

    let laid = file
        .sync_all()
        .and_then(|()| file.write_all_at(head, 0))
        .and_then(|()| file.sync_data());
    match laid {
        Ok(()) => {
            undo.done();
            Ok(true)
        }
        Err(error) => {
            match taken_back(shifting, file, shifted, &changes[0].before) {
                Ok(()) => undo.done(),
                Err(rollback) => {
                    tracing::warn!(error = %rollback, path = %track.display(), "a head lengthened in place failed part way and could not be taken back, so its undo is left for the next run");
                }
            }
            Err(error)
        }
    }
}

fn taken_back(shifting: &dyn Shifting, file: &File, shifted: u64, before: &[u8]) -> io::Result<()> {
    file.write_all_at(before, shifted)?;
    file.sync_data()?;
    shifting.collapsed(file, shifted)?;
    file.sync_all()
}

#[cfg(test)]
pub(crate) struct Copying {
    pub(crate) block: u64,
}

#[cfg(test)]
impl Copying {
    fn moved(file: &File, from: u64, to: u64) -> io::Result<()> {
        let length = file.metadata()?.len();
        let mut held = vec![0; usize::try_from(length).map_err(io::Error::other)?];
        file.read_exact_at(&mut held, 0)?;
        let kept = &held[usize::try_from(from).map_err(io::Error::other)?..];
        let new_length = length - from + to;
        file.set_len(new_length)?;
        file.write_all_at(kept, to)
    }
}

#[cfg(test)]
impl Shifting for Copying {
    fn block_of(&self, _: &File) -> Option<u64> {
        Some(self.block)
    }

    fn inserted(&self, file: &File, bytes: u64) -> io::Result<()> {
        Self::moved(file, 0, bytes)?;
        file.write_all_at(
            &vec![0; usize::try_from(bytes).map_err(io::Error::other)?],
            0,
        )
    }

    fn collapsed(&self, file: &File, bytes: u64) -> io::Result<()> {
        Self::moved(file, bytes, 0)
    }
}

#[cfg(test)]
pub(crate) struct Refusing;

#[cfg(test)]
impl Shifting for Refusing {
    fn block_of(&self, _: &File) -> Option<u64> {
        Some(4096)
    }

    fn inserted(&self, _: &File, _: u64) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }

    fn collapsed(&self, _: &File, _: u64) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}

#[cfg(test)]
mod tests {
    use std::{env, fs, path::PathBuf, process};

    use super::*;
    use crate::journal::{Mended, mend_what_a_dead_writer_left};

    struct Scratch(PathBuf);

    impl Scratch {
        fn holding(name: &str, bytes: &[u8]) -> Self {
            let folder = env::temp_dir().join(format!("resonate-insert-{}-{name}", process::id()));
            let _ = fs::remove_dir_all(&folder);
            fs::create_dir_all(&folder).expect("a scratch folder");
            let track = folder.join("Track.flac");
            fs::write(&track, bytes).expect("a scratch file");
            Self(track)
        }

        fn opened(&self) -> File {
            File::options()
                .read(true)
                .write(true)
                .open(&self.0)
                .expect("the scratch file opens")
        }

        fn folder(&self) -> &Path {
            self.0.parent().expect("a folder")
        }

        fn leftovers(&self) -> usize {
            fs::read_dir(self.folder()).expect("the folder").count() - 1
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.folder());
        }
    }

    fn counted(length: usize) -> Vec<u8> {
        (0..length).map(|at| (at % 251) as u8).collect()
    }

    fn lengthened_file(standing: &[u8], head: usize, by: usize) -> (Vec<u8>, Vec<u8>) {
        let mut grown_head = vec![0xEE; head + by];
        grown_head[0] = 0xAA;
        let mut grown = grown_head.clone();
        grown.extend_from_slice(&standing[head..]);
        (grown_head, grown)
    }

    #[test]
    fn room_opened_at_the_head_is_filled_and_the_audio_behind_it_stays_whole() {
        let standing = counted(10_000);
        let track = Scratch::holding("lands", &standing);
        let (head, grown) = lengthened_file(&standing, 700, 4096);

        let landed = lengthened(
            &Copying { block: 4096 },
            &track.opened(),
            &track.0,
            standing.len() as u64,
            700,
            &head,
        )
        .expect("a lengthened head");

        assert!(landed);
        assert_eq!(fs::read(&track.0).expect("the track"), grown);
        assert_eq!(track.leftovers(), 0, "a journal was left beside the track");
    }

    #[test]
    fn a_filesystem_that_will_not_open_room_leaves_the_track_and_no_journal() {
        let standing = counted(10_000);
        let track = Scratch::holding("refused", &standing);
        let (head, _) = lengthened_file(&standing, 700, 4096);

        let landed = lengthened(
            &Refusing,
            &track.opened(),
            &track.0,
            standing.len() as u64,
            700,
            &head,
        )
        .expect("a refusal");

        assert!(!landed);
        assert_eq!(fs::read(&track.0).expect("the track"), standing);
        assert_eq!(track.leftovers(), 0);
    }

    #[test]
    fn a_head_that_failed_part_way_is_taken_back_to_the_track_it_was() {
        let standing = counted(10_000);
        let track = Scratch::holding("taken-back", &standing);
        let file = track.opened();
        let shifting = Copying { block: 4096 };

        shifting.inserted(&file, 4096).expect("room");
        file.write_all_at(&[0xEE; 300], 0).expect("part of a head");

        taken_back(&shifting, &file, 4096, &standing[..700]).expect("taken back");

        assert_eq!(fs::read(&track.0).expect("the track"), standing);
    }

    fn left_part_way(
        name: &str,
        standing: &[u8],
        head: usize,
        by: usize,
        state: &[u8],
    ) -> (Scratch, Vec<u8>) {
        let track = Scratch::holding(name, standing);
        let (new_head, grown) = lengthened_file(standing, head, by);
        let changes = [Change {
            start: 0,
            before: standing[..head].to_vec(),
            after: &new_head,
        }];
        let undo = Undo::kept_shifting(&track.0, standing.len() as u64, by as u64, &changes)
            .expect("a journal");
        drop(undo);
        fs::write(&track.0, state).expect("a state the writer died in");

        let named = track
            .0
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        let journals: Vec<PathBuf> = fs::read_dir(track.folder())
            .expect("the folder")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.to_string_lossy().ends_with(".resonate-undo"))
            .collect();
        assert_eq!(journals.len(), 1, "{named} left no journal");
        let dead = track
            .folder()
            .join(format!(".{named}.999999999-0.resonate-undo"));
        fs::rename(&journals[0], &dead).expect("a journal of a dead writer");
        (track, grown)
    }

    fn opened_but_unwritten(standing: &[u8], by: usize) -> Vec<u8> {
        let mut state = vec![0; by];
        state.extend_from_slice(standing);
        state
    }

    #[test]
    fn a_write_that_died_before_the_room_was_opened_leaves_the_track_as_it_was() {
        let standing = counted(10_000);
        let (track, _) = left_part_way("before", &standing, 700, 4096, &standing);

        mend_what_a_dead_writer_left(&track.0);

        assert_eq!(fs::read(&track.0).expect("the track"), standing);
        assert_eq!(track.leftovers(), 0, "the journal was left");
    }

    #[test]
    fn a_write_that_died_with_the_room_open_and_empty_is_carried_on_to_the_head_it_meant() {
        let standing = counted(10_000);
        let state = opened_but_unwritten(&standing, 4096);
        let (track, grown) = left_part_way("open-empty", &standing, 700, 4096, &state);

        mend_what_a_dead_writer_left(&track.0);

        assert_eq!(fs::read(&track.0).expect("the track"), grown);
        assert_eq!(track.leftovers(), 0, "the journal was left");
    }

    #[test]
    fn a_write_that_died_half_way_through_the_head_is_carried_on_too() {
        let standing = counted(10_000);
        let (_, grown) = lengthened_file(&standing, 700, 4096);
        let mut state = opened_but_unwritten(&standing, 4096);
        state[..2000].copy_from_slice(&grown[..2000]);
        let (track, grown) = left_part_way("half", &standing, 700, 4096, &state);

        mend_what_a_dead_writer_left(&track.0);

        assert_eq!(fs::read(&track.0).expect("the track"), grown);
    }

    #[test]
    fn a_write_that_died_having_landed_is_left_as_it_stands() {
        let standing = counted(10_000);
        let (_, grown) = lengthened_file(&standing, 700, 4096);
        let (track, grown) = left_part_way("landed", &standing, 700, 4096, &grown);

        mend_what_a_dead_writer_left(&track.0);

        assert_eq!(fs::read(&track.0).expect("the track"), grown);
        assert_eq!(track.leftovers(), 0);
    }

    #[test]
    fn a_track_another_program_changed_after_the_room_was_opened_is_not_written_over() {
        let standing = counted(10_000);
        let mut state = opened_but_unwritten(&standing, 4096);
        state[3000] ^= 0xFF;
        let (track, _) = left_part_way("changed", &standing, 700, 4096, &state);

        mend_what_a_dead_writer_left(&track.0);

        assert_eq!(fs::read(&track.0).expect("the track"), state);
    }

    #[test]
    fn a_mend_of_a_shifted_write_names_what_it_did() {
        let standing = counted(10_000);
        let state = opened_but_unwritten(&standing, 4096);
        let (track, _) = left_part_way("named", &standing, 700, 4096, &state);
        let journal = fs::read_dir(track.folder())
            .expect("the folder")
            .flatten()
            .map(|entry| entry.path())
            .find(|path| path.to_string_lossy().ends_with(".resonate-undo"))
            .expect("a journal");

        assert_eq!(
            crate::journal::mend_a_cut_short_write(&journal).expect("a mend"),
            Mended::CarriedOn
        );
    }
}
