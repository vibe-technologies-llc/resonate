#[cfg(target_os = "android")]
mod backend;
mod catalog;
mod catalog_extras;
mod collections;
mod documents;
mod downloads;
mod error;
mod features;
mod online_tools;
mod playback;
mod playlists;
#[cfg(target_os = "android")]
mod process;

use std::{fs, path::Path, sync::Arc, time::Duration};

use parking_lot::Mutex;
use resonate_codec::{Picturing, Sources, probe_pictured};
use resonate_core::{Frames, Resumable, Resumption, SampleRate, TrackId, Volume};
use resonate_engine::{Command, EngineConfig, Event, PlaybackState, Player, QueueItem, RepeatMode};

use crate::{
    catalog::Catalog,
    documents::{GrantedFiles, document_uri, location},
};
pub use crate::{
    documents::Documents,
    error::{Error, Result, StoreOp},
    playback::{
        GainMode, LossyRestoration, PlaybackPreferences, ResamplingPhase, ResamplingQuality,
        SleepTimer, default_playback_preferences, equaliser_frequencies,
    },
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct SavedPlace {
    revision: u64,
    row: u32,
    position_ms: u64,
}

#[derive(uniffi::Object)]
pub struct AndroidPlayer {
    player: Mutex<Option<Player>>,
    catalog: Catalog,
    sources: Arc<Sources>,
    failure: Mutex<Option<String>>,
    saved: Mutex<Option<SavedPlace>>,
    preferences: Mutex<PlaybackPreferences>,
    services: features::Services,
    playlist_undo: Mutex<Vec<playlists::PlaylistUndo>>,
    downloads: downloads::Downloads,
    #[cfg(target_os = "android")]
    routing: Arc<backend::Routing>,
}

#[uniffi::export]
impl AndroidPlayer {
    #[uniffi::constructor]
    pub fn new(folder: String, documents: Box<dyn Documents>) -> Result<Arc<Self>> {
        fs::create_dir_all(&folder).map_err(|source| Error::Folder {
            op: StoreOp::CreateFolder,
            source,
        })?;
        let catalog = Catalog::open(&Path::new(&folder).join("documents.sqlite"))?;
        let granted = GrantedFiles::new(Arc::from(documents))?;
        let sources = Arc::new(
            Sources::local()
                .spooling_under(Path::new(&folder).to_path_buf())
                .and(Arc::new(granted)),
        );
        let config = EngineConfig::default();
        #[cfg(target_os = "android")]
        let routing = Arc::new(backend::Routing::default());
        #[cfg(target_os = "android")]
        let player = {
            let backend_routing = Arc::clone(&routing);
            Player::with_sources_and_backend(config, Arc::clone(&sources), move |_| {
                Ok(Box::new(backend::AndroidAudio::new(backend_routing)))
            })?
        };
        #[cfg(not(target_os = "android"))]
        let player = Player::with_sources(config, Arc::clone(&sources))?;
        let this = Arc::new(Self {
            player: Mutex::new(Some(player)),
            catalog,
            sources,
            failure: Mutex::new(None),
            saved: Mutex::new(None),
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: features::Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: downloads::Downloads::default(),
            #[cfg(target_os = "android")]
            routing,
        });
        let (ids, at, position) = this.catalog.resume()?;
        if !ids.is_empty() {
            let tracks = ids
                .into_iter()
                .map(|id| this.catalog.track(id))
                .collect::<Result<Vec<_>>>()?;
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
            this.command(Command::Resume(Resumption {
                rows,
                row: at as usize,
                at: Frames::from_duration(Duration::from_millis(position), rate),
                ..Resumption::default()
            }))?;
        }
        Ok(this)
    }

    pub fn index_document(&self, uri: String, name: String) -> Result<Track> {
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
        self.catalog.tagged(track.id, &info)?;
        if let Some(picture) = picture.into_copied() {
            self.catalog
                .keep_artwork(track.id, &picture.bytes, "embedded", true)?;
        }
        if let Some(text) = info.tags.lyrics
            && let Err(error) = self.keep_lyric_text(track.id, text)
        {
            tracing::debug!(%error, "embedded lyrics could not be read");
        }
        self.catalog.track(track.id)
    }

    pub fn tracks(&self, text: String, offset: u32, limit: u32) -> Result<Vec<Track>> {
        self.catalog.tracks(&text, offset, limit)
    }

    pub fn queue(&self) -> Result<Vec<Track>> {
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        player
            .queued()
            .rows
            .iter()
            .map(|item| {
                self.catalog
                    .track_uri(&document_uri(&item.location)?)
                    .map(|mut track| {
                        track.queue_id = Some(item.id.get());
                        track
                    })
            })
            .collect()
    }

    pub fn load(&self, ids: Vec<u64>, start_at: u32, autoplay: bool) -> Result<()> {
        let items: Vec<QueueItem> = ids
            .into_iter()
            .map(|id| {
                let track = self.catalog.track(id)?;
                Ok(QueueItem::whole(TrackId::new(id)?, location(&track.uri)?))
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

    pub fn snapshot(&self) -> Result<Snapshot> {
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        for event in player.events().try_iter() {
            if let Event::Failed { error, .. } | Event::CommandFailed { error, .. } = event {
                *self.failure.lock() = Some(error.to_string());
            }
        }
        let state = player.state();
        self.count_listening(&state, &player.queued().rows)?;
        let queue = player.queued();
        let current = state.current.and_then(|track| {
            queue
                .rows
                .iter()
                .find(|item| item.id == track.id)
                .and_then(|item| {
                    document_uri(&item.location)
                        .ok()
                        .and_then(|uri| self.catalog.track_uri(&uri).ok())
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
        Ok(Snapshot {
            transport: match state.playback {
                PlaybackState::Idle | PlaybackState::Stopped => Transport::Stopped,
                PlaybackState::Buffering => Transport::Loading,
                PlaybackState::Playing => Transport::Playing,
                PlaybackState::Paused => Transport::Paused,
            },
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
        })
    }

    pub fn save(&self) -> Result<()> {
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
        let mut saved = self.saved.lock();
        if saved.as_ref() == Some(&place) {
            return Ok(());
        }
        if saved
            .as_ref()
            .is_some_and(|saved| saved.revision == place.revision)
        {
            self.catalog.save_place(place.row, place.position_ms)?;
        } else {
            self.catalog.save_queue(
                &queue
                    .rows
                    .iter()
                    .map(|item| {
                        self.catalog
                            .track_uri(&document_uri(&item.location)?)
                            .map(|track| track.id)
                    })
                    .collect::<Result<Vec<_>>>()?,
                place.row,
                place.position_ms,
            )?;
        }
        *saved = Some(place);
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.configure_online(false);
        self.cancel_analysis();
        self.cancel_download();
        self.finish_listening()?;
        self.save()?;
        if let Some(player) = self.player.lock().take() {
            player.shutdown()?;
        }
        Ok(())
    }
}

impl AndroidPlayer {
    fn command(&self, command: Command) -> Result<()> {
        self.player
            .lock()
            .as_ref()
            .ok_or(Error::Closed)?
            .send(command)?;
        Ok(())
    }
}
