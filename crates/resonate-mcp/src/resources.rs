use resonate_core::{uri_escaped, uri_unescaped};
use resonate_library::{Direction, Library, PlaylistName, PlaylistOrder, Window};
use serde_json::{Value, json};

use crate::{
    Result, catalog,
    completions::Completable,
    controlling::Reach,
    passes::Passes,
    tools::{LISTED_BY_DEFAULT, MISSING_BY_DEFAULT, QUEUED_BY_DEFAULT, TOP_BY_DEFAULT},
    transport,
};

const JSON: &str = "application/json";
const NOW_PLAYING: &str = "resonate://player/now-playing";
const QUEUE: &str = "resonate://player/queue";
const PASSES: &str = "resonate://library/passes";
const PLAYLISTS: &str = "resonate://library/playlists";
const FAVOURITES: &str = "resonate://library/favourites";
const STATISTICS: &str = "resonate://library/statistics";
const STATISTICS_OVER: &str = "resonate://library/statistics/";
const STATISTICS_TEMPLATE: &str = "resonate://library/statistics/{window}";
const SUGGESTIONS: &str = "resonate://library/suggestions";
const MISSING: &str = "resonate://library/missing";
const ONE_PLAYLIST: &str = "resonate://library/playlist/";
const ONE_PLAYLIST_TEMPLATE: &str = "resonate://library/playlist/{name}";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Resource {
    NowPlaying,
    Queue,
    Passes,
    Playlists,
    Playlist(PlaylistName),
    Favourites,
    Statistics(Window),
    Suggestions,
    Missing,
}

impl Resource {
    pub const FIXED: [Self; 8] = [
        Self::NowPlaying,
        Self::Queue,
        Self::Passes,
        Self::Playlists,
        Self::Favourites,
        Self::Statistics(Window::Month),
        Self::Suggestions,
        Self::Missing,
    ];

    pub fn at(uri: &str) -> Option<Self> {
        if let Some(escaped) = uri.strip_prefix(ONE_PLAYLIST) {
            let name = String::from_utf8(uri_unescaped(escaped)?).ok()?;
            return (!name.trim().is_empty()).then(|| Self::Playlist(PlaylistName::new(name)));
        }
        if let Some(named) = uri.strip_prefix(STATISTICS_OVER) {
            return Window::ALL
                .into_iter()
                .find(|window| window.name() == named)
                .map(Self::Statistics);
        }
        Self::FIXED.into_iter().find(|fixed| fixed.uri() == uri)
    }

    pub fn uri(&self) -> String {
        match self {
            Self::NowPlaying => NOW_PLAYING.to_owned(),
            Self::Queue => QUEUE.to_owned(),
            Self::Passes => PASSES.to_owned(),
            Self::Playlists => PLAYLISTS.to_owned(),
            Self::Playlist(name) => {
                format!("{ONE_PLAYLIST}{}", uri_escaped(name.as_str().as_bytes()))
            }
            Self::Favourites => FAVOURITES.to_owned(),
            Self::Statistics(Window::Month) => STATISTICS.to_owned(),
            Self::Statistics(window) => format!("{STATISTICS_OVER}{}", window.name()),
            Self::Suggestions => SUGGESTIONS.to_owned(),
            Self::Missing => MISSING.to_owned(),
        }
    }

    fn name(&self) -> String {
        match self {
            Self::NowPlaying => "now_playing".to_owned(),
            Self::Queue => "queue".to_owned(),
            Self::Passes => "library_passes".to_owned(),
            Self::Playlists => "playlists".to_owned(),
            Self::Playlist(name) => name.to_string(),
            Self::Favourites => "favourites".to_owned(),
            Self::Statistics(Window::Month) => "listening_statistics".to_owned(),
            Self::Statistics(window) => format!("listening_statistics_{}", window.name()),
            Self::Suggestions => "suggested_playlists".to_owned(),
            Self::Missing => "missing_tracks".to_owned(),
        }
    }

    fn describes(&self) -> String {
        match self {
            Self::NowPlaying => "What the running player is playing, where in it the player is, \
                                 whether it is playing or paused, the volume and any sleep timer."
                .to_owned(),
            Self::Queue => format!(
                "The running player's queue in play order, its first {QUEUED_BY_DEFAULT} rows \
                 described, each with the queue_id remove_from_queue takes."
            ),
            Self::Passes => "Whether the scan, the lookup and the poll this session started are \
                             idle, running or finished, and how far each has come."
                .to_owned(),
            Self::Playlists => "Every playlist the library holds, with its length, its play count \
                                and, for one that fills itself, the search it fills from."
                .to_owned(),
            Self::Playlist(name) => format!(
                "The first {LISTED_BY_DEFAULT} rows of the playlist {name}, each with the \
                 track_id add_to_queue takes."
            ),
            Self::Favourites => format!(
                "The {LISTED_BY_DEFAULT} tracks, albums and artists most recently marked as \
                 favourites."
            ),
            Self::Statistics(window) => format!(
                "What was listened to over {}: the totals and the {TOP_BY_DEFAULT} tracks, \
                 albums and artists heard most.",
                over(*window)
            ),
            Self::Suggestions => "The playlists the catalog suggests to itself, each with the \
                                  search it would fill from."
                .to_owned(),
            Self::Missing => format!(
                "The first {MISSING_BY_DEFAULT} tracks of held albums the catalog holds no file \
                 for, each with the release_track_id want_tracks takes."
            ),
        }
    }

