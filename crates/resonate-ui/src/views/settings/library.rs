use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    ClickEvent, Context, Div, ElementId, PathPromptOptions, SharedString, Stateful, Window, div,
    prelude::*, px, rgb,
};
use resonate_core::Switch;
use resonate_engine::{Command, PreviousRestarts, ShuffleStyle, SkipUnderRepeat, SmartShuffle};
use resonate_library::{
    EnrichStats, EnrichSummary, Failure, Failures, HistoryKept, ImportStats, ImportSummary, Layout,
    OrganiseStats, OrganiseSummary, PollStats, RetagStats, RetagSummary, ScanStats, Vault, Wanted,
    Written,
};

use crate::{
    ConfigKey, Notice, Pass, Planned, ResonateApp, Setting, format,
    icons::{self, Icon},
    theme, toast,
    views::{
        focus::{self, by_the_pointer},
        hint::Names as _,
        kit::{self, Bleeds as _, EndsInAnEllipsis as _, Found as _, Press},
        listing,
        root::RootView,
        settings::{Armed, Choice, Rows, action, note, progress, reading, switch_row},
        spoken::Spoken as _,
    },
    words,
};

const FOLDER_GROUP: &str = "folder";

const FILE_DROPPED_ID: &str = "file-dropped";

impl Choice for HistoryKept {
    const ALL: &'static [Self] = &Self::OFFERED;

    fn label(self) -> &'static str {
        match self.span().map(|span| span.as_secs() / SECONDS_A_DAY) {
            None => words::settings_library::forever(),
            Some(1_826) => words::settings_library::five_years(),
            Some(730) => words::settings_library::two_years(),
            Some(365) => words::settings_library::a_year(),
            Some(182) => words::settings_library::six_months(),
            Some(_) => words::settings_library::a_while(),
        }
    }

    fn in_force(self) -> SharedString {
        match self {
            Self::Forever => SharedString::new_static(self.label()),
            Self::Days(days) => SharedString::from(words::settings_library::days_kept(days.get())),
        }
    }

    fn meaning(self) -> SharedString {
        match self {
            Self::Forever => {
                SharedString::new_static(words::settings_library::every_listen_is_kept_so_the())
            }
            Self::Days(days) => SharedString::from(
                words::settings_library::listens_older_than_forgotten(days.get()),
            ),
        }
    }
}

const SECONDS_A_DAY: u64 = 86_400;

fn forgets_listens(now: HistoryKept, asked: HistoryKept) -> bool {
    match (now.span(), asked.span()) {
        (_, None) => false,
        (None, Some(_)) => true,
        (Some(now), Some(asked)) => asked < now,
    }
}

const WRITES_SHOWN: usize = 12;

impl RootView {
    pub(super) fn folders_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let roots = library.roots().to_vec();
        let waiting = library.roots_waiting().to_vec();
        let primary = cx.global::<ResonateApp>().music_folder.clone();
        let filed = cx.global::<ResonateApp>().file_dropped;
        let gone = self.music_folder_is_there(cx) == Some(false);

        let apart = primary.clone().filter(|primary| !roots.contains(primary));
        let nothing_listed = roots.is_empty() && apart.is_none();

        let mut listed = kit::rows().when(nothing_listed, |body| {
            body.child(note(words::settings_library::no_folders()))
        });
        for root in &roots {
            listed = listed.child(self.folder(root, primary.as_deref(), cx));
        }
        if let Some(apart) = &apart {
            listed = listed.child(self.primary_apart(apart, cx));
        }

