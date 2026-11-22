use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::{MediaLocation, PlaylistId, ReleaseTrackId, SourceId, TrackId};
use resonate_library::{
    AlbumQuery, Cut, DeliveryFolder, Layout, Library, Looked, LookingFor, PollOptions, Quality,
    Release, Removal, Replaceable, Replaced, Replacing, Result, Track, TrackQuery, Trashes,
};
#[cfg(unix)]
use resonate_library::{
    ChosenVersion, Favoured, FileTags, Picturing, ScanOptions, Sources, TagSink as _,
    TagSource as _, Vault,
};
use resonate_providers::{Provider, Providers};

use crate::support::{
    ALBUM, ARTIST, Delivering, LastResort, Offering, RECORDING, RELEASE, TITLE, TRACK, Tree,
    Unanswering, Wav, all, asking_in_turn, beside, every_file_under, ffmpeg, mbid, offered,
    one_song, options, orbits, release_row, scan, the_row_at, unreachable_server, whole,
};
#[cfg(unix)]
use crate::support::{HOURS, opened_with_a_vault, png_art, vaulted};

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

const TWO_SECONDS_OF_FRAMES: usize = 88_200;

const THE_CODEC_OF_A_FILE_NAMED: [(&str, &[&str]); 4] = [
    ("opus", &["-c:a", "libopus", "-b:a", "96k"]),
    ("mp3", &["-c:a", "libmp3lame", "-b:a", "128k"]),
    ("flac", &["-c:a", "flac"]),
    ("ogg", &["-c:a", "libvorbis"]),
];

fn encoded_with_tags(tree: &Tree, name: &str, tags: &[&str]) -> Option<PathBuf> {
    encoded_lasting(tree, name, tags, TWO_SECONDS_OF_FRAMES, &[])
}

fn encoded_lasting(
    tree: &Tree,
    name: &str,
    tags: &[&str],
    frames: usize,
    shaped: &[&str],
) -> Option<PathBuf> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name}");
        return None;
    }

    let extension = Path::new(name)
        .extension()
        .and_then(|found| found.to_str())?;
    let (_, codec) = THE_CODEC_OF_A_FILE_NAMED
        .iter()
        .find(|(known, _)| *known == extension)?;
    let held = Tree::new();
    let source = held.write("tone.wav", &Wav::new().frames(frames).build());
    let target = tree.path().join(name);

    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("a writable temporary directory");
    }

    let mut command = Command::new("ffmpeg");

    command
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .args(*codec)
        .args(shaped);
    for tag in tags {
        command.args(["-metadata", tag]);
    }

    let encoded = command
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

fn pink_floyd(title: &str) -> [String; 3] {
    [
        format!("title={title}"),
        "artist=Pink Floyd".to_owned(),
        "album=Meddle".to_owned(),
    ]
}

fn encoded_as_pink_floyd(tree: &Tree, name: &str, title: &str) -> Option<PathBuf> {
    let tags = pink_floyd(title);
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    encoded_with_tags(tree, name, &tags)
}

#[test]
fn a_playlists_lossy_songs_are_listed_once_each_with_the_lossless_copy_held() -> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(flac), Some(mp3), Some(money)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
        encoded_as_pink_floyd(&tree, "lossless/C.flac", "Money"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let playlist = library.start_playlist(
        "Dark",
        &[whole(&opus), whole(&mp3), whole(&opus), whole(&money)],
    )?;

    let listed = library.songs_to_better_in(playlist)?;

    assert_eq!(
        listed
            .iter()
            .map(|row| row.track.title.as_str())
            .collect::<Vec<_>>(),
        ["Echoes", "Time"]
    );

    let held = listed[0].held.as_ref().expect("the lossless copy is held");

    assert_eq!(library.track_at(&flac, None)?.expect("held").id, held.id);
    assert!(listed[0].can_be_ticked());
    assert_eq!(listed[1].held.as_ref().map(|copy| copy.id), None);
    assert_eq!(listed[1].release_track, None);
    assert!(!listed[1].can_be_ticked());
    assert!(listed.iter().all(|row| !row.pending));
    Ok(())
}

const A_SHEET_CUTTING_A_LOSSY_FILE: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "whole.opus" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Echoes"
    INDEX 01 00:01:00
"#;

const A_SHEET_CUTTING_A_LOSSLESS_FILE: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "album.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Fearless"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Fearless"
    INDEX 01 00:01:00
"#;

#[test]
fn a_cut_of_a_sheet_is_neither_a_lossy_song_to_replace_nor_a_copy_to_replace_one_with() -> Result<()>
{
    let tree = Tree::new();
    let (Some(_), Some(_), Some(fearless)) = (
        encoded_with_tags(&tree, "whole.opus", &[]),
        encoded_with_tags(&tree, "album.flac", &[]),
        encoded_as_pink_floyd(&tree, "fearless.mp3", "Fearless"),
    ) else {
        return Ok(());
    };

    tree.write("whole.cue", A_SHEET_CUTTING_A_LOSSY_FILE.as_bytes());
    tree.write("album.cue", A_SHEET_CUTTING_A_LOSSLESS_FILE.as_bytes());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let cuts: Vec<Cut> = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .filter(|row| row.span.is_some())
        .map(|row| Cut {
            location: row.location,
            span: row.span,
        })
        .collect();

    assert_eq!(cuts.len(), 4, "the sheets did not cut their files");

    let mut every = cuts;

    every.push(whole(&fearless));

    let playlist = library.start_playlist("Cuts", &every)?;

    let listed = library.songs_to_better_in(playlist)?;

    assert_eq!(listed.len(), 1, "a cut was listed as a lossy song");
    assert_eq!(listed[0].track.title, "Fearless");
    assert!(
        listed[0].held.is_none(),
        "a cut of a sheet was offered as the copy"
    );
    Ok(())
}

#[test]
fn a_song_playing_a_lesser_copy_is_offered_the_best_and_one_playing_the_best_is_not_listed()
-> Result<()> {
    let tree = Tree::new();
    let tags = pink_floyd("Echoes");
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();

    let (Some(opus), Some(better), Some(chosen)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_lasting(
            &tree,
            "one/A.flac",
            &tags,
            TWO_SECONDS_OF_FRAMES,
            &["-ar", "96000"],
        ),
        encoded_as_pink_floyd(&tree, "two/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let song = library.track_at(&opus, None)?.expect("the song is held").id;
    let chosen = library.track_at(&chosen, None)?.expect("held").id;
    let playlist = library.start_playlist("Dark", &[whole(&opus)])?;

    let better = library.track_at(&better, None)?.expect("held");
    let chosen_row = library.track(chosen)?.expect("held");

    assert!(
        Quality::of(&better).is_above(Quality::of(&chosen_row)),
        "the copy chosen is not the lesser of the two"
    );
    assert!(library.choose_version(song, chosen)?);

    let listed = library.songs_to_better_in(playlist)?;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].held.as_ref().map(|copy| copy.id), Some(better.id));

    assert!(library.choose_version(song, better.id)?);
    assert!(library.songs_to_better_in(playlist)?.is_empty());
    Ok(())
}

