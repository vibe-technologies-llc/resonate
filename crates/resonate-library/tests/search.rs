use std::{
    collections::BTreeSet,
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{MediaLocation, TrackId};
use resonate_library::{
    AlbumOrder, AlbumQuery, Cut, Direction, Library, Reason, Result, SavedQuery, ScanOptions,
    SortOrder, TrackQuery,
};

const UTF8: u8 = 3;
const CD_RATE: u32 = 44_100;
const SLOW_RATE: u32 = 8_000;
const HI_RES_RATE: u32 = 96_000;
const A_TENTH: u32 = 10;

const TITLE: &[u8; 4] = b"TIT2";
const ARTIST: &[u8; 4] = b"TPE1";
const ALBUM: &[u8; 4] = b"TALB";
const YEAR: &[u8; 4] = b"TDRC";

const ENOUGH_TO_BE_OFFERED: usize = 10;

struct Tree {
    root: PathBuf,
}

impl Tree {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-search-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary directory");
        Self { root }
    }

    fn write(&self, relative: &str, wav: &Wav) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("a writable temporary directory");
        }
        fs::write(&path, wav.build()).expect("a writable temporary file");
    }

    fn path(&self) -> &Path {
        &self.root
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Wav {
    rate: u32,
    frames: u32,
    id3: Vec<u8>,
}

impl Wav {
    fn new() -> Self {
        Self {
            rate: CD_RATE,
            frames: CD_RATE / A_TENTH,
            id3: Vec::new(),
        }
    }

    fn lasting(mut self, rate: u32, tenths: u32) -> Self {
        self.rate = rate;
        self.frames = rate * tenths / A_TENTH;
        self
    }

    fn text(mut self, id: &[u8; 4], value: &str) -> Self {
        let mut body = vec![UTF8];
        body.extend_from_slice(value.as_bytes());
        self.id3.extend_from_slice(id);
        self.id3.extend_from_slice(&synchsafe(body.len() as u32));
        self.id3.extend_from_slice(&[0, 0]);
        self.id3.extend_from_slice(&body);
        self
    }

    fn build(&self) -> Vec<u8> {
        let data = vec![0_u8; self.frames as usize * 2];

        let mut fmt = Vec::new();
        fmt.extend_from_slice(&1_u16.to_le_bytes());
        fmt.extend_from_slice(&1_u16.to_le_bytes());
        fmt.extend_from_slice(&self.rate.to_le_bytes());
        fmt.extend_from_slice(&(self.rate * 2).to_le_bytes());
        fmt.extend_from_slice(&2_u16.to_le_bytes());
        fmt.extend_from_slice(&16_u16.to_le_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(b"WAVE");
        chunk(&mut body, b"fmt ", &fmt);
        chunk(&mut body, b"data", &data);

        let mut file = Vec::new();
        if !self.id3.is_empty() {
            file.extend_from_slice(b"ID3\x04\x00\x00");
            file.extend_from_slice(&synchsafe(self.id3.len() as u32));
            file.extend_from_slice(&self.id3);
        }
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
    }
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn scanned(tree: &Tree) -> Result<Library> {
    let library = Library::open_in_memory()?;
    let summary = library
        .scan(ScanOptions {
            roots: vec![tree.path().to_path_buf()],
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })?
        .join()?;
    assert!(!summary.cancelled, "the scan reported itself cancelled");

    Ok(library)
}

fn titled(library: &Library, text: &str) -> Result<BTreeSet<String>> {
    Ok(library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            ..TrackQuery::default()
        })?
        .into_iter()
        .map(|track| track.title)
        .collect())
}

fn named(titles: &[&str]) -> BTreeSet<String> {
    titles.iter().map(|title| (*title).to_owned()).collect()
}

#[test]
fn a_search_made_only_of_punctuation_asks_nothing_and_so_holds_everything() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new().text(TITLE, "Echoes").text(ARTIST, "Ada"),
    );
    tree.write("b.wav", &Wav::new().text(TITLE, "Time").text(ARTIST, "Ada"));

    let library = scanned(&tree)?;
    let everything = named(&["Echoes", "Time"]);

    for text in ["!!!", "+/-", "-!!!", "\"...\"", "artist:?"] {
        assert_eq!(
            titled(&library, text)?,
            everything,
            "{text} held too little"
        );
    }
    assert_eq!(titled(&library, "echoes !!!")?, named(&["Echoes"]));
    Ok(())
}

