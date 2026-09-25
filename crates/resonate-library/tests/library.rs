use std::{
    env,
    ffi::OsStr,
    fs, io,
    num::NonZeroUsize,
    os,
    os::unix::fs::MetadataExt as _,
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    slice,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_core::{
    AlbumId, ChannelLayout, FrameSpan, Frames, MediaLocation, PlaylistId, QueueStamp,
    ReleaseTrackId, Reordered, Resumable, Resumption, SampleFormat, SampleRate, SourceId, Span,
    StreamSpec, TrackId, WantId,
};
use resonate_library::{
    Album, AlbumQuery, Artist, ArtistMatch, ArtistProfile, ArtistQuery, ArtistRelease, Codec,
    CoverArt, CoverSource, Credit, Cut, Direction, Edit, Encoding, EnrichOptions, EnrichSummary,
    Error, Favoured, FileTags, Fingerprinters, Form, Genre, GroupAsked, GroupMatch, GroupRelease,
    HeldMedium, ImageFormat, ImportOptions, ImportSummary, Isrc, Kept, Layout, Library, LifeSpan,
    Link, ListeningService, LookupOp, Mbid, Medium, Missing, MissingTrack, OrganiseOptions,
    OrganiseSummary, Picturing, Playing, PlaylistFormat, PlaylistOrder, PollOptions, Pruned,
    Recording, RecordingAsked, RecordingMatch, RecordingRelease, Reference, Refusal, Refused,
    Relation, Release, ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, Result,
    RetagOptions, RetagSummary, RowOrder, SavedQuery, ScanOptions, ScanStats, Scrobble, Scrobbler,
    Search, Service, SheetEncoding, Sidecar, SortOrder, Sought, Sources, Suggestion, TagField,
    TagSet, TagSource, Track, TrackQuery, UnheldRelease, Unwritten, Vault, Waits, Window, Wording,
    Written,
};
use resonate_providers::{
    Delivery, Extension, Identity, Obtained, Provider, Providers, Result as ProvidedResult,
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const UTF8: u8 = 3;
const FRONT_COVER: u8 = 3;

const TITLE: &[u8; 4] = b"TIT2";
const ARTIST: &[u8; 4] = b"TPE1";
const ALBUM_ARTIST: &[u8; 4] = b"TPE2";
const ALBUM: &[u8; 4] = b"TALB";
const TRACK: &[u8; 4] = b"TRCK";
const YEAR: &[u8; 4] = b"TDRC";
const GENRE: &[u8; 4] = b"TCON";
const ISRC: &[u8; 4] = b"TSRC";
const LABEL: &[u8; 4] = b"TPUB";
const COMPILATION: &[u8; 4] = b"TCMP";
const USER_TEXT: &[u8; 4] = b"TXXX";
const UNIQUE_FILE_ID: &[u8; 4] = b"UFID";
const MUSICBRAINZ: &str = "http://musicbrainz.org";
const MUSICBRAINZ_ALBUM: &str = "MusicBrainz Album Id";
const MUSICBRAINZ_ARTIST: &str = "MusicBrainz Artist Id";
const MUSICBRAINZ_ALBUM_ARTIST: &str = "MusicBrainz Album Artist Id";
const MUSICBRAINZ_RELEASE_GROUP: &str = "MusicBrainz Release Group Id";
const MUSICBRAINZ_RELEASE_TRACK: &str = "MusicBrainz Release Track Id";
const BARCODE: &str = "BARCODE";
const CATALOG_NUMBER: &str = "CATALOGNUMBER";

const RELEASE: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";
const RELEASE_GROUP: &str = "f5093c06-23e3-404f-aeaa-40f72885ee3a";
const ORBITERS: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const ADA: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const RELEASE_TRACK: &str = "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11";
const HOURS: &str = "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d";
const HOURS_GROUP: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
const LIVE_GROUP: &str = "1b2c3d4e-5f6a-4b7c-8d8e-0f1a2b3c4d5e";
const BEST_OF_GROUP: &str = "2c3d4e5f-6a7b-4c8d-8e9f-1a2b3c4d5e6f";
const SCORE_GROUP: &str = "3d4e5f6a-7b8c-4d9e-8fa0-2b3c4d5e6f7a";
const SINGLE_GROUP: &str = "4e5f6a7b-8c9d-4eaf-8ab1-3c4d5e6f7a8b";
const BROADCAST_GROUP: &str = "5f6a7b8c-9dae-4fb0-8bc2-4d5e6f7a8b9c";
const RECORDING: &str = "b1a9c0de-1111-4222-8333-444455556666";
const ANOTHER_RECORDING: &str = "c2b8d1ef-2222-4333-8444-555566667777";

const CODE: &str = "GB-AYE-71-00195";
const ANOTHER_CODE: &str = "GB-AYE-71-00196";

const THE_FILES_LENGTH: Duration = Duration::from_millis(100);

const A_DAY: Duration = Duration::from_secs(24 * 60 * 60);
const WAITED: Waits = Waits {
    retry_after: A_DAY,
    refused_again_after: A_DAY,
    refresh_after: A_DAY,
};

const ASKED: i64 = 1_700_000_000_000_000_000;
const ANSWERED: i64 = 1_700_000_001_000_000_000;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-library-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, bytes).expect("a writable temporary file");
        path
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Wav {
    bits: u16,
    frames: usize,
    id3: Vec<u8>,
    id3_in_a_chunk: bool,
}

impl Wav {
    fn new() -> Self {
        Self {
            bits: 16,
            frames: 4_410,
            id3: Vec::new(),
            id3_in_a_chunk: false,
        }
    }

    fn id3_in_a_chunk(mut self) -> Self {
        self.id3_in_a_chunk = true;
        self
    }

    fn bits(mut self, bits: u16) -> Self {
        self.bits = bits;
        self
    }

    fn frames(mut self, frames: usize) -> Self {
        self.frames = frames;
        self
    }

    fn text(mut self, id: &[u8; 4], value: &str) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, id, &body);
        self
    }

    fn described(mut self, description: &str, value: &str) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(description.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, USER_TEXT, &body);
        self
    }

    fn identified(mut self, owner: &str, id: &str) -> Self {
        let mut body = owner.as_bytes().to_vec();
        body.push(0);
        body.extend_from_slice(id.as_bytes());
        frame(&mut self.id3, UNIQUE_FILE_ID, &body);
        self
    }

    fn picture(mut self, bytes: &[u8]) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(b"image/png\0");
        body.push(FRONT_COVER);
        body.push(0);
        body.extend_from_slice(bytes);
        frame(&mut self.id3, b"APIC", &body);
        self
    }

    fn build(&self) -> Vec<u8> {
        let stride = usize::from(self.bits / 8);
        let span = 1_i64 << (self.bits - 1);
        let mut data = Vec::with_capacity(self.frames * usize::from(CHANNELS) * stride);
        for n in 0..self.frames * usize::from(CHANNELS) {
            let sample = ((n as i64 % (2 * span - 1)) - span + 1) as i32;
            data.extend_from_slice(&sample.to_le_bytes()[..stride]);
        }

        let block_align = CHANNELS * self.bits / 8;
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1_u16.to_le_bytes());
        fmt.extend_from_slice(&CHANNELS.to_le_bytes());
        fmt.extend_from_slice(&RATE.to_le_bytes());
        fmt.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
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

fn frame(into: &mut Vec<u8>, id: &[u8; 4], body: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&synchsafe(body.len() as u32));
    into.extend_from_slice(&[0, 0]);
    into.extend_from_slice(body);
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn png(size: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend((0..size).map(|n| n as u8));
    bytes
}

fn options(tree: &Tree) -> ScanOptions {
    ScanOptions {
        roots: vec![tree.path().to_path_buf()],
        incremental: true,
        follow_symlinks: false,
        extract_cover_art: true,
        workers: NonZeroUsize::MIN,
    }
}

fn scan(library: &Library, options: &ScanOptions) -> Result<ScanStats> {
    let summary = library.scan(options.clone())?.join()?;
    assert!(!summary.cancelled, "the scan reported itself cancelled");
    Ok(summary.stats)
}

fn loaded(playlist: PlaylistId) -> Option<Playing> {
    Some(Playing {
        playlist,
        queue: QueueStamp::default(),
    })
}

fn all(library: &Library) -> Result<Vec<Track>> {
    library.tracks(&TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })
}

fn resumable(path: &str) -> Resumable {
    Resumable {
        location: MediaLocation::local(path),
        span: None,
    }
}

fn titles(tracks: &[Track]) -> Vec<&str> {
    tracks.iter().map(|track| track.title.as_str()).collect()
}

#[test]
fn a_search_that_matches_nothing_is_answered_in_the_catalogs_own_spelling() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    tree.write(
        "geralt.wav",
        &Wav::new()
            .text(TITLE, "Silver for Monsters")
            .text(ALBUM, "The Witcher 3")
            .text(ARTIST, "Marcin Przybyłowicz")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    assert!(
        library
            .tracks(&TrackQuery {
                text: Some("floid".to_owned()),
                ..TrackQuery::default()
            })?
            .is_empty(),
        "the misspelt word matched something, so there is nothing to suggest"
    );
    assert_eq!(
        library.did_you_mean("floid")?,
        Some("Floyd".to_owned()),
        "a misspelt word was not answered in the spelling the catalog holds"
    );
    assert_eq!(
        library.did_you_mean("przybylowitz")?,
        Some("Przybyłowicz".to_owned()),
        "the marked spelling is the one a pane would draw"
    );
    assert_eq!(
        library.did_you_mean("album:medle")?,
        Some("album:Meddle".to_owned())
    );
    assert_eq!(
        library.did_you_mean("pinkfloyd")?,
        Some("Pink Floyd".to_owned()),
        "a run-together was not split into the two words the catalog holds"
    );
    assert_eq!(
        library.did_you_mean("\"silver fer monsters\"")?,
        Some("\"Silver for Monsters\"".to_owned()),
        "a phrase mistyped in a word too short to correct was not weighed against the whole name"
    );
    assert_eq!(
        library.did_you_mean("pink floy d")?,
        Some("pink Floyd".to_owned()),
        "a word typed in two halves was not run together"
    );
    assert_eq!(
        library.did_you_mean("echoes")?,
        None,
        "a word the catalog holds was corrected to something else"
    );
    assert_eq!(library.did_you_mean("")?, None);
    Ok(())
}

#[test]
fn a_text_search_is_ordered_by_relevance_rather_than_by_album() -> Result<()> {
    let tree = Tree::new();
    for (file, title, album, artist) in [
        ("a.wav", "Zenith", "Zenith", "Ada"),
        ("b.wav", "Alpha Zenith", "Nightfall", "Ben"),
        ("c.wav", "Beta", "Zenith Skies", "Cleo"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let ranked = library.tracks(&TrackQuery {
        text: Some("zenith".to_owned()),
        ..TrackQuery::default()
    })?;
    assert_eq!(
        titles(&ranked).first(),
        Some(&"Zenith"),
        "the track matching in both title and album should rank first, got {:?}",
        titles(&ranked)
    );

    let by_title = library.tracks(&TrackQuery {
        text: Some("zenith".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })?;
    assert_eq!(titles(&by_title), vec!["Alpha Zenith", "Beta", "Zenith"]);
    Ok(())
}

#[test]
fn relevance_falls_back_to_album_order_when_there_is_no_text_to_rank() -> Result<()> {
    let tree = Tree::new();
    for (file, title, track) in [("a.wav", "second", "2"), ("b.wav", "first", "1")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, "One Album")
                .text(TRACK, track)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let listed = library.tracks(&TrackQuery::default())?;
    assert_eq!(titles(&listed), vec!["first", "second"]);
    Ok(())
}

#[test]
fn an_empty_library_migrates_and_reads_back_as_empty() -> Result<()> {
    let library = Library::open_in_memory()?;

    assert!(all(&library)?.is_empty());
    assert!(library.albums(&AlbumQuery::default())?.is_empty());
    assert!(library.roots()?.is_empty());
    Ok(())
}

#[test]
fn a_scan_stores_what_the_probe_found() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "one.wav",
        &Wav::new()
            .bits(24)
            .frames(2_205)
            .text(TITLE, "Coma Ecliptic")
            .text(ARTIST, "Between the Buried and Me")
            .text(ALBUM, "Coma Ecliptic")
            .text(TRACK, "4")
            .build(),
    );

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.discovered, 1);
    assert_eq!(stats.added, 1);
    assert_eq!(stats.failed.total(), 0);

    let tracks = all(&library)?;
    let track = tracks.first().expect("the scan added one track");
    assert_eq!(track.title, "Coma Ecliptic");
    assert_eq!(track.artist.as_deref(), Some("Between the Buried and Me"));
    assert_eq!(track.track_number, Some(4));
    assert_eq!(
        track.spec,
        StreamSpec::new(
            SampleRate::new(RATE)?,
            ChannelLayout::Stereo,
            SampleFormat::S24
        )
    );
    assert_eq!(track.duration.map(|frames| frames.get()), Some(2_205));
    assert_eq!(track.codec, Codec::Pcm);
    assert!(track.album_id.is_some());
    Ok(())
}

#[test]
fn every_sample_format_survives_the_round_trip() -> Result<()> {
    let tree = Tree::new();
    for (index, bits) in [16_u16, 24, 32].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .bits(bits)
                .text(TITLE, &format!("track {index}"))
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let formats: Vec<SampleFormat> = all(&library)?
        .iter()
        .map(|track| track.spec.format)
        .collect();
    assert_eq!(
        formats,
        vec![SampleFormat::S16, SampleFormat::S24, SampleFormat::S32]
    );
    Ok(())
}

#[test]
fn a_second_scan_of_an_untouched_tree_changes_nothing() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "one").build());
    tree.write("two.wav", &Wav::new().text(TITLE, "two").build());

    let library = Library::open_in_memory()?;
    let first = scan(&library, &options(&tree))?;
    assert_eq!(first.added, 2);

    let second = scan(&library, &options(&tree))?;
    assert_eq!(second.discovered, 2);
    assert_eq!(second.processed, 2);
    assert_eq!(second.added, 0);
    assert_eq!(second.updated, 0);
    assert_eq!(second.removed, 0);
    assert_eq!(all(&library)?.len(), 2);
    Ok(())
}

#[test]
fn a_rewritten_file_is_probed_again() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "before").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    tree.write(
        "one.wav",
        &Wav::new().frames(8_820).text(TITLE, "after").build(),
    );
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.added, 0);
    assert_eq!(stats.updated, 1);

    let tracks = all(&library)?;
    let track = tracks.first().expect("the track is still there");
    assert_eq!(track.title, "after");
    assert_eq!(track.duration.map(|frames| frames.get()), Some(8_820));
    Ok(())
}

#[test]
fn a_file_that_left_the_tree_leaves_the_library() -> Result<()> {
    let tree = Tree::new();
    let gone = tree.write("gone.wav", &Wav::new().text(TITLE, "gone").build());
    tree.write("kept.wav", &Wav::new().text(TITLE, "kept").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(all(&library)?.len(), 2);

    fs::remove_file(&gone).expect("the fixture is removable");
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 1);
    let tracks = all(&library)?;
    assert_eq!(tracks.len(), 1);
    assert_eq!(
        tracks.first().map(|track| track.title.as_str()),
        Some("kept")
    );
    Ok(())
}

#[test]
fn a_file_the_watch_heard_taken_away_is_forgotten_without_a_scan() -> Result<()> {
    let tree = Tree::new();
    let gone = tree.write("album/gone.wav", &Wav::new().text(TITLE, "gone").build());
    tree.write("album/kept.wav", &Wav::new().text(TITLE, "kept").build());
    tree.write("other/one.wav", &Wav::new().text(TITLE, "one").build());
    tree.write("other/two.wav", &Wav::new().text(TITLE, "two").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(all(&library)?.len(), 4);

    fs::remove_file(&gone).expect("the fixture is removable");
    fs::remove_dir_all(tree.path().join("other")).expect("the folder is removable");
    let root = tree.path().canonicalize().expect("the tree is there");
    let named = [
        root.join("album/gone.wav"),
        root.join("other"),
        root.join("album/kept.wav"),
    ];

    assert_eq!(library.forget_the_gone(&named)?, 3);
    assert_eq!(titles(&all(&library)?), vec!["kept"]);
    assert_eq!(library.forget_the_gone(&named)?, 0);
    Ok(())
}

#[test]
fn tracks_group_into_one_album_under_their_album_artist() -> Result<()> {
    let tree = Tree::new();
    for (index, title) in ["Lunar", "Solar", "Stellar"].into_iter().enumerate() {
        tree.write(
            &format!("disc/{index}.wav"),
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, &format!("Guest {index}"))
                .text(ALBUM_ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(YEAR, "1998-03-02")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1);
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.title, "Orbits");
    assert_eq!(album.track_count, 3);
    assert_eq!(album.year, Some(1998));
    assert!(album.artist_id.is_some());

    let results = library.search("Orbiters", 10)?;
    assert_eq!(results.artists.len(), 1);
    let artist = results.artists.first().expect("the album artist is stored");
    assert_eq!(artist.name, "The Orbiters");
    assert_eq!(artist.track_count, 3);
    assert_eq!(artist.album_count, 1);

    let by_album = library.tracks(&TrackQuery {
        album: Some(album.id),
        ..TrackQuery::default()
    })?;
    assert_eq!(by_album.len(), 3);

    let listed = library.artists(&ArtistQuery::default())?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed.first().map(|entry| entry.id), Some(artist.id));
    Ok(())
}

#[test]
fn a_compilation_stays_one_album_while_its_performers_stay_their_own() -> Result<()> {
    let tree = Tree::new();
    for (index, (title, artist)) in [("Dawn", "Ada"), ("Noon", "Ben"), ("Dusk", "Cleo")]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("various/{index}.wav"),
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "a compilation split into {albums:?}");
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.title, "Hours");
    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "a compilation belongs to no single artist"
    );

    let artists = library.artists(&ArtistQuery::default())?;
    let mut names: Vec<&str> = artists.iter().map(|artist| artist.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["Ada", "Ben", "Cleo"]);
    for artist in &artists {
        assert_eq!(artist.track_count, 1, "{} lost its track", artist.name);
    }
    Ok(())
}

#[test]
fn a_release_id_holds_together_a_compilation_that_carries_no_flag() -> Result<()> {
    let performers = [("Dawn", "Ada"), ("Noon", "Ben"), ("Dusk", "Cleo")];
    let release = "8f4f0e21-1b0f-4a3f-9a6c-0d3f2a5c7e11";

    let tagged = Tree::new();
    let untagged = Tree::new();
    for (index, (title, artist)) in performers.into_iter().enumerate() {
        let track = Wav::new()
            .text(TITLE, title)
            .text(ARTIST, artist)
            .text(ALBUM, "Hours");
        untagged.write(&format!("{index}.wav"), &track.build());
        tagged.write(
            &format!("{index}.wav"),
            &track.described(MUSICBRAINZ_ALBUM, release).build(),
        );
    }

    let loose = Library::open_in_memory()?;
    scan(&loose, &options(&untagged))?;
    assert_eq!(
        loose.albums(&AlbumQuery::default())?.len(),
        3,
        "a compilation with nothing to group it stayed together anyway"
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tagged))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "a release id did not group {albums:?}");
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.title, "Hours");
    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "a release its performers disagree about belongs to no single artist"
    );

    let artists = library.artists(&ArtistQuery::default())?;
    let mut names: Vec<&str> = artists.iter().map(|artist| artist.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, vec!["Ada", "Ben", "Cleo"]);
    for artist in &artists {
        assert_eq!(artist.track_count, 1, "{} lost its track", artist.name);
    }
    Ok(())
}

#[test]
fn a_release_every_track_agrees_about_keeps_its_artist() -> Result<()> {
    let tree = Tree::new();
    let release = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";
    for index in 0..3 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(YEAR, "1998-03-02")
                .described(MUSICBRAINZ_ALBUM, release)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1);
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.track_count, 3);
    assert_eq!(album.year, Some(1998));
    assert!(
        album.artist_id.is_some(),
        "an album nobody disagreed about lost its artist"
    );
    Ok(())
}

#[test]
fn two_releases_that_share_a_title_and_an_artist_are_told_apart_by_their_ids() -> Result<()> {
    let tree = Tree::new();
    for (index, release) in [
        "aaaaaaaa-0000-0000-0000-000000000001",
        "bbbbbbbb-0000-0000-0000-000000000002",
    ]
    .into_iter()
    .enumerate()
    {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "Live")
                .described(MUSICBRAINZ_ALBUM, release)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(
        albums.len(),
        2,
        "two releases sharing a title were merged into {albums:?}"
    );
    for album in &albums {
        assert_eq!(album.track_count, 1);
        assert!(album.artist_id.is_some());
    }
    Ok(())
}

#[test]
fn an_album_artist_outranks_the_compilation_flag() -> Result<()> {
    let tree = Tree::new();
    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, "The Curators")
                .text(ALBUM, "Selected")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1);
    assert_eq!(albums.first().map(|album| album.track_count), Some(2));
    Ok(())
}

#[test]
fn two_albums_loose_in_a_root_sharing_a_title_stay_apart_under_their_own_artists() -> Result<()> {
    let tree = Tree::new();
    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Greatest Hits")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}

#[test]
fn a_folder_whose_files_name_different_album_artists_is_still_one_album() -> Result<()> {
    let tree = Tree::new();
    for (index, owner) in ["P. T. Adamczyk", "Marcin Przybyłowicz", "Kerry Eurodyne"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("Cyberpunk 2077/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, owner)
                .text(ALBUM_ARTIST, owner)
                .text(ALBUM, "Cyberpunk 2077")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(
        albums.len(),
        1,
        "one sleeve became an album per album artist: {albums:?}"
    );
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "an album its tracks disagree about belongs to no one singer"
    );
    Ok(())
}

#[test]
fn a_track_that_moves_between_the_folders_of_one_set_stays_under_the_album_it_was_in() -> Result<()>
{
    let tree = Tree::new();
    for (folder, owner) in [("Part One", "Ada"), ("Part Two", "Ada")] {
        tree.write(
            &format!("{folder}/one.wav"),
            &Wav::new()
                .text(TITLE, folder)
                .text(ARTIST, owner)
                .text(ALBUM_ARTIST, owner)
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 1);
    assert_eq!(
        keys(&database).len(),
        3,
        "the album is not named by its owner and by both folders"
    );
    Ok(())
}

#[test]
fn tracks_sharing_a_folder_are_one_album_however_their_singers_differ() -> Result<()> {
    let tree = Tree::new();
    for (index, artist) in ["Ada", "Ben", "Ada"].into_iter().enumerate() {
        tree.write(
            &format!("Under One Roof/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Under One Roof")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "one album split into {albums:?}");
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.track_count, 3);
    assert_eq!(album.artist_count, 2);
    assert_eq!(
        album.artist_id, None,
        "an album its tracks disagree about belongs to no one singer"
    );
    Ok(())
}

#[test]
fn two_albums_sharing_a_title_in_folders_of_their_own_stay_apart() -> Result<()> {
    let tree = Tree::new();
    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{artist}/Greatest Hits/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Greatest Hits")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}

#[test]
fn the_discs_of_a_set_in_folders_of_their_own_are_still_one_album() -> Result<()> {
    let tree = Tree::new();
    for (index, disc) in ["CD1", "Disc 2", "disk-03"].into_iter().enumerate() {
        tree.write(
            &format!("The Wall/{disc}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "a disc set split into {albums:?}");
    assert_eq!(albums.first().map(|album| album.track_count), Some(3));
    Ok(())
}

#[test]
fn the_discs_of_a_set_numbered_in_words_gather_the_way_numbered_ones_do() -> Result<()> {
    let tree = Tree::new();
    for (index, disc) in ["Disc One", "Second Disc", "CD Three"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("The Wall/{disc}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "a disc set split into {albums:?}");
    assert_eq!(albums.first().map(|album| album.track_count), Some(3));
    Ok(())
}

#[test]
fn a_folder_whose_name_merely_starts_like_a_disc_is_an_album_of_its_own() -> Result<()> {
    let tree = Tree::new();
    for (index, folder) in ["Discovery", "Disconnected"].into_iter().enumerate() {
        tree.write(
            &format!("Ada/{folder}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, folder)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 2, "two albums gathered into {albums:?}");
    Ok(())
}

#[test]
fn an_album_artist_still_gathers_discs_the_folders_keep_apart() -> Result<()> {
    let tree = Tree::new();
    for (index, folder) in ["The Wall Part One", "The Wall Part Two"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("{folder}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, &format!("guest {index}"))
                .text(ALBUM_ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "an album artist did not gather {albums:?}");
    Ok(())
}

#[test]
fn an_album_names_its_artist_on_its_own_row() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Blue Light")
            .text(ALBUM, "Harbour")
            .text(ARTIST, "Ida Fenn")
            .text(ALBUM_ARTIST, "Ida Fenn")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("the scanned album");
    assert_eq!(album.title, "Harbour");
    assert_eq!(album.artist.as_deref(), Some("Ida Fenn"));
    assert!(album.artist_id.is_some(), "the album should name an artist");
    Ok(())
}

#[test]
fn an_album_names_its_artist_although_the_artists_listing_left_it_out() -> Result<()> {
    let tree = Tree::new();
    for (file, title, album, artist) in [
        ("a.wav", "Blue Light", "Harbour", "Ida Fenn"),
        ("b.wav", "Slack Water", "Estuary", "Omar Pike"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let listed = library.artists(&ArtistQuery {
        limit: Some(1),
        ..ArtistQuery::default()
    })?;
    assert_eq!(
        listed.len(),
        1,
        "the cap should hold the listing to one artist"
    );
    let left_out = listed[0].name.clone();

    let albums = library.albums(&AlbumQuery::default())?;
    for album in &albums {
        assert!(
            album.artist.is_some(),
            "every album should name its artist off its own row, {} did not",
            album.title
        );
    }
    assert!(
        albums
            .iter()
            .any(|album| album.artist.as_deref().is_some_and(|name| name != left_out)),
        "the album whose artist the capped listing left out should still name it"
    );
    Ok(())
}

#[test]
fn an_artists_detail_names_the_artist_it_is_about() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Blue Light")
            .text(ALBUM, "Harbour")
            .text(ARTIST, "Ida Fenn")
            .text(ALBUM_ARTIST, "Ida Fenn")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let artists = library.artists(&ArtistQuery::default())?;
    let id = artists.first().expect("the scanned artist").id;
    let detail = library.artist_detail(id)?.expect("the artist detail");
    assert_eq!(detail.name, "Ida Fenn");
    Ok(())
}

#[test]
fn albums_and_artists_are_ordered_by_index_rank_rather_than_by_name() -> Result<()> {
    let tree = Tree::new();
    for (file, title, album, artist) in [
        ("a.wav", "Filler", "Zephyr Sessions", "Wanda Zephyr"),
        ("b.wav", "Zephyr", "Zephyr", "Zephyr"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery {
        text: Some("zephyr".to_owned()),
        ..AlbumQuery::default()
    })?;
    assert_eq!(
        albums.first().map(|album| album.title.as_str()),
        Some("Zephyr"),
        "the album matching in every field should rank first, got {:?}",
        albums.iter().map(|album| &album.title).collect::<Vec<_>>()
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("zephyr".to_owned()),
        ..ArtistQuery::default()
    })?;
    assert_eq!(
        artists.first().map(|artist| artist.name.as_str()),
        Some("Zephyr"),
        "got {:?}",
        artists
            .iter()
            .map(|artist| &artist.name)
            .collect::<Vec<_>>()
    );

    let searched = library.search("zephyr", 10)?;
    assert_eq!(
        searched.albums.first().map(|album| album.title.as_str()),
        Some("Zephyr")
    );
    Ok(())
}

#[test]
fn an_unmatched_album_query_still_lists_every_album_by_title() -> Result<()> {
    let tree = Tree::new();
    for (file, album) in [("a.wav", "Beta"), ("b.wav", "Alpha")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, album)
                .text(ALBUM, album)
                .text(ALBUM_ARTIST, album)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Beta"]
    );

    let artists = library.artists(&ArtistQuery::default())?;
    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Beta"]
    );
    Ok(())
}

#[test]
fn removing_a_root_takes_its_tracks_and_their_index_with_it() -> Result<()> {
    let kept = Tree::new();
    kept.write(
        "keep.wav",
        &Wav::new()
            .text(TITLE, "Keeper")
            .text(ALBUM, "Kept")
            .text(ALBUM_ARTIST, "Kept Artist")
            .build(),
    );
    let dropped = Tree::new();
    dropped.write(
        "drop.wav",
        &Wav::new()
            .text(TITLE, "Dropper")
            .text(ALBUM, "Dropped")
            .text(ALBUM_ARTIST, "Dropped Artist")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(
        &library,
        &ScanOptions {
            roots: vec![kept.path().to_path_buf(), dropped.path().to_path_buf()],
            ..options(&kept)
        },
    )?;
    assert_eq!(all(&library)?.len(), 2);
    assert_eq!(library.roots()?.len(), 2);

    assert!(library.remove_root(dropped.path())?);
    assert!(
        !library.remove_root(dropped.path())?,
        "removing a root twice should report nothing removed"
    );

    assert_eq!(titles(&all(&library)?), vec!["Keeper"]);
    assert_eq!(library.roots()?.len(), 1);
    assert_eq!(library.search("Dropper", 10)?.tracks.len(), 0);
    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 1);
    assert_eq!(library.artists(&ArtistQuery::default())?.len(), 1);
    Ok(())
}

#[test]
fn removing_a_root_whose_directory_is_gone_still_clears_it() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "Gone").build());
    let root = tree.path().canonicalize().expect("a real directory");

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    fs::remove_dir_all(&root).expect("a removable directory");

    assert!(library.remove_root(&root)?);
    assert!(all(&library)?.is_empty());
    assert!(library.roots()?.is_empty());
    Ok(())
}

#[test]
fn search_matches_a_track_through_any_of_its_indexed_fields() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "one.wav",
        &Wav::new()
            .text(TITLE, "Halcyon Days")
            .text(ARTIST, "Elder")
            .text(ALBUM, "Reflections of a Floating World")
            .build(),
    );
    tree.write(
        "two.wav",
        &Wav::new()
            .text(TITLE, "Sanctuary")
            .text(ARTIST, "Yob")
            .text(ALBUM, "Clearing the Path")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    for query in [
        "Halcyon",
        "halcy",
        "HALCYON",
        "hALCYON",
        "Elder",
        "ELDER",
        "floating",
        "FLOATING WORLD",
        "title:HALCYON",
        "artist:ELDER",
    ] {
        let results = library.search(query, 10)?;
        assert_eq!(results.tracks.len(), 1, "{query} matched the wrong count");
        assert_eq!(
            results.tracks.first().map(|track| track.title.as_str()),
            Some("Halcyon Days"),
            "{query} matched the wrong track"
        );
    }

    assert_eq!(library.search("Sanctuary", 10)?.tracks.len(), 1);
    assert_eq!(library.search("nothing here", 10)?.tracks.len(), 0);
    assert_eq!(library.search("   ", 10)?.tracks.len(), 0);
    Ok(())
}

#[test]
fn a_name_is_found_however_its_letters_are_marked_or_cased() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "one.wav",
        &Wav::new()
            .text(TITLE, "Kıskanç")
            .text(ARTIST, "Marcin Przybyłowicz")
            .text(ALBUM, "Straße")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    for query in [
        "Kıskanç",
        "KISKANÇ",
        "kiskanc",
        "KISKANC",
        "kıskanc",
        "title:KISKANC",
        "Przybylowicz",
        "Przybyłowicz",
        "artist:przybylowicz",
        "Strasse",
        "Straße",
    ] {
        let results = library.search(query, 10)?;
        assert_eq!(
            results.tracks.first().map(|track| track.title.as_str()),
            Some("Kıskanç"),
            "{query} did not reach the track"
        );
    }

    assert_eq!(library.search("kismet", 10)?.tracks.len(), 0);
    Ok(())
}

#[test]
fn search_text_that_would_break_the_index_query_is_treated_as_words() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "Quotes and OR").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    for query in ["\"quotes\"", "quotes OR", "quotes*(", "AND OR NOT"] {
        assert!(
            library.search(query, 10).is_ok(),
            "{query} was passed through to the index"
        );
    }
    assert_eq!(library.search("\"quotes\"", 10)?.tracks.len(), 1);
    Ok(())
}

#[test]
fn cover_art_is_extracted_once_and_read_back_whole() -> Result<()> {
    let art = png(512);
    let tree = Tree::new();
    for index in 0..3 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ALBUM, "Covered")
                .text(ALBUM_ARTIST, "Someone")
                .picture(&art)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");
    assert!(album.has_cover_art);
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    Ok(())
}

#[test]
fn an_album_takes_the_cover_any_of_its_tracks_carries_rather_than_only_the_first() -> Result<()> {
    let art = png(512);
    let tree = Tree::new();
    for index in 0..6 {
        let mut file = Wav::new()
            .text(TITLE, &format!("track {index}"))
            .text(ALBUM, "Covered")
            .text(ALBUM_ARTIST, "Someone");
        if index == 0 {
            file = file.picture(&art);
        }
        tree.write(&format!("{index}.wav"), &file.build());
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");
    assert!(
        album.has_cover_art,
        "an album kept no cover although one of its tracks carries one"
    );
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    Ok(())
}

#[test]
fn an_album_takes_the_year_any_of_its_tracks_declares_rather_than_only_the_first() -> Result<()> {
    let tree = Tree::new();
    for index in 0..6 {
        let mut file = Wav::new()
            .text(TITLE, &format!("track {index}"))
            .text(ALBUM, "Dated")
            .text(ALBUM_ARTIST, "Someone");
        if index == 0 {
            file = file.text(YEAR, "1971-10-30");
        }
        tree.write(&format!("{index}.wav"), &file.build());
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");
    assert_eq!(album.year, Some(1971));
    assert_eq!(matched(&library, "year:1971")?.len(), 6);
    Ok(())
}

#[test]
fn a_date_written_with_no_separators_still_names_the_year_it_starts_with() -> Result<()> {
    let tree = Tree::new();
    for (file, written) in [
        ("compact.wav", "19750601"),
        ("month.wav", "197506"),
        ("bare.wav", "1975"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, file)
                .text(ALBUM, file)
                .text(YEAR, written)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 3);
    for album in &albums {
        assert_eq!(album.year, Some(1975), "{} kept no year", album.title);
    }
    Ok(())
}

#[test]
fn cover_art_is_left_alone_when_the_scan_did_not_ask_for_it() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "one.wav",
        &Wav::new()
            .text(TITLE, "bare")
            .text(ALBUM, "Bare")
            .picture(&png(64))
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(
        &library,
        &ScanOptions {
            extract_cover_art: false,
            ..options(&tree)
        },
    )?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");
    assert!(!album.has_cover_art);
    assert_eq!(library.cover_art(album.id)?, None);
    Ok(())
}

#[test]
fn a_failed_file_is_counted_as_what_went_wrong_with_it() -> Result<()> {
    let tree = Tree::new();
    tree.write("good.wav", &Wav::new().text(TITLE, "good").build());
    tree.write("lying.flac", b"this is not a flac file at all");
    let whole = Wav::new().text(TITLE, "cut short").build();
    tree.write("cut.wav", &whole[..24]);

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.added, 1);
    assert_eq!(stats.failed.total(), 2);
    assert_eq!(
        stats.failed.misnamed, 1,
        "a file whose bytes are not the container its extension promised is misnamed, got {:?}",
        stats.failed
    );
    assert_eq!(
        stats.failed.unreadable, 1,
        "a file whose container is recognised and whose bytes are not readable is unreadable, \
         got {:?}",
        stats.failed
    );
    assert_eq!(stats.failed.unnamed, 0);
    Ok(())
}

#[test]
fn a_file_that_will_not_probe_is_counted_and_stepped_over() -> Result<()> {
    let tree = Tree::new();
    tree.write("good.wav", &Wav::new().text(TITLE, "good").build());
    tree.write("broken.flac", b"this is not a flac file at all");
    tree.write("ignored.txt", b"not audio, never discovered");

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.discovered, 2);
    assert_eq!(stats.failed.total(), 1);
    assert_eq!(stats.added, 1);
    assert_eq!(all(&library)?.len(), 1);
    Ok(())
}

#[test]
fn a_track_with_no_title_tag_is_named_after_its_file() -> Result<()> {
    let tree = Tree::new();
    tree.write("nested/Unnamed Take.wav", &Wav::new().build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let tracks = all(&library)?;
    assert_eq!(
        tracks.first().map(|track| track.title.as_str()),
        Some("Unnamed Take")
    );
    assert_eq!(library.search("Unnamed", 10)?.tracks.len(), 1);
    Ok(())
}

#[test]
fn nested_directories_are_walked_to_the_bottom() -> Result<()> {
    let tree = Tree::new();
    tree.write("a/b/c/d/deep.wav", &Wav::new().text(TITLE, "deep").build());
    tree.write("a/shallow.wav", &Wav::new().text(TITLE, "shallow").build());

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.discovered, 2);
    assert_eq!(all(&library)?.len(), 2);
    Ok(())
}

#[test]
fn scanning_registers_the_root_it_was_given() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let roots = library.roots()?;
    assert_eq!(roots.len(), 1);
    assert_eq!(
        roots.first().map(PathBuf::as_path),
        Some(
            tree.path()
                .canonicalize()
                .expect("the fixture tree exists")
                .as_path()
        )
    );
    Ok(())
}

#[test]
fn a_root_that_is_not_a_directory_is_refused() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("one.wav", &Wav::new().build());
    let library = Library::open_in_memory()?;

    assert!(matches!(
        library.add_root(&file),
        Err(Error::RootNotADirectory { .. })
    ));

    let handle = library.scan(ScanOptions {
        roots: vec![file],
        ..options(&tree)
    })?;
    assert!(matches!(
        handle.join(),
        Err(Error::RootNotADirectory { .. })
    ));
    Ok(())
}

#[test]
fn a_scan_of_what_is_held_passes_over_a_root_that_is_gone_or_no_longer_held() -> Result<()> {
    let kept = Tree::new();
    let unplugged = Tree::new();
    let dropped = Tree::new();
    kept.write("one.wav", &Wav::new().build());
    unplugged.write("two.wav", &Wav::new().build());
    dropped.write("three.wav", &Wav::new().build());
    let library = Library::open_in_memory()?;
    for tree in [&kept, &unplugged, &dropped] {
        scan(&library, &options(tree))?;
    }
    let canonical = |tree: &Tree| tree.path().canonicalize().expect("the tree exists");
    let (kept_root, unplugged_root, dropped_root) =
        (canonical(&kept), canonical(&unplugged), canonical(&dropped));

    assert!(library.remove_root(&dropped_root)?);
    fs::remove_dir_all(&unplugged_root).expect("the tree taken away");
    kept.write("four.wav", &Wav::new().build());

    assert!(
        library
            .scan_what_is_held(ScanOptions {
                roots: vec![dropped_root.clone()],
                ..options(&kept)
            })?
            .is_none()
    );
    let summary = library
        .scan_what_is_held(ScanOptions {
            roots: vec![unplugged_root.clone(), kept_root.clone(), dropped_root],
            ..options(&kept)
        })?
        .expect("a held root to walk")
        .join()?;

    assert_eq!(summary.stats.added, 1);
    let mut held = vec![kept_root, unplugged_root];
    held.sort();
    assert_eq!(library.roots()?, held);
    assert_eq!(all(&library)?.len(), 3);
    Ok(())
}

#[test]
fn a_root_inside_a_root_is_refused() -> Result<()> {
    let tree = Tree::new();
    tree.write("rock/one.wav", &Wav::new().build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let inside = tree.path().join("rock");
    assert!(matches!(
        library.add_root(&inside),
        Err(Error::RootInsideRoot { .. })
    ));

    let handle = library.scan(ScanOptions {
        roots: vec![inside],
        ..options(&tree)
    })?;
    assert!(matches!(handle.join(), Err(Error::RootInsideRoot { .. })));
    assert_eq!(library.roots()?.len(), 1);
    Ok(())
}

#[test]
fn a_root_takes_in_the_one_it_covers_and_the_counts_it_held() -> Result<()> {
    let tree = Tree::new();
    tree.write("rock/one.wav", &Wav::new().text(TITLE, "One").build());
    tree.write("two.wav", &Wav::new().text(TITLE, "Two").build());
    let library = Library::open_in_memory()?;

    scan(
        &library,
        &ScanOptions {
            roots: vec![tree.path().join("rock")],
            ..options(&tree)
        },
    )?;
    let one = MediaLocation::local(tree.path().join("rock/one.wav"));
    assert_eq!(
        library
            .track_played(&one, None)?
            .map(|counted| counted.track.plays),
        Some(1)
    );

    scan(&library, &options(&tree))?;

    assert_eq!(
        library.roots()?,
        vec![tree.path().canonicalize().expect("the fixture tree exists")]
    );
    assert_eq!(titles(&all(&library)?), vec!["One", "Two"]);
    assert_eq!(
        library
            .track_at(tree.path().join("rock/one.wav").as_path(), None)?
            .map(|row| row.plays),
        Some(1)
    );
    Ok(())
}

#[test]
fn a_directory_past_the_depth_limit_is_stepped_past_and_the_prune_still_runs() -> Result<()> {
    let tree = Tree::new();
    let deep: String = "down/".repeat(40);
    tree.write(
        &format!("{deep}far.wav"),
        &Wav::new().text(TITLE, "Far").build(),
    );
    tree.write("near.wav", &Wav::new().text(TITLE, "Near").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(titles(&all(&library)?), vec!["Near"]);

    fs::remove_file(tree.path().join("near.wav")).expect("the fixture file is writable");
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 1);
    assert!(all(&library)?.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn two_links_to_one_directory_are_walked_once_rather_than_read_as_a_cycle() -> Result<()> {
    let tree = Tree::new();
    let shared = Tree::new();
    tree.write("albums/keep.wav", &Wav::new().text(TITLE, "Keep").build());
    shared.write("one.wav", &Wav::new().text(TITLE, "One").build());

    for named in ["albums/first", "albums/second"] {
        os::unix::fs::symlink(shared.path(), tree.path().join(named))
            .expect("the fixture tree takes a link");
    }

    let library = Library::open_in_memory()?;
    scan(
        &library,
        &ScanOptions {
            follow_symlinks: true,
            ..options(&tree)
        },
    )?;

    assert_eq!(titles(&all(&library)?), vec!["Keep", "One"]);
    Ok(())
}

#[test]
fn a_count_answers_for_the_whole_match_where_a_page_answers_for_itself() -> Result<()> {
    const MP3: i64 = 7;

    let tree = Tree::new();
    for index in 0..5 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, if index < 2 { "One" } else { "Other" })
                .text(ALBUM, &format!("album {}", index / 2))
                .build(),
        );
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    beside(&database)
        .execute(
            "UPDATE tracks SET codec = ?1 WHERE title IN ('track 0', 'track 1')",
            [MP3],
        )
        .expect("the catalog takes a codec");
    assert_eq!(
        library
            .tracks(&TrackQuery {
                sort: SortOrder::Title,
                reading: SortOrder::Title.reads(),
                ..TrackQuery::default()
            })?
            .iter()
            .filter(|track| track.codec == Codec::Mp3)
            .count(),
        2,
        "the code this test writes is not the one the catalog reads as mp3"
    );

    let paged = TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(2),
        ..TrackQuery::default()
    };
    assert_eq!(library.tracks(&paged)?.len(), 2);

    let whole = library.measured(&TrackQuery {
        limit: None,
        ..paged.clone()
    })?;
    assert_eq!(whole.rows, 5);
    assert_eq!(
        whole.lossless, 3,
        "the two rows written as mp3 were counted lossless"
    );
    assert!(whole.length.is_some());

    let capped = library.measured(&paged)?;
    assert_eq!(
        capped.rows, 2,
        "a measurement given a limit answers for the page it names"
    );

    let albums = AlbumQuery {
        limit: Some(1),
        ..AlbumQuery::default()
    };
    assert_eq!(library.albums(&albums)?.len(), 1);
    assert_eq!(library.albums_counted(&albums)?, 3);

    let artists = ArtistQuery {
        limit: Some(1),
        ..ArtistQuery::default()
    };
    assert_eq!(library.artists(&artists)?.len(), 1);
    assert_eq!(library.artists_counted(&artists)?, 2);

    let narrowed = ArtistQuery {
        text: Some("Other".to_owned()),
        ..ArtistQuery::default()
    };
    assert_eq!(
        library.artists_counted(&narrowed)?,
        1,
        "a count over a narrowed listing counts what the search matches"
    );
    Ok(())
}

#[test]
fn limit_and_offset_page_through_a_sorted_result() -> Result<()> {
    let tree = Tree::new();
    for index in 0..5 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new().text(TITLE, &format!("track {index}")).build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let page = library.tracks(&TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(2),
        offset: 2,
        ..TrackQuery::default()
    })?;

    assert_eq!(page.len(), 2);
    assert_eq!(
        page.iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["track 2", "track 3"]
    );
    Ok(())
}

#[test]
fn cancelling_a_scan_stops_it_before_it_prunes() -> Result<()> {
    let tree = Tree::new();
    for index in 0..200 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new().frames(64).text(TITLE, "same").build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(all(&library)?.len(), 200);

    let handle = library.scan(ScanOptions {
        incremental: false,
        ..options(&tree)
    })?;
    handle.cancel();
    let summary = handle.join()?;

    assert!(summary.cancelled, "the scan outran the cancel");
    assert_eq!(summary.stats.removed, 0);
    assert_eq!(all(&library)?.len(), 200);
    Ok(())
}

#[test]
fn a_root_the_scan_was_not_given_is_tidied_of_the_files_that_have_gone() -> Result<()> {
    let here = Tree::new();
    let there = Tree::new();
    here.write("here.wav", &Wav::new().text(TITLE, "here").build());
    let gone = there.write("gone.wav", &Wav::new().text(TITLE, "gone").build());
    there.write("stays.wav", &Wav::new().text(TITLE, "stays").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&here))?;
    scan(&library, &options(&there))?;
    assert_eq!(titles(&all(&library)?), vec!["gone", "here", "stays"]);

    fs::remove_file(&gone).expect("the fixture file is removable");
    let stats = scan(&library, &options(&here))?;

    assert_eq!(
        stats.removed, 1,
        "a root this scan did not walk kept a row for a file that has gone"
    );
    assert_eq!(titles(&all(&library)?), vec!["here", "stays"]);
    Ok(())
}

#[test]
fn a_root_that_is_not_there_is_left_alone_rather_than_emptied() -> Result<()> {
    let here = Tree::new();
    let unplugged = Tree::new();
    here.write("here.wav", &Wav::new().text(TITLE, "here").build());
    unplugged.write("away.wav", &Wav::new().text(TITLE, "away").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&here))?;
    scan(&library, &options(&unplugged))?;

    fs::remove_dir_all(unplugged.path()).expect("the fixture tree is removable");
    let stats = scan(
        &library,
        &ScanOptions {
            roots: Vec::new(),
            ..options(&here)
        },
    )?;

    assert_eq!(
        stats.removed, 0,
        "a root that is not there was emptied rather than left alone"
    );
    assert_eq!(titles(&all(&library)?), vec!["away", "here"]);
    Ok(())
}

#[test]
fn the_progress_a_watcher_holds_both_reports_a_scan_and_stops_it() -> Result<()> {
    let tree = Tree::new();
    for index in 0..200 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new().frames(64).text(TITLE, "same").build(),
        );
    }

    let library = Library::open_in_memory()?;
    let handle = library.scan(options(&tree))?;
    let watched = Arc::clone(handle.progress());

    watched.cancel();
    let summary = handle.join()?;

    assert!(watched.is_cancelled(), "the progress forgot the cancel");
    assert!(summary.cancelled, "the scan outran the cancel");
    assert_eq!(watched.snapshot(), summary.stats);
    Ok(())
}

#[test]
fn a_track_reads_back_by_its_own_id() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "findable").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let tracks = all(&library)?;
    let track = tracks.first().expect("the scan added one track");
    let found = library.track(track.id)?;

    assert_eq!(found.as_ref(), Some(track));
    assert_eq!(found.and_then(|track| track.album_id), None);
    Ok(())
}

#[test]
fn a_file_library_survives_being_closed_and_reopened() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "persistent").build());
    let database = tree.path().join("library.db");

    {
        let library = Library::open(&database)?;
        scan(&library, &options(&tree))?;
        assert_eq!(all(&library)?.len(), 1);
    }

    let library = Library::open(&database)?;
    let tracks = all(&library)?;
    assert_eq!(
        tracks.first().map(|track| track.title.as_str()),
        Some("persistent")
    );
    assert_eq!(library.roots()?.len(), 1);
    Ok(())
}

fn named(library: &Library, order: PlaylistOrder) -> Result<Vec<String>> {
    read_as(library, order, Direction::Ascending)
}

fn read_as(library: &Library, order: PlaylistOrder, direction: Direction) -> Result<Vec<String>> {
    Ok(library
        .playlists(order, direction, None)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect())
}

fn named_as(library: &Library, named: Option<&str>) -> Result<Vec<String>> {
    Ok(library
        .playlists(PlaylistOrder::Name, Direction::Ascending, named)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect())
}

fn scanned_playlist_tree() -> (Tree, Library) {
    let tree = Tree::new();
    for (file, title) in [("a.wav", "Echoes"), ("b.wav", "Dogs"), ("c.wav", "Sheep")] {
        tree.write(file, &Wav::new().text(TITLE, title).build());
    }

    let library = Library::open_in_memory().expect("an in-memory library");
    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library)
}

#[test]
fn a_playlist_keeps_the_order_rows_were_added_in() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let wanted: Vec<Cut> = ["b.wav", "a.wav", "c.wav"]
        .iter()
        .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
        .collect();

    let id = library.create_playlist("  Evening  ")?;
    assert_eq!(library.add_to_playlist(id, &wanted)?, 3);

    let listed = library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "Evening", "the name was stored untrimmed");
    assert_eq!(listed[0].entries, 3);
    assert!(listed[0].duration.is_some_and(|played| !played.is_zero()));

    let entries = library.playlist_entries(id, None)?;
    let titles: Vec<Option<&str>> = entries
        .iter()
        .map(|entry| entry.track.as_ref().map(|track| track.title.as_str()))
        .collect();
    assert_eq!(titles, vec![Some("Dogs"), Some("Echoes"), Some("Sheep")]);
    assert_eq!(library.playlist_cuts(id)?, wanted);
    Ok(())
}

#[test]
fn a_row_the_scan_has_never_seen_is_still_a_row() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let unscanned = MediaLocation::local(tree.path().join("nothing-scanned.wav"));

    let id = library.create_playlist("Mixed")?;
    library.add_to_playlist(
        id,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("a.wav"))),
            Cut::whole(unscanned.clone()),
        ],
    )?;

    let entries = library.playlist_entries(id, None)?;
    assert_eq!(entries.len(), 2);
    assert!(entries[0].track.is_some());
    assert_eq!(entries[1].location(), &unscanned);
    assert!(entries[1].track.is_none(), "an unscanned row grew a track");

    let listed = library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;
    assert_eq!(listed[0].entries, 2, "an unscanned row was not counted");
    Ok(())
}

#[test]
fn a_playlist_moves_and_loses_a_whole_span_of_rows_at_once() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let files = ["a.wav", "b.wav", "c.wav", "d.wav", "e.wav"];
    for file in &files[3..] {
        tree.write(file, &Wav::new().text(TITLE, file).build());
    }
    library.add_to_playlist(
        id,
        &files
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert!(library.move_in_playlist(id, Span::between(0, 1), 3)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "a", "b", "e"],
        "a span moved down did not end on the row it landed on"
    );

    assert!(library.move_in_playlist(id, Span::between(2, 3), 1)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "a", "b", "d", "e"],
        "a span moved up did not start on the row it landed on"
    );

    assert!(
        !library.move_in_playlist(id, Span::between(1, 3), 2)?,
        "a span dropped on itself moved"
    );
    assert!(
        !library.move_in_playlist(id, Span::between(3, 5), 0)?,
        "a span running past the end moved"
    );

    assert!(library.remove_from_playlist(id, Span::between(1, 2))?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "e"],
        "a span removed did not close up behind it"
    );

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["c", "d", "e", "a"],
        "the positions left behind a span were not dense"
    );
    Ok(())
}

#[test]
fn a_playlist_reorders_and_loses_the_row_it_is_told_to() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["a.wav", "b.wav", "c.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert!(library.move_in_playlist(id, Span::one(0), 2)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "c", "a"],
        "the moved row did not land where it was told"
    );

    assert!(library.move_in_playlist(id, Span::one(2), 1)?);
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "a", "c"],
        "a row moved back up did not land where it was told"
    );

    assert!(library.remove_from_playlist(id, Span::one(1))?);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "c"]);

    assert!(!library.move_in_playlist(id, Span::one(0), 9)?);
    assert!(!library.move_in_playlist(id, Span::one(1), 1)?);
    assert!(!library.remove_from_playlist(id, Span::one(9))?);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "c"]);

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["b", "c", "a"],
        "a row added after an edit did not land at the end"
    );
    Ok(())
}

#[test]
fn two_playlists_cannot_share_a_name_however_it_is_cased() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Evening")?;

    assert!(matches!(
        library.create_playlist("evening"),
        Err(Error::DuplicatePlaylist { .. })
    ));
    assert!(matches!(
        library.create_playlist("   "),
        Err(Error::UnnamedPlaylist)
    ));

    library.rename_playlist(id, "Evening")?;
    library.rename_playlist(id, "Morning")?;
    assert_eq!(
        library.playlist(id)?.map(|playlist| playlist.name),
        Some("Morning".to_owned())
    );
    assert_eq!(
        library.playlist_named("MORNING")?.map(|found| found.id),
        Some(id)
    );
    Ok(())
}

#[test]
fn a_name_is_one_name_whatever_alphabet_it_is_written_in() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Café")?;

    assert!(matches!(
        library.create_playlist("CAFÉ"),
        Err(Error::DuplicatePlaylist { .. })
    ));
    assert_eq!(
        library.playlist_named("café")?.map(|found| found.id),
        Some(id),
        "a name was addressed under a fold that reaches ASCII and nothing else"
    );
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, Some("CAFÉ"))?
            .len(),
        1,
        "the index narrowed under a fold that reaches ASCII and nothing else"
    );
    Ok(())
}

#[test]
fn a_name_is_one_name_however_its_accents_are_written() -> Result<()> {
    let library = Library::open_in_memory()?;
    let composed = "Caf\u{e9}";
    let combining = "Cafe\u{301}";
    let id = library.create_playlist(composed)?;

    assert_ne!(composed, combining, "the two spellings are the same bytes");
    assert!(
        matches!(
            library.create_playlist(combining),
            Err(Error::DuplicatePlaylist { .. })
        ),
        "a name written with a combining acute started a playlist of its own"
    );
    assert_eq!(
        library.playlist_named(combining)?.map(|found| found.id),
        Some(id),
        "a name written with a combining acute addressed nothing"
    );
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, Some(combining))?
            .len(),
        1,
        "the index narrowed on the spelling rather than the name"
    );

    let renamed = library.rename_playlist(id, "Cafe\u{301} Noir");
    assert!(renamed.is_ok(), "a rename in one spelling was refused");
    assert_eq!(
        library
            .playlist_named("caf\u{e9} noir")?
            .map(|found| found.id),
        Some(id),
        "a renamed playlist kept the fold of the spelling it was given"
    );
    Ok(())
}

#[test]
fn a_playlist_nothing_holds_is_never_the_one_in_play() -> Result<()> {
    let library = Library::open_in_memory()?;
    let unknown = PlaylistId::new(404)?;

    library.set_playing_playlist(loaded(unknown));
    assert_eq!(
        library.playing_playlist(QueueStamp::default()),
        None,
        "the cell reported a playlist the catalog does not hold"
    );
    Ok(())
}

#[test]
fn a_playlist_is_in_play_only_while_the_queue_is_the_one_it_was_loaded_as() -> Result<()> {
    let library = Library::open_in_memory()?;
    let evening = library.create_playlist("Evening")?;
    let as_loaded = QueueStamp::of(["a.wav", "b.wav"]);
    let after_an_edit = QueueStamp::of(["a.wav"]);

    library.set_playing_playlist(Some(Playing {
        playlist: evening,
        queue: as_loaded,
    }));
    assert_eq!(library.playing_playlist(as_loaded), Some(evening));
    assert_eq!(
        library.playing_playlist(after_an_edit),
        None,
        "a queue a row had left still read as the playlist it was loaded from"
    );
    assert_eq!(
        library.playing_playlist(as_loaded),
        Some(evening),
        "a queue put back the way it was loaded no longer read as the playlist"
    );
    Ok(())
}

#[test]
fn a_playlist_that_is_dropped_takes_its_rows_and_the_transport_with_it() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;
    library.set_playing_playlist(loaded(id));

    assert!(library.remove_playlist(id)?);
    assert_eq!(library.playing_playlist(QueueStamp::default()), None);
    assert!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .is_empty()
    );
    assert!(library.playlist_entries(id, None)?.is_empty());
    assert!(!library.remove_playlist(id)?);
    Ok(())
}

#[test]
fn forgetting_a_root_leaves_the_playlist_rows_that_came_from_it() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(id, slice::from_ref(&file))?;

    assert!(library.remove_root(tree.path())?);
    assert!(all(&library)?.is_empty());

    let entries = library.playlist_entries(id, None)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].location(), &file.location);
    assert!(entries[0].track.is_none());
    Ok(())
}

#[test]
fn only_an_edit_moves_the_revision_a_bus_client_watches() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let created = library.playlists_revision();

    let id = library.create_playlist("Evening")?;
    let named = library.playlists_revision();
    assert_ne!(named, created, "creating a playlist moved nothing");

    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;
    let filled = library.playlists_revision();
    assert_ne!(filled, named, "adding a row moved nothing");

    library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;
    library.playlist_entries(id, None)?;
    assert_eq!(library.playlists_revision(), filled, "a read moved it");

    assert!(!library.remove_from_playlist(id, Span::one(9))?);
    assert_eq!(
        library.playlists_revision(),
        filled,
        "an edit that changed nothing moved it"
    );
    Ok(())
}

#[test]
fn a_playlist_holds_local_files_only() -> Result<()> {
    let library = Library::open_in_memory()?;
    let id = library.create_playlist("Evening")?;
    let remote = MediaLocation::new(
        SourceId::new("subsonic").expect("a lowercase name"),
        "track/1",
    );

    assert!(matches!(
        library.add_to_playlist(id, &[Cut::whole(remote)]),
        Err(Error::NotALocalFile { .. })
    ));
    assert!(library.playlist_entries(id, None)?.is_empty());
    Ok(())
}

#[test]
fn a_playlist_written_out_carries_what_a_reader_needs_and_comes_back_whole() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["b.wav", "gone.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.m3u8");
    let written = library.export_playlist(id, &sheet)?;
    assert_eq!(written.rows, 3);
    assert_eq!(written.format, PlaylistFormat::M3u);

    let written = fs::read_to_string(&sheet).expect("the sheet was written");
    assert!(
        written.starts_with("#EXTM3U\n#PLAYLIST:Evening\n"),
        "{written}"
    );
    assert!(
        written.contains("\nb.wav\n"),
        "a file beside the sheet was not named relative to it: {written}"
    );
    assert!(written.contains(",Dogs\n"), "{written}");
    assert!(
        written.contains("#EXTINF:-1,gone\n"),
        "a row no scan has seen lost its name: {written}"
    );

    let read_back = library.import_playlist(&sheet, Some("Evening again"))?;
    assert_eq!(read_back.name, "Evening again");
    assert_eq!(read_back.added, 3);
    assert_eq!(read_back.elsewhere, 0);
    assert_eq!(
        read_back.missing, 1,
        "a row whose file is gone was not counted"
    );
    assert_eq!(
        stems(&library.playlist_cuts(read_back.id)?),
        vec!["b", "gone", "a"]
    );
    Ok(())
}

#[test]
fn a_sheet_another_player_wrote_names_itself_and_counts_what_it_could_not_take() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("from-another-player.m3u");
    fs::write(
        &sheet,
        format!(
            "#EXTM3U\n\
             #PLAYLIST:   Someone else's evening   \n\
             \n\
             # a line no tag defines\n\
             #EXTINF:213,Pink Floyd - Echoes\n\
             a.wav\n\
             file://{root}/b.wav\n\
             http://stream.example/live.ogg\n\
             {root}/c.wav\n",
            root = tree.path().display()
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.name, "Someone else's evening");
    assert_eq!(imported.added, 3);
    assert_eq!(imported.elsewhere, 1, "a stream was read as a file");
    assert_eq!(imported.missing, 0);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "c"]
    );

    let again = library.import_playlist(&sheet, None)?;
    assert_eq!(
        again.id, imported.id,
        "a second import made a second playlist"
    );
    assert_eq!(again.added, 0, "a second import added rows again");
    assert_eq!(again.already, 3, "the rows already held were not counted");
    assert_eq!(
        library.playlist(imported.id)?.map(|found| found.entries),
        Some(3),
        "importing the same sheet twice doubled the playlist"
    );
    Ok(())
}

fn one_song(bits: u16, frames: usize) -> Vec<u8> {
    Wav::new()
        .bits(bits)
        .frames(frames)
        .text(TITLE, "Echoes")
        .text(ARTIST, "Pink Floyd")
        .text(ALBUM, "Meddle")
        .text(TRACK, "6")
        .build()
}

#[test]
fn one_song_in_two_formats_is_listed_once_as_the_better_of_the_two() -> Result<()> {
    let tree = Tree::new();
    tree.write("flac/06 Echoes.wav", &one_song(16, 44_100));
    let best = tree.write("hires/06 Echoes.wav", &one_song(24, 44_100));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;
    assert_eq!(rows.len(), 1, "one song in two formats was listed twice");
    assert_eq!(rows[0].location.as_path(), Some(best.as_path()));
    assert_eq!(rows[0].alternatives, 1);

    let alternatives = library.alternatives_of(rows[0].id)?;
    assert_eq!(alternatives.len(), 1);
    assert_eq!(alternatives[0].spec.format.valid_bits(), 16);

    let albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1);
    assert_eq!(
        albums[0].track_count, 1,
        "the album counted both copies of one song"
    );
    Ok(())
}

#[test]
fn two_identical_copies_of_one_song_are_listed_once_as_the_first_scanned() -> Result<()> {
    let tree = Tree::new();
    let copies = [
        tree.write("a/06 Echoes.wav", &one_song(16, 44_100)),
        tree.write("b/06 Echoes.wav", &one_song(16, 44_100)),
    ];

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;
    assert_eq!(rows.len(), 1, "two identical copies were listed twice");
    assert_eq!(rows[0].alternatives, 1);
    let mut stored = Vec::new();
    for copy in &copies {
        stored.push(
            library
                .track_at(copy, None)?
                .expect("the scan stored the file it walked")
                .id,
        );
    }
    let first_id = stored.into_iter().min().expect("two copies were stored");
    assert_eq!(
        rows[0].id, first_id,
        "the copy listed was not the one the catalog held first"
    );
    Ok(())
}

#[test]
fn a_copy_in_the_best_copys_own_format_is_hidden_beside_one_in_another() -> Result<()> {
    let tree = Tree::new();
    tree.write("a/06 Echoes.wav", &one_song(24, 44_100));
    tree.write("b/06 Echoes.wav", &one_song(24, 44_100));
    tree.write("c/06 Echoes.wav", &one_song(16, 44_100));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;
    assert_eq!(
        rows.len(),
        1,
        "a copy matching the best copy's format stayed on the list"
    );
    assert_eq!(rows[0].alternatives, 2);
    Ok(())
}

#[test]
fn a_copy_billed_under_its_release_title_still_meets_one_tagged_the_same() -> Result<()> {
    let tree = Tree::new();
    let reissue = Wav::new()
        .bits(16)
        .frames(44_100)
        .text(TITLE, "Echoes")
        .text(ARTIST, "Pink Floyd")
        .text(ALBUM, "Meddle (Remastered)")
        .text(TRACK, "6")
        .build();
    tree.write("a/06 Echoes.wav", &one_song(16, 44_100));
    tree.write("b/06 Echoes.wav", &reissue);

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        all(&library)?.len(),
        2,
        "two albums of different names met before anything named them one"
    );

    let billed = beside(&database)
        .execute(
            "UPDATE albums SET release_title = 'Meddle' WHERE title = 'Meddle (Remastered)'",
            [],
        )
        .expect("the album row is writable");
    assert_eq!(billed, 1);
    scan(&library, &options(&tree))?;

    assert_eq!(
        all(&library)?.len(),
        1,
        "a copy billed as Meddle did not meet the copy tagged Meddle"
    );
    Ok(())
}

#[test]
fn two_copies_whose_lengths_disagree_are_two_songs() -> Result<()> {
    let tree = Tree::new();
    tree.write("a/06 Echoes.wav", &one_song(16, 44_100));
    tree.write("b/06 Echoes.wav", &one_song(24, 44_100 * 4));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    assert_eq!(all(&library)?.len(), 2);
    Ok(())
}

#[test]
fn the_better_copy_leaving_puts_the_one_it_hid_back_on_the_list() -> Result<()> {
    let tree = Tree::new();
    tree.write("flac/06 Echoes.wav", &one_song(16, 44_100));
    let best = tree.write("hires/06 Echoes.wav", &one_song(24, 44_100));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    fs::remove_file(&best).expect("the better copy goes");
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].spec.format.valid_bits(), 16);
    assert_eq!(rows[0].alternatives, 0);
    Ok(())
}

#[test]
fn a_sheet_written_on_windows_resolves_its_backslashed_rows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    fs::create_dir_all(tree.path().join("lists")).expect("a writable temporary folder");
    let sheet = tree.path().join("lists/windows.m3u");
    fs::write(&sheet, "#EXTM3U\r\n..\\a.wav\r\n..\\b.wav\r\n").expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 2);
    assert_eq!(
        imported.missing, 0,
        "a backslashed row read as a missing file"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
fn a_sheet_that_names_one_file_twice_keeps_both_rows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("doubled.m3u8");
    fs::write(&sheet, "#EXTM3U\na.wav\nb.wav\na.wav\n").expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;
    assert_eq!(
        imported.added, 3,
        "a file the sheet itself repeated was lost"
    );
    assert_eq!(imported.already, 0);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "a"]
    );

    let again = library.import_playlist(&sheet, Some("Evening"))?;
    assert_eq!(again.added, 0, "the same sheet read twice was not a no-op");
    assert_eq!(again.already, 3);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "a"]
    );
    Ok(())
}

#[test]
fn a_row_a_sheet_cannot_name_plainly_is_written_as_a_uri() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let awkward = ["#hash.wav", "two\nlines.wav", "plain.wav"];
    for file in awkward {
        tree.write(file, b"a file no scan has read");
    }

    let id = library.start_playlist(
        "Awkward",
        &awkward
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    for (name, format) in [("awkward.m3u8", "M3U"), ("awkward.pls", "PLS")] {
        let sheet = tree.path().join(name);
        library.export_playlist(id, &sheet)?;

        let read_back = library.import_playlist(&sheet, Some(format))?;
        assert_eq!(
            read_back.added, 3,
            "{format} lost a row whose path it could not write plainly"
        );
        assert_eq!(read_back.elsewhere, 0);
        assert_eq!(read_back.missing, 0);
        assert_eq!(
            stems(&library.playlist_cuts(read_back.id)?),
            vec!["#hash", "two\nlines", "plain"]
        );
    }
    Ok(())
}

#[test]
fn a_row_whose_file_is_not_there_is_still_stored_as_an_absolute_path() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.path().join("beneath/list.m3u8");
    tree.write("beneath/list.m3u8", b"#EXTM3U\n../missing.wav\n");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;
    assert_eq!(imported.added, 1);
    assert_eq!(imported.missing, 1);
    assert_eq!(
        library.playlist_cuts(imported.id)?,
        vec![Cut::whole(MediaLocation::local(
            tree.path().join("missing.wav")
        ))],
        "a row no mount answered for kept the parent segments it was written with"
    );
    Ok(())
}

#[test]
fn a_row_holding_an_escaped_nul_names_no_file() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.path().join("nul.m3u8");
    fs::write(
        &sheet,
        format!(
            "#EXTM3U\nfile://{root}/a%00b.wav\nfile://{root}/c.wav\n",
            root = tree.path().display()
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;
    assert_eq!(imported.added, 1);
    assert_eq!(
        imported.elsewhere, 1,
        "a path holding an interior NUL was stored as a row"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["c"]);
    Ok(())
}

#[test]
fn tidying_a_playlist_drops_only_the_rows_whose_files_have_gone() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["a.wav", "gone.wav", "b.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let filled = library.playlists_revision();
    assert_eq!(library.prune_playlist(id)?, 1);
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["a", "b"]);
    assert_ne!(library.playlists_revision(), filled, "a tidy moved nothing");

    let tidied = library.playlists_revision();
    assert_eq!(library.prune_playlist(id)?, 0);
    assert_eq!(
        library.playlists_revision(),
        tidied,
        "a tidy that dropped nothing moved it"
    );
    Ok(())
}

#[test]
fn a_playlist_file_neither_side_can_read_is_refused_by_name() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.write("bytes.m3u", &[0xff, 0xfe, 0x00]);

    assert!(matches!(
        library.import_playlist(&sheet, None),
        Err(Error::NonUtf8PlaylistFile { .. })
    ));
    assert!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .is_empty(),
        "a sheet that would not read still named a playlist"
    );

    let unknown = PlaylistId::new(9_999)?;
    assert!(matches!(
        library.export_playlist(unknown, &tree.path().join("nothing.m3u8")),
        Err(Error::UnknownPlaylist(_))
    ));
    Ok(())
}

fn stems(cuts: &[Cut]) -> Vec<String> {
    cuts.iter()
        .map(|cut| cut.location.stem().unwrap_or_default().into_owned())
        .collect()
}

#[test]
fn a_playlist_goes_out_and_comes_back_as_pls() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["b.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.pls");
    let written = library.export_playlist(id, &sheet)?;
    assert_eq!(written.format, PlaylistFormat::Pls);
    assert_eq!(written.rows, 2);

    let text = fs::read_to_string(&sheet).expect("the sheet was written");
    assert!(
        text.starts_with("[playlist]\nX-GNOME-Title=Evening\n"),
        "{text}"
    );
    assert!(text.contains("File1=b.wav\n"), "{text}");
    assert!(text.contains("Title1=Dogs\n"), "{text}");
    assert!(text.ends_with("NumberOfEntries=2\nVersion=2\n"), "{text}");

    let read_back = library.import_playlist(&sheet, Some("Evening again"))?;
    assert_eq!(read_back.format, PlaylistFormat::Pls);
    assert_eq!(read_back.added, 2);
    assert_eq!(
        read_back.short, 0,
        "a sheet this player wrote disagreed with its own count"
    );
    assert_eq!(
        stems(&library.playlist_cuts(read_back.id)?),
        vec!["b", "a"],
        "the rows did not come back in the order they were numbered"
    );
    Ok(())
}

#[test]
fn a_playlist_goes_out_and_comes_back_as_xspf() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["b.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.xspf");
    let written = library.export_playlist(id, &sheet)?;
    assert_eq!(written.format, PlaylistFormat::Xspf);
    assert_eq!(written.rows, 2);

    let text = fs::read_to_string(&sheet).expect("the sheet was written");
    assert!(text.contains("<title>Evening</title>"), "{text}");
    assert!(text.contains("<location>b.wav</location>"), "{text}");
    assert!(text.contains("<title>Dogs</title>"), "{text}");

    let read_back = library.import_playlist(&sheet, None)?;
    assert_eq!(
        read_back.id, id,
        "the title the sheet declares named another playlist"
    );
    assert_eq!(read_back.added, 0);
    assert_eq!(read_back.already, 2);
    Ok(())
}

#[test]
fn an_xspf_sheet_another_player_wrote_is_read_past_its_markup() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("from-another-player.xspf");
    fs::write(
        &sheet,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <xspf:playlist version=\"1\" xmlns:xspf=\"http://xspf.org/ns/0/\">\n\
             <!-- written elsewhere, 3 > 2 -->\n\
             <xspf:title>  Someone else&apos;s evening  </xspf:title>\n\
             <xspf:trackList>\n\
             <xspf:track>\
             <xspf:location>file://{root}/a.wav</xspf:location>\
             <xspf:title>Echoes</xspf:title>\
             </xspf:track>\n\
             <xspf:track><xspf:location>b%2Ewav</xspf:location></xspf:track>\n\
             <xspf:track><xspf:location>http://stream.example/live.ogg</xspf:location></xspf:track>\n\
             <xspf:track><xspf:title>a row naming nothing</xspf:title></xspf:track>\n\
             </xspf:trackList>\n\
             </xspf:playlist>\n",
            root = tree.path().display()
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.name, "Someone else's evening");
    assert_eq!(imported.format, PlaylistFormat::Xspf);
    assert_eq!(imported.added, 2);
    assert_eq!(imported.elsewhere, 1, "a stream was read as a file");
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
fn an_xspf_sheet_resolves_its_rows_against_the_base_it_declares() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    for file in ["discs/one/a.wav", "discs/two/b.wav", "discs/d.wav", "c.wav"] {
        tree.write(file, b"a file no scan has read");
    }

    let sheet = tree.path().join("based.xspf");
    fs::write(
        &sheet,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE playlist>\n\
         <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\" xml:base=\"discs/\">\n\
         <title>Both discs</title>\n\
         <trackList>\n\
         <track xml:base=\"one/\" annotation=\"5 > 4\"><location>a.wav</location></track>\n\
         <track xml:base=\"two/\"><location>b.wav</location></track>\n\
         <track><location>../c.wav</location></track>\n\
         <track>\
         <location>http://stream.example/live.ogg</location>\
         <location>d.wav</location>\
         </track>\n\
         <track><location>http://stream.example/only.ogg</location></track>\n\
         </trackList>\n\
         </playlist>\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.name, "Both discs");
    assert_eq!(imported.added, 4);
    assert_eq!(
        imported.elsewhere, 1,
        "a track offering nothing but a stream was not counted"
    );
    assert_eq!(
        imported.missing, 0,
        "a row did not resolve against the base its sheet declared"
    );
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "c", "d"],
        "an alternate location beside a stream was not taken"
    );
    Ok(())
}

#[test]
fn a_pls_sheet_is_weighed_against_the_count_it_declares() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    for file in ["a.wav", "b.wav"] {
        tree.write(file, b"a file no scan has read");
    }

    let sheet = tree.path().join("half.pls");
    fs::write(
        &sheet,
        "[playlist]\nX-GNOME-Title=Half a sheet\nFile1=a.wav\nFile2=b.wav\n\
         NumberOfEntries=5\nVersion=2\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.name, "Half a sheet");
    assert_eq!(imported.added, 2);
    assert_eq!(
        imported.short, 3,
        "the count the sheet declared was not weighed against what it held"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
fn a_sheet_is_read_by_what_it_holds_rather_than_what_it_is_called() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("mislabelled.m3u");
    fs::write(&sheet, "[playlist]\nFile1=a.wav\nNumberOfEntries=1\n")
        .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.format, PlaylistFormat::Pls);
    assert_eq!(imported.name, "mislabelled", "the stem did not name it");
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a"]);
    Ok(())
}

#[test]
fn a_sheet_written_in_a_legacy_encoding_still_comes_in() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    tree.write("Bj\u{f6}rk.wav", b"a file no scan has read");

    let mut bytes = b"#EXTM3U\n#PLAYLIST:Kv".to_vec();
    bytes.push(0xe6);
    bytes.extend_from_slice(b"ld ");
    bytes.push(0x93);
    bytes.extend_from_slice(b"Live");
    bytes.push(0x94);
    bytes.extend_from_slice(b"\nBj");
    bytes.push(0xf6);
    bytes.extend_from_slice(b"rk.wav\n");

    let sheet = tree.write("legacy.m3u", &bytes);
    let imported = library.import_playlist(&sheet, None)?;
    assert_eq!(imported.encoding, SheetEncoding::Windows1252);
    assert_eq!(
        imported.name, "Kvæld “Live”",
        "the bytes above Latin-1 were not read as Windows-1252"
    );
    assert_eq!(imported.added, 1);
    assert_eq!(imported.missing, 0);
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["Björk"]);

    let strict = tree.write("legacy.m3u8", &bytes);
    assert!(
        matches!(
            library.import_playlist(&strict, None),
            Err(Error::NonUtf8PlaylistFile { .. })
        ),
        "a sheet whose name declares UTF-8 was read as something else"
    );
    Ok(())
}

#[test]
fn a_playlist_that_is_played_records_when_and_how_often() -> Result<()> {
    let library = Library::open_in_memory()?;
    let morning = library.create_playlist("Morning")?;
    let evening = library.create_playlist("Evening")?;

    assert!(
        library
            .playlist(evening)?
            .is_some_and(|found| found.played.is_none() && found.plays == 0),
        "a playlist nothing has played named a last play or a count"
    );

    library.set_playing_playlist(loaded(evening));
    let played = library.playlist(evening)?.and_then(|found| found.played);
    assert!(played.is_some(), "playing a playlist recorded nothing");
    assert_eq!(
        library.playlist(evening)?.map(|found| found.plays),
        Some(1),
        "playing a playlist counted nothing"
    );

    let by_play: Vec<String> = library
        .playlists(PlaylistOrder::Played, Direction::Ascending, None)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect();
    assert_eq!(
        by_play,
        vec!["Morning", "Evening"],
        "one nothing has played did not come first"
    );

    library.set_playing_playlist(None);
    assert_eq!(
        library.playlist(evening)?.and_then(|found| found.played),
        played,
        "clearing what is in play rewrote a last play"
    );
    assert_eq!(
        library.playlist(evening)?.map(|found| found.plays),
        Some(1),
        "clearing what is in play counted a play"
    );
    assert_eq!(morning, PlaylistId::new(1)?);
    Ok(())
}

#[test]
fn the_playlists_are_listed_in_whatever_order_is_asked_for() -> Result<()> {
    let library = Library::open_in_memory()?;
    let anthems = library.create_playlist("Anthems")?;
    let ballads = library.create_playlist("Ballads")?;
    let carols = library.create_playlist("Carols")?;

    for _ in 0..3 {
        library.set_playing_playlist(loaded(carols));
    }
    library.set_playing_playlist(loaded(anthems));
    library.rename_playlist(ballads, "Ballads and laments")?;

    assert_eq!(
        named(&library, PlaylistOrder::Name)?,
        vec!["Anthems", "Ballads and laments", "Carols"]
    );
    assert_eq!(
        named(&library, PlaylistOrder::Created)?,
        vec!["Anthems", "Ballads and laments", "Carols"],
        "the oldest did not come first"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Modified)?,
        vec!["Anthems", "Carols", "Ballads and laments"],
        "the last one edited did not come last"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Played)?,
        vec!["Ballads and laments", "Carols", "Anthems"],
        "one nothing has played did not come first"
    );
    assert_eq!(
        named(&library, PlaylistOrder::Plays)?,
        vec!["Ballads and laments", "Anthems", "Carols"],
        "the most played did not come last"
    );

    assert_eq!(
        PlaylistOrder::Name.reads(),
        Direction::Ascending,
        "a listing by name opens back to front"
    );
    assert!(
        PlaylistOrder::ALL
            .into_iter()
            .filter(|order| *order != PlaylistOrder::Name)
            .all(|order| order.reads() == Direction::Descending),
        "an order the newest or the most belongs at the top of opens front to back"
    );

    assert_eq!(
        read_as(&library, PlaylistOrder::Plays, Direction::Descending)?,
        vec!["Carols", "Anthems", "Ballads and laments"],
        "the most played did not come first when the listing was asked to read down"
    );
    assert_eq!(
        read_as(&library, PlaylistOrder::Played, Direction::Descending)?,
        vec!["Anthems", "Carols", "Ballads and laments"],
        "one nothing has played did not come last when the listing was asked to read down"
    );
    assert_eq!(
        read_as(&library, PlaylistOrder::Name, Direction::Descending)?,
        vec!["Carols", "Ballads and laments", "Anthems"],
        "a listing by name would not read Z to A"
    );
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_as_the_library_grows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.save_query(
        "Everything",
        &SavedQuery {
            text: None,
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    let found = library.playlist(id)?.expect("the playlist was saved");
    assert_eq!(found.entries, 3, "the query did not count what it matches");
    assert!(found.duration.is_some_and(|played| !played.is_zero()));
    assert_eq!(
        found.query.as_ref().map(|query| query.sort),
        Some(SortOrder::Title)
    );

    let entries = library.playlist_entries(id, None)?;
    let titles: Vec<&str> = entries
        .iter()
        .map(|entry| entry.track.as_ref().expect("a scanned row").title.as_str())
        .collect();
    assert_eq!(titles, vec!["Dogs", "Echoes", "Sheep"]);

    tree.write("d.wav", &Wav::new().text(TITLE, "Animals").build());
    scan(&library, &options(&tree))?;
    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(4),
        "a track the scan added did not reach the saved query"
    );
    Ok(())
}

#[test]
fn a_saved_query_takes_its_rows_from_the_text_and_the_limit() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);

    let capped = library.save_query(
        "One dog",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: Some(1),
        },
    )?;
    assert_eq!(library.playlist_entries(capped, None)?.len(), 1);
    assert_eq!(
        library.playlist(capped)?.map(|found| found.entries),
        Some(1),
        "the count went past the limit the query holds"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_revised_where_it_stands() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);
    let saved = library.playlists_revision();

    let revised = SavedQuery {
        text: Some("echoes".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(1),
    };
    library.revise_query(id, "Dogs Live", &revised)?;
    assert_ne!(
        library.playlists_revision(),
        saved,
        "revising a search moved nothing"
    );

    let found = library.playlist(id)?.expect("the playlist is still there");
    assert_eq!(found.name, "Dogs Live", "a revision left the name behind");
    assert_eq!(found.query, Some(revised), "the query read back as it was");
    assert_eq!(found.entries, 1, "the count did not follow the new search");
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["a"]);
    Ok(())
}

#[test]
fn a_search_narrows_a_playlist_to_the_rows_it_matches() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["b.wav", "a.wav", "c.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let whole = library.playlist_entries(id, None)?;
    assert_eq!(
        whole.iter().map(|entry| entry.position).collect::<Vec<_>>(),
        vec![0, 1, 2],
        "a row did not carry the place the list holds it in"
    );

    let narrowed = library.playlist_entries(id, Some("sheep"))?;
    assert_eq!(narrowed.len(), 1, "the search narrowed to the wrong rows");
    assert_eq!(
        narrowed[0].position, 2,
        "a narrowed row forgot where it stands in the list"
    );
    assert_eq!(stems(&[narrowed[0].cut.clone()]), vec!["c"]);

    assert_eq!(
        library.playlist_entries(id, Some("is:lossless"))?.len(),
        3,
        "a term did not reach a playlist's rows"
    );
    assert!(
        library
            .playlist_entries(id, Some("nothing here"))?
            .is_empty(),
        "a search that matches nothing still drew rows"
    );
    Ok(())
}

#[test]
fn a_playlist_row_no_scan_has_seen_matches_no_search() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let unscanned = tree.write("late.wav", &Wav::new().text(TITLE, "Sheep").build());

    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("c.wav"))),
            Cut::whole(MediaLocation::local(unscanned)),
        ],
    )?;

    assert_eq!(library.playlist_entries(id, None)?.len(), 2);
    let narrowed = library.playlist_entries(id, Some("sheep"))?;
    assert_eq!(
        stems(
            &narrowed
                .iter()
                .map(|entry| entry.cut.clone())
                .collect::<Vec<_>>()
        ),
        vec!["c"],
        "a row the catalog has never seen answered a search"
    );
    Ok(())
}

#[test]
fn a_search_narrows_the_index_by_the_words_it_holds() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();
    for name in ["Evening jazz", "Morning jazz", "Anthems"] {
        library.create_playlist(name)?;
    }

    assert_eq!(
        named_as(&library, Some("jazz"))?,
        vec!["Evening jazz", "Morning jazz"],
        "a word did not narrow the index by name"
    );
    assert_eq!(
        named_as(&library, Some("morning JAZZ"))?,
        vec!["Morning jazz"],
        "two words did not both have to hold, or the match was cased"
    );
    assert_eq!(
        named_as(&library, Some("is:hires"))?,
        vec!["Anthems", "Evening jazz", "Morning jazz"],
        "a term narrowed an index that has only a name to answer with"
    );
    assert!(
        named_as(&library, Some("nothing"))?.is_empty(),
        "a word nothing is named after still listed a playlist"
    );
    Ok(())
}

#[test]
fn a_listing_of_names_reads_the_same_order_as_the_listing_beside_it_and_stops_where_told()
-> Result<()> {
    let library = Library::open_in_memory()?;
    for name in ["Anthems", "Ballads", "Carols", "Dirges"] {
        library.create_playlist(name)?;
    }

    let names = |from, most| -> Result<Vec<String>> {
        Ok(library
            .playlist_names(PlaylistOrder::Name, Direction::Ascending, from, most)?
            .into_iter()
            .map(|playlist| playlist.name)
            .collect())
    };

    assert_eq!(names(0, None)?, named_as(&library, None)?);
    assert_eq!(names(1, Some(2))?, vec!["Ballads", "Carols"]);
    assert_eq!(names(0, Some(0))?, Vec::<String>::new());
    assert_eq!(names(4, None)?, Vec::<String>::new());
    assert_eq!(
        library
            .playlist_names(PlaylistOrder::Name, Direction::Descending, 0, Some(1))?
            .into_iter()
            .map(|playlist| playlist.name)
            .collect::<Vec<_>>(),
        vec!["Dirges"],
        "a listing read the other way round started at the same end"
    );
    Ok(())
}

#[test]
fn the_count_is_counted_rather_than_read_off_a_listing() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    assert_eq!(library.playlist_count(None)?, 0);

    let evening = library.create_playlist("Evening jazz")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.create_playlist("Morning jazz")?;
    library.save_query("Anthems", &SavedQuery::default())?;

    assert_eq!(
        library.playlist_count(None)?,
        3,
        "a playlist that fills itself was counted as its rows rather than as one playlist"
    );
    assert_eq!(
        library.playlist_count(Some("jazz"))?,
        2,
        "a count read past the narrowing the listing beside it takes"
    );
    assert_eq!(
        library.playlist_count(Some("nothing"))? as usize,
        named_as(&library, Some("nothing"))?.len()
    );
    assert_eq!(
        library.playlist_count(None)? as usize,
        named_as(&library, None)?.len(),
        "the count and the listing disagreed about how many playlists there are"
    );
    Ok(())
}

#[test]
fn the_lists_are_read_without_the_playlists_that_fill_themselves() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));

    let evening = library.create_playlist("Evening jazz")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.create_playlist("Morning jazz")?;
    library.save_query("Anthems", &SavedQuery::default())?;

    let lists = library.playlist_lists(PlaylistOrder::Name, Direction::Ascending)?;
    assert_eq!(
        lists
            .iter()
            .map(|list| list.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Evening jazz", "Morning jazz"],
        "a playlist that fills itself was offered as a list to put rows in"
    );
    assert!(
        lists.iter().all(|list| list.query.is_none()),
        "a list came back carrying a saved query"
    );
    assert_eq!(lists[0].entries, 2);
    assert_eq!(lists[1].entries, 0);

    let reversed = library.playlist_lists(PlaylistOrder::Name, Direction::Descending)?;
    assert_eq!(
        reversed
            .iter()
            .map(|list| list.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Morning jazz", "Evening jazz"]
    );
    Ok(())
}

#[test]
fn a_saved_query_and_a_search_both_have_to_hold() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    tree.write("d.wav", &Wav::new().text(TITLE, "Dogs Live").build());
    scan(&library, &options(&tree))?;

    let id = library.save_query(
        "Dogs",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["b", "d"]);

    let narrowed = library.playlist_entries(id, Some("live"))?;
    assert_eq!(
        stems(
            &narrowed
                .iter()
                .map(|entry| entry.cut.clone())
                .collect::<Vec<_>>()
        ),
        vec!["d"],
        "the query's own text and the search did not both hold"
    );
    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(2),
        "narrowing the rows shown changed what the query says it holds"
    );
    Ok(())
}

#[test]
fn a_list_has_no_query_to_revise() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &[Cut::whole(MediaLocation::local(tree.path().join("a.wav")))],
    )?;

    assert!(matches!(
        library.revise_query(id, "Evening", &SavedQuery::default()),
        Err(Error::NotAQuery { .. })
    ));
    assert!(matches!(
        library.revise_query(PlaylistId::new(404)?, "Nothing", &SavedQuery::default()),
        Err(Error::UnknownPlaylist(_))
    ));
    assert_eq!(
        library.playlist_entries(id, None)?.len(),
        1,
        "a refused revision changed what the list holds"
    );
    Ok(())
}

fn sortable_playlist_tree() -> (Tree, Library) {
    let tree = Tree::new();
    let written = [
        ("c.wav", "Echoes", "Syd Barrett", 44_100),
        ("a.wav", "Dogs", "Pink Floyd", 13_230),
        ("b.wav", "Sheep", "Roger Waters", 4_410),
    ];
    for (file, title, artist, frames) in written {
        tree.write(
            file,
            &Wav::new()
                .frames(frames)
                .text(TITLE, title)
                .text(ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory().expect("an in-memory library");
    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library)
}

fn held(library: &Library, id: PlaylistId) -> Result<Vec<String>> {
    Ok(stems(&library.playlist_cuts(id)?))
}

#[test]
fn a_playlist_is_put_in_order_rather_than_moved_a_row_at_a_time() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["c.wav", "a.wav", "b.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["a", "c", "b"]);
    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?,
        0,
        "a list already in that order was written again"
    );

    assert_eq!(
        library.sort_playlist(id, RowOrder::Title, Direction::Descending)?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["b", "c", "a"]);

    library.sort_playlist(id, RowOrder::Artist, Direction::Ascending)?;
    assert_eq!(held(&library, id)?, vec!["a", "b", "c"]);

    library.sort_playlist(id, RowOrder::Length, Direction::Ascending)?;
    assert_eq!(held(&library, id)?, vec!["b", "a", "c"]);

    library.sort_playlist(id, RowOrder::File, Direction::Descending)?;
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);
    Ok(())
}

#[test]
fn putting_a_list_in_order_leaves_a_row_no_scan_has_seen_at_the_end() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Mixed")?;
    library.add_to_playlist(
        id,
        &["c.wav", "d.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;
    assert_eq!(held(&library, id)?, vec!["a", "c", "d"]);

    library.sort_playlist(id, RowOrder::Title, Direction::Descending)?;
    assert_eq!(
        held(&library, id)?,
        vec!["c", "a", "d"],
        "an unscanned row did not stay at the end"
    );

    library.sort_playlist(id, RowOrder::File, Direction::Ascending)?;
    assert_eq!(
        held(&library, id)?,
        vec!["a", "c", "d"],
        "the file order left a row out"
    );
    Ok(())
}

#[test]
fn putting_a_list_in_order_moves_what_a_pane_redraws_on() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.add_to_playlist(
        id,
        &["c.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;
    let added = library.playlists_revision();

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;
    let sorted = library.playlists_revision();
    assert_ne!(sorted, added, "putting a list in order redrew nothing");

    library.sort_playlist(id, RowOrder::Title, Direction::Ascending)?;
    assert_eq!(
        library.playlists_revision(),
        sorted,
        "a list already in that order redrew the pane"
    );
    Ok(())
}

#[test]
fn a_list_kept_in_an_order_puts_a_row_added_later_where_the_order_wants_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("b.wav"), file("c.wav")])?;

    assert_eq!(
        library.keep_playlist_in_order(
            id,
            Some(Kept {
                order: RowOrder::Title,
                reading: Direction::Ascending,
            })
        )?,
        2
    );
    assert_eq!(held(&library, id)?, vec!["c", "b"]);

    library.add_to_playlist(id, slice::from_ref(&file("a.wav")))?;
    assert_eq!(
        held(&library, id)?,
        vec!["a", "c", "b"],
        "an added row landed at the end of a list kept in order"
    );

    let kept = library.playlist(id)?.expect("the playlist is there").kept;
    assert_eq!(
        kept,
        Some(Kept {
            order: RowOrder::Title,
            reading: Direction::Ascending,
        })
    );
    Ok(())
}

#[test]
fn a_list_kept_in_an_order_is_not_one_to_place_by_hand() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert!(matches!(
        library.move_in_playlist(id, Span::one(0), 2),
        Err(Error::KeptInOrder { .. })
    ));
    assert!(matches!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending),
        Err(Error::KeptInOrder { .. })
    ));
    assert_eq!(held(&library, id)?, vec!["a", "b", "c"]);

    assert!(library.remove_from_playlist(id, Span::one(1))?);
    assert_eq!(
        held(&library, id)?,
        vec!["a", "c"],
        "a kept order refused the edits that do not place a row"
    );
    Ok(())
}

#[test]
fn putting_a_list_that_is_already_in_hand_back_in_hand_writes_nothing() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("c.wav"), file("b.wav")])?;
    let unchanged = library.playlist(id)?.expect("the playlist").modified;

    assert_eq!(library.keep_playlist_in_order(id, None)?, 0);
    assert_eq!(
        library.playlist(id)?.expect("the playlist").modified,
        unchanged,
        "a list put back in hand it was already in was marked as changed"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "a list put back in hand it was already in left a step to walk back"
    );
    Ok(())
}

#[test]
fn a_span_past_the_end_of_a_list_takes_only_the_rows_there_are() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;

    assert!(!library.move_in_playlist(id, Span::one(usize::MAX), 0)?);
    assert!(!library.move_in_playlist(id, Span::one(0), usize::MAX)?);
    assert_eq!(positions(&library, id)?, vec![0, 1, 2]);
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);

    assert!(library.remove_from_playlist(id, Span::between(1, usize::MAX))?);
    assert_eq!(held(&library, id)?, vec!["c"]);
    assert_eq!(positions(&library, id)?, vec![0]);

    assert!(!library.remove_from_playlist(id, Span::between(1, usize::MAX))?);
    assert_eq!(positions(&library, id)?, vec![0]);
    Ok(())
}

fn positions(library: &Library, id: PlaylistId) -> Result<Vec<usize>> {
    Ok(library
        .playlist_entries(id, None)?
        .iter()
        .map(|entry| entry.position)
        .collect())
}

#[test]
fn letting_a_kept_order_go_leaves_the_rows_where_they_stand() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("c.wav"), file("b.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Descending,
        }),
    )?;
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);

    assert_eq!(library.keep_playlist_in_order(id, None)?, 0);
    assert_eq!(held(&library, id)?, vec!["c", "b", "a"]);
    assert_eq!(
        library.playlist(id)?.expect("the playlist is there").kept,
        None
    );

    assert!(library.move_in_playlist(id, Span::one(0), 2)?);
    assert_eq!(held(&library, id)?, vec!["b", "a", "c"]);
    library.add_to_playlist(id, slice::from_ref(&file("d.wav")))?;
    assert_eq!(
        held(&library, id)?,
        vec!["b", "a", "c", "d"],
        "a list back in hand put an added row somewhere other than the end"
    );
    Ok(())
}

#[test]
fn keeping_a_list_in_an_order_moves_what_a_pane_redraws_on() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.add_to_playlist(id, &[file("a.wav"), file("b.wav")])?;
    let added = library.playlists_revision();

    let kept = Kept {
        order: RowOrder::File,
        reading: Direction::Ascending,
    };
    assert_eq!(library.keep_playlist_in_order(id, Some(kept))?, 0);
    assert_ne!(
        library.playlists_revision(),
        added,
        "a list that took an order it was already in redrew nothing"
    );
    Ok(())
}

#[test]
fn an_imported_sheet_lands_in_the_order_a_list_is_kept_in() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let id = library.create_playlist("Evening")?;
    library.keep_playlist_in_order(
        id,
        Some(Kept {
            order: RowOrder::Title,
            reading: Direction::Ascending,
        }),
    )?;

    let sheet = tree.path().join("evening.m3u8");
    fs::write(
        &sheet,
        format!(
            "{}\n{}\n{}\n",
            tree.path().join("c.wav").display(),
            tree.path().join("b.wav").display(),
            tree.path().join("a.wav").display()
        ),
    )
    .expect("a writable sheet");

    assert_eq!(library.import_playlist(&sheet, Some("Evening"))?.added, 3);
    assert_eq!(held(&library, id)?, vec!["a", "c", "b"]);
    Ok(())
}

#[test]
fn a_saved_query_is_kept_in_the_order_it_asked_for_and_no_other() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();
    let id = library.save_query("Everything", &SavedQuery::default())?;

    assert!(matches!(
        library.keep_playlist_in_order(
            id,
            Some(Kept {
                order: RowOrder::Title,
                reading: Direction::Ascending,
            })
        ),
        Err(Error::NotAList { .. })
    ));
    assert_eq!(
        library.playlist(id)?.expect("the playlist is there").kept,
        None
    );
    Ok(())
}

#[test]
fn a_saved_query_is_not_a_list_to_edit() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let id = library.save_query("Everything", &SavedQuery::default())?;

    assert!(matches!(
        library.add_to_playlist(id, slice::from_ref(&file)),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.remove_from_playlist(id, Span::one(0)),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.move_in_playlist(id, Span::one(0), 1),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.prune_playlist(id),
        Err(Error::NotAList { .. })
    ));
    assert!(matches!(
        library.sort_playlist(id, RowOrder::Title, Direction::Ascending),
        Err(Error::NotAList { .. })
    ));
    assert_eq!(
        library.playlist_entries(id, None)?.len(),
        3,
        "a refused edit changed what the query answers"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_renamed_dropped_and_written_out_like_any_other() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.save_query(
        "Evening",
        &SavedQuery {
            text: Some("echoes".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert!(matches!(
        library.save_query("evening", &SavedQuery::default()),
        Err(Error::DuplicatePlaylist { .. })
    ));
    library.rename_playlist(id, "Morning")?;

    let sheet = tree.path().join("morning.m3u8");
    assert_eq!(library.export_playlist(id, &sheet)?.rows, 1);
    let written = fs::read_to_string(&sheet).expect("the sheet was written");
    assert!(written.contains("a.wav\n"), "{written}");
    assert!(written.contains(",Echoes\n"), "{written}");

    let read_back = library.import_playlist(&sheet, Some("A list"))?;
    assert!(
        library
            .playlist(read_back.id)?
            .is_some_and(|found| found.query.is_none()),
        "a sheet read back in became a query"
    );

    assert!(library.remove_playlist(id)?);
    assert!(library.playlist(id)?.is_none());
    Ok(())
}

fn scanned_shapes() -> (Tree, Library) {
    let tree = Tree::new();
    tree.write(
        "cold.wav",
        &Wav::new()
            .frames(4_410)
            .text(TITLE, "Cold")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Winter")
            .text(YEAR, "1975")
            .build(),
    );
    tree.write(
        "heat.wav",
        &Wav::new()
            .bits(24)
            .frames(44_100)
            .text(TITLE, "Heat")
            .text(ARTIST, "Ben")
            .text(ALBUM, "Summer")
            .text(YEAR, "1995")
            .build(),
    );

    let library = Library::open_in_memory().expect("an in-memory library");
    scan(&library, &options(&tree)).expect("a scan of two files");
    (tree, library)
}

fn matched(library: &Library, text: &str) -> Result<Vec<String>> {
    let found = library.tracks(&TrackQuery {
        text: Some(text.to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })?;

    Ok(found.into_iter().map(|track| track.title).collect())
}

#[test]
fn a_term_narrows_a_search_to_what_the_file_is() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "depth:24")?, vec!["Heat"]);
    assert_eq!(matched(&library, "depth:>16")?, vec!["Heat"]);
    assert_eq!(matched(&library, "depth:16")?, vec!["Cold"]);
    assert_eq!(matched(&library, "is:hires")?, vec!["Heat"]);
    assert_eq!(matched(&library, "is:lossless")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "is:lossy")?.is_empty());
    assert_eq!(matched(&library, "is:stereo")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "is:mono")?.is_empty());
    assert_eq!(matched(&library, "codec:pcm")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "codec:flac")?.is_empty());
    assert_eq!(matched(&library, "rate:44.1k")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "rate:>=96k")?.is_empty());
    Ok(())
}

#[test]
fn a_year_is_read_off_the_album_and_a_length_off_the_frames() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "year:1975")?, vec!["Cold"]);
    assert_eq!(matched(&library, "year:1970-1979")?, vec!["Cold"]);
    assert_eq!(matched(&library, "year:>=1990")?, vec!["Heat"]);
    assert_eq!(matched(&library, "length:>0.5s")?, vec!["Heat"]);
    assert_eq!(matched(&library, "length:<0.5s")?, vec!["Cold"]);
    assert!(matched(&library, "length:>1h")?.is_empty());
    Ok(())
}

#[test]
fn an_age_is_measured_back_from_now() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "added:<1d")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "added:1y")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "added:>1d")?.is_empty());
    Ok(())
}

#[test]
fn a_play_is_counted_against_the_track_it_named() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    let once = library
        .track_played(&cold, None)?
        .expect("the row the play counted");
    assert_eq!(once.track.plays, 1);
    assert_eq!(once.track.location, cold);
    assert!(once.track.played.is_some());

    let twice = library
        .track_played(&cold, None)?
        .expect("the row the play counted");
    assert_eq!(twice.track.plays, 2);
    assert_eq!(twice.track.id, once.track.id);
    assert_ne!(twice.listen, once.listen);

    let played = library
        .track_at(tree.path().join("cold.wav").as_path(), None)?
        .expect("the track the scan stored");
    assert_eq!(played, twice.track);

    let heat = library
        .track_at(tree.path().join("heat.wav").as_path(), None)?
        .expect("the track the scan stored");
    assert_eq!(heat.plays, 0);
    assert_eq!(heat.played, None);
    Ok(())
}

#[test]
fn a_play_of_something_the_catalog_does_not_hold_counts_nothing() -> Result<()> {
    let (tree, library) = scanned_shapes();

    assert!(
        library
            .track_played(&MediaLocation::local(tree.path().join("nothing.wav")), None)?
            .is_none()
    );
    assert!(
        library
            .track_played(
                &MediaLocation::new(
                    SourceId::new("radio").expect("a lowercase name"),
                    "a-stream"
                ),
                None
            )?
            .is_none()
    );
    Ok(())
}

#[derive(Default)]
struct Told {
    batches: Mutex<Vec<Vec<Scrobble>>>,
    malformed: Option<&'static str>,
    unreachable: std::sync::atomic::AtomicBool,
}

impl Told {
    fn refusing(title: &'static str) -> Self {
        Self {
            malformed: Some(title),
            ..Self::default()
        }
    }

    fn titles(&self) -> Vec<Vec<String>> {
        self.batches
            .lock()
            .iter()
            .map(|batch| batch.iter().map(|told| told.title.clone()).collect())
            .collect()
    }
}

impl Scrobbler for Told {
    fn service(&self) -> ListeningService {
        ListeningService::ListenBrainz
    }

    fn submit(&self, listens: &[Scrobble]) -> Result<()> {
        if self.unreachable.load(Ordering::Relaxed) {
            return Err(Error::Unreachable {
                op: LookupOp::Submit,
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            });
        }
        self.batches.lock().push(listens.to_vec());
        match self.malformed {
            Some(title) if listens.iter().any(|told| told.title == title) => Err(Error::Refused {
                op: LookupOp::Submit,
                status: 400,
            }),
            _ => Ok(()),
        }
    }
}

fn scanned_listening() -> (Tree, Library, [MediaLocation; 3]) {
    let tree = Tree::new();
    let cold = tree.write(
        "cold.wav",
        &Wav::new()
            .text(TITLE, "Cold")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Winter")
            .text(TRACK, "3")
            .build(),
    );
    let heat = tree.write(
        "heat.wav",
        &Wav::new()
            .frames(44_100)
            .text(TITLE, "Heat")
            .text(ARTIST, "Ben")
            .build(),
    );
    let loose = tree.write("loose.wav", &Wav::new().text(TITLE, "Loose").build());

    let library = Library::open_in_memory().expect("an in-memory library");
    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library, [cold, heat, loose].map(MediaLocation::local))
}

#[test]
fn a_service_is_told_what_was_heard_after_it_was_first_asked_and_each_play_once() -> Result<()> {
    let (_tree, library, [cold, heat, loose]) = scanned_listening();
    library.track_played(&cold, None)?;
    let told = Told::default();

    let started = library.submit_listens(&told)?;
    assert!(started.started);
    assert_eq!(started.submitted, 0);
    assert!(told.titles().is_empty());

    library.track_played(&heat, None)?;
    library.track_played(&loose, None)?;
    library.track_played(&cold, None)?;
    let submitted = library.submit_listens(&told)?;

    assert_eq!(submitted.submitted, 2);
    assert_eq!(submitted.unnamed, 1);
    assert!(!submitted.started);
    assert_eq!(told.titles(), vec![vec!["Heat", "Cold"]]);
    let batch = told.batches.lock()[0].clone();
    assert_eq!(batch[0].artist, "Ben");
    assert_eq!(batch[0].album, None);
    assert_eq!(batch[0].length, Some(Duration::from_secs(1)));
    assert_eq!(batch[1].artist, "Ada");
    assert_eq!(batch[1].album.as_deref(), Some("Winter"));
    assert_eq!(batch[1].number, Some(3));
    assert!(batch[0].listen < batch[1].listen);
    assert!(batch[0].at <= batch[1].at);

    assert_eq!(library.submit_listens(&told)?.submitted, 0);
    assert_eq!(told.titles().len(), 1);
    Ok(())
}

#[test]
fn a_play_the_service_refuses_as_malformed_is_passed_over_and_the_rest_are_told() -> Result<()> {
    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::refusing("Heat");
    library.submit_listens(&told)?;

    library.track_played(&cold, None)?;
    library.track_played(&heat, None)?;
    library.track_played(&cold, None)?;
    let submitted = library.submit_listens(&told)?;

    assert_eq!(submitted.submitted, 2);
    assert_eq!(submitted.refused, 1);
    assert_eq!(
        told.titles(),
        vec![
            vec!["Cold", "Heat", "Cold"],
            vec!["Cold"],
            vec!["Heat"],
            vec!["Cold"],
        ]
    );

    assert_eq!(library.submit_listens(&told)?, Default::default());
    assert_eq!(told.titles().len(), 4);
    Ok(())
}

#[test]
fn a_play_a_service_could_not_be_reached_for_is_told_the_next_time() -> Result<()> {
    let (_tree, library, [cold, heat, _]) = scanned_listening();
    let told = Told::default();
    library.submit_listens(&told)?;
    library.track_played(&cold, None)?;
    library.track_played(&heat, None)?;

    told.unreachable.store(true, Ordering::Relaxed);
    assert!(matches!(
        library.submit_listens(&told),
        Err(Error::Unreachable {
            op: LookupOp::Submit,
            ..
        })
    ));

    told.unreachable.store(false, Ordering::Relaxed);
    assert_eq!(library.submit_listens(&told)?.submitted, 2);
    assert_eq!(told.titles(), vec![vec!["Cold", "Heat"]]);
    Ok(())
}

#[test]
fn a_rescan_keeps_how_often_a_track_was_played() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().text(TITLE, "before").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert!(
        library
            .track_played(&MediaLocation::local(tree.path().join("one.wav")), None)?
            .is_some()
    );

    tree.write(
        "one.wav",
        &Wav::new().frames(8_820).text(TITLE, "after").build(),
    );
    scan(&library, &options(&tree))?;

    let tracks = all(&library)?;
    let track = tracks.first().expect("the track is still there");
    assert_eq!(track.title, "after");
    assert_eq!(track.plays, 1);
    assert!(track.played.is_some());
    Ok(())
}

#[test]
fn a_search_narrows_by_how_often_and_how_lately_a_track_was_played() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    library.track_played(&cold, None)?;
    library.track_played(&cold, None)?;

    assert_eq!(matched(&library, "plays:0")?, vec!["Heat"]);
    assert_eq!(matched(&library, "plays:>0")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:2")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1-3")?, vec!["Cold"]);
    assert!(matched(&library, "plays:>2")?.is_empty());
    assert_eq!(matched(&library, "played:<1d")?, vec!["Cold"]);
    assert!(matched(&library, "played:>1d")?.is_empty());
    Ok(())
}

#[test]
fn a_play_counted_against_a_track_is_a_listen_a_window_can_narrow_on() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    assert!(matched(&library, "plays:>0@30d")?.is_empty());

    library.track_played(&cold, None)?;
    assert_eq!(matched(&library, "plays:>0@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:0@30d")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn a_window_narrows_on_how_often_a_track_was_heard_inside_it() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    let heat = MediaLocation::local(tree.path().join("heat.wav"));
    for _ in 0..3 {
        library.track_played(&cold, None)?;
    }
    library.track_played(&heat, None)?;

    assert_eq!(matched(&library, "plays:>1@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:>0@30d")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "plays:3@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1-3@30d")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "plays:>3@30d")?.is_empty());
    assert_eq!(matched(&library, "-plays:>1@30d")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn forgetting_a_root_takes_the_listens_counted_under_it_with_it() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    library.track_played(&cold, None)?;
    library.track_played(&cold, None)?;
    assert_eq!(matched(&library, "plays:>1@30d")?, vec!["Cold"]);

    assert!(library.remove_root(tree.path())?);
    assert!(all(&library)?.is_empty());

    scan(&library, &options(&tree))?;
    assert_eq!(titles(&all(&library)?), vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "plays:0")?, vec!["Cold", "Heat"]);
    assert!(
        matched(&library, "plays:>0@30d")?.is_empty(),
        "a rescanned row answered for the listens a forgotten root left orphaned"
    );
    Ok(())
}

#[test]
fn a_track_never_played_answers_a_denied_play_term() -> Result<()> {
    let (tree, library) = scanned_shapes();
    library.track_played(&MediaLocation::local(tree.path().join("cold.wav")), None)?;

    assert_eq!(matched(&library, "-played:<1d")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-plays:0")?, vec!["Cold"]);
    Ok(())
}

#[test]
fn a_track_nothing_has_played_falls_to_the_bottom_of_both_play_orders() -> Result<()> {
    let (tree, library) = scanned_shapes();
    library.track_played(&MediaLocation::local(tree.path().join("heat.wav")), None)?;

    let lately = library.tracks(&TrackQuery {
        sort: SortOrder::Played,
        reading: SortOrder::Played.reads(),
        ..TrackQuery::default()
    })?;
    assert_eq!(titles(&lately), vec!["Heat", "Cold"]);

    let most = library.tracks(&TrackQuery {
        sort: SortOrder::Plays,
        reading: SortOrder::Plays.reads(),
        ..TrackQuery::default()
    })?;
    assert_eq!(titles(&most), vec!["Heat", "Cold"]);
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_what_was_played_most() -> Result<()> {
    let (tree, library) = scanned_shapes();
    library.track_played(&MediaLocation::local(tree.path().join("heat.wav")), None)?;

    let id = library.save_query(
        "On repeat",
        &SavedQuery {
            text: Some("plays:>0".to_owned()),
            sort: SortOrder::Plays,
            reading: SortOrder::Plays.reads(),
            limit: Some(25),
        },
    )?;
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["heat"]);

    library.track_played(&MediaLocation::local(tree.path().join("cold.wav")), None)?;
    library.track_played(&MediaLocation::local(tree.path().join("cold.wav")), None)?;
    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["cold", "heat"]);

    let revised = library
        .playlist(id)?
        .expect("the saved query is still there");
    assert_eq!(
        revised.query.map(|query| query.sort),
        Some(SortOrder::Plays)
    );
    Ok(())
}

#[test]
fn terms_and_words_all_have_to_hold_at_once() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "heat is:hires")?, vec!["Heat"]);
    assert!(matched(&library, "cold is:hires")?.is_empty());
    assert_eq!(matched(&library, "is:lossless year:>=1990")?, vec!["Heat"]);
    assert!(matched(&library, "year:1975 year:>=1990")?.is_empty());
    Ok(())
}

#[test]
fn a_denied_token_takes_out_whatever_it_names() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "-is:hires")?, vec!["Cold"]);
    assert_eq!(matched(&library, "is:lossless -depth:24")?, vec!["Cold"]);
    assert_eq!(matched(&library, "-year:1975")?, vec!["Heat"]);
    assert!(matched(&library, "-codec:pcm")?.is_empty());
    assert_eq!(matched(&library, "-cold")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-artist:ada")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn a_denied_range_leaves_what_falls_either_side_of_it() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "-year:1970-1979")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-year:1970-1999")?.len(), 0);
    Ok(())
}

#[test]
fn either_of_two_holds_where_neither_alone_would() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(
        matched(&library, "depth:16 or depth:24")?,
        vec!["Cold", "Heat"]
    );
    assert_eq!(
        matched(&library, "year:1975 or year:1995")?,
        vec!["Cold", "Heat"]
    );
    assert_eq!(matched(&library, "is:hires or ada")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "is:mono or is:hires")?, vec!["Heat"]);
    assert_eq!(
        matched(&library, "is:lossless year:1975 or is:hires")?,
        vec!["Cold", "Heat"],
        "a clause beside an alternation should still have to hold"
    );
    assert_eq!(
        matched(&library, "ada or ben -is:hires")?,
        vec!["Cold"],
        "a denial beside an alternation should still have to hold"
    );
    Ok(())
}

#[test]
fn a_denial_and_an_alternation_reach_the_album_and_artist_listings_too() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    let albums = library.albums(&AlbumQuery {
        text: Some("-is:hires".to_owned()),
        ..AlbumQuery::default()
    })?;
    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Winter"]
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("year:1975 or year:1995".to_owned()),
        ..ArtistQuery::default()
    })?;
    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Ada", "Ben"]
    );
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_what_it_denies() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let id = library.save_query(
        "Anything but hi-res",
        &SavedQuery {
            text: Some("-is:hires".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["cold"]);

    tree.write("more.wav", &Wav::new().text(TITLE, "Warmth").build());
    scan(&library, &options(&tree))?;
    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["cold", "more"],
        "a file the scan added did not reach a saved query that names what it is not"
    );
    Ok(())
}

#[test]
fn a_column_holds_the_words_it_names_to_that_field() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "artist:ada")?, vec!["Cold"]);
    assert_eq!(matched(&library, "album:winter")?, vec!["Cold"]);
    assert_eq!(matched(&library, "title:cold")?, vec!["Cold"]);
    assert!(
        matched(&library, "title:ada")?.is_empty(),
        "an artist's name matched through the title column"
    );
    assert_eq!(matched(&library, "ada")?, vec!["Cold"]);
    Ok(())
}

#[test]
fn a_term_reaches_the_album_and_artist_listings_too() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    let albums = library.albums(&AlbumQuery {
        text: Some("depth:24".to_owned()),
        ..AlbumQuery::default()
    })?;
    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Summer"]
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("is:hires".to_owned()),
        ..ArtistQuery::default()
    })?;
    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Ben"]
    );

    let searched = library.search("year:1975", 10)?;
    assert_eq!(searched.tracks.len(), 1);
    assert_eq!(searched.albums.len(), 1);
    assert_eq!(searched.artists.len(), 1);
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_its_terms() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let id = library.save_query(
        "Hi-res",
        &SavedQuery {
            text: Some("is:hires".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["heat"]);
    assert_eq!(library.playlist(id)?.map(|found| found.entries), Some(1));

    tree.write(
        "more.wav",
        &Wav::new().bits(24).text(TITLE, "Warmth").build(),
    );
    scan(&library, &options(&tree))?;
    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(2),
        "a hi-res track the scan added did not reach the saved query"
    );
    Ok(())
}

#[test]
fn a_playlist_is_copied_into_another_rather_than_moved_out_of() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.add_to_playlist(anthems, slice::from_ref(&file("b.wav")))?;

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["b", "c", "a"],
        "a copy did not land at the end in the order it was taken"
    );
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a"],
        "a copy took the rows out of the playlist it came from"
    );

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["b", "c", "a", "c", "a"],
        "a second copy reconciled rather than appending"
    );
    Ok(())
}

#[test]
fn a_copy_refuses_the_playlist_it_came_from() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = Cut::whole(MediaLocation::local(tree.path().join("a.wav")));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, slice::from_ref(&file))?;

    assert!(matches!(
        library.copy_playlist(evening, evening, None),
        Err(Error::IntoItself { .. })
    ));
    assert_eq!(
        held(&library, evening)?,
        vec!["a"],
        "a playlist copied into itself doubled"
    );

    let unknown = PlaylistId::new(404)?;
    assert!(matches!(
        library.copy_playlist(evening, unknown, None),
        Err(Error::UnknownPlaylist(_))
    ));
    assert!(matches!(
        library.copy_playlist(unknown, evening, None),
        Err(Error::UnknownPlaylist(_))
    ));

    let query = library.save_query("Everything", &SavedQuery::default())?;
    assert!(matches!(
        library.copy_playlist(evening, query, None),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn a_search_narrowing_a_saved_query_is_read_beside_it_rather_than_into_it() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let floyd = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("\"pink floyd".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(
        narrowed_to(&library, floyd, None)?,
        vec!["Dogs"],
        "an unbalanced quote did not close at the end of the text it was written in"
    );
    assert_eq!(
        narrowed_to(&library, floyd, Some("dogs"))?,
        vec!["Dogs"],
        "the typed words were swallowed into the phrase the saved text left open"
    );

    let dangling = library.save_query(
        "Dangling",
        &SavedQuery {
            text: Some("dogs or".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    assert!(
        narrowed_to(&library, dangling, Some("sheep"))?.is_empty(),
        "a trailing or alternated with a word the saved text never held"
    );
    Ok(())
}

fn narrowed_to(library: &Library, id: PlaylistId, matching: Option<&str>) -> Result<Vec<String>> {
    Ok(library
        .playlist_entries(id, matching)?
        .into_iter()
        .filter_map(|entry| entry.track.map(|track| track.title))
        .collect())
}

#[test]
fn a_saved_query_is_frozen_into_a_list_by_copying_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let query = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("artist:floyd".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    let kept = library.create_playlist("Anthems")?;

    assert_eq!(library.copy_playlist(query, kept, None)?, 1);
    assert_eq!(held(&library, kept)?, vec!["a"]);

    tree.write(
        "d.wav",
        &Wav::new()
            .text(TITLE, "Pigs")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.playlist(query)?.map(|found| found.entries),
        Some(2),
        "a scan did not reach the query the copy was taken from"
    );
    assert_eq!(
        held(&library, kept)?,
        vec!["a"],
        "the list a query was copied into followed the query afterwards"
    );
    Ok(())
}

#[test]
fn a_search_narrows_what_a_copy_takes() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.copy_playlist(evening, anthems, Some("dogs"))?, 1);
    assert_eq!(
        held(&library, anthems)?,
        vec!["a"],
        "a copy took rows the search did not match"
    );
    assert_eq!(
        library.copy_playlist(evening, anthems, Some("nothing at all"))?,
        0
    );
    assert_eq!(held(&library, anthems)?, vec!["a"]);
    Ok(())
}

#[test]
fn a_copy_into_a_kept_list_lands_where_the_order_puts_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    let anthems = library.create_playlist("Anthems")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("b.wav")])?;
    library.add_to_playlist(anthems, slice::from_ref(&file("a.wav")))?;
    library.keep_playlist_in_order(
        anthems,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.copy_playlist(evening, anthems, None)?, 2);
    assert_eq!(
        held(&library, anthems)?,
        vec!["a", "b", "c"],
        "a copy into a kept list landed at the end rather than in its order"
    );
    Ok(())
}

#[test]
fn a_search_takes_the_rows_it_matched_out_of_a_playlist() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "b"],
        "a drop took the rows the search did not match"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "the rows left behind were not closed up"
    );

    assert_eq!(library.remove_matching(evening, "nothing at all")?, 0);
    assert_eq!(held(&library, evening)?, vec!["c", "b"]);

    assert_eq!(library.remove_matching(evening, "is:lossless")?, 2);
    assert!(
        held(&library, evening)?.is_empty(),
        "a search matching every row left one behind"
    );
    Ok(())
}

#[test]
fn a_row_no_scan_has_seen_survives_every_search_that_drops() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let unscanned = tree.write("late.wav", &Wav::new().text(TITLE, "Dogs").build());
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(
        evening,
        &[
            Cut::whole(MediaLocation::local(tree.path().join("a.wav"))),
            Cut::whole(MediaLocation::local(unscanned)),
        ],
    )?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["late"],
        "a row the catalog has never seen answered a search that drops"
    );
    Ok(())
}

#[test]
fn a_saved_query_has_no_rows_to_drop() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let query = library.save_query("Floyd", &SavedQuery::default())?;

    assert!(matches!(
        library.remove_matching(query, "dogs"),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn a_kept_list_is_dropped_out_of_and_stays_in_its_order() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["b", "c"],
        "a kept list lost its order when a search dropped a row"
    );
    Ok(())
}

#[test]
fn what_a_search_dropped_is_put_back_where_it_stood() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;

    assert_eq!(library.remove_matching(evening, "dogs")?, 1);
    assert_eq!(held(&library, evening)?, vec!["c", "b"]);
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Dropped),
        "the drop left nothing to put back"
    );

    let put_back = library.undo()?.expect("the drop is there to put back");
    assert_eq!(put_back.edit, Edit::Dropped);
    assert_eq!(put_back.playlist, evening);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "b"],
        "the row came back somewhere other than where it went from"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the rows put back were not dense again"
    );
    Ok(())
}

#[test]
fn folding_keeps_the_first_of_every_doubled_row_and_drops_the_rest() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(
        evening,
        &[
            file("c.wav"),
            file("a.wav"),
            file("c.wav"),
            file("b.wav"),
            file("c.wav"),
            file("a.wav"),
        ],
    )?;

    assert_eq!(library.fold_doubles(evening)?, 3);
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "b"],
        "folding kept a row other than the first of each file"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the rows left behind were not closed up"
    );
    Ok(())
}

#[test]
fn folding_a_playlist_that_holds_each_file_once_writes_nothing() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    let unchanged = library.playlist(evening)?.expect("the playlist").modified;

    assert_eq!(library.fold_doubles(evening)?, 0);
    assert_eq!(held(&library, evening)?, vec!["c", "a", "b"]);
    assert_eq!(
        library.playlist(evening)?.expect("the playlist").modified,
        unchanged,
        "a fold that dropped nothing still marked the playlist as changed"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "a fold that dropped nothing still left a step to walk back"
    );
    Ok(())
}

#[test]
fn what_a_fold_took_out_is_put_back_where_it_stood() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("c.wav")])?;

    assert_eq!(library.fold_doubles(evening)?, 1);
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);

    let put_back = library.undo()?.expect("the fold is there to put back");
    assert_eq!(put_back.edit, Edit::Folded);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["c", "a", "c"],
        "the double came back somewhere other than where it went from"
    );

    assert_eq!(library.redo()?.map(|again| again.edit), Some(Edit::Folded));
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);
    Ok(())
}

#[test]
fn a_kept_list_is_folded_and_stays_in_its_order() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("c.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;

    assert_eq!(library.fold_doubles(evening)?, 1);
    assert_eq!(
        held(&library, evening)?,
        vec!["a", "c"],
        "a kept list lost its order when a fold dropped a row"
    );
    Ok(())
}

#[test]
fn a_saved_query_has_no_doubles_to_fold() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let query = library.save_query("Floyd", &SavedQuery::default())?;

    assert!(matches!(
        library.fold_doubles(query),
        Err(Error::NotAList { .. })
    ));
    Ok(())
}

#[test]
fn undoing_walks_back_one_edit_at_a_time() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.add_to_playlist(evening, &[file("b.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;
    library.remove_from_playlist(evening, Span::one(0))?;

    assert_eq!(held(&library, evening)?, vec!["b", "c"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Removed)
    );
    assert_eq!(held(&library, evening)?, vec!["a", "b", "c"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Ordered)
    );
    assert_eq!(held(&library, evening)?, vec!["c", "a", "b"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Added)
    );
    assert_eq!(held(&library, evening)?, vec!["c", "a"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Added)
    );
    assert!(held(&library, evening)?.is_empty());

    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Started)
    );
    assert!(
        library.playlist(evening)?.is_none(),
        "undoing the start of a playlist left it behind"
    );
    assert_eq!(library.undo()?, None, "there was a step nothing had taken");
    Ok(())
}

#[test]
fn a_step_says_how_many_are_behind_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;

    assert_eq!(
        library.undoable().map(|undoable| undoable.behind),
        Some(2),
        "the step on offer did not count the start and the addition under it"
    );
    assert_eq!(library.undo()?.map(|put_back| put_back.behind), Some(2));
    assert_eq!(library.undoable().map(|undoable| undoable.behind), Some(1));
    assert_eq!(
        library.redoable().map(|again| again.behind),
        Some(0),
        "the one step walked back counted others behind it"
    );

    library.undo()?.expect("the addition is there to put back");
    library.undo()?.expect("the start is there to put back");
    assert_eq!(library.undoable(), None);
    assert_eq!(
        library.redoable().map(|again| again.behind),
        Some(2),
        "the steps walked back were not counted on the other stack"
    );
    Ok(())
}

#[test]
fn a_discarded_playlist_comes_back_under_the_id_it_had() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let kept = Kept {
        order: RowOrder::File,
        reading: Direction::Descending,
    };
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.keep_playlist_in_order(evening, Some(kept))?;
    let standing = library.playlist(evening)?.expect("the playlist is there");

    assert!(library.remove_playlist(evening)?);
    assert!(library.playlist(evening)?.is_none());

    let put_back = library.undo()?.expect("the discard is there to put back");
    assert_eq!(put_back.edit, Edit::Discarded);
    assert_eq!(put_back.name, "Evening");
    assert_eq!(
        library.playlist(evening)?,
        Some(standing),
        "the playlist came back as something other than what went"
    );
    assert_eq!(held(&library, evening)?, vec!["c", "b", "a"]);
    Ok(())
}

#[test]
fn a_saved_query_comes_back_with_the_search_it_filled_itself_from() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let asked = SavedQuery {
        text: Some("dogs".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: None,
    };
    let floyd = library.save_query("Floyd", &asked)?;
    library.revise_query(
        floyd,
        "Barrett",
        &SavedQuery {
            text: Some("echoes".to_owned()),
            sort: SortOrder::Artist,
            reading: SortOrder::Artist.reads(),
            limit: Some(5),
        },
    )?;

    let put_back = library.undo()?.expect("the revision is there to put back");
    assert_eq!(put_back.edit, Edit::Revised);
    assert_eq!(
        put_back.name, "Floyd",
        "the name the revision took is not what came back"
    );

    let found = library.playlist(floyd)?.expect("the query is still there");
    assert_eq!(found.name, "Floyd");
    assert_eq!(found.query, Some(asked));
    Ok(())
}

#[test]
fn an_edit_that_moved_nothing_leaves_nothing_to_put_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav"), file("c.wav")])?;

    assert_eq!(
        library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?,
        0
    );
    assert_eq!(library.remove_matching(evening, "nothing at all")?, 0);
    assert_eq!(library.prune_playlist(evening)?, 0);
    assert!(!library.move_in_playlist(evening, Span::one(0), 0)?);
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Added),
        "an edit that moved no row still left a step to walk back"
    );
    Ok(())
}

#[test]
fn a_play_counted_between_an_edit_and_its_undo_is_not_taken_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.remove_from_playlist(evening, Span::one(1))?;
    library.set_playing_playlist(loaded(evening));

    library.undo()?.expect("the removal is there to put back");
    let found = library.playlist(evening)?.expect("the playlist is there");
    assert_eq!(
        found.plays, 1,
        "the play the undo crossed was taken back with it"
    );
    assert!(found.played.is_some());
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn only_so_many_steps_are_held_to_walk_back() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let evening = library.create_playlist("hold 0")?;
    for step in 1..=40 {
        library.rename_playlist(evening, &format!("hold {step}"))?;
    }

    let mut walked = 0;
    while library.undo()?.is_some() {
        walked += 1;
    }
    assert_eq!(walked, 32, "the stack held a different number of steps");
    assert_eq!(
        library.playlist(evening)?.map(|found| found.name),
        Some("hold 8".to_owned()),
        "the steps that went were not the oldest ones"
    );
    Ok(())
}

#[test]
fn a_rename_is_a_step_that_holds_a_name_rather_than_the_rows_beneath_it() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let evening = library.create_playlist("hold 0")?;
    let rows: Vec<Cut> = (0..2_000)
        .map(|row| Cut::whole(MediaLocation::local(tree.path().join(format!("{row}.wav")))))
        .collect();
    library.add_to_playlist(evening, &rows)?;
    for step in 1..=32 {
        library.rename_playlist(evening, &format!("hold {step}"))?;
    }

    assert_eq!(
        library.undoable().map(|step| step.behind),
        Some(31),
        "the rows under a rename were counted against the stack's own bound"
    );

    let mut walked = 0;
    while library.undo()?.is_some() {
        walked += 1;
    }
    assert_eq!(walked, 32, "the stack held a different number of steps");

    let found = library.playlist(evening)?.expect("the playlist is there");
    assert_eq!(found.name, "hold 0");
    assert_eq!(
        found.entries, 2_000,
        "walking back a rename moved the rows under it"
    );
    Ok(())
}

#[test]
fn a_copy_and_an_import_are_each_one_step_to_walk_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    let morning = library.create_playlist("Morning")?;
    library.copy_playlist(evening, morning, None)?;

    assert_eq!(held(&library, morning)?, vec!["a", "b"]);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Copied)
    );
    assert!(
        held(&library, morning)?.is_empty(),
        "one copy took more than one step to walk back"
    );

    let sheet = tree.write(
        "night.m3u8",
        format!(
            "{}\n{}\n",
            tree.path().join("a.wav").display(),
            tree.path().join("c.wav").display()
        )
        .as_bytes(),
    );
    let read = library.import_playlist(&sheet, Some("Night"))?;
    assert_eq!(read.added, 2);
    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Imported)
    );
    assert!(
        library.playlist_named("Night")?.is_none(),
        "the sheet that made a playlist took more than one step to walk back"
    );
    Ok(())
}

#[test]
fn a_step_walked_back_is_put_back_again() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav"), file("b.wav")])?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;
    library.remove_from_playlist(evening, Span::one(0))?;

    assert_eq!(held(&library, evening)?, vec!["b", "c"]);
    assert_eq!(
        library.redoable(),
        None,
        "there was a step nothing had walked"
    );
    assert_eq!(
        library.redo()?,
        None,
        "a step was put back that nothing took"
    );

    assert_eq!(
        library.undo()?.map(|put_back| put_back.edit),
        Some(Edit::Removed)
    );
    assert_eq!(held(&library, evening)?, vec!["a", "b", "c"]);
    assert_eq!(
        library.redoable().map(|again| again.edit),
        Some(Edit::Removed),
        "the step walked back was not there to put back again"
    );

    let again = library.redo()?.expect("the removal is there to do again");
    assert_eq!(again.edit, Edit::Removed);
    assert_eq!(again.playlist, evening);
    assert_eq!(again.name, "Evening");
    assert_eq!(
        held(&library, evening)?,
        vec!["b", "c"],
        "putting the edit back left the playlist somewhere else"
    );
    assert_eq!(
        library
            .playlist_entries(evening, None)?
            .iter()
            .map(|entry| entry.position)
            .collect::<Vec<_>>(),
        vec![0, 1],
        "the rows put back again were not dense"
    );
    assert_eq!(
        library.redo()?,
        None,
        "one redo put back more than one step"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Removed),
        "a step put back again was not there to walk back once more"
    );
    Ok(())
}

#[test]
fn walking_forward_crosses_every_step_walking_back_did() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.rename_playlist(evening, "Morning")?;
    library.sort_playlist(evening, RowOrder::File, Direction::Ascending)?;

    let mut walked = Vec::new();
    while let Some(put_back) = library.undo()? {
        walked.push(put_back.edit);
    }
    assert_eq!(
        walked,
        vec![Edit::Ordered, Edit::Renamed, Edit::Added, Edit::Started]
    );
    assert!(library.playlist(evening)?.is_none());

    let mut again = Vec::new();
    while let Some(done) = library.redo()? {
        again.push(done.edit);
    }
    assert_eq!(
        again,
        vec![Edit::Started, Edit::Added, Edit::Renamed, Edit::Ordered],
        "walking forward crossed other steps than walking back had"
    );

    let found = library.playlist(evening)?.expect("the playlist is back");
    assert_eq!(
        found.name, "Morning",
        "the name the edits left did not come back"
    );
    assert_eq!(held(&library, evening)?, vec!["a", "c"]);
    Ok(())
}

#[test]
fn an_edit_forgets_what_was_walked_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.remove_from_playlist(evening, Span::one(0))?;
    library.undo()?.expect("the removal is there to put back");

    assert!(library.redoable().is_some());
    library.rename_playlist(evening, "Morning")?;
    assert_eq!(
        library.redoable(),
        None,
        "an edit made after a step was walked back still offered to put it back"
    );
    assert_eq!(library.redo()?, None);
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn an_edit_that_moved_nothing_forgets_nothing_that_was_walked_back() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("a.wav"), file("b.wav")])?;
    library.undo()?.expect("the addition is there to put back");

    assert_eq!(library.prune_playlist(evening)?, 0);
    assert_eq!(
        library.redoable().map(|again| again.edit),
        Some(Edit::Added),
        "an edit that wrote nothing took away what was there to put back"
    );
    library.redo()?.expect("the addition is there to do again");
    assert_eq!(held(&library, evening)?, vec!["a", "b"]);
    Ok(())
}

#[test]
fn a_discarded_playlist_goes_again_under_the_id_it_came_back_with() -> Result<()> {
    let (tree, library) = sortable_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.create_playlist("Evening")?;
    library.add_to_playlist(evening, &[file("c.wav"), file("a.wav")])?;
    library.keep_playlist_in_order(
        evening,
        Some(Kept {
            order: RowOrder::File,
            reading: Direction::Ascending,
        }),
    )?;
    let standing = library.playlist(evening)?.expect("the playlist is there");
    assert!(library.remove_playlist(evening)?);

    library.undo()?.expect("the discard is there to put back");
    assert_eq!(library.playlist(evening)?, Some(standing));

    let again = library.redo()?.expect("the discard is there to do again");
    assert_eq!(again.edit, Edit::Discarded);
    assert_eq!(again.name, "Evening");
    assert!(
        library.playlist(evening)?.is_none(),
        "the playlist put back stayed after the discard was done again"
    );
    assert_eq!(
        library.undoable().map(|undoable| undoable.edit),
        Some(Edit::Discarded),
        "the discard done again left nothing to walk back"
    );
    Ok(())
}

#[test]
fn a_saved_query_is_revised_again_after_the_revision_was_walked_back() -> Result<()> {
    let (_tree, library) = sortable_playlist_tree();
    let revised = SavedQuery {
        text: Some("echoes".to_owned()),
        sort: SortOrder::Artist,
        reading: SortOrder::Artist.reads(),
        limit: Some(5),
    };
    let floyd = library.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("dogs".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    library.revise_query(floyd, "Barrett", &revised)?;
    library.undo()?.expect("the revision is there to put back");

    let again = library.redo()?.expect("the revision is there to do again");
    assert_eq!(again.edit, Edit::Revised);
    assert_eq!(
        again.name, "Barrett",
        "the revision put back named the query what it was before"
    );

    let found = library.playlist(floyd)?.expect("the query is still there");
    assert_eq!(found.name, "Barrett");
    assert_eq!(found.query, Some(revised));
    Ok(())
}

const MEDDLE_SHEET: &str = r#"REM DATE 1971
PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.wav" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Pillow of Winds"
    INDEX 01 00:00:20
  TRACK 03 AUDIO
    TITLE "Echoes"
    INDEX 01 00:00:40
"#;

fn scanned_sheet() -> (Tree, Library) {
    let tree = Tree::new();
    tree.write("Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());

    let library = Library::open_in_memory().expect("an in-memory library");
    scan(&library, &options(&tree)).expect("a scan of one file a sheet cuts");
    (tree, library)
}

#[test]
fn a_file_a_cue_sheet_cuts_is_scanned_as_the_tracks_the_sheet_names() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(rows.len(), 3, "the sheet's tracks did not become rows");
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["One of These Days", "A Pillow of Winds", "Echoes"]
    );

    let held = MediaLocation::local(tree.path().join("Meddle.wav"));
    for row in &rows {
        assert_eq!(row.location, held, "every row names the one file");
        assert_eq!(row.artist.as_deref(), Some("Pink Floyd"));
    }

    let spans: Vec<Option<Frames>> = rows
        .iter()
        .map(|row| row.span.map(FrameSpan::start))
        .collect();
    assert_eq!(
        spans,
        vec![
            Some(Frames::ZERO),
            Some(Frames(44_100 / 75 * 20)),
            Some(Frames(44_100 / 75 * 40)),
        ],
        "the rows do not start where the sheet says"
    );
    Ok(())
}

const GAPS_APPENDED_SHEET: &str = "PERFORMER \"AURORA\"\r\nTITLE \"The Gods We Can Touch\"\r\nFILE \"13.wav\" WAVE\r\n  TRACK 13 AUDIO\r\n    TITLE \"A Little Place Called The Moon\"\r\n    INDEX 01 00:00:00\r\n  TRACK 14 AUDIO\r\n    TITLE \"Blood In The Wine\"\r\n    INDEX 00 00:00:70\r\nFILE \"14.wav\" WAVE\r\n    INDEX 01 00:00:00\r\n";

#[test]
fn a_sheet_that_ends_each_file_with_the_next_tracks_gap_is_scanned_as_one_whole_row_a_file()
-> Result<()> {
    let tree = Tree::new();
    tree.write("13.wav", &Wav::new().frames(44_100).build());
    tree.write("14.wav", &Wav::new().frames(44_100).build());
    tree.write("The Gods We Can Touch.cue", GAPS_APPENDED_SHEET.as_bytes());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    let held: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                row.location.clone(),
                row.title.as_str(),
                row.span.map(FrameSpan::start),
                row.duration,
            )
        })
        .collect();
    assert_eq!(
        held,
        vec![
            (
                MediaLocation::local(tree.path().join("13.wav")),
                "A Little Place Called The Moon",
                Some(Frames::ZERO),
                Some(Frames(44_100)),
            ),
            (
                MediaLocation::local(tree.path().join("14.wav")),
                "Blood In The Wine",
                Some(Frames::ZERO),
                Some(Frames(44_100)),
            ),
        ],
        "a gap at the tail of a file was cut into a row of its own"
    );
    Ok(())
}

#[test]
fn a_play_counted_against_one_cue_row_is_not_counted_against_its_neighbours() -> Result<()> {
    let (_, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let second = &rows[1];

    let counted = library
        .track_played(&second.location, second.span)?
        .expect("the row that was heard");
    assert_eq!(counted.track.plays, 1);
    assert_eq!(counted.track.id, second.id);

    let after = library.tracks(&TrackQuery::default())?;
    assert_eq!(
        after.iter().map(|row| row.plays).collect::<Vec<_>>(),
        vec![0, 1, 0],
        "a play reached a row the listener never heard"
    );
    Ok(())
}

#[test]
fn reading_every_file_again_keeps_each_rows_id_plays_and_favourite() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let second = &rows[1];
    library.track_played(&second.location, second.span)?;
    library.favour(Favoured::Track(second.id), true)?;

    let read = scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;
    assert_eq!(read.updated, 3, "a row was skipped rather than read");

    let after = library.tracks(&TrackQuery::default())?;
    assert_eq!(
        after.iter().map(|row| row.id).collect::<Vec<_>>(),
        rows.iter().map(|row| row.id).collect::<Vec<_>>()
    );
    assert_eq!(
        after.iter().map(|row| row.plays).collect::<Vec<_>>(),
        vec![0, 1, 0]
    );
    assert!(after[1].favourite.is_some());
    Ok(())
}

#[test]
fn removing_a_track_from_the_library_keeps_its_file_and_survives_a_rescan() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let removed = &rows[1];
    library.favour(Favoured::Track(removed.id), true)?;

    assert!(library.hide_track(removed.id, true)?);
    assert!(
        !library.hide_track(removed.id, true)?,
        "hiding twice should be harmless"
    );
    assert!(tree.path().join("Meddle.wav").exists());
    assert!(
        library.track(removed.id)?.is_some(),
        "the stored row was deleted"
    );
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), 2);
    assert!(library.favourite_tracks(&TrackQuery::default())?.is_empty());
    assert_eq!(library.measured(&TrackQuery::default())?.rows, 2);

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;
    assert!(tree.path().join("Meddle.wav").exists());
    assert!(library.track(removed.id)?.is_some());
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), 2);
    Ok(())
}

#[test]
fn a_hidden_track_is_found_by_asking_for_it_and_shown_again() -> Result<()> {
    let (_, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let hidden = &rows[1];
    library.hide_track(hidden.id, true)?;

    let asked = library.tracks(&TrackQuery {
        text: Some("is:hidden".to_owned()),
        ..TrackQuery::default()
    })?;
    assert_eq!(
        asked
            .iter()
            .map(|track| (track.id, track.hidden))
            .collect::<Vec<_>>(),
        [(hidden.id, true)]
    );
    assert_eq!(
        library
            .tracks(&TrackQuery {
                text: Some("-is:hidden".to_owned()),
                ..TrackQuery::default()
            })?
            .len(),
        2
    );

    assert!(library.hide_track(hidden.id, false)?);
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), 3);
    assert!(
        library
            .tracks(&TrackQuery {
                text: Some("is:hidden".to_owned()),
                ..TrackQuery::default()
            })?
            .is_empty()
    );
    Ok(())
}

#[test]
fn a_cue_rows_neighbours_are_told_apart_by_the_span_rather_than_the_path() -> Result<()> {
    let (_, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;

    for row in &rows {
        let found = library
            .track_at(row.location.as_path().expect("a local row"), row.span)?
            .expect("a row read back by path and span");
        assert_eq!(
            found.id, row.id,
            "a row was read back as one of its siblings"
        );
        assert_eq!(found.title, row.title);
    }
    Ok(())
}

#[test]
fn taking_the_sheet_away_leaves_the_file_as_the_one_track_it_is() -> Result<()> {
    let (tree, library) = scanned_sheet();
    fs::remove_file(tree.path().join("Meddle.cue")).expect("the sheet goes away");

    scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.len(),
        1,
        "the cut rows outlived the sheet that cut them"
    );
    assert_eq!(rows[0].span, None, "the whole file still reads as a span");
    Ok(())
}

fn aged(path: &Path, by: Duration) {
    let held = fs::metadata(path).expect("the file is there");
    let when = held.modified().expect("an mtime the platform keeps") - by;
    fs::File::options()
        .write(true)
        .open(path)
        .expect("the file is writable")
        .set_modified(when)
        .expect("the mtime moves");
}

#[test]
fn a_sheet_older_than_the_file_it_cut_is_still_missed_once_it_has_gone() -> Result<()> {
    let tree = Tree::new();
    tree.write("Meddle.wav", &Wav::new().frames(44_100).build());
    let sheet = tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());
    aged(&sheet, A_DAY);

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        library.tracks(&TrackQuery::default())?.len(),
        3,
        "the sheet did not cut the file it names"
    );

    fs::remove_file(&sheet).expect("the sheet goes away");
    let stats = scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.len(),
        1,
        "a sheet older than the file it cut left its rows behind when it went"
    );
    assert_eq!(rows[0].span, None, "the whole file still reads as a span");
    assert_eq!(stats.removed, 2, "the rows the sheet cut were not pruned");
    Ok(())
}

#[test]
fn a_sheet_rewritten_under_a_file_that_has_not_moved_cuts_it_again() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let titles = |library: &Library| -> Result<Vec<String>> {
        Ok(library
            .tracks(&TrackQuery::default())?
            .into_iter()
            .map(|row| row.title)
            .collect())
    };
    assert_eq!(titles(&library)?.len(), 3);

    tree.write(
        "Meddle.cue",
        MEDDLE_SHEET.replace("Echoes", "Seamus").as_bytes(),
    );
    scan(&library, &options(&tree))?;

    assert!(
        titles(&library)?.contains(&"Seamus".to_owned()),
        "a rewritten sheet did not cut the file again: {:?}",
        titles(&library)?
    );
    Ok(())
}

#[test]
fn a_rescan_of_a_cut_file_changes_nothing() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let before = library.tracks(&TrackQuery::default())?;

    let stats = scan(&library, &options(&tree))?;
    let after = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        stats.added, 0,
        "a rescan added a row that was already there"
    );
    assert_eq!(
        stats.updated, 0,
        "a rescan rewrote a row that had not moved"
    );
    assert_eq!(
        before.iter().map(|row| row.id).collect::<Vec<_>>(),
        after.iter().map(|row| row.id).collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn a_cue_row_put_in_a_playlist_is_the_cut_it_was_rather_than_the_file_it_came_out_of() -> Result<()>
{
    let (_tree, library) = scanned_sheet();
    let cut = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|track| track.title == "Echoes")
        .expect("the sheet's third track was scanned");
    let span = cut.span.expect("a cue row is a cut of its file");

    let id = library.create_playlist("Meddle")?;
    library.add_to_playlist(id, &[Cut::of(&cut)])?;

    let entries = library.playlist_entries(id, None)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].span(),
        Some(span),
        "the row lost the span that told it from its neighbours"
    );
    assert_eq!(
        entries[0].track.as_ref().map(|track| track.title.as_str()),
        Some("Echoes"),
        "the row joined to whichever of the file's tracks starts first"
    );
    assert_eq!(library.playlist_cuts(id)?, vec![Cut::of(&cut)]);
    Ok(())
}

#[test]
fn the_rows_a_sheet_cut_are_a_playlist_of_their_own_lengths() -> Result<()> {
    let (_tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let cuts: Vec<Cut> = rows.iter().map(Cut::of).collect();

    let id = library.create_playlist("Meddle")?;
    library.add_to_playlist(id, &cuts)?;

    let entries = library.playlist_entries(id, None)?;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.track.as_ref().map(|track| track.title.as_str()))
            .collect::<Vec<_>>(),
        rows.iter()
            .map(|track| Some(track.title.as_str()))
            .collect::<Vec<_>>(),
        "three rows of one file drew as three of the same row"
    );

    let whole: Duration = rows
        .iter()
        .filter_map(|track| Some(track.duration?.to_duration(track.spec.rate)))
        .sum();
    let found = library.playlist(id)?.expect("the playlist is there");
    let counted = found
        .duration
        .expect("a playlist of scanned rows has a length");

    assert_eq!(found.entries, 3);
    assert!(
        counted.abs_diff(whole) < Duration::from_millis(1),
        "the length was counted off one row three times over: {counted:?} against {whole:?}"
    );
    Ok(())
}

#[test]
fn a_search_narrowing_a_playlist_of_one_files_cuts_matches_the_cut_it_names_alone() -> Result<()> {
    let (_tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let cuts: Vec<Cut> = rows.iter().map(Cut::of).collect();
    let id = library.create_playlist("Meddle")?;
    library.add_to_playlist(id, &cuts)?;

    let echoes = rows
        .iter()
        .find(|track| track.title == "Echoes")
        .expect("a row titled Echoes");
    let shown = library.playlist_entries(id, Some("echoes"))?;
    assert_eq!(
        shown.len(),
        1,
        "a search for one cut matched every cut of the file"
    );
    assert_eq!(shown[0].cut, Cut::of(echoes));

    assert_eq!(library.remove_matching(id, "echoes")?, 1);
    let left = library.playlist_entries(id, None)?;
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|entry| entry.cut != Cut::of(echoes)));
    Ok(())
}

#[test]
fn two_rows_one_sheet_cut_out_of_a_file_are_not_doubles_of_each_other() -> Result<()> {
    let (_tree, library) = scanned_sheet();
    let cuts: Vec<Cut> = library
        .tracks(&TrackQuery::default())?
        .iter()
        .map(Cut::of)
        .collect();

    let id = library.create_playlist("Meddle")?;
    library.add_to_playlist(id, &cuts)?;

    assert_eq!(
        library.fold_doubles(id)?,
        0,
        "the cuts of one file were folded onto each other"
    );
    assert_eq!(library.playlist_entries(id, None)?.len(), 3);

    library.add_to_playlist(id, &cuts[..1])?;
    assert_eq!(
        library.fold_doubles(id)?,
        1,
        "the same cut added twice was not folded"
    );
    Ok(())
}

const EMBEDDED_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Pillow of Winds"
    INDEX 01 00:00:50
  TRACK 03 AUDIO
    TITLE "Echoes"
    INDEX 01 00:01:25
"#;

const SHEET_BESIDE_IT: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Fearless"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "San Tropez"
    INDEX 01 00:01:00
"#;

fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn flac_embedding(tree: &Tree, name: &str, sheet: &str) -> Option<PathBuf> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name} carrying a cue sheet");
        return None;
    }

    let held = Tree::new();
    let source = held.write("tone.wav", &Wav::new().frames(88_200).build());
    let target = tree.path().join(name);
    let encoded = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .args(["-c:a", "flac", "-metadata"])
        .arg(format!("CUESHEET={sheet}"))
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) || !target.exists() {
        eprintln!("skipped: ffmpeg would not encode {name}");
        return None;
    }
    Some(target)
}

#[test]
fn a_file_carrying_its_own_cue_sheet_is_scanned_as_the_tracks_that_sheet_names() -> Result<()> {
    let tree = Tree::new();
    let Some(path) = flac_embedding(&tree, "Meddle.flac", EMBEDDED_SHEET) else {
        return Ok(());
    };

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(stats.discovered, 3, "one file became three rows uncounted");
    assert_eq!(stats.added, 3);
    assert_eq!(rows.len(), 3, "an embedded sheet left the file whole");
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["One of These Days", "A Pillow of Winds", "Echoes"]
    );

    let held = MediaLocation::local(&path);
    for row in &rows {
        assert_eq!(row.location, held, "every row names the one file");
        assert_eq!(row.artist.as_deref(), Some("Pink Floyd"));
    }
    assert_eq!(
        rows.iter().map(|row| row.span).collect::<Vec<_>>(),
        vec![
            Some(FrameSpan::between(Frames(0), Frames(29_400))),
            Some(FrameSpan::between(Frames(29_400), Frames(58_800))),
            Some(FrameSpan::between(Frames(58_800), Frames(88_200))),
        ],
        "the rows do not start where the embedded sheet says"
    );
    Ok(())
}

#[test]
fn a_sheet_beside_the_file_still_cuts_it_where_the_file_embeds_one_too() -> Result<()> {
    let tree = Tree::new();
    if flac_embedding(&tree, "Meddle.flac", EMBEDDED_SHEET).is_none() {
        return Ok(());
    }
    tree.write("Meddle.cue", SHEET_BESIDE_IT.as_bytes());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Fearless", "San Tropez"],
        "the sheet the file embeds outranked the one beside it"
    );
    Ok(())
}

#[test]
fn a_rescan_of_a_file_that_embeds_its_own_sheet_changes_nothing() -> Result<()> {
    let tree = Tree::new();
    if flac_embedding(&tree, "Meddle.flac", EMBEDDED_SHEET).is_none() {
        return Ok(());
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let before = library.tracks(&TrackQuery::default())?;

    let stats = scan(&library, &options(&tree))?;
    let after = library.tracks(&TrackQuery::default())?;

    assert_eq!(stats.discovered, 3);
    assert_eq!(
        stats.added, 0,
        "a rescan added a row that was already there"
    );
    assert_eq!(
        stats.updated, 0,
        "a rescan rewrote a row that had not moved"
    );
    assert_eq!(
        before.iter().map(|row| row.id).collect::<Vec<_>>(),
        after.iter().map(|row| row.id).collect::<Vec<_>>()
    );
    Ok(())
}

fn mbid(text: &str) -> Mbid {
    Mbid::new(text).expect("a well-formed mbid")
}

fn only_album(library: &Library) -> Result<Album> {
    let mut albums = library.albums(&AlbumQuery::default())?;
    assert_eq!(albums.len(), 1, "one album was expected, got {albums:?}");
    Ok(albums.remove(0))
}

fn artist_named(library: &Library, name: &str) -> Result<Artist> {
    library
        .artists(&ArtistQuery::default())?
        .into_iter()
        .find(|artist| artist.name == name)
        .ok_or_else(|| panic!("no artist named {name}"))
}

fn release_row(position: u32, title: &str, links: Vec<Link>) -> ReleaseTrack {
    ReleaseTrack {
        position,
        number: position.to_string(),
        title: title.to_owned(),
        artist: None,
        recording: None,
        track: None,
        length: Some(Duration::from_millis(1_000 * u64::from(position))),
        isrc: None,
        links,
    }
}

fn orbits(tracks: Vec<ReleaseTrack>, links: Vec<Link>) -> Release {
    Release {
        id: mbid(RELEASE),
        group: Some(mbid(RELEASE_GROUP)),
        title: "Orbits".to_owned(),
        credit: Vec::new(),
        date: Some("1998-03-02".to_owned()),
        country: Some("GB".to_owned()),
        label: Some("Harvest".to_owned()),
        catalog_number: Some("SHVL 795".to_owned()),
        barcode: Some("724382920021".to_owned()),
        kind: Some("Album".to_owned()),
        disambiguation: Some("2011 remaster".to_owned()),
        has_front_cover: true,
        links,
        media: vec![Medium {
            position: 1,
            format: Some("CD".to_owned()),
            title: None,
            tracks,
        }],
    }
}

const ORBITERS_PICTURE: &str = "https://commons.wikimedia.org/wiki/File:Orbiters1973.jpg";

fn orbiters() -> ArtistProfile {
    ArtistProfile {
        mbid: mbid(ORBITERS),
        name: "The Orbiters".to_owned(),
        sort_name: Some("Orbiters, The".to_owned()),
        kind: Some("Group".to_owned()),
        gender: None,
        country: Some("GB".to_owned()),
        area: Some("London".to_owned()),
        began_in: Some("Cambridge".to_owned()),
        span: LifeSpan {
            begin: Some("1965".to_owned()),
            end: None,
            ended: true,
        },
        disambiguation: Some("the English band".to_owned()),
        aliases: Vec::new(),
        genres: vec![
            Genre {
                name: "progressive rock".to_owned(),
                weight: 12,
            },
            Genre {
                name: "psychedelic rock".to_owned(),
                weight: 7,
            },
        ],
        links: vec![
            Link::new("image", ORBITERS_PICTURE.to_owned()),
            Link::new(
                "streaming",
                "https://open.spotify.com/artist/0k17h".to_owned(),
            ),
            Link::new(
                "official homepage",
                "https://www.theorbiters.example/".to_owned(),
            ),
        ],
    }
}

fn orbits_tree() -> Tree {
    let tree = Tree::new();
    for (file, title, number) in [
        ("1.wav", "One of These Days", Some("1")),
        ("2.wav", "A Pillow of Winds", Some("2")),
        ("3.wav", "Fearless!", None),
    ] {
        let mut track = Wav::new()
            .text(TITLE, title)
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits");
        if let Some(number) = number {
            track = track.text(TRACK, number);
        }
        tree.write(file, &track.build());
    }

    tree
}

fn scanned_orbits() -> Result<(Tree, Library)> {
    let tree = orbits_tree();
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

fn scanned_orbits_on_disk() -> Result<(Tree, Library, PathBuf)> {
    let tree = orbits_tree();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    Ok((tree, library, database))
}

#[derive(Debug, PartialEq, Eq)]
struct Stored {
    title: String,
    artist: Option<String>,
    tagged_title: Option<String>,
    tagged_artist: Option<String>,
    isrc: Option<String>,
    release_title: Option<String>,
    asked: Option<i64>,
    answered: Option<i64>,
}

fn beside(database: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(database).expect("the catalog file opens for a second reader")
}

fn settled(file: &Path) -> String {
    file.canonicalize()
        .expect("the file the scan walked is still there")
        .to_str()
        .expect("a path the catalog can hold")
        .to_owned()
}

fn stored(database: &Path, file: &Path) -> Stored {
    beside(database)
        .query_row(
            "SELECT title, artist, tagged_title, tagged_artist, isrc, release_title,
                    asked, answered
               FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| {
                Ok(Stored {
                    title: row.get(0)?,
                    artist: row.get(1)?,
                    tagged_title: row.get(2)?,
                    tagged_artist: row.get(3)?,
                    isrc: row.get(4)?,
                    release_title: row.get(5)?,
                    asked: row.get(6)?,
                    answered: row.get(7)?,
                })
            },
        )
        .expect("the scan stored the track")
}

fn answer_track(database: &Path, file: &Path, title: &str, artist: &str, release: &str) {
    let written = beside(database)
        .execute(
            "UPDATE tracks
                SET title = ?1, artist = ?2, release_title = ?3, asked = ?4, answered = ?5
              WHERE path = ?6",
            rusqlite::params![title, artist, release, ASKED, ANSWERED, settled(file)],
        )
        .expect("the row the enrichment answers for is writable");
    assert_eq!(written, 1, "one track row was expected to be answered for");
}

#[test]
fn a_scan_writes_the_release_and_artist_ids_the_tags_carry() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "dawn.wav",
        &Wav::new()
            .text(TITLE, "Dawn")
            .text(ARTIST, "Ada")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .described(MUSICBRAINZ_ARTIST, ADA)
            .described(MUSICBRAINZ_ALBUM_ARTIST, &ORBITERS.to_ascii_uppercase())
            .described(MUSICBRAINZ_RELEASE_GROUP, RELEASE_GROUP)
            .described(MUSICBRAINZ_RELEASE_TRACK, RELEASE_TRACK)
            .build(),
    );
    for (file, title, artist, id) in [
        ("noon.wav", "Noon", "Ada", ADA),
        ("dusk.wav", "Dusk", "Ben", "not an id at all"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .described(MUSICBRAINZ_ARTIST, id)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let orbits = albums
        .iter()
        .find(|album| album.title == "Orbits")
        .expect("the release was grouped");
    assert_eq!(orbits.mbid, Some(mbid(RELEASE)));
    assert_eq!(orbits.missing, 0);
    let hours = albums
        .iter()
        .find(|album| album.title == "Hours")
        .expect("the compilation was grouped");
    assert_eq!(hours.mbid, None);

    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ORBITERS)),
        "the album artist's id was not written, or not lowercased"
    );
    assert_eq!(
        artist_named(&library, "Ada")?.mbid,
        Some(mbid(ADA)),
        "a performer's id was not written"
    );
    assert_eq!(
        artist_named(&library, "Ben")?.mbid,
        None,
        "a tag holding no id was stored as one"
    );
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.mbid),
        Some(Some(mbid(RELEASE)))
    );
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.group),
        Some(Some(mbid(RELEASE_GROUP))),
        "the release group the tags carry was dropped on the floor"
    );
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.group),
        Some(None),
        "an album whose tags name no release group was given one"
    );
    Ok(())
}

#[test]
fn a_scan_stores_what_the_tags_declare_about_the_release() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "1/12")
            .text(LABEL, "  Harvest ")
            .described(CATALOG_NUMBER, "   ")
            .described(MUSICBRAINZ_ALBUM_ARTIST, ORBITERS)
            .build(),
    );
    tree.write(
        "2.wav",
        &Wav::new()
            .text(TITLE, "A Pillow of Winds")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2/12")
            .described(BARCODE, "724382920021")
            .described(CATALOG_NUMBER, "SHVL 795")
            .described(MUSICBRAINZ_ALBUM_ARTIST, ORBITERS)
            .build(),
    );
    tree.write(
        "3.wav",
        &Wav::new()
            .text(TITLE, "Dawn")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Hours")
            .text(COMPILATION, "1")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let orbits = albums
        .iter()
        .find(|album| album.title == "Orbits")
        .expect("the release was grouped");
    let held = library.release_of(orbits.id)?.expect("the album is known");
    assert_eq!(held.barcode.as_deref(), Some("724382920021"));
    assert_eq!(
        held.catalog_number.as_deref(),
        Some("SHVL 795"),
        "a blank catalogue number was stored, or the one a later file carried was not"
    );
    assert_eq!(
        held.label.as_deref(),
        Some("Harvest"),
        "the label was not trimmed"
    );

    let asked = library
        .album_to_ask(orbits.id)?
        .expect("the album is known");
    assert_eq!(asked.track_count, 2);
    assert_eq!(asked.tagged_tracks, Some(12));
    assert_eq!(asked.catalog_number.as_deref(), Some("SHVL 795"));
    assert_eq!(asked.barcode.as_deref(), Some("724382920021"));
    assert_eq!(asked.owner_mbid, Some(mbid(ORBITERS)));

    let hours = albums
        .iter()
        .find(|album| album.title == "Hours")
        .expect("the compilation was grouped");
    let asked = library.album_to_ask(hours.id)?.expect("the album is known");
    assert_eq!(asked.tagged_tracks, None);
    assert_eq!(asked.catalog_number, None);
    assert_eq!(asked.owner_mbid, None);
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.label),
        Some(None)
    );
    Ok(())
}

#[test]
fn a_rescan_keeps_a_barcode_the_file_no_longer_carries() -> Result<()> {
    let tree = Tree::new();
    let tagged = |barcode: Option<&str>| {
        let mut track = Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits");
        if let Some(barcode) = barcode {
            track = track.described(BARCODE, barcode);
        }
        track.build()
    };
    tree.write("1.wav", &tagged(Some("724382920021")));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    let barcode = |library: &Library| -> Result<Option<String>> {
        Ok(library
            .release_of(album.id)?
            .expect("the album is known")
            .barcode)
    };
    assert_eq!(barcode(&library)?.as_deref(), Some("724382920021"));

    tree.write("1.wav", &tagged(None));
    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(only_album(&library)?.id, album.id);
    assert_eq!(
        barcode(&library)?.as_deref(),
        Some("724382920021"),
        "a barcode the file stopped declaring was cleared"
    );

    tree.write("1.wav", &tagged(Some("5099902894027")));
    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        barcode(&library)?.as_deref(),
        Some("5099902894027"),
        "a barcode the file was retagged with was not taken"
    );
    Ok(())
}

#[test]
fn an_artist_is_found_by_the_fold_of_a_credit_name_however_it_is_spelled() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "blood.wav",
        &Wav::new()
            .text(TITLE, "Silver for Monsters")
            .text(ARTIST, "Marcin Przybyłowicz")
            .text(ALBUM, "Blood and Wine")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let billed = artist_named(&library, "Marcin Przybyłowicz")?;
    assert_eq!(
        library.artist_named("Marcin Przybyłowicz")?,
        Some(billed.id),
        "the marked spelling did not find the artist it is billed under"
    );
    assert_eq!(
        library.artist_named("Marcin Przybylowicz")?,
        Some(billed.id),
        "the stripped spelling did not fold onto the artist it names"
    );
    assert_eq!(
        library.artist_named("MARCIN PRZYBYLOWICZ")?,
        Some(billed.id),
        "the fold is not the one the key was written with"
    );
    assert_eq!(library.artist_named("Marcin Przybysz")?, None);
    Ok(())
}

#[test]
fn a_rescan_leaves_every_enrichment_column_where_it_stood() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let album = only_album(&library)?;
    assert!(
        library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id),
        "an album nothing has asked about was not offered to be asked"
    );

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(2, "A Pillow of Winds", Vec::new()),
                release_row(3, "Fearless", Vec::new()),
            ],
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;
    assert_eq!(library.rematch(album.id)?, 3);
    let art = png(256);
    assert!(library.land_archive_cover(
        album.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: art.clone()
        }
    )?);
    assert!(
        !library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id && !asked.rematch_only),
        "an album answered today was offered to be asked again"
    );
    assert!(
        !library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id),
        "an album whose every row is paired was offered for a rematch that could move nothing"
    );
    assert!(
        library
            .albums_to_ask(WAITED, true)?
            .iter()
            .any(|asked| asked.id == album.id)
    );

    let before = library.release_of(album.id)?.expect("the album is known");
    let rows_before = library.release_tracks(album.id)?;
    assert_eq!(before.cover_source, CoverSource::Archive);
    assert_eq!(before.links.len(), 1);
    assert!(before.asked.is_some() && before.answered.is_some());

    let pillow = tree.path().join("2.wav");
    answer_track(
        &database,
        &pillow,
        "A Pillow of Winds (2011 Remaster)",
        "The Orbiters",
        "Orbits",
    );

    tree.write(
        "2.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, "A Pillow of Winds")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2")
            .build(),
    );
    let stats = scan(&library, &options(&tree))?;
    assert_eq!(stats.updated, 1);

    let after = only_album(&library)?;
    assert_eq!(after.id, album.id);
    assert_eq!(after.mbid, Some(mbid(RELEASE)));
    assert_eq!(after.year, Some(1998));
    assert_eq!(library.release_of(album.id)?.as_ref(), Some(&before));
    assert_eq!(library.release_tracks(album.id)?, rows_before);
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    assert_eq!(
        stored(&database, &pillow),
        Stored {
            title: "A Pillow of Winds (2011 Remaster)".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            tagged_title: Some("A Pillow of Winds".to_owned()),
            tagged_artist: Some("The Orbiters".to_owned()),
            isrc: None,
            release_title: Some("Orbits".to_owned()),
            asked: Some(ASKED),
            answered: Some(ANSWERED),
        }
    );
    Ok(())
}

#[test]
fn a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged() -> Result<()> {
    let tree = Tree::new();
    let tagged = Wav::new()
        .text(TITLE, "one of these days")
        .text(ARTIST, "the orbiters")
        .build();
    let file = tree.write("1.wav", &tagged);
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    answer_track(
        &database,
        &file,
        "One of These Days",
        "The Orbiters",
        "Meddle",
    );

    tree.write("1.wav", &tagged);
    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        stored(&database, &file),
        Stored {
            title: "One of These Days".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            tagged_title: Some("one of these days".to_owned()),
            tagged_artist: Some("the orbiters".to_owned()),
            isrc: None,
            release_title: Some("Meddle".to_owned()),
            asked: Some(ASKED),
            answered: Some(ANSWERED),
        }
    );

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days (Edit)")
            .text(ARTIST, "the orbiters")
            .build(),
    );
    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        stored(&database, &file),
        Stored {
            title: "One of These Days (Edit)".to_owned(),
            artist: Some("the orbiters".to_owned()),
            tagged_title: Some("One of These Days (Edit)".to_owned()),
            tagged_artist: Some("the orbiters".to_owned()),
            isrc: None,
            release_title: Some("Meddle".to_owned()),
            asked: Some(ASKED),
            answered: None,
        }
    );
    Ok(())
}

#[test]
fn an_isrc_tag_is_stored_in_the_one_spelling_a_recording_code_has() -> Result<()> {
    let tree = Tree::new();
    let named = tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, "gb-aye-71-00195")
            .build(),
    );
    let bare = tree.write("2.wav", &Wav::new().build());
    let nonsense = tree.write("3.wav", &Wav::new().text(ISRC, "not a code").build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let held = stored(&database, &named);
    assert_eq!(held.isrc.as_deref(), Some("GBAYE7100195"));
    assert_eq!(
        stored(&database, &nonsense).isrc,
        None,
        "a tag that is no recording code was stored as though it were one"
    );
    assert_eq!(held.tagged_title.as_deref(), Some("One of These Days"));
    assert_eq!(held.tagged_artist.as_deref(), Some("The Orbiters"));

    let untagged = stored(&database, &bare);
    assert_eq!(untagged.isrc, None);
    assert_eq!(untagged.title, "2");
    assert_eq!(
        untagged.tagged_title, None,
        "a file naming no title was given one by the stem fallback"
    );
    assert_eq!(untagged.tagged_artist, None);
    Ok(())
}

#[test]
fn a_stem_is_never_read_over_a_tag_the_file_already_carried() -> Result<()> {
    let tree = Tree::new();
    let tagged = tree.write(
        "03 - Miles Davis - So What.wav",
        &Wav::new()
            .text(TITLE, "Blue in Green")
            .text(ARTIST, "Bill Evans")
            .text(TRACK, "9")
            .build(),
    );
    let bare = tree.write("01 - Ada - Zenith.wav", &Wav::new().build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let held = stored(&database, &tagged);
    assert_eq!(held.tagged_title.as_deref(), Some("Blue in Green"));
    assert_eq!(held.tagged_artist.as_deref(), Some("Bill Evans"));

    let named = stored(&database, &bare);
    assert_eq!(named.title, "Zenith");
    assert_eq!(
        named.tagged_title.as_deref(),
        Some("Zenith"),
        "a name read out of the stem should be what the file says it is"
    );
    assert_eq!(named.tagged_artist.as_deref(), Some("Ada"));

    let listed = all(&library)?;
    let numbered = |title: &str| {
        listed
            .iter()
            .find(|track| track.title == title)
            .and_then(|track| track.track_number)
    };
    assert_eq!(
        numbered("Blue in Green"),
        Some(9),
        "the number the stem carries was read over the one the file tagged"
    );
    assert_eq!(numbered("Zenith"), Some(1));
    Ok(())
}

#[test]
fn an_artist_is_taken_from_a_stem_only_where_the_title_it_parsed_is_the_title_tagged() -> Result<()>
{
    let tree = Tree::new();
    let agreeing = tree.write(
        "03 - Miles Davis - So What.wav",
        &Wav::new().text(TITLE, "So What").build(),
    );
    let disagreeing = tree.write(
        "So What (Remastered 2011) - Live Version.wav",
        &Wav::new().text(TITLE, "So What").build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let taken = stored(&database, &agreeing);
    assert_eq!(taken.tagged_title.as_deref(), Some("So What"));
    assert_eq!(taken.tagged_artist.as_deref(), Some("Miles Davis"));

    let refused = stored(&database, &disagreeing);
    assert_eq!(refused.tagged_title.as_deref(), Some("So What"));
    assert_eq!(
        refused.tagged_artist, None,
        "a stem whose title is not the one tagged gave up its artist"
    );
    Ok(())
}

#[test]
fn a_file_cover_found_on_a_rescan_replaces_one_the_archive_gave() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    assert!(library.land_archive_cover(
        album.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: png(128)
        }
    )?);
    assert!(
        !library.land_archive_cover(
            album.id,
            &CoverArt {
                format: ImageFormat::Png,
                bytes: png(64)
            }
        )?,
        "an archive cover was laid over the one already held"
    );

    let art = png(512);
    tree.write(
        "3.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, "Fearless!")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .picture(&art)
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.release_of(album.id)?.map(|held| held.cover_source),
        Some(CoverSource::File)
    );
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    Ok(())
}

#[test]
fn release_rows_are_matched_by_recording_id_then_position_then_folded_title_and_the_rest_are_missing()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(2, "A Pillow of Winds", Vec::new()),
                release_row(3, "Fearless", Vec::new()),
                release_row(4, "San Tropez", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;
    assert_eq!(library.rematch(album.id)?, 3);

    let by_title: Vec<(String, Option<String>)> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| {
            let matched = row
                .track
                .map(|id| library.track(id))
                .transpose()?
                .flatten()
                .map(|track| track.title);
            Ok((row.title, matched))
        })
        .collect::<Result<_>>()?;
    assert_eq!(
        by_title,
        vec![
            (
                "One of These Days".to_owned(),
                Some("One of These Days".to_owned())
            ),
            (
                "A Pillow of Winds".to_owned(),
                Some("A Pillow of Winds".to_owned())
            ),
            ("Fearless".to_owned(), Some("Fearless!".to_owned())),
            ("San Tropez".to_owned(), None),
        ]
    );
    assert_eq!(only_album(&library)?.missing, 1);
    Ok(())
}

#[test]
fn every_link_a_release_and_its_recordings_carry_is_kept_with_its_provider() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![release_row(
                1,
                "One of These Days",
                vec![
                    Link::new(
                        "streaming",
                        "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
                    ),
                    Link::new(
                        "lyrics",
                        "https://genius.com/Orbiters-one-lyrics".to_owned(),
                    ),
                ],
            )],
            vec![
                Link::new("streaming", "https://tidal.com/album/55391740".to_owned()),
                Link::new(
                    "purchase for download",
                    "https://www.7digital.com/artist/orbiters/release/orbits".to_owned(),
                ),
                Link::new("discogs", "https://www.discogs.com/release/1234".to_owned()),
            ],
        ),
    )?;

    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.mbid, Some(mbid(RELEASE)));
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert_eq!(release.barcode.as_deref(), Some("724382920021"));
    let providers: Vec<(Relation, Service)> = release
        .links
        .iter()
        .map(|link| (link.relation, link.service))
        .collect();
    assert_eq!(
        providers,
        vec![
            (Relation::Streaming, Service::Tidal),
            (Relation::PurchaseForDownload, Service::SevenDigital),
            (Relation::Discogs, Service::Discogs),
        ]
    );

    let rows = library.release_tracks(album.id)?;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.length, Some(Duration::from_secs(1)));
    assert_eq!(
        row.links,
        vec![
            Link {
                relation: Relation::Lyrics,
                service: Service::Other,
                url: "https://genius.com/Orbiters-one-lyrics".to_owned(),
            },
            Link {
                relation: Relation::Streaming,
                service: Service::Spotify,
                url: "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
            },
        ]
    );
    Ok(())
}

#[test]
fn an_artist_profile_genres_links_and_portrait_are_written_and_read_back_whole() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = artist_named(&library, "The Orbiters")?;
    assert!(library.artists_to_ask(WAITED, false)?.contains(&artist.id));
    assert_eq!(
        library.artist_detail(artist.id)?.map(|held| held.mbid),
        Some(None)
    );

    let profile = orbiters();
    assert_eq!(
        resonate_library::portrait_urls(&profile.links).collect::<Vec<&str>>(),
        vec![ORBITERS_PICTURE]
    );
    library.land_artist(artist.id, &profile)?;
    let portrait = png(96);
    assert!(library.land_portrait(
        artist.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: portrait.clone()
        }
    )?);
    assert!(!library.land_portrait(
        artist.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: png(32)
        }
    )?);

    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");
    assert_eq!(detail.mbid, Some(profile.mbid.clone()));
    assert_eq!(detail.sort_name, profile.sort_name);
    assert_eq!(detail.kind, profile.kind);
    assert_eq!(detail.country, profile.country);
    assert_eq!(detail.area, profile.area);
    assert_eq!(detail.began_in, profile.began_in);
    assert_eq!(detail.span, profile.span);
    assert_eq!(detail.disambiguation, profile.disambiguation);
    assert_eq!(detail.genres, profile.genres);
    assert!(detail.asked.is_some() && detail.answered.is_some());
    assert_eq!(
        detail
            .links
            .iter()
            .map(|link| link.service)
            .collect::<Vec<_>>(),
        vec![Service::WikimediaCommons, Service::Spotify, Service::Other]
    );

    let again = artist_named(&library, "The Orbiters")?;
    assert_eq!(again.mbid, Some(mbid(ORBITERS)));
    assert!(again.has_portrait);
    assert_eq!(
        library.portrait(artist.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: portrait,
        })
    );
    assert!(!library.artists_to_ask(WAITED, false)?.contains(&artist.id));

    library.write_artist_mbid(artist.id, &mbid(ADA))?;
    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ADA))
    );
    Ok(())
}

#[test]
fn kept_lyrics_are_keyed_by_path_and_span_and_a_miss_is_remembered() -> Result<()> {
    let library = Library::open_in_memory()?;
    let whole = MediaLocation::local("/music/Orbits/Echoes.flac");
    let cut = Some(FrameSpan::starting(Frames(44_100)));

    assert_eq!(library.kept_lyrics(&whole, None)?, None);
    library.keep_lyrics(&whole, None, Some("[00:01.00]Overhead the albatross"), true)?;
    library.keep_lyrics(&whole, cut, None, false)?;

    let kept = library
        .kept_lyrics(&whole, None)?
        .expect("the lyrics were kept");
    assert_eq!(
        kept.text.as_deref(),
        Some("[00:01.00]Overhead the albatross")
    );
    assert!(kept.synced);
    let miss = library
        .kept_lyrics(&whole, cut)?
        .expect("the miss was kept");
    assert_eq!(miss.text, None);
    assert!(!miss.synced);

    library.keep_lyrics(&whole, None, Some("Overhead the albatross"), false)?;
    let replaced = library
        .kept_lyrics(&whole, None)?
        .expect("the lyrics were kept");
    assert_eq!(replaced.text.as_deref(), Some("Overhead the albatross"));
    assert!(!replaced.synced);
    assert!(replaced.taken >= kept.taken);

    let elsewhere = MediaLocation::new(SourceId::new("tidal")?, "55391743");
    assert!(matches!(
        library.kept_lyrics(&elsewhere, None),
        Err(Error::NotALocalFile { .. })
    ));
    assert!(matches!(
        library.keep_lyrics(&elsewhere, None, None, false),
        Err(Error::NotALocalFile { .. })
    ));
    Ok(())
}

#[test]
fn a_want_is_one_per_release_row_and_carries_its_links() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "One of These Days", Vec::new()),
                release_row(
                    4,
                    "San Tropez",
                    vec![Link::new(
                        "streaming",
                        "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
                    )],
                ),
            ],
            Vec::new(),
        ),
    )?;
    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");

    let want = library.want(missing.id)?;
    assert_eq!(library.want(missing.id)?, want);
    assert!(matches!(
        library.want(ReleaseTrackId::MAX),
        Err(Error::UnknownReleaseTrack(_))
    ));

    let wants = library.wants()?;
    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].release_track, missing.id);
    assert_eq!(wants[0].album, album.id);
    assert_eq!(wants[0].album_title, "Orbits");
    assert_eq!(wants[0].title, "San Tropez");
    assert_eq!(wants[0].tried, None);
    assert_eq!(
        wants[0]
            .links
            .iter()
            .map(|link| link.service)
            .collect::<Vec<_>>(),
        vec![Service::Spotify]
    );

    assert!(library.unwant(want)?);
    assert!(!library.unwant(want)?);
    assert!(library.wants()?.is_empty());

    let again = library.want(missing.id)?;
    assert_eq!(library.wants()?.len(), 1);
    assert_eq!(library.wants()?[0].id, again);
    assert!(library.remove_root(tree.path())?);
    assert!(
        library.wants()?.is_empty(),
        "a want survived the root its album was scanned under"
    );
    Ok(())
}

#[test]
fn a_want_follows_the_track_it_was_for_across_a_release_the_reference_re_edited() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let recorded = |position: u32, title: &str, recording: &str| ReleaseTrack {
        recording: Some(mbid(recording)),
        ..release_row(position, title, Vec::new())
    };
    library.land_release(
        album.id,
        &orbits(
            vec![
                recorded(1, "One of These Days", RECORDING),
                recorded(2, "San Tropez", ANOTHER_RECORDING),
            ],
            Vec::new(),
        ),
    )?;
    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    library.want(missing.id)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                recorded(1, "Seamus", RELEASE_GROUP),
                recorded(2, "One of These Days", RECORDING),
                recorded(3, "San Tropez", ANOTHER_RECORDING),
            ],
            Vec::new(),
        ),
    )?;

    let wants = library.wants()?;
    assert_eq!(wants.len(), 1);
    assert_eq!(
        wants[0].title, "San Tropez",
        "the want moved to whatever row now sits where its own used to"
    );
    let moved = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the re-edited release still holds the row");
    assert_eq!(wants[0].release_track, moved.id);
    assert_eq!(moved.position, 3);
    Ok(())
}

#[test]
fn forgetting_a_root_takes_the_release_rows_under_its_albums_with_it() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![release_row(1, "One of These Days", Vec::new())],
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;
    assert_eq!(library.rematch(album.id)?, 1);
    assert_eq!(library.release_tracks(album.id)?.len(), 1);

    assert!(library.remove_root(tree.path())?);
    assert_eq!(library.release_of(album.id)?, None);
    assert!(library.release_tracks(album.id)?.is_empty());
    assert!(library.albums(&AlbumQuery::default())?.is_empty());
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Called {
    Release(Mbid),
    FindRelease(ReleaseAsked),
    Recording(Mbid),
    Isrc(Isrc),
    FindRecording(RecordingAsked),
    FindSongs(String),
    ReleaseGroup(Mbid),
    FindReleaseGroup(GroupAsked),
    Artist(Mbid),
    FindArtist(String),
    ReleaseGroupsOf(Mbid),
    Cover(Mbid, Option<Mbid>),
    GroupCover(Mbid),
    Portrait(String),
}

impl Called {
    fn op(&self) -> LookupOp {
        match self {
            Self::Release(_) => LookupOp::Release,
            Self::FindRelease(_) => LookupOp::FindRelease,
            Self::Recording(_) => LookupOp::Recording,
            Self::Isrc(_) => LookupOp::Isrc,
            Self::FindRecording(_) | Self::FindSongs(_) => LookupOp::FindRecording,
            Self::ReleaseGroup(_) => LookupOp::ReleaseGroup,
            Self::FindReleaseGroup(_) => LookupOp::FindReleaseGroup,
            Self::Artist(_) => LookupOp::Artist,
            Self::FindArtist(_) => LookupOp::FindArtist,
            Self::ReleaseGroupsOf(_) => LookupOp::ReleaseGroupsOfArtist,
            Self::Cover(..) => LookupOp::Cover,
            Self::GroupCover(_) => LookupOp::Cover,
            Self::Portrait(_) => LookupOp::Portrait,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Refused,
    Unreachable,
}

#[derive(Default)]
struct Canned {
    releases: Vec<Release>,
    found_releases: Vec<ReleaseMatch>,
    found_releases_in_words: Vec<ReleaseMatch>,
    recordings: Vec<Recording>,
    found_recordings: Vec<RecordingMatch>,
    found_recordings_in_words: Vec<RecordingMatch>,
    found_songs: Vec<RecordingMatch>,
    isrcs: Vec<(Isrc, Recording)>,
    groups: Vec<ReleaseGroup>,
    found_groups: Vec<GroupMatch>,
    found_groups_in_words: Vec<GroupMatch>,
    artists: Vec<ArtistProfile>,
    found_artists: Vec<ArtistMatch>,
    artist_releases: Vec<(Mbid, Vec<ArtistRelease>)>,
    covers: Vec<(Mbid, CoverArt)>,
    group_covers: Vec<(Mbid, CoverArt)>,
    portraits: Vec<(String, CoverArt)>,
}

struct Gate {
    op: Option<LookupOp>,
    started: Sender<()>,
    go: Receiver<()>,
}

struct Fake {
    source: SourceId,
    canned: Canned,
    faults: Vec<(LookupOp, usize, Fault)>,
    calls: Mutex<Vec<Called>>,
    gate: Mutex<Option<Gate>>,
}

impl Fake {
    fn new(canned: Canned) -> Self {
        Self {
            source: SourceId::new("musicbrainz").expect("a nameable source"),
            canned,
            faults: Vec::new(),
            calls: Mutex::new(Vec::new()),
            gate: Mutex::new(None),
        }
    }

    fn faulting(mut self, op: LookupOp, at: usize, fault: Fault) -> Self {
        self.faults.push((op, at, fault));
        self
    }

    fn gated(self) -> (Self, Receiver<()>, Sender<()>) {
        self.gated_on(None)
    }

    fn gated_on(self, op: Option<LookupOp>) -> (Self, Receiver<()>, Sender<()>) {
        let (started, has_started) = mpsc::channel();
        let (go, may_go) = mpsc::channel();
        *self.gate.lock() = Some(Gate {
            op,
            started,
            go: may_go,
        });
        (self, has_started, go)
    }

    fn calls(&self) -> Vec<Called> {
        self.calls.lock().clone()
    }

    fn called(&self, op: LookupOp) -> usize {
        self.calls
            .lock()
            .iter()
            .filter(|called| called.op() == op)
            .count()
    }

    fn note(&self, called: Called) -> Result<()> {
        let op = called.op();
        let nth = self.called(op);
        self.calls.lock().push(called);

        let gate = {
            let mut held = self.gate.lock();
            match held.as_ref() {
                Some(gate) if gate.op.is_none_or(|wanted| wanted == op) => held.take(),
                Some(_) | None => None,
            }
        };
        if let Some(gate) = gate {
            let _ = gate.started.send(());
            let _ = gate.go.recv();
        }

        match self
            .faults
            .iter()
            .find(|(faulted, at, _)| *faulted == op && *at == nth)
        {
            Some((_, _, Fault::Refused)) => Err(Error::Refused { op, status: 503 }),
            Some((_, _, Fault::Unreachable)) => Err(Error::Unreachable {
                op,
                source: io::Error::from(io::ErrorKind::ConnectionRefused),
            }),
            None => Ok(()),
        }
    }
}

impl Reference for Fake {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, id: &Mbid) -> Result<Option<Release>> {
        self.note(Called::Release(id.clone()))?;
        Ok(self
            .canned
            .releases
            .iter()
            .find(|release| release.id == *id)
            .cloned())
    }

    fn find_release(&self, asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
        self.note(Called::FindRelease(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_releases.clone(),
            Wording::Words => self.canned.found_releases_in_words.clone(),
        })
    }

    fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
        self.note(Called::Recording(id.clone()))?;
        Ok(self
            .canned
            .recordings
            .iter()
            .find(|recording| recording.id == *id)
            .cloned())
    }

    fn recordings_of_isrc(&self, isrc: &Isrc) -> Result<Vec<Recording>> {
        self.note(Called::Isrc(isrc.clone()))?;
        Ok(self
            .canned
            .isrcs
            .iter()
            .filter(|(held, _)| held == isrc)
            .map(|(_, recording)| recording.clone())
            .collect())
    }

    fn find_recording(&self, asked: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
        self.note(Called::FindRecording(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_recordings.clone(),
            Wording::Words => self.canned.found_recordings_in_words.clone(),
        })
    }

    fn find_songs(&self, words: &str) -> Result<Vec<RecordingMatch>> {
        self.note(Called::FindSongs(words.to_owned()))?;
        Ok(self.canned.found_songs.clone())
    }

    fn release_group(&self, id: &Mbid) -> Result<Option<ReleaseGroup>> {
        self.note(Called::ReleaseGroup(id.clone()))?;
        Ok(self
            .canned
            .groups
            .iter()
            .find(|group| group.id == *id)
            .cloned())
    }

    fn find_release_group(&self, asked: &GroupAsked) -> Result<Vec<GroupMatch>> {
        self.note(Called::FindReleaseGroup(asked.clone()))?;
        Ok(match asked.wording {
            Wording::Phrase => self.canned.found_groups.clone(),
            Wording::Words => self.canned.found_groups_in_words.clone(),
        })
    }

    fn artist(&self, id: &Mbid) -> Result<Option<ArtistProfile>> {
        self.note(Called::Artist(id.clone()))?;
        Ok(self
            .canned
            .artists
            .iter()
            .find(|profile| profile.mbid == *id)
            .cloned())
    }

    fn find_artist(&self, name: &str) -> Result<Vec<ArtistMatch>> {
        self.note(Called::FindArtist(name.to_owned()))?;
        Ok(self.canned.found_artists.clone())
    }

    fn release_groups_of(&self, artist: &Mbid) -> Result<Vec<ArtistRelease>> {
        self.note(Called::ReleaseGroupsOf(artist.clone()))?;
        Ok(self
            .canned
            .artist_releases
            .iter()
            .find(|(held, _)| held == artist)
            .map(|(_, releases)| releases.clone())
            .unwrap_or_default())
    }

    fn cover(&self, release: &Mbid, group: Option<&Mbid>) -> Result<Option<CoverArt>> {
        self.note(Called::Cover(release.clone(), group.cloned()))?;
        Ok(self
            .canned
            .covers
            .iter()
            .find(|(held, _)| held == release)
            .map(|(_, art)| art.clone()))
    }

    fn group_cover(&self, group: &Mbid) -> Result<Option<CoverArt>> {
        self.note(Called::GroupCover(group.clone()))?;
        Ok(self
            .canned
            .group_covers
            .iter()
            .find(|(held, _)| held == group)
            .map(|(_, art)| art.clone()))
    }

    fn portrait(&self, links: &[Link]) -> Result<Option<CoverArt>> {
        for url in resonate_library::portrait_urls(links) {
            self.note(Called::Portrait(url.to_owned()))?;
            if let Some(art) = self
                .canned
                .portraits
                .iter()
                .find(|(held, _)| held == url)
                .map(|(_, art)| art.clone())
            {
                return Ok(Some(art));
            }
        }

        Ok(None)
    }
}

#[derive(Clone)]
enum Delivering {
    Nothing,
    File(PathBuf),
    Bytes {
        key: &'static str,
        extension: &'static str,
        bytes: Vec<u8>,
    },
    Endless,
    Stalling,
}

struct Trickle;

struct Stall {
    began: bool,
}

impl io::Read for Stall {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        if !self.began {
            self.began = true;
            let given = into.len().min(64);
            into[..given].fill(0);
            return Ok(given);
        }
        loop {
            thread::park();
        }
    }
}

impl io::Read for Trickle {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        thread::sleep(Duration::from_millis(5));
        let given = into.len().min(64);
        into[..given].fill(0);
        Ok(given)
    }
}

struct Offering {
    source: SourceId,
    delivering: Delivering,
    asked: Mutex<Vec<Identity>>,
}

impl Offering {
    fn new(source: &str, delivering: Delivering) -> Self {
        Self {
            source: SourceId::new(source).expect("a nameable source"),
            delivering,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<Identity> {
        self.asked.lock().clone()
    }

    fn registered(self: &Arc<Self>) -> Arc<Providers> {
        Arc::new(Providers::none().and(Arc::clone(self) as Arc<dyn Provider>))
    }
}

impl Provider for Offering {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn obtain(&self, identity: &Identity) -> ProvidedResult<Obtained> {
        self.asked.lock().push(identity.clone());
        Ok(match self.delivering.clone() {
            Delivering::Nothing => Obtained::Nothing,
            Delivering::File(path) => Obtained::Found(Delivery::File(path)),
            Delivering::Bytes {
                key,
                extension,
                bytes,
            } => Obtained::Found(Delivery::Stream {
                key: key.into(),
                extension: Extension::new(extension)?,
                reader: Box::new(std::io::Cursor::new(bytes)),
            }),
            Delivering::Endless => Obtained::Found(Delivery::Stream {
                key: "endless".into(),
                extension: Extension::new("wav")?,
                reader: Box::new(Trickle),
            }),
            Delivering::Stalling => Obtained::Found(Delivery::Stream {
                key: "stalling".into(),
                extension: Extension::new("wav")?,
                reader: Box::new(Stall { began: false }),
            }),
        })
    }
}

fn png_art(size: usize) -> CoverArt {
    CoverArt {
        format: ImageFormat::Png,
        bytes: png(size),
    }
}

fn enrich(library: &Library, fake: &Arc<Fake>, refresh: bool) -> Result<EnrichSummary> {
    let reference = Arc::clone(fake) as Arc<dyn Reference>;
    let summary = library
        .enrich(
            reference,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                refresh,
                ..EnrichOptions::default()
            },
        )?
        .join()?;
    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

fn orbits_track(title: &str, number: u32) -> Wav {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, "The Orbiters")
        .text(ALBUM, "Orbits")
        .text(TRACK, &number.to_string())
}

const ORBITS_TITLES: [&str; 3] = ["One of These Days", "A Pillow of Winds", "Fearless"];

fn write_orbits(tree: &Tree, tagged: bool) {
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        let mut track = orbits_track(title, number);
        if tagged {
            track = track.described(MUSICBRAINZ_ALBUM, RELEASE);
        }
        tree.write(&format!("Orbits/{number}.wav"), &track.build());
    }
}

fn orbits_rows() -> Vec<ReleaseTrack> {
    ORBITS_TITLES
        .into_iter()
        .enumerate()
        .map(|(index, title)| release_row(index as u32 + 1, title, Vec::new()))
        .collect()
}

fn credited(artist: Option<&str>, id: Option<&str>) -> Vec<Credit> {
    artist
        .into_iter()
        .map(|name| Credit {
            name: name.to_owned(),
            joined_by: String::new(),
            mbid: id.map(mbid),
        })
        .collect()
}

fn orbits_match(score: u8, artist: Option<&str>, tracks: Option<u32>) -> ReleaseMatch {
    ReleaseMatch {
        release: mbid(RELEASE),
        group: None,
        score,
        title: "Orbits".to_owned(),
        credit: credited(artist, None),
        track_count: tracks,
        date: None,
    }
}

fn orbits_group_match(score: u8, artist: Option<&str>) -> GroupMatch {
    GroupMatch {
        group: mbid(RELEASE_GROUP),
        score,
        title: "Orbits".to_owned(),
        credit: credited(artist, None),
    }
}

fn group_release(id: &str, date: &str, tracks: Option<u32>) -> GroupRelease {
    GroupRelease {
        id: mbid(id),
        title: "Orbits".to_owned(),
        date: Some(date.to_owned()),
        country: None,
        track_count: tracks,
    }
}

fn orbits_group(releases: Vec<GroupRelease>, links: Vec<Link>) -> ReleaseGroup {
    ReleaseGroup {
        id: mbid(RELEASE_GROUP),
        title: "Orbits".to_owned(),
        credit: Vec::new(),
        kind: Some("Album".to_owned()),
        first_released: Some("1971-10-30".to_owned()),
        disambiguation: Some("the original run".to_owned()),
        links,
        releases,
    }
}

fn orbits_asked() -> ReleaseAsked {
    ReleaseAsked {
        title: "Orbits".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        barcode: None,
        catalog_number: None,
        wording: Wording::Phrase,
    }
}

fn orbits_asked_in_words() -> ReleaseAsked {
    ReleaseAsked {
        wording: Wording::Words,
        ..orbits_asked()
    }
}

fn orbits_group_asked() -> GroupAsked {
    GroupAsked {
        title: "Orbits".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        year: None,
        wording: Wording::Phrase,
    }
}

fn orbits_group_asked_in_words() -> GroupAsked {
    GroupAsked {
        wording: Wording::Words,
        ..orbits_group_asked()
    }
}

fn hours() -> Release {
    Release {
        id: mbid(HOURS),
        group: None,
        title: "Hours".to_owned(),
        credit: Vec::new(),
        date: None,
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: None,
        disambiguation: None,
        has_front_cover: false,
        links: Vec::new(),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![release_row(1, "Dawn", Vec::new())],
        }],
    }
}

fn write_hours(tree: &Tree, picture: Option<&[u8]>) {
    let mut track = Wav::new()
        .text(TITLE, "Dawn")
        .text(ARTIST, "Ada")
        .text(ALBUM, "Hours")
        .described(MUSICBRAINZ_ALBUM, HOURS)
        .described(MUSICBRAINZ_ARTIST, ADA);
    if let Some(picture) = picture {
        track = track.picture(picture);
    }
    tree.write("Hours/1.wav", &track.build());
}

fn ada() -> ArtistProfile {
    ArtistProfile {
        mbid: mbid(ADA),
        name: "Ada".to_owned(),
        sort_name: None,
        kind: Some("Person".to_owned()),
        gender: None,
        country: None,
        area: None,
        began_in: None,
        span: LifeSpan::default(),
        disambiguation: None,
        aliases: Vec::new(),
        genres: Vec::new(),
        links: Vec::new(),
    }
}

fn album_titled(library: &Library, title: &str) -> Result<Album> {
    library
        .albums(&AlbumQuery::default())?
        .into_iter()
        .find(|album| album.title == title)
        .ok_or_else(|| panic!("no album titled {title}"))
}

#[test]
fn an_album_tagged_with_a_release_id_is_asked_for_by_that_id_and_never_searched() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    assert_eq!(fake.called(LookupOp::FindRelease), 0);
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(fake.calls()[0], Called::Release(mbid(RELEASE)));
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(only_album(&library)?.missing, 0);
    Ok(())
}

#[test]
fn a_release_the_reference_answers_fills_the_album_and_lists_every_track_it_names() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(
            rows,
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        )],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(ReleaseAsked {
                title: "Orbits".to_owned(),
                artist: Some("The Orbiters".to_owned()),
                artist_mbid: None,
                barcode: None,
                catalog_number: None,
                wording: Wording::Phrase,
            }),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(
        fake.called(LookupOp::Cover),
        1,
        "the picture the release names was not fetched beside the pass"
    );
    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 1);
    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert!(release.asked.is_some() && release.answered.is_some());
    assert_eq!(release.links.len(), 1);
    let titles: Vec<String> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| row.title)
        .collect();
    assert_eq!(
        titles,
        vec![
            "One of These Days",
            "A Pillow of Winds",
            "Fearless",
            "San Tropez"
        ]
    );
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(summary.stats.artists, 1);
    assert_eq!(summary.stats.refused, 0);
    Ok(())
}

const DISC_TITLES: [[&str; 2]; 2] = [
    ["One of These Days", "A Pillow of Winds"],
    ["Fearless", "San Tropez"],
];

fn write_orbits_as_two_discs(tree: &Tree) {
    for (index, titles) in DISC_TITLES.into_iter().enumerate() {
        let disc = index + 1;
        for (index, title) in titles.into_iter().enumerate() {
            let number = index as u32 + 1;
            let track = orbits_track(title, number)
                .text(ALBUM, &format!("Orbits [Disc {disc}]"))
                .described(MUSICBRAINZ_ALBUM, RELEASE);
            tree.write(&format!("Orbits/CD{disc}/{number}.wav"), &track.build());
        }
    }
}

fn orbits_on_two_discs() -> Release {
    let disc = |index: usize, title: Option<&str>| Medium {
        position: index as u32 + 1,
        format: Some("CD".to_owned()),
        title: title.map(str::to_owned),
        tracks: DISC_TITLES[index]
            .into_iter()
            .enumerate()
            .map(|(at, named)| release_row(at as u32 + 1, named, Vec::new()))
            .collect(),
    };

    Release {
        media: vec![disc(0, Some("The Early Orbit")), disc(1, None)],
        ..orbits(Vec::new(), Vec::new())
    }
}

#[test]
fn a_set_of_two_discs_is_billed_by_the_release_and_keeps_a_medium_of_its_own_for_each_disc()
-> Result<()> {
    let tree = Tree::new();
    write_orbits_as_two_discs(&tree);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let scanned = only_album(&library)?.title;
    assert!(
        DISC_TITLES
            .iter()
            .enumerate()
            .any(|(index, _)| scanned == format!("Orbits [Disc {}]", index + 1)),
        "the scan bills the set by whichever disc it read first, and got {scanned}"
    );

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits_on_two_discs()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;
    assert_eq!(album.title, "Orbits");

    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(
        release.media,
        vec![
            HeldMedium {
                position: 1,
                format: Some("CD".to_owned()),
                title: Some("The Early Orbit".to_owned()),
            },
            HeldMedium {
                position: 2,
                format: Some("CD".to_owned()),
                title: None,
            },
        ]
    );

    let placed: Vec<(u32, u32, String)> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| (row.disc, row.position, row.title))
        .collect();
    assert_eq!(
        placed,
        vec![
            (1, 1, "One of These Days".to_owned()),
            (1, 2, "A Pillow of Winds".to_owned()),
            (2, 1, "Fearless".to_owned()),
            (2, 2, "San Tropez".to_owned()),
        ]
    );

    scan(&library, &options(&tree))?;
    assert_eq!(
        only_album(&library)?.title,
        "Orbits",
        "a rescan read the set back under the disc tag the files carry"
    );
    Ok(())
}

#[test]
fn a_search_match_is_taken_only_under_the_strict_rule_and_a_near_miss_stamps_asked_alone()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(
        fake.called(LookupOp::FindRelease),
        2,
        "a near miss was not asked again in words"
    );
    assert_eq!(fake.called(LookupOp::Release), 0);
    let album = only_album(&library)?;
    assert_eq!(album.mbid, None);
    let release = library.release_of(album.id)?.expect("the album is known");
    assert!(release.asked.is_some());
    assert_eq!(release.answered, None);

    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("the orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));

    let tree = Tree::new();
    for (file, title, artist) in [
        ("dawn.wav", "Dawn", "Ada"),
        ("noon.wav", "Noon", "Ben"),
        ("dusk.wav", "Dusk", "Cleo"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    assert_eq!(album.artist_id, None);
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            release: mbid(HOURS),
            group: None,
            score: 95,
            title: "Hours".to_owned(),
            credit: credited(Some("Various Artists"), None),
            track_count: Some(3),
            date: None,
        }],
        releases: vec![hours()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(HOURS)));
    Ok(())
}

#[test]
fn a_phrase_that_answers_nothing_is_asked_again_in_words_and_lands_under_the_strict_rule()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases_in_words: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));

    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_groups_in_words: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::FindReleaseGroup(orbits_group_asked_in_words()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));
    Ok(())
}

fn orbits_in_two_folders() -> Tree {
    let tree = Tree::new();
    for (folder, frames) in [("ripped", 4_410), ("ripped again", 44_100 * 4)] {
        for (file, title) in [
            ("1.wav", "One of These Days"),
            ("2.wav", "A Pillow of Winds"),
            ("3.wav", "Fearless!"),
        ] {
            tree.write(
                &format!("{folder}/{file}"),
                &Wav::new()
                    .frames(frames)
                    .text(TITLE, title)
                    .text(ARTIST, "The Orbiters")
                    .text(ALBUM, "Orbits")
                    .build(),
            );
        }
    }

    tree
}

#[test]
fn a_scan_whose_write_fails_answers_the_failure_and_hands_the_tree_back() -> Result<()> {
    const FILES: usize = 2_400;
    const WAITS_AT_MOST: Duration = Duration::from_secs(60);

    let tree = Tree::new();
    let tone = Wav::new().frames(16).build();
    for nth in 0..FILES {
        tree.write(&format!("{nth:04}.wav"), &tone);
    }
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    let mut wide = options(&tree);
    wide.workers = NonZeroUsize::new(4).expect("four workers");
    scan(&library, &wide)?;

    beside(&database)
        .execute_batch(
            "CREATE TRIGGER refuse_a_touch BEFORE UPDATE OF seen ON tracks
             BEGIN SELECT RAISE(ABORT, 'the disc is full'); END;",
        )
        .expect("a trigger that refuses every write");

    let handle = library.scan(wide.clone())?;
    let (told, heard) = mpsc::channel();
    thread::spawn(move || {
        let _ = told.send(handle.join());
    });
    let answered = heard
        .recv_timeout(WAITS_AT_MOST)
        .expect("a scan whose write failed never finished");
    assert!(matches!(answered, Err(Error::Store { .. })));

    beside(&database)
        .execute_batch("DROP TRIGGER refuse_a_touch")
        .expect("the trigger taken away");
    scan(&library, &wide)?;
    Ok(())
}

#[test]
fn two_albums_that_turn_out_to_share_a_release_are_gathered_into_one() -> Result<()> {
    let tree = orbits_in_two_folders();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        library.albums(&AlbumQuery::default())?.len(),
        2,
        "the two folders were not scanned as two albums to gather"
    );

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(
        album.track_count, 6,
        "the gathered album does not hold both folders' tracks"
    );
    assert_eq!(all(&library)?.len(), 6, "a track was lost to the gathering");
    assert_eq!(
        keys(&database).len(),
        3,
        "the album is not named by both folders' keys and the release's own"
    );

    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    assert_eq!(
        album.mbid,
        Some(mbid(RELEASE)),
        "a rescan split the gathered album and lost what the release wrote"
    );
    assert_eq!(album.track_count, 6);
    Ok(())
}

#[test]
fn a_gathered_album_brings_its_favourite_and_its_vaulted_cover_with_it() -> Result<()> {
    for marked in 0..2 {
        let tree = orbits_in_two_folders();
        let held = Tree::new();
        let (library, vault) = opened_with_a_vault(&held)?;
        scan(&library, &options(&tree))?;

        let albums = library.albums(&AlbumQuery::default())?;
        assert_eq!(albums.len(), 2);
        let chosen = albums[marked].id;
        library.favour(Favoured::Album(chosen), true)?;
        assert!(library.land_archive_cover(chosen, &a_real_picture(12, 12))?);
        assert!(library.albums(&AlbumQuery::default())?[marked].has_cover_art);

        let fake = Arc::new(Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        }));
        enrich(&library, &fake, false)?;

        let album = only_album(&library)?;
        assert!(
            album.favourite.is_some(),
            "the favourite was lost gathering album {marked} into the other"
        );
        assert!(
            album.has_cover_art,
            "the vaulted cover of album {marked} was lost"
        );
        assert!(library.cover_art(album.id)?.is_some());
        assert_eq!(library.prune_the_vault()?.covers, 0);
        assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    }
    Ok(())
}

#[test]
fn a_phrase_that_answers_a_near_miss_is_asked_again_in_words_and_lands_there() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_releases_in_words: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        found_groups: vec![orbits_group_match(100, Some("Someone Else"))],
        found_groups_in_words: vec![orbits_group_match(100, Some("The Orbiters"))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let searches: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| {
            matches!(
                called.op(),
                LookupOp::FindRelease | LookupOp::FindReleaseGroup
            )
        })
        .collect();
    assert_eq!(
        searches,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
        ],
        "a phrase that landed nothing was not asked again in words, or the words were \
         not enough to spare the group search"
    );
    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    let release = library.release_of(album.id)?.expect("the album is known");
    assert!(release.answered.is_some());
    Ok(())
}

#[test]
fn an_album_no_release_matches_is_landed_as_its_release_group_with_a_cover() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(HOURS, "1996-05-06", None)],
            vec![Link::new(
                "free streaming",
                "https://example.invalid/orbits".to_owned(),
            )],
        )],
        group_covers: vec![(mbid(RELEASE_GROUP), png_art(64))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    let of_the_album: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| called.op() != LookupOp::FindRecording)
        .collect();
    assert_eq!(
        of_the_album,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::Cover), 1);

    let album = only_album(&library)?;
    assert_eq!(album.mbid, None);
    assert_eq!(album.year, Some(1971));
    assert!(album.has_cover_art);
    assert!(library.release_tracks(album.id)?.is_empty());

    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.mbid, None);
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.kind.as_deref(), Some("Album"));
    assert_eq!(release.date.as_deref(), Some("1971-10-30"));
    assert_eq!(release.disambiguation.as_deref(), Some("the original run"));
    assert!(release.asked.is_some() && release.answered.is_some());
    assert_eq!(release.cover_source, CoverSource::Archive);
    assert_eq!(
        release.links,
        vec![Link::new(
            "free streaming",
            "https://example.invalid/orbits".to_owned()
        )]
    );

    assert_eq!(summary.stats.releases, 0);
    assert_eq!(summary.stats.matched, 0);
    assert_eq!(summary.stats.covers, 1);
    assert_eq!(summary.stats.refused, 0);
    Ok(())
}

#[test]
fn a_group_whose_release_matches_the_album_lands_that_release_and_its_rows() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(5)),
                group_release(RELEASE, "1971-10-30", Some(3)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::Cover), 1);

    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(library.release_tracks(album.id)?.len(), 3);
    assert_eq!(album.missing, 0);

    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert_eq!(release.date.as_deref(), Some("1998-03-02"));
    assert!(release.answered.is_some());

    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.covers, 1);
    Ok(())
}

#[test]
fn an_album_wider_than_every_pressing_of_its_group_lands_the_widest_one() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let two_of_the_three: Vec<ReleaseTrack> = orbits_rows().into_iter().take(2).collect();
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(1)),
                group_release(RELEASE, "1971-10-30", Some(2)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(two_of_the_three, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let of_the_album: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| called.op() != LookupOp::FindRecording)
        .collect();
    assert_eq!(
        of_the_album,
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );

    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(library.release_tracks(album.id)?.len(), 2);
    assert_eq!(album.missing, 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 2);
    Ok(())
}

#[test]
fn an_album_tagged_with_a_release_group_id_is_asked_for_that_group_and_never_searched_for_one()
-> Result<()> {
    let tree = Tree::new();
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_RELEASE_GROUP, RELEASE_GROUP)
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(Vec::new(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindReleaseGroup), 0);
    assert_eq!(fake.called(LookupOp::ReleaseGroup), 1);
    assert_eq!(fake.called(LookupOp::Release), 0);
    assert_eq!(
        fake.calls()[..3],
        [
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
        ]
    );

    let album = only_album(&library)?;
    assert_eq!(album.mbid, None);
    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.kind.as_deref(), Some("Album"));
    assert!(release.answered.is_some());
    Ok(())
}

#[test]
fn an_album_asked_within_the_retry_window_is_not_asked_again_and_refresh_asks_anyway() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &fake, false)?;
    assert_eq!(fake.called(LookupOp::FindRelease), 2);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);
    let album = only_album(&library)?;
    assert!(
        library
            .release_of(album.id)?
            .is_some_and(|release| release.asked.is_some())
    );

    let summary = enrich(&library, &fake, false)?;
    assert_eq!(fake.called(LookupOp::FindRelease), 2);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);
    assert_eq!(summary.stats.albums, 0);
    assert_eq!(summary.stats.artists, 0);

    let summary = enrich(&library, &fake, true)?;
    assert_eq!(fake.called(LookupOp::FindRelease), 4);
    assert_eq!(fake.called(LookupOp::FindArtist), 2);
    assert_eq!(summary.stats.albums, 1);
    assert_eq!(summary.stats.artists, 1);
    Ok(())
}

#[test]
fn a_cover_the_archive_holds_lands_only_where_the_files_embedded_none() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let embedded = png(64);
    write_hours(&tree, Some(&embedded));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        covers: vec![(mbid(RELEASE), png_art(256)), (mbid(HOURS), png_art(128))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let covers: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Cover)
        .collect();
    assert_eq!(
        covers,
        vec![Called::Cover(mbid(RELEASE), Some(mbid(RELEASE_GROUP)))]
    );
    assert_eq!(summary.stats.covers, 1);

    let orbits = album_titled(&library, "Orbits")?;
    assert_eq!(library.cover_art(orbits.id)?, Some(png_art(256)));
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.cover_source),
        Some(CoverSource::Archive)
    );
    let hours = album_titled(&library, "Hours")?;
    assert_eq!(
        library.cover_art(hours.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: embedded,
        })
    );
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.cover_source),
        Some(CoverSource::File)
    );
    Ok(())
}

#[test]
fn an_artist_named_in_a_release_credit_is_looked_up_by_the_credit_id_and_never_searched()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());
    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    let artists: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Artist)
        .collect();
    assert_eq!(artists, vec![Called::Artist(mbid(ORBITERS))]);
    assert_eq!(summary.stats.artists, 1);

    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");
    assert!(detail.answered.is_some());
    assert_eq!(detail.sort_name.as_deref(), Some("Orbiters, The"));
    Ok(())
}

fn artist_release(
    id: &str,
    title: &str,
    kind: Option<&str>,
    secondary: &[&str],
    first_released: Option<&str>,
) -> ArtistRelease {
    ArtistRelease {
        mbid: mbid(id),
        title: title.to_owned(),
        kind: kind.map(str::to_owned),
        secondary: secondary.iter().map(|each| (*each).to_owned()).collect(),
        first_released: first_released.map(str::to_owned),
    }
}

fn orbiters_groups() -> Vec<ArtistRelease> {
    vec![
        artist_release(
            RELEASE_GROUP,
            "Orbits",
            Some("Album"),
            &[],
            Some("1998-03-02"),
        ),
        artist_release(HOURS_GROUP, "Hours", Some("EP"), &[], Some("1996-05-06")),
        artist_release(
            LIVE_GROUP,
            "Orbits Live",
            Some("Album"),
            &["Live"],
            Some("1999"),
        ),
        artist_release(
            BEST_OF_GROUP,
            "The Best of the Orbiters",
            Some("Album"),
            &["Compilation"],
            Some("2001"),
        ),
        artist_release(
            SCORE_GROUP,
            "The Orbit",
            Some("Album"),
            &["Soundtrack"],
            Some("2003-11"),
        ),
        artist_release(SINGLE_GROUP, "San Tropez", Some("Single"), &[], None),
        artist_release(
            BROADCAST_GROUP,
            "A Broadcast",
            Some("Broadcast"),
            &[],
            Some("1997"),
        ),
    ]
}

fn unheld(
    artist: &Artist,
    id: &str,
    title: &str,
    kind: &str,
    first_released: Option<&str>,
) -> UnheldRelease {
    UnheldRelease {
        artist: artist.id,
        artist_name: artist.name.clone(),
        mbid: mbid(id),
        title: title.to_owned(),
        kind: Some(kind.to_owned()),
        first_released: first_released.map(str::to_owned),
    }
}

#[test]
fn an_artists_discography_is_kept_once_its_profile_lands_and_the_catalog_says_what_it_does_not_hold()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());
    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];
    let canned = || Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release.clone()],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        ..Canned::default()
    };
    let fake = Arc::new(Fake::new(canned()));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::Release(mbid(RELEASE)),
            Called::Artist(mbid(ORBITERS)),
            Called::ReleaseGroupsOf(mbid(ORBITERS)),
        ]
    );
    assert_eq!(summary.stats.releases_found, 3);

    let artist = artist_named(&library, "The Orbiters")?;
    let expected = vec![
        unheld(&artist, HOURS_GROUP, "Hours", "EP", Some("1996-05-06")),
        unheld(&artist, SCORE_GROUP, "The Orbit", "Album", Some("2003-11")),
    ];
    assert_eq!(library.unheld_releases(None, None)?, expected);
    assert_eq!(library.unheld_releases(None, Some(1))?, expected[..1]);
    assert_eq!(
        library
            .artist_detail(artist.id)?
            .map(|detail| detail.releases_unheld),
        Some(2)
    );
    assert_eq!(
        library.missing_counted(None)?,
        Missing {
            tracks: 0,
            releases: 2,
        }
    );

    let fake = Arc::new(Fake::new(canned()));
    let summary = enrich(&library, &fake, true)?;
    assert_eq!(fake.called(LookupOp::ReleaseGroupsOfArtist), 1);
    assert_eq!(summary.stats.releases_found, 3);
    assert_eq!(library.unheld_releases(None, None)?, expected);
    assert_eq!(
        library
            .artist_detail(artist.id)?
            .map(|detail| detail.releases_unheld),
        Some(2)
    );
    Ok(())
}

#[test]
fn what_the_catalog_is_short_of_is_narrowed_by_the_words_typed() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());
    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let artist = artist_named(&library, "The Orbiters")?;
    let hours = unheld(&artist, HOURS_GROUP, "Hours", "EP", Some("1996-05-06"));
    let orbit = unheld(&artist, SCORE_GROUP, "The Orbit", "Album", Some("2003-11"));

    assert_eq!(
        library.unheld_releases(Some("hours"), None)?,
        vec![hours.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("ep"), None)?,
        vec![hours.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("2003"), None)?,
        vec![orbit.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("orbiters album"), None)?,
        vec![orbit.clone()]
    );
    assert!(
        library
            .unheld_releases(Some("nothing of the sort"), None)?
            .is_empty()
    );
    assert_eq!(
        library.missing_counted(Some("hours"))?,
        Missing {
            tracks: 0,
            releases: 1,
        }
    );

    let album = only_album(&library)?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    assert_eq!(library.rematch(album.id)?, 3);

    assert_eq!(library.missing_tracks(Some("orbits"), None)?.len(), 1);
    assert!(library.missing_tracks(Some("hours"), None)?.is_empty());
    assert_eq!(
        library.missing_counted(Some("orbits"))?,
        Missing {
            tracks: 1,
            releases: 0,
        }
    );
    Ok(())
}

#[test]
fn an_unheld_release_is_found_by_a_name_spelt_either_way() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = artist_named(&library, "The Orbiters")?;
    library.land_artist_releases(
        artist.id,
        &[artist_release(
            HOURS_GROUP,
            "Hōrs Przybyłowicz",
            Some("EP"),
            &[],
            Some("1996-05-06"),
        )],
    )?;

    assert_eq!(
        library.unheld_releases(Some("przybylowicz"), None)?.len(),
        1
    );
    assert_eq!(library.unheld_releases(Some("HORS"), None)?.len(), 1);
    Ok(())
}

#[test]
fn the_rows_an_album_is_short_of_are_listed_with_their_wants() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    rows.push(release_row(5, "Seamus", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    assert_eq!(library.rematch(album.id)?, 3);
    let san_tropez = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    let want = library.want(san_tropez.id)?;

    let missing = library.missing_tracks(None, None)?;
    assert_eq!(missing.len(), 2);
    assert_eq!(
        missing[0],
        MissingTrack {
            album: album.id,
            album_title: "Orbits".to_owned(),
            owner: Some("The Orbiters".to_owned()),
            release_track: san_tropez.id,
            disc: 1,
            position: 4,
            number: Some("4".to_owned()),
            title: "San Tropez".to_owned(),
            artist: None,
            length: Some(Duration::from_secs(4)),
            want: Some(want),
        }
    );
    assert_eq!(missing[1].title, "Seamus");
    assert_eq!(missing[1].position, 5);
    assert_eq!(missing[1].want, None);
    assert_eq!(library.missing_tracks(None, Some(1))?, missing[..1]);
    assert_eq!(
        library.missing_counted(None)?,
        Missing {
            tracks: 2,
            releases: 0,
        }
    );
    assert!(library.unheld_releases(None, None)?.is_empty());
    Ok(())
}

#[test]
fn an_artist_named_in_a_release_group_credit_is_spared_a_search_the_same_way() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut group = orbits_group(vec![group_release(HOURS, "1996-05-06", None)], Vec::new());
    group.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(4))],
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![group],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    let artists: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Artist)
        .collect();
    assert_eq!(artists, vec![Called::Artist(mbid(ORBITERS))]);
    assert_eq!(summary.stats.artists, 1);

    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    Ok(())
}

#[test]
fn an_artist_search_is_taken_only_at_a_perfect_score_with_the_same_folded_name() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let found = |score: u8, name: &str| ArtistMatch {
        mbid: mbid(ORBITERS),
        name: name.to_owned(),
        score,
        kind: Some("Group".to_owned()),
        disambiguation: None,
        aliases: Vec::new(),
    };

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(99, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(fake.called(LookupOp::FindArtist), 1);
    assert_eq!(fake.called(LookupOp::Artist), 0);
    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, None);
    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");
    assert!(detail.asked.is_some());
    assert_eq!(detail.answered, None);

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(100, "Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, true)?;
    assert_eq!(fake.called(LookupOp::Artist), 0);
    assert_eq!(artist_named(&library, "The Orbiters")?.mbid, None);

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![found(100, "the orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, true)?;
    assert_eq!(fake.called(LookupOp::Artist), 1);
    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    assert!(
        library
            .artist_detail(artist.id)?
            .is_some_and(|detail| detail.answered.is_some())
    );
    Ok(())
}

#[test]
fn a_portrait_is_fetched_only_where_the_profile_names_an_image_and_none_is_held() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "Orbits/1.wav",
        &orbits_track("One of These Days", 1)
            .described(MUSICBRAINZ_ARTIST, ORBITERS)
            .build(),
    );
    write_hours(&tree, None);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        artists: vec![orbiters(), ada()],
        portraits: vec![(ORBITERS_PICTURE.to_owned(), png_art(96))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let portraits: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::Portrait)
        .collect();
    assert_eq!(
        portraits,
        vec![Called::Portrait(ORBITERS_PICTURE.to_owned())]
    );
    assert_eq!(summary.stats.artists, 2);
    assert_eq!(summary.stats.portraits, 1);

    let orbiters = artist_named(&library, "The Orbiters")?;
    assert!(orbiters.has_portrait);
    assert_eq!(library.portrait(orbiters.id)?, Some(png_art(96)));
    let ada = artist_named(&library, "Ada")?;
    assert!(!ada.has_portrait);
    assert_eq!(library.portrait(ada.id)?, None);

    let summary = enrich(&library, &fake, true)?;
    assert_eq!(fake.called(LookupOp::Portrait), 1);
    assert_eq!(summary.stats.portraits, 0);
    Ok(())
}

#[test]
fn a_reference_that_cannot_be_reached_ends_the_pass_and_stamps_nothing() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    write_hours(&tree, None);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 1, Fault::Unreachable),
    );
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, Some(LookupOp::Release));
    assert_eq!(fake.called(LookupOp::Release), 2);
    assert_eq!(fake.called(LookupOp::Artist), 0);
    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.refused, 0);

    let mut answered = 0;
    let mut untouched = 0;
    for album in library.albums(&AlbumQuery::default())? {
        let release = library.release_of(album.id)?.expect("the album is known");
        match (release.asked, release.answered) {
            (Some(_), Some(_)) => answered += 1,
            (None, None) => untouched += 1,
            other => panic!("an album was left half stamped: {other:?}"),
        }
    }
    assert_eq!((answered, untouched), (1, 1));
    Ok(())
}

#[test]
fn a_refusal_counts_and_stamps_asked_and_the_pass_carries_on() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 0, Fault::Refused),
    );
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stopped_by, None);
    assert_eq!(summary.stats.refused, 1);
    assert_eq!(summary.stats.releases, 0);
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(fake.called(LookupOp::Cover), 0);
    assert_eq!(fake.called(LookupOp::FindArtist), 1);

    let album = only_album(&library)?;
    assert_eq!(album.mbid, None);
    let release = library.release_of(album.id)?.expect("the album is known");
    assert!(release.asked.is_some());
    assert_eq!(release.answered, None);
    Ok(())
}

#[test]
fn a_refusal_earns_the_wait_a_service_is_given_rather_than_the_one_a_miss_earns() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(Canned {
            found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
            releases: vec![orbits(orbits_rows(), Vec::new())],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 0, Fault::Refused),
    );
    assert_eq!(enrich(&library, &fake, false)?.stats.refused, 1);

    let refused_at_once = Waits {
        retry_after: A_DAY,
        refused_again_after: Duration::ZERO,
        refresh_after: A_DAY,
    };
    let missed_at_once = Waits {
        retry_after: Duration::ZERO,
        refused_again_after: A_DAY,
        refresh_after: A_DAY,
    };

    assert_eq!(
        library.albums_to_ask(refused_at_once, false)?.len(),
        1,
        "a refused album waits out the wait a refusal earns"
    );
    assert!(
        library.albums_to_ask(missed_at_once, false)?.is_empty(),
        "a refused album was counted as an album nobody has heard of"
    );
    Ok(())
}

#[test]
fn a_rescan_that_adds_a_track_takes_it_off_the_missing_count_without_asking_again() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "Orbits/1.wav",
        &orbits_track("One of These Days", 1)
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .build(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;
    assert_eq!(summary.stats.matched, 1);
    assert_eq!(only_album(&library)?.missing, 2);

    tree.write(
        "Orbits/2.wav",
        &orbits_track("A Pillow of Winds", 2)
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .build(),
    );
    let stats = scan(&library, &options(&tree))?;
    assert_eq!(stats.added, 1);
    assert_eq!(only_album(&library)?.missing, 2);

    let silent = Arc::new(Fake::new(Canned::default()));
    let summary = enrich(&library, &silent, false)?;
    assert!(
        silent.calls().is_empty(),
        "the reference was asked again: {:?}",
        silent.calls()
    );
    assert_eq!(summary.stats.albums, 0);
    assert_eq!(
        summary.stats.matched, 1,
        "the count says how many rows the rematch moved, not how many are paired"
    );
    assert_eq!(only_album(&library)?.missing, 1);
    Ok(())
}

#[test]
fn a_picture_is_fetched_beside_the_pass_rather_than_in_it() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        covers: vec![(mbid(RELEASE), png_art(256))],
        found_artists: vec![artist_match(100, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    })
    .gated_on(Some(LookupOp::Cover));
    let fake = Arc::new(fake);
    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started.recv().expect("the cover reaches the reference");
    let asked_while_held = waited_for(|| fake.called(LookupOp::FindArtist) == 1);
    go.send(()).expect("the reference is still waiting");
    let summary = handle.join()?;

    assert!(
        asked_while_held,
        "the pass waited for a picture rather than going on: {:?}",
        fake.calls()
    );
    assert_eq!(summary.stats.covers, 1);
    assert!(album_titled(&library, "Orbits")?.has_cover_art);
    Ok(())
}

#[test]
fn a_pass_the_reference_ended_is_left_for_the_next_run_to_pick_up() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    write_hours(&tree, None);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(library.unfinished_enrichment()?, None);

    let fake = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
            ..Canned::default()
        })
        .faulting(LookupOp::Release, 1, Fault::Unreachable),
    );
    let summary = enrich(&library, &fake, true)?;

    assert_eq!(summary.stopped_by, Some(LookupOp::Release));
    let left = library
        .unfinished_enrichment()?
        .expect("a pass the reference ended is left unfinished");
    assert!(
        left.refresh,
        "the next run would pick the pass up without the refresh it was asked for"
    );

    let answering = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        ..Canned::default()
    }));
    enrich(&library, &answering, true)?;
    assert_eq!(
        library.unfinished_enrichment()?,
        None,
        "a pass that reached the end of its queue is still there to be picked up"
    );
    Ok(())
}

fn tagged_orbits() -> Result<(Tree, Library)> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

#[test]
fn a_cover_the_archive_did_not_hand_over_is_asked_for_again_by_the_next_pass() -> Result<()> {
    let (_tree, library) = tagged_orbits()?;
    let faulted = Arc::new(
        Fake::new(Canned {
            releases: vec![orbits(orbits_rows(), Vec::new())],
            covers: vec![(mbid(RELEASE), png_art(128))],
            ..Canned::default()
        })
        .faulting(LookupOp::Cover, 0, Fault::Refused),
    );
    let first = enrich(&library, &faulted, false)?;
    assert_eq!(first.stats.releases, 1);
    assert_eq!(first.stats.covers, 0);
    assert_eq!(library.cover_art(only_album(&library)?.id)?, None);

    let answering = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    let second = enrich(&library, &answering, false)?;

    assert_eq!(
        answering.calls(),
        vec![Called::Cover(mbid(RELEASE), Some(mbid(RELEASE_GROUP)))],
        "the pass asked for more than the cover it is still short of"
    );
    assert_eq!(second.stats.covers, 1);
    assert_eq!(
        library.cover_art(only_album(&library)?.id)?,
        Some(png_art(128))
    );
    Ok(())
}

#[test]
fn a_cover_the_archive_answered_it_does_not_hold_is_not_asked_for_again_within_the_month()
-> Result<()> {
    let (_tree, library) = tagged_orbits()?;
    let empty_handed = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &empty_handed, false)?;
    assert_eq!(empty_handed.called(LookupOp::Cover), 1);

    let again = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    enrich(&library, &again, false)?;

    assert_eq!(again.called(LookupOp::Cover), 0);
    assert_eq!(library.cover_art(only_album(&library)?.id)?, None);

    assert_eq!(library.ask_again_for_covers()?, 1);
    let asked_again = enrich(&library, &again, false)?;
    assert_eq!(again.called(LookupOp::Cover), 1);
    assert_eq!(asked_again.stats.covers, 1);
    assert_eq!(library.ask_again_for_covers()?, 0);
    Ok(())
}

#[test]
fn a_cover_the_archive_said_it_lacked_a_month_ago_is_asked_for_again() -> Result<()> {
    const A_YEAR_AGO_IN_NANOS: i64 = 365 * 24 * 60 * 60 * 1_000_000_000;

    let tree = Tree::new();
    write_orbits(&tree, true);
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    let empty_handed = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &empty_handed, false)?;
    assert_eq!(empty_handed.called(LookupOp::Cover), 1);

    let stamped: i64 = beside(&database)
        .query_row("SELECT cover_asked FROM albums", [], |row| row.get(0))
        .expect("the answer was stamped");
    beside(&database)
        .execute(
            "UPDATE albums SET cover_asked = ?1",
            [stamped - A_YEAR_AGO_IN_NANOS],
        )
        .expect("the stamp is put back a year");

    let later = Arc::new(Fake::new(Canned {
        covers: vec![(mbid(RELEASE), png_art(128))],
        ..Canned::default()
    }));
    let asked_again = enrich(&library, &later, false)?;

    assert_eq!(later.called(LookupOp::Cover), 1);
    assert_eq!(asked_again.stats.covers, 1);
    assert_eq!(
        library.cover_art(only_album(&library)?.id)?,
        Some(png_art(128))
    );
    Ok(())
}

#[test]
fn cancelling_an_enrichment_stops_it_between_requests() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    write_hours(&tree, None);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        covers: vec![(mbid(RELEASE), png_art(256)), (mbid(HOURS), png_art(128))],
        ..Canned::default()
    })
    .gated();
    let fake = Arc::new(fake);
    let reference = Arc::clone(&fake) as Arc<dyn Reference>;
    let handle = library.enrich(
        reference,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started
        .recv()
        .expect("the first request reaches the reference");
    handle.cancel();
    assert!(handle.progress().is_cancelled());
    go.send(()).expect("the reference is still waiting");
    let summary = handle.join()?;

    assert!(summary.cancelled);
    assert_eq!(summary.stopped_by, None);
    assert_eq!(fake.calls().len(), 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(
        library.unfinished_enrichment()?,
        None,
        "a pass the listener stopped was left for the next run to pick up"
    );

    let answered = library
        .albums(&AlbumQuery::default())?
        .into_iter()
        .filter(|album| {
            library
                .release_of(album.id)
                .ok()
                .flatten()
                .is_some_and(|release| release.answered.is_some())
        })
        .count();
    assert_eq!(answered, 1);
    Ok(())
}

fn waited_for(settled: impl Fn() -> bool) -> bool {
    const LOOKS: u32 = 500;
    const BETWEEN_LOOKS: Duration = Duration::from_millis(10);

    (0..LOOKS).any(|_| {
        let now = settled();
        if !now {
            thread::sleep(BETWEEN_LOOKS);
        }
        now
    })
}

fn asked_in_order(fake: &Fake) -> Vec<Called> {
    fake.calls()
        .into_iter()
        .filter(|called| !matches!(called.op(), LookupOp::Cover | LookupOp::Portrait))
        .collect()
}

fn artist_match(score: u8, name: &str) -> ArtistMatch {
    ArtistMatch {
        mbid: mbid(ORBITERS),
        name: name.to_owned(),
        score,
        kind: Some("Group".to_owned()),
        disambiguation: None,
        aliases: Vec::new(),
    }
}

fn under_a_gated_pass(
    library: &Library,
    fake: Fake,
    has_started: &Receiver<()>,
    go: &Sender<()>,
    refresh: bool,
    reached: impl FnOnce(&Sought),
) -> Result<(Arc<Fake>, EnrichSummary)> {
    let fake = Arc::new(fake);
    let sought = Arc::new(Sought::default());
    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions {
            refresh,
            sought: Arc::clone(&sought),
            ..EnrichOptions::default()
        },
    )?;

    has_started
        .recv()
        .expect("the first request reaches the reference");
    reached(&sought);
    go.send(()).expect("the reference is still waiting");
    let summary = handle.join()?;
    assert!(!summary.cancelled, "the pass reported itself cancelled");

    Ok((fake, summary))
}

fn asked_at(fake: &Fake, wanted: &Called) -> Option<usize> {
    fake.calls().iter().position(|called| called == wanted)
}

#[test]
fn an_album_that_became_due_under_a_running_pass_is_asked_next_when_it_is_sought() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new()), hours()],
        artists: vec![orbiters(), ada()],
        found_artists: vec![artist_match(100, "The Orbiters"), artist_match(100, "Ada")],
        ..Canned::default()
    })
    .gated();
    let (fake, _) = under_a_gated_pass(&library, fake, &has_started, &go, false, |sought| {
        write_hours(&tree, None);
        scan(&library, &options(&tree)).expect("a second scan lands the new album");
        sought.album(
            album_titled(&library, "Hours")
                .expect("the new album was scanned")
                .id,
        );
    })?;

    let hours = asked_at(&fake, &Called::Release(mbid(HOURS)));
    assert!(
        hours.is_some(),
        "an album scanned under the pass was never asked about: {:?}",
        fake.calls()
    );
    let artist = fake
        .calls()
        .iter()
        .position(|called| called.op() == LookupOp::FindArtist || called.op() == LookupOp::Artist);
    assert!(
        hours < artist,
        "the sought album was asked after the artists the queue already held: {:?}",
        fake.calls()
    );
    Ok(())
}

#[test]
fn a_seek_naming_an_album_the_reference_has_already_answered_asks_nothing() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let answering = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    }));
    enrich(&library, &answering, false)?;

    let orbits = album_titled(&library, "Orbits")?.id;
    let silent = Arc::new(Fake::new(Canned::default()));
    let sought = Arc::new(Sought::default());
    sought.album(orbits);
    let summary = library
        .enrich(
            Arc::clone(&silent) as Arc<dyn Reference>,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                sought,
                ..EnrichOptions::default()
            },
        )?
        .join()?;

    let asked = queued(&silent);
    assert!(
        asked.is_empty(),
        "a seek asked again about an album that was not due: {asked:?}"
    );
    assert_eq!(summary.stats.albums, 0);
    Ok(())
}

fn queued(fake: &Fake) -> Vec<Called> {
    fake.calls()
        .into_iter()
        .filter(|called| called.op() != LookupOp::Portrait)
        .collect()
}

#[test]
fn a_portrait_that_never_landed_is_looked_for_again_without_asking_the_reference_twice()
-> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let refusing = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    }));
    enrich(&library, &refusing, false)?;
    assert_eq!(
        refusing.called(LookupOp::Portrait),
        1,
        "the picture was not asked for once while the artist was being landed"
    );

    let artist = artist_named(&library, "The Orbiters")?;
    assert!(!artist.has_portrait, "the fake answered with a portrait");

    let answering = Arc::new(Fake::new(Canned {
        portraits: vec![(ORBITERS_PICTURE.to_owned(), png_art(96))],
        ..Canned::default()
    }));
    let summary = enrich(&library, &answering, false)?;

    assert_eq!(
        queued(&answering),
        Vec::new(),
        "the retry asked the reference about an album or an artist that was not due"
    );
    assert_eq!(
        answering.called(LookupOp::Portrait),
        1,
        "the picture was not looked for again out of the links already held"
    );
    assert_eq!(summary.stats.portraits, 1);
    assert!(artist_named(&library, "The Orbiters")?.has_portrait);

    let settled = Arc::new(Fake::new(Canned::default()));
    enrich(&library, &settled, false)?;
    assert_eq!(
        settled.called(LookupOp::Portrait),
        0,
        "a portrait that has landed was looked for again"
    );
    Ok(())
}

#[test]
fn a_seek_naming_the_album_the_pass_is_asking_about_does_not_ask_twice() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = album_titled(&library, "Orbits")?.id;

    let (fake, has_started, go) = Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        found_artists: vec![artist_match(100, "The Orbiters")],
        ..Canned::default()
    })
    .gated();
    let (fake, _) = under_a_gated_pass(&library, fake, &has_started, &go, true, |sought| {
        sought.album(album);
    })?;

    assert_eq!(
        fake.called(LookupOp::Release),
        1,
        "the album being asked about was asked about again: {:?}",
        fake.calls()
    );
    Ok(())
}

fn isrc(text: &str) -> Isrc {
    Isrc::new(text).expect("a well-formed isrc")
}

fn on_orbits(position: u32) -> Vec<RecordingRelease> {
    vec![RecordingRelease {
        id: mbid(RELEASE),
        title: "Orbits".to_owned(),
        date: Some("1971-10-30".to_owned()),
        disc: Some(1),
        position: Some(position),
    }]
}

fn orbits_recording(
    id: &str,
    title: &str,
    code: &str,
    releases: Vec<RecordingRelease>,
) -> Recording {
    Recording {
        id: mbid(id),
        title: title.to_owned(),
        credit: vec![Credit {
            name: "The Orbiters".to_owned(),
            joined_by: String::new(),
            mbid: Some(mbid(ORBITERS)),
        }],
        length: Some(THE_FILES_LENGTH),
        isrcs: vec![isrc(code)],
        releases,
    }
}

fn recording_match(id: &str, title: &str) -> RecordingMatch {
    RecordingMatch {
        recording: mbid(id),
        score: 100,
        title: title.to_owned(),
        credit: vec![Credit {
            name: "The Orbiters".to_owned(),
            joined_by: String::new(),
            mbid: None,
        }],
        length: Some(THE_FILES_LENGTH),
        isrcs: Vec::new(),
        releases: Vec::new(),
    }
}

fn scanned_lone(track: Wav) -> Result<(Tree, Library, PathBuf, PathBuf)> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &track.build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    Ok((tree, library, database, file))
}

fn recorded(database: &Path, file: &Path) -> Option<String> {
    beside(database)
        .query_row(
            "SELECT mbid FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| row.get(0),
        )
        .expect("the scan stored the track")
}

#[test]
fn an_isrc_that_names_no_release_is_followed_up_by_asking_where_the_recording_sits() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;

    let bare = orbits_recording(RECORDING, "One of These Days", CODE, Vec::new());
    let whole = orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(2));
    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(isrc(CODE), bare)],
        recordings: vec![whole],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        1,
        "an ISRC answering no release was not followed up"
    );
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);
    assert_eq!(
        held.release_title.as_deref(),
        Some("Orbits"),
        "the release the follow-up named was not written"
    );
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let row = library
        .track_at(&file, None)?
        .expect("the catalog holds the track");
    assert_eq!(
        row.track_number,
        Some(2),
        "the place the follow-up named was not written"
    );
    assert_eq!(row.disc_number, Some(1));
    Ok(())
}

#[test]
fn a_track_with_no_album_is_identified_by_the_isrc_the_file_carries() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;
    assert!(library.albums(&AlbumQuery::default())?.is_empty());

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
        )],
        found_recordings: vec![recording_match(ANOTHER_RECORDING, "One of These Days")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 1);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(fake.called(LookupOp::Recording), 0);
    assert_eq!(fake.called(LookupOp::Release), 0);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);

    let held = stored(&database, &file);
    assert_eq!(held.title, "One of These Days");
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.asked.is_some() && held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let tracks = all(&library)?;
    assert_eq!(tracks[0].track_number, Some(1));
    assert_eq!(tracks[0].disc_number, Some(1));

    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    assert_eq!(tracks[0].artist_id, Some(artist.id));
    assert_eq!(fake.called(LookupOp::FindArtist), 0);
    assert_eq!(fake.called(LookupOp::Artist), 1);
    Ok(())
}

#[test]
fn a_track_is_identified_by_the_recording_id_the_file_carries_before_any_search() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .identified(MUSICBRAINZ, RECORDING),
    )?;
    assert_eq!(
        recorded(&database, &file).as_deref(),
        Some(RECORDING),
        "the scan did not read the recording id out of the file"
    );

    let fake = Arc::new(Fake::new(Canned {
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        found_recordings: vec![recording_match(ANOTHER_RECORDING, "One of These Days")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::Isrc), 0);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(
        fake.calls().first(),
        Some(&Called::Recording(mbid(RECORDING)))
    );
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);
    assert_eq!(held.isrc.as_deref(), Some("GBAYE7100195"));
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.answered.is_some());
    Ok(())
}

#[test]
fn an_artist_a_landing_names_for_the_first_time_is_asked_about_in_the_same_pass() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "An Orbiters Tribute")
            .identified(MUSICBRAINZ, RECORDING),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    assert!(
        fake.calls().contains(&Called::Artist(mbid(ORBITERS))),
        "the artist the recording credited was left for the next pass: {:?}",
        fake.calls()
    );
    let artist = artist_named(&library, "The Orbiters")?;
    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");
    assert!(detail.answered.is_some());
    Ok(())
}

#[test]
fn a_track_that_names_no_artist_is_never_searched_for() -> Result<()> {
    let (_tree, library, database, file) =
        scanned_lone(Wav::new().text(TITLE, "One of These Days"))?;
    assert!(
        library.albums(&AlbumQuery::default())?.is_empty(),
        "a file naming no album was filed under one"
    );
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert!(
        fake.calls().is_empty(),
        "a track naming nobody was asked about: {:?}",
        fake.calls()
    );
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);
    let held = stored(&database, &file);
    assert!(held.asked.is_some());
    assert_eq!(held.answered, None);
    assert_eq!(recorded(&database, &file), None);

    let (_tree, library, ..) = scanned_lone(Wav::new().text(ARTIST, "The Orbiters"))?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "1")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(
        fake.called(LookupOp::FindRecording),
        0,
        "a track whose title is only its file name was searched for"
    );
    Ok(())
}

#[test]
fn a_track_naming_no_artist_under_an_unowned_album_is_searched_with_its_release() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ALBUM, "Orbits"),
    )?;
    assert_eq!(only_album(&library)?.artist, None);
    let fake = Arc::new(Fake::new(Canned::default()));
    enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    let asked = RecordingAsked {
        title: "One of These Days".to_owned(),
        artist: None,
        artist_mbid: None,
        release: Some("Orbits".to_owned()),
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };
    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    Ok(())
}

#[test]
fn a_track_naming_no_artist_under_an_owned_album_is_searched_with_the_owner_and_lands() -> Result<()>
{
    let tree = Tree::new();
    tree.write(
        "Orbits/1.wav",
        &Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "1")
            .build(),
    );
    let unnamed = tree.write(
        "Orbits/2.wav",
        &Wav::new()
            .text(TITLE, "A Pillow of Winds")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    assert_eq!(album.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(stored(&database, &unnamed).artist, None);

    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "A Pillow of Winds")],
        recordings: vec![orbits_recording(
            RECORDING,
            "A Pillow of Winds",
            CODE,
            on_orbits(2),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    assert!(
        searched.contains(&Called::FindRecording(RecordingAsked {
            title: "A Pillow of Winds".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            artist_mbid: None,
            release: None,
            length: Some(THE_FILES_LENGTH),
            wording: Wording::Phrase,
        })),
        "the track naming no artist was not searched for with its album's owner: {searched:?}"
    );
    assert_eq!(summary.stats.named, 1);
    assert_eq!(recorded(&database, &unnamed).as_deref(), Some(RECORDING));
    let held = stored(&database, &unnamed);
    assert_eq!(held.artist.as_deref(), Some("The Orbiters"));
    assert!(held.answered.is_some());
    Ok(())
}

#[test]
fn a_tagged_release_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands() -> Result<()>
{
    let tree = Tree::new();
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_ALBUM, HOURS)
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(only_album(&library)?.mbid, Some(mbid(HOURS)));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::Release(mbid(HOURS)),
            Called::FindRelease(orbits_asked()),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);
    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 0);
    Ok(())
}

#[test]
fn a_tagged_release_group_id_the_reference_does_not_hold_falls_back_to_a_group_search() -> Result<()>
{
    let tree = Tree::new();
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_RELEASE_GROUP, HOURS)
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let fake = Arc::new(Fake::new(Canned {
        found_groups: vec![orbits_group_match(100, Some("The Orbiters"))],
        groups: vec![orbits_group(
            vec![group_release(RELEASE, "1971-10-30", Some(3))],
            Vec::new(),
        )],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::FindRelease(orbits_asked_in_words()),
            Called::ReleaseGroup(mbid(HOURS)),
            Called::FindReleaseGroup(orbits_group_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    let release = library.release_of(album.id)?.expect("the album is known");
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    Ok(())
}

#[test]
fn a_tagged_artist_id_the_reference_does_not_hold_falls_back_to_a_search_that_lands() -> Result<()>
{
    let tree = Tree::new();
    tree.write(
        "Orbits/1.wav",
        &orbits_track("One of These Days", 1)
            .described(MUSICBRAINZ_ARTIST, ADA)
            .build(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ADA))
    );

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![artist_match(100, "The Orbiters")],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let of_the_artist: Vec<Called> = asked_in_order(&fake)
        .into_iter()
        .filter(|called| {
            matches!(
                called.op(),
                LookupOp::Artist | LookupOp::FindArtist | LookupOp::Portrait
            )
        })
        .collect();
    assert_eq!(
        of_the_artist,
        vec![
            Called::Artist(mbid(ADA)),
            Called::FindArtist("The Orbiters".to_owned()),
            Called::Artist(mbid(ORBITERS)),
        ]
    );
    assert_eq!(summary.stats.artists, 1);
    let artist = artist_named(&library, "The Orbiters")?;
    assert_eq!(artist.mbid, Some(mbid(ORBITERS)));
    assert!(
        library
            .artist_detail(artist.id)?
            .is_some_and(|detail| detail.answered.is_some())
    );
    Ok(())
}

#[test]
fn a_hit_one_track_short_lands_the_pressing_its_group_names_and_lists_the_missing_row() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            group: Some(mbid(RELEASE_GROUP)),
            ..orbits_match(100, Some("The Orbiters"), Some(4))
        }],
        groups: vec![orbits_group(
            vec![
                group_release(HOURS, "1996-05-06", Some(2)),
                group_release(RELEASE, "1971-10-30", Some(4)),
            ],
            Vec::new(),
        )],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );
    assert_eq!(fake.called(LookupOp::FindReleaseGroup), 0);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.matched, 3);

    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 1);
    assert_eq!(library.release_tracks(album.id)?.len(), 4);
    Ok(())
}

fn identifiers(database: &Path, file: &Path) -> (Option<String>, Option<String>, Option<String>) {
    beside(database)
        .query_row(
            "SELECT mbid, release_track_mbid, isrc FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the scan stored the track")
}

#[test]
fn a_paired_track_receives_the_identifiers_its_release_row_holds() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let file = tree.path().join("1.wav");
    assert_eq!(identifiers(&database, &file), (None, None, None));

    let mut rows = orbits_rows();
    rows[0] = ReleaseTrack {
        recording: Some(mbid(RECORDING)),
        track: Some(mbid(RELEASE_TRACK)),
        isrc: Some("GBAYE7100195".to_owned()),
        ..rows[0].clone()
    };
    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 3);
    assert_eq!(
        identifiers(&database, &file),
        (
            Some(RECORDING.to_owned()),
            Some(RELEASE_TRACK.to_owned()),
            Some("GBAYE7100195".to_owned())
        )
    );
    assert_eq!(
        identifiers(&database, &tree.path().join("2.wav")),
        (None, None, None)
    );
    Ok(())
}

#[test]
fn an_album_whose_owner_holds_an_id_is_searched_for_by_that_id_and_agrees_by_it() -> Result<()> {
    let tree = Tree::new();
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        tree.write(
            &format!("Orbits/{number}.wav"),
            &orbits_track(title, number)
                .described(MUSICBRAINZ_ARTIST, ORBITERS)
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ORBITERS))
    );

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![ReleaseMatch {
            credit: credited(Some("Orbiters"), Some(ORBITERS)),
            ..orbits_match(100, None, Some(3))
        }],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(ReleaseAsked {
                artist_mbid: Some(mbid(ORBITERS)),
                ..orbits_asked()
            }),
            Called::Release(mbid(RELEASE)),
            Called::Artist(mbid(ORBITERS)),
            Called::ReleaseGroupsOf(mbid(ORBITERS)),
        ]
    );
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(only_album(&library)?.mbid, Some(mbid(RELEASE)));
    Ok(())
}

#[test]
fn a_track_the_album_pass_already_paired_is_never_asked_about() -> Result<()> {
    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 3);
    assert_eq!(summary.stats.tracks, 0);
    assert_eq!(fake.called(LookupOp::FindRecording), 0);
    assert_eq!(fake.called(LookupOp::Isrc), 0);

    let tree = Tree::new();
    write_orbits(&tree, true);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let two_of_the_three: Vec<ReleaseTrack> = orbits_rows().into_iter().take(2).collect();
    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(two_of_the_three, Vec::new())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.matched, 2);
    assert_eq!(summary.stats.tracks, 1);
    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    let asked = RecordingAsked {
        title: "Fearless".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        release: None,
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };
    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    Ok(())
}

#[test]
fn a_search_match_leaves_the_title_the_file_carried_and_writes_the_identifiers_alone() -> Result<()>
{
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days (2011 Remaster)",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(fake.called(LookupOp::Recording), 1);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.named, 0);

    let held = stored(&database, &file);
    assert_eq!(held.title, "One of These Days");
    assert_eq!(held.tagged_title.as_deref(), Some("One of These Days"));
    assert_eq!(held.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert!(held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    Ok(())
}

#[test]
fn a_whole_rescan_keeps_the_recording_a_lookup_identified_a_file_by_until_it_is_retagged()
-> Result<()> {
    let (tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    fs::write(
        &file,
        Wav::new()
            .text(TITLE, "Fearless")
            .text(ARTIST, "The Orbiters")
            .build(),
    )
    .expect("a writable temporary file");
    scan(&library, &options(&tree))?;

    assert_eq!(recorded(&database, &file), None);
    Ok(())
}

#[test]
fn a_track_the_phrase_answered_nothing_for_is_searched_again_in_words() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings_in_words: vec![recording_match(RECORDING, "One of These Days")],
        recordings: vec![orbits_recording(
            RECORDING,
            "One of These Days",
            CODE,
            on_orbits(1),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    let asked = RecordingAsked {
        title: "One of These Days".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        release: None,
        length: Some(THE_FILES_LENGTH),
        wording: Wording::Phrase,
    };
    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindRecording)
        .collect();
    assert_eq!(
        searched,
        vec![
            Called::FindRecording(asked.clone()),
            Called::FindRecording(RecordingAsked {
                wording: Wording::Words,
                ..asked
            }),
        ]
    );
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));
    assert!(stored(&database, &file).answered.is_some());
    Ok(())
}

#[test]
fn a_recording_the_search_placed_is_landed_without_asking_for_it_again() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![RecordingMatch {
            releases: on_orbits(4),
            ..recording_match(RECORDING, "One of These Days")
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        0,
        "a search answer that says where the recording sits was asked for a second time"
    );
    assert_eq!(summary.stats.tracks, 1);

    let held = stored(&database, &file);
    assert_eq!(held.release_title.as_deref(), Some("Orbits"));
    assert_eq!(
        held.isrc, None,
        "the answer named no code, so none was to be written"
    );
    assert!(held.answered.is_some());
    assert_eq!(recorded(&database, &file).as_deref(), Some(RECORDING));

    let tracks = all(&library)?;
    assert_eq!(tracks[0].track_number, Some(4));
    assert_eq!(tracks[0].disc_number, Some(1));
    Ok(())
}

#[test]
fn a_code_the_search_answer_named_is_written_without_asking_for_the_recording_again() -> Result<()>
{
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters"),
    )?;
    let fake = Arc::new(Fake::new(Canned {
        found_recordings: vec![RecordingMatch {
            releases: on_orbits(4),
            isrcs: vec![isrc(CODE)],
            ..recording_match(RECORDING, "One of These Days")
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::FindRecording), 1);
    assert_eq!(
        fake.called(LookupOp::Recording),
        0,
        "the code was in the search answer, so nothing was to be asked twice"
    );
    assert_eq!(
        stored(&database, &file).isrc.as_deref(),
        Some("GBAYE7100195")
    );
    Ok(())
}

#[test]
fn an_exact_identification_corrects_a_title_the_file_carried() -> Result<()> {
    let tree = Tree::new();
    let carried = tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "one of these days")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE)
            .build(),
    );
    let bare = tree.write("2.wav", &Wav::new().text(ISRC, ANOTHER_CODE).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    assert_eq!(stored(&database, &bare).title, "2");

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![
            (
                isrc(CODE),
                orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
            ),
            (
                isrc(ANOTHER_CODE),
                orbits_recording(
                    ANOTHER_RECORDING,
                    "A Pillow of Winds",
                    ANOTHER_CODE,
                    on_orbits(2),
                ),
            ),
        ],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.tracks, 2);
    assert_eq!(summary.stats.named, 2);

    let corrected = stored(&database, &carried);
    assert_eq!(corrected.title, "One of These Days");
    assert_eq!(corrected.tagged_title.as_deref(), Some("one of these days"));
    assert_eq!(corrected.artist.as_deref(), Some("The Orbiters"));

    let filled = stored(&database, &bare);
    assert_eq!(filled.title, "A Pillow of Winds");
    assert_eq!(filled.tagged_title, None);
    assert_eq!(filled.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(filled.tagged_artist, None);
    Ok(())
}

#[test]
fn a_track_whose_album_never_answered_lands_the_release_its_recording_names() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "One of These Days")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(ISRC, CODE),
    )?;
    assert_eq!(only_album(&library)?.mbid, None);

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(orbits_rows(), Vec::new())],
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "One of These Days", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        fake.called(LookupOp::FindRelease),
        2,
        "the album's near miss was not asked again in words"
    );
    assert_eq!(fake.called(LookupOp::Release), 1);
    assert_eq!(summary.stats.releases, 1);
    assert_eq!(summary.stats.tracks, 1);
    assert_eq!(summary.stats.matched, 1);

    let album = only_album(&library)?;
    assert_eq!(album.mbid, Some(mbid(RELEASE)));
    assert_eq!(album.missing, 2);
    assert_eq!(library.release_tracks(album.id)?.len(), 3);
    let release = library.release_of(album.id)?.expect("the album is known");
    assert!(release.answered.is_some());
    assert_eq!(
        stored(&database, &file).release_title.as_deref(),
        Some("Orbits")
    );
    Ok(())
}

#[test]
fn a_corrected_title_is_what_the_search_index_finds_the_track_by() -> Result<()> {
    let (_tree, library, ..) = scanned_lone(
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, CODE),
    )?;
    assert_eq!(titles(&library.search("Echos", 10)?.tracks), vec!["Echos"]);

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "Echoes", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    let summary = enrich(&library, &fake, false)?;
    assert_eq!(summary.stats.named, 1);

    assert_eq!(
        titles(&library.search("Echoes", 10)?.tracks),
        vec!["Echoes"]
    );
    assert!(
        library.search("Echos", 10)?.tracks.is_empty(),
        "the title the file carried still finds the row the enrichment renamed"
    );
    Ok(())
}

#[test]
fn a_rescan_indexes_and_bills_the_names_the_lookup_kept() -> Result<()> {
    let tagged = || {
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "Orbiterz")
            .text(ISRC, CODE)
    };
    let (tree, library, ..) = scanned_lone(tagged())?;

    let fake = Arc::new(Fake::new(Canned {
        isrcs: vec![(
            isrc(CODE),
            orbits_recording(RECORDING, "Echoes", CODE, on_orbits(1)),
        )],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    assert_eq!(enrich(&library, &fake, false)?.stats.named, 1);
    let answered = library.search("Echoes", 10)?.tracks;
    assert_eq!(titles(&answered), vec!["Echoes"]);

    tree.write("1.wav", &tagged().build());
    assert_eq!(scan(&library, &options(&tree))?.updated, 1);

    let rescanned = library.search("Echoes", 10)?.tracks;
    assert_eq!(
        titles(&rescanned),
        vec!["Echoes"],
        "a rescan indexed the file's title over the one the row still shows"
    );
    assert!(library.search("Echos", 10)?.tracks.is_empty());
    assert_eq!(rescanned[0].artist.as_deref(), Some("The Orbiters"));
    assert_eq!(
        rescanned[0].artist_id, answered[0].artist_id,
        "a rescan pointed the row back at the artist the file names"
    );
    Ok(())
}

fn wanted_san_tropez(library: &Library) -> Result<WantId> {
    let album = only_album(library)?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    library.want(missing.id)
}

#[test]
fn the_stub_provider_delivers_nothing_and_a_poll_stamps_the_try() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let want = wanted_san_tropez(&library)?;

    let providers = Arc::new(Providers::none());
    assert!(!providers.has_a_source());
    assert_eq!(
        providers.names(),
        vec![SourceId::new("unprovided").expect("a nameable source")]
    );
    assert_eq!(library.last_tried()?, None);

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;
    assert!(!summary.cancelled);
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.nothing, 1);
    assert_eq!(summary.stats.offered, 0);
    assert_eq!(summary.stats.refused, 0);

    let wants = library.wants()?;
    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert!(wants[0].tried.is_some());
    assert_eq!(wants[0].offered, None);
    assert_eq!(library.last_tried()?, wants[0].tried);
    Ok(())
}

#[test]
fn a_file_delivered_with_no_vault_is_written_as_the_offer() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    wanted_san_tropez(&library)?;
    let delivered = tree.path().join("inbox").join("san-tropez.flac");
    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered.clone())));
    let providers = inbox.registered();
    assert!(providers.has_a_source());
    assert_eq!(
        providers.names(),
        vec![
            SourceId::new("unprovided").expect("a nameable source"),
            SourceId::new("inbox")?
        ]
    );

    let summary = library.poll(providers, PollOptions::default())?.join()?;
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert_eq!(summary.stats.nothing, 0);
    assert_eq!(inbox.asked().len(), 1);

    let wants = library.wants()?;
    assert_eq!(
        wants[0].offered,
        Some(MediaLocation::local(&delivered).to_uri())
    );
    assert!(wants[0].tried.is_some());
    Ok(())
}

#[test]
fn a_provider_is_handed_the_identity_and_the_service_links_the_catalog_holds() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();
    rows.push(ReleaseTrack {
        recording: Some(mbid(ORBITERS)),
        track: Some(mbid(RELEASE_TRACK)),
        isrc: Some("GBN9Y1100089".to_owned()),
        ..release_row(
            4,
            "San Tropez",
            vec![Link::new(
                "free streaming",
                "https://tidal.com/track/55391743".to_owned(),
            )],
        )
    });
    library.land_release(
        album.id,
        &orbits(
            rows,
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;
    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    library.want(missing.id)?;

    let asked = Arc::new(Offering::new("asked", Delivering::Nothing));
    library
        .poll(asked.registered(), PollOptions::default())?
        .join()?;

    let identity = asked.asked().pop().expect("the want was asked about");
    assert_eq!(identity.title, "San Tropez");
    assert_eq!(identity.album.as_deref(), Some("Orbits"));
    assert_eq!(identity.recording, Some(mbid(ORBITERS)));
    assert_eq!(identity.track, Some(mbid(RELEASE_TRACK)));
    assert_eq!(identity.release, Some(mbid(RELEASE)));
    assert_eq!(identity.isrc, Some(Isrc::new("GBN9Y1100089")?));
    assert_eq!(identity.length, Some(Duration::from_millis(4_000)));
    assert_eq!(identity.disc, Some(1));
    assert_eq!(identity.position, Some(4));
    assert_eq!(
        identity.track_on(Service::Tidal),
        Some("https://tidal.com/track/55391743")
    );
    assert_eq!(
        identity.release_on(Service::Tidal),
        Some("https://tidal.com/album/55391740")
    );
    Ok(())
}

#[test]
fn a_want_tried_lately_is_not_asked_again_until_the_window_passes() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    wanted_san_tropez(&library)?;
    let quiet = Arc::new(Offering::new("quiet", Delivering::Nothing));
    let providers = quiet.registered();

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(quiet.asked().len(), 1);

    let summary = library
        .poll(Arc::clone(&providers), PollOptions::default())?
        .join()?;
    assert_eq!(summary.stats.asked, 0);
    assert_eq!(quiet.asked().len(), 1);

    let summary = library
        .poll(
            providers,
            PollOptions {
                again_after: Duration::ZERO,
                ..PollOptions::default()
            },
        )?
        .join()?;
    assert_eq!(summary.stats.asked, 1);
    assert_eq!(quiet.asked().len(), 2);
    Ok(())
}

#[test]
fn a_catalog_nothing_has_played_has_nothing_to_resume() -> Result<()> {
    let library = Library::open_in_memory()?;

    assert_eq!(library.resumption()?, None);
    Ok(())
}

#[test]
fn a_kept_queue_comes_back_as_it_was_left() -> Result<()> {
    let library = Library::open_in_memory()?;
    let cut = FrameSpan::between(Frames(44_100), Frames(88_200));
    let kept = Resumption {
        rows: vec![
            resumable("/music/first.flac"),
            Resumable {
                location: MediaLocation::local("/music/whole.flac"),
                span: Some(cut),
            },
        ],
        order: vec![0, 1],
        row: 1,
        at: Frames(12_345),
        shuffle: false,
    };

    library.keep_resumption(&kept)?;
    assert_eq!(library.resumption()?, Some(kept));
    Ok(())
}

#[test]
fn a_shuffled_queue_comes_back_saying_where_each_row_was_loaded() -> Result<()> {
    let library = Library::open_in_memory()?;
    let kept = Resumption {
        rows: vec![
            resumable("/music/first.flac"),
            resumable("/music/second.flac"),
            resumable("/music/third.flac"),
        ],
        order: vec![2, 0, 1],
        row: 1,
        at: Frames(4_410),
        shuffle: true,
    };

    library.keep_resumption(&kept)?;
    assert_eq!(library.resumption()?, Some(kept));
    Ok(())
}

#[test]
fn a_row_from_a_source_that_is_not_the_filesystem_is_kept_by_its_uri() -> Result<()> {
    let library = Library::open_in_memory()?;
    let elsewhere = MediaLocation::new(SourceId::new("subsonic")?, "track/1.flac");
    let kept = Resumption {
        rows: vec![Resumable {
            location: elsewhere.clone(),
            span: None,
        }],
        order: vec![0],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
    };

    library.keep_resumption(&kept)?;
    assert_eq!(
        library.resumption()?.map(|kept| kept.rows),
        Some(vec![Resumable {
            location: elsewhere,
            span: None,
        }])
    );
    Ok(())
}

#[test]
fn keeping_the_place_moves_it_without_rewriting_the_rows_or_the_shuffle() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows = vec![
        resumable("/music/first.flac"),
        resumable("/music/second.flac"),
    ];

    library.keep_resumption(&Resumption {
        rows: rows.clone(),
        order: vec![1, 0],
        row: 0,
        at: Frames::ZERO,
        shuffle: true,
    })?;
    library.keep_place(1, Frames(999))?;

    assert_eq!(
        library.resumption()?,
        Some(Resumption {
            rows,
            order: vec![1, 0],
            row: 1,
            at: Frames(999),
            shuffle: true,
        })
    );
    Ok(())
}

#[test]
fn an_order_kept_on_its_own_leaves_the_rows_where_they_were() -> Result<()> {
    let library = Library::open_in_memory()?;
    let rows = vec![
        resumable("/music/first.flac"),
        resumable("/music/second.flac"),
        resumable("/music/third.flac"),
    ];
    library.keep_resumption(&Resumption {
        rows: rows.clone(),
        order: vec![0, 1, 2],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
    })?;

    library.keep_order(&Reordered {
        order: vec![2, 0, 1],
        row: 1,
        at: Frames(4_410),
        shuffle: true,
    })?;

    assert_eq!(
        library.resumption()?,
        Some(Resumption {
            rows,
            order: vec![2, 0, 1],
            row: 1,
            at: Frames(4_410),
            shuffle: true,
        }),
        "keeping the order alone did not leave the rows as they were"
    );
    Ok(())
}

#[test]
fn a_queue_kept_again_replaces_the_rows_rather_than_adding_to_them() -> Result<()> {
    let library = Library::open_in_memory()?;
    let longer = Resumption {
        rows: (0..3)
            .map(|number| resumable(&format!("/music/{number}.flac")))
            .collect(),
        order: vec![0, 1, 2],
        row: 2,
        at: Frames::ZERO,
        shuffle: true,
    };
    let shorter = Resumption {
        rows: vec![resumable("/music/only.flac")],
        order: vec![0],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
    };

    library.keep_resumption(&longer)?;
    library.keep_resumption(&shorter)?;
    assert_eq!(library.resumption()?, Some(shorter));
    Ok(())
}

#[test]
fn a_queue_kept_as_nothing_leaves_nothing_to_resume() -> Result<()> {
    let library = Library::open_in_memory()?;

    library.keep_resumption(&Resumption {
        rows: vec![resumable("/music/only.flac")],
        order: vec![0],
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
    })?;
    library.keep_resumption(&Resumption {
        rows: Vec::new(),
        order: Vec::new(),
        row: 0,
        at: Frames::ZERO,
        shuffle: false,
    })?;

    assert_eq!(library.resumption()?, None);
    Ok(())
}

#[test]
fn forgetting_a_kept_queue_takes_the_place_with_it() -> Result<()> {
    let library = Library::open_in_memory()?;

    library.keep_resumption(&Resumption {
        rows: vec![resumable("/music/only.flac")],
        order: vec![0],
        row: 0,
        at: Frames(500),
        shuffle: false,
    })?;
    library.forget_resumption()?;

    assert_eq!(library.resumption()?, None);
    library.forget_resumption()?;
    Ok(())
}

fn filed_under(tree: &Tree) -> PathBuf {
    tree.path()
        .canonicalize()
        .expect("the tree the scan walked is still there")
}

fn previewed(library: &Library) -> Result<OrganiseSummary> {
    let summary = library.organise(OrganiseOptions::default())?.join()?;
    assert!(!summary.cancelled, "the preview reported itself cancelled");
    assert_eq!(summary.stats.moved, 0, "a preview reported a file moved");
    assert_eq!(
        summary.stats.pruned, 0,
        "a preview reported a folder pruned"
    );
    Ok(summary)
}

fn nothing_moved(summary: &OrganiseSummary) {
    for planned in &summary.plan.moves {
        assert!(
            planned.from.exists(),
            "{} left where it stood",
            planned.from.display()
        );
        assert!(
            !planned.to.exists(),
            "{} was made by a preview",
            planned.to.display()
        );
        for sidecar in &planned.sidecars {
            assert!(
                sidecar.from.exists(),
                "{} left where it stood",
                sidecar.from.display()
            );
            assert!(
                !sidecar.to.exists(),
                "{} was made by a preview",
                sidecar.to.display()
            );
        }
    }
    for refused in &summary.plan.refused {
        assert!(
            refused.from.exists(),
            "{} left where it stood",
            refused.from.display()
        );
    }
    for folder in &summary.plan.folders {
        assert!(folder.exists(), "{} was pruned", folder.display());
    }
}

fn meddle(title: &str, number: &str) -> Vec<u8> {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, "Pink Floyd")
        .text(ALBUM_ARTIST, "Pink Floyd")
        .text(ALBUM, "Meddle")
        .text(TRACK, number)
        .build()
}

fn landings(summary: &OrganiseSummary) -> Vec<(PathBuf, PathBuf)> {
    summary
        .plan
        .moves
        .iter()
        .map(|planned| (planned.from.clone(), planned.to.clone()))
        .collect()
}

#[test]
fn a_preview_moves_nothing_and_names_every_file_it_would_move() -> Result<()> {
    let tree = Tree::new();
    tree.write("1.wav", &meddle("One of These Days", "1"));
    tree.write("2.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    let root = filed_under(&tree);
    assert_eq!(
        landings(&summary),
        vec![
            (
                root.join("1.wav"),
                root.join("Pink Floyd/Meddle/01 One of These Days.wav")
            ),
            (
                root.join("2.wav"),
                root.join("Pink Floyd/Meddle/02 Echoes.wav")
            ),
        ]
    );
    assert!(summary.plan.refused.is_empty());
    assert_eq!(summary.plan.unchanged, 0);
    nothing_moved(&summary);
    Ok(())
}

#[test]
fn a_track_already_where_the_layout_puts_it_is_counted_unchanged_and_never_moved() -> Result<()> {
    let tree = Tree::new();
    let held = tree.write(
        "Pink Floyd/Meddle/01 One of These Days.wav",
        &meddle("One of These Days", "1"),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    assert!(summary.plan.moves.is_empty());
    assert!(summary.plan.refused.is_empty());
    assert_eq!(summary.plan.unchanged, 1);
    assert_eq!(summary.stats.unchanged, 1);
    assert!(summary.plan.folders.is_empty());
    assert!(held.exists(), "the file the layout already names moved");
    nothing_moved(&summary);
    Ok(())
}

#[test]
fn a_set_filed_as_disc_folders_is_planned_into_one_album_folder_with_the_disc_in_each_name()
-> Result<()> {
    let tree = Tree::new();
    tree.write(
        "The Wall/CD1/a.wav",
        &Wav::new()
            .text(TITLE, "In the Flesh?")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .text(ALBUM, "The Wall")
            .text(TRACK, "1")
            .build(),
    );
    tree.write(
        "The Wall/CD2/b.wav",
        &Wav::new()
            .text(TITLE, "Hey You")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .text(ALBUM, "The Wall")
            .text(TRACK, "1")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    let root = filed_under(&tree);
    assert_eq!(
        landings(&summary),
        vec![
            (
                root.join("The Wall/CD1/a.wav"),
                root.join("Pink Floyd/The Wall/1-01 In the Flesh?.wav")
            ),
            (
                root.join("The Wall/CD2/b.wav"),
                root.join("Pink Floyd/The Wall/2-01 Hey You.wav")
            ),
        ]
    );
    assert_eq!(
        summary.plan.folders,
        vec![root.join("The Wall/CD1"), root.join("The Wall/CD2")]
    );
    assert!(summary.plan.refused.is_empty());
    nothing_moved(&summary);
    Ok(())
}

#[test]
fn two_tracks_naming_one_destination_leave_the_first_moving_and_the_second_collided() -> Result<()>
{
    let tree = Tree::new();
    tree.write("a.wav", &meddle("Echoes", "6"));
    tree.write("b.wav", &meddle("Echoes", "6"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    let root = filed_under(&tree);
    assert_eq!(
        landings(&summary),
        vec![(
            root.join("a.wav"),
            root.join("Pink Floyd/Meddle/06 Echoes.wav")
        )]
    );
    assert_eq!(
        summary.plan.refused,
        vec![Refused {
            from: root.join("b.wav"),
            refusal: Refusal::Collided {
                with: root.join("a.wav")
            },
        }]
    );
    assert_eq!(summary.stats.collided, 1);
    nothing_moved(&summary);
    Ok(())
}

#[test]
fn a_track_with_nothing_to_file_it_under_is_counted_unidentified() -> Result<()> {
    let tree = Tree::new();
    tree.write("loose.wav", &Wav::new().text(TITLE, "Stray").build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    let root = filed_under(&tree);
    assert!(summary.plan.moves.is_empty());
    assert_eq!(
        summary.plan.refused,
        vec![Refused {
            from: root.join("loose.wav"),
            refusal: Refusal::Loose,
        }]
    );
    assert_eq!(summary.stats.unidentified, 1);
    nothing_moved(&summary);
    Ok(())
}

#[test]
fn a_cue_cut_file_keeps_its_own_name_and_is_planned_by_its_rows_folder() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let summary = previewed(&library)?;
    let root = filed_under(&tree);

    assert_eq!(summary.plan.moves.len(), 1);
    let planned = &summary.plan.moves[0];
    assert_eq!(planned.from, root.join("Meddle.wav"));
    assert_eq!(planned.rows, 3);
    assert_eq!(
        planned.to.file_name(),
        Some(OsStr::new("Meddle.wav")),
        "a file its rows disagree about was renamed after one of them"
    );
    assert_eq!(
        planned.to.parent().and_then(Path::file_name),
        Some(OsStr::new("Meddle")),
        "the rows did not file the file they were cut out of under their album"
    );
    assert_eq!(
        planned.sidecars,
        vec![Sidecar {
            from: root.join("Meddle.cue"),
            to: planned.to.with_file_name("Meddle.cue"),
        }]
    );
    assert!(summary.plan.refused.is_empty());
    nothing_moved(&summary);
    Ok(())
}

fn applied(library: &Library) -> Result<OrganiseSummary> {
    let summary = library
        .organise(OrganiseOptions {
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert!(!summary.cancelled, "the run reported itself cancelled");
    assert_eq!(
        summary.stats.moved as usize,
        summary.plan.files_moving(),
        "the run counted moves it does not name"
    );
    for planned in &summary.plan.moves {
        for (from, to) in planned.files() {
            assert!(to.exists(), "{} did not land", to.display());
            assert!(!from.exists(), "{} is still where it stood", from.display());
        }
        for sidecar in &planned.sidecars {
            assert!(sidecar.to.exists(), "{} did not land", sidecar.to.display());
        }
    }
    Ok(summary)
}

const ECHOES_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.wav" WAVE
  TRACK 01 AUDIO
    TITLE "Echoes"
    INDEX 01 00:00:00
"#;

#[test]
fn a_sheet_that_cuts_one_track_out_of_a_file_names_it_by_the_name_the_layout_gave_it() -> Result<()>
{
    let tree = Tree::new();
    tree.write("Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("Meddle.cue", ECHOES_SHEET.as_bytes());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = applied(&library)?;
    let root = filed_under(&tree);

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(
        landings(&summary),
        vec![(
            root.join("Meddle.wav"),
            root.join("Pink Floyd/Meddle/01 Echoes.wav")
        )]
    );
    assert_eq!(
        summary.plan.moves[0].sidecars,
        vec![Sidecar {
            from: root.join("Meddle.cue"),
            to: root.join("Pink Floyd/Meddle/01 Echoes.cue"),
        }]
    );

    let written = fs::read_to_string(root.join("Pink Floyd/Meddle/01 Echoes.cue"))
        .expect("the sheet travelled with the file it cuts");
    assert_eq!(
        written,
        ECHOES_SHEET.replace("\"Meddle.wav\"", "\"01 Echoes.wav\""),
        "the sheet still names the file by the name it no longer has"
    );
    Ok(())
}

#[test]
fn a_sheet_that_cuts_a_file_into_several_rows_keeps_naming_the_name_it_kept() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let summary = applied(&library)?;
    let root = filed_under(&tree);

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(
        fs::read_to_string(root.join("Pink Floyd/Meddle/Meddle.cue"))
            .expect("the sheet travelled with the file it cuts"),
        MEDDLE_SHEET,
        "a sheet beside a file that kept its name was rewritten"
    );
    Ok(())
}

#[test]
fn a_sheet_named_apart_from_the_file_it_cuts_travels_with_it_and_the_rows_survive_a_rescan()
-> Result<()> {
    let tree = Tree::new();
    tree.write("CDImage.wav", &Wav::new().frames(44_100).build());
    tree.write(
        "Meddle.cue",
        MEDDLE_SHEET
            .replace("\"Meddle.wav\"", "\"CDImage.wav\"")
            .as_bytes(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let rows = all(&library)?;
    assert_eq!(rows.len(), 3);
    for row in &rows {
        library.track_played(&row.location, row.span)?;
    }

    let summary = applied(&library)?;
    let root = filed_under(&tree);
    assert_eq!(summary.stats.moved, 1);
    assert_eq!(
        summary.plan.moves[0].sidecars,
        vec![Sidecar {
            from: root.join("Meddle.cue"),
            to: root.join("Pink Floyd/Meddle/Meddle.cue"),
        }]
    );

    scan(&library, &options(&tree))?;
    let after = all(&library)?;
    assert_eq!(
        after.len(),
        3,
        "the rescan read the file whole and pruned its rows"
    );
    assert!(
        after.iter().all(|row| row.plays == 1),
        "a play went with a pruned row"
    );
    Ok(())
}

const MEDDLE_BY_FILE_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "one.wav" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
FILE "two.wav" WAVE
  TRACK 02 AUDIO
    TITLE "Echoes"
    INDEX 01 00:00:00
"#;

#[test]
fn the_files_a_sheet_names_one_each_are_filed_together_with_the_sheet_beside_them() -> Result<()> {
    let tree = Tree::new();
    tree.write("rip/one.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/two.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_BY_FILE_SHEET.as_bytes());
    tree.write("rip/one.log", b"ripped");
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let root = filed_under(&tree);

    let preview = previewed(&library)?;
    assert!(
        preview.plan.refused.is_empty(),
        "{:?}",
        preview.plan.refused
    );
    assert_eq!(
        preview.plan.moves.len(),
        1,
        "the files one sheet names were planned apart"
    );
    assert_eq!(preview.plan.files_moving(), 2);
    let album = root.join("Pink Floyd/Meddle");
    assert_eq!(
        preview.plan.moves[0]
            .files()
            .map(|(from, to)| (from.to_path_buf(), to.to_path_buf()))
            .collect::<Vec<_>>(),
        vec![
            (
                root.join("rip/one.wav"),
                album.join("01 One of These Days.wav")
            ),
            (root.join("rip/two.wav"), album.join("02 Echoes.wav")),
        ]
    );
    assert_eq!(
        preview.plan.moves[0].sidecars,
        vec![
            Sidecar {
                from: root.join("rip/Meddle.cue"),
                to: album.join("Meddle.cue"),
            },
            Sidecar {
                from: root.join("rip/one.log"),
                to: album.join("01 One of These Days.log"),
            },
        ]
    );
    assert_eq!(preview.plan.folders, vec![root.join("rip")]);
    nothing_moved(&preview);

    let summary = applied(&library)?;
    assert_eq!(summary.stats.moved, 2);
    assert!(!root.join("rip").exists(), "the emptied folder was left");
    let written =
        fs::read_to_string(album.join("Meddle.cue")).expect("the sheet travelled beside its files");
    assert_eq!(
        written,
        MEDDLE_BY_FILE_SHEET
            .replace("\"one.wav\"", "\"01 One of These Days.wav\"")
            .replace("\"two.wav\"", "\"02 Echoes.wav\""),
        "the sheet still names a file by the name it no longer has"
    );
    let held: Vec<PathBuf> = all(&library)?
        .into_iter()
        .filter_map(|track| track.location.as_path().map(Path::to_path_buf))
        .collect();
    assert_eq!(
        held.len(),
        2,
        "the catalog did not follow both files: {held:?}"
    );
    assert!(held.iter().all(|path| path.starts_with(&album)), "{held:?}");
    Ok(())
}

#[test]
fn the_files_a_sheet_names_wait_for_a_file_standing_where_one_lands_to_move_on_first() -> Result<()>
{
    let tree = Tree::new();
    tree.write("rip/one.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/two.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_BY_FILE_SHEET.as_bytes());
    tree.write(
        "Pink Floyd/Meddle/01 One of These Days.wav",
        &meddle("Fearless", "3"),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let root = filed_under(&tree);
    let album = root.join("Pink Floyd/Meddle");

    let preview = previewed(&library)?;
    assert!(
        preview.plan.refused.is_empty(),
        "{:?}",
        preview.plan.refused
    );
    assert_eq!(
        landings(&preview)
            .into_iter()
            .map(|(from, _)| from)
            .collect::<Vec<_>>(),
        vec![
            album.join("01 One of These Days.wav"),
            root.join("rip/one.wav"),
        ],
        "the file standing where the sheet's first file lands was not moved on first"
    );
    assert_eq!(preview.plan.files_moving(), 3);

    let summary = library
        .organise(OrganiseOptions {
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;
    assert_eq!(summary.stats.moved, 3);
    assert_eq!(summary.stats.failed, 0);
    assert!(!root.join("rip").exists(), "the emptied folder was left");
    assert!(album.join("03 Fearless.wav").is_file());
    assert!(album.join("01 One of These Days.wav").is_file());
    assert!(album.join("02 Echoes.wav").is_file());
    assert!(album.join("Meddle.cue").is_file());
    Ok(())
}

#[test]
fn the_files_a_sheet_names_wait_in_vain_on_a_file_that_stays_and_each_says_what_it_met()
-> Result<()> {
    let tree = Tree::new();
    tree.write("rip/one.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/two.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_BY_FILE_SHEET.as_bytes());
    tree.write("Pink Floyd/Meddle/02 Echoes.wav", &meddle("Echoes", "2"));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let root = filed_under(&tree);
    let held = root.join("Pink Floyd/Meddle/02 Echoes.wav");

    let preview = previewed(&library)?;

    assert!(preview.plan.moves.is_empty(), "{:?}", preview.plan.moves);
    assert_eq!(preview.plan.unchanged, 1);
    let mut refused: Vec<Refused> = preview.plan.refused.clone();
    refused.sort_by(|one, other| one.from.cmp(&other.from));
    assert_eq!(
        refused,
        ["rip/one.wav", "rip/two.wav"]
            .iter()
            .map(|file| Refused {
                from: root.join(file),
                refusal: Refusal::Collided { with: held.clone() },
            })
            .collect::<Vec<_>>()
    );
    assert_eq!(preview.plan.folders, Vec::<PathBuf>::new());
    Ok(())
}

#[test]
fn a_file_a_sheet_names_beside_one_nothing_scanned_is_left_where_it_stands() -> Result<()> {
    let tree = Tree::new();
    tree.write("one.wav", &Wav::new().frames(44_100).build());
    tree.write("two.wav", &Wav::new().frames(44_100).build());
    tree.write("three.raw", b"not audio this build scans");
    tree.write(
        "Meddle.cue",
        format!(
            "{MEDDLE_BY_FILE_SHEET}FILE \"three.raw\" BINARY\n  TRACK 03 AUDIO\n    TITLE \"Fearless\"\n    INDEX 01 00:00:00\n"
        )
        .as_bytes(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = previewed(&library)?;
    let root = filed_under(&tree);
    assert!(summary.plan.moves.is_empty());
    let mut refused: Vec<Refused> = summary.plan.refused.clone();
    refused.sort_by(|one, other| one.from.cmp(&other.from));
    assert_eq!(
        refused,
        ["one.wav", "two.wav"]
            .iter()
            .map(|file| Refused {
                from: root.join(file),
                refusal: Refusal::SharesASheet {
                    sheet: root.join("Meddle.cue"),
                },
            })
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn naming_a_root_files_the_tracks_scanned_from_it_and_leaves_the_others_where_they_stand()
-> Result<()> {
    let one = Tree::new();
    one.write("here.wav", &meddle("Echoes", "1"));
    let other = Tree::new();
    other.write("there.wav", &meddle("Fearless", "2"));

    let library = Library::open_in_memory()?;
    scan(
        &library,
        &ScanOptions {
            roots: vec![one.path().to_path_buf(), other.path().to_path_buf()],
            ..options(&one)
        },
    )?;

    let named = filed_under(&one);
    let summary = library
        .organise(OrganiseOptions {
            roots: vec![named.clone()],
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(
        landings(&summary),
        vec![(
            named.join("here.wav"),
            named.join("Pink Floyd/Meddle/01 Echoes.wav")
        )]
    );
    assert!(
        filed_under(&other).join("there.wav").exists(),
        "a root the run was not given was filed anyway"
    );
    Ok(())
}

#[test]
fn a_root_the_catalog_does_not_hold_is_refused_rather_than_filing_nothing() -> Result<()> {
    let tree = Tree::new();
    tree.write("here.wav", &meddle("Echoes", "1"));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let elsewhere = env::temp_dir();
    let outcome = library
        .organise(OrganiseOptions {
            roots: vec![elsewhere.clone()],
            ..OrganiseOptions::default()
        })?
        .join();

    assert!(
        matches!(outcome, Err(Error::NotARoot { ref path }) if *path == elsewhere),
        "a root the catalog does not hold was taken as one"
    );
    Ok(())
}

fn across_the_boundary() -> Option<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let elsewhere = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "resonate-elsewhere-{}-{}",
        process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&elsewhere).expect("a writable directory beside the build");

    let here = fs::metadata(env::temp_dir()).expect("a temporary directory to move out of");
    let there = fs::metadata(&elsewhere).expect("the directory just made");
    if here.dev() == there.dev() {
        let _ = fs::remove_dir_all(&elsewhere);
        return None;
    }

    Some(elsewhere)
}

#[test]
fn a_track_whose_destination_is_on_another_filesystem_is_copied_over_and_taken_away() -> Result<()>
{
    let tree = Tree::new();
    let stood = tree.write("here.wav", &meddle("Echoes", "1"));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let Some(elsewhere) = across_the_boundary() else {
        eprintln!("skipped: nothing here is on a second filesystem to move onto");
        return Ok(());
    };
    let root = filed_under(&tree);
    os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
        .expect("a link into the second filesystem");

    let held = fs::read(&stood).expect("the file the scan walked");
    let stamp = fs::metadata(&stood)
        .and_then(|standing| standing.modified())
        .expect("a filesystem that keeps a modification time");

    let summary = applied(&library)?;
    let landed = root.join("Pink Floyd/Meddle/01 Echoes.wav");

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.stats.failed, 0);
    assert_eq!(landings(&summary), vec![(stood.clone(), landed.clone())]);
    assert!(
        !stood.exists(),
        "the file it was copied from is still there"
    );
    assert_eq!(
        fs::read(&landed).expect("the file that landed on the second filesystem"),
        held,
        "the bytes that landed are not the bytes that stood"
    );
    assert_eq!(
        fs::metadata(&landed)
            .and_then(|there| there.modified())
            .ok(),
        Some(stamp),
        "a copied file carries the time it was copied at rather than the time it was written"
    );
    assert_eq!(where_the_rows_are(&library)?, vec![landed]);

    let _ = fs::remove_dir_all(&elsewhere);
    Ok(())
}

#[test]
fn a_copy_a_killed_run_left_whole_on_the_other_filesystem_is_taken_as_landed() -> Result<()> {
    let tree = Tree::new();
    let stood = tree.write("here.wav", &meddle("Echoes", "1"));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let Some(elsewhere) = across_the_boundary() else {
        eprintln!("skipped: nothing here is on a second filesystem to move onto");
        return Ok(());
    };
    let root = filed_under(&tree);
    os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
        .expect("a link into the second filesystem");
    let landed = root.join("Pink Floyd/Meddle/01 Echoes.wav");
    fs::create_dir_all(landed.parent().expect("a folder")).expect("the folder a run made");
    fs::copy(&stood, &landed).expect("the copy a killed run wrote");
    let stamp = fs::metadata(&stood)
        .and_then(|standing| standing.modified())
        .expect("a filesystem that keeps a modification time");
    fs::File::options()
        .write(true)
        .open(&landed)
        .and_then(|copy| copy.set_modified(stamp))
        .expect("the copy carries the source's time");

    let summary = applied(&library)?;

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.stats.failed, 0);
    assert!(
        summary.plan.refused.is_empty(),
        "{:?}",
        summary.plan.refused
    );
    assert!(
        !stood.exists(),
        "the file it was copied from is still there"
    );
    assert_eq!(where_the_rows_are(&library)?, vec![landed.clone()]);
    assert_eq!(
        fs::read_dir(landed.parent().expect("a folder"))
            .expect("the folder it landed in")
            .count(),
        1,
        "the copy left something beside the file it landed"
    );

    let _ = fs::remove_dir_all(&elsewhere);
    Ok(())
}

#[test]
fn a_file_of_the_same_size_and_time_but_other_bytes_is_still_in_the_way() -> Result<()> {
    let tree = Tree::new();
    let stood = tree.write("here.wav", &meddle("Echoes", "1"));
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let Some(elsewhere) = across_the_boundary() else {
        eprintln!("skipped: nothing here is on a second filesystem to move onto");
        return Ok(());
    };
    let root = filed_under(&tree);
    os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
        .expect("a link into the second filesystem");
    let landed = root.join("Pink Floyd/Meddle/01 Echoes.wav");
    fs::create_dir_all(landed.parent().expect("a folder")).expect("a folder");
    let mut other = fs::read(&stood).expect("the file the scan walked");
    let last = other.len() - 1;
    other[last] ^= 0xFF;
    fs::write(&landed, &other).expect("another file of the same size");
    let stamp = fs::metadata(&stood)
        .and_then(|standing| standing.modified())
        .expect("a modification time");
    fs::File::options()
        .write(true)
        .open(&landed)
        .and_then(|copy| copy.set_modified(stamp))
        .expect("the same time");

    let summary = applied(&library)?;

    assert_eq!(summary.stats.moved, 0);
    assert!(stood.exists());
    assert_eq!(fs::read(&landed).expect("the file in the way"), other);

    let _ = fs::remove_dir_all(&elsewhere);
    Ok(())
}

fn where_the_rows_are(library: &Library) -> Result<Vec<PathBuf>> {
    let mut rows = Vec::new();
    for track in all(library)? {
        rows.extend(library.alternatives_of(track.id)?);
        rows.push(track);
    }
    Ok(rows
        .iter()
        .filter_map(|track| track.location.as_path().map(Path::to_path_buf))
        .collect())
}

#[test]
fn organising_puts_every_track_in_its_owner_and_album_folder_and_the_catalog_names_it_where_it_landed()
-> Result<()> {
    let tree = Tree::new();
    tree.write("1.wav", &meddle("One of These Days", "1"));
    tree.write("2.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = applied(&library)?;
    let root = filed_under(&tree);
    let days = root.join("Pink Floyd/Meddle/01 One of These Days.wav");
    let echoes = root.join("Pink Floyd/Meddle/02 Echoes.wav");

    assert_eq!(summary.stats.moved, 2);
    assert_eq!(summary.stats.failed, 0);
    assert_eq!(summary.stats.collided, 0);
    assert!(summary.plan.refused.is_empty());
    assert_eq!(
        landings(&summary),
        vec![
            (root.join("1.wav"), days.clone()),
            (root.join("2.wav"), echoes.clone()),
        ]
    );

    assert!(!root.join("1.wav").exists());
    assert!(!root.join("2.wav").exists());
    assert_eq!(where_the_rows_are(&library)?, vec![echoes, days]);
    Ok(())
}

#[test]
fn a_set_filed_as_disc_folders_lands_in_one_album_folder_with_the_disc_written_into_each_name()
-> Result<()> {
    let tree = Tree::new();
    tree.write(
        "The Wall/CD1/a.wav",
        &Wav::new()
            .text(TITLE, "In the Flesh?")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .text(ALBUM, "The Wall")
            .text(TRACK, "1")
            .build(),
    );
    tree.write(
        "The Wall/CD2/b.wav",
        &Wav::new()
            .text(TITLE, "Hey You")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .text(ALBUM, "The Wall")
            .text(TRACK, "1")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = applied(&library)?;
    let root = filed_under(&tree);
    let flesh = root.join("Pink Floyd/The Wall/1-01 In the Flesh?.wav");
    let you = root.join("Pink Floyd/The Wall/2-01 Hey You.wav");

    assert_eq!(summary.stats.moved, 2);
    assert!(flesh.exists() && you.exists());
    assert_eq!(where_the_rows_are(&library)?, vec![you, flesh]);

    assert!(
        !root.join("The Wall").exists(),
        "the set's own folder outlived every disc folder under it"
    );
    assert_eq!(summary.stats.pruned, 3);
    Ok(())
}

#[test]
fn a_play_counted_against_a_track_survives_the_move_because_the_row_is_rewritten_rather_than_rescanned()
-> Result<()> {
    let tree = Tree::new();
    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let stood = root.join("loose/echoes.wav");
    let before = library
        .track_at(&stood, None)?
        .expect("the scan stored the file it walked");
    library.track_played(&before.location, None)?;

    applied(&library)?;
    let landed = root.join("Pink Floyd/Meddle/02 Echoes.wav");
    let after = library
        .track_at(&landed, None)?
        .expect("the catalog names the file where it landed");

    assert_eq!(
        after.id, before.id,
        "the row was rescanned rather than moved"
    );
    assert_eq!(after.plays, 1);
    assert_eq!(after.added, before.added);
    assert_eq!(library.track_at(&stood, None)?, None);

    let counts = scan(&library, &options(&tree))?;
    assert_eq!(counts.added, 0, "the scan read the moved file as a new one");
    assert_eq!(counts.removed, 0, "the scan pruned the row it had moved");
    let settled = library
        .track_at(&landed, None)?
        .expect("the row outlived the scan that walked past it");
    assert_eq!(settled.id, before.id);
    assert_eq!(settled.plays, 1);
    Ok(())
}

#[test]
fn a_playlist_row_a_kept_lyric_and_a_kept_queue_row_all_follow_the_file_that_moved() -> Result<()> {
    let tree = Tree::new();
    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let stood = MediaLocation::local(root.join("loose/echoes.wav"));
    let set = library.start_playlist("Set", &[Cut::whole(stood.clone())])?;
    library.keep_lyrics(&stood, None, Some("and no one sings me lullabies"), false)?;
    library.keep_resumption(&Resumption {
        rows: vec![Resumable {
            location: stood.clone(),
            span: None,
        }],
        order: vec![0],
        row: 0,
        at: Frames(4_410),
        shuffle: false,
    })?;

    applied(&library)?;
    let landed = MediaLocation::local(root.join("Pink Floyd/Meddle/02 Echoes.wav"));

    let rows = library.playlist_entries(set, None)?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].location(), &landed);
    assert!(
        rows[0].track.is_some(),
        "the playlist row no longer resolves to a track"
    );

    let kept = library
        .kept_lyrics(&landed, None)?
        .expect("the words kept against the file followed it");
    assert_eq!(kept.text.as_deref(), Some("and no one sings me lullabies"));
    assert_eq!(library.kept_lyrics(&stood, None)?, None);

    let resumed = library
        .resumption()?
        .expect("the queue kept for the next run is still there");
    assert_eq!(resumed.rows.len(), 1);
    assert_eq!(resumed.rows[0].location, landed);
    assert_eq!(resumed.at, Frames(4_410));
    Ok(())
}

#[test]
fn a_destination_another_file_already_holds_is_refused_and_both_files_stay_where_they_are()
-> Result<()> {
    let tree = Tree::new();
    tree.write("Pink Floyd/Meddle/01 Echoes.wav", &meddle("Echoes", "1"));
    tree.write("elsewhere/x.wav", &meddle("Echoes", "1"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = applied(&library)?;
    let root = filed_under(&tree);
    let held = root.join("Pink Floyd/Meddle/01 Echoes.wav");
    let standing = root.join("elsewhere/x.wav");

    assert_eq!(summary.stats.moved, 0);
    assert_eq!(summary.stats.unchanged, 1);
    assert_eq!(summary.stats.collided, 1);
    assert_eq!(
        summary.plan.refused,
        vec![Refused {
            from: standing.clone(),
            refusal: Refusal::Collided { with: held.clone() },
        }]
    );

    assert!(held.exists() && standing.exists());
    let mut where_they_are = where_the_rows_are(&library)?;
    where_they_are.sort();
    assert_eq!(where_they_are, vec![held, standing]);
    Ok(())
}

#[test]
fn a_cue_sheet_moves_with_the_file_it_cuts_and_that_file_keeps_the_name_the_sheet_calls_it()
-> Result<()> {
    let (tree, library) = scanned_sheet();
    let before = library.tracks(&TrackQuery::default())?;

    let summary = applied(&library)?;
    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.plan.moves.len(), 1);

    let landed = summary.plan.moves[0].to.clone();
    assert_eq!(summary.plan.moves[0].rows, 3);
    assert_eq!(landed.file_name(), Some(OsStr::new("Meddle.wav")));
    assert_eq!(
        landed.parent().and_then(Path::file_name),
        Some(OsStr::new("Meddle"))
    );

    let sheet = landed.with_file_name("Meddle.cue");
    assert!(sheet.exists(), "the sheet did not travel with the file");
    assert!(!tree.path().join("Meddle.wav").exists());
    assert!(!tree.path().join("Meddle.cue").exists());

    let after = library.tracks(&TrackQuery::default())?;
    assert_eq!(after.len(), 3);
    for (was, now) in before.iter().zip(&after) {
        assert_eq!(now.id, was.id, "a cue row was rescanned rather than moved");
        assert_eq!(now.span, was.span);
        assert_eq!(now.location, MediaLocation::local(&landed));
    }

    let counts = scan(&library, &options(&tree))?;
    assert_eq!(counts.added, 0, "the sheet's rows were read as new ones");
    assert_eq!(counts.removed, 0, "the sheet's rows were pruned");
    Ok(())
}

#[test]
fn a_source_folder_left_empty_is_taken_away_and_one_still_holding_a_file_is_not() -> Result<()> {
    let tree = Tree::new();
    tree.write("rips/1.wav", &meddle("One of These Days", "1"));
    tree.write("keeps/2.wav", &meddle("Echoes", "2"));
    tree.write("keeps/notes.txt", b"what the rip was made from");

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = applied(&library)?;
    let root = filed_under(&tree);

    assert_eq!(summary.stats.moved, 2);
    assert_eq!(summary.stats.pruned, 1);
    assert!(
        !root.join("rips").exists(),
        "a folder nothing is left in stayed"
    );
    assert!(
        root.join("keeps").exists(),
        "a folder still holding a file was taken away"
    );
    assert!(root.join("keeps/notes.txt").exists());
    Ok(())
}

#[test]
fn a_second_run_moves_nothing_because_every_file_is_already_where_the_layout_puts_it() -> Result<()>
{
    let tree = Tree::new();
    tree.write("rips/1.wav", &meddle("One of These Days", "1"));
    tree.write("rips/2.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let first = applied(&library)?;
    assert_eq!(first.stats.moved, 2);
    let landed = where_the_rows_are(&library)?;

    let again = applied(&library)?;
    assert_eq!(again.stats.moved, 0, "a settled library was moved again");
    assert_eq!(again.stats.unchanged, 2);
    assert_eq!(again.stats.pruned, 0);
    assert_eq!(again.stats.failed, 0);
    assert_eq!(again.stats.collided, 0);
    assert!(again.plan.moves.is_empty());
    assert!(again.plan.refused.is_empty());
    assert!(again.plan.folders.is_empty());
    assert_eq!(where_the_rows_are(&library)?, landed);
    for path in &landed {
        assert!(path.exists(), "{} left on the second run", path.display());
    }
    Ok(())
}

#[test]
fn a_file_that_has_gone_since_the_scan_is_counted_failed_and_the_rest_of_the_run_carries_on()
-> Result<()> {
    let tree = Tree::new();
    let gone = tree.write("1.wav", &meddle("One of These Days", "1"));
    tree.write("2.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    fs::remove_file(&gone).expect("the file goes away");

    let summary = applied(&library)?;
    let root = filed_under(&tree);
    let echoes = root.join("Pink Floyd/Meddle/02 Echoes.wav");

    assert_eq!(summary.stats.failed, 1);
    assert_eq!(summary.stats.moved, 1);
    assert_eq!(
        summary.plan.refused,
        vec![Refused {
            from: root.join("1.wav"),
            refusal: Refusal::SourceGone,
        }]
    );
    assert!(echoes.exists());
    assert!(library.track_at(&echoes, None)?.is_some());
    assert!(
        library.track_at(&root.join("1.wav"), None)?.is_some(),
        "the row of a file that has gone was rewritten anyway"
    );
    Ok(())
}

#[test]
fn a_track_still_plays_after_the_move_because_the_catalog_names_where_it_landed() -> Result<()> {
    let tree = Tree::new();
    let bytes = meddle("Echoes", "2");
    tree.write("loose/echoes.wav", &bytes);

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    applied(&library)?;
    let landed = filed_under(&tree).join("Pink Floyd/Meddle/02 Echoes.wav");
    let track = library
        .track_at(&landed, None)?
        .expect("the catalog names the file where it landed");
    let named = track
        .location
        .as_path()
        .expect("a scanned row names a local file");

    assert_eq!(named, landed);
    assert_eq!(
        fs::read(named).expect("the file the catalog names opens"),
        bytes
    );
    Ok(())
}

#[test]
fn a_move_the_volume_refuses_is_refused_alone_and_the_rest_of_its_batch_lands() -> Result<()> {
    let tree = Tree::new();
    tree.write("1.wav", &meddle("One of These Days", "1"));
    tree.write(
        "2.wav",
        &Wav::new()
            .text(TITLE, "Stray")
            .text(ARTIST, "Syd Barrett")
            .text(ALBUM_ARTIST, "Blocked")
            .text(ALBUM, "Barrett")
            .text(TRACK, "1")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    tree.write("Blocked", b"a file where a folder has to go");

    let summary = applied(&library)?;
    let root = filed_under(&tree);

    let days = root.join("Pink Floyd/Meddle/01 One of These Days.wav");
    assert_eq!(
        summary.stats.moved, 1,
        "one refused move took its whole batch with it"
    );
    assert_eq!(summary.stats.failed, 1);
    assert!(days.exists());
    assert!(library.track_at(&days, None)?.is_some());
    assert!(
        root.join("2.wav").exists(),
        "the refused file did not stay where it stood"
    );
    assert!(library.track_at(&root.join("2.wav"), None)?.is_some());
    assert!(
        summary
            .plan
            .refused
            .iter()
            .any(|refused| refused.from == root.join("2.wav")
                && matches!(refused.refusal, Refusal::Unmoved { .. })),
        "the refused move was not named: {:?}",
        summary.plan.refused
    );

    fs::remove_file(root.join("Blocked")).expect("the file in the folder's way goes away");
    let again = applied(&library)?;
    assert_eq!(
        again.stats.moved, 1,
        "what was refused was not there for the run after it"
    );
    assert_eq!(again.stats.failed, 0);
    assert!(
        library
            .track_at(&root.join("Blocked/Barrett/01 Stray.wav"), None)?
            .is_some()
    );
    Ok(())
}

#[test]
fn a_stale_row_naming_a_destination_is_taken_out_of_the_way_rather_than_failing_its_batch()
-> Result<()> {
    let tree = Tree::new();
    tree.write("1.wav", &meddle("One of These Days", "1"));
    tree.write("nameless.wav", &Wav::new().text(TITLE, "Nothing").build());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let days = root.join("Pink Floyd/Meddle/01 One of These Days.wav");
    beside(&database)
        .execute(
            "UPDATE tracks SET path = ?1 WHERE path LIKE '%nameless.wav'",
            [days.to_str().expect("a path this test wrote is utf-8")],
        )
        .expect("the catalog takes a path no file stands at");

    let summary = applied(&library)?;

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.stats.failed, 0);
    assert_eq!(
        summary.stats.unidentified, 1,
        "the row the layout can place nowhere is the one left standing"
    );
    assert_eq!(landings(&summary), vec![(root.join("1.wav"), days.clone())]);
    assert_eq!(
        library.track_at(&days, None)?.map(|track| track.title),
        Some("One of These Days".to_owned()),
        "the row at the destination is the file that landed there"
    );
    Ok(())
}

fn hunted(title: &str, artist: &str, number: &str) -> Vec<u8> {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, artist)
        .text(ALBUM, "Hunted")
        .text(TRACK, number)
        .build()
}

const SILVER: &str = "Silver for Monsters";
const FIELDS: &str = "The Fields of Ard Skellig";

fn scanned_soundtrack() -> Result<(Tree, Library, PathBuf)> {
    let tree = Tree::new();
    tree.write("OST/1.wav", &hunted(SILVER, "Marcin Przybylowicz", "1"));
    tree.write("OST/2.wav", &hunted(FIELDS, "Percival", "2"));

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    Ok((tree, library, database))
}

fn grouping_key(database: &Path) -> String {
    beside(database)
        .query_row("SELECT key FROM album_keys", [], |row| row.get(0))
        .expect("the catalog holds one album to be keyed")
}

fn keys(database: &Path) -> Vec<String> {
    let reader = beside(database);
    let mut statement = reader
        .prepare("SELECT key FROM album_keys ORDER BY album_id, key")
        .expect("the albums are readable");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .expect("the albums answer their keys")
}

fn names_the_folder(key: &str, folder: &Path) -> bool {
    key.ends_with(folder.to_str().expect("a folder the catalog can hold"))
}

#[test]
fn an_album_grouped_by_its_folder_is_re_keyed_to_the_folder_it_moved_into() -> Result<()> {
    let (tree, library, database) = scanned_soundtrack()?;
    let root = filed_under(&tree);

    let stood = grouping_key(&database);
    assert!(
        names_the_folder(&stood, &root.join("OST")),
        "the scan keyed the album by something other than its folder: {stood:?}"
    );
    let before = only_album(&library)?;

    applied(&library)?;
    let landed = root.join("Hunted");
    assert!(
        landed.is_dir(),
        "the album did not land in a folder of its own"
    );

    let key = grouping_key(&database);
    assert!(
        names_the_folder(&key, &landed),
        "the album still names the folder it came out of: {key:?}"
    );
    assert_eq!(
        only_album(&library)?.id,
        before.id,
        "re-keying the album moved its rows onto another one"
    );
    Ok(())
}

#[test]
fn a_re_scan_after_a_move_still_finds_one_album_rather_than_two() -> Result<()> {
    let (tree, library, database) = scanned_soundtrack()?;
    let before = only_album(&library)?;
    let cover = png(64);

    let written = beside(&database)
        .execute(
            "UPDATE albums SET mbid = ?1, cover_art = ?2, cover_source = ?3 WHERE id = ?4",
            rusqlite::params![RELEASE, cover, 1, before.id.get() as i64],
        )
        .expect("the album the enrichment answers for is writable");
    assert_eq!(written, 1, "one album row was expected to be answered for");

    applied(&library)?;
    let landed = filed_under(&tree).join("Hunted/01 Silver for Monsters.wav");
    assert!(
        landed.exists(),
        "the first track did not land where it was planned"
    );

    tree.write(
        "Hunted/01 Silver for Monsters.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, SILVER)
            .text(ARTIST, "Marcin Przybylowicz")
            .text(ALBUM, "Hunted")
            .text(TRACK, "1")
            .build(),
    );
    let counts = scan(&library, &options(&tree))?;
    assert_eq!(counts.updated, 1, "the touched file was not probed again");
    assert_eq!(counts.added, 0, "the touched file was read as a new one");
    only_album(&library)?;

    tree.write(
        "Hunted/02 The Fields of Ard Skellig.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, FIELDS)
            .text(ARTIST, "Percival")
            .text(ALBUM, "Hunted")
            .text(TRACK, "2")
            .build(),
    );
    scan(&library, &options(&tree))?;

    let after = only_album(&library)?;
    assert_eq!(
        after.id, before.id,
        "the re-probed track was grouped onto an album of its own"
    );
    assert_eq!(after.track_count, 2);
    assert_eq!(after.mbid, Some(mbid(RELEASE)));
    assert!(
        after.has_cover_art,
        "the album's cover was swept with its row"
    );
    assert_eq!(
        library.cover_art(after.id)?.map(|art| art.bytes),
        Some(cover)
    );
    Ok(())
}

#[test]
fn an_album_whose_tracks_land_in_different_folders_keeps_the_key_it_had() -> Result<()> {
    let (tree, library, database) = scanned_soundtrack()?;
    let stood = grouping_key(&database);

    let summary = library
        .organise(OrganiseOptions {
            layout: Layout::read("{artist}/{album}/{track} {title}")?,
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    let root = filed_under(&tree);
    assert_eq!(summary.stats.moved, 2);
    assert!(
        root.join("Marcin Przybylowicz/Hunted/01 Silver for Monsters.wav")
            .exists()
    );
    assert!(
        root.join("Percival/Hunted/02 The Fields of Ard Skellig.wav")
            .exists()
    );

    assert_eq!(
        grouping_key(&database),
        stood,
        "an album split across two folders was keyed by one of them"
    );
    Ok(())
}

#[test]
fn an_album_moving_into_a_folder_another_album_already_names_keeps_the_key_it_had() -> Result<()> {
    let tree = Tree::new();
    tree.write("A/1.wav", &hunted(SILVER, "Marcin Przybylowicz", "1"));
    tree.write("B/2.wav", &hunted(FIELDS, "Marcin Przybylowicz", "2"));

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let held = keys(&database);
    assert_eq!(held.len(), 2, "two folders were expected to be two albums");

    applied(&library)?;
    let landed = root.join("Marcin Przybylowicz/Hunted");
    let after = keys(&database);

    assert_eq!(after.len(), 2, "two albums were merged into one");
    assert_eq!(
        after
            .iter()
            .filter(|key| names_the_folder(key, &landed))
            .count(),
        1,
        "both albums were keyed by the folder they share"
    );
    assert!(
        after.iter().any(|key| held.contains(key)),
        "the album that could not be re-keyed was keyed by something new"
    );
    Ok(())
}

const MEDDLE_AIFF_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.aiff" AIFF
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Echoes"
    INDEX 01 00:00:20
"#;

struct Aiff {
    frames: u32,
    id3: Vec<u8>,
}

impl Aiff {
    fn new() -> Self {
        Self {
            frames: 4_410,
            id3: Vec::new(),
        }
    }

    fn frames(mut self, frames: u32) -> Self {
        self.frames = frames;
        self
    }

    fn text(mut self, id: &[u8; 4], value: &str) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, id, &body);
        self
    }

    fn described(mut self, description: &str, value: &str) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(description.as_bytes());
        body.push(0);
        body.extend_from_slice(value.as_bytes());
        frame(&mut self.id3, USER_TEXT, &body);
        self
    }

    fn picture(mut self, bytes: &[u8]) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(b"image/png\0");
        body.push(FRONT_COVER);
        body.push(0);
        body.extend_from_slice(bytes);
        frame(&mut self.id3, b"APIC", &body);
        self
    }

    fn build(&self) -> Vec<u8> {
        let bits = 16_u16;
        let mut common = Vec::new();
        common.extend_from_slice(&CHANNELS.to_be_bytes());
        common.extend_from_slice(&self.frames.to_be_bytes());
        common.extend_from_slice(&bits.to_be_bytes());
        let leading = RATE.leading_zeros();
        let exponent = (16_383 + 31 - leading) as u16;
        common.extend_from_slice(&exponent.to_be_bytes());
        common.extend_from_slice(&(u64::from(RATE) << (leading + 32)).to_be_bytes());

        let stride = usize::from(CHANNELS) * usize::from(bits / 8);
        let mut sound = vec![0_u8; 8];
        sound.resize(sound.len() + self.frames as usize * stride, 0);

        let mut body = b"AIFF".to_vec();
        aiff_chunk(&mut body, b"COMM", &common);
        aiff_chunk(&mut body, b"SSND", &sound);
        if !self.id3.is_empty() {
            let mut tag = b"ID3\x04\x00\x00".to_vec();
            tag.extend_from_slice(&synchsafe(self.id3.len() as u32));
            tag.extend_from_slice(&self.id3);
            aiff_chunk(&mut body, b"ID3 ", &tag);
        }

        let mut file = b"FORM".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_be_bytes());
        file.extend_from_slice(&body);
        file
    }
}

fn aiff_chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn retagged(library: &Library, apply: bool) -> Result<RetagSummary> {
    let summary = library
        .retag(
            Arc::new(FileTags::default()),
            RetagOptions {
                roots: Vec::new(),
                apply,
            },
        )?
        .join()?;
    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

fn tags_of(file: &Path) -> TagSet {
    FileTags::default()
        .read(&MediaLocation::local(file), Picturing::Whether)
        .expect("a file whose tags read back")
        .tags
}

fn cover_of(file: &Path) -> Option<CoverArt> {
    FileTags::default()
        .read(&MediaLocation::local(file), Picturing::Copied)
        .expect("a file whose picture reads back")
        .picture
        .into_copied()
}

fn covered(library: &Library, art: &CoverArt) -> Result<()> {
    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("one album was grouped");
    assert!(
        library.land_archive_cover(album.id, art)?,
        "the catalog took no cover to write back"
    );
    Ok(())
}

fn fields_of(written: &Written) -> Vec<TagField> {
    written.edits.iter().map(|edit| edit.field).collect()
}

fn why_passed_over(summary: &RetagSummary) -> Vec<Unwritten> {
    summary
        .retagging
        .passed_over
        .iter()
        .map(|over| over.why)
        .collect()
}

#[test]
fn only_what_a_lookup_answered_for_is_written_into_the_file() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let preview = retagged(&library, false)?;
    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(
        fields_of(&preview.retagging.writes[0]),
        vec![TagField::Title]
    );
    assert_eq!(preview.stats.written, 0, "a preview wrote something");
    assert_eq!(
        tags_of(&file).title.as_deref(),
        Some("Echos"),
        "a preview changed the file"
    );

    let applied = retagged(&library, true)?;
    assert_eq!(applied.stats.written, 1);
    assert_eq!(applied.stats.fields, 1);
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert_eq!(
        tags_of(&file).artist.as_deref(),
        Some("The Orbiters"),
        "a name the file already carried was written again"
    );

    let again = retagged(&library, false)?;
    assert!(
        again.retagging.writes.is_empty(),
        "the write did not stick: {:?}",
        again.retagging.writes
    );
    assert_eq!(again.stats.unchanged, 1);
    Ok(())
}

#[test]
fn a_rescan_reads_this_builds_own_write_as_the_names_it_already_knew() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");
    retagged(&library, true)?;

    let held = stored(&database, &file);
    assert_eq!(
        held.tagged_title.as_deref(),
        Some("Echoes"),
        "the catalog did not follow the file it had just written"
    );
    assert!(held.answered.is_some());

    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    let after = stored(&database, &file);
    assert_eq!(after.title, "Echoes");
    assert!(
        after.answered.is_some(),
        "a rescan read this build's own write as a retagging and asked again"
    );
    Ok(())
}

#[test]
fn a_release_that_landed_is_written_into_every_file_it_named() -> Result<()> {
    let tree = Tree::new();
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        tree.write(
            &format!("Orbits/{number}.aiff"),
            &Aiff::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(TRACK, &number.to_string())
                .described(MUSICBRAINZ_ALBUM, RELEASE)
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let fake = Arc::new(Fake::new(Canned {
        releases: vec![orbits(orbits_rows(), Vec::new())],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let summary = retagged(&library, true)?;
    assert_eq!(summary.stats.written, 3, "{:?}", why_passed_over(&summary));

    let held = tags_of(&tree.path().join("Orbits/1.aiff"));
    assert_eq!(held.album.as_deref(), Some("Orbits"));
    assert_eq!(held.date.as_deref(), Some("1998-03-02"));
    assert_eq!(held.label.as_deref(), Some("Harvest"));
    assert_eq!(held.catalog_number.as_deref(), Some("SHVL 795"));
    assert_eq!(held.barcode.as_deref(), Some("724382920021"));
    assert_eq!(held.musicbrainz_album_id.as_deref(), Some(RELEASE));
    assert_eq!(
        held.musicbrainz_release_group_id.as_deref(),
        Some(RELEASE_GROUP)
    );
    assert_eq!(held.track_total, Some(3));
    assert_eq!(held.disc_total, Some(1));
    assert_eq!(
        held.album_artist, None,
        "an album artist no lookup named was written"
    );
    Ok(())
}

#[test]
fn a_cover_the_catalog_holds_is_written_into_a_file_that_carries_none() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .build(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let art = png_art(512);
    covered(&library, &art)?;

    let preview = retagged(&library, false)?;
    assert_eq!(preview.retagging.writes.len(), 1);
    assert_eq!(preview.retagging.writes[0].picture.as_deref(), Some(&art));
    assert_eq!(cover_of(&file), None, "a preview wrote a picture");

    let applied = retagged(&library, true)?;
    assert_eq!(applied.stats.pictures, 1, "{:?}", why_passed_over(&applied));
    assert_eq!(cover_of(&file), Some(art));

    let again = retagged(&library, false)?;
    assert!(
        again.retagging.writes.is_empty(),
        "the picture did not stick: {:?}",
        again.retagging.writes
    );
    Ok(())
}

#[test]
fn a_file_that_carries_a_picture_keeps_the_one_it_has() -> Result<()> {
    let held = png_art(64);
    let tree = Tree::new();
    let file = tree.write(
        "1.aiff",
        &Aiff::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .text(ALBUM_ARTIST, "Pink Floyd")
            .picture(&held.bytes)
            .build(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = retagged(&library, true)?;

    assert!(
        summary.retagging.writes.is_empty(),
        "a file with a picture of its own was offered another: {:?}",
        summary.retagging.writes
    );
    assert_eq!(summary.stats.pictures, 0);
    assert_eq!(cover_of(&file), Some(held));
    Ok(())
}

#[test]
fn a_track_under_no_album_is_offered_no_picture() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.aiff", &Aiff::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert_eq!(summary.stats.pictures, 0);
    assert_eq!(cover_of(&file), None);
    Ok(())
}

#[test]
fn a_wave_file_is_written_into_and_read_back_as_what_was_written() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(
        Wav::new()
            .text(TITLE, "Echos")
            .text(ARTIST, "The Orbiters")
            .id3_in_a_chunk(),
    )?;
    assert_eq!(
        stored(&database, &file).tagged_title.as_deref(),
        Some("Echos"),
        "the scan did not read the ID3 chunk"
    );
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");

    let applied = retagged(&library, true)?;
    assert_eq!(applied.stats.written, 1, "{:?}", why_passed_over(&applied));
    assert_eq!(tags_of(&file).title.as_deref(), Some("Echoes"));
    assert_eq!(tags_of(&file).artist.as_deref(), Some("The Orbiters"));

    let again = retagged(&library, false)?;
    assert!(
        again.retagging.writes.is_empty(),
        "the write did not stick: {:?}",
        again.retagging.writes
    );
    Ok(())
}

#[test]
fn a_wave_file_tagged_ahead_of_its_riff_header_is_refused_rather_than_written() -> Result<()> {
    let (_tree, library, database, file) = scanned_lone(Wav::new().text(TITLE, "Echos"))?;
    answer_track(&database, &file, "Echoes", "The Orbiters", "Orbits");
    let held = fs::read(&file).expect("the file is there");

    let summary = retagged(&library, true)?;

    assert_eq!(why_passed_over(&summary), vec![Unwritten::Refused]);
    assert_eq!(fs::read(&file).expect("the file is still there"), held);
    Ok(())
}

#[test]
fn a_format_this_build_would_not_read_its_own_write_back_from_is_passed_over() -> Result<()> {
    let tree = Tree::new();
    tree.write("1.caf", &Wav::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert_eq!(why_passed_over(&summary), vec![Unwritten::Unsupported]);
    assert_eq!(summary.stats.passed_over, 1);
    Ok(())
}

#[test]
fn a_row_cut_out_of_a_file_it_shares_is_never_written_to() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("Meddle.aiff", &Aiff::new().frames(44_100).build());
    tree.write("Meddle.cue", MEDDLE_AIFF_SHEET.as_bytes());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    assert_eq!(
        library.tracks(&TrackQuery::default())?.len(),
        2,
        "the sheet did not cut the file into rows"
    );

    let held = fs::metadata(&file).expect("the cut file is there").len();
    let summary = retagged(&library, true)?;

    assert!(summary.retagging.writes.is_empty());
    assert_eq!(why_passed_over(&summary), vec![Unwritten::Cut]);
    assert_eq!(
        fs::metadata(&file)
            .expect("the cut file is still there")
            .len(),
        held,
        "a file a sheet cuts was written to"
    );
    Ok(())
}

#[test]
fn a_suggestion_is_read_against_the_catalog_as_the_scan_left_it() -> Result<()> {
    let tree = Tree::new();
    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    assert_eq!(library.did_you_mean("metalica")?, None);

    tree.write(
        "ride.wav",
        &Wav::new()
            .text(TITLE, "Ride the Lightning")
            .text(ARTIST, "Metallica")
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.did_you_mean("metalica")?,
        Some("Metallica".to_owned()),
        "a suggestion was read against the catalog as it stood before the scan"
    );
    Ok(())
}

fn heard_in(database: &Path) -> Vec<i64> {
    let connection = beside(database);
    let mut statement = connection
        .prepare("SELECT heard FROM listens ORDER BY id")
        .expect("the listens read back");
    statement
        .query_map([], |row| row.get::<_, i64>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .expect("the listens read back")
}

#[test]
fn a_play_records_how_long_of_it_was_heard() -> Result<()> {
    let tree = Tree::new();
    tree.write("cold.wav", &Wav::new().text(TITLE, "Cold").build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    scan(&library, &options(&tree))?;

    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    let counted = library
        .track_played(&cold, None)?
        .expect("the row the play counted");
    assert_eq!(
        heard_in(&database),
        vec![0],
        "a play that has not been heard out yet was counted as though it had"
    );

    library.listened(counted.listen, Duration::from_secs(90))?;
    assert_eq!(heard_in(&database), vec![90_000_000_000]);

    let again = library
        .track_played(&cold, None)?
        .expect("the row the play counted");
    assert_ne!(again.listen, counted.listen);
    library.listened(again.listen, Duration::from_millis(1_500))?;
    assert_eq!(
        heard_in(&database),
        vec![90_000_000_000, 1_500_000_000],
        "one visit's length was written over another's"
    );
    Ok(())
}

fn favoured_tracks(library: &Library) -> Result<Vec<String>> {
    Ok(library
        .favourite_tracks(&TrackQuery {
            sort: SortOrder::Favourited,
            reading: SortOrder::Favourited.reads(),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

#[test]
fn a_favourite_track_is_found_by_is_favourite() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");
    assert_eq!(cold.favourite, None);
    assert!(favoured_tracks(&library)?.is_empty());

    assert!(library.favour(Favoured::Track(cold.id), true)?);

    let narrowed = library.tracks(&TrackQuery {
        text: Some("is:favourite".to_owned()),
        sort: SortOrder::Title,
        ..TrackQuery::default()
    })?;
    assert_eq!(titles(&narrowed), vec!["Cold"]);
    assert!(narrowed[0].favourite.is_some());
    assert_eq!(
        favoured_tracks(&library)?,
        vec!["Cold"],
        "the favourites listing and is:favourite do not agree"
    );
    Ok(())
}

#[test]
fn favouriting_an_album_leaves_its_tracks_alone() -> Result<()> {
    let (_tree, library) = scanned_shapes();
    let winter = album_titled(&library, "Winter")?;
    assert_eq!(winter.favourite, None);

    assert!(library.favour(Favoured::Album(winter.id), true)?);

    assert!(
        library
            .album(winter.id)?
            .and_then(|album| album.favourite)
            .is_some()
    );
    assert_eq!(
        library
            .favourite_albums(&AlbumQuery::default())?
            .into_iter()
            .map(|album| album.title)
            .collect::<Vec<String>>(),
        vec!["Winter"]
    );
    assert!(
        favoured_tracks(&library)?.is_empty(),
        "favouriting an album favourited the tracks under it"
    );

    let ada = artist_named(&library, "Ada")?;
    assert_eq!(ada.favourite, None);
    assert!(library.favour(Favoured::Artist(ada.id), true)?);
    assert_eq!(
        library
            .favourite_artists(&ArtistQuery::default())?
            .into_iter()
            .map(|artist| artist.name)
            .collect::<Vec<String>>(),
        vec!["Ada"]
    );
    assert!(favoured_tracks(&library)?.is_empty());
    Ok(())
}

#[test]
fn unfavouriting_puts_a_row_back_the_way_it_was() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");

    assert!(library.favour(Favoured::Track(cold.id), true)?);
    assert!(library.favour(Favoured::Track(cold.id), false)?);

    assert_eq!(
        library.track(cold.id)?,
        Some(cold),
        "unfavouriting left the row somewhere other than where it started"
    );
    assert!(favoured_tracks(&library)?.is_empty());
    assert!(
        !library.favour(Favoured::Track(TrackId::MAX), false)?,
        "an id the catalog holds no row for answered as though a row had changed"
    );
    Ok(())
}

#[test]
fn favouring_a_favourite_keeps_the_stamp_it_was_marked_with_and_answers_that_nothing_moved()
-> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = library
        .track_at(&tree.path().join("cold.wav"), None)?
        .expect("the scan stored the file it walked");

    assert!(library.favour(Favoured::Track(cold.id), true)?);
    let marked = library
        .track(cold.id)?
        .expect("the row is still there")
        .favourite;
    assert!(marked.is_some(), "favouring left no stamp");

    assert!(
        !library.favour(Favoured::Track(cold.id), true)?,
        "favouring a favourite answered as though it had moved"
    );
    assert_eq!(
        library
            .track(cold.id)?
            .expect("the row is still there")
            .favourite,
        marked,
        "favouring a favourite again restamped it"
    );

    assert!(library.favour(Favoured::Track(cold.id), false)?);
    assert!(
        !library.favour(Favoured::Track(cold.id), false)?,
        "unfavouring a row that is no favourite answered as though it had moved"
    );
    Ok(())
}

fn listed_playlists(library: &Library) -> Result<Vec<Vec<String>>> {
    let mut every = Vec::new();
    for order in PlaylistOrder::ALL {
        for reading in Direction::ALL {
            every.push(
                library
                    .playlists(order, reading, None)?
                    .into_iter()
                    .map(|found| found.name)
                    .collect(),
            );
        }
    }

    Ok(every)
}

#[test]
fn a_pinned_playlist_leads_every_order() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    library.start_playlist("Morning", &[file("a.wav")])?;
    library.start_playlist("Evening", &[file("b.wav"), file("c.wav")])?;
    let night = library.start_playlist("Night", &[file("c.wav")])?;
    assert!(
        listed_playlists(&library)?
            .iter()
            .any(|listed| listed.first().map(String::as_str) != Some("Night")),
        "the listings led with Night before anything was pinned, so the pin proves nothing"
    );

    assert!(library.pin_playlist(night, true)?);
    assert!(
        library
            .playlist(night)?
            .and_then(|found| found.pinned)
            .is_some()
    );
    for listed in listed_playlists(&library)? {
        assert_eq!(
            listed.first().map(String::as_str),
            Some("Night"),
            "a pinned playlist did not lead the listing: {listed:?}"
        );
    }

    assert!(library.pin_playlist(night, false)?);
    assert_eq!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .into_iter()
            .map(|found| found.name)
            .collect::<Vec<String>>(),
        vec!["Evening", "Morning", "Night"],
        "an unpinned playlist kept the front of the listing"
    );
    assert!(
        !library.pin_playlist(PlaylistId::MAX, true)?,
        "an id the catalog holds no playlist for answered as though a row had changed"
    );
    Ok(())
}

#[test]
fn undoing_an_edit_keeps_a_playlist_pinned() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.start_playlist("Evening", &[file("a.wav"), file("b.wav")])?;
    assert!(library.pin_playlist(evening, true)?);
    let pinned = library
        .playlist(evening)?
        .expect("the playlist is there")
        .pinned;
    assert!(pinned.is_some());

    assert!(library.remove_from_playlist(evening, Span::one(0))?);
    let put_back = library.undo()?.expect("the drop is there to put back");
    assert_eq!(put_back.playlist, evening);

    let after = library.playlist(evening)?.expect("the playlist is there");
    assert_eq!(after.entries, 2, "the row did not come back");
    assert_eq!(
        after.pinned, pinned,
        "undoing an edit wrote the playlist row back without what pinned it"
    );
    Ok(())
}

#[test]
fn a_pin_made_after_an_edit_survives_walking_the_edit_back() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let file = |name: &str| Cut::whole(MediaLocation::local(tree.path().join(name)));
    let evening = library.start_playlist("Evening", &[file("a.wav")])?;

    library.add_to_playlist(evening, &[file("b.wav")])?;
    assert!(library.pin_playlist(evening, true)?);
    library.undo()?.expect("the add is there to walk back");
    let after = library.playlist(evening)?.expect("the playlist is there");
    assert_eq!(after.entries, 1);
    assert!(
        after.pinned.is_some(),
        "walking back an add took a later pin away"
    );

    library.add_to_playlist(evening, &[file("b.wav")])?;
    assert!(library.pin_playlist(evening, false)?);
    library.undo()?.expect("the add is there to walk back");
    let after = library.playlist(evening)?.expect("the playlist is there");
    assert!(
        after.pinned.is_none(),
        "walking back an add put a pin taken off back"
    );
    Ok(())
}

fn narrowed(library: &Library, text: &str) -> Result<Vec<String>> {
    Ok(library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            sort: SortOrder::Title,
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

#[test]
fn a_genre_tag_is_searchable() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(GENRE, "Progressive Rock")
            .build(),
    );
    tree.write("b.wav", &Wav::new().text(TITLE, "Dogs").build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let echoes = library
        .track_at(&tree.path().join("a.wav"), None)?
        .expect("the scan stored the file it walked");
    assert_eq!(echoes.genre.as_deref(), Some("Progressive Rock"));

    assert_eq!(narrowed(&library, "genre:progressive")?, vec!["Echoes"]);
    assert_eq!(narrowed(&library, "genre:rock")?, vec!["Echoes"]);
    assert!(
        narrowed(&library, "genre:dogs")?.is_empty(),
        "a title answered a search scoped to the genre"
    );
    assert_eq!(
        narrowed(&library, "progressive")?,
        vec!["Echoes"],
        "an unscoped word did not reach the genre the index holds"
    );
    Ok(())
}

#[test]
fn a_genre_musicbrainz_gave_an_artist_reaches_its_tracks() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    assert!(narrowed(&library, "genre:psychedelic")?.is_empty());

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![ArtistMatch {
            mbid: mbid(ORBITERS),
            name: "The Orbiters".to_owned(),
            score: 100,
            kind: Some("Group".to_owned()),
            disambiguation: None,
            aliases: Vec::new(),
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    assert_eq!(
        library
            .artist_detail(artist_named(&library, "The Orbiters")?.id)?
            .map(|detail| detail.genres.len()),
        Some(2)
    );
    assert_eq!(
        narrowed(&library, "genre:psychedelic")?,
        vec!["A Pillow of Winds", "Fearless!", "One of These Days"],
        "the genres the reference gave the artist did not reach the tracks it made"
    );
    Ok(())
}

#[test]
fn a_saved_query_reads_back_the_order_it_was_saved_with() -> Result<()> {
    let (_tree, library) = scanned_playlist_tree();

    for sort in SortOrder::ALL {
        for reading in Direction::ALL {
            let query = SavedQuery {
                text: None,
                sort,
                reading,
                limit: None,
            };
            let id = library.save_query(&format!("{sort:?} read {reading:?}"), &query)?;

            assert_eq!(
                library.playlist(id)?.and_then(|found| found.query),
                Some(query),
                "{sort:?} read {reading:?} did not come back as the order it was saved with"
            );
        }
    }
    Ok(())
}

const A_STREAMING_LINK: &str = "https://open.spotify.com/track/1a2b3c4d5e";
const AN_ENCYCLOPAEDIA_LINK: &str = "https://en.wikipedia.org/wiki/Orbits";
const SONG_LINK: &str = "https://song.link/";
const MUSICBRAINZ_RECORDING: &str = "https://musicbrainz.org/recording/";

const HEARD_FOR: Duration = Duration::from_secs(180);

const TRACKS_OF_A_DECADE: u32 = 24;

fn orbits_from_the_nineties(tree: &Tree) {
    for number in 1..=TRACKS_OF_A_DECADE {
        tree.write(
            &format!("orbits/{number:02}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Orbit {number:02}"))
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(GENRE, "Progressive Rock")
                .text(YEAR, "1995")
                .text(TRACK, &number.to_string())
                .build(),
        );
    }
}

fn asked(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions
        .iter()
        .filter_map(|suggestion| suggestion.query.text.as_deref())
        .collect()
}

#[test]
fn listening_time_is_the_time_that_was_heard() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let track = all(&library)?.remove(0);

    let counted = library
        .track_played(&track.location, track.span)?
        .expect("the catalog counted the play against the row it scanned");
    library.listened(counted.listen, HEARD_FOR)?;

    let heard = library.statistics(Window::Week)?;
    assert_eq!(heard.plays, 1);
    assert_eq!(heard.listened, HEARD_FOR);
    assert_eq!(heard.tracks, 1);
    assert_eq!(heard.albums, 1);
    assert_eq!(heard.artists, 1);

    let most = library.most_listened(Window::Week, 5)?;
    assert_eq!(
        most.tracks
            .first()
            .map(|row| (row.name.as_str(), row.listened)),
        Some(("Echoes", HEARD_FOR)),
        "the track heard for three minutes is not the one most listened to"
    );
    assert_eq!(
        most.albums.first().map(|row| row.name.as_str()),
        Some("Orbits")
    );
    assert_eq!(
        most.artists.first().map(|row| row.name.as_str()),
        Some("The Orbiters")
    );

    let drawn = library.listening_by_day(Window::Week)?;
    assert_eq!(
        drawn.last().map(|day| (day.plays, day.listened)),
        Some((1, HEARD_FOR)),
        "the day the play was counted on does not hold the time it was heard for"
    );
    Ok(())
}

#[test]
fn a_thin_catalog_offers_no_suggestion_it_cannot_fill() -> Result<()> {
    let tree = Tree::new();
    for (file, title) in [("a.wav", "Alpha"), ("b.wav", "Beta")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(GENRE, "Progressive Rock")
                .text(YEAR, "1995")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let suggestions = library.suggestions()?;
    assert!(
        suggestions.is_empty(),
        "a catalog of two tracks offered {:?}",
        asked(&suggestions)
    );
    Ok(())
}

#[test]
fn every_suggestion_reads_back_through_the_grammar_it_was_written_in() -> Result<()> {
    let tree = Tree::new();
    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let suggestions = library.suggestions()?;
    assert!(
        !suggestions.is_empty(),
        "a catalog of {TRACKS_OF_A_DECADE} tracks offered nothing"
    );
    for suggestion in suggestions.iter() {
        let text = suggestion
            .query
            .text
            .as_deref()
            .expect("a suggestion asks the catalog something");
        let read = Search::read(text);

        assert!(!read.is_empty(), "{text} reads as no search at all");
        assert_eq!(
            read.to_string(),
            *text,
            "{text} did not read back as what it was written as"
        );
        assert!(suggestion.rows > 0, "{text} was offered filling nothing");
        assert!(!suggestion.name.is_empty());
        assert!(!suggestion.reason.says().is_empty());
    }

    let texts = asked(&suggestions);
    assert!(texts.contains(&"year:1990-1999"), "{texts:?}");
    assert!(texts.contains(&"genre:\"Progressive Rock\""), "{texts:?}");
    assert!(texts.contains(&"artist:\"The Orbiters\""), "{texts:?}");
    assert!(texts.contains(&"plays:0"), "{texts:?}");
    Ok(())
}

#[test]
fn a_suggestion_says_how_long_it_runs_and_which_covers_picture_it() -> Result<()> {
    let tree = Tree::new();
    orbits_from_the_nineties(&tree);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let bare = library.suggestions()?;
    assert!(
        bare.iter()
            .all(|suggestion| suggestion.pictured_by.is_empty() && suggestion.length.is_some()),
        "a catalog with no cover pictured a suggestion or one ran for no time"
    );

    let album = only_album(&library)?;
    assert!(library.land_archive_cover(album.id, &png_art(64))?);
    let pictured = library.suggestions()?;
    assert!(
        pictured
            .iter()
            .all(|suggestion| suggestion.pictured_by == vec![album.id]),
        "the one covered album did not picture every suggestion it fills"
    );
    Ok(())
}

fn a_sleeve(side: u32, mirrored: bool) -> CoverArt {
    let drawn = image::RgbImage::from_fn(side, side, |x, y| {
        let across = if mirrored { side - 1 - x } else { x };
        let (u, v) = (across as f32 / side as f32, y as f32 / side as f32);
        if (0.15..0.55).contains(&u) && (0.2..0.6).contains(&v) {
            image::Rgb([220, 40, 30])
        } else {
            image::Rgb([(u * 60.0) as u8, (v * 90.0) as u8, 70])
        }
    });
    let mut written = std::io::Cursor::new(Vec::new());
    drawn
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a written picture");

    CoverArt {
        format: ImageFormat::Png,
        bytes: written.into_inner(),
    }
}

fn orbits_in_three_editions(tree: &Tree) {
    for (edition, album) in ["Orbits", "Orbits (Expanded)", "Other Orbits"]
        .into_iter()
        .enumerate()
    {
        for number in 1..=TRACKS_OF_A_DECADE / 2 {
            tree.write(
                &format!("orbits-{edition}/{number:02}.wav"),
                &Wav::new()
                    .text(TITLE, &format!("Orbit {number:02}"))
                    .text(ARTIST, "The Orbiters")
                    .text(ALBUM, album)
                    .text(GENRE, "Progressive Rock")
                    .text(YEAR, "1995")
                    .text(TRACK, &number.to_string())
                    .build(),
            );
        }
    }
}

#[test]
fn one_sleeve_saved_at_two_resolutions_pictures_a_suggestion_once() -> Result<()> {
    let tree = Tree::new();
    orbits_in_three_editions(&tree);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let album = |title: &str| -> Result<AlbumId> {
        Ok(library
            .albums(&AlbumQuery::default())?
            .into_iter()
            .find(|album| album.title == title)
            .unwrap_or_else(|| panic!("no album called {title}"))
            .id)
    };
    let edition = album("Orbits")?;
    let expanded = album("Orbits (Expanded)")?;
    let other = album("Other Orbits")?;
    assert!(library.land_archive_cover(edition, &a_sleeve(600, false))?);
    assert!(library.land_archive_cover(expanded, &a_sleeve(250, false))?);
    assert!(library.land_archive_cover(other, &a_sleeve(600, true))?);

    let decade = library
        .suggestions()?
        .iter()
        .find(|suggestion| suggestion.query.text.as_deref() == Some("year:1990-1999"))
        .cloned()
        .expect("the catalog suggests its decade");
    assert_eq!(decade.pictured_by.len(), 2, "{:?}", decade.pictured_by);
    assert!(decade.pictured_by.contains(&other));
    assert!(
        decade.pictured_by.contains(&edition) != decade.pictured_by.contains(&expanded),
        "one sleeve filled two tiles, or none: {:?}",
        decade.pictured_by
    );
    Ok(())
}

#[test]
fn suggestions_are_kept_until_a_name_they_were_built_from_moves() -> Result<()> {
    let tree = Tree::new();
    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let first = library.suggestions()?;
    assert!(
        Arc::ptr_eq(&first, &library.suggestions()?),
        "a second ask with nothing moved worked the suggestions out again"
    );

    library.save_query(
        "kept apart",
        &SavedQuery {
            text: Some("plays:0".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;
    assert!(
        Arc::ptr_eq(&first, &library.suggestions()?),
        "a playlist written moved suggestions that read no playlist"
    );

    let cold = all(&library)?
        .into_iter()
        .next()
        .expect("the scan stored the files it walked");
    library.favour(Favoured::Track(cold.id), true)?;
    assert!(
        !Arc::ptr_eq(&first, &library.suggestions()?),
        "a track written was answered with suggestions from before it"
    );
    Ok(())
}

#[test]
fn a_suggestion_saves_as_the_query_it_already_was() -> Result<()> {
    let tree = Tree::new();
    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    let suggestion = library
        .suggestions()?
        .first()
        .cloned()
        .expect("the catalog offers a suggestion");
    let saved = library.save_query(&suggestion.name, &suggestion.query)?;
    let playlist = library
        .playlist(saved)?
        .expect("the suggestion was saved as a playlist");

    assert_eq!(playlist.name, suggestion.name);
    assert_eq!(playlist.query, Some(suggestion.query));
    assert_eq!(u64::from(playlist.entries), suggestion.rows);
    Ok(())
}

#[test]
fn a_share_names_the_track_and_the_album_it_came_from() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(YEAR, "1971")
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let track = all(&library)?.remove(0);

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");
    assert_eq!(shared.title, "Echoes");
    assert_eq!(shared.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(shared.album.as_deref(), Some("Orbits"));
    assert_eq!(shared.year, Some(1971));
    assert!(shared.links.is_empty());
    assert_eq!(
        shared.written(),
        "The Orbiters — Echoes\nfrom Orbits (1971)"
    );
    Ok(())
}

#[test]
fn a_share_prefers_a_streaming_link_over_musicbrainz() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![release_row(
                1,
                "Echoes",
                vec![Link::new("streaming", A_STREAMING_LINK.to_owned())],
            )],
            vec![Link::new("wikipedia", AN_ENCYCLOPAEDIA_LINK.to_owned())],
        ),
    )?;
    library.rematch(album.id)?;

    let track = all(&library)?.remove(0);
    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(shared.recording, Some(mbid(RECORDING)));
    assert_eq!(
        shared.links.first().map(|link| link.url.as_str()),
        Some(A_STREAMING_LINK),
        "the recording\'s own link is not the first the share weighs"
    );
    assert!(
        shared
            .written()
            .ends_with(&format!("{SONG_LINK}{A_STREAMING_LINK}")),
        "{}",
        shared.written()
    );
    Ok(())
}

#[test]
fn a_share_falls_back_to_musicbrainz_where_no_service_is_linked() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    library.land_release(
        album.id,
        &orbits(
            vec![release_row(1, "Echoes", Vec::new())],
            vec![Link::new("wikipedia", AN_ENCYCLOPAEDIA_LINK.to_owned())],
        ),
    )?;
    library.rematch(album.id)?;

    let track = all(&library)?.remove(0);
    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(
        shared.links.first().map(|link| link.url.as_str()),
        Some(AN_ENCYCLOPAEDIA_LINK),
        "the album\'s link is held even where song.link cannot resolve it"
    );
    assert!(
        shared
            .written()
            .ends_with(&format!("{MUSICBRAINZ_RECORDING}{RECORDING}")),
        "{}",
        shared.written()
    );
    Ok(())
}

#[test]
fn a_share_of_a_track_nothing_knows_about_is_the_text_alone() -> Result<()> {
    let tree = Tree::new();
    tree.write("mystery.wav", &Wav::new().build());

    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let track = all(&library)?.remove(0);

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");
    assert_eq!(shared.recording, None);
    assert!(shared.links.is_empty());
    assert_eq!(shared.written(), shared.title);
    assert!(!shared.written().contains('\n'));
    Ok(())
}

fn vaulted(library: &Library, apply: bool) -> Result<ImportSummary> {
    let summary = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                roots: Vec::new(),
                apply,
                ..ImportOptions::default()
            },
        )?
        .join()?;
    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

fn a_real_picture(width: u32, height: u32) -> CoverArt {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[(x * 5) as u8, (y * 9) as u8, (x + y) as u8, 255]);
        }
    }
    let drawn = image::RgbaImage::from_raw(width, height, pixels).expect("a drawn picture");
    let mut written = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(drawn)
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a written picture");

    CoverArt {
        format: ImageFormat::Png,
        bytes: written.into_inner(),
    }
}

fn opened_with_a_vault(held: &Tree) -> Result<(Library, Arc<Vault>)> {
    let vault = Arc::new(Vault::open(held.path()).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(Arc::clone(&vault))?;
    Ok((library, vault))
}

#[test]
fn an_import_points_the_row_at_the_vault_and_leaves_the_file_where_it_stood() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM, "Meddle")
            .build(),
    );
    let bytes = fs::read(&path).expect("a source file");
    let stamp = fs::metadata(&path)
        .and_then(|held| held.modified())
        .expect("a stamped source file");

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;
    assert_eq!(summary.stats.vaulted, 1);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(summary.plan.vaulted.len(), 1);
    assert_eq!(summary.plan.vaulted[0].wanted.form, Form::Flac);

    let rows = all(&library)?;
    assert_eq!(
        rows[0].location.as_path(),
        Some(path.as_path()),
        "a vaulted row stopped being named by its own file"
    );
    assert_eq!(rows[0].title, "Echoes");

    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault stands in for the row");
    let landed = stood.location.as_path().expect("a local object");
    assert!(landed.starts_with(vault.root()));
    assert_eq!(landed.extension().and_then(OsStr::to_str), Some("flac"));
    assert_eq!(stood.tags.title.as_deref(), Some("Echoes"));
    assert_eq!(stood.tags.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(stood.tags.album.as_deref(), Some("Meddle"));

    assert_eq!(fs::read(&path).expect("a source file"), bytes);
    assert_eq!(
        fs::metadata(&path)
            .and_then(|held| held.modified())
            .expect("a stamped source file"),
        stamp
    );

    let objects = library.vault_objects()?;
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].form, Form::Flac);
    assert_eq!(objects[0].taken_from, path);
    assert!(objects[0].validated);
    Ok(())
}

#[test]
fn a_rescan_after_an_import_leaves_the_row_in_the_vault_and_imports_nothing_again() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let stats = scan(&library, &options(&tree))?;
    assert_eq!(stats.added, 0);
    assert_eq!(stats.updated, 0);
    assert_eq!(stats.removed, 0);

    let again = vaulted(&library, true)?;
    assert_eq!(again.stats.walked, 0);
    assert_eq!(again.stats.vaulted, 0);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the vault still stands in for the row");
    assert!(
        stood
            .location
            .as_path()
            .expect("a local object")
            .starts_with(vault.root())
    );
    assert_eq!(library.vault_objects()?.len(), 1);
    Ok(())
}

#[test]
fn an_object_kept_under_an_older_encoder_is_weighed_again_and_stamped_with_this_one() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::open(held.path()).expect("a writable vault"));
    let library = Library::open_with_vault(&database, Arc::clone(&vault))?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    assert_eq!(
        library.vault_objects()?[0].encoding,
        Encoding::OF_THIS_BUILD
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 0);

    beside(&database)
        .execute("UPDATE vault_objects SET encoding = 0", [])
        .expect("an object stamped as an older encoder's");

    let preview = vaulted(&library, false)?;
    assert_eq!(preview.stats.walked, 1);
    assert!(preview.plan.wanted[0].renewing);
    assert_eq!(preview.plan.wanted[0].form, Form::Flac);

    let renewed = vaulted(&library, true)?;
    assert_eq!(renewed.stats.vaulted, 1);
    assert_eq!(renewed.stats.passed, 0);
    let objects = library.vault_objects()?;
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].encoding, Encoding::OF_THIS_BUILD);
    assert!(objects[0].path.is_file());

    let rows = all(&library)?;
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_some(),
        "a renewed row no longer reaches its object"
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 0);
    Ok(())
}

#[test]
fn a_row_whose_source_has_gone_is_not_weighed_again_and_keeps_its_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let database = tree.path().join("library.db");
    let vault = Arc::new(Vault::open(held.path()).expect("a writable vault"));
    let library = Library::open_with_vault(&database, Arc::clone(&vault))?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&path).expect("the source goes");
    beside(&database)
        .execute("UPDATE vault_objects SET encoding = 0", [])
        .expect("an object stamped as an older encoder's");

    let summary = vaulted(&library, true)?;
    assert_eq!(summary.stats.walked, 0);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(library.vault_objects()?.len(), 1);
    Ok(())
}

#[test]
fn an_import_on_several_workers_keeps_every_row_and_shared_audio_once() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    for index in 0..12 {
        tree.write(
            &format!("{index:02}.wav"),
            &Wav::new()
                .frames(4_410 + 500 * (index % 3))
                .text(TITLE, &format!("take {index}"))
                .build(),
        );
    }

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    let asked = vaulted(&library, false)?;

    let summary = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                apply: true,
                workers: NonZeroUsize::new(4).expect("four workers"),
                ..ImportOptions::default()
            },
        )?
        .join()?;

    assert_eq!(summary.stats.vaulted, 12);
    assert_eq!(summary.stats.passed, 0);
    assert_eq!(
        summary
            .plan
            .vaulted
            .iter()
            .map(|kept| kept.wanted.from.clone())
            .collect::<Vec<_>>(),
        asked
            .plan
            .wanted
            .iter()
            .map(|wanted| wanted.from.clone())
            .collect::<Vec<_>>(),
        "the plan is not in the order the rows were asked for"
    );
    assert_eq!(library.vault_objects()?.len(), 3);
    assert_eq!(vault.holding().expect("a counted vault").objects, 3);
    for row in all(&library)? {
        let stood = library
            .stand_in()
            .stands_in(&row.location, row.span)
            .expect("the vault stands in for every row");
        assert!(stood.location.as_path().expect("a local object").is_file());
    }
    Ok(())
}

#[test]
fn a_vaulted_row_counts_its_plays_and_sits_in_a_playlist_as_the_file_it_came_from() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let row = all(&library)?.remove(0);
    let counted = library.track_played(&row.location, row.span)?;
    assert!(counted.is_some(), "a play of a vaulted row was not counted");

    let playlist = library.create_playlist("Evening")?;
    library.add_to_playlist(playlist, &[Cut::of(&row)])?;
    let cuts = library.playlist_cuts(playlist)?;
    assert_eq!(cuts[0].location.as_path(), Some(path.as_path()));
    Ok(())
}

#[test]
fn a_vaulted_row_outlives_the_file_it_came_from_and_keeps_its_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&path).expect("the source goes");

    let stats = scan(&library, &options(&tree))?;
    assert_eq!(stats.removed, 0, "the only copy's row was pruned");
    let rows = all(&library)?;
    assert_eq!(rows.len(), 1);
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_some(),
        "the row no longer reaches its object"
    );
    assert_eq!(library.vault_objects_nothing_names()?.len(), 0);
    Ok(())
}

#[test]
fn a_file_changed_where_it_stands_is_weighed_again_rather_than_played_from_its_old_object()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    fs::write(&path, Wav::new().text(TITLE, "Echoes (remastered)").build())
        .expect("the file is ripped again");
    let later = std::time::SystemTime::now() + Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_modified(later))
        .expect("a later mtime");
    scan(&library, &options(&tree))?;

    let rows = all(&library)?;
    assert!(
        library
            .stand_in()
            .stands_in(&rows[0].location, rows[0].span)
            .is_none(),
        "a file ripped again went on playing its old object"
    );
    assert_eq!(vaulted(&library, false)?.stats.walked, 1);
    Ok(())
}

#[test]
fn a_release_points_a_row_back_at_its_own_file_and_leaves_the_only_copy_where_it_is() -> Result<()>
{
    let tree = Tree::new();
    let held = Tree::new();
    tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());
    let gone = tree.write(
        "dogs.wav",
        &Wav::new().text(TITLE, "Dogs").frames(8_820).build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    fs::remove_file(&gone).expect("one source goes");

    let released = library.release_from_vault(&[])?;
    assert_eq!(released.released, 1);
    assert_eq!(
        released.stranded, 1,
        "the row the vault alone holds was released"
    );

    let stood_in = |title: &str| -> Result<bool> {
        let row = all(&library)?
            .into_iter()
            .find(|row| row.title == title)
            .expect("the row is still in the catalog");
        Ok(library
            .stand_in()
            .stands_in(&row.location, row.span)
            .is_some())
    };
    assert!(
        !stood_in("Echoes")?,
        "a released row still played from the vault"
    );
    assert!(stood_in("Dogs")?, "the only copy stopped being reached");

    assert_eq!(
        library.vault_objects_nothing_names()?.len(),
        1,
        "the released row's object is not left for --prune"
    );
    assert_eq!(
        vaulted(&library, false)?.stats.walked,
        1,
        "a released row is not weighed again by the next import"
    );
    assert!(matches!(
        library.release_from_vault(&[PathBuf::from("/nowhere")]),
        Err(Error::NotARoot { .. })
    ));
    Ok(())
}

#[test]
fn a_vaulted_row_is_passed_over_by_a_retagging() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let summary = retagged(&library, false)?;
    assert_eq!(summary.retagging.passed_over.len(), 1);
    assert_eq!(summary.retagging.passed_over[0].why, Unwritten::Vaulted);
    assert!(summary.retagging.writes.is_empty());
    Ok(())
}

#[test]
fn an_albums_cover_moves_into_the_vault_and_reads_back_as_a_drawable_picture() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;

    let album = library.albums(&AlbumQuery::default())?[0].id;
    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::File
    );

    let summary = vaulted(&library, true)?;
    assert_eq!(summary.stats.covers, 1);
    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::Vault
    );
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);

    let drawn = library.cover_art(album)?.expect("a cover read back");
    assert_eq!(drawn.format, ImageFormat::Png);

    let went_in = image::load_from_memory(&art.bytes)
        .expect("a picture")
        .to_rgba8();
    let came_out = image::load_from_memory(&drawn.bytes)
        .expect("a picture")
        .to_rgba8();
    assert_eq!(went_in, came_out);
    Ok(())
}

#[test]
fn a_vault_opened_through_another_spelling_or_moved_keeps_every_object_and_cover() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );
    let catalog = held.path().join("catalog.db");
    let first = held.path().join("vault");
    let through = held.path().join("through");
    fs::create_dir_all(&first).expect("a vault folder");
    std::os::unix::fs::symlink(&first, &through).expect("a second spelling of the vault");

    {
        let vault = Arc::new(Vault::open(&through).expect("a writable vault"));
        let library = Library::open_with_vault(&catalog, vault)?;
        scan(&library, &options(&tree))?;
        let summary = vaulted(&library, true)?;
        assert_eq!(summary.stats.vaulted, 1);
        assert_eq!(summary.stats.covers, 1);
    }

    let pruned = {
        let vault = Arc::new(Vault::open(&first).expect("a writable vault"));
        let library = Library::open_with_vault(&catalog, vault)?;
        library.prune_the_vault()?
    };
    assert_eq!(
        pruned,
        Pruned::default(),
        "a prune under another spelling took what the catalog names"
    );

    let moved = held.path().join("moved");
    fs::remove_file(&through).expect("the second spelling taken away");
    fs::rename(&first, &moved).expect("a moved vault");

    let vault = Arc::new(Vault::open(&moved).expect("a writable vault"));
    let library = Library::open_with_vault(&catalog, Arc::clone(&vault))?;
    assert_eq!(library.prune_the_vault()?, Pruned::default());
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    assert_eq!(vault.holding().expect("a counted vault").objects, 1);

    let rows = all(&library)?;
    let stood = library
        .stand_in()
        .stands_in(&rows[0].location, rows[0].span)
        .expect("the moved vault still stands in for the row");
    assert!(
        stood
            .location
            .as_path()
            .expect("a local object")
            .starts_with(vault.root())
    );

    let album = library.albums(&AlbumQuery::default())?[0].id;
    assert!(library.cover_art(album)?.is_some());
    let objects = library.vault_objects()?;
    assert!(objects[0].path.starts_with(vault.root()));
    Ok(())
}

#[test]
fn a_prune_takes_away_a_cover_nothing_names_and_leaves_the_rest() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build(),
    );

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;
    let stray = vault
        .keep_cover(&a_real_picture(8, 8))
        .expect("a cover nothing names");

    let pruned = library.prune_the_vault()?;
    assert_eq!(pruned.covers, 1);
    assert_eq!(pruned.objects, 0);
    assert!(!stray.path.exists());
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);
    Ok(())
}

#[test]
fn a_rescan_leaves_an_album_the_vault_covers_with_the_vaults_cover_alone() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    let track = |title: &str| {
        Wav::new()
            .text(TITLE, title)
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build()
    };
    tree.write("echoes.wav", &track("Echoes"));

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    tree.write("pillow.wav", &track("A Pillow of Winds"));
    scan(&library, &options(&tree))?;

    let album = library.albums(&AlbumQuery::default())?[0].id;
    assert_eq!(
        library
            .release_of(album)?
            .expect("a release row")
            .cover_source,
        CoverSource::Vault,
        "a rescan wrote the file's cover over the vault's"
    );
    Ok(())
}

#[test]
fn a_cover_the_vault_cannot_read_is_passed_over_and_the_import_carries_on() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .picture(b"\x89PNG\r\n\x1a\nnot a picture at all")
            .build(),
    );
    tree.write(
        "time.wav",
        &Wav::new()
            .text(TITLE, "Time")
            .text(ALBUM, "The Dark Side of the Moon")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;

    let summary = vaulted(&library, true)?;
    assert_eq!(
        summary.stats.vaulted, 2,
        "one unreadable cover ended the import"
    );
    assert_eq!(summary.stats.covers, 0);
    assert_eq!(summary.stats.covers_passed, 1);
    Ok(())
}

#[test]
fn a_cover_the_vault_already_holds_is_not_kept_a_second_time() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let art = a_real_picture(24, 18);
    let track = |title: &str| {
        Wav::new()
            .text(TITLE, title)
            .text(ALBUM, "Meddle")
            .picture(&art.bytes)
            .build()
    };
    tree.write("echoes.wav", &track("Echoes"));

    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    tree.write("pillow.wav", &track("A Pillow of Winds"));
    scan(&library, &options(&tree))?;
    let again = vaulted(&library, true)?;

    assert_eq!(again.stats.vaulted, 1);
    assert_eq!(
        vault.holding().expect("a counted vault").covers,
        1,
        "the vault's own cover was kept again under another name"
    );
    Ok(())
}

#[test]
fn the_rows_a_sheet_cuts_are_weighed_against_their_share_of_the_file_and_play_as_themselves()
-> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let file = tree.write("Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());
    let size = fs::metadata(&file).expect("a sized file").len();

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    let summary = vaulted(&library, true)?;

    assert_eq!(summary.stats.walked, 3);
    assert!(
        summary.stats.was_bytes <= size,
        "three rows of one file counted {} bytes of a {size}-byte file",
        summary.stats.was_bytes
    );
    for row in all(&library)? {
        assert_eq!(row.location.as_path(), Some(file.as_path()));
        if let Some(stood) = library.stand_in().stands_in(&row.location, row.span) {
            assert_eq!(stood.tags.title.as_deref(), Some(row.title.as_str()));
        }
    }
    Ok(())
}

#[test]
fn an_object_nothing_names_any_more_is_what_a_prune_reads_back() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    tree.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ALBUM, "Meddle")
            .build(),
    );

    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    assert!(library.vault_objects_nothing_names()?.is_empty());

    assert!(library.remove_root(tree.path())?);

    let loose = library.vault_objects_nothing_names()?;
    assert_eq!(loose.len(), 1);
    assert!(library.forget_vault_object(&loose[0].key)?);
    assert!(library.vault_objects()?.is_empty());
    Ok(())
}

#[test]
fn a_delivered_file_lands_in_the_vault_and_the_want_names_where_it_went() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&orbits))?;
    let want = wanted_san_tropez(&library)?;

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered.clone())));
    let summary = library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);

    let objects = library.vault_objects()?;
    assert_eq!(objects.len(), 1);
    assert_eq!(objects[0].form, Form::Flac);
    assert!(objects[0].path.starts_with(vault.root()));
    assert!(
        delivered.is_file(),
        "the delivered file is left where it stood"
    );

    let wants = library.wants()?;
    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&objects[0].path).to_uri().as_str())
    );

    let album = only_album(&library)?;
    let row = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "San Tropez")
        .expect("the release holds the row");
    let held = row
        .track
        .expect("the delivery is paired with the row it was wanted for");
    assert_eq!(wants[0].held, Some(held));
    assert!(
        library
            .missing_tracks(None, None)?
            .iter()
            .all(|missing| missing.title != "San Tropez")
    );

    let track = library.track(held)?.expect("the delivery is a track row");
    assert_eq!(track.title, "San Tropez");
    assert_eq!(track.album_id, Some(album.id));
    assert_eq!(track.location.as_path(), Some(objects[0].path.as_path()));
    assert!(
        track.delivered,
        "a row a provider delivered read back as scanned"
    );
    assert_eq!(
        titles(&library.search("Tropez", 10)?.tracks),
        vec!["San Tropez"]
    );
    let stood = library
        .stand_in()
        .stands_in(&track.location, track.span)
        .expect("the vault stands in for the delivered row");
    assert_eq!(stood.location.as_path(), Some(objects[0].path.as_path()));
    assert_eq!(stood.tags.title.as_deref(), Some("San Tropez"));
    assert_eq!(stood.tags.album.as_deref(), Some("Orbits"));

    scan(&library, &options(&orbits))?;
    assert!(
        library.track(held)?.is_some(),
        "a scan of the roots took away a row that belongs to none"
    );

    let summary = library
        .poll(
            inbox.registered(),
            PollOptions {
                again_after: Duration::ZERO,
                ..PollOptions::default()
            },
        )?
        .join()?;
    assert_eq!(summary.stats.asked, 0, "a held want was asked about again");

    library.unwant(want)?;
    assert_eq!(library.prune_the_vault()?.objects, 0);
    assert!(
        objects[0].path.is_file(),
        "a prune took away the object a delivered row is played from"
    );
    Ok(())
}

#[test]
fn a_delivered_row_is_forgotten_by_its_path_and_its_want_is_due_again() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let orbits = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&orbits))?;
    let want = wanted_san_tropez(&library)?;
    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;
    let object = library.vault_objects()?[0].path.clone();
    let scanned = all(&library)?
        .into_iter()
        .find(|track| track.title != "San Tropez")
        .expect("a scanned row");

    assert!(
        !library.forget_delivered(scanned.location.as_path().expect("a local row"))?,
        "a row scanned from a root was forgotten as a delivery"
    );
    assert!(library.forget_delivered(&object)?);
    assert!(!library.forget_delivered(&object)?);

    assert!(
        all(&library)?
            .iter()
            .all(|track| track.title != "San Tropez")
    );
    assert!(library.track(scanned.id)?.is_some());
    let wants = library.wants()?;
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].held, None);
    assert!(library.is_a_want_due(PollOptions::ASKING_EVERY_WANT)?);
    assert_eq!(library.prune_the_vault()?.objects, 1);
    Ok(())
}

#[test]
fn one_object_delivered_for_two_wants_is_searched_for_by_the_row_it_stayed() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let delivered = tree.write(
        "delivered.wav",
        &Wav::new().text(TITLE, "Echoes").frames(8_820).build(),
    );

    let scanned = orbits_tree();
    let (library, _vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&scanned))?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    rows.push(release_row(5, "Seamus", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    for missing in library.release_tracks(album.id)? {
        if missing.title == "San Tropez" || missing.title == "Seamus" {
            library.want(missing.id)?;
        }
    }

    let inbox = Arc::new(Offering::new("inbox", Delivering::File(delivered)));
    library
        .poll(inbox.registered(), PollOptions::default())?
        .join()?;

    let landed: Vec<TrackId> = library
        .wants()?
        .into_iter()
        .filter_map(|want| want.held)
        .collect();
    assert_eq!(landed.len(), 2);
    assert_eq!(landed[0], landed[1], "one object became two track rows");

    let track = library
        .track(landed[0])?
        .expect("the delivery is a track row");
    let other = if track.title == "Seamus" {
        "San Tropez"
    } else {
        "Seamus"
    };
    assert_eq!(
        titles(&library.search(&track.title, 10)?.tracks),
        vec![track.title.clone()]
    );
    assert!(
        library.search(other, 10)?.tracks.is_empty(),
        "the row was indexed under the later want's names"
    );
    Ok(())
}

#[test]
fn a_streamed_delivery_lands_in_the_vault_and_leaves_nothing_in_staging() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&orbits))?;
    wanted_san_tropez(&library)?;

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new().text(TITLE, "San Tropez").frames(8_820).build(),
        },
    ));
    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.unkept, 0);

    let objects = library.vault_objects()?;
    assert_eq!(objects.len(), 1);
    assert!(objects[0].path.starts_with(vault.root()));
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );

    let wants = library.wants()?;
    assert_eq!(
        wants[0].offered.as_deref(),
        Some(MediaLocation::local(&objects[0].path).to_uri().as_str())
    );
    let held = wants[0].held.expect("the stream is a track row");
    let track = library.track(held)?.expect("the delivered row");
    assert_eq!(track.location.as_path(), Some(objects[0].path.as_path()));

    let released = library.release_from_vault(&[])?;
    assert_eq!(
        released.released, 0,
        "a delivered row was released from the vault"
    );
    assert!(
        library
            .stand_in()
            .stands_in(&track.location, track.span)
            .is_some()
    );
    Ok(())
}

#[test]
fn a_poll_cancelled_mid_delivery_stops_reading_and_leaves_the_want_untried() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&orbits))?;
    wanted_san_tropez(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::Endless));
    let polling = library.poll(shop.registered(), PollOptions::default())?;
    thread::sleep(Duration::from_millis(100));
    polling.cancel();
    let summary = polling.join()?;

    assert!(summary.cancelled);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert!(library.vault_objects()?.is_empty());
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );
    let wants = library.wants()?;
    assert_eq!(wants[0].tried, None);
    Ok(())
}

#[test]
fn a_stream_that_stops_sending_is_given_up_as_late_and_the_want_stamped_as_tried() -> Result<()> {
    let held = Tree::new();
    let orbits = orbits_tree();
    let (library, vault) = opened_with_a_vault(&held)?;
    scan(&library, &options(&orbits))?;
    wanted_san_tropez(&library)?;

    let shop = Arc::new(Offering::new("shop", Delivering::Stalling));
    let began = Instant::now();
    let summary = library
        .poll(
            shop.registered(),
            PollOptions {
                answers_within: Duration::from_millis(200),
                ..PollOptions::default()
            },
        )?
        .join()?;

    assert!(began.elapsed() < Duration::from_secs(5));
    assert!(!summary.cancelled);
    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.late, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 0);
    assert!(library.vault_objects()?.is_empty());
    assert_eq!(
        std::fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );
    let wants = library.wants()?;
    assert!(wants[0].tried.is_some());
    assert_eq!(wants[0].offered, None);
    Ok(())
}

#[test]
fn a_streamed_delivery_with_no_vault_is_counted_unkept_and_offers_nothing() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    wanted_san_tropez(&library)?;

    let shop = Arc::new(Offering::new(
        "shop",
        Delivering::Bytes {
            key: "track/55391743",
            extension: "wav",
            bytes: Wav::new().frames(8_820).build(),
        },
    ));
    let summary = library
        .poll(shop.registered(), PollOptions::default())?
        .join()?;

    assert_eq!(summary.stats.offered, 1);
    assert_eq!(summary.stats.kept, 0);
    assert_eq!(summary.stats.unkept, 1);

    let wants = library.wants()?;
    assert!(wants[0].tried.is_some());
    assert_eq!(wants[0].offered, None);
    Ok(())
}

fn matching(library: &Library, text: &str) -> Result<Vec<String>> {
    Ok(library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

fn orbits_file(tree: &Tree, name: &str) -> MediaLocation {
    MediaLocation::local(
        fs::canonicalize(tree.path().join(name)).expect("the scanned file is there"),
    )
}

#[test]
fn a_track_is_found_by_the_words_it_sings_and_only_when_they_are_asked_for() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    library.keep_lyrics(
        &orbits_file(&tree, "1.wav"),
        None,
        Some("[00:01.00]Overhead the albatross\n[00:04.50]<00:04.50>hangs motionless"),
        true,
    )?;

    assert_eq!(
        matching(&library, "lyrics:albatross")?,
        ["One of These Days"]
    );
    assert_eq!(
        matching(&library, "lyric:\"albatross hangs\"")?,
        ["One of These Days"]
    );
    assert!(matching(&library, "albatross")?.is_empty());
    assert!(matching(&library, "lyrics:00")?.is_empty());
    assert_eq!(matching(&library, "pillow")?, ["A Pillow of Winds"]);

    scan(&library, &options(&tree))?;
    assert_eq!(
        matching(&library, "lyrics:albatross")?,
        ["One of These Days"]
    );
    Ok(())
}

#[test]
fn a_search_of_plain_words_is_offered_as_the_words_a_track_sings() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    library.keep_lyrics(
        &orbits_file(&tree, "2.wav"),
        None,
        Some("Overhead the albatross hangs motionless upon the air"),
        false,
    )?;

    let sung = library
        .sung("albatross hangs")?
        .expect("a track sings those words");
    assert_eq!(sung.query, "lyrics:\"albatross hangs\"");
    assert_eq!(sung.tracks, 1);
    assert_eq!(matching(&library, &sung.query)?, ["A Pillow of Winds"]);
    assert_eq!(library.sung("hangs albatross")?, None);
    assert_eq!(library.sung("year:1971 albatross")?, None);
    assert_eq!(library.sung("title:albatross")?, None);
    Ok(())
}

#[test]
fn a_search_reaches_the_rows_the_catalog_knows_it_is_short_of() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();
    rows.push(release_row(4, "San Tropez", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    assert_eq!(library.rematch(album.id)?, 3);

    let found = library.unheld_matching("tropez", None)?;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "San Tropez");
    assert_eq!(found[0].album, album.id);
    assert_eq!(library.unheld_matching("orbits san", None)?.len(), 1);
    assert_eq!(library.unheld_matching("title:tropez", None)?.len(), 1);
    assert!(library.unheld_matching("pillow", None)?.is_empty());
    assert!(library.unheld_matching("year:1998", None)?.is_empty());
    assert!(library.unheld_matching("tropez nowhere", None)?.is_empty());
    Ok(())
}

const ECHOES: &str = "d7c2a3f0-3333-4444-8555-666677778888";
const MEDDLE: &str = "e8d3b4a1-4444-4555-8666-777788889999";

fn meddle_release() -> Release {
    let mut echoes = release_row(2, "Echoes", Vec::new());
    echoes.recording = Some(mbid(ECHOES));
    let mut seamus = release_row(1, "Seamus", Vec::new());
    seamus.recording = Some(mbid(ANOTHER_RECORDING));
    Release {
        id: mbid(MEDDLE),
        group: None,
        title: "Meddle".to_owned(),
        credit: credited(Some("Pink Floyd"), None),
        date: Some("1971-10-30".to_owned()),
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: Some("Album".to_owned()),
        disambiguation: None,
        has_front_cover: false,
        links: Vec::new(),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![seamus, echoes],
        }],
    }
}

fn echoes_found() -> RecordingMatch {
    RecordingMatch {
        recording: mbid(ECHOES),
        score: 100,
        title: "Echoes".to_owned(),
        credit: credited(Some("Pink Floyd"), None),
        length: Some(Duration::from_secs(1_411)),
        isrcs: Vec::new(),
        releases: vec![
            RecordingRelease {
                id: mbid(HOURS),
                title: "Echoes: The Best Of".to_owned(),
                date: Some("2001-11-05".to_owned()),
                disc: None,
                position: None,
            },
            RecordingRelease {
                id: mbid(MEDDLE),
                title: "Meddle".to_owned(),
                date: Some("1971-10-30".to_owned()),
                disc: None,
                position: None,
            },
        ],
    }
}

#[test]
fn a_song_found_elsewhere_is_wanted_by_landing_the_release_it_first_came_out_on() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let fake = Fake::new(Canned {
        found_songs: vec![
            echoes_found(),
            recording_match(RECORDING, "Echoes"),
            recording_match(ANOTHER_RECORDING, "One of These Days"),
        ],
        releases: vec![meddle_release()],
        ..Canned::default()
    });

    let found = library.found_elsewhere(&fake, "echoes year:1971")?;
    assert_eq!(
        fake.calls(),
        vec![Called::FindSongs("echoes".to_owned())],
        "only the words were sent"
    );
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].title, "Echoes");
    assert_eq!(found[0].artist, "Pink Floyd");
    assert_eq!(
        found[0].release.as_ref().map(|release| release.id.as_str()),
        Some(MEDDLE)
    );
    assert!(library.found_elsewhere(&fake, "year:1971")?.is_empty());

    let want = library.want_found(&fake, &found[0])?;
    let wants = library.wants()?;
    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].title, "Echoes");
    assert_eq!(wants[0].album_title, "Meddle");
    assert_eq!(
        library.missing_tracks(None, None)?.len(),
        1,
        "a release the library holds nothing of lists only the row that was wanted"
    );
    assert_eq!(library.missing_counted(None)?.tracks, 1);
    let unheld = library.unheld_matching("echoes", None)?;
    assert_eq!(unheld.len(), 1);
    assert_eq!(unheld[0].want, Some(want));
    assert!(library.unheld_matching("seamus", None)?.is_empty());
    assert_eq!(
        library.albums(&AlbumQuery::default())?.len(),
        1,
        "an album holding no file is not browsed"
    );

    let again = library.found_elsewhere(&fake, "echoes")?;
    assert!(
        again.iter().all(|found| found.recording.as_str() != ECHOES
            && found.recording.as_str() != ANOTHER_RECORDING),
        "a recording the catalog names is not offered again"
    );
    assert_eq!(library.want_found(&fake, &found[0])?, want);

    scan(&library, &options(&tree))?;
    assert_eq!(library.wants()?.len(), 1, "a scan kept the wanted release");

    assert!(library.unwant(want)?);
    scan(&library, &options(&tree))?;
    assert!(library.unheld_matching("echoes", None)?.is_empty());
    Ok(())
}

#[test]
fn a_song_with_no_release_named_is_wanted_from_the_one_its_recording_first_came_out_on()
-> Result<()> {
    let library = Library::open_in_memory()?;
    let mut matched = echoes_found();
    matched.releases.clear();
    let mut recorded = matched.clone().into_recording();
    recorded.releases = echoes_found().releases;
    let fake = Fake::new(Canned {
        found_songs: vec![matched],
        recordings: vec![recorded],
        releases: vec![meddle_release()],
        ..Canned::default()
    });

    let found = library.found_elsewhere(&fake, "echoes")?;
    assert_eq!(found[0].release, None);
    library.want_found(&fake, &found[0])?;
    assert_eq!(library.wants()?[0].album_title, "Meddle");

    let mut lost = found[0].clone();
    lost.recording = mbid(RECORDING);
    assert!(matches!(
        library.want_found(&fake, &lost),
        Err(Error::Unreleased { .. })
    ));
    lost.release = Some(RecordingRelease {
        id: mbid(MEDDLE),
        title: "Meddle".to_owned(),
        date: None,
        disc: None,
        position: None,
    });
    assert!(matches!(
        library.want_found(&fake, &lost),
        Err(Error::NotOnTheRelease { .. })
    ));
    Ok(())
}

fn scanned_credits(files: &[(&str, &str, &str)]) -> Result<(Tree, Library)> {
    let tree = Tree::new();
    for (file, title, artist) in files {
        tree.write(
            file,
            &Wav::new().text(TITLE, title).text(ARTIST, artist).build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

fn titles_by(library: &Library, artist: resonate_core::ArtistId) -> Result<Vec<String>> {
    let mut titles: Vec<String> = library
        .tracks(&TrackQuery {
            artist: Some(artist),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect();
    titles.sort();
    Ok(titles)
}

#[test]
fn a_collaboration_is_listed_under_each_artist_it_credits_and_not_as_one_of_its_own() -> Result<()>
{
    let (_tree, library) = scanned_credits(&[
        ("cold.wav", "Cold", "Ada"),
        ("heat.wav", "Heat", "The Orbiters"),
        ("both.wav", "Both", "Ada & The Orbiters"),
    ])?;

    let names: Vec<String> = library
        .artists(&ArtistQuery::default())?
        .into_iter()
        .map(|artist| artist.name)
        .collect();
    assert!(
        !names.iter().any(|name| name == "Ada & The Orbiters"),
        "the collaboration was kept as an artist of its own: {names:?}"
    );

    let ada = artist_named(&library, "Ada")?;
    let orbiters = artist_named(&library, "The Orbiters")?;
    assert_eq!(titles_by(&library, ada.id)?, vec!["Both", "Cold"]);
    assert_eq!(titles_by(&library, orbiters.id)?, vec!["Both", "Heat"]);
    assert_eq!(ada.track_count, 2);
    assert_eq!(orbiters.track_count, 2);

    let both = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|track| track.title == "Both")
        .expect("the collaboration is scanned");
    assert_eq!(both.artist.as_deref(), Some("Ada & The Orbiters"));
    Ok(())
}

#[test]
fn a_name_whose_halves_name_nobody_held_is_one_artist() -> Result<()> {
    let (_tree, library) = scanned_credits(&[("song.wav", "Song", "Simon & Garfunkel")])?;

    let duo = artist_named(&library, "Simon & Garfunkel")?;
    assert_eq!(titles_by(&library, duo.id)?, vec!["Song"]);
    Ok(())
}

#[test]
fn a_collaboration_the_reference_cannot_name_is_asked_about_one_member_at_a_time() -> Result<()> {
    let (_tree, library) = scanned_credits(&[("both.wav", "Both", "Ada & The Orbiters")])?;
    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![
            ArtistMatch {
                mbid: mbid(ADA),
                ..artist_match(100, "Ada")
            },
            artist_match(100, "The Orbiters"),
        ],
        artists: vec![
            orbiters(),
            ArtistProfile {
                mbid: mbid(ADA),
                name: "Ada".to_owned(),
                ..orbiters()
            },
        ],
        ..Canned::default()
    }));
    enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindArtist)
        .collect();
    assert_eq!(
        searched,
        vec![
            Called::FindArtist("Ada & The Orbiters".to_owned()),
            Called::FindArtist("Ada".to_owned()),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );

    let ada = artist_named(&library, "Ada")?;
    let orbiters = artist_named(&library, "The Orbiters")?;
    assert_eq!(ada.mbid, Some(mbid(ADA)));
    assert_eq!(titles_by(&library, ada.id)?, vec!["Both"]);
    assert_eq!(titles_by(&library, orbiters.id)?, vec!["Both"]);
    assert!(
        fake.calls().contains(&Called::Artist(mbid(ORBITERS))),
        "a member billed in the pass was not asked about in it"
    );
    Ok(())
}

fn moved(from: &Path, to: &Path) {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).expect("a writable temporary directory");
    }
    fs::rename(from, to).expect("a file moved within the tree");
}

#[test]
fn a_file_moved_between_scans_keeps_its_row_its_plays_and_its_place_in_a_playlist() -> Result<()> {
    let tree = Tree::new();
    let before = tree.write(
        "loose/echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    tree.write(
        "loose/seamus.wav",
        &Wav::new().text(TITLE, "Seamus").build(),
    );
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let echoes = library
        .track_played(&MediaLocation::local(&before), None)?
        .expect("a counted play")
        .track;
    library.favour(Favoured::Track(echoes.id), true)?;
    let playlist = library.create_playlist("Meddle")?;
    library.add_to_playlist(playlist, &[Cut::whole(MediaLocation::local(&before))])?;

    let after = tree.path().join("filed/Pink Floyd/06 Echoes.wav");
    moved(&before, &after);
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 1);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    let row = library
        .track_at(&after, None)?
        .expect("the row followed the file");
    assert_eq!(row.id, echoes.id);
    assert_eq!(row.plays, 1);
    assert!(row.favourite.is_some());
    assert_eq!(all(&library)?.len(), 2);
    let entries = library.playlist_entries(playlist, None)?;
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].location().as_path(),
        Some(after.canonicalize().expect("the moved file").as_path())
    );
    Ok(())
}

#[test]
fn an_album_moved_into_a_folder_of_its_own_stays_the_album_it_was() -> Result<()> {
    let tree = Tree::new();
    for (file, title) in [
        ("01.wav", "One of These Days"),
        ("02.wav", "A Pillow of Winds"),
    ] {
        tree.write(
            &format!("rips/{file}"),
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, "Pink Floyd")
                .text(ALBUM, "Meddle")
                .build(),
        );
    }
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let album = only_album(&library)?;
    library.favour(Favoured::Album(album.id), true)?;

    for file in ["01.wav", "02.wav"] {
        moved(
            &tree.path().join("rips").join(file),
            &tree.path().join("Pink Floyd/Meddle").join(file),
        );
    }
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 2);
    let still = only_album(&library)?;
    assert_eq!(still.id, album.id);
    assert!(still.favourite.is_some());
    scan(&library, &options(&tree))?;
    assert_eq!(only_album(&library)?.id, album.id);
    Ok(())
}

#[test]
fn two_files_alike_in_every_way_are_not_guessed_between_when_they_move() -> Result<()> {
    let tree = Tree::new();
    let twin = Wav::new().text(TITLE, "Echoes").build();
    let first = tree.write("a/echoes.wav", &twin);
    let second = tree.write("b/echoes.wav", &twin);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    moved(&first, &tree.path().join("c/echoes.wav"));
    moved(&second, &tree.path().join("d/echoes.wav"));
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 0);
    assert_eq!(stats.added, 2);
    assert_eq!(stats.removed, 2);
    Ok(())
}

#[test]
fn two_files_alike_in_every_way_are_told_apart_by_the_folders_they_moved_with() -> Result<()> {
    let tree = Tree::new();
    let twin = Wav::new().text(TITLE, "Echoes").build();
    let first = tree.write("vinyl/echoes.wav", &twin);
    let second = tree.write("tape/echoes.wav", &twin);
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;
    let heard = library
        .track_played(&MediaLocation::local(&first), None)?
        .expect("a counted play")
        .track;

    let filed_first = tree.path().join("filed/vinyl/echoes.wav");
    let filed_second = tree.path().join("filed/tape/echoes.wav");
    moved(&first, &filed_first);
    moved(&second, &filed_second);
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 2);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    let row = library
        .track_at(&filed_first, None)?
        .expect("the row followed the file");
    assert_eq!(row.id, heard.id);
    assert_eq!(row.plays, 1);
    assert_eq!(
        library
            .track_at(&filed_second, None)?
            .expect("the twin followed its file")
            .plays,
        0
    );
    Ok(())
}

#[test]
fn a_file_copied_rather_than_moved_is_a_row_of_its_own() -> Result<()> {
    let tree = Tree::new();
    let source = tree.write("echoes.wav", &Wav::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;
    scan(&library, &options(&tree))?;

    fs::create_dir_all(tree.path().join("copy")).expect("a writable temporary directory");
    fs::copy(&source, tree.path().join("copy/echoes.wav")).expect("a copy");
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 0);
    assert_eq!(stats.added, 1);
    assert_eq!(all(&library)?.len(), 2);
    Ok(())
}
