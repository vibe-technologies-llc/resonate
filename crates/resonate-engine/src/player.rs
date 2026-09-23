use std::{
    sync::{Arc, atomic::Ordering},
    thread::{self, JoinHandle},
    time::Duration,
};

use crossbeam_channel::{Receiver, Sender, bounded, unbounded};
use resonate_analysis::{Analysis, Watching};
use resonate_codec::Sources;
use resonate_core::{FrameSpan, MediaLocation};
use resonate_pipewire::{PipeWire, SinkInfo};

use crate::{
    Backend, Command, CoverArt, Engine, EngineConfig, Error, Event, Landing, MediaInfo, Outcome,
    OutputSettings, PlayerState, Published, QueueItem, Queued, Request, Result, StreamDigest,
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
    thread: Option<JoinHandle<()>>,
}

impl Player {
    pub fn new(config: EngineConfig) -> Result<Self> {
        Self::with_sources(config, Arc::new(Sources::local()))
    }

    pub fn with_sources(config: EngineConfig, sources: Arc<Sources>) -> Result<Self> {
        Self::spawn(config, sources, |config| {
            let backend = PipeWire::start(&config.app_name)?;
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
                            let engine = Engine::new(
                                config, sources, backend, requests, announce, published,
                            );
                            let _ = ready.send(Ok(()));
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

    pub fn queue(&self) -> Arc<Vec<QueueItem>> {
        Arc::clone(&self.published.queue.read().rows)
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
        resonate_analysis::analyse(&self.sources, location, span, watch)
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

impl Drop for Player {
    fn drop(&mut self) {
        self.stop_thread();
    }
}
