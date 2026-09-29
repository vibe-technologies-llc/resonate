use std::{
    fmt, fs,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
};

use resonate_core::{
    Decibels, FrameSpan, Frames, MediaLocation, SampleRate, TextEncoding,
    text::{UTF8_BOM, decoded, decoded_as, encoded},
};

use crate::{Error, MediaInfo, ReplayGain, Result, TagSet, source::Sources, tags::Uppercased};

const SECTORS_PER_SECOND: u64 = 75;
const MOST_TRACKS: usize = 999;
const LARGEST_CUE_SHEET: u64 = 1 << 20;
const SHEET_EXTENSION: &str = "cue";
const FILE_COMMAND: &str = "FILE";
const FIRST_INDEX: u32 = 1;
const BYTE_ORDER_MARK: char = '\u{feff}';

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

    fn is_before(self, other: Self) -> bool {
        match (self, other) {
            (Self::Written(one), Self::Written(other)) => one < other,
            (Self::Sampled(one), Self::Sampled(other)) => one < other,
            (Self::Written(_), Self::Sampled(_)) | (Self::Sampled(_), Self::Written(_)) => false,
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
            Some(next) if next.start.at(rate) < start => return None,
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

    pub(crate) fn heard_from_the_head(&mut self) {
        let Some(first) = self.tracks.first_mut() else {
            return;
        };
        if first.kind != CueTrackKind::Audio {
            return;
        }
        first.start = match first.start {
            CueStart::Written(_) => CueStart::Written(CueStamp::default()),
            CueStart::Sampled(_) => CueStart::Sampled(Frames::ZERO),
        };
    }

    pub(crate) fn billed_by(mut self, file: &TagSet) -> Self {
        let total = self.audio_tracks().count() as u32;
        for track in &mut self.tracks {
            let own = std::mem::take(&mut track.tags);
            track.tags = TagSet {
                track_number: Some(track.number),
                track_total: Some(total),
                isrc: own.isrc,
                barcode: own.barcode.or_else(|| file.barcode.clone()),
                ..the_albums_of(file)
            };
        }
        self
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
    let stem = path.file_stem()?.to_str()?;

    sheets_beside(path.parent()?, named, stem)
        .into_iter()
        .find_map(|sheet| {
            let held = read_media(sources, &MediaLocation::local(sheet)).ok()?;
            cut_named(held, named)
        })
}

fn sheets_beside(folder: &Path, named: &str, stem: &str) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut sheets: Vec<(bool, PathBuf)> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file = entry.file_name();
            let (lead, extension) = file.to_str()?.rsplit_once('.')?;
            if !extension.eq_ignore_ascii_case(SHEET_EXTENSION) {
                return None;
            }
            let by_stem = folded(lead) == folded(stem);
            (by_stem || folded(lead) == folded(named)).then(|| (!by_stem, entry.path()))
        })
        .collect();
    sheets.sort();
    sheets.into_iter().map(|(_, sheet)| sheet).collect()
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
    let at = the_best_named(
        cut.iter()
            .enumerate()
            .map(|(at, file)| (at, Naming::of(&file.named, named))),
    )?;
    Some(cut.swap_remove(at))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Naming {
    ByStem,
    ByCase,
    Exactly,
}

impl Naming {
    pub fn of(named: &str, file: &str) -> Option<Self> {
        let named = the_name_alone(named);
        if named == file {
            return Some(Self::Exactly);
        }
        if folded(named) == folded(file) {
            return Some(Self::ByCase);
        }
        (folded(stem_of(named)) == folded(stem_of(file))).then_some(Self::ByStem)
    }
}

pub fn the_one_named<'a, T>(
    named: &str,
    candidates: impl IntoIterator<Item = (T, &'a str)>,
) -> Option<T> {
    the_best_named(
        candidates
            .into_iter()
            .map(|(candidate, file)| (candidate, Naming::of(named, file))),
    )
}

pub fn the_best_named<T>(candidates: impl IntoIterator<Item = (T, Option<Naming>)>) -> Option<T> {
    let mut best: Option<(Naming, T)> = None;
    let mut tied = false;
    for (candidate, naming) in candidates {
        let Some(naming) = naming else {
            continue;
        };
        match &best {
            Some((held, _)) if naming < *held => {}
            Some((held, _)) if naming == *held => tied = true,
            _ => {
                best = Some((naming, candidate));
                tied = false;
            }
        }
    }

    if tied {
        tracing::debug!("a cue sheet's file line names more than one file alike");
        return None;
    }
    best.map(|(_, candidate)| candidate)
}

