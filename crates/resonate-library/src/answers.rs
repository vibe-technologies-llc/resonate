use std::time::SystemTime;

use resonate_analysis::{BANDS, JUDGED_UNDER, LossyGuess, Verdict};
use resonate_core::{
    Answer, Answered, Figure, Recording, SharedLyrics, SharedStudy, TrackId, WholeFile,
};
use rusqlite::{Connection, Row, Transaction, params};

use crate::{
    Cut, Error, LyricText, Result, StoreOp,
    db::Inner,
    store, sung,
    synced::{RECORDING_COLUMNS, RECORDING_JOINS, lasts_alike, recording_at, resolved},
};

const WHOLE_FILE: &str = "t.span_start = 0 AND t.span_frames IS NULL";

pub(crate) fn after(inner: &Inner, mark: i64, most: usize) -> Result<Vec<Answered>> {
    let most = i64::try_from(most).unwrap_or(i64::MAX);
    let mut answered = inner.read(|connection| {
        let mut answered = lyrics_after(connection, mark, most)?;
        answered.extend(studies_after(connection, mark, most)?);
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
