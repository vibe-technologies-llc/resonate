use ahash::AHashSet;
use resonate_library::{Direction, Library, PlaylistOrder, Window};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    ArgumentName, PromptName, Refusal, ResourceUri, Result, prompts::Prompt, resources::Template,
};

const MOST_OFFERED: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Completable {
    Brief,
    NewPlaylist,
    Playlist,
    Window,
}

#[derive(Deserialize)]
pub(crate) struct Completing {
    #[serde(rename = "ref")]
    reference: Referred,
    argument: Argued,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Referred {
    #[serde(rename = "ref/prompt")]
    Prompt { name: String },
    #[serde(rename = "ref/resource")]
    Resource { uri: String },
}

#[derive(Deserialize)]
struct Argued {
    name: String,
    value: String,
}

impl Completing {
    pub(crate) fn complete(
        &self,
        library: &Library,
    ) -> std::result::Result<Result<Value>, Refusal> {
        let completable = self.completable()?;
        Ok(completable
            .offered(&self.argument.value, library)
            .map(completion))
    }

    fn completable(&self) -> std::result::Result<Completable, Refusal> {
        let argument = self.argument.name.as_str();
        let completes = match &self.reference {
            Referred::Prompt { name } => Prompt::named(name)
                .ok_or_else(|| Refusal::UnknownPrompt(PromptName::new(name.as_str())))?
                .completes(argument),
            Referred::Resource { uri } => Template::at(uri)
                .ok_or_else(|| Refusal::UnknownResource(ResourceUri::new(uri.as_str())))?
                .completes(argument),
        };
        completes.ok_or_else(|| Refusal::UnknownArgument(ArgumentName::new(argument)))
    }
}

impl Completable {
    fn offered(self, typed: &str, library: &Library) -> Result<Vec<String>> {
        Ok(match self {
            Self::Brief => library.names_completing(typed)?,
            Self::Window => matching(typed, Window::ALL.map(|window| window.name().to_owned())),
            Self::Playlist => matching(typed, held_playlists(library)?),
            Self::NewPlaylist => {
                let taken: AHashSet<String> = held_playlists(library)?
                    .iter()
                    .map(|name| name.to_lowercase())
                    .collect();
                let free = library
                    .suggestions()?
                    .iter()
                    .map(|suggestion| suggestion.name.clone())
                    .filter(|name| !taken.contains(&name.to_lowercase()))
                    .collect::<Vec<_>>();
                matching(typed, free)
            }
        })
    }
}

fn held_playlists(library: &Library) -> Result<Vec<String>> {
    Ok(library
        .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect())
}

fn matching(typed: &str, names: impl IntoIterator<Item = String>) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    let (beginning, holding): (Vec<_>, Vec<_>) = names
        .into_iter()
        .map(|name| (name.to_lowercase(), name))
        .filter(|(lowered, _)| lowered.contains(&typed))
        .partition(|(lowered, _)| lowered.starts_with(&typed));

    beginning
        .into_iter()
        .chain(holding)
        .map(|(_, name)| name)
        .collect()
}

fn completion(offered: Vec<String>) -> Value {
    let total = offered.len();
    let values: Vec<String> = offered.into_iter().take(MOST_OFFERED).collect();

    json!({
        "completion": {
            "values": values,
            "total": total,
            "hasMore": total > values.len(),
        },
    })
}
