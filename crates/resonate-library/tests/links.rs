use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_library::{
    AlbumLink, AlbumNames, AlbumQuery, ArtistFound, ArtistLink, ArtistMatch, BarcodeMatch, Error,
    Isrc, Issued, KEPT_IN_STEP_EVERY, Library, Link, LinkNames, Linked, LinkedPlaylist,
    LinkedProfile, ListedSong, LookupOp, Mbid, PlaylistLink, ProfileLink, ProfilePlaylist,
    RecordingAsked, RecordingMatch, RecordingRelease, Relation, ReleaseTrack, Result, Service,
    SongLink, Track, Upkeep, Wording,
};

use crate::support::{
    ALBUM, ANOTHER_CODE, ARTIST, CODE, Called, Canned, DAYBREAK, Fake, Fault, HOURS, HOURS_BONUS,
    HOURS_GROUP, HOURS_PAPER_BOATS, HOURS_REISSUE, ISRC, LOW_TIDE, MUSICBRAINZ, ORBITERS,
    ORBITS_TITLES, RECORDING, RELEASE, RELEASE_GROUP, SLOW_WATER, TITLE, Tree, Wav, YEAR, all,
    artist_named, barcode, credited, enrich, hours_pressed, isrc, learnt_canned, low_tide_release,
    mbid, only_album, options, orbiters, orbits, orbits_group, orbits_match, orbits_rows,
    release_row, scan, scanned_orbits, slow_water_found,
};

const A_STREAMING_LINK: &str = "https://open.spotify.com/track/1a2b3c4d5e";

const AN_ENCYCLOPAEDIA_LINK: &str = "https://en.wikipedia.org/wiki/Orbits";

#[test]
fn a_share_names_the_track_and_the_album_it_came_from() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(YEAR, "1971")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let track = all(&library)?.remove(0);

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(shared.title, "Lantern");
    assert_eq!(shared.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(shared.album.as_deref(), Some("Orbits"));
    assert_eq!(shared.year, Some(1971));
    assert!(shared.links.is_empty());
    assert_eq!(shared.written(), None);
    Ok(())
}

#[test]
fn a_share_prefers_a_streaming_link_over_musicbrainz() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![release_row(
                1,
                "Lantern",
                vec![Link::new("streaming", A_STREAMING_LINK.to_owned())],
            )],
            vec![Link::new("wikipedia", AN_ENCYCLOPAEDIA_LINK.to_owned())],
        ),
    )?;
    library.rematch(album.id)?;

    let track = all(&library)?.remove(0);
    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(shared.recording, Some(mbid(RECORDING)));
    assert_eq!(
        shared.links.first().map(|link| link.url.as_str()),
        Some(A_STREAMING_LINK),
        "the recording\'s own link is not the first the share weighs"
    );
    assert_eq!(
        shared.written().as_deref(),
        Some("https://song.link/s/1a2b3c4d5e")
    );
    Ok(())
}

#[test]
fn a_share_falls_back_to_musicbrainz_where_no_service_is_linked() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![release_row(1, "Lantern", Vec::new())],
            vec![Link::new("wikipedia", AN_ENCYCLOPAEDIA_LINK.to_owned())],
        ),
    )?;
    library.rematch(album.id)?;

    let track = all(&library)?.remove(0);
    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(
        shared.links.first().map(|link| link.url.as_str()),
        Some(AN_ENCYCLOPAEDIA_LINK),
        "the album\'s link is held even where song.link cannot resolve it"
    );
    assert_eq!(
        shared.written().as_deref(),
        Some("https://musicbrainz.org/recording/b1a9c0de-1111-4222-8333-444455556666")
    );
    Ok(())
}

#[test]
fn a_share_of_a_track_nothing_knows_about_has_no_link() -> Result<()> {
    let tree = Tree::new();

    tree.write("mystery.wav", &Wav::new().build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let track = all(&library)?.remove(0);

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    assert_eq!(shared.recording, None);
    assert!(shared.links.is_empty());
    assert_eq!(shared.written(), None);
    Ok(())
}

const A_DEEZER_LINK: &str = "https://www.deezer.com/track/677241";

fn a_track_with_no_service_linked(tree: &Tree) -> Result<(Library, Track)> {
    tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, "GBAYE7100195")
            .identified(MUSICBRAINZ, RECORDING)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(tree))?;

    let track = all(&library)?.remove(0);

    Ok((library, track))
}

