use std::{collections::BTreeSet, fs, sync::Arc};

use resonate_library::{
    AlbumOrder, AlbumQuery, ArtistMatch, ArtistOrder, ArtistProfile, ArtistQuery, Direction,
    Library, LookupOp, Result, SortOrder, TrackQuery,
};

use crate::support::{
    ADA, ALBUM, ALBUM_ARTIST, ARTIST, COMPILATION, Called, Canned, Fake, MUSICBRAINZ_ALBUM,
    ORBITERS, TITLE, TRACK, Tree, Wav, YEAR, artist_match, artist_named, enrich, keys, mbid,
    only_album, options, orbiters, scan,
};

const ARTIST_SORT: &[u8; 4] = b"TSOP";

const ALBUM_ARTIST_SORT: &[u8; 4] = b"TSO2";

const ALBUM_SORT: &[u8; 4] = b"TSOA";

const TITLE_SORT: &[u8; 4] = b"TSOT";

#[test]
fn tracks_group_into_one_album_under_their_album_artist() -> Result<()> {
    let tree = Tree::new();

    for (index, title) in ["Lunar", "Solar", "Stellar"].into_iter().enumerate() {
        tree.write(
            &format!("disc/{index}.wav"),
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, &format!("Guest {index}"))
                .text(ALBUM_ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(YEAR, "1998-03-02")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1);

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.title, "Orbits");
    assert_eq!(album.track_count, 3);
    assert_eq!(album.year, Some(1998));
    assert!(album.artist_id.is_some());

    let results = library.search("Orbiters", 10)?;

    assert_eq!(results.artists.len(), 1);

    let artist = results.artists.first().expect("the album artist is stored");

    assert_eq!(artist.name, "The Orbiters");
    assert_eq!(artist.track_count, 3);
    assert_eq!(artist.album_count, 1);

    let by_album = library.tracks(&TrackQuery {
        album: Some(album.id),
        ..TrackQuery::default()
    })?;

    assert_eq!(by_album.len(), 3);

    let listed = library.artists(&ArtistQuery::default())?;

    assert_eq!(listed.len(), 1);
    assert_eq!(listed.first().map(|entry| entry.id), Some(artist.id));
    Ok(())
}

