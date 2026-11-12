use std::sync::{Arc, atomic::AtomicBool};

use crossbeam_channel::Receiver;
use resonate_apple::{Button, Notice, Notices};
use resonate_core::MediaLocation;

use crate::{
    Result,
    desktop::Errand,
    interfaces::Shared,
    notify::{Shown, actions, pressed},
};

const TRACK: &str = "track";
const LISTEN: &str = "listen";

pub(crate) struct Desk;

impl Desk {
    pub(crate) const fn open() -> Result<Self> {
        Ok(Self)
    }

    pub(crate) fn run(
        self,
        shared: &Arc<Shared>,
        errands: &Receiver<Errand>,
        _wanted: &AtomicBool,
    ) {
        let answering = Arc::clone(shared);
        let opened = Notices::open(&buttons(), move |key| {
            if let Some(command) = pressed(key) {
                let _ = answering.settle(command);
            }
        });
        let notices = match opened {
            Ok(notices) => notices,
            Err(error) => {
                tracing::debug!(%error, "a track change will not be told to macOS");
                return;
            }
        };

        for errand in errands {
            match errand {
                Errand::Show(shown) => raise(&notices, TRACK, &shown, true),
                Errand::Tell(shown) => raise(&notices, LISTEN, &shown, false),
                Errand::Grip => {}
            }
        }

        notices.hush(TRACK);
    }
}

fn buttons() -> Vec<Button<'static>> {
    actions()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[key, label]| Button { key, label })
        .collect()
}

fn raise(notices: &Notices, kind: &str, shown: &Shown, offers_buttons: bool) {
    let cover = shown.art.as_deref().and_then(MediaLocation::from_uri);

    notices.raise(
        kind,
        &Notice {
            title: &shown.summary,
            body: &shown.body,
            picture: cover.as_ref().and_then(MediaLocation::as_path),
            offers_buttons,
        },
    );
}