#[test]
fn a_share_with_no_service_linked_asks_where_the_track_streams_and_hands_that_to_song_link()
-> Result<()> {
    let tree = Tree::new();
    let (library, track) = a_track_with_no_service_linked(&tree)?;
    let fake = Fake::new(Canned {
        streamed: Some(Link {
            relation: Relation::Streaming,
            service: Service::Deezer,
            url: A_DEEZER_LINK.to_owned(),
        }),
        ..Canned::default()
    });

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned")
        .streamed_where_asked(&fake);

    let calls = fake.calls();
    let [Called::StreamedAt(asked)] = calls.as_slice() else {
        panic!("the share asked {calls:?}");
    };

    assert_eq!(asked.title, "Lantern");
    assert_eq!(asked.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(asked.isrc.as_ref().map(Isrc::as_str), Some("GBAYE7100195"));
    assert!(asked.length.is_some(), "the share asked with no length");
    assert_eq!(
        shared.written().as_deref(),
        Some("https://song.link/d/677241")
    );
    Ok(())
}

#[test]
fn a_share_already_linked_to_a_service_asks_nothing() -> Result<()> {
    let tree = Tree::new();
    let (library, track) = a_track_with_no_service_linked(&tree)?;
    let fake = Fake::new(Canned::default());

    let mut shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned");

    shared
        .links
        .push(Link::new("streaming", A_STREAMING_LINK.to_owned()));

    let shared = shared.streamed_where_asked(&fake);

    assert!(
        fake.calls().is_empty(),
        "the share asked {:?}",
        fake.calls()
    );
    assert_eq!(
        shared.written().as_deref(),
        Some("https://song.link/s/1a2b3c4d5e")
    );
    Ok(())
}

#[test]
fn a_share_the_service_could_not_answer_for_falls_back_to_musicbrainz() -> Result<()> {
    let tree = Tree::new();
    let (library, track) = a_track_with_no_service_linked(&tree)?;
    let fake = Fake::new(Canned::default()).faulting(LookupOp::StreamLink, 0, Fault::Unreachable);

    let shared = library
        .shareable(track.id)?
        .expect("the catalog holds the track it just scanned")
        .streamed_where_asked(&fake);

    assert_eq!(fake.called(LookupOp::StreamLink), 1);
    assert_eq!(
        shared.written().as_deref(),
        Some("https://musicbrainz.org/recording/b1a9c0de-1111-4222-8333-444455556666")
    );
    Ok(())
}

const A_SPOTIFY_LINK: &str = "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT";

fn linked_to_spotify() -> SongLink {
    SongLink::read(A_SPOTIFY_LINK).expect("a song link")
}

#[test]
fn a_link_to_a_song_nothing_holds_is_followed_by_its_isrc_to_the_recording_to_want() -> Result<()> {
    let library = Library::open_in_memory()?;
    let code = isrc(CODE);
    let mut short = slow_water_found().into_recording();

    short.id = mbid(RECORDING);
    short.length = Some(Duration::from_secs(300));

    let mut take = slow_water_found().into_recording();

    take.releases = vec![take.releases[1].clone()];

    let whole = slow_water_found().into_recording();
    let fake = Fake::new(Canned {
        linked: Some(LinkNames {
            isrcs: vec![code.clone()],
            length: Some(Duration::from_secs(1_410)),
            title: None,
            artist: None,
        }),
        isrcs: vec![(code.clone(), short), (code.clone(), take)],
        recordings: vec![whole],
        ..Canned::default()
    });

    let Linked::Found(found) = library.follow_link(&fake, &linked_to_spotify())? else {
        panic!("the link named no song to want");
    };

    assert_eq!(found.recording, mbid(SLOW_WATER));
    assert_eq!(found.artist, "Example Band");
    assert_eq!(
        found.release.as_ref().map(|release| release.id.clone()),
        Some(mbid(LOW_TIDE)),
        "the recording asked for whole is placed on the album it was meant for"
    );
    assert_eq!(
        fake.calls(),
        vec![
            Called::SongLinked(linked_to_spotify()),
            Called::Isrc(code),
            Called::Recording(mbid(SLOW_WATER)),
        ]
    );
    Ok(())
}

#[test]
fn a_link_to_a_song_the_library_holds_answers_the_track_and_asks_musicbrainz_nothing() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows[0] = ReleaseTrack {
        recording: Some(mbid(RECORDING)),
        isrcs: vec!["GBAYE7100195".to_owned()],
        ..rows[0].clone()
    };

    let identifying = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &identifying, false)?;

    let fake = Fake::new(Canned {
        linked: Some(LinkNames {
            isrcs: vec![isrc(CODE)],
            length: None,
            title: None,
            artist: None,
        }),
        ..Canned::default()
    });

    let by_isrc = library.follow_link(&fake, &linked_to_spotify())?;
    let by_recording = library.follow_link(&fake, &SongLink::MusicBrainz(mbid(RECORDING)))?;

    let first = all(&library)?
        .into_iter()
        .find(|track| track.title == ORBITS_TITLES[0])
        .expect("the first track held")
        .id;
    let held = Linked::Held {
        track: first,
        title: ORBITS_TITLES[0].to_owned(),
        artist: Some("The Orbiters".to_owned()),
    };

    assert_eq!(by_isrc, held);
    assert_eq!(by_recording, held);
    assert_eq!(
        fake.calls(),
        vec![Called::SongLinked(linked_to_spotify())],
        "a song held is not asked about"
    );
    Ok(())
}

fn orbits_with_the_first_held_by_recording() -> Result<(Tree, Library)> {
    let (tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows[0] = ReleaseTrack {
        recording: Some(mbid(RECORDING)),
        ..rows[0].clone()
    };

    let identifying = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &identifying, false)?;
    Ok((tree, library))
}

fn a_service_listing(songs: Vec<ListedSong>) -> Fake {
    Fake::new(Canned {
        playlist_linked: Some(LinkedPlaylist {
            name: "Night drive".to_owned(),
            songs,
        }),
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    })
}

