use std::fmt;

use resonate_library::{Library, Window};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::{
    Refusal, Result,
    completions::Completable,
    controlling::Reach,
    passes::Passes,
    resources::{Resource, over},
    tools::{Tool, WindowArg},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Prompt {
    BuildAPlaylist,
    ReviewMyListening,
    CompleteMyAlbums,
    AboutThisTrack,
}

struct Argument {
    name: &'static str,
    describes: &'static str,
    required: bool,
    completes: Completable,
}

const BRIEF: &str = "brief";

impl Prompt {
    pub const ALL: [Self; 4] = [
        Self::BuildAPlaylist,
        Self::ReviewMyListening,
        Self::CompleteMyAlbums,
        Self::AboutThisTrack,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::BuildAPlaylist => "build_a_playlist",
            Self::ReviewMyListening => "review_my_listening",
            Self::CompleteMyAlbums => "complete_my_albums",
            Self::AboutThisTrack => "about_this_track",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|prompt| prompt.name() == name)
    }

    const fn title(self) -> &'static str {
        match self {
            Self::BuildAPlaylist => "Build a playlist",
            Self::ReviewMyListening => "Review my listening",
            Self::CompleteMyAlbums => "Complete my albums",
            Self::AboutThisTrack => "About this track",
        }
    }

    const fn describes(self) -> &'static str {
        match self {
            Self::BuildAPlaylist => {
                "Choose tracks from the library that fit a brief and keep \
                                     them as a playlist."
            }
            Self::ReviewMyListening => {
                "Read what was listened to over a window and find what \
                                        the library holds that is not being played."
            }
            Self::CompleteMyAlbums => {
                "Weigh the tracks held albums are short of and want the \
                                       ones agreed to."
            }
            Self::AboutThisTrack => {
                "Talk about what the running player is playing and what else \
                                     of it the library holds."
            }
        }
    }

    const fn takes(self) -> &'static [Argument] {
        match self {
            Self::BuildAPlaylist => &[
                Argument {
                    name: BRIEF,
                    describes: "What the playlist is for: a mood, an occasion, a sound.",
                    required: true,
                    completes: Completable::Brief,
                },
                Argument {
                    name: "name",
                    describes: "What to call it; one is chosen where none is given.",
                    required: false,
                    completes: Completable::NewPlaylist,
                },
            ],
            Self::ReviewMyListening => &[Argument {
                name: "window",
                describes: "How far back to look: week, month, year or everything.",
                required: false,
                completes: Completable::Window,
            }],
            Self::CompleteMyAlbums | Self::AboutThisTrack => &[],
        }
    }

    pub(crate) fn completes(self, argument: &str) -> Option<Completable> {
        self.takes()
            .iter()
            .find(|taken| taken.name == argument)
            .map(|taken| taken.completes)
    }

    pub(crate) fn listed(self) -> Value {
        json!({
            "name": self.name(),
            "title": self.title(),
            "description": self.describes(),
            "arguments": self.takes().iter().map(|argument| json!({
                "name": argument.name,
                "description": argument.describes,
                "required": argument.required,
            })).collect::<Vec<_>>(),
        })
    }

    pub(crate) fn get(
        self,
        arguments: Value,
        library: &Library,
        players: &dyn Reach,
        passes: &Passes,
    ) -> std::result::Result<Result<Value>, Refusal> {
        let (asked, reading) = self.asked(arguments)?;
        Ok(reading.embedded(library, players, passes).map(|embedded| {
            json!({
                "description": self.describes(),
                "messages": [
                    { "role": "user", "content": { "type": "text", "text": asked } },
                    { "role": "user", "content": embedded },
                ],
            })
        }))
    }

    fn asked(self, arguments: Value) -> std::result::Result<(String, Resource), Refusal> {
        let search = Tool::Search.name();
        Ok(match self {
            Self::BuildAPlaylist => {
                let asked: Briefed = self.taken(arguments)?;
                let brief = self.given(BRIEF, &asked.brief)?;
                let named = match asked.name.as_deref().map(str::trim) {
                    Some(name) if !name.is_empty() => format!("under the name {name}"),
                    _ => "under a short name of your choosing that none of the playlists below \
                          already has"
                        .to_owned(),
                };
                (
                    format!(
                        "Build a playlist from my music library for this brief: {brief}. Search \
                         the catalog with {search} — its grammar narrows by artist, album, \
                         genre, year, length and plays — and choose the tracks that fit rather \
                         than every match. Then make it with {create} {named}, holding the \
                         track_ids you chose, and tell me what you put in it and why. The \
                         playlists I already have are below.",
                        create = Tool::CreatePlaylist.name(),
                    ),
                    Resource::Playlists,
                )
            }
            Self::ReviewMyListening => {
                let asked: Windowed = self.taken(arguments)?;
                let window = asked.window.map_or(Window::Month, WindowArg::window);
                (
                    format!(
                        "Below is what I listened to over {}. Tell me what it says about my \
                         listening — what I kept coming back to and how the time was spent — \
                         and suggest two or three searches in the library's grammar that find \
                         music I hold and have not been playing, trying each with {search} \
                         before you offer it.",
                        over(window),
                    ),
                    Resource::Statistics(window),
                )
            }
            Self::CompleteMyAlbums => {
                let _: Nothing = self.taken(arguments)?;
                (
                    format!(
                        "Below are tracks of albums in my library that the catalog knows of and \
                         holds no file for. Group them by album, say which albums are short of \
                         the most, and ask me which I want before calling {want} with their \
                         release_track_ids — never want a track I have not agreed to.",
                        want = Tool::WantTracks.name(),
                    ),
                    Resource::Missing,
                )
            }
            Self::AboutThisTrack => {
                let _: Nothing = self.taken(arguments)?;
                (
                    format!(
                        "Tell me about the track that is playing, below: the song, who made it \
                         and the album it is on, from what you know. Then look its album and \
                         its artist up in my library with {search} and say what else of theirs \
                         I hold."
                    ),
                    Resource::NowPlaying,
                )
            }
        })
    }

    fn taken<Asked: DeserializeOwned>(
        self,
        arguments: Value,
    ) -> std::result::Result<Asked, Refusal> {
        serde_json::from_value(arguments).map_err(|source| Refusal::BadPromptArguments {
            prompt: self,
            source,
        })
    }

    fn given<'a>(
        self,
        argument: &'static str,
        value: &'a str,
    ) -> std::result::Result<&'a str, Refusal> {
        let value = value.trim();
        if value.is_empty() {
            return Err(Refusal::BlankArgument {
                prompt: self,
                argument,
            });
        }
        Ok(value)
    }
}

impl fmt::Display for Prompt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Deserialize)]
struct Briefed {
    brief: String,
    name: Option<String>,
}

#[derive(Deserialize)]
struct Windowed {
    window: Option<WindowArg>,
}

#[derive(Deserialize)]
struct Nothing {}
