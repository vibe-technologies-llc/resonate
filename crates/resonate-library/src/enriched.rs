use std::time::{Duration, SystemTime, UNIX_EPOCH};

use resonate_core::{AlbumId, ArtistId, Frames, MediaLocation, SampleRate, TrackId};
use rusqlite::{
    Connection, OptionalExtension, Row, Transaction, params, params_from_iter, types::Value,
};

use crate::{
    AlbumToAsk, ArtistProfile, ArtistRelease, ArtistToAsk, Certainty, CoverArt, CoverSource, Error,
    Fruitless, Isrc, Link, Mbid, Medium, Recording, RecordingRelease, Release, ReleaseGroup,
    ReleaseTrack, Result, StoreOp, TrackToAsk, Unfinished, VaultKey, Waits,
    model::{CoverFrom, CoverWanted},
    store,
};

const ONE_DISC: u32 = 1;

const UNCOVERED: &str = "albums.cover_art IS NULL AND albums.cover_path IS NULL";

const WAITS_DOUBLE_AT_MOST: u32 = 5;

const HOLDS_RELEASE_ROWS: &str =
    "EXISTS (SELECT 1 FROM release_tracks rt WHERE rt.album_id = a.id)";

const HOLDS_AN_UNPAIRED_ROW: &str =
    "EXISTS (SELECT 1 FROM release_tracks rt WHERE rt.album_id = a.id AND rt.track_id IS NULL)";

const HOLDS_AN_UNPAIRED_TRACK: &str = "EXISTS (SELECT 1 FROM tracks t WHERE t.album_id = a.id
                 AND NOT EXISTS (SELECT 1 FROM release_tracks rt WHERE rt.track_id = t.id))";

const IS_PAIRED: &str = "EXISTS (SELECT 1 FROM release_tracks rt WHERE rt.track_id = t.id)";

const HOLDS_A_GROUP_ALONE: &str = "(a.release_group IS NOT NULL AND a.mbid IS NULL)";

const ASKED_NOW: &str = "1";

fn waited_its_turn(row: &str) -> String {
    let earned = doubled("?2", &format!("{row}.asks"));
    let bad_day = doubled("?5", &format!("{row}.refusals"));
    format!("{row}.asked + (CASE WHEN {row}.refusals > 0 THEN {bad_day} ELSE {earned} END) < ?3")
}

fn doubled(wait: &str, count: &str) -> String {
    format!("{wait} * (1 << min(max({count} - 1, 0), {WAITS_DOUBLE_AT_MOST}))")
}

fn due_again(row: &str) -> String {
    let waited = waited_its_turn(row);
    format!(
        "(?1 OR {row}.asked IS NULL
                 OR ({row}.answered IS NULL AND {waited})
                 OR ({row}.answered IS NOT NULL AND {row}.answered < ?4))"
    )
}

fn album_due() -> String {
    format!(
        "({} OR ({HOLDS_A_GROUP_ALONE} AND {}))",
        due_again("a"),
        waited_its_turn("a")
    )
}

type Pairing<'a> = &'a dyn Fn(&ReleaseRow, &CatalogRow) -> bool;

struct ReleaseRow {
    id: i64,
    disc: u32,
    position: u32,
    recording: Option<String>,
    track: Option<String>,
    isrc: Option<String>,
    folded: String,
    paired: Option<i64>,
}

struct CatalogRow {
    id: i64,
    recording: Option<String>,
    release_track: Option<String>,
    disc: Option<u32>,
    number: Option<u32>,
    folded: String,
}

struct HeldWant {
    track: Option<String>,
    recording: Option<String>,
    disc: i64,
    position: i64,
    wanted: i64,
    tried: Option<i64>,
    offered: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Carried {
    Track,
    Recording,
    Seat,
}

impl Carried {
    const RANKED: [Self; 3] = [Self::Track, Self::Recording, Self::Seat];

