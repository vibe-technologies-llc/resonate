use std::time::Duration;

use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, ObjectFit, Role, SharedString, Stateful,
    Window, div, img, prelude::*, px, relative, rgb,
};
use resonate_core::SourceId;
use resonate_engine::NodeName;
use resonate_listen::{DESKTOP_IS_HEARD, Heard, Hearing, Listening};

use crate::{
    Pane, ResonateApp, Setting, SettingsCategory,
    icons::{self, Icon},
    keys,
    listening::{FOLLOWED_EVERY, Following, Found, Stage, TRIES_ON_A_MISS},
    motion,
    system::sound_system,
    theme,
    views::{
        hint::Names,
        kit::{self, EndsInAnEllipsis as _, Press, Tone},
        root::RootView,
        scrollbar::Scrollbars,
        sheets,
        spoken::{self, Spoken, Urgency},
    },
    words,
};

const MICROPHONES_DRAWN_AT_MOST: f32 = 168.0;

const EARLIER_DRAWN_AT_MOST: f32 = 168.0;

const EARLIER_ROW_INSET: f32 = 8.0;

const TILE_MARK_SHARE: f32 = 0.36;

const HEADLINE_LEADING: f32 = 1.4;

const TITLE_LEADING: f32 = 1.2;

const EARLIER_GROUP: &str = "listen-earlier";

const READY: Ending = Ending {
    weight: Weight::Ready,
    said: Said::Ready,
    next: Next::Listen,
};

const NO_MATCH: Ending = Ending {
    weight: Weight::Aside,
    said: Said::NoMatch,
    next: Next::ListenAgain,
};

const NOTHING_HEARD: Ending = Ending {
    weight: Weight::Aside,
    said: Said::NothingHeard,
    next: Next::ListenAgain,
};

const DESKTOP_UNRECORDED: Ending = Ending {
    weight: Weight::Trouble,
    said: Said::DesktopUnrecorded,
    next: Next::ListenAgain,
};

const MICROPHONE_UNRECORDED: Ending = Ending {
    weight: Weight::Trouble,
    said: Said::MicrophoneUnrecorded,
    next: Next::ListenAgain,
};

const UNREACHED: Ending = Ending {
    weight: Weight::Trouble,
    said: Said::Unreached,
    next: Next::ListenAgain,
};

const REFUSED: Ending = Ending {
    weight: Weight::Trouble,
    said: Said::Refused,
    next: Next::ListenAgain,
};

const OFFLINE: Ending = Ending {
    weight: Weight::Aside,
    said: Said::Offline,
    next: Next::OpenTheSettings,
};

