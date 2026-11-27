use std::{sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_core::ReleaseTrackId;
use resonate_library::{
    ArtistPressings, Credit, EnrichOptions, Error, Fingerprinters, Learning, Library, LookupOp,
    Mbid, Missing, MissingTrack, Performer, Reference, Release, Result,
};

use crate::support::{
    Called, Canned, DAYBREAK, Fake, Fault, HOURS, HOURS_GROUP, ORBITERS, RELEASE, SCORE_GROUP,
    SINGLE_GROUP, artist_named, artist_release, asked_in_order, enrich, hours_pressed,
    hours_pressings, learnt_canned, mbid, only_album, orbiters, orbiters_groups, orbits,
    orbits_asked, orbits_match, orbits_rows, png_art, release_row, scanned_orbits, unheld,
    waited_for,
};

const SCORE_PRESSING: &str = "5f6a7b8c-9d0e-4fa1-9bc2-4d5e6f7a8b9c";

const SINGLE_PRESSING: &str = "6a7b8c9d-0e1f-4ab2-8cd3-5e6f7a8b9c0d";

const ORBIT_THEME: &str = "7b8c9d0e-1f2a-4bc3-9de4-6f7a8b9c0d1e";

const HARBOUR_LIGHTS_SONG: &str = "8c9d0e1f-2a3b-4cd4-8ef5-7a8b9c0d1e2f";

const NIGHTFALL_GROUP: &str = "6a7b8c9d-aebf-4ac1-8cd3-5e6f7a8b9cad";

#[test]
fn an_artists_discography_is_kept_once_its_profile_lands_and_the_catalog_says_what_it_does_not_hold()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let canned = || Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release.clone()],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        ..Canned::default()
    };
    let fake = Arc::new(Fake::new(canned()));
    let summary = enrich(&library, &fake, false)?;

    assert_eq!(
        asked_in_order(&fake),
        vec![
            Called::FindRelease(orbits_asked()),
            Called::Release(mbid(RELEASE)),
            Called::Artist(mbid(ORBITERS)),
            Called::ReleaseGroupsOf(mbid(ORBITERS)),
            Called::ReleasesOfArtist(mbid(ORBITERS), 0),
            Called::ReleasesOfGroup(mbid(HOURS_GROUP)),
            Called::ReleasesOfGroup(mbid(SCORE_GROUP)),
            Called::ReleasesOfGroup(mbid(SINGLE_GROUP)),
        ]
    );
    assert_eq!(summary.stats.releases_found, 4);

    let artist = artist_named(&library, "The Orbiters")?;
    let expected = vec![
        unheld(&artist, HOURS_GROUP, "Hours", "EP", Some("1996-05-06")),
        unheld(&artist, SCORE_GROUP, "The Orbit", "Album", Some("2003-11")),
        unheld(&artist, SINGLE_GROUP, "Harbour Lights", "Single", None),
    ];

    assert_eq!(library.unheld_releases(None, None)?, expected);
    assert_eq!(library.unheld_releases(None, Some(1))?, expected[..1]);
    assert_eq!(
        library
            .artist_detail(artist.id)?
            .map(|detail| detail.releases_unheld),
        Some(3)
    );
    assert_eq!(
        library.missing_counted(None)?,
        Missing {
            tracks: 0,
            releases: 3,
        }
    );

    let fake = Arc::new(Fake::new(canned()));
    let summary = enrich(&library, &fake, true)?;

    assert_eq!(fake.called(LookupOp::ReleaseGroupsOfArtist), 1);
    assert_eq!(summary.stats.releases_found, 4);
    assert_eq!(library.unheld_releases(None, None)?, expected);
    assert_eq!(
        library
            .artist_detail(artist.id)?
            .map(|detail| detail.releases_unheld),
        Some(3)
    );
    Ok(())
}

