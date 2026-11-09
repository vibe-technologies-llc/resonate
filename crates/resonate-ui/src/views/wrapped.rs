use std::{env, fs, path::PathBuf, sync::Arc, time::SystemTime};

use gpui::{
    Background, Context, Div, FontWeight, Role, SharedString, Stateful, div, linear_color_stop,
    linear_gradient, prelude::*, px, relative, rgb,
};
use resonate_core::{
    Calendar,
    wrapped::{REVIEW_NAME, Slide, SlideBar, SlideKind, SlideRow, YearReview, year_worth_showing},
};
use resonate_library::{HistoryKept, Library};

use crate::{
    ResonateApp, clipboard, fonts,
    icons::Icon,
    keys,
    models::Notice,
    motion, theme, toast,
    views::{
        hint::Names,
        kit::{self, EndsInAnEllipsis as _, Tone},
        root::RootView,
        sheets,
        spoken::{Placed, Spoken},
    },
    words,
    wrapped_card::{self, CardColours},
};

const DECK_HEIGHT_AT_MOST: f32 = 0.88;

const PROGRESS_THICKNESS: f32 = 3.0;

const WASH_OF_THE_DECK: u8 = 0x2a;

const WASH_REACHES: f32 = 0.55;

const ACTIONS_PARTED_BY: f32 = 16.0;

const RANK_WIDTH: f32 = 20.0;

const TALLIES_STAND_APART: f32 = 8.0;

const FIRST_ROW_WASH: u8 = 0x1f;

fn back_hint() -> String {
    keys::keyed(words::wrapped::slide_before(), &[key!(seek_back)])
}

fn next_hint() -> String {
    keys::keyed(words::wrapped::slide_after(), &[key!(seek)])
}

