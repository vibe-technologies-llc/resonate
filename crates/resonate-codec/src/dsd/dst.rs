use std::io::{self, Read, Seek, SeekFrom};

use crate::{dsd::DSD_SILENCE, source::MediaStream};

pub(crate) const MOST_CHANNELS: usize = 6;
pub(crate) const FRAMES_A_SECOND: u64 = 75;

const MOST_ELEMENTS: usize = 2 * MOST_CHANNELS;
const LONGEST_FILTER: usize = 128;
const HISTORY_BYTES: usize = 16;
const FILTER_LENGTH_BITS: u32 = 7;
const FILTER_COEFFICIENT_BITS: u32 = 9;
const PROBABILITY_LENGTH_BITS: u32 = 6;
const PROBABILITY_BITS: u32 = 7;
const PROBABILITY_OFFSET: i32 = 1;
const EVEN_ODDS: u32 = 128;
const PREDICTION_METHOD_BITS: u32 = 2;
const UNUSED_PREDICTION_METHOD: u32 = 3;
const RICE_PARAMETER_BITS: u32 = 3;
const PLAIN_FRAME_PADDING_BITS: u32 = 6;
const CODER_PRECISION_BITS: u32 = 12;
const CODER_RANGE: u32 = (1 << CODER_PRECISION_BITS) - 1;
const CODER_HALF: u32 = 1 << (CODER_PRECISION_BITS - 1);
const FIRST_HISTORY: u128 = u128::from_le_bytes([0xAA; HISTORY_BYTES]);

const FILTER_PREDICTORS: [&[i32]; 3] = [&[-8], &[-16, 8], &[-9, -5, 6]];
const PROBABILITY_PREDICTORS: [&[i32]; 3] = [&[-8], &[-16, 8], &[-24, 24, -8]];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unpacking {
    Empty,
    Segmented,
    TooManyElements,
    MapOutOfRange,
    UnusedPredictionMethod,
    ProbabilityOutOfRange,
    FilterOverflows,
    PaddingNotZero,
    CoderNotStarted,
}

struct Bits<'a> {
    bytes: &'a [u8],
    read: usize,
}

impl<'a> Bits<'a> {
    const fn over(bytes: &'a [u8]) -> Self {
        Self { bytes, read: 0 }
    }

    fn left(&self) -> usize {
        (self.bytes.len() * 8).saturating_sub(self.read)
    }

    fn bit(&mut self) -> u32 {
        let held = self
            .bytes
            .get(self.read / 8)
            .map_or(0, |byte| u32::from(byte >> (7 - self.read % 8)) & 1);
        self.read += 1;
        held
    }

    fn bits(&mut self, count: u32) -> u32 {
        (0..count).fold(0, |held, _| (held << 1) | self.bit())
    }

    fn signed(&mut self, count: u32) -> i32 {
        let held = self.bits(count);
        let shift = 32 - count;
        ((held << shift) as i32) >> shift
    }

    fn rice(&mut self, parameter: u32) -> i32 {
        let mut zeros = 0_u32;
        while self.left() > 0 && self.bit() == 0 {
            zeros += 1;
        }
        let magnitude = zeros.wrapping_shl(parameter) | self.bits(parameter);
        let magnitude = magnitude as i32;
        if magnitude != 0 && self.bit() == 1 {
            magnitude.wrapping_neg()
        } else {
            magnitude
        }
    }
}

struct Table {
    elements: usize,
    length: [usize; MOST_ELEMENTS],
    coefficients: [[i32; LONGEST_FILTER]; MOST_ELEMENTS],
}

impl Table {
    const fn new() -> Self {
        Self {
            elements: 1,
            length: [0; MOST_ELEMENTS],
            coefficients: [[0; LONGEST_FILTER]; MOST_ELEMENTS],
        }
    }
}