#[test]
fn the_songs_of_releases_not_held_are_learnt_in_the_lookup_and_found_without_asking() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(learnt_canned()));

    let summary = enrich(&library, &fake, false)?;
    let artist = artist_named(&library, "The Orbiters")?;
    let daybreak = library.songs_kept_for("dayb")?;
    let by_release = library.songs_kept_for("orbiters hours")?;
    let not_held = library.songs_not_held_by(artist.id)?;
    let albums = library.albums_not_held_by(artist.id)?;

    assert_eq!(summary.stats.songs, 2);
    assert_eq!(daybreak.len(), 1);
    assert_eq!(daybreak[0].recording, mbid(DAYBREAK));
    assert_eq!(daybreak[0].title, "Daybreak");
    assert_eq!(daybreak[0].artist, "The Orbiters");
    assert_eq!(daybreak[0].performer, Some(Performer::Held(artist.id)));
    assert_eq!(
        daybreak[0]
            .release
            .as_ref()
            .map(|release| (release.id.clone(), release.title.as_str())),
        Some((mbid(HOURS), "Hours"))
    );
    assert_eq!(by_release, daybreak);
    assert!(
        library.songs_kept_for("paper boats")?.is_empty(),
        "a song held by its title was offered"
    );
    assert_eq!(not_held, daybreak);
    assert_eq!(
        albums
            .iter()
            .map(|album| (album.release.mbid.clone(), album.pressing.clone()))
            .collect::<Vec<_>>(),
        vec![
            (mbid(HOURS_GROUP), Some(mbid(HOURS))),
            (mbid(SCORE_GROUP), None),
            (mbid(SINGLE_GROUP), None),
        ]
    );

    let again = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &again, false)?;

    assert_eq!(again.called(LookupOp::ReleasesOfGroup), 0);
    Ok(())
}

#[test]
fn the_albums_of_a_library_artist_not_held_answer_a_search_without_asking() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &fake, false)?;

    let kept = |text| -> Result<Vec<(Mbid, String, String)>> {
        Ok(library
            .albums_kept_for(text)?
            .into_iter()
            .map(|album| (album.group, album.title, album.artist))
            .collect())
    };

    assert_eq!(
        kept("orbit")?,
        [
            (
                mbid(HOURS_GROUP),
                "Hours".to_owned(),
                "The Orbiters".to_owned()
            ),
            (
                mbid(SCORE_GROUP),
                "The Orbit".to_owned(),
                "The Orbiters".to_owned()
            ),
        ],
        "an artist's word answers each album and EP of theirs nobody holds"
    );
    assert_eq!(kept("orbiters hours")?.len(), 1);
    assert!(kept("orbits")?.is_empty(), "a held album was offered");
    assert!(
        kept("lights")?.is_empty(),
        "a single was offered as an album"
    );
    assert!(kept("year:1996")?.is_empty(), "a term was read as a word");
    Ok(())
}

#[test]
fn an_artists_page_reads_the_songs_of_its_releases_not_held_before_the_lookup_reaches_them()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let cut_short = Arc::new(Fake::new(learnt_canned()).faulting(
        LookupOp::ReleasesOfGroup,
        0,
        Fault::Unreachable,
    ));

    let _ = enrich(&library, &cut_short, false);
    let artist = artist_named(&library, "The Orbiters")?;

    assert!(
        library.songs_not_held_by(artist.id)?.is_empty(),
        "the songs were read before the page asked"
    );

    let page = Fake::new(learnt_canned());
    let landed = library.learn_the_songs_of_artist(&page, artist.id, &PageTold::default())?;
    let songs: Vec<Mbid> = library
        .songs_not_held_by(artist.id)?
        .into_iter()
        .map(|song| song.recording)
        .collect();

    assert_eq!(landed, 2);
    assert_eq!(songs, [mbid(DAYBREAK)]);
    assert_eq!(page.called(LookupOp::ReleasesOfGroup), 3);
    assert_eq!(
        library.learn_the_songs_of_artist(&page, artist.id, &PageTold::default())?,
        0
    );
    assert_eq!(
        page.called(LookupOp::ReleasesOfGroup),
        3,
        "a group read was asked about again"
    );
    Ok(())
}

#[test]
fn a_group_an_artists_page_read_while_the_lookup_ran_is_not_asked_for_again() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let (fake, has_started, go) =
        Fake::new(learnt_canned()).gated_on(Some(LookupOp::ReleasesOfGroup));
    let fake = Arc::new(fake);

    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started
        .recv()
        .expect("the lookup reaches the songs of a group");

    let artist = artist_named(&library, "The Orbiters")?;
    let page = Fake::new(learnt_canned());

    library.learn_the_songs_of_artist(&page, artist.id, &PageTold::default())?;
    go.send(()).expect("the lookup is still waiting");
    handle.join()?;

    assert_eq!(page.called(LookupOp::ReleasesOfGroup), 3);
    assert_eq!(
        fake.called(LookupOp::ReleasesOfGroup),
        1,
        "the lookup asked again for the songs the page had read"
    );
    Ok(())
}

