mod catalog;
mod matching;

use std::{cell::Cell, rc::Rc, time::Duration};

use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, HighlightStyle, MouseButton, MouseDownEvent,
    Role, ScrollHandle, SharedString, Stateful, StyledText, Task, Window, div, prelude::*, px, rgb,
    rgba,
};
use resonate_core::{Appearance, Following, Span};
use resonate_engine::{Command, RepeatMode};
use resonate_eq::Binding;
use resonate_library::{
    AlbumOrder, AlbumQuery, ArtistOrder, ArtistQuery, Favoured, SortOrder, TrackQuery,
};

use self::{
    catalog::{
        CommandKey, Leading, LibraryHits, Mode, PaletteAction, PaletteEntry, SectionKind,
        StatusBadge, entries_for,
    },
    matching::{Scope, record_recent},
};
use crate::{
    ResonateApp, Selection, Setting,
    app::PALETTE_CONTEXT,
    icons::{self, Icon},
    motion,
    serial::Serial,
    theme,
    views::{
        field::{DeleteBackward, Field, Submitted},
        hint::Names as _,
        kit::{self, Casts as _, EndsInAnEllipsis as _},
        reorder::Shift,
        root::{Pane, RootView},
        scrollbar::Scrollbars,
        sheets,
        spoken::{Placed, Spoken as _},
    },
    words,
};

const LIBRARY_SEARCH_SETTLE: Duration = Duration::from_millis(100);
const LIBRARY_ARTISTS_LIMIT: usize = 4;
const LIBRARY_ALBUMS_LIMIT: usize = 4;
const LIBRARY_TRACKS_LIMIT: usize = 5;
const PAGE_STEP: usize = 6;
const HORIZONTAL_MARGIN: f32 = 32.0;
const VERTICAL_MARGIN: f32 = 48.0;
const CHROME_OVERHEAD: f32 = 128.0;
const MODE_PILL_ACCENT_WASH: u8 = 0x22;
const KEYCAP_BORDER_WIDTH: f32 = 1.0;

pub(crate) struct CommandPalette {
    open: bool,
    mode: Mode,
    pub(crate) query: Entity<Field>,
    selected: usize,
    scroll: ScrollHandle,
    reached_unseen: Rc<Cell<bool>>,
    recent: Vec<CommandKey>,
    hits: LibraryHits,
    asked: Serial,
    _search_task: Task<()>,
}

impl CommandPalette {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let query =
            cx.new(|cx| Field::new(words::command_palette::placeholder, window, cx).searching());

        Self {
            open: false,
            mode: Mode::Root,
            query,
            selected: 0,
            scroll: ScrollHandle::new(),
            reached_unseen: Rc::default(),
            recent: Vec::new(),
            hits: LibraryHits::default(),
            asked: Serial::default(),
            _search_task: Task::ready(()),
        }
    }

    pub(crate) const fn is_open(&self) -> bool {
        self.open
    }

    pub(crate) const fn mode(&self) -> Mode {
        self.mode
    }

    pub(crate) fn can_pop_on_backspace(&self, cx: &gpui::App) -> bool {
        self.open && self.mode != Mode::Root && self.query.read(cx).text().is_empty()
    }

    fn reset_selection(&mut self) {
        self.selected = 0;
        self.reached_unseen.set(true);
    }

    fn step(&mut self, delta: isize, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }

        let current = self.selected.min(total - 1) as isize;
        let next = (current + delta).rem_euclid(total as isize) as usize;

        self.selected = next;
        self.reached_unseen.set(true);
    }

    fn step_page(&mut self, down: bool, total: usize) {
        if total == 0 {
            self.selected = 0;
            return;
        }

        let current = self.selected.min(total - 1);

        self.selected = if down {
            (current + PAGE_STEP).min(total - 1)
        } else {
            current.saturating_sub(PAGE_STEP)
        };
        self.reached_unseen.set(true);
    }

    fn clear_hits(&mut self) {
        self.asked.moved();
        self.hits = LibraryHits::default();
        self._search_task = Task::ready(());
    }
}

