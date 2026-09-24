use std::{
    env, fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_codec::{
    Codec, Container, CueStamp, CueStart, DecodeStatus, Decoder, Faststart, FileTags, Picturing,
    Sources, TagEdit, TagField, TagSink, TagSource, Writing, probe, probe_cover_art, probe_stream,
};
use resonate_core::{
    AudioBuffer, ChannelLayout, FrameSpan, Frames, MediaLocation, SampleFormat, SampleRate,
    StreamSpec,
};

const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;
const SECONDS: usize = 2;

const PCM_EXTENSIBLE: u16 = 0xFFFE;
const PCM: u16 = 1;
const SURROUND_MASK: u32 = 0x3F;
const PCM_SUBFORMAT: [u8; 16] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71,
];

const DSD64: u32 = 2_822_400;
const DSF_BLOCK: usize = 4_096;

const ADTS_HEADER_BYTES: usize = 7;
const ADTS_CRC_BYTES: usize = 2;
const ADTS_SYNC: u8 = 0xFF;
const ADTS_SYNC_TAIL: u8 = 0xF0;
const ADTS_PROTECTION_ABSENT: u8 = 0x1;

const AAC_FRAMES_PER_PACKET: u32 = 1_024;
const AAC_PRIMING: u32 = 1_024;
const CAF_VERSION: u16 = 1;
const CAF_MPEG4_AAC_LC: u32 = 2;
const CAF_VARIABLE_BYTES_PER_PACKET: u32 = 0;
const CAF_UNUSED_BITS_PER_CHANNEL: u32 = 0;
const CAF_NO_EDITS: u32 = 0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum BitsPerSample {
    LeastFirst,
    MostFirst,
}

impl BitsPerSample {
    const fn declared(self) -> u32 {
        match self {
            Self::LeastFirst => 1,
            Self::MostFirst => 8,
        }
    }
}

fn dsf(rate: u32, channels: u16, hertz: f64, bits: BitsPerSample) -> Vec<u8> {
    let lanes = usize::from(channels);
    let blocks = 16;
    let per_channel = blocks * DSF_BLOCK;
    let total_bits = (per_channel * 8) as u64;

    let mut planes = vec![Vec::with_capacity(per_channel); lanes];
    for (lane, plane) in planes.iter_mut().enumerate() {
        let mut error = 0.0_f64;
        let tone = hertz * (lane as f64 + 1.0);
        let mut byte = 0_u8;

        for n in 0..per_channel * 8 {
            let wanted = 0.5 * (std::f64::consts::TAU * tone * n as f64 / f64::from(rate)).sin();
            error += wanted;
            let high = error > 0.0;
            error -= if high { 1.0 } else { -1.0 };

            let within = n % 8;
            let set = u8::from(high);
            byte |= match bits {
                BitsPerSample::LeastFirst => set << within,
                BitsPerSample::MostFirst => set << (7 - within),
            };
            if within == 7 {
                plane.push(byte);
                byte = 0;
            }
        }
    }

    let mut data = Vec::with_capacity(per_channel * lanes);
    for block in 0..blocks {
        for plane in &planes {
            data.extend_from_slice(&plane[block * DSF_BLOCK..(block + 1) * DSF_BLOCK]);
        }
    }

    let mut fmt = Vec::new();
    fmt.extend_from_slice(b"fmt ");
    fmt.extend_from_slice(&52_u64.to_le_bytes());
    fmt.extend_from_slice(&1_u32.to_le_bytes());
    fmt.extend_from_slice(&0_u32.to_le_bytes());
    fmt.extend_from_slice(&u32::from(channels).to_le_bytes());
    fmt.extend_from_slice(&u32::from(channels).to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&bits.declared().to_le_bytes());
    fmt.extend_from_slice(&total_bits.to_le_bytes());
    fmt.extend_from_slice(&(DSF_BLOCK as u32).to_le_bytes());
    fmt.extend_from_slice(&0_u32.to_le_bytes());

    let mut chunk = Vec::new();
    chunk.extend_from_slice(b"data");
    chunk.extend_from_slice(&((data.len() + 12) as u64).to_le_bytes());
    chunk.extend_from_slice(&data);

    let total = (28 + fmt.len() + chunk.len()) as u64;
    let mut file = Vec::with_capacity(total as usize);
    file.extend_from_slice(b"DSD ");
    file.extend_from_slice(&28_u64.to_le_bytes());
    file.extend_from_slice(&total.to_le_bytes());
    file.extend_from_slice(&0_u64.to_le_bytes());
    file.extend_from_slice(&fmt);
    file.extend_from_slice(&chunk);
    file
}

const REAL_FIXTURES: &str = "RESONATE_REAL_FIXTURES";

const READABLE_EXTENSIONS: [&str; 14] = [
    "aac", "aif", "aiff", "caf", "flac", "m4a", "m4b", "mka", "mp3", "mp4", "oga", "ogg", "opus",
    "wav",
];

const ONE_PIXEL_PNG: [u8; 69] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

const FLAC_MAGIC: [u8; 4] = *b"fLaC";
const FLAC_LAST_BLOCK: u8 = 0x80;
const FLAC_BLOCK_HEADER: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlacBlock {
    StreamInfo,
    Padding,
    SeekTable,
    VorbisComment,
    CueSheet,
    Picture,
    Other,
}

impl FlacBlock {
    fn of(kind: u8) -> Self {
        match kind {
            0 => Self::StreamInfo,
            1 => Self::Padding,
            3 => Self::SeekTable,
            4 => Self::VorbisComment,
            5 => Self::CueSheet,
            6 => Self::Picture,
            _ => Self::Other,
        }
    }
}

fn blocks(path: &Path) -> Vec<FlacBlock> {
    let held = fs::read(path).expect("a flac file reads back");
    assert!(held.starts_with(&FLAC_MAGIC), "not a flac file");

    let mut found = Vec::new();
    let mut at = FLAC_MAGIC.len();
    while let Some(header) = held.get(at..at + FLAC_BLOCK_HEADER) {
        let kind = header[0] & !FLAC_LAST_BLOCK;
        let size = u32::from_be_bytes([0, header[1], header[2], header[3]]) as usize;
        found.push(FlacBlock::of(kind));

        at += FLAC_BLOCK_HEADER + size;
        if header[0] & FLAC_LAST_BLOCK != 0 {
            break;
        }
    }
    found
}

#[derive(Clone, Copy)]
struct Shape {
    rate: u32,
    channels: u16,
    bits: u16,
}

const CD: Shape = Shape {
    rate: RATE,
    channels: CHANNELS,
    bits: 16,
};

const STUDIO: Shape = Shape {
    rate: 96_000,
    channels: 2,
    bits: 24,
};

const SURROUND: Shape = Shape {
    rate: 48_000,
    channels: 6,
    bits: 24,
};

const BROADCAST: Shape = Shape {
    rate: 48_000,
    channels: 2,
    bits: 16,
};

const SPOKEN: Shape = Shape {
    rate: 22_050,
    channels: 1,
    bits: 16,
};

impl Shape {
    const fn frames(self) -> usize {
        self.rate as usize * SECONDS
    }

    const fn bytes_per_sample(self) -> usize {
        self.bits as usize / 8
    }

    const fn block_align(self) -> u16 {
        self.channels * self.bits / 8
    }

