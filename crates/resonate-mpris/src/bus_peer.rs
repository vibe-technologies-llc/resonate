use std::{collections::HashMap, path::Path, sync::Arc};

use resonate_core::{PlaylistId, TrackId, ZoneId};
use resonate_engine::{Asleep, Until};
use zbus::{OwnedObjectPath, OwnedVariant, names::WellKnownName};

use crate::{
    BusOp, Described, Error, PlaybackStatus, PlaylistInfo, PlaylistOrder, Reloadable, Result,
    blocking::{Connection, Proxy, fdo::DBusProxy},
    interfaces::OWN_INTERFACE,
    peer::Peer,
    remote::{described, names_this_build},
    service::OBJECT_PATH,
    track::{no_track, playlist_of, playlist_path, sleep_asked, sleep_timer, track_of, track_path},
    wire::Act,
};

const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const TRACK_LIST: &str = "org.mpris.MediaPlayer2.TrackList";
const PLAYLISTS: &str = "org.mpris.MediaPlayer2.Playlists";
const RAISE: &str = "Raise";
const CAN_RAISE: &str = "CanRaise";
const ADD_TRACK: &str = "AddTrack";
const ADD_TRACKS: &str = "AddTracks";
const REMOVE_TRACK: &str = "RemoveTrack";
const GET_TRACKS_METADATA: &str = "GetTracksMetadata";
const GET_PLAYLISTS: &str = "GetPlaylists";
const ACTIVATE_PLAYLIST: &str = "ActivatePlaylist";
const SET_SLEEP: &str = "SetSleep";
const RELOCATE: &str = "Relocate";
const TUNE: &str = "Tune";
const RELOAD: &str = "Reload";
const HAND_OVER: &str = "HandOver";
const PLAYING_NEXT: &str = "PlayingNext";
const ZONE: &str = "Zone";
const PLAY_ORDER: &str = "PlayOrder";
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
const PLAYBACK_STATUS: &str = "PlaybackStatus";
const VOLUME: &str = "Volume";
const SHUFFLE: &str = "Shuffle";
const POSITION: &str = "Position";
const SLEEP: &str = "Sleep";

pub(crate) struct BusPeer {
    connection: Connection,
    name: WellKnownName<'static>,
}

impl BusPeer {
    pub(crate) const fn new(connection: Connection, name: WellKnownName<'static>) -> Self {
        Self { connection, name }
    }

    pub(crate) fn serving(
        connection: &Connection,
    ) -> Result<Vec<(WellKnownName<'static>, Arc<dyn Peer>)>> {
        Ok(ours(connection)?
            .into_iter()
            .map(|name| {
                let peer: Arc<dyn Peer> = Arc::new(Self::new(connection.clone(), name.clone()));
                (name, peer)
            })
            .collect())
    }

