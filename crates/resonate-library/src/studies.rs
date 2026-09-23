use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime},
};

use ahash::AHashSet;
use parking_lot::{Condvar, Mutex};
use resonate_analysis::{Cutoff, JUDGED_UNDER, LossyGuess, Study, Verdict, Watching};
use resonate_codec::Sources;
use resonate_core::{Chromaprint, FrameSpan, MediaLocation, TrackId};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::{
    EnrichProgress, Error, Fingerprinters, Library, Mbid, RecordingMatch, Result, Sounded, StoreOp,
    TrackToAsk, enrich, reference::credited_as, store,
};

pub const HEARD_AT_LEAST: u8 = 80;

const STUDY_WORKERS_AT_LEAST: usize = 1;

const CORES_KEPT_FOR_EVERYTHING_ELSE: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Agreement {
    Agrees,
    Disagrees,
    Unnamed,
    Unheard,
}

impl Agreement {
    pub const ALL: [Self; 4] = [Self::Agrees, Self::Disagrees, Self::Unnamed, Self::Unheard];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Agrees => "agrees",
            Self::Disagrees => "disagrees",
            Self::Unnamed => "unnamed",
            Self::Unheard => "unheard",
        }
    }

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|agreement| agreement.as_str() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardAs {
    pub recording: Mbid,
    pub score: u8,
    pub title: String,
    pub artist: Option<String>,
}

impl HeardAs {
    pub fn of(found: &RecordingMatch) -> Self {
        let billed = credited_as(&found.credit);
        Self {
            recording: found.recording.clone(),
            score: found.score,
            title: found.title.clone(),
            artist: (!billed.trim().is_empty()).then_some(billed),
        }
    }

