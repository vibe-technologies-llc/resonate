use std::io::{BufRead, Write};

use resonate_library::Library;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Code, Error, MethodName, Refusal, ResourceUri, Result, StreamOp, ToolName,
    controlling::Reach,
    error::said,
    passes::{Lookups, Passes},
    resources::{self, Resource},
    tools::Tool,
};

const JSON_RPC: &str = "2.0";
const LATEST_PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = [LATEST_PROTOCOL, "2025-03-26", "2024-11-05"];
const SERVER_NAME: &str = "resonate";

const INITIALIZE: &str = "initialize";
const PING: &str = "ping";
const TOOLS_LIST: &str = "tools/list";
const TOOLS_CALL: &str = "tools/call";
const RESOURCES_LIST: &str = "resources/list";
const RESOURCE_TEMPLATES_LIST: &str = "resources/templates/list";
const RESOURCES_READ: &str = "resources/read";

const INSTRUCTIONS: &str = "Resonate is a music player. The catalog tools read and edit its \
                            library directly — its favourites, its playlists and the missing \
                            tracks it wants — and answer whether or not a player is running; \
                            the transport tools reach a resonate player on the session bus, say \
                            so when none is there, and answer with what the player reads back \
                            once a gesture has landed. A track_id names a row of the catalog, a \
                            queue_id a row of the running player's queue and a \
                            release_track_id a row of a release the catalog holds no file for; \
                            none of them are the same numbers. A scan, a lookup and a poll run \
                            in the background once started: library_passes says how far each \
                            has come, and one still running when the session ends is stopped \
                            at the next file. The same readings are offered as resources: what \
                            is playing, the queue, the passes, the playlists and each playlist's \
                            rows, the favourites, the month's listening, the suggestions and \
                            the missing tracks.";

pub struct Server {
    library: Library,
    players: Box<dyn Reach>,
    passes: Passes,
}

enum Message {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification,
    Answer,
}

#[derive(Deserialize)]
struct Initialising {
    #[serde(rename = "protocolVersion")]
    protocol_version: String,
}

#[derive(Deserialize)]
struct Reading {
    uri: String,
}

enum Unanswered {
    Refused(Refusal),
    Failed(Error),
}

impl From<Refusal> for Unanswered {
    fn from(refusal: Refusal) -> Self {
        Self::Refused(refusal)
    }
}

impl From<Error> for Unanswered {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl Unanswered {
    fn code(&self) -> Code {
        match self {
            Self::Refused(refusal) => refusal.code(),
            Self::Failed(_) => Code::InternalError,
        }
    }

    fn said(&self) -> String {
        match self {
            Self::Refused(refusal) => said(refusal),
            Self::Failed(error) => said(error),
        }
    }
}

#[derive(Deserialize)]
struct Calling {
    name: String,
    arguments: Option<Value>,
}

impl Server {
    pub fn new(library: Library, players: impl Reach + 'static) -> Self {
        Self {
            library,
            players: Box::new(players),
            passes: Passes::over(Lookups::none()),
        }
    }

    #[must_use]
    pub fn looking_up_with(self, lookups: Lookups) -> Self {
        Self {
            passes: Passes::over(lookups),
            ..self
        }
    }

    pub fn serve(&self, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = input
                .read_until(b'\n', &mut line)
                .map_err(|source| Error::Stream {
                    op: StreamOp::Read,
                    source,
                })?;
            if read == 0 {
                self.passes.drain();
                return Ok(());
            }
            if line.trim_ascii().is_empty() {
                continue;
            }
            let Some(answer) = self.answer(&line) else {
                continue;
            };
            writeln!(output, "{answer}")
                .and_then(|()| output.flush())
                .map_err(|source| Error::Stream {
                    op: StreamOp::Write,
                    source,
                })?;
        }
    }

    pub fn answer(&self, line: &[u8]) -> Option<Value> {
        let (id, method, params) = match read(line) {
            Ok(Message::Request { id, method, params }) => (id, method, params),
            Ok(Message::Notification | Message::Answer) => return None,
            Err((id, refusal)) => return Some(refused(id, &Unanswered::Refused(refusal))),
        };

        Some(match self.respond(&method, params) {
            Ok(result) => json!({ "jsonrpc": JSON_RPC, "id": id, "result": result }),
            Err(unanswered) => refused(id, &unanswered),
        })
    }