struct Coefficients {
    length_bits: u32,
    width: u32,
    signed: bool,
    offset: i32,
    predictors: [&'static [i32]; 3],
}

const FILTERS: Coefficients = Coefficients {
    length_bits: FILTER_LENGTH_BITS,
    width: FILTER_COEFFICIENT_BITS,
    signed: true,
    offset: 0,
    predictors: FILTER_PREDICTORS,
};

const PROBABILITIES: Coefficients = Coefficients {
    length_bits: PROBABILITY_LENGTH_BITS,
    width: PROBABILITY_BITS,
    signed: false,
    offset: PROBABILITY_OFFSET,
    predictors: PROBABILITY_PREDICTORS,
};

fn read_map(
    bits: &mut Bits<'_>,
    table: &mut Table,
    map: &mut [usize; MOST_CHANNELS],
    channels: usize,
) -> Result<(), Unpacking> {
    table.elements = 1;
    *map = [0; MOST_CHANNELS];
    if bits.bit() == 1 {
        return Ok(());
    }
    for slot in map.iter_mut().take(channels).skip(1) {
        let width = u32::BITS - (table.elements as u32).leading_zeros();
        let element = bits.bits(width) as usize;
        if element == table.elements {
            table.elements += 1;
            if table.elements >= MOST_ELEMENTS {
                return Err(Unpacking::TooManyElements);
            }
        } else if element > table.elements {
            return Err(Unpacking::MapOutOfRange);
        }
        *slot = element;
    }
    Ok(())
}

fn read_plain(bits: &mut Bits<'_>, into: &mut [i32], coefficients: &Coefficients) {
    for held in into {
        let value = if coefficients.signed {
            bits.signed(coefficients.width)
        } else {
            bits.bits(coefficients.width) as i32
        };
        *held = value + coefficients.offset;
    }
}

fn read_table(
    bits: &mut Bits<'_>,
    table: &mut Table,
    coefficients: &Coefficients,
) -> Result<(), Unpacking> {
    for element in 0..table.elements {
        let length = bits.bits(coefficients.length_bits) as usize + 1;
        table.length[element] = length;
        let held = &mut table.coefficients[element];
        if bits.bit() == 0 {
            read_plain(bits, &mut held[..length], coefficients);
            continue;
        }

        let method = bits.bits(PREDICTION_METHOD_BITS);
        if method == UNUSED_PREDICTION_METHOD {
            return Err(Unpacking::UnusedPredictionMethod);
        }
        let order = method as usize + 1;
        read_plain(bits, &mut held[..order], coefficients);
        let parameter = bits.bits(RICE_PARAMETER_BITS);
        let predictor = coefficients.predictors[method as usize];

        for at in order..length {
            let predicted = predictor
                .iter()
                .enumerate()
                .fold(0_i32, |sum, (back, weight)| {
                    sum.wrapping_add(weight.wrapping_mul(held[at - back - 1]))
                });
            let mut value = bits.rice(parameter);
            if predicted >= 0 {
                value = value.wrapping_sub(predicted.wrapping_add(4) / 8);
            } else {
                value = value.wrapping_add(predicted.wrapping_neg().wrapping_add(3) / 8);
            }
            if !coefficients.signed
                && (value < coefficients.offset
                    || value >= coefficients.offset + (1 << coefficients.width))
            {
                return Err(Unpacking::ProbabilityOutOfRange);
            }
            held[at] = value;
        }
    }
    Ok(())
}

struct Coder {
    range: u32,
    code: u32,
}

impl Coder {
    fn start(bits: &mut Bits<'_>) -> Self {
        Self {
            range: CODER_RANGE,
            code: bits.bits(CODER_PRECISION_BITS),
        }
    }

    fn decode(&mut self, bits: &mut Bits<'_>, probability: u32) -> u32 {
        let step = (self.range >> 8) | ((self.range >> 7) & 1);
        let below = step * probability;
        let above = self.range.wrapping_sub(below);
        let one = self.code < above;
        if one {
            self.range = above;
        } else {
            self.range = below;
            self.code = self.code.wrapping_sub(above);
        }
        if self.range < CODER_HALF {
            let shift = CODER_PRECISION_BITS - 1 - self.range.checked_ilog2().unwrap_or(0);
            self.range <<= shift;
            self.code = (self.code << shift) | bits.bits(shift);
        }
        u32::from(one)
    }
}

fn first_probability(coefficient: i32) -> u32 {
    u32::from(((coefficient & 127) as u8).reverse_bits() >> 1) + 1
}

pub(crate) struct Unpacker {
    channels: usize,
    samples: usize,
    filters: Table,
    probabilities: Table,
    lookups: Vec<[[i16; 256]; HISTORY_BYTES]>,
}

impl Unpacker {
    pub(crate) fn new(channels: usize, samples_a_frame: usize) -> Self {
        Self {
            channels,
            samples: samples_a_frame,
            filters: Table::new(),
            probabilities: Table::new(),
            lookups: vec![[[0; 256]; HISTORY_BYTES]; MOST_ELEMENTS],
        }
    }

    pub(crate) const fn frame_bytes(&self) -> usize {
        self.samples / 8 * self.channels
    }

