use resonate_library::{Library, Replaceable, Replacing};
use serde_json::{Value, json};

use crate::{
    Result, catalog,
    written::{self, AlbumTitles},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    Waiting,
    Held,
    Wanted,
    Unmatched,
}

impl Standing {
    const fn of(song: &Replaceable) -> Self {
        match (
            song.pending,
            song.held.is_some(),
            song.release_track.is_some(),
        ) {
            (true, _, _) => Self::Waiting,
            (false, true, _) => Self::Held,
            (false, false, true) => Self::Wanted,
            (false, false, false) => Self::Unmatched,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Held => "held",
            Self::Wanted => "wanted",
            Self::Unmatched => "unmatched",
        }
    }
}

pub(crate) fn lossy_songs(library: &Library, name: &str, most: usize) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let songs = library.lossy_songs_of(found.id)?;
    let mut titles = AlbumTitles::over(library);
    let listed = songs
        .iter()
        .take(most)
        .map(|song| lossy_song(&mut titles, song))
        .collect::<Result<Vec<_>>>()?;

    Ok(json!({
        "playlist": written::playlist(&found),
        "matched": songs.len(),
        "replaceable": songs.iter().filter(|song| song.can_be_ticked()).count(),
        "songs": listed,
    }))
}

fn lossy_song(titles: &mut AlbumTitles<'_>, song: &Replaceable) -> Result<Value> {
    let mut written = titles.track(&song.track)?;
    if let Value::Object(fields) = &mut written {
        fields.insert("codec".to_owned(), Value::from(song.track.codec.as_str()));
        fields.insert(
            "standing".to_owned(),
            Value::from(Standing::of(song).name()),
        );
        if let Some(held) = &song.held {
            fields.insert(
                "lossless_copy".to_owned(),
                json!({
                    "track_id": held.id.get(),
                    "codec": held.codec.as_str(),
                    "uri": held.location.to_uri_within(held.span),
                }),
            );
        }
        if let Some(row) = song.release_track {
            fields.insert("release_track_id".to_owned(), Value::from(row.get()));
        }
    }
    Ok(written)
}

pub(crate) fn replace_lossy_songs(library: &Library, name: &str) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let songs = library.lossy_songs_of(found.id)?;
    let chosen: Vec<Replaceable> = songs
        .iter()
        .filter(|song| song.can_be_ticked())
        .cloned()
        .collect();
    let waiting = songs.iter().filter(|song| song.pending).count();
    let replacing = match chosen.is_empty() {
        true => Replacing::default(),
        false => library.replace(&chosen)?,
    };
    let left = songs.len() - chosen.len() - waiting;
    let held = catalog::playlist_called(library, &found.name)?;

    Ok(json!({
        "playlist": written::playlist(&held),
        "lossy": songs.len(),
        "replaced": replacing.folded,
        "trashed": replacing.trashed.files,
        "not_trashed": replacing.trashed.kept,
        "wanted": replacing.wanted,
        "already_waiting": waiting,
        "left": left as u64 + replacing.unmatched,
    }))
}
