use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use gpui::{
    AnimationElement, BoxShadow, Context, Div, ExternalPaths, SharedString, Stateful, div, hsla,
    point, prelude::*, px, relative, rgb, rgba,
};
use parking_lot::Mutex;
use resonate_core::names_audio;
use resonate_library::{
    Dropped, Layout, Looks, TakeInHandle, TakeInOptions, TakeInProgress, TakeInSummary,
    TakenPassing, take_in, weigh,
};

use crate::{
    Notice, ResonateApp, format,
    icons::{self, Icon},
    motion, theme, toast,
    views::{
        kit::{self, EndsInAnEllipsis as _},
        root::RootView,
        settings::Category,
    },
};

const DRAG_LOOKED_AT_EVERY: Duration = Duration::from_millis(100);

const COPY_LOOKED_AT_EVERY: Duration = Duration::from_millis(150);

const FOLDER_LOOKED_AT_EVERY: Duration = Duration::from_secs(2);

const NAMES_SHOWN: usize = 6;

const CARD_WIDTH: f32 = 480.0;

const ABOVE_A_TOAST: f32 = 56.0;

const NO_FOLDER_TITLE: &str = "Choose a music folder first";

const NO_FOLDER_SAYS: &str = "Songs dropped here are copied into your primary music folder. Set \
                              one in Settings under Library.";

const FOLDER_GONE_TITLE: &str = "The music folder isn't there";

const FOLDER_GONE_SAYS: &str = "If it is on a drive, mount it, then drop again.";

const NOTHING_TITLE: &str = "Nothing here the library reads";

const NOTHING_SAYS: &str = "Audio files, cue sheets and folders holding them are copied, \
                            with the covers and lyrics beside them; anything else is left where \
                            it is.";

const BUSY_TITLE: &str = "Still copying";

const BUSY_SAYS: &str = "Drop again once the songs being copied have landed.";

const READY_SAYS: &str = "Copied as they are into";

const READY_TO_FILE_SAYS: &str = "Copied and filed by your layout into";

const WEIGHING_TITLE: &str = "Looking at what is dragged…";

const WEIGHING_SAYS: &str = "Drop when it says what would be copied.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Incoming {
    paths: Vec<PathBuf>,
    weighed: Option<Vec<Dropped>>,
}

#[derive(Default)]
pub(crate) struct FolderStanding {
    seen: Option<Seen>,
    looking: bool,
}

struct Seen {
    folder: PathBuf,
    there: bool,
    at: Instant,
}

impl FolderStanding {
    fn of(&self, folder: &Path) -> Option<bool> {
        self.seen
            .as_ref()
            .filter(|seen| seen.folder == folder)
            .map(|seen| seen.there)
    }

    fn is_due_for(&self, folder: &Path) -> bool {
        !self.looking
            && self.seen.as_ref().is_none_or(|seen| {
                seen.folder != folder || seen.at.elapsed() >= FOLDER_LOOKED_AT_EVERY
            })
    }
}

#[derive(Clone, Default)]
pub(crate) struct Copying(Arc<Mutex<Option<TakeInHandle>>>);

enum Copy {
    Running,
    Finished(TakeInHandle),
    WoundDown,
}

impl Copying {
    fn hold(&self, handle: TakeInHandle) {
        *self.0.lock() = Some(handle);
    }

    fn looked_at(&self) -> Copy {
        let mut held = self.0.lock();
        match held.as_ref().map(TakeInHandle::is_finished) {
            None => Copy::WoundDown,
            Some(false) => Copy::Running,
            Some(true) => held.take().map_or(Copy::WoundDown, Copy::Finished),
        }
    }

    pub(crate) fn wind_down(&self, patience: Duration) -> bool {
        let Some(handle) = self.0.lock().take() else {
            return true;
        };
        handle.wound_down(patience).is_some()
    }
}

pub(crate) struct TakingIn {
    progress: Arc<TakeInProgress>,
    into: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Ready { taken: usize, skipped: usize },
    Weighing,
    NoFolder,
    FolderGone,
    NothingToTake,
    Busy,
}

