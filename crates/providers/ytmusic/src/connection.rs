use std::io::ErrorKind;

use resonate_providers::Error;

use crate::{CHECKED_WITH, Tier, VideoId, YtMusic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Connection {
    Premium,
    SignedInWithoutPremium,
    NotSignedIn,
    ToolMissing,
}

fn the_tool_did_not_run(error: &Error) -> bool {
    matches!(error, Error::Io { source, .. } if source.kind() != ErrorKind::TimedOut)
}

impl YtMusic {
    pub fn check(&self) -> Connection {
        let Some(video) = VideoId::read(CHECKED_WITH) else {
            return Connection::NotSignedIn;
        };
        let best = match self.tool.formats(&video) {
            Ok(formats) => formats.best,
            Err(error) if the_tool_did_not_run(&error) => {
                tracing::warn!(%error, "yt-dlp could not be run");
                return Connection::ToolMissing;
            }
            Err(error) => {
                tracing::warn!(%error, "the account could not be checked");
                return Connection::NotSignedIn;
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
