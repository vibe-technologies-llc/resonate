use std::{
    collections::BTreeMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_analysis::{
    BANDS, BandGains, BandLevels, Cut, JUDGED_UNDER, LossyGuess, Loudest, Study,
};
use resonate_codec::{MediaInfo, Scanned};
use resonate_core::Calendar;
use resonate_library::Agreement;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    Error, Result, StoreOp,
    catalog::{Catalog, picture_crc, read},
    features::{
        AnalysisReport, LibraryBreakdown, LibraryShare, LibraryStatistics, ListenedOnDay,
        ListenedTo, ListeningDetail,
    },
    stamp::{DocumentStamp, INDEXING_REVISION},
};

struct Figures<'a> {
    verdict: &'a str,
    peak: f32,
    rms: f32,
    true_peak: f32,
    loudness: Option<f32>,
    dynamic_range: Option<u32>,
    clipped: u64,
    bits: Option<u32>,
    band_levels: &'a [u8],
    measured: Option<Measured>,
}

struct Measured {
    cutoff_hz: Option<u32>,
    cutoff_drop: Option<f32>,
    lossy_guess: Option<&'static str>,
    upsampled_from: Option<u32>,
    declared_bits: Option<u8>,
    loudness_range: Option<f32>,
    mono_as_stereo: bool,
}

pub(crate) struct LandedStudy<'a> {
    pub(crate) verdict: &'a str,
    pub(crate) peak: f32,
    pub(crate) rms: f32,
    pub(crate) true_peak: f32,
    pub(crate) loudness: Option<f32>,
    pub(crate) dynamic_range: Option<u8>,
    pub(crate) clipped: u64,
    pub(crate) bits: Option<u8>,
    pub(crate) band_levels: &'a [u8],
    pub(crate) cutoff_hz: Option<u32>,
    pub(crate) cutoff_drop: Option<f32>,
    pub(crate) lossy_guess: Option<&'static str>,
    pub(crate) upsampled_from: Option<u32>,
    pub(crate) declared_bits: Option<u8>,
    pub(crate) loudness_range: Option<f32>,
    pub(crate) mono_as_stereo: bool,
}

const STUDY_UPSERT: &str = "INSERT INTO studies (track, verdict, peak, rms, true_peak, loudness, dynamic_range, clipped, bits, band_levels, studied, studied_under, cutoff_hz, cutoff_drop, lossy_guess, upsampled_from, declared_bits, loudness_range, mono_as_stereo)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
     ON CONFLICT(track) DO UPDATE SET verdict = excluded.verdict, peak = excluded.peak, rms = excluded.rms, true_peak = excluded.true_peak, loudness = excluded.loudness, dynamic_range = excluded.dynamic_range, clipped = excluded.clipped, bits = excluded.bits, band_levels = excluded.band_levels, studied = excluded.studied, studied_under = excluded.studied_under, cutoff_hz = excluded.cutoff_hz, cutoff_drop = excluded.cutoff_drop, lossy_guess = excluded.lossy_guess, upsampled_from = excluded.upsampled_from, declared_bits = excluded.declared_bits, loudness_range = excluded.loudness_range, mono_as_stereo = excluded.mono_as_stereo";

const LANDED_STUDY: &str = "INSERT INTO studies (track, verdict, peak, rms, true_peak, loudness, dynamic_range, clipped, bits, band_levels, studied, studied_under, cutoff_hz, cutoff_drop, lossy_guess, upsampled_from, declared_bits, loudness_range, mono_as_stereo)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
     ON CONFLICT(track) DO UPDATE SET verdict = excluded.verdict, peak = excluded.peak, rms = excluded.rms, true_peak = excluded.true_peak, loudness = excluded.loudness, dynamic_range = excluded.dynamic_range, clipped = excluded.clipped, bits = excluded.bits, band_levels = excluded.band_levels, studied = excluded.studied, studied_under = excluded.studied_under, cutoff_hz = excluded.cutoff_hz, cutoff_drop = excluded.cutoff_drop, lossy_guess = excluded.lossy_guess, upsampled_from = excluded.upsampled_from, declared_bits = excluded.declared_bits, loudness_range = excluded.loudness_range, mono_as_stereo = excluded.mono_as_stereo, agreement = NULL
     WHERE studies.studied_under IS NOT NULL AND studies.studied_under <> excluded.studied_under";