#[test]
fn a_playlist_kept_in_step_with_its_link_takes_what_the_service_added_and_drops_what_it_took_out()
-> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let link = PlaylistLink::Spotify("37i9dQZF1DXcBWIGoYBM5M".to_owned());
    let holding = a_service_listing(vec![
        ListedSong::Recording(mbid(RECORDING)),
        ListedSong::Recording(mbid(SLOW_WATER)),
    ]);
    let emptied = a_service_listing(vec![ListedSong::Recording(mbid(SLOW_WATER))]);

    let imported = library
        .import_playlist_link(&holding, &link, None, Upkeep::InStep)?
        .expect("a playlist the service read");
    let again = library
        .import_playlist_link(&holding, &link, None, Upkeep::InStep)?
        .expect("the same playlist read again");
    let source = library
        .playlist(imported.playlist)?
        .and_then(|playlist| playlist.source)
        .expect("the link the playlist came from");
    let refreshed = library
        .refresh_playlist(&emptied, imported.playlist)?
        .expect("the playlist read again");

    assert!(imported.started);
    assert_eq!(imported.name, "Night drive");
    assert_eq!(imported.added, 1);
    assert_eq!(again.playlist, imported.playlist);
    assert!(
        !again.started,
        "importing a kept link again started a second playlist"
    );
    assert_eq!((again.added, again.removed), (0, 0));
    assert_eq!(source.link, link);
    assert_eq!(source.upkeep, Upkeep::InStep);
    assert!(!source.is_due(SystemTime::now()));
    assert!(source.is_due(SystemTime::now() + KEPT_IN_STEP_EVERY));
    assert_eq!((refreshed.added, refreshed.removed), (0, 1));
    assert!(
        library
            .playlist_entries(imported.playlist, None)?
            .is_empty()
    );
    assert_eq!(
        refreshed
            .found
            .iter()
            .map(|found| found.recording.clone())
            .collect::<Vec<_>>(),
        [mbid(SLOW_WATER)],
        "a song nobody wants yet is offered to be wanted"
    );
    assert_eq!(
        library.playlists_kept_in_step()?.len(),
        1,
        "one playlist follows the link"
    );
    Ok(())
}

fn a_profile_holding(playlists: &[(u64, &str, ListedSong)]) -> Fake {
    Fake::new(Canned {
        profile_linked: Some(LinkedProfile {
            name: "listener".to_owned(),
            playlists: playlists
                .iter()
                .map(|(id, title, _)| ProfilePlaylist {
                    title: (*title).to_owned(),
                    link: PlaylistLink::Deezer(*id),
                })
                .collect(),
        }),
        playlists_linked: playlists
            .iter()
            .map(|(id, title, song)| {
                (
                    PlaylistLink::Deezer(*id),
                    LinkedPlaylist {
                        name: (*title).to_owned(),
                        songs: vec![song.clone()],
                    },
                )
            })
            .collect(),
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    })
}

#[test]
fn a_profile_followed_takes_its_new_playlists_lets_go_of_its_deleted_ones_and_never_revives_one_let_go()
-> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let profile = ProfileLink::Deezer(2_529);
    let first = a_profile_holding(&[
        (1, "Loved Tracks", ListedSong::Recording(mbid(RECORDING))),
        (2, "Night drive", ListedSong::Recording(mbid(SLOW_WATER))),
    ]);
    let later = a_profile_holding(&[
        (1, "Loved Tracks", ListedSong::Recording(mbid(RECORDING))),
        (3, "Morning", ListedSong::Recording(mbid(RECORDING))),
    ]);

    let followed = library
        .follow_profile(&first, &profile, Upkeep::InStep)?
        .expect("a profile the service read");
    let loved = followed.playlists[0].playlist;
    let night = followed.playlists[1].playlist;

    library.keep_playlist_from_its_link(loved, Upkeep::Once)?;

    let again = library
        .follow_profile(&later, &profile, Upkeep::InStep)?
        .expect("the profile read again");
    let due_now = library.profiles_due_a_refresh(SystemTime::now())?;
    let due_later = library.profiles_due_a_refresh(SystemTime::now() + KEPT_IN_STEP_EVERY)?;

    assert_eq!(followed.name, "listener");
    assert_eq!(followed.playlists.len(), 2);
    assert!(followed.playlists.iter().all(|read| read.started));
    assert_eq!(
        (again.playlists.len(), again.let_go, again.gone),
        (1, 1, 1),
        "the new playlist taken, the one let go left, the deleted one's link dropped"
    );
    assert_eq!(again.playlists[0].name, "Morning");
    assert_eq!(library.playlist_source(night)?, None);
    assert_eq!(
        library.playlist_source(loved)?.map(|source| source.upkeep),
        Some(Upkeep::Once)
    );
    assert!(due_now.is_empty());
    assert_eq!(due_later, [profile]);
    Ok(())
}

