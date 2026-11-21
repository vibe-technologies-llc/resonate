use gpui::Context;
use resonate_core::Frames;
use resonate_engine::{Command, Placement, QueueItem, Unclaimed};
use resonate_library::{ExploreOffer, MixEntry, Track, weave_mix};

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

    pub(crate) fn play_mixed(
        &mut self,
        entries: &[MixEntry<Track>],
        start_at: usize,
        shuffle: bool,
        cx: &mut Context<Self>,
    ) {
        if entries.is_empty() {
            return;
        }

        let arranged: Vec<MixEntry<Track>> = if shuffle {
            let mut local = Vec::new();
            let mut suggested = Vec::new();

            for entry in entries {
                match entry {
                    MixEntry::Local(track) => local.push(track.clone()),
                    MixEntry::Suggested(offer) => suggested.push((**offer).clone()),
                }
            }
            shuffle_slice(&mut local);
            shuffle_slice(&mut suggested);
            weave_mix(local, |track| track.artist.as_deref(), suggested)
        } else {
            entries.to_vec()
        };
        let (items, local, offers) = mixed_items(&arranged, &[]);

        if items.is_empty() {
            return;
        }

        let start_at = if shuffle {
            0
        } else {
            start_at.min(items.len().saturating_sub(1))
        };
        let started_local = arranged.get(start_at).and_then(|entry| match entry {
            MixEntry::Local(track) => Some(track),
            MixEntry::Suggested(_) => None,
        });

        self.server
            .update(cx, |server, _| server.remember_explored(&offers));
        self.library.update(cx, |library, _| {
            library.set_playing_playlist(None);
            library.remember(&local);
            if let Some(track) = started_local {
                library.ask_about(track.album_id, track.artist_id);
            }
        });
        self.plays_in_order(cx);
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

    pub(crate) fn queue_mixed(
        &mut self,
        entries: &[MixEntry<Track>],
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        let beside: Vec<QueueItem> = self
            .player_acted_on()
            .read(cx)
            .queue()
            .iter()
            .map(|item| (**item).clone())
            .collect();
        let (items, local, offers) = mixed_items(entries, &beside);
        let rows = items.len();

        if rows == 0 {
            return;
        }

        let play = self.player_acted_on().read(cx).state().current.is_none();

        self.server
            .update(cx, |server, _| server.remember_explored(&offers));
        self.library
            .update(cx, |library, _| library.remember(&local));
        self.send(Command::Insert { items, at, play }, cx);

        let label = match (play, at) {
            (true, _) => words::root::queued_and_playing(rows),
            (false, Placement::Next) => words::root::to_play_next(rows),
            (false, Placement::Queued | Placement::At(_)) => words::root::added_to_the_queue(rows),
        };

        self.report(Notice::Done(label), cx);
        cx.notify();
    }

    pub(crate) fn play_explored_playlist(
        &mut self,
        offers: &[ExploreOffer],
        start_at: usize,
        shuffle: bool,
        cx: &mut Context<Self>,
    ) {
        let mut offers = offers.to_vec();

        if offers.is_empty() {
            return;
        }
        if shuffle {
            shuffle_slice(&mut offers);
        }
        self.server
            .update(cx, |server, _| server.remember_explored(&offers));

        let locations = offers.into_iter().map(|offer| offer.location).collect();
        let start = if shuffle { 0 } else { start_at };

        self.play_served(locations, start, cx);
    }

    pub(crate) fn queue_explored_playlist(
        &mut self,
        offers: &[ExploreOffer],
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        if offers.is_empty() {
            return;
        }
        self.server
            .update(cx, |server, _| server.remember_explored(offers));

        let locations = offers.iter().map(|offer| offer.location.clone()).collect();

        self.queue_served(locations, at, cx);
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

fn mixed_items(
    entries: &[MixEntry<Track>],
    beside: &[QueueItem],
) -> (Vec<QueueItem>, Vec<Track>, Vec<ExploreOffer>) {
    let mut local = Vec::new();
    let mut offers = Vec::new();
    let mut existing: Vec<QueueItem> = beside.to_vec();

    for entry in entries {
        if let MixEntry::Local(track) = entry {
            existing.push(QueueItem {
                id: track.id,
                location: track.location.clone(),
                span: track.span,
            });
            local.push(track.clone());
        }
    }

    let mut minting = Unclaimed::beside(&existing);
    let mut items = Vec::with_capacity(entries.len());

    for entry in entries {
        match entry {
            MixEntry::Local(track) => items.push(QueueItem {
                id: track.id,
                location: track.location.clone(),
                span: track.span,
            }),
            MixEntry::Suggested(offer) => {
                offers.push((**offer).clone());
                items.push(QueueItem::whole(minting.mint(), offer.location.clone()));
            }
        }
    }
    (items, local, offers)
}

fn shuffle_slice<T>(items: &mut [T]) {
    if items.len() < 2 {
        return;
    }

    let mut state = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0x9e37_79b9_7f4a_7c15_u64, |since| since.as_nanos() as u64)
        | 1;
    for index in (1..items.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;

        let swap = (state as usize) % (index + 1);

        items.swap(index, swap);
    }
}
