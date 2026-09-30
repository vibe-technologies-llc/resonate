use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use resonate_library::{
    Billed, ListeningService, LookupOp, Love, Mbid, Scrobble, Scrobbler, TokenHeld,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Client, Host,
    client::{Encoded, Posted},
};

const SUBMIT_LISTENS: &str = "/1/submit-listens";
const RECORDING_FEEDBACK: &str = "/1/feedback/recording-feedback";
const VALIDATE_TOKEN: &str = "/1/validate-token";
const LOVED: i8 = 1;
const NEITHER: i8 = 0;
const JSON: &str = "application/json";
const TOKEN_SCHEME: &str = "Token";
const ONE_LISTEN: &str = "single";
const SEVERAL_LISTENS: &str = "import";
const PLAYING_NOW: &str = "playing_now";
const NAME: &str = "resonate";
const ACCEPTED: &str = "ok";

#[derive(Deserialize)]
struct Answer {
    status: String,
}

#[derive(Deserialize)]
struct Validated {
    valid: bool,
    user_name: Option<String>,
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
        self.posted(&submission(listens))
    }

    fn playing_now(&self, playing: &Billed) -> resonate_library::Result<()> {
        self.posted(&now_playing(playing))
    }

    fn love(&self, recording: &Mbid, love: Love) -> resonate_library::Result<()> {
        self.posted_to(
            RECORDING_FEEDBACK,
            LookupOp::Love,
            &feedback(recording, love),
        )
    }

    fn token_held(&self) -> resonate_library::Result<TokenHeld> {
        let url = format!("{}{VALIDATE_TOKEN}", Host::ListenBrainz.base());
        let answered = self.client.json_as::<Validated>(
            Host::ListenBrainz,
            LookupOp::Token,
            &url,
            &self.authorization(),
        );

        match answered {
            Ok(Some(validated)) => Ok(held_by(validated)),
            Ok(None) => Ok(TokenHeld::Unknown),
            Err(crate::Error::Refused { status, .. }) if TOKEN_UNKNOWN.contains(&status) => {
                Ok(TokenHeld::Unknown)
            }
            Err(error) => Err(error.into()),
        }
    }
}

const TOKEN_UNKNOWN: [u16; 3] = [400, 401, 403];

fn held_by(validated: Validated) -> TokenHeld {
    match validated.user_name {
        Some(user) if validated.valid && !user.trim().is_empty() => TokenHeld::By(user),
        _ => TokenHeld::Unknown,
    }
}

fn feedback(recording: &Mbid, love: Love) -> Value {
    json!({
        "recording_mbid": recording.as_str(),
        "score": match love {
            Love::Loved => LOVED,
            Love::TakenBack => NEITHER,
        },
    })
}

impl ListenBrainz {
    fn posted(&self, body: &Value) -> resonate_library::Result<()> {
        self.posted_to(SUBMIT_LISTENS, LookupOp::Submit, body)
    }

    fn posted_to(&self, path: &str, op: LookupOp, body: &Value) -> resonate_library::Result<()> {
        let url = format!("{}{path}", Host::ListenBrainz.base());
        let answer: Option<Answer> = self.client.posted(
            Host::ListenBrainz,
            op,
            &url,
            &Posted {
                content_type: JSON.to_owned(),
                encoded: Encoded::Plain,
                bytes: body.to_string().into_bytes(),
                authorization: Some(self.authorization()),
            },
        )?;

        match answer {
            Some(answer) if answer.status == ACCEPTED => Ok(()),
            Some(_) | None => Err(resonate_library::Error::Unreadable { op }),
        }
    }

    fn authorization(&self) -> String {
        format!("{TOKEN_SCHEME} {}", self.token.trim())
    }
}

fn submission(listens: &[Scrobble]) -> Value {
    json!({
        "listen_type": if listens.len() == 1 { ONE_LISTEN } else { SEVERAL_LISTENS },
        "payload": listens.iter().map(listen).collect::<Vec<_>>(),
    })
}

fn now_playing(playing: &Billed) -> Value {
    json!({
        "listen_type": PLAYING_NOW,
        "payload": [{ "track_metadata": metadata(playing) }],
    })
}

