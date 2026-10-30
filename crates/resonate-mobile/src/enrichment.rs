use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, VecDeque},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Sender, bounded};
use parking_lot::Mutex;
use resonate_analysis::{Watching, band_figures};
use resonate_codec::{CoverArt, ImageFormat, Sources, probe_scanned};
use resonate_core::{AlbumId, Chromaprint, Isrc, Link, Mbid, SourceId, TrackId};
use resonate_library::{
    A_THUMBNAIL_BELOW, Agreement, AlbumToAsk, BETTERED_AFTER, COVERS_ASKED_AGAIN_AFTER, Certainty,
    Fingerprints, Fruitless, Identified, KeptLyrics, LookupOp, LyricText, LyricsAsked,
    MISSED_AGAIN_AFTER, PORTRAITS_ASKED_AGAIN_AFTER, RECOGNITIONS_REFUSED_BEFORE_GIVING_UP,
    REFUSALS_THAT_END_A_PASS, Recording, Reference, Release, ReleaseGroup, TrackToAsk, WAITS,
    Wording, best_recording, best_release, betters, closest_release, due_again, due_parameters,
    group_asked, needs_its_releases_told, recording_asked, release_asked, the_strict_recording,
    top_group, top_release,
};
use resonate_online::{AcoustId, Client, Online};
use rusqlite::{OptionalExtension, params, params_from_iter, types::Value};

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track,
    catalog::Catalog,
    catalog_extras::{kept_picture, unreferenced_picture_dropped},
    documents::location,
    features::{ArtistInformation, recognised_through},
};

const PICTURES_WAITING_AT_MOST: usize = 64;

const PERFORMERS_WEIGHED_AT_MOST: usize = 32;

const LISTED_APART: char = '\n';

pub(crate) const FETCHED_LYRICS: &str = "lrclib";
pub(crate) const SHARED_LYRICS: &str = "shared";

const ARCHIVED_COVER: &str = "coverartarchive";

