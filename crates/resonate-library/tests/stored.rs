use std::{fs, path::Path, sync::Arc, time::Duration};

use resonate_core::{FrameSpan, Frames, MediaLocation, SourceId};
use resonate_library::{
    AlbumQuery, BETTERED_AFTER, ChosenVersion, CoverArt, CoverSource, Cut, EnrichOptions, Error,
    Fingerprinters, ImageFormat, Library, Link, LookupOp, LyricText, LyricsAhead, Played,
    Reference, Relation, Result, Service,
};

use crate::support::{
    ADA, ALBUM, ALBUM_ARTIST, ANSWERED, ARTIST, ASKED, COMPILATION, Canned, FIRST_WRITE, Fake,
    Fault, ISRC, LABEL, MUSICBRAINZ_ALBUM, MUSICBRAINZ_ARTIST, MUSICBRAINZ_RELEASE_GROUP, ORBITERS,
    ORBITERS_PICTURE, RATE, RECORDING, RELEASE, RELEASE_GROUP, RELEASE_TRACK, SECOND_WRITE, Stored,
    TITLE, TRACK, Tree, WAITED, Wav, all, answer_track, artist_named, beside, enrich, matching,
    mbid, only_album, options, orbiters, orbits, orbits_file, png, recorded, release_row, scan,
    scanned_orbits, scanned_orbits_on_disk, stored, the_row_at, timed_words, words,
};

const MUSICBRAINZ_ALBUM_ARTIST: &str = "MusicBrainz Album Artist Id";

const MUSICBRAINZ_RELEASE_TRACK: &str = "MusicBrainz Release Track Id";

const BARCODE: &str = "BARCODE";

const CATALOG_NUMBER: &str = "CATALOGNUMBER";

#[test]
fn a_scan_writes_the_release_and_artist_ids_the_tags_carry() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "dawn.wav",
        &Wav::new()
            .text(TITLE, "Dawn")
            .text(ARTIST, "Ada")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .described(MUSICBRAINZ_ALBUM, RELEASE)
            .described(MUSICBRAINZ_ARTIST, ADA)
            .described(MUSICBRAINZ_ALBUM_ARTIST, &ORBITERS.to_ascii_uppercase())
            .described(MUSICBRAINZ_RELEASE_GROUP, RELEASE_GROUP)
            .described(MUSICBRAINZ_RELEASE_TRACK, RELEASE_TRACK)
            .build(),
    );
    for (file, title, artist, id) in [
        ("noon.wav", "Noon", "Ada", ADA),
        ("dusk.wav", "Dusk", "Ben", "not an id at all"),
    ] {
        tree.write(
            file,
            &Wav::new()
                .text(TITLE, title)
                .text(ARTIST, artist)
                .text(ALBUM, "Hours")
                .text(COMPILATION, "1")
                .described(MUSICBRAINZ_ARTIST, id)
                .build(),
        );
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let orbits = albums
        .iter()
        .find(|album| album.title == "Orbits")
        .expect("the release was grouped");

    assert_eq!(orbits.mbid, Some(mbid(RELEASE)));
    assert_eq!(orbits.missing, 0);

    let hours = albums
        .iter()
        .find(|album| album.title == "Hours")
        .expect("the compilation was grouped");

    assert_eq!(hours.mbid, None);

    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ORBITERS)),
        "the album artist's id was not written, or not lowercased"
    );
    assert_eq!(
        artist_named(&library, "Ada")?.mbid,
        Some(mbid(ADA)),
        "a performer's id was not written"
    );
    assert_eq!(
        artist_named(&library, "Ben")?.mbid,
        None,
        "a tag holding no id was stored as one"
    );
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.mbid),
        Some(Some(mbid(RELEASE)))
    );
    assert_eq!(
        library.release_of(orbits.id)?.map(|held| held.group),
        Some(Some(mbid(RELEASE_GROUP))),
        "the release group the tags carry was dropped on the floor"
    );
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.group),
        Some(None),
        "an album whose tags name no release group was given one"
    );
    Ok(())
}

#[test]
fn a_scan_stores_what_the_tags_declare_about_the_release() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "Small Mercies")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "1/12")
            .text(LABEL, "  Harvest ")
            .described(CATALOG_NUMBER, "   ")
            .described(MUSICBRAINZ_ALBUM_ARTIST, ORBITERS)
            .build(),
    );
    tree.write(
        "2.wav",
        &Wav::new()
            .text(TITLE, "A Quilt of Winds")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM_ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2/12")
            .described(BARCODE, "724382920021")
            .described(CATALOG_NUMBER, "SHVL 795")
            .described(MUSICBRAINZ_ALBUM_ARTIST, ORBITERS)
            .build(),
    );
    tree.write(
        "3.wav",
        &Wav::new()
            .text(TITLE, "Dawn")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Hours")
            .text(COMPILATION, "1")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let albums = library.albums(&AlbumQuery::default())?;
    let orbits = albums
        .iter()
        .find(|album| album.title == "Orbits")
        .expect("the release was grouped");
    let held = library.release_of(orbits.id)?.expect("the album is known");

    assert_eq!(held.barcode.as_deref(), Some("724382920021"));
    assert_eq!(
        held.catalog_number.as_deref(),
        Some("SHVL 795"),
        "a blank catalogue number was stored, or the one a later file carried was not"
    );
    assert_eq!(
        held.label.as_deref(),
        Some("Harvest"),
        "the label was not trimmed"
    );

    let asked = library
        .album_to_ask(orbits.id)?
        .expect("the album is known");

    assert_eq!(asked.track_count, 2);
    assert_eq!(asked.tagged_tracks, Some(12));
    assert_eq!(asked.catalog_number.as_deref(), Some("SHVL 795"));
    assert_eq!(asked.barcode.as_deref(), Some("724382920021"));
    assert_eq!(asked.owner_mbid, Some(mbid(ORBITERS)));

    let hours = albums
        .iter()
        .find(|album| album.title == "Hours")
        .expect("the compilation was grouped");
    let asked = library.album_to_ask(hours.id)?.expect("the album is known");

    assert_eq!(asked.tagged_tracks, None);
    assert_eq!(asked.catalog_number, None);
    assert_eq!(asked.owner_mbid, None);
    assert_eq!(
        library.release_of(hours.id)?.map(|held| held.label),
        Some(None)
    );
    Ok(())
}

