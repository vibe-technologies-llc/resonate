use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

use resonate_codec::{Codec, Sources};
use resonate_core::{
    FrameSpan, MediaLocation, SampleFormat, SampleRate, StreamSpec,
    eq::{Preamp, Profile},
};
use resonate_dsp::{Equaliser, Processor, TruePeakMeter};

use crate::{
    Error, Result, Watching,
    analysis::{native_format, opened},
    conform::Walking,
};

pub const CUTS_AT_MOST: usize = 32;

const FRAMES_AT_A_TIME: usize = 4_096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Loudest {
    pub peak: f64,
    pub measured: usize,
    pub passed_over: usize,
}

impl Loudest {
    pub fn peak_db(self) -> Option<f64> {
        (self.measured > 0 && self.peak > 0.0).then(|| 20.0 * self.peak.log10())
    }

    pub fn fitted_preamp(self) -> Option<Preamp> {
        self.peak_db().map(Preamp::holding_under_full_scale)
    }

    pub fn joined(self, other: Self) -> Self {
        Self {
            peak: self.peak.max(other.peak),
            measured: self.measured + other.measured,
            passed_over: self.passed_over + other.passed_over,
        }
    }
}

pub fn peak_through(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    profile: &Arc<Profile>,
    watch: &dyn Watching,
) -> Result<f64> {
    let (mut decoder, info) = opened(sources, location, span)?;
    let codec = Codec::from_id(info.codec);
    let format = native_format(&info, codec);
    decoder.set_output_format(format);
    decoder.follow_format_changes();

    let rate = info.spec.rate;
    let layout = info.spec.channels;
    let channels = usize::from(layout.count().get());

    let mut stage = Equaliser::new(without_the_preamp(profile, rate), rate);
    stage
        .prepare(
            StreamSpec::new(rate, layout, SampleFormat::F32),
            FRAMES_AT_A_TIME,
        )
        .map_err(|source| Error::Equalising {
            source: Box::new(source),
        })?;
    let mut meter = TruePeakMeter::at(
        rate,
        NonZeroUsize::new(channels).unwrap_or(NonZeroUsize::MIN),
    );

    let mut walking = Walking::new(StreamSpec::new(rate, layout, format));
    let mut widened = Vec::new();
    let mut shaped = Vec::new();
    let mut narrowed = Vec::new();
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let Some(heard) = walking.step(&mut decoder)? else {
            break;
        };

        widened.clear();
        widened.extend(heard.samples.iter().map(|sample| f64::from(*sample)));
        shaped.clear();
        shaped.resize(widened.len(), 0.0);
        stage.process(&widened, &mut shaped);

        narrowed.clear();
        narrowed.extend(shaped.iter().map(|sample| *sample as f32));
        meter.note(&narrowed);
    }

    Ok(meter.finish())
}

pub fn loudest_through(
    sources: &Sources,
    cuts: &[Cut],
    profile: &Arc<Profile>,
    watch: &dyn Watching,
) -> Result<Loudest> {
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, |available| (available.get() / 2).max(1))
        .min(cuts.len().max(1));

    let measured: Vec<Result<Loudest>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| measure_the_cuts(sources, cuts, profile, watch, &next)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or(Err(Error::Stopped)))
            .collect()
    });

    measured
        .into_iter()
        .try_fold(Loudest::default(), |all, each| Ok(all.joined(each?)))
}

fn measure_the_cuts(
    sources: &Sources,
    cuts: &[Cut],
    profile: &Arc<Profile>,
    watch: &dyn Watching,
    next: &AtomicUsize,
) -> Result<Loudest> {
    let mut loudest = Loudest::default();
    while let Some(cut) = cuts.get(next.fetch_add(1, Ordering::Relaxed)) {
        match peak_through(sources, &cut.location, cut.span, profile, watch) {
            Ok(peak) => {
                loudest.peak = loudest.peak.max(peak);
                loudest.measured += 1;
            }
            Err(Error::Stopped) => return Err(Error::Stopped),
            Err(error) => {
                tracing::debug!(%error, location = %cut.location, "a cut is left out of the preamp's fit");
                loudest.passed_over += 1;
            }
        }
    }
    Ok(loudest)
}

fn without_the_preamp(profile: &Arc<Profile>, rate: SampleRate) -> Arc<Profile> {
    let mut bare = Arc::unwrap_or_clone(profile.at_rate(rate));
    bare.set_preamp(Preamp::NONE);
    Arc::new(bare)
}
