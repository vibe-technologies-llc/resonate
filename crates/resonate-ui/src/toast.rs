use std::{collections::VecDeque, time::Duration};

use gpui::{
    Animation, AnimationElement, AnimationExt, App, BoxShadow, Context, Div, Global, Pixels,
    SharedString, Task, div, ease_in_out, ease_out_quint, hsla, point, prelude::*, px, rgb,
};
use resonate_engine::{Cause, CommandKind};

use crate::{
    icons::{self, Icon},
    models::{Notice, Tone},
    theme,
    views::RootView,
};

const LINGER: Duration = Duration::from_secs(6);

const LINGER_IN_A_BURST: Duration = Duration::from_secs(2);

const ARRIVAL: Duration = Duration::from_millis(220);

const WITHDRAWAL: Duration = Duration::from_millis(220);

const RISE: Pixels = px(10.0);

const WAITING_AT_MOST: usize = 4;

const WIDEST: Pixels = px(520.0);

const ABOVE_ANOTHER_PILL: f32 = 44.0;

#[derive(Default)]
pub(crate) struct Toaster {
    showing: Option<Toast>,
    waiting: VecDeque<Notice>,
    told: u64,
    fading: Option<Task<()>>,
}

impl Global for Toaster {}

#[derive(Clone)]
struct Toast {
    notice: Notice,
    serial: u64,
    leaving: bool,
}

impl Toaster {
    fn already_says(&self, notice: &Notice) -> bool {
        self.showing
            .as_ref()
            .is_some_and(|toast| !toast.leaving && toast.notice == *notice)
            || self.waiting.contains(notice)
    }
}

pub(crate) fn tell(notice: Notice, cx: &mut App) {
    let toaster = cx.default_global::<Toaster>();
    if toaster.already_says(&notice) {
        return;
    }
    if toaster.showing.is_none() {
        show(notice, cx);
        return;
    }
    if toaster.waiting.len() < WAITING_AT_MOST {
        toaster.waiting.push_back(notice);
    }
    withdraw(cx);
}

pub(crate) fn dismiss(cx: &mut App) {
    withdraw(cx);
}

pub(crate) fn is_showing(cx: &App) -> bool {
    cx.try_global::<Toaster>()
        .and_then(|toaster| toaster.showing.as_ref())
        .is_some_and(|toast| !toast.leaving)
}

fn show(notice: Notice, cx: &mut App) {
    let lingers = if cx.default_global::<Toaster>().waiting.is_empty() {
        LINGER
    } else {
        LINGER_IN_A_BURST
    };
    let fading = cx.spawn(async move |cx| {
        cx.background_executor().timer(lingers).await;
        let _ = cx.update(withdraw);
    });

    let toaster = cx.default_global::<Toaster>();
    toaster.told += 1;
    toaster.showing = Some(Toast {
        notice,
        serial: toaster.told,
        leaving: false,
    });
    toaster.fading = Some(fading);
}

fn withdraw(cx: &mut App) {
    let standing = cx
        .try_global::<Toaster>()
        .and_then(|toaster| toaster.showing.as_ref())
        .is_some_and(|toast| !toast.leaving);
    if !standing {
        return;
    }

    let fading = cx.spawn(async move |cx| {
        cx.background_executor().timer(WITHDRAWAL).await;
        let _ = cx.update(|cx| {
            let toaster = cx.default_global::<Toaster>();
            toaster.showing = None;
            if let Some(waiting) = toaster.waiting.pop_front() {
                show(waiting, cx);
            }
        });
    });

    let toaster = cx.default_global::<Toaster>();
    if let Some(toast) = toaster.showing.as_mut() {
        toast.leaving = true;
    }
    toaster.fading = Some(fading);
}