    pub(crate) fn unpack(&mut self, frame: &[u8], into: &mut [u8]) -> Result<(), Unpacking> {
        if frame.len() <= 1 {
            return Err(Unpacking::Empty);
        }
        let into = &mut into[..self.frame_bytes()];
        let mut bits = Bits::over(frame);

        if bits.bit() == 0 {
            bits.bit();
            if bits.bits(PLAIN_FRAME_PADDING_BITS) != 0 {
                return Err(Unpacking::PaddingNotZero);
            }
            let plain = frame.get(1..).unwrap_or_default();
            let copied = plain.len().min(into.len());
            into[..copied].copy_from_slice(&plain[..copied]);
            into[copied..].fill(DSD_SILENCE);
            return Ok(());
        }

        if bits.bit() == 0 || bits.bit() == 0 || bits.bit() == 0 {
            return Err(Unpacking::Segmented);
        }

        let same_map = bits.bit() == 1;
        let mut filter_of = [0_usize; MOST_CHANNELS];
        read_map(&mut bits, &mut self.filters, &mut filter_of, self.channels)?;
        let mut probability_of = filter_of;
        if same_map {
            self.probabilities.elements = self.filters.elements;
        } else {
            read_map(
                &mut bits,
                &mut self.probabilities,
                &mut probability_of,
                self.channels,
            )?;
        }

        let mut half = [false; MOST_CHANNELS];
        for held in half.iter_mut().take(self.channels) {
            *held = bits.bit() == 1;
        }

        read_table(&mut bits, &mut self.filters, &FILTERS)?;
        read_table(&mut bits, &mut self.probabilities, &PROBABILITIES)?;

        if bits.bit() == 1 {
            return Err(Unpacking::CoderNotStarted);
        }
        let mut coder = Coder::start(&mut bits);
        self.build_lookups()?;

        into.fill(0);
        let mut history = [FIRST_HISTORY; MOST_CHANNELS];
        coder.decode(
            &mut bits,
            first_probability(self.filters.coefficients[0][0]),
        );

        for sample in 0..self.samples {
            for channel in 0..self.channels {
                let filter = filter_of[channel];
                let lookup = &self.lookups[filter];
                let past = history[channel].to_le_bytes();
                let predicted = lookup.iter().zip(past).fold(0_i32, |sum, (table, byte)| {
                    sum + i32::from(table[usize::from(byte)])
                }) as i16;

                let probability = if !half[channel] || sample >= self.filters.length[filter] {
                    let element = probability_of[channel];
                    let index = (i32::from(predicted).unsigned_abs() >> 3) as usize;
                    let last = self.probabilities.length[element].saturating_sub(1);
                    self.probabilities.coefficients[element][index.min(last)] as u32
                } else {
                    EVEN_ODDS
                };

                let residual = coder.decode(&mut bits, probability);
                let heard = (u32::from(predicted < 0) ^ residual) & 1;
                into[(sample >> 3) * self.channels + channel] |=
                    (heard as u8) << (7 - (sample & 7));
                history[channel] = (history[channel] << 1) | u128::from(heard);
            }
        }
        Ok(())
    }

