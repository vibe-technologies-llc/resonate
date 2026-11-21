use std::collections::BTreeSet;

use resonate_core::TrackId;
use resonate_library::{Library, PlaylistName, Replaceable, Replacing};
use serde_json::{Value, json};

use crate::{
    Error, Result, catalog,
    written::{self, AlbumTitles},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Standing {
    Waiting,
    Held,
    Wanted,
    Offered,
    Unmatched,
}

impl Standing {
    fn of(song: &Replaceable) -> Self {
        match (
            song.pending,
            song.held.is_some(),
            song.release_track.is_some(),
            song.offer().is_some(),
        ) {
            (true, _, _, _) => Self::Waiting,
            (false, true, _, _) => Self::Held,
            (false, false, true, _) => Self::Wanted,
            (false, false, false, true) => Self::Offered,
            (false, false, false, false) => Self::Unmatched,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Held => "held",
            Self::Wanted => "wanted",
            Self::Offered => "offered",
            Self::Unmatched => "unmatched",
        }
    }
}

pub(crate) fn lossy_songs(library: &Library, name: &str, most: usize) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let songs = library.songs_to_better_in(found.id)?;
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
                "better_copy".to_owned(),
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
        if let Some((by, _)) = song.offer() {
            fields.insert("offered_by".to_owned(), Value::from(by.as_str()));
        }
    }
    Ok(written)
}

fn only_the_chosen(
    playlist: &str,
    songs: Vec<Replaceable>,
    chosen: Option<&[TrackId]>,
) -> Result<Vec<Replaceable>> {
    let Some(chosen) = chosen else {
        return Ok(songs);
    };
    let held: BTreeSet<TrackId> = songs.iter().map(|song| song.track.id).collect();

    if let Some(track) = chosen.iter().find(|track| !held.contains(track)) {
        return Err(Error::NotALossySong {
            playlist: PlaylistName::new(playlist),
            track: *track,
        });
    }

    let wanted: BTreeSet<TrackId> = chosen.iter().copied().collect();
    Ok(songs
        .into_iter()
        .filter(|song| wanted.contains(&song.track.id))
        .collect())
}

pub(crate) fn replace_lossy_songs(
    library: &Library,
    name: &str,
    chosen: Option<&[TrackId]>,
) -> Result<Value> {
    let found = catalog::playlist_called(library, name)?;
    let songs = only_the_chosen(&found.name, library.songs_to_better_in(found.id)?, chosen)?;
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
        "played_beside": replacing.beside,
        "wanted": replacing.wanted,
        "downloading": replacing.kept,
        "need_a_music_folder": replacing.unfiled,
        "already_waiting": waiting,
        "left": left as u64 + replacing.unmatched,
    }))
}