#[test]
fn a_rescan_keeps_a_barcode_the_file_no_longer_carries() -> Result<()> {
    let tree = Tree::new();
    let tagged = |barcode: Option<&str>| {
        let mut track = Wav::new()
            .text(TITLE, "Small Mercies")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits");

        if let Some(barcode) = barcode {
            track = track.described(BARCODE, barcode);
        }
        track.build()
    };

    tree.write("1.wav", &tagged(Some("724382920021")));

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;
    let barcode = |library: &Library| -> Result<Option<String>> {
        Ok(library
            .release_of(album.id)?
            .expect("the album is known")
            .barcode)
    };

    assert_eq!(barcode(&library)?.as_deref(), Some("724382920021"));

    tree.write("1.wav", &tagged(None));

    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(only_album(&library)?.id, album.id);
    assert_eq!(
        barcode(&library)?.as_deref(),
        Some("724382920021"),
        "a barcode the file stopped declaring was cleared"
    );

    tree.write("1.wav", &tagged(Some("5099902894027")));

    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        barcode(&library)?.as_deref(),
        Some("5099902894027"),
        "a barcode the file was retagged with was not taken"
    );
    Ok(())
}

#[test]
fn an_artist_is_found_by_the_fold_of_a_credit_name_however_it_is_spelled() -> Result<()> {
    let tree = Tree::new();

    tree.write(
        "blood.wav",
        &Wav::new()
            .text(TITLE, "Silver for Monsters")
            .text(ARTIST, "Marcin Przybyłowicz")
            .text(ALBUM, "Blood and Wine")
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let billed = artist_named(&library, "Marcin Przybyłowicz")?;

    assert_eq!(
        library.artist_named("Marcin Przybyłowicz")?,
        Some(billed.id),
        "the marked spelling did not find the artist it is billed under"
    );
    assert_eq!(
        library.artist_named("Marcin Przybylowicz")?,
        Some(billed.id),
        "the stripped spelling did not fold onto the artist it names"
    );
    assert_eq!(
        library.artist_named("MARCIN PRZYBYLOWICZ")?,
        Some(billed.id),
        "the fold is not the one the key was written with"
    );
    assert_eq!(library.artist_named("Marcin Przybysz")?, None);
    Ok(())
}

#[test]
fn a_rescan_leaves_every_enrichment_column_where_it_stood() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let album = only_album(&library)?;

    assert!(
        library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id),
        "an album nothing has asked about was not offered to be asked"
    );

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "Small Mercies", Vec::new()),
                release_row(2, "A Quilt of Winds", Vec::new()),
                release_row(3, "Paper Boats", Vec::new()),
            ],
            vec![Link::new(
                "streaming",
                "https://tidal.com/album/55391740".to_owned(),
            )],
        ),
    )?;

    assert_eq!(library.rematch(album.id)?, 3);

    let art = png(256);

    assert!(library.land_archive_cover(
        album.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: art.clone()
        }
    )?);
    assert!(
        !library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id && !asked.rematch_only),
        "an album answered today was offered to be asked again"
    );
    assert!(
        !library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id),
        "an album whose every row is paired was offered for a rematch that could move nothing"
    );
    assert!(
        library
            .albums_to_ask(WAITED, true)?
            .iter()
            .any(|asked| asked.id == album.id)
    );

    let before = library.release_of(album.id)?.expect("the album is known");
    let rows_before = library.release_tracks(album.id)?;

    assert_eq!(before.cover_source, CoverSource::Archive);
    assert_eq!(before.links.len(), 1);
    assert!(before.asked.is_some() && before.answered.is_some());

    let quilt = tree.path().join("2.wav");

    answer_track(
        &database,
        &quilt,
        "A Quilt of Winds (2011 Remaster)",
        "The Orbiters",
        "Orbits",
    );

    tree.write(
        "2.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, "A Quilt of Winds")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "2")
            .build(),
    );

    let stats = scan(&library, &options(&tree))?;

    assert_eq!(stats.updated, 1);

    let after = only_album(&library)?;

    assert_eq!(after.id, album.id);
    assert_eq!(after.mbid, Some(mbid(RELEASE)));
    assert_eq!(after.year, Some(1998));
    assert_eq!(library.release_of(album.id)?.as_ref(), Some(&before));
    assert_eq!(library.release_tracks(album.id)?, rows_before);
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    assert_eq!(
        stored(&database, &quilt),
        Stored {
            title: "A Quilt of Winds (2011 Remaster)".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            tagged_title: Some("A Quilt of Winds".to_owned()),
            tagged_artist: Some("The Orbiters".to_owned()),
            isrc: None,
            release_title: Some("Orbits".to_owned()),
            asked: Some(ASKED),
            answered: Some(ANSWERED),
        }
    );
    Ok(())
}

