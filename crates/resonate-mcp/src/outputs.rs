use serde_json::{Map, Value, json};

use crate::tools::Tool;

fn text() -> Value {
    json!({ "type": "string" })
}

fn whole() -> Value {
    json!({ "type": "integer" })
}

fn number() -> Value {
    json!({ "type": "number" })
}

fn flag() -> Value {
    json!({ "type": "boolean" })
}

fn list(item: Value) -> Value {
    json!({ "type": "array", "items": item })
}

fn object(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties
            .into_iter()
            .map(|(name, schema)| (name.to_owned(), schema))
            .collect::<Map<String, Value>>(),
        "required": required,
    })
}

fn track_properties() -> Vec<(&'static str, Value)> {
    vec![
        ("track_id", whole()),
        ("title", text()),
        ("artist", text()),
        ("album", text()),
        ("disc", whole()),
        ("number", whole()),
        ("length_seconds", number()),
        ("genre", text()),
        ("plays", whole()),
        ("last_played", text()),
        ("favourite", flag()),
        ("uri", text()),
    ]
}

fn track() -> Value {
    object(track_properties(), &["track_id", "title", "favourite"])
}

fn playlist_row() -> Value {
    let mut properties = track_properties();
    properties.push(("row", whole()));
    object(properties, &["row"])
}

fn album() -> Value {
    object(
        vec![
            ("album_id", whole()),
            ("title", text()),
            ("artist", text()),
            ("year", whole()),
            ("tracks", whole()),
            ("missing", whole()),
            ("favourite", flag()),
        ],
        &["album_id", "title", "favourite"],
    )
}

fn artist() -> Value {
    object(
        vec![
            ("artist_id", whole()),
            ("name", text()),
            ("albums", whole()),
            ("tracks", whole()),
            ("favourite", flag()),
        ],
        &["artist_id", "name", "favourite"],
    )
}

fn playlist() -> Value {
    object(
        vec![
            ("name", text()),
            ("tracks", whole()),
            ("length_seconds", number()),
            ("plays", whole()),
            ("last_played", text()),
            ("fills_from", text()),
            ("pinned", flag()),
        ],
        &["name", "tracks", "pinned"],
    )
}

fn listened() -> Value {
    object(
        vec![
            ("name", text()),
            ("plays", whole()),
            ("listened_seconds", number()),
        ],
        &["name", "plays", "listened_seconds"],
    )
}

fn suggestion() -> Value {
    object(
        vec![
            ("name", text()),
            ("why", text()),
            ("tracks", whole()),
            ("length_seconds", number()),
            ("search", text()),
        ],
        &["name", "why", "tracks"],
    )
}

fn missing_track() -> Value {
    object(
        vec![
            ("release_track_id", whole()),
            ("title", text()),
            ("artist", text()),
            ("album", text()),
            ("album_artist", text()),
            ("disc", whole()),
            ("number", text()),
            ("length_seconds", number()),
            ("wanted", flag()),
        ],
        &["release_track_id", "title", "number", "wanted"],
    )
}

fn row_properties() -> Vec<(&'static str, Value)> {
    vec![
        ("queue_id", text()),
        ("title", text()),
        ("artist", text()),
        ("album", text()),
        ("length_seconds", number()),
        ("uri", text()),
    ]
}

fn row() -> Value {
    object(row_properties(), &["queue_id"])
}

fn queue_row() -> Value {
    let mut properties = row_properties();
    properties.push(("playing", flag()));
    object(properties, &["queue_id"])
}

fn asleep() -> Value {
    object(
        vec![
            (
                "until",
                json!({
                    "type": "string",
                    "enum": ["after_a_while", "end_of_track", "end_of_queue"],
                }),
            ),
            ("left_seconds", number()),
        ],
        &["until"],
    )
}

fn counts() -> Value {
    json!({ "type": "object", "additionalProperties": whole() })
}

fn pass() -> Value {
    object(
        vec![
            (
                "state",
                json!({ "type": "string", "enum": ["idle", "running", "finished", "failed"] }),
            ),
            ("progress", counts()),
            ("stats", counts()),
            ("cancelled", flag()),
            ("stopped_asking_for", text()),
            ("error", text()),
        ],
        &["state"],
    )
}