#[test]
fn an_artist_is_listed_under_the_name_its_files_sort_it_by() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "beatles.wav",
        &Wav::new()
            .text(TITLE, "Help!")
            .text(ARTIST, "The Beatles")
            .text(ARTIST_SORT, "Beatles, The")
            .text(ALBUM, "Help!")
            .build(),
    );
    tree.write(
        "cure.wav",
        &Wav::new()
            .text(TITLE, "Lovesong")
            .text(ARTIST, "The Cure")
            .text(ALBUM_ARTIST, "The Cure")
            .text(ALBUM_ARTIST_SORT, "Cure, The")
            .text(ALBUM, "Disintegration")
            .build(),
    );
    for (file, artist) in [
        ("abba.wav", "ABBA"),
        ("blur.wav", "Blur"),
        ("doves.wav", "Doves"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, "Song")
                .text(ARTIST, artist)
                .text(ALBUM, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let named = |reading| -> Result<Vec<String>> {
        Ok(library
            .artists(&ArtistQuery {
                sort: ArtistOrder::Name,
                reading,
                ..ArtistQuery::default()
            })?
            .into_iter()
            .map(|artist| artist.name)
            .collect())
    };

    assert_eq!(
        named(Direction::Ascending)?,
        ["ABBA", "The Beatles", "Blur", "The Cure", "Doves"]
    );
    assert_eq!(
        named(Direction::Descending)?,
        ["Doves", "The Cure", "Blur", "The Beatles", "ABBA"]
    );

    let by_owner: Vec<String> = library
        .albums(&AlbumQuery {
            sort: AlbumOrder::Artist,
            reading: Direction::Ascending,
            ..AlbumQuery::default()
        })?
        .into_iter()
        .map(|album| album.title)
        .collect();

    assert_eq!(
        by_owner,
        ["ABBA", "Help!", "Blur", "Disintegration", "Doves"]
    );
    Ok(())
}

#[test]
fn an_album_is_listed_under_the_title_its_files_sort_it_by() -> Result<()> {
    let tree = Tree::new();

    for (file, title, sort) in [
        ("black.wav", "The Black Album", Some("Black Album, The")),
        (
            "black-again.wav",
            "The Black Album",
            Some("Black Album, The"),
        ),
        ("abbey.wav", "Abbey Road", None),
        ("revolver.wav", "Revolver", None),
    ] {
        let wav = Wav::new()
            .text(TITLE, "Song")
            .text(ARTIST, "The Beatles")
            .text(ALBUM, title);
        let wav = match sort {
            Some(sort) => wav.text(ALBUM_SORT, sort),
            None => wav,
        };

        tree.write(file, &wav.build());
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let titled = |reading| -> Result<Vec<String>> {
        Ok(library
            .albums(&AlbumQuery {
                sort: AlbumOrder::Title,
                reading,
                ..AlbumQuery::default()
            })?
            .into_iter()
            .map(|album| album.title)
            .collect())
    };

    assert_eq!(
        titled(Direction::Ascending)?,
        ["Abbey Road", "The Black Album", "Revolver"]
    );
    assert_eq!(
        titled(Direction::Descending)?,
        ["Revolver", "The Black Album", "Abbey Road"]
    );
    Ok(())
}

#[test]
fn a_track_is_listed_under_the_title_and_artist_its_file_sorts_it_by() -> Result<()> {
    let tree = Tree::new();

    for (file, title, title_sort, artist, artist_sort) in [
        ("end.wav", "The End", Some("End, The"), "Doves", None),
        ("abba.wav", "Angel Eyes", None, "ABBA", None),
        (
            "beatles.wav",
            "Help!",
            None,
            "The Beatles",
            Some("Beatles, The"),
        ),
        ("fade.wav", "Fade", None, "Blur", None),
        ("yesterday.wav", "Yesterday", None, "The Beatles", None),
    ] {
        let wav = Wav::new().text(TITLE, title).text(ARTIST, artist);
        let wav = match title_sort {
            Some(sort) => wav.text(TITLE_SORT, sort),
            None => wav,
        };
        let wav = match artist_sort {
            Some(sort) => wav.text(ARTIST_SORT, sort),
            None => wav,
        };

        tree.write(file, &wav.build());
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let listed = |sort, reading| -> Result<Vec<String>> {
        Ok(library
            .tracks(&TrackQuery {
                sort,
                reading,
                ..TrackQuery::default()
            })?
            .into_iter()
            .map(|track| track.title)
            .collect())
    };

    assert_eq!(
        listed(SortOrder::Title, Direction::Ascending)?,
        ["Angel Eyes", "The End", "Fade", "Help!", "Yesterday"]
    );
    assert_eq!(
        listed(SortOrder::Title, Direction::Descending)?,
        ["Yesterday", "Help!", "Fade", "The End", "Angel Eyes"]
    );

    let by_artist = listed(SortOrder::Artist, Direction::Ascending)?;

    assert_eq!(&by_artist[..1], ["Angel Eyes"]);
    assert_eq!(
        by_artist[1..3].iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([&"Help!".to_owned(), &"Yesterday".to_owned()]),
        "a Beatles track its file does not sort was not filed under the artist's sort name"
    );
    assert_eq!(&by_artist[3..], ["Fade", "The End"]);

    let backwards = listed(SortOrder::Artist, Direction::Descending)?;

    assert_eq!(&backwards[..2], ["The End", "Fade"]);
    assert_eq!(&backwards[4..], ["Angel Eyes"]);
    Ok(())
}

#[test]
fn a_compilation_stays_one_album_while_its_performers_stay_their_own() -> Result<()> {
    let tree = Tree::new();

    for (index, (title, artist)) in [("Dawn", "Ada"), ("Noon", "Ben"), ("Dusk", "Cleo")]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("various/{index}.wav"),
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "a compilation split into {albums:?}");

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.title, "Hours");
    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "a compilation belongs to no single artist"
    );

    let artists = library.artists(&ArtistQuery::default())?;
    let mut names: Vec<&str> = artists.iter().map(|artist| artist.name.as_str()).collect();

    names.sort_unstable();

    assert_eq!(names, vec!["Ada", "Ben", "Cleo"]);

    for artist in &artists {
        assert_eq!(artist.track_count, 1, "{} lost its track", artist.name);
    }
    Ok(())
}