        listed
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w(px(0.0)).child(self.text_field(
                        "typed-root",
                        &self.typed.root,
                        |this, window, cx| this.leave_typed_root(window, cx),
                        cx,
                    )))
                    .child(self.add_folder(cx)),
            )
            .when(!waiting.is_empty(), |body| {
                body.child(note(SharedString::from(waiting_line(&waiting))))
            })
            .when(primary.is_none() && !roots.is_empty(), |body| {
                body.child(note(words::settings_library::no_music_folder()))
            })
            .when(gone, |body| {
                body.child(note(words::settings_library::music_folder_gone()))
            })
            .child(self.in_the_ring(
                FILE_DROPPED_ID,
                switch_row(
                    words::settings_library::file_dropped_songs_by_the_layout(),
                    words::settings_library::off_they_keep_the_names_and(),
                    filed,
                    FILE_DROPPED_ID,
                ),
                move |this, _, cx| {
                    this.file_what_is_dropped(!filed, cx);
                    this.store(&Setting::Switched(Switch::FileDropped, !filed), cx);
                },
                cx,
            ))
            .when(!nothing_listed, |body| {
                body.note(words::settings_library::folders_note())
            })
            .note(words::settings_library::primary_note())
    }

    pub(crate) fn root_typed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.root.read(cx).text().trim().to_owned();
        if typed.is_empty() {
            self.leave_typed_root(window, cx);
            return;
        }
        let folder = spelt_out_from_home(&typed);
        let looked = folder.clone();

        self.settings_shown.looking_at_the_typed_root =
            cx.spawn_in(window, async move |this, cx| {
                let there = cx
                    .background_executor()
                    .spawn(async move { looked.is_dir() })
                    .await;

                let _ = this.update_in(cx, |this, window, cx| {
                    if !there {
                        this.report(
                            Notice::Trouble(words::settings_library::folder_is_not_a_folder(
                                resonate_core::path_text::shown(&folder),
                            )),
                            cx,
                        );
                        return;
                    }
                    this.typed
                        .root
                        .update(cx, |typed, cx| typed.hold(String::new(), cx));
                    this.library
                        .update(cx, |library, cx| library.add_roots(vec![folder], cx));
                    this.leave_typed_root(window, cx);
                    cx.notify();
                });
            });
    }

    fn folder(&self, root: &Path, primary: Option<&Path>, cx: &mut Context<Self>) -> Stateful<Div> {
        let path = root.to_path_buf();
        let chosen = primary == Some(root);
        let choosing = path.clone();

        let id: ElementId = (ElementId::Path(Arc::from(root)), "primary").into();
        let named = format!("primary-{}", resonate_core::path_text::shown(root));
        let choice = self.in_the_ring_at(
            named,
            self.folder_choice(&path, chosen, id),
            move |this, _, cx| this.choose_as_primary(choosing.clone(), cx),
            cx,
        );

        folder_card(root, chosen)
            .child(choice)
            .child(self.forget_root(path, cx))
    }

    fn primary_apart(&self, folder: &Path, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = (ElementId::Path(Arc::from(folder)), "clear-primary");
        let clear = kit::mark_when(
            Press::Takes,
            id,
            Icon::Close,
            words::settings_library::clear_primary_hint(),
            FOLDER_GROUP,
        );

        folder_card(folder, true)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.0))
                    .items_center()
                    .gap_3()
                    .child(kit::level_with_the_choice(kit::radio(true)))
                    .child(folder_name(folder))
                    .child(kit::badge(
                        words::settings_library::primary_badge(),
                        theme::accent(),
                    )),
            )
            .child(self.in_the_ring_at(
                format!("clear-primary-{}", resonate_core::path_text::shown(folder)),
                clear,
                |this, _, cx| this.forget_the_music_folder(cx),
                cx,
            ))
    }

    fn folder_choice(&self, folder: &Path, chosen: bool, id: ElementId) -> Stateful<Div> {
        div()
            .id(id.clone())
            .found_as(&id)
            .chosen_among(&folder.to_string_lossy(), String::new, chosen)
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_3()
            .rounded_md()
            .cursor_pointer()
            .names(if chosen {
                words::settings_library::primary_hint()
            } else {
                words::settings_library::make_primary_hint()
            })
            .child(kit::level_with_the_choice(kit::radio(chosen)))
            .child(folder_name(folder))
            .when(chosen, |row| {
                row.child(kit::badge(
                    words::settings_library::primary_badge(),
                    theme::accent(),
                ))
            })
    }

    fn choose_as_primary(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let held = cx.global::<ResonateApp>().music_folder.clone();
        if held.as_deref() == Some(root.as_path()) {
            self.forget_the_music_folder(cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            let looked = root.clone();
            let there = cx
                .background_executor()
                .spawn(async move { looked.is_dir() })
                .await;

            let _ = this.update(cx, |this, cx| {
                if there {
                    this.set_the_music_folder(root, cx);
                } else {
                    this.report(
                        Notice::Trouble(words::settings_library::not_a_folder().to_owned()),
                        cx,
                    );
                }
            });
        })
        .detach();
    }

    fn forget_root(&self, root: PathBuf, cx: &mut Context<Self>) -> Div {
        let id = (ElementId::Path(Arc::from(root.as_path())), "forget");
        let named = format!("forget-{}", resonate_core::path_text::shown(&root));
        let asked = self.folder_armed() == Some(&root);

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .when(asked, |part| {
                part.child(
                    div()
                        .text_size(px(theme::text_xs()))
                        .text_color(rgb(theme::failure()))
                        .child(words::settings_library::forget_asked_label()),
                )
            })
            .child(self.root_in_the_ring(
                named,
                kit::mark_when(
                    Press::Takes,
                    id,
                    Icon::Discard,
                    if asked {
                        words::settings_library::forget_asked()
                    } else {
                        words::settings_library::forget_hint()
                    },
                    FOLDER_GROUP,
                ),
                root,
                cx,
            ))
    }

    pub(crate) fn ask_to_forget_the_folder(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if self.folder_armed() == Some(&root) {
            self.settings_shown.armed = None;
            let held = cx.global::<ResonateApp>().music_folder.clone();
            if held.is_some_and(|primary| primary.starts_with(&root)) {
                self.forget_the_music_folder(cx);
            }
            self.library
                .update(cx, |library, cx| library.forget_root(root, cx));
        } else {
            self.lower_every_armed_press();
            self.arm(Armed::ForgetTheFolder(root));
        }
        cx.notify();
    }

    fn root_in_the_ring(
        &self,
        named: String,
        control: Stateful<Div>,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = root.clone();

        control
            .track_focus(&self.controls.at(named, cx))
            .key_context(crate::app::CONTROL_CONTEXT)
            .focus_visible(focus::picked)
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, _, cx| {
                    this.ask_to_forget_the_folder(root.clone(), cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {
                if by_the_pointer(click) {
                    this.ask_to_forget_the_folder(clicked.clone(), cx);
                }
            }))
    }

    fn add_folder(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "add-folder",
            words::settings_library::add_folder(),
            Icon::Plus,
            false,
            |this, _, cx| this.choose_music_folder(cx),
            self,
            cx,
        )
    }

    pub(super) fn rescanning_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let scanning = library.is_scanning();
        let stopping = library.is_stopping();
        let stats = library.stats();

        kit::rows()
            .child(
                kit::action_row()
                    .child(self.rescan(busy, scanning, cx))
                    .child(self.read_everything_again(busy, cx))
                    .when(scanning, |row| row.child(self.stop_scan(stopping, cx))),
            )
            .when_some(stats, |body, stats| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .when(scanning, |told| told.child(progress(read_so_far(stats))))
                        .child(note(counted(stats))),
                )
            })
            .note(words::settings_library::files_note())
    }

    pub(super) fn looking_up_group(&mut self, cx: &mut Context<Self>) -> Rows {
        self.details_part(cx)
    }

    pub(super) fn retagging_group(&mut self, cx: &mut Context<Self>) -> Rows {
        self.tags_part(cx)
    }

    fn details_part(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let held_back = library.is_busy() || library.is_enriching() || !library.can_enrich();
        let enriching = library.is_enriching();
        let stopping = library.is_stopping_enrich();
        let hurried = library.is_hurried();
        let unhurriable =
            hurried || stopping || !library.can_enrich() || (!enriching && library.is_busy());
        let breakdown = library.enrich_stats().map(|stats| lookup_lines(&stats));
        let ending = library
            .looked_up()
            .filter(|_| !enriching)
            .and_then(|summary| lookup_ending(&summary));

        kit::rows()
            .child(
                kit::action_row()
                    .child(self.look_up(held_back, enriching, cx))
                    .child(self.refresh_all(held_back, cx))
                    .child(self.hurry_the_lookup(unhurriable, hurried, cx))
                    .child(self.look_for_missing_covers(held_back, cx))
                    .when(enriching, |row| row.child(self.stop_lookup(stopping, cx))),
            )
            .when_some(
                breakdown.filter(|lines| !lines.is_empty()),
                |body, lines| {
                    body.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(kit::row_name(if enriching {
                                words::settings_library::lookup_under_way()
                            } else {
                                words::settings_library::last_lookup()
                            }))
                            .children(
                                lines
                                    .into_iter()
                                    .map(|(title, counted)| reading(title, counted)),
                            ),
                    )
                },
            )
            .when_some(ending, |body, ending| body.child(note(ending)))
            .note(words::settings_library::details_note())
    }

    fn rescan(&self, busy: bool, scanning: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "rescan",
            if scanning {
                words::settings_library::scanning()
            } else {
                words::settings_library::rescan_folders()
            },
            Icon::Search,
            busy,
            |this, _, cx| {
                this.library.update(cx, |library, cx| library.rescan(cx));
            },
            self,
            cx,
        )
    }

    fn read_everything_again(&self, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "read-everything-again",
            words::settings_library::read_every_file_again(),
            Icon::Redo,
            busy,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.read_everything_again(cx));
            },
            self,
            cx,
        )
    }

    fn look_for_missing_covers(&self, held_back: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "look-for-missing-covers",
            words::settings_library::look_for_missing_covers(),
            Icon::Albums,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.look_for_missing_covers(cx));
            },
            self,
            cx,
        )
    }

    fn stop_scan(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-scan",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library.update(cx, |library, cx| library.stop_scan(cx));
            },
            self,
            cx,
        )
    }

    fn look_up(&self, held_back: bool, enriching: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "look-up",
            if enriching {
                words::settings_library::looking_up()
            } else {
                words::settings_library::look_up()
            },
            Icon::Globe,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.enrich(false, cx));
            },
            self,
            cx,
        )
    }

    fn hurry_the_lookup(
        &self,
        held_back: bool,
        hurried: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "hurry-lookup",
            if hurried {
                words::settings_library::sped_up()
            } else {
                words::settings_library::speed_up_once()
            },
            Icon::Next,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.hurry_the_lookup(cx));
            },
            self,
            cx,
        )
    }

    fn refresh_all(&self, held_back: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "refresh-all",
            words::settings_library::refresh_all(),
            Icon::Redo,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.enrich(true, cx));
            },
            self,
            cx,
        )
    }

    fn stop_lookup(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-lookup",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_enrich(cx));
            },
            self,
            cx,
        )
    }

    fn choose_music_folder(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(SharedString::new_static(words::settings_library::add())),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the folder picker could not be opened");
                    let reported = this.update(cx, |_, cx| {
                        toast::tell(
                            Notice::Trouble(words::settings_library::no_folder_picker().to_owned()),
                            cx,
                        );
                    });
                    let _ = reported;
                    return;
                }
            };

            let added = this.update(cx, |this, cx| {
                this.library
                    .update(cx, |library, cx| library.add_roots(chosen, cx));
            });
            let _ = added;
        })
        .detach();
    }
}

