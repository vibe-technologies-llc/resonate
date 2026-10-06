use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    io::{self, BufRead, Read as _, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread,
    time::Duration,
};

use resonate_library::Library;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Code, Error, MethodName, PromptName, Refusal, ResourceUri, Result, StreamOp, ToolName,
    completions::Completing,
    controlling::Reach,
    error::said,
    passes::{Lookups, Passes},
    prompts::Prompt,
    resources::{self, Resource},
    tools::Tool,
};

const JSON_RPC: &str = "2.0";
const LONGEST_MESSAGE: usize = 4 * 1024 * 1024;
const LATEST_PROTOCOL: &str = "2025-06-18";
const PROTOCOLS: [&str; 3] = [LATEST_PROTOCOL, "2025-03-26", "2024-11-05"];
const SERVER_NAME: &str = "resonate";
const LOOKED_OVER_EVERY: Duration = Duration::from_millis(500);

const INITIALIZE: &str = "initialize";
const PING: &str = "ping";
const TOOLS_LIST: &str = "tools/list";
const TOOLS_CALL: &str = "tools/call";
const RESOURCES_LIST: &str = "resources/list";
const RESOURCE_TEMPLATES_LIST: &str = "resources/templates/list";
const RESOURCES_READ: &str = "resources/read";
const RESOURCES_SUBSCRIBE: &str = "resources/subscribe";
const RESOURCES_UNSUBSCRIBE: &str = "resources/unsubscribe";
const RESOURCE_UPDATED: &str = "notifications/resources/updated";
const RESOURCE_LIST_CHANGED: &str = "notifications/resources/list_changed";
const PROMPTS_LIST: &str = "prompts/list";
const PROMPTS_GET: &str = "prompts/get";
const COMPLETION_COMPLETE: &str = "completion/complete";

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
                            rows, the favourites, the listening over a window, the suggestions \
                            and the missing tracks. The prompts build a playlist from a brief, \
                            review the listening, weigh what the albums are short of and talk \
                            about what is playing; their arguments, and those of the resource \
                            templates, are completed from the catalog.";

pub struct Server {
    library: Library,
    players: Box<dyn Reach>,
    passes: Passes,
    pushing: Cell<bool>,
    subscribed: RefCell<BTreeMap<String, Option<Value>>>,
    listed: RefCell<Option<Vec<Value>>>,
}

enum Line {
    Ended,
    Held,
    TooLong,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flow {
    Going,
    Ended,
}

enum Heard {
    Read(io::Result<Line>, Vec<u8>),
    Tick,
    Stop,
}

#[derive(Clone)]
pub struct Stop {
    sender: SyncSender<Heard>,
    asked: Arc<AtomicBool>,
}

impl Stop {
    pub fn stop(&self) {
        self.asked.store(true, Ordering::Release);
        match self.sender.try_send(Heard::Stop) {
            Ok(()) | Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                tracing::debug!("the session had already ended when it was told to stop");
            }
        }
    }
}

pub struct Stoppable {
    sender: SyncSender<Heard>,
    heard: Receiver<Heard>,
    asked: Arc<AtomicBool>,
}