#[test]
fn a_rescan_keeps_an_answered_tracks_names_unless_the_file_itself_was_retagged() -> Result<()> {
    let tree = Tree::new();
    let tagged = Wav::new()
        .text(TITLE, "small mercies")
        .text(ARTIST, "the orbiters")
        .build();

    let file = tree.write_stamped("1.wav", &tagged, FIRST_WRITE);
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    answer_track(
        &database,
        &file,
        "Small Mercies",
        "The Orbiters",
        "Tideline",
    );

    tree.write_stamped("1.wav", &tagged, SECOND_WRITE);

    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        stored(&database, &file),
        Stored {
            title: "Small Mercies".to_owned(),
            artist: Some("The Orbiters".to_owned()),
            tagged_title: Some("small mercies".to_owned()),
            tagged_artist: Some("the orbiters".to_owned()),
            isrc: None,
            release_title: Some("Tideline".to_owned()),
            asked: Some(ASKED),
            answered: Some(ANSWERED),
        }
    );

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "Small Mercies (Edit)")
            .text(ARTIST, "the orbiters")
            .build(),
    );

    assert_eq!(scan(&library, &options(&tree))?.updated, 1);
    assert_eq!(
        stored(&database, &file),
        Stored {
            title: "Small Mercies (Edit)".to_owned(),
            artist: Some("the orbiters".to_owned()),
            tagged_title: Some("Small Mercies (Edit)".to_owned()),
            tagged_artist: Some("the orbiters".to_owned()),
            isrc: None,
            release_title: Some("Tideline".to_owned()),
            asked: Some(ASKED),
            answered: None,
        }
    );
    Ok(())
}

#[test]
fn an_isrc_tag_is_stored_in_the_one_spelling_a_recording_code_has() -> Result<()> {
    let tree = Tree::new();
    let named = tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "Small Mercies")
            .text(ARTIST, "The Orbiters")
            .text(ISRC, "gb-aye-71-00195")
            .build(),
    );
    let bare = tree.write("2.wav", &Wav::new().build());
    let nonsense = tree.write("3.wav", &Wav::new().text(ISRC, "not a code").build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let held = stored(&database, &named);

    assert_eq!(held.isrc.as_deref(), Some("GBAYE7100195"));
    assert_eq!(
        stored(&database, &nonsense).isrc,
        None,
        "a tag that is no recording code was stored as though it were one"
    );
    assert_eq!(held.tagged_title.as_deref(), Some("Small Mercies"));
    assert_eq!(held.tagged_artist.as_deref(), Some("The Orbiters"));

    let untagged = stored(&database, &bare);

    assert_eq!(untagged.isrc, None);
    assert_eq!(untagged.title, "2");
    assert_eq!(
        untagged.tagged_title, None,
        "a file naming no title was given one by the stem fallback"
    );
    assert_eq!(untagged.tagged_artist, None);
    Ok(())
}

#[test]
fn a_stem_is_never_read_over_a_tag_the_file_already_carried() -> Result<()> {
    let tree = Tree::new();
    let tagged = tree.write(
        "03 - Mira Dale - Salt Road.wav",
        &Wav::new()
            .text(TITLE, "Blue in Green")
            .text(ARTIST, "Bill Evans")
            .text(TRACK, "9")
            .build(),
    );
    let bare = tree.write("01 - Ada - Zenith.wav", &Wav::new().build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let held = stored(&database, &tagged);

    assert_eq!(held.tagged_title.as_deref(), Some("Blue in Green"));
    assert_eq!(held.tagged_artist.as_deref(), Some("Bill Evans"));

    let named = stored(&database, &bare);

    assert_eq!(named.title, "Zenith");
    assert_eq!(
        named.tagged_title.as_deref(),
        Some("Zenith"),
        "a name read out of the stem should be what the file says it is"
    );
    assert_eq!(named.tagged_artist.as_deref(), Some("Ada"));

    let listed = all(&library)?;
    let numbered = |title: &str| {
        listed
            .iter()
            .find(|track| track.title == title)
            .and_then(|track| track.track_number)
    };

    assert_eq!(
        numbered("Blue in Green"),
        Some(9),
        "the number the stem carries was read over the one the file tagged"
    );
    assert_eq!(numbered("Zenith"), Some(1));
    Ok(())
}

#[test]
fn an_artist_is_taken_from_a_stem_only_where_the_title_it_parsed_is_the_title_tagged() -> Result<()>
{
    let tree = Tree::new();
    let agreeing = tree.write(
        "03 - Mira Dale - Salt Road.wav",
        &Wav::new().text(TITLE, "Salt Road").build(),
    );
    let disagreeing = tree.write(
        "Salt Road (Remastered 2011) - Live Version.wav",
        &Wav::new().text(TITLE, "Salt Road").build(),
    );
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;

    let taken = stored(&database, &agreeing);

    assert_eq!(taken.tagged_title.as_deref(), Some("Salt Road"));
    assert_eq!(taken.tagged_artist.as_deref(), Some("Mira Dale"));

    let refused = stored(&database, &disagreeing);

    assert_eq!(refused.tagged_title.as_deref(), Some("Salt Road"));
    assert_eq!(
        refused.tagged_artist, None,
        "a stem whose title is not the one tagged gave up its artist"
    );
    Ok(())
}

#[test]
fn a_song_whose_fetched_metadata_is_forgotten_reads_as_tagged_and_asks_again_for_its_cover()
-> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let album = only_album(&library)?;
    let file = tree.path().join("1.wav");

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "Small Mercies", Vec::new()),
                release_row(2, "A Quilt of Winds", Vec::new()),
                release_row(3, "Paper Boats", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;
    library.rematch(album.id)?;

    assert!(library.land_archive_cover(
        album.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: png(128)
        }
    )?);

    answer_track(&database, &file, "Small Mercies", "Orbiters", "Orbits");

    let track = the_row_at(&library, &file)?.id;

    assert_eq!(library.forget_what_was_fetched_for(track)?, Some(album.id));

    let forgotten = stored(&database, &file);

    assert_eq!(forgotten.title, "Small Mercies");
    assert_eq!(forgotten.artist.as_deref(), Some("The Orbiters"));
    assert_eq!(forgotten.release_title, None);
    assert_eq!((forgotten.asked, forgotten.answered), (None, None));
    assert_eq!(library.cover_art(album.id)?, None);
    assert_eq!(
        library
            .release_of(album.id)?
            .expect("the album is still known")
            .cover_source,
        CoverSource::File
    );
    assert!(
        library
            .albums_to_ask(WAITED, false)?
            .iter()
            .any(|asked| asked.id == album.id && !asked.rematch_only),
        "the album was not asked again for what was forgotten"
    );
    Ok(())
}

