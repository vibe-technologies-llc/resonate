use std::{slice, time::Instant};

use gpui::{
    AnimationElement, Context, Div, Role, SharedString, Stateful, div, prelude::*, px, rgb,
};
use resonate_core::TrackId;

use super::{DOWNLOADS_PANEL_GAP, DOWNLOADS_TOGGLE_PRESS, RootView};
use crate::{
    Selection,
    downloads::{self, Download, Fetching},
    icons::{self, Icon},
    motion, theme,
    toast::{self},
    views::{
        browser::{self},
        hint::Names,
        kit::{self, Casts as _, EndsInAnEllipsis},
        listing::{self},
        scrollbar::Scrollbars,
        spoken::{self, Spoken},
    },
    words,
};

impl RootView {
    pub(super) fn download_status(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let library = self.library.read(cx);
        let fetching: Vec<Fetching> = library
            .downloads()
            .iter()
            .map(|download| library.fetching(download))
            .collect();
        let keeping = library.keeps_left();
        let underway = keeping.is_some() || fetching.iter().copied().any(Fetching::is_underway);
        let arriving = library.downloads().iter().any(|download| {
            library
                .fetched(download)
                .is_some_and(|fetched| fetched.is_arriving())
        });
        let said = keeping.map_or_else(
            || downloads::summed_up(&fetching, arriving),
            downloads::keeping_said,
        );
        let open = self.downloads_open;
        let hint = if open {
            words::root::hide_downloads_hint()
        } else {
            words::root::show_downloads_hint()
        };
        let mark = if underway {
            theme::accent()
        } else {
            theme::faint()
        };

        let row = div()
            .id("downloads")
            .debug_selector(|| "downloads".to_owned())
            .named_as(Role::Button, &said, hint)
            .thrown(open)
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::sidebar_row()))
            .px_3()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(hint)
            .child(icons::icon(Icon::Download, theme::pane_icon(), mark))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(said),
            )
            .child(icons::icon(
                if open {
                    Icon::ChevronDown
                } else {
                    Icon::ChevronUp
                },
                theme::row_marker_icon(),
                theme::faint(),
            ));

        self.in_the_standing_ring(
            "downloads",
            row,
            |this, _, cx| {
                let closed_by_this_press = this
                    .downloads_pressed_off
                    .take()
                    .is_some_and(|pressed| pressed.elapsed() < DOWNLOADS_TOGGLE_PRESS);
                if closed_by_this_press {
                    return;
                }
                this.downloads_open = !this.downloads_open;
                cx.notify();
            },
            cx,
        )
    }

    pub(super) fn downloads_over_the_app(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnimationElement<Stateful<Div>>> {
        let library = self.library.read(cx);
        let keeping = library.keeps_left();
        if !self.downloads_open || (library.downloads().is_empty() && keeping.is_none()) {
            return None;
        }
        let rows: Vec<(Download, Fetching)> = library
            .downloads()
            .iter()
            .map(|download| (download.clone(), library.fetching(download)))
            .collect();
        let finished = rows.iter().any(|(_, fetching)| !fetching.is_underway());

        let mut list = div()
            .id("download-list")
            .flex()
            .flex_col()
            .gap_0p5()
            .max_h(px(theme::downloads_height()))
            .overflow_y_scroll()
            .track_scroll(&self.scrolls.downloads)
            .when_some(keeping, |list, left| list.child(self.keep_row(left, cx)));
        for (at, (download, fetching)) in rows.into_iter().enumerate() {
            list = list.child(self.download_row(at, &download, fetching, cx));
        }

        let resting = px(theme::transport_height() + DOWNLOADS_PANEL_GAP);

        Some(motion::risen_in(
            div()
                .id("downloads-panel")
                .named_as(Role::Dialog, words::root::downloads_panel_label(), "")
                .absolute()
                .left(px(theme::sidebar_width() + DOWNLOADS_PANEL_GAP))
                .bottom(resting)
                .w(px(theme::downloads_width()))
                .flex()
                .flex_col()
                .gap_1()
                .p_2()
                .rounded_lg()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::soft_edge()))
                .casting(kit::Cast::Short)
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.downloads_pressed_off = Some(Instant::now());
                    this.close_the_downloads(cx);
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .pl_2()
                        .child(kit::eyebrow(words::root::downloads()))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .when(finished, |actions| {
                                    actions.child(self.in_the_pane_ring(
                                        kit::button(
                                            "clear-downloads",
                                            None,
                                            words::root::clear_finished(),
                                            words::root::clear_downloads_hint(),
                                            kit::Tone::Ghost,
                                        ),
                                        |this, _, cx| {
                                            this.library.update(cx, |library, cx| {
                                                library.clear_finished_downloads(cx);
                                            });
                                        },
                                        cx,
                                    ))
                                })
                                .child(self.in_the_pane_ring(
                                    kit::icon_button(
                                        "close-downloads",
                                        Icon::Close,
                                        words::root::hide_downloads_hint(),
                                    ),
                                    |this, _, cx| {
                                        this.close_the_downloads(cx);
                                    },
                                    cx,
                                )),
                        ),
                )
                .child(
                    div().relative().child(list).child(
                        Scrollbars::of(cx)
                            .vertical("downloads-scrollbar", self.scrolls.downloads.clone()),
                    ),
                ),
            "downloads-arrives",
            resting,
        ))
    }

    pub(super) fn close_the_downloads(&mut self, cx: &mut Context<Self>) {
        self.downloads_open = false;
        cx.notify();
    }

    pub(super) fn keep_row(&self, left: u64, cx: &mut Context<Self>) -> Div {
        let said = downloads::keeping_said(left);
        div()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(words::root::keeping_songs()),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::accent()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(said),
                    ),
            )
            .child(self.in_the_pane_ring(
                kit::icon_button("stop-keeping", Icon::Stop, words::root::stop_keeping_hint()),
                |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.stop_keeping(cx));
                },
                cx,
            ))
    }

    pub(super) fn download_row(
        &self,
        at: usize,
        download: &Download,
        fetching: Fetching,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let recording = download.found.recording.clone();
        let again = download.found.clone();
        let state = browser::fetching_colour(fetching);
        let library = self.library.read(cx);
        let saying = downloads::saying_while(fetching, library.fetched(download).as_ref());
        let album = library.downloaded_album(download);
        let track = library
            .downloaded_track(download)
            .filter(|_| fetching == Fetching::Downloaded);

        let spoken_row = || {
            spoken::joined([
                download.found.title.as_str(),
                download.found.artist.as_str(),
                saying.as_ref(),
            ])
        };

        let text = div()
            .id(listing::keyed_by("open-download", &recording))
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .child(
                div()
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::text()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(download.found.title.clone())),
            )
            .child(
                div()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(theme::faint()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(download.found.artist.clone())),
            )
            .child(
                div()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(state))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(saying.clone()),
            );
        let text = match album {
            Some(album) => self.in_the_pane_ring(
                text.cursor_pointer()
                    .names(words::root::open_download_hint())
                    .named_as(
                        Role::Link,
                        &download.found.title,
                        words::root::open_download_hint(),
                    ),
                move |this, _, cx| {
                    this.close_the_downloads(cx);
                    this.opened(Selection::Album(album), cx);
                },
                cx,
            ),
            None => text,
        };

        let actions = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .when_some(track, |actions, track| {
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("play-download", &recording),
                        Icon::Play,
                        words::root::play_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.play_what_was_downloaded(track, cx);
                    },
                    cx,
                ))
            })
            .when(fetching.can_be_asked_again(), |actions| {
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("ask-again", &recording),
                        Icon::Redo,
                        words::root::ask_again_hint(),
                    ),
                    move |this, _, cx| {
                        let wanted = again.clone();
                        this.library
                            .update(cx, |library, cx| library.want_found(wanted, cx));
                    },
                    cx,
                ))
            })
            .when(fetching.can_be_cancelled(), |actions| {
                let cancelled = recording.clone();
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("cancel-download", &recording),
                        Icon::Stop,
                        words::root::cancel_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.cancel_download(&cancelled, cx);
                        });
                    },
                    cx,
                ))
            })
            .when(!fetching.is_underway(), |actions| {
                let dismissed = recording.clone();
                actions.child(self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("dismiss-download", &recording),
                        Icon::Close,
                        words::root::dismiss_download_hint(),
                    ),
                    move |this, _, cx| {
                        this.library.update(cx, |library, cx| {
                            library.dismiss_download(&dismissed, cx);
                        });
                    },
                    cx,
                ))
            });

        div()
            .id(listing::keyed_by("download", &recording))
            .debug_selector(move || format!("download-{at}"))
            .listed_as(spoken_row)
            .flex()
            .items_center()
            .gap_2()
            .pl_3()
            .pr_1()
            .py_1p5()
            .rounded_md()
            .child(text)
            .child(actions)
    }

    pub(super) fn play_what_was_downloaded(&mut self, track: TrackId, cx: &mut Context<Self>) {
        let catalog = self.library.read(cx).catalog();
        cx.spawn(async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move { catalog.track(track) })
                .await;
            let played = this.update(cx, |this, cx| match read {
                Ok(Some(track)) => this.play(slice::from_ref(&track), 0, cx),
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(%error, "a downloaded song could not be read to play");
                    toast::tell(toast::could_not(words::root::play_that_song(), &error), cx);
                }
            });
            let _ = played;
        })
        .detach();
    }
}
