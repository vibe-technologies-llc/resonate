use std::{
    cell::Cell,
    collections::BTreeSet,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyElement, App, Bounds, ClickEvent, Context, Div, ElementId, Global, MouseButton,
    MouseDownEvent, Pixels, Point, Role, SharedString, Size, Stateful, Task, Window, anchored,
    canvas, deferred, div, point, prelude::*, px, rgb,
};
use resonate_core::{AlbumId, ArtistId, FrameSpan, MediaLocation, TrackId};
use resonate_engine::Placement;
use resonate_library::{Favoured, PlaylistEntry, Track};

use crate::{
    Selection, clipboard, format,
    icons::{self, Icon},
    motion, theme,
    views::{
        better,
        kit::{self, EndsInAnEllipsis as _},
        playlists::Held,
        reorder::Shift,
        root::RootView,
        spoken::{self, Placed, Spoken},
    },
    words,
};

const MENU_GROUP: &str = "menu-entry";
const OPENED_IN_FROM_THE_ROWS_EDGE: f32 = 24.0;
const BESIDE_ITS_ROW: f32 = 4.0;
const PANEL_PADDING: f32 = 4.0;
const STAYS_OPEN_FOR_THE_POINTER_CROSSING: Duration = Duration::from_millis(250);

type Doing = dyn Fn(&mut RootView, &mut Window, &mut Context<RootView>);

type Offers = dyn Fn(&mut RootView, Point<Pixels>, &mut Context<RootView>) -> Menu;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtTheReach {
    pub(crate) shift: Shift,
    pub(crate) row: usize,
}

struct ReachedMenu {
    reach: AtTheReach,
    drawn: Bounds<Pixels>,
    offers: Rc<Offers>,
}

impl Global for ReachedMenu {}

#[derive(Default, PartialEq, Eq)]
pub(crate) struct Opened(pub(crate) Option<ElementId>);

impl Global for Opened {}

pub(crate) fn lit_by_its_menu(row: impl Into<ElementId>, cx: &App) -> bool {
    let row = row.into();
    cx.try_global::<Opened>()
        .is_some_and(|opened| opened.0.as_ref() == Some(&row))
}

