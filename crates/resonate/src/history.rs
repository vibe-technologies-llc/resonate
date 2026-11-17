use std::{
    fs::{self, File},
    io::{self, BufWriter, Write as _},
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use resonate_library::{
    Billed, BroughtIn, HeardElsewhere, HeardOnce, HistoryFrom, Library, ListeningService,
    heard_in_lines,
};

use crate::{
    cli::HistoryArgs,
    config::Config,
    error::{Error, Result},
    online,
    vocabulary::Meaning as _,
    words,
};

const READ_AT_ONCE: usize = 1_000;
const NAMED_AS: &str = "resonate";
const FILE_READ_AS: ListeningService = ListeningService::ListenBrainz;

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
    let mut from = Some(HistoryFrom::START);
    while let Some(at) = from {
        let read = library.history_from(at, READ_AT_ONCE)?;
        for heard in &read.heard {
            writeln!(written, "{}", line_of(heard)).map_err(failed)?;
            listens += 1;
        }
        from = read.next;
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

    let mut brought = BroughtIn::default();
    let mut from = None;
    loop {
        let page = history.page(from)?;
        brought = landed(library, service, brought, &page.heard)?;
        match page.next {
            Some(next) if Some(next) != from => from = Some(next),
            Some(_) | None => break,
        }
    }

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
    let heard = heard_in_lines(&bytes)?;

    let mut brought = BroughtIn::default();
    for page in heard.chunks(READ_AT_ONCE) {
        brought = landed(library, FILE_READ_AS, brought, page)?;
    }

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

fn landed(
    library: &Library,
    service: ListeningService,
    so_far: BroughtIn,
    page: &[HeardElsewhere],
) -> Result<BroughtIn> {
    let brought = so_far.and(library.bring_in(service, page)?);
    said!(
        "{}",
        words::history::brought_so_far(brought.credited, brought.held_already, brought.unmatched)
    );
    Ok(brought)
}

pub(crate) fn line_of(heard: &HeardOnce) -> String {
    let mut told = Object::default();
    if let Some(billed) = &heard.billed {
        told.text(
            "recording_mbid",
            billed.recording.as_ref().map(|mbid| mbid.as_str()),
        );
        told.text(
            "release_mbid",
            billed.release.as_ref().map(|mbid| mbid.as_str()),
        );
        told.text(
            "release_group_mbid",
            billed.release_group.as_ref().map(|mbid| mbid.as_str()),
        );
        if let Some(artist) = &billed.artist_mbid {
            told.raw("artist_mbids", &format!("[{}]", quoted(artist.as_str())));
        }
        told.text("isrc", billed.isrc.as_ref().map(|isrc| isrc.as_str()));
        told.number("tracknumber", billed.number.map(u64::from));
        told.number("duration_ms", billed.length.map(milliseconds));
    }
    told.number("listened_ms", Some(milliseconds(heard.heard)));
    told.raw(
        "heard_elsewhere",
        if heard.elsewhere { "true" } else { "false" },
    );
    told.text("origin_url", Some(&heard.location));
    told.text("media_player", Some(NAMED_AS));
    told.text("submission_client", Some(NAMED_AS));
    told.text("submission_client_version", Some(env!("CARGO_PKG_VERSION")));

    let mut metadata = Object::default();
    if let Some(Billed {
        title,
        artist,
        album,
        ..
    }) = &heard.billed
    {
        metadata.text("artist_name", Some(artist));
        metadata.text("track_name", Some(title));
        metadata.text("release_name", album.as_deref());
    }
    metadata.raw("additional_info", &told.closed());

    let mut listen = Object::default();
    let at = heard
        .began
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    listen.number("listened_at", Some(at));
    listen.raw("track_metadata", &metadata.closed());
    listen.closed()
}

fn milliseconds(span: Duration) -> u64 {
    u64::try_from(span.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Default)]
struct Object(Vec<String>);

impl Object {
    fn raw(&mut self, name: &str, value: &str) {
        self.0.push(format!("{}:{value}", quoted(name)));
    }

    fn text(&mut self, name: &str, value: Option<&str>) {
        if let Some(value) = value {
            self.raw(name, &quoted(value));
        }
    }

    fn number(&mut self, name: &str, value: Option<u64>) {
        if let Some(value) = value {
            self.raw(name, &value.to_string());
        }
    }

    fn closed(&self) -> String {
        format!("{{{}}}", self.0.join(","))
    }
}

fn quoted(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('"');
    for letter in text.chars() {
        match letter {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            control if u32::from(control) < 0x20 => {
                quoted.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser as _;
    use resonate_core::Recording;
    use resonate_library::{Library, Mbid};

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

    fn heard(billed: Option<Billed>) -> HeardOnce {
        HeardOnce {
            began: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            heard: Duration::from_millis(61_500),
            elsewhere: false,
            billed,
            location: "file:///music/a%22b.flac".to_owned(),
        }
    }

    #[test]
    fn a_listen_held_is_written_as_listenbrainz_writes_one_with_its_names_and_ids() {
        let billed = Billed {
            title: "Say \"hi\"\n".to_owned(),
            artist: "Low".to_owned(),
            album: Some("Words".to_owned()),
            recording: Some(Mbid::new("0a1b2c3d-1111-4222-8333-444455556666").expect("an mbid")),
            release: None,
            release_group: None,
            artist_mbid: None,
            isrc: None,
            number: Some(3),
            length: Some(Duration::from_secs(185)),
        };

        let line = line_of(&heard(Some(billed)));

        assert!(line.starts_with("{\"listened_at\":1700000000,\"track_metadata\":{"));
        assert!(line.contains("\"artist_name\":\"Low\",\"track_name\":\"Say \\\"hi\\\"\\n\",\"release_name\":\"Words\""));
        assert!(line.contains("\"recording_mbid\":\"0a1b2c3d-1111-4222-8333-444455556666\""));
        assert!(line.contains("\"tracknumber\":3,\"duration_ms\":185000,\"listened_ms\":61500"));
        assert!(line.contains("\"heard_elsewhere\":false"));
        assert!(!line.contains('\n'));
        assert!(line.ends_with("}}}"));
    }

    #[test]
    fn a_listen_of_a_song_no_longer_held_names_only_where_it_was() {
        let line = line_of(&heard(None));

        assert!(!line.contains("track_name"));
        assert!(line.contains("\"origin_url\":\"file:///music/a%22b.flac\""));
    }

    #[test]
    fn every_control_character_is_escaped() {
        assert_eq!(quoted("a\u{1}b\\"), "\"a\\u0001b\\\\\"");
    }
}
