use resonate_providers::{Error, WhyAway};

use crate::{CHECKED_WITH, Tier, VideoId, YtMusic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Premium,
    SignedInWithoutPremium,
    NotSignedIn,
    ToolMissing,
    CheckSongGone,
    CouldNotCheck,
}

impl YtMusic {
    pub fn check(&self) -> Connection {
        let Some(video) = VideoId::read(CHECKED_WITH) else {
            return Connection::CouldNotCheck;
        };
        let best = match self.tool.formats_checked(&video) {
            Ok(Some(formats)) => formats.best,
            Ok(None) => {
                tracing::warn!(video = CHECKED_WITH, "the song a check uses is gone");
                return Connection::CheckSongGone;
            }
            Err(error) if error.why_away() == WhyAway::ToolMissing => {
                tracing::warn!(%error, "yt-dlp could not be run");
                return Connection::ToolMissing;
            }
            Err(error @ Error::Unwelcome { .. }) => {
                tracing::warn!(%error, "the account was turned away");
                return Connection::NotSignedIn;
            }
            Err(error) => {
                tracing::warn!(%error, "the account could not be checked");
                return Connection::CouldNotCheck;
            }
        };
        if !self.tool.holds_cookies() {
            return Connection::NotSignedIn;
        }
        if best.is_some_and(Tier::is_premium) {
            Connection::Premium
        } else {
            Connection::SignedInWithoutPremium
        }
    }
}
