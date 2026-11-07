use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime},
};

use ahash::{AHashMap, AHashSet};
use resonate_core::{Gate, Link, Locator, MediaLocation, SourceId, TrackId};
use resonate_providers::{
    ANSWERED_ALONE_WITHIN, Answer, Asking, Away, Delivered, Delivery, Identity, Providers,
    TURNED_TO_APART, WAITED_ON_AFTER_AN_OFFER, WhyAway,
};
use rusqlite::{Connection, OptionalExtension as _, Row as SqlRow, Transaction, params};

use crate::{
    Cut, DeliveryFolder, Error, Library, PollOptions, PollProgress, Result, StoreOp, TagEdit,
    TagField, Want,
    filed::{self, AlbumToFile, ToFile, Unfiled},
    organise,
    pass::PassKind,
    playlist,
    store::{self, HeldAlike},
    supply::{self, Drawing, Drawn, HEEDED_EVERY, Unopened},
    synced::{self, HeldRow},
    volumes,
};

const WATCHED_AT: &str = "https://music.youtube.com/watch?v=";

const HEARD_THERE: &str = "streaming";

const A_SONG_IS_KEYED: &str = "track/";

const KEEP_TRIES_AT_MOST: i64 = 5;

const KEEPS_QUEUED: &str = "SELECT id, source, video, title, artist, album, length, playlist
       FROM kept_songs WHERE landed IS NULL AND gone = 0 ORDER BY tried, asked, id";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeepStopped {
    pub why: WhyAway,
    pub song: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeepSummary {
    pub landed: usize,
    pub gone: usize,
    pub unkept: usize,
    pub stopped_by: Option<SourceId>,
    pub stopped: Option<KeepStopped>,
    pub yielded: bool,
}

impl SongToKeep {
    fn taken_from(&self) -> MediaLocation {
        MediaLocation::new(
            self.source.clone(),
            format!("{A_SONG_IS_KEYED}{}", self.video),
        )
    }

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
        let named = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let (title, owner) = (named(&self.album), named(&self.artist));
        (title.is_some() || owner.is_some()).then(|| AlbumToFile {
            title: title.unwrap_or_default(),
            owner,
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

impl HeldCopy {
    fn of(copy: HeldAlike) -> Self {
        Self {
            track: Some(copy.track),
            cut: Cut {
                location: MediaLocation::local(copy.held.path),
                span: copy.span,
            },
        }
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum WhatLanded {
    Stays,
    IsForgotten,
}

struct Pressed<'a> {
    song: &'a SongToKeep,
    place: Option<i64>,
    what_landed: WhatLanded,
}

enum Turn {
    Landed,
    Gone,
    Unkept,
    Failed,
    Mend,
    Stopped,
    Yielded,
    Over,
}

struct Walking<'a> {
    into: &'a DeliveryFolder,
    providers: &'a Providers,
    progress: &'a PollProgress,
    away: Away,
    unread: Vec<PathBuf>,
}

struct Heeded<'a>(&'a AtomicBool);

impl Drop for Heeded<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

struct KeepUnderWay<'a>(&'a Library);

impl<'a> KeepUnderWay<'a> {
    fn of(library: &'a Library) -> Self {
        library.begins_keeping();
        Self(library)
    }
}

impl Drop for KeepUnderWay<'_> {
    fn drop(&mut self) {
        self.0.ends_keeping();
    }
}

impl Library {
    pub fn keep_song(&self, song: SongToKeep) -> Result<Kept> {
        self.a_folder_to_keep_into()?;
        refused_untitled(&song)?;
        let copy = self.copy_of(&song)?;
        if let Some(HeldCopy {
            track: Some(track), ..
        }) = copy
        {
            return Ok(Kept::AlreadyHeld(track));
        }
        let what_landed = match &copy {
            Some(copy) => {
                self.root_of_an_unread_copy(copy)?;
                WhatLanded::Stays
            }
            None => WhatLanded::IsForgotten,
        };
        self.inner()
            .write(|transaction| queued_alone(transaction, &song, what_landed))?;
        Ok(Kept::Queued)
    }

