use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use resonate_core::MediaLocation;
use resonate_library::{
    ArtistProfile, ArtistRelease, Credit, Genre, GroupAsked, GroupMatch, GroupRelease, Issued,
    Library, LifeSpan, Link, Medium, Recording, RecordingMatch, RecordingRelease, Release,
    ReleaseAsked, ReleaseGroup, ReleaseMatch, ReleaseTrack, Result, Wording,
};

use crate::support::{
    ADA, ALBUM, ALBUM_ARTIST, ANOTHER_RECORDING, ARTIST, BEST_OF_GROUP, BROADCAST_GROUP, Canned,
    Delivering, Fake, HOURS, HOURS_GROUP, LIVE_GROUP, MUSICBRAINZ_ALBUM, MUSICBRAINZ_ARTIST,
    ORBITERS, Offering, RELEASE, RELEASE_GROUP, SCORE_GROUP, SINGLE_GROUP, THE_FILES_LENGTH, TITLE,
    TRACK, Tree, Wav, artist_release, credited, isrc, mbid, options, release_row, scan,
};

pub(crate) const LOW_TIDE_SHEET: &str = r#"REM DATE 1971
PERFORMER "Example Band"
TITLE "Tideline"
FILE "Tideline.wav" WAVE
  TRACK 01 AUDIO
    TITLE "Small Mercies"
    INDEX 01 00:00:00
  TRACK 02 AUDIO
    TITLE "A Quilt of Winds"
    INDEX 01 00:00:20
  TRACK 03 AUDIO
    TITLE "Lantern"
    INDEX 01 00:00:40
"#;

pub(crate) const TWO_DISC_SHEET: &str = "PERFORMER \"Example Band\"\nTITLE \"The Lighthouse\"\nFILE \"CD1/lighthouse.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Open the Gate?\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"The Narrow Bridge\"\n    INDEX 01 00:00:40\nFILE \"cd2\\\\lighthouse.wav\" WAVE\n  TRACK 03 AUDIO\n    TITLE \"Another Door\"\n    INDEX 01 00:00:00\n";

pub(crate) fn orbits(tracks: Vec<ReleaseTrack>, links: Vec<Link>) -> Release {
    Release {
        id: mbid(RELEASE),
        group: Some(mbid(RELEASE_GROUP)),
        title: "Orbits".to_owned(),
        credit: Vec::new(),
        date: Some("1998-03-02".to_owned()),
        country: Some("GB".to_owned()),
        label: Some("Harvest".to_owned()),
        catalog_number: Some("SHVL 795".to_owned()),
        barcode: Some("724382920021".to_owned()),
        kind: Some("Album".to_owned()),
        disambiguation: Some("2011 remaster".to_owned()),
        has_front_cover: true,
        links,
        media: vec![Medium {
            position: 1,
            format: Some("CD".to_owned()),
            title: None,
            tracks,
        }],
    }
}

pub(crate) const ORBITERS_PICTURE: &str =
    "https://commons.wikimedia.org/wiki/File:Orbiters1973.jpg";

pub(crate) fn orbiters() -> ArtistProfile {
    ArtistProfile {
        mbid: mbid(ORBITERS),
        name: "The Orbiters".to_owned(),
        sort_name: Some("Orbiters, The".to_owned()),
        kind: Some("Group".to_owned()),
        gender: None,
        country: Some("GB".to_owned()),
        area: Some("London".to_owned()),
        began_in: Some("Cambridge".to_owned()),
        span: LifeSpan {
            begin: Some("1965".to_owned()),
            end: None,
            ended: true,
        },
        disambiguation: Some("the English band".to_owned()),
        aliases: Vec::new(),
        genres: vec![
            Genre {
                name: "progressive rock".to_owned(),
                weight: 12,
            },
            Genre {
                name: "psychedelic rock".to_owned(),
                weight: 7,
            },
        ],
        links: vec![
            Link::new("image", ORBITERS_PICTURE.to_owned()),
            Link::new(
                "streaming",
                "https://open.spotify.com/artist/0k17h".to_owned(),
            ),
            Link::new(
                "official homepage",
                "https://www.theorbiters.example/".to_owned(),
            ),
        ],
    }
}

