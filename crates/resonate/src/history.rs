use std::{
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{file_facts, naming, writer::Held};
use resonate_library::{
    BroughtIn, KeptListens, Library, ListeningService, bring_in_file, bring_in_history,
    history_lines,
};

use crate::{
    cli::HistoryArgs,
    config::Config,
    error::{Error, Result},
    online,
    vocabulary::Meaning as _,
    words,
};

const STAGED_SUFFIX: &str = "staged";

static STAGED: AtomicU64 = AtomicU64::new(0);

pub(crate) fn history(library: &Library, config: &Config, asked: &HistoryArgs) -> Result<()> {
    if let Some(file) = &asked.export {
        return exported(library, file);
    }
    if let Some(file) = &asked.import_file {
        return brought_in_from_file(library, file);
    }

    let Some(service) = asked.import else {
        return Ok(());
    };
    brought_in(library, config, service.meaning(), asked.user.as_deref())
}

fn exported(kept: &dyn KeptListens, file: &Path) -> Result<()> {
    let target = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());

    let listens = match fs::metadata(&target) {
        Ok(standing) if !standing.is_file() => written_straight(kept, &target)?,
        _ => staged_and_placed(kept, &target)?,
    };

    said!(
        "{}",
        words::history::exported(listens, file.display().to_string())
    );
    Ok(())
}

fn staged_and_placed(kept: &dyn KeptListens, target: &Path) -> Result<u64> {
    let staged = staged_beside(target);

    let placed = written_and_placed(kept, target, &staged);

    if placed.is_err() {
        let _ = fs::remove_file(&staged);
    }
    placed
}

fn written_straight(kept: &dyn KeptListens, target: &Path) -> Result<u64> {
    let failed = |source| Error::WriteHistory {
        path: target.to_path_buf(),
        source,
    };
    let opened = OpenOptions::new()
        .write(true)
        .open(target)
        .map_err(failed)?;

    every_line_into(kept, &opened, &failed)
}

fn every_line_into(
    kept: &dyn KeptListens,
    file: &fs::File,
    failed: &dyn Fn(io::Error) -> Error,
) -> Result<u64> {
    let mut written = BufWriter::new(file);

    let mut listens = 0_u64;

    for line in history_lines(kept) {
        writeln!(written, "{}", line?).map_err(failed)?;
        listens += 1;
    }

    written.flush().map_err(failed)?;
    Ok(listens)
}

fn written_and_placed(kept: &dyn KeptListens, target: &Path, staged: &Path) -> Result<u64> {
    let staging = |source| Error::WriteHistory {
        path: staged.to_path_buf(),
        source,
    };
    let held = Held::made(staged).map_err(staging)?;

    let listens = every_line_into(kept, held.file(), &staging)?;

    held.file().sync_all().map_err(staging)?;
    file_facts::replace(staged, target)
        .map_err(|source| Error::WriteHistory {
            path: target.to_path_buf(),
            source,
        })?
        .noted(target);
    drop(held);
    Ok(listens)
}