type StoredLyrics = (Option<String>, Option<String>, String);

const MOST_LISTENED_AT_MOST: u32 = 100;

const DAYS_SHOWN_AT_MOST: usize = 366;

const SECONDS_A_QUARTER_HOUR: i64 = 900;

fn days_shown(days: u32) -> usize {
    if days == 0 {
        usize::MAX
    } else {
        DAYS_SHOWN_AT_MOST
    }
}

pub(crate) fn the_listeners_calendar(time_zone: &str, utc_offset_seconds: i32) -> Calendar {
    Calendar::named(time_zone)
        .or_else(|| Calendar::fixed(utc_offset_seconds))
        .unwrap_or_else(Calendar::utc)
}

#[derive(Clone, Copy, Default)]
struct Heard {
    plays: u64,
    listened_ms: u64,
}

fn moment(seconds: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds.max(0).unsigned_abs())
}

fn seconds_of(moment: SystemTime) -> u64 {
    moment
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

fn by_day(quarters: &[(i64, Heard)], calendar: &Calendar, shown: usize) -> Vec<ListenedOnDay> {
    let mut days: BTreeMap<i64, Heard> = BTreeMap::new();
    for (quarter, heard) in quarters {
        let day = calendar.day_of(moment(quarter.saturating_mul(SECONDS_A_QUARTER_HOUR)));
        let held = days.entry(day).or_default();
        held.plays = held.plays.saturating_add(heard.plays);
        held.listened_ms = held.listened_ms.saturating_add(heard.listened_ms);
    }

    let skipped = days.len().saturating_sub(shown);
    days.into_iter()
        .skip(skipped)
        .map(|(day, heard)| ListenedOnDay {
            starts: seconds_of(calendar.midnight_of(day)),
            plays: heard.plays,
            listened_ms: heard.listened_ms,
        })
        .collect()
}

const THE_ALBUMS_MOST_LISTENED_TO: &str = "SELECT d.album_key, min(d.album), min(coalesce(nullif(d.album_artist, ''), d.artist)), count(*), coalesce(sum(l.heard_ms), 0) AS listened FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND d.album_key != '' AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY d.album_key ORDER BY listened DESC, count(*) DESC, min(d.album) COLLATE NOCASE LIMIT ?2";

const THE_ARTISTS_MOST_LISTENED_TO: &str = "SELECT folded(d.artist), min(d.artist), '', count(*), coalesce(sum(l.heard_ms), 0) AS listened FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND d.artist != '' AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY folded(d.artist) ORDER BY listened DESC, count(*) DESC, min(d.artist) COLLATE NOCASE LIMIT ?2";

const LISTENING_BY_QUARTER_HOUR: &str = "SELECT l.began / ?2 AS quarter, count(*), coalesce(sum(l.heard_ms), 0) FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400) GROUP BY quarter";

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
    pub(crate) fn tagged(
        &self,
        id: u64,
        info: &MediaInfo,
        stamp: Option<DocumentStamp>,
    ) -> Result<()> {
        let tags = &info.tags;
        self.0.lock().execute("UPDATE documents SET album_artist = coalesce(nullif(?2, ''), album_artist), genre = coalesce(nullif(?3, ''), genre), date = coalesce(nullif(?4, ''), date), track_number = ?5, disc_number = ?6, bits = ?7, channels = ?8, recording = coalesce(?9, recording), release = coalesce(?10, release), artist_mbid = coalesce(?11, artist_mbid), isrc = coalesce(?12, isrc), size = ?13, modified = ?14, indexed_by = ?15 WHERE id = ?1", params![id as i64, tags.album_artist, tags.genre, tags.date, tags.track_number, tags.disc_number, info.bits_per_coded_sample, info.spec.channels.count().get(), tags.musicbrainz_track_id, tags.musicbrainz_album_id, tags.musicbrainz_artist_id, tags.isrc, stamp.map(DocumentStamp::size_column), stamp.map(DocumentStamp::modified_column), stamp.map(|_| INDEXING_REVISION)]).map_err(|source| Error::store(StoreOp::Write, source))?;
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
        let (plays, listened_ms) = connection.query_row("SELECT count(*), coalesce(sum(l.heard_ms), 0) FROM listens l JOIN documents d ON d.id = l.track WHERE d.missing = 0 AND l.counted = 1 AND (?1 = 0 OR l.began >= unixepoch() - ?1 * 86400)", [days], |row| Ok((row.get::<_, i64>(0)? as u64, row.get::<_, i64>(1)? as u64))).map_err(|source| Error::store(StoreOp::Read, source))?;
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
        calendar: &Calendar,
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
        let mut query = connection
            .prepare(LISTENING_BY_QUARTER_HOUR)
            .map_err(read_error)?;
        let quarters = query
            .query_map(params![days, SECONDS_A_QUARTER_HOUR], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Heard {
                        plays: row.get::<_, i64>(1)?.max(0) as u64,
                        listened_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    },
                ))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        let (songs_heard, first_heard) = connection
            .query_row(THE_SONGS_HEARD, [days], |row| {
                Ok((
                    row.get::<_, i64>(0)?.max(0) as u64,
                    row.get::<_, Option<i64>>(1)?
                        .map(|began| began.max(0) as u64),
                ))
            })
            .map_err(read_error)?;
        drop(query);
        drop(connection);

        Ok(ListeningDetail {
            albums,
            artists,
            days: by_day(&quarters, calendar, days_shown(days)),
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

    pub(crate) fn keep_study(
        &self,
        id: u64,
        report: &AnalysisReport,
        levels: &BandLevels,
    ) -> Result<()> {
        self.keep_figures(
            id,
            Figures {
                verdict: &report.verdict,
                peak: report.peak,
                rms: report.rms,
                true_peak: report.true_peak,
                loudness: report.loudness_lufs,
                dynamic_range: report.dynamic_range,
                clipped: report.clipped,
                bits: report.bits_in_use,
                band_levels: levels.as_bytes(),
                measured: None,
            },
        )
    }

    pub(crate) fn keep_studied(&self, id: u64, study: &Study) -> Result<()> {
        self.keep_figures(
            id,
            Figures {
                verdict: study.judgement.verdict.as_str(),
                peak: study.levels.peak,
                rms: study.levels.rms,
                true_peak: study.loudness.true_peak,
                loudness: study.loudness.integrated,
                dynamic_range: study.loudness.dynamic_range.map(u32::from),
                clipped: study.levels.clipped,
                bits: study.levels.bits_in_use.map(u32::from),
                band_levels: BandLevels::of(&study.spectrum).as_bytes(),
                measured: Some(Measured {
                    cutoff_hz: study.judgement.cutoff.map(|cutoff| cutoff.hz),
                    cutoff_drop: study.judgement.cutoff.map(|cutoff| cutoff.drop_db),
                    lossy_guess: study.judgement.lossy_guess().map(LossyGuess::as_str),
                    upsampled_from: study.judgement.upsampled_from(),
                    declared_bits: study.examined.declared_bits,
                    loudness_range: study.loudness.range,
                    mono_as_stereo: study.levels.stereo.is_some_and(|stereo| stereo.identical),
                }),
            },
        )
    }

    fn keep_figures(&self, id: u64, figures: Figures<'_>) -> Result<()> {
        let measured = figures.measured.as_ref();
        let connection = self.0.lock();
        connection
            .execute(
                STUDY_UPSERT,
                params![
                    id as i64,
                    figures.verdict,
                    figures.peak,
                    figures.rms,
                    figures.true_peak,
                    figures.loudness,
                    figures.dynamic_range,
                    figures.clipped.min(i64::MAX as u64) as i64,
                    figures.bits,
                    figures.band_levels,
                    nanos_now(),
                    measured.map(|_| i64::from(JUDGED_UNDER)),
                    measured.and_then(|measured| measured.cutoff_hz),
                    measured.and_then(|measured| measured.cutoff_drop),
                    measured.and_then(|measured| measured.lossy_guess),
                    measured.and_then(|measured| measured.upsampled_from),
                    measured.and_then(|measured| measured.declared_bits),
                    measured.and_then(|measured| measured.loudness_range),
                    measured.map(|measured| measured.mono_as_stereo),
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        connection
            .execute("DELETE FROM unstudied WHERE track = ?1", [id as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn land_study(&self, id: u64, study: &LandedStudy<'_>) -> Result<bool> {
        let connection = self.0.lock();
        let landed = connection
            .execute(
                LANDED_STUDY,
                params![
                    id as i64,
                    study.verdict,
                    study.peak,
                    study.rms,
                    study.true_peak,
                    study.loudness,
                    study.dynamic_range,
                    study.clipped.min(i64::MAX as u64) as i64,
                    study.bits,
                    study.band_levels,
                    nanos_now(),
                    i64::from(JUDGED_UNDER),
                    study.cutoff_hz,
                    study.cutoff_drop,
                    study.lossy_guess,
                    study.upsampled_from,
                    study.declared_bits,
                    study.loudness_range,
                    study.mono_as_stereo,
                ],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        connection
            .execute("DELETE FROM unstudied WHERE track = ?1", [id as i64])
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(landed > 0)
    }

    pub(crate) fn keep_identity(&self, id: u64, scanned: &Scanned) -> Result<()> {
        let packets = scanned.packets.map(|digest| digest.0.cast_signed());
        let frames = scanned
            .info
            .duration
            .map(|frames| frames.get().cast_signed());
        self.0
            .lock()
            .execute(
                "UPDATE documents SET packets = ?2, frames = ?3
                  WHERE id = ?1 AND (packets IS NOT ?2 OR frames IS NOT ?3)",
                params![id as i64, packets, frames],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn studies_without_bands(&self) -> Result<Vec<(u64, String)>> {
        let connection = self.0.lock();
        let read_error = |source| Error::store(StoreOp::Read, source);
        let mut statement = connection
            .prepare_cached(
                "SELECT d.id, d.uri FROM documents d JOIN studies s ON s.track = d.id
                 WHERE d.missing = 0 AND coalesce(length(s.band_levels), 0) != ?1
                 ORDER BY d.id",
            )
            .map_err(read_error)?;
        statement
            .query_map([BANDS as i64], |row| {
                Ok((row.get::<_, i64>(0)? as u64, row.get::<_, String>(1)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub(crate) fn keep_bands(&self, id: u64, levels: &BandLevels) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE studies SET band_levels = ?2
                 WHERE track = ?1 AND coalesce(length(band_levels), 0) != ?3",
                params![id as i64, levels.as_bytes(), BANDS as i64],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    pub(crate) fn note_unstudied(&self, id: u64) -> Result<()> {
        self.0
            .lock()
            .execute(
                "INSERT OR REPLACE INTO unstudied (track, under) VALUES (?1, ?2)",
                params![id as i64, resonate_analysis::JUDGED_UNDER],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }
}

impl Catalog {
    pub(crate) fn loudest_kept(&self, gains: &BandGains) -> Result<Loudest> {
        let connection = self.0.lock();
        let read_error = |source| Error::store(StoreOp::Read, source);
        let mut statement = connection
            .prepare_cached(
                "SELECT s.true_peak, s.band_levels FROM studies s JOIN documents d ON d.id = s.track
                 WHERE d.missing = 0 AND length(s.band_levels) = ?1",
            )
            .map_err(read_error)?;
        let rows = statement
            .query_map([BANDS as i64], |row| {
                Ok((row.get::<_, f64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(read_error)?;

        let mut loudest = Loudest::default();
        for row in rows {
            let (true_peak, kept) = row.map_err(read_error)?;
            if let Some(levels) = BandLevels::from_bytes(&kept) {
                loudest.weigh(gains, &levels, true_peak as f32);
            }
        }
        Ok(loudest)
    }

    pub(crate) fn unmeasured(&self, cuts: &[Cut]) -> Result<Vec<Cut>> {
        let connection = self.0.lock();
        let read_error = |source| Error::store(StoreOp::Read, source);
        let mut statement = connection
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM studies s JOIN documents d ON d.id = s.track
                                 WHERE d.uri = ?1 AND d.missing = 0 AND length(s.band_levels) = ?2)",
            )
            .map_err(read_error)?;

        let mut owed = Vec::new();
        for cut in cuts {
            let held = cut.span.is_none()
                && statement
                    .query_row(params![cut.location.to_uri(), BANDS as i64], |row| {
                        row.get::<_, bool>(0)
                    })
                    .map_err(read_error)?;
            if !held {
                owed.push(cut.clone());
            }
        }
        Ok(owed)
    }

    pub(crate) fn keep_agreement(&self, id: u64, agreement: Agreement) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE studies SET agreement = ?2 WHERE track = ?1",
                params![id as i64, agreement.as_str()],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
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

fn nanos_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use resonate_core::{CivilDate, SECONDS_PER_DAY};

    use super::*;
    use crate::Track;

    #[test]
    fn listening_detail_ranks_albums_and_artists_and_buckets_days_in_the_listeners_calendar() {
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

        let week = catalog
            .listening_detail(7, 10, &Calendar::utc())
            .expect("a week");
        let everything = catalog
            .listening_detail(0, 10, &Calendar::utc())
            .expect("all time");

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
        assert_eq!(week.days[1].plays, 2);
        assert_eq!(everything.albums[1].plays, 2);
        assert_eq!(everything.days.len(), 3);
        assert_eq!(
            catalog
                .listening_detail(0, 1, &Calendar::utc())
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
        let detail = catalog
            .listening_detail(0, 10, &Calendar::utc())
            .expect("the detail");

        assert_eq!(statistics.plays, 2);
        assert_eq!(statistics.listened_ms, 50_000);
        assert_eq!(
            statistics.listened_ms,
            detail.days.iter().map(|day| day.listened_ms).sum::<u64>()
        );
    }

    #[test]
    fn a_missing_songs_counted_listens_are_left_out_of_the_totals_and_the_days() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let held = catalog
            .upsert(Track {
                uri: "content://test/held.flac".into(),
                name: "held.flac".into(),
                ..Track::default()
            })
            .expect("a song")
            .id as i64;
        let gone = catalog
            .upsert(Track {
                uri: "content://test/gone.flac".into(),
                name: "gone.flac".into(),
                ..Track::default()
            })
            .expect("a song")
            .id as i64;

        for (song, heard_ms) in [(held, 30_000), (gone, 40_000)] {
            catalog
                .0
                .lock()
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, unixepoch(), ?2, 1)",
                    params![song, heard_ms],
                )
                .expect("a listen");
        }
        catalog
            .0
            .lock()
            .execute("UPDATE documents SET missing = 1 WHERE id = ?1", [gone])
            .expect("the song goes missing");

        let statistics = catalog.statistics(0).expect("the statistics");
        let detail = catalog
            .listening_detail(0, 10, &Calendar::utc())
            .expect("the detail");

        assert_eq!(statistics.plays, 1);
        assert_eq!(statistics.listened_ms, 30_000);
        assert_eq!(detail.songs_heard, 1);
        assert_eq!(
            detail.days.iter().map(|day| day.listened_ms).sum::<u64>(),
            30_000
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
                    params![song, now - back * SECONDS_PER_DAY],
                )
                .expect("a listen");
        }

        let everything = catalog
            .listening_detail(0, 10, &Calendar::utc())
            .expect("all time");
        let year = catalog
            .listening_detail(365, 10, &Calendar::utc())
            .expect("a year");

        assert!(everything.days.len() >= 500);
        assert!(year.days.len() <= DAYS_SHOWN_AT_MOST);
        assert!(year.days.len() >= 365);
    }

    const LJUBLJANA: &str = "Europe/Ljubljana";
    const SUMMER_IN_LJUBLJANA: i32 = 7_200;
    const AN_HOUR: i64 = 3_600;
    const THE_FIRST_OF_JANUARY_2024: i64 = 19_723;
    const THE_FIRST_OF_JULY_2024: i64 = 19_905;

    fn listened_at(catalog: &Catalog, moments: &[i64]) {
        let song = catalog
            .upsert(Track {
                uri: "content://test/song.flac".into(),
                name: "song.flac".into(),
                ..Track::default()
            })
            .expect("a song")
            .id as i64;
        for began in moments {
            catalog
                .0
                .lock()
                .execute(
                    "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, 60000, 1)",
                    params![song, began],
                )
                .expect("a listen");
        }
    }

    #[test]
    fn a_winter_first_of_the_month_counts_in_its_month_while_the_phone_keeps_summer_time() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let ljubljana = the_listeners_calendar(LJUBLJANA, SUMMER_IN_LJUBLJANA);
        let new_years_midnight = THE_FIRST_OF_JANUARY_2024 * SECONDS_PER_DAY - AN_HOUR;
        let half_past_midnight = new_years_midnight + AN_HOUR / 2;
        let half_past_eleven_at_night = new_years_midnight + 23 * AN_HOUR + AN_HOUR / 2;
        let the_next_morning = new_years_midnight + 33 * AN_HOUR;
        listened_at(
            &catalog,
            &[
                half_past_midnight,
                half_past_midnight + 60,
                half_past_eleven_at_night,
                the_next_morning,
            ],
        );

        let detail = catalog
            .listening_detail(0, 10, &ljubljana)
            .expect("all time");
        let busiest = detail
            .days
            .iter()
            .max_by_key(|day| day.listened_ms)
            .expect("a day heard");
        let busiest_began = UNIX_EPOCH + Duration::from_secs(busiest.starts);

        assert_eq!(detail.days.len(), 2);
        assert_eq!(busiest.plays, 3);
        assert_eq!(busiest.starts, new_years_midnight as u64);
        assert_eq!(
            ljubljana.date_at(busiest_began),
            CivilDate {
                year: 2024,
                month: 1,
                day: 1
            }
        );
        assert_eq!(detail.days[1].plays, 1);
    }

    #[test]
    fn a_summer_day_begins_at_the_summer_midnight_of_a_named_zone() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let ljubljana = the_listeners_calendar(LJUBLJANA, 0);
        let summer_midnight = THE_FIRST_OF_JULY_2024 * SECONDS_PER_DAY - 2 * AN_HOUR;
        listened_at(&catalog, &[summer_midnight + 60]);

        let detail = catalog
            .listening_detail(0, 10, &ljubljana)
            .expect("all time");

        assert_eq!(detail.days.len(), 1);
        assert_eq!(detail.days[0].starts, summer_midnight as u64);
    }

    #[test]
    fn a_zone_the_database_does_not_name_falls_back_to_the_phones_offset() {
        assert_eq!(
            the_listeners_calendar("Atlantis/Poseidonia", SUMMER_IN_LJUBLJANA),
            Calendar::fixed(SUMMER_IN_LJUBLJANA).expect("a fixed offset is a zone")
        );
        assert_eq!(
            the_listeners_calendar(LJUBLJANA, 0),
            Calendar::named(LJUBLJANA).expect("the zone database names Ljubljana")
        );
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
