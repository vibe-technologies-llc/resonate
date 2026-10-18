use resonate_library::{
    Album, AlbumOrder, AlbumQuery, Artist, ArtistOrder, ArtistQuery, Favoured, Library, SortOrder,
    Track, TrackQuery,
};

use crate::{
    Error, Result, ago,
    cli::FavouritesArgs,
    error::{ArtistName, Given, Named},
    folded_name,
    info::clock,
    table::Table,
};

const NO_ARTIST: &str = "-";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Tracks,
    Albums,
    Artists,
}

struct Chosen {
    what: Favoured,
    shown: String,
}

pub fn run(library: &Library, wanted: &FavouritesArgs) -> Result<()> {
    if wanted.add.is_empty() && wanted.remove.is_empty() {
        return print(library, wanted);
    }
    mark(library, wanted)
}

fn mark(library: &Library, wanted: &FavouritesArgs) -> Result<()> {
    let kind = the_one_kind(wanted)?;
    let favourite = !wanted.add.is_empty();
    let names = if favourite {
        &wanted.add
    } else {
        &wanted.remove
    };

    for name in names {
        let chosen = chosen_by(library, kind, name, wanted.by.as_deref())?;
        let changed = library.favour(chosen.what, favourite)?;
        said!("{}", told_of(favourite, changed, &chosen.shown));
    }
    Ok(())
}

fn told_of(favourite: bool, changed: bool, shown: &str) -> String {
    match (favourite, changed) {
        (true, true) => format!("favourited {shown}"),
        (true, false) => format!("{shown} is a favourite already"),
        (false, true) => format!("took {shown} off the favourites"),
        (false, false) => format!("{shown} is no favourite"),
    }
}

fn the_one_kind(wanted: &FavouritesArgs) -> Result<Kind> {
    match (wanted.tracks, wanted.albums, wanted.artists) {
        (true, false, false) => Ok(Kind::Tracks),
        (false, true, false) => Ok(Kind::Albums),
        (false, false, true) => Ok(Kind::Artists),
        _ => Err(Error::MarkNeedsOneKind),
    }
}

fn chosen_by(library: &Library, kind: Kind, name: &str, by: Option<&str>) -> Result<Chosen> {
    match kind {
        Kind::Tracks => the_only(tracks_named(library, name, by)?, Named::Track, name),
        Kind::Albums => the_only(albums_named(library, name, by)?, Named::Album, name),
        Kind::Artists => {
            let id = library
                .artist_named(name)?
                .ok_or_else(|| Error::NoSuchArtist(ArtistName::new(name)))?;
            Ok(Chosen {
                what: Favoured::Artist(id),
                shown: name.trim().to_owned(),
            })
        }
    }
}

fn the_only(mut found: Vec<Chosen>, kind: Named, name: &str) -> Result<Chosen> {
    match found.len() {
        0 => Err(Error::NothingNamed {
            kind,
            given: Given::new(name),
        }),
        1 => Ok(found.remove(0)),
        held => Err(Error::SeveralNamed {
            given: Given::new(name),
            held,
        }),
    }
}

fn is_by(artist: Option<&str>, by: Option<&str>) -> bool {
    by.is_none_or(|by| artist.is_some_and(|artist| folded_name(artist) == folded_name(by)))
}

fn tracks_named(library: &Library, name: &str, by: Option<&str>) -> Result<Vec<Chosen>> {
    let asked = TrackQuery {
        text: Some(name.to_owned()),
        ..TrackQuery::default()
    };

    Ok(library
        .tracks(&asked)?
        .into_iter()
        .filter(|track| folded_name(&track.title) == folded_name(name))
        .filter(|track| is_by(track.artist.as_deref(), by))
        .map(|track| Chosen {
            what: Favoured::Track(track.id),
            shown: shown_with(&track.title, track.artist.as_deref()),
        })
        .collect())
}

fn albums_named(library: &Library, name: &str, by: Option<&str>) -> Result<Vec<Chosen>> {
    let asked = AlbumQuery {
        text: Some(name.to_owned()),
        ..AlbumQuery::default()
    };

    Ok(library
        .albums(&asked)?
        .into_iter()
        .filter(|album| folded_name(&album.title) == folded_name(name))
        .filter(|album| is_by(album.artist.as_deref(), by))
        .map(|album| Chosen {
            what: Favoured::Album(album.id),
            shown: shown_with(&album.title, album.artist.as_deref()),
        })
        .collect())
}

fn shown_with(title: &str, artist: Option<&str>) -> String {
    match artist {
        Some(artist) => format!("{title} by {artist}"),
        None => title.to_owned(),
    }
}

fn print(library: &Library, wanted: &FavouritesArgs) -> Result<()> {
    let everything = !wanted.tracks && !wanted.albums && !wanted.artists;
    let mut drawn = false;

    if wanted.tracks || everything {
        draw(
            &mut drawn,
            "TRACKS",
            tracks(&library.favourite_tracks(&asked())?),
        );
    }
    if wanted.albums || everything {
        draw(
            &mut drawn,
            "ALBUMS",
            albums(&library.favourite_albums(&asked_of_albums())?),
        );
    }
    if wanted.artists || everything {
        draw(
            &mut drawn,
            "ARTISTS",
            artists(&library.favourite_artists(&asked_of_artists())?),
        );
    }
    Ok(())
}

fn draw(drawn: &mut bool, heading: &'static str, table: Option<Table>) {
    if *drawn {
        said!();
    }
    *drawn = true;
    said!("{heading}");

    match table {
        Some(table) => said_on!("{}", table.render()),
        None => said!("nothing here is a favourite yet"),
    }
}

fn asked() -> TrackQuery {
    let sort = SortOrder::Favourited;

    TrackQuery {
        sort,
        reading: sort.reads(),
        ..TrackQuery::default()
    }
}

