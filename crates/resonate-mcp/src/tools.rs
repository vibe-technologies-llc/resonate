use std::{fmt, num::NonZeroU64, path::PathBuf, time::Duration};

use resonate_core::{AlbumId, ArtistId, ReleaseTrackId, Span, TrackId, Volume};
use resonate_engine::Until;
use resonate_library::{Favoured, Library, Window};
use resonate_mpris::Seeking;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    Refusal, Result,
    catalog::{self, Wanted},
    controlling::Reach,
    edits::{self, Dropping, Filling, Marking},
    passes::{Pass, Passes},
    transport::{self, Action, Adding},
};

const MOST_ROWS: usize = 1_000;
const SEARCHED_BY_DEFAULT: usize = 25;
pub(crate) const LISTED_BY_DEFAULT: usize = 200;
pub(crate) const QUEUED_BY_DEFAULT: usize = 100;
pub(crate) const TOP_BY_DEFAULT: usize = 10;
pub(crate) const MISSING_BY_DEFAULT: usize = 100;
const SECONDS_A_MINUTE: u64 = 60;
const WHOLE: f64 = 100.0;

const GRAMMAR: &str = "Words match the title, artist and album. `artist:`, `album:`, `title:`, \
                       `genre:` and `lyrics:` scope the words after them; `year:1970-1979`, \
                       `added:<30d`, `plays:0`, `plays:>20@30d`, `played:>1y`, `length:>10:00`, \
                       `rate:>48000`, `depth:24`, `codec:flac`, `is:hires`, `is:lossless`, \
                       `is:lossy` and `is:favourite` narrow. A leading `-` denies a token, `or` \
                       offers an alternative, and quotes make a phrase.";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Search,
    Playlists,
    PlaylistTracks,
    Favourites,
    Statistics,
    Suggestions,
    Missing,
    MarkFavourite,
    CreatePlaylist,
    AddToPlaylist,
    RemoveFromPlaylist,
    RenamePlaylist,
    DiscardPlaylist,
    WantTracks,
    NowPlaying,
    Control,
    Seek,
    SetVolume,
    Queue,
    AddToQueue,
    RemoveFromQueue,
    PlayPlaylist,
    SleepTimer,
    StartScan,
    StartLookup,
    StartPoll,
    LibraryPasses,
    StopPass,
}

