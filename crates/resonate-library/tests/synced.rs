use std::{
    env, fs,
    io::{Cursor, Read},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use resonate_analysis::Watch;
use resonate_core::{Answer, Answered, Chromaprint, Link, Mbid, MediaLocation, Recording};
use resonate_library::{
    Agreement, Album, AlbumQuery, CoverArt, Credit, Cut, DeliveryFolder, Direction, HeardAs,
    HeldTags, ImageFormat, Layout, Library, LyricText, Medium, Release, ReleaseTrack, Result,
    SavedQuery, ScanOptions, ScanStats, SongFiled, SortOrder, SyncedQueryBody, TrackQuery, Vault,
    WAITS,
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const SECONDS: u32 = 3;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-synced-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self {
            root: root.canonicalize().expect("a canonical path"),
        }
    }

    fn song(&self, relative: &str, title: &str, artist: &str, album: &str) -> PathBuf {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable folder");
        }
        fs::write(&path, tagged_wav(title, artist, album)).expect("a written song");
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn tagged_wav(title: &str, artist: &str, album: &str) -> Vec<u8> {
    tagged_wav_opening_on(title, artist, album, 0)
}

fn tagged_wav_opening_on(title: &str, artist: &str, album: &str, first: u8) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let mut data = vec![0_u8; (RATE * SECONDS) as usize * usize::from(block_align)];
    data[0] = first;

    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut info = b"INFO".to_vec();
    for (id, value) in [(b"INAM", title), (b"IART", artist), (b"IPRD", album)] {
        let mut text = value.as_bytes().to_vec();
        text.push(0);
        chunk(&mut info, id, &text);
    }

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"LIST", &info);
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn syncsafe(value: usize) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn id3_frame(id: &[u8; 4], text: &str) -> Vec<u8> {
    let mut payload = vec![3_u8];
    payload.extend_from_slice(text.as_bytes());

    let mut frame = id.to_vec();
    frame.extend_from_slice(&syncsafe(payload.len()));
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&payload);
    frame
}

fn id3_tag(frames: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let body: Vec<u8> = frames
        .iter()
        .flat_map(|(id, text)| id3_frame(id, text))
        .collect();

    let mut tag = b"ID3".to_vec();
    tag.extend_from_slice(&[4, 0, 0]);
    tag.extend_from_slice(&syncsafe(body.len()));
    tag.extend_from_slice(&body);
    tag
}

