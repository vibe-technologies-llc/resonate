use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, Context, Div, SharedString, Stateful, div, prelude::*, px, rgb, uniform_list,
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
        root::{RootView, empty, listed},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_OFFERED: &str = "The catalog has nothing to suggest yet.";

const SCAN_MORE: &str = "Scan more music and lists it can fill itself will appear here.";

const PLAY_HINT: &str = "Play what this list would hold";

const SHUFFLE_HINT: &str = "Play what this list would hold, shuffled";

const QUEUE_HINT: &str = "Queue what this list would hold after what is already queued, ahead of the rest of what is playing";

const SAVE_HINT: &str = "Keep this as a playlist that fills itself";

const SAVED_HINT: &str = "A playlist already fills itself from this search";

const OPEN_HINT: &str = "See what this list would hold";

const SEARCH_HINT: &str = "Put this list's search in the search box and see it in the tracks pane";

const BACK_HINT: &str = "Back to every suggestion";

const CARD_PADDING: f32 = 12.0;

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
            .px_6()
            .py_5()
            .overflow_y_scroll()
            .track_scroll(&self.suggestions_scroll);
        for kind in SuggestionKind::ALL {
            let cards: Vec<AnyElement> = offered
                .iter()
                .enumerate()
                .filter(|(_, suggestion)| suggestion.reason.kind() == kind)
                .map(|(index, suggestion)| {
                    self.suggestion_card(index, suggestion, cx)
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
                            .gap_3()
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
                        let opened = opening.clone();
                        this.library
                            .update(cx, |library, cx| library.open_suggestion(Some(opened), cx));
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
                            let mut drawn = Vec::new();
                            for index in range {
                                let Some(track) = rows.get(index) else {
                                    continue;
                                };
                                drawn.push(this.track_row(
                                    &rows,
                                    index,
                                    track,
                                    playing == Some(track.id),
                                    Plays::TheseRows,
                                    cx,
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
    #[test]
    fn a_shuffle_starts_somewhere_inside_the_list() {
        for rows in [1, 2, 7, 500] {
            assert!(crate::views::root::somewhere_in(rows) < rows);
        }
        assert_eq!(crate::views::root::somewhere_in(0), 0);
    }
}
