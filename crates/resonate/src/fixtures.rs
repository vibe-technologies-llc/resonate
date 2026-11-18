use std::{
    env, fs, io,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use clap::Parser as _;
use resonate_library::{Library, PassHandle, Removal, ScanOptions, Trashes};

use crate::cli::Cli;

const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const RATE: u32 = 44_100;
const UTF8: u8 = 3;
const MP3_HEADER: [u8; 4] = [0xff, 0xfb, 0x90, 0x00];
const MP3_FRAME_LENGTH: usize = 417;
const MP3_FRAMES: usize = 40;
const SOUNDING_STEP: u32 = 7;

static SCRATCH_FOLDERS: AtomicU64 = AtomicU64::new(0);

pub const TITLE: &[u8; 4] = b"TIT2";
pub const ARTIST: &[u8; 4] = b"TPE1";
pub const ALBUM: &[u8; 4] = b"TALB";

pub fn silent_wave(seconds_tenths: u32) -> Vec<u8> {
    wave_after(Vec::new(), seconds_tenths)
}

pub fn tagged_wave(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    wave_after(id3_tag(tags), 1)
}

pub fn sounding_wave(seconds_tenths: u32) -> Vec<u8> {
    let frames = RATE / 10 * seconds_tenths;
    let data = (0..frames * u32::from(CHANNELS))
        .flat_map(|sample| (sample.wrapping_mul(SOUNDING_STEP) as i16).to_le_bytes())
        .collect();
    wave_holding(Vec::new(), data)
}

pub fn tagged_mp3(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut file = id3_tag(tags);
    for _ in 0..MP3_FRAMES {
        file.extend_from_slice(&MP3_HEADER);
        file.resize(file.len() + MP3_FRAME_LENGTH - MP3_HEADER.len(), 0);
    }
    file
}

fn id3_tag(tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut frames = Vec::new();
    for (id, value) in tags {
        let mut body = vec![UTF8];
        body.extend_from_slice(value.as_bytes());
        frames.extend_from_slice(*id);
        frames.extend_from_slice(&synchsafe(body.len() as u32));
        frames.extend_from_slice(&[0, 0]);
        frames.extend_from_slice(&body);
    }

    let mut tag = b"ID3\x04\x00\x00".to_vec();
    tag.extend_from_slice(&synchsafe(frames.len() as u32));
    tag.extend_from_slice(&frames);
    tag
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        (value >> 21) as u8 & 0x7f,
        (value >> 14) as u8 & 0x7f,
        (value >> 7) as u8 & 0x7f,
        value as u8 & 0x7f,
    ]
}

fn wave_after(file: Vec<u8>, seconds_tenths: u32) -> Vec<u8> {
    let frames = RATE / 10 * seconds_tenths;
    let data = vec![0_u8; frames as usize * usize::from(CHANNELS * BITS / 8)];
    wave_holding(file, data)
}

fn wave_holding(mut file: Vec<u8>, data: Vec<u8>) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    for (id, payload) in [(b"fmt ", format.as_slice()), (b"data", data.as_slice())] {
        body.extend_from_slice(id);
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
    }
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

pub fn scratch(name: &str) -> PathBuf {
    let made = SCRATCH_FOLDERS.fetch_add(1, Ordering::Relaxed);
    let folder = env::temp_dir().join(format!("resonate-{name}-{}-{made}", process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

pub fn scan_of(root: &Path) -> ScanOptions {
    ScanOptions {
        roots: vec![root.to_path_buf()],
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: false,
        workers: NonZeroUsize::MIN,
    }
}

pub struct Discarded;

impl Trashes for Discarded {
    fn trash(&self, file: &Path) -> Removal {
        match fs::remove_file(file) {
            Ok(()) => Removal::Removed,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Removal::AlreadyGone,
            Err(_) => Removal::Kept,
        }
    }
}

pub fn scanned(root: &Path) -> Library {
    let library = Library::open_in_memory().expect("a catalog");
    library.add_root(root).expect("a root");
    library
        .scan(scan_of(root))
        .and_then(PassHandle::join)
        .expect("a scan");
    library
}

pub fn parsed(arguments: &[&str]) -> Cli {
    Cli::try_parse_from(arguments.iter().copied())
        .unwrap_or_else(|error| panic!("{arguments:?} was refused: {error}"))
}
