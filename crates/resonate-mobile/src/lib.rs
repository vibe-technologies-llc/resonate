mod abx;
mod ageing;
mod album_songs;
mod arranged;
mod artist_discography;
mod catalog;
mod catalog_extras;
#[cfg(test)]
mod catalog_steps_tests;
mod collections;
mod deleting;
mod documents;
mod downloads;
mod enrichment;
mod equaliser;
mod error;
mod explore;
mod failure;
mod features;
mod foreign;
#[cfg(test)]
mod harness;
mod history;
mod language;
mod learnt;
mod listening;
mod logging;
mod loves;
mod lyrics;
mod measurement;
mod mixes;
mod offering;
mod one_file;
mod online_tools;
mod organising;
mod output;
mod partials;
mod playback;
mod playlist_files;
mod playlist_links;
mod playlists;
mod profile_links;
mod queue_edit;
mod rendering;
mod replacing;
#[cfg(test)]
mod resuming_tests;
mod rewriting;
mod room;
mod saved_searches;
mod saving_lyrics;
mod search;
#[cfg(test)]
mod search_parity;
mod served;
mod sharing;
mod sounding;
mod spotify;
mod stamp;
mod sync;
mod tagging;
mod together;
mod updates;
mod vanished;
mod versions;
mod watching;
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}
mod wrapped;
mod zoning;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_codec::{Picturing, Sources, probe_pictured};
use resonate_core::{
    Frames, MediaLocation, Resumable, Resumption, SampleRate, TrackId, Volume, ZoneId,
};
use resonate_engine::{
    Command, EngineConfig, OutputStatus, PlaybackState, PlayerState, QueueItem, QueueRows,
    RepeatMode,
};
use resonate_library::{NamedByTheFile, named_by_the_file};

use crate::{
    arranged::{Arranged, Laid},
    catalog::Catalog,
    documents::{GRANTED_SCHEMES, GrantedFiles, document_uri, location},
    one_file::{Probed, Spelled},
    stamp::DocumentStamp,
    vanished::Absorbing,
};
pub use crate::{
    documents::{Documents, audio_extensions, document_not_found, document_refused},
    equaliser::{
        EqualiserBand, EqualiserCurve, EqualiserFilter, EqualiserImport, EqualiserLimits,
        EqualiserPreset, equaliser_frequencies, equaliser_limits, equaliser_presets,
        equaliser_text, parse_eq_profile, response_curve, ten_band_curve, ten_band_gains,
    },
    error::{Error, Result, StoreOp},
    learnt::{RendererLearnt, TrialHeard},
    logging::{LogLevel, Logbook, keep_log},
    measurement::{
        BuiltInTarget, MeasurementFit, MeasurementOffers, MeasurementSweep, MeasurementTarget,
        fit_measurement, fit_recorded_sweep, measurement_offers, measurement_rate_for,
        measurement_sweep,
    },
    playback::{
        GainMode, LossyRestoration, PlaybackPreferences, ResamplingPhase, ResamplingQuality,
        SleepTimer, default_playback_preferences,
    },
    rendering::{NetworkRenderer, RendererKind},
    rewriting::DocumentRewriting,
    saved_searches::{KeptSearch, SavedSearch, SearchOrder, search_order_reads_descending},
    saving_lyrics::{
        DocumentBeside, LyricsSaved, LyricsStanding, MadeBeside, SheetsBeside, lyric_folders,
    },
    sounding::HeardElsewhere,
    together::{
        InvitationKind, ListeningParty, PartyChatLine, PartyClockApart, PartyConnection,
        PartyFollowing, PartyHandingOver, PartyReactionKind, PartyRequest, PartySaying,
        SharedPlaylist, SharedPlaylistReach, TogetherPerson, invitation_kind,
    },
    versions::{FoundVersion, FoundVersions, SongVersion, VersionSearch, VersionSearchOffered},
    watching::PlayerWatch,
    zoning::PlayerZone,
};

uniffi::setup_scaffolding!();

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct Track {
    pub id: u64,
    pub uri: String,
    pub name: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub rate: u32,
    pub codec: String,
    pub favourite: bool,
    pub album_artist: String,
    pub genre: String,
    pub date: String,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    pub bits: Option<u32>,
    pub channels: u32,
    pub recording: Option<String>,
    pub release: Option<String>,
    pub artist_mbid: Option<String>,
    pub art_revision: u64,
    pub queue_id: Option<u64>,
}

