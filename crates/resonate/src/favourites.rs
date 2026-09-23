use resonate_library::{
    Album, AlbumOrder, AlbumQuery, Artist, ArtistOrder, ArtistQuery, Library, SortOrder, Track,
    TrackQuery,
};

use crate::{Result, ago, cli::FavouritesArgs, info::clock, table::Table};

const NO_ARTIST: &str = "-";

pub fn print(library: &Library, wanted: &FavouritesArgs) -> Result<()> {
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
        println!();
    }
    *drawn = true;
    println!("{heading}");

    match table {
        Some(table) => print!("{}", table.render()),
        None => println!("nothing here is a favourite yet"),
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
