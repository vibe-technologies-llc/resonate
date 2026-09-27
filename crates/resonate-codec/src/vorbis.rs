const IDENTIFICATION: u8 = 1;
const SETUP: u8 = 5;
const MAGIC: &[u8; 6] = b"vorbis";
const HEADERS: usize = 3;
const BLOCK_SIZES_AT: usize = 28;
const SMALLEST_BLOCK_EXPONENT: u8 = 6;
const LARGEST_BLOCK_EXPONENT: u8 = 13;
const LOW_NIBBLE: u8 = 0x0F;
const MOST_MODES: u32 = 64;
const MODE_BITS: u32 = 40;
const MODE_COUNT_BITS: u32 = 6;
const MAPPING_BITS: u32 = 8;
const LARGEST_MAPPING: u32 = 63;
const TRANSFORM_BITS: u32 = 16;
const WINDOW_BITS: u32 = 16;
const LEAST_SETUP_BITS_LEFT: u64 = 97;
const XIPH_LACE_CONTINUES: u8 = 0xFF;
const AUDIO_PACKET_FLAG: u8 = 1;
const WINDOW_QUARTERS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Windows {
    short: u32,
    long: u32,
    long_modes: u64,
    mode_bits: u32,
}

impl Windows {
    pub(crate) fn of_codec_private(private: &[u8]) -> Option<Self> {
        let [identification, _, setup] = xiph_laced(private)?;
        let (short, long) = block_sizes(identification)?;
        let long_modes = modes(setup)?;

        Some(Self {
            short,
            long,
            long_modes: long_modes.flags,
            mode_bits: bits_to_count(long_modes.count),
        })
    }

    pub(crate) fn block_of(self, packet: &[u8]) -> Option<u32> {
        let &first = packet.first()?;
        if first & AUDIO_PACKET_FLAG != 0 {
            return None;
        }
        let mode = u32::from(first >> 1) & ((1 << self.mode_bits) - 1);
        let long = self.long_modes.checked_shr(mode)? & 1 == 1;

        Some(if long { self.long } else { self.short })
    }

    pub(crate) const fn samples_between(previous: Option<u32>, current: u32) -> u64 {
        let before = match previous {
            Some(previous) => previous,
            None => current,
        };

        ((before + current) / WINDOW_QUARTERS) as u64
    }
}

fn xiph_laced(private: &[u8]) -> Option<[&[u8]; HEADERS]> {
    let (&more, mut rest) = private.split_first()?;
    if usize::from(more) + 1 != HEADERS {
        return None;
    }
    let mut sizes = [0_usize; HEADERS - 1];
    for size in &mut sizes {
        loop {
            let (&byte, after) = rest.split_first()?;
            rest = after;
            *size = size.checked_add(usize::from(byte))?;
            if byte != XIPH_LACE_CONTINUES {
                break;
            }
        }
    }
    let (identification, rest) = rest.split_at_checked(sizes[0])?;
    let (comment, setup) = rest.split_at_checked(sizes[1])?;

    Some([identification, comment, setup])
}

fn headed(packet: &[u8], kind: u8) -> bool {
    packet.first() == Some(&kind) && packet.get(1..=MAGIC.len()) == Some(MAGIC.as_slice())
}

fn block_sizes(identification: &[u8]) -> Option<(u32, u32)> {
    if !headed(identification, IDENTIFICATION) {
        return None;
    }
    let &packed = identification.get(BLOCK_SIZES_AT)?;
    let (short, long) = (packed & LOW_NIBBLE, packed >> 4);
    let usable =
        |exponent: u8| (SMALLEST_BLOCK_EXPONENT..=LARGEST_BLOCK_EXPONENT).contains(&exponent);
    if !usable(short) || !usable(long) || short > long {
        return None;
    }

    Some((1 << short, 1 << long))
}

struct Modes {
    count: u32,
    flags: u64,
}

fn modes(setup: &[u8]) -> Option<Modes> {
    if !headed(setup, SETUP) {
        return None;
    }
    let mut backwards = Backwards::over(setup);
    loop {
        if backwards.left() <= LEAST_SETUP_BITS_LEFT {
            return None;
        }
        if backwards.bit()? {
            break;
        }
    }
    let framing = backwards.read;

    let mut count = None;
    let mut tried = 0;
    while backwards.left() >= LEAST_SETUP_BITS_LEFT {
        let mapping = backwards.bits(MAPPING_BITS)?;
        let transform = backwards.bits(TRANSFORM_BITS)?;
        let window = backwards.bits(WINDOW_BITS)?;
        if mapping > LARGEST_MAPPING || transform != 0 || window != 0 {
            break;
        }
        backwards.bit()?;
        tried += 1;
        if tried > MOST_MODES {
            break;
        }
        let after_the_modes = backwards.read;
        if backwards.bits(MODE_COUNT_BITS)? + 1 == tried {
            count = Some(tried);
        }
        backwards.read = after_the_modes;
    }
    let count = count?;

    backwards.read = framing;
    let mut flags = 0_u64;
    for mode in (0..count).rev() {
        backwards.skip(MODE_BITS);
        if backwards.bit()? {
            flags |= 1 << mode;
        }
    }

    Some(Modes { count, flags })
}

fn bits_to_count(count: u32) -> u32 {
    u32::BITS - count.saturating_sub(1).leading_zeros()
}

struct Backwards<'a> {
    bytes: &'a [u8],
    read: u64,
}

