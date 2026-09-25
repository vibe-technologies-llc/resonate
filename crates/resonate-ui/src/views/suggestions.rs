use std::{
    ops::Range,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{
    AnyElement, Background, Context, Div, FontWeight, ObjectFit, SharedString, Stateful, div, img,
    linear_color_stop, linear_gradient, prelude::*, px, rgb, uniform_list,
};
use resonate_core::{Accent, AlbumId};
use resonate_engine::{Command, Placement};
use resonate_library::{
    PICTURED_BY_AT_MOST, Reason, SavedQuery, Search, Suggestion, SuggestionKind, Track,
};

use crate::{
    Drawn, Pane, format,
    icons::{self, Icon},
    theme,
    views::{
        browser::Plays,
        hint::Names,
        kit::{self, EndsInAnEllipsis, Press, Tone},
        listing, menu,
        root::{RootView, empty, listed},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_OFFERED: &str = "The catalog has nothing to suggest yet.";

const SCAN_MORE: &str = "Scan more music and lists it can fill itself will appear here.";

const PLAY_HINT: &str = "Play what this list would hold";

const SHUFFLE_HINT: &str = "Play what this list would hold, shuffled";

const NEXT_HINT: &str = "Hear what this list would hold straight after the track playing";

const QUEUE_HINT: &str = "Queue what this list would hold after what is already queued, ahead of the rest of what is playing";

const SAVE_HINT: &str = "Keep this as a playlist that fills itself";

const SAVED_HINT: &str = "A playlist already fills itself from this search";

const OPEN_HINT: &str = "See what this list would hold";

const SEARCH_HINT: &str = "Put this list's search in the search box and see it in the tracks pane";

const BACK_HINT: &str = "Back to every suggestion";

const CARD_GROUP: &str = "suggestion-card";

const TILES_A_SIDE: usize = 2;

const NAME_ON_THE_ART: f32 = 0.11;

const MARK_ON_THE_ART: f32 = 0.22;

const ART_ROUNDING: f32 = 8.0;

const MARK_ALPHA: u8 = 0x9c;

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
        let side = theme::suggestion_card() - 2.0 * CARD_PADDING;
        let art = self.suggestion_art(suggestion, side, cx);

        kit::card()
            .flex_none()
            .w(px(theme::suggestion_card()))
            .p(px(CARD_PADDING))
            .gap_3()
            .justify_between()
            .child(
                div()
                    .id(("suggestion-open", index))
                    .group(CARD_GROUP)
                    .flex()
                    .flex_col()
                    .gap_2()
                    .cursor_pointer()
                    .names(OPEN_HINT)
                    .child(
                        div()
                            .rounded(px(ART_ROUNDING))
                            .border_1()
                            .border_color(theme::tinted(theme::text(), 0x0c))
                            .group_hover(CARD_GROUP, |art| art.border_color(rgb(theme::outline())))
                            .child(art),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
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
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_1p5()
                    .child(self.save_suggestion(("suggestion-save", index), suggestion, cx))
                    .child(self.queue_suggestion(
                        ("suggestion-queue", index),
                        &suggestion.query,
                        Placement::Queued,
                        cx,
                    ))
                    .child(self.play_suggestion(("suggestion-play", index), &suggestion.query, cx)),
            )
    }

    fn opened_suggestion(
        &mut self,
        suggestion: &Suggestion,
        tracks: &Arc<[Track]>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let playing = self.playing_now(cx).track;
        let art = self.suggestion_art(suggestion, theme::scope_cover(), cx);
        let reads = suggestion
            .query
            .text
            .as_deref()
            .map(|text| Search::read(text).reads())
            .unwrap_or_default();
        let searched = suggestion.query.text.clone().unwrap_or_default();
        let shown = tracks.len();
        let rows = Arc::clone(tracks);

        let actions = kit::actions()
            .child(
                kit::button(
                    "opened-suggestion-search",
                    Some(Icon::Search),
                    "Search for these",
                    SEARCH_HINT,
                    Tone::Ghost,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.search_instead(searched.clone(), window, cx);
                    this.choose_pane(Pane::Tracks, cx);
                })),
            )
            .child(self.save_suggestion("opened-suggestion-save", suggestion, cx))
            .child(self.queue_suggestion(
                "opened-suggestion-next",
                &suggestion.query,
                Placement::Next,
                cx,
            ))
            .child(self.queue_suggestion(
                "opened-suggestion-last",
                &suggestion.query,
                Placement::Queued,
                cx,
            ))
            .child(self.shuffle_suggestion(&suggestion.query, cx))
            .child(self.play_suggestion("opened-suggestion-play", &suggestion.query, cx));

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
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
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
                        .child(div().pt_2().child(actions)),
                ),
            );

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .child(listing::columns("#", true, sorting::unsorted(), cx))
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
        cx: &mut Context<Self>,
    ) -> Div {
        let drawn: Vec<Arc<gpui::Image>> = suggestion
            .pictured_by
            .iter()
            .take(PICTURED_BY_AT_MOST)
            .filter_map(|album| self.drawn_album(*album, cx))
            .collect();
        let (from, to) = ground_of(suggestion);
        let side = side.round();

        let frame = div()
            .relative()
            .flex_none()
            .size(px(side))
            .rounded(px(ART_ROUNDING))
            .overflow_hidden();

        match drawn.as_slice() {
            [] => frame
                .bg(ground(from, to))
                .child(lettered(suggestion, side, from)),
            [only] => frame.child(
                img(Arc::clone(only))
                    .size(px(side))
                    .rounded(px(ART_ROUNDING))
                    .object_fit(ObjectFit::Cover),
            ),
            several => frame.child(tiled(several, side, suggestion)),
        }
    }

    fn drawn_album(&mut self, album: AlbumId, cx: &mut Context<Self>) -> Option<Arc<gpui::Image>> {
        self.library
            .update(cx, |library, cx| library.cover(album, Drawn::InAGrid, cx))
    }

    fn save_suggestion(
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
            return kit::button_when(
                Press::Greyed,
                id,
                Some(Icon::Check),
                "Saved",
                SAVED_HINT,
                Tone::Ghost,
            );
        }
        let saving = suggestion.query.clone();
        let named = suggestion.name.clone();

        kit::button(id, Some(Icon::Plus), "Save", SAVE_HINT, Tone::Ghost).on_click(cx.listener(
            move |this, _, _, cx| {
                let saved = saving.clone();
                let name = named.clone();
                this.library.update(cx, |library, cx| {
                    library.save_query(name, saved, cx);
                });
            },
        ))
    }

    fn queue_suggestion(
        &self,
        id: impl Into<gpui::ElementId>,
        query: &SavedQuery,
        at: Placement,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let queueing = query.clone();
        let (icon, label, hint) = match at {
            Placement::Next => (Icon::QueueNext, menu::PLAY_NEXT, NEXT_HINT),
            Placement::Queued | Placement::At(_) => {
                (Icon::QueueLast, menu::ADD_TO_QUEUE, QUEUE_HINT)
            }
        };

        kit::button(id, Some(icon), label, hint, Tone::Outlined).on_click(cx.listener(
            move |this, _, window, cx| {
                this.with_the_rows_of(&queueing, window, cx, move |this, rows, _, cx| {
                    this.queue(&listed(&rows), at, cx);
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
                this.play(&rows, somewhere_in(rows.len()), cx);
                this.send(Command::SetShuffle(true), cx);
            });
        }))
    }
}