#[test]
fn a_release_id_holds_together_a_compilation_that_carries_no_flag() -> Result<()> {
    let performers = [("Dawn", "Ada"), ("Noon", "Ben"), ("Dusk", "Cleo")];
    let release = "8f4f0e21-1b0f-4a3f-9a6c-0d3f2a5c7e11";

    let tagged = Tree::new();
    let untagged = Tree::new();

    for (index, (title, artist)) in performers.into_iter().enumerate() {
        let track = Wav::new()
            .text(TITLE, title)
            .text(ARTIST, artist)
            .text(ALBUM, "Hours");

        untagged.write(&format!("{index}.wav"), &track.build());
        tagged.write(
            &format!("{index}.wav"),
            &track.described(MUSICBRAINZ_ALBUM, release).build(),
        );
    }

    let loose = Library::open_in_memory()?;

    scan(&loose, &options(&untagged))?;

    assert_eq!(
        loose.albums(&AlbumQuery::default())?.len(),
        3,
        "a compilation with nothing to group it stayed together anyway"
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tagged))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "a release id did not group {albums:?}");

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.title, "Hours");
    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "a release its performers disagree about belongs to no single artist"
    );

    let artists = library.artists(&ArtistQuery::default())?;
    let mut names: Vec<&str> = artists.iter().map(|artist| artist.name.as_str()).collect();

    names.sort_unstable();

    assert_eq!(names, vec!["Ada", "Ben", "Cleo"]);

    for artist in &artists {
        assert_eq!(artist.track_count, 1, "{} lost its track", artist.name);
    }
    Ok(())
}

#[test]
fn a_release_every_track_agrees_about_keeps_its_artist() -> Result<()> {
    let tree = Tree::new();
    let release = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";

    for index in 0..3 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(YEAR, "1998-03-02")
                .described(MUSICBRAINZ_ALBUM, release)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1);

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.track_count, 3);
    assert_eq!(album.year, Some(1998));
    assert!(
        album.artist_id.is_some(),
        "an album nobody disagreed about lost its artist"
    );
    Ok(())
}

#[test]
fn two_releases_that_share_a_title_and_an_artist_are_told_apart_by_their_ids() -> Result<()> {
    let tree = Tree::new();

    for (index, release) in [
        "aaaaaaaa-0000-0000-0000-000000000001",
        "bbbbbbbb-0000-0000-0000-000000000002",
    ]
    .into_iter()
    .enumerate()
    {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "Live")
                .described(MUSICBRAINZ_ALBUM, release)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(
        albums.len(),
        2,
        "two releases sharing a title were merged into {albums:?}"
    );

    for album in &albums {
        assert_eq!(album.track_count, 1);
        assert!(album.artist_id.is_some());
    }
    Ok(())
}

#[test]
fn an_album_artist_outranks_the_compilation_flag() -> Result<()> {
    let tree = Tree::new();

    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, "The Curators")
                .text(ALBUM, "Selected")
                .text(COMPILATION, "1")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1);
    assert_eq!(albums.first().map(|album| album.track_count), Some(2));
    Ok(())
}

