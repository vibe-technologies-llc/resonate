use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use resonate_core::{Mbid, Recording};

use crate::{
    DeviceId, Error, Malformed, Result, SpaceId, Token,
    catalog::Held,
    change::{
        Change, DeviceForgotten, DeviceNote, FavouriteNote, Handoff, Listen, Platform,
        PlaylistBody, PlaylistKey, PlaylistNote, Stamp,
    },
    key::{DEVICE_BYTES, SPACE_BYTES, TOKEN_BYTES},
};

pub const PROTOCOL_VERSION: u16 = 1;
pub const SERVER_ALPN: &[u8] = b"resonate-sync/1";
pub const PEER_ALPN: &[u8] = b"resonate-peer/1";
pub const LARGEST_FRAME: usize = 32 * 1024 * 1024;
pub const LONGEST_TEXT: usize = 4096;
pub const LARGEST_BLOB: usize = LARGEST_FRAME;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToPeer {
    Holdings,
    Fetch { recording: Recording, offset: u64 },
    Meet { device: DeviceId, records: bool },
    Changes { whole: bool, changes: Vec<Change> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FromPeer {
    Held { items: Vec<Held> },
    HeldEnd,
    Sending { size: u64, extension: String },
    Missing,
    Met { device: DeviceId },
    Taken,
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
        self.bytes(text.as_bytes());
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

    pub fn blobs(&mut self, blobs: &[Vec<u8>]) {
        self.length(blobs.len());
        for blob in blobs {
            self.bytes(blob);
        }
    }
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

    pub fn blobs(&mut self) -> Result<Vec<Vec<u8>>> {
        let count = self.count()?;
        let mut blobs = Vec::with_capacity(count);
        for _ in 0..count {
            blobs.push(self.bytes(LARGEST_BLOB)?.to_vec());
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
                let count = reader.count()?;
                let mut entries = Vec::with_capacity(count);
                for _ in 0..count {
                    let seq = reader.number()?;
                    let blob = if reader.flag()? {
                        Some(reader.bytes(LARGEST_BLOB)?.to_vec())
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
    }
    writer.finish()
}

pub fn decoded_bundle(bytes: &[u8]) -> Result<Vec<Change>> {
    let mut reader = Reader::new(bytes);
    let count = reader.count()?;
    let mut changes = Vec::with_capacity(count);
    for _ in 0..count {
        let encoded = reader.bytes(LARGEST_BLOB)?;
        if let Some(change) = decoded_change(encoded)? {
            changes.push(change);
        }
    }
    reader.done()?;
    Ok(changes)
}

fn decoded_change(bytes: &[u8]) -> Result<Option<Change>> {
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
                blobs: vec![vec![1, 2, 3], vec![]],
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
        ] {
            assert_eq!(ToServer::decoded(&frame.encoded()).expect("decoded"), frame);
        }
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
                parts: vec![vec![1]],
            },
            FromServer::Records {
                entries: vec![
                    Record {
                        seq: 4,
                        blob: Some(vec![1, 2]),
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
        assert_eq!(FromPeer::decoded(&held.encoded()).expect("decoded"), held);
        assert_eq!(
            FromPeer::decoded(&FromPeer::Missing.encoded()).expect("decoded"),
            FromPeer::Missing
        );
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
        ];

        let mut bundle = encoded_bundle(&changes);
        assert_eq!(decoded_bundle(&bundle).expect("decoded"), changes);

        bundle[0] += 1;
        bundle.extend_from_slice(&[1, 200]);
        assert_eq!(decoded_bundle(&bundle).expect("decoded"), changes);
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
