#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::indexing_slicing
)]

const WORD_BYTES: usize = 4;
const LEFT_ALIGNED_BY_BITS: u32 = 8;

pub fn left_aligned(window: &mut [u8]) {
    let (words, _) = window.as_chunks_mut::<WORD_BYTES>();
    for word in words {
        let shifted = i32::from_le_bytes(*word) << LEFT_ALIGNED_BY_BITS;
        *word = shifted.to_le_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(bytes: &[u8]) -> Vec<i32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_le_bytes(*word))
            .collect()
    }

    #[test]
    fn twenty_four_bits_sit_against_the_top_of_their_word() {
        let mut window = Vec::new();
        for word in [1_i32, -1, 0x7F_FFFF, -0x80_0000] {
            window.extend_from_slice(&word.to_le_bytes());
        }

        left_aligned(&mut window);

        assert_eq!(
            words(&window),
            [1 << 8, -1 << 8, 0x7F_FFFF << 8, -0x80_0000 << 8]
        );
    }

    #[test]
    fn bytes_short_of_a_word_are_left_as_they_were() {
        let mut window = [1_u8, 0, 0, 0, 0xAB, 0xCD];

        left_aligned(&mut window);

        assert_eq!(window, [0, 1, 0, 0, 0xAB, 0xCD]);
    }
}