    fn proxy(&self, interface: &'static str) -> Result<Proxy<'_>> {
        Proxy::new(&self.connection, self.name.clone(), OBJECT_PATH, interface)
            .map_err(|source| Error::bus(BusOp::Connect, source))
    }

    fn metadata_fields(&self) -> Result<HashMap<String, OwnedVariant>> {
        self.proxy(PLAYER)?
            .get_property(METADATA)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn added_one_by_one(&self, uris: &[String], after: &OwnedObjectPath, play: bool) -> Result<()> {
        let tracks = self.proxy(TRACK_LIST)?;
        for (place, uri) in uris.iter().enumerate().rev() {
            let heard_now = play && place == 0;
            tracks
                .call_method(ADD_TRACK, &(uri, after, heard_now))
                .map_err(|source| Error::bus(BusOp::Queue, source))?;
        }
        Ok(())
    }
}

impl Peer for BusPeer {
    fn can_raise(&self) -> Result<bool> {
        self.proxy(ROOT)?
            .get_property(CAN_RAISE)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn told(&self, act: Act) -> Result<()> {
        let (interface, method) = match act {
            Act::Raise => (ROOT, RAISE),
            Act::Play => (PLAYER, PLAY),
            Act::Pause => (PLAYER, PAUSE),
            Act::PlayPause => (PLAYER, PLAY_PAUSE),
            Act::Stop => (PLAYER, STOP),
            Act::Next => (PLAYER, NEXT),
            Act::Previous => (PLAYER, PREVIOUS),
        };

        self.proxy(interface)?
            .call::<_, _, ()>(method, &())
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn seek(&self, micros: i64) -> Result<()> {
        self.proxy(PLAYER)?
            .call::<_, _, ()>(SEEK, &(micros,))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn set_position(&self, track: TrackId, micros: i64) -> Result<()> {
        self.proxy(PLAYER)?
            .call::<_, _, ()>(SET_POSITION, &(track_path(track), micros))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn playback(&self) -> Result<Option<PlaybackStatus>> {
        let status: String = self
            .proxy(PLAYER)?
            .get_property(PLAYBACK_STATUS)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(PlaybackStatus::read(&status))
    }

    fn volume(&self) -> Result<f64> {
        self.proxy(PLAYER)?
            .get_property(VOLUME)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn set_volume(&self, volume: f64) -> Result<()> {
        self.proxy(PLAYER)?
            .set_property(VOLUME, volume)
            .map_err(|source| Error::bus(BusOp::Write, source.into()))
    }

    fn shuffle(&self) -> Result<bool> {
        self.proxy(PLAYER)?
            .get_property(SHUFFLE)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn metadata(&self) -> Result<Option<Described>> {
        Ok(described(&self.metadata_fields()?))
    }

    fn position(&self) -> Result<i64> {
        self.proxy(PLAYER)?
            .get_property(POSITION)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn queued(&self) -> Result<Vec<TrackId>> {
        let paths: Vec<OwnedObjectPath> = self
            .proxy(TRACK_LIST)?
            .get_property(TRACKS)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(paths
            .iter()
            .filter_map(|path| track_of(path.as_str()))
            .collect())
    }

    fn play_order(&self) -> Result<Vec<TrackId>> {
        let paths: Vec<OwnedObjectPath> = self
            .proxy(OWN_INTERFACE)?
            .get_property(PLAY_ORDER)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(paths
            .iter()
            .filter_map(|path| track_of(path.as_str()))
            .collect())
    }

    fn described(&self, rows: &[TrackId]) -> Result<Vec<Described>> {
        let paths: Vec<OwnedObjectPath> = rows.iter().copied().map(track_path).collect();
        let listed: Vec<HashMap<String, OwnedVariant>> = self
            .proxy(TRACK_LIST)?
            .call(GET_TRACKS_METADATA, &(paths,))
            .map_err(|source| Error::bus(BusOp::Call, source))?;

        Ok(listed.iter().filter_map(described).collect())
    }

    fn remove_track(&self, track: TrackId) -> Result<()> {
        self.proxy(TRACK_LIST)?
            .call::<_, _, ()>(REMOVE_TRACK, &(track_path(track),))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn playlists(
        &self,
        order: PlaylistOrder,
        reverse: bool,
        from: u32,
        most: u32,
    ) -> Result<Vec<PlaylistInfo>> {
        let listed: Vec<(OwnedObjectPath, String, String)> = self
            .proxy(PLAYLISTS)?
            .call(GET_PLAYLISTS, &(from, most, order.name(), reverse))
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

    fn activate_playlist(&self, playlist: PlaylistId) -> Result<()> {
        self.proxy(PLAYLISTS)?
            .call::<_, _, ()>(ACTIVATE_PLAYLIST, &(playlist_path(playlist),))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn sleep(&self) -> Result<Option<Asleep>> {
        let (mode, seconds): (String, u64) = self
            .proxy(OWN_INTERFACE)?
            .get_property(SLEEP)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(sleep_timer(&mode, seconds))
    }

    fn set_sleep(&self, until: Option<Until>) -> Result<()> {
        let (mode, seconds) = sleep_asked(until);

        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(SET_SLEEP, &(mode, seconds))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn add_tracks(&self, uris: &[String], after: Option<TrackId>, play: bool) -> Result<()> {
        let after = after.map_or_else(no_track, track_path);

        match self
            .proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(ADD_TRACKS, &(uris, &after, play))
        {
            Ok(()) => Ok(()),
            Err(zbus::Error::FDO(refused))
                if matches!(*refused, zbus::fdo::Error::UnknownMethod(_)) =>
            {
                self.added_one_by_one(uris, &after, play)
            }
            Err(source) => Err(Error::bus(BusOp::Queue, source)),
        }
    }

    fn relocate(&self, moved: &[(String, String)]) -> Result<()> {
        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(RELOCATE, &(moved,))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn tune(&self, settings: &[(String, String)]) -> Result<()> {
        let pairs: Vec<(&str, &str)> = settings
            .iter()
            .map(|(setting, value)| (setting.as_str(), value.as_str()))
            .collect();

        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(TUNE, &(pairs,))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn reload(&self, group: Reloadable, settings: &Path) -> Result<()> {
        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(
                RELOAD,
                &(group.name(), settings.to_string_lossy().into_owned()),
            )
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn hand_over(&self, to: ZoneId) -> Result<()> {
        self.proxy(OWN_INTERFACE)?
            .call::<_, _, ()>(HAND_OVER, &(to.get(),))
            .map_err(|source| Error::bus(BusOp::Call, source))
    }

    fn zone(&self) -> Result<String> {
        self.proxy(OWN_INTERFACE)?
            .get_property(ZONE)
            .map_err(|source| Error::bus(BusOp::Read, source))
    }

    fn waiting(&self) -> Result<usize> {
        let waiting: u32 = self
            .proxy(OWN_INTERFACE)?
            .get_property(PLAYING_NEXT)
            .map_err(|source| Error::bus(BusOp::Read, source))?;

        Ok(usize::try_from(waiting).unwrap_or(usize::MAX))
    }
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
                .map_err(|source| Error::bus(BusOp::Find, source))
        })
        .collect()
}