    pub(crate) fn listed(&self) -> Value {
        json!({
            "uri": self.uri(),
            "name": self.name(),
            "description": self.describes(),
            "mimeType": JSON,
        })
    }

    pub(crate) fn templates() -> Value {
        json!(Template::ALL.map(Template::listed))
    }

    pub(crate) fn read(
        &self,
        library: &Library,
        players: &dyn Reach,
        passes: &Passes,
    ) -> Result<Value> {
        match self {
            Self::NowPlaying => players
                .player()
                .and_then(|player| transport::now_playing(&*player)),
            Self::Queue => players
                .player()
                .and_then(|player| transport::queue(&*player, QUEUED_BY_DEFAULT)),
            Self::Passes => Ok(passes.states()),
            Self::Playlists => catalog::playlists(library, None),
            Self::Playlist(name) => {
                catalog::playlist_tracks(library, name.as_str(), None, LISTED_BY_DEFAULT)
            }
            Self::Favourites => catalog::favourites(library, LISTED_BY_DEFAULT),
            Self::Statistics(window) => catalog::statistics(library, *window, TOP_BY_DEFAULT),
            Self::Suggestions => catalog::suggestions(library),
            Self::Missing => catalog::missing(library, None, MISSING_BY_DEFAULT),
        }
    }

    pub(crate) fn contents(asked: &str, read: &Value) -> Value {
        json!({ "contents": [written(asked, read)] })
    }

    pub(crate) fn embedded(
        &self,
        library: &Library,
        players: &dyn Reach,
        passes: &Passes,
    ) -> Result<Value> {
        let read = self.read(library, players, passes)?;
        Ok(json!({ "type": "resource", "resource": written(&self.uri(), &read) }))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Template {
    Playlist,
    Statistics,
}

impl Template {
    const ALL: [Self; 2] = [Self::Playlist, Self::Statistics];

    pub(crate) fn at(uri: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|template| template.uri() == uri)
    }

    const fn uri(self) -> &'static str {
        match self {
            Self::Playlist => ONE_PLAYLIST_TEMPLATE,
            Self::Statistics => STATISTICS_TEMPLATE,
        }
    }

    const fn argument(self) -> &'static str {
        match self {
            Self::Playlist => "name",
            Self::Statistics => "window",
        }
    }

    pub(crate) fn completes(self, argument: &str) -> Option<Completable> {
        (argument == self.argument()).then_some(match self {
            Self::Playlist => Completable::Playlist,
            Self::Statistics => Completable::Window,
        })
    }

    fn listed(self) -> Value {
        match self {
            Self::Playlist => json!({
                "uriTemplate": self.uri(),
                "name": "playlist",
                "description": format!(
                    "The first {LISTED_BY_DEFAULT} rows of one playlist, by the name playlists \
                     gives it."
                ),
                "mimeType": JSON,
            }),
            Self::Statistics => json!({
                "uriTemplate": self.uri(),
                "name": "listening_statistics_over",
                "description": format!(
                    "What was listened to over a window — {} — with the {TOP_BY_DEFAULT} tracks, \
                     albums and artists heard most.",
                    Window::ALL.map(Window::name).join(", ")
                ),
                "mimeType": JSON,
            }),
        }
    }
}

fn written(uri: &str, read: &Value) -> Value {
    json!({ "uri": uri, "mimeType": JSON, "text": read.to_string() })
}

pub(crate) const fn over(window: Window) -> &'static str {
    match window {
        Window::Week => "the last week",
        Window::Month => "the last month",
        Window::Year => "the last year",
        Window::Everything => "all time",
    }
}

pub(crate) fn every(library: &Library) -> Result<Vec<Resource>> {
    let playlists = library.playlists(PlaylistOrder::Name, Direction::Ascending, None)?;

    Ok(Resource::FIXED
        .into_iter()
        .chain(
            playlists
                .into_iter()
                .map(|playlist| Resource::Playlist(PlaylistName::new(playlist.name))),
        )
        .collect())
}
