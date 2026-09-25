use std::{
    cell::RefCell,
    env, fs,
    num::NonZeroUsize,
    path::PathBuf,
    process,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, TrackId, Volume};
use resonate_engine::{Asleep, Placement, Until};
use resonate_library::{AlbumQuery, Library, Mbid, Medium, Release, ReleaseTrack, ScanOptions};
use resonate_mcp::{Controlling, Error, Reach, Row, Server, Tool};
use resonate_mpris::{Described, PlaybackStatus, PlaylistInfo, Queueing, Seeking};
use serde_json::{Value, json};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const BITS: u16 = 16;
const FRAMES: u32 = 4_410;
const PLAYER: &str = "org.mpris.MediaPlayer2.resonate";

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-mcp-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn wav(&self, name: &str, title: &str, album: &str, number: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, wav(title, album, number)).expect("a writable temporary file");
        path
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn info(into: &mut Vec<u8>, id: &[u8; 4], value: &str) {
    let mut text = value.as_bytes().to_vec();
    text.push(0);
    chunk(into, id, &text);
}

fn wav(title: &str, album: &str, number: &str) -> Vec<u8> {
    let block_align = CHANNELS * BITS / 8;
    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());

    let mut listed = b"INFO".to_vec();
    info(&mut listed, b"INAM", title);
    info(&mut listed, b"IART", "The Orbiters");
    info(&mut listed, b"IPRD", album);
    info(&mut listed, b"ITRK", number);

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"LIST", &listed);
    chunk(
        &mut body,
        b"data",
        &vec![0; (FRAMES * u32::from(block_align)) as usize],
    );

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

fn scanned(tree: &Tree) -> Library {
    let library = Library::open_in_memory().expect("an in-memory catalog");
    library
        .add_root(&tree.root)
        .expect("the tree to be a root of the catalog");
    library
        .scan(ScanOptions {
            roots: vec![tree.root.clone()],
            incremental: false,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })
        .expect("a scan to start")
        .join()
        .expect("a scan to finish");
    library
}

#[derive(Clone, Debug, PartialEq)]
enum Call {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    Seek(Seeking),
    SetVolume(Volume),
    Queue(Vec<Row>, Queueing),
    Remove(TrackId),
    Activate(PlaylistId),
    SetSleep(Option<Until>),
}

struct Standing {
    rows: Vec<Described>,
    playing: Option<usize>,
    status: Option<PlaybackStatus>,
    position: Duration,
    volume: Volume,
    playlists: Vec<PlaylistInfo>,
    asleep: Option<Asleep>,
    calls: Vec<Call>,
    described: Vec<usize>,
}

impl Default for Standing {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            playing: None,
            status: Some(PlaybackStatus::Stopped),
            position: Duration::from_millis(61_500),
            volume: Volume::MAX,
            playlists: Vec::new(),
            asleep: None,
            calls: Vec::new(),
            described: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
struct Fake {
    running: Option<Rc<RefCell<Standing>>>,
}

impl Fake {
    fn with(standing: Standing) -> (Self, Rc<RefCell<Standing>>) {
        let shared = Rc::new(RefCell::new(standing));
        (
            Self {
                running: Some(Rc::clone(&shared)),
            },
            shared,
        )
    }
}

struct Handle(Rc<RefCell<Standing>>);

impl Handle {
    fn called(&self, call: Call) -> resonate_mpris::Result<()> {
        self.0.borrow_mut().calls.push(call);
        Ok(())
    }
}

impl Reach for Fake {
    fn player(&self) -> resonate_mcp::Result<Box<dyn Controlling>> {
        let running = self.running.clone().ok_or(Error::NothingRunning)?;
        Ok(Box::new(Handle(running)))
    }
}

impl Handle {
    fn stands(&self, status: PlaybackStatus) {
        self.0.borrow_mut().status = Some(status);
    }

    fn moves_by(&self, by: isize) {
        let mut standing = self.0.borrow_mut();
        let len = standing.rows.len();
        standing.playing = standing
            .playing
            .and_then(|row| row.checked_add_signed(by))
            .filter(|row| *row < len);
        standing.position = Duration::ZERO;
    }
}

impl Controlling for Handle {
    fn name(&self) -> String {
        PLAYER.to_owned()
    }

    fn playback(&self) -> resonate_mpris::Result<Option<PlaybackStatus>> {
        Ok(self.0.borrow().status)
    }

    fn play(&self) -> resonate_mpris::Result<()> {
        self.stands(PlaybackStatus::Playing);
        self.called(Call::Play)
    }

    fn pause(&self) -> resonate_mpris::Result<()> {
        self.stands(PlaybackStatus::Paused);
        self.called(Call::Pause)
    }

    fn play_pause(&self) -> resonate_mpris::Result<()> {
        let playing = self.0.borrow().status == Some(PlaybackStatus::Playing);
        self.stands(if playing {
            PlaybackStatus::Paused
        } else {
            PlaybackStatus::Playing
        });
        self.called(Call::PlayPause)
    }

    fn stop(&self) -> resonate_mpris::Result<()> {
        self.stands(PlaybackStatus::Stopped);
        self.called(Call::Stop)
    }

    fn next(&self) -> resonate_mpris::Result<()> {
        self.moves_by(1);
        self.called(Call::Next)
    }

    fn previous(&self) -> resonate_mpris::Result<()> {
        self.moves_by(-1);
        self.called(Call::Previous)
    }

    fn seek(&self, by: Seeking) -> resonate_mpris::Result<()> {
        {
            let mut standing = self.0.borrow_mut();
            standing.position = match by {
                Seeking::Forward(span) => standing.position + span,
                Seeking::Backward(span) => standing.position.saturating_sub(span),
            };
        }
        self.called(Call::Seek(by))
    }

    fn volume(&self) -> resonate_mpris::Result<Volume> {
        Ok(self.0.borrow().volume)
    }

    fn set_volume(&self, volume: Volume) -> resonate_mpris::Result<()> {
        self.0.borrow_mut().volume = volume;
        self.called(Call::SetVolume(volume))
    }

    fn metadata(&self) -> resonate_mpris::Result<Option<Described>> {
        let standing = self.0.borrow();
        Ok(standing.playing.map(|row| standing.rows[row].clone()))
    }

