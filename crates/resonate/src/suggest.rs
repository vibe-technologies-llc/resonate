use std::fmt;

use resonate_library::{Library, Suggestion};

use crate::{Error, Result, missing::folded_name, table::Table, words};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SuggestionName(Box<str>);

impl SuggestionName {
    pub fn new(name: &str) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for SuggestionName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub fn print(library: &Library, save: Option<&str>) -> Result<()> {
    let suggested = library.suggestions()?;

    match save {
        Some(name) => keep(library, &suggested, name),
        None => {
            list(&suggested);
            Ok(())
        }
    }
}

fn list(suggested: &[Suggestion]) {
    if suggested.is_empty() {
        said!("{}", words::suggest::too_little());
        return;
    }

    let mut table = Table::new(vec![
        words::suggest::name(),
        words::suggest::why(),
        words::suggest::tracks(),
        words::suggest::fills_from(),
    ]);
    for suggestion in suggested {
        table.push(vec![
            suggestion.name.clone(),
            suggestion.reason.says(),
            suggestion.rows.to_string(),
            fills_from(suggestion),
        ]);
    }

    said_on!("{}", table.render());
}

fn fills_from(suggestion: &Suggestion) -> String {
    suggestion
        .query
        .text
        .clone()
        .unwrap_or_else(|| words::suggest::the_whole_library().to_owned())
}

fn keep(library: &Library, suggested: &[Suggestion], name: &str) -> Result<()> {
    let wanted = folded_name(name);
    let found = suggested
        .iter()
        .find(|suggestion| folded_name(&suggestion.name) == wanted)
        .ok_or_else(|| Error::NoSuchSuggestion(SuggestionName::new(name)))?;

    let (id, revised) = match library.playlist_named(&found.name)? {
        Some(held) => {
            library.revise_query(held.id, &held.name, &found.query)?;
            (held.id, Some(held.name))
        }
        None => (library.save_query(&found.name, &found.query)?, None),
    };
    let held = library.playlist(id)?.map_or(0, |playlist| playlist.entries);

    said!(
        "{}",
        match revised {
            Some(name) => words::suggest::now_fills_itself(fills_from(found), held, name),
            None => words::suggest::fills_itself(fills_from(found), held, &found.name),
        }
    );
    Ok(())
}
