use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
#[cfg(unix)]
use std::{
    os::unix::fs::MetadataExt as _,
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

use resonate_core::{AlbumId, Frames, MediaLocation, Resumable, Resumption, ZoneId};
use resonate_library::{
    AlbumQuery, Cut, Error, Favoured, Layout, Library, OrganiseOptions, OrganiseSummary, Refusal,
    Refused, Result, ScanOptions, Sidecar, TrackQuery, Window,
};

use crate::support::{
    ALBUM, ALBUM_ARTIST, ARTIST, MEDDLE_SHEET, RELEASE, TITLE, TRACK, Tree, Wav, all, applied,
    beside, filed_under, keys, landings, mbid, meddle, only_album, options, png, previewed, scan,
    scanned_sheet, the_row_of, words,
};

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
            .text(TITLE, "In the Flesh")
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
                root.join("Pink Floyd/The Wall/1-01 In the Flesh.wav")
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
        library.track_played(&row.location, row.span, Duration::ZERO)?;
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
fn a_sheet_that_cuts_no_audio_track_leaves_its_file_to_the_whole_file_pass() -> Result<()> {
    let tree = Tree::new();

    tree.write("one.wav", &meddle("One of These Days", "1"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let file = filed_under(&tree).join("one.wav");
    let before = library
        .track_at(&file, None)?
        .expect("the file is a track of its own");

    library.favour(Favoured::Track(before.id), true)?;

    tree.write(
        "data.cue",
        b"FILE \"one.wav\" WAVE\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n",
    );
    scan(&library, &options(&tree))?;

    let after = library
        .track_at(&file, None)?
        .expect("a sheet cutting nothing took the file's row away");

    assert_eq!(after.id, before.id);
    assert!(after.favourite.is_some());
    Ok(())
}

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
fn organising_a_second_name_onto_the_filed_copy_of_its_file_keeps_the_filed_rows_history()
-> Result<()> {
    let tree = Tree::new();
    let filed = tree.write("Pink Floyd/Meddle/02 Echoes.wav", &meddle("Echoes", "2"));
    let linked = filed.with_file_name("echoes.wav");

    fs::hard_link(&filed, &linked).expect("a second name for the filed copy");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let filed = root.join("Pink Floyd/Meddle/02 Echoes.wav");
    let linked = root.join("Pink Floyd/Meddle/echoes.wav");

    for _ in 0..2 {
        let counted = library
            .track_played(&MediaLocation::local(&filed), None, Duration::from_secs(2))?
            .expect("a counted play");

        library.listened(counted.listen, Duration::from_secs(2))?;
    }

    let favoured = library.track_at(&filed, None)?.expect("the filed row").id;

    library.favour(Favoured::Track(favoured), true)?;

    let before = library.statistics(Window::Everything)?;

    let summary = applied(&library)?;
    let after = library.statistics(Window::Everything)?;
    let standing = library
        .track_at(&filed, None)?
        .expect("a row at the filed name");

    assert_eq!(landings(&summary), vec![(linked.clone(), filed.clone())]);
    assert!(!linked.exists(), "the second name was left standing");
    assert_eq!(library.track_at(&linked, None)?, None);
    assert_eq!(standing.plays, 2);
    assert!(standing.favourite.is_some(), "the favourite was lost");
    assert_eq!(before.plays, 2);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);
    Ok(())
}

#[test]
fn organising_a_second_name_in_another_folder_lands_it_on_the_filed_name_with_both_rows_history()
-> Result<()> {
    let tree = Tree::new();
    let filed = tree.write("Pink Floyd/Meddle/02 Echoes.wav", &meddle("Echoes", "2"));
    let downloaded = tree.path().join("downloads/echoes.wav");

    fs::create_dir_all(tree.path().join("downloads")).expect("a download client's folder");
    fs::hard_link(&filed, &downloaded).expect("a second name for the filed copy");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let filed = root.join("Pink Floyd/Meddle/02 Echoes.wav");
    let downloaded = root.join("downloads/echoes.wav");

    for (played, times) in [(&filed, 2), (&downloaded, 1)] {
        for _ in 0..times {
            let counted = library
                .track_played(&MediaLocation::local(played), None, Duration::from_secs(2))?
                .expect("a counted play");

            library.listened(counted.listen, Duration::from_secs(2))?;
        }
    }

    let favoured = library.track_at(&filed, None)?.expect("the filed row").id;

    library.favour(Favoured::Track(favoured), true)?;

    let before = library.statistics(Window::Everything)?;

    let summary = applied(&library)?;
    let after = library.statistics(Window::Everything)?;
    let standing = library
        .track_at(&filed, None)?
        .expect("a row at the filed name");
    let held: Vec<PathBuf> = all(&library)?
        .into_iter()
        .filter_map(|track| track.location.as_path().map(Path::to_path_buf))
        .collect();

    assert_eq!(
        landings(&summary),
        vec![(downloaded.clone(), filed.clone())]
    );
    assert!(filed.is_file(), "the filed name was taken away");
    assert!(!downloaded.exists(), "the second name was left standing");
    assert!(
        !root.join("downloads/02 Echoes.wav").exists(),
        "the second name was renamed in its own folder"
    );
    assert_eq!(held, vec![filed.clone()]);
    assert_eq!(standing.plays, 3);
    assert!(standing.favourite.is_some(), "the favourite was lost");
    assert_eq!(before.plays, 3);
    assert_eq!(after.plays, before.plays);
    assert_eq!(after.listened, before.listened);
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

#[cfg(unix)]
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

#[cfg(unix)]
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

    std::os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
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

#[cfg(unix)]
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

    std::os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
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

#[cfg(unix)]
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

    std::os::unix::fs::symlink(&elsewhere, root.join("Pink Floyd"))
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
            .text(TITLE, "In the Flesh")
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
    let flesh = root.join("Pink Floyd/The Wall/1-01 In the Flesh.wav");
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

    library.track_played(&before.location, None, Duration::ZERO)?;

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
fn an_applied_run_is_walked_back_file_for_file_and_walking_it_back_again_files_them_again()
-> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write("loose/echoes.cue", MEDDLE_BY_FILE_SHEET.as_bytes());
    tree.write("loose/days.wav", &meddle("One of These Days", "1"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let stood = root.join("loose/echoes.wav");
    let filed = root.join("Pink Floyd/Meddle/02 Echoes.wav");
    let heard = library
        .track_played(&MediaLocation::local(&stood), None, Duration::ZERO)?
        .expect("the loose file was counted");

    let forward = applied(&library)?;

    assert_eq!(forward.plan.files_moving(), 2);
    assert!(filed.is_file());

    let walked = |apply: bool| -> Result<OrganiseSummary> {
        library
            .organise(OrganiseOptions {
                apply,
                walk_back: true,
                ..OrganiseOptions::default()
            })?
            .join()
    };
    let preview = walked(false)?;

    assert_eq!(preview.plan.files_moving(), 2);
    assert!(filed.is_file(), "a preview of the walk back moved a file");

    let back = walked(true)?;

    assert_eq!(back.stats.moved, 2);
    assert!(stood.is_file(), "the file was not put back where it stood");
    assert!(root.join("loose/days.wav").is_file());
    assert!(!filed.exists());
    assert!(
        !root.join("Pink Floyd").exists(),
        "the folders the run made were left behind"
    );

    let returned = library
        .track_at(&stood, None)?
        .expect("the row followed its file back");

    assert_eq!(returned.id, the_row_of(&heard).id);
    assert_eq!(returned.plays, 1);

    let again = walked(true)?;

    assert_eq!(again.stats.moved, 2);
    assert!(
        filed.is_file(),
        "walking the walk back back did not file it again"
    );
    Ok(())
}

#[test]
fn a_walk_back_leaves_a_sidecar_rather_than_overwrite_a_file_or_refuse_its_track() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write("loose/echoes.lrc", b"[00:01.00]overhead the albatross");
    tree.write("loose/days.wav", &meddle("One of These Days", "1"));
    tree.write("loose/days.txt", b"one of these days");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);

    applied(&library)?;

    let filed = root.join("Pink Floyd/Meddle");

    assert!(filed.join("02 Echoes.lrc").is_file());
    assert!(filed.join("01 One of These Days.txt").is_file());

    fs::create_dir_all(root.join("loose")).expect("the emptied folder again");
    fs::write(root.join("loose/echoes.lrc"), b"written since").expect("a file where one stood");
    fs::remove_file(filed.join("01 One of These Days.txt")).expect("a sidecar deleted");

    let back = library
        .organise(OrganiseOptions {
            apply: true,
            walk_back: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(back.stats.moved, 2, "{:?}", back.plan.refused);
    assert!(root.join("loose/echoes.wav").is_file());
    assert!(
        root.join("loose/days.wav").is_file(),
        "a deleted sidecar refused its track"
    );
    assert_eq!(
        fs::read(root.join("loose/echoes.lrc")).expect("the file there since"),
        b"written since",
        "a sidecar was renamed over a file that appeared where it went"
    );
    Ok(())
}

#[test]
fn a_sidecar_travels_with_the_track_whose_longer_stem_it_carries() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("One of These Days", "1"));
    tree.write("loose/echoes.lrc", b"[00:01.00]overhead the albatross");
    tree.write("loose/echoes.x.wav", &meddle("Echoes", "2"));
    tree.write("loose/echoes.x.lrc", b"[00:01.00]live");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);

    let summary = applied(&library)?;

    let filed = root.join("Pink Floyd/Meddle");

    assert_eq!(summary.stats.moved, 2, "{:?}", summary.plan.refused);
    assert_eq!(
        fs::read(filed.join("01 One of These Days.lrc")).expect("the sidecar of the shorter stem"),
        b"[00:01.00]overhead the albatross"
    );
    assert_eq!(
        fs::read(filed.join("02 Echoes.lrc")).expect("the sidecar of the longer stem"),
        b"[00:01.00]live"
    );
    Ok(())
}

#[test]
fn a_sidecar_whose_new_name_would_outrun_a_component_is_left_and_its_track_still_moves()
-> Result<()> {
    let tree = Tree::new();
    let long = format!("echoes.{}", "x".repeat(248));

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write(&format!("loose/{long}"), b"notes");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);

    let summary = applied(&library)?;

    assert_eq!(summary.stats.moved, 1, "{:?}", summary.plan.refused);
    assert!(root.join("Pink Floyd/Meddle/02 Echoes.wav").is_file());
    assert!(root.join("loose").join(&long).is_file());
    Ok(())
}

