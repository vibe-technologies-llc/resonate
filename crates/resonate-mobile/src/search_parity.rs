use std::{
    collections::BTreeSet,
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

use resonate_analysis::Verdict;
use resonate_codec::Codec;
use resonate_library::{Agreement, Library, ScanOptions, Search, Shape, TrackQuery};
use rusqlite::{Connection, params};

use crate::{
    Track,
    catalog::Catalog,
    search::{LISTED_AT_MOST, Searching},
};

const SECONDS_A_DAY: u64 = 86_400;
const AN_HOUR_PAST_THE_DAY: u64 = 3_600;
const NANOSECONDS_A_SECOND: i64 = 1_000_000_000;
const UTF8: u8 = 3;

const TITLE: &[u8; 4] = b"TIT2";
const ARTIST: &[u8; 4] = b"TPE1";
const ALBUM: &[u8; 4] = b"TALB";
const GENRE: &[u8; 4] = b"TCON";
const YEAR: &[u8; 4] = b"TDRC";

const CODEC_WORDS: [(Codec, &str); 13] = [
    (Codec::Flac, "flac"),
    (Codec::Alac, "alac"),
    (Codec::Dsd, "dsd"),
    (Codec::Pcm, "wav"),
    (Codec::Aac, "aac"),
    (Codec::Mp3, "mp3"),
    (Codec::Vorbis, "vorbis"),
    (Codec::Opus, "opus"),
    (Codec::WavPack, "wavpack"),
    (Codec::WavPackHybrid, "hybrid"),
    (Codec::MonkeysAudio, "ape"),
    (Codec::Adpcm, "adpcm"),
    (Codec::Unknown, "unknown"),
];

const MIXED: &str = "mixed";

const TERM_KEYS: [&str; 9] = [
    "added", "plays", "played", "year", "length", "rate", "depth", "codec", "is",
];

struct Song {
    title: &'static str,
    codec: Codec,
    rate: u32,
    bits: u8,
    channels: u32,
    seconds: u32,
    added_days: u64,
    heard_days: &'static [u64],
    favourite: bool,
    year: &'static str,
    studied: Option<Verdict>,
    misnamed: bool,
}

const SONGS: [Song; 17] = [
    Song {
        title: "A1",
        codec: Codec::Flac,
        rate: 44_100,
        bits: 16,
        channels: 2,
        seconds: 2,
        added_days: 2,
        heard_days: &[1, 3],
        favourite: true,
        year: "1973",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A2",
        codec: Codec::Flac,
        rate: 44_100,
        bits: 24,
        channels: 2,
        seconds: 3,
        added_days: 10,
        heard_days: &[],
        favourite: false,
        year: "1995",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A3",
        codec: Codec::Flac,
        rate: 96_000,
        bits: 24,
        channels: 6,
        seconds: 2,
        added_days: 40,
        heard_days: &[0, 5, 40],
        favourite: true,
        year: "2008",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A4",
        codec: Codec::Flac,
        rate: 96_000,
        bits: 16,
        channels: 1,
        seconds: 2,
        added_days: 400,
        heard_days: &[100, 200, 300, 400],
        favourite: false,
        year: "2021",
        studied: Some(Verdict::Fake),
        misnamed: false,
    },
    Song {
        title: "A5",
        codec: Codec::Pcm,
        rate: 48_000,
        bits: 16,
        channels: 2,
        seconds: 2,
        added_days: 5,
        heard_days: &[2],
        favourite: false,
        year: "1969",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A6",
        codec: Codec::Pcm,
        rate: 8_000,
        bits: 16,
        channels: 1,
        seconds: 180,
        added_days: 15,
        heard_days: &[],
        favourite: false,
        year: "",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A7",
        codec: Codec::Mp3,
        rate: 8_000,
        bits: 16,
        channels: 2,
        seconds: 240,
        added_days: 1,
        heard_days: &[1],
        favourite: false,
        year: "2001",
        studied: Some(Verdict::Suspect),
        misnamed: false,
    },
    Song {
        title: "A8",
        codec: Codec::Mp3,
        rate: 96_000,
        bits: 16,
        channels: 2,
        seconds: 2,
        added_days: 31,
        heard_days: &[3, 3],
        favourite: false,
        year: "1999",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "A9",
        codec: Codec::Opus,
        rate: 11_025,
        bits: 16,
        channels: 2,
        seconds: 185,
        added_days: 3,
        heard_days: &[400],
        favourite: false,
        year: "2015",
        studied: Some(Verdict::Genuine),
        misnamed: true,
    },
    Song {
        title: "B1",
        codec: Codec::Aac,
        rate: 22_050,
        bits: 16,
        channels: 2,
        seconds: 59,
        added_days: 200,
        heard_days: &[],
        favourite: false,
        year: "1980",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B2",
        codec: Codec::Alac,
        rate: 192_000,
        bits: 24,
        channels: 6,
        seconds: 1,
        added_days: 7,
        heard_days: &[6],
        favourite: true,
        year: "2019",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B3",
        codec: Codec::Vorbis,
        rate: 8_000,
        bits: 16,
        channels: 2,
        seconds: 245,
        added_days: 8,
        heard_days: &[],
        favourite: false,
        year: "2000",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B4",
        codec: Codec::WavPack,
        rate: 11_025,
        bits: 24,
        channels: 2,
        seconds: 60,
        added_days: 9,
        heard_days: &[],
        favourite: false,
        year: "2000",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B5",
        codec: Codec::Dsd,
        rate: 8_000,
        bits: 16,
        channels: 2,
        seconds: 30,
        added_days: 1,
        heard_days: &[],
        favourite: false,
        year: "1999",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B6",
        codec: Codec::Adpcm,
        rate: 11_025,
        bits: 16,
        channels: 1,
        seconds: 2,
        added_days: 90,
        heard_days: &[],
        favourite: false,
        year: "",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B7",
        codec: Codec::Unknown,
        rate: 44_100,
        bits: 16,
        channels: 1,
        seconds: 1,
        added_days: 50,
        heard_days: &[],
        favourite: false,
        year: "1983",
        studied: None,
        misnamed: false,
    },
    Song {
        title: "B8",
        codec: Codec::WavPackHybrid,
        rate: 48_000,
        bits: 16,
        channels: 2,
        seconds: 1,
        added_days: 60,
        heard_days: &[],
        favourite: false,
        year: "2004",
        studied: None,
        misnamed: false,
    },
];

const MONKEYS: Song = Song {
    title: "B9",
    codec: Codec::MonkeysAudio,
    rate: 44_100,
    bits: 24,
    channels: 2,
    seconds: 2,
    added_days: 120,
    heard_days: &[10],
    favourite: false,
    year: "2004",
    studied: None,
    misnamed: false,
};

fn songs() -> impl Iterator<Item = &'static Song> {
    SONGS.iter().chain([&MONKEYS])
}

