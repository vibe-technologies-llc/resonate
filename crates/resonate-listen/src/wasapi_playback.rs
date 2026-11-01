use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use resonate_audio::{AudioSource, Error as AudioError, Operation};
use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, Handle, SampleType, StreamMode,
    WasapiError, WaveFormat,
};

use crate::{
    CaptureOp, Error, Result,
    endpoints::Output,
    session::{BYTES_PER_FRAME, HEARD_AT, HEARD_IN_CHANNELS, Played},
    wasapi_apartment::{Apartment, backend_error, initialize_error},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const EVENT_WAIT_MS: u32 = 100;
const FLOAT_BITS: usize = 32;
const FRONT_LEFT_AND_RIGHT: u32 = 0x3;
const DEFAULT_BUFFER: i64 = 0;

pub(crate) struct Playback {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Rendering {
    client: AudioClient,
    render: AudioRenderClient,
    event: Handle,
    scratch: Vec<u8>,
}

impl Playback {
    pub(crate) fn open(output: Output, played: Played) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, started) = sync_channel(1);
        let stopping = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("resonate-listen-playback".into())
            .spawn(move || run(&output, played, &ready, &stopping))
            .map_err(|source| {
                Error::capture(
                    CaptureOp::Play,
                    AudioError::Backend {
                        op: Operation::Open,
                        source,
                    },
                )
            })?;
        match started.recv_timeout(OPEN_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::Release);
                Err(Error::capture(CaptureOp::Play, AudioError::OpenUnanswered))
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(Error::capture(CaptureOp::Play, AudioError::LoopStopped))
            }
        }
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(output: &Output, mut played: Played, ready: &SyncSender<Result<()>>, stop: &AtomicBool) {
    let _apartment = Apartment::enter();
    let rendering = match rendering_for(output) {
        Ok(rendering) => rendering,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let Rendering {
        client,
        render,
        event,
        mut scratch,
    } = rendering;
    let started = filled(&client, &render, &mut scratch, &mut played).and_then(|()| {
        client
            .start_stream()
            .map_err(|e| backend_error(Operation::Start, e))
            .map_err(|source| Error::Output {
                source: Box::new(source),
            })
    });
    if let Err(error) = started {
        let _ = ready.send(Err(error));
        return;
    }
    let _ = ready.send(Ok(()));
    while !stop.load(Ordering::Acquire) {
        let _ = event.wait_for_event(EVENT_WAIT_MS);
        if let Err(error) = filled(&client, &render, &mut scratch, &mut played) {
            tracing::warn!(%error, "the measuring sound could not carry on");
            break;
        }
    }
    let _ = client.stop_stream();
}

fn heard_wave() -> WaveFormat {
    WaveFormat::new(
        FLOAT_BITS,
        FLOAT_BITS,
        &SampleType::Float,
        HEARD_AT.hz() as usize,
        HEARD_IN_CHANNELS,
        Some(FRONT_LEFT_AND_RIGHT),
    )
}

fn rendering_for(output: &Output) -> Result<Rendering> {
    let play = |error: WasapiError, op| Error::capture(CaptureOp::Play, backend_error(op, error));
    let enumerator = DeviceEnumerator::new()
        .map_err(|e| Error::capture(CaptureOp::Connect, backend_error(Operation::Connect, e)))?;
    let device = match output {
        Output::Default => enumerator
            .get_default_device(&Direction::Render)
            .map_err(|_| Error::NoOutput)?,
        Output::Endpoint { name, id } => enumerator
            .get_device(id)
            .map_err(|_| Error::NoSuchSink { name: name.clone() })?,
    };
    let mut client = device
        .get_iaudioclient()
        .map_err(|e| play(e, Operation::Open))?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: DEFAULT_BUFFER,
    };
    client
        .initialize_client(&heard_wave(), &Direction::Render, &mode)
        .map_err(|e| Error::capture(CaptureOp::Play, initialize_error(Operation::Format, e)))?;
    let event = client
        .set_get_eventhandle()
        .map_err(|e| play(e, Operation::Open))?;
    let render = client
        .get_audiorenderclient()
        .map_err(|e| play(e, Operation::Open))?;
    let buffer_frames = client
        .get_buffer_size()
        .map_err(|e| play(e, Operation::Open))?;
    let scratch = vec![0_u8; BYTES_PER_FRAME * usize::try_from(buffer_frames).unwrap_or_default()];
    Ok(Rendering {
        client,
        render,
        event,
        scratch,
    })
}

fn filled(
    client: &AudioClient,
    render: &AudioRenderClient,
    scratch: &mut [u8],
    played: &mut Played,
) -> Result<()> {
    let output = |error: WasapiError| Error::Output {
        source: Box::new(backend_error(Operation::Start, error)),
    };
    let available = client.get_available_space_in_frames().map_err(output)?;
    let wanted =
        (usize::try_from(available).unwrap_or_default() * BYTES_PER_FRAME).min(scratch.len());
    let Some(block) = scratch.get_mut(..wanted) else {
        return Ok(());
    };
    let frames = played.fill(block) / BYTES_PER_FRAME;
    let Some(window) = block.get(..frames * BYTES_PER_FRAME) else {
        return Ok(());
    };
    if frames == 0 {
        return Ok(());
    }
    render.write_to_device(frames, window, None).map_err(output)
}
