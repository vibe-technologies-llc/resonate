use resonate_analysis::{BANDS, JUDGED_UNDER, LossyGuess, Verdict};
use resonate_codec::{Sources, probe_scanned};
use resonate_core::{Answer, Figure, Recording, SharedLyrics, SharedStudy, SourceId, WholeFile};
use resonate_library::LyricText;
use resonate_lyrics::read_lyrics;
use rusqlite::{OptionalExtension, Row, params};

use super::{
    Answered, DocumentCatalog, Learnt, NANOS_A_SECOND, RECORDING_COLUMNS, lasts_alike,
    recording_at, unavailable,
};
use crate::{
    catalog::Catalog,
    catalog_extras::LandedStudy,
    documents::location,
    enrichment::{SHARED_LYRICS, is_synced},
};

const NEARLY_THE_SAME_MILLISECOND: i64 = 1;

struct Alike {
    id: u64,
    uri: String,
    packets: Option<i64>,
    frames: Option<i64>,
}

impl DocumentCatalog {
    pub(super) fn lyrics_to_share(&self, mark: i64, most: usize) -> Answered<Vec<Learnt>> {
        let most = i64::try_from(most).unwrap_or(i64::MAX);
        let rows = {
            let connection = self.catalog.0.lock();
            let mut query = connection
                .prepare_cached(&format!(
                    "SELECT l.asked, l.text, {RECORDING_COLUMNS}
                       FROM lyrics l JOIN documents d ON d.id = l.track
                      WHERE l.text IS NOT NULL AND d.missing = 0 AND l.asked * ?3 >= ?1
                      ORDER BY l.asked, l.track LIMIT ?2"
                ))
                .map_err(unavailable)?;
            query
                .query_map(params![mark, most, NANOS_A_SECOND], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        recording_at(row, 2)?,
                    ))
                })
                .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
                .map_err(unavailable)?
        };

        Ok(rows
            .into_iter()
            .map(|(asked, text, recording)| Learnt {
                recording,
                at: asked.saturating_mul(NANOS_A_SECOND),
                answer: Answer::Lyrics(SharedLyrics {
                    synced: is_synced(&text),
                    text,
                }),
            })
            .filter(|learnt| learnt.answer.travels())
            .collect())
    }

    pub(super) fn studies_to_share(&self, mark: i64, most: usize) -> Answered<Vec<Learnt>> {
        let most = i64::try_from(most).unwrap_or(i64::MAX);
        let connection = self.catalog.0.lock();
        let mut query = connection
            .prepare_cached(&format!(
                "SELECT s.studied, d.packets, d.frames, d.rate, s.verdict, s.cutoff_hz,
                        s.cutoff_drop, s.lossy_guess, s.upsampled_from, s.bits, s.declared_bits,
                        s.peak, s.rms, s.true_peak, s.loudness, s.loudness_range,
                        s.dynamic_range, s.clipped, s.mono_as_stereo, s.band_levels,
                        {RECORDING_COLUMNS}
                   FROM studies s JOIN documents d ON d.id = s.track
                  WHERE s.studied >= ?1 AND s.studied_under = ?3 AND length(s.band_levels) = ?4
                    AND d.missing = 0 AND d.packets IS NOT NULL AND d.frames IS NOT NULL
                  ORDER BY s.studied, s.track LIMIT ?2"
            ))
            .map_err(unavailable)?;
        query
            .query_map(
                params![mark, most, i64::from(JUDGED_UNDER), BANDS as i64],
                |row| {
                    let at: i64 = row.get(0)?;
                    let study = study_at(row)?;
                    Ok(Learnt {
                        recording: recording_at(row, 20)?,
                        at,
                        answer: Answer::Study(study),
                    })
                },
            )
            .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
            .map_err(unavailable)
    }

    pub(super) fn take_lyrics(
        &self,
        recording: &Recording,
        lyrics: &SharedLyrics,
    ) -> Answered<bool> {
        let Some(id) = self.held_strictly(recording)? else {
            return Ok(false);
        };
        let reads = SourceId::new(SHARED_LYRICS)
            .ok()
            .and_then(|source| read_lyrics(source, &lyrics.text).ok().flatten())
            .is_some();
        if !reads {
            return Ok(true);
        }

        let told = LyricText {
            text: lyrics.text.clone(),
            synced: lyrics.synced,
            lyricsfile: None,
        };
        let held = self.catalog.lyric_text(id).map_err(failed)?;
        let as_rich_already = held
            .and_then(|(text, document, _)| {
                text.map(|text| LyricText {
                    synced: is_synced(&text),
                    text,
                    lyricsfile: document,
                })
            })
            .is_some_and(|held| held.detail() >= told.detail());
        if as_rich_already {
            return Ok(true);
        }

        self.catalog
            .keep_lyrics(id, Some(&told.text), None, SHARED_LYRICS)
            .map_err(failed)?;
        self.sheets.forget(id);
        Ok(true)
    }

    pub(super) fn take_study(&self, study: &SharedStudy) -> Answered<bool> {
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

        let mut same_file = false;
        for copy in the_same_file(&self.catalog, &self.sources, study.file)? {
            same_file = true;
            self.catalog
                .land_study(
                    copy,
                    &LandedStudy {
                        verdict: verdict.as_str(),
                        peak: study.peak.value(),
                        rms: study.rms.value(),
                        true_peak: study.true_peak.value(),
                        loudness: study.loudness.map(Figure::value),
                        dynamic_range: study.dynamic_range,
                        clipped: study.clipped,
                        bits: study.bits_in_use,
                        band_levels: &study.band_levels,
                        cutoff_hz: study.cutoff_hz,
                        cutoff_drop: study.cutoff_drop.map(Figure::value),
                        lossy_guess: lossy_guess.map(LossyGuess::as_str),
                        upsampled_from: study.upsampled_from,
                        declared_bits: study.declared_bits,
                        loudness_range: study.loudness_range.map(Figure::value),
                        mono_as_stereo: study.mono_as_stereo,
                    },
                )
                .map_err(failed)?;
        }
        Ok(same_file)
    }

    fn held_strictly(&self, recording: &Recording) -> Answered<Option<u64>> {
        let connection = self.catalog.0.lock();
        let Some(id) = self.resolved_in(&connection, recording)? else {
            return Ok(None);
        };
        let held = connection
            .query_row(
                &format!("SELECT {RECORDING_COLUMNS} FROM documents d WHERE d.id = ?1"),
                [id as i64],
                |row| recording_at(row, 0),
            )
            .optional()
            .map_err(unavailable)?;
        let Some(held) = held else {
            return Ok(None);
        };

        let one_recording = recording.mbid.is_some() && held.mbid == recording.mbid;
        let one_length = held.length.is_some()
            && recording.length.is_some()
            && lasts_alike(held.length, recording.length);
        Ok((one_recording || one_length).then_some(id))
    }
}

