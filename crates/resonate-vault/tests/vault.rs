use std::{
    env, fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::{self, Command},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use resonate_codec::{
    CoverArt, DecodeStatus, Decoder, FormatHint, ImageFormat, Media, MediaProvider, Reading,
    Sources, probe,
};
use resonate_core::{AudioBuffer, MediaLocation, SampleData, SampleFormat, SourceId, StreamSpec};
use resonate_vault::{Form, Keeping, Kept, Taking, Vault, VaultFiles};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const FRAMES: usize = 40_000;

static NEXT: AtomicU32 = AtomicU32::new(0);

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        let root = env::temp_dir().join(format!(
            "resonate-vault-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a scratch tree");
        Self { root }
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        fs::write(&path, bytes).expect("a written fixture");
        path
    }

    fn vault(&self) -> Vault {
        Vault::open(self.root.join("vault")).expect("a vault")
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct InMemory {
    source: SourceId,
    bytes: Vec<u8>,
}

impl MediaProvider for InMemory {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn open(&self, _location: &MediaLocation) -> resonate_codec::Result<Media> {
        Ok(Media {
            stream: Box::new(Reading::new(Cursor::new(self.bytes.clone()))),
            hint: Some(FormatHint::Extension("wav".into())),
        })
    }
}

fn signal(frames: usize) -> Vec<i32> {
    let mut held = Vec::with_capacity(frames * usize::from(CHANNELS));
    let mut state = 0x1234_5678_u32;
    for at in 0..frames {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((state >> 16) & 0x7ff) as i32 - 0x400;
        let tone = ((at as f64 * 0.05).sin() * 6000.0) as i32;
        held.push(tone + noise);
        held.push(tone - noise);
    }
    held
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn wave(bits: u16, float: bool, data: &[u8], extra: Option<&[u8]>) -> Vec<u8> {
    let block_align = CHANNELS * (bits / 8);
    let mut format = Vec::new();
    format.extend_from_slice(&(if float { 3_u16 } else { 1_u16 }).to_le_bytes());
    format.extend_from_slice(&CHANNELS.to_le_bytes());
    format.extend_from_slice(&RATE.to_le_bytes());
    format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&bits.to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    chunk(&mut body, b"fmt ", &format);
    if let Some(extra) = extra {
        chunk(&mut body, b"LIST", extra);
    }
    chunk(&mut body, b"data", data);

    let mut whole = Vec::new();
    whole.extend_from_slice(b"RIFF");
    whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
    whole.extend_from_slice(&body);
    whole
}

fn sixteen_bit(samples: &[i32], extra: Option<&[u8]>) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| (*sample as i16).to_le_bytes())
        .collect();
    wave(16, false, &data, extra)
}

fn twenty_four_bit(samples: &[i32]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| {
            let widened = sample << 8;
            widened.to_le_bytes()[..3].to_vec()
        })
        .collect();
    wave(24, false, &data, None)
}

fn floating(samples: &[i32]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| (f32::from(*sample as i16) / 32_768.0).to_le_bytes())
        .collect();
    wave(32, true, &data, None)
}

fn decoded(path: &Path, format: SampleFormat) -> Vec<i32> {
    let sources = Sources::local();
    let location = MediaLocation::local(path);
    let (mut decoder, info) = Decoder::open(&sources, &location).expect("a readable object");
    decoder.set_output_format(format);

    let spec = StreamSpec::new(info.spec.rate, info.spec.channels, format);
    let mut block = AudioBuffer::empty(spec);
    let mut held = Vec::new();
    while decoder.next_block(&mut block).expect("a decoded block") == DecodeStatus::Decoded {
        match block.data() {
            SampleData::S16(samples) => held.extend(samples.iter().map(|s| i32::from(*s))),
            SampleData::S24(samples) | SampleData::S32(samples) => held.extend_from_slice(samples),
            SampleData::F32(samples) => {
                held.extend(samples.iter().map(|s| s.to_bits().cast_signed()));
            }
        }
    }
    held
}

