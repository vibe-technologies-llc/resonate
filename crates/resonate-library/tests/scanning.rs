use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    slice,
    sync::Arc,
    time::Duration,
};
#[cfg(unix)]
use std::{ffi::OsStr, os::unix::fs::MetadataExt as _, process};

use parking_lot::Mutex;
use resonate_core::{
    AlbumId, ChannelLayout, Frames, MediaLocation, Resumable, Resumption, SampleFormat, SampleRate,
    StreamSpec, TrackId, ZoneId,
};
use resonate_library::{
    AlbumQuery, ArtistQuery, Codec, CoverArt, Cut, Deleted, Error, Favoured, HiddenTracks,
    ImageFormat, Library, LyricsAhead, NamesMoved, Removal, Result, ScanOptions, Sources, Track,
    TrackQuery, Trashes, Window,
};

use crate::support::{
    ALBUM, ALBUM_ARTIST, ARTIST, MEDDLE_SHEET, RATE, TITLE, TRACK, TrashedByTheDesktop, Tree, Wav,
    YEAR, all, beside, link_to_directory, link_to_file, matched, moved, narrowed, one_song,
    only_album, options, png, scan, scanned_sheet, scanned_two_discs, settled, stored, titles,
};
#[cfg(unix)]
use crate::support::{Told, cancelled_once_a_batch_is_written, filed_under, many_short_songs};

#[test]
fn an_empty_library_opens_and_reads_back_as_empty() -> Result<()> {
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
fn a_scan_keeps_content_advisories_and_updates_them_on_a_rescan() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    for (advisory, expected) in [("1", Some(true)), ("2", Some(false)), ("0", None)] {
        tree.write(
            "one.wav",
            &Wav::new()
                .text(TITLE, "A song")
                .described("ITUNESADVISORY", advisory)
                .build(),
        );

        let mut scanning = options(&tree);

        scanning.incremental = false;
        scan(&library, &scanning)?;

        let tracks = all(&library)?;

        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].explicit, expected);
    }
    Ok(())
}

fn a_variable_rate_stream() -> Vec<u8> {
    let mut file = Vec::new();

    for (header, length, count) in [
        ([0xff, 0xfb, 0x10, 0x00], 104, 40),
        ([0xff, 0xfb, 0xe0, 0x00], 1_044, 60),
    ] {
        for _ in 0..count {
            file.extend_from_slice(&header);
            file.resize(file.len() + length - header.len(), 0);
        }
    }
    file
}

#[test]
fn a_scan_keeps_the_frames_it_walked_and_the_catalog_hands_them_to_playback() -> Result<()> {
    let tree = Tree::new();
    let bytes = a_variable_rate_stream();

    let path = tree.write("walked.mp3", &bytes);

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;
    let walked = library
        .hinting()
        .walked(&MediaLocation::local(&path))
        .expect("the scan kept what it walked");

    assert_eq!(stats.failed.total(), 0);
    assert_eq!(walked.size(), bytes.len() as u64);
    assert!(
        walked.written().starts_with("1 115200 "),
        "{}",
        walked.written()
    );
    assert!(
        library
            .hinting()
            .walked(&MediaLocation::local(tree.path().join("absent.mp3")))
            .is_none()
    );
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
fn hidden_trash_and_system_folders_and_dot_files_are_passed_over() -> Result<()> {
    let tree = Tree::new();

    tree.write("album/one.wav", &Wav::new().text(TITLE, "one").build());
    tree.write("album/._one.wav", b"Mac OS X        AppleDouble");
    tree.write(
        "album/.one.4242-0.wav",
        &Wav::new().text(TITLE, "staged").build(),
    );
    tree.write(
        ".Trash-1000/files/old.wav",
        &Wav::new().text(TITLE, "trashed").build(),
    );
    tree.write(
        ".stversions/album/one~1.wav",
        &Wav::new().text(TITLE, "versioned").build(),
    );
    tree.write(
        "$RECYCLE.BIN/S-1-5/gone.wav",
        &Wav::new().text(TITLE, "recycled").build(),
    );
    tree.write(
        "lost+found/#123.wav",
        &Wav::new().text(TITLE, "found").build(),
    );

    let library = Library::open_in_memory()?;
    let stats = scan(&library, &options(&tree))?;

    let titles: Vec<String> = all(&library)?
        .into_iter()
        .map(|track| track.title)
        .collect();

    assert_eq!(titles, ["one"]);
    assert_eq!(
        stats.failed.total(),
        0,
        "a passed-over file was read and failed"
    );
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

#[cfg(unix)]
#[test]
fn a_file_the_watch_heard_gone_is_forgotten_beside_a_name_no_row_can_hold() -> Result<()> {
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
        root.join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(
            b"caf\xe9.wav",
        )),
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
fn a_file_forgotten_as_gone_keeps_its_listens_and_has_them_back_when_it_returns() -> Result<()> {
    let tree = Tree::new();
    let bytes = Wav::new().text(TITLE, "Echoes").build();

    let echoes = tree.write("echoes.wav", &bytes);

    tree.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&echoes), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }

    let before = library.statistics(Window::Everything)?;

    fs::remove_file(&echoes).expect("the file goes");

    assert_eq!(library.forget_the_gone(slice::from_ref(&echoes))?, 1);

    let after = library.statistics(Window::Everything)?;

    assert_eq!(titles(&all(&library)?), vec!["Dogs"]);
    assert_eq!(before.plays, 2);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);

    tree.write("echoes.wav", &bytes);
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.track_at(&echoes, None)?.expect("held again").plays,
        2
    );
    assert_eq!(library.statistics(Window::Everything)?.plays, 2);
    Ok(())
}

#[test]
#[cfg(unix)]
fn deleting_a_track_removes_its_file_and_its_row_and_leaves_the_rest() -> Result<()> {
    let tree = Tree::new();
    let doomed = tree.write(
        "album/doomed.wav",
        &Wav::new().text(TITLE, "doomed").build(),
    );
    let kept = tree.write("album/kept.wav", &Wav::new().text(TITLE, "kept").build());

    let data = Tree::new();
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let track = all(&library)?
        .into_iter()
        .find(|track| track.title == "doomed")
        .map(|track| track.id)
        .expect("the track was scanned");

    let deleted = library.delete_tracks(&[track])?;

    assert_eq!(
        deleted,
        Deleted {
            tracks: 1,
            files: 1,
            kept: 0,
        }
    );
    assert!(!doomed.exists());
    assert!(kept.exists());
    assert!(data.path().join("Trash/files/doomed.wav").is_file());

    let info = fs::read_to_string(data.path().join("Trash/info/doomed.wav.trashinfo"))
        .expect("a record of where the file came from");
    let written = format!(
        "[Trash Info]\nPath={}\nDeletionDate=",
        doomed.display().to_string().replace(' ', "%20")
    );

    assert!(info.starts_with(&written), "{info}");
    assert_eq!(titles(&all(&library)?), vec!["kept"]);
    assert!(matches!(
        library.delete_tracks(&[track]),
        Err(Error::UnknownTrack(unknown)) if unknown == track
    ));
    Ok(())
}