fn code_of(codec: Codec) -> i64 {
    match codec {
        Codec::Unknown => 0,
        Codec::Flac => 1,
        Codec::Alac => 2,
        Codec::Pcm => 5,
        Codec::Aac => 6,
        Codec::Mp3 => 7,
        Codec::Vorbis => 8,
        Codec::Opus => 9,
        Codec::Dsd => 10,
        Codec::WavPack => 11,
        Codec::MonkeysAudio => 12,
        Codec::WavPackHybrid => 13,
        Codec::Adpcm => 14,
    }
}

fn seconds_ago(now: u64, days: u64) -> u64 {
    now - days * SECONDS_A_DAY - AN_HOUR_PAST_THE_DAY
}

fn now_in_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("a clock past the epoch")
        .as_secs()
}

fn synchsafe(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7f) as u8,
        ((value >> 14) & 0x7f) as u8,
        ((value >> 7) & 0x7f) as u8,
        (value & 0x7f) as u8,
    ]
}

fn frame(id: &[u8; 4], value: &str) -> Vec<u8> {
    let mut body = vec![UTF8];
    body.extend_from_slice(value.as_bytes());

    let mut frame = id.to_vec();
    frame.extend_from_slice(&synchsafe(body.len() as u32));
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&body);
    frame
}