    fn full_scale(self) -> f64 {
        f64::from(1_u32 << (self.bits - 1))
    }
}

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-encoded-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn at(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn ffmpeg() -> bool {
    tool("ffmpeg", "-version")
}

fn tool(name: &str, ask: &str) -> bool {
    Command::new(name)
        .arg(ask)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn ran(name: &str, arguments: &[&str]) -> bool {
    Command::new(name)
        .args(arguments)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn tone(shape: Shape) -> Vec<i32> {
    let amplitude = shape.full_scale() * 0.9;
    let mut samples = Vec::with_capacity(shape.frames() * usize::from(shape.channels));
    for frame in 0..shape.frames() {
        let time = frame as f64 / f64::from(shape.rate);
        for channel in 0..shape.channels {
            let hz = 440.0 * 1.26_f64.powi(i32::from(channel));
            let wave = (2.0 * std::f64::consts::PI * hz * time).sin();
            samples.push((wave * amplitude) as i32);
        }
    }
    samples
}

fn wav(path: &Path, shape: Shape, samples: &[i32]) {
    let stride = shape.bytes_per_sample();
    let mut data = Vec::with_capacity(samples.len() * stride);
    for sample in samples {
        data.extend_from_slice(
            sample
                .to_le_bytes()
                .get(..stride)
                .expect("a sample is at most four bytes wide"),
        );
    }

    let extensible = shape.channels > 2;
    let align = shape.block_align();
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&if extensible { PCM_EXTENSIBLE } else { PCM }.to_le_bytes());
    fmt.extend_from_slice(&shape.channels.to_le_bytes());
    fmt.extend_from_slice(&shape.rate.to_le_bytes());
    fmt.extend_from_slice(&(shape.rate * u32::from(align)).to_le_bytes());
    fmt.extend_from_slice(&align.to_le_bytes());
    fmt.extend_from_slice(&shape.bits.to_le_bytes());
    if extensible {
        fmt.extend_from_slice(&22_u16.to_le_bytes());
        fmt.extend_from_slice(&shape.bits.to_le_bytes());
        fmt.extend_from_slice(&SURROUND_MASK.to_le_bytes());
        fmt.extend_from_slice(&PCM_SUBFORMAT);
    }

    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    fs::write(path, file).expect("a writable temporary file");
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn encode(source: &Path, target: &Path, codec: &[&str]) -> bool {
    let mut command = Command::new("ffmpeg");
    command
        .args(["-y", "-v", "error", "-i"])
        .arg(source)
        .args(codec)
        .args(["-metadata", "title=Echoes"])
        .args(["-metadata", "artist=Pink Floyd"])
        .args(["-metadata", "album=Meddle"])
        .args(["-metadata", "album_artist=Pink Floyd"])
        .args(["-metadata", "date=1971"])
        .args(["-metadata", "track=2"])
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    command.status().is_ok_and(|status| status.success()) && target.exists()
}

fn adts_packets(stream: &[u8]) -> Option<Vec<&[u8]>> {
    let mut packets = Vec::new();
    let mut at = 0;

    while at < stream.len() {
        let header = stream.get(at..at + ADTS_HEADER_BYTES)?;
        if header[0] != ADTS_SYNC || header[1] & ADTS_SYNC_TAIL != ADTS_SYNC_TAIL {
            return None;
        }

        let head = if header[1] & ADTS_PROTECTION_ABSENT == 0 {
            ADTS_HEADER_BYTES + ADTS_CRC_BYTES
        } else {
            ADTS_HEADER_BYTES
        };
        let length = adts_frame_length(header);

        packets.push(stream.get(at + head..at + length)?);
        at += length;
    }

    (!packets.is_empty()).then_some(packets)
}

fn adts_frame_length(header: &[u8]) -> usize {
    (usize::from(header[3] & 0x3) << 11)
        | (usize::from(header[4]) << 3)
        | (usize::from(header[5]) >> 5)
}

fn caf_length(value: u64) -> Vec<u8> {
    let mut bytes = vec![(value & 0x7F) as u8];
    let mut left = value >> 7;
    while left > 0 {
        bytes.push(((left & 0x7F) as u8) | 0x80);
        left >>= 7;
    }
    bytes.reverse();
    bytes
}

fn caf(shape: Shape, packets: &[&[u8]], music: u64, priming: u32, remainder: u32) -> Vec<u8> {
    let mut description = Vec::new();
    description.extend_from_slice(&f64::from(shape.rate).to_be_bytes());
    description.extend_from_slice(b"aac ");
    description.extend_from_slice(&CAF_MPEG4_AAC_LC.to_be_bytes());
    description.extend_from_slice(&CAF_VARIABLE_BYTES_PER_PACKET.to_be_bytes());
    description.extend_from_slice(&AAC_FRAMES_PER_PACKET.to_be_bytes());
    description.extend_from_slice(&u32::from(shape.channels).to_be_bytes());
    description.extend_from_slice(&CAF_UNUSED_BITS_PER_CHANNEL.to_be_bytes());

    let mut table = Vec::new();
    table.extend_from_slice(&(packets.len() as i64).to_be_bytes());
    table.extend_from_slice(&(music as i64).to_be_bytes());
    table.extend_from_slice(&(priming as i32).to_be_bytes());
    table.extend_from_slice(&(remainder as i32).to_be_bytes());
    for packet in packets {
        table.extend_from_slice(&caf_length(packet.len() as u64));
    }

    let mut audio = Vec::new();
    audio.extend_from_slice(&CAF_NO_EDITS.to_be_bytes());
    for packet in packets {
        audio.extend_from_slice(packet);
    }

    let mut held = Vec::new();
    held.extend_from_slice(b"caff");
    held.extend_from_slice(&CAF_VERSION.to_be_bytes());
    held.extend_from_slice(&0_u16.to_be_bytes());
    for (kind, body) in [
        (b"desc", &description),
        (b"pakt", &table),
        (b"data", &audio),
    ] {
        held.extend_from_slice(kind);
        held.extend_from_slice(&(body.len() as i64).to_be_bytes());
        held.extend_from_slice(body);
    }
    held
}

struct Piped(Cursor<Vec<u8>>);

impl Read for Piped {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Piped {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "a pipe"))
    }
}

struct Decoded {
    samples: Vec<i32>,
    spec: StreamSpec,
}

fn decode(path: &Path) -> Decoded {
    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(path))
        .expect("a well-formed file opens");

    Decoded {
        samples: drain(&mut decoder, info.spec),
        spec: info.spec,
    }
}

fn drain(decoder: &mut Decoder, spec: StreamSpec) -> Vec<i32> {
    decoder.set_output_format(SampleFormat::S32);

    let mut block = AudioBuffer::empty(spec);
    let mut samples = Vec::new();
    while decoder.next_block(&mut block).expect("a clean decode") == DecodeStatus::Decoded {
        match block.data() {
            resonate_core::SampleData::S32(store) => samples.extend_from_slice(store),
            other => panic!("the decoder ignored the requested output format: {other:?}"),
        }
    }
    samples
}

fn drift(decoded: &[i32], wanted: &[i32]) -> f64 {
    let apart = decoded
        .iter()
        .zip(wanted)
        .map(|(held, other)| held.saturating_sub(*other))
        .collect::<Vec<_>>();

    rms(&apart)
}

fn widened(samples: &[i32], bits: u16) -> Vec<i32> {
    samples.iter().map(|sample| sample << (32 - bits)).collect()
}

fn rms(samples: &[i32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples
        .iter()
        .map(|sample| {
            let scaled = f64::from(*sample) / f64::from(i32::MAX);
            scaled * scaled
        })
        .sum();
    (sum / samples.len() as f64).sqrt()
}

fn two_audio_tracks(tree: &Tree, name: &str) -> Option<PathBuf> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name} fixture");
        return None;
    }

    let source = tree.at("paired.wav");
    wav(&source, CD, &tone(CD));

    let target = tree.at(name);
    let encoded = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .arg("-i")
        .arg(&source)
        .args(["-map", "0:a", "-map", "1:a", "-c:a", "libvorbis"])
        .args(["-metadata", "title=Echoes"])
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) || !target.exists() {
        eprintln!("skipped: ffmpeg would not encode {name}");
        return None;
    }
    Some(target)
}

fn fixture(tree: &Tree, name: &str, codec: &[&str]) -> Option<(PathBuf, Vec<i32>)> {
    shaped(tree, CD, name, codec)
}

fn tagged(tree: &Tree, name: &str, codec: &[&str], tags: &[&str]) -> Option<PathBuf> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name} fixture");
        return None;
    }

    let source = tree.at("tagged.wav");
    wav(&source, CD, &tone(CD));

    let target = tree.at(name);
    let mut command = Command::new("ffmpeg");
    command.args(["-y", "-v", "error", "-i"]).arg(&source);
    command.args(codec);
    for tag in tags {
        command.args(["-metadata", tag]);
    }
    let encoded = command
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if !encoded.is_ok_and(|status| status.success()) || !target.exists() {
        eprintln!("skipped: ffmpeg would not encode {name}");
        return None;
    }
    Some(target)
}