fn pressed_for(group: &str, id: &str, title: &str, song: (&str, &str)) -> Release {
    Release {
        group: Some(mbid(group)),
        title: title.to_owned(),
        ..hours_pressed(id, "2003-11-01", &[song])
    }
}

fn score_and_single() -> Vec<Release> {
    vec![
        pressed_for(
            SCORE_GROUP,
            SCORE_PRESSING,
            "The Orbit",
            ("Orbit Theme", ORBIT_THEME),
        ),
        pressed_for(
            SINGLE_GROUP,
            SINGLE_PRESSING,
            "Harbour Lights",
            ("Harbour Lights", HARBOUR_LIGHTS_SONG),
        ),
    ]
}

fn browsed_in(pages: Vec<ArtistPressings>) -> Canned {
    Canned {
        artist_pressings: vec![(mbid(ORBITERS), pages)],
        ..learnt_canned()
    }
}

#[test]
fn an_artists_songs_are_read_off_one_browse_of_their_releases_rather_than_a_request_a_group()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(browsed_in(vec![
        ArtistPressings {
            pressings: hours_pressings(),
            credited: 4,
            read_to: 2,
        },
        ArtistPressings {
            pressings: score_and_single(),
            credited: 4,
            read_to: 4,
        },
    ])));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::ReleasesOfArtist), 2);
    assert_eq!(fake.called(LookupOp::ReleasesOfGroup), 0);
    assert_eq!(library.songs_kept_for("daybreak")?.len(), 1);
    assert_eq!(library.songs_kept_for("orbit theme")?.len(), 1);
    assert_eq!(library.songs_kept_for("harbour lights")?.len(), 1);
    Ok(())
}

#[test]
fn a_group_the_browse_never_reached_is_read_on_its_own() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(browsed_in(vec![ArtistPressings {
        pressings: hours_pressings(),
        credited: 2,
        read_to: 2,
    }])));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::ReleasesOfArtist), 1);
    assert_eq!(fake.called(LookupOp::ReleasesOfGroup), 2);
    assert_eq!(library.songs_kept_for("daybreak")?.len(), 1);
    Ok(())
}

#[test]
fn a_browse_longer_than_the_groups_it_would_save_is_left_for_the_groups_themselves() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(browsed_in(vec![ArtistPressings {
        pressings: hours_pressings(),
        credited: 100,
        read_to: 2,
    }])));

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::ReleasesOfArtist), 1);
    assert_eq!(fake.called(LookupOp::ReleasesOfGroup), 2);
    assert_eq!(library.songs_kept_for("daybreak")?.len(), 1);
    Ok(())
}

#[test]
fn an_artist_with_few_groups_due_is_asked_for_each_group_alone() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let cut_short = Arc::new(Fake::new(learnt_canned()).faulting(
        LookupOp::ReleasesOfGroup,
        1,
        Fault::Unreachable,
    ));

    let _ = enrich(&library, &cut_short, false);
    let artist = artist_named(&library, "The Orbiters")?;

    let page = Fake::new(browsed_in(vec![ArtistPressings {
        pressings: score_and_single(),
        credited: 2,
        read_to: 2,
    }]));

    library.learn_the_songs_of_artist(&page, artist.id, &PageTold::default())?;

    assert_eq!(page.called(LookupOp::ReleasesOfArtist), 0);
    assert_eq!(page.called(LookupOp::ReleasesOfGroup), 2);
    Ok(())
}

#[test]
fn a_browse_refused_falls_back_to_reading_each_group() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(
        Fake::new(browsed_in(vec![ArtistPressings {
            pressings: hours_pressings(),
            credited: 2,
            read_to: 2,
        }]))
        .faulting(LookupOp::ReleasesOfArtist, 0, Fault::Refused),
    );

    enrich(&library, &fake, false)?;

    assert_eq!(fake.called(LookupOp::ReleasesOfArtist), 1);
    assert_eq!(fake.called(LookupOp::ReleasesOfGroup), 3);
    assert_eq!(library.songs_kept_for("daybreak")?.len(), 1);
    Ok(())
}

#[derive(Default)]
struct PageTold {
    batches: Mutex<Vec<usize>>,
    leaves_after: Option<usize>,
}