fn declared_wav(frames: &[(&[u8; 4], &str)], distinct: u8) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let mut data = vec![0_u8; (RATE * SECONDS) as usize * usize::from(block_align)];
    *data.last_mut().expect("a sample") = distinct;

    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"id3 ", &id3_tag(frames));
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn float_wav(distinct: u8) -> Vec<u8> {
    let float_bits = 32_u16;
    let block_align = CHANNELS * float_bits / 8;
    let mut data = vec![0_u8; (RATE * SECONDS) as usize * usize::from(block_align)];
    *data.last_mut().expect("a sample") = distinct;

    let mut format = Vec::new();
    format.extend_from_slice(&3_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&float_bits.to_le_bytes());

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn scan(library: &Library, root: &Path) -> Result<ScanStats> {
    Ok(library
        .scan(ScanOptions {
            roots: vec![root.to_path_buf()],
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?
        .stats)
}

fn song(title: &str, artist: &str, album: &str) -> Recording {
    Recording {
        mbid: None,
        title: title.into(),
        artist: artist.into(),
        album: album.into(),
        length: Some(Duration::from_secs(u64::from(SECONDS))),
    }
}

fn titles(entries: &[Option<Recording>]) -> Vec<Option<&str>> {
    entries
        .iter()
        .map(|entry| entry.as_ref().map(|recording| recording.title.as_str()))
        .collect()
}

#[test]
fn a_playlist_reads_as_recordings_and_one_written_from_elsewhere_resolves_what_is_held()
-> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("Floyd/Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let fearless = tree.song(
        "Floyd/Meddle/fearless.wav",
        "Fearless",
        "Pink Floyd",
        "Meddle",
    );
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&there, &tree.root)?;

    here.start_playlist(
        "Mix",
        &[
            Cut::whole(MediaLocation::local(&echoes)),
            Cut::whole(MediaLocation::local(&fearless)),
            Cut::whole(MediaLocation::local("/nowhere/unscanned.wav")),
        ],
    )?;
    let read = here.synced_playlists()?;

    assert_eq!(read.len(), 1);
    assert_eq!(
        titles(&read[0].entries),
        [Some("Echoes"), Some("Fearless"), None]
    );
    let read_length = read[0].entries[0].as_ref().and_then(|entry| entry.length);
    assert_eq!(read_length, Some(Duration::from_secs(u64::from(SECONDS))));

    let written = there.write_synced_playlist(
        None,
        "Mix",
        &[
            song("fearless", "PINK FLOYD", "Meddle"),
            song("Not Here", "Nobody", "Nothing"),
            song("Echoes", "Pink Floyd", "Meddle"),
        ],
    )?;

    assert_eq!(written.name, "Mix");
    assert_eq!(titles(&written.entries), [Some("Fearless"), Some("Echoes")]);
    assert_eq!(there.synced_playlists()?, [written]);
    Ok(())
}

#[test]
fn a_saved_query_is_read_with_its_text_and_written_on_another_catalog_to_be_asked_there()
-> Result<()> {
    let tree = Tree::new();
    tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    tree.song("fearless.wav", "Fearless", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    here.create_playlist("Plain")?;
    here.save_query(
        "Floyd",
        &SavedQuery {
            text: Some("artist:floyd".into()),
            sort: SortOrder::Title,
            reading: Direction::Descending,
            limit: Some(1),
        },
    )?;

    let read = here.synced_queries()?;
    let there = Library::open_in_memory()?;
    scan(&there, &tree.root)?;
    let written = there
        .write_synced_query(None, &read[0].body)?
        .expect("a query this build reads");
    let entries = there.playlist_entries(written.id, None)?;

    assert_eq!(read.len(), 1);
    assert_eq!(read[0].body.name, "Floyd");
    assert_eq!(read[0].body.text.as_deref(), Some("artist:floyd"));
    assert_eq!(read[0].body.rows, Some(1));
    assert_eq!(written.body, read[0].body);
    assert_eq!(there.synced_queries()?.len(), 1);
    assert!(there.synced_playlists()?.is_empty());
    assert_eq!(entries.len(), 1);
    Ok(())
}

#[test]
fn a_query_written_over_itself_is_revised_and_a_code_this_build_lacks_writes_nothing() -> Result<()>
{
    let library = Library::open_in_memory()?;
    library.create_playlist("Floyd")?;
    let body = SyncedQueryBody {
        name: "Floyd".into(),
        text: Some("artist:floyd".into()),
        sort: 0,
        reading: 0,
        rows: None,
    };

    let first = library.write_synced_query(None, &body)?.expect("written");
    let revised = library
        .write_synced_query(
            Some((first.id, first.created)),
            &SyncedQueryBody {
                text: Some("artist:waters".into()),
                ..body.clone()
            },
        )?
        .expect("revised");
    let unknown = library.write_synced_query(
        None,
        &SyncedQueryBody {
            sort: 200,
            ..body.clone()
        },
    )?;

    assert_eq!(first.body.name, "Floyd (2)");
    assert_eq!(revised.id, first.id);
    assert_eq!(revised.body.text.as_deref(), Some("artist:waters"));
    assert_eq!(library.synced_queries()?.len(), 1);
    assert!(unknown.is_none());
    assert!(library.remove_synced_playlist(first.id, first.created)?);
    assert!(library.synced_queries()?.is_empty());
    Ok(())
}

#[test]
fn writing_over_a_playlist_ends_its_undo_steps_and_never_takes_another_name() -> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;
    scan(&library, &tree.root)?;
    let mix = library.create_playlist("Mix")?;
    library.add_to_playlist(mix, &[Cut::whole(MediaLocation::local(&echoes))])?;
    let other = library.create_playlist("Summer")?;
    assert!(library.undoable().is_some());

    let created = library
        .synced_playlists()?
        .into_iter()
        .find(|playlist| playlist.id == mix)
        .expect("held")
        .created;
    let written = library.write_synced_playlist(
        Some((mix, created)),
        "Summer",
        &[song("Echoes", "Pink Floyd", "Meddle")],
    )?;

    assert_eq!(written.id, mix);
    assert_eq!(written.name, "Summer (2)");
    assert!(
        library
            .undoable()
            .is_none_or(|undoable| undoable.playlist == other)
    );
    assert!(
        library
            .undo()?
            .is_none_or(|undone| undone.playlist == other)
    );
    assert_eq!(library.synced_playlists()?.len(), 1);
    assert!(library.remove_synced_playlist(mix, created)?);
    assert!(!library.remove_synced_playlist(mix, created)?);
    Ok(())
}

#[test]
fn a_playlist_from_elsewhere_under_a_name_taken_here_lands_under_a_free_one() -> Result<()> {
    let tree = Tree::new();
    tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;
    scan(&library, &tree.root)?;
    library.create_playlist("Mix")?;

    let written =
        library.write_synced_playlist(None, "Mix", &[song("Echoes", "Pink Floyd", "Meddle")])?;

    assert_eq!(written.name, "Mix (2)");
    assert_eq!(titles(&written.entries), [Some("Echoes")]);
    Ok(())
}

#[test]
fn a_hidden_favourite_is_still_a_favourite_to_the_devices_it_syncs_with() -> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;
    scan(&library, &tree.root)?;
    assert!(library.favour_recording(&song("Echoes", "Pink Floyd", "Meddle"), true)?);
    let held = library.track_at(&echoes, None)?.expect("held");

    library.hide_track(held.id, true)?;

    let favoured = library.favoured_recordings()?;
    assert_eq!(favoured.len(), 1);
    assert_eq!(favoured[0].title, "Echoes");
    Ok(())
}

#[test]
fn favourites_and_listens_travel_as_recordings_and_one_from_elsewhere_is_never_sent_back()
-> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;
    scan(&library, &tree.root)?;

    assert!(library.favour_recording(&song("Echoes", "Pink Floyd", "Meddle"), true)?);
    assert!(!library.favour_recording(&song("Gone", "Nobody", "Nothing"), true)?);
    let favoured = library.favoured_recordings()?;
    assert_eq!(favoured.len(), 1);
    assert_eq!(favoured[0].title, "Echoes");

    let counted = library
        .track_played(&MediaLocation::local(&echoes), None, Duration::from_secs(2))?
        .expect("a counted play");
    library.listened(counted.listen, Duration::from_secs(2))?;
    let listens = library.listens_to_sync(0, 10)?;
    assert_eq!(listens.len(), 1);
    assert_eq!(listens[0].recording.title, "Echoes");

    let before = library.synced_revision();
    assert!(library.credit_listen_elsewhere(
        &song("Echoes", "Pink Floyd", "Meddle"),
        &[1; 16],
        7,
        1_000,
        Duration::from_secs(60)
    )?);
    assert!(library.synced_revision() > before);
    assert_eq!(library.listens_to_sync(listens[0].id, 10)?, []);
    assert_eq!(library.track_at(&echoes, None)?.expect("held").plays, 2);

    let again = library.synced_revision();
    assert!(library.credit_listen_elsewhere(
        &song("Echoes", "Pink Floyd", "Meddle"),
        &[1; 16],
        7,
        1_000,
        Duration::from_secs(60)
    )?);
    assert_eq!(library.synced_revision(), again);
    assert_eq!(library.track_at(&echoes, None)?.expect("held").plays, 2);

    assert!(library.credit_listen_elsewhere(
        &song("Echoes", "Pink Floyd", "Meddle"),
        &[2; 16],
        7,
        1_000,
        Duration::from_secs(60)
    )?);
    assert_eq!(library.track_at(&echoes, None)?.expect("held").plays, 3);
    Ok(())
}

