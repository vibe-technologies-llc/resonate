use std::{sync::Arc, time::Duration};

use resonate_core::MediaLocation;
use resonate_library::{
    AlbumQuery, ArtistMatch, ArtistQuery, Codec, Library, Result, SavedQuery, SortOrder, TrackQuery,
};

use crate::support::{
    ALBUM, ARTIST, Canned, Fake, GENRE, ORBITERS, TITLE, TRACK, TRACKS_OF_A_DECADE, Tree, Wav,
    artist_named, asked, beside, enrich, matched, matching, mbid, narrowed, only_album, options,
    orbiters, orbits, orbits_file, orbits_rows, release_row, scan, scanned_orbits, scanned_shapes,
    stems, timed_words, titles, words,
};

#[test]
fn a_search_that_matches_nothing_is_answered_in_the_catalogs_own_spelling() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ALBUM, "Tideline")
            .text(ARTIST, "Example Band")
            .build(),
    );
    tree.write(
        "geralt.wav",
        &Wav::new()
            .text(TITLE, "Silver for Monsters")
            .text(ALBUM, "The Witcher 3")
            .text(ARTIST, "Marcin Przybyłowicz")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert!(
        library
            .tracks(&TrackQuery {
                text: Some("bind".to_owned()),
                ..TrackQuery::default()
            })?
            .is_empty(),
        "the misspelt word matched something, so there is nothing to suggest"
    );
    assert_eq!(
        library.did_you_mean("bind")?,
        Some("Band".to_owned()),
        "a misspelt word was not answered in the spelling the catalog holds"
    );
    assert_eq!(
        library.did_you_mean("przybylowitz")?,
        Some("Przybyłowicz".to_owned()),
        "the marked spelling is the one a pane would draw"
    );
    assert_eq!(
        library.did_you_mean("album:tidline")?,
        Some("album:Tideline".to_owned())
    );
    assert_eq!(
        library.did_you_mean("exampleband")?,
        Some("Example Band".to_owned()),
        "a run-together was not split into the two words the catalog holds"
    );
    assert_eq!(
        library.did_you_mean("\"silver fer monsters\"")?,
        Some("\"Silver for Monsters\"".to_owned()),
        "a phrase mistyped in a word too short to correct was not weighed against the whole name"
    );
    assert_eq!(
        library.did_you_mean("example ban d")?,
        Some("example Band".to_owned()),
        "a word typed in two halves was not run together"
    );
    assert_eq!(
        library.did_you_mean("lantern")?,
        None,
        "a word the catalog holds was corrected to something else"
    );
    assert_eq!(library.did_you_mean("")?, None);
    Ok(())
}