const YEAR_DIGITS: usize = 4;

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct EnrichmentOptions {
    pub refresh: bool,
    pub at_most: Option<u32>,
    #[uniffi(default = "")]
    pub acoustid_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum EnrichmentStop {
    Finished,
    Cancelled,
    Offline,
    Unreachable,
    Refused,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EnrichmentStatus {
    pub running: bool,
    pub due: u32,
    pub asked: u32,
    pub matched: u32,
    pub covers: u32,
    pub portraits: u32,
    pub lyrics: u32,
    pub refused: u32,
    pub stopped: Option<EnrichmentStop>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ChangedArt {
    pub revision: u64,
    pub tracks: Vec<u64>,
}

#[derive(Default)]
pub(crate) struct Enriching {
    running: Mutex<Option<Running>>,
    tally: Arc<Tally>,
}

struct Running {
    told: Arc<Told>,
    pass: JoinHandle<()>,
}

#[derive(Clone)]
pub(crate) struct Asking {
    reference: Arc<dyn Reference>,
    client: Arc<Client>,
    printer: Option<Arc<dyn Fingerprints>>,
}

impl Asking {
    pub(crate) fn over(client: Arc<Client>) -> Self {
        Self {
            reference: Arc::new(Online::with_client(Arc::clone(&client))),
            client,
            printer: None,
        }
    }

    pub(crate) fn hearing_through(self, printer: Arc<dyn Fingerprints>) -> Self {
        Self {
            printer: Some(printer),
            ..self
        }
    }

    fn is_reaching(&self) -> bool {
        self.client.is_reaching() && !self.client.is_lookup_held_by_the_meter()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaving {
    Listener,
    Closing,
}

#[derive(Default)]
struct Sought(Mutex<Vec<u64>>);

impl Sought {
    fn seek(&self, track: u64) {
        let mut sought = self.0.lock();
        sought.retain(|held| *held != track);
        sought.push(track);
    }

    fn taken(&self) -> Vec<u64> {
        std::mem::take(&mut *self.0.lock())
    }
}

#[derive(Default)]
struct Told {
    cancelled: AtomicBool,
    closing: AtomicBool,
    asked_first: Sought,
    sung_first: Sought,
}

impl Told {
    fn cancel(&self, leaving: Leaving) {
        self.closing
            .fetch_or(leaving == Leaving::Closing, Ordering::AcqRel);
        self.cancelled.store(true, Ordering::Release);
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    fn keeps_the_mark(&self) -> bool {
        self.closing.load(Ordering::Acquire)
    }

    fn seek(&self, track: u64) {
        self.asked_first.seek(track);
        self.sung_first.seek(track);
    }
}

#[derive(Default)]
struct Tally {
    running: AtomicBool,
    due: AtomicU32,
    asked: AtomicU32,
    matched: AtomicU32,
    covers: AtomicU32,
    portraits: AtomicU32,
    lyrics: AtomicU32,
    refused: AtomicU32,
    stopped: Mutex<Option<EnrichmentStop>>,
}

impl Tally {
    fn began(&self) {
        for count in [
            &self.due,
            &self.asked,
            &self.matched,
            &self.covers,
            &self.portraits,
            &self.lyrics,
            &self.refused,
        ] {
            count.store(0, Ordering::Relaxed);
        }
        *self.stopped.lock() = None;
        self.running.store(true, Ordering::Release);
    }

    fn ended(&self, stop: EnrichmentStop) {
        *self.stopped.lock() = Some(stop);
        self.running.store(false, Ordering::Release);
    }

    fn count(count: &AtomicU32, by: usize) {
        count.fetch_add(u32::try_from(by).unwrap_or(u32::MAX), Ordering::Relaxed);
    }

    fn status(&self) -> EnrichmentStatus {
        EnrichmentStatus {
            running: self.running.load(Ordering::Acquire),
            due: self.due.load(Ordering::Relaxed),
            asked: self.asked.load(Ordering::Relaxed),
            matched: self.matched.load(Ordering::Relaxed),
            covers: self.covers.load(Ordering::Relaxed),
            portraits: self.portraits.load(Ordering::Relaxed),
            lyrics: self.lyrics.load(Ordering::Relaxed),
            refused: self.refused.load(Ordering::Relaxed),
            stopped: *self.stopped.lock(),
        }
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn start_enrichment(&self, options: EnrichmentOptions) -> bool {
        if !self.services.online.client().is_reaching() {
            return false;
        }
        let yielding = Arc::new(self.services.online.client().yielding());
        let key = options.acoustid_key.trim().to_owned();
        let asking = Asking::over(Arc::clone(&yielding));
        let asking = if key.is_empty() {
            asking
        } else {
            asking.hearing_through(Arc::new(AcoustId::new(yielding, key)))
        };
        self.start_enriching(asking, options)
    }

    pub fn stop_enrichment(&self) {
        self.stop_enriching(Leaving::Listener);
    }

    pub fn enrichment_status(&self) -> EnrichmentStatus {
        self.enriching.tally.status()
    }

    pub fn seek_enrichment(&self, track: u64) {
        if let Some(running) = self.enriching.running.lock().as_ref() {
            running.told.seek(track);
        }
    }

    pub fn art_changed_since(&self, revision: u64) -> Result<ChangedArt> {
        self.catalog.art_changed_since(revision)
    }

    pub fn artist_portrait(&self, mbid: String) -> Result<Option<Vec<u8>>> {
        self.catalog.portrait(&mbid)
    }

    pub fn ask_again_for_covers(&self) -> Result<u32> {
        self.catalog.ask_again_for_covers()
    }
}

impl MobilePlayer {
    fn start_enriching(&self, asking: Asking, options: EnrichmentOptions) -> bool {
        if !asking.is_reaching() {
            return false;
        }
        let mut running = self.enriching.running.lock();
        if running
            .as_ref()
            .is_some_and(|running| !running.pass.is_finished())
        {
            return false;
        }
        if let Some(finished) = running.take()
            && finished.pass.join().is_err()
        {
            tracing::warn!("the last enrichment pass ended in a panic");
        }

        let told = Arc::new(Told::default());
        let tally = Arc::clone(&self.enriching.tally);
        let catalog = Arc::clone(&self.catalog);
        let sources = Arc::clone(&self.sources);
        tally.began();

        let pass = thread::Builder::new()
            .name("resonate-enrich".into())
            .spawn({
                let told = Arc::clone(&told);
                let tally = Arc::clone(&tally);
                move || run(&catalog, &sources, &asking, &told, &tally, options)
            });
        match pass {
            Ok(pass) => {
                *running = Some(Running { told, pass });
                true
            }
            Err(error) => {
                tracing::warn!(%error, "the enrichment pass could not start");
                tally.ended(EnrichmentStop::Failed);
                false
            }
        }
    }

    pub(crate) fn stop_enriching_to_close(&self) {
        self.stop_enriching(Leaving::Closing);
    }

    fn stop_enriching(&self, leaving: Leaving) {
        let Some(running) = self.enriching.running.lock().take() else {
            return;
        };
        running.told.cancel(leaving);
        if running.pass.join().is_err() {
            tracing::warn!("the enrichment pass ended in a panic");
        }
    }

    pub(crate) fn enrich_one(&self, asking: &Asking, id: u64) -> Result<()> {
        let told = Told::default();
        let tally = Tally::default();
        let pictures = Pictures::inline();
        let pass = Pass::new(&self.catalog, asking, &told, &tally, &pictures);

        let asked = pass.one(id);
        if let Err(error) = sing(&self.catalog, asking, &told, &tally, id, &Cell::new(0)) {
            tracing::debug!(
                ?error,
                track = id,
                "the lyrics of an enriched track were not asked"
            );
        }

        match asked {
            Ok(()) | Err(Ended::Refused | Ended::Cancelled) => Ok(()),
            Err(Ended::Offline) => Err(Error::OnlineOff),
            Err(Ended::Unreachable(error) | Ended::Failed(error)) => Err(*error),
        }
    }
}

#[derive(Debug)]
enum Ended {
    Cancelled,
    Offline,
    Unreachable(Box<Error>),
    Refused,
    Failed(Box<Error>),
}

impl From<Error> for Ended {
    fn from(error: Error) -> Self {
        Self::Failed(Box::new(error))
    }
}

type Asked<T> = std::result::Result<T, Ended>;

enum Heard<T> {
    Answered(T),
    Refused,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Ask {
    Album(String),
    Track(u64),
    Artist(String),
}

fn run(
    catalog: &Arc<Catalog>,
    sources: &Arc<Sources>,
    asking: &Asking,
    told: &Arc<Told>,
    tally: &Arc<Tally>,
    options: EnrichmentOptions,
) {
    let due = match catalog.carried_on(options.refresh, SystemTime::now()) {
        Ok(due) => due,
        Err(error) => {
            tracing::warn!(%error, "the enrichment mark could not be read");
            tally.ended(EnrichmentStop::Failed);
            return;
        }
    };
    let at_most = options.at_most.map(|at_most| at_most as usize);

    let lyrics = thread::Builder::new()
        .name("resonate-enrich-lyrics".into())
        .spawn({
            let catalog = Arc::clone(catalog);
            let asking = asking.clone();
            let told = Arc::clone(told);
            let tally = Arc::clone(tally);
            move || sing_the_due(&catalog, &asking, &told, &tally, due, at_most)
        });
    let hearing = thread::Builder::new().name("resonate-study".into()).spawn({
        let catalog = Arc::clone(catalog);
        let sources = Arc::clone(sources);
        let asking = asking.clone();
        let told = Arc::clone(told);
        let tally = Arc::clone(tally);
        move || hear_the_due(&catalog, &sources, &asking, &told, &tally, due, at_most)
    });
    let pictures = Pictures::start(catalog, asking, told, tally);

    let ended = Pass::new(catalog, asking, told, tally, &pictures).run(due, at_most);
    pictures.rest();
    match hearing {
        Ok(hearing) => {
            if hearing.join().is_err() {
                tracing::warn!("the study worker ended in a panic");
            }
        }
        Err(error) => tracing::warn!(%error, "the study worker could not start"),
    }
    match lyrics {
        Ok(lyrics) => {
            if lyrics.join().is_err() {
                tracing::warn!("the lyric reader ended in a panic");
            }
        }
        Err(error) => tracing::warn!(%error, "the lyric reader could not start"),
    }

    let stop = match ended {
        Ok(()) => EnrichmentStop::Finished,
        Err(Ended::Cancelled) => EnrichmentStop::Cancelled,
        Err(Ended::Offline) => EnrichmentStop::Offline,
        Err(Ended::Refused) => EnrichmentStop::Refused,
        Err(Ended::Unreachable(error)) => {
            tracing::info!(%error, "the reference could not be reached, so the pass stops");
            EnrichmentStop::Unreachable
        }
        Err(Ended::Failed(error)) => {
            tracing::warn!(%error, "the enrichment pass failed");
            EnrichmentStop::Failed
        }
    };
    let finished = stop == EnrichmentStop::Finished
        || (stop == EnrichmentStop::Cancelled && !told.keeps_the_mark());
    if finished && let Err(error) = catalog.note_finished() {
        tracing::warn!(%error, "the next pass may carry on one that already finished");
    }
    tally.ended(stop);
}

#[derive(Clone, Copy)]
struct Due {
    refresh: bool,
    started: i64,
    now: SystemTime,
}

impl Due {
    fn parameters(self, only: Value) -> Vec<Value> {
        due_parameters(WAITS, self.refresh, self.now)
            .into_iter()
            .chain([Value::Integer(self.started), only])
            .collect()
    }
}

fn due_where(row: &str) -> String {
    format!(
        "{} AND (NOT ?1 OR coalesce({row}.asked, 0) < ?6)",
        due_again(row)
    )
}

enum Picture {
    Cover {
        album: String,
        release: Mbid,
        group: Option<Mbid>,
    },
    GroupCover {
        album: String,
        group: Mbid,
    },
    Portrait {
        artist: String,
        links: Vec<Link>,
    },
}

struct Pictures {
    wanted: Option<Sender<Picture>>,
    reader: Option<JoinHandle<()>>,
}

impl Pictures {
    fn inline() -> Self {
        Self {
            wanted: None,
            reader: None,
        }
    }

    fn start(
        catalog: &Arc<Catalog>,
        asking: &Asking,
        told: &Arc<Told>,
        tally: &Arc<Tally>,
    ) -> Self {
        let (wanted, asked) = bounded::<Picture>(PICTURES_WAITING_AT_MOST);
        let catalog = Arc::clone(catalog);
        let asking = asking.clone();
        let told = Arc::clone(told);
        let tally = Arc::clone(tally);
        let reader = thread::Builder::new()
            .name("resonate-enrich-pictures".into())
            .spawn(move || {
                for picture in asked {
                    if told.is_cancelled() || !asking.is_reaching() {
                        continue;
                    }
                    fetch(&catalog, &asking, &tally, picture);
                }
            });
        match reader {
            Ok(reader) => Self {
                wanted: Some(wanted),
                reader: Some(reader),
            },
            Err(error) => {
                tracing::warn!(%error, "the picture reader could not start, so pictures are fetched in the pass");
                Self::inline()
            }
        }
    }

    fn want(&self, catalog: &Catalog, asking: &Asking, tally: &Tally, picture: Picture) {
        let unsent = match &self.wanted {
            Some(wanted) => wanted.send(picture).err().map(|unsent| unsent.0),
            None => Some(picture),
        };
        if let Some(picture) = unsent {
            fetch(catalog, asking, tally, picture);
        }
    }

    fn rest(mut self) {
        drop(self.wanted.take());
        if let Some(reader) = self.reader.take()
            && reader.join().is_err()
        {
            tracing::warn!("the picture reader ended in a panic");
        }
    }
}

fn fetch(catalog: &Catalog, asking: &Asking, tally: &Tally, picture: Picture) {
    let reference = &*asking.reference;
    let landed = match picture {
        Picture::Cover {
            album,
            release,
            group,
        } => answered_picture(tally, reference.cover(&release, group.as_ref()))
            .map(|found| catalog.land_cover(&album, found.as_ref(), &tally.covers)),
        Picture::GroupCover { album, group } => {
            answered_picture(tally, reference.group_cover(&group))
                .map(|found| catalog.land_cover(&album, found.as_ref(), &tally.covers))
        }
        Picture::Portrait { artist, links } => answered_picture(tally, reference.portrait(&links))
            .map(|found| catalog.land_portrait(&artist, found.as_ref(), &tally.portraits)),
    };
    if let Some(Err(error)) = landed {
        tracing::warn!(%error, "a fetched picture was not kept");
    }
}

fn answered_picture(
    tally: &Tally,
    found: resonate_library::Result<Option<CoverArt>>,
) -> Option<Option<CoverArt>> {
    match found {
        Ok(found) => Some(found),
        Err(error) => {
            tracing::debug!(%error, "a picture could not be fetched");
            Tally::count(&tally.refused, 1);
            None
        }
    }
}

struct AlbumHeld {
    key: String,
    album: AlbumToAsk,
    answered: bool,
}

struct TrackHeld {
    track: TrackToAsk,
    album: Option<String>,
}

struct Pass<'a> {
    catalog: &'a Catalog,
    asking: &'a Asking,
    told: &'a Told,
    tally: &'a Tally,
    pictures: &'a Pictures,
    refusals: Cell<u32>,
    refused_in_a_row: Cell<u32>,
    covered: RefCell<BTreeSet<String>>,
    pictured: RefCell<BTreeSet<String>>,
    first: RefCell<BTreeSet<Ask>>,
    pictured_at_once: Cell<bool>,
}

impl<'a> Pass<'a> {
    fn new(
        catalog: &'a Catalog,
        asking: &'a Asking,
        told: &'a Told,
        tally: &'a Tally,
        pictures: &'a Pictures,
    ) -> Self {
        Self {
            catalog,
            asking,
            told,
            tally,
            pictures,
            refusals: Cell::new(0),
            refused_in_a_row: Cell::new(0),
            covered: RefCell::new(BTreeSet::new()),
            pictured: RefCell::new(BTreeSet::new()),
            first: RefCell::new(BTreeSet::new()),
            pictured_at_once: Cell::new(false),
        }
    }

    fn run(&self, due: Due, at_most: Option<usize>) -> Asked<()> {
        let capped = |mut found: Vec<Ask>| {
            if let Some(at_most) = at_most {
                found.truncate(at_most);
            }
            found
        };
        let albums = capped(
            self.catalog
                .albums_due(due, Value::Null)?
                .into_iter()
                .map(Ask::Album)
                .collect(),
        );
        let tracks = capped(
            self.catalog
                .tracks_due(due, Value::Null)?
                .into_iter()
                .map(Ask::Track)
                .collect(),
        );
        let queue: VecDeque<Ask> = albums.into_iter().chain(tracks).collect();
        let mut queue = self.downloads_first(queue)?;
        let mut spent = BTreeSet::new();
        Tally::count(&self.tally.due, queue.len());

        self.walk(&mut queue, &mut spent, due)?;
        self.look_again_for_covers(due.now)?;
        loop {
            self.catalog.take_in_artists()?;
            let artists = capped(
                self.catalog
                    .artists_due(due, Value::Null)?
                    .into_iter()
                    .map(Ask::Artist)
                    .filter(|artist| !spent.contains(artist))
                    .collect(),
            );
            if artists.is_empty() {
                return Ok(());
            }
            Tally::count(&self.tally.due, artists.len());
            queue.extend(artists);
            self.walk(&mut queue, &mut spent, due)?;
            if at_most.is_some() {
                return Ok(());
            }
        }
    }

    fn downloads_first(&self, queue: VecDeque<Ask>) -> Asked<VecDeque<Ask>> {
        let due: BTreeSet<&Ask> = queue.iter().collect();
        let mut first = Vec::new();
        let mut taken = BTreeSet::new();
        for (track, album) in self.catalog.downloaded()? {
            for ask in [album.map(Ask::Album), Some(Ask::Track(track))]
                .into_iter()
                .flatten()
            {
                if due.contains(&ask) && taken.insert(ask.clone()) {
                    first.push(ask);
                }
            }
        }
        drop(due);

        self.first.borrow_mut().extend(taken.iter().cloned());
        Ok(first
            .into_iter()
            .chain(queue.into_iter().filter(|ask| !taken.contains(ask)))
            .collect())
    }

    fn walk(&self, queue: &mut VecDeque<Ask>, spent: &mut BTreeSet<Ask>, due: Due) -> Asked<()> {
        loop {
            self.go_on()?;
            self.lift(queue, spent, due)?;
            let Some(ask) = queue.pop_front() else {
                return Ok(());
            };
            if !spent.insert(ask.clone()) {
                continue;
            }
            self.pictured_at_once
                .set(self.first.borrow_mut().remove(&ask));
            match &ask {
                Ask::Album(key) => self.album(key)?,
                Ask::Track(id) => self.track(*id)?,
                Ask::Artist(mbid) => self.artist(mbid)?,
            }
            Tally::count(&self.tally.asked, 1);
        }
    }

    fn lift(&self, queue: &mut VecDeque<Ask>, spent: &BTreeSet<Ask>, due: Due) -> Asked<()> {
        for sought in self.told.asked_first.taken() {
            let track = match self.catalog.track(sought) {
                Ok(track) => track,
                Err(Error::UnknownTrack) => continue,
                Err(error) => return Err(error.into()),
            };
            let album = self.catalog.album_key_of(sought)?;
            let asks = [
                track.artist_mbid.map(Ask::Artist),
                Some(Ask::Track(sought)),
                album.map(Ask::Album),
            ];
            for ask in asks.into_iter().flatten() {
                if spent.contains(&ask) {
                    continue;
                }
                if let Some(at) = queue.iter().position(|queued| *queued == ask) {
                    queue.remove(at);
                } else if self.is_due(&ask, due)? {
                    Tally::count(&self.tally.due, 1);
                } else {
                    continue;
                }
                self.first.borrow_mut().insert(ask.clone());
                queue.push_front(ask);
            }
        }
        Ok(())
    }

    fn is_due(&self, ask: &Ask, due: Due) -> Asked<bool> {
        Ok(match ask {
            Ask::Album(key) => !self
                .catalog
                .albums_due(due, Value::Text(key.clone()))?
                .is_empty(),
            Ask::Track(id) => !self
                .catalog
                .tracks_due(due, Value::Integer(*id as i64))?
                .is_empty(),
            Ask::Artist(mbid) => {
                self.catalog.take_in_artists()?;
                !self
                    .catalog
                    .artists_due(due, Value::Text(mbid.clone()))?
                    .is_empty()
            }
        })
    }

    fn one(&self, id: u64) -> Asked<()> {
        if let Some(album) = self.catalog.album_key_of(id)? {
            self.album(&album)?;
        }
        self.track(id)?;
        if let Some(artist) = self.catalog.track(id)?.artist_mbid {
            self.catalog.take_in_artists()?;
            self.artist(&artist)?;
        }
        Ok(())
    }

    fn go_on(&self) -> Asked<()> {
        if self.told.is_cancelled() {
            return Err(Ended::Cancelled);
        }
        if !self.asking.is_reaching() {
            return Err(Ended::Offline);
        }
        Ok(())
    }

    fn ask<T>(
        &self,
        op: LookupOp,
        asked: impl FnOnce(&dyn Reference) -> resonate_library::Result<T>,
    ) -> Asked<Heard<T>> {
        self.go_on()?;
        match asked(&*self.asking.reference) {
            Ok(answer) => {
                self.refused_in_a_row.set(0);
                Ok(Heard::Answered(answer))
            }
            Err(
                resonate_library::Error::Refused { .. }
                | resonate_library::Error::Unreadable { .. },
            ) => {
                tracing::debug!(?op, "the reference refused a lookup");
                self.note_a_refusal()?;
                Ok(Heard::Refused)
            }
            Err(resonate_library::Error::TooLarge { .. }) => Ok(Heard::Refused),
            Err(_) if !self.asking.is_reaching() => Err(Ended::Offline),
            Err(source) => Err(Ended::Unreachable(Box::new(Error::reference(op, source)))),
        }
    }

    fn note_a_refusal(&self) -> Asked<()> {
        self.refusals.set(self.refusals.get().saturating_add(1));
        Tally::count(&self.tally.refused, 1);
        let refused = self.refused_in_a_row.get().saturating_add(1);
        self.refused_in_a_row.set(refused);
        if refused >= REFUSALS_THAT_END_A_PASS {
            return Err(Ended::Refused);
        }
        Ok(())
    }

    fn fruitlessly(&self, since: u32) -> Fruitless {
        if self.refusals.get() > since {
            Fruitless::Refused
        } else {
            Fruitless::Missed
        }
    }

    fn either_way<T, W>(
        &self,
        op: LookupOp,
        asked: impl Fn(&dyn Reference, Wording) -> resonate_library::Result<Vec<T>>,
        weighed: impl Fn(Vec<T>) -> W,
        landed: impl Fn(&W) -> bool,
    ) -> Asked<Heard<W>> {
        let phrased = match self.ask(op, |reference| asked(reference, Wording::Phrase))? {
            Heard::Answered(found) => weighed(found),
            Heard::Refused => return Ok(Heard::Refused),
        };
        if landed(&phrased) {
            return Ok(Heard::Answered(phrased));
        }

        Ok(
            match self.ask(op, |reference| asked(reference, Wording::Words))? {
                Heard::Answered(found) => Heard::Answered(weighed(found)),
                Heard::Refused => Heard::Refused,
            },
        )
    }

    fn album(&self, key: &str) -> Asked<()> {
        let Some(held) = self.catalog.album_to_ask(key)? else {
            return Ok(());
        };
        let album = &held.album;
        let since = self.refusals.get();

        if let Some(mbid) = &album.mbid {
            match self.ask(LookupOp::Release, |reference| reference.release(mbid))? {
                Heard::Answered(Some(release)) => return self.take_release(&held, &release),
                Heard::Answered(None) => {}
                Heard::Refused => return self.album_missed(&held, since),
            }
        }
        let found = self.either_way(
            LookupOp::FindRelease,
            |reference, wording| reference.find_release(&release_asked(album, wording)),
            |found| top_release(found, album),
            |identified| *identified != Identified::Nothing,
        )?;
        match found {
            Heard::Answered(Identified::Found(release)) => {
                return self.land_release(&held, &release, since);
            }
            Heard::Answered(Identified::Group(group)) => {
                return self.land_group(&held, &group, since);
            }
            Heard::Answered(Identified::Nothing) => {}
            Heard::Refused => return self.album_missed(&held, since),
        }
        if let Some(group) = &album.group {
            match self.ask(LookupOp::ReleaseGroup, |reference| {
                reference.release_group(group)
            })? {
                Heard::Answered(Some(found)) => return self.settle_group(&held, &found, since),
                Heard::Answered(None) => {}
                Heard::Refused => return self.album_missed(&held, since),
            }
        }
        let grouped = self.either_way(
            LookupOp::FindReleaseGroup,
            |reference, wording| reference.find_release_group(&group_asked(album, wording)),
            |found| top_group(found, album),
            Option::is_some,
        )?;
        match grouped {
            Heard::Answered(Some(group)) => self.land_group(&held, &group, since),
            Heard::Answered(None) | Heard::Refused => self.album_missed(&held, since),
        }
    }

    fn album_missed(&self, held: &AlbumHeld, since: u32) -> Asked<()> {
        self.catalog
            .stamp_album_asked(&held.key, self.fruitlessly(since), SystemTime::now())?;
        Ok(())
    }

    fn land_release(&self, held: &AlbumHeld, release: &Mbid, since: u32) -> Asked<()> {
        match self.ask(LookupOp::Release, |reference| reference.release(release))? {
            Heard::Answered(Some(release)) => self.take_release(held, &release),
            Heard::Answered(None) | Heard::Refused => self.album_missed(held, since),
        }
    }

    fn land_group(&self, held: &AlbumHeld, group: &Mbid, since: u32) -> Asked<()> {
        match self.ask(LookupOp::ReleaseGroup, |reference| {
            reference.release_group(group)
        })? {
            Heard::Answered(Some(found)) => self.settle_group(held, &found, since),
            Heard::Answered(None) | Heard::Refused => self.album_missed(held, since),
        }
    }

    fn settle_group(&self, held: &AlbumHeld, group: &ReleaseGroup, since: u32) -> Asked<()> {
        match closest_release(group, &held.album) {
            Some((release, _)) => {
                let release = release.id.clone();
                self.land_release(held, &release, since)
            }
            None => self.take_group(held, group),
        }
    }

    fn take_release(&self, held: &AlbumHeld, release: &Release) -> Asked<()> {
        self.catalog
            .land_album_release(&held.key, release, SystemTime::now())?;
        Tally::count(&self.tally.matched, 1);
        if self.wants_a_cover(held)? {
            match (release.has_front_cover, &release.group) {
                (true, group) => self.want(Picture::Cover {
                    album: held.key.clone(),
                    release: release.id.clone(),
                    group: group.clone(),
                }),
                (false, Some(group)) => self.want(Picture::GroupCover {
                    album: held.key.clone(),
                    group: group.clone(),
                }),
                (false, None) => {}
            }
        }
        self.catalog.bill(&release.credit)?;
        Ok(())
    }

    fn take_group(&self, held: &AlbumHeld, group: &ReleaseGroup) -> Asked<()> {
        self.catalog
            .land_album_group(&held.key, &group.id, SystemTime::now())?;
        if self.wants_a_cover(held)? {
            self.want(Picture::GroupCover {
                album: held.key.clone(),
                group: group.id.clone(),
            });
        }
        self.catalog.bill(&group.credit)?;
        Ok(())
    }

    fn wants_a_cover(&self, held: &AlbumHeld) -> Asked<bool> {
        if self.covered.borrow().contains(&held.key) {
            return Ok(false);
        }
        Ok(!self.catalog.covered_well(&held.key)?)
    }

    fn want(&self, picture: Picture) {
        match &picture {
            Picture::Cover { album, .. } | Picture::GroupCover { album, .. } => {
                self.covered.borrow_mut().insert(album.clone());
            }
            Picture::Portrait { artist, .. } => {
                self.pictured.borrow_mut().insert(artist.clone());
            }
        }
        if self.pictured_at_once.get() {
            fetch(self.catalog, self.asking, self.tally, picture);
        } else {
            self.pictures
                .want(self.catalog, self.asking, self.tally, picture);
        }
    }

    fn look_again_for_covers(&self, now: SystemTime) -> Asked<()> {
        for picture in self.catalog.covers_due(now)? {
            self.go_on()?;
            let (Picture::Cover { album, .. } | Picture::GroupCover { album, .. }) = &picture
            else {
                continue;
            };
            if self.covered.borrow().contains(album) || self.catalog.covered_well(album)? {
                continue;
            }
            self.want(picture);
        }
        Ok(())
    }

    fn track(&self, id: u64) -> Asked<()> {
        let Some(held) = self.catalog.track_to_ask(id)? else {
            return Ok(());
        };
        let since = self.refusals.get();

        let found = match self.by_isrc(&held.track)? {
            Some(found) => Some(found),
            None => match self.by_recording(&held.track)? {
                Some(found) => Some(found),
                None => self.by_search(&held.track)?,
            },
        };
        match found {
            Some((recording, _)) => self.take_recording(&held, &recording, since),
            None => {
                self.catalog
                    .stamp_track_asked(id, self.fruitlessly(since), SystemTime::now())?;
                Ok(())
            }
        }
    }

    fn by_isrc(&self, track: &TrackToAsk) -> Asked<Option<(Recording, Certainty)>> {
        let Some(isrc) = &track.isrc else {
            return Ok(None);
        };
        match self.ask(LookupOp::Isrc, |reference| {
            reference.recordings_of_isrc(isrc)
        })? {
            Heard::Answered(found) => match best_recording(found, track) {
                Some((named, certainty)) => Ok(Some((self.told_where_it_sits(named)?, certainty))),
                None => Ok(None),
            },
            Heard::Refused => Ok(None),
        }
    }

    fn by_recording(&self, track: &TrackToAsk) -> Asked<Option<(Recording, Certainty)>> {
        let Some(mbid) = &track.mbid else {
            return Ok(None);
        };
        Ok(self
            .recording(mbid)?
            .map(|recording| (recording, Certainty::Exactly)))
    }

    fn by_search(&self, track: &TrackToAsk) -> Asked<Option<(Recording, Certainty)>> {
        if recording_asked(track, Wording::Phrase).is_none() {
            return Ok(None);
        }
        let found = self.either_way(
            LookupOp::FindRecording,
            |reference, wording| match recording_asked(track, wording) {
                Some(asked) => reference.find_recording(&asked),
                None => Ok(Vec::new()),
            },
            |found| the_strict_recording(found, track),
            Option::is_some,
        )?;
        match found {
            Heard::Answered(Some(top)) => Ok(Some((
                self.told_where_it_sits(top.into_recording())?,
                Certainty::Nearly,
            ))),
            Heard::Answered(None) | Heard::Refused => Ok(None),
        }
    }

    fn told_where_it_sits(&self, named: Recording) -> Asked<Recording> {
        if !needs_its_releases_told(&named) {
            return Ok(named);
        }
        Ok(self.recording(&named.id)?.unwrap_or(named))
    }

    fn recording(&self, mbid: &Mbid) -> Asked<Option<Recording>> {
        Ok(
            match self.ask(LookupOp::Recording, |reference| reference.recording(mbid))? {
                Heard::Answered(found) => found,
                Heard::Refused => None,
            },
        )
    }

    fn take_recording(&self, held: &TrackHeld, recording: &Recording, since: u32) -> Asked<()> {
        let album = match &held.album {
            Some(key) => self.catalog.album_to_ask(key)?,
            None => None,
        };
        let release = best_release(recording, album.as_ref().map(|album| &album.album))
            .map(|release| release.id.clone());
        let artist = recording
            .credit
            .iter()
            .find_map(|credit| credit.mbid.as_ref())
            .map(Mbid::as_str);
        let id = held.track.id.get();

        self.catalog.learned(
            id,
            Some(recording.id.as_str()),
            release.as_ref().map(Mbid::as_str),
            artist,
            "",
        )?;
        self.catalog.stamp_track_answered(id, SystemTime::now())?;
        Tally::count(&self.tally.matched, 1);

        match (album, release) {
            (Some(album), Some(release)) if !album.answered => {
                self.land_release(&album, &release, since)
            }
            _ => Ok(()),
        }
    }

    fn artist(&self, mbid: &str) -> Asked<()> {
        let Ok(id) = Mbid::new(mbid) else {
            self.catalog
                .stamp_artist_asked(mbid, Fruitless::Missed, SystemTime::now())?;
            return Ok(());
        };
        let since = self.refusals.get();

        match self.ask(LookupOp::Artist, |reference| reference.artist(&id))? {
            Heard::Answered(Some(profile)) => {
                let now = SystemTime::now();
                self.catalog
                    .keep_artist(mbid, &information_of(&profile), now)?;
                if profile.may_be_pictured()
                    && !self.pictured.borrow().contains(mbid)
                    && self.catalog.wants_a_portrait(mbid, now)?
                {
                    self.want(Picture::Portrait {
                        artist: mbid.to_owned(),
                        links: profile.links,
                    });
                }
                Ok(())
            }
            Heard::Answered(None) | Heard::Refused => {
                self.catalog.stamp_artist_asked(
                    mbid,
                    self.fruitlessly(since),
                    SystemTime::now(),
                )?;
                Ok(())
            }
        }
    }
}

fn information_of(profile: &resonate_library::ArtistProfile) -> ArtistInformation {
    ArtistInformation {
        name: profile.name.clone(),
        kind: profile.kind.clone().unwrap_or_default(),
        country: profile.country.clone().unwrap_or_default(),
        area: profile.area.clone().unwrap_or_default(),
        genres: profile
            .genres
            .iter()
            .map(|genre| genre.name.clone())
            .collect(),
        links: profile.links.iter().map(|link| link.url.clone()).collect(),
        picture: None,
    }
}

struct Hearing<'a>(&'a Told);

impl Watching for Hearing<'_> {
    fn stopped(&self) -> bool {
        self.0.is_cancelled()
    }
}

fn measure_the_unmeasured(
    catalog: &Catalog,
    sources: &Sources,
    told: &Told,
    at_most: Option<usize>,
) {
    let mut owed = match catalog.studies_without_bands() {
        Ok(owed) => owed,
        Err(error) => {
            tracing::warn!(%error, "the studies owed their bands could not be read");
            return;
        }
    };
    if let Some(at_most) = at_most {
        owed.truncate(at_most);
    }

    for (track, uri) in owed {
        if told.is_cancelled() {
            return;
        }
        let at = match location(&uri) {
            Ok(at) => at,
            Err(error) => {
                tracing::debug!(%error, track, "a study's document has no location to measure");
                continue;
            }
        };
        match band_figures(sources, &at, None, &Hearing(told)) {
            Ok(figures) => {
                if let Err(error) = catalog.keep_bands(track, &figures) {
                    tracing::warn!(%error, track, "a study's bands were not kept");
                }
            }
            Err(resonate_analysis::Error::Stopped) => return,
            Err(error) => {
                tracing::debug!(%error, track, "a study's bands could not be measured, so they are measured another time");
            }
        }
    }
}

fn hear_the_due(
    catalog: &Catalog,
    sources: &Sources,
    asking: &Asking,
    told: &Told,
    tally: &Tally,
    due: Due,
    at_most: Option<usize>,
) {
    measure_the_unmeasured(catalog, sources, told, at_most);
    let Some(printer) = asking.printer.as_deref() else {
        return;
    };
    let mut tracks = match catalog.tracks_to_hear(due.refresh) {
        Ok(tracks) => tracks,
        Err(error) => {
            tracing::warn!(%error, "the tracks to hear could not be read");
            return;
        }
    };
    if let Some(at_most) = at_most {
        tracks.truncate(at_most);
    }
    Tally::count(&tally.due, tracks.len());

    let hearer = Hearer {
        catalog,
        sources,
        asking,
        printer,
        told,
        tally,
        refused_in_a_row: Cell::new(0),
    };
    for track in tracks {
        match hearer.hear(track) {
            Ok(()) => Tally::count(&tally.asked, 1),
            Err(Ended::Failed(error)) => {
                tracing::warn!(%error, track, "a track was not heard");
            }
            Err(Ended::Cancelled | Ended::Offline | Ended::Refused | Ended::Unreachable(_)) => {
                return;
            }
        }
    }
}

struct Hearer<'a> {
    catalog: &'a Catalog,
    sources: &'a Sources,
    asking: &'a Asking,
    printer: &'a dyn Fingerprints,
    told: &'a Told,
    tally: &'a Tally,
    refused_in_a_row: Cell<u32>,
}

impl Hearer<'_> {
    fn hear(&self, track: u64) -> Asked<()> {
        if self.told.is_cancelled() {
            return Err(Ended::Cancelled);
        }
        if !self.asking.is_reaching() {
            return Err(Ended::Offline);
        }
        let held = match self.catalog.track(track) {
            Ok(held) => held,
            Err(Error::UnknownTrack) => return Ok(()),
            Err(error) => return Err(error.into()),
        };

        if let Some(print) = self.catalog.kept_print(track)? {
            return self.recognised(track, held, print);
        }

        let studied = resonate_analysis::study(
            self.sources,
            &location(&held.uri)?,
            None,
            &Hearing(self.told),
        );
        let study = match studied {
            Ok(study) => study,
            Err(resonate_analysis::Error::Stopped) => return Err(Ended::Cancelled),
            Err(error) => {
                tracing::debug!(%error, track, "a track could not be studied");
                if !error.is_out_of_reach() {
                    self.catalog.note_unstudied(track)?;
                }
                return Ok(());
            }
        };
        self.catalog.keep_studied(track, &study)?;
        self.keep_identity(track, &held.uri);
        let Some(print) = study.print else {
            self.catalog.keep_agreement(track, Agreement::Unheard)?;
            return Ok(());
        };

        self.recognised(track, held, print)
    }

    fn recognised(&self, track: u64, held: Track, print: Chromaprint) -> Asked<()> {
        match recognised_through(self.catalog, self.printer, track, held, print) {
            Ok(_) => {
                self.refused_in_a_row.set(0);
                Ok(())
            }
            Err(Error::Reference { op, source }) => self.weigh_a_failure(op, *source),
            Err(error) => Err(error.into()),
        }
    }

    fn keep_identity(&self, track: u64, uri: &str) {
        let scanned = location(uri).and_then(|at| Ok(probe_scanned(self.sources, &at)?));
        match scanned {
            Ok(scanned) => {
                if let Err(error) = self.catalog.keep_identity(track, &scanned) {
                    tracing::debug!(%error, track, "the identity of a studied track was not kept");
                }
            }
            Err(error) => tracing::debug!(%error, track, "a studied track could not be identified"),
        }
    }

    fn weigh_a_failure(&self, op: LookupOp, source: resonate_library::Error) -> Asked<()> {
        match source {
            resonate_library::Error::Refused { .. }
            | resonate_library::Error::Unreadable { .. } => {
                Tally::count(&self.tally.refused, 1);
                let refused = self.refused_in_a_row.get().saturating_add(1);
                self.refused_in_a_row.set(refused);
                if refused >= RECOGNITIONS_REFUSED_BEFORE_GIVING_UP {
                    return Err(Ended::Refused);
                }
                Ok(())
            }
            resonate_library::Error::TooLarge { .. } => Ok(()),
            _ if !self.asking.is_reaching() => Err(Ended::Offline),
            source => Err(Ended::Unreachable(Box::new(Error::reference(op, source)))),
        }
    }
}

fn sing_the_due(
    catalog: &Catalog,
    asking: &Asking,
    told: &Told,
    tally: &Tally,
    due: Due,
    at_most: Option<usize>,
) {
    let tracks = match catalog.lyrics_due(due, None) {
        Ok(tracks) => tracks,
        Err(error) => {
            tracing::warn!(%error, "the lyrics due could not be read");
            return;
        }
    };
    let mut tracks = VecDeque::from(tracks);
    if let Some(at_most) = at_most {
        tracks.truncate(at_most);
    }
    Tally::count(&tally.due, tracks.len());

    let refused_in_a_row = Cell::new(0);
    let mut sung = BTreeSet::new();
    loop {
        if let Err(error) = sing_the_sought_first(catalog, told, tally, due, &mut tracks, &sung) {
            tracing::warn!(%error, "the lyrics of a song sought could not be read");
        }
        let Some(track) = tracks.pop_front() else {
            return;
        };
        if !sung.insert(track) {
            continue;
        }
        match sing(catalog, asking, told, tally, track, &refused_in_a_row) {
            Ok(()) => Tally::count(&tally.asked, 1),
            Err(Ended::Failed(error)) => {
                tracing::warn!(%error, track, "a track's lyrics were not kept");
            }
            Err(Ended::Cancelled | Ended::Offline | Ended::Refused | Ended::Unreachable(_)) => {
                return;
            }
        }
    }
}

fn sing_the_sought_first(
    catalog: &Catalog,
    told: &Told,
    tally: &Tally,
    due: Due,
    tracks: &mut VecDeque<u64>,
    sung: &BTreeSet<u64>,
) -> Result<()> {
    for sought in told.sung_first.taken() {
        if sung.contains(&sought) {
            continue;
        }
        if let Some(at) = tracks.iter().position(|queued| *queued == sought) {
            tracks.remove(at);
        } else if catalog.lyrics_due(due, Some(sought))?.is_empty() {
            continue;
        } else {
            Tally::count(&tally.due, 1);
        }
        tracks.push_front(sought);
    }
    Ok(())
}

fn sing(
    catalog: &Catalog,
    asking: &Asking,
    told: &Told,
    tally: &Tally,
    track: u64,
    refused_in_a_row: &Cell<u32>,
) -> Asked<()> {
    if told.is_cancelled() {
        return Err(Ended::Cancelled);
    }
    if !asking.is_reaching() {
        return Err(Ended::Offline);
    }
    let held = catalog.track(track)?;
    let Some(asked) = lyrics_asked(&held) else {
        return Ok(());
    };

    let told_text = match asking.reference.lyrics(&asked) {
        Ok(told_text) => {
            refused_in_a_row.set(0);
            told_text
        }
        Err(
            resonate_library::Error::Refused { .. } | resonate_library::Error::Unreadable { .. },
        ) => {
            Tally::count(&tally.refused, 1);
            refused_in_a_row.set(refused_in_a_row.get().saturating_add(1));
            if refused_in_a_row.get() >= REFUSALS_THAT_END_A_PASS {
                return Err(Ended::Refused);
            }
            return Ok(());
        }
        Err(resonate_library::Error::TooLarge { .. }) => return Ok(()),
        Err(_) if !asking.is_reaching() => return Err(Ended::Offline),
        Err(source) => {
            return Err(Ended::Unreachable(Box::new(Error::reference(
                LookupOp::Lyrics,
                source,
            ))));
        }
    };

    let kept = catalog.lyric_text(track)?;
    let Some(told_text) = told_text else {
        if kept.is_none() {
            catalog.keep_lyrics(track, None, None, FETCHED_LYRICS)?;
        } else {
            catalog.note_lyrics_asked(track)?;
        }
        return Ok(());
    };
    let held_text = kept.and_then(|(text, document, _)| {
        text.map(|text| LyricText {
            synced: is_synced(&text),
            text,
            lyricsfile: document,
        })
    });
    let richer = KeptLyrics {
        sung: held_text,
        taken: SystemTime::now(),
    }
    .richer_of(Some(told_text.clone()));
    if richer.as_ref() == Some(&told_text) {
        catalog.keep_lyrics(
            track,
            Some(&told_text.text),
            told_text.lyricsfile.as_deref(),
            FETCHED_LYRICS,
        )?;
        Tally::count(&tally.lyrics, 1);
    } else {
        catalog.note_lyrics_asked(track)?;
    }
    Ok(())
}

pub(crate) fn is_synced(text: &str) -> bool {
    SourceId::new(FETCHED_LYRICS)
        .ok()
        .and_then(|source| resonate_lyrics::read_lyrics(source, text).ok().flatten())
        .is_some_and(|lyrics| lyrics.lines().iter().any(|line| line.at.is_some()))
}

fn lyrics_asked(track: &Track) -> Option<LyricsAsked> {
    let artist = track.artist.trim();
    if artist.is_empty() || track.title.trim().is_empty() || track.title == track.name {
        return None;
    }
    Some(LyricsAsked {
        title: track.title.clone(),
        artist: artist.to_owned(),
        album: (!track.album.trim().is_empty()).then(|| track.album.clone()),
        length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
    })
}

fn nanos(at: SystemTime) -> i64 {
    at.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_nanos()).unwrap_or(i64::MAX)
    })
}

