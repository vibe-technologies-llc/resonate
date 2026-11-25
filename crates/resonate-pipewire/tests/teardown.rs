use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{
    AudioSink, AudioSource, CaptureRequest, Capturing, Error, LatencyRequest, MediaRole, NodeName,
    PipeWire, SinkInfo, SinkStream, StreamEvent, StreamRequest, StreamState,
};

fn in_this_suite(test: &str) -> String {
    let (_, module) = module_path!()
        .split_once("::")
        .expect("a module of the suite");
    format!("{module}::{test}")
}

const HOSTED_AT: &str = "RESONATE_HOSTED_PIPEWIRE";
const SOCKET: &str = "pipewire-0";
const SINK: &str = "resonate-teardown-sink";
const CLIENT: &str = "resonate-teardown-test";
const PATIENCE: Duration = Duration::from_secs(10);
const SILENT_PATIENCE: Duration = Duration::from_secs(20);
const ASKED_WITHIN: Duration = Duration::from_millis(500);
const POLL_EVERY: Duration = Duration::from_millis(20);
const HELD_INSIDE_A_FILL: Duration = Duration::from_secs(1);
const TIMES_TRIED: usize = 20;

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
            node.name = resonate-teardown-sink
            media.class = Audio/Sink
            audio.format = F32LE
            audio.rate = 48000
            audio.position = [ FL FR ]
            object.linger = true
            adapter.auto-port-config = { mode = passthrough monitor = false control = false position = preserve }
        }
    }
]
"#;

fn spec() -> StreamSpec {
    StreamSpec::new(
        SampleRate::HZ_48000,
        ChannelLayout::Stereo,
        SampleFormat::F32,
    )
}

#[derive(Default)]
struct Watched {
    pulled: AtomicUsize,
    armed: AtomicBool,
    entered: AtomicBool,
    let_go: AtomicBool,
    let_go_inside: AtomicBool,
}

impl Watched {
    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn entered(&self) -> bool {
        self.entered.load(Ordering::Acquire)
    }

    fn let_go(&self) -> bool {
        self.let_go.load(Ordering::Acquire)
    }

    fn let_go_inside(&self) -> bool {
        self.let_go_inside.load(Ordering::Acquire)
    }
}

struct Lingering {
    watched: Arc<Watched>,
    held: Duration,
}

impl AudioSource for Lingering {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        let watched = Arc::clone(&self.watched);
        let held = self.held;

        dst.fill(0);
        watched.pulled.fetch_add(dst.len(), Ordering::Relaxed);

        if watched.armed.swap(false, Ordering::AcqRel) {
            watched.entered.store(true, Ordering::Release);
            thread::sleep(held);
            if watched.let_go() {
                watched.let_go_inside.store(true, Ordering::Release);
            }
        }
        dst.len()
    }
}

impl Drop for Lingering {
    fn drop(&mut self) {
        self.watched.let_go.store(true, Ordering::Release);
    }
}

struct Kept {
    let_go: Arc<AtomicBool>,
}

impl AudioSink for Kept {
    fn take(&mut self, _: &[u8]) {}
}

impl Drop for Kept {
    fn drop(&mut self) {
        self.let_go.store(true, Ordering::Release);
    }
}

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

    fn stop(&self) {
        let stopped = Command::new("kill")
            .args(["-STOP", &self.daemon.id().to_string()])
            .status()
            .is_ok_and(|status| status.success());

        assert!(stopped, "the hosted daemon could not be stopped");
    }

    fn kill(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
    }
}

impl Drop for Hosted {
    fn drop(&mut self) {
        self.kill();
    }
}

fn until(done: impl FnMut() -> bool) -> bool {
    until_within(PATIENCE, done)
}

fn until_within(patience: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + patience;

    while Instant::now() < deadline {
        if done() {
            return true;
        }
        thread::sleep(POLL_EVERY);
    }
    false
}

fn disconnected_among(mut events: impl Iterator<Item = StreamEvent>) -> bool {
    events.any(|event| {
        matches!(
            event,
            StreamEvent::StateChanged {
                to: StreamState::Unconnected,
                ..
            }
        )
    })
}

fn the_hosted_sink(pipewire: &PipeWire) -> SinkInfo {
    let mut found = None;

    until(|| {
        found = pipewire
            .enumerate_sinks(ASKED_WITHIN)
            .ok()
            .and_then(|sinks| {
                sinks
                    .into_iter()
                    .find(|sink| sink.name.as_str() == SINK && !sink.formats.is_empty())
            });
        found.is_some()
    });
    found.expect("the hosted daemon's sink is found with its formats")
}

