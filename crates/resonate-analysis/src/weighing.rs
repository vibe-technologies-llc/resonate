use resonate_core::{ChannelLayout, SampleRate};
use resonate_dsp::{FilterPhase, Processor, Quality, Resampler, ResamplerConfig};

use crate::{
    granules::Mp3,
    steps::{Aac, Stepping, Steps, Vorbis},
};

const BROUGHT_BACK_TO: [(u32, SampleRate); 2] = [
    (44_100, SampleRate::HZ_48000),
    (48_000, SampleRate::HZ_44100),
];
const FRAMES_AT_A_TIME: usize = 4_096;
const LANES_WEIGHED_AT_MOST: usize = 2;
const SEARCHES_RESAMPLED: usize = 2;

struct Coded {
    aac: Stepping<Aac>,
    mp3: Stepping<Mp3>,
    vorbis: Stepping<Vorbis>,
}

impl Coded {
    fn new(rate: u32, channels: usize) -> Self {
        Self {
            aac: Stepping::new(rate, channels),
            mp3: Stepping::new(rate, channels),
            vorbis: Stepping::new(rate, channels),
        }
    }

    fn unweighed() -> Self {
        Self {
            aac: Stepping::unweighed(),
            mp3: Stepping::unweighed(),
            vorbis: Stepping::unweighed(),
        }
    }

    fn is_done(&self) -> bool {
        self.aac.is_done() && self.mp3.is_done() && self.vorbis.is_done()
    }

    fn note(&mut self, interleaved: &[f32]) {
        self.aac.note(interleaved);
        self.mp3.note(interleaved);
        self.vorbis.note(interleaved);
    }

    fn finished(self, into: &mut Vec<Steps>) {
        into.extend(self.aac.finished());
        into.extend(self.mp3.finished());
        into.extend(self.vorbis.finished());
    }
}

struct Resampled {
    resampler: Resampler,
    channels: usize,
    lanes: usize,
    widened: Vec<f64>,
    out: Vec<f64>,
    narrowed: Vec<f32>,
    coded: Coded,
}

impl Resampled {
    fn new(rate: u32, channels: usize) -> Option<Self> {
        let (_, to) = BROUGHT_BACK_TO
            .into_iter()
            .find(|(from, _)| *from == rate)?;
        let lanes = channels.min(LANES_WEIGHED_AT_MOST);
        let layout = if lanes == 1 {
            ChannelLayout::Mono
        } else {
            ChannelLayout::Stereo
        };
        let resampler = Resampler::new(ResamplerConfig {
            input_rate: SampleRate::new(rate).ok()?,
            output_rate: to,
            channels: layout,
            quality: Quality::Balanced,
            phase: FilterPhase::Linear,
            max_frames_in: FRAMES_AT_A_TIME,
        })
        .ok()?;
        Some(Self {
            resampler,
            channels,
            lanes,
            widened: Vec::new(),
            out: Vec::new(),
            narrowed: Vec::new(),
            coded: Coded {
                aac: Stepping::searching(to.hz(), lanes, SEARCHES_RESAMPLED),
                mp3: Stepping::searching(to.hz(), lanes, SEARCHES_RESAMPLED),
                vorbis: Stepping::unweighed(),
            },
        })
    }

    fn note(&mut self, interleaved: &[f32]) {
        if self.coded.is_done() {
            return;
        }
        for chunk in interleaved.chunks(FRAMES_AT_A_TIME * self.channels) {
            self.widened.clear();
            self.widened.extend(
                chunk
                    .chunks_exact(self.channels)
                    .flat_map(|frame| &frame[..self.lanes])
                    .map(|sample| f64::from(*sample)),
            );
            let room = self.resampler.max_output_frames(FRAMES_AT_A_TIME);
            self.out.clear();
            self.out.resize(room * self.lanes, 0.0);
            let made = self
                .resampler
                .process(&self.widened, &mut self.out)
                .frames_out;
            self.narrowed.clear();
            self.narrowed.extend(
                self.out[..made * self.lanes]
                    .iter()
                    .map(|sample| *sample as f32),
            );
            self.coded.note(&self.narrowed);
        }
    }
}

pub(crate) struct Weighing {
    native: Coded,
    resampled: Option<Resampled>,
}

impl Weighing {
    pub(crate) fn new(rate: u32, channels: usize) -> Self {
        Self {
            native: Coded::new(rate, channels),
            resampled: Resampled::new(rate, channels),
        }
    }

    pub(crate) fn unweighed() -> Self {
        Self {
            native: Coded::unweighed(),
            resampled: None,
        }
    }

    pub(crate) fn note(&mut self, interleaved: &[f32]) {
        self.native.note(interleaved);
        if let Some(resampled) = &mut self.resampled {
            resampled.note(interleaved);
        }
    }

    pub(crate) fn finished(self) -> Vec<Steps> {
        let mut steps = Vec::new();
        self.native.finished(&mut steps);
        if let Some(resampled) = self.resampled {
            resampled.coded.finished(&mut steps);
        }
        steps
    }
}