fn shaped(tree: &Tree, shape: Shape, name: &str, codec: &[&str]) -> Option<(PathBuf, Vec<i32>)> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a {name} fixture");
        return None;
    }

    let samples = tone(shape);
    let source = tree.at("source.wav");
    wav(&source, shape, &samples);

    let target = tree.at(name);
    if !encode(&source, &target, codec) {
        eprintln!("skipped: ffmpeg would not encode {name}");
        return None;
    }
    Some((target, samples))
}

#[test]
fn flac_decodes_to_exactly_the_samples_that_went_in() {
    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "tone.flac", &["-c:a", "flac"]) else {
        return;
    };

    let decoded = decode(&path);

    assert_eq!(decoded.spec.rate, SampleRate::HZ_44100);
    assert_eq!(decoded.spec.channels, ChannelLayout::Stereo);
    assert_eq!(decoded.spec.format, SampleFormat::S16);
    assert_eq!(
        decoded.samples,
        widened(&samples, 16),
        "flac is lossless and did not round-trip"
    );
}

#[test]
fn alac_decodes_to_exactly_the_samples_that_went_in() {
    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "tone.m4a", &["-c:a", "alac"]) else {
        return;
    };

    let decoded = decode(&path);

    assert_eq!(decoded.spec.rate, SampleRate::HZ_44100);
    assert_eq!(decoded.spec.channels, ChannelLayout::Stereo);
    assert_eq!(
        decoded.spec.format,
        SampleFormat::S16,
        "alac reported a depth its magic cookie does not declare"
    );
    assert_eq!(
        decoded.samples,
        widened(&samples, 16),
        "alac is lossless and did not round-trip"
    );
}

#[test]
fn a_lossless_encoder_round_trips_the_shapes_past_the_cd_one() {
    let cases: [(&str, Shape, ChannelLayout, &[&str]); 5] = [
        (
            "studio.flac",
            STUDIO,
            ChannelLayout::Stereo,
            &["-c:a", "flac"],
        ),
        (
            "studio.m4a",
            STUDIO,
            ChannelLayout::Stereo,
            &["-c:a", "alac"],
        ),
        (
            "surround.flac",
            SURROUND,
            ChannelLayout::Surround51,
            &["-c:a", "flac"],
        ),
        (
            "studio.aiff",
            STUDIO,
            ChannelLayout::Stereo,
            &["-c:a", "pcm_s24be"],
        ),
        (
            "studio.caf",
            STUDIO,
            ChannelLayout::Stereo,
            &["-c:a", "pcm_s24le"],
        ),
    ];

    for (name, shape, layout, codec) in cases {
        let tree = Tree::new();
        let Some((path, samples)) = shaped(&tree, shape, name, codec) else {
            return;
        };

        let decoded = decode(&path);

        assert_eq!(
            decoded.spec.rate.hz(),
            shape.rate,
            "{name} changed its rate"
        );
        assert_eq!(decoded.spec.channels, layout, "{name} changed its layout");
        assert_eq!(
            decoded.spec.format,
            SampleFormat::S24,
            "{name} did not stay twenty-four bits deep"
        );
        assert_eq!(
            decoded.samples,
            widened(&samples, shape.bits),
            "{name} is lossless and did not round-trip"
        );
    }
}

#[test]
fn a_lossy_stream_decodes_to_the_same_signal_it_encoded() {
    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "tone.mp3", &["-c:a", "libmp3lame", "-b:a", "192k"])
    else {
        return;
    };

    let decoded = decode(&path);
    let skip = RATE as usize * usize::from(CHANNELS) / 10;
    let kept = decoded
        .samples
        .get(skip..skip + samples.len() / 2)
        .expect("a decoded span past the encoder delay");

    assert_eq!(decoded.spec.rate, SampleRate::HZ_44100);
    assert_eq!(decoded.spec.channels, ChannelLayout::Stereo);

    let expected = rms(&widened(&samples, 16));
    let actual = rms(kept);
    assert!(
        (actual - expected).abs() < expected * 0.1,
        "decoded loudness {actual:.4} is nothing like the source's {expected:.4}"
    );
}

#[test]
fn a_lossy_stream_holds_its_signal_past_the_cd_shape() {
    let cases: [(&str, Shape, ChannelLayout, &[&str]); 4] = [
        (
            "broadcast.m4a",
            BROADCAST,
            ChannelLayout::Stereo,
            &["-c:a", "aac", "-b:a", "256k"],
        ),
        (
            "broadcast.ogg",
            BROADCAST,
            ChannelLayout::Stereo,
            &["-c:a", "libvorbis", "-b:a", "256k"],
        ),
        (
            "broadcast.mka",
            BROADCAST,
            ChannelLayout::Stereo,
            &["-c:a", "libvorbis", "-b:a", "256k"],
        ),
        (
            "spoken.mp3",
            SPOKEN,
            ChannelLayout::Mono,
            &["-c:a", "libmp3lame", "-b:a", "128k"],
        ),
    ];

    for (name, shape, layout, codec) in cases {
        let tree = Tree::new();
        let Some((path, samples)) = shaped(&tree, shape, name, codec) else {
            return;
        };

        let decoded = decode(&path);
        let skip = shape.rate as usize * usize::from(shape.channels) / 10;
        let kept = decoded
            .samples
            .get(skip..skip + samples.len() / 2)
            .expect("a decoded span past the encoder delay");

        assert_eq!(
            decoded.spec.rate.hz(),
            shape.rate,
            "{name} changed its rate"
        );
        assert_eq!(decoded.spec.channels, layout, "{name} changed its layout");

        let expected = rms(&widened(&samples, shape.bits));
        let actual = rms(kept);
        assert!(
            (actual - expected).abs() < expected * 0.1,
            "{name} decoded to a loudness of {actual:.4}, nothing like the source's {expected:.4}"
        );
    }
}

#[test]
fn a_bare_adts_stream_decodes_as_the_media_type_the_desktop_advertises_promises() {
    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "rip.aac", &["-c:a", "aac", "-b:a", "256k"]) else {
        return;
    };

    let probed =
        probe(&Sources::local(), &MediaLocation::local(&path)).expect("an adts stream probes");
    assert_eq!(Container::from_id(probed.container), Container::Adts);
    assert_eq!(Codec::from_id(probed.codec), Codec::Aac);

    let decoded = decode(&path);
    let skip = CD.rate as usize * usize::from(CD.channels) / 10;
    let kept = decoded
        .samples
        .get(skip..skip + samples.len() / 2)
        .expect("a decoded span past the encoder delay");

    assert_eq!(decoded.spec.rate.hz(), CD.rate);
    assert_eq!(decoded.spec.channels, ChannelLayout::Stereo);

    let expected = rms(&widened(&samples, CD.bits));
    let actual = rms(kept);
    assert!(
        (actual - expected).abs() < expected * 0.1,
        "an adts stream decoded to a loudness of {actual:.4}, nothing like the source's {expected:.4}"
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tagging {
    Everything,
    TitleAlone,
    Nothing,
}

struct Encoded {
    name: &'static str,
    arguments: &'static [&'static str],
    container: Container,
    codec: Codec,
    tagging: Tagging,
}