    pub fn keep_playlist(&self, list: PlaylistToKeep) -> Result<KeptList> {
        self.a_folder_to_keep_into()?;
        list.songs.iter().try_for_each(refused_untitled)?;
        let name = self
            .inner()
            .read(|connection| playlist::name_a_list_may_hold(connection, &list.name))?;
        let name = name.as_str();
        let standing = self
            .inner()
            .read(|connection| standing_in(connection, name))?;
        let the_list_stands = self.playlist_named(name)?.is_some();

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
                Some(Standing::Queued) => None,
                Some(Standing::Landed) if the_list_stands => None,
                Some(Standing::Landed | Standing::GivenUp) | None => self.copy_of(song)?,
            };
            let mut what_landed = WhatLanded::Stays;
            match (stands, copy) {
                (Some(Standing::Landed), None) if the_list_stands => kept.held += 1,
                (_, Some(copy)) => {
                    kept.held += 1;
                    self.root_of_an_unread_copy(&copy)?;
                    to_place.push((pressed.len(), copy.cut, place));
                }
                (Some(Standing::Landed), None) => {
                    kept.queued += 1;
                    what_landed = WhatLanded::IsForgotten;
                }
                (_, None) => kept.queued += 1,
            }
            pressed.push(Pressed {
                song,
                place,
                what_landed,
            });
        }

        self.inner().write(|transaction| {
            pressed
                .iter()
                .try_for_each(|pressed| queued_in(transaction, name, pressed))
        })?;
        if !to_place.is_empty() {
            let cuts: Vec<(Cut, Option<i64>)> = to_place
                .iter()
                .map(|(_, cut, place)| (cut.clone(), *place))
                .collect();
            self.placed(name, &cuts)?;
            self.inner().write(|transaction| {
                to_place.iter().try_for_each(|(at, cut, _)| {
                    landed_in(transaction, name, pressed[*at].song, cut)
                })
            })?;
        }
        Ok(kept)
    }

    pub fn keeps_pending(&self) -> Result<u64> {
        let queued = self.inner().read(|connection| {
            connection
                .query_row(
                    "SELECT count(*) FROM kept_songs WHERE landed IS NULL AND gone = 0",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map(i64::cast_unsigned)
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        Ok(queued + self.roots_owed_a_read()?.len() as u64)
    }

    pub fn keep_pending(
        &self,
        providers: &Providers,
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<KeepSummary> {
        let _asking = self.asking_alone(PassKind::Poll)?;
        let _under_way = KeepUnderWay::of(self);
        organise::sweep_what_a_dead_writer_staged(self);
        self.read_what_landed_unread()?;

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
            let _heeded = Heeded(&over);
            let mut walking = Walking {
                into: &into,
                providers,
                progress: &progress,
                away: Away::default(),
                unread: Vec::new(),
            };
            let walked = self.walked(queued, &mut walking);
            walking.unread.sort();
            walking.unread.dedup();
            for root in walking.unread {
                synced::scanned_once_free(self, root);
            }
            walked
        })
    }

    fn root_stands(&self, root: &Path) -> Result<()> {
        if self.root_reaching(root)?.is_some() {
            return Ok(());
        }
        match self.add_root(root) {
            Err(Error::AlreadyWalking) => self
                .inner()
                .write(|transaction| store::register_root_beside_a_walk(transaction, root)),
            added => added,
        }
    }

    fn root_of_an_unread_copy(&self, copy: &HeldCopy) -> Result<Option<PathBuf>> {
        let (None, Some(path)) = (copy.track, copy.cut.location.as_path()) else {
            return Ok(None);
        };
        let reached = self.root_reaching(path)?;
        if reached.is_some() {
            return Ok(reached);
        }
        let folder = self
            .delivering_into()
            .and_then(|into| into.path.canonicalize().ok())
            .filter(|folder| path.starts_with(folder));
        if let Some(folder) = &folder {
            self.root_stands(folder)?;
        }
        Ok(folder)
    }

    fn roots_owed_a_read(&self) -> Result<Vec<PathBuf>> {
        let unread = self.inner().read(landed_unread)?;
        let mut roots = Vec::new();
        for path in unread.iter().filter(|path| path.exists()) {
            roots.extend(self.root_reaching(path)?);
        }
        Ok(roots)
    }

    fn read_what_landed_unread(&self) -> Result<()> {
        let mut roots = self.roots_owed_a_read()?;
        roots.sort();
        roots.dedup();
        for root in roots {
            synced::scanned_once_free(self, root);
        }
        Ok(())
    }

    pub fn has_a_folder_to_keep_into(&self) -> Result<()> {
        self.a_folder_to_keep_into().map(drop)
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

    fn walked(&self, queued: Vec<Queued>, walking: &mut Walking<'_>) -> Result<KeepSummary> {
        let gate = self.download_gate();
        let progress = walking.progress;
        let mut summary = KeepSummary::default();
        for row in queued {
            let yields = self.waited_on_the_meter(&gate, progress);
            if progress.is_cancelled() {
                break;
            }
            if yields {
                summary.yielded = true;
                break;
            }
            match self.turn(&row, walking)? {
                Turn::Landed => summary.landed += 1,
                Turn::Gone => summary.gone += 1,
                Turn::Unkept => summary.unkept += 1,
                Turn::Failed => {
                    self.tried_in_vain(&row)?;
                    summary.unkept += 1;
                }
                Turn::Mend => {
                    self.moved_back(&row)?;
                    summary.unkept += 1;
                }
                Turn::Stopped => {
                    let why = walking.away.why(&row.song.source).unwrap_or_default();
                    if why.says_nothing_of_the_song() {
                        self.moved_back(&row)?;
                    } else if self.tried_in_vain(&row)? {
                        summary.unkept += 1;
                    }
                    summary.stopped_by = Some(row.song.source);
                    summary.stopped = Some(KeepStopped {
                        why,
                        song: row.song.title,
                    });
                    break;
                }
                Turn::Yielded => {
                    summary.yielded = true;
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

    fn waited_on_the_meter(&self, gate: &Gate, progress: &PollProgress) -> bool {
        while gate.is_shut() && !progress.is_cancelled() && !self.a_poll_waits_on_a_keep() {
            progress.waits_on_the_meter(true);
            gate.wait_while_shut(HEEDED_EVERY);
        }
        progress.waits_on_the_meter(false);
        self.a_poll_waits_on_a_keep()
    }

    fn turn(&self, row: &Queued, walking: &mut Walking<'_>) -> Result<Turn> {
        if self.is_kept_for_a_list_it_cannot_enter(row)? {
            tracing::info!(
                video = row.song.video,
                "a kept song waits while its list is followed and takes no song"
            );
            return Ok(Turn::Unkept);
        }
        if let Some(copy) = self.copy_of(&row.song)? {
            walking.unread.extend(self.root_of_an_unread_copy(&copy)?);
            return self.settled_turn(row, &copy.cut);
        }

        let progress = walking.progress;
        let answer = asked(&row.song, walking.providers, progress, &mut walking.away);
        if answer.cancelled {
            return Ok(Turn::Over);
        }
        let Some(delivered) = answer.delivered else {
            return self.unanswered(row, &answer, &walking.away);
        };

        let gate = self.download_gate();
        let held_by_the_meter = gate.is_shut();
        let yields = self.waited_on_the_meter(&gate, progress);
        if progress.is_cancelled() {
            return Ok(Turn::Over);
        }
        if yields && held_by_the_meter {
            return Ok(Turn::Yielded);
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
            |reader, extension| filed::filed(self, walking.into, &song, reader, extension),
        )?;

        let video = &row.song.video;
        match drawn {
            Drawn::Read(Ok(filed)) => {
                let cut = Cut::whole(MediaLocation::local(&filed.path));
                let settled = self.settled_turn(row, &cut);
                self.root_stands(&filed.root)?;
                if !synced::scanned_if_free(self, filed.root.clone()) {
                    walking.unread.push(filed.root);
                }
                settled
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
            Drawn::Read(Err(unfiled @ Unfiled::Io(_))) => {
                tracing::warn!(
                    video,
                    ?unfiled,
                    "a kept song could not be written to the folder"
                );
                Ok(Turn::Mend)
            }
            Drawn::Read(Err(unfiled)) => {
                tracing::warn!(video, ?unfiled, "a kept song could not be filed");
                Ok(Turn::Failed)
            }
            Drawn::Unopened(Unopened::Gone) => {
                tracing::info!(video, "a kept song's video was gone when it was opened");
                self.given_up(row)?;
                Ok(Turn::Gone)
            }
            Drawn::Unopened(Unopened::Refused(_)) if progress.is_cancelled() => Ok(Turn::Over),
            Drawn::Unopened(Unopened::Refused(error)) => {
                tracing::warn!(%error, cause = ?error.cause(), video, "a kept song could not be opened");
                if error.is_the_provider_away() {
                    walking.away.note_why(&row.song.source, error.why_away());
                    return Ok(Turn::Stopped);
                }
                Ok(Turn::Failed)
            }
            Drawn::Stalled => {
                tracing::warn!(
                    video,
                    "a kept song stopped sending and waits for the next pass"
                );
                Ok(Turn::Failed)
            }
        }
    }

    fn unanswered(&self, row: &Queued, answer: &Answer, away: &Away) -> Result<Turn> {
        if away.holds(&row.song.source) {
            return Ok(Turn::Stopped);
        }
        if answer.refused + answer.late + answer.passed_over == 0 {
            self.given_up(row)?;
            return Ok(Turn::Unkept);
        }
        Ok(Turn::Failed)
    }

    fn copy_of(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        if let Some(held) = self.held_alike(song)? {
            return Ok(Some(held));
        }
        if let Some(held) = self.delivered_from_the_video(song)? {
            return Ok(Some(held));
        }
        self.landed_before(song)
    }

    fn delivered_from_the_video(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        let taken_from = song.taken_from().to_uri();
        let copies = self
            .inner()
            .read(|connection| store::held_from_a_delivery(connection, &taken_from))?;
        let copy = self.first_there(copies, |copy| &copy.held)?;

        Ok(copy.map(HeldCopy::of))
    }

    fn held_alike(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        let identity = song.identity();
        let (Some(lead), Some(length)) = (identity.lead_artist(), song.length) else {
            return Ok(None);
        };
        let copies = self
            .inner()
            .read(|connection| store::held_named_alike(connection, &song.title, lead, length))?;
        let copy = self.first_there(copies, |copy| &copy.held)?;

        Ok(copy.map(HeldCopy::of))
    }

    fn landed_before(&self, song: &SongToKeep) -> Result<Option<HeldCopy>> {
        let landed = self
            .inner()
            .read(|connection| landed_as(connection, &song.source, &song.video))?;
        let Some(held) = self.first_there(landed, |held| held)? else {
            return Ok(None);
        };

        Ok(Some(HeldCopy {
            track: self.track_at(&held.path, None)?.map(|track| track.id),
            cut: Cut::whole(MediaLocation::local(held.path)),
        }))
    }

    fn first_there<T>(&self, copies: Vec<T>, held: impl Fn(&T) -> &HeldRow) -> Result<Option<T>> {
        if copies.is_empty() {
            return Ok(None);
        }
        let vault = self.vault().map(AsRef::as_ref);
        let away = self.inner().read(volumes::absent)?;

        Ok(copies.into_iter().find(|copy| {
            let row = held(copy);
            row.stands(vault) || volumes::is_on_an_absent_one(&row.path, &away)
        }))
    }

    fn is_kept_for_a_list_it_cannot_enter(&self, row: &Queued) -> Result<bool> {
        let Some(list) = &row.playlist else {
            return Ok(false);
        };
        self.inner()
            .read(|connection| playlist::is_followed(connection, list))
    }

    fn settled_turn(&self, row: &Queued, cut: &Cut) -> Result<Turn> {
        match self.settled_by(row, cut) {
            Ok(()) => Ok(Turn::Landed),
            Err(Error::PlaylistReadOnly(_)) => Ok(Turn::Unkept),
            Err(error) => Err(error),
        }
    }

    fn settled_by(&self, row: &Queued, cut: &Cut) -> Result<()> {
        let path = playlist::local_path(&cut.location)?.into_owned();
        if let Some(list) = &row.playlist {
            let place = self
                .inner()
                .read(|connection| place_of(connection, row.id))?;
            self.placed(list, &[(cut.clone(), place)])?;
        }
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "UPDATE kept_songs SET landed = ?2, gone = 0 WHERE id = ?1",
                    params![row.id, path],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    fn moved_back(&self, row: &Queued) -> Result<()> {
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "UPDATE kept_songs SET tried = ?2 WHERE id = ?1",
                    params![row.id, store::to_nanos(SystemTime::now())],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    fn tried_in_vain(&self, row: &Queued) -> Result<bool> {
        self.inner().write(|transaction| {
            transaction
                .query_row(
                    "UPDATE kept_songs
                        SET tried = ?2, tries = tries + 1, gone = tries + 1 >= ?3
                      WHERE id = ?1
                  RETURNING gone",
                    params![
                        row.id,
                        store::to_nanos(SystemTime::now()),
                        KEEP_TRIES_AT_MOST
                    ],
                    |row| row.get(0),
                )
                .map_err(|source| Error::store(StoreOp::Update, source))
        })
    }

    pub(crate) fn note_landed_for_a_want(
        &self,
        want: &Want,
        taken_from: &MediaLocation,
        at: &MediaLocation,
    ) -> Result<()> {
        let (Locator::Key(key), Some(path)) = (taken_from.locator(), at.as_path()) else {
            return Ok(());
        };
        let Some(video) = key.strip_prefix(A_SONG_IS_KEYED) else {
            return Ok(());
        };
        self.inner().write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO kept_songs (source, video, title, artist, album, length, asked, landed)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT (source, video) WHERE playlist IS NULL
                     DO UPDATE SET landed = excluded.landed, gone = 0",
                    params![
                        taken_from.source().as_str(),
                        video,
                        want.title,
                        want.artist,
                        want.album_title,
                        length_column(want.length),
                        store::to_nanos(SystemTime::now()),
                        store::path_text(path),
                    ],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub(crate) fn a_keep_holds_the_offer(&self, offer: &Delivered) -> bool {
        let Delivery::Stream { key, .. } = &offer.delivery else {
            return false;
        };
        let Some(video) = key.strip_prefix(A_SONG_IS_KEYED) else {
            return false;
        };
        let held = self
            .inner()
            .read(|connection| landed_as(connection, &offer.provider, video))
            .and_then(|landed| self.first_there(landed, |held| held));
        match held {
            Ok(held) => held.is_some(),
            Err(error) => {
                tracing::warn!(%error, "what was kept could not be weighed against an offer");
                false
            }
        }
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
        let already: AHashSet<playlist::Row> = match self.playlist_named(list)? {
            Some(standing) => self
                .inner()
                .read(|connection| playlist::rows(connection, standing.id))?
                .into_iter()
                .collect(),
            None => AHashSet::new(),
        };
        let plain: Vec<Cut> = cuts.iter().map(|(cut, _)| cut.clone()).collect();
        let filled = playlist::fill(self.inner(), list, &plain)?;
        for (cut, place) in cuts {
            if let Some(place) = place
                && !already.contains(&playlist::Row::of(cut)?)
            {
                self.seated(filled.playlist, list, cuts, cut, *place)?;
            }
        }
        Ok(())
    }

    fn seated(
        &self,
        id: resonate_core::PlaylistId,
        list: &str,
        cuts: &[(Cut, Option<i64>)],
        cut: &Cut,
        place: i64,
    ) -> Result<()> {
        let wanted = playlist::Row::of(cut)?;
        let (rows, mut places, kept_in_an_order) = self.inner().read(|connection| {
            Ok((
                playlist::rows(connection, id)?,
                places_in(connection, list)?,
                playlist::kept_in(connection, id)?.is_some(),
            ))
        })?;
        if kept_in_an_order {
            return Ok(());
        }
        for (placing, at) in cuts {
            if let Some(at) = at {
                places.insert(playlist::Row::of(placing)?.path, *at);
            }
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

fn refused_untitled(song: &SongToKeep) -> Result<()> {
    if song.title.trim().is_empty() {
        return Err(Error::KeptWithNoTitle);
    }
    Ok(())
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
            within: ANSWERED_ALONE_WITHIN,
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

fn length_column(length: Option<Duration>) -> Option<i64> {
    length.map(|length| i64::try_from(length.as_millis()).unwrap_or(i64::MAX))
}

fn queued_alone(
    transaction: &Transaction<'_>,
    song: &SongToKeep,
    what_landed: WhatLanded,
) -> Result<()> {
    transaction
        .execute(
            "INSERT INTO kept_songs (source, video, title, artist, album, length, asked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (source, video) WHERE playlist IS NULL
             DO UPDATE SET gone = 0, tried = NULL, tries = 0,
                           landed = CASE WHEN ?8 THEN NULL ELSE landed END",
            params![
                song.source.as_str(),
                song.video,
                song.title,
                song.artist,
                song.album,
                length_column(song.length),
                store::to_nanos(SystemTime::now()),
                what_landed == WhatLanded::IsForgotten,
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
                 (source, video, title, artist, album, length, playlist, place, asked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT (source, video, playlist)
             DO UPDATE SET place = excluded.place, gone = 0, tried = NULL, tries = 0,
                           landed = CASE WHEN ?10 THEN NULL ELSE landed END",
            params![
                song.source.as_str(),
                song.video,
                song.title,
                song.artist,
                song.album,
                length_column(song.length),
                list,
                pressed.place,
                store::to_nanos(SystemTime::now()),
                pressed.what_landed == WhatLanded::IsForgotten,
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Insert, source))
}

fn landed_in(
    transaction: &Transaction<'_>,
    list: &str,
    song: &SongToKeep,
    cut: &Cut,
) -> Result<()> {
    transaction
        .execute(
            "UPDATE kept_songs SET landed = ?4, gone = 0
              WHERE source = ?1 AND video = ?2 AND playlist = ?3",
            params![
                song.source.as_str(),
                song.video,
                list,
                playlist::local_path(&cut.location)?,
            ],
        )
        .map(drop)
        .map_err(|source| Error::store(StoreOp::Update, source))
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

fn landed_as(connection: &Connection, source: &SourceId, video: &str) -> Result<Vec<HeldRow>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT DISTINCT landed FROM kept_songs
              WHERE source = ?1 AND video = ?2 AND landed IS NOT NULL",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map(params![source.as_str(), video], |row| {
            Ok(HeldRow {
                path: store::path_of(&row.get::<_, String>(0)?),
                within_the_vault: None,
            })
        })
        .and_then(|rows| rows.collect::<rusqlite::Result<_>>())
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn landed_unread(connection: &Connection) -> Result<Vec<PathBuf>> {
    let mut statement = connection
        .prepare_cached(
            "SELECT DISTINCT k.landed FROM kept_songs k
              WHERE k.landed IS NOT NULL
                AND NOT EXISTS (SELECT 1 FROM tracks t WHERE t.path = k.landed)",
        )
        .map_err(|source| Error::store(StoreOp::Prepare, source))?;
    statement
        .query_map([], |row| Ok(store::path_of(&row.get::<_, String>(0)?)))
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