#[derive(Clone)]
pub(crate) struct Menu {
    at: Point<Pixels>,
    entries: Vec<Entry>,
    reached: Option<usize>,
    beneath: Option<Beneath>,
    owner: Option<ElementId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Beneath {
    at: usize,
    reached: Option<usize>,
}

#[derive(Clone)]
struct Entry {
    icon: Option<Icon>,
    label: SharedString,
    key: Option<SharedString>,
    chosen: Option<bool>,
    takes: Takes,
}

#[derive(Clone)]
enum Takes {
    Nothing,
    Press(Rc<Doing>),
    Opens {
        entries: Rc<[Entry]>,
        drawn: Rc<Cell<Bounds<Pixels>>>,
    },
}

impl Entry {
    const fn is_apart(&self) -> bool {
        matches!(self.takes, Takes::Nothing)
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Offered {
    Here(usize),
    Beneath(usize, usize),
}

fn stepped(entries: &[Entry], reached: Option<usize>, step: isize) -> Option<usize> {
    let taking: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| !entry.is_apart())
        .map(|(at, _)| at)
        .collect();
    let last = taking.len().checked_sub(1)?;
    let at = reached.and_then(|reached| taking.iter().position(|entry| *entry == reached));
    let next = match (at, step) {
        (None, 1) => 0,
        (None, _) => last,
        (Some(at), 1) if at == last => 0,
        (Some(at), 1) => at + 1,
        (Some(0), _) => last,
        (Some(at), _) => at - 1,
    };

    taking.get(next).copied()
}

impl Menu {
    #[cfg(test)]
    pub(crate) fn labels(&self) -> Vec<String> {
        self.entries
            .iter()
            .flat_map(|entry| match &entry.takes {
                Takes::Opens { entries, .. } => entries
                    .iter()
                    .map(|under| under.label.to_string())
                    .collect(),
                _ => vec![entry.label.to_string()],
            })
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn offering(&self, label: &str) -> Option<Offered> {
        self.entries.iter().enumerate().find_map(|(at, entry)| {
            if let Takes::Opens { entries, .. } = &entry.takes {
                return entries
                    .iter()
                    .position(|under| under.label == label)
                    .map(|under| Offered::Beneath(at, under));
            }

            (entry.label == label).then_some(Offered::Here(at))
        })
    }

    pub(crate) fn at(at: Point<Pixels>) -> Self {
        Self {
            at,
            entries: Vec::new(),
            reached: None,
            beneath: None,
            owner: None,
        }
    }

    pub(crate) fn opened_on(mut self, owner: Option<ElementId>) -> Self {
        self.owner = owner;
        self
    }

    pub(crate) fn owner(&self) -> Option<&ElementId> {
        self.owner.as_ref()
    }

    pub(crate) fn does(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        does: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        self.entries.push(Entry {
            icon: Some(icon),
            label: label.into(),
            key: None,
            chosen: None,
            takes: Takes::Press(Rc::new(does)),
        });
        self
    }

    pub(crate) fn chooses(
        mut self,
        label: impl Into<SharedString>,
        chosen: bool,
        does: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        self.entries.push(Entry {
            icon: chosen.then_some(Icon::Check),
            label: label.into(),
            key: None,
            chosen: Some(chosen),
            takes: Takes::Press(Rc::new(does)),
        });
        self
    }

    pub(crate) fn nests(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        fill: impl FnOnce(Self) -> Self,
    ) -> Self {
        let filled = fill(Self::at(self.at)).without_a_last_line();
        if filled.offers_anything() {
            self.entries.push(Entry {
                icon: Some(icon),
                label: label.into(),
                key: None,
                chosen: None,
                takes: Takes::Opens {
                    entries: Rc::from(filled.entries),
                    drawn: Rc::default(),
                },
            });
        }
        self
    }

    pub(crate) fn under(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        key: &'static str,
        does: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        self = self.does(icon, label, does);
        if let Some(last) = self.entries.last_mut() {
            last.key = Some(SharedString::new_static(key));
        }
        self
    }

    pub(crate) fn when_some<T>(self, held: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self {
        match held {
            Some(held) => then(self, held),
            None => self,
        }
    }

    pub(crate) fn apart(mut self) -> Self {
        if self.entries.last().is_some_and(|last| !last.is_apart()) {
            self.entries.push(Entry {
                icon: None,
                label: SharedString::new_static(""),
                key: None,
                chosen: None,
                takes: Takes::Nothing,
            });
        }
        self
    }

    fn without_a_last_line(mut self) -> Self {
        while self.entries.last().is_some_and(Entry::is_apart) {
            self.entries.pop();
        }
        self
    }

    const fn is_reached(&self) -> bool {
        self.reached.is_some()
    }

    pub(crate) fn offers_anything(&self) -> bool {
        self.entries.iter().any(|entry| !entry.is_apart())
    }

    fn entries_beneath(&self, at: usize) -> Option<&Rc<[Entry]>> {
        match &self.entries.get(at)?.takes {
            Takes::Opens { entries, .. } => Some(entries),
            _ => None,
        }
    }

    fn opens_beneath(&mut self, at: usize) -> bool {
        if self.entries_beneath(at).is_none() {
            return false;
        }
        if self.beneath.map(|beneath| beneath.at) != Some(at) {
            self.beneath = Some(Beneath { at, reached: None });
        }
        true
    }

    fn steps(&mut self, step: isize) {
        if let Some(Beneath {
            at,
            reached: Some(reached),
        }) = self.beneath
        {
            let moved = self
                .entries_beneath(at)
                .and_then(|entries| stepped(entries, Some(reached), step));
            self.beneath = Some(Beneath { at, reached: moved });
            return;
        }
        self.beneath = None;
        self.reached = stepped(&self.entries, self.reached, step);
    }

    fn steps_in(&mut self) -> bool {
        if self
            .beneath
            .is_some_and(|beneath| beneath.reached.is_some())
        {
            return false;
        }
        let Some(at) = self.reached else {
            return false;
        };
        if !self.opens_beneath(at) {
            return false;
        }
        let first = self
            .entries_beneath(at)
            .and_then(|entries| stepped(entries, None, 1));
        self.beneath = Some(Beneath { at, reached: first });
        true
    }

    fn steps_out(&mut self) -> bool {
        let Some(beneath) = self.beneath.take() else {
            return false;
        };
        self.reached = Some(beneath.at);
        true
    }

    fn reached(&self) -> Option<Rc<Doing>> {
        let entry = match self.beneath {
            Some(Beneath {
                at,
                reached: Some(reached),
            }) => self.entries_beneath(at)?.get(reached)?,
            _ => self.entries.get(self.reached?)?,
        };

        match &entry.takes {
            Takes::Press(does) => Some(Rc::clone(does)),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handing {
    ToTheMenu,
    BackToTheWindow,
    Stays,
}

impl Handing {
    const fn of(listening: bool, menu: Option<&Menu>, held: bool) -> Self {
        match (menu, held) {
            (Some(menu), false) if listening && menu.is_reached() => Self::ToTheMenu,
            (None, true) => Self::BackToTheWindow,
            _ => Self::Stays,
        }
    }
}

pub(crate) struct Called {
    pub(crate) title: SharedString,
    pub(crate) artist: SharedString,
    pub(crate) album: Option<SharedString>,
}

impl RootView {
    pub(crate) fn album_named(
        &self,
        album: Option<AlbumId>,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        cx: &App,
    ) -> Option<SharedString> {
        album
            .and_then(|id| {
                self.library
                    .read(cx)
                    .album_of(id)
                    .map(|held| SharedString::from(held.title.clone()))
            })
            .or_else(|| {
                self.player
                    .read(cx)
                    .media(location, span)
                    .and_then(|info| info.tags.album.clone())
                    .map(SharedString::from)
            })
    }

    pub(crate) fn hand_the_reader_the_menu(&self, window: &mut Window, cx: &mut Context<Self>) {
        let held = self.menu_focus.is_focused(window);

        match Handing::of(spoken::listening(), self.menu.as_ref(), held) {
            Handing::ToTheMenu => window.focus(&self.menu_focus, cx),
            Handing::BackToTheWindow => window.focus(&self.focus, cx),
            Handing::Stays => {}
        }
    }

    pub(crate) fn open_a_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if !menu.offers_anything() {
            return;
        }
        self.naming = None;
        self.adding = None;
        self.record = None;
        self.menu = Some(menu.without_a_last_line());
        cx.notify();
    }

    pub(crate) fn close_the_menu(&mut self, cx: &mut Context<Self>) -> bool {
        let opened = self.menu.take().is_some();
        if opened {
            self.beneath_closes = Task::ready(());
            cx.notify();
        }
        opened
    }

    pub(crate) fn back_out_of_the_menu(&mut self, cx: &mut Context<Self>) -> bool {
        let stepped_out = self.menu.as_mut().is_some_and(Menu::steps_out);
        if stepped_out {
            cx.notify();
        }
        stepped_out
    }

    pub(crate) fn step_into_the_menu(&mut self, cx: &mut Context<Self>) -> bool {
        let stepped_in = self.menu.as_mut().is_some_and(Menu::steps_in);
        if stepped_in {
            cx.notify();
        }
        stepped_in
    }

    fn opens_beneath_the_menu(&mut self, at: usize, cx: &mut Context<Self>) {
        self.beneath_closes = Task::ready(());
        if self
            .menu
            .as_mut()
            .is_some_and(|menu| menu.opens_beneath(at))
        {
            cx.notify();
        }
    }

    fn keeps_what_is_beneath(&mut self) {
        self.beneath_closes = Task::ready(());
    }

    fn closes_beneath_the_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.as_ref().is_none_or(|menu| menu.beneath.is_none()) {
            return;
        }
        self.beneath_closes = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(STAYS_OPEN_FOR_THE_POINTER_CROSSING)
                .await;
            let closed = this.update(cx, |this, cx| {
                if let Some(menu) = this.menu.as_mut() {
                    menu.beneath = None;
                    cx.notify();
                }
            });
            let _ = closed;
        });
    }

    pub(crate) fn step_the_menu(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        menu.steps(step);
        cx.notify();
        true
    }

    pub(crate) fn press_the_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.step_into_the_menu(cx) {
            return true;
        }
        let Some(menu) = self.menu.as_ref() else {
            return false;
        };
        let Some(does) = menu.reached() else {
            return self.close_the_menu(cx);
        };
        self.menu = None;
        does(self, window, cx);
        cx.notify();
        true
    }