#[test]
fn forgetting_what_was_fetched_keeps_a_cover_the_files_carry() -> Result<()> {
    let tree = Tree::new();
    let art = png(64);

    tree.write(
        "1.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, "Paper Boats")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .picture(&art)
            .build(),
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let album = only_album(&library)?;
    let track = the_row_at(&library, &tree.path().join("1.wav"))?.id;
    let before = library.cover_art(album.id)?;

    assert!(before.is_some());
    assert_eq!(library.forget_what_was_fetched_for(track)?, Some(album.id));
    assert_eq!(library.cover_art(album.id)?, before);
    Ok(())
}

#[test]
fn a_file_cover_found_on_a_rescan_replaces_one_the_archive_gave() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    assert!(library.land_archive_cover(
        album.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: png(128)
        }
    )?);
    assert!(
        !library.land_archive_cover(
            album.id,
            &CoverArt {
                format: ImageFormat::Png,
                bytes: png(64)
            }
        )?,
        "an archive cover was laid over the one already held"
    );

    let art = png(512);

    tree.write(
        "3.wav",
        &Wav::new()
            .frames(8_820)
            .text(TITLE, "Paper Boats!")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .picture(&art)
            .build(),
    );
    scan(&library, &options(&tree))?;

    assert_eq!(
        library.release_of(album.id)?.map(|held| held.cover_source),
        Some(CoverSource::File)
    );
    assert_eq!(
        library.cover_art(album.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: art,
        })
    );
    Ok(())
}

#[test]
fn release_rows_are_matched_by_recording_id_then_title_then_position_and_the_rest_are_missing()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![
                release_row(1, "Small Mercies", Vec::new()),
                release_row(2, "A Quilt of Winds", Vec::new()),
                release_row(3, "Paper Boats", Vec::new()),
                release_row(4, "Harbour Lights", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;

    assert_eq!(library.rematch(album.id)?, 3);

    let by_title: Vec<(String, Option<String>)> = library
        .release_tracks(album.id)?
        .into_iter()
        .map(|row| {
            let matched = row
                .track
                .map(|id| library.track(id))
                .transpose()?
                .flatten()
                .map(|track| track.title);
            Ok((row.title, matched))
        })
        .collect::<Result<_>>()?;

    assert_eq!(
        by_title,
        vec![
            ("Small Mercies".to_owned(), Some("Small Mercies".to_owned())),
            (
                "A Quilt of Winds".to_owned(),
                Some("A Quilt of Winds".to_owned())
            ),
            ("Paper Boats".to_owned(), Some("Paper Boats!".to_owned())),
            ("Harbour Lights".to_owned(), None),
        ]
    );
    assert_eq!(only_album(&library)?.missing, 1);
    Ok(())
}

#[test]
fn a_pressing_in_another_order_pairs_each_row_with_the_song_of_its_title() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let album = only_album(&library)?;
    let mut swapped = release_row(1, "A Quilt of Winds", Vec::new());

    swapped.recording = Some(mbid(RECORDING));
    library.land_release(
        album.id,
        &orbits(
            vec![
                swapped,
                release_row(2, "Small Mercies", Vec::new()),
                release_row(3, "Paper Boats", Vec::new()),
            ],
            Vec::new(),
        ),
    )?;
    library.rematch(album.id)?;

    for row in library.release_tracks(album.id)? {
        let paired = row
            .track
            .map(|id| library.track(id))
            .transpose()?
            .flatten()
            .expect("every row has its song");

        assert_eq!(
            entitled(&paired.title),
            entitled(&row.title),
            "a row was paired by its place with its neighbour"
        );
    }

    assert_eq!(
        recorded(&database, &tree.path().join("2.wav")).as_deref(),
        Some(RECORDING),
        "the recording id was stamped on the neighbour at its place"
    );
    assert_eq!(recorded(&database, &tree.path().join("1.wav")), None);
    Ok(())
}

fn entitled(title: &str) -> String {
    title.trim_end_matches('!').to_owned()
}

#[test]
fn every_link_a_release_and_its_recordings_carry_is_kept_with_its_provider() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let album = only_album(&library)?;

    library.land_release(
        album.id,
        &orbits(
            vec![release_row(
                1,
                "Small Mercies",
                vec![
                    Link::new(
                        "streaming",
                        "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
                    ),
                    Link::new(
                        "lyrics",
                        "https://genius.com/Orbiters-one-lyrics".to_owned(),
                    ),
                ],
            )],
            vec![
                Link::new("streaming", "https://tidal.com/album/55391740".to_owned()),
                Link::new(
                    "purchase for download",
                    "https://www.7digital.com/artist/orbiters/release/orbits".to_owned(),
                ),
                Link::new("discogs", "https://www.discogs.com/release/1234".to_owned()),
            ],
        ),
    )?;

    let release = library.release_of(album.id)?.expect("the album is known");

    assert_eq!(release.mbid, Some(mbid(RELEASE)));
    assert_eq!(release.group, Some(mbid(RELEASE_GROUP)));
    assert_eq!(release.label.as_deref(), Some("Harvest"));
    assert_eq!(release.barcode.as_deref(), Some("724382920021"));

    let providers: Vec<(Relation, Service)> = release
        .links
        .iter()
        .map(|link| (link.relation, link.service))
        .collect();

    assert_eq!(
        providers,
        vec![
            (Relation::Streaming, Service::Tidal),
            (Relation::PurchaseForDownload, Service::SevenDigital),
            (Relation::Discogs, Service::Discogs),
        ]
    );

    let rows = library.release_tracks(album.id)?;

    assert_eq!(rows.len(), 1);

    let row = &rows[0];

    assert_eq!(row.length, Some(Duration::from_secs(1)));
    assert_eq!(
        row.links,
        vec![
            Link {
                relation: Relation::Lyrics,
                service: Service::Other,
                url: "https://genius.com/Orbiters-one-lyrics".to_owned(),
            },
            Link {
                relation: Relation::Streaming,
                service: Service::Spotify,
                url: "https://open.spotify.com/track/0Ga3szKsJOeZ0eAfydm1WV".to_owned(),
            },
        ]
    );
    Ok(())
}