fn played_through(library: &Library, path: &Path, times: usize) -> Result<()> {
    for _ in 0..times {
        library
            .track_played(&MediaLocation::local(path), None, Duration::from_secs(2))?
            .expect("a play of a catalogued file is counted");
    }
    Ok(())
}

fn paired_on_a_release(library: &Library, songs: &[&Track]) -> Result<Vec<ReleaseTrackId>> {
    let album = songs[0].album_id.expect("the song files under an album");
    let rows = songs
        .iter()
        .zip(1..)
        .map(|(song, position)| release_row(position, &song.title, Vec::new()))
        .collect();

    library.land_release(album, &orbits(rows, Vec::new()))?;
    library.rematch(album)?;

    let held = library.release_tracks(album)?;
    Ok(songs
        .iter()
        .map(|song| {
            held.iter()
                .find(|row| row.track == Some(song.id))
                .map(|row| row.id)
                .expect("the release row is paired with the song")
        })
        .collect())
}

#[cfg(unix)]
fn trashed(data: &Tree, name: &str) -> bool {
    data.path().join("Trash/files").join(name).is_file()
}

#[test]
#[cfg(unix)]
fn a_lossy_song_with_a_lossless_copy_held_plays_it_by_default_and_keeps_everything_it_had()
-> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;
    let lossless = the_row_at(&library, &flac)?;

    played_through(&library, &opus, 3)?;
    library.favour(Favoured::Track(lossy.id), true)?;

    let road_trip = library.start_playlist("Road trip", &[whole(&opus), whole(&opus)])?;

    let replacing = library.replace(&library.songs_to_better_in(road_trip)?)?;

    assert_eq!(
        (replacing.beside, replacing.wanted, replacing.unmatched),
        (1, 0, 0),
        "{replacing:?}"
    );
    assert!(opus.exists() && flac.exists());
    assert!(!trashed(&data, "A.opus"));

    let song = library.track(lossy.id)?.expect("the lossy song stays");

    assert_eq!(song.plays, 3);
    assert!(song.favourite.is_some());
    assert_eq!(
        library.versions_chosen()?,
        [ChosenVersion {
            track: lossy.id,
            copy: lossless.id,
        }]
    );

    let mut varied = library.tracks_with_variants()?;
    let mut both = [lossy.id, lossless.id];

    varied.sort();
    both.sort();

    assert_eq!(varied, both);

    let variants = library.variants_of(lossless.id)?.expect("the song stands");

    assert_eq!(variants.song.id, lossy.id);
    assert_eq!(
        variants.held.iter().map(|copy| copy.id).collect::<Vec<_>>(),
        [lossless.id]
    );
    assert_eq!(variants.plays(), lossless.id);

    let cuts = library.playlist_cuts(road_trip)?;

    assert!(
        cuts.iter()
            .all(|cut| cut.location.as_path() == Some(opus.as_path())),
        "the playlist no longer names its own song: {cuts:?}"
    );
    assert!(library.songs_to_better_in(road_trip)?.is_empty());

    assert!(library.forget_the_version_chosen(lossy.id)?);

    let variants = library.variants_of(lossy.id)?.expect("the song stands");

    assert_eq!(variants.plays(), lossy.id);
    assert_eq!(
        variants.held.len(),
        1,
        "forgetting the choice let the variant go"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossless_song_plays_a_copy_of_higher_resolution_and_never_one_alike() -> Result<()> {
    let tree = Tree::new();
    let tags = pink_floyd("Echoes");
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();

    let (Some(plain), Some(alike), Some(finer)) = (
        encoded_as_pink_floyd(&tree, "cd/A.flac", "Echoes"),
        encoded_as_pink_floyd(&tree, "again/A.flac", "Echoes"),
        encoded_lasting(
            &tree,
            "studio/A.flac",
            &tags,
            TWO_SECONDS_OF_FRAMES,
            &["-ar", "96000"],
        ),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let song = the_row_at(&library, &plain)?;
    let alike = the_row_at(&library, &alike)?;
    let finer = the_row_at(&library, &finer)?;
    let offered = |copy: &Track| Replaceable {
        track: song.clone(),
        held: Some(copy.clone()),
        release_track: None,
        pending: false,
        looked: None,
    };

    let refused = library.replace(&[offered(&alike)])?;
    let replacing = library.replace(&[offered(&finer)])?;

    assert_eq!(refused.beside, 0, "{refused:?}");
    assert_eq!(replacing.beside, 1, "{replacing:?}");
    assert!(plain.exists());
    assert_eq!(library.version_chosen_for(song.id)?, Some(finer.id));
    Ok(())
}

#[test]
#[cfg(unix)]
fn two_lossy_copies_of_one_song_both_play_the_one_lossless_copy() -> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(mp3), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/A.mp3", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus), whole(&mp3)])?;

    let replacing = library.replace(&library.songs_to_better_in(playlist)?)?;

    let flac = the_row_at(&library, &flac)?.id;

    assert_eq!(replacing.beside, 2, "{replacing:?}");
    assert!(opus.exists() && mp3.exists());

    for lossy in [&opus, &mp3] {
        let song = the_row_at(&library, lossy)?.id;

        assert_eq!(library.version_chosen_for(song)?, Some(flac));
    }
    Ok(())
}

#[test]
fn a_lossy_song_matched_on_musicbrainz_is_wanted_and_waits_while_one_with_neither_is_counted_unmatched()
-> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;
    let paired = paired_on_a_release(&library, &[&lossy])?[0];
    let playlist = library.start_playlist("Road trip", &[whole(&opus), whole(&mp3)])?;

    let replacing = library.replace(&library.songs_to_better_in(playlist)?)?;

    assert_eq!(
        (replacing.beside, replacing.wanted, replacing.unmatched),
        (0, 1, 1),
        "{replacing:?}"
    );

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(replacing.wants, [(paired, wants[0].id)]);
    assert_eq!(wants[0].release_track, paired);
    assert_eq!(wants[0].replacing, Some(lossy.id));
    assert!(
        !wants[0].is_answered(),
        "the lossy copy answered its own replacement"
    );
    assert!(opus.exists());

    let listed = library.songs_to_better_in(playlist)?;
    let waiting = listed
        .iter()
        .find(|row| row.track.id == lossy.id)
        .expect("the lossy song is still listed");

    assert!(waiting.pending);
    Ok(())
}