fn request(sink: &SinkInfo) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec: spec(),
        latency: LatencyRequest::Auto,
        role: MediaRole::Music,
        media_name: "resonate teardown test".to_owned(),
        force_graph_rate: false,
        no_convert: true,
        realtime: true,
    }
}

fn capture_request() -> CaptureRequest {
    CaptureRequest {
        from: Capturing::Desktop {
            sink: Some(NodeName::new(SINK.to_owned())),
        },
        spec: spec(),
        media_name: "resonate teardown capture".to_owned(),
    }
}

fn pw_cli_lists(folder: &Path, kind: &str) -> String {
    Command::new("pw-cli")
        .args(["ls", kind])
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .output()
        .map(|listed| String::from_utf8_lossy(&listed.stdout).into_owned())
        .unwrap_or_default()
}

fn the_nodes_of_class(folder: &Path, class: &str) -> Vec<String> {
    let mut node = None;
    let mut nodes = Vec::new();

    for line in pw_cli_lists(folder, "Node").lines() {
        if let Some(id) = line.trim().strip_prefix("id ") {
            node = id.split(',').next().map(str::to_owned);
        }
        if line.contains(class)
            && let Some(id) = node.take()
        {
            nodes.push(id);
        }
    }
    nodes
}

fn quoted<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.trim()
        .strip_prefix(key)?
        .trim_start()
        .strip_prefix('=')?
        .trim()
        .strip_prefix('"')?
        .strip_suffix('"')
}

fn the_first_output_of(folder: &Path, node: &str) -> Option<String> {
    let mut port = None;
    let mut on_the_node = false;

    for line in pw_cli_lists(folder, "Port").lines() {
        if let Some(id) = line.trim().strip_prefix("id ") {
            port = id.split(',').next().map(str::to_owned);
            on_the_node = false;
        }
        if quoted(line, "node.id") == Some(node) {
            on_the_node = true;
        }
        if on_the_node && quoted(line, "port.name") == Some("output_1") {
            return port;
        }
    }
    None
}

