use std::{
    collections::HashMap,
    fmt,
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::Duration,
};

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, TrackId, Volume, ZoneId};
use resonate_engine::{Asleep, Placement, Until};
use zbus::{
    OwnedObjectPath, OwnedVariant,
    names::{OwnedWellKnownName, WellKnownName},
};

use crate::{
    BusOp, Error, PlaylistInfo, PlaylistOrder, Reloadable, Result, Tunable,
    blocking::{Connection, connection},
    bus_peer::BusPeer,
    peer::{Peer, PipePeer},
    service::{BUS_NAME, ZONE_PREFIX},
    track::{PlaybackStatus, track_of},
    wire::Act,
};

pub(crate) const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
const MOVES_A_CALL: usize = 4096;
pub const ROWS_A_CALL: usize = 128;
const INSTANCE_PREFIX: &str = "instance";
const TRACK_ID: &str = "mpris:trackid";
const LENGTH: &str = "mpris:length";
const ART_URL: &str = "mpris:artUrl";
const URL: &str = "xesam:url";
const TITLE: &str = "xesam:title";
const ARTIST: &str = "xesam:artist";
const ALBUM: &str = "xesam:album";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlayerName(Box<str>);

impl PlayerName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PlayerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZoneAsked {
    Numbered(ZoneId),
    Named(String),
}

impl ZoneAsked {
    pub fn read(given: &str) -> Self {
        let given = given.trim();
        match given.parse().ok().and_then(ZoneId::new) {
            Some(zone) => Self::Numbered(zone),
            None => Self::Named(given.to_owned()),
        }
    }
}

