use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use resonate_listen::{Hearing, Listener, Listening};
use resonate_pipewire::NodeName;

const HOSTED_AT: &str = "RESONATE_HOSTED_PIPEWIRE";
const THIS_TEST: &str = "a_recording_from_a_named_microphone_is_opened_on_that_microphone";
const LISTENER: &str = "resonate-listen-microphone-test";
const MICROPHONE: &str = "resonate-hosted-microphone";
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
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = resonate-hosted-microphone
            node.description = "A microphone this test hosts"
            media.class = Audio/Source
            audio.position = [ MONO ]
            object.linger = true
        }
    }
]
"#;

struct Hosted {
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
        Some(Self { daemon })
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

fn pw_cli(folder: &Path, args: &[&str]) -> String {
    Command::new("pw-cli")
        .args(args)
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map(|said| String::from_utf8_lossy(&said.stdout).into_owned())
        .unwrap_or_default()
}

fn the_capture_node(folder: &Path) -> Option<String> {
    let named = format!("node.name = \"{LISTENER}\"");
    let listed = pw_cli(folder, &["ls", "Node"]);
    let mut id = None;
    for line in listed.lines() {
        if let Some(rest) = line.trim().strip_prefix("id ") {
            id = rest.split(',').next().map(str::to_owned);
        } else if line.contains(&named) {
            return id;
        }
    }
    None
}

fn the_capture_aims_at(folder: &Path, source: &str) -> bool {
    let aimed = format!("target.object = \"{source}\"");
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if let Some(id) = the_capture_node(folder) {
            let described = pw_cli(folder, &["info", &id]);
            return described.contains(&aimed) && !described.contains("stream.capture.sink");
        }
        thread::sleep(POLL_EVERY);
    }
    false
}

fn runs_under_a_hosted_daemon() -> bool {
    let Some(folder) = env::var_os(HOSTED_AT).map(PathBuf::from) else {
        return false;
    };
    let Some(_hosted) = Hosted::start(&folder) else {
        eprintln!("skipped: no pipewire binary to host a daemon with");
        return true;
    };

    let listener = Listener::new(LISTENER);
    let microphones = listener
        .microphones()
        .expect("the hosted daemon lists its sources");
    let listed = microphones
        .iter()
        .any(|microphone| microphone.name.as_str() == MICROPHONE);

    let hearing = Hearing::new();
    let recording = {
        let hearing = Arc::clone(&hearing);
        thread::spawn(move || {
            let from = Listening::Microphone(Some(NodeName::new(MICROPHONE)));
            listener.record(&from, LISTENS_FOR, &hearing)
        })
    };
    let aimed = the_capture_aims_at(&folder, MICROPHONE);
    hearing.stop();
    let _ = recording.join();

    assert!(
        listed,
        "the hosted microphone was not listed: {microphones:?}"
    );
    assert!(aimed, "no capture was opened on the named microphone");
    true
}

#[test]
fn a_recording_from_a_named_microphone_is_opened_on_that_microphone() {
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

    let folder = env::temp_dir().join(format!("rlm-{}", std::process::id()));
    fs::create_dir_all(&folder).expect("a folder for the hosted daemon");
    fs::write(folder.join("hosted.conf"), HOSTED_CONFIG).expect("the hosted daemon's config");
    let ran = Command::new(env::current_exe().expect("the test binary"))
        .args([THIS_TEST, "--exact", "--nocapture"])
        .env(HOSTED_AT, &folder)
        .env("PIPEWIRE_RUNTIME_DIR", &folder)
        .status()
        .expect("the test binary runs again under the hosted daemon");
    let _ = fs::remove_dir_all(&folder);

    assert!(ran.success(), "the run under the hosted daemon failed");
}