impl<'a> Backwards<'a> {
    const fn over(bytes: &'a [u8]) -> Self {
        Self { bytes, read: 0 }
    }

    fn left(&self) -> u64 {
        (self.bytes.len() as u64 * 8).saturating_sub(self.read)
    }

    fn skip(&mut self, bits: u32) {
        self.read += u64::from(bits);
    }

    fn bit(&mut self) -> Option<bool> {
        let byte = self.read / 8;
        let at = self
            .bytes
            .len()
            .checked_sub(1 + usize::try_from(byte).ok()?)?;
        let shift = 7 - (self.read % 8);
        self.read += 1;

        Some((self.bytes[at] >> shift) & 1 == 1)
    }

    fn bits(&mut self, count: u32) -> Option<u32> {
        (0..count).try_fold(0_u32, |held, _| Some((held << 1) | u32::from(self.bit()?)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Forwards {
        bytes: Vec<u8>,
        written: u64,
    }

    impl Forwards {
        fn new() -> Self {
            Self {
                bytes: Vec::new(),
                written: 0,
            }
        }

        fn put(&mut self, value: u32, bits: u32) {
            for bit in 0..bits {
                if self.written.is_multiple_of(8) {
                    self.bytes.push(0);
                }
                let last = self.bytes.len() - 1;
                self.bytes[last] |= (((value >> bit) & 1) as u8) << (self.written % 8);
                self.written += 1;
            }
        }
    }

    fn setup(long_flags: &[bool], padding_bytes: usize) -> Vec<u8> {
        let mut packet = vec![SETUP];
        packet.extend_from_slice(MAGIC);
        packet.extend(std::iter::repeat_n(0xA5, padding_bytes));
        let mut modes = Forwards::new();
        modes.put(long_flags.len() as u32 - 1, MODE_COUNT_BITS);
        for (mapping, &long) in long_flags.iter().enumerate() {
            modes.put(u32::from(long), 1);
            modes.put(0, WINDOW_BITS);
            modes.put(0, TRANSFORM_BITS);
            modes.put(mapping as u32 % 2, MAPPING_BITS);
        }
        modes.put(1, 1);
        packet.extend(modes.bytes);
        packet
    }

    fn identification(short: u8, long: u8) -> Vec<u8> {
        let mut packet = vec![IDENTIFICATION];
        packet.extend_from_slice(MAGIC);
        packet.resize(BLOCK_SIZES_AT, 0);
        packet.push(short | (long << 4));
        packet.push(1);
        packet
    }

    fn laced(headers: [&[u8]; HEADERS]) -> Vec<u8> {
        let mut private = vec![2];
        for header in &headers[..2] {
            let mut size = header.len();
            while size >= usize::from(XIPH_LACE_CONTINUES) {
                private.push(XIPH_LACE_CONTINUES);
                size -= usize::from(XIPH_LACE_CONTINUES);
            }
            private.push(size as u8);
        }
        for header in headers {
            private.extend_from_slice(header);
        }
        private
    }

    #[test]
    fn the_block_sizes_and_which_modes_are_long_are_read_out_of_the_codec_private() {
        let private = laced([
            &identification(8, 11),
            b"\x03vorbis comment",
            &setup(&[false, true], 300),
        ]);

        let windows = Windows::of_codec_private(&private).expect("the headers read");

        assert_eq!(
            windows,
            Windows {
                short: 256,
                long: 2_048,
                long_modes: 0b10,
                mode_bits: 1,
            }
        );
        assert_eq!(windows.block_of(&[0b0000_0000]), Some(256));
        assert_eq!(windows.block_of(&[0b0000_0010]), Some(2_048));
        assert_eq!(
            windows.block_of(&[0b0000_0001]),
            None,
            "a header packet is no audio"
        );
    }

    #[test]
    fn a_packet_is_a_quarter_of_the_window_before_it_and_a_quarter_of_its_own() {
        assert_eq!(
            Windows::samples_between(None, 2_048),
            1_024,
            "the first packet renders half its own window where gapless trimming is off"
        );
        assert_eq!(Windows::samples_between(Some(2_048), 2_048), 1_024);
        assert_eq!(Windows::samples_between(Some(256), 2_048), 576);
        assert_eq!(Windows::samples_between(Some(2_048), 256), 576);
    }

    #[test]
    fn a_single_mode_names_itself_in_no_bits_of_the_packet() {
        let private = laced([&identification(8, 11), b"\x03vorbis", &setup(&[true], 200)]);
        let windows = Windows::of_codec_private(&private).expect("the headers read");

        assert_eq!(windows.mode_bits, 0);
        assert_eq!(windows.block_of(&[0b1111_1110]), Some(2_048));
    }

    #[test]
    fn headers_that_are_not_vorbis_or_not_three_name_no_windows() {
        let good_setup = setup(&[false, true], 300);
        assert!(Windows::of_codec_private(&[]).is_none());
        assert!(
            Windows::of_codec_private(&laced([b"\x01opus", b"\x03vorbis", &good_setup])).is_none()
        );
        let mut two = laced([&identification(8, 11), b"\x03vorbis", &good_setup]);
        two[0] = 1;
        assert!(Windows::of_codec_private(&two).is_none());
        assert!(
            Windows::of_codec_private(&laced([&identification(12, 8), b"\x03vorbis", &good_setup]))
                .is_none()
        );
    }
}