    fn position(&self) -> resonate_mpris::Result<Duration> {
        Ok(self.0.borrow().position)
    }

    fn queued(&self) -> resonate_mpris::Result<Vec<TrackId>> {
        Ok(self.0.borrow().rows.iter().map(|row| row.track).collect())
    }

    fn described(&self, rows: &[TrackId]) -> resonate_mpris::Result<Vec<Described>> {
        let mut standing = self.0.borrow_mut();
        standing.described.push(rows.len());
        Ok(rows
            .iter()
            .filter_map(|id| standing.rows.iter().find(|row| row.track == *id).cloned())
            .collect())
    }

    fn queue(&self, rows: &[Row], queueing: Queueing) -> resonate_mpris::Result<usize> {
        {
            let mut standing = self.0.borrow_mut();
            let first = standing.rows.len() as u64 + 1_000;
            let minted: Vec<Described> = (first..)
                .zip(rows)
                .map(|(id, _)| row(id, "Queued"))
                .collect();
            standing.rows.extend(minted);
        }
        self.called(Call::Queue(rows.to_vec(), queueing))?;
        Ok(rows.len())
    }

    fn remove_track(&self, track: TrackId) -> resonate_mpris::Result<()> {
        self.0.borrow_mut().rows.retain(|row| row.track != track);
        self.called(Call::Remove(track))
    }

    fn playlists(&self) -> resonate_mpris::Result<Vec<PlaylistInfo>> {
        Ok(self.0.borrow().playlists.clone())
    }

    fn activate_playlist(&self, playlist: PlaylistId) -> resonate_mpris::Result<()> {
        self.called(Call::Activate(playlist))
    }

    fn sleep(&self) -> resonate_mpris::Result<Option<Asleep>> {
        Ok(self.0.borrow().asleep)
    }