    pub(crate) fn menu_over_the_app(
        &self,
        room: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.menu.clone()?;
        let entries = menu
            .entries
            .iter()
            .filter(|entry| !entry.is_apart())
            .count();
        let mut panel = menu_panel("menu").track_focus(&self.menu_focus);

        let mut offered = 0;
        for (at, entry) in menu.entries.iter().enumerate() {
            panel = panel.child(if entry.is_apart() {
                apart().into_any_element()
            } else {
                offered += 1;
                self.menu_entry(
                    Row::Of { at },
                    entry,
                    menu.reached == Some(at),
                    Placed {
                        at: offered - 1,
                        of: entries,
                    },
                    cx,
                )
            });
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.close_the_menu(cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.close_the_menu(cx);
                    }),
                )
                .child(
                    deferred(
                        anchored()
                            .position(menu.at)
                            .snap_to_window_with_margin(px(8.0))
                            .child(motion::lifted_in(panel, "menu-arrives")),
                    )
                    .with_priority(1),
                )
                .children(self.panel_beneath(&menu, room, cx))
                .into_any_element(),
        )
    }

    fn panel_beneath(
        &self,
        menu: &Menu,
        room: Size<Pixels>,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let Beneath { at, reached } = menu.beneath?;
        let Takes::Opens { entries, drawn } = &menu.entries.get(at)?.takes else {
            return None;
        };
        let offered = entries.iter().filter(|entry| !entry.is_apart()).count();
        let mut panel =
            menu_panel("menu-beneath").on_hover(cx.listener(|this, hovered: &bool, _, _| {
                if *hovered {
                    this.keeps_what_is_beneath();
                }
            }));

        let mut placed = 0;
        for (under, entry) in entries.iter().enumerate() {
            panel = panel.child(if entry.is_apart() {
                apart().into_any_element()
            } else {
                placed += 1;
                self.menu_entry(
                    Row::Beneath { at, under },
                    entry,
                    reached == Some(under),
                    Placed {
                        at: placed - 1,
                        of: offered,
                    },
                    cx,
                )
            });
        }

        Some(
            deferred(
                anchored()
                    .position(beside_its_row(drawn.get(), room))
                    .snap_to_window_with_margin(px(8.0))
                    .child(motion::lifted_in(panel, "menu-beneath-arrives")),
            )
            .with_priority(2)
            .into_any_element(),
        )
    }