#[test]
fn a_lossy_song_a_want_once_delivered_is_wanted_again_with_its_offer_forgotten_and_its_refusals_kept()
-> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;
    let paired = paired_on_a_release(&library, &[&lossy])?[0];
    let want = library.want(paired)?;

    {
        let other = beside(&database);

        other
            .execute(
                "UPDATE wants SET offered = ?1, tried = 5, misses = 2 WHERE id = ?2",
                rusqlite::params![MediaLocation::local(&opus).to_uri(), want.get() as i64],
            )
            .expect("the want names the lossy delivery");
        other
            .execute(
                "INSERT INTO refused_offers (want_id, taken_from, refused) VALUES (?1, 'inbox:short', 1)",
                [want.get() as i64],
            )
            .expect("an offer was refused for the want");
    }

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;

    let replacing = library.replace(&library.songs_to_better_in(playlist)?)?;

    assert_eq!(replacing.wanted, 1, "{replacing:?}");

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1);
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].offered, None);
    assert_eq!(wants[0].tried, None);
    assert_eq!(wants[0].misses, 0);

    let refused: i64 = beside(&database)
        .query_row(
            "SELECT count(*) FROM refused_offers WHERE want_id = ?1",
            [want.get() as i64],
            |row| row.get(0),
        )
        .expect("the refusals are counted");

    assert_eq!(refused, 1, "re-arming the want forgot what it refused");
    Ok(())
}

fn looked_for(
    library: &Library,
    providers: &Providers,
    rows: &[Replaceable],
) -> Result<Vec<(TrackId, Looked)>> {
    let told = Mutex::new(Vec::new());

    library.look_for_replacements(
        providers,
        rows,
        &LookingFor {
            cancelled: &|| false,
            told: &|track, looked| told.lock().push((track, looked)),
        },
    )?;
    Ok(told.into_inner())
}

fn offering_as(source: &str, extension: &'static str) -> Arc<Offering> {
    Arc::new(Offering::new(
        source,
        Delivering::Bytes {
            key: "track/1",
            extension,
            bytes: vec![0; 16],
        },
    ))
}

#[test]
fn a_lossy_song_is_found_where_a_provider_offers_it_lossless_and_the_finding_is_kept() -> Result<()>
{
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;

    paired_on_a_release(&library, &[&lossy])?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;
    let lossy_only = offering_as("one", "mp3");
    let lossless = offering_as("two", "flac");
    let rows = library.songs_to_better_in(playlist)?;

    assert_eq!(rows[0].looked, None);

    let told = looked_for(&library, &asking_in_turn(&[&lossy_only, &lossless]), &rows)?;

    let by = SourceId::new("two").expect("a nameable source");
    let two = Looked::Offered {
        at: MediaLocation::new(by.clone(), "track/1"),
        by,
    };

    assert_eq!(told, [(lossy.id, two.clone())]);
    assert_eq!(lossy_only.asked().len(), 1);
    assert_eq!(lossless.asked().len(), 1);
    assert_eq!(lossless.opened(), 0, "looking for a copy downloaded it");
    assert_eq!(library.songs_to_better_in(playlist)?[0].looked, Some(two));
    assert!(library.wants()?.is_empty(), "looking for a copy wanted it");
    Ok(())
}

#[test]
fn a_lossy_song_offered_only_lossy_is_offered_nowhere_whether_matched_or_not() -> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;

    paired_on_a_release(&library, &[&lossy])?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus), whole(&mp3)])?;
    let lossy_only = offering_as("one", "opus");

    let told = looked_for(
        &library,
        &lossy_only.registered(),
        &library.songs_to_better_in(playlist)?,
    )?;

    let unmatched = the_row_at(&library, &mp3)?;
    let mut told = told;

    told.sort_by_key(|(track, _)| *track);

    let mut expected = [
        (lossy.id, Looked::Unoffered),
        (unmatched.id, Looked::Unoffered),
    ];

    expected.sort_by_key(|(track, _)| *track);

    assert_eq!(told, expected);
    assert_eq!(lossy_only.asked().len(), 2);

    let asked_unmatched = lossy_only
        .asked()
        .into_iter()
        .find(|identity| identity.title == "Time")
        .expect("the unmatched song was asked about by its tags");

    assert_eq!(asked_unmatched.recording, None);
    assert!(asked_unmatched.artist.is_some());
    assert!(asked_unmatched.length.is_some());

    let listed = library.songs_to_better_in(playlist)?;
    let found = |id| {
        listed
            .iter()
            .find(|row| row.track.id == id)
            .expect("the lossy song is listed")
    };

    assert!(found(lossy.id).is_offered_nowhere());
    assert!(found(lossy.id).can_be_ticked());
    assert!(found(unmatched.id).is_offered_nowhere());
    assert!(!found(unmatched.id).can_be_ticked());
    Ok(())
}

#[test]
fn a_look_a_provider_could_not_answer_keeps_nothing() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;

    paired_on_a_release(&library, &[&lossy])?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;
    let away = Unanswering::new("away", unreachable_server);
    let providers = Providers::none().and(Arc::clone(&away) as Arc<dyn Provider>);

    let told = looked_for(&library, &providers, &library.songs_to_better_in(playlist)?)?;

    assert_eq!(told, [(lossy.id, Looked::Unheard)]);
    assert_eq!(away.asked(), 1);
    assert_eq!(library.songs_to_better_in(playlist)?[0].looked, None);
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_song_matched_nowhere_is_found_by_its_tags_kept_from_the_service_and_plays_in_the_lossy_songs_place()
-> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.flac", "Echoes"),
    ) else {
        return Ok(());
    };

    FileTags::over(Sources::local())
        .write(
            &MediaLocation::local(&opus),
            resonate_codec::Writing {
                edits: &[],
                taken: &[],
                picture: Some(&png_art(8)),
                unpictured: false,
                popularity: None,
            },
        )
        .expect("the lossy song takes a picture");

    let data = Tree::new();
    let library = Library::open(&tree.path().join("library.db"))?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;

    played_through(&library, &opus, 2)?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;
    let store = Arc::new(Offering::new(
        "store",
        Delivering::Bytes {
            key: "track/77",
            extension: "flac",
            bytes: fs::read(&flac).expect("the lossless copy reads"),
        },
    ));
    let rows = library.songs_to_better_in(playlist)?;

    assert_eq!(rows[0].release_track, None, "the song is matched nowhere");
    assert!(!rows[0].can_be_ticked());

    looked_for(&library, &store.registered(), &rows)?;

    let rows = library.songs_to_better_in(playlist)?;

    assert!(rows[0].can_be_ticked(), "{rows:?}");
    assert_eq!(store.opened(), 0);

    let unfiled = library.replace(&rows)?;

    assert_eq!((unfiled.kept, unfiled.unfiled), (0, 1), "{unfiled:?}");
    assert_eq!(library.keeps_queued()?, 0);

    filing_into(&library, &tree);

    let replacing = library.replace(&rows)?;

    assert_eq!((replacing.kept, replacing.wanted), (1, 0), "{replacing:?}");
    assert!(library.songs_to_better_in(playlist)?[0].pending);
    assert!(library.wants()?.is_empty());

    let summary = library.keep_pending(&store.registered(), &|| false)?;

    assert_eq!(summary.landed, 1, "{summary:?}");
    assert!(opus.exists(), "the lossy copy went");
    assert!(!trashed(&data, "A.opus"));
    assert!(library.songs_to_better_in(playlist)?.is_empty());

    let cuts = library.playlist_cuts(playlist)?;

    assert_eq!(cuts.len(), 1);
    assert_eq!(cuts[0].location, MediaLocation::local(&opus));

    let played = library
        .version_chosen_for(lossy.id)?
        .expect("the kept copy plays in the song's place");
    let kept = library
        .track(played)?
        .expect("the kept copy is held")
        .location
        .as_path()
        .expect("the kept copy is a file")
        .to_path_buf();

    assert_eq!(kept.extension(), Some(OsStr::new("flac")));
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);

    let carried = FileTags::over(Sources::local())
        .read(&MediaLocation::local(&kept), Picturing::Whether)
        .expect("the kept copy reads");

    assert!(
        carried.picture.carries_one(),
        "the lossless copy lost the lossy song's picture"
    );
    Ok(())
}

