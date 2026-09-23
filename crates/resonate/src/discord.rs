use std::sync::Arc;

use resonate_core::Presence;
#[cfg(feature = "discord")]
use resonate_core::{FrameSpan, MediaLocation};
#[cfg(feature = "discord")]
use resonate_discord::{Cover, Discord, Releases};
use resonate_engine::Player;
use resonate_library::Library;

use crate::config::Config;

#[cfg(feature = "discord")]
struct Catalogued {
    library: Option<Arc<Library>>,
}

#[cfg(feature = "discord")]
impl Releases for Catalogued {
    fn cover(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Cover> {
        let library = self.library.as_ref()?;
        let path = location.as_path()?;
        let released = library.track_at(path, span).and_then(|track| {
            match track.and_then(|track| track.album_id) {
                Some(album) => library.release_of(album),
                None => Ok(None),
            }
        });

        match released {
            Ok(release) => {
                let release = release?;
                release
                    .mbid
                    .map(Cover::Release)
                    .or_else(|| release.group.map(Cover::Group))
            }
            Err(error) => {
                tracing::debug!(%error, "the release a cover is drawn from could not be read");
                None
            }
        }
    }
}

pub(crate) struct Presenter {
    #[cfg(feature = "discord")]
    discord: Discord,
}

impl Presenter {
    pub(crate) fn leave(self) {}

    pub(crate) fn follow(&self, presence: &Presence) {
        #[cfg(feature = "discord")]
        self.discord.follow(presence);
        #[cfg(not(feature = "discord"))]
        if presence.active() {
            tracing::warn!("this build carries no Discord presence, so nothing is shown there");
        }
    }
}

#[cfg(feature = "discord")]
pub(crate) fn start(
    player: &Arc<Player>,
    library: Option<&Arc<Library>>,
    config: &Config,
) -> Presenter {
    let releases = Catalogued {
        library: library.map(Arc::clone),
    };
    let presenter = Presenter {
        discord: Discord::new(Arc::clone(player), Arc::new(releases)),
    };
    presenter.follow(&config.presence());
    presenter
}

#[cfg(not(feature = "discord"))]
pub(crate) fn start(
    _player: &Arc<Player>,
    _library: Option<&Arc<Library>>,
    config: &Config,
) -> Presenter {
    let presenter = Presenter {};
    presenter.follow(&config.presence());
    presenter
}

#[cfg(feature = "ui")]
impl resonate_ui::Present for Presenter {
    fn follow(&self, presence: &Presence) {
        Self::follow(self, presence);
    }
}