#[cfg(unix)]
const HOME_VARIABLE: &str = "HOME";

#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";

fn spelt_out_from_home(typed: &str) -> PathBuf {
    let home = std::env::var_os(HOME_VARIABLE).filter(|home| !home.is_empty());
    match (typed.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            PathBuf::from(home).join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(typed),
    }
}

fn waiting_line(waiting: &[crate::RootWaiting]) -> String {
    let named: Vec<String> = waiting
        .iter()
        .map(|waiting| match waiting {
            crate::RootWaiting::ToAdd(root) => {
                words::settings_library::folder_to_add(resonate_core::path_text::shown(root))
            }
            crate::RootWaiting::ToForget(root) => {
                words::settings_library::folder_to_forget(resonate_core::path_text::shown(root))
            }
        })
        .collect();
    words::settings_library::waiting_for_the_pass(named.join(", "))
}

fn lookup_lines(stats: &EnrichStats) -> Vec<(&'static str, String)> {
    stats
        .told_by_section()
        .map(|(section, counted)| {
            let told: Vec<String> = counted
                .into_iter()
                .map(|(outcome, count)| format!("{count} {}", outcome.label()))
                .collect();
            (section.title(), told.join(" · "))
        })
        .collect()
}

fn lookup_ending(summary: &EnrichSummary) -> Option<String> {
    match (summary.stopped_by, summary.cancelled) {
        (Some(op), _) => Some(words::settings_library::lookup_ended_early(op.asked_for())),
        (None, true) => {
            Some(words::settings_library::the_last_lookup_was_stopped_early().to_owned())
        }
        (None, false) => None,
    }
}

fn read_so_far(stats: ScanStats) -> f32 {
    if stats.discovered == 0 {
        return 0.0;
    }

    stats.processed as f32 / stats.discovered as f32
}

fn broken_down(failed: Failures) -> String {
    let named: Vec<String> = Failure::ALL
        .into_iter()
        .filter(|failure| failed.counted(*failure) > 0)
        .map(|failure| format!("{} {}", failed.counted(failure), failure.as_str()))
        .collect();

    if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    }
}

fn counted(stats: ScanStats) -> String {
    let said = words::settings_library::scan_counted(
        stats.added,
        stats.discovered,
        stats.failed.total(),
        stats.moved,
        stats.processed,
        stats.removed,
        stats.updated,
    );

    said + &broken_down(stats.failed)
}

impl RootView {
    pub(super) fn resuming_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let keeps = self.library.read(cx).resumes();