fn staged_beside(target: &Path) -> PathBuf {
    let stamp = format!(
        ".{}-{}.{STAGED_SUFFIX}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    );

    target.with_file_name(naming::named_within(
        ".",
        target.file_name().unwrap_or_default(),
        stamp,
    ))
}

fn brought_in(
    library: &Library,
    config: &Config,
    service: ListeningService,
    user: Option<&str>,
) -> Result<()> {
    let history = online::history(config, service, user)?;

    let brought = bring_in_history(library, history.as_ref(), &mut said_so_far)?;

    said!(
        "{}",
        words::history::brought_in(
            service.name(),
            brought.credited,
            brought.held_already,
            brought.unmatched
        )
    );
    Ok(())
}

fn brought_in_from_file(library: &Library, file: &Path) -> Result<()> {
    let brought = bring_in_file(library, file, &mut said_so_far)?;

    said!(
        "{}",
        words::history::brought_in_from_file(
            file.display().to_string(),
            brought.credited,
            brought.held_already,
            brought.unmatched
        )
    );
    Ok(())
}

fn said_so_far(brought: BroughtIn) {
    said!(
        "{}",
        words::history::brought_so_far(brought.credited, brought.held_already, brought.unmatched)
    );
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        time::{Duration, UNIX_EPOCH},
    };

    use clap::Parser as _;
    use resonate_core::Recording;
    use resonate_library::{
        Billed, HeardElsewhere, HeardOnce, HistoryFrom, HistoryRead, Library, line_of,
    };

    use super::*;
    use crate::{
        cli::{Cli, Sub},
        fixtures::{ALBUM, ARTIST, TITLE, parsed, scanned, scratch, tagged_wave},
    };

    const SONGS: [(&str, &str, &str); 2] =
        [("Echoes", "Pink Floyd", "Meddle"), ("Say", "Low", "Words")];
    const MOMENTS: [u64; 4] = [1_700_000_000, 1_700_100_000, 1_700_200_000, 1_700_300_000];

    fn catalog_of_songs(name: &str) -> (PathBuf, Library) {
        let root = scratch(name);

        for (title, artist, album) in SONGS {
            fs::write(
                root.join(format!("{title}.wav")),
                tagged_wave(&[(TITLE, title), (ARTIST, artist), (ALBUM, album)]),
            )
            .expect("a song");
        }

        let library = scanned(&root);
        (root, library)
    }

    fn heard_at(song: usize, second: u64) -> HeardElsewhere {
        let (title, artist, album) = SONGS[song];
        HeardElsewhere {
            at: UNIX_EPOCH + Duration::from_secs(second),
            recording: Recording {
                mbid: None,
                title: title.to_owned(),
                artist: artist.to_owned(),
                album: album.to_owned(),
                length: None,
            },
        }
    }

    fn moments_heard(library: &Library) -> Vec<(u64, u64)> {
        let mut moments = library
            .history_from(HistoryFrom::START, 100)
            .expect("the history")
            .heard
            .iter()
            .map(|heard| {
                (
                    heard
                        .began
                        .duration_since(UNIX_EPOCH)
                        .expect("after the epoch")
                        .as_secs(),
                    heard.heard.as_secs(),
                )
            })
            .collect::<Vec<_>>();

        moments.sort_unstable();
        moments
    }

    fn history_args(arguments: &[&str]) -> HistoryArgs {
        let cli = parsed(&[&["resonate", "history"], arguments].concat());
        match cli.command {
            Some(Sub::History(asked)) => asked,
            _ => panic!("not a history command"),
        }
    }

    #[test]
    fn a_history_exported_to_a_file_is_imported_into_a_fresh_catalog_and_a_second_import_adds_nothing()
     {
        let (source_root, source) = catalog_of_songs("history-source");
        let (target_root, target) = catalog_of_songs("history-target");
        let listens = [
            heard_at(0, MOMENTS[0]),
            heard_at(0, MOMENTS[1]),
            heard_at(1, MOMENTS[2]),
            heard_at(1, MOMENTS[3]),
        ];

        source
            .bring_in(ListeningService::LastFm, &listens)
            .expect("listens to export");

        let file = source_root.join("history.jsonl");

        exported(&source, &file).expect("exported");
        brought_in_from_file(&target, &file).expect("imported");

        let first = moments_heard(&target);

        brought_in_from_file(&target, &file).expect("imported again");

        assert_eq!(first.len(), 4);
        assert_eq!(
            first.iter().map(|(began, _)| *began).collect::<Vec<_>>(),
            MOMENTS
        );
        assert_eq!(moments_heard(&target), first);
        assert_eq!(
            target
                .history_from(HistoryFrom::START, 100)
                .expect("the history")
                .heard
                .iter()
                .filter(|heard| heard.elsewhere)
                .count(),
            4
        );

        fs::remove_dir_all(source_root).expect("cleaned");
        fs::remove_dir_all(target_root).expect("cleaned");
    }

    struct FailingPartWay;

    impl KeptListens for FailingPartWay {
        fn history_from(
            &self,
            from: HistoryFrom,
            _: usize,
        ) -> resonate_library::Result<HistoryRead> {
            match from {
                HistoryFrom::START => Ok(HistoryRead {
                    heard: vec![HeardOnce {
                        began: UNIX_EPOCH + Duration::from_secs(MOMENTS[0]),
                        heard: Duration::from_secs(1),
                        elsewhere: false,
                        billed: None,
                        location: "file:///heard.wav".to_owned(),
                    }],
                    next: Some(HistoryFrom::Held(1)),
                }),
                _ => Err(resonate_library::Error::NoVault),
            }
        }

        fn bring_in(
            &self,
            _: ListeningService,
            _: &[HeardElsewhere],
        ) -> resonate_library::Result<BroughtIn> {
            Ok(BroughtIn::default())
        }
    }

    #[test]
    fn an_export_failing_part_way_leaves_the_file_it_would_replace_as_it_was_and_no_stage() {
        let root = scratch("history-export-failing");
        let file = root.join("history.jsonl");
        let before = b"what an earlier export wrote\n";

        fs::write(&file, before).expect("an earlier export");

        let failed = exported(&FailingPartWay, &file);

        assert!(matches!(
            failed,
            Err(Error::Library(resonate_library::Error::NoVault))
        ));
        assert_eq!(fs::read(&file).expect("the earlier export"), before);
        assert_eq!(
            fs::read_dir(&root).expect("the folder").count(),
            1,
            "a stage was left beside the export"
        );

        fs::remove_dir_all(root).expect("cleaned");
    }

    #[test]
    fn an_export_replaces_the_file_it_names_whole_and_leaves_no_stage() {
        let (root, library) = catalog_of_songs("history-export-replaces");
        let file = root.join("history.jsonl");

        fs::write(
            &file,
            "an earlier export that ran far longer than this one\n".repeat(64),
        )
        .expect("an earlier export");
        library
            .bring_in(ListeningService::LastFm, &[heard_at(0, MOMENTS[0])])
            .expect("a listen to export");

        exported(&library, &file).expect("exported");

        let written = fs::read_to_string(&file).expect("the export");

        assert_eq!(written.lines().count(), 1);
        assert!(written.starts_with(&format!("{{\"listened_at\":{},", MOMENTS[0])));
        assert_eq!(fs::read_dir(&root).expect("the folder").count(), 3);

        fs::remove_dir_all(root).expect("cleaned");
    }

    #[test]
    #[cfg(unix)]
    fn an_export_to_something_that_is_no_file_is_written_into_it_as_it_stands() {
        let (root, library) = catalog_of_songs("history-export-device");

        library
            .bring_in(ListeningService::LastFm, &[heard_at(0, MOMENTS[0])])
            .expect("a listen to export");

        let written = exported(&library, Path::new("/dev/null"));

        fs::remove_dir_all(root).expect("cleaned");

        assert!(written.is_ok(), "{written:?}");
    }

    #[test]
    fn a_file_with_a_line_that_is_no_listen_is_refused_with_its_number_and_brings_nothing_in() {
        let (root, library) = catalog_of_songs("history-refused");
        let file = root.join("history.jsonl");
        let good = line_of(&HeardOnce {
            began: UNIX_EPOCH + Duration::from_secs(MOMENTS[0]),
            heard: Duration::from_secs(1),
            elsewhere: false,
            billed: Some(Billed {
                title: SONGS[0].0.to_owned(),
                artist: SONGS[0].1.to_owned(),
                album: Some(SONGS[0].2.to_owned()),
                recording: None,
                release: None,
                release_group: None,
                artist_mbid: None,
                isrc: None,
                number: None,
                length: None,
            }),
            location: "file:///gone.wav".to_owned(),
        });

        fs::write(&file, format!("{good}\n\nnot a listen\n")).expect("a history file");

        let refused = brought_in_from_file(&library, &file);

        assert!(matches!(
            refused,
            Err(Error::Library(resonate_library::Error::ListenLine {
                line: 3,
                ..
            }))
        ));
        assert!(moments_heard(&library).is_empty());

        fs::remove_dir_all(root).expect("cleaned");
    }

    #[test]
    fn a_file_that_is_not_there_is_refused_naming_it() {
        let (root, library) = catalog_of_songs("history-missing");
        let file = root.join("missing.jsonl");

        let refused = brought_in_from_file(&library, &file);

        assert!(matches!(
            refused,
            Err(Error::Library(resonate_library::Error::Io { path, .. })) if path == file
        ));

        fs::remove_dir_all(root).expect("cleaned");
    }

    #[test]
    fn a_file_and_a_service_are_two_ways_in_never_both_and_a_user_belongs_to_a_service() {
        let by_file = history_args(&["--import-file", "listenbrainz"]);
        let by_service = history_args(&["--import", "listenbrainz"]);

        assert_eq!(by_file.import_file, Some(PathBuf::from("listenbrainz")));
        assert!(by_file.import.is_none());
        assert!(by_service.import_file.is_none());
        assert!(by_service.import.is_some());
        for refused in [
            &["--import", "lastfm", "--import-file", "a.jsonl"][..],
            &["--export", "a.jsonl", "--import-file", "b.jsonl"],
            &["--import-file", "a.jsonl", "--user", "me"],
        ] {
            assert!(
                Cli::try_parse_from([&["resonate", "history"], refused].concat()).is_err(),
                "{refused:?}"
            );
        }
    }
}