const NO_SERVICE: Ending = Ending {
    weight: Weight::Aside,
    said: Said::NoService,
    next: Next::Nothing,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Said {
    Ready,
    NoMatch,
    NothingHeard,
    DesktopUnrecorded,
    MicrophoneUnrecorded,
    Unreached,
    Refused,
    Offline,
    NoService,
}

impl Said {
    fn headline(self) -> &'static str {
        match self {
            Self::Ready => words::listen::ready(),
            Self::NoMatch => words::listen::no_match(),
            Self::NothingHeard => words::listen::nothing_heard(),
            Self::DesktopUnrecorded | Self::MicrophoneUnrecorded => {
                words::listen::could_not_record()
            }
            Self::Unreached => words::listen::unreached(),
            Self::Refused => words::listen::refused(),
            Self::Offline => words::listen::offline(),
            Self::NoService => words::listen::no_service(),
        }
    }

    fn sentence(self) -> String {
        match self {
            Self::Ready => words::listen::ready_more().to_owned(),
            Self::NoMatch => words::listen::no_match_more().to_owned(),
            Self::NothingHeard => words::listen::nothing_heard_more().to_owned(),
            Self::DesktopUnrecorded => words::listen::desktop_unrecorded(sound_system!()),
            Self::MicrophoneUnrecorded => words::listen::microphone_unrecorded(sound_system!()),
            Self::Unreached => words::listen::unreached_more().to_owned(),
            Self::Refused => words::listen::refused_more().to_owned(),
            Self::Offline => words::listen::offline_more().to_owned(),
            Self::NoService => words::listen::no_service_more().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Weight {
    Ready,
    Aside,
    Trouble,
}

impl Weight {
    fn mark(self) -> (Icon, u32) {
        match self {
            Self::Ready => (Icon::Listen, theme::accent()),
            Self::Aside => (Icon::Info, theme::muted()),
            Self::Trouble => (Icon::Alert, theme::failure()),
        }
    }

    const fn urgency(self) -> Urgency {
        match self {
            Self::Ready | Self::Aside => Urgency::Told,
            Self::Trouble => Urgency::Warned,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Next {
    Listen,
    ListenAgain,
    OpenTheSettings,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ending {
    weight: Weight,
    said: Said,
    next: Next,
}

impl Ending {
    fn headline(self) -> &'static str {
        self.said.headline()
    }

    fn sentence(self) -> String {
        self.said.sentence()
    }
}

const fn ending_of(stage: &Stage, from: &Listening) -> Option<Ending> {
    Some(match stage {
        Stage::Recording { .. } | Stage::Asking | Stage::Found(_) => return None,
        Stage::Idle => READY,
        Stage::Unknown => NO_MATCH,
        Stage::Silent => NOTHING_HEARD,
        Stage::CaptureFailed => match from {
            Listening::Desktop => DESKTOP_UNRECORDED,
            Listening::Microphone(_) => MICROPHONE_UNRECORDED,
        },
        Stage::Unreached => UNREACHED,
        Stage::Refused => REFUSED,
        Stage::Offline => OFFLINE,
        Stage::NoService => NO_SERVICE,
    })
}

fn stage_key(stage: &Stage) -> SharedString {
    match stage {
        Stage::Idle => "listen-idle".into(),
        Stage::Recording { nth, .. } => format!("listen-recording-{nth}").into(),
        Stage::Asking => "listen-asking".into(),
        Stage::Found(found) => format!(
            "listen-found-{}-{}",
            found.heard.title,
            found.heard.artist.as_deref().unwrap_or_default()
        )
        .into(),
        Stage::Unknown => "listen-unknown".into(),
        Stage::Silent => "listen-silent".into(),
        Stage::CaptureFailed => "listen-capture-failed".into(),
        Stage::Unreached => "listen-unreached".into(),
        Stage::Refused => "listen-refused".into(),
        Stage::Offline => "listen-offline".into(),
        Stage::NoService => "listen-no-service".into(),
    }
}

fn seconds_left(length: Duration, share: f32) -> u64 {
    let left = length.mul_f32((1.0 - share).clamp(0.0, 1.0));

    left.as_secs() + u64::from(left.subsec_nanos() > 0)
}

fn service_named(by: &SourceId) -> &str {
    match by.as_str() {
        "shazam" => "Shazam",
        "audd" => "AudD",
        other => other,
    }
}

fn heard_from(from: &Listening, microphones: &[(String, String)]) -> String {
    match from {
        Listening::Desktop => words::listen::the_desktop().to_owned(),
        Listening::Microphone(Some(node)) => microphones
            .iter()
            .find(|(name, _)| name.as_str() == node.as_str())
            .map_or_else(
                || words::listen::the_microphone().to_owned(),
                |(_, said)| said.clone(),
            ),
        Listening::Microphone(None) => words::listen::the_microphone().to_owned(),
    }
}

fn recording_said(from: &Listening, microphones: &[(String, String)]) -> String {
    words::listen::recording(heard_from(from, microphones))
}

fn tries_said(nth: u8) -> String {
    match nth {
        1 => words::listen::keep_it_playing().to_owned(),
        _ => words::listen::trying_again(nth, TRIES_ON_A_MISS),
    }
}

fn following_said(following: &Following) -> Option<String> {
    match following {
        Following::Off => None,
        Following::Waiting => Some(words::listen::following_every(FOLLOWED_EVERY.as_secs())),
        Following::Listening { .. } => Some(words::listen::following_now().to_owned()),
    }
}

fn album_and_year(heard: &Heard) -> String {
    [heard.album.clone(), heard.year.map(|year| year.to_string())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
}

fn headline(said: impl Into<SharedString>) -> Div {
    div()
        .min_w(px(0.0))
        .text_size(px(theme::text_base()))
        .line_height(px(theme::text_base() * HEADLINE_LEADING))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::text()))
        .child(said.into())
}

fn sentence(said: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::muted()))
        .child(said.into())
}

fn under(said: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(theme::faint()))
        .child(said.into())
}

fn rail(share: f32) -> Div {
    div()
        .flex_none()
        .h(px(theme::RAIL_TRACK))
        .w_full()
        .rounded_full()
        .bg(rgb(theme::raised()))
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(rgb(theme::accent()))
                .w(relative(share.clamp(0.0, 1.0))),
        )
}

fn stage_tile(drawn: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(theme::listen_cover()))
        .rounded_lg()
        .overflow_hidden()
        .bg(rgb(theme::raised()))
        .child(drawn)
}

