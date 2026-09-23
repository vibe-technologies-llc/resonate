mod activity;
mod error;
mod frame;
mod publish;
mod socket;

use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use resonate_core::{FrameSpan, MediaLocation, Presence};
use resonate_engine::Player;

use crate::publish::Running;
pub use crate::{
    activity::Cover,
    error::{Error, IpcOp, JsonOp, Result},
};

pub trait Releases: Send + Sync + 'static {
    fn cover(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Cover>;
}

pub struct Discord {
    player: Arc<Player>,
    releases: Arc<dyn Releases>,
    presence: Arc<RwLock<Presence>>,
    running: Mutex<Option<Running>>,
}

impl Discord {
    pub fn new(player: Arc<Player>, releases: Arc<dyn Releases>) -> Self {
        Self {
            player,
            releases,
            presence: Arc::new(RwLock::new(Presence::OFF)),
            running: Mutex::new(None),
        }
    }

    pub fn follow(&self, presence: &Presence) {
        presence.clone_into(&mut self.presence.write());
        let mut running = self.running.lock();

        match (presence.active(), running.take()) {
            (true, Some(publishing)) => {
                publishing.nudge();
                *running = Some(publishing);
            }
            (true, None) => {
                *running = Running::start(
                    Arc::clone(&self.player),
                    Arc::clone(&self.releases),
                    Arc::clone(&self.presence),
                );
            }
            (false, Some(publishing)) => publishing.stop(),
            (false, None) => {}
        }
    }
}

impl Drop for Discord {
    fn drop(&mut self) {
        if let Some(publishing) = self.running.get_mut().take() {
            publishing.stop();
        }
    }
}