fn chunk(into: &mut Vec<u8>, id: &[u8; 4], payload: &[u8]) {
    into.extend_from_slice(id);
    into.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    into.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        into.push(0);
    }
}

fn wave_of(song: &Song) -> Vec<u8> {
    let bytes_a_frame = u32::from(song.bits) / 8;
    let data = vec![0_u8; (song.rate * song.seconds * bytes_a_frame) as usize];

    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&song.rate.to_le_bytes());
    format.extend_from_slice(&(song.rate * bytes_a_frame).to_le_bytes());
    format.extend_from_slice(&(bytes_a_frame as u16).to_le_bytes());
    format.extend_from_slice(&u16::from(song.bits).to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    chunk(&mut body, b"fmt ", &format);
    chunk(&mut body, b"data", &data);

    let mut tags = Vec::new();
    tags.extend(frame(TITLE, song.title));
    tags.extend(frame(ARTIST, "Parity"));
    tags.extend(frame(ALBUM, &format!("Album {}", song.title)));
    tags.extend(frame(GENRE, "Test"));
    if !song.year.is_empty() {
        tags.extend(frame(YEAR, song.year));
    }

    let mut file = b"ID3\x04\x00\x00".to_vec();
    file.extend_from_slice(&synchsafe(tags.len() as u32));
    file.extend_from_slice(&tags);
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        let folder = env::temp_dir().join(format!("resonate-search-parity-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("music")).expect("a test folder");
        Self(folder)
    }

    fn music(&self) -> PathBuf {
        self.0.join("music")
    }

    fn catalog(&self) -> PathBuf {
        self.0.join("library.sqlite")
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scanned(folder: &Folder) {
    for song in songs() {
        fs::write(
            folder.music().join(format!("{}.wav", song.title)),
            wave_of(song),
        )
        .expect("a song");
    }

    let library = Library::open(&folder.catalog()).expect("a library");
    let summary = library
        .scan(ScanOptions {
            roots: vec![folder.music()],
            incremental: true,
            follow_symlinks: false,
            extract_cover_art: false,
            workers: NonZeroUsize::MIN,
        })
        .expect("a scan")
        .join()
        .expect("a finished scan");
    assert!(!summary.cancelled);
}

fn shaped_as_desktop_rows(folder: &Folder, now: u64) {
    let connection = Connection::open(folder.catalog()).expect("the desktop catalog");

    for song in songs() {
        let nanos = |seconds: u64| seconds as i64 * NANOSECONDS_A_SECOND;
        let heard: Vec<i64> = song
            .heard_days
            .iter()
            .map(|days| nanos(seconds_ago(now, *days)))
            .collect();

        connection
            .execute(
                "UPDATE tracks SET codec = ?2, channels = ?3, added = ?4, plays = ?5, played = ?6, favourite = ?7 WHERE title = ?1",
                params![
                    song.title,
                    code_of(song.codec),
                    song.channels,
                    nanos(seconds_ago(now, song.added_days)),
                    heard.len() as i64,
                    heard.iter().max(),
                    song.favourite.then_some(1_i64),
                ],
            )
            .expect("a song shaped");
        for at in heard {
            connection
                .execute(
                    "INSERT INTO listens (track_id, at, heard, began) SELECT id, ?2, 0, ?2 FROM tracks WHERE title = ?1",
                    params![song.title, at],
                )
                .expect("a listen");
        }
        if let Some(verdict) = song.studied {
            connection
                .execute(
                    "INSERT INTO track_studies (track_id, studied, studied_under, verdict, peak, rms, true_peak, clipped, mono_as_stereo, agreement)
                     SELECT id, 1, 0, ?2, 0.5, 0.1, 0.5, 0, 0, ?3 FROM tracks WHERE title = ?1",
                    params![
                        song.title,
                        verdict.as_str(),
                        song.misnamed.then_some(Agreement::Disagrees.as_str()),
                    ],
                )
                .expect("a study");
        }
    }
}

fn the_phone_holding_the_same_songs(now: u64) -> Catalog {
    let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");

    for song in songs() {
        let track = catalog
            .upsert(Track {
                uri: format!("content://parity/{}.wav", song.title),
                name: format!("{}.wav", song.title),
                title: song.title.into(),
                artist: "Parity".into(),
                album: format!("Album {}", song.title),
                duration_ms: u64::from(song.seconds) * 1000,
                rate: song.rate,
                codec: song.codec.as_str().into(),
                ..Track::default()
            })
            .expect("an indexed song");
        let connection = catalog.0.lock();

        connection
            .execute(
                "UPDATE documents SET genre = 'Test', date = ?2, bits = ?3, channels = ?4, added = ?5, favourite = ?6 WHERE id = ?1",
                params![
                    track.id as i64,
                    song.year,
                    song.bits,
                    song.channels,
                    seconds_ago(now, song.added_days) as i64,
                    song.favourite,
                ],
            )
            .expect("a song shaped");
        for days in song.heard_days {
            connection
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, 30000, 1)",
                    params![track.id as i64, seconds_ago(now, *days) as i64],
                )
                .expect("a listen");
        }
        if let Some(verdict) = song.studied {
            connection
                .execute(
                    "INSERT INTO studies (track, verdict, peak, rms, true_peak, clipped) VALUES (?1, ?2, 0.5, 0.1, 0.5, 0)",
                    params![track.id as i64, verdict.as_str()],
                )
                .expect("a study");
        }
        drop(connection);
        if song.misnamed {
            catalog
                .keep_agreement(track.id, Agreement::Disagrees)
                .expect("an agreement");
        }
    }

    catalog
}

fn comparisons(key: &str, values: &[&str]) -> Vec<String> {
    ["", "<", "<=", "=", ">=", ">"]
        .iter()
        .flat_map(|operator| {
            values
                .iter()
                .map(move |value| format!("{key}:{operator}{value}"))
        })
        .collect()
}

fn queries() -> Vec<(&'static str, String)> {
    let mut asked: Vec<(&'static str, String)> = Vec::new();
    let mut ask = |key: &'static str, queries: Vec<String>| {
        asked.extend(queries.into_iter().map(|query| (key, query)));
    };

    ask(
        "added",
        comparisons("added", &["7d", "30d", "1y", "2w", "12h"]),
    );
    ask(
        "plays",
        comparisons("plays", &["0", "1", "2", "4"])
            .into_iter()
            .chain(comparisons("plays", &["1@7d", "0@30d", "1@1y", "2@1mo"]))
            .chain(["plays:1-3".into(), "plays:0-1@1y".into()])
            .collect(),
    );
    ask("played", comparisons("played", &["7d", "2d", "1y", "60d"]));
    ask(
        "year",
        comparisons("year", &["1973", "2000", "2021", "1900", "2100"])
            .into_iter()
            .chain([
                "year:1970-1979".into(),
                "year:2000-1990".into(),
                "year:1999-2000".into(),
            ])
            .collect(),
    );
    ask(
        "length",
        comparisons(
            "length",
            &["3:00", "3:05", "0:59", "4:05", "1m", "3m", "30s", "2s"],
        )
        .into_iter()
        .chain([
            "length:2m-4m".into(),
            "length:30s-1m".into(),
            "length:1-2".into(),
        ])
        .collect(),
    );
    ask(
        "rate",
        comparisons(
            "rate",
            &["44.1", "44100", "48k", "96", "8k", "11025", "192khz"],
        )
        .into_iter()
        .chain(["rate:8k-48k".into(), "rate:96-44.1".into()])
        .collect(),
    );
    ask(
        "depth",
        comparisons("depth", &["16", "24", "24bit", "20", "8", "32"])
            .into_iter()
            .chain(["depth:16-24".into(), "depth:24-16bits".into()])
            .collect(),
    );
    ask(
        "codec",
        CODEC_WORDS
            .iter()
            .flat_map(|(_, word)| [format!("codec:{word}"), format!("-codec:{word}")])
            .collect(),
    );
    ask(
        "is",
        Shape::ALL
            .iter()
            .flat_map(|shape| {
                [
                    format!("is:{}", shape.name()),
                    format!("-is:{}", shape.name()),
                ]
            })
            .collect(),
    );
    ask(
        MIXED,
        [
            "is:lossless rate:>44.1k",
            "is:hires or is:lossy",
            "is:favourite depth:24",
            "codec:flac or codec:mp3 length:>1m",
            "-is:stereo plays:>0",
            "year:>=2000 added:<30d -is:mono",
            "is:fake or is:suspect or is:misnamed",
            "is:multichannel rate:>=96k",
            "played:<7d plays:>=2@1y",
            "-codec:flac -codec:wav -is:lossless",
        ]
        .map(String::from)
        .to_vec(),
    );

    asked
}

fn titles_of_the_desktop(library: &Library, text: &str) -> BTreeSet<String> {
    library
        .tracks(&TrackQuery {
            text: Some(text.to_owned()),
            ..TrackQuery::default()
        })
        .expect("a desktop search")
        .into_iter()
        .map(|track| track.title)
        .collect()
}

fn titles_of_the_phone(searching: &Searching, catalog: &Catalog, text: &str) -> BTreeSet<String> {
    let found = searching
        .search(catalog, text, LISTED_AT_MOST)
        .expect("a phone search");

    found.tracks.into_iter().map(|track| track.title).collect()
}

#[test]
fn the_desktops_sql_and_the_phones_memory_answer_every_term_of_the_grammar_alike() {
    let folder = Folder::new();
    let now = now_in_seconds();

    scanned(&folder);
    shaped_as_desktop_rows(&folder, now);
    let library = Library::open(&folder.catalog()).expect("a library");
    let catalog = the_phone_holding_the_same_songs(now);
    let searching = Searching::default();
    let everything = songs().count();

    let mut disagreements = Vec::new();
    let mut told_apart = 0;
    let mut asked = 0;
    for (key, query) in queries() {
        let reads = Search::read(&query).reads();
        assert!(
            !reads.is_empty()
                && reads
                    .iter()
                    .all(|read| key == MIXED || read.trim_start_matches('-').starts_with(key)),
            "{query} was not read as {key}: {reads:?}"
        );

        let desktop = titles_of_the_desktop(&library, &query);
        let phone = titles_of_the_phone(&searching, &catalog, &query);

        asked += 1;
        if !desktop.is_empty() && desktop.len() < everything {
            told_apart += 1;
        }
        if desktop != phone {
            disagreements.push(format!(
                "{query}\n  desktop only: {:?}\n  phone only:   {:?}",
                desktop.difference(&phone).collect::<Vec<_>>(),
                phone.difference(&desktop).collect::<Vec<_>>(),
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "{} of {asked} queries are answered otherwise:\n{}",
        disagreements.len(),
        disagreements.join("\n")
    );
    assert!(
        told_apart * 10 >= asked * 6,
        "only {told_apart} of {asked} queries told songs apart"
    );
}

#[test]
fn the_table_asks_every_kind_of_term_the_grammar_has() {
    let kinds: BTreeSet<&str> = queries()
        .into_iter()
        .map(|(key, _)| key)
        .filter(|key| *key != MIXED)
        .collect();

    assert_eq!(kinds, BTreeSet::from(TERM_KEYS));
}