impl Tool {
    pub(crate) fn gives(self) -> Value {
        match self {
            Self::Search => object(
                vec![
                    ("tracks", list(track())),
                    ("albums", list(album())),
                    ("artists", list(artist())),
                    ("did_you_mean", text()),
                ],
                &["tracks", "albums", "artists"],
            ),
            Self::Playlists => object(
                vec![("matched", whole()), ("playlists", list(playlist()))],
                &["matched", "playlists"],
            ),
            Self::PlaylistTracks => object(
                vec![
                    ("playlist", playlist()),
                    ("matched", whole()),
                    ("tracks", list(playlist_row())),
                ],
                &["playlist", "matched", "tracks"],
            ),
            Self::Favourites => object(
                vec![
                    ("tracks", list(track())),
                    ("albums", list(album())),
                    ("artists", list(artist())),
                ],
                &["tracks", "albums", "artists"],
            ),
            Self::Statistics => object(
                vec![
                    ("window", text()),
                    ("since", text()),
                    ("plays", whole()),
                    ("listened_seconds", number()),
                    ("tracks_heard", whole()),
                    ("albums_heard", whole()),
                    ("artists_heard", whole()),
                    ("tracks", list(listened())),
                    ("albums", list(listened())),
                    ("artists", list(listened())),
                ],
                &[
                    "window",
                    "plays",
                    "listened_seconds",
                    "tracks_heard",
                    "albums_heard",
                    "artists_heard",
                    "tracks",
                    "albums",
                    "artists",
                ],
            ),
            Self::Suggestions => {
                object(vec![("suggestions", list(suggestion()))], &["suggestions"])
            }
            Self::Missing => object(
                vec![("missing", whole()), ("tracks", list(missing_track()))],
                &["missing", "tracks"],
            ),
            Self::MarkFavourite => object(
                vec![
                    ("favourite", flag()),
                    ("named", whole()),
                    ("changed", whole()),
                ],
                &["favourite", "named", "changed"],
            ),
            Self::CreatePlaylist | Self::RenamePlaylist => {
                object(vec![("playlist", playlist())], &["playlist"])
            }
            Self::AddToPlaylist => object(
                vec![("playlist", playlist()), ("added", whole())],
                &["playlist", "added"],
            ),
            Self::RemoveFromPlaylist => object(
                vec![("playlist", playlist()), ("removed", whole())],
                &["playlist", "removed"],
            ),
            Self::DiscardPlaylist => object(vec![("discarded", playlist())], &["discarded"]),
            Self::UndoEdit => object(
                vec![
                    ("walked", json!({ "type": ["string", "null"] })),
                    ("edit", text()),
                    ("playlist", text()),
                    ("more_to_undo", whole()),
                ],
                &["walked"],
            ),
            Self::WantTracks => object(vec![("wanted", whole())], &["wanted"]),
            Self::NowPlaying => object(
                vec![
                    ("player", text()),
                    ("status", text()),
                    ("track", row()),
                    ("position_seconds", number()),
                    ("volume_percent", number()),
                    ("sleep_timer", asleep()),
                ],
                &["player", "volume_percent"],
            ),
            Self::Control => object(
                vec![
                    ("player", text()),
                    ("done", text()),
                    ("status", text()),
                    ("track", row()),
                ],
                &["player", "done"],
            ),
            Self::Seek => object(
                vec![
                    ("player", text()),
                    ("moved_seconds", number()),
                    ("track", row()),
                    ("position_seconds", number()),
                ],
                &["player"],
            ),
            Self::SetVolume => object(
                vec![("player", text()), ("volume_percent", number())],
                &["player", "volume_percent"],
            ),
            Self::Queue => object(
                vec![
                    ("player", text()),
                    ("rows", whole()),
                    ("playing_row", whole()),
                    ("shuffle", flag()),
                    ("queue", list(queue_row())),
                ],
                &["player", "rows", "shuffle", "queue"],
            ),
            Self::AddToQueue => object(
                vec![("player", text()), ("queued", whole()), ("rows", whole())],
                &["player", "queued", "rows"],
            ),
            Self::RemoveFromQueue => object(
                vec![("player", text()), ("removed", text()), ("rows", whole())],
                &["player", "removed", "rows"],
            ),
            Self::PlayPlaylist => object(
                vec![("player", text()), ("playing", text())],
                &["player", "playing"],
            ),
            Self::SleepTimer => object(
                vec![("player", text()), ("sleep_timer", asleep())],
                &["player"],
            ),
            Self::StartScan => object(
                vec![
                    ("started", text()),
                    ("roots", list(text())),
                    ("full", flag()),
                    ("follow_links", flag()),
                    ("lookup_after", flag()),
                ],
                &["started", "roots", "full", "follow_links", "lookup_after"],
            ),
            Self::ForgetFolder => object(
                vec![("forgotten", text()), ("roots", list(text()))],
                &["forgotten", "roots"],
            ),
            Self::StartLookup => object(
                vec![
                    ("started", text()),
                    ("refresh", flag()),
                    ("carrying_on", flag()),
                ],
                &["started", "refresh", "carrying_on"],
            ),
            Self::StartPoll => object(
                vec![("started", text()), ("a_provider_is_registered", flag())],
                &["started", "a_provider_is_registered"],
            ),
            Self::LibraryPasses => object(
                vec![("scan", pass()), ("lookup", pass()), ("poll", pass())],
                &["scan", "lookup", "poll"],
            ),
            Self::StopPass => pass(),
        }
    }
}