#[test]
fn a_song_from_another_device_is_filed_in_the_music_folder_and_catalogued() -> Result<()> {
    let tree = Tree::new();
    tree.song("held/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;
    scan(&library, &tree.root)?;
    let held = library.held_songs()?;
    assert_eq!(held.len(), 1);
    assert_eq!(
        held[0].size,
        fs::metadata(&held[0].path).expect("held").len()
    );

    library.deliver_into(Some(DeliveryFolder {
        path: tree.root.clone(),
        layout: Layout::default(),
    }));
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");
    let SongFiled::Filed(filed) = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?
    else {
        panic!("filed");
    };

    assert!(filed.starts_with(&tree.root));
    assert!(
        filed.ends_with("Meddle/San Tropez.wav"),
        "{}",
        filed.display()
    );
    let resolved = library.resolve_recording(&arriving)?.expect("catalogued");
    assert_eq!(resolved.location, MediaLocation::local(&filed));
    Ok(())
}

#[test]
fn a_song_from_another_device_whose_extension_climbs_out_of_the_folder_is_refused() -> Result<()> {
    let tree = Tree::new();
    let folder = tree.root.join("music");
    fs::create_dir_all(&folder).expect("a folder");
    let library = Library::open_in_memory()?;
    library.deliver_into(Some(DeliveryFolder {
        path: folder,
        layout: Layout::default(),
    }));
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let filed = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav/../../escaped.wav",
        &mut Cursor::new(bytes),
    )?;

    assert_eq!(filed, SongFiled::Refused);
    assert!(!tree.root.join("escaped.wav").exists());
    assert_eq!(fs::read_dir(&tree.root).expect("a folder").count(), 1);
    Ok(())
}

#[test]
fn a_song_from_another_device_that_does_not_decode_is_refused_and_not_left_behind() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.root.clone(),
        layout: Layout::default(),
    }));
    let arriving = song("Seamus", "Pink Floyd", "Meddle");

    let filed = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(b"RIFF this is no wave at all".to_vec()),
    )?;

    assert_eq!(filed, SongFiled::Refused);
    assert!(!tree.root.join("Meddle/Seamus.wav").exists());
    Ok(())
}

#[test]
fn a_song_from_another_device_with_no_music_folder_lands_in_the_open_vault() -> Result<()> {
    let tree = Tree::new();
    let vault = Arc::new(Vault::make(tree.root.join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(Arc::clone(&vault))?;
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let SongFiled::Vaulted(kept) = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?
    else {
        panic!("vaulted");
    };

    assert!(kept.starts_with(vault.root()));
    let resolved = library.resolve_recording(&arriving)?.expect("catalogued");
    assert_eq!(resolved.location, MediaLocation::local(&kept));
    let rows = library.tracks(&TrackQuery::default())?;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "San Tropez");
    assert_eq!(rows[0].artist.as_deref(), Some("Pink Floyd"));
    assert!(rows[0].album_id.is_some());
    let offered = library.held_songs()?;
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].path, kept);
    assert_eq!(offered[0].extension, "flac");
    Ok(())
}

#[test]
fn a_song_from_another_device_comes_back_nowhere_with_neither_folder_nor_vault() -> Result<()> {
    let library = Library::open_in_memory()?;
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let filed = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    assert_eq!(filed, SongFiled::Nowhere);
    Ok(())
}

#[test]
fn two_songs_of_one_album_from_another_device_land_in_the_vault_under_one_album() -> Result<()> {
    let tree = Tree::new();
    let vault = Arc::new(Vault::make(tree.root.join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(vault)?;
    for title in ["San Tropez", "Seamus"] {
        let arriving = song(title, "Pink Floyd", "Meddle");
        let mut bytes = tagged_wav(title, "Pink Floyd", "Meddle");
        if title == "Seamus" {
            *bytes.last_mut().expect("a sample") = 1;
        }
        library.file_song_from_elsewhere(
            &arriving,
            &HeldTags::default(),
            "wav",
            &mut Cursor::new(bytes),
        )?;
    }

    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].album_id, rows[1].album_id);
    Ok(())
}

#[test]
fn a_song_whose_audio_the_vault_already_holds_under_another_recording_is_refused() -> Result<()> {
    let tree = Tree::new();
    let vault = Arc::new(Vault::make(tree.root.join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(vault)?;
    let first = song("San Tropez", "Pink Floyd", "Meddle");
    let again = song("San Tropez (Edit)", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");
    library.file_song_from_elsewhere(
        &first,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes.clone()),
    )?;

    let filed = library.file_song_from_elsewhere(
        &again,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    assert_eq!(filed, SongFiled::Refused);
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), 1);
    Ok(())
}

#[test]
fn a_song_from_another_device_goes_to_the_vault_when_both_a_vault_and_a_music_folder_are_set()
-> Result<()> {
    let tree = Tree::new();
    let vault = Arc::new(Vault::make(tree.root.join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(Arc::clone(&vault))?;
    let folder = tree.root.join("music");
    fs::create_dir_all(&folder).expect("a folder");
    library.deliver_into(Some(DeliveryFolder {
        path: folder.clone(),
        layout: Layout::default(),
    }));
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let filed = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    let SongFiled::Vaulted(kept) = filed else {
        panic!("vaulted");
    };
    assert!(kept.starts_with(vault.root()));
    assert_eq!(fs::read_dir(&folder).expect("a folder").count(), 0);
    Ok(())
}

#[test]
fn a_song_from_another_device_with_a_folder_and_no_vault_is_still_filed_in_the_folder() -> Result<()>
{
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.root.clone(),
        layout: Layout::default(),
    }));
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let filed = library.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    assert!(matches!(filed, SongFiled::Filed(_)));
    Ok(())
}