#[test]
fn two_albums_loose_in_a_root_sharing_a_title_stay_apart_under_their_own_artists() -> Result<()> {
    let tree = Tree::new();

    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Greatest Hits")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}

fn a_compilation_loose_in_a_root(tree: &Tree) {
    for (index, artist) in ["Ada", "Ben", "Cleo"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Now That's What I Call Music 42")
                .text(TRACK, &format!("{}/3", index + 1))
                .text(YEAR, "1999")
                .build(),
        );
    }
}

#[test]
fn a_compilation_loose_in_a_root_is_one_album_whatever_its_artists() -> Result<()> {
    let tree = Tree::new();

    a_compilation_loose_in_a_root(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "a record three artists share kept one of them"
    );
    assert_eq!(album.year, Some(1999));
    Ok(())
}

#[test]
fn a_compilation_gathered_out_of_a_root_stays_one_album_when_a_file_is_read_again() -> Result<()> {
    let tree = Tree::new();

    a_compilation_loose_in_a_root(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let gathered = only_album(&library)?;

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "track 1, retitled")
            .text(ARTIST, "Ben")
            .text(ALBUM, "Now That's What I Call Music 42")
            .text(TRACK, "2/3")
            .text(YEAR, "1999")
            .build(),
    );
    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    assert_eq!(album.id, gathered.id);
    assert_eq!(album.track_count, 3);
    Ok(())
}

#[test]
fn a_compilation_whose_last_artist_arrives_later_is_gathered_by_the_scan_that_reads_it()
-> Result<()> {
    let tree = Tree::new();

    a_compilation_loose_in_a_root(&tree);

    let late = tree.path().join("2.wav");
    let held = fs::read(&late).expect("the third track was written");

    fs::remove_file(&late).expect("the third track is held back");

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let gathered = only_album(&library)?;

    tree.write("elsewhere.wav", &Wav::new().text(ALBUM, "Animals").build());

    fs::write(&late, held).expect("the third track arrives");
    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let compilation: Vec<_> = albums
        .iter()
        .filter(|album| album.title == "Now That's What I Call Music 42")
        .collect();

    assert_eq!(compilation.len(), 1, "the late artist's track stood apart");
    assert_eq!(compilation[0].id, gathered.id);
    assert_eq!(compilation[0].track_count, 3);
    Ok(())
}