impl Learning for PageTold {
    fn abandoned(&self) -> bool {
        self.leaves_after
            .is_some_and(|after| self.batches.lock().len() >= after)
    }

    fn landed(&self, songs: usize) {
        self.batches.lock().push(songs);
    }
}

#[test]
fn an_artists_page_is_told_of_each_batch_of_songs_as_it_lands() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = scanned_artist_with_its_discography(&library)?;
    let page = Fake::new(learnt_canned());
    let told = PageTold::default();

    let landed = library.learn_the_songs_of_artist(&page, artist, &told)?;

    let batches = told.batches.lock().clone();

    assert_eq!(batches.len(), 3, "{batches:?}");
    assert_eq!(batches.iter().sum::<usize>(), landed);
    assert_eq!(batches[0], 2, "the earliest group was not told first");
    Ok(())
}

#[test]
fn an_artists_page_left_asks_nothing_more() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = scanned_artist_with_its_discography(&library)?;
    let page = Fake::new(learnt_canned());
    let told = PageTold {
        leaves_after: Some(1),
        ..PageTold::default()
    };

    library.learn_the_songs_of_artist(&page, artist, &told)?;

    assert_eq!(page.called(LookupOp::ReleasesOfGroup), 1);
    assert_eq!(told.batches.lock().len(), 1);
    Ok(())
}

fn scanned_artist_with_its_discography(library: &Library) -> Result<resonate_core::ArtistId> {
    let cut_short = Arc::new(Fake::new(learnt_canned()).faulting(
        LookupOp::ReleasesOfArtist,
        0,
        Fault::Unreachable,
    ));
    let _ = enrich(library, &cut_short, false);
    Ok(artist_named(library, "The Orbiters")?.id)
}

#[test]
fn a_release_group_refused_waits_before_its_songs_are_asked_for_again() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake =
        Arc::new(Fake::new(learnt_canned()).faulting(LookupOp::ReleasesOfGroup, 0, Fault::Refused));

    enrich(&library, &fake, false)?;

    let again = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &again, false)?;

    assert!(library.songs_kept_for("daybreak")?.is_empty());
    assert_eq!(again.called(LookupOp::ReleasesOfGroup), 0);
    Ok(())
}

#[test]
fn the_sleeve_of_a_release_not_held_is_asked_for_as_soon_as_its_songs_land() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut canned = learnt_canned();

    canned.covers.push((mbid(HOURS), png_art(8)));

    let (fake, has_started, go) =
        Fake::new(canned).gated_on_the(Some(LookupOp::ReleasesOfGroup), 1);
    let fake = Arc::new(fake);
    let handle = library.enrich(
        Arc::clone(&fake) as Arc<dyn Reference>,
        Arc::new(Fingerprinters::none()),
        EnrichOptions::default(),
    )?;

    has_started
        .recv()
        .expect("the lookup reaches the songs of a second group");

    let asked_while_held = waited_for(|| fake.called(LookupOp::Cover) >= 1);

    go.send(()).expect("the lookup is still waiting");
    handle.join()?;

    assert!(
        asked_while_held,
        "the sleeve waited for every group's songs: {:?}",
        fake.calls()
    );
    assert_eq!(library.unheld_cover(&mbid(HOURS_GROUP))?, Some(png_art(8)));
    Ok(())
}

#[test]
fn the_covers_of_releases_not_held_are_kept_in_the_lookup_and_not_asked_for_again() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut canned = learnt_canned();

    canned.covers.push((mbid(HOURS), png_art(8)));
    canned.group_covers.push((mbid(SCORE_GROUP), png_art(16)));

    let fake = Arc::new(Fake::new(canned));

    enrich(&library, &fake, false)?;

    assert_eq!(
        library.unheld_cover(&mbid(HOURS_GROUP))?,
        Some(png_art(8)),
        "the pressing the songs were read off was not the sleeve kept"
    );
    assert_eq!(
        library.unheld_cover(&mbid(SCORE_GROUP))?,
        Some(png_art(16)),
        "a group with no pressing read was not asked for its own cover"
    );
    assert_eq!(library.unheld_cover(&mbid(SINGLE_GROUP))?, None);

    let again = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &again, false)?;

    assert_eq!(again.called(LookupOp::Cover), 0);
    Ok(())
}