#[test]
fn every_encoder_the_workspace_claims_is_recognised_by_the_probe() {
    let tree = Tree::new();
    let cases = [
        Encoded {
            name: "tone.flac",
            arguments: &["-c:a", "flac"],
            container: Container::Flac,
            codec: Codec::Flac,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.m4a",
            arguments: &["-c:a", "alac"],
            container: Container::IsoMp4,
            codec: Codec::Alac,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.aac.m4a",
            arguments: &["-c:a", "aac", "-b:a", "192k"],
            container: Container::IsoMp4,
            codec: Codec::Aac,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.mp3",
            arguments: &["-c:a", "libmp3lame", "-b:a", "192k"],
            container: Container::Mpeg,
            codec: Codec::Mp3,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.ogg",
            arguments: &["-c:a", "libvorbis", "-b:a", "192k"],
            container: Container::Ogg,
            codec: Codec::Vorbis,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.mka",
            arguments: &["-c:a", "libvorbis", "-b:a", "192k"],
            container: Container::Matroska,
            codec: Codec::Vorbis,
            tagging: Tagging::Everything,
        },
        Encoded {
            name: "tone.aiff",
            arguments: &["-c:a", "pcm_s16be"],
            container: Container::Aiff,
            codec: Codec::Pcm,
            tagging: Tagging::TitleAlone,
        },
        Encoded {
            name: "tone.caf",
            arguments: &["-c:a", "pcm_s16le"],
            container: Container::Caf,
            codec: Codec::Pcm,
            tagging: Tagging::Nothing,
        },
    ];

    for case in cases {
        let Some((path, _)) = fixture(&tree, case.name, case.arguments) else {
            return;
        };
        let name = case.name;

        let report = probe_stream(&Sources::local(), &MediaLocation::local(&path))
            .expect("a real file probes");
        assert_eq!(
            Container::from_id(report.info.container),
            case.container,
            "{name} was not recognised"
        );
        assert_eq!(
            Codec::from_id(report.info.codec),
            case.codec,
            "{name} decoded under the wrong codec"
        );
        assert_eq!(report.info.spec.rate, SampleRate::HZ_44100);
        assert_eq!(report.info.spec.channels, ChannelLayout::Stereo);
        if case.tagging == Tagging::Nothing {
            continue;
        }
        assert!(!report.tags.is_empty(), "{name} carried no raw tags at all");

        let tags = &report.info.tags;
        assert_eq!(
            tags.title.as_deref(),
            Some("Echoes"),
            "{name} lost its title"
        );
        if case.tagging == Tagging::TitleAlone {
            continue;
        }
        assert_eq!(
            tags.artist.as_deref(),
            Some("Pink Floyd"),
            "{name} lost its artist"
        );
        assert_eq!(
            tags.album.as_deref(),
            Some("Meddle"),
            "{name} lost its album"
        );
        assert_eq!(
            tags.album_artist.as_deref(),
            Some("Pink Floyd"),
            "{name} lost its album artist"
        );
        assert_eq!(tags.track_number, Some(2), "{name} lost its track number");
        assert_eq!(
            tags.date.as_deref(),
            Some("1971"),
            "{name} lost its release date"
        );

        let profile = report.profile.expect("a bitrate profile");
        assert!(
            profile.overall.kilobits() > 0.0,
            "{name} reported no bitrate"
        );
    }
}

#[test]
fn an_iso_container_reports_the_order_its_boxes_were_written_in() {
    let tree = Tree::new();
    let Some((trailing, _)) = fixture(&tree, "trailing.m4a", &["-c:a", "alac"]) else {
        return;
    };
    let Some((faststart, _)) = fixture(
        &tree,
        "faststart.m4a",
        &["-c:a", "alac", "-movflags", "+faststart"],
    ) else {
        return;
    };

    let trailing = probe_stream(&Sources::local(), &MediaLocation::local(&trailing))
        .expect("a real file probes");
    let faststart = probe_stream(&Sources::local(), &MediaLocation::local(&faststart))
        .expect("a real file probes");

    let trailing = trailing.layout.expect("an iso container has a box layout");
    let faststart = faststart.layout.expect("an iso container has a box layout");

    assert_eq!(trailing.faststart, Some(Faststart::Trailing));
    assert_eq!(faststart.faststart, Some(Faststart::Ready));
    assert!(
        trailing.boxes.len() > 1,
        "the box walk found nothing to order"
    );
}

#[test]
fn a_matroska_segment_declares_the_length_its_track_does_not() {
    let tree = Tree::new();
    let Some((path, _)) = fixture(&tree, "tone.mka", &["-c:a", "libvorbis"]) else {
        return;
    };

    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
        .expect("a well-formed file opens");
    let duration = info.duration.expect("the segment declares a duration");

    assert!(
        duration.get().abs_diff(u64::from(RATE) * SECONDS as u64) < u64::from(RATE) / 10,
        "the segment reported {duration}, nothing like the two seconds that went in"
    );

    let target = Frames(u64::from(RATE) / 2);
    let landed = decoder.seek(target).expect("a track with a length seeks");
    assert!(
        landed.get().abs_diff(target.get()) < u64::from(RATE) / 1_000,
        "a seek to {target} landed on {landed}, past the tick the container counts in"
    );
}

#[test]
fn a_flac_in_matroska_is_as_long_as_its_frames_whatever_the_segment_declares() {
    const SEGMENT_DURATION_AS_A_DOUBLE: [u8; 3] = [0x44, 0x89, 0x88];

    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "lossless.mka", &["-c:a", "flac"]) else {
        return;
    };
    let frames = Frames((samples.len() / usize::from(CD.channels)) as u64);

    let mut bytes = fs::read(&path).expect("the fixture reads back");
    let at = bytes
        .windows(SEGMENT_DURATION_AS_A_DOUBLE.len())
        .position(|window| window == SEGMENT_DURATION_AS_A_DOUBLE)
        .expect("ffmpeg declares the segment's duration as a double")
        + SEGMENT_DURATION_AS_A_DOUBLE.len();
    let declared = f64::from_be_bytes(bytes[at..at + 8].try_into().expect("eight bytes"));
    bytes[at..at + 8].copy_from_slice(&(declared * 3.0).to_be_bytes());
    let overlong = tree.at("overlong.mka");
    fs::write(&overlong, &bytes).expect("the patched fixture writes");

    for path in [path, overlong] {
        let info = probe(&Sources::local(), &MediaLocation::local(&path))
            .expect("a well-formed file probes");

        assert_eq!(
            info.duration,
            Some(frames),
            "{} was not counted to the frames that went in",
            path.display()
        );
    }
}

#[test]
fn a_segment_title_names_a_track_only_where_it_is_the_only_one() {
    let tree = Tree::new();
    let Some((sole, _)) = fixture(&tree, "sole.mka", &["-c:a", "libvorbis"]) else {
        return;
    };
    let Some(paired) = two_audio_tracks(&tree, "paired.mka") else {
        return;
    };

    let sources = Sources::local();
    let one = probe(&sources, &MediaLocation::local(&sole)).expect("a well-formed file probes");
    let two = probe(&sources, &MediaLocation::local(&paired)).expect("a well-formed file probes");

    assert_eq!(
        one.tags.title.as_deref(),
        Some("Echoes"),
        "the only audio track lost the name the segment gave it"
    );
    assert_eq!(
        two.tags.title, None,
        "a segment naming the file named one of its two audio tracks as well"
    );
}

#[test]
fn the_credits_a_matroska_carries_reach_the_set_rather_than_stopping_at_the_raw_listing() {
    let tree = Tree::new();
    let Some(path) = tagged(
        &tree,
        "credited.mka",
        &["-c:a", "libvorbis"],
        &[
            "title=Echoes",
            "COMPOSER=Richard Wright",
            "CONDUCTOR=Ron Goodwin",
            "LYRICIST=Roger Waters",
            "PRODUCER=Pink Floyd",
            "SOUND_ENGINEER=Alan Parsons",
            "PUBLISHER=Harvest",
            "COPYRIGHT=(c) 1971 Harvest",
            "COMMENT=the 1994 remaster",
            "ISRC=GBAYE7100195",
            "BPM=68",
        ],
    ) else {
        return;
    };

    let probed =
        probe(&Sources::local(), &MediaLocation::local(&path)).expect("a well-formed file probes");
    let tags = &probed.tags;

    assert_eq!(tags.credits.composer.as_deref(), Some("Richard Wright"));
    assert_eq!(tags.credits.conductor.as_deref(), Some("Ron Goodwin"));
    assert_eq!(tags.credits.lyricist.as_deref(), Some("Roger Waters"));
    assert_eq!(tags.credits.producer.as_deref(), Some("Pink Floyd"));
    assert_eq!(tags.credits.engineer.as_deref(), Some("Alan Parsons"));
    assert_eq!(tags.label.as_deref(), Some("Harvest"));
    assert_eq!(tags.copyright.as_deref(), Some("(c) 1971 Harvest"));
    assert_eq!(tags.comment.as_deref(), Some("the 1994 remaster"));
    assert_eq!(tags.isrc.as_deref(), Some("GBAYE7100195"));
    assert_eq!(tags.beats_per_minute, Some(68));
}

