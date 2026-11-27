mod explore;

use std::{ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, ElementId, Entity, FontWeight, ScrollHandle, SharedString,
    Stateful, Subscription, Task, UniformListScrollHandle, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::AlbumId;
use resonate_engine::Placement;
use resonate_library::{
    ExploreOffer, Library, MixEntry, PICTURED_BY_AT_MOST, Reason, SavedQuery, Search, Suggestion,
    SuggestionKind, Track, TrackQuery,
};

use crate::{
    Drawn, LibraryModel, Pane, Previewed, format,
    held::Rows,
    icons::Icon,
    theme, toast,
    views::{
        browser::{self, Chooses, Plays, Rowed, TRACK_CONTROLS},
        hint::Names,
        kit::{self, Casts as _, EndsInAnEllipsis, Press, Tone},
        listing,
        mosaic::{self, Mosaic},
        pointed::{self, LitUnderThePointer as _},
        reorder::{Listed, Shift},
        root::{RootView, empty, listed},
        scrollbar::Scrollbars,
        sorting,
        spoken::{self, Spoken as _},
    },
    words,
};

const SUGGESTION_CELL: &str = "suggestion-cell";

const SHELF_PADDING: f32 = 24.0;

pub(crate) struct SuggestionsPane {
    library: Entity<LibraryModel>,
    catalog: Arc<Library>,
    shelves: ScrollHandle,
    rows: UniformListScrollHandle,
    offered: Arc<[Suggestion]>,
    opened: Option<Previewed>,
    _previewing: Task<()>,
    explore: explore::ExplorePane,
    _landing: Subscription,
}

impl SuggestionsPane {
    pub(crate) fn new(
        library: &Entity<LibraryModel>,
        catalog: Arc<Library>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut pane = Self {
            library: library.clone(),
            catalog,
            shelves: ScrollHandle::new(),
            rows: UniformListScrollHandle::default(),
            offered: Arc::default(),
            opened: None,
            _previewing: Task::ready(()),
            explore: explore::ExplorePane::default(),
            _landing: cx.observe(library, |pane, library, cx| {
                pane.take_what_landed(&library, cx);
            }),
        };

        pane.take_what_landed(library, cx);

        pane
    }

    fn take_what_landed(&mut self, library: &Entity<LibraryModel>, cx: &mut Context<Self>) {
        let Some(offered) = library.update(cx, |library, _| library.take_the_offers()) else {
            return;
        };

        self.offered = offered;
        cx.notify();
    }

    pub(crate) fn offered(&self) -> Arc<[Suggestion]> {
        Arc::clone(&self.offered)
    }

    pub(crate) const fn opened(&self) -> Option<&Previewed> {
        self.opened.as_ref()
    }

    pub(crate) fn rows(&self) -> UniformListScrollHandle {
        self.rows.clone()
    }

    fn open(&mut self, query: Option<SavedQuery>, cx: &mut Context<Self>) {
        self.explore.open_playlist(None);
        self.explore.mix_changed();

        let Some(query) = query else {
            self.opened = None;
            self._previewing = Task::ready(());
            cx.notify();
            return;
        };

        if self.opened.as_ref().is_some_and(|held| held.query == query) {
            return;
        }

        let opening = Previewed::opening(query.clone());
        let asked = opening.asked();

        self.opened = Some(opening);
        cx.notify();
        self.read_the_preview(query, asked, cx);
    }

    fn preview_further(&mut self, drawn_to: usize, cx: &mut Context<Self>) {
        let Some((query, asked)) = self
            .opened
            .as_mut()
            .and_then(|previewed| previewed.further(drawn_to))
        else {
            return;
        };

        self.read_the_preview(query, asked, cx);
    }

    fn read_the_preview(&mut self, query: SavedQuery, asked_for: usize, cx: &mut Context<Self>) {
        let catalog = Arc::clone(&self.catalog);

        self._previewing = cx.spawn(async move |this, cx| {
            let asked = query.clone();
            let read = cx
                .background_executor()
                .spawn(async move {
                    catalog.tracks(&TrackQuery {
                        limit: Some(asked_for),
                        ..TrackQuery::from(&asked)
                    })
                })
                .await;

            let landed = this.update(cx, |this, cx| {
                let tracks = match read {
                    Ok(tracks) => tracks,
                    Err(error) => {
                        tracing::error!(%error, "a suggestion could not be previewed");
                        toast::tell(
                            toast::could_not(words::suggestions::preview_that_suggestion(), &error),
                            cx,
                        );
                        return;
                    }
                };
                let still_open = this.opened.as_ref().is_some_and(|held| held.query == query);

                this.library.update(cx, |library, cx| {
                    library.remember(&tracks);
                    if still_open {
                        cx.notify();
                    }
                });
                if let Some(previewed) = this.opened.as_mut().filter(|_| still_open) {
                    previewed.tracks = tracks.into();
                    cx.notify();
                }
            });
            let _ = landed;
        });
    }
}

impl RootView {
    pub(crate) fn suggestions_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let pane = self.suggestions.read(cx);
        let offered = Arc::clone(&pane.offered);
        let opened = pane.opened.clone();
        let scroll = pane.shelves.clone();
        let has_explore_playlist = pane.explore.opened_playlist().is_some();

        if self.explore_enabled(cx)
            && has_explore_playlist
            && let Some(playlist_view) = self.opened_explore_playlist(cx)
        {
            return playlist_view;
        }
        if let Some(opened) = opened
            && let Some(suggestion) = offered.iter().find(|held| held.query == opened.query)
        {
            let suggestion = suggestion.clone();

            return self.opened_suggestion(&suggestion, &opened.tracks, cx);
        }

        let nothing = offered.is_empty();
        let explore = self.explore_enabled(cx);

        let mut shelves = div()
            .id("suggestions")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_8()
            .p(px(SHELF_PADDING))
            .overflow_y_scroll()
            .track_scroll(&scroll);
        let shelved = in_shelf_order(&offered);
        let mut explore_placed = false;

        for kind in SuggestionKind::ALL {
            let cards: Vec<AnyElement> = shelved
                .iter()
                .enumerate()
                .filter_map(|(place, index)| Some((place, *index, offered.get(*index)?)))
                .filter(|(_, _, suggestion)| suggestion.reason.kind() == kind)
                .map(|(place, index, suggestion)| {
                    let reached = self.reaches(Shift::Listing(Listed::Offered), place, cx);
                    self.suggestion_card(index, suggestion, reached, cx)
                        .into_any_element()
                })
                .collect();

            if !cards.is_empty() {
                shelves = shelves.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_4()
                        .child(kit::group_title(kind.name()).flex_none())
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .content_start()
                                .gap(px(theme::grid_gap()))
                                .children(cards),
                        ),
                );
            }
            if explore && kind == SuggestionKind::ForYou {
                shelves = shelves.child(self.explore_playlists_shelf(cx));
                explore_placed = true;
            }
        }

        if explore && !explore_placed {
            shelves = shelves
                .when(nothing, |shelves| {
                    shelves.child(kit::figure(words::suggestions::scan_more()))
                })
                .child(self.explore_playlists_shelf(cx));
        } else if explore && nothing {
            shelves = shelves.child(kit::figure(words::suggestions::scan_more()));
        }
        kit::pane()
            .child(suggestions_heading(offered.len()))
            .when(nothing && !explore, |pane| {
                pane.child(empty(
                    Icon::Suggestions,
                    words::suggestions::nothing_offered(),
                    Some(words::suggestions::scan_more()),
                ))
            })
            .when(!nothing || explore, |pane| {
                pane.child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(kit::measures_the_grid(self.rooms.grid_width.clone()))
                        .child(Scrollbars::of(cx).around("suggestions-scrollbar", scroll, shelves)),
                )
            })
            .into_any_element()
    }

    fn suggestion_card(
        &mut self,
        index: usize,
        suggestion: &Suggestion,
        reached: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let opening = suggestion.query.clone();
        let playing = suggestion.query.clone();
        let side = self.grid_side();
        let art = self.suggestion_art(suggestion, side, Drawn::InAGrid, cx);
        let cell_id = ElementId::from(("suggestion-card", index));
        let shelves = self.suggestions.read(cx).shelves.clone();
        let pointed = pointed::is_pointed_at(&cell_id);

        div()
            .id(cell_id.clone())
            .debug_selector(move || format!("suggestion-open-{index}"))
            .follows_the_pointer(cell_id)
            .group(SUGGESTION_CELL)
            .listed_as(|| said_of(suggestion))
            .chosen(reached)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .cursor_pointer()
            .names(words::suggestions::open_hint())
            .child(
                div()
                    .relative()
                    .rounded(px(mosaic::ART_ROUNDING))
                    .group_hover(SUGGESTION_CELL, |frame| frame.casting(kit::Cast::Short))
                    .child(art)
                    .when(reached, |frame| {
                        frame
                            .child(browser::reached_ring())
                            .child(kit::brought_into_view(
                                shelves,
                                Rc::clone(&self.rooms.reached_unseen),
                            ))
                    })
                    .child(
                        div()
                            .absolute()
                            .top_1()
                            .right_1()
                            .flex()
                            .rounded_md()
                            .bg(kit::cover_veil())
                            .opacity(0.0)
                            .group_hover(SUGGESTION_CELL, |marks| marks.opacity(1.0))
                            .child(self.queue_mark(
                                ("suggestion-queue", index),
                                &suggestion.query,
                                cx,
                            ))
                            .child(self.save_mark(("suggestion-save", index), suggestion, cx)),
                    )
                    .child(
                        kit::play_on_the_art(
                            ("suggestion-play", index),
                            words::suggestions::play_hint(),
                        )
                        .absolute()
                        .bottom_2()
                        .right_2()
                        .opacity(0.0)
                        .group_hover(SUGGESTION_CELL, |mark| mark.opacity(1.0))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.with_the_rows_of(&playing, window, cx, |this, rows, _, cx| {
                                    this.plays_in_order(cx);
                                    this.play(&rows, 0, cx);
                                });
                            },
                        )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if pointed {
                                theme::accent()
                            } else {
                                theme::text()
                            }))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(suggestion.name.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(suggestion.reason.says())),
                    )
                    .child(kit::figure(measured(suggestion)).text_color(rgb(theme::faint()))),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_offered(opening.clone(), cx);
            }))
    }

    pub(crate) fn offered_in_shelf_order(&self, cx: &App) -> Vec<SavedQuery> {
        let offered = self.suggestions.read(cx).offered();

        in_shelf_order(&offered)
            .into_iter()
            .filter_map(|index| offered.get(index).map(|held| held.query.clone()))
            .collect()
    }

    pub(crate) fn open_offered(&mut self, query: SavedQuery, cx: &mut Context<Self>) {
        self.let_go_of_the_reach_in(Shift::Listing(Listed::Offered), cx);
        self.suggestions
            .update(cx, |pane, cx| pane.open(Some(query), cx));
        cx.notify();
    }

    pub(crate) fn cards_a_page(&self, cx: &App) -> usize {
        let seen = self.suggestions.read(cx).shelves.bounds().size;
        let down = f32::from(seen.height) / (self.grid_row() + theme::grid_gap());

        self.grid_columns() * (down.max(1.0) as usize)
    }

    fn opened_suggestion(
        &mut self,
        suggestion: &Suggestion,
        tracks: &Arc<[Track]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let playing = self.playing_now(cx).track;
        let art = self.suggestion_art(suggestion, kit::hero_side(), Drawn::OnThePage, cx);
        let reads = suggestion
            .query
            .text
            .as_deref()
            .map(|text| Search::read(text).reads())
            .unwrap_or_default();
        let searched = suggestion.query.text.clone().unwrap_or_default();
        let shown = tracks.len();
        let rows = Rows::from(Arc::clone(tracks));
        let scroll = self.suggestions.read(cx).rows();
        let explore_allowed = self.explore_enabled(cx);
        let mix_mode = explore_allowed && self.suggestions.read(cx).explore.mix_mode();
        let (mix_page, mix_offers) = if mix_mode {
            self.mix_explore_state(&suggestion.query, cx)
        } else {
            (explore::MixPage::Idle, Vec::new())
        };

        let mix_banner = if mix_mode {
            self.mix_explore_banner(&mix_page, mix_offers.len(), cx)
        } else {
            None
        };
        let woven: Option<Arc<[MixEntry<Track>]>> = if mix_mode && !mix_offers.is_empty() {
            Some(explore::weave_suggestion_tracks(tracks, mix_offers.clone()).into())
        } else {
            None
        };
        let list_len = woven.as_ref().map_or(shown, |entries| entries.len());

        let actions = kit::action_row()
            .flex_nowrap()
            .pt_2()
            .child(self.play_suggestion(
                "opened-suggestion-play",
                &suggestion.query,
                mix_mode.then(|| mix_offers.clone()),
                cx,
            ))
            .child(self.shuffle_suggestion(
                &suggestion.query,
                mix_mode.then(|| mix_offers.clone()),
                cx,
            ))
            .child(self.queue_suggestion(
                &suggestion.query,
                mix_mode.then(|| mix_offers.clone()),
                cx,
            ))
            .when(explore_allowed, |row| {
                row.child(self.mix_explore_button(mix_mode, cx))
            })
            .child(self.save_mark("opened-suggestion-save", suggestion, cx))
            .child(self.in_the_pane_ring(
                kit::icon_button(
                    "opened-suggestion-search",
                    Icon::Search,
                    words::suggestions::search_hint(),
                ),
                move |this, window, cx| {
                    this.search_instead(searched.clone(), window, cx);
                    this.choose_pane(Pane::Tracks, cx);
                },
                cx,
            ));

        let heading = kit::heading()
            .child(self.in_the_pane_ring(
                kit::way_back(
                    "suggestions-back",
                    words::suggestions::suggestions(),
                    words::suggestions::back_hint(),
                ),
                |this, _, cx| {
                    this.suggestions.update(cx, |pane, cx| pane.open(None, cx));
                    cx.notify();
                },
                cx,
            ))
            .child(
                kit::hero(self.rooms.hero_row.clone())
                    .child(kit::hero_art(art))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(kit::eyebrow(words::suggestions::suggestion_of(
                                suggestion.reason.kind().name(),
                            )))
                            .child(kit::title(SharedString::from(suggestion.name.clone())))
                            .child(kit::subtitle(SharedString::from(suggestion.reason.says())))
                            .child(kit::figure(measured_shown(suggestion, shown)))
                            .when(!reads.is_empty(), |column| {
                                column.child(div().pt_1().child(listing::reads(&reads)))
                            })
                            .child(actions),
                    ),
            );

        kit::pane()
            .child(heading)
            .children(mix_banner)
            .child(listing::columns(
                "#",
                true,
                TRACK_CONTROLS,
                sorting::unsorted(),
                &self.rooms.columns_fit,
                &self.controls,
                cx,
            ))
            .child(
                Scrollbars::of(cx).around(
                    "suggestion-tracks-scrollbar",
                    scroll.clone(),
                    uniform_list(
                        "suggestion-tracks",
                        list_len,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            this.suggestions.update(cx, |pane, cx| {
                                pane.preview_further(range.end, cx);
                            });

                            let mut drawn = Vec::new();

                            if let Some(entries) = &woven {
                                for index in range {
                                    let Some(entry) = entries.get(index) else {
                                        continue;
                                    };
                                    match entry {
                                        MixEntry::Local(track) => {
                                            let local_index = entries[..index]
                                                .iter()
                                                .filter(|item| matches!(item, MixEntry::Local(_)))
                                                .count();
                                            let shift = Shift::Listing(Listed::Suggested);

                                            let row = this.track_row(
                                                &rows,
                                                local_index,
                                                track,
                                                Rowed {
                                                    playing: playing == Some(track.id),
                                                    plays: Plays::TheseRows,
                                                    reach: this.at_the_reach(
                                                        shift,
                                                        local_index,
                                                        cx,
                                                    ),
                                                    chooses: Some(Chooses::among(
                                                        Listed::Suggested,
                                                        local_index,
                                                    )),
                                                },
                                                cx,
                                            );

                                            drawn.push(this.marked_where_reached(
                                                shift,
                                                local_index,
                                                row,
                                                cx,
                                            ));
                                        }
                                        MixEntry::Suggested(offer) => {
                                            drawn.push(
                                                this.woven_mix_row(entries, index, offer, cx),
                                            );
                                        }
                                    }
                                }
                                return drawn;
                            }
                            for index in range {
                                let Some(track) = rows.get(index) else {
                                    continue;
                                };
                                let shift = Shift::Listing(Listed::Suggested);

                                let row = this.track_row(
                                    &rows,
                                    index,
                                    track,
                                    Rowed {
                                        playing: playing == Some(track.id),
                                        plays: Plays::TheseRows,
                                        reach: this.at_the_reach(shift, index, cx),
                                        chooses: Some(Chooses::among(Listed::Suggested, index)),
                                    },
                                    cx,
                                );

                                drawn.push(this.marked_where_reached(shift, index, row, cx));
                            }
                            drawn
                        }),
                    )
                    .track_scroll(&scroll)
                    .h_full()
                    .w_full(),
                ),
            )
            .into_any_element()
    }

    fn suggestion_art(
        &mut self,
        suggestion: &Suggestion,
        side: f32,
        drawn_at: Drawn,
        cx: &mut Context<Self>,
    ) -> Div {
        let drawn: Vec<crate::models::Picture> = suggestion
            .pictured_by
            .iter()
            .take(PICTURED_BY_AT_MOST)
            .filter_map(|album| self.drawn_album(*album, drawn_at, cx))
            .collect();
        Mosaic {
            drawn: &drawn,
            mark: icon_of(suggestion.reason),
            name: &suggestion.name,
        }
        .drawn(side)
    }

    fn drawn_album(
        &mut self,
        album: AlbumId,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<crate::models::Picture> {
        self.library
            .update(cx, |library, cx| library.cover(album, drawn, cx))
    }

    fn save_mark(
        &self,
        id: impl Into<gpui::ElementId>,
        suggestion: &Suggestion,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let saved = self
            .library
            .read(cx)
            .saved_playlists()
            .iter()
            .any(|playlist| playlist.query.as_ref() == Some(&suggestion.query));

        if saved {
            return kit::mark_when(
                Press::Greyed,
                id,
                Icon::Check,
                words::suggestions::saved_hint(),
                "suggestion-card",
            );
        }

        let saving = suggestion.query.clone();
        let named = suggestion.name.clone();

        self.in_the_pane_ring(
            kit::icon_button(id, Icon::Plus, words::suggestions::save_hint()),
            move |this, _, cx| {
                cx.stop_propagation();

                let saved = saving.clone();
                let name = named.clone();

                this.library.update(cx, |library, cx| {
                    library.save_query(name, saved, cx);
                });
            },
            cx,
        )
    }

    fn queue_suggestion(
        &self,
        query: &SavedQuery,
        explore_offers: Option<Vec<ExploreOffer>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queueing = query.clone();

        self.in_the_pane_ring(
            kit::icon_button(
                "opened-suggestion-queue",
                Icon::QueueLast,
                words::suggestions::queue_hint(),
            ),
            move |this, window, cx| {
                let offers = explore_offers.clone();

                this.with_the_rows_of(&queueing, window, cx, move |this, rows, _, cx| {
                    if let Some(offers) = offers.filter(|offers| !offers.is_empty()) {
                        let woven = explore::weave_suggestion_tracks(&rows, offers);

                        this.queue_mixed(&woven, Placement::Queued, cx);
                    } else {
                        this.queue(&listed(&rows), Placement::Queued, cx);
                    }
                });
            },
            cx,
        )
    }

    fn queue_mark(
        &self,
        id: impl Into<gpui::ElementId>,
        query: &SavedQuery,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queueing = query.clone();

        self.in_the_pane_ring(
            kit::icon_button(id, Icon::QueueLast, words::suggestions::queue_hint()),
            move |this, window, cx| {
                cx.stop_propagation();
                this.with_the_rows_of(&queueing, window, cx, move |this, rows, _, cx| {
                    this.queue(&listed(&rows), Placement::Queued, cx);
                });
            },
            cx,
        )
    }

    fn play_suggestion(
        &self,
        id: impl Into<gpui::ElementId>,
        query: &SavedQuery,
        explore_offers: Option<Vec<ExploreOffer>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let playing = query.clone();

        self.in_the_pane_ring(
            kit::button(
                id,
                Some(Icon::Play),
                words::suggestions::play(),
                words::suggestions::play_hint(),
                Tone::Primary,
            ),
            move |this, window, cx| {
                let offers = explore_offers.clone();

                this.with_the_rows_of(&playing, window, cx, move |this, rows, _, cx| {
                    if let Some(offers) = offers.filter(|offers| !offers.is_empty()) {
                        let woven = explore::weave_suggestion_tracks(&rows, offers);

                        this.play_mixed(&woven, 0, false, cx);
                    } else {
                        this.plays_in_order(cx);
                        this.play(&rows, 0, cx);
                    }
                });
            },
            cx,
        )
    }

    fn shuffle_suggestion(
        &self,
        query: &SavedQuery,
        explore_offers: Option<Vec<ExploreOffer>>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let shuffling = query.clone();

        self.in_the_pane_ring(
            kit::button(
                "opened-suggestion-shuffle",
                Some(Icon::Shuffle),
                words::suggestions::shuffle(),
                words::suggestions::shuffle_hint(),
                Tone::Outlined,
            ),
            move |this, window, cx| {
                let offers = explore_offers.clone();

                this.with_the_rows_of(&shuffling, window, cx, move |this, rows, _, cx| {
                    if let Some(offers) = offers.filter(|offers| !offers.is_empty()) {
                        let woven = explore::weave_suggestion_tracks(&rows, offers);

                        this.play_mixed(&woven, 0, true, cx);
                    } else {
                        this.play_shuffled(&rows, cx);
                    }
                });
            },
            cx,
        )
    }
}

