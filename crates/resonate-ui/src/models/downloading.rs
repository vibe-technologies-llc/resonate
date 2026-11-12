use std::time::SystemTime;

use gpui::Context;
use resonate_core::{AlbumId, TrackId};
use resonate_library::{Found, Mbid, PollOptions};

use super::LibraryModel;
use crate::downloads::{Download, Fetched, Fetcher, Fetching, Polling};

impl LibraryModel {
    pub fn downloads(&self) -> &[Download] {
        self.downloads.all()
    }

    pub fn fetching(&self, download: &Download) -> Fetching {
        download.fetching_while(&self.polling())
    }

    pub(crate) fn fetched(&self, download: &Download) -> Option<Fetched> {
        self.fetched_for(download.want()?)
    }

    pub(super) fn polling(&self) -> Polling {
        Polling {
            asking: self.asking_for(),
            unheard: self.providers_unheard,
            now: SystemTime::now(),
        }
    }

    pub fn fetching_found(&self, found: &Found) -> Option<Fetching> {
        self.downloads
            .of(&found.recording)
            .map(|download| self.fetching(download))
    }

    pub fn is_downloading(&self) -> bool {
        self.downloads
            .all()
            .iter()
            .any(|download| self.fetching(download).is_underway())
    }

    pub fn dismiss_download(&mut self, recording: &Mbid, cx: &mut Context<Self>) {
        self.downloads.dismiss(recording);
        cx.notify();
    }

    pub fn cancel_download(&mut self, recording: &Mbid, cx: &mut Context<Self>) {
        let Some(want) = self.downloads.of(recording).and_then(Download::want) else {
            return;
        };
        if self.asking_for().contains(&want) {
            self.stop_poll(cx);
            self.fetch_what_was_wanted(PollOptions::default(), cx);
        }
        self.downloads.dismiss(recording);
        self.unwant(want, cx);
        cx.notify();
    }

    pub fn sources_moved(&mut self, cx: &mut Context<Self>) {
        if self.fetched_by(cx) == Fetcher::Nobody {
            return;
        }
        if self.downloads.provided() {
            cx.notify();
        }
        self.fetch_what_was_wanted(PollOptions::ASKING_EVERY_WANT, cx);
    }

    pub fn downloaded_track(&self, download: &Download) -> Option<TrackId> {
        self.standings.get(&download.want()?)?.held()
    }

    pub fn downloaded_album(&self, download: &Download) -> Option<AlbumId> {
        self.standings.get(&download.want()?)?.album()
    }

    pub fn clear_finished_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads.clear_finished();
        cx.notify();
    }
}