    pub(crate) fn as_a_match(&self) -> RecordingMatch {
        RecordingMatch {
            recording: self.recording.clone(),
            score: self.score,
            title: self.title.clone(),
            credit: Vec::new(),
            length: None,
            isrcs: Vec::new(),
            releases: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Studied {
    pub at: SystemTime,
    pub verdict: Verdict,
    pub cutoff: Option<Cutoff>,
    pub lossy_guess: Option<LossyGuess>,
    pub upsampled_from: Option<u32>,
    pub bits_in_use: Option<u8>,
    pub declared_bits: Option<u8>,
    pub peak: f32,
    pub rms: f32,
    pub true_peak: f32,
    pub loudness: Option<f32>,
    pub loudness_range: Option<f32>,
    pub dynamic_range: Option<u8>,
    pub clipped: u64,
    pub mono_as_stereo: bool,
    pub print: Option<Chromaprint>,
    pub recognised: Option<SystemTime>,
    pub heard_as: Option<HeardAs>,
    pub agreement: Option<Agreement>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StudiedTrack {
    pub id: TrackId,
    pub title: String,
    pub artist: Option<String>,
    pub location: MediaLocation,
    pub span: Option<FrameSpan>,
    pub studied: Studied,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StudyFilter {
    pub fakes: bool,
    pub suspects: bool,
    pub misnamed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ToStudy {
    pub(crate) id: TrackId,
    pub(crate) location: MediaLocation,
    pub(crate) span: Option<FrameSpan>,
    pub(crate) print: Option<Chromaprint>,
}

const STUDIED_COLUMNS: &str = "s.studied, s.verdict, s.cutoff_hz, s.cutoff_drop, s.lossy_guess,
     s.upsampled_from, s.bits_in_use, s.declared_bits, s.peak, s.rms, s.loudness,
     s.dynamic_range, s.clipped, s.mono_as_stereo, s.print, s.print_length, s.recognised,
     s.heard_as, s.heard_score, s.heard_title, s.heard_artist, s.agreement, s.true_peak,
     s.loudness_range";

fn studied_from(row: &Row<'_>, from: usize) -> rusqlite::Result<Studied> {
    let at = |nth: usize| from + nth;
    let verdict: String = row.get(at(1))?;
    let cutoff_hz: Option<i64> = row.get(at(2))?;
    let cutoff_drop: Option<f64> = row.get(at(3))?;
    let lossy_guess: Option<String> = row.get(at(4))?;
    let print: Option<String> = row.get(at(14))?;
    let print_length: Option<i64> = row.get(at(15))?;
    let recognised: Option<i64> = row.get(at(16))?;
    let heard_as: Option<String> = row.get(at(17))?;
    let heard_score: Option<i64> = row.get(at(18))?;
    let heard_title: Option<String> = row.get(at(19))?;
    let heard_artist: Option<String> = row.get(at(20))?;
    let agreement: Option<String> = row.get(at(21))?;

    Ok(Studied {
        at: store::from_nanos(row.get(at(0))?),
        verdict: Verdict::named(&verdict).unwrap_or(Verdict::NotJudged),
        cutoff: cutoff_hz.map(|hz| Cutoff {
            hz: hz.clamp(0, i64::from(u32::MAX)) as u32,
            drop_db: cutoff_drop.unwrap_or_default() as f32,
        }),
        lossy_guess: lossy_guess.as_deref().and_then(LossyGuess::named),
        upsampled_from: row
            .get::<_, Option<i64>>(at(5))?
            .map(|hz| hz.clamp(0, i64::from(u32::MAX)) as u32),
        bits_in_use: row.get::<_, Option<i64>>(at(6))?.map(small),
        declared_bits: row.get::<_, Option<i64>>(at(7))?.map(small),
        peak: row.get::<_, f64>(at(8))? as f32,
        rms: row.get::<_, f64>(at(9))? as f32,
        true_peak: row.get::<_, f64>(at(22))? as f32,
        loudness: row.get::<_, Option<f64>>(at(10))?.map(|lufs| lufs as f32),
        loudness_range: row.get::<_, Option<f64>>(at(23))?.map(|lu| lu as f32),
        dynamic_range: row.get::<_, Option<i64>>(at(11))?.map(small),
        clipped: row.get::<_, i64>(at(12))?.max(0) as u64,
        mono_as_stereo: row.get(at(13))?,
        print: print.and_then(|encoded| {
            let length = Duration::from_secs(print_length.unwrap_or_default().max(0) as u64);
            Chromaprint::new(&encoded, length).ok()
        }),
        recognised: recognised.map(store::from_nanos),
        heard_as: heard_as
            .as_deref()
            .and_then(|mbid| Mbid::new(mbid).ok())
            .map(|recording| HeardAs {
                recording,
                score: heard_score.map_or(0, small),
                title: heard_title.unwrap_or_default(),
                artist: heard_artist,
            }),
        agreement: agreement.as_deref().and_then(Agreement::named),
    })
}

fn small(value: i64) -> u8 {
    value.clamp(0, i64::from(u8::MAX)) as u8
}

pub(crate) fn write_study(
    tx: &Transaction<'_>,
    track: TrackId,
    study: &Study,
    now: SystemTime,
) -> Result<()> {
    let judgement = &study.judgement;
    tx.execute(
        "INSERT OR REPLACE INTO track_studies (
             track_id, studied, studied_under, verdict, cutoff_hz, cutoff_drop, lossy_guess,
             upsampled_from, bits_in_use, declared_bits, peak, rms, loudness, dynamic_range,
             clipped, mono_as_stereo, print, print_length, true_peak, loudness_range
         ) VALUES (
             ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19,
             ?20
         )",
        params![
            track.get() as i64,
            store::to_nanos(now),
            i64::from(JUDGED_UNDER),
            judgement.verdict.as_str(),
            judgement.cutoff.map(|cutoff| i64::from(cutoff.hz)),
            judgement.cutoff.map(|cutoff| f64::from(cutoff.drop_db)),
            judgement.lossy_guess().map(LossyGuess::as_str),
            judgement.upsampled_from().map(i64::from),
            study.levels.bits_in_use.map(i64::from),
            study.examined.declared_bits.map(i64::from),
            f64::from(study.levels.peak),
            f64::from(study.levels.rms),
            study.loudness.integrated.map(f64::from),
            study.loudness.dynamic_range.map(i64::from),
            study.levels.clipped as i64,
            study.levels.stereo.is_some_and(|stereo| stereo.identical),
            study.print.as_ref().map(Chromaprint::encoded),
            study
                .print
                .as_ref()
                .map(|print| print.length().as_secs() as i64),
            f64::from(study.loudness.true_peak),
            study.loudness.range.map(f64::from),
        ],
    )
    .map_err(|source| Error::store(StoreOp::Insert, source))?;
    Ok(())
}

pub(crate) fn write_recognition(
    tx: &Transaction<'_>,
    track: TrackId,
    heard: Option<&HeardAs>,
    agreement: Agreement,
    now: SystemTime,
) -> Result<bool> {
    let changed = tx
        .execute(
            "UPDATE track_studies SET
                 recognised   = ?1,
                 heard_as     = ?2,
                 heard_score  = ?3,
                 heard_title  = ?4,
                 heard_artist = ?5,
                 agreement    = ?6
             WHERE track_id = ?7",
            params![
                store::to_nanos(now),
                heard.map(|heard| heard.recording.as_str()),
                heard.map(|heard| i64::from(heard.score)),
                heard.map(|heard| heard.title.as_str()),
                heard.and_then(|heard| heard.artist.as_deref()),
                agreement.as_str(),
                track.get() as i64,
            ],
        )
        .map_err(|source| Error::store(StoreOp::Update, source))?;
    Ok(changed > 0)
}

pub(crate) fn study_of(
    connection: &Connection,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<Option<(TrackId, Studied)>> {
    let Some(path) = location.as_path() else {
        return Ok(None);
    };
    let path = store::path_text(path)?;
    let (span_start, _) = store::span_columns(span);
    connection
        .query_row(
            &format!(
                "SELECT t.id, {STUDIED_COLUMNS}
                   FROM tracks t JOIN track_studies s ON s.track_id = t.id
                  WHERE t.path = ?1 AND t.span_start = ?2"
            ),
            params![path, span_start],
            |row| Ok((row.get::<_, i64>(0)?, studied_from(row, 1)?)),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .map(|(id, studied)| Ok((TrackId::new(id as u64)?, studied)))
        .transpose()
}

pub(crate) fn album_loudness(connection: &Connection, track: TrackId) -> Result<Option<f64>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT t.duration, t.sample_rate, s.loudness
               FROM tracks t LEFT JOIN track_studies s ON s.track_id = t.id
              WHERE t.album_id = (SELECT album_id FROM tracks WHERE id = ?1)
                AND t.alternative_of IS NULL",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![track.get() as i64], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<f64>>(2)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    let mut energy = 0.0;
    let mut seconds = 0.0;
    for (frames, rate, loudness) in rows {
        let (Some(frames), Some(loudness)) = (frames.filter(|frames| *frames > 0), loudness) else {
            return Ok(None);
        };
        if rate <= 0 {
            return Ok(None);
        }
        let lasting = frames as f64 / rate as f64;
        energy += lasting * 10_f64.powf(loudness / 10.0);
        seconds += lasting;
    }

    Ok((seconds > 0.0).then(|| 10.0 * (energy / seconds).log10()))
}

pub(crate) fn track_held(
    connection: &Connection,
    location: &MediaLocation,
    span: Option<FrameSpan>,
) -> Result<Option<TrackId>> {
    let Some(path) = location.as_path() else {
        return Ok(None);
    };
    let path = store::path_text(path)?;
    let (span_start, _) = store::span_columns(span);
    connection
        .query_row(
            "SELECT id FROM tracks WHERE path = ?1 AND span_start = ?2",
            params![path, span_start],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|source| Error::store(StoreOp::Query, source))?
        .map(|id| Ok(TrackId::new(id as u64)?))
        .transpose()
}

pub(crate) fn studied(connection: &Connection, filter: StudyFilter) -> Result<Vec<StudiedTrack>> {
    let mut narrowed = Vec::new();
    if filter.fakes {
        narrowed.push(format!("s.verdict = '{}'", Verdict::Fake.as_str()));
    }
    if filter.suspects {
        narrowed.push(format!("s.verdict = '{}'", Verdict::Suspect.as_str()));
    }
    if filter.misnamed {
        narrowed.push(format!("s.agreement = '{}'", Agreement::Disagrees.as_str()));
    }
    let narrowing = if narrowed.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", narrowed.join(" OR "))
    };

    let mut statement = connection
        .prepare(&format!(
            "SELECT t.id, t.title, t.artist, t.path, t.span_start, t.span_frames, {STUDIED_COLUMNS}
               FROM tracks t JOIN track_studies s ON s.track_id = t.id
               {narrowing}
              ORDER BY t.artist COLLATE NOCASE, t.album_id, t.disc_number, t.track_number, t.id"
        ))
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, Option<i64>>(5)?,
                studied_from(row, 6)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    rows.into_iter()
        .map(
            |(id, title, artist, path, span_start, span_frames, studied)| {
                Ok(StudiedTrack {
                    id: TrackId::new(id as u64)?,
                    title,
                    artist,
                    location: MediaLocation::local(path),
                    span: store::span(span_start, span_frames),
                    studied,
                })
            },
        )
        .collect()
}

pub(crate) fn to_study(connection: &Connection, again: bool) -> Result<Vec<ToStudy>> {
    let mut statement = connection
        .prepare(
            "SELECT t.id, t.path, t.span_start, t.span_frames,
                    s.track_id IS NOT NULL AND s.studied_under = ?1, s.print, s.print_length
               FROM tracks t LEFT JOIN track_studies s ON s.track_id = t.id
              WHERE s.track_id IS NULL
                 OR s.studied_under != ?1
                 OR (s.print IS NOT NULL AND (s.recognised IS NULL OR ?2))
              ORDER BY t.id",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let rows = statement
        .query_map(params![i64::from(JUDGED_UNDER), again], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<i64>>(3)?,
                row.get::<_, bool>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
            ))
        })
        .and_then(Iterator::collect::<rusqlite::Result<Vec<_>>>)
        .map_err(|source| Error::store(StoreOp::Query, source))?;

    rows.into_iter()
        .map(
            |(id, path, span_start, span_frames, current, print, print_length)| {
                let print = current
                    .then(|| {
                        let length =
                            Duration::from_secs(print_length.unwrap_or_default().max(0) as u64);
                        print.and_then(|encoded| Chromaprint::new(&encoded, length).ok())
                    })
                    .flatten();
                Ok(ToStudy {
                    id: TrackId::new(id as u64)?,
                    location: MediaLocation::local(path),
                    span: store::span(span_start, span_frames),
                    print,
                })
            },
        )
        .collect()
}

#[derive(Debug, Default)]
struct Claimed {
    ever: AHashSet<TrackId>,
    busy: AHashSet<TrackId>,
}

#[derive(Debug, Default)]
pub(crate) struct Claims {
    held: Mutex<Claimed>,
    settled: Condvar,
}

pub(crate) struct Claim<'a> {
    claims: &'a Claims,
    track: TrackId,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.claims.held.lock().busy.remove(&self.track);
        self.claims.settled.notify_all();
    }
}

impl Claims {
    pub(crate) fn claim(&self, track: TrackId) -> Option<Claim<'_>> {
        let mut held = self.held.lock();
        if !held.ever.insert(track) {
            return None;
        }
        held.busy.insert(track);
        Some(Claim {
            claims: self,
            track,
        })
    }

