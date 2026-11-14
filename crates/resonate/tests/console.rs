#![cfg(windows)]

use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{PipeReader, PipeWriter, Read as _, Write as _},
    path::PathBuf,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use resonate_win32::{
    ConsoleEvent, ConsoleMode, Process, PseudoConsole, console_input_mode, tell_the_console,
};

fn in_this_suite(test: &str) -> String {
    let (_, module) = module_path!()
        .split_once("::")
        .expect("a module of the suite");
    format!("{module}::{test}")
}

const PATIENCE: Duration = Duration::from_secs(15);
const POLL_EVERY: Duration = Duration::from_millis(200);
const SETTLED_FOR: Duration = Duration::from_millis(500);
const RATE: u32 = 44_100;
const SECONDS: u32 = 30;
const COLUMNS: i16 = 200;
const ROWS: i16 = 50;

const ERRAND: &str = "RESONATE_CONSOLE_ERRAND";
const ASKED: &str = "RESONATE_CONSOLE_ASKED";
const ERRAND_TEST: &str = "an_errand_run_inside_the_console";
const MODE_SAID: &str = "console-mode-";
const READ_THE_MODE: &str = "mode";
const INTERRUPT: &str = "interrupt";
const BREAK: &str = "break";
const STATUS_CONTROL_C_EXIT: u32 = 0xC000_013A;
const CTRL_C_TYPED: &[u8] = b"\x03";
const ESCAPE: char = '\u{1b}';

static ASKINGS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn an_errand_run_inside_the_console() {
    let Ok(errand) = env::var(ERRAND) else {
        return;
    };
    match errand.as_str() {
        READ_THE_MODE => {
            let mode = console_input_mode().expect("the console's input mode");
            let asked = env::var(ASKED).unwrap_or_default();
            println!("{MODE_SAID}{asked}={}.", mode.0);
        }
        INTERRUPT => tell_the_console(ConsoleEvent::Interrupt).expect("an interrupt told"),
        BREAK => tell_the_console(ConsoleEvent::Break).expect("a break told"),
        other => panic!("no errand is called {other}"),
    }
}

struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        let path = env::temp_dir().join(format!("resonate-{name}-{}", std::process::id()));
        fs::create_dir_all(&path).expect("a temporary home");
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

    fn environment(&self, extra: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        let mut environment: Vec<(OsString, OsString)> = env::vars_os()
            .filter(|(key, _)| key != ERRAND && key != ASKED)
            .collect();
        environment.extend(
            [
                ("XDG_CONFIG_HOME", self.0.join("config")),
                ("XDG_DATA_HOME", self.0.join("data")),
                ("XDG_CACHE_HOME", self.0.join("cache")),
                ("XDG_STATE_HOME", self.0.join("state")),
            ]
            .map(|(key, value)| (key.into(), value.into_os_string())),
        );
        environment.extend(
            extra
                .iter()
                .map(|(key, value)| ((*key).into(), (*value).into())),
        );
        environment
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Console {
    console: PseudoConsole,
    typed: PipeWriter,
    shown: Arc<Mutex<Vec<u8>>>,
}

fn shown_from(mut output: PipeReader) -> Arc<Mutex<Vec<u8>>> {
    let shown = Arc::new(Mutex::new(Vec::new()));
    let keeping = Arc::clone(&shown);
    thread::spawn(move || {
        let mut held = [0_u8; 4_096];
        while let Ok(read) = output.read(&mut held) {
            let Some(arrived) = held.get(..read).filter(|arrived| !arrived.is_empty()) else {
                return;
            };
            keeping.lock().extend_from_slice(arrived);
        }
    });
    shown
}

fn without_escapes(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut letters = text.chars().peekable();
    while let Some(letter) = letters.next() {
        if letter != ESCAPE {
            plain.push(letter);
            continue;
        }
        if letters.next_if_eq(&'[').is_some() {
            while letters
                .next()
                .is_some_and(|inside| !inside.is_ascii_alphabetic() && inside != '~')
            {}
        } else if letters.next_if_eq(&']').is_some() {
            while letters.next().is_some_and(|inside| inside != '\u{7}') {}
        } else {
            letters.next();
        }
    }
    plain
}

fn mode_said(shown: &str, asked: usize) -> Option<ConsoleMode> {
    let said = format!("{MODE_SAID}{asked}=");
    let plain = without_escapes(shown);
    let (_, after) = plain.rsplit_once(&said)?;
    let (digits, _) = after.split_once('.')?;
    digits.trim().parse().ok().map(ConsoleMode)
}

impl Console {
    fn open() -> Option<Self> {
        let (console, typed, output) = PseudoConsole::open(COLUMNS, ROWS).ok()?;
        Some(Self {
            console,
            typed,
            shown: shown_from(output),
        })
    }

    fn shown(&self) -> String {
        String::from_utf8_lossy(&self.shown.lock()).into_owned()
    }

    fn errand(&self, home: &Home, errand: &str, asked: usize) -> Process {
        let asked = asked.to_string();
        let test = env::current_exe().expect("this test's own executable");
        let errand_test = in_this_suite(ERRAND_TEST);
        let arguments = [
            errand_test.as_str(),
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ]
        .map(OsStr::new);
        self.console
            .spawn(
                &test,
                &arguments,
                &home.environment(&[(ERRAND, errand), (ASKED, &asked)]),
            )
            .expect("the errand to start inside the console")
    }

    fn mode(&self, home: &Home) -> ConsoleMode {
        let asked = ASKINGS.fetch_add(1, Ordering::Relaxed);
        let errand = self.errand(home, READ_THE_MODE, asked);
        let mut mode = None;
        let answered = until(|| {
            mode = mode_said(&self.shown(), asked);
            mode.is_some()
        });
        let _ = errand.end();
        assert!(
            answered,
            "the console's mode was never said; it showed:\n{}",
            self.shown()
        );
        mode.unwrap_or(ConsoleMode(0))
    }

    fn playing(&self, home: &Home) -> Process {
        let silence = home.silence();
        let arguments = [OsStr::new("play"), silence.as_os_str()];
        self.console
            .spawn(
                env!("CARGO_BIN_EXE_resonate").as_ref(),
                &arguments,
                &home.environment(&[]),
            )
            .expect("the binary to start inside the console")
    }

    fn type_a_ctrl_c(&mut self) {
        self.typed
            .write_all(CTRL_C_TYPED)
            .expect("a Ctrl-C typed into the console");
        self.typed.flush().expect("the typing to reach the console");
    }
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

fn left(child: &Process) -> u32 {
    let mut code = None;
    until(|| {
        code = child.exit_code().expect("the child to be waited on");
        code.is_some()
    });
    code.unwrap_or_else(|| {
        let _ = child.end();
        panic!("play did not leave within {PATIENCE:?} of being told to");
    })
}

fn has_an_audio_endpoint() -> bool {
    Command::new(env!("CARGO_BIN_EXE_resonate"))
        .arg("sinks")
        .output()
        .is_ok_and(|listed| String::from_utf8_lossy(&listed.stdout).contains("wasapi:"))
}

#[derive(Clone, Copy)]
enum Telling {
    TypedCtrlC,
    Interrupt,
    Break,
}

fn told_to_leave(told: Telling) {
    if !has_an_audio_endpoint() {
        eprintln!("skipped: no audio endpoint for play to play to");
        return;
    }
    let Some(mut console) = Console::open() else {
        eprintln!("skipped: no pseudo console to give play");
        return;
    };
    let home = Home::new("console");
    let before = console.mode(&home);
    assert!(before.takes_a_line_at_a_time(), "{before:?}");

    let child = console.playing(&home);
    let keyed = until(|| {
        child.exit_code().ok().flatten().is_none() && !console.mode(&home).takes_a_line_at_a_time()
    });
    thread::sleep(SETTLED_FOR);
    let still_playing = child
        .exit_code()
        .expect("the child to be waited on")
        .is_none();
    match told {
        Telling::TypedCtrlC => console.type_a_ctrl_c(),
        Telling::Interrupt => drop(console.errand(&home, INTERRUPT, usize::MAX)),
        Telling::Break => drop(console.errand(&home, BREAK, usize::MAX)),
    }
    let code = left(&child);
    let after = console.mode(&home);

    assert!(
        keyed,
        "play never took the console a key at a time; it showed:\n{}",
        console.shown()
    );
    assert!(still_playing, "play left before it was told to: {code:#x}");
    assert_ne!(code, STATUS_CONTROL_C_EXIT, "play left with nothing caught");
    assert!(
        after.takes_a_line_at_a_time(),
        "play left with the console still a key at a time: {after:?}"
    );
}

#[test]
fn a_ctrl_c_typed_leaves_play_with_the_console_as_it_found_it() {
    told_to_leave(Telling::TypedCtrlC);
}

#[test]
fn an_interrupt_leaves_play_with_the_console_as_it_found_it() {
    told_to_leave(Telling::Interrupt);
}

#[test]
fn a_break_leaves_play_with_the_console_as_it_found_it() {
    told_to_leave(Telling::Break);
}

#[test]
fn a_console_closed_under_play_leaves_it() {
    if !has_an_audio_endpoint() {
        eprintln!("skipped: no audio endpoint for play to play to");
        return;
    }
    let Some(console) = Console::open() else {
        eprintln!("skipped: no pseudo console to give play");
        return;
    };
    let home = Home::new("closed");

    let child = console.playing(&home);
    let keyed = until(|| {
        child.exit_code().ok().flatten().is_none() && !console.mode(&home).takes_a_line_at_a_time()
    });
    thread::sleep(SETTLED_FOR);
    let still_playing = child
        .exit_code()
        .expect("the child to be waited on")
        .is_none();
    let shown_before = console.shown();
    let Console {
        console: pseudo,
        typed,
        shown: _,
    } = console;
    drop(pseudo);
    drop(typed);
    let code = left(&child);

    assert!(
        keyed,
        "play never took the console a key at a time; it showed:\n{shown_before}"
    );
    assert!(
        still_playing,
        "play left before its console closed: {code:#x}"
    );
}
