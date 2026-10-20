use std::{
    env, fs,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process,
};

use lofty::{
    file::FileType,
    io::{Length, Truncate},
};

use crate::{
    journal,
    overlay::{Landing, Overlay},
    padded::{Head, Room},
};

const FILE_TYPES: [FileType; 5] = [
    FileType::Flac,
    FileType::Mpeg,
    FileType::Aac,
    FileType::Vorbis,
    FileType::Opus,
];
const ROOMS: [Room; 4] = [
    Room::Held,
    Room::Generous,
    Room::Blocks(4096),
    Room::Blocks(0),
];
const LARGEST_OVERLAID_OFFSET: u64 = 96 << 10;
const OPERATION_BYTES: usize = 4;

pub fn read_a_padded_head(bytes: &[u8]) {
    let Some(([kind, room], rest)) = bytes.split_first_chunk::<2>() else {
        return;
    };
    let kind = FILE_TYPES[usize::from(*kind) % FILE_TYPES.len()];
    let room = ROOMS[usize::from(*room) % ROOMS.len()];
    let (head, written) = rest.split_at(rest.len() / 2);

    let Some(head) = Head::of(kind, &mut Cursor::new(head)) else {
        return;
    };
    let _ = head.tail_read(head.length.saturating_add(written.len() as u64));
    let _ = head.rebuilt(written, room);
}

pub fn read_an_undo_journal(bytes: &[u8]) {
    let mut sealed = bytes.to_vec();
    sealed.extend_from_slice(&journal::hashed(bytes).to_le_bytes());
    let _ = journal::decoded(Path::new("/music"), &sealed);
}

struct Scratch(PathBuf);

impl Scratch {
    fn holding(bytes: &[u8]) -> Self {
        let path = env::temp_dir().join(format!("resonate-fuzz-overlay-{}", process::id()));
        fs::write(&path, bytes).expect("a scratch file");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn edit_through_an_overlay(bytes: &[u8]) {
    let Some((base_length, operations)) = bytes.split_first() else {
        return;
    };
    let (base, operations) =
        operations.split_at((usize::from(*base_length) * 64).min(operations.len()));
    let mut model = base.to_vec();

    let scratch = Scratch::holding(base);
    let file = fs::File::options()
        .read(true)
        .write(true)
        .open(&scratch.0)
        .expect("the scratch file opens");
    let mut overlay = Overlay::over(&file).expect("an overlay");
    let mut at = 0_u64;

    for operation in operations.chunks_exact(OPERATION_BYTES) {
        let [kind, high, low, extra] = [operation[0], operation[1], operation[2], operation[3]];
        let offset = (u64::from(u16::from_be_bytes([high, low])) * 2).min(LARGEST_OVERLAID_OFFSET);
        match kind % 5 {
            0 => {
                at = overlay.seek(SeekFrom::Start(offset)).expect("a seek");
            }
            1 => {
                let written = vec![extra; usize::from(extra) * 32];
                if written.is_empty() {
                    continue;
                }
                overlay.write_all(&written).expect("a write");
                let start = at as usize;
                if model.len() < start + written.len() {
                    model.resize(start + written.len(), 0);
                }
                model[start..start + written.len()].copy_from_slice(&written);
                at += written.len() as u64;
            }
            2 => {
                let mut read = vec![0; usize::from(extra) * 16];
                let count = overlay.read(&mut read).expect("a read");
                let start = (at as usize).min(model.len());
                assert_eq!(&read[..count], &model[start..start + count]);
                at += count as u64;
            }
            3 => {
                overlay.truncate(offset).expect("a truncate");
                model.resize(offset as usize, 0);
            }
            _ => assert_eq!(overlay.len().expect("a length"), model.len() as u64),
        }
    }

    match overlay.land(&scratch.0).expect("a landing") {
        Landing::InPlace => {
            assert_eq!(fs::read(&scratch.0).expect("the landed file"), model);
        }
        Landing::Unchanged => assert_eq!(model, base),
        Landing::TooWide => assert_eq!(fs::read(&scratch.0).expect("the file"), base),
    }
}