    fn menu_entry(
        &self,
        row: Row,
        entry: &Entry,
        reached: bool,
        placed: Placed,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selector = row.selector();

        div()
            .id(row.id())
            .debug_selector(move || selector.clone())
            .named_as(
                Role::MenuItem,
                &entry.label,
                if matches!(entry.takes, Takes::Opens { .. }) {
                    words::menu::open_beneath()
                } else {
                    ""
                },
            )
            .keyed(entry.key.clone())
            .placed(placed)
            .chosen(reached)
            .focused_while(reached)
            .group(MENU_GROUP)
            .relative()
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::menu_row()))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .when(reached, |row| {
                row.bg(rgb(theme::hover())).text_color(rgb(theme::text()))
            })
            .hover(|row| row.bg(rgb(theme::hover())).text_color(rgb(theme::text())))
            .when_some(entry.icon, |row, icon| {
                row.child(icons::lit_on_hover(
                    icons::icon(icon, theme::menu_icon(), theme::faint()),
                    MENU_GROUP,
                ))
            })
            .when(entry.chosen == Some(false), |row| {
                row.child(div().flex_none().size(px(theme::menu_icon())))
            })
            .when_some(entry.chosen, |row, chosen| row.thrown(chosen))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(entry.label.clone()),
            )
            .when_some(entry.key.clone(), |row, key| {
                row.child(kit::figure(key).text_color(rgb(theme::faint())))
            })
            .map(|element| match (&entry.takes, row) {
                (Takes::Opens { drawn, .. }, Row::Of { at }) => {
                    let drawn = Rc::clone(drawn);

                    element
                        .child(icons::icon(
                            Icon::ChevronRight,
                            theme::menu_icon(),
                            theme::faint(),
                        ))
                        .child(
                            canvas(move |bounds, _, _| drawn.set(bounds), |_, _, _, _| {})
                                .absolute()
                                .inset_0(),
                        )
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered {
                                this.opens_beneath_the_menu(at, cx);
                            }
                        }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.opens_beneath_the_menu(at, cx);
                        }))
                }
                (Takes::Press(does), row) => {
                    let pressed = Rc::clone(does);

                    element
                        .when(matches!(row, Row::Of { .. }), |element| {
                            element.on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if *hovered {
                                    this.closes_beneath_the_menu(cx);
                                }
                            }))
                        })
                        .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                            this.menu = None;
                            pressed(this, window, cx);
                            cx.notify();
                        }))
                }
                _ => element,
            })
            .into_any_element()
    }
}

#[derive(Clone, Copy)]
enum Row {
    Of { at: usize },
    Beneath { at: usize, under: usize },
}