#[test]
fn a_vaulted_song_is_passed_on_to_a_third_device_as_the_bytes_it_decodes_to() -> Result<()> {
    let tree = Tree::new();
    let first = Library::open_in_memory_with_vault(Arc::new(
        Vault::make(tree.root.join("first")).expect("a writable vault"),
    ))?;
    let second = Library::open_in_memory_with_vault(Arc::new(
        Vault::make(tree.root.join("second")).expect("a writable vault"),
    ))?;
    let arriving = song("San Tropez", "Pink Floyd", "Meddle");
    first.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(float_wav(1)),
    )?;

    let offered = first.held_songs()?;
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0].extension, "wav");
    let mut sent = first.open_song(&offered[0].path)?.expect("opens");
    assert_eq!(sent.size, offered[0].size);
    assert_eq!(sent.extension, "wav");
    let mut bytes = Vec::new();
    sent.read_to_end(&mut bytes).expect("read");
    assert_eq!(bytes.len() as u64, offered[0].size);
    assert_eq!(bytes, float_wav(1));

    let filed = second.file_song_from_elsewhere(
        &arriving,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    assert!(matches!(filed, SongFiled::Vaulted(_)));
    assert_eq!(second.held_songs()?.len(), 1);
    Ok(())
}

#[test]
fn a_plain_file_is_opened_to_send_as_it_stands() -> Result<()> {
    let tree = Tree::new();
    let path = tree.song("held/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let library = Library::open_in_memory()?;

    let mut opened = library.open_song(&path)?.expect("opens");
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes).expect("read");

    assert_eq!(opened.extension, "wav");
    assert_eq!(opened.size, bytes.len() as u64);
    assert_eq!(bytes, fs::read(&path).expect("the file"));
    assert!(library.open_song(&tree.root.join("gone.wav"))?.is_none());
    Ok(())
}

#[test]
fn songs_from_another_device_group_by_the_album_artist_and_year_their_files_declare() -> Result<()>
{
    let tree = Tree::new();
    let vault = Arc::new(Vault::make(tree.root.join("vault")).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(vault)?;
    let waters = song("San Tropez", "Roger Waters", "Meddle");
    let gilmour = song("Seamus", "David Gilmour", "Meddle");
    let first = declared_wav(
        &[
            (b"TIT2", "San Tropez"),
            (b"TPE1", "Roger Waters"),
            (b"TPE2", "Pink Floyd"),
            (b"TALB", "Meddle"),
            (b"TDRC", "1971"),
            (b"TRCK", "3/6"),
        ],
        0,
    );
    let second = declared_wav(
        &[
            (b"TIT2", "Seamus"),
            (b"TPE1", "David Gilmour"),
            (b"TPE2", "Pink Floyd"),
            (b"TALB", "Meddle"),
            (b"TDRC", "1971"),
        ],
        1,
    );
    library.file_song_from_elsewhere(
        &waters,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(first),
    )?;
    library.file_song_from_elsewhere(
        &gilmour,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(second),
    )?;

    let rows = library.tracks(&TrackQuery::default())?;
    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].album_id, rows[1].album_id);
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].title, "Meddle");
    assert_eq!(albums[0].artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(albums[0].year, Some(1971));
    assert_eq!(albums[0].artist_count, 2);
    Ok(())
}

#[test]
fn songs_passed_on_by_a_vaulted_device_keep_the_album_artist_year_and_totals_the_vault_dropped()
-> Result<()> {
    let tree = Tree::new();
    let first = Library::open_in_memory_with_vault(Arc::new(
        Vault::make(tree.root.join("first")).expect("a writable vault"),
    ))?;
    let without = Library::open_in_memory_with_vault(Arc::new(
        Vault::make(tree.root.join("without")).expect("a writable vault"),
    ))?;
    let with = Library::open_in_memory_with_vault(Arc::new(
        Vault::make(tree.root.join("with")).expect("a writable vault"),
    ))?;
    let waters = song("San Tropez", "Roger Waters", "Meddle");
    let gilmour = song("Seamus", "David Gilmour", "Meddle");
    let declared_by = |title, artist, sample| {
        declared_wav(
            &[
                (b"TIT2", title),
                (b"TPE1", artist),
                (b"TPE2", "Pink Floyd"),
                (b"TALB", "Meddle"),
                (b"TDRC", "1971"),
                (b"TRCK", "3/6"),
                (b"TPOS", "1/2"),
            ],
            sample,
        )
    };
    first.file_song_from_elsewhere(
        &waters,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(declared_by("San Tropez", "Roger Waters", 0)),
    )?;
    first.file_song_from_elsewhere(
        &gilmour,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(declared_by("Seamus", "David Gilmour", 1)),
    )?;

    for held in first.held_songs()? {
        let tags = first.song_tags(&held.path)?;
        let mut bytes = Vec::new();
        first
            .open_song(&held.path)?
            .expect("opens")
            .read_to_end(&mut bytes)
            .expect("read");

        assert_eq!(tags.album_artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(tags.date.as_deref(), Some("1971"));
        assert_eq!(tags.track_total, Some(6));
        assert_eq!(tags.disc_total, Some(2));

        without.file_song_from_elsewhere(
            &held.recording,
            &HeldTags::default(),
            "wav",
            &mut Cursor::new(bytes.clone()),
        )?;
        with.file_song_from_elsewhere(&held.recording, &tags, "wav", &mut Cursor::new(bytes))?;
    }

    let ungrouped = without.albums(&AlbumQuery::default())?;
    let grouped = with.albums(&AlbumQuery::default())?;

    assert_eq!(ungrouped.len(), 2);
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[0].artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(grouped[0].year, Some(1971));
    assert_eq!(grouped[0].artist_count, 2);
    Ok(())
}

#[test]
fn a_song_filed_in_a_music_folder_reports_no_tags_to_pass_on() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let path = tree.song("a.wav", "San Tropez", "Pink Floyd", "Meddle");

    assert_eq!(library.song_tags(&path)?, HeldTags::default());
    Ok(())
}

