use std::{fs, path::Path};

use resonate_core::{MediaLocation, PlaylistId, path_text};
use resonate_library::{
    AlbumQuery, Cut, Direction, Error, Library, PlaylistFormat, PlaylistOrder, Result,
    TextEncoding, TrackQuery,
};

use crate::support::{
    ALBUM, ALBUM_ARTIST, ARTIST, TITLE, TRACK, Tree, Wav, options, scan, scanned_playlist_tree,
    scanned_sheet, stems,
};

const DISC: &[u8; 4] = b"TPOS";

#[test]
fn a_playlist_written_out_carries_what_a_reader_needs_and_comes_back_whole() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["b.wav", "gone.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.m3u8");
    let written = library.export_playlist(id, &sheet)?;

    assert_eq!(written.rows, 3);
    assert_eq!(written.format, PlaylistFormat::M3u);

    let written = fs::read_to_string(&sheet).expect("the sheet was written");

    assert!(
        written.starts_with("#EXTM3U\n#PLAYLIST:Evening\n"),
        "{written}"
    );
    assert!(
        written.contains("\nb.wav\n"),
        "a file beside the sheet was not named relative to it: {written}"
    );
    assert!(written.contains(",Dogs\n"), "{written}");
    assert!(
        written.contains("#EXTINF:-1,gone\n"),
        "a row no scan has seen lost its name: {written}"
    );

    let read_back = library.import_playlist(&sheet, Some("Evening again"))?;

    assert_eq!(read_back.name, "Evening again");
    assert_eq!(read_back.added, 3);
    assert_eq!(read_back.elsewhere, 0);
    assert_eq!(
        read_back.missing, 1,
        "a row whose file is gone was not counted"
    );
    assert_eq!(
        stems(&library.playlist_cuts(read_back.id)?),
        vec!["b", "gone", "a"]
    );
    Ok(())
}

#[test]
fn a_sheet_another_player_wrote_names_itself_and_counts_what_it_could_not_take() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("from-another-player.m3u");

    fs::write(
        &sheet,
        format!(
            "#EXTM3U\n\
             #PLAYLIST:   Someone else's evening   \n\
             \n\
             # a line no tag defines\n\
             #EXTINF:213,Example Band - Lantern\n\
             a.wav\n\
             {root}/b.wav\n\
             http://stream.example/live.ogg\n\
             {plain}/c.wav\n",
            root = uri_of(tree.path()),
            plain = path_text::plain(tree.path()).display()
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.name, "Someone else's evening");
    assert_eq!(imported.added, 3);
    assert_eq!(imported.elsewhere, 1, "a stream was read as a file");
    assert_eq!(imported.missing, 0);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "c"]
    );

    let again = library.import_playlist(&sheet, None)?;

    assert_eq!(
        again.id, imported.id,
        "a second import made a second playlist"
    );
    assert_eq!(again.added, 0, "a second import added rows again");
    assert_eq!(again.already, 3, "the rows already held were not counted");
    assert_eq!(
        library.playlist(imported.id)?.map(|found| found.entries),
        Some(3),
        "importing the same sheet twice doubled the playlist"
    );
    Ok(())
}