#[test]
fn an_album_not_held_is_wanted_whole_from_the_pressing_its_songs_were_read_off() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &fake, false)?;

    let artist = artist_named(&library, "The Orbiters")?;

    let wanted = library.want_album(fake.as_ref(), &mbid(HOURS_GROUP), None)?;

    assert_eq!(
        wanted
            .iter()
            .map(|(song, _)| song.recording.clone())
            .collect::<Vec<_>>(),
        vec![mbid(DAYBREAK)]
    );
    assert_eq!(library.wants()?.len(), 1);
    assert!(
        library
            .albums_not_held_by(artist.id)?
            .iter()
            .any(|album| album.release.mbid == mbid(HOURS_GROUP)),
        "an album wanted whole left the page before a file of it arrived"
    );
    assert!(library.songs_kept_for("daybreak")?.is_empty());
    assert_eq!(
        library
            .songs_not_held_by(artist.id)?
            .iter()
            .map(|song| song.recording.clone())
            .collect::<Vec<_>>(),
        vec![mbid(DAYBREAK)],
        "a wanted song left the artist's page before it arrived"
    );
    Ok(())
}

#[test]
fn an_album_not_held_is_opened_as_an_album_with_its_songs_listed_and_none_wanted() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(learnt_canned()));

    enrich(&library, &fake, false)?;

    let opened = library.open_album_uncovered(fake.as_ref(), &mbid(HOURS_GROUP))?;

    assert!(library.wants()?.is_empty());
    assert!(
        !library.release_tracks(opened.wanted)?.is_empty(),
        "an album opened carried no songs to press"
    );
    Ok(())
}

#[test]
fn an_album_whose_songs_were_never_read_has_them_asked_for_when_it_is_wanted() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(learnt_canned()));

    let wanted = library.want_album(fake.as_ref(), &mbid(HOURS_GROUP), None)?;

    assert_eq!(wanted.len(), 2);
    assert_eq!(fake.called(LookupOp::ReleasesOfGroup), 1);
    Ok(())
}

#[test]
fn a_single_is_not_held_only_where_its_song_is_not_and_a_discography_says_what_it_did_not_read()
-> Result<()> {
    const PAPER_BOATS_SINGLE: &str = "4a5b6c7d-8e9f-4a0b-9c1d-2e3f4a5b6c7d";
    const SATELLITE_SINGLE: &str = "5b6c7d8e-9f0a-4b1c-8d2e-3f4a5b6c7d8e";

    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let mut groups = orbiters_groups();

    groups.push(artist_release(
        PAPER_BOATS_SINGLE,
        "Paper Boats",
        Some("Single"),
        &[],
        Some("1998-01-01"),
    ));
    groups.push(artist_release(
        SATELLITE_SINGLE,
        "Satellite",
        Some("Single"),
        &[],
        Some("2004-01-01"),
    ));

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), groups)],
        releases_unread: 250,
        further_releases: vec![artist_release(
            NIGHTFALL_GROUP,
            "Nightfall",
            Some("Album"),
            &[],
            Some("2011-01-01"),
        )],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let artist = artist_named(&library, "The Orbiters")?;
    let unheld: Vec<String> = library
        .unheld_releases(None, None)?
        .into_iter()
        .map(|release| release.title)
        .collect();

    assert!(
        unheld.contains(&"Satellite".to_owned()),
        "a single whose song is not held was not listed: {unheld:?}"
    );
    assert!(
        !unheld.contains(&"Paper Boats".to_owned()),
        "a single whose song is held on the album was listed: {unheld:?}"
    );

    let detail = library.artist_detail(artist.id)?.expect("the artist");

    assert_eq!(detail.releases_unheld, 4);
    assert_eq!(detail.releases_unread, 250);

    assert_eq!(library.read_the_rest_of(artist.id, fake.as_ref())?, 1);

    let unheld: Vec<String> = library
        .unheld_releases(None, None)?
        .into_iter()
        .map(|release| release.title)
        .collect();

    assert!(unheld.contains(&"Nightfall".to_owned()), "{unheld:?}");
    assert!(
        unheld.contains(&"Satellite".to_owned()),
        "reading the rest took away what the first read kept: {unheld:?}"
    );

    let detail = library.artist_detail(artist.id)?.expect("the artist");

    assert_eq!(detail.releases_unheld, 5);
    assert_eq!(detail.releases_unread, 0);
    assert_eq!(
        library.read_the_rest_of(artist.id, fake.as_ref())?,
        0,
        "a discography read to its end was read again"
    );
    Ok(())
}

