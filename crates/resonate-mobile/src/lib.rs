mod catalog;
mod catalog_extras;
mod collections;
mod deleting;
mod documents;
mod downloads;
mod enrichment;
mod equaliser;
mod error;
mod failure;
mod features;
#[cfg(test)]
mod harness;
mod listening;
mod logging;
mod loves;
mod lyrics;
mod mixes;
mod online_tools;
mod organising;
mod output;
mod playback;
mod playlist_files;
mod playlists;
mod queue_edit;
mod room;
mod search;
mod sharing;
mod stamp;
mod sync;
mod tagging;
mod vanished;
mod watching;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_codec::{Picturing, Sources, probe_pictured};
use resonate_core::{Frames, Resumable, Resumption, SampleRate, TrackId, Volume};
use resonate_engine::{
    Command, EngineConfig, PlaybackState, Player, PlayerState, QueueItem, QueueRows, RepeatMode,
};

use crate::{
    catalog::Catalog,
    documents::{GRANTED_SCHEMES, GrantedFiles, document_uri, location},
    stamp::DocumentStamp,
};
pub use crate::{
    documents::{Documents, audio_extensions},
    equaliser::{
        EqualiserBand, EqualiserCurve, EqualiserFilter, EqualiserImport, EqualiserLimits,
        EqualiserPreset, equaliser_frequencies, equaliser_limits, equaliser_presets,
        equaliser_text, fitted_preamp, parse_eq_profile, response_curve, ten_band_curve,
        ten_band_gains,
    },
    error::{Error, Result, StoreOp},
    logging::{LogLevel, Logbook, keep_log},
    playback::{
        GainMode, LossyRestoration, PlaybackPreferences, ResamplingPhase, ResamplingQuality,
        SleepTimer, default_playback_preferences,
    },
    watching::PlayerWatch,
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

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum Transport {
    Stopped,
    Loading,
    Playing,
    Paused,
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

const fn transport_of(playback: PlaybackState) -> Transport {
    match playback {
        PlaybackState::Idle | PlaybackState::Stopped => Transport::Stopped,
        PlaybackState::Buffering => Transport::Loading,
        PlaybackState::Playing => Transport::Playing,
        PlaybackState::Paused => Transport::Paused,
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SavedPlace {
    revision: u64,
    row: u32,
    position_ms: u64,
}

#[derive(uniffi::Object)]
pub struct MobilePlayer {
    player: Mutex<Option<Player>>,
    catalog: Arc<Catalog>,
    sources: Arc<Sources>,
    failure: Mutex<Option<String>>,
    waiting: Mutex<Option<String>>,
    saved: Mutex<Option<SavedPlace>>,
    preferences: Mutex<PlaybackPreferences>,
    services: features::Services,
    playlist_undo: Mutex<Vec<playlists::PlaylistUndo>>,
    downloads: downloads::Downloads,
    syncing: sync::Syncing,
    routing: Arc<output::Routing>,
    enriching: enrichment::Enriching,
    room: room::Room,
    tag_undo: tagging::TagUndo,
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
        let sources = Arc::new(GRANTED_SCHEMES.iter().try_fold(
            Sources::local().spooling_under(Path::new(&folder).to_path_buf()),
            |sources, scheme| {
                Ok::<_, Error>(
                    sources.and(Arc::new(GrantedFiles::new(Arc::clone(&documents), scheme)?)),
                )
            },
        )?);
        let config = EngineConfig::default();
        let routing = Arc::new(output::Routing::new());
        let player = output::player(config, Arc::clone(&sources), &routing)?;
        let this = Arc::new(Self {
            player: Mutex::new(Some(player)),
            catalog,
            sources,
            failure: Mutex::new(None),
            waiting: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: features::Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: downloads::Downloads::default(),
            syncing: sync::Syncing::under(PathBuf::from(&folder)),
            routing,
            enriching: enrichment::Enriching::default(),
            room: room::Room::under(PathBuf::from(&folder)),
            tag_undo: tagging::TagUndo::default(),
        });
        this.resume_saved()?;
        watching::keep(&this);
        Ok(this)
    }

    pub fn index_document(&self, uri: String, name: String) -> Result<Track> {
        self.indexed(uri, name, None)
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
        self.indexed(uri, name, stamp)
    }
}

impl MobilePlayer {
    fn indexed(&self, uri: String, name: String, stamp: Option<DocumentStamp>) -> Result<Track> {
        let (info, picture) = probe_pictured(&self.sources, &location(&uri)?, Picturing::Copied)?;
        let duration_ms = info.duration.map_or(0, |frames| {
            frames
                .to_duration(info.spec.rate)
                .as_millis()
                .min(u128::from(u64::MAX)) as u64
        });
        let track = self.catalog.upsert(Track {
            id: 0,
            uri,
            title: info.tags.title.clone().unwrap_or_else(|| name.clone()),
            name,
            artist: info.tags.artist.clone().unwrap_or_default(),
            album: info.tags.album.clone().unwrap_or_default(),
            duration_ms,
            rate: info.spec.rate.hz(),
            codec: resonate_codec::Codec::from_id(info.codec).to_string(),
            favourite: false,
            ..Track::default()
        })?;
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
        self.take_the_place_of_missing(track.id)?;
        self.catalog.track(track.id)
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn song_uris(&self) -> Result<Vec<String>> {
        self.catalog.uris()
    }

    pub fn tracks(&self, text: String, offset: u32, limit: u32) -> Result<Vec<Track>> {
        self.catalog.tracks(&text, offset, limit)
    }

    pub fn queue(&self) -> Result<Vec<Track>> {
        let rows = {
            let held = self.player.lock();
            held.as_ref().ok_or(Error::Closed)?.queued().rows
        };

        let uris = rows
            .iter()
            .map(|item| document_uri(&item.location))
            .collect::<Result<Vec<_>>>()?;
        let mut tracks = self.catalog.tracks_at(&uris)?;
        for (track, item) in tracks.iter_mut().zip(rows.iter()) {
            track.queue_id = Some(item.id.get());
        }
        Ok(tracks)
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
        if !autoplay {
            return self.command(Command::Resume(Resumption {
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
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        for command in commands {
            player.send(command)?;
        }
        *previous = preferences;
        Ok(())
    }

    pub fn set_sleep(&self, timer: Option<SleepTimer>) -> Result<()> {
        self.command(Command::SleepUntil(timer.map(Into::into)))
    }

    pub fn seek(&self, position_ms: u64) -> Result<()> {
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        if let Some(track) = player.state().current {
            player.send(Command::Seek(Frames::from_duration(
                Duration::from_millis(position_ms),
                track.source.rate,
            )))?;
        }
        Ok(())
    }

    pub fn transport(&self) -> Result<Transport> {
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        Ok(transport_of(player.state().playback))
    }

    pub fn snapshot(&self) -> Result<Snapshot> {
        let (events, state, queue) = {
            let held = self.player.lock();
            let player = held.as_ref().ok_or(Error::Closed)?;
            let events = player.events().try_iter().collect::<Vec<_>>();
            (events, player.state(), player.queued())
        };
        self.take_in(events, &queue.rows);
        let waiting = self.still_waiting(&state);
        let heard = state
            .current
            .map(|track| track.id)
            .or_else(|| row_being_opened(&state, &queue.rows));
        let current = heard.and_then(|id| {
            queue
                .rows
                .iter()
                .find(|item| item.id == id)
                .and_then(|item| {
                    document_uri(&item.location)
                        .ok()
                        .and_then(|uri| self.catalog.kept_track_uri(&uri).ok())
                        .map(|mut track| {
                            track.queue_id = Some(item.id.get());
                            track
                        })
                })
        });
        let duration_ms = state
            .current
            .and_then(|track| {
                track
                    .duration
                    .map(|frames| frames.to_duration(track.source.rate).as_millis() as u64)
            })
            .unwrap_or_else(|| current.as_ref().map_or(0, |track| track.duration_ms));
        let queue_revision = queue.revision;
        let next_first = queue.next.map(|next| next.first() as u32);
        let next_count = queue
            .next
            .map_or(0, |next| (next.last() - next.first() + 1) as u32);
        Ok(Snapshot {
            transport: transport_of(state.playback),
            current,
            queue_index: state.queue_position.unwrap_or(0) as u32,
            position_ms: state.current.map_or(0, |track| {
                track.position.to_duration(track.source.rate).as_millis() as u64
            }),
            duration_ms,
            volume: state.volume.get(),
            shuffle: state.shuffle,
            repeat: match state.repeat {
                RepeatMode::Off => 0,
                RepeatMode::Track => 1,
                RepeatMode::Queue => 2,
            },
            output_rate: state.output.map_or(0, |output| output.negotiated.rate.hz()),
            underruns: state.output.map_or(0, |output| output.underruns),
            failure: self.failure.lock().take(),
            sleep: state.sleeping.map(|sleep| sleep.until.into()),
            sleep_remaining_ms: state
                .sleeping
                .and_then(|sleep| sleep.left.map(|left| left.as_millis() as u64)),
            queue_revision,
            catalog_revision: self.catalog.revision(),
            next_first,
            next_count,
            waiting,
        })
    }

    pub fn save(&self) -> Result<()> {
        let mut saved = self.saved.lock();
        let (place, rows) = {
            let held = self.player.lock();
            let player = held.as_ref().ok_or(Error::Closed)?;
            let state = player.state();
            let queue = player.queued();
            let place = SavedPlace {
                revision: queue.revision,
                row: state.queue_position.unwrap_or(0) as u32,
                position_ms: state.current.map_or(0, |track| {
                    track.position.to_duration(track.source.rate).as_millis() as u64
                }),
            };
            (place, queue.rows)
        };
        if saved.as_ref() == Some(&place) {
            return Ok(());
        }
        if saved
            .as_ref()
            .is_some_and(|saved| saved.revision == place.revision)
        {
            self.catalog.save_place(place.row, place.position_ms)?;
        } else {
            let uris = rows
                .iter()
                .map(|item| document_uri(&item.location))
                .collect::<Result<Vec<_>>>()?;
            self.catalog
                .save_queue(&self.catalog.ids_at(&uris)?, place.row, place.position_ms)?;
        }
        *saved = Some(place);
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.sync_stop();
        self.stop_enriching_to_close();
        self.configure_online(false);
        self.cancel_analysis();
        self.downloads.cancel_every_job();
        self.finish_listening()?;
        self.save()?;
        if let Some(player) = self.player.lock().take() {
            player.shutdown()?;
        }
        Ok(())
    }
}

impl MobilePlayer {
    pub(crate) fn resume_saved(&self) -> Result<()> {
        let (ids, at, position) = self.catalog.resume()?;
        if !ids.is_empty() {
            let tracks = self.catalog.tracks_by_id(&ids)?;
            let rate = tracks
                .get(at as usize)
                .or_else(|| tracks.first())
                .map_or(SampleRate::HZ_48000, |track| {
                    SampleRate::new(track.rate).unwrap_or(SampleRate::HZ_48000)
                });
            let rows = tracks
                .into_iter()
                .map(|track| {
                    Ok(Resumable {
                        location: location(&track.uri)?,
                        span: None,
                        held: Some(TrackId::new(track.id)?),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            self.command(Command::Resume(Resumption {
                rows,
                row: at as usize,
                at: Frames::from_duration(Duration::from_millis(position), rate),
                ..Resumption::default()
            }))?;
        }
        Ok(())
    }

    fn command(&self, command: Command) -> Result<()> {
        self.player
            .lock()
            .as_ref()
            .ok_or(Error::Closed)?
            .send(command)?;
        Ok(())
    }
}