impl Track {
    fn gone(location: &MediaLocation) -> Self {
        let uri = location.to_uri();
        let name = uri.rsplit('/').next().unwrap_or(&uri).to_owned();
        Self {
            uri,
            title: name.clone(),
            name,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct QueueWindow {
    pub first: u32,
    pub total: u32,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    first: usize,
    rows: usize,
}

impl Shown {
    fn around(
        playing: Option<usize>,
        rows: usize,
        heard_at_most: usize,
        ahead_at_most: usize,
    ) -> Self {
        let playing = playing.unwrap_or(0).min(rows);
        let first = playing.saturating_sub(heard_at_most);
        let end = playing
            .saturating_add(1)
            .saturating_add(ahead_at_most)
            .min(rows);
        Self {
            first,
            rows: end.saturating_sub(first),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Transport {
    Stopped,
    Loading,
    Playing,
    Paused,
}

impl Transport {
    const fn sounds(self) -> bool {
        matches!(self, Self::Playing | Self::Loading)
    }
}

fn row_being_opened(state: &PlayerState, rows: &QueueRows) -> Option<TrackId> {
    let opening = matches!(
        state.playback,
        PlaybackState::Buffering | PlaybackState::Paused
    );
    opening
        .then_some(state.queue_position)
        .flatten()
        .and_then(|at| rows.get(at))
        .map(|item| item.id)
}

fn holds_a_place_to_seek(state: &PlayerState, rows: &QueueRows) -> bool {
    state.current.is_some() || row_being_opened(state, rows).is_some()
}

fn place_saved(
    party_following: bool,
    state: &PlayerState,
    rows: &QueueRows,
    revision: u64,
) -> Option<SavedPlace> {
    (!party_following).then(|| place_to_save(state, rows, revision))
}

fn place_to_save(state: &PlayerState, rows: &QueueRows, revision: u64) -> SavedPlace {
    let reached = match state.current {
        Some(track) => {
            Reached::At(track.position.to_duration(track.source.rate).as_millis() as u64)
        }
        None if row_being_opened(state, rows).is_some() => Reached::StillOpening,
        None => Reached::At(0),
    };
    SavedPlace {
        revision,
        shuffle: state.shuffle,
        row: state.queue_position.unwrap_or(0) as u32,
        reached,
    }
}

const fn transport_of(playback: PlaybackState) -> Transport {
    match playback {
        PlaybackState::Idle | PlaybackState::Stopped => Transport::Stopped,
        PlaybackState::Buffering => Transport::Loading,
        PlaybackState::Playing => Transport::Playing,
        PlaybackState::Paused => Transport::Paused,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OutputFigures {
    rate: u32,
    underruns: u64,
    overs: u64,
}

impl OutputFigures {
    fn of(output: Option<&OutputStatus>) -> Self {
        output.map_or_else(Self::default, |output| Self {
            rate: output.negotiated.rate.hz(),
            underruns: output.underruns,
            overs: output.overs,
        })
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Snapshot {
    pub transport: Transport,
    pub current: Option<Track>,
    pub queue_index: u32,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: u32,
    pub output_rate: u32,
    pub underruns: u64,
    #[uniffi(default)]
    pub overs: u64,
    pub failure: Option<String>,
    pub sleep: Option<SleepTimer>,
    pub sleep_remaining_ms: Option<u64>,
    pub queue_revision: u64,
    pub catalog_revision: u64,
    #[uniffi(default)]
    pub next_first: Option<u32>,
    #[uniffi(default)]
    pub next_count: u32,
    #[uniffi(default)]
    pub waiting: Option<String>,
    #[uniffi(default)]
    pub zone: u32,
    #[uniffi(default)]
    pub zones_revision: u64,
    #[uniffi(default)]
    pub heard_elsewhere: Option<HeardElsewhere>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reached {
    At(u64),
    StillOpening,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SavedPlace {
    revision: u64,
    shuffle: bool,
    row: u32,
    reached: Reached,
}

impl SavedPlace {
    fn arranged_as(&self, other: &Self) -> bool {
        self.revision == other.revision && self.shuffle == other.shuffle
    }
}

#[derive(uniffi::Object)]
pub struct MobilePlayer {
    playing: zoning::Playing,
    catalog: Arc<Catalog>,
    sources: Arc<Sources>,
    preferences: Mutex<PlaybackPreferences>,
    services: features::Services,
    playlist_undo: Mutex<Vec<playlists::PlaylistUndo>>,
    downloads: downloads::Downloads,
    syncing: sync::Syncing,
    enriching: enrichment::Enriching,
    room: room::Room,
    tagging: tagging::Tagging,
    served: Arc<served::Served>,
    rendering: rendering::Rendering,
    together: together::Together,
    offered: offering::Offered,
    releases: resonate_online::KeptRelease,
}

#[uniffi::export]
impl MobilePlayer {
    #[uniffi::constructor]
    pub fn new(folder: String, documents: Box<dyn Documents>) -> Result<Arc<Self>> {
        fs::create_dir_all(&folder).map_err(|source| Error::Folder {
            op: StoreOp::CreateFolder,
            source,
        })?;

        let catalog = Arc::new(Catalog::open(&Path::new(&folder).join("documents.sqlite"))?);
        let documents: Arc<dyn Documents> = Arc::from(documents);
        let files = Arc::clone(&documents);
        let served = Arc::new(served::Served::default());

        let sources = Arc::new(GRANTED_SCHEMES.iter().try_fold(
            served.sources(Sources::local().spooling_under(Path::new(&folder).to_path_buf())),
            |sources, scheme| {
                Ok::<_, Error>(
                    sources.and(Arc::new(GrantedFiles::new(Arc::clone(&documents), scheme)?)),
                )
            },
        )?);
        let routing = Arc::new(output::Routing::new());
        let kept = catalog.kept_zones()?;
        let rendering = rendering::Rendering::on_the_network();

        rendering.taught_from(&catalog)?;
        if kept.iter().any(|zone| zone.renderer.is_some()) {
            rendering.look_behind();
        }

        let config = EngineConfig {
            volume: zoning::first_zones_volume(&kept),
            sink: zoning::first_zones_sink(&kept),
            ..EngineConfig::default()
        };
        let kinds = rendering.kinds();

        let player = output::player(config, Arc::clone(&sources), &routing, kinds.clone())?;
        let engines: Box<zoning::Engines> = {
            let sources = Arc::clone(&sources);
            Box::new(
                move |_: ZoneId, config: EngineConfig, routing: &Arc<output::Routing>| {
                    output::player(config, Arc::clone(&sources), routing, kinds.clone())
                },
            )
        };

        let playing = zoning::Playing::start(
            player,
            routing,
            kept.iter().find(|zone| zone.id.is_first()),
            engines,
        );
        let this = Arc::new(Self {
            playing,
            catalog,
            sources,
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: features::Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: downloads::Downloads::under(Path::new(&folder)),
            syncing: sync::Syncing::under(PathBuf::from(&folder)),
            enriching: enrichment::Enriching::default(),
            room: room::Room::under(PathBuf::from(&folder)),
            tagging: tagging::Tagging::under(PathBuf::from(&folder)),
            served,
            together: together::Together::default(),
            rendering,
            offered: offering::Offered::under(PathBuf::from(&folder)),
            releases: resonate_online::KeptRelease::in_folder(Path::new(&folder)),
        });

        this.downloads.stall_by(this.services.downloads.clone());
        this.services.files.reach_through(files);

        let carried_over = this.catalog.carries_queues_over()?;

        if carried_over {
            this.resume_saved_in(ZoneId::FIRST)?;
        }
        watching::keep(&this, ZoneId::FIRST);
        this.restore_zones(&kept, carried_over)?;
        Ok(this)
    }

    pub fn index_document(&self, uri: String, name: String) -> Result<Track> {
        self.indexed(uri, name, None, Spelled::Anew)
    }

    pub fn scan_document(
        &self,
        uri: String,
        name: String,
        size: u64,
        modified_ms: i64,
    ) -> Result<Track> {
        let stamp = DocumentStamp::of(size, modified_ms);

        if let Some(stamp) = stamp
            && let Some(known) = self.catalog.unchanged(&uri, stamp)?
        {
            return Ok(known);
        }
        self.indexed(uri, name, stamp, Spelled::AsListed)
    }
}

impl MobilePlayer {
    fn indexed(
        &self,
        uri: String,
        name: String,
        stamp: Option<DocumentStamp>,
        spelled: Spelled,
    ) -> Result<Track> {
        self.indexed_and_named(uri, name, stamp, Absorbing::EveryHidden, spelled)
            .map(|(track, _)| track)
    }

    pub(crate) fn indexed_and_named(
        &self,
        uri: String,
        name: String,
        stamp: Option<DocumentStamp>,
        absorbing: Absorbing,
        spelled: Spelled,
    ) -> Result<(Track, NamedByTheFile)> {
        let (info, picture) = probe_pictured(&self.sources, &location(&uri)?, Picturing::Copied)?;
        let mut named = info.tags.clone();

        let by_the_file = named_by_the_file(&name, &mut named);
        let duration_ms = info.duration.map_or(0, |frames| {
            frames
                .to_duration(info.spec.rate)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        let probed = Probed {
            rate: info.spec.rate.hz(),
            duration_ms,
        };
        let uri = self.spelled_as_held(uri, &name, spelled, &probed)?;

        let track = self.catalog.upsert_named(
            Track {
                id: 0,
                uri,
                title: named.title.unwrap_or_else(|| name.clone()),
                name,
                artist: named.artist.unwrap_or_default(),
                album: info.tags.album.clone().unwrap_or_default(),
                duration_ms,
                rate: info.spec.rate.hz(),
                codec: resonate_codec::Codec::from_id(info.codec).to_string(),
                favourite: false,
                ..Track::default()
            },
            by_the_file,
        )?;

        self.services.analyses.lock().remove(&track.id);
        self.catalog.tagged(track.id, &info, stamp)?;
        if let Some(picture) = picture.into_copied() {
            self.catalog
                .keep_artwork(track.id, &picture.bytes, "embedded", true)?;
        }
        if let Some(text) = info.tags.lyrics
            && let Err(error) = self.keep_lyric_text(track.id, text)
        {
            tracing::debug!(%error, "embedded lyrics could not be read");
        }
        self.take_the_place_of_missing(track.id, absorbing)?;
        Ok((self.catalog.track(track.id)?, by_the_file))
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn song_uris(&self) -> Result<Vec<String>> {
        self.catalog.uris()
    }

    pub fn queue(&self) -> Result<Vec<Track>> {
        let rows = self.playing.player()?.queued().rows;
        self.tracks_of_rows(&rows.iter().collect::<Vec<_>>())
    }

    pub fn queue_window(&self, heard_at_most: u32, ahead_at_most: u32) -> Result<QueueWindow> {
        let player = self.playing.player()?;
        let rows = player.queued().rows;

        let shown = Shown::around(
            player.state().queue_position,
            rows.len(),
            heard_at_most as usize,
            ahead_at_most as usize,
        );
        let tracks = self.tracks_of_rows(
            &rows
                .iter()
                .skip(shown.first)
                .take(shown.rows)
                .collect::<Vec<_>>(),
        )?;
        Ok(QueueWindow {
            first: shown.first as u32,
            total: rows.len() as u32,
            tracks,
        })
    }

    pub fn load(&self, ids: Vec<u64>, start_at: u32, autoplay: bool) -> Result<()> {
        let items: Vec<QueueItem> = self
            .catalog
            .tracks_by_id(&ids)?
            .into_iter()
            .map(|track| {
                Ok(QueueItem::whole(
                    TrackId::new(track.id)?,
                    location(&track.uri)?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let items = self.as_chosen(items)?;

        if !autoplay {
            return self.command_landed(Command::Resume(Resumption {
                rows: items
                    .into_iter()
                    .map(|item| Resumable {
                        location: item.location,
                        span: item.span,
                        held: Some(item.id),
                    })
                    .collect(),
                row: start_at as usize,
                ..Resumption::default()
            }));
        }
        self.command(Command::Load {
            items,
            start_at: start_at as usize,
            autoplay,
        })
    }

    pub fn play(&self) -> Result<()> {
        self.command(Command::Play)
    }
    pub fn pause(&self) -> Result<()> {
        self.command(Command::Pause)
    }
    pub fn stop(&self) -> Result<()> {
        self.command(Command::Stop)
    }
    pub fn next(&self) -> Result<()> {
        self.command(Command::Next)
    }
    pub fn previous(&self) -> Result<()> {
        self.command(Command::Previous)
    }
    pub fn jump_to(&self, index: u32) -> Result<()> {
        self.command(Command::JumpTo(index as usize))
    }
    pub fn set_volume(&self, volume: f32) -> Result<()> {
        self.command(Command::SetVolume(Volume::new(volume)?))
    }
    pub fn set_shuffle(&self, shuffled: bool) -> Result<()> {
        self.command(Command::SetShuffle(shuffled))
    }
    pub fn set_repeat(&self, repeat: u32) -> Result<()> {
        self.command(Command::SetRepeat(match repeat {
            1 => RepeatMode::Track,
            2 => RepeatMode::Queue,
            _ => RepeatMode::Off,
        }))
    }
    pub fn favourite(&self, id: u64, favourite: bool) -> Result<()> {
        self.catalog.favourite(id, favourite)
    }

    pub fn playback_preferences(&self) -> PlaybackPreferences {
        self.preferences.lock().clone()
    }

    pub fn configure_playback(&self, preferences: PlaybackPreferences) -> Result<()> {
        let mut previous = self.preferences.lock();

        let commands = preferences.commands(&previous)?;

        if preferences.carries_the_queue_over != self.catalog.carries_queues_over()? {
            self.catalog
                .carry_queues_over(preferences.carries_the_queue_over)?;
        }
        for command in commands {
            self.command(command)?;
        }
        *previous = preferences;
        Ok(())
    }

    pub fn set_sleep(&self, timer: Option<SleepTimer>) -> Result<()> {
        self.command(Command::SleepUntil(timer.map(Into::into)))
    }

    pub fn seek(&self, position_ms: u64) -> Result<()> {
        let player = self.playing.player()?;
        let (state, queue) = (player.state(), player.queued());

        if holds_a_place_to_seek(&state, &queue.rows) {
            self.command(Command::SeekToMoment(Duration::from_millis(position_ms)))?;
        }
        Ok(())
    }

    pub fn jump_to_at(&self, index: u32, position_ms: u64) -> Result<()> {
        let rows = self.playing.player()?.queued().rows;

        self.command(Command::JumpTo(index as usize))?;
        if position_ms > 0 && rows.get(index as usize).is_some() {
            self.command(Command::SeekToMoment(Duration::from_millis(position_ms)))?;
        }
        Ok(())
    }

    pub fn transport(&self) -> Result<Transport> {
        Ok(transport_of(self.playing.player()?.state().playback))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let zone = self.playing.shown();
        let zones_revision = self.playing.zones()?.revision();
        let zoned = self.playing.zoned(zone)?;
        let (events, state, queue) = {
            let player = self.playing.player_of(zone)?;
            let events = player.events().try_iter().collect::<Vec<_>>();
            (events, player.state(), player.queued())
        };

        self.take_in(&zoned, events, &queue.rows);

        let waiting = Self::still_waiting(&zoned, &state);
        let heard = self.heard(&state, &queue.rows);
        let queue_revision = queue.revision;
        let next_first = queue.next.map(|next| next.first() as u32);
        let next_count = queue
            .next
            .map_or(0, |next| (next.last() - next.first() + 1) as u32);
        let figures = OutputFigures::of(state.output.as_ref());
        Ok(Snapshot {
            transport: transport_of(state.playback),
            current: heard.current,
            queue_index: state.queue_position.unwrap_or(0) as u32,
            position_ms: heard.position_ms,
            duration_ms: heard.duration_ms,
            volume: state.volume.get(),
            shuffle: state.shuffle,
            repeat: match state.repeat {
                RepeatMode::Off => 0,
                RepeatMode::Track => 1,
                RepeatMode::Queue => 2,
            },
            output_rate: figures.rate,
            underruns: figures.underruns,
            overs: figures.overs,
            failure: zoned.failure.lock().take(),
            sleep: state.sleeping.map(|sleep| sleep.until.into()),
            sleep_remaining_ms: state
                .sleeping
                .and_then(|sleep| sleep.left.map(|left| left.as_millis() as u64)),
            queue_revision,
            catalog_revision: self.catalog.revision(),
            next_first,
            next_count,
            waiting,
            zone: zone.get(),
            zones_revision,
            heard_elsewhere: self.heard_elsewhere(zone),
        })
    }

    pub fn save(&self) -> Result<()> {
        self.save_while(|zone| self.party_following(zone))
    }

    pub fn shutdown(&self) -> Result<()> {
        self.offered.leave();
        self.sync_stop();
        self.pause_enrichment();
        self.configure_online(false);
        self.cancel_analysis();
        self.downloads.cancel_every_job();

        let finished = self.finish_listening();
        let saved = self.save();
        let stopped = match self.playing.take() {
            Some(zones) => zoning::shut_down(zones),
            None => Ok(()),
        };
        finished.and(saved).and(stopped)
    }
}

impl MobilePlayer {
    pub(crate) fn save_while(&self, following: impl Fn(ZoneId) -> bool) -> Result<()> {
        self.playing
            .listed()
            .into_iter()
            .map(|zone| self.save_zone(&zone, following(zone.id)))
            .fold(Ok(()), Result::and)
    }

    fn save_zone(&self, zone: &resonate_engine::Zone, following: bool) -> Result<()> {
        let Ok(zoned) = self.playing.zoned(zone.id) else {
            return Ok(());
        };
        let mut keeping = zoned.keeping.lock();

        if !self.playing.still_holds(zone) {
            return Ok(());
        }
        self.keep_volume_of(zone, &zoned)?;

        let (place, queue, rate) = {
            let state = zone.player.state();
            let queue = zone.player.queued();

            if keeping.waits_on_its_restore(queue.revision) {
                return Ok(());
            }

            let Some(place) = place_saved(following, &state, &queue.rows, queue.revision) else {
                return Ok(());
            };
            let rate = state.current.map(|track| track.source.rate.hz());
            (place, queue, rate)
        };

        if keeping.place.as_ref() == Some(&place) {
            return Ok(());
        }

        let served_playing = queue
            .rows
            .get(place.row as usize)
            .is_some_and(|item| served::Served::names(&item.location));

        if keeping
            .place
            .as_ref()
            .is_some_and(|saved| saved.arranged_as(&place))
        {
            self.catalog
                .save_place_of(zone.id, place.row, place.reached)?;
            if served_playing && let Some(rate) = rate {
                self.catalog.keep_served_rate_of(zone.id, place.row, rate)?;
            }
        } else {
            let uris = queue
                .rows
                .iter()
                .filter(|item| !served::Served::names(&item.location))
                .map(|item| document_uri(&item.location))
                .collect::<Result<Vec<_>>>()?;
            let mut ids = self.catalog.ids_held_at(&uris)?.into_iter();
            let loaded = queue.rows.in_load_order();
            let arranged = Arranged {
                rows: queue
                    .rows
                    .iter()
                    .zip(loaded)
                    .map(|(item, loaded)| Laid { row: item, loaded })
                    .collect(),
                row: place.row as usize,
                shuffle: place.shuffle,
                next: queue.next,
            };

            let (kept, passed) = arranged.retained(|at, item| {
                if served::Served::names(&item.location) {
                    return Some(catalog::KeptRow::Served(catalog::KeptServed {
                        song: self.served.song_at(&item.location),
                        rate: rate.filter(|_| at == place.row as usize),
                    }));
                }
                ids.next().flatten().map(catalog::KeptRow::Document)
            });
            let reached = if passed.the_place {
                Reached::At(0)
            } else {
                place.reached
            };

            self.catalog.save_rows_of(zone.id, &kept, reached)?;
            if passed.any {
                keeping.place = None;
                return Ok(());
            }
        }
        keeping.place = Some(place);
        Ok(())
    }

    pub(crate) fn resume_saved_in(&self, zone: ZoneId) -> Result<()> {
        let Some(resumption) = self.saved_resumption(zone)? else {
            return Ok(());
        };

        self.restore_in(zone, || {
            self.playing.send_to(zone, Command::Resume(resumption))
        })
    }

    fn restore_in(&self, zone: ZoneId, sent: impl FnOnce() -> Result<()>) -> Result<()> {
        let zoned = self.playing.zoned(zone)?;
        let player = self.playing.player_of(zone)?;
        let mut keeping = zoned.keeping.lock();

        let over_revision = player.queued().revision;

        sent()?;
        keeping.restore_sent(over_revision);
        Ok(())
    }

    fn saved_resumption(&self, zone: ZoneId) -> Result<Option<Resumption>> {
        let (kept, position) = self.catalog.resume_rows_of(zone)?;

        if kept.is_empty() {
            return Ok(None);
        }

        let ids: Vec<u64> = kept
            .rows
            .iter()
            .filter_map(|laid| match laid.row {
                catalog::KeptRow::Document(id) => Some(id),
                catalog::KeptRow::Served(_) => None,
            })
            .collect();
        let mut tracks = self.catalog.tracks_by_id(&ids)?.into_iter();
        let place = kept.row;
        let mut playing_rate = None;

        let (resumed, _) = kept.retained(|row, held| {
            let (resumable, its_rate) = match held {
                catalog::KeptRow::Document(_) => {
                    let track = tracks.next()?;
                    let location = location(&track.uri).ok()?;
                    (
                        Resumable {
                            location,
                            span: None,
                            held: TrackId::new(track.id).ok(),
                        },
                        SampleRate::new(track.rate).ok(),
                    )
                }
                catalog::KeptRow::Served(served) => {
                    let location = MediaLocation::from_uri(&served.song.uri)?;
                    let its_rate = served.rate.and_then(|rate| SampleRate::new(rate).ok());

                    self.served.kept(std::slice::from_ref(&served.song));
                    (
                        Resumable {
                            location,
                            span: None,
                            held: None,
                        },
                        its_rate,
                    )
                }
            };

            if row == place {
                playing_rate = its_rate;
            }
            Some(resumable)
        });
        let at_ms = if playing_rate.is_some() { position } else { 0 };
        let at = Frames::from_duration(
            Duration::from_millis(at_ms),
            playing_rate.unwrap_or(SampleRate::HZ_48000),
        );

        Ok(Some(resumed.resumption(at)))
    }

    fn tracks_of_rows(&self, rows: &[&Arc<QueueItem>]) -> Result<Vec<Track>> {
        let uris = rows
            .iter()
            .filter(|item| !served::Served::names(&item.location))
            .map(|item| document_uri(&item.location))
            .collect::<Result<Vec<_>>>()?;
        let mut held = self.catalog.tracks_held_at(&uris)?.into_iter();
        Ok(rows
            .iter()
            .map(|item| {
                let mut track = if served::Served::names(&item.location) {
                    self.served.track_of(item)
                } else {
                    held.next()
                        .flatten()
                        .unwrap_or_else(|| Track::gone(&item.location))
                };

                track.queue_id = Some(item.id.get());
                track
            })
            .collect())
    }

    fn queue_track(&self, rows: &QueueRows, id: TrackId) -> Option<Track> {
        let item = rows.iter().find(|item| item.id == id)?;

        if served::Served::names(&item.location) {
            return Some(self.served.track_of(item));
        }

        let uri = document_uri(&item.location).ok()?;
        let mut track = self.catalog.kept_track_uri(&uri).ok()?;

        track.queue_id = Some(item.id.get());
        Some(track)
    }

    fn command(&self, command: Command) -> Result<()> {
        self.playing.send(command)
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, Frames, SampleFormat, Span, StreamSpec};
    use resonate_engine::{OutputMode, SinkId, StreamVolume};

    use super::*;
    use crate::harness::{CatalogFile, Harness};

    fn output_with(underruns: u64, overs: u64) -> OutputStatus {
        let rate = SampleRate::new(48_000).expect("a rate");
        OutputStatus {
            sink: SinkId::new(1),
            negotiated: StreamSpec::new(rate, ChannelLayout::Stereo, SampleFormat::F32),
            words: None,
            mode: OutputMode::Converted,
            graph_rate: None,
            latency: Frames::ZERO,
            underruns,
            went_without: Frames::ZERO,
            overs,
            device_turned: false,
            device_muted: false,
            stream_volume: StreamVolume::Unity,
            holds_seconds_downstream: false,
        }
    }

    #[test]
    fn a_snapshot_carries_the_clamped_samples_of_the_open_stream_and_none_without_one() {
        let open = output_with(2, 41);

        assert_eq!(
            OutputFigures::of(Some(&open)),
            OutputFigures {
                rate: 48_000,
                underruns: 2,
                overs: 41,
            }
        );
        assert_eq!(OutputFigures::of(None), OutputFigures::default());
    }

    fn row_gone_from_the_catalog(harness: &crate::harness::Harness, name: char) {
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "DELETE FROM documents WHERE id = ?1",
                [harness.song(name).id as i64],
            )
            .expect("a catalog row gone");
    }

    #[test]
    fn a_queue_row_whose_catalog_row_is_gone_stays_listed_beside_the_rows_that_are_held() {
        let harness = crate::harness::Harness::new();

        harness.resumed("abc", 1, 0);

        let rows = harness.queue_ids();

        row_gone_from_the_catalog(&harness, 'b');

        let queue = harness.player.queue().expect("a queue past a gone row");

        assert_eq!(queue.len(), 3);
        assert_eq!(
            queue.iter().map(|track| track.queue_id).collect::<Vec<_>>(),
            rows.iter().copied().map(Some).collect::<Vec<_>>()
        );
        assert_eq!(crate::harness::name_of(&queue[0]), 'a');
        assert_eq!(queue[1].name, "b.wav");
        assert_eq!(crate::harness::name_of(&queue[2]), 'c');
    }

    #[test]
    fn a_saved_row_whose_uri_does_not_read_is_passed_over_and_the_place_follows_the_song() {
        let harness = crate::harness::Harness::new();
        let ids = harness.ids("abcd");

        harness
            .player
            .catalog
            .save_queue(&ids, 2, 0)
            .expect("a saved queue");
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET uri = 'resonate-displaced:1', missing = 1 WHERE id = ?1",
                [ids[0] as i64],
            )
            .expect("a displaced row");

        harness
            .player
            .resume_saved_in(ZoneId::FIRST)
            .expect("a resumption past a row that does not read");

        let resumed = harness.settled(|snapshot| snapshot.current.is_some());

        assert_eq!(harness.queued(), "bcd");
        assert_eq!(
            resumed.current.map(|current| current.id),
            Some(harness.song('c').id)
        );
        assert_eq!(resumed.queue_index, 1);
    }

    fn arrangement(harness: &crate::harness::Harness) -> (String, String, bool, Option<Span>) {
        let player = harness.player.playing.player().expect("an open player");
        let queued = player.queued();
        let named = |track: &Track| crate::harness::name_of(track);
        let titled = |item: &QueueItem| {
            harness
                .songs
                .iter()
                .find(|song| location(&song.uri).ok().as_ref() == Some(&item.location))
                .map_or('?', named)
        };

        (
            harness.queued(),
            queued
                .in_list_order()
                .iter()
                .map(|item| titled(item))
                .collect(),
            player.state().shuffle,
            queued.next,
        )
    }

    #[test]
    fn a_shuffled_queue_comes_back_shuffled_with_its_list_order_and_the_rows_played_next() {
        let harness = crate::harness::Harness::new();

        harness.resumed("abcd", 0, 0);
        harness.player.set_shuffle(true).expect("shuffle on");
        harness.settled(|snapshot| snapshot.shuffle);
        harness
            .player
            .play_next(harness.ids("c"))
            .expect("a row played next");
        harness.settled(|snapshot| snapshot.next_count == 1);

        let saved = arrangement(&harness);

        harness.player.save().expect("a save");
        harness.resumed("dcba", 0, 0);
        harness
            .player
            .resume_saved_in(ZoneId::FIRST)
            .expect("the saved queue resumed");
        harness.settled(|snapshot| snapshot.shuffle && snapshot.next_count == 1);

        let resumed = arrangement(&harness);

        assert!(saved.2);
        assert_eq!(saved.1, "acbcd");
        assert_eq!(resumed, saved);
    }

    #[test]
    fn a_restore_still_on_its_way_leaves_every_saved_queue_as_it_was_through_a_save_and_a_shutdown()
    {
        let file = CatalogFile::new();
        let kitchen = {
            let harness = Harness::over_the_catalog_in(&file);

            let kitchen = harness
                .player
                .zone_added("Kitchen", 0, String::new())
                .expect("a second zone");

            harness
                .player
                .catalog
                .save_queue(&harness.ids("abcd"), 1, 500)
                .expect("the first zone's queue");
            harness
                .player
                .catalog
                .save_queue_of(kitchen.id, &harness.ids("dc"), 1, 0)
                .expect("the kitchen's queue");
            kitchen.id
        };

        let harness = Harness::over_the_catalog_in(&file);
        let kept = harness.player.catalog.kept_zones().expect("the kept zones");

        harness
            .player
            .zone_opened(
                &harness.player.playing.zones().expect("the zones"),
                &kept[0],
                0,
            )
            .expect("the kitchen reopened");

        let mut held = [ZoneId::FIRST, kitchen].map(|zone| {
            let resumption = harness
                .player
                .saved_resumption(zone)
                .expect("read")
                .expect("a saved queue");

            harness
                .player
                .restore_in(zone, || Ok(()))
                .expect("a restore on its way");
            resumption
        });

        harness
            .player
            .save()
            .expect("a save before the restores land");

        let saved_early = [ZoneId::FIRST, kitchen]
            .map(|zone| harness.player.catalog.resume_of(zone).expect("read"));

        harness
            .player
            .playing
            .player_of(ZoneId::FIRST)
            .expect("the first zone")
            .request(Command::Resume(std::mem::take(&mut held[0])))
            .expect("the first restore sent")
            .wait_for(crate::harness::SETTLED_WITHIN)
            .expect("the first restore landed");
        harness.settled(|snapshot| snapshot.current.is_some());
        harness
            .player
            .add_to_queue(harness.ids("c"))
            .expect("a song added once the first queue stands");
        harness.player.shutdown().expect("shut down");

        let (first, row, _) = harness
            .player
            .catalog
            .resume_of(ZoneId::FIRST)
            .expect("read");

        assert_eq!(
            saved_early,
            [(harness.ids("abcd"), 1, 500), (harness.ids("dc"), 1, 0)]
        );
        assert_eq!((first, row), (harness.ids("abcdc"), 1));
        assert_eq!(
            harness.player.catalog.resume_of(kitchen).expect("read"),
            (harness.ids("dc"), 1, 0)
        );
        assert_eq!(
            harness
                .player
                .catalog
                .kept_zones()
                .expect("read")
                .iter()
                .map(|zone| zone.id)
                .collect::<Vec<_>>(),
            vec![kitchen]
        );
    }

    fn opening_its_row(harness: &Harness) {
        let player = harness.player.playing.player().expect("an open player");
        let began = std::time::Instant::now();

        loop {
            let (state, queue) = (player.state(), player.queued());

            if !queue.rows.is_empty()
                && place_to_save(&state, &queue.rows, queue.revision).reached
                    == Reached::StillOpening
            {
                return;
            }

            assert!(
                began.elapsed() < crate::harness::SETTLED_WITHIN,
                "the row never began opening"
            );

            std::thread::sleep(crate::harness::LOOKED_AGAIN_AFTER);
        }
    }

    #[test]
    fn a_queue_edited_while_its_song_opens_is_kept_through_a_shutdown_and_reopens_where_it_was_owed()
     {
        let file = CatalogFile::new();

        {
            let harness = Harness::over_the_catalog_in(&file);

            harness
                .player
                .catalog
                .save_queue(&harness.ids("abcd"), 1, 500)
                .expect("a saved queue");
            harness.hold_opens();
            harness
                .player
                .resume_saved_in(ZoneId::FIRST)
                .expect("the saved queue resumed");
            opening_its_row(&harness);
            harness
                .player
                .add_to_queue(harness.ids("a"))
                .expect("a song added while the place opens");
            harness.player.shutdown().expect("shut down");
        }

        let harness = Harness::over_the_catalog_in(&file);

        let kept = harness.player.catalog.resume().expect("read");

        harness
            .player
            .resume_saved_in(ZoneId::FIRST)
            .expect("the kept queue resumed");
        harness.settled(|snapshot| snapshot.current.is_some());

        assert_eq!(kept, (harness.ids("abcda"), 1, 500));
        assert_eq!(harness.queued(), "abcda");
        assert_eq!(harness.playing(), ('b', 1, 500));
    }

    #[test]
    fn a_window_of_the_queue_holds_the_rows_around_the_one_playing_and_says_where_it_begins() {
        assert_eq!(
            Shown::around(Some(10), 100, 3, 5),
            Shown { first: 7, rows: 9 }
        );
        assert_eq!(
            Shown::around(Some(1), 100, 3, 5),
            Shown { first: 0, rows: 7 }
        );
        assert_eq!(
            Shown::around(Some(98), 100, 3, 5),
            Shown { first: 95, rows: 5 }
        );
        assert_eq!(Shown::around(None, 4, 3, 5), Shown { first: 0, rows: 4 });
        assert_eq!(Shown::around(None, 0, 3, 5), Shown { first: 0, rows: 0 });
    }

    #[test]
    fn the_queue_is_read_as_a_window_around_the_row_playing() {
        let harness = crate::harness::Harness::new();

        harness.resumed("abcd", 2, 0);

        let window = harness.player.queue_window(1, 0).expect("a window");
        let whole = harness.player.queue().expect("the queue");

        assert_eq!(window.first, 1);
        assert_eq!(window.total, 4);
        assert_eq!(
            window
                .tracks
                .iter()
                .map(crate::harness::name_of)
                .collect::<String>(),
            "bc"
        );
        assert_eq!(
            window
                .tracks
                .iter()
                .map(|track| track.queue_id)
                .collect::<Vec<_>>(),
            whole[1..3]
                .iter()
                .map(|track| track.queue_id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_jump_with_a_place_lands_there_whatever_rate_the_catalog_names() {
        let harness = crate::harness::Harness::new();

        harness.resumed("abc", 0, 0);
        harness
            .player
            .catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET rate = 22050 WHERE id = ?1",
                [harness.song('c').id as i64],
            )
            .expect("a rate");

        harness.player.jump_to_at(2, 500).expect("a jump");

        let landed = harness.settled(|snapshot| {
            snapshot.position_ms > 0
                && snapshot
                    .current
                    .as_ref()
                    .is_some_and(|current| current.id == harness.song('c').id)
        });

        assert_eq!(landed.queue_index, 2);
        assert!(
            (450..550).contains(&landed.position_ms),
            "landed at {}",
            landed.position_ms
        );
    }
}
