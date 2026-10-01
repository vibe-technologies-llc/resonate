use std::{
    env, fs,
    io::{self, Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{self, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_codec::{
    Codec, Container, CoverArt, CueStamp, CueStart, DecodeStatus, Decoder, Faststart, FileTags,
    ImageFormat, Picturing, Popularity, Rated, Sources, TagEdit, TagField, TagSet, TagSink,
    TagSource, Writing, probe, probe_cover_art, probe_stream,
};
use resonate_core::{
    AudioBuffer, ChannelCount, ChannelLayout, FrameSpan, Frames, MediaLocation, SampleFormat,
    SampleRate, StreamSpec,
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
    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &wave_fmt(shape));
    chunk(&mut body, b"data", &wave_data(shape, samples));

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    fs::write(path, file).expect("a writable temporary file");
}

fn wave_data(shape: Shape, samples: &[i32]) -> Vec<u8> {
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
    data
}

fn wave_fmt(shape: Shape) -> Vec<u8> {
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
    fmt
}

const WAVE64_RIFF: [u8; 16] = [
    0x72, 0x69, 0x66, 0x66, 0x2E, 0x91, 0xCF, 0x11, 0xA5, 0xD6, 0x28, 0xDB, 0x04, 0xC1, 0x00, 0x00,
];
const WAVE64_TAIL: [u8; 12] = [
    0xF3, 0xAC, 0xD3, 0x11, 0x8C, 0xD1, 0x00, 0xC0, 0x4F, 0x8E, 0xDB, 0x8A,
];
const WAVE64_HEADER: u64 = 24;
const UNSTATED: u32 = u32::MAX;

#[derive(Clone, Copy, Debug)]
enum Wide {
    Rf64,
    Bw64,
    Wave64,
}

fn wide_wave(path: &Path, wide: Wide, shape: Shape, samples: &[i32]) {
    let fmt = wave_fmt(shape);
    let data = wave_data(shape, samples);

    let file = match wide {
        Wide::Rf64 | Wide::Bw64 => {
            let mut ds64 = Vec::new();
            ds64.extend_from_slice(&0_u64.to_le_bytes());
            ds64.extend_from_slice(&(data.len() as u64).to_le_bytes());
            ds64.extend_from_slice(
                &(samples.len() as u64 / u64::from(shape.channels)).to_le_bytes(),
            );
            ds64.extend_from_slice(&0_u32.to_le_bytes());

            let mut body = b"WAVE".to_vec();
            chunk(&mut body, b"ds64", &ds64);
            chunk(&mut body, b"fmt ", &fmt);
            body.extend_from_slice(b"data");
            body.extend_from_slice(&UNSTATED.to_le_bytes());
            body.extend_from_slice(&data);

            let mut file = match wide {
                Wide::Bw64 => b"BW64".to_vec(),
                Wide::Rf64 | Wide::Wave64 => b"RF64".to_vec(),
            };
            file.extend_from_slice(&UNSTATED.to_le_bytes());
            file.extend_from_slice(&body);
            file
        }
        Wide::Wave64 => {
            let mut body = Vec::new();
            wave64_chunk(&mut body, *b"fmt ", &fmt);
            wave64_chunk(&mut body, *b"data", &data);

            let mut file = WAVE64_RIFF.to_vec();
            file.extend_from_slice(&(body.len() as u64 + 40).to_le_bytes());
            file.extend_from_slice(b"wave");
            file.extend_from_slice(&WAVE64_TAIL);
            file.extend_from_slice(&body);
            file
        }
    };
    fs::write(path, file).expect("a writable temporary file");
}

fn wave64_chunk(into: &mut Vec<u8>, id: [u8; 4], payload: &[u8]) {
    into.extend_from_slice(&id);
    into.extend_from_slice(&WAVE64_TAIL);
    into.extend_from_slice(&(payload.len() as u64 + WAVE64_HEADER).to_le_bytes());
    into.extend_from_slice(payload);
    while !into.len().is_multiple_of(8) {
        into.push(0);
    }
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
fn a_wave_too_long_for_riff_decodes_to_exactly_the_samples_that_went_in() {
    let tree = Tree::new();
    let shapes = [
        (CD, ChannelLayout::Stereo),
        (STUDIO, ChannelLayout::Stereo),
        (SURROUND, ChannelLayout::Surround51),
        (SPOKEN, ChannelLayout::Mono),
    ];

    for wide in [Wide::Rf64, Wide::Bw64, Wide::Wave64] {
        for (shape, layout) in shapes {
            let samples = tone(shape);
            let path = tree.at(&format!("{wide:?}-{}.wav", shape.channels));
            wide_wave(&path, wide, shape, &samples);

            let info =
                probe(&Sources::local(), &MediaLocation::local(&path)).expect("a wide wave probes");
            let named = match wide {
                Wide::Rf64 | Wide::Bw64 => Container::Rf64,
                Wide::Wave64 => Container::Wave64,
            };
            assert_eq!(Container::from_id(info.container), named, "{wide:?}");
            assert_eq!(Codec::from_id(info.codec), Codec::Pcm, "{wide:?}");
            assert_eq!(
                info.duration,
                Some(Frames(shape.frames() as u64)),
                "{wide:?}"
            );

            let decoded = decode(&path);
            assert_eq!(decoded.spec.channels, layout, "{wide:?}");
            assert_eq!(decoded.spec.rate.hz(), shape.rate, "{wide:?}");
            assert_eq!(
                decoded.samples,
                widened(&samples, shape.bits),
                "{wide:?} at {} channels did not decode to what went in",
                shape.channels
            );
        }
    }
}

#[test]
fn a_seek_into_a_wide_wave_lands_on_the_frame_asked_for() {
    let tree = Tree::new();
    let samples = tone(STUDIO);
    let lanes = usize::from(STUDIO.channels);

    for wide in [Wide::Rf64, Wide::Wave64] {
        let path = tree.at(&format!("{wide:?}.w64"));
        wide_wave(&path, wide, STUDIO, &samples);
        let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
            .expect("a wide wave opens");

        let wanted = Frames(70_001);
        let landed = decoder.seek(wanted).expect("a seek inside the file");
        let heard = drain(&mut decoder, info.spec);

        assert_eq!(landed, wanted, "{wide:?}");
        assert_eq!(
            heard.first(),
            widened(&samples, STUDIO.bits).get(70_001 * lanes),
            "{wide:?} did not land on the frame asked for"
        );
        assert_eq!(heard.len(), samples.len() - 70_001 * lanes, "{wide:?}");
    }
}

#[test]
fn a_wave_ffmpeg_writes_as_rf64_or_wave64_decodes_to_what_went_in_and_keeps_its_tags() {
    let tree = Tree::new();
    let cases: [(&str, &[&str], Container); 2] = [
        (
            "tone.wav",
            &[
                "-c:a",
                "pcm_s24le",
                "-rf64",
                "always",
                "-metadata",
                "title=Echoes",
            ],
            Container::Rf64,
        ),
        (
            "tone.w64",
            &["-c:a", "pcm_s24le", "-metadata", "title=Echoes"],
            Container::Wave64,
        ),
    ];

    for (name, codec, container) in cases {
        let Some((path, samples)) = shaped(&tree, STUDIO, name, codec) else {
            return;
        };

        let info = probe(&Sources::local(), &MediaLocation::local(&path))
            .expect("a wave ffmpeg wrote probes");
        assert_eq!(Container::from_id(info.container), container, "{name}");
        assert_eq!(
            info.duration,
            Some(Frames(STUDIO.frames() as u64)),
            "{name}"
        );
        if container == Container::Rf64 {
            assert_eq!(info.tags.title.as_deref(), Some("Echoes"), "{name}");
        }
        assert_eq!(
            decode(&path).samples,
            widened(&samples, STUDIO.bits),
            "{name} did not decode to what went in"
        );
    }
}

const CHAPTERS: &str = ";FFMETADATA1\ntitle=Meddle\nartist=Pink Floyd\n\
[CHAPTER]\nTIMEBASE=1/1000\nSTART=0\nEND=1000\ntitle=One of These Days\n\
[CHAPTER]\nTIMEBASE=1/1000\nSTART=1000\nEND=1500\ntitle=A Pillow of Winds\n\
[CHAPTER]\nTIMEBASE=1/1000\nSTART=1500\nEND=2000\ntitle=Fearless\n";

fn chaptered(tree: &Tree, name: &str, codec: &[&str]) -> Option<(PathBuf, Vec<i32>)> {
    if !ffmpeg() {
        eprintln!("skipped: no ffmpeg to build a chaptered {name}");
        return None;
    }
    let samples = tone(CD);
    let source = tree.at("source.wav");
    wav(&source, CD, &samples);
    let chapters = tree.at("chapters.txt");
    fs::write(&chapters, CHAPTERS).expect("a writable temporary file");

    let target = tree.at(name);
    let made = Command::new("ffmpeg")
        .args(["-y", "-v", "error", "-i"])
        .arg(&source)
        .arg("-i")
        .arg(&chapters)
        .args(["-map", "0:a", "-map_metadata", "1", "-map_chapters", "1"])
        .args(codec)
        .arg(&target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        eprintln!("skipped: ffmpeg would not write a chaptered {name}");
        return None;
    }
    Some((target, samples))
}

#[test]
fn a_file_carrying_chapters_is_cut_where_they_start_and_named_by_them() {
    let cases: [(&str, &[&str]); 3] = [
        ("book.m4b", &["-c:a", "aac"]),
        ("book.mp3", &["-c:a", "libmp3lame"]),
        ("book.mka", &["-c:a", "flac"]),
    ];

    for (name, codec) in cases {
        let tree = Tree::new();
        let Some((path, samples)) = chaptered(&tree, name, codec) else {
            return;
        };
        let location = MediaLocation::local(&path);

        let info = probe(&Sources::local(), &location).expect("a chaptered file probes");
        let cut = info.cue.expect("the chapters cut the file");
        let rows: Vec<_> = cut
            .audio_tracks()
            .map(|(_, track)| (track.start.at(info.spec.rate), track.titled().title))
            .collect();
        assert_eq!(
            rows,
            vec![
                (Frames::ZERO, Some("One of These Days".to_owned())),
                (Frames(44_100), Some("A Pillow of Winds".to_owned())),
                (Frames(66_150), Some("Fearless".to_owned())),
            ],
            "{name}"
        );

        let second = cut
            .span_of(1, info.spec.rate, info.duration)
            .expect("a span");
        let heard = resonate_codec::probe_span(&Sources::local(), &location, second)
            .expect("a chapter probes as its own row");
        assert_eq!(
            heard.tags.title.as_deref(),
            Some("A Pillow of Winds"),
            "{name}"
        );
        assert_eq!(heard.tags.album.as_deref(), Some("Meddle"), "{name}");
        assert_eq!(heard.duration, Some(Frames(22_050)), "{name}");

        if name.ends_with(".mka") {
            assert_eq!(
                decode(&path).samples,
                widened(&samples, CD.bits),
                "a Matroska file carrying chapters did not decode whole"
            );
        }
    }
}

#[test]
fn chapter_comments_cut_a_flac_or_an_ogg_where_they_start() {
    let comments = [
        "CHAPTER001=00:00:00.000",
        "CHAPTER001NAME=One of These Days",
        "CHAPTER002=00:00:01.250",
        "CHAPTER002NAME=A Pillow of Winds",
    ];
    for (name, codec) in [("book.flac", "flac"), ("book.ogg", "libvorbis")] {
        let tree = Tree::new();
        let mut arguments = vec!["-c:a", codec];
        for comment in comments {
            arguments.extend(["-metadata", comment]);
        }
        let Some((path, _)) = shaped(&tree, CD, name, &arguments) else {
            return;
        };

        let info = probe(&Sources::local(), &MediaLocation::local(&path))
            .expect("a file carrying chapter comments probes");
        let cut = info.cue.expect("the chapter comments cut the file");
        assert_eq!(
            cut.audio_tracks()
                .map(|(_, track)| (track.start.at(info.spec.rate), track.titled().title))
                .collect::<Vec<_>>(),
            vec![
                (Frames::ZERO, Some("One of These Days".to_owned())),
                (Frames(55_125), Some("A Pillow of Winds".to_owned())),
            ],
            "{name}"
        );
    }
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

const MPEG_ONE_LAYER_THREE_KBPS: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const MPEG_ONE_RATES: [u32; 3] = [44_100, 48_000, 32_000];
const SIDE_INFO_AFTER: usize = 4;
const BIG_VALUES_PAST_THE_GREATEST: u8 = 0xff;

fn mpeg_frame_length(header: &[u8]) -> Option<usize> {
    let kbps = *MPEG_ONE_LAYER_THREE_KBPS.get(usize::from(header.get(2)? >> 4))?;
    let rate = *MPEG_ONE_RATES.get(usize::from((header.get(2)? >> 2) & 0b11))?;
    let padding = usize::from((header.get(2)? >> 1) & 1);
    (kbps > 0).then(|| (144_000 * kbps / rate) as usize + padding)
}

fn frame_at(stream: &[u8], nth: usize) -> Option<usize> {
    let mut at = 0;
    for _ in 0..nth {
        at += mpeg_frame_length(stream.get(at..at + 4)?)?;
    }
    (stream.get(at) == Some(&0xff)).then_some(at)
}

fn frames_decoded(path: &Path) -> usize {
    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &MediaLocation::local(path)).expect("the file opens");
    let mut block = AudioBuffer::empty(info.spec);
    let mut frames = 0;
    while decoder.next_block(&mut block).expect("a decode") == DecodeStatus::Decoded {
        frames += block.frames();
    }
    frames
}

#[test]
fn an_undecodable_packet_is_played_as_the_silence_it_would_have_lasted() {
    const SPOILED: usize = 20;

    let tree = Tree::new();
    let codec = [
        "-c:a",
        "libmp3lame",
        "-b:a",
        "128k",
        "-write_xing",
        "0",
        "-id3v2_version",
        "0",
    ];
    let Some((pristine, _)) = fixture(&tree, "tone.mp3", &codec) else {
        return;
    };
    let mut bytes = fs::read(&pristine).expect("the encoded file");
    let Some(header) = frame_at(&bytes, SPOILED) else {
        eprintln!("skipped: ffmpeg wrote frames this walk does not read");
        return;
    };
    let protected = bytes[header + 1] & 1 == 0;
    let side = header + SIDE_INFO_AFTER + if protected { 2 } else { 0 };
    bytes[side + 4] = BIG_VALUES_PAST_THE_GREATEST;
    bytes[side + 5] |= 0x80;
    let spoiled = tree.at("spoiled.mp3");
    fs::write(&spoiled, &bytes).expect("a writable temporary file");

    assert_eq!(
        frames_decoded(&spoiled),
        frames_decoded(&pristine),
        "a packet that would not decode left the stream short by its length"
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

struct Unmeasured(Cursor<Vec<u8>>);

impl Read for Unmeasured {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Seek for Unmeasured {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.0.seek(to)
    }
}

impl resonate_codec::MediaStream for Unmeasured {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        None
    }
}

struct ServedUnmeasured {
    source: resonate_core::SourceId,
    bytes: Vec<u8>,
}

impl resonate_codec::MediaProvider for ServedUnmeasured {
    fn source(&self) -> &resonate_core::SourceId {
        &self.source
    }

    fn open(&self, _: &MediaLocation) -> resonate_codec::Result<resonate_codec::Media> {
        Ok(resonate_codec::Media {
            stream: Box::new(Unmeasured(Cursor::new(self.bytes.clone()))),
            hint: None,
        })
    }
}

#[test]
fn a_stream_that_seeks_but_declares_no_length_seeks_all_the_same() {
    let tree = Tree::new();
    let Some((path, _)) = fixture(&tree, "rip.aac", &["-c:a", "aac", "-b:a", "128k"]) else {
        return;
    };
    let served = resonate_core::SourceId::new("served").expect("a nameable source");
    let sources = Sources::local().and(std::sync::Arc::new(ServedUnmeasured {
        source: served.clone(),
        bytes: fs::read(&path).expect("the stream reads"),
    }));
    let (mut decoder, info) = Decoder::open(&sources, &MediaLocation::new(served, "rip.aac"))
        .expect("an adts stream opens");
    assert!(info.is_seekable);
    assert_eq!(
        info.duration, None,
        "the stream declared a length after all"
    );

    let wanted = Frames(u64::from(CD.rate) / 2);
    let landed = decoder
        .seek(wanted)
        .expect("a seek inside the stream lands");

    assert_eq!(landed, wanted);
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
fn a_vorbis_in_matroska_is_as_long_as_its_packets_whatever_the_segment_declares() {
    const SEGMENT_DURATION_AS_A_DOUBLE: [u8; 3] = [0x44, 0x89, 0x88];

    let tree = Tree::new();
    let Some((path, _)) = fixture(&tree, "lossy.mka", &["-c:a", "libvorbis"]) else {
        return;
    };
    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
        .expect("a well-formed file opens");
    let decoded = Frames((drain(&mut decoder, info.spec).len() / usize::from(CD.channels)) as u64);

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
            Some(decoded),
            "{} was not counted to the frames its packets decode to",
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
fn a_vorbis_rip_over_a_pipe_is_spooled_and_drops_the_priming_and_padding_its_pages_declare() {
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

    assert!(
        info.is_seekable,
        "a pipe short enough to spool was not read whole"
    );
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
        Some(music),
        "a spooled pipe reaches the last page, so its end bounds the music"
    );

    let decoded = drain(&mut decoder, info.spec);
    assert_eq!(
        (decoded.len() / lanes) as u64,
        music.get(),
        "an ogg over a pipe decoded its priming or its padding as audio"
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

fn id3_frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut frame = id.to_vec();
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(body);
    frame
}

fn id3_tag(frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
    let body: Vec<u8> = frames
        .iter()
        .flatten()
        .copied()
        .chain(std::iter::repeat_n(0, padding))
        .collect();
    let size = body.len() as u32;
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    tag.extend_from_slice(&[
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    tag.extend_from_slice(&body);
    tag
}

fn tagged_dsf(tag: &[u8]) -> Vec<u8> {
    const METADATA_AT: usize = 20;
    const TOTAL_AT: usize = 12;

    let mut file = dsf(DSD64, 2, 1_000.0, BitsPerSample::LeastFirst);
    let at = file.len() as u64;
    file.extend_from_slice(tag);
    let total = file.len() as u64;
    file[METADATA_AT..METADATA_AT + 8].copy_from_slice(&at.to_le_bytes());
    file[TOTAL_AT..TOTAL_AT + 8].copy_from_slice(&total.to_le_bytes());
    file
}

#[test]
fn a_dsf_offers_the_cover_its_id3_tag_carries_however_large_the_tag() {
    const PAST_A_MEBIBYTE: usize = 3 << 20;

    let mut picture = vec![0];
    picture.extend_from_slice(b"image/png\0");
    picture.push(3);
    picture.push(0);
    picture.extend_from_slice(&ONE_PIXEL_PNG);
    let tag = id3_tag(
        &[id3_frame(b"APIC", &picture), id3_frame(b"TIT2", b"\0Pulse")],
        PAST_A_MEBIBYTE,
    );

    let tree = Tree::new();
    let path = tree.at("tagged.dsf");
    fs::write(&path, tagged_dsf(&tag)).expect("a dsf fixture");
    let location = MediaLocation::local(&path);

    let info = probe(&Sources::local(), &location).expect("a dsf probes");
    let cover = probe_cover_art(&Sources::local(), &location).expect("a dsf opens");

    assert_eq!(info.tags.title.as_deref(), Some("Pulse"));
    assert_eq!(
        cover.map(|art| art.bytes),
        Some(ONE_PIXEL_PNG.to_vec()),
        "the picture the DSF's tag carries was not offered"
    );
}

#[test]
fn a_dsf_places_its_channels_by_the_type_it_declares_rather_than_their_count() {
    const CHANNEL_TYPE_AT: usize = 48;

    let cases: [(u16, u32, ChannelLayout); 7] = [
        (2, 2, ChannelLayout::Stereo),
        (
            3,
            3,
            ChannelLayout::Discrete(ChannelCount::new(3).expect("three")),
        ),
        (4, 4, ChannelLayout::Quad),
        (
            4,
            5,
            ChannelLayout::Discrete(ChannelCount::new(4).expect("four")),
        ),
        (5, 6, ChannelLayout::Surround50),
        (6, 7, ChannelLayout::Surround51),
        (
            4,
            9,
            ChannelLayout::Discrete(ChannelCount::new(4).expect("four")),
        ),
    ];

    let tree = Tree::new();
    for (channels, kind, layout) in cases {
        let path = tree.at(&format!("{channels}-{kind}.dsf"));
        let mut file = dsf(DSD64, channels, 1_000.0, BitsPerSample::LeastFirst);
        file[CHANNEL_TYPE_AT..CHANNEL_TYPE_AT + 4].copy_from_slice(&kind.to_le_bytes());
        fs::write(&path, file).expect("a dsf fixture");

        let info = probe(&Sources::local(), &MediaLocation::local(&path)).expect("a dsf probes");
        assert_eq!(info.spec.channels, layout, "channel type {kind}");
        assert_eq!(info.speakers.is_named(), kind != 9, "channel type {kind}");
    }
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
fn a_seek_into_the_first_moments_of_opus_in_matroska_hears_what_decoding_from_the_start_hears() {
    let tree = Tree::new();
    let Some((path, _)) = shaped(
        &tree,
        BROADCAST,
        "broadcast.mka",
        &["-c:a", "libopus", "-b:a", "320k"],
    ) else {
        return;
    };
    let whole = decode(&path).samples;
    let channels = usize::from(BROADCAST.channels);

    for at in [
        Frames::ZERO,
        Frames(4_800),
        Frames(u64::from(BROADCAST.rate) / 4),
    ] {
        let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&path))
            .expect("a well-formed file opens");
        assert!(
            info.priming() > Frames::ZERO,
            "the stream declared no priming"
        );
        decoder
            .seek(Frames(u64::from(BROADCAST.rate)))
            .expect("a seek into the track");
        assert_eq!(decoder.seek(at).expect("a seek back"), at);
        let from_there = drain(&mut decoder, info.spec);

        let skipped = at.get() as usize * channels;
        assert_eq!(
            from_there.len(),
            whole.len() - skipped,
            "a seek to {at} heard another length than the whole decode holds from there"
        );
        let apart = drift(&from_there, &whole[skipped..]);
        assert!(
            apart < 1e-6,
            "a seek to {at} landed {apart:e} RMS of full scale away from what the whole decode holds there"
        );
    }
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
            value: written_as(field),
        })
        .collect();
    tags.write(
        &location,
        Writing {
            edits: &edits,
            taken: &[],
            picture: None,
            unpictured: false,
            popularity: None,
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
fn a_cover_taken_away_is_gone_and_a_cover_written_replaces_the_one_there() {
    let cases: [(&str, &[&str]); 3] = [
        ("covered.m4a", &["-c:a", "aac"]),
        ("covered.mp3", &["-c:a", "libmp3lame"]),
        ("covered.flac", &["-c:a", "flac"]),
    ];
    let first = CoverArt {
        format: ImageFormat::Png,
        bytes: ONE_PIXEL_PNG.to_vec(),
    };
    let mut second = first.clone();
    second.bytes.extend_from_slice(b"another");

    for (name, codec) in cases {
        let tree = Tree::new();
        let Some((path, _)) = shaped(&tree, CD, name, codec) else {
            return;
        };
        let location = MediaLocation::local(&path);
        let tags = FileTags::default();
        let pictured = |picture: Option<&CoverArt>, unpictured: bool| {
            tags.write(
                &location,
                Writing {
                    edits: &[],
                    taken: &[],
                    picture,
                    unpictured,
                    popularity: None,
                },
            )
            .expect("a file whose picture was written");
        };
        let read = || {
            tags.read(&location, Picturing::Copied)
                .expect("a readable file")
                .picture
                .into_copied()
        };

        pictured(Some(&first), false);
        pictured(Some(&second), false);
        assert_eq!(read(), Some(second.clone()), "{name}");

        pictured(None, true);
        assert_eq!(read(), None, "{name} kept a cover taken away");
    }
}

fn read_back_from_an_ape_tag(read: &TagSet, edits: &[TagEdit]) {
    for edit in edits {
        assert_eq!(
            edit.field.read(read).as_deref(),
            Some(edit.value.as_str()),
            "{} did not read back as an APE tag holds it",
            edit.field
        );
    }
}

fn written_as(field: TagField) -> String {
    match field {
        TagField::TrackNumber | TagField::TrackTotal => "6".to_owned(),
        TagField::DiscNumber | TagField::DiscTotal | TagField::Compilation => "1".to_owned(),
        TagField::BeatsPerMinute => "120".to_owned(),
        TagField::ReplayGainTrackGain => "-6.50 dB".to_owned(),
        TagField::ReplayGainAlbumGain => "-7.25 dB".to_owned(),
        TagField::ReplayGainTrackPeak => "0.988547".to_owned(),
        TagField::ReplayGainAlbumPeak => "0.999969".to_owned(),
        _ => format!("{field}"),
    }
}

fn rated_as(popularity: Popularity) -> Writing<'static> {
    Writing {
        edits: &[],
        taken: &[],
        picture: None,
        unpictured: false,
        popularity: Some(popularity),
    }
}

fn counted_and_favoured_through(path: &Path) {
    let name = path.display();
    let before = decode(path).samples;
    let location = MediaLocation::local(path);
    let tags = FileTags::default();
    assert!(tags.writes(&location), "{name} was not offered for writing");

    let favoured = Popularity {
        favourite: true,
        plays: 5,
    };
    tags.write(&location, rated_as(favoured))
        .expect("a favoured file");
    assert_eq!(
        tags.rated(&location).expect("a readable file"),
        Rated::Favourite { plays: Some(5) },
        "{name}"
    );

    let heard = Popularity {
        favourite: false,
        plays: 6,
    };
    tags.write(&location, rated_as(heard))
        .expect("a counted file");
    let held = tags.rated(&location).expect("a readable file");
    assert_eq!(held, Rated::Unrated { plays: Some(6) }, "{name}");
    assert!(!held.differs_from(heard), "{name} did not read back");
    assert_eq!(
        decode(path).samples,
        before,
        "counting the plays of {name} moved the audio"
    );
}

#[test]
fn a_play_count_and_a_favourite_read_back_out_of_every_tag_this_build_writes() {
    for (name, codec) in [
        ("counted.mp3", &["-c:a", "libmp3lame"][..]),
        ("counted.m4a", &["-c:a", "aac", "-b:a", "128k"][..]),
        ("counted.ogg", &["-c:a", "libvorbis"][..]),
        ("counted.opus", &["-c:a", "libopus", "-b:a", "128k"][..]),
        ("counted.wv", &["-c:a", "wavpack"][..]),
    ] {
        let tree = Tree::new();
        let Some((path, _)) = fixture(&tree, name, codec) else {
            continue;
        };
        counted_and_favoured_through(&path);
    }
}

#[test]
fn a_favourite_reaches_a_monkeys_audio_file_as_its_ape_tag_says_one() {
    if !monkeys() {
        eprintln!("skipped: no mac to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, CD, &tone(CD));
    let target = tree.at("counted.ape");
    assert!(
        monkeyed(&source, &target, "-c2000"),
        "mac would not write it"
    );

    counted_and_favoured_through(&target);
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

const WIDEST: Shape = Shape {
    rate: 48_000,
    channels: 2,
    bits: 32,
};

const WIDEST_ALONE: Shape = Shape {
    rate: 48_000,
    channels: 1,
    bits: 32,
};

const LONE: Shape = Shape {
    rate: 44_100,
    channels: 1,
    bits: 16,
};

const IEEE_FLOAT: u16 = 3;
const FLOAT_BYTES: u16 = 4;

fn wavpack() -> bool {
    tool("wavpack", "--version")
}

fn packed(source: &Path, target: &Path, modes: &[&str]) -> bool {
    Command::new("wavpack")
        .args(["-q", "-y"])
        .args(modes)
        .arg(source)
        .arg("-o")
        .arg(target)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
        && target.exists()
}

fn float_wav(path: &Path, rate: u32, channels: u16, samples: &[f32]) {
    let align = channels * FLOAT_BYTES;
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&IEEE_FLOAT.to_le_bytes());
    fmt.extend_from_slice(&channels.to_le_bytes());
    fmt.extend_from_slice(&rate.to_le_bytes());
    fmt.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
    fmt.extend_from_slice(&align.to_le_bytes());
    fmt.extend_from_slice(&(FLOAT_BYTES * 8).to_le_bytes());

    let data: Vec<u8> = samples
        .iter()
        .flat_map(|sample| sample.to_le_bytes())
        .collect();
    let mut body = b"WAVE".to_vec();
    chunk(&mut body, b"fmt ", &fmt);
    chunk(&mut body, b"data", &data);

    let mut file = b"RIFF".to_vec();
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    fs::write(path, file).expect("a writable temporary file");
}

fn awkward_floats(frames: usize) -> Vec<f32> {
    let mut samples = Vec::with_capacity(frames * 2);
    let mut state = 0x2545_f491_u32;
    for frame in 0..frames {
        for _ in 0..2 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let sample = match frame % 9 {
                0 => 0.0,
                1 => -0.0,
                2 => f32::from_bits(state & 0x007f_ffff),
                3 => f32::from_bits((state & 0x807f_ffff) | 0x0d00_0000),
                4 => 1.0,
                5 => -1.5,
                _ => {
                    let time = frame as f32 / 48_000.0;
                    0.8 * (std::f32::consts::TAU * 440.0 * time).sin()
                        * f32::from_bits((state >> 9) | 0x3f80_0000)
                        / 2.0
                }
            };
            samples.push(sample);
        }
    }
    samples
}

fn drain_floats(decoder: &mut Decoder, spec: StreamSpec) -> Vec<u32> {
    decoder.set_output_format(SampleFormat::F32);
    let mut block = AudioBuffer::empty(spec);
    let mut samples = Vec::new();
    while decoder.next_block(&mut block).expect("a clean decode") == DecodeStatus::Decoded {
        match block.data() {
            resonate_core::SampleData::F32(store) => {
                samples.extend(store.iter().map(|sample| sample.to_bits()));
            }
            other => panic!("the decoder ignored the requested output format: {other:?}"),
        }
    }
    samples
}

#[test]
fn wavpack_decodes_every_depth_and_layout_to_exactly_what_went_in() {
    if !wavpack() {
        eprintln!("skipped: no wavpack to build the fixtures");
        return;
    }
    let cases: [(&str, Shape, ChannelLayout, &[&str]); 6] = [
        ("cd.wv", CD, ChannelLayout::Stereo, &[]),
        ("cd.hh.wv", CD, ChannelLayout::Stereo, &["-hh", "-x6"]),
        ("studio.wv", STUDIO, ChannelLayout::Stereo, &["-f"]),
        ("widest.wv", WIDEST, ChannelLayout::Stereo, &["-h"]),
        ("surround.wv", SURROUND, ChannelLayout::Surround51, &[]),
        ("lone.wv", LONE, ChannelLayout::Mono, &["-x3"]),
    ];

    for (name, shape, layout, modes) in cases {
        let tree = Tree::new();
        let samples = tone(shape);
        let source = tree.at("source.wav");
        wav(&source, shape, &samples);
        let target = tree.at(name);
        assert!(
            packed(&source, &target, modes),
            "wavpack would not write {name}"
        );

        let report = probe_stream(&Sources::local(), &MediaLocation::local(&target))
            .expect("a WavPack file probes");
        assert_eq!(
            Container::from_id(report.info.container),
            Container::WavPack
        );
        assert_eq!(Codec::from_id(report.info.codec), Codec::WavPack);

        let decoded = decode(&target);
        assert_eq!(
            decoded.spec.rate.hz(),
            shape.rate,
            "{name} changed its rate"
        );
        assert_eq!(decoded.spec.channels, layout, "{name} changed its layout");
        let wanted = widened(&samples, shape.bits);
        assert_eq!(
            decoded.samples.len(),
            wanted.len(),
            "{name} changed its length"
        );
        assert_eq!(
            first_apart(&decoded.samples, &wanted),
            None,
            "{name} is lossless and did not round-trip"
        );
    }
}

#[test]
fn a_floating_wavpack_decodes_to_every_bit_that_went_in() {
    if !wavpack() {
        eprintln!("skipped: no wavpack to build the fixtures");
        return;
    }
    let tree = Tree::new();
    let samples = awkward_floats(48_000);
    let source = tree.at("floats.wav");
    float_wav(&source, 48_000, 2, &samples);

    for modes in [&[][..], &["-hh", "-x4"][..]] {
        let target = tree.at("floats.wv");
        assert!(
            packed(&source, &target, modes),
            "wavpack would not write floats"
        );

        let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&target))
            .expect("a floating WavPack file opens");
        assert_eq!(info.spec.format, SampleFormat::F32);
        let decoded = drain_floats(&mut decoder, info.spec);

        let wanted: Vec<u32> = samples.iter().map(|sample| sample.to_bits()).collect();
        assert_eq!(decoded.len(), wanted.len());
        assert_eq!(
            first_apart(&decoded, &wanted),
            None,
            "a floating WavPack under {modes:?} did not decode to the bits that went in"
        );
    }
}

#[test]
fn a_hybrid_wavpack_decodes_to_what_the_reference_decoder_makes_of_it() {
    if !wavpack() || !tool("wvunpack", "--version") {
        eprintln!("skipped: no wavpack and wvunpack to build the fixture");
        return;
    }
    let tree = Tree::new();
    let samples = tone(CD);
    let source = tree.at("source.wav");
    wav(&source, CD, &samples);
    let target = tree.at("hybrid.wv");
    assert!(
        packed(&source, &target, &["-b256"]),
        "wavpack would not write a hybrid file"
    );

    let unpacked = tree.at("unpacked.wav");
    assert!(
        ran(
            "wvunpack",
            &[
                "-q",
                "-y",
                target.to_str().expect("a UTF-8 path"),
                "-o",
                unpacked.to_str().expect("a UTF-8 path"),
            ]
        ),
        "wvunpack would not decode the hybrid file"
    );

    assert_eq!(
        decode(&target).samples,
        decode(&unpacked).samples,
        "a hybrid WavPack decoded to something the reference decoder does not"
    );
    let report = probe_stream(&Sources::local(), &MediaLocation::local(&target))
        .expect("a hybrid WavPack probes");
    assert_eq!(Codec::from_id(report.info.codec), Codec::WavPackHybrid);
    assert!(!Codec::from_id(report.info.codec).is_lossless());
}

#[test]
fn a_hybrid_wavpack_in_matroska_is_billed_as_hybrid_and_a_lossless_one_is_not() {
    if !wavpack() || !ffmpeg() {
        eprintln!("skipped: no wavpack and ffmpeg to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, CD, &tone(CD));

    for (name, modes, billed) in [
        ("hybrid", &["-b256"][..], Codec::WavPackHybrid),
        ("lossless", &[][..], Codec::WavPack),
    ] {
        let native = tree.at(&format!("{name}.wv"));
        assert!(
            packed(&source, &native, modes),
            "wavpack would not write the {name} file"
        );
        let wrapped = tree.at(&format!("{name}.mka"));
        assert!(
            ran(
                "ffmpeg",
                &[
                    "-loglevel",
                    "error",
                    "-y",
                    "-i",
                    native.to_str().expect("a UTF-8 path"),
                    "-c:a",
                    "copy",
                    wrapped.to_str().expect("a UTF-8 path"),
                ]
            ),
            "ffmpeg would not put the {name} WavPack into Matroska"
        );

        let report = probe_stream(&Sources::local(), &MediaLocation::local(&wrapped))
            .expect("a WavPack in Matroska probes");
        assert_eq!(Codec::from_id(report.info.codec), billed, "{name}");
        assert_eq!(
            decode(&wrapped).samples,
            decode(&native).samples,
            "a {name} WavPack in Matroska decoded to something the native stream does not"
        );
    }
}

#[test]
fn a_wavpack_carries_its_ape_tags_into_the_set_and_seeks_where_asked() {
    if !wavpack() {
        eprintln!("skipped: no wavpack to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, CD, &tone(CD));
    let target = tree.at("tagged.wv");
    assert!(
        packed(
            &source,
            &target,
            &[
                "-w",
                "Title=Echoes",
                "-w",
                "Artist=Pink Floyd",
                "-w",
                "Album=Meddle",
                "-w",
                "Album Artist=Pink Floyd",
                "-w",
                "Track=2/6",
                "-w",
                "Year=1971",
            ]
        ),
        "wavpack would not write a tagged file"
    );

    let report = probe_stream(&Sources::local(), &MediaLocation::local(&target))
        .expect("a tagged WavPack probes");
    let tags = &report.info.tags;
    assert_eq!(tags.title.as_deref(), Some("Echoes"));
    assert_eq!(tags.artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(tags.album.as_deref(), Some("Meddle"));
    assert_eq!(tags.album_artist.as_deref(), Some("Pink Floyd"));
    assert_eq!(tags.track_number, Some(2));
    assert_eq!(tags.date.as_deref(), Some("1971"));

    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&target))
        .expect("a tagged WavPack opens");
    assert!(info.is_seekable);
    let whole = decode(&target).samples;
    let lanes = usize::from(CHANNELS);
    for target in [
        Frames(u64::from(RATE)),
        Frames(u64::from(RATE) / 3),
        Frames(0),
    ] {
        let landed = decoder.seek(target).expect("a seekable stream seeks");
        assert_eq!(landed, target, "seek to {target} landed on {landed}");
        let from_there = drain(&mut decoder, info.spec);
        let skipped = target.get() as usize * lanes;
        assert_eq!(
            from_there,
            whole[skipped..],
            "a seek to {target} heard another stream"
        );
    }
}

#[test]
fn every_field_written_into_a_wavpack_file_reads_back_and_the_audio_is_left_alone() {
    if !wavpack() {
        eprintln!("skipped: no wavpack to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, CD, &tone(CD));
    let path = tree.at("written.wv");
    assert!(
        packed(&source, &path, &[]),
        "wavpack would not write the fixture"
    );

    let before = decode(&path).samples;
    let location = MediaLocation::local(&path);
    let tags = FileTags::default();
    assert!(
        tags.writes(&location),
        "a WavPack file was not offered for writing"
    );

    let edits: Vec<TagEdit> = TagField::ALL
        .into_iter()
        .map(|field| TagEdit {
            field,
            value: written_as(field),
        })
        .collect();
    tags.write(
        &location,
        Writing {
            edits: &edits,
            taken: &[],
            picture: None,
            unpictured: false,
            popularity: None,
        },
    )
    .expect("a written WavPack file");

    let read = tags
        .read(&location, Picturing::Whether)
        .expect("a readable WavPack file")
        .tags;
    read_back_from_an_ape_tag(&read, &edits);
    assert_eq!(
        decode(&path).samples,
        before,
        "writing the tags moved the audio"
    );
}

fn first_apart<T: PartialEq>(decoded: &[T], wanted: &[T]) -> Option<usize> {
    decoded
        .iter()
        .zip(wanted)
        .position(|(held, sent)| held != sent)
}

fn monkeys() -> bool {
    Command::new("mac")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

fn monkeyed(source: &Path, target: &Path, level: &str) -> bool {
    ran(
        "mac",
        &[
            source.to_str().expect("a UTF-8 path"),
            target.to_str().expect("a UTF-8 path"),
            level,
        ],
    ) && target.exists()
}

#[test]
fn monkeys_audio_decodes_every_depth_and_layout_to_exactly_what_went_in() {
    if !monkeys() {
        eprintln!("skipped: no mac to build the fixtures");
        return;
    }
    let cases: [(&str, Shape, ChannelLayout, &str); 6] = [
        ("cd.ape", CD, ChannelLayout::Stereo, "-c2000"),
        ("cd.insane.ape", CD, ChannelLayout::Stereo, "-c5000"),
        ("studio.ape", STUDIO, ChannelLayout::Stereo, "-c1000"),
        ("widest.ape", WIDEST_ALONE, ChannelLayout::Mono, "-c3000"),
        (
            "surround.ape",
            SURROUND,
            ChannelLayout::Surround51,
            "-c4000",
        ),
        ("lone.ape", LONE, ChannelLayout::Mono, "-c2000"),
    ];

    for (name, shape, layout, level) in cases {
        let tree = Tree::new();
        let samples = tone(shape);
        let source = tree.at("source.wav");
        wav(&source, shape, &samples);
        let target = tree.at(name);
        assert!(
            monkeyed(&source, &target, level),
            "mac would not write {name}"
        );

        let report = probe_stream(&Sources::local(), &MediaLocation::local(&target))
            .expect("a Monkey's Audio file probes");
        assert_eq!(
            Container::from_id(report.info.container),
            Container::MonkeysAudio
        );
        assert_eq!(Codec::from_id(report.info.codec), Codec::MonkeysAudio);

        let decoded = decode(&target);
        assert_eq!(
            decoded.spec.rate.hz(),
            shape.rate,
            "{name} changed its rate"
        );
        assert_eq!(decoded.spec.channels, layout, "{name} changed its layout");
        let wanted = widened(&samples, shape.bits);
        assert_eq!(
            decoded.samples.len(),
            wanted.len(),
            "{name} changed its length"
        );
        assert_eq!(
            first_apart(&decoded.samples, &wanted),
            None,
            "{name} is lossless and did not round-trip"
        );
    }
}

#[test]
fn a_floating_monkeys_audio_decodes_to_every_bit_that_went_in() {
    if !monkeys() {
        eprintln!("skipped: no mac to build the fixture");
        return;
    }
    let tree = Tree::new();
    let frames = 96_000;
    let samples: Vec<f32> = (0..frames)
        .map(|frame| {
            let time = frame as f32 / 48_000.0;
            let fading = (-(frame as f32) / 3_000.0).exp();
            if frame % 997 == 0 {
                -0.0
            } else {
                0.8 * (std::f32::consts::TAU * 440.0 * time).sin() * fading
            }
        })
        .collect();
    let source = tree.at("floats.wav");
    float_wav(&source, 48_000, 1, &samples);
    let target = tree.at("floats.ape");
    assert!(
        monkeyed(&source, &target, "-c3000"),
        "mac would not write floats"
    );

    let (mut decoder, info) = Decoder::open(&Sources::local(), &MediaLocation::local(&target))
        .expect("a floating Monkey's Audio file opens");
    assert_eq!(info.spec.format, SampleFormat::F32);
    let decoded = drain_floats(&mut decoder, info.spec);
    let wanted: Vec<u32> = samples.iter().map(|sample| sample.to_bits()).collect();
    assert_eq!(decoded.len(), wanted.len());
    assert_eq!(
        first_apart(&decoded, &wanted),
        None,
        "a floating Monkey's Audio lost bits"
    );
}

#[test]
fn a_32_bit_stereo_monkeys_audio_is_refused_rather_than_heard_wrong() {
    if !monkeys() {
        eprintln!("skipped: no mac to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, WIDEST, &tone(WIDEST));
    let target = tree.at("widest.ape");
    assert!(
        monkeyed(&source, &target, "-c2000"),
        "mac would not write the fixture"
    );

    assert!(
        probe(&Sources::local(), &MediaLocation::local(&target)).is_ok(),
        "a 32-bit stereo file did not even probe"
    );
    assert!(matches!(
        Decoder::open(&Sources::local(), &MediaLocation::local(&target)),
        Err(resonate_codec::Error::NoDecoder { .. })
    ));
}

#[test]
fn a_monkeys_audio_made_from_an_aiff_decodes_to_the_same_samples() {
    if !monkeys() || !ffmpeg() {
        eprintln!("skipped: no mac and ffmpeg to build the fixture");
        return;
    }
    let tree = Tree::new();
    let samples = tone(STUDIO);
    let source = tree.at("source.wav");
    wav(&source, STUDIO, &samples);
    let aiff = tree.at("source.aiff");
    assert!(
        encode(&source, &aiff, &["-c:a", "pcm_s24be"]),
        "ffmpeg would not write an AIFF"
    );
    let target = tree.at("from-aiff.ape");
    assert!(
        monkeyed(&aiff, &target, "-c2000"),
        "mac would not take an AIFF"
    );

    assert_eq!(decode(&target).samples, widened(&samples, STUDIO.bits));
}

#[test]
fn a_monkeys_audio_takes_its_tags_and_seeks_where_asked() {
    if !monkeys() {
        eprintln!("skipped: no mac to build the fixture");
        return;
    }
    let tree = Tree::new();
    let source = tree.at("source.wav");
    wav(&source, CD, &tone(CD));
    let path = tree.at("written.ape");
    assert!(
        monkeyed(&source, &path, "-c2000"),
        "mac would not write the fixture"
    );

    let before = decode(&path).samples;
    let location = MediaLocation::local(&path);
    let tags = FileTags::default();
    assert!(
        tags.writes(&location),
        "a Monkey's Audio file was not offered for writing"
    );

    let edits: Vec<TagEdit> = TagField::ALL
        .into_iter()
        .map(|field| TagEdit {
            field,
            value: written_as(field),
        })
        .collect();
    tags.write(
        &location,
        Writing {
            edits: &edits,
            taken: &[],
            picture: None,
            unpictured: false,
            popularity: None,
        },
    )
    .expect("a written Monkey's Audio file");

    let read = tags
        .read(&location, Picturing::Whether)
        .expect("a readable Monkey's Audio file")
        .tags;
    read_back_from_an_ape_tag(&read, &edits);
    assert_eq!(
        decode(&path).samples,
        before,
        "writing the tags moved the audio"
    );

    let (mut decoder, info) =
        Decoder::open(&Sources::local(), &location).expect("a tagged Monkey's Audio opens");
    assert!(info.is_seekable);
    let lanes = usize::from(CHANNELS);
    for target in [
        Frames(u64::from(RATE) * 2),
        Frames(u64::from(RATE) / 3),
        Frames(0),
    ] {
        let landed = decoder.seek(target).expect("a seekable stream seeks");
        assert_eq!(landed, target, "seek to {target} landed on {landed}");
        let from_there = drain(&mut decoder, info.spec);
        let skipped = target.get() as usize * lanes;
        assert_eq!(
            from_there,
            before[skipped..],
            "a seek to {target} heard another stream"
        );
    }
}

#[test]
fn an_adpcm_wave_opens_and_is_billed_as_the_lossy_codec_it_is() {
    for (name, codec) in [("ima.wav", "adpcm_ima_wav"), ("ms.wav", "adpcm_ms")] {
        let tree = Tree::new();
        let Some((path, samples)) = fixture(&tree, name, &["-c:a", codec]) else {
            return;
        };

        let info =
            probe(&Sources::local(), &MediaLocation::local(&path)).expect("an ADPCM WAVE probes");
        assert_eq!(Codec::from_id(info.codec), Codec::Adpcm, "{name}");
        assert!(!Codec::from_id(info.codec).is_lossless());

        let decoded = decode(&path);
        assert!(
            decoded.samples.len().abs_diff(samples.len()) < samples.len() / 10,
            "{name} decoded to {} samples where {} went in",
            decoded.samples.len(),
            samples.len()
        );
    }
}
