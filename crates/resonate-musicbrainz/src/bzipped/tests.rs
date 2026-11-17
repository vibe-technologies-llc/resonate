use std::io::{self, Read, Write};

use bzip2::{Compression, read::MultiBzDecoder, write::BzEncoder};

use super::{decoded, decoded_across, read_whole};

const SEED: u64 = 0x6d69_627a_3220_6273;
const THREADS: usize = 3;
const SIZES: [usize; 14] = [
    0, 1, 2, 3, 4, 5, 255, 256, 4096, 99_999, 100_001, 250_000, 900_001, 1_000_000,
];
const SMALLEST_BLOCKS: u32 = 1;
const LARGEST_BLOCKS: u32 = 9;
const STREAMS_ONE_AFTER_ANOTHER: usize = 64;
const LONGEST_SMALL_STREAM: usize = 5000;
const CUTS_AN_ARCHIVE: usize = 48;
const FLIPS_AN_ARCHIVE: usize = 96;
const TRAILING: [&[u8]; 6] = [
    b"\0",
    b"\0\0\0\0",
    b"BZ",
    b"BZh",
    b"BZh91AY&SY",
    b"trailing",
];
const SHORTER_THAN_THE_FIRST_RANDOMISED_BYTE: usize = 600;
const RANDOMISED_FLAG_BYTE: usize = 14;
const RANDOMISED_FLAG_BIT: u8 = 0x80;
const RUN_LENGTHS_AT_THE_EDGES: [usize; 10] = [1, 2, 3, 4, 5, 6, 254, 255, 256, 260];

struct Seeded(u64);

impl Seeded {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut mixed = self.0;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        mixed ^ (mixed >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn byte(&mut self) -> u8 {
        self.next().to_le_bytes()[0]
    }
}

fn is_a_randomised_block(refusal: &io::Error) -> bool {
    refusal
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<mibz2::Error>())
        .is_some_and(|refused| refused.kind == mibz2::ErrorKind::Randomised)
}

#[derive(Clone, Copy, Debug)]
enum Kind {
    Random,
    Text,
    Runs,
}

const KINDS: [Kind; 3] = [Kind::Random, Kind::Text, Kind::Runs];

fn made(kind: Kind, len: usize, seeded: &mut Seeded) -> Vec<u8> {
    match kind {
        Kind::Random => (0..len).map(|_| seeded.byte()).collect(),
        Kind::Text => text(len, seeded),
        Kind::Runs => runs(len, seeded),
    }
}

fn text(len: usize, seeded: &mut Seeded) -> Vec<u8> {
    const WORDS: [&[u8]; 8] = [
        b"recording\t",
        b"release\t",
        b"\\N\t",
        b"artist_credit\t",
        b"2026-11-17\t",
        b"f1d1f2e3-0a4b-4c5d-8e6f-7a8b9c0d1e2f\t",
        b"Official\t",
        b"\n",
    ];
    let mut text = Vec::with_capacity(len + 64);
    while text.len() < len {
        text.extend_from_slice(WORDS[seeded.below(WORDS.len())]);
    }
    text.truncate(len);
    text
}

fn runs(len: usize, seeded: &mut Seeded) -> Vec<u8> {
    let mut runs = Vec::with_capacity(len + 512);
    while runs.len() < len {
        let length = if seeded.below(2) == 0 {
            RUN_LENGTHS_AT_THE_EDGES[seeded.below(RUN_LENGTHS_AT_THE_EDGES.len())]
        } else {
            1 + seeded.below(4096)
        };
        let byte = seeded.byte() % 4;
        runs.extend(std::iter::repeat_n(byte, length));
    }
    runs.truncate(len);
    runs
}

fn packed(plain: &[u8], blocks: u32) -> Vec<u8> {
    let mut encoder = BzEncoder::new(Vec::new(), Compression::new(blocks));
    encoder.write_all(plain).expect("an archive written");
    encoder.finish().expect("an archive finished")
}

fn by_the_reference(packed: &[u8]) -> io::Result<Vec<u8>> {
    let mut plain = Vec::new();
    MultiBzDecoder::new(packed).read_to_end(&mut plain)?;
    Ok(plain)
}

fn alone(packed: &[u8]) -> io::Result<Vec<u8>> {
    read_whole(decoded(packed), u64::MAX)
}

fn across(packed: &[u8]) -> io::Result<Vec<u8>> {
    read_whole(
        decoded_across(io::Cursor::new(packed.to_vec()), THREADS),
        u64::MAX,
    )
}

fn assert_decoded_as_the_reference_decodes(packed: &[u8], plain: &[u8], case: &str) {
    let reference = by_the_reference(packed).expect("the reference decodes it");
    let alone = alone(packed).unwrap_or_else(|error| panic!("{case}: refused alone: {error}"));
    let across = across(packed).unwrap_or_else(|error| panic!("{case}: refused across: {error}"));

    assert!(reference == plain, "{case}: the reference read other bytes");
    assert!(
        alone == reference,
        "{case}: alone, {} bytes against {}",
        alone.len(),
        reference.len()
    );
    assert!(
        across == reference,
        "{case}: across, {} bytes against {}",
        across.len(),
        reference.len()
    );
}

