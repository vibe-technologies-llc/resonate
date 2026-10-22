#[cfg(target_os = "android")]
mod oboe;
#[cfg(target_os = "android")]
mod oboe_render;
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
#[cfg(any(target_os = "android", test))]
mod waits;

use std::sync::Arc;

use resonate_codec::Sources;
use resonate_engine::{EngineConfig, Player};

#[cfg(target_os = "android")]
pub(crate) use self::oboe::Routing;
#[cfg(target_os = "ios")]
pub(crate) use self::remote_io_state::Routing;
use crate::Result;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct Routing;

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Routing {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn select(&self, _: i32) -> bool {
        false
    }

    pub(crate) fn actual(&self) -> i32 {
        0
    }

    pub(crate) fn route_changed(&self, _: u32, _: f64) -> bool {
        false
    }
}

#[cfg(target_os = "android")]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    routing: &Arc<Routing>,
) -> Result<Player> {
    let routing = Arc::clone(routing);
    Ok(Player::with_sources_and_backend(
        config,
        sources,
        move |_| Ok(Box::new(oboe::AndroidAudio::new(routing))),
    )?)
}

#[cfg(target_os = "ios")]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    routing: &Arc<Routing>,
) -> Result<Player> {
    let routing = Arc::clone(routing);
    Ok(Player::with_sources_and_backend(
        config,
        sources,
        move |_| Ok(Box::new(remote_io::IosAudio::new(routing))),
    )?)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn player(
    config: EngineConfig,
    sources: Arc<Sources>,
    _: &Arc<Routing>,
) -> Result<Player> {
    Ok(Player::with_sources(config, sources)?)
}