fn in_shelf_order(offered: &[Suggestion]) -> Vec<usize> {
    SuggestionKind::ALL
        .into_iter()
        .flat_map(|kind| {
            offered
                .iter()
                .enumerate()
                .filter(move |(_, suggestion)| suggestion.reason.kind() == kind)
                .map(|(index, _)| index)
        })
        .collect()
}

fn measured(suggestion: &Suggestion) -> String {
    let mut parts = vec![words::suggestions::tracks(suggestion.rows)];

    if let Some(length) = suggestion.length {
        parts.push(format::spanned(length));
    }

    parts.join(" · ")
}

fn said_of(suggestion: &Suggestion) -> SharedString {
    spoken::joined([
        suggestion.name.as_str(),
        suggestion.reason.says().as_str(),
        measured(suggestion).as_str(),
    ])
}

fn measured_shown(suggestion: &Suggestion, shown: usize) -> String {
    let whole = measured(suggestion);
    match (shown as u64) < suggestion.rows && shown > 0 {
        true => words::suggestions::FirstShown { shown, whole }.said(),
        false => whole,
    }
}

fn icon_of(reason: Reason) -> Icon {
    match reason {
        Reason::Decade(_)
        | Reason::GenreOfADecade(_)
        | Reason::BackCatalogue
        | Reason::OldFlames
        | Reason::SecondChances => Icon::Albums,
        Reason::Genre | Reason::UnheardGenre => Icon::Tracks,
        Reason::Artist
        | Reason::KindredArtists
        | Reason::DeepCuts
        | Reason::ArtistToDiscover
        | Reason::ArtistLeftBehind => Icon::Artists,
        Reason::NeverHeard | Reason::RecentlyAdded | Reason::FreshArrivals => Icon::Suggestions,
        Reason::MostPlayed => Icon::Statistics,
        Reason::OnRepeat => Icon::Repeat,
        Reason::ForgottenFavourites | Reason::StarredUnheard => Icon::Favourited,
        Reason::HiRes | Reason::Surround | Reason::Mono => Icon::Equaliser,
        Reason::LongPlayers | Reason::SlowBurners | Reason::ShortAndSharp => Icon::Disc,
    }
}

