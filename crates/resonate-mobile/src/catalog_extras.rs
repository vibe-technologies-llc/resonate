use resonate_codec::MediaInfo;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Error, Result, StoreOp,
    catalog::{Catalog, picture_crc, read},
    features::{
        AnalysisReport, LibraryBreakdown, LibraryShare, LibraryStatistics, ListenedOnDay,
        ListenedTo, ListeningDetail,
    },
};

type StoredLyrics = (Option<String>, Option<String>, String);

const MOST_LISTENED_AT_MOST: u32 = 100;

const DAYS_SHOWN_AT_MOST: u32 = 366;

const SECONDS_A_DAY: i64 = 86_400;

const EVERY_DAY: i64 = -1;

fn days_shown(days: u32) -> i64 {
    if days == 0 {
        EVERY_DAY
    } else {
        i64::from(DAYS_SHOWN_AT_MOST)
    }
}

const THE_ALBUMS_MOST_LISTENED_TO: &str = "SELECT d.album_key, min(d.album), min(coalesce(nullif(d.album_artist, ''), d.artist)), count(*), coalesce(sum(l.heard_ms), 0) AS listened FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND d.album_key != '' AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY d.album_key ORDER BY listened DESC, count(*) DESC, min(d.album) COLLATE NOCASE LIMIT ?2";

const THE_ARTISTS_MOST_LISTENED_TO: &str = "SELECT folded(d.artist), min(d.artist), '', count(*), coalesce(sum(l.heard_ms), 0) AS listened FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND d.artist != '' AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY folded(d.artist) ORDER BY listened DESC, count(*) DESC, min(d.artist) COLLATE NOCASE LIMIT ?2";

const LISTENING_BY_DAY: &str = "SELECT (l.began + ?2) / 86400 AS day, count(*), coalesce(sum(l.heard_ms), 0) FROM listens l WHERE l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY day ORDER BY day DESC LIMIT ?3";

const SHARES_SHOWN_AT_MOST: u32 = 8;

const THE_FORMATS_HELD: &str = "SELECT upper(codec), count(*), coalesce(sum(duration_ms), 0) FROM documents WHERE missing = 0 AND codec != '' GROUP BY upper(codec) ORDER BY count(*) DESC, upper(codec) LIMIT ?1";

const THE_RATES_HELD: &str = "SELECT rate, count(*), coalesce(sum(duration_ms), 0) FROM documents WHERE missing = 0 AND rate > 0 GROUP BY rate ORDER BY count(*) DESC, rate LIMIT ?1";

const THE_GENRES_HELD: &str = "SELECT min(genre), count(*), coalesce(sum(duration_ms), 0) FROM documents WHERE missing = 0 AND genre != '' GROUP BY folded(genre) ORDER BY count(*) DESC, folded(genre) LIMIT ?1";

const THE_DECADES_HELD: &str = "SELECT (cast(substr(date, 1, 4) AS INTEGER) / 10) * 10, count(*), coalesce(sum(duration_ms), 0) FROM documents WHERE missing = 0 AND substr(date, 1, 4) GLOB '[0-9][0-9][0-9][0-9]' AND cast(substr(date, 1, 4) AS INTEGER) >= 1000 GROUP BY 1 ORDER BY count(*) DESC, 1 LIMIT ?1";

const THE_SONGS_HEARD: &str = "SELECT count(DISTINCT l.track), min(l.began) FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400)";

