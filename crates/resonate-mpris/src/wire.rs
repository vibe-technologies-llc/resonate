use std::{
    io::{self, Read, Write},
    time::Duration,
};

use resonate_core::{MediaLocation, PlaylistId, TrackId};
use thiserror::Error;

use crate::{Described, Error, PipeOp, PlaylistInfo, PlaylistOrder, Result};

pub(crate) const FRAME_AT_MOST: usize = 64 << 20;
const HEADER_BYTES: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Call {
    CanRaise = 1,
    Told,
    Seek,
    SetPosition,
    Playback,
    Volume,
    SetVolume,
    Shuffle,
    Metadata,
    Position,
    Queued,
    PlayOrder,
    Described,
    RemoveTrack,
    Playlists,
    ActivatePlaylist,
    Sleep,
    SetSleep,
    AddTracks,
    Relocate,
    Tune,
    Reload,
    HandOver,
    Zone,
    Waiting,
    Device,
}

impl Call {
    const ALL: [Self; 26] = [
        Self::CanRaise,
        Self::Told,
        Self::Seek,
        Self::SetPosition,
        Self::Playback,
        Self::Volume,
        Self::SetVolume,
        Self::Shuffle,
        Self::Metadata,
        Self::Position,
        Self::Queued,
        Self::PlayOrder,
        Self::Described,
        Self::RemoveTrack,
        Self::Playlists,
        Self::ActivatePlaylist,
        Self::Sleep,
        Self::SetSleep,
        Self::AddTracks,
        Self::Relocate,
        Self::Tune,
        Self::Reload,
        Self::HandOver,
        Self::Zone,
        Self::Waiting,
        Self::Device,
    ];