        kit::rows()
            .child(self.in_the_ring(
                "resume-the-queue",
                switch_row(
                    words::settings_library::carry_the_queue_over(),
                    words::settings_library::off_every_run_opens_on_an(),
                    keeps,
                    "resume-the-queue",
                ),
                move |this, _, cx| this.set_resume(!keeps, cx),
                cx,
            ))
            .note(words::settings_library::resuming_note())
    }

    pub(super) fn history_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let kept = cx.global::<ResonateApp>().history_kept;
        let armed = self.history_armed();

        kit::rows()
            .child(kit::field(
                words::settings_library::keep_what_was_listened_to(),
                self.choices("history-kept", kept, cx, |this, kept: HistoryKept, cx| {
                    this.ask_to_keep_the_history(kept, cx)
                }),
            ))
            .when_some(armed, |body, armed| {
                body.child(note(
                    words::settings_library::press_again_to_forget_listens(armed.label()),
                ))
            })
    }

    fn ask_to_keep_the_history(&mut self, kept: HistoryKept, cx: &mut Context<Self>) {
        let now = cx.global::<ResonateApp>().history_kept;
        if forgets_listens(now, kept) && self.history_armed() != Some(kept) {
            self.lower_every_armed_press();
            self.arm(Armed::AgeTheHistory(kept));
            cx.notify();
            return;
        }
        self.settings_shown.armed = None;
        self.keep_the_history(kept, cx);
        cx.notify();
    }

    pub(crate) fn keep_the_history(&mut self, kept: HistoryKept, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.history_kept = kept);
        self.library
            .update(cx, |library, cx| library.age_the_history(kept, cx));
        self.store(&Setting::HistoryKept(kept), cx);
    }

    pub(super) fn repeating_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let skip = self.player.read(cx).state().skip_under_repeat;
        let repeats_the_queue = skip == SkipUnderRepeat::RepeatsTheQueue;
        let flipped = if repeats_the_queue {
            SkipUnderRepeat::KeepsRepeatingTheTrack
        } else {
            SkipUnderRepeat::RepeatsTheQueue
        };

        kit::rows().child(self.in_the_ring(
            "skip-repeats-queue",
            switch_row(
                words::settings_library::a_skip_repeats_the_queue(),
                words::settings_library::off_a_skip_goes_on_repeating(),
                repeats_the_queue,
                "skip-repeats-queue",
            ),
            move |this, _, cx| {
                this.send(Command::SetSkipUnderRepeat(flipped), cx);
                this.store(&Setting::SkipUnderRepeat(flipped), cx);
            },
            cx,
        ))
    }

    pub(super) fn shuffling_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let smart = self.player.read(cx).state().smart_shuffle;
        let spreads = smart.style == ShuffleStyle::Smart;

        let rows = kit::rows().child(kit::field(
            words::settings_library::how_shuffle_orders_the_queue(),
            self.choices("shuffle-style", smart.style, cx, move |this, style, cx| {
                this.shuffle_as(SmartShuffle { style, ..smart }, cx);
            }),
        ));
        if !spreads {
            return rows;
        }
        rows.child(self.in_the_ring(
            "shuffle-spreads-artists",
            switch_row(
                words::settings_library::spread_each_artist_apart(),
                words::settings_library::songs_by_one_artist_are_heard(),
                smart.spreads_artists,
                "shuffle-spreads-artists",
            ),
            move |this, _, cx| {
                this.shuffle_as(
                    SmartShuffle {
                        spreads_artists: !smart.spreads_artists,
                        ..smart
                    },
                    cx,
                );
            },
            cx,
        ))
        .child(self.in_the_ring(
            "shuffle-spreads-albums",
            switch_row(
                words::settings_library::spread_each_album_apart(),
                words::settings_library::songs_from_one_album_are_kept(),
                smart.spreads_albums,
                "shuffle-spreads-albums",
            ),
            move |this, _, cx| {
                this.shuffle_as(
                    SmartShuffle {
                        spreads_albums: !smart.spreads_albums,
                        ..smart
                    },
                    cx,
                );
            },
            cx,
        ))
        .child(self.in_the_ring(
            "shuffle-fresh-first",
            switch_row(
                words::settings_library::songs_heard_this_week_come_last(),
                words::settings_library::what_you_have_not_heard_lately(),
                smart.fresh_first,
                "shuffle-fresh-first",
            ),
            move |this, _, cx| {
                this.shuffle_as(
                    SmartShuffle {
                        fresh_first: !smart.fresh_first,
                        ..smart
                    },
                    cx,
                );
            },
            cx,
        ))
    }

    pub(crate) fn shuffle_as(&mut self, smart: SmartShuffle, cx: &mut Context<Self>) {
        let was = self.player.read(cx).state().smart_shuffle;
        if was == smart {
            return;
        }
        self.send(Command::SetSmartShuffle(smart), cx);

        if was.style != smart.style {
            self.store(&Setting::ShuffleStyle(smart.style), cx);
        }
        if was.spreads_artists != smart.spreads_artists {
            self.store(&Setting::ShuffleSpreadsArtists(smart.spreads_artists), cx);
        }
        if was.spreads_albums != smart.spreads_albums {
            self.store(&Setting::ShuffleSpreadsAlbums(smart.spreads_albums), cx);
        }
        if was.fresh_first != smart.fresh_first {
            self.store(&Setting::ShuffleFreshFirst(smart.fresh_first), cx);
        }
        cx.notify();
    }

    pub(super) fn previous_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let policy = self.player.read(cx).state().previous_restarts;
        let restarts = policy == PreviousRestarts::RestartsTheTrack;
        let flipped = if restarts {
            PreviousRestarts::AlwaysGoesBack
        } else {
            PreviousRestarts::RestartsTheTrack
        };

        kit::rows().child(self.in_the_ring(
            "previous-restarts",
            switch_row(
                words::settings_library::previous_restarts_the_song(),
                words::settings_library::off_previous_always_goes_back_a(),
                restarts,
                "previous-restarts",
            ),
            move |this, _, cx| {
                this.send(Command::SetPreviousRestarts(flipped), cx);
                this.store(&Setting::PreviousRestarts(flipped), cx);
            },
            cx,
        ))
    }

    pub(crate) fn set_resume(&mut self, resume: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.resume = resume);
        self.library
            .update(cx, |library, cx| library.set_resume(resume, cx));
        self.store(&Setting::Switched(Switch::Resume, resume), cx);
    }
}

impl RootView {
    pub(super) fn organising_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let typed = self.typed.organising.read(cx).text().trim().to_owned();
        let refused = Layout::read(&typed).err().map(|error| error.to_string());
        let unreadable = refused.is_some();
        self.typed
            .organising
            .update(cx, |organising, cx| organising.refusing(unreadable, cx));
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let organising = library.is_organising();
        let moving = library.is_moving();
        let planned = library.previewed();
        let previewed = planned.is_shown();
        let armed = self.is_armed(&Armed::MoveTheFiles) && previewed;
        let walks_back = library.walks_back();
        let walking_back = self.is_armed(&Armed::WalkTheFilingBack) && walks_back;
        let stopping = library.is_stopping_organise();
        let stats = library.organise_stats();
        let told = library
            .organised()
            .filter(|(pass, _)| !pass.applies())
            .map(|(_, summary)| filed(summary));

