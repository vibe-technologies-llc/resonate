use std::{
    env,
    fs::{self, File, OpenOptions},
    io::Read as _,
    os::unix::{fs::OpenOptionsExt as _, process::ExitStatusExt as _},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use rustix::{
    fs::OFlags,
    pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt},
    termios::{LocalModes, tcgetattr},
};

const PATIENCE: Duration = Duration::from_secs(15);
const POLL_EVERY: Duration = Duration::from_millis(20);
const SETTLED_FOR: Duration = Duration::from_millis(500);
const KEPT_BY_A_LINE_AT_A_TIME: LocalModes = LocalModes::ICANON.union(LocalModes::ECHO);
const RATE: u32 = 44_100;
const SECONDS: u32 = 30;

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-{name}-{}", std::process::id()));
        fs::create_dir_all(path.join("graph")).expect("a temporary home");
        Self(path)
    }

    fn silence(&self) -> PathBuf {
        let data = RATE * SECONDS * 4;
        let mut wav = b"RIFF".to_vec();
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&RATE.to_le_bytes());
        wav.extend_from_slice(&(RATE * 4).to_le_bytes());
        wav.extend_from_slice(&4_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        wav.resize(wav.len() + data as usize, 0);

        let path = self.0.join("silence.wav");
        fs::write(&path, wav).expect("a silent file to play");
        path
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Terminal {
    seat: File,
}

impl Terminal {
    fn open() -> Option<(Self, File)> {
        let controller = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).ok()?;
        grantpt(&controller).ok()?;
        unlockpt(&controller).ok()?;
        let named = ptsname(&controller, Vec::new()).ok()?;
        let seat = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(OFlags::NOCTTY.bits() as i32)
            .open(Path::new(named.to_str().ok()?))
            .ok()?;
        Some((Self { seat }, File::from(controller)))
    }

    fn modes(&self) -> LocalModes {
        tcgetattr(&self.seat)
            .expect("the terminal's modes")
            .local_modes
    }

    fn stdio(&self) -> Stdio {
        Stdio::from(self.seat.try_clone().expect("the terminal's seat"))
    }
}

fn drained(mut controller: File) {
    thread::spawn(move || {
        let mut held = [0_u8; 4_096];
        while controller.read(&mut held).is_ok_and(|read| read > 0) {}
    });
}

fn playing(home: &Home, terminal: &Terminal) -> Child {
    Command::new("setsid")
        .arg("--ctty")
        .arg(env!("CARGO_BIN_EXE_resonate"))
        .arg("play")
        .arg(home.silence())
        .env("HOME", &home.0)
        .env("XDG_CONFIG_HOME", home.0.join("config"))
        .env("XDG_DATA_HOME", home.0.join("data"))
        .env("XDG_CACHE_HOME", home.0.join("cache"))
        .env("XDG_STATE_HOME", home.0.join("state"))
        .env("PIPEWIRE_RUNTIME_DIR", home.0.join("graph"))
        .env("DBUS_SESSION_BUS_ADDRESS", "unix:path=/nonexistent")
        .stdin(terminal.stdio())
        .stdout(terminal.stdio())
        .stderr(terminal.stdio())
        .spawn()
        .expect("the binary to start")
}

fn until(mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(POLL_EVERY);
    }
    false
}

fn left(child: &mut Child) -> ExitStatus {
    let mut status = None;
    until(|| {
        status = child.try_wait().expect("the child to be waited on");
        status.is_some()
    });
    status.unwrap_or_else(|| {
        let _ = child.kill();
        panic!("play did not leave within {PATIENCE:?} of the signal");
    })
}

fn signalled(child: &Child, signal: &str) {
    let sent = Command::new("kill")
        .arg(format!("-{signal}"))
        .arg(child.id().to_string())
        .status()
        .expect("kill to run");
    assert!(sent.success(), "kill -{signal} failed");
}

fn told_to_leave_by(signal: &str) {
    if Command::new("setsid")
        .arg("--version")
        .stdout(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("skipped: no setsid to give play a terminal of its own");
        return;
    }
    let Some((terminal, controller)) = Terminal::open() else {
        eprintln!("skipped: no pseudo-terminal to give play");
        return;
    };
    let home = Home::new(&format!("terminal-{signal}"));
    let before = terminal.modes();
    assert!(before.contains(KEPT_BY_A_LINE_AT_A_TIME));

    drained(controller);
    let mut child = playing(&home, &terminal);
    let keyed = until(|| !terminal.modes().contains(LocalModes::ICANON));
    thread::sleep(SETTLED_FOR);
    let still_playing = child
        .try_wait()
        .expect("the child to be waited on")
        .is_none();
    signalled(&child, signal);
    let status = left(&mut child);
    let after = terminal.modes();

    assert!(keyed, "play never took the terminal a key at a time");
    assert!(still_playing, "play left before it was told to: {status}");
    assert_eq!(
        status.signal(),
        None,
        "{signal} killed play with nothing caught"
    );
    assert_eq!(
        after & KEPT_BY_A_LINE_AT_A_TIME,
        before & KEPT_BY_A_LINE_AT_A_TIME,
        "play left on {signal} with the terminal still a key at a time"
    );
}

#[test]
fn an_interrupt_leaves_play_with_the_terminal_as_it_found_it() {
    told_to_leave_by("INT");
}

#[test]
fn a_termination_leaves_play_with_the_terminal_as_it_found_it() {
    told_to_leave_by("TERM");
}

#[test]
fn a_hang_up_leaves_play_with_the_terminal_as_it_found_it() {
    told_to_leave_by("HUP");
}