    fn of(tag: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|call| *call as u8 == tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Fault {
    InvalidArgs = 1,
    NotSupported,
    Failed,
}

impl Fault {
    const ALL: [Self; 3] = [Self::InvalidArgs, Self::NotSupported, Self::Failed];

    fn of(tag: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|fault| *fault as u8 == tag)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum Act {
    Raise = 1,
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
}

impl Act {
    const ALL: [Self; 7] = [
        Self::Raise,
        Self::Play,
        Self::Pause,
        Self::PlayPause,
        Self::Stop,
        Self::Next,
        Self::Previous,
    ];

    fn of(tag: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|act| *act as u8 == tag)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum WireFault {
    #[error("a frame of {length} bytes is longer than the {FRAME_AT_MOST} allowed")]
    TooLong { length: u64 },

    #[error("a frame ended before its fields did")]
    Truncated,

    #[error("a frame carried bytes after its last field")]
    Trailing,

    #[error("{tag} names no request")]
    NoSuchRequest { tag: u8 },

    #[error("{tag} names no reply")]
    NoSuchReply { tag: u8 },

    #[error("{tag} names no action")]
    NoSuchAct { tag: u8 },

    #[error("{tag} names no fault")]
    NoSuchFault { tag: u8 },

    #[error("{name} names no playlist order")]
    NoSuchOrder { name: String },

    #[error("a text field was not UTF-8")]
    NotText,

    #[error("zero is no id")]
    ZeroId,

    #[error("a reply to {call:?} did not have the shape that call is answered in")]
    Unexpected { call: Call },
}

#[derive(Debug, PartialEq)]
pub(crate) enum Request {
    CanRaise,
    Told(Act),
    Seek(i64),
    SetPosition {
        track: TrackId,
        micros: i64,
    },
    Playback,
    Volume,
    SetVolume(f64),
    Shuffle,
    Metadata,
    Position,
    Queued,
    PlayOrder,
    Described(Vec<TrackId>),
    RemoveTrack(TrackId),
    Playlists {
        order: PlaylistOrder,
        reverse: bool,
        from: u32,
        most: u32,
    },
    ActivatePlaylist(PlaylistId),
    Sleep,
    SetSleep {
        mode: String,
        seconds: u64,
    },
    AddTracks {
        uris: Vec<String>,
        after: Option<TrackId>,
        play: bool,
    },
    Relocate(Vec<(String, String)>),
    Tune(Vec<(String, String)>),
    Reload {
        group: String,
        settings: String,
    },
    HandOver(u32),
    Zone,
    Waiting,
    Device,
}

impl Request {
    pub(crate) const fn call(&self) -> Call {
        match self {
            Self::CanRaise => Call::CanRaise,
            Self::Told(_) => Call::Told,
            Self::Seek(_) => Call::Seek,
            Self::SetPosition { .. } => Call::SetPosition,
            Self::Playback => Call::Playback,
            Self::Volume => Call::Volume,
            Self::SetVolume(_) => Call::SetVolume,
            Self::Shuffle => Call::Shuffle,
            Self::Metadata => Call::Metadata,
            Self::Position => Call::Position,
            Self::Queued => Call::Queued,
            Self::PlayOrder => Call::PlayOrder,
            Self::Described(_) => Call::Described,
            Self::RemoveTrack(_) => Call::RemoveTrack,
            Self::Playlists { .. } => Call::Playlists,
            Self::ActivatePlaylist(_) => Call::ActivatePlaylist,
            Self::Sleep => Call::Sleep,
            Self::SetSleep { .. } => Call::SetSleep,
            Self::AddTracks { .. } => Call::AddTracks,
            Self::Relocate(_) => Call::Relocate,
            Self::Tune(_) => Call::Tune,
            Self::Reload { .. } => Call::Reload,
            Self::HandOver(_) => Call::HandOver,
            Self::Zone => Call::Zone,
            Self::Waiting => Call::Waiting,
            Self::Device => Call::Device,
        }
    }

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        out.u8(self.call() as u8);
        match self {
            Self::CanRaise
            | Self::Playback
            | Self::Volume
            | Self::Shuffle
            | Self::Metadata
            | Self::Position
            | Self::Queued
            | Self::PlayOrder
            | Self::Sleep
            | Self::Zone
            | Self::Waiting
            | Self::Device => {}
            Self::Told(act) => out.u8(*act as u8),
            Self::Seek(micros) => out.i64(*micros),
            Self::SetPosition { track, micros } => {
                out.u64(track.get());
                out.i64(*micros);
            }
            Self::SetVolume(volume) => out.f64(*volume),
            Self::Described(tracks) => out.tracks(tracks),
            Self::RemoveTrack(track) => out.u64(track.get()),
            Self::Playlists {
                order,
                reverse,
                from,
                most,
            } => {
                out.text(order.name());
                out.bool(*reverse);
                out.u32(*from);
                out.u32(*most);
            }
            Self::ActivatePlaylist(playlist) => out.u64(playlist.get()),
            Self::SetSleep { mode, seconds } => {
                out.text(mode);
                out.u64(*seconds);
            }
            Self::AddTracks { uris, after, play } => {
                out.count(uris.len());
                for uri in uris {
                    out.text(uri);
                }
                out.maybe_id(after.map(TrackId::get));
                out.bool(*play);
            }
            Self::Relocate(pairs) | Self::Tune(pairs) => out.pairs(pairs),
            Self::Reload { group, settings } => {
                out.text(group);
                out.text(settings);
            }
            Self::HandOver(zone) => out.u32(*zone),
        }
        out.finish()
    }

    pub(crate) fn decode(bytes: &[u8]) -> std::result::Result<Self, WireFault> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        let call = Call::of(tag).ok_or(WireFault::NoSuchRequest { tag })?;
        let request = match call {
            Call::CanRaise => Self::CanRaise,
            Call::Told => {
                let tag = reader.u8()?;
                Self::Told(Act::of(tag).ok_or(WireFault::NoSuchAct { tag })?)
            }
            Call::Seek => Self::Seek(reader.i64()?),
            Call::SetPosition => Self::SetPosition {
                track: reader.track()?,
                micros: reader.i64()?,
            },
            Call::Playback => Self::Playback,
            Call::Volume => Self::Volume,
            Call::SetVolume => Self::SetVolume(reader.f64()?),
            Call::Shuffle => Self::Shuffle,
            Call::Metadata => Self::Metadata,
            Call::Position => Self::Position,
            Call::Queued => Self::Queued,
            Call::PlayOrder => Self::PlayOrder,
            Call::Described => Self::Described(reader.tracks()?),
            Call::RemoveTrack => Self::RemoveTrack(reader.track()?),
            Call::Playlists => {
                let name = reader.text()?;
                Self::Playlists {
                    order: PlaylistOrder::named(&name).ok_or(WireFault::NoSuchOrder { name })?,
                    reverse: reader.bool()?,
                    from: reader.u32()?,
                    most: reader.u32()?,
                }
            }
            Call::ActivatePlaylist => Self::ActivatePlaylist(reader.playlist()?),
            Call::Sleep => Self::Sleep,
            Call::SetSleep => Self::SetSleep {
                mode: reader.text()?,
                seconds: reader.u64()?,
            },
            Call::AddTracks => {
                let mut uris = Vec::new();
                for _ in 0..reader.count()? {
                    uris.push(reader.text()?);
                }
                Self::AddTracks {
                    uris,
                    after: reader.maybe_track()?,
                    play: reader.bool()?,
                }
            }
            Call::Relocate => Self::Relocate(reader.pairs()?),
            Call::Tune => Self::Tune(reader.pairs()?),
            Call::Reload => Self::Reload {
                group: reader.text()?,
                settings: reader.text()?,
            },
            Call::HandOver => Self::HandOver(reader.u32()?),
            Call::Zone => Self::Zone,
            Call::Waiting => Self::Waiting,
            Call::Device => Self::Device,
        };
        reader.end()?;
        Ok(request)
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Reply {
    Done,
    Flag(bool),
    Real(f64),
    Micros(i64),
    Text(String),
    Count(u32),
    Tracks(Vec<TrackId>),
    Rows(Vec<Described>),
    Row(Option<Described>),
    Listed(Vec<PlaylistInfo>),
    Sleeping { mode: String, seconds: u64 },
    Faulted(Fault),
}

impl Reply {
    const DONE: u8 = 1;
    const FLAG: u8 = 2;
    const REAL: u8 = 3;
    const MICROS: u8 = 4;
    const TEXT: u8 = 5;
    const COUNT: u8 = 6;
    const TRACKS: u8 = 7;
    const ROWS: u8 = 8;
    const ROW: u8 = 9;
    const LISTED: u8 = 10;
    const SLEEPING: u8 = 11;
    const FAULTED: u8 = 12;

    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut out = Writer::default();
        match self {
            Self::Done => out.u8(Self::DONE),
            Self::Flag(flag) => {
                out.u8(Self::FLAG);
                out.bool(*flag);
            }
            Self::Real(real) => {
                out.u8(Self::REAL);
                out.f64(*real);
            }
            Self::Micros(micros) => {
                out.u8(Self::MICROS);
                out.i64(*micros);
            }
            Self::Text(text) => {
                out.u8(Self::TEXT);
                out.text(text);
            }
            Self::Count(count) => {
                out.u8(Self::COUNT);
                out.u32(*count);
            }
            Self::Tracks(tracks) => {
                out.u8(Self::TRACKS);
                out.tracks(tracks);
            }
            Self::Rows(rows) => {
                out.u8(Self::ROWS);
                out.count(rows.len());
                for row in rows {
                    out.described(row);
                }
            }
            Self::Row(row) => {
                out.u8(Self::ROW);
                out.bool(row.is_some());
                if let Some(row) = row {
                    out.described(row);
                }
            }
            Self::Listed(playlists) => {
                out.u8(Self::LISTED);
                out.count(playlists.len());
                for playlist in playlists {
                    out.u64(playlist.id.get());
                    out.text(&playlist.name);
                }
            }
            Self::Sleeping { mode, seconds } => {
                out.u8(Self::SLEEPING);
                out.text(mode);
                out.u64(*seconds);
            }
            Self::Faulted(fault) => {
                out.u8(Self::FAULTED);
                out.u8(*fault as u8);
            }
        }
        out.finish()
    }

    pub(crate) fn decode(bytes: &[u8]) -> std::result::Result<Self, WireFault> {
        let mut reader = Reader::new(bytes);
        let tag = reader.u8()?;
        let reply = match tag {
            Self::DONE => Self::Done,
            Self::FLAG => Self::Flag(reader.bool()?),
            Self::REAL => Self::Real(reader.f64()?),
            Self::MICROS => Self::Micros(reader.i64()?),
            Self::TEXT => Self::Text(reader.text()?),
            Self::COUNT => Self::Count(reader.u32()?),
            Self::TRACKS => Self::Tracks(reader.tracks()?),
            Self::ROWS => {
                let mut rows = Vec::new();
                for _ in 0..reader.count()? {
                    rows.push(reader.described()?);
                }
                Self::Rows(rows)
            }
            Self::ROW => Self::Row(match reader.bool()? {
                true => Some(reader.described()?),
                false => None,
            }),
            Self::LISTED => {
                let mut playlists = Vec::new();
                for _ in 0..reader.count()? {
                    playlists.push(PlaylistInfo {
                        id: reader.playlist()?,
                        name: reader.text()?,
                    });
                }
                Self::Listed(playlists)
            }
            Self::SLEEPING => Self::Sleeping {
                mode: reader.text()?,
                seconds: reader.u64()?,
            },
            Self::FAULTED => {
                let tag = reader.u8()?;
                Self::Faulted(Fault::of(tag).ok_or(WireFault::NoSuchFault { tag })?)
            }
            tag => return Err(WireFault::NoSuchReply { tag }),
        };
        reader.end()?;
        Ok(reply)
    }

    fn shaped<T>(self, call: Call, take: impl FnOnce(Self) -> Option<T>) -> Result<T> {
        if let Self::Faulted(fault) = self {
            return Err(Error::Refused { call, fault });
        }

        take(self).ok_or_else(|| WireFault::Unexpected { call }.into())
    }

    pub(crate) fn done(self, call: Call) -> Result<()> {
        self.shaped(call, |reply| match reply {
            Self::Done => Some(()),
            _ => None,
        })
    }

    pub(crate) fn flag(self, call: Call) -> Result<bool> {
        self.shaped(call, |reply| match reply {
            Self::Flag(flag) => Some(flag),
            _ => None,
        })
    }

    pub(crate) fn real(self, call: Call) -> Result<f64> {
        self.shaped(call, |reply| match reply {
            Self::Real(real) => Some(real),
            _ => None,
        })
    }

    pub(crate) fn micros(self, call: Call) -> Result<i64> {
        self.shaped(call, |reply| match reply {
            Self::Micros(micros) => Some(micros),
            _ => None,
        })
    }

    pub(crate) fn text(self, call: Call) -> Result<String> {
        self.shaped(call, |reply| match reply {
            Self::Text(text) => Some(text),
            _ => None,
        })
    }

    pub(crate) fn count(self, call: Call) -> Result<u32> {
        self.shaped(call, |reply| match reply {
            Self::Count(count) => Some(count),
            _ => None,
        })
    }

    pub(crate) fn tracks(self, call: Call) -> Result<Vec<TrackId>> {
        self.shaped(call, |reply| match reply {
            Self::Tracks(tracks) => Some(tracks),
            _ => None,
        })
    }

    pub(crate) fn rows(self, call: Call) -> Result<Vec<Described>> {
        self.shaped(call, |reply| match reply {
            Self::Rows(rows) => Some(rows),
            _ => None,
        })
    }

    pub(crate) fn row(self, call: Call) -> Result<Option<Described>> {
        self.shaped(call, |reply| match reply {
            Self::Row(row) => Some(row),
            _ => None,
        })
    }

    pub(crate) fn listed(self, call: Call) -> Result<Vec<PlaylistInfo>> {
        self.shaped(call, |reply| match reply {
            Self::Listed(listed) => Some(listed),
            _ => None,
        })
    }

    pub(crate) fn sleeping(self, call: Call) -> Result<(String, u64)> {
        self.shaped(call, |reply| match reply {
            Self::Sleeping { mode, seconds } => Some((mode, seconds)),
            _ => None,
        })
    }
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn finish(self) -> Vec<u8> {
        self.0
    }

    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }

    fn bool(&mut self, value: bool) {
        self.u8(u8::from(value));
    }

    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn f64(&mut self, value: f64) {
        self.u64(value.to_bits());
    }

    fn count(&mut self, length: usize) {
        self.u32(u32::try_from(length).unwrap_or(u32::MAX));
    }

    fn text(&mut self, text: &str) {
        self.count(text.len());
        self.0.extend_from_slice(text.as_bytes());
    }

    fn maybe_text(&mut self, text: Option<&str>) {
        self.bool(text.is_some());
        if let Some(text) = text {
            self.text(text);
        }
    }

    fn maybe_id(&mut self, id: Option<u64>) {
        self.bool(id.is_some());
        if let Some(id) = id {
            self.u64(id);
        }
    }

    fn tracks(&mut self, tracks: &[TrackId]) {
        self.count(tracks.len());
        for track in tracks {
            self.u64(track.get());
        }
    }

    fn pairs(&mut self, pairs: &[(String, String)]) {
        self.count(pairs.len());
        for (first, second) in pairs {
            self.text(first);
            self.text(second);
        }
    }

    fn described(&mut self, row: &Described) {
        self.u64(row.track.get());
        let url = row
            .location
            .as_ref()
            .map(|location| location.to_uri_within(row.span));
        self.maybe_text(url.as_deref());
        self.maybe_text(row.title.as_deref());
        self.maybe_text(row.artist.as_deref());
        self.maybe_text(row.album.as_deref());
        self.maybe_id(
            row.length
                .map(|length| u64::try_from(length.as_micros()).unwrap_or(u64::MAX)),
        );
        self.maybe_text(row.art.as_deref());
    }
}

struct Reader<'a> {
    left: &'a [u8],
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { left: bytes }
    }