#[test]
fn seeking_a_real_container_lands_where_it_was_asked_to() {
    let tree = Tree::new();
    let Some((path, _)) = fixture(&tree, "tone.flac", &["-c:a", "flac"]) else {
        return;
    };

    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
        .expect("a well-formed file opens");
    assert!(info.is_seekable);

    for target in [
        Frames(0),
        Frames(u64::from(RATE)),
        Frames(u64::from(RATE) / 3),
    ] {
        let landed = decoder.seek(target).expect("a seekable stream seeks");
        assert_eq!(landed, target, "seek to {target} landed on {landed}");

        let mut block = AudioBuffer::empty(info.spec);
        assert_eq!(
            decoder.next_block(&mut block).expect("a clean decode"),
            DecodeStatus::Decoded
        );
        assert_eq!(
            decoder.position(),
            Frames(target.get() + block.frames() as u64)
        );
    }
}

#[test]
fn a_reference_flac_rip_keeps_its_samples_its_tags_and_its_cover() {
    if !ffmpeg() || !tool("flac", "--version") || !tool("metaflac", "--version") {
        eprintln!("skipped: no flac and metaflac to build a rip the way a ripper does");
        return;
    }

    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("rip.wav");
    wav(&source, CD, &samples);

    let target = tree.at("rip.flac");
    let picture = tree.at("cover.png");
    fs::write(&picture, ONE_PIXEL_PNG).expect("a cover to embed");

    let encoded = ran(
        "flac",
        &[
            "-s",
            "-f",
            "--best",
            "-o",
            &target.to_string_lossy(),
            &source.to_string_lossy(),
        ],
    );
    let described = ran(
        "metaflac",
        &[
            &format!("--import-picture-from={}", picture.to_string_lossy()),
            "--set-tag=TITLE=Echoes",
            "--set-tag=ARTIST=Pink Floyd",
            &target.to_string_lossy(),
        ],
    );
    if !encoded || !described {
        eprintln!("skipped: flac would not build the rip");
        return;
    }

    assert_eq!(
        blocks(&target),
        vec![
            FlacBlock::StreamInfo,
            FlacBlock::SeekTable,
            FlacBlock::VorbisComment,
            FlacBlock::Picture,
            FlacBlock::Padding,
        ],
        "the reference encoder stopped writing the blocks a rip carries"
    );

    let location = MediaLocation::local(&target);
    let info = probe(&Sources::local(), &location).expect("a rip probes");

    assert_eq!(info.bits_per_coded_sample, Some(16));
    assert_eq!(info.duration, Some(Frames(samples.len() as u64 / 2)));
    assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
    assert_eq!(info.tags.artist.as_deref(), Some("Pink Floyd"));

    let cover = probe_cover_art(&Sources::local(), &location)
        .expect("a rip with a picture probes")
        .expect("the picture the ripper embedded");
    assert_eq!(cover.bytes, ONE_PIXEL_PNG);

    let decoded = decode(&target);
    assert_eq!(
        decoded.samples,
        widened(&samples, CD.bits),
        "a reference FLAC is lossless and did not round-trip"
    );
}

#[test]
fn a_lame_encoded_rip_declares_the_priming_its_xing_header_carries() {
    if !ffmpeg() || !tool("lame", "--version") {
        eprintln!("skipped: no lame to build an mp3 the way a ripper does");
        return;
    }

    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("rip.wav");
    wav(&source, CD, &samples);

    let target = tree.at("rip.mp3");
    let encoded = ran(
        "lame",
        &[
            "--quiet",
            "--preset",
            "standard",
            "--add-id3v2",
            "--tt",
            "Echoes",
            "--ta",
            "Pink Floyd",
            &source.to_string_lossy(),
            &target.to_string_lossy(),
        ],
    );
    if !encoded {
        eprintln!("skipped: lame would not build the rip");
        return;
    }

    let held = fs::read(&target).expect("the rip reads back");
    assert!(held.starts_with(b"ID3"), "lame wrote no id3v2 tag to skip");
    assert!(
        held.windows(4).any(|window| window == b"Xing"),
        "lame wrote no Xing header"
    );

    let info = probe(&Sources::local(), &MediaLocation::local(&target)).expect("a rip probes");

    let lanes = usize::from(CD.channels);
    let music = Frames((samples.len() / lanes) as u64);

    assert_eq!(info.tags.title.as_deref(), Some("Echoes"));
    assert_eq!(info.tags.artist.as_deref(), Some("Pink Floyd"));
    assert!(
        info.encoder_delay > 0,
        "the LAME tag's priming never reached the decoder"
    );
    assert_eq!(
        info.duration,
        Some(music),
        "the Xing frame count did not land on the sample the file holds"
    );

    let decoded = decode(&target);
    assert_eq!(
        (decoded.samples.len() / lanes) as u64,
        music.get(),
        "an mp3 rip decoded its priming and its padding as audio"
    );

    let wanted = widened(&samples, CD.bits);
    let primed =
        usize::try_from(info.encoder_delay).expect("a priming of a thousand frames") * lanes;
    assert!(
        drift(&decoded.samples, &wanted) < drift(&decoded.samples, &wanted[primed..]),
        "the priming came off twice, once inside the decoder and once here"
    );
}

#[test]
fn a_vorbis_rip_over_a_pipe_drops_the_priming_its_pages_declare() {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build an ogg the way an encoder does");
        return;
    }

    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("rip.wav");
    wav(&source, CD, &samples);

    let target = tree.at("rip.ogg");
    let encoded = ran(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            &source.to_string_lossy(),
            "-c:a",
            "libvorbis",
            "-b:a",
            "192k",
            &target.to_string_lossy(),
        ],
    );
    if !encoded {
        eprintln!("skipped: ffmpeg would not build the rip");
        return;
    }

    let lanes = usize::from(CD.channels);
    let music = Frames((samples.len() / lanes) as u64);
    let whole = decode(&target);
    assert_eq!(
        (whole.samples.len() / lanes) as u64,
        music.get(),
        "an ogg read as a file decoded its priming and its padding as audio"
    );

    let held = fs::read(&target).expect("the rip reads back");
    let (mut decoder, info) =
        Decoder::open_reader(Piped(Cursor::new(held)), &MediaLocation::local(&target))
            .expect("a well-formed ogg opens over a pipe");

    assert!(!info.is_seekable, "the pipe was read as a file");
    assert!(
        info.encoder_delay > 0,
        "the first page's discard never reached the decoder"
    );
    assert_eq!(
        info.playable.map(FrameSpan::start),
        Some(Frames(u64::from(info.encoder_delay))),
        "nothing told the decoder which frames are the music"
    );
    assert_eq!(
        info.playable.and_then(FrameSpan::frames),
        None,
        "a pipe reaches no end bound, so nothing can say where the music stops"
    );

    let decoded = drain(&mut decoder, info.spec);
    let wanted = widened(&samples, CD.bits);
    let primed =
        usize::try_from(info.encoder_delay).expect("a priming of a hundred frames") * lanes;
    assert!(
        drift(&decoded, &wanted) < drift(&decoded[primed..], &wanted),
        "an ogg over a pipe kept the priming no end bound could bound"
    );
}

