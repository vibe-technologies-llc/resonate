use std::{
    env, fs,
    io::{self, Read},
    os::unix::{
        fs::{FileTypeExt, MetadataExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

use resonate_core::AppId;
use serde::{Deserialize, Serialize};

use crate::{
    Error, IpcOp, JsonOp, Result,
    activity::Activity,
    frame::{Frame, Opcode, split_frame, write_frame},
};

const PROTOCOL: u8 = 1;
const SOCKETS_PER_FOLDER: u8 = 10;
const SOCKET_STEM: &str = "discord-ipc-";
const SHARED_TEMPORARY: &str = "/tmp";
const SANDBOXES: [&str; 11] = [
    "",
    "app/com.discordapp.Discord",
    "app/com.discordapp.DiscordCanary",
    "app/com.discordapp.DiscordPTB",
    "app/dev.vencord.Vesktop",
    ".flatpak/com.discordapp.Discord/xdg-run",
    ".flatpak/com.discordapp.DiscordCanary/xdg-run",
    ".flatpak/dev.vencord.Vesktop/xdg-run",
    "snap.discord",
    "snap.discord-canary",
    "snap.discord-ptb",
];
const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
const READ_AT_ONCE: usize = 4096;
const READY_WITHIN: Duration = Duration::from_secs(5);
const SET_ACTIVITY: &str = "SET_ACTIVITY";
const DISPATCH: &str = "DISPATCH";
const READY: &str = "READY";
const ERROR: &str = "ERROR";

pub(crate) fn candidates(runtime: Option<&Path>, temporary: Option<&Path>) -> Vec<PathBuf> {
    let mut folders: Vec<&Path> = Vec::new();
    for folder in [runtime, temporary, Some(Path::new(SHARED_TEMPORARY))]
        .into_iter()
        .flatten()
    {
        if folder.is_absolute() && !folders.contains(&folder) {
            folders.push(folder);
        }
    }

    let mut paths = Vec::new();
    for folder in folders {
        for sandbox in SANDBOXES {
            let within = folder.join(sandbox);
            for number in 0..SOCKETS_PER_FOLDER {
                paths.push(within.join(format!("{SOCKET_STEM}{number}")));
            }
        }
    }
    paths
}

fn candidates_here() -> Vec<PathBuf> {
    let runtime = env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let temporary = env::var_os("TMPDIR").map(PathBuf::from);
    candidates(runtime.as_deref(), temporary.as_deref())
}

fn held_by(socket: &fs::Metadata, user: u32) -> bool {
    socket.file_type().is_socket() && socket.uid() == user
}

#[derive(Serialize)]
struct Handshake {
    v: u8,
    client_id: String,
}

#[derive(Serialize)]
struct Command<'a> {
    cmd: &'static str,
    args: Arguments<'a>,
    nonce: String,
}

#[derive(Serialize)]
struct Arguments<'a> {
    pid: u32,
    activity: Option<&'a Activity>,
}

#[derive(Deserialize)]
struct ReplyDoc {
    #[serde(default)]
    cmd: Option<String>,
    #[serde(default)]
    evt: Option<String>,
    #[serde(default)]
    data: Option<FaultDoc>,
}

#[derive(Deserialize)]
struct FaultDoc {
    #[serde(default)]
    code: Option<i64>,
}

#[derive(Deserialize)]
struct CloseDoc {
    #[serde(default)]
    code: i64,
}

pub(crate) struct Session {
    stream: UnixStream,
    nonce: u64,
    received: Vec<u8>,
}

impl Session {
    pub(crate) fn find(app: AppId, answered_last: Option<&Path>) -> Result<(Self, PathBuf)> {
        let first = answered_last.map(Path::to_path_buf);
        let rest = candidates_here()
            .into_iter()
            .filter(|path| Some(path.as_path()) != answered_last);
        Self::find_among(app, first.into_iter().chain(rest))
    }

    fn find_among(app: AppId, paths: impl IntoIterator<Item = PathBuf>) -> Result<(Self, PathBuf)> {
        let mut answered = None;
        for path in paths {
            match Self::open(&path, app) {
                Ok(session) => {
                    tracing::debug!(path = %path.display(), "reached Discord");
                    return Ok((session, path));
                }
                Err(error) if error.names_no_application() => return Err(error),
                Err(Error::Socket { .. }) => {}
                Err(Error::NotOurs { path }) => {
                    tracing::warn!(path = %path.display(), "a Discord socket another user owns was passed over");
                }
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "a Discord socket would not take this client; trying the next");
                    answered.get_or_insert(error);
                }
            }
        }
        Err(answered.unwrap_or(Error::NoDiscord))
    }

    pub(crate) fn open(path: &Path, app: AppId) -> Result<Self> {
        Self::ours(path)?;
        let stream = UnixStream::connect(path).map_err(|source| Error::Socket {
            op: IpcOp::Connect,
            source,
        })?;
        configured(
            stream
                .set_read_timeout(Some(ANSWERS_WITHIN))
                .and_then(|()| stream.set_write_timeout(Some(ANSWERS_WITHIN))),
        )?;

        let mut session = Self {
            stream,
            nonce: 0,
            received: Vec::new(),
        };
        let handshake = Handshake {
            v: PROTOCOL,
            client_id: app.to_string(),
        };
        session.send(Opcode::Handshake, &encoded(&handshake)?)?;
        session.await_ready()?;
        Ok(session)
    }

    fn ours(path: &Path) -> Result<()> {
        let held = fs::metadata(path).map_err(|source| Error::Socket {
            op: IpcOp::Connect,
            source,
        })?;
        if held_by(&held, rustix::process::getuid().as_raw()) {
            Ok(())
        } else {
            Err(Error::NotOurs {
                path: path.to_path_buf(),
            })
        }
    }

    pub(crate) fn set(&mut self, activity: Option<&Activity>) -> Result<()> {
        self.nonce = self.nonce.wrapping_add(1);
        let command = Command {
            cmd: SET_ACTIVITY,
            args: Arguments {
                pid: process::id(),
                activity,
            },
            nonce: self.nonce.to_string(),
        };
        self.send(Opcode::Frame, &encoded(&command)?)
    }

    pub(crate) fn drain(&mut self) -> Result<()> {
        while let Some(frame) = self.waiting()? {
            self.answer(frame)?;
        }
        Ok(())
    }

    fn await_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + READY_WITHIN;
        while let Some(frame) = self.next_frame_by(deadline)? {
            if frame.opcode == Opcode::Frame {
                let reply: ReplyDoc = decoded(&frame.body)?;
                match (reply.cmd.as_deref(), reply.evt.as_deref()) {
                    (Some(DISPATCH), Some(READY)) => return Ok(()),
                    (_, Some(ERROR)) => {
                        return Err(Error::Refused {
                            code: reply.data.and_then(|fault| fault.code).unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            } else {
                self.answer(frame)?;
            }
        }
        Err(Error::Socket {
            op: IpcOp::Read,
            source: io::ErrorKind::TimedOut.into(),
        })
    }

    fn next_frame_by(&mut self, deadline: Instant) -> Result<Option<Frame>> {
        loop {
            if let Some(frame) = split_frame(&mut self.received)? {
                return Ok(Some(frame));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            configured(self.stream.set_read_timeout(Some(left.min(ANSWERS_WITHIN))))?;
            self.receive()?;
        }
    }

    fn answer(&mut self, frame: Frame) -> Result<()> {
        match frame.opcode {
            Opcode::Ping => self.send(Opcode::Pong, &frame.body),
            Opcode::Close => {
                let close: CloseDoc = decoded(&frame.body)?;
                Err(Error::Closed { code: close.code })
            }
            Opcode::Frame => {
                let reply: ReplyDoc = decoded(&frame.body)?;
                match (reply.evt.as_deref(), reply.data) {
                    (Some(ERROR), Some(FaultDoc { code: Some(code) })) => {
                        Err(Error::Refused { code })
                    }
                    _ => Ok(()),
                }
            }
            Opcode::Handshake | Opcode::Pong => Ok(()),
        }
    }

    fn waiting(&mut self) -> Result<Option<Frame>> {
        if let Some(frame) = split_frame(&mut self.received)? {
            return Ok(Some(frame));
        }

        configured(self.stream.set_nonblocking(true))?;
        let received = self.receive();
        configured(self.stream.set_nonblocking(false))?;
        received?;

        split_frame(&mut self.received)
    }

    fn receive(&mut self) -> Result<()> {
        let mut chunk = [0; READ_AT_ONCE];
        match self.stream.read(&mut chunk) {
            Ok(0) => Err(Error::Socket {
                op: IpcOp::Read,
                source: io::ErrorKind::UnexpectedEof.into(),
            }),
            Ok(read) => {
                self.received.extend_from_slice(&chunk[..read]);
                Ok(())
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                Ok(())
            }
            Err(source) => Err(Error::Socket {
                op: IpcOp::Read,
                source,
            }),
        }
    }

    fn send(&mut self, opcode: Opcode, body: &[u8]) -> Result<()> {
        write_frame(&mut self.stream, opcode, body)
    }
}

fn configured(result: io::Result<()>) -> Result<()> {
    result.map_err(|source| Error::Socket {
        op: IpcOp::Configure,
        source,
    })
}

fn encoded(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|source| Error::Json {
        op: JsonOp::Encode,
        source,
    })
}

fn decoded<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|source| Error::Json {
        op: JsonOp::Decode,
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Write,
        os::unix::net::UnixListener,
        sync::{
            atomic::{AtomicU32, Ordering},
            mpsc,
        },
        thread,
        time::SystemTime,
    };

    use resonate_core::{Pictured, Presence, Shown};
    use serde_json::{Value, json};

    use super::*;
    use crate::{activity::Playing, frame::read_frame};

    const APP: &str = "1234567890123456789";
    static FOLDERS: AtomicU32 = AtomicU32::new(0);

    struct Folder(PathBuf);

    impl Folder {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "resonate-discord-{}-{}",
                process::id(),
                FOLDERS.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).expect("a folder");
            Self(path)
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn app() -> AppId {
        AppId::parse(APP).expect("a snowflake")
    }

    fn body(frame: &Frame) -> Value {
        serde_json::from_slice(&frame.body).expect("json")
    }

    fn send(stream: &mut UnixStream, opcode: Opcode, value: &Value) {
        write_frame(stream, opcode, &serde_json::to_vec(value).expect("json")).expect("sent");
    }

    fn ready(stream: &mut UnixStream) {
        send(
            stream,
            Opcode::Frame,
            &json!({ "cmd": "DISPATCH", "evt": "READY", "data": { "v": 1 } }),
        );
    }

    #[test]
    fn every_folder_and_sandbox_is_tried_in_turn_and_none_twice() {
        let found = candidates(Some(Path::new("/run/user/1000")), Some(Path::new("/tmp")));

        assert_eq!(found.len(), 2 * SANDBOXES.len() * 10);
        assert_eq!(found[0], Path::new("/run/user/1000/discord-ipc-0"));
        assert_eq!(found[9], Path::new("/run/user/1000/discord-ipc-9"));
        assert_eq!(
            found[10],
            Path::new("/run/user/1000/app/com.discordapp.Discord/discord-ipc-0")
        );
        assert!(found.contains(&PathBuf::from("/tmp/snap.discord/discord-ipc-3")));
        assert!(found.contains(&PathBuf::from(
            "/run/user/1000/.flatpak/dev.vencord.Vesktop/xdg-run/discord-ipc-0"
        )));
        assert!(found.contains(&PathBuf::from(
            "/run/user/1000/app/com.discordapp.DiscordCanary/discord-ipc-0"
        )));
        assert!(found.contains(&PathBuf::from("/tmp/snap.discord-ptb/discord-ipc-1")));
    }

    #[test]
    fn a_ready_written_a_few_bytes_at_a_time_still_opens_the_session() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bound");

        let discord = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            read_frame(&mut stream).expect("a handshake");
            let mut wire = Vec::new();
            write_frame(
                &mut wire,
                Opcode::Frame,
                &serde_json::to_vec(&json!({ "cmd": "DISPATCH", "evt": "READY" })).expect("json"),
            )
            .expect("framed");
            for piece in wire.chunks(wire.len() / 3 + 1) {
                stream.write_all(piece).expect("written");
                thread::sleep(Duration::from_millis(400));
            }
            read_frame(&mut stream).expect("an activity")
        });

        let mut session = Session::open(&path, app()).expect("a session");
        session.set(None).expect("set");
        let set = discord.join().expect("the fake Discord");

        assert_eq!(body(&set)["cmd"], "SET_ACTIVITY");
    }

    #[test]
    fn a_frame_drained_in_halves_is_read_whole_once_the_second_arrives() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bound");

        let (halfway, heard_half) = mpsc::channel();
        let (go_on, told_to_go_on) = mpsc::channel::<()>();
        let discord = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            read_frame(&mut stream).expect("a handshake");
            ready(&mut stream);
            let mut wire = Vec::new();
            write_frame(
                &mut wire,
                Opcode::Frame,
                &serde_json::to_vec(&json!({ "evt": "ERROR", "data": { "code": 4002 } }))
                    .expect("json"),
            )
            .expect("framed");
            let (first, second) = wire.split_at(wire.len() / 2);
            stream.write_all(first).expect("written");
            halfway.send(()).expect("told");
            told_to_go_on.recv().expect("told");
            stream.write_all(second).expect("written");
            stream
        });

        let mut session = Session::open(&path, app()).expect("a session");
        heard_half.recv().expect("half sent");
        thread::sleep(Duration::from_millis(50));
        let at_half = session.drain();
        go_on.send(()).expect("told");
        let deadline = Instant::now() + Duration::from_secs(5);
        let refused = loop {
            match session.drain() {
                Ok(()) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                other => break other,
            }
        };
        drop(discord.join().expect("the fake Discord"));

        assert!(at_half.is_ok());
        assert!(matches!(refused, Err(Error::Refused { code: 4002 })));
    }

    #[test]
    fn a_relative_folder_is_never_tried() {
        let found = candidates(Some(Path::new("relative")), None);

        assert!(found.iter().all(|path| path.starts_with(SHARED_TEMPORARY)));
    }

    #[test]
    fn a_session_shakes_hands_sets_an_activity_answers_a_ping_and_clears() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bound");

        let (ponged, heard_the_pong) = mpsc::channel();
        let discord = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            let handshake = read_frame(&mut stream).expect("a handshake");
            ready(&mut stream);
            let set = read_frame(&mut stream).expect("an activity");
            send(&mut stream, Opcode::Ping, &json!({ "beat": 1 }));
            let pong = read_frame(&mut stream).expect("a pong");
            ponged.send(()).expect("told");
            let cleared = read_frame(&mut stream).expect("a clear");
            (handshake, set, pong, cleared)
        });

        let presence = Presence {
            enabled: true,
            app: Some(app()),
            shown: Shown::Track,
            pictured: Pictured::Nothing,
            ..Presence::OFF
        };
        let playing = Playing {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            album: None,
            cover: None,
            position: Duration::ZERO,
            duration: None,
            paused: false,
        };
        let activity = Activity::of(&presence, &playing, SystemTime::now()).expect("shown");

        let mut session = Session::open(&path, app()).expect("a session");
        session.set(Some(&activity)).expect("set");
        let deadline = Instant::now() + Duration::from_secs(5);
        while heard_the_pong.try_recv().is_err() {
            assert!(Instant::now() < deadline, "the ping was never answered");
            session.drain().expect("drained");
            thread::sleep(Duration::from_millis(10));
        }
        session.set(None).expect("cleared");
        let (handshake, set, pong, cleared) = discord.join().expect("the fake Discord");

        assert_eq!(handshake.opcode, Opcode::Handshake);
        assert_eq!(body(&handshake), json!({ "v": 1, "client_id": APP }));
        assert_eq!(set.opcode, Opcode::Frame);
        let set = body(&set);
        assert_eq!(set["cmd"], "SET_ACTIVITY");
        assert_eq!(set["args"]["pid"], process::id());
        assert_eq!(set["args"]["activity"]["type"], 2);
        assert_eq!(set["args"]["activity"]["details"], "Echoes");
        assert_eq!(set["args"]["activity"]["state"], "Pink Floyd");
        assert!(set["args"]["activity"]["timestamps"]["start"].as_u64() > Some(0));
        assert_eq!(pong.opcode, Opcode::Pong);
        assert_eq!(body(&pong), json!({ "beat": 1 }));
        assert_eq!(body(&cleared)["args"]["activity"], Value::Null);
    }

    #[test]
    fn an_application_discord_does_not_know_is_a_close_naming_it() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bound");

        let discord = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            read_frame(&mut stream).expect("a handshake");
            send(
                &mut stream,
                Opcode::Close,
                &json!({ "code": 4000, "message": "Invalid Client ID" }),
            );
        });

        let refused = Session::open(&path, app());
        discord.join().expect("the fake Discord");

        assert!(matches!(&refused, Err(error) if error.names_no_application()));
    }

    #[test]
    fn a_refused_activity_is_read_back_as_a_refusal() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let listener = UnixListener::bind(&path).expect("bound");

        let discord = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            read_frame(&mut stream).expect("a handshake");
            ready(&mut stream);
            read_frame(&mut stream).expect("an activity");
            send(
                &mut stream,
                Opcode::Frame,
                &json!({ "cmd": "SET_ACTIVITY", "evt": "ERROR", "data": { "code": 4002, "message": "no" } }),
            );
            stream
        });

        let mut session = Session::open(&path, app()).expect("a session");
        session.set(None).expect("set");
        let stream = discord.join().expect("the fake Discord");

        let deadline = Instant::now() + Duration::from_secs(5);
        let refused = loop {
            match session.drain() {
                Ok(()) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                other => break other,
            }
        };
        drop(stream);

        assert!(matches!(refused, Err(Error::Refused { code: 4002 })));
    }

    fn a_discord_at(
        path: &Path,
        answer: impl FnOnce(&mut UnixStream) + Send + 'static,
    ) -> thread::JoinHandle<()> {
        let listener = UnixListener::bind(path).expect("bound");
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accepted");
            read_frame(&mut stream).expect("a handshake");
            answer(&mut stream);
            let _ = read_frame(&mut stream);
        })
    }

    #[test]
    fn a_socket_that_will_not_take_this_client_is_passed_for_the_next() {
        let folder = Folder::new();
        let closing = folder.0.join("discord-ipc-0");
        let erring = folder.0.join("discord-ipc-1");
        let taking = folder.0.join("discord-ipc-2");
        let closed = a_discord_at(&closing, |stream| {
            send(
                stream,
                Opcode::Close,
                &json!({ "code": 4003, "message": "no" }),
            );
        });
        let erred = a_discord_at(&erring, |stream| {
            send(
                stream,
                Opcode::Frame,
                &json!({ "cmd": "DISPATCH", "evt": "ERROR", "data": { "code": 4001, "message": "no" } }),
            );
        });
        let took = a_discord_at(&taking, ready);

        let asked = Instant::now();
        let found = Session::find_among(
            app(),
            [
                folder.0.join("discord-ipc-9"),
                closing.clone(),
                erring.clone(),
                taking.clone(),
            ],
        );
        let asked_for = asked.elapsed();
        let reached = found.map(|(session, path)| {
            drop(session);
            path
        });
        for fake in [closed, erred, took] {
            fake.join().expect("a fake Discord");
        }

        assert_eq!(reached.ok(), Some(taking));
        assert!(
            asked_for < ANSWERS_WITHIN,
            "an error in the handshake was waited out for {asked_for:?}"
        );
    }

    #[test]
    fn an_application_every_discord_refuses_ends_the_search_at_the_first() {
        let folder = Folder::new();
        let refusing = folder.0.join("discord-ipc-0");
        let refused = a_discord_at(&refusing, |stream| {
            send(
                stream,
                Opcode::Close,
                &json!({ "code": 4000, "message": "Invalid Client ID" }),
            );
        });

        let found = Session::find_among(app(), [refusing, folder.0.join("discord-ipc-1")]);
        refused.join().expect("a fake Discord");

        assert!(matches!(&found, Err(error) if error.names_no_application()));
    }

    #[test]
    fn a_folder_nobody_listens_in_is_no_session() {
        let folder = Folder::new();

        let refused = Session::open(&folder.0.join("discord-ipc-0"), app());
        assert!(matches!(
            refused,
            Err(Error::Socket {
                op: IpcOp::Connect,
                ..
            })
        ));
    }

    #[test]
    fn a_socket_is_held_by_the_user_that_made_it_and_by_no_other() {
        let folder = Folder::new();
        let path = folder.0.join("discord-ipc-0");
        let _listener = UnixListener::bind(&path).expect("bound");
        let held = fs::metadata(&path).expect("a socket");
        let user = rustix::process::getuid().as_raw();

        assert!(held_by(&held, user));
        assert!(!held_by(&held, user.wrapping_add(1)));
    }

    #[test]
    fn a_file_that_is_no_socket_is_not_connected_to_and_the_search_goes_on() {
        let folder = Folder::new();
        let plain = folder.0.join("discord-ipc-0");
        fs::write(&plain, b"").expect("a file");

        let refused = Session::open(&plain, app());

        assert!(matches!(refused, Err(Error::NotOurs { .. })));
        assert!(matches!(
            Session::find_among(app(), [plain]),
            Err(Error::NoDiscord)
        ));
    }
}