fn a_spotify_account_listing(songs: Vec<ListedSong>) -> Fake {
    let own = PlaylistLink::Spotify("3cEYpjA9oz9GiPac4AsH4n".to_owned());
    Fake::new(Canned {
        profile_linked: Some(LinkedProfile {
            name: "A Listener".to_owned(),
            playlists: vec![
                ProfilePlaylist {
                    title: "Liked Songs".to_owned(),
                    link: PlaylistLink::SpotifyLiked,
                },
                ProfilePlaylist {
                    title: "Kept for the road".to_owned(),
                    link: own.clone(),
                },
            ],
        }),
        playlists_linked: vec![
            (
                PlaylistLink::SpotifyLiked,
                LinkedPlaylist {
                    name: "Liked Songs".to_owned(),
                    songs: songs.clone(),
                },
            ),
            (
                own,
                LinkedPlaylist {
                    name: "Kept for the road".to_owned(),
                    songs,
                },
            ),
        ],
        isrcs: vec![(
            Isrc::new("USRC17607839").expect("a code"),
            slow_water_found().into_recording(),
        )],
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    })
}

#[test]
fn a_spotify_account_is_followed_as_a_profile_each_song_by_its_isrc_and_one_not_found_said_so()
-> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let coded = ListedSong::Named(LinkNames {
        isrcs: vec![Isrc::new("USRC17607839").expect("a code")],
        length: None,
        title: Some("Lantern".to_owned()),
        artist: Some("Example Band".to_owned()),
    });
    let nothing_knows = ListedSong::Named(LinkNames {
        isrcs: vec![Isrc::new("ZZZZZ0000000").expect("a code")],
        length: Some(Duration::from_secs(200)),
        title: Some("Near the same name".to_owned()),
        artist: Some("Someone else".to_owned()),
    });

    let account = a_spotify_account_listing(vec![coded, nothing_knows]);

    let followed = library
        .follow_profile(&account, &ProfileLink::SpotifyAccount, Upkeep::InStep)?
        .expect("the account read");
    let liked = library.playlist_source(followed.playlists[0].playlist)?;

    assert_eq!(followed.name, "A Listener");
    assert_eq!(followed.playlists.len(), 2);
    assert_eq!(followed.playlists[0].name, "Liked Songs");
    assert_eq!(followed.playlists[0].found.len(), 1, "found by its ISRC");
    assert_eq!(
        followed.playlists[0].unnamed, 1,
        "one nothing names strictly is reported, never matched loosely"
    );
    assert_eq!(
        liked.map(|source| (source.link, source.upkeep)),
        Some((PlaylistLink::SpotifyLiked, Upkeep::InStep))
    );
    assert_eq!(
        library
            .profiles_followed()?
            .into_iter()
            .map(|(link, _)| link)
            .collect::<Vec<_>>(),
        [ProfileLink::SpotifyAccount]
    );
    Ok(())
}

#[test]
fn a_spotify_account_with_nobody_signed_in_names_nothing_and_starts_no_playlist() -> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let nobody = Fake::new(Canned::default());

    assert_eq!(
        library.follow_profile(&nobody, &ProfileLink::SpotifyAccount, Upkeep::InStep)?,
        None
    );
    assert!(library.profiles_followed()?.is_empty());
    assert!(library.playlists_kept_in_step()?.is_empty());
    Ok(())
}

#[test]
fn a_profile_let_go_keeps_its_playlists_rows_and_no_longer_follows_them() -> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let account = a_spotify_account_listing(vec![ListedSong::Recording(mbid(RECORDING))]);

    let followed = library
        .follow_profile(&account, &ProfileLink::SpotifyAccount, Upkeep::InStep)?
        .expect("the account read");
    let let_go = library.let_go_of_the_profile(&ProfileLink::SpotifyAccount)?;

    assert_eq!(let_go, 2);
    assert!(library.profiles_followed()?.is_empty());

    for read in &followed.playlists {
        assert_eq!(library.playlist_source(read.playlist)?, None);
        assert_eq!(library.playlist_entries(read.playlist, None)?.len(), 1);
    }
    Ok(())
}

#[test]
fn a_profile_moved_once_leaves_its_playlists_with_no_link_and_is_not_followed() -> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let profile = ProfileLink::Deezer(2_529);
    let holding = a_profile_holding(&[(1, "Loved Tracks", ListedSong::Recording(mbid(RECORDING)))]);

    library.follow_profile(&holding, &profile, Upkeep::InStep)?;

    let moved = library
        .follow_profile(&holding, &profile, Upkeep::Once)?
        .expect("the profile read");

    assert_eq!(moved.playlists.len(), 1);
    assert_eq!(library.playlist_source(moved.playlists[0].playlist)?, None);
    assert!(library.profiles_followed()?.is_empty());
    Ok(())
}