#[test]
fn a_caf_rip_drops_the_priming_and_the_remainder_its_packet_table_declares() {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build the aac a caf carries");
        return;
    }

    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("rip.wav");
    wav(&source, CD, &samples);

    let stream = tree.at("rip.aac");
    let encoded = ran(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            &source.to_string_lossy(),
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            &stream.to_string_lossy(),
        ],
    );
    if !encoded {
        eprintln!("skipped: ffmpeg would not build the rip");
        return;
    }

    let held = fs::read(&stream).expect("the rip reads back");
    let Some(packets) = adts_packets(&held) else {
        eprintln!("skipped: ffmpeg wrote an adts stream this test cannot repack");
        return;
    };

    let lanes = usize::from(CD.channels);
    let music = (samples.len() / lanes) as u64;
    let block = packets.len() as u64 * u64::from(AAC_FRAMES_PER_PACKET);
    let Some(remainder) = block
        .checked_sub(u64::from(AAC_PRIMING))
        .and_then(|held| held.checked_sub(music))
        .and_then(|held| u32::try_from(held).ok())
    else {
        eprintln!("skipped: ffmpeg primed the rip by something other than one block");
        return;
    };

    let target = tree.at("rip.caf");
    fs::write(&target, caf(CD, &packets, music, AAC_PRIMING, remainder))
        .expect("a writable temporary directory");

    let info = probe(&Sources::local(), &MediaLocation::local(&target)).expect("a rip probes");

    assert_eq!(Container::from_id(info.container), Container::Caf);
    assert_eq!(Codec::from_id(info.codec), Codec::Aac);
    assert_eq!(
        info.encoder_delay, AAC_PRIMING,
        "the packet table's priming never reached the decoder"
    );
    assert_eq!(info.encoder_padding, remainder);
    assert_eq!(
        info.duration,
        Some(Frames(music)),
        "the priming was counted as music the file holds"
    );
    assert_eq!(
        info.playable.and_then(FrameSpan::frames),
        Some(Frames(music)),
        "nothing told the decoder which frames are the music"
    );

    let decoded = decode(&target);
    assert_eq!(
        (decoded.samples.len() / lanes) as u64,
        music,
        "a CAF rip decoded its priming and its remainder as audio"
    );

    let wanted = widened(&samples, CD.bits);
    let primed = AAC_PRIMING as usize * lanes;
    assert!(
        drift(&decoded.samples, &wanted) < drift(&decoded.samples[primed..], &wanted),
        "the decoded audio still lines up with the priming in front of it"
    );
}

#[test]
fn an_aac_rip_drops_the_priming_and_the_padding_its_edit_list_declares() {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build an m4a the way a store does");
        return;
    }

    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("rip.wav");
    wav(&source, CD, &samples);

    let target = tree.at("rip.m4a");
    let encoded = ran(
        "ffmpeg",
        &[
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-i",
            &source.to_string_lossy(),
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            &target.to_string_lossy(),
        ],
    );
    if !encoded {
        eprintln!("skipped: ffmpeg would not build the rip");
        return;
    }

    let lanes = usize::from(CHANNELS);
    let music = Frames((samples.len() / lanes) as u64);
    let info = probe(&Sources::local(), &MediaLocation::local(&target)).expect("a rip probes");

    assert!(
        info.encoder_delay > 0,
        "the edit list's priming never reached the decoder"
    );
    assert_eq!(
        info.duration,
        Some(music),
        "the priming was counted as music the file holds"
    );
    assert_eq!(
        info.playable.and_then(FrameSpan::frames),
        Some(music),
        "nothing told the decoder which frames are the music"
    );

    let decoded = decode(&target);
    assert_eq!(
        (decoded.samples.len() / lanes) as u64,
        music.get(),
        "an AAC rip decoded its priming and its padding as audio"
    );

    let wanted = widened(&samples, CD.bits);
    let primed =
        usize::try_from(info.encoder_delay).expect("a priming of a few thousand frames") * lanes;
    assert!(
        drift(&decoded.samples, &wanted) < drift(&decoded.samples[primed..], &wanted),
        "the decoded audio still lines up with the priming in front of it"
    );
}

#[test]
fn every_real_file_the_run_was_pointed_at_opens_and_decodes() {
    let Some(root) = env::var_os(REAL_FIXTURES) else {
        eprintln!("skipped: set {REAL_FIXTURES} to a folder of real files to read them");
        return;
    };

    let mut walked = Vec::new();
    collect(Path::new(&root), &mut walked);
    assert!(
        !walked.is_empty(),
        "{REAL_FIXTURES} names a folder holding no file this build claims to read"
    );

    let sources = Sources::local();
    let mut read = 0;
    for path in &walked {
        let location = MediaLocation::local(path);
        let named = path.display();

        let info = match probe(&sources, &location) {
            Ok(info) => info,
            Err(refused) => panic!("{named} did not probe: {refused}"),
        };
        assert_ne!(
            Container::from_id(info.container),
            Container::Unknown,
            "{named} probed under no container this build names"
        );
        assert_ne!(
            Codec::from_id(info.codec),
            Codec::Unknown,
            "{named} probed under no codec this build names"
        );
        assert!(info.duration.is_some(), "{named} reports no duration");

        let (mut decoder, info) = Decoder::open(&sources, &location)
            .unwrap_or_else(|refused| panic!("{named}: {refused}"));
        let mut out = AudioBuffer::empty(info.spec);
        let status = decoder
            .next_block(&mut out)
            .unwrap_or_else(|refused| panic!("{named} did not decode its first block: {refused}"));
        assert_eq!(status, DecodeStatus::Decoded, "{named} decoded nothing");
        read += 1;
    }

    eprintln!(
        "read {read} real files under {}",
        Path::new(&root).display()
    );
}

fn collect(at: &Path, into: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(at) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, into);
        } else if is_audio(&path) {
            into.push(path);
        }
    }
}

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            READABLE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())
        })
}

#[test]
fn a_span_of_a_real_container_decodes_exactly_the_slice_it_names() {
    let tree = Tree::new();
    let Some((path, samples)) = fixture(&tree, "album.flac", &["-c:a", "flac"]) else {
        return;
    };

    let channels = usize::from(CHANNELS);
    for (start, end) in [(0_u64, 9_000_u64), (9_000, 21_337), (21_337, 44_100)] {
        let span = FrameSpan::between(Frames(start), Frames(end));
        let (mut decoder, info) =
            Decoder::open_span(&Sources::local(), &MediaLocation::local(&path), span)
                .expect("a span of a real container opens");

        assert_eq!(
            info.duration,
            Some(Frames(end - start)),
            "the span reported a length that is not its own"
        );

        decoder.set_output_format(SampleFormat::S32);
        let mut out = AudioBuffer::empty(info.spec);
        let mut decoded = Vec::new();
        while decoder.next_block(&mut out).expect("a span decodes") == DecodeStatus::Decoded {
            match out.data() {
                resonate_core::SampleData::S32(store) => decoded.extend_from_slice(store),
                other => panic!("the decoder ignored the requested output format: {other:?}"),
            }
        }

        let wanted = widened(
            &samples[start as usize * channels..end as usize * channels],
            CD.bits,
        );
        assert_eq!(
            decoded, wanted,
            "the span {start}..{end} is not the slice the file holds"
        );
    }
}

#[test]
fn a_dsd_file_decodes_to_the_tone_its_bits_carry() {
    let tree = Tree::new();
    let path = tree.at("tone.dsf");
    fs::write(&path, dsf(DSD64, 2, 1_000.0, BitsPerSample::LeastFirst)).expect("a dsf fixture");

    let location = MediaLocation::local(&path);
    let info = probe(&Sources::local(), &location).expect("a dsf probes");

    assert_eq!(Container::from_id(info.container), Container::Dsf);
    assert_eq!(Codec::from_id(info.codec), Codec::Dsd);
    assert_eq!(info.spec.rate, SampleRate::HZ_176400);
    assert_eq!(info.spec.format, SampleFormat::S24);
    assert_eq!(info.bits_per_coded_sample, Some(1));

    let (mut decoder, info) = Decoder::open(&Sources::local(), &location).expect("a dsf opens");
    decoder.set_output_format(SampleFormat::F32);

    let mut out = AudioBuffer::empty(info.spec);
    let mut decoded: Vec<f32> = Vec::new();
    while decoder.next_block(&mut out).expect("a dsf decodes") == DecodeStatus::Decoded {
        match out.data() {
            resonate_core::SampleData::F32(store) => decoded.extend_from_slice(store),
            other => panic!("the decoder ignored the requested output format: {other:?}"),
        }
    }

    let settled: Vec<f32> = decoded.iter().skip(4_000).step_by(2).copied().collect();
    let loudest = settled.iter().fold(0.0_f32, |held, s| held.max(s.abs()));
    assert!(
        loudest > 0.2,
        "a decimated DSD tone came back near silence at {loudest}"
    );
    assert!(
        loudest <= 1.0,
        "a decimated DSD tone came back past full scale at {loudest}"
    );

    let rate = f64::from(SampleRate::HZ_176400.hz());
    let tone = energy_at(&settled, 1_000.0, rate);
    for elsewhere in [400.0, 2_500.0, 6_000.0, 15_000.0] {
        let other = energy_at(&settled, elsewhere, rate);
        assert!(
            tone > other * 8.0,
            "a 1 kHz DSD tone is no louder at 1 kHz ({tone}) than at {elsewhere} Hz ({other})"
        );
    }
}