    pub(crate) fn wait_for(&self, track: TrackId) {
        let mut held = self.held.lock();
        while held.busy.contains(&track) {
            self.settled.wait(&mut held);
        }
    }
}

impl Watching for EnrichProgress {
    fn stopped(&self) -> bool {
        self.is_cancelled()
    }
}

pub(crate) struct Studies {
    workers: Vec<JoinHandle<()>>,
}

impl Studies {
    pub(crate) fn start(
        library: &Library,
        fingerprinters: &Arc<Fingerprinters>,
        progress: &Arc<EnrichProgress>,
        claims: &Arc<Claims>,
        asked: Vec<ToStudy>,
    ) -> Self {
        let recognising = fingerprinters.has_a_source();
        let asked: Arc<[ToStudy]> = asked
            .into_iter()
            .filter(|asked| asked.print.is_none() || recognising)
            .collect();
        let next = Arc::new(AtomicUsize::new(0));
        let workers = (0..workers_for(asked.len()))
            .filter_map(|nth| {
                let library = library.shared();
                let fingerprinters = Arc::clone(fingerprinters);
                let progress = Arc::clone(progress);
                let asked = Arc::clone(&asked);
                let next = Arc::clone(&next);
                let claims = Arc::clone(claims);
                thread::Builder::new()
                    .name(format!("resonate-study-{nth}"))
                    .spawn(move || {
                        let sources = library.sources();
                        while let Some(one) = asked.get(next.fetch_add(1, Ordering::Relaxed)) {
                            if progress.is_cancelled() {
                                return;
                            }
                            if let Some(_claim) = claims.claim(one.id) {
                                study_one(&library, &sources, &fingerprinters, &progress, one);
                            }
                        }
                    })
                    .map_err(|error| {
                        tracing::warn!(%error, "a study worker did not start");
                    })
                    .ok()
            })
            .collect();

        Self { workers }
    }