#[test]
fn a_song_kept_to_replace_a_lossy_one_that_arrives_lossy_is_refused_and_the_lossy_song_stays()
-> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.mp3", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;

    filing_into(&library, &tree);

    let by = SourceId::new("store").expect("a nameable source");
    let mut rows = library.songs_to_better_in(playlist)?;

    rows[0].looked = Some(Looked::Offered {
        at: MediaLocation::new(by.clone(), "track/77"),
        by,
    });

    assert_eq!(library.replace(&rows)?.kept, 1);

    let store = Arc::new(Offering::new(
        "store",
        Delivering::Bytes {
            key: "track/77",
            extension: "flac",
            bytes: fs::read(&mp3).expect("the lossy copy reads"),
        },
    ));

    let summary = library.keep_pending(&store.registered(), &|| false)?;

    assert_eq!((summary.landed, summary.unkept), (0, 1), "{summary:?}");
    assert!(opus.exists());
    assert!(files_named(tree.path(), "flac").is_empty());

    let listed = library.songs_to_better_in(playlist)?;

    assert_eq!(listed.len(), 1);
    assert!(!listed[0].pending, "a refused keep still holds the song");
    Ok(())
}

#[test]
fn a_song_kept_to_replace_a_lossy_one_is_never_downloaded_from_an_offer_named_for_lossy_sound()
-> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.mp3", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;

    filing_into(&library, &tree);

    let by = SourceId::new("store").expect("a nameable source");
    let mut rows = library.songs_to_better_in(playlist)?;

    rows[0].looked = Some(Looked::Offered {
        at: MediaLocation::new(by.clone(), "track/77"),
        by,
    });

    assert_eq!(library.replace(&rows)?.kept, 1);

    let store = Arc::new(Offering::new(
        "store",
        Delivering::Bytes {
            key: "track/77",
            extension: "mp3",
            bytes: fs::read(&mp3).expect("the lossy copy reads"),
        },
    ));

    let summary = library.keep_pending(&store.registered(), &|| false)?;

    assert_eq!(summary.landed, 0, "{summary:?}");
    assert_eq!(store.asked().len(), 1);
    assert_eq!(
        store.opened(),
        0,
        "an offer named for lossy sound was opened"
    );
    assert!(opus.exists());
    assert!(files_named(tree.path(), "mp3").is_empty());
    Ok(())
}

#[test]
fn a_last_resort_is_never_asked_whether_a_lossy_song_is_held_lossless() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;
    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;
    let tube = offering_as("tube", "flac");
    let shop = offering_as("shop", "flac");

    let providers = Providers::none()
        .and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>)
        .and(Arc::clone(&shop) as Arc<dyn Provider>);

    let told = looked_for(&library, &providers, &library.songs_to_better_in(playlist)?)?;
    let alone = looked_for(
        &library,
        &Providers::none().and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>),
        &library.songs_to_better_in(playlist)?,
    )?;

    let by = SourceId::new("shop").expect("a nameable source");

    assert_eq!(
        told,
        [(
            lossy.id,
            Looked::Offered {
                at: MediaLocation::new(by.clone(), "track/1"),
                by,
            }
        )]
    );
    assert!(alone.is_empty(), "{alone:?}");
    assert!(tube.asked().is_empty(), "a last resort was asked");
    assert_eq!(shop.asked().len(), 1);
    Ok(())
}

struct HeldOpenByTheDesktop {
    reached: Mutex<Option<Sender<()>>>,
    lifted: Mutex<Receiver<()>>,
}

impl Trashes for HeldOpenByTheDesktop {
    fn trash(&self, file: &Path) -> Removal {
        if let Some(reached) = self.reached.lock().take() {
            reached.send(()).expect("the test waits for the trash");
        }
        self.lifted.lock().recv().expect("the test lifts the trash");
        fs::remove_file(file).expect("the desktop takes the file");
        Removal::Removed
    }
}

#[test]
fn a_replacement_is_kept_while_another_pass_walks_the_tree_since_it_moves_no_file() -> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(flac), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
    ) else {
        return Ok(());
    };
    let doomed = tree.write("doomed.wav", &Wav::new().text(TITLE, "doomed").build());
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;
    let lossless = the_row_at(&library, &flac)?;
    let unmatched = the_row_at(&library, &mp3)?;

    paired_on_a_release(&library, &[&unmatched])?;

    let doomed = the_row_at(&library, &doomed)?.id;
    let playlist = library.start_playlist("Road trip", &[whole(&opus), whole(&mp3)])?;
    let listed = library.songs_to_better_in(playlist)?;

    assert!(listed.iter().all(Replaceable::can_be_ticked), "{listed:?}");

    let (reached, reaches) = mpsc::channel();
    let (lift, lifted) = mpsc::channel();

    library.trash_through(Some(Arc::new(HeldOpenByTheDesktop {
        reached: Mutex::new(Some(reached)),
        lifted: Mutex::new(lifted),
    })));

    let replacing = thread::scope(|scope| -> Result<Replacing> {
        let deleting = scope.spawn(|| library.delete_tracks(&[doomed]));

        reaches.recv().expect("the deletion reached the trash");

        let replacing = library.replace(&listed);

        lift.send(()).expect("the deletion still waits");
        deleting.join().expect("the deletion finished")?;
        replacing
    })?;

    assert_eq!(
        (replacing.beside, replacing.wanted),
        (1, 1),
        "{replacing:?}"
    );
    assert_eq!(library.version_chosen_for(lossy.id)?, Some(lossless.id));
    assert_eq!(library.wants()?.len(), 1);
    assert!(opus.exists() && flac.exists() && mp3.exists());
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_vaulted_lossy_song_plays_its_lossless_copy_and_keeps_its_file_and_object() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let (library, _vault) = opened_with_a_vault(&held)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let object = library
        .vault_objects()?
        .into_iter()
        .find(|object| object.taken_from == opus)
        .expect("the lossy song went into the vault");

    played_through(&library, &opus, 2)?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;

    let replacing = library.replace(&library.songs_to_better_in(playlist)?)?;

    assert_eq!(replacing.beside, 1, "{replacing:?}");
    assert!(opus.exists());
    assert!(!trashed(&data, "A.opus"));
    assert!(
        library
            .vault_objects()?
            .iter()
            .any(|kept| kept.key == object.key),
        "the lossy song's vault object was let go"
    );
    assert!(object.path.exists());

    let song = the_row_at(&library, &opus)?;

    assert_eq!(song.plays, 2);
    assert_eq!(
        library.version_chosen_for(song.id)?,
        Some(the_row_at(&library, &flac)?.id)
    );
    Ok(())
}