pub(super) fn the_same_file(
    catalog: &Catalog,
    sources: &Sources,
    file: WholeFile,
) -> Answered<Vec<u64>> {
    Ok(copies_of(catalog, file)?
        .into_iter()
        .filter(|copy| identity_of(catalog, sources, copy) == Some((file.packets, file.frames)))
        .map(|copy| copy.id)
        .collect())
}

fn copies_of(catalog: &Catalog, file: WholeFile) -> Answered<Vec<Alike>> {
    let milliseconds = i64::try_from(u128::from(file.frames) * 1000 / u128::from(file.rate.max(1)))
        .unwrap_or(i64::MAX);
    let connection = catalog.0.lock();
    let mut query = connection
        .prepare_cached(
            "SELECT d.id, d.uri, d.packets, d.frames
               FROM documents d
              WHERE d.missing = 0 AND d.rate = ?1 AND d.duration_ms BETWEEN ?2 AND ?3
              ORDER BY d.id",
        )
        .map_err(unavailable)?;
    query
        .query_map(
            params![
                file.rate,
                milliseconds.saturating_sub(NEARLY_THE_SAME_MILLISECOND),
                milliseconds.saturating_add(NEARLY_THE_SAME_MILLISECOND),
            ],
            |row| {
                Ok(Alike {
                    id: row.get::<_, i64>(0)?.cast_unsigned(),
                    uri: row.get(1)?,
                    packets: row.get(2)?,
                    frames: row.get(3)?,
                })
            },
        )
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(unavailable)
}

