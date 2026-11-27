use std::{
    f64::consts::TAU,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

use resonate_core::{MediaLocation, SampleFormat, SampleRate, TrackId};
use resonate_library::{
    Ceiling, KeptOriginal, Library, Recompressed, Result, SizedAfter, Sources, faults,
};
use resonate_vault::{FlacHead, Halt, SampleDigest, heard_at_depth};

use crate::support::{
    LOW_TIDE_SHEET, TITLE, Tree, Wav, all, opened_with_a_vault, options, scan, vaulted,
};

const BLOCK: usize = 4_096;
const STREAM_INFO: u8 = 0;
const VORBIS_COMMENT: u8 = 4;
const PICTURE: u8 = 6;
const LAST_BLOCK: u8 = 0x80;
const VERBATIM: u8 = 0b0000_0010;
static NEVER: AtomicBool = AtomicBool::new(false);

struct Plain {
    rate: u32,
    bits: u8,
    channels: u8,
    frames: usize,
    noise: f64,
}

impl Plain {
    const fn cd(frames: usize) -> Self {
        Self {
            rate: 44_100,
            bits: 16,
            channels: 2,
            frames,
            noise: 0.0,
        }
    }

    fn samples(&self) -> Vec<i32> {
        let full = f64::from(1_u32 << (self.bits - 1)) - 1.0;
        let mut state = 0x2545_f491_u32;
        let mut samples = Vec::with_capacity(self.frames * usize::from(self.channels));

        for frame in 0..self.frames {
            let moment = frame as f64 / f64::from(self.rate);

            for channel in 0..self.channels {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;

                let noise = (f64::from(state) / f64::from(u32::MAX) - 0.5) * self.noise;
                let tone = (TAU * (440.0 + 110.0 * f64::from(channel)) * moment).sin() * 0.4;

                samples.push(((tone + noise) * full).round() as i32);
            }
        }
        samples
    }

    fn written(&self, tags: &[(&str, &str)], picture: Option<&[u8]>) -> Vec<u8> {
        let samples = self.samples();
        let mut digest = SampleDigest::default();

        digest.note(&samples, self.bits);

        let mut file = b"fLaC".to_vec();
        let mut blocks = vec![(STREAM_INFO, self.stream_info(*digest.settled().as_bytes()))];

        blocks.push((VORBIS_COMMENT, comments(tags)));
        if let Some(picture) = picture {
            blocks.push((PICTURE, picture_block(picture)));
        }

        let last = blocks.len() - 1;

        for (at, (kind, body)) in blocks.into_iter().enumerate() {
            let length = (body.len() as u32).to_be_bytes();

            file.push(kind | if at == last { LAST_BLOCK } else { 0 });
            file.extend_from_slice(&length[1..]);
            file.extend_from_slice(&body);
        }

        let channels = usize::from(self.channels);

        for (number, frame) in samples.chunks(BLOCK * channels).enumerate() {
            file.extend(self.frame(number, frame));
        }
        file
    }

    fn stream_info(&self, digest: [u8; 16]) -> Vec<u8> {
        let mut body = Vec::with_capacity(34);
        let packed = (u64::from(self.rate) << 44)
            | (u64::from(self.channels - 1) << 41)
            | (u64::from(self.bits - 1) << 36)
            | self.frames as u64;

        body.extend_from_slice(&(BLOCK as u16).to_be_bytes());
        body.extend_from_slice(&(BLOCK as u16).to_be_bytes());
        body.extend_from_slice(&[0; 6]);
        body.extend_from_slice(&packed.to_be_bytes());
        body.extend_from_slice(&digest);
        body
    }

    fn frame(&self, number: usize, samples: &[i32]) -> Vec<u8> {
        let channels = usize::from(self.channels);
        let frames = samples.len() / channels;
        let mut frame = vec![0xff, 0xf8, 0b0111_0000, (self.channels - 1) << 4];
        let mut coded = [0_u8; 4];

        frame.extend_from_slice(
            char::from_u32(number as u32)
                .expect("a frame number a character holds")
                .encode_utf8(&mut coded)
                .as_bytes(),
        );
        frame.extend_from_slice(&((frames - 1) as u16).to_be_bytes());
        frame.push(crc8(&frame));

        let width = usize::from(self.bits / 8);

        for channel in 0..channels {
            frame.push(VERBATIM);
            for sample in samples.iter().skip(channel).step_by(channels) {
                frame.extend_from_slice(&sample.to_be_bytes()[4 - width..]);
            }
        }
        frame.extend_from_slice(&crc16(&frame).to_be_bytes());
        frame
    }
}

fn comments(tags: &[(&str, &str)]) -> Vec<u8> {
    let vendor = b"a plain writer";
    let mut body = (vendor.len() as u32).to_le_bytes().to_vec();

    body.extend_from_slice(vendor);
    body.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for (key, value) in tags {
        let comment = format!("{key}={value}");

        body.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        body.extend_from_slice(comment.as_bytes());
    }
    body
}

fn picture_block(data: &[u8]) -> Vec<u8> {
    let mime = b"image/png";
    let mut body = 3_u32.to_be_bytes().to_vec();

    body.extend_from_slice(&(mime.len() as u32).to_be_bytes());
    body.extend_from_slice(mime);
    body.extend_from_slice(&0_u32.to_be_bytes());
    for value in [1_u32, 1, 24, 0] {
        body.extend_from_slice(&value.to_be_bytes());
    }
    body.extend_from_slice(&(data.len() as u32).to_be_bytes());
    body.extend_from_slice(data);
    body
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0_u8;

    for byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = match crc & 0x80 {
                0 => crc << 1,
                _ => (crc << 1) ^ 0x07,
            };
        }
    }
    crc
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0_u16;

    for byte in bytes {
        crc ^= u16::from(*byte) << 8;
        for _ in 0..8 {
            crc = match crc & 0x8000 {
                0 => crc << 1,
                _ => (crc << 1) ^ 0x8005,
            };
        }
    }
    crc
}

