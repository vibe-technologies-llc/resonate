use std::time::Duration;

use gpui::{
    AnyElement, Bounds, Context, Div, DragMoveEvent, IntoElement, Pixels, Point, Render,
    ScrollStrategy, SharedString, Stateful, UniformListScrollHandle, Window, div, prelude::*, px,
    rgb, rgba,
};
use resonate_core::{PlaylistId, Span};

use crate::{
    icons::{self, Icon},
    theme,
    views::{kit, root::RootView},
};

pub(crate) const MOVING_HINT: &str = "Drag a row to where it should play. From the keyboard, up and down reach a row, shift-up and \
     shift-down reach more of them, home, end, page up and page down reach further, ctrl-a reaches \
     every row, alt-up and alt-down move whatever is reached, enter plays it and delete takes it \
     away. Shift-click reaches every row between";

const DRAGGING_EDGE: f32 = 36.0;

const DRAGGING_STEP: Duration = Duration::from_millis(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shift {
    Queue,
    Playlist(PlaylistId),
    Listing(Listed),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listed {
    Tracks,
    Albums,
    Artists,
}

impl Shift {
    pub(crate) const fn is_edited(self) -> bool {
        matches!(self, Self::Queue | Self::Playlist(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Toward {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    Above,
    Below,
}

impl Step {
    pub(crate) const fn end(self, rows: Span) -> usize {
        match self {
            Self::Above => rows.first(),
            Self::Below => rows.last(),
        }
    }

    pub(crate) const fn landing(self, rows: Span, held: usize) -> Option<usize> {
        let end = self.end(rows);
        match self {
            Self::Above => end.checked_sub(1),
            Self::Below if end + 1 < held => Some(end + 1),
            Self::Below => None,
        }
    }

    const fn glyph(self) -> Icon {
        match self {
            Self::Above => Icon::ChevronUp,
            Self::Below => Icon::ChevronDown,
        }
    }

    const fn saying(self) -> &'static str {
        match self {
            Self::Above => "Play what is reached one place sooner — alt-up",
            Self::Below => "Play what is reached one place later — alt-down",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reach {
    pub(crate) shift: Shift,
    pub(crate) anchor: usize,
    pub(crate) row: usize,
}

impl Reach {
    pub(crate) const fn at(shift: Shift, row: usize) -> Self {
        Self {
            shift,
            anchor: row,
            row,
        }
    }

    pub(crate) const fn rows(self) -> Span {
        Span::between(self.anchor, self.row)
    }

    pub(crate) const fn within(self, held: usize) -> Option<Self> {
        let Some(last) = held.checked_sub(1) else {
            return None;
        };

        Some(Self {
            shift: self.shift,
            anchor: if self.anchor < last {
                self.anchor
            } else {
                last
            },
            row: if self.row < last { self.row } else { last },
        })
    }

    pub(crate) const fn stepped(self, step: Step) -> Self {
        match step {
            Step::Above => Self {
                shift: self.shift,
                anchor: self.anchor.saturating_sub(1),
                row: self.row.saturating_sub(1),
            },
            Step::Below => Self {
                shift: self.shift,
                anchor: self.anchor + 1,
                row: self.row + 1,
            },
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Carried {
    pub(crate) shift: Shift,
    pub(crate) rows: Span,
    pub(crate) title: SharedString,
}

pub(crate) struct Ghost {
    title: SharedString,
    beside: Option<SharedString>,
    under: Point<Pixels>,
}

impl Render for Ghost {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().pl(self.under.x).pt(self.under.y).child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .max_w(theme::width(theme::ghost_width()))
                .px_3()
                .py_1p5()
                .rounded_md()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(theme::tinted(theme::accent(), 0xaa))
                .font_family(theme::ui_face())
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::text()))
                .child(div().flex_1().truncate().child(self.title.clone()))
                .when_some(self.beside.clone(), |ghost, beside| {
                    ghost.child(
                        div()
                            .flex_none()
                            .text_color(rgb(theme::muted()))
                            .child(beside),
                    )
                }),
        )
    }
}

pub(crate) fn beside(rows: Span) -> Option<SharedString> {
    match rows.rows() {
        1 => None,
        held => Some(SharedString::from(format!("and {} more", held - 1))),
    }
}

pub(crate) fn marked(listed: Stateful<Div>, reached: bool) -> Stateful<Div> {
    let edge = if reached {
        rgb(theme::accent())
    } else {
        rgba(theme::UNMARKED)
    };

    listed
        .border_l_2()
        .border_color(edge)
        .when(reached, |row| row.bg(theme::tinted(theme::accent(), 0x0f)))
}

pub(crate) fn movable(
    listed: Stateful<Div>,
    onto: usize,
    carried: Carried,
    reached: bool,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let shift = carried.shift;

    marked(listed, reached)
        .on_drag(carried, |carried, under, _, cx| {
            cx.new(|_| Ghost {
                title: carried.title.clone(),
                beside: beside(carried.rows),
                under,
            })
        })
        .drag_over::<Carried>(|style, _, _, _| style.bg(rgb(theme::hover())))
        .on_drop(cx.listener(move |this, carried: &Carried, _, cx| {
            if carried.shift == shift {
                this.shift_rows(shift, carried.rows, onto, cx);
            }
        }))
}

#[derive(Clone)]
pub(crate) struct Creeping {
    scroll: UniformListScrollHandle,
    bounds: Bounds<Pixels>,
    held: usize,
}

impl Creeping {
    fn step(&self, at: Point<Pixels>) -> bool {
        let Some(toward) = toward_an_edge(at, self.bounds) else {
            return false;
        };
        let (top, bottom) = {
            let state = self.scroll.0.borrow();
            (
                state.base_handle.top_item(),
                state.base_handle.bottom_item(),
            )
        };

        match toward {
            Toward::Start => self
                .scroll
                .scroll_to_item(top.saturating_sub(1), ScrollStrategy::Top),
            Toward::End => self.scroll.scroll_to_item(
                (bottom + 1).min(self.held.saturating_sub(1)),
                ScrollStrategy::Bottom,
            ),
        }
        true
    }
}

pub(crate) fn follows_a_drag(
    listed: Stateful<Div>,
    scroll: UniformListScrollHandle,
    held: usize,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    listed.on_drag_move::<Carried>(cx.listener(
        move |this, event: &DragMoveEvent<Carried>, window, cx| {
            this.creep(
                Creeping {
                    scroll: scroll.clone(),
                    bounds: event.bounds,
                    held,
                },
                window,
                cx,
            );
        },
    ))
}

fn toward_an_edge(at: Point<Pixels>, bounds: Bounds<Pixels>) -> Option<Toward> {
    if !bounds.contains(&at) {
        return None;
    }
    let edge = px(DRAGGING_EDGE);

    if at.y < bounds.top() + edge {
        Some(Toward::Start)
    } else if at.y > bounds.bottom() - edge {
        Some(Toward::End)
    } else {
        None
    }
}

impl RootView {
    pub(crate) fn creep(&mut self, creeping: Creeping, window: &Window, cx: &mut Context<Self>) {
        let already = self.creeping.is_some();
        self.creeping = creeping.step(window.mouse_position()).then_some(creeping);

        if already || self.creeping.is_none() {
            return;
        }

        self.creeping_on = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAGGING_STEP).await;
                let carried_on = this.update_in(cx, |this, window, cx| {
                    let Some(creeping) = this.creeping.clone() else {
                        return false;
                    };
                    if !cx.has_active_drag() || !creeping.step(window.mouse_position()) {
                        this.creeping = None;
                        return false;
                    }
                    cx.notify();
                    true
                });
                if !matches!(carried_on, Ok(true)) {
                    return;
                }
            }
        });
    }

    pub(crate) fn mover(
        &self,
        id: &'static str,
        step: Step,
        row: usize,
        held: usize,
        shift: Shift,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let moving = self.acting_on(shift, row);
        let Some(to) = step.landing(moving, held) else {
            return div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(theme::row_control()))
                .child(icons::icon(
                    step.glyph(),
                    theme::row_control_icon(),
                    theme::outline(),
                ))
                .into_any_element();
        };

        kit::icon_button((id, row), step.glyph(), step.saying())
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.shift_rows(shift, moving, to, cx);
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, SharedString, point, px, size};
    use proptest::prelude::*;
    use resonate_core::Span;

    use super::{DRAGGING_EDGE, Reach, Shift, Step, Toward, beside, toward_an_edge};

    fn a_list_and_a_span() -> impl Strategy<Value = (usize, Span)> {
        (1usize..12)
            .prop_flat_map(|held| (Just(held), 0..held, 0..held))
            .prop_map(|(held, one, other)| (held, Span::between(one, other)))
    }

    fn a_span_with_a_row_beneath_it() -> impl Strategy<Value = (usize, Span)> {
        a_list_and_a_span().prop_filter("a span with a row beneath it", |(held, rows)| {
            rows.last() + 1 < *held
        })
    }

    fn moved(list: &[usize], rows: Span, to: usize) -> Vec<usize> {
        let mut moved = list.to_vec();
        let taken = moved.drain(rows.range()).collect::<Vec<_>>();
        let at = rows.landing(to);
        moved.splice(at..at, taken);
        moved
    }

    #[test]
    fn a_row_above_the_first_is_nowhere() {
        assert_eq!(Step::Above.landing(Span::one(0), 4), None);
        assert_eq!(Step::Above.landing(Span::one(3), 4), Some(2));
    }

    #[test]
    fn a_row_below_the_last_is_nowhere() {
        assert_eq!(Step::Below.landing(Span::one(3), 4), None);
        assert_eq!(Step::Below.landing(Span::one(0), 4), Some(1));
    }

    #[test]
    fn the_only_row_goes_neither_way() {
        assert_eq!(Step::Above.landing(Span::one(0), 1), None);
        assert_eq!(Step::Below.landing(Span::one(0), 1), None);
    }

    #[test]
    fn a_span_steps_off_whichever_end_it_is_moving() {
        let rows = Span::between(2, 4);

        assert_eq!(Step::Above.landing(rows, 8), Some(1));
        assert_eq!(Step::Below.landing(rows, 8), Some(5));
        assert_eq!(Step::Below.landing(Span::between(5, 7), 8), None);
        assert_eq!(Step::Above.landing(Span::between(0, 2), 8), None);
    }

    #[test]
    fn a_reach_holds_every_row_between_where_it_started_and_where_it_is() {
        let reach = Reach {
            shift: Shift::Queue,
            anchor: 5,
            row: 2,
        };

        assert_eq!(reach.rows(), Span::between(2, 5));
        assert_eq!(Reach::at(Shift::Queue, 3).rows(), Span::one(3));
    }

    #[test]
    fn a_reach_past_the_end_is_pulled_back_to_it() {
        let reach = Reach {
            shift: Shift::Queue,
            anchor: 1,
            row: 9,
        };

        assert_eq!(reach.within(4).map(Reach::rows), Some(Span::between(1, 3)));
        assert_eq!(reach.within(0), None);
    }

    #[test]
    fn a_reach_that_moves_keeps_both_of_its_ends() {
        let reach = Reach {
            shift: Shift::Queue,
            anchor: 4,
            row: 2,
        };

        assert_eq!(reach.stepped(Step::Below).rows(), Span::between(3, 5));
        assert_eq!(reach.stepped(Step::Above).rows(), Span::between(1, 3));
    }

    #[test]
    fn a_pointer_creeps_only_within_a_band_at_either_end_of_the_list() {
        let listed = Bounds::new(point(px(20.0), px(100.0)), size(px(400.0), px(300.0)));
        let edge = px(DRAGGING_EDGE);

        assert_eq!(
            toward_an_edge(point(px(200.0), listed.top() + edge / 2.0), listed),
            Some(Toward::Start)
        );
        assert_eq!(
            toward_an_edge(point(px(200.0), listed.bottom() - edge / 2.0), listed),
            Some(Toward::End)
        );
        assert_eq!(toward_an_edge(point(px(200.0), px(250.0)), listed), None);
    }

    #[test]
    fn a_pointer_taken_off_the_list_creeps_nowhere() {
        let listed = Bounds::new(point(px(20.0), px(100.0)), size(px(400.0), px(300.0)));

        assert_eq!(toward_an_edge(point(px(200.0), px(420.0)), listed), None);
        assert_eq!(toward_an_edge(point(px(200.0), px(80.0)), listed), None);
        assert_eq!(toward_an_edge(point(px(440.0), px(390.0)), listed), None);
    }

    #[test]
    fn a_ghost_says_how_many_rows_it_carries_beyond_the_first() {
        assert_eq!(beside(Span::one(2)), None);
        assert_eq!(
            beside(Span::between(2, 3)),
            Some(SharedString::new_static("and 1 more"))
        );
        assert_eq!(
            beside(Span::between(2, 5)),
            Some(SharedString::new_static("and 3 more"))
        );
    }

    proptest! {
        #[test]
        fn a_span_at_the_top_cannot_step_up_and_any_other_steps_one_row_toward_it(
            (held, rows) in a_list_and_a_span(),
        ) {
            match Step::Above.landing(rows, held) {
                None => prop_assert_eq!(rows.first(), 0),
                Some(to) => {
                    prop_assert_eq!(rows.landing(to), rows.first() - 1);
                    prop_assert_eq!(Step::Above.end(rows), rows.first());
                }
            }
        }

        #[test]
        fn a_span_at_the_bottom_cannot_step_down_and_any_other_steps_one_row_toward_it(
            (held, rows) in a_list_and_a_span(),
        ) {
            match Step::Below.landing(rows, held) {
                None => prop_assert_eq!(rows.last() + 1, held),
                Some(to) => {
                    prop_assert!(to < held);
                    prop_assert_eq!(rows.landing(to), rows.first() + 1);
                    prop_assert!(rows.landing(to) + rows.rows() <= held);
                    prop_assert_eq!(Step::Below.end(rows), rows.last());
                }
            }
        }

        #[test]
        fn a_step_down_and_the_step_back_up_leave_the_rows_where_they_were(
            (held, rows) in a_span_with_a_row_beneath_it(),
        ) {
            let list = (0..held).collect::<Vec<_>>();
            let down = Step::Below
                .landing(rows, held)
                .expect("a span with a row beneath it steps down");
            let stepped = moved(&list, rows, down);
            let shifted = Span::between(rows.first() + 1, rows.last() + 1);
            let up = Step::Above
                .landing(shifted, held)
                .expect("a span that has stepped down steps back up");

            prop_assert_ne!(&stepped, &list);
            prop_assert_eq!(moved(&stepped, shifted, up), list);
        }
    }
}
