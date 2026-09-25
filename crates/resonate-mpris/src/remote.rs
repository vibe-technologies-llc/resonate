use std::{collections::HashMap, fmt, time::Duration};

use resonate_core::{FrameSpan, MediaLocation, PlaylistId, TrackId, Volume};
use resonate_engine::{Asleep, Placement, Until};
use zbus::{
    blocking::{Connection, Proxy, connection, fdo::DBusProxy},
    names::{OwnedWellKnownName, WellKnownName},
    zvariant::{OwnedObjectPath, OwnedValue},
};

use crate::{
    BusOp, Error, PlaylistInfo, PlaylistOrder, Result,
    interfaces::OWN_INTERFACE,
    service::{BUS_NAME, OBJECT_PATH},
    track::{
        NO_TRACK, PlaybackStatus, no_track, playlist_of, playlist_path, sleep_asked, sleep_timer,
        track_of, track_path,
    },
};

const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const RAISE: &str = "Raise";
const CAN_RAISE: &str = "CanRaise";
const TRACK_LIST: &str = "org.mpris.MediaPlayer2.TrackList";
const PLAYLISTS: &str = "org.mpris.MediaPlayer2.Playlists";
const ADD_TRACK: &str = "AddTrack";
const REMOVE_TRACK: &str = "RemoveTrack";
const GET_TRACKS_METADATA: &str = "GetTracksMetadata";
const GET_PLAYLISTS: &str = "GetPlaylists";
const ACTIVATE_PLAYLIST: &str = "ActivatePlaylist";
const SET_SLEEP: &str = "SetSleep";
const PLAYING_NEXT: &str = "PlayingNext";
const PLAY: &str = "Play";
const PAUSE: &str = "Pause";
const PLAY_PAUSE: &str = "PlayPause";
const STOP: &str = "Stop";
const NEXT: &str = "Next";
const PREVIOUS: &str = "Previous";
const SEEK: &str = "Seek";
const SET_POSITION: &str = "SetPosition";
const TRACKS: &str = "Tracks";
const METADATA: &str = "Metadata";
const POSITION: &str = "Position";
const VOLUME: &str = "Volume";
const SLEEP: &str = "Sleep";
const PLAYBACK_STATUS: &str = "PlaybackStatus";
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
    pub playback: Option<PlaybackStatus>,
    pub queued: usize,
    pub title: Option<String>,
    pub artist: Option<String>,
}

pub struct Running {
    connection: Connection,
    name: WellKnownName<'static>,
}

