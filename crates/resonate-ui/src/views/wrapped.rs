use std::{env, fs, path::PathBuf, sync::Arc, time::SystemTime};

use gpui::{
    BoxShadow, Context, Div, FontWeight, Role, SharedString, Stateful, div, point, prelude::*, px,
    relative, rgb, rgba,
};
use resonate_core::{
    Calendar,
    wrapped::{REVIEW_NAME, Slide, SlideBar, SlideRow, YearReview, year_worth_showing},
};
use resonate_library::{HistoryKept, Library};

use crate::{
    ResonateApp, clipboard, fonts,
    models::Notice,
    motion, theme, toast,
    views::{
        hint::Names,
        kit::{self, Tone},
        root::RootView,
        sheets,
        spoken::{Placed, Spoken},
    },
    wrapped_card::{self, CardColours},
};

const WRAPPED_DECK_TALL: f32 = 480.0;

const NOTHING_PLAYED: &str = "Nothing has been played yet, so there is no year to review";

const BACK_HINT: &str = keyed!("The slide before", key!(seek_back));

const NEXT_HINT: &str = keyed!("The next slide", key!(seek));

const CLOSE_HINT: &str = keyed!("Close", key!(leave));

const COPY_HINT: &str = "Copy the year as a few lines of text";

const SAVE_HINT: &str = "Save a card of the year as a picture";

const YEAR_HINT: &str = "Review this year";

const COPIED: &str = "Copied your year";

const CARD_FAILED: &str = "Couldn't draw the card";

const NO_FILE_PICKER: &str = "No file picker could be opened";

const LAST_RESORT_FOLDER: &str = ".";

const WHOLE_SHARE: f32 = 1_000.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Deck {
    pub(crate) years: Arc<[i64]>,
    pub(crate) review: Arc<YearReview>,
    pub(crate) slides: Arc<[Slide]>,
    pub(crate) at: usize,
}

impl Deck {
    fn of(years: Vec<i64>, review: YearReview) -> Self {
        Self {
            years: years.into(),
            slides: review.slides().into(),
            review: Arc::new(review),
            at: 0,
        }
    }

    fn turned_by(&mut self, slides: isize) -> bool {
        let last = self.slides.len().saturating_sub(1);
        let Some(to) = self.at.checked_add_signed(slides).map(|to| to.min(last)) else {
            return false;
        };
        let moved = to != self.at;
        self.at = to;

        moved
    }
}

fn reviewed(
    library: &Library,
    year: Option<i64>,
    history: HistoryKept,
) -> resonate_library::Result<Option<Deck>> {
    let calendar = Calendar::local();
    let years = library.years_heard(&calendar)?;
    let today = calendar.date_at(SystemTime::now());
    let Some(year) = year.or_else(|| year_worth_showing(&years, today)) else {
        return Ok(None);
    };

    Ok(Some(Deck::of(
        years,
        library.year_review(year, &calendar, history)?,
    )))
}

impl RootView {
    pub(crate) fn open_wrapped(&mut self, cx: &mut Context<Self>) {
        self.read_wrapped(None, cx);
    }

    pub(crate) fn close_wrapped(&mut self, cx: &mut Context<Self>) {
        self.wrapped = None;
        self.reading_wrapped = None;
        cx.notify();
    }

    pub(crate) fn turn_the_slide(&mut self, slides: isize, cx: &mut Context<Self>) -> bool {
        let Some(deck) = self.wrapped.as_mut() else {
            return false;
        };
        if deck.turned_by(slides) {
            cx.notify();
        }

        true
    }

    fn read_wrapped(&mut self, year: Option<i64>, cx: &mut Context<Self>) {
        let library = self.library.read(cx).catalog();
        let history = cx.global::<ResonateApp>().history_kept;
        let read = cx.background_spawn(async move { reviewed(&library, year, history) });

        self.reading_wrapped = Some(cx.spawn(async move |this, cx| {
            let answer = read.await;
            let taken = this.update(cx, |this, cx| {
                match answer {
                    Ok(Some(deck)) => this.wrapped = Some(deck),
                    Ok(None) => toast::tell(Notice::Noted(NOTHING_PLAYED.to_owned()), cx),
                    Err(error) => {
                        tracing::error!(%error, "the year could not be read");
                        toast::tell(toast::could_not("read your year", &error), cx);
                    }
                }
                cx.notify();
            });
            let _ = taken;
        }));
    }

