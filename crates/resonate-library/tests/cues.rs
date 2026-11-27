use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use resonate_core::{FrameSpan, Frames, MediaLocation};
use resonate_library::{
    AlbumQuery, Cut, Favoured, Library, Refusal, Refused, Result, ScanOptions, TrackQuery,
};

use crate::support::{
    A_DAY, LOW_TIDE_SHEET, TWO_DISC_SHEET, Tree, Wav, ffmpeg, filed_under, options, previewed,
    scan, scanned_sheet, scanned_two_discs, the_row_of,
};

const REQUIRED_BY: &str = "RESONATE_LIBRARY_FFMPEG_TESTS";

#[test]
fn a_file_a_cue_sheet_cuts_is_scanned_as_the_tracks_the_sheet_names() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(rows.len(), 3, "the sheet's tracks did not become rows");
    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Small Mercies", "A Quilt of Winds", "Lantern"]
    );

    let held = MediaLocation::local(tree.path().join("Tideline.wav"));

    for row in &rows {
        assert_eq!(row.location, held, "every row names the one file");
        assert_eq!(row.artist.as_deref(), Some("Example Band"));
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

#[test]
fn a_cue_track_whose_index_moves_keeps_its_row_and_what_was_heard_of_it() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let held = MediaLocation::local(tree.path().join("Tideline.wav"));
    let slow_water = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Lantern")
        .expect("the third cut");

    library.track_played(&held, slow_water.span, Duration::ZERO)?;

    let playlist = library.create_playlist("Side Two")?;

    library.add_to_playlist(playlist, &[Cut::of(&slow_water)])?;

    tree.write(
        "Tideline.cue",
        LOW_TIDE_SHEET
            .replace("INDEX 01 00:00:40", "INDEX 01 00:00:45")
            .as_bytes(),
    );
    scan(&library, &options(&tree))?;

    let moved = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Lantern")
        .expect("the third cut, still there");

    assert_eq!(moved.id, slow_water.id, "the cut became a row of its own");
    assert_eq!(moved.plays, 1);
    assert_eq!(
        moved.span.map(FrameSpan::start),
        Some(Frames(44_100 / 75 * 45))
    );

    let entries = library.playlist_entries(playlist, None)?;

    assert!(
        entries[0].track.is_some(),
        "the playlist row lost the cut it named"
    );
    assert_eq!(
        entries[0].cut,
        Cut::of(&moved),
        "the playlist row kept the length the cut had when it was added"
    );
    Ok(())
}

const LOW_TIDE_TWO_TRACK_SHEET: &str = r#"PERFORMER "Example Band"
TITLE "Tideline (the other sheet)"
FILE "Tideline.wav" WAVE
  TRACK 01 AUDIO
    TITLE "Side One"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Side Two"
    INDEX 01 00:00:30
"#;

#[test]
fn two_sheets_in_one_folder_naming_one_file_cut_it_once_by_the_first_in_name_order() -> Result<()> {
    let tree = Tree::new();

    tree.write("Tideline.wav", &Wav::new().frames(44_100).build());
    tree.write("A.cue", LOW_TIDE_SHEET.as_bytes());
    tree.write("B.cue", LOW_TIDE_TWO_TRACK_SHEET.as_bytes());

    let library = Library::open_in_memory()?;

    for _ in 0..3 {
        scan(&library, &options(&tree))?;

        let titles: Vec<String> = library
            .tracks(&TrackQuery::default())?
            .into_iter()
            .map(|row| row.title)
            .collect();

        assert_eq!(
            titles,
            ["Small Mercies", "A Quilt of Winds", "Lantern"],
            "the file was cut by both sheets or by whichever the directory listed first"
        );
    }
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
fn a_sheet_naming_files_in_folders_below_it_cuts_them_there() -> Result<()> {
    let (tree, library) = scanned_two_discs();
    let rows = library.tracks(&TrackQuery::default())?;

    let mut held: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                row.location.as_path().map(Path::to_path_buf),
                row.span.map(FrameSpan::start),
                row.title.clone(),
            )
        })
        .collect();

    held.sort();

    assert_eq!(
        held,
        vec![
            (
                Some(tree.path().join("CD1/lighthouse.wav")),
                Some(Frames::ZERO),
                "Open the Gate?".to_owned()
            ),
            (
                Some(tree.path().join("CD1/lighthouse.wav")),
                Some(Frames(44_100 / 75 * 40)),
                "The Narrow Bridge".to_owned()
            ),
            (
                Some(tree.path().join("CD2/lighthouse.wav")),
                Some(Frames::ZERO),
                "Another Door".to_owned()
            ),
        ],
        "the files below the sheet were not cut as it says"
    );

    let again = scan(&library, &options(&tree))?;

    assert_eq!(
        (again.added, again.updated, again.removed),
        (0, 0, 0),
        "an unchanged rescan wrote rows"
    );
    assert_eq!(library.tracks(&TrackQuery::default())?.len(), 3);
    Ok(())
}