fn heard(path: &Path, bits: u8) -> resonate_vault::HeardSamples {
    heard_at_depth(
        &Sources::local(),
        &MediaLocation::local(path),
        bits,
        Halt::NEVER,
    )
    .expect("a file that decodes")
}

fn head_of(path: &Path) -> FlacHead {
    FlacHead::read(&mut fs::File::open(path).expect("a file")).expect("a FLAC head")
}

fn scanned(tree: &Tree) -> Result<Library> {
    let library = Library::open_in_memory()?;

    scan(&library, &options(tree))?;
    Ok(library)
}

fn the_id_of(library: &Library, path: &Path) -> Result<TrackId> {
    Ok(all(library)?
        .into_iter()
        .find(|track| track.location.as_path() == Some(path))
        .expect("the file's row")
        .id)
}

fn left_beside(path: &Path) -> Vec<PathBuf> {
    let mut left: Vec<PathBuf> = fs::read_dir(path.parent().expect("a folder"))
        .expect("a readable folder")
        .flatten()
        .map(|entry| entry.path())
        .filter(|entry| entry != path)
        .collect();

    left.sort();
    left
}

const PICTURE_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n a picture held whole";

#[test]
fn a_flac_written_plainly_is_compressed_smaller_bit_for_bit_and_keeps_its_tags_picture_and_row()
-> Result<()> {
    let tree = Tree::new();
    let path = tree.write(
        "Tideline/Lantern.flac",
        &Plain::cd(44_100).written(
            &[
                ("TITLE", "Lantern"),
                ("ARTIST", "Example Band"),
                ("MOOD", "still"),
            ],
            Some(PICTURE_BYTES),
        ),
    );
    let before = fs::metadata(&path).expect("a file").len();
    let heard_before = heard(&path, 16);
    let blocks_before = head_of(&path).blocks;
    let library = scanned(&tree)?;

    let id = the_id_of(&library, &path)?;

    library.track_played(&MediaLocation::local(&path), None, Duration::from_secs(1))?;

    let outcome = library.recompress(id, Ceiling::Kept, &NEVER)?;

    let Recompressed::Smaller(shrunk) = outcome else {
        panic!("a plainly written FLAC was kept: {outcome:?}");
    };
    let after = fs::metadata(&path).expect("the file").len();
    let row = library.track(id)?.expect("the row kept its id");

    assert_eq!(shrunk.before, before);
    assert_eq!(shrunk.after, after);
    assert!(after < before / 2, "{after} of {before}");
    assert_eq!(heard(&path, 16), heard_before);
    assert_eq!(head_of(&path).blocks, blocks_before);
    assert_eq!(row.file_size, after);
    assert_eq!(row.title, "Lantern");
    assert_eq!(row.plays, 1);
    assert_eq!(left_beside(&path), Vec::<PathBuf>::new());
    Ok(())
}

