use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_listen::{Hearing, Listener, Listening};

const HOSTED_AT: &str = "RESONATE_HOSTED_PIPEWIRE";
const THIS_TEST: &str =
    "a_capture_whose_daemon_restarts_is_opened_again_on_the_graph_that_comes_back";
const LISTENER: &str = "resonate-listen-reconnect-test";
const SOCKET: &str = "pipewire-0";
const PATIENCE: Duration = Duration::from_secs(10);
const POLL_EVERY: Duration = Duration::from_millis(50);
const LISTENS_FOR: Duration = Duration::from_secs(30);

const HOSTED_CONFIG: &str = r#"
context.properties = {
    core.daemon = true
    core.name = pipewire-0
    support.dbus = false
}
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    audio.adapt = audioconvert/libspa-audioconvert
    support.* = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-metadata }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-access }
    { name = libpipewire-module-adapter }
]
context.objects = [
    { factory = metadata args = { metadata.name = default } }
    { factory = spa-node-factory
        args = {
            factory.name = support.node.driver
            node.name = Dummy-Driver
            priority.driver = 20000
        }
    }
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = resonate-hosted-sink
            node.description = "A sink this test hosts"
            media.class = Audio/Sink
            audio.position = [ FL FR ]
            object.linger = true
        }
    }
]
"#;

struct Hosted {
    folder: PathBuf,
    daemon: Child,
}

impl Hosted {
    fn start(folder: &Path) -> Option<Self> {
        for stale in [SOCKET, "pipewire-0.lock"] {
            let _ = fs::remove_file(folder.join(stale));
        }
        let daemon = Command::new("pipewire")
            .arg("-c")
            .arg(folder.join("hosted.conf"))
            .env("PIPEWIRE_RUNTIME_DIR", folder)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let deadline = Instant::now() + PATIENCE;
        while !folder.join(SOCKET).exists() && Instant::now() < deadline {
            thread::sleep(POLL_EVERY);
        }
        Some(Self {
            folder: folder.to_owned(),
            daemon,
        })
    }

    fn kill(mut self) -> PathBuf {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        self.folder
    }
}

fn holds_the_capture(folder: &Path) -> bool {
    let named = format!("node.name = \"{LISTENER}\"");
    Command::new("pw-cli")
        .args(["ls", "Node"])
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|listed| String::from_utf8_lossy(&listed.stdout).contains(&named))
}

fn comes_to_hold_the_capture(folder: &Path) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if holds_the_capture(folder) {
            return true;
        }
        thread::sleep(POLL_EVERY);
    }
    false
}

fn a_folder_of_its_own() -> PathBuf {
    let folder = env::temp_dir().join(format!("rls-{}", std::process::id()));
    fs::create_dir_all(&folder).expect("a folder for the hosted daemon");
    fs::write(folder.join("hosted.conf"), HOSTED_CONFIG).expect("the hosted daemon's config");
    folder
}

fn runs_under_a_hosted_daemon() -> bool {
    let Some(folder) = env::var_os(HOSTED_AT).map(PathBuf::from) else {
        return false;
    };
    let Some(hosted) = Hosted::start(&folder) else {
        eprintln!("skipped: no pipewire binary to host a daemon with");
        return true;
    };

    let hearing = Hearing::new();
    let recording = {
        let hearing = Arc::clone(&hearing);
        thread::spawn(move || {
            Listener::new(LISTENER).record(&Listening::Desktop, LISTENS_FOR, &hearing)
        })
    };
    let captured_first = comes_to_hold_the_capture(&folder);

    let folder = hosted.kill();
    let hosted = Hosted::start(&folder).expect("the hosted daemon starts again");
    let captured_again = comes_to_hold_the_capture(&folder);

    hearing.stop();
    let _ = recording.join();
    hosted.kill();

    assert!(
        captured_first,
        "the capture never reached the hosted daemon"
    );
    assert!(
        captured_again,
        "the capture was not opened again once the daemon was back"
    );
    true
}

#[test]
fn a_capture_whose_daemon_restarts_is_opened_again_on_the_graph_that_comes_back() {
    if runs_under_a_hosted_daemon() {
        return;
    }
    let tools = ["pipewire", "pw-cli"].into_iter().all(|tool| {
        Command::new(tool)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    });
    if !tools {
        eprintln!("skipped: no pipewire and pw-cli to host a daemon with");
        return;
    }

    let folder = a_folder_of_its_own();
    let ran = Command::new(env::current_exe().expect("the test binary"))
        .args([THIS_TEST, "--exact", "--nocapture"])
        .env(HOSTED_AT, &folder)
        .env("PIPEWIRE_RUNTIME_DIR", &folder)
        .status()
        .expect("the test binary runs again under the hosted daemon");
    let _ = fs::remove_dir_all(&folder);

    assert!(ran.success(), "the run under the hosted daemon failed");
}