fn kept(vault: &Vault, sources: &Sources, location: &MediaLocation) -> Kept {
    match vault
        .keep(&Taking {
            sources,
            location,
            span: None,
            renewing: false,
        })
        .expect("a vault that kept it")
    {
        Keeping::Kept(kept) => kept,
        Keeping::Refused(refusal) => panic!("refused: {}", refusal.as_str()),
    }
}

fn picture(width: u32, height: u32) -> CoverArt {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[
                (x * 7 % 256) as u8,
                (y * 11 % 256) as u8,
                ((x + y) * 3 % 256) as u8,
                255,
            ]);
        }
    }
    let drawn = image::RgbaImage::from_raw(width, height, pixels).expect("a drawn picture");
    let mut written = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(drawn)
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a written picture");

    CoverArt {
        format: ImageFormat::Png,
        bytes: written.into_inner(),
    }
}

#[test]
fn a_sixteen_bit_source_is_kept_as_flac_and_decodes_back_sample_for_sample() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("sixteen.wav", &sixteen_bit(&samples, None));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    assert_eq!(held.form, Form::Flac);
    assert!(!held.deduped);
    assert_eq!(held.frames.get(), FRAMES as u64);
    assert_eq!(held.path.extension().and_then(|e| e.to_str()), Some("flac"));
    assert_eq!(decoded(&held.path, SampleFormat::S16), samples);
    assert!(
        vault
            .verify(&held.path, Form::Flac)
            .expect("a verified object")
    );
}

#[test]
fn a_delivered_stream_is_kept_like_a_file_and_nothing_is_left_in_staging() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let vault = tree.vault();
    let mut delivery = Cursor::new(sixteen_bit(&samples, None));

    let Keeping::Kept(held) = vault
        .keep_delivered(&mut delivery, "wav")
        .expect("a kept delivery")
    else {
        panic!("a delivery of plain PCM was refused");
    };

    assert_eq!(held.form, Form::Flac);
    assert_eq!(decoded(&held.path, SampleFormat::S16), samples);
    assert_eq!(
        fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0
    );
}

#[test]
fn a_twenty_four_bit_source_is_kept_as_flac_and_decodes_back_sample_for_sample() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("twentyfour.wav", &twenty_four_bit(&samples));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));
    let widened: Vec<i32> = samples.iter().map(|sample| sample << 8).collect();

    assert_eq!(held.form, Form::Flac);
    assert_eq!(decoded(&held.path, SampleFormat::S24), widened);
    assert!(
        vault
            .verify(&held.path, Form::Flac)
            .expect("a verified object")
    );
}

#[test]
fn an_object_the_vault_wrote_carries_no_tags_and_no_picture() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let named = b"INFOINAM\x08\x00\x00\x00A title\0".to_vec();
    let path = tree.write("tagged.wav", &sixteen_bit(&samples, Some(&named)));
    let vault = tree.vault();

    let sources = Sources::local();
    let before = probe(&sources, &MediaLocation::local(&path)).expect("a readable source");
    assert_eq!(before.tags.title.as_deref(), Some("A title"));

    let held = kept(&vault, &sources, &MediaLocation::local(&path));
    let after = probe(&sources, &MediaLocation::local(&held.path)).expect("a readable object");

    assert_eq!(after.tags, resonate_codec::TagSet::default());
    assert!(
        resonate_codec::probe_cover_art(&sources, &MediaLocation::local(&held.path))
            .expect("a readable object")
            .is_none()
    );
}

#[test]
fn the_same_audio_in_two_containers_settles_on_one_object() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let plain = tree.write("plain.wav", &sixteen_bit(&samples, None));
    let listed = tree.write(
        "listed.wav",
        &sixteen_bit(&samples, Some(b"INFOINAM\x06\x00\x00\x00Named\0")),
    );
    let vault = tree.vault();
    let sources = Sources::local();

    let first = kept(&vault, &sources, &MediaLocation::local(&plain));
    let second = kept(&vault, &sources, &MediaLocation::local(&listed));

    assert_ne!(fs::read(&plain).unwrap(), fs::read(&listed).unwrap());
    assert_eq!(first.key, second.key);
    assert_eq!(first.path, second.path);
    assert!(!first.deduped);
    assert!(second.deduped);
    assert_eq!(vault.holding().expect("a counted vault").objects, 1);
}