#[test]
fn a_playlist_imported_once_keeps_what_the_service_took_out_and_can_be_kept_in_step_later()
-> Result<()> {
    let (_tree, library) = orbits_with_the_first_held_by_recording()?;
    let link = PlaylistLink::Deezer(7);
    let holding = a_service_listing(vec![ListedSong::Recording(mbid(RECORDING))]);
    let emptied = a_service_listing(Vec::new());

    let imported = library
        .import_playlist_link(&holding, &link, None, Upkeep::Once)?
        .expect("a playlist the service read");
    let refreshed = library
        .refresh_playlist(&emptied, imported.playlist)?
        .expect("the playlist read again");
    let held_once = library.playlist_entries(imported.playlist, None)?.len();
    let now_kept = library.keep_playlist_from_its_link(imported.playlist, Upkeep::InStep)?;
    let due = library.playlists_due_a_refresh(SystemTime::now() + KEPT_IN_STEP_EVERY)?;
    let mirrored = library
        .refresh_playlist(&emptied, imported.playlist)?
        .expect("the playlist read again");
    let forgotten = library.forget_playlist_source(imported.playlist)?;

    assert_eq!((refreshed.added, refreshed.removed), (0, 0));
    assert_eq!(
        held_once, 1,
        "a playlist imported once lost a row the service dropped"
    );
    assert!(now_kept);
    assert_eq!(due, [imported.playlist]);
    assert_eq!(mirrored.removed, 1);
    assert!(forgotten);
    assert_eq!(
        library
            .playlist(imported.playlist)?
            .and_then(|playlist| playlist.source),
        None
    );
    assert!(matches!(
        library.refresh_playlist(&emptied, imported.playlist),
        Err(Error::NoPlaylistSource { .. })
    ));
    Ok(())
}

#[test]
fn a_playlist_link_names_the_songs_held_the_songs_to_want_and_fills_a_playlist_once() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows[0] = ReleaseTrack {
        recording: Some(mbid(RECORDING)),
        ..rows[0].clone()
    };

    let identifying = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![orbits(rows, Vec::new())],
        ..Canned::default()
    }));

    enrich(&library, &identifying, false)?;

    let held = all(&library)?
        .into_iter()
        .find(|track| track.title == ORBITS_TITLES[0])
        .expect("the first track held")
        .id;
    let fake = Fake::new(Canned {
        playlist_linked: Some(LinkedPlaylist {
            name: "Night drive".to_owned(),
            songs: vec![
                ListedSong::Recording(mbid(RECORDING)),
                ListedSong::Recording(mbid(SLOW_WATER)),
                ListedSong::Named(LinkNames {
                    isrcs: vec![isrc(ANOTHER_CODE)],
                    length: None,
                    title: None,
                    artist: None,
                }),
            ],
        }),
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    });

    let followed = library
        .follow_playlist_link(&fake, &PlaylistLink::Deezer(1))?
        .expect("a playlist the service read");
    let first = library.fill_playlist_named(&followed.name, &followed.held)?;
    let again = library.fill_playlist_named(&followed.name, &followed.held)?;

    assert_eq!(followed.name, "Night drive");
    assert_eq!(followed.held, [held]);
    assert_eq!(
        followed
            .found
            .iter()
            .map(|found| found.recording.clone())
            .collect::<Vec<_>>(),
        [mbid(SLOW_WATER)]
    );
    assert_eq!(followed.unnamed, 1);
    assert!(first.started);
    assert_eq!(first.added, 1);
    assert_eq!(again.playlist, first.playlist);
    assert!(!again.started);
    assert_eq!(
        again.added, 0,
        "a playlist followed again took a song twice"
    );
    assert_eq!(library.playlist_entries(first.playlist, None)?.len(), 1);
    Ok(())
}

#[test]
fn a_link_no_service_can_name_names_nothing() -> Result<()> {
    let library = Library::open_in_memory()?;
    let unnamed = Fake::new(Canned::default());
    let unknown = Fake::new(Canned {
        linked: Some(LinkNames {
            isrcs: vec![isrc(ANOTHER_CODE)],
            length: None,
            title: None,
            artist: None,
        }),
        ..Canned::default()
    });

    assert_eq!(
        library.follow_link(&unnamed, &linked_to_spotify())?,
        Linked::Unnamed
    );
    assert_eq!(
        library.follow_link(&unknown, &linked_to_spotify())?,
        Linked::Unnamed
    );
    Ok(())
}

const A_YOUTUBE_LINK: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

fn linked_to_youtube() -> SongLink {
    SongLink::read(A_YOUTUBE_LINK).expect("a song link")
}

fn linked_to_youtube_music() -> SongLink {
    SongLink::read("https://music.youtube.com/watch?v=dQw4w9WgXcQ").expect("a song link")
}

fn titled_on_the_page(title: &str, artist: &str, seconds: u64) -> LinkNames {
    LinkNames {
        isrcs: Vec::new(),
        length: Some(Duration::from_secs(seconds)),
        title: Some(title.to_owned()),
        artist: Some(artist.to_owned()),
    }
}

fn slow_water_asked(wording: Wording) -> RecordingAsked {
    RecordingAsked {
        title: "Lantern".to_owned(),
        artist: Some("Example Band".to_owned()),
        artist_mbid: None,
        release: None,
        length: Some(Duration::from_secs(1_410)),
        wording,
    }
}

fn slow_water_asked_at_any_length(wording: Wording) -> RecordingAsked {
    RecordingAsked {
        length: None,
        ..slow_water_asked(wording)
    }
}

fn slow_water_lasting(seconds: u64) -> RecordingMatch {
    RecordingMatch {
        length: Some(Duration::from_secs(seconds)),
        ..slow_water_found()
    }
}