fn somewhere_in(rows: usize) -> usize {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as usize)
        .unwrap_or_default();

    now.checked_rem(rows).unwrap_or_default()
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

fn ground_of(suggestion: &Suggestion) -> (u32, u32) {
    let (from, to) = accents_of(suggestion);

    (theme::hue(from), theme::hue(to))
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
        Reason::Genre | Reason::Artist => named_accents(&suggestion.name),
    }
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn named_accents(name: &str) -> (Accent, Accent) {
    let hashed = name.bytes().fold(FNV_OFFSET_BASIS, |hashed, byte| {
        (hashed ^ u64::from(byte)).wrapping_mul(FNV_PRIME)
    });
    let accents = Accent::ALL.len() as u64;
    let first = (hashed % accents) as usize;
    let apart = 1 + ((hashed / accents) % (accents - 1)) as usize;

    (
        Accent::ALL[first],
        Accent::ALL[(first + apart) % Accent::ALL.len()],
    )
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

fn lettered(suggestion: &Suggestion, side: f32, ground: u32) -> Div {
    let ink = theme::ink_over(ground);

    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .justify_between()
        .p(px(side * 0.08))
        .child(icons::icon(
            icon_of(suggestion.reason),
            side * MARK_ON_THE_ART,
            ink,
        ))
        .child(
            div()
                .text_size(px(side * NAME_ON_THE_ART))
                .line_height(px(side * NAME_ON_THE_ART * 1.1))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::tinted(ink, MARK_ALPHA))
                .line_clamp(3)
                .child(SharedString::from(suggestion.name.clone())),
        )
}