#[test]
fn a_96_khz_24_bit_flac_under_the_cd_ceiling_becomes_44_khz_16_bit() -> Result<()> {
    let tree = Tree::new();
    let plain = Plain {
        rate: 96_000,
        bits: 24,
        channels: 2,
        frames: 96_000,
        noise: 0.001,
    };

    let path = tree.write(
        "Tideline/Harbour.flac",
        &plain.written(&[("TITLE", "Harbour")], None),
    );
    let library = scanned(&tree)?;

    let id = the_id_of(&library, &path)?;

    let outcome = library.recompress(id, Ceiling::Cd, &NEVER)?;

    let Recompressed::Smaller(shrunk) = outcome else {
        panic!("a high resolution FLAC was kept under the CD ceiling: {outcome:?}");
    };
    let head = head_of(&path);
    let row = library.track(id)?.expect("the row kept its id");
    let read = heard(&path, 16);

    assert_eq!(shrunk.from.rate, SampleRate::HZ_96000);
    assert_eq!((shrunk.to.rate, shrunk.to.bits), (SampleRate::HZ_44100, 16));
    assert_eq!((head.info.rate, head.info.bits), (44_100, 16));
    assert_eq!(read.frames.get(), 44_100);
    assert_eq!(row.spec.rate, SampleRate::HZ_44100);
    assert_eq!(row.spec.format, SampleFormat::S16);
    assert_eq!(row.title, "Harbour");
    Ok(())
}

#[test]
fn a_flac_the_encoder_wrote_already_is_kept_byte_for_byte() -> Result<()> {
    let tree = Tree::new();
    let plain = Plain {
        noise: 0.05,
        ..Plain::cd(44_100)
    };

    let written = tree.write(
        "Noise/Plain.flac",
        &plain.written(&[("TITLE", "Noise")], None),
    );
    let library = scanned(&tree)?;

    let id = the_id_of(&library, &written)?;

    let first = library.recompress(id, Ceiling::Kept, &NEVER)?;
    let bytes = fs::read(&written).expect("the file");
    let second = library.recompress(id, Ceiling::Kept, &NEVER)?;

    assert!(matches!(first, Recompressed::Smaller(_)), "{first:?}");
    assert!(
        matches!(second, Recompressed::Kept(KeptOriginal::NoSmaller { .. })),
        "{second:?}"
    );
    assert_eq!(fs::read(&written).expect("the file"), bytes);
    assert_eq!(left_beside(&written), Vec::<PathBuf>::new());
    Ok(())
}

#[test]
fn a_copy_that_does_not_read_back_as_it_went_in_keeps_the_original() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write(
        "Tideline/Lantern.flac",
        &Plain::cd(44_100).written(&[("TITLE", "Lantern")], None),
    );
    let bytes = fs::read(&path).expect("the file");
    let library = scanned(&tree)?;

    let id = the_id_of(&library, &path)?;
    let spoiled = faults::spoil_the_copies_of(&path);

    let outcome = library.recompress(id, Ceiling::Kept, &NEVER)?;

    drop(spoiled);

    assert_eq!(outcome, Recompressed::Kept(KeptOriginal::Unverified));
    assert_eq!(fs::read(&path).expect("the file"), bytes);
    assert_eq!(left_beside(&path), Vec::<PathBuf>::new());
    Ok(())
}