#[test]
fn a_music_video_running_past_its_song_is_followed_to_the_take_it_plays() -> Result<()> {
    let library = Library::open_in_memory()?;
    let mut live = slow_water_lasting(1_402);

    live.recording = mbid(HOURS_BONUS);
    live.title = "Lantern (live)".to_owned();

    let mut demo = slow_water_lasting(900);

    demo.recording = mbid(DAYBREAK);

    let video = Fake::new(Canned {
        linked: Some(titled_on_the_page("Lantern", "Example Band", 1_410)),
        found_recordings: vec![
            demo,
            slow_water_lasting(1_330),
            live,
            slow_water_lasting(1_500),
        ],
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    });
    let page_of_a_song = Fake::new(Canned {
        linked: Some(titled_on_the_page("Lantern", "Example Band", 1_410)),
        found_recordings: vec![slow_water_lasting(1_330)],
        ..Canned::default()
    });

    let Linked::Found(found) = library.follow_link(&video, &linked_to_youtube())? else {
        panic!("the video named no song to want");
    };

    assert_eq!(found.recording, mbid(SLOW_WATER));
    assert_eq!(
        video.calls(),
        vec![
            Called::SongLinked(linked_to_youtube()),
            Called::FindRecording(slow_water_asked(Wording::Phrase)),
            Called::FindRecording(slow_water_asked(Wording::Words)),
            Called::FindRecording(slow_water_asked_at_any_length(Wording::Phrase)),
            Called::Recording(mbid(SLOW_WATER)),
        ],
        "the length the video ran bounded the take from above alone"
    );
    assert_eq!(
        library.follow_link(&page_of_a_song, &linked_to_youtube_music())?,
        Linked::Unnamed,
        "a page of the song itself still holds a take to its own length"
    );
    Ok(())
}

#[test]
fn a_song_link_naming_no_isrc_is_followed_by_its_title_and_artist_under_the_strict_rule()
-> Result<()> {
    let library = Library::open_in_memory()?;
    let fake = Fake::new(Canned {
        linked: Some(titled_on_the_page(
            "Example Band - Lantern (Official Audio)",
            "Example Band - Topic",
            1_410,
        )),
        found_recordings: vec![slow_water_found()],
        recordings: vec![slow_water_found().into_recording()],
        ..Canned::default()
    });

    let Linked::Found(found) = library.follow_link(&fake, &linked_to_youtube())? else {
        panic!("the link's title and artist named no song to want");
    };

    assert_eq!(found.recording, mbid(SLOW_WATER));
    assert_eq!(
        found.release.as_ref().map(|release| release.id.clone()),
        Some(mbid(LOW_TIDE))
    );
    assert_eq!(
        fake.calls(),
        vec![
            Called::SongLinked(linked_to_youtube()),
            Called::FindRecording(slow_water_asked(Wording::Phrase)),
            Called::Recording(mbid(SLOW_WATER)),
        ],
        "the upload's billing was read as the song by the artist and the video's words left out"
    );
    Ok(())
}

#[test]
fn a_song_link_whose_title_finds_only_a_near_miss_still_names_nothing() -> Result<()> {
    let library = Library::open_in_memory()?;
    let mut by_another = slow_water_found();

    by_another.credit = credited(Some("The Orbiters"), None);

    let longer = Fake::new(Canned {
        linked: Some(titled_on_the_page("Lantern", "Example Band", 1_410)),
        found_recordings: vec![RecordingMatch {
            length: Some(Duration::from_secs(1_500)),
            ..slow_water_found()
        }],
        found_recordings_in_words: vec![by_another],
        ..Canned::default()
    });
    let nameless = Fake::new(Canned {
        linked: Some(LinkNames {
            artist: None,
            ..titled_on_the_page("Lantern", "Example Band", 1_410)
        }),
        found_recordings: vec![slow_water_found()],
        ..Canned::default()
    });

    assert_eq!(
        library.follow_link(&longer, &linked_to_youtube())?,
        Linked::Unnamed
    );
    assert_eq!(
        longer.calls(),
        vec![
            Called::SongLinked(linked_to_youtube()),
            Called::FindRecording(slow_water_asked(Wording::Phrase)),
            Called::FindRecording(slow_water_asked(Wording::Words)),
            Called::FindRecording(slow_water_asked_at_any_length(Wording::Phrase)),
            Called::FindRecording(slow_water_asked_at_any_length(Wording::Words)),
        ],
        "a take longer than the video it was filmed for is no take of it"
    );
    assert_eq!(
        library.follow_link(&nameless, &linked_to_youtube())?,
        Linked::Unnamed
    );
    assert_eq!(
        nameless.called(LookupOp::FindRecording),
        0,
        "a bare title was searched for"
    );
    Ok(())
}

const A_SPOTIFY_ALBUM: &str = "https://open.spotify.com/album/6N9PS4QXF1D0OWPk0Sxtb4";

const ON_THE_PAGE: &str = "035627515026";

const ON_DEEZER: &str = "859381157694";

fn linked_to_an_album() -> AlbumLink {
    AlbumLink::read(A_SPOTIFY_ALBUM).expect("an album link")
}

fn hours_barcoded(code: &str) -> BarcodeMatch {
    BarcodeMatch {
        release: mbid(HOURS),
        group: Some(mbid(HOURS_GROUP)),
        barcode: Some(code.to_owned()),
        title: "Hours".to_owned(),
        credit: credited(Some("The Orbiters"), Some(ORBITERS)),
    }
}

