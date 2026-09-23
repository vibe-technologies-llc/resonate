use std::time::Duration;

use gpui::{
    BoxShadow, Context, Div, FontWeight, ObjectFit, SharedString, Stateful, Window, div, hsla, img,
    point, prelude::*, px, relative, rgb, rgba,
};
use resonate_engine::NodeName;
use resonate_listen::Listening;

use crate::{
    Pane, Setting,
    icons::{self, Icon},
    listening::{Found, Stage},
    theme,
    views::{
        hint::Names,
        kit::{self, Tone},
        root::RootView,
    },
};

const LISTEN_HINT: &str = "Listen again to whatever is sounding";
const STOP_HINT: &str = "Stop listening";
const CLOSE_HINT: &str = "Close";
const FIND_HINT: &str =
    "Look for it in the library, and on MusicBrainz where the library has not got it";
const OPEN_HINT: &str = "Open the page the service keeps for it";
const DESKTOP_HINT: &str = "Hear what the desktop is playing";
const MICROPHONE_HINT: &str = "Hear a microphone";

impl RootView {
    pub(crate) fn open_the_listener(&mut self, cx: &mut Context<Self>) {
        self.listening_open = true;
        self.listen.update(cx, |listen, cx| {
            listen.look_for_microphones(cx);
            listen.listen(cx);
        });
        cx.notify();
    }

    pub(crate) fn close_the_listener(&mut self, cx: &mut Context<Self>) {
        self.listen.update(cx, |listen, cx| listen.stop(cx));
        self.listening_open = false;
        cx.notify();
    }

    pub(crate) fn listen_from(&mut self, from: Listening, cx: &mut Context<Self>) {
        self.store(&Setting::ListenFrom(from.clone()), cx);
        self.listen.update(cx, |listen, cx| listen.choose(from, cx));
    }

    pub(crate) fn listen_for(&mut self, length: Duration, cx: &mut Context<Self>) {
        self.store(&Setting::ListenFor(length), cx);
        self.listen
            .update(cx, |listen, cx| listen.listen_for(length, cx));
    }

    fn find_what_was_heard(&mut self, found: &Found, window: &mut Window, cx: &mut Context<Self>) {
        let heard = &found.heard;
        let asked = match heard.artist.as_deref() {
            Some(artist) => format!("{} {artist}", heard.title),
            None => heard.title.clone(),
        };
        self.close_the_listener(cx);
        self.search_instead(asked, window, cx);
        self.choose_pane(Pane::Tracks, cx);
    }

