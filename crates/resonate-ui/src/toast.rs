use std::{collections::VecDeque, time::Duration};

use gpui::{
    Animation, AnimationElement, AnimationExt, App, BoxShadow, Context, Div, FocusHandle, Global,
    Pixels, SharedString, Task, Window, div, ease_in_out, ease_out_quint, point, prelude::*, px,
    rgb,
};
use resonate_engine::{Cause, CommandKind};

use crate::{
    app::{CONTROL_CONTEXT, PressControl},
    icons::{self, Icon},
    models::{Notice, Tone},
    theme,
    views::{
        RootView, focus,
        spoken::{Spoken, Urgency},
    },
    words,
};

const LINGERS_IN_STEPS_OF: Duration = Duration::from_millis(100);

const LINGER_AT_LEAST: Duration = Duration::from_secs(6);

const TROUBLE_LINGERS_AT_LEAST: Duration = Duration::from_secs(10);

const LINGER_IN_A_BURST_AT_LEAST: Duration = Duration::from_secs(2);

const READING_A_WORD: Duration = Duration::from_millis(500);

const ARRIVAL: Duration = Duration::from_millis(220);

const WITHDRAWAL: Duration = Duration::from_millis(220);

const RISE: Pixels = px(10.0);

const WAITING_AT_MOST: usize = 4;

const WIDEST: Pixels = px(520.0);

const ABOVE_ANOTHER_PILL: f32 = 44.0;

#[derive(Default)]
pub(crate) struct Toaster {
    showing: Option<Toast>,
    waiting: VecDeque<Waiting>,
    told: u64,
    fading: Option<Task<()>>,
    hovered: bool,
    focused: bool,
}

impl Global for Toaster {}