fn suggestions_heading(offered: usize) -> Div {
    kit::heading().child(
        kit::heading_row().child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(theme::heading_name()))
                .gap_1()
                .child(kit::title(words::suggestions::suggestions()))
                .when(offered > 0, |column| {
                    column.child(kit::subtitle(words::suggestions::lists(offered)))
                }),
        ),
    )
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use resonate_core::TrackId;
    use resonate_library::{Library, TrackQuery};

    use super::*;
    use crate::driven::{Driven, Folder};

    const SONGS: usize = 12;

    fn twelve_songs(folder: &Folder) -> (Arc<Library>, Vec<TrackId>) {
        for nth in 1..=SONGS {
            folder.tone(&format!("Track {nth}.wav"), 1);
        }

        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

        Driven::scanned(&library, folder);

        let ids = library
            .tracks(&TrackQuery::default())
            .expect("the scanned tracks")
            .iter()
            .map(|track| track.id)
            .collect();

        (library, ids)
    }

    fn suggestions_shown(cx: &mut TestAppContext, folder: &Folder) -> (Driven, Vec<TrackId>) {
        let (library, ids) = twelve_songs(folder);
        let mut driven = Driven::opened_in(cx, library, folder);

        driven.until(|root, cx| !root.offered_in_shelf_order(cx).is_empty());
        driven.click("tab-suggestions");
        driven.until_drawn("suggestion-open-0");

        (driven, ids)
    }

    fn songs_drawn(driven: &mut Driven, ids: &[TrackId]) -> usize {
        driven.settle();
        ids.iter()
            .filter(|id| {
                driven
                    .cx
                    .debug_bounds(Box::leak(format!("track-{}", id.get()).into_boxed_str()))
                    .is_some()
            })
            .count()
    }

    fn until_songs_are_drawn(driven: &mut Driven, ids: &[TrackId]) {
        let began = std::time::Instant::now();
        while songs_drawn(driven, ids) == 0 {
            assert!(
                began.elapsed() < std::time::Duration::from_secs(10),
                "the opened suggestion never drew a song"
            );
        }
    }

    fn reached_at(driven: &mut Driven) -> Option<(Shift, usize)> {
        driven.read(|root, cx| root.reached(cx).map(|reach| (reach.shift, reach.row)))
    }

    fn tabbed_onto(driven: &mut Driven, named: &'static str) -> bool {
        const TAB_STOPS_AT_MOST: usize = 64;

        let root = driven.root.clone();
        (0..TAB_STOPS_AT_MOST).any(|_| {
            driven.cx.simulate_keystrokes("tab");
            driven
                .cx
                .update(|window, cx| root.read(cx).controls.at(named, cx).is_focused(window))
        })
    }

    #[gpui::test]
    fn the_pane_shelves_each_offer_as_a_card_and_a_card_pressed_opens_onto_its_songs_and_back(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, ids) = suggestions_shown(cx, &folder);
        let offered = driven.read(|root, cx| root.offered_in_shelf_order(cx).len());

        assert_eq!(driven.read(|root, _| root.pane), Pane::Suggestions);
        assert!(driven.cx.debug_bounds("suggestions-back").is_none());
        assert!(
            driven
                .cx
                .debug_bounds(Box::leak(
                    format!("suggestion-open-{}", offered - 1).into_boxed_str()
                ))
                .is_some(),
            "every offer draws a card"
        );
        assert_eq!(songs_drawn(&mut driven, &ids), 0);

        driven.click("suggestion-open-0");
        driven.until_drawn("suggestions-back");
        until_songs_are_drawn(&mut driven, &ids);

        assert!(driven.cx.debug_bounds("suggestion-open-0").is_none());
        assert!(driven.cx.debug_bounds("opened-suggestion-play").is_some());

        driven.click("suggestions-back");
        driven.until_drawn("suggestion-open-0");

        assert!(driven.cx.debug_bounds("suggestions-back").is_none());
        assert_eq!(songs_drawn(&mut driven, &ids), 0);
    }

    #[gpui::test]
    fn the_arrows_reach_a_card_and_enter_opens_it_then_reach_its_songs_and_enter_plays_one(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, ids) = suggestions_shown(cx, &folder);

        driven.cx.simulate_keystrokes("down");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::Offered), 0))
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until_drawn("suggestions-back");
        until_songs_are_drawn(&mut driven, &ids);
        driven.cx.simulate_keystrokes("down");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::Suggested), 0))
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| !root.player.read(cx).queue().is_empty());

        assert!(driven.queue().iter().all(|queued| ids.contains(queued)));
    }

    fn focused_on(driven: &mut Driven, named: &'static str) -> bool {
        let root = driven.root.clone();
        driven
            .cx
            .update(|window, cx| root.read(cx).controls.at(named, cx).is_focused(window))
    }

    #[gpui::test]
    fn tab_walks_the_opened_suggestions_way_back_then_its_play_and_enter_presses_each(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, ids) = suggestions_shown(cx, &folder);

        driven.click("suggestion-open-0");
        driven.until_drawn("suggestions-back");
        until_songs_are_drawn(&mut driven, &ids);

        assert!(
            tabbed_onto(&mut driven, "suggestions-back"),
            "tab never reached the way back"
        );

        driven.cx.simulate_keystrokes("tab");

        assert!(focused_on(&mut driven, "opened-suggestion-play"));

        driven.cx.simulate_keystrokes("shift-tab");

        assert!(focused_on(&mut driven, "suggestions-back"));

        driven.cx.simulate_keystrokes("enter");
        driven.until_drawn("suggestion-open-0");

        assert!(driven.cx.debug_bounds("suggestions-back").is_none());

        driven.click("suggestion-open-0");
        driven.until_drawn("suggestions-back");
        until_songs_are_drawn(&mut driven, &ids);

        assert!(
            tabbed_onto(&mut driven, "opened-suggestion-play"),
            "tab never reached the suggestion's Play"
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| !root.player.read(cx).queue().is_empty());

        assert!(driven.queue().iter().all(|queued| ids.contains(queued)));
    }

    fn offered(name: &str, reason: Reason) -> Suggestion {
        Suggestion {
            name: name.to_owned(),
            reason,
            query: SavedQuery::default(),
            rows: 1,
            length: None,
            pictured_by: Vec::new(),
        }
    }

    #[test]
    fn a_card_is_reached_in_the_order_the_shelves_draw_it() {
        let offered = [
            offered("Rock", Reason::Genre),
            offered("Never heard", Reason::NeverHeard),
            offered("The 1970s", Reason::Decade(1970)),
            offered("Most played", Reason::MostPlayed),
            offered("Hi-res", Reason::HiRes),
        ];

        let reached: Vec<&str> = in_shelf_order(&offered)
            .into_iter()
            .map(|index| offered[index].name.as_str())
            .collect();

        assert_eq!(
            reached,
            ["Never heard", "Most played", "The 1970s", "Rock", "Hi-res"]
        );
    }

    #[test]
    fn a_card_is_heard_as_its_name_its_reason_and_what_it_would_hold() {
        let rock = Suggestion {
            rows: 12,
            length: Some(std::time::Duration::from_secs(3_600)),
            ..offered("Rock", Reason::Genre)
        };

        let said = said_of(&rock);

        assert!(said.starts_with("Rock, "));
        assert!(said.contains(rock.reason.says().as_str()));
        assert!(said.ends_with(&measured(&rock)));
        assert!(said.contains("12 tracks"));
    }

    #[test]
    fn a_shuffle_starts_somewhere_inside_the_list() {
        for rows in [1, 2, 7, 500] {
            assert!(crate::views::root::somewhere_in(rows) < rows);
        }

        assert_eq!(crate::views::root::somewhere_in(0), 0);
    }
}