    fn build_lookups(&mut self) -> Result<(), Unpacking> {
        for element in 0..self.filters.elements {
            let length = self.filters.length[element];
            let coefficients = &self.filters.coefficients[element];
            for (byte, lookup) in self.lookups[element].iter_mut().enumerate() {
                let taps = length.saturating_sub(byte * 8).min(8);
                for (pattern, held) in lookup.iter_mut().enumerate() {
                    let sum = (0..taps).fold(0_i64, |sum, bit| {
                        let sign = if (pattern >> bit) & 1 == 1 { 1 } else { -1 };
                        sum + sign * i64::from(coefficients[byte * 8 + bit])
                    });
                    *held = i16::try_from(sum).map_err(|_| Unpacking::FilterOverflows)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Packed {
    pub(crate) at: u64,
    pub(crate) bytes: u64,
}

pub(crate) struct Unpacked {
    source: Box<dyn MediaStream>,
    frames: Vec<Packed>,
    unpacker: Unpacker,
    position: u64,
    held: Vec<u8>,
    held_frame: Option<usize>,
    packed: Vec<u8>,
}

impl Unpacked {
    pub(crate) fn over(
        source: Box<dyn MediaStream>,
        frames: Vec<Packed>,
        channels: usize,
        samples_a_frame: usize,
    ) -> Self {
        let unpacker = Unpacker::new(channels, samples_a_frame);
        let held = vec![DSD_SILENCE; unpacker.frame_bytes()];
        Self {
            source,
            frames,
            unpacker,
            position: 0,
            held,
            held_frame: None,
            packed: Vec::new(),
        }
    }

    fn length(&self) -> u64 {
        self.frames.len() as u64 * self.unpacker.frame_bytes() as u64
    }

    fn hold(&mut self, frame: usize) -> io::Result<()> {
        if self.held_frame == Some(frame) {
            return Ok(());
        }
        let packed = self.frames[frame];
        self.source.seek(SeekFrom::Start(packed.at))?;
        self.packed.clear();
        (&mut self.source)
            .take(packed.bytes)
            .read_to_end(&mut self.packed)?;
        self.held_frame = None;
        if let Err(refusal) = self.unpacker.unpack(&self.packed, &mut self.held) {
            tracing::debug!(?refusal, frame, "a DST frame would not unpack");
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "a DST frame would not unpack",
            ));
        }
        self.held_frame = Some(frame);
        Ok(())
    }
}

impl Read for Unpacked {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let frame_bytes = self.unpacker.frame_bytes() as u64;
        if buf.is_empty() || self.position >= self.length() || frame_bytes == 0 {
            return Ok(0);
        }
        let frame = (self.position / frame_bytes) as usize;
        let within = (self.position % frame_bytes) as usize;
        self.hold(frame)?;
        let from = &self.held[within..];
        let taken = from.len().min(buf.len());
        buf[..taken].copy_from_slice(&from[..taken]);
        self.position += taken as u64;
        Ok(taken)
    }
}

impl Seek for Unpacked {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let length = self.length();
        let target = match to {
            SeekFrom::Start(at) => Some(at),
            SeekFrom::End(back) => length.checked_add_signed(back),
            SeekFrom::Current(by) => self.position.checked_add_signed(by),
        };
        let Some(target) = target else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a seek before the start of the unpacked stream",
            ));
        };
        self.position = target;
        Ok(target)
    }
}

impl MediaStream for Unpacked {
    fn is_seekable(&self) -> bool {
        self.source.is_seekable()
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.length())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use resonate_core::{AudioBuffer, MediaLocation, SampleData};

    use super::*;
    use crate::{DecodeStatus, Decoder};

    const DSD64: u32 = 2_822_400;
    const SAMPLES_A_FRAME: usize = (DSD64 as u64 / FRAMES_A_SECOND) as usize;
    const FILTER: [i32; 12] = [120, -60, 40, 25, -18, 12, 9, -6, 4, 3, -2, 1];
    const ODDS: [i32; 12] = [120, 110, 96, 80, 64, 48, 40, 32, 24, 16, 8, 4];

    #[derive(Default)]
    struct Writer {
        bits: Vec<u8>,
    }

    impl Writer {
        fn put(&mut self, value: u32, count: u32) {
            for at in (0..count).rev() {
                self.bits.push(((value >> at) & 1) as u8);
            }
        }

        fn signed(&mut self, value: i32, count: u32) {
            self.put(value as u32 & ((1 << count) - 1), count);
        }

        fn rice(&mut self, value: i32, parameter: u32) {
            let magnitude = value.unsigned_abs();
            for _ in 0..magnitude >> parameter {
                self.bits.push(0);
            }
            self.bits.push(1);
            self.put(magnitude & ((1 << parameter) - 1), parameter);
            if magnitude != 0 {
                self.bits.push(u8::from(value < 0));
            }
        }

        fn bytes(&self) -> Vec<u8> {
            self.bits
                .chunks(8)
                .map(|byte| {
                    byte.iter()
                        .enumerate()
                        .fold(0_u8, |held, (at, bit)| held | (bit << (7 - at)))
                })
                .collect()
        }
    }

    struct Encoder {
        range: u32,
        low: u32,
        out: Writer,
    }

    impl Encoder {
        fn new(out: Writer) -> Self {
            Self {
                range: CODER_RANGE,
                low: 0,
                out,
            }
        }

        fn encode(&mut self, one: bool, probability: u32) {
            let step = (self.range >> 8) | ((self.range >> 7) & 1);
            let below = step * probability;
            let above = self.range - below;
            if one {
                self.range = above;
            } else {
                self.low += above;
                self.range = below;
                if self.low > CODER_RANGE {
                    self.low -= CODER_RANGE + 1;
                    self.carry();
                }
            }
            while self.range < CODER_HALF {
                self.out.bits.push(((self.low >> 11) & 1) as u8);
                self.low = (self.low << 1) & CODER_RANGE;
                self.range <<= 1;
            }
        }