pub(crate) fn make_command_palette(
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> Entity<CommandPalette> {
    let palette = cx.new(|cx| CommandPalette::new(window, cx));
    let query = palette.read(cx).query.clone();

    cx.observe(&palette, |_, _, cx| cx.notify()).detach();
    cx.observe(&query, |this, _, cx| {
        this.command_palette_query_changed(cx);
    })
    .detach();
    cx.subscribe_in(&query, window, |this, _, _: &Submitted, window, cx| {
        this.run_selected_palette_entry(window, cx);
    })
    .detach();

    palette
}

impl RootView {
    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.read(cx).is_open() {
            self.close_command_palette(window, cx);
        } else {
            self.open_command_palette(window, cx);
        }
    }

    pub(crate) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        self.change_the_orders(cx, |orders| orders.ordering = None);
        self.record = None;
        self.remember_the_opener(window, cx);

        let query = self.command_palette.update(cx, |palette, cx| {
            palette.open = true;
            palette.mode = Mode::Root;
            palette.reset_selection();
            palette.clear_hits();

            let field = palette.query.clone();

            field.update(cx, |field, cx| {
                field.clear(cx);
                field.take_focus(window, cx);
            });
            cx.notify();
            field
        });
        let _ = query;

        cx.notify();
    }

    pub(crate) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.command_palette.read(cx).is_open() {
            return;
        }
        self.command_palette.update(cx, |palette, cx| {
            palette.open = false;
            palette.mode = Mode::Root;
            palette.clear_hits();
            palette.query.update(cx, |field, cx| field.clear(cx));
            cx.notify();
        });
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn escape_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette.read(cx).mode() != Mode::Root {
            self.pop_command_palette_mode(window, cx);
            return;
        }
        self.close_command_palette(window, cx);
    }

    fn pop_command_palette_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette.update(cx, |palette, cx| {
            palette.mode = Mode::Root;
            palette.reset_selection();
            palette.clear_hits();
            palette.query.update(cx, |field, cx| {
                field.clear(cx);
                field.take_focus(window, cx);
            });
            cx.notify();
        });
        cx.notify();
    }

    fn enter_command_palette_mode(
        &mut self,
        mode: Mode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.command_palette.update(cx, |palette, cx| {
            palette.mode = mode;
            palette.reset_selection();
            palette.clear_hits();
            palette.query.update(cx, |field, cx| {
                field.clear(cx);
                field.take_focus(window, cx);
            });
            cx.notify();
        });
        cx.notify();
    }

    fn toggle_palette_scope(&mut self, scope: Scope, window: &mut Window, cx: &mut Context<Self>) {
        let Some(prefix) = scope.prefix() else {
            return;
        };
        let (current_scope, rest) = {
            let text = self.command_palette.read(cx).query.read(cx).text();
            let (parsed, tail) = Scope::parse(text);
            (parsed, tail.to_owned())
        };

        let next = if current_scope == scope {
            rest
        } else if rest.is_empty() {
            format!("{prefix} ")
        } else {
            format!("{prefix} {rest}")
        };

        self.command_palette.update(cx, |palette, cx| {
            palette.mode = Mode::Root;
            palette.reset_selection();
            palette.query.update(cx, |field, cx| {
                field.open_with(next, cx);
                field.take_focus(window, cx);
            });
            cx.notify();
        });
        cx.notify();
    }

    fn command_palette_query_changed(&mut self, cx: &mut Context<Self>) {
        if !self.command_palette.read(cx).is_open() {
            return;
        }

        let (mode, raw) = {
            let palette = self.command_palette.read(cx);
            (palette.mode, palette.query.read(cx).text().to_owned())
        };

        self.command_palette.update(cx, |palette, _| {
            palette.reset_selection();
        });

        if mode != Mode::Root {
            self.command_palette
                .update(cx, |palette, _| palette.clear_hits());
            cx.notify();
            return;
        }

        let (scope, trimmed) = Scope::parse(&raw);

        let wants_library = matches!(scope, Scope::All | Scope::Music) && !trimmed.is_empty();

        if !wants_library {
            self.command_palette
                .update(cx, |palette, _| palette.clear_hits());
            cx.notify();
            return;
        }

        let wanted = trimmed.to_owned();

        if self.command_palette.read(cx).hits.query == wanted {
            cx.notify();
            return;
        }

        let catalog = self.library.read(cx).catalog();

        self.command_palette.update(cx, |palette, cx| {
            let ticket = palette.asked.moved();

            palette.hits.query = wanted.clone();
            palette.hits.loading = true;
            palette._search_task = cx.spawn(async move |this, cx| {
                cx.background_executor().timer(LIBRARY_SEARCH_SETTLE).await;

                let query_text = wanted.clone();
                let found = cx
                    .background_executor()
                    .spawn(async move {
                        let artists = catalog
                            .artists(&ArtistQuery {
                                text: Some(query_text.clone()),
                                sort: ArtistOrder::Relevance,
                                limit: Some(LIBRARY_ARTISTS_LIMIT),
                                ..ArtistQuery::default()
                            })
                            .unwrap_or_default();
                        let albums = catalog
                            .albums(&AlbumQuery {
                                text: Some(query_text.clone()),
                                sort: AlbumOrder::Relevance,
                                limit: Some(LIBRARY_ALBUMS_LIMIT),
                                ..AlbumQuery::default()
                            })
                            .unwrap_or_default();
                        let tracks = catalog
                            .tracks(&TrackQuery {
                                text: Some(query_text),
                                sort: SortOrder::Relevance,
                                limit: Some(LIBRARY_TRACKS_LIMIT),
                                ..TrackQuery::default()
                            })
                            .unwrap_or_default();
                        (artists, albums, tracks)
                    })
                    .await;

                let _ = this.update(cx, |palette, cx| {
                    if palette.asked != ticket {
                        return;
                    }
                    palette.hits = LibraryHits {
                        query: wanted,
                        loading: false,
                        artists: found.0,
                        albums: found.1,
                        tracks: found.2,
                    };
                    cx.notify();
                });
            });
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn step_command_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        if !self.command_palette.read(cx).is_open() {
            return;
        }

        let total = self.command_palette_entries(cx).len();

        self.command_palette.update(cx, |palette, cx| {
            palette.step(delta, total);
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn page_command_palette(&mut self, down: bool, cx: &mut Context<Self>) {
        if !self.command_palette.read(cx).is_open() {
            return;
        }

        let total = self.command_palette_entries(cx).len();

        self.command_palette.update(cx, |palette, cx| {
            palette.step_page(down, total);
            cx.notify();
        });
        cx.notify();
    }

    fn command_palette_entries(&self, cx: &mut Context<Self>) -> Vec<PaletteEntry> {
        let (mode, raw_query, recent, hits) = {
            let palette = self.command_palette.read(cx);
            (
                palette.mode,
                palette.query.read(cx).text().to_owned(),
                palette.recent.clone(),
                palette.hits.clone(),
            )
        };
        entries_for(self, mode, &raw_query, &recent, &hits, cx)
    }

    fn run_selected_palette_entry(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.command_palette.read(cx).is_open() {
            return;
        }

        let entries = self.command_palette_entries(cx);
        let selected = self.command_palette.read(cx).selected;

        let Some(entry) = entries.get(selected).or_else(|| entries.first()) else {
            return;
        };

        self.execute_palette_action(entry.action.clone(), entry.mru_key, window, cx);
    }

    fn execute_palette_action(
        &mut self,
        action: PaletteAction,
        mru_key: Option<CommandKey>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(key) = mru_key {
            self.command_palette.update(cx, |palette, _| {
                record_recent(&mut palette.recent, key);
            });
        }

        if let PaletteAction::EnterMode(mode) = action {
            self.enter_command_palette_mode(mode, window, cx);
            return;
        }

        self.close_command_palette(window, cx);

        match action {
            PaletteAction::EnterMode(_) => {}
            PaletteAction::TogglePlayPause => self.send(Command::TogglePlayPause, cx),
            PaletteAction::Next => self.send(Command::Next, cx),
            PaletteAction::Previous => self.send(Command::Previous, cx),
            PaletteAction::Stop => self.send(Command::Stop, cx),
            PaletteAction::ToggleShuffle => {
                let shuffle = self.player.read(cx).state().shuffle;

                self.send(Command::SetShuffle(!shuffle), cx);
            }
            PaletteAction::CycleRepeat => {
                let repeat = match self.player.read(cx).state().repeat {
                    RepeatMode::Off => RepeatMode::Queue,
                    RepeatMode::Queue => RepeatMode::Track,
                    RepeatMode::Track => RepeatMode::Off,
                };

                self.send(Command::SetRepeat(repeat), cx);
            }
            PaletteAction::ToggleMute => self.toggle_mute(cx),
            PaletteAction::VolumeStep(delta) => self.volume_by(delta, cx),
            PaletteAction::SeekBy(seconds) => self.seek_by(seconds, cx),
            PaletteAction::ClearQueue => {
                let len = self.player.read(cx).queue().len();
                if let Some(last) = len.checked_sub(1) {
                    self.drop_rows(Shift::Queue, Span::between(0, last), cx);
                }
            }
            PaletteAction::ToggleFavouritePlaying => {
                let now = self.playing_now(cx);
                if let Some(track) = now.track {
                    self.library.update(cx, |library, cx| {
                        library.favour(Favoured::Track(track), !now.favourite, cx);
                    });
                }
            }
            PaletteAction::GoToPlayingAlbum(album) => self.opened(Selection::Album(album), cx),
            PaletteAction::GoToPlayingArtist(artist) => self.opened(Selection::Artist(artist), cx),
            PaletteAction::OpenListen => self.open_the_listener(cx),
            PaletteAction::FocusSearch => self.enter_search(window, cx),
            PaletteAction::GoBack => self.go_back(cx),
            PaletteAction::GoForward => self.go_forward(cx),
            PaletteAction::GoToPane(Pane::Queue) => self.toggle_queue(cx),
            PaletteAction::GoToPane(pane) => self.choose_pane(pane, cx),
            PaletteAction::SelectSink(sink) => self.route_to(sink, cx),
            PaletteAction::SelectZone(zone) => self.show_zone(zone, cx),
            PaletteAction::MovePlaybackTo(zone) => self.move_playback_to(zone, cx),
            PaletteAction::AddZone => self.add_a_zone(cx),
            PaletteAction::ToggleEqualiser => {
                let on = self.equaliser.read(cx).bindings().enabled;

                self.switch_the_equaliser(!on, cx);
            }
            PaletteAction::SelectEqProfile(profile) => {
                let sink = self.player.read(cx).output_settings().sink.clone();
                let binding = profile.map(Binding::Profile);

                self.bind_a_curve(sink, binding, cx);
            }
            PaletteAction::SetSleep(until) => self.send(Command::SleepUntil(until), cx),
            PaletteAction::SelectTheme(palette) => {
                let kept = theme::chosen();
                let wearing = Appearance {
                    theme: palette,
                    follows: Following {
                        scheme: false,
                        ..kept.follows
                    },
                    ..kept
                };

                self.wear(wearing, &Setting::Theme(palette), cx);
                self.store(&Setting::FollowDesktopScheme(false), cx);
            }
            PaletteAction::SelectAccent(accent) => {
                let dressed = Appearance {
                    accent,
                    ..theme::chosen()
                };

                self.wear(dressed, &Setting::Accent(accent), cx);
            }
            PaletteAction::ToggleReducedMotion => {
                let reduced = cx.global::<ResonateApp>().reduced_motion;

                self.reduce_motion(!reduced, cx);
            }
            PaletteAction::ToggleFullScreen => window.toggle_fullscreen(),
            PaletteAction::ScanLibrary => {
                self.library.update(cx, |library, cx| library.rescan(cx));
            }
            PaletteAction::EnrichLibrary => {
                self.library
                    .update(cx, |library, cx| library.enrich(false, cx));
            }
            PaletteAction::OpenDownloads => {
                self.open_the_downloads(cx);
            }
            PaletteAction::OpenWrapped => self.open_wrapped(cx),
            PaletteAction::OpenSettings(category, page) => {
                self.set_pane(Pane::Settings, cx);
                self.open_the_place(category, page, cx);
            }
            PaletteAction::OpenPlaylist(playlist) => {
                self.set_pane(Pane::Playlists, cx);
                self.show_playlist(Some(playlist), cx);
            }
            PaletteAction::OpenArtist(artist) => self.opened(Selection::Artist(artist), cx),
            PaletteAction::OpenAlbum(album) => self.opened(Selection::Album(album), cx),
            PaletteAction::PlayTrack(track) => self.play(&[*track], 0, cx),
        }
    }

    pub(crate) fn command_palette_sheet(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let viewport = window.viewport_size();
        let right_to_left = resonate_words::spoken().is_right_to_left();
        let (mode, raw_query, selected, scroll, reached_unseen, query_entity, loading) = {
            let palette = self.command_palette.read(cx);
            (
                palette.mode,
                palette.query.read(cx).text().to_owned(),
                palette.selected,
                palette.scroll.clone(),
                Rc::clone(&palette.reached_unseen),
                palette.query.clone(),
                palette.hits.loading,
            )
        };

        let (active_scope, _) = if mode == Mode::Root {
            Scope::parse(&raw_query)
        } else {
            (Scope::All, raw_query.as_str())
        };

        let entries = self.command_palette_entries(cx);
        let total = entries.len();
        let selected = if total == 0 {
            0
        } else {
            selected.min(total - 1)
        };

        let card_width =
            theme::palette_width().min((f32::from(viewport.width) - HORIZONTAL_MARGIN).max(320.0));
        let top_inset =
            theme::palette_top_inset().min((f32::from(viewport.height) * 0.14).max(16.0));
        let max_results_height = theme::palette_results_height().min(
            (f32::from(viewport.height) - top_inset - VERTICAL_MARGIN - CHROME_OVERHEAD).max(120.0),
        );

        let header = self.palette_input_bar(mode, &raw_query, query_entity, cx);
        let body = self.palette_results_body(
            &entries,
            selected,
            scroll,
            reached_unseen,
            max_results_height,
            loading,
            right_to_left,
            cx,
        );
        let footer = self.palette_footer(mode, active_scope, cx);

        let card = div()
            .id("command-palette")
            .debug_selector(|| "command-palette".to_owned())
            .dialog(words::command_palette::title())
            .key_context(PALETTE_CONTEXT)
            .track_focus(&self.sheet_panels.holding(sheets::PALETTE_PANEL, cx))
            .capture_action(cx.listener(|this, _: &DeleteBackward, window, cx| {
                if this.command_palette.read(cx).can_pop_on_backspace(cx) {
                    cx.stop_propagation();
                    this.pop_command_palette_mode(window, cx);
                }
            }))
            .flex()
            .flex_col()
            .w(px(card_width))
            .rounded_xl()
            .overflow_hidden()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::soft_edge()))
            .casting(kit::Cast::Long)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(header)
            .child(kit::rule())
            .child(body)
            .child(kit::rule())
            .child(footer);

        div()
            .id("command-palette-backdrop")
            .absolute()
            .inset_0()
            .flex()
            .justify_center()
            .items_start()
            .pt(px(top_inset))
            .bg(rgba(theme::scrim()))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.close_command_palette(window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, window, cx| {
                    this.close_command_palette(window, cx);
                }),
            )
            .child(motion::lifted_in(card, "command-palette-arrives"))
    }

    fn palette_input_bar(
        &self,
        mode: Mode,
        raw_query: &str,
        query_entity: Entity<Field>,
        cx: &mut Context<Self>,
    ) -> Div {
        let empty = raw_query.is_empty();
        let placeholder_for_mode = mode.placeholder();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2p5()
            .h(px(theme::palette_input_height()))
            .px_4()
            .child(icons::icon(
                mode.icon(),
                theme::pane_icon(),
                theme::accent(),
            ))
            .when_some(mode.label(), |bar, label| {
                bar.child(
                    div()
                        .id("palette-mode-pill")
                        .debug_selector(|| "palette-mode-pill".to_owned())
                        .named_as(
                            Role::Button,
                            label,
                            &words::command_palette::back_to_root_hint(key!(leave)),
                        )
                        .names(words::command_palette::back_to_root_hint(key!(leave)))
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .bg(theme::tinted(theme::accent(), MODE_PILL_ACCENT_WASH))
                        .text_size(px(theme::text_xs()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::accent()))
                        .cursor_pointer()
                        .hover(|pill| pill.bg(kit::chosen_wash()))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.pop_command_palette_mode(window, cx);
                        }))
                        .child(label)
                        .child(icons::icon(
                            Icon::ChevronRight,
                            theme::clear_mark(),
                            theme::accent(),
                        )),
                )
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .text_size(px(theme::text_base()))
                    .child(query_entity)
                    .when(empty && mode != Mode::Root, |holder| {
                        holder.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .bg(rgb(theme::surface()))
                                .text_color(rgb(theme::faint()))
                                .truncate()
                                .ends_in_an_ellipsis()
                                .child(placeholder_for_mode),
                        )
                    }),
            )
            .when(!empty, |bar| {
                bar.child(
                    div()
                        .id("palette-clear")
                        .debug_selector(|| "palette-clear".to_owned())
                        .group("palette-clear-group")
                        .said_as(Role::Button, words::command_palette::clear_hint())
                        .names(words::command_palette::clear_hint())
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(theme::clear_control()))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|button| button.bg(rgb(theme::hover())))
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.command_palette.update(cx, |palette, cx| {
                                palette.query.update(cx, |field, cx| {
                                    field.clear(cx);
                                    field.take_focus(window, cx);
                                });
                            });
                        }))
                        .child(icons::lit_on_hover(
                            icons::icon(Icon::Close, theme::clear_mark(), theme::muted()),
                            "palette-clear-group",
                        )),
                )
            })
            .when(empty, |bar| bar.child(keycap(key!(leave))))
    }

    #[allow(clippy::too_many_arguments)]
    fn palette_results_body(
        &self,
        entries: &[PaletteEntry],
        selected: usize,
        scroll: ScrollHandle,
        reached_unseen: Rc<Cell<bool>>,
        max_height: f32,
        loading: bool,
        right_to_left: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        if entries.is_empty() {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .py_8()
                .px_6()
                .child(kit::empty_mark(Icon::Search, theme::faint()))
                .child(
                    div()
                        .id("palette-empty-title")
                        .headed(2, words::command_palette::empty_title())
                        .text_size(px(theme::text_base()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::text()))
                        .child(words::command_palette::empty_title()),
                )
                .child(
                    div()
                        .id("palette-empty-hint")
                        .read_plainly(words::command_palette::empty_hint())
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::muted()))
                        .child(words::command_palette::empty_hint()),
                );
        }

        let total = entries.len();
        let mut content = div()
            .id("palette-results-list")
            .named_as(Role::List, words::command_palette::results_label(), "")
            .flex()
            .flex_col()
            .p_2()
            .gap_0p5();

        let mut last_section: Option<SectionKind> = None;

        for (index, entry) in entries.iter().enumerate() {
            if last_section != Some(entry.section) {
                let top_pad = last_section.is_some();

                last_section = Some(entry.section);
                content = content.child(
                    div()
                        .flex()
                        .items_center()
                        .px_2()
                        .pt(px(if top_pad { 8.0 } else { 2.0 }))
                        .pb_1()
                        .child(kit::eyebrow(entry.section.label())),
                );
            }

            let is_selected = index == selected;

            content = content.child(self.palette_row(
                entry,
                index,
                total,
                is_selected,
                &scroll,
                &reached_unseen,
                right_to_left,
                cx,
            ));
        }

        if loading {
            content = content.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(theme::faint()))
                    .child(motion::spinning(
                        icons::icon(Icon::Spinner, theme::clear_mark(), theme::faint()),
                        "palette-searching-spinner",
                    ))
                    .child(words::command_palette::searching_library()),
            );
        }

        div()
            .relative()
            .w_full()
            .max_h(px(max_height))
            .child(
                div()
                    .id("palette-results-scroll")
                    .w_full()
                    .max_h(px(max_height))
                    .overflow_y_scroll()
                    .track_scroll(&scroll)
                    .child(content),
            )
            .child(Scrollbars::of(cx).vertical("palette-scrollbar", scroll))
    }

    #[allow(clippy::too_many_arguments)]
    fn palette_row(
        &self,
        entry: &PaletteEntry,
        index: usize,
        total: usize,
        selected: bool,
        scroll: &ScrollHandle,
        reached_unseen: &Rc<Cell<bool>>,
        right_to_left: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let action = entry.action.clone();
        let mru_key = entry.mru_key;
        let title_element = highlighted_title(entry.title.clone(), &entry.highlights);
        let leading = draw_leading(&entry.leading, selected);
        let trailing = draw_trailing(entry.status.as_ref(), entry.shortcut, selected);

        let spoken_label = match &entry.subtitle {
            Some(subtitle) => format!("{}, {}", entry.title, subtitle),
            None => entry.title.to_string(),
        };

        div()
            .id(("palette-entry", index))
            .debug_selector(move || format!("palette-entry-{index}"))
            .listed_as(move || SharedString::from(spoken_label))
            .chosen(selected)
            .placed(Placed {
                at: index,
                of: total,
            })
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .h(px(theme::palette_row_height()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .when_else(
                selected,
                |row| row.bg(kit::chosen_wash()),
                |row| row.hover(|row| row.bg(rgb(theme::hover()))),
            )
            .when(selected, |row| {
                let bar = div()
                    .absolute()
                    .when(right_to_left, Styled::right_0)
                    .when(!right_to_left, Styled::left_0)
                    .top(px(
                        (theme::palette_row_height() - theme::chosen_mark()) / 2.0
                    ))
                    .w(px(2.0))
                    .h(px(theme::chosen_mark()))
                    .rounded_full()
                    .bg(rgb(theme::accent()));
                row.child(bar).child(kit::brought_into_view(
                    scroll.clone(),
                    Rc::clone(reached_unseen),
                ))
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.execute_palette_action(action.clone(), mru_key, window, cx);
            }))
            .child(leading)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .flex_shrink(1.0)
                            .min_w(px(0.0))
                            .text_size(px(theme::text_sm()))
                            .font_weight(if selected {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(title_element),
                    )
                    .when_some(entry.subtitle.clone(), |words_col, subtitle| {
                        words_col.child(
                            div()
                                .flex_shrink(1.0)
                                .min_w(px(0.0))
                                .text_size(px(theme::text_xs()))
                                .text_color(rgb(theme::faint()))
                                .truncate()
                                .ends_in_an_ellipsis()
                                .child(subtitle),
                        )
                    }),
            )
            .child(trailing)
    }

    fn palette_footer(&self, mode: Mode, active_scope: Scope, cx: &mut Context<Self>) -> Div {
        let mut scopes = div().flex().items_center().gap_1();

        if mode == Mode::Root {
            for scope in Scope::FILTERS {
                let prefix = scope.prefix().unwrap_or('>');
                let label = scope_label(scope);
                let chosen = active_scope == scope;
                let hint = words::command_palette::scope_hint(label);

                scopes = scopes.child(
                    div()
                        .id(("palette-scope", prefix as usize))
                        .debug_selector(move || format!("palette-scope-{prefix}"))
                        .named_as(Role::Button, label, &hint)
                        .names(hint)
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .cursor_pointer()
                        .text_size(px(theme::text_xs()))
                        .when_else(
                            chosen,
                            |chip| {
                                chip.bg(kit::chosen_wash())
                                    .text_color(rgb(theme::accent()))
                                    .font_weight(FontWeight::MEDIUM)
                            },
                            |chip| {
                                chip.text_color(rgb(theme::faint()))
                                    .hover(|chip| chip.bg(rgb(theme::hover())))
                            },
                        )
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_palette_scope(scope, window, cx);
                        }))
                        .child(prefix.to_string())
                        .child(label),
                );
            }
        }

        let hints = div()
            .flex()
            .items_center()
            .gap_3()
            .text_size(px(theme::text_xs()))
            .text_color(rgb(theme::faint()))
            .child(footer_hint("↑↓", words::command_palette::footer_navigate()))
            .child(footer_hint("↵", words::command_palette::footer_run()))
            .when_else(
                mode == Mode::Root,
                |row| {
                    row.child(footer_hint(
                        key!(leave),
                        words::command_palette::footer_close(),
                    ))
                },
                |row| row.child(footer_hint("⌫", words::command_palette::footer_back())),
            );

        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .py_2()
            .bg(rgb(theme::background()))
            .child(hints)
            .child(scopes)
    }
}

