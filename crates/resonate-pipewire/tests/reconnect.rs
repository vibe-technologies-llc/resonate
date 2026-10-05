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
const SINK: &str = "resonate-hosted-sink";
const SOCKET: &str = "pipewire-0";
const PATIENCE: Duration = Duration::from_secs(10);
const HUNG_PATIENCE: Duration = Duration::from_secs(20);
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

    fn signalled(&self, signal: &str) {
        let sent = Command::new("kill")
            .args([signal, &self.daemon.id().to_string()])
            .status()
            .expect("kill runs");
        assert!(
            sent.success(),
            "the hosted daemon could not be sent {signal}"
        );
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
    told_it_is_disconnected_within(pipewire, PATIENCE)
}

fn told_it_is_disconnected_within(pipewire: &PipeWire, patience: Duration) -> bool {
    let deadline = Instant::now() + patience;
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
        realtime: false,
    }
}

fn a_folder_of_its_own(tag: &str) -> PathBuf {
    let folder = env::temp_dir().join(format!("rpw-{}-{tag}", std::process::id()));
    fs::create_dir_all(&folder).expect("a folder for the hosted daemon");
    fs::write(folder.join("hosted.conf"), HOSTED_CONFIG).expect("the hosted daemon's config");
    folder
}

fn a_daemon_restarting_under_the_client(folder: &Path) {
    let Some(hosted) = Hosted::start(folder) else {
        eprintln!("skipped: no pipewire binary to host a daemon with");
        return;
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
}

fn a_daemon_hanging_under_the_client(folder: &Path) {
    let Some(hosted) = Hosted::start(folder) else {
        eprintln!("skipped: no pipewire binary to host a daemon with");
        return;
    };

    let pipewire = PipeWire::start("resonate-reconnect-test").expect("the hosted daemon answers");
    the_hosted_sink(&pipewire).expect("the hosted daemon's sink is found");

    hosted.signalled("-STOP");
    let taken_as_lost = told_it_is_disconnected_within(&pipewire, HUNG_PATIENCE);
    hosted.signalled("-CONT");
    let found_again = the_hosted_sink(&pipewire);

    let _ = pipewire.shutdown();
    hosted.kill();

    assert!(
        taken_as_lost,
        "a daemon that stopped answering was never taken as gone"
    );
    assert!(
        found_again.is_some(),
        "the sink was never found once the daemon answered again"
    );
}

fn a_daemon_coming_after_the_client(folder: &Path) {
    let pipewire = PipeWire::start("resonate-reconnect-test")
        .expect("the client starts with no daemon to reach");
    let gone_at_first = told_it_is_disconnected(&pipewire);
    let Some(hosted) = Hosted::start(folder) else {
        eprintln!("skipped: no pipewire binary to host a daemon with");
        return;
    };

    let found = the_hosted_sink(&pipewire);
    let opened = found
        .as_ref()
        .map(|sink| pipewire.open(&request(sink), Box::new(Silence)).is_ok());

    let _ = pipewire.shutdown();
    hosted.kill();

    assert!(gone_at_first, "the client did not say it had no daemon");
    assert!(
        found.is_some(),
        "the sink was never found once the daemon came"
    );
    assert_eq!(
        opened,
        Some(true),
        "no stream opened on the daemon that came"
    );
}

#[test]
fn a_client_whose_daemon_restarts_finds_the_graph_again_and_opens_on_it() {
    hosted_as(
        "a_client_whose_daemon_restarts_finds_the_graph_again_and_opens_on_it",
        "r",
        a_daemon_restarting_under_the_client,
    );
}

#[test]
fn a_client_whose_daemon_stops_answering_takes_it_as_gone_and_finds_it_again() {
    hosted_as(
        "a_client_whose_daemon_stops_answering_takes_it_as_gone_and_finds_it_again",
        "h",
        a_daemon_hanging_under_the_client,
    );
}

#[test]
fn a_client_started_before_its_daemon_finds_the_graph_once_it_comes() {
    hosted_as(
        "a_client_started_before_its_daemon_finds_the_graph_once_it_comes",
        "b",
        a_daemon_coming_after_the_client,
    );
}

fn hosted_as(this_test: &str, tag: &str, under: fn(&Path)) {
    if let Some(folder) = env::var_os(HOSTED_AT).map(PathBuf::from) {
        under(&folder);
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

    let folder = a_folder_of_its_own(tag);
    let ran = Command::new(env::current_exe().expect("the test binary"))
        .args([this_test, "--exact", "--nocapture"])
        .env(HOSTED_AT, &folder)
        .env("PIPEWIRE_RUNTIME_DIR", &folder)
        .status()
        .expect("the test binary runs again under the hosted daemon");
    let _ = fs::remove_dir_all(&folder);

    assert!(ran.success(), "the run under the hosted daemon failed");
}