fn listen(told: &Scrobble) -> Value {
    json!({
        "listened_at": told
            .at
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
        "track_metadata": metadata(&told.billed),
    })
}

fn metadata(told: &Billed) -> Value {
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
    if let Some(group) = &told.release_group {
        additional.insert("release_group_mbid".to_owned(), Value::from(group.as_str()));
    }
    if let Some(isrc) = &told.isrc {
        additional.insert("isrc".to_owned(), Value::from(isrc.as_str()));
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
    Value::Object(metadata)
}

fn milliseconds(length: Duration) -> u64 {
    u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use resonate_core::ListenId;
    use resonate_library::{Isrc, Mbid};

    use super::*;

    fn told(title: &str) -> Scrobble {
        Scrobble {
            listen: ListenId::new(7).expect("a listen id is not zero"),
            at: UNIX_EPOCH + Duration::from_secs(1_700_000_000),
            billed: billed(title),
        }
    }

    fn billed(title: &str) -> Billed {
        Billed {
            title: title.to_owned(),
            artist: "Pink Floyd".to_owned(),
            album: None,
            recording: None,
            release: None,
            release_group: None,
            artist_mbid: None,
            isrc: None,
            number: None,
            length: None,
        }
    }

    #[test]
    fn what_is_playing_now_is_told_with_no_moment_and_as_one_listen() {
        assert_eq!(
            now_playing(&billed("Echoes")),
            json!({
                "listen_type": "playing_now",
                "payload": [{
                    "track_metadata": {
                        "artist_name": "Pink Floyd",
                        "track_name": "Echoes",
                        "additional_info": {
                            "media_player": "resonate",
                            "submission_client": "resonate",
                            "submission_client_version": env!("CARGO_PKG_VERSION"),
                        },
                    },
                }],
            })
        );
    }

    #[test]
    fn a_favourite_is_told_as_a_love_and_taken_back_as_no_feedback_at_all() {
        let recording =
            Mbid::new("b1a9c0de-1111-4222-8333-444455556666").expect("a well-formed mbid");

        assert_eq!(
            feedback(&recording, Love::Loved),
            json!({ "recording_mbid": "b1a9c0de-1111-4222-8333-444455556666", "score": 1 })
        );
        assert_eq!(
            feedback(&recording, Love::TakenBack),
            json!({ "recording_mbid": "b1a9c0de-1111-4222-8333-444455556666", "score": 0 })
        );
    }

    #[test]
    fn a_token_is_held_by_the_user_the_service_names_and_by_nobody_it_calls_invalid() {
        let read = |text: &str| {
            held_by(serde_json::from_str::<Validated>(text).expect("a validation answer"))
        };

        assert_eq!(
            read(r#"{"code":200,"message":"Token valid.","valid":true,"user_name":"ada"}"#),
            TokenHeld::By("ada".to_owned())
        );
        assert_eq!(
            read(r#"{"code":200,"message":"Token invalid.","valid":false}"#),
            TokenHeld::Unknown
        );
        assert_eq!(
            read(r#"{"valid":true,"user_name":" "}"#),
            TokenHeld::Unknown
        );
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
            billed: Billed {
                album: Some("Meddle".to_owned()),
                recording: Some(
                    Mbid::new("b1a9c0de-1111-4222-8333-444455556666").expect("a well-formed mbid"),
                ),
                release: Some(
                    Mbid::new("aadf62d6-d475-42e0-b622-e6da7a59fdf7").expect("a well-formed mbid"),
                ),
                release_group: Some(
                    Mbid::new("2a9b3ef6-6e5f-3b51-9d4a-4f29fd2b6c41").expect("a well-formed mbid"),
                ),
                artist_mbid: Some(
                    Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("a well-formed mbid"),
                ),
                isrc: Some(Isrc::new("GBN9Y1100065").expect("a well-formed isrc")),
                number: Some(6),
                length: Some(Duration::from_millis(1_412_500)),
                ..billed("Echoes")
            },
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
            additional["release_group_mbid"],
            "2a9b3ef6-6e5f-3b51-9d4a-4f29fd2b6c41"
        );
        assert_eq!(additional["isrc"], "GBN9Y1100065");
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
