use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
use resonate_pipewire::{
    AudioSource, LatencyRequest, MediaRole, PipeWire, SinkChange, SinkInfo, StreamEvent,
    StreamRequest, StreamState, Words,
};

const HOSTED_AT: &str = "RESONATE_HOSTED_PIPEWIRE";
const SOCKET: &str = "pipewire-0";
const PATIENCE: Duration = Duration::from_secs(10);
const ASKED_WITHIN: Duration = Duration::from_millis(500);
const POLL_EVERY: Duration = Duration::from_millis(20);

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
            node.name = resonate-sixteen-surround
            media.class = Audio/Sink
            audio.format = S16LE
            audio.rate = 44100
            audio.position = [ FL FR FC LFE RL RR ]
            object.linger = true
        }
    }
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = resonate-thirty-two-surround
            media.class = Audio/Sink
            audio.format = S32LE
            audio.rate = 96000
            audio.position = [ FL FR FC LFE RL RR SL SR ]
            object.linger = true
        }
    }
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = resonate-twenty-four-packed
            media.class = Audio/Sink
            audio.format = S24LE
            audio.rate = 48000
            audio.position = [ FL FR ]
            object.linger = true
        }
    }
    { factory = adapter
        args = {
            factory.name = support.null-audio-sink
            node.name = resonate-twenty-four-padded
            media.class = Audio/Sink
            audio.format = S24_32LE
            audio.rate = 48000
            audio.position = [ FL FR ]
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

#[derive(Clone, Copy, Debug)]
struct Hosting {
    sink: &'static str,
    spec: StreamSpec,
    words: Words,
}

const HOSTED: [Hosting; 4] = [
    Hosting {
        sink: "resonate-sixteen-surround",
        spec: StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::Surround51,
            SampleFormat::S16,
        ),
        words: Words::Whole,
    },
    Hosting {
        sink: "resonate-thirty-two-surround",
        spec: StreamSpec::new(
            SampleRate::HZ_96000,
            ChannelLayout::Surround71,
            SampleFormat::S32,
        ),
        words: Words::Whole,
    },
    Hosting {
        sink: "resonate-twenty-four-packed",
        spec: StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        words: Words::Packed,
    },
    Hosting {
        sink: "resonate-twenty-four-padded",
        spec: StreamSpec::new(
            SampleRate::HZ_48000,
            ChannelLayout::Stereo,
            SampleFormat::S24,
        ),
        words: Words::Padded,
    },
];

struct Silence;

impl AudioSource for Silence {
    fn fill(&mut self, dst: &mut [u8]) -> usize {
        dst.fill(0);
        dst.len()
    }
}

fn request(sink: &SinkInfo, spec: StreamSpec) -> StreamRequest {
    StreamRequest {
        target: Some(sink.id),
        spec,
        latency: LatencyRequest::Auto,
        role: MediaRole::Music,
        media_name: "resonate formats test".to_owned(),
        force_graph_rate: false,
        no_convert: true,
        realtime: false,
    }
}

fn the_hosted_sinks(pipewire: &PipeWire) -> Vec<SinkInfo> {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if let Ok(sinks) = pipewire.enumerate_sinks(ASKED_WITHIN)
            && HOSTED.iter().all(|hosting| {
                sinks
                    .iter()
                    .any(|sink| sink.name.as_str() == hosting.sink && !sink.formats.is_empty())
            })
        {
            return sinks;
        }
        thread::sleep(POLL_EVERY);
    }
    panic!("the hosted daemon's sinks were never all found with their formats");
}

fn named<'a>(sinks: &'a [SinkInfo], name: &str) -> &'a SinkInfo {
    sinks
        .iter()
        .find(|sink| sink.name.as_str() == name)
        .expect("a hosted sink")
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

fn every_word_and_map_is_read_as_the_sink_names_it(_: &Path) {
    let pipewire = PipeWire::start("resonate-formats-test").expect("the hosted daemon answers");
    let sinks = the_hosted_sinks(&pipewire);
    let _ = pipewire.shutdown();

    for hosting in HOSTED {
        let sink = named(&sinks, hosting.sink);
        let entry = sink
            .formats
            .iter()
            .find(|entry| entry.format == hosting.spec.format)
            .unwrap_or_else(|| panic!("{}: no {:?} entry", hosting.sink, hosting.spec.format));
        assert_eq!(entry.words, hosting.words, "{}", hosting.sink);
        assert_eq!(entry.rates, [hosting.spec.rate], "{}", hosting.sink);
        assert!(
            entry.takes(hosting.spec.channels),
            "{}: {:?} is not among {:?}",
            hosting.sink,
            hosting.spec.channels,
            entry.channels
        );
        assert_eq!(
            sink.formats.len(),
            1,
            "{}: {:?}",
            hosting.sink,
            sink.formats
        );
    }
}