#[test]
fn a_lossy_song_is_not_replaced_by_a_lossless_copy_whose_file_is_gone() -> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    played_through(&library, &opus, 1)?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;
    let listed = library.songs_to_better_in(playlist)?;

    assert!(listed[0].held.is_some());

    fs::remove_file(&flac).expect("the lossless copy goes");

    let replacing = library.replace(&listed)?;

    assert_eq!(
        (replacing.beside, replacing.unmatched),
        (0, 1),
        "{replacing:?}"
    );
    assert!(opus.exists());
    assert_eq!(the_row_at(&library, &opus)?.plays, 1);
    Ok(())
}

#[test]
fn dismissing_a_replacements_want_cancels_it_and_leaves_the_lossy_song() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let lossy = the_row_at(&library, &opus)?;

    paired_on_a_release(&library, &[&lossy])?;

    let playlist = library.start_playlist("Road trip", &[whole(&opus)])?;

    library.replace(&library.songs_to_better_in(playlist)?)?;

    let want = library.wants()?.remove(0);

    assert_eq!(want.replacing, Some(lossy.id));

    assert!(library.unwant(want.id)?);

    let listed = library.songs_to_better_in(playlist)?;

    assert_eq!(listed.len(), 1);
    assert!(!listed[0].pending, "the replacement outlived its want");
    assert!(library.wants()?.is_empty());
    assert!(opus.exists());
    Ok(())
}

fn paired_as_the_recording(
    library: &Library,
    song: &Track,
    recording: Option<&str>,
    release: &str,
) -> Result<ReleaseTrackId> {
    let album = song.album_id.expect("the song files under an album");
    let mut row = release_row(1, &song.title, Vec::new());

    row.recording = recording.map(mbid);
    library.land_release(
        album,
        &Release {
            id: mbid(release),
            ..orbits(vec![row], Vec::new())
        },
    )?;
    library.rematch(album)?;
    Ok(library
        .release_tracks(album)?
        .into_iter()
        .find(|row| row.track == Some(song.id))
        .map(|row| row.id)
        .expect("the release row is paired with the song"))
}

struct Waiting {
    lossy: Track,
    row: ReleaseTrackId,
    playlist: PlaylistId,
}

fn waiting_to_be_replaced(
    library: &Library,
    opus: &Path,
    recording: Option<&str>,
) -> Result<Waiting> {
    let lossy = the_row_at(library, opus)?;
    let row = paired_as_the_recording(library, &lossy, recording, RELEASE)?;

    played_through(library, opus, 2)?;

    let playlist = library.start_playlist("Road trip", &[whole(opus), whole(opus)])?;
    let replacing = library.replace(&library.songs_to_better_in(playlist)?)?;

    assert_eq!(replacing.wanted, 1, "{replacing:?}");
    Ok(Waiting {
        lossy,
        row,
        playlist,
    })
}

fn replacements_of(database: &Path, track: TrackId) -> Vec<Option<i64>> {
    let connection = beside(database);
    let mut statement = connection
        .prepare("SELECT release_track_id FROM replacements WHERE track_id = ?1")
        .expect("the replacements are read");
    statement
        .query_map([track.get() as i64], |row| row.get(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .expect("the replacements are read")
}

fn still_pending(library: &Library, waiting: &Waiting) -> Result<bool> {
    Ok(library
        .songs_to_better_in(waiting.playlist)?
        .iter()
        .any(|row| row.track.id == waiting.lossy.id && row.pending))
}

fn tagged_as_the_recording(tree: &Tree, name: &str, recording: &str) -> Option<PathBuf> {
    let mut tags = pink_floyd("Echoes").to_vec();

    tags.push(format!("MUSICBRAINZ_TRACKID={recording}"));

    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    encoded_with_tags(tree, name, &tags)
}

#[test]
#[cfg(unix)]
fn a_lossless_copy_delivered_for_a_replacement_plays_in_the_lossy_songs_place() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let data = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let mut tags = pink_floyd("Echoes").to_vec();

    tags.push(format!("MUSICBRAINZ_ALBUMID={RELEASE}"));

    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    let Some(flac) = encoded_with_tags(&tree, "lossless/A.flac", &tags) else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;

    assert!(
        still_pending(&library, &waiting)?,
        "a copy named alike was taken before it was offered"
    );

    offered(&library, &flac)?;

    let settled = library.settle_replacements()?;

    assert_eq!(settled.beside, 1, "{settled:?}");

    let kept = the_row_at(&library, &flac)?;

    assert_eq!(library.version_chosen_for(waiting.lossy.id)?, Some(kept.id));
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);

    let cuts = library.playlist_cuts(waiting.playlist)?;

    assert_eq!(cuts.len(), 2);
    assert!(
        cuts.iter()
            .all(|cut| cut.location.as_path() == Some(opus.as_path())),
        "the playlist no longer names its own song: {cuts:?}"
    );

    let album = waiting
        .lossy
        .album_id
        .expect("the song files under an album");
    let row = library
        .release_tracks(album)?
        .into_iter()
        .find(|row| row.id == waiting.row)
        .expect("the release row stays");

    assert_eq!(row.track, Some(waiting.lossy.id));
    assert!(library.wants()?.is_empty(), "the want outlived its answer");
    assert!(replacements_of(&database, waiting.lossy.id).is_empty());
    assert!(opus.exists());
    assert!(!trashed(&data, "A.opus"));
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossless_copy_holding_the_same_recording_plays_in_the_lossy_songs_place() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let data = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, Some(RECORDING))?;
    let Some(flac) = tagged_as_the_recording(&tree, "lossless/A.flac", RECORDING) else {
        return Ok(());
    };

    let scanned = library.scan(options(&tree))?.join()?;

    assert_eq!(
        scanned.replaced.beside, 1,
        "the scan kept quiet about the copy"
    );
    assert!(opus.exists(), "the scan took the lossy song away");
    assert!(!trashed(&data, "A.opus"));
    assert_eq!(
        library.version_chosen_for(waiting.lossy.id)?,
        Some(the_row_at(&library, &flac)?.id)
    );
    assert!(replacements_of(&database, waiting.lossy.id).is_empty());
    assert!(library.wants()?.is_empty());
    Ok(())
}