    pub(crate) fn rest(self) {
        for worker in self.workers {
            let _ = worker.join();
        }
    }
}

fn workers_for(asked: usize) -> usize {
    let cores = thread::available_parallelism().map_or(STUDY_WORKERS_AT_LEAST, usize::from);
    (cores / CORES_KEPT_FOR_EVERYTHING_ELSE)
        .max(STUDY_WORKERS_AT_LEAST)
        .min(asked)
}

fn study_one(
    library: &Library,
    sources: &Sources,
    fingerprinters: &Fingerprinters,
    progress: &EnrichProgress,
    asked: &ToStudy,
) {
    let print = match &asked.print {
        Some(print) => Some(print.clone()),
        None => match studied_now(library, sources, progress, asked) {
            Some(print) => print,
            None => return,
        },
    };
    let Some(print) = print else {
        return;
    };
    if fingerprinters.has_a_source() && !progress.is_cancelled() {
        recognised_and_noted(library, fingerprinters, progress, asked.id, print);
    }
}

fn studied_now(
    library: &Library,
    sources: &Sources,
    progress: &EnrichProgress,
    asked: &ToStudy,
) -> Option<Option<Chromaprint>> {
    let study = match resonate_analysis::study(sources, &asked.location, asked.span, progress) {
        Ok(study) => study,
        Err(resonate_analysis::Error::Stopped) => return None,
        Err(error) => {
            tracing::debug!(%error, location = %asked.location, "a track could not be studied");
            return None;
        }
    };
    if let Err(error) = library.note_study(asked.id, &study) {
        tracing::warn!(%error, track = %asked.id, "a study was not kept");
        return None;
    }
    progress.note_studied(study.judgement.verdict);
    Some(study.print)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    pub matches: Vec<RecordingMatch>,
    pub agreement: Agreement,
    pub refused: bool,
}