pub fn stoppable() -> (Stop, Stoppable) {
    let (sender, heard) = mpsc::sync_channel(1);
    let asked = Arc::new(AtomicBool::new(false));
    let stop = Stop {
        sender: sender.clone(),
        asked: Arc::clone(&asked),
    };
    (
        stop,
        Stoppable {
            sender,
            heard,
            asked,
        },
    )
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
struct Getting {
    name: String,
    arguments: Option<Value>,
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
            pushing: Cell::new(false),
            subscribed: RefCell::new(BTreeMap::new()),
            listed: RefCell::new(None),
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
            let read = next_line(&mut input, &mut line);
            if self.took(read, &line, &mut output)? == Flow::Ended {
                return Ok(());
            }
        }
    }

    pub fn serve_until_stopped(
        &self,
        input: impl BufRead + Send + 'static,
        mut output: impl Write,
        stoppable: Stoppable,
    ) -> Result<()> {
        let Stoppable {
            sender,
            heard,
            asked,
        } = stoppable;
        let ticking = sender.clone();
        thread::Builder::new()
            .name("resonate-mcp-read".to_owned())
            .spawn(move || read_into(input, &sender))
            .and_then(|_| {
                thread::Builder::new()
                    .name("resonate-mcp-tick".to_owned())
                    .spawn(move || tick_into(&ticking))
            })
            .map_err(|source| Error::Stream {
                op: StreamOp::Read,
                source,
            })?;
        self.pushing.set(true);

        for told in heard {
            if asked.load(Ordering::Acquire) {
                break;
            }
            match told {
                Heard::Stop => break,
                Heard::Tick => {
                    for notification in self.changed() {
                        written_out(&mut output, &notification)?;
                    }
                }
                Heard::Read(read, line) => {
                    if self.took(read, &line, &mut output)? == Flow::Ended {
                        return Ok(());
                    }
                }
            }
        }
        self.passes.drain();
        Ok(())
    }

    fn took(&self, read: io::Result<Line>, line: &[u8], output: &mut impl Write) -> Result<Flow> {
        let read = read.map_err(|source| Error::Stream {
            op: StreamOp::Read,
            source,
        })?;
        let answer = match read {
            Line::Ended => {
                self.passes.drain();
                return Ok(Flow::Ended);
            }
            Line::TooLong => Some(refused(
                Value::Null,
                &Unanswered::Refused(Refusal::TooLong {
                    longest: LONGEST_MESSAGE,
                }),
            )),
            Line::Held if line.trim_ascii().is_empty() => None,
            Line::Held => self.answer(line),
        };
        if let Some(answer) = answer {
            written_out(output, &answer)?;
        }
        Ok(Flow::Going)
    }

    fn changed(&self) -> Vec<Value> {
        let mut notifications = Vec::new();
        for (uri, last) in self.subscribed.borrow_mut().iter_mut() {
            let now = self.reading(uri);
            if now != *last {
                *last = now;
                notifications.push(notification(RESOURCE_UPDATED, json!({ "uri": uri })));
            }
        }

        let mut listed = self.listed.borrow_mut();
        if let Some(last) = listed.as_mut() {
            match self.listing() {
                Ok(now) if now != *last => {
                    *last = now;
                    notifications.push(notification(RESOURCE_LIST_CHANGED, json!({})));
                }
                Ok(_) => {}
                Err(error) => tracing::debug!(%error, "the resources could not be listed again"),
            }
        }
        notifications
    }

    fn reading(&self, uri: &str) -> Option<Value> {
        let resource = Resource::at(uri)?;
        resource
            .read(&self.library, &*self.players, &self.passes)
            .inspect_err(|error| tracing::debug!(%uri, %error, "a subscribed resource failed"))
            .ok()
    }

    fn listing(&self) -> Result<Vec<Value>> {
        Ok(resources::every(&self.library)?
            .iter()
            .map(Resource::listed)
            .collect())
    }

    fn subscription(&self, method: &str, params: Value) -> std::result::Result<String, Refusal> {
        if !self.pushing.get() {
            return Err(Refusal::UnknownMethod(MethodName::new(method)));
        }
        let asked: Reading = parameters(method, params)?;
        if Resource::at(&asked.uri).is_none() {
            return Err(Refusal::UnknownResource(ResourceUri::new(asked.uri)));
        }
        Ok(asked.uri)
    }

    pub fn answer(&self, line: &[u8]) -> Option<Value> {
        match serde_json::from_slice(line) {
            Ok(Value::Array(batch)) => self.answer_batch(batch),
            Ok(message) => self.answer_one(message),
            Err(source) => Some(refused(
                Value::Null,
                &Unanswered::Refused(Refusal::Unparsed(source)),
            )),
        }
    }

    fn answer_batch(&self, batch: Vec<Value>) -> Option<Value> {
        if batch.is_empty() {
            return Some(refused(
                Value::Null,
                &Unanswered::Refused(Refusal::EmptyBatch),
            ));
        }
        let answers: Vec<Value> = batch
            .into_iter()
            .filter_map(|message| self.answer_one(message))
            .collect();

        (!answers.is_empty()).then_some(Value::Array(answers))
    }

    fn answer_one(&self, message: Value) -> Option<Value> {
        let (id, method, params) = match read(message) {
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
                Ok(initialised(&asked.protocol_version, self.pushing.get()))
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
                let offered = self.listing()?;
                if self.pushing.get() {
                    *self.listed.borrow_mut() = Some(offered.clone());
                }
                Ok(json!({ "resources": offered }))
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
            RESOURCES_SUBSCRIBE => {
                let uri = self.subscription(method, params)?;
                let now = self.reading(&uri);
                self.subscribed.borrow_mut().insert(uri, now);
                Ok(json!({}))
            }
            RESOURCES_UNSUBSCRIBE => {
                let uri = self.subscription(method, params)?;
                self.subscribed.borrow_mut().remove(&uri);
                Ok(json!({}))
            }
            PROMPTS_LIST => Ok(json!({ "prompts": Prompt::ALL.map(Prompt::listed) })),
            PROMPTS_GET => {
                let asked: Getting = parameters(method, params)?;
                let prompt = Prompt::named(&asked.name)
                    .ok_or_else(|| Refusal::UnknownPrompt(PromptName::new(asked.name)))?;
                let arguments = match asked.arguments {
                    None | Some(Value::Null) => Value::Object(Map::new()),
                    Some(given) => given,
                };
                Ok(prompt.get(arguments, &self.library, &*self.players, &self.passes)??)
            }
            COMPLETION_COMPLETE => {
                let asked: Completing = parameters(method, params)?;
                Ok(asked.complete(&self.library)??)
            }
            other => Err(Refusal::UnknownMethod(MethodName::new(other)).into()),
        }
    }
}