impl Row {
    fn id(self) -> ElementId {
        match self {
            Self::Of { at } => ("menu-entry", at).into(),
            Self::Beneath { at, under } => {
                ElementId::NamedInteger(format!("menu-beneath-{at}").into(), under as u64)
            }
        }
    }

    fn selector(self) -> String {
        match self {
            Self::Of { at } => format!("menu-entry-{at}"),
            Self::Beneath { at, under } => format!("menu-beneath-{at}-{under}"),
        }
    }
}

fn beside_its_row(row: Bounds<Pixels>, room: Size<Pixels>) -> Point<Pixels> {
    let wide = px(theme::menu_width());
    let gap = px(BESIDE_ITS_ROW);
    let x = if row.right() + gap + wide <= room.width {
        row.right() + gap
    } else {
        row.left() - gap - wide
    };

    point(x, row.top() - px(PANEL_PADDING))
}

fn menu_panel(id: &'static str) -> Stateful<Div> {
    kit::floating()
        .id(id)
        .spoken_as(Role::Menu)
        .w(theme::width(theme::menu_width()))
}

fn apart() -> Div {
    kit::rule().my_1().mx_2()
}

pub(crate) fn opens_a_reachable_menu(
    element: Stateful<Div>,
    reach: Option<AtTheReach>,
    offers: impl Fn(&mut RootView, Point<Pixels>, &mut Context<RootView>) -> Menu + 'static,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let offers: Rc<Offers> = Rc::new(offers);
    let pressed = Rc::clone(&offers);
    let element = opens_a_menu(element, move |this, at, cx| pressed(this, at, cx), cx);
    let Some(reach) = reach else {
        return element;
    };

    element.child(
        canvas(
            move |drawn, _, cx| {
                cx.set_global(ReachedMenu {
                    reach,
                    drawn,
                    offers: Rc::clone(&offers),
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0(),
    )
}

pub(crate) fn offered_at_the_reach(
    reach: AtTheReach,
    this: &mut RootView,
    cx: &mut Context<RootView>,
) -> Option<Menu> {
    let (drawn, offers) = cx
        .try_global::<ReachedMenu>()
        .filter(|kept| kept.reach == reach)
        .map(|kept| (kept.drawn, Rc::clone(&kept.offers)))?;
    let at = point(
        drawn.left() + px(OPENED_IN_FROM_THE_ROWS_EDGE),
        drawn.center().y,
    );
    let mut menu = offers(this, at, cx);
    menu.steps(1);
    Some(menu)
}

pub(crate) fn opens_a_menu(
    mut element: Stateful<Div>,
    offers: impl Fn(&mut RootView, Point<Pixels>, &mut Context<RootView>) -> Menu + 'static,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let owner = element.interactivity().element_id.clone();
    let lit = owner
        .as_ref()
        .is_some_and(|owner| lit_by_its_menu(owner.clone(), cx));
    element
        .when(lit, |element| element.bg(rgb(theme::hover())))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let menu = offers(this, event.position, cx).opened_on(owner.clone());
                this.open_a_menu(menu, cx);
            }),
        )
}

impl Menu {
    pub(crate) fn queues(
        self,
        rows: impl Fn() -> Arc<[PlaylistEntry]> + Clone + 'static,
        plays: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        let next = rows.clone();
        let last = rows;

        self.under(Icon::Play, words::menu::play(), "enter", plays)
            .does(
                Icon::QueueNext,
                words::menu::play_next(),
                move |this, _, cx| {
                    this.queue(&next(), Placement::Next, cx);
                },
            )
            .does(
                Icon::QueueLast,
                words::menu::add_to_queue(),
                move |this, _, cx| {
                    this.queue(&last(), Placement::Queued, cx);
                },
            )
    }

    pub(crate) fn holds(self, rows: impl Fn() -> Held + 'static) -> Self {
        self.does(
            Icon::Plus,
            words::menu::add_to_playlist(),
            move |this, window, cx| {
                this.hold_for_a_playlist(rows(), window, cx);
            },
        )
    }

    pub(crate) fn reaches(
        self,
        track: Option<TrackId>,
        album: Option<AlbumId>,
        artist: Option<ArtistId>,
    ) -> Self {
        self.apart()
            .nests(Icon::Next, words::menu::go_to(), move |mut menu| {
                if let Some(artist) = artist {
                    menu = menu.does(
                        Icon::Artists,
                        words::menu::go_to_artist(),
                        move |this, _, cx| {
                            let now = this.library.read(cx).standing_of(track);
                            let artist = now.map_or(Some(artist), |(_, artist)| artist);
                            if let Some(artist) = artist {
                                this.opened(Selection::Artist(artist), cx);
                            }
                        },
                    );
                }
                if let Some(album) = album {
                    menu = menu.does(
                        Icon::Albums,
                        words::menu::go_to_album(),
                        move |this, _, cx| {
                            let now = this.library.read(cx).standing_of(track);
                            let album = now.map_or(Some(album), |(album, _)| album);
                            if let Some(album) = album {
                                this.opened(Selection::Album(album), cx);
                            }
                        },
                    );
                }

                menu
            })
    }

    pub(crate) fn favours(self, what: Favoured, already: bool) -> Self {
        let (icon, label) = match already {
            true => (Icon::Favourited, words::menu::unfavour()),
            false => (Icon::Favourite, words::menu::favour()),
        };

        self.does(icon, label, move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.favour(what, !already, cx));
        })
    }

    pub(crate) fn looks_for_versions(self, track: Track, at: Point<Pixels>) -> Self {
        self.nests(
            Icon::Search,
            words::menu::find_another_version(),
            move |mut menu| {
                let title = resonate_core::titles::without_its_rating(&track.title).to_owned();
                let artist = track.artist.clone().unwrap_or_default();
                let versions = [
                    ("Clean", words::menu::look_for_clean()),
                    ("Explicit", words::menu::look_for_explicit()),
                ];
                for (version, label) in versions {
                    let query = format!("{title} {artist} {version}");
                    menu = menu.does(Icon::Search, label, move |this, window, cx| {
                        this.search_instead(query.clone(), window, cx);
                    });
                }
                menu.does(
                    Icon::Search,
                    better::find_a_better_version(),
                    move |this, _, cx| this.look_for_a_better_version(track.clone(), at, cx),
                )
            },
        )
    }

    pub(crate) fn shares(self, track: TrackId) -> Self {
        self.nests(Icon::Share, words::menu::share(), move |menu| {
            menu.does(Icon::Share, "song.link", move |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.share(track, cx));
            })
            .does(
                Icon::Share,
                words::menu::upload_to_catbox(),
                move |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.share_file(track, cx));
                },
            )
        })
    }

    pub(crate) fn offers_the_other_copies(self, (own, copies): (Track, Vec<Track>)) -> Self {
        let labels = copy_labels(&own, &copies);

        self.nests(
            Icon::Tracks,
            words::menu::other_copies(),
            move |mut menu| {
                for (copy, label) in copies.into_iter().zip(labels) {
                    let playing: Arc<[Track]> = Arc::from([copy]);
                    menu = menu.does(Icon::Play, label, move |this, _, cx| {
                        this.play(&playing, 0, cx);
                    });
                }
                menu
            },
        )
    }

    pub(crate) fn shares_each(self, tracks: Vec<TrackId>) -> Self {
        self.does(
            Icon::Share,
            words::menu::share_each(),
            move |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.share_each(tracks.clone(), cx));
            },
        )
    }

    pub(crate) fn offers_the_files(self, paths: Vec<PathBuf>) -> Self {
        if paths.is_empty() {
            return self;
        }

        let written = paths
            .iter()
            .map(|path| path.to_string_lossy())
            .collect::<Vec<_>>()
            .join("\n");
        let shown = one_file_in_each_folder(&paths);

        self.nests(Icon::Export, words::menu::copy(), move |menu| {
            menu.does(Icon::Rename, words::menu::copy_paths(), move |_, _, cx| {
                clipboard::copy(written.clone(), cx);
            })
        })
        .does(
            Icon::Folder,
            words::menu::show_in_folder(),
            move |_, _, cx| {
                for path in &shown {
                    cx.reveal_path(path);
                }
            },
        )
    }

    pub(crate) fn offers_the_file(self, location: &MediaLocation, names: Called) -> Self {
        let path = location.locator().as_path().map(Path::to_path_buf);
        let shown = path.clone();

        self.nests(Icon::Export, words::menu::copy(), move |menu| {
            let menu = match path {
                Some(path) => {
                    let written = path.to_string_lossy().into_owned();

                    menu.does(Icon::Rename, words::menu::copy_path(), move |_, _, cx| {
                        clipboard::copy(written.clone(), cx);
                    })
                }
                None => menu,
            };

            [
                (words::menu::copy_title(), Some(names.title)),
                (words::menu::copy_artist(), Some(names.artist)),
                (words::menu::copy_album(), names.album),
            ]
            .into_iter()
            .filter_map(|(label, name)| {
                name.filter(|name| !name.trim().is_empty())
                    .map(|name| (label, name))
            })
            .fold(menu, |menu, (label, name)| {
                menu.does(Icon::Export, label, move |_, _, cx| {
                    clipboard::copy(name.to_string(), cx);
                })
            })
        })
        .when_some(shown, |menu, shown| {
            menu.does(
                Icon::Folder,
                words::menu::show_in_folder(),
                move |_, _, cx| {
                    cx.reveal_path(&shown);
                },
            )
        })
    }
}

