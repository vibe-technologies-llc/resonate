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
use resonate_dsp::{Convolver, Equaliser, Processor, TruePeakMeter};

use crate::{
    Applied, BandGains, BandPeaks, Error, Result, Watching,
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

    pub fn weigh(&mut self, gains: &BandGains, peaks: &BandPeaks) {
        self.peak = self.peak.max(gains.bound_beside(peaks, self.peak));
        self.measured += 1;
    }

    pub fn joined(self, other: Self) -> Self {
        Self {
            peak: self.peak.max(other.peak),
            measured: self.measured + other.measured,
            passed_over: self.passed_over + other.passed_over,
        }
    }
}

struct Shaping {
    convolver: Option<Convolver>,
    stage: Equaliser,
    widened: Vec<f64>,
    corrected: Vec<f64>,
    shaped: Vec<f64>,
    narrowed: Vec<f32>,
}

impl Shaping {
    fn prepared(applied: &Applied, spec: StreamSpec) -> Result<Self> {
        let rate = spec.rate;
        let mut stage = Equaliser::new(without_the_preamp(applied.profile(), rate), rate);
        stage
            .prepare(spec, FRAMES_AT_A_TIME)
            .map_err(|source| Error::Equalising {
                source: Box::new(source),
            })?;
        let convolver = applied
            .correction()
            .map(|impulse| {
                let mut convolver = Convolver::new(Arc::clone(impulse));
                convolver
                    .prepare(spec, FRAMES_AT_A_TIME)
                    .map(|_| convolver)
                    .map_err(|source| Error::Correcting {
                        source: Box::new(source),
                    })
            })
            .transpose()?;
        Ok(Self {
            convolver,
            stage,
            widened: Vec::new(),
            corrected: Vec::new(),
            shaped: Vec::new(),
            narrowed: Vec::new(),
        })
    }

    fn shape(&mut self, heard: &[f32], meter: &mut TruePeakMeter) {
        self.widened.clear();
        self.widened
            .extend(heard.iter().map(|sample| f64::from(*sample)));
        let Some(convolver) = self.convolver.as_mut() else {
            let widened = std::mem::take(&mut self.widened);
            self.equalise(&widened, meter);
            self.widened = widened;
            return;
        };
        self.corrected.clear();
        self.corrected.resize(self.widened.len(), 0.0);
        convolver.process(&self.widened, &mut self.corrected);
        let corrected = std::mem::take(&mut self.corrected);
        self.equalise(&corrected, meter);
        self.corrected = corrected;
    }

    fn drain(&mut self, channels: usize, meter: &mut TruePeakMeter) {
        loop {
            let Some(convolver) = self.convolver.as_mut() else {
                return;
            };
            self.corrected.clear();
            self.corrected.resize(FRAMES_AT_A_TIME * channels, 0.0);
            let flushed = convolver.flush(&mut self.corrected);
            if flushed == 0 {
                return;
            }
            self.corrected.truncate(flushed * channels);
            let corrected = std::mem::take(&mut self.corrected);
            self.equalise(&corrected, meter);
            self.corrected = corrected;
        }
    }

    fn equalise(&mut self, input: &[f64], meter: &mut TruePeakMeter) {
        self.shaped.clear();
        self.shaped.resize(input.len(), 0.0);
        self.stage.process(input, &mut self.shaped);

        self.narrowed.clear();
        self.narrowed
            .extend(self.shaped.iter().map(|sample| *sample as f32));
        meter.note(&self.narrowed);
    }
}

pub fn peak_through(
    sources: &Sources,
    location: &MediaLocation,
    span: Option<FrameSpan>,
    applied: &Applied,
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

    let mut shaping = Shaping::prepared(applied, StreamSpec::new(rate, layout, SampleFormat::F32))?;
    let mut meter = TruePeakMeter::at(
        rate,
        NonZeroUsize::new(channels).unwrap_or(NonZeroUsize::MIN),
    );

    let mut walking = Walking::new(StreamSpec::new(rate, layout, format));
    loop {
        if watch.stopped() {
            return Err(Error::Stopped);
        }
        let Some(heard) = walking.step(&mut decoder)? else {
            break;
        };
        shaping.shape(heard.samples, &mut meter);
    }
    shaping.drain(channels, &mut meter);

    Ok(meter.finish())
}

pub fn loudest_through(
    sources: &Sources,
    cuts: &[Cut],
    applied: &Applied,
    watch: &dyn Watching,
) -> Result<Loudest> {
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, |available| (available.get() / 2).max(1))
        .min(cuts.len().max(1));

    let measured: Vec<Result<Loudest>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| scope.spawn(|| measure_the_cuts(sources, cuts, applied, watch, &next)))
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
    applied: &Applied,
    watch: &dyn Watching,
    next: &AtomicUsize,
) -> Result<Loudest> {
    let mut loudest = Loudest::default();
    while let Some(cut) = cuts.get(next.fetch_add(1, Ordering::Relaxed)) {
        match peak_through(sources, &cut.location, cut.span, applied, watch) {
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