fn close_hint() -> String {
    keys::keyed(words::wrapped::close(), &[key!(leave)])
}

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
                    Ok(None) => toast::tell(
                        Notice::Noted(words::wrapped::nothing_played().to_owned()),
                        cx,
                    ),
                    Err(error) => {
                        tracing::error!(%error, "the year could not be read");
                        toast::tell(
                            toast::could_not(words::wrapped::read_your_year(), &error),
                            cx,
                        );
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
        toast::tell(Notice::Done(words::wrapped::copied().to_owned()), cx);
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
                        toast::tell(
                            Notice::Trouble(words::wrapped::no_file_picker().to_owned()),
                            cx,
                        );
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
                        Some(()) => Notice::Done(words::wrapped::saved(named_file(&chosen))),
                        None => Notice::Trouble(words::wrapped::card_failed().to_owned()),
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

        let stage = div()
            .flex()
            .flex_col()
            .justify_center()
            .h(px(theme::wrapped_stage()))
            .flex_shrink(1.0)
            .min_h(px(0.0))
            .when_some(slide, |stage, slide| {
                stage.child(motion::faded_in(
                    slide_drawn(&slide, &deck.review),
                    ("wrapped-slide", deck.at),
                    motion::HANDS_OVER,
                ))
            });

        let card = kit::sheet()
            .id("wrapped-sheet")
            .dialog(REVIEW_NAME)
            .track_focus(&self.sheet_panels.holding(sheets::WRAPPED_PANEL, cx))
            .relative()
            .w(px(theme::wrapped_width()))
            .max_h(relative(DECK_HEIGHT_AT_MOST))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_xl()
                    .bg(the_deck_ground()),
            )
            .child(self.wrapped_head(deck, cx))
            .child(stage)
            .child(kit::rule())
            .child(self.wrapped_foot(deck, cx));

        kit::over_a_scrim(motion::lifted_in(card, "wrapped-sheet-arrives"))
    }

    fn wrapped_head(&self, deck: &Deck, cx: &mut Context<Self>) -> Div {
        let count = deck.slides.len();
        let reading = words::wrapped::slide_of(count, deck.at + 1);

        let mut progress = div()
            .id("wrapped-progress")
            .read_plainly(&reading)
            .flex()
            .flex_1()
            .items_center()
            .gap_1();
        for at in 0..count {
            let colour = if at <= deck.at {
                theme::accent()
            } else {
                theme::soft_edge()
            };
            progress = progress.child(
                div()
                    .flex_1()
                    .h(px(PROGRESS_THICKNESS))
                    .rounded_full()
                    .bg(rgb(colour)),
            );
        }

        div()
            .flex()
            .items_center()
            .gap_4()
            .child(progress)
            .child(kit::figure(reading))
            .child(self.in_the_pane_ring(
                kit::icon_button("wrapped-close", Icon::Close, close_hint()),
                |this, _, cx| this.close_wrapped(cx),
                cx,
            ))
    }

    fn a_turn(
        &self,
        slides: isize,
        id: &'static str,
        label: &'static str,
        saying: String,
        takes: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let tone = if slides > 0 {
            Tone::Primary
        } else {
            Tone::Ghost
        };
        if !takes {
            return kit::button_when(kit::Press::Greyed, id, None, label, saying, tone);
        }

        self.in_the_pane_ring(
            kit::button(id, None, label, saying, tone),
            move |this, _, cx| {
                this.turn_the_slide(slides, cx);
            },
            cx,
        )
    }

    fn wrapped_foot(&self, deck: &Deck, cx: &mut Context<Self>) -> Div {
        let last = deck.slides.len().saturating_sub(1);
        let holds_anything = deck.review.holds_anything();

        let mut years = div().flex().flex_1().items_center();
        if deck.years.len() > 1 {
            let mut offered = kit::segmented();
            for (index, year) in deck.years.iter().copied().enumerate() {
                offered = offered.child(
                    self.in_the_pane_ring(
                        kit::segment(
                            ("wrapped-year", index),
                            SharedString::from(year.to_string()),
                            year == deck.review.year,
                        )
                        .names(words::wrapped::year_hint()),
                        move |this, _, cx| this.read_wrapped(Some(year), cx),
                        cx,
                    ),
                );
            }
            years = years.child(offered);
        }

        let back = self.a_turn(
            -1,
            "wrapped-back",
            words::wrapped::back(),
            back_hint(),
            deck.at > 0,
            cx,
        );
        let onward = if deck.at < last {
            self.a_turn(
                1,
                "wrapped-next",
                words::wrapped::next(),
                next_hint(),
                true,
                cx,
            )
        } else {
            self.in_the_pane_ring(
                kit::button(
                    "wrapped-done",
                    None,
                    words::wrapped::done(),
                    close_hint(),
                    Tone::Primary,
                ),
                |this, _, cx| this.close_wrapped(cx),
                cx,
            )
        };

        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .child(years)
            .when(holds_anything, |foot| {
                foot.child(self.in_the_pane_ring(
                    kit::button(
                        "wrapped-copy",
                        None,
                        words::wrapped::copy(),
                        words::wrapped::copy_hint(),
                        Tone::Ghost,
                    ),
                    |this, _, cx| this.copy_the_year(cx),
                    cx,
                ))
                .child(self.in_the_pane_ring(
                    kit::button(
                        "wrapped-save",
                        None,
                        words::wrapped::save_card(),
                        words::wrapped::save_hint(),
                        Tone::Ghost,
                    ),
                    |this, _, cx| this.save_the_card(cx),
                    cx,
                ))
                .child(
                    div()
                        .w(px(1.0))
                        .h(px(ACTIONS_PARTED_BY))
                        .mx_1()
                        .bg(rgb(theme::soft_edge())),
                )
            })
            .child(back)
            .child(onward)
    }
}

fn the_deck_ground() -> Background {
    linear_gradient(
        160.0,
        linear_color_stop(theme::tinted(theme::accent(), WASH_OF_THE_DECK), 0.0),
        linear_color_stop(theme::tinted(theme::accent(), 0x00), WASH_REACHES),
    )
}

