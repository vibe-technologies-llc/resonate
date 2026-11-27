use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    thread,
    time::Duration,
};

use resonate_core::{MediaLocation, PlaylistId, SourceId, WantId};
use resonate_library::{
    Album, AlbumQuery, Artist, ArtistMatch, ArtistQuery, ArtistRelease, Barcode, CoverArt, Credit,
    Cut, DeliveryFolder, Direction, EnrichOptions, EnrichSummary, FileTags, Fingerprinters,
    GroupRelease, ImageFormat, ImportOptions, ImportSummary, Isrc, Layout, Library, Link, LookupOp,
    LyricText, Mbid, OrganiseOptions, OrganiseSummary, Pace, PlaylistOrder, RecordingMatch,
    Reference, ReleaseTrack, Result, RetagOptions, RetagSummary, SortOrder, Sources, Suggestion,
    TagField, Track, TrackQuery, UnheldRelease, Vault, Written,
};
use resonate_providers::{Provider, Providers};

use crate::support::{
    ALBUM, ANSWERED, ARTIST, ASKED, Called, Canned, DAYBREAK, Fake, HOURS, HOURS_GROUP,
    HOURS_PAPER_BOATS, LOW_TIDE_SHEET, ORBITERS, Offering, Stored, THE_FILES_LENGTH, TITLE, TRACK,
    TWO_DISC_SHEET, Tree, Wav, YEAR, hours_pressed, hours_pressings, options, orbiters,
    orbiters_groups, orbits, orbits_match, orbits_rows, png, scan,
};

pub(crate) fn named(library: &Library, order: PlaylistOrder) -> Result<Vec<String>> {
    read_as(library, order, Direction::Ascending)
}

pub(crate) fn read_as(
    library: &Library,
    order: PlaylistOrder,
    direction: Direction,
) -> Result<Vec<String>> {
    Ok(library
        .playlists(order, direction, None)?
        .into_iter()
        .map(|playlist| playlist.name)
        .collect())
}

pub(crate) fn scanned_playlist_tree() -> (Tree, Library) {
    let tree = Tree::new();

    for (file, title) in [("a.wav", "Lantern"), ("b.wav", "Dogs"), ("c.wav", "Sheep")] {
        tree.write(file, &Wav::new().text(TITLE, title).build());
    }

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library)
}

pub(crate) fn one_song(bits: u16, frames: usize) -> Vec<u8> {
    Wav::new()
        .bits(bits)
        .frames(frames)
        .text(TITLE, "Lantern")
        .text(ARTIST, "Example Band")
        .text(ALBUM, "Tideline")
        .text(TRACK, "6")
        .build()
}

pub(crate) fn whole(path: &Path) -> Cut {
    Cut::whole(MediaLocation::local(path))
}

pub(crate) fn the_row_at(library: &Library, path: &Path) -> Result<Track> {
    Ok(library
        .track_at(path, None)?
        .expect("the file is catalogued"))
}

pub(crate) fn offered(library: &Library, file: &Path) -> Result<()> {
    let want = library.wants()?.remove(0);
    library.note_tried(want.id, Some(&MediaLocation::local(file)))
}

pub(crate) fn stems(cuts: &[Cut]) -> Vec<String> {
    cuts.iter()
        .map(|cut| cut.location.stem().unwrap_or_default().into_owned())
        .collect()
}

pub(crate) fn sortable_playlist_tree() -> (Tree, Library) {
    let tree = Tree::new();
    let written = [
        ("c.wav", "Lantern", "Zephyr Band", 44_100),
        ("a.wav", "Dogs", "Example Band", 13_230),
        ("b.wav", "Sheep", "Hollow Band", 4_410),
    ];

    for (file, title, artist, frames) in written {
        tree.write(
            file,
            &Wav::new()
                .frames(frames)
                .text(TITLE, title)
                .text(ARTIST, artist)
                .build(),
        );
    }

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of three files");
    (tree, library)
}

pub(crate) fn held(library: &Library, id: PlaylistId) -> Result<Vec<String>> {
    Ok(stems(&library.playlist_cuts(id)?))
}