#[test]
fn a_sheet_read_as_empty_keeps_the_rows_it_cut_in_folders_below_it() -> Result<()> {
    let (tree, library) = scanned_two_discs();
    let before: Vec<_> = library
        .tracks(&TrackQuery::default())?
        .iter()
        .map(|row| row.id)
        .collect();

    tree.write("The Lighthouse.cue", b"");

    let stats = scan(&library, &options(&tree))?;
    let during: Vec<_> = library
        .tracks(&TrackQuery::default())?
        .iter()
        .map(|row| row.id)
        .collect();

    assert_eq!(
        stats.removed, 0,
        "rows were pruned under a sheet that could not be read"
    );
    assert_eq!(
        during, before,
        "a sheet read as empty cost the files below it their cuts"
    );

    tree.write("The Lighthouse.cue", TWO_DISC_SHEET.as_bytes());
    scan(&library, &options(&tree))?;

    let after: Vec<_> = library
        .tracks(&TrackQuery::default())?
        .iter()
        .map(|row| row.id)
        .collect();

    assert_eq!(after, before, "the cuts came back as new rows");
    Ok(())
}

#[test]
fn a_file_a_sheet_above_it_cuts_is_left_where_it_stands() -> Result<()> {
    let (tree, library) = scanned_two_discs();

    let summary = previewed(&library)?;
    let root = filed_under(&tree);

    assert!(summary.plan.moves.is_empty(), "{:?}", summary.plan.moves);

    let mut refused: Vec<Refused> = summary.plan.refused.clone();

    refused.sort_by(|one, other| one.from.cmp(&other.from));

    assert_eq!(
        refused,
        ["CD1/lighthouse.wav", "CD2/lighthouse.wav"]
            .iter()
            .map(|file| Refused {
                from: root.join(file),
                refusal: Refusal::NamedFromAbove {
                    sheet: root.join("The Lighthouse.cue"),
                },
            })
            .collect::<Vec<_>>()
    );
    Ok(())
}

#[test]
fn a_sheet_naming_its_file_by_another_case_extension_or_folder_still_cuts_it() -> Result<()> {
    let tree = Tree::new();

    tree.write("album.wav", &Wav::new().frames(88_200).build());
    tree.write("other.wav", &Wav::new().frames(44_100).build());
    tree.write(
        "Album.cue",
        b"FILE \"C:\\Rips\\ALBUM.FLAC\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:01:00\n",
    );
    tree.write(
        "Other.wav.CUE",
        b"FILE \"OTHER.WAV\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Other\"\n    INDEX 01 00:00:00\n",
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let rows = library.tracks(&TrackQuery::default())?;

    let mut held: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                row.location.as_path().map(Path::to_path_buf),
                row.span.map(FrameSpan::start),
            )
        })
        .collect();

    held.sort();

    assert_eq!(
        held,
        vec![
            (Some(tree.path().join("album.wav")), Some(Frames::ZERO)),
            (Some(tree.path().join("album.wav")), Some(Frames(44_100))),
            (Some(tree.path().join("other.wav")), Some(Frames::ZERO)),
        ]
    );
    assert!(rows.iter().any(|row| row.title == "Other"));
    Ok(())
}

#[test]
fn a_play_counted_against_one_cue_row_is_not_counted_against_its_neighbours() -> Result<()> {
    let (_, library) = scanned_sheet();
    let rows = library.tracks(&TrackQuery::default())?;
    let second = &rows[1];

    let counted = library
        .track_played(&second.location, second.span, Duration::ZERO)?
        .expect("the row that was heard");

    assert_eq!(the_row_of(&counted).plays, 1);
    assert_eq!(the_row_of(&counted).id, second.id);

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

    library.track_played(&second.location, second.span, Duration::ZERO)?;
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

    fs::remove_file(tree.path().join("Tideline.cue")).expect("the sheet goes away");

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

#[test]
fn a_sheet_read_as_empty_keeps_the_rows_it_cut_until_it_reads_again() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let held = MediaLocation::local(tree.path().join("Tideline.wav"));
    let slow_water = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|row| row.title == "Lantern")
        .expect("the third cut");

    library.track_played(&held, slow_water.span, Duration::ZERO)?;

    tree.write("Tideline.cue", b"");

    let stats = scan(&library, &options(&tree))?;
    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.len(),
        3,
        "a sheet read as empty cost the file its cuts"
    );
    assert_eq!(
        stats.removed, 0,
        "rows were pruned under a sheet that could not be read"
    );

    tree.write("Tideline.cue", LOW_TIDE_SHEET.as_bytes());
    scan(&library, &options(&tree))?;

    let again = library.tracks(&TrackQuery::default())?;

    assert_eq!(again.len(), 3);
    assert_eq!(
        again.iter().map(|row| row.id).collect::<Vec<_>>(),
        rows.iter().map(|row| row.id).collect::<Vec<_>>(),
        "the cuts came back as new rows"
    );
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

    tree.write("Tideline.wav", &Wav::new().frames(44_100).build());

    let sheet = tree.write("Tideline.cue", LOW_TIDE_SHEET.as_bytes());

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
        "Tideline.cue",
        LOW_TIDE_SHEET.replace("Lantern", "Marlow").as_bytes(),
    );
    scan(&library, &options(&tree))?;

    assert!(
        titles(&library)?.contains(&"Marlow".to_owned()),
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
        .find(|track| track.title == "Lantern")
        .expect("the sheet's third track was scanned");
    let span = cut.span.expect("a cue row is a cut of its file");

    let id = library.create_playlist("Tideline")?;

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
        Some("Lantern"),
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

    let id = library.create_playlist("Tideline")?;

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
    let id = library.create_playlist("Tideline")?;

    library.add_to_playlist(id, &cuts)?;

    let slow_water = rows
        .iter()
        .find(|track| track.title == "Lantern")
        .expect("a row titled Lantern");
    let shown = library.playlist_entries(id, Some("lantern"))?;

    assert_eq!(
        shown.len(),
        1,
        "a search for one cut matched every cut of the file"
    );
    assert_eq!(shown[0].cut, Cut::of(slow_water));

    assert_eq!(library.remove_matching(id, "lantern")?, 1);

    let left = library.playlist_entries(id, None)?;

    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|entry| entry.cut != Cut::of(slow_water)));
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

    let id = library.create_playlist("Tideline")?;

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