#[test]
fn a_song_from_another_device_is_held_by_what_was_asked_and_never_filed_twice() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.root.clone(),
        layout: Layout::default(),
    }));
    let asked = song(
        "San Tropez (2011 Remaster)",
        "Pink Floyd",
        "Meddle (Remastered)",
    );
    let bytes = tagged_wav("San Tropez", "Pink Floyd", "Meddle");

    let SongFiled::Filed(filed) = library.file_song_from_elsewhere(
        &asked,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes.clone()),
    )?
    else {
        panic!("filed");
    };
    let held = library.resolve_recording(&asked)?;
    let again = library.file_song_from_elsewhere(
        &asked,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(bytes),
    )?;

    assert_eq!(
        held.map(|cut| cut.location),
        Some(MediaLocation::local(&filed)),
        "a song is held by the recording it was asked for, whatever its file says"
    );
    assert_eq!(again, SongFiled::Filed(filed.clone()));
    let mut names: Vec<_> = fs::read_dir(filed.parent().expect("a folder"))
        .expect("a folder")
        .map(|entry| entry.expect("an entry").file_name())
        .collect();
    names.sort();
    assert_eq!(names, [filed.file_name().expect("a name").to_owned()]);
    Ok(())
}

#[test]
fn a_song_from_another_device_named_otherwise_is_written_into_its_playlist_and_favoured_as_its_file_names_it()
-> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    library.deliver_into(Some(DeliveryFolder {
        path: tree.root.clone(),
        layout: Layout::default(),
    }));
    let asked = song(
        "San Tropez (2011 Remaster)",
        "Pink Floyd",
        "Meddle (Remastered)",
    );
    let SongFiled::Filed(_) = library.file_song_from_elsewhere(
        &asked,
        &HeldTags::default(),
        "wav",
        &mut Cursor::new(tagged_wav("San Tropez", "Pink Floyd", "Meddle")),
    )?
    else {
        panic!("filed");
    };

    let held_as = library.held_as(&asked)?;
    let written = library.write_synced_playlist(None, "Mix", std::slice::from_ref(&asked))?;
    let favoured = library.favour_recording(&asked, true)?;

    assert_eq!(
        held_as
            .as_ref()
            .map(|own| (own.title.as_str(), own.album.as_str())),
        Some(("San Tropez", "Meddle"))
    );
    assert_eq!(written.entries, std::slice::from_ref(&held_as));
    assert!(favoured);
    assert_eq!(
        library.favoured_recordings()?,
        held_as.into_iter().collect::<Vec<_>>()
    );
    assert_eq!(
        library.held_as(&song("Echoes", "Pink Floyd", "Meddle"))?,
        None
    );
    Ok(())
}

#[test]
fn a_copy_numbered_beside_its_song_is_folded_into_it_and_one_that_differs_is_left() -> Result<()> {
    let tree = Tree::new();
    let trash = Tree::new();
    let original = tree.song("Echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let copy = tree.root.join("Echoes (1).wav");
    fs::copy(&original, &copy).expect("a copy");
    let other = tree.song("Fearless.wav", "Fearless", "Pink Floyd", "Meddle");
    let differing = tree.song(
        "Fearless (1).wav",
        "Fearless",
        "Pink Floyd",
        "Meddle (Live)",
    );
    let library = Library::open_in_memory()?;
    library.trash_into(Some(trash.root.clone()));
    scan(&library, &tree.root)?;
    let copied = library
        .track_at(&copy, None)?
        .expect("the copy was scanned");
    library.favour(resonate_library::Favoured::Track(copied.id), true)?;
    let playlist = library.create_playlist("Mix")?;
    library.add_to_playlist(
        playlist,
        &[Cut {
            location: MediaLocation::local(&copy),
            span: None,
        }],
    )?;

    let found = library.numbered_copies()?;
    let deleted = library.fold_copies(&found.numbered)?;

    assert_eq!(found.numbered.len(), 1);
    assert_eq!(found.numbered[0].kept, original);
    assert_eq!(found.numbered[0].copy, copy);
    assert_eq!(found.differing, std::slice::from_ref(&differing));
    assert_eq!(deleted.files, 1);
    assert!(!copy.exists());
    assert!(original.exists() && other.exists() && differing.exists());
    let kept = library.track_at(&original, None)?.expect("the song stays");
    assert!(
        kept.favourite.is_some(),
        "the copy's favourite is the song's"
    );
    let entries = library.playlist_entries(playlist, None)?;
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.cut.location.clone())
            .collect::<Vec<_>>(),
        [MediaLocation::local(&original)]
    );
    assert!(library.numbered_copies()?.numbered.is_empty());
    Ok(())
}

fn answers_of(answered: &[Answered]) -> Vec<(&str, &Answer)> {
    let mut answers = answered
        .iter()
        .map(|answered| (answered.recording.title.as_str(), &answered.answer))
        .collect::<Vec<_>>();
    answers.sort_by_key(|(_, answer)| matches!(answer, Answer::Study(_)));
    answers
}

#[test]
fn a_study_and_kept_words_land_on_a_catalog_holding_the_same_file_and_read_back_as_they_went()
-> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("Floyd/Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&there, &tree.root)?;
    let location = MediaLocation::local(&echoes);
    let track = here.track_held(&location, None)?.expect("a held track");
    let study = resonate_analysis::study(&here.sources(), &location, None, &Watch::default())
        .expect("a study");
    here.note_study(track, &study)?;
    let words = LyricText {
        text: "[00:01.00] Overhead the albatross".into(),
        synced: true,
        lyricsfile: None,
    };
    here.keep_lyrics(&location, None, Some(&words))?;

    let shared = here.answers_to_share(0, 10)?;
    let taken = shared
        .iter()
        .map(|answered| there.take_shared_answer(&answered.recording, &answered.answer))
        .collect::<Result<Vec<_>>>()?;
    let read_back = there.answers_to_share(0, 10)?;

    assert_eq!(shared.len(), 2);
    assert_eq!(taken, [true, true]);
    assert_eq!(answers_of(&read_back), answers_of(&shared));
    assert!(there.study_of(&location, None)?.is_some());
    assert_eq!(
        there
            .kept_lyrics(&location, None)?
            .and_then(|kept| kept.sung)
            .map(|sung| sung.text),
        Some(words.text)
    );
    Ok(())
}