    fn respond(&self, method: &str, params: Value) -> std::result::Result<Value, Unanswered> {
        match method {
            INITIALIZE => {
                let asked: Initialising = parameters(method, params)?;
                Ok(initialised(&asked.protocol_version))
            }
            PING => Ok(json!({})),
            TOOLS_LIST => Ok(json!({ "tools": Tool::ALL.map(Tool::listed) })),
            TOOLS_CALL => {
                let asked: Calling = parameters(method, params)?;
                let tool = Tool::named(&asked.name)
                    .ok_or_else(|| Refusal::UnknownTool(ToolName::new(asked.name)))?;
                let arguments = match asked.arguments {
                    None | Some(Value::Null) => Value::Object(Map::new()),
                    Some(given) => given,
                };
                let outcome = tool.run(arguments, &self.library, &*self.players, &self.passes)?;
                Ok(called(tool, outcome))
            }
            RESOURCES_LIST => {
                let offered = resources::every(&self.library)?;
                Ok(json!({ "resources": offered.iter().map(Resource::listed).collect::<Vec<_>>() }))
            }
            RESOURCE_TEMPLATES_LIST => Ok(json!({ "resourceTemplates": Resource::templates() })),
            RESOURCES_READ => {
                let asked: Reading = parameters(method, params)?;
                let resource = Resource::at(&asked.uri).ok_or_else(|| {
                    Refusal::UnknownResource(ResourceUri::new(asked.uri.as_str()))
                })?;
                let read = resource.read(&self.library, &*self.players, &self.passes)?;
                Ok(Resource::contents(&asked.uri, &read))
            }
            other => Err(Refusal::UnknownMethod(MethodName::new(other)).into()),
        }
    }
}

fn read(line: &[u8]) -> std::result::Result<Message, (Value, Refusal)> {
    let message: Value =
        serde_json::from_slice(line).map_err(|source| (Value::Null, Refusal::Unparsed(source)))?;
    let Value::Object(mut envelope) = message else {
        return Err((Value::Null, Refusal::NotARequest));
    };
    let id = envelope.remove("id");
    if envelope.get("jsonrpc").and_then(Value::as_str) != Some(JSON_RPC) {
        return Err((answerable(id), Refusal::NotARequest));
    }

    match (id, envelope.remove("method")) {
        (None, Some(Value::String(_))) => Ok(Message::Notification),
        (Some(id @ (Value::String(_) | Value::Number(_))), Some(Value::String(method))) => {
            Ok(Message::Request {
                id,
                method,
                params: envelope
                    .remove("params")
                    .unwrap_or_else(|| Value::Object(Map::new())),
            })
        }
        (Some(_), None) if envelope.contains_key("result") || envelope.contains_key("error") => {
            Ok(Message::Answer)
        }
        (id, _) => Err((answerable(id), Refusal::NotARequest)),
    }
}

fn answerable(id: Option<Value>) -> Value {
    match id {
        Some(id @ (Value::String(_) | Value::Number(_))) => id,
        _ => Value::Null,
    }
}

fn parameters<Asked: serde::de::DeserializeOwned>(
    method: &str,
    params: Value,
) -> std::result::Result<Asked, Refusal> {
    serde_json::from_value(params).map_err(|source| Refusal::BadParameters {
        method: MethodName::new(method),
        source,
    })
}

fn refused(id: Value, unanswered: &Unanswered) -> Value {
    json!({
        "jsonrpc": JSON_RPC,
        "id": id,
        "error": { "code": unanswered.code().number(), "message": unanswered.said() },
    })
}

fn initialised(asked: &str) -> Value {
    let protocol = PROTOCOLS
        .into_iter()
        .find(|known| *known == asked)
        .unwrap_or(LATEST_PROTOCOL);

    json!({
        "protocolVersion": protocol,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": false, "listChanged": false },
        },
        "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn called(tool: Tool, outcome: Result<Value>) -> Value {
    match outcome {
        Ok(answer) => json!({
            "content": [{ "type": "text", "text": answer.to_string() }],
            "structuredContent": answer,
            "isError": false,
        }),
        Err(error) => {
            tracing::debug!(%tool, %error, "a tool ran and failed");
            json!({
                "content": [{ "type": "text", "text": said(&error) }],
                "isError": true,
            })
        }
    }
}