    pub(crate) fn listen_sheet(&self, cx: &mut Context<Self>) -> Div {
        let listen = self.listen.read(cx);
        let stage = listen.stage();
        let from = listen.from().clone();
        let length = listen.length();
        let microphones = listen.microphones().to_vec();
        let earlier: Vec<Found> = listen.heard().iter().skip(1).cloned().collect();
        let listening = listen.is_listening();

        let card = div()
            .id("listen-sheet")
            .flex()
            .flex_col()
            .gap_4()
            .w(px(theme::listen_width()))
            .p_5()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_1()
            .border_color(rgb(theme::border()))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.5),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(48.0),
                spread_radius: px(0.0),
            }])
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_the_listener(cx)))
            .child(self.listen_heading(listening, cx))
            .child(self.listen_sources(&from, &microphones, listening, cx))
            .child(self.listen_stage(&stage, &from, length, cx))
            .when(!earlier.is_empty(), |card| {
                card.child(self.heard_earlier(&earlier, cx))
            });

        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(theme::scrim()))
            .occlude()
            .child(card)
    }

    fn listen_heading(&self, listening: bool, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(icons::icon(Icon::Listen, 18.0, theme::accent()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(theme::text_lg()))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .child("Listen"),
            )
            .when(listening, |row| {
                row.child(
                    kit::button(
                        "listen-stop",
                        Some(Icon::Stop),
                        "Stop",
                        STOP_HINT,
                        Tone::Ghost,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.listen.update(cx, |listen, cx| listen.stop(cx));
                    })),
                )
            })
            .child(
                kit::icon_button("listen-close", Icon::Close, CLOSE_HINT)
                    .on_click(cx.listener(|this, _, _, cx| this.close_the_listener(cx))),
            )
    }

    fn listen_sources(
        &self,
        from: &Listening,
        microphones: &[(String, String)],
        listening: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let row = kit::segmented()
            .child(
                kit::segment("listen-desktop", "Desktop", !from.is_a_microphone())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.listen_from(Listening::Desktop, cx)),
                    )
                    .names(DESKTOP_HINT),
            )
            .child(
                kit::segment("listen-microphone", "Microphone", from.is_a_microphone())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.listen_from(Listening::Microphone(None), cx);
                    }))
                    .names(MICROPHONE_HINT),
            );

        let chosen = match from {
            Listening::Microphone(Some(node)) => Some(node.to_string()),
            _ => None,
        };
        let choices = from.is_a_microphone() && microphones.len() > 1;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(listening, |sources| sources.opacity(0.6))
            .child(row)
            .when(choices, |sources| {
                sources.child(microphones.iter().enumerate().fold(
                    div().flex().flex_wrap().gap_1p5(),
                    |chips, (index, (name, described))| {
                        let node = name.clone();
                        chips.child(
                            kit::chip(
                                ("listen-microphone-choice", index),
                                described.clone(),
                                chosen.as_deref() == Some(name.as_str()),
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.listen_from(
                                        Listening::Microphone(Some(NodeName::new(node.clone()))),
                                        cx,
                                    );
                                },
                            )),
                        )
                    },
                ))
            })
    }

    fn listen_stage(
        &self,
        stage: &Stage,
        from: &Listening,
        length: std::time::Duration,
        cx: &mut Context<Self>,
    ) -> Div {
        match stage {
            Stage::Recording(hearing) => {
                let heard_from = if from.is_a_microphone() {
                    "the microphone"
                } else {
                    "what the desktop plays"
                };
                listen_note(format!(
                    "Listening to {heard_from} for {} seconds…",
                    length.as_secs()
                ))
                .child(
                    div()
                        .h(px(4.0))
                        .w_full()
                        .rounded_full()
                        .bg(rgb(theme::raised()))
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .bg(rgb(theme::accent()))
                                .w(relative(hearing.share())),
                        ),
                )
            }
            Stage::Asking => listen_note("Asking who it is…".to_owned()),
            Stage::Found(found) => self.found_card(found, cx),
            Stage::Unknown => {
                self.listen_again("Nothing that was heard is a song the services know.", cx)
            }
            Stage::Silent => {
                self.listen_again("Nothing reached the recording. Is anything playing?", cx)
            }
            Stage::Unreached => {
                self.listen_again("The services that name a song could not be reached.", cx)
            }
            Stage::NoService => listen_note(
                "Online is off in the settings, so nothing can name what is heard.".to_owned(),
            ),
            Stage::Idle => self.listen_again("Hear what is sounding and name it.", cx),
        }
    }

    fn listen_again(&self, said: &str, cx: &mut Context<Self>) -> Div {
        listen_note(said.to_owned()).child(
            kit::actions().child(
                kit::button(
                    "listen-again",
                    Some(Icon::Listen),
                    "Listen",
                    LISTEN_HINT,
                    Tone::Primary,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.listen.update(cx, |listen, cx| listen.listen(cx));
                })),
            ),
        )
    }

    fn found_card(&self, found: &Found, cx: &mut Context<Self>) -> Div {
        let heard = &found.heard;
        let side = px(theme::listen_cover());
        let beneath = [heard.album.clone(), heard.year.map(|year| year.to_string())]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · ");
        let link = heard.link.clone();
        let finding = found.clone();

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_4()
                    .items_center()
                    .child(
                        div()
                            .flex_none()
                            .size(side)
                            .rounded_lg()
                            .overflow_hidden()
                            .bg(rgb(theme::raised()))
                            .when_some(found.picture.clone(), |cover, picture| {
                                cover.child(img(picture).size(side).object_fit(ObjectFit::Cover))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .min_w(px(0.0))
                            .child(
                                div()
                                    .text_size(px(theme::text_xl()))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(rgb(theme::text()))
                                    .child(SharedString::from(heard.title.clone())),
                            )
                            .when_some(heard.artist.clone(), |lines, artist| {
                                lines.child(
                                    div()
                                        .text_size(px(theme::text_base()))
                                        .text_color(rgb(theme::text()))
                                        .child(artist),
                                )
                            })
                            .when(!beneath.is_empty(), |lines| {
                                lines.child(
                                    div()
                                        .text_size(px(theme::text_sm()))
                                        .text_color(rgb(theme::muted()))
                                        .child(beneath),
                                )
                            })
                            .child(
                                div()
                                    .text_size(px(theme::text_xs()))
                                    .text_color(rgb(theme::faint()))
                                    .child(format!("Named by {}", heard.by)),
                            ),
                    ),
            )
            .child(
                kit::actions()
                    .child(
                        kit::button(
                            "listen-again",
                            Some(Icon::Listen),
                            "Listen again",
                            LISTEN_HINT,
                            Tone::Ghost,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.listen.update(cx, |listen, cx| listen.listen(cx));
                        })),
                    )
                    .when_some(link, |actions, link| {
                        actions.child(
                            kit::button(
                                "listen-open",
                                Some(Icon::Link),
                                "Open",
                                OPEN_HINT,
                                Tone::Ghost,
                            )
                            .on_click(move |_, _, cx| cx.open_url(&link)),
                        )
                    })
                    .child(
                        kit::button(
                            "listen-find",
                            Some(Icon::Search),
                            "Find it",
                            FIND_HINT,
                            Tone::Primary,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.find_what_was_heard(&finding, window, cx);
                            },
                        )),
                    ),
            )
    }

    fn heard_earlier(&self, earlier: &[Found], cx: &mut Context<Self>) -> Div {
        earlier.iter().enumerate().fold(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .pt_3()
                .border_t_1()
                .border_color(rgb(theme::border()))
                .child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(theme::muted()))
                        .child("Heard earlier"),
                ),
            |list, (index, found)| {
                let heard = &found.heard;
                let said = match heard.artist.as_deref() {
                    Some(artist) => format!("{} — {artist}", heard.title),
                    None => heard.title.clone(),
                };
                let finding = found.clone();
                list.child(
                    earlier_row(("listen-earlier", index), said).on_click(cx.listener(
                        move |this, _, window, cx| this.find_what_was_heard(&finding, window, cx),
                    )),
                )
            },
        )
    }
}

fn listen_note(said: String) -> Div {
    div().flex().flex_col().gap_3().child(
        div()
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .child(said),
    )
}

fn earlier_row(id: impl Into<gpui::ElementId>, said: String) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_md()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::text()))
        .cursor_pointer()
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(said)
}