#[test]
fn a_lyricsfile_one_catalog_keeps_lands_on_another_holding_the_song_with_plainer_words_and_never_over_richer()
-> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("Floyd/Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    let richer = Library::open_in_memory()?;
    for library in [&here, &there, &richer] {
        scan(library, &tree.root)?;
    }
    let location = MediaLocation::local(&echoes);
    let lines = LyricText {
        text: "[00:01.00] Overhead the albatross".into(),
        synced: true,
        lyricsfile: None,
    };
    let document = LyricText {
        lyricsfile: Some("version: '1.0'".into()),
        ..lines.clone()
    };
    let own = LyricText {
        lyricsfile: Some("version: '1.1'".into()),
        ..lines.clone()
    };
    here.keep_lyrics(&location, None, Some(&document))?;
    there.keep_lyrics(&location, None, Some(&lines))?;
    richer.keep_lyrics(&location, None, Some(&own))?;

    let wanted = there.lyricsfiles_wanted(0, 10)?;
    let shared = here
        .lyricsfile_to_share(&wanted[0])?
        .expect("a lyricsfile to share");
    let nothing_to_share = there.lyricsfile_to_share(&wanted[0])?;
    let other_song = Recording {
        title: "Fearless".into(),
        ..wanted[0].clone()
    };
    let strangers = there.take_shared_lyricsfile(&other_song, &shared)?;
    let over_richer = richer.take_shared_lyricsfile(&wanted[0], &shared)?;
    let taken = there.take_shared_lyricsfile(&wanted[0], &shared)?;
    let again = there.take_shared_lyricsfile(&wanted[0], &shared)?;

    assert_eq!(wanted.len(), 1);
    assert_eq!(wanted[0].title, "Echoes");
    assert!(here.lyricsfiles_wanted(0, 10)?.is_empty());
    assert!(richer.lyricsfiles_wanted(0, 10)?.is_empty());
    assert!(there.lyricsfiles_wanted(1, 10)?.is_empty());
    assert_eq!(shared.document, "version: '1.0'");
    assert_eq!(nothing_to_share, None);
    assert!(!strangers);
    assert!(!over_richer);
    assert!(taken);
    assert!(!again);
    assert_eq!(
        there
            .kept_lyrics(&location, None)?
            .and_then(|kept| kept.sung),
        Some(document)
    );
    assert_eq!(
        richer
            .kept_lyrics(&location, None)?
            .and_then(|kept| kept.sung),
        Some(own)
    );
    assert!(there.lyricsfiles_wanted(0, 10)?.is_empty());
    Ok(())
}

#[test]
fn a_study_of_another_encode_or_words_for_another_take_are_never_written() -> Result<()> {
    let here_tree = Tree::new();
    let there_tree = Tree::new();
    let echoes = here_tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let other_encode = there_tree.root.join("echoes.wav");
    fs::write(
        &other_encode,
        tagged_wav_opening_on("Echoes", "Pink Floyd", "Meddle", 9),
    )
    .expect("a written song");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &here_tree.root)?;
    scan(&there, &there_tree.root)?;
    let location = MediaLocation::local(&echoes);
    let track = here.track_held(&location, None)?.expect("a held track");
    let study = resonate_analysis::study(&here.sources(), &location, None, &Watch::default())
        .expect("a study");
    here.note_study(track, &study)?;

    let shared = here.answers_to_share(0, 10)?;
    let another_take = Recording {
        length: Some(Duration::from_secs(u64::from(SECONDS) + 30)),
        ..song("Echoes", "Pink Floyd", "Meddle")
    };
    let words = Answer::Lyrics(resonate_core::SharedLyrics {
        text: "Overhead the albatross".into(),
        synced: false,
    });

    assert_eq!(shared.len(), 1);
    assert!(!there.take_shared_answer(&shared[0].recording, &shared[0].answer)?);
    assert!(!there.take_shared_answer(&another_take, &words)?);
    assert!(
        there
            .study_of(&MediaLocation::local(&other_encode), None)?
            .is_none()
    );
    assert!(there.answers_to_share(0, 10)?.is_empty());
    Ok(())
}

fn studied_with_a_print(library: &Library, location: &MediaLocation) -> Result<Chromaprint> {
    let track = library.track_held(location, None)?.expect("a held track");
    let mut study = resonate_analysis::study(&library.sources(), location, None, &Watch::default())
        .expect("a study");
    let print = Chromaprint::new("AQADtEmSJEmSJEmS", Duration::from_secs(181)).expect("a print");
    study.print = Some(print.clone());
    library.note_study(track, &study)?;
    Ok(print)
}

