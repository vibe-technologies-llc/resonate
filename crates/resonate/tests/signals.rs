#![cfg(feature = "mcp")]

use std::{
    env, fs,
    io::{BufRead as _, BufReader, Write as _},
    os::unix::process::ExitStatusExt as _,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const LEAVES_WITHIN: Duration = Duration::from_secs(10);
const HANG_UP: i32 = 1;
const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"signals","version":"0"}}}"#;

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).expect("a temporary home");
        Self(path)
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn serving(home: &Home) -> Child {
    Command::new(env!("CARGO_BIN_EXE_resonate"))
        .arg("mcp")
        .env("HOME", &home.0)
        .env("XDG_CONFIG_HOME", home.0.join("config"))
        .env("XDG_DATA_HOME", home.0.join("data"))
        .env("XDG_CACHE_HOME", home.0.join("cache"))
        .env("XDG_STATE_HOME", home.0.join("state"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("the binary to start")
}

fn answered_once(child: &mut Child) {
    let stdin = child.stdin.as_mut().expect("a piped stdin");
    writeln!(stdin, "{INITIALIZE}").expect("the request to be written");
    stdin.flush().expect("the request to be sent");

    let stdout = child.stdout.take().expect("a piped stdout");
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .expect("an answer to be read");
    assert!(line.contains("\"id\":1"), "the server answered {line:?}");
}

fn signalled(child: &Child, signal: &str) {
    let sent = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill to run");
    assert!(sent.success(), "kill -{signal} failed");
}

fn left(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + LEAVES_WITHIN;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("the child to be waited on") {
            return status;
        }
        thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    panic!("the server did not leave within {LEAVES_WITHIN:?} of the signal");
}

#[test]
fn a_hang_up_is_heard_as_a_request_to_leave_rather_than_killing_the_process() {
    let home = Home::new("hang-up");
    let mut child = serving(&home);
    answered_once(&mut child);

    signalled(&child, "HUP");
    let status = left(&mut child);

    assert_ne!(
        status.signal(),
        Some(HANG_UP),
        "the hang-up killed the process with nothing caught"
    );
}
