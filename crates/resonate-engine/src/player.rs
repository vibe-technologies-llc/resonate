use std::{
    sync::{Arc, atomic::Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};

use ahash::AHashSet;
use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use resonate_analysis::{Analysis, CUTS_AT_MOST, Cut, KeptAnalyses, Loudest, Watching};
use resonate_audio::SinkInfo;
use resonate_codec::Sources;
use resonate_core::{FrameSpan, MediaLocation, eq::Profile};
#[cfg(target_os = "linux")]
use resonate_pipewire::PipeWire;

use crate::{
    Backend, Command, CoverArt, Engine, EngineConfig, Error, Event, Landing, MediaInfo, Outcome,
    OutputSettings, PlayerState, Published, QueueRows, Queued, Request, Result, StreamDigest,
    Tapped,
    catalog::{ArtRead, Catalog, TagsRead},
};

const EVENT_SLOTS: usize = 256;

pub struct Player {
    commands: Option<Sender<Request>>,
    events: Receiver<Event>,
    published: Published,
    catalog: Catalog,
    sources: Arc<Sources>,
    kept: Option<KeptAnalyses>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    #[cfg(target_os = "linux")]
    pub fn new(config: EngineConfig) -> Result<Self> {
        Self::with_sources(config, Arc::new(Sources::local()))
    }

    #[cfg(target_os = "linux")]
    pub fn with_sources(config: EngineConfig, sources: Arc<Sources>) -> Result<Self> {
        Self::spawn(config, sources, |config| {
            let backend = PipeWire::start(&config.app_name).map_err(resonate_audio::Error::from)?;
            Ok(Box::new(backend))
        })
    }

    pub fn with_backend<F>(config: EngineConfig, open: F) -> Result<Self>
    where
        F: FnOnce(&EngineConfig) -> Result<Box<dyn Backend>> + Send + 'static,
    {
        Self::spawn(config, Arc::new(Sources::local()), open)
    }

    pub fn with_sources_and_backend<F>(
        config: EngineConfig,
        sources: Arc<Sources>,
        open: F,
    ) -> Result<Self>
    where
        F: FnOnce(&EngineConfig) -> Result<Box<dyn Backend>> + Send + 'static,
    {
        Self::spawn(config, sources, open)
    }

    fn spawn<F>(config: EngineConfig, sources: Arc<Sources>, open: F) -> Result<Self>
    where
        F: FnOnce(&EngineConfig) -> Result<Box<dyn Backend>> + Send + 'static,
    {
        let (commands, requests) = unbounded();
        let (announce, events) = bounded(EVENT_SLOTS);
        let (ready, started) = bounded(1);
        let published = Published::default();

        let thread = {
            let published = published.clone();
            let sources = Arc::clone(&sources);
            thread::Builder::new()
                .name("resonate-engine".to_owned())
                .spawn(move || {
                    match open(&config) {
                        Ok(backend) => {
                            *published.settings.write() = Arc::new(OutputSettings::of(&config));
                            let _ = ready.send(Ok(()));
                            let engine = Engine::new(
                                config, sources, backend, requests, announce, published,
                            );
                            engine.run();
                        }
                        Err(error) => {
                            let _ = ready.send(Err(error));
                        }
                    };
                })
                .map_err(|_| Error::EngineStopped)?
        };

        match started.recv() {
            Ok(Ok(())) => Ok(Self {
                commands: Some(commands),
                events,
                published,
                catalog: Catalog::new(Arc::clone(&sources)),
                sources,
                kept: None,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(error)
            }
            Err(_) => Err(Error::EngineStopped),
        }
    }

    pub fn send(&self, command: Command) -> Result<()> {
        self.post(Request::told(command))
    }

    pub fn send_if_the_queue_is_still(&self, revision: u64, command: Command) -> Result<()> {
        self.post(Request::told(command).if_the_queue_is_still(revision))
    }

    pub fn request_if_the_queue_is_still(
        &self,
        revision: u64,
        command: Command,
    ) -> Result<Outcome> {
        let (request, outcome) = Request::asked(command);
        self.post(request.if_the_queue_is_still(revision))?;
        Ok(outcome)
    }

    pub fn request(&self, command: Command) -> Result<Outcome> {
        let (request, outcome) = Request::asked(command);
        self.post(request)?;
        Ok(outcome)
    }

    pub fn settle(&self, command: Command) -> Result<Landing> {
        let (request, landing) = Request::settled(command);
        self.post(request)?;
        Ok(landing)
    }

    fn post(&self, request: Request) -> Result<()> {
        self.commands
            .as_ref()
            .ok_or(Error::EngineStopped)?
            .send(request)
            .map_err(|_| Error::EngineStopped)
    }

    pub fn state(&self) -> PlayerState {
        self.published.state.read().clone()
    }

    pub fn output_settings(&self) -> Arc<OutputSettings> {
        Arc::clone(&self.published.settings.read())
    }

    pub fn queue(&self) -> QueueRows {
        self.published.queue.read().rows.clone()
    }

    pub fn queued(&self) -> Queued {
        self.published.queue.read().clone()
    }

    pub const fn events(&self) -> &Receiver<Event> {
        &self.events
    }

    pub fn sinks(&self) -> Arc<[SinkInfo]> {
        Arc::clone(&self.published.sinks.read())
    }

    pub fn sinks_once_surveyed(&self, within: Duration) -> Arc<[SinkInfo]> {
        if !self.published.first_survey.waited_for(within) {
            tracing::debug!(?within, "the first sink survey had not answered in time");
        }
        self.sinks()
    }

    pub fn digest(&self) -> Option<Arc<StreamDigest>> {
        self.published.digest.read().clone()
    }

    pub fn tap(&self) -> Tapped {
        self.published.tap.read().clone()
    }

    pub fn listen_in(&self, listening: bool) {
        self.published.listening.store(listening, Ordering::Relaxed);
    }

    pub fn media(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Option<Arc<MediaInfo>> {
        self.catalog.media(location, span)
    }

    pub fn tags_read(&self, location: &MediaLocation, span: Option<FrameSpan>) -> TagsRead {
        self.catalog.tags_read(location, span)
    }

    pub fn art(&self, location: &MediaLocation) -> Option<Arc<CoverArt>> {
        self.catalog.art(location)
    }

    pub fn art_read(&self, location: &MediaLocation) -> ArtRead {
        self.catalog.art_read(location)
    }

    pub fn media_within(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        patience: Duration,
    ) -> Option<Arc<MediaInfo>> {
        self.catalog.media_within(location, span, patience)
    }

    pub fn analyse(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        watch: &dyn Watching,
    ) -> resonate_analysis::Result<Analysis> {
        if let Some(recalled) = self
            .kept
            .as_ref()
            .and_then(|kept| kept.recalled(location, span))
        {
            return Ok(recalled);
        }
        let analysis = resonate_analysis::analyse(&self.sources, location, span, watch)?;
        if let Some(kept) = &self.kept {
            kept.keep(location, span, &analysis);
        }
        Ok(analysis)
    }

    pub fn cuts_to_fit(&self) -> Vec<Cut> {
        cuts_to_fit(&self.queue(), self.state().queue_position)
    }

    pub fn loudest_among(
        &self,
        cuts: &[Cut],
        profile: &Arc<Profile>,
        watch: &dyn Watching,
    ) -> resonate_analysis::Result<Loudest> {
        resonate_analysis::loudest_through(&self.sources, cuts, profile, watch)
    }

    pub fn keeping_analyses(mut self, kept: KeptAnalyses) -> Self {
        self.kept = Some(kept);
        self
    }

    pub fn media_revision(&self) -> u64 {
        self.catalog.revision()
    }

    pub fn shutdown(mut self) -> Result<()> {
        self.stop_thread();
        Ok(())
    }

    fn stop_thread(&mut self) {
        drop(self.commands.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub fn cuts_to_fit(rows: &QueueRows, playing: Option<usize>) -> Vec<Cut> {
    let first = playing.filter(|row| *row < rows.len()).unwrap_or(0);
    let mut seen = AHashSet::new();
    rows.iter()
        .skip(first)
        .chain(rows.iter().take(first))
        .filter(|item| seen.insert((item.location.clone(), item.span)))
        .take(CUTS_AT_MOST)
        .map(|item| Cut {
            location: item.location.clone(),
            span: item.span,
        })
        .collect()
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop_thread();
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::TrackId;

    use super::*;
    use crate::QueueItem;

    fn row(number: u64, name: &str) -> QueueItem {
        QueueItem::whole(
            TrackId::new(number).expect("a track id"),
            MediaLocation::local(name),
        )
    }

    #[test]
    fn the_cuts_to_fit_start_at_the_playing_row_wrap_round_and_meet_each_song_once() {
        let rows: QueueRows = [row(1, "a"), row(2, "b"), row(3, "a"), row(4, "c")]
            .into_iter()
            .collect();

        let names: Vec<_> = cuts_to_fit(&rows, Some(1))
            .into_iter()
            .map(|cut| cut.location)
            .collect();

        assert_eq!(
            names,
            vec![
                MediaLocation::local("b"),
                MediaLocation::local("a"),
                MediaLocation::local("c"),
            ]
        );
        assert_eq!(cuts_to_fit(&rows, None).len(), 3);
        assert_eq!(cuts_to_fit(&rows, Some(40)).len(), 3);
    }

    #[test]
    fn the_cuts_to_fit_stop_at_the_most_the_fit_will_decode() {
        let rows: QueueRows = (1..=CUTS_AT_MOST as u64 * 2)
            .map(|number| row(number, &format!("song-{number}")))
            .collect();

        assert_eq!(cuts_to_fit(&rows, None).len(), CUTS_AT_MOST);
    }
}