#[derive(Clone)]
struct Toast {
    notice: Notice,
    times: u32,
    serial: u64,
    leaving: bool,
    focus: Option<FocusHandle>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lingering {
    Gone,
    Held,
    Free,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Waiting {
    notice: Notice,
    times: u32,
}

impl Waiting {
    const fn once(notice: Notice) -> Self {
        Self { notice, times: 1 }
    }
}

fn said_with_its_count(notice: &Notice, times: u32) -> String {
    match notice.tone() {
        Tone::Trouble if times > 1 => format!("{} (×{times})", notice.text()),
        _ => notice.text().to_owned(),
    }
}

impl Toaster {
    fn lingering(&self, serial: u64) -> Lingering {
        match self.showing.as_ref() {
            Some(toast) if toast.serial == serial && !toast.leaving => {
                if self.hovered || self.focused {
                    Lingering::Held
                } else {
                    Lingering::Free
                }
            }
            _ => Lingering::Gone,
        }
    }

    fn counts_a_repeat_of(&mut self, notice: &Notice) -> bool {
        let standing = self
            .showing
            .as_mut()
            .filter(|toast| !toast.leaving && toast.notice == *notice)
            .map(|toast| &mut toast.times);
        let waiting = self
            .waiting
            .iter_mut()
            .find(|waiting| waiting.notice == *notice)
            .map(|waiting| &mut waiting.times);

        match standing.or(waiting) {
            Some(times) => {
                *times += 1;
                true
            }
            None => false,
        }
    }
}

pub(crate) fn tell(notice: Notice, cx: &mut App) {
    let toaster = cx.default_global::<Toaster>();
    if toaster.counts_a_repeat_of(&notice) {
        cx.refresh_windows();
        return;
    }
    if toaster.showing.is_none() {
        show(Waiting::once(notice), cx);
        return;
    }
    wait(&mut toaster.waiting, notice);
    withdraw(cx);
}

fn wait(waiting: &mut VecDeque<Waiting>, notice: Notice) {
    if waiting.len() >= WAITING_AT_MOST {
        let makes_room = waiting
            .iter()
            .position(|waiting| waiting.notice.tone() != Tone::Trouble);
        match makes_room {
            Some(at) => {
                waiting.remove(at);
            }
            None if notice.tone() != Tone::Trouble => return,
            None => {}
        }
    }
    waiting.push_back(Waiting::once(notice));
}

fn reading_time(text: &str) -> Duration {
    READING_A_WORD * u32::try_from(text.split_whitespace().count()).unwrap_or(u32::MAX)
}

fn lingers(notice: &Notice, others_wait: bool) -> Duration {
    let reading = reading_time(notice.text());
    match (notice.tone(), others_wait) {
        (Tone::Trouble, false) => TROUBLE_LINGERS_AT_LEAST.max(reading),
        (_, false) => LINGER_AT_LEAST.max(reading),
        (_, true) => LINGER_IN_A_BURST_AT_LEAST.max(reading),
    }
}

pub(crate) fn dismiss(cx: &mut App) {
    withdraw(cx);
}

#[cfg(test)]
pub(crate) fn showing_text(cx: &App) -> Option<String> {
    let toast = cx.try_global::<Toaster>()?.showing.as_ref()?;
    Some(said_with_its_count(&toast.notice, toast.times))
}

pub(crate) fn is_showing(cx: &App) -> bool {
    cx.try_global::<Toaster>()
        .and_then(|toaster| toaster.showing.as_ref())
        .is_some_and(|toast| !toast.leaving)
}

fn show(Waiting { notice, times }: Waiting, cx: &mut App) {
    let others_wait = !cx.default_global::<Toaster>().waiting.is_empty();
    let lingers = lingers(&notice, others_wait);
    let serial = cx.default_global::<Toaster>().told + 1;
    let fading = Some(cx.spawn(async move |cx| {
        let mut remaining = lingers;
        loop {
            cx.background_executor().timer(LINGERS_IN_STEPS_OF).await;
            match cx.update(|cx| cx.default_global::<Toaster>().lingering(serial)) {
                Lingering::Gone => return,
                Lingering::Held => {}
                Lingering::Free => {
                    remaining = remaining.saturating_sub(LINGERS_IN_STEPS_OF);
                    if remaining.is_zero() {
                        cx.update(withdraw);
                        return;
                    }
                }
            }
        }
    }));
    let focus = Some(cx.focus_handle().tab_stop(true));

    let toaster = cx.default_global::<Toaster>();
    toaster.told = serial;
    toaster.hovered = false;
    toaster.focused = false;
    toaster.showing = Some(Toast {
        notice,
        times,
        serial,
        leaving: false,
        focus,
    });
    toaster.fading = fading;
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
        cx.update(|cx| {
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

pub(crate) fn drawn(
    lifted: bool,
    window: &Window,
    cx: &mut Context<RootView>,
) -> Option<AnimationElement<Div>> {
    let toaster = cx.try_global::<Toaster>()?;
    let toast = toaster.showing.clone()?;
    let focused = toast
        .focus
        .as_ref()
        .is_some_and(|focus| focus.is_focused(window));

    if toaster.focused != focused {
        cx.default_global::<Toaster>().focused = focused;
    }
    let (icon, ink, urgency) = match toast.notice.tone() {
        Tone::Trouble => (Icon::Alert, theme::failure(), Urgency::Warned),
        Tone::Done => (Icon::Check, theme::done(), Urgency::Told),
        Tone::Noted => (Icon::Info, theme::accent(), Urgency::Told),
    };
    let said = SharedString::from(said_with_its_count(&toast.notice, toast.times));
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
                .id(("toast", toast.serial as usize))
                .debug_selector(|| "toast".to_owned())
                .told(said.clone(), urgency)
                .when_some(toast.focus.as_ref(), |pill, handle| {
                    pill.track_focus(handle)
                        .key_context(CONTROL_CONTEXT)
                        .focus_visible(focus::lit)
                        .on_action(cx.listener(|_, _: &PressControl, _, cx| dismiss(cx)))
                })
                .on_hover(|hovering, _, cx| {
                    cx.default_global::<Toaster>().hovered = *hovering;
                })
                .flex()
                .items_center()
                .gap_2()
                .max_w(WIDEST)
                .px_4()
                .py_2()
                .rounded_full()
                .bg(rgb(theme::raised()))
                .border_1()
                .border_color(rgb(theme::soft_edge()))
                .shadow(vec![BoxShadow {
                    color: theme::shadow(0.45),
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(16.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| dismiss(cx)))
                .child(icons::icon(icon, theme::row_control_icon(), ink))
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::text()))
                        .child(said),
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
        Some(why) => words::toast::could_not_because(doing, why),
        None => words::toast::could_not(doing),
    })
}

fn standing_in_the_way(error: &resonate_library::Error) -> Option<String> {
    use resonate_library::Error as Library;

    let why = match error {
        Library::UnknownLayoutField { field } => {
            return Some(words::toast::unknown_layout_field(field.to_string()));
        }
        Library::LayoutSyntax { .. } => words::toast::layout_syntax(),
        Library::LayoutEscapes { .. } => words::toast::layout_escapes(),
        Library::AlreadyWalking => words::toast::already_walking(),
        Library::AlreadyAsking { .. } => words::toast::already_asking(),
        Library::Unreachable { .. } => words::toast::unreachable(),
        Library::Refused { .. } => words::toast::refused(),
        Library::Io { .. } | Library::Move { .. } => words::toast::unreadable_or_unwritten(),
        Library::NoVault => words::toast::no_vault(),
        Library::DestinationNotADirectory { .. } => words::toast::no_music_folder(),
        Library::DuplicatePlaylist { .. } => words::toast::duplicate_playlist(),
        Library::UnnamedPlaylist => words::toast::unnamed_playlist(),
        Library::RootNotADirectory { .. } => words::toast::not_a_folder(),
        Library::RootInsideRoot { .. } => words::toast::root_inside_root(),
        Library::NotARoot { .. } => words::toast::not_a_root(),
        Library::NotAPlaylistFile { .. } | Library::UnreadablePlaylistFile { .. } => {
            words::toast::not_a_playlist_file()
        }
        _ => return None,
    };
    Some(why.to_owned())
}

pub(crate) fn eq_could_not(doing: &str, error: &resonate_eq::Error) -> Notice {
    use resonate_eq::Error as Eq;

    let why = match error {
        Eq::Unreachable { .. } => Some(words::toast::autoeq_unreachable()),
        Eq::Refused { .. } => Some(words::toast::autoeq_refused()),
        Eq::Unreadable { .. } => Some(words::toast::autoeq_unreadable()),
        Eq::Store { .. } => Some(words::toast::profile_store()),
        Eq::NotAProfile { .. } => Some(words::toast::not_a_profile()),
        Eq::NoSuchProfile => Some(words::toast::no_such_profile()),
        Eq::NameNotUsable => Some(words::toast::name_not_usable()),
        Eq::TooLarge { .. } => Some(words::toast::too_large()),
        Eq::TooManyLines { .. } => Some(words::toast::too_many_lines()),
        Eq::MeasurementTooLarge { .. } => Some(words::toast::measurement_too_large()),
        Eq::NoMeasuredPoints => Some(words::toast::no_measured_points()),
        Eq::TooFewMeasuredPoints { .. } => Some(words::toast::too_few_measured_points()),
        Eq::ResponseMissesTheMidBand { .. } => Some(words::toast::misses_the_mid_band()),
        Eq::NothingToCorrect { .. } => Some(words::toast::nothing_to_correct()),
        _ => None,
    };
    Notice::Trouble(match why {
        Some(why) => words::toast::could_not_because(doing, why),
        None => words::toast::could_not(doing),
    })
}

pub(crate) fn would_not_play(cause: Cause, title: &str) -> String {
    match cause {
        Cause::Unreadable => words::toast::not_found(title),
        Cause::Unsupported => words::toast::unsupported(title),
        Cause::Damaged => words::toast::damaged(title),
        Cause::CannotSeek => words::toast::cannot_seek_in(title),
        cause => plainly(cause).to_owned(),
    }
}

pub(crate) fn waits_for_a_device(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => words::toast::waits_for_a_device(),
        _ => words::toast::waits_for_the_device(),
    }
}

pub(crate) fn would_not_do(command: CommandKind, cause: Cause) -> String {
    match cause {
        Cause::Unreadable | Cause::Unsupported | Cause::Damaged => {
            words::toast::could_not_open().to_owned()
        }
        Cause::CannotSeek => words::toast::cannot_seek().to_owned(),
        Cause::NothingPlaying => match command {
            CommandKind::Pause | CommandKind::Seek | CommandKind::SeekBy => {
                words::toast::nothing_playing().to_owned()
            }
            _ => words::toast::nothing_queued().to_owned(),
        },
        cause => plainly(cause).to_owned(),
    }
}

fn plainly(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => words::toast::no_device(),
        Cause::DeviceGone => words::toast::device_gone(),
        Cause::SoundServer => words::toast::sound_server(),
        Cause::DeviceRefused => words::toast::device_refused(),
        Cause::QueueMoved => words::toast::queue_moved(),
        Cause::PlayerStopped => words::toast::player_stopped(),
        Cause::NothingPlaying => words::toast::nothing_playing(),
        Cause::CannotSeek => words::toast::cannot_seek(),
        Cause::Unreadable => words::toast::a_track_not_found(),
        Cause::Unsupported => words::toast::a_track_unsupported(),
        Cause::Damaged => words::toast::a_track_damaged(),
        Cause::HeldByAnotherZone => words::toast::held_by_another_zone(),
        Cause::ZoneRefused => words::toast::zone_refused(),
        Cause::NotHandedOver => words::toast::not_handed_over(),
        Cause::NothingToCompare => words::toast::nothing_to_compare(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trouble(text: &str) -> Notice {
        Notice::Trouble(text.to_owned())
    }

    fn noted(text: &str) -> Notice {
        Notice::Noted(text.to_owned())
    }

    fn notices_of(waiting: &VecDeque<Waiting>) -> Vec<Notice> {
        waiting
            .iter()
            .map(|waiting| waiting.notice.clone())
            .collect()
    }

    fn standing(notice: Notice) -> Toaster {
        Toaster {
            showing: Some(Toast {
                notice,
                times: 1,
                serial: 1,
                leaving: false,
                focus: None,
            }),
            ..Toaster::default()
        }
    }

    #[test]
    fn a_trouble_told_again_while_it_shows_is_counted_not_shown_twice() {
        let mut toaster = standing(trouble("Couldn't read a cover"));

        assert!(toaster.counts_a_repeat_of(&trouble("Couldn't read a cover")));
        assert!(toaster.counts_a_repeat_of(&trouble("Couldn't read a cover")));
        assert!(!toaster.counts_a_repeat_of(&trouble("Couldn't read a portrait")));

        let showing = toaster.showing.expect("the toast still shows");
        assert_eq!(showing.times, 3);
        assert_eq!(
            said_with_its_count(&showing.notice, showing.times),
            "Couldn't read a cover (×3)"
        );
    }

    #[test]
    fn a_trouble_told_again_while_it_waits_is_counted_on_the_one_waiting() {
        let mut toaster = standing(trouble("first"));
        wait(&mut toaster.waiting, trouble("second"));

        assert!(toaster.counts_a_repeat_of(&trouble("second")));

        assert_eq!(toaster.waiting.len(), 1);
        assert_eq!(toaster.waiting[0].times, 2);
    }

    #[test]
    fn a_toast_on_its_way_out_does_not_take_a_repeat_and_a_plain_notice_shows_no_count() {
        let mut toaster = standing(trouble("leaving"));
        toaster.showing.as_mut().expect("showing").leaving = true;

        assert!(!toaster.counts_a_repeat_of(&trouble("leaving")));
        assert_eq!(said_with_its_count(&noted("plain"), 4), "plain");
        assert_eq!(said_with_its_count(&trouble("once"), 1), "once");
    }

    #[test]
    fn a_full_line_drops_its_oldest_plain_notice_for_a_newer_one_and_never_a_trouble() {
        let mut waiting = VecDeque::new();
        for told in [trouble("a"), noted("b"), noted("c"), trouble("d")] {
            wait(&mut waiting, told);
        }

        wait(&mut waiting, trouble("e"));

        assert_eq!(
            notices_of(&waiting),
            [trouble("a"), noted("c"), trouble("d"), trouble("e")]
        );
    }

    #[test]
    fn a_full_line_of_troubles_turns_a_plain_notice_away_and_keeps_every_trouble() {
        let mut waiting = VecDeque::new();
        for told in ["a", "b", "c", "d"] {
            wait(&mut waiting, trouble(told));
        }

        wait(&mut waiting, noted("plain"));
        assert_eq!(waiting.len(), WAITING_AT_MOST);
        assert!(!notices_of(&waiting).contains(&noted("plain")));

        wait(&mut waiting, trouble("e"));
        assert_eq!(waiting.len(), WAITING_AT_MOST + 1);
        assert_eq!(waiting.back(), Some(&Waiting::once(trouble("e"))));
        assert_eq!(waiting.front(), Some(&Waiting::once(trouble("a"))));
    }

    #[test]
    fn a_trouble_lingers_longer_than_a_plain_notice_yet_never_stays() {
        let failed = trouble("Couldn't read a cover");

        assert_eq!(lingers(&failed, false), TROUBLE_LINGERS_AT_LEAST);
        assert!(TROUBLE_LINGERS_AT_LEAST > LINGER_AT_LEAST);
        assert_eq!(lingers(&failed, true), LINGER_IN_A_BURST_AT_LEAST);
    }

    #[test]
    fn a_plain_notice_lingers_a_while_and_less_in_a_burst() {
        let short = noted("Scan finished");

        assert_eq!(lingers(&short, false), LINGER_AT_LEAST);
        assert_eq!(lingers(&short, true), LINGER_IN_A_BURST_AT_LEAST);
    }

    #[test]
    fn a_long_notice_lingers_for_as_long_as_it_takes_to_read() {
        let long = noted(
            "Took 34 tracks out of the queue and kept them so that they can be put back where \
             they stood",
        );
        let words = long.text().split_whitespace().count() as u32;

        assert_eq!(lingers(&long, false), READING_A_WORD * words);
        assert!(READING_A_WORD * words > LINGER_AT_LEAST);
        assert_eq!(lingers(&long, true), READING_A_WORD * words);
    }

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
            Cause::HeldByAnotherZone,
            Cause::ZoneRefused,
            Cause::NotHandedOver,
            Cause::NothingToCompare,
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