fn scope_label(scope: Scope) -> &'static str {
    match scope {
        Scope::All | Scope::Commands => words::command_palette::scope_commands(),
        Scope::Settings => words::command_palette::scope_settings(),
        Scope::Music => words::command_palette::scope_music(),
        Scope::Playlists => words::command_palette::scope_playlists(),
    }
}

fn footer_hint(glyph: &'static str, label: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(keycap(glyph))
        .child(label)
}

fn keycap(text: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(theme::palette_keycap_height()))
        .px_1p5()
        .rounded_sm()
        .bg(rgb(theme::raised()))
        .border(px(KEYCAP_BORDER_WIDTH))
        .border_color(rgb(theme::border()))
        .child(kit::figure(text).text_color(rgb(theme::muted())))
}

fn highlighted_title(text: SharedString, ranges: &[std::ops::Range<usize>]) -> AnyElement {
    if ranges.is_empty() {
        return text.into_any_element();
    }

    let struck = HighlightStyle {
        color: Some(rgb(theme::accent()).into()),
        font_weight: Some(FontWeight::SEMIBOLD),
        ..HighlightStyle::default()
    };

    StyledText::new(text)
        .with_highlights(ranges.iter().cloned().map(|range| (range, struck)))
        .into_any_element()
}

fn draw_leading(leading: &Leading, selected: bool) -> AnyElement {
    match leading {
        Leading::Icon(icon) => icons::icon(
            *icon,
            theme::menu_icon(),
            if selected {
                theme::accent()
            } else {
                theme::muted()
            },
        )
        .into_any_element(),
        Leading::ThemeSwatch(palette) => {
            let sample = theme::sample(Appearance {
                theme: *palette,
                ..theme::worn()
            });
            let side = theme::palette_swatch();
            div()
                .flex()
                .flex_none()
                .items_center()
                .overflow_hidden()
                .rounded_sm()
                .border_1()
                .border_color(rgb(theme::border()))
                .h(px(side))
                .child(div().w(px(side * 0.55)).h_full().bg(rgb(sample.background)))
                .child(div().w(px(side * 0.55)).h_full().bg(rgb(sample.surface)))
                .child(div().w(px(side * 0.55)).h_full().bg(rgb(sample.accent)))
                .into_any_element()
        }
        Leading::AccentDot(accent) => {
            let sample = theme::sample(Appearance {
                accent: *accent,
                ..theme::worn()
            });
            let dot = theme::palette_swatch_dot();
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(theme::menu_icon()))
                .child(
                    div()
                        .size(px(dot))
                        .rounded_full()
                        .bg(rgb(sample.accent))
                        .border_1()
                        .border_color(rgb(theme::border())),
                )
                .into_any_element()
        }
    }
}

