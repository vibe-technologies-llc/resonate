use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use resonate_core::{
    Answer, CODES_SHARED_AT_MOST, Chromaprint, Figure, LARGEST_SHARED_LYRICSFILE,
    LINKS_SHARED_AT_MOST, LONGEST_SHARED_LYRICS, LONGEST_SHARED_PRINT, Link, Mbid, Recording,
    SharedCredit, SharedHeard, SharedLyrics, SharedLyricsfile, SharedMedium, SharedPrint,
    SharedRecognition, SharedRelease, SharedStudy, SharedTrack, WholeFile,
};

use crate::{
    DeviceId, Error, Malformed, Result, SpaceId, Token,
    catalog::{Held, SongTags},
    change::{
        AnswerNote, Change, DeviceForgotten, DeviceNote, FavouriteNote, Handoff, Listen, Platform,
        PlaylistBody, PlaylistFolded, PlaylistKey, PlaylistNote, QueryBody, QueryFolded, QueryNote,
        Stamp,
    },
    key::{DEVICE_BYTES, SPACE_BYTES, TOKEN_BYTES},
    seal::SEALED_AT_LEAST,
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const SERVER_ALPN: &[u8] = b"resonate-sync/1";
pub const PEER_ALPN: &[u8] = b"resonate-peer/1";
pub const LARGEST_FRAME: usize = 32 * 1024 * 1024;
pub const LONGEST_TEXT: usize = 4096;
pub const LARGEST_BLOB: usize = LARGEST_FRAME;
pub const LARGEST_RECORD: usize = 256 * 1024;
pub const LARGEST_DATAGRAM: usize = 2048;
const RECORD_AT_LEAST: usize = 2;
const BAND_LEVELS_AT_MOST: usize = 256;
const ANSWER_LYRICS: u8 = 0;
const ANSWER_STUDY: u8 = 1;
const ANSWER_RELEASE: u8 = 2;
const ANSWER_KINDS: &[u8] = &[ANSWER_LYRICS, ANSWER_STUDY, ANSWER_RELEASE];
const MEDIUM_AT_LEAST: usize = 4;
const TRACK_AT_LEAST: usize = 9;
const CREDIT_AT_LEAST: usize = 3;
const LINK_AT_LEAST: usize = 2;
const CODE_AT_LEAST: usize = 1;
const MBID_AT_LEAST: usize = 37;
const RECORDING_AT_LEAST: usize = 5;
const WHOLE_FILE_AT_LEAST: usize = 3;
const RECOGNITION_UNHEARD: u8 = 0;
const RECOGNITION_HEARD: u8 = 1;
pub const COVERS_ASKED_AT_MOST: usize = 512;
pub const LYRICSFILES_ASKED_AT_MOST: usize = 512;
pub const PRINTS_ASKED_AT_MOST: usize = 512;
pub const LARGEST_SHARED_COVER: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToServer {
    Hello {
        version: u16,
        space: SpaceId,
        token: Token,
        device: DeviceId,
        after: u64,
    },
    Append {
        blobs: Vec<Vec<u8>>,
    },
    Compact {
        through: u64,
        parts: Vec<Vec<u8>>,
    },
    Punch {
        to: DeviceId,
        sealed: Vec<u8>,
    },
    Bye,
    Relaying,
    Relay {
        to: DeviceId,
        datagram: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub seq: u64,
    pub blob: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Refusal {
    Version,
    Token,
    TooLarge,
    SpaceFull,
    TooFast,
    Busy,
    Stale,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromServer {
    Welcome {
        online: Vec<DeviceId>,
        seen_at: SocketAddr,
        head: u64,
    },
    Snapshot {
        through: u64,
        parts: Vec<Vec<u8>>,
    },
    Records {
        entries: Vec<Record>,
    },
    Appended {
        count: u32,
    },
    Arrived {
        device: DeviceId,
    },
    Left {
        device: DeviceId,
    },
    Punched {
        device: DeviceId,
        address: SocketAddr,
        sealed: Vec<u8>,
    },
    Compacted {
        through: u64,
    },
    Refused(Refusal),
    Relayed {
        from: DeviceId,
        datagram: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToPeer {
    Holdings,
    Fetch { recording: Recording, offset: u64 },
    Meet { device: DeviceId, records: bool },
    Changes { whole: bool, changes: Vec<Change> },
    Tags { recording: Recording },
    Covers { releases: Vec<Mbid> },
    Lyricsfiles { recordings: Vec<Recording> },
    Prints { files: Vec<WholeFile> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromPeer {
    Held {
        items: Vec<Held>,
    },
    HeldEnd,
    Sending {
        size: u64,
        extension: String,
    },
    Missing,
    Met {
        device: DeviceId,
    },
    Taken,
    Tagged {
        tags: SongTags,
    },
    Covered {
        release: Mbid,
        picture: Vec<u8>,
    },
    CoversEnd,
    Documented {
        recording: Recording,
        lyricsfile: SharedLyricsfile,
    },
    LyricsfilesEnd,
    Printed {
        file: WholeFile,
        print: SharedPrint,
    },
    PrintsEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Beacon {
    pub device: DeviceId,
    pub port: u16,
    pub answering: bool,
    pub addresses: Vec<IpAddr>,
}

#[derive(Default)]
pub struct Writer(Vec<u8>);

pub struct Reader<'a> {
    rest: &'a [u8],
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn finish(self) -> Vec<u8> {
        self.0
    }

    pub fn byte(&mut self, value: u8) {
        self.0.push(value);
    }

    pub fn flag(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    pub fn number(&mut self, mut value: u64) {
        loop {
            let low = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                self.0.push(low);
                return;
            }
            self.0.push(low | 0x80);
        }
    }

    pub fn signed(&mut self, value: i64) {
        self.number(((value << 1) ^ (value >> 63)).cast_unsigned());
    }

    pub fn length(&mut self, length: usize) {
        self.number(u64::try_from(length).unwrap_or(u64::MAX));
    }

    pub fn bytes(&mut self, bytes: &[u8]) {
        self.length(bytes.len());
        self.0.extend_from_slice(bytes);
    }

    pub fn text(&mut self, text: &str) {
        self.bytes(within_the_longest_text(text).as_bytes());
    }

    pub fn fixed(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(bytes);
    }

    pub fn duration(&mut self, duration: Duration) {
        self.number(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
    }

    pub fn address(&mut self, address: SocketAddr) {
        self.ip(address.ip());
        self.number(u64::from(address.port()));
    }

    pub fn ip(&mut self, ip: IpAddr) {
        match ip {
            IpAddr::V4(ip) => {
                self.byte(4);
                self.fixed(&ip.octets());
            }
            IpAddr::V6(ip) => {
                self.byte(6);
                self.fixed(&ip.octets());
            }
        }
    }

    pub fn device(&mut self, device: DeviceId) {
        self.fixed(&device.0);
    }

    pub fn stamp(&mut self, stamp: Stamp) {
        self.signed(stamp.nanos);
        self.device(stamp.device);
    }

    pub fn recording(&mut self, recording: &Recording) {
        match &recording.mbid {
            Some(mbid) => {
                self.flag(true);
                self.text(mbid.as_str());
            }
            None => self.flag(false),
        }
        self.text(&recording.title);
        self.text(&recording.artist);
        self.text(&recording.album);
        match recording.length {
            Some(length) => {
                self.flag(true);
                self.duration(length);
            }
            None => self.flag(false),
        }
    }

    fn optional_text(&mut self, text: Option<&str>) {
        match text {
            Some(text) => {
                self.flag(true);
                self.text(text);
            }
            None => self.flag(false),
        }
    }

    fn optional_number(&mut self, number: Option<u32>) {
        match number {
            Some(number) => {
                self.flag(true);
                self.number(u64::from(number));
            }
            None => self.flag(false),
        }
    }

    fn optional_byte(&mut self, value: Option<u8>) {
        self.optional_number(value.map(u32::from));
    }

    fn figure(&mut self, figure: Figure) {
        self.fixed(&figure.bits().to_le_bytes());
    }

    fn optional_figure(&mut self, figure: Option<Figure>) {
        match figure {
            Some(figure) => {
                self.flag(true);
                self.figure(figure);
            }
            None => self.flag(false),
        }
    }

    pub fn whole_file(&mut self, file: WholeFile) {
        self.number(file.packets);
        self.number(file.frames);
        self.number(u64::from(file.rate));
    }

    pub fn print(&mut self, shared: &SharedPrint) {
        let encoded = shared.print.encoded();
        let end = encoded.len().min(LONGEST_SHARED_PRINT);
        self.bytes(&encoded.as_bytes()[..end]);
        self.number(shared.print.length().as_secs());
        match &shared.recognition {
            None => self.flag(false),
            Some(recognition) => {
                self.flag(true);
                self.recognition(recognition);
            }
        }
    }

    fn recognition(&mut self, recognition: &SharedRecognition) {
        match recognition {
            SharedRecognition::Unheard => self.byte(RECOGNITION_UNHEARD),
            SharedRecognition::Heard(heard) => {
                self.byte(RECOGNITION_HEARD);
                self.text(heard.recording.as_str());
                self.byte(heard.score);
                self.text(&heard.title);
                self.optional_text(heard.artist.as_deref());
            }
        }
    }

    pub fn lyricsfile(&mut self, lyricsfile: &SharedLyricsfile) {
        let end = lyricsfile.text.floor_char_boundary(LONGEST_SHARED_LYRICS);
        self.bytes(&lyricsfile.text.as_bytes()[..end]);
        self.flag(lyricsfile.synced);
        let end = lyricsfile
            .document
            .floor_char_boundary(LARGEST_SHARED_LYRICSFILE);
        self.bytes(&lyricsfile.document.as_bytes()[..end]);
    }

    pub fn answer(&mut self, answer: &Answer) {
        match answer {
            Answer::Lyrics(lyrics) => {
                self.byte(ANSWER_LYRICS);
                let end = lyrics.text.floor_char_boundary(LONGEST_SHARED_LYRICS);
                self.bytes(&lyrics.text.as_bytes()[..end]);
                self.flag(lyrics.synced);
            }
            Answer::Study(study) => {
                self.byte(ANSWER_STUDY);
                self.whole_file(study.file);
                self.number(u64::from(study.judged_under));
                self.text(&study.verdict);
                self.optional_number(study.cutoff_hz);
                self.optional_figure(study.cutoff_drop);
                self.optional_text(study.lossy_guess.as_deref());
                self.optional_number(study.upsampled_from);
                self.optional_byte(study.bits_in_use);
                self.optional_byte(study.declared_bits);
                self.figure(study.peak);
                self.figure(study.rms);
                self.figure(study.true_peak);
                self.optional_figure(study.loudness);
                self.optional_figure(study.loudness_range);
                self.optional_byte(study.dynamic_range);
                self.number(study.clipped);
                self.flag(study.mono_as_stereo);
                let end = study.band_levels.len().min(BAND_LEVELS_AT_MOST);
                self.bytes(&study.band_levels[..end]);
            }
            Answer::Release(release) => {
                self.byte(ANSWER_RELEASE);
                self.release(release);
            }
        }
    }

    fn optional_mbid(&mut self, mbid: Option<&Mbid>) {
        self.optional_text(mbid.map(Mbid::as_str));
    }

    fn links(&mut self, links: &[Link]) {
        let kept = &links[..links.len().min(LINKS_SHARED_AT_MOST)];
        self.length(kept.len());
        for link in kept {
            self.text(link.relation.spelled());
            self.text(&link.url);
        }
    }

    fn release(&mut self, release: &SharedRelease) {
        self.text(release.id.as_str());
        self.optional_mbid(release.group.as_ref());
        self.text(&release.title);
        self.length(release.credit.len());
        for credit in &release.credit {
            self.text(&credit.name);
            self.text(&credit.joined_by);
            self.optional_mbid(credit.mbid.as_ref());
        }
        for text in [
            &release.date,
            &release.country,
            &release.label,
            &release.catalog_number,
            &release.barcode,
            &release.kind,
            &release.disambiguation,
        ] {
            self.optional_text(text.as_deref());
        }
        self.flag(release.has_front_cover);
        self.links(&release.links);
        self.length(release.media.len());
        for medium in &release.media {
            self.number(u64::from(medium.position));
            self.optional_text(medium.format.as_deref());
            self.optional_text(medium.title.as_deref());
            self.length(medium.tracks.len());
            for track in &medium.tracks {
                self.track(track);
            }
        }
    }

    fn track(&mut self, track: &SharedTrack) {
        self.number(u64::from(track.position));
        self.text(&track.number);
        self.text(&track.title);
        self.optional_text(track.artist.as_deref());
        self.optional_mbid(track.recording.as_ref());
        self.optional_mbid(track.track.as_ref());
        self.optional_number(track.length_ms);
        let codes = &track.isrcs[..track.isrcs.len().min(CODES_SHARED_AT_MOST)];
        self.length(codes.len());
        for code in codes {
            self.text(code);
        }
        self.links(&track.links);
    }

    pub fn tags(&mut self, tags: &SongTags) {
        self.optional_text(tags.album_artist.as_deref());
        self.optional_text(tags.date.as_deref());
        self.optional_number(tags.track_number);
        self.optional_number(tags.track_total);
        self.optional_number(tags.disc_number);
        self.optional_number(tags.disc_total);
        self.flag(tags.compilation);
    }

    pub fn blobs(&mut self, blobs: &[Vec<u8>]) {
        self.length(blobs.len());
        for blob in blobs {
            self.bytes(blob);
        }
    }
}

fn within_the_longest_text(text: &str) -> &str {
    let end = text.floor_char_boundary(LONGEST_TEXT);
    &text[..end]
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    pub fn done(&self) -> Result<()> {
        if self.rest.is_empty() {
            Ok(())
        } else {
            Err(Error::Malformed(Malformed::Trailing {
                left: self.rest.len(),
            }))
        }
    }

    pub fn byte(&mut self) -> Result<u8> {
        let (first, rest) = self
            .rest
            .split_first()
            .ok_or(Error::Malformed(Malformed::Ended))?;
        self.rest = rest;
        Ok(*first)
    }

    pub fn flag(&mut self) -> Result<bool> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            tag => Err(Error::Malformed(Malformed::Tag { tag })),
        }
    }

    pub fn number(&mut self) -> Result<u64> {
        let mut value = 0_u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            let low = u64::from(byte & 0x7f);
            if shift == 63 && low > 1 {
                return Err(Error::Malformed(Malformed::Number));
            }
            value |= low << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(Error::Malformed(Malformed::Number))
    }

    pub fn signed(&mut self) -> Result<i64> {
        let zigzag = self.number()?;
        Ok((zigzag >> 1).cast_signed() ^ -((zigzag & 1).cast_signed()))
    }

    pub fn small(&mut self) -> Result<u32> {
        u32::try_from(self.number()?).map_err(|_| Error::Malformed(Malformed::Number))
    }

    pub fn length(&mut self, most: usize) -> Result<usize> {
        let length = usize::try_from(self.number()?).unwrap_or(usize::MAX);
        if length > most || length > self.rest.len() {
            return Err(Error::Malformed(Malformed::Overlong { length }));
        }
        Ok(length)
    }

    pub fn count(&mut self) -> Result<usize> {
        self.length(self.rest.len())
    }

    pub fn count_of(&mut self, each_at_least: usize) -> Result<usize> {
        self.length(self.rest.len() / each_at_least.max(1))
    }

    pub fn sealed(&mut self) -> Result<Vec<u8>> {
        let blob = self.bytes(LARGEST_BLOB)?;
        if blob.len() < SEALED_AT_LEAST {
            return Err(Error::Malformed(Malformed::Short { length: blob.len() }));
        }
        Ok(blob.to_vec())
    }

    pub fn fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        let Some((taken, rest)) = self.rest.split_first_chunk::<N>() else {
            return Err(Error::Malformed(Malformed::Ended));
        };
        self.rest = rest;
        Ok(*taken)
    }

    pub fn bytes(&mut self, most: usize) -> Result<&'a [u8]> {
        let length = self.length(most)?;
        let (taken, rest) = self.rest.split_at(length);
        self.rest = rest;
        Ok(taken)
    }

    pub fn text(&mut self) -> Result<String> {
        let bytes = self.bytes(LONGEST_TEXT)?;
        str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::Malformed(Malformed::NotText))
    }

    pub fn duration(&mut self) -> Result<Duration> {
        self.number().map(Duration::from_millis)
    }

    pub fn address(&mut self) -> Result<SocketAddr> {
        let ip = self.ip()?;
        let port =
            u16::try_from(self.number()?).map_err(|_| Error::Malformed(Malformed::Number))?;
        Ok(SocketAddr::new(ip, port))
    }

    pub fn ip(&mut self) -> Result<IpAddr> {
        match self.byte()? {
            4 => Ok(IpAddr::V4(Ipv4Addr::from(self.fixed::<4>()?))),
            6 => Ok(IpAddr::V6(Ipv6Addr::from(self.fixed::<16>()?))),
            tag => Err(Error::Malformed(Malformed::Tag { tag })),
        }
    }

    pub fn device(&mut self) -> Result<DeviceId> {
        self.fixed::<DEVICE_BYTES>().map(DeviceId)
    }

    pub fn stamp(&mut self) -> Result<Stamp> {
        let nanos = self.signed()?;
        let device = self.device()?;
        Ok(Stamp { nanos, device })
    }

    pub fn whole_file(&mut self) -> Result<WholeFile> {
        Ok(WholeFile {
            packets: self.number()?,
            frames: self.number()?,
            rate: self.small()?,
        })
    }

    pub fn print(&mut self) -> Result<SharedPrint> {
        let encoded = str::from_utf8(self.bytes(LONGEST_SHARED_PRINT)?)
            .map_err(|_| Error::Malformed(Malformed::NotText))?;
        let length = Duration::from_secs(self.number()?);
        let print = Chromaprint::new(encoded, length)
            .map_err(|_| Error::Malformed(Malformed::NotAPrint))?;
        let recognition = if self.flag()? {
            Some(self.recognition()?)
        } else {
            None
        };
        Ok(SharedPrint { print, recognition })
    }

    fn recognition(&mut self) -> Result<SharedRecognition> {
        match self.byte()? {
            RECOGNITION_UNHEARD => Ok(SharedRecognition::Unheard),
            RECOGNITION_HEARD => Ok(SharedRecognition::Heard(SharedHeard {
                recording: self.mbid()?,
                score: self.byte()?,
                title: self.text()?,
                artist: self.optional_text()?,
            })),
            tag => Err(Error::Malformed(Malformed::Tag { tag })),
        }
    }

    pub fn lyricsfile(&mut self) -> Result<SharedLyricsfile> {
        let text = str::from_utf8(self.bytes(LONGEST_SHARED_LYRICS)?)
            .map(str::to_owned)
            .map_err(|_| Error::Malformed(Malformed::NotText))?;
        let synced = self.flag()?;
        let document = str::from_utf8(self.bytes(LARGEST_SHARED_LYRICSFILE)?)
            .map(str::to_owned)
            .map_err(|_| Error::Malformed(Malformed::NotText))?;
        Ok(SharedLyricsfile {
            text,
            synced,
            document,
        })
    }

    pub fn recording(&mut self) -> Result<Recording> {
        let mbid = if self.flag()? {
            let text = self.text()?;
            Some(Mbid::new(&text).map_err(|_| Error::Malformed(Malformed::NotAnMbid))?)
        } else {
            None
        };
        let title = self.text()?;
        let artist = self.text()?;
        let album = self.text()?;
        let length = if self.flag()? {
            Some(self.duration()?)
        } else {
            None
        };
        Ok(Recording {
            mbid,
            title,
            artist,
            album,
            length,
        })
    }

    fn optional_text(&mut self) -> Result<Option<String>> {
        if self.flag()? {
            self.text().map(Some)
        } else {
            Ok(None)
        }
    }

    fn optional_number(&mut self) -> Result<Option<u32>> {
        if self.flag()? {
            self.small().map(Some)
        } else {
            Ok(None)
        }
    }

    fn optional_byte(&mut self) -> Result<Option<u8>> {
        self.optional_number()?
            .map(|value| u8::try_from(value).map_err(|_| Error::Malformed(Malformed::Number)))
            .transpose()
    }

    fn figure(&mut self) -> Result<Figure> {
        self.fixed::<4>()
            .map(|bytes| Figure::from_bits(u32::from_le_bytes(bytes)))
    }

    fn optional_figure(&mut self) -> Result<Option<Figure>> {
        if self.flag()? {
            self.figure().map(Some)
        } else {
            Ok(None)
        }
    }

    pub fn answer(&mut self) -> Result<Option<Answer>> {
        self.answer_among(ANSWER_KINDS)
    }

    fn answer_among(&mut self, known: &[u8]) -> Result<Option<Answer>> {
        let kind = self.byte()?;
        if !known.contains(&kind) {
            return Ok(None);
        }
        match kind {
            ANSWER_LYRICS => {
                let text = str::from_utf8(self.bytes(LONGEST_SHARED_LYRICS)?)
                    .map(str::to_owned)
                    .map_err(|_| Error::Malformed(Malformed::NotText))?;
                Ok(Some(Answer::Lyrics(SharedLyrics {
                    text,
                    synced: self.flag()?,
                })))
            }
            ANSWER_STUDY => {
                let file = self.whole_file()?;
                Ok(Some(Answer::Study(SharedStudy {
                    file,
                    judged_under: self.small()?,
                    verdict: self.text()?,
                    cutoff_hz: self.optional_number()?,
                    cutoff_drop: self.optional_figure()?,
                    lossy_guess: self.optional_text()?,
                    upsampled_from: self.optional_number()?,
                    bits_in_use: self.optional_byte()?,
                    declared_bits: self.optional_byte()?,
                    peak: self.figure()?,
                    rms: self.figure()?,
                    true_peak: self.figure()?,
                    loudness: self.optional_figure()?,
                    loudness_range: self.optional_figure()?,
                    dynamic_range: self.optional_byte()?,
                    clipped: self.number()?,
                    mono_as_stereo: self.flag()?,
                    band_levels: self.bytes(BAND_LEVELS_AT_MOST)?.to_vec(),
                })))
            }
            ANSWER_RELEASE => self
                .release()
                .map(|release| Some(Answer::Release(Box::new(release)))),
            _ => Ok(None),
        }
    }

    fn mbid(&mut self) -> Result<Mbid> {
        Mbid::new(&self.text()?).map_err(|_| Error::Malformed(Malformed::NotAnMbid))
    }

    fn optional_mbid(&mut self) -> Result<Option<Mbid>> {
        if self.flag()? {
            self.mbid().map(Some)
        } else {
            Ok(None)
        }
    }

    fn at_most(&mut self, each_at_least: usize, most: usize) -> Result<usize> {
        let count = self.count_of(each_at_least)?;
        if count > most {
            return Err(Error::Malformed(Malformed::Overlong { length: count }));
        }
        Ok(count)
    }

    fn links(&mut self) -> Result<Vec<Link>> {
        let count = self.at_most(LINK_AT_LEAST, LINKS_SHARED_AT_MOST)?;
        (0..count)
            .map(|_| {
                let relation = self.text()?;
                Ok(Link::new(&relation, self.text()?))
            })
            .collect()
    }

    fn release(&mut self) -> Result<SharedRelease> {
        let id = self.mbid()?;
        let group = self.optional_mbid()?;
        let title = self.text()?;
        let credits = self.count_of(CREDIT_AT_LEAST)?;
        let credit = (0..credits)
            .map(|_| {
                Ok(SharedCredit {
                    name: self.text()?,
                    joined_by: self.text()?,
                    mbid: self.optional_mbid()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let date = self.optional_text()?;
        let country = self.optional_text()?;
        let label = self.optional_text()?;
        let catalog_number = self.optional_text()?;
        let barcode = self.optional_text()?;
        let kind = self.optional_text()?;
        let disambiguation = self.optional_text()?;
        let has_front_cover = self.flag()?;
        let links = self.links()?;
        let media = self.count_of(MEDIUM_AT_LEAST)?;
        let media = (0..media)
            .map(|_| {
                let position = self.small()?;
                let format = self.optional_text()?;
                let title = self.optional_text()?;
                let tracks = self.count_of(TRACK_AT_LEAST)?;
                let tracks = (0..tracks)
                    .map(|_| self.track())
                    .collect::<Result<Vec<_>>>()?;
                Ok(SharedMedium {
                    position,
                    format,
                    title,
                    tracks,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SharedRelease {
            id,
            group,
            title,
            credit,
            date,
            country,
            label,
            catalog_number,
            barcode,
            kind,
            disambiguation,
            has_front_cover,
            links,
            media,
        })
    }

    fn track(&mut self) -> Result<SharedTrack> {
        let position = self.small()?;
        let number = self.text()?;
        let title = self.text()?;
        let artist = self.optional_text()?;
        let recording = self.optional_mbid()?;
        let track = self.optional_mbid()?;
        let length_ms = self.optional_number()?;
        let codes = self.at_most(CODE_AT_LEAST, CODES_SHARED_AT_MOST)?;
        let isrcs = (0..codes)
            .map(|_| self.text())
            .collect::<Result<Vec<_>>>()?;
        Ok(SharedTrack {
            position,
            number,
            title,
            artist,
            recording,
            track,
            length_ms,
            isrcs,
            links: self.links()?,
        })
    }

    pub fn tags(&mut self) -> Result<SongTags> {
        Ok(SongTags {
            album_artist: self.optional_text()?,
            date: self.optional_text()?,
            track_number: self.optional_number()?,
            track_total: self.optional_number()?,
            disc_number: self.optional_number()?,
            disc_total: self.optional_number()?,
            compilation: self.flag()?,
        })
    }

    pub fn blobs(&mut self) -> Result<Vec<Vec<u8>>> {
        let count = self.count_of(SEALED_AT_LEAST + 1)?;
        let mut blobs = Vec::with_capacity(count);
        for _ in 0..count {
            blobs.push(self.sealed()?);
        }
        Ok(blobs)
    }
}

impl ToServer {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Hello {
                version,
                space,
                token,
                device,
                after,
            } => {
                writer.byte(0);
                writer.number(u64::from(*version));
                writer.fixed(&space.0);
                writer.fixed(&token.0);
                writer.device(*device);
                writer.number(*after);
            }
            Self::Append { blobs } => {
                writer.byte(1);
                writer.blobs(blobs);
            }
            Self::Compact { through, parts } => {
                writer.byte(2);
                writer.number(*through);
                writer.blobs(parts);
            }
            Self::Punch { to, sealed } => {
                writer.byte(3);
                writer.device(*to);
                writer.bytes(sealed);
            }
            Self::Bye => writer.byte(4),
            Self::Relaying => writer.byte(5),
            Self::Relay { to, datagram } => {
                writer.byte(6);
                writer.device(*to);
                writer.bytes(datagram);
            }
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let message = match reader.byte()? {
            0 => Self::Hello {
                version: u16::try_from(reader.number()?)
                    .map_err(|_| Error::Malformed(Malformed::Number))?,
                space: SpaceId(reader.fixed::<SPACE_BYTES>()?),
                token: Token(reader.fixed::<TOKEN_BYTES>()?),
                device: reader.device()?,
                after: reader.number()?,
            },
            1 => Self::Append {
                blobs: reader.blobs()?,
            },
            2 => Self::Compact {
                through: reader.number()?,
                parts: reader.blobs()?,
            },
            3 => Self::Punch {
                to: reader.device()?,
                sealed: reader.bytes(LONGEST_TEXT)?.to_vec(),
            },
            4 => Self::Bye,
            5 => Self::Relaying,
            6 => Self::Relay {
                to: reader.device()?,
                datagram: reader.bytes(LARGEST_DATAGRAM)?.to_vec(),
            },
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;
        Ok(message)
    }
}

impl Refusal {
    const fn tag(self) -> u8 {
        match self {
            Self::Version => 0,
            Self::Token => 1,
            Self::TooLarge => 2,
            Self::SpaceFull => 3,
            Self::TooFast => 4,
            Self::Busy => 5,
            Self::Stale => 6,
            Self::Unknown => 255,
        }
    }

    const fn from_tag(tag: u8) -> Self {
        match tag {
            0 => Self::Version,
            1 => Self::Token,
            2 => Self::TooLarge,
            3 => Self::SpaceFull,
            4 => Self::TooFast,
            5 => Self::Busy,
            6 => Self::Stale,
            _ => Self::Unknown,
        }
    }
}

impl FromServer {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Welcome {
                online,
                seen_at,
                head,
            } => {
                writer.byte(0);
                writer.length(online.len());
                for device in online {
                    writer.device(*device);
                }
                writer.address(*seen_at);
                writer.number(*head);
            }
            Self::Snapshot { through, parts } => {
                writer.byte(1);
                writer.number(*through);
                writer.blobs(parts);
            }
            Self::Records { entries } => {
                writer.byte(2);
                writer.length(entries.len());
                for entry in entries {
                    writer.number(entry.seq);
                    match &entry.blob {
                        Some(blob) => {
                            writer.flag(true);
                            writer.bytes(blob);
                        }
                        None => writer.flag(false),
                    }
                }
            }
            Self::Appended { count } => {
                writer.byte(3);
                writer.number(u64::from(*count));
            }
            Self::Arrived { device } => {
                writer.byte(4);
                writer.device(*device);
            }
            Self::Left { device } => {
                writer.byte(5);
                writer.device(*device);
            }
            Self::Punched {
                device,
                address,
                sealed,
            } => {
                writer.byte(6);
                writer.device(*device);
                writer.address(*address);
                writer.bytes(sealed);
            }
            Self::Compacted { through } => {
                writer.byte(7);
                writer.number(*through);
            }
            Self::Refused(refusal) => {
                writer.byte(8);
                writer.byte(refusal.tag());
            }
            Self::Relayed { from, datagram } => {
                writer.byte(9);
                writer.device(*from);
                writer.bytes(datagram);
            }
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let message = match reader.byte()? {
            0 => {
                let count = reader.count()?;
                let mut online = Vec::with_capacity(count);
                for _ in 0..count {
                    online.push(reader.device()?);
                }
                Self::Welcome {
                    online,
                    seen_at: reader.address()?,
                    head: reader.number()?,
                }
            }
            1 => Self::Snapshot {
                through: reader.number()?,
                parts: reader.blobs()?,
            },
            2 => {
                let count = reader.count_of(RECORD_AT_LEAST)?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let seq = reader.number()?;
                    let blob = if reader.flag()? {
                        Some(reader.sealed()?)
                    } else {
                        None
                    };
                    entries.push(Record { seq, blob });
                }
                Self::Records { entries }
            }
            3 => Self::Appended {
                count: reader.small()?,
            },
            4 => Self::Arrived {
                device: reader.device()?,
            },
            5 => Self::Left {
                device: reader.device()?,
            },
            6 => Self::Punched {
                device: reader.device()?,
                address: reader.address()?,
                sealed: reader.bytes(LONGEST_TEXT)?.to_vec(),
            },
            7 => Self::Compacted {
                through: reader.number()?,
            },
            8 => Self::Refused(Refusal::from_tag(reader.byte()?)),
            9 => Self::Relayed {
                from: reader.device()?,
                datagram: reader.bytes(LARGEST_DATAGRAM)?.to_vec(),
            },
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;
        Ok(message)
    }
}

impl ToPeer {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Holdings => writer.byte(0),
            Self::Fetch { recording, offset } => {
                writer.byte(1);
                writer.recording(recording);
                writer.number(*offset);
            }
            Self::Meet { device, records } => {
                writer.byte(2);
                writer.device(*device);
                writer.flag(*records);
            }
            Self::Changes { whole, changes } => {
                writer.byte(3);
                writer.flag(*whole);
                writer.bytes(&encoded_bundle(changes));
            }
            Self::Tags { recording } => {
                writer.byte(4);
                writer.recording(recording);
            }
            Self::Covers { releases } => {
                writer.byte(5);
                writer.length(releases.len());
                for release in releases {
                    writer.text(release.as_str());
                }
            }
            Self::Lyricsfiles { recordings } => {
                writer.byte(6);
                writer.length(recordings.len());
                for recording in recordings {
                    writer.recording(recording);
                }
            }
            Self::Prints { files } => {
                writer.byte(7);
                writer.length(files.len());
                for file in files {
                    writer.whole_file(*file);
                }
            }
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let message = match reader.byte()? {
            0 => Self::Holdings,
            1 => Self::Fetch {
                recording: reader.recording()?,
                offset: reader.number()?,
            },
            2 => Self::Meet {
                device: reader.device()?,
                records: reader.flag()?,
            },
            3 => Self::Changes {
                whole: reader.flag()?,
                changes: decoded_bundle(reader.bytes(LARGEST_FRAME)?)?,
            },
            4 => Self::Tags {
                recording: reader.recording()?,
            },
            5 => {
                let count = reader.count_of(MBID_AT_LEAST)?;
                if count > COVERS_ASKED_AT_MOST {
                    return Err(Error::Malformed(Malformed::Overlong { length: count }));
                }
                let mut releases = Vec::with_capacity(count);
                for _ in 0..count {
                    releases.push(reader.mbid()?);
                }
                Self::Covers { releases }
            }
            6 => {
                let count = reader.count_of(RECORDING_AT_LEAST)?;
                if count > LYRICSFILES_ASKED_AT_MOST {
                    return Err(Error::Malformed(Malformed::Overlong { length: count }));
                }
                let mut recordings = Vec::with_capacity(count);
                for _ in 0..count {
                    recordings.push(reader.recording()?);
                }
                Self::Lyricsfiles { recordings }
            }
            7 => {
                let count = reader.count_of(WHOLE_FILE_AT_LEAST)?;
                if count > PRINTS_ASKED_AT_MOST {
                    return Err(Error::Malformed(Malformed::Overlong { length: count }));
                }
                let mut files = Vec::with_capacity(count);
                for _ in 0..count {
                    files.push(reader.whole_file()?);
                }
                Self::Prints { files }
            }
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;
        Ok(message)
    }
}

impl FromPeer {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        match self {
            Self::Held { items } => {
                writer.byte(0);
                writer.length(items.len());
                for item in items {
                    writer.recording(&item.recording);
                    writer.number(item.size);
                    writer.text(&item.extension);
                }
            }
            Self::HeldEnd => writer.byte(1),
            Self::Sending { size, extension } => {
                writer.byte(2);
                writer.number(*size);
                writer.text(extension);
            }
            Self::Missing => writer.byte(3),
            Self::Met { device } => {
                writer.byte(4);
                writer.device(*device);
            }
            Self::Taken => writer.byte(5),
            Self::Tagged { tags } => {
                writer.byte(6);
                writer.tags(tags);
            }
            Self::Covered { release, picture } => {
                writer.byte(7);
                writer.text(release.as_str());
                writer.bytes(picture);
            }
            Self::CoversEnd => writer.byte(8),
            Self::Documented {
                recording,
                lyricsfile,
            } => {
                writer.byte(9);
                writer.recording(recording);
                writer.lyricsfile(lyricsfile);
            }
            Self::LyricsfilesEnd => writer.byte(10),
            Self::Printed { file, print } => {
                writer.byte(11);
                writer.whole_file(*file);
                writer.print(print);
            }
            Self::PrintsEnd => writer.byte(12),
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let message = match reader.byte()? {
            0 => {
                let count = reader.count()?;
                let mut items = Vec::with_capacity(count);
                for _ in 0..count {
                    items.push(Held {
                        recording: reader.recording()?,
                        size: reader.number()?,
                        extension: reader.text()?,
                    });
                }
                Self::Held { items }
            }
            1 => Self::HeldEnd,
            2 => Self::Sending {
                size: reader.number()?,
                extension: reader.text()?,
            },
            3 => Self::Missing,
            4 => Self::Met {
                device: reader.device()?,
            },
            5 => Self::Taken,
            6 => Self::Tagged {
                tags: reader.tags()?,
            },
            7 => Self::Covered {
                release: reader.mbid()?,
                picture: reader.bytes(LARGEST_SHARED_COVER)?.to_vec(),
            },
            8 => Self::CoversEnd,
            9 => Self::Documented {
                recording: reader.recording()?,
                lyricsfile: reader.lyricsfile()?,
            },
            10 => Self::LyricsfilesEnd,
            11 => Self::Printed {
                file: reader.whole_file()?,
                print: reader.print()?,
            },
            12 => Self::PrintsEnd,
            tag => return Err(Error::Malformed(Malformed::Tag { tag })),
        };
        reader.done()?;
        Ok(message)
    }
}

impl Beacon {
    pub fn encoded(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.device(self.device);
        writer.number(u64::from(self.port));
        writer.flag(self.answering);
        writer.length(self.addresses.len());
        for address in &self.addresses {
            writer.ip(*address);
        }
        writer.finish()
    }

    pub fn decoded(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let beacon = Self {
            device: reader.device()?,
            port: u16::try_from(reader.number()?)
                .map_err(|_| Error::Malformed(Malformed::Number))?,
            answering: reader.flag()?,
            addresses: {
                let count = reader.count()?;
                let mut addresses = Vec::with_capacity(count);
                for _ in 0..count {
                    addresses.push(reader.ip()?);
                }
                addresses
            },
        };
        reader.done()?;
        Ok(beacon)
    }
}

pub fn encoded_candidates(candidates: &[SocketAddr]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(candidates.len());
    for candidate in candidates {
        writer.address(*candidate);
    }
    writer.finish()
}

pub fn decoded_candidates(bytes: &[u8]) -> Result<Vec<SocketAddr>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count()?;
    let mut candidates = Vec::with_capacity(count);
    for _ in 0..count {
        candidates.push(reader.address()?);
    }
    reader.done()?;
    Ok(candidates)
}

const PLATFORM_LINUX: u8 = 0;
const PLATFORM_ANDROID: u8 = 1;
const PLATFORM_IOS: u8 = 2;
const PLATFORM_OTHER: u8 = 255;

pub fn encoded_bundle(changes: &[Change]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(changes.len());
    for change in changes {
        writer.bytes(&encoded_change(change));
    }
    writer.finish()
}

pub fn encoded_change(change: &Change) -> Vec<u8> {
    let mut writer = Writer::new();
    match change {
        Change::Device(note) => {
            writer.byte(0);
            writer.device(note.device);
            writer.text(&note.name);
            writer.byte(match note.platform {
                Platform::Linux => PLATFORM_LINUX,
                Platform::Android => PLATFORM_ANDROID,
                Platform::Ios => PLATFORM_IOS,
                Platform::Other => PLATFORM_OTHER,
            });
            writer.stamp(note.stamp);
        }
        Change::Playlist(note) => {
            writer.byte(1);
            writer.fixed(&note.key.0);
            writer.stamp(note.stamp);
            match &note.body {
                Some(body) => {
                    writer.flag(true);
                    writer.text(&body.name);
                    writer.length(body.entries.len());
                    for entry in &body.entries {
                        writer.recording(entry);
                    }
                }
                None => writer.flag(false),
            }
        }
        Change::Favourite(note) => {
            writer.byte(2);
            writer.recording(&note.recording);
            writer.flag(note.favoured);
            writer.stamp(note.stamp);
        }
        Change::Listen(listen) => {
            writer.byte(3);
            writer.device(listen.device);
            writer.number(listen.id);
            writer.recording(&listen.recording);
            writer.signed(listen.at);
            writer.duration(listen.heard);
        }
        Change::Handoff(handoff) => {
            writer.byte(4);
            writer.device(handoff.device);
            writer.stamp(handoff.stamp);
            writer.length(handoff.rows.len());
            for row in &handoff.rows {
                writer.recording(row);
            }
            writer.number(u64::from(handoff.at));
            writer.duration(handoff.position);
            writer.flag(handoff.playing);
        }
        Change::Forgotten(forgotten) => {
            writer.byte(5);
            writer.device(forgotten.device);
            writer.stamp(forgotten.stamp);
        }
        Change::Query(note) => {
            writer.byte(6);
            writer.fixed(&note.key.0);
            writer.stamp(note.stamp);
            match &note.body {
                Some(body) => {
                    writer.flag(true);
                    writer.text(&body.name);
                    match &body.text {
                        Some(text) => {
                            writer.flag(true);
                            writer.text(text);
                        }
                        None => writer.flag(false),
                    }
                    writer.byte(body.sort);
                    writer.byte(body.reading);
                    match body.rows {
                        Some(rows) => {
                            writer.flag(true);
                            writer.number(u64::from(rows));
                        }
                        None => writer.flag(false),
                    }
                }
                None => writer.flag(false),
            }
        }
        Change::Answer(note) => {
            writer.byte(7);
            writer.recording(&note.recording);
            writer.stamp(note.stamp);
            writer.answer(&note.answer);
        }
        Change::PlaylistFolded(folded) => {
            writer.byte(8);
            writer.fixed(&folded.folded.0);
            writer.fixed(&folded.into.0);
            writer.length(folded.entries.len());
            for entry in &folded.entries {
                writer.recording(entry);
            }
        }
        Change::QueryFolded(folded) => {
            writer.byte(9);
            writer.fixed(&folded.folded.0);
            writer.fixed(&folded.into.0);
        }
    }
    writer.finish()
}

pub fn decoded_bundle(bytes: &[u8]) -> Result<Vec<Change>> {
    decoded_bundle_knowing(bytes, ANSWER_KINDS)
}

fn decoded_bundle_knowing(bytes: &[u8], answers: &[u8]) -> Result<Vec<Change>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count()?;
    let mut changes = Vec::with_capacity(count);
    for _ in 0..count {
        let encoded = reader.bytes(LARGEST_BLOB)?;
        if let Some(change) = decoded_change(encoded, answers)? {
            changes.push(change);
        }
    }
    reader.done()?;
    Ok(changes)
}

fn decoded_change(bytes: &[u8], answers: &[u8]) -> Result<Option<Change>> {
    let mut reader = Reader::new(bytes);
    let change = match reader.byte()? {
        0 => Change::Device(DeviceNote {
            device: reader.device()?,
            name: reader.text()?,
            platform: match reader.byte()? {
                PLATFORM_LINUX => Platform::Linux,
                PLATFORM_ANDROID => Platform::Android,
                PLATFORM_IOS => Platform::Ios,
                _ => Platform::Other,
            },
            stamp: reader.stamp()?,
        }),
        1 => {
            let key = PlaylistKey(reader.fixed()?);
            let stamp = reader.stamp()?;
            let body = if reader.flag()? {
                let name = reader.text()?;
                let count = reader.count()?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    entries.push(reader.recording()?);
                }
                Some(PlaylistBody { name, entries })
            } else {
                None
            };
            Change::Playlist(PlaylistNote { key, stamp, body })
        }
        2 => Change::Favourite(FavouriteNote {
            recording: reader.recording()?,
            favoured: reader.flag()?,
            stamp: reader.stamp()?,
        }),
        3 => Change::Listen(Listen {
            device: reader.device()?,
            id: reader.number()?,
            recording: reader.recording()?,
            at: reader.signed()?,
            heard: reader.duration()?,
        }),
        4 => {
            let device = reader.device()?;
            let stamp = reader.stamp()?;
            let count = reader.count()?;
            let mut rows = Vec::with_capacity(count);
            for _ in 0..count {
                rows.push(reader.recording()?);
            }
            Change::Handoff(Handoff {
                device,
                stamp,
                rows,
                at: reader.small()?,
                position: reader.duration()?,
                playing: reader.flag()?,
            })
        }
        5 => Change::Forgotten(DeviceForgotten {
            device: reader.device()?,
            stamp: reader.stamp()?,
        }),
        6 => {
            let key = PlaylistKey(reader.fixed()?);
            let stamp = reader.stamp()?;
            let body = if reader.flag()? {
                let name = reader.text()?;
                let text = if reader.flag()? {
                    Some(reader.text()?)
                } else {
                    None
                };
                let sort = reader.byte()?;
                let reading = reader.byte()?;
                let rows = if reader.flag()? {
                    Some(reader.small()?)
                } else {
                    None
                };
                Some(QueryBody {
                    name,
                    text,
                    sort,
                    reading,
                    rows,
                })
            } else {
                None
            };
            Change::Query(QueryNote { key, stamp, body })
        }
        7 => {
            let recording = reader.recording()?;
            let stamp = reader.stamp()?;
            let Some(answer) = reader.answer_among(answers)? else {
                return Ok(None);
            };
            Change::Answer(AnswerNote {
                recording,
                stamp,
                answer,
            })
        }
        8 => {
            let folded = PlaylistKey(reader.fixed()?);
            let into = PlaylistKey(reader.fixed()?);
            let count = reader.count()?;
            let mut entries = Vec::with_capacity(count);
            for _ in 0..count {
                entries.push(reader.recording()?);
            }
            Change::PlaylistFolded(PlaylistFolded {
                folded,
                into,
                entries,
            })
        }
        9 => Change::QueryFolded(QueryFolded {
            folded: PlaylistKey(reader.fixed()?),
            into: PlaylistKey(reader.fixed()?),
        }),
        _ => return Ok(None),
    };
    reader.done()?;
    Ok(Some(change))
}

pub fn encoded_recordings(recordings: &[Recording]) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.length(recordings.len());
    for recording in recordings {
        writer.recording(recording);
    }
    writer.finish()
}

pub fn decoded_recordings(bytes: &[u8]) -> Result<Vec<Recording>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count()?;
    let mut recordings = Vec::with_capacity(count);
    for _ in 0..count {
        recordings.push(reader.recording()?);
    }
    reader.done()?;
    Ok(recordings)
}

pub fn encoded_answer(answer: &Answer) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.answer(answer);
    writer.finish()
}

pub fn decoded_answer(bytes: &[u8]) -> Result<Option<Answer>> {
    let mut reader = Reader::new(bytes);
    let answer = reader.answer()?;
    if answer.is_some() {
        reader.done()?;
    }
    Ok(answer)
}

pub fn encoded_recording(recording: &Recording) -> Vec<u8> {
    let mut writer = Writer::new();
    writer.recording(recording);
    writer.finish()
}

pub fn decoded_recording(bytes: &[u8]) -> Result<Recording> {
    let mut reader = Reader::new(bytes);
    let recording = reader.recording()?;
    reader.done()?;
    Ok(recording)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn song(title: &str) -> Recording {
        Recording {
            mbid: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("mbid")),
            title: title.into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: Some(Duration::from_millis(1_403_000)),
        }
    }

    fn stamp() -> Stamp {
        Stamp {
            nanos: -5,
            device: DeviceId([7; DEVICE_BYTES]),
        }
    }

    #[test]
    fn every_frame_to_the_server_reads_back_as_written() {
        for frame in [
            ToServer::Hello {
                version: PROTOCOL_VERSION,
                space: SpaceId([1; SPACE_BYTES]),
                token: Token([2; TOKEN_BYTES]),
                device: DeviceId([3; DEVICE_BYTES]),
                after: 300,
            },
            ToServer::Append {
                blobs: vec![vec![1; SEALED_AT_LEAST], vec![2; 300]],
            },
            ToServer::Compact {
                through: u64::MAX,
                parts: vec![vec![9; 300]],
            },
            ToServer::Punch {
                to: DeviceId([4; DEVICE_BYTES]),
                sealed: vec![5; 40],
            },
            ToServer::Bye,
            ToServer::Relaying,
            ToServer::Relay {
                to: DeviceId([6; DEVICE_BYTES]),
                datagram: vec![7; 1452],
            },
        ] {
            assert_eq!(ToServer::decoded(&frame.encoded()).expect("decoded"), frame);
        }
    }

    #[test]
    fn a_datagram_past_what_a_relay_carries_is_refused() {
        let past = ToServer::Relay {
            to: DeviceId([6; DEVICE_BYTES]),
            datagram: vec![7; LARGEST_DATAGRAM + 1],
        };
        let fits = ToServer::Relay {
            to: DeviceId([6; DEVICE_BYTES]),
            datagram: vec![7; LARGEST_DATAGRAM],
        };

        assert!(ToServer::decoded(&past.encoded()).is_err());
        assert_eq!(ToServer::decoded(&fits.encoded()).expect("decoded"), fits);
    }

    #[test]
    fn a_blob_too_short_to_be_sealed_or_a_count_past_what_could_follow_is_refused() {
        let short = ToServer::Append {
            blobs: vec![vec![1; SEALED_AT_LEAST - 1]],
        };
        let mut writer = Writer::new();
        writer.byte(1);
        writer.number(u64::try_from(LARGEST_FRAME).expect("a count"));
        writer.0.extend(std::iter::repeat_n(0, 1024));

        assert!(matches!(
            ToServer::decoded(&short.encoded()),
            Err(Error::Malformed(Malformed::Short { .. }))
        ));
        assert!(matches!(
            ToServer::decoded(&writer.0),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
    }

    #[test]
    fn every_frame_from_the_server_reads_back_as_written() {
        for frame in [
            FromServer::Welcome {
                online: vec![DeviceId([1; DEVICE_BYTES])],
                seen_at: "203.0.113.9:40000".parse().expect("address"),
                head: 12,
            },
            FromServer::Snapshot {
                through: 3,
                parts: vec![vec![1; SEALED_AT_LEAST]],
            },
            FromServer::Records {
                entries: vec![
                    Record {
                        seq: 4,
                        blob: Some(vec![1; 40]),
                    },
                    Record { seq: 5, blob: None },
                ],
            },
            FromServer::Appended { count: 2 },
            FromServer::Arrived {
                device: DeviceId([2; DEVICE_BYTES]),
            },
            FromServer::Left {
                device: DeviceId([2; DEVICE_BYTES]),
            },
            FromServer::Punched {
                device: DeviceId([3; DEVICE_BYTES]),
                address: "[2001:db8::1]:443".parse().expect("address"),
                sealed: vec![8; 12],
            },
            FromServer::Compacted { through: 7 },
            FromServer::Refused(Refusal::SpaceFull),
            FromServer::Relayed {
                from: DeviceId([5; DEVICE_BYTES]),
                datagram: vec![9; 1452],
            },
        ] {
            assert_eq!(
                FromServer::decoded(&frame.encoded()).expect("decoded"),
                frame
            );
        }
    }

    #[test]
    fn peer_frames_read_back_as_written() {
        let fetch = ToPeer::Fetch {
            recording: song("Echoes"),
            offset: 1 << 40,
        };
        let held = FromPeer::Held {
            items: vec![Held {
                recording: song("One of These Days"),
                size: 30_000_000,
                extension: "flac".into(),
            }],
        };

        let meet = ToPeer::Meet {
            device: DeviceId([3; DEVICE_BYTES]),
            records: true,
        };
        let changes = ToPeer::Changes {
            whole: true,
            changes: vec![Change::Favourite(FavouriteNote {
                recording: song("Time"),
                favoured: true,
                stamp: stamp(),
            })],
        };
        let met = FromPeer::Met {
            device: DeviceId([4; DEVICE_BYTES]),
        };
        let asked = ToPeer::Tags {
            recording: song("Echoes"),
        };
        let tagged = FromPeer::Tagged {
            tags: SongTags {
                album_artist: Some("Pink Floyd".into()),
                date: Some("1971-10-30".into()),
                track_number: Some(6),
                track_total: Some(6),
                disc_number: Some(1),
                disc_total: Some(2),
                compilation: true,
            },
        };

        assert_eq!(ToPeer::decoded(&fetch.encoded()).expect("decoded"), fetch);
        assert_eq!(ToPeer::decoded(&meet.encoded()).expect("decoded"), meet);
        assert_eq!(
            ToPeer::decoded(&changes.encoded()).expect("decoded"),
            changes
        );
        assert_eq!(FromPeer::decoded(&met.encoded()).expect("decoded"), met);
        assert_eq!(
            FromPeer::decoded(&FromPeer::Taken.encoded()).expect("decoded"),
            FromPeer::Taken
        );
        assert_eq!(ToPeer::decoded(&asked.encoded()).expect("decoded"), asked);
        assert_eq!(
            FromPeer::decoded(&tagged.encoded()).expect("decoded"),
            tagged
        );
        assert_eq!(
            FromPeer::decoded(
                &FromPeer::Tagged {
                    tags: SongTags::default()
                }
                .encoded()
            )
            .expect("decoded"),
            FromPeer::Tagged {
                tags: SongTags::default()
            }
        );
        assert_eq!(FromPeer::decoded(&held.encoded()).expect("decoded"), held);
        assert_eq!(
            FromPeer::decoded(&FromPeer::Missing.encoded()).expect("decoded"),
            FromPeer::Missing
        );
    }

    #[test]
    fn the_tags_of_a_song_travel_in_frames_an_earlier_build_refuses_whole_and_the_fetch_is_unchanged()
     {
        let asked = ToPeer::Tags {
            recording: song("Echoes"),
        }
        .encoded();
        let tagged = FromPeer::Tagged {
            tags: SongTags::default(),
        }
        .encoded();
        let fetch = ToPeer::Fetch {
            recording: song("Echoes"),
            offset: 0,
        }
        .encoded();
        let sending = FromPeer::Sending {
            size: 9,
            extension: "flac".into(),
        }
        .encoded();

        assert_eq!(asked[0], 4);
        assert_eq!(tagged[0], 6);
        assert_eq!(fetch[0], 1);
        assert_eq!(sending, [2, 9, 4, b'f', b'l', b'a', b'c']);
    }

    #[test]
    fn covers_asked_of_a_device_read_back_and_too_many_asked_at_once_are_refused() {
        let release = Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid");
        let asked = ToPeer::Covers {
            releases: vec![release.clone(); 3],
        };
        let covered = FromPeer::Covered {
            release: release.clone(),
            picture: vec![0xff, 0xd8, 0xff, 0xe0],
        };
        let too_many = ToPeer::Covers {
            releases: vec![release; COVERS_ASKED_AT_MOST + 1],
        };

        assert_eq!(ToPeer::decoded(&asked.encoded()).expect("decoded"), asked);
        assert_eq!(
            FromPeer::decoded(&covered.encoded()).expect("decoded"),
            covered
        );
        assert_eq!(
            FromPeer::decoded(&FromPeer::CoversEnd.encoded()).expect("decoded"),
            FromPeer::CoversEnd
        );
        assert_eq!(asked.encoded()[0], 5);
        assert!(matches!(
            ToPeer::decoded(&too_many.encoded()),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
    }

    #[test]
    fn lyricsfiles_asked_of_a_device_read_back_and_too_many_asked_at_once_are_refused() {
        let recording = Recording {
            mbid: Some(Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid")),
            title: "Echoes".into(),
            artist: "Pink Floyd".into(),
            album: "Meddle".into(),
            length: Some(Duration::from_secs(1_400)),
        };
        let asked = ToPeer::Lyricsfiles {
            recordings: vec![recording.clone(); 3],
        };
        let documented = FromPeer::Documented {
            recording: recording.clone(),
            lyricsfile: SharedLyricsfile {
                text: "[00:01.00]Overhead".into(),
                synced: true,
                document: "version: '1.0'".into(),
            },
        };
        let too_many = ToPeer::Lyricsfiles {
            recordings: vec![recording; LYRICSFILES_ASKED_AT_MOST + 1],
        };

        assert_eq!(ToPeer::decoded(&asked.encoded()).expect("decoded"), asked);
        assert_eq!(
            FromPeer::decoded(&documented.encoded()).expect("decoded"),
            documented
        );
        assert_eq!(
            FromPeer::decoded(&FromPeer::LyricsfilesEnd.encoded()).expect("decoded"),
            FromPeer::LyricsfilesEnd
        );
        assert_eq!(asked.encoded()[0], 6);
        assert!(matches!(
            ToPeer::decoded(&too_many.encoded()),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
    }

    fn printed_file() -> WholeFile {
        WholeFile {
            packets: 0xfeed,
            frames: 8_000_000,
            rate: 44_100,
        }
    }

    fn shared_print(recognition: Option<SharedRecognition>) -> SharedPrint {
        SharedPrint {
            print: Chromaprint::new("AQADtEmSJEmSJEmS", Duration::from_secs(181)).expect("a print"),
            recognition,
        }
    }

    #[test]
    fn prints_asked_of_a_device_read_back_and_too_many_asked_at_once_are_refused() {
        let asked = ToPeer::Prints {
            files: vec![printed_file(); 3],
        };
        let too_many = ToPeer::Prints {
            files: vec![printed_file(); PRINTS_ASKED_AT_MOST + 1],
        };

        assert_eq!(ToPeer::decoded(&asked.encoded()).expect("decoded"), asked);
        assert_eq!(asked.encoded()[0], 7);
        assert!(matches!(
            ToPeer::decoded(&too_many.encoded()),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
        assert_eq!(
            FromPeer::decoded(&FromPeer::PrintsEnd.encoded()).expect("decoded"),
            FromPeer::PrintsEnd
        );
    }

    #[test]
    fn a_print_reads_back_with_the_recognition_it_carries_or_none() {
        let heard = |artist: Option<&str>| {
            SharedRecognition::Heard(SharedHeard {
                recording: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid"),
                score: 96,
                title: "Echoes".into(),
                artist: artist.map(str::to_owned),
            })
        };
        for recognition in [
            None,
            Some(SharedRecognition::Unheard),
            Some(heard(Some("Pink Floyd"))),
            Some(heard(None)),
        ] {
            let printed = FromPeer::Printed {
                file: printed_file(),
                print: shared_print(recognition),
            };

            assert_eq!(
                FromPeer::decoded(&printed.encoded()).expect("decoded"),
                printed
            );
        }
    }

    #[test]
    fn a_print_that_is_not_one_or_a_recognition_of_a_kind_unknown_is_refused() {
        let printed = FromPeer::Printed {
            file: printed_file(),
            print: shared_print(Some(SharedRecognition::Unheard)),
        }
        .encoded();
        let mut not_a_print = printed.clone();
        let at = not_a_print
            .iter()
            .position(|byte| *byte == b'A')
            .expect("the print");
        not_a_print[at] = b'+';
        let mut unknown_kind = printed;
        let last = unknown_kind.len() - 1;
        unknown_kind[last] = 9;

        assert!(matches!(
            FromPeer::decoded(&not_a_print),
            Err(Error::Malformed(Malformed::NotAPrint))
        ));
        assert!(matches!(
            FromPeer::decoded(&unknown_kind),
            Err(Error::Malformed(Malformed::Tag { tag: 9 }))
        ));
    }

    #[test]
    fn the_seeds_of_a_print_asked_and_given_decode() {
        let asked = include_bytes!("../../../fuzz/seeds/sync/prints");
        let given = include_bytes!("../../../fuzz/seeds/sync/printed");

        assert!(matches!(
            ToPeer::decoded(asked),
            Ok(ToPeer::Prints { files }) if files.len() == 2
        ));
        assert!(matches!(
            FromPeer::decoded(given),
            Ok(FromPeer::Printed { file, print })
                if file == printed_file()
                    && matches!(print.recognition, Some(SharedRecognition::Heard(_)))
        ));
    }

    #[test]
    fn a_bundle_of_every_change_reads_back_and_an_unknown_change_is_skipped() {
        let changes = vec![
            Change::Device(DeviceNote {
                device: DeviceId([1; DEVICE_BYTES]),
                name: "Desk".into(),
                platform: Platform::Linux,
                stamp: stamp(),
            }),
            Change::Playlist(PlaylistNote {
                key: PlaylistKey([2; 16]),
                stamp: stamp(),
                body: Some(PlaylistBody {
                    name: "Summer".into(),
                    entries: vec![song("Echoes"), song("Fearless")],
                }),
            }),
            Change::Playlist(PlaylistNote {
                key: PlaylistKey([3; 16]),
                stamp: stamp(),
                body: None,
            }),
            Change::Favourite(FavouriteNote {
                recording: song("Echoes"),
                favoured: false,
                stamp: stamp(),
            }),
            Change::Listen(Listen {
                device: DeviceId([1; DEVICE_BYTES]),
                id: 99,
                recording: song("Echoes"),
                at: 1_700_000_000_000_000_000,
                heard: Duration::from_secs(60),
            }),
            Change::Handoff(Handoff {
                device: DeviceId([1; DEVICE_BYTES]),
                stamp: stamp(),
                rows: vec![song("Echoes")],
                at: 0,
                position: Duration::from_secs(3),
                playing: true,
            }),
            Change::Forgotten(DeviceForgotten {
                device: DeviceId([4; DEVICE_BYTES]),
                stamp: stamp(),
            }),
            Change::Query(QueryNote {
                key: PlaylistKey([5; 16]),
                stamp: stamp(),
                body: Some(QueryBody {
                    name: "Recent".into(),
                    text: Some("added:<30d".into()),
                    sort: 4,
                    reading: 1,
                    rows: Some(25),
                }),
            }),
            Change::Query(QueryNote {
                key: PlaylistKey([6; 16]),
                stamp: stamp(),
                body: Some(QueryBody {
                    name: "Everything".into(),
                    text: None,
                    sort: 0,
                    reading: 0,
                    rows: None,
                }),
            }),
            Change::Query(QueryNote {
                key: PlaylistKey([7; 16]),
                stamp: stamp(),
                body: None,
            }),
            Change::Answer(AnswerNote {
                recording: song("Echoes"),
                stamp: stamp(),
                answer: Answer::Lyrics(SharedLyrics {
                    text: "[00:01.00] Overhead the albatross".into(),
                    synced: true,
                }),
            }),
            Change::Answer(AnswerNote {
                recording: song("Echoes"),
                stamp: stamp(),
                answer: studied(),
            }),
            Change::PlaylistFolded(PlaylistFolded {
                folded: PlaylistKey([8; 16]),
                into: PlaylistKey([2; 16]),
                entries: vec![song("Fearless")],
            }),
            Change::QueryFolded(QueryFolded {
                folded: PlaylistKey([9; 16]),
                into: PlaylistKey([5; 16]),
            }),
        ];

        let mut bundle = encoded_bundle(&changes);
        assert_eq!(decoded_bundle(&bundle).expect("decoded"), changes);

        bundle[0] += 1;
        bundle.extend_from_slice(&[1, 200]);
        assert_eq!(decoded_bundle(&bundle).expect("decoded"), changes);
    }

    fn studied() -> Answer {
        Answer::Study(SharedStudy {
            file: WholeFile {
                packets: u64::MAX,
                frames: 61_871_400,
                rate: 44_100,
            },
            judged_under: 1,
            verdict: "genuine".into(),
            cutoff_hz: Some(21_000),
            cutoff_drop: Some(Figure::of(31.5)),
            lossy_guess: None,
            upsampled_from: None,
            bits_in_use: Some(16),
            declared_bits: Some(16),
            peak: Figure::of(-0.3),
            rms: Figure::of(-18.25),
            true_peak: Figure::of(f32::NAN),
            loudness: Some(Figure::of(-14.1)),
            loudness_range: None,
            dynamic_range: Some(11),
            clipped: 3,
            mono_as_stereo: false,
            band_levels: vec![0, 4, 255, 9],
        })
    }

    #[test]
    fn an_answer_of_a_kind_a_later_build_adds_is_skipped_and_the_rest_read() {
        let known = Change::Answer(AnswerNote {
            recording: song("Echoes"),
            stamp: stamp(),
            answer: studied(),
        });
        let mut later = Writer::new();
        later.byte(7);
        later.recording(&song("Echoes"));
        later.stamp(stamp());
        later.byte(200);
        later.text("a release");
        let mut bundle = Writer::new();
        bundle.length(2);
        bundle.bytes(&later.finish());
        bundle.bytes(&encoded_change(&known));

        assert_eq!(
            decoded_bundle(&bundle.finish()).expect("decoded"),
            vec![known]
        );
    }

    fn released() -> Answer {
        let echoes = SharedTrack {
            position: 6,
            number: "6".into(),
            title: "Echoes".into(),
            artist: None,
            recording: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("mbid")),
            track: None,
            length_ms: Some(1_411_000),
            isrcs: vec!["GBN9Y1100006".into()],
            links: vec![Link::new(
                "streaming",
                "https://tidal.com/track/55391743".into(),
            )],
        };
        Answer::Release(Box::new(SharedRelease {
            id: Mbid::new("b84ee12a-09ef-421b-82de-0441a926375b").expect("mbid"),
            group: Some(Mbid::new("a5bdf1b3-0cd8-3a1e-9e3a-0a4b1a4c4f0e").expect("mbid")),
            title: "Meddle".into(),
            credit: vec![SharedCredit {
                name: "Pink Floyd".into(),
                joined_by: String::new(),
                mbid: Some(Mbid::new("83d91898-7763-47d7-b03b-b92132375c47").expect("mbid")),
            }],
            date: Some("1971-10-30".into()),
            country: Some("GB".into()),
            label: Some("Harvest".into()),
            catalog_number: None,
            barcode: None,
            kind: Some("Album".into()),
            disambiguation: None,
            has_front_cover: true,
            links: vec![Link::new(
                "discogs",
                "https://www.discogs.com/release/1".into(),
            )],
            media: vec![SharedMedium {
                position: 1,
                format: Some("CD".into()),
                title: None,
                tracks: vec![echoes],
            }],
        }))
    }

    #[test]
    fn a_release_reads_back_and_a_build_knowing_only_lyrics_and_studies_skips_it() {
        let release = Change::Answer(AnswerNote {
            recording: song("Echoes"),
            stamp: stamp(),
            answer: released(),
        });
        let study = Change::Answer(AnswerNote {
            recording: song("Echoes"),
            stamp: stamp(),
            answer: studied(),
        });
        let bundle = encoded_bundle(&[release.clone(), study.clone()]);

        assert_eq!(
            decoded_bundle(&bundle).expect("decoded"),
            vec![release, study.clone()]
        );
        assert_eq!(
            decoded_bundle_knowing(&bundle, &[ANSWER_LYRICS, ANSWER_STUDY]).expect("decoded"),
            vec![study]
        );
    }

    #[test]
    fn every_platform_reads_back_and_a_later_one_reads_as_other() {
        let noted = |platform| {
            vec![Change::Device(DeviceNote {
                device: DeviceId([1; DEVICE_BYTES]),
                name: "Pocket".into(),
                platform,
                stamp: stamp(),
            })]
        };

        for platform in [
            Platform::Linux,
            Platform::Android,
            Platform::Ios,
            Platform::Other,
        ] {
            assert_eq!(
                decoded_bundle(&encoded_bundle(&noted(platform))).expect("decoded"),
                noted(platform)
            );
        }

        let mut later = encoded_bundle(&noted(Platform::Ios));
        let linux = encoded_bundle(&noted(Platform::Linux));
        let tag = later
            .iter()
            .zip(&linux)
            .position(|(ios, linux)| ios != linux)
            .expect("the platform tag");
        later[tag] = 9;
        assert_eq!(
            decoded_bundle(&later).expect("decoded"),
            noted(Platform::Other)
        );
    }

    #[test]
    fn lengths_past_what_is_left_are_refused_before_allocating() {
        let mut writer = Writer::new();
        writer.byte(1);
        writer.number(u64::MAX >> 1);

        assert!(matches!(
            ToServer::decoded(&writer.finish()),
            Err(Error::Malformed(Malformed::Overlong { .. }))
        ));
        assert!(matches!(
            FromServer::decoded(&[2, 0, 9]),
            Err(Error::Malformed(Malformed::Trailing { left: 1 }))
        ));
        assert!(matches!(
            FromServer::decoded(&[0, 0x80]),
            Err(Error::Malformed(Malformed::Ended))
        ));
    }

    #[test]
    fn a_beacon_reads_back_and_a_port_past_sixteen_bits_is_refused() {
        let beacon = Beacon {
            device: DeviceId([9; DEVICE_BYTES]),
            port: u16::MAX,
            answering: false,
            addresses: vec![
                IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                IpAddr::V6(Ipv6Addr::LOCALHOST),
            ],
        };
        let mut writer = Writer::new();
        writer.device(beacon.device);
        writer.number(u64::from(u16::MAX) + 1);
        writer.flag(false);
        writer.length(0);

        assert_eq!(Beacon::decoded(&beacon.encoded()).expect("decoded"), beacon);
        assert!(matches!(
            Beacon::decoded(&writer.finish()),
            Err(Error::Malformed(Malformed::Number))
        ));
    }

    #[test]
    fn signed_numbers_survive_the_zigzag() {
        for value in [0, 1, -1, i64::MIN, i64::MAX, 1_700_000_000_000_000_000] {
            let mut writer = Writer::new();
            writer.signed(value);
            let bytes = writer.finish();
            let mut reader = Reader::new(&bytes);

            assert_eq!(reader.signed().expect("read"), value);
        }
    }
}