fn identity_of(catalog: &Catalog, sources: &Sources, copy: &Alike) -> Option<(u64, u64)> {
    if let (Some(packets), Some(frames)) = (copy.packets, copy.frames) {
        return Some((packets.cast_unsigned(), frames.cast_unsigned()));
    }
    let scanned = location(&copy.uri)
        .ok()
        .and_then(|at| probe_scanned(sources, &at).ok())?;
    if let Err(error) = catalog.keep_identity(copy.id, &scanned) {
        tracing::warn!(%error, "the identity of a document could not be kept");
    }
    let packets = scanned.packets?.0;
    let frames = scanned.info.duration?.get();
    Some((packets, frames))
}

fn failed(error: crate::Error) -> resonate_sync::Refused {
    tracing::warn!(%error, "the document catalog could not take an answer");
    resonate_sync::Refused::Unavailable
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
        mono_as_stereo: row.get::<_, Option<bool>>(18)?.unwrap_or(false),
        band_levels: row.get(19)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use resonate_analysis::{BandLevels, Watch};
    use resonate_core::Mbid;
    use resonate_sync::Catalog;

    use super::*;
    use crate::{
        features::AnalysisReport,
        harness::{FOLDER, Harness, RATE, silence},
        sync::tests::{documents_of, folder, named, player, song},
    };

    const RECORDING: &str = "b1a9c0e9-d987-4042-ae91-78d6a3267d69";
    const TIMED: &str = "[00:01.00]First line\n[00:02.00]Second line";
    const PLAIN: &str = "First line\nSecond line";
    const OTHER_PLAIN: &str = "Another reading\nOf the words";
    const WAV_HEADER_BYTES: usize = 44;

    fn lyrics_of(text: &str, synced: bool) -> SharedLyrics {
        SharedLyrics {
            text: text.to_owned(),
            synced,
        }
    }

    fn kept_text(documents: &DocumentCatalog, id: u64) -> Option<String> {
        documents
            .catalog
            .lyric_text(id)
            .expect("kept lyrics")
            .and_then(|(text, _, _)| text)
    }

    fn study_of(file: WholeFile, verdict: &str) -> SharedStudy {
        SharedStudy {
            file,
            judged_under: JUDGED_UNDER,
            verdict: verdict.to_owned(),
            cutoff_hz: Some(16_000),
            cutoff_drop: Some(Figure::of(42.5)),
            lossy_guess: Some(LossyGuess::Near128.as_str().to_owned()),
            upsampled_from: None,
            bits_in_use: Some(16),
            declared_bits: Some(24),
            peak: Figure::of(-3.5),
            rms: Figure::of(-18.25),
            true_peak: Figure::of(-2.5),
            loudness: Some(Figure::of(-14.0)),
            loudness_range: Some(Figure::of(6.5)),
            dynamic_range: Some(11),
            clipped: 7,
            mono_as_stereo: false,
            band_levels: vec![40; BANDS],
        }
    }

    fn whole_file_of(harness: &Harness, name: &str) -> WholeFile {
        let scanned = probe_scanned(
            &harness.player.sources,
            &location(&format!("{FOLDER}{name}")).expect("a document"),
        )
        .expect("a scanned file");
        WholeFile {
            packets: scanned.packets.expect("a digest").0,
            frames: scanned.info.duration.expect("a length").get(),
            rate: RATE,
        }
    }

    fn with_a_tone(harness: &Harness) -> u64 {
        let mut bytes = silence();
        for byte in &mut bytes[WAV_HEADER_BYTES..] {
            *byte = 0x11;
        }
        fs::write(harness.folder.join("e.wav"), bytes).expect("a tone");
        harness
            .player
            .index_document(format!("{FOLDER}e.wav"), "e.wav".to_owned())
            .expect("an indexed tone")
            .id
    }

    fn studied_as(harness: &Harness, id: u64) -> Option<String> {
        harness
            .player
            .catalog
            .0
            .lock()
            .query_row(
                "SELECT verdict FROM studies WHERE track = ?1",
                [id as i64],
                |row| row.get(0),
            )
            .optional()
            .expect("a read")
    }

    fn every_studied(harness: &Harness) -> Vec<Option<String>> {
        harness
            .songs
            .iter()
            .map(|song| studied_as(harness, song.id))
            .collect()
    }

    fn documents_over(harness: &Harness) -> DocumentCatalog {
        documents_of(&harness.player, &harness.folder)
    }

    fn change(harness: &Harness, sql: &str, id: u64) {
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(sql, [id as i64])
            .expect("a change");
    }

    fn study_row(harness: &Harness, id: u64, verdict: &str, under: Option<u32>) {
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "INSERT INTO studies (track, verdict, peak, rms, true_peak, clipped, band_levels, studied, studied_under)
                 VALUES (?1, ?2, -1.0, -10.0, -1.0, 0, zeroblob(?3), 1, ?4)",
                params![id as i64, verdict, BANDS as i64, under],
            )
            .expect("a study");
    }

    #[test]
    fn kept_lyrics_are_listed_for_sending_with_their_timing_and_the_song_they_belong_to() {
        let folder = folder("lyrics-listed");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        let timed = song(&player, "content://a/timed", "Joga", Some(RECORDING));
        let plain = song(&player, "content://a/plain", "Hunter", None);
        let missed = song(&player, "content://a/missed", "Bachelorette", None);
        let long = song(&player, "content://a/long", "Pluto", None);
        let oversized = "la ".repeat(11_000);
        for (track, text) in [
            (&timed, Some(TIMED)),
            (&plain, Some(PLAIN)),
            (&missed, None),
            (&long, Some(oversized.as_str())),
        ] {
            player
                .catalog
                .keep_lyrics(track.id, text, None, "lrclib")
                .expect("kept lyrics");
        }

        let listed = documents.lyrics_to_share(0, 10).expect("listed");
        let after_now = documents
            .lyrics_to_share(i64::MAX, 10)
            .expect("listed after the future");

        let mut told: Vec<_> = listed
            .iter()
            .map(|learnt| match &learnt.answer {
                Answer::Lyrics(lyrics) => (
                    learnt.recording.title.clone(),
                    lyrics.synced,
                    learnt.recording.mbid.is_some(),
                ),
                Answer::Study(_) | Answer::Release(_) => panic!("not words among the words"),
            })
            .collect();
        told.sort();
        assert_eq!(
            told,
            vec![
                ("Hunter".to_owned(), false, false),
                ("Joga".to_owned(), true, true),
            ]
        );
        assert!(after_now.is_empty());
        assert!(documents.takes_answers());
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn words_land_only_on_a_song_held_strictly_and_never_over_as_rich_words() {
        let folder = folder("lyrics-landed");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        let held = song(&player, "content://a/joga", "Joga", Some(RECORDING));
        let by_name = named("Joga", "Björk", 305);
        let by_recording = Recording {
            mbid: Mbid::new(RECORDING).ok(),
            length: None,
            ..named("Another title", "Someone", 10)
        };
        let without_length = Recording {
            length: None,
            ..named("Joga", "Björk", 305)
        };
        let too_long_apart = named("Joga", "Björk", 320);
        let not_held = named("Hunter", "Björk", 305);

        let strangers = [&without_length, &too_long_apart, &not_held]
            .map(|recording| documents.take_lyrics(recording, &lyrics_of(TIMED, true)));
        let nothing_yet = kept_text(&documents, held.id);
        let landed = documents.take_lyrics(&by_name, &lyrics_of(PLAIN, false));
        let plain_kept = kept_text(&documents, held.id);
        let as_rich = documents.take_lyrics(&by_recording, &lyrics_of(OTHER_PLAIN, false));
        let unchanged = kept_text(&documents, held.id);
        let richer = documents.take_lyrics(&by_name, &lyrics_of(TIMED, true));
        let timed_kept = kept_text(&documents, held.id);
        let plainer_later = documents.take_lyrics(&by_name, &lyrics_of(OTHER_PLAIN, false));
        let still_timed = kept_text(&documents, held.id);

        assert_eq!(strangers, [Ok(false), Ok(false), Ok(false)]);
        assert_eq!(nothing_yet, None);
        assert_eq!(landed, Ok(true));
        assert_eq!(plain_kept.as_deref(), Some(PLAIN));
        assert_eq!(as_rich, Ok(true));
        assert_eq!(unchanged.as_deref(), Some(PLAIN));
        assert_eq!(richer, Ok(true));
        assert_eq!(timed_kept.as_deref(), Some(TIMED));
        assert_eq!(plainer_later, Ok(true));
        assert_eq!(still_timed.as_deref(), Some(TIMED));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn words_landed_are_listed_again_as_they_arrived() {
        let folder = folder("lyrics-echo");
        let player = player(&folder);
        let documents = documents_of(&player, &folder);
        song(&player, "content://a/joga", "Joga", Some(RECORDING));

        documents
            .take_lyrics(&named("Joga", "Björk", 305), &lyrics_of(TIMED, true))
            .expect("landed");
        let listed = documents.lyrics_to_share(0, 10).expect("listed");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].answer, Answer::Lyrics(lyrics_of(TIMED, true)));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_study_lands_on_every_copy_that_is_the_same_file_and_on_no_other_encode() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let tone = with_a_tone(&harness);
        let file = whole_file_of(&harness, "a.wav");

        let landed = documents.take_study(&study_of(file, "fake"));

        assert_eq!(landed, Ok(true));
        assert_eq!(
            every_studied(&harness),
            vec![Some("fake".to_owned()); harness.songs.len()]
        );
        assert_eq!(studied_as(&harness, tone), None);
    }

    #[test]
    fn a_study_of_another_encode_another_length_or_another_rate_is_never_written() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let tone = with_a_tone(&harness);
        let file = whole_file_of(&harness, "a.wav");
        let other_digest = WholeFile {
            packets: file.packets ^ 1,
            ..file
        };
        let other_length = WholeFile {
            frames: file.frames + u64::from(RATE),
            ..file
        };
        let other_rate = WholeFile {
            rate: 48_000,
            ..file
        };
        let tone_file = whole_file_of(&harness, "e.wav");

        let refused = [other_digest, other_length, other_rate]
            .map(|file| documents.take_study(&study_of(file, "fake")));
        let tone_alone = documents.take_study(&study_of(tone_file, "genuine"));

        assert_eq!(refused, [Ok(false), Ok(false), Ok(false)]);
        assert_eq!(tone_alone, Ok(true));
        assert_eq!(studied_as(&harness, tone).as_deref(), Some("genuine"));
        assert_eq!(every_studied(&harness), vec![None; harness.songs.len()]);
    }

    #[test]
    fn a_study_never_lands_over_one_at_this_judgement_but_replaces_one_of_another() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let file = whole_file_of(&harness, "a.wav");
        let [a, b, c, d] = [0, 1, 2, 3].map(|nth| harness.songs[nth].id);
        study_row(&harness, a, "genuine", Some(JUDGED_UNDER));
        study_row(&harness, b, "genuine", Some(JUDGED_UNDER + 7));
        study_row(&harness, c, "genuine", None);

        let landed = documents.take_study(&study_of(file, "fake"));

        assert_eq!(landed, Ok(true));
        assert_eq!(
            [a, b, c, d].map(|id| studied_as(&harness, id)),
            [
                Some("genuine".to_owned()),
                Some("fake".to_owned()),
                Some("genuine".to_owned()),
                Some("fake".to_owned()),
            ]
        );
    }

    #[test]
    fn a_study_this_build_cannot_read_is_settled_and_written_nowhere() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let file = whole_file_of(&harness, "a.wav");
        let unknown_verdict = study_of(file, "a verdict from later");
        let other_judgement = SharedStudy {
            judged_under: JUDGED_UNDER + 1,
            ..study_of(file, "fake")
        };
        let other_bands = SharedStudy {
            band_levels: vec![1; BANDS + 1],
            ..study_of(file, "fake")
        };
        let unknown_guess = SharedStudy {
            lossy_guess: Some("a guess from later".to_owned()),
            ..study_of(file, "fake")
        };

        let settled = [unknown_verdict, other_judgement, other_bands, unknown_guess]
            .map(|study| documents.take_study(&study));

        assert_eq!(settled, [Ok(true), Ok(true), Ok(true), Ok(true)]);
        assert_eq!(every_studied(&harness), vec![None; harness.songs.len()]);
    }

    #[test]
    fn landed_studies_are_listed_for_sending_exactly_as_they_arrived_beside_kept_words() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let file = whole_file_of(&harness, "a.wav");
        let study = study_of(file, "fake");
        let [a, b, c] = [0, 1, 2].map(|nth| harness.songs[nth].id);
        harness
            .player
            .catalog
            .keep_lyrics(a, Some(TIMED), None, "lrclib")
            .expect("kept lyrics");
        documents.take_study(&study).expect("landed");
        change(
            &harness,
            "UPDATE documents SET packets = NULL, frames = NULL WHERE id = ?1",
            c,
        );
        change(
            &harness,
            "UPDATE studies SET band_levels = NULL WHERE track = ?1",
            b,
        );

        let listed = documents.answers_after(0, 100).expect("listed");

        let studies: Vec<_> = listed
            .iter()
            .filter_map(|learnt| match &learnt.answer {
                Answer::Study(study) => Some(study.clone()),
                Answer::Lyrics(_) | Answer::Release(_) => None,
            })
            .collect();
        let words = listed
            .iter()
            .filter(|learnt| matches!(learnt.answer, Answer::Lyrics(_)))
            .count();
        assert_eq!(studies, vec![study.clone(), study]);
        assert_eq!(words, 1);
        assert!(listed.windows(2).all(|pair| pair[0].at <= pair[1].at));
        assert_eq!(documents.answers_after(0, 2).expect("a page").len(), 2);
    }

    #[test]
    fn a_study_the_phone_kept_itself_is_listed_with_the_file_it_measured() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let tone = with_a_tone(&harness);
        let at = location(&format!("{FOLDER}e.wav")).expect("a document");
        let study = resonate_analysis::study(&harness.player.sources, &at, None, &Watch::default())
            .expect("a study");
        let scanned = probe_scanned(&harness.player.sources, &at).expect("a scan");

        harness
            .player
            .catalog
            .keep_studied(tone, &study)
            .expect("kept");
        let before_it_was_known = documents.studies_to_share(0, 10).expect("listed");
        harness
            .player
            .catalog
            .keep_identity(tone, &scanned)
            .expect("identified");
        let listed = documents.studies_to_share(0, 10).expect("listed");

        assert!(before_it_was_known.is_empty());
        assert_eq!(listed.len(), 1);
        let Answer::Study(told) = &listed[0].answer else {
            panic!("a study");
        };
        assert_eq!(told.file, whole_file_of(&harness, "e.wav"));
        assert_eq!(told.verdict, study.judgement.verdict.as_str());
        assert_eq!(told.judged_under, JUDGED_UNDER);
        assert_eq!(told.band_levels.len(), BANDS);
        assert_eq!(listed[0].recording.title, "e.wav");
    }

    #[test]
    fn a_study_of_a_file_that_changed_or_kept_by_the_analysis_pane_alone_is_not_sent() {
        let harness = Harness::new();
        let documents = documents_over(&harness);
        let [a, b] = [0, 1].map(|nth| harness.songs[nth].id);
        let at = location(&format!("{FOLDER}a.wav")).expect("a document");
        let study = resonate_analysis::study(&harness.player.sources, &at, None, &Watch::default())
            .expect("a study");
        let scanned = probe_scanned(&harness.player.sources, &at).expect("a scan");
        for id in [a, b] {
            harness
                .player
                .catalog
                .keep_identity(id, &scanned)
                .expect("identified");
        }
        let report = AnalysisReport {
            track_id: b,
            verdict: study.judgement.verdict.as_str().to_owned(),
            peak: study.levels.peak,
            rms: study.levels.rms,
            true_peak: study.loudness.true_peak,
            loudness_lufs: study.loudness.integrated,
            dynamic_range: study.loudness.dynamic_range.map(u32::from),
            clipped: study.levels.clipped,
            bits_in_use: study.levels.bits_in_use.map(u32::from),
            waveform: Vec::new(),
            spectrum: Vec::new(),
            spectrum_step_hz: 0.0,
            findings: Vec::new(),
        };

        harness
            .player
            .catalog
            .keep_studied(a, &study)
            .expect("kept");
        harness
            .player
            .catalog
            .keep_study(b, &report, &BandLevels::of(&study.spectrum))
            .expect("kept by the pane");
        let listed = documents.studies_to_share(0, 10).expect("listed");
        change(
            &harness,
            "UPDATE documents SET size = 99, modified = 99 WHERE id = ?1",
            a,
        );
        let after_the_file_changed = documents.studies_to_share(0, 10).expect("listed");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].recording.title, "a.wav");
        assert!(after_the_file_changed.is_empty());
    }
}
