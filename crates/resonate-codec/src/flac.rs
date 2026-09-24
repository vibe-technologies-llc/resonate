use std::io::{Read, Seek, SeekFrom};

use resonate_core::Frames;

use crate::{
    TagSet,
    cue::{CueFile, CueStart, CueTrack, CueTrackKind},
    prescan::read_exact,
};

const MAGIC: [u8; 4] = *b"fLaC";
const BLOCK_HEADER_BYTES: usize = 4;
const LAST_BLOCK: u8 = 0x80;
const BLOCK_KIND: u8 = 0x7F;
const CUESHEET: u8 = 5;

const MAX_BLOCKS: usize = 1_024;
const MAX_CUESHEET_BYTES: u64 = 1 << 20;

const CATALOG_BYTES: usize = 128;
const LEAD_IN_BYTES: usize = 8;
const IS_CD_DA_AT: usize = CATALOG_BYTES + LEAD_IN_BYTES;
const IS_CD_DA: u8 = 0x80;
const RESERVED_BYTES: usize = 259;
const TRACK_COUNT_AT: usize = IS_CD_DA_AT + RESERVED_BYTES;
const SHEET_HEADER_BYTES: usize = TRACK_COUNT_AT + 1;

const TRACK_BYTES: usize = 36;
const TRACK_NUMBER_AT: usize = 8;
const TRACK_FLAGS_AT: usize = 21;
const NOT_AUDIO: u8 = 0x80;
const TRACK_INDEX_COUNT_AT: usize = 35;

const INDEX_BYTES: usize = 12;
const INDEX_NUMBER_AT: usize = 8;
const FIRST_INDEX: u8 = 1;

const CD_DA_LEAD_OUT: u8 = 170;
const LEAD_OUT: u8 = 255;

const FRAME_SYNC: u8 = 0xFF;
const FRAME_SYNC_TAIL: u8 = 0xF8;
const FRAME_SYNC_TAIL_MASK: u8 = 0xFE;
const BLOCK_SIZE_SHIFT: u32 = 4;
const CODED_NUMBER_AT: usize = 4;
const LONGEST_CODED_NUMBER: u32 = 7;
const SHORTEST_BLOCK: u64 = 192;
const SMALL_BLOCK_UNIT: u64 = 576;
const LARGE_BLOCK_UNIT: u64 = 256;
pub(crate) const FRAME_HEADER_BYTES_AT_MOST: usize = 16;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Flac {
    pub(crate) cue: Option<CueFile>,
}

pub(crate) fn samples_in_a_frame(header: &[u8]) -> Option<u64> {
    let [sync, tail, sizing, ..] = *header else {
        return None;
    };
    if sync != FRAME_SYNC || tail & FRAME_SYNC_TAIL_MASK != FRAME_SYNC_TAIL {
        return None;
    }

    let code = sizing >> BLOCK_SIZE_SHIFT;
    let spelled_at = || {
        let lead = *header.get(CODED_NUMBER_AT)?;
        let width = match lead.leading_ones() {
            0 => 1,
            1 => return None,
            width if width <= LONGEST_CODED_NUMBER => width,
            _ => return None,
        };
        CODED_NUMBER_AT.checked_add(usize::try_from(width).ok()?)
    };
    match code {
        0 => None,
        1 => Some(SHORTEST_BLOCK),
        2..=5 => Some(SMALL_BLOCK_UNIT << (code - 2)),
        6 => {
            let at = spelled_at()?;
            Some(u64::from(*header.get(at)?) + 1)
        }
        7 => {
            let at = spelled_at()?;
            let spelled = header.get(at..at.checked_add(2)?)?;
            Some(u64::from(u16::from_be_bytes(spelled.try_into().ok()?)) + 1)
        }
        _ => Some(LARGE_BLOCK_UNIT << (code - 8)),
    }
}

pub(crate) fn read<S: Read + Seek + ?Sized>(source: &mut S) -> Flac {
    let Ok(origin) = source.stream_position() else {
        return Flac::default();
    };
    let found = scan(source).unwrap_or_default();
    if source.seek(SeekFrom::Start(origin)).is_err() {
        tracing::debug!("a FLAC metadata block scan could not restore the stream position");
    }
    found
}