impl Verdict {
    pub(crate) const fn drops(self) -> bool {
        matches!(self, Self::Ready { .. })
    }

    const fn is_trouble(self) -> bool {
        !matches!(self, Self::Ready { .. } | Self::Weighing)
    }
}

pub(crate) fn verdict(
    weighed: Option<&[Dropped]>,
    folder: Option<&Path>,
    folder_is_there: Option<bool>,
    busy: bool,
) -> Verdict {
    if busy {
        return Verdict::Busy;
    }
    if folder.is_none() {
        return Verdict::NoFolder;
    }
    let (Some(weighed), Some(there)) = (weighed, folder_is_there) else {
        return match folder_is_there {
            Some(false) => Verdict::FolderGone,
            _ => Verdict::Weighing,
        };
    };
    let taken = weighed
        .iter()
        .filter(|dropped| dropped.looks.is_taken())
        .count();

    if !there {
        Verdict::FolderGone
    } else if taken == 0 {
        Verdict::NothingToTake
    } else {
        Verdict::Ready {
            taken,
            skipped: weighed.len() - taken,
        }
    }
}

pub(crate) fn told_of(summary: &TakeInSummary, into: &Path) -> Notice {
    let stats = summary.stats;
    let folder = into.file_name().map_or_else(
        || into.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let files = |count: u64| format::counted(count as usize, "file", "files");
    let already = match stats.held {
        0 => String::new(),
        held => format!(" · {held} already there"),
    };

    if summary.cancelled {
        return Notice::Noted(format!("Stopped after copying {}", files(stats.copied)));
    }
    if stats.copied > 0 {
        return Notice::Done(format!(
            "Copied {} ({}) into {folder}{already}",
            files(stats.copied),
            format::bytes(stats.bytes)
        ));
    }
    if stats.held > 0 && stats.passed == 0 {
        return Notice::Noted(format!("Already in {folder}, so nothing was copied"));
    }

    let why = summary
        .passed
        .iter()
        .find(|passed| passed.why.is_a_refusal())
        .map_or(TakenPassing::NotAudio, |passed| passed.why);
    Notice::Trouble(format!("Nothing was copied — a file {}", why.as_str()))
}

fn songs_landed(summary: &TakeInSummary) -> Vec<PathBuf> {
    summary
        .landed
        .iter()
        .filter(|landed| names_audio(&landed.to))
        .map(|landed| landed.to.clone())
        .collect()
}

fn name_of(path: &Path) -> SharedString {
    SharedString::from(path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    ))
}

fn what_becomes_of(looks: Looks) -> (Icon, &'static str, bool) {
    match looks {
        Looks::Audio => (Icon::Check, "copied", true),
        Looks::Sheet => (Icon::Check, "cue sheet, copied", true),
        Looks::Companion => (Icon::Check, "beside a song, copied with it", true),
        Looks::Folder => (
            Icon::Folder,
            "folder, its audio, covers and lyrics are copied",
            true,
        ),
        Looks::Other => (Icon::Close, "not audio, left out", false),
        Looks::Gone => (Icon::Close, "not there", false),
    }
}

impl RootView {
    pub(crate) fn is_already_weighing(&self, paths: &[PathBuf]) -> bool {
        self.incoming
            .as_ref()
            .is_some_and(|incoming| incoming.paths == paths)
    }

    pub(crate) fn dragged_over(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if self.is_already_weighing(paths) {
            return;
        }

        let first = self.incoming.is_none();
        self.incoming = Some(Incoming {
            paths: paths.to_vec(),
            weighed: None,
        });
        self.weigh_the_drag(paths.to_vec(), cx);
        self.music_folder_is_there(cx);
        if first {
            self.watch_the_drag(cx);
        }
        cx.notify();
    }

    fn weigh_the_drag(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.weighing_the_drag = cx.spawn(async move |this, cx| {
            let held = paths.clone();
            let weighed = cx
                .background_executor()
                .spawn(async move { weigh(&held) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(incoming) = this
                    .incoming
                    .as_mut()
                    .filter(|incoming| incoming.paths == paths)
                {
                    incoming.weighed = Some(weighed);
                    cx.notify();
                }
            });
        });
    }

