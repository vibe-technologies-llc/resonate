use std::{
    mem,
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
    decoder.follow_format_changes();
    let mut meter = meter_for(info.spec);
    let mut loudest = 0.0_f64;
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
            Ok(DecodeStatus::Reformatted(sounding)) => {
                loudest = loudest.max(mem::replace(&mut meter, meter_for(sounding)).finish());
            }
            Ok(DecodeStatus::Decoded) => {
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
    Some(Gain::new(loudest.max(meter.finish()) as f32).ok())
}

fn meter_for(spec: StreamSpec) -> TruePeakMeter {
    let channels =
        NonZeroUsize::new(usize::from(spec.channels.count().get())).unwrap_or(NonZeroUsize::MIN);
    TruePeakMeter::at(spec.rate, channels)
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        process::{self, Command, Stdio},
    };

    use super::*;

    const SINE_SOURCE_AMPLITUDE: f32 = 0.125;
    const QUIET: f32 = 0.1;
    const LOUD: f32 = 0.8;
    const PEAK_AGREES_WITHIN: f32 = 0.08;

    struct Link {
        rate: u32,
        amplitude: f32,
        twinned: bool,
    }

    struct Scratch(PathBuf);

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn chained(scratch: &Path, links: &[Link]) -> Option<PathBuf> {
        let mut whole = Vec::new();
        for (index, link) in links.iter().enumerate() {
            let part = scratch.join(format!("{index}.ogg"));
            let tone = format!(
                "sine=frequency=997:sample_rate={}:duration=1,volume={}",
                link.rate,
                link.amplitude / SINE_SOURCE_AMPLITUDE
            );
            let filter = if link.twinned {
                "pan=stereo|c0=c0|c1=c0"
            } else {
                "anull"
            };
            let made = Command::new("ffmpeg")
                .args(["-y", "-v", "error", "-f", "lavfi", "-i", &tone])
                .args(["-af", filter])
                .args(["-c:a", "libvorbis", "-q:a", "8", "-f", "ogg"])
                .arg(&part)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if !made {
                eprintln!("skipped: no ffmpeg with libvorbis to write a link");
                return None;
            }
            whole.extend_from_slice(&fs::read(&part).ok()?);
        }
        let path = scratch.join("chained.ogg");
        fs::write(&path, whole).ok()?;
        Some(path)
    }

    #[test]
    fn a_peak_is_the_loudest_of_every_shape_a_stream_turns_through() {
        let scratch = Scratch(env::temp_dir().join(format!("resonate-peak-{}", process::id())));
        fs::create_dir_all(&scratch.0).expect("a scratch folder");
        let Some(path) = chained(
            &scratch.0,
            &[
                Link {
                    rate: 44_100,
                    amplitude: QUIET,
                    twinned: false,
                },
                Link {
                    rate: 48_000,
                    amplitude: LOUD,
                    twinned: true,
                },
            ],
        ) else {
            return;
        };

        let peak = loudest(
            &Sources::local(),
            &MediaLocation::local(&path),
            None,
            &AtomicBool::new(false),
        );

        let Some(Some(peak)) = peak else {
            panic!("a stream that turns went unmeasured: {peak:?}");
        };
        assert!(
            (peak.get() - LOUD).abs() < PEAK_AGREES_WITHIN,
            "{}",
            peak.get()
        );
    }
}
