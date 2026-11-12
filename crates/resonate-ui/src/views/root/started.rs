use gpui::Context;
use resonate_core::Frames;
use resonate_engine::{Command, QueueItem, Unclaimed};
use resonate_library::Track;

use super::RootView;
use crate::{Continuing, Notice, ResonateApp, held, words};

impl RootView {
    pub(crate) fn play(&mut self, tracks: &[Track], start_at: usize, cx: &mut Context<Self>) {
        self.play_runs(&[tracks], start_at, cx);
    }

    pub(crate) fn play_rows(
        &mut self,
        tracks: &held::Rows<Track>,
        start_at: usize,
        cx: &mut Context<Self>,
    ) {
        let runs: Vec<&[Track]> = tracks.chunks().collect();
        self.play_runs(&runs, start_at, cx);
    }

    pub(super) fn play_runs(&mut self, runs: &[&[Track]], start_at: usize, cx: &mut Context<Self>) {
        let items: Vec<QueueItem> = runs
            .iter()
            .flat_map(|run| run.iter())
            .map(|track| QueueItem {
                id: track.id,
                location: track.location.clone(),
                span: track.span,
            })
            .collect();
        if items.is_empty() {
            return;
        }

        let started = runs.iter().flat_map(|run| run.iter()).nth(start_at);
        self.library.update(cx, |library, _| {
            library.set_playing_playlist(None);
            for run in runs {
                library.remember(run);
            }
            if let Some(track) = started {
                library.ask_about(track.album_id, track.artist_id);
            }
        });
        self.send(
            Command::Load {
                items,
                start_at,
                autoplay: true,
            },
            cx,
        );
        cx.notify();
    }

    pub(crate) fn continue_from(&mut self, device: &str, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            return;
        };
        let Continuing { cuts, at } = syncing.continue_from(device);
        if cuts.is_empty() {
            self.report(Notice::Trouble(words::root::nothing_continued(device)), cx);
            return;
        }
        let mut minting = Unclaimed::beside(&[]);
        let items = cuts
            .into_iter()
            .map(|cut| QueueItem {
                id: minting.mint(),
                location: cut.location,
                span: cut.span,
            })
            .collect();
        self.library
            .update(cx, |library, _| library.set_playing_playlist(None));
        self.send(
            Command::Load {
                items,
                start_at: 0,
                autoplay: true,
            },
            cx,
        );
        if at != Frames::default() {
            self.send(Command::Seek(at), cx);
        }
        cx.notify();
    }
}