    fn end(&self) -> std::result::Result<(), WireFault> {
        match self.left.is_empty() {
            true => Ok(()),
            false => Err(WireFault::Trailing),
        }
    }

    fn take(&mut self, length: usize) -> std::result::Result<&'a [u8], WireFault> {
        let (taken, left) = self
            .left
            .split_at_checked(length)
            .ok_or(WireFault::Truncated)?;
        self.left = left;
        Ok(taken)
    }

    fn array<const N: usize>(&mut self) -> std::result::Result<[u8; N], WireFault> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn u8(&mut self) -> std::result::Result<u8, WireFault> {
        Ok(self.array::<1>()?[0])
    }

    fn bool(&mut self) -> std::result::Result<bool, WireFault> {
        Ok(self.u8()? != 0)
    }

    fn u32(&mut self) -> std::result::Result<u32, WireFault> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> std::result::Result<u64, WireFault> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn i64(&mut self) -> std::result::Result<i64, WireFault> {
        Ok(i64::from_le_bytes(self.array()?))
    }

    fn f64(&mut self) -> std::result::Result<f64, WireFault> {
        Ok(f64::from_bits(self.u64()?))
    }

    fn count(&mut self) -> std::result::Result<usize, WireFault> {
        let count = usize::try_from(self.u32()?).unwrap_or(usize::MAX);
        match count > self.left.len() {
            true => Err(WireFault::Truncated),
            false => Ok(count),
        }
    }

    fn text(&mut self) -> std::result::Result<String, WireFault> {
        let length = self.count()?;
        String::from_utf8(self.take(length)?.to_vec()).map_err(|_| WireFault::NotText)
    }

    fn maybe_text(&mut self) -> std::result::Result<Option<String>, WireFault> {
        match self.bool()? {
            true => self.text().map(Some),
            false => Ok(None),
        }
    }

    fn track(&mut self) -> std::result::Result<TrackId, WireFault> {
        TrackId::new(self.u64()?).map_err(|_| WireFault::ZeroId)
    }

    fn playlist(&mut self) -> std::result::Result<PlaylistId, WireFault> {
        PlaylistId::new(self.u64()?).map_err(|_| WireFault::ZeroId)
    }

    fn maybe_track(&mut self) -> std::result::Result<Option<TrackId>, WireFault> {
        match self.bool()? {
            true => self.track().map(Some),
            false => Ok(None),
        }
    }

    fn tracks(&mut self) -> std::result::Result<Vec<TrackId>, WireFault> {
        let mut tracks = Vec::new();
        for _ in 0..self.count()? {
            tracks.push(self.track()?);
        }
        Ok(tracks)
    }

    fn pairs(&mut self) -> std::result::Result<Vec<(String, String)>, WireFault> {
        let mut pairs = Vec::new();
        for _ in 0..self.count()? {
            pairs.push((self.text()?, self.text()?));
        }
        Ok(pairs)
    }

    fn described(&mut self) -> std::result::Result<Described, WireFault> {
        let track = self.track()?;
        let cut = self
            .maybe_text()?
            .as_deref()
            .and_then(MediaLocation::from_uri_within);
        let (location, span) = match cut {
            Some((location, span)) => (Some(location), span),
            None => (None, None),
        };
        let title = self.maybe_text()?;
        let artist = self.maybe_text()?;
        let album = self.maybe_text()?;
        let length = match self.bool()? {
            true => Some(Duration::from_micros(self.u64()?)),
            false => None,
        };

        Ok(Described {
            track,
            location,
            span,
            title,
            artist,
            album,
            length,
            art: self.maybe_text()?,
        })
    }
}