pub(crate) fn scanned_shapes() -> (Tree, Library) {
    let tree = Tree::new();

    tree.write(
        "cold.wav",
        &Wav::new()
            .frames(4_410)
            .text(TITLE, "Cold")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Winter")
            .text(YEAR, "1975")
            .build(),
    );
    tree.write(
        "heat.wav",
        &Wav::new()
            .bits(24)
            .frames(44_100)
            .text(TITLE, "Heat")
            .text(ARTIST, "Ben")
            .text(ALBUM, "Summer")
            .text(YEAR, "1995")
            .build(),
    );

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of two files");
    (tree, library)
}

pub(crate) fn matched(library: &Library, text: &str) -> Result<Vec<String>> {
    let found = library.tracks(&TrackQuery {
        text: Some(text.to_owned()),
        sort: SortOrder::Title,
        reading: SortOrder::Title.reads(),
        ..TrackQuery::default()
    })?;

    Ok(found.into_iter().map(|track| track.title).collect())
}

pub(crate) fn scanned_sheet() -> (Tree, Library) {
    let tree = Tree::new();

    tree.write("Tideline.wav", &Wav::new().frames(44_100).build());
    tree.write("Tideline.cue", LOW_TIDE_SHEET.as_bytes());

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of one file a sheet cuts");
    (tree, library)
}

pub(crate) fn scanned_two_discs() -> (Tree, Library) {
    let tree = Tree::new();

    tree.write("CD1/lighthouse.wav", &Wav::new().frames(44_100).build());
    tree.write("CD2/lighthouse.wav", &Wav::new().frames(44_100).build());
    tree.write("The Lighthouse.cue", TWO_DISC_SHEET.as_bytes());

    let library = Library::open_in_memory().expect("an in-memory library");

    scan(&library, &options(&tree)).expect("a scan of two files a sheet above them cuts");
    (tree, library)
}

pub(crate) fn ffmpeg() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

pub(crate) fn mbid(text: &str) -> Mbid {
    Mbid::new(text).expect("a well-formed mbid")
}

pub(crate) fn only_album(library: &Library) -> Result<Album> {
    let mut albums = library.albums(&AlbumQuery::default())?;

    assert_eq!(albums.len(), 1, "one album was expected, got {albums:?}");
    Ok(albums.remove(0))
}

pub(crate) fn artist_named(library: &Library, name: &str) -> Result<Artist> {
    library
        .artists(&ArtistQuery::default())?
        .into_iter()
        .find(|artist| artist.name == name)
        .ok_or_else(|| panic!("no artist named {name}"))
}

pub(crate) fn release_row(position: u32, title: &str, links: Vec<Link>) -> ReleaseTrack {
    ReleaseTrack {
        position,
        number: position.to_string(),
        title: title.to_owned(),
        artist: None,
        recording: None,
        track: None,
        length: Some(Duration::from_millis(1_000 * u64::from(position))),
        isrcs: Vec::new(),
        links,
    }
}

pub(crate) fn beside(database: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(database).expect("the catalog file opens for a second reader")
}

pub(crate) fn settled(file: &Path) -> String {
    file.canonicalize()
        .expect("the file the scan walked is still there")
        .to_str()
        .expect("a path the catalog can hold")
        .to_owned()
}

pub(crate) fn stored(database: &Path, file: &Path) -> Stored {
    beside(database)
        .query_row(
            "SELECT title, artist, tagged_title, tagged_artist, isrc, release_title,
                    asked, answered
               FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| {
                Ok(Stored {
                    title: row.get(0)?,
                    artist: row.get(1)?,
                    tagged_title: row.get(2)?,
                    tagged_artist: row.get(3)?,
                    isrc: row.get(4)?,
                    release_title: row.get(5)?,
                    asked: row.get(6)?,
                    answered: row.get(7)?,
                })
            },
        )
        .expect("the scan stored the track")
}

pub(crate) fn answer_track(database: &Path, file: &Path, title: &str, artist: &str, release: &str) {
    let written = beside(database)
        .execute(
            "UPDATE tracks
                SET title = ?1, artist = ?2, release_title = ?3, asked = ?4, answered = ?5
              WHERE path = ?6",
            rusqlite::params![title, artist, release, ASKED, ANSWERED, settled(file)],
        )
        .expect("the row the enrichment answers for is writable");

    assert_eq!(written, 1, "one track row was expected to be answered for");
}

pub(crate) fn words(text: &str) -> LyricText {
    LyricText {
        text: text.to_owned(),
        synced: false,
        lyricsfile: None,
    }
}