        kit::rows()
            .child(
                kit::field(
                    words::settings_library::template_label(),
                    self.text_field(
                        "organise-as",
                        &self.typed.organising,
                        |this, window, cx| this.leave_organising(window, cx),
                        cx,
                    ),
                )
                .when_some(refused, |field, refused| {
                    field.child(listing::noticed(&Notice::Trouble(refused)))
                }),
            )
            .child(
                kit::action_row()
                    .child(self.preview_the_filing(busy || unreadable, organising && !moving, cx))
                    .child(self.file_the_tracks(
                        busy || unreadable || !previewed,
                        moving,
                        armed,
                        cx,
                    ))
                    .when(organising, |row| row.child(self.stop_filing(stopping, cx)))
                    .when(walks_back && !organising, |row| {
                        row.child(self.put_the_filing_back(busy, walking_back, cx))
                    }),
            )
            .when(!previewed && !organising, |body| {
                body.child(note(unplanned(
                    planned,
                    words::settings_library::preview_first(),
                )))
            })
            .when(armed, |body| {
                body.child(note(words::settings_library::ask_again_to_move()))
            })
            .when(walking_back, |body| {
                body.child(note(words::settings_library::ask_again_to_put_back()))
            })
            .when(organising, |body| {
                body.child(note(going(stats.unwrap_or_default(), moving)))
            })
            .when_some(told, |body, told| body.child(note(told)))
            .note(words::settings_library::organising_note())
    }

    fn preview_the_filing(
        &self,
        held_back: bool,
        previewing: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "preview-the-filing",
            if previewing {
                words::settings_library::previewing()
            } else {
                words::settings_library::preview()
            },
            Icon::Search,
            held_back,
            |this, _, cx| this.file_them(Pass::Preview, cx),
            self,
            cx,
        )
    }

    fn file_the_tracks(
        &self,
        held_back: bool,
        moving: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "file-the-tracks",
            if moving {
                words::settings_library::moving()
            } else if armed {
                words::settings_library::press_again_to_move_the_files()
            } else {
                words::settings_library::apply()
            },
            Icon::Tidy,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.file_them(Pass::Apply, cx);
                } else {
                    this.arm_the_move(cx);
                }
            },
            self,
            cx,
        )
    }

    fn stop_filing(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-filing",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_organise(cx));
            },
            self,
            cx,
        )
    }

    fn put_the_filing_back(
        &self,
        held_back: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "put-the-filing-back",
            if armed {
                words::settings_library::press_again_to_put_them_back()
            } else {
                words::settings_library::put_the_last_run_back()
            },
            Icon::Undo,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.lower(&Armed::WalkTheFilingBack);
                    this.library
                        .update(cx, |library, cx| library.walk_the_filing_back(cx));
                } else {
                    this.arm(Armed::WalkTheFilingBack);
                }
                cx.notify();
            },
            self,
            cx,
        )
    }

    fn arm_the_move(&mut self, cx: &mut Context<Self>) {
        self.arm(Armed::MoveTheFiles);
        cx.notify();
    }

    fn file_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.lower(&Armed::MoveTheFiles);
        let typed = self.typed.organising.read(cx).text().trim().to_owned();

        match Layout::read(&typed) {
            Ok(layout) => self
                .library
                .update(cx, |library, cx| library.organise(layout, pass, cx)),
            Err(error) => self.report(
                toast::could_not(words::settings_library::file_the_tracks_that_way(), &error),
                cx,
            ),
        }
        cx.notify();
    }

    pub(crate) fn layout_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.typed.organising.read(cx).text().trim().to_owned();

        match Layout::read(&given) {
            Ok(_) => {
                self.set_organise_as(given, cx);
                self.leave_organising(window, cx);
            }
            Err(error) => self.report(
                toast::could_not(words::settings_library::use_that_layout(), &error),
                cx,
            ),
        }
        cx.notify();
    }

    pub(crate) fn layout_left(&mut self, cx: &mut Context<Self>) {
        let given = self.typed.organising.read(cx).text().trim().to_owned();
        if given == cx.global::<ResonateApp>().organise_as {
            return;
        }

        match Layout::read(&given) {
            Ok(_) => self.set_organise_as(given, cx),
            Err(error) => self.report(
                toast::could_not(words::settings_library::use_that_layout(), &error),
                cx,
            ),
        }
        cx.notify();
    }

    pub(crate) fn set_organise_as(&mut self, template: String, cx: &mut Context<Self>) {
        self.typed
            .organising
            .update(cx, |organising, cx| organising.hold(template.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.organise_as = template.clone());
        self.library
            .update(cx, |library, cx| library.forget_the_preview(cx));
        self.store(&Setting::OrganiseAs(template), cx);
    }
}

impl RootView {
    fn tags_part(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let tagging = library.is_tagging();
        let writing = library.is_writing_tags();
        let planned = library.previewed_tags();
        let previewed = planned.is_shown();
        let armed = self.is_armed(&Armed::WriteTheTags) && previewed;
        let walks_back = library.tags_walk_back();
        let walking_back = self.is_armed(&Armed::WalkTheTagsBack) && walks_back;
        let stopping = library.is_stopping_retag();
        let stats = library.retag_stats();
        let told = library
            .tagged()
            .filter(|(pass, _)| !pass.applies())
            .map(|(_, summary)| (written(summary), listed(summary)));

        kit::rows()
            .child(
                kit::action_row()
                    .child(self.preview_the_tags(busy, tagging && !writing, cx))
                    .child(self.write_the_tags(busy || !previewed, writing, armed, cx))
                    .when(tagging, |row| row.child(self.stop_tagging(stopping, cx)))
                    .when(walks_back && !tagging, |row| {
                        row.child(self.put_the_tags_back(busy, walking_back, cx))
                    }),
            )
            .when(!previewed && !tagging, |body| {
                body.child(note(unplanned(
                    planned,
                    words::settings_library::preview_tags_first(),
                )))
            })
            .when(armed, |body| {
                body.child(note(words::settings_library::ask_again_to_write()))
            })
            .when(walking_back, |body| {
                body.child(note(
                    words::settings_library::ask_again_to_put_the_tags_back(),
                ))
            })
            .when(tagging, |body| {
                body.child(note(so_far(stats.unwrap_or_default(), writing)))
            })
            .when_some(told, |body, (told, listed)| {
                body.when_some(listed, |body, listed| body.children(listed))
                    .child(note(told))
            })
            .note(words::settings_library::tagging_note())
    }

    fn put_the_tags_back(
        &self,
        held_back: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "put-the-tags-back",
            if armed {
                words::settings_library::press_again_to_put_them_back()
            } else {
                words::settings_library::put_the_last_run_back()
            },
            Icon::Undo,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.lower(&Armed::WalkTheTagsBack);
                    this.library
                        .update(cx, |library, cx| library.walk_the_tags_back(cx));
                } else {
                    this.arm(Armed::WalkTheTagsBack);
                }
                cx.notify();
            },
            self,
            cx,
        )
    }

    fn preview_the_tags(
        &self,
        held_back: bool,
        previewing: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "preview-the-tags",
            if previewing {
                words::settings_library::previewing()
            } else {
                words::settings_library::preview()
            },
            Icon::Search,
            held_back,
            |this, _, cx| this.tag_them(Pass::Preview, cx),
            self,
            cx,
        )
    }

    fn write_the_tags(
        &self,
        held_back: bool,
        writing: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "write-the-tags",
            if writing {
                words::settings_library::writing()
            } else if armed {
                words::settings_library::press_again_to_write_the_files()
            } else {
                words::settings_library::apply()
            },
            Icon::Tidy,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.tag_them(Pass::Apply, cx);
                } else {
                    this.arm_the_write(cx);
                }
            },
            self,
            cx,
        )
    }

    fn stop_tagging(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-tagging",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_retag(cx));
            },
            self,
            cx,
        )
    }

    fn arm_the_write(&mut self, cx: &mut Context<Self>) {
        self.arm(Armed::WriteTheTags);
        cx.notify();
    }

    fn tag_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.lower(&Armed::WriteTheTags);
        self.library
            .update(cx, |library, cx| library.retag(pass, cx));
        cx.notify();
    }
}

fn listed(summary: &RetagSummary) -> Option<Vec<gpui::AnyElement>> {
    let writes = &summary.retagging.writes;
    if writes.is_empty() {
        return None;
    }

    let mut listing: Vec<gpui::AnyElement> = writes
        .iter()
        .take(WRITES_SHOWN)
        .map(|write| write_row(write).into_any_element())
        .collect();
    if let Some(rest) = writes
        .len()
        .checked_sub(WRITES_SHOWN)
        .filter(|rest| *rest > 0)
    {
        listing.push(note(words::settings_library::and_more(rest)).into_any_element());
    }

    Some(listing)
}