fn before(now: SystemTime, wait: Duration) -> i64 {
    nanos(now.checked_sub(wait).unwrap_or(UNIX_EPOCH))
}

fn seconds_before(now: SystemTime, wait: Duration) -> i64 {
    now.checked_sub(wait)
        .unwrap_or(UNIX_EPOCH)
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

fn unblank(text: &str) -> Option<String> {
    (!text.trim().is_empty()).then(|| text.to_owned())
}

fn year_of(date: &str) -> Option<i32> {
    let digits = date.get(..YEAR_DIGITS)?;
    digits
        .bytes()
        .all(|digit| digit.is_ascii_digit())
        .then(|| digits.parse().ok())
        .flatten()
}

fn stem_of(name: &str) -> String {
    Path::new(name).file_stem().map_or_else(
        || name.to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    )
}

fn read_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Read, source)
}

fn write_error(source: rusqlite::Error) -> Error {
    Error::store(StoreOp::Write, source)
}

fn fruitless_columns(why: Fruitless) -> (i64, bool) {
    (
        i64::from(why == Fruitless::Missed),
        why == Fruitless::Refused,
    )
}

struct AlbumRow {
    id: u64,
    album: String,
    artist: String,
    album_artist: String,
    date: String,
    release: Option<String>,
    release_group: Option<String>,
}