fn energy_at(samples: &[f32], hertz: f64, rate: f64) -> f64 {
    let mut real = 0.0;
    let mut imaginary = 0.0;
    for (n, sample) in samples.iter().enumerate() {
        let angle = std::f64::consts::TAU * hertz * n as f64 / rate;
        real += f64::from(*sample) * angle.cos();
        imaginary += f64::from(*sample) * angle.sin();
    }
    real.hypot(imaginary) / samples.len() as f64
}

#[test]
fn a_dsd_file_asked_for_dop_carries_the_marker_a_dac_locks_onto() {
    let tree = Tree::new();
    let path = tree.at("marked.dsf");
    fs::write(&path, dsf(DSD64, 2, 1_000.0, BitsPerSample::LeastFirst)).expect("a dsf fixture");

    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a dsf opens");
    assert_eq!(info.spec.format, SampleFormat::S24);

    let mut out = AudioBuffer::empty(info.spec);
    assert_eq!(
        decoder.next_block(&mut out).expect("a dsf decodes"),
        DecodeStatus::Decoded
    );

    let resonate_core::SampleData::S24(samples) = out.data() else {
        panic!("a DoP stream is not twenty-four bits deep");
    };
    for (frame, pair) in samples.as_chunks::<2>().0.iter().enumerate().take(64) {
        let wanted = if frame % 2 == 0 { 0x05 } else { 0xFA };
        for sample in pair {
            let marker = (sample.to_le_bytes())[2];
            assert_eq!(
                marker, wanted,
                "frame {frame} carries {marker:#04x} where a DAC reads {wanted:#04x}"
            );
        }
    }
}

#[test]
fn a_dsf_declaring_the_other_bit_order_decodes_to_the_same_tone() {
    let tree = Tree::new();
    let mut held = Vec::new();

    for (name, bits) in [
        ("lsb.dsf", BitsPerSample::LeastFirst),
        ("msb.dsf", BitsPerSample::MostFirst),
    ] {
        let path = tree.at(name);
        fs::write(&path, dsf(DSD64, 2, 1_000.0, bits)).expect("a dsf fixture");

        let (mut decoder, info) =
            Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a dsf opens");
        decoder.set_output_format(SampleFormat::F32);

        let mut out = AudioBuffer::empty(info.spec);
        let mut decoded: Vec<f32> = Vec::new();
        while decoder.next_block(&mut out).expect("a dsf decodes") == DecodeStatus::Decoded {
            match out.data() {
                resonate_core::SampleData::F32(store) => decoded.extend_from_slice(store),
                other => panic!("the decoder ignored the requested output format: {other:?}"),
            }
        }
        held.push(decoded);
    }

    let rate = f64::from(SampleRate::HZ_176400.hz());
    for decoded in &held {
        let settled: Vec<f32> = decoded.iter().skip(4_000).step_by(2).copied().collect();
        let tone = energy_at(&settled, 1_000.0, rate);
        let elsewhere = energy_at(&settled, 6_000.0, rate);
        assert!(
            tone > elsewhere * 8.0,
            "a bit order was read the wrong way round: {tone} at 1 kHz against {elsewhere}"
        );
    }
}

const CUT_SHEET: &str = r#"PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "cut.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Pillow of Winds"
    INDEX 01 00:00:50
  TRACK 03 AUDIO
    TITLE "Echoes"
    INDEX 01 00:01:25
"#;

const CUT_AT: [Frames; 4] = [Frames(0), Frames(29_400), Frames(58_800), Frames(88_200)];

fn cut_spans() -> Vec<FrameSpan> {
    CUT_AT
        .windows(2)
        .map(|pair| FrameSpan::between(pair[0], pair[1]))
        .collect()
}

fn embedded(path: &Path) -> resonate_codec::CueFile {
    probe(&Sources::local(), &MediaLocation::local(path))
        .expect("a cut rip probes")
        .cue
        .expect("the sheet the file carries")
}

fn spans_of(path: &Path) -> Vec<FrameSpan> {
    let info = probe(&Sources::local(), &MediaLocation::local(path)).expect("a cut rip probes");
    let cut = info.cue.as_ref().expect("the sheet the file carries");
    cut.audio_tracks()
        .filter_map(|(index, _)| cut.span_of(index, info.spec.rate, info.duration))
        .collect()
}

fn import_cuesheet(sheet: &Path, target: &Path) -> bool {
    ran(
        "metaflac",
        &[
            &format!("--import-cuesheet-from={}", sheet.to_string_lossy()),
            &target.to_string_lossy(),
        ],
    )
}

#[test]
fn a_cuesheet_block_a_ripper_embedded_cuts_the_file_into_the_tracks_it_names() {
    if !ffmpeg() || !tool("metaflac", "--version") {
        eprintln!("skipped: no metaflac to embed a cue sheet the way a ripper does");
        return;
    }

    let tree = Tree::new();
    let Some((target, _)) = fixture(&tree, "cut.flac", &["-c:a", "flac"]) else {
        return;
    };
    let sheet = tree.at("cut.cue");
    fs::write(&sheet, CUT_SHEET).expect("a writable temporary file");

    if !import_cuesheet(&sheet, &target) {
        eprintln!("skipped: metaflac would not embed the sheet");
        return;
    }
    assert!(
        blocks(&target).contains(&FlacBlock::CueSheet),
        "metaflac stopped writing the block it was asked to import"
    );

    let cut = embedded(&target);

    assert_eq!(cut.audio_tracks().count(), 3);
    assert_eq!(
        cut.tracks.len(),
        4,
        "the lead-out is kept so the last audio track has an end"
    );
    assert_eq!(
        cut.tracks
            .iter()
            .map(|track| track.start)
            .collect::<Vec<_>>(),
        CUT_AT.map(CueStart::Sampled).to_vec(),
        "a block counts in samples and is read as counting in samples"
    );
    assert_eq!(spans_of(&target), cut_spans());
    assert!(
        cut.tracks.iter().all(|track| track.tags.title.is_none()),
        "a CUESHEET block carries no titles to read"
    );
}

#[test]
fn a_cuesheet_comment_names_its_tracks_and_wins_over_the_block_beside_it() {
    let tree = Tree::new();
    let Some(target) = tagged(
        &tree,
        "cut.flac",
        &["-c:a", "flac"],
        &[&format!("CUESHEET={CUT_SHEET}")],
    ) else {
        return;
    };

    let cut = embedded(&target);

    assert_eq!(
        cut.audio_tracks()
            .map(|(_, track)| track.tags.title.clone().unwrap_or_default())
            .collect::<Vec<_>>(),
        vec!["One of These Days", "A Pillow of Winds", "Echoes"]
    );
    assert_eq!(
        cut.tracks[0].start,
        CueStart::Written(CueStamp::new(0, 0, 0))
    );
    assert_eq!(spans_of(&target), cut_spans());

    if !tool("metaflac", "--version") {
        eprintln!("skipped: no metaflac to embed a block beside the comment");
        return;
    }
    let sheet = tree.at("cut.cue");
    fs::write(&sheet, CUT_SHEET).expect("a writable temporary file");
    if !import_cuesheet(&sheet, &target) {
        eprintln!("skipped: metaflac would not embed the sheet");
        return;
    }
    assert!(blocks(&target).contains(&FlacBlock::CueSheet));

    let both = embedded(&target);

    assert_eq!(
        both.tracks[0].tags.title.as_deref(),
        Some("One of These Days"),
        "the block was read where the comment carries the names"
    );
    assert_eq!(
        both.tracks[0].start,
        CueStart::Written(CueStamp::new(0, 0, 0))
    );
    assert_eq!(spans_of(&target), cut_spans());
}