#[test]
fn a_text_search_is_ordered_by_relevance_rather_than_by_album() -> Result<()> {
    let tree = Tree::new();

    for (file, title, album, artist) in [
        ("a.wav", "Zenith", "Zenith", "Ada"),
        ("b.wav", "Alpha Zenith", "Nightfall", "Ben"),
        ("c.wav", "Beta", "Zenith Skies", "Cleo"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, album)
                .text(ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let ranked = library.tracks(&TrackQuery {
        text: Some("zenith".to_owned()),
        ..TrackQuery::default()
    })?;

    assert_eq!(
        titles(&ranked).first(),
        Some(&"Zenith"),
        "the track matching in both title and album should rank first, got {:?}",
        titles(&ranked)
    );

    let by_title = library.tracks(&TrackQuery {
        text: Some("zenith".to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })?;

    assert_eq!(titles(&by_title), vec!["Alpha Zenith", "Beta", "Zenith"]);
    Ok(())
}

#[test]
fn relevance_falls_back_to_album_order_when_there_is_no_text_to_rank() -> Result<()> {
    let tree = Tree::new();

    for (file, title, track) in [("a.wav", "second", "2"), ("b.wav", "first", "1")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ALBUM, "One Album")
                .text(TRACK, track)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let listed = library.tracks(&TrackQuery::default())?;

    assert_eq!(titles(&listed), vec!["first", "second"]);
    Ok(())
}

#[test]
fn search_matches_a_track_through_any_of_its_indexed_fields() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "one.wav",
        &Wav::new()
            .text(TITLE, "Halcyon Days")
            .text(ARTIST, "Elder")
            .text(ALBUM, "Reflections of a Floating World")
            .build(),
    );
    tree.write(
        "two.wav",
        &Wav::new()
            .text(TITLE, "Sanctuary")
            .text(ARTIST, "Yob")
            .text(ALBUM, "Clearing the Path")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    for query in [
        "Halcyon",
        "halcy",
        "HALCYON",
        "hALCYON",
        "Elder",
        "ELDER",
        "floating",
        "FLOATING WORLD",
        "title:HALCYON",
        "artist:ELDER",
    ] {
        let results = library.search(query, 10)?;

        assert_eq!(results.tracks.len(), 1, "{query} matched the wrong count");
        assert_eq!(
            results.tracks.first().map(|track| track.title.as_str()),
            Some("Halcyon Days"),
            "{query} matched the wrong track"
        );
    }

    assert_eq!(library.search("Sanctuary", 10)?.tracks.len(), 1);
    assert_eq!(library.search("nothing here", 10)?.tracks.len(), 0);
    assert_eq!(
        library.search("   ", 10)?.tracks.len(),
        2,
        "a search asking nothing should hold everything, as no search does"
    );
    Ok(())
}

#[test]
fn a_name_is_found_however_its_letters_are_marked_or_cased() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "one.wav",
        &Wav::new()
            .text(TITLE, "Kıskanç")
            .text(ARTIST, "Marcin Przybyłowicz")
            .text(ALBUM, "Straße")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    for query in [
        "Kıskanç",
        "KISKANÇ",
        "kiskanc",
        "KISKANC",
        "kıskanc",
        "title:KISKANC",
        "Przybylowicz",
        "Przybyłowicz",
        "artist:przybylowicz",
        "Strasse",
        "Straße",
    ] {
        let results = library.search(query, 10)?;

        assert_eq!(
            results.tracks.first().map(|track| track.title.as_str()),
            Some("Kıskanç"),
            "{query} did not reach the track"
        );
    }

    assert_eq!(library.search("kismet", 10)?.tracks.len(), 0);
    Ok(())
}

#[test]
fn search_text_that_would_break_the_index_query_is_treated_as_words() -> Result<()> {
    let tree = Tree::new();

    tree.write("one.wav", &Wav::new().text(TITLE, "Quotes and OR").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    for query in ["\"quotes\"", "quotes OR", "quotes*(", "AND OR NOT"] {
        assert!(
            library.search(query, 10).is_ok(),
            "{query} was passed through to the index"
        );
    }

    assert_eq!(library.search("\"quotes\"", 10)?.tracks.len(), 1);
    Ok(())
}

#[test]
fn a_count_answers_for_the_whole_match_where_a_page_answers_for_itself() -> Result<()> {
    const MP3: i64 = 7;

    let tree = Tree::new();

    for index in 0..5 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("track {index}"))
                .text(ARTIST, if index < 2 { "One" } else { "Other" })
                .text(ALBUM, &format!("album {}", index / 2))
                .build(),
        );
    }

    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    beside(&database)
        .execute(
            "UPDATE tracks SET codec = ?1 WHERE title IN ('track 0', 'track 1')",
            [MP3],
        )
        .expect("the catalog takes a codec");

    assert_eq!(
        library
            .tracks(&TrackQuery {
                sort: SortOrder::Title,
                reading: SortOrder::Title.reads(),
                ..TrackQuery::default()
            })?
            .iter()
            .filter(|track| track.codec == Codec::Mp3)
            .count(),
        2,
        "the code this test writes is not the one the catalog reads as mp3"
    );

    let paged = TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(2),
        ..TrackQuery::default()
    };

    assert_eq!(library.tracks(&paged)?.len(), 2);

    let whole = library.measured(&TrackQuery {
        limit: None,
        ..paged.clone()
    })?;

    assert_eq!(whole.rows, 5);
    assert_eq!(
        whole.lossless, 3,
        "the two rows written as mp3 were counted lossless"
    );
    assert!(whole.length.is_some());

    let capped = library.measured(&paged)?;

    assert_eq!(
        capped.rows, 2,
        "a measurement given a limit answers for the page it names"
    );

    let albums = AlbumQuery {
        limit: Some(1),
        ..AlbumQuery::default()
    };

    assert_eq!(library.albums(&albums)?.len(), 1);
    assert_eq!(library.albums_counted(&albums)?, 3);

    let artists = ArtistQuery {
        limit: Some(1),
        ..ArtistQuery::default()
    };

    assert_eq!(library.artists(&artists)?.len(), 1);
    assert_eq!(library.artists_counted(&artists)?, 2);

    let narrowed = ArtistQuery {
        text: Some("Other".to_owned()),
        ..ArtistQuery::default()
    };

    assert_eq!(
        library.artists_counted(&narrowed)?,
        1,
        "a count over a narrowed listing counts what the search matches"
    );
    Ok(())
}