#[test]
fn a_float_source_is_kept_as_a_compressed_wave_and_reads_back_bit_for_bit() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("floating.wav", &floating(&samples));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    assert_eq!(held.form, Form::Wave);
    assert!(held.path.to_string_lossy().ends_with(".wav.zst"));
    assert!(held.bytes < fs::metadata(&path).unwrap().len());
    assert!(
        vault
            .verify(&held.path, Form::Wave)
            .expect("a verified object")
    );

    let through = Sources::local().and(Arc::new(VaultFiles::over(&vault)));
    let (mut decoder, info) =
        Decoder::open(&through, &MediaLocation::local(&held.path)).expect("a readable object");
    decoder.set_output_format(SampleFormat::F32);

    let spec = StreamSpec::new(info.spec.rate, info.spec.channels, SampleFormat::F32);
    let mut block = AudioBuffer::empty(spec);
    let mut read = Vec::new();
    while decoder.next_block(&mut block).expect("a decoded block") == DecodeStatus::Decoded {
        if let SampleData::F32(held) = block.data() {
            read.extend_from_slice(held);
        }
    }

    let wanted: Vec<f32> = samples
        .iter()
        .map(|sample| f32::from(*sample as i16) / 32_768.0)
        .collect();
    assert_eq!(read, wanted);
}

#[test]
fn a_location_served_out_of_memory_is_kept_like_any_other() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let vault = tree.vault();

    let source = SourceId::new("memory").expect("a lowercase name");
    let sources = Sources::local().and(Arc::new(InMemory {
        source: source.clone(),
        bytes: sixteen_bit(&samples, None),
    }));
    let location = MediaLocation::new(source, "offered/one");

    let held = kept(&vault, &sources, &location);

    assert_eq!(held.form, Form::Flac);
    assert_eq!(decoded(&held.path, SampleFormat::S16), samples);
}

#[test]
fn a_cover_read_back_out_of_the_vault_is_the_picture_that_went_in() {
    let tree = Tree::new();
    let vault = tree.vault();
    let art = picture(61, 47);

    let held = vault.keep_cover(&art).expect("a kept cover");
    assert_eq!(held.width, 61);
    assert_eq!(held.height, 47);
    assert!(!held.deduped);
    assert_eq!(held.path.extension().and_then(|e| e.to_str()), Some("jxl"));

    let drawn = vault.picture(&held.path).expect("a drawable cover");
    assert_eq!(drawn.format, ImageFormat::Png);

    let went_in = image::load_from_memory(&art.bytes)
        .expect("a picture")
        .to_rgba8();
    let came_out = image::load_from_memory(&drawn.bytes)
        .expect("a picture")
        .to_rgba8();
    assert_eq!(went_in, came_out);

    let again = vault.keep_cover(&art).expect("a kept cover");
    assert!(again.deduped);
    assert_eq!(vault.holding().expect("a counted vault").covers, 1);
}

#[test]
fn a_cover_is_drawn_once_and_a_forgotten_one_is_not_drawn_again_from_memory() {
    let tree = Tree::new();
    let vault = tree.vault();
    let held = vault.keep_cover(&picture(33, 21)).expect("a kept cover");

    let first = vault.picture(&held.path).expect("a drawable cover");
    std::fs::write(&held.path, b"no longer a picture").expect("a meddled cover");
    let second = vault.picture(&held.path).expect("the drawn cover held");
    assert_eq!(first, second);

    assert!(vault.forget(&held.path).expect("a forgotten cover"));
    assert!(vault.picture(&held.path).is_err());
}

