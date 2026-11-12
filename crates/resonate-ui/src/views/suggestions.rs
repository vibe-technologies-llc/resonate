use std::{ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, ElementId, FontWeight, SharedString, Stateful, div, prelude::*,
    px, rgb, uniform_list,
};
use resonate_core::AlbumId;
use resonate_engine::Placement;
use resonate_library::{
    PICTURED_BY_AT_MOST, Reason, SavedQuery, Search, Suggestion, SuggestionKind, Track,
};

use crate::{
    Drawn, Pane, format,
    held::Rows,
    icons::Icon,
    theme,
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

impl RootView {
    pub(crate) fn suggestions_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let offered = library.suggestions();
        let opened = library.opened_suggestion().cloned();
        if let Some(opened) = opened
            && let Some(suggestion) = offered.iter().find(|held| held.query == opened.query)
        {
            let suggestion = suggestion.clone();
            return self.opened_suggestion(&suggestion, &opened.tracks, cx);
        }

        let nothing = offered.is_empty();

        let mut shelves = div()
            .id("suggestions")
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .gap_8()
            .p(px(SHELF_PADDING))
            .overflow_y_scroll()
            .track_scroll(&self.scrolls.suggestions);
        let shelved = in_shelf_order(&offered);
        for kind in SuggestionKind::ALL {
            let cards: Vec<AnyElement> = shelved
                .iter()
                .enumerate()
                .filter_map(|(place, index)| Some((place, *index, offered.get(*index)?)))
                .filter(|(_, _, suggestion)| suggestion.reason.kind() == kind)
                .map(|(place, index, suggestion)| {
                    let reached = self.reaches(Shift::Listing(Listed::Offered), place);
                    self.suggestion_card(index, suggestion, reached, cx)
                        .into_any_element()
                })
                .collect();
            if cards.is_empty() {
                continue;
            }
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

        kit::pane()
            .child(suggestions_heading(offered.len()))
            .when(nothing, |pane| {
                pane.child(empty(
                    Icon::Suggestions,
                    words::suggestions::nothing_offered(),
                    Some(words::suggestions::scan_more()),
                ))
            })
            .when(!nothing, |pane| {
                pane.child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .min_h(px(0.0))
                        .child(kit::measures_the_grid(self.rooms.grid_width.clone()))
                        .child(Scrollbars::of(cx).around(
                            "suggestions-scrollbar",
                            self.scrolls.suggestions.clone(),
                            shelves,
                        )),
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
                                self.scrolls.suggestions.clone(),
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
        let offered = self.library.read(cx).suggestions();

        in_shelf_order(&offered)
            .into_iter()
            .filter_map(|index| offered.get(index).map(|held| held.query.clone()))
            .collect()
    }

    pub(crate) fn open_offered(&mut self, query: SavedQuery, cx: &mut Context<Self>) {
        self.let_go_of_the_reach_in(Shift::Listing(Listed::Offered));
        self.library
            .update(cx, |library, cx| library.open_suggestion(Some(query), cx));
    }

    pub(crate) fn cards_a_page(&self) -> usize {
        let seen = self.scrolls.suggestions.bounds().size;
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
        let art = self.suggestion_art(suggestion, self.hero_side(), Drawn::OnThePage, cx);
        let reads = suggestion
            .query
            .text
            .as_deref()
            .map(|text| Search::read(text).reads())
            .unwrap_or_default();
        let searched = suggestion.query.text.clone().unwrap_or_default();
        let shown = tracks.len();
        let rows = Rows::from(Arc::clone(tracks));

        let actions = kit::action_row()
            .flex_nowrap()
            .pt_2()
            .child(self.play_suggestion("opened-suggestion-play", &suggestion.query, cx))
            .child(self.shuffle_suggestion(&suggestion.query, cx))
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
                    this.library
                        .update(cx, |library, cx| library.open_suggestion(None, cx));
                },
                cx,
            ))
            .child(
                kit::hero().child(art).child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::measures_its_height(self.rooms.hero_height.clone()))
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
                    self.scrolls.suggestion_rows.clone(),
                    uniform_list(
                        "suggestion-tracks",
                        shown,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            this.library.update(cx, |library, cx| {
                                library.preview_further(range.end, cx);
                            });
                            let mut drawn = Vec::new();
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
                                        reach: this.at_the_reach(shift, index),
                                        chooses: Some(Chooses::among(Listed::Suggested, index)),
                                    },
                                    cx,
                                );
                                drawn.push(this.marked_where_reached(shift, index, row));
                            }
                            drawn
                        }),
                    )
                    .track_scroll(&self.scrolls.suggestion_rows)
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
                this.with_the_rows_of(&playing, window, cx, |this, rows, _, cx| {
                    this.plays_in_order(cx);
                    this.play(&rows, 0, cx);
                });
            },
            cx,
        )
    }

    fn shuffle_suggestion(&self, query: &SavedQuery, cx: &mut Context<Self>) -> Stateful<Div> {
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
                this.with_the_rows_of(&shuffling, window, cx, |this, rows, _, cx| {
                    this.play_shuffled(&rows, cx);
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
        true => words::suggestions::first_shown(shown, whole),
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
    use super::*;

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