impl fmt::Display for ZoneAsked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Numbered(zone) => write!(f, "{zone}"),
            Self::Named(name) => f.write_str(name),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Queueing {
    pub at: Placement,
    pub play: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Seeking {
    Forward(Duration),
    Backward(Duration),
}

impl Seeking {
    fn offset(self) -> i64 {
        match self {
            Self::Forward(span) => micros_of(span),
            Self::Backward(span) => -micros_of(span),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Described {
    pub track: TrackId,
    pub location: Option<MediaLocation>,
    pub span: Option<FrameSpan>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub length: Option<Duration>,
    pub art: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Standing {
    pub name: OwnedWellKnownName,
    pub zone: Option<String>,
    pub playback: Option<PlaybackStatus>,
    pub queued: usize,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub struct Running {
    peer: Arc<dyn Peer>,
    name: WellKnownName<'static>,
}

#[derive(Clone)]
pub struct Session {
    bus: Option<Connection>,
}

impl Session {
    pub fn open() -> Result<Self> {
        match session() {
            Ok(connection) => Ok(Self {
                bus: Some(connection),
            }),
            Err(error) => Self::without_a_bus(error),
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    fn without_a_bus(error: Error) -> Result<Self> {
        Err(error)
    }

    #[cfg(any(windows, target_os = "macos"))]
    fn without_a_bus(error: Error) -> Result<Self> {
        tracing::debug!(%error, "no session bus; players are looked for on their pipes");
        Ok(Self { bus: None })
    }

    fn players(&self) -> Result<Vec<Running>> {
        let mut players = Vec::new();
        if let Some(connection) = &self.bus {
            players.extend(BusPeer::serving(connection)?);
        }
        #[cfg(windows)]
        players.extend(crate::pipe::serving()?);
        #[cfg(target_os = "macos")]
        players.extend(crate::socket::serving()?);
        players.sort_by(|first, second| first.0.as_str().cmp(second.0.as_str()));

        Ok(players
            .into_iter()
            .map(|(name, peer)| Running { peer, name })
            .collect())
    }
}

impl Running {
    pub fn found() -> Result<Option<Self>> {
        Self::found_in(&Session::open()?)
    }

    pub fn found_in(session: &Session) -> Result<Option<Self>> {
        Ok(session.players()?.into_iter().next())
    }

    pub fn listed() -> Result<Vec<Self>> {
        Self::listed_in(&Session::open()?)
    }

    pub fn listed_in(session: &Session) -> Result<Vec<Self>> {
        session.players()
    }

    pub fn named(given: &PlayerName) -> Result<Option<Self>> {
        Self::named_in(&Session::open()?, given)
    }

    pub fn named_in(session: &Session, given: &PlayerName) -> Result<Option<Self>> {
        Ok(session
            .players()?
            .into_iter()
            .find(|running| answers_to(running.name.as_str(), given.as_str())))
    }

    pub fn zoned(asked: &ZoneAsked) -> Result<Option<Self>> {
        Self::zoned_in(&Session::open()?, asked)
    }

    pub fn zoned_in(session: &Session, asked: &ZoneAsked) -> Result<Option<Self>> {
        Ok(session.players()?.into_iter().find(|running| match asked {
            ZoneAsked::Numbered(zone) => numbered(running.name.as_str()) == Some(*zone),
            ZoneAsked::Named(given) => running
                .zone()
                .is_ok_and(|zone| zone.to_lowercase() == given.to_lowercase()),
        }))
    }

    pub fn through<S: Read + Write + Send + 'static>(
        stream: S,
        name: WellKnownName<'static>,
    ) -> Result<Self> {
        Ok(Self {
            peer: Arc::new(PipePeer::over(stream)?),
            name,
        })
    }

    pub const fn name(&self) -> &WellKnownName<'static> {
        &self.name
    }

    pub fn zone_number(&self) -> Option<ZoneId> {
        numbered(self.name.as_str())
    }

    pub fn hand_over(&self, to: ZoneId) -> Result<()> {
        self.peer.hand_over(to)
    }

    pub fn zone(&self) -> Result<String> {
        self.peer.zone()
    }

    pub fn standing(&self) -> Result<Standing> {
        let playback = self.playback()?;
        let playing = self.peer.metadata()?;

        Ok(Standing {
            name: self.name.clone().into(),
            zone: self.zone().ok(),
            playback,
            queued: self.peer.queued()?.len(),
            title: playing.as_ref().and_then(|playing| playing.title.clone()),
            artist: playing.and_then(|playing| playing.artist),
        })
    }

    pub fn playback(&self) -> Result<Option<PlaybackStatus>> {
        self.peer.playback()
    }

    pub fn a_window() -> Result<Option<Self>> {
        for running in Self::listed()? {
            if running.can_raise().unwrap_or(false) {
                return Ok(Some(running));
            }
        }
        Ok(None)
    }

    pub fn can_raise(&self) -> Result<bool> {
        self.peer.can_raise()
    }

    pub fn raise(&self) -> Result<()> {
        self.peer.told(Act::Raise)
    }

    pub fn play(&self) -> Result<()> {
        self.peer.told(Act::Play)
    }

    pub fn pause(&self) -> Result<()> {
        self.peer.told(Act::Pause)
    }

    pub fn play_pause(&self) -> Result<()> {
        self.peer.told(Act::PlayPause)
    }

    pub fn stop(&self) -> Result<()> {
        self.peer.told(Act::Stop)
    }

    pub fn next(&self) -> Result<()> {
        self.peer.told(Act::Next)
    }

    pub fn previous(&self) -> Result<()> {
        self.peer.told(Act::Previous)
    }

    pub fn seek(&self, by: Seeking) -> Result<()> {
        self.peer.seek(by.offset())
    }

    pub fn set_position(&self, track: TrackId, position: Duration) -> Result<()> {
        self.peer.set_position(track, micros_of(position))
    }

    pub fn volume(&self) -> Result<Volume> {
        let reading = self.peer.volume()?;

        Volume::new(reading as f32).map_err(|_| Error::NotAVolume { reading })
    }

    pub fn set_volume(&self, volume: Volume) -> Result<()> {
        self.peer.set_volume(f64::from(volume.get()))
    }

    pub fn shuffle(&self) -> Result<bool> {
        self.peer.shuffle()
    }

    pub fn metadata(&self) -> Result<Option<Described>> {
        self.peer.metadata()
    }

    pub fn position(&self) -> Result<Duration> {
        Ok(span_of(self.peer.position()?))
    }

    pub fn rows(&self) -> Result<Vec<Described>> {
        self.described(&self.queued()?)
    }

    pub fn queued(&self) -> Result<Vec<TrackId>> {
        self.peer.queued()
    }

    pub fn play_order(&self) -> Result<Vec<TrackId>> {
        self.peer.play_order()
    }

    pub fn described(&self, rows: &[TrackId]) -> Result<Vec<Described>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        self.peer.described(rows)
    }

    pub fn remove_track(&self, track: TrackId) -> Result<()> {
        self.peer.remove_track(track)
    }

    pub fn playlists(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: usize,
        most: Option<usize>,
    ) -> Result<Vec<PlaylistInfo>> {
        let index = u32::try_from(from).unwrap_or(u32::MAX);
        let max_count = most.map_or(u32::MAX, |most| u32::try_from(most).unwrap_or(u32::MAX));

        self.peer.playlists(order, reverse, index, max_count)
    }

    pub fn activate_playlist(&self, playlist: PlaylistId) -> Result<()> {
        self.peer.activate_playlist(playlist)
    }

    pub fn sleep(&self) -> Result<Option<Asleep>> {
        self.peer.sleep()
    }

    pub fn set_sleep(&self, until: Option<Until>) -> Result<()> {
        self.peer.set_sleep(until)
    }

    pub fn relocate(&self, moved: &[(MediaLocation, MediaLocation)]) -> Result<()> {
        for some in moved.chunks(MOVES_A_CALL) {
            let uris: Vec<(String, String)> = some
                .iter()
                .map(|(from, to)| (from.to_uri(), to.to_uri()))
                .collect();
            self.peer.relocate(&uris)?;
        }
        Ok(())
    }

    pub fn tune(&self, settings: &[(Tunable, String)]) -> Result<()> {
        if settings.is_empty() {
            return Ok(());
        }
        let pairs: Vec<(String, String)> = settings
            .iter()
            .map(|(setting, value)| (setting.name().to_owned(), value.clone()))
            .collect();

        self.peer.tune(&pairs)
    }

    pub fn reload(&self, group: Reloadable, settings: &Path) -> Result<()> {
        self.peer.reload(group, settings)
    }

    pub fn queue(
        &self,
        rows: &[(MediaLocation, Option<FrameSpan>)],
        queueing: Queueing,
    ) -> Result<usize> {
        if rows.is_empty() {
            return Ok(0);
        }
        let after = self.landing(queueing.at)?;
        let uris: Vec<String> = rows
            .iter()
            .map(|(location, span)| location.to_uri_within(*span))
            .collect();

        let mut landed = 0;
        for (nth, chunk) in uris.chunks(ROWS_A_CALL).enumerate().rev() {
            let heard_now = queueing.play && nth == 0;
            match self.peer.add_tracks(chunk, after, heard_now) {
                Ok(()) => landed += chunk.len(),
                Err(source) => return Err(partway(landed, rows.len(), source)),
            }
        }
        Ok(rows.len())
    }

    fn landing(&self, at: Placement) -> Result<Option<TrackId>> {
        let tracks = self.peer.queued()?;
        let row = match at {
            Placement::At(row) => row.min(tracks.len()),
            Placement::Next => self.after_the_playing(&tracks, 0)?,
            Placement::Queued => self.after_the_playing(&tracks, self.peer.waiting()?)?,
        };

        Ok(row
            .checked_sub(1)
            .and_then(|before| tracks.get(before).copied()))
    }

    fn after_the_playing(&self, tracks: &[TrackId], waiting: usize) -> Result<usize> {
        let playing = self
            .peer
            .metadata()?
            .and_then(|playing| tracks.iter().position(|track| *track == playing.track));

        Ok(playing.map_or(tracks.len(), |at| {
            at.saturating_add(1)
                .saturating_add(waiting)
                .min(tracks.len())
        }))
    }
}

fn partway(landed: usize, asked: usize, source: Error) -> Error {
    if landed == 0 {
        return source;
    }
    Error::QueuedPartway {
        landed,
        asked,
        source: Box::new(source),
    }
}

fn session() -> Result<Connection> {
    connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))
}

pub(crate) fn names_this_build(name: &str) -> bool {
    name == BUS_NAME || name.strip_prefix(BUS_NAME).is_some_and(names_an_instance)
}

fn names_an_instance(rest: &str) -> bool {
    rest.strip_prefix('.')
        .is_some_and(|instance| !instance.is_empty())
}

fn numbered(name: &str) -> Option<ZoneId> {
    let rest = name.strip_prefix(BUS_NAME)?;
    if rest.is_empty() {
        return Some(ZoneId::FIRST);
    }
    let last = rest.rsplit('.').next()?;
    match last.strip_prefix(ZONE_PREFIX) {
        Some(number) => ZoneId::new(number.parse().ok()?),
        None => last.starts_with(INSTANCE_PREFIX).then_some(ZoneId::FIRST),
    }
}

fn answers_to(name: &str, given: &str) -> bool {
    name == given
        || name
            .strip_prefix(BUS_NAME)
            .and_then(|rest| rest.strip_prefix('.'))
            == Some(given)
}

const fn micros_of(span: Duration) -> i64 {
    let micros = span.as_micros();
    if micros > i64::MAX as u128 {
        i64::MAX
    } else {
        micros as i64
    }
}

fn span_of(micros: i64) -> Duration {
    Duration::from_micros(micros.max(0).unsigned_abs())
}

pub(crate) fn described(fields: &HashMap<String, OwnedVariant>) -> Option<Described> {
    let path = fields
        .get(TRACK_ID)
        .and_then(|value| OwnedObjectPath::try_from(value.clone()).ok())?;

    let cut = text(fields, URL)
        .as_deref()
        .and_then(MediaLocation::from_uri_within);
    let (location, span) = match cut {
        Some((location, span)) => (Some(location), span),
        None => (None, None),
    };

    Some(Described {
        track: track_of(path.as_str())?,
        location,
        span,
        title: text(fields, TITLE),
        artist: first(fields, ARTIST),
        album: text(fields, ALBUM),
        length: length(fields),
        art: text(fields, ART_URL),
    })
}

fn length(fields: &HashMap<String, OwnedVariant>) -> Option<Duration> {
    let micros = i64::try_from(fields.get(LENGTH)?.clone()).ok()?;

    Some(span_of(micros))
}

fn text(metadata: &HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    String::try_from(metadata.get(key)?.clone()).ok()
}

fn first(metadata: &HashMap<String, OwnedVariant>, key: &str) -> Option<String> {
    Vec::<String>::try_from(metadata.get(key)?.clone())
        .ok()?
        .into_iter()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_of_this_build_is_the_one_it_claims_or_an_instance_of_it() {
        assert!(names_this_build(&format!("{BUS_NAME}.zone2")));
        assert!(names_this_build(BUS_NAME));
        assert!(names_this_build(&format!("{BUS_NAME}.instance1234")));
        assert!(!names_this_build(&format!("{BUS_NAME}.")));
        assert!(!names_this_build(&format!("{BUS_NAME}-other")));
        assert!(!names_this_build("org.mpris.MediaPlayer2.vlc"));
    }

    #[test]
    fn a_zone_is_found_by_the_number_its_bus_name_ends_in_and_the_first_by_the_plain_name() {
        let second = ZoneId::new(2).expect("a second zone");

        assert_eq!(numbered(BUS_NAME), Some(ZoneId::FIRST));
        assert_eq!(
            numbered(&format!("{BUS_NAME}.instance1234")),
            Some(ZoneId::FIRST)
        );
        assert_eq!(numbered(&format!("{BUS_NAME}.zone2")), Some(second));
        assert_eq!(
            numbered(&format!("{BUS_NAME}.instance1234.zone2")),
            Some(second)
        );
        assert_eq!(numbered(&format!("{BUS_NAME}.zonex")), None);
        assert_eq!(numbered("org.mpris.MediaPlayer2.vlc"), None);
        assert!(answers_to(&format!("{BUS_NAME}.zone2"), "zone2"));
    }

    #[test]
    fn a_zone_asked_for_by_a_number_is_numbered_and_by_anything_else_named() {
        assert_eq!(
            ZoneAsked::read(" 2 "),
            ZoneAsked::Numbered(ZoneId::new(2).expect("a second zone"))
        );
        assert_eq!(ZoneAsked::read("0"), ZoneAsked::Named("0".to_owned()));
        assert_eq!(
            ZoneAsked::read("Living room"),
            ZoneAsked::Named("Living room".to_owned())
        );
    }

    #[test]
    fn an_instance_is_named_by_its_whole_bus_name_or_by_the_instance_alone() {
        let instance = format!("{BUS_NAME}.instance1234");

        assert!(answers_to(&instance, &instance));
        assert!(answers_to(&instance, "instance1234"));
        assert!(!answers_to(&instance, "instance99"));
        assert!(answers_to(BUS_NAME, BUS_NAME));
        assert!(!answers_to(BUS_NAME, "instance1234"));
    }

    #[test]
    fn the_plain_name_sorts_before_every_instance_under_it() {
        let mut names = vec![
            format!("{BUS_NAME}.instance9"),
            BUS_NAME.to_owned(),
            format!("{BUS_NAME}.instance10"),
        ];
        names.sort_unstable();

        assert_eq!(
            names,
            vec![
                BUS_NAME.to_owned(),
                format!("{BUS_NAME}.instance10"),
                format!("{BUS_NAME}.instance9"),
            ]
        );
    }
}