    fn of(rank: usize) -> Option<Self> {
        Self::RANKED.get(rank).copied()
    }
}

pub(crate) fn folded_title(title: &str) -> String {
    title
        .chars()
        .filter(|glyph| glyph.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn stripped_title(title: &str) -> String {
    folded_title(&store::folded_letters(title))
}

pub(crate) fn note_enrichment_began(
    tx: &Transaction<'_>,
    refresh: bool,
    now: SystemTime,
) -> Result<()> {
    tx.execute(
        "INSERT INTO enrichment (id, refresh, began) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET refresh = excluded.refresh, began = excluded.began",
        params![i64::from(refresh), store::to_nanos(now)],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Insert, source))
}

pub(crate) fn note_enrichment_finished(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM enrichment", [])
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

pub(crate) fn unfinished_enrichment(connection: &Connection) -> Result<Option<Unfinished>> {
    connection
        .query_row(
            "SELECT refresh, began FROM enrichment WHERE id = 1",
            [],
            |row| {
                Ok(Unfinished {
                    refresh: row.get::<_, i64>(0)? != 0,
                    began: store::from_nanos(row.get(1)?),
                })
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn stamp_album_asked(
    tx: &Transaction<'_>,
    album: AlbumId,
    why: Fruitless,
    now: SystemTime,
) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE albums SET
                 asked    = ?1,
                 asks     = asks + ?2,
                 refusals = CASE WHEN ?3 THEN refusals + 1 ELSE 0 END
             WHERE id = ?4",
            params![
                store::to_nanos(now),
                why.asks(),
                why.refused(),
                album.get() as i64
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownAlbum(album));
    }
    Ok(())
}

pub(crate) fn land_release(
    tx: &Transaction<'_>,
    album: AlbumId,
    release: &Release,
    now: SystemTime,
) -> Result<()> {
    let id = album.get() as i64;
    let changed = tx
        .execute(
            "UPDATE albums SET
                 mbid           = ?1,
                 release_group  = ?2,
                 release_title  = ?3,
                 date           = ?4,
                 country        = ?5,
                 label          = ?6,
                 catalog_number = ?7,
                 barcode        = ?8,
                 kind           = ?9,
                 disambiguation = ?10,
                 year           = coalesce(year, ?11),
                 asks           = 0,
                 refusals       = 0,
                 asked          = ?12,
                 answered       = ?12
             WHERE id = ?13",
            params![
                release.id.as_str(),
                release.group.as_ref().map(Mbid::as_str),
                release.title,
                release.date,
                release.country,
                release.label,
                release.catalog_number,
                release.barcode,
                release.kind,
                release.disambiguation,
                release.date.as_deref().and_then(store::year),
                store::to_nanos(now),
                id,
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownAlbum(album));
    }
    gather_under(tx, id, &release.id)?;

    let held = wants_under(tx, id)?;
    tx.execute(
        "DELETE FROM release_tracks WHERE album_id = ?1",
        params![id],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    tx.execute("DELETE FROM album_links WHERE album_id = ?1", params![id])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    write_links(tx, "album_links", "album_id", id, &release.links)?;
    write_media(tx, id, &release.media)?;

    let mut insert = tx
        .prepare(
            "INSERT INTO release_tracks (
                 album_id, disc, position, number, title, artist, recording_mbid, track_mbid,
                 length_ms, isrc, folded
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             RETURNING id",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for medium in &release.media {
        for track in &medium.tracks {
            let row: i64 = insert
                .query_row(
                    params![
                        id,
                        i64::from(medium.position),
                        i64::from(track.position),
                        track.number,
                        track.title,
                        track.artist,
                        track.recording.as_ref().map(Mbid::as_str),
                        track.track.as_ref().map(Mbid::as_str),
                        track
                            .length
                            .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX)),
                        track.isrc,
                        release_track_haystack(release, track),
                    ],
                    |row| row.get(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;
            write_links(
                tx,
                "release_track_links",
                "release_track_id",
                row,
                &track.links,
            )?;
        }
    }
    drop(insert);

    want_again(tx, id, held)
}

fn release_track_haystack(release: &Release, track: &ReleaseTrack) -> String {
    let credited = release.credited_as();
    let artist = track.artist.as_deref().unwrap_or(&credited);

    store::folded_letters(&format!("{} {artist} {}", track.title, release.title))
}

pub(crate) fn land_release_group(
    tx: &Transaction<'_>,
    album: AlbumId,
    group: &ReleaseGroup,
    now: SystemTime,
) -> Result<()> {
    let id = album.get() as i64;
    let changed = tx
        .execute(
            "UPDATE albums SET
                 release_group  = ?1,
                 kind           = coalesce(kind, ?2),
                 date           = coalesce(date, ?3),
                 year           = coalesce(year, ?4),
                 disambiguation = coalesce(disambiguation, ?5),
                 refusals       = 0,
                 answered       = ?6
             WHERE id = ?7",
            params![
                group.id.as_str(),
                group.kind,
                group.first_released,
                group.first_released.as_deref().and_then(store::year),
                group.disambiguation,
                store::to_nanos(now),
                id,
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownAlbum(album));
    }

    tx.execute("DELETE FROM album_links WHERE album_id = ?1", params![id])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    write_links(tx, "album_links", "album_id", id, &group.links)
}

fn write_media(tx: &Transaction<'_>, album: i64, media: &[Medium]) -> Result<()> {
    tx.execute(
        "DELETE FROM release_media WHERE album_id = ?1",
        params![album],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;

    let mut insert = tx
        .prepare(
            "INSERT INTO release_media (album_id, position, format, title)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for medium in media {
        insert
            .execute(params![
                album,
                i64::from(medium.position),
                medium.format,
                medium.title
            ])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    Ok(())
}

fn gather_under(tx: &Transaction<'_>, album: i64, release: &Mbid) -> Result<()> {
    let key = store::release_key(release.as_str());
    for other in albums_sharing(tx, album, release.as_str(), &key)? {
        gather(tx, album, other)?;
    }

    store::key_album(tx, &key, album)
}

fn albums_sharing(tx: &Transaction<'_>, album: i64, release: &str, key: &str) -> Result<Vec<i64>> {
    let mut statement = tx
        .prepare(
            "SELECT DISTINCT a.id FROM albums a
               LEFT JOIN album_keys k ON k.album_id = a.id
              WHERE a.id != ?1 AND (a.mbid = ?2 OR k.key = ?3)",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album, release, key], |row| row.get(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn gather(tx: &Transaction<'_>, into: i64, other: i64) -> Result<()> {
    tracing::debug!(
        into,
        other,
        "two albums the catalog grouped apart hold one release and are gathered"
    );

    for moving in [
        "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
        "UPDATE release_tracks SET album_id = ?1 WHERE album_id = ?2",
        "UPDATE album_keys SET album_id = ?1 WHERE album_id = ?2",
    ] {
        tx.execute(moving, params![into, other])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }

    tx.execute(
        &format!(
            "UPDATE albums SET
             artist_id     = CASE WHEN albums.artist_id IS o.artist_id THEN albums.artist_id END,
             cover_art     = CASE WHEN {uncovered} THEN o.cover_art ELSE albums.cover_art END,
             cover_format  = CASE WHEN {uncovered} THEN o.cover_format ELSE albums.cover_format END,
             cover_source  = CASE WHEN {uncovered} THEN o.cover_source ELSE albums.cover_source END,
             cover_key     = CASE WHEN {uncovered} THEN o.cover_key ELSE albums.cover_key END,
             cover_path    = CASE WHEN {uncovered} THEN o.cover_path ELSE albums.cover_path END,
             favourite     = coalesce(min(albums.favourite, o.favourite), albums.favourite, o.favourite),
             year          = coalesce(albums.year, o.year),
             tagged_tracks = coalesce(albums.tagged_tracks, o.tagged_tracks)
          FROM (SELECT * FROM albums WHERE id = ?2) AS o
         WHERE albums.id = ?1",
            uncovered = UNCOVERED,
        ),
        params![into, other],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))?;

    tx.execute("DELETE FROM albums WHERE id = ?1", params![other])
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Delete, source))
}

fn wants_under(tx: &Transaction<'_>, album: i64) -> Result<Vec<HeldWant>> {
    let mut statement = tx
        .prepare(
            "SELECT rt.track_mbid, rt.recording_mbid, rt.disc, rt.position,
                    w.wanted, w.tried, w.offered
               FROM wants w JOIN release_tracks rt ON rt.id = w.release_track_id
              WHERE rt.album_id = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album], |row| {
            Ok(HeldWant {
                track: row.get(0)?,
                recording: row.get(1)?,
                disc: row.get(2)?,
                position: row.get(3)?,
                wanted: row.get(4)?,
                tried: row.get(5)?,
                offered: row.get(6)?,
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn want_again(tx: &Transaction<'_>, album: i64, held: Vec<HeldWant>) -> Result<()> {
    let mut landing = Vec::with_capacity(held.len());
    for want in held {
        if let Some((carried, row)) = row_again(tx, album, &want)? {
            landing.push((carried, row, want));
        }
    }
    landing.sort_by_key(|(carried, ..)| *carried);

    let mut insert = tx
        .prepare(
            "INSERT INTO wants (release_track_id, wanted, tried, offered)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(release_track_id) DO NOTHING",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for (_, row, want) in landing {
        insert
            .execute(params![row, want.wanted, want.tried, want.offered])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    Ok(())
}

fn row_again(tx: &Transaction<'_>, album: i64, want: &HeldWant) -> Result<Option<(Carried, i64)>> {
    let found: Option<(i64, i64)> = tx
        .query_row(
            "SELECT id,
                    CASE WHEN track_mbid = ?2     THEN 0
                         WHEN recording_mbid = ?3 THEN 1
                         ELSE 2 END
               FROM release_tracks
              WHERE album_id = ?1
                AND (track_mbid = ?2 OR recording_mbid = ?3 OR (disc = ?4 AND position = ?5))
              ORDER BY 2
              LIMIT 1",
            params![album, want.track, want.recording, want.disc, want.position],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    Ok(found.and_then(|(row, rank)| {
        Carried::of(usize::try_from(rank).unwrap_or(usize::MAX)).map(|carried| (carried, row))
    }))
}

fn write_links(
    tx: &Transaction<'_>,
    table: &str,
    column: &str,
    owner: i64,
    links: &[Link],
) -> Result<()> {
    if links.is_empty() {
        return Ok(());
    }
    let mut insert = tx
        .prepare(&format!(
            "INSERT INTO {table} ({column}, relation, provider, url) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT DO NOTHING"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    for link in links {
        insert
            .execute(params![
                owner,
                store::relation_code(link.relation),
                store::service_code(link.service),
                link.url,
            ])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    Ok(())
}

pub(crate) fn rematch_release_tracks(tx: &Transaction<'_>, album: AlbumId) -> Result<u32> {
    let id = album.get() as i64;
    let rows = release_rows(tx, id)?;
    let tracks = catalog_rows(tx, id)?;
    let discs = rows.iter().map(|row| row.disc).max().unwrap_or(ONE_DISC);

    let mut matched: Vec<Option<i64>> = vec![None; rows.len()];
    let mut taken = vec![false; tracks.len()];
    let passes: [Pairing<'_>; 4] = [
        &|row, track| row.recording.is_some() && row.recording == track.recording,
        &|row, track| row.track.is_some() && row.track == track.release_track,
        &|row, track| {
            track.number == Some(row.position)
                && track.disc.or((discs == ONE_DISC).then_some(ONE_DISC)) == Some(row.disc)
        },
        &|row, track| row.folded == track.folded,
    ];
    for pairs in passes {
        pair(&rows, &tracks, &mut matched, &mut taken, pairs);
    }

    let mut update = tx
        .prepare("UPDATE release_tracks SET track_id = ?1 WHERE id = ?2")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut identify = tx
        .prepare(
            "UPDATE tracks SET mbid = coalesce(mbid, ?1),
                               release_track_mbid = coalesce(release_track_mbid, ?2),
                               isrc = coalesce(isrc, ?3)
              WHERE id = ?4",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut moved = 0;
    for (row, track) in rows.iter().zip(&matched) {
        if let Some(track) = track {
            identify
                .execute(params![row.recording, row.track, row.isrc, track])
                .map_err(|source| Error::store(StoreOp::Update, source))?;
        }
        if row.paired == *track {
            continue;
        }
        update
            .execute(params![track, row.id])
            .map_err(|source| Error::store(StoreOp::Update, source))?;
        moved += 1;
    }

    Ok(moved)
}

fn pair(
    rows: &[ReleaseRow],
    tracks: &[CatalogRow],
    matched: &mut [Option<i64>],
    taken: &mut [bool],
    pairs: Pairing<'_>,
) {
    for (at, row) in rows.iter().enumerate() {
        if matched[at].is_some() {
            continue;
        }
        let found = tracks
            .iter()
            .enumerate()
            .find(|(candidate, track)| !taken[*candidate] && pairs(row, track));
        if let Some((candidate, track)) = found {
            matched[at] = Some(track.id);
            taken[candidate] = true;
        }
    }
}

fn release_rows(tx: &Transaction<'_>, album: i64) -> Result<Vec<ReleaseRow>> {
    let mut statement = tx
        .prepare(
            "SELECT id, disc, position, recording_mbid, track_mbid, isrc, title, track_id
               FROM release_tracks WHERE album_id = ?1 ORDER BY disc, position",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album], |row| {
            Ok(ReleaseRow {
                id: row.get(0)?,
                disc: row.get(1)?,
                position: row.get(2)?,
                recording: row.get(3)?,
                track: row.get(4)?,
                isrc: row.get(5)?,
                folded: folded_title(&row.get::<_, String>(6)?),
                paired: row.get(7)?,
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn catalog_rows(tx: &Transaction<'_>, album: i64) -> Result<Vec<CatalogRow>> {
    let mut statement = tx
        .prepare(
            "SELECT id, mbid, release_track_mbid, disc_number, track_number, title
               FROM tracks WHERE album_id = ?1 ORDER BY disc_number, track_number, id",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    statement
        .query_map(params![album], |row| {
            Ok(CatalogRow {
                id: row.get(0)?,
                recording: row.get(1)?,
                release_track: row.get(2)?,
                disc: row.get(3)?,
                number: row.get(4)?,
                folded: folded_title(&row.get::<_, String>(5)?),
            })
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn land_archive_cover(
    tx: &Transaction<'_>,
    album: AlbumId,
    art: &CoverArt,
) -> Result<bool> {
    tx.execute(
        "UPDATE albums SET cover_art = ?1, cover_format = ?2, cover_source = ?3
          WHERE id = ?4 AND cover_art IS NULL AND cover_path IS NULL",
        params![
            art.bytes,
            store::image_format_code(art.format),
            store::cover_source_code(CoverSource::Archive),
            album.get() as i64,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn note_cover_asked(tx: &Transaction<'_>, album: AlbumId, at: SystemTime) -> Result<()> {
    tx.execute(
        "UPDATE albums SET cover_asked = ?1 WHERE id = ?2",
        params![store::to_nanos(at), album.get() as i64],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn ask_again_for_covers(tx: &Transaction<'_>) -> Result<usize> {
    tx.execute(
        &format!(
            "UPDATE albums SET cover_asked = NULL WHERE {UNCOVERED} AND cover_asked IS NOT NULL"
        ),
        [],
    )
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn albums_wanting_a_cover(connection: &Connection) -> Result<Vec<CoverWanted>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT id, mbid, release_group FROM albums
              WHERE {UNCOVERED} AND cover_asked IS NULL
                AND (mbid IS NOT NULL OR release_group IS NOT NULL)
              ORDER BY id"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let held = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut wanting = Vec::with_capacity(held.len());
    for (album, release, group) in held {
        let group = store::mbid_in(group.as_deref());
        let from = match (store::mbid_in(release.as_deref()), group) {
            (Some(release), group) => CoverFrom::Release { release, group },
            (None, Some(group)) => CoverFrom::Group(group),
            (None, None) => continue,
        };
        wanting.push(CoverWanted {
            album: AlbumId::new(album as u64)?,
            from,
        });
    }
    Ok(wanting)
}

pub(crate) fn land_vault_cover(
    tx: &Transaction<'_>,
    album: AlbumId,
    key: VaultKey,
    within: &str,
) -> Result<bool> {
    tx.execute(
        "UPDATE albums SET cover_key = ?1, cover_path = ?2, cover_source = ?3
          WHERE id = ?4 AND cover_art IS NULL AND cover_path IS NULL",
        params![
            key.to_string(),
            within,
            store::cover_source_code(CoverSource::Vault),
            album.get() as i64,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn vault_the_cover(
    tx: &Transaction<'_>,
    album: AlbumId,
    key: VaultKey,
    within: &str,
) -> Result<bool> {
    tx.execute(
        "UPDATE albums SET cover_key = ?1, cover_path = ?2, cover_source = ?3,
                           cover_art = NULL, cover_format = NULL
          WHERE id = ?4 AND cover_path IS NULL",
        params![
            key.to_string(),
            within,
            store::cover_source_code(CoverSource::Vault),
            album.get() as i64,
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn stamp_track_asked(
    tx: &Transaction<'_>,
    track: TrackId,
    why: Fruitless,
    now: SystemTime,
) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE tracks SET
                 asked    = ?1,
                 asks     = asks + ?2,
                 refusals = CASE WHEN ?3 THEN refusals + 1 ELSE 0 END
             WHERE id = ?4",
            params![
                store::to_nanos(now),
                why.asks(),
                why.refused(),
                track.get() as i64
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownTrack(track));
    }
    Ok(())
}

struct HeldNames {
    title: String,
    artist: Option<String>,
    album: Option<String>,
    genre: Option<String>,
    artist_id: Option<i64>,
}

fn held_names(tx: &Transaction<'_>, track: i64) -> Result<Option<HeldNames>> {
    tx.query_row(
        "SELECT t.title, t.artist, a.title, t.genre, t.artist_id
           FROM tracks t LEFT JOIN albums a ON a.id = t.album_id
          WHERE t.id = ?1",
        params![track],
        |row| {
            Ok(HeldNames {
                title: row.get(0)?,
                artist: row.get(1)?,
                album: row.get(2)?,
                genre: row.get(3)?,
                artist_id: row.get(4)?,
            })
        },
    )
    .optional()
    .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn land_recording(
    tx: &Transaction<'_>,
    track: TrackId,
    recording: &Recording,
    certainty: Certainty,
    release: Option<&RecordingRelease>,
    now: SystemTime,
) -> Result<bool> {
    let id = track.get() as i64;
    let Some(held) = held_names(tx, id)? else {
        return Err(Error::UnknownTrack(track));
    };

    let lead = recording.credit.first();
    let billing = recording.credited_as();
    let billed = (!billing.trim().is_empty()).then_some(billing);
    let artist_id = match (&billed, lead) {
        (Some(_), Some(lead)) => Some(store::artist_named_in(
            tx,
            &lead.name,
            lead.mbid.as_ref().map(Mbid::as_str),
        )?),
        (Some(_) | None, _) => None,
    };
    if billed.is_some() {
        for member in recording.credit.iter().skip(1) {
            store::artist_named_in(tx, &member.name, member.mbid.as_ref().map(Mbid::as_str))?;
        }
    }

    let (title, artist) = tx
        .query_row(
            "UPDATE tracks SET
                 title  = coalesce(CASE WHEN ?1 OR tagged_title  IS NULL THEN ?2 END, title),
                 artist = coalesce(CASE WHEN ?1 OR tagged_artist IS NULL THEN ?3 END, artist),
                 artist_id = coalesce(?4, artist_id),
                 track_number = coalesce(track_number, ?5),
                 disc_number  = coalesce(disc_number,  ?6),
                 mbid = coalesce(mbid, ?7),
                 isrc = coalesce(isrc, ?8),
                 release_title = ?9,
                 asks = 0, refusals = 0, asked = ?10, answered = ?10
              WHERE id = ?11
             RETURNING title, artist",
            params![
                certainty == Certainty::Exactly,
                recording.title,
                billed,
                artist_id,
                release.and_then(|release| release.position),
                release.and_then(|release| release.disc),
                recording.id.as_str(),
                recording.isrcs.first().map(Isrc::as_str),
                release.map(|release| release.title.as_str()),
                store::to_nanos(now),
                id,
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;

    store::index_row(
        tx,
        id,
        &title,
        artist.as_deref().unwrap_or_default(),
        held.album.as_deref().unwrap_or_default(),
        &store::indexed_genre_of(tx, held.genre.as_deref(), artist_id.or(held.artist_id))?,
    )?;

    Ok(title != held.title || artist != held.artist)
}

pub(crate) fn stamp_artist_asked(
    tx: &Transaction<'_>,
    artist: ArtistId,
    why: Fruitless,
    now: SystemTime,
) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE artists SET
                 asked    = ?1,
                 asks     = asks + ?2,
                 refusals = CASE WHEN ?3 THEN refusals + 1 ELSE 0 END
             WHERE id = ?4",
            params![
                store::to_nanos(now),
                why.asks(),
                why.refused(),
                artist.get() as i64
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownArtist(artist));
    }
    Ok(())
}

pub(crate) fn land_artist(
    tx: &Transaction<'_>,
    artist: ArtistId,
    profile: &ArtistProfile,
    now: SystemTime,
) -> Result<()> {
    let id = artist.get() as i64;
    let changed = tx
        .execute(
            "UPDATE artists SET
                 mbid           = ?1,
                 sort_name      = ?2,
                 kind           = ?3,
                 gender         = ?4,
                 country        = ?5,
                 area           = ?6,
                 began_in       = ?7,
                 began          = ?8,
                 ended          = ?9,
                 has_ended      = ?10,
                 disambiguation = ?11,
                 asks           = 0,
                 refusals       = 0,
                 asked          = ?12,
                 answered       = ?12
             WHERE id = ?13",
            params![
                profile.mbid.as_str(),
                profile.sort_name,
                profile.kind,
                profile.gender,
                profile.country,
                profile.area,
                profile.began_in,
                profile.span.begin,
                profile.span.end,
                profile.span.ended,
                profile.disambiguation,
                store::to_nanos(now),
                id,
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownArtist(artist));
    }

    tx.execute(
        "DELETE FROM artist_genres WHERE artist_id = ?1",
        params![id],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let mut insert = tx
        .prepare(
            "INSERT INTO artist_genres (artist_id, name, weight) VALUES (?1, ?2, ?3)
             ON CONFLICT(artist_id, name) DO UPDATE SET weight = max(weight, excluded.weight)",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for genre in &profile.genres {
        insert
            .execute(params![id, genre.name, i64::from(genre.weight)])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    drop(insert);
    store::reindex_the_tracks_of(tx, id)?;

    tx.execute("DELETE FROM artist_links WHERE artist_id = ?1", params![id])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    write_links(tx, "artist_links", "artist_id", id, &profile.links)
}

pub(crate) fn land_artist_releases(
    tx: &Transaction<'_>,
    artist: ArtistId,
    releases: &[ArtistRelease],
) -> Result<usize> {
    let id = artist.get() as i64;
    let known = tx
        .query_row("SELECT 1 FROM artists WHERE id = ?1", params![id], |_| {
            Ok(())
        })
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if known.is_none() {
        return Err(Error::UnknownArtist(artist));
    }

    tx.execute(
        "DELETE FROM artist_releases WHERE artist_id = ?1",
        params![id],
    )
    .map_err(|source| Error::store(StoreOp::Delete, source))?;
    let mut insert = tx
        .prepare(
            "INSERT INTO artist_releases (artist_id, mbid, title, kind, first_released, folded)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(artist_id, mbid) DO NOTHING",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut written = 0;
    for release in releases {
        written += insert
            .execute(params![
                id,
                release.mbid.as_str(),
                release.title,
                release.kind,
                release.first_released,
                spelt_out(release)
            ])
            .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }

    Ok(written)
}

fn spelt_out(release: &ArtistRelease) -> String {
    let mut said = release.title.clone();
    for also in [release.kind.as_deref(), release.first_released.as_deref()]
        .into_iter()
        .flatten()
    {
        said.push(' ');
        said.push_str(also);
    }

    store::folded_letters(&said)
}

pub(crate) fn land_portrait(
    tx: &Transaction<'_>,
    artist: ArtistId,
    art: &CoverArt,
) -> Result<bool> {
    tx.execute(
        "UPDATE artists SET portrait = ?1, portrait_format = ?2
          WHERE id = ?3 AND portrait IS NULL",
        params![
            art.bytes,
            store::image_format_code(art.format),
            artist.get() as i64
        ],
    )
    .map(|changed| changed > 0)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

pub(crate) fn write_artist_mbid(tx: &Transaction<'_>, artist: ArtistId, mbid: &Mbid) -> Result<()> {
    let changed = tx
        .execute(
            "UPDATE artists SET mbid = ?1 WHERE id = ?2",
            params![mbid.as_str(), artist.get() as i64],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    if changed == 0 {
        return Err(Error::UnknownArtist(artist));
    }
    Ok(())
}

fn due(waits: Waits, refresh: bool, now: SystemTime) -> [Value; 5] {
    let before =
        |ago: Duration| Value::Integer(store::to_nanos(now.checked_sub(ago).unwrap_or(UNIX_EPOCH)));
    let nanos = |wait: Duration| Value::Integer(i64::try_from(wait.as_nanos()).unwrap_or(i64::MAX));
    [
        Value::Integer(i64::from(refresh)),
        nanos(waits.retry_after),
        Value::Integer(store::to_nanos(now)),
        before(waits.refresh_after),
        nanos(waits.refused_again_after),
    ]
}

fn due_for(asked: [Value; 5], id: u64) -> [Value; 6] {
    let [refresh, waited, now, refreshed, refused] = asked;
    [
        refresh,
        waited,
        now,
        refreshed,
        refused,
        Value::Integer(id as i64),
    ]
}

fn asking_albums(due: &str) -> String {
    format!(
        "SELECT a.id, a.title, r.name,
                (SELECT count(*) FROM tracks t WHERE t.album_id = a.id),
                a.year, a.mbid, a.release_group, a.barcode, a.catalog_number,
                a.tagged_tracks, r.mbid, (a.cover_art IS NOT NULL OR a.cover_path IS NOT NULL),
                {HOLDS_RELEASE_ROWS}, {due}
           FROM albums a LEFT JOIN artists r ON r.id = a.artist_id"
    )
}

pub(crate) fn albums_to_ask(
    connection: &Connection,
    waits: Waits,
    refresh: bool,
    now: SystemTime,
) -> Result<Vec<AlbumToAsk>> {
    let album_due = album_due();
    let asking = asking_albums(&album_due);
    let mut statement = connection
        .prepare(&format!(
            "{asking}
              WHERE {album_due}
                 OR ({HOLDS_RELEASE_ROWS} AND ({HOLDS_AN_UNPAIRED_ROW} OR {HOLDS_AN_UNPAIRED_TRACK}))
              ORDER BY a.id"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let found = statement
        .query_map(due(waits, refresh, now), RawAlbumToAsk::read)
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.into_iter().map(RawAlbumToAsk::into_album).collect()
}

pub(crate) fn album_to_ask(connection: &Connection, id: AlbumId) -> Result<Option<AlbumToAsk>> {
    let asking = asking_albums(ASKED_NOW);
    let found = connection
        .query_row(
            &format!("{asking} WHERE a.id = ?1"),
            [id.get() as i64],
            RawAlbumToAsk::read,
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.map(RawAlbumToAsk::into_album).transpose()
}

pub(crate) fn album_if_due(
    connection: &Connection,
    id: AlbumId,
    waits: Waits,
    refresh: bool,
    now: SystemTime,
) -> Result<Option<AlbumToAsk>> {
    let album_due = album_due();
    let asking = asking_albums(&album_due);
    let found = connection
        .query_row(
            &format!("{asking} WHERE a.id = ?6 AND {album_due}"),
            params_from_iter(due_for(due(waits, refresh, now), id.get())),
            RawAlbumToAsk::read,
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.map(RawAlbumToAsk::into_album).transpose()
}

struct RawAlbumToAsk {
    id: i64,
    title: String,
    owner: Option<String>,
    track_count: u32,
    year: Option<i32>,
    mbid: Option<String>,
    group: Option<String>,
    barcode: Option<String>,
    catalog_number: Option<String>,
    tagged_tracks: Option<u32>,
    owner_mbid: Option<String>,
    has_cover: bool,
    has_release_rows: bool,
    due: bool,
}

impl RawAlbumToAsk {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            title: row.get(1)?,
            owner: row.get(2)?,
            track_count: row.get(3)?,
            year: row.get(4)?,
            mbid: row.get(5)?,
            group: row.get(6)?,
            barcode: row.get(7)?,
            catalog_number: row.get(8)?,
            tagged_tracks: row.get(9)?,
            owner_mbid: row.get(10)?,
            has_cover: row.get(11)?,
            has_release_rows: row.get(12)?,
            due: row.get(13)?,
        })
    }

    fn into_album(self) -> Result<AlbumToAsk> {
        Ok(AlbumToAsk {
            id: AlbumId::new(self.id as u64)?,
            title: self.title,
            owner: self.owner,
            track_count: self.track_count,
            year: self.year,
            mbid: self.mbid.as_deref().map(Mbid::new).transpose()?,
            group: self.group.as_deref().map(Mbid::new).transpose()?,
            barcode: self.barcode,
            catalog_number: self.catalog_number,
            tagged_tracks: self.tagged_tracks,
            owner_mbid: owner_mbid_held(self.owner_mbid.as_deref()),
            has_cover: self.has_cover,
            has_release_rows: self.has_release_rows,
            rematch_only: !self.due,
        })
    }
}

fn owner_mbid_held(stored: Option<&str>) -> Option<Mbid> {
    let text = stored?;
    match Mbid::new(text) {
        Ok(mbid) => Some(mbid),
        Err(error) => {
            tracing::debug!(%error, text, "an artist's row holds what is not an id");
            None
        }
    }
}

pub(crate) fn tracks_to_ask(
    connection: &Connection,
    waits: Waits,
    refresh: bool,
    now: SystemTime,
) -> Result<Vec<TrackId>> {
    let track_due = due_again("t");
    let mut statement = connection
        .prepare(&format!(
            "SELECT t.id FROM tracks t
              WHERE NOT {IS_PAIRED} AND {track_due}
              ORDER BY t.id"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let found = statement
        .query_map(due(waits, refresh, now), |row| row.get::<_, i64>(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found
        .into_iter()
        .map(|id| TrackId::new(id as u64).map_err(Error::from))
        .collect()
}

const HELD_RECORDING: &str = "t.mbid";

const HELD_OR_PAIRED_RECORDING: &str =
    "coalesce(t.mbid, (SELECT rt.recording_mbid FROM release_tracks rt
                                                  WHERE rt.track_id = t.id
                                                    AND rt.recording_mbid IS NOT NULL
                                                  LIMIT 1))";

fn track_read(
    connection: &Connection,
    id: TrackId,
    recording: &str,
    also: &str,
) -> Result<Option<TrackToAsk>> {
    let found = connection
        .query_row(
            &format!(
                "SELECT t.title, t.tagged_title, t.artist, t.tagged_artist, t.album_id,
                        a.answered IS NOT NULL, t.path, t.span_start, t.span_frames,
                        t.duration, t.sample_rate, {recording}, t.isrc, ta.mbid,
                        coalesce(a.release_title, a.title), oa.name, oa.mbid
                   FROM tracks t
                        LEFT JOIN albums a ON a.id = t.album_id
                        LEFT JOIN artists ta ON ta.id = t.artist_id
                        LEFT JOIN artists oa ON oa.id = a.artist_id
                  WHERE t.id = ?1 {also}"
            ),
            [id.get() as i64],
            RawTrackToAsk::read,
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found.map(|raw| raw.into_track(id)).transpose()
}

pub(crate) fn track_to_ask(connection: &Connection, id: TrackId) -> Result<Option<TrackToAsk>> {
    track_read(
        connection,
        id,
        HELD_RECORDING,
        &format!("AND NOT {IS_PAIRED}"),
    )
}

pub(crate) fn track_as_heard(connection: &Connection, id: TrackId) -> Result<Option<TrackToAsk>> {
    track_read(connection, id, HELD_OR_PAIRED_RECORDING, "")
}

struct RawTrackToAsk {
    title: String,
    tagged_title: Option<String>,
    artist: Option<String>,
    tagged_artist: Option<String>,
    album: Option<i64>,
    album_answered: bool,
    path: String,
    span_start: i64,
    span_frames: Option<i64>,
    duration: Option<i64>,
    sample_rate: u32,
    mbid: Option<String>,
    isrc: Option<String>,
    artist_mbid: Option<String>,
    album_title: Option<String>,
    owner: Option<String>,
    owner_mbid: Option<String>,
}

impl RawTrackToAsk {
    fn read(row: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            title: row.get(0)?,
            tagged_title: row.get(1)?,
            artist: row.get(2)?,
            tagged_artist: row.get(3)?,
            album: row.get(4)?,
            album_answered: row.get(5)?,
            path: row.get(6)?,
            span_start: row.get(7)?,
            span_frames: row.get(8)?,
            duration: row.get(9)?,
            sample_rate: row.get(10)?,
            mbid: row.get(11)?,
            isrc: row.get(12)?,
            artist_mbid: row.get(13)?,
            album_title: row.get(14)?,
            owner: row.get(15)?,
            owner_mbid: row.get(16)?,
        })
    }

    fn into_track(self, id: TrackId) -> Result<TrackToAsk> {
        let rate = SampleRate::new(self.sample_rate)?;

        Ok(TrackToAsk {
            id,
            title: self.title,
            tagged_title: self.tagged_title,
            artist: self.artist,
            tagged_artist: self.tagged_artist,
            artist_mbid: owner_mbid_held(self.artist_mbid.as_deref()),
            album: self.album.map(|id| AlbumId::new(id as u64)).transpose()?,
            album_title: self.album_title,
            album_answered: self.album_answered,
            owner: self.owner,
            owner_mbid: owner_mbid_held(self.owner_mbid.as_deref()),
            location: MediaLocation::local(self.path),
            span: store::span(self.span_start, self.span_frames),
            length: self
                .duration
                .map(|frames| Frames(frames.max(0) as u64).to_duration(rate)),
            mbid: self.mbid.as_deref().map(Mbid::new).transpose()?,
            isrc: self.isrc.as_deref().map(Isrc::new).transpose()?,
        })
    }
}

pub(crate) fn artist_to_ask(connection: &Connection, id: ArtistId) -> Result<Option<ArtistToAsk>> {
    let found = connection
        .query_row(
            "SELECT name, mbid, portrait IS NOT NULL FROM artists WHERE id = ?1",
            [id.get() as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, bool>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found
        .map(|(name, mbid, has_portrait)| {
            Ok(ArtistToAsk {
                id,
                name,
                mbid: mbid.as_deref().map(Mbid::new).transpose()?,
                has_portrait,
            })
        })
        .transpose()
}

pub(crate) fn artist_is_due(
    connection: &Connection,
    id: ArtistId,
    waits: Waits,
    refresh: bool,
    now: SystemTime,
) -> Result<bool> {
    connection
        .query_row(
            &format!(
                "SELECT 1 FROM artists a WHERE a.id = ?6 AND {}",
                due_again("a")
            ),
            params_from_iter(due_for(due(waits, refresh, now), id.get())),
            |_| Ok(()),
        )
        .optional()
        .map(|found: Option<()>| found.is_some())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

pub(crate) fn artists_to_ask(
    connection: &Connection,
    waits: Waits,
    refresh: bool,
    now: SystemTime,
) -> Result<Vec<ArtistId>> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT a.id FROM artists a WHERE {} ORDER BY a.id",
            due_again("a")
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;

    let found = statement
        .query_map(due(waits, refresh, now), |row| row.get::<_, i64>(0))
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    found
        .into_iter()
        .map(|id| ArtistId::new(id as u64).map_err(Error::from))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use resonate_codec::{Codec, TagSet};
    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
    use rusqlite::Connection;

    use super::*;
    use crate::{
        Medium, ReleaseTrack, schema,
        store::{Cache, TrackRecord},
    };

    const RELEASE: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";
    const RECORDING: &str = "83d91898-7763-47d7-b03b-b92132375c47";
    const TRACK: &str = "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11";
    const ALBUM: &str = "Meddle";
    const GROUP: &str = "2f6a1b3c-7d8e-4f90-a1b2-c3d4e5f60718";

    fn scanned(title: &str, number: Option<u32>, recording: Option<&str>) -> TrackRecord {
        TrackRecord {
            root_id: 1,
            path: PathBuf::from(format!("/music/Meddle/{title}.wav")),
            sleeve: Some(PathBuf::from("/music/Meddle")),
            existing: None,
            file_size: 0,
            modified: UNIX_EPOCH,
            sheet_modified: None,
            spec: StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            codec: Codec::Pcm,
            duration: None,
            span: None,
            tags: TagSet {
                title: Some(title.to_owned()),
                album: Some(ALBUM.to_owned()),
                track_number: number,
                musicbrainz_track_id: recording.map(str::to_owned),
                ..TagSet::default()
            },
            embeds_a_picture: false,
        }
    }

    fn tagged(title: &str, recording: Option<&str>, release_track: Option<&str>) -> TrackRecord {
        let mut record = scanned(title, None, recording);
        record.tags.musicbrainz_release_track_id = release_track.map(str::to_owned);
        record
    }

    fn row(position: u32, title: &str, recording: Option<&str>) -> ReleaseTrack {
        ReleaseTrack {
            position,
            number: position.to_string(),
            title: title.to_owned(),
            artist: None,
            recording: recording.map(|id| Mbid::new(id).expect("a well-formed mbid")),
            track: None,
            length: None,
            isrc: None,
            links: Vec::new(),
        }
    }

    fn row_of_the_release(position: u32, title: &str, track: &str) -> ReleaseTrack {
        ReleaseTrack {
            track: Some(Mbid::new(track).expect("a well-formed mbid")),
            ..row(position, title, None)
        }
    }

    fn meddle(tracks: Vec<ReleaseTrack>) -> Release {
        Release {
            id: Mbid::new(RELEASE).expect("a well-formed mbid"),
            group: None,
            title: ALBUM.to_owned(),
            credit: Vec::new(),
            date: Some("1971-10-30".to_owned()),
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
                tracks,
            }],
        }
    }

    const A_DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const AN_HOUR: Duration = Duration::from_secs(60 * 60);
    const A_MONTH: Duration = Duration::from_secs(30 * 24 * 60 * 60);
    const A_MOMENT: Duration = Duration::from_secs(1);
    const WAITED: Waits = Waits {
        retry_after: A_DAY,
        refused_again_after: AN_HOUR,
        refresh_after: A_MONTH,
    };

    fn one_album(tx: &Transaction<'_>) -> AlbumId {
        let mut cache = Cache::default();
        store::apply(
            tx,
            &mut cache,
            &scanned("One of These Days", Some(1), None),
            0,
            false,
        )
        .expect("the track is stored");
        let grouped: i64 = tx
            .query_row("SELECT id FROM albums", [], |row| row.get(0))
            .expect("one album was grouped");

        AlbumId::new(grouped as u64).expect("a non-zero id")
    }

    #[test]
    fn an_album_that_answers_nothing_waits_twice_as_long_before_it_is_asked_again() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let album = one_album(&tx);
        let asked = UNIX_EPOCH + A_MONTH;
        let due_at = |now: SystemTime| {
            !albums_to_ask(&tx, WAITED, false, now)
                .expect("the albums read back")
                .is_empty()
        };

        assert!(
            due_at(asked),
            "an album nothing has asked about is due at once"
        );

        stamp_album_asked(&tx, album, Fruitless::Missed, asked).expect("the album is stamped");
        assert!(!due_at(asked + A_MOMENT));
        assert!(due_at(asked + A_DAY + A_MOMENT));

        stamp_album_asked(&tx, album, Fruitless::Missed, asked)
            .expect("the album is stamped again");
        assert!(
            !due_at(asked + A_DAY + A_MOMENT),
            "an album asked twice in vain was asked again after one wait"
        );
        assert!(due_at(asked + A_DAY * 2 + A_MOMENT));

        stamp_album_asked(&tx, album, Fruitless::Missed, asked)
            .expect("the album is stamped a third time");
        assert!(!due_at(asked + A_DAY * 2 + A_MOMENT));
        assert!(due_at(asked + A_DAY * 4 + A_MOMENT));
    }

    #[test]
    fn an_album_the_reference_refused_is_asked_again_without_the_wait_a_miss_earns() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let album = one_album(&tx);
        let asked = UNIX_EPOCH + A_MONTH;
        let due_at = |now: SystemTime| {
            !albums_to_ask(&tx, WAITED, false, now)
                .expect("the albums read back")
                .is_empty()
        };

        stamp_album_asked(&tx, album, Fruitless::Refused, asked).expect("the album is stamped");
        assert!(
            !due_at(asked + A_MOMENT),
            "a refusal is not asked again in the same breath"
        );
        assert!(
            due_at(asked + AN_HOUR + A_MOMENT),
            "a refusal waits the hour a service is given to come back"
        );

        stamp_album_asked(&tx, album, Fruitless::Refused, asked)
            .expect("the album is refused again");
        assert!(
            !due_at(asked + AN_HOUR + A_MOMENT),
            "a second refusal in a row waits twice the hour the first one did"
        );
        assert!(due_at(asked + AN_HOUR * 2 + A_MOMENT));

        stamp_album_asked(&tx, album, Fruitless::Missed, asked).expect("the album is missed");
        assert!(
            !due_at(asked + AN_HOUR + A_MOMENT),
            "a miss after a refusal waits the day a first miss earns"
        );
        assert!(due_at(asked + A_DAY + A_MOMENT));
    }

    #[test]
    fn a_service_that_refuses_a_row_for_ever_is_asked_about_it_less_and_less_often() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let album = one_album(&tx);
        let asked = UNIX_EPOCH + A_MONTH;
        let due_at = |now: SystemTime| {
            !albums_to_ask(&tx, WAITED, false, now)
                .expect("the albums read back")
                .is_empty()
        };

        for refusals in 1..=WAITS_DOUBLE_AT_MOST + 3 {
            stamp_album_asked(&tx, album, Fruitless::Refused, asked).expect("the album is refused");
            let earned = AN_HOUR * (1 << u32::min(refusals - 1, WAITS_DOUBLE_AT_MOST));
            assert!(
                !due_at(asked + earned),
                "refusal {refusals} was asked again before the wait it had earned"
            );
            assert!(
                due_at(asked + earned + A_MOMENT),
                "refusal {refusals} was not asked again once its wait was out"
            );
        }
    }

    fn the_group(releases: Vec<crate::GroupRelease>) -> ReleaseGroup {
        ReleaseGroup {
            id: Mbid::new(GROUP).expect("a well-formed mbid"),
            title: ALBUM.to_owned(),
            credit: Vec::new(),
            kind: Some("Album".to_owned()),
            first_released: Some("1971-10-30".to_owned()),
            disambiguation: None,
            links: Vec::new(),
            releases,
        }
    }

    #[test]
    fn an_album_landed_as_its_group_alone_is_asked_again_for_a_pressing() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let album = one_album(&tx);
        let asked = UNIX_EPOCH + A_MONTH;
        let due_at = |now: SystemTime| {
            albums_to_ask(&tx, WAITED, false, now)
                .expect("the albums read back")
                .iter()
                .any(|found| found.id == album && !found.rematch_only)
        };

        stamp_album_asked(&tx, album, Fruitless::Missed, asked).expect("the album is stamped");
        land_release_group(&tx, album, &the_group(Vec::new()), asked).expect("the group lands");
        assert!(
            !due_at(asked + A_MOMENT),
            "an album asked a moment ago is due again"
        );
        assert!(
            due_at(asked + A_DAY + A_MOMENT),
            "an album holding a group and no pressing waited for the refresh to go stale"
        );

        stamp_album_asked(&tx, album, Fruitless::Missed, asked)
            .expect("the album is stamped again");
        land_release_group(&tx, album, &the_group(Vec::new()), asked)
            .expect("the group lands again");
        assert!(
            !due_at(asked + A_DAY + A_MOMENT),
            "an album asked twice for a pressing was asked again after one wait"
        );
        assert!(due_at(asked + A_DAY * 2 + A_MOMENT));

        land_release(
            &tx,
            album,
            &meddle(vec![row(1, "One of These Days", None)]),
            asked,
        )
        .expect("the pressing lands");
        assert!(
            !due_at(asked + A_DAY * 4),
            "an album whose pressing landed is still being asked for one"
        );
    }

    #[test]
    fn a_release_that_lands_puts_the_wait_back_to_where_it_started() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let album = one_album(&tx);
        let asked = UNIX_EPOCH + A_MONTH;
        let due_at = |now: SystemTime| {
            albums_to_ask(&tx, WAITED, false, now)
                .expect("the albums read back")
                .iter()
                .any(|found| found.id == album && !found.rematch_only)
        };

        for _ in 0..3 {
            stamp_album_asked(&tx, album, Fruitless::Missed, asked).expect("the album is stamped");
        }
        land_release(
            &tx,
            album,
            &meddle(vec![row(1, "One of These Days", None)]),
            asked,
        )
        .expect("the release lands");
        assert!(
            !due_at(asked + A_DAY * 4),
            "an album the reference answered is asked again before it is stale"
        );

        stamp_album_asked(&tx, album, Fruitless::Missed, asked)
            .expect("the album is stamped once more");
        assert!(
            !due_at(asked + A_MOMENT),
            "an album asked a moment ago is due again"
        );
        assert!(
            due_at(asked + A_MONTH + A_MOMENT),
            "an answer that went stale did not make the album due"
        );
    }

    #[test]
    fn a_recording_id_matches_a_row_before_its_position_or_its_title_can_mislead() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let mut cache = Cache::default();
        for record in [
            scanned("One of These Days", Some(3), Some(RECORDING)),
            scanned("A Pillow of Winds", Some(2), None),
            scanned("Fearless!", None, None),
        ] {
            store::apply(&tx, &mut cache, &record, 0, false).expect("the track is stored");
        }
        let grouped: i64 = tx
            .query_row("SELECT id FROM albums", [], |row| row.get(0))
            .expect("one album was grouped");
        let album = AlbumId::new(grouped as u64).expect("a non-zero id");

        land_release(
            &tx,
            album,
            &meddle(vec![
                row(1, "One of These Days", Some(RECORDING)),
                row(2, "A Pillow of Winds", None),
                row(3, "Fearless", None),
                row(4, "San Tropez", None),
            ]),
            UNIX_EPOCH,
        )
        .expect("the release lands");
        let matched = rematch_release_tracks(&tx, album).expect("the rows are matched");
        assert_eq!(matched, 3);

        let mut statement = tx
            .prepare(
                "SELECT rt.position, t.title FROM release_tracks rt
                   LEFT JOIN tracks t ON t.id = rt.track_id ORDER BY rt.position",
            )
            .expect("the pairing is readable");
        let paired: Vec<(u32, Option<String>)> = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
            .expect("the pairing reads back");
        assert_eq!(
            paired,
            vec![
                (1, Some("One of These Days".to_owned())),
                (2, Some("A Pillow of Winds".to_owned())),
                (3, Some("Fearless!".to_owned())),
                (4, None),
            ]
        );
    }

    #[test]
    fn a_release_track_id_is_weighed_against_the_column_that_holds_one_and_not_the_recordings() {
        let mut connection = Connection::open_in_memory().expect("an in-memory database");
        schema::lay_out(&connection).expect("the schema applies");
        connection
            .execute("INSERT INTO roots (id, path) VALUES (1, '/music')", [])
            .expect("a root is stored");
        let tx = connection.transaction().expect("a transaction");
        let mut cache = Cache::default();
        for record in [
            tagged("One of These Days", Some(TRACK), None),
            tagged("A Pillow of Winds", None, Some(TRACK)),
        ] {
            store::apply(&tx, &mut cache, &record, 0, false).expect("the track is stored");
        }
        let grouped: i64 = tx
            .query_row("SELECT id FROM albums", [], |row| row.get(0))
            .expect("one album was grouped");
        let album = AlbumId::new(grouped as u64).expect("a non-zero id");

        land_release(
            &tx,
            album,
            &meddle(vec![row_of_the_release(1, "San Tropez", TRACK)]),
            UNIX_EPOCH,
        )
        .expect("the release lands");
        assert_eq!(
            rematch_release_tracks(&tx, album).expect("the rows are matched"),
            1
        );

        let paired: Option<String> = tx
            .query_row(
                "SELECT t.title FROM release_tracks rt
                   LEFT JOIN tracks t ON t.id = rt.track_id",
                [],
                |row| row.get(0),
            )
            .expect("the pairing reads back");
        assert_eq!(paired.as_deref(), Some("A Pillow of Winds"));
    }

    #[test]
    fn a_title_is_folded_to_its_letters_and_digits_lowercased() {
        assert_eq!(folded_title("Fearless!"), "fearless");
        assert_eq!(folded_title("Echoes (Live) — Pt. 2"), "echoeslivept2");
        assert_eq!(folded_title("ÉCHOES"), "échoes");
        assert_eq!(folded_title("   "), "");
    }
}