fn ground(from: u32, to: u32) -> Background {
    linear_gradient(
        135.0,
        linear_color_stop(rgb(from), 0.0),
        linear_color_stop(rgb(to), 1.0),
    )
}

fn tiled(drawn: &[Arc<gpui::Image>], side: f32, suggestion: &Suggestion) -> Div {
    let tile = (side / TILES_A_SIDE as f32).floor();
    let tiles = TILES_A_SIDE * TILES_A_SIDE;
    let (from, to) = ground_of(suggestion);

    let mut grid = div()
        .flex()
        .flex_wrap()
        .size(px(tile * TILES_A_SIDE as f32));
    for at in 0..tiles {
        let corner = Corner::of_tile(at);
        let cell = corner.round(div().size(px(tile)).overflow_hidden());
        grid = grid.child(match drawn.get(at) {
            Some(art) => cell.child(
                corner.round(
                    img(Arc::clone(art))
                        .size(px(tile))
                        .object_fit(ObjectFit::Cover),
                ),
            ),
            None => cell.bg(rgb(if at % 2 == 0 { from } else { to })),
        });
    }

    grid
}

#[derive(Clone, Copy)]
enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    const fn of_tile(at: usize) -> Self {
        let right = at % TILES_A_SIDE == TILES_A_SIDE - 1;
        let bottom = at / TILES_A_SIDE == TILES_A_SIDE - 1;
        match (bottom, right) {
            (false, false) => Self::TopLeft,
            (false, true) => Self::TopRight,
            (true, false) => Self::BottomLeft,
            (true, true) => Self::BottomRight,
        }
    }

    fn round<E: Styled>(self, element: E) -> E {
        let rounding = px(ART_ROUNDING);
        match self {
            Self::TopLeft => element.rounded_tl(rounding),
            Self::TopRight => element.rounded_tr(rounding),
            Self::BottomLeft => element.rounded_bl(rounding),
            Self::BottomRight => element.rounded_br(rounding),
        }
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

    #[test]
    fn a_named_suggestion_is_grounded_in_two_different_accents_and_the_same_two_every_time() {
        for name in ["Progressive Rock", "The Orbiters", "", "Trip Hop", "Jazz"] {
            let (from, to) = named_accents(name);

            assert_ne!(from, to, "{name} was grounded in one accent");
            assert_eq!(
                named_accents(name),
                (from, to),
                "{name} moved between reads"
            );
        }
    }

    #[test]
    fn a_shuffle_starts_somewhere_inside_the_list() {
        for rows in [1, 2, 7, 500] {
            assert!(somewhere_in(rows) < rows);
        }
        assert_eq!(somewhere_in(0), 0);
    }
}