#[test]
fn a_lossless_copy_named_alike_alone_never_replaces_a_song_unattended() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, Some(RECORDING))?;
    let Some(flac) = encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes") else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;

    let settled = library.settle_replacements()?;

    assert_eq!(settled, Replaced::default());
    assert!(still_pending(&library, &waiting)?);
    assert!(opus.exists());
    assert!(flac.exists());
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    assert_eq!(library.wants()?[0].release_track, waiting.row);
    Ok(())
}

#[test]
fn a_lossy_copy_offered_for_a_replacement_is_never_taken_for_it() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, Some(RECORDING))?;
    let Some(other) = tagged_as_the_recording(&tree, "delivered/A.opus", RECORDING) else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;
    offered(&library, &other)?;

    let settled = library.settle_replacements()?;

    assert_eq!(settled, Replaced::default());
    assert!(still_pending(&library, &waiting)?);
    assert!(opus.exists());
    assert!(other.exists());
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_cancelled_scan_settles_no_replacement() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let data = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let Some(flac) = encoded_as_pink_floyd(&tree, "lossless/A.flac", "Echoes") else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;
    offered(&library, &flac)?;
    for index in 0..200 {
        tree.write(
            &format!("padding/{index}.wav"),
            &Wav::new().frames(64).text(TITLE, "padding").build(),
        );
    }

    let handle = library.scan(ScanOptions {
        incremental: false,
        ..options(&tree)
    })?;

    handle.cancel();

    let summary = handle.join()?;

    assert!(summary.cancelled, "the scan outran the cancel");
    assert!(
        still_pending(&library, &waiting)?,
        "a cancelled scan settled"
    );
    assert!(opus.exists());
    assert_eq!(library.version_chosen_for(waiting.lossy.id)?, None);
    assert_eq!(library.settle_replacements()?.beside, 1);
    assert!(opus.exists());
    Ok(())
}

#[test]
fn a_replacement_whose_release_is_landed_again_rejoins_the_row_its_song_now_holds() -> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;

    assert_eq!(
        replacements_of(&database, waiting.lossy.id),
        [Some(waiting.row.get() as i64)]
    );

    let album = waiting
        .lossy
        .album_id
        .expect("the song files under an album");

    library.land_release(
        album,
        &orbits(
            vec![
                release_row(1, "Speak to Me", Vec::new()),
                release_row(2, &waiting.lossy.title, Vec::new()),
            ],
            Vec::new(),
        ),
    )?;
    library.rematch(album)?;

    let again = library
        .release_tracks(album)?
        .into_iter()
        .find(|row| row.track == Some(waiting.lossy.id))
        .map(|row| row.id)
        .expect("the song holds a row of the release landed again");

    assert_ne!(again, waiting.row, "the release was not landed again");
    assert_eq!(replacements_of(&database, waiting.lossy.id), [None]);

    assert_eq!(library.settle_replacements()?, Replaced::default());

    assert_eq!(
        replacements_of(&database, waiting.lossy.id),
        [Some(again.get() as i64)]
    );

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1, "{wants:?}");
    assert_eq!(wants[0].release_track, again);
    assert_eq!(wants[0].replacing, Some(waiting.lossy.id));
    assert!(still_pending(&library, &waiting)?);
    assert!(opus.exists());
    Ok(())
}

#[test]
fn a_replacement_whose_match_is_forgotten_waits_unpending_and_rejoins_once_matched_again()
-> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let album = waiting
        .lossy
        .album_id
        .expect("the song files under an album");

    assert!(library.forget_the_match(album)?);
    assert_eq!(replacements_of(&database, waiting.lossy.id), [None]);

    assert_eq!(library.settle_replacements()?, Replaced::default());

    assert_eq!(
        replacements_of(&database, waiting.lossy.id),
        [None],
        "a replacement left between a release's landing and its rematch was cancelled"
    );
    assert!(!still_pending(&library, &waiting)?);
    assert!(opus.exists());
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);

    let again = paired_as_the_recording(&library, &waiting.lossy, None, RELEASE)?;

    assert_eq!(library.settle_replacements()?, Replaced::default());

    assert_eq!(
        replacements_of(&database, waiting.lossy.id),
        [Some(again.get() as i64)]
    );

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1, "{wants:?}");
    assert_eq!(wants[0].release_track, again);
    assert_eq!(wants[0].replacing, Some(waiting.lossy.id));
    assert!(still_pending(&library, &waiting)?);
    assert!(opus.exists());
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossless_copy_on_another_album_keeps_its_own_row_and_the_lossy_song_keeps_its_own()
-> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(_)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, Some(RECORDING))?;
    let tags = [
        "title=Echoes".to_owned(),
        "artist=Pink Floyd".to_owned(),
        "album=Live at Pompeii".to_owned(),
    ];
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    let Some(flac) = encoded_with_tags(&tree, "live/A.flac", &tags) else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;

    let live = the_row_at(&library, &flac)?;
    let own = paired_as_the_recording(&library, &live, Some(RECORDING), HOURS)?;
    let live_album = live.album_id.expect("the copy files under an album");

    assert_ne!(live_album, waiting.lossy.album_id.expect("filed"));

    offered(&library, &flac)?;

    let settled = library.settle_replacements()?;

    assert_eq!(settled.beside, 1, "{settled:?}");

    let held_own = library
        .release_tracks(live_album)?
        .into_iter()
        .find(|row| row.id == own)
        .expect("the copy's own row stays");

    assert_eq!(held_own.track, Some(live.id));

    let lossy_album = waiting.lossy.album_id.expect("filed");
    let left = library
        .release_tracks(lossy_album)?
        .into_iter()
        .find(|row| row.id == waiting.row)
        .expect("the lossy song's row stays on its album");

    assert_eq!(left.track, Some(waiting.lossy.id));
    assert!(opus.exists());
    assert_eq!(library.version_chosen_for(waiting.lossy.id)?, Some(live.id));
    Ok(())
}

#[cfg(unix)]
const EIGHT_SECONDS_OF_FRAMES: usize = 352_800;

