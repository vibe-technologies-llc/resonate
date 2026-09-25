use std::{
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use resonate_core::{MediaLocation, SourceId, TrackHints};
use resonate_library::{
    Agreement, ArtistMatch, ArtistProfile, ArtistRelease, CoverArt, Credit, EnrichOptions,
    EnrichSummary, Fingerprinters, Fingerprints, GroupAsked, GroupMatch, ImportOptions, Isrc,
    Library, Link, Mbid, Printed, Recording, RecordingAsked, RecordingMatch, Reference, Release,
    ReleaseAsked, ReleaseGroup, ReleaseMatch, Result, ScanOptions, SortOrder, Sounded, Sources,
    StudyFilter, TrackQuery, Vault, Verdict,
};
use rustfft::{FftPlanner, num_complex::Complex};

const RATE: u32 = 44_100;
const SECONDS: u32 = 8;
const PERIOD: usize = 65_536;
const UTF8: u8 = 3;
const TITLE: &[u8; 4] = b"TIT2";
const ARTIST: &[u8; 4] = b"TPE1";
const ALBUM: &[u8; 4] = b"TALB";
const REPLAY_GAIN_REFERENCE_LUFS: f32 = -18.0;
const HEARD: &str = "d1e2f3a4-b5c6-4d7e-8f9a-0b1c2d3e4f5a";
const RIGHT: &str = "e2f3a4b5-c6d7-4e8f-9a0b-1c2d3e4f5a6b";

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-studies-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
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

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn frame(into: &mut Vec<u8>, id: &[u8; 4], value: &str) {
    let mut body = vec![UTF8];
    body.extend_from_slice(value.as_bytes());
    into.extend_from_slice(id);
    into.extend_from_slice(&synchsafe(body.len() as u32));
    into.extend_from_slice(&[0, 0]);
    into.extend_from_slice(&body);
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
}

fn noise_up_to(ceiling_hz: f32, seed: u32) -> Vec<f32> {
    let bin_hz = RATE as f32 / PERIOD as f32;
    let mut state = seed;
    let mut spectrum = vec![Complex::<f32>::default(); PERIOD];
    for bin in 1..PERIOD / 2 {
        let hz = bin as f32 * bin_hz;
        if hz > ceiling_hz {
            break;
        }
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let phase = (state >> 8) as f32 / (1 << 24) as f32 * std::f32::consts::TAU;
        spectrum[bin] = Complex::from_polar(1.0 / hz.sqrt(), phase);
        spectrum[PERIOD - bin] = spectrum[bin].conj();
    }
    FftPlanner::new()
        .plan_fft_inverse(PERIOD)
        .process(&mut spectrum);
    let peak = spectrum
        .iter()
        .map(|sample| sample.re.abs())
        .fold(0.0, f32::max);
    spectrum
        .iter()
        .map(|sample| sample.re / peak * 0.5)
        .collect()
}

#[derive(Clone, Copy)]
enum Sampled {
    Sixteen,
    Float,
}

impl Sampled {
    const fn format_tag(self) -> u16 {
        match self {
            Self::Sixteen => 1,
            Self::Float => 3,
        }
    }

    const fn bytes(self) -> u16 {
        match self {
            Self::Sixteen => 2,
            Self::Float => 4,
        }
    }

    fn write(self, into: &mut Vec<u8>, sample: f32) {
        let sixteen = (sample * 32_767.0) as i16;
        match self {
            Self::Sixteen => into.extend_from_slice(&sixteen.to_le_bytes()),
            Self::Float => into.extend_from_slice(&(f32::from(sixteen) / 32_768.0).to_le_bytes()),
        }
    }
}

fn song(ceiling_hz: f32, seed: u32, tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    song_in(Sampled::Sixteen, ceiling_hz, seed, tags)
}

fn song_in(sampled: Sampled, ceiling_hz: f32, seed: u32, tags: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let period = noise_up_to(ceiling_hz, seed);
    let mut data = Vec::new();
    for frame in 0..(RATE * SECONDS) as usize {
        sampled.write(&mut data, period[frame % PERIOD]);
        sampled.write(&mut data, period[(frame + PERIOD / 3) % PERIOD]);
    }

    let block = 2 * sampled.bytes();
    let mut format = Vec::new();
    format.extend_from_slice(&sampled.format_tag().to_le_bytes());
    format.extend_from_slice(&2_u16.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block)).to_le_bytes());
    format.extend_from_slice(&block.to_le_bytes());
    format.extend_from_slice(&(8 * sampled.bytes()).to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut id3 = Vec::new();
    for (id, value) in tags {
        frame(&mut id3, id, value);
    }
    let mut file = Vec::new();
    if !id3.is_empty() {
        file.extend_from_slice(b"ID3\x04\x00\x00");
        file.extend_from_slice(&synchsafe(id3.len() as u32));
        file.extend_from_slice(&id3);
    }
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

struct Silent {
    source: SourceId,
    known: Option<Recording>,
}

impl Silent {
    fn new() -> Self {
        Self {
            source: SourceId::new("musicbrainz").expect("a nameable source"),
            known: None,
        }
    }
}

impl Reference for Silent {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, _id: &Mbid) -> Result<Option<Release>> {
        Ok(None)
    }

    fn find_release(&self, _asked: &ReleaseAsked) -> Result<Vec<ReleaseMatch>> {
        Ok(Vec::new())
    }

    fn recording(&self, id: &Mbid) -> Result<Option<Recording>> {
        Ok(self.known.clone().filter(|known| &known.id == id))
    }

    fn recordings_of_isrc(&self, _isrc: &Isrc) -> Result<Vec<Recording>> {
        Ok(Vec::new())
    }

    fn find_recording(&self, _asked: &RecordingAsked) -> Result<Vec<RecordingMatch>> {
        Ok(Vec::new())
    }

    fn find_songs(&self, _words: &str) -> Result<Vec<RecordingMatch>> {
        Ok(Vec::new())
    }

    fn release_group(&self, _id: &Mbid) -> Result<Option<ReleaseGroup>> {
        Ok(None)
    }

    fn find_release_group(&self, _asked: &GroupAsked) -> Result<Vec<GroupMatch>> {
        Ok(Vec::new())
    }

    fn group_cover(&self, _group: &Mbid) -> Result<Option<CoverArt>> {
        Ok(None)
    }

    fn artist(&self, _id: &Mbid) -> Result<Option<ArtistProfile>> {
        Ok(None)
    }

    fn find_artist(&self, _name: &str) -> Result<Vec<ArtistMatch>> {
        Ok(Vec::new())
    }

    fn release_groups_of(&self, _artist: &Mbid) -> Result<Vec<ArtistRelease>> {
        Ok(Vec::new())
    }

    fn cover(&self, _release: &Mbid, _group: Option<&Mbid>) -> Result<Option<CoverArt>> {
        Ok(None)
    }

    fn portrait(&self, _links: &[Link]) -> Result<Option<CoverArt>> {
        Ok(None)
    }
}