#[test]
#[cfg(unix)]
fn deleting_a_track_takes_its_lyrics_and_the_sheet_that_cuts_it_alone_and_the_folder_it_emptied_and_keeps_the_listens()
-> Result<()> {
    const SHEET: &str =
        "FILE \"song.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Song\"\n    INDEX 01 00:00:00\n";
    const SHARED_SHEET: &str = "FILE \"x.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"X\"\n    INDEX 01 00:00:00\nFILE \"y.wav\" WAVE\n  TRACK 02 AUDIO\n    TITLE \"Y\"\n    INDEX 01 00:00:00\n";

    let tree = Tree::new();
    let data = Tree::new();
    let song = tree.write("album/one/song.wav", &Wav::new().frames(44_100).build());
    let lyrics = tree.write("album/one/song.lrc", b"[00:01.00]words");
    let sheet = tree.write("album/one/song.cue", SHEET.as_bytes());
    let note = tree.write("album/one/notes.txt", b"liner notes");

    tree.write("album/two/x.wav", &Wav::new().frames(44_100).build());
    tree.write("album/two/y.wav", &Wav::new().frames(22_050).build());

    let shared = tree.write("album/two/both.cue", SHARED_SHEET.as_bytes());
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let song_row = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Song")
        .expect("the sheet's row");

    library.track_played(&song_row.location, song_row.span, Duration::ZERO)?;

    let before = library.statistics(Window::Everything)?;

    library.delete_tracks(&[song_row.id])?;

    assert!(!song.exists() && !lyrics.exists() && !sheet.exists());
    assert!(note.exists(), "a file that is not the song's was taken");
    assert!(data.path().join("Trash/files/song.lrc").is_file());
    assert!(data.path().join("Trash/files/song.cue").is_file());
    assert!(
        note.parent().expect("a folder").is_dir(),
        "a folder still holding a file was removed"
    );

    let after = library.statistics(Window::Everything)?;

    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);

    let x = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "X")
        .expect("the shared sheet's first row");

    library.delete_tracks(&[x.id])?;

    assert!(
        shared.exists(),
        "a sheet cutting another file too was taken"
    );
    assert!(tree.path().join("album/two").is_dir());

    let emptied = tree.write("album/three/alone.wav", &Wav::new().frames(8_820).build());

    scan(&library, &options(&tree))?;

    let alone = library
        .track_at(&emptied, None)?
        .expect("the file was scanned");

    library.delete_tracks(&[alone.id])?;

    assert!(!tree.path().join("album/three").exists());
    assert!(tree.path().is_dir(), "the root itself was pruned");
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_track_deleted_and_restored_gets_back_its_listens_as_they_were_and_tells_none_of_them_again()
-> Result<()> {
    const ORIGIN: [u8; 16] = [9; 16];

    let tree = Tree::new();
    let data = Tree::new();
    let cold = tree.write(
        "cold.wav",
        &Wav::new()
            .text(TITLE, "Cold")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Winter")
            .build(),
    );
    let recording = resonate_core::Recording {
        mbid: None,
        title: "Cold".into(),
        artist: "Ada".into(),
        album: "Winter".into(),
        length: Some(Duration::from_millis(100)),
    };
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let told = Told::default();

    library.submit_listens(&told)?;

    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&cold), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }
    library.settle_listens_after(Duration::ZERO);

    let synced = library.listens_to_sync(0, 10)?;
    let mark = synced.last().expect("the listens to sync").id;

    assert!(library.credit_listen_elsewhere(
        &recording,
        &ORIGIN,
        7,
        1_000,
        Duration::from_secs(60)
    )?);
    assert_eq!(library.submit_listens(&told)?.submitted, 2);

    let before = library.statistics(Window::Everything)?;
    let held = library.track_at(&cold, None)?.expect("held");

    library.delete_tracks(&[held.id])?;
    fs::rename(data.path().join("Trash/files/cold.wav"), &cold).expect("a restored file");
    scan(&library, &options(&tree))?;

    assert_eq!(synced.len(), 2);
    assert_eq!(library.listens_to_sync(mark, 10)?, []);
    assert_eq!(library.track_at(&cold, None)?.expect("held again").plays, 3);
    assert!(library.credit_listen_elsewhere(
        &recording,
        &ORIGIN,
        7,
        1_000,
        Duration::from_secs(60)
    )?);
    assert_eq!(library.track_at(&cold, None)?.expect("held again").plays, 3);
    assert_eq!(library.submit_listens(&told)?.submitted, 0);
    assert_eq!(told.titles(), vec![vec!["Cold", "Cold"]]);
    assert_eq!(library.statistics(Window::Everything)?, before);
    Ok(())
}

#[test]
#[cfg(unix)]
fn deleting_two_files_of_one_name_keeps_both_in_the_trash() -> Result<()> {
    let tree = Tree::new();
    let data = Tree::new();

    tree.write("a/song.wav", &Wav::new().text(TITLE, "First").build());
    tree.write("b/song.wav", &Wav::new().text(TITLE, "Second").build());

    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let both: Vec<TrackId> = all(&library)?.iter().map(|track| track.id).collect();

    let deleted = library.delete_tracks(&both)?;

    assert_eq!(deleted.files, 2);

    let held = |folder: &str| {
        fs::read_dir(data.path().join("Trash").join(folder))
            .expect("a trash folder")
            .count()
    };

    assert_eq!(held("files"), 2);
    assert_eq!(held("info"), 2);
    Ok(())
}

#[test]
fn a_file_with_no_trash_to_go_to_is_kept_with_its_row() -> Result<()> {
    let tree = Tree::new();
    let data = Tree::new();
    let song = tree.write("song.wav", &Wav::new().text(TITLE, "Song").build());
    let blocked = data.write("not-a-folder", b"");
    let library = Library::open_in_memory()?;

    library.trash_into(Some(blocked));
    scan(&library, &options(&tree))?;

    let track = all(&library)?[0].id;

    let deleted = library.delete_tracks(&[track])?;

    assert_eq!(
        deleted,
        Deleted {
            tracks: 0,
            files: 0,
            kept: 1,
        }
    );
    assert!(song.exists());
    assert_eq!(all(&library)?.len(), 1);
    Ok(())
}

#[test]
fn a_file_is_trashed_through_the_desktops_trash_where_one_is_given_and_never_by_hand() -> Result<()>
{
    let tree = Tree::new();
    let data = Tree::new();
    let song = tree.write("song.wav", &Wav::new().text(TITLE, "Song").build());
    let desktop = Arc::new(TrashedByTheDesktop {
        asked: Mutex::new(Vec::new()),
        answer: Removal::Removed,
    });
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.root.clone()));
    library.trash_through(Some(Arc::clone(&desktop) as Arc<dyn Trashes>));
    scan(&library, &options(&tree))?;

    let track = all(&library)?[0].id;

    let deleted = library.delete_tracks(&[track])?;

    assert_eq!(deleted.files, 1);

    let asked = desktop.asked.lock().clone();

    assert_eq!(asked.len(), 1);
    assert!(asked[0].ends_with("song.wav"));
    assert!(!song.exists());
    assert!(!data.root.join("Trash").exists());
    assert!(all(&library)?.is_empty());
    Ok(())
}