impl Running {
    pub fn found() -> Result<Option<Self>> {
        let connection = session()?;
        let Some(name) = ours(&connection)?.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(Self { connection, name }))
    }

    pub fn listed() -> Result<Vec<Self>> {
        let connection = session()?;

        Ok(ours(&connection)?
            .into_iter()
            .map(|name| Self {
                connection: connection.clone(),
                name,
            })
            .collect())
    }

    pub fn named(given: &PlayerName) -> Result<Option<Self>> {
        let connection = session()?;
        let Some(name) = ours(&connection)?
            .into_iter()
            .find(|name| answers_to(name.as_str(), given.as_str()))
        else {
            return Ok(None);
        };
        Ok(Some(Self { connection, name }))
    }

    pub const fn name(&self) -> &WellKnownName<'static> {
        &self.name
    }

    pub fn standing(&self) -> Result<Standing> {
        let playback = self.playback()?;
        let metadata: HashMap<String, OwnedValue> = self
            .proxy(PLAYER)?
            .get_property(METADATA)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(Standing {
            name: self.name.clone().into(),
            playback,
            queued: self.tracks()?.len(),
            title: text(&metadata, TITLE),
            artist: first(&metadata, ARTIST),
        })
    }

    pub fn playback(&self) -> Result<Option<PlaybackStatus>> {
        let status: String = self
            .proxy(PLAYER)?
            .get_property(PLAYBACK_STATUS)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(PlaybackStatus::read(&status))
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
        self.proxy(ROOT)?
            .get_property(CAN_RAISE)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    pub fn raise(&self) -> Result<()> {
        self.told(ROOT, RAISE)
    }

    pub fn play(&self) -> Result<()> {
        self.told(PLAYER, PLAY)
    }

    pub fn pause(&self) -> Result<()> {
        self.told(PLAYER, PAUSE)
    }

    pub fn play_pause(&self) -> Result<()> {
        self.told(PLAYER, PLAY_PAUSE)
    }

    pub fn stop(&self) -> Result<()> {
        self.told(PLAYER, STOP)
    }

    pub fn next(&self) -> Result<()> {
        self.told(PLAYER, NEXT)
    }

    pub fn previous(&self) -> Result<()> {
        self.told(PLAYER, PREVIOUS)
    }

    pub fn seek(&self, by: Seeking) -> Result<()> {
        self.proxy(PLAYER)?
            .call::<_, _, ()>(SEEK, &(by.offset(),))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    pub fn set_position(&self, track: TrackId, position: Duration) -> Result<()> {
        self.proxy(PLAYER)?
            .call::<_, _, ()>(SET_POSITION, &(track_path(track), micros_of(position)))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    pub fn volume(&self) -> Result<Volume> {
        let reading: f64 = self
            .proxy(PLAYER)?
            .get_property(VOLUME)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Volume::new(reading as f32).map_err(|_| Error::NotAVolume { reading })
    }

    pub fn set_volume(&self, volume: Volume) -> Result<()> {
        self.proxy(PLAYER)?
            .set_property(VOLUME, f64::from(volume.get()))
            .map_err(|source| Error::bus(BusOp::Write, source.into()))
    }

    pub fn metadata(&self) -> Result<Option<Described>> {
        let fields: HashMap<String, OwnedValue> = self
            .proxy(PLAYER)?
            .get_property(METADATA)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(described(&fields))
    }

    pub fn position(&self) -> Result<Duration> {
        let micros: i64 = self
            .proxy(PLAYER)?
            .get_property(POSITION)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(span_of(micros))
    }

    pub fn rows(&self) -> Result<Vec<Described>> {
        self.described(&self.queued()?)
    }

    pub fn queued(&self) -> Result<Vec<TrackId>> {
        Ok(self
            .tracks()?
            .iter()
            .filter_map(|path| track_of(path.as_str()))
            .collect())
    }

    pub fn described(&self, rows: &[TrackId]) -> Result<Vec<Described>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let paths: Vec<OwnedObjectPath> = rows.iter().copied().map(track_path).collect();
        let listed: Vec<HashMap<String, OwnedValue>> = self
            .proxy(TRACK_LIST)?
            .call(GET_TRACKS_METADATA, &(paths,))
            .map_err(|source| Error::bus(BusOp::Call, source))?;

        Ok(listed.iter().filter_map(described).collect())
    }

    pub fn remove_track(&self, track: TrackId) -> Result<()> {
        self.proxy(TRACK_LIST)?
            .call::<_, _, ()>(REMOVE_TRACK, &(track_path(track),))
            .map_err(|source| Error::bus(BusOp::Call, source))
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
        let listed: Vec<(OwnedObjectPath, String, String)> = self
            .proxy(PLAYLISTS)?
            .call(GET_PLAYLISTS, &(index, max_count, order.name(), reverse))
            .map_err(|source| Error::bus(BusOp::Call, source))?;

        Ok(listed
            .into_iter()
            .filter_map(|(path, name, _)| {
                Some(PlaylistInfo {
                    id: playlist_of(path.as_str())?,
                    name,
                })
            })
            .collect())
    }

    pub fn activate_playlist(&self, playlist: PlaylistId) -> Result<()> {
        self.proxy(PLAYLISTS)?
            .call::<_, _, ()>(ACTIVATE_PLAYLIST, &(playlist_path(playlist),))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    pub fn sleep(&self) -> Result<Option<Asleep>> {
        let (mode, seconds): (String, u64) = self
            .proxy(OWN_INTERFACE)?
            .get_property(SLEEP)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(sleep_timer(&mode, seconds))
    }

    pub fn set_sleep(&self, until: Option<Until>) -> Result<()> {
        let (mode, seconds) = sleep_asked(until);

        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(SET_SLEEP, &(mode, seconds))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn told(&self, interface: &'static str, method: &'static str) -> Result<()> {
        self.proxy(interface)?
            .call::<_, _, ()>(method, &())
            .map_err(|source| Error::bus(BusOp::Call, source))
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
        let tracks = self.proxy(TRACK_LIST)?;

        for (place, (location, span)) in rows.iter().enumerate().rev() {
            let heard_now = queueing.play && place == 0;
            tracks
                .call_method(
                    ADD_TRACK,
                    &(location.to_uri_within(*span), &after, heard_now),
                )
                .map_err(|source| Error::bus(BusOp::Queue, source))?;
        }
        Ok(rows.len())
    }

    fn landing(&self, at: Placement) -> Result<OwnedObjectPath> {
        let tracks = self.tracks()?;
        let row = match at {
            Placement::At(row) => row.min(tracks.len()),
            Placement::Next => self.after_the_playing(&tracks, 0)?,
            Placement::Queued => self.after_the_playing(&tracks, self.waiting()?)?,
        };

        Ok(row
            .checked_sub(1)
            .and_then(|before| tracks.get(before).cloned())
            .unwrap_or_else(no_track))
    }

    fn after_the_playing(&self, tracks: &[OwnedObjectPath], waiting: usize) -> Result<usize> {
        Ok(self.playing(tracks)?.map_or(tracks.len(), |at| {
            at.saturating_add(1)
                .saturating_add(waiting)
                .min(tracks.len())
        }))
    }

    fn waiting(&self) -> Result<usize> {
        let waiting: u32 = self
            .proxy(OWN_INTERFACE)?
            .get_property(PLAYING_NEXT)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(usize::try_from(waiting).unwrap_or(usize::MAX))
    }

    fn tracks(&self) -> Result<Vec<OwnedObjectPath>> {
        self.proxy(TRACK_LIST)?
            .get_property(TRACKS)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn playing(&self, tracks: &[OwnedObjectPath]) -> Result<Option<usize>> {
        let metadata: HashMap<String, OwnedValue> = self
            .proxy(PLAYER)?
            .get_property(METADATA)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        let Some(playing) = metadata
            .get(TRACK_ID)
            .and_then(|value| OwnedObjectPath::try_from(value.clone()).ok())
            .filter(|path| path.as_str() != NO_TRACK)
        else {
            return Ok(None);
        };
        Ok(tracks.iter().position(|path| *path == playing))
    }

    fn proxy(&self, interface: &'static str) -> Result<Proxy<'_>> {
        Proxy::new(&self.connection, self.name.clone(), OBJECT_PATH, interface)
            .map_err(|source| Error::bus(BusOp::Connect, source))
    }
}

fn session() -> Result<Connection> {
    connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))
}

fn ours(connection: &Connection) -> Result<Vec<WellKnownName<'static>>> {
    let bus = DBusProxy::new(connection).map_err(|source| Error::bus(BusOp::Find, source))?;
    let names = bus
        .list_names()
        .map_err(|source| Error::bus(BusOp::Find, source.into()))?;

    let mut ours: Vec<&str> = names
        .iter()
        .map(|name| name.as_str())
        .filter(|name| names_this_build(name))
        .collect();
    ours.sort_unstable();

    ours.into_iter()
        .map(|name| {
            WellKnownName::try_from(name.to_owned())
                .map_err(|source| Error::bus(BusOp::Find, source.into()))
        })
        .collect()
}

fn names_this_build(name: &str) -> bool {
    name == BUS_NAME || name.strip_prefix(BUS_NAME).is_some_and(names_an_instance)
}

fn names_an_instance(rest: &str) -> bool {
    rest.strip_prefix('.')
        .is_some_and(|instance| !instance.is_empty())
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

fn described(fields: &HashMap<String, OwnedValue>) -> Option<Described> {
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

fn length(fields: &HashMap<String, OwnedValue>) -> Option<Duration> {
    let micros = i64::try_from(fields.get(LENGTH)?.clone()).ok()?;

    Some(span_of(micros))
}

fn text(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
    String::try_from(metadata.get(key)?.clone()).ok()
}

fn first(metadata: &HashMap<String, OwnedValue>, key: &str) -> Option<String> {
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
        assert!(names_this_build(BUS_NAME));
        assert!(names_this_build(&format!("{BUS_NAME}.instance1234")));
        assert!(!names_this_build(&format!("{BUS_NAME}.")));
        assert!(!names_this_build(&format!("{BUS_NAME}-other")));
        assert!(!names_this_build("org.mpris.MediaPlayer2.vlc"));
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