#[test]
fn a_renewal_replaces_the_object_standing_under_its_key_only_where_it_comes_out_smaller() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("renewed.wav", &sixteen_bit(&samples, None));
    let vault = tree.vault();
    let location = MediaLocation::local(&path);
    let sources = Sources::local();
    let renewed = || match vault
        .keep(&Taking {
            sources: &sources,
            location: &location,
            span: None,
            renewing: true,
        })
        .expect("a vault that weighed it again")
    {
        Keeping::Kept(kept) => kept,
        Keeping::Refused(refusal) => panic!("refused: {}", refusal.as_str()),
    };

    let first = kept(&vault, &sources, &location);
    let again = renewed();
    assert!(again.deduped);
    assert!(!again.replaced);
    assert_eq!(again.path, first.path);

    let mut padded = fs::read(&first.path).expect("a standing object");
    padded.extend_from_slice(&[0; 4096]);
    fs::write(&first.path, &padded).expect("a heavier standing object");

    let smaller = renewed();
    assert!(smaller.replaced);
    assert!(!smaller.deduped);
    assert_eq!(smaller.path, first.path);
    assert_eq!(smaller.bytes, first.bytes);
    assert_eq!(decoded(&smaller.path, SampleFormat::S16), samples);
}

#[test]
fn an_object_that_has_been_meddled_with_is_not_verified() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("meddled.wav", &sixteen_bit(&samples, None));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));
    assert!(
        vault
            .verify(&held.path, Form::Flac)
            .expect("a verified object")
    );

    let mut bytes = fs::read(&held.path).expect("an object");
    let at = bytes.len() / 2;
    bytes[at] ^= 0xff;
    fs::write(&held.path, &bytes).expect("a meddled object");

    assert!(!vault.verify(&held.path, Form::Flac).unwrap_or(false));
}

#[test]
fn the_file_the_vault_read_is_left_exactly_as_it_was() {
    let tree = Tree::new();
    let samples = signal(FRAMES);
    let bytes = sixteen_bit(&samples, Some(b"INFOINAM\x06\x00\x00\x00Named\0"));
    let path = tree.write("untouched.wav", &bytes);
    let before = fs::metadata(&path).expect("a source file");
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));
    let after = fs::metadata(&path).expect("a source file");

    assert!(held.path.starts_with(vault.root()));
    assert_eq!(fs::read(&path).expect("a source file"), bytes);
    assert_eq!(before.len(), after.len());
    assert_eq!(
        before.modified().expect("a stamp"),
        after.modified().expect("a stamp")
    );
}

#[test]
fn nothing_outside_the_vault_is_read_or_taken_away() {
    let tree = Tree::new();
    let outside = tree.write("outside.wav", b"not the vault's to touch");
    let vault = tree.vault();

    assert!(!vault.holds(&outside));
    assert!(vault.forget(&outside).is_err());
    assert!(vault.picture(&outside).is_err());
    assert!(vault.verify(&outside, Form::Flac).is_err());
    assert_eq!(
        fs::read(&outside).expect("a file left alone"),
        b"not the vault's to touch"
    );
}

fn reference_tools() -> bool {
    let found = Command::new("flac")
        .arg("--version")
        .output()
        .is_ok_and(|answered| answered.status.success());
    if !found {
        eprintln!("skipped: no flac to weigh what this build wrote");
    }
    found
}

#[test]
fn the_reference_decoder_reads_what_this_build_wrote_without_a_complaint() {
    if !reference_tools() {
        return;
    }

    let tree = Tree::new();
    let samples = signal(FRAMES);
    let path = tree.write("weighed.wav", &sixteen_bit(&samples, None));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    let tested = Command::new("flac")
        .args(["--test", "--silent", "--warnings-as-errors"])
        .arg(&held.path)
        .output()
        .expect("a reference decoder");
    assert!(
        tested.status.success(),
        "{}",
        String::from_utf8_lossy(&tested.stderr)
    );

    let theirs = tree.root.join("theirs.flac");
    let encoded = Command::new("flac")
        .args(["--silent", "--force", "-8", "-o"])
        .arg(&theirs)
        .arg(&path)
        .output()
        .expect("a reference encoder");
    assert!(encoded.status.success());

    let ours = fs::metadata(&held.path).expect("an object").len();
    let reference = fs::metadata(&theirs).expect("a reference object").len();
    assert!(
        ours <= reference,
        "this build wrote {ours} bytes where flac -8 wrote {reference}"
    );
}

fn noise(frames: usize) -> Vec<i32> {
    let mut held = Vec::with_capacity(frames * usize::from(CHANNELS));
    let mut state = 0x9e37_79b9_u32;
    for _ in 0..frames * usize::from(CHANNELS) {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        held.push((state >> 8) as i32 - 0x0080_0000);
    }
    held
}