impl Tool {
    pub const ALL: [Self; 28] = [
        Self::Search,
        Self::Playlists,
        Self::PlaylistTracks,
        Self::Favourites,
        Self::Statistics,
        Self::Suggestions,
        Self::Missing,
        Self::MarkFavourite,
        Self::CreatePlaylist,
        Self::AddToPlaylist,
        Self::RemoveFromPlaylist,
        Self::RenamePlaylist,
        Self::DiscardPlaylist,
        Self::WantTracks,
        Self::NowPlaying,
        Self::Control,
        Self::Seek,
        Self::SetVolume,
        Self::Queue,
        Self::AddToQueue,
        Self::RemoveFromQueue,
        Self::PlayPlaylist,
        Self::SleepTimer,
        Self::StartScan,
        Self::StartLookup,
        Self::StartPoll,
        Self::LibraryPasses,
        Self::StopPass,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Search => "search_library",
            Self::Playlists => "list_playlists",
            Self::PlaylistTracks => "playlist_tracks",
            Self::Favourites => "favourites",
            Self::Statistics => "listening_statistics",
            Self::Suggestions => "suggested_playlists",
            Self::Missing => "list_missing",
            Self::MarkFavourite => "mark_favourite",
            Self::CreatePlaylist => "create_playlist",
            Self::AddToPlaylist => "add_to_playlist",
            Self::RemoveFromPlaylist => "remove_from_playlist",
            Self::RenamePlaylist => "rename_playlist",
            Self::DiscardPlaylist => "discard_playlist",
            Self::WantTracks => "want_tracks",
            Self::NowPlaying => "now_playing",
            Self::Control => "control_playback",
            Self::Seek => "seek",
            Self::SetVolume => "set_volume",
            Self::Queue => "show_queue",
            Self::AddToQueue => "add_to_queue",
            Self::RemoveFromQueue => "remove_from_queue",
            Self::PlayPlaylist => "play_playlist",
            Self::SleepTimer => "set_sleep_timer",
            Self::StartScan => "start_scan",
            Self::StartLookup => "start_lookup",
            Self::StartPoll => "start_poll",
            Self::LibraryPasses => "library_passes",
            Self::StopPass => "stop_pass",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    pub const fn reads_only(self) -> bool {
        matches!(
            self,
            Self::Search
                | Self::Playlists
                | Self::PlaylistTracks
                | Self::Favourites
                | Self::Statistics
                | Self::Suggestions
                | Self::Missing
                | Self::NowPlaying
                | Self::Queue
                | Self::LibraryPasses
        )
    }

    pub const fn destroys(self) -> bool {
        matches!(
            self,
            Self::RemoveFromPlaylist
                | Self::DiscardPlaylist
                | Self::RemoveFromQueue
                | Self::PlayPlaylist
        )
    }

    fn describes(self) -> String {
        match self {
            Self::Search => format!(
                "Search the music library's catalog for tracks, albums and artists. Needs no \
                 player. {GRAMMAR}"
            ),
            Self::Playlists => "List the playlists the library holds, with their lengths, play \
                                counts and, for one that fills itself, the search it fills from. \
                                Needs no player."
                .to_owned(),
            Self::PlaylistTracks => "List the tracks of one playlist, named as list_playlists \
                                     names it. Needs no player."
                .to_owned(),
            Self::Favourites => {
                "List the tracks, albums and artists marked as favourites, most recently marked \
                 first. Needs no player."
                    .to_owned()
            }
            Self::Statistics => "Say what was listened to over a window: the totals and the \
                                 tracks, albums and artists heard most. Needs no player."
                .to_owned(),
            Self::Suggestions => "List the playlists the catalog suggests to itself, each with \
                                  the search it would fill from. Needs no player."
                .to_owned(),
            Self::Missing => format!(
                "List the tracks of held albums the catalog knows of and holds no file for, each \
                 with the release_track_id want_tracks takes and whether it is wanted already. \
                 Needs no player. {GRAMMAR}"
            ),
            Self::MarkFavourite => "Mark tracks, albums or artists as favourites, or take the \
                                    mark away with favourite set to false, by the ids \
                                    search_library gives them. Needs no player."
                .to_owned(),
            Self::CreatePlaylist => format!(
                "Make a playlist in the library: empty, holding catalog track_ids or the matches \
                 of a query in album order, or one that fills itself from a search every time it \
                 is read, with fills_from. Needs no player. {GRAMMAR}"
            ),
            Self::AddToPlaylist => format!(
                "Append tracks to the end of a playlist: catalog track_ids, or the matches of a \
                 query in album order. A playlist that fills itself from a search takes none. \
                 Needs no player. {GRAMMAR}"
            ),
            Self::RemoveFromPlaylist => "Take rows out of a playlist: a row, or a run from row \
                                         to through_row, as playlist_tracks numbers them, or \
                                         every row a search matches. Needs no player."
                .to_owned(),
            Self::RenamePlaylist => "Give a playlist another name. Needs no player.".to_owned(),
            Self::DiscardPlaylist => "Take a playlist and its rows away for good, or the search \
                                      one that fills itself fills from. The files it named stay \
                                      where they are. Needs no player."
                .to_owned(),
            Self::WantTracks => "Mark missing tracks as wanted, by the release_track_id \
                                 list_missing gives them, so the library's providers look for \
                                 them. Needs no player."
                .to_owned(),
            Self::NowPlaying => "Say what the running player is playing: the track, where in it \
                                 the player is, whether it is playing or paused, the volume and \
                                 any sleep timer."
                .to_owned(),
            Self::Control => "Play, pause, toggle between the two, stop, or go to the next or \
                              previous track on the running player, and say what it is doing \
                              once it has."
                .to_owned(),
            Self::Seek => "Move the playing track forward by a number of seconds, or back by a \
                           negative number, and say where it landed."
                .to_owned(),
            Self::SetVolume => "Set the running player's volume, as a percentage, and read it \
                                back."
                .to_owned(),
            Self::Queue => "List the running player's queue in play order, each row with the \
                            queue_id remove_from_queue takes, marking the row being played."
                .to_owned(),
            Self::AddToQueue => format!(
                "Add tracks to the running player's queue: either catalog track_ids from \
                 search_library, playlist_tracks or favourites, or a search whose matches are \
                 queued in album order. They go at the end, or after the row being played with \
                 next, and play starts on the first of them with play. {GRAMMAR}"
            ),
            Self::RemoveFromQueue => "Take one row out of the running player's queue, by the \
                                      queue_id show_queue gives it."
                .to_owned(),
            Self::PlayPlaylist => "Replace the running player's queue with a playlist, by the \
                                   name list_playlists gives it, and play it."
                .to_owned(),
            Self::SleepTimer => "Pause the running player after a number of minutes, at the end \
                                 of the track playing or at the end of the queue, or take a timer \
                                 away with off. Give minutes or at, not both."
                .to_owned(),
            Self::StartScan => "Start reading the library's folders into its catalog, or new \
                                folders given as roots, which are kept as library roots from \
                                then on. Answers at once; library_passes says how far it has \
                                come. Needs no player."
                .to_owned(),
            Self::StartLookup => "Start asking MusicBrainz and the other services this build \
                                  reaches about the albums, tracks and artists not asked about \
                                  lately, or about every one of them again with refresh. \
                                  Carries on a lookup an earlier run left unfinished. Answers at \
                                  once; library_passes says how far it has come. Needs no \
                                  player."
                .to_owned(),
            Self::StartPoll => "Start asking every registered provider for the wanted tracks \
                                not asked about lately, or every wanted track again with again. \
                                Answers at once; library_passes says how far it has come. Needs \
                                no player."
                .to_owned(),
            Self::LibraryPasses => "Say, for the scan, the lookup and the poll this session \
                                    started, whether each is idle, running with how far it has \
                                    come, or finished with what it found. Needs no player."
                .to_owned(),
            Self::StopPass => "Stop a scan, lookup or poll this session started at the next \
                               file, and say where it stands. What it has done stays done. \
                               Needs no player."
                .to_owned(),
        }
    }

    fn takes(self) -> Value {
        let properties = match self {
            Self::Search => json!({
                "query": { "type": "string", "description": "What to search for." },
                "limit": limit(SEARCHED_BY_DEFAULT, "The most of each kind to answer with."),
            }),
            Self::Playlists => json!({
                "named": {
                    "type": "string",
                    "description": "Only the playlists whose name holds every one of these words.",
                },
            }),
            Self::PlaylistTracks => json!({
                "playlist": { "type": "string", "description": "The playlist's name." },
                "matching": {
                    "type": "string",
                    "description": "Only the rows this search matches, in the library's grammar.",
                },
                "limit": limit(LISTED_BY_DEFAULT, "The most rows to answer with."),
            }),
            Self::Favourites => json!({
                "limit": limit(LISTED_BY_DEFAULT, "The most of each kind to answer with."),
            }),
            Self::Statistics => json!({
                "window": {
                    "type": "string",
                    "enum": Window::ALL.map(Window::name),
                    "default": Window::Month.name(),
                    "description": "How far back to look.",
                },
                "top": limit(TOP_BY_DEFAULT, "How many of each kind heard most to list."),
            }),
            Self::Missing => json!({
                "query": {
                    "type": "string",
                    "description": "Only the albums holding a track this search matches.",
                },
                "limit": limit(MISSING_BY_DEFAULT, "The most rows to answer with."),
            }),
            Self::MarkFavourite => json!({
                "track_ids": ids("Catalog track ids."),
                "album_ids": ids("Catalog album ids."),
                "artist_ids": ids("Catalog artist ids."),
                "favourite": {
                    "type": "boolean",
                    "default": true,
                    "description": "False takes the mark away.",
                },
            }),
            Self::CreatePlaylist => json!({
                "name": { "type": "string", "description": "The new playlist's name." },
                "track_ids": ids("Catalog track ids it starts with, in the order given."),
                "query": {
                    "type": "string",
                    "description": "A search whose matches it starts with, in album order.",
                },
                "fills_from": {
                    "type": "string",
                    "description": "A search it fills itself from whenever it is read.",
                },
                "limit": limit(QUEUED_BY_DEFAULT, "The most matches of query or fills_from."),
            }),
            Self::AddToPlaylist => json!({
                "playlist": { "type": "string", "description": "The playlist's name." },
                "track_ids": ids("Catalog track ids, appended in the order given."),
                "query": {
                    "type": "string",
                    "description": "A search whose matches are appended in album order.",
                },
                "limit": limit(QUEUED_BY_DEFAULT, "The most matches of query to append."),
            }),
            Self::RemoveFromPlaylist => json!({
                "playlist": { "type": "string", "description": "The playlist's name." },
                "row": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "The row, as playlist_tracks numbers it.",
                },
                "through_row": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "The last row of a run starting at row.",
                },
                "matching": {
                    "type": "string",
                    "description": "Every row this search matches.",
                },
            }),
            Self::RenamePlaylist => json!({
                "playlist": { "type": "string", "description": "The playlist's name." },
                "to": { "type": "string", "description": "Its new name." },
            }),
            Self::WantTracks => json!({
                "release_track_ids": ids("Release track ids, as list_missing gives them."),
            }),
            Self::Suggestions | Self::NowPlaying | Self::LibraryPasses => json!({}),
            Self::StartScan => json!({
                "roots": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Folders to add to the library and scan; none scans every \
                                    root the library already holds.",
                },
            }),
            Self::StartLookup => json!({
                "refresh": {
                    "type": "boolean",
                    "default": false,
                    "description": "Ask again about what was already answered.",
                },
            }),
            Self::StartPoll => json!({
                "again": {
                    "type": "boolean",
                    "default": false,
                    "description": "Ask about the wants asked about lately too.",
                },
            }),
            Self::StopPass => json!({
                "pass": {
                    "type": "string",
                    "enum": Pass::ALL.map(Pass::name),
                    "description": "Which pass to stop.",
                },
            }),
            Self::Control => json!({
                "action": {
                    "type": "string",
                    "enum": Action::ALL.map(Action::name),
                    "description": "What to do.",
                },
            }),
            Self::Seek => json!({
                "seconds": {
                    "type": "number",
                    "description": "How far to move; negative goes back.",
                },
            }),
            Self::SetVolume => json!({
                "percent": {
                    "type": "number",
                    "minimum": 0,
                    "maximum": WHOLE,
                    "description": "The volume, from 0 for silence to 100 for full.",
                },
            }),
            Self::Queue => json!({
                "limit": limit(QUEUED_BY_DEFAULT, "The most rows to answer with."),
            }),
            Self::AddToQueue => json!({
                "track_ids": {
                    "type": "array",
                    "items": { "type": "integer", "minimum": 1 },
                    "description": "Catalog track ids, queued in the order given.",
                },
                "query": {
                    "type": "string",
                    "description": "A search whose matches are queued in album order.",
                },
                "limit": limit(QUEUED_BY_DEFAULT, "The most matches of query to queue."),
                "next": {
                    "type": "boolean",
                    "default": false,
                    "description": "Queue them after the row being played rather than at the end.",
                },
                "play": {
                    "type": "boolean",
                    "default": false,
                    "description": "Start playing the first of them now.",
                },
            }),
            Self::RemoveFromQueue => json!({
                "queue_id": {
                    "type": "string",
                    "description": "The row's queue_id, exactly as show_queue gives it.",
                },
            }),
            Self::PlayPlaylist | Self::DiscardPlaylist => json!({
                "playlist": { "type": "string", "description": "The playlist's name." },
            }),
            Self::SleepTimer => json!({
                "minutes": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Pause after this many minutes.",
                },
                "at": {
                    "type": "string",
                    "enum": SleepAt::ALL.map(SleepAt::name),
                    "description": "Pause at the end of the track or the queue, or take the \
                                    timer away.",
                },
            }),
        };

        json!({
            "type": "object",
            "properties": properties,
            "required": self.requires(),
        })
    }

    const fn requires(self) -> &'static [&'static str] {
        match self {
            Self::Search => &["query"],
            Self::PlaylistTracks
            | Self::PlayPlaylist
            | Self::DiscardPlaylist
            | Self::AddToPlaylist
            | Self::RemoveFromPlaylist => &["playlist"],
            Self::CreatePlaylist => &["name"],
            Self::RenamePlaylist => &["playlist", "to"],
            Self::WantTracks => &["release_track_ids"],
            Self::Control => &["action"],
            Self::Seek => &["seconds"],
            Self::SetVolume => &["percent"],
            Self::RemoveFromQueue => &["queue_id"],
            Self::StopPass => &["pass"],
            Self::Playlists
            | Self::Favourites
            | Self::Statistics
            | Self::Suggestions
            | Self::Missing
            | Self::MarkFavourite
            | Self::NowPlaying
            | Self::Queue
            | Self::AddToQueue
            | Self::SleepTimer
            | Self::StartScan
            | Self::StartLookup
            | Self::StartPoll
            | Self::LibraryPasses => &[],
        }
    }

    pub(crate) fn listed(self) -> Value {
        json!({
            "name": self.name(),
            "description": self.describes(),
            "inputSchema": self.takes(),
            "annotations": {
                "readOnlyHint": self.reads_only(),
                "destructiveHint": self.destroys(),
            },
        })
    }

    pub(crate) fn run(
        self,
        arguments: Value,
        library: &Library,
        players: &dyn Reach,
        passes: &Passes,
    ) -> std::result::Result<Result<Value>, Refusal> {
        Ok(match self {
            Self::Search => {
                let asked: Searching = self.taken(arguments)?;
                catalog::search(
                    library,
                    &asked.query,
                    rows(asked.limit, SEARCHED_BY_DEFAULT),
                )
            }
            Self::Playlists => {
                let asked: Naming = self.taken(arguments)?;
                catalog::playlists(library, asked.named.as_deref())
            }
            Self::PlaylistTracks => {
                let asked: Listing = self.taken(arguments)?;
                catalog::playlist_tracks(
                    library,
                    &asked.playlist,
                    asked.matching.as_deref(),
                    rows(asked.limit, LISTED_BY_DEFAULT),
                )
            }
            Self::Favourites => {
                let asked: Limited = self.taken(arguments)?;
                catalog::favourites(library, rows(asked.limit, LISTED_BY_DEFAULT))
            }
            Self::Statistics => {
                let asked: Windowed = self.taken(arguments)?;
                catalog::statistics(
                    library,
                    asked.window.map_or(Window::Month, WindowArg::window),
                    rows(asked.top, TOP_BY_DEFAULT),
                )
            }
            Self::Suggestions => {
                let _: Nothing = self.taken(arguments)?;
                catalog::suggestions(library)
            }
            Self::Missing => {
                let asked: Narrowed = self.taken(arguments)?;
                catalog::missing(
                    library,
                    asked.query.as_deref(),
                    rows(asked.limit, MISSING_BY_DEFAULT),
                )
            }
            Self::MarkFavourite => {
                let marking = self.marking(arguments)?;
                edits::favour(library, &marking)
            }
            Self::CreatePlaylist => {
                let (name, filling) = self.filling(arguments)?;
                edits::create_playlist(library, &name, &filling)
            }
            Self::AddToPlaylist => {
                let asked: Appending = self.taken(arguments)?;
                let wanted = self.wanted(asked.track_ids, asked.query, asked.limit)?;
                edits::add_to_playlist(library, &asked.playlist, &wanted)
            }
            Self::RemoveFromPlaylist => {
                let (playlist, dropping) = self.dropping(arguments)?;
                edits::remove_from_playlist(library, &playlist, &dropping)
            }
            Self::RenamePlaylist => {
                let asked: Renaming = self.taken(arguments)?;
                edits::rename_playlist(library, &asked.playlist, &asked.to)
            }
            Self::DiscardPlaylist => {
                let asked: Choosing = self.taken(arguments)?;
                edits::discard_playlist(library, &asked.playlist)
            }
            Self::WantTracks => {
                let asked: WantingTracks = self.taken(arguments)?;
                let release_tracks: Vec<ReleaseTrackId> = asked
                    .release_track_ids
                    .into_iter()
                    .map(ReleaseTrackId::of)
                    .collect();
                edits::want(library, &release_tracks)
            }
            Self::NowPlaying => {
                let _: Nothing = self.taken(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::now_playing(&*player))
            }
            Self::Control => {
                let asked: Controlled = self.taken(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::control(&*player, asked.action))
            }
            Self::Seek => {
                let by = self.seeking(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::seek(&*player, by))
            }
            Self::SetVolume => {
                let volume = self.volume(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::set_volume(&*player, volume))
            }
            Self::Queue => {
                let asked: Limited = self.taken(arguments)?;
                players.player().and_then(|player| {
                    transport::queue(&*player, rows(asked.limit, QUEUED_BY_DEFAULT))
                })
            }
            Self::AddToQueue => {
                let adding = self.adding(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::add(&*player, library, &adding))
            }
            Self::RemoveFromQueue => {
                let row = self.queue_row(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::remove(&*player, row))
            }
            Self::PlayPlaylist => {
                let asked: Choosing = self.taken(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::play_playlist(&*player, &asked.playlist))
            }
            Self::SleepTimer => {
                let until = self.sleeping(arguments)?;
                players
                    .player()
                    .and_then(|player| transport::set_sleep(&*player, until))
            }
            Self::StartScan => {
                let asked: Scanning = self.taken(arguments)?;
                passes.scan(library, &asked.roots)
            }
            Self::StartLookup => {
                let asked: LookingUp = self.taken(arguments)?;
                passes.look_up(library, asked.refresh)
            }
            Self::StartPoll => {
                let asked: Polling = self.taken(arguments)?;
                passes.poll(library, asked.again)
            }
            Self::LibraryPasses => {
                let _: Nothing = self.taken(arguments)?;
                Ok(passes.states())
            }
            Self::StopPass => {
                let asked: Stopping = self.taken(arguments)?;
                Ok(passes.stop(asked.pass))
            }
        })
    }

    fn taken<Asked: DeserializeOwned>(
        self,
        arguments: Value,
    ) -> std::result::Result<Asked, Refusal> {
        serde_json::from_value(arguments)
            .map_err(|source| Refusal::BadArguments { tool: self, source })
    }

    fn queue_row(self, arguments: Value) -> std::result::Result<TrackId, Refusal> {
        let asked: Removing = self.taken(arguments)?;

        asked
            .queue_id
            .trim()
            .parse()
            .map(TrackId::of)
            .map_err(|_| Refusal::Unreadable {
                tool: self,
                field: "queue_id",
            })
    }

    fn seeking(self, arguments: Value) -> std::result::Result<Seeking, Refusal> {
        let asked: Seeked = self.taken(arguments)?;
        let out_of_range = || Refusal::OutOfRange {
            tool: self,
            field: "seconds",
        };
        let span = Duration::try_from_secs_f64(asked.seconds.abs()).map_err(|_| out_of_range())?;

        Ok(if asked.seconds.is_sign_negative() {
            Seeking::Backward(span)
        } else {
            Seeking::Forward(span)
        })
    }

    fn volume(self, arguments: Value) -> std::result::Result<Volume, Refusal> {
        let asked: Loudness = self.taken(arguments)?;
        let out_of_range = || Refusal::OutOfRange {
            tool: self,
            field: "percent",
        };
        if !(0.0..=WHOLE).contains(&asked.percent) {
            return Err(out_of_range());
        }
        Volume::new((asked.percent / WHOLE) as f32).map_err(|_| out_of_range())
    }

    fn adding(self, arguments: Value) -> std::result::Result<Adding, Refusal> {
        let asked: Queueing = self.taken(arguments)?;

        Ok(Adding {
            wanted: self.wanted(asked.track_ids, asked.query, asked.limit)?,
            next: asked.next,
            play: asked.play,
        })
    }

    fn wanted(
        self,
        track_ids: Option<Vec<NonZeroU64>>,
        query: Option<String>,
        limit: Option<usize>,
    ) -> std::result::Result<Wanted, Refusal> {
        match (track_ids, query) {
            (Some(ids), None) => Ok(Wanted::Tracks(ids.into_iter().map(TrackId::of).collect())),
            (None, Some(query)) => Ok(Wanted::Matching {
                query,
                most: rows(limit, QUEUED_BY_DEFAULT),
            }),
            (Some(_), Some(_)) | (None, None) => Err(Refusal::OneOf {
                tool: self,
                fields: &["track_ids", "query"],
            }),
        }
    }

    fn marking(self, arguments: Value) -> std::result::Result<Marking, Refusal> {
        let asked: Marked = self.taken(arguments)?;
        let favoured: Vec<Favoured> = asked
            .track_ids
            .into_iter()
            .map(|id| Favoured::Track(TrackId::of(id)))
            .chain(
                asked
                    .album_ids
                    .into_iter()
                    .map(|id| Favoured::Album(AlbumId::of(id))),
            )
            .chain(
                asked
                    .artist_ids
                    .into_iter()
                    .map(|id| Favoured::Artist(ArtistId::of(id))),
            )
            .collect();
        if favoured.is_empty() {
            return Err(Refusal::AtLeastOneOf {
                tool: self,
                fields: &["track_ids", "album_ids", "artist_ids"],
            });
        }

        Ok(Marking {
            favoured,
            favourite: asked.favourite,
        })
    }

    fn filling(self, arguments: Value) -> std::result::Result<(String, Filling), Refusal> {
        let asked: Creating = self.taken(arguments)?;
        let filling = match (asked.track_ids, asked.query, asked.fills_from) {
            (None, None, None) => Filling::Empty,
            (Some(ids), None, None) => {
                Filling::Rows(Wanted::Tracks(ids.into_iter().map(TrackId::of).collect()))
            }
            (None, Some(query), None) => Filling::Rows(Wanted::Matching {
                query,
                most: rows(asked.limit, QUEUED_BY_DEFAULT),
            }),
            (None, None, Some(query)) => Filling::Search {
                query,
                most: asked.limit.map(|most| most.clamp(1, MOST_ROWS)),
            },
            _ => {
                return Err(Refusal::AtMostOneOf {
                    tool: self,
                    fields: &["track_ids", "query", "fills_from"],
                });
            }
        };

        Ok((asked.name, filling))
    }

    fn dropping(self, arguments: Value) -> std::result::Result<(String, Dropping), Refusal> {
        let asked: Dropped = self.taken(arguments)?;
        let dropping = match (asked.row, asked.through_row, asked.matching) {
            (Some(row), through, None) => {
                Dropping::Rows(Span::between(row, through.unwrap_or(row)))
            }
            (None, None, Some(matching)) => Dropping::Matching(matching),
            _ => {
                return Err(Refusal::OneOf {
                    tool: self,
                    fields: &["row", "matching"],
                });
            }
        };

        Ok((asked.playlist, dropping))
    }

    fn sleeping(self, arguments: Value) -> std::result::Result<Option<Until>, Refusal> {
        let asked: Sleeping = self.taken(arguments)?;

        match (asked.minutes, asked.at) {
            (Some(minutes), None) => Ok(Some(Until::After(Duration::from_secs(
                minutes.get().saturating_mul(SECONDS_A_MINUTE),
            )))),
            (None, Some(at)) => Ok(at.until()),
            (Some(_), Some(_)) | (None, None) => Err(Refusal::OneOf {
                tool: self,
                fields: &["minutes", "at"],
            }),
        }
    }
}

