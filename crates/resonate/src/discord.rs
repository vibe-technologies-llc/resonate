use std::sync::Arc;

use resonate_core::Presence;
#[cfg(feature = "discord")]
use resonate_core::{FrameSpan, MediaLocation};
#[cfg(feature = "discord")]
use resonate_discord::{Cover, Discord, Releases};
use resonate_engine::Player;
use resonate_library::Library;
#[cfg(feature = "discord")]
use resonate_library::ReleaseDetail;

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
            Ok(release) => cover_of(release?),
            Err(error) => {
                tracing::debug!(%error, "the release a cover is drawn from could not be read");
                None
            }
        }
    }
}

#[cfg(feature = "discord")]
fn cover_of(release: ReleaseDetail) -> Option<Cover> {
    release
        .mbid
        .filter(|_| release.may_have_a_front)
        .map(Cover::Release)
        .or_else(|| release.group.map(Cover::Group))
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

#[cfg(all(test, feature = "discord"))]
mod tests {
    use resonate_library::{CoverSource, Mbid};

    use super::*;

    const RELEASE: &str = "0a7d6f2b-8c1e-4e5a-9b3f-2d6c8e1f4a7b";
    const GROUP: &str = "6b8e4d2c-1f3a-4c5e-8d7b-9a0f2e4c6b8d";

    fn released(may_have_a_front: bool) -> ReleaseDetail {
        ReleaseDetail {
            mbid: Some(Mbid::new(RELEASE).expect("an id")),
            group: Some(Mbid::new(GROUP).expect("an id")),
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: None,
            disambiguation: None,
            cover_source: CoverSource::Archive,
            may_have_a_front,
            asked: None,
            answered: None,
            links: Vec::new(),
            media: Vec::new(),
        }
    }

    #[test]
    fn a_release_with_no_front_cover_is_drawn_from_its_group_instead() {
        assert_eq!(
            cover_of(released(true)),
            Some(Cover::Release(Mbid::new(RELEASE).expect("an id")))
        );
        assert_eq!(
            cover_of(released(false)),
            Some(Cover::Group(Mbid::new(GROUP).expect("an id")))
        );
    }
}