fn tick_into(sender: &SyncSender<Heard>) {
    loop {
        thread::sleep(LOOKED_OVER_EVERY);
        if sender.send(Heard::Tick).is_err() {
            return;
        }
    }
}

fn written_out(output: &mut impl Write, message: &Value) -> Result<()> {
    writeln!(output, "{message}")
        .and_then(|()| output.flush())
        .map_err(|source| Error::Stream {
            op: StreamOp::Write,
            source,
        })
}

fn notification(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": JSON_RPC, "method": method, "params": params })
}

fn read_into(mut input: impl BufRead, sender: &SyncSender<Heard>) {
    loop {
        let mut line = Vec::new();
        let read = next_line(&mut input, &mut line);
        let last = !matches!(read, Ok(Line::Held | Line::TooLong));
        if sender.send(Heard::Read(read, line)).is_err() || last {
            return;
        }
    }
}

#[cfg(fuzzing)]
pub(crate) fn read_every_line(bytes: &[u8]) {
    let mut input = io::Cursor::new(bytes);
    let mut line = Vec::new();
    loop {
        line.clear();
        match next_line(&mut input, &mut line) {
            Ok(Line::Ended) | Err(_) => return,
            Ok(Line::TooLong) => {}
            Ok(Line::Held) => match serde_json::from_slice(&line) {
                Ok(Value::Array(batch)) => batch.into_iter().for_each(|message| {
                    let _ = read(message);
                }),
                Ok(message) => {
                    let _ = read(message);
                }
                Err(_) => {}
            },
        }
    }
}

fn next_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    let read = input
        .by_ref()
        .take(LONGEST_MESSAGE as u64 + 1)
        .read_until(b'\n', line)?;
    if read == 0 {
        return Ok(Line::Ended);
    }
    if line.len() <= LONGEST_MESSAGE || line.ends_with(b"\n") {
        return Ok(Line::Held);
    }
    past_the_rest_of_the_line(input)?;
    Ok(Line::TooLong)
}

fn past_the_rest_of_the_line(input: &mut impl BufRead) -> io::Result<()> {
    loop {
        let (used, ended) = {
            let held = input.fill_buf()?;
            if held.is_empty() {
                return Ok(());
            }
            match held.iter().position(|byte| *byte == b'\n') {
                Some(at) => (at + 1, true),
                None => (held.len(), false),
            }
        };
        input.consume(used);
        if ended {
            return Ok(());
        }
    }
}

fn read(message: Value) -> std::result::Result<Message, (Value, Refusal)> {
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

fn initialised(asked: &str, pushing: bool) -> Value {
    let protocol = PROTOCOLS
        .into_iter()
        .find(|known| *known == asked)
        .unwrap_or(LATEST_PROTOCOL);

    json!({
        "protocolVersion": protocol,
        "capabilities": {
            "tools": { "listChanged": false },
            "resources": { "subscribe": pushing, "listChanged": pushing },
            "prompts": { "listChanged": false },
            "completions": {},
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
