use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use resonate_library::{ListeningService, LookupOp, Scrobble, Scrobbler};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Client, Host,
    client::{Encoded, Posted},
};

const SUBMIT_LISTENS: &str = "/1/submit-listens";
const JSON: &str = "application/json";
const TOKEN_SCHEME: &str = "Token";
const ONE_LISTEN: &str = "single";
const SEVERAL_LISTENS: &str = "import";
const NAME: &str = "resonate";
const ACCEPTED: &str = "ok";

#[derive(Deserialize)]
struct Answer {
    status: String,
}

pub struct ListenBrainz {
    client: Arc<Client>,
    token: String,
}

impl ListenBrainz {
    pub fn new(client: Arc<Client>, token: String) -> Self {
        Self { client, token }
    }
}

impl Scrobbler for ListenBrainz {
    fn service(&self) -> ListeningService {
        ListeningService::ListenBrainz
    }

    fn submit(&self, listens: &[Scrobble]) -> resonate_library::Result<()> {
        let url = format!("{}{SUBMIT_LISTENS}", Host::ListenBrainz.base());
        let answer: Option<Answer> = self.client.posted(
            Host::ListenBrainz,
            LookupOp::Submit,
            &url,
            &Posted {
                content_type: JSON.to_owned(),
                encoded: Encoded::Plain,
                bytes: submission(listens).to_string().into_bytes(),
                authorization: Some(format!("{TOKEN_SCHEME} {}", self.token.trim())),
            },
        )?;

        match answer {
            Some(answer) if answer.status == ACCEPTED => Ok(()),
            Some(_) | None => Err(resonate_library::Error::Unreadable {
                op: LookupOp::Submit,
            }),
        }
    }
}

fn submission(listens: &[Scrobble]) -> Value {
    json!({
        "listen_type": if listens.len() == 1 { ONE_LISTEN } else { SEVERAL_LISTENS },
        "payload": listens.iter().map(listen).collect::<Vec<_>>(),
    })
}

fn listen(told: &Scrobble) -> Value {
    let mut additional = Map::new();
    additional.insert("media_player".to_owned(), Value::from(NAME));
    additional.insert("submission_client".to_owned(), Value::from(NAME));
    additional.insert(
        "submission_client_version".to_owned(),
        Value::from(env!("CARGO_PKG_VERSION")),
    );
    if let Some(recording) = &told.recording {
        additional.insert("recording_mbid".to_owned(), Value::from(recording.as_str()));
    }
    if let Some(release) = &told.release {
        additional.insert("release_mbid".to_owned(), Value::from(release.as_str()));
    }
    if let Some(artist) = &told.artist_mbid {
        additional.insert("artist_mbids".to_owned(), json!([artist.as_str()]));
    }
    if let Some(number) = told.number {
        additional.insert("tracknumber".to_owned(), Value::from(number));
    }
    if let Some(length) = told.length.filter(|length| !length.is_zero()) {
        additional.insert("duration_ms".to_owned(), Value::from(milliseconds(length)));
    }

    let mut metadata = Map::new();
    metadata.insert("artist_name".to_owned(), Value::from(told.artist.as_str()));
    metadata.insert("track_name".to_owned(), Value::from(told.title.as_str()));
    if let Some(album) = &told.album {
        metadata.insert("release_name".to_owned(), Value::from(album.as_str()));
    }
    metadata.insert("additional_info".to_owned(), Value::Object(additional));

    json!({
        "listened_at": told
            .at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
        "track_metadata": metadata,
    })
}

fn milliseconds(length: Duration) -> u64 {
    u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use resonate_core::ListenId;
    use resonate_library::Mbid;

    use super::*;

    fn told(title: &str) -> Scrobble {
        Scrobble {
            listen: ListenId::new(7).expect("a listen id is not zero"),
            at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            title: title.to_owned(),
            artist: "Pink Floyd".to_owned(),
            album: None,
            recording: None,
            release: None,
            artist_mbid: None,
            number: None,
            length: None,
        }
    }

    #[test]
    fn one_listen_is_submitted_as_a_single_and_several_as_an_import() {
        assert_eq!(
            submission(&[told("Echoes")])["listen_type"],
            Value::from("single")
        );
        assert_eq!(
            submission(&[told("Echoes"), told("Time")])["listen_type"],
            Value::from("import")
        );
    }

    #[test]
    fn a_listen_says_what_was_heard_when_and_nothing_the_catalog_does_not_hold() {
        let bare = listen(&told("Echoes"));
        assert_eq!(
            bare,
            json!({
                "listened_at": 1_700_000_000,
                "track_metadata": {
                    "artist_name": "Pink Floyd",
                    "track_name": "Echoes",
                    "additional_info": {
                        "media_player": "resonate",
                        "submission_client": "resonate",
                        "submission_client_version": env!("CARGO_PKG_VERSION"),
                    },
                },
            })
        );

        let whole = listen(&Scrobble {
            album: Some("Meddle".to_owned()),
            recording: Some(
                Mbid::new("b1a9c0de-1111-4222-8333-444455556666").expect("a well-formed mbid"),
            ),
            release: Some(
                Mbid::new("aadf62d6-d475-42e0-b622-e6da7a59fdf7").expect("a well-formed mbid"),
            ),
            artist_mbid: Some(
                Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("a well-formed mbid"),
            ),
            number: Some(6),
            length: Some(Duration::from_millis(1_412_500)),
            ..told("Echoes")
        });
        let metadata = &whole["track_metadata"];
        assert_eq!(metadata["release_name"], "Meddle");
        let additional = &metadata["additional_info"];
        assert_eq!(
            additional["recording_mbid"],
            "b1a9c0de-1111-4222-8333-444455556666"
        );
        assert_eq!(
            additional["release_mbid"],
            "aadf62d6-d475-42e0-b622-e6da7a59fdf7"
        );
        assert_eq!(
            additional["artist_mbids"],
            json!(["83d91898-7763-47d7-b03b-b92132375c47"])
        );
        assert_eq!(additional["tracknumber"], 6);
        assert_eq!(additional["duration_ms"], 1_412_500);
    }

    #[test]
    fn a_listen_before_the_epoch_is_written_as_the_epoch_rather_than_refused() {
        let early = listen(&Scrobble {
            at: UNIX_EPOCH - Duration::from_secs(5),
            ..told("Echoes")
        });
        assert_eq!(early["listened_at"], 0);
    }
}