fn named_file(path: &std::path::Path) -> String {
    path.file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

fn slide_drawn(slide: &Slide, review: &YearReview) -> Stateful<Div> {
    let headline = SharedString::from(slide.headline.clone());
    let tallied = slide.kind == SlideKind::Whole && review.holds_anything();
    let says_little = slide.rows.is_empty() && slide.bars.is_empty();
    let headline_size = if says_little {
        theme::text_lyric()
    } else {
        theme::text_title()
    };
    let detail = slide.detail.clone().filter(|_| !tallied);

    div()
        .id("wrapped-slide-body")
        .flex()
        .flex_col()
        .gap_3()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .when(says_little, |body| body.items_center().text_center())
        .child(kit::eyebrow(slide.eyebrow.clone()).text_color(rgb(theme::accent())))
        .child(
            div()
                .id("wrapped-headline")
                .headed(1, &slide.headline)
                .text_size(px(headline_size))
                .line_height(px(headline_size * 1.15))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text()))
                .child(headline),
        )
        .when_some(detail, |body, detail| {
            body.child(
                div()
                    .id("wrapped-detail")
                    .read_plainly(&detail)
                    .text_size(px(theme::text_lg()))
                    .text_color(rgb(theme::muted()))
                    .child(SharedString::from(detail)),
            )
        })
        .when(tallied, |body| {
            body.child(div().h(px(TALLIES_STAND_APART)))
                .child(tallies_drawn(review))
        })
        .when(!slide.rows.is_empty(), |body| {
            body.child(rows_drawn(&slide.rows))
        })
        .when(!slide.bars.is_empty(), |body| {
            body.child(bars_drawn(&slide.bars))
        })
}

fn tallies_drawn(review: &YearReview) -> Stateful<Div> {
    let tallies = [
        (words::wrapped::plays(), review.plays),
        (words::wrapped::songs(), review.songs),
        (words::wrapped::artists(), review.artists),
        (words::wrapped::albums(), review.albums),
        (words::wrapped::days(), u64::from(review.rhythm.days_heard)),
    ];

    let mut drawn = div().id("wrapped-tallies").flex().w_full().gap_2();
    for (at, (named, count)) in tallies.into_iter().enumerate() {
        let reading = count.to_string();

        drawn = drawn.child(
            kit::card()
                .id(("wrapped-tally", at))
                .read_plainly(&format!("{named}: {reading}"))
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .gap_1()
                .p_3()
                .bg(rgb(theme::raised()))
                .border_color(rgb(theme::soft_edge()))
                .child(kit::eyebrow(named))
                .child(
                    kit::figure(reading)
                        .text_size(px(theme::text_xl()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::text())),
                ),
        );
    }

    drawn
}

fn rows_drawn(rows: &[SlideRow]) -> Stateful<Div> {
    let mut listed = div()
        .id("wrapped-rows")
        .spoken_as(Role::List)
        .flex()
        .flex_col()
        .gap_0p5();
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
                .px_2()
                .py_1p5()
                .rounded_lg()
                .when(at == 0, |row| {
                    row.bg(theme::tinted(theme::accent(), FIRST_ROW_WASH))
                })
                .child(
                    div()
                        .w(px(RANK_WIDTH))
                        .flex_none()
                        .text_size(px(theme::text_base()))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(if at == 0 {
                            theme::accent()
                        } else {
                            theme::faint()
                        }))
                        .child(SharedString::from((at + 1).to_string())),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .child(
                            div()
                                .truncate()
                                .ends_in_an_ellipsis()
                                .text_size(px(theme::text_base()))
                                .text_color(rgb(theme::text()))
                                .child(SharedString::from(row.name.clone())),
                        )
                        .when_some(row.by.clone(), |named, by| {
                            named.child(
                                div()
                                    .truncate()
                                    .ends_in_an_ellipsis()
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