#[test]
fn limit_and_offset_page_through_a_sorted_result() -> Result<()> {
    let tree = Tree::new();

    for index in 0..5 {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new().text(TITLE, &format!("track {index}")).build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let page = library.tracks(&TrackQuery {
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        limit: Some(2),
        offset: 2,
        ..TrackQuery::default()
    })?;

    assert_eq!(page.len(), 2);
    assert_eq!(
        page.iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["track 2", "track 3"]
    );
    Ok(())
}

#[test]
fn a_term_narrows_a_search_to_what_the_file_is() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "depth:24")?, vec!["Heat"]);
    assert_eq!(matched(&library, "depth:>16")?, vec!["Heat"]);
    assert_eq!(matched(&library, "depth:16")?, vec!["Cold"]);
    assert_eq!(matched(&library, "is:hires")?, vec!["Heat"]);
    assert_eq!(matched(&library, "is:lossless")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "is:lossy")?.is_empty());
    assert_eq!(matched(&library, "is:stereo")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "is:mono")?.is_empty());
    assert_eq!(matched(&library, "codec:pcm")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "codec:flac")?.is_empty());
    assert_eq!(matched(&library, "rate:44.1k")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "rate:>=96k")?.is_empty());
    Ok(())
}

#[test]
fn a_year_is_read_off_the_album_and_a_length_off_the_frames() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "year:1975")?, vec!["Cold"]);
    assert_eq!(matched(&library, "year:1970-1979")?, vec!["Cold"]);
    assert_eq!(matched(&library, "year:>=1990")?, vec!["Heat"]);
    assert_eq!(matched(&library, "length:>0.5s")?, vec!["Heat"]);
    assert_eq!(matched(&library, "length:<0.5s")?, vec!["Cold"]);
    assert!(matched(&library, "length:>1h")?.is_empty());
    Ok(())
}

#[test]
fn an_age_is_measured_back_from_now() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "added:<1d")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "added:1y")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "added:>1d")?.is_empty());
    Ok(())
}

#[test]
fn a_search_narrows_by_how_often_and_how_lately_a_track_was_played() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));

    library.track_played(&cold, None, Duration::ZERO)?;
    library.track_played(&cold, None, Duration::ZERO)?;

    assert_eq!(matched(&library, "plays:0")?, vec!["Heat"]);
    assert_eq!(matched(&library, "plays:>0")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:2")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1-3")?, vec!["Cold"]);
    assert!(matched(&library, "plays:>2")?.is_empty());
    assert_eq!(matched(&library, "played:<1d")?, vec!["Cold"]);
    assert!(matched(&library, "played:>1d")?.is_empty());
    Ok(())
}

