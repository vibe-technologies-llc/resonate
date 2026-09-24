use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{
    AudioSource, Error, LatencyRequest, MediaRole, PipeWire, SinkInfo, StreamRequest,
};

const HOSTED_AT: &str = "RESONATE_HOSTED_PIPEWIRE";
const THIS_TEST: &str = "a_client_whose_daemon_restarts_finds_the_graph_again_and_opens_on_it";
const SINK: &str = "resonate-hosted-sink";
const SOCKET: &str = "pipewire-0";
const PATIENCE: Duration = Duration::from_secs(10);
const ASKED_WITHIN: Duration = Duration::from_millis(500);
const POLL_EVERY: Duration = Duration::from_millis(50);

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

struct Silence;

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        dst.fill(0);
        dst.len()
    }

    fn on_underrun(&mut self, _missing_bytes: usize) {}
}

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

fn the_hosted_sink(pipewire: &PipeWire) -> Option<SinkInfo> {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if let Ok(sinks) = pipewire.enumerate_sinks(ASKED_WITHIN)
            && let Some(sink) = sinks.into_iter().find(|sink| sink.name.as_str() == SINK)
        {
            return Some(sink);
        }
        thread::sleep(POLL_EVERY);
    }
    None
}

fn told_it_is_disconnected(pipewire: &PipeWire) -> bool {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if matches!(
            pipewire.enumerate_sinks(ASKED_WITHIN),
            Err(Error::Disconnected)
        ) {
            return true;
        }
        thread::sleep(POLL_EVERY);
    }
    false
}

fn request(sink: &SinkInfo) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec: StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::F32,
        ),
        latency: LatencyRequest::Auto,
        role: MediaRole::Music,
        media_name: "resonate reconnect test".to_owned(),
        force_graph_rate: false,
        no_convert: false,
        exclusive: false,
        realtime: false,
    }
}

fn a_folder_of_its_own() -> PathBuf {
    let folder = env::temp_dir().join(format!("rpw-{}", std::process::id()));
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

    let pipewire = PipeWire::start("resonate-reconnect-test").expect("the hosted daemon answers");
    let before = the_hosted_sink(&pipewire).expect("the hosted daemon's sink is found");
    let opened = pipewire
        .open(&request(&before), Box::new(Silence))
        .expect("a stream opens on the hosted sink");

    let folder = hosted.kill();
    let gone_between = told_it_is_disconnected(&pipewire);
    drop(opened);
    let hosted = Hosted::start(&folder).expect("the hosted daemon starts again");

    let after = the_hosted_sink(&pipewire).expect("the sink is found once the daemon is back");
    let reopened = pipewire.open(&request(&after), Box::new(Silence));
    let opens_again = reopened.is_ok();

    drop(reopened);
    let _ = pipewire.shutdown();
    hosted.kill();

    assert!(
        gone_between,
        "the client never said the daemon had gone between the two"
    );
    assert!(
        opens_again,
        "no stream opened on the daemon once it was back"
    );
    true
}

#[test]
fn a_client_whose_daemon_restarts_finds_the_graph_again_and_opens_on_it() {
    if runs_under_a_hosted_daemon() {
        return;
    }
    if Command::new("pipewire")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("skipped: no pipewire binary to host a daemon with");
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