fn tile_mark(mark: Icon, shade: u32) -> gpui::Svg {
    icons::icon(mark, theme::listen_cover() * TILE_MARK_SHARE, shade)
}

fn staged(tile: Div, words: Div) -> Div {
    div()
        .flex()
        .items_center()
        .gap_4()
        .child(tile)
        .child(words.flex().flex_col().flex_1().min_w(px(0.0)))
}

fn recording(hearing: &Hearing, said: String, length: Duration, nth: u8) -> Div {
    let share = hearing.share();

    staged(
        stage_tile(tile_mark(Icon::Listen, theme::accent())),
        div()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(headline(said).truncate().ends_in_an_ellipsis())
                    .child(kit::figure(words::listen::seconds_left(seconds_left(
                        length, share,
                    )))),
            )
            .child(rail(share))
            .child(under(tries_said(nth))),
    )
}

fn asking() -> Div {
    staged(
        stage_tile(motion::spinning(
            tile_mark(Icon::Spinner, theme::accent()),
            "listen-asking-spins",
        )),
        div()
            .gap_2()
            .child(headline(words::listen::asking()))
            .child(rail(1.0))
            .child(under(words::listen::asking_under())),
    )
}

fn ended(ending: Ending) -> Div {
    let (mark, shade) = ending.weight.mark();

    staged(
        stage_tile(tile_mark(mark, shade)),
        div()
            .gap_1()
            .child(headline(ending.headline()))
            .child(sentence(ending.sentence())),
    )
}

fn found_card(found: &Found) -> Div {
    let heard = &found.heard;
    let beneath = album_and_year(heard);
    let cover = match found.picture.clone() {
        Some(picture) => stage_tile(
            img(picture)
                .size(px(theme::listen_cover()))
                .object_fit(ObjectFit::Cover),
        ),
        None => stage_tile(tile_mark(Icon::Disc, theme::faint())),
    };

    staged(
        cover,
        div()
            .gap_0p5()
            .child(
                div()
                    .pb_1()
                    .text_size(px(theme::text_xs()))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(theme::faint()))
                    .child(words::listen::named_by(service_named(&heard.by))),
            )
            .child(
                div()
                    .text_size(px(theme::text_xl()))
                    .line_height(px(theme::text_xl() * TITLE_LEADING))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(theme::text()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(SharedString::from(heard.title.clone())),
            )
            .when_some(heard.artist.clone(), |lines, artist| {
                lines.child(
                    div()
                        .text_size(px(theme::text_base()))
                        .text_color(rgb(theme::text()))
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(artist),
                )
            })
            .when(!beneath.is_empty(), |lines| {
                lines.child(
                    div()
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::muted()))
                        .truncate()
                        .ends_in_an_ellipsis()
                        .child(beneath),
                )
            }),
    )
}

fn setting_named(name: &'static str, said_under: Option<String>) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w(px(0.0))
        .gap_0p5()
        .child(
            div()
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(theme::text()))
                .child(name),
        )
        .when_some(said_under, |named, said| named.child(under(said)))
}