fn write_row(write: &Written) -> Div {
    let shown = write
        .path
        .file_name()
        .unwrap_or(write.path.as_os_str())
        .to_string_lossy()
        .into_owned();

    kit::named(shown, fields_of(write))
}

fn fields_of(write: &Written) -> String {
    let mut named: Vec<&str> = write.edits.iter().map(|edit| edit.field.as_str()).collect();
    if write.picture.is_some() {
        named.push(words::settings_library::cover_art());
    }

    named.join(", ")
}

fn so_far(stats: RetagStats, writing: bool) -> String {
    if writing {
        words::settings_library::retag_written_so_far(
            stats.passed_over,
            stats.unchanged,
            stats.walked,
            stats.written,
        )
    } else {
        words::settings_library::retag_so_far(stats.passed_over, stats.unchanged, stats.walked)
    }
}

fn written(summary: &RetagSummary) -> String {
    let stats = summary.stats;
    let retagging = &summary.retagging;
    if retagging.writes.is_empty() && retagging.passed_over.is_empty() && stats.unchanged == 0 {
        return words::settings_library::nothing_to_tag().to_owned();
    }

    let fields: u64 = retagging
        .writes
        .iter()
        .map(|write| write.edits.len() as u64)
        .sum();
    let pictures = retagging
        .writes
        .iter()
        .filter(|write| write.picture.is_some())
        .count();
    let ratings = retagging
        .writes
        .iter()
        .filter(|write| write.popularity.is_some())
        .count();
    let told = words::settings_library::retag_previewed(
        fields,
        stats.passed_over,
        pictures,
        ratings,
        stats.unchanged,
        retagging.writes.len(),
    );

    stopped_early(told, summary.cancelled)
}

fn going(stats: OrganiseStats, moving: bool) -> String {
    if moving {
        words::settings_library::organise_moved_so_far(
            stats.collided,
            stats.failed,
            stats.moved,
            stats.unchanged,
            stats.unidentified,
        )
    } else {
        words::settings_library::organise_so_far(
            stats.collided,
            stats.failed,
            stats.unchanged,
            stats.unidentified,
        )
    }
}

fn filed(summary: &OrganiseSummary) -> String {
    let stats = summary.stats;
    let told = words::settings_library::organise_previewed(
        stats.collided,
        stats.failed,
        summary.plan.folders.len(),
        summary.plan.files_moving(),
        stats.unchanged,
        stats.unidentified,
    );

    stopped_early(told, summary.cancelled)
}

fn stopped_early(told: String, cancelled: bool) -> String {
    if cancelled {
        words::settings_library::stopped_before_it_finished(told)
    } else {
        told
    }
}

impl RootView {
    pub(super) fn vault_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let Some(root) = library.vault_root() else {
            return self.naming_a_vault(cx);
        };

        let busy = library.is_busy();
        let importing = library.is_importing();
        let keeping = library.is_keeping();
        let planned = library.previewed_import();
        let previewed = planned.is_shown();
        let armed = self.is_armed(&Armed::KeepTheTracks) && previewed;
        let stopping = library.is_stopping_import();
        let stats = library.import_stats();
        let told = library
            .imported()
            .filter(|(pass, _)| !pass.applies())
            .map(|(_, summary)| vaulted(summary));
        let listed = library
            .imported()
            .filter(|(pass, _)| !pass.applies())
            .map(|(_, summary)| wanted_rows(summary));
        let tending = self.tending_rows(busy, cx);

        kit::rows()
            .child(folder_row(&root))
            .child(
                kit::action_row()
                    .child(self.preview_the_import(busy, importing && !keeping, cx))
                    .child(self.keep_the_tracks(busy || !previewed, keeping, armed, cx))
                    .when(importing, |row| row.child(self.stop_keeping(stopping, cx))),
            )
            .when(!previewed && !importing, |body| {
                body.child(note(unplanned(
                    planned,
                    words::settings_library::preview_import_first(),
                )))
            })
            .when(armed, |body| {
                body.child(note(words::settings_library::ask_again_to_keep()))
            })
            .when(importing && keeping, |body| {
                body.child(progress(kept_so_far(stats.unwrap_or_default())))
            })
            .when_some(listed, |body, listed| body.children(listed))
            .when_some(told, |body, told| body.child(note(told)))
            .children(tending)
            .note(words::settings_library::vault_note())
    }

    fn naming_a_vault(&self, cx: &mut Context<Self>) -> Rows {
        let named = self.settings_shown.vault_named.clone();

        kit::rows()
            .child(match &named {
                Some(folder) => div().child(folder_row(folder)),
                None => div().child(note(words::settings_library::no_vault())),
            })
            .child(kit::action_row().child(action(
                "choose-vault-folder",
                words::settings_library::choose_folder(),
                Icon::Folder,
                false,
                |this, _, cx| this.pick_a_folder(cx, Self::name_the_vault),
                self,
                cx,
            )))
            .when(named.is_some(), |body| {
                body.child(note(words::settings_library::vault_at_the_next_start()))
            })
    }

    pub(crate) fn name_the_vault(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let music = cx.global::<ResonateApp>().music_folder.clone();
        let refusal =
            unusable_as_the_vault(&folder, self.library.read(cx).roots(), music.as_deref());
        if let Some(refusal) = refusal {
            self.report(Notice::Trouble(refusal.to_owned()), cx);
            return;
        }

        let making = folder.clone();
        let made = cx
            .background_executor()
            .spawn(async move { Vault::make(&making).map(drop) });
        self.settings_shown.making_the_vault = cx.spawn(async move |this, cx| {
            let made = made.await;
            let _ = this.update(cx, |this, cx| match made {
                Ok(()) => {
                    this.store(&Setting::Vault(folder.clone()), cx);
                    this.settings_shown.vault_named = Some(folder);
                    cx.notify();
                }
                Err(error) => {
                    tracing::warn!(%error, "the folder chosen for the vault could not be made one");
                    this.report(
                        Notice::Trouble(words::settings_library::vault_not_made().to_owned()),
                        cx,
                    );
                }
            });
        });
    }

    fn preview_the_import(
        &self,
        held_back: bool,
        previewing: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "preview-the-import",
            if previewing {
                words::settings_library::previewing()
            } else {
                words::settings_library::preview()
            },
            Icon::Search,
            held_back,
            |this, _, cx| this.keep_them(Pass::Preview, cx),
            self,
            cx,
        )
    }

    fn keep_the_tracks(
        &self,
        held_back: bool,
        keeping: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "keep-the-tracks",
            if keeping {
                words::settings_library::keeping()
            } else if armed {
                words::settings_library::press_again_to_keep_the_tracks()
            } else {
                words::settings_library::apply()
            },
            Icon::Tidy,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.keep_them(Pass::Apply, cx);
                } else {
                    this.arm_the_keep(cx);
                }
            },
            self,
            cx,
        )
    }

    fn stop_keeping(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-keeping",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Close,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_import(cx));
            },
            self,
            cx,
        )
    }

    fn arm_the_keep(&mut self, cx: &mut Context<Self>) {
        self.arm(Armed::KeepTheTracks);
        cx.notify();
    }

    fn keep_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.lower(&Armed::KeepTheTracks);
        self.library
            .update(cx, |library, cx| library.import(pass, cx));
        cx.notify();
    }
}

