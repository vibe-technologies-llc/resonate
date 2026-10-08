use std::{
    collections::BTreeMap,
    fmt::{self, Write as _},
    sync::Arc,
    time::UNIX_EPOCH,
};

use md5::{Digest, Md5};
use resonate_library::{
    Billed, ListeningService, LookupOp, Love, Loved, LovedNames, Mbid, Scrobble, Scrobbler,
    TokenHeld,
};
use serde::Deserialize;

use crate::{
    Client, Host,
    client::{Encoded, Posted},
    query::Params,
};

const FORM: &str = "application/x-www-form-urlencoded";
const SCROBBLED_AT_ONCE: usize = 50;
const ANSWERED_AS: &str = "json";
const SCROBBLE: &str = "track.scrobble";
const PLAYING_NOW: &str = "track.updateNowPlaying";
const WHO_IS_SIGNED_IN: &str = "user.getInfo";
const SIGN_IN: &str = "auth.getMobileSession";
const LOVE: &str = "track.love";
const UNLOVE: &str = "track.unlove";
const AUTHENTICATION_FAILED: u32 = 4;
const INVALID_API_KEY: u32 = 10;
const INVALID_SESSION: u32 = 9;
const SUSPENDED_API_KEY: u32 = 26;
const RATE_LIMITED: u32 = 29;
const OFFLINE_FOR_NOW: [u32; 2] = [11, 16];
const REFUSED_FOR_THE_SESSION: u16 = 401;
const REFUSED_FOR_NOW: u16 = 429;
const UNAVAILABLE: u16 = 503;

#[derive(Clone, PartialEq, Eq)]
pub struct Application {
    pub key: String,
    pub secret: String,
}

impl fmt::Debug for Application {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Application")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Session {
    pub name: String,
    pub key: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

pub struct LastFm {
    client: Arc<Client>,
    application: Application,
    session: String,
}

impl LastFm {
    pub fn new(client: Arc<Client>, application: Application, session: String) -> Self {
        Self {
            client,
            application,
            session,
        }
    }

    fn told(&self, method: &str, op: LookupOp, fields: Fields) -> resonate_library::Result<Answer> {
        let fields = fields
            .with("method", method)
            .with("api_key", &self.application.key)
            .with("sk", &self.session);
        asked(&self.client, op, &fields, &self.application.secret)
    }
}

pub fn signed_in(
    client: &Client,
    application: &Application,
    user: &str,
    password: &str,
) -> resonate_library::Result<Session> {
    let fields = Fields::default()
        .with("method", SIGN_IN)
        .with("api_key", &application.key)
        .with("username", user)
        .with("password", password);
    let answer = asked(client, LookupOp::Token, &fields, &application.secret)?;

    answer
        .session
        .filter(|session| !session.key.trim().is_empty())
        .map(|session| Session {
            name: session.name,
            key: session.key,
        })
        .ok_or(resonate_library::Error::Unreadable {
            op: LookupOp::Token,
        })
}

impl Scrobbler for LastFm {
    fn service(&self) -> ListeningService {
        ListeningService::LastFm
    }

    fn submit(&self, listens: &[Scrobble]) -> resonate_library::Result<()> {
        for batch in listens.chunks(SCROBBLED_AT_ONCE) {
            self.told(SCROBBLE, LookupOp::Submit, scrobbled(batch))?;
        }
        Ok(())
    }

    fn playing_now(&self, playing: &Billed) -> resonate_library::Result<()> {
        self.told(
            PLAYING_NOW,
            LookupOp::Submit,
            billed(Fields::default(), playing, None),
        )
        .map(drop)
    }

    fn love(&self, loved: &Loved, love: Love) -> resonate_library::Result<()> {
        let Some(named) = &loved.named else {
            tracing::debug!(
                recording = %loved.recording,
                "no track names the recording, so Last.fm is told nothing of it"
            );
            return Ok(());
        };
        let method = match love {
            Love::Loved => LOVE,
            Love::TakenBack => UNLOVE,
        };
        self.told(method, LookupOp::Love, loved_as(named)).map(drop)
    }