struct ByEar {
    source: SourceId,
    heard: Vec<(&'static str, RecordingMatch)>,
    asked: AtomicU64,
}

impl ByEar {
    fn new(heard: Vec<(&'static str, RecordingMatch)>) -> Self {
        Self {
            source: SourceId::new("by-ear").expect("a nameable source"),
            heard,
            asked: AtomicU64::new(0),
        }
    }
}

impl Fingerprints for ByEar {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn recognise(&self, sounded: &Sounded) -> Result<Printed> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        assert!(!sounded.print.encoded().is_empty());
        let name = sounded
            .location
            .as_path()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let found: Vec<RecordingMatch> = self
            .heard
            .iter()
            .filter(|(file, _)| *file == name)
            .map(|(_, found)| found.clone())
            .collect();
        Ok(if found.is_empty() {
            Printed::Nothing
        } else {
            Printed::Recognised(found)
        })
    }
}

fn heard_as(recording: &str, title: &str, artist: &str, score: u8) -> RecordingMatch {
    RecordingMatch {
        recording: Mbid::new(recording).expect("an mbid"),
        score,
        title: title.to_owned(),
        credit: vec![Credit {
            name: artist.to_owned(),
            joined_by: String::new(),
            mbid: None,
        }],
        length: Some(Duration::from_secs(u64::from(SECONDS))),
        isrcs: Vec::new(),
        releases: Vec::new(),
    }
}

fn scanned(tree: &Tree) -> Result<Library> {
    let library = Library::open_in_memory()?;
    let summary = library
        .scan(ScanOptions {
            roots: vec![tree.path().to_path_buf()],
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?;
    assert!(!summary.cancelled);
    Ok(library)
}

fn rescanned(library: &Library, tree: &Tree) -> Result<()> {
    library
        .scan(ScanOptions {
            roots: vec![tree.path().to_path_buf()],
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?;
    Ok(())
}

fn enriched(
    library: &Library,
    reference: Silent,
    fingerprinters: Fingerprinters,
) -> Result<EnrichSummary> {
    let summary = library
        .enrich(
            Arc::new(reference),
            Arc::new(fingerprinters),
            EnrichOptions::default(),
        )?
        .join()?;
    assert!(!summary.cancelled);
    Ok(summary)
}

fn matched(library: &Library, text: &str) -> Result<Vec<String>> {
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

#[test]
fn a_studied_track_is_hinted_the_gain_its_loudness_and_its_albums_ask_for() -> Result<()> {
    let tree = Tree::new();
    let first = tree.write(
        "1.wav",
        &song(
            21_700.0,
            1,
            &[(TITLE, "One"), (ARTIST, "Ada"), (ALBUM, "Orbits")],
        ),
    );
    tree.write(
        "2.wav",
        &song(
            21_700.0,
            2,
            &[(TITLE, "Two"), (ARTIST, "Ada"), (ALBUM, "Orbits")],
        ),
    );
    let library = scanned(&tree)?;
    enriched(&library, Silent::new(), Fingerprinters::none())?;

    let (_, studied) = library
        .study_of(&MediaLocation::local(&first), None)?
        .expect("a study");
    let loudness = studied.loudness.expect("a measured loudness");
    let hints = library.hinting().hints(&MediaLocation::local(&first), None);
    let track = hints.measured.track.expect("a measured track gain").get();
    assert!(
        (track - (REPLAY_GAIN_REFERENCE_LUFS - loudness)).abs() < 1e-3,
        "a track measured at {loudness} LUFS was hinted {track} dB"
    );
    let album = hints.measured.album.expect("a measured album gain").get();
    assert!(
        (album - track).abs() < 1.0,
        "two tracks of one loudness were hinted an album gain of {album} dB against {track} dB"
    );

    tree.write(
        "3.wav",
        &song(
            21_700.0,
            3,
            &[(TITLE, "Three"), (ARTIST, "Ada"), (ALBUM, "Orbits")],
        ),
    );
    rescanned(&library, &tree)?;
    assert_eq!(
        library
            .hinting()
            .hints(&MediaLocation::local(&first), None)
            .measured
            .album,
        None,
        "an album one of whose tracks is unstudied was hinted a gain for all of it"
    );
    Ok(())
}

#[test]
fn every_track_is_studied_as_the_pass_runs_and_a_transcode_is_found_fake() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "genuine.wav",
        &song(21_700.0, 1, &[(TITLE, "Genuine"), (ARTIST, "Ada")]),
    );
    let fake = tree.write(
        "transcoded.wav",
        &song(16_000.0, 2, &[(TITLE, "Transcoded"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;

    let summary = enriched(&library, Silent::new(), Fingerprinters::none())?;
    assert_eq!(summary.stats.studied, 2);
    assert_eq!(summary.stats.fakes, 1);
    assert_eq!(summary.stats.recognised, 0);

    let fakes = library.studies(StudyFilter {
        fakes: true,
        ..StudyFilter::default()
    })?;
    assert_eq!(fakes.len(), 1);
    assert_eq!(fakes[0].title, "Transcoded");
    let cutoff = fakes[0].studied.cutoff.expect("a wall");
    assert!((15_700..=16_300).contains(&cutoff.hz), "{}", cutoff.hz);
    assert!(fakes[0].studied.print.is_some());

    let (_, studied) = library
        .study_of(&MediaLocation::local(&fake), None)?
        .expect("a study");
    assert_eq!(studied.verdict, Verdict::Fake);
    assert_eq!(studied.recognised, None);
    assert_eq!(library.studies(StudyFilter::default())?.len(), 2);

    let hints = library.hinting().hints(&MediaLocation::local(&fake), None);
    let true_peak = hints.true_peak.expect("a measured true peak").get();
    assert!(
        true_peak >= studied.peak && true_peak > 0.0,
        "the true peak {true_peak} reads under the sample peak {}",
        studied.peak
    );
    let lowpass = hints.lowpass.expect("the wall as a hint").centihertz() / 100;
    assert!((15_700..=16_300).contains(&lowpass), "{lowpass}");
    assert_eq!(
        library
            .hinting()
            .hints(&MediaLocation::local(tree.path().join("absent.wav")), None),
        TrackHints::default()
    );

    assert_eq!(matched(&library, "is:fake")?, vec!["Transcoded"]);
    assert_eq!(matched(&library, "-is:fake")?, vec!["Genuine"]);

    let again = enriched(&library, Silent::new(), Fingerprinters::none())?;
    assert_eq!(
        again.stats.studied, 0,
        "a study already held is not taken again"
    );
    Ok(())
}

#[test]
fn a_track_whose_audio_is_another_song_is_misnamed_and_one_that_agrees_is_not() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "right.wav",
        &song(21_700.0, 3, &[(TITLE, "Tuesday"), (ARTIST, "Ada")]),
    );
    let renamed = tree.write(
        "renamed.wav",
        &song(21_700.0, 4, &[(TITLE, "Tuesday Again"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;
    let ear = Arc::new(ByEar::new(vec![
        ("right.wav", heard_as(RIGHT, "Tuesday", "Ada", 97)),
        ("renamed.wav", heard_as(HEARD, "Wednesday", "Grace", 96)),
    ]));

    let summary = enriched(
        &library,
        Silent::new(),
        Fingerprinters::none().and(Arc::clone(&ear) as Arc<dyn Fingerprints>),
    )?;
    assert_eq!(summary.stats.studied, 2);
    assert_eq!(summary.stats.recognised, 2);
    assert_eq!(summary.stats.misnamed, 1);
    assert_eq!(
        ear.asked.load(Ordering::Relaxed),
        2,
        "each track is asked about once"
    );

    let (_, studied) = library
        .study_of(&MediaLocation::local(&renamed), None)?
        .expect("a study");
    assert_eq!(studied.agreement, Some(Agreement::Disagrees));
    let heard = studied.heard_as.expect("what it was heard as");
    assert_eq!(heard.title, "Wednesday");
    assert_eq!(heard.artist.as_deref(), Some("Grace"));
    assert_eq!(heard.score, 96);

    assert_eq!(matched(&library, "is:misnamed")?, vec!["Tuesday Again"]);
    let misnamed = library.studies(StudyFilter {
        misnamed: true,
        ..StudyFilter::default()
    })?;
    assert_eq!(misnamed.len(), 1);

    let again = enriched(
        &library,
        Silent::new(),
        Fingerprinters::none().and(Arc::clone(&ear) as Arc<dyn Fingerprints>),
    )?;
    assert_eq!(again.stats.recognised, 0);
    assert_eq!(
        ear.asked.load(Ordering::Relaxed),
        2,
        "a recognition held is not asked again"
    );
    Ok(())
}

#[test]
fn a_track_heard_as_another_song_takes_that_name_when_asked_and_keeps_it_through_a_rescan()
-> Result<()> {
    let tree = Tree::new();
    let renamed = tree.write(
        "renamed.wav",
        &song(21_700.0, 4, &[(TITLE, "Tuesday Again"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;
    let ear = Arc::new(ByEar::new(vec![(
        "renamed.wav",
        heard_as(HEARD, "Wednesday", "Grace", 96),
    )]));
    enriched(
        &library,
        Silent::new(),
        Fingerprinters::none().and(Arc::clone(&ear) as Arc<dyn Fingerprints>),
    )?;
    let track = library
        .track_at(&renamed, None)?
        .expect("the scanned track")
        .id;

    let taken = library
        .take_what_was_heard(track)?
        .expect("what the audio was heard as");
    assert_eq!(taken.title, "Wednesday");
    assert_eq!(taken.artist.as_deref(), Some("Grace"));
    assert_eq!(taken.recording.as_str(), HEARD);

    let named = library.track(track)?.expect("the renamed track");
    assert_eq!(named.title, "Wednesday");
    assert_eq!(named.artist.as_deref(), Some("Grace"));
    let recorded = |library: &Library| {
        library
            .shareable(track)
            .ok()
            .flatten()
            .and_then(|shared| shared.recording)
            .map(|recording| recording.as_str().to_owned())
    };
    assert_eq!(recorded(&library).as_deref(), Some(HEARD));
    let (_, studied) = library
        .study_of(&MediaLocation::local(&renamed), None)?
        .expect("a study");
    assert_eq!(studied.agreement, Some(Agreement::Agrees));
    assert!(matched(&library, "is:misnamed")?.is_empty());
    assert_eq!(matched(&library, "wednesday grace")?, vec!["Wednesday"]);

    library
        .scan(ScanOptions {
            roots: vec![tree.path().to_path_buf()],
            incremental: false,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?;
    let rescanned = library.track(track)?.expect("the rescanned track");
    assert_eq!(rescanned.title, "Wednesday");
    assert_eq!(recorded(&library).as_deref(), Some(HEARD));
    Ok(())
}

#[test]
fn a_track_heard_as_nothing_has_nothing_to_take() -> Result<()> {
    let tree = Tree::new();
    let quiet = tree.write(
        "quiet.wav",
        &song(21_700.0, 6, &[(TITLE, "Tuesday"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;
    let track = library.track_at(&quiet, None)?.expect("the scanned track");

    assert_eq!(library.take_what_was_heard(track.id)?, None);
    assert_eq!(
        library.track(track.id)?.expect("the track").title,
        "Tuesday"
    );
    Ok(())
}

#[test]
fn a_track_that_names_nothing_is_named_by_what_its_audio_was_heard_as() -> Result<()> {
    let tree = Tree::new();
    let unnamed = tree.write("untitled.wav", &song(21_700.0, 5, &[]));
    let library = scanned(&tree)?;
    let ear = Arc::new(ByEar::new(vec![(
        "untitled.wav",
        heard_as(HEARD, "Found by Ear", "Grace", 99),
    )]));
    let reference = Silent {
        known: Some(heard_as(HEARD, "Found by Ear", "Grace", 100).into_recording()),
        ..Silent::new()
    };

    enriched(
        &library,
        reference,
        Fingerprinters::none().and(Arc::clone(&ear) as Arc<dyn Fingerprints>),
    )?;

    let (id, studied) = library
        .study_of(&MediaLocation::local(&unnamed), None)?
        .expect("a study");
    assert_eq!(studied.agreement, Some(Agreement::Unnamed));
    let track = library.track(id)?.expect("the track");
    assert_eq!(track.title, "Found by Ear");
    assert_eq!(ear.asked.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn a_file_that_changed_forgets_its_study_and_is_studied_again() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write(
        "changing.wav",
        &song(21_700.0, 6, &[(TITLE, "Changing"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;
    enriched(&library, Silent::new(), Fingerprinters::none())?;
    let location = MediaLocation::local(&path);
    assert_eq!(
        library
            .study_of(&location, None)?
            .map(|(_, held)| held.verdict),
        Some(Verdict::Genuine)
    );

    rescanned(&library, &tree)?;
    assert!(
        library.study_of(&location, None)?.is_some(),
        "an unchanged file keeps its study"
    );

    tree.write(
        "changing.wav",
        &song(
            16_000.0,
            7,
            &[(TITLE, "Changing"), (ARTIST, "Ada"), (b"TALB", "Now")],
        ),
    );
    rescanned(&library, &tree)?;
    assert!(library.study_of(&location, None)?.is_none());

    let summary = enriched(&library, Silent::new(), Fingerprinters::none())?;
    assert_eq!(summary.stats.studied, 1);
    assert_eq!(
        library
            .study_of(&location, None)?
            .map(|(_, held)| held.verdict),
        Some(Verdict::Fake)
    );
    Ok(())
}

#[test]
fn a_vaulted_row_whose_file_has_gone_is_studied_out_of_its_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let path = tree.write(
        "floating.wav",
        &song_in(
            Sampled::Float,
            16_000.0,
            8,
            &[(TITLE, "Floating"), (ARTIST, "Ada")],
        ),
    );
    let vault = Arc::new(Vault::open(held.path()).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(vault)?;
    rescanned(&library, &tree)?;
    let imported = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                apply: true,
                ..ImportOptions::default()
            },
        )?
        .join()?;
    assert_eq!(imported.stats.vaulted, 1);
    let packed = fs::read_dir(held.path().join("audio"))
        .expect("an audio folder")
        .flat_map(|fan| fs::read_dir(fan.expect("a fan-out").path()).expect("a fan-out folder"))
        .filter_map(|object| object.ok())
        .any(|object| object.file_name().to_string_lossy().ends_with(".wav.zst"));
    assert!(packed, "a float source is kept as a packed WAVE");
    fs::remove_file(&path).expect("the original goes");

    let summary = enriched(&library, Silent::new(), Fingerprinters::none())?;
    assert_eq!(summary.stats.studied, 1);
    assert_eq!(
        library
            .study_of(&MediaLocation::local(&path), None)?
            .map(|(_, held)| held.verdict),
        Some(Verdict::Fake)
    );
    Ok(())
}

#[test]
fn a_lookup_with_studies_off_decodes_nothing() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write(
        "unstudied.wav",
        &song(21_700.0, 9, &[(TITLE, "Unstudied"), (ARTIST, "Ada")]),
    );
    let library = scanned(&tree)?;

    let summary = library
        .enrich(
            Arc::new(Silent::new()),
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                studies: false,
                ..EnrichOptions::default()
            },
        )?
        .join()?;
    assert_eq!(summary.stats.studied, 0);
    assert!(
        library
            .study_of(&MediaLocation::local(&path), None)?
            .is_none()
    );
    Ok(())
}