#[test]
fn a_window_narrows_on_how_often_a_track_was_heard_inside_it() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let cold = MediaLocation::local(tree.path().join("cold.wav"));
    let heat = MediaLocation::local(tree.path().join("heat.wav"));

    for _ in 0..3 {
        library.track_played(&cold, None, Duration::ZERO)?;
    }
    library.track_played(&heat, None, Duration::ZERO)?;

    assert_eq!(matched(&library, "plays:>1@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:>0@30d")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "plays:3@30d")?, vec!["Cold"]);
    assert_eq!(matched(&library, "plays:1-3@30d")?, vec!["Cold", "Heat"]);
    assert!(matched(&library, "plays:>3@30d")?.is_empty());
    assert_eq!(matched(&library, "-plays:>1@30d")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn a_track_never_played_answers_a_denied_play_term() -> Result<()> {
    let (tree, library) = scanned_shapes();

    library.track_played(
        &MediaLocation::local(tree.path().join("cold.wav")),
        None,
        Duration::ZERO,
    )?;

    assert_eq!(matched(&library, "-played:<1d")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-plays:0")?, vec!["Cold"]);
    Ok(())
}

#[test]
fn a_track_nothing_has_played_falls_to_the_bottom_of_both_play_orders() -> Result<()> {
    let (tree, library) = scanned_shapes();

    library.track_played(
        &MediaLocation::local(tree.path().join("heat.wav")),
        None,
        Duration::ZERO,
    )?;

    let lately = library.tracks(&TrackQuery {
        sort: SortOrder::Played,
        reading: SortOrder::Played.reads(),
        ..TrackQuery::default()
    })?;

    assert_eq!(titles(&lately), vec!["Heat", "Cold"]);

    let most = library.tracks(&TrackQuery {
        sort: SortOrder::Plays,
        reading: SortOrder::Plays.reads(),
        ..TrackQuery::default()
    })?;

    assert_eq!(titles(&most), vec!["Heat", "Cold"]);
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_what_was_played_most() -> Result<()> {
    let (tree, library) = scanned_shapes();

    library.track_played(
        &MediaLocation::local(tree.path().join("heat.wav")),
        None,
        Duration::ZERO,
    )?;

    let id = library.save_query(
        "On repeat",
        &SavedQuery {
            text: Some("plays:>0".to_owned()),
            sort: SortOrder::Plays,
            reading: SortOrder::Plays.reads(),
            limit: Some(25),
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["heat"]);

    library.track_played(
        &MediaLocation::local(tree.path().join("cold.wav")),
        None,
        Duration::ZERO,
    )?;
    library.track_played(
        &MediaLocation::local(tree.path().join("cold.wav")),
        None,
        Duration::ZERO,
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["cold", "heat"]);

    let revised = library
        .playlist(id)?
        .expect("the saved query is still there");

    assert_eq!(
        revised.query.map(|query| query.sort),
        Some(SortOrder::Plays)
    );
    Ok(())
}

#[test]
fn terms_and_words_all_have_to_hold_at_once() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "heat is:hires")?, vec!["Heat"]);
    assert!(matched(&library, "cold is:hires")?.is_empty());
    assert_eq!(matched(&library, "is:lossless year:>=1990")?, vec!["Heat"]);
    assert!(matched(&library, "year:1975 year:>=1990")?.is_empty());
    Ok(())
}

#[test]
fn a_denied_token_takes_out_whatever_it_names() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "-is:hires")?, vec!["Cold"]);
    assert_eq!(matched(&library, "is:lossless -depth:24")?, vec!["Cold"]);
    assert_eq!(matched(&library, "-year:1975")?, vec!["Heat"]);
    assert!(matched(&library, "-codec:pcm")?.is_empty());
    assert_eq!(matched(&library, "-cold")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-artist:ada")?, vec!["Heat"]);
    Ok(())
}

#[test]
fn a_denied_range_leaves_what_falls_either_side_of_it() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "-year:1970-1979")?, vec!["Heat"]);
    assert_eq!(matched(&library, "-year:1970-1999")?.len(), 0);
    Ok(())
}

#[test]
fn either_of_two_holds_where_neither_alone_would() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(
        matched(&library, "depth:16 or depth:24")?,
        vec!["Cold", "Heat"]
    );
    assert_eq!(
        matched(&library, "year:1975 or year:1995")?,
        vec!["Cold", "Heat"]
    );
    assert_eq!(matched(&library, "is:hires or ada")?, vec!["Cold", "Heat"]);
    assert_eq!(matched(&library, "is:mono or is:hires")?, vec!["Heat"]);
    assert_eq!(
        matched(&library, "is:lossless year:1975 or is:hires")?,
        vec!["Cold", "Heat"],
        "a clause beside an alternation should still have to hold"
    );
    assert_eq!(
        matched(&library, "ada or ben -is:hires")?,
        vec!["Cold"],
        "a denial beside an alternation should still have to hold"
    );
    Ok(())
}