#[test]
fn a_saved_query_with_no_search_fills_itself_with_the_whole_catalog() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new().text(TITLE, "Echoes").text(ARTIST, "Ada"),
    );
    tree.write("b.wav", &Wav::new().text(TITLE, "Time").text(ARTIST, "Ada"));

    let library = scanned(&tree)?;

    for (name, text) in [("Blank", ""), ("Spaces", "   "), ("Marks", "!!!")] {
        let saved = library.save_query(
            name,
            &SavedQuery {
                text: Some(text.to_owned()),
                ..SavedQuery::default()
            },
        )?;
        assert_eq!(
            library.playlist_entries(saved, None)?.len(),
            2,
            "a query saved as {text:?} held the wrong rows"
        );
    }
    Ok(())
}

#[test]
fn a_list_narrowed_by_a_text_a_search_reads_nothing_in_holds_no_row_and_drops_none() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "a.wav",
        &Wav::new().text(TITLE, "Echoes").text(ARTIST, "Ada"),
    );
    tree.write("b.wav", &Wav::new().text(TITLE, "Time").text(ARTIST, "Ada"));

    let library = scanned(&tree)?;

    let list = library.create_playlist("Evening")?;
    let cuts: Vec<Cut> = ["a.wav", "b.wav"]
        .into_iter()
        .map(|file| Cut::whole(MediaLocation::local(tree.path().join(file))))
        .collect();
    library.add_to_playlist(list, &cuts)?;

    assert_eq!(library.playlist_entries(list, Some("!!!"))?.len(), 0);
    assert_eq!(library.playlist_entries(list, Some(""))?.len(), 2);
    assert_eq!(library.playlist_entries(list, Some("echoes !!!"))?.len(), 1);
    assert_eq!(library.playlist_entries(list, Some("nowhere"))?.len(), 0);
    assert_eq!(library.remove_matching(list, "!!!")?, 0);
    assert_eq!(library.playlist_entries(list, None)?.len(), 2);
    Ok(())
}

#[test]
fn an_exact_length_holds_every_track_that_long_to_the_unit_it_was_typed_in() -> Result<()> {
    let tree = Tree::new();
    for (title, tenths) in [
        ("Short", 2_095),
        ("Just", 2_100),
        ("Within", 2_104),
        ("Past", 2_110),
        ("Three", 1_800),
        ("Nearly Four", 2_399),
        ("Four", 2_400),
    ] {
        tree.write(
            &format!("{title}.wav"),
            &Wav::new()
                .lasting(SLOW_RATE, tenths)
                .text(TITLE, title)
                .text(ARTIST, "Ada"),
        );
    }

    let library = scanned(&tree)?;

    assert_eq!(titled(&library, "length:3:30")?, named(&["Just", "Within"]));
    assert_eq!(
        titled(&library, "length:3m30s")?,
        named(&["Just", "Within"])
    );
    assert_eq!(
        titled(&library, "length:3m")?,
        named(&["Three", "Short", "Just", "Within", "Past", "Nearly Four"])
    );
    assert_eq!(
        titled(&library, "length:<=3:30")?,
        named(&["Three", "Short", "Just", "Within"])
    );
    assert_eq!(
        titled(&library, "length:>3:30")?,
        named(&["Past", "Nearly Four", "Four"])
    );
    assert_eq!(titled(&library, "length:4m")?, named(&["Four"]));
    assert_eq!(
        titled(&library, "-length:3m")?,
        named(&["Four"]),
        "a denied length should hold what the length does not"
    );
    Ok(())
}

#[test]
fn a_range_typed_backwards_and_a_rate_in_kilohertz_hold_what_they_name() -> Result<()> {
    let tree = Tree::new();
    tree.write(
        "nineties/a.wav",
        &Wav::new()
            .text(TITLE, "Nineties")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Then")
            .text(YEAR, "1995"),
    );
    tree.write(
        "seventies/a.wav",
        &Wav::new()
            .lasting(HI_RES_RATE, 1)
            .text(TITLE, "Seventies")
            .text(ARTIST, "Ada")
            .text(ALBUM, "Before")
            .text(YEAR, "1975"),
    );

    let library = scanned(&tree)?;

    assert_eq!(titled(&library, "year:2000-1990")?, named(&["Nineties"]));
    assert_eq!(titled(&library, "year:1990-2000")?, named(&["Nineties"]));
    assert_eq!(titled(&library, "rate:44.1")?, named(&["Nineties"]));
    assert_eq!(titled(&library, "rate:96")?, named(&["Seventies"]));
    assert_eq!(titled(&library, "rate:>48")?, named(&["Seventies"]));
    Ok(())
}