#[test]
fn what_the_catalog_is_short_of_is_narrowed_by_the_words_typed() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let artist = artist_named(&library, "The Orbiters")?;
    let hours = unheld(&artist, HOURS_GROUP, "Hours", "EP", Some("1996-05-06"));
    let orbit = unheld(&artist, SCORE_GROUP, "The Orbit", "Album", Some("2003-11"));

    assert_eq!(
        library.unheld_releases(Some("hours"), None)?,
        vec![hours.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("ep"), None)?,
        vec![hours.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("2003"), None)?,
        vec![orbit.clone()]
    );
    assert_eq!(
        library.unheld_releases(Some("orbiters album"), None)?,
        vec![orbit.clone()]
    );
    assert!(
        library
            .unheld_releases(Some("nothing of the sort"), None)?
            .is_empty()
    );
    assert_eq!(
        library.missing_counted(Some("hours"))?,
        Missing {
            tracks: 0,
            releases: 1,
        }
    );

    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    assert_eq!(library.rematch(album.id)?, 3);

    assert_eq!(library.missing_tracks(Some("orbits"), None)?.len(), 1);
    assert!(library.missing_tracks(Some("hours"), None)?.is_empty());
    assert_eq!(
        library.missing_counted(Some("orbits"))?,
        Missing {
            tracks: 1,
            releases: 0,
        }
    );
    Ok(())
}

#[test]
fn an_unheld_release_is_found_by_a_name_spelt_either_way() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = artist_named(&library, "The Orbiters")?;

    library.land_artist_releases(
        artist.id,
        &[artist_release(
            HOURS_GROUP,
            "Hōrs Przybyłowicz",
            Some("EP"),
            &[],
            Some("1996-05-06"),
        )],
        (0, 1),
    )?;

    assert_eq!(
        library.unheld_releases(Some("przybylowicz"), None)?.len(),
        1
    );
    assert_eq!(library.unheld_releases(Some("HORS"), None)?.len(), 1);
    Ok(())
}

#[test]
fn an_unheld_release_is_found_by_a_piece_of_a_word_whatever_its_length_or_punctuation() -> Result<()>
{
    let (_tree, library) = scanned_orbits()?;
    let artist = artist_named(&library, "The Orbiters")?;

    library.land_artist_releases(
        artist.id,
        &[artist_release(
            HOURS_GROUP,
            "100% a_b\\c",
            Some("EP"),
            &[],
            Some("1996-05-06"),
        )],
        (0, 1),
    )?;

    for piece in ["100", "00%", "a_b", "b\\c", "orbit", "rbite", "rb"] {
        assert_eq!(
            library.unheld_releases(Some(piece), None)?.len(),
            1,
            "{piece}"
        );
    }

    assert!(library.unheld_releases(Some("axb"), None)?.is_empty());
    assert_eq!(library.albums_kept_for("a_b")?.len(), 1);
    assert!(library.albums_kept_for("axb")?.is_empty());
    Ok(())
}

#[test]
fn the_rows_an_album_is_short_of_are_listed_with_their_wants() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    rows.push(release_row(5, "Marlow", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    assert_eq!(library.rematch(album.id)?, 3);

    let harbour_lights = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "Harbour Lights")
        .expect("the release holds the row");
    let want = library.want(harbour_lights.id)?;

    let missing = library.missing_tracks(None, None)?;

    assert_eq!(missing.len(), 2);
    assert_eq!(
        missing[0],
        MissingTrack {
            album: album.id,
            album_title: "Orbits".to_owned(),
            owner: Some("The Orbiters".to_owned()),
            release_track: harbour_lights.id,
            disc: 1,
            position: 4,
            number: Some("4".to_owned()),
            title: "Harbour Lights".to_owned(),
            artist: None,
            length: Some(Duration::from_secs(4)),
            want: Some(want),
        }
    );
    assert_eq!(missing[1].title, "Marlow");
    assert_eq!(missing[1].position, 5);
    assert_eq!(missing[1].want, None);
    assert_eq!(library.missing_tracks(None, Some(1))?, missing[..1]);
    assert_eq!(
        library.missing_counted(None)?,
        Missing {
            tracks: 2,
            releases: 0,
        }
    );
    assert!(library.unheld_releases(None, None)?.is_empty());
    Ok(())
}