impl fmt::Display for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

fn limit(default: usize, description: &str) -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": MOST_ROWS,
        "default": default,
        "description": description,
    })
}

fn ids(description: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "integer", "minimum": 1 },
        "description": description,
    })
}

fn rows(asked: Option<usize>, default: usize) -> usize {
    asked.unwrap_or(default).clamp(1, MOST_ROWS)
}

#[derive(Deserialize)]
struct Nothing {}

#[derive(Deserialize)]
struct Searching {
    query: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct Naming {
    named: Option<String>,
}

#[derive(Deserialize)]
struct Choosing {
    playlist: String,
}

#[derive(Deserialize)]
struct Listing {
    playlist: String,
    matching: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct Limited {
    limit: Option<usize>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WindowArg {
    Week,
    Month,
    Year,
    Everything,
}

impl WindowArg {
    pub(crate) const fn window(self) -> Window {
        match self {
            Self::Week => Window::Week,
            Self::Month => Window::Month,
            Self::Year => Window::Year,
            Self::Everything => Window::Everything,
        }
    }
}

#[derive(Deserialize)]
struct Windowed {
    window: Option<WindowArg>,
    top: Option<usize>,
}

#[derive(Deserialize)]
struct Controlled {
    action: Action,
}

#[derive(Deserialize)]
struct Seeked {
    seconds: f64,
}

#[derive(Deserialize)]
struct Loudness {
    percent: f64,
}

#[derive(Deserialize)]
struct Queueing {
    track_ids: Option<Vec<NonZeroU64>>,
    query: Option<String>,
    limit: Option<usize>,
    #[serde(default)]
    next: bool,
    #[serde(default)]
    play: bool,
}

#[derive(Deserialize)]
struct Narrowed {
    query: Option<String>,
    limit: Option<usize>,
}

const fn favoured_by_default() -> bool {
    true
}

#[derive(Deserialize)]
struct Marked {
    #[serde(default)]
    track_ids: Vec<NonZeroU64>,
    #[serde(default)]
    album_ids: Vec<NonZeroU64>,
    #[serde(default)]
    artist_ids: Vec<NonZeroU64>,
    #[serde(default = "favoured_by_default")]
    favourite: bool,
}

#[derive(Deserialize)]
struct Creating {
    name: String,
    track_ids: Option<Vec<NonZeroU64>>,
    query: Option<String>,
    fills_from: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct Appending {
    playlist: String,
    track_ids: Option<Vec<NonZeroU64>>,
    query: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct Dropped {
    playlist: String,
    row: Option<usize>,
    through_row: Option<usize>,
    matching: Option<String>,
}

#[derive(Deserialize)]
struct Renaming {
    playlist: String,
    to: String,
}

#[derive(Deserialize)]
struct WantingTracks {
    release_track_ids: Vec<NonZeroU64>,
}

#[derive(Deserialize)]
struct Removing {
    queue_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SleepAt {
    EndOfTrack,
    EndOfQueue,
    Off,
}

impl SleepAt {
    const ALL: [Self; 3] = [Self::EndOfTrack, Self::EndOfQueue, Self::Off];

    const fn name(self) -> &'static str {
        match self {
            Self::EndOfTrack => "end_of_track",
            Self::EndOfQueue => "end_of_queue",
            Self::Off => "off",
        }
    }

    const fn until(self) -> Option<Until> {
        match self {
            Self::EndOfTrack => Some(Until::EndOfTrack),
            Self::EndOfQueue => Some(Until::EndOfQueue),
            Self::Off => None,
        }
    }
}

#[derive(Deserialize)]
struct Sleeping {
    minutes: Option<NonZeroU64>,
    at: Option<SleepAt>,
}

#[derive(Deserialize)]
struct Scanning {
    #[serde(default)]
    roots: Vec<PathBuf>,
}

#[derive(Deserialize)]
struct LookingUp {
    #[serde(default)]
    refresh: bool,
}

#[derive(Deserialize)]
struct Polling {
    #[serde(default)]
    again: bool,
}

#[derive(Deserialize)]
struct Stopping {
    pass: Pass,
}