#[cfg(unix)]
fn live_at_pompeii(recording: &str) -> [String; 4] {
    [
        "title=Echoes".to_owned(),
        "artist=Pink Floyd".to_owned(),
        "album=Live at Pompeii".to_owned(),
        format!("MUSICBRAINZ_TRACKID={recording}"),
    ]
}

#[cfg(unix)]
fn wanted_with_no_copy_held(library: &Library, opus: &Path) -> Result<Waiting> {
    let lossy = the_row_at(library, opus)?;
    let row = paired_as_the_recording(library, &lossy, Some(RECORDING), RELEASE)?;

    played_through(library, opus, 2)?;

    let playlist = library.start_playlist("Road trip", &[whole(opus), whole(opus)])?;
    let replacing = library.replace(&[Replaceable {
        track: the_row_at(library, opus)?,
        held: None,
        release_track: Some(row),
        pending: false,
        looked: None,
    }])?;

    assert_eq!(replacing.wanted, 1, "{replacing:?}");
    Ok(Waiting {
        lossy,
        row,
        playlist,
    })
}

#[test]
#[cfg(unix)]
fn a_replacement_whose_row_another_song_now_holds_is_never_settled_by_that_rows_delivery()
-> Result<()> {
    let tree = Tree::new();
    let (Some(opus), Some(mp3), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&tree, "lossy/B.mp3", "Time"),
        encoded_as_pink_floyd(&tree, "delivered/B.flac", "Time"),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let time = the_row_at(&library, &mp3)?;

    beside(&database)
        .execute(
            "UPDATE release_tracks SET track_id = ?1 WHERE id = ?2",
            [time.id.get() as i64, waiting.row.get() as i64],
        )
        .expect("the row is placed on another song");
    offered(&library, &flac)?;

    let settled = library.settle_replacements()?;

    assert_eq!(
        settled,
        Replaced::default(),
        "a different song took its place"
    );
    assert!(opus.exists());
    assert!(!trashed(&data, "A.opus"));
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    assert_eq!(the_row_at(&library, &flac)?.plays, 0);
    assert_eq!(
        replacements_of(&database, waiting.lossy.id),
        [None],
        "the replacement still names the row another song holds"
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossless_copy_of_the_recording_lasting_otherwise_never_replaces_a_song_unattended()
-> Result<()> {
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let data = Tree::new();
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, Some(RECORDING))?;
    let mut tags = pink_floyd("Echoes").to_vec();

    tags.push(format!("MUSICBRAINZ_TRACKID={RECORDING}"));

    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();
    let Some(flac) = encoded_lasting(
        &tree,
        "lossless/A.flac",
        &tags,
        EIGHT_SECONDS_OF_FRAMES,
        &[],
    ) else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;

    assert!(
        opus.exists(),
        "a copy lasting six seconds longer took its place"
    );
    assert!(still_pending(&library, &waiting)?);
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    assert_eq!(the_row_at(&library, &flac)?.plays, 0);
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_vaulted_copy_whose_object_is_gone_never_replaces_a_lossy_song() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let tags = live_at_pompeii(RECORDING);
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();

    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_with_tags(&tree, "live/A.flac", &tags),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let (library, _vault) = opened_with_a_vault(&held)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let object = library
        .vault_objects()?
        .into_iter()
        .find(|object| object.taken_from == flac)
        .expect("the lossless copy went into the vault");
    let waiting = wanted_with_no_copy_held(&library, &opus)?;

    fs::remove_file(&flac).expect("the lossless file goes");
    fs::remove_file(&object.path).expect("the lossless copy's object goes");

    let settled = library.settle_replacements()?;

    assert_eq!(settled, Replaced::default(), "{settled:?}");
    assert!(opus.exists());
    assert!(library.track(waiting.lossy.id)?.is_some());
    assert!(still_pending(&library, &waiting)?);
    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    Ok(())
}

#[test]
#[cfg(unix)]
fn the_best_of_two_lossless_copies_of_the_recording_plays_in_the_lossy_songs_place() -> Result<()> {
    let tree = Tree::new();
    let tags = live_at_pompeii(RECORDING);
    let tags: Vec<&str> = tags.iter().map(String::as_str).collect();

    let (Some(opus), Some(plain)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_with_tags(&tree, "live/A.flac", &tags),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let library = Library::open_in_memory()?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = wanted_with_no_copy_held(&library, &opus)?;
    let Some(finer) = encoded_lasting(
        &tree,
        "live/B.flac",
        &tags,
        TWO_SECONDS_OF_FRAMES,
        &["-ar", "96000"],
    ) else {
        return Ok(());
    };

    scan(&library, &options(&tree))?;

    let plain = the_row_at(&library, &plain)?;
    let finer = the_row_at(&library, &finer)?;

    assert!(plain.id < finer.id, "the finer copy was catalogued first");
    assert_eq!(finer.spec.rate.hz(), 96_000);
    assert_eq!(
        library.version_chosen_for(waiting.lossy.id)?,
        Some(finer.id),
        "the finer copy was passed over"
    );
    assert!(opus.exists());
    Ok(())
}

#[test]
fn a_replacement_rejoining_a_row_whose_want_delivered_the_lossy_song_wants_it_again() -> Result<()>
{
    let tree = Tree::new();
    let Some(opus) = encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes") else {
        return Ok(());
    };
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let album = waiting
        .lossy
        .album_id
        .expect("the song files under an album");

    assert!(library.forget_the_match(album)?);

    let again = paired_as_the_recording(&library, &waiting.lossy, None, RELEASE)?;
    let want = library.want(again)?;

    beside(&database)
        .execute(
            "UPDATE wants SET offered = ?1, tried = 5, misses = 1 WHERE id = ?2",
            rusqlite::params![MediaLocation::local(&opus).to_uri(), want.get() as i64],
        )
        .expect("the want names the lossy delivery");

    assert_eq!(library.settle_replacements()?, Replaced::default());

    let wants = library.wants()?;

    assert_eq!(wants.len(), 1, "{wants:?}");
    assert_eq!(wants[0].id, want);
    assert_eq!(wants[0].replacing, Some(waiting.lossy.id));
    assert_eq!(
        wants[0].offered, None,
        "the lossy delivery answers its replacement"
    );
    assert_eq!((wants[0].tried, wants[0].misses), (None, 0));
    assert!(!wants[0].is_answered());
    assert!(still_pending(&library, &waiting)?);
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
fn a_better_copy_added_later_takes_the_song_over_and_one_gone_by_the_watch_hands_it_back()
-> Result<()> {
    let tree = Tree::new();

    tree.write("flac/06 Echoes.wav", &one_song(16, 44_100));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let best = tree.write("hires/06 Echoes.wav", &one_song(24, 44_100));

    scan(&library, &options(&tree))?;

    let rows = all(&library)?;

    assert_eq!(
        rows.len(),
        1,
        "a copy added later was listed beside the song"
    );
    assert_eq!(rows[0].location.as_path(), Some(best.as_path()));
    assert_eq!(rows[0].alternatives, 1);

    fs::remove_file(&best).expect("the better copy goes");

    assert_eq!(library.forget_the_gone(&[best])?, 1);

    let rows = all(&library)?;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].spec.format.valid_bits(), 16);
    assert_eq!(rows[0].alternatives, 0);
    Ok(())
}

