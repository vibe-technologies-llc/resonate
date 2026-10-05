use std::io::{Read, Seek, SeekFrom};

use resonate_core::{Decibels, Frames};

use crate::{
    prescan::{past_id3, read_exact},
    tags::ReplayGain,
};

const MPEG_SYNC: u8 = 0xff;
const MPEG_SYNC_HIGH: u8 = 0xe0;
const HEADER_BYTES: u64 = 4;

const BITRATES_MPEG_ONE_LAYER_ONE: [u32; 15] = [
    0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
];
const BITRATES_MPEG_ONE_LAYER_TWO: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
];
const BITRATES_MPEG_ONE_LAYER_THREE: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];
const BITRATES_MPEG_TWO_LAYER_ONE: [u32; 15] = [
    0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
];
const BITRATES_MPEG_TWO_LAYERS_TWO_AND_THREE: [u32; 15] =
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
const LAYER_TWO_REFUSED_IN_MONO_KBPS: [u32; 4] = [224, 256, 320, 384];
const LAYER_TWO_REFUSED_BESIDE_MONO_KBPS: [u32; 4] = [32, 48, 56, 80];
const MONO: u8 = 0b11;
const FREE_BITRATE: u8 = 0b0000;
const BAD_BITRATE: u8 = 0b1111;
const BITS_A_KILOBIT: u32 = 1_000;

const XING: &[u8; 4] = b"Xing";
const INFO: &[u8; 4] = b"Info";
const VBRI: &[u8; 4] = b"VBRI";
const VBRI_AT: u64 = HEADER_BYTES + 32;
const XING_NAMES_ITS_FRAMES: u8 = 0b0001;
const XING_NOTE_BYTES: u64 = 8;
const XING_FIELDS: [(u8, u64); 4] = [
    (XING_NAMES_ITS_FRAMES, 4),
    (0b0010, 4),
    (0b0100, 100),
    (0b1000, 4),
];
const LAME_TAG_BYTES: usize = 19;
const LAME_PEAK_AT: usize = 11;
const LAME_GAINS_AT: [usize; 2] = [15, 17];
const LAME_PEAK_FULL_SCALE: f32 = 8_388_608.0;
const LAME_TRACK_GAIN: u16 = 0b001;
const LAME_ALBUM_GAIN: u16 = 0b010;
const LAME_GAIN_NEGATIVE: u16 = 0x200;
const LAME_GAIN_TENTHS: u16 = 0x1ff;
const TENTHS_A_DECIBEL: f32 = 10.0;
const TRAILERS: [&[u8]; 3] = [b"TAG", b"APETAGEX", b"LYRICSBEGIN"];
const LONGEST_TRAILER: usize = 11;

