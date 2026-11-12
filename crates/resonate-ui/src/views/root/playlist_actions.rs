use gpui::{App, Context, Pixels, Point, Window};
use resonate_core::PlaylistId;
use resonate_engine::{Command, Placement, QueueRows, stamp_of};
use resonate_library::{
    Direction, Kept, Playing, Playlist, PlaylistEntry, RowOrder, SavedQuery, SortOrder, Track,
};

use super::{Pane, RootView, queued, somewhere_in};
use crate::{
    Notice,
    toast::{self},
    views::{
        playlists::{self, Held, Naming},
        sorting::Ordered,
    },
    words,
};

impl RootView {
    pub(crate) fn with_playlist_entries(
        &mut self,
        playlist: PlaylistId,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Vec<PlaylistEntry>, &mut Context<Self>) + 'static,
    ) {
        if let Some(held) = self.library.read(cx).held_entries_of(playlist) {
            then(self, held, cx);
            return;
        }
        let library = self.library.read(cx).catalog();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { library.playlist_entries(playlist, None) })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(entries) => then(this, entries, cx),
                Err(error) => {
                    tracing::error!(%error, playlist = playlist.get(), "a playlist could not be read");
                    toast::tell(toast::could_not(words::root::read_that_playlist(), &error), cx);
                }
            });
        })
        .detach();
    }

    pub(crate) fn play_the_playlist(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.plays_in_order(cx);
            this.play_playlist(playlist, &entries, 0, true, cx);
        });
    }

    pub(crate) fn shuffle_the_playlist(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.play_playlist(playlist, &entries, somewhere_in(entries.len()), true, cx);
            this.send(Command::SetShuffle(true), cx);
        });
    }

    pub(crate) fn queue_the_playlist(
        &mut self,
        playlist: PlaylistId,
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        self.with_playlist_entries(playlist, cx, move |this, entries, cx| {
            this.queue(&entries, at, cx);
        });
    }

    pub(crate) fn queue_the_playlists(
        &mut self,
        playlists: Vec<PlaylistId>,
        at: Placement,
        now: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(first) = playlists.first().copied() else {
            return;
        };
        let library = self.library.read(cx).catalog();

        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    let mut entries = Vec::new();
                    for playlist in playlists {
                        entries.extend(library.playlist_entries(playlist, None)?);
                    }
                    Ok::<_, resonate_library::Error>(entries)
                })
                .await;
            let _ = this.update(cx, |this, cx| match read {
                Ok(entries) if now => {
                    this.plays_in_order(cx);
                    this.play_playlist(first, &entries, 0, false, cx);
                }
                Ok(entries) => this.queue(&entries, at, cx),
                Err(error) => {
                    tracing::error!(%error, "playlists could not be read");
                    toast::tell(
                        toast::could_not(words::root::read_those_playlists(), &error),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    pub(crate) fn play_playlist(
        &mut self,
        playlist: PlaylistId,
        entries: &[PlaylistEntry],
        start_at: usize,
        whole: bool,
        cx: &mut Context<Self>,
    ) {
        let items = playlists::queue_items(entries, &QueueRows::default());
        if items.is_empty() {
            return;
        }
        let scanned: Vec<Track> = entries
            .iter()
            .filter_map(|entry| entry.track.clone())
            .collect();

        let queue = stamp_of(&items);
        self.library.update(cx, |library, _| {
            library.remember(&scanned);
            library.set_playing_playlist(whole.then_some(Playing { playlist, queue }));
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

    pub(crate) fn queue(
        &mut self,
        entries: &[PlaylistEntry],
        at: Placement,
        cx: &mut Context<Self>,
    ) {
        let items = playlists::queue_items(entries, &self.player.read(cx).queue());
        let rows = items.len();
        if rows == 0 {
            return;
        }
        let scanned: Vec<Track> = entries
            .iter()
            .filter_map(|entry| entry.track.clone())
            .collect();

        let play = self.player.read(cx).state().current.is_none();

        self.library
            .update(cx, |library, _| library.remember(&scanned));
        self.send(Command::Insert { items, at, play }, cx);
        self.report(Notice::Done(queued(rows, at, play)), cx);
        cx.notify();
    }

    pub(crate) fn hold_for_a_playlist(
        &mut self,
        holding: Held,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if holding.rows.is_empty() {
            return;
        }
        self.naming = None;
        self.ordering = None;
        self.adding = Some(holding);
        self.name.update(cx, |name, cx| {
            name.clear(cx);
            name.take_focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn name_a_playlist(
        &mut self,
        naming: Naming,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.adding = None;
        self.ordering = None;
        self.naming = Some(naming);

        let found = naming
            .playlist()
            .and_then(|playlist| self.saved(playlist, cx));
        let query = found
            .as_ref()
            .and_then(|found| found.query.clone())
            .unwrap_or_default();
        self.query_sort = query.sort;
        self.query_reading = query.reading;
        self.query_cap = query.limit;

        let held = found.map_or_else(String::new, |found| found.name);
        self.name.update(cx, |name, cx| {
            name.set_text(held, cx);
            name.take_focus(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn revise_search(
        &mut self,
        playlist: PlaylistId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = self
            .saved(playlist, cx)
            .and_then(|found| found.query)
            .and_then(|query| query.text)
            .unwrap_or_default();

        self.show_everything(cx);
        self.search
            .update(cx, |search, cx| search.set_text(text, cx));
        self.set_pane(Pane::Tracks, cx);
        self.name_a_playlist(Naming::Query(Some(playlist)), window, cx);
    }

    pub(crate) fn sort_a_playlist(
        &mut self,
        playlist: PlaylistId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.naming = None;
        self.adding = None;

        self.read_the_kept_order(Some(playlist), cx);
        self.order_a_listing(Ordered::Rows(playlist), at, cx);
    }

    pub(super) fn read_the_kept_order(&mut self, playlist: Option<PlaylistId>, cx: &App) {
        let kept = playlist
            .and_then(|playlist| self.saved(playlist, cx))
            .and_then(|found| found.kept);
        self.keeping = kept.is_some();
        let Kept { order, reading } = kept.unwrap_or(Kept {
            order: RowOrder::default(),
            reading: Direction::default(),
        });
        self.row_order = order;
        self.row_reading = reading;
    }

    pub(crate) fn put_in_order(
        &mut self,
        playlist: PlaylistId,
        order: RowOrder,
        reading: Direction,
        cx: &mut Context<Self>,
    ) {
        self.row_order = order;
        self.row_reading = reading;
        let kept = self.keeping.then_some(Kept { order, reading });

        self.library.update(cx, |library, cx| match kept {
            Some(kept) => library.keep_playlist_in_order(playlist, Some(kept), cx),
            None => library.sort_playlist(playlist, order, reading, cx),
        });
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn keep_in_order(
        &mut self,
        playlist: PlaylistId,
        keeping: bool,
        cx: &mut Context<Self>,
    ) {
        if self.keeping == keeping {
            return;
        }
        self.keeping = keeping;
        let kept = keeping.then_some(Kept {
            order: self.row_order,
            reading: self.row_reading,
        });

        self.library.update(cx, |library, cx| {
            library.keep_playlist_in_order(playlist, kept, cx);
        });
        cx.notify();
    }

    pub(crate) fn undo_edit(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        if self.pane == Pane::Queue && self.put_the_queue_back(cx) {
            return;
        }
        self.library.update(cx, |library, cx| library.undo(cx));
    }

    pub(crate) fn redo_edit(&mut self, cx: &mut Context<Self>) {
        if self.something_stands_over_the_pane() {
            return;
        }
        if self.pane == Pane::Queue && self.take_the_queue_out_again(cx) {
            return;
        }
        self.library.update(cx, |library, cx| library.redo(cx));
    }

    pub(crate) fn sort_a_search(&mut self, sort: SortOrder, cx: &mut Context<Self>) {
        self.query_sort = sort;
        self.query_reading = sort.reads();
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn read_a_search(&mut self, reading: Direction, cx: &mut Context<Self>) {
        self.query_reading = reading;
        self.let_go_of_a_reach_the_rows_left(cx);
        cx.notify();
    }

    pub(crate) fn cap_a_search(&mut self, cap: Option<usize>, cx: &mut Context<Self>) {
        self.query_cap = cap;
        cx.notify();
    }

    pub(super) fn saved(&self, playlist: PlaylistId, cx: &App) -> Option<Playlist> {
        self.library.read(cx).saved_playlist(playlist).cloned()
    }

    pub(crate) fn stop_naming(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.naming = None;
        self.adding = None;
        self.name.update(cx, |name, cx| name.clear(cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn name_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.name.read(cx).text().trim().to_owned();
        if given.is_empty() {
            return;
        }

        match (self.naming, self.adding.take()) {
            (_, Some(holding)) => self.library.update(cx, |library, cx| {
                library.create_playlist(given, holding.rows.to_vec(), cx);
            }),
            (Some(Naming::New), None) => self.library.update(cx, |library, cx| {
                library.create_playlist(given, Vec::new(), cx);
            }),
            (Some(Naming::Query(saved)), None) => {
                let library = self.library.read(cx);
                let text = library
                    .meant()
                    .map_or(library.query(), |meant| meant.searched.as_str())
                    .trim()
                    .to_owned();
                let query = SavedQuery {
                    text: (!text.is_empty()).then_some(text),
                    sort: self.query_sort,
                    reading: self.query_reading,
                    limit: self.query_cap,
                };
                self.library.update(cx, |library, cx| match saved {
                    Some(playlist) => library.revise_query(playlist, given, query, cx),
                    None => library.save_query(given, query, cx),
                });
                if saved.is_some() {
                    self.set_pane(Pane::Playlists, cx);
                }
            }
            (Some(Naming::Rename(playlist)), None) => self.library.update(cx, |library, cx| {
                library.rename_playlist(playlist, given, cx);
            }),
            (None, None) => return,
        }
        self.stop_naming(window, cx);
    }
}
