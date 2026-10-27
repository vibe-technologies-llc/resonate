use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime},
};

use resonate_analysis::{BANDS, JUDGED_UNDER, LossyGuess, Verdict};
use resonate_core::{
    AlbumId, Answer, Answered, Chromaprint, Figure, Link, Mbid, Recording, SharedCredit,
    SharedHeard, SharedLyrics, SharedLyricsfile, SharedMedium, SharedPrint, SharedRecognition,
    SharedRelease, SharedStudy, SharedTrack, TrackId, WholeFile,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::{
    CoverSource, Credit, Cut, Error, HeardAs, LyricText, Medium, RecordingMatch, Release,
    ReleaseMatch, ReleaseTrack, Result, StoreOp,
    db::Inner,
    enrich::{EXACT_SCORE, Fit, Pressing, agreement, takes_strictly},
    enriched, store, studies, sung,
    synced::{RECORDING_COLUMNS, RECORDING_JOINS, lasts_alike, recording_at, resolved},
};

struct ReleaseHead {
    album: i64,
    at: i64,
    id: Option<String>,
    group: Option<String>,
    title: String,
    date: Option<String>,
    country: Option<String>,
    label: Option<String>,
    catalog_number: Option<String>,
    barcode: Option<String>,
    kind: Option<String>,
    disambiguation: Option<String>,
    has_front_cover: bool,
}

const WHOLE_FILE: &str = "t.span_start = 0 AND t.span_frames IS NULL";

pub(crate) fn after(inner: &Inner, mark: i64, most: usize) -> Result<Vec<Answered>> {
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    let mut answered = inner.read(|connection| {
        let mut answered = lyrics_after(connection, mark, most)?;
        answered.extend(studies_after(connection, mark, most)?);
        answered.extend(releases_after(connection, mark, most)?);
        Ok(answered)
    })?;

    answered.sort_by_key(|answered| answered.at);
    answered.truncate(usize::try_from(most).unwrap_or(usize::MAX));
    Ok(answered)
}

fn lyrics_after(connection: &Connection, mark: i64, most: i64) -> Result<Vec<Answered>> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT k.taken, k.text, k.synced, {RECORDING_COLUMNS}
               FROM lyrics_kept k JOIN tracks t ON t.path = k.path AND t.span_start = k.span_start
               {RECORDING_JOINS}
              WHERE k.text IS NOT NULL AND k.taken >= ?1
              ORDER BY k.taken LIMIT ?2"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![mark, most], |row| {
            let at: i64 = row.get(0)?;
            let lyrics = SharedLyrics {
                text: row.get(1)?,
                synced: row.get(2)?,
            };
            Ok(recording_at(row, 3)?.map(|recording| Answered {
                recording,
                at,
                answer: Answer::Lyrics(lyrics),
            }))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(rows.into_iter().flatten().collect())
}

fn studies_after(connection: &Connection, mark: i64, most: i64) -> Result<Vec<Answered>> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT s.studied, t.packets, t.duration, t.sample_rate, s.verdict, s.cutoff_hz,
                    s.cutoff_drop, s.lossy_guess, s.upsampled_from, s.bits_in_use, s.declared_bits,
                    s.peak, s.rms, s.true_peak, s.loudness, s.loudness_range, s.dynamic_range,
                    s.clipped, s.mono_as_stereo, s.band_levels, {RECORDING_COLUMNS}
               FROM track_studies s JOIN tracks t ON t.id = s.track_id
               {RECORDING_JOINS}
              WHERE s.studied >= ?1 AND s.studied_under = ?3 AND length(s.band_levels) = ?4
                AND t.packets IS NOT NULL AND t.duration IS NOT NULL AND {WHOLE_FILE}
              ORDER BY s.studied LIMIT ?2"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(
            params![mark, most, i64::from(JUDGED_UNDER), BANDS as i64],
            |row| {
                let at: i64 = row.get(0)?;
                let study = study_at(row)?;
                Ok(recording_at(row, 20)?.map(|recording| Answered {
                    recording,
                    at,
                    answer: Answer::Study(study),
                }))
            },
        )
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(rows.into_iter().flatten().collect())
}

