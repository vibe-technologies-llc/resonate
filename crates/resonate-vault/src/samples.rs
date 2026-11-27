use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{AudioBuffer, Frames, MediaLocation, SampleFormat, StreamSpec};

use crate::{
    Halt,
    error::{Error, Result, VaultOp},
    key::VaultKey,
    pcm::{self, Digest},
};

const DECODED_BITS: u8 = 32;
const BITS_A_BYTE: u8 = 8;

#[derive(Default)]
pub struct SampleDigest {
    digest: Digest,
    bytes: Vec<u8>,
}

impl SampleDigest {
    pub fn note(&mut self, samples: &[i32], bits: u8) {
        let width = usize::from(bits.div_ceil(BITS_A_BYTE));

        self.bytes.clear();
        for sample in samples {
            self.bytes
                .extend_from_slice(&sample.to_le_bytes()[..width.min(4)]);
        }
        self.digest.note(&self.bytes);
    }

    pub fn settled(&self) -> VaultKey {
        self.digest.settled()
    }
}

pub fn narrowed_to_depth(samples: &mut [i32], bits: u8) -> bool {
    let dropped = u32::from(DECODED_BITS.saturating_sub(bits));

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeardSamples {
    pub digest: VaultKey,
    pub frames: Frames,
    pub wider_than_its_depth: bool,
}

pub fn heard_at_depth(
    sources: &Sources,
    location: &MediaLocation,
    bits: u8,
    halt: Halt<'_>,
) -> Result<HeardSamples> {
    let (mut decoder, info) =
        Decoder::open(sources, location).map_err(|source| Error::codec(VaultOp::Verify, source))?;

    decoder.refuse_holes();
    decoder.set_output_format(SampleFormat::S32);

    let mut block = AudioBuffer::empty(StreamSpec::new(
        info.spec.rate,
        info.spec.channels,
        SampleFormat::S32,
    ));
    let mut widened = Vec::new();
    let mut digest = SampleDigest::default();
    let mut frames = 0_u64;
    let mut wider_than_its_depth = false;

    loop {
        halt.heard()?;

        let status = decoder
            .next_block(&mut block)
            .map_err(|source| Error::codec(VaultOp::Verify, source))?;

        match status {
            DecodeStatus::EndOfStream => break,
            DecodeStatus::Reformatted(_) => {}
            DecodeStatus::Decoded => {
                pcm::widened(&block, &mut widened);
                wider_than_its_depth |= !narrowed_to_depth(&mut widened, bits);
                digest.note(&widened, bits);
                frames += block.frames() as u64;
            }
        }
    }

    Ok(HeardSamples {
        digest: digest.settled(),
        frames: Frames(frames),
        wider_than_its_depth,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_decoded_wide_come_back_to_their_depth_and_a_live_low_bit_is_told() {
        let mut held = vec![3 << 12, -5 << 12];
        let mut wider = vec![(3 << 12) | 1];

        assert!(narrowed_to_depth(&mut held, 20));
        assert_eq!(held, vec![3, -5]);
        assert!(!narrowed_to_depth(&mut wider, 20));
    }

    #[test]
    fn a_digest_packs_each_sample_in_the_bytes_its_depth_fills() {
        let mut twenty = SampleDigest::default();
        let mut by_hand = Digest::default();

        twenty.note(&[0x0001_0203, -1], 20);
        by_hand.note(&[0x03, 0x02, 0x01, 0xff, 0xff, 0xff]);

        assert_eq!(twenty.settled(), by_hand.settled());
    }
}