fn scan<S: Read + Seek + ?Sized>(source: &mut S) -> Option<Flac> {
    if read_exact::<4, S>(source)? != MAGIC {
        return None;
    }

    let mut found = Flac::default();
    for _ in 0..MAX_BLOCKS {
        let Some(header) = read_exact::<BLOCK_HEADER_BYTES, S>(source) else {
            break;
        };
        let declared = declared_bytes(&header);
        let Ok(body) = source.stream_position() else {
            break;
        };

        if kind(&header) == CUESHEET && found.cue.is_none() && declared <= MAX_CUESHEET_BYTES {
            found.cue = read_cuesheet(source, declared);
        }

        let Some(next) = body.checked_add(declared) else {
            break;
        };
        if source.seek(SeekFrom::Start(next)).is_err() {
            break;
        }
        if is_last(&header) {
            break;
        }
    }

    Some(found)
}

const fn kind(header: &[u8; BLOCK_HEADER_BYTES]) -> u8 {
    header[0] & BLOCK_KIND
}

const fn is_last(header: &[u8; BLOCK_HEADER_BYTES]) -> bool {
    header[0] & LAST_BLOCK != 0
}

fn declared_bytes(header: &[u8; BLOCK_HEADER_BYTES]) -> u64 {
    u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]))
}

fn read_cuesheet<S: Read + ?Sized>(source: &mut S, declared: u64) -> Option<CueFile> {
    let mut block = Vec::new();
    if source.take(declared).read_to_end(&mut block).is_err() {
        return None;
    }

    let lead_out = match block.get(IS_CD_DA_AT)? & IS_CD_DA != 0 {
        true => CD_DA_LEAD_OUT,
        false => LEAD_OUT,
    };
    let count = usize::from(*block.get(TRACK_COUNT_AT)?);

    let mut tracks = Vec::with_capacity(count.min(block.len() / TRACK_BYTES));
    let mut at = SHEET_HEADER_BYTES;
    for _ in 0..count {
        let record = block.get(at..at.checked_add(TRACK_BYTES)?)?;
        let indexes = usize::from(record[TRACK_INDEX_COUNT_AT]);
        let past = at
            .checked_add(TRACK_BYTES)?
            .checked_add(indexes.checked_mul(INDEX_BYTES)?)?;
        let points = block.get(at + TRACK_BYTES..past)?;

        tracks.push(cue_track(record, points, lead_out));
        at = past;
    }

    Some(CueFile {
        named: String::new(),
        tracks,
    })
}

fn cue_track(record: &[u8], points: &[u8], lead_out: u8) -> CueTrack {
    let number = record[TRACK_NUMBER_AT];
    let offset = eight_bytes(record);
    let kind = match record[TRACK_FLAGS_AT] & NOT_AUDIO != 0 || number == lead_out {
        true => CueTrackKind::Data,
        false => CueTrackKind::Audio,
    };

    CueTrack {
        number: u32::from(number),
        kind,
        start: CueStart::Sampled(Frames(offset.saturating_add(music_at(points)))),
        pregap: None,
        tags: TagSet::default(),
    }
}

fn music_at(points: &[u8]) -> u64 {
    points
        .as_chunks::<INDEX_BYTES>()
        .0
        .iter()
        .find(|point| point[INDEX_NUMBER_AT] == FIRST_INDEX)
        .map_or(0, |point| eight_bytes(point))
}