    fn copy_the_year(&self, cx: &mut Context<Self>) {
        let Some(deck) = self.wrapped.as_ref() else {
            return;
        };

        clipboard::copy(deck.review.summary(), cx);
        toast::tell(Notice::Done(COPIED.to_owned()), cx);
    }

    fn save_the_card(&self, cx: &mut Context<Self>) {
        let Some(deck) = self.wrapped.as_ref() else {
            return;
        };
        let review = Arc::clone(&deck.review);
        let colours = CardColours {
            ground: theme::background(),
            raised: theme::raised(),
            accent: theme::accent(),
            text: theme::text(),
            muted: theme::muted(),
            family: fonts::body().to_string(),
        };
        let beside = env::home_dir().unwrap_or_else(|| PathBuf::from(LAST_RESORT_FOLDER));
        let named = format!("resonate-wrapped-{}.png", review.year);
        let picked = cx.prompt_for_new_path(&beside, Some(&named));

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the file picker could not be opened");
                    let told = this.update(cx, |_, cx| {
                        toast::tell(Notice::Trouble(NO_FILE_PICKER.to_owned()), cx);
                    });
                    let _ = told;
                    return;
                }
            };
            let written = cx
                .background_spawn({
                    let chosen = chosen.clone();
                    async move {
                        let picture = wrapped_card::pictured(&review, &colours)
                            .inspect_err(|error| tracing::error!(%error, "the card failed"))
                            .ok()?;

                        fs::write(&chosen, picture)
                            .inspect_err(|error| {
                                tracing::error!(%error, "the card could not be written");
                            })
                            .ok()
                    }
                })
                .await;
            let told = this.update(cx, |_, cx| {
                toast::tell(
                    match written {
                        Some(()) => Notice::Done(format!("Saved {}", named_file(&chosen))),
                        None => Notice::Trouble(CARD_FAILED.to_owned()),
                    },
                    cx,
                );
            });
            let _ = told;
        })
        .detach();
    }

    pub(crate) fn wrapped_sheet(&self, cx: &mut Context<Self>) -> Div {
        let Some(deck) = self.wrapped.as_ref() else {
            return div();
        };
        let slide = deck.slides.get(deck.at).cloned();

        let card = div()
            .id("wrapped-sheet")
            .dialog(REVIEW_NAME)
            .track_focus(&self.sheet_panels.holding(sheets::WRAPPED_PANEL, cx))
            .flex()
            .flex_col()
            .gap_4()
            .w(px(theme::wrapped_width()))
            .h(px(WRAPPED_DECK_TALL))
            .max_h(relative(0.92))
            .p_6()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .shadow(vec![BoxShadow {
                color: theme::shadow(0.5),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(48.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .child(div().flex_1().min_h(px(0.0)).overflow_hidden().when_some(
                slide,
                |body, slide| {
                    body.child(motion::faded_in(
                        slide_drawn(&slide),
                        ("wrapped-slide", deck.at),
                        motion::HANDS_OVER,
                    ))
                },
            ))
            .child(self.wrapped_steps(deck, cx))
            .child(self.wrapped_foot(deck, cx));

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme::scrim()))
            .occlude()
            .child(motion::lifted_in(card, "wrapped-sheet-arrives"))
    }

    fn wrapped_steps(&self, deck: &Deck, cx: &mut Context<Self>) -> Div {
        let last = deck.slides.len().saturating_sub(1);
        let back = self.a_turn(-1, "wrapped-back", "Back", BACK_HINT, deck.at > 0, cx);
        let next = self.a_turn(1, "wrapped-next", "Next", NEXT_HINT, deck.at < last, cx);

        div()
            .flex()
            .items_center()
            .justify_between()
            .child(back)
            .child(kit::figure(format!("{} of {}", deck.at + 1, last + 1)))
            .child(next)
    }

    fn a_turn(
        &self,
        slides: isize,
        id: &'static str,
        label: &'static str,
        saying: &'static str,
        takes: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        if !takes {
            return kit::button_when(kit::Press::Greyed, id, None, label, saying, Tone::Ghost);
        }

        self.in_the_pane_ring(
            kit::button(id, None, label, saying, Tone::Ghost),
            move |this, _, cx| {
                this.turn_the_slide(slides, cx);
            },
            cx,
        )
    }

    fn wrapped_foot(&self, deck: &Deck, cx: &mut Context<Self>) -> Div {
        let mut years = div().flex().flex_1().flex_wrap().items_center().gap_1p5();
        if deck.years.len() > 1 {
            for (index, year) in deck.years.iter().copied().enumerate() {
                years = years.child(
                    self.in_the_pane_ring(
                        kit::chip(
                            ("wrapped-year", index),
                            SharedString::from(year.to_string()),
                            year == deck.review.year,
                        )
                        .names(YEAR_HINT),
                        move |this, _, cx| this.read_wrapped(Some(year), cx),
                        cx,
                    ),
                );
            }
        }
        let holds_anything = deck.review.holds_anything();

        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(years)
            .when(holds_anything, |foot| {
                foot.child(self.in_the_pane_ring(
                    kit::button("wrapped-copy", None, "Copy", COPY_HINT, Tone::Ghost),
                    |this, _, cx| this.copy_the_year(cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    kit::button("wrapped-save", None, "Save card", SAVE_HINT, Tone::Outlined),
                    |this, _, cx| this.save_the_card(cx),
                    cx,
                ))
            })
            .child(self.in_the_pane_ring(
                kit::button("wrapped-close", None, "Close", CLOSE_HINT, Tone::Ghost),
                |this, _, cx| this.close_wrapped(cx),
                cx,
            ))
    }
}

fn named_file(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn slide_drawn(slide: &Slide) -> Stateful<Div> {
    let headline = SharedString::from(slide.headline.clone());

    div()
        .id("wrapped-slide-body")
        .flex()
        .flex_col()
        .gap_3()
        .min_h(px(theme::chart_height() * 2.0))
        .overflow_y_scroll()
        .child(kit::eyebrow(slide.eyebrow.to_uppercase()))
        .child(
            div()
                .id("wrapped-headline")
                .headed(1, &slide.headline)
                .text_size(px(theme::text_title()))
                .line_height(px(theme::text_title() * 1.15))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text()))
                .child(headline),
        )
        .when_some(slide.detail.clone(), |body, detail| {
            body.child(
                div()
                    .id("wrapped-detail")
                    .read_plainly(&detail)
                    .text_size(px(theme::text_base()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(detail)),
            )
        })
        .when(!slide.rows.is_empty(), |body| {
            body.child(rows_drawn(&slide.rows))
        })
        .when(!slide.bars.is_empty(), |body| {
            body.child(bars_drawn(&slide.bars))
        })
}

fn rows_drawn(rows: &[SlideRow]) -> Stateful<Div> {
    let mut listed = div()
        .id("wrapped-rows")
        .spoken_as(Role::List)
        .flex()
        .flex_col()
        .gap_1p5();
    for (at, row) in rows.iter().enumerate() {
        let said = row.by.as_ref().map_or_else(
            || format!("{}, {}", row.name, row.figure),
            |by| format!("{}, {by}, {}", row.name, row.figure),
        );

        listed = listed.child(
            div()
                .id(("wrapped-row", at))
                .listed_as(move || SharedString::from(said))
                .placed(Placed { at, of: rows.len() })
                .flex()
                .items_center()
                .gap_3()
                .child(kit::figure((at + 1).to_string()))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(theme::text_base()))
                                .text_color(rgb(theme::text()))
                                .child(SharedString::from(row.name.clone())),
                        )
                        .when_some(row.by.clone(), |named, by| {
                            named.child(
                                div()
                                    .truncate()
                                    .text_size(px(theme::text_xs()))
                                    .text_color(rgb(theme::muted()))
                                    .child(SharedString::from(by)),
                            )
                        }),
                )
                .child(kit::figure(row.figure.clone())),
        );
    }

    listed
}