fn asked_of_albums() -> AlbumQuery {
    let sort = AlbumOrder::Favourited;

    AlbumQuery {
        sort,
        reading: sort.reads(),
        ..AlbumQuery::default()
    }
}

fn asked_of_artists() -> ArtistQuery {
    let sort = ArtistOrder::Favourited;

    ArtistQuery {
        sort,
        reading: sort.reads(),
        ..ArtistQuery::default()
    }
}

fn tracks(held: &[Track]) -> Option<Table> {
    if held.is_empty() {
        return None;
    }
    let mut table = Table::new(vec!["TITLE", "ARTIST", "LENGTH", "PLAYS", "FAVOURITED"]);
    for track in held {
        table.push(vec![
            track.title.clone(),
            track.artist.clone().unwrap_or_else(|| NO_ARTIST.to_owned()),
            track
                .duration
                .map_or_else(String::new, |frames| clock(frames, track.spec.rate)),
            track.plays.to_string(),
            ago(track.favourite),
        ]);
    }
    Some(table)
}

fn albums(held: &[Album]) -> Option<Table> {
    if held.is_empty() {
        return None;
    }
    let mut table = Table::new(vec!["ALBUM", "ARTIST", "YEAR", "TRACKS", "FAVOURITED"]);
    for album in held {
        table.push(vec![
            album.title.clone(),
            album.artist.clone().unwrap_or_else(|| NO_ARTIST.to_owned()),
            album.year.map_or_else(String::new, |year| year.to_string()),
            album.track_count.to_string(),
            ago(album.favourite),
        ]);
    }
    Some(table)
}

fn artists(held: &[Artist]) -> Option<Table> {
    if held.is_empty() {
        return None;
    }
    let mut table = Table::new(vec!["ARTIST", "ALBUMS", "TRACKS", "FAVOURITED"]);
    for artist in held {
        table.push(vec![
            artist.name.clone(),
            artist.album_count.to_string(),
            artist.track_count.to_string(),
            ago(artist.favourite),
        ]);
    }
    Some(table)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use clap::Parser as _;

    use super::*;
    use crate::{
        cli::{Cli, Sub},
        fixtures::{ALBUM, ARTIST, TITLE, scanned, scratch, tagged_wave},
    };

    fn asked(arguments: &[&str]) -> FavouritesArgs {
        let cli = Cli::try_parse_from(
            ["resonate", "favourites"]
                .into_iter()
                .chain(arguments.iter().copied()),
        )
        .expect("a command");
        let Some(Sub::Favourites(args)) = cli.command else {
            unreachable!("the command named favourites");
        };
        args
    }

    fn library_of_two_songs_sharing_a_title() -> (PathBuf, Library) {
        let root = scratch("favourites");
        for (file, artist, album) in [("a.wav", "Ada", "First"), ("b.wav", "Ben", "Second")] {
            fs::write(
                root.join(file),
                tagged_wave(&[(TITLE, "Intro"), (ARTIST, artist), (ALBUM, album)]),
            )
            .expect("a song");
        }
        let library = scanned(&root);
        (root, library)
    }

    fn favourite_titles(library: &Library) -> Vec<String> {
        library
            .favourite_tracks(&asked_for_all())
            .expect("the favourites")
            .into_iter()
            .map(|track| shown_with(&track.title, track.artist.as_deref()))
            .collect()
    }

    fn asked_for_all() -> TrackQuery {
        TrackQuery::default()
    }

    #[test]
    fn a_track_named_on_the_command_line_is_marked_and_taken_off_again() {
        let (root, library) = library_of_two_songs_sharing_a_title();

        run(
            &library,
            &asked(&["--tracks", "--add", "intro", "--by", "ben"]),
        )
        .expect("a mark");
        let marked = favourite_titles(&library);
        run(
            &library,
            &asked(&["--tracks", "--remove", "Intro", "--by", "Ben"]),
        )
        .expect("a removal");
        let removed = favourite_titles(&library);
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(marked, ["Intro by Ben"]);
        assert!(removed.is_empty());
    }

    #[test]
    fn a_title_two_songs_share_is_refused_until_an_artist_narrows_it() {
        let (root, library) = library_of_two_songs_sharing_a_title();

        let several = run(&library, &asked(&["--tracks", "--add", "Intro"]));
        let nobody = run(
            &library,
            &asked(&["--tracks", "--add", "Intro", "--by", "Cleo"]),
        );
        let held = favourite_titles(&library);
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert!(matches!(several, Err(Error::SeveralNamed { held: 2, .. })));
        assert!(matches!(
            nobody,
            Err(Error::NothingNamed {
                kind: Named::Track,
                ..
            })
        ));
        assert!(held.is_empty(), "a refused name marked a song");
    }

    #[test]
    fn an_album_and_an_artist_are_marked_by_name_and_a_mark_without_a_kind_is_refused() {
        let (root, library) = library_of_two_songs_sharing_a_title();

        run(&library, &asked(&["--albums", "--add", "Second"])).expect("an album");
        run(&library, &asked(&["--artists", "--add", "Ada"])).expect("an artist");
        let without_a_kind = run(&library, &asked(&["--add", "Second"]));
        let a_stranger = run(&library, &asked(&["--artists", "--add", "Cleo"]));
        let albums = library
            .favourite_albums(&AlbumQuery::default())
            .expect("the albums");
        let artists = library
            .favourite_artists(&ArtistQuery::default())
            .expect("the artists");
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].title, "Second");
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].name, "Ada");
        assert!(matches!(without_a_kind, Err(Error::MarkNeedsOneKind)));
        assert!(matches!(a_stranger, Err(Error::NoSuchArtist(_))));
    }
}