fn raw_twenty_four_bit(samples: &[i32]) -> Vec<u8> {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes()[..3].to_vec())
        .collect();
    wave(24, false, &data, None)
}

#[test]
fn an_object_is_never_larger_than_the_file_it_came_from() {
    let tree = Tree::new();
    let bytes = raw_twenty_four_bit(&noise(FRAMES));
    let path = tree.write("incompressible.wav", &bytes);
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    assert_eq!(held.form, Form::Kept);
    assert!(
        held.bytes <= bytes.len() as u64,
        "the vault wrote {} bytes where the source held {}",
        held.bytes,
        bytes.len()
    );
    assert_eq!(fs::read(&held.path).expect("an object"), bytes);
    assert_eq!(decoded(&held.path, SampleFormat::S24), noise(FRAMES));
}

fn surround(channels: u16, mask: u32, rate: u32, frames: usize) -> Vec<u8> {
    const EXTENSIBLE: u16 = 0xFFFE;
    const EXTENSION_BYTES: u16 = 22;
    const PCM_SUBFORMAT: [u8; 16] = [
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xaa, 0x00, 0x38, 0x9b,
        0x71,
    ];
    const BITS: u16 = 16;

    let block_align = channels * (BITS / 8);
    let mut format = Vec::new();
    format.extend_from_slice(&EXTENSIBLE.to_le_bytes());
    format.extend_from_slice(&channels.to_le_bytes());
    format.extend_from_slice(&rate.to_le_bytes());
    format.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    format.extend_from_slice(&block_align.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());
    format.extend_from_slice(&EXTENSION_BYTES.to_le_bytes());
    format.extend_from_slice(&BITS.to_le_bytes());
    format.extend_from_slice(&mask.to_le_bytes());
    format.extend_from_slice(&PCM_SUBFORMAT);

    let mut data = Vec::with_capacity(frames * usize::from(block_align));
    for at in 0..frames {
        for lane in 0..channels {
            let tone = ((at as f64 * 0.01 * f64::from(lane + 1)).sin() * 4000.0) as i16;
            data.extend_from_slice(&tone.to_le_bytes());
        }
    }

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut whole = Vec::new();
    whole.extend_from_slice(b"RIFF");
    whole.extend_from_slice(&(body.len() as u32).to_le_bytes());
    whole.extend_from_slice(&body);
    whole
}

#[test]
fn a_surround_source_is_kept_with_the_speakers_it_names() {
    const QUAD: u32 = 0x33;
    const TWO_POINT_ONE: u32 = 0x0B;
    const SIDE_FIVE_POINT_ONE: u32 = 0x60F;
    const FIVE_POINT_ONE: u32 = 0x3F;
    const PAST_WHAT_FLAC_HOLDS: u32 = 192_000;

    let tree = Tree::new();
    let vault = tree.vault();
    let sources = Sources::local();

    for (name, channels, mask, rate, frames, carried_by_flac) in [
        ("quad.wav", 4, QUAD, RATE, FRAMES, true),
        ("two-one.wav", 3, TWO_POINT_ONE, RATE, FRAMES, false),
        ("side.wav", 6, SIDE_FIVE_POINT_ONE, RATE, FRAMES, false),
        (
            "five-one.wav",
            6,
            FIVE_POINT_ONE,
            PAST_WHAT_FLAC_HOLDS,
            FRAMES + 1,
            false,
        ),
    ] {
        let path = tree.write(name, &surround(channels, mask, rate, frames));
        let location = MediaLocation::local(&path);
        let went_in = probe(&sources, &location).expect("a readable source");

        let held = kept(&vault, &sources, &location);
        assert_eq!(
            held.form == Form::Flac,
            carried_by_flac,
            "{name} was kept as {:?}",
            held.form
        );
        let came_out = probe(
            &Sources::local().and(Arc::new(VaultFiles::over(&vault))),
            &MediaLocation::local(&held.path),
        )
        .expect("a readable object");
        assert_eq!(
            came_out.speakers, went_in.speakers,
            "{name} came back with other speakers than it went in with"
        );
        assert_eq!(came_out.spec.channels, went_in.spec.channels);
    }
}

