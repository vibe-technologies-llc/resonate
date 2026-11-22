use std::{
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{PlaylistId, QueueStamp};
use resonate_library::{
    Library, Playing, Result, ScanOptions, ScanStats, SortOrder, Track, TrackQuery,
};

pub(crate) const RATE: u32 = 44_100;

pub(crate) const CHANNELS: u16 = 2;

pub(crate) const UTF8: u8 = 3;

pub(crate) const FRONT_COVER: u8 = 3;

pub(crate) const TITLE: &[u8; 4] = b"TIT2";

pub(crate) const ARTIST: &[u8; 4] = b"TPE1";

pub(crate) const ALBUM_ARTIST: &[u8; 4] = b"TPE2";

pub(crate) const ALBUM: &[u8; 4] = b"TALB";

pub(crate) const TRACK: &[u8; 4] = b"TRCK";

pub(crate) const YEAR: &[u8; 4] = b"TDRC";

pub(crate) const GENRE: &[u8; 4] = b"TCON";

pub(crate) const ISRC: &[u8; 4] = b"TSRC";

pub(crate) const LABEL: &[u8; 4] = b"TPUB";

pub(crate) const COMPILATION: &[u8; 4] = b"TCMP";

pub(crate) const USER_TEXT: &[u8; 4] = b"TXXX";

pub(crate) const UNIQUE_FILE_ID: &[u8; 4] = b"UFID";

pub(crate) const MUSICBRAINZ: &str = "http://musicbrainz.org";

pub(crate) const MUSICBRAINZ_ALBUM: &str = "MusicBrainz Album Id";

pub(crate) const MUSICBRAINZ_ARTIST: &str = "MusicBrainz Artist Id";

pub(crate) const MUSICBRAINZ_RELEASE_GROUP: &str = "MusicBrainz Release Group Id";

pub(crate) struct Tree {
    pub(crate) root: PathBuf,
}

impl Tree {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let root = env::temp_dir().join(format!(
            "resonate-library-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self {
            root: root.canonicalize().expect("a canonical path"),
        }
    }

    pub(crate) fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, bytes).expect("a writable temporary file");
        path
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(crate) struct Wav {
    pub(crate) bits: u16,
    pub(crate) rate: u32,
    pub(crate) frames: usize,
    pub(crate) from: usize,
    pub(crate) id3: Vec<u8>,
    pub(crate) id3_in_a_chunk: bool,
}

impl Wav {
    pub(crate) fn new() -> Self {
        Self {
            bits: 16,
            rate: RATE,
            frames: 4_410,
            from: 0,
            id3: Vec::new(),
            id3_in_a_chunk: false,
        }
    }

    pub(crate) fn sounding_from(mut self, from: usize) -> Self {
        self.from = from;
        self
    }

    pub(crate) fn id3_in_a_chunk(mut self) -> Self {
        self.id3_in_a_chunk = true;
        self
    }

    pub(crate) fn bits(mut self, bits: u16) -> Self {
        self.bits = bits;
        self
    }

    pub(crate) fn rate(mut self, rate: u32) -> Self {
        self.rate = rate;
        self
    }

    pub(crate) fn frames(mut self, frames: usize) -> Self {
        self.frames = frames;
        self
    }

    pub(crate) fn text(mut self, id: &[u8; 4], value: &str) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, id, &body);
        self
    }

    pub(crate) fn described(mut self, description: &str, value: &str) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(description.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, USER_TEXT, &body);
        self
    }

    pub(crate) fn sung(mut self, words: &str) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(b"eng\0");
        body.extend_from_slice(words.as_bytes());
        frame(&mut self.id3, b"USLT", &body);
        self
    }

    pub(crate) fn identified(mut self, owner: &str, id: &str) -> Self {
        let mut body = owner.as_bytes().to_vec();

        body.push(0);
        body.extend_from_slice(id.as_bytes());
        frame(&mut self.id3, UNIQUE_FILE_ID, &body);
        self
    }

    pub(crate) fn picture(mut self, bytes: &[u8]) -> Self {
        let mut body = vec![UTF8];

        body.extend_from_slice(b"image/png\0");
        body.push(FRONT_COVER);
        body.push(0);
        body.extend_from_slice(bytes);
        frame(&mut self.id3, b"APIC", &body);
        self
    }

    pub(crate) fn build(&self) -> Vec<u8> {
        let stride = usize::from(self.bits / 8);
        let span = 1_i64 << (self.bits - 1);
        let mut data = Vec::with_capacity(self.frames * usize::from(CHANNELS) * stride);

        for n in self.from..self.from + self.frames * usize::from(CHANNELS) {
            let sample = ((n as i64 % (2 * span - 1)) - span + 1) as i32;

            data.extend_from_slice(&sample.to_le_bytes()[..stride]);
        }

        let block_align = CHANNELS * self.bits / 8;
        let mut fmt = Vec::new();

        fmt.extend_from_slice(&1_u16.to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&self.rate.to_le_bytes());
        fmt.extend_from_slice(&(self.rate * u32::from(block_align)).to_le_bytes());
        fmt.extend_from_slice(&block_align.to_le_bytes());
        fmt.extend_from_slice(&self.bits.to_le_bytes());

        let mut body = Vec::new();

        body.extend_from_slice(b"WAVE");
        chunk(&mut body, b"fmt ", &fmt);
        chunk(&mut body, b"data", &data);

        let mut tag = Vec::new();

        if !self.id3.is_empty() {
            tag.extend_from_slice(b"ID3\x04\x00\x00");
            tag.extend_from_slice(&synchsafe(self.id3.len() as u32));
            tag.extend_from_slice(&self.id3);
        }

        let mut file = Vec::new();

        if self.id3_in_a_chunk {
            chunk(&mut body, b"id3 ", &tag);
        } else {
            file.extend_from_slice(&tag);
        }
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }
}

pub(crate) fn frame(into: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&synchsafe(body.len() as u32));
    into.extend_from_slice(&[0, 0]);
    into.extend_from_slice(body);
}

pub(crate) fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

pub(crate) fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

pub(crate) fn png(size: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();

    bytes.extend((0..size).map(|n| n as u8));
    bytes
}

pub(crate) fn options(tree: &Tree) -> ScanOptions {
    ScanOptions {
        roots: vec![tree.path().to_path_buf()],
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: true,
        workers: NonZeroUsize::MIN,
    }
}

pub(crate) fn scan(library: &Library, options: &ScanOptions) -> Result<ScanStats> {
    let summary = library.scan(options.clone())?.join()?;

    assert!(!summary.cancelled, "the scan reported itself cancelled");
    Ok(summary.stats)
}

pub(crate) fn loaded(playlist: PlaylistId) -> Option<Playing> {
    Some(Playing {
        playlist,
        queue: QueueStamp::default(),
    })
}

pub(crate) fn all(library: &Library) -> Result<Vec<Track>> {
    library.tracks(&TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })
}

pub(crate) fn titles(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|track| track.title.as_str()).collect()
}