pub(crate) fn orbits_tree() -> Tree {
    let tree = Tree::new();

    for (file, title, number) in [
        ("1.wav", "Small Mercies", Some("1")),
        ("2.wav", "A Quilt of Winds", Some("2")),
        ("3.wav", "Paper Boats!", None),
    ] {
        let mut track = Wav::new()
            .text(TITLE, title)
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits");

        if let Some(number) = number {
            track = track.text(TRACK, number);
        }
        tree.write(file, &track.build());
    }

    tree
}

pub(crate) fn scanned_orbits() -> Result<(Tree, Library)> {
    let tree = orbits_tree();
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;
    Ok((tree, library))
}

pub(crate) fn scanned_orbits_on_disk() -> Result<(Tree, Library, PathBuf)> {
    let tree = orbits_tree();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    Ok((tree, library, database))
}

pub(crate) fn orbits_track(title: &str, number: u32) -> Wav {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, "The Orbiters")
        .text(ALBUM, "Orbits")
        .text(TRACK, &number.to_string())
}

pub(crate) const ORBITS_TITLES: [&str; 3] = ["Small Mercies", "A Quilt of Winds", "Paper Boats"];

pub(crate) fn write_orbits(tree: &Tree, tagged: bool) {
    for (index, title) in ORBITS_TITLES.into_iter().enumerate() {
        let number = index as u32 + 1;
        let mut track = orbits_track(title, number);

        if tagged {
            track = track.described(MUSICBRAINZ_ALBUM, RELEASE);
        }
        tree.write(&format!("Orbits/{number}.wav"), &track.build());
    }
}

pub(crate) fn orbits_rows() -> Vec<ReleaseTrack> {
    ORBITS_TITLES
        .into_iter()
        .enumerate()
        .map(|(index, title)| release_row(index as u32 + 1, title, Vec::new()))
        .collect()
}

pub(crate) fn orbits_match(score: u8, artist: Option<&str>, tracks: Option<u32>) -> ReleaseMatch {
    ReleaseMatch {
        release: mbid(RELEASE),
        group: None,
        score,
        title: "Orbits".to_owned(),
        credit: credited(artist, None),
        track_count: tracks,
        date: None,
    }
}

pub(crate) fn orbits_group_match(score: u8, artist: Option<&str>) -> GroupMatch {
    GroupMatch {
        group: mbid(RELEASE_GROUP),
        score,
        title: "Orbits".to_owned(),
        credit: credited(artist, None),
    }
}

pub(crate) fn orbits_group(releases: Vec<GroupRelease>, links: Vec<Link>) -> ReleaseGroup {
    ReleaseGroup {
        id: mbid(RELEASE_GROUP),
        title: "Orbits".to_owned(),
        credit: Vec::new(),
        kind: Some("Album".to_owned()),
        first_released: Some("1971-10-30".to_owned()),
        disambiguation: Some("the original run".to_owned()),
        links,
        releases,
    }
}

pub(crate) fn orbits_asked() -> ReleaseAsked {
    ReleaseAsked {
        title: "Orbits".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        barcode: None,
        catalog_number: None,
        wording: Wording::Phrase,
    }
}

pub(crate) fn orbits_asked_in_words() -> ReleaseAsked {
    ReleaseAsked {
        wording: Wording::Words,
        ..orbits_asked()
    }
}

pub(crate) fn orbits_group_asked() -> GroupAsked {
    GroupAsked {
        title: "Orbits".to_owned(),
        artist: Some("The Orbiters".to_owned()),
        artist_mbid: None,
        year: None,
        wording: Wording::Phrase,
    }
}

pub(crate) fn hours() -> Release {
    Release {
        id: mbid(HOURS),
        group: None,
        title: "Hours".to_owned(),
        credit: Vec::new(),
        date: None,
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: None,
        disambiguation: None,
        has_front_cover: false,
        links: Vec::new(),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![release_row(1, "Dawn", Vec::new())],
        }],
    }
}