        fn carry(&mut self) {
            for bit in self.out.bits.iter_mut().rev() {
                if *bit == 1 {
                    *bit = 0;
                } else {
                    *bit = 1;
                    return;
                }
            }
        }

        fn finish(mut self) -> Vec<u8> {
            self.out.put(self.low, CODER_PRECISION_BITS);
            self.out.bytes()
        }
    }

    fn lookups() -> [[i16; 256]; HISTORY_BYTES] {
        let mut lookup = [[0_i16; 256]; HISTORY_BYTES];
        for (byte, table) in lookup.iter_mut().enumerate() {
            let taps = FILTER.len().saturating_sub(byte * 8).min(8);
            for (pattern, held) in table.iter_mut().enumerate() {
                *held = (0..taps)
                    .map(|bit| {
                        let sign = if (pattern >> bit) & 1 == 1 { 1 } else { -1 };
                        sign * FILTER[byte * 8 + bit]
                    })
                    .sum::<i32>() as i16;
            }
        }
        lookup
    }

    fn packed(dsd: &[u8], channels: usize) -> Vec<u8> {
        let mut head = Writer::default();
        head.put(1, 1);
        head.put(0b111, 3);
        head.put(1, 1);
        head.put(1, 1);
        for _ in 0..channels {
            head.put(0, 1);
        }
        head.put(FILTER.len() as u32 - 1, FILTER_LENGTH_BITS);
        head.put(0, 1);
        for coefficient in FILTER {
            head.signed(coefficient, FILTER_COEFFICIENT_BITS);
        }
        head.put(ODDS.len() as u32 - 1, PROBABILITY_LENGTH_BITS);
        head.put(1, 1);
        head.put(0, PREDICTION_METHOD_BITS);
        head.put((ODDS[0] - PROBABILITY_OFFSET) as u32, PROBABILITY_BITS);
        let parameter = 3;
        head.put(parameter, RICE_PARAMETER_BITS);
        for at in 1..ODDS.len() {
            let predicted = -8 * ODDS[at - 1];
            let sent = if predicted >= 0 {
                ODDS[at] + (predicted + 4) / 8
            } else {
                ODDS[at] - (-predicted + 3) / 8
            };
            head.rice(sent, parameter);
        }
        head.put(0, 1);

        let lookup = lookups();
        let mut encoder = Encoder::new(head);
        encoder.encode(false, first_probability(FILTER[0]));
        let mut history = [FIRST_HISTORY; MOST_CHANNELS];
        for sample in 0..SAMPLES_A_FRAME {
            for channel in 0..channels {
                let past = history[channel].to_le_bytes();
                let predicted = lookup
                    .iter()
                    .zip(past)
                    .map(|(table, byte)| i32::from(table[usize::from(byte)]))
                    .sum::<i32>() as i16;
                let index = (i32::from(predicted).unsigned_abs() >> 3) as usize;
                let probability = ODDS[index.min(ODDS.len() - 1)] as u32;
                let heard = (dsd[(sample >> 3) * channels + channel] >> (7 - (sample & 7))) & 1;
                let residual = u32::from(heard) ^ u32::from(predicted < 0);
                encoder.encode(residual == 1, probability);
                history[channel] = (history[channel] << 1) | u128::from(heard);
            }
        }
        encoder.finish()
    }

    fn signal(channels: usize, frames: usize, seed: u32) -> Vec<u8> {
        let mut error = [0.0_f64; MOST_CHANNELS];
        let mut bytes = vec![0_u8; SAMPLES_A_FRAME / 8 * channels * frames];
        let mut noise = seed;
        for sample in 0..SAMPLES_A_FRAME * frames {
            for channel in 0..channels {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                let tone = 0.5
                    * (std::f64::consts::TAU * 1_000.0 * (channel + 1) as f64 * sample as f64
                        / f64::from(DSD64))
                    .sin();
                error[channel] += tone + f64::from(noise % 64) / 4_096.0;
                let high = error[channel] > 0.0;
                error[channel] -= if high { 1.0 } else { -1.0 };
                if high {
                    bytes[(sample >> 3) * channels + channel] |= 1 << (7 - (sample & 7));
                }
            }
        }
        bytes
    }