fn earlier_row(at: usize, heard: &Heard) -> Stateful<Div> {
    let said = spoken::song(&heard.title, heard.artist.as_deref());

    div()
        .id(("listen-earlier", at))
        .named_as(Role::Button, &said, words::listen::earlier_hint())
        .group(EARLIER_GROUP)
        .flex()
        .flex_none()
        .items_center()
        .gap_2()
        .h(px(theme::menu_row()))
        .px(px(EARLIER_ROW_INSET))
        .rounded_md()
        .cursor_pointer()
        .text_size(px(theme::text_sm()))
        .hover(|row| row.bg(rgb(theme::hover())))
        .child(
            div()
                .min_w(px(0.0))
                .text_color(rgb(theme::text()))
                .truncate()
                .ends_in_an_ellipsis()
                .child(SharedString::from(heard.title.clone())),
        )
        .when_some(heard.artist.clone(), |row, artist| {
            row.child(
                div()
                    .min_w(px(0.0))
                    .text_color(rgb(theme::muted()))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(artist),
            )
        })
        .child(div().flex_1())
        .child(icons::lit_on_hover(
            icons::icon(Icon::Search, theme::menu_icon(), theme::faint()),
            EARLIER_GROUP,
        ))
        .names(words::listen::earlier_hint())
}

impl RootView {
    pub(crate) fn open_the_listener(&mut self, cx: &mut Context<Self>) {
        self.listening_open = true;
        self.listen
            .update(cx, |listen, cx| listen.look_for_microphones(cx));
        self.listen_now(cx);
        cx.notify();
    }

    fn listen_now(&mut self, cx: &mut Context<Self>) {
        let online = cx.global::<ResonateApp>().online.enabled;
        self.listen
            .update(cx, |listen, cx| listen.listen(online, cx));
    }

    pub(crate) fn close_the_listener(&mut self, cx: &mut Context<Self>) {
        self.listen.update(cx, |listen, cx| listen.stop(cx));
        self.listening_open = false;
        cx.notify();
    }

    pub(crate) fn listen_from(&mut self, from: Listening, cx: &mut Context<Self>) {
        if self.listen.read(cx).is_listening() {
            return;
        }
        self.store(&Setting::ListenFrom(from.clone()), cx);
        self.listen.update(cx, |listen, cx| listen.choose(from, cx));
    }

    fn keep_listening(&mut self, cx: &mut Context<Self>) {
        self.listen.update(cx, |listen, cx| {
            let keeps = !listen.keeps_listening();
            listen.keep_listening(keeps, cx);
        });
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

    fn open_the_online_settings(&mut self, cx: &mut Context<Self>) {
        self.close_the_listener(cx);
        self.set_pane(Pane::Settings, cx);
        self.show_settings(SettingsCategory::Online, cx);
    }

    pub(crate) fn listen_sheet(&self, cx: &mut Context<Self>) -> Div {
        let listen = self.listen.read(cx);
        let stage = listen.stage();
        let from = listen.from().clone();
        let length = listen.length();
        let microphones = listen.microphones().to_vec();
        let unheard = from.is_a_microphone() && listen.hears_no_microphone();
        let shown_above = usize::from(matches!(stage, Stage::Found(_)));
        let earlier: Vec<Found> = listen.heard().iter().skip(shown_above).cloned().collect();
        let listening = listen.is_listening();
        let keeps_listening = listen.keeps_listening();
        let following = listen.following();

        let card = kit::sheet()
            .id("listen-sheet")
            .dialog(words::listen::listen_label())
            .track_focus(&self.sheet_panels.holding(sheets::LISTEN_PANEL, cx))
            .w(px(theme::listen_width()))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.close_the_listener(cx)))
            .child(
                kit::sheet_heading(
                    "listen-heading",
                    Icon::Listen,
                    theme::accent(),
                    words::listen::listen_label(),
                )
                .child(self.in_the_pane_ring(
                    kit::icon_button(
                        sheets::LISTEN_CLOSE,
                        Icon::Close,
                        keys::keyed(words::listen::close(), &[key!(leave)]),
                    ),
                    |this, _, cx| this.close_the_listener(cx),
                    cx,
                )),
            )
            .child(listen_stage(&stage, &from, &microphones, length))
            .when_some(
                self.stage_actions(&stage, &from, unheard, cx),
                ParentElement::child,
            )
            .child(self.listen_settings(
                &from,
                &microphones,
                unheard,
                listening,
                keeps_listening,
                &following,
                cx,
            ))
            .when(!earlier.is_empty(), |card| {
                card.child(self.heard_earlier(&earlier, cx))
            });