pub(crate) fn drawn(lifted: bool, cx: &mut Context<RootView>) -> Option<AnimationElement<Div>> {
    let toast = cx.try_global::<Toaster>()?.showing.clone()?;
    let (icon, ink) = match toast.notice.tone() {
        Tone::Trouble => (Icon::Alert, theme::failure()),
        Tone::Done => (Icon::Check, theme::done()),
        Tone::Noted => (Icon::Info, theme::accent()),
    };
    let lift = if lifted { ABOVE_ANOTHER_PILL } else { 0.0 };
    let resting = px(theme::transport_height() + theme::type_ahead_lift() + lift);

    let floated = div()
        .absolute()
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .id("toast")
                .flex()
                .items_center()
                .gap_2()
                .max_w(WIDEST)
                .px_4()
                .py_2()
                .rounded_full()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::outline()))
                .shadow(vec![BoxShadow {
                    color: hsla(0.0, 0.0, 0.0, 0.45),
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(16.0),
                    spread_radius: px(0.0),
                }])
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| dismiss(cx)))
                .child(icons::icon(icon, theme::row_control_icon(), ink))
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::text()))
                        .child(SharedString::from(toast.notice.text().to_owned())),
                ),
        );

    Some(if toast.leaving {
        floated.with_animation(
            SharedString::from(format!("toast-out-{}", toast.serial)),
            Animation::new(WITHDRAWAL).with_easing(ease_in_out),
            move |floated, delta| {
                floated
                    .bottom(resting + RISE * delta)
                    .opacity(1.0 - delta.clamp(0.0, 1.0))
            },
        )
    } else {
        floated.with_animation(
            SharedString::from(format!("toast-in-{}", toast.serial)),
            Animation::new(ARRIVAL).with_easing(ease_out_quint()),
            move |floated, delta| {
                floated
                    .bottom(resting - RISE * (1.0 - delta))
                    .opacity(delta.clamp(0.0, 1.0))
            },
        )
    })
}

pub(crate) fn could_not(doing: &str, error: &resonate_library::Error) -> Notice {
    Notice::Trouble(match standing_in_the_way(error) {
        Some(why) => format!("Couldn't {doing} — {why}"),
        None => format!("Couldn't {doing}"),
    })
}

fn standing_in_the_way(error: &resonate_library::Error) -> Option<String> {
    use resonate_library::Error as Library;

    let why = match error {
        Library::UnknownLayoutField { field } => {
            return Some(format!(
                "the layout asks for {field}, which a track isn't known by"
            ));
        }
        Library::LayoutSyntax { .. } => "a brace in the layout isn't closed or doubled",
        Library::LayoutEscapes { .. } => "the layout reaches outside the folder it files under",
        Library::AlreadyWalking => "another library task is still running",
        Library::Unreachable { .. } => "the service couldn't be reached",
        Library::Refused { .. } => "the service turned the request down",
        Library::Io { .. } | Library::Move { .. } => "a file couldn't be read or written",
        Library::NoVault => "there's no vault yet",
        Library::DuplicatePlaylist { .. } => "a playlist already has that name",
        Library::UnnamedPlaylist => "it needs a name",
        Library::RootNotADirectory { .. } => "that isn't a folder",
        Library::RootInsideRoot { .. } => "that folder is already in the library",
        Library::NotARoot { .. } => "that folder isn't in the library",
        Library::NotAPlaylistFile { .. } | Library::NonUtf8PlaylistFile { .. } => {
            "that file isn't a playlist Resonate can read"
        }
        _ => return None,
    };
    Some(why.to_owned())
}

pub(crate) fn eq_could_not(doing: &str, error: &resonate_eq::Error) -> Notice {
    use resonate_eq::Error as Eq;

    let why = match error {
        Eq::Unreachable { .. } => Some("AutoEq couldn't be reached"),
        Eq::Unreadable { .. } => Some("AutoEq's answer couldn't be read"),
        Eq::Store { .. } => Some("the profile file couldn't be read or written"),
        Eq::NotAProfile { .. } => Some("that file isn't an equaliser profile"),
        Eq::NoSuchProfile => Some("that profile isn't kept any more"),
        Eq::NameNotUsable => Some("that name can't be used for a profile"),
        Eq::TooLarge { .. } => Some("the file is too large"),
        _ => None,
    };
    Notice::Trouble(match why {
        Some(why) => format!("Couldn't {doing} — {why}"),
        None => format!("Couldn't {doing}"),
    })
}