    fn token_held(&self) -> resonate_library::Result<TokenHeld> {
        match self.told(WHO_IS_SIGNED_IN, LookupOp::Token, Fields::default()) {
            Ok(Answer {
                user: Some(user), ..
            }) => Ok(TokenHeld::By(user.name)),
            Ok(_) => Err(resonate_library::Error::Unreadable {
                op: LookupOp::Token,
            }),
            Err(resonate_library::Error::Refused { status, .. })
                if status == REFUSED_FOR_THE_SESSION =>
            {
                Ok(TokenHeld::Unknown)
            }
            Err(error) => Err(error),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Fields(BTreeMap<String, String>);

impl Fields {
    fn with(mut self, name: &str, value: &str) -> Self {
        self.0.insert(name.to_owned(), value.to_owned());
        self
    }

    fn maybe(self, name: &str, value: Option<&str>) -> Self {
        match value {
            Some(value) => self.with(name, value),
            None => self,
        }
    }

    fn signature(&self, secret: &str) -> String {
        let mut signing = String::new();
        for (name, value) in &self.0 {
            signing.push_str(name);
            signing.push_str(value);
        }
        signing.push_str(secret);
        let mut written = String::with_capacity(32);
        for byte in Md5::digest(signing.as_bytes()) {
            let _ = write!(written, "{byte:02x}");
        }
        written
    }

    fn signed(&self, secret: &str) -> String {
        self.0
            .iter()
            .fold(Params::new(), |params, (name, value)| {
                params.with(name, value)
            })
            .with("api_sig", &self.signature(secret))
            .with("format", ANSWERED_AS)
            .finish_as_form()
    }
}

fn loved_as(named: &LovedNames) -> Fields {
    Fields::default()
        .with("artist", &named.artist)
        .with("track", &named.title)
}

fn scrobbled(batch: &[Scrobble]) -> Fields {
    batch
        .iter()
        .enumerate()
        .fold(Fields::default(), |fields, (at, told)| {
            let began = told
                .at
                .duration_since(UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            billed(fields, &told.billed, Some(at))
                .with(&format!("timestamp[{at}]"), &began.to_string())
        })
}

fn billed(fields: Fields, billed: &Billed, at: Option<usize>) -> Fields {
    let named = |name: &str| match at {
        Some(at) => format!("{name}[{at}]"),
        None => name.to_owned(),
    };
    fields
        .with(&named("artist"), &billed.artist)
        .with(&named("track"), &billed.title)
        .maybe(&named("album"), billed.album.as_deref())
        .maybe(&named("mbid"), billed.recording.as_ref().map(Mbid::as_str))
        .maybe(
            &named("trackNumber"),
            billed.number.map(|number| number.to_string()).as_deref(),
        )
        .maybe(
            &named("duration"),
            billed
                .length
                .map(|length| length.as_secs().to_string())
                .as_deref(),
        )
}

#[derive(Default, Deserialize)]
struct Answer {
    error: Option<u32>,
    session: Option<SessionDoc>,
    user: Option<UserDoc>,
}

#[derive(Deserialize)]
struct SessionDoc {
    name: String,
    key: String,
}

#[derive(Debug, Deserialize)]
struct UserDoc {
    name: String,
}

fn asked(
    client: &Client,
    op: LookupOp,
    fields: &Fields,
    secret: &str,
) -> resonate_library::Result<Answer> {
    let answer: Option<Answer> = client.posted(
        Host::LastFm,
        op,
        Host::LastFm.base(),
        &Posted {
            content_type: FORM.to_owned(),
            encoded: Encoded::Plain,
            bytes: fields.signed(secret).into_bytes(),
            authorization: None,
        },
    )?;
    let answer = answer.ok_or(resonate_library::Error::Unreadable { op })?;

    match answer.error {
        None => Ok(answer),
        Some(code) => Err(refused(op, code)),
    }
}

fn refused(op: LookupOp, code: u32) -> resonate_library::Error {
    let status = match code {
        AUTHENTICATION_FAILED | INVALID_API_KEY | INVALID_SESSION | SUSPENDED_API_KEY => {
            REFUSED_FOR_THE_SESSION
        }
        RATE_LIMITED => REFUSED_FOR_NOW,
        code if OFFLINE_FOR_NOW.contains(&code) => UNAVAILABLE,
        _ => return resonate_library::Error::Unreadable { op },
    };
    resonate_library::Error::Refused { op, status }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use resonate_core::ListenId;

    use super::*;

    #[test]
    fn an_application_and_a_session_print_neither_secret_nor_session_key() {
        let application = Application {
            key: "public-key".to_owned(),
            secret: "application-hush".to_owned(),
        };
        let session = Session {
            name: "listener".to_owned(),
            key: "session-sesame".to_owned(),
        };

        let printed = format!("{application:?} {session:?}");

        assert!(printed.contains("public-key"));
        assert!(printed.contains("listener"));
        assert!(!printed.contains("application-hush"));
        assert!(!printed.contains("session-sesame"));
    }

    fn heard(title: &str, seconds: u64) -> Scrobble {
        Scrobble {
            listen: ListenId::new(1).expect("a listen"),
            at: SystemTime::UNIX_EPOCH + Duration::from_secs(seconds),
            billed: Billed {
                title: title.to_owned(),
                artist: "Pink Floyd".to_owned(),
                album: Some("Meddle".to_owned()),
                recording: None,
                release: None,
                release_group: None,
                artist_mbid: None,
                isrc: None,
                number: Some(6),
                length: Some(Duration::from_secs(1_410)),
            },
        }
    }

    #[test]
    fn a_request_is_signed_by_its_fields_in_name_order_and_the_secret() {
        let fields = Fields::default()
            .with("method", SIGN_IN)
            .with("api_key", "key")
            .with("username", "someone")
            .with("password", "sesame");

        let mut expected = String::new();
        for byte in Md5::digest(
            "api_keykeymethodauth.getMobileSessionpasswordsesameusernamesomeonesecret".as_bytes(),
        ) {
            let _ = write!(expected, "{byte:02x}");
        }
        assert_eq!(fields.signature("secret"), expected);
        assert!(fields.signed("secret").ends_with("&format=json"));
        assert!(!fields.signed("secret").contains("secret"));
    }

    #[test]
    fn a_batch_of_listens_is_numbered_and_each_carries_when_it_began() {
        let fields = scrobbled(&[heard("Echoes", 100), heard("Fearless", 200)]);

        assert_eq!(fields.0.get("track[0]").map(String::as_str), Some("Echoes"));
        assert_eq!(
            fields.0.get("track[1]").map(String::as_str),
            Some("Fearless")
        );
        assert_eq!(
            fields.0.get("timestamp[1]").map(String::as_str),
            Some("200")
        );
        assert_eq!(
            fields.0.get("duration[0]").map(String::as_str),
            Some("1410")
        );
        assert_eq!(
            fields.0.get("trackNumber[0]").map(String::as_str),
            Some("6")
        );
        assert!(!fields.0.contains_key("mbid[0]"));
    }

    #[test]
    fn a_love_names_the_track_by_its_artist_and_title() {
        let fields = loved_as(&LovedNames {
            title: "Echoes".to_owned(),
            artist: "Pink Floyd".to_owned(),
        });

        assert_eq!(fields.0.get("track").map(String::as_str), Some("Echoes"));
        assert_eq!(
            fields.0.get("artist").map(String::as_str),
            Some("Pink Floyd")
        );
        assert_eq!(fields.0.len(), 2);
    }

    #[test]
    fn an_answer_naming_a_refused_session_is_told_apart_from_a_busy_service() {
        assert!(matches!(
            refused(LookupOp::Submit, INVALID_SESSION),
            resonate_library::Error::Refused {
                status: REFUSED_FOR_THE_SESSION,
                ..
            }
        ));
        assert!(matches!(
            refused(LookupOp::Submit, RATE_LIMITED),
            resonate_library::Error::Refused {
                status: REFUSED_FOR_NOW,
                ..
            }
        ));
        assert!(matches!(
            refused(LookupOp::Submit, 6),
            resonate_library::Error::Unreadable { .. }
        ));
    }
}