#[test]
fn a_cue_sheets_cut_and_a_vaulted_row_are_refused_and_their_files_left() -> Result<()> {
    let tree = Tree::new();
    let held = Tree::new();
    let cut = tree.write("Sheet/Tideline.wav", &Wav::new().frames(44_100).build());

    tree.write("Sheet/Tideline.cue", LOW_TIDE_SHEET.as_bytes());

    let lone = tree.write(
        "Lone/Lantern.wav",
        &Wav::new().text(TITLE, "Lantern").build(),
    );
    let (library, _vault) = opened_with_a_vault(&held)?;

    scan(&library, &options(&tree))?;
    vaulted(&library, true)?;

    let rows = all(&library)?;
    let cuts: Vec<TrackId> = rows
        .iter()
        .filter(|track| track.location.as_path() == Some(cut.as_path()))
        .map(|track| track.id)
        .collect();
    let lone_id = the_id_of(&library, &lone)?;

    assert!(cuts.len() > 1, "{rows:?}");

    for id in cuts {
        assert_eq!(
            library.recompress(id, Ceiling::Kept, &NEVER)?,
            Recompressed::Kept(KeptOriginal::CutBySheet)
        );
    }

    assert_eq!(
        library.recompress(lone_id, Ceiling::Kept, &NEVER)?,
        Recompressed::Kept(KeptOriginal::Vaulted)
    );
    Ok(())
}

#[test]
fn a_pass_over_the_library_shrinks_what_it_can_and_counts_what_it_saved() -> Result<()> {
    let tree = Tree::new();
    let plain = tree.write(
        "Tideline/Lantern.flac",
        &Plain::cd(44_100).written(&[("TITLE", "Lantern")], None),
    );
    let noise = Plain {
        noise: 0.05,
        ..Plain::cd(22_050)
    };

    let tight = tree.write(
        "Noise/Static.flac",
        &noise.written(&[("TITLE", "Static")], None),
    );
    let library = scanned(&tree)?;

    let tight_id = the_id_of(&library, &tight)?;

    library.recompress(tight_id, Ceiling::Kept, &NEVER)?;

    let plain_before = fs::metadata(&plain).expect("a file").len();
    let candidates = library.recompress_candidates()?;
    let summary = library.recompress_all(Ceiling::Kept)?.join()?;

    let plain_after = fs::metadata(&plain).expect("a file").len();

    assert_eq!(candidates.len(), 2);
    assert_eq!(summary.stats.candidates, 2);
    assert_eq!(summary.stats.weighed, 2);
    assert_eq!(summary.stats.smaller, 1);
    assert_eq!(summary.stats.kept, 1);
    assert_eq!(summary.stats.saved(), plain_before - plain_after);
    assert!(!summary.cancelled);
    assert_eq!(
        library
            .track(the_id_of(&library, &plain)?)?
            .expect("the row")
            .file_size,
        plain_after
    );
    Ok(())
}

#[test]
fn the_largest_files_are_listed_first_a_page_at_a_time_with_a_sheets_cuts_as_one_file() -> Result<()>
{
    let tree = Tree::new();
    let small = tree.write("small.wav", &Wav::new().frames(1_000).build());
    let large = tree.write("large.wav", &Wav::new().frames(30_000).build());
    let alike_a = tree.write("alike a.wav", &Wav::new().frames(10_000).build());
    let alike_b = tree.write("alike b.wav", &Wav::new().frames(10_000).build());
    let cut = tree.write("Tideline.wav", &Wav::new().frames(44_100).build());

    tree.write("Tideline.cue", LOW_TIDE_SHEET.as_bytes());

    let library = scanned(&tree)?;
    let mut paged = Vec::new();
    let mut after: Option<SizedAfter> = None;

    loop {
        let page = library.largest_files(after.as_ref(), 2)?;

        let Some(last) = page.last() else {
            break;
        };

        after = Some(last.key());
        paged.extend(page);
    }

    let whole = library.largest_files(None, 100)?;
    let paths: Vec<&Path> = paged.iter().map(|file| file.path.as_path()).collect();
    let cut_rows = all(&library)?
        .into_iter()
        .filter(|track| track.location.as_path() == Some(cut.as_path()))
        .count();

    assert_eq!(paged, whole);
    assert_eq!(
        paths,
        [
            cut.as_path(),
            large.as_path(),
            alike_a.as_path(),
            alike_b.as_path(),
            small.as_path()
        ]
    );
    assert_eq!(paged[0].tracks.len(), cut_rows);
    assert!(cut_rows > 1);
    assert!(paged.windows(2).all(|pair| pair[0].bytes >= pair[1].bytes));
    assert!(paged.iter().all(|file| !file.vaulted));
    Ok(())
}