fn bars_drawn(bars: &[SlideBar]) -> Stateful<Div> {
    let mut drawn = div()
        .id("wrapped-bars")
        .spoken_as(Role::List)
        .flex()
        .w_full()
        .items_end()
        .h(px(theme::chart_height()))
        .gap(px(theme::CHART_BAR_GAP));
    for (at, bar) in bars.iter().enumerate() {
        let said = bar.said.clone();
        let named = bar.said.clone();
        let colour = if bar.peak {
            theme::accent()
        } else {
            theme::soft_edge()
        };

        drawn = drawn.child(
            div()
                .id(("wrapped-bar", at))
                .listed_as(move || SharedString::from(said))
                .placed(Placed { at, of: bars.len() })
                .flex()
                .flex_1()
                .flex_col()
                .justify_end()
                .h_full()
                .min_w(px(theme::CHART_BAR))
                .names_when_raised(move || SharedString::from(named.clone()))
                .child(
                    div()
                        .w_full()
                        .h(relative(f32::from(bar.share_permille) / WHOLE_SHARE))
                        .min_h(px(theme::CHART_BASELINE))
                        .rounded_sm()
                        .bg(rgb(colour)),
                ),
        );
    }

    drawn
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::wrapped::Ranked;

    use super::*;

    fn a_deck() -> Deck {
        Deck::of(
            vec![2026],
            YearReview {
                year: 2026,
                listened: Duration::from_secs(3_600),
                plays: 2,
                top_songs: vec![Ranked {
                    name: "Echoes".to_owned(),
                    by: None,
                    plays: 2,
                    listened: Duration::from_secs(3_600),
                }],
                ..YearReview::default()
            },
        )
    }

    #[gpui::test]
    fn the_arrows_turn_the_slides_and_escape_closes_the_deck_leaving_the_pane_alone(
        cx: &mut gpui::TestAppContext,
    ) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog opens"));
        let mut driven = crate::driven::Driven::open(cx, library);
        let root = driven.root.clone();
        let pane = driven.read(|root, _| root.pane);

        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.wrapped = Some(a_deck());
                window.focus(&root.focus, cx);
                cx.notify();
            });
        });
        driven.settle();

        assert!(driven.read(|root, _| root.something_stands_over_the_pane()));

        driven.cx.simulate_keystrokes("right");
        driven.settle();
        assert_eq!(
            driven.read(|root, _| root.wrapped.as_ref().map(|deck| deck.at)),
            Some(1)
        );

        driven.cx.simulate_keystrokes("left left");
        driven.settle();
        assert_eq!(
            driven.read(|root, _| root.wrapped.as_ref().map(|deck| deck.at)),
            Some(0)
        );

        driven.click("wrapped-next");
        assert_eq!(
            driven.read(|root, _| root.wrapped.as_ref().map(|deck| deck.at)),
            Some(1)
        );

        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert!(driven.read(|root, _| root.wrapped.is_none()));
        assert_eq!(driven.read(|root, _| root.pane), pane);
    }

    #[test]
    fn a_deck_turns_within_its_slides_and_says_whether_it_moved() {
        let mut deck = a_deck();
        let last = deck.slides.len() - 1;

        assert!(!deck.turned_by(-1));
        assert_eq!(deck.at, 0);
        assert!(deck.turned_by(1));
        assert_eq!(deck.at, 1);
        assert!(deck.turned_by(99));
        assert_eq!(deck.at, last);
        assert!(!deck.turned_by(1));
        assert_eq!(deck.at, last);
    }
}