fn draw_trailing(
    status: Option<&StatusBadge>,
    shortcut: Option<&'static str>,
    selected: bool,
) -> Div {
    let mut row = div().flex().flex_none().items_center().gap_2();

    if let Some(status) = status {
        match status {
            StatusBadge::ActiveCheck => {
                row = row.child(icons::icon(
                    Icon::Check,
                    theme::check_mark(),
                    theme::accent(),
                ));
            }
            StatusBadge::State(label) => {
                row = row.child(kit::badge(
                    label.clone(),
                    if selected {
                        theme::accent()
                    } else {
                        theme::muted()
                    },
                ));
            }
            StatusBadge::DrillDown => {
                row = row.child(icons::icon(
                    Icon::ChevronRight,
                    theme::clear_mark(),
                    if selected {
                        theme::accent()
                    } else {
                        theme::faint()
                    },
                ));
            }
        }
    }

    if let Some(shortcut) = shortcut {
        row = row.child(keycap(shortcut));
    }

    row
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_core::Theme;
    use resonate_library::Library;

    use crate::{driven::Driven, theme, views::root::Pane};

    #[gpui::test]
    fn command_palette_opens_on_ctrl_k_navigates_to_pane_and_closes(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));
        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();

        assert!(driven.read(|root, cx| root.command_palette.read(cx).is_open()));

        driven.cx.simulate_input("lyrics");
        driven.settle();
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, cx| {
            !root.command_palette.read(cx).is_open() && root.pane == Pane::Lyrics
        }));
    }

    #[gpui::test]
    fn command_palette_toggles_closed_on_second_ctrl_k_and_on_escape(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));
        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();

        assert!(driven.read(|root, cx| root.command_palette.read(cx).is_open()));

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();

        assert!(driven.read(|root, cx| !root.command_palette.read(cx).is_open()));

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();
        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert!(driven.read(|root, cx| !root.command_palette.read(cx).is_open()));
    }

    #[gpui::test]
    fn command_palette_drills_into_theme_mode_and_switches_theme(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));
        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();
        driven.cx.simulate_input("switch theme");
        driven.settle();
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, cx| {
            root.command_palette.read(cx).is_open()
                && root.command_palette.read(cx).mode() == super::Mode::Themes
        }));

        driven.cx.simulate_keystrokes("backspace");
        driven.settle();

        assert!(driven.read(|root, cx| {
            root.command_palette.read(cx).is_open()
                && root.command_palette.read(cx).mode() == super::Mode::Root
        }));

        driven.cx.simulate_input("nord");
        driven.settle();
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, cx| !root.command_palette.read(cx).is_open()));
        assert_eq!(theme::worn().theme, Theme::Nord);
    }

    #[gpui::test]
    fn command_palette_jumps_directly_to_a_settings_group_via_question_prefix(
        cx: &mut TestAppContext,
    ) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));
        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();
        driven.cx.simulate_input("? dither");
        driven.settle();
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, cx| {
            !root.command_palette.read(cx).is_open() && root.pane == Pane::Settings
        }));
    }

    #[gpui::test]
    fn command_palette_steps_selection_with_arrows_and_wraps(cx: &mut TestAppContext) {
        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));
        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.command_palette.read(cx).selected),
            0
        );

        driven.cx.simulate_keystrokes("down");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.command_palette.read(cx).selected),
            1
        );

        driven.cx.simulate_keystrokes("up");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.command_palette.read(cx).selected),
            0
        );

        driven.cx.simulate_keystrokes("up");
        driven.settle();

        assert!(driven.read(|root, cx| root.command_palette.read(cx).selected) > 0);
    }

    #[gpui::test]
    fn command_palette_finds_and_opens_an_artist_from_the_library(cx: &mut TestAppContext) {
        let folder = crate::driven::Folder::new();

        folder.tagged(
            "echoes.wav",
            1,
            &[
                (b"INAM", "Echoes"),
                (b"IART", "Pink Floyd"),
                (b"IPRD", "Meddle"),
            ],
        );

        let library = Arc::new(Library::open_in_memory().expect("an in-memory catalog"));

        Driven::scanned(&library, &folder);

        let mut driven = Driven::open(cx, library);

        driven.cx.simulate_keystrokes("secondary-k");
        driven.settle();
        driven.cx.simulate_input("@ pink floyd");
        driven.until(|root, cx| {
            let hits = &root.command_palette.read(cx).hits;
            !hits.loading && !hits.artists.is_empty()
        });

        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert!(driven.read(|root, cx| {
            !root.command_palette.read(cx).is_open()
                && matches!(
                    root.library.read(cx).selection(),
                    crate::Selection::Artist(_)
                )
        }));
    }
}
