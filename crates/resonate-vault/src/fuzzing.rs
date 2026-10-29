use std::{
    env, fs,
    io::{Cursor, Read, Seek, SeekFrom},
    path::PathBuf,
    process,
};

use resonate_codec::{Container, MediaStream, Reading};
use resonate_core::MediaLocation;

use crate::{
    bare, transcoded,
    unpacking::{self, Unpacking},
};

const CONTAINERS: [Container; 16] = [
    Container::Wave,
    Container::Aiff,
    Container::Caf,
    Container::Mpeg,
    Container::Adts,
    Container::Ogg,
    Container::Flac,
    Container::Dsf,
    Container::Dff,
    Container::IsoMp4,
    Container::Matroska,
    Container::WavPack,
    Container::MonkeysAudio,
    Container::Rf64,
    Container::Wave64,
    Container::Unknown,
];
const MOST_UNPACKED_READ: u64 = 1 << 20;

pub fn bare_a_file(bytes: &[u8]) {
    let Some((kind, bytes)) = bytes.split_first() else {
        return;
    };
    let container = CONTAINERS[usize::from(*kind) % CONTAINERS.len()];
    let location = MediaLocation::local(PathBuf::from("a.file"));

    let mut stream: Box<dyn MediaStream> = Box::new(Reading::new(Cursor::new(bytes.to_vec())));
    let _ = bare::bare(&mut stream, container, &location);

    let mut stream: Box<dyn MediaStream> = Box::new(Reading::new(Cursor::new(bytes.to_vec())));
    let _ = bare::declared_digest(&mut stream);
}

struct Scratch(PathBuf);

impl Scratch {
    fn holding(bytes: &[u8]) -> Self {
        let path = env::temp_dir().join(format!("resonate-fuzz-unpacking-{}", process::id()));
        fs::write(&path, bytes).expect("a scratch file");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn unpack_a_file(bytes: &[u8]) {
    let scratch = Scratch::holding(bytes);
    let _ = unpacking::frames_of(&scratch.0);

    let Ok(mut unpacking) = Unpacking::open(&scratch.0) else {
        return;
    };
    let mut buffer = Vec::new();
    let _ = (&mut unpacking)
        .take(MOST_UNPACKED_READ)
        .read_to_end(&mut buffer);
    let _ = unpacking.seek(SeekFrom::Start(0));
    let _ = unpacking.seek(SeekFrom::End(0));
    let _ = unpacking.seek(SeekFrom::Current(-64));
    let _ = (&mut unpacking)
        .take(MOST_UNPACKED_READ)
        .read_to_end(&mut buffer);
}

pub fn transcode_a_jpeg(bytes: &[u8]) {
    let Some(transcoded) = transcoded::of(bytes) else {
        return;
    };
    let _ = transcoded::reconstructs(&transcoded.bytes, bytes);
}
