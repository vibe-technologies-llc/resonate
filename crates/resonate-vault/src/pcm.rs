use resonate_core::{AudioBuffer, SampleData};
use symphonia::core::{checksum::Md5, io::Monitor};

use crate::key::VaultKey;

pub(crate) const BLOCK_FRAMES: usize = 4096;

#[derive(Default)]
pub(crate) struct Digest(Md5);

impl Digest {
    pub(crate) fn note(&mut self, bytes: &[u8]) {
        self.0.process_buf_bytes(bytes);
    }

    pub(crate) fn settled(&self) -> VaultKey {
        VaultKey::of(self.0.md5())
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