#[test]
fn a_sheet_written_on_windows_resolves_its_backslashed_rows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();

    fs::create_dir_all(tree.path().join("lists")).expect("a writable temporary folder");

    let sheet = tree.path().join("lists/windows.m3u");

    fs::write(&sheet, "#EXTM3U\r\n..\\a.wav\r\n..\\b.wav\r\n").expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 2);
    assert_eq!(
        imported.missing, 0,
        "a backslashed row read as a missing file"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_sheet_from_another_drive_layout_is_reconnected_by_the_one_file_its_trailing_folders_name()
-> Result<()> {
    let tree = Tree::new();

    for relative in [
        "Artist/Album/one.wav",
        "Artist/Album/two.wav",
        "Other/Album/one.wav",
    ] {
        tree.write(relative, &Wav::new().frames(4_410).build());
    }

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let sheet = tree.path().join("windows.m3u");

    fs::write(
        &sheet,
        "#EXTM3U\r\nD:\\Music\\Artist\\Album\\two.wav\r\n\
         D:\\Music\\Album\\one.wav\r\n\
         D:\\Music\\Other\\Album\\ONE.WAV\r\n\
         D:\\Music\\Nothing\\gone.wav\r\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 4);
    assert_eq!(
        imported.missing, 2,
        "the row two files answered to, and the one nothing answered to, are the missing"
    );

    let cuts = library.playlist_cuts(imported.id)?;

    assert_eq!(
        cuts[0].location.as_path(),
        Some(tree.path().join("Artist/Album/two.wav").as_path())
    );
    assert_eq!(
        cuts[2].location.as_path(),
        Some(tree.path().join("Other/Album/one.wav").as_path()),
        "a name differing only in case was not the file"
    );
    assert_ne!(
        cuts[1].location.as_path(),
        Some(tree.path().join("Artist/Album/one.wav").as_path()),
        "a row two files could be was taken for one of them"
    );
    Ok(())
}

fn deluxe_disc_tree(tree: &Tree, numbering: [(u32, &str); 5]) {
    for (index, (disc, track)) in numbering.into_iter().enumerate() {
        tree.write(
            &format!("{index}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Song {index}"))
                .text(ARTIST, "The Orbiters")
                .text(ALBUM_ARTIST, "The Orbiters")
                .text(ALBUM, "Orbits Deluxe")
                .text(DISC, &format!("{disc}/2"))
                .text(TRACK, track)
                .build(),
        );
    }
}

fn asked_tracks_of_the_only_album(library: &Library) -> Result<Option<u32>> {
    let albums = library.albums(&AlbumQuery::default())?;
    let album = albums.first().expect("the rip was grouped");
    Ok(library
        .album_to_ask(album.id)?
        .expect("the album is known")
        .tagged_tracks)
}

#[test]
fn a_set_declaring_its_track_count_a_disc_at_a_time_is_asked_after_as_the_whole_it_adds_up_to()
-> Result<()> {
    let tree = Tree::new();

    deluxe_disc_tree(
        &tree,
        [(1, "1/3"), (1, "2/3"), (1, "3/3"), (2, "1/2"), (2, "2/2")],
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(asked_tracks_of_the_only_album(&library)?, Some(5));
    Ok(())
}

#[test]
fn a_set_numbered_straight_on_across_its_discs_is_asked_after_as_the_count_each_file_declares()
-> Result<()> {
    let tree = Tree::new();

    deluxe_disc_tree(
        &tree,
        [(1, "1/5"), (1, "2/5"), (1, "3/5"), (2, "4/5"), (2, "5/5")],
    );

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    assert_eq!(asked_tracks_of_the_only_album(&library)?, Some(5));
    Ok(())
}

#[test]
fn a_sheet_naming_a_file_in_another_case_beyond_ascii_still_reconnects_it() -> Result<()> {
    let tree = Tree::new();

    tree.write("Artist/Album/étude.wav", &Wav::new().frames(4_410).build());

    let library = Library::open_in_memory()?;

    scan(&library, &options(&tree))?;

    let sheet = tree.path().join("windows.m3u");

    fs::write(&sheet, "#EXTM3U\r\nD:\\Music\\Artist\\Album\\ÉTUDE.WAV\r\n")
        .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.missing, 0);
    assert_eq!(
        library.playlist_cuts(imported.id)?[0].location.as_path(),
        Some(tree.path().join("Artist/Album/étude.wav").as_path())
    );
    Ok(())
}

#[test]
fn a_sheet_that_names_one_file_twice_keeps_both_rows() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("doubled.m3u8");

    fs::write(&sheet, "#EXTM3U\na.wav\nb.wav\na.wav\n").expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;

    assert_eq!(
        imported.added, 3,
        "a file the sheet itself repeated was lost"
    );
    assert_eq!(imported.already, 0);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "a"]
    );

    let again = library.import_playlist(&sheet, Some("Evening"))?;

    assert_eq!(again.added, 0, "the same sheet read twice was not a no-op");
    assert_eq!(again.already, 3);
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "a"]
    );
    Ok(())
}