pub(crate) fn would_not_play(cause: Cause, title: &str) -> String {
    match cause {
        Cause::Unreadable => format!("Couldn't find “{title}” — it may have been moved or deleted"),
        Cause::Unsupported => format!("“{title}” is in a format Resonate can't play"),
        Cause::Damaged => format!("Couldn't play “{title}” — the file looks damaged"),
        Cause::CannotSeek => format!("Couldn't jump around in “{title}”"),
        cause => plainly(cause).to_owned(),
    }
}

pub(crate) fn would_not_do(command: CommandKind, cause: Cause) -> String {
    match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => {
            "That track couldn't be opened".to_owned()
        }
        Cause::CannotSeek => "This track can't be skipped through".to_owned(),
        Cause::NothingPlaying => match command {
            CommandKind::Pause | CommandKind::Seek | CommandKind::SeekBy => {
                "Nothing is playing right now".to_owned()
            }
            _ => "There's nothing in the queue to play".to_owned(),
        },
        cause => plainly(cause).to_owned(),
    }
}

const fn plainly(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => "No speakers or headphones to play through",
        Cause::DeviceGone => "The audio device was unplugged or switched off",
        Cause::SoundServer => "Lost touch with the sound system — trying again",
        Cause::DeviceRefused => "The audio device wouldn't play this track's format",
        Cause::QueueMoved => "The queue changed before that could happen",
        Cause::PlayerStopped => "The player stopped responding — restart Resonate",
        Cause::NothingPlaying => "Nothing is playing right now",
        Cause::CannotSeek => "This track can't be skipped through",
        Cause::Unreadable => "A track couldn't be found",
        Cause::Unsupported => "A track is in a format Resonate can't play",
        Cause::Damaged => "A track couldn't be played",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_library_task_that_failed_says_what_it_was_doing_and_why_in_plain_words() {
        assert_eq!(
            could_not("start the scan", &resonate_library::Error::AlreadyWalking),
            Notice::Trouble(
                "Couldn't start the scan — another library task is still running".to_owned()
            )
        );
        assert_eq!(
            could_not(
                "drop that library folder",
                &resonate_library::Error::UnnamedPlaylist
            ),
            Notice::Trouble("Couldn't drop that library folder — it needs a name".to_owned())
        );
    }

    #[test]
    fn a_track_that_will_not_play_is_named_and_the_reason_is_said_plainly() {
        assert_eq!(
            would_not_play(Cause::Unreadable, "Echoes"),
            "Couldn't find “Echoes” — it may have been moved or deleted"
        );
        assert_eq!(
            would_not_play(Cause::NoDevice, "Echoes"),
            "No speakers or headphones to play through"
        );
    }

    #[test]
    fn a_refused_command_says_what_stood_in_its_way_rather_than_what_it_was_called() {
        assert_eq!(
            would_not_do(CommandKind::Pause, Cause::NothingPlaying),
            "Nothing is playing right now"
        );
        assert_eq!(
            would_not_do(CommandKind::Next, Cause::NothingPlaying),
            "There's nothing in the queue to play"
        );
        for cause in [
            Cause::Unreadable,
            Cause::Unsupported,
            Cause::Damaged,
            Cause::NoDevice,
            Cause::DeviceGone,
            Cause::SoundServer,
            Cause::DeviceRefused,
            Cause::CannotSeek,
            Cause::QueueMoved,
            Cause::NothingPlaying,
            Cause::PlayerStopped,
        ] {
            let said = would_not_do(CommandKind::Play, cause);
            assert!(
                !said.ends_with('.'),
                "{said} reads as a sentence, not a notice"
            );
            assert!(
                !said.contains("Error"),
                "{said} speaks the engine's language"
            );
        }
    }
}