pub(crate) fn write_hours(tree: &Tree, picture: Option<&[u8]>) {
    let mut track = Wav::new()
        .text(TITLE, "Dawn")
        .text(ARTIST, "Ada")
        .text(ALBUM, "Hours")
        .described(MUSICBRAINZ_ALBUM, HOURS)
        .described(MUSICBRAINZ_ARTIST, ADA);

    if let Some(picture) = picture {
        track = track.picture(picture);
    }
    tree.write("Hours/1.wav", &track.build());
}

pub(crate) fn ada() -> ArtistProfile {
    ArtistProfile {
        mbid: mbid(ADA),
        name: "Ada".to_owned(),
        sort_name: None,
        kind: Some("Person".to_owned()),
        gender: None,
        country: None,
        area: None,
        began_in: None,
        span: LifeSpan::default(),
        disambiguation: None,
        aliases: Vec::new(),
        genres: Vec::new(),
        links: Vec::new(),
    }
}

pub(crate) fn orbiters_groups() -> Vec<ArtistRelease> {
    vec![
        artist_release(
            RELEASE_GROUP,
            "Orbits",
            Some("Album"),
            &[],
            Some("1998-03-02"),
        ),
        artist_release(HOURS_GROUP, "Hours", Some("EP"), &[], Some("1996-05-06")),
        artist_release(
            LIVE_GROUP,
            "Orbits Live",
            Some("Album"),
            &["Live"],
            Some("1999"),
        ),
        artist_release(
            BEST_OF_GROUP,
            "The Best of the Orbiters",
            Some("Album"),
            &["Compilation"],
            Some("2001"),
        ),
        artist_release(
            SCORE_GROUP,
            "The Orbit",
            Some("Album"),
            &["Soundtrack"],
            Some("2003-11"),
        ),
        artist_release(SINGLE_GROUP, "Harbour Lights", Some("Single"), &[], None),
        artist_release(
            BROADCAST_GROUP,
            "A Broadcast",
            Some("Broadcast"),
            &[],
            Some("1997"),
        ),
    ]
}

pub(crate) const DAYBREAK: &str = "d3c9e2f0-3333-4444-8555-666677778888";

pub(crate) const HOURS_PAPER_BOATS: &str = "e4daf301-4444-4555-8666-777788889999";

pub(crate) const HOURS_REISSUE: &str = "f5eb0412-5555-4666-8777-88889999aaaa";

pub(crate) const HOURS_BONUS: &str = "a6fc1523-6666-4777-8888-9999aaaabbbb";

pub(crate) fn hours_pressed(id: &str, date: &str, songs: &[(&str, &str)]) -> Release {
    Release {
        id: mbid(id),
        group: Some(mbid(HOURS_GROUP)),
        title: "Hours".to_owned(),
        credit: credited(Some("The Orbiters"), Some(ORBITERS)),
        date: Some(date.to_owned()),
        kind: Some("EP".to_owned()),
        has_front_cover: false,
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: songs
                .iter()
                .enumerate()
                .map(|(index, (title, recording))| ReleaseTrack {
                    recording: Some(mbid(recording)),
                    ..release_row(index as u32 + 1, title, Vec::new())
                })
                .collect(),
        }],
        ..orbits(Vec::new(), Vec::new())
    }
}

pub(crate) fn hours_pressings() -> Vec<Release> {
    vec![
        hours_pressed(
            HOURS_REISSUE,
            "2011",
            &[
                ("Daybreak", DAYBREAK),
                ("Paper Boats", HOURS_PAPER_BOATS),
                ("Daybreak (demo)", HOURS_BONUS),
            ],
        ),
        hours_pressed(
            HOURS,
            "1996-05-06",
            &[("Daybreak", DAYBREAK), ("Paper Boats", HOURS_PAPER_BOATS)],
        ),
    ]
}