#[test]
#[cfg(unix)]
fn a_row_a_sheet_cannot_name_plainly_is_written_as_a_uri() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let awkward = ["#hash.wav", "two\nlines.wav", "plain.wav"];

    for file in awkward {
        tree.write(file, b"a file no scan has read");
    }

    let id = library.start_playlist(
        "Awkward",
        &awkward
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    for (name, format) in [("awkward.m3u8", "M3U"), ("awkward.pls", "PLS")] {
        let sheet = tree.path().join(name);

        library.export_playlist(id, &sheet)?;

        let read_back = library.import_playlist(&sheet, Some(format))?;

        assert_eq!(
            read_back.added, 3,
            "{format} lost a row whose path it could not write plainly"
        );
        assert_eq!(read_back.elsewhere, 0);
        assert_eq!(read_back.missing, 0);
        assert_eq!(
            stems(&library.playlist_cuts(read_back.id)?),
            vec!["#hash", "two\nlines", "plain"]
        );
    }
    Ok(())
}

#[test]
fn a_playlist_of_cue_rows_exports_and_imports_as_the_rows_it_holds() -> Result<()> {
    let (tree, library) = scanned_sheet();
    let cuts: Vec<Cut> = library
        .tracks(&TrackQuery::default())?
        .into_iter()
        .map(|row| Cut {
            location: row.location,
            span: row.span,
        })
        .collect();

    assert!(cuts.iter().all(|cut| cut.span.is_some()));

    let id = library.start_playlist("Tideline", &cuts)?;

    for (name, format) in [("tideline.m3u8", "M3U"), ("tideline.xspf", "XSPF")] {
        let sheet = tree.path().join(name);

        library.export_playlist(id, &sheet)?;

        let written = fs::read_to_string(&sheet).expect("a written sheet");

        assert!(
            written.contains("start-time="),
            "{format} carried no cut: {written}"
        );

        let read_back = library.import_playlist(&sheet, Some(format))?;

        assert_eq!(read_back.added, 3, "{format}");
        assert_eq!(
            library.playlist_cuts(read_back.id)?,
            cuts,
            "{format} did not read back the rows it wrote"
        );
    }
    Ok(())
}

#[test]
fn a_row_whose_file_is_not_there_is_still_stored_as_an_absolute_path() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.path().join("beneath/list.m3u8");

    tree.write("beneath/list.m3u8", b"#EXTM3U\n../missing.wav\n");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;

    assert_eq!(imported.added, 1);
    assert_eq!(imported.missing, 1);
    assert_eq!(
        library.playlist_cuts(imported.id)?,
        vec![Cut::whole(MediaLocation::local(
            tree.path().join("missing.wav")
        ))],
        "a row no mount answered for kept the parent segments it was written with"
    );
    Ok(())
}

#[test]
fn a_row_holding_an_escaped_nul_names_no_file() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.path().join("nul.m3u8");

    fs::write(
        &sheet,
        format!(
            "#EXTM3U\n{root}/a%00b.wav\n{root}/c.wav\n",
            root = uri_of(tree.path())
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, Some("Evening"))?;

    assert_eq!(imported.added, 1);
    assert_eq!(
        imported.elsewhere, 1,
        "a path holding an interior NUL was stored as a row"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["c"]);
    Ok(())
}

#[test]
fn a_playlist_file_neither_side_can_read_is_refused_by_name() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;
    let sheet = tree.write("bytes.m3u", &[0xff, 0xfe, 0x00]);

    assert!(matches!(
        library.import_playlist(&sheet, None),
        Err(Error::UnreadablePlaylistFile { .. })
    ));
    assert!(
        library
            .playlists(PlaylistOrder::Name, Direction::Ascending, None)?
            .is_empty(),
        "a sheet that would not read still named a playlist"
    );

    let unknown = PlaylistId::new(9_999)?;

    assert!(matches!(
        library.export_playlist(unknown, &tree.path().join("nothing.m3u8")),
        Err(Error::UnknownPlaylist(_))
    ));
    Ok(())
}