    pub(crate) fn music_folder_is_there(&mut self, cx: &mut Context<Self>) -> Option<bool> {
        let folder = cx.global::<ResonateApp>().music_folder.clone()?;
        if self.music_folder_standing.is_due_for(&folder) {
            self.look_at_the_music_folder(folder.clone(), cx);
        }
        self.music_folder_standing.of(&folder)
    }

    fn look_at_the_music_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.music_folder_standing.looking = true;
        self.looking_at_the_music_folder = cx.spawn(async move |this, cx| {
            let looked = folder.clone();
            let there = cx
                .background_executor()
                .spawn(async move { looked.is_dir() })
                .await;
            let _ = this.update(cx, |this, cx| {
                let moved = this.music_folder_standing.of(&folder) != Some(there);
                this.music_folder_standing = FolderStanding {
                    seen: Some(Seen {
                        folder,
                        there,
                        at: Instant::now(),
                    }),
                    looking: false,
                };
                if moved {
                    cx.notify();
                }
            });
        });
    }

    fn watch_the_drag(&mut self, cx: &mut Context<Self>) {
        self.watching_the_drag = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRAG_LOOKED_AT_EVERY).await;
                let left = this.update(cx, |this, cx| {
                    if cx.has_active_drag() {
                        return false;
                    }
                    this.incoming = None;
                    cx.notify();
                    true
                });
                if left.unwrap_or(true) {
                    return;
                }
            }
        });
    }

    pub(crate) fn dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let known = self
            .incoming
            .take()
            .filter(|incoming| incoming.paths == paths)
            .and_then(|incoming| incoming.weighed);
        cx.notify();

        let folder = cx.global::<ResonateApp>().music_folder.clone();
        cx.spawn(async move |this, cx| {
            let held = paths.clone();
            let looked = folder.clone();
            let (weighed, there) = cx
                .background_executor()
                .spawn(async move {
                    let weighed = known.unwrap_or_else(|| weigh(&held));
                    let there = looked.as_deref().map(Path::is_dir);
                    (weighed, there)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let busy = this.taking_in.is_some();
                let landed = verdict(Some(&weighed), folder.as_deref(), there, busy);
                this.land_the_drop(landed, paths, cx);
            });
        })
        .detach();
    }

    fn land_the_drop(&mut self, verdict: Verdict, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        match verdict {
            Verdict::NoFolder => {
                self.report(Notice::Trouble(NO_FOLDER_TITLE.to_owned()), cx);
                self.set_pane(crate::Pane::Settings, cx);
                self.show_settings(Category::Library, cx);
            }
            Verdict::FolderGone => {
                self.report(Notice::Trouble(FOLDER_GONE_TITLE.to_owned()), cx);
            }
            Verdict::NothingToTake => {
                self.report(Notice::Trouble(NOTHING_TITLE.to_owned()), cx);
            }
            Verdict::Busy => self.report(Notice::Noted(BUSY_TITLE.to_owned()), cx),
            Verdict::Weighing => {}
            Verdict::Ready { .. } => self.copy_into_the_music_folder(paths, cx),
        }
    }

    fn copy_into_the_music_folder(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let Some(into) = cx.global::<ResonateApp>().music_folder.clone() else {
            return;
        };

        let handle = match take_in(TakeInOptions {
            paths,
            into: into.clone(),
        }) {
            Ok(handle) => handle,
            Err(error) => {
                tracing::error!(%error, "dropped songs could not be copied");
                self.report(toast::could_not("copy the songs", &error), cx);
                return;
            }
        };

        self.taking_in = Some(TakingIn {
            progress: Arc::clone(handle.progress()),
            into: into.clone(),
        });
        cx.notify();
        let copying = cx.global::<ResonateApp>().copying.clone();
        copying.hold(handle);

        self._taking_in = cx.spawn(async move |this, cx| {
            let handle = loop {
                cx.background_executor().timer(COPY_LOOKED_AT_EVERY).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    return;
                }
                match copying.looked_at() {
                    Copy::Running => {}
                    Copy::Finished(handle) => break handle,
                    Copy::WoundDown => return,
                }
            };

            let finished = this.update(cx, |this, cx| {
                this.taking_in = None;
                match handle.join() {
                    Ok(summary) => this.took_in(&summary, &into, cx),
                    Err(error) => {
                        tracing::error!(%error, "the copy of dropped songs failed");
                        this.report(toast::could_not("finish copying the songs", &error), cx);
                    }
                }
                cx.notify();
            });
            let _ = finished;
        });
    }

    fn took_in(&mut self, summary: &TakeInSummary, into: &Path, cx: &mut Context<Self>) {
        self.report(told_of(summary, into), cx);
        if summary.stats.copied == 0 {
            return;
        }

        let global = cx.global::<ResonateApp>();
        if global.file_dropped {
            let layout = Layout::read(&global.organise_as).unwrap_or_default();
            let songs = songs_landed(summary);
            self.library.update(cx, |library, cx| {
                library.file_once_scanned(layout, songs, cx)
            });
        }

        let roots = self.library.read(cx).roots().to_vec();
        let reached = roots
            .into_iter()
            .find(|root| into.starts_with(root))
            .unwrap_or_else(|| into.to_path_buf());
        self.library
            .update(cx, |library, cx| library.add_roots(vec![reached], cx));
    }

    pub(crate) fn stop_copying(&mut self, cx: &mut Context<Self>) {
        if let Some(taking) = &self.taking_in {
            taking.progress.cancel();
            cx.notify();
        }
    }

    pub(crate) fn drop_overlay(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let incoming = self.incoming.clone()?;
        let there = self.music_folder_is_there(cx);
        let folder = cx.global::<ResonateApp>().music_folder.clone();
        let verdict = verdict(
            incoming.weighed.as_deref(),
            folder.as_deref(),
            there,
            self.taking_in.is_some(),
        );
        let weighed = incoming.weighed.unwrap_or_default();
        let ready_says = if cx.global::<ResonateApp>().file_dropped {
            READY_TO_FILE_SAYS
        } else {
            READY_SAYS
        };
        let ink = if verdict.is_trouble() {
            theme::failure()
        } else {
            theme::accent()
        };

        let (title, says) = match verdict {
            Verdict::Ready { taken, skipped } => (
                SharedString::from(format!(
                    "Drop to add {}",
                    format::counted(taken, "item", "items")
                )),
                match skipped {
                    0 => SharedString::new_static(ready_says),
                    _ => SharedString::from(format!(
                        "{ready_says} · {} left out",
                        format::counted(skipped, "item", "items")
                    )),
                },
            ),
            Verdict::NoFolder => (
                SharedString::new_static(NO_FOLDER_TITLE),
                SharedString::new_static(NO_FOLDER_SAYS),
            ),
            Verdict::FolderGone => (
                SharedString::new_static(FOLDER_GONE_TITLE),
                SharedString::new_static(FOLDER_GONE_SAYS),
            ),
            Verdict::NothingToTake => (
                SharedString::new_static(NOTHING_TITLE),
                SharedString::new_static(NOTHING_SAYS),
            ),
            Verdict::Busy => (
                SharedString::new_static(BUSY_TITLE),
                SharedString::new_static(BUSY_SAYS),
            ),
            Verdict::Weighing => (
                SharedString::new_static(WEIGHING_TITLE),
                SharedString::new_static(WEIGHING_SAYS),
            ),
        };

        let hidden = weighed.len().saturating_sub(NAMES_SHOWN);
        let names = weighed
            .iter()
            .take(NAMES_SHOWN)
            .fold(div().flex().flex_col().gap_1(), |list, dropped| {
                list.child(dropped_row(dropped))
            });

        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(CARD_WIDTH))
            .p_6()
            .rounded_xl()
            .bg(rgb(theme::surface()))
            .border_2()
            .border_dashed()
            .border_color(rgb(ink))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.5),
                offset: point(px(0.0), px(16.0)),
                blur_radius: px(48.0),
                spread_radius: px(0.0),
            }])
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icons::icon(
                        if verdict.is_trouble() {
                            Icon::Alert
                        } else {
                            Icon::Import
                        },
                        28.0,
                        ink,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w(px(0.0))
                            .child(kit::title(title))
                            .child(
                                div()
                                    .text_size(px(theme::text_sm()))
                                    .text_color(rgb(theme::muted()))
                                    .child(says),
                            ),
                    ),
            )
            .when(verdict.drops(), |card| {
                card.when_some(folder, |card, folder| card.child(destination(&folder, ink)))
            })
            .child(names)
            .when(hidden > 0, |card| {
                card.child(kit::figure(format!("and {hidden} more")))
            });

        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(theme::scrim()))
                .occlude()
                .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    this.dropped(paths.paths().to_vec(), cx);
                }))
                .child(motion::lifted_in(card, "drop-arrives")),
        )
    }

    pub(crate) fn copying_pill(&self, cx: &mut Context<Self>) -> Option<AnimationElement<Div>> {
        let taking = self.taking_in.as_ref()?;
        let stats = taking.progress.snapshot();
        let stopping = taking.progress.is_cancelled();
        let reached = (stats.seen_to() + 1).min(stats.found.max(1));
        let share = match stats.bytes_found {
            0 => 0.0,
            found => (stats.bytes as f32 / found as f32).clamp(0.0, 1.0),
        };
        let says = if stopping {
            SharedString::new_static("Stopping…")
        } else {
            SharedString::from(format!(
                "Copying {reached} of {} · {} of {}",
                stats.found,
                format::bytes(stats.bytes),
                format::bytes(stats.bytes_found)
            ))
        };
        let into = name_of(&taking.into);

        let resting = px(theme::transport_height() + theme::type_ahead_lift() + ABOVE_A_TOAST);

        Some(motion::risen_in(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(resting)
                .flex()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .w(px(CARD_WIDTH))
                        .px_4()
                        .py_3()
                        .rounded_xl()
                        .bg(rgb(theme::raised()))
                        .border_1()
                        .border_color(rgb(theme::outline()))
                        .shadow(vec![BoxShadow {
                            color: hsla(0.0, 0.0, 0.0, 0.45),
                            offset: point(px(0.0), px(4.0)),
                            blur_radius: px(16.0),
                            spread_radius: px(0.0),
                        }])
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icons::icon(Icon::Import, 16.0, theme::accent()))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.0))
                                        .truncate()
                                        .ends_in_an_ellipsis()
                                        .text_size(px(theme::text_sm()))
                                        .child(says),
                                )
                                .child(kit::eyebrow(into))
                                .child(self.stop_button(stopping, cx)),
                        )
                        .child(bar(share)),
                ),
            "copying-arrives",
            resting,
        ))
    }

    fn stop_button(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let button = kit::icon_button("stop-copying", Icon::Stop, "Stop copying");
        if stopping {
            return button;
        }
        button.on_click(cx.listener(|this, _, _, cx| this.stop_copying(cx)))
    }
}