#[test]
fn an_artist_mix_holds_the_artist_it_names_and_not_one_whose_name_begins_with_it() -> Result<()> {
    let tree = Tree::new();
    for number in 0..ENOUGH_TO_BE_OFFERED {
        tree.write(
            &format!("air/{number}.wav"),
            &Wav::new()
                .text(TITLE, &format!("Moon Safari {number}"))
                .text(ARTIST, "Air"),
        );
        tree.write(
            &format!("supply/{number}.wav"),
            &Wav::new()
                .text(TITLE, &format!("All Out of Love {number}"))
                .text(ARTIST, "Air Supply"),
        );
    }

    let library = scanned(&tree)?;
    let suggestions = library.suggestions()?;
    let mix = suggestions
        .iter()
        .find(|suggestion| suggestion.reason == Reason::Artist && suggestion.name == "Air")
        .expect("Air is offered a mix of its own");
    let saved = library.save_query("Air", &mix.query)?;
    let held = library.playlist_entries(saved, None)?;

    assert_eq!(mix.rows, ENOUGH_TO_BE_OFFERED as u64);
    assert_eq!(held.len(), ENOUGH_TO_BE_OFFERED);
    assert!(
        held.iter()
            .filter_map(|entry| entry.track.as_ref())
            .all(|track| track.artist.as_deref() == Some("Air")),
        "the mix for Air held another artist's tracks"
    );
    assert_eq!(
        titled(&library, "artist:=air")?.len(),
        ENOUGH_TO_BE_OFFERED,
        "a whole name should be one however it is cased"
    );
    assert_eq!(
        titled(&library, "artist:=\"air supply\"")?.len(),
        ENOUGH_TO_BE_OFFERED
    );
    assert_eq!(
        titled(&library, "artist:air")?.len(),
        2 * ENOUGH_TO_BE_OFFERED,
        "a word should still reach every name it begins"
    );
    Ok(())
}

#[test]
fn a_listing_paged_through_its_ties_hands_out_every_row_once() -> Result<()> {
    let tree = Tree::new();
    let held = 7;
    for number in 0..held {
        tree.write(
            &format!("{number}/a.wav"),
            &Wav::new()
                .text(TITLE, "Intro")
                .text(ARTIST, &format!("Artist {number}"))
                .text(ALBUM, "Greatest Hits"),
        );
    }

    let library = scanned(&tree)?;

    for sort in SortOrder::ALL {
        for reading in Direction::ALL {
            let mut seen: Vec<TrackId> = Vec::new();
            for offset in 0..held {
                seen.extend(
                    library
                        .tracks(&TrackQuery {
                            sort,
                            reading,
                            limit: Some(1),
                            offset,
                            ..TrackQuery::default()
                        })?
                        .into_iter()
                        .map(|track| track.id),
                );
            }
            let whole: Vec<TrackId> = library
                .tracks(&TrackQuery {
                    sort,
                    reading,
                    ..TrackQuery::default()
                })?
                .into_iter()
                .map(|track| track.id)
                .collect();

            assert_eq!(seen, whole, "{sort:?} read {reading:?} paged differently");
        }
    }

    for sort in AlbumOrder::ALL {
        for reading in Direction::ALL {
            let mut seen = Vec::new();
            for offset in 0..held {
                seen.extend(
                    library
                        .albums(&AlbumQuery {
                            sort,
                            reading,
                            limit: Some(1),
                            offset,
                            ..AlbumQuery::default()
                        })?
                        .into_iter()
                        .map(|album| album.id),
                );
            }
            let whole: Vec<_> = library
                .albums(&AlbumQuery {
                    sort,
                    reading,
                    ..AlbumQuery::default()
                })?
                .into_iter()
                .map(|album| album.id)
                .collect();

            assert_eq!(whole.len(), held);
            assert_eq!(seen, whole, "{sort:?} read {reading:?} paged differently");
        }
    }
    Ok(())
}