        kit::over_a_scrim(motion::lifted_in(card, "listen-sheet-arrives"))
    }

    fn stage_actions(
        &self,
        stage: &Stage,
        from: &Listening,
        unheard: bool,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let actions = kit::sheet_actions();

        match stage {
            Stage::Recording { .. } | Stage::Asking => Some(actions.child(self.in_the_pane_ring(
                kit::button(
                    "listen-stop",
                    Some(Icon::Stop),
                    words::listen::stop(),
                    words::listen::stop_hint(),
                    Tone::Outlined,
                ),
                |this, _, cx| this.listen.update(cx, |listen, cx| listen.stop(cx)),
                cx,
            ))),
            Stage::Found(found) => Some(self.found_actions(actions, found, cx)),
            _ => {
                let ending = ending_of(stage, from)?;
                self.what_comes_next(ending.next, unheard, cx)
                    .map(|next| actions.child(next))
            }
        }
    }

    fn what_comes_next(
        &self,
        next: Next,
        unheard: bool,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let (label, saying) = match next {
            Next::Nothing => return None,
            Next::OpenTheSettings => {
                return Some(self.in_the_pane_ring(
                    kit::button(
                        "listen-settings",
                        Some(Icon::Settings),
                        words::listen::open_the_settings(),
                        words::listen::settings_hint(),
                        Tone::Primary,
                    ),
                    |this, _, cx| this.open_the_online_settings(cx),
                    cx,
                ));
            }
            Next::Listen => (words::listen::listen(), words::listen::listen_hint()),
            Next::ListenAgain => (
                words::listen::listen_again(),
                words::listen::listen_again_hint(),
            ),
        };

        Some(match unheard {
            true => kit::button_when(
                Press::Greyed,
                "listen-again",
                Some(Icon::Listen),
                label,
                words::listen::no_microphone_hint(),
                Tone::Primary,
            ),
            false => self.in_the_pane_ring(
                kit::button(
                    "listen-again",
                    Some(Icon::Listen),
                    label,
                    saying,
                    Tone::Primary,
                ),
                |this, _, cx| this.listen_now(cx),
                cx,
            ),
        })
    }

    fn found_actions(&self, actions: Div, found: &Found, cx: &mut Context<Self>) -> Div {
        let link = found.heard.link.clone();
        let finding = found.clone();

        actions
            .when_some(link, |actions, link| {
                actions.child(self.in_the_pane_ring(
                    kit::button(
                        "listen-open",
                        Some(Icon::Link),
                        words::listen::open(),
                        words::listen::open_hint(),
                        Tone::Ghost,
                    ),
                    move |_, _, cx| cx.open_url(&link),
                    cx,
                ))
            })
            .child(self.in_the_pane_ring(
                kit::button(
                    "listen-again",
                    Some(Icon::Listen),
                    words::listen::listen_again(),
                    words::listen::listen_again_hint(),
                    Tone::Outlined,
                ),
                |this, _, cx| this.listen_now(cx),
                cx,
            ))
            .child(self.in_the_pane_ring(
                kit::button(
                    "listen-find",
                    Some(Icon::Search),
                    words::listen::find(),
                    words::listen::find_hint(),
                    Tone::Primary,
                ),
                move |this, window, cx| this.find_what_was_heard(&finding, window, cx),
                cx,
            ))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "each is one reading of the listen model the settings are drawn from"
    )]
    fn listen_settings(
        &self,
        from: &Listening,
        microphones: &[(String, String)],
        unheard: bool,
        listening: bool,
        keeps: bool,
        following: &Following,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .pt_4()
            .border_t_1()
            .border_color(rgb(theme::soft_edge()))
            .child(self.listen_sources(from, microphones, unheard, listening, cx))
            .child(self.keeps_listening_row(keeps, following, cx))
    }

    fn listen_sources(
        &self,
        from: &Listening,
        microphones: &[(String, String)],
        unheard: bool,
        listening: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let pressable = !listening;
        let source = |id: &'static str, label: &'static str, hint: &'static str, held: bool| {
            let segment = kit::segment(id, label, held).names(hint);
            match pressable {
                true => segment,
                false => segment.cursor_default().greyed_for_a_reader(),
            }
        };
        let desktop = source(
            "listen-desktop",
            words::listen::desktop(),
            words::listen::desktop_hint(),
            !from.is_a_microphone(),
        );
        let microphone = source(
            "listen-microphone",
            words::listen::microphone(),
            words::listen::microphone_hint(),
            from.is_a_microphone(),
        );
        let sources = kit::segmented()
            .children(DESKTOP_IS_HEARD.then(|| match pressable {
                true => self.in_the_pane_ring(
                    desktop,
                    |this, _, cx| this.listen_from(Listening::Desktop, cx),
                    cx,
                ),
                false => desktop,
            }))
            .child(match pressable {
                true => self.in_the_pane_ring(
                    microphone,
                    |this, _, cx| this.listen_from(Listening::Microphone(None), cx),
                    cx,
                ),
                false => microphone,
            });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .when(listening, |sources| sources.opacity(kit::GREYED))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .child(setting_named(words::listen::source_label(), None))
                    .child(sources),
            )
            .when(from.is_a_microphone(), |sources| {
                sources.child(self.microphone_choices(from, microphones, unheard, pressable, cx))
            })
    }

    fn microphone_choices(
        &self,
        from: &Listening,
        microphones: &[(String, String)],
        unheard: bool,
        pressable: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if unheard {
            return div()
                .flex()
                .items_center()
                .gap_2()
                .child(icons::icon(
                    Icon::Alert,
                    theme::menu_icon(),
                    theme::failure(),
                ))
                .child(
                    div()
                        .id("listen-no-microphone")
                        .debug_selector(|| "listen-no-microphone".into())
                        .read_plainly(words::listen::no_microphone())
                        .child(sentence(words::listen::no_microphone())),
                )
                .into_any_element();
        }

        let held = match from {
            Listening::Microphone(Some(node)) => Some(node.as_str()),
            Listening::Microphone(None) | Listening::Desktop => None,
        };
        match microphones {
            [] => div().into_any_element(),
            [(_, only)] => under(only.clone()).into_any_element(),
            _ => microphones
                .iter()
                .enumerate()
                .fold(
                    div()
                        .id("listen-microphones")
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .max_h(px(MICROPHONES_DRAWN_AT_MOST))
                        .overflow_y_scroll()
                        .child(self.microphone_row(
                            kit::radio_row(
                                "listen-microphone-default",
                                words::listen::default_microphone(),
                                held.is_none(),
                            ),
                            Listening::Microphone(None),
                            pressable,
                            cx,
                        )),
                    |rows, (at, (name, said))| {
                        rows.child(self.microphone_row(
                            kit::radio_row(
                                ("listen-microphone-choice", at),
                                said.clone(),
                                held == Some(name.as_str()),
                            ),
                            Listening::Microphone(Some(NodeName::new(name.clone()))),
                            pressable,
                            cx,
                        ))
                    },
                )
                .into_any_element(),
        }
    }

    fn microphone_row(
        &self,
        row: Stateful<Div>,
        from: Listening,
        pressable: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match pressable {
            true => self.in_the_pane_ring(
                row,
                move |this, _, cx| this.listen_from(from.clone(), cx),
                cx,
            ),
            false => row.cursor_default().greyed_for_a_reader(),
        }
    }

    fn keeps_listening_row(
        &self,
        keeps: bool,
        following: &Following,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let row = div()
            .id("listen-keep")
            .named_as(
                Role::Switch,
                words::listen::keep_listening(),
                words::listen::keep_listening_hint(),
            )
            .thrown(keeps)
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .mx(px(-EARLIER_ROW_INSET))
            .px(px(EARLIER_ROW_INSET))
            .py_1p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .child(setting_named(
                words::listen::keep_listening(),
                Some(
                    following_said(following)
                        .unwrap_or_else(|| words::listen::keep_listening_under().to_owned()),
                ),
            ))
            .child(kit::switch("listen-keep-track", keeps))
            .names(words::listen::keep_listening_hint());

        self.in_the_pane_ring(row, |this, _, cx| this.keep_listening(cx), cx)
    }

    fn heard_earlier(&self, earlier: &[Found], cx: &mut Context<Self>) -> Div {
        let rows = earlier.iter().enumerate().fold(
            div()
                .id("listen-earlier-rows")
                .flex()
                .flex_col()
                .gap_0p5()
                .max_h(px(EARLIER_DRAWN_AT_MOST))
                .overflow_y_scroll()
                .track_scroll(&self.heard_earlier_scroll),
            |rows, (at, found)| {
                let finding = found.clone();
                rows.child(self.in_the_pane_ring(
                    earlier_row(at, &found.heard),
                    move |this, window, cx| this.find_what_was_heard(&finding, window, cx),
                    cx,
                ))
            },
        );

        div()
            .flex()
            .flex_col()
            .gap_1()
            .pt_4()
            .border_t_1()
            .border_color(rgb(theme::soft_edge()))
            .child(kit::eyebrow(words::listen::heard_earlier()))
            .child(
                div()
                    .relative()
                    .mx(px(-EARLIER_ROW_INSET))
                    .child(rows)
                    .child(
                        Scrollbars::of(cx)
                            .vertical("heard-earlier-scrollbar", self.heard_earlier_scroll.clone()),
                    ),
            )
    }
}