fn succeeds(command: &mut Command, folder: &Path) -> bool {
    command
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn ported(folder: &Path, node: &str) -> bool {
    let config = "{ direction: Output, mode: passthrough, monitor: false, control: false, \
                  format: { mediaType: audio, mediaSubtype: raw, format: F32LE, rate: 48000, \
                  channels: 2, position: [ FL FR ] } }";
    succeeds(
        Command::new("pw-cli").args(["set-param", node, "PortConfig", config]),
        folder,
    )
}

fn linked(folder: &Path, node: &str) -> bool {
    the_first_output_of(folder, node).is_some_and(|port| {
        succeeds(
            Command::new("pw-link")
                .arg(port)
                .arg(format!("{SINK}:playback_1")),
            folder,
        )
    })
}

fn a_quarter_second() -> usize {
    let frame = usize::from(spec().channel_count().get())
        * usize::from(spec().format.bytes_per_sample().get());
    spec().rate.hz() as usize / 4 * frame
}

struct Flowing {
    pipewire: PipeWire,
    stream: SinkStream,
    watched: Arc<Watched>,
}

fn a_stream_held_inside_a_fill(folder: &Path, held: Duration) -> Flowing {
    let pipewire = PipeWire::start(CLIENT).expect("the hosted daemon answers");
    let sink = the_hosted_sink(&pipewire);
    let watched = Arc::new(Watched::default());

    let stream = pipewire
        .open(
            &request(&sink),
            Box::new(Lingering {
                watched: Arc::clone(&watched),
                held,
            }),
        )
        .expect("the stream opens");

    stream.set_active(true).expect("the stream activates");

    let mut node = None;

    assert!(
        until(|| {
            node = the_nodes_of_class(folder, "Stream/Output/Audio")
                .into_iter()
                .next();
            node.is_some()
        }),
        "the stream never stood in the graph"
    );

    let node = node.unwrap_or_default();

    assert!(
        until(|| ported(folder, &node)),
        "the stream was given no ports"
    );
    assert!(until(|| linked(folder, &node)), "the stream never linked");
    assert!(
        until(|| watched.pulled.load(Ordering::Relaxed) >= a_quarter_second()),
        "the stream was never pulled"
    );

    watched.arm();

    assert!(
        until(|| watched.entered()),
        "the data thread never entered the fill held for it"
    );
    Flowing {
        pipewire,
        stream,
        watched,
    }
}

fn shut_down_inside_a_fill(folder: &Path, hosted: Hosted) {
    let Flowing {
        pipewire,
        stream,
        watched,
    } = a_stream_held_inside_a_fill(folder, HELD_INSIDE_A_FILL);

    hosted.stop();

    let _ = pipewire.shutdown();

    let let_go_by_the_shutdown = watched.let_go();
    let heard_disconnected = disconnected_among(stream.events().try_iter());

    drop(stream);

    assert!(
        let_go_by_the_shutdown,
        "the shutdown kept the stream's source"
    );
    assert!(
        heard_disconnected,
        "the stream's listener was let go before the stream was disconnected"
    );
    assert!(
        !watched.let_go_inside(),
        "the source was let go while the data thread was still filling from it"
    );
}

fn graph_lost_inside_a_fill(folder: &Path, mut hosted: Hosted) {
    let Flowing {
        pipewire,
        stream,
        watched,
    } = a_stream_held_inside_a_fill(folder, HELD_INSIDE_A_FILL);

    hosted.kill();

    let let_go_by_the_loss = until(|| watched.let_go());

    drop(stream);

    let _ = pipewire.shutdown();

    assert!(
        let_go_by_the_loss,
        "the lost graph kept the stream's source"
    );
    assert!(
        !watched.let_go_inside(),
        "the source was let go while the data thread was still filling from it"
    );
}

fn graph_gone_silent(_: &Path, hosted: Hosted) {
    let pipewire = PipeWire::start(CLIENT).expect("the hosted daemon answers");
    let sink = the_hosted_sink(&pipewire);
    let watched = Arc::new(Watched::default());

    let stream = pipewire
        .open(
            &request(&sink),
            Box::new(Lingering {
                watched: Arc::clone(&watched),
                held: Duration::ZERO,
            }),
        )
        .expect("the stream opens");
    let events = stream.events().clone();

    stream.set_active(true).expect("the stream activates");
    hosted.stop();

    let let_go_by_the_silence = until_within(SILENT_PATIENCE, || watched.let_go());
    let heard_disconnected = disconnected_among(events.try_iter());

    drop(stream);

    let _ = pipewire.shutdown();

    assert!(
        let_go_by_the_silence,
        "a graph that stopped answering kept the stream's source"
    );
    assert!(
        heard_disconnected,
        "the stream's listener was let go before the stream was disconnected"
    );
}

fn capture_given_up_on(folder: &Path, _: Hosted) {
    let pipewire = PipeWire::start(CLIENT).expect("the hosted daemon answers");

    the_hosted_sink(&pipewire);

    let kept_let_go = Arc::new(AtomicBool::new(false));
    let mut given_up = None;

    for _ in 0..TIMES_TRIED {
        let let_go = Arc::new(AtomicBool::new(false));

        match pipewire.capture_within(
            &capture_request(),
            Box::new(Kept {
                let_go: Arc::clone(&let_go),
            }),
            Duration::ZERO,
        ) {
            Ok(answered) => answered.stop().expect("a capture answered in time stops"),
            Err(error) => {
                given_up = Some((error, let_go));
                break;
            }
        }
    }

    let caught_up = pipewire.enumerate_sinks(PATIENCE);
    let abandoned_dropped = given_up
        .as_ref()
        .is_some_and(|(_, let_go)| let_go.load(Ordering::Acquire));
    let none_left = the_nodes_of_class(folder, "Stream/Input/Audio").is_empty();

    let kept = pipewire
        .capture(
            &capture_request(),
            Box::new(Kept {
                let_go: Arc::clone(&kept_let_go),
            }),
        )
        .expect("a capture waited for opens");
    let kept_stands = until(|| the_nodes_of_class(folder, "Stream/Input/Audio").len() == 1);
    let kept_held = !kept_let_go.load(Ordering::Acquire);
    let events = kept.events().clone();

    kept.stop().expect("the capture stops");

    let stopped_heard_disconnected = until(|| disconnected_among(events.try_iter()));
    let stopped_let_go = until(|| kept_let_go.load(Ordering::Acquire));

    let _ = pipewire.shutdown();

    assert!(
        matches!(given_up, Some((Error::OpenUnanswered, _))),
        "no capture was given up on as unanswered: {:?}",
        given_up.map(|(error, _)| error)
    );
    assert!(caught_up.is_ok(), "the client stopped answering");
    assert!(
        abandoned_dropped,
        "the loop kept the sink of a capture its asker gave up on"
    );
    assert!(
        none_left,
        "a capture its asker gave up on stood in the graph"
    );
    assert!(kept_stands, "a capture waited for never stood in the graph");
    assert!(kept_held, "a capture waited for let its sink go");
    assert!(
        stopped_heard_disconnected,
        "a stopped capture's listener was let go before the capture was disconnected"
    );
    assert!(stopped_let_go, "a stopped capture kept its sink");
}

fn closed_while_linked(folder: &Path, _: Hosted) {
    let Flowing {
        pipewire,
        stream,
        watched,
    } = a_stream_held_inside_a_fill(folder, Duration::ZERO);
    let events = stream.events().clone();

    stream.close().expect("the stream closes");

    let let_go = until(|| watched.let_go());
    let heard_disconnected = disconnected_among(events.try_iter());
    let none_left = until(|| the_nodes_of_class(folder, "Stream/Output/Audio").is_empty());

    let _ = pipewire.shutdown();

    assert!(let_go, "the closed stream kept its source");
    assert!(
        heard_disconnected,
        "the stream's listener was let go before the stream was disconnected"
    );
    assert!(none_left, "the closed stream stayed in the graph");
}

fn threads() -> usize {
    fs::read_dir("/proc/self/task").map_or(0, Iterator::count)
}

fn clients_named(folder: &Path, name: &str) -> usize {
    let quoted_name = format!("\"{name}\"");
    pw_cli_lists(folder, "Client")
        .lines()
        .filter(|line| line.contains("application.name") && line.contains(&quoted_name))
        .count()
}

fn start_given_up_on(folder: &Path, _: Hosted) {
    let before = threads();
    let mut given_up = false;

    for _ in 0..TIMES_TRIED {
        match PipeWire::start_within(CLIENT, Duration::ZERO) {
            Err(Error::LoopStopped) => {
                given_up = true;
                break;
            }
            Ok(started) => {
                let _ = started.shutdown();
            }
            Err(error) => panic!("the start failed otherwise than by its wait: {error}"),
        }
    }

    let no_thread_left = until(|| threads() <= before);
    let no_client_left = until(|| clients_named(folder, CLIENT) == 0);

    assert!(given_up, "no start was ever given up on");
    assert!(
        no_thread_left,
        "{} threads outlived a start given up on, {before} before it",
        threads()
    );
    assert!(
        no_client_left,
        "the daemon still held the client of a start given up on"
    );
}

#[test]
fn a_client_shut_down_while_the_data_thread_fills_lets_the_source_go_only_once_it_is_out() {
    hosted_as(
        "a_client_shut_down_while_the_data_thread_fills_lets_the_source_go_only_once_it_is_out",
        "s",
        shut_down_inside_a_fill,
    );
}

#[test]
fn a_stream_closed_while_linked_hears_itself_disconnected_and_leaves_the_graph() {
    hosted_as(
        "a_stream_closed_while_linked_hears_itself_disconnected_and_leaves_the_graph",
        "x",
        closed_while_linked,
    );
}

#[test]
fn a_graph_lost_while_the_data_thread_fills_lets_the_source_go_only_once_it_is_out() {
    hosted_as(
        "a_graph_lost_while_the_data_thread_fills_lets_the_source_go_only_once_it_is_out",
        "l",
        graph_lost_inside_a_fill,
    );
}

#[test]
fn a_graph_gone_silent_disconnects_its_stream_before_letting_the_listener_go() {
    hosted_as(
        "a_graph_gone_silent_disconnects_its_stream_before_letting_the_listener_go",
        "q",
        graph_gone_silent,
    );
}

#[test]
fn a_capture_answered_after_its_asker_gave_up_is_dropped_and_never_stands_in_the_graph() {
    hosted_as(
        "a_capture_answered_after_its_asker_gave_up_is_dropped_and_never_stands_in_the_graph",
        "c",
        capture_given_up_on,
    );
}

#[test]
fn a_start_given_up_on_leaves_no_thread_and_no_client_behind() {
    hosted_as(
        "a_start_given_up_on_leaves_no_thread_and_no_client_behind",
        "t",
        start_given_up_on,
    );
}

fn hosted_as(this_test: &str, tag: &str, under: fn(&Path, Hosted)) {
    if let Some(folder) = env::var_os(HOSTED_AT).map(PathBuf::from) {
        let Some(hosted) = Hosted::start(&folder) else {
            eprintln!("skipped: no pipewire binary to host a daemon with");
            return;
        };

        under(&folder, hosted);
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

    let folder = env::temp_dir().join(format!("rpt-{}-{tag}", std::process::id()));

    fs::create_dir_all(&folder).expect("a folder for the hosted daemon");
    fs::write(folder.join("hosted.conf"), HOSTED_CONFIG).expect("the hosted daemon's config");

    let ran = Command::new(env::current_exe().expect("the test binary"))
        .args([in_this_suite(this_test).as_str(), "--exact", "--nocapture"])
        .env(HOSTED_AT, &folder)
        .env("PIPEWIRE_RUNTIME_DIR", &folder)
        .status()
        .expect("the test binary runs again under the hosted daemon");
    let _ = fs::remove_dir_all(&folder);

    assert!(ran.success(), "the run under the hosted daemon failed");
}
