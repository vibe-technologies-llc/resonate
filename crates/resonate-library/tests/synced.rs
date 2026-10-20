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

use resonate_core::{MediaLocation, Recording};
use resonate_library::{
    AlbumQuery, Cut, DeliveryFolder, Direction, Layout, Library, Result, SavedQuery, ScanOptions,
    ScanStats, SongFiled, SortOrder, SyncedQueryBody, TrackQuery, Vault,
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
    let block_align = CHANNELS * BITS / 8;
    let data = vec![0_u8; (RATE * SECONDS) as usize * usize::from(block_align)];

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
        1_000,
        Duration::from_secs(60)
    )?);
    assert!(library.synced_revision() > before);
    assert_eq!(library.listens_to_sync(listens[0].id, 10)?, []);
    assert_eq!(library.track_at(&echoes, None)?.expect("held").plays, 2);
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
    let SongFiled::Filed(filed) =
        library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?
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

    let SongFiled::Vaulted(kept) =
        library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?
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

    let filed = library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?;

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
        library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?;
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
    library.file_song_from_elsewhere(&first, "wav", &mut Cursor::new(bytes.clone()))?;

    let filed = library.file_song_from_elsewhere(&again, "wav", &mut Cursor::new(bytes))?;

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

    let filed = library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?;

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

    let filed = library.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?;

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
    first.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(float_wav(1)))?;

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

    let filed = second.file_song_from_elsewhere(&arriving, "wav", &mut Cursor::new(bytes))?;

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
    library.file_song_from_elsewhere(&waters, "wav", &mut Cursor::new(first))?;
    library.file_song_from_elsewhere(&gilmour, "wav", &mut Cursor::new(second))?;

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