pub(crate) fn one_file_in_each_folder(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();

    paths
        .iter()
        .filter(|path| seen.insert(path.parent().map(Path::to_path_buf)))
        .cloned()
        .collect()
}

fn kind_of(copy: &Track) -> String {
    format!("{} {}", copy.codec.as_str(), format::quality(copy.spec))
}

fn where_it_is(copy: &Track) -> Option<String> {
    let path = copy.location.locator().as_path()?;
    let file = path.file_name()?.to_string_lossy();
    Some(match path.parent().and_then(Path::file_name) {
        Some(folder) => format!("{}/{file}", folder.to_string_lossy()),
        None => file.into_owned(),
    })
}

fn copy_labels(own: &Track, copies: &[Track]) -> Vec<String> {
    let kinds: Vec<String> = copies.iter().map(kind_of).collect();
    let own_kind = kind_of(own);

    copies
        .iter()
        .zip(&kinds)
        .map(|(copy, kind)| {
            let shared =
                *kind == own_kind || kinds.iter().filter(|other| *other == kind).count() > 1;
            match where_it_is(copy).filter(|_| shared) {
                Some(place) => words::menu::play_the_copy_in(kind.as_str(), place),
                None => words::menu::play_the_copy(kind.as_str()),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
    use resonate_library::Codec;

    use super::*;

    fn copy(id: u64, path: &str, codec: Codec, format: SampleFormat) -> Track {
        Track {
            explicit: None,
            id: TrackId::new(id).expect("a valid id"),
            location: MediaLocation::local(path),
            title: "Echoes".to_owned(),
            artist: None,
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: None,
            spec: StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Stereo, format),
            codec,
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

    fn two_entries() -> Menu {
        Menu::at(point(px(0.0), px(0.0)))
            .does(Icon::Play, "Play", |_, _, _| {})
            .does(Icon::Queue, "Add to queue", |_, _, _| {})
    }

    #[test]
    fn a_menu_reached_by_key_is_handed_the_reader_and_the_window_gets_it_back_once_it_closes() {
        let mut menu = two_entries();

        assert_eq!(
            Handing::of(true, Some(&menu), false),
            Handing::Stays,
            "a menu opened by a pointer has no reached entry to follow"
        );

        menu.steps(1);

        assert_eq!(Handing::of(true, Some(&menu), false), Handing::ToTheMenu);
        assert_eq!(Handing::of(true, Some(&menu), true), Handing::Stays);
        assert_eq!(Handing::of(true, None, true), Handing::BackToTheWindow);
        assert_eq!(Handing::of(true, None, false), Handing::Stays);
    }

    #[test]
    fn nothing_moves_the_keyboard_focus_while_no_reader_listens() {
        let mut menu = two_entries();
        menu.steps(1);

        assert_eq!(Handing::of(false, Some(&menu), false), Handing::Stays);
    }

    fn a_menu_with_a_menu_beneath() -> Menu {
        two_entries()
            .apart()
            .nests(Icon::Export, words::menu::copy(), |menu| {
                menu.does(Icon::Rename, words::menu::copy_title(), |_, _, _| {})
                    .does(Icon::Rename, words::menu::copy_artist(), |_, _, _| {})
            })
            .apart()
    }

    #[test]
    fn a_menu_beneath_is_walked_by_the_keys_and_stepping_out_lands_on_the_row_it_hangs_from() {
        let mut menu = a_menu_with_a_menu_beneath();

        menu.steps(1);
        assert!(!menu.steps_in(), "Play opens nothing");
        menu.steps(1);
        menu.steps(1);
        assert_eq!(menu.reached, Some(3));
        assert!(menu.reached().is_none(), "a row that opens presses nothing");

        assert!(menu.steps_in());
        assert_eq!(
            menu.beneath,
            Some(Beneath {
                at: 3,
                reached: Some(0)
            })
        );
        assert!(menu.reached().is_some());

        menu.steps(1);
        assert_eq!(
            menu.beneath,
            Some(Beneath {
                at: 3,
                reached: Some(1)
            })
        );
        assert!(
            !menu.steps_in(),
            "enter on a row beneath presses it rather than going back to the first"
        );
        menu.steps(1);
        assert_eq!(
            menu.beneath,
            Some(Beneath {
                at: 3,
                reached: Some(0)
            }),
            "the walk wraps within what is beneath"
        );

        assert!(menu.steps_out());
        assert_eq!(menu.beneath, None);
        assert_eq!(menu.reached, Some(3));
        assert!(!menu.steps_out(), "nothing is left to step out of");
    }

    #[test]
    fn a_menu_opens_with_no_line_left_at_its_foot_and_an_empty_nest_is_not_offered() {
        let menu = two_entries()
            .apart()
            .nests(Icon::Export, words::menu::copy(), |menu| menu)
            .apart()
            .without_a_last_line();

        assert_eq!(menu.labels(), ["Play", "Add to queue"]);
        assert_eq!(menu.entries.len(), 2);
    }

    #[test]
    fn a_label_beneath_is_found_with_the_row_it_hangs_from() {
        let menu = a_menu_with_a_menu_beneath();

        assert_eq!(menu.offering("Play"), Some(Offered::Here(0)));
        assert_eq!(
            menu.offering(words::menu::copy_artist()),
            Some(Offered::Beneath(3, 1))
        );
        assert_eq!(menu.offering("Nothing like it"), None);
    }

    #[test]
    fn a_menu_beneath_stands_beside_its_row_on_whichever_side_has_room() {
        let row = Bounds::new(point(px(100.0), px(50.0)), gpui::size(px(236.0), px(30.0)));
        let wide = gpui::size(px(1200.0), px(800.0));
        let narrow = gpui::size(px(500.0), px(800.0));

        assert_eq!(
            beside_its_row(row, wide),
            point(px(100.0 + 236.0 + BESIDE_ITS_ROW), px(50.0 - PANEL_PADDING))
        );
        assert_eq!(
            beside_its_row(row, narrow).x,
            px(100.0 - BESIDE_ITS_ROW - theme::menu_width())
        );
    }

    #[test]
    fn files_in_one_folder_are_shown_by_the_first_of_them_and_each_other_folder_by_its_own() {
        let paths: Vec<PathBuf> = [
            "/music/a/01.flac",
            "/music/b/01.flac",
            "/music/a/02.flac",
            "/music/c/03.flac",
            "/music/b/04.flac",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();

        assert_eq!(
            one_file_in_each_folder(&paths),
            [
                PathBuf::from("/music/a/01.flac"),
                PathBuf::from("/music/b/01.flac"),
                PathBuf::from("/music/c/03.flac"),
            ]
        );
    }

    #[test]
    fn a_copy_of_a_kind_nothing_else_shares_is_named_by_its_kind_alone() {
        let own = copy(
            1,
            "/music/hires/06 Echoes.flac",
            Codec::Flac,
            SampleFormat::S24,
        );
        let other = copy(
            2,
            "/music/cd/06 Echoes.flac",
            Codec::Flac,
            SampleFormat::S16,
        );

        let labels = copy_labels(&own, std::slice::from_ref(&other));
        assert_eq!(labels, vec![format!("Play the {} copy", kind_of(&other))]);
    }

    #[test]
    fn copies_of_one_kind_are_told_apart_by_where_they_are() {
        let own = copy(1, "/music/a/06 Echoes.flac", Codec::Flac, SampleFormat::S16);
        let twin = copy(2, "/music/b/06 Echoes.flac", Codec::Flac, SampleFormat::S16);
        let lossy = copy(3, "/music/c/06 Echoes.mp3", Codec::Mp3, SampleFormat::F32);
        let lossy_again = copy(4, "/music/d/06 Echoes.mp3", Codec::Mp3, SampleFormat::F32);

        let labels = copy_labels(&own, &[twin.clone(), lossy.clone(), lossy_again.clone()]);
        assert_eq!(
            labels,
            vec![
                format!("Play the {} copy in b/06 Echoes.flac", kind_of(&twin)),
                format!("Play the {} copy in c/06 Echoes.mp3", kind_of(&lossy)),
                format!("Play the {} copy in d/06 Echoes.mp3", kind_of(&lossy_again)),
            ]
        );
    }
}