#[test]
fn an_artist_profile_genres_links_and_portrait_are_written_and_read_back_whole() -> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let artist = artist_named(&library, "The Orbiters")?;

    assert!(library.artists_to_ask(WAITED, false)?.contains(&artist.id));
    assert_eq!(
        library.artist_detail(artist.id)?.map(|held| held.mbid),
        Some(None)
    );

    let profile = orbiters();

    assert_eq!(
        resonate_library::portrait_urls(&profile.links).collect::<Vec<&str>>(),
        vec![ORBITERS_PICTURE]
    );

    library.land_artist(artist.id, &profile)?;

    let portrait = png(96);

    assert!(library.land_portrait(
        artist.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: portrait.clone()
        }
    )?);
    assert!(!library.land_portrait(
        artist.id,
        &CoverArt {
            format: ImageFormat::Png,
            bytes: png(32)
        }
    )?);

    let detail = library
        .artist_detail(artist.id)?
        .expect("the artist is known");

    assert_eq!(detail.mbid, Some(profile.mbid.clone()));
    assert_eq!(detail.sort_name, profile.sort_name);
    assert_eq!(detail.kind, profile.kind);
    assert_eq!(detail.country, profile.country);
    assert_eq!(detail.area, profile.area);
    assert_eq!(detail.began_in, profile.began_in);
    assert_eq!(detail.span, profile.span);
    assert_eq!(detail.disambiguation, profile.disambiguation);
    assert_eq!(detail.genres, profile.genres);
    assert!(detail.asked.is_some() && detail.answered.is_some());
    assert_eq!(
        detail
            .links
            .iter()
            .map(|link| link.service)
            .collect::<Vec<_>>(),
        vec![Service::WikimediaCommons, Service::Spotify, Service::Other]
    );

    let again = artist_named(&library, "The Orbiters")?;

    assert_eq!(again.mbid, Some(mbid(ORBITERS)));
    assert!(again.has_portrait);
    assert_eq!(
        library.portrait(artist.id)?,
        Some(CoverArt {
            format: ImageFormat::Png,
            bytes: portrait,
        })
    );
    assert!(!library.artists_to_ask(WAITED, false)?.contains(&artist.id));

    library.write_artist_mbid(artist.id, &mbid(ADA))?;

    assert_eq!(
        artist_named(&library, "The Orbiters")?.mbid,
        Some(mbid(ADA))
    );
    Ok(())
}

#[test]
fn a_lookup_fetches_every_track_its_lyrics_and_keeps_them_against_the_file() -> Result<()> {
    let (tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned {
        lyrics: vec![
            ("Small Mercies", timed_words("[00:01.00]Small mercies")),
            ("A Quilt of Winds", words("A blanket of feathers")),
        ],
        ..Canned::default()
    }));

    let summary = enrich(&library, &fake, false)?;

    assert_eq!(summary.stats.lyrics, 2);
    assert_eq!(summary.stats.lyrics_missing, 1);

    let mut asked = fake.sung();

    asked.sort_by(|one, other| one.title.cmp(&other.title));

    assert_eq!(
        asked
            .iter()
            .map(|asked| asked.title.as_str())
            .collect::<Vec<_>>(),
        ["A Quilt of Winds", "Paper Boats!", "Small Mercies"]
    );
    assert!(asked.iter().all(|asked| asked.artist == "The Orbiters"));
    assert!(
        asked
            .iter()
            .all(|asked| asked.album.as_deref() == Some("Orbits"))
    );
    assert!(asked.iter().all(|asked| asked.length.is_some()));

    let kept = |file: &str| -> Result<Option<LyricText>> {
        Ok(library
            .kept_lyrics(&orbits_file(&tree, file), None)?
            .and_then(|kept| kept.sung))
    };

    assert_eq!(kept("1.wav")?, Some(timed_words("[00:01.00]Small mercies")));
    assert_eq!(kept("2.wav")?, Some(words("A blanket of feathers")));
    assert!(
        library
            .kept_lyrics(&orbits_file(&tree, "3.wav"), None)?
            .is_some_and(|kept| kept.sung.is_none()),
        "the miss was not remembered"
    );
    assert_eq!(matching(&library, "lyrics:feathers")?, ["A Quilt of Winds"]);

    let again = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &again, false)?;

    assert!(
        again.sung().is_empty(),
        "a track kept lately was asked again"
    );

    let refreshed = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &refreshed, true)?;

    assert_eq!(refreshed.sung().len(), 3);
    assert_eq!(
        kept("1.wav")?,
        Some(timed_words("[00:01.00]Small mercies")),
        "a refresh that heard nothing put out what was kept"
    );
    Ok(())
}