#[test]
fn two_albums_loose_in_a_root_numbered_from_one_each_stay_apart() -> Result<()> {
    let tree = Tree::new();

    for (index, artist) in ["Ada", "Ada", "Ben", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Greatest Hits")
                .text(TRACK, &format!("{}", index % 2 + 1))
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}

#[test]
fn a_folder_whose_files_name_different_album_artists_is_still_one_album() -> Result<()> {
    let tree = Tree::new();

    for (index, owner) in ["P. T. Adamczyk", "Marcin Przybyłowicz", "Kerry Eurodyne"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("Cyberpunk 2077/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, owner)
                .text(ALBUM_ARTIST, owner)
                .text(ALBUM, "Cyberpunk 2077")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(
        albums.len(),
        1,
        "one sleeve became an album per album artist: {albums:?}"
    );

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.track_count, 3);
    assert_eq!(
        album.artist_id, None,
        "an album its tracks disagree about belongs to no one singer"
    );
    Ok(())
}

#[test]
fn a_track_that_moves_between_the_folders_of_one_set_stays_under_the_album_it_was_in() -> Result<()>
{
    let tree = Tree::new();

    for (folder, owner) in [("Part One", "Ada"), ("Part Two", "Ada")] {
        tree.write(
            &format!("{folder}/one.wav"),
            &Wav::new()
                .text(TITLE, folder)
                .text(ARTIST, owner)
                .text(ALBUM_ARTIST, owner)
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 1);
    assert_eq!(
        keys(&database).len(),
        3,
        "the album is not named by its owner and by both folders"
    );
    Ok(())
}

#[test]
fn tracks_sharing_a_folder_are_one_album_however_their_singers_differ() -> Result<()> {
    let tree = Tree::new();

    for (index, artist) in ["Ada", "Ben", "Ada"].into_iter().enumerate() {
        tree.write(
            &format!("Under One Roof/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Under One Roof")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "one album split into {albums:?}");

    let album = albums.first().expect("one album was grouped");

    assert_eq!(album.track_count, 3);
    assert_eq!(album.artist_count, 2);
    assert_eq!(
        album.artist_id, None,
        "an album its tracks disagree about belongs to no one singer"
    );
    Ok(())
}

#[test]
fn two_albums_sharing_a_title_in_folders_of_their_own_stay_apart() -> Result<()> {
    let tree = Tree::new();

    for (index, artist) in ["Ada", "Ben"].into_iter().enumerate() {
        tree.write(
            &format!("{artist}/Greatest Hits/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, artist)
                .text(ALBUM, "Greatest Hits")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.albums(&AlbumQuery::default())?.len(), 2);
    Ok(())
}

#[test]
fn the_discs_of_a_set_in_folders_of_their_own_are_still_one_album() -> Result<()> {
    let tree = Tree::new();

    for (index, disc) in ["CD1", "Disc 2", "disk-03"].into_iter().enumerate() {
        tree.write(
            &format!("The Wall/{disc}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "a disc set split into {albums:?}");
    assert_eq!(albums.first().map(|album| album.track_count), Some(3));
    Ok(())
}

#[test]
fn the_discs_of_a_set_numbered_in_words_gather_the_way_numbered_ones_do() -> Result<()> {
    let tree = Tree::new();

    for (index, disc) in ["Disc One", "Second Disc", "CD Three"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("The Wall/{disc}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "a disc set split into {albums:?}");
    assert_eq!(albums.first().map(|album| album.track_count), Some(3));
    Ok(())
}

#[test]
fn a_folder_whose_name_merely_starts_like_a_disc_is_an_album_of_its_own() -> Result<()> {
    let tree = Tree::new();

    for (index, folder) in ["Discovery", "Disconnected"].into_iter().enumerate() {
        tree.write(
            &format!("Ada/{folder}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, "Ada")
                .text(ALBUM, folder)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 2, "two albums gathered into {albums:?}");
    Ok(())
}

#[test]
fn an_album_artist_still_gathers_discs_the_folders_keep_apart() -> Result<()> {
    let tree = Tree::new();

    for (index, folder) in ["The Wall Part One", "The Wall Part Two"]
        .into_iter()
        .enumerate()
    {
        tree.write(
            &format!("{folder}/{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, &format!("guest {index}"))
                .text(ALBUM_ARTIST, "Ada")
                .text(ALBUM, "The Wall")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "an album artist did not gather {albums:?}");
    Ok(())
}

#[test]
fn an_album_names_its_artist_on_its_own_row() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Blue Light")
            .text(ALBUM, "Harbour")
            .text(ARTIST, "Ida Fenn")
            .text(ALBUM_ARTIST, "Ida Fenn")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("the scanned album");

    assert_eq!(album.title, "Harbour");
    assert_eq!(album.artist.as_deref(), Some("Ida Fenn"));
    assert!(album.artist_id.is_some(), "the album should name an artist");
    Ok(())
}

#[test]
fn an_album_names_its_artist_although_the_artists_listing_left_it_out() -> Result<()> {
    let tree = Tree::new();

    for (file, title, album, artist) in [
        ("a.wav", "Blue Light", "Harbour", "Ida Fenn"),
        ("b.wav", "Slack Water", "Estuary", "Omar Pike"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let listed = library.artists(&ArtistQuery {
        limit: Some(1),
        ..ArtistQuery::default()
    })?;

    assert_eq!(
        listed.len(),
        1,
        "the cap should hold the listing to one artist"
    );

    let left_out = listed[0].name.clone();

    let albums = library.albums(&AlbumQuery::default())?;

    for album in &albums {
        assert!(
            album.artist.is_some(),
            "every album should name its artist off its own row, {} did not",
            album.title
        );
    }

    assert!(
        albums
            .iter()
            .any(|album| album.artist.as_deref().is_some_and(|name| name != left_out)),
        "the album whose artist the capped listing left out should still name it"
    );
    Ok(())
}

#[test]
fn an_artists_detail_names_the_artist_it_is_about() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Blue Light")
            .text(ALBUM, "Harbour")
            .text(ARTIST, "Ida Fenn")
            .text(ALBUM_ARTIST, "Ida Fenn")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let artists = library.artists(&ArtistQuery::default())?;
    let id = artists.first().expect("the scanned artist").id;
    let detail = library.artist_detail(id)?.expect("the artist detail");

    assert_eq!(detail.name, "Ida Fenn");
    Ok(())
}

#[test]
fn albums_and_artists_are_ordered_by_index_rank_rather_than_by_name() -> Result<()> {
    let tree = Tree::new();

    for (file, title, album, artist) in [
        ("a.wav", "Filler", "Zephyr Sessions", "Wanda Zephyr"),
        ("b.wav", "Zephyr", "Zephyr", "Zephyr"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .text(ALBUM_ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery {
        text: Some("zephyr".to_owned()),
        ..AlbumQuery::default()
    })?;

    assert_eq!(
        albums.first().map(|album| album.title.as_str()),
        Some("Zephyr"),
        "the album matching in every field should rank first, got {:?}",
        albums.iter().map(|album| &album.title).collect::<Vec<_>>()
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("zephyr".to_owned()),
        ..ArtistQuery::default()
    })?;

    assert_eq!(
        artists.first().map(|artist| artist.name.as_str()),
        Some("Zephyr"),
        "got {:?}",
        artists
            .iter()
            .map(|artist| &artist.name)
            .collect::<Vec<_>>()
    );

    let searched = library.search("zephyr", 10)?;

    assert_eq!(
        searched.albums.first().map(|album| album.title.as_str()),
        Some("Zephyr")
    );
    Ok(())
}

#[test]
fn an_unmatched_album_query_still_lists_every_album_by_title() -> Result<()> {
    let tree = Tree::new();

    for (file, album) in [("a.wav", "Beta"), ("b.wav", "Alpha")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, album)
                .text(ALBUM, album)
                .text(ALBUM_ARTIST, album)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Beta"]
    );

    let artists = library.artists(&ArtistQuery::default())?;

    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Alpha", "Beta"]
    );
    Ok(())
}

fn scanned_credits(files: &[(&str, &str, &str)]) -> Result<(Tree, Library)> {
    let tree = Tree::new();

    for (file, title, artist) in files {
        tree.write(
            file,
            &Wav::new().text(TITLE, title).text(ARTIST, artist).build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

fn titles_by(library: &Library, artist: resonate_core::ArtistId) -> Result<Vec<String>> {
    let mut titles: Vec<String> = library
        .tracks(&TrackQuery {
            artist: Some(artist),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect();

    titles.sort();
    Ok(titles)
}

#[test]
fn a_collaboration_is_listed_under_each_artist_it_credits_and_not_as_one_of_its_own() -> Result<()>
{
    let (_tree, library) = scanned_credits(&[
        ("cold.wav", "Cold", "Ada"),
        ("heat.wav", "Heat", "The Orbiters"),
        ("both.wav", "Both", "Ada & The Orbiters"),
    ])?;

    let names: Vec<String> = library
        .artists(&ArtistQuery::default())?
        .into_iter()
        .map(|artist| artist.name)
        .collect();

    assert!(
        !names.iter().any(|name| name == "Ada & The Orbiters"),
        "the collaboration was kept as an artist of its own: {names:?}"
    );

    let ada = artist_named(&library, "Ada")?;
    let orbiters = artist_named(&library, "The Orbiters")?;

    assert_eq!(titles_by(&library, ada.id)?, vec!["Both", "Cold"]);
    assert_eq!(titles_by(&library, orbiters.id)?, vec!["Both", "Heat"]);
    assert_eq!(ada.track_count, 2);
    assert_eq!(orbiters.track_count, 2);

    let both = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .find(|track| track.title == "Both")
        .expect("the collaboration is scanned");

    assert_eq!(both.artist.as_deref(), Some("Ada & The Orbiters"));
    Ok(())
}

#[test]
fn artists_a_tag_lists_apart_are_each_an_artist_though_none_was_held_alone() -> Result<()> {
    let (_tree, library) = scanned_credits(&[
        ("ocean.wav", "Ocean Eyes", "Billie Eilish"),
        ("lovely.wav", "Lovely", "Billie Eilish; Khalid"),
        ("trio.wav", "Trio", "Ada; Bea & Cy; Dot"),
    ])?;

    let names: Vec<String> = library
        .artists(&ArtistQuery::default())?
        .into_iter()
        .map(|artist| artist.name)
        .collect();

    assert!(
        !names.iter().any(|name| name.contains("; ")),
        "a list of artists was kept as an artist of its own: {names:?}"
    );

    let billie = artist_named(&library, "Billie Eilish")?;
    let khalid = artist_named(&library, "Khalid")?;

    assert_eq!(
        titles_by(&library, billie.id)?,
        vec!["Lovely", "Ocean Eyes"]
    );
    assert_eq!(titles_by(&library, khalid.id)?, vec!["Lovely"]);

    for member in ["Ada", "Bea & Cy", "Dot"] {
        let artist = artist_named(&library, member)?;

        assert_eq!(titles_by(&library, artist.id)?, vec!["Trio"], "{member}");
    }
    Ok(())
}

#[test]
fn a_name_whose_halves_name_nobody_held_is_one_artist() -> Result<()> {
    let (_tree, library) = scanned_credits(&[("song.wav", "Song", "Simon & Garfunkel")])?;

    let duo = artist_named(&library, "Simon & Garfunkel")?;

    assert_eq!(titles_by(&library, duo.id)?, vec!["Song"]);
    Ok(())
}

#[test]
fn a_collaboration_the_reference_cannot_name_is_asked_about_one_member_at_a_time() -> Result<()> {
    let (_tree, library) = scanned_credits(&[("both.wav", "Both", "Ada & The Orbiters")])?;
    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![
            ArtistMatch {
                mbid: mbid(ADA),
                ..artist_match(100, "Ada")
            },
            artist_match(100, "The Orbiters"),
        ],
        artists: vec![
            orbiters(),
            ArtistProfile {
                mbid: mbid(ADA),
                name: "Ada".to_owned(),
                ..orbiters()
            },
        ],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let searched: Vec<Called> = fake
        .calls()
        .into_iter()
        .filter(|called| called.op() == LookupOp::FindArtist)
        .collect();

    assert_eq!(
        searched,
        vec![
            Called::FindArtist("Ada & The Orbiters".to_owned()),
            Called::FindArtist("Ada".to_owned()),
            Called::FindArtist("The Orbiters".to_owned()),
        ]
    );

    let ada = artist_named(&library, "Ada")?;
    let orbiters = artist_named(&library, "The Orbiters")?;

    assert_eq!(ada.mbid, Some(mbid(ADA)));
    assert_eq!(titles_by(&library, ada.id)?, vec!["Both"]);
    assert_eq!(titles_by(&library, orbiters.id)?, vec!["Both"]);
    assert!(
        fake.calls().contains(&Called::Artist(mbid(ORBITERS))),
        "a member billed in the pass was not asked about in it"
    );
    Ok(())
}