impl Catalog {
    fn carried_on(&self, refresh: bool, now: SystemTime) -> Result<Due> {
        let connection = self.0.lock();
        let left: Option<(i64, bool)> = connection
            .query_row(
                "SELECT started, refresh FROM enrichment WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(read_error)?;
        let due = match left {
            Some((started, carried)) if !refresh => Due {
                refresh: carried,
                started,
                now,
            },
            _ => Due {
                refresh,
                started: nanos(now),
                now,
            },
        };
        connection
            .execute(
                "INSERT INTO enrichment (id, started, refresh) VALUES (1, ?1, ?2)
                 ON CONFLICT(id) DO UPDATE SET started = excluded.started, refresh = excluded.refresh",
                params![due.started, due.refresh],
            )
            .map_err(write_error)?;
        Ok(due)
    }

    fn note_finished(&self) -> Result<()> {
        self.0
            .lock()
            .execute("DELETE FROM enrichment", [])
            .map_err(write_error)?;
        Ok(())
    }

    fn keys_due(&self, sql: &str, due: Due, only: Value) -> Result<Vec<Value>> {
        let connection = self.0.lock();
        let mut query = connection.prepare(sql).map_err(read_error)?;
        query
            .query_map(params_from_iter(due.parameters(only)), |row| row.get(0))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<Value>>>()
            .map_err(read_error)
    }

    fn albums_due(&self, due: Due, only: Value) -> Result<Vec<String>> {
        let sql = format!(
            "SELECT k.album_key FROM (SELECT album_key, min(id) AS first FROM documents
                WHERE album_key IS NOT NULL GROUP BY album_key) k
             LEFT JOIN album_art a ON a.album_key = k.album_key
             WHERE {} AND (?7 IS NULL OR k.album_key = ?7) ORDER BY k.first",
            due_where("a")
        );
        Ok(self
            .keys_due(&sql, due, only)?
            .into_iter()
            .filter_map(|key| match key {
                Value::Text(key) => Some(key),
                _ => None,
            })
            .collect())
    }