#[test]
fn a_set_short_of_a_lyricsfile_is_asked_about_again_once_it_has_aged_and_bettered() -> Result<()> {
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let plainly = Arc::new(Fake::new(Canned {
        lyrics: vec![("Small Mercies", words("Small mercies"))],
        ..Canned::default()
    }));

    enrich(&library, &plainly, false)?;

    beside(&database)
        .execute(
            "UPDATE lyrics_kept SET taken = ?1",
            [i64::try_from((BETTERED_AFTER * 2).as_nanos()).expect("fits")],
        )
        .expect("the kept rows age");

    let documented = LyricText {
        lyricsfile: Some("version: '1.0'".to_owned()),
        ..timed_words("[00:01.00]Small mercies")
    };
    let better = Arc::new(Fake::new(Canned {
        lyrics: vec![("Small Mercies", documented.clone())],
        ..Canned::default()
    }));
    let summary = enrich(&library, &better, false)?;

    assert_eq!(better.sung().len(), 3);
    assert_eq!(summary.stats.lyrics, 1);
    assert_eq!(
        library
            .kept_lyrics(&orbits_file(&tree, "1.wav"), None)?
            .and_then(|kept| kept.sung),
        Some(documented)
    );

    beside(&database)
        .execute("UPDATE lyrics_kept SET taken = 0", [])
        .expect("the kept rows age");

    let settled = Arc::new(Fake::new(Canned::default()));

    enrich(&library, &settled, false)?;

    assert_eq!(
        settled.sung().len(),
        2,
        "a track holding a lyricsfile was asked again"
    );
    Ok(())
}

#[test]
fn a_lookup_told_not_to_fetch_lyrics_asks_for_none_and_one_that_cannot_reach_the_service_stops_asking()
-> Result<()> {
    let (_tree, library) = scanned_orbits()?;
    let fake = Arc::new(Fake::new(Canned::default()));

    library
        .enrich(
            Arc::clone(&fake) as Arc<dyn Reference>,
            Arc::new(Fingerprinters::none()),
            EnrichOptions {
                lyrics: false,
                ..EnrichOptions::default()
            },
        )?
        .join()?;

    assert!(fake.sung().is_empty());

    let unreachable =
        Arc::new(Fake::new(Canned::default()).faulting(LookupOp::Lyrics, 0, Fault::Unreachable));
    let summary = enrich(&library, &unreachable, false)?;

    assert_eq!(unreachable.sung().len(), 1);
    assert_eq!(summary.stopped_by, None);

    let refusing =
        Arc::new(Fake::new(Canned::default()).faulting(LookupOp::Lyrics, 0, Fault::Refused));
    let summary = enrich(&library, &refusing, false)?;

    assert_eq!(refusing.sung().len(), 3);
    assert_eq!(summary.stats.refused, 1);
    Ok(())
}

#[test]
fn a_lyrics_offset_is_held_per_row_and_goes_with_the_track() -> Result<()> {
    let tree = Tree::new();
    let path = tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Ada")
            .build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let whole = MediaLocation::local(&path);
    let cut = Some(FrameSpan::starting(Frames(44_100)));
    let ahead = LyricsAhead::ZERO.earlier().earlier().earlier();

    assert_eq!(library.lyrics_ahead(&whole, None)?, LyricsAhead::ZERO);

    library.hold_lyrics_ahead(&whole, None, ahead)?;

    assert_eq!(library.lyrics_ahead(&whole, None)?, ahead);
    assert_eq!(library.lyrics_ahead(&whole, cut)?, LyricsAhead::ZERO);

    library.hold_lyrics_ahead(&whole, None, LyricsAhead::ZERO)?;

    assert_eq!(library.lyrics_ahead(&whole, None)?, LyricsAhead::ZERO);

    library.hold_lyrics_ahead(&whole, None, ahead.later().later().later().later())?;

    assert_eq!(library.lyrics_ahead(&whole, None)?.millis(), -100);

    fs::remove_file(&path).expect("the file goes");
    library.forget_the_gone(std::slice::from_ref(&path))?;

    assert_eq!(library.lyrics_ahead(&whole, None)?, LyricsAhead::ZERO);
    Ok(())
}

#[test]
fn a_version_chosen_for_a_song_is_kept_until_forgotten_and_goes_with_either_track() -> Result<()> {
    let tree = Tree::new();
    let opus = tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Ada")
            .build(),
    );
    let flac = tree.write(
        "elsewhere/lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Ada")
            .build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let song = library.track_at(&opus, None)?.expect("the song is held").id;
    let copy = library.track_at(&flac, None)?.expect("the copy is held").id;

    assert_eq!(library.version_chosen_for(song)?, None);
    assert!(library.choose_version(song, copy)?);
    assert_eq!(library.version_chosen_for(song)?, Some(copy));
    assert_eq!(
        library.versions_chosen()?,
        [ChosenVersion { track: song, copy }]
    );

    assert_eq!(
        library.versions_chosen_among(&[song])?,
        library.versions_chosen()?
    );
    assert_eq!(library.versions_chosen_among(&[copy])?, []);
    assert_eq!(library.versions_chosen_among(&[])?, []);

    assert!(library.choose_version(song, song)?);
    assert_eq!(library.version_chosen_for(song)?, None);

    let unheld = resonate_core::TrackId::new(9_999).expect("an id");

    assert!(matches!(
        library.choose_version(unheld, copy),
        Err(Error::UnknownTrack(id)) if id == unheld
    ));

    library.choose_version(song, copy)?;

    assert!(library.forget_the_version_chosen(song)?);
    assert!(!library.forget_the_version_chosen(song)?);

    library.choose_version(song, copy)?;
    fs::remove_file(&flac).expect("the copy goes");
    library.forget_the_gone(std::slice::from_ref(&flac))?;

    assert_eq!(library.version_chosen_for(song)?, None);
    assert!(
        library
            .variants_of(song)?
            .expect("the song stands")
            .held
            .is_empty()
    );
    Ok(())
}