fn listen_stage(
    stage: &Stage,
    from: &Listening,
    microphones: &[(String, String)],
    length: Duration,
) -> impl IntoElement {
    let key = stage_key(stage);
    let (drawn, said, urgency) = match stage {
        Stage::Recording { hearing, from, nth } => {
            let said = recording_said(from, microphones);
            (
                recording(hearing, said.clone(), length, *nth),
                said,
                Urgency::Told,
            )
        }
        Stage::Asking => (asking(), words::listen::asking().to_owned(), Urgency::Told),
        Stage::Found(found) => (
            found_card(found),
            spoken::song(&found.heard.title, found.heard.artist.as_deref()).to_string(),
            Urgency::Told,
        ),
        _ => {
            let ending = ending_of(stage, from).unwrap_or(READY);
            (
                ended(ending),
                words::listen::ended(ending.headline(), ending.sentence()),
                ending.weight.urgency(),
            )
        }
    };

    motion::faded_in(
        div()
            .id(key.clone())
            .told(said, urgency)
            .flex()
            .flex_col()
            .child(drawn),
        ElementId::Name(format!("{key}-arrives").into()),
        motion::ARRIVES_OVER,
    )
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use gpui::TestAppContext;
    use resonate_engine::NodeName;
    use resonate_library::Library;
    use resonate_listen::Listening;

    use super::{
        DESKTOP_UNRECORDED, MICROPHONE_UNRECORDED, Next, OFFLINE, Weight, ending_of, seconds_left,
    };
    use crate::{Pane, SettingsCategory, driven::Driven, listening::Stage};

    fn catalog() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("a catalog in memory"))
    }

    fn stage(driven: &mut Driven) -> Stage {
        driven.read(|root, cx| root.listen.read(cx).stage())
    }

    #[test]
    fn every_way_a_listen_ends_is_a_headline_and_a_sentence_that_offers_what_comes_next() {
        let ended = [
            Stage::Idle,
            Stage::Unknown,
            Stage::Silent,
            Stage::CaptureFailed,
            Stage::Unreached,
            Stage::Refused,
            Stage::Offline,
            Stage::NoService,
        ];

        for stage in &ended {
            for from in [Listening::Desktop, Listening::Microphone(None)] {
                let ending = ending_of(stage, &from).expect("an ending says how it ended");

                assert!(!ending.headline().ends_with('.'), "{stage:?}");
                assert!(ending.sentence().ends_with(['.', '?']), "{stage:?}");
                assert_eq!(
                    ending.headline().chars().next().map(char::is_uppercase),
                    Some(true),
                    "{stage:?}"
                );
            }
        }
        assert!(ending_of(&Stage::Asking, &Listening::Desktop).is_none());
    }

    #[test]
    fn a_recording_that_failed_names_the_source_it_was_taken_from() {
        assert_eq!(
            ending_of(&Stage::CaptureFailed, &Listening::Desktop),
            Some(DESKTOP_UNRECORDED)
        );
        assert_eq!(
            ending_of(&Stage::CaptureFailed, &Listening::Microphone(None)),
            Some(MICROPHONE_UNRECORDED)
        );
        assert_eq!(DESKTOP_UNRECORDED.weight, Weight::Trouble);
        assert_eq!(OFFLINE.next, Next::OpenTheSettings);
    }

    #[test]
    fn the_seconds_left_count_down_to_nothing_and_round_up_on_the_way() {
        let ten = Duration::from_secs(10);

        assert_eq!(seconds_left(ten, 0.0), 10);
        assert_eq!(seconds_left(ten, 0.05), 10);
        assert_eq!(seconds_left(ten, 0.5), 5);
        assert_eq!(seconds_left(ten, 0.99), 1);
        assert_eq!(seconds_left(ten, 1.0), 0);
        assert_eq!(seconds_left(ten, 1.5), 0);
    }

    #[gpui::test]
    fn a_source_pressed_while_recording_is_refused_and_the_recording_names_its_own(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        driven.click("listen");
        let listen = driven.read(|root, _| root.listen.clone());
        driven.cx.update(|_, cx| {
            listen.update(cx, |listen, cx| {
                listen.recording_without_a_device();
                cx.notify();
            });
        });
        driven.settle();

        driven.click("listen-microphone");

        assert_eq!(
            driven.read(|root, cx| root.listen.read(cx).from().clone()),
            Listening::Desktop
        );
        assert!(matches!(
            stage(&mut driven),
            Stage::Recording {
                from: Listening::Desktop,
                ..
            }
        ));
    }

    #[gpui::test]
    fn a_microphone_nobody_connected_is_said_to_be_missing_and_the_default_one_can_be_chosen_again(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        let listen = driven.read(|root, _| root.listen.clone());
        let heard_from =
            |driven: &mut Driven| driven.read(|root, cx| root.listen.read(cx).from().clone());
        let hearing = |driven: &mut Driven, microphones: Vec<(String, String)>| {
            let listen = listen.clone();
            driven.cx.update(|_, cx| {
                listen.update(cx, |listen, cx| {
                    listen.hearing_of(microphones);
                    cx.notify();
                });
            });
            driven.settle();
        };

        driven.click("listen");
        driven.click("listen-microphone");
        hearing(&mut driven, Vec::new());
        driven.bounds_of("listen-no-microphone");

        hearing(
            &mut driven,
            vec![
                ("alsa_input.desk".to_owned(), "Desk microphone".to_owned()),
                ("alsa_input.headset".to_owned(), "Headset".to_owned()),
            ],
        );
        assert!(driven.cx.debug_bounds("listen-no-microphone").is_none());
        driven.click("listen-microphone-choice-0");
        assert_eq!(
            heard_from(&mut driven),
            Listening::Microphone(Some(NodeName::new("alsa_input.desk")))
        );
        driven.click("listen-microphone-default");
        assert_eq!(heard_from(&mut driven), Listening::Microphone(None));
    }

    #[gpui::test]
    fn online_off_offers_the_online_settings_and_leaves_the_sheet_for_them(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());

        driven.click("listen");
        assert!(matches!(stage(&mut driven), Stage::Offline));
        driven.click("listen-settings");

        assert!(!driven.read(|root, _| root.listening_open));
        assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
        assert_eq!(
            driven.read(|root, _| root.settings_category),
            SettingsCategory::Online
        );
    }

    #[gpui::test]
    fn online_switched_on_in_this_run_is_not_said_to_be_off(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());

        driven.click("listen");
        assert!(matches!(stage(&mut driven), Stage::Offline));

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.close_the_listener(cx);
                root.set_online(true, cx);
                root.open_the_listener(cx);
            });
        });
        driven.settle();
        assert!(matches!(stage(&mut driven), Stage::NoService));
    }
}