#[test]
fn a_denial_and_an_alternation_reach_the_album_and_artist_listings_too() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    let albums = library.albums(&AlbumQuery {
        text: Some("-is:hires".to_owned()),
        ..AlbumQuery::default()
    })?;

    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Winter"]
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("year:1975 or year:1995".to_owned()),
        ..ArtistQuery::default()
    })?;

    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Ada", "Ben"]
    );
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_what_it_denies() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let id = library.save_query(
        "Anything but hi-res",
        &SavedQuery {
            text: Some("-is:hires".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["cold"]);

    tree.write("more.wav", &Wav::new().text(TITLE, "Warmth").build());
    scan(&library, &options(&tree))?;

    assert_eq!(
        stems(&library.playlist_cuts(id)?),
        vec!["cold", "more"],
        "a file the scan added did not reach a saved query that names what it is not"
    );
    Ok(())
}

#[test]
fn a_column_holds_the_words_it_names_to_that_field() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    assert_eq!(matched(&library, "artist:ada")?, vec!["Cold"]);
    assert_eq!(matched(&library, "album:winter")?, vec!["Cold"]);
    assert_eq!(matched(&library, "title:cold")?, vec!["Cold"]);
    assert!(
        matched(&library, "title:ada")?.is_empty(),
        "an artist's name matched through the title column"
    );
    assert_eq!(matched(&library, "ada")?, vec!["Cold"]);
    Ok(())
}

#[test]
fn a_term_reaches_the_album_and_artist_listings_too() -> Result<()> {
    let (_tree, library) = scanned_shapes();

    let albums = library.albums(&AlbumQuery {
        text: Some("depth:24".to_owned()),
        ..AlbumQuery::default()
    })?;

    assert_eq!(
        albums
            .iter()
            .map(|album| album.title.as_str())
            .collect::<Vec<_>>(),
        vec!["Summer"]
    );

    let artists = library.artists(&ArtistQuery {
        text: Some("is:hires".to_owned()),
        ..ArtistQuery::default()
    })?;

    assert_eq!(
        artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Ben"]
    );

    let searched = library.search("year:1975", 10)?;

    assert_eq!(searched.tracks.len(), 1);
    assert_eq!(searched.albums.len(), 1);
    assert_eq!(searched.artists.len(), 1);
    Ok(())
}