fn kept_so_far(stats: ImportStats) -> f32 {
    if stats.walked == 0 {
        return 0.0;
    }
    (stats.vaulted + stats.passed) as f32 / stats.walked as f32
}

fn wanted_rows(summary: &ImportSummary) -> Vec<gpui::AnyElement> {
    let wanted = &summary.plan.wanted;
    if wanted.is_empty() {
        return vec![note(words::settings_library::nothing_to_keep()).into_any_element()];
    }

    let mut listed: Vec<gpui::AnyElement> = wanted
        .iter()
        .take(WRITES_SHOWN)
        .map(|asked| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::text()))
                        .ends_in_an_ellipsis()
                        .child(named_file(&asked.from)),
                )
                .child(kit::figure(format::bytes(asked.bytes)))
                .child(kit::badge(kept_as(asked), theme::muted()))
                .into_any_element()
        })
        .collect();

    if wanted.len() > WRITES_SHOWN {
        listed.push(
            note(words::settings_library::and_more(
                wanted.len() - WRITES_SHOWN,
            ))
            .into_any_element(),
        );
    }
    listed
}

fn kept_as(asked: &Wanted) -> String {
    if asked.renewing {
        words::settings_library::kept_again(asked.form.as_str())
    } else {
        asked.form.as_str().to_owned()
    }
}

fn named_file(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn vaulted(summary: &ImportSummary) -> String {
    stopped_early(
        words::settings_library::vault_previewed(summary.stats.walked),
        summary.cancelled,
    )
}

impl RootView {
    pub(crate) fn file_what_is_dropped(&self, filed: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.file_dropped = filed);
        cx.notify();
    }

    pub(crate) fn choose_the_primary_folder(&self, cx: &mut Context<Self>) {
        self.pick_a_folder(cx, Self::set_the_music_folder);
    }

    fn pick_a_folder(
        &self,
        cx: &mut Context<Self>,
        landed: impl FnOnce(&mut Self, PathBuf, &mut Context<Self>) + 'static,
    ) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::new_static(words::settings_library::choose())),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the folder picker could not be opened");
                    let reported = this.update(cx, |_, cx| {
                        toast::tell(
                            Notice::Trouble(words::settings_library::no_folder_picker().to_owned()),
                            cx,
                        );
                    });
                    let _ = reported;
                    return;
                }
            };
            let Some(folder) = chosen.into_iter().next() else {
                return;
            };

            let settled = cx
                .background_executor()
                .spawn(async move { folder.canonicalize().unwrap_or(folder) })
                .await;

            let named = this.update(cx, |this, cx| landed(this, settled, cx));
            let _ = named;
        })
        .detach();
    }

    pub(crate) fn set_the_music_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let vault = self.library.read(cx).vault_root();
        if let Some(refusal) = unusable_as_the_music_folder(&folder, vault.as_deref()) {
            self.report(Notice::Trouble(refusal.to_owned()), cx);
            return;
        }

        cx.update_global::<ResonateApp, _>(|global, _| {
            global.music_folder = Some(folder.clone());
        });
        self.store(&Setting::MusicFolder(folder.clone()), cx);

        let reached = self
            .library
            .read(cx)
            .roots()
            .iter()
            .any(|root| folder.starts_with(root));
        if !reached {
            self.library
                .update(cx, |library, cx| library.add_roots(vec![folder], cx));
        }
        cx.notify();
    }

    pub(crate) fn forget_the_music_folder(&mut self, cx: &mut Context<Self>) {
        self.clear_the_music_folder(cx);
        self.forget(ConfigKey::MusicFolder, cx);
    }

    pub(crate) fn clear_the_music_folder(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.music_folder = None);
        cx.notify();
    }

    pub(super) fn inbox_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let inbox = library.inbox().map(Path::to_path_buf);
        let busy = library.is_busy();
        let polling = library.is_polling();
        let stopping = library.is_stopping_poll();
        let held_back = busy || !library.can_poll(cx);
        let told = library.poll_stats().map(asked_of_the_inbox);

        kit::rows()
            .child(match &inbox {
                Some(folder) => div().child(folder_row(folder)),
                None => div().child(note(words::settings_library::no_inbox())),
            })
            .child(
                kit::action_row()
                    .child(self.choose_inbox(busy, cx))
                    .child(self.poll_now(held_back, polling, cx))
                    .when(polling, |row| row.child(self.stop_polling(stopping, cx))),
            )
            .when_some(told, |body, told| body.child(note(told)))
            .when(inbox.is_some(), |body| {
                body.note(words::settings_library::inbox_note())
            })
    }

    fn choose_inbox(&self, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "choose-inbox",
            words::settings_library::choose_folder(),
            Icon::Folder,
            busy,
            |this, _, cx| this.choose_inbox_folder(cx),
            self,
            cx,
        )
    }

    fn poll_now(&self, held_back: bool, polling: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "poll-now",
            if polling {
                words::settings_library::polling()
            } else {
                words::settings_library::poll_now()
            },
            Icon::Search,
            held_back,
            |this, _, cx| {
                this.library.update(cx, |library, cx| library.poll(cx));
            },
            self,
            cx,
        )
    }

    fn stop_polling(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-polling",
            if stopping {
                words::settings_library::stopping()
            } else {
                words::settings_library::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library.update(cx, |library, cx| library.stop_poll(cx));
            },
            self,
            cx,
        )
    }

    fn choose_inbox_folder(&self, cx: &mut Context<Self>) {
        self.pick_a_folder(cx, Self::set_inbox);
    }

    fn set_inbox(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.store(&Setting::Inbox(folder.clone()), cx);
        self.library
            .update(cx, |library, cx| library.set_inbox(Some(folder), cx));
    }
}

fn folder_card(folder: &Path, chosen: bool) -> Stateful<Div> {
    div()
        .id(SharedString::from(folder.display().to_string()))
        .group(FOLDER_GROUP)
        .flex()
        .items_center()
        .gap_3()
        .bleeds()
        .when(chosen, |card| card.bg(kit::chosen_wash()))
}

