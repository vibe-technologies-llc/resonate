mod activity;
mod error;
mod frame;
mod publish;
mod session;
#[cfg(unix)]
mod socket;
#[cfg(windows)]
#[path = "pipe.rs"]
mod socket;

mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

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

#[cfg(test)]
mod said_through_the_catalog {
    use std::path::Path;

    const PROPER: &[&str] = &[
        "Resonate",
        "app/com.discordapp.Discord",
        "app/com.discordapp.DiscordCanary",
        "app/com.discordapp.DiscordPTB",
        "app/dev.vencord.Vesktop",
        ".flatpak/com.discordapp.Discord/xdg-run",
        ".flatpak/com.discordapp.DiscordCanary/xdg-run",
        ".flatpak/com.discordapp.DiscordPTB/xdg-run",
        ".flatpak/dev.vencord.Vesktop/xdg-run",
    ];

    #[test]
    fn every_word_discord_shows_for_the_player_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found = resonate_words_build::prose_in(&root, PROPER).expect("the sources read");

        let written: Vec<String> = found
            .iter()
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();

        assert!(
            written.is_empty(),
            "words written in the code:\n{}",
            written.join("\n")
        );
    }
}