#[test]
fn a_song_with_a_version_chosen_is_played_as_that_version_only_where_the_row_is_the_song()
-> Result<()> {
    let tree = Tree::new();
    let opus = tree.write(
        "lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Ada")
            .build(),
    );
    let flac = tree.write(
        "elsewhere/lantern.wav",
        &Wav::new()
            .text(TITLE, "Lantern")
            .text(ARTIST, "Ada")
            .build(),
    );
    let time = tree.write(
        "time.wav",
        &Wav::new().text(TITLE, "Time").text(ARTIST, "Ada").build(),
    );
    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let song = library.track_at(&opus, None)?.expect("the song is held").id;
    let copy = library.track_at(&flac, None)?.expect("the copy is held").id;
    let other = library
        .track_at(&time, None)?
        .expect("the other is held")
        .id;
    let song_at = MediaLocation::local(&opus);
    let other_at = MediaLocation::local(&time);

    assert!(library.chosen_among(&[song, other])?.is_empty());

    library.choose_version(song, copy)?;

    let chosen = library.chosen_among(&[song, other, song])?;

    assert_eq!(
        chosen.in_place_of(song, &song_at, None),
        Some(&Played {
            track: copy,
            cut: Cut::whole(MediaLocation::local(&flac)),
        })
    );
    assert_eq!(chosen.in_place_of(other, &other_at, None), None);
    assert_eq!(
        chosen.in_place_of(
            song,
            &MediaLocation::local("/elsewhere/unscanned.wav"),
            None
        ),
        None
    );
    assert_eq!(
        chosen.in_place_of(copy, &MediaLocation::local(&flac), None),
        None
    );
    assert!(library.chosen_among(&[copy])?.is_empty());
    Ok(())
}

#[test]
fn kept_lyrics_are_keyed_by_path_and_span_and_a_miss_is_remembered() -> Result<()> {
    let library = Library::open_in_memory()?;
    let whole = MediaLocation::local("/music/Orbits/Lantern.flac");
    let cut = Some(FrameSpan::starting(Frames(44_100)));

    assert_eq!(library.kept_lyrics(&whole, None)?, None);
    assert!(library.keep_lyrics(
        &whole,
        None,
        Some(&timed_words("[00:01.00]Sunlight the fireflies"))
    )?);
    assert!(!library.keep_lyrics(&whole, cut, None)?);

    let kept = library
        .kept_lyrics(&whole, None)?
        .expect("the lyrics were kept");

    assert_eq!(
        kept.sung,
        Some(timed_words("[00:01.00]Sunlight the fireflies"))
    );

    let miss = library
        .kept_lyrics(&whole, cut)?
        .expect("the miss was kept");

    assert_eq!(miss.sung, None);

    assert!(!library.keep_lyrics(&whole, None, Some(&words("Sunlight the fireflies")))?);

    let held = library
        .kept_lyrics(&whole, None)?
        .expect("the lyrics were kept");

    assert_eq!(
        held.sung,
        Some(timed_words("[00:01.00]Sunlight the fireflies")),
        "a synced set was traded for plain words"
    );
    assert!(held.taken >= kept.taken);

    assert!(!library.keep_lyrics(&whole, None, None)?);
    assert_eq!(
        library
            .kept_lyrics(&whole, None)?
            .and_then(|kept| kept.sung),
        Some(timed_words("[00:01.00]Sunlight the fireflies")),
        "a miss put out what was kept"
    );

    let documented = LyricText {
        lyricsfile: Some("version: '1.0'".to_owned()),
        ..timed_words("[00:01.00]Sunlight the fireflies")
    };

    assert!(library.keep_lyrics(&whole, None, Some(&documented))?);
    assert_eq!(
        library
            .kept_lyrics(&whole, None)?
            .and_then(|kept| kept.sung),
        Some(documented)
    );

    let elsewhere = MediaLocation::new(SourceId::new("tidal")?, "55391743");

    assert!(matches!(
        library.kept_lyrics(&elsewhere, None),
        Err(Error::NotALocalFile { .. })
    ));
    assert!(matches!(
        library.keep_lyrics(&elsewhere, None, None),
        Err(Error::NotALocalFile { .. })
    ));
    Ok(())
}

#[test]
fn kept_lyrics_go_with_a_file_that_goes_or_is_replaced_and_stay_with_one_left_alone() -> Result<()>
{
    let (tree, library, database) = scanned_orbits_on_disk()?;
    let first = orbits_file(&tree, "1.wav");
    let second = orbits_file(&tree, "2.wav");
    let third = orbits_file(&tree, "3.wav");

    for location in [&first, &second, &third] {
        library.keep_lyrics(location, None, Some(&words("Sunlight the fireflies")))?;
    }

    let refusals = |database: &Path| -> i64 {
        beside(database)
            .query_row("SELECT count(*) FROM lyrics_refused", [], |row| row.get(0))
            .expect("the refusals count")
    };

    for location in [&first, &second, &third] {
        beside(&database)
            .execute(
                "INSERT INTO lyrics_refused (path, span_start, refused, refusals) VALUES (?1, 0, 1, 1)",
                [location.as_path().expect("a local file").to_str().expect("a path")],
            )
            .expect("a refusal is written");
    }

    let is_kept = |location: &MediaLocation| -> Result<bool> {
        Ok(library.kept_lyrics(location, None)?.is_some())
    };

    scan(&library, &options(&tree))?;

    assert!(is_kept(&first)? && is_kept(&second)? && is_kept(&third)?);
    assert_eq!(refusals(&database), 3);

    tree.write(
        "1.wav",
        &Wav::new()
            .text(TITLE, "A Different Song")
            .text(ARTIST, "The Orbiters")
            .text(ALBUM, "Orbits")
            .text(TRACK, "1")
            .build(),
    );
    fs::remove_file(tree.path().join("2.wav")).expect("a removable file");
    scan(&library, &options(&tree))?;

    assert!(
        !is_kept(&first)?,
        "a file replaced at the same path kept the old song's words"
    );
    assert!(!is_kept(&second)?, "a file that went kept its lyrics");
    assert!(is_kept(&third)?, "a file left alone lost its lyrics");
    assert_eq!(refusals(&database), 1);
    Ok(())
}