fn eight_bytes(from: &[u8]) -> u64 {
    let mut held = [0_u8; 8];
    held.copy_from_slice(&from[..8]);
    u64::from_be_bytes(held)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    const STREAMINFO: u8 = 0;
    const PADDING: u8 = 1;

    fn frame_header(sizing: u8, coded_number: &[u8], spelled: &[u8]) -> Vec<u8> {
        let mut header = vec![
            FRAME_SYNC,
            FRAME_SYNC_TAIL,
            sizing << BLOCK_SIZE_SHIFT | 0x9,
            0x18,
        ];
        header.extend_from_slice(coded_number);
        header.extend_from_slice(spelled);
        header.push(0xAB);
        header
    }

    #[test]
    fn a_frame_says_how_many_samples_it_holds_by_its_block_size_code() {
        assert_eq!(samples_in_a_frame(&frame_header(1, &[0], &[])), Some(192));
        assert_eq!(samples_in_a_frame(&frame_header(3, &[7], &[])), Some(1_152));
        assert_eq!(
            samples_in_a_frame(&frame_header(12, &[7], &[])),
            Some(4_096)
        );
        assert_eq!(
            samples_in_a_frame(&frame_header(15, &[7], &[])),
            Some(32_768)
        );
    }

    #[test]
    fn a_block_size_spelled_after_the_frame_number_is_read_past_it_however_wide_it_is() {
        let one_byte_number = [0x05];
        let three_byte_number = [0xE1, 0x80, 0x80];

        assert_eq!(
            samples_in_a_frame(&frame_header(6, &one_byte_number, &[99])),
            Some(100)
        );
        assert_eq!(
            samples_in_a_frame(&frame_header(7, &three_byte_number, &[0x0A, 0xBF])),
            Some(2_752)
        );
    }

    #[test]
    fn a_header_that_is_not_a_frame_or_is_cut_short_says_nothing() {
        let continuation_lead = [0x80];

        assert_eq!(samples_in_a_frame(&[0xFF, 0xF0, 0x19, 0x18, 0]), None);
        assert_eq!(samples_in_a_frame(&frame_header(0, &[0], &[])), None);
        assert_eq!(
            samples_in_a_frame(&frame_header(7, &[0], &[0x0A])[..6]),
            None
        );
        assert_eq!(
            samples_in_a_frame(&frame_header(6, &continuation_lead, &[99])),
            None
        );
        assert_eq!(samples_in_a_frame(&[FRAME_SYNC, FRAME_SYNC_TAIL]), None);
    }

    fn block(into: &mut Vec<u8>, kind: u8, payload: &[u8], last: bool) {
        let head = kind | if last { LAST_BLOCK } else { 0 };
        into.push(head);
        into.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
        into.extend_from_slice(payload);
    }

    fn flac(blocks: &[(u8, Vec<u8>)]) -> Vec<u8> {
        let mut file = MAGIC.to_vec();
        for (at, (kind, payload)) in blocks.iter().enumerate() {
            block(&mut file, *kind, payload, at + 1 == blocks.len());
        }
        file
    }

    struct Point {
        offset: u64,
        number: u8,
    }

    struct Track {
        offset: u64,
        number: u8,
        audio: bool,
        points: Vec<Point>,
    }

    impl Track {
        fn audio(number: u8, offset: u64) -> Self {
            Self {
                offset,
                number,
                audio: true,
                points: vec![Point {
                    offset: 0,
                    number: FIRST_INDEX,
                }],
            }
        }

        fn lead_out(offset: u64) -> Self {
            Self {
                offset,
                number: CD_DA_LEAD_OUT,
                audio: true,
                points: Vec::new(),
            }
        }

        fn written(&self, into: &mut Vec<u8>) {
            into.extend_from_slice(&self.offset.to_be_bytes());
            into.push(self.number);
            into.extend_from_slice(&[0; 12]);
            into.push(if self.audio { 0 } else { NOT_AUDIO });
            into.extend_from_slice(&[0; 13]);
            into.push(self.points.len() as u8);
            for point in &self.points {
                into.extend_from_slice(&point.offset.to_be_bytes());
                into.push(point.number);
                into.extend_from_slice(&[0; 3]);
            }
        }
    }

    fn cuesheet(tracks: &[Track], declared: Option<u8>) -> Vec<u8> {
        let mut payload = vec![0_u8; SHEET_HEADER_BYTES];
        payload[IS_CD_DA_AT] = IS_CD_DA;
        payload[TRACK_COUNT_AT] = declared.unwrap_or(tracks.len() as u8);
        for track in tracks {
            track.written(&mut payload);
        }
        payload
    }

    fn meddle() -> Vec<Track> {
        vec![
            Track::audio(1, 0),
            Track::audio(2, 15_773_100),
            Track::audio(3, 56_007_000),
            Track::lead_out(100_000_000),
        ]
    }

    fn found(file: Vec<u8>) -> Flac {
        read(&mut Cursor::new(file))
    }

    #[test]
    fn a_sheet_a_ripper_embedded_names_every_track_at_the_sample_it_starts_on() {
        let file = flac(&[
            (STREAMINFO, vec![0; 34]),
            (CUESHEET, cuesheet(&meddle(), None)),
            (PADDING, vec![0; 64]),
        ]);

        let cut = found(file).cue.expect("an embedded sheet");

        assert_eq!(cut.tracks.len(), 4);
        assert_eq!(cut.audio_tracks().count(), 3);
        assert_eq!(
            cut.tracks
                .iter()
                .map(|track| track.start)
                .collect::<Vec<_>>(),
            vec![
                CueStart::Sampled(Frames::ZERO),
                CueStart::Sampled(Frames(15_773_100)),
                CueStart::Sampled(Frames(56_007_000)),
                CueStart::Sampled(Frames(100_000_000)),
            ]
        );
        assert_eq!(cut.tracks[3].kind, CueTrackKind::Data);
        assert_eq!(cut.tracks[0].tags, TagSet::default());
    }

    #[test]
    fn a_track_whose_music_starts_after_its_pregap_begins_at_the_first_index() {
        let mut tracks = meddle();
        tracks[1].points.insert(
            0,
            Point {
                offset: 0,
                number: 0,
            },
        );
        tracks[1].points[1].offset = 88_200;

        let cut = found(flac(&[(CUESHEET, cuesheet(&tracks, None))]))
            .cue
            .expect("an embedded sheet");

        assert_eq!(
            cut.tracks[1].start,
            CueStart::Sampled(Frames(15_773_100 + 88_200))
        );
    }

    #[test]
    fn a_sheet_holding_nothing_but_the_lead_out_names_no_audio_at_all() {
        let cut = found(flac(&[(
            CUESHEET,
            cuesheet(&[Track::lead_out(44_100)], None),
        )]))
        .cue
        .expect("an embedded sheet");

        assert_eq!(cut.tracks.len(), 1);
        assert_eq!(cut.audio_tracks().count(), 0);
    }

    #[test]
    fn a_block_cut_short_of_what_it_declares_names_nothing() {
        let payload = cuesheet(&meddle(), None);
        let mut file = MAGIC.to_vec();
        file.push(CUESHEET | LAST_BLOCK);
        file.extend_from_slice(&(payload.len() as u32).to_be_bytes()[1..]);
        file.extend_from_slice(&payload[..payload.len() - TRACK_BYTES]);

        assert_eq!(found(file).cue, None);
    }

    #[test]
    fn a_block_naming_more_tracks_than_it_holds_names_none_of_them() {
        let file = flac(&[(CUESHEET, cuesheet(&meddle(), Some(200)))]);

        assert_eq!(found(file).cue, None);
    }

    #[test]
    fn a_length_that_would_run_past_the_end_of_the_count_is_not_followed() {
        let mut file = MAGIC.to_vec();
        file.push(CUESHEET);
        file.extend_from_slice(&[0xFF, 0xFF, 0xFF]);
        file.extend_from_slice(&cuesheet(&meddle(), None));

        assert_eq!(found(file).cue, None);

        let mut short = MAGIC.to_vec();
        short.push(PADDING);
        short.extend_from_slice(&[0xFF, 0xFF, 0xFF]);
        short.push(CUESHEET | LAST_BLOCK);

        assert_eq!(found(short).cue, None);
    }

    #[test]
    fn a_file_that_is_not_a_flac_costs_the_magic_and_nothing_more() {
        let mut source = Cursor::new(b"RIFF\0\0\0\0WAVE".to_vec());

        assert_eq!(read(&mut source), Flac::default());
        assert_eq!(source.position(), 0);
        assert_eq!(found(Vec::new()), Flac::default());
    }

    #[test]
    fn a_flac_carrying_no_cuesheet_block_names_no_sheet() {
        let file = flac(&[(STREAMINFO, vec![0; 34]), (PADDING, vec![0; 1_024])]);

        assert_eq!(found(file).cue, None);
    }

    #[test]
    fn the_scan_leaves_the_stream_where_it_found_it() {
        let mut source = Cursor::new(flac(&[(CUESHEET, cuesheet(&meddle(), None))]));

        assert!(read(&mut source).cue.is_some());
        assert_eq!(source.position(), 0);
    }

    #[test]
    fn a_block_claiming_more_bytes_than_a_sheet_can_hold_is_refused_rather_than_read() {
        let mut file = MAGIC.to_vec();
        file.push(CUESHEET | LAST_BLOCK);
        let declared = MAX_CUESHEET_BYTES + 1;
        file.extend_from_slice(&(declared as u32).to_be_bytes()[1..]);
        file.extend_from_slice(&cuesheet(&meddle(), None));

        assert_eq!(found(file).cue, None);
    }
}