fn study_at(row: &Row<'_>) -> rusqlite::Result<SharedStudy> {
    let figure =
        |at: usize| -> rusqlite::Result<Figure> { Ok(Figure::of(row.get::<_, f64>(at)? as f32)) };
    let some_figure = |at: usize| -> rusqlite::Result<Option<Figure>> {
        Ok(row
            .get::<_, Option<f64>>(at)?
            .map(|value| Figure::of(value as f32)))
    };
    let small = |at: usize| -> rusqlite::Result<Option<u8>> {
        Ok(row
            .get::<_, Option<i64>>(at)?
            .map(|value| value.clamp(0, i64::from(u8::MAX)) as u8))
    };
    let hertz = |at: usize| -> rusqlite::Result<Option<u32>> {
        Ok(row
            .get::<_, Option<i64>>(at)?
            .map(|value| value.clamp(0, i64::from(u32::MAX)) as u32))
    };

    Ok(SharedStudy {
        file: WholeFile {
            packets: row.get::<_, i64>(1)?.cast_unsigned(),
            frames: row.get::<_, i64>(2)?.max(0).cast_unsigned(),
            rate: row.get(3)?,
        },
        judged_under: JUDGED_UNDER,
        verdict: row.get(4)?,
        cutoff_hz: hertz(5)?,
        cutoff_drop: some_figure(6)?,
        lossy_guess: row.get(7)?,
        upsampled_from: hertz(8)?,
        bits_in_use: small(9)?,
        declared_bits: small(10)?,
        peak: figure(11)?,
        rms: figure(12)?,
        true_peak: figure(13)?,
        loudness: some_figure(14)?,
        loudness_range: some_figure(15)?,
        dynamic_range: small(16)?,
        clipped: row.get::<_, i64>(17)?.max(0).cast_unsigned(),
        mono_as_stereo: row.get(18)?,
        band_levels: row.get(19)?,
    })
}

pub(crate) fn take(inner: &Inner, recording: &Recording, answer: &Answer) -> Result<bool> {
    match answer {
        Answer::Lyrics(lyrics) => {
            inner.write(|transaction| take_lyrics(transaction, recording, lyrics))
        }
        Answer::Study(study) => take_study(inner, study),
        Answer::Release(release) => {
            inner.write(|transaction| take_release(transaction, recording, release))
        }
    }
}

fn take_lyrics(
    transaction: &Transaction<'_>,
    recording: &Recording,
    lyrics: &SharedLyrics,
) -> Result<bool> {
    let Some(cut) = held_strictly(transaction, recording)? else {
        return Ok(false);
    };
    let Some(path) = cut.location.as_path() else {
        return Ok(false);
    };
    let path = store::path_text(path).into_owned();
    let (span_start, _) = store::span_columns(cut.span);
    let told = LyricText {
        text: lyrics.text.clone(),
        synced: lyrics.synced,
        lyricsfile: None,
    };

    let held = sung::kept(transaction, &path, span_start)?.and_then(|kept| kept.sung);
    if held.is_some_and(|held| held.detail() >= told.detail()) {
        return Ok(true);
    }
    sung::keep(
        transaction,
        &path,
        span_start,
        Some(&told),
        SystemTime::now(),
    )?;
    Ok(true)
}