#[test]
fn a_dsf_whose_music_ends_inside_its_last_block_stops_where_the_music_does() {
    const SAMPLE_COUNT_AT: usize = 64;
    const BITS_PER_DOP_FRAME: u64 = 16;
    const FRAMES_SHORT: u64 = 1_000;

    let tree = Tree::new();
    let path = tree.at("short.dsf");
    let mut file = dsf(DSD64, 2, 1_000.0, BitsPerSample::LeastFirst);
    let field = &mut file[SAMPLE_COUNT_AT..SAMPLE_COUNT_AT + 8];
    let padded = u64::from_le_bytes(field.try_into().expect("eight bytes"));
    let declared = padded - FRAMES_SHORT * BITS_PER_DOP_FRAME;
    field.copy_from_slice(&declared.to_le_bytes());
    fs::write(&path, file).expect("a dsf fixture");

    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(&path)).expect("a dsf opens");
    let music = Frames(declared / BITS_PER_DOP_FRAME);
    assert_eq!(info.duration, Some(music));

    let mut out = AudioBuffer::empty(info.spec);
    let mut decoded = 0_u64;
    while decoder.next_block(&mut out).expect("a dsf decodes") == DecodeStatus::Decoded {
        decoded += out.frames() as u64;
    }

    assert_eq!(Frames(decoded), music);
    assert_eq!(decoder.position(), music);
}

fn decoded_by_libopus(path: &Path) -> Option<Vec<i32>> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-c:a", "libopus", "-i"])
        .arg(path)
        .args(["-f", "s32le", "-c:a", "pcm_s32le", "-"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        output
            .stdout
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| i32::from_le_bytes(*word))
            .collect()
    })
}

#[test]
fn opus_decodes_to_what_libopus_decodes_it_to_in_every_container_that_carries_it() {
    let cases: [(&str, Shape, ChannelLayout); 5] = [
        ("broadcast.opus", BROADCAST, ChannelLayout::Stereo),
        ("broadcast.mka", BROADCAST, ChannelLayout::Stereo),
        ("broadcast.mp4", BROADCAST, ChannelLayout::Stereo),
        ("surround.opus", SURROUND, ChannelLayout::Surround51),
        ("surround.mka", SURROUND, ChannelLayout::Surround51),
    ];

    for (name, shape, layout) in cases {
        let tree = Tree::new();
        let Some((path, samples)) =
            shaped(&tree, shape, name, &["-c:a", "libopus", "-b:a", "320k"])
        else {
            return;
        };
        let Some(reference) = decoded_by_libopus(&path) else {
            eprintln!("skipped: this ffmpeg has no libopus to decode {name} with");
            return;
        };
        assert!(
            reference.len() >= samples.len(),
            "libopus decoded {name} short of what went in"
        );

        let decoded = decode(&path);
        assert_eq!(decoded.spec.rate.hz(), 48_000, "{name} changed its rate");
        assert_eq!(decoded.spec.channels, layout, "{name} changed its layout");
        assert_eq!(
            decoded.samples.len(),
            samples.len(),
            "{name} decoded to another length than went in, its priming or its end unkept"
        );
        let (_, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
            .expect("a well-formed file opens");
        let decoded_frames = (samples.len() / usize::from(shape.channels)) as u64;
        assert_eq!(
            info.duration,
            Some(Frames(decoded_frames)),
            "{name} declares another length than it decodes to"
        );

        let kept = samples.len();
        let apart = drift(&decoded.samples[..kept], &reference[..kept]);
        assert!(
            apart < 1e-4,
            "{name} is {apart:e} RMS of full scale away from what libopus decodes it to"
        );
    }
}

#[test]
fn a_stereo_opus_stream_of_mono_packets_decodes_to_both_channels_at_libopus_s_level() {
    let tree = Tree::new();
    let Some((path, samples)) = shaped(
        &tree,
        BROADCAST,
        "folded.opus",
        &["-c:a", "libopus", "-b:a", "6k"],
    ) else {
        return;
    };
    let Some(reference) = decoded_by_libopus(&path) else {
        eprintln!("skipped: this ffmpeg has no libopus to decode folded.opus with");
        return;
    };

    let decoded = decode(&path);
    assert_eq!(decoded.spec.channels, ChannelLayout::Stereo);
    assert_eq!(decoded.samples.len(), samples.len());
    assert!(
        decoded
            .samples
            .as_chunks::<2>()
            .0
            .iter()
            .all(|[left, right]| left == right),
        "a mono packet was not handed to both channels alike"
    );

    let (ours, theirs) = (rms(&decoded.samples), rms(&reference[..samples.len()]));
    assert!(
        (ours - theirs).abs() < theirs * 0.01,
        "folded.opus decoded to a level of {ours:.4} where libopus reads {theirs:.4}"
    );
}

#[test]
fn a_seek_into_opus_hears_what_decoding_from_the_start_hears_there() {
    let tree = Tree::new();
    let Some((path, _)) = shaped(
        &tree,
        BROADCAST,
        "broadcast.opus",
        &["-c:a", "libopus", "-b:a", "320k"],
    ) else {
        return;
    };
    let whole = decode(&path).samples;

    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
        .expect("a well-formed file opens");
    let at = Frames(u64::from(BROADCAST.rate));
    assert_eq!(decoder.seek(at).expect("a seek inside the track"), at);
    let from_there = drain(&mut decoder, info.spec);

    let channels = usize::from(BROADCAST.channels);
    let skipped = at.get() as usize * channels;
    assert_eq!(from_there.len(), whole.len() - skipped);
    let apart = drift(&from_there, &whole[skipped..]);
    assert!(
        apart < 1e-6,
        "a seek landed {apart:e} RMS of full scale away from what the whole decode holds there"
    );
}

#[test]
fn every_field_written_into_an_opus_file_reads_back_and_the_audio_is_left_alone() {
    let tree = Tree::new();
    let Some((path, _)) = shaped(
        &tree,
        BROADCAST,
        "written.opus",
        &["-c:a", "libopus", "-b:a", "128k"],
    ) else {
        return;
    };
    let before = decode(&path).samples;
    let location = MediaLocation::local(&path);
    let tags = FileTags::default();
    assert!(
        tags.writes(&location),
        "an Opus file was not offered for writing"
    );

    let edits: Vec<TagEdit> = TagField::ALL
        .into_iter()
        .map(|field| TagEdit {
            field,
            value: match field {
                TagField::TrackNumber | TagField::TrackTotal => "6".to_owned(),
                TagField::DiscNumber | TagField::DiscTotal => "1".to_owned(),
                _ => format!("{field}"),
            },
        })
        .collect();
    tags.write(
        &location,
        Writing {
            edits: &edits,
            picture: None,
        },
    )
    .expect("a written Opus file");

    let read = tags
        .read(&location, Picturing::Whether)
        .expect("a readable Opus file")
        .tags;
    for edit in &edits {
        assert_eq!(
            edit.field.read(&read).as_deref(),
            Some(edit.value.as_str()),
            "{} did not read back as it was written",
            edit.field
        );
    }
    assert_eq!(
        decode(&path).samples,
        before,
        "writing the tags moved the audio"
    );
}

#[test]
fn a_seek_landing_ahead_of_the_music_does_not_hear_the_priming() {
    let lanes = usize::from(CHANNELS);
    for (name, codec) in [
        ("primed.mp3", &["-c:a", "libmp3lame"][..]),
        ("primed.ogg", &["-c:a", "libvorbis"][..]),
        ("primed.opus", &["-c:a", "libopus"][..]),
    ] {
        let tree = Tree::new();
        let Some((path, _)) = fixture(&tree, name, codec) else {
            return;
        };
        let whole = decode(&path).samples;

        for at in [Frames::ZERO, Frames(1_000)] {
            let (mut decoder, info) =
                Decoder::open(&Sources::local(), &MediaLocation::local(&path))
                    .expect("a well-formed file opens");
            assert!(info.priming() > Frames::ZERO, "{name} declared no priming");
            decoder
                .seek(Frames(u64::from(RATE)))
                .expect("a seek into the track");
            assert_eq!(decoder.seek(at).expect("a seek back"), at);
            let from_there = drain(&mut decoder, info.spec);

            let skipped = at.get() as usize * lanes;
            assert_eq!(from_there.len(), whole.len() - skipped);
            let apart = drift(&from_there, &whole[skipped..]);
            assert!(
                apart < 0.01,
                "a seek to {at} in {name} landed {apart:e} RMS of full scale away from the whole decode"
            );
        }
    }
}
