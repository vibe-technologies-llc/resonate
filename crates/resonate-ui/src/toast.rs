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
    views::{
        RootView,
        spoken::{Spoken, Urgency},
    },
};

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
}

impl Global for Toaster {}

#[derive(Clone)]
struct Toast {
    notice: Notice,
    times: u32,
    serial: u64,
    leaving: bool,
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

pub(crate) fn is_showing(cx: &App) -> bool {
    cx.try_global::<Toaster>()
        .and_then(|toaster| toaster.showing.as_ref())
        .is_some_and(|toast| !toast.leaving)
}

fn show(Waiting { notice, times }: Waiting, cx: &mut App) {
    let others_wait = !cx.default_global::<Toaster>().waiting.is_empty();
    let lingers = lingers(&notice, others_wait);
    let fading = Some(cx.spawn(async move |cx| {
        cx.background_executor().timer(lingers).await;
        cx.update(withdraw);
    }));

    let toaster = cx.default_global::<Toaster>();
    toaster.told += 1;
    toaster.showing = Some(Toast {
        notice,
        times,
        serial: toaster.told,
        leaving: false,
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

pub(crate) fn drawn(lifted: bool, cx: &mut Context<RootView>) -> Option<AnimationElement<Div>> {
    let toast = cx.try_global::<Toaster>()?.showing.clone()?;
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
                .told(said.clone(), urgency)
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
        Library::AlreadyAsking { .. } => "another Resonate is already asking the same services",
        Library::Unreachable { .. } => "the service couldn't be reached",
        Library::Refused { .. } => "the service turned the request down",
        Library::Io { .. } | Library::Move { .. } => "a file couldn't be read or written",
        Library::NoVault => "there's no vault yet",
        Library::DestinationNotADirectory { .. } => "the music folder isn't there",
        Library::DuplicatePlaylist { .. } => "a playlist already has that name",
        Library::UnnamedPlaylist => "it needs a name",
        Library::RootNotADirectory { .. } => "that isn't a folder",
        Library::RootInsideRoot { .. } => "that folder is already in the library",
        Library::NotARoot { .. } => "that folder isn't in the library",
        Library::NotAPlaylistFile { .. } | Library::UnreadablePlaylistFile { .. } => {
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
        Eq::Refused { .. } => Some("AutoEq turned the request down"),
        Eq::Unreadable { .. } => Some("AutoEq's answer couldn't be read"),
        Eq::Store { .. } => Some("the profile file couldn't be read or written"),
        Eq::NotAProfile { .. } => Some("that file isn't an equaliser profile"),
        Eq::NoSuchProfile => Some("that profile isn't kept any more"),
        Eq::NameNotUsable => Some("that name can't be used for a profile"),
        Eq::TooLarge { .. } => Some("the file is too large"),
        Eq::TooManyLines { .. } => Some("the file holds too many lines"),
        Eq::MeasurementTooLarge { .. } => Some("the file is too large to be a measurement"),
        Eq::NoMeasuredPoints => Some("no line of that file reads as a frequency and a level"),
        Eq::TooFewMeasuredPoints { .. } => Some("that file holds too few points"),
        Eq::ResponseMissesTheMidBand { .. } => {
            Some("the measurement and its target must both reach from 500 Hz to 2 kHz")
        }
        Eq::NothingToCorrect { .. } => Some("that measurement already sits on its target"),
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

pub(crate) const fn waits_for_a_device(cause: Cause) -> &'static str {
    match cause {
        Cause::NoDevice => {
            "No speakers or headphones to play through — playing on once one is there"
        }
        _ => "The audio device stopped playing — playing on once one is there",
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