pub(crate) fn timed_words(text: &str) -> LyricText {
    LyricText {
        synced: true,
        ..words(text)
    }
}

pub(crate) fn png_art(size: usize) -> CoverArt {
    CoverArt {
        format: ImageFormat::Png,
        bytes: png(size),
    }
}

pub(crate) fn enrich(library: &Library, fake: &Arc<Fake>, refresh: bool) -> Result<EnrichSummary> {
    let reference = Arc::clone(fake) as Arc<dyn Reference>;
    let summary = library
        .enrich(
            reference,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                refresh,
                pace: Pace::Hurried,
                ..EnrichOptions::default()
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

pub(crate) fn credited(artist: Option<&str>, id: Option<&str>) -> Vec<Credit> {
    artist
        .into_iter()
        .map(|name| Credit {
            name: name.to_owned(),
            joined_by: String::new(),
            mbid: id.map(mbid),
        })
        .collect()
}

pub(crate) fn group_release(id: &str, date: &str, tracks: Option<u32>) -> GroupRelease {
    GroupRelease {
        id: mbid(id),
        title: "Orbits".to_owned(),
        date: Some(date.to_owned()),
        country: None,
        track_count: tracks,
        discs: Vec::new(),
    }
}

pub(crate) fn album_titled(library: &Library, title: &str) -> Result<Album> {
    library
        .albums(&AlbumQuery::default())?
        .into_iter()
        .find(|album| album.title == title)
        .ok_or_else(|| panic!("no album titled {title}"))
}

pub(crate) fn artist_release(
    id: &str,
    title: &str,
    kind: Option<&str>,
    secondary: &[&str],
    first_released: Option<&str>,
) -> ArtistRelease {
    ArtistRelease {
        mbid: mbid(id),
        title: title.to_owned(),
        kind: kind.map(str::to_owned),
        secondary: secondary.iter().map(|each| (*each).to_owned()).collect(),
        first_released: first_released.map(str::to_owned),
    }
}

pub(crate) fn unheld(
    artist: &Artist,
    id: &str,
    title: &str,
    kind: &str,
    first_released: Option<&str>,
) -> UnheldRelease {
    UnheldRelease {
        artist: artist.id,
        artist_name: artist.name.clone(),
        mbid: mbid(id),
        title: title.to_owned(),
        kind: Some(kind.to_owned()),
        first_released: first_released.map(str::to_owned),
    }
}

pub(crate) fn learnt_canned() -> Canned {
    let mut release = orbits(orbits_rows(), Vec::new());

    release.credit = credited(Some("The Orbiters"), Some(ORBITERS));
    Canned {
        found_releases: vec![orbits_match(100, Some("The Orbiters"), Some(3))],
        releases: vec![
            release,
            hours_pressed(
                HOURS,
                "1996-05-06",
                &[("Daybreak", DAYBREAK), ("Paper Boats", HOURS_PAPER_BOATS)],
            ),
        ],
        artists: vec![orbiters()],
        artist_releases: vec![(mbid(ORBITERS), orbiters_groups())],
        pressings: vec![(mbid(HOURS_GROUP), hours_pressings())],
        ..Canned::default()
    }
}

pub(crate) fn waited_for(settled: impl Fn() -> bool) -> bool {
    const LOOKS: u32 = 500;
    const BETWEEN_LOOKS: Duration = Duration::from_millis(10);

    (0..LOOKS).any(|_| {
        let now = settled();

        if !now {
            thread::sleep(BETWEEN_LOOKS);
        }
        now
    })
}

pub(crate) fn asked_in_order(fake: &Fake) -> Vec<Called> {
    fake.calls()
        .into_iter()
        .filter(|called| !matches!(called.op(), LookupOp::Cover | LookupOp::Portrait))
        .collect()
}

pub(crate) fn artist_match(score: u8, name: &str) -> ArtistMatch {
    ArtistMatch {
        mbid: mbid(ORBITERS),
        name: name.to_owned(),
        score,
        kind: Some("Group".to_owned()),
        disambiguation: None,
        aliases: Vec::new(),
    }
}

pub(crate) fn isrc(text: &str) -> Isrc {
    Isrc::new(text).expect("a well-formed isrc")
}

pub(crate) fn recording_match(id: &str, title: &str) -> RecordingMatch {
    RecordingMatch {
        recording: mbid(id),
        score: 100,
        title: title.to_owned(),
        credit: vec![Credit {
            name: "The Orbiters".to_owned(),
            joined_by: String::new(),
            mbid: None,
        }],
        length: Some(THE_FILES_LENGTH),
        isrcs: Vec::new(),
        releases: Vec::new(),
    }
}

pub(crate) fn scanned_lone(track: Wav) -> Result<(Tree, Library, PathBuf, PathBuf)> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &track.build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    Ok((tree, library, database, file))
}

pub(crate) fn recorded(database: &Path, file: &Path) -> Option<String> {
    beside(database)
        .query_row(
            "SELECT mbid FROM tracks WHERE path = ?1",
            rusqlite::params![settled(file)],
            |row| row.get(0),
        )
        .expect("the scan stored the track")
}

pub(crate) fn wanted_harbour_lights(library: &Library) -> Result<WantId> {
    let album = only_album(library)?;
    let mut rows = orbits_rows();

    rows.push(release_row(4, "Harbour Lights", Vec::new()));
    library.land_release(album.id, &orbits(rows, Vec::new()))?;

    let missing = library
        .release_tracks(album.id)?
        .into_iter()
        .find(|row| row.title == "Harbour Lights")
        .expect("the release holds the row");
    library.want(missing.id)
}

pub(crate) fn filed_under(tree: &Tree) -> PathBuf {
    tree.path()
        .canonicalize()
        .expect("the tree the scan walked is still there")
}

pub(crate) fn previewed(library: &Library) -> Result<OrganiseSummary> {
    let summary = library.organise(OrganiseOptions::default())?.join()?;

    assert!(!summary.cancelled, "the preview reported itself cancelled");
    assert_eq!(summary.stats.moved, 0, "a preview reported a file moved");
    assert_eq!(
        summary.stats.pruned, 0,
        "a preview reported a folder pruned"
    );
    Ok(summary)
}

pub(crate) fn landings(summary: &OrganiseSummary) -> Vec<(PathBuf, PathBuf)> {
    summary
        .plan
        .moves
        .iter()
        .map(|planned| (planned.from.clone(), planned.to.clone()))
        .collect()
}

pub(crate) fn applied(library: &Library) -> Result<OrganiseSummary> {
    let summary = library
        .organise(OrganiseOptions {
            apply: true,
            ..OrganiseOptions::default()
        })?
        .join()?;

    assert!(!summary.cancelled, "the run reported itself cancelled");
    assert_eq!(
        summary.stats.moved as usize,
        summary.plan.files_moving(),
        "the run counted moves it does not name"
    );

    let landings: Vec<&Path> = summary
        .plan
        .moves
        .iter()
        .flat_map(|planned| planned.files().map(|(_, to)| to))
        .collect();

    for planned in &summary.plan.moves {
        for (from, to) in planned.files() {
            assert!(
                planned.parks() || to.exists(),
                "{} did not land",
                to.display()
            );
            assert!(
                landings.contains(&from) || !from.exists(),
                "{} is still where it stood",
                from.display()
            );
        }
        for sidecar in &planned.sidecars {
            assert!(sidecar.to.exists(), "{} did not land", sidecar.to.display());
        }
    }
    Ok(summary)
}

pub(crate) fn keys(database: &Path) -> Vec<String> {
    let reader = beside(database);
    let mut statement = reader
        .prepare("SELECT key FROM album_keys ORDER BY album_id, key")
        .expect("the albums are readable");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .expect("the albums answer their keys")
}

pub(crate) fn retagged(library: &Library, apply: bool) -> Result<RetagSummary> {
    let summary = library
        .retag(
            Arc::new(FileTags::default()),
            RetagOptions {
                roots: Vec::new(),
                apply,
                undo: false,
                lyrics: Vec::new(),
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

pub(crate) fn fields_of(written: &Written) -> Vec<TagField> {
    written.edits.iter().map(|edit| edit.field).collect()
}

pub(crate) fn narrowed(library: &Library, text: &str) -> Result<Vec<String>> {
    Ok(library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            sort: SortOrder::Title,
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

pub(crate) fn asked(suggestions: &[Suggestion]) -> Vec<&str> {
    suggestions
        .iter()
        .filter_map(|suggestion| suggestion.query.text.as_deref())
        .collect()
}

pub(crate) fn vaulted(library: &Library, apply: bool) -> Result<ImportSummary> {
    let summary = library
        .import(
            Arc::new(Sources::local()),
            ImportOptions {
                roots: Vec::new(),
                apply,
                ..ImportOptions::default()
            },
        )?
        .join()?;

    assert!(!summary.cancelled, "the pass reported itself cancelled");
    Ok(summary)
}

pub(crate) fn a_real_picture(width: u32, height: u32) -> CoverArt {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);

    for y in 0..height {
        for x in 0..width {
            pixels.extend_from_slice(&[(x * 5) as u8, (y * 9) as u8, (x + y) as u8, 255]);
        }
    }

    let drawn = image::RgbaImage::from_raw(width, height, pixels).expect("a drawn picture");
    let mut written = std::io::Cursor::new(Vec::new());

    image::DynamicImage::ImageRgba8(drawn)
        .write_to(&mut written, image::ImageFormat::Png)
        .expect("a written picture");

    CoverArt {
        format: ImageFormat::Png,
        bytes: written.into_inner(),
    }
}

pub(crate) fn opened_with_a_vault(held: &Tree) -> Result<(Library, Arc<Vault>)> {
    let vault = Arc::new(Vault::make(held.path()).expect("a writable vault"));
    let library = Library::open_in_memory_with_vault(Arc::clone(&vault))?;
    Ok((library, vault))
}

pub(crate) fn source(name: &str) -> SourceId {
    SourceId::new(name).expect("a nameable source")
}

pub(crate) fn every_file_under(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();

    for entry in fs::read_dir(folder).expect("a readable folder").flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(every_file_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

pub(crate) fn asking_in_turn(offerings: &[&Arc<Offering>]) -> Arc<Providers> {
    Arc::new(
        offerings
            .iter()
            .fold(Providers::none(), |providers, offering| {
                providers.and(Arc::clone(offering) as Arc<dyn Provider>)
            }),
    )
}

pub(crate) fn matching(library: &Library, text: &str) -> Result<Vec<String>> {
    Ok(library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            sort: SortOrder::Title,
            reading: SortOrder::Title.reads(),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

pub(crate) fn barcode(code: &str) -> Barcode {
    Barcode::new(code).expect("a well-formed barcode")
}

pub(crate) fn moved(from: &Path, to: &Path) {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).expect("a writable temporary directory");
    }
    fs::rename(from, to).expect("a file moved within the tree");
}

pub(crate) fn the_row_of(counted: &resonate_library::Counted) -> &Track {
    counted
        .track
        .as_ref()
        .expect("the play was counted against a row")
}

pub(crate) fn unreachable_server(provider: SourceId) -> resonate_providers::Error {
    resonate_providers::Error::Io {
        provider,
        op: resonate_providers::ProviderOp::Search,
        source: io::Error::from(io::ErrorKind::ConnectionRefused),
    }
}

pub(crate) fn delivering_into(library: &Library, tree: &Tree) {
    library.deliver_into(Some(DeliveryFolder {
        path: tree.path().to_path_buf(),
        layout: Layout::default(),
    }));
}

pub(crate) fn a_music_folder() -> Result<(Tree, Library)> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    delivering_into(&library, &tree);
    Ok((tree, library))
}

pub(crate) fn a_music_folder_on_disk() -> Result<(Tree, Library, PathBuf)> {
    let tree = Tree::new();
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    delivering_into(&library, &tree);
    Ok((tree, library, database))
}

pub(crate) fn held_as(tree: &Tree, relative: &str, title: &str, frames: usize) -> PathBuf {
    tree.write(
        relative,
        &Wav::new()
            .frames(frames)
            .text(TITLE, title)
            .text(ARTIST, "the orbiters")
            .build(),
    )
}

pub(crate) fn files_under(folder: &Path, found: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(folder).expect("a readable folder").flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, found);
        } else {
            found.push(path);
        }
    }
}

pub(crate) fn songs_under(tree: &Tree) -> Vec<PathBuf> {
    let mut found = Vec::new();

    files_under(tree.path(), &mut found);
    found.retain(|path| path.extension() == Some(OsStr::new("wav")));
    found.sort();
    found
}

pub(crate) fn followed(database: &Path, list: &str) {
    beside(database)
        .execute(
            "INSERT INTO playlist_shares (playlist_id, created, editable)
             SELECT id, created, 0 FROM playlists WHERE name = ?1",
            [list],
        )
        .expect("the list is followed");
}
