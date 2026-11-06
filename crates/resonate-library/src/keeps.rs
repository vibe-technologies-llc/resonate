use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use resonate_core::{Link, MediaLocation, SourceId, TrackId};
use resonate_providers::{
    Answer, Asking, Away, Identity, Providers, TURNED_TO_APART, WAITED_ON_AFTER_AN_OFFER,
};
use rusqlite::{Connection, OptionalExtension as _, Row as SqlRow, Transaction, params};

use crate::{
    ANSWERS_WITHIN, Cut, DeliveryFolder, Error, Library, PollOptions, PollProgress, Result,
    StoreOp, TagEdit, TagField,
    filed::{self, AlbumToFile, ToFile, Unfiled},
    organise,
    pass::PassKind,
    playlist, store,
    supply::{self, Drawing, Drawn, HEEDED_EVERY, Unopened},
    synced::{self, HeldRow},
    volumes,
};

const WATCHED_AT: &str = "https://music.youtube.com/watch?v=";

const HEARD_THERE: &str = "streaming";

const KEEPS_QUEUED: &str = "SELECT id, source, video, title, artist, album, length, playlist
       FROM kept_songs WHERE landed IS NULL AND gone = 0 ORDER BY asked, id";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SongToKeep {
    pub source: SourceId,
    pub video: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub length: Option<Duration>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaylistToKeep {
    pub name: String,
    pub ordered: bool,
    pub songs: Vec<SongToKeep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kept {
    Queued,
    AlreadyHeld(TrackId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeptList {
    pub queued: usize,
    pub held: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeepSummary {
    pub landed: usize,
    pub gone: usize,
    pub unkept: usize,
    pub stopped_by: Option<SourceId>,
}

impl SongToKeep {
    fn identity(&self) -> Identity {
        Identity {
            artist: self.artist.clone(),
            album: self.album.clone(),
            length: self.length,
            links: vec![Link::new(
                HEARD_THERE,
                format!("{WATCHED_AT}{}", self.video),
            )],
            ..Identity::named(self.title.as_str())
        }
    }

    fn album(&self) -> Option<AlbumToFile> {
        let title = self.album.as_deref()?.trim();
        (!title.is_empty()).then(|| AlbumToFile {
            title: title.to_owned(),
            owner: None,
            year: None,
            discs: 1,
        })
    }

    fn tags(&self) -> Vec<TagEdit> {
        [
            (TagField::Title, Some(self.title.as_str())),
            (TagField::Artist, self.artist.as_deref()),
            (TagField::Album, self.album.as_deref()),
        ]
        .into_iter()
        .filter_map(|(field, value)| {
            let value = value?.trim();
            (!value.is_empty()).then(|| TagEdit {
                field,
                value: value.to_owned(),
            })
        })
        .collect()
    }

    fn to_file<'a>(&'a self, album: Option<&'a AlbumToFile>) -> ToFile<'a> {
        ToFile {
            title: &self.title,
            artist: self.artist.as_deref(),
            album,
            disc: None,
            position: None,
            length: self.length,
            tags: self.tags(),
        }
    }
}

struct HeldCopy {
    track: Option<TrackId>,
    cut: Cut,
}

struct Queued {
    id: i64,
    song: SongToKeep,
    playlist: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Standing {
    Landed,
    Queued,
    GivenUp,
}

struct Pressed<'a> {
    song: &'a SongToKeep,
    place: Option<i64>,
    landed: Option<String>,
}

enum Turn {
    Landed,
    Gone,
    Unkept,
    Stopped,
    Over,
}

impl Library {
    pub fn keep_song(&self, song: SongToKeep) -> Result<Kept> {
        self.a_folder_to_keep_into()?;
        match self.copy_of(&song)? {
            Some(HeldCopy {
                track: Some(track), ..
            }) => Ok(Kept::AlreadyHeld(track)),
            Some(_) => Ok(Kept::Queued),
            None => {
                self.inner()
                    .write(|transaction| queued_alone(transaction, &song))?;
                Ok(Kept::Queued)
            }
        }
    }

    pub fn keep_playlist(&self, list: PlaylistToKeep) -> Result<KeptList> {
        self.a_folder_to_keep_into()?;
        let name = self
            .inner()
            .read(|connection| playlist::name_a_list_may_hold(connection, &list.name))?;
        let name = name.as_str();
        let standing = self
            .inner()
            .read(|connection| standing_in(connection, name))?;

        let mut seen = AHashSet::new();
        let mut kept = KeptList::default();
        let mut pressed = Vec::with_capacity(list.songs.len());
        let mut to_place = Vec::new();
        for (place, song) in list.songs.iter().enumerate() {
            if !seen.insert((song.source.as_str(), song.video.as_str())) {
                continue;
            }
            let place = list.ordered.then_some(place as i64);
            let stands = standing
                .get(&(song.source.as_str().to_owned(), song.video.clone()))
                .copied();
            let copy = match stands {
                Some(Standing::Landed | Standing::Queued) => None,
                Some(Standing::GivenUp) | None => self.copy_of(song)?,
            };
            let mut landed = None;
            match (stands, copy) {
                (Some(Standing::Landed), _) => kept.held += 1,
                (_, Some(copy)) => {
                    kept.held += 1;
                    landed = Some(playlist::local_path(&copy.cut.location)?.into_owned());
                    to_place.push((copy.cut, place));
                }
                (_, None) => kept.queued += 1,
            }
            pressed.push(Pressed {
                song,
                place,
                landed,
            });
        }

        self.inner().write(|transaction| {
            pressed
                .iter()
                .try_for_each(|pressed| queued_in(transaction, name, pressed))
        })?;
        if !to_place.is_empty() {
            self.placed(name, &to_place)?;
        }
        Ok(kept)
    }

    pub fn keeps_pending(&self) -> Result<u64> {
        self.inner().read(|connection| {
            connection
                .query_row(
                    "SELECT count(*) FROM kept_songs WHERE landed IS NULL AND gone = 0",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map(i64::cast_unsigned)
                .map_err(|source| Error::store(StoreOp::Query, source))
        })
    }

    pub fn keep_pending(
        &self,
        providers: &Providers,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<KeepSummary> {
        let _asking = self.asking_alone(PassKind::Poll)?;
        organise::sweep_what_a_dead_writer_staged(self);

        let queued = self.keeps_of(providers)?;
        if queued.is_empty() {
            return Ok(KeepSummary::default());
        }
        let into = self.a_folder_to_keep_into()?;

        let progress = PollProgress::default();
        if cancelled() {
            progress.cancel();
        }
        let over = AtomicBool::new(false);
        thread::scope(|scope| {
            thread::Builder::new()
                .name("resonate-keep-heeding".to_owned())
                .spawn_scoped(scope, || heeding(cancelled, &progress, &over))
                .map_err(|source| Error::ThreadSpawn { source })?;
            let walked = self.walked(queued, &into, providers, &progress);
            over.store(true, Ordering::Relaxed);
            walked
        })
    }

    fn a_folder_to_keep_into(&self) -> Result<DeliveryFolder> {
        self.delivering_into().ok_or(Error::NoDeliveryFolder)
    }

    fn keeps_of(&self, providers: &Providers) -> Result<Vec<Queued>> {
        let queued = self.inner().read(keeps_queued)?;
        Ok(queued
            .into_iter()
            .filter(|row| providers.only(&row.song.source).has_a_source())
            .collect())
    }

    fn walked(
        &self,
        queued: Vec<Queued>,
        into: &DeliveryFolder,
        providers: &Providers,
        progress: &PollProgress,
    ) -> Result<KeepSummary> {
        let gate = self.download_gate();
        let mut away = Away::default();
        let mut summary = KeepSummary::default();
        for row in queued {
            progress.waits_out_the_meter(&gate);
            if progress.is_cancelled() {
                break;
            }
            match self.turn(&row, into, providers, progress, &mut away)? {
                Turn::Landed => summary.landed += 1,
                Turn::Gone => summary.gone += 1,
                Turn::Unkept => summary.unkept += 1,
                Turn::Stopped => {
                    summary.stopped_by = Some(row.song.source);
                    break;
                }
                Turn::Over => break,
            }
        }
        if summary.landed > 0 {
            self.restate_the_statistics();
        }
        Ok(summary)
    }

    fn turn(
        &self,
        row: &Queued,
        into: &DeliveryFolder,
        providers: &Providers,
        progress: &PollProgress,
        away: &mut Away,
    ) -> Result<Turn> {
        if let Some(copy) = self.copy_of(&row.song)? {
            self.settled_by(row, &copy.cut)?;
            return Ok(Turn::Landed);
        }

        let answer = asked(&row.song, providers, progress, away);
        if answer.cancelled {
            return Ok(Turn::Over);
        }
        let Some(delivered) = answer.delivered else {
            return self.unanswered(row, &answer, away);
        };

        let gate = self.download_gate();
        progress.waits_out_the_meter(&gate);
        if progress.is_cancelled() {
            return Ok(Turn::Over);
        }
        let album = row.song.album();
        let song = row.song.to_file(album.as_ref());
        let drawn = supply::drawn(
            delivered.delivery,
            &Drawing {
                progress,
                want: None,
                options: PollOptions::default(),
                gate,
            },
            |reader, extension| filed::filed(self, into, &song, reader, extension),
        )?;

        let video = &row.song.video;
        match drawn {
            Drawn::Read(Ok(filed)) => {
                let cut = Cut::whole(MediaLocation::local(&filed.path));
                let settled = self.settled_by(row, &cut);
                synced::scanned_once_free(self, filed.root);
                settled.map(|()| Turn::Landed)
            }
            Drawn::Read(Err(_)) if progress.is_cancelled() => Ok(Turn::Over),
            Drawn::Read(Err(Unfiled::Catalog(error))) => Err(error),
            Drawn::Read(Err(Unfiled::NoFolder)) => Err(Error::NoDeliveryFolder),
            Drawn::Read(Err(
                refused @ (Unfiled::NotAsLongAsWanted { .. }
                | Unfiled::TooLarge
                | Unfiled::Undecodable),
            )) => {
                tracing::warn!(video, ?refused, "the bytes of a kept song were refused");
                self.given_up(row)?;
                Ok(Turn::Unkept)
            }
            Drawn::Read(Err(unfiled)) => {
                tracing::warn!(video, ?unfiled, "a kept song could not be filed");
                Ok(Turn::Unkept)
            }
            Drawn::Unopened(Unopened::Gone) => {
                tracing::info!(video, "a kept song's video was gone when it was opened");
                self.given_up(row)?;
                Ok(Turn::Gone)
            }
            Drawn::Unopened(Unopened::Refused(error)) => {
                tracing::warn!(%error, cause = ?error.cause(), video, "a kept song could not be opened");
                if error.is_the_provider_away() {
                    away.note(&row.song.source);
                    return Ok(Turn::Stopped);
                }
                Ok(Turn::Unkept)
            }
            Drawn::Stalled => {
                tracing::warn!(
                    video,
                    "a kept song stopped sending and waits for the next pass"
                );
                Ok(Turn::Unkept)
            }
        }
    }

    fn unanswered(&self, row: &Queued, answer: &Answer, away: &Away) -> Result<Turn> {
        if away.holds(&row.song.source) {
            return Ok(Turn::Stopped);
        }
        if answer.refused + answer.late + answer.passed_over == 0 {
            self.given_up(row)?;
        }
        Ok(Turn::Unkept)
    }

    fn copy_of(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        match self.held_alike(song)? {
            Some(held) => Ok(Some(held)),
            None => self.landed_before(song),
        }
    }

    fn held_alike(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        let identity = song.identity();
        let (Some(lead), Some(length)) = (identity.lead_artist(), song.length) else {
            return Ok(None);
        };
        let copies = self
            .inner()
            .read(|connection| store::held_named_alike(connection, &song.title, lead, length))?;
        if copies.is_empty() {
            return Ok(None);
        }
        let vault = self.vault().map(AsRef::as_ref);
        let away = self.inner().read(volumes::absent)?;

        Ok(copies
            .into_iter()
            .find(|copy| {
                copy.held.stands(vault) || volumes::is_on_an_absent_one(&copy.held.path, &away)
            })
            .map(|copy| HeldCopy {
                track: Some(copy.track),
                cut: Cut {
                    location: MediaLocation::local(copy.held.path),
                    span: copy.span,
                },
            }))
    }

    fn landed_before(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        let landed = self
            .inner()
            .read(|connection| landed_as(connection, song))?;
        let Some(held) = landed.into_iter().find(|held| held.stands(None)) else {
            return Ok(None);
        };

        Ok(Some(HeldCopy {
            track: self.track_at(&held.path, None)?.map(|track| track.id),
            cut: Cut::whole(MediaLocation::local(held.path)),
        }))
    }

    fn settled_by(&self, row: &Queued, cut: &Cut) -> Result<()> {
        let path = playlist::local_path(&cut.location)?.into_owned();
        let placed = match &row.playlist {
            Some(list) => self
                .inner()
                .read(|connection| place_of(connection, row.id))
                .and_then(|place| self.placed(list, &[(cut.clone(), place)])),
            None => Ok(()),
        };
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "UPDATE kept_songs SET landed = ?2, gone = 0 WHERE id = ?1",
                    params![row.id, path],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })?;
        placed
    }

    fn given_up(&self, row: &Queued) -> Result<()> {
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "UPDATE kept_songs SET gone = 1 WHERE id = ?1",
                    params![row.id],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    fn placed(&self, list: &str, cuts: &[(Cut, Option<i64>)]) -> Result<()> {
        let plain: Vec<Cut> = cuts.iter().map(|(cut, _)| cut.clone()).collect();
        let filled = playlist::fill(self.inner(), list, &plain)?;
        if filled.added == 0 {
            return Ok(());
        }
        for (cut, place) in cuts {
            if let Some(place) = place {
                self.seated(filled.playlist, list, cut, *place)?;
            }
        }
        Ok(())
    }

    fn seated(
        &self,
        id: resonate_core::PlaylistId,
        list: &str,
        cut: &Cut,
        place: i64,
    ) -> Result<()> {
        let wanted = playlist::Row::of(cut)?;
        let (rows, places, kept_in_an_order) = self.inner().read(|connection| {
            Ok((
                playlist::rows(connection, id)?,
                places_in(connection, list)?,
                playlist::kept_in(connection, id)?.is_some(),
            ))
        })?;
        if kept_in_an_order {
            return Ok(());
        }
        let Some(at) = rows.iter().position(|row| *row == wanted) else {
            return Ok(());
        };
        let ahead_of_it = rows[..at]
            .iter()
            .position(|row| places.get(&row.path).is_some_and(|other| *other > place));
        if let Some(to) = ahead_of_it {
            playlist::move_rows(self.inner(), id, resonate_core::Span::between(at, at), to)?;
        }
        Ok(())
    }
}

fn heeding(cancelled: &(dyn Fn() -> bool + Sync), progress: &PollProgress, over: &AtomicBool) {
    while !over.load(Ordering::Relaxed) {
        if cancelled() {
            progress.cancel();
            return;
        }
        thread::sleep(HEEDED_EVERY);
    }
}

fn asked(
    song: &SongToKeep,
    providers: &Providers,
    progress: &PollProgress,
    away: &mut Away,
) -> Answer {
    let cancelled = || progress.is_cancelled();
    providers.only(&song.source).first(
        &song.identity(),
        &Asking {
            within: ANSWERS_WITHIN,
            cancelled: &cancelled,
            turning_to: &|_| {},
            declined: &|_| false,
            passing: &[],
            apart: TURNED_TO_APART,
            grace: WAITED_ON_AFTER_AN_OFFER,
            earlier_rounds_said_no: true,
        },
        away,
    )
}

fn length_column(song: &SongToKeep) -> Option<i64> {
    song.length
        .map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX))
}

fn queued_alone(transaction: &Transaction<'_>, song: &SongToKeep) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO kept_songs (source, video, title, artist, album, length, asked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (source, video) WHERE playlist IS NULL
             DO UPDATE SET gone = 0, landed = NULL",
            params![
                song.source.as_str(),
                song.video,
                song.title,
                song.artist,
                song.album,
                length_column(song),
                store::to_nanos(SystemTime::now()),
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn queued_in(transaction: &Transaction<'_>, list: &str, pressed: &Pressed<'_>) -> Result<()> {
    let song = pressed.song;
    transaction
        .execute(
            "INSERT INTO kept_songs
                 (source, video, title, artist, album, length, playlist, place, asked, landed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT (source, video, playlist)
             DO UPDATE SET place = excluded.place, gone = 0,
                           landed = coalesce(excluded.landed, landed)",
            params![
                song.source.as_str(),
                song.video,
                song.title,
                song.artist,
                song.album,
                length_column(song),
                list,
                pressed.place,
                store::to_nanos(SystemTime::now()),
                pressed.landed,
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn standing_in(
    connection: &Connection,
    list: &str,
) -> Result<AHashMap<(String, String), Standing>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT source, video, landed IS NOT NULL, gone FROM kept_songs WHERE playlist = ?1",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![list], |row| {
            let standing = match (row.get::<_, bool>(2)?, row.get::<_, bool>(3)?) {
                (true, _) => Standing::Landed,
                (false, true) => Standing::GivenUp,
                (false, false) => Standing::Queued,
            };
            Ok(((row.get(0)?, row.get(1)?), standing))
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<_>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn keeps_queued(connection: &Connection) -> Result<Vec<Queued>> {
    let mut statement = connection
        .prepare_cached(KEEPS_QUEUED)
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    let read = statement
        .query_map([], queued_at)
        .and_then(|rows| rows.collect::<rusqlite::Result<Vec<_>>>())
        .map_err(|source| Error::store(StoreOp::Query, source))?;
    Ok(read.into_iter().flatten().collect())
}

fn queued_at(row: &SqlRow<'_>) -> rusqlite::Result<Option<Queued>> {
    let named: String = row.get(1)?;
    let Ok(source) = SourceId::new(&named) else {
        tracing::warn!(
            source = named,
            "a kept song names a source that reads as none"
        );
        return Ok(None);
    };
    Ok(Some(Queued {
        id: row.get(0)?,
        song: SongToKeep {
            source,
            video: row.get(2)?,
            title: row.get(3)?,
            artist: row.get(4)?,
            album: row.get(5)?,
            length: row
                .get::<_, Option<i64>>(6)?
                .map(|length| Duration::from_millis(length.cast_unsigned())),
        },
        playlist: row.get(7)?,
    }))
}

fn landed_as(connection: &Connection, song: &SongToKeep) -> Result<Vec<HeldRow>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT DISTINCT landed FROM kept_songs
              WHERE source = ?1 AND video = ?2 AND landed IS NOT NULL",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![song.source.as_str(), song.video], |row| {
            Ok(HeldRow {
                path: store::path_of(&row.get::<_, String>(0)?),
                within_the_vault: None,
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<_>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn place_of(connection: &Connection, id: i64) -> Result<Option<i64>> {
    connection
        .query_row(
            "SELECT place FROM kept_songs WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()
        .map(Option::flatten)
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn places_in(connection: &Connection, list: &str) -> Result<AHashMap<String, i64>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT landed, min(place) FROM kept_songs
              WHERE playlist = ?1 AND landed IS NOT NULL AND place IS NOT NULL
              GROUP BY landed",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![list], |row| Ok((row.get(0)?, row.get(1)?)))
        .and_then(|rows| rows.collect::<rusqlite::Result<_>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}
