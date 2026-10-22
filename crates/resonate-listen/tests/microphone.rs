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
const LISTENER: &str = "resonate-listen-microphone-test";
const MICROPHONE: &str = "resonate-hosted-microphone";
const TONE: &str = "resonate-hosted-tone";
const TONE_FOR: Duration = Duration::from_secs(2);
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
    audiotestsrc = audiotestsrc/libspa-audiotestsrc
    support.* = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-metadata }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-access }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
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
    { factory = adapter
        args = {
            factory.name = audiotestsrc
            node.name = resonate-hosted-tone
            node.description = "A tone this test hosts"
            media.class = Audio/Source
            audio.position = [ FL FR ]
            node.pause-on-idle = false
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

fn pw_cli_succeeds(folder: &Path, args: &[&str]) -> bool {
    Command::new("pw-cli")
        .args(args)
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn node_named(folder: &Path, name: &str) -> Option<String> {
    let named = format!("node.name = \"{name}\"");
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

fn ported(folder: &Path, node: &str, direction: &str, format: &str) -> bool {
    let config = format!(
        "{{ direction: {direction}, mode: dsp, monitor: false, control: false, format: {{ mediaType: audio, mediaSubtype: raw, format: {format}, rate: 48000, channels: 2, position: [ FL FR ] }} }}"
    );
    pw_cli_succeeds(folder, &["set-param", node, "PortConfig", &config])
}

fn port_the_tone_and_the_capture(folder: &Path) -> bool {
    let (Some(capture), Some(tone)) = (the_capture_node(folder), node_named(folder, TONE)) else {
        return false;
    };
    ported(folder, &tone, "Output", "F32P") && ported(folder, &capture, "Input", "F32LE")
}

fn link_the_tone_to_the_capture(folder: &Path) -> bool {
    ["FL", "FR"].into_iter().all(|channel| {
        Command::new("pw-link")
            .arg(format!("{TONE}:capture_{channel}"))
            .arg(format!("{LISTENER}:input_{channel}"))
            .env("PIPEWIRE_RUNTIME_DIR", folder)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    })
}

fn the_microphone_is_aimed_at(folder: &Path) {
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
    let aimed = the_capture_aims_at(folder, MICROPHONE);
    hearing.stop();
    let _ = recording.join();

    assert!(
        listed,
        "the hosted microphone was not listed: {microphones:?}"
    );
    assert!(aimed, "no capture was opened on the named microphone");
}

fn the_tone_is_heard(folder: &Path) {
    let listener = Listener::new(LISTENER);
    let hearing = Hearing::new();
    let recording = thread::spawn(move || {
        let from = Listening::Microphone(Some(NodeName::new(TONE)));
        listener.record(&from, TONE_FOR, &hearing)
    });

    let ported = until(|| port_the_tone_and_the_capture(folder));
    let linked = ported && until(|| link_the_tone_to_the_capture(folder));
    let clip = recording.join().expect("the recording thread ends");

    assert!(ported, "the tone and the capture were given no ports");
    assert!(linked, "the capture never linked to the tone");
    let clip = clip.expect("the tone is recorded");
    assert!(!clip.is_silent(), "the recording of the tone is silent");
    assert!(
        clip.length() >= TONE_FOR / 2,
        "the recording is {:?} long",
        clip.length()
    );
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

fn hosted_as(this_test: &str, tag: &str, under: fn(&Path)) {
    if let Some(folder) = env::var_os(HOSTED_AT).map(PathBuf::from) {
        let Some(_hosted) = Hosted::start(&folder) else {
            eprintln!("skipped: no pipewire binary to host a daemon with");
            return;
        };
        under(&folder);
        return;
    }
    let tools = ["pipewire", "pw-cli", "pw-link"].into_iter().all(|tool| {
        Command::new(tool)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
    });
    if !tools {
        eprintln!("skipped: no pipewire, pw-cli and pw-link to host a daemon with");
        return;
    }

    let folder = env::temp_dir().join(format!("rlm-{}-{tag}", std::process::id()));
    fs::create_dir_all(&folder).expect("a folder for the hosted daemon");
    fs::write(folder.join("hosted.conf"), HOSTED_CONFIG).expect("the hosted daemon's config");
    let ran = Command::new(env::current_exe().expect("the test binary"))
        .args([this_test, "--exact", "--nocapture"])
        .env(HOSTED_AT, &folder)
        .env("PIPEWIRE_RUNTIME_DIR", &folder)
        .status()
        .expect("the test binary runs again under the hosted daemon");
    let _ = fs::remove_dir_all(&folder);

    assert!(ran.success(), "the run under the hosted daemon failed");
}

#[test]
fn a_recording_from_a_named_microphone_is_opened_on_that_microphone() {
    hosted_as(
        "a_recording_from_a_named_microphone_is_opened_on_that_microphone",
        "m",
        the_microphone_is_aimed_at,
    );
}

#[test]
fn a_recording_from_a_microphone_hears_the_tone_that_reaches_it() {
    hosted_as(
        "a_recording_from_a_microphone_hears_the_tone_that_reaches_it",
        "t",
        the_tone_is_heard,
    );
}