fn uri_of(folder: &Path) -> String {
    MediaLocation::local(folder).to_uri()
}

#[test]
fn a_playlist_goes_out_and_comes_back_as_pls() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["b.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.pls");
    let written = library.export_playlist(id, &sheet)?;

    assert_eq!(written.format, PlaylistFormat::Pls);
    assert_eq!(written.rows, 2);

    let text = fs::read_to_string(&sheet).expect("the sheet was written");

    assert!(
        text.starts_with("[playlist]\nX-GNOME-Title=Evening\n"),
        "{text}"
    );
    assert!(text.contains("File1=b.wav\n"), "{text}");
    assert!(text.contains("Title1=Dogs\n"), "{text}");
    assert!(text.ends_with("NumberOfEntries=2\nVersion=2\n"), "{text}");

    let read_back = library.import_playlist(&sheet, Some("Evening again"))?;

    assert_eq!(read_back.format, PlaylistFormat::Pls);
    assert_eq!(read_back.added, 2);
    assert_eq!(
        read_back.short, 0,
        "a sheet this player wrote disagreed with its own count"
    );
    assert_eq!(
        stems(&library.playlist_cuts(read_back.id)?),
        vec!["b", "a"],
        "the rows did not come back in the order they were numbered"
    );
    Ok(())
}

#[test]
fn a_playlist_goes_out_and_comes_back_as_xspf() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let id = library.create_playlist("Evening")?;

    library.add_to_playlist(
        id,
        &["b.wav", "a.wav"]
            .iter()
            .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
            .collect::<Vec<_>>(),
    )?;

    let sheet = tree.path().join("evening.xspf");
    let written = library.export_playlist(id, &sheet)?;

    assert_eq!(written.format, PlaylistFormat::Xspf);
    assert_eq!(written.rows, 2);

    let text = fs::read_to_string(&sheet).expect("the sheet was written");

    assert!(text.contains("<title>Evening</title>"), "{text}");
    assert!(text.contains("<location>b.wav</location>"), "{text}");
    assert!(text.contains("<title>Dogs</title>"), "{text}");

    let read_back = library.import_playlist(&sheet, None)?;

    assert_eq!(
        read_back.id, id,
        "the title the sheet declares named another playlist"
    );
    assert_eq!(read_back.added, 0);
    assert_eq!(read_back.already, 2);
    Ok(())
}

#[test]
fn an_xspf_sheet_another_player_wrote_is_read_past_its_markup() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("from-another-player.xspf");

    fs::write(
        &sheet,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <xspf:playlist version=\"1\" xmlns:xspf=\"http://xspf.org/ns/0/\">\n\
             <!-- written elsewhere, 3 > 2 -->\n\
             <xspf:title>  Someone else&apos;s evening  </xspf:title>\n\
             <xspf:trackList>\n\
             <xspf:track>\
             <xspf:location>{root}/a.wav</xspf:location>\
             <xspf:title>Lantern</xspf:title>\
             </xspf:track>\n\
             <xspf:track><xspf:location>b%2Ewav</xspf:location></xspf:track>\n\
             <xspf:track><xspf:location>http://stream.example/live.ogg</xspf:location></xspf:track>\n\
             <xspf:track><xspf:title>a row naming nothing</xspf:title></xspf:track>\n\
             </xspf:trackList>\n\
             </xspf:playlist>\n",
            root = uri_of(tree.path())
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.name, "Someone else's evening");
    assert_eq!(imported.format, PlaylistFormat::Xspf);
    assert_eq!(imported.added, 2);
    assert_eq!(imported.elsewhere, 1, "a stream was read as a file");
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
fn an_xspf_sheet_resolves_its_rows_against_the_base_it_declares() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    for file in ["discs/one/a.wav", "discs/two/b.wav", "discs/d.wav", "c.wav"] {
        tree.write(file, b"a file no scan has read");
    }

    let sheet = tree.path().join("based.xspf");

    fs::write(
        &sheet,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE playlist>\n\
         <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\" xml:base=\"discs/\">\n\
         <title>Both discs</title>\n\
         <trackList>\n\
         <track xml:base=\"one/\" annotation=\"5 > 4\"><location>a.wav</location></track>\n\
         <track xml:base=\"two/\"><location>b.wav</location></track>\n\
         <track><location>../c.wav</location></track>\n\
         <track>\
         <location>http://stream.example/live.ogg</location>\
         <location>d.wav</location>\
         </track>\n\
         <track><location>http://stream.example/only.ogg</location></track>\n\
         </trackList>\n\
         </playlist>\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.name, "Both discs");
    assert_eq!(imported.added, 4);
    assert_eq!(
        imported.elsewhere, 1,
        "a track offering nothing but a stream was not counted"
    );
    assert_eq!(
        imported.missing, 0,
        "a row did not resolve against the base its sheet declared"
    );
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "b", "c", "d"],
        "an alternate location beside a stream was not taken"
    );
    Ok(())
}