    fn tracks_to_hear(&self, refresh: bool) -> Result<Vec<u64>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT d.id FROM documents d LEFT JOIN studies s ON s.track = d.id
                 WHERE d.missing = 0 AND (?1 OR (s.agreement IS NULL AND NOT EXISTS (
                    SELECT 1 FROM unstudied u WHERE u.track = d.id AND u.under = ?2))
                    OR (s.studied_under = ?2 AND s.print IS NOT NULL AND s.recognised IS NULL))
                 ORDER BY d.downloaded IS NULL, d.downloaded DESC, d.id",
            )
            .map_err(read_error)?;
        query
            .query_map(params![refresh, resonate_analysis::JUDGED_UNDER], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(read_error)?
            .map(|id| id.map(|id| id as u64))
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn downloaded(&self) -> Result<Vec<(u64, Option<String>)>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT id, album_key FROM documents WHERE downloaded IS NOT NULL AND missing = 0
                 ORDER BY downloaded DESC, id DESC",
            )
            .map_err(read_error)?;
        query
            .query_map([], |row| Ok((row.get::<_, i64>(0)? as u64, row.get(1)?)))
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    pub(crate) fn note_downloaded(&self, track: u64) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE documents SET downloaded = unixepoch() WHERE id = ?1",
                [track as i64],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn tracks_due(&self, due: Due, only: Value) -> Result<Vec<u64>> {
        let sql = format!(
            "SELECT d.id FROM documents d WHERE {} AND (?7 IS NULL OR d.id = ?7) ORDER BY d.id",
            due_where("d")
        );
        Ok(self
            .keys_due(&sql, due, only)?
            .into_iter()
            .filter_map(|id| match id {
                Value::Integer(id) => u64::try_from(id).ok(),
                _ => None,
            })
            .collect())
    }

    fn artists_due(&self, due: Due, only: Value) -> Result<Vec<String>> {
        let sql = format!(
            "SELECT a.mbid FROM artists a
             LEFT JOIN (SELECT artist_mbid, max(downloaded) AS downloaded FROM documents
                WHERE downloaded IS NOT NULL GROUP BY artist_mbid) w ON w.artist_mbid = a.mbid
             WHERE {} AND (?7 IS NULL OR a.mbid = ?7)
             ORDER BY w.downloaded IS NULL, w.downloaded DESC, a.rowid",
            due_where("a")
        );
        Ok(self
            .keys_due(&sql, due, only)?
            .into_iter()
            .filter_map(|mbid| match mbid {
                Value::Text(mbid) => Some(mbid),
                _ => None,
            })
            .collect())
    }

    fn lyrics_due(&self, due: Due, only: Option<u64>) -> Result<Vec<u64>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT d.id FROM documents d LEFT JOIN lyrics l ON l.track = d.id
                 WHERE (?1 OR l.track IS NULL
                    OR (l.text IS NULL AND l.asked <= ?2)
                    OR (l.text IS NOT NULL AND l.document IS NULL AND l.origin IN (?4, ?5) AND l.asked <= ?3))
                    AND (?6 IS NULL OR d.id = ?6)
                 ORDER BY d.downloaded IS NULL, d.downloaded DESC, d.id",
            )
            .map_err(read_error)?;
        query
            .query_map(
                params![
                    due.refresh,
                    seconds_before(due.now, MISSED_AGAIN_AFTER),
                    seconds_before(due.now, BETTERED_AFTER),
                    FETCHED_LYRICS,
                    SHARED_LYRICS,
                    only.map(|id| id as i64)
                ],
                |row| row.get::<_, i64>(0).map(|id| id as u64),
            )
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)
    }

    fn note_lyrics_asked(&self, track: u64) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE lyrics SET asked = unixepoch() WHERE track = ?1",
                [track as i64],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn take_in_artists(&self) -> Result<()> {
        self.0
            .lock()
            .execute(
                "INSERT OR IGNORE INTO artists (mbid, name)
                 SELECT artist_mbid, min(artist) FROM documents
                 WHERE artist_mbid IS NOT NULL GROUP BY artist_mbid",
                [],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn bill(&self, credit: &[resonate_library::Credit]) -> Result<()> {
        let connection = self.0.lock();
        for credited in credit {
            if let Some(mbid) = &credited.mbid {
                connection
                    .execute(
                        "INSERT OR IGNORE INTO artists (mbid, name) VALUES (?1, ?2)",
                        params![mbid.as_str(), credited.name],
                    )
                    .map_err(write_error)?;
            }
        }
        Ok(())
    }

    pub(crate) fn album_key_of(&self, track: u64) -> Result<Option<String>> {
        Ok(self
            .0
            .lock()
            .query_row(
                "SELECT album_key FROM documents WHERE id = ?1",
                [track as i64],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?
            .flatten())
    }

    fn album_to_ask(&self, key: &str) -> Result<Option<AlbumHeld>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT id, album, artist, album_artist, date, release, release_group
                 FROM documents WHERE album_key = ?1 ORDER BY id",
            )
            .map_err(read_error)?;
        let rows = query
            .query_map([key], |row| {
                Ok(AlbumRow {
                    id: row.get::<_, i64>(0)? as u64,
                    album: row.get(1)?,
                    artist: row.get(2)?,
                    album_artist: row.get(3)?,
                    date: row.get(4)?,
                    release: row.get(5)?,
                    release_group: row.get(6)?,
                })
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        let landed: Option<(Option<String>, Option<String>, Option<i64>)> = connection
            .query_row(
                "SELECT release, release_group, answered FROM album_art WHERE album_key = ?1",
                [key],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(read_error)?;
        drop(query);
        drop(connection);

        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let (landed_release, landed_group, answered) = landed.unwrap_or_default();
        let owner = unblank(&first.album_artist).or_else(|| unblank(&first.artist));
        let mut performers: Vec<String> = Vec::new();
        for row in &rows {
            if performers.len() < PERFORMERS_WEIGHED_AT_MOST
                && !row.artist.trim().is_empty()
                && !performers.contains(&row.artist)
            {
                performers.push(row.artist.clone());
            }
        }
        let tagged = |pick: fn(&AlbumRow) -> Option<&String>| {
            rows.iter().find_map(pick).and_then(|id| Mbid::new(id).ok())
        };

        Ok(Some(AlbumHeld {
            key: key.to_owned(),
            answered: answered.is_some(),
            album: AlbumToAsk {
                id: AlbumId::new(first.id)?,
                title: first.album.clone(),
                owner,
                track_count: u32::try_from(rows.len()).unwrap_or(u32::MAX),
                year: rows.iter().find_map(|row| year_of(&row.date)),
                mbid: landed_release
                    .and_then(|id| Mbid::new(&id).ok())
                    .or_else(|| tagged(|row| row.release.as_ref())),
                group: landed_group
                    .and_then(|id| Mbid::new(&id).ok())
                    .or_else(|| tagged(|row| row.release_group.as_ref())),
                barcode: None,
                catalog_number: None,
                tagged_tracks: None,
                owner_mbid: None,
                performers,
                has_cover: false,
                has_release_rows: false,
                rematch_only: false,
            },
        }))
    }

    fn track_to_ask(&self, id: u64) -> Result<Option<TrackHeld>> {
        let track = match self.track(id) {
            Ok(track) => track,
            Err(Error::UnknownTrack) => return Ok(None),
            Err(error) => return Err(error),
        };
        let connection = self.0.lock();
        let (isrc, album): (Option<String>, Option<String>) = connection
            .query_row(
                "SELECT isrc, album_key FROM documents WHERE id = ?1",
                [id as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(read_error)?;
        let album_id: Option<i64> = match &album {
            Some(key) => connection
                .query_row(
                    "SELECT min(id) FROM documents WHERE album_key = ?1",
                    [key],
                    |row| row.get(0),
                )
                .map_err(read_error)?,
            None => None,
        };
        let album_answered = match &album {
            Some(key) => connection
                .query_row(
                    "SELECT answered IS NOT NULL FROM album_art WHERE album_key = ?1",
                    [key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(read_error)?
                .unwrap_or(false),
            None => false,
        };
        drop(connection);

        let untagged = track.title == track.name;
        let artist = unblank(&track.artist);
        Ok(Some(TrackHeld {
            album,
            track: TrackToAsk {
                id: TrackId::new(id)?,
                title: if untagged {
                    stem_of(&track.name)
                } else {
                    track.title.clone()
                },
                tagged_title: (!untagged).then(|| track.title.clone()),
                artist: artist.clone(),
                tagged_artist: artist,
                artist_mbid: track
                    .artist_mbid
                    .as_deref()
                    .and_then(|id| Mbid::new(id).ok()),
                album: album_id
                    .and_then(|id| u64::try_from(id).ok())
                    .and_then(|id| AlbumId::new(id).ok()),
                album_title: unblank(&track.album),
                album_answered,
                owner: unblank(&track.album_artist),
                owner_mbid: None,
                location: location(&track.uri)?,
                span: None,
                length: (track.duration_ms > 0).then(|| Duration::from_millis(track.duration_ms)),
                mbid: track.recording.as_deref().and_then(|id| Mbid::new(id).ok()),
                isrc: isrc.as_deref().and_then(|isrc| Isrc::new(isrc).ok()),
            },
        }))
    }

    fn stamp_album_asked(&self, key: &str, why: Fruitless, now: SystemTime) -> Result<()> {
        let (asks, refused) = fruitless_columns(why);
        self.0
            .lock()
            .execute(
                "INSERT INTO album_art (album_key, asked, asks, refusals) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(album_key) DO UPDATE SET asked = excluded.asked, asks = asks + ?3,
                 refusals = CASE WHEN ?4 THEN refusals + 1 ELSE 0 END",
                params![key, nanos(now), asks, i64::from(refused)],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn stamp_track_asked(&self, id: u64, why: Fruitless, now: SystemTime) -> Result<()> {
        let (asks, refused) = fruitless_columns(why);
        self.0
            .lock()
            .execute(
                "UPDATE documents SET asked = ?2, asks = asks + ?3,
                 refusals = CASE WHEN ?4 THEN refusals + 1 ELSE 0 END WHERE id = ?1",
                params![id as i64, nanos(now), asks, refused],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn stamp_track_answered(&self, id: u64, now: SystemTime) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE documents SET asked = ?2, answered = ?2, asks = 0, refusals = 0 WHERE id = ?1",
                params![id as i64, nanos(now)],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn stamp_artist_asked(&self, mbid: &str, why: Fruitless, now: SystemTime) -> Result<()> {
        let (asks, refused) = fruitless_columns(why);
        self.0
            .lock()
            .execute(
                "UPDATE artists SET asked = ?2, asks = asks + ?3,
                 refusals = CASE WHEN ?4 THEN refusals + 1 ELSE 0 END WHERE mbid = ?1",
                params![mbid, nanos(now), asks, refused],
            )
            .map_err(write_error)?;
        Ok(())
    }

    fn land_album_release(&self, key: &str, release: &Release, now: SystemTime) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let group = release.group.as_ref().map(Mbid::as_str);
        transaction
            .execute(
                "INSERT INTO album_art (album_key, release, release_group, asked, answered)
                 VALUES (?1, ?2, ?3, ?4, ?4)
                 ON CONFLICT(album_key) DO UPDATE SET release = excluded.release,
                 release_group = excluded.release_group, asked = excluded.asked,
                 answered = excluded.answered, asks = 0, refusals = 0",
                params![key, release.id.as_str(), group, nanos(now)],
            )
            .map_err(write_error)?;
        transaction
            .execute(
                "UPDATE documents SET release = coalesce(release, ?2),
                 release_group = coalesce(release_group, ?3) WHERE album_key = ?1",
                params![key, release.id.as_str(), group],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)
    }

    fn land_album_group(&self, key: &str, group: &Mbid, now: SystemTime) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        transaction
            .execute(
                "INSERT INTO album_art (album_key, release_group, asked, answered)
                 VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(album_key) DO UPDATE SET release_group = excluded.release_group,
                 asked = excluded.asked, answered = excluded.answered, asks = 0, refusals = 0",
                params![key, group.as_str(), nanos(now)],
            )
            .map_err(write_error)?;
        transaction
            .execute(
                "UPDATE documents SET release_group = coalesce(release_group, ?2) WHERE album_key = ?1",
                params![key, group.as_str()],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)
    }

    fn covered_well(&self, key: &str) -> Result<bool> {
        let held: Option<Vec<u8>> = self
            .0
            .lock()
            .query_row(
                "SELECT coalesce(
                    (SELECT p.bytes FROM album_art a JOIN pictures p ON p.id = a.picture WHERE a.album_key = ?1),
                    (SELECT p.bytes FROM artwork w JOIN pictures p ON p.id = w.picture
                      JOIN documents d ON d.id = w.track
                      WHERE d.album_key = ?1 ORDER BY d.id LIMIT 1))",
                [key],
                |row| row.get(0),
            )
            .map_err(read_error)?;
        Ok(held.is_some_and(|bytes| {
            ImageFormat::sniff(&bytes)
                .and_then(|format| CoverArt { format, bytes }.shorter_side())
                .is_none_or(|side| side >= A_THUMBNAIL_BELOW)
        }))
    }

    fn covers_due(&self, now: SystemTime) -> Result<Vec<Picture>> {
        let connection = self.0.lock();
        let mut query = connection
            .prepare(
                "SELECT album_key, release, release_group FROM album_art
                 WHERE picture IS NULL AND (release IS NOT NULL OR release_group IS NOT NULL)
                   AND (cover_asked IS NULL OR cover_asked < ?1)
                 ORDER BY album_key",
            )
            .map_err(read_error)?;
        let due = query
            .query_map([before(now, COVERS_ASKED_AGAIN_AFTER)], |row| {
                let named = |at| {
                    row.get::<_, Option<String>>(at)
                        .map(|id| id.and_then(|id| Mbid::new(&id).ok()))
                };
                Ok((row.get::<_, String>(0)?, named(1)?, named(2)?))
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(due
            .into_iter()
            .filter_map(|(album, release, group)| match (release, group) {
                (Some(release), group) => Some(Picture::Cover {
                    album,
                    release,
                    group,
                }),
                (None, Some(group)) => Some(Picture::GroupCover { album, group }),
                (None, None) => None,
            })
            .collect())
    }

    fn land_cover(&self, key: &str, found: Option<&CoverArt>, covers: &AtomicU32) -> Result<()> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let now = nanos(SystemTime::now());
        match found {
            Some(found) => {
                let picture = kept_picture(&transaction, &found.bytes).map_err(write_error)?;
                let before: Option<i64> = transaction
                    .query_row(
                        "SELECT picture FROM album_art WHERE album_key = ?1",
                        [key],
                        |row| row.get(0),
                    )
                    .optional()
                    .map_err(write_error)?
                    .flatten();
                transaction
                    .execute(
                        "INSERT INTO album_art (album_key, picture, origin, cover_asked) VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(album_key) DO UPDATE SET picture = excluded.picture,
                         origin = excluded.origin, cover_asked = excluded.cover_asked",
                        params![key, picture, ARCHIVED_COVER, now],
                    )
                    .map_err(write_error)?;
                if let Some(before) = before.filter(|before| *before != picture) {
                    unreferenced_picture_dropped(&transaction, before).map_err(write_error)?;
                }
                transaction
                    .execute(
                        "UPDATE documents SET art_revision = art_revision + 1 WHERE album_key = ?1",
                        [key],
                    )
                    .map_err(write_error)?;
                Tally::count(covers, 1);
            }
            None => {
                transaction
                    .execute(
                        "INSERT INTO album_art (album_key, cover_asked) VALUES (?1, ?2)
                         ON CONFLICT(album_key) DO UPDATE SET cover_asked = excluded.cover_asked",
                        params![key, now],
                    )
                    .map_err(write_error)?;
            }
        }
        transaction.commit().map_err(write_error)
    }

    fn wants_a_portrait(&self, mbid: &str, now: SystemTime) -> Result<bool> {
        Ok(self
            .0
            .lock()
            .query_row(
                "SELECT portrait IS NULL AND (portrait_asked IS NULL OR portrait_asked < ?2)
                 FROM artists WHERE mbid = ?1",
                params![mbid, before(now, PORTRAITS_ASKED_AGAIN_AFTER)],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?
            .unwrap_or(true))
    }

    fn land_portrait(
        &self,
        mbid: &str,
        found: Option<&CoverArt>,
        portraits: &AtomicU32,
    ) -> Result<()> {
        self.0
            .lock()
            .execute(
                "UPDATE artists SET portrait = coalesce(?2, portrait), portrait_asked = ?3 WHERE mbid = ?1",
                params![mbid, found.map(|found| &found.bytes), nanos(SystemTime::now())],
            )
            .map_err(write_error)?;
        if found.is_some() {
            Tally::count(portraits, 1);
        }
        Ok(())
    }

    pub(crate) fn keep_artist(
        &self,
        mbid: &str,
        information: &ArtistInformation,
        now: SystemTime,
    ) -> Result<()> {
        let listed = |values: &[String]| values.join(&LISTED_APART.to_string());
        let asked_now = nanos(now);
        self.0
            .lock()
            .execute(
                "INSERT INTO artists (mbid, name, kind, country, area, genres, links, portrait,
                    portrait_asked, asked, answered)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, CASE WHEN ?8 IS NULL THEN NULL ELSE ?9 END, ?9, ?9)
                 ON CONFLICT(mbid) DO UPDATE SET name = excluded.name, kind = excluded.kind,
                    country = excluded.country, area = excluded.area, genres = excluded.genres,
                    links = excluded.links, portrait = coalesce(excluded.portrait, portrait),
                    portrait_asked = coalesce(excluded.portrait_asked, portrait_asked),
                    asked = excluded.asked, answered = excluded.answered, asks = 0, refusals = 0",
                params![
                    mbid,
                    information.name,
                    information.kind,
                    information.country,
                    information.area,
                    listed(&information.genres),
                    listed(&information.links),
                    information.picture,
                    asked_now
                ],
            )
            .map_err(write_error)?;
        Ok(())
    }

    pub(crate) fn kept_artist(&self, mbid: &str) -> Result<Option<ArtistInformation>> {
        let listed = |values: Option<String>| {
            values.map_or_else(Vec::new, |values| {
                values
                    .split(LISTED_APART)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
        };
        self.0
            .lock()
            .query_row(
                "SELECT name, kind, country, area, genres, links, portrait FROM artists
                 WHERE mbid = ?1 AND answered IS NOT NULL",
                [mbid],
                |row| {
                    Ok(ArtistInformation {
                        name: row.get(0)?,
                        kind: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                        country: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                        area: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                        genres: listed(row.get(4)?),
                        links: listed(row.get(5)?),
                        picture: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(read_error)
    }

    fn portrait(&self, mbid: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .0
            .lock()
            .query_row(
                "SELECT portrait FROM artists WHERE mbid = ?1",
                [mbid],
                |row| row.get(0),
            )
            .optional()
            .map_err(read_error)?
            .flatten())
    }

    fn ask_again_for_covers(&self) -> Result<u32> {
        let mut connection = self.0.lock();
        let transaction = connection.transaction().map_err(write_error)?;
        let cleared = transaction
            .execute(
                "UPDATE album_art SET cover_asked = NULL WHERE picture IS NULL AND cover_asked IS NOT NULL",
                [],
            )
            .map_err(write_error)?;
        let unmatched = transaction
            .execute(
                "UPDATE album_art SET asked = NULL
                 WHERE picture IS NULL AND release IS NULL AND release_group IS NULL
                   AND answered IS NULL AND asked IS NOT NULL
                   AND NOT EXISTS (SELECT 1 FROM artwork w JOIN documents d ON d.id = w.track
                                    WHERE d.album_key = album_art.album_key)",
                [],
            )
            .map_err(write_error)?;
        transaction.commit().map_err(write_error)?;
        Ok(u32::try_from(cleared + unmatched).unwrap_or(u32::MAX))
    }

    fn art_changed_since(&self, revision: u64) -> Result<ChangedArt> {
        let connection = self.0.lock();
        let current: i64 = connection
            .query_row(
                "SELECT coalesce(max(art_changed), 0) FROM documents",
                [],
                |row| row.get(0),
            )
            .map_err(read_error)?;
        let mut query = connection
            .prepare("SELECT id FROM documents WHERE art_changed > ?1 ORDER BY art_changed")
            .map_err(read_error)?;
        let tracks = query
            .query_map([i64::try_from(revision).unwrap_or(i64::MAX)], |row| {
                row.get::<_, i64>(0).map(|id| id as u64)
            })
            .map_err(read_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(read_error)?;
        Ok(ChangedArt {
            revision: u64::try_from(current).unwrap_or_default(),
            tracks,
        })
    }

    pub(crate) fn pictured(&self, id: u64) -> Result<Option<Vec<u8>>> {
        match self.picture_of(id)? {
            Some(picture) => self.picture(picture as u64),
            None => Ok(None),
        }
    }

    pub(crate) fn picture_of(&self, id: u64) -> Result<Option<i64>> {
        let (own, album): (Option<i64>, Option<i64>) = self
            .0
            .lock()
            .query_row(
                "SELECT (SELECT picture FROM artwork WHERE track = d.id),
                        (SELECT a.picture FROM album_art a WHERE a.album_key = d.album_key)
                 FROM documents d WHERE d.id = ?1",
                [id as i64],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(read_error)?
            .unwrap_or((None, None));

        match (own, album) {
            (Some(own), Some(album)) if own != album => self.better_of(own, album).map(Some),
            (own, album) => Ok(own.or(album)),
        }
    }

    fn better_of(&self, own: i64, album: i64) -> Result<i64> {
        if let Some(chosen) = self.2.lock().get(&(own, album)) {
            return Ok(*chosen);
        }
        let (Some(held), Some(candidate)) =
            (self.picture(own as u64)?, self.picture(album as u64)?)
        else {
            return Ok(own);
        };
        let chosen = if album_betters_own(held, candidate) {
            album
        } else {
            own
        };
        self.2.lock().insert((own, album), chosen);
        Ok(chosen)
    }
}

fn album_betters_own(own: Vec<u8>, album: Vec<u8>) -> bool {
    let (Some(own_format), Some(album_format)) =
        (ImageFormat::sniff(&own), ImageFormat::sniff(&album))
    else {
        return false;
    };
    betters(
        &CoverArt {
            format: album_format,
            bytes: album,
        },
        &CoverArt {
            format: own_format,
            bytes: own,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::{env, sync::atomic::AtomicUsize, time::Instant};

    use resonate_codec::Sources;
    use resonate_library::{
        AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
        Barcode, BarcodeMatch, Credit, Discography, GroupAsked, GroupMatch, LinkNames,
        LinkedPlaylist, PlaylistLink, RecordingAsked, RecordingMatch, ReleaseAsked, ReleaseMatch,
        SongLink, SongsAsked, StreamAsked,
    };
    use resonate_online::Identity;

    use super::*;
    use crate::{PlaybackPreferences, features::Services};

    const RELEASE: &str = "f5093c06-23e3-404f-aeaa-40f72885ee3a";

    const GROUP: &str = "1dc4c347-a1db-32aa-b14f-bc9cc507b843";

    const ARTIST: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";

    const COVER: [u8; 4] = [7, 7, 7, 7];

    #[derive(Default)]
    struct Fake {
        calls: AtomicUsize,
        covers: AtomicUsize,
        each_takes: Duration,
        asked: Mutex<Vec<String>>,
    }

    impl Fake {
        fn answered<T>(&self, answer: T) -> resonate_library::Result<T> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            thread::sleep(self.each_takes);
            Ok(answer)
        }

        fn noted(&self, asked: String) {
            self.asked.lock().push(asked);
        }

        fn first_asked(&self, kind: &str) -> Option<String> {
            self.asked
                .lock()
                .iter()
                .find_map(|asked| asked.strip_prefix(kind).map(str::to_owned))
        }

        fn credit() -> Vec<Credit> {
            vec![Credit {
                name: "Artist".into(),
                joined_by: String::new(),
                mbid: Mbid::new(ARTIST).ok(),
            }]
        }
    }

    impl Reference for Fake {
        fn source(&self) -> &SourceId {
            unreachable!("the pass never asks the reference its name")
        }

        fn release(&self, id: &Mbid) -> resonate_library::Result<Option<Release>> {
            self.answered((id.as_str() == RELEASE).then(|| Release {
                id: id.clone(),
                group: Mbid::new(GROUP).ok(),
                title: "Album".into(),
                credit: Self::credit(),
                date: None,
                country: None,
                label: None,
                catalog_number: None,
                barcode: None,
                kind: None,
                disambiguation: None,
                has_front_cover: true,
                links: Vec::new(),
                media: Vec::new(),
            }))
        }

        fn find_release(
            &self,
            asked: &ReleaseAsked,
        ) -> resonate_library::Result<Vec<ReleaseMatch>> {
            self.noted(format!("album {}", asked.title));
            self.answered(
                (asked.title == "Album")
                    .then(|| ReleaseMatch {
                        release: Mbid::new(RELEASE).expect("an mbid"),
                        group: Mbid::new(GROUP).ok(),
                        score: 100,
                        title: "Album".into(),
                        credit: Self::credit(),
                        track_count: Some(10),
                        date: None,
                    })
                    .into_iter()
                    .collect(),
            )
        }

        fn releases_by_barcode(
            &self,
            _barcode: &Barcode,
        ) -> resonate_library::Result<Vec<BarcodeMatch>> {
            self.answered(Vec::new())
        }

        fn recording(&self, _id: &Mbid) -> resonate_library::Result<Option<Recording>> {
            self.answered(None)
        }

        fn recordings_of_isrc(&self, _isrc: &Isrc) -> resonate_library::Result<Vec<Recording>> {
            self.answered(Vec::new())
        }

        fn find_recording(
            &self,
            _asked: &RecordingAsked,
        ) -> resonate_library::Result<Vec<RecordingMatch>> {
            self.answered(Vec::new())
        }

        fn find_songs(&self, _asked: &SongsAsked) -> resonate_library::Result<Vec<RecordingMatch>> {
            self.answered(Vec::new())
        }

        fn release_group(&self, _id: &Mbid) -> resonate_library::Result<Option<ReleaseGroup>> {
            self.answered(None)
        }

        fn find_release_group(
            &self,
            _asked: &GroupAsked,
        ) -> resonate_library::Result<Vec<GroupMatch>> {
            self.answered(Vec::new())
        }

        fn find_albums(&self, _words: &str) -> resonate_library::Result<Vec<AlbumMatch>> {
            self.answered(Vec::new())
        }

        fn group_cover(&self, _group: &Mbid) -> resonate_library::Result<Option<CoverArt>> {
            self.covers.fetch_add(1, Ordering::SeqCst);
            self.answered(None)
        }

        fn artist(&self, _id: &Mbid) -> resonate_library::Result<Option<ArtistProfile>> {
            self.answered(None)
        }

        fn find_artist(&self, _name: &str) -> resonate_library::Result<Vec<ArtistMatch>> {
            self.answered(Vec::new())
        }

        fn release_groups_of(
            &self,
            _artist: &Mbid,
            _from: u32,
        ) -> resonate_library::Result<Discography> {
            self.answered(Discography::default())
        }

        fn releases_of_group(&self, _group: &Mbid) -> resonate_library::Result<Vec<Release>> {
            self.answered(Vec::new())
        }

        fn releases_of_artist(
            &self,
            _artist: &Mbid,
            _from: u32,
        ) -> resonate_library::Result<ArtistPressings> {
            self.answered(ArtistPressings::default())
        }

        fn cover(
            &self,
            _release: &Mbid,
            _group: Option<&Mbid>,
        ) -> resonate_library::Result<Option<CoverArt>> {
            self.covers.fetch_add(1, Ordering::SeqCst);
            self.answered(Some(CoverArt {
                format: ImageFormat::Png,
                bytes: COVER.to_vec(),
            }))
        }

        fn portrait(&self, _links: &[Link]) -> resonate_library::Result<Option<CoverArt>> {
            self.answered(None)
        }

        fn streamed_at(&self, _asked: &StreamAsked) -> resonate_library::Result<Option<Link>> {
            self.answered(None)
        }

        fn lyrics(&self, asked: &LyricsAsked) -> resonate_library::Result<Option<LyricText>> {
            self.noted(format!("words {}", asked.title));
            self.answered(None)
        }

        fn song_linked(&self, _link: &SongLink) -> resonate_library::Result<Option<LinkNames>> {
            self.answered(None)
        }

        fn album_linked(&self, _link: &AlbumLink) -> resonate_library::Result<Option<AlbumNames>> {
            self.answered(None)
        }

        fn artist_linked(&self, _link: &ArtistLink) -> resonate_library::Result<Option<String>> {
            self.answered(None)
        }

        fn playlist_linked(
            &self,
            _link: &PlaylistLink,
        ) -> resonate_library::Result<Option<LinkedPlaylist>> {
            self.answered(None)
        }

        fn artist_at(&self, _page: &str) -> resonate_library::Result<Option<Mbid>> {
            self.answered(None)
        }
    }

    fn asking(fake: &Arc<Fake>) -> Asking {
        Asking {
            reference: Arc::clone(fake) as Arc<dyn Reference>,
            client: Arc::new(Client::new(Identity::of_this_build())),
            printer: None,
        }
    }

    fn player() -> MobilePlayer {
        MobilePlayer {
            playing: crate::zoning::Playing::closed(),
            catalog: Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog")),
            sources: Arc::new(Sources::local()),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: crate::downloads::Downloads::default(),
            syncing: crate::sync::Syncing::under(env::temp_dir()),
            enriching: Enriching::default(),
            room: crate::room::Room::under(std::env::temp_dir()),
            tagging: crate::tagging::Tagging::under(std::env::temp_dir()),
            served: crate::served::Served::default(),
            rendering: crate::rendering::Rendering::default(),
        }
    }

    fn songs(catalog: &Catalog, album: &str, count: u32) -> Vec<u64> {
        (1..=count)
            .map(|number| {
                catalog
                    .upsert(Track {
                        uri: format!("content://test/{album}-{number}.flac"),
                        name: format!("{album}-{number}.flac"),
                        title: format!("Song {number}"),
                        artist: "Artist".into(),
                        album: album.into(),
                        duration_ms: 180_000,
                        ..Track::default()
                    })
                    .expect("an indexed song")
                    .id
            })
            .collect()
    }

    fn passed(catalog: &Arc<Catalog>, fake: &Arc<Fake>) -> EnrichmentStatus {
        passed_as_told(catalog, fake, &Arc::new(Told::default()))
    }

    fn passed_as_told(
        catalog: &Arc<Catalog>,
        fake: &Arc<Fake>,
        told: &Arc<Told>,
    ) -> EnrichmentStatus {
        let tally = Arc::new(Tally::default());
        tally.began();

        run(
            catalog,
            &Arc::new(Sources::local()),
            &asking(fake),
            told,
            &tally,
            EnrichmentOptions::default(),
        );

        tally.status()
    }

    fn albums_of_one_song(catalog: &Catalog, albums: &[&str]) -> Vec<u64> {
        albums
            .iter()
            .map(|album| {
                catalog
                    .upsert(Track {
                        uri: format!("content://test/{album}.flac"),
                        name: format!("{album}.flac"),
                        title: format!("{album} song"),
                        artist: "Artist".into(),
                        album: (*album).into(),
                        duration_ms: 180_000,
                        ..Track::default()
                    })
                    .expect("an indexed song")
                    .id
            })
            .collect()
    }

    fn marks(catalog: &Catalog) -> i64 {
        catalog
            .0
            .lock()
            .query_row("SELECT count(*) FROM enrichment", [], |row| row.get(0))
            .expect("the enrichment mark")
    }

    #[test]
    fn one_album_of_ten_songs_asks_for_one_cover_that_every_song_shows() {
        let catalog = Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog"));
        let fake = Arc::new(Fake::default());

        let tracks = songs(&catalog, "Album", 10);
        let status = passed(&catalog, &fake);
        let changed = catalog.art_changed_since(0).expect("changed pictures");

        assert_eq!(status.stopped, Some(EnrichmentStop::Finished));
        assert_eq!(fake.covers.load(Ordering::SeqCst), 1);
        assert_eq!(status.covers, 1);
        assert_eq!(marks(&catalog), 0);
        assert_eq!(changed.tracks.len(), tracks.len());
        assert!(
            catalog
                .art_changed_since(changed.revision)
                .expect("nothing newer")
                .tracks
                .is_empty()
        );
        for track in &tracks {
            assert_eq!(
                catalog.pictured(*track).expect("a picture"),
                Some(COVER.to_vec())
            );
            assert_eq!(catalog.artwork(*track).expect("no own picture"), None);
            assert_eq!(
                catalog.track(*track).expect("a track").release.as_deref(),
                Some(RELEASE)
            );
        }
    }

    #[test]
    fn a_downloaded_song_is_looked_up_before_the_songs_held_before_it() {
        let catalog = Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog"));
        let fake = Arc::new(Fake::default());

        albums_of_one_song(&catalog, &["Older", "Old"]);
        let fetched = albums_of_one_song(&catalog, &["Fetched", "Newest"]);
        albums_of_one_song(&catalog, &["Scanned after"]);
        for track in &fetched {
            catalog.note_downloaded(*track).expect("a download noted");
        }
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET downloaded = downloaded - 60 WHERE id = ?1",
                [fetched[0] as i64],
            )
            .expect("an earlier download");
        let status = passed(&catalog, &fake);

        assert_eq!(status.stopped, Some(EnrichmentStop::Finished));
        assert_eq!(fake.first_asked("album ").as_deref(), Some("Newest"));
        assert_eq!(fake.first_asked("words ").as_deref(), Some("Newest song"));
        let mut albums: Vec<String> = fake
            .asked
            .lock()
            .iter()
            .filter_map(|asked| asked.strip_prefix("album ").map(str::to_owned))
            .collect();
        albums.dedup();
        assert_eq!(albums.iter().position(|album| album == "Fetched"), Some(1));
    }

    #[test]
    fn a_song_sought_has_its_album_and_words_asked_before_the_rest() {
        let catalog = Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog"));
        let fake = Arc::new(Fake::default());
        let told = Arc::new(Told::default());

        let tracks = albums_of_one_song(&catalog, &["First", "Second", "Sought"]);
        told.seek(tracks[2]);
        let status = passed_as_told(&catalog, &fake, &told);

        assert_eq!(status.stopped, Some(EnrichmentStop::Finished));
        assert_eq!(fake.first_asked("album ").as_deref(), Some("Sought"));
        assert_eq!(fake.first_asked("words ").as_deref(), Some("Sought song"));
    }

    #[test]
    fn a_miss_is_not_asked_again_before_its_wait_is_over() {
        let catalog = Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog"));
        let fake = Arc::new(Fake::default());
        let tracks = songs(&catalog, "Unknown", 3);

        passed(&catalog, &fake);
        let first = fake.calls.swap(0, Ordering::SeqCst);
        passed(&catalog, &fake);
        let again = fake.calls.swap(0, Ordering::SeqCst);

        assert!(first > 0);
        assert_eq!(again, 0);

        let over_a_day_ago = nanos(SystemTime::now() - Duration::from_secs(25 * 60 * 60));
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET asked = ?2 WHERE id = ?1",
                params![tracks[0] as i64, over_a_day_ago],
            )
            .expect("an older ask");
        passed(&catalog, &fake);

        assert!(fake.calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn looking_again_for_covers_asks_again_about_an_album_nothing_matched() {
        let catalog = Arc::new(Catalog::open(Path::new(":memory:")).expect("a catalog"));
        let fake = Arc::new(Fake::default());
        songs(&catalog, "Unknown", 3);

        passed(&catalog, &fake);
        fake.calls.swap(0, Ordering::SeqCst);
        passed(&catalog, &fake);
        let waiting = fake.calls.swap(0, Ordering::SeqCst);
        let asked_again = catalog
            .ask_again_for_covers()
            .expect("the covers are asked again");
        passed(&catalog, &fake);

        assert_eq!(waiting, 0);
        assert!(asked_again > 0);
        assert!(fake.calls.load(Ordering::SeqCst) > 0);
    }

    #[test]
    fn stopping_ends_the_pass_between_requests() {
        let player = player();
        let fake = Arc::new(Fake {
            each_takes: Duration::from_millis(20),
            ..Fake::default()
        });
        songs(&player.catalog, "", 200);

        assert!(player.start_enriching(asking(&fake), EnrichmentOptions::default()));
        assert!(!player.start_enriching(asking(&fake), EnrichmentOptions::default()));
        let waited = Instant::now();
        while fake.calls.load(Ordering::SeqCst) == 0 && waited.elapsed() < Duration::from_secs(5) {
            thread::sleep(Duration::from_millis(5));
        }
        let stopping = Instant::now();
        player.stop_enrichment();
        let status = player.enrichment_status();

        assert!(stopping.elapsed() < Duration::from_secs(2));
        assert!(!status.running);
        assert_eq!(status.stopped, Some(EnrichmentStop::Cancelled));
        assert!(fake.calls.load(Ordering::SeqCst) < 200);
        assert_eq!(marks(&player.catalog), 0);
    }

    #[test]
    fn a_pass_closed_with_the_app_leaves_its_mark_to_carry_on() {
        let player = player();
        let fake = Arc::new(Fake {
            each_takes: Duration::from_millis(20),
            ..Fake::default()
        });
        songs(&player.catalog, "", 50);

        assert!(player.start_enriching(
            asking(&fake),
            EnrichmentOptions {
                refresh: true,
                ..EnrichmentOptions::default()
            }
        ));
        player.stop_enriching_to_close();
        let carried = player
            .catalog
            .carried_on(false, SystemTime::now())
            .expect("the carried mark");

        assert_eq!(marks(&player.catalog), 1);
        assert!(carried.refresh);
    }

    #[test]
    fn a_song_with_no_picture_of_its_own_shows_its_albums() {
        let player = player();
        let track = songs(&player.catalog, "Album", 1)[0];
        let key = player
            .catalog
            .album_key_of(track)
            .expect("a key")
            .expect("an album");
        player
            .catalog
            .land_cover(
                &key,
                Some(&CoverArt {
                    format: ImageFormat::Png,
                    bytes: COVER.to_vec(),
                }),
                &AtomicU32::new(0),
            )
            .expect("an album cover");

        assert_eq!(
            player.artwork(track).expect("a picture"),
            Some(COVER.to_vec())
        );

        player
            .catalog
            .keep_artwork(track, &[1, 2, 3], "embedded", true)
            .expect("an embedded picture");

        assert_eq!(
            player.artwork(track).expect("a picture"),
            Some(vec![1, 2, 3])
        );
    }

    #[test]
    fn the_album_key_folds_the_title_and_falls_back_to_the_artist() {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        let keyed = |album: &str, artist: &str| {
            Some(format!(
                "{}\u{1f}{}",
                resonate_core::folded_letters(album),
                resonate_core::folded_letters(artist)
            ))
        };

        let track = catalog
            .upsert(Track {
                uri: "content://test/a.flac".into(),
                title: "A".into(),
                artist: "Björk".into(),
                album: "Début".into(),
                ..Track::default()
            })
            .expect("a song");
        let untitled = catalog
            .upsert(Track {
                uri: "content://test/b.flac".into(),
                title: "B".into(),
                artist: "Björk".into(),
                ..Track::default()
            })
            .expect("a song without an album");

        assert_eq!(
            catalog.album_key_of(track.id).expect("a key"),
            keyed("Début", "Björk")
        );
        assert_eq!(catalog.album_key_of(untitled.id).expect("no key"), None);

        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET album_artist = 'Various' WHERE id = ?1",
                [track.id as i64],
            )
            .expect("a tagged album artist");

        assert_eq!(
            catalog.album_key_of(track.id).expect("a key"),
            keyed("Début", "Various")
        );
    }

    mod hearing {
        use std::{fs, sync::atomic::AtomicU32};

        use resonate_core::SourceId;
        use resonate_library::{Fingerprints, Printed, Sounded};

        use super::*;
        use crate::harness::{FOLDER, Harness};

        const RATE: u32 = 44_100;

        const SECONDS_A_SONG: u32 = 4;

        struct Printer {
            source: SourceId,
            calls: AtomicU32,
            refusals_left: AtomicU32,
            heard: &'static str,
        }

        impl Printer {
            fn hearing(heard: &'static str) -> Arc<Self> {
                Self::refusing(heard, 0)
            }

            fn refusing(heard: &'static str, refusals: u32) -> Arc<Self> {
                Arc::new(Self {
                    source: SourceId::new("fake").expect("a source"),
                    calls: AtomicU32::new(0),
                    refusals_left: AtomicU32::new(refusals),
                    heard,
                })
            }

            fn calls(&self) -> u32 {
                self.calls.load(Ordering::SeqCst)
            }
        }

        impl Fingerprints for Printer {
            fn source(&self) -> &SourceId {
                &self.source
            }

            fn recognise(&self, _sounded: &Sounded) -> resonate_library::Result<Printed> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                let refused = self
                    .refusals_left
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        left.checked_sub(1)
                    })
                    .is_ok();
                if refused {
                    return Err(resonate_library::Error::Refused {
                        op: LookupOp::Recognise,
                        status: 503,
                    });
                }
                Ok(Printed::Recognised(vec![RecordingMatch {
                    recording: Mbid::new("1b5b8f5d-6c3f-4f8b-8d3e-2d1f0a6b9c11").expect("an mbid"),
                    score: 100,
                    title: self.heard.to_owned(),
                    credit: vec![Credit {
                        name: "Pink Floyd".to_owned(),
                        joined_by: String::new(),
                        mbid: None,
                    }],
                    length: None,
                    isrcs: Vec::new(),
                    releases: Vec::new(),
                }]))
            }
        }

        fn tones() -> Vec<u8> {
            let frames = RATE * SECONDS_A_SONG;
            let mut data = Vec::new();
            for frame in 0..frames {
                let at = f64::from(frame) / f64::from(RATE);
                let pitch = 220.0 * (1.0 + (at * 0.7).sin().abs() * 3.0);
                let sample = ((at * pitch * std::f64::consts::TAU).sin() * 12_000.0) as i16;
                data.extend_from_slice(&sample.to_le_bytes());
                data.extend_from_slice(&sample.to_le_bytes());
            }

            let mut format = Vec::new();
            format.extend_from_slice(&1_u16.to_le_bytes());
            format.extend_from_slice(&2_u16.to_le_bytes());
            format.extend_from_slice(&RATE.to_le_bytes());
            format.extend_from_slice(&(RATE * 4).to_le_bytes());
            format.extend_from_slice(&4_u16.to_le_bytes());
            format.extend_from_slice(&16_u16.to_le_bytes());

            let mut body = b"WAVE".to_vec();
            for (id, payload) in [(b"fmt ", &format), (b"data", &data)] {
                body.extend_from_slice(id);
                body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
                body.extend_from_slice(payload);
            }

            let mut file = b"RIFF".to_vec();
            file.extend_from_slice(&(body.len() as u32).to_le_bytes());
            file.extend_from_slice(&body);
            file
        }

        fn sounding_library() -> Harness {
            let harness = Harness::new();
            for name in ["a", "b", "c", "d"] {
                fs::write(harness.folder.join(format!("{name}.wav")), tones()).expect("a song");
            }
            for (name, title, artist) in [
                ('a', "Time", "Pink Floyd"),
                ('b', "Money", "Pink Floyd"),
                ('c', "Time", "Air"),
                ('d', "d.wav", ""),
            ] {
                harness
                    .player
                    .catalog
                    .0
                    .lock()
                    .execute(
                        "UPDATE documents SET title = ?2, artist = ?3 WHERE id = ?1",
                        params![harness.song(name).id as i64, title, artist],
                    )
                    .expect("a named song");
            }
            harness
        }

        fn passed_by(
            harness: &Harness,
            asking: &Asking,
            told: &Arc<Told>,
            options: EnrichmentOptions,
        ) -> EnrichmentStatus {
            let tally = Arc::new(Tally::default());
            tally.began();

            run(
                &harness.player.catalog,
                &harness.player.sources,
                asking,
                told,
                &tally,
                options,
            );

            tally.status()
        }

        fn hearing(harness: &Harness, printer: &Arc<Printer>) -> EnrichmentStatus {
            let fake = Arc::new(Fake::default());
            let asking =
                asking(&fake).hearing_through(Arc::clone(printer) as Arc<dyn Fingerprints>);
            passed_by(
                harness,
                &asking,
                &Arc::new(Told::default()),
                EnrichmentOptions::default(),
            )
        }

        fn misnamed(harness: &Harness) -> Vec<String> {
            let mut titles: Vec<_> = harness
                .player
                .search_library("is:misnamed".into(), 10)
                .expect("a search")
                .tracks
                .into_iter()
                .map(|track| track.title)
                .collect();
            titles.sort();
            titles
        }

        fn studies(harness: &Harness) -> i64 {
            harness
                .player
                .catalog
                .0
                .lock()
                .query_row("SELECT count(*) FROM studies", [], |row| row.get(0))
                .expect("a count")
        }

        fn measured(harness: &Harness) -> i64 {
            harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT count(*) FROM studies WHERE length(band_levels) = 29 AND length(band_peaks) = ?1",
                    [resonate_analysis::BAND_PEAKS_BYTES as i64],
                    |row| row.get(0),
                )
                .expect("a count")
        }

        #[test]
        fn a_pass_given_a_key_studies_and_hears_every_song_so_is_misnamed_fills_unasked() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");

            let before = misnamed(&harness);
            let status = hearing(&harness, &printer);
            let after = misnamed(&harness);

            assert!(before.is_empty());
            assert_eq!(printer.calls(), 4);
            assert_eq!(studies(&harness), 4);
            assert_eq!(measured(&harness), 4);
            assert_eq!(after, vec!["Money".to_owned(), "Time".to_owned()]);
            assert!(!status.running);
        }

        #[test]
        fn a_song_the_pass_studied_is_known_by_its_packets_and_judged_under_this_build() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");

            hearing(&harness, &printer);
            let count = |sql: &str| -> i64 {
                harness
                    .player
                    .catalog
                    .0
                    .lock()
                    .query_row(sql, [], |row| row.get(0))
                    .expect("a count")
            };

            assert_eq!(
                count(
                    "SELECT count(*) FROM documents WHERE packets IS NOT NULL AND frames IS NOT NULL"
                ),
                4
            );
            assert_eq!(
                count(&format!(
                    "SELECT count(*) FROM studies WHERE studied_under = {} AND studied IS NOT NULL",
                    resonate_analysis::JUDGED_UNDER
                )),
                4
            );
        }

        #[test]
        fn a_song_heard_is_not_asked_again_unless_the_pass_refreshes() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");

            hearing(&harness, &printer);
            hearing(&harness, &printer);
            let asked_twice = printer.calls();

            let fake = Arc::new(Fake::default());
            let asking =
                asking(&fake).hearing_through(Arc::clone(&printer) as Arc<dyn Fingerprints>);
            passed_by(
                &harness,
                &asking,
                &Arc::new(Told::default()),
                EnrichmentOptions {
                    refresh: true,
                    ..EnrichmentOptions::default()
                },
            );

            assert_eq!(asked_twice, 4);
            assert_eq!(printer.calls(), 8);
        }

        #[test]
        fn a_pass_with_no_key_studies_and_hears_nothing() {
            let harness = sounding_library();
            let fake = Arc::new(Fake::default());

            passed_by(
                &harness,
                &asking(&fake),
                &Arc::new(Told::default()),
                EnrichmentOptions::default(),
            );

            assert_eq!(studies(&harness), 0);
            assert!(misnamed(&harness).is_empty());
        }

        fn studied_before_the_bands(harness: &Harness, names: &str) {
            for name in names.chars() {
                harness
                    .player
                    .catalog
                    .0
                    .lock()
                    .execute(
                        "INSERT INTO studies (track, verdict, peak, rms, true_peak, clipped)
                         VALUES (?1, 'genuine', 0.5, 0.1, 0.6, 0)",
                        [harness.song(name).id as i64],
                    )
                    .expect("a study");
            }
        }

        fn bands_of(harness: &Harness, name: char) -> Option<Vec<u8>> {
            harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT band_levels FROM studies WHERE track = ?1",
                    [harness.song(name).id as i64],
                    |row| row.get(0),
                )
                .expect("a study")
        }

        fn peaks_of(harness: &Harness, name: char) -> Option<Vec<u8>> {
            harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT band_peaks FROM studies WHERE track = ?1",
                    [harness.song(name).id as i64],
                    |row| row.get(0),
                )
                .expect("a study")
        }

        fn passing_with_no_key(harness: &Harness, told: &Arc<Told>) {
            let fake = Arc::new(Fake::default());
            passed_by(harness, &asking(&fake), told, EnrichmentOptions::default());
        }

        #[test]
        fn a_study_kept_without_its_bands_is_measured_by_a_pass_with_no_key_and_keeps_all_else() {
            let harness = sounding_library();
            studied_before_the_bands(&harness, "ab");

            passing_with_no_key(&harness, &Arc::new(Told::default()));

            assert_eq!(bands_of(&harness, 'a').map(|bands| bands.len()), Some(29));
            assert_eq!(bands_of(&harness, 'b').map(|bands| bands.len()), Some(29));
            assert_eq!(studies(&harness), 2);
            assert_eq!(measured(&harness), 2);
            let figures: (String, f64) = harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT verdict, true_peak FROM studies WHERE track = ?1",
                    [harness.song('a').id as i64],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("the figures");
            assert_eq!(figures, ("genuine".to_owned(), 0.6));
        }

        #[test]
        fn a_song_measured_by_the_pass_is_no_longer_owed_a_decode_when_the_preamp_is_fitted() {
            let harness = sounding_library();
            studied_before_the_bands(&harness, "a");
            let cuts = vec![resonate_analysis::Cut {
                location: location(&harness.song('a').uri).expect("a location"),
                span: None,
            }];

            let before = harness.player.catalog.unmeasured(&cuts).expect("a read");
            passing_with_no_key(&harness, &Arc::new(Told::default()));
            let after = harness.player.catalog.unmeasured(&cuts).expect("a read");

            assert_eq!(before.len(), 1);
            assert!(after.is_empty());
        }

        #[test]
        fn bands_already_kept_are_not_measured_again_and_a_song_without_a_study_gains_none() {
            let harness = sounding_library();
            studied_before_the_bands(&harness, "a");
            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE studies SET band_levels = ?1, band_peaks = ?2",
                    [
                        vec![7_u8; 29],
                        vec![3_u8; resonate_analysis::BAND_PEAKS_BYTES],
                    ],
                )
                .expect("kept bands");

            passing_with_no_key(&harness, &Arc::new(Told::default()));

            assert_eq!(bands_of(&harness, 'a'), Some(vec![7_u8; 29]));
            assert_eq!(
                peaks_of(&harness, 'a'),
                Some(vec![3_u8; resonate_analysis::BAND_PEAKS_BYTES])
            );
            assert_eq!(studies(&harness), 1);
        }

        #[test]
        fn a_study_kept_with_its_levels_but_not_its_peaks_is_measured_for_both() {
            let harness = sounding_library();
            studied_before_the_bands(&harness, "a");
            harness
                .player
                .catalog
                .0
                .lock()
                .execute("UPDATE studies SET band_levels = ?1", [vec![7_u8; 29]])
                .expect("kept levels");

            passing_with_no_key(&harness, &Arc::new(Told::default()));

            assert_eq!(
                peaks_of(&harness, 'a').map(|peaks| peaks.len()),
                Some(resonate_analysis::BAND_PEAKS_BYTES)
            );
            assert_ne!(bands_of(&harness, 'a'), Some(vec![7_u8; 29]));
            assert_eq!(measured(&harness), 1);
        }

        #[test]
        fn a_pass_told_to_stop_measures_nothing_and_a_file_that_will_not_open_stays_owed() {
            let harness = sounding_library();
            studied_before_the_bands(&harness, "ab");
            let told = Arc::new(Told::default());
            told.cancel(Leaving::Listener);

            passing_with_no_key(&harness, &told);
            let stopped = measured(&harness);
            fs::remove_file(harness.folder.join("a.wav")).expect("a file gone");
            passing_with_no_key(&harness, &Arc::new(Told::default()));

            assert_eq!(stopped, 0);
            assert_eq!(bands_of(&harness, 'a'), None);
            assert_eq!(bands_of(&harness, 'b').map(|bands| bands.len()), Some(29));
        }

        #[test]
        fn a_refusal_leaves_the_song_unheard_for_the_next_pass_to_ask() {
            let harness = sounding_library();
            let printer = Printer::refusing("Time", 2);

            hearing(&harness, &printer);
            let after_refusals = misnamed(&harness);
            hearing(&harness, &printer);

            assert_eq!(printer.calls(), 6);
            assert!(after_refusals.len() < 2);
            assert_eq!(
                misnamed(&harness),
                vec!["Money".to_owned(), "Time".to_owned()]
            );
        }

        #[test]
        fn recognition_is_given_up_after_a_run_of_refusals_while_the_pass_goes_on() {
            let harness = sounding_library();
            for name in ["e", "f", "g", "h"] {
                fs::write(harness.folder.join(format!("{name}.wav")), tones()).expect("a song");
                harness
                    .player
                    .index_document(format!("{FOLDER}{name}.wav"), format!("{name}.wav"))
                    .expect("an indexed song");
            }
            let printer = Printer::refusing("Time", u32::MAX);

            let status = hearing(&harness, &printer);

            assert_eq!(printer.calls(), RECOGNITIONS_REFUSED_BEFORE_GIVING_UP);
            assert_eq!(status.refused, RECOGNITIONS_REFUSED_BEFORE_GIVING_UP);
            assert!(misnamed(&harness).is_empty());
        }

        #[test]
        fn a_pass_told_to_stop_studies_nothing() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");
            let told = Arc::new(Told::default());
            told.cancel(Leaving::Listener);
            let fake = Arc::new(Fake::default());
            let asking =
                asking(&fake).hearing_through(Arc::clone(&printer) as Arc<dyn Fingerprints>);

            passed_by(&harness, &asking, &told, EnrichmentOptions::default());

            assert_eq!(printer.calls(), 0);
            assert_eq!(studies(&harness), 0);
        }

        fn broken_then_stamped(harness: &Harness) -> u64 {
            let song = harness.song('a').id;
            fs::write(harness.folder.join("a.wav"), b"not a song").expect("a broken file");
            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE documents SET size = 10, modified = 1000 WHERE id = ?1",
                    [song as i64],
                )
                .expect("a stamped document");
            song
        }

        fn mended(harness: &Harness) {
            fs::write(harness.folder.join("a.wav"), tones()).expect("a mended file");
        }

        fn unstudied(harness: &Harness) -> Vec<i64> {
            let held = harness.player.catalog.0.lock();
            let mut query = held
                .prepare("SELECT track FROM unstudied ORDER BY track")
                .expect("a query");
            query
                .query_map([], |row| row.get(0))
                .expect("rows")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("tracks")
        }

        #[test]
        fn a_song_that_will_not_decode_is_not_decoded_again_until_its_file_changes() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");
            let song = broken_then_stamped(&harness);

            hearing(&harness, &printer);
            let after_failing = (studies(&harness), unstudied(&harness));

            mended(&harness);
            hearing(&harness, &printer);
            let after_mending_unseen = (studies(&harness), unstudied(&harness));

            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE documents SET size = 11 WHERE id = ?1",
                    [song as i64],
                )
                .expect("a changed file");
            hearing(&harness, &printer);

            assert_eq!(after_failing, (3, vec![song as i64]));
            assert_eq!(after_mending_unseen, (3, vec![song as i64]));
            assert_eq!(studies(&harness), 4);
            assert!(unstudied(&harness).is_empty());
            assert_eq!(printer.calls(), 4);
        }

        #[test]
        fn a_song_that_will_not_decode_is_studied_again_when_the_pass_refreshes() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");
            let song = broken_then_stamped(&harness);

            hearing(&harness, &printer);
            let marked = unstudied(&harness);

            mended(&harness);
            let fake = Arc::new(Fake::default());
            let asking =
                asking(&fake).hearing_through(Arc::clone(&printer) as Arc<dyn Fingerprints>);
            passed_by(
                &harness,
                &asking,
                &Arc::new(Told::default()),
                EnrichmentOptions {
                    refresh: true,
                    ..EnrichmentOptions::default()
                },
            );

            assert_eq!(marked, vec![song as i64]);
            assert_eq!(studies(&harness), 4);
            assert!(unstudied(&harness).is_empty());
        }

        #[test]
        fn a_song_marked_under_an_earlier_judgement_is_studied_again() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");
            let song = broken_then_stamped(&harness);

            hearing(&harness, &printer);
            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE unstudied SET under = under - 1 WHERE track = ?1",
                    [song as i64],
                )
                .expect("an older mark");
            mended(&harness);
            hearing(&harness, &printer);

            assert_eq!(studies(&harness), 4);
            assert!(unstudied(&harness).is_empty());
        }

        #[test]
        fn a_song_forgotten_takes_its_mark_with_it() {
            let harness = sounding_library();
            let printer = Printer::hearing("Time");
            let song = broken_then_stamped(&harness);

            hearing(&harness, &printer);
            let marked = unstudied(&harness);
            harness
                .player
                .forget_deleted_songs(vec![song])
                .expect("a deletion");

            assert_eq!(marked, vec![song as i64]);
            assert!(unstudied(&harness).is_empty());
        }

        #[test]
        fn a_song_with_no_print_is_marked_unheard_and_not_studied_again() {
            let harness = Harness::new();
            let printer = Printer::hearing("Time");

            hearing(&harness, &printer);
            let once = studies(&harness);
            hearing(&harness, &printer);

            assert_eq!(once, 4);
            assert_eq!(printer.calls(), 0);
            assert!(misnamed(&harness).is_empty());
        }

        #[test]
        fn a_print_landed_from_another_device_is_heard_without_decoding_the_song_again() {
            let harness = Harness::new();
            let printer = Printer::hearing("Time");
            let landed = harness.song('a').id;
            let print =
                Chromaprint::new("AQADtEmSJEmSJEmS", Duration::from_secs(2)).expect("a print");

            hearing(&harness, &printer);
            let unprinted = printer.calls();
            harness
                .player
                .catalog
                .land_print(landed, &print)
                .expect("a landed print");
            harness
                .player
                .catalog
                .0
                .lock()
                .execute(
                    "UPDATE documents SET title = 'Time', artist = 'Pink Floyd' WHERE id = ?1",
                    [landed as i64],
                )
                .expect("a named song");
            fs::remove_file(harness.folder.join("a.wav")).expect("a song out of reach");
            hearing(&harness, &printer);
            let heard: (Option<String>, Option<String>, bool) = harness
                .player
                .catalog
                .0
                .lock()
                .query_row(
                    "SELECT agreement, heard_title, recognised IS NOT NULL
                       FROM studies WHERE track = ?1",
                    [landed as i64],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .expect("a study");
            hearing(&harness, &printer);

            assert_eq!(unprinted, 0);
            assert_eq!(
                heard,
                (Some("agrees".to_owned()), Some("Time".to_owned()), true)
            );
            assert_eq!(printer.calls(), 1);
        }
    }
}
