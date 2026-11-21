use std::{
    fs,
    path::{Path, PathBuf},
};

use resonate_codec::{DecodeStatus, Decoder, Sources, probe};
use resonate_core::{AudioBuffer, MediaLocation};

const BLOCKS_WEIGHED: usize = 4_096;
const LONGEST_BLOCK: usize = 1 << 16;
const FOUND_AT_LEAST: usize = 14;
const TIMED_BY_MILLISECONDS: &str = "tone-id3.mp3";
const TIMED_BY_MPEG_FRAMES: &str = "tone-sylt-frames.mp3";

fn inputs_in(folder: PathBuf) -> Vec<PathBuf> {
    let mut inputs: Vec<PathBuf> = fs::read_dir(folder)
        .expect("a folder of inputs for the probe fuzz target")
        .map(|entry| entry.expect("a listed input").path())
        .collect();

    inputs.sort();
    inputs
}

fn found() -> Vec<PathBuf> {
    inputs_in(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/found"))
}

fn seeds() -> Vec<PathBuf> {
    inputs_in(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/probe"))
}

fn weigh_the_blocks_of(path: &Path) {
    let _ = probe(&Sources::local(), &MediaLocation::local(path));

    let Ok((mut decoder, info)) = Decoder::open(&Sources::local(), &MediaLocation::local(path))
    else {
        return;
    };

    decoder.follow_format_changes();

    let mut block = AudioBuffer::empty(info.spec);

    for _ in 0..BLOCKS_WEIGHED {
        match decoder.next_block(&mut block) {
            Ok(DecodeStatus::Decoded) => assert!(
                block.frames() <= LONGEST_BLOCK,
                "{} decoded {} frames into one block",
                path.display(),
                block.frames()
            ),
            Ok(DecodeStatus::Reformatted(_)) => {}
            Ok(DecodeStatus::EndOfStream) | Err(_) => break,
        }
    }
}

#[test]
fn no_input_a_fuzz_run_found_decodes_into_a_block_longer_than_any_decoder_makes() {
    let found = found();

    assert!(found.len() >= FOUND_AT_LEAST);

    for path in found {
        weigh_the_blocks_of(&path);
    }
}

#[test]
fn no_seed_of_the_probe_target_decodes_into_a_block_longer_than_any_decoder_makes() {
    let seeds = seeds();

    assert!(!seeds.is_empty());

    for path in seeds {
        weigh_the_blocks_of(&path);
    }
}

#[test]
fn the_probe_seeds_reach_the_timed_lyrics_reader_in_either_stamp_format() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/probe");

    for seed in [TIMED_BY_MILLISECONDS, TIMED_BY_MPEG_FRAMES] {
        let info = probe(&Sources::local(), &MediaLocation::local(folder.join(seed)))
            .expect("a seed that probes");
        let lyrics = info.tags.lyrics.expect("words the seed's frame times");

        assert!(lyrics.starts_with("[00:"), "{seed} read as {lyrics:?}");
    }
}
