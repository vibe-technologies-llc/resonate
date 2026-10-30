mod formats;
#[cfg(any(target_os = "android", target_os = "ios", test))]
mod handover;
#[cfg(target_os = "android")]
mod oboe;
#[cfg(any(target_os = "android", test))]
mod oboe_frames;
#[cfg(any(target_os = "android", test))]
mod oboe_offers;
#[cfg(target_os = "android")]
mod oboe_render;
#[cfg(any(target_os = "android", test))]
mod oboe_reshape;
#[cfg(any(target_os = "android", test))]
mod oboe_state;
#[cfg(target_os = "ios")]
mod remote_io;
#[cfg(any(target_os = "ios", test))]
mod remote_io_buffers;
#[cfg(any(target_os = "ios", test))]
mod remote_io_fill;
#[cfg(target_os = "ios")]
mod remote_io_render;
#[cfg(any(target_os = "ios", test))]
mod remote_io_state;
#[cfg(any(target_os = "ios", test))]
mod remote_io_stream;
#[cfg(any(target_os = "ios", test))]
mod remote_io_unit;
#[cfg(any(target_os = "android", test))]
mod waits;

use std::sync::Arc;

use resonate_codec::Sources;
use resonate_engine::{EngineConfig, Player};
use resonate_upnp::Network;

pub use self::formats::{OutputFormat, OutputMixer, OutputStream};
#[cfg(target_os = "android")]
pub(crate) use self::oboe::Routing;
#[cfg(target_os = "ios")]
pub(crate) use self::remote_io_state::Routing;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct Routing {
    requested: std::sync::atomic::AtomicI32,
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Routing {
    pub(crate) fn new() -> Self {
        Self {
            requested: std::sync::atomic::AtomicI32::new(0),
        }
    }

    pub(crate) fn serving(_: resonate_audio::SinkId) -> Self {
        Self::new()
    }

    pub(crate) fn select(&self, device: i32) -> bool {
        self.requested
            .store(device, std::sync::atomic::Ordering::Relaxed);
        true
    }

    pub(crate) fn actual(&self) -> i32 {
        0
    }

    pub(crate) fn requested(&self) -> i32 {
        self.requested.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn route_changed(&self, _: u32, _: f64) -> bool {
        false
    }

    pub(crate) fn follow_song_rate(&self, _: bool) -> bool {
        false
    }

    pub(crate) fn offer(&self, _: i32, _: &[OutputFormat]) -> bool {
        false
    }

    pub(crate) fn stream(&self) -> Option<OutputStream> {
        None
    }

    pub(crate) fn watch_mixer(&self, _: Arc<dyn OutputMixer>) {}
}

#[cfg(target_os = "android")]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    routing: &Arc<Routing>,
    network: Option<Arc<Network>>,
) -> resonate_engine::Result<Player> {
    let routing = Arc::clone(routing);
    Player::with_sources_and_backend(config, sources, move |_| {
        Ok(beside(Box::new(oboe::AndroidAudio::new(routing)), network))
    })
}

#[cfg(target_os = "ios")]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    routing: &Arc<Routing>,
    network: Option<Arc<Network>>,
) -> resonate_engine::Result<Player> {
    let routing = Arc::clone(routing);
    Player::with_sources_and_backend(config, sources, move |_| {
        Ok(beside(Box::new(remote_io::IosAudio::new(routing)), network))
    })
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    _: &Arc<Routing>,
    _: Option<Arc<Network>>,
) -> resonate_engine::Result<Player> {
    Player::with_sources(config, sources)
}

#[cfg(any(target_os = "android", target_os = "ios", test))]
pub(crate) fn beside(
    phone: Box<dyn resonate_audio::Backend>,
    network: Option<Arc<Network>>,
) -> Box<dyn resonate_audio::Backend> {
    match network {
        Some(network) => Box::new(resonate_upnp::Beside::over(phone, network)),
        None => phone,
    }
}
