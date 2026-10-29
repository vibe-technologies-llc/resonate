mod abx;
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
mod measurement;
mod mixes;
mod online_tools;
mod organising;
mod output;
mod partials;
mod playback;
mod playlist_files;
mod playlists;
mod queue_edit;
#[cfg(test)]
mod resuming_tests;
mod rewriting;
mod room;
mod saved_searches;
mod search;
mod served;
mod sharing;
mod stamp;
mod sync;
mod tagging;
mod vanished;
mod watching;
mod zoning;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use parking_lot::Mutex;
use resonate_codec::{Picturing, Sources, probe_pictured};
use resonate_core::{Frames, Resumable, Resumption, SampleRate, TrackId, Volume, ZoneId};
use resonate_engine::{
    Command, EngineConfig, OutputStatus, PlaybackState, PlayerState, QueueItem, QueueRows,
    RepeatMode,
};

use crate::{
    catalog::Catalog,
    documents::{GRANTED_SCHEMES, GrantedFiles, document_uri, location},
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
    rewriting::DocumentRewriting,
    saved_searches::{SavedSearch, SearchOrder, search_order_reads_descending},
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
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

fn place_to_save(state: &PlayerState, rows: &QueueRows, revision: u64) -> Option<SavedPlace> {
    if state.current.is_none() && row_being_opened(state, rows).is_some() {
        return None;
    }
    Some(SavedPlace {
        revision,
        row: state.queue_position.unwrap_or(0) as u32,
        position_ms: state.current.map_or(0, |track| {
            track.position.to_duration(track.source.rate).as_millis() as u64
        }),
    })
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SavedPlace {
    revision: u64,
    row: u32,
    position_ms: u64,
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
    served: served::Served,
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
        let served = served::Served::default();
        let sources = Arc::new(
            GRANTED_SCHEMES.iter().try_fold(
                Sources::local()
                    .spooling_under(Path::new(&folder).to_path_buf())
                    .and(served.provider()),
                |sources, scheme| {
                    Ok::<_, Error>(
                        sources.and(Arc::new(GrantedFiles::new(Arc::clone(&documents), scheme)?)),
                    )
                },
            )?,
        );
        let routing = Arc::new(output::Routing::new());
        let kept = catalog.kept_zones()?;
        let config = EngineConfig {
            volume: zoning::first_zones_volume(&kept),
            ..EngineConfig::default()
        };
        let player = output::player(config, Arc::clone(&sources), &routing)?;
        let engines: Box<zoning::Engines> = {
            let sources = Arc::clone(&sources);
            Box::new(
                move |_: ZoneId, config: EngineConfig, routing: &Arc<output::Routing>| {
                    output::player(config, Arc::clone(&sources), routing)
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
        });
        this.resume_saved_in(ZoneId::FIRST)?;
        watching::keep(&this, ZoneId::FIRST);
        this.restore_zones(&kept)?;
        Ok(this)
    }

    pub fn index_document(&self, uri: String, name: String) -> Result<Track> {
        self.indexed(uri, name, None, Absorbing::EveryHidden)
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
        self.indexed(uri, name, stamp, Absorbing::EveryHidden)
    }
}

impl MobilePlayer {
    fn indexed(
        &self,
        uri: String,
        name: String,
        stamp: Option<DocumentStamp>,
        absorbing: Absorbing,
    ) -> Result<Track> {
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
        self.take_the_place_of_missing(track.id, absorbing)?;
        self.catalog.track(track.id)
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn song_uris(&self) -> Result<Vec<String>> {
        self.catalog.uris()
    }

    pub fn queue(&self) -> Result<Vec<Track>> {
        let rows = self.playing.player()?.queued().rows;

        let uris = rows
            .iter()
            .filter(|item| !served::Served::names(&item.location))
            .map(|item| document_uri(&item.location))
            .collect::<Result<Vec<_>>>()?;
        let mut held = self.catalog.tracks_at(&uris)?.into_iter();
        let mut tracks = Vec::with_capacity(rows.len());
        for item in rows.iter() {
            let mut track = if served::Served::names(&item.location) {
                self.served.track_of(item)
            } else {
                let Some(track) = held.next() else {
                    break;
                };
                track
            };
            track.queue_id = Some(item.id.get());
            tracks.push(track);
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
        if let Some(rate) = self.rate_to_seek_in(&state, &queue.rows) {
            self.command(Command::Seek(Frames::from_duration(
                Duration::from_millis(position_ms),
                rate,
            )))?;
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
        let heard = state
            .current
            .map(|track| track.id)
            .or_else(|| row_being_opened(&state, &queue.rows));
        let current = heard.and_then(|id| self.queue_track(&queue.rows, id));
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
        let figures = OutputFigures::of(state.output.as_ref());
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
        })
    }

    pub fn save(&self) -> Result<()> {
        for zone in self.playing.listed() {
            self.save_zone(&zone)?;
        }
        Ok(())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.sync_stop();
        self.stop_enriching_to_close();
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
    fn save_zone(&self, zone: &resonate_engine::Zone) -> Result<()> {
        self.keep_volume_of(zone)?;
        let zoned = self.playing.zoned(zone.id)?;
        let mut saved = zoned.saved.lock();
        let (place, rows) = {
            let state = zone.player.state();
            let queue = zone.player.queued();
            let Some(place) = place_to_save(&state, &queue.rows, queue.revision) else {
                return Ok(());
            };
            (place, queue.rows)
        };
        if rows
            .iter()
            .any(|item| served::Served::names(&item.location))
        {
            return Ok(());
        }
        if saved.as_ref() == Some(&place) {
            return Ok(());
        }
        if saved
            .as_ref()
            .is_some_and(|saved| saved.revision == place.revision)
        {
            self.catalog
                .save_place_of(zone.id, place.row, place.position_ms)?;
        } else {
            let uris = rows
                .iter()
                .map(|item| document_uri(&item.location))
                .collect::<Result<Vec<_>>>()?;
            self.catalog.save_queue_of(
                zone.id,
                &self.catalog.ids_at(&uris)?,
                place.row,
                place.position_ms,
            )?;
        }
        *saved = Some(place);
        Ok(())
    }

    pub(crate) fn resume_saved_in(&self, zone: ZoneId) -> Result<()> {
        let (ids, at, position) = self.catalog.resume_of(zone)?;
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
            self.playing.send_to(
                zone,
                Command::Resume(Resumption {
                    rows,
                    row: at as usize,
                    at: Frames::from_duration(Duration::from_millis(position), rate),
                    ..Resumption::default()
                }),
            )?;
        }
        Ok(())
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

    fn rate_to_seek_in(&self, state: &PlayerState, rows: &QueueRows) -> Option<SampleRate> {
        match state.current {
            Some(track) => Some(track.source.rate),
            None => row_being_opened(state, rows)
                .and_then(|id| self.queue_track(rows, id))
                .and_then(|track| SampleRate::new(track.rate).ok()),
        }
    }

    fn command(&self, command: Command) -> Result<()> {
        self.playing.send(command)
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::{ChannelLayout, Frames, SampleFormat, StreamSpec};
    use resonate_engine::{OutputMode, SinkId, StreamVolume};

    use super::*;

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
}
