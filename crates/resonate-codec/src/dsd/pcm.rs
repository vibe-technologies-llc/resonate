use crate::dsd::{
    BitOrder, DOP_DECIMATION, DSD_SILENCE,
    dop::BIT_REVERSE,
    window::{beta_for, kaiser_sinc},
};

const PASSBAND_HZ: f64 = 45_000.0;
const STOPBAND_DB: f64 = 96.0;
const FIR_BYTES: usize = 64;
const FIR_STEP_MASK: usize = FIR_BYTES - 1;
const FIR_TAPS: usize = FIR_BYTES * 8;
const BYTE_VALUES: usize = 256;
const DC_BLOCK_HZ: f64 = 2.0;
const ONE_BIT_HIGH: f64 = 1.0;
const ONE_BIT_LOW: f64 = -1.0;

type PartialSums = [[f32; BYTE_VALUES]; FIR_BYTES];

#[derive(Clone, Copy)]
struct History {
    held: [u8; FIR_BYTES],
    oldest: usize,
}

impl History {
    const fn silent() -> Self {
        Self {
            held: [DSD_SILENCE; FIR_BYTES],
            oldest: 0,
        }
    }

    fn took(&mut self, byte: u8) {
        self.held[self.oldest] = byte;
        self.oldest = (self.oldest + 1) & FIR_STEP_MASK;
    }
}

pub(crate) struct Decimator {
    table: Box<PartialSums>,
    history: Vec<History>,
    blocked: Vec<DcBlock>,
    bits: BitOrder,
}

impl Decimator {
    pub(crate) fn new(dsd_hz: u32, carrier_hz: u32, lanes: usize, bits: BitOrder) -> Self {
        Self {
            table: partial_sums(dsd_hz),
            history: vec![History::silent(); lanes],
            blocked: vec![DcBlock::at(carrier_hz); lanes],
            bits,
        }
    }

    pub(crate) fn prime(&mut self) {
        for plane in &mut self.history {
            *plane = History::silent();
        }
        for blocked in &mut self.blocked {
            blocked.forget();
        }
    }

    pub(crate) fn frames(&mut self, planes: &[Vec<u8>], into: &mut Vec<f32>, lanes: usize) {
        let taking = planes.iter().map(Vec::len).min().unwrap_or(0);
        let per_frame = DOP_DECIMATION as usize / 8;

        for frame in 0..taking / per_frame.max(1) {
            for lane in 0..lanes {
                let (Some(plane), Some(history), Some(blocked)) = (
                    planes.get(lane),
                    self.history.get_mut(lane),
                    self.blocked.get_mut(lane),
                ) else {
                    continue;
                };
                let at = frame * per_frame;
                let Some(fresh) = plane.get(at..at + per_frame) else {
                    continue;
                };
                for byte in fresh {
                    history.took(ordered(*byte, self.bits));
                }
                into.push(blocked.next(convolved(&self.table, history)));
            }
        }
    }
}

fn ordered(byte: u8, bits: BitOrder) -> u8 {
    match bits {
        BitOrder::LeastSignificantFirst => BIT_REVERSE[usize::from(byte)],
        BitOrder::MostSignificantFirst => byte,
    }
}

fn convolved(table: &PartialSums, history: &History) -> f32 {
    let mut sum = 0.0;
    for (step, sums) in table.iter().enumerate() {
        let byte = history.held[(history.oldest + step) & FIR_STEP_MASK];
        sum += sums[usize::from(byte)];
    }
    sum
}

fn partial_sums(dsd_hz: u32) -> Box<PartialSums> {
    let cutoff = PASSBAND_HZ / f64::from(dsd_hz);
    let kernel = kaiser_sinc(FIR_TAPS, cutoff, beta_for(STOPBAND_DB));

    let mut table = vec![[0.0_f32; BYTE_VALUES]; FIR_BYTES];
    for (step, sums) in table.iter_mut().enumerate() {
        for (value, slot) in sums.iter_mut().enumerate() {
            let mut sum = 0.0;
            for bit in 0..8 {
                let set = (value >> (7 - bit)) & 1 == 1;
                let weight = kernel.get(step * 8 + bit).copied().unwrap_or(0.0);
                sum += weight * if set { ONE_BIT_HIGH } else { ONE_BIT_LOW };
            }
            *slot = sum as f32;
        }
    }
    table
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| Box::new([[0.0_f32; BYTE_VALUES]; FIR_BYTES]))
}

