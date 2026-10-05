use std::time::{Duration, SystemTime};

use ahash::AHashSet;
use resonate_core::{AlbumId, ArtistId, ReleaseTrackId, WantId};
use rusqlite::{OptionalExtension as _, Transaction, params};

use crate::{
    AlbumMatch, ByArtist, Column, Error, Issued, Mbid, RecordingMatch, RecordingRelease, Release,
    Result, Search, StoreOp, enrich::SOUNDTRACK, enriched, store,
};

pub const FOUND_ELSEWHERE_AT_MOST: usize = 12;

pub const ALBUMS_FOUND_ELSEWHERE_AT_MOST: usize = 12;

const ALBUM_KINDS: [&str; 2] = [AN_ALBUM, AN_EP];

pub const ARTISTS_FOUND_ELSEWHERE_AT_MOST: usize = 4;

const FEWEST_LETTERS_ASKED_ELSEWHERE: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sung {
    pub query: String,
    pub tracks: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Covering {
    pub album: AlbumId,
    pub release: Mbid,
    pub group: Option<Mbid>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Uncovered<T> {
    pub wanted: T,
    pub covering: Option<Covering>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub recording: Mbid,
    pub title: String,
    pub artist: String,
    pub length: Option<Duration>,
    pub release: Option<RecordingRelease>,
    pub releases: Vec<RecordingRelease>,
}

impl Found {
    pub fn from(&self, release: &RecordingRelease) -> Self {
        Self {
            release: Some(release.clone()),
            ..self.clone()
        }
    }

    pub fn in_the_order_worth_offering(&self) -> Vec<&RecordingRelease> {
        in_the_order_worth_offering(&self.releases)
    }

    fn answers(&self, words: &[String]) -> bool {
        let named: Vec<String> = [self.title.as_str(), self.artist.as_str()]
            .into_iter()
            .chain(self.releases.iter().map(|release| release.title.as_str()))
            .flat_map(str::split_whitespace)
            .map(store::folded_letters)
            .collect();

        words
            .iter()
            .all(|word| named.iter().any(|name| name.starts_with(word.as_str())))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtistFound {
    pub mbid: Mbid,
    pub name: String,
}

fn folded_words_asked(text: &str) -> Vec<String> {
    words_asked(text)
        .iter()
        .flat_map(|word| word.split_whitespace())
        .map(store::folded_letters)
        .filter(|word| !word.is_empty())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlbumFound {
    pub group: Mbid,
    pub title: String,
    pub artist: String,
    pub kind: Option<String>,
    pub first_released: Option<String>,
}

impl AlbumMatch {
    fn is_an_album(&self) -> bool {
        self.kind
            .as_deref()
            .is_some_and(|kind| ALBUM_KINDS.contains(&kind))
            && self
                .secondary
                .iter()
                .all(|secondary| secondary == SOUNDTRACK)
    }

    fn answers(&self, words: &[String]) -> bool {
        let named: Vec<String> = std::iter::once(self.title.clone())
            .chain(std::iter::once(self.credited_as()))
            .flat_map(|text| {
                text.split_whitespace()
                    .map(store::folded_letters)
                    .collect::<Vec<_>>()
            })
            .collect();

        words
            .iter()
            .all(|word| named.iter().any(|name| name.starts_with(word.as_str())))
    }
}

pub(crate) fn albums_named_by(matches: &[AlbumMatch], text: &str) -> Vec<AlbumFound> {
    let words = folded_words_asked(text);
    if words.is_empty() {
        return Vec::new();
    }

    let mut seen = AHashSet::new();
    let mut named = Vec::new();
    for matched in matches {
        if !matched.is_an_album() || !matched.answers(&words) {
            continue;
        }
        let artist = matched.credited_as();
        let key = (
            store::folded_letters(&matched.title),
            store::folded_letters(&artist),
        );
        if !seen.insert(key) {
            continue;
        }
        named.push(AlbumFound {
            group: matched.group.clone(),
            title: matched.title.clone(),
            artist,
            kind: matched.kind.clone(),
            first_released: matched.first_released.clone(),
        });
    }

    named
}

pub(crate) fn artists_named_by(matches: &[RecordingMatch], text: &str) -> Vec<ArtistFound> {
    let words = folded_words_asked(text);
    if words.is_empty() {
        return Vec::new();
    }

    let mut seen = AHashSet::new();
    let mut named = Vec::new();
    for credit in matches.iter().flat_map(|matched| &matched.credit) {
        let Some(mbid) = &credit.mbid else {
            continue;
        };
        let folded: Vec<String> = credit
            .name
            .split_whitespace()
            .map(store::folded_letters)
            .collect();
        let answers = words
            .iter()
            .all(|word| folded.iter().any(|name| name.starts_with(word.as_str())));
        if answers && seen.insert(mbid.clone()) {
            named.push(ArtistFound {
                mbid: mbid.clone(),
                name: credit.name.clone(),
            });
        }
    }

    named
}

pub(crate) fn artist_of_found(tx: &Transaction<'_>, found: &ArtistFound) -> Result<ArtistId> {
    let id = store::artist_named(tx, &found.name, Some(&found.mbid))?;
    tx.execute(
        "UPDATE artists SET found_elsewhere = ?2
          WHERE id = ?1
            AND found_elsewhere IS NULL
            AND id NOT IN (SELECT artist_id FROM tracks WHERE artist_id IS NOT NULL)
            AND id NOT IN (SELECT artist_id FROM albums WHERE artist_id IS NOT NULL)
            AND id NOT IN (SELECT artist_id FROM track_credits)",
        params![id, store::to_nanos(SystemTime::now())],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))?;

    Ok(ArtistId::new(id as u64)?)
}

pub fn in_the_order_worth_offering(releases: &[RecordingRelease]) -> Vec<&RecordingRelease> {
    let mut offered: Vec<&RecordingRelease> = releases.iter().collect();
    offered.sort_by_key(|release| worth(release));
    offered
}

fn worth(release: &RecordingRelease) -> (Standing, Meant, bool, String) {
    (
        standing_of(&release.issued),
        meant_as(&release.issued),
        release.date.is_none(),
        release.date.clone().unwrap_or_default(),
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SongsAsked {
    pub words: String,
    pub by: Option<ByArtist>,
}

pub(crate) fn words_asked(text: &str) -> Vec<String> {
    match ByArtist::read(text) {
        Some(by) => words_typed(&by.words()),
        None => words_typed(text),
    }
}

fn words_typed(text: &str) -> Vec<String> {
    Search::read(text)
        .clauses
        .iter()
        .filter_map(|clause| clause.lone_word())
        .filter(|word| {
            word.column.is_none_or(|column| {
                matches!(column, Column::Title | Column::Artist | Column::Album)
            })
        })
        .map(|word| word.text.clone())
        .filter(|word| !word.trim().is_empty())
        .collect()
}

pub fn asks_elsewhere(text: &str) -> bool {
    songs_asked(text).is_some()
}

pub fn songs_asked(text: &str) -> Option<SongsAsked> {
    let words = words_asked(text);
    let letters = words
        .iter()
        .flat_map(|word| word.chars())
        .filter(|letter| letter.is_alphanumeric())
        .count();
    let lowered = |words: &str| {
        words
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>()
            .join(" ")
    };

    (letters >= FEWEST_LETTERS_ASKED_ELSEWHERE).then(|| SongsAsked {
        words: lowered(&words.join(" ")),
        by: ByArtist::read(text).map(|by| ByArtist {
            title: lowered(&by.title),
            artist: lowered(&by.artist),
        }),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Likeness {
    Same,
    Within,
    Apart,
}

fn letters_of(text: &str) -> String {
    store::folded_letters(text)
        .chars()
        .filter(|letter| letter.is_alphanumeric())
        .collect()
}

fn likeness(typed: &str, named: &str) -> Likeness {
    if typed.is_empty() || named.is_empty() {
        return Likeness::Apart;
    }
    if typed == named {
        return Likeness::Same;
    }
    if named.contains(typed) || typed.contains(named) {
        return Likeness::Within;
    }

    Likeness::Apart
}

pub fn weighed_for(asked: &SongsAsked, matches: Vec<RecordingMatch>) -> Vec<RecordingMatch> {
    let Some(by) = &asked.by else {
        return matches;
    };
    let artist = letters_of(&by.artist);
    let title = letters_of(&by.title);
    let as_credited = |matched: &RecordingMatch| {
        std::iter::once(matched.credited_as())
            .chain(matched.credit.iter().map(|credit| credit.name.clone()))
            .map(|name| likeness(&artist, &letters_of(&name)))
            .min()
            .unwrap_or(Likeness::Apart)
    };
    let as_titled = |matched: &RecordingMatch| likeness(&title, &letters_of(&matched.title));

    let nearest_artist = matches.iter().map(as_credited).min();
    let mut kept: Vec<RecordingMatch> = matches
        .into_iter()
        .filter(|matched| Some(as_credited(matched)) == nearest_artist)
        .collect();
    let nearest_title = kept.iter().map(as_titled).min();
    if nearest_title.is_some_and(|nearest| nearest < Likeness::Apart) {
        kept.retain(|matched| as_titled(matched) < Likeness::Apart);
        kept.sort_by_key(as_titled);
    }

    kept
}

pub fn still_answering(found: &[Found], text: &str) -> Vec<Found> {
    let words = folded_words_asked(text);
    if words.is_empty() {
        return Vec::new();
    }

    found
        .iter()
        .filter(|found| found.answers(&words))
        .cloned()
        .collect()
}

pub(crate) fn found_among(
    matches: Vec<RecordingMatch>,
    held: impl Fn(&Mbid) -> bool,
) -> Vec<Found> {
    let mut seen = AHashSet::new();
    let mut found = Vec::new();
    for matched in matches {
        if held(&matched.recording) {
            continue;
        }
        let artist = matched.credited_as();
        let named = (
            store::folded_letters(&matched.title),
            store::folded_letters(&artist),
        );
        if !seen.insert(named) {
            continue;
        }
        found.push(Found {
            release: meant_release(&matched.releases).cloned(),
            releases: matched.releases,
            recording: matched.recording,
            title: matched.title,
            artist,
            length: matched.length,
        });
        if found.len() == FOUND_ELSEWHERE_AT_MOST {
            break;
        }
    }

    found
}

const AN_ALBUM: &str = "Album";
const AN_EP: &str = "EP";
const A_SINGLE: &str = "Single";
const A_SOUNDTRACK: &str = "Soundtrack";
const OFFICIAL: &str = "Official";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Standing {
    Official,
    Unstated,
    Otherwise,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Meant {
    Album,
    Ep,
    Single,
    Unstated,
    Otherwise,
}

fn standing_of(issued: &Issued) -> Standing {
    match issued.status.as_deref() {
        Some(OFFICIAL) => Standing::Official,
        None => Standing::Unstated,
        Some(_) => Standing::Otherwise,
    }
}

fn meant_as(issued: &Issued) -> Meant {
    let plain = issued.secondary.iter().all(|kind| kind == A_SOUNDTRACK);
    match issued.kind.as_deref() {
        None => Meant::Unstated,
        Some(AN_ALBUM) if plain => Meant::Album,
        Some(AN_EP) if plain => Meant::Ep,
        Some(A_SINGLE) if plain => Meant::Single,
        Some(_) => Meant::Otherwise,
    }
}

pub(crate) fn meant_release(releases: &[RecordingRelease]) -> Option<&RecordingRelease> {
    releases.iter().min_by_key(|release| worth(release))
}

pub(crate) fn album_of_release(
    tx: &Transaction<'_>,
    release: &Release,
    now: SystemTime,
) -> Result<AlbumId> {
    let held: Option<i64> = tx
        .query_row(
            "SELECT id FROM albums WHERE mbid = ?1 ORDER BY id LIMIT 1",
            params![release.id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if let Some(held) = held {
        return Ok(AlbumId::new(held as u64)?);
    }

    let credited = release.credited_as();
    let owner = match credited.trim() {
        "" => None,
        named => Some(store::artist_named(
            tx,
            named,
            release
                .credit
                .first()
                .and_then(|credit| credit.mbid.as_ref()),
        )?),
    };
    let id: i64 = tx
        .query_row(
            "INSERT INTO albums (title, artist_id, mbid, found_elsewhere)
             VALUES (?1, ?2, ?3, ?4) RETURNING id",
            params![
                release.title,
                owner,
                release.id.as_str(),
                store::to_nanos(now)
            ],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    let album = AlbumId::new(id as u64)?;
    enriched::land_release(tx, album, release, now)?;

    Ok(album)
}

pub(crate) fn release_track_of(
    tx: &Transaction<'_>,
    album: AlbumId,
    recording: &Mbid,
) -> Result<Option<ReleaseTrackId>> {
    let row: Option<i64> = tx
        .query_row(
            "SELECT id FROM release_tracks
              WHERE album_id = ?1 AND recording_mbid = ?2
              ORDER BY disc, position LIMIT 1",
            params![album.get() as i64, recording.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    row.map(|row| ReleaseTrackId::new(row as u64).map_err(Error::from))
        .transpose()
}

pub(crate) fn want_in(
    tx: &Transaction<'_>,
    release_track: ReleaseTrackId,
    now: SystemTime,
) -> Result<WantId> {
    let row = release_track.get() as i64;
    let known = tx
        .query_row(
            "SELECT 1 FROM release_tracks WHERE id = ?1",
            params![row],
            |_| Ok(()),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if known.is_none() {
        return Err(Error::UnknownReleaseTrack(release_track));
    }

    tx.execute(
        "INSERT INTO wants (release_track_id, wanted) VALUES (?1, ?2)
         ON CONFLICT(release_track_id) DO NOTHING",
        params![row, store::to_nanos(now)],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    tx.execute(
        "UPDATE wants SET tried = NULL, misses = 0
          WHERE release_track_id = ?1 AND offered IS NULL",
        params![row],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))?;
    tx.execute(
        "DELETE FROM dismissed_missing
          WHERE (album_id, disc, position, folded) IN
                (SELECT album_id, disc, position, folded FROM release_tracks WHERE id = ?1)",
        params![row],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let id: i64 = tx
        .query_row(
            "SELECT id FROM wants WHERE release_track_id = ?1",
            params![row],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(WantId::new(id as u64)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Credit;

    const ONE: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const TWO: &str = "5b11f4ce-a62d-471e-81fc-a69a8278c7da";
    const THREE: &str = "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d";

    fn mbid(text: &str) -> Mbid {
        Mbid::new(text).expect("a well-formed mbid")
    }

    fn released(id: &str, date: Option<&str>) -> RecordingRelease {
        RecordingRelease {
            id: mbid(id),
            title: format!("release {id}"),
            date: date.map(str::to_owned),
            disc: None,
            position: None,
            issued: Issued::default(),
        }
    }

    fn matched(
        id: &str,
        title: &str,
        artist: &str,
        releases: Vec<RecordingRelease>,
    ) -> RecordingMatch {
        RecordingMatch {
            recording: mbid(id),
            score: 100,
            title: title.to_owned(),
            credit: vec![Credit {
                name: artist.to_owned(),
                joined_by: String::new(),
                mbid: None,
            }],
            length: None,
            isrcs: Vec::new(),
            releases,
        }
    }

    #[test]
    fn only_the_names_a_search_asks_for_are_asked_elsewhere() {
        assert_eq!(words_asked("echoes pink"), vec!["echoes", "pink"]);
        assert_eq!(
            words_asked("title:echoes artist:\"pink floyd\" year:1971 -live genre:rock"),
            vec!["echoes", "pink floyd"]
        );
        assert!(words_asked("year:1971 is:hires").is_empty());
        assert!(words_asked("lyrics:\"overhead the albatross\"").is_empty());
    }

    #[test]
    fn a_search_is_asked_elsewhere_only_once_it_names_enough_to_find() {
        assert!(asks_elsewhere("echoes"));
        assert!(asks_elsewhere("ab c"));
        assert!(!asks_elsewhere("ab"));
        assert!(!asks_elsewhere("year:1971"));
        assert!(!asks_elsewhere(""));
    }

    #[test]
    fn a_song_found_twice_is_offered_once_and_what_the_catalog_holds_not_at_all() {
        let found = found_among(
            vec![
                matched(ONE, "Echoes", "Pink Floyd", Vec::new()),
                matched(TWO, "echoes", "Pink Floyd", Vec::new()),
                matched(THREE, "Echoes (live)", "Pink Floyd", Vec::new()),
            ],
            |recording| recording.as_str() == THREE,
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].recording.as_str(), ONE);
        assert_eq!(found[0].artist, "Pink Floyd");
    }

    fn issued(
        id: &str,
        date: &str,
        kind: &str,
        secondary: &[&str],
        status: &str,
    ) -> RecordingRelease {
        RecordingRelease {
            issued: Issued {
                kind: Some(kind.to_owned()),
                secondary: secondary.iter().map(|&kind| kind.to_owned()).collect(),
                status: Some(status.to_owned()),
            },
            ..released(id, Some(date))
        }
    }

    #[test]
    fn a_song_is_placed_on_its_album_before_a_single_or_a_compilation_that_came_out_first() {
        let releases = vec![
            issued(ONE, "1971-01-01", "Single", &[], "Official"),
            issued(TWO, "1970-06-01", "Album", &["Compilation"], "Official"),
            issued(THREE, "1971-10-30", "Album", &[], "Official"),
        ];

        assert_eq!(
            meant_release(&releases).map(|release| release.id.as_str()),
            Some(THREE)
        );
    }

    #[test]
    fn a_found_song_offers_every_release_it_is_on_the_one_it_would_be_placed_on_first() {
        let found = found_among(
            vec![matched(
                ONE,
                "Echoes",
                "Pink Floyd",
                vec![
                    issued(ONE, "1971-01-01", "Single", &[], "Official"),
                    issued(TWO, "1970-06-01", "Album", &["Compilation"], "Official"),
                    issued(THREE, "1971-10-30", "Album", &[], "Official"),
                ],
            )],
            |_| false,
        );
        let [found] = found.as_slice() else {
            panic!("one song was found");
        };

        let offered: Vec<&str> = found
            .in_the_order_worth_offering()
            .into_iter()
            .map(|release| release.id.as_str())
            .collect();
        assert_eq!(offered, vec![THREE, ONE, TWO]);
        assert_eq!(
            found.release.as_ref().map(|release| release.id.as_str()),
            Some(THREE)
        );

        let chosen = found.from(&found.releases[0]);
        assert_eq!(
            chosen.release.as_ref().map(|release| release.id.as_str()),
            Some(ONE)
        );
        assert_eq!(chosen.recording, found.recording);
    }

    #[test]
    fn an_official_release_is_placed_before_a_bootleg_of_the_same_album() {
        let releases = vec![
            issued(ONE, "1970-01-01", "Album", &[], "Bootleg"),
            issued(TWO, "1972-01-01", "Album", &[], "Official"),
        ];

        assert_eq!(
            meant_release(&releases).map(|release| release.id.as_str()),
            Some(TWO)
        );
    }

    #[test]
    fn a_soundtrack_is_an_album_and_an_ep_comes_before_a_single() {
        let soundtrack = vec![
            issued(ONE, "2001-01-01", "Single", &[], "Official"),
            issued(TWO, "2002-01-01", "Album", &["Soundtrack"], "Official"),
        ];
        let smaller = vec![
            issued(ONE, "2001-01-01", "Single", &[], "Official"),
            issued(THREE, "2002-01-01", "EP", &[], "Official"),
        ];

        assert_eq!(
            meant_release(&soundtrack).map(|release| release.id.as_str()),
            Some(TWO)
        );
        assert_eq!(
            meant_release(&smaller).map(|release| release.id.as_str()),
            Some(THREE)
        );
    }

    #[test]
    fn a_song_is_wanted_from_the_release_it_first_came_out_on() {
        let releases = vec![
            released(ONE, Some("1995-05-01")),
            released(TWO, None),
            released(THREE, Some("1971-10-30")),
        ];

        assert_eq!(
            meant_release(&releases).map(|release| release.id.as_str()),
            Some(THREE)
        );
        assert_eq!(
            meant_release(&[released(TWO, None)]).map(|release| release.id.as_str()),
            Some(TWO)
        );
        assert!(meant_release(&[]).is_none());
    }

    #[test]
    fn a_search_is_asked_in_one_spelling_however_it_was_typed() {
        assert_eq!(
            songs_asked("Pink  Floyd").map(|asked| asked.words),
            Some("pink floyd".to_owned())
        );
        assert_eq!(
            songs_asked("You F O by  Stela Cole"),
            Some(SongsAsked {
                words: "you f o stela cole".to_owned(),
                by: Some(ByArtist {
                    title: "you f o".to_owned(),
                    artist: "stela cole".to_owned(),
                }),
            }),
            "the word naming the artist is not asked for"
        );
        assert_eq!(
            songs_asked("pink floyd year:1971"),
            songs_asked("PINK floyd")
        );
        assert_eq!(songs_asked("ab"), None);
    }

    #[test]
    fn songs_asked_for_by_an_artist_keep_the_nearest_artist_and_the_titles_that_match() {
        let you_f_o = matched(
            "11111111-1111-4111-8111-111111111111",
            "You F O",
            "Stela Cole",
            Vec::new(),
        );
        let remixed = matched(
            "22222222-2222-4222-8222-222222222222",
            "You F.O. (Remix)",
            "Stela Cole",
            Vec::new(),
        );
        let another = matched(
            "33333333-3333-4333-8333-333333333333",
            "God Loves You",
            "Stela Cole",
            Vec::new(),
        );
        let near_name = matched(
            "44444444-4444-4444-8444-444444444444",
            "You F O",
            "Stella Cole",
            Vec::new(),
        );
        let answered = vec![another.clone(), remixed.clone(), near_name, you_f_o.clone()];

        for typed in [
            "you fo by stela cole",
            "You F.O. by Stela Cole",
            "stela cole - you f o",
        ] {
            let asked = songs_asked(typed).expect("words worth asking");
            assert_eq!(
                weighed_for(&asked, answered.clone()),
                vec![you_f_o.clone(), remixed.clone()],
                "{typed}"
            );
        }

        let unknown_title = songs_asked("purple rain by stela cole").expect("words worth asking");
        assert_eq!(
            weighed_for(&unknown_title, answered.clone()),
            vec![another, remixed, you_f_o.clone()],
            "a title nothing matches keeps every song by the artist"
        );
        let plain = songs_asked("you f o").expect("words worth asking");
        assert_eq!(weighed_for(&plain, answered.clone()), answered);
    }

    fn found(id: &str, title: &str, artist: &str, release: &str) -> Found {
        let mut released = released(id, None);
        released.title = release.to_owned();
        Found {
            recording: mbid(id),
            title: title.to_owned(),
            artist: artist.to_owned(),
            length: None,
            release: Some(released.clone()),
            releases: vec![released],
        }
    }

    #[test]
    fn songs_found_for_fewer_words_are_narrowed_to_those_still_answering_more() {
        let answered = [
            found(ONE, "Time", "Pink Floyd", "The Dark Side of the Moon"),
            found(TWO, "Echoes", "Pink Floyd", "Meddle"),
            found(THREE, "Pink Moon", "Nick Drake", "Pink Moon"),
        ];

        let narrowed = still_answering(&answered, "pink floyd ti");
        let by_release = still_answering(&answered, "pink medd");
        let accented = still_answering(&answered, "ÉCHO");

        assert_eq!(narrowed, vec![answered[0].clone()]);
        assert_eq!(by_release, vec![answered[1].clone()]);
        assert_eq!(accented, vec![answered[1].clone()]);
        assert!(still_answering(&answered, "").is_empty());
        assert!(still_answering(&answered, "radiohead").is_empty());
    }
}
