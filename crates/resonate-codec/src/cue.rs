use std::{fmt, io::Read};

use resonate_core::{Decibels, FrameSpan, Frames, MediaLocation, SampleRate};

use crate::{
    Error, MediaInfo, Result, TagSet,
    source::Sources,
    tags::Uppercased,
    text::{TextEncoding, decoded, encoded},
};

const SECTORS_PER_SECOND: u64 = 75;
const MOST_TRACKS: usize = 999;
const LARGEST_CUE_SHEET: u64 = 1 << 20;
const SHEET_EXTENSIONS: [&str; 2] = ["cue", "CUE"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CueStamp {
    minutes: u32,
    seconds: u8,
    sectors: u8,
}

impl CueStamp {
    pub const fn new(minutes: u32, seconds: u8, sectors: u8) -> Self {
        Self {
            minutes,
            seconds,
            sectors,
        }
    }

    pub const fn total_sectors(self) -> u64 {
        (self.minutes as u64 * 60 + self.seconds as u64) * SECTORS_PER_SECOND + self.sectors as u64
    }

    pub fn at(self, rate: SampleRate) -> Frames {
        let sectors = u128::from(self.total_sectors());
        let hz = u128::from(rate.hz());
        Frames(u64::try_from(sectors * hz / u128::from(SECTORS_PER_SECOND)).unwrap_or(u64::MAX))
    }

    fn read(text: &str) -> Option<Self> {
        let mut fields = text.split(':');
        let minutes = fields.next()?.trim().parse().ok()?;
        let seconds = fields.next()?.trim().parse().ok()?;
        let sectors = fields.next()?.trim().parse().ok()?;
        if fields.next().is_some() || seconds >= 60 || u64::from(sectors) >= SECTORS_PER_SECOND {
            return None;
        }

        Some(Self::new(minutes, seconds, sectors))
    }
}

impl fmt::Display for CueStamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:02}:{:02}:{:02}",
            self.minutes, self.seconds, self.sectors
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CueStart {
    Written(CueStamp),
    Sampled(Frames),
}

impl CueStart {
    pub fn at(self, rate: SampleRate) -> Frames {
        match self {
            Self::Written(stamp) => stamp.at(rate),
            Self::Sampled(frames) => frames,
        }
    }
}

impl Default for CueStart {
    fn default() -> Self {
        Self::Written(CueStamp::default())
    }
}

