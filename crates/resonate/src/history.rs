use std::{
    fs::{self, File},
    io::{self, BufWriter, Write as _},
    path::Path,
};

use resonate_library::{
    BroughtIn, Library, ListeningService, bring_in_history, bring_in_lines, history_lines,
};

use crate::{
    cli::HistoryArgs,
    config::Config,
    error::{Error, Result},
    online,
    vocabulary::Meaning as _,
    words,
};

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

fn exported(library: &Library, file: &Path) -> Result<()> {
    let failed = |source: io::Error| Error::WriteHistory {
        path: file.to_path_buf(),
        source,
    };
    let mut written = BufWriter::new(File::create(file).map_err(failed)?);

    let mut listens = 0_u64;
    for line in history_lines(library) {
        writeln!(written, "{}", line?).map_err(failed)?;
        listens += 1;
    }
    let file_written = written
        .into_inner()
        .map_err(|error| failed(error.into_error()))?;
    file_written.sync_all().map_err(failed)?;

    said!(
        "{}",
        words::history::exported(listens, file.display().to_string())
    );
    Ok(())
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
    let bytes = fs::read(file).map_err(|source| resonate_library::Error::Io {
        path: file.to_path_buf(),
        source,
    })?;

    let brought = bring_in_lines(library, &bytes, &mut said_so_far)?;

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
    use resonate_library::{Billed, HeardElsewhere, HeardOnce, HistoryFrom, Library, line_of};

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