fn the_name_alone(named: &str) -> &str {
    named
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(named)
}

fn stem_of(named: &str) -> &str {
    match named.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => named,
    }
}

fn folded(named: &str) -> String {
    named.to_lowercase()
}

pub fn renamed(sheet: &[u8], from: &str, to: &str) -> Option<Vec<u8>> {
    let held = read(sheet);
    let from = the_best_named(held.claims().map(|named| (named, Naming::of(named, from))))?;

    let named = encoded(from, held.encoding)?;
    let at = the_run_on_the_file_line(sheet, &named, held.encoding)?;
    let Some(naming) = encoded(to, held.encoding) else {
        return Some(carried_into_unicode(
            sheet,
            held.encoding,
            at..at + named.len(),
            to,
        ));
    };

    let mut written = Vec::with_capacity(sheet.len() + naming.len() - named.len());
    written.extend_from_slice(&sheet[..at]);
    written.extend_from_slice(&naming);
    written.extend_from_slice(&sheet[at + named.len()..]);
    Some(written)
}

fn carried_into_unicode(
    sheet: &[u8],
    encoding: TextEncoding,
    named: Range<usize>,
    to: &str,
) -> Vec<u8> {
    let body = if sheet.starts_with(&UTF8_BOM) {
        UTF8_BOM.len()
    } else {
        0
    };
    let before = decoded_as(sheet.get(body..named.start).unwrap_or_default(), encoding);
    let after = decoded_as(sheet.get(named.end..).unwrap_or_default(), encoding);

    let mut written = Vec::with_capacity(UTF8_BOM.len() + before.len() + to.len() + after.len());
    written.extend_from_slice(&UTF8_BOM);
    written.extend_from_slice(before.as_bytes());
    written.extend_from_slice(to.as_bytes());
    written.extend_from_slice(after.as_bytes());
    written
}