pub(crate) fn heard_and_noted(
    library: &Library,
    fingerprinters: &Fingerprinters,
    id: TrackId,
    print: Chromaprint,
) -> Option<Heard> {
    let track = match library.track_as_heard(id) {
        Ok(Some(track)) => track,
        Ok(None) => return None,
        Err(error) => {
            tracing::warn!(%error, track = %id, "a studied track could not be read back");
            return None;
        }
    };
    let recognition = fingerprinters.recognise(&sounded(&track, print));
    if recognition.refused && recognition.matches.is_empty() {
        return Some(Heard {
            matches: Vec::new(),
            agreement: Agreement::Unheard,
            refused: true,
        });
    }

    let agreement = enrich::agreement(&recognition.matches, &track);
    let heard = enrich::top_heard(&recognition.matches).map(HeardAs::of);
    if let Err(error) = library.note_recognition(id, heard.as_ref(), agreement) {
        tracing::warn!(%error, track = %id, "a recognition was not kept");
    }
    Some(Heard {
        matches: recognition.matches,
        agreement,
        refused: false,
    })
}

pub(crate) fn recognised_and_noted(
    library: &Library,
    fingerprinters: &Fingerprinters,
    progress: &EnrichProgress,
    id: TrackId,
    print: Chromaprint,
) -> Option<Vec<RecordingMatch>> {
    let heard = heard_and_noted(library, fingerprinters, id, print)?;
    if heard.refused {
        progress.refuse();
        return None;
    }
    progress.note_recognised(heard.agreement);
    Some(heard.matches)
}

pub(crate) fn sounded(track: &TrackToAsk, print: Chromaprint) -> Sounded {
    Sounded {
        location: track.location.clone(),
        span: track.span,
        length: track.length,
        title: track.tagged_title.clone(),
        artist: track.tagged_artist.clone(),
        print,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_agreement_reads_back_the_name_it_was_written_as() {
        for agreement in Agreement::ALL {
            assert_eq!(Agreement::named(agreement.as_str()), Some(agreement));
        }
        assert_eq!(Agreement::named("perhaps"), None);
    }

    const STUDIED_COLUMN_COUNT: usize = 24;

    #[test]
    fn the_columns_read_are_the_columns_named() {
        assert_eq!(STUDIED_COLUMNS.split(',').count(), STUDIED_COLUMN_COUNT);
    }
}