fn every_word_and_map_is_taken_by_the_daemon_with_nothing_converting(_: &Path) {
    let pipewire = PipeWire::start("resonate-formats-test").expect("the hosted daemon answers");
    let sinks = the_hosted_sinks(&pipewire);

    for hosting in HOSTED {
        let sink = named(&sinks, hosting.sink);
        let stream = pipewire
            .open(&request(sink, hosting.spec), Box::new(Silence))
            .unwrap_or_else(|error| panic!("{}: the stream would not open: {error}", hosting.sink));
        stream.set_active(true).expect("the stream activates");

        let mut heard = Vec::new();
        let taken = until(|| {
            heard.extend(stream.events().try_iter());
            heard.iter().any(|event| {
                matches!(
                    event,
                    StreamEvent::StateChanged {
                        to: StreamState::Paused | StreamState::Streaming,
                        ..
                    }
                )
            })
        });
        stream.close().expect("the stream closes");

        assert!(taken, "{}: never taken: {heard:?}", hosting.sink);
        assert!(
            !heard.iter().any(|event| matches!(
                event,
                StreamEvent::StateChanged {
                    to: StreamState::Failed,
                    ..
                }
            )),
            "{}: refused: {heard:?}",
            hosting.sink
        );
    }
    let _ = pipewire.shutdown();
}

fn a_sink_leaving_under_an_open_stream(folder: &Path) {
    let pipewire = PipeWire::start("resonate-formats-test").expect("the hosted daemon answers");
    let changes = pipewire.subscribe_sinks();
    let sinks = the_hosted_sinks(&pipewire);
    let hosting = HOSTED[2];
    let sink = named(&sinks, hosting.sink).clone();

    let stream = pipewire
        .open(&request(&sink, hosting.spec), Box::new(Silence))
        .expect("the stream opens");
    stream.set_active(true).expect("the stream activates");

    let destroyed = Command::new("pw-cli")
        .args(["destroy", hosting.sink])
        .env("PIPEWIRE_RUNTIME_DIR", folder)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    assert!(destroyed, "pw-cli could not take the sink away");

    let announced = until(|| {
        changes
            .try_iter()
            .any(|change| change == SinkChange::Removed(sink.id))
    });
    let gone_from_the_list = until(|| {
        pipewire
            .enumerate_sinks(ASKED_WITHIN)
            .is_ok_and(|sinks| sinks.iter().all(|listed| listed.id != sink.id))
    });
    let closed = stream.close();
    let others_still_listed = pipewire.enumerate_sinks(ASKED_WITHIN).is_ok_and(|sinks| {
        sinks
            .iter()
            .any(|listed| listed.name.as_str() == HOSTED[0].sink)
    });
    let _ = pipewire.shutdown();

    assert!(announced, "the sink's leaving was never announced");
    assert!(gone_from_the_list, "the sink stayed listed after it left");
    assert!(closed.is_ok(), "the stream whose sink left would not close");
    assert!(
        others_still_listed,
        "the client stopped answering once a sink left"
    );
}

#[test]
fn every_word_and_channel_map_a_sink_advertises_is_read_as_it_names_it() {
    hosted_as(
        "every_word_and_channel_map_a_sink_advertises_is_read_as_it_names_it",
        "w",
        every_word_and_map_is_read_as_the_sink_names_it,
    );
}

#[test]
fn a_stream_in_every_word_and_channel_map_is_taken_by_the_daemon_with_nothing_converting() {
    hosted_as(
        "a_stream_in_every_word_and_channel_map_is_taken_by_the_daemon_with_nothing_converting",
        "s",
        every_word_and_map_is_taken_by_the_daemon_with_nothing_converting,
    );
}

#[test]
fn a_sink_leaving_under_an_open_stream_is_announced_and_the_stream_still_closes() {
    hosted_as(
        "a_sink_leaving_under_an_open_stream_is_announced_and_the_stream_still_closes",
        "l",
        a_sink_leaving_under_an_open_stream,
    );
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

    let folder = env::temp_dir().join(format!("rpf-{}-{tag}", std::process::id()));
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