const EMBEDDED_SHEET: &str = r#"PERFORMER "Example Band"
TITLE "Tideline"
FILE "Tideline.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Small Mercies"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Quilt of Winds"
    INDEX 01 00:00:50
  TRACK 03 AUDIO
    TITLE "Lantern"
    INDEX 01 00:01:25
"#;

const SHEET_BESIDE_IT: &str = r#"PERFORMER "Example Band"
TITLE "Tideline"
FILE "Tideline.flac" WAVE
  TRACK 01 AUDIO
    TITLE "Paper Boats"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "Harbour Lights"
    INDEX 01 00:01:00
"#;

fn flac_embedding(tree: &Tree, name: &str, sheet: &str) -> Option<PathBuf> {
    if !ffmpeg() {
        passed_over!("no ffmpeg to build a {name} carrying a cue sheet");
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
        passed_over!("ffmpeg would not encode {name}");
        return None;
    }
    Some(target)
}

#[test]
fn a_file_carrying_its_own_cue_sheet_is_scanned_as_the_tracks_that_sheet_names() -> Result<()> {
    let tree = Tree::new();
    let Some(path) = flac_embedding(&tree, "Tideline.flac", EMBEDDED_SHEET) else {
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
        vec!["Small Mercies", "A Quilt of Winds", "Lantern"]
    );

    let held = MediaLocation::local(&path);

    for row in &rows {
        assert_eq!(row.location, held, "every row names the one file");
        assert_eq!(row.artist.as_deref(), Some("Example Band"));
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
fn a_book_carrying_chapters_is_scanned_as_a_row_a_chapter() -> Result<()> {
    if !ffmpeg() {
        passed_over!("no ffmpeg to build a chaptered book");
        return Ok(());
    }

    let tree = Tree::new();
    let held = Tree::new();
    let source = held.write("tone.wav", &Wav::new().frames(88_200).build());
    let chapters = held.write(
        "chapters.txt",
        b";FFMETADATA1\ntitle=The Hobbit\nartist=J. R. R. Tolkien\n\
[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1200\ntitle=An Unexpected Party\n\
[CHAPTER]\nTIMEBASE=1/1000\nSTART=1200\nEND=2000\ntitle=Roast Mutton\n",
    );
    let path = tree.path().join("The Hobbit.m4b");

    let encoded = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .arg("-i")
        .arg(&chapters)
        .args([
            "-map",
            "0:a",
            "-map_metadata",
            "1",
            "-map_chapters",
            "1",
            "-c:a",
            "aac",
        ])
        .arg(&path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) {
        passed_over!("ffmpeg would not write a chaptered book");
        return Ok(());
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.iter()
            .map(|row| (row.title.as_str(), row.span.map(FrameSpan::start)))
            .collect::<Vec<_>>(),
        vec![
            ("An Unexpected Party", Some(Frames::ZERO)),
            ("Roast Mutton", Some(Frames(52_920))),
        ],
        "the book was not cut where its chapters start"
    );

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["The Hobbit"]
    );
    Ok(())
}

#[test]
fn a_sheet_beside_the_file_still_cuts_it_where_the_file_embeds_one_too() -> Result<()> {
    let tree = Tree::new();

    if flac_embedding(&tree, "Tideline.flac", EMBEDDED_SHEET).is_none() {
        return Ok(());
    }
    tree.write("Tideline.cue", SHEET_BESIDE_IT.as_bytes());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let rows = library.tracks(&TrackQuery::default())?;

    assert_eq!(
        rows.iter()
            .map(|row| row.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Paper Boats", "Harbour Lights"],
        "the sheet the file embeds outranked the one beside it"
    );
    Ok(())
}

#[test]
fn a_rescan_of_a_file_that_embeds_its_own_sheet_changes_nothing() -> Result<()> {
    let tree = Tree::new();

    if flac_embedding(&tree, "Tideline.flac", EMBEDDED_SHEET).is_none() {
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