#[test]
fn a_walk_back_cut_short_keeps_what_it_did_not_put_back_for_the_next_one() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write("loose/days.wav", &meddle("One of These Days", "1"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let days = root.join("loose/days.wav");
    let echoes = root.join("loose/echoes.wav");

    let forward = applied(&library)?;

    assert_eq!(forward.plan.files_moving(), 2);

    fs::create_dir_all(root.join("loose")).expect("the emptied folder again");
    fs::write(&days, b"in the way").expect("a file where one stood");

    let walked = || -> Result<OrganiseSummary> {
        library
            .organise(OrganiseOptions {
                apply: true,
                walk_back: true,
                ..OrganiseOptions::default()
            })?
            .join()
    };
    let cut_short = walked()?;

    assert_eq!(cut_short.stats.moved, 1);
    assert!(echoes.is_file());
    assert!(library.walks_back()?);

    fs::remove_file(&days).expect("the file in the way goes");

    let rest = walked()?;

    assert_eq!(rest.stats.moved, 1, "the file left behind was forgotten");
    assert!(days.is_file());
    assert!(echoes.is_file(), "a file already put back was moved again");
    assert!(!root.join("Pink Floyd").exists());
    Ok(())
}

#[test]
fn a_path_named_twice_in_a_run_is_planned_once() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let echoes = root.join("loose/echoes.wav");
    let summary = library
        .organise(OrganiseOptions {
            only: vec![echoes.clone(), echoes.clone(), echoes],
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(summary.plan.moves.len(), 1);
    assert!(summary.plan.refused.is_empty());
    Ok(())
}

#[test]
fn a_folders_pictures_follow_its_tracks_only_where_every_track_lands_in_one_folder() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write("loose/days.wav", &meddle("One of These Days", "1"));
    tree.write("loose/cover.jpg", b"picture");
    tree.write("loose/notes.txt", b"words");
    tree.write("split/echoes.wav", &meddle("Echoes", "2"));
    tree.write("split/cover.jpg", b"picture");
    tree.write("split/kept.wav", &meddle("Fearless", "3"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let filed = root.join("Pink Floyd/Meddle");
    let summary = library
        .organise(OrganiseOptions {
            apply: true,
            only: vec![root.join("loose/echoes.wav"), root.join("loose/days.wav")],
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(summary.stats.moved, 2);
    assert_eq!(
        fs::read(filed.join("cover.jpg")).expect("the picture followed"),
        b"picture"
    );
    assert!(!root.join("loose/cover.jpg").exists());
    assert!(root.join("loose/notes.txt").is_file());

    let back = library
        .organise(OrganiseOptions {
            apply: true,
            walk_back: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(back.stats.moved, 2);
    assert!(root.join("loose/cover.jpg").is_file());
    assert!(!filed.exists());

    let split = library
        .organise(OrganiseOptions {
            apply: true,
            only: vec![root.join("split/echoes.wav")],
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(split.stats.moved, 1);
    assert!(root.join("split/cover.jpg").is_file());
    Ok(())
}

#[test]
fn a_run_given_files_files_those_alone_and_leaves_the_rest_where_they_stand() -> Result<()> {
    let tree = Tree::new();

    tree.write("loose/echoes.wav", &meddle("Echoes", "2"));
    tree.write("loose/echoes.cue", MEDDLE_BY_FILE_SHEET.as_bytes());
    tree.write("loose/days.wav", &meddle("One of These Days", "1"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let echoes = root.join("loose/echoes.wav");
    let days = root.join("loose/days.wav");
    let summary = library
        .organise(OrganiseOptions {
            apply: true,
            only: vec![echoes.clone()],
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert_eq!(summary.plan.files_moving(), 1);
    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.stats.unchanged, 0);
    assert!(root.join("Pink Floyd/Meddle/02 Echoes.wav").is_file());
    assert!(root.join("Pink Floyd/Meddle/02 Echoes.cue").is_file());
    assert!(!echoes.exists());
    assert!(days.is_file(), "a file the run was not given was moved");
    assert!(library.track_at(&days, None)?.is_some());
    Ok(())
}

#[test]
fn two_files_filed_under_each_others_names_trade_places_and_keep_their_plays() -> Result<()> {
    let tree = Tree::new();
    let echoes = meddle("Echoes", "2");
    let days = meddle("One of These Days", "1");

    tree.write("Pink Floyd/Meddle/01 One of These Days.wav", &echoes);
    tree.write("Pink Floyd/Meddle/02 Echoes.wav", &days);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let root = filed_under(&tree);
    let first = root.join("Pink Floyd/Meddle/01 One of These Days.wav");
    let second = root.join("Pink Floyd/Meddle/02 Echoes.wav");
    let heard = library
        .track_played(&MediaLocation::local(&first), None, Duration::ZERO)?
        .expect("the file holding Echoes was counted");

    let summary = applied(&library)?;

    assert!(
        summary.plan.refused.is_empty(),
        "{:?}",
        summary.plan.refused
    );
    assert_eq!(
        fs::read(&second).expect("Echoes where it was filed"),
        echoes
    );
    assert_eq!(
        fs::read(&first).expect("the other where it was filed"),
        days
    );

    let moved = library
        .track_at(&second, None)?
        .expect("Echoes is named where it now stands");

    assert_eq!(moved.id, the_row_of(&heard).id);
    assert_eq!(moved.plays, 1);
    assert_eq!(
        library.track_at(&first, None)?.map(|track| track.plays),
        Some(0)
    );

    let left: Vec<_> = fs::read_dir(root.join("Pink Floyd/Meddle"))
        .expect("the album's folder")
        .flatten()
        .map(|entry| entry.file_name())
        .collect();

    assert_eq!(left.len(), 2, "a parked name was left behind: {left:?}");
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

    library.keep_lyrics(&stood, None, Some(&words("and no one sings me lullabies")))?;
    library.keep_resumption(
        ZoneId::FIRST,
        &Resumption {
            rows: vec![Resumable {
                location: stood.clone(),
                span: None,
                held: None,
            }],
            order: vec![0],
            row: 0,
            at: Frames(4_410),
            shuffle: false,
            next: None,
        },
    )?;

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

    assert_eq!(kept.sung, Some(words("and no one sings me lullabies")));
    assert_eq!(library.kept_lyrics(&stood, None)?, None);

    let resumed = library
        .resumption()?
        .expect("the queue kept for the next run is still there");

    assert_eq!(resumed.rows.len(), 1);
    assert_eq!(resumed.rows[0].location, landed);
    assert_eq!(
        resumed.rows[0].held,
        library
            .track_at(landed.as_path().expect("a local file"), None)?
            .map(|track| track.id),
        "the resumed row does not name the library row it is"
    );
    assert!(resumed.rows[0].held.is_some());
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

#[cfg(unix)]
struct AnotherVolume(PathBuf);

#[cfg(unix)]
impl AnotherVolume {
    fn beside(tree: &Tree) -> Option<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let folder = Path::new("/dev/shm").join(format!(
            "resonate-organise-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));

        fs::create_dir_all(&folder).ok()?;

        let volume = Self(folder);
        let apart = fs::metadata(&volume.0).map(|held| held.dev()).ok()
            != fs::metadata(tree.path()).map(|held| held.dev()).ok();

        if !apart {
            eprintln!("skipping: the shared memory folder is on the fixture tree's volume");
            return None;
        }
        Some(volume)
    }
}

#[cfg(unix)]
impl Drop for AnotherVolume {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
const LONG_AGO: Duration = Duration::from_secs(60 * 60 * 24 * 400);

#[cfg(unix)]
fn ripped_on_another_volume(
    tree: &Tree,
    far: &AnotherVolume,
) -> Result<(Library, PathBuf, PathBuf)> {
    let rip = far.0.join("1.wav");

    fs::write(&rip, meddle("One of These Days", "1")).expect("the second volume is writable");
    fs::File::options()
        .write(true)
        .open(&rip)
        .and_then(|file| file.set_modified(SystemTime::now() - LONG_AGO))
        .expect("the rip takes an old time");
    std::os::unix::fs::symlink(&far.0, tree.path().join("rips"))
        .expect("the fixture tree takes a link");

    let library = Library::open(&tree.path().join("library.db"))?;

    scan(
        &library,
        &ScanOptions {
            follow_symlinks: true,
            ..options(tree)
        },
    )?;

    let source = filed_under(tree).join("rips/1.wav");
    let landing = filed_under(tree).join("Pink Floyd/Meddle/01 One of These Days.wav");

    assert!(library.track_at(&source, None)?.is_some());
    Ok((library, source, landing))
}

#[cfg(unix)]
fn staged_beside(folder: &Path) -> Vec<PathBuf> {
    fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok().map(|entry| entry.path()))
                .filter(|path| path.to_string_lossy().ends_with(".resonate-staging"))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(unix)]
#[test]
fn a_file_organised_onto_another_volume_lands_whole_before_the_catalog_follows_and_its_source_goes_after()
-> Result<()> {
    let tree = Tree::new();
    let Some(far) = AnotherVolume::beside(&tree) else {
        return Ok(());
    };

    let (library, source, landing) = ripped_on_another_volume(&tree, &far)?;
    let bytes = fs::read(&source).expect("the rip");
    let stood = fs::metadata(&source)
        .and_then(|held| held.modified())
        .expect("a stamped rip");

    let summary = applied(&library)?;

    let landed = fs::metadata(&landing).expect("the copy landed");

    assert_eq!(summary.stats.moved, 1);
    assert_eq!(summary.stats.failed, 0);
    assert_eq!(fs::read(&landing).expect("the copy"), bytes);
    assert_eq!(landed.modified().expect("a stamped copy"), stood);
    assert_ne!(
        landed.dev(),
        fs::metadata(&far.0).expect("the volume").dev()
    );
    assert!(
        !far.0.join("1.wav").exists(),
        "the source outlived its copy"
    );
    assert!(library.track_at(&landing, None)?.is_some());
    assert!(library.track_at(&source, None)?.is_none());
    assert_eq!(
        staged_beside(landing.parent().expect("a folder")),
        Vec::<PathBuf>::new()
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_copy_a_killed_run_landed_on_another_volume_is_taken_as_copied_and_one_that_differs_is_not()
-> Result<()> {
    let tree = Tree::new();
    let Some(far) = AnotherVolume::beside(&tree) else {
        return Ok(());
    };

    let (library, source, landing) = ripped_on_another_volume(&tree, &far)?;
    let bytes = fs::read(&source).expect("the rip");
    let stood = fs::metadata(&source)
        .and_then(|held| held.modified())
        .expect("a stamped rip");
    let mut spoiled = bytes.clone();

    *spoiled.last_mut().expect("a rip holds bytes") ^= 1;
    fs::create_dir_all(landing.parent().expect("a folder")).expect("the landing folder");

    let landed_by_the_killed_run = |held: &[u8]| {
        fs::write(&landing, held).expect("the copy the killed run landed");
        fs::File::options()
            .write(true)
            .open(&landing)
            .and_then(|file| file.set_modified(stood))
            .expect("the copy carries the rip's time");
    };

    landed_by_the_killed_run(&spoiled);

    let collided = applied(&library)?;
    let source_kept = fs::read(&source).expect("the rip stays");
    let copy_kept = fs::read(&landing).expect("the differing copy stays");

    landed_by_the_killed_run(&bytes);

    let resumed = applied(&library)?;

    assert_eq!(collided.stats.moved, 0);
    assert_eq!(collided.stats.collided, 1);
    assert_eq!(source_kept, bytes);
    assert_eq!(copy_kept, spoiled);
    assert_eq!(resumed.stats.moved, 1);
    assert_eq!(resumed.stats.failed, 0);
    assert!(
        resumed.plan.refused.is_empty(),
        "{:?}",
        resumed.plan.refused
    );
    assert_eq!(fs::read(&landing).expect("the copy"), bytes);
    assert!(
        !far.0.join("1.wav").exists(),
        "the source outlived its copy"
    );
    assert!(library.track_at(&landing, None)?.is_some());
    assert!(library.track_at(&source, None)?.is_none());
    Ok(())
}

#[cfg(unix)]
#[test]
fn a_catalog_that_will_not_follow_a_copy_onto_another_volume_keeps_the_source_and_takes_the_copy()
-> Result<()> {
    let tree = Tree::new();
    let Some(far) = AnotherVolume::beside(&tree) else {
        return Ok(());
    };

    let (library, source, landing) = ripped_on_another_volume(&tree, &far)?;
    let bytes = fs::read(&source).expect("the rip");

    beside(&tree.path().join("library.db"))
        .execute_batch(
            "CREATE TRIGGER moves_refused BEFORE UPDATE OF path ON tracks
             BEGIN SELECT RAISE(ABORT, 'the catalog refuses the move'); END",
        )
        .expect("the catalog takes a trigger");

    let summary = applied(&library)?;

    assert_eq!(summary.stats.moved, 0);
    assert_eq!(summary.stats.failed, 1);
    assert_eq!(fs::read(&source).expect("the rip stays"), bytes);
    assert!(
        !landing.exists(),
        "the copy the catalog never named was left"
    );
    assert!(library.track_at(&source, None)?.is_some());
    assert!(library.track_at(&landing, None)?.is_none());
    assert_eq!(
        staged_beside(landing.parent().expect("a folder")),
        Vec::<PathBuf>::new()
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

#[test]
fn a_whole_rescan_after_two_albums_land_in_one_folder_keeps_each_the_album_it_was() -> Result<()> {
    let tree = Tree::new();

    tree.write("A/1.wav", &hunted(SILVER, "Marcin Przybylowicz", "1"));
    tree.write("B/2.wav", &hunted(FIELDS, "Marcin Przybylowicz", "2"));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let filed = |library: &Library| -> Result<Vec<(String, Option<AlbumId>)>> {
        Ok(all(library)?
            .into_iter()
            .map(|track| (track.title, track.album_id))
            .collect())
    };
    let before = filed(&library)?;

    assert_ne!(
        before[0].1, before[1].1,
        "two folders were expected to be two albums"
    );

    applied(&library)?;
    scan(
        &library,
        &ScanOptions {
            incremental: false,
            ..options(&tree)
        },
    )?;

    assert_eq!(
        filed(&library)?,
        before,
        "a whole rescan filed a track under the album its new folder names"
    );
    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}
