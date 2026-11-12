use std::{slice, time::SystemTime};

use gpui::Context;
use resonate_core::TrackId;
use resonate_library::Favoured;

use super::{Change, Edited, LibraryModel, Wanted, arranged::Named};

impl LibraryModel {
    pub fn favour(&mut self, what: Favoured, favourite: bool, cx: &mut Context<Self>) {
        self.favoured.insert(what, favourite);
        let mut named_before = None;
        if let Favoured::Track(id) = what
            && let Some(before) = self.named.get(&id).cloned()
            && before.track.is_some()
        {
            let mut named = before.clone();
            if let Some(track) = named.track.as_mut() {
                track.favourite = favourite.then(SystemTime::now);
            }
            self.named.insert(id, named);
            self.revision = self.revision.wrapping_add(1);
            named_before = Some((id, before));
        }
        cx.notify();

        self.edited_then(
            Wanted::TheSearch,
            Change::Favour { favourite },
            move |library| library.favour(what, favourite).map(|_| None),
            move |this, edited, _| match edited {
                Edited::Failed => this.unfavour_what_did_not_save(what, named_before),
                Edited::Landed => this.favours_written.push((what, favourite)),
            },
            cx,
        );
    }

    fn unfavour_what_did_not_save(
        &mut self,
        what: Favoured,
        named_before: Option<(TrackId, Named)>,
    ) {
        self.favoured.remove(&what);
        if let Some((id, before)) = named_before {
            self.named.insert(id, before);
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn renamed(&mut self, id: TrackId, cx: &mut Context<Self>) {
        match self.library.track(id) {
            Ok(Some(track)) => self.remember(slice::from_ref(&track)),
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "a renamed track could not be read back"),
        }
        self.read(Wanted::Everything, cx);
    }

    pub fn hide_track(&mut self, id: TrackId, hidden: bool, cx: &mut Context<Self>) {
        self.hide_tracks(vec![id], hidden, cx);
    }

    pub fn hide_tracks(&mut self, ids: Vec<TrackId>, hidden: bool, cx: &mut Context<Self>) {
        self.edited(
            Wanted::Everything,
            Change::Hide { hidden },
            move |library| {
                for id in ids {
                    library.hide_track(id, hidden)?;
                }
                Ok(None)
            },
            cx,
        );
    }
}