fn flat_gains() -> resonate_analysis::BandGains {
    resonate_analysis::BandGains::of(
        &resonate_analysis::Applied::from(Arc::new(resonate_core::eq::Profile::flat())),
        resonate_core::SampleRate::HZ_44100,
    )
    .expect("gains under a flat curve")
}

fn whole_cut(location: &MediaLocation) -> resonate_analysis::Cut {
    resonate_analysis::Cut {
        location: location.clone(),
        span: None,
    }
}

#[test]
fn a_study_the_enrichment_keeps_holds_the_bands_its_music_sounds_in() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let location = MediaLocation::local(&file);

    let loudest = library.loudest_kept(&flat_gains())?;

    assert_eq!(loudest.measured, 1);
    assert!(loudest.peak > 0.5, "{}", loudest.peak);
    assert_eq!(library.unmeasured(&[whole_cut(&location)])?, vec![]);
    Ok(())
}

#[test]
fn a_study_kept_without_its_bands_is_measured_alone_and_keeps_everything_else_it_held() -> Result<()>
{
    let tree = Tree::new();
    let file = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let location = MediaLocation::local(&file);
    let studied = library
        .study_of(&location, None)?
        .expect("the enrichment studied the file");

    beside(&database)
        .execute(
            "UPDATE track_studies SET band_levels = NULL, band_peaks = NULL",
            [],
        )
        .expect("the study reads as one kept before its bands were measured");

    assert_eq!(
        library.unmeasured(&[whole_cut(&location)])?,
        vec![whole_cut(&location)]
    );
    assert_eq!(library.loudest_kept(&flat_gains())?.measured, 0);

    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    assert_eq!(library.unmeasured(&[whole_cut(&location)])?, vec![]);
    assert_eq!(
        library.study_of(&location, None)?,
        Some(studied),
        "measuring the bands studied the file again"
    );
    Ok(())
}

#[test]
fn bands_that_cannot_be_measured_are_kept_as_failed_and_not_decoded_again_on_every_pass()
-> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;
    beside(&database)
        .execute(
            "UPDATE track_studies SET band_levels = NULL, band_peaks = NULL",
            [],
        )
        .expect("the study reads as one kept before its bands were measured");
    fs::write(&file, b"no longer audio").expect("the file broken");

    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let kept_as_failed: i64 = beside(&database)
        .query_row("SELECT count(*) FROM unstudied", [], |row| row.get(0))
        .expect("the failed studies counted");

    assert_eq!(kept_as_failed, 1);
    Ok(())
}

#[test]
fn a_study_kept_without_its_peaks_is_measured_alone_and_is_not_fitted_from_until_it_is()
-> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let location = MediaLocation::local(&file);
    let kept = library.loudest_kept(&flat_gains())?.peak;

    beside(&database)
        .execute("UPDATE track_studies SET band_peaks = NULL", [])
        .expect("the study reads as one kept before its peaks were measured");

    assert_eq!(
        library.unmeasured(&[whole_cut(&location)])?,
        vec![whole_cut(&location)]
    );
    assert_eq!(library.loudest_kept(&flat_gains())?.measured, 0);

    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    assert_eq!(library.unmeasured(&[whole_cut(&location)])?, vec![]);

    let measured = library.loudest_kept(&flat_gains())?;

    assert_eq!(measured.measured, 1);
    assert!(
        (measured.peak - kept).abs() < 1e-6,
        "{} against {kept}",
        measured.peak
    );
    Ok(())
}

#[test]
fn peaks_kept_before_what_came_together_still_fit_and_are_measured_again() -> Result<()> {
    let tree = Tree::new();
    let file = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let location = MediaLocation::local(&file);
    let length_kept = || -> i64 {
        beside(&database)
            .query_row("SELECT length(band_peaks) FROM track_studies", [], |row| {
                row.get(0)
            })
            .expect("a kept row")
    };

    beside(&database)
        .execute(
            "UPDATE track_studies SET band_peaks = substr(band_peaks, 1, ?1)",
            [resonate_analysis::BAND_PEAKS_BYTES_BEFORE_TOGETHER as i64],
        )
        .expect("the study reads as one kept by the build before");

    assert_eq!(library.unmeasured(&[whole_cut(&location)])?, vec![]);
    assert_eq!(library.loudest_kept(&flat_gains())?.measured, 1);

    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    assert_eq!(length_kept(), resonate_analysis::BAND_PEAKS_BYTES as i64);
    assert_eq!(library.loudest_kept(&flat_gains())?.measured, 1);
    Ok(())
}

#[test]
fn a_song_no_study_holds_is_left_to_be_decoded_and_one_it_does_is_not() -> Result<()> {
    let tree = Tree::new();
    let studied = tree.write("1.wav", &Wav::new().frames(4 * RATE as usize).build());
    let database = tree.path().join("library.db");
    let library = Library::open(&database)?;

    scan(&library, &options(&tree))?;
    enrich(&library, &Arc::new(Fake::new(Canned::default())), false)?;

    let unscanned = MediaLocation::local(tree.path().join("elsewhere.wav"));

    let owed = library.unmeasured(&[
        whole_cut(&MediaLocation::local(&studied)),
        whole_cut(&unscanned),
    ])?;

    assert_eq!(owed, vec![whole_cut(&unscanned)]);
    Ok(())
}