fn held_strictly(connection: &Connection, recording: &Recording) -> Result<Option<Cut>> {
    let Some(cut) = resolved(connection, recording)? else {
        return Ok(None);
    };
    let Some(path) = cut.location.as_path() else {
        return Ok(None);
    };
    let (span_start, _) = store::span_columns(cut.span);
    let held = connection
        .query_row(
            &format!(
                "SELECT {RECORDING_COLUMNS} FROM tracks t {RECORDING_JOINS}
                  WHERE t.path = ?1 AND t.span_start = ?2"
            ),
            params![store::path_text(path), span_start],
            |row| recording_at(row, 0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let Some(held) = held else {
        return Ok(None);
    };

    let one_recording = recording.mbid.is_some() && held.mbid == recording.mbid;
    let one_length = held.length.is_some()
        && recording.length.is_some()
        && lasts_alike(held.length, recording.length);
    Ok((one_recording || one_length).then_some(cut))
}

fn take_study(inner: &Inner, study: &SharedStudy) -> Result<bool> {
    let Some(verdict) = Verdict::named(&study.verdict) else {
        return Ok(true);
    };
    let lossy_guess = match study.lossy_guess.as_deref() {
        None => None,
        Some(named) => match LossyGuess::named(named) {
            Some(guess) => Some(guess),
            None => return Ok(true),
        },
    };
    if study.judged_under != JUDGED_UNDER || study.band_levels.len() != BANDS {
        return Ok(true);
    }

    inner.write(|transaction| {
        let copies = copies_of(transaction, study.file)?;
        for (track, studied) in &copies {
            if !studied {
                write_shared_study(transaction, *track, study, verdict, lossy_guess)?;
            }
        }
        Ok(!copies.is_empty())
    })
}

fn copies_of(connection: &Connection, file: WholeFile) -> Result<Vec<(TrackId, bool)>> {
    let mut statement = connection
        .prepare_cached(&format!(
            "SELECT t.id, s.track_id IS NOT NULL AND s.studied_under = ?4
               FROM tracks t LEFT JOIN track_studies s ON s.track_id = t.id
              WHERE t.packets = ?1 AND t.duration = ?2 AND t.sample_rate = ?3 AND {WHOLE_FILE}"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(
            params![
                file.packets.cast_signed(),
                file.frames.cast_signed(),
                file.rate,
                i64::from(JUDGED_UNDER)
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, bool>(1)?)),
        )
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    rows.into_iter()
        .map(|(id, studied)| Ok((TrackId::new(id.cast_unsigned())?, studied)))
        .collect()
}

fn write_shared_study(
    transaction: &Transaction<'_>,
    track: TrackId,
    study: &SharedStudy,
    verdict: Verdict,
    lossy_guess: Option<LossyGuess>,
) -> Result<()> {
    let figure = |figure: Figure| f64::from(figure.value());
    transaction
        .execute(
            "INSERT OR REPLACE INTO track_studies (
                 track_id, studied, studied_under, verdict, cutoff_hz, cutoff_drop, lossy_guess,
                 upsampled_from, bits_in_use, declared_bits, peak, rms, loudness, dynamic_range,
                 clipped, mono_as_stereo, true_peak, loudness_range, band_levels
             ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
             )",
            params![
                track.get().cast_signed(),
                store::to_nanos(SystemTime::now()),
                i64::from(JUDGED_UNDER),
                verdict.as_str(),
                study.cutoff_hz.map(i64::from),
                study.cutoff_drop.map(figure),
                lossy_guess.map(LossyGuess::as_str),
                study.upsampled_from.map(i64::from),
                study.bits_in_use.map(i64::from),
                study.declared_bits.map(i64::from),
                figure(study.peak),
                figure(study.rms),
                study.loudness.map(figure),
                study.dynamic_range.map(i64::from),
                study.clipped.cast_signed(),
                study.mono_as_stereo,
                figure(study.true_peak),
                study.loudness_range.map(figure),
                study.band_levels,
            ],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    transaction
        .execute(
            "DELETE FROM unstudied WHERE track_id = ?1",
            params![track.get().cast_signed()],
        )
        .map_err(|source| Error::store(StoreOp::Delete, source))?;
    Ok(())
}

fn releases_after(connection: &Connection, mark: i64, most: i64) -> Result<Vec<Answered>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT a.id, a.answered, a.mbid, a.release_group, a.release_title, a.date, a.country,
                    a.label, a.catalog_number, a.barcode, a.kind, a.disambiguation,
                    coalesce(a.front_cover, 0)
               FROM albums a
              WHERE a.answered >= ?1 AND a.mbid IS NOT NULL AND a.release_title IS NOT NULL
                AND a.pressing_fit IS NULL
                AND EXISTS (SELECT 1 FROM release_credits c WHERE c.album_id = a.id)
                AND EXISTS (SELECT 1 FROM release_tracks rt
                             WHERE rt.album_id = a.id AND rt.track_id IS NOT NULL)
              ORDER BY a.answered LIMIT ?2",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let heads = statement
        .query_map(params![mark, most], |row| {
            Ok(ReleaseHead {
                album: row.get(0)?,
                at: row.get(1)?,
                id: row.get(2)?,
                group: row.get(3)?,
                title: row.get(4)?,
                date: row.get(5)?,
                country: row.get(6)?,
                label: row.get(7)?,
                catalog_number: row.get(8)?,
                barcode: row.get(9)?,
                kind: row.get(10)?,
                disambiguation: row.get(11)?,
                has_front_cover: row.get(12)?,
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut answered = Vec::with_capacity(heads.len());
    for head in heads {
        let Some(id) = store::mbid_in(head.id.as_deref()) else {
            continue;
        };
        let Some(recording) = first_paired(connection, head.album)? else {
            continue;
        };
        let release = SharedRelease {
            id,
            group: store::mbid_in(head.group.as_deref()),
            title: head.title,
            credit: credit_of(connection, head.album)?,
            date: head.date,
            country: head.country,
            label: head.label,
            catalog_number: head.catalog_number,
            barcode: head.barcode,
            kind: head.kind,
            disambiguation: head.disambiguation,
            has_front_cover: head.has_front_cover,
            links: links_of(
                connection,
                "SELECT relation, url FROM album_links WHERE album_id = ?1 ORDER BY url",
                head.album,
            )?,
            media: media_of(connection, head.album)?,
        };
        answered.push(Answered {
            recording,
            at: head.at,
            answer: Answer::Release(Box::new(release)),
        });
    }
    Ok(answered)
}

fn first_paired(connection: &Connection, album: i64) -> Result<Option<Recording>> {
    connection
        .query_row(
            &format!(
                "SELECT {RECORDING_COLUMNS}
                   FROM release_tracks rt JOIN tracks t ON t.id = rt.track_id
                   {RECORDING_JOINS}
                  WHERE rt.album_id = ?1
                  ORDER BY rt.disc, rt.position LIMIT 1"
            ),
            params![album],
            |row| recording_at(row, 0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn credit_of(connection: &Connection, album: i64) -> Result<Vec<SharedCredit>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT name, joined_by, mbid FROM release_credits WHERE album_id = ?1
              ORDER BY position",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![album], |row| {
            Ok(SharedCredit {
                name: row.get(0)?,
                joined_by: row.get(1)?,
                mbid: store::mbid_in(row.get::<_, Option<String>>(2)?.as_deref()),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn links_of(connection: &Connection, query: &str, owner: i64) -> Result<Vec<Link>> {
    let mut statement = connection
        .prepare_cached(query)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![owner], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    rows.into_iter()
        .map(|(relation, url)| Ok(Link::new(store::relation_of(relation)?.spelled(), url)))
        .collect()
}

fn codes_of(connection: &Connection, row: i64, first: Option<String>) -> Result<Vec<String>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT isrc FROM release_track_isrcs WHERE release_track_id = ?1 ORDER BY isrc",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rest = statement
        .query_map(params![row], |row| row.get::<_, String>(0))
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    let others = rest
        .into_iter()
        .filter(|code| first.as_ref() != Some(code))
        .collect::<Vec<_>>();
    Ok(first.into_iter().chain(others).collect())
}

fn media_of(connection: &Connection, album: i64) -> Result<Vec<SharedMedium>> {
    let mut statement = connection
        .prepare_cached("SELECT position, format, title FROM release_media WHERE album_id = ?1")
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let mut media = statement
        .query_map(params![album], |row| {
            Ok(SharedMedium {
                position: row.get(0)?,
                format: row.get(1)?,
                title: row.get(2)?,
                tracks: Vec::new(),
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .into_iter()
        .map(|medium| (medium.position, medium))
        .collect::<BTreeMap<_, _>>();

    let mut statement = connection
        .prepare_cached(
            "SELECT id, disc, position, number, title, artist, recording_mbid, track_mbid,
                    length_ms, isrc
               FROM release_tracks WHERE album_id = ?1 ORDER BY disc, position",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![album], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, Option<String>>(9)?,
                SharedTrack {
                    position: row.get(2)?,
                    number: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    title: row.get(4)?,
                    artist: row.get(5)?,
                    recording: store::mbid_in(row.get::<_, Option<String>>(6)?.as_deref()),
                    track: store::mbid_in(row.get::<_, Option<String>>(7)?.as_deref()),
                    length_ms: row
                        .get::<_, Option<i64>>(8)?
                        .map(|length| u32::try_from(length.max(0)).unwrap_or(u32::MAX)),
                    isrcs: Vec::new(),
                    links: Vec::new(),
                },
            ))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    for (row, disc, first, track) in rows {
        let track = SharedTrack {
            isrcs: codes_of(connection, row, first)?,
            links: links_of(
                connection,
                "SELECT relation, url FROM release_track_links WHERE release_track_id = ?1
                  ORDER BY url",
                row,
            )?,
            ..track
        };
        media
            .entry(disc)
            .or_insert_with(|| SharedMedium {
                position: disc,
                format: None,
                title: None,
                tracks: Vec::new(),
            })
            .tracks
            .push(track);
    }
    Ok(media.into_values().collect())
}

fn as_release(shared: &SharedRelease) -> Release {
    Release {
        id: shared.id.clone(),
        group: shared.group.clone(),
        title: shared.title.clone(),
        credit: shared
            .credit
            .iter()
            .map(|credit| Credit {
                name: credit.name.clone(),
                joined_by: credit.joined_by.clone(),
                mbid: credit.mbid.clone(),
            })
            .collect(),
        date: shared.date.clone(),
        country: shared.country.clone(),
        label: shared.label.clone(),
        catalog_number: shared.catalog_number.clone(),
        barcode: shared.barcode.clone(),
        kind: shared.kind.clone(),
        disambiguation: shared.disambiguation.clone(),
        has_front_cover: shared.has_front_cover,
        links: shared.links.clone(),
        media: shared
            .media
            .iter()
            .map(|medium| Medium {
                position: medium.position,
                format: medium.format.clone(),
                title: medium.title.clone(),
                tracks: medium
                    .tracks
                    .iter()
                    .map(|track| ReleaseTrack {
                        position: track.position,
                        number: track.number.clone(),
                        title: track.title.clone(),
                        artist: track.artist.clone(),
                        recording: track.recording.clone(),
                        track: track.track.clone(),
                        length: track
                            .length_ms
                            .map(|length| Duration::from_millis(u64::from(length))),
                        isrcs: track.isrcs.clone(),
                        links: track.links.clone(),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn take_release(
    transaction: &Transaction<'_>,
    recording: &Recording,
    shared: &SharedRelease,
) -> Result<bool> {
    let (albums, held_as_keyed) = albums_holding(transaction, recording, shared)?;
    if albums.is_empty() {
        return Ok(false);
    }

    let release = as_release(shared);
    let mut landed = false;
    for album in albums {
        let Some(pressing) = landing_on(transaction, album, &release)? else {
            continue;
        };
        enriched::land_release(transaction, album, &release, pressing, SystemTime::now())?;
        enriched::rematch_release_tracks(transaction, album)?;
        identify_the_credited(transaction, &release.credit)?;
        landed = true;
    }
    Ok(landed || held_as_keyed)
}

fn albums_holding(
    connection: &Connection,
    recording: &Recording,
    release: &SharedRelease,
) -> Result<(BTreeSet<AlbumId>, bool)> {
    let mut albums = BTreeSet::new();
    albums.extend(album_holding_strictly(connection, recording)?);
    if albums.is_empty() {
        albums.extend(albums_holding_its_recordings(connection, release)?);
    }
    let held_as_keyed = !albums.is_empty();
    if !held_as_keyed {
        for track in release.tracks() {
            let named = Recording {
                mbid: track.recording.clone(),
                title: track.title.clone(),
                artist: track
                    .artist
                    .clone()
                    .unwrap_or_else(|| recording.artist.clone()),
                album: recording.album.clone(),
                length: track
                    .length_ms
                    .map(|length| Duration::from_millis(u64::from(length))),
            };
            albums.extend(album_holding_strictly(connection, &named)?);
        }
    }
    let albums = albums
        .into_iter()
        .map(|album| Ok(AlbumId::new(album.cast_unsigned())?))
        .collect::<Result<_>>()?;
    Ok((albums, held_as_keyed))
}

fn album_holding_strictly(connection: &Connection, recording: &Recording) -> Result<Option<i64>> {
    let Some(cut) = held_strictly(connection, recording)? else {
        return Ok(None);
    };
    let Some(path) = cut.location.as_path() else {
        return Ok(None);
    };
    let (span_start, _) = store::span_columns(cut.span);
    connection
        .query_row(
            "SELECT album_id FROM tracks WHERE path = ?1 AND span_start = ?2",
            params![store::path_text(path), span_start],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn albums_holding_its_recordings(
    connection: &Connection,
    release: &SharedRelease,
) -> Result<BTreeSet<i64>> {
    let mut albums = BTreeSet::new();
    let mut statement = connection
        .prepare_cached(
            "SELECT DISTINCT album_id FROM tracks WHERE mbid = ?1 AND album_id IS NOT NULL",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    for recorded in release
        .tracks()
        .filter_map(|track| track.recording.as_ref())
    {
        let held = statement
            .query_map(params![recorded.as_str()], |row| row.get::<_, i64>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        albums.extend(held);
    }
    Ok(albums)
}

fn landing_on(
    connection: &Connection,
    album: AlbumId,
    release: &Release,
) -> Result<Option<Pressing>> {
    let Some(asked) = enriched::album_to_ask(connection, album)? else {
        return Ok(None);
    };
    if enriched::refuses(connection, album, &release.id)? {
        return Ok(None);
    }
    let answered: bool = connection
        .query_row(
            "SELECT answered IS NOT NULL FROM albums WHERE id = ?1",
            params![album.get().cast_signed()],
            |row| row.get(0),
        )
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    if let Some(named) = &asked.mbid {
        return Ok((*named == release.id && !answered).then_some(Pressing::AsHeld));
    }
    let another_group = asked
        .group
        .as_ref()
        .is_some_and(|group| release.group.as_ref() != Some(group));
    if another_group {
        return Ok(None);
    }

    let found = ReleaseMatch {
        release: release.id.clone(),
        group: release.group.clone(),
        score: EXACT_SCORE,
        title: release.title.clone(),
        credit: release.credit.clone(),
        track_count: Some(release.track_count()),
        date: release.date.clone(),
    };
    Ok(takes_strictly(&found, &asked).then_some(Pressing::Fitted(Fit::Exact)))
}

fn identify_the_credited(transaction: &Transaction<'_>, credit: &[Credit]) -> Result<()> {
    for credited in credit {
        let Some(mbid) = &credited.mbid else {
            continue;
        };
        transaction
            .execute(
                "UPDATE artists SET mbid = ?1 WHERE key = ?2 AND mbid IS NULL",
                params![mbid.as_str(), store::folded_letters(&credited.name)],
            )
            .map_err(|source| Error::store(StoreOp::Update, source))?;
    }
    Ok(())
}

const PICTURED: &str = "(cover_art IS NOT NULL OR cover_path IS NOT NULL)";

pub(crate) fn covers_wanted(inner: &Inner, most: usize) -> Result<Vec<Mbid>> {
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    let ids = inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT mbid FROM albums WHERE mbid IS NOT NULL AND NOT {PICTURED}
                  GROUP BY mbid ORDER BY max(id) DESC LIMIT ?1"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(params![most], |row| row.get::<_, String>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    Ok(ids
        .iter()
        .filter_map(|id| store::mbid_in(Some(id)))
        .collect())
}

pub(crate) fn album_covering(inner: &Inner, release: &Mbid) -> Result<Option<AlbumId>> {
    let album = inner.read(|connection| {
        connection
            .query_row(
                &format!(
                    "SELECT id FROM albums WHERE mbid = ?1 AND {PICTURED}
                       AND (pressing_fit IS NULL OR cover_source IS NOT ?2)
                     ORDER BY id LIMIT 1"
                ),
                params![
                    release.as_str(),
                    store::cover_source_code(CoverSource::File)
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    album
        .map(|album| Ok(AlbumId::new(album.cast_unsigned())?))
        .transpose()
}

pub(crate) fn albums_uncovered(inner: &Inner, release: &Mbid) -> Result<Vec<AlbumId>> {
    let albums = inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT id FROM albums WHERE mbid = ?1 AND NOT {PICTURED} ORDER BY id"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(params![release.as_str()], |row| row.get::<_, i64>(0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })?;
    albums
        .into_iter()
        .map(|album| Ok(AlbumId::new(album.cast_unsigned())?))
        .collect()
}

pub(crate) fn lyricsfiles_wanted(
    inner: &Inner,
    skip: usize,
    most: usize,
) -> Result<Vec<Recording>> {
    let skip = i64::try_from(skip).unwrap_or(i64::MAX);
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT {RECORDING_COLUMNS}
                   FROM lyrics_kept k JOIN tracks t ON t.path = k.path AND t.span_start = k.span_start
                   {RECORDING_JOINS}
                  WHERE k.text IS NOT NULL AND k.lyricsfile IS NULL AND t.hidden = 0
                  ORDER BY k.taken DESC, t.id LIMIT ?1 OFFSET ?2"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let wanted = statement
            .query_map(params![most, skip], |row| recording_at(row, 0))
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        Ok(wanted.into_iter().flatten().collect())
    })
}

pub(crate) fn lyricsfile_to_share(
    inner: &Inner,
    recording: &Recording,
) -> Result<Option<SharedLyricsfile>> {
    inner.read(|connection| {
        let Some((path, span_start)) = held_row(connection, recording)? else {
            return Ok(None);
        };
        let kept = sung::kept(connection, &path, span_start)?.and_then(|kept| kept.sung);
        Ok(kept.and_then(|kept| {
            Some(SharedLyricsfile {
                text: kept.text,
                synced: kept.synced,
                document: kept.lyricsfile?,
            })
        }))
    })
}

pub(crate) fn take_lyricsfile(
    inner: &Inner,
    recording: &Recording,
    shared: &SharedLyricsfile,
) -> Result<bool> {
    if !shared.travels() {
        return Ok(false);
    }
    inner.write(|transaction| {
        let Some((path, span_start)) = held_row(transaction, recording)? else {
            return Ok(false);
        };
        let told = LyricText {
            text: shared.text.clone(),
            synced: shared.synced,
            lyricsfile: Some(shared.document.clone()),
        };
        let held = sung::kept(transaction, &path, span_start)?.and_then(|kept| kept.sung);
        if held.is_some_and(|held| held.detail() >= told.detail()) {
            return Ok(false);
        }
        sung::keep(
            transaction,
            &path,
            span_start,
            Some(&told),
            SystemTime::now(),
        )
    })
}

fn file_at(row: &Row<'_>, from: usize) -> rusqlite::Result<WholeFile> {
    Ok(WholeFile {
        packets: row.get::<_, i64>(from)?.cast_unsigned(),
        frames: row.get::<_, i64>(from + 1)?.max(0).cast_unsigned(),
        rate: row.get(from + 2)?,
    })
}

pub(crate) fn prints_wanted(inner: &Inner, skip: usize, most: usize) -> Result<Vec<WholeFile>> {
    let skip = i64::try_from(skip).unwrap_or(i64::MAX);
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    inner.read(|connection| {
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT t.packets, t.duration, t.sample_rate
                   FROM tracks t JOIN track_studies s ON s.track_id = t.id
                  WHERE s.studied_under = ?1 AND (s.print IS NULL OR s.recognised IS NULL)
                    AND t.packets IS NOT NULL AND t.duration IS NOT NULL AND {WHOLE_FILE}
                  GROUP BY t.packets, t.duration, t.sample_rate
                  ORDER BY max(s.studied) DESC, min(t.id) LIMIT ?2 OFFSET ?3"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        statement
            .query_map(params![i64::from(JUDGED_UNDER), most, skip], |row| {
                file_at(row, 0)
            })
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))
    })
}

pub(crate) fn print_to_share(inner: &Inner, file: WholeFile) -> Result<Option<SharedPrint>> {
    inner.read(|connection| {
        let kept = connection
            .query_row(
                &format!(
                    "SELECT s.print, s.print_length, s.recognised IS NOT NULL, s.heard_as,
                            s.heard_score, s.heard_title, s.heard_artist
                       FROM tracks t JOIN track_studies s ON s.track_id = t.id
                      WHERE t.packets = ?1 AND t.duration = ?2 AND t.sample_rate = ?3
                        AND {WHOLE_FILE} AND s.studied_under = ?4 AND s.print IS NOT NULL
                      ORDER BY s.recognised IS NOT NULL DESC, s.studied DESC LIMIT 1"
                ),
                params![
                    file.packets.cast_signed(),
                    file.frames.cast_signed(),
                    file.rate,
                    i64::from(JUDGED_UNDER)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, bool>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<String>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                    ))
                },
            )
            .optional()
            .map_err(|source| Error::store(StoreOp::Query, source))?;
        let Some((encoded, length, recognised, heard_as, score, title, artist)) = kept else {
            return Ok(None);
        };

        let length = Duration::from_secs(length.unwrap_or_default().max(0).cast_unsigned());
        let Ok(print) = Chromaprint::new(&encoded, length) else {
            return Ok(None);
        };
        let recognition = match (recognised, heard_as, title) {
            (false, _, _) => None,
            (true, None, _) => Some(SharedRecognition::Unheard),
            (true, Some(heard_as), Some(title)) => {
                store::mbid_in(Some(&heard_as)).map(|recording| {
                    SharedRecognition::Heard(SharedHeard {
                        recording,
                        score: score.map_or(0, |score| score.clamp(0, i64::from(u8::MAX)) as u8),
                        title,
                        artist,
                    })
                })
            }
            (true, Some(_), None) => None,
        };
        Ok(Some(SharedPrint { print, recognition }))
    })
}

pub(crate) fn take_print(inner: &Inner, file: WholeFile, shared: &SharedPrint) -> Result<bool> {
    if !shared.travels() {
        return Ok(false);
    }
    inner.write(|transaction| {
        let mut statement = transaction
            .prepare_cached(&format!(
                "SELECT t.id, s.print IS NULL, s.recognised IS NULL
                   FROM tracks t JOIN track_studies s ON s.track_id = t.id
                  WHERE t.packets = ?1 AND t.duration = ?2 AND t.sample_rate = ?3
                    AND {WHOLE_FILE} AND s.studied_under = ?4"
            ))
            .map_err(|source| Error::store(StoreOp::Prepare, source))?;
        let copies = statement
            .query_map(
                params![
                    file.packets.cast_signed(),
                    file.frames.cast_signed(),
                    file.rate,
                    i64::from(JUDGED_UNDER)
                ],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, bool>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                },
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(|source| Error::store(StoreOp::Query, source))?;

        let mut landed = false;
        for (id, unprinted, unrecognised) in copies {
            let track = TrackId::new(id.cast_unsigned())?;
            if unprinted {
                landed |= studies::write_print(transaction, track, &shared.print)?;
            }
            if let (true, Some(recognition)) = (unrecognised, &shared.recognition) {
                landed |= take_recognition(transaction, track, recognition)?;
            }
        }
        Ok(landed)
    })
}

fn take_recognition(
    transaction: &Transaction<'_>,
    track: TrackId,
    recognition: &SharedRecognition,
) -> Result<bool> {
    let Some(named) = enriched::track_as_heard(transaction, track)? else {
        return Ok(false);
    };
    let heard = match recognition {
        SharedRecognition::Unheard => None,
        SharedRecognition::Heard(heard) => Some(HeardAs {
            recording: heard.recording.clone(),
            score: heard.score,
            title: heard.title.clone(),
            artist: heard.artist.clone(),
        }),
    };
    let found: Vec<RecordingMatch> = heard.iter().map(HeardAs::as_a_match).collect();
    let weighed = agreement(&found, &named);
    studies::write_recognition(
        transaction,
        track,
        heard.as_ref(),
        weighed,
        SystemTime::now(),
    )
}

fn held_row(connection: &Connection, recording: &Recording) -> Result<Option<(String, i64)>> {
    let Some(cut) = held_strictly(connection, recording)? else {
        return Ok(None);
    };
    let Some(path) = cut.location.as_path() else {
        return Ok(None);
    };
    let (span_start, _) = store::span_columns(cut.span);
    Ok(Some((store::path_text(path).into_owned(), span_start)))
}