#[test]
fn a_file_the_desktops_trash_would_not_take_is_kept_with_its_row() -> Result<()> {
    let tree = Tree::new();
    let song = tree.write("song.wav", &Wav::new().text(TITLE, "Song").build());
    let desktop = Arc::new(TrashedByTheDesktop {
        asked: Mutex::new(Vec::new()),
        answer: Removal::Kept,
    });
    let library = Library::open_in_memory()?;

    library.trash_through(Some(desktop as Arc<dyn Trashes>));
    scan(&library, &options(&tree))?;

    let track = all(&library)?[0].id;

    let deleted = library.delete_tracks(&[track])?;

    assert_eq!(
        deleted,
        Deleted {
            tracks: 0,
            files: 0,
            kept: 1,
        }
    );
    assert!(song.exists());
    assert_eq!(all(&library)?.len(), 1);
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
fn an_album_titled_composed_on_one_track_and_decomposed_on_another_is_one_album() -> Result<()> {
    let tree = Tree::new();

    for (file, album) in [
        ("01.wav", "\u{c1}g\u{e6}tis byrjun"),
        ("02.wav", "A\u{301}g\u{e6}tis byrjun"),
    ] {
        tree.write(
            &format!("rip/{file}"),
            &Wav::new()
                .text(TITLE, file)
                .text(ARTIST, "Sigur Ros")
                .text(ALBUM, album)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 1);
    Ok(())
}

#[test]
fn a_row_under_a_folder_deeper_than_the_walk_reads_is_kept_rather_than_pruned() -> Result<()> {
    let tree = Tree::new();
    let nested = ["d"; 36].join("/");
    let deep = tree.write(
        &format!("{nested}/one.wav"),
        &Wav::new().text(TITLE, "Deep").build(),
    );
    let narrow = tree.path().join(["d"; 10].join("/"));

    let library = Library::open_in_memory()?;

    scan(
        &library,
        &ScanOptions {
            roots: vec![narrow],
            ..options(&tree)
        },
    )?;

    let held = library
        .track_at(&deep, None)?
        .expect("the narrow root reached the file");
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 0);
    assert_eq!(
        library.track_at(&deep, None)?.map(|row| row.id),
        Some(held.id)
    );
    Ok(())
}

#[test]
fn a_root_gone_from_the_disc_is_cleared_however_its_path_is_spelt() -> Result<()> {
    let tree = Tree::new();

    tree.write("one.wav", &Wav::new().text(TITLE, "Gone").build());

    let root = tree.path().canonicalize().expect("a real directory");
    let spelt_otherwise = root
        .join("..")
        .join(root.file_name().expect("a named folder"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    fs::remove_dir_all(&root).expect("a removable directory");

    assert!(library.remove_root(&spelt_otherwise.join("."))?);
    assert!(library.roots()?.is_empty());
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
    Ok(())
}

#[test]
fn winding_down_cancels_the_pass_walking_the_tree_and_waits_for_it_to_let_go() -> Result<()> {
    let tree = Tree::new();

    for number in 0..200 {
        tree.write(
            &format!("album/{number:03}.wav"),
            &Wav::new().text(TITLE, "a track").build(),
        );
    }

    let library = Library::open_in_memory()?;

    assert!(
        library.wind_down(Duration::from_millis(1)),
        "nothing was running and it waited for something"
    );

    let scanning = library.scan(options(&tree))?;

    assert!(
        library.wind_down(Duration::from_secs(20)),
        "the scan was never let go of"
    );

    assert!(
        library.scan(options(&tree)).is_ok(),
        "the tree was still held after winding down"
    );

    let summary = scanning.join()?;

    assert!(summary.cancelled || summary.stats.added == 200);
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn a_name_a_mac_hands_back_decomposed_is_catalogued_composed_and_still_opens() -> Result<()> {
    let tree = Tree::new();
    let decomposed = tree.path().join("Bjo\u{308}rk").join("Je\u{301}ga.wav");
    let composed = tree.path().join("Bj\u{f6}rk").join("J\u{e9}ga.wav");

    fs::create_dir(tree.path().join("Bjo\u{308}rk")).expect("a folder named decomposed");
    fs::write(&decomposed, Wav::new().text(TITLE, "Joga").build())
        .expect("a file named decomposed");

    let library = Library::open_in_memory()?;
    let first = scan(&library, &options(&tree))?;
    let second = scan(&library, &options(&tree))?;
    let tracks = all(&library)?;
    let track = tracks.first().expect("the track was catalogued");

    assert_eq!(first.added, 1, "got {:?}", first);
    assert_eq!(second.added, 0, "got {:?}", second);
    assert_eq!(track.location.as_path(), Some(composed.as_path()));
    assert!(resonate_codec::probe(&Sources::local(), &track.location).is_ok());
    assert_eq!(
        library.track_at(&decomposed, None)?.map(|found| found.id),
        Some(track.id)
    );
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_file_with_a_name_that_is_not_text_is_catalogued_and_opens_under_the_name_it_has() -> Result<()>
{
    let tree = Tree::new();

    tree.write("good.wav", &Wav::new().text(TITLE, "good").build());

    let bytes = |name: &[u8]| {
        tree.path()
            .join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(name))
    };
    let cafe = bytes(b"caf\xe9.wav");
    let inside = bytes(b"dir\xff").join("inside.wav");

    fs::write(&cafe, Wav::new().text(TITLE, "cafe").build())
        .expect("a file with a name that is not text");
    fs::create_dir(bytes(b"dir\xff")).expect("a folder with a name that is not text");
    fs::write(&inside, Wav::new().text(TITLE, "inside").build()).expect("a file inside it");

    let library = Library::open_in_memory()?;
    let first = scan(&library, &options(&tree))?;
    let second = scan(&library, &options(&tree))?;

    assert_eq!(first.added, 3, "got {:?}", first);
    assert_eq!(second.added, 0, "got {:?}", second);
    assert_eq!(second.failed.total(), 0, "got {:?}", second.failed);
    assert_eq!(titles(&all(&library)?), vec!["cafe", "good", "inside"]);

    for (title, path) in [("cafe", &cafe), ("inside", &inside)] {
        let tracks = all(&library)?;
        let track = tracks
            .iter()
            .find(|track| track.title == title)
            .expect("the track was catalogued");

        assert_eq!(track.location.as_path(), Some(path.as_path()));
        assert!(
            resonate_codec::probe(&Sources::local(), &track.location).is_ok(),
            "{title} did not open"
        );
        assert_eq!(
            library.track_at(path, None)?.map(|found| found.id),
            Some(track.id)
        );
        assert_eq!(
            MediaLocation::from_uri(&track.location.to_uri()),
            Some(track.location.clone())
        );
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_file_with_a_name_that_is_not_text_is_forgotten_once_it_is_gone() -> Result<()> {
    let tree = Tree::new();
    let cafe = tree
        .path()
        .join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(
            b"caf\xe9.wav",
        ));

    fs::write(&cafe, Wav::new().text(TITLE, "cafe").build()).expect("a file");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(all(&library)?.len(), 1);

    fs::remove_file(&cafe).expect("the file goes");

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 1, "got {stats:?}");
    assert!(all(&library)?.is_empty());
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_cue_sheet_with_a_name_that_is_not_text_still_cuts_the_file_it_names() -> Result<()> {
    let tree = Tree::new();

    tree.write("Meddle.wav", &Wav::new().frames(44_100).build());
    fs::write(
        tree.path()
            .join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(
                b"Meddle\xe9.cue",
            )),
        MEDDLE_SHEET.as_bytes(),
    )
    .expect("a sheet with a name that is not text");

    let library = Library::open_in_memory()?;

    for _ in 0..2 {
        let stats = scan(&library, &options(&tree))?;

        assert_eq!(stats.failed.total(), 0, "got {:?}", stats.failed);
        assert_eq!(all(&library)?.len(), 3, "the sheet did not cut its file");
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn a_cue_sheet_cannot_name_a_file_whose_name_is_not_text_so_the_file_is_scanned_whole() -> Result<()>
{
    let tree = Tree::new();

    fs::write(
        tree.path()
            .join(<OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(
                b"Meddle\xe9.wav",
            )),
        Wav::new().frames(44_100).build(),
    )
    .expect("a file with a name that is not text");
    tree.write("Meddle.cue", MEDDLE_SHEET.as_bytes());

    let library = Library::open_in_memory()?;

    for _ in 0..2 {
        let stats = scan(&library, &options(&tree))?;

        assert_eq!(stats.failed.total(), 0, "got {:?}", stats.failed);
        assert_eq!(all(&library)?.len(), 1);
    }
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
fn the_names_stamp_moves_for_a_title_written_and_stands_through_a_play_and_a_favourite()
-> Result<()> {
    let tree = Tree::new();
    let path = tree.write("one.wav", &Wav::new().text(TITLE, "Echoes").build());
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let track = all(&library)?.remove(0);
    let named = library.names_stamp();

    library.track_played(&MediaLocation::local(&path), None, Duration::ZERO)?;
    library.favour(Favoured::Track(track.id), true)?;

    assert!(
        named.still_holds_at(library.names_stamp()),
        "a play or a favourite moved the names a window reads"
    );

    tree.write("one.wav", &Wav::new().text(TITLE, "Dogs").build());
    scan(&library, &options(&tree))?;

    assert!(!named.still_holds_at(library.names_stamp()));
    Ok(())
}

#[test]
fn the_catalog_names_only_the_rows_retitled_since_a_stamp_and_whether_any_arrived() -> Result<()> {
    let tree = Tree::new();

    tree.write("one.wav", &Wav::new().text(TITLE, "Echoes").build());
    tree.write("two.wav", &Wav::new().text(TITLE, "Time").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let held = all(&library)?;
    let echoes = held
        .iter()
        .find(|track| track.title == "Echoes")
        .expect("Echoes")
        .id;
    let time = held
        .iter()
        .find(|track| track.title == "Time")
        .expect("Time")
        .id;
    let named = library.names_stamp();

    assert_eq!(library.names_moved_since(named), NamesMoved::Nothing);

    tree.write("one.wav", &Wav::new().text(TITLE, "Dogs").build());
    scan(&library, &options(&tree))?;

    let moved = library.names_moved_since(named);

    assert!(moved.names(Some(echoes)));
    assert!(
        !moved.names(Some(time)),
        "a row nothing retitled is named as moved"
    );
    assert!(
        !moved.names(None),
        "no row arrived, yet one is said to have"
    );

    let retitled = library.names_stamp();

    tree.write("three.wav", &Wav::new().text(TITLE, "Money").build());
    scan(&library, &options(&tree))?;

    let moved = library.names_moved_since(retitled);

    assert!(moved.names(None), "a row arriving is not said to have");
    assert!(!moved.names(Some(echoes)));
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
fn a_scan_the_watch_asked_for_leaves_the_roots_beside_it_to_the_watch() -> Result<()> {
    let changed = Tree::new();
    let beside = Tree::new();

    changed.write("one.wav", &Wav::new().build());
    beside.write("two.wav", &Wav::new().build());

    let library = Library::open_in_memory()?;

    for tree in [&changed, &beside] {
        scan(&library, &options(tree))?;
    }

    let changed_root = changed.path().canonicalize().expect("the tree exists");

    fs::remove_file(beside.path().join("two.wav")).expect("the file taken away");
    changed.write("three.wav", &Wav::new().build());
    library
        .scan_what_is_held(ScanOptions {
            roots: vec![changed_root.clone()],
            ..options(&changed)
        })?
        .expect("a held root to walk")
        .join()?;

    assert_eq!(
        all(&library)?.len(),
        3,
        "a scan the watch asked for stat every file of a root nobody changed"
    );

    changed.write("four.wav", &Wav::new().build());
    library
        .scan(ScanOptions {
            roots: vec![changed_root],
            ..options(&changed)
        })?
        .join()?;

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
            .track_played(&one, None, Duration::ZERO)?
            .and_then(|counted| counted.track)
            .map(|track| track.plays),
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

#[test]
fn a_changed_file_that_will_not_probe_keeps_its_row_and_what_was_heard_of_it() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write("torn.wav", &Wav::new().text(TITLE, "Torn").build());

    tree.write("whole.wav", &Wav::new().text(TITLE, "Whole").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    library.track_played(&MediaLocation::local(&path), None, Duration::ZERO)?;

    fs::write(&path, b"RIFF torn midway through a write").expect("the fixture file is writable");

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.removed, 0);
    assert_eq!(stats.failed.total(), 1);
    assert_eq!(titles(&all(&library)?), vec!["Torn", "Whole"]);
    assert_eq!(library.track_at(&path, None)?.map(|row| row.plays), Some(1));
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_folder_the_scan_cannot_read_keeps_every_row_under_it() -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let tree = Tree::new();

    tree.write(
        "locked/deep/one.wav",
        &Wav::new().text(TITLE, "One").build(),
    );
    tree.write("locked/two.wav", &Wav::new().text(TITLE, "Two").build());
    tree.write("open.wav", &Wav::new().text(TITLE, "Open").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let locked = tree.path().join("locked");

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000))
        .expect("the fixture folder takes a mode");
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))
            .expect("the fixture folder takes a mode");
        eprintln!("skipping: this user reads a folder whatever its mode");
        return Ok(());
    }

    let stats = scan(&library, &options(&tree));

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755))
        .expect("the fixture folder takes a mode");

    assert_eq!(stats?.removed, 0);
    assert_eq!(titles(&all(&library)?), vec!["One", "Open", "Two"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_folder_readable_but_not_searchable_keeps_its_rows_when_another_root_is_scanned() -> Result<()>
{
    use std::os::unix::fs::PermissionsExt as _;

    let here = Tree::new();
    let there = Tree::new();

    here.write("here.wav", &Wav::new().text(TITLE, "Here").build());
    there.write("shut/one.wav", &Wav::new().text(TITLE, "One").build());
    there.write("open.wav", &Wav::new().text(TITLE, "Open").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&here))?;
    scan(&library, &options(&there))?;

    let shut = there.path().join("shut");

    fs::set_permissions(&shut, fs::Permissions::from_mode(0o444))
        .expect("the fixture folder takes a mode");
    if fs::metadata(shut.join("one.wav")).is_ok() {
        fs::set_permissions(&shut, fs::Permissions::from_mode(0o755))
            .expect("the fixture folder takes a mode");
        eprintln!("skipping: this user searches a folder whatever its mode");
        return Ok(());
    }

    let stats = scan(&library, &options(&here));

    fs::set_permissions(&shut, fs::Permissions::from_mode(0o755))
        .expect("the fixture folder takes a mode");

    assert_eq!(stats?.removed, 0);
    assert_eq!(titles(&all(&library)?), vec!["Here", "One", "Open"]);
    Ok(())
}

#[test]
fn a_volume_not_mounted_keeps_every_row_on_it_whether_its_mount_point_is_empty_or_gone()
-> Result<()> {
    let tree = Tree::new();

    tree.write(
        "drive/album/one.wav",
        &Wav::new().text(TITLE, "One").build(),
    );
    tree.write("drive/two.wav", &Wav::new().text(TITLE, "Two").build());
    tree.write("home.wav", &Wav::new().text(TITLE, "Home").build());

    let drive = tree
        .path()
        .canonicalize()
        .expect("the fixture tree has a canonical path")
        .join("drive");

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    beside(&database)
        .execute(
            "INSERT INTO volumes (path) VALUES (?1)",
            [drive.to_str().expect("the fixture path is UTF-8")],
        )
        .expect("the catalog takes a volume");

    fs::remove_dir_all(&drive).expect("the fixture drive can be emptied");
    fs::create_dir(&drive).expect("the fixture mount point can be made");

    let emptied = scan(&library, &options(&tree))?;

    assert_eq!(emptied.removed, 0);
    assert_eq!(titles(&all(&library)?), vec!["Home", "One", "Two"]);

    fs::remove_dir(&drive).expect("the fixture mount point can be taken away");

    let vanished = scan(&library, &options(&tree))?;

    assert_eq!(vanished.removed, 0);
    assert_eq!(library.forget_the_gone(&[drive])?, 0);
    assert_eq!(titles(&all(&library)?), vec!["Home", "One", "Two"]);
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_folder_on_another_volume_is_noted_and_its_rows_kept_once_it_is_not_mounted() -> Result<()> {
    let tree = Tree::new();
    let Some(elsewhere) = Elsewhere::beside(&tree, "volume") else {
        return Ok(());
    };

    fs::write(
        elsewhere.0.join("far.wav"),
        Wav::new().text(TITLE, "Far").build(),
    )
    .expect("the second volume is writable");
    tree.write("near.wav", &Wav::new().text(TITLE, "Near").build());

    let link = tree.path().join("far");

    std::os::unix::fs::symlink(&elsewhere.0, &link).expect("the fixture tree takes a link");

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    let following = ScanOptions {
        follow_symlinks: true,
        ..options(&tree)
    };

    scan(&library, &following)?;

    let noted: Vec<String> = beside(&database)
        .prepare("SELECT path FROM volumes")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get(0))
                .and_then(Iterator::collect)
        })
        .expect("the catalog lists its volumes");

    fs::remove_file(&link).expect("the fixture link can be taken away");
    fs::create_dir(&link).expect("the fixture mount point can be made");

    let unmounted = scan(&library, &following);

    drop(elsewhere);

    let canonical = tree
        .path()
        .canonicalize()
        .expect("the fixture tree has a canonical path");

    assert_eq!(noted, vec![canonical.join("far").display().to_string()]);
    assert_eq!(unmounted?.removed, 0);
    assert_eq!(titles(&all(&library)?), vec!["Far", "Near"]);
    Ok(())
}

#[cfg(unix)]
struct Elsewhere(PathBuf);

#[cfg(unix)]
impl Elsewhere {
    fn beside(tree: &Tree, named: &str) -> Option<Self> {
        let elsewhere = Path::new("/dev/shm").join(format!("resonate-{named}-{}", process::id()));

        if fs::create_dir_all(&elsewhere).is_err() {
            eprintln!("skipping: no second volume to link into");
            return None;
        }

        let elsewhere = Self(elsewhere);
        let same_volume = fs::metadata(&elsewhere.0).map(|held| held.dev()).ok()
            == fs::metadata(tree.path()).map(|held| held.dev()).ok();

        if same_volume {
            eprintln!("skipping: the shared memory folder is on the fixture tree's volume");
            return None;
        }
        Some(elsewhere)
    }
}

#[cfg(unix)]
impl Drop for Elsewhere {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn ids_of(tracks: &[Track]) -> BTreeSet<TrackId> {
    tracks.iter().map(|track| track.id).collect()
}

#[cfg(unix)]
fn the_first_under(tracks: &[Track], folder: &Path) -> Track {
    tracks
        .iter()
        .find(|track| {
            track
                .location
                .as_path()
                .is_some_and(|path| path.starts_with(folder))
        })
        .expect("the interrupted scan wrote a song of the drive")
        .clone()
}

#[cfg(unix)]
fn unplugged(link: &Path) {
    fs::remove_file(link).expect("the fixture link can be taken away");
    fs::create_dir(link).expect("the fixture mount point can be made");
}

#[cfg(unix)]
#[test]
fn a_volume_first_walked_by_a_scan_cancelled_partway_keeps_its_rows_and_favourite_once_unplugged()
-> Result<()> {
    let tree = Tree::new();
    let Some(elsewhere) = Elsewhere::beside(&tree, "cancelled") else {
        return Ok(());
    };

    tree.write("near.wav", &Wav::new().text(TITLE, "Near").build());
    many_short_songs(&elsewhere.0);

    let link = tree.path().join("far");

    std::os::unix::fs::symlink(&elsewhere.0, &link).expect("the fixture tree takes a link");

    let library = Library::open(&tree.path().join("library.db"))?;
    let following = ScanOptions {
        follow_symlinks: true,
        ..options(&tree)
    };

    cancelled_once_a_batch_is_written(&library, &following)?;

    let held = all(&library)?;
    let far = the_first_under(&held, &link);

    library.favour(Favoured::Track(far.id), true)?;
    unplugged(&link);

    let unmounted = scan(&library, &following)?;

    assert_eq!(unmounted.removed, 0);
    assert_eq!(ids_of(&all(&library)?), ids_of(&held));
    assert!(
        library
            .track(far.id)?
            .is_some_and(|track| track.favourite.is_some()),
        "the favourite went with a row of the unplugged drive"
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_volume_first_walked_by_a_scan_whose_write_failed_keeps_its_rows_and_favourite_once_unplugged()
-> Result<()> {
    let tree = Tree::new();
    let Some(elsewhere) = Elsewhere::beside(&tree, "refused") else {
        return Ok(());
    };

    tree.write("near.wav", &Wav::new().text(TITLE, "Near").build());

    let gone = tree.write("gone.wav", &Wav::new().text(TITLE, "Gone").build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;
    let following = ScanOptions {
        follow_symlinks: true,
        ..options(&tree)
    };

    scan(&library, &following)?;
    fs::remove_file(&gone).expect("the file is taken away");
    for title in ["One", "Two"] {
        fs::write(
            elsewhere.0.join(format!("{title}.wav")),
            Wav::new().text(TITLE, title).build(),
        )
        .expect("the second volume is writable");
    }

    let link = tree.path().join("far");

    std::os::unix::fs::symlink(&elsewhere.0, &link).expect("the fixture tree takes a link");
    stopped_at_the_mark(&library, &database, &following)?;

    let held: Vec<Track> = all(&library)?
        .into_iter()
        .filter(|track| track.title != "Gone")
        .collect();
    let far = the_first_under(&held, &link);

    library.favour(Favoured::Track(far.id), true)?;
    unplugged(&link);

    let unmounted = scan(&library, &following)?;

    assert_eq!(unmounted.removed, 1);
    assert_eq!(titles(&all(&library)?), vec!["Near", "One", "Two"]);
    assert_eq!(ids_of(&all(&library)?), ids_of(&held));
    assert!(
        library
            .track(far.id)?
            .is_some_and(|track| track.favourite.is_some()),
        "the favourite went with a row of the unplugged drive"
    );
    Ok(())
}

#[test]
fn two_links_to_one_directory_are_walked_once_rather_than_read_as_a_cycle() -> Result<()> {
    let tree = Tree::new();
    let shared = Tree::new();

    tree.write("albums/keep.wav", &Wav::new().text(TITLE, "Keep").build());
    shared.write("one.wav", &Wav::new().text(TITLE, "One").build());

    for named in ["albums/first", "albums/second"] {
        if !link_to_directory(shared.path(), &tree.path().join(named)) {
            return Ok(());
        }
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
fn a_link_to_a_folder_inside_another_links_target_is_walked_once_whichever_comes_first()
-> Result<()> {
    let tree = Tree::new();
    let shared = Tree::new();

    shared.write("inner/one.wav", &Wav::new().text(TITLE, "One").build());
    fs::create_dir_all(tree.path().join("albums")).expect("a writable temporary directory");

    for (named, target) in [
        ("albums/a", shared.path().to_path_buf()),
        ("albums/b", shared.path().join("inner")),
        ("albums/c", shared.path().to_path_buf()),
    ] {
        if !link_to_directory(&target, &tree.path().join(named)) {
            return Ok(());
        }
    }

    let library = Library::open_in_memory()?;

    scan(
        &library,
        &ScanOptions {
            follow_symlinks: true,
            ..options(&tree)
        },
    )?;

    assert_eq!(titles(&all(&library)?), vec!["One"]);
    Ok(())
}

#[test]
fn a_link_naming_a_file_is_never_catalogued_whether_links_are_followed_or_not() -> Result<()> {
    let tree = Tree::new();
    let shared = Tree::new();
    let song = tree.write("album/song.wav", &Wav::new().text(TITLE, "Song").build());

    shared.write("far.wav", &Wav::new().text(TITLE, "Far").build());

    let linked = [
        link_to_file(&song, &tree.path().join("alias.wav")),
        link_to_file(&song, &tree.path().join("album/also.wav")),
        link_to_directory(shared.path(), &tree.path().join("shared")),
    ];

    if linked.contains(&false) {
        return Ok(());
    }

    let mut followed = Vec::new();

    for follow_symlinks in [false, true] {
        let library = Library::open_in_memory()?;

        scan(
            &library,
            &ScanOptions {
                follow_symlinks,
                ..options(&tree)
            },
        )?;
        followed.push(
            all(&library)?
                .into_iter()
                .filter_map(|track| track.location.as_path().map(Path::to_path_buf))
                .collect::<Vec<_>>(),
        );
    }

    let canonical = tree
        .path()
        .canonicalize()
        .expect("the fixture tree has a canonical path");

    assert_eq!(followed[0], vec![canonical.join("album/song.wav")]);
    assert_eq!(
        followed[1].iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &canonical.join("album/song.wav"),
            &canonical.join("shared/far.wav")
        ])
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_row_an_older_build_catalogued_for_a_link_hands_its_history_to_the_file_it_names() -> Result<()>
{
    let tree = Tree::new();

    tree.write("album/song.wav", &Wav::new().text(TITLE, "Song").build());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let song = root.join("album/song.wav");
    let link = root.join("alias.wav");

    std::os::unix::fs::symlink(&song, &link).expect("the fixture tree takes a link");
    catalogued_again_at(&database, &song, &link);

    for (played, times) in [(&song, 1), (&link, 2)] {
        for _ in 0..times {
            let counted = library
                .track_played(&MediaLocation::local(played), None, Duration::from_secs(2))?
                .expect("a counted play");

            library.listened(counted.listen, Duration::from_secs(2))?;
        }
    }

    let linked = library
        .track_at(&link, None)?
        .expect("the row the older build kept");

    library.favour(Favoured::Track(linked.id), true)?;

    let before = library.statistics(Window::Everything)?;

    scan(&library, &options(&tree))?;

    let after = library.statistics(Window::Everything)?;
    let standing = library.track_at(&song, None)?.expect("the file's own row");

    assert_eq!(library.track_at(&link, None)?, None);
    assert_eq!(standing.plays, 3);
    assert!(
        standing.favourite.is_some(),
        "the link's favourite was lost"
    );
    assert_eq!(before.plays, 3);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);
    Ok(())
}

#[cfg(unix)]
fn catalogued_again_at(database: &Path, held: &Path, at: &Path) {
    let catalog = beside(database);
    let columns: Vec<String> = catalog
        .prepare("SELECT name FROM pragma_table_xinfo('tracks') WHERE hidden = 0 AND name != 'id'")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .expect("the catalog names its columns");
    let chosen: Vec<&str> = columns
        .iter()
        .map(|column| match column.as_str() {
            "path" => "?1",
            other => other,
        })
        .collect();

    let written = catalog
        .execute(
            &format!(
                "INSERT INTO tracks ({}) SELECT {} FROM tracks WHERE path = ?2",
                columns.join(", "),
                chosen.join(", ")
            ),
            rusqlite::params![settled_text(at), settled(held)],
        )
        .expect("the catalog takes a second row");

    assert_eq!(written, 1);
}

#[cfg(unix)]
fn settled_text(path: &Path) -> String {
    path.to_str()
        .expect("a path the catalog can hold")
        .to_owned()
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
fn a_queue_of_tracks_and_their_albums_reads_back_in_one_pass() -> Result<()> {
    let (_tree, library) = scanned_two_discs();
    let rows = library.tracks(&TrackQuery::default())?;
    let mut ids: Vec<TrackId> = rows.iter().map(|row| row.id).collect();

    ids.extend(ids.clone());
    ids.push(TrackId::MAX);

    let mut read = library.tracks_with_ids(&ids)?;

    read.sort_by_key(|track| track.id);

    let mut wanted = rows.clone();

    wanted.sort_by_key(|track| track.id);

    assert_eq!(read, wanted);

    let albums: Vec<AlbumId> = rows.iter().filter_map(|row| row.album_id).collect();
    let titles = library.album_titles(&albums)?;

    assert!(!titles.is_empty());

    for (album, title) in titles {
        assert_eq!(library.album(album)?.map(|held| held.title), Some(title));
    }
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

#[test]
fn a_row_marked_to_be_probed_again_is_read_again_though_its_file_has_not_moved() -> Result<()> {
    let tree = Tree::new();

    tree.write("06 Echoes.wav", &one_song(16, 44_100));

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let billed_wrongly =
        "UPDATE tracks SET codec = 11, vault_key = 'kept', vault_path = 'audio/kept'";

    beside(&database)
        .execute(billed_wrongly, [])
        .expect("the track row is writable");
    scan(&library, &options(&tree))?;

    let held = |database: &Path| {
        beside(database)
            .query_row(
                "SELECT codec, probe_again, vault_key, vault_path FROM tracks",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .expect("the track row reads")
    };

    assert_eq!(
        held(&database).0,
        11,
        "a row nobody marked was probed again though its file had not moved"
    );

    beside(&database)
        .execute("UPDATE tracks SET probe_again = 1", [])
        .expect("the track row is writable");
    scan(&library, &options(&tree))?;

    assert_eq!(
        held(&database),
        (5, 0, Some("kept".to_owned()), Some("audio/kept".to_owned())),
        "a marked row was not billed by what its file is, or lost its vault object on the way"
    );
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
fn the_hidden_tracks_are_listed_by_title_counted_past_the_list_and_shown_again_at_once()
-> Result<()> {
    let (_, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(library.hidden_tracks(10)?, HiddenTracks::default());

    let mut hidden: Vec<&Track> = rows.iter().take(2).collect();

    for track in &hidden {
        library.hide_track(track.id, true)?;
    }
    hidden.sort_by_key(|track| track.title.to_lowercase());

    let listed = library.hidden_tracks(10)?;
    let first = library.hidden_tracks(1)?;

    assert_eq!(listed.counted, 2);
    assert_eq!(
        listed
            .tracks
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        hidden.iter().map(|track| track.id).collect::<Vec<_>>(),
        "the hidden tracks are not listed by title"
    );
    assert_eq!(first.counted, 2, "a short list counts only what it holds");
    assert_eq!(first.tracks.len(), 1);

    assert_eq!(library.show_every_hidden_track()?, 2);
    assert_eq!(library.hidden_tracks(10)?, HiddenTracks::default());
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), rows.len());
    Ok(())
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
        .track_played(&MediaLocation::local(&before), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");

    library.favour(Favoured::Track(echoes.id), true)?;

    let playlist = library.create_playlist("Meddle")?;

    library.add_to_playlist(playlist, &[Cut::whole(MediaLocation::local(&before))])?;

    let ahead = LyricsAhead::ZERO.earlier().earlier();

    library.hold_lyrics_ahead(&MediaLocation::local(&before), None, ahead)?;

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
    assert_eq!(
        library.lyrics_ahead(&row.location, None)?,
        ahead,
        "the lyric offset stayed behind at the old path"
    );
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
        .track_played(&MediaLocation::local(&first), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");

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
fn a_file_retagged_as_it_moved_is_followed_by_what_it_sounds_like() -> Result<()> {
    let tree = Tree::new();
    let before = tree.write(
        "incoming/track01.wav",
        &Wav::new().frames(7_919).text(TITLE, "Echoes").build(),
    );

    tree.write(
        "incoming/track02.wav",
        &Wav::new().frames(7_907).text(TITLE, "Seamus").build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let heard = library
        .track_played(&MediaLocation::local(&before), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");

    fs::remove_file(&before).expect("the file is taken away");

    let after = tree.write(
        "Pink Floyd/Meddle/06 Echoes.wav",
        &Wav::new()
            .frames(7_919)
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .text(ALBUM, "Meddle")
            .build(),
    );
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 1);
    assert_eq!(stats.added, 0);

    let row = library
        .track_at(&after, None)?
        .expect("the row followed the file");

    assert_eq!(row.id, heard.id);
    assert_eq!(row.plays, 1);
    Ok(())
}

#[test]
fn a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_print_its_study_took()
-> Result<()> {
    const SECONDS_HEARD: usize = 6;

    let tree = Tree::new();
    let before = tree.write(
        "incoming/track01.wav",
        &Wav::new()
            .frames(SECONDS_HEARD * RATE as usize)
            .text(TITLE, "Echoes")
            .build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let heard = library
        .track_played(&MediaLocation::local(&before), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");
    let print = resonate_analysis::print(
        &Sources::local(),
        &MediaLocation::local(&before),
        None,
        &resonate_analysis::Watch::default(),
    )
    .expect("a readable file")
    .expect("a print of the file");

    beside(&database)
        .execute(
            "INSERT INTO track_studies (track_id, studied, studied_under, verdict, peak, rms,
                                        true_peak, clipped, mono_as_stereo, print, print_length)
             VALUES (?1, 0, 0, 'genuine', 1.0, 0.5, 1.0, 0, 0, ?2, ?3)",
            rusqlite::params![heard.id.get() as i64, print.encoded(), SECONDS_HEARD as i64],
        )
        .expect("the study is kept");
    beside(&database)
        .execute("UPDATE tracks SET packets = NULL", [])
        .expect("the row reads as one scanned before its packets were digested");

    fs::remove_file(&before).expect("the file is taken away");

    let after = tree.write(
        "Pink Floyd/Meddle/06 Money.wav",
        &Wav::new()
            .frames(SECONDS_HEARD * RATE as usize)
            .text(TITLE, "Money")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 1);

    let row = library
        .track_at(&after, None)?
        .expect("the row followed the file");

    assert_eq!(row.id, heard.id);
    assert_eq!(row.plays, 1);
    Ok(())
}

#[test]
fn a_file_retagged_past_every_name_as_it_moved_is_followed_by_the_packets_its_scan_digested()
-> Result<()> {
    let tree = Tree::new();
    let before = tree.write(
        "incoming/track01.wav",
        &Wav::new().frames(9_973).text(TITLE, "Echoes").build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let heard = library
        .track_played(&MediaLocation::local(&before), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");

    fs::remove_file(&before).expect("the file is taken away");

    let after = tree.write(
        "Pink Floyd/Meddle/06 Money.wav",
        &Wav::new()
            .frames(9_973)
            .text(TITLE, "Money")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );
    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 1, "a file nobody studied was not followed");

    let row = library
        .track_at(&after, None)?
        .expect("the row followed the file");

    assert_eq!(row.id, heard.id);
    assert_eq!(row.plays, 1);
    assert_eq!(row.title, "Money", "the row kept the title it was heard as");
    assert_eq!(row.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(
        row.file_size,
        fs::metadata(&after).expect("the file is there").len(),
        "the row kept the size of the file before it was retagged"
    );
    assert_eq!(narrowed(&library, "money")?, vec!["Money"]);
    assert_eq!(narrowed(&library, "artist:floyd")?, vec!["Money"]);
    assert!(
        narrowed(&library, "echoes")?.is_empty(),
        "the index still held the title the file had before it moved"
    );
    Ok(())
}

#[test]
fn a_file_a_sheet_cuts_moved_with_its_sheet_keeps_every_rows_plays() -> Result<()> {
    let tree = Tree::new();

    tree.write("rip/Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_SHEET.as_bytes());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let echoes = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Echoes")
        .expect("the sheet's third row");

    library.track_played(&echoes.location, echoes.span, Duration::ZERO)?;

    fs::create_dir_all(tree.path().join("Pink Floyd")).expect("a writable temporary directory");
    fs::rename(
        tree.path().join("rip"),
        tree.path().join("Pink Floyd/Meddle"),
    )
    .expect("the rip is moved by hand");

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 3);
    assert_eq!(stats.added, 0);

    let moved = library
        .track_at(
            &tree.path().join("Pink Floyd/Meddle/Meddle.wav"),
            echoes.span,
        )?
        .expect("the row followed the file");

    assert_eq!(moved.id, echoes.id);
    assert_eq!(moved.plays, 1);
    Ok(())
}

#[test]
fn a_file_a_sheet_cuts_moved_as_its_sheet_was_edited_takes_the_titles_the_edited_sheet_gives()
-> Result<()> {
    let tree = Tree::new();

    tree.write("rip/Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_SHEET.as_bytes());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let echoes = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Echoes")
        .expect("the sheet's third row");

    library.track_played(&echoes.location, echoes.span, Duration::ZERO)?;

    fs::create_dir_all(tree.path().join("Pink Floyd")).expect("a writable temporary directory");
    fs::rename(
        tree.path().join("rip"),
        tree.path().join("Pink Floyd/Meddle"),
    )
    .expect("the rip is moved by hand");
    tree.write(
        "Pink Floyd/Meddle/Meddle.cue",
        MEDDLE_SHEET.replace("Echoes", "Fearless").as_bytes(),
    );

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 3);
    assert_eq!(stats.added, 0);

    let moved = library
        .track_at(
            &tree.path().join("Pink Floyd/Meddle/Meddle.wav"),
            echoes.span,
        )?
        .expect("the row followed the file");

    assert_eq!(moved.id, echoes.id);
    assert_eq!(moved.plays, 1);
    assert_eq!(moved.title, "Fearless");
    assert_eq!(narrowed(&library, "fearless")?, vec!["Fearless"]);
    assert!(
        narrowed(&library, "echoes")?.is_empty(),
        "the index still held the title the sheet had before it was edited"
    );
    Ok(())
}

#[test]
fn a_scan_naming_one_root_leaves_a_hand_move_inside_another_to_the_walk_that_can_pair_it()
-> Result<()> {
    let named = Tree::new();
    let beside = Tree::new();

    named.write("one.wav", &Wav::new().frames(100).build());

    let before = beside.write("old/Echoes.wav", &Wav::new().frames(4_410).build());
    let deleted = beside.write("deleted.wav", &Wav::new().frames(9_973).build());
    let library = Library::open_in_memory()?;

    for tree in [&named, &beside] {
        scan(&library, &options(tree))?;
    }

    let heard = library
        .track_played(&MediaLocation::local(&before), None, Duration::ZERO)?
        .expect("a counted play")
        .track
        .expect("the play was counted against a row");

    let after = beside.path().join("new/Echoes.wav");

    fs::create_dir_all(after.parent().expect("a folder")).expect("a writable folder");
    fs::rename(&before, &after).expect("the file is moved by hand");
    fs::remove_file(&deleted).expect("the file is taken away");
    scan(&library, &options(&named))?;

    assert!(
        library.track_at(&before, None)?.is_some(),
        "a tidy forgot a file that could have moved into the root it did not walk"
    );
    assert!(
        library.track_at(&deleted, None)?.is_none(),
        "a tidy kept a file nothing in its root could be the move of"
    );

    let stats = scan(&library, &options(&beside))?;

    assert_eq!(stats.moved, 1);
    assert_eq!(stats.added, 0);

    let row = library.track_at(&after, None)?.expect("the row followed");

    assert_eq!(row.id, heard.id);
    assert_eq!(row.plays, 1);
    assert!(library.track_at(&before, None)?.is_none());
    Ok(())
}

const THE_MARK_REFUSED: &str = "CREATE TRIGGER the_mark_refused BEFORE UPDATE OF seen ON tracks
     WHEN NEW.seen < 0
     BEGIN SELECT RAISE(ABORT, 'the catalog refuses the mark'); END";

const THE_MARK_ALLOWED: &str = "DROP TRIGGER the_mark_refused";

fn stopped_at_the_mark(library: &Library, database: &Path, options: &ScanOptions) -> Result<()> {
    beside(database)
        .execute_batch(THE_MARK_REFUSED)
        .expect("the catalog takes a trigger");

    let stopped = library.scan(options.clone())?.join();

    beside(database)
        .execute_batch(THE_MARK_ALLOWED)
        .expect("the catalog lets the trigger go");

    assert!(
        stopped.is_err(),
        "the scan went on past the mark it was refused"
    );
    Ok(())
}

fn heard_favoured_and_queued(library: &Library, track: &Track) -> Result<()> {
    let counted = library
        .track_played(&track.location, track.span, Duration::ZERO)?
        .expect("a counted play");

    library.listened(counted.listen, Duration::from_secs(2))?;
    library.favour(Favoured::Track(track.id), true)?;
    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: vec![Resumable {
                location: track.location.clone(),
                span: track.span,
                held: Some(track.id),
            }],
            order: vec![0],
            row: 0,
            at: Frames::ZERO,
            shuffle: false,
            next: None,
        },
    )
}

fn followed_as_it_was(library: &Library, database: &Path, was: &Track, to: &Path) -> Result<()> {
    let now = library
        .track_at(to, was.span)?
        .expect("the row followed the file");

    let listens: i64 = beside(database)
        .query_row(
            "SELECT count(*) FROM listens WHERE track_id = ?1",
            [now.id.get() as i64],
            |row| row.get(0),
        )
        .expect("the catalog counts the listens");
    let resumed = library.resumption()?.expect("the queue was kept");

    assert_eq!(now.id, was.id);
    assert_eq!(now.plays, 1);
    assert!(now.favourite.is_some(), "the favourite stayed behind");
    assert_eq!(listens, 1);
    assert_eq!(
        resumed.rows,
        vec![Resumable {
            location: now.location.clone(),
            span: now.span,
            held: Some(was.id),
        }]
    );
    Ok(())
}

fn watched_alone(library: &Library, tree: &Tree) -> Result<()> {
    let watched = library
        .scan_what_is_held(options(tree))?
        .expect("the tree is a root")
        .join()?;

    assert!(
        !watched.cancelled,
        "the watch's scan reported itself cancelled"
    );
    Ok(())
}

fn every_root(tree: &Tree) -> ScanOptions {
    ScanOptions {
        roots: Vec::new(),
        ..options(tree)
    }
}

#[test]
fn a_file_moved_as_a_scan_stopped_short_of_following_it_keeps_its_row_favourite_listens_and_queue_place()
-> Result<()> {
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

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let echoes = library
        .track_at(&before, None)?
        .expect("the scan stored the song");

    heard_favoured_and_queued(&library, &echoes)?;

    let after = tree.path().join("filed/Pink Floyd/06 Echoes.wav");

    moved(&before, &after);
    stopped_at_the_mark(&library, &database, &options(&tree))?;

    assert!(
        library.track_at(&after, None)?.is_some(),
        "the stopped scan wrote nothing of the arrival"
    );

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 1);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    assert_eq!(all(&library)?.len(), 2);
    followed_as_it_was(&library, &database, &echoes, &after)
}

#[test]
fn a_file_a_sheet_cuts_moved_as_a_scan_stopped_short_of_following_it_keeps_every_rows_identity()
-> Result<()> {
    let tree = Tree::new();

    tree.write("rip/Meddle.wav", &Wav::new().frames(44_100).build());
    tree.write("rip/Meddle.cue", MEDDLE_SHEET.as_bytes());

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let echoes = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Echoes")
        .expect("the sheet's third row");

    heard_favoured_and_queued(&library, &echoes)?;
    fs::create_dir_all(tree.path().join("Pink Floyd")).expect("a writable temporary directory");
    fs::rename(
        tree.path().join("rip"),
        tree.path().join("Pink Floyd/Meddle"),
    )
    .expect("the rip is moved by hand");
    stopped_at_the_mark(&library, &database, &options(&tree))?;

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 3);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    followed_as_it_was(
        &library,
        &database,
        &echoes,
        &tree.path().join("Pink Floyd/Meddle/Meddle.wav"),
    )
}

#[test]
fn a_file_moved_into_a_root_the_watch_scanned_alone_is_followed_by_the_next_scan_of_every_root()
-> Result<()> {
    let catalog = Tree::new();
    let source = Tree::new();
    let destination = Tree::new();
    let before = source.write(
        "echoes.wav",
        &Wav::new()
            .text(TITLE, "Echoes")
            .text(ARTIST, "Pink Floyd")
            .build(),
    );

    source.write("seamus.wav", &Wav::new().text(TITLE, "Seamus").build());
    destination.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let database = catalog.path().join("library.db");
    let library = Library::open(&database)?;

    for tree in [&source, &destination] {
        scan(&library, &options(tree))?;
    }

    let echoes = library
        .track_at(&before, None)?
        .expect("the scan stored the song");

    heard_favoured_and_queued(&library, &echoes)?;

    let after = destination.path().join("Pink Floyd/Echoes.wav");

    moved(&before, &after);
    watched_alone(&library, &destination)?;

    assert!(
        library.track_at(&after, None)?.is_some(),
        "the watch's scan wrote nothing of the arrival"
    );

    let stats = scan(&library, &every_root(&source))?;

    assert_eq!(stats.moved, 1);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    assert!(library.track_at(&before, None)?.is_none());
    followed_as_it_was(&library, &database, &echoes, &after)
}

#[test]
fn a_file_a_sheet_cuts_moved_into_a_root_the_watch_scanned_alone_keeps_every_rows_identity()
-> Result<()> {
    let catalog = Tree::new();
    let source = Tree::new();
    let destination = Tree::new();

    source.write("rip/Meddle.wav", &Wav::new().frames(44_100).build());
    source.write("rip/Meddle.cue", MEDDLE_SHEET.as_bytes());
    destination.write("dogs.wav", &Wav::new().text(TITLE, "Dogs").build());

    let database = catalog.path().join("library.db");
    let library = Library::open(&database)?;

    for tree in [&source, &destination] {
        scan(&library, &options(tree))?;
    }

    let echoes = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Echoes")
        .expect("the sheet's third row");

    heard_favoured_and_queued(&library, &echoes)?;
    fs::create_dir_all(destination.path().join("Pink Floyd"))
        .expect("a writable temporary directory");
    fs::rename(
        source.path().join("rip"),
        destination.path().join("Pink Floyd/Meddle"),
    )
    .expect("the rip is moved by hand");

    watched_alone(&library, &destination)?;

    let stats = scan(&library, &every_root(&source))?;

    assert_eq!(stats.moved, 3);
    assert_eq!(stats.added, 0);
    assert_eq!(stats.removed, 0);
    followed_as_it_was(
        &library,
        &database,
        &echoes,
        &destination.path().join("Pink Floyd/Meddle/Meddle.wav"),
    )
}

#[test]
fn a_followed_move_takes_the_advisory_and_the_frames_walked_the_arrival_was_probed_with()
-> Result<()> {
    let tree = Tree::new();
    let song = tree.write(
        "loose/song.wav",
        &Wav::new()
            .text(TITLE, "A song")
            .described("ITUNESADVISORY", "1")
            .build(),
    );
    let stream = tree.write("loose/walked.mp3", &a_variable_rate_stream());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let advised = library
        .track_at(&song, None)?
        .expect("the scan stored the song");
    let walked = library
        .track_at(&stream, None)?
        .expect("the scan stored the stream");

    beside(&database)
        .execute(
            "UPDATE tracks SET walked = NULL WHERE path = ?1",
            [settled(&stream)],
        )
        .expect("the stream's walk is lost");

    let song_after = tree.path().join("filed/song.wav");
    let stream_after = tree.path().join("filed/walked.mp3");

    moved(&song, &song_after);
    moved(&stream, &stream_after);
    tree.write(
        "filed/song.wav",
        &Wav::new()
            .text(TITLE, "A song")
            .described("ITUNESADVISORY", "2")
            .build(),
    );

    let stats = scan(&library, &options(&tree))?;
    let followed = library
        .track_at(&song_after, None)?
        .expect("the row followed the song");
    let walked_after = library
        .track_at(&stream_after, None)?
        .expect("the row followed the stream");
    let hinted = library
        .hinting()
        .walked(&MediaLocation::local(&stream_after));

    assert_eq!(advised.explicit, Some(true));
    assert_eq!(stats.moved, 2);
    assert_eq!(stats.added, 0);
    assert_eq!(followed.id, advised.id);
    assert_eq!(followed.explicit, Some(false));
    assert_eq!(walked_after.id, walked.id);
    assert!(
        hinted.is_some(),
        "the followed row kept the walk it had lost rather than the arrival's"
    );
    Ok(())
}

#[test]
fn a_row_named_by_its_file_name_keeps_what_a_lookup_answered_when_it_is_renamed() -> Result<()> {
    let tree = Tree::new();
    let before = tree.write(
        "The Orbiters - Echoes.wav",
        &Wav::new().frames(7_919).build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    beside(&database)
        .execute(
            "UPDATE tracks SET title = 'Echoes (Answered)', answered = 1 WHERE path = ?1",
            rusqlite::params![settled(&before)],
        )
        .expect("the row is answered");

    fs::remove_file(&before).expect("the file is renamed");

    let after = tree.write(
        "The Orbiters - Echoes (2011).wav",
        &Wav::new().frames(7_919).text(b"TCOM", "Somebody").build(),
    );

    scan(&library, &options(&tree))?;
    scan(&library, &options(&tree))?;

    let held = stored(&database, &after);

    assert_eq!(held.title, "Echoes (Answered)");
    assert!(
        held.answered.is_some(),
        "a rename was read as a retagging and the row asked about again"
    );
    Ok(())
}

#[test]
fn a_file_taken_away_and_another_of_its_length_added_are_not_one_file() -> Result<()> {
    let tree = Tree::new();
    let gone = tree.write(
        "old.wav",
        &Wav::new().frames(7_919).text(TITLE, "Echoes").build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    fs::remove_file(&gone).expect("the file is taken away");
    tree.write(
        "new.wav",
        &Wav::new()
            .frames(7_919)
            .sounding_from(1)
            .text(TITLE, "Seamus")
            .build(),
    );

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.moved, 0);
    assert_eq!(stats.added, 1);
    assert_eq!(stats.removed, 1);
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

#[test]
fn deleting_more_tracks_than_one_query_reads_forgets_every_row_and_names_an_unknown_one()
-> Result<()> {
    const MORE_THAN_ONE_BATCH: usize = 600;

    let tree = Tree::new();

    for number in 0..MORE_THAN_ONE_BATCH {
        tree.write(
            &format!("album/{number:04}.wav"),
            &Wav::new().text(TITLE, &format!("song {number}")).build(),
        );
    }

    let library = Library::open_in_memory()?;

    library.trash_through(Some(Arc::new(TrashedByTheDesktop {
        asked: Mutex::new(Vec::new()),
        answer: Removal::Removed,
    })));
    scan(&library, &options(&tree))?;

    let mut tracks: Vec<TrackId> = all(&library)?.into_iter().map(|track| track.id).collect();

    assert_eq!(tracks.len(), MORE_THAN_ONE_BATCH);

    tracks.push(TrackId::MAX);

    assert!(matches!(
        library.delete_tracks(&tracks),
        Err(Error::UnknownTrack(unknown)) if unknown == TrackId::MAX
    ));

    tracks.pop();

    let deleted = library.delete_tracks(&tracks)?;

    assert_eq!(deleted.tracks, MORE_THAN_ONE_BATCH as u64);
    assert_eq!(deleted.files, MORE_THAN_ONE_BATCH as u64);
    assert!(all(&library)?.is_empty());
    Ok(())
}