    fn set_sleep(&self, until: Option<Until>) -> resonate_mpris::Result<()> {
        self.0.borrow_mut().asleep = until.map(|until| Asleep {
            until,
            left: match until {
                Until::After(left) => Some(left),
                Until::EndOfTrack | Until::EndOfQueue => None,
            },
        });
        self.called(Call::SetSleep(until))
    }
}

fn track_id(raw: u64) -> TrackId {
    TrackId::new(raw).expect("a track id is not zero")
}

fn row(raw: u64, title: &str) -> Described {
    Described {
        track: track_id(raw),
        location: Some(MediaLocation::local(format!("/music/{title}.flac"))),
        span: None,
        title: Some(title.to_owned()),
        artist: Some("The Orbiters".to_owned()),
        album: Some("Hours".to_owned()),
        length: Some(Duration::from_secs(240)),
        art: None,
    }
}

fn server(library: Library, players: Fake) -> Server {
    Server::new(library, players)
}

fn nothing_running() -> Server {
    server(
        Library::open_in_memory().expect("an in-memory catalog"),
        Fake::default(),
    )
}

fn asked(server: &Server, message: &Value) -> Option<Value> {
    server.answer(message.to_string().as_bytes())
}

fn request(method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": 7, "method": method, "params": params })
}

fn result(server: &Server, method: &str, params: Value) -> Value {
    let answer = asked(server, &request(method, params)).expect("a request to be answered");
    assert_eq!(answer["jsonrpc"], "2.0");
    assert_eq!(answer["id"], 7);
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

fn error_code(server: &Server, message: &Value) -> i64 {
    let answer = asked(server, message).expect("a refusal to be answered");
    assert!(answer.get("result").is_none(), "{answer}");
    answer["error"]["code"]
        .as_i64()
        .expect("an error carries a numeric code")
}

fn called(server: &Server, tool: &str, arguments: Value) -> Value {
    let answered = result(
        server,
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
    );
    assert_eq!(answered["isError"], false, "{answered}");
    let structured = answered["structuredContent"].clone();
    let text = answered["content"][0]["text"]
        .as_str()
        .expect("a result carries its text");
    assert_eq!(
        serde_json::from_str::<Value>(text).expect("the text to be the JSON it stands for"),
        structured
    );
    structured
}

fn failed(server: &Server, tool: &str, arguments: Value) -> String {
    let answered = result(
        server,
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
    );
    assert_eq!(answered["isError"], true, "{answered}");
    answered["content"][0]["text"]
        .as_str()
        .expect("a failure says what failed")
        .to_owned()
}

#[test]
fn initialising_answers_the_version_asked_for_where_it_is_one_this_server_speaks() {
    let server = nothing_running();
    let known = result(
        &server,
        "initialize",
        json!({ "protocolVersion": "2025-03-26", "capabilities": {} }),
    );
    let unknown = result(
        &server,
        "initialize",
        json!({ "protocolVersion": "1999-01-01", "capabilities": {} }),
    );

    assert_eq!(known["protocolVersion"], "2025-03-26");
    assert_eq!(unknown["protocolVersion"], "2025-06-18");
    assert_eq!(known["serverInfo"]["name"], "resonate");
    assert_eq!(known["capabilities"]["tools"]["listChanged"], false);
    assert_eq!(known["capabilities"]["resources"]["subscribe"], false);
}

#[test]
fn a_notification_is_never_answered() {
    let server = nothing_running();

    assert_eq!(
        asked(
            &server,
            &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        ),
        None
    );
    assert_eq!(
        asked(
            &server,
            &json!({ "jsonrpc": "2.0", "method": "notifications/cancelled", "params": {} })
        ),
        None
    );
    assert_eq!(
        asked(
            &server,
            &json!({ "jsonrpc": "2.0", "method": "tools/call", "params": {} })
        ),
        None
    );
}

#[test]
fn an_answer_to_a_request_this_server_never_made_is_passed_over() {
    let server = nothing_running();

    assert_eq!(
        asked(&server, &json!({ "jsonrpc": "2.0", "id": 3, "result": {} })),
        None
    );
}

#[test]
fn a_broken_envelope_is_a_json_rpc_error_rather_than_a_tool_failure() {
    let server = nothing_running();

    let unparsed = server
        .answer(b"{ not json")
        .expect("a parse error to be answered");
    assert_eq!(unparsed["error"]["code"], -32_700);
    assert_eq!(unparsed["id"], Value::Null);

    assert_eq!(
        error_code(&server, &json!({ "id": 1, "method": "ping" })),
        -32_600
    );
    assert_eq!(error_code(&server, &json!([1, 2])), -32_600);
    assert_eq!(
        error_code(
            &server,
            &json!({ "jsonrpc": "2.0", "id": null, "method": "ping" })
        ),
        -32_600
    );
    assert_eq!(
        error_code(&server, &request("completion/complete", json!({}))),
        -32_601
    );
    assert_eq!(
        error_code(&server, &request("initialize", json!({}))),
        -32_602
    );
    assert_eq!(
        error_code(
            &server,
            &request("tools/call", json!({ "name": "launch_rockets" }))
        ),
        -32_602
    );
    assert_eq!(
        error_code(
            &server,
            &request(
                "tools/call",
                json!({ "name": "seek", "arguments": { "seconds": "soon" } })
            )
        ),
        -32_602
    );
    assert_eq!(
        error_code(
            &server,
            &request(
                "tools/call",
                json!({ "name": "set_volume", "arguments": { "percent": 140 } })
            )
        ),
        -32_602
    );
    assert_eq!(
        error_code(
            &server,
            &request(
                "tools/call",
                json!({ "name": "add_to_queue", "arguments": { "track_ids": [1], "query": "x" } })
            )
        ),
        -32_602
    );
    assert_eq!(
        error_code(
            &server,
            &request(
                "tools/call",
                json!({ "name": "set_sleep_timer", "arguments": {} })
            )
        ),
        -32_602
    );
}

#[test]
fn a_refusal_answers_under_the_id_it_was_asked_under() {
    let server = nothing_running();
    let answer = asked(
        &server,
        &json!({ "jsonrpc": "2.0", "id": "a-string", "method": "nope" }),
    )
    .expect("an unknown method to be answered");

    assert_eq!(answer["id"], "a-string");
    assert_eq!(answer["error"]["code"], -32_601);
}

#[test]
fn every_tool_is_listed_with_a_schema_naming_what_it_requires() {
    let server = nothing_running();
    let listed = result(&server, "tools/list", json!({}));
    let tools = listed["tools"].as_array().expect("a list of tools");

    assert_eq!(tools.len(), Tool::ALL.len());
    for (tool, entry) in Tool::ALL.iter().zip(tools) {
        assert_eq!(entry["name"], tool.name());
        assert_eq!(Tool::named(tool.name()), Some(*tool));
        assert_eq!(entry["inputSchema"]["type"], "object");
        assert_eq!(entry["annotations"]["readOnlyHint"], tool.reads_only());
        assert_eq!(entry["annotations"]["destructiveHint"], tool.destroys());
        assert!(!(tool.reads_only() && tool.destroys()), "{}", tool.name());
        assert!(
            entry["description"]
                .as_str()
                .is_some_and(|said| !said.is_empty())
        );
        for required in entry["inputSchema"]["required"]
            .as_array()
            .expect("a list of what is required")
        {
            let field = required.as_str().expect("a field's name");
            assert!(
                entry["inputSchema"]["properties"].get(field).is_some(),
                "{} requires {field} and does not describe it",
                tool.name()
            );
        }
    }
}

#[test]
fn a_ping_is_answered_with_nothing() {
    assert_eq!(result(&nothing_running(), "ping", json!({})), json!({}));
}

#[test]
fn a_transport_tool_with_no_player_fails_as_a_tool_and_the_catalog_still_answers() {
    let server = nothing_running();

    let said = failed(&server, "now_playing", json!({}));
    assert!(said.contains("no player"), "{said}");
    assert!(
        failed(&server, "control_playback", json!({ "action": "pause" })).contains("no player")
    );

    let listed = called(&server, "list_playlists", json!({}));
    assert_eq!(listed, json!({ "playlists": [] }));
}

#[test]
fn what_is_playing_is_read_off_the_player() {
    let (players, _) = Fake::with(Standing {
        rows: vec![row(9, "Echoes"), row(8, "Time")],
        playing: Some(1),
        status: Some(PlaybackStatus::Playing),
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let playing = called(&server, "now_playing", json!({}));

    assert_eq!(playing["player"], PLAYER);
    assert_eq!(playing["status"], "Playing");
    assert_eq!(playing["track"]["title"], "Time");
    assert_eq!(playing["track"]["queue_id"], "8");
    assert_eq!(playing["track"]["uri"], "file:///music/Time.flac");
    assert_eq!(playing["position_seconds"], 61.5);
    assert_eq!(playing["volume_percent"], 100.0);
    assert!(playing.get("sleep_timer").is_none());
}

#[test]
fn a_queue_id_is_written_as_text_so_no_client_rounds_it() {
    let (players, standing) = Fake::with(Standing {
        rows: vec![row(u64::MAX, "Echoes"), row(u64::MAX - 1, "Time")],
        playing: Some(0),
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let queue = called(&server, "show_queue", json!({}));
    assert_eq!(queue["rows"], 2);
    assert_eq!(queue["playing_row"], 0);
    assert_eq!(queue["queue"][0]["queue_id"], u64::MAX.to_string());
    assert_eq!(queue["queue"][0]["playing"], true);
    assert!(queue["queue"][1].get("playing").is_none());

    let removed = called(
        &server,
        "remove_from_queue",
        json!({ "queue_id": (u64::MAX - 1).to_string() }),
    );
    assert_eq!(removed["removed"], (u64::MAX - 1).to_string());
    assert_eq!(
        standing.borrow().calls,
        vec![Call::Remove(track_id(u64::MAX - 1))]
    );

    assert!(
        failed(&server, "remove_from_queue", json!({ "queue_id": "12" }))
            .contains("no row numbered 12")
    );
}

#[test]
fn every_transport_gesture_reaches_the_player_as_the_call_it_names() {
    let (players, standing) = Fake::with(Standing::default());
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    for action in ["play", "pause", "toggle", "stop", "next", "previous"] {
        let done = called(&server, "control_playback", json!({ "action": action }));
        assert_eq!(done["done"], action);
    }
    called(&server, "seek", json!({ "seconds": -12.5 }));
    called(&server, "seek", json!({ "seconds": 30 }));
    called(&server, "set_volume", json!({ "percent": 50 }));

    assert_eq!(
        standing.borrow().calls,
        vec![
            Call::Play,
            Call::Pause,
            Call::PlayPause,
            Call::Stop,
            Call::Next,
            Call::Previous,
            Call::Seek(Seeking::Backward(Duration::from_millis(12_500))),
            Call::Seek(Seeking::Forward(Duration::from_secs(30))),
            Call::SetVolume(Volume::new(0.5).expect("half is a volume")),
        ]
    );
}

#[test]
fn a_sleep_timer_is_set_in_minutes_or_at_an_end_and_read_back() {
    let (players, standing) = Fake::with(Standing::default());
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let after = called(&server, "set_sleep_timer", json!({ "minutes": 20 }));
    assert_eq!(after["sleep_timer"]["until"], "after_a_while");
    assert_eq!(after["sleep_timer"]["left_seconds"], 1_200.0);

    let at_the_end = called(&server, "set_sleep_timer", json!({ "at": "end_of_queue" }));
    assert_eq!(
        at_the_end["sleep_timer"],
        json!({ "until": "end_of_queue" })
    );

    let off = called(&server, "set_sleep_timer", json!({ "at": "off" }));
    assert!(off.get("sleep_timer").is_none());

    assert_eq!(
        standing.borrow().calls,
        vec![
            Call::SetSleep(Some(Until::After(Duration::from_secs(1_200)))),
            Call::SetSleep(Some(Until::EndOfQueue)),
            Call::SetSleep(None),
        ]
    );
}

#[test]
fn a_playlist_is_played_by_the_name_the_player_offers_it_under() {
    let chill = PlaylistId::new(4).expect("a playlist id is not zero");
    let (players, standing) = Fake::with(Standing {
        playlists: vec![PlaylistInfo {
            id: chill,
            name: "Late Night".to_owned(),
        }],
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let playing = called(
        &server,
        "play_playlist",
        json!({ "playlist": "late night" }),
    );

    assert_eq!(playing["playing"], "Late Night");
    assert_eq!(standing.borrow().calls, vec![Call::Activate(chill)]);
    assert!(
        failed(&server, "play_playlist", json!({ "playlist": "Morning" }))
            .contains("no playlist named Morning")
    );
}

#[test]
fn the_catalog_is_searched_and_what_it_finds_is_queued_by_its_track_ids() {
    let tree = Tree::new();
    let second = tree.wav("b.wav", "Signal Fire", "Hours", "2");
    let first = tree.wav("a.wav", "Night Signal", "Hours", "1");
    tree.wav("c.wav", "Elsewhere", "Days", "1");
    let (players, standing) = Fake::with(Standing::default());
    let server = server(scanned(&tree), players);

    let found = called(&server, "search_library", json!({ "query": "signal" }));
    let tracks = found["tracks"].as_array().expect("a list of tracks");
    assert_eq!(tracks.len(), 2);
    assert!(tracks.iter().all(|track| track["album"] == "Hours"));
    assert_eq!(found["albums"][0]["title"], "Hours");
    assert_eq!(found["albums"][0]["tracks"], 2);

    let ids: Vec<u64> = tracks
        .iter()
        .map(|track| track["track_id"].as_u64().expect("a numeric track id"))
        .collect();
    let queued = called(
        &server,
        "add_to_queue",
        json!({ "track_ids": ids, "next": true, "play": true }),
    );
    assert_eq!(queued["queued"], 2);

    let by_album = called(&server, "add_to_queue", json!({ "query": "album:Hours" }));
    assert_eq!(by_album["queued"], 2);

    let calls = standing.borrow().calls.clone();
    let Call::Queue(rows, queueing) = &calls[0] else {
        panic!("the ids were not queued: {calls:?}");
    };
    assert_eq!(rows.len(), 2);
    assert_eq!(
        *queueing,
        Queueing {
            at: Placement::Next,
            play: true
        }
    );
    assert_eq!(
        calls[1],
        Call::Queue(
            vec![
                (MediaLocation::local(first), None::<FrameSpan>),
                (MediaLocation::local(second), None),
            ],
            Queueing {
                at: Placement::Last,
                play: false
            }
        )
    );

    assert!(
        failed(&server, "add_to_queue", json!({ "track_ids": [999] }))
            .contains("no track numbered 999")
    );
}

#[test]
fn a_playlists_rows_are_listed_from_the_catalog_with_no_player() {
    let tree = Tree::new();
    let path = tree.wav("a.wav", "Night Signal", "Hours", "1");
    let library = scanned(&tree);
    let id = library
        .create_playlist("Late Night")
        .expect("a playlist to be made");
    let track = library
        .tracks(&Default::default())
        .expect("the catalog to be read")
        .remove(0);
    library
        .add_to_playlist(id, &[resonate_library::Cut::of(&track)])
        .expect("a row to be added");
    let server = server(library, Fake::default());

    let listed = called(&server, "list_playlists", json!({}));
    assert_eq!(listed["playlists"][0]["name"], "Late Night");
    assert_eq!(listed["playlists"][0]["tracks"], 1);

    let rows = called(
        &server,
        "playlist_tracks",
        json!({ "playlist": "late night" }),
    );
    assert_eq!(rows["matched"], 1);
    assert_eq!(rows["tracks"][0]["title"], "Night Signal");
    assert_eq!(
        rows["tracks"][0]["uri"],
        MediaLocation::local(path).to_uri()
    );

    assert!(
        failed(&server, "playlist_tracks", json!({ "playlist": "Morning" }))
            .contains("no playlist named Morning")
    );
}

#[test]
fn a_transport_gesture_answers_with_what_the_player_reads_back_once_it_has_landed() {
    let (players, _) = Fake::with(Standing {
        rows: vec![row(9, "Echoes"), row(8, "Time"), row(7, "Money")],
        playing: Some(0),
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let played = called(&server, "control_playback", json!({ "action": "play" }));
    assert_eq!(played["status"], "Playing");
    assert_eq!(played["track"]["title"], "Echoes");

    let skipped = called(&server, "control_playback", json!({ "action": "next" }));
    assert_eq!(skipped["track"]["title"], "Time");
    assert_eq!(skipped["track"]["queue_id"], "8");

    let paused = called(&server, "control_playback", json!({ "action": "toggle" }));
    assert_eq!(paused["status"], "Paused");

    let sought = called(&server, "seek", json!({ "seconds": 30 }));
    assert_eq!(sought["moved_seconds"], 30.0);
    assert_eq!(sought["position_seconds"], 30.0);
    assert_eq!(sought["track"]["title"], "Time");

    let louder = called(&server, "set_volume", json!({ "percent": 25 }));
    assert_eq!(louder["volume_percent"], 25.0);
}

#[test]
fn a_queue_is_described_only_as_far_as_it_is_listed() {
    let (players, standing) = Fake::with(Standing {
        rows: (1..=40).map(|id| row(id, "Row")).collect(),
        playing: Some(30),
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    let queue = called(&server, "show_queue", json!({ "limit": 3 }));

    assert_eq!(queue["rows"], 40);
    assert_eq!(queue["playing_row"], 30);
    assert_eq!(queue["queue"].as_array().map(Vec::len), Some(3));
    assert_eq!(standing.borrow().described, vec![3]);
}

#[test]
fn a_favourite_is_marked_and_taken_away_by_its_catalog_id() {
    let tree = Tree::new();
    tree.wav("a.wav", "Night Signal", "Hours", "1");
    let server = server(scanned(&tree), Fake::default());
    let found = called(&server, "search_library", json!({ "query": "signal" }));
    let track = found["tracks"][0]["track_id"].clone();
    let album = found["albums"][0]["album_id"].clone();

    let marked = called(
        &server,
        "mark_favourite",
        json!({ "track_ids": [track], "album_ids": [album] }),
    );
    assert_eq!(
        marked,
        json!({ "favourite": true, "named": 2, "changed": 2 })
    );

    let favourites = called(&server, "favourites", json!({}));
    assert_eq!(favourites["tracks"][0]["title"], "Night Signal");
    assert_eq!(favourites["albums"][0]["title"], "Hours");

    let again = called(&server, "mark_favourite", json!({ "track_ids": [track] }));
    assert_eq!(again["changed"], 0);

    let unmarked = called(
        &server,
        "mark_favourite",
        json!({ "track_ids": [track], "favourite": false }),
    );
    assert_eq!(unmarked["changed"], 1);
    assert_eq!(
        called(&server, "favourites", json!({}))["tracks"],
        json!([])
    );

    let nothing = request(
        "tools/call",
        json!({ "name": "mark_favourite", "arguments": {} }),
    );
    assert_eq!(error_code(&server, &nothing), -32_602);
}

#[test]
fn a_playlist_is_made_filled_trimmed_and_renamed_in_the_catalog() {
    let tree = Tree::new();
    tree.wav("a.wav", "Night Signal", "Hours", "1");
    tree.wav("b.wav", "Signal Fire", "Hours", "2");
    tree.wav("c.wav", "Elsewhere", "Days", "1");
    let server = server(scanned(&tree), Fake::default());

    let made = called(&server, "create_playlist", json!({ "name": "Late Night" }));
    assert_eq!(made["playlist"]["name"], "Late Night");
    assert_eq!(made["playlist"]["tracks"], 0);

    let added = called(
        &server,
        "add_to_playlist",
        json!({ "playlist": "late night", "query": "album:Hours" }),
    );
    assert_eq!(added["added"], 2);
    assert_eq!(added["playlist"]["tracks"], 2);

    let elsewhere = called(&server, "search_library", json!({ "query": "elsewhere" }));
    called(
        &server,
        "add_to_playlist",
        json!({
            "playlist": "Late Night",
            "track_ids": [elsewhere["tracks"][0]["track_id"]],
        }),
    );

    let rows = called(
        &server,
        "playlist_tracks",
        json!({ "playlist": "Late Night" }),
    );
    let listed: Vec<(Value, Value)> = rows["tracks"]
        .as_array()
        .expect("a list of rows")
        .iter()
        .map(|row| (row["row"].clone(), row["title"].clone()))
        .collect();
    assert_eq!(
        listed,
        vec![
            (json!(0), json!("Night Signal")),
            (json!(1), json!("Signal Fire")),
            (json!(2), json!("Elsewhere")),
        ]
    );

    let dropped = called(
        &server,
        "remove_from_playlist",
        json!({ "playlist": "Late Night", "row": 0, "through_row": 1 }),
    );
    assert_eq!(dropped["removed"], 2);
    assert_eq!(dropped["playlist"]["tracks"], 1);
    assert!(
        failed(
            &server,
            "remove_from_playlist",
            json!({ "playlist": "Late Night", "row": 5 })
        )
        .contains("holds no row 5")
    );
    let matched = called(
        &server,
        "remove_from_playlist",
        json!({ "playlist": "Late Night", "matching": "elsewhere" }),
    );
    assert_eq!(matched["removed"], 1);

    let renamed = called(
        &server,
        "rename_playlist",
        json!({ "playlist": "Late Night", "to": "Small Hours" }),
    );
    assert_eq!(renamed["playlist"]["name"], "Small Hours");

    let filling = called(
        &server,
        "create_playlist",
        json!({ "name": "Signals", "fills_from": "signal" }),
    );
    assert_eq!(filling["playlist"]["fills_from"], "signal");
    assert_eq!(filling["playlist"]["tracks"], 2);
    assert!(
        failed(
            &server,
            "add_to_playlist",
            json!({ "playlist": "Signals", "query": "elsewhere" })
        )
        .contains("fills itself")
    );

    let both = request(
        "tools/call",
        json!({
            "name": "create_playlist",
            "arguments": { "name": "Both", "query": "signal", "fills_from": "signal" },
        }),
    );
    assert_eq!(error_code(&server, &both), -32_602);
    assert!(failed(&server, "create_playlist", json!({ "name": "Signals" })).contains("Signals"));

    let discarded = called(
        &server,
        "discard_playlist",
        json!({ "playlist": "Small Hours" }),
    );
    assert_eq!(discarded["discarded"]["name"], "Small Hours");
    let left = called(&server, "list_playlists", json!({}));
    let names: Vec<&Value> = left["playlists"]
        .as_array()
        .expect("a list of playlists")
        .iter()
        .map(|playlist| &playlist["name"])
        .collect();
    assert_eq!(names, vec![&json!("Signals")]);
    assert!(
        failed(
            &server,
            "discard_playlist",
            json!({ "playlist": "Small Hours" })
        )
        .contains("Small Hours")
    );
}

#[test]
fn a_track_an_album_is_short_of_is_listed_and_wanted() {
    let tree = Tree::new();
    tree.wav("a.wav", "Night Signal", "Hours", "1");
    tree.wav("b.wav", "Signal Fire", "Hours", "2");
    let library = scanned(&tree);
    let album = library
        .albums(&AlbumQuery::default())
        .expect("the catalog to be read")
        .remove(0);
    let rows = ["Night Signal", "Signal Fire", "Afterglow"]
        .into_iter()
        .zip(1..)
        .map(|(title, position)| ReleaseTrack {
            position,
            number: position.to_string(),
            title: title.to_owned(),
            artist: None,
            recording: None,
            track: None,
            length: Some(Duration::from_secs(200)),
            isrc: None,
            links: Vec::new(),
        })
        .collect();
    library
        .land_release(
            album.id,
            &Release {
                id: Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("an mbid"),
                group: None,
                title: "Hours".to_owned(),
                credit: Vec::new(),
                date: None,
                country: None,
                label: None,
                catalog_number: None,
                barcode: None,
                kind: None,
                disambiguation: None,
                has_front_cover: false,
                links: Vec::new(),
                media: vec![Medium {
                    position: 1,
                    format: None,
                    title: None,
                    tracks: rows,
                }],
            },
        )
        .expect("the release to land");
    library.rematch(album.id).expect("the rows to be matched");
    let server = server(library, Fake::default());

    let missing = called(&server, "list_missing", json!({}));
    assert_eq!(missing["missing"], 1);
    let afterglow = &missing["tracks"][0];
    assert_eq!(afterglow["title"], "Afterglow");
    assert_eq!(afterglow["album"], "Hours");
    assert_eq!(afterglow["number"], "3");
    assert_eq!(afterglow["wanted"], false);

    let wanted = called(
        &server,
        "want_tracks",
        json!({ "release_track_ids": [afterglow["release_track_id"]] }),
    );
    assert_eq!(wanted["wanted"], 1);
    assert_eq!(
        called(&server, "list_missing", json!({}))["tracks"][0]["wanted"],
        true
    );
    assert!(
        called(&server, "list_missing", json!({ "query": "elsewhere" }))["tracks"]
            .as_array()
            .is_some_and(Vec::is_empty)
    );
    assert!(
        !failed(
            &server,
            "want_tracks",
            json!({ "release_track_ids": [999] })
        )
        .is_empty()
    );
}

#[test]
fn serving_answers_a_line_per_request_and_ends_with_its_input() {
    let server = nothing_running();
    let input = [
        request("ping", json!({})).to_string(),
        String::new(),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }).to_string(),
        "   ".to_owned(),
        request("tools/list", json!({})).to_string(),
    ]
    .join("\n");
    let mut output = Vec::new();

    server
        .serve(input.as_bytes(), &mut output)
        .expect("serving to end with its input");

    let lines: Vec<Value> = String::from_utf8(output)
        .expect("the output to be text")
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line to be one message"))
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"], json!({}));
    assert!(lines[1]["result"]["tools"].is_array());
}

#[test]
fn a_line_that_is_not_text_is_refused_rather_than_ending_the_session() {
    let server = nothing_running();
    let mut input = vec![0xff, 0xfe, b'\n'];
    input.extend_from_slice(request("ping", json!({})).to_string().as_bytes());
    let mut output = Vec::new();

    server
        .serve(input.as_slice(), &mut output)
        .expect("a bad line not to end the session");

    let text = String::from_utf8(output).expect("the output to be text");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("-32700"));
    assert!(lines[1].contains("\"result\":{}"));
}

fn once_settled(server: &Server, pass: &str) -> Value {
    for _ in 0..600 {
        let states = called(server, "library_passes", json!({}));
        if states[pass]["state"] != "running" {
            return states[pass].clone();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the {pass} never settled");
}

#[test]
fn a_scan_started_over_the_protocol_runs_behind_the_session_and_says_what_it_found() {
    let tree = Tree::new();
    tree.wav("01.wav", "Signal", "Hours", "1");
    tree.wav("02.wav", "Noise", "Hours", "2");
    let server = nothing_running();

    let idle = called(&server, "library_passes", json!({}));
    for pass in ["scan", "lookup", "poll"] {
        assert_eq!(idle[pass]["state"], "idle", "{idle}");
    }

    let started = called(
        &server,
        "start_scan",
        json!({ "roots": [tree.root.display().to_string()] }),
    );
    assert_eq!(started["started"], "scan");
    assert_eq!(started["roots"], json!([tree.root.display().to_string()]));

    let settled = once_settled(&server, "scan");
    assert_eq!(settled["state"], "finished", "{settled}");
    assert_eq!(settled["stats"]["added"], 2, "{settled}");
    assert_eq!(settled["cancelled"], false);

    let found = called(&server, "search_library", json!({ "query": "noise" }));
    assert_eq!(found["tracks"].as_array().map(Vec::len), Some(1), "{found}");

    let again = called(&server, "start_scan", json!({}));
    assert_eq!(again["roots"], json!([tree.root.display().to_string()]));
    assert_eq!(once_settled(&server, "scan")["stats"]["added"], 0);
}

#[test]
fn a_folder_that_is_not_there_is_refused_rather_than_kept_as_a_root() {
    let server = nothing_running();

    let said = failed(
        &server,
        "start_scan",
        json!({ "roots": ["/nowhere/resonate/music"] }),
    );

    assert!(said.contains("not a folder"), "{said}");
    let idle = called(&server, "library_passes", json!({}));
    assert_eq!(idle["scan"]["state"], "idle");
}

#[test]
fn a_lookup_asked_of_a_build_that_reaches_nothing_is_that_tool_failing_alone() {
    let server = nothing_running();

    let said = failed(&server, "start_lookup", json!({}));

    assert!(said.contains("look anything up"), "{said}");
    assert_eq!(
        called(&server, "library_passes", json!({}))["lookup"]["state"],
        "idle"
    );
}

#[test]
fn a_poll_with_no_want_to_ask_about_finishes_having_asked_nothing() {
    let server = nothing_running();

    let started = called(&server, "start_poll", json!({ "again": true }));
    assert_eq!(started["started"], "poll");
    assert_eq!(started["a_provider_is_registered"], false);

    let settled = once_settled(&server, "poll");
    assert_eq!(settled["state"], "finished", "{settled}");
    assert_eq!(settled["stats"]["asked"], 0);
}

#[test]
fn stopping_a_pass_that_is_not_running_answers_where_it_stands() {
    let server = nothing_running();

    assert_eq!(
        called(&server, "stop_pass", json!({ "pass": "lookup" }))["state"],
        "idle"
    );
    assert_eq!(
        error_code(
            &server,
            &request(
                "tools/call",
                json!({ "name": "stop_pass", "arguments": { "pass": "everything" } })
            )
        ),
        -32_602
    );
}

#[test]
fn a_session_that_ends_under_a_running_scan_waits_for_it_to_stop() {
    let tree = Tree::new();
    for number in 1..=40 {
        tree.wav(
            &format!("{number:02}.wav"),
            &format!("Take {number}"),
            "Hours",
            &number.to_string(),
        );
    }
    let library = Library::open_in_memory().expect("an in-memory catalog");
    let server = server(library, Fake::default());
    let started = request(
        "tools/call",
        json!({
            "name": "start_scan",
            "arguments": { "roots": [tree.root.display().to_string()] },
        }),
    );
    let input = format!("{started}\n");
    let mut output = Vec::new();

    server
        .serve(input.as_bytes(), &mut output)
        .expect("the session to end cleanly");

    let settled = called(&server, "library_passes", json!({}));
    assert_eq!(settled["scan"]["state"], "finished", "{settled}");
}

fn read_resource(server: &Server, uri: &str) -> Value {
    let answered = result(server, "resources/read", json!({ "uri": uri }));
    let contents = &answered["contents"][0];
    assert_eq!(contents["uri"], uri, "{answered}");
    assert_eq!(contents["mimeType"], "application/json");
    serde_json::from_str(
        contents["text"]
            .as_str()
            .expect("a resource carries its text"),
    )
    .expect("the text to be JSON")
}

#[test]
fn every_resource_is_listed_and_each_playlist_as_one_of_its_own() {
    let tree = Tree::new();
    tree.wav("a.wav", "Night Signal", "Hours", "1");
    let library = scanned(&tree);
    let id = library
        .create_playlist("AC/DC & Friends?")
        .expect("a playlist to be made");
    let track = library
        .tracks(&Default::default())
        .expect("the catalog to be read")
        .remove(0);
    library
        .add_to_playlist(id, &[resonate_library::Cut::of(&track)])
        .expect("a row to be added");
    let server = server(library, Fake::default());

    let listed = result(&server, "resources/list", json!({}));
    let uris: Vec<&str> = listed["resources"]
        .as_array()
        .expect("a list of resources")
        .iter()
        .map(|resource| resource["uri"].as_str().expect("a resource has a uri"))
        .collect();
    assert_eq!(
        uris,
        [
            "resonate://player/now-playing",
            "resonate://player/queue",
            "resonate://library/passes",
            "resonate://library/playlists",
            "resonate://library/favourites",
            "resonate://library/statistics",
            "resonate://library/suggestions",
            "resonate://library/missing",
            "resonate://library/playlist/AC/DC%20%26%20Friends%3F",
        ]
    );
    assert_eq!(listed["resources"][8]["name"], "AC/DC & Friends?");

    let rows = read_resource(&server, uris[8]);
    assert_eq!(rows["playlist"]["name"], "AC/DC & Friends?");
    assert_eq!(rows["tracks"][0]["title"], "Night Signal");
    assert_eq!(
        read_resource(
            &server,
            "resonate://library/playlist/ac%2Fdc%20%26%20friends%3F"
        ),
        rows
    );

    let templates = result(&server, "resources/templates/list", json!({}));
    assert_eq!(
        templates["resourceTemplates"][0]["uriTemplate"],
        "resonate://library/playlist/{name}"
    );
}

#[test]
fn a_resource_reads_what_the_tool_of_the_same_reading_answers() {
    let (players, _) = Fake::with(Standing {
        rows: vec![row(9, "Echoes"), row(8, "Time")],
        playing: Some(0),
        status: Some(PlaybackStatus::Paused),
        ..Standing::default()
    });
    let server = server(
        Library::open_in_memory().expect("an in-memory catalog"),
        players,
    );

    for (uri, tool) in [
        ("resonate://player/now-playing", "now_playing"),
        ("resonate://player/queue", "show_queue"),
        ("resonate://library/passes", "library_passes"),
        ("resonate://library/playlists", "list_playlists"),
        ("resonate://library/favourites", "favourites"),
        ("resonate://library/statistics", "listening_statistics"),
        ("resonate://library/suggestions", "suggested_playlists"),
        ("resonate://library/missing", "list_missing"),
    ] {
        assert_eq!(
            read_resource(&server, uri),
            called(&server, tool, json!({})),
            "{uri}"
        );
    }
}

#[test]
fn a_resource_nobody_offers_is_refused_and_one_that_cannot_be_read_fails() {
    let server = nothing_running();

    for uri in [
        "resonate://player/elsewhere",
        "resonate://library/playlist/",
        "resonate://library/playlist/%zz",
        "file:///music/Time.flac",
    ] {
        assert_eq!(
            error_code(&server, &request("resources/read", json!({ "uri": uri }))),
            -32_002,
            "{uri}"
        );
    }
    assert_eq!(
        error_code(&server, &request("resources/read", json!({}))),
        -32_602
    );

    let answer = asked(
        &server,
        &request(
            "resources/read",
            json!({ "uri": "resonate://player/now-playing" }),
        ),
    )
    .expect("a failed read to be answered");
    assert_eq!(answer["error"]["code"], -32_603, "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|said| said.contains("no player")),
        "{answer}"
    );

    let absent = asked(
        &server,
        &request(
            "resources/read",
            json!({ "uri": "resonate://library/playlist/Morning" }),
        ),
    )
    .expect("a read of a playlist nobody made to be answered");
    assert!(
        absent["error"]["message"]
            .as_str()
            .is_some_and(|said| said.contains("no playlist named Morning")),
        "{absent}"
    );
}

fn prompted(server: &Server, name: &str, arguments: Value) -> (String, Value) {
    let got = result(
        server,
        "prompts/get",
        json!({ "name": name, "arguments": arguments }),
    );
    let messages = got["messages"]
        .as_array()
        .expect("a prompt carries messages");
    assert_eq!(messages.len(), 2, "{got}");
    assert!(
        messages.iter().all(|message| message["role"] == "user"),
        "{got}"
    );
    let asked = messages[0]["content"]["text"]
        .as_str()
        .expect("a prompt opens with what it asks")
        .to_owned();
    assert_eq!(messages[1]["content"]["type"], "resource", "{got}");
    (asked, messages[1]["content"]["resource"].clone())
}

#[test]
fn every_prompt_is_listed_with_the_arguments_it_requires() {
    let server = nothing_running();

    let initialised = result(
        &server,
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "capabilities": {} }),
    );
    assert_eq!(initialised["capabilities"]["prompts"]["listChanged"], false);

    let listed = result(&server, "prompts/list", json!({}));
    let prompts = listed["prompts"].as_array().expect("a list of prompts");
    let names: Vec<&str> = prompts
        .iter()
        .map(|prompt| prompt["name"].as_str().expect("a prompt has a name"))
        .collect();
    assert_eq!(
        names,
        [
            "build_a_playlist",
            "review_my_listening",
            "complete_my_albums",
            "about_this_track"
        ]
    );
    assert_eq!(prompts[0]["arguments"][0]["name"], "brief");
    assert_eq!(prompts[0]["arguments"][0]["required"], true);
    assert_eq!(prompts[0]["arguments"][1]["required"], false);
    assert_eq!(prompts[3]["arguments"], json!([]));
}

#[test]
fn a_prompt_embeds_the_reading_its_resource_answers() {
    let tree = Tree::new();
    tree.wav("a.wav", "Night Signal", "Hours", "1");
    let library = scanned(&tree);
    library
        .create_playlist("Mornings")
        .expect("a playlist to be made");
    let (players, _) = Fake::with(Standing {
        rows: vec![row(9, "Echoes")],
        playing: Some(0),
        status: Some(PlaybackStatus::Playing),
        ..Standing::default()
    });
    let server = server(library, players);

    let (asked, embedded) = prompted(
        &server,
        "build_a_playlist",
        json!({ "brief": "  rain on a window  ", "name": "Drizzle" }),
    );
    assert!(asked.contains("brief: rain on a window."), "{asked}");
    assert!(asked.contains("under the name Drizzle"), "{asked}");
    assert!(asked.contains("search_library") && asked.contains("create_playlist"));
    assert_eq!(embedded["uri"], "resonate://library/playlists");
    let text = embedded["text"]
        .as_str()
        .expect("an embedded reading is text");
    assert_eq!(
        serde_json::from_str::<Value>(text).expect("the text to be JSON"),
        read_resource(&server, "resonate://library/playlists")
    );

    let (unnamed, _) = prompted(&server, "build_a_playlist", json!({ "brief": "rain" }));
    assert!(unnamed.contains("of your choosing"), "{unnamed}");

    let (asked, embedded) = prompted(&server, "review_my_listening", json!({ "window": "year" }));
    assert!(asked.contains("over the last year"), "{asked}");
    assert_eq!(embedded["uri"], "resonate://library/statistics/year");
    assert_eq!(
        serde_json::from_str::<Value>(embedded["text"].as_str().expect("text"))
            .expect("the text to be JSON"),
        called(&server, "listening_statistics", json!({ "window": "year" }))
    );

    let (_, embedded) = prompted(&server, "review_my_listening", json!({}));
    assert_eq!(embedded["uri"], "resonate://library/statistics");

    let (asked, embedded) = prompted(&server, "complete_my_albums", json!({}));
    assert!(asked.contains("want_tracks"), "{asked}");
    assert_eq!(embedded["uri"], "resonate://library/missing");

    let (_, embedded) = prompted(&server, "about_this_track", json!({}));
    assert_eq!(embedded["uri"], "resonate://player/now-playing");
    assert!(
        embedded["text"]
            .as_str()
            .is_some_and(|text| text.contains("Echoes")),
        "{embedded}"
    );
}

#[test]
fn a_prompt_asked_wrongly_is_refused_and_one_whose_reading_fails_fails() {
    let server = nothing_running();

    for params in [
        json!({ "name": "compose_a_symphony" }),
        json!({ "name": "build_a_playlist" }),
        json!({ "name": "build_a_playlist", "arguments": { "brief": "   " } }),
        json!({ "name": "review_my_listening", "arguments": { "window": "decade" } }),
        json!({}),
    ] {
        assert_eq!(
            error_code(&server, &request("prompts/get", params.clone())),
            -32_602,
            "{params}"
        );
    }

    let answer = asked(
        &server,
        &request("prompts/get", json!({ "name": "about_this_track" })),
    )
    .expect("a failed prompt to be answered");
    assert_eq!(answer["error"]["code"], -32_603, "{answer}");
    assert!(
        answer["error"]["message"]
            .as_str()
            .is_some_and(|said| said.contains("no player")),
        "{answer}"
    );
}

#[test]
fn a_window_of_listening_is_a_resource_under_the_window_it_names() {
    let server = nothing_running();

    for window in ["week", "month", "year", "everything"] {
        assert_eq!(
            read_resource(&server, &format!("resonate://library/statistics/{window}")),
            called(&server, "listening_statistics", json!({ "window": window })),
            "{window}"
        );
    }
    assert_eq!(
        error_code(
            &server,
            &request(
                "resources/read",
                json!({ "uri": "resonate://library/statistics/decade" })
            )
        ),
        -32_002
    );
    let templates = result(&server, "resources/templates/list", json!({}));
    assert_eq!(
        templates["resourceTemplates"][1]["uriTemplate"],
        "resonate://library/statistics/{window}"
    );
}