#[test]
fn a_print_and_its_recognition_land_on_a_copy_that_is_the_same_file_and_are_weighed_against_its_own_name()
-> Result<()> {
    let tree = Tree::new();
    let renamed_tree = Tree::new();
    let other_tree = Tree::new();
    let echoes = tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let renamed = renamed_tree.song("time.wav", "Brain Damage", "Roger Waters", "The Wall");
    let another_encode = other_tree.root.join("echoes.wav");
    fs::write(
        &another_encode,
        tagged_wav_opening_on("Echoes", "Pink Floyd", "Meddle", 9),
    )
    .expect("a written song");
    let here = Library::open_in_memory()?;
    let agreeing = Library::open_in_memory()?;
    let disagreeing = Library::open_in_memory()?;
    let unstudied = Library::open_in_memory()?;
    let elsewhere = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&agreeing, &tree.root)?;
    scan(&disagreeing, &renamed_tree.root)?;
    scan(&unstudied, &tree.root)?;
    scan(&elsewhere, &other_tree.root)?;
    let location = MediaLocation::local(&echoes);
    let print = studied_with_a_print(&here, &location)?;
    let track = here.track_held(&location, None)?.expect("a held track");
    let heard = HeardAs {
        recording: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid"),
        score: 96,
        title: "Echoes".into(),
        artist: Some("Pink Floyd".into()),
    };
    here.note_recognition(track, Some(&heard), Agreement::Agrees)?;
    let elsewhere_at = MediaLocation::local(&another_encode);
    let elsewhere_track = elsewhere
        .track_held(&elsewhere_at, None)?
        .expect("a held track");
    let elsewhere_study =
        resonate_analysis::study(&elsewhere.sources(), &elsewhere_at, None, &Watch::default())
            .expect("a study");
    elsewhere.note_study(elsewhere_track, &elsewhere_study)?;
    for answered in here.answers_to_share(0, 10)? {
        for library in [&agreeing, &disagreeing] {
            library.take_shared_answer(&answered.recording, &answered.answer)?;
        }
    }

    let wanted = agreeing.prints_wanted(0, 10)?;
    let shared = here.print_to_share(wanted[0])?.expect("a print to share");
    let nothing_to_share = agreeing.print_to_share(wanted[0])?;
    let not_studied = unstudied.take_shared_print(wanted[0], &shared)?;
    let another_file = elsewhere.take_shared_print(wanted[0], &shared)?;
    let taken = agreeing.take_shared_print(wanted[0], &shared)?;
    let named_otherwise = disagreeing.take_shared_print(wanted[0], &shared)?;
    let again = agreeing.take_shared_print(wanted[0], &shared)?;

    let (_, agreeing_study) = agreeing.study_of(&location, None)?.expect("a study");
    let (_, disagreeing_study) = disagreeing
        .study_of(&MediaLocation::local(&renamed), None)?
        .expect("a study");
    assert_eq!(wanted.len(), 1);
    assert!(here.prints_wanted(0, 10)?.is_empty());
    assert_eq!(shared.print, print);
    assert_eq!(nothing_to_share, None);
    assert!(!not_studied);
    assert!(!another_file);
    assert!(taken);
    assert!(named_otherwise);
    assert!(!again);
    assert_eq!(agreeing_study.print, Some(print.clone()));
    assert_eq!(agreeing_study.heard_as, Some(heard.clone()));
    assert_eq!(agreeing_study.agreement, Some(Agreement::Agrees));
    assert!(agreeing_study.recognised.is_some());
    assert_eq!(disagreeing_study.print, Some(print));
    assert_eq!(disagreeing_study.heard_as, Some(heard));
    assert_eq!(disagreeing_study.agreement, Some(Agreement::Disagrees));
    assert!(agreeing.prints_wanted(0, 10)?.is_empty());
    assert!(elsewhere.print_to_share(wanted[0])?.is_none());
    Ok(())
}