impl fmt::Display for CueStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Written(stamp) => stamp.fmt(f),
            Self::Sampled(frames) => write!(f, "{frames}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CueTrackKind {
    #[default]
    Audio,
    Data,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueTrack {
    pub number: u32,
    pub kind: CueTrackKind,
    pub start: CueStart,
    pub pregap: Option<CueStamp>,
    pub tags: TagSet,
}

impl CueTrack {
    pub fn titled(&self) -> TagSet {
        let mut tags = self.tags.clone();
        if tags.title.is_none() {
            tags.title = Some(format!("Track {}", self.number));
        }
        tags
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueFile {
    pub named: String,
    pub tracks: Vec<CueTrack>,
}

impl CueFile {
    pub fn span_of(
        &self,
        index: usize,
        rate: SampleRate,
        whole: Option<Frames>,
    ) -> Option<FrameSpan> {
        let track = self.tracks.get(index)?;
        let start = track.start.at(rate);

        let span = match self.tracks.get(index + 1) {
            Some(next) => FrameSpan::between(start, next.start.at(rate)),
            None => FrameSpan::starting(start),
        };
        Some(match whole {
            Some(whole) => span.within(whole),
            None => span,
        })
    }

    pub fn cut_at(&self, start: Frames, rate: SampleRate) -> Option<&CueTrack> {
        self.audio_tracks()
            .map(|(_, track)| track)
            .find(|track| track.start.at(rate) == start)
    }

    pub fn audio_tracks(&self) -> impl Iterator<Item = (usize, &CueTrack)> {
        self.tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| track.kind == CueTrackKind::Audio)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CueSheet {
    pub files: Vec<CueFile>,
    pub encoding: TextEncoding,
}

impl CueSheet {
    pub fn claims(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|file| file.named.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.files.iter().all(|file| file.tracks.is_empty())
    }
}

pub fn read(bytes: &[u8]) -> CueSheet {
    let (text, encoding) = decoded(bytes);
    let mut sheet = Reading::default();

    for line in text.lines() {
        sheet.line(line);
    }
    sheet.finish(encoding)
}

pub fn read_media(sources: &Sources, location: &MediaLocation) -> Result<CueSheet> {
    let mut media = sources.open(location)?;
    let mut held = Vec::new();

    media
        .stream
        .by_ref()
        .take(LARGEST_CUE_SHEET.saturating_add(1))
        .read_to_end(&mut held)
        .map_err(|source| Error::Io {
            location: location.clone(),
            source,
        })?;

    if held.len() as u64 > LARGEST_CUE_SHEET {
        return Err(Error::SheetTooLarge {
            location: location.clone(),
            limit: LARGEST_CUE_SHEET,
        });
    }
    Ok(read(&held))
}

pub(crate) fn cut_for(
    sources: &Sources,
    location: &MediaLocation,
    info: &MediaInfo,
) -> Option<CueFile> {
    match info.cue.clone() {
        Some(embedded) => Some(embedded),
        None => beside(sources, location),
    }
}

fn beside(sources: &Sources, location: &MediaLocation) -> Option<CueFile> {
    let path = location.as_path()?;
    let named = path.file_name()?.to_str()?;

    SHEET_EXTENSIONS.iter().find_map(|extension| {
        let sheet = MediaLocation::local(path.with_extension(extension));
        let held = read_media(sources, &sheet).ok()?;
        cut_named(held, named)
    })
}

fn cut_named(sheet: CueSheet, named: &str) -> Option<CueFile> {
    let mut cut: Vec<CueFile> = sheet
        .files
        .into_iter()
        .filter(|file| file.audio_tracks().next().is_some())
        .collect();

    if cut.len() == 1 {
        return cut.pop();
    }
    cut.into_iter().find(|file| file.named == named)
}

pub fn renamed(sheet: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    let held = read(sheet);
    if held.claims().filter(|named| *named == from).count() != 1 {
        return None;
    }

    let named = encoded(from, held.encoding)?;
    let naming = encoded(to, held.encoding)?;
    let at = the_one_run_of(sheet, &named)?;

    let mut written = Vec::with_capacity(sheet.len() + naming.len() - named.len());
    written.extend_from_slice(&sheet[..at]);
    written.extend_from_slice(&naming);
    written.extend_from_slice(&sheet[at + named.len()..]);
    Some(written)
}

fn the_one_run_of(sheet: &[u8], named: &[u8]) -> Option<usize> {
    let last = sheet.len().checked_sub(named.len())?;
    let mut found = None;
    for at in 0..=last {
        if &sheet[at..at + named.len()] != named {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(at);
    }

    found
}

#[derive(Default)]
struct Reading {
    files: Vec<CueFile>,
    album: TagSet,
    open: Option<CueTrack>,
    indexed: Indexed,
    pregap: Option<CueStamp>,
    dropped: bool,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Indexed {
    #[default]
    Not,
    Provisionally,
    AtTheFirst,
}

impl Reading {
    fn line(&mut self, line: &str) {
        let line = line.trim();
        let Some((command, rest)) = split_command(line) else {
            return;
        };

        let Some(command) = Uppercased::of(command) else {
            return;
        };
        match command.as_str() {
            "FILE" => self.file(rest),
            "TRACK" => self.track(rest),
            "INDEX" => self.index(rest),
            "PREGAP" => self.pregap = CueStamp::read(rest.trim()),
            "TITLE" => self.named(rest, Named::Title),
            "PERFORMER" => self.named(rest, Named::Performer),
            "SONGWRITER" => self.named(rest, Named::Songwriter),
            "ISRC" => self.named(rest, Named::Isrc),
            "CATALOG" => self.named(rest, Named::Catalog),
            "REM" => self.remark(rest),
            _ => {}
        }
    }

    fn file(&mut self, rest: &str) {
        let named = quoted(strip_file_type(rest));
        let crossing = match self.indexed {
            Indexed::AtTheFirst => None,
            Indexed::Not | Indexed::Provisionally if named.is_empty() => None,
            Indexed::Not | Indexed::Provisionally => self.open.take(),
        };
        self.close();
        if named.is_empty() {
            return;
        }
        self.files.push(CueFile {
            named,
            tracks: Vec::new(),
        });

        if let Some(track) = crossing {
            self.indexed = Indexed::Not;
            self.open = Some(CueTrack {
                start: CueStart::default(),
                ..track
            });
        }
    }

    fn track(&mut self, rest: &str) {
        self.close();
        let mut fields = rest.split_whitespace();
        let Some(number) = fields.next().and_then(|held| held.parse().ok()) else {
            return;
        };
        let kind = match fields.next() {
            Some(named) if named.eq_ignore_ascii_case("AUDIO") => CueTrackKind::Audio,
            Some(_) => CueTrackKind::Data,
            None => CueTrackKind::Audio,
        };

        self.indexed = Indexed::Not;
        self.open = Some(CueTrack {
            number,
            kind,
            start: CueStart::default(),
            pregap: None,
            tags: TagSet::default(),
        });
    }

    fn index(&mut self, rest: &str) {
        let Some(track) = self.open.as_mut() else {
            return;
        };
        let mut fields = rest.split_whitespace();
        let Some(number) = fields.next().and_then(|held| held.parse::<u32>().ok()) else {
            return;
        };
        let Some(stamp) = fields.next().and_then(CueStamp::read) else {
            return;
        };

        self.indexed = match (number, self.indexed) {
            (1, _) => Indexed::AtTheFirst,
            (_, Indexed::Not) => Indexed::Provisionally,
            _ => return,
        };
        track.start = CueStart::Written(stamp);
    }

    fn named(&mut self, rest: &str, field: Named) {
        let value = quoted(rest);
        if value.is_empty() {
            return;
        }

        let album = self.open.is_none();
        let tags = match self.open.as_mut() {
            Some(track) => &mut track.tags,
            None => &mut self.album,
        };

        match field {
            Named::Title if album => tags.album = Some(value),
            Named::Title => tags.title = Some(value),
            Named::Performer if album => tags.album_artist = Some(value),
            Named::Performer => tags.artist = Some(value),
            Named::Songwriter => tags.credits.composer = Some(value),
            Named::Isrc => tags.isrc = Some(value),
            Named::Catalog if album => {
                tags.musicbrainz_album_id = None;
                tags.barcode = Some(value);
            }
            Named::Catalog => {}
        }
    }

    fn remark(&mut self, rest: &str) {
        let Some((named, value)) = split_command(rest) else {
            return;
        };
        let value = quoted(value);
        if value.is_empty() {
            return;
        }

        let tags = match self.open.as_mut() {
            Some(track) => &mut track.tags,
            None => &mut self.album,
        };

        let Some(named) = Uppercased::of(named) else {
            return;
        };
        match named.as_str() {
            "DATE" => tags.date = Some(value),
            "GENRE" => tags.genre = Some(value),
            "COMMENT" => tags.comment = Some(value),
            "REPLAYGAIN_TRACK_GAIN" => tags.replay_gain.track_gain = decibels(&value),
            "REPLAYGAIN_TRACK_PEAK" => tags.replay_gain.track_peak = peak(&value),
            "REPLAYGAIN_ALBUM_GAIN" => tags.replay_gain.album_gain = decibels(&value),
            "REPLAYGAIN_ALBUM_PEAK" => tags.replay_gain.album_peak = peak(&value),
            _ => {}
        }
    }

    fn close(&mut self) {
        let Some(mut track) = self.open.take() else {
            return;
        };
        track.pregap = self.pregap.take();

        let Some(file) = self.files.last_mut() else {
            return;
        };
        if file.tracks.len() >= MOST_TRACKS {
            if !self.dropped {
                tracing::warn!(
                    limit = MOST_TRACKS,
                    "a cue sheet names more tracks than one is read as holding"
                );
                self.dropped = true;
            }
            return;
        }
        file.tracks.push(track);
    }

    fn finish(mut self, encoding: TextEncoding) -> CueSheet {
        self.close();

        let album = self.album;
        let mut files = self.files;
        for file in &mut files {
            let total = file.tracks.len() as u32;
            for track in &mut file.tracks {
                let number = track.number;
                settle(&mut track.tags, &album, total);
                track.tags.track_number = Some(number);
            }
        }

        CueSheet { files, encoding }
    }
}

#[derive(Clone, Copy)]
enum Named {
    Title,
    Performer,
    Songwriter,
    Isrc,
    Catalog,
}

fn settle(tags: &mut TagSet, album: &TagSet, total: u32) {
    tags.album = album.album.clone();
    tags.album_artist = album.album_artist.clone();
    tags.track_total = Some(total);

    if tags.artist.is_none() {
        tags.artist = album.album_artist.clone();
    }
    if tags.date.is_none() {
        tags.date = album.date.clone();
    }
    if tags.genre.is_none() {
        tags.genre = album.genre.clone();
    }
    if tags.replay_gain.album_gain.is_none() {
        tags.replay_gain.album_gain = album.replay_gain.album_gain;
    }
    if tags.replay_gain.album_peak.is_none() {
        tags.replay_gain.album_peak = album.replay_gain.album_peak;
    }
}

fn split_command(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    match line.split_once(char::is_whitespace) {
        Some((command, rest)) => Some((command, rest.trim_start())),
        None => Some((line, "")),
    }
}

fn strip_file_type(rest: &str) -> &str {
    let rest = rest.trim();
    if let Some(end) = closing_quote(rest) {
        return rest.get(..=end).unwrap_or(rest);
    }
    match rest.rsplit_once(char::is_whitespace) {
        Some((named, _)) => named.trim_end(),
        None => rest,
    }
}

fn closing_quote(rest: &str) -> Option<usize> {
    if !rest.starts_with('"') {
        return None;
    }
    rest.char_indices()
        .skip(1)
        .find(|(_, held)| *held == '"')
        .map(|(at, _)| at)
}

fn quoted(value: &str) -> String {
    let value = value.trim();
    let Some(rest) = value.strip_prefix('"') else {
        return value.to_owned();
    };
    match rest.split_once('"') {
        Some((held, _)) => held.to_owned(),
        None => rest.to_owned(),
    }
}

fn decibels(value: &str) -> Option<Decibels> {
    let text = value.trim();
    let text = text
        .strip_suffix("dB")
        .or(text.strip_suffix("DB"))
        .unwrap_or(text);
    Decibels::new(text.trim().parse().ok()?).ok()
}

fn peak(value: &str) -> Option<f32> {
    let held: f32 = value.trim().parse().ok()?;
    (held.is_finite() && held >= 0.0).then_some(held)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEDDLE: &str = r#"REM GENRE "Progressive Rock"
REM DATE 1971
PERFORMER "Pink Floyd"
TITLE "Meddle"
FILE "Meddle.flac" WAVE
  TRACK 01 AUDIO
    TITLE "One of These Days"
    PERFORMER "Pink Floyd"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Pillow of Winds"
    SONGWRITER "Gilmour"
    INDEX 00 05:57:25
    INDEX 01 05:57:50
  TRACK 06 AUDIO
    TITLE "Echoes"
    REM REPLAYGAIN_TRACK_GAIN -7.06 dB
    INDEX 01 21:10:00
"#;

    fn meddle() -> CueFile {
        let sheet = read(MEDDLE.as_bytes());
        assert_eq!(sheet.files.len(), 1);
        sheet.files.into_iter().next().expect("one file")
    }

    #[test]
    fn a_sheet_names_the_file_its_tracks_are_cut_from() {
        let sheet = read(MEDDLE.as_bytes());

        assert_eq!(sheet.claims().collect::<Vec<_>>(), vec!["Meddle.flac"]);
        assert_eq!(sheet.encoding, TextEncoding::Utf8);
    }

    #[test]
    fn a_track_takes_its_own_tags_and_the_albums_around_them() {
        let file = meddle();
        let first = &file.tracks[0];

        assert_eq!(first.number, 1);
        assert_eq!(first.kind, CueTrackKind::Audio);
        assert_eq!(first.tags.title.as_deref(), Some("One of These Days"));
        assert_eq!(first.tags.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(first.tags.album.as_deref(), Some("Meddle"));
        assert_eq!(first.tags.album_artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(first.tags.date.as_deref(), Some("1971"));
        assert_eq!(first.tags.genre.as_deref(), Some("Progressive Rock"));
        assert_eq!(first.tags.track_total, Some(3));
        assert_eq!(first.tags.track_number, Some(1));
        assert_eq!(file.tracks[2].tags.track_number, Some(6));
        assert_eq!(
            file.tracks[1].tags.credits.composer.as_deref(),
            Some("Gilmour")
        );
    }

    #[test]
    fn a_track_that_declares_both_indexes_starts_where_the_music_does() {
        let file = meddle();

        assert_eq!(
            file.tracks[1].start,
            CueStart::Written(CueStamp::new(5, 57, 50))
        );
        assert_eq!(
            file.tracks[2].start,
            CueStart::Written(CueStamp::new(21, 10, 0))
        );
    }

    #[test]
    fn a_sub_index_after_the_first_leaves_the_start_where_the_first_put_it() {
        let sheet = read(
            b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n    INDEX 02 02:10:00\n  TRACK 02 AUDIO\n    INDEX 00 04:00:00\n    INDEX 01 04:02:00\n    INDEX 02 05:00:00\n",
        );
        let tracks = &sheet.files[0].tracks;

        assert_eq!(tracks[0].start, CueStart::Written(CueStamp::new(0, 0, 0)));
        assert_eq!(tracks[1].start, CueStart::Written(CueStamp::new(4, 2, 0)));
    }

    #[test]
    fn a_span_is_read_back_as_the_row_it_was_cut_for() {
        let file = meddle();
        let rate = SampleRate::HZ_44100;
        let echoes = file
            .span_of(2, rate, None)
            .expect("the last track is a span");

        let row = file.cut_at(echoes.start(), rate).expect("the row it names");
        assert_eq!(row.tags.title.as_deref(), Some("Echoes"));
        assert_eq!(
            row.tags.replay_gain.track_gain,
            Decibels::new(-7.06).ok(),
            "the row's own gain was not read back"
        );
        assert!(
            file.cut_at(Frames(echoes.start().get() + 1), rate)
                .is_none(),
            "a frame no track starts on named a row"
        );
    }

    #[test]
    fn a_row_a_sheet_never_titled_is_billed_by_its_number() {
        let sheet = read(b"FILE \"Meddle.flac\" WAVE\n TRACK 04 AUDIO\n  INDEX 01 00:00:00\n");
        let row = &sheet.files[0].tracks[0];

        assert_eq!(row.tags.title, None);
        assert_eq!(row.titled().title.as_deref(), Some("Track 4"));
    }

    #[test]
    fn a_sheet_beside_a_file_is_the_one_that_names_it() {
        let two = read(
            b"FILE \"one.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n\
              FILE \"two.flac\" WAVE\n TRACK 02 AUDIO\n  INDEX 01 00:00:00\n",
        );

        assert_eq!(
            cut_named(two.clone(), "two.flac").map(|cut| cut.named),
            Some("two.flac".to_owned())
        );
        assert_eq!(cut_named(two, "three.flac"), None);

        let one = read(b"FILE \"one.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n");
        assert_eq!(
            cut_named(one, "renamed.flac").map(|cut| cut.named),
            Some("one.flac".to_owned()),
            "a sheet holding one cut was not taken for the file beside it"
        );
    }

    #[test]
    fn a_stamp_is_seventy_five_sectors_to_the_second_at_every_rate() {
        let stamp = CueStamp::new(5, 57, 50);

        assert_eq!(stamp.total_sectors(), (5 * 60 + 57) * 75 + 50);
        assert_eq!(stamp.at(SampleRate::HZ_44100), Frames(15_773_100));
        assert_eq!(
            CueStamp::new(0, 1, 0).at(SampleRate::HZ_44100),
            Frames(44_100)
        );
        assert_eq!(
            CueStamp::new(0, 0, 75).at(SampleRate::HZ_96000),
            Frames(96_000)
        );
    }

    #[test]
    fn a_stamp_too_long_for_its_minutes_is_not_read_and_the_longest_that_is_converts() {
        let sheet = read(
            b"FILE \"a.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n TRACK 02 AUDIO\n  INDEX 01 5000000000000000:00:00\n",
        );
        assert!(
            sheet.files[0]
                .tracks
                .iter()
                .all(|track| track.start == CueStart::Written(CueStamp::default())),
            "a stamp past what a minute count holds was read: {:?}",
            sheet.files[0].tracks
        );

        let longest = CueStamp::new(u32::MAX, 59, 74);
        assert_eq!(
            longest.total_sectors(),
            (u64::from(u32::MAX) * 60 + 59) * 75 + 74
        );
        assert_eq!(
            longest.at(SampleRate::HZ_384000),
            Frames(longest.total_sectors() * 384_000 / 75)
        );
    }

    #[test]
    fn a_track_ends_where_the_next_one_begins_and_the_last_runs_on() {
        let file = meddle();
        let rate = SampleRate::HZ_44100;

        let first = file.span_of(0, rate, None).expect("a first span");
        assert_eq!(first.start(), Frames::ZERO);
        assert_eq!(first.end(), Some(CueStamp::new(5, 57, 50).at(rate)));

        let last = file.span_of(2, rate, None).expect("a last span");
        assert_eq!(last.start(), CueStamp::new(21, 10, 0).at(rate));
        assert_eq!(last.end(), None);

        let bounded = file.span_of(2, rate, Some(Frames(60_000_000)));
        assert_eq!(bounded.and_then(FrameSpan::end), Some(Frames(60_000_000)));
    }

    #[test]
    fn a_data_track_is_kept_apart_from_the_audio_ones() {
        let sheet = read(
            b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:10:00\n",
        );
        let file = &sheet.files[0];

        assert_eq!(file.tracks[0].kind, CueTrackKind::Data);
        assert_eq!(file.tracks[1].kind, CueTrackKind::Audio);
        assert_eq!(file.audio_tracks().count(), 1);
    }

    #[test]
    fn a_track_with_no_index_at_all_still_starts_at_the_head_of_the_file() {
        let sheet = read(b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Echoes\"\n");

        assert_eq!(sheet.files[0].tracks[0].start, CueStart::default());
    }

    #[test]
    fn a_command_the_grammar_does_not_know_is_stepped_over() {
        let sheet = read(
            b"CATALOG 0000000000000\nFLAGS DCP\nCDTEXTFILE \"disc.cdt\"\nSOMETHINGELSE 4\nFILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n",
        );

        assert_eq!(sheet.files[0].tracks.len(), 1);
    }

    #[test]
    fn a_sheet_naming_two_files_cuts_each_one_on_its_own() {
        let sheet = read(
            b"FILE \"one.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\nFILE \"two.flac\" WAVE\n TRACK 02 AUDIO\n  INDEX 01 00:00:00\n",
        );

        assert_eq!(
            sheet.claims().collect::<Vec<_>>(),
            vec!["one.flac", "two.flac"]
        );
        assert_eq!(sheet.files[0].tracks.len(), 1);
        assert_eq!(sheet.files[1].tracks.len(), 1);
        assert_eq!(
            sheet.files[1]
                .span_of(0, SampleRate::HZ_44100, None)
                .map(FrameSpan::end),
            Some(None)
        );
    }

    #[test]
    fn a_track_whose_gap_ends_the_file_before_it_is_cut_from_the_file_its_first_index_names() {
        let sheet = read(
            b"FILE \"12.flac\" WAVE\r\n  TRACK 12 AUDIO\r\n    TITLE \"Twelve\"\r\n    INDEX 01 00:00:00\r\n  TRACK 13 AUDIO\r\n    TITLE \"Thirteen\"\r\n    INDEX 00 04:10:30\r\nFILE \"13.flac\" WAVE\r\n    INDEX 01 00:00:00\r\n  TRACK 14 AUDIO\r\n    TITLE \"Fourteen\"\r\n    INDEX 00 03:00:00\r\nFILE \"14.flac\" WAVE\r\n    INDEX 01 00:00:00\r\n",
        );

        let titled = |file: &CueFile| {
            file.tracks
                .iter()
                .map(|track| (track.tags.title.clone(), track.start))
                .collect::<Vec<_>>()
        };
        assert_eq!(sheet.files.len(), 3);
        for (file, title) in sheet.files.iter().zip(["Twelve", "Thirteen", "Fourteen"]) {
            assert_eq!(
                titled(file),
                vec![(Some(title.to_owned()), CueStart::default())],
                "{} holds a track it only ends with the gap of",
                file.named
            );
        }
        assert_eq!(
            sheet.files[0].span_of(0, SampleRate::HZ_44100, Some(Frames(11_054_988))),
            Some(FrameSpan::starting(Frames::ZERO).within(Frames(11_054_988)))
        );
    }

    #[test]
    fn a_track_with_no_index_before_the_next_file_line_belongs_to_that_file() {
        let sheet = read(
            b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"Two\"\nFILE \"two.flac\" WAVE\n    INDEX 01 00:00:00\n",
        );

        assert_eq!(sheet.files[0].tracks.len(), 1);
        assert_eq!(sheet.files[1].tracks.len(), 1);
        assert_eq!(sheet.files[1].tracks[0].tags.title.as_deref(), Some("Two"));
    }

    #[test]
    fn a_file_name_keeps_its_spaces_and_loses_its_type() {
        let sheet =
            read(b"FILE \"Pink Floyd - Meddle.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n");
        assert_eq!(sheet.files[0].named, "Pink Floyd - Meddle.flac");

        let bare = read(b"FILE Meddle.flac WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n");
        assert_eq!(bare.files[0].named, "Meddle.flac");
    }

    #[test]
    fn a_sheet_a_tagger_wrote_in_another_encoding_still_reads() {
        let mut wide = vec![0xFF, 0xFE];
        for unit in
            "FILE \"Écoute.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n".encode_utf16()
        {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        let sheet = read(&wide);

        assert_eq!(sheet.encoding, TextEncoding::Utf16Le);
        assert_eq!(sheet.files[0].named, "Écoute.flac");
    }

    #[test]
    fn a_sheet_carrying_more_tracks_than_a_disc_could_stops_at_the_bound() {
        let mut text = String::from("FILE \"one.flac\" WAVE\n");
        for number in 0..MOST_TRACKS + 10 {
            text.push_str(&format!(
                "  TRACK {number:02} AUDIO\n    INDEX 01 00:00:00\n"
            ));
        }
        let sheet = read(text.as_bytes());

        assert_eq!(sheet.files[0].tracks.len(), MOST_TRACKS);
    }

    #[test]
    fn rubbish_reads_as_a_sheet_that_names_nothing() {
        assert!(read(b"not a cue sheet at all").is_empty());
        assert!(read(b"").is_empty());
        assert!(read(&[0xFF, 0x00, 0x11]).is_empty());
    }

    #[test]
    fn a_per_track_replay_gain_is_the_only_one_a_single_file_rip_carries() {
        let file = meddle();
        let echoes = &file.tracks[2];

        assert_eq!(
            echoes.tags.replay_gain.track_gain.map(Decibels::get),
            Some(-7.06)
        );
        assert_eq!(file.tracks[0].tags.replay_gain.track_gain, None);
    }

    #[test]
    fn carriage_returns_do_not_reach_the_names_a_sheet_carries() {
        let sheet = read(b"TITLE \"Meddle\"\r\nFILE \"Meddle.flac\" WAVE\r\n  TRACK 01 AUDIO\r\n    TITLE \"Echoes\"\r\n    INDEX 01 00:00:00\r\n");

        assert_eq!(
            sheet.files[0].tracks[0].tags.title.as_deref(),
            Some("Echoes")
        );
        assert_eq!(
            sheet.files[0].tracks[0].tags.album.as_deref(),
            Some("Meddle")
        );
        assert_eq!(sheet.files[0].named, "Meddle.flac");
    }

    #[test]
    fn a_renamed_sheet_names_the_audio_by_its_new_name_and_keeps_every_other_byte() {
        let written = renamed(
            MEDDLE.as_bytes(),
            "Meddle.flac",
            "01 One of These Days.flac",
        )
        .expect("a sheet naming the file once is rewritten");
        let text = String::from_utf8(written).expect("a UTF-8 sheet stays UTF-8");

        assert_eq!(
            text,
            MEDDLE.replace("\"Meddle.flac\"", "\"01 One of These Days.flac\"")
        );
        assert_eq!(
            read(text.as_bytes()).files[0].named,
            "01 One of These Days.flac"
        );
    }

    #[test]
    fn a_renamed_sheet_is_written_in_the_encoding_it_was_read_in() {
        let mut wide = vec![0xFF, 0xFE];
        for unit in
            "FILE \"Écoute.flac\" WAVE\r\n TRACK 01 AUDIO\r\n  INDEX 01 00:00:00\r\n".encode_utf16()
        {
            wide.extend_from_slice(&unit.to_le_bytes());
        }

        let written = renamed(&wide, "Écoute.flac", "01 Écoute.flac")
            .expect("a wide sheet naming the file once is rewritten");
        let sheet = read(&written);

        assert_eq!(sheet.encoding, TextEncoding::Utf16Le);
        assert_eq!(sheet.files[0].named, "01 Écoute.flac");
        assert_eq!(written[..2], [0xFF, 0xFE]);
    }

    #[test]
    fn a_sheet_that_does_not_name_the_file_once_is_left_as_it_was() {
        assert_eq!(
            renamed(MEDDLE.as_bytes(), "Obscured.flac", "one.flac"),
            None
        );

        let twice = b"FILE \"one.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\nFILE \"one.flac\" WAVE\n TRACK 02 AUDIO\n  INDEX 01 00:00:00\n";
        assert_eq!(renamed(twice, "one.flac", "two.flac"), None);

        let remarked =
            b"REM COMMENT \"one.flac\"\nFILE \"one.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n";
        assert_eq!(renamed(remarked, "one.flac", "two.flac"), None);
    }

    #[test]
    fn a_sheet_a_name_cannot_be_written_into_is_left_as_it_was() {
        let legacy = [
            b"FILE \"Ecout".as_slice(),
            &[0xE9],
            b".flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n",
        ]
        .concat();

        assert_eq!(read(&legacy).encoding, TextEncoding::Windows1252);
        assert_eq!(
            renamed(&legacy, "Ecout\u{e9}.flac", "Przybyłowicz.flac"),
            None
        );
        assert!(renamed(&legacy, "Ecout\u{e9}.flac", "Ecoute.flac").is_some());
    }
}
