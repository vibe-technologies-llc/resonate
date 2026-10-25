use resonate_codec::{DecodeStatus, Decoder};
use resonate_core::{AudioBuffer, SampleData, StreamSpec};
use resonate_dsp::{FilterPhase, Processor, Quality, Remix, Resampler, ResamplerConfig};

use crate::{AnalysisOp, Error, Result, analysis::normalise};

const FRAMES_AT_A_TIME: usize = 4_096;

struct Segment {
    remix: Option<Remix>,
    resampler: Option<Resampler>,
    sounding_channels: usize,
    opened_channels: usize,
}

pub(crate) struct Conforming {
    opened: StreamSpec,
    segment: Option<Segment>,
    turned: bool,
    widened: Vec<f64>,
    remixed: Vec<f64>,
    resampled: Vec<f64>,
    conformed: Vec<f32>,
}

impl Conforming {
    pub(crate) fn new(opened: StreamSpec) -> Self {
        Self {
            opened,
            segment: None,
            turned: false,
            widened: Vec::new(),
            remixed: Vec::new(),
            resampled: Vec::new(),
            conformed: Vec::new(),
        }
    }

    pub(crate) fn turn(&mut self, sounding: StreamSpec) -> Result<&[f32]> {
        self.turned = true;
        self.conformed.clear();
        self.drain();
        self.segment = self.segment_for(sounding)?;
        Ok(&self.conformed)
    }

    pub(crate) fn through<'a>(&'a mut self, interleaved: &'a [f32]) -> &'a [f32] {
        if self.segment.is_none() {
            return interleaved;
        }
        self.conformed.clear();
        self.convert(interleaved);
        &self.conformed
    }

    pub(crate) fn finish(&mut self) -> &[f32] {
        self.conformed.clear();
        self.drain();
        self.segment = None;
        &self.conformed
    }

    fn segment_for(&self, sounding: StreamSpec) -> Result<Option<Segment>> {
        let same_rate = sounding.rate == self.opened.rate;
        let same_layout = sounding.channels == self.opened.channels;
        if same_rate && same_layout {
            return Ok(None);
        }
        let refused = |source: resonate_dsp::Error| Error::Conformity {
            from: self.opened,
            to: sounding,
            source: Box::new(source),
        };
        let remix = (!same_layout).then(|| Remix::new(sounding.channels, self.opened.channels));
        let resampler = if same_rate {
            None
        } else {
            let resampler = Resampler::new(ResamplerConfig {
                input_rate: sounding.rate,
                output_rate: self.opened.rate,
                channels: self.opened.channels,
                quality: Quality::Balanced,
                phase: FilterPhase::Linear,
                max_frames_in: FRAMES_AT_A_TIME,
            })
            .map_err(refused)?;
            Some(resampler)
        };
        Ok(Some(Segment {
            remix,
            resampler,
            sounding_channels: usize::from(sounding.channels.count().get()),
            opened_channels: usize::from(self.opened.channels.count().get()),
        }))
    }

    fn convert(&mut self, interleaved: &[f32]) {
        let Some(segment) = &mut self.segment else {
            return;
        };
        let at_a_time = FRAMES_AT_A_TIME * segment.sounding_channels;
        for chunk in interleaved.chunks(at_a_time) {
            self.widened.clear();
            self.widened
                .extend(chunk.iter().map(|sample| f64::from(*sample)));

            let remixed: &[f64] = match &mut segment.remix {
                Some(remix) => {
                    let frames = chunk.len() / segment.sounding_channels;
                    self.remixed.clear();
                    self.remixed.resize(frames * segment.opened_channels, 0.0);
                    remix.process(&self.widened, &mut self.remixed);
                    &self.remixed
                }
                None => &self.widened,
            };
            match &mut segment.resampler {
                Some(resampler) => {
                    let room = resampler.max_output_frames(FRAMES_AT_A_TIME);
                    self.resampled.clear();
                    self.resampled.resize(room * segment.opened_channels, 0.0);
                    let made = resampler.process(remixed, &mut self.resampled).frames_out;
                    let written = self
                        .resampled
                        .get(..made * segment.opened_channels)
                        .unwrap_or_default();
                    self.conformed
                        .extend(written.iter().map(|sample| *sample as f32));
                }
                None => self
                    .conformed
                    .extend(remixed.iter().map(|sample| *sample as f32)),
            }
        }
    }

    fn drain(&mut self) {
        let Some(segment) = &mut self.segment else {
            return;
        };
        let Some(resampler) = &mut segment.resampler else {
            return;
        };
        self.resampled
            .resize(resampler.max_flush_frames() * segment.opened_channels, 0.0);
        let made = resampler.flush(&mut self.resampled);
        let written = self
            .resampled
            .get(..made * segment.opened_channels)
            .unwrap_or_default();
        self.conformed
            .extend(written.iter().map(|sample| *sample as f32));
    }
}

pub(crate) struct Heard<'a> {
    pub(crate) native: Option<&'a SampleData>,
    pub(crate) samples: &'a [f32],
}

pub(crate) struct Walking {
    block: AudioBuffer,
    normalised: Vec<f32>,
    conforming: Conforming,
    ended: bool,
}

impl Walking {
    pub(crate) fn new(opened: StreamSpec) -> Self {
        Self {
            block: AudioBuffer::empty(opened),
            normalised: Vec::new(),
            conforming: Conforming::new(opened),
            ended: false,
        }
    }

    pub(crate) fn step(&mut self, decoder: &mut Decoder) -> Result<Option<Heard<'_>>> {
        if self.ended {
            return Ok(None);
        }
        let status = decoder
            .next_block(&mut self.block)
            .map_err(|source| Error::codec(AnalysisOp::Decode, source))?;
        match status {
            DecodeStatus::EndOfStream => {
                self.ended = true;
                let tail = self.conforming.finish();
                Ok((!tail.is_empty()).then_some(Heard {
                    native: None,
                    samples: tail,
                }))
            }
            DecodeStatus::Reformatted(sounding) => Ok(Some(Heard {
                native: None,
                samples: self.conforming.turn(sounding)?,
            })),
            DecodeStatus::Decoded => {
                normalise(self.block.data(), &mut self.normalised);
                let native = (!self.conforming.turned).then(|| self.block.data());
                Ok(Some(Heard {
                    native,
                    samples: self.conforming.through(&self.normalised),
                }))
            }
        }
    }
}