#[test]
fn a_link_to_an_album_is_followed_by_its_barcode_to_the_release_group_to_want() -> Result<()> {
    let library = Library::open_in_memory()?;
    let fake = Fake::new(Canned {
        album_linked: Some(AlbumNames {
            barcodes: vec![barcode(ON_THE_PAGE), barcode(ON_DEEZER)],
        }),
        barcoded: vec![hours_barcoded("0859381157694")],
        ..learnt_canned()
    });

    let followed = library.follow_album_link(&fake, &linked_to_an_album())?;

    assert_eq!(
        followed,
        Linked::Album {
            group: mbid(HOURS_GROUP),
            release: Some(mbid(HOURS)),
            title: "Hours".to_owned(),
            artist: Some("The Orbiters".to_owned()),
        }
    );
    assert_eq!(
        fake.calls(),
        vec![
            Called::AlbumLinked(linked_to_an_album()),
            Called::ByBarcode(barcode(ON_THE_PAGE)),
            Called::ByBarcode(barcode(ON_DEEZER)),
        ],
        "each code was asked in turn until one named a release, a leading zero apart"
    );

    let wanted = library.want_album(&fake, &mbid(HOURS_GROUP), Some(&mbid(HOURS)))?;

    assert_eq!(wanted.len(), 2);
    assert_eq!(library.wants()?.len(), 2);
    Ok(())
}

#[test]
fn an_album_linked_by_its_reissue_is_wanted_from_the_reissue_and_not_the_usual_pressing()
-> Result<()> {
    let library = Library::open_in_memory()?;
    let mut canned = learnt_canned();

    canned.album_linked = Some(AlbumNames {
        barcodes: vec![barcode(ON_THE_PAGE)],
    });
    canned.barcoded = vec![BarcodeMatch {
        release: mbid(HOURS_REISSUE),
        ..hours_barcoded(ON_THE_PAGE)
    }];
    canned.releases.push(hours_pressed(
        HOURS_REISSUE,
        "2011",
        &[
            ("Daybreak", DAYBREAK),
            ("Paper Boats", HOURS_PAPER_BOATS),
            ("Daybreak (demo)", HOURS_BONUS),
        ],
    ));

    let fake = Fake::new(canned);

    let Linked::Album { group, release, .. } =
        library.follow_album_link(&fake, &linked_to_an_album())?
    else {
        panic!("the link names an album");
    };

    let wanted = library.want_album(&fake, &group, release.as_ref())?;

    let songs: Vec<Mbid> = wanted
        .iter()
        .map(|(song, _)| song.recording.clone())
        .collect();

    assert_eq!(release, Some(mbid(HOURS_REISSUE)));
    assert_eq!(
        songs,
        [mbid(DAYBREAK), mbid(HOURS_PAPER_BOATS), mbid(HOURS_BONUS)]
    );
    assert_eq!(library.wants()?.len(), 3);
    assert_eq!(
        fake.called(LookupOp::ReleasesOfGroup),
        0,
        "the group's usual pressing was read though the link named one"
    );
    assert_eq!(
        fake.called(LookupOp::Release),
        1,
        "the pressing the link named was asked for once"
    );
    Ok(())
}

#[test]
fn a_link_to_an_artist_opens_the_artist_held_or_the_one_musicbrainz_names_exactly() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let on_deezer = ArtistLink::Deezer(7_307_038);
    let held = Fake::new(Canned {
        artist_linked: Some("the orbiters".to_owned()),
        ..Canned::default()
    });
    let elsewhere = Fake::new(Canned {
        artist_linked: Some("Rick Astley".to_owned()),
        found_artists: vec![ArtistMatch {
            mbid: mbid(SLOW_WATER),
            name: "Rick Astley".to_owned(),
            score: 100,
            kind: None,
            disambiguation: None,
            aliases: Vec::new(),
        }],
        ..Canned::default()
    });
    let nobody = Fake::new(Canned::default());

    let Linked::HeldArtist { artist, name } = library.follow_artist_link(&held, &on_deezer)? else {
        panic!("the artist the catalog holds was not named");
    };

    assert_eq!(artist, artist_named(&library, "The Orbiters")?.id);
    assert_eq!(name, "The Orbiters");
    assert_eq!(held.called(LookupOp::FindArtist), 0);
    assert_eq!(
        library.follow_artist_link(&elsewhere, &on_deezer)?,
        Linked::Artist(ArtistFound {
            mbid: mbid(SLOW_WATER),
            name: "Rick Astley".to_owned(),
        })
    );
    assert_eq!(
        library.follow_artist_link(&nobody, &on_deezer)?,
        Linked::Unnamed
    );
    assert_eq!(
        library.follow_artist_link(&nobody, &ArtistLink::MusicBrainz(mbid(SLOW_WATER)))?,
        Linked::Unnamed
    );
    Ok(())
}