#[test]
fn a_source_wider_than_flac_holds_is_kept_as_it_stands_and_heard_through_the_copy() {
    const TEN_CHANNELS: u16 = 10;
    const TEN_SPEAKERS: u32 = 0x3FF;

    let tree = Tree::new();
    let file = surround(TEN_CHANNELS, TEN_SPEAKERS, RATE, FRAMES);
    let path = tree.write("ten.wav", &file);
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    assert_eq!(held.form, Form::Kept);
    assert_eq!(fs::read(&held.path).expect("an object"), file);
    assert_eq!(
        decoded(&held.path, SampleFormat::S16),
        decoded(&path, SampleFormat::S16)
    );
}

#[test]
fn a_wave_that_would_come_out_no_smaller_is_given_up_on_and_the_source_kept() {
    const HIGH_RATE: u32 = 192_000;
    const RATE_AT: std::ops::Range<usize> = 24..28;
    const BYTES_A_SECOND_AT: std::ops::Range<usize> = 28..32;

    let tree = Tree::new();
    let mut state = 0x9e37_79b9_u32;
    let noise: Vec<i32> = (0..FRAMES * usize::from(CHANNELS))
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            i32::from(state as i16)
        })
        .collect();
    let mut file = sixteen_bit(&noise, None);
    file[RATE_AT].copy_from_slice(&HIGH_RATE.to_le_bytes());
    file[BYTES_A_SECOND_AT].copy_from_slice(&(HIGH_RATE * u32::from(CHANNELS) * 2).to_le_bytes());
    let path = tree.write("noise.wav", &file);
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    assert_eq!(held.form, Form::Kept);
    assert_eq!(fs::read(&held.path).expect("an object"), file);
    assert_eq!(
        fs::read_dir(vault.root().join("staging"))
            .expect("the staging folder")
            .count(),
        0,
        "a compression given up on left its staging behind"
    );
    assert!(
        fs::read_dir(vault.root().join("audio"))
            .expect("the audio folder")
            .flatten()
            .flat_map(|fanned| fs::read_dir(fanned.path()).expect("a fanned folder"))
            .flatten()
            .all(|object| object.path() == held.path),
        "a wave object was landed beside the kept source"
    );
}

#[test]
fn a_kept_mp3_sheds_its_tags_and_keeps_every_frame_it_decodes_to() {
    const ID3V1_BYTES: usize = 128;

    let tree = Tree::new();
    let source = tree.write("tone.wav", &sixteen_bit(&signal(FRAMES), None));
    let path = tree.root.join("tagged.mp3");
    let encoded = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .args(["-c:a", "libmp3lame", "-b:a", "128k", "-write_id3v1", "1"])
        .args([
            "-metadata",
            "title=Echoes",
            "-metadata",
            "artist=Pink Floyd",
        ])
        .arg(&path)
        .status();
    if !encoded.is_ok_and(|status| status.success()) {
        eprintln!("skipped: no ffmpeg with libmp3lame to write a tagged MP3");
        return;
    }
    let before = fs::read(&path).expect("the tagged file");
    assert!(before.starts_with(b"ID3"));
    assert!(before[before.len() - ID3V1_BYTES..].starts_with(b"TAG"));
    let vault = tree.vault();

    let held = kept(&vault, &Sources::local(), &MediaLocation::local(&path));

    let object = fs::read(&held.path).expect("an object");
    assert_eq!(held.form, Form::Kept);
    assert!(!object.starts_with(b"ID3"), "the leading tag was kept");
    assert!(
        !object[object.len() - ID3V1_BYTES..].starts_with(b"TAG"),
        "the trailing tag was kept"
    );
    assert!(object.len() < before.len());
    assert_eq!(
        decoded(&held.path, SampleFormat::S16),
        decoded(&path, SampleFormat::S16)
    );
    assert_eq!(fs::read(&path).expect("the tagged file"), before);
    assert!(
        probe(&Sources::local(), &MediaLocation::local(&held.path))
            .expect("a probe of the object")
            .tags
            .title
            .is_none()
    );
}
