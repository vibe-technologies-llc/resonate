use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crossbeam_channel::{Receiver, Sender};
use resonate_codec::Sources;
use resonate_core::{FrameSpan, MediaLocation, SourceId};
use resonate_engine::{Command, Placement, Player, QueueItem, Unclaimed};
use resonate_library::Library;
use resonate_mpris::{Heard, Host, Mpris, Opened};

use crate::{names_a_sheet, playlists::Collection, sheet_items};

const IDENTITY: &str = "Resonate";
const DESKTOP_ENTRY: &str = "resonate";
const OPENS_IN_FRONT: bool = true;
const MIME_TYPES: [&str; 18] = [
    "audio/flac",
    "audio/x-flac",
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4b",
    "audio/aac",
    "audio/ogg",
    "audio/x-vorbis+ogg",
    "audio/opus",
    "audio/x-opus+ogg",
    "audio/x-wav",
    "audio/wav",
    "audio/x-matroska",
    "audio/x-aiff",
    "audio/aiff",
    "audio/x-caf",
    "audio/x-dsf",
    "audio/x-dff",
];

struct Attention {
    changes: Receiver<bool>,
    held: AtomicBool,
}

impl Attention {
    fn held(&self) -> bool {
        for held in self.changes.try_iter() {
            self.held.store(held, Ordering::Release);
        }
        self.held.load(Ordering::Acquire)
    }
}

struct Desktop {
    player: Arc<Player>,
    sources: Arc<Sources>,
    library: Option<Arc<Library>>,
    quit: Option<Sender<()>>,
    attention: Option<Attention>,
    raise: Option<Sender<()>>,
    notify: Arc<AtomicBool>,
}

pub(crate) struct Window {
    pub(crate) attention: Receiver<bool>,
    pub(crate) raise: Sender<()>,
}

impl Host for Desktop {
    fn identity(&self) -> String {
        IDENTITY.to_owned()
    }

    fn desktop_entry(&self) -> Option<String> {
        Some(DESKTOP_ENTRY.to_owned())
    }

    fn can_quit(&self) -> bool {
        self.quit.is_some()
    }

    fn can_raise(&self) -> bool {
        self.raise.is_some()
    }

    fn raise(&self) {
        let Some(raise) = self.raise.as_ref() else {
            return;
        };
        if raise.try_send(()).is_err() {
            tracing::debug!("a raise from the session bus arrived while one was already pending");
        }
    }

    fn attended(&self) -> bool {
        self.attention.as_ref().is_some_and(Attention::held)
    }

    fn notifies(&self) -> bool {
        self.notify.load(Ordering::Acquire)
    }

    fn quit(&self) {
        let Some(quit) = self.quit.as_ref() else {
            return;
        };
        if quit.try_send(()).is_err() {
            tracing::debug!("a quit from the session bus arrived while one was already pending");
        }
    }

    fn sources(&self) -> Vec<SourceId> {
        self.sources.names()
    }

    fn mime_types(&self) -> Vec<String> {
        MIME_TYPES.iter().map(|kind| (*kind).to_owned()).collect()
    }

    fn open(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Opened {
        let queue = self.player.queue();
        let at = Placement::Next.row(queue.len(), self.player.state().queue_position);
        let mut minting = Unclaimed::beside(&queue);

        let items = match location.as_path().filter(|_| names_a_sheet(location)) {
            Some(sheet) => sheet_items(&self.sources, sheet, &mut minting),
            None => vec![QueueItem {
                id: minting.mint(),
                location: location.clone(),
                span,
            }],
        };
        if items.is_empty() {
            return Opened::Refused;
        }
        let insert = Command::Insert {
            items,
            at: Placement::At(at),
            play: true,
        };

        match self.player.send(insert) {
            Ok(()) => Opened::Accepted,
            Err(error) => {
                tracing::warn!(%error, "a file opened from the session bus did not reach the engine");
                Opened::Refused
            }
        }
    }

    fn heard(&self, location: &MediaLocation, span: Option<FrameSpan>) -> Option<Heard> {
        let catalog = self.library.as_ref()?;
        let path = location.as_path()?;

        match catalog.track_at(path, span) {
            Ok(track) => track.map(|track| Heard {
                plays: track.plays,
                played: track.played,
            }),
            Err(error) => {
                tracing::debug!(%error, "a play count asked for on the session bus could not be read");
                None
            }
        }
    }
}

pub(crate) fn start(
    player: &Arc<Player>,
    sources: &Arc<Sources>,
    library: Option<&Arc<Library>>,
    quit: Option<Sender<()>>,
    window: Option<Window>,
    notify: &Arc<AtomicBool>,
) -> Option<Mpris> {
    let (attention, raise) = match window {
        Some(Window { attention, raise }) => (Some(attention), Some(raise)),
        None => (None, None),
    };
    let host = Desktop {
        player: Arc::clone(player),
        sources: Arc::clone(sources),
        library: library.map(Arc::clone),
        quit,
        attention: attention.map(|changes| Attention {
            changes,
            held: AtomicBool::new(OPENS_IN_FRONT),
        }),
        raise,
        notify: Arc::clone(notify),
    };
    let playlists = library.map(|library| {
        Arc::new(Collection::new(library, player)) as Arc<dyn resonate_mpris::Playlists>
    });

    match Mpris::start(Arc::clone(player), Arc::new(host), playlists) {
        Ok(mpris) => Some(mpris),
        Err(error) => {
            tracing::warn!(%error, "media keys and desktop controls are unavailable");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_mime_types() -> Vec<&'static str> {
        let entry = include_str!("../../../packaging/resonate.desktop");
        entry
            .lines()
            .find_map(|line| line.strip_prefix("MimeType="))
            .expect("the desktop entry declares its mime types")
            .split(';')
            .filter(|kind| !kind.is_empty())
            .collect()
    }

    fn metainfo_media_types() -> Vec<&'static str> {
        let metainfo = include_str!("../../../packaging/org.resonate.Resonate.metainfo.xml");
        metainfo
            .lines()
            .filter_map(|line| {
                line.trim()
                    .strip_prefix("<mediatype>")?
                    .strip_suffix("</mediatype>")
            })
            .collect()
    }

    #[test]
    fn the_metainfo_provides_the_media_types_the_bus_offers_and_no_others() {
        let provided = metainfo_media_types();

        for kind in MIME_TYPES {
            assert!(
                provided.contains(&kind),
                "{kind} is missing from the metainfo"
            );
        }
        for kind in provided {
            assert!(
                MIME_TYPES.contains(&kind),
                "{kind} is provided by the metainfo and not offered on the bus"
            );
        }
    }

    #[test]
    fn the_desktop_entry_and_the_bus_advertise_the_same_media_types() {
        let declared = entry_mime_types();

        for kind in MIME_TYPES {
            assert!(declared.contains(&kind), "{kind} is missing from the entry");
        }
        for kind in declared {
            assert!(
                MIME_TYPES.contains(&kind),
                "{kind} is claimed by the entry and not offered on the bus"
            );
        }
    }
}
