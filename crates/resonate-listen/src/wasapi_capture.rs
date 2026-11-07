use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{RecvTimeoutError, SyncSender, sync_channel},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use resonate_audio::{Error, Operation, Result};
use wasapi::{
    AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, Handle, SampleType, StreamMode,
    WasapiError, WaveFormat,
};

use crate::{
    endpoints::Tap,
    recording::Recorder,
    session::{BYTES_PER_FRAME, HEARD_AT, HEARD_IN_CHANNELS, Held, frames_in, zeroed_if_silent},
    wasapi_apartment::{Apartment, backend_error, initialize_error},
};

const OPEN_WITHIN: Duration = Duration::from_secs(5);
const EVENT_WAIT_MS: u32 = 100;
const FLOAT_BITS: usize = 32;
const FRONT_LEFT_AND_RIGHT: u32 = 0x3;
const DEFAULT_BUFFER: i64 = 0;

pub(crate) struct Capture {
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<()>>>,
}

struct Opened {
    client: AudioClient,
    capture: AudioCaptureClient,
    event: Handle,
    scratch: Vec<u8>,
}

struct Running {
    stop: Arc<AtomicBool>,
    lost: Arc<AtomicBool>,
    quiet_is_silence: bool,
}

impl Capture {
    pub(crate) fn open(tap: &Tap, recorder: Recorder) -> Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let lost = Arc::new(AtomicBool::new(false));
        let (ready, started) = sync_channel(1);
        let running = Running {
            stop: Arc::clone(&stop),
            lost: Arc::clone(&lost),
            quiet_is_silence: matches!(tap, Tap::Desktop),
        };
        let tap = tap.clone();
        let thread = thread::Builder::new()
            .name("resonate-listen-capture".into())
            .spawn(move || run(&tap, recorder, &ready, &running))
            .map_err(|source| Error::Backend {
                op: Operation::Open,
                source,
            })?;
        match started.recv_timeout(OPEN_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                stop,
                lost,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(RecvTimeoutError::Timeout) => {
                stop.store(true, Ordering::Release);
                Err(Error::OpenUnanswered)
            }
            Err(RecvTimeoutError::Disconnected) => Err(Error::LoopStopped),
        }
    }

    pub(crate) fn stop(mut self) -> Result<()> {
        self.stop.store(true, Ordering::Release);
        match self.thread.take() {
            Some(thread) => thread.join().unwrap_or(Err(Error::LoopStopped)),
            None => Ok(()),
        }
    }
}

impl Held for Capture {
    fn is_lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run(
    tap: &Tap,
    recorder: Recorder,
    ready: &SyncSender<Result<()>>,
    running: &Running,
) -> Result<()> {
    let _apartment = Apartment::enter();
    let opened = match opened_for(tap) {
        Ok(opened) => opened,
        Err(error) => {
            let _ = ready.send(Err(error));
            return Ok(());
        }
    };
    let _ = ready.send(Ok(()));
    let served = serve(opened, recorder, running);
    if let Err(error) = &served {
        tracing::warn!(%error, "a WASAPI capture failed");
        running.lost.store(true, Ordering::Release);
    }
    served
}

fn heard_wave() -> WaveFormat {
    WaveFormat::new(
        FLOAT_BITS,
        FLOAT_BITS,
        SampleType::Float,
        HEARD_AT.hz() as usize,
        HEARD_IN_CHANNELS,
        Some(FRONT_LEFT_AND_RIGHT),
    )
}

fn opened_for(tap: &Tap) -> Result<Opened> {
    let enumerator = DeviceEnumerator::new().map_err(|e| backend_error(Operation::Connect, e))?;
    let device = match tap {
        Tap::Desktop => enumerator.get_default_device(Direction::Render),
        Tap::Microphone(None) => enumerator.get_default_device(Direction::Capture),
        Tap::Microphone(Some(id)) => enumerator.get_device(id),
    }
    .map_err(|e| backend_error(Operation::Open, e))?;
    let mut client = device
        .get_iaudioclient()
        .map_err(|e| backend_error(Operation::Open, e))?;
    let mode = StreamMode::EventsShared {
        autoconvert: true,
        buffer_duration_hns: DEFAULT_BUFFER,
    };
    client
        .initialize_client(&heard_wave(), Direction::Capture, mode, None)
        .map_err(|e| initialize_error(Operation::Format, e))?;
    let event = client
        .set_get_eventhandle()
        .map_err(|e| backend_error(Operation::Open, e))?;
    let capture = client
        .get_audiocaptureclient()
        .map_err(|e| backend_error(Operation::Open, e))?;
    let buffer_frames = client
        .get_buffer_size()
        .map_err(|e| backend_error(Operation::Open, e))?;
    let scratch = vec![0_u8; BYTES_PER_FRAME * usize::try_from(buffer_frames).unwrap_or_default()];
    Ok(Opened {
        client,
        capture,
        event,
        scratch,
    })
}

fn serve(opened: Opened, mut recorder: Recorder, running: &Running) -> Result<()> {
    let Opened {
        client,
        capture,
        event,
        mut scratch,
    } = opened;
    client
        .start_stream()
        .map_err(|e| backend_error(Operation::Start, e))?;
    let mut turned = Instant::now();
    while !running.stop.load(Ordering::Acquire) {
        let _ = event.wait_for_event(EVENT_WAIT_MS);
        let heard = drained(&capture, &mut scratch, &mut recorder)
            .map_err(|e| backend_error(Operation::Start, e))?;
        let now = Instant::now();
        if heard == 0 && running.quiet_is_silence {
            recorder.pad(frames_in(now - turned), HEARD_IN_CHANNELS);
        }
        turned = now;
    }
    client
        .stop_stream()
        .map_err(|e| backend_error(Operation::Close, e))
}

fn drained(
    capture: &AudioCaptureClient,
    scratch: &mut [u8],
    recorder: &mut Recorder,
) -> std::result::Result<usize, WasapiError> {
    let mut frames = 0;
    loop {
        let waiting = capture.get_next_packet_size()?.unwrap_or(0);
        if waiting == 0 {
            return Ok(frames);
        }
        let (read, info) = capture.read_from_device(scratch)?;
        let read = usize::try_from(read).unwrap_or_default();
        let Some(window) = scratch.get_mut(..read * BYTES_PER_FRAME) else {
            return Ok(frames);
        };
        if read == 0 {
            return Ok(frames);
        }
        zeroed_if_silent(window, info.flags.silent);
        recorder.admit(window);
        frames += read;
    }
}