#[derive(Clone, Copy, Debug)]
struct DcBlock {
    pole: f32,
    last_in: f32,
    last_out: f32,
}

impl DcBlock {
    fn at(carrier_hz: u32) -> Self {
        let pole = 1.0 - (std::f64::consts::TAU * DC_BLOCK_HZ / f64::from(carrier_hz));
        Self {
            pole: pole.clamp(0.0, 1.0) as f32,
            last_in: 0.0,
            last_out: 0.0,
        }
    }

    fn forget(&mut self) {
        self.last_in = 0.0;
        self.last_out = 0.0;
    }

    fn next(&mut self, sample: f32) -> f32 {
        let out = sample - self.last_in + self.pole * self.last_out;
        self.last_in = sample;
        self.last_out = out;
        out
    }
}

#[cfg(test)]
mod tests {
    use std::slice;

    use super::*;

    const DSD64: u32 = 2_822_400;
    const CARRIER: u32 = 176_400;

    fn decimated(plane: u8, frames: usize) -> Vec<f32> {
        let mut held = Decimator::new(DSD64, CARRIER, 1, BitOrder::MostSignificantFirst);
        let bytes = vec![plane; frames * DOP_DECIMATION as usize / 8];
        let mut out = Vec::new();
        held.frames(&[bytes], &mut out, 1);
        out
    }

    #[test]
    fn every_bit_set_is_positive_full_scale_and_none_is_negative() {
        let high = decimated(0xFF, 400);
        let low = decimated(0x00, 400);

        assert!(
            high.last().copied().unwrap_or(0.0) > 0.97,
            "an all-ones plane did not reach full scale: {:?}",
            high.last()
        );
        assert!(
            low.last().copied().unwrap_or(0.0) < -0.97,
            "an all-zeros plane is not silence but negative full scale: {:?}",
            low.last()
        );
    }

    #[test]
    fn the_silence_pattern_really_is_near_silence() {
        let quiet = decimated(DSD_SILENCE, 400);
        let loudest = quiet
            .iter()
            .skip(200)
            .fold(0.0_f32, |held, sample| held.max(sample.abs()));

        assert!(
            loudest < 0.02,
            "the silence pattern is not quiet: {loudest}"
        );
    }

    fn shifting_history(plane: &[u8]) -> Vec<f32> {
        let table = partial_sums(DSD64);
        let mut held = vec![DSD_SILENCE; FIR_BYTES];
        let mut blocked = DcBlock::at(CARRIER);
        let per_frame = DOP_DECIMATION as usize / 8;

        let mut out = Vec::new();
        for frame in 0..plane.len() / per_frame {
            for byte in &plane[frame * per_frame..(frame + 1) * per_frame] {
                held.remove(0);
                held.push(*byte);
            }
            let mut sum = 0.0;
            for (step, byte) in held.iter().enumerate() {
                sum += table[step][usize::from(*byte)];
            }
            out.push(blocked.next(sum));
        }
        out
    }

    #[test]
    fn the_ring_answers_what_a_shifting_history_answered() {
        let plane: Vec<u8> = (0..8_192_u32)
            .map(|step| (step.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();

        let mut held = Decimator::new(DSD64, CARRIER, 1, BitOrder::MostSignificantFirst);
        let mut ringed = Vec::new();
        held.frames(slice::from_ref(&plane), &mut ringed, 1);

        assert_eq!(ringed, shifting_history(&plane));
        assert!(!ringed.is_empty());
    }

    #[test]
    fn a_constant_plane_has_its_offset_taken_out_over_time() {
        let held = decimated(0xFF, 4_000);
        let settled = held.last().copied().unwrap_or(0.0);
        let early = held.get(400).copied().unwrap_or(0.0);

        assert!(
            settled < early,
            "the dc blocker let a constant through unchanged"
        );
    }
}