#[test]
fn an_xspf_base_naming_the_file_scheme_in_capitals_still_resolves_the_rows_under_it() -> Result<()>
{
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("discs/a.wav", b"a file no scan has read");

    let sheet = tree.path().join("shouted.xspf");

    fs::write(
        &sheet,
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\" \
             xml:base=\"FILE://LOCALHOST{root}/discs/\">\n\
             <trackList><track><location>a.wav</location></track></trackList>\n\
             </playlist>\n",
            root = uri_of(tree.path()).trim_start_matches("file://")
        ),
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 1);
    assert_eq!(imported.elsewhere, 0, "a local base was read as elsewhere");
    assert_eq!(imported.missing, 0);
    Ok(())
}

#[test]
#[cfg(unix)]
fn an_xspf_sheet_a_windows_player_wrote_resolves_with_its_separators_turned() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("discs/one/a.wav", b"a file no scan has read");
    tree.write("AC\\DC.wav", b"a file whose name holds a backslash");

    let sheet = tree.path().join("windows.xspf");

    fs::write(
        &sheet,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n\
         <trackList>\n\
         <track xml:base=\"discs\\\"><location>one\\a.wav</location></track>\n\
         <track><location>AC%5CDC.wav</location></track>\n\
         </trackList>\n\
         </playlist>\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 2);
    assert_eq!(
        imported.missing, 0,
        "a row a Windows player wrote was read as one file of that literal name"
    );
    assert_eq!(
        stems(&library.playlist_cuts(imported.id)?),
        vec!["a", "AC\\DC"]
    );
    Ok(())
}

#[test]
fn an_xspf_location_written_with_a_capital_hex_reference_reads_as_the_character() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("discs/a.wav", b"a file no scan has read");

    let sheet = tree.path().join("referenced.xspf");

    fs::write(
        &sheet,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n\
         <trackList><track><location>discs&#X2F;a.wav</location></track></trackList>\n\
         </playlist>\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 1);
    assert_eq!(
        imported.missing, 0,
        "the reference was kept as literal text"
    );
    Ok(())
}

#[test]
fn a_pls_sheet_is_weighed_against_the_count_it_declares() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    for file in ["a.wav", "b.wav"] {
        tree.write(file, b"a file no scan has read");
    }

    let sheet = tree.path().join("half.pls");

    fs::write(
        &sheet,
        "[playlist]\nX-GNOME-Title=Half a sheet\nFile1=a.wav\nFile2=b.wav\n\
         NumberOfEntries=5\nVersion=2\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.name, "Half a sheet");
    assert_eq!(imported.added, 2);
    assert_eq!(
        imported.short, 3,
        "the count the sheet declared was not weighed against what it held"
    );
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a", "b"]);
    Ok(())
}