#[test]
fn every_archive_decodes_byte_for_byte_as_libbz2_decodes_it() {
    let mut seeded = Seeded(SEED);

    for kind in KINDS {
        for len in SIZES {
            for blocks in [SMALLEST_BLOCKS, LARGEST_BLOCKS] {
                let plain = made(kind, len, &mut seeded);
                let packed = packed(&plain, blocks);

                assert_decoded_as_the_reference_decodes(
                    &packed,
                    &plain,
                    &format!("{kind:?}, {len} bytes, blocks of {blocks}"),
                );
            }
        }
    }
}

#[test]
fn many_small_streams_one_after_another_decode_as_their_bytes_in_order() {
    let mut seeded = Seeded(SEED ^ 1);
    let mut plain = Vec::new();
    let mut packed_streams = Vec::new();

    for _ in 0..STREAMS_ONE_AFTER_ANOTHER {
        let kind = KINDS[seeded.below(KINDS.len())];
        let len = if seeded.below(8) == 0 {
            0
        } else {
            seeded.below(LONGEST_SMALL_STREAM)
        };
        let blocks = 1 + seeded.below(9) as u32;
        let stream = made(kind, len, &mut seeded);
        packed_streams.extend(packed(&stream, blocks));
        plain.extend(stream);
    }

    assert_decoded_as_the_reference_decodes(&packed_streams, &plain, "streams one after another");
}

#[test]
fn no_input_at_all_is_refused_as_the_reference_refuses_it() {
    assert!(by_the_reference(&[]).is_err());
    assert!(alone(&[]).is_err());
    assert!(across(&[]).is_err());
}

#[test]
fn bytes_after_the_last_stream_are_refused_as_the_reference_refuses_them() {
    let mut seeded = Seeded(SEED ^ 4);

    for (case, _, packed) in archives_to_harm(&mut seeded) {
        for trailing in TRAILING {
            let mut followed = packed.clone();
            followed.extend_from_slice(trailing);

            assert!(
                by_the_reference(&followed).is_err(),
                "{case} + {trailing:?}: the reference"
            );
            assert!(
                alone(&followed).is_err(),
                "{case} + {trailing:?}: read alone"
            );
            assert!(
                across(&followed).is_err(),
                "{case} + {trailing:?}: read across"
            );
        }
    }
}

fn archives_to_harm(seeded: &mut Seeded) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    [
        (Kind::Random, 10_000, LARGEST_BLOCKS),
        (Kind::Text, 300_000, SMALLEST_BLOCKS),
        (Kind::Runs, 250_000, SMALLEST_BLOCKS),
        (Kind::Text, 40, LARGEST_BLOCKS),
    ]
    .into_iter()
    .map(|(kind, len, blocks)| {
        let plain = made(kind, len, seeded);
        let packed = packed(&plain, blocks);
        (
            format!("{kind:?}, {len} bytes, blocks of {blocks}"),
            plain,
            packed,
        )
    })
    .collect()
}

#[test]
fn a_cut_archive_is_refused_never_read_short() {
    let mut seeded = Seeded(SEED ^ 2);

    for (case, _, packed) in archives_to_harm(&mut seeded) {
        for _ in 0..CUTS_AN_ARCHIVE {
            let kept = 1 + seeded.below(packed.len() - 1);
            let cut = &packed[..kept];

            assert!(
                by_the_reference(cut).is_err(),
                "{case}, cut to {kept}: the reference read it"
            );
            assert!(alone(cut).is_err(), "{case}, cut to {kept}: read alone");
            assert!(across(cut).is_err(), "{case}, cut to {kept}: read across");
        }
    }
}

#[test]
fn a_flipped_bit_is_refused_or_leaves_every_byte_as_it_was() {
    let mut seeded = Seeded(SEED ^ 3);

    for (case, plain, packed) in archives_to_harm(&mut seeded) {
        for _ in 0..FLIPS_AN_ARCHIVE {
            let bit = seeded.below(packed.len() * 8);
            let mut flipped = packed.clone();
            flipped[bit / 8] ^= 1 << (bit % 8);

            let reference = by_the_reference(&flipped);

            for (reader, read) in [("alone", alone(&flipped)), ("across", across(&flipped))] {
                match read {
                    Ok(read) => {
                        assert!(
                            read == plain,
                            "{case}, bit {bit} flipped: {reader} read other bytes"
                        );
                    }
                    Err(refusal) => assert!(
                        reference.is_err() || is_a_randomised_block(&refusal),
                        "{case}, bit {bit} flipped: {reader} refused what the reference reads: \
                         {refusal}"
                    ),
                }
            }
        }
    }
}

#[test]
fn a_randomised_block_is_refused_though_the_reference_reads_it() {
    let plain = text(
        SHORTER_THAN_THE_FIRST_RANDOMISED_BYTE,
        &mut Seeded(SEED ^ 5),
    );
    let mut randomised = packed(&plain, LARGEST_BLOCKS);
    randomised[RANDOMISED_FLAG_BYTE] ^= RANDOMISED_FLAG_BIT;

    assert_eq!(by_the_reference(&randomised).ok(), Some(plain));
    assert!(alone(&randomised).is_err_and(|refusal| is_a_randomised_block(&refusal)));
    assert!(across(&randomised).is_err_and(|refusal| is_a_randomised_block(&refusal)));
}
