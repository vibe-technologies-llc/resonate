use resonate_core::{AudioBuffer, SampleData, SampleFormat, StreamSpec};
use symphonia::core::{checksum::Md5, io::Monitor};

use crate::key::VaultKey;

pub(crate) const BLOCK_FRAMES: usize = 4096;

const TURN_MARK: &[u8] = b"resonate:turn";
const DROPPED_MARK: &[u8] = b"resonate:dropped";

#[derive(Default)]
pub(crate) struct Digest(Md5);

impl Digest {
    pub(crate) fn note(&mut self, bytes: &[u8]) {
        self.0.process_buf_bytes(bytes);
    }

    pub(crate) fn turned(&mut self, to: StreamSpec) {
        self.note(TURN_MARK);
        self.note(&to.rate.hz().to_le_bytes());
        self.note(&[to.channels.count().get()]);
    }

    pub(crate) fn dropped_live_bits(&mut self) {
        self.note(DROPPED_MARK);
    }

    pub(crate) fn settled(&self) -> VaultKey {
        VaultKey::of(self.0.md5())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Narrowing {
    from: SampleFormat,
    to: SampleFormat,
}

impl Narrowing {
    pub(crate) const fn between(from: SampleFormat, to: SampleFormat) -> Self {
        Self { from, to }
    }

    const fn dropped_bits(self) -> u8 {
        self.from.valid_bits().saturating_sub(self.to.valid_bits())
    }

    pub(crate) const fn drops(self) -> bool {
        self.dropped_bits() > 0
    }

    pub(crate) const fn decoded_as(self) -> SampleFormat {
        match self.drops() {
            true => self.from,
            false => self.to,
        }
    }

    pub(crate) fn narrowed(self, samples: &mut [i32]) -> bool {
        let dropped = u32::from(self.dropped_bits());
        if dropped == 0 {
            return true;
        }
        let below = (1_i32 << dropped) - 1;
        let mut lost = 0;
        for sample in samples {
            lost |= *sample & below;
            *sample >>= dropped;
        }
        lost == 0
    }
}

pub(crate) fn packed(samples: &[i32], format: SampleFormat, into: &mut Vec<u8>) {
    into.clear();
    let width = match format {
        SampleFormat::S16 => 2,
        SampleFormat::S24 => 3,
        SampleFormat::S32 | SampleFormat::F32 => 4,
    };
    for sample in samples {
        into.extend_from_slice(&sample.to_le_bytes()[..width]);
    }
}

pub(crate) fn little_endian(block: &AudioBuffer, into: &mut Vec<u8>) {
    into.clear();
    match block.data() {
        SampleData::S16(samples) => {
            for sample in samples {
                into.extend_from_slice(&sample.to_le_bytes());
            }
        }
        SampleData::S24(samples) => {
            for sample in samples {
                into.extend_from_slice(&sample.to_le_bytes()[..3]);
            }
        }
        SampleData::S32(samples) => {
            for sample in samples {
                into.extend_from_slice(&sample.to_le_bytes());
            }
        }
        SampleData::F32(samples) => {
            for sample in samples {
                into.extend_from_slice(&sample.to_le_bytes());
            }
        }
    }
}

pub(crate) fn in_full(samples: &[f64], into: &mut Vec<u8>) {
    into.clear();
    for sample in samples {
        into.extend_from_slice(&sample.to_le_bytes());
    }
}

pub(crate) fn widened(block: &AudioBuffer, into: &mut Vec<i32>) {
    into.clear();
    match block.data() {
        SampleData::S16(samples) => into.extend(samples.iter().map(|sample| i32::from(*sample))),
        SampleData::S24(samples) | SampleData::S32(samples) => into.extend_from_slice(samples),
        SampleData::F32(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelCount, ChannelLayout, SampleFormat, SampleRate, StreamSpec};

    use super::*;

    fn stereo(format: SampleFormat) -> StreamSpec {
        StreamSpec::new(
            SampleRate::HZ_44100,
            ChannelLayout::from_count(ChannelCount::STEREO),
            format,
        )
    }

    #[test]
    fn a_twenty_four_bit_sample_is_written_as_the_three_bytes_flac_hashes() {
        let mut block = AudioBuffer::empty(stereo(SampleFormat::S24));
        block.set_frames(1);
        if let SampleData::S24(samples) = block.data_mut() {
            samples[0] = -1;
            samples[1] = 0x0034_5678;
        }

        let mut bytes = Vec::new();
        little_endian(&block, &mut bytes);

        assert_eq!(bytes, vec![0xff, 0xff, 0xff, 0x78, 0x56, 0x34]);
    }

    #[test]
    fn the_same_run_of_bytes_settles_on_the_same_key_twice_running() {
        let mut one = Digest::default();
        let mut other = Digest::default();
        one.note(b"the same bytes");
        other.note(b"the same ");
        other.note(b"bytes");

        assert_eq!(one.settled(), other.settled());
    }
}
