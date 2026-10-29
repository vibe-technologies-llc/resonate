use resonate_core::{SampleFormat, StreamSpec};

use crate::{Carriage, Result, Wrapping, flac::Live, wave};

pub(crate) enum Wire {
    Pcm {
        format: SampleFormat,
        wrapping: Wrapping,
    },
    Flac(Box<Live>),
}

impl Wire {
    pub(crate) fn begin(carriage: &Carriage, spec: StreamSpec) -> Result<(Self, Vec<u8>)> {
        match carriage.wrapping() {
            Wrapping::Flac => {
                let (live, head) = Live::begin(spec)?;
                Ok((Self::Flac(Box::new(live)), head))
            }
            wrapping => {
                let head = if wrapping == Wrapping::Wave {
                    wave::header(spec)
                } else {
                    Vec::new()
                };
                Ok((
                    Self::Pcm {
                        format: spec.format,
                        wrapping,
                    },
                    head,
                ))
            }
        }
    }

    pub(crate) fn carry(&mut self, ring: &[u8], frames: u64, wire: &mut Vec<u8>) -> Result<u64> {
        match self {
            Self::Pcm { format, wrapping } => {
                wave::carried(ring, *format, *wrapping, wire);
                Ok(frames)
            }
            Self::Flac(live) => live.carry(ring, wire),
        }
    }

    pub(crate) fn finish(&mut self, wire: &mut Vec<u8>) -> Result<u64> {
        match self {
            Self::Pcm { .. } => {
                wire.clear();
                Ok(0)
            }
            Self::Flac(live) => live.finish(wire),
        }
    }
}