pub(crate) fn on_orbits(position: u32) -> Vec<RecordingRelease> {
    vec![RecordingRelease {
        id: mbid(RELEASE),
        title: "Orbits".to_owned(),
        date: Some("1971-10-30".to_owned()),
        disc: Some(1),
        position: Some(position),
        issued: Issued::default(),
    }]
}

pub(crate) fn orbits_recording(
    id: &str,
    title: &str,
    code: &str,
    releases: Vec<RecordingRelease>,
) -> Recording {
    Recording {
        id: mbid(id),
        title: title.to_owned(),
        credit: vec![Credit {
            name: "The Orbiters".to_owned(),
            joined_by: String::new(),
            mbid: Some(mbid(ORBITERS)),
        }],
        length: Some(THE_FILES_LENGTH),
        isrcs: vec![isrc(code)],
        releases,
    }
}

pub(crate) fn low_tide(title: &str, number: &str) -> Vec<u8> {
    Wav::new()
        .text(TITLE, title)
        .text(ARTIST, "Example Band")
        .text(ALBUM_ARTIST, "Example Band")
        .text(ALBUM, "Tideline")
        .text(TRACK, number)
        .build()
}

pub(crate) const TRACKS_OF_A_DECADE: u32 = 24;

pub(crate) const THE_POLL_HAS_WAITED: Duration = Duration::from_millis(400);

pub(crate) fn orbits_file(tree: &Tree, name: &str) -> MediaLocation {
    MediaLocation::local(
        fs::canonicalize(tree.path().join(name)).expect("the scanned file is there"),
    )
}

pub(crate) const SLOW_WATER: &str = "d7c2a3f0-3333-4444-8555-666677778888";

pub(crate) const LOW_TIDE: &str = "e8d3b4a1-4444-4555-8666-777788889999";

pub(crate) fn low_tide_release() -> Release {
    let mut slow_water = release_row(2, "Lantern", Vec::new());

    slow_water.recording = Some(mbid(SLOW_WATER));

    let mut marlow = release_row(1, "Marlow", Vec::new());

    marlow.recording = Some(mbid(ANOTHER_RECORDING));
    Release {
        id: mbid(LOW_TIDE),
        group: None,
        title: "Tideline".to_owned(),
        credit: credited(Some("Example Band"), None),
        date: Some("1971-10-30".to_owned()),
        country: None,
        label: None,
        catalog_number: None,
        barcode: None,
        kind: Some("Album".to_owned()),
        disambiguation: None,
        has_front_cover: false,
        links: Vec::new(),
        media: vec![Medium {
            position: 1,
            format: None,
            title: None,
            tracks: vec![marlow, slow_water],
        }],
    }
}

pub(crate) fn slow_water_found() -> RecordingMatch {
    RecordingMatch {
        recording: mbid(SLOW_WATER),
        score: 100,
        title: "Lantern".to_owned(),
        credit: credited(Some("Example Band"), None),
        length: Some(Duration::from_secs(1_411)),
        isrcs: Vec::new(),
        releases: vec![
            RecordingRelease {
                id: mbid(HOURS),
                title: "Lantern: The Best Of".to_owned(),
                date: Some("2001-11-05".to_owned()),
                disc: None,
                position: None,
                issued: Issued::default(),
            },
            RecordingRelease {
                id: mbid(LOW_TIDE),
                title: "Tideline".to_owned(),
                date: Some("1971-10-30".to_owned()),
                disc: None,
                position: None,
                issued: Issued::default(),
            },
        ],
    }
}

pub(crate) fn a_provider_offering_slow_water(source: &str) -> Arc<Offering> {
    Arc::new(Offering::new(
        source,
        Delivering::Bytes {
            key: "track/1411",
            extension: "wav",
            bytes: Wav::new().text(TITLE, "Lantern").frames(8_820).build(),
        },
    ))
}

pub(crate) fn low_tide_fake() -> Fake {
    Fake::new(Canned {
        found_songs: vec![slow_water_found()],
        releases: vec![low_tide_release()],
        ..Canned::default()
    })
}
