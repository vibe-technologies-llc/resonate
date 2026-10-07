use std::{fs, path::PathBuf};

use resonate_codec::{DecodeStatus, Decoder, Sources};
use resonate_core::{AudioBuffer, MediaLocation};

const BLOCKS_WEIGHED: usize = 4_096;
const LONGEST_BLOCK: usize = 1 << 16;

fn found() -> Vec<PathBuf> {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/found");
    let mut found: Vec<PathBuf> = fs::read_dir(folder)
        .expect("the folder of inputs a fuzz run found")
        .map(|entry| entry.expect("a listed input").path())
        .collect();
    found.sort();
    found
}

#[test]
fn no_input_a_fuzz_run_found_decodes_into_a_block_longer_than_any_decoder_makes() {
    for path in found() {
        let Ok((mut decoder, info)) =
            Decoder::open(&Sources::local(), &MediaLocation::local(&path))
        else {
            continue;
        };
        let mut block = AudioBuffer::empty(info.spec);

        for _ in 0..BLOCKS_WEIGHED {
            match decoder.next_block(&mut block) {
                Ok(DecodeStatus::Decoded) => assert!(
                    block.frames() <= LONGEST_BLOCK,
                    "{} decoded {} frames into one block",
                    path.display(),
                    block.frames()
                ),
                Ok(DecodeStatus::EndOfStream) | Err(_) => break,
            }
        }
    }
}
