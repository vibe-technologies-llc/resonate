#![no_main]

use std::io::{self, Read};

use bzip2::read::MultiBzDecoder;
use libfuzzer_sys::fuzz_target;

const THREADS: usize = 2;
const DECODED_AT_MOST: u64 = 64 * 1024 * 1024;

fn by_the_reference(packed: &[u8]) -> io::Result<Vec<u8>> {
    let mut plain = Vec::new();
    MultiBzDecoder::new(packed)
        .take(DECODED_AT_MOST)
        .read_to_end(&mut plain)?;
    Ok(plain)
}

fn is_a_randomised_block(refusal: &io::Error) -> bool {
    refusal
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<mibz2::Error>())
        .is_some_and(|refused| refused.kind == mibz2::ErrorKind::Randomised)
}

fn reached_the_bound(read: &io::Result<Vec<u8>>) -> bool {
    read.as_ref()
        .is_ok_and(|plain| plain.len() as u64 == DECODED_AT_MOST)
}

fuzz_target!(|packed: &[u8]| {
    let alone = resonate_musicbrainz::bzip2_decoded(packed, DECODED_AT_MOST);
    let across = resonate_musicbrainz::bzip2_decoded_across(packed, THREADS, DECODED_AT_MOST);
    let reference = by_the_reference(packed);

    if [&alone, &across, &reference]
        .into_iter()
        .any(reached_the_bound)
    {
        return;
    }

    match (&alone, &across) {
        (Ok(alone), Ok(across)) => assert!(alone == across, "alone and across read other bytes"),
        (Err(_), Err(_)) => {}
        _ => panic!(
            "alone {:?} against across {:?}",
            alone.as_ref().map(Vec::len),
            across.as_ref().map(Vec::len)
        ),
    }

    match (&alone, &reference) {
        (Ok(alone), Ok(reference)) => assert!(alone == reference, "the reference read other bytes"),
        (Err(refusal), Ok(_)) => assert!(
            is_a_randomised_block(refusal),
            "refused what the reference reads: {refusal}"
        ),
        (Ok(alone), Err(refusal)) => panic!(
            "read {} bytes the reference refuses: {refusal}",
            alone.len()
        ),
        (Err(_), Err(_)) => {}
    }
});
