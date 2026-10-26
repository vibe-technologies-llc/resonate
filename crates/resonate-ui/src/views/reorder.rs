use std::{sync::Arc, time::Duration};

use gpui::{
    AnyElement, Bounds, Context, Div, DragMoveEvent, IntoElement, Pixels, Point, Render,
    ScrollStrategy, SharedString, Stateful, UniformListScrollHandle, Window, div, prelude::*, px,
    rgb, rgba,
};
use resonate_core::{AlbumId, PlaylistId, Span};
use resonate_engine::Placement;
use resonate_library::{Album, Track};

use crate::{
    icons::{self, Icon},
    theme,
    views::{
        kit::{self, EndsInAnEllipsis as _},
        root::RootView,
    },
};

pub(crate) const MOVING_HINT: &str = concat!(
    "Drag a row to where it should play. ",
    keyed!("Reach a row", key!(reach_above), key!(reach_below)),
    ". ",
    keyed!("Reach more of them", key!(widen_above), key!(widen_below)),
    ". ",
    keyed!(
        "Reach further",
        key!(reach_first),
        key!(reach_last),
        key!(reach_page_above),
        key!(reach_page_below)
    ),
    ". ",
    keyed!("Reach every row", key!(reach_everything)),
    ". ",
    keyed!("Move what is reached", key!(raise_row), key!(lower_row)),
    ". ",
    keyed!("Play it", key!(play_reached)),
    ". ",
    keyed!("Take it away", key!(drop_reached)),
    ". ",
    keyed!("Open its menu", "menu", key!(open_menu)),
    ". ",
    keyed!("Reach every row between", "shift-click")
);

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
    Playlists,
    Favourites,
    Missing,
    Suggested,
    Offered,
    Heard,
    Top,
}

impl Shift {
    pub(crate) const fn is_edited(self) -> bool {
        matches!(self, Self::Queue | Self::Playlist(_))
    }