fn folder_name(folder: &Path) -> Div {
    div()
        .flex()
        .flex_1()
        .min_w(px(0.0))
        .items_center()
        .gap_2()
        .child(icons::icon(
            Icon::Folder,
            theme::root_icon(),
            theme::muted(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .text_size(px(theme::text_sm()))
                .child(SharedString::from(resonate_core::path_text::shown(folder))),
        )
}

fn folder_row(folder: &Path) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .child(icons::icon(
            Icon::Folder,
            theme::root_icon(),
            theme::muted(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .ends_in_an_ellipsis()
                .text_size(px(theme::text_sm()))
                .child(SharedString::from(resonate_core::path_text::shown(folder))),
        )
}

fn unusable_as_the_music_folder(folder: &Path, vault: Option<&Path>) -> Option<&'static str> {
    if !folder.is_dir() {
        return Some(words::settings_library::not_a_folder());
    }
    vault
        .filter(|vault| folder.starts_with(vault))
        .map(|_| words::settings_library::in_the_vault())
}

fn unusable_as_the_vault(
    folder: &Path,
    roots: &[PathBuf],
    music: Option<&Path>,
) -> Option<&'static str> {
    if !folder.is_dir() {
        return Some(words::settings_library::not_a_folder());
    }
    roots
        .iter()
        .map(PathBuf::as_path)
        .chain(music)
        .any(|held| folder.starts_with(held))
        .then_some(words::settings_library::in_the_library())
}

fn asked_of_the_inbox(stats: PollStats) -> String {
    words::settings_library::inbox_asked(
        stats.asked,
        stats.kept,
        stats.late,
        stats.nothing,
        stats.refused,
        stats.unkept,
    )
}

fn unplanned(planned: Planned, first: &'static str) -> &'static str {
    match planned {
        Planned::Outdated => words::settings_library::moved_under_the_preview(),
        Planned::Not | Planned::Shown(_) => first,
    }
}

impl Choice for ShuffleStyle {
    const ALL: &'static [Self] = &ShuffleStyle::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Random => words::settings_library::random(),
            Self::Smart => words::settings_library::smart(),
            Self::Albums => words::settings_library::by_album(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Random => words::settings_library::every_song_in_the_queue_in(),
            Self::Smart => words::settings_library::every_song_in_the_queue_laid(),
            Self::Albums => words::settings_library::whole_albums_one_after_another_each(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::settings::said_under;

    #[test]
    fn a_music_folder_is_a_folder_outside_the_vault() {
        let held =
            std::env::temp_dir().join(format!("resonate-music-folder-{}", std::process::id()));
        let vault = held.join("vault");
        let inside = vault.join("audio");
        let outside = held.join("music");
        std::fs::create_dir_all(&inside).expect("a writable temporary directory");
        std::fs::create_dir_all(&outside).expect("a writable temporary directory");

        let unnamed = unusable_as_the_music_folder(&outside, None);
        let beside = unusable_as_the_music_folder(&outside, Some(&vault));
        let within = unusable_as_the_music_folder(&inside, Some(&vault));
        let itself = unusable_as_the_music_folder(&vault, Some(&vault));
        let missing = unusable_as_the_music_folder(&held.join("nowhere"), None);
        let _ = std::fs::remove_dir_all(&held);

        assert_eq!(unnamed, None);
        assert_eq!(beside, None);
        assert_eq!(within, Some(words::settings_library::in_the_vault()));
        assert_eq!(itself, Some(words::settings_library::in_the_vault()));
        assert_eq!(missing, Some(words::settings_library::not_a_folder()));
    }

    #[test]
    fn a_vault_is_a_folder_outside_every_library_folder() {
        let held =
            std::env::temp_dir().join(format!("resonate-vault-named-{}", std::process::id()));
        let root = held.join("music");
        let music = held.join("dropped");
        let inside = root.join("archive");
        let dropped_into = music.join("archive");
        let beside = held.join("vault");
        for folder in [&inside, &dropped_into, &beside] {
            std::fs::create_dir_all(folder).expect("a writable temporary directory");
        }
        let roots = [root.clone()];

        let outside = unusable_as_the_vault(&beside, &roots, Some(&music));
        let within_a_root = unusable_as_the_vault(&inside, &roots, Some(&music));
        let the_root = unusable_as_the_vault(&root, &roots, None);
        let within_the_music = unusable_as_the_vault(&dropped_into, &roots, Some(&music));
        let missing = unusable_as_the_vault(&held.join("nowhere"), &[], None);
        let _ = std::fs::remove_dir_all(&held);

        assert_eq!(outside, None);
        assert_eq!(
            within_a_root,
            Some(words::settings_library::in_the_library())
        );
        assert_eq!(the_root, Some(words::settings_library::in_the_library()));
        assert_eq!(
            within_the_music,
            Some(words::settings_library::in_the_library())
        );
        assert_eq!(missing, Some(words::settings_library::not_a_folder()));
    }

    #[test]
    fn only_a_shorter_span_forgets_listens() {
        let forever = HistoryKept::Forever;
        let year = HistoryKept::for_days(365).expect("a span of days");
        let months = HistoryKept::for_days(182).expect("a span of days");

        assert!(forgets_listens(forever, year));
        assert!(forgets_listens(year, months));
        assert!(!forgets_listens(months, year));
        assert!(!forgets_listens(year, forever));
        assert!(!forgets_listens(year, year));
        assert!(!forgets_listens(forever, forever));
    }

    #[test]
    fn a_span_of_days_off_the_table_says_how_many_are_kept() {
        let quarter = HistoryKept::for_days(90).expect("a span of days");

        assert_eq!(
            said_under(quarter),
            "90 days is in force, which is none of these."
        );
        assert_eq!(
            said_under(HistoryKept::Forever),
            HistoryKept::Forever.meaning()
        );
    }

    #[test]
    fn a_lookup_is_told_a_section_at_a_time_with_the_outcomes_it_counted() {
        let stats = EnrichStats {
            albums: 12,
            releases: 8,
            albums_unmatched: 3,
            covers: 5,
            ..EnrichStats::default()
        };

        assert_eq!(
            lookup_lines(&stats),
            vec![
                (
                    "Albums",
                    "12 asked about · 8 matched to a release · 3 with no strict match".to_owned()
                ),
                ("Pictures", "5 covers fetched".to_owned()),
            ]
        );
        assert!(lookup_lines(&EnrichStats::default()).is_empty());
    }

    #[test]
    fn a_lookup_that_ended_early_says_why_and_one_that_ran_out_says_nothing() {
        let whole = EnrichSummary::default();
        let stopped = EnrichSummary {
            cancelled: true,
            ..EnrichSummary::default()
        };
        let unreached = EnrichSummary {
            stopped_by: Some(resonate_library::LookupOp::Release),
            ..EnrichSummary::default()
        };

        assert_eq!(lookup_ending(&whole), None);
        assert_eq!(
            lookup_ending(&stopped).as_deref(),
            Some("The last lookup was stopped early")
        );
        assert!(
            lookup_ending(&unreached)
                .is_some_and(|said| said.contains("couldn't be reached while looking up"))
        );
    }
}
