use std::{
    io,
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{AlbumId, SourceId};
use resonate_library::{
    AlbumQuery, CoverArt, Favoured, ImageFormat, KeptSong, Library, Result, SavedQuery, Search,
    SortOrder, Sources, TrackQuery,
};
use resonate_providers::{Error as ProvidedError, ProviderOp, Providers, Result as ProvidedResult};

use crate::support::{
    ALBUM, ARTIST, GENRE, RATE, TITLE, TRACK, TRACKS_OF_A_DECADE, Tree, Wav, YEAR, a_music_folder,
    a_music_folder_on_disk, all, asked, delivering_into, held_as, only_album, options, png_art,
    scan, songs_under, source,
};

#[test]
fn a_suggestion_is_read_against_the_catalog_as_the_scan_left_it() -> Result<()> {
    let tree = Tree::new();

    tree.write("lantern.wav", &Wav::new().text(TITLE, "Lantern").build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(library.did_you_mean("metalica")?, None);

    tree.write(
        "ride.wav",
        &Wav::new()
            .text(TITLE, "Ride the Lightning")
            .text(ARTIST, "Metallica")
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.did_you_mean("metalica")?,
        Some("Metallica".to_owned()),
        "a suggestion was read against the catalog as it stood before the scan"
    );
    Ok(())
}

fn orbits_from_the_nineties(tree: &Tree) {
    for number in 1..=TRACKS_OF_A_DECADE {
        tree.write(
            &format!("orbits/{number:02}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Orbit {number:02}"))
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(GENRE, "Progressive Rock")
                .text(YEAR, "1995")
                .text(TRACK, &number.to_string())
                .build(),
        );
    }
}

#[test]
fn a_thin_catalog_offers_no_suggestion_it_cannot_fill() -> Result<()> {
    let tree = Tree::new();

    for (file, title) in [("a.wav", "Alpha"), ("b.wav", "Beta")] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits")
                .text(GENRE, "Progressive Rock")
                .text(YEAR, "1995")
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let suggestions = library.suggestions()?;

    assert!(
        suggestions.is_empty(),
        "a catalog of two tracks offered {:?}",
        asked(&suggestions)
    );
    Ok(())
}

#[test]
fn every_suggestion_reads_back_through_the_grammar_it_was_written_in() -> Result<()> {
    let tree = Tree::new();

    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let suggestions = library.suggestions()?;

    assert!(
        !suggestions.is_empty(),
        "a catalog of {TRACKS_OF_A_DECADE} tracks offered nothing"
    );

    for suggestion in suggestions.iter() {
        let text = suggestion
            .query
            .text
            .as_deref()
            .expect("a suggestion asks the catalog something");
        let read = Search::read(text);

        assert!(!read.is_empty(), "{text} reads as no search at all");
        assert_eq!(
            read.to_string(),
            *text,
            "{text} did not read back as what it was written as"
        );
        assert!(suggestion.rows > 0, "{text} was offered filling nothing");
        assert!(!suggestion.name.is_empty());
        assert!(!suggestion.reason.says().is_empty());
    }

    let texts = asked(&suggestions);

    assert!(texts.contains(&"year:1990-1999"), "{texts:?}");
    assert!(texts.contains(&"genre:=\"Progressive Rock\""), "{texts:?}");
    assert!(texts.contains(&"artist:=\"The Orbiters\""), "{texts:?}");
    assert!(texts.contains(&"plays:0"), "{texts:?}");
    Ok(())
}

#[test]
fn a_suggestion_says_how_long_it_runs_and_which_covers_picture_it() -> Result<()> {
    let tree = Tree::new();

    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let bare = library.suggestions()?;

    assert!(
        bare.iter()
            .all(|suggestion| suggestion.pictured_by.is_empty() && suggestion.length.is_some()),
        "a catalog with no cover pictured a suggestion or one ran for no time"
    );

    let album = only_album(&library)?;

    assert!(library.land_archive_cover(album.id, &png_art(64))?);

    let pictured = library.suggestions()?;

    assert!(
        pictured
            .iter()
            .all(|suggestion| suggestion.pictured_by == vec![album.id]),
        "the one covered album did not picture every suggestion it fills"
    );
    Ok(())
}

fn a_sleeve(side: u32, mirrored: bool) -> CoverArt {
    let drawn = image::RgbImage::from_fn(side, side, |x, y| {
        let across = if mirrored { side - 1 - x } else { x };
        let (u, v) = (across as f32 / side as f32, y as f32 / side as f32);
        if (0.15..0.55).contains(&u) && (0.2..0.6).contains(&v) {
            image::Rgb([220, 40, 30])
        } else {
            image::Rgb([(u * 60.0) as u8, (v * 90.0) as u8, 70])
        }
    });
    let mut written = std::io::Cursor::new(Vec::new());

    drawn
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a written picture");

    CoverArt {
        format: ImageFormat::Png,
        bytes: written.into_inner(),
    }
}

fn orbits_in_three_editions(tree: &Tree) {
    for (edition, album) in ["Orbits", "Orbits (Expanded)", "Other Orbits"]
        .into_iter()
        .enumerate()
    {
        for number in 1..=TRACKS_OF_A_DECADE / 2 {
            tree.write(
                &format!("orbits-{edition}/{number:02}.wav"),
                &Wav::new()
                    .text(TITLE, &format!("Orbit {number:02}"))
                    .text(ARTIST, "The Orbiters")
                    .text(ALBUM, album)
                    .text(GENRE, "Progressive Rock")
                    .text(YEAR, "1995")
                    .text(TRACK, &number.to_string())
                    .build(),
            );
        }
    }
}

#[test]
fn one_sleeve_saved_at_two_resolutions_pictures_a_suggestion_once() -> Result<()> {
    let tree = Tree::new();

    orbits_in_three_editions(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = |title: &str| -> Result<AlbumId> {
        Ok(library
            .albums(&AlbumQuery::default())?
            .into_iter()
            .find(|album| album.title == title)
            .unwrap_or_else(|| panic!("no album called {title}"))
            .id)
    };
    let edition = album("Orbits")?;
    let expanded = album("Orbits (Expanded)")?;
    let other = album("Other Orbits")?;

    assert!(library.land_archive_cover(edition, &a_sleeve(600, false))?);
    assert!(library.land_archive_cover(expanded, &a_sleeve(250, false))?);
    assert!(library.land_archive_cover(other, &a_sleeve(600, true))?);

    let decade = library
        .suggestions()?
        .iter()
        .find(|suggestion| suggestion.query.text.as_deref() == Some("year:1990-1999"))
        .cloned()
        .expect("the catalog suggests its decade");

    assert_eq!(decade.pictured_by.len(), 2, "{:?}", decade.pictured_by);
    assert!(decade.pictured_by.contains(&other));
    assert!(
        decade.pictured_by.contains(&edition) != decade.pictured_by.contains(&expanded),
        "one sleeve filled two tiles, or none: {:?}",
        decade.pictured_by
    );
    Ok(())
}

#[test]
fn suggestions_are_kept_until_a_name_they_were_built_from_moves() -> Result<()> {
    let tree = Tree::new();

    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let first = library.suggestions()?;

    assert!(
        Arc::ptr_eq(&first, &library.suggestions()?),
        "a second ask with nothing moved worked the suggestions out again"
    );

    library.save_query(
        "kept apart",
        &SavedQuery {
            text: Some("plays:0".to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            limit: None,
        },
    )?;

    assert!(
        Arc::ptr_eq(&first, &library.suggestions()?),
        "a playlist written moved suggestions that read no playlist"
    );

    let cold = all(&library)?
        .into_iter()
        .next()
        .expect("the scan stored the files it walked");

    library.favour(Favoured::Track(cold.id), true)?;

    assert!(
        !Arc::ptr_eq(&first, &library.suggestions()?),
        "a track written was answered with suggestions from before it"
    );
    Ok(())
}

#[test]
fn a_suggestion_saves_as_the_query_it_already_was() -> Result<()> {
    let tree = Tree::new();

    orbits_from_the_nineties(&tree);

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let suggestion = library
        .suggestions()?
        .first()
        .cloned()
        .expect("the catalog offers a suggestion");
    let saved = library.save_query(&suggestion.name, &suggestion.query)?;
    let playlist = library
        .playlist(saved)?
        .expect("the suggestion was saved as a playlist");

    assert_eq!(playlist.name, suggestion.name);
    assert_eq!(playlist.query, Some(suggestion.query));
    assert_eq!(u64::from(playlist.entries), suggestion.rows);
    Ok(())
}

struct ExploreCatalog {
    source: SourceId,
    suggestions: Vec<resonate_providers::Song>,
    searched: Mutex<Vec<String>>,
    opened: Mutex<Vec<String>>,
    away: bool,
}

impl ExploreCatalog {
    fn with(scheme: &str, suggestions: Vec<resonate_providers::Song>) -> Self {
        Self {
            source: source(scheme),
            suggestions,
            searched: Mutex::default(),
            opened: Mutex::default(),
            away: false,
        }
    }
}

impl resonate_providers::PlaysInPlace for ExploreCatalog {
    fn scheme(&self) -> &SourceId {
        &self.source
    }

    fn title(&self) -> &'static str {
        "Test catalog"
    }

    fn artists(&self) -> ProvidedResult<Vec<resonate_providers::Artist>> {
        Ok(Vec::new())
    }

    fn artist(
        &self,
        _: &resonate_providers::Id,
    ) -> ProvidedResult<resonate_providers::ArtistAlbums> {
        Err(resonate_providers::not_offered(
            &self.source,
            ProviderOp::Browse,
        ))
    }

    fn album(&self, _: &resonate_providers::Id) -> ProvidedResult<resonate_providers::AlbumSongs> {
        Err(resonate_providers::not_offered(
            &self.source,
            ProviderOp::Browse,
        ))
    }

    fn search(&self, words: &str) -> ProvidedResult<resonate_providers::Found> {
        self.searched.lock().push(words.to_owned());
        if self.away {
            return Err(ProvidedError::Busy {
                provider: self.source.clone(),
                op: ProviderOp::Search,
            });
        }
        Ok(resonate_providers::Found {
            songs: self.suggestions.clone(),
            ..Default::default()
        })
    }

    fn streamed(
        &self,
        id: &resonate_providers::Id,
    ) -> ProvidedResult<resonate_providers::Streamed> {
        self.opened.lock().push(id.to_string());

        let audio = Wav::new().frames(RATE as usize).build();
        let length = audio.len() as u64;
        Ok(resonate_providers::Streamed::seeking(
            io::Cursor::new(audio),
            Some(length),
            Some("audio/wav".to_owned()),
        ))
    }

    fn cover(&self, _: &resonate_providers::Id, _: u32) -> ProvidedResult<Vec<u8>> {
        Err(resonate_providers::not_offered(
            &self.source,
            ProviderOp::Cover,
        ))
    }

    fn cover_of(&self, _: &resonate_providers::Named) -> ProvidedResult<resonate_providers::Id> {
        Err(resonate_providers::not_offered(
            &self.source,
            ProviderOp::Cover,
        ))
    }
}

fn explore_song(id: &str, title: &str, artist: &str) -> resonate_providers::Song {
    let mut song =
        resonate_providers::Song::only(resonate_providers::Id::new(id).expect("a provider id"));

    song.title = title.to_owned();
    song.artist = Some(artist.to_owned());
    song.length = Some(Duration::from_secs(1));
    song.suffix = Some("wav".to_owned());
    song
}

#[test]
fn explore_uses_the_library_and_mix_seeds_and_keeps_new_songs_from_answering_services() -> Result<()>
{
    let (tree, library) = a_music_folder()?;

    held_as(&tree, "held.wav", "Held Song", RATE as usize);
    tree.write(
        "other.wav",
        &Wav::new()
            .frames(RATE as usize)
            .text(ARTIST, "Other Artist")
            .text(TITLE, "Another Song")
            .build(),
    );
    scan(&library, &options(&tree))?;

    let first = Arc::new(ExploreCatalog::with(
        "first",
        vec![
            explore_song("held", "Held Song", "The Orbiters"),
            explore_song("new", "A New Song", "The Orbiters"),
            explore_song("other", "A New Song", "Other Artist"),
            explore_song("else", "Unrelated", "Unrelated Artist"),
        ],
    ));
    let second = Arc::new(ExploreCatalog::with(
        "second",
        vec![explore_song("duplicate", "A New Song", "THE ORBITERS")],
    ));
    let away = Arc::new(ExploreCatalog {
        away: true,
        ..ExploreCatalog::with("away", Vec::new())
    });

    let sources = [
        resonate_providers::SongCatalog::new("first".to_owned(), first.clone()),
        resonate_providers::SongCatalog::new("second".to_owned(), second.clone()),
        resonate_providers::SongCatalog::new("away".to_owned(), away.clone()),
    ];
    let cache = resonate_library::ExploreCache::default();

    let home = library.explore(None, &sources, &cache, false)?;

    assert_eq!(home.failed, 1);
    assert_eq!(home.songs.len(), 2);
    assert!(home.songs.iter().all(|offer| offer.catalog == "first"));
    assert_eq!(first.searched.lock().len(), 2);
    assert_eq!(away.searched.lock().len(), 1);
    assert!(first.opened.lock().is_empty());
    assert_eq!(library.explore(None, &sources, &cache, false)?, home);
    assert_eq!(first.searched.lock().len(), 2);

    let mix = SavedQuery {
        text: Some("artist:=\"the orbiters\"".to_owned()),
        ..SavedQuery::default()
    };
    let found = library.explore(Some(&mix), &sources, &cache, false)?;

    assert_eq!(found.songs.len(), 1);
    assert_eq!(found.songs[0].song.id.as_str(), "new");
    assert_eq!(first.searched.lock().len(), 2);
    assert_eq!(songs_under(&tree).len(), 2);

    library.explore(Some(&mix), &sources, &cache, true)?;

    assert_eq!(first.searched.lock().len(), 3);
    assert_eq!(
        first.searched.lock().last().map(String::as_str),
        Some("the orbiters")
    );
    Ok(())
}

#[test]
fn an_explored_song_streams_without_a_copy_and_its_exact_download_survives_reopening() -> Result<()>
{
    let (tree, library, database) = a_music_folder_on_disk()?;
    let service = Arc::new(ExploreCatalog::with("exact", Vec::new()));
    let song = explore_song("song-42", "The Chosen Song", "The Orbiters");

    let location = resonate_providers::Named::Song(song.id.clone()).location(&service.source);
    let offer = resonate_library::ExploreOffer {
        catalog: "first".to_owned(),
        service: "Test catalog".to_owned(),
        location: location.clone(),
        song,
    };
    let playing = Arc::clone(&service);
    let sources = Sources::local().and(Arc::new(resonate_library::PlayedInPlace::new(
        service.source.clone(),
        move |_: &SourceId| Some(Arc::clone(&playing) as Arc<dyn resonate_providers::PlaysInPlace>),
    )));

    let (decoder, _) =
        resonate_codec::Decoder::open(&sources, &location).expect("the stream plays");

    drop(decoder);

    assert!(songs_under(&tree).is_empty());

    assert_eq!(library.keep_explored(offer.clone())?, KeptSong::Queued);
    assert_eq!(library.keep_explored(offer.clone())?, KeptSong::Queued);
    assert_eq!(library.keeps_served_queued()?, 1);
    assert!(library.kept_explored_locations()?.contains(&location));

    drop(library);

    let library = Library::open(&database)?;

    delivering_into(&library, &tree);

    let changed = Providers::none().with_song_catalog(resonate_providers::SongCatalog::new(
        "another catalog".to_owned(),
        service.clone(),
    ));

    assert_eq!(library.keep_pending(&changed, &|| false)?.landed, 0);
    assert_eq!(*service.opened.lock(), ["song-42"]);

    let providers = changed.with_song_catalog(resonate_providers::SongCatalog::new(
        "first".to_owned(),
        service.clone(),
    ));

    assert_eq!(library.keep_pending(&providers, &|| false)?.landed, 1);
    assert_eq!(*service.opened.lock(), ["song-42", "song-42"]);
    assert_eq!(songs_under(&tree).len(), 1);

    scan(&library, &options(&tree))?;

    let tracks = library.tracks(&TrackQuery::default())?;

    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "The Chosen Song");
    assert_eq!(
        library.keep_explored(offer)?,
        KeptSong::AlreadyHeld(tracks[0].id)
    );
    assert_eq!(library.keeps_served_queued()?, 0);
    Ok(())
}

#[test]
fn explore_rotates_direct_and_related_artists_and_stays_stable_within_the_day() {
    let direct = ["First", "Second", "Third"].map(str::to_owned);
    let kinship = resonate_library::Kinship::of(&[
        ("First".to_owned(), "Jazz".to_owned()),
        ("Kindred One".to_owned(), "Jazz".to_owned()),
        ("Kindred Two".to_owned(), "Jazz".to_owned()),
        ("Kindred Three".to_owned(), "Jazz".to_owned()),
    ]);

    let today = resonate_library::explore_seeds(&direct, &kinship, 0);

    assert_eq!(today, ["First", "Second", "Kindred One", "Kindred Three"]);
    assert_eq!(resonate_library::explore_seeds(&direct, &kinship, 0), today);
    assert_eq!(
        resonate_library::explore_seeds(&direct, &kinship, 1),
        ["Second", "Third", "Kindred Three", "Kindred Two"],
    );
}

#[test]
fn explore_blueprints_and_playlists_generate_custom_curated_lists_and_share_cache() -> Result<()> {
    let profile = resonate_library::ExploreProfile {
        recent_artists: vec!["The Orbiters".to_owned(), "Solar Drift".to_owned()],
        favourite_artists: vec!["Lunar Echo".to_owned()],
        top_artists: vec!["The Orbiters".to_owned(), "Solar Drift".to_owned()],
        left_behind_artists: vec!["Old Comet".to_owned()],
        top_genres: vec!["space rock".to_owned()],
    };
    let kinship = resonate_library::Kinship::of(&[
        ("The Orbiters".to_owned(), "space rock".to_owned()),
        ("Solar Drift".to_owned(), "space rock".to_owned()),
        ("Kindred Star".to_owned(), "space rock".to_owned()),
        ("Lunar Echo".to_owned(), "dream pop".to_owned()),
        ("Old Comet".to_owned(), "space rock".to_owned()),
    ]);

    let blueprints = resonate_library::explore_blueprints(&profile, &kinship, 0);

    assert!(
        blueprints
            .iter()
            .any(|bp| bp.kind == resonate_library::ExplorePlaylistKind::DiscoverMix)
    );
    assert!(
        blueprints
            .iter()
            .any(|bp| bp.kind == resonate_library::ExplorePlaylistKind::BeyondArtist)
    );
    assert!(
        blueprints
            .iter()
            .any(|bp| bp.kind == resonate_library::ExplorePlaylistKind::GenreDiscovery)
    );
    assert!(
        blueprints
            .iter()
            .any(|bp| bp.kind == resonate_library::ExplorePlaylistKind::FavouritesExpansion)
    );
    assert!(
        blueprints
            .iter()
            .any(|bp| bp.kind == resonate_library::ExplorePlaylistKind::RediscoverBeyond)
    );

    let (tree, library) = a_music_folder()?;

    held_as(&tree, "held.wav", "Held Song", RATE as usize);
    tree.write(
        "other.wav",
        &Wav::new()
            .frames(RATE as usize)
            .text(ARTIST, "Solar Drift")
            .text(TITLE, "Owned Drift")
            .build(),
    );
    scan(&library, &options(&tree))?;

    let mut song_a = explore_song("a1", "Orbit One", "The Orbiters");

    song_a.album = Some("Album One".to_owned());

    let mut song_b = explore_song("a2", "Orbit Two", "The Orbiters");

    song_b.album = Some("Album One".to_owned());

    let mut song_c = explore_song("a3", "Orbit Three", "The Orbiters");

    song_c.album = Some("Album One".to_owned());

    let song_d = explore_song("b1", "Drift Away", "Solar Drift");
    let song_e = explore_song("b2", "Drift Back", "Solar Drift");

    let service = Arc::new(ExploreCatalog::with(
        "first",
        vec![
            explore_song("held", "Held Song", "The Orbiters"),
            song_a,
            song_b,
            song_c,
            song_d,
            song_e,
        ],
    ));
    let sources = [resonate_providers::SongCatalog::new(
        "first".to_owned(),
        service.clone(),
    )];
    let cache = resonate_library::ExploreCache::default();

    let first_run = library.explore_playlists(&sources, &cache, false)?;

    assert!(!first_run.playlists.is_empty());

    let searches_after_first = service.searched.lock().len();

    assert_eq!(searches_after_first, 2);

    let discover = &first_run.playlists[0];

    assert_eq!(
        discover.kind,
        resonate_library::ExplorePlaylistKind::DiscoverMix
    );
    assert!(
        discover
            .songs
            .iter()
            .all(|offer| offer.song.title != "Held Song")
    );

    let album_one_count = discover
        .songs
        .iter()
        .filter(|offer| offer.song.album.as_deref() == Some("Album One"))
        .count();

    assert!(album_one_count <= 2);

    let second_run = library.explore_playlists(&sources, &cache, false)?;

    assert_eq!(second_run, first_run);
    assert_eq!(service.searched.lock().len(), searches_after_first);
    Ok(())
}

#[test]
fn weave_mix_interleaves_streamed_suggestions_and_avoids_adjacent_same_artist() {
    let source = SourceId::new("first").expect("a source");
    let make_offer = |id: &str, title: &str, artist: &str| {
        let song = explore_song(id, title, artist);
        let location = resonate_providers::Named::Song(song.id.clone()).location(&source);
        resonate_library::ExploreOffer {
            catalog: "first".to_owned(),
            service: "Test".to_owned(),
            location,
            song,
        }
    };

    let local = vec![
        ("L1", "Alpha"),
        ("L2", "Beta"),
        ("L3", "Gamma"),
        ("L4", "Delta"),
        ("L5", "Epsilon"),
        ("L6", "Zeta"),
        ("L7", "Eta"),
    ];
    let suggestions = vec![
        make_offer("s1", "Sug One", "Gamma"),
        make_offer("s2", "Sug Two", "Nova"),
        make_offer("s3", "Sug Three", "Orion"),
    ];

    let woven = resonate_library::weave_mix(local, |(_, artist)| Some(*artist), suggestions);

    assert_eq!(woven.len(), 9);
    assert!(matches!(
        woven[0],
        resonate_library::MixEntry::Local(("L1", _))
    ));
    assert!(matches!(
        woven[1],
        resonate_library::MixEntry::Local(("L2", _))
    ));
    assert!(matches!(
        woven[2],
        resonate_library::MixEntry::Local(("L3", _))
    ));

    match &woven[3] {
        resonate_library::MixEntry::Suggested(offer) => {
            assert_eq!(offer.song.artist.as_deref(), Some("Nova"));
        }
        resonate_library::MixEntry::Local(_) => panic!("expected a suggested song after 3 locals"),
    }

    assert!(matches!(
        woven[4],
        resonate_library::MixEntry::Local(("L4", _))
    ));
    assert!(matches!(
        woven[5],
        resonate_library::MixEntry::Local(("L5", _))
    ));
    assert!(matches!(
        woven[6],
        resonate_library::MixEntry::Local(("L6", _))
    ));
    assert!(matches!(woven[7], resonate_library::MixEntry::Suggested(_)));
    assert!(matches!(
        woven[8],
        resonate_library::MixEntry::Local(("L7", _))
    ));
}

#[test]
fn explore_playlists_persist_across_restarts_until_refreshed_or_expired() -> Result<()> {
    let (tree, library, database) = a_music_folder_on_disk()?;

    held_as(&tree, "held.wav", "Held Song", RATE as usize);
    tree.write(
        "other.wav",
        &Wav::new()
            .frames(RATE as usize)
            .text(ARTIST, "Solar Drift")
            .text(TITLE, "Owned Drift")
            .build(),
    );
    scan(&library, &options(&tree))?;

    let mut song_a = explore_song("a1", "Orbit One", "The Orbiters");

    song_a.album = Some("Album One".to_owned());
    song_a.disc = Some(1);
    song_a.track = Some(3);
    song_a.cover = resonate_providers::Id::new("cov-1");
    song_a.favourite = Some(true);
    song_a.rating = resonate_core::titles::Rating::Explicit;

    let song_b = explore_song("b1", "Drift Away", "Solar Drift");

    let service = Arc::new(ExploreCatalog::with(
        "first",
        vec![
            explore_song("held", "Held Song", "The Orbiters"),
            song_a,
            song_b,
        ],
    ));
    let sources = [resonate_providers::SongCatalog::new(
        "first".to_owned(),
        service.clone(),
    )];
    let first_cache = resonate_library::ExploreCache::default();

    let first_run = library.explore_playlists(&sources, &first_cache, false)?;

    assert!(!first_run.playlists.is_empty());

    let searches_after_cold = service.searched.lock().len();

    assert_eq!(searches_after_cold, 2);

    drop(library);

    let reopened = Library::open(&database)?;
    let fresh_cache = resonate_library::ExploreCache::default();

    let warm_run = reopened.explore_playlists(&sources, &fresh_cache, false)?;

    assert_eq!(warm_run, first_run);
    assert_eq!(service.searched.lock().len(), searches_after_cold);

    let other_server = [resonate_providers::SongCatalog::new(
        "second-server".to_owned(),
        service.clone(),
    )];
    let other_cache = resonate_library::ExploreCache::default();

    let other_run = reopened.explore_playlists(&other_server, &other_cache, false)?;

    assert!(!other_run.playlists.is_empty());
    assert_eq!(service.searched.lock().len(), searches_after_cold + 2);

    let refreshed = reopened.explore_playlists(&sources, &fresh_cache, true)?;

    assert_eq!(refreshed, first_run);
    assert_eq!(service.searched.lock().len(), searches_after_cold + 4);

    let expiry_cache = resonate_library::ExploreCache::default();
    let blueprints = reopened.explore_blueprints()?;
    let day = resonate_core::Calendar::local().day_of(SystemTime::now());

    let still_warm = expiry_cache.hydrate_blueprints_with(
        day + resonate_library::EXPLORE_CACHE_DAYS - 1,
        blueprints.clone(),
        &sources,
        false,
        &reopened,
    );

    assert!(!still_warm.artists().is_empty());
    assert_eq!(service.searched.lock().len(), searches_after_cold + 4);

    let expired_cache = resonate_library::ExploreCache::default();
    let _expired = expired_cache.hydrate_blueprints_with(
        day + resonate_library::EXPLORE_CACHE_DAYS,
        blueprints,
        &sources,
        false,
        &reopened,
    );

    assert_eq!(service.searched.lock().len(), searches_after_cold + 6);
    Ok(())
}

#[test]
fn explore_covers_survive_reopening_and_are_shared_by_cover_id_within_a_catalog() -> Result<()> {
    let (_tree, library, database) = a_music_folder_on_disk()?;
    let source = SourceId::new("test-cover").expect("a source id");
    let offer_of = |id: &str, catalog: &str| {
        let mut song = explore_song(id, "Orbit One", "The Orbiters");

        song.cover = resonate_providers::Id::new("sleeve");
        resonate_library::ExploreOffer {
            catalog: catalog.to_owned(),
            service: "Test".to_owned(),
            location: resonate_providers::Named::Song(song.id.clone()).location(&source),
            song,
        }
    };
    let offer = offer_of("one", "first");
    let side = NonZeroU32::new(128).expect("a drawn side");
    let bytes = b"a cover";

    assert!(library.explore_cover(&offer, side).is_none());

    library.keep_explore_cover(&offer, side, bytes);
    drop(library);

    let reopened = Library::open(&database)?;
    let from_same_album = offer_of("two", "first");

    assert_eq!(
        reopened.explore_cover(&from_same_album, side).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(
        reopened
            .explore_cover(&offer_of("one", "second"), side)
            .is_none()
    );
    assert!(
        reopened
            .explore_cover(&offer, side.saturating_mul(side))
            .is_none()
    );

    let connection = rusqlite::Connection::open(&database).expect("the catalog reopens");

    connection
        .execute(
            "UPDATE explore_covers SET day = day - ?1",
            [resonate_library::EXPLORE_CACHE_DAYS],
        )
        .expect("the cover expires");

    assert!(reopened.explore_cover(&offer, side).is_none());

    reopened.keep_explore_cover(&offer, side, bytes);

    assert_eq!(
        reopened.explore_cover(&offer, side).as_deref(),
        Some(bytes.as_slice())
    );
    Ok(())
}