    pub(crate) const fn chooses_apart(self) -> bool {
        matches!(
            self,
            Self::Listing(
                Listed::Tracks
                    | Listed::Albums
                    | Listed::Artists
                    | Listed::Playlists
                    | Listed::Favourites
                    | Listed::Suggested
            )
        )
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
            Self::Above => keyed!("Play what is reached one place sooner", key!(raise_row)),
            Self::Below => keyed!("Play what is reached one place later", key!(lower_row)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Chosen(Vec<Span>);

impl Chosen {
    pub(crate) fn of(rows: Span) -> Self {
        Self(vec![rows])
    }

    pub(crate) fn count(&self) -> usize {
        self.0.iter().map(|span| span.rows()).sum()
    }

    pub(crate) fn holds(&self, row: usize) -> bool {
        self.0.iter().any(|span| span.holds(row))
    }

    pub(crate) fn first(&self) -> Option<usize> {
        self.0.first().map(|span| span.first())
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.0.iter().flat_map(|span| span.range())
    }

    pub(crate) fn united(mut self, rows: Span) -> Self {
        self.0.push(rows);
        self.0.sort_by_key(|span| span.first());

        let mut merged: Vec<Span> = Vec::with_capacity(self.0.len());
        for span in self.0 {
            match merged.last_mut() {
                Some(last) if span.first() <= last.last().saturating_add(1) => {
                    *last = Span::between(last.first(), last.last().max(span.last()));
                }
                _ => merged.push(span),
            }
        }

        Self(merged)
    }

    pub(crate) fn without(self, row: usize) -> Self {
        let mut left = Vec::with_capacity(self.0.len() + 1);
        for span in self.0 {
            if !span.holds(row) {
                left.push(span);
                continue;
            }
            if let Some(before) = row.checked_sub(1).filter(|before| *before >= span.first()) {
                left.push(Span::between(span.first(), before));
            }
            if row < span.last() {
                left.push(Span::between(row + 1, span.last()));
            }
        }

        Self(left)
    }

    fn within(self, held: usize) -> Self {
        let Some(last) = held.checked_sub(1) else {
            return Self::default();
        };

        Self(
            self.0
                .into_iter()
                .filter(|span| span.first() <= last)
                .map(|span| Span::between(span.first(), span.last().min(last)))
                .collect(),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Reach {
    pub(crate) shift: Shift,
    pub(crate) anchor: usize,
    pub(crate) row: usize,
    chosen: bool,
    apart: Chosen,
}

impl Reach {
    pub(crate) fn at(shift: Shift, row: usize) -> Self {
        Self::spanning(shift, row, row)
    }

    pub(crate) fn spanning(shift: Shift, anchor: usize, row: usize) -> Self {
        Self {
            shift,
            anchor,
            row,
            chosen: true,
            apart: Chosen::default(),
        }
    }

    pub(crate) fn widened_to(&self, row: usize) -> Self {
        Self {
            shift: self.shift,
            anchor: self.anchor,
            row,
            chosen: true,
            apart: self.apart.clone(),
        }
    }

    pub(crate) fn rows(&self) -> Span {
        Span::between(self.anchor, self.row)
    }

    pub(crate) fn selected(&self) -> Chosen {
        if self.chosen {
            self.apart.clone().united(self.rows())
        } else {
            self.apart.clone()
        }
    }

    pub(crate) fn reaches(&self, row: usize) -> bool {
        (self.chosen && self.rows().holds(row)) || self.apart.holds(row)
    }

    pub(crate) fn is_chosen(&self) -> bool {
        self.chosen
    }

    pub(crate) fn toggled(&self, row: usize) -> Self {
        let selected = self.selected();
        if selected.holds(row) {
            return Self {
                shift: self.shift,
                anchor: row,
                row,
                chosen: false,
                apart: selected.without(row),
            };
        }

        Self {
            shift: self.shift,
            anchor: row,
            row,
            chosen: true,
            apart: selected,
        }
    }

    pub(crate) fn hopped_to(&self, row: usize) -> Self {
        let selected = self.selected();
        let chosen = selected.holds(row);

        Self {
            shift: self.shift,
            anchor: row,
            row,
            chosen,
            apart: if chosen {
                selected.without(row)
            } else {
                selected
            },
        }
    }

    pub(crate) fn within(&self, held: usize) -> Option<Self> {
        let last = held.checked_sub(1)?;

        Some(Self {
            shift: self.shift,
            anchor: self.anchor.min(last),
            row: self.row.min(last),
            chosen: self.chosen,
            apart: self.apart.clone().within(held),
        })
    }

    pub(crate) fn after_dropping(shift: Shift, rows: Span, held: usize) -> Option<Self> {
        let left = held.checked_sub(rows.rows())?;
        let last = left.checked_sub(1)?;
        let first = rows.first();

        Some(Self::at(shift, if first < last { first } else { last }))
    }

    pub(crate) fn stepped(&self, step: Step) -> Self {
        let (anchor, row) = match step {
            Step::Above => (self.anchor.saturating_sub(1), self.row.saturating_sub(1)),
            Step::Below => (self.anchor + 1, self.row + 1),
        };

        Self::spanning(self.shift, anchor, row)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Carried {
    pub(crate) shift: Shift,
    pub(crate) rows: Span,
    pub(crate) title: SharedString,
}

#[derive(Clone, Debug)]
pub(crate) enum Lift {
    Tracks(Run),
    Album(AlbumId, SharedString),
    Albums(AlbumRun),
}

#[derive(Clone, Debug)]
pub(crate) struct Run {
    page: Arc<[Track]>,
    rows: Chosen,
}

impl Run {
    pub(crate) fn tracks(&self) -> Vec<Track> {
        self.rows
            .rows()
            .filter_map(|row| self.page.get(row).cloned())
            .collect()
    }

    fn title(&self) -> SharedString {
        let first = self
            .rows
            .first()
            .and_then(|row| self.page.get(row))
            .map(|track| track.title.as_str());

        titled_with_more(first, self.rows.count())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct AlbumRun {
    page: Arc<[Album]>,
    rows: Chosen,
}

impl AlbumRun {
    pub(crate) fn albums(&self) -> Vec<AlbumId> {
        self.rows
            .rows()
            .filter_map(|row| self.page.get(row).map(|album| album.id))
            .collect()
    }

    fn title(&self) -> SharedString {
        let first = self
            .rows
            .first()
            .and_then(|row| self.page.get(row))
            .map(|album| album.title.as_str());

        titled_with_more(first, self.rows.count())
    }
}

fn titled_with_more(first: Option<&str>, held: usize) -> SharedString {
    match (first, held) {
        (Some(title), 0 | 1) => title.to_owned().into(),
        (Some(title), held) => format!("{title} and {} more", held - 1).into(),
        (None, _) => SharedString::default(),
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Lifted {
    pub(crate) lift: Lift,
}

impl Lifted {
    pub(crate) fn tracks(page: &Arc<[Track]>, rows: Chosen) -> Self {
        Self {
            lift: Lift::Tracks(Run {
                page: Arc::clone(page),
                rows,
            }),
        }
    }

    pub(crate) fn album(album: AlbumId, title: &str) -> Self {
        Self {
            lift: Lift::Album(album, SharedString::from(title.to_owned())),
        }
    }

    pub(crate) fn albums(page: &Arc<[Album]>, rows: Chosen) -> Self {
        Self {
            lift: Lift::Albums(AlbumRun {
                page: Arc::clone(page),
                rows,
            }),
        }
    }

    pub(crate) fn title(&self) -> SharedString {
        match &self.lift {
            Lift::Tracks(run) => run.title(),
            Lift::Album(_, title) => title.clone(),
            Lift::Albums(run) => run.title(),
        }
    }
}

pub(crate) fn liftable(listed: Stateful<Div>, lifted: Lifted) -> Stateful<Div> {
    listed.on_drag(lifted, |lifted, under, _, cx| {
        cx.new(|_| Ghost {
            title: lifted.title(),
            beside: None,
            under,
        })
    })
}

pub(crate) fn takes_a_lift(
    target: Stateful<Div>,
    into: LiftedTo,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    target
        .drag_over::<Lifted>(|style, _, _, _| style.bg(rgb(theme::hover())))
        .on_drop(cx.listener(move |this, lifted: &Lifted, _, cx| {
            this.lifted_into(lifted, into, cx);
        }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LiftedTo {
    Queue(Placement),
    Playlist(PlaylistId),
    PlaylistAt(PlaylistId, usize),
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
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(self.title.clone()),
                )
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

pub(crate) fn marked<E: Styled + FluentBuilder>(listed: E, reached: bool) -> E {
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

pub(crate) fn cursor_marked<E: Styled + FluentBuilder>(listed: E, cursor_only: bool) -> E {
    listed.when(cursor_only, |row| {
        row.border_color(theme::tinted(theme::accent(), CURSOR_ALPHA))
    })
}

pub(crate) const CURSOR_ALPHA: u8 = 0x80;

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
        .when_some(lifted_onto(shift, onto), |row, into| {
            takes_a_lift(row, into, cx)
        })
}

const fn lifted_onto(shift: Shift, onto: usize) -> Option<LiftedTo> {
    match shift {
        Shift::Queue => Some(LiftedTo::Queue(Placement::At(onto))),
        Shift::Playlist(playlist) => Some(LiftedTo::PlaylistAt(playlist, onto)),
        Shift::Listing(_) => None,
    }
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
    use std::{sync::Arc, time::SystemTime};

    use gpui::{Bounds, SharedString, point, px, size};
    use proptest::prelude::*;
    use resonate_core::{
        ChannelLayout, MediaLocation, SampleFormat, SampleRate, Span, StreamSpec, TrackId,
    };
    use resonate_library::{Codec, Track};

    use super::{
        Chosen, DRAGGING_EDGE, Lift, Lifted, Listed, Reach, Shift, Step, Toward, beside,
        toward_an_edge,
    };

    fn song(id: u64) -> Track {
        Track {
            id: TrackId::new(id).expect("a valid id"),
            location: MediaLocation::local(format!("/music/{id}.flac")),
            title: format!("Song {id}"),
            artist: None,
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: None,
            spec: StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            codec: Codec::Flac,
            replay_gain: Default::default(),
            file_size: 0,
            modified: SystemTime::UNIX_EPOCH,
            added: SystemTime::UNIX_EPOCH,
            plays: 0,
            played: None,
            span: None,
            favourite: None,
            genre: None,
            hidden: false,
            alternatives: 0,
            delivered: false,
        }
    }

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
    fn a_lifted_run_shares_the_page_it_was_lifted_from_and_is_named_by_its_first_song() {
        let page: Arc<[Track]> = (1..=4).map(song).collect();

        let lifted = Lifted::tracks(&page, Chosen::of(Span::between(1, 3)));
        let Lift::Tracks(run) = &lifted.lift else {
            unreachable!("tracks were lifted");
        };

        assert_eq!(Arc::strong_count(&page), 2);
        assert_eq!(run.tracks().len(), 3);
        assert_eq!(run.tracks()[0].title, "Song 2");
        assert_eq!(lifted.title(), "Song 2 and 2 more");
        assert_eq!(
            Lifted::tracks(&page, Chosen::of(Span::one(0))).title(),
            "Song 1"
        );
    }

    #[test]
    fn a_lift_of_rows_chosen_apart_takes_those_rows_in_order_and_names_the_first() {
        let page: Arc<[Track]> = (1..=6).map(song).collect();
        let rows = Chosen::of(Span::one(4))
            .united(Span::one(1))
            .united(Span::one(2));

        let lifted = Lifted::tracks(&page, rows);
        let Lift::Tracks(run) = &lifted.lift else {
            unreachable!("tracks were lifted");
        };

        let lifted_tracks = run.tracks();
        let titles: Vec<&str> = lifted_tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect();
        assert_eq!(titles, ["Song 2", "Song 3", "Song 5"]);
        assert_eq!(lifted.title(), "Song 2 and 2 more");
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
        let reach = Reach::spanning(Shift::Queue, 5, 2);

        assert_eq!(reach.rows(), Span::between(2, 5));
        assert_eq!(Reach::at(Shift::Queue, 3).rows(), Span::one(3));
    }

    fn chosen_of(reach: &Reach) -> Vec<usize> {
        reach.selected().rows().collect()
    }

    #[test]
    fn rows_chosen_apart_are_kept_in_order_and_merged_where_they_touch() {
        let chosen = Chosen::of(Span::between(5, 6))
            .united(Span::between(2, 3))
            .united(Span::one(4))
            .united(Span::one(9));

        assert_eq!(chosen.rows().collect::<Vec<_>>(), [2, 3, 4, 5, 6, 9]);
        assert_eq!(chosen.count(), 6);
        assert_eq!(chosen.first(), Some(2));
        assert!(chosen.holds(4) && chosen.holds(9) && !chosen.holds(7));
    }

    #[test]
    fn a_row_taken_out_of_the_chosen_leaves_a_gap_and_nothing_else() {
        let chosen = Chosen::of(Span::between(2, 6));

        assert_eq!(
            chosen.clone().without(4).rows().collect::<Vec<_>>(),
            [2, 3, 5, 6]
        );
        assert_eq!(
            chosen.clone().without(2).rows().collect::<Vec<_>>(),
            [3, 4, 5, 6]
        );
        assert_eq!(
            chosen.clone().without(6).rows().collect::<Vec<_>>(),
            [2, 3, 4, 5]
        );
        assert_eq!(chosen.clone().without(0), chosen);
        assert_eq!(Chosen::of(Span::one(3)).without(3).count(), 0);
        assert_eq!(Chosen::of(Span::one(0)).without(0).count(), 0);
    }

    #[test]
    fn a_control_press_adds_a_row_beside_the_ones_already_chosen() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 3).toggled(7);

        assert_eq!(chosen_of(&reach), [3, 7]);
        assert_eq!((reach.anchor, reach.row), (7, 7));
        assert!(reach.is_chosen());
        assert!(reach.reaches(3) && reach.reaches(7) && !reach.reaches(5));
    }

    #[test]
    fn a_control_press_on_a_chosen_row_lets_it_go_and_keeps_the_others() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 3)
            .toggled(7)
            .toggled(3);

        assert_eq!(chosen_of(&reach), [7]);
        assert_eq!(reach.row, 3);
        assert!(!reach.is_chosen());
        assert!(!reach.reaches(3) && reach.reaches(7));
    }

    #[test]
    fn a_row_let_go_from_the_middle_of_a_run_splits_the_run() {
        let reach = Reach::spanning(Shift::Listing(Listed::Tracks), 2, 6).toggled(4);

        assert_eq!(chosen_of(&reach), [2, 3, 5, 6]);
    }

    #[test]
    fn the_only_row_chosen_can_be_let_go_and_the_cursor_stays_on_it() {
        let reach = Reach::at(Shift::Listing(Listed::Albums), 5).toggled(5);

        assert_eq!(reach.selected().count(), 0);
        assert_eq!(reach.row, 5);
        assert_eq!(reach.toggled(5).selected().count(), 1);
    }

    #[test]
    fn a_hop_moves_the_cursor_without_touching_what_is_chosen() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 3).toggled(7);
        let hopped = reach.hopped_to(5);

        assert_eq!(chosen_of(&hopped), [3, 7]);
        assert_eq!(hopped.row, 5);
        assert!(!hopped.is_chosen());
        assert_eq!(chosen_of(&hopped.toggled(5)), [3, 5, 7]);
        assert!(reach.hopped_to(3).is_chosen());
        assert_eq!(chosen_of(&reach.hopped_to(3)), [3, 7]);
    }

    #[test]
    fn widening_a_reach_keeps_the_rows_chosen_apart() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 1)
            .toggled(8)
            .widened_to(10);

        assert_eq!(chosen_of(&reach), [1, 8, 9, 10]);
    }

    #[test]
    fn a_plain_reach_chooses_nothing_apart() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 1).toggled(8);

        assert_eq!(chosen_of(&reach.stepped(Step::Below)), [9]);
        assert_eq!(chosen_of(&Reach::at(reach.shift, 4)), [4]);
    }

    #[test]
    fn rows_chosen_apart_past_the_end_are_pulled_in_or_dropped() {
        let reach = Reach::at(Shift::Listing(Listed::Tracks), 1)
            .toggled(5)
            .toggled(9);

        let within = reach.within(6).expect("a list of six rows");

        assert_eq!(chosen_of(&within), [1, 5]);
        assert_eq!(within.row, 5);
        assert!(reach.within(0).is_none());
    }

    #[test]
    fn only_the_catalog_listings_choose_rows_apart() {
        for listed in [
            Listed::Tracks,
            Listed::Albums,
            Listed::Artists,
            Listed::Playlists,
            Listed::Favourites,
            Listed::Suggested,
        ] {
            assert!(Shift::Listing(listed).chooses_apart(), "{listed:?}");
        }
        for listed in [Listed::Missing, Listed::Offered, Listed::Heard, Listed::Top] {
            assert!(!Shift::Listing(listed).chooses_apart(), "{listed:?}");
        }
        assert!(!Shift::Queue.chooses_apart());
    }

    proptest! {
        #[test]
        fn control_presses_choose_exactly_the_rows_pressed_an_odd_number_of_times(
            presses in proptest::collection::vec(0usize..20, 1..40),
        ) {
            let mut reach = Reach::at(Shift::Listing(Listed::Tracks), presses[0]);
            let mut model = std::collections::BTreeSet::from([presses[0]]);

            for row in &presses[1..] {
                reach = reach.toggled(*row);
                if !model.remove(row) {
                    model.insert(*row);
                }
            }

            prop_assert_eq!(chosen_of(&reach), model.into_iter().collect::<Vec<_>>());
        }
    }

    #[test]
    fn dropping_the_last_rows_leaves_the_reach_on_the_row_that_is_now_last() {
        assert_eq!(
            Reach::after_dropping(Shift::Queue, Span::between(3, 4), 5),
            Some(Reach::at(Shift::Queue, 2))
        );
        assert_eq!(
            Reach::after_dropping(Shift::Queue, Span::one(4), 5),
            Some(Reach::at(Shift::Queue, 3))
        );
    }

    #[test]
    fn dropping_rows_from_the_middle_reaches_the_row_that_moved_up_into_their_place() {
        assert_eq!(
            Reach::after_dropping(Shift::Queue, Span::between(1, 2), 5),
            Some(Reach::at(Shift::Queue, 1))
        );
    }

    #[test]
    fn dropping_every_row_leaves_nothing_reached() {
        assert_eq!(
            Reach::after_dropping(Shift::Queue, Span::between(0, 4), 5),
            None
        );
        assert_eq!(Reach::after_dropping(Shift::Queue, Span::one(0), 0), None);
    }

    proptest! {
        #[test]
        fn a_reach_left_after_dropping_is_always_a_row_that_remains(
            (held, rows) in a_list_and_a_span(),
        ) {
            let left = held - rows.rows();

            match Reach::after_dropping(Shift::Queue, rows, held) {
                Some(reach) => prop_assert!(reach.row < left && reach.anchor == reach.row),
                None => prop_assert_eq!(left, 0),
            }
        }
    }

    #[test]
    fn a_reach_past_the_end_is_pulled_back_to_it() {
        let reach = Reach::spanning(Shift::Queue, 1, 9);

        assert_eq!(
            reach.within(4).map(|within| within.rows()),
            Some(Span::between(1, 3))
        );
        assert_eq!(reach.within(0), None);
    }

    #[test]
    fn a_reach_that_moves_keeps_both_of_its_ends() {
        let reach = Reach::spanning(Shift::Queue, 4, 2);

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