    #[test]
    fn a_coded_frame_unpacks_to_exactly_the_bits_that_were_packed() {
        for channels in [1, 2, 6] {
            let dsd = signal(channels, 1, 0x9E37_79B9);
            let frame = packed(&dsd, channels);

            let mut unpacker = Unpacker::new(channels, SAMPLES_A_FRAME);
            let mut held = vec![0; unpacker.frame_bytes()];
            unpacker
                .unpack(&frame, &mut held)
                .expect("a well-formed frame");
            assert_eq!(
                held, dsd,
                "{channels} channels did not come back bit for bit"
            );
        }
    }

    #[test]
    fn a_plain_frame_is_copied_and_a_short_one_padded_with_silence() {
        let mut unpacker = Unpacker::new(2, SAMPLES_A_FRAME);
        let mut held = vec![0; unpacker.frame_bytes()];
        let mut frame = vec![0_u8];
        frame.extend_from_slice(&[0x12, 0x34, 0x56]);
        unpacker.unpack(&frame, &mut held).expect("a plain frame");
        assert_eq!(&held[..3], &[0x12, 0x34, 0x56]);
        assert!(held[3..].iter().all(|byte| *byte == DSD_SILENCE));

        assert_eq!(
            unpacker.unpack(&[0x01, 0x00], &mut held),
            Err(Unpacking::PaddingNotZero)
        );
        assert_eq!(unpacker.unpack(&[0x80], &mut held), Err(Unpacking::Empty));
    }

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut held = id.to_vec();
        held.extend_from_slice(&(body.len() as u64).to_be_bytes());
        held.extend_from_slice(body);
        if body.len() % 2 == 1 {
            held.push(0);
        }
        held
    }

    fn dsdiff(channels: u16, compression: &[u8; 4], sound: &[u8]) -> Vec<u8> {
        let mut prop = b"SND ".to_vec();
        prop.extend(chunk(b"FS  ", &DSD64.to_be_bytes()));
        let mut named = channels.to_be_bytes().to_vec();
        for lane in 0..channels {
            named.extend_from_slice(format!("C{lane:03}").as_bytes());
        }
        prop.extend(chunk(b"CHNL", &named));
        let mut kind = compression.to_vec();
        kind.push(0);
        kind.push(0);
        prop.extend(chunk(b"CMPR", &kind));

        let mut body = b"DSD ".to_vec();
        body.extend(chunk(b"FVER", &0x0105_0000_u32.to_be_bytes()));
        body.extend(chunk(b"PROP", &prop));
        body.extend_from_slice(sound);
        let mut file = b"FRM8".to_vec();
        file.extend_from_slice(&(body.len() as u64).to_be_bytes());
        file.extend(body);
        file
    }

    fn drained(file: Vec<u8>, name: &str) -> Vec<i32> {
        let (mut decoder, info) =
            Decoder::open_reader(Cursor::new(file), &MediaLocation::local(name))
                .expect("a DSDIFF opens");
        let mut block = AudioBuffer::empty(info.spec);
        let mut samples = Vec::new();
        while decoder.next_block(&mut block).expect("a clean decode") == DecodeStatus::Decoded {
            if let SampleData::S24(held) = block.data() {
                samples.extend_from_slice(held);
            }
        }
        samples
    }

    #[test]
    fn a_dst_dsdiff_plays_exactly_what_the_same_bits_uncompressed_play() {
        let frames = 3;
        let dsd = signal(2, frames, 0x1234_5678);
        let frame_bytes = SAMPLES_A_FRAME / 8 * 2;

        let mut dst = Vec::new();
        let mut info = (frames as u32).to_be_bytes().to_vec();
        info.extend_from_slice(&(FRAMES_A_SECOND as u16).to_be_bytes());
        dst.extend(chunk(b"FRTE", &info));
        for (at, frame) in dsd.chunks(frame_bytes).enumerate() {
            if at == 1 {
                let mut plain = vec![0_u8];
                plain.extend_from_slice(frame);
                dst.extend(chunk(b"DSTF", &plain));
            } else {
                dst.extend(chunk(b"DSTF", &packed(frame, 2)));
            }
            dst.extend(chunk(b"DSTC", &[0; 4]));
        }

        let compressed = drained(dsdiff(2, b"DST ", &chunk(b"DST ", &dst)), "packed.dff");
        let plain = drained(dsdiff(2, b"DSD ", &chunk(b"DSD ", &dsd)), "plain.dff");
        assert!(!plain.is_empty());
        assert_eq!(compressed, plain, "a DST stream decoded to other samples");
    }
}
