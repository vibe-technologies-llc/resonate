use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use parking_lot::Mutex;
use resonate_codec::{DecodeStatus, Decoder, MediaInfo, Packing, Sources};
use resonate_core::{
    AppliedGain, AudioBuffer, FrameSpan, Gain, MediaLocation, SampleData, SampleFormat, StreamSpec,
};
use resonate_dsp::TruePeakMeter;

use crate::EngineConfig;

pub(crate) struct Measuring {
    landed: Arc<Mutex<Option<Measured>>>,
    stop: Arc<AtomicBool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Measured {
    Peaking(Gain),
    Unmeasured,
}

impl Measuring {
    pub(crate) fn wanted(config: &EngineConfig, info: &MediaInfo, gain: AppliedGain) -> bool {
        config.true_peak
            && gain.peak.is_none()
            && info.packing == Packing::Samples
            && (gain.requested() > Gain::UNITY || info.spec.format.is_float())
    }

    pub(crate) fn start(
        sources: Arc<Sources>,
        location: MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<Self> {
        let landed = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let lands = Arc::clone(&landed);
        let stopped = Arc::clone(&stop);
        let spawned = thread::Builder::new()
            .name("resonate-peak".into())
            .spawn(move || {
                let measured = match loudest(&sources, &location, span, &stopped) {
                    Some(Some(peak)) => Measured::Peaking(peak),
                    Some(None) => Measured::Unmeasured,
                    None => return,
                };
                *lands.lock() = Some(measured);
            });
        match spawned {
            Ok(_) => Some(Self { landed, stop }),
            Err(error) => {
                tracing::debug!(%error, "the thread that measures a track's peak could not start");
                None
            }
        }
    }

    pub(crate) fn landed(&self) -> Option<Measured> {
        self.landed.lock().take()
    }
}

impl Drop for Measuring {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn loudest(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    stop: &AtomicBool,
) -> Option<Option<Gain>> {
    let opened = match span {
        Some(span) => Decoder::open_span(sources, location, span),
        None => Decoder::open(sources, location),
    };
    let (mut decoder, info) = match opened {
        Ok(opened) => opened,
        Err(error) => {
            tracing::debug!(%error, %location, "a track's peak could not be measured");
            return Some(None);
        }
    };
    decoder.set_output_format(SampleFormat::F32);
    let channels = NonZeroUsize::new(usize::from(info.spec.channels.count().get()))
        .unwrap_or(NonZeroUsize::MIN);
    let mut meter = TruePeakMeter::new(channels);
    let mut block = AudioBuffer::empty(StreamSpec::new(
        info.spec.rate,
        info.spec.channels,
        SampleFormat::F32,
    ));
    loop {
        if stop.load(Ordering::Relaxed) {
            return None;
        }
        match decoder.next_block(&mut block) {
            Ok(DecodeStatus::EndOfStream) => break,
            Ok(_) => {
                if let SampleData::F32(samples) = block.data() {
                    meter.note(samples);
                }
            }
            Err(error) => {
                tracing::debug!(%error, %location, "a track's peak could not be measured to its end");
                return Some(None);
            }
        }
    }
    Some(Gain::new(meter.finish() as f32).ok())
}