pub(crate) fn write_frame(to: &mut impl Write, body: &[u8]) -> Result<()> {
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| usize::try_from(*length).is_ok_and(|length| length <= FRAME_AT_MOST))
        .ok_or_else(|| WireFault::TooLong {
            length: u64::try_from(body.len()).unwrap_or(u64::MAX),
        })?;

    to.write_all(&length.to_le_bytes())
        .and_then(|()| to.write_all(body))
        .and_then(|()| to.flush())
        .map_err(|source| Error::pipe(PipeOp::Write, source))
}

pub(crate) fn read_frame(from: &mut impl Read) -> Result<Option<Vec<u8>>> {
    let mut header = [0; HEADER_BYTES];
    let mut filled = 0;
    while filled < HEADER_BYTES {
        match from.read(&mut header[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => {
                return Err(Error::pipe(
                    PipeOp::Read,
                    io::ErrorKind::UnexpectedEof.into(),
                ));
            }
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error)
                if filled == 0
                    && matches!(
                        error.kind(),
                        io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                    ) =>
            {
                return Ok(None);
            }
            Err(source) => return Err(Error::pipe(PipeOp::Read, source)),
        }
    }

    let length = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
    if length > FRAME_AT_MOST {
        return Err(WireFault::TooLong {
            length: u64::try_from(length).unwrap_or(u64::MAX),
        }
        .into());
    }

    let mut body = Vec::new();
    from.by_ref()
        .take(u64::try_from(length).unwrap_or(u64::MAX))
        .read_to_end(&mut body)
        .map_err(|source| Error::pipe(PipeOp::Read, source))?;
    match body.len() == length {
        true => Ok(Some(body)),
        false => Err(Error::pipe(
            PipeOp::Read,
            io::ErrorKind::UnexpectedEof.into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn track(id: u64) -> TrackId {
        TrackId::new(id).expect("a non-zero track id")
    }

    fn row() -> Described {
        Described {
            track: track(7),
            location: Some(MediaLocation::local("/music/a b.flac")),
            span: None,
            title: Some("Echoes".to_owned()),
            artist: Some("Pink Floyd".to_owned()),
            album: None,
            length: Some(Duration::from_micros(1_234_567)),
            art: Some("file:///tmp/cover.jpg".to_owned()),
        }
    }

    fn requests() -> Vec<Request> {
        vec![
            Request::CanRaise,
            Request::Told(Act::PlayPause),
            Request::Seek(-5_000_000),
            Request::SetPosition {
                track: track(3),
                micros: 42,
            },
            Request::Playback,
            Request::Volume,
            Request::SetVolume(0.5),
            Request::Shuffle,
            Request::Metadata,
            Request::Position,
            Request::Queued,
            Request::PlayOrder,
            Request::Described(vec![track(1), track(9)]),
            Request::RemoveTrack(track(2)),
            Request::Playlists {
                order: PlaylistOrder::ModifiedDate,
                reverse: true,
                from: 4,
                most: u32::MAX,
            },
            Request::ActivatePlaylist(PlaylistId::new(5).expect("a playlist")),
            Request::Sleep,
            Request::SetSleep {
                mode: "after".to_owned(),
                seconds: 90,
            },
            Request::AddTracks {
                uris: vec!["file:///a.flac".to_owned(), "file:///b.flac".to_owned()],
                after: Some(track(4)),
                play: true,
            },
            Request::AddTracks {
                uris: Vec::new(),
                after: None,
                play: false,
            },
            Request::Relocate(vec![("file:///a".to_owned(), "file:///b".to_owned())]),
            Request::Tune(vec![("dither".to_owned(), "tpdf".to_owned())]),
            Request::Reload {
                group: "equaliser".to_owned(),
                settings: r"C:\Users\x\config.toml".to_owned(),
            },
            Request::HandOver(2),
            Request::Zone,
            Request::Waiting,
            Request::Device,
        ]
    }

    #[test]
    fn every_request_is_read_back_as_it_was_written() {
        for request in requests() {
            let bytes = request.encode();

            assert_eq!(
                Request::decode(&bytes).expect("a request that was written"),
                request
            );
        }
    }

    #[test]
    fn every_call_has_a_request_that_names_it() {
        let named: Vec<Call> = requests().iter().map(Request::call).collect();

        for call in Call::ALL {
            assert!(named.contains(&call), "{call:?} has no request");
        }
    }

    #[test]
    fn every_reply_is_read_back_as_it_was_written() {
        let replies = vec![
            Reply::Done,
            Reply::Flag(true),
            Reply::Real(0.25),
            Reply::Micros(-3),
            Reply::Text("Study".to_owned()),
            Reply::Count(4),
            Reply::Tracks(vec![track(1), track(2)]),
            Reply::Rows(vec![row(), row()]),
            Reply::Row(Some(row())),
            Reply::Row(None),
            Reply::Listed(vec![PlaylistInfo {
                id: PlaylistId::new(2).expect("a playlist"),
                name: "Evening".to_owned(),
            }]),
            Reply::Sleeping {
                mode: "off".to_owned(),
                seconds: 0,
            },
            Reply::Faulted(Fault::NotSupported),
        ];

        for reply in replies {
            let bytes = reply.encode();

            assert_eq!(
                Reply::decode(&bytes).expect("a reply that was written"),
                reply
            );
        }
    }

    #[test]
    fn a_cut_row_keeps_its_frames_across_the_wire() {
        let span = resonate_core::FrameSpan::starting(resonate_core::Frames(44_100));
        let cut = Described {
            span: Some(span),
            ..row()
        };

        let bytes = Reply::Row(Some(cut.clone())).encode();

        assert_eq!(
            Reply::decode(&bytes).expect("a reply that was written"),
            Reply::Row(Some(cut))
        );
    }

    #[test]
    fn a_frame_that_ends_early_or_runs_on_is_refused_by_name() {
        let whole = Request::SetPosition {
            track: track(3),
            micros: 42,
        }
        .encode();

        assert_eq!(
            Request::decode(&whole[..whole.len() - 1]),
            Err(WireFault::Truncated)
        );
        let mut longer = whole;
        longer.push(0);
        assert_eq!(Request::decode(&longer), Err(WireFault::Trailing));
    }

    #[test]
    fn a_tag_nothing_defines_is_refused_by_name() {
        assert_eq!(
            Request::decode(&[200]),
            Err(WireFault::NoSuchRequest { tag: 200 })
        );
        assert_eq!(
            Reply::decode(&[200]),
            Err(WireFault::NoSuchReply { tag: 200 })
        );
        assert_eq!(
            Request::decode(&[Call::Told as u8, 99]),
            Err(WireFault::NoSuchAct { tag: 99 })
        );
        assert_eq!(
            Reply::decode(&[Reply::FAULTED, 99]),
            Err(WireFault::NoSuchFault { tag: 99 })
        );
    }

    #[test]
    fn a_zero_id_and_text_that_is_not_utf8_are_refused() {
        let mut zero = vec![Call::RemoveTrack as u8];
        zero.extend_from_slice(&0u64.to_le_bytes());
        let mut garbled = vec![Reply::TEXT];
        garbled.extend_from_slice(&1u32.to_le_bytes());
        garbled.push(0xff);

        assert_eq!(Request::decode(&zero), Err(WireFault::ZeroId));
        assert_eq!(Reply::decode(&garbled), Err(WireFault::NotText));
    }

    #[test]
    fn a_count_larger_than_the_frame_could_hold_is_refused_before_anything_is_reserved() {
        let mut lying = vec![Call::Described as u8];
        lying.extend_from_slice(&u32::MAX.to_le_bytes());

        assert_eq!(Request::decode(&lying), Err(WireFault::Truncated));
    }

    #[test]
    fn a_fault_in_place_of_the_shape_asked_for_is_a_refusal_and_any_other_shape_is_unexpected() {
        let refused = Reply::Faulted(Fault::InvalidArgs).done(Call::HandOver);
        let wrong = Reply::Flag(true).done(Call::HandOver);

        assert!(matches!(
            refused,
            Err(Error::Refused {
                call: Call::HandOver,
                fault: Fault::InvalidArgs
            })
        ));
        assert!(matches!(
            wrong,
            Err(Error::Wire(WireFault::Unexpected {
                call: Call::HandOver
            }))
        ));
    }

    #[test]
    fn frames_are_read_back_whole_in_the_order_they_were_written_and_end_cleanly() {
        let mut wire = Vec::new();
        write_frame(&mut wire, b"first").expect("a frame");
        write_frame(&mut wire, b"").expect("an empty frame");
        write_frame(&mut wire, b"third").expect("a frame");
        let mut read = Cursor::new(wire);

        assert_eq!(
            read_frame(&mut read).expect("a frame"),
            Some(b"first".to_vec())
        );
        assert_eq!(read_frame(&mut read).expect("a frame"), Some(Vec::new()));
        assert_eq!(
            read_frame(&mut read).expect("a frame"),
            Some(b"third".to_vec())
        );
        assert_eq!(read_frame(&mut read).expect("the end"), None);
    }

    #[test]
    fn a_frame_cut_short_is_an_error_and_one_too_long_is_refused_unread() {
        let mut cut = Vec::new();
        write_frame(&mut cut, b"whole").expect("a frame");
        cut.truncate(cut.len() - 1);
        let announced = u32::try_from(FRAME_AT_MOST + 1)
            .expect("fits")
            .to_le_bytes();

        assert!(matches!(
            read_frame(&mut Cursor::new(cut)),
            Err(Error::Pipe {
                op: PipeOp::Read,
                ..
            })
        ));
        assert!(matches!(
            read_frame(&mut Cursor::new(announced.to_vec())),
            Err(Error::Wire(WireFault::TooLong { .. }))
        ));
    }
}