#[test]
fn an_artist_link_on_spotify_is_followed_through_musicbrainz_to_the_artist_it_names() -> Result<()>
{
    const PAGE: &str = "https://open.spotify.com/artist/3YQKmKGau1PzlVlkL1iodx";

    let library = Library::open_in_memory()?;
    let link = ArtistLink::read(&format!("{PAGE}?si=shared")).expect("an artist link");
    let named = Fake::new(Canned {
        artists_at: vec![(PAGE.to_owned(), mbid(ORBITERS))],
        artists: vec![orbiters()],
        ..Canned::default()
    });
    let nobody = Fake::new(Canned::default());
    let on_deezer = Fake::new(Canned {
        artists_at: vec![(
            "https://www.deezer.com/artist/7307038".to_owned(),
            mbid(ORBITERS),
        )],
        artists: vec![orbiters()],
        artist_linked: Some("Somebody Else".to_owned()),
        ..Canned::default()
    });

    assert_eq!(
        library.follow_artist_link(&named, &link)?,
        Linked::Artist(ArtistFound {
            mbid: mbid(ORBITERS),
            name: "The Orbiters".to_owned(),
        })
    );
    assert_eq!(library.follow_artist_link(&nobody, &link)?, Linked::Unnamed);
    assert_eq!(nobody.called(LookupOp::FindArtist), 0);
    assert_eq!(
        library.follow_artist_link(&on_deezer, &ArtistLink::Deezer(7_307_038))?,
        Linked::Artist(ArtistFound {
            mbid: mbid(ORBITERS),
            name: "The Orbiters".to_owned(),
        })
    );
    assert!(
        !on_deezer
            .calls()
            .iter()
            .any(|called| matches!(called, Called::ArtistLinked(_))),
        "a Deezer artist MusicBrainz files was still asked for by name"
    );
    Ok(())
}

#[test]
fn an_album_link_whose_codes_name_no_release_or_several_groups_names_nothing() -> Result<()> {
    let library = Library::open_in_memory()?;
    let unnamed = Fake::new(Canned::default());
    let unknown = Fake::new(Canned {
        album_linked: Some(AlbumNames {
            barcodes: vec![barcode(ON_THE_PAGE)],
        }),
        barcoded: vec![hours_barcoded(ON_DEEZER)],
        ..Canned::default()
    });
    let shared = Fake::new(Canned {
        album_linked: Some(AlbumNames {
            barcodes: vec![barcode(ON_THE_PAGE)],
        }),
        barcoded: vec![
            hours_barcoded(ON_THE_PAGE),
            BarcodeMatch {
                group: Some(mbid(RELEASE_GROUP)),
                ..hours_barcoded(ON_THE_PAGE)
            },
        ],
        ..Canned::default()
    });

    for fake in [&unnamed, &unknown, &shared] {
        assert_eq!(
            library.follow_album_link(fake, &linked_to_an_album())?,
            Linked::Unnamed
        );
    }

    assert_eq!(
        unnamed.calls(),
        vec![Called::AlbumLinked(linked_to_an_album())]
    );
    Ok(())
}

#[test]
fn a_link_to_an_album_the_library_holds_answers_the_album_and_how_many_songs_it_lacks() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Daybreak", Vec::new()));

    let mut release = orbits(rows, Vec::new());

    release.credit = credited(Some("The Orbiters"), None);

    let identifying = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release.clone()],
        ..Canned::default()
    }));

    enrich(&library, &identifying, false)?;

    let album = library.albums(&AlbumQuery::default())?.remove(0);
    let fake = Fake::new(Canned {
        groups: vec![orbits_group(Vec::new(), Vec::new())],
        releases: vec![release],
        ..Canned::default()
    });

    let by_group = library.follow_album_link(&fake, &AlbumLink::Group(mbid(RELEASE_GROUP)))?;
    let by_release = library.follow_album_link(&fake, &AlbumLink::Release(mbid(RELEASE)))?;

    let held = Linked::HeldAlbum {
        album: album.id,
        title: "Orbits".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        missing: 1,
    };

    assert_eq!(by_group, held);
    assert_eq!(by_release, held);
    assert_eq!(
        fake.calls(),
        vec![
            Called::ReleaseGroup(mbid(RELEASE_GROUP)),
            Called::Release(mbid(RELEASE)),
        ]
    );
    Ok(())
}

#[test]
fn a_song_with_no_release_named_is_wanted_from_the_one_its_recording_first_came_out_on()
-> Result<()> {
    let library = Library::open_in_memory()?;
    let mut matched = slow_water_found();

    matched.releases.clear();

    let mut recorded = matched.clone().into_recording();

    recorded.releases = slow_water_found().releases;

    let fake = Fake::new(Canned {
        found_songs: vec![matched],
        recordings: vec![recorded],
        releases: vec![low_tide_release()],
        ..Canned::default()
    });

    let found = library.found_elsewhere(&fake, "lantern")?;

    assert_eq!(found[0].release, None);

    library.want_found(&fake, &found[0])?;

    assert_eq!(library.wants()?[0].album_title, "Tideline");

    let mut lost = found[0].clone();

    lost.recording = mbid(RECORDING);

    assert!(matches!(
        library.want_found(&fake, &lost),
        Err(Error::Unreleased { .. })
    ));

    lost.release = Some(RecordingRelease {
        id: mbid(LOW_TIDE),
        title: "Tideline".to_owned(),
        date: None,
        disc: None,
        position: None,
        issued: Issued::default(),
    });

    assert!(matches!(
        library.want_found(&fake, &lost),
        Err(Error::NotOnTheRelease { .. })
    ));
    Ok(())
}
