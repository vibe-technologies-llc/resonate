use std::{
    fs::File,
    io::{self, BufWriter, Write as _},
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use resonate_library::{Billed, HeardOnce, HistoryFrom, Library, ListeningService};

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

pub(crate) fn history(library: &Library, config: &Config, asked: &HistoryArgs) -> Result<()> {
    if let Some(file) = &asked.export {
        return exported(library, file);
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

    let mut brought = resonate_library::BroughtIn::default();
    let mut from = None;
    loop {
        let page = history.page(from)?;
        brought = brought.and(library.bring_in(service, &page.heard)?);
        said!(
            "{}",
            words::history::brought_so_far(
                brought.credited,
                brought.held_already,
                brought.unmatched
            )
        );
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
    use resonate_library::Mbid;

    use super::*;

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