#[test]
fn a_dismissed_missing_row_leaves_the_listing_through_a_refresh_until_wanted_or_brought_back()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    rows.push(release_row(5, "Marlow", Vec::new()));
    library.land_release(album.id, &orbits(rows.clone(), Vec::new()))?;
    library.rematch(album.id)?;

    let row_titled = |title: &str| -> Result<ReleaseTrackId> {
        Ok(library
            .release_tracks(album.id)?
            .into_iter()
            .find(|row| row.title == title)
            .expect("the release holds the row")
            .id)
    };
    let titles = |library: &Library| -> Result<Vec<String>> {
        Ok(library
            .missing_tracks(None, None)?
            .into_iter()
            .map(|row| row.title)
            .collect())
    };

    library.want(row_titled("Harbour Lights")?)?;

    assert!(library.dismiss_missing(row_titled("Harbour Lights")?)?);
    assert!(!library.dismiss_missing(row_titled("Harbour Lights")?)?);
    assert_eq!(titles(&library)?, vec!["Marlow".to_owned()]);
    assert!(library.wants()?.is_empty());
    assert_eq!(
        library.missing_counted(None)?,
        Missing {
            tracks: 1,
            releases: 0,
        }
    );
    assert_eq!(
        library.dismissed()?,
        Missing {
            tracks: 1,
            releases: 0,
        }
    );
    assert!(library.unheld_matching("lights", None)?.is_empty());

    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    library.rematch(album.id)?;

    assert_eq!(titles(&library)?, vec!["Marlow".to_owned()]);

    library.dismiss_missing(row_titled("Marlow")?)?;

    assert!(titles(&library)?.is_empty());

    library.want(row_titled("Marlow")?)?;

    assert_eq!(titles(&library)?, vec!["Marlow".to_owned()]);

    assert_eq!(
        library.bring_back_dismissed()?,
        Missing {
            tracks: 1,
            releases: 0,
        }
    );
    assert_eq!(
        titles(&library)?,
        vec!["Harbour Lights".to_owned(), "Marlow".to_owned()]
    );
    assert_eq!(library.dismissed()?, Missing::default());
    assert!(matches!(
        library.dismiss_missing(ReleaseTrackId::MAX),
        Err(Error::UnknownReleaseTrack(_))
    ));
    Ok(())
}

#[test]
fn dismissing_one_of_two_missing_rows_of_a_title_leaves_the_other_listed() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Interlude", Vec::new()));
    rows.push(release_row(5, "Marlow", Vec::new()));
    rows.push(release_row(6, "Interlude", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;
    library.rematch(album.id)?;

    let first = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "Interlude")
        .expect("the release holds the row")
        .id;

    assert!(library.dismiss_missing(first)?);

    let listed: Vec<(u32, String)> = library
        .missing_tracks(None, None)?
        .into_iter()
        .map(|row| (row.position, row.title))
        .collect();

    assert_eq!(
        listed,
        [(5, "Marlow".to_owned()), (6, "Interlude".to_owned())]
    );
    assert_eq!(library.dismissed()?.tracks, 1);
    Ok(())
}

#[test]
fn a_dismissed_unheld_release_leaves_the_listing_and_the_artists_count() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = vec![Credit {
        name: "The Orbiters".to_owned(),
        joined_by: String::new(),
        mbid: Some(mbid(ORBITERS)),
    }];

    let fake = Arc::new(Fake::new(Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![release],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        ..Canned::default()
    }));

    enrich(&library, &fake, false)?;

    let artist = artist_named(&library, "The Orbiters")?;

    assert!(library.dismiss_release(artist.id, &mbid(HOURS_GROUP))?);
    assert!(!library.dismiss_release(artist.id, &mbid(HOURS_GROUP))?);
    assert!(!library.dismiss_release(artist.id, &mbid(RELEASE))?);
    assert_eq!(
        library.unheld_releases(None, None)?,
        vec![
            unheld(&artist, SCORE_GROUP, "The Orbit", "Album", Some("2003-11")),
            unheld(&artist, SINGLE_GROUP, "Harbour Lights", "Single", None),
        ]
    );
    assert_eq!(
        library
            .artist_detail(artist.id)?
            .map(|detail| detail.releases_unheld),
        Some(2)
    );
    assert_eq!(
        library.dismissed()?,
        Missing {
            tracks: 0,
            releases: 1,
        }
    );

    library.bring_back_dismissed()?;

    assert_eq!(library.unheld_releases(None, None)?.len(), 3);
    Ok(())
}