#[test]
fn a_blank_pls_row_names_no_file_rather_than_the_sheets_own_folder() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("a.wav", b"a file no scan has read");

    let sheet = tree.path().join("blank.pls");

    fs::write(
        &sheet,
        "[playlist]\nFile1=\nFile2=a.wav\nNumberOfEntries=2\nVersion=2\n",
    )
    .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.added, 1);
    assert_eq!(imported.elsewhere, 1, "a blank row was read as a file");
    assert_eq!(imported.missing, 0);
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a"]);
    Ok(())
}

#[test]
fn a_sheet_is_read_by_what_it_holds_rather_than_what_it_is_called() -> Result<()> {
    let (tree, library) = scanned_playlist_tree();
    let sheet = tree.path().join("mislabelled.m3u");

    fs::write(&sheet, "[playlist]\nFile1=a.wav\nNumberOfEntries=1\n")
        .expect("a writable temporary file");

    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.format, PlaylistFormat::Pls);
    assert_eq!(imported.name, "mislabelled", "the stem did not name it");
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["a"]);
    Ok(())
}

#[test]
fn a_sheet_written_in_a_legacy_encoding_still_comes_in() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("S\u{f6}lvi.wav", b"a file no scan has read");

    let mut bytes = b"#EXTM3U\n#PLAYLIST:Kv".to_vec();

    bytes.push(0xe6);
    bytes.extend_from_slice(b"ld ");
    bytes.push(0x93);
    bytes.extend_from_slice(b"Live");
    bytes.push(0x94);
    bytes.extend_from_slice(b"\nS");
    bytes.push(0xf6);
    bytes.extend_from_slice(b"lvi.wav\n");

    let sheet = tree.write("legacy.m3u", &bytes);
    let imported = library.import_playlist(&sheet, None)?;

    assert_eq!(imported.encoding, TextEncoding::WINDOWS_1252);
    assert_eq!(
        imported.name, "Kvæld “Live”",
        "the bytes above Latin-1 were not read as Windows-1252"
    );
    assert_eq!(imported.added, 1);
    assert_eq!(imported.missing, 0);
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["Sölvi"]);

    let strict = tree.write("legacy.m3u8", &bytes);

    assert!(
        matches!(
            library.import_playlist(&strict, None),
            Err(Error::UnreadablePlaylistFile { .. })
        ),
        "a sheet whose name declares UTF-8 was read as something else"
    );
    Ok(())
}

#[test]
fn a_sheet_in_utf_16_or_a_cyrillic_code_page_comes_in_whole() -> Result<()> {
    let tree = Tree::new();
    let library = Library::open_in_memory()?;

    tree.write("Пример.wav", b"a file no scan has read");

    let sheet = "#EXTM3U\n#PLAYLIST:Тихая вода\nПример.wav\n";

    let mut wide = vec![0xFF, 0xFE];

    for unit in sheet.encode_utf16() {
        wide.extend_from_slice(&unit.to_le_bytes());
    }

    let imported = library.import_playlist(&tree.write("wide.m3u8", &wide), None)?;

    assert_eq!(imported.encoding, TextEncoding::Utf16Le);
    assert_eq!(imported.name, "Тихая вода");
    assert_eq!(stems(&library.playlist_cuts(imported.id)?), vec!["Пример"]);

    let cyrillic = resonate_core::text::encoded(
        &sheet.replace("Тихая вода", "Утренний прилив"),
        TextEncoding::Legacy(resonate_core::LegacyEncoding::WINDOWS_1251),
    )
    .expect("Cyrillic letters");
    let imported = library.import_playlist(&tree.write("cyrillic.m3u", &cyrillic), None)?;

    assert_eq!(
        imported.encoding,
        TextEncoding::Legacy(resonate_core::LegacyEncoding::WINDOWS_1251)
    );
    assert_eq!(imported.name, "Утренний прилив");
    assert_eq!(imported.missing, 0);
    Ok(())
}