fn the_run_on_the_file_line(sheet: &[u8], named: &[u8], encoding: TextEncoding) -> Option<usize> {
    let unit = unit_bytes(encoding);
    let last = sheet.len().checked_sub(named.len())?;
    let mut found = None;
    for at in (0..=last).step_by(unit) {
        if &sheet[at..at + named.len()] != named || !follows_a_file_command(sheet, at, encoding) {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(at);
    }

    found
}

fn follows_a_file_command(sheet: &[u8], at: usize, encoding: TextEncoding) -> bool {
    let unit = unit_bytes(encoding);
    let mut start = at;
    while let Some(before) = start.checked_sub(unit) {
        if matches!(
            character(&sheet[before..start], encoding),
            Some('\n' | '\r')
        ) {
            break;
        }
        start = before;
    }

    let mut lead = String::new();
    for piece in sheet[start..at].chunks(unit) {
        match character(piece, encoding) {
            Some(BYTE_ORDER_MARK) if lead.is_empty() => {}
            Some(held) if held.is_ascii() => lead.push(held),
            _ => return false,
        }
    }

    let lead =
        lead.trim_start_matches(|held: char| held.is_whitespace() || held == BYTE_ORDER_MARK);
    let Some((command, rest)) = lead.split_once(char::is_whitespace) else {
        return false;
    };
    command.eq_ignore_ascii_case(FILE_COMMAND) && matches!(rest.trim_start(), "" | "\"")
}

const fn unit_bytes(encoding: TextEncoding) -> usize {
    encoding.unit_bytes()
}

fn character(piece: &[u8], encoding: TextEncoding) -> Option<char> {
    match (encoding, piece) {
        (TextEncoding::Utf8 | TextEncoding::Legacy(_), [byte]) => Some(char::from(*byte)),
        (TextEncoding::Utf16Le, [low, high]) => {
            char::from_u32(u32::from(u16::from_le_bytes([*low, *high])))
        }
        (TextEncoding::Utf16Be, [high, low]) => {
            char::from_u32(u32::from(u16::from_be_bytes([*high, *low])))
        }
        _ => None,
    }
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
    Unreadably,
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
            Indexed::AtTheFirst | Indexed::Unreadably => None,
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
            if number == FIRST_INDEX && self.indexed != Indexed::AtTheFirst {
                self.indexed = Indexed::Unreadably;
            }
            return;
        };

        self.indexed = match (number, self.indexed) {
            (FIRST_INDEX, _) => Indexed::AtTheFirst,
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
            Named::Catalog if album => tags.barcode = Some(value),
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
            "COMPOSER" => tags.credits.composer = Some(value),
            "DISCNUMBER" => {
                let (number, total) = numbered(&value);
                tags.disc_number = number.or(tags.disc_number);
                tags.disc_total = total.or(tags.disc_total);
            }
            "TOTALDISCS" | "DISCTOTAL" => tags.disc_total = numbered(&value).0,
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
        if self.indexed == Indexed::Unreadably {
            tracing::debug!(
                track = track.number,
                "a cue track whose first index does not read is left out"
            );
            return;
        }
        if let Some(before) = file.tracks.last()
            && track.start.is_before(before.start)
        {
            tracing::debug!(
                track = track.number,
                "a cue track starting before the one ahead of it is left out"
            );
            return;
        }
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
        let total = files
            .iter()
            .map(|file| file.audio_tracks().count())
            .sum::<usize>() as u32;
        for file in &mut files {
            file.heard_from_the_head();
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
    if tags.credits.composer.is_none() {
        tags.credits.composer = album.credits.composer.clone();
    }
    if tags.disc_number.is_none() {
        tags.disc_number = album.disc_number;
    }
    if tags.disc_total.is_none() {
        tags.disc_total = album.disc_total;
    }
    if tags.barcode.is_none() {
        tags.barcode = album.barcode.clone();
    }
    if tags.replay_gain.album_gain.is_none() {
        tags.replay_gain.album_gain = album.replay_gain.album_gain;
    }
    if tags.replay_gain.album_peak.is_none() {
        tags.replay_gain.album_peak = album.replay_gain.album_peak;
    }
}

fn the_albums_of(file: &TagSet) -> TagSet {
    TagSet {
        artist: file.album_artist.clone().or_else(|| file.artist.clone()),
        album: file.album.clone(),
        album_artist: file.album_artist.clone(),
        disc_number: file.disc_number,
        disc_total: file.disc_total,
        date: file.date.clone(),
        genre: file.genre.clone(),
        label: file.label.clone(),
        copyright: file.copyright.clone(),
        musicbrainz_album_id: file.musicbrainz_album_id.clone(),
        musicbrainz_album_artist_id: file.musicbrainz_album_artist_id.clone(),
        musicbrainz_release_group_id: file.musicbrainz_release_group_id.clone(),
        catalog_number: file.catalog_number.clone(),
        compilation: file.compilation,
        replay_gain: ReplayGain {
            album_gain: file.replay_gain.album_gain,
            album_peak: file.replay_gain.album_peak,
            ..ReplayGain::default()
        },
        ..TagSet::default()
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
    rest.rfind('"').filter(|at| *at > 0)
}

fn quoted(value: &str) -> String {
    let value = value.trim();
    let Some(rest) = value.strip_prefix('"') else {
        return value.to_owned();
    };
    match rest.rsplit_once('"') {
        Some((held, _)) => held.to_owned(),
        None => rest.to_owned(),
    }
}

fn numbered(value: &str) -> (Option<u32>, Option<u32>) {
    let (number, total) = match value.split_once('/') {
        Some((number, total)) => (number, Some(total)),
        None => (value, None),
    };
    let read = |held: &str| held.trim().parse().ok().filter(|held: &u32| *held > 0);
    (read(number), total.and_then(read))
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
        assert_eq!(
            sheet.files[0]
                .tracks
                .iter()
                .map(|track| (track.number, track.start))
                .collect::<Vec<_>>(),
            vec![(1, CueStart::Written(CueStamp::default()))],
            "a stamp past what a minute count holds was read"
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
    fn a_sheet_of_one_file_a_track_counts_every_audio_track_it_names() {
        let sheet = read(
            b"FILE \"01.flac\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n\
              FILE \"02.flac\" WAVE\n TRACK 02 AUDIO\n  INDEX 01 00:00:00\n\
              FILE \"03.flac\" WAVE\n TRACK 03 AUDIO\n  INDEX 01 00:00:00\n\
              FILE \"disc.bin\" BINARY\n TRACK 04 MODE1/2352\n  INDEX 01 00:00:00\n",
        );

        let totals = sheet
            .files
            .iter()
            .flat_map(|file| &file.tracks)
            .map(|track| (track.number, track.tags.track_total))
            .collect::<Vec<_>>();
        assert_eq!(
            totals,
            vec![(1, Some(3)), (2, Some(3)), (3, Some(3)), (4, Some(3))]
        );
    }

    #[test]
    fn a_track_whose_first_index_does_not_read_is_left_out_rather_than_overlapping() {
        let sheet = read(
            b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 00 03:58:00\n    INDEX 01 04:0x:00\n  TRACK 03 AUDIO\n    INDEX 01 08:00:00\n",
        );
        let file = &sheet.files[0];

        assert_eq!(
            file.tracks
                .iter()
                .map(|track| track.number)
                .collect::<Vec<_>>(),
            vec![1, 3]
        );
        assert_eq!(file.tracks[0].tags.track_total, Some(2));
        assert_eq!(
            file.span_of(0, SampleRate::HZ_44100, None)
                .and_then(FrameSpan::end),
            Some(CueStamp::new(8, 0, 0).at(SampleRate::HZ_44100))
        );
    }

    #[test]
    fn a_track_starting_before_the_one_ahead_of_it_is_left_out() {
        let sheet = read(
            b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 05:00:00\n  TRACK 03 AUDIO\n    INDEX 01 03:00:00\n  TRACK 04 AUDIO\n    TITLE \"Never indexed\"\n  TRACK 05 AUDIO\n    INDEX 01 07:00:00\n",
        );
        let file = &sheet.files[0];

        assert_eq!(
            file.tracks
                .iter()
                .map(|track| track.number)
                .collect::<Vec<_>>(),
            vec![1, 2, 5]
        );
    }

    #[test]
    fn a_span_whose_next_track_starts_earlier_is_refused_rather_than_turned_round() {
        let file = CueFile {
            named: "one.flac".to_owned(),
            tracks: vec![
                CueTrack {
                    start: CueStart::Sampled(Frames(44_100)),
                    ..CueTrack::default()
                },
                CueTrack {
                    start: CueStart::Sampled(Frames(10)),
                    ..CueTrack::default()
                },
            ],
        };

        assert_eq!(file.span_of(0, SampleRate::HZ_44100, None), None);
    }

    #[test]
    fn audio_before_the_first_index_of_a_file_is_the_first_tracks() {
        let hidden = read(
            b"FILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 03:25:12\n  TRACK 02 AUDIO\n    INDEX 01 07:00:00\n",
        );
        assert_eq!(
            hidden.files[0].tracks[0].start,
            CueStart::Written(CueStamp::default())
        );

        let prepended = read(
            b"FILE \"01.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\nFILE \"02.flac\" WAVE\n  TRACK 02 AUDIO\n    INDEX 00 00:00:00\n    INDEX 01 00:02:10\n",
        );
        assert_eq!(
            prepended.files[1].tracks[0].start,
            CueStart::Written(CueStamp::default()),
            "the gap at the head of a file belonged to no row"
        );

        let mixed = read(
            b"FILE \"disc.bin\" BINARY\n  TRACK 01 MODE1/2352\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    INDEX 01 00:10:00\n",
        );
        assert_eq!(
            mixed.files[0].tracks[1].start,
            CueStart::Written(CueStamp::new(0, 10, 0))
        );
    }

    #[test]
    fn a_value_carrying_quotes_of_its_own_is_read_to_its_last_quote() {
        let sheet = read(
            b"TITLE \"The \"Real\" Album\"\nFILE \"The \"Real\" Thing.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"The \"Real\" Thing\"\n    PERFORMER Unquoted Artist\n    INDEX 01 00:00:00\n",
        );
        let track = &sheet.files[0].tracks[0];

        assert_eq!(sheet.files[0].named, "The \"Real\" Thing.flac");
        assert_eq!(track.tags.title.as_deref(), Some("The \"Real\" Thing"));
        assert_eq!(track.tags.album.as_deref(), Some("The \"Real\" Album"));
        assert_eq!(track.tags.artist.as_deref(), Some("Unquoted Artist"));
    }

    #[test]
    fn the_disc_the_composer_and_the_catalogue_a_sheet_names_reach_every_track() {
        let sheet = read(
            b"REM DISCNUMBER 2\nREM TOTALDISCS 3\nREM COMPOSER \"Bach\"\nCATALOG 0724356757828\nSONGWRITER \"Gilmour\"\nFILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    REM COMPOSER \"Waters\"\n    INDEX 01 04:00:00\n",
        );
        let [first, second] = sheet.files[0].tracks.as_slice() else {
            panic!("two tracks: {:?}", sheet.files[0].tracks);
        };

        assert_eq!(first.tags.disc_number, Some(2));
        assert_eq!(first.tags.disc_total, Some(3));
        assert_eq!(first.tags.barcode.as_deref(), Some("0724356757828"));
        assert_eq!(first.tags.credits.composer.as_deref(), Some("Gilmour"));
        assert_eq!(second.tags.credits.composer.as_deref(), Some("Waters"));
        assert_eq!(second.tags.disc_number, Some(2));

        let slashed = read(
            b"REM DISCNUMBER 1/2\nFILE \"one.flac\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n",
        );
        let tags = &slashed.files[0].tracks[0].tags;
        assert_eq!((tags.disc_number, tags.disc_total), (Some(1), Some(2)));
    }

    #[test]
    fn a_block_cut_is_billed_by_the_album_its_file_is_tagged_as() {
        let file = TagSet {
            title: Some("Meddle".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            album: Some("Meddle".to_owned()),
            date: Some("1971".to_owned()),
            isrc: Some("GBN9Y1100001".to_owned()),
            track_number: Some(1),
            ..TagSet::default()
        };
        let cut = CueFile {
            named: String::new(),
            tracks: vec![
                CueTrack {
                    number: 1,
                    tags: TagSet {
                        isrc: Some("GBAYE7100001".to_owned()),
                        ..TagSet::default()
                    },
                    ..CueTrack::default()
                },
                CueTrack {
                    number: 2,
                    start: CueStart::Sampled(Frames(44_100)),
                    ..CueTrack::default()
                },
                CueTrack {
                    number: 170,
                    kind: CueTrackKind::Data,
                    start: CueStart::Sampled(Frames(88_200)),
                    ..CueTrack::default()
                },
            ],
        }
        .billed_by(&file);

        let second = &cut.tracks[1].tags;
        assert_eq!(second.album.as_deref(), Some("Meddle"));
        assert_eq!(second.artist.as_deref(), Some("Pink Floyd"));
        assert_eq!(second.date.as_deref(), Some("1971"));
        assert_eq!(second.title, None);
        assert_eq!(second.isrc, None);
        assert_eq!(
            (second.track_number, second.track_total),
            (Some(2), Some(2))
        );
        assert_eq!(cut.tracks[0].tags.isrc.as_deref(), Some("GBAYE7100001"));
    }

    #[test]
    fn a_file_line_names_its_file_whatever_the_case_the_extension_or_the_folder() {
        assert_eq!(
            Naming::of("Album.flac", "Album.flac"),
            Some(Naming::Exactly)
        );
        assert_eq!(Naming::of("ALBUM.WAV", "album.wav"), Some(Naming::ByCase));
        assert_eq!(Naming::of("ALBUM.WAV", "album.flac"), Some(Naming::ByStem));
        assert_eq!(
            Naming::of("C:\\Rips\\Écoute.wav", "écoute.flac"),
            Some(Naming::ByStem)
        );
        assert_eq!(Naming::of("CD1\\01.flac", "01.flac"), Some(Naming::Exactly));
        assert_eq!(Naming::of("Album.flac", "Albums.flac"), None);

        let beside = [("wav", "album.wav"), ("flac", "Album.flac")];
        assert_eq!(the_one_named("Album.flac", beside), Some("flac"));
        assert_eq!(the_one_named("ALBUM.WAV", beside), Some("wav"));
        assert_eq!(
            the_one_named("Album.ape", beside),
            None,
            "a stem naming two files alike was taken for one"
        );
    }

    #[test]
    fn a_sheet_beside_a_file_is_found_under_any_case_and_after_its_whole_name() {
        let folder = std::env::temp_dir().join(format!("resonate-cue-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a folder");
        for named in ["Album.flac.Cue", "ALBUM.CUE", "Album.log", "Other.cue"] {
            fs::write(folder.join(named), b"").expect("a file");
        }

        let found = sheets_beside(&folder, "Album.flac", "Album");
        fs::remove_dir_all(&folder).expect("the folder goes");

        assert_eq!(
            found,
            vec![folder.join("ALBUM.CUE"), folder.join("Album.flac.Cue")]
        );
    }

    #[test]
    fn a_sheet_naming_its_file_by_another_case_is_renamed_on_its_file_line() {
        let written = renamed(
            b"FILE \"ALBUM.WAV\" WAVE\n TRACK 01 AUDIO\n  INDEX 01 00:00:00\n",
            "album.wav",
            "01 Album.wav",
        )
        .expect("the file line is rewritten");

        assert_eq!(read(&written).files[0].named, "01 Album.wav");
    }

    #[test]
    fn a_sheet_written_in_a_japanese_code_page_is_read_and_renamed_in_it() {
        let japanese = TextEncoding::Legacy(resonate_core::LegacyEncoding::SHIFT_JIS);
        let text = "PERFORMER \"植松伸夫\"\nTITLE \"ファイナルファンタジー オリジナル・サウンドトラック\"\nFILE \"ファイナルファンタジー.flac\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"序曲\"\n    INDEX 01 00:00:00\n";
        let bytes = encoded(text, japanese).expect("Japanese letters");

        let sheet = read(&bytes);
        assert_eq!(sheet.encoding, japanese);
        assert_eq!(sheet.files[0].named, "ファイナルファンタジー.flac");
        assert_eq!(sheet.files[0].tracks[0].tags.title.as_deref(), Some("序曲"));

        let written =
            renamed(&bytes, "ファイナルファンタジー.flac", "01 序曲.flac").expect("rewritten");
        assert_eq!(read(&written).files[0].named, "01 序曲.flac");
        assert_eq!(read(&written).encoding, japanese);
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
    }

    #[test]
    fn a_sheet_naming_its_audio_in_a_remark_as_well_has_the_file_line_alone_rewritten() {
        let remarked = "REM COMMENT \"one.flac\"\r\nFILE \"one.flac\" WAVE\r\n TRACK 01 AUDIO\r\n  INDEX 01 00:00:00\r\n";

        let written = renamed(remarked.as_bytes(), "one.flac", "two.flac")
            .expect("the FILE line is rewritten");

        assert_eq!(
            String::from_utf8(written).expect("a UTF-8 sheet"),
            remarked.replace("FILE \"one.flac\"", "FILE \"two.flac\"")
        );
    }

    #[test]
    fn a_wide_sheet_naming_its_audio_in_a_title_as_well_has_the_file_line_alone_rewritten() {
        let text = "TITLE \"one.flac\"\r\nfile one.flac WAVE\r\n TRACK 01 AUDIO\r\n  INDEX 01 00:00:00\r\n";
        let wide = |text: &str| {
            let mut wide = vec![0xFE, 0xFF];
            for unit in text.encode_utf16() {
                wide.extend_from_slice(&unit.to_be_bytes());
            }
            wide
        };

        let written = renamed(&wide(text), "one.flac", "two.flac").expect("the FILE line");

        assert_eq!(
            written,
            wide(&text.replace("file one.flac", "file two.flac"))
        );
    }

    #[test]
    fn a_sheet_whose_encoding_has_no_letters_for_the_new_name_is_carried_into_unicode() {
        let legacy = [
            b"REM COMMENT \"".as_slice(),
            &[0x93, 0xE9, 0x94],
            b"\"\r\nFILE \"Ecout".as_slice(),
            &[0xE9],
            b".flac\" WAVE\r\n TRACK 01 AUDIO\r\n  INDEX 01 00:00:00\r\n",
        ]
        .concat();
        assert_eq!(read(&legacy).encoding, TextEncoding::WINDOWS_1252);

        let written = renamed(&legacy, "Ecout\u{e9}.flac", "Przybyłowicz.flac")
            .expect("the sheet is rewritten");

        let unicode = "\u{feff}REM COMMENT \"\u{201c}\u{e9}\u{201d}\"\r\nFILE \"Przybyłowicz.flac\" \
                       WAVE\r\n TRACK 01 AUDIO\r\n  INDEX 01 00:00:00\r\n";
        assert_eq!(written, unicode.as_bytes());
        let reread = read(&written);
        assert_eq!(reread.encoding, TextEncoding::Utf8);
        assert_eq!(
            reread.claims().collect::<Vec<_>>(),
            vec!["Przybyłowicz.flac"]
        );

        let kept = renamed(&legacy, "Ecout\u{e9}.flac", "Ecoute.flac").expect("it renames");
        assert_eq!(read(&kept).encoding, TextEncoding::WINDOWS_1252);
    }
}
