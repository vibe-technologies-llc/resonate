use std::{
    env,
    io::{self, Read},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    process,
    time::{Duration, Instant},
};

use resonate_core::AppId;
use serde::{Deserialize, Serialize};

use crate::{
    Error, IpcOp, JsonOp, Result,
    activity::Activity,
    frame::{Frame, Opcode, read_frame, write_frame},
};

const PROTOCOL: u8 = 1;
const SOCKETS_PER_FOLDER: u8 = 10;
const SOCKET_STEM: &str = "discord-ipc-";
const SHARED_TEMPORARY: &str = "/tmp";
const SANDBOXES: [&str; 4] = [
    "",
    "app/com.discordapp.Discord",
    "snap.discord",
    ".flatpak/dev.vencord.Vesktop/xdg-run",
];
const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
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
}

impl Session {
    pub(crate) fn find(app: AppId) -> Result<Self> {
        for path in candidates_here() {
            match Self::open(&path, app) {
                Ok(session) => {
                    tracing::debug!(path = %path.display(), "reached Discord");
                    return Ok(session);
                }
                Err(error @ Error::Closed { .. }) => return Err(error),
                Err(Error::Socket { .. }) => {}
                Err(error) => {
                    tracing::debug!(%error, path = %path.display(), "a Discord socket answered in a way this client does not read");
                }
            }
        }
        Err(Error::NoDiscord)
    }

    pub(crate) fn open(path: &Path, app: AppId) -> Result<Self> {
        let stream = UnixStream::connect(path).map_err(|source| Error::Socket {
            op: IpcOp::Connect,
            source,
        })?;
        stream
            .set_read_timeout(Some(ANSWERS_WITHIN))
            .and_then(|()| stream.set_write_timeout(Some(ANSWERS_WITHIN)))
            .map_err(|source| Error::Socket {
                op: IpcOp::Configure,
                source,
            })?;

        let mut session = Self { stream, nonce: 0 };
        let handshake = Handshake {
            v: PROTOCOL,
            client_id: app.to_string(),
        };
        session.send(Opcode::Handshake, &encoded(&handshake)?)?;
        session.await_ready()?;
        Ok(session)
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
        while Instant::now() < deadline {
            let frame = read_frame(&mut self.stream)?;
            if frame.opcode == Opcode::Frame {
                let reply: ReplyDoc = decoded(&frame.body)?;
                if reply.cmd.as_deref() == Some(DISPATCH) && reply.evt.as_deref() == Some(READY) {
                    return Ok(());
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
        let configured = |result: io::Result<()>| {
            result.map_err(|source| Error::Socket {
                op: IpcOp::Configure,
                source,
            })
        };
        configured(self.stream.set_nonblocking(true))?;
        let mut first = [0; 1];
        let peeked = self.stream.read(&mut first);
        configured(self.stream.set_nonblocking(false))?;

        match peeked {
            Ok(0) => Err(Error::Socket {
                op: IpcOp::Read,
                source: io::ErrorKind::UnexpectedEof.into(),
            }),
            Ok(_) => read_frame(&mut first.as_slice().chain(&mut self.stream)).map(Some),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
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
    use crate::activity::Playing;

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
}