#[test]
fn a_saved_query_fills_itself_from_its_terms() -> Result<()> {
    let (tree, library) = scanned_shapes();
    let id = library.save_query(
        "Hi-res",
        &SavedQuery {
            text: Some("is:hires".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert_eq!(stems(&library.playlist_cuts(id)?), vec!["heat"]);
    assert_eq!(library.playlist(id)?.map(|found| found.entries), Some(1));

    tree.write(
        "more.wav",
        &Wav::new().bits(24).text(TITLE, "Warmth").build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.playlist(id)?.map(|found| found.entries),
        Some(2),
        "a hi-res track the scan added did not reach the saved query"
    );
    Ok(())
}

#[test]
fn a_genre_tag_is_searchable() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "a.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(GENRE, "Progressive Rock")
            .build(),
    );
    tree.write("b.wav", &Wav::new().text(TITLE, "Dogs").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let slow_water = library
        .track_at(&tree.path().join("a.wav"), None)?
        .expect("the scan stored the file it walked");

    assert_eq!(slow_water.genre.as_deref(), Some("Progressive Rock"));

    assert_eq!(narrowed(&library, "genre:progressive")?, vec!["Lantern"]);
    assert_eq!(narrowed(&library, "genre:rock")?, vec!["Lantern"]);
    assert!(
        narrowed(&library, "genre:dogs")?.is_empty(),
        "a title answered a search scoped to the genre"
    );
    assert_eq!(
        narrowed(&library, "progressive")?,
        vec!["Lantern"],
        "an unscoped word did not reach the genre the index holds"
    );
    Ok(())
}

#[test]
fn a_whole_genre_is_asked_of_each_genre_a_track_lists_and_of_no_longer_name() -> Result<()> {
    let tree = Tree::new();

    for (name, genre) in [
        ("Strings", "House; Disco"),
        ("Brass", "Disco"),
        ("Twang", "Rockabilly"),
        ("Riff", "Rock"),
    ] {
        tree.write(
            &format!("{name}.wav"),
            &Wav::new().text(TITLE, name).text(GENRE, genre).build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(narrowed(&library, "genre:=House")?, vec!["Strings"]);
    assert_eq!(
        narrowed(&library, "genre:=Disco")?,
        vec!["Brass", "Strings"]
    );
    assert_eq!(narrowed(&library, "genre:=Rock")?, vec!["Riff"]);
    assert!(
        narrowed(&library, "genre:=\"House Disco\"")?.is_empty(),
        "two listed genres were read as the one name"
    );
    assert_eq!(
        narrowed(&library, "genre:Rock")?,
        vec!["Riff", "Twang"],
        "a bare genre word stopped reaching the names it begins"
    );
    Ok(())
}

#[test]
fn a_genre_list_is_offered_as_each_genre_in_it_and_as_the_whole_name_of_each() -> Result<()> {
    let tree = Tree::new();

    for number in 1..=TRACKS_OF_A_DECADE {
        let genre = if number % 2 == 0 {
            "House; Disco"
        } else {
            "Rockabilly"
        };

        tree.write(
            &format!("{number:02}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Track {number:02}"))
                .text(GENRE, genre)
                .build(),
        );
    }
    tree.write(
        "rock.wav",
        &Wav::new().text(TITLE, "Rock").text(GENRE, "Rock").build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let suggestions = library.suggestions()?;
    let texts = asked(&suggestions);

    assert!(texts.contains(&"genre:=\"House\""), "{texts:?}");
    assert!(texts.contains(&"genre:=\"Disco\""), "{texts:?}");
    assert!(
        texts.iter().all(|text| !text.contains(';')),
        "a list of genres was offered as one: {texts:?}"
    );

    let house = suggestions
        .iter()
        .find(|suggestion| suggestion.query.text.as_deref() == Some("genre:=\"House\""))
        .expect("House is offered");

    assert_eq!(house.rows, u64::from(TRACKS_OF_A_DECADE / 2));
    Ok(())
}

#[test]
fn a_genre_musicbrainz_gave_an_artist_reaches_its_tracks() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;

    assert!(narrowed(&library, "genre:psychedelic")?.is_empty());

    let fake = Arc::new(Fake::new(Canned {
        found_artists: vec![ArtistMatch {
            mbid: mbid(ORBITERS),
            name: "The Orbiters".to_owned(),
            score: 100,
            kind: Some("Group".to_owned()),
            disambiguation: None,
            aliases: Vec::new(),
        }],
        artists: vec![orbiters()],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    assert_eq!(
        library
            .artist_detail(artist_named(&library, "The Orbiters")?.id)?
            .map(|detail| detail.genres.len()),
        Some(2)
    );
    assert_eq!(
        narrowed(&library, "genre:psychedelic")?,
        vec!["A Quilt of Winds", "Paper Boats!", "Small Mercies"],
        "the genres the reference gave the artist did not reach the tracks it made"
    );
    Ok(())
}

#[test]
fn a_kana_voicing_mark_keeps_two_names_apart_and_an_old_index_is_folded_again() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "glass.wav",
        &Wav::new()
            .text(TITLE, "ガラス")
            .text(ARTIST, "バンド")
            .build(),
    );
    tree.write(
        "crow.wav",
        &Wav::new()
            .text(TITLE, "カラス")
            .text(ARTIST, "ハンド")
            .build(),
    );

    let database = tree.path().join("library.db");

    {
        let library = Library::open(&database)?;

        scan(&library, &options(&tree))?;

        assert_eq!(library.artists(&ArtistQuery::default())?.len(), 2);
        assert_eq!(matching(&library, "ガラス")?, ["ガラス"]);
    }

    let older = beside(&database);

    older
        .busy_timeout(Duration::from_secs(10))
        .expect("the catalog waits for a writer");
    older
        .execute_batch(
            "UPDATE tracks_fts SET title = 'カラス'
              WHERE rowid = (SELECT id FROM tracks WHERE title = 'ガラス');
             CREATE TABLE index_refold_wanted (since INTEGER) STRICT;",
        )
        .expect("the index is laid back as the older fold left it");

    let reopened = Library::open(&database)?;

    assert_eq!(
        matching(&reopened, "ガラス")?,
        ["ガラス"],
        "the index kept the fold that dropped the voicing mark"
    );
    assert_eq!(matching(&reopened, "カラス")?, ["カラス"]);
    Ok(())
}

#[test]
fn a_track_is_found_by_the_words_it_sings_and_only_when_they_are_asked_for() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    library.keep_lyrics(
        &orbits_file(&tree, "1.wav"),
        None,
        Some(&timed_words(
            "[00:01.00]Sunlight the fireflies\n[00:04.50]<00:04.50>sways in silence",
        )),
    )?;

    assert_eq!(matching(&library, "lyrics:fireflies")?, ["Small Mercies"]);
    assert_eq!(
        matching(&library, "lyric:\"fireflies sways\"")?,
        ["Small Mercies"]
    );
    assert!(matching(&library, "fireflies")?.is_empty());
    assert!(matching(&library, "lyrics:00")?.is_empty());
    assert_eq!(matching(&library, "quilt")?, ["A Quilt of Winds"]);

    scan(&library, &options(&tree))?;

    assert_eq!(matching(&library, "lyrics:fireflies")?, ["Small Mercies"]);
    Ok(())
}

#[test]
fn a_search_of_plain_words_is_offered_as_the_words_a_track_sings() -> Result<()> {
    let (tree, library) = scanned_orbits()?;

    library.keep_lyrics(
        &orbits_file(&tree, "2.wav"),
        None,
        Some(&words(
            "Sunlight the fireflies sways in silence over the bay",
        )),
    )?;

    let sung = library
        .sung("fireflies sways")?
        .expect("a track sings those words");

    assert_eq!(sung.query, "lyrics:\"fireflies sways\"");
    assert_eq!(sung.tracks, 1);
    assert_eq!(matching(&library, &sung.query)?, ["A Quilt of Winds"]);
    assert_eq!(library.sung("sways fireflies")?, None);
    assert_eq!(library.sung("year:1971 fireflies")?, None);
    assert_eq!(library.sung("title:fireflies")?, None);
    Ok(())
}

#[test]
fn a_search_reaches_the_rows_the_catalog_knows_it_is_short_of() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    assert_eq!(library.rematch(album.id)?, 3);

    let found = library.unheld_matching("lights", None)?;

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].title, "Harbour Lights");
    assert_eq!(found[0].album, album.id);
    assert_eq!(library.unheld_matching("orbits harbour", None)?.len(), 1);
    assert_eq!(library.unheld_matching("title:lights", None)?.len(), 1);
    assert!(library.unheld_matching("quilt", None)?.is_empty());
    assert!(library.unheld_matching("year:1998", None)?.is_empty());
    assert!(library.unheld_matching("lights nowhere", None)?.is_empty());
    Ok(())
}

