use std::{ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, SharedString, Stateful, div, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{Accent, AlbumId};
use resonate_engine::Placement;
use resonate_library::{
    PICTURED_BY_AT_MOST, Reason, SavedQuery, Search, Suggestion, SuggestionKind, Track,
};

use crate::{
    Drawn, Pane, format,
    icons::Icon,
    theme,
    views::{
        browser::{Plays, TRACK_CONTROLS},
        hint::Names,
        kit::{self, EndsInAnEllipsis, Press, Tone},
        listing,
        mosaic::{self, Framed, Mosaic},
        reorder::{self, Listed, Shift},
        root::{RootView, empty, listed},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_OFFERED: &str = "The catalog has nothing to suggest yet.";

const SCAN_MORE: &str = "Scan more music and lists it can fill itself will appear here.";

const PLAY_HINT: &str = "Play what this list would hold, in order";

const SHUFFLE_HINT: &str = "Play what this list would hold, shuffled";

const QUEUE_HINT: &str = "Queue what this list would hold after what is already queued, ahead of the rest of what is playing";

const SAVE_HINT: &str = "Keep this as a playlist that fills itself";

const SAVED_HINT: &str = "A playlist already fills itself from this search";

const OPEN_HINT: &str = "See what this list would hold";

const SEARCH_HINT: &str = "Put this list's search in the search box and see it in the tracks pane";

const BACK_HINT: &str = "Back to every suggestion";

const CARD_PADDING: f32 = 12.0;

const CARD_GAP: f32 = 12.0;

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
            .gap_6()
            .px(px(SHELF_PADDING))
            .py_5()
            .overflow_y_scroll()
            .track_scroll(&self.suggestions_scroll);
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
                    .gap_3()
                    .child(kit::eyebrow(kind.name().to_uppercase()))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .content_start()
                            .gap(px(CARD_GAP))
                            .children(cards),
                    ),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(suggestions_heading(offered.len()))
            .when(nothing, |pane| {
                pane.child(empty(Icon::Suggestions, NOTHING_OFFERED, Some(SCAN_MORE)))
            })
            .when(!nothing, |pane| {
                pane.child(Scrollbars::of(cx).around(
                    "suggestions-scrollbar",
                    self.suggestions_scroll.clone(),
                    shelves,
                ))
            })
            .into_any_element()
    }

    fn suggestion_card(
        &mut self,
        index: usize,
        suggestion: &Suggestion,
        reached: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let opening = suggestion.query.clone();
        let side = theme::suggestion_card();
        let art = self.suggestion_art(suggestion, side, Drawn::InAGrid, true, cx);

        kit::card()
            .flex_none()
            .w(px(side))
            .p_0()
            .gap_0()
            .overflow_hidden()
            .hover(|card| card.border_color(rgb(theme::outline())))
            .child(
                div()
                    .id(("suggestion-open", index))
                    .flex()
                    .flex_col()
                    .flex_grow()
                    .cursor_pointer()
                    .names(OPEN_HINT)
                    .child(art)
                    .child(div().flex_1())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .px(px(CARD_PADDING))
                            .pt(px(CARD_PADDING))
                            .child(
                                kit::card_title(SharedString::from(suggestion.name.clone()))
                                    .truncate()
                                    .ends_in_an_ellipsis(),
                            )
                            .child(
                                div()
                                    .text_size(px(theme::text_sm()))
                                    .text_color(rgb(theme::muted()))
                                    .child(SharedString::from(suggestion.reason.says())),
                            )
                            .child(kit::figure(measured(suggestion))),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_offered(opening.clone(), cx);
                    })),
            )
            .child(
                kit::action_row()
                    .px(px(CARD_PADDING))
                    .pt_2()
                    .pb(px(CARD_PADDING))
                    .child(self.play_suggestion(("suggestion-play", index), &suggestion.query, cx))
                    .child(self.queue_mark(("suggestion-queue", index), &suggestion.query, cx))
                    .child(self.save_mark(("suggestion-save", index), suggestion, cx)),
            )
            .when(reached, |card| {
                card.child(reached_edge()).child(kit::brought_into_view(
                    self.suggestions_scroll.clone(),
                    Rc::clone(&self.reached_unseen),
                ))
            })
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
        let seen = self.suggestions_scroll.bounds().size;
        let taken = theme::suggestion_card() + CARD_GAP;
        let across = (f32::from(seen.width) - 2.0 * SHELF_PADDING + CARD_GAP) / taken;
        let down = f32::from(seen.height) / taken;

        (across.max(1.0) as usize) * (down.max(1.0) as usize)
    }

    fn opened_suggestion(
        &mut self,
        suggestion: &Suggestion,
        tracks: &Arc<[Track]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let playing = self.playing_now(cx).track;
        let art = self.suggestion_art(suggestion, self.hero_side(), Drawn::OnThePage, false, cx);
        let reads = suggestion
            .query
            .text
            .as_deref()
            .map(|text| Search::read(text).reads())
            .unwrap_or_default();
        let searched = suggestion.query.text.clone().unwrap_or_default();
        let shown = tracks.len();
        let rows = Arc::clone(tracks);

        let actions = kit::action_row()
            .flex_nowrap()
            .pt_2()
            .child(self.play_suggestion("opened-suggestion-play", &suggestion.query, cx))
            .child(self.shuffle_suggestion(&suggestion.query, cx))
            .child(self.save_mark("opened-suggestion-save", suggestion, cx))
            .child(
                kit::icon_button("opened-suggestion-search", Icon::Search, SEARCH_HINT).on_click(
                    cx.listener(move |this, _, window, cx| {
                        this.search_instead(searched.clone(), window, cx);
                        this.choose_pane(Pane::Tracks, cx);
                    }),
                ),
            );

        let heading = kit::heading()
            .child(
                kit::way_back("suggestions-back", "Suggestions", BACK_HINT).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.open_suggestion(None, cx));
                    },
                )),
            )
            .child(
                kit::hero().child(art).child(
                    div()
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::measures_its_height(self.hero_height.clone()))
                        .child(kit::eyebrow(format!(
                            "SUGGESTION · {}",
                            suggestion.reason.kind().name().to_uppercase()
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(listing::columns(
                "#",
                true,
                TRACK_CONTROLS,
                sorting::unsorted(),
                cx,
            ))
            .child(
                Scrollbars::of(cx).around(
                    "suggestion-tracks-scrollbar",
                    self.suggestion_rows.clone(),
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
                                let reached =
                                    this.reaches(Shift::Listing(Listed::Suggested), index);
                                drawn.push(reorder::marked(
                                    this.track_row(
                                        &rows,
                                        index,
                                        track,
                                        playing == Some(track.id),
                                        Plays::TheseRows,
                                        cx,
                                    ),
                                    reached,
                                ));
                            }
                            drawn
                        }),
                    )
                    .track_scroll(self.suggestion_rows.clone())
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
        on_a_card: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let drawn: Vec<Arc<gpui::Image>> = suggestion
            .pictured_by
            .iter()
            .take(PICTURED_BY_AT_MOST)
            .filter_map(|album| self.drawn_album(*album, drawn_at, cx))
            .collect();
        let framed = if on_a_card {
            Framed::OnACard
        } else {
            Framed::Alone
        };

        Mosaic {
            drawn: &drawn,
            ground: accents_of(suggestion),
            mark: icon_of(suggestion.reason),
            name: &suggestion.name,
        }
        .drawn(side, framed)
    }

    fn drawn_album(
        &mut self,
        album: AlbumId,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Arc<gpui::Image>> {
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
                SAVED_HINT,
                "suggestion-card",
            );
        }
        let saving = suggestion.query.clone();
        let named = suggestion.name.clone();

        kit::icon_button(id, Icon::Plus, SAVE_HINT).on_click(cx.listener(move |this, _, _, cx| {
            let saved = saving.clone();
            let name = named.clone();
            this.library.update(cx, |library, cx| {
                library.save_query(name, saved, cx);
            });
        }))
    }

    fn queue_mark(
        &self,
        id: impl Into<gpui::ElementId>,
        query: &SavedQuery,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queueing = query.clone();

        kit::icon_button(id, Icon::QueueLast, QUEUE_HINT).on_click(cx.listener(
            move |this, _, window, cx| {
                this.with_the_rows_of(&queueing, window, cx, move |this, rows, _, cx| {
                    this.queue(&listed(&rows), Placement::Queued, cx);
                });
            },
        ))
    }

    fn play_suggestion(
        &self,
        id: impl Into<gpui::ElementId>,
        query: &SavedQuery,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let playing = query.clone();

        kit::button(id, Some(Icon::Play), "Play", PLAY_HINT, Tone::Primary).on_click(cx.listener(
            move |this, _, window, cx| {
                this.with_the_rows_of(&playing, window, cx, |this, rows, _, cx| {
                    this.plays_in_order(cx);
                    this.play(&rows, 0, cx);
                });
            },
        ))
    }

    fn shuffle_suggestion(&self, query: &SavedQuery, cx: &mut Context<Self>) -> Stateful<Div> {
        let shuffling = query.clone();

        kit::button(
            "opened-suggestion-shuffle",
            Some(Icon::Shuffle),
            "Shuffle",
            SHUFFLE_HINT,
            Tone::Outlined,
        )
        .on_click(cx.listener(move |this, _, window, cx| {
            this.with_the_rows_of(&shuffling, window, cx, |this, rows, _, cx| {
                this.play_shuffled(&rows, cx);
            });
        }))
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

fn reached_edge() -> Div {
    div()
        .absolute()
        .inset_0()
        .rounded_lg()
        .border_2()
        .border_color(rgb(theme::accent()))
}

fn measured(suggestion: &Suggestion) -> String {
    let mut parts = vec![format::counted(suggestion.rows as usize, "track", "tracks")];
    if let Some(length) = suggestion.length {
        parts.push(format::spanned(length));
    }

    parts.join(" · ")
}

fn measured_shown(suggestion: &Suggestion, shown: usize) -> String {
    let whole = measured(suggestion);
    match (shown as u64) < suggestion.rows && shown > 0 {
        true => format!("{whole} · the first {shown} shown"),
        false => whole,
    }
}

fn accents_of(suggestion: &Suggestion) -> (Accent, Accent) {
    match suggestion.reason {
        Reason::Decade(_) => (Accent::Amber, Accent::Peach),
        Reason::NeverHeard => (Accent::Teal, Accent::Blue),
        Reason::MostPlayed => (Accent::Red, Accent::Peach),
        Reason::RecentlyAdded => (Accent::Green, Accent::Teal),
        Reason::BackCatalogue => (Accent::Amber, Accent::Mauve),
        Reason::HiRes => (Accent::Green, Accent::Blue),
        Reason::LongPlayers => (Accent::Blue, Accent::Mauve),
        Reason::Genre | Reason::Artist => mosaic::named_accents(&suggestion.name),
    }
}

fn icon_of(reason: Reason) -> Icon {
    match reason {
        Reason::Decade(_) | Reason::BackCatalogue => Icon::Albums,
        Reason::Genre => Icon::Tracks,
        Reason::Artist => Icon::Artists,
        Reason::NeverHeard | Reason::RecentlyAdded => Icon::Suggestions,
        Reason::MostPlayed => Icon::Statistics,
        Reason::HiRes => Icon::Equaliser,
        Reason::LongPlayers => Icon::Disc,
    }
}

fn suggestions_heading(offered: usize) -> Div {
    kit::heading().child(
        kit::heading_row().child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap_1()
                .child(kit::eyebrow("COLLECTION"))
                .child(kit::title("Suggestions"))
                .when(offered > 0, |column| {
                    column.child(kit::subtitle(format::counted(offered, "list", "lists")))
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
    fn a_shuffle_starts_somewhere_inside_the_list() {
        for rows in [1, 2, 7, 500] {
            assert!(crate::views::root::somewhere_in(rows) < rows);
        }
        assert_eq!(crate::views::root::somewhere_in(0), 0);
    }
}