fn destination(folder: &Path, ink: u32) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .child(icons::icon(Icon::Folder, theme::root_icon(), ink))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .text_size(px(theme::text_sm()))
                .child(SharedString::from(folder.display().to_string())),
        )
}

fn dropped_row(dropped: &Dropped) -> Div {
    let (icon, says, taken) = what_becomes_of(dropped.looks);
    let ink = if taken {
        theme::done()
    } else {
        theme::failure()
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .child(icons::icon(icon, 14.0, ink))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .text_size(px(theme::text_sm()))
                .child(name_of(&dropped.path)),
        )
        .child(kit::figure(says))
}

fn bar(share: f32) -> Div {
    div()
        .h(px(4.0))
        .w_full()
        .rounded_full()
        .bg(rgb(theme::border()))
        .child(
            div()
                .h_full()
                .w(relative(share))
                .rounded_full()
                .bg(rgb(theme::accent())),
        )
}

#[cfg(test)]
mod tests {
    use resonate_library::{TakeInStats, TakenPassed};

    use super::*;

    fn dropped(looks: Looks) -> Dropped {
        Dropped {
            path: PathBuf::from("/from/item"),
            looks,
        }
    }

    #[test]
    fn a_drag_is_ready_only_with_a_folder_that_is_there_something_to_take_and_nothing_running() {
        let mixed = [
            dropped(Looks::Audio),
            dropped(Looks::Other),
            dropped(Looks::Folder),
        ];
        let folder = Path::new("/music");

        assert_eq!(
            verdict(Some(&mixed), Some(folder), Some(true), false),
            Verdict::Ready {
                taken: 2,
                skipped: 1
            }
        );
        assert_eq!(
            verdict(Some(&mixed), None, Some(false), false),
            Verdict::NoFolder
        );
        assert_eq!(
            verdict(Some(&mixed), Some(folder), Some(false), false),
            Verdict::FolderGone
        );
        assert_eq!(
            verdict(Some(&mixed), Some(folder), Some(true), true),
            Verdict::Busy
        );
        assert_eq!(
            verdict(
                Some(&[dropped(Looks::Other), dropped(Looks::Gone)]),
                Some(folder),
                Some(true),
                false
            ),
            Verdict::NothingToTake
        );
    }