const SAMPLED_AT: u64 = 8;
const OPENING_FRAMES_WEIGHED: usize = 32;
const RESYNCED_WITHIN: u64 = 64 * 1024;
const SAMPLE_RESYNCED_WITHIN: u64 = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MpegVersion {
    One,
    Two,
    TwoAndAHalf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MpegLayer {
    One,
    Two,
    Three,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stream {
    version: MpegVersion,
    layer: MpegLayer,
    rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameHeader {
    stream: Stream,
    kbps: u32,
    mono: bool,
    pub(crate) length: usize,
}

impl FrameHeader {
    pub(crate) fn read(header: &[u8]) -> Option<Self> {
        let [sync, flags, rates, mode, ..] = *header else {
            return None;
        };
        if sync != MPEG_SYNC || flags & MPEG_SYNC_HIGH != MPEG_SYNC_HIGH {
            return None;
        }
        let version = match (flags >> 3) & 0b11 {
            0b00 => MpegVersion::TwoAndAHalf,
            0b10 => MpegVersion::Two,
            0b11 => MpegVersion::One,
            _ => return None,
        };
        let layer = match (flags >> 1) & 0b11 {
            0b01 => MpegLayer::Three,
            0b10 => MpegLayer::Two,
            0b11 => MpegLayer::One,
            _ => return None,
        };
        let bitrate_index = rates >> 4;
        if bitrate_index == FREE_BITRATE || bitrate_index == BAD_BITRATE {
            return None;
        }
        let bitrates = match (version, layer) {
            (MpegVersion::One, MpegLayer::One) => &BITRATES_MPEG_ONE_LAYER_ONE,
            (MpegVersion::One, MpegLayer::Two) => &BITRATES_MPEG_ONE_LAYER_TWO,
            (MpegVersion::One, MpegLayer::Three) => &BITRATES_MPEG_ONE_LAYER_THREE,
            (_, MpegLayer::One) => &BITRATES_MPEG_TWO_LAYER_ONE,
            (_, MpegLayer::Two | MpegLayer::Three) => &BITRATES_MPEG_TWO_LAYERS_TWO_AND_THREE,
        };
        let kbps = *bitrates.get(usize::from(bitrate_index))?;
        let rate = match ((rates >> 2) & 0b11, version) {
            (0b00, MpegVersion::One) => 44_100,
            (0b01, MpegVersion::One) => 48_000,
            (0b10, MpegVersion::One) => 32_000,
            (0b00, MpegVersion::Two) => 22_050,
            (0b01, MpegVersion::Two) => 24_000,
            (0b10, MpegVersion::Two) => 16_000,
            (0b00, MpegVersion::TwoAndAHalf) => 11_025,
            (0b01, MpegVersion::TwoAndAHalf) => 12_000,
            (0b10, MpegVersion::TwoAndAHalf) => 8_000,
            _ => return None,
        };
        let mono = mode >> 6 == MONO;
        if layer == MpegLayer::Two {
            let refused = if mono {
                &LAYER_TWO_REFUSED_IN_MONO_KBPS
            } else {
                &LAYER_TWO_REFUSED_BESIDE_MONO_KBPS
            };
            if refused.contains(&kbps) {
                return None;
            }
        }
        let (slots_a_frame, bytes_a_slot) = match (layer, version) {
            (MpegLayer::One, _) => (12, 4),
            (MpegLayer::Two, _) | (MpegLayer::Three, MpegVersion::One) => (144, 1),
            (MpegLayer::Three, _) => (72, 1),
        };
        let padding = u32::from((rates >> 1) & 1);
        let slots = slots_a_frame * kbps * BITS_A_KILOBIT / rate + padding;

        Some(Self {
            stream: Stream {
                version,
                layer,
                rate,
            },
            kbps,
            mono,
            length: usize::try_from(slots * bytes_a_slot).ok()?,
        })
    }

    const fn samples(self) -> u64 {
        match (self.stream.layer, self.stream.version) {
            (MpegLayer::One, _) => 384,
            (MpegLayer::Two, _) | (MpegLayer::Three, MpegVersion::One) => 1_152,
            (MpegLayer::Three, _) => 576,
        }
    }

    const fn xing_at(self) -> Option<u64> {
        let side_info = match (self.stream.layer, self.stream.version, self.mono) {
            (MpegLayer::Three, MpegVersion::One, false) => 32,
            (MpegLayer::Three, MpegVersion::One, true)
            | (MpegLayer::Three, MpegVersion::Two | MpegVersion::TwoAndAHalf, false) => 17,
            (MpegLayer::Three, MpegVersion::Two | MpegVersion::TwoAndAHalf, true) => 9,
            (MpegLayer::One | MpegLayer::Two, _, _) => return None,
        };
        Some(HEADER_BYTES + side_info)
    }

    const fn next(self, at: u64) -> u64 {
        at.saturating_add(self.length as u64)
    }
}

pub(crate) fn counted_frames<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Frames> {
    let start = past_id3(source)?;
    let end = source.seek(SeekFrom::End(0)).ok()?;
    let first = header_at(source, start)?;
    header_at(source, first.next(start)).filter(|second| second.stream == first.stream)?;

    let audio_from = match opening_note(source, start, first) {
        Opening::Counted => return None,
        Opening::Uncounted => first.next(start),
        Opening::Music => start,
    };
    if is_constant(source, audio_from, end, first) {
        return None;
    }
    Some(Frames(walked(source, audio_from, end, first.stream)))
}

enum Opening {
    Counted,
    Uncounted,
    Music,
}

pub(crate) fn encoder_gain<S: Read + Seek + ?Sized>(source: &mut S) -> ReplayGain {
    let Ok(origin) = source.stream_position() else {
        return ReplayGain::default();
    };
    let found = lame_gain(source).unwrap_or_default();
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("a LAME header read could not restore the stream position");
    }
    found
}

fn lame_gain<S: Read + Seek + ?Sized>(source: &mut S) -> Option<ReplayGain> {
    let start = past_id3(source)?;
    let first = header_at(source, start)?;
    let xing_at = start + first.xing_at()?;
    let note = bytes_at::<8, S>(source, xing_at)?;
    let (named, flags) = note.split_at(4);
    if named != XING && named != INFO {
        return None;
    }
    let flags = *flags.last()?;
    let fields: u64 = XING_FIELDS
        .iter()
        .filter(|(flag, _)| flags & flag != 0)
        .map(|(_, bytes)| bytes)
        .sum();
    let lame = bytes_at::<LAME_TAG_BYTES, S>(source, xing_at + XING_NOTE_BYTES + fields)?;

    let peak = u32::from_be_bytes(lame.get(LAME_PEAK_AT..LAME_PEAK_AT + 4)?.try_into().ok()?);
    let mut gain = ReplayGain {
        track_peak: (peak > 0).then(|| peak as f32 / LAME_PEAK_FULL_SCALE),
        ..ReplayGain::default()
    };
    for at in LAME_GAINS_AT {
        let field = u16::from_be_bytes(lame.get(at..at + 2)?.try_into().ok()?);
        match lame_gain_field(field) {
            Some((LAME_TRACK_GAIN, db)) => gain.track_gain = Some(db),
            Some((LAME_ALBUM_GAIN, db)) => gain.album_gain = Some(db),
            Some(_) | None => {}
        }
    }
    Some(gain)
}

fn lame_gain_field(field: u16) -> Option<(u16, Decibels)> {
    let name = field >> 13;
    let originator = (field >> 10) & 0b111;
    if name == 0 || originator == 0 {
        return None;
    }
    let tenths = f32::from(field & LAME_GAIN_TENTHS);
    let signed = if field & LAME_GAIN_NEGATIVE != 0 {
        -tenths
    } else {
        tenths
    };
    Some((name, Decibels::new(signed / TENTHS_A_DECIBEL).ok()?))
}

fn opening_note<S: Read + Seek + ?Sized>(
    source: &mut S,
    start: u64,
    first: FrameHeader,
) -> Opening {
    if bytes_at::<4, S>(source, start + VBRI_AT).is_some_and(|named| &named == VBRI) {
        return Opening::Counted;
    }
    let Some(xing_at) = first.xing_at() else {
        return Opening::Music;
    };
    let Some(note) = bytes_at::<8, S>(source, start + xing_at) else {
        return Opening::Music;
    };
    let (named, flags) = note.split_at(4);
    if named != XING && named != INFO {
        return Opening::Music;
    }
    match flags.last() {
        Some(flags) if flags & XING_NAMES_ITS_FRAMES != 0 => Opening::Counted,
        Some(_) | None => Opening::Uncounted,
    }
}

fn is_constant<S: Read + Seek + ?Sized>(
    source: &mut S,
    from: u64,
    end: u64,
    first: FrameHeader,
) -> bool {
    let mut at = from;
    for _ in 0..OPENING_FRAMES_WEIGHED {
        let Some(header) = header_at(source, at).filter(|header| header.stream == first.stream)
        else {
            break;
        };
        if header.kbps != first.kbps {
            return false;
        }
        at = header.next(at);
    }

    let span = end.saturating_sub(from);
    (1..=SAMPLED_AT).all(|sample| {
        let near = from + span / (SAMPLED_AT + 1) * sample;
        resynced(source, near, end, first.stream, SAMPLE_RESYNCED_WITHIN)
            .is_none_or(|(_, header)| header.kbps == first.kbps)
    })
}

fn walked<S: Read + Seek + ?Sized>(source: &mut S, from: u64, end: u64, stream: Stream) -> u64 {
    let mut at = from;
    let mut counted = 0_u64;
    while at.saturating_add(HEADER_BYTES) <= end {
        let header = match header_at(source, at).filter(|header| header.stream == stream) {
            Some(header) => header,
            None if is_a_trailer(source, at) => break,
            None => match resynced(source, at + 1, end, stream, RESYNCED_WITHIN) {
                Some((found, header)) => {
                    at = found;
                    header
                }
                None => break,
            },
        };
        counted = counted.saturating_add(header.samples());
        at = header.next(at);
    }
    counted
}

fn resynced<S: Read + Seek + ?Sized>(
    source: &mut S,
    from: u64,
    end: u64,
    stream: Stream,
    within: u64,
) -> Option<(u64, FrameHeader)> {
    let searched = usize::try_from(within.min(end.saturating_sub(from))).ok()?;
    source.seek(SeekFrom::Start(from)).ok()?;
    let mut held = vec![0_u8; searched];
    source.read_exact(&mut held).ok()?;

    (0..searched).find_map(|offset| {
        let header = FrameHeader::read(held.get(offset..)?)?;
        if header.stream != stream {
            return None;
        }
        let at = from + offset as u64;
        header_at(source, header.next(at))
            .filter(|next| next.stream == stream)
            .map(|_| (at, header))
    })
}

fn is_a_trailer<S: Read + Seek + ?Sized>(source: &mut S, at: u64) -> bool {
    let Some(held) = bytes_at::<LONGEST_TRAILER, S>(source, at) else {
        return true;
    };
    TRAILERS.iter().any(|trailer| held.starts_with(trailer))
}

fn header_at<S: Read + Seek + ?Sized>(source: &mut S, at: u64) -> Option<FrameHeader> {
    bytes_at::<4, S>(source, at).and_then(|held| FrameHeader::read(&held))
}

fn bytes_at<const N: usize, S: Read + Seek + ?Sized>(source: &mut S, at: u64) -> Option<[u8; N]> {
    source.seek(SeekFrom::Start(at)).ok()?;
    read_exact::<N, S>(source)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const AT_128: [u8; 4] = [0xff, 0xfb, 0x90, 0x00];
    const AT_128_LENGTH: usize = 417;
    const AT_320: [u8; 4] = [0xff, 0xfb, 0xe0, 0x00];
    const AT_320_LENGTH: usize = 1_044;
    const AT_32: [u8; 4] = [0xff, 0xfb, 0x10, 0x00];
    const AT_32_LENGTH: usize = 104;

    fn frames(header: [u8; 4], length: usize, count: usize) -> Vec<u8> {
        let mut held = Vec::new();
        for _ in 0..count {
            held.extend_from_slice(&header);
            held.resize(held.len() + length - header.len(), 0);
        }
        held
    }

    fn counted(file: Vec<u8>) -> Option<Frames> {
        counted_frames(&mut Cursor::new(file))
    }

    #[test]
    fn a_variable_rate_stream_with_no_header_naming_its_length_is_counted_frame_by_frame() {
        let mut file = frames(AT_32, AT_32_LENGTH, 40);
        file.extend(frames(AT_320, AT_320_LENGTH, 200));
        file.extend(frames(AT_128, AT_128_LENGTH, 100));
        file.extend_from_slice(b"TAG");
        file.resize(file.len() + 125, 0);

        assert_eq!(counted(file), Some(Frames(340 * 1_152)));
    }

    #[test]
    fn junk_between_frames_is_stepped_over_and_counts_nothing() {
        let mut file = frames(AT_32, AT_32_LENGTH, 40);
        file.extend(frames(AT_320, AT_320_LENGTH, 20));
        file.extend_from_slice(&[0x12; 300]);
        file.extend(frames(AT_128, AT_128_LENGTH, 20));

        assert_eq!(counted(file), Some(Frames(80 * 1_152)));
    }

    #[test]
    fn a_constant_rate_stream_is_left_to_the_readers_arithmetic() {
        assert_eq!(counted(frames(AT_128, AT_128_LENGTH, 400)), None);
    }

    #[test]
    fn a_header_naming_its_frames_is_left_to_the_reader_and_one_naming_none_is_walked_past() {
        let mut named = frames(AT_32, AT_32_LENGTH, 1);
        named[36..40].copy_from_slice(XING);
        named[43] = XING_NAMES_ITS_FRAMES;
        named.extend(frames(AT_320, AT_320_LENGTH, 50));
        assert_eq!(counted(named.clone()), None);

        let mut unnamed = named;
        unnamed[43] = 0;
        unnamed.extend(frames(AT_32, AT_32_LENGTH, 40));
        assert_eq!(
            counted(unnamed),
            Some(Frames(90 * 1_152)),
            "the note's own frame was counted as music"
        );
    }

    #[test]
    fn the_gains_a_lame_header_names_are_read_and_an_unset_one_is_not() {
        let mut file = frames(AT_128, AT_128_LENGTH, 3);
        file[36..40].copy_from_slice(INFO);
        file[43] = XING_NAMES_ITS_FRAMES;
        let lame = 36 + 8 + 4;
        file[lame..lame + 9].copy_from_slice(b"LAME3.100");
        file[lame + 11..lame + 15].copy_from_slice(&4_194_304_u32.to_be_bytes());
        let radio_by_the_model_minus_6_5_db: u16 = (0b001 << 13) | (0b011 << 10) | 0x200 | 65;
        file[lame + 15..lame + 17].copy_from_slice(&radio_by_the_model_minus_6_5_db.to_be_bytes());

        let gain = encoder_gain(&mut Cursor::new(file.clone()));
        assert_eq!(gain.track_gain.map(Decibels::get), Some(-6.5));
        assert_eq!(gain.track_peak, Some(0.5));
        assert_eq!(gain.album_gain, None, "an unset audiophile gain was read");

        file[lame + 15..lame + 17].copy_from_slice(&0_u16.to_be_bytes());
        assert_eq!(encoder_gain(&mut Cursor::new(file)).track_gain, None);
    }

    #[test]
    fn what_is_not_mpeg_audio_is_not_counted() {
        assert_eq!(counted(b"fLaC\0\0\0\x22".to_vec()), None);
        assert_eq!(counted(frames(AT_128, AT_128_LENGTH, 1)), None);
    }
}