#[test]
fn a_recognition_that_heard_nothing_travels_as_nothing_and_a_print_alone_leaves_the_recognition_owed()
-> Result<()> {
    let tree = Tree::new();
    let echoes = tree.song("echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&there, &tree.root)?;
    let location = MediaLocation::local(&echoes);
    studied_with_a_print(&here, &location)?;
    let track = here.track_held(&location, None)?.expect("a held track");
    let study = resonate_analysis::study(&there.sources(), &location, None, &Watch::default())
        .expect("a study");
    there.note_study(
        there.track_held(&location, None)?.expect("a held track"),
        &study,
    )?;
    let file = there.prints_wanted(0, 10)?[0];

    let printed_only = here.print_to_share(file)?.expect("a print to share");
    here.note_recognition(track, None, Agreement::Unheard)?;
    let heard_nothing = here.print_to_share(file)?.expect("a print to share");
    there.take_shared_print(file, &printed_only)?;
    let (_, owed) = there.study_of(&location, None)?.expect("a study");

    assert_eq!(printed_only.recognition, None);
    assert_eq!(
        heard_nothing.recognition,
        Some(resonate_core::SharedRecognition::Unheard)
    );
    assert!(owed.print.is_some());
    assert_eq!(owed.recognised, None);
    assert_eq!(there.prints_wanted(0, 10)?, [file]);

    there.take_shared_print(file, &heard_nothing)?;
    let (_, settled) = there.study_of(&location, None)?.expect("a study");

    assert_eq!(settled.heard_as, None);
    assert_eq!(settled.agreement, Some(Agreement::Unheard));
    assert!(settled.recognised.is_some());
    assert!(there.prints_wanted(0, 10)?.is_empty());
    Ok(())
}

fn meddle(titles: &[&str]) -> Release {
    Release {
        id: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("an mbid"),
        group: Some(Mbid::new("a5bdf1b3-0cd8-3a1e-9e3a-0a4b1a4c4f0e").expect("an mbid")),
        title: "Meddle".into(),
        credit: vec![Credit {
            name: "Pink Floyd".into(),
            joined_by: String::new(),
            mbid: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid")),
        }],
        date: Some("1971-10-30".into()),
        country: Some("GB".into()),
        label: Some("Harvest".into()),
        catalog_number: Some("SHVL 795".into()),
        barcode: None,
        kind: Some("Album".into()),
        disambiguation: None,
        has_front_cover: false,
        links: vec![Link::new(
            "discogs",
            "https://www.discogs.com/release/1".into(),
        )],
        media: vec![Medium {
            position: 1,
            format: Some("Vinyl".into()),
            title: None,
            tracks: titles
                .iter()
                .zip(1..)
                .map(|(title, position)| ReleaseTrack {
                    position,
                    number: position.to_string(),
                    title: (*title).into(),
                    artist: None,
                    recording: None,
                    track: None,
                    length: Some(Duration::from_secs(u64::from(SECONDS))),
                    isrcs: vec![format!("GBN9Y110000{position}")],
                    links: Vec::new(),
                })
                .collect(),
        }],
    }
}

fn album_of(library: &Library) -> Result<Album> {
    Ok(library
        .albums(&AlbumQuery::default())?
        .into_iter()
        .next()
        .expect("an album"))
}

#[test]
fn a_release_landed_on_one_catalog_lands_on_another_holding_the_album_and_is_not_asked_again()
-> Result<()> {
    let tree = Tree::new();
    tree.song(
        "Meddle/one.wav",
        "One of These Days",
        "Pink Floyd",
        "Meddle",
    );
    tree.song("Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&there, &tree.root)?;
    let release = meddle(&["One of These Days", "Echoes"]);
    let album = album_of(&here)?.id;
    here.land_release(album, &release)?;
    here.rematch(album)?;

    let shared = here.answers_to_share(0, 10)?;
    let taken = shared
        .iter()
        .map(|answered| there.take_shared_answer(&answered.recording, &answered.answer))
        .collect::<Result<Vec<_>>>()?;
    let read_back = there.answers_to_share(0, 10)?;
    let landed = album_of(&there)?;
    let floyd = there
        .artist_named("Pink Floyd")?
        .and_then(|artist| there.artist_detail(artist).transpose())
        .transpose()?
        .expect("an artist");

    assert_eq!(shared.len(), 1);
    assert!(matches!(&shared[0].answer, Answer::Release(_)));
    assert_eq!(taken, [true]);
    assert_eq!(landed.mbid, Some(release.id.clone()));
    assert_eq!(answers_of(&read_back), answers_of(&shared));
    assert_eq!(floyd.mbid, release.credit[0].mbid);
    assert!(there.albums_to_ask(WAITS, false)?.is_empty());
    assert!(there.tracks_to_ask(WAITS, false)?.is_empty());
    Ok(())
}

#[test]
fn a_release_the_album_held_there_does_not_strictly_match_is_never_written() -> Result<()> {
    let here_tree = Tree::new();
    let there_tree = Tree::new();
    let shorter_tree = Tree::new();
    for tree in [&here_tree, &there_tree] {
        tree.song(
            "Meddle/one.wav",
            "One of These Days",
            "Pink Floyd",
            "Meddle",
        );
        tree.song("Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    }
    there_tree.song("Meddle/fearless.wav", "Fearless", "Pink Floyd", "Meddle");
    shorter_tree.song("Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    let shorter = Library::open_in_memory()?;
    scan(&here, &here_tree.root)?;
    scan(&there, &there_tree.root)?;
    scan(&shorter, &shorter_tree.root)?;
    let album = album_of(&here)?.id;
    here.land_release(album, &meddle(&["One of These Days", "Echoes"]))?;
    here.rematch(album)?;

    let shared = here.answers_to_share(0, 10)?;
    let another_band = Recording {
        artist: "Pink Fluid".into(),
        ..shared[0].recording.clone()
    };

    assert_eq!(shared.len(), 1);
    assert!(there.take_shared_answer(&shared[0].recording, &shared[0].answer)?);
    assert!(!shorter.take_shared_answer(&shared[0].recording, &shared[0].answer)?);
    assert!(!there.take_shared_answer(&another_band, &shared[0].answer)?);
    assert_eq!(album_of(&there)?.mbid, None);
    assert_eq!(album_of(&shorter)?.mbid, None);
    assert!(there.answers_to_share(0, 10)?.is_empty());
    Ok(())
}

#[test]
fn a_release_shared_by_a_track_held_under_another_name_lands_through_any_other_track_it_holds()
-> Result<()> {
    let here_tree = Tree::new();
    let there_tree = Tree::new();
    here_tree.song(
        "Meddle/one.wav",
        "One of These Days",
        "Pink Floyd",
        "Meddle",
    );
    there_tree.song("Meddle/one.wav", "Un de ces jours", "Pink Floyd", "Meddle");
    for tree in [&here_tree, &there_tree] {
        tree.song("Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    }
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &here_tree.root)?;
    scan(&there, &there_tree.root)?;
    let release = meddle(&["One of These Days", "Echoes"]);
    let album = album_of(&here)?.id;
    here.land_release(album, &release)?;
    here.rematch(album)?;

    let shared = here.answers_to_share(0, 10)?;
    let taken = there.take_shared_answer(&shared[0].recording, &shared[0].answer)?;

    assert_eq!(shared.len(), 1);
    assert_eq!(shared[0].recording.title, "One of These Days");
    assert!(taken);
    assert_eq!(album_of(&there)?.mbid, Some(release.id));
    Ok(())
}

#[test]
fn a_cover_one_catalog_holds_for_a_release_lands_on_another_holding_it_without_one() -> Result<()> {
    let tree = Tree::new();
    tree.song("Meddle/echoes.wav", "Echoes", "Pink Floyd", "Meddle");
    let here = Library::open_in_memory()?;
    let there = Library::open_in_memory()?;
    scan(&here, &tree.root)?;
    scan(&there, &tree.root)?;
    let release = meddle(&["Echoes"]);
    let picture = [b"\x89PNG\r\n\x1a\n".as_slice(), &[7; 64]].concat();
    for library in [&here, &there] {
        let album = album_of(library)?.id;
        library.land_release(album, &release)?;
    }
    let covered = album_of(&here)?.id;
    here.land_archive_cover(
        covered,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: picture.clone(),
        },
    )?;

    let wanted = there.covers_wanted(10)?;
    let offered = here.cover_to_share(&release.id)?.expect("a cover to share");
    let not_a_picture = there.take_shared_cover(&release.id, b"not a picture")?;
    let taken = there.take_shared_cover(&release.id, &offered.bytes)?;
    let landed = there
        .cover_art(album_of(&there)?.id)?
        .expect("a cover landed");

    assert_eq!(wanted, std::slice::from_ref(&release.id));
    assert!(here.covers_wanted(10)?.is_empty());
    assert!(!not_a_picture);
    assert!(taken);
    assert_eq!(landed.bytes, picture);
    assert!(there.covers_wanted(10)?.is_empty());
    assert!(!there.take_shared_cover(&release.id, &offered.bytes)?);
    Ok(())
}