fn filing_into(library: &Library, tree: &Tree) {
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));
}

fn files_named(folder: &Path, extension: &str) -> Vec<PathBuf> {
    every_file_under(folder)
        .into_iter()
        .filter(|path| path.extension() == Some(OsStr::new(extension)))
        .collect()
}

#[cfg(unix)]
fn played_in_its_songs_place(
    library: &Library,
    waiting: &Waiting,
    song: &Path,
    file: &Path,
) -> Result<()> {
    let cuts = library.playlist_cuts(waiting.playlist)?;

    assert_eq!(cuts.len(), 2);
    assert!(
        cuts.iter().all(|cut| cut.location.as_path() == Some(song)),
        "the playlist no longer names its own song: {cuts:?}"
    );

    let copy = library.track_at(file, None)?.expect("the copy is held").id;

    assert_eq!(library.version_chosen_for(waiting.lossy.id)?, Some(copy));
    Ok(())
}

#[test]
fn a_replacement_is_never_asked_of_a_last_resort() -> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;

    filing_into(&library, &tree);

    let tube = Arc::new(Offering::new("tube", Delivering::File(flac)));
    let providers = Arc::new(
        Providers::none().and(Arc::new(LastResort(Arc::clone(&tube))) as Arc<dyn Provider>),
    );

    let summary = library
        .poll(providers, PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert!(
        tube.asked().is_empty(),
        "a last resort was asked for a lossless copy"
    );
    assert_eq!(summary.stats.offered, 0, "{summary:?}");
    assert_eq!(summary.replaced, Replaced::default());
    assert!(opus.exists());
    assert!(still_pending(&library, &waiting)?);
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossy_delivery_for_a_replacement_is_refused_and_the_next_provider_asked() -> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(offered_opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "one/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "two/A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let held = Tree::new();
    let data = Tree::new();
    let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));
    let library = Library::open_with_vault(&tree.path().join("library.db"), vault)?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;
    let one = Arc::new(Offering::new("one", Delivering::File(offered_opus)));
    let two = Arc::new(Offering::new("two", Delivering::File(flac)));

    let summary = library
        .poll(
            asking_in_turn(&[&one, &two]),
            PollOptions::ASKING_EVERY_WANT,
        )?
        .join()?;

    assert_eq!(
        (
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.unkept
        ),
        (2, 1, 1),
        "{summary:?}"
    );
    assert_eq!((one.asked().len(), two.asked().len()), (1, 1));
    assert_eq!(summary.replaced.beside, 1, "{summary:?}");

    let mut kept = Vec::new();

    for object in library.vault_objects()? {
        kept.extend(library.track_at(&object.path, None)?);
    }

    assert_eq!(kept.len(), 1, "the vault holds {kept:?}");

    let kept = kept.remove(0);

    assert!(kept.codec.is_lossless(), "{kept:?}");

    let kept_at = kept
        .location
        .as_path()
        .expect("the vault's row names its object")
        .to_path_buf();

    played_in_its_songs_place(&library, &waiting, &opus, &kept_at)?;

    assert!(opus.exists());
    assert!(!trashed(&data, "A.opus"));
    assert!(library.wants()?.is_empty(), "the want outlived its answer");
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_lossless_delivery_filed_in_the_music_folder_replaces_the_lossy_song_at_the_polls_end()
-> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let data = Tree::new();
    let library = Library::open(&data.path().join("library.db"))?;

    library.trash_into(Some(data.path().to_path_buf()));
    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;

    filing_into(&library, &tree);

    let store = Arc::new(Offering::new("store", Delivering::File(flac)));

    let summary = library
        .poll(store.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(summary.stats.kept, 1, "{summary:?}");
    assert_eq!(summary.replaced.beside, 1, "{summary:?}");

    let filed = files_named(tree.path(), "flac");

    assert_eq!(filed.len(), 1, "{filed:?}");

    played_in_its_songs_place(&library, &waiting, &opus, &filed[0])?;

    assert_eq!(the_row_at(&library, &opus)?.plays, 2);
    assert!(opus.exists());
    assert!(!trashed(&data, "A.opus"));
    Ok(())
}

#[test]
fn a_replacement_with_neither_a_vault_nor_a_music_folder_is_never_asked_of_a_provider() -> Result<()>
{
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(flac)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.flac", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;

    library.deliver_into(None);

    let store = Arc::new(Offering::new("store", Delivering::File(flac)));

    let summary = library
        .poll(store.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert!(
        store.asked().is_empty(),
        "a provider was asked for a copy nowhere could keep"
    );
    assert!(!library.is_a_want_due(PollOptions::ASKING_EVERY_WANT)?);
    assert_eq!(library.next_want_due()?, None);
    assert_eq!(
        (
            summary.stats.asked,
            summary.stats.offered,
            summary.stats.kept,
            summary.stats.unkept
        ),
        (0, 0, 0, 0),
        "{summary:?}"
    );
    assert_eq!(summary.replaced, Replaced::default());
    assert_eq!(library.wants()?[0].offered, None);
    assert!(still_pending(&library, &waiting)?);
    assert!(opus.exists());
    Ok(())
}

#[test]
fn a_lossy_file_filed_for_a_replacement_is_taken_away_and_refused() -> Result<()> {
    let tree = Tree::new();
    let shop = Tree::new();

    let (Some(opus), Some(mp3)) = (
        encoded_as_pink_floyd(&tree, "lossy/A.opus", "Echoes"),
        encoded_as_pink_floyd(&shop, "A.mp3", "Echoes"),
    ) else {
        return Ok(());
    };
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let waiting = waiting_to_be_replaced(&library, &opus, None)?;

    filing_into(&library, &tree);

    let before = every_file_under(tree.path());
    let store = Arc::new(Offering::new("store", Delivering::File(mp3)));

    let summary = library
        .poll(store.registered(), PollOptions::ASKING_EVERY_WANT)?
        .join()?;

    assert_eq!(summary.stats.kept, 0, "{summary:?}");
    assert_eq!(summary.replaced, Replaced::default());
    assert_eq!(
        every_file_under(tree.path()),
        before,
        "a lossy filing was left in the music folder"
    );
    assert!(files_named(tree.path(), "mp3").is_empty());
    assert!(opus.exists());
    assert!(still_pending(&library, &waiting)?);
    Ok(())
}