#[test]
fn a_title_by_an_artist_is_read_as_that_title_by_the_artist_the_catalog_holds() -> Result<()> {
    let tree = Tree::new();

    for (file, title, artist) in [
        ("1.wav", "Hey A.B.", "Mira Vale"),
        ("2.wav", "Hey A.B.", "Somebody Else"),
        ("3.wav", "Small Mercies", "Mira Vale"),
    ] {
        tree.write(
            file,
            &Wav::new().text(TITLE, title).text(ARTIST, artist).build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let meant = library
        .meant("Hey A B by mira vale")?
        .expect("the words read as a title by a held artist");
    let found = library.tracks(&TrackQuery {
        text: Some(meant.searched.clone()),
        ..TrackQuery::default()
    })?;

    assert_eq!(meant.title, "Hey A B");
    assert_eq!(meant.artist, "Mira Vale");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].artist.as_deref(), Some("Mira Vale"));
    assert_eq!(found[0].title, "Hey A.B.");
    assert_eq!(
        library
            .meant("Mira Vale - Hey A.B.")?
            .map(|meant| meant.artist),
        Some("Mira Vale".to_owned())
    );
    assert_eq!(
        library
            .meant("hey a b by mirra vale")?
            .map(|meant| meant.artist),
        Some("Mira Vale".to_owned()),
        "a name a letter off is the artist the catalog holds"
    );
    assert_eq!(library.meant("hey a b by nobody held")?, None);
    assert_eq!(
        library.meant("silver rain by mira vale")?,
        None,
        "a reading that would find nothing is not taken"
    );
    assert_eq!(library.meant("mira vale")?, None);
    Ok(())
}