fn kilohertz(hertz: i64) -> String {
    let kilohertz = hertz as f64 / 1000.0;
    if kilohertz.fract() == 0.0 {
        format!("{kilohertz:.0} kHz")
    } else {
        format!("{kilohertz:.1} kHz")
    }
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

impl Catalog {
    pub(crate) fn tagged(&self, id: u64, info: &MediaInfo) -> Result<()> {
        let tags = &info.tags;
        self.0.lock().execute("UPDATE documents SET album_artist = coalesce(nullif(?2, ''), album_artist), genre = coalesce(nullif(?3, ''), genre), date = coalesce(nullif(?4, ''), date), track_number = ?5, disc_number = ?6, bits = ?7, channels = ?8, recording = coalesce(?9, recording), release = coalesce(?10, release), artist_mbid = coalesce(?11, artist_mbid), isrc = coalesce(?12, isrc) WHERE id = ?1", params![id as i64, tags.album_artist, tags.genre, tags.date, tags.track_number, tags.disc_number, info.bits_per_coded_sample, info.spec.channels.count().get(), tags.musicbrainz_track_id, tags.musicbrainz_album_id, tags.musicbrainz_artist_id, tags.isrc]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn artwork(&self, id: u64) -> Result<Option<Vec<u8>>> {
        self.0
            .lock()
            .query_row(
                "SELECT p.bytes FROM artwork a JOIN pictures p ON p.id = a.picture WHERE a.track = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn keep_artwork(
        &self,
        id: u64,
        bytes: &[u8],
        origin: &str,
        replace: bool,
    ) -> Result<()> {
        self.keep_artwork_of(&[id], bytes, origin, replace)
    }

    pub(crate) fn keep_artwork_of(
        &self,
        ids: &[u64],
        bytes: &[u8],
        origin: &str,
        replace: bool,
    ) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection
            .transaction()
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        let write_error = |source| Error::store(StoreOp::Write, source);
        let picture = kept_picture(&transaction, bytes).map_err(write_error)?;
        let mut replaced = Vec::new();
        for id in ids {
            let before: Option<i64> = transaction
                .query_row(
                    "SELECT picture FROM artwork WHERE track = ?1",
                    [*id as i64],
                    |row| row.get(0),
                )
                .optional()
                .map_err(write_error)?;
            let changed = transaction.execute("INSERT INTO artwork(track, picture, origin) VALUES (?1, ?2, ?3) ON CONFLICT(track) DO UPDATE SET picture = excluded.picture, origin = excluded.origin WHERE (?4 OR artwork.origin = excluded.origin) AND artwork.picture != excluded.picture", params![*id as i64, picture, origin, replace]).map_err(write_error)?;
            if changed > 0 {
                transaction
                    .execute(
                        "UPDATE documents SET art_revision = art_revision + 1 WHERE id = ?1",
                        [*id as i64],
                    )
                    .map_err(write_error)?;
                replaced.extend(before);
            }
        }
        replaced.push(picture);
        for picture in replaced {
            unreferenced_picture_dropped(&transaction, picture).map_err(write_error)?;
        }
        transaction.commit().map_err(write_error)
    }

    pub(crate) fn picture(&self, picture: u64) -> Result<Option<Vec<u8>>> {
        self.0
            .lock()
            .query_row(
                "SELECT bytes FROM pictures WHERE id = ?1",
                [picture as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn lyric_text(&self, id: u64) -> Result<Option<StoredLyrics>> {
        self.0
            .lock()
            .query_row(
                "SELECT text, document, origin FROM lyrics WHERE track = ?1",
                [id as i64],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Read, source))
    }

    pub(crate) fn keep_lyrics(
        &self,
        id: u64,
        text: Option<&str>,
        document: Option<&str>,
        origin: &str,
    ) -> Result<()> {
        self.0.lock().execute("INSERT INTO lyrics(track, text, document, origin) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(track) DO UPDATE SET text = excluded.text, document = excluded.document, origin = excluded.origin, asked = unixepoch()", params![id as i64, text, document, origin]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn learned(
        &self,
        id: u64,
        recording: Option<&str>,
        release: Option<&str>,
        artist: Option<&str>,
        album_artist: &str,
    ) -> Result<()> {
        self.0.lock().execute("UPDATE documents SET recording = coalesce(recording, ?2), release = coalesce(release, ?3), artist_mbid = coalesce(artist_mbid, ?4), album_artist = CASE WHEN album_artist = '' AND folded(?5) = folded(artist) THEN ?5 ELSE album_artist END WHERE id = ?1", params![id as i64, recording, release, artist, album_artist]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn statistics(&self, days: u32) -> Result<LibraryStatistics> {
        let connection = self.0.lock();
        let (songs, albums, artists, duration_ms) = connection.query_row("SELECT count(*), count(DISTINCT album || char(0) || album_artist), count(DISTINCT artist), coalesce(sum(duration_ms), 0) FROM documents WHERE missing = 0", [], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64, row.get::<_, i64>(2)? as u64, row.get::<_, i64>(3)? as u64))).map_err(|source| Error::store(StoreOp::Read, source))?;
        let (plays, listened_ms) = connection.query_row("SELECT count(*), coalesce(sum(heard_ms), 0) FROM listens WHERE counted = 1 AND (?1 = 0 OR began >= unixepoch() - ?1 * 86400)", [days], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))).map_err(|source| Error::store(StoreOp::Read, source))?;
        let mut query = connection.prepare("SELECT d.* FROM documents d JOIN listens l ON l.track = d.id WHERE d.missing = 0 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) AND l.counted = 1 GROUP BY d.id ORDER BY sum(l.heard_ms) DESC, count(*) DESC, d.title LIMIT 25").map_err(|source| Error::store(StoreOp::Read, source))?;
        let top_tracks = query
            .query_map([days], read)
            .map_err(|source| Error::store(StoreOp::Read, source))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        Ok(LibraryStatistics {
            songs,
            albums,
            artists,
            duration_ms,
            plays,
            listened_ms,
            top_tracks,
        })
    }

    pub(crate) fn listening_detail(
        &self,
        days: u32,
        limit: u32,
        utc_offset_seconds: i32,
    ) -> Result<ListeningDetail> {
        let connection = self.0.lock();
        let limit = limit.clamp(1, MOST_LISTENED_AT_MOST);
        let ranked = |sql: &str| -> Result<Vec<ListenedTo>> {
            let mut query = connection.prepare(sql).map_err(read_error)?;
            query
                .query_map(params![days, limit], |row| {
                    Ok(ListenedTo {
                        key: row.get(0)?,
                        name: row.get(1)?,
                        by: row.get(2)?,
                        plays: row.get::<_, i64>(3)?.max(0) as u64,
                        listened_ms: row.get::<_, i64>(4)?.max(0) as u64,
                    })
                })
                .map_err(read_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_error)
        };
        let albums = ranked(THE_ALBUMS_MOST_LISTENED_TO)?;
        let artists = ranked(THE_ARTISTS_MOST_LISTENED_TO)?;
        let mut query = connection.prepare(LISTENING_BY_DAY).map_err(read_error)?;
        let mut by_day = query
            .query_map(
                params![days, i64::from(utc_offset_seconds), days_shown(days)],
                |row| {
                    let day = row.get::<_, i64>(0)?;
                    Ok(ListenedOnDay {
                        starts: (day * SECONDS_A_DAY - i64::from(utc_offset_seconds)).max(0) as u64,
                        plays: row.get::<_, i64>(1)?.max(0) as u64,
                        listened_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    })
                },
            )
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        by_day.reverse();
        let (songs_heard, first_heard) = connection
            .query_row(THE_SONGS_HEARD, [days], |row| {
                Ok((
                    row.get::<_, i64>(0)?.max(0) as u64,
                    row.get::<_, Option<i64>>(1)?
                        .map(|began| began.max(0) as u64),
                ))
            })
            .map_err(read_error)?;
        Ok(ListeningDetail {
            albums,
            artists,
            days: by_day,
            songs_heard,
            first_heard,
        })
    }

    pub(crate) fn library_breakdown(&self) -> Result<LibraryBreakdown> {
        let connection = self.0.lock();
        let shares = |sql: &str, named: fn(&rusqlite::Row<'_>) -> rusqlite::Result<String>| {
            let mut query = connection.prepare(sql).map_err(read_error)?;
            query
                .query_map([SHARES_SHOWN_AT_MOST], |row| {
                    Ok(LibraryShare {
                        name: named(row)?,
                        songs: row.get::<_, i64>(1)?.max(0) as u64,
                        duration_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    })
                })
                .map_err(read_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(read_error)
        };
        Ok(LibraryBreakdown {
            formats: shares(THE_FORMATS_HELD, |row| row.get(0))?,
            rates: shares(THE_RATES_HELD, |row| Ok(kilohertz(row.get(0)?)))?,
            genres: shares(THE_GENRES_HELD, |row| row.get(0))?,
            decades: shares(THE_DECADES_HELD, |row| {
                Ok(format!("{}s", row.get::<_, i64>(0)?))
            })?,
        })
    }

    pub(crate) fn keep_study(&self, id: u64, report: &AnalysisReport) -> Result<()> {
        self.0.lock().execute("INSERT INTO studies(track, verdict, peak, rms, true_peak, loudness, dynamic_range, clipped, bits) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) ON CONFLICT(track) DO UPDATE SET verdict = excluded.verdict, peak = excluded.peak, rms = excluded.rms, true_peak = excluded.true_peak, loudness = excluded.loudness, dynamic_range = excluded.dynamic_range, clipped = excluded.clipped, bits = excluded.bits", params![id as i64, report.verdict, report.peak, report.rms, report.true_peak, report.loudness_lufs, report.dynamic_range, report.clipped.min(i64::MAX as u64) as i64, report.bits_in_use]).map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }
}

pub(crate) fn kept_picture(transaction: &Transaction<'_>, bytes: &[u8]) -> rusqlite::Result<i64> {
    let crc = picture_crc(bytes);
    let held = transaction
        .query_row(
            "SELECT id FROM pictures WHERE crc = ?1 AND bytes = ?2",
            params![crc, bytes],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(held) = held {
        return Ok(held);
    }
    transaction.execute(
        "INSERT INTO pictures(crc, bytes) VALUES (?1, ?2)",
        params![crc, bytes],
    )?;
    Ok(transaction.last_insert_rowid())
}

pub(crate) fn unreferenced_picture_dropped(
    transaction: &Transaction<'_>,
    picture: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "DELETE FROM pictures WHERE id = ?1
           AND NOT EXISTS (SELECT 1 FROM artwork WHERE picture = ?1)
           AND NOT EXISTS (SELECT 1 FROM album_art WHERE picture = ?1)",
        [picture],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::Track;

    #[test]
    fn listening_detail_ranks_albums_and_artists_and_buckets_days_in_the_listeners_offset() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = |uri: &str, title: &str, artist: &str, album: &str| {
            catalog
                .upsert(Track {
                    uri: format!("content://test/{uri}.flac"),
                    name: format!("{uri}.flac"),
                    title: title.into(),
                    artist: artist.into(),
                    album: album.into(),
                    duration_ms: 200_000,
                    ..Track::default()
                })
                .expect("a song")
                .id as i64
        };
        let joga = song("joga", "Joga", "Björk", "Homogenic");
        let bachelorette = song("bachelorette", "Bachelorette", "BJORK", "Homogenic");
        let sometimes = song("sometimes", "Sometimes", "Air", "Moon Safari");
        let hidden = song("hidden", "Hidden", "Air", "Moon Safari");
        catalog
            .0
            .lock()
            .execute("UPDATE documents SET missing = 1 WHERE id = ?1", [hidden])
            .expect("a missing song");
        let now: i64 = catalog
            .0
            .lock()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .expect("the time");
        let day = 86_400;
        for (track, began, heard, counted) in [
            (joga, now - 60, 200_000, 1),
            (bachelorette, now - 120, 100_000, 1),
            (sometimes, now - 3 * day, 50_000, 1),
            (sometimes, now - 3 * day - 60, 40_000, 0),
            (hidden, now - 30, 900_000, 1),
            (sometimes, now - 40 * day, 50_000, 1),
        ] {
            catalog
                .0
                .lock()
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, ?3, ?4)",
                    params![track, began, heard, counted],
                )
                .expect("a listen");
        }

        let week = catalog.listening_detail(7, 10, 0).expect("a week");
        let everything = catalog.listening_detail(0, 10, 0).expect("all time");

        assert_eq!(week.albums.len(), 2);
        assert_eq!(week.albums[0].name, "Homogenic");
        assert_eq!(week.albums[0].by.to_lowercase(), "bjork");
        assert_eq!(week.albums[0].plays, 2);
        assert_eq!(week.albums[0].listened_ms, 300_000);
        assert_eq!(week.albums[1].name, "Moon Safari");
        assert_eq!(week.artists[0].key, "bjork");
        assert_eq!(week.artists[0].plays, 2);
        assert_eq!(week.artists[1].name, "Air");
        assert_eq!(week.artists[1].plays, 1);
        assert_eq!(week.days.len(), 2);
        assert_eq!(week.songs_heard, 3);
        assert_eq!(everything.songs_heard, 3);
        assert!(everything.first_heard < week.first_heard);
        assert!(week.days[0].starts < week.days[1].starts);
        assert_eq!(week.days[1].plays, 3);
        assert_eq!(everything.albums[1].plays, 2);
        assert_eq!(everything.days.len(), 3);
        assert_eq!(
            catalog
                .listening_detail(0, 1, 0)
                .expect("one ranked")
                .albums
                .len(),
            1
        );
    }

    #[test]
    fn statistics_count_the_listens_that_counted_alone() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                name: "song.flac".into(),
                ..Track::default()
            })
            .expect("a song")
            .id as i64;

        for (counted, heard_ms) in [(1, 30_000), (1, 20_000), (0, 99_000)] {
            catalog
                .0
                .lock()
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, unixepoch(), ?2, ?3)",
                    params![song, heard_ms, counted],
                )
                .expect("a listen");
        }

        let statistics = catalog.statistics(0).expect("the statistics");
        let detail = catalog.listening_detail(0, 10, 0).expect("the detail");

        assert_eq!(statistics.plays, 2);
        assert_eq!(statistics.listened_ms, 50_000);
        assert_eq!(
            statistics.listened_ms,
            detail.days.iter().map(|day| day.listened_ms).sum::<u64>()
        );
    }

    #[test]
    fn all_time_lists_every_day_heard_and_a_year_lists_at_most_a_year_of_them() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                name: "song.flac".into(),
                ..Track::default()
            })
            .expect("a song")
            .id as i64;
        let now: i64 = catalog
            .0
            .lock()
            .query_row("SELECT unixepoch()", [], |row| row.get(0))
            .expect("the time");
        for back in 0..500 {
            catalog
                .0
                .lock()
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, 1000, 1)",
                    params![song, now - back * SECONDS_A_DAY],
                )
                .expect("a listen");
        }

        let everything = catalog.listening_detail(0, 10, 0).expect("all time");
        let year = catalog.listening_detail(365, 10, 0).expect("a year");

        assert!(everything.days.len() >= 500);
        assert!(year.days.len() <= usize::try_from(DAYS_SHOWN_AT_MOST).expect("a small count"));
        assert!(year.days.len() >= 365);
    }

    #[test]
    fn the_breakdown_counts_formats_rates_genres_and_decades_of_the_songs_still_held() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let song = |uri: &str, codec: &str, rate: u32| {
            catalog
                .upsert(Track {
                    uri: format!("content://test/{uri}.flac"),
                    name: format!("{uri}.flac"),
                    title: uri.into(),
                    codec: codec.into(),
                    rate,
                    duration_ms: 100_000,
                    ..Track::default()
                })
                .expect("a song")
                .id as i64
        };
        let tagged = |id: i64, genre: &str, date: &str| {
            catalog
                .0
                .lock()
                .execute(
                    "UPDATE documents SET genre = ?2, date = ?3 WHERE id = ?1",
                    params![id, genre, date],
                )
                .expect("tags");
            id
        };
        tagged(song("a", "flac", 44_100), "Trip Hop", "1994-05-01");
        tagged(song("b", "FLAC", 96_000), "trip hop", "1998");
        tagged(song("c", "MP3", 44_100), "", "2003");
        tagged(song("d", "", 0), "Jazz", "unknown");
        let gone = tagged(song("e", "FLAC", 192_000), "Jazz", "1959");
        catalog
            .0
            .lock()
            .execute("UPDATE documents SET missing = 1 WHERE id = ?1", [gone])
            .expect("a missing song");

        let held = catalog.library_breakdown().expect("the breakdown");
        let named = |shares: &[LibraryShare]| {
            shares
                .iter()
                .map(|share| (share.name.clone(), share.songs))
                .collect::<Vec<_>>()
        };

        assert_eq!(
            named(&held.formats),
            vec![("FLAC".into(), 2), ("MP3".into(), 1)]
        );
        assert_eq!(
            named(&held.rates),
            vec![("44.1 kHz".into(), 2), ("96 kHz".into(), 1)]
        );
        assert_eq!(
            named(&held.genres),
            vec![("Trip Hop".into(), 2), ("Jazz".into(), 1)]
        );
        assert_eq!(
            named(&held.decades),
            vec![("1990s".into(), 2), ("2000s".into(), 1)]
        );
        assert_eq!(held.formats[0].duration_ms, 200_000);
    }

    #[test]
    fn rescanning_an_untagged_delivery_keeps_its_selected_names() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let track = Track {
            uri: "content://test/delivery.wav".into(),
            name: "delivery.wav".into(),
            title: "Selected title".into(),
            artist: "Selected artist".into(),
            album: "Selected album".into(),
            ..Track::default()
        };
        let saved = catalog
            .upsert(track.clone())
            .expect("selected delivery metadata");
        let rescanned = catalog
            .upsert(Track {
                title: track.name.clone(),
                artist: String::new(),
                album: String::new(),
                ..track
            })
            .expect("an untagged rescan");
        assert_eq!(saved.id, rescanned.id);
        assert_eq!(saved.title, rescanned.title);
        assert_eq!(saved.artist, rescanned.artist);
        assert_eq!(saved.album, rescanned.album);
    }

    fn pictures_held(catalog: &Catalog) -> i64 {
        catalog
            .0
            .lock()
            .query_row("SELECT count(*) FROM pictures", [], |row| row.get(0))
            .expect("a picture count")
    }

    fn songs(catalog: &Catalog, count: usize) -> Vec<u64> {
        (0..count)
            .map(|at| {
                catalog
                    .upsert(Track {
                        uri: format!("content://test/{at}.flac"),
                        album: "One Album".into(),
                        ..Track::default()
                    })
                    .expect("a track")
                    .id
            })
            .collect()
    }

    #[test]
    fn a_cover_shared_by_every_song_of_a_folder_is_kept_once() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let ids = songs(&catalog, 12);

        catalog
            .keep_artwork_of(&ids, &[1, 2, 3], "folder", false)
            .expect("a folder cover");
        catalog
            .keep_artwork_of(&ids, &[1, 2, 3], "folder", false)
            .expect("the same cover again");

        assert_eq!(pictures_held(&catalog), 1);
        assert_eq!(
            catalog.picture_of(ids[0]).expect("a picture"),
            catalog.picture_of(ids[11]).expect("a picture")
        );
        assert_eq!(catalog.track(ids[5]).expect("a track").art_revision, 1);
    }

    #[test]
    fn a_picture_nothing_shows_any_more_is_dropped() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let ids = songs(&catalog, 2);
        catalog
            .keep_artwork_of(&ids, &[1, 1, 1], "folder", false)
            .expect("a cover");

        catalog
            .keep_artwork(ids[0], &[2, 2, 2], "folder", false)
            .expect("one song's new cover");

        assert_eq!(pictures_held(&catalog), 2);

        catalog
            .keep_artwork(ids[1], &[2, 2, 2], "folder", false)
            .expect("the other song's new cover");

        assert_eq!(pictures_held(&catalog), 1);

        catalog
            .keep_artwork(ids[0], &[9], "embedded", false)
            .expect("a refused cover");

        assert_eq!(pictures_held(&catalog), 1);
    }

    #[test]
    fn companion_artwork_refreshes_without_replacing_an_embedded_cover() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let track = catalog
            .upsert(Track {
                uri: "content://test/art.flac".into(),
                ..Track::default()
            })
            .expect("a track");
        catalog
            .keep_artwork(track.id, &[1], "folder", false)
            .expect("a folder cover");
        catalog
            .keep_artwork(track.id, &[2], "folder", false)
            .expect("a changed folder cover");
        assert_eq!(
            catalog.artwork(track.id).expect("the changed picture"),
            Some(vec![2])
        );
        catalog
            .keep_artwork(track.id, &[3], "embedded", true)
            .expect("an embedded cover");
        catalog
            .keep_artwork(track.id, &[4], "folder", false)
            .expect("a fallback");
        catalog
            .keep_artwork(track.id, &[5], "coverartarchive", false)
            .expect("a remote fallback");
        assert_eq!(
            catalog.artwork(track.id).expect("the embedded picture"),
            Some(vec![3])
        );
        assert_eq!(
            catalog
                .track(track.id)
                .expect("the updated track")
                .art_revision,
            3
        );
    }
}