    #[test]
    fn a_drag_not_yet_weighed_or_a_folder_not_yet_looked_at_is_weighing_and_drops_nothing() {
        let audio = [dropped(Looks::Audio)];
        let folder = Path::new("/music");

        assert_eq!(
            verdict(None, Some(folder), Some(true), false),
            Verdict::Weighing
        );
        assert_eq!(
            verdict(Some(&audio), Some(folder), None, false),
            Verdict::Weighing
        );
        assert_eq!(
            verdict(None, Some(folder), Some(false), false),
            Verdict::FolderGone
        );
        assert_eq!(verdict(None, None, None, false), Verdict::NoFolder);
        assert!(!Verdict::Weighing.drops());
        assert!(!Verdict::Weighing.is_trouble());
    }

    #[test]
    fn only_a_ready_drag_is_dropped() {
        assert!(
            Verdict::Ready {
                taken: 1,
                skipped: 0
            }
            .drops()
        );
        for refused in [
            Verdict::NoFolder,
            Verdict::FolderGone,
            Verdict::NothingToTake,
            Verdict::Busy,
        ] {
            assert!(!refused.drops());
            assert!(refused.is_trouble());
        }
    }

    #[test]
    fn what_a_copy_did_is_told_in_the_tone_it_deserves() {
        let into = Path::new("/music/library");
        let copied = TakeInSummary {
            stats: TakeInStats {
                found: 4,
                copied: 3,
                held: 1,
                bytes: 3 * 1024 * 1024,
                ..TakeInStats::default()
            },
            ..TakeInSummary::default()
        };
        let held = TakeInSummary {
            stats: TakeInStats {
                found: 1,
                held: 1,
                ..TakeInStats::default()
            },
            ..TakeInSummary::default()
        };
        let refused = TakeInSummary {
            stats: TakeInStats {
                found: 1,
                passed: 1,
                ..TakeInStats::default()
            },
            passed: vec![TakenPassed {
                from: PathBuf::from("/from/a.flac"),
                why: TakenPassing::Unwritable,
            }],
            ..TakeInSummary::default()
        };
        let stopped = TakeInSummary {
            cancelled: true,
            ..copied.clone()
        };

        assert_eq!(
            told_of(&copied, into),
            Notice::Done("Copied 3 files (3.0 MiB) into library · 1 already there".to_owned())
        );
        assert_eq!(
            told_of(&held, into),
            Notice::Noted("Already in library, so nothing was copied".to_owned())
        );
        assert_eq!(
            told_of(&refused, into),
            Notice::Trouble(
                "Nothing was copied — a file could not be written to the folder".to_owned()
            )
        );
        assert_eq!(
            told_of(&stopped, into),
            Notice::Noted("Stopped after copying 3 files".to_owned())
        );
    }
}
