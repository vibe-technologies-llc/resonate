use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui::{
    Context, Div, ElementId, PathPromptOptions, SharedString, Stateful, Window, div, prelude::*,
    px, rgb,
};
use resonate_engine::{Command, SkipUnderRepeat};
use resonate_library::{
    Failure, Failures, ImportStats, ImportSummary, Layout, OrganiseStats, OrganiseSummary,
    PollStats, RetagStats, RetagSummary, ScanStats, Wanted, Written,
};

use crate::{
    Notice, Pass, ResonateApp, Setting, format,
    icons::{self, Icon},
    theme,
    views::{
        kit::{self, EndsInAnEllipsis as _, Press},
        listing,
        root::RootView,
        settings::{action, hugging, named, note, progress, rows, switch_row},
    },
};

const FOLDER_GROUP: &str = "folder";

const FORGET_HINT: &str = "Forget this folder and every track scanned from it. The files are left \
                           alone.";

const NO_FOLDERS: &str = "No folders yet. Add one and Resonate will scan it.";

const FOLDERS_NOTE: &str = "Everything under these folders is read into the library and followed \
                            while the window is open. The files are only read: nothing in them \
                            changes unless Tagging or Organising below is applied.";

const SCANNING_NOTE: &str = "Rescan reads what was added or changed since the last scan. Enrich \
                             asks MusicBrainz about the albums and artists not asked lately, \
                             which a scan does on its own when Online allows it.";

const RESUMING_NOTE: &str = "The queue comes back paused on the row it was playing, so nothing \
                             starts on its own. Files named on the command line are queued \
                             instead of what was kept.";

const REFRESHING_NOTE: &str = "The scan's progress shows under Scanning above. What MusicBrainz \
                               answered is asked again with Refresh all under Online.";

const TEMPLATE_LABEL: &str = "Where each track is filed";

const ORGANISING_NOTE: &str = "Each / is a folder under the music folder the track was scanned \
                               from. Preview lists what would move; only Apply moves anything, \
                               and the catalog follows the files.";

const PREVIEW_FIRST: &str = "Preview first, so what would move is on screen before anything does.";

const ASK_AGAIN_TO_MOVE: &str = "Press it again to move the files. Nothing is copied — every \
                                 track is renamed where it stands.";

const TAGGING_NOTE: &str = "Writes what a lookup answered for back into the files themselves, so \
                            another player reads the same names. Only fields the catalog was told \
                            about are touched, and a cover goes in only where the file carries \
                            none.";

const PREVIEW_TAGS_FIRST: &str = "Preview first, so what would be written is on screen before \
                                  anything is.";

const ASK_AGAIN_TO_WRITE: &str = "Press it again to write the files. Each one is read back \
                                  afterwards, and what does not read back is not followed.";

const NOTHING_TO_TAG: &str = "Nothing to write. Every scanned file already says what the catalog \
                              was told about it.";

const WRITES_SHOWN: usize = 12;

const COVER_ART: &str = "cover art";

impl RootView {
    pub(super) fn folders_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let roots = library.roots().to_vec();
        let busy = library.is_busy();
        let notice = library.notice().cloned();

        let mut listed = rows();
        for root in &roots {
            listed = listed.child(self.folder(root, busy, cx));
        }

        kit::section_body()
            .when(roots.is_empty(), |body| body.child(note(NO_FOLDERS)))
            .when(!roots.is_empty(), |body| body.child(listed))
            .child(hugging(self.add_folder(busy, cx)))
            .when(!roots.is_empty(), |body| body.child(note(FOLDERS_NOTE)))
            .when_some(notice, |body, notice| body.child(listing::noticed(&notice)))
    }

    fn folder(&self, root: &Path, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let path = root.to_path_buf();
        let shown = SharedString::from(root.display().to_string());

        div()
            .id(shown.clone())
            .group(FOLDER_GROUP)
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::border()))
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
                    .text_size(px(theme::text_sm()))
                    .child(shown),
            )
            .child(self.forget_root(path, busy, cx))
    }

    fn forget_root(&self, root: PathBuf, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let id = (ElementId::Path(Arc::from(root.as_path())), "forget");
        let named = format!("forget {}", root.display());

        if busy {
            return kit::mark_when(Press::Greyed, id, Icon::Discard, FORGET_HINT, FOLDER_GROUP);
        }

        self.root_in_the_ring(
            named,
            kit::mark_when(Press::Takes, id, Icon::Discard, FORGET_HINT, FOLDER_GROUP),
            root,
            cx,
        )
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
            .tab_stop(true)
            .key_context(crate::app::CONTROL_CONTEXT)
            .focus(|mark| mark.bg(theme::tinted(theme::accent(), 0x2a)))
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, _, cx| {
                    let root = root.clone();
                    this.library
                        .update(cx, |library, cx| library.forget_root(root, cx));
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let root = clicked.clone();
                this.library
                    .update(cx, |library, cx| library.forget_root(root, cx));
            }))
    }

    fn add_folder(&self, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "add-folder",
            "Add folder…",
            Icon::Plus,
            busy,
            |this, _, cx| this.choose_music_folder(cx),
            self,
            cx,
        )
    }

    pub(super) fn scanning_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let scanning = library.is_scanning();
        let stopping = library.is_stopping();
        let stopped = library.was_stopped();
        let stats = library.stats();
        let enriching = library.is_enriching();
        let held_back = busy || enriching || !library.can_enrich();

        kit::section_body()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.rescan(busy, scanning, cx))
                    .when(scanning, |row| row.child(self.stop_scan(stopping, cx)))
                    .child(self.enrich_now(held_back, enriching, cx)),
            )
            .when(scanning, |body| {
                body.child(progress(read_so_far(stats.unwrap_or_default())))
            })
            .child(note(match stats {
                Some(stats) => SharedString::from(counted(stats, stopped && !scanning)),
                None => SharedString::new_static(SCANNING_NOTE),
            }))
    }

    fn rescan(&self, busy: bool, scanning: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "rescan",
            if scanning {
                "Scanning…"
            } else {
                "Rescan folders"
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

    fn enrich_now(
        &self,
        held_back: bool,
        enriching: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "enrich",
            if enriching { "Enriching…" } else { "Enrich" },
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

    fn stop_scan(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-scan",
            if stopping { "Stopping…" } else { "Stop" },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library.update(cx, |library, cx| library.stop_scan(cx));
            },
            self,
            cx,
        )
    }

    pub(super) fn refreshing_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let enriching = library.is_enriching();
        let covers_held_back = busy || enriching || !library.can_enrich();

        kit::section_body()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(action(
                        "read-everything-again",
                        "Read every file again",
                        Icon::Redo,
                        busy,
                        |this, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.read_everything_again(cx));
                        },
                        self,
                        cx,
                    ))
                    .child(action(
                        "look-for-missing-covers",
                        "Look for missing covers",
                        Icon::Albums,
                        covers_held_back,
                        |this, _, cx| {
                            this.library
                                .update(cx, |library, cx| library.look_for_missing_covers(cx));
                        },
                        self,
                        cx,
                    )),
            )
            .child(note(REFRESHING_NOTE))
    }

    fn choose_music_folder(&self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some(SharedString::new_static("Add")),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the folder picker could not be opened");
                    let reported = this.update(cx, |this, cx| {
                        this.library.update(cx, |library, _| {
                            library.report(Notice::Trouble(error.to_string()));
                        });
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

fn counted(stats: ScanStats, stopped: bool) -> String {
    let counts = format!(
        "discovered {} · read {} · added {} · updated {} · removed {} · failed {}{}",
        stats.discovered,
        stats.processed,
        stats.added,
        stats.updated,
        stats.removed,
        stats.failed.total(),
        broken_down(stats.failed)
    );

    if stopped {
        format!("{counts} · stopped before it finished")
    } else {
        counts
    }
}

impl RootView {
    pub(super) fn resuming_group(&mut self, cx: &mut Context<Self>) -> Div {
        let keeps = self.library.read(cx).resumes();

        kit::section_body()
            .child(self.in_the_ring(
                "resume-the-queue",
                switch_row(
                    "Carry the queue over",
                    "Off, every run opens on an empty queue",
                    keeps,
                    "resume-the-queue",
                ),
                move |this, _, cx| this.set_resume(!keeps, cx),
                cx,
            ))
            .child(note(RESUMING_NOTE))
    }

    pub(super) fn repeating_group(&mut self, cx: &mut Context<Self>) -> Div {
        let skip = self.player.read(cx).state().skip_under_repeat;
        let repeats_the_queue = skip == SkipUnderRepeat::RepeatsTheQueue;
        let flipped = if repeats_the_queue {
            SkipUnderRepeat::KeepsRepeatingTheTrack
        } else {
            SkipUnderRepeat::RepeatsTheQueue
        };

        kit::section_body().child(self.in_the_ring(
            "skip-repeats-queue",
            switch_row(
                "A skip repeats the queue",
                "Off, a skip goes on repeating the track it lands on",
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

    pub(crate) fn set_resume(&self, resume: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.resume = resume);
        self.library
            .update(cx, |library, cx| library.set_resume(resume, cx));
        self.store(&Setting::Resume(resume), cx);
    }
}

impl RootView {
    pub(super) fn organising_group(&mut self, cx: &mut Context<Self>) -> Div {
        let typed = self.organising.read(cx).text().trim().to_owned();
        let refused = Layout::read(&typed).err().map(|error| error.to_string());
        let unreadable = refused.is_some();
        let armed = self.moving_the_files;

        let library = self.library.read(cx);
        let busy = library.is_busy();
        let organising = library.is_organising();
        let moving = library.is_moving();
        let previewed = library.previewed();
        let stopping = library.is_stopping_organise();
        let stats = library.organise_stats();
        let told = library
            .organised()
            .map(|(pass, summary)| filed(summary, pass.applies()));

        kit::section_body()
            .child(kit::field(TEMPLATE_LABEL, self.template_field(cx)))
            .when_some(refused, |body, refused| {
                body.child(listing::noticed(&Notice::Trouble(refused)))
            })
            .child(note(ORGANISING_NOTE))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.preview_the_filing(busy || unreadable, organising && !moving, cx))
                    .child(self.file_the_tracks(
                        busy || unreadable || !previewed,
                        moving,
                        armed,
                        cx,
                    ))
                    .when(organising, |row| row.child(self.stop_filing(stopping, cx))),
            )
            .when(!previewed && !organising, |body| {
                body.child(note(PREVIEW_FIRST))
            })
            .when(armed, |body| body.child(note(ASK_AGAIN_TO_MOVE)))
            .when(organising, |body| {
                body.child(note(going(stats.unwrap_or_default(), moving)))
            })
            .when_some(told, |body, told| body.child(note(told)))
    }

    fn template_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("organise-as")
            .flex()
            .items_center()
            .gap_2()
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.leave_organising(window, cx)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.organising
                    .update(cx, |organising, _| organising.take_focus(window));
                cx.notify();
            }))
            .child(self.organising.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
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
                "Previewing…"
            } else {
                "Preview"
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
                "Moving…"
            } else if armed {
                "Press again to move the files"
            } else {
                "Apply"
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
            if stopping { "Stopping…" } else { "Stop" },
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

    fn arm_the_move(&mut self, cx: &mut Context<Self>) {
        self.moving_the_files = true;
        cx.notify();
    }

    fn file_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.moving_the_files = false;
        let typed = self.organising.read(cx).text().trim().to_owned();

        match Layout::read(&typed) {
            Ok(layout) => self
                .library
                .update(cx, |library, cx| library.organise(layout, pass, cx)),
            Err(error) => self.report(Notice::Trouble(error.to_string()), cx),
        }
        cx.notify();
    }

    pub(crate) fn layout_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let given = self.organising.read(cx).text().trim().to_owned();

        match Layout::read(&given) {
            Ok(_) => {
                self.set_organise_as(given, cx);
                self.leave_organising(window, cx);
            }
            Err(error) => self.report(Notice::Trouble(error.to_string()), cx),
        }
        cx.notify();
    }

    pub(crate) fn set_organise_as(&self, template: String, cx: &mut Context<Self>) {
        self.organising
            .update(cx, |organising, cx| organising.hold(template.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.organise_as = template.clone());
        self.library
            .update(cx, |library, cx| library.forget_the_preview(cx));
        self.store(&Setting::OrganiseAs(template), cx);
    }
}

impl RootView {
    pub(super) fn tagging_group(&mut self, cx: &mut Context<Self>) -> Div {
        let armed = self.writing_the_tags;

        let library = self.library.read(cx);
        let busy = library.is_busy();
        let tagging = library.is_tagging();
        let writing = library.is_writing_tags();
        let previewed = library.previewed_tags();
        let stopping = library.is_stopping_retag();
        let stats = library.retag_stats();
        let told = library
            .tagged()
            .map(|(pass, summary)| (written(summary, pass.applies()), listed(summary)));

        kit::section_body()
            .child(note(TAGGING_NOTE))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.preview_the_tags(busy, tagging && !writing, cx))
                    .child(self.write_the_tags(busy || !previewed, writing, armed, cx))
                    .when(tagging, |row| row.child(self.stop_tagging(stopping, cx))),
            )
            .when(!previewed && !tagging, |body| {
                body.child(note(PREVIEW_TAGS_FIRST))
            })
            .when(armed, |body| body.child(note(ASK_AGAIN_TO_WRITE)))
            .when(tagging, |body| {
                body.child(note(so_far(stats.unwrap_or_default(), writing)))
            })
            .when_some(told, |body, (told, listed)| {
                body.when_some(listed, |body, listed| body.child(listed))
                    .child(note(told))
            })
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
                "Previewing…"
            } else {
                "Preview"
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
                "Writing…"
            } else if armed {
                "Press again to write the files"
            } else {
                "Apply"
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
            if stopping { "Stopping…" } else { "Stop" },
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
        self.writing_the_tags = true;
        cx.notify();
    }

    fn tag_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.writing_the_tags = false;
        self.library
            .update(cx, |library, cx| library.retag(pass, cx));
        cx.notify();
    }
}

fn listed(summary: &RetagSummary) -> Option<Div> {
    let writes = &summary.retagging.writes;
    if writes.is_empty() {
        return None;
    }

    let mut listing = rows();
    for write in writes.iter().take(WRITES_SHOWN) {
        listing = listing.child(write_row(write));
    }
    if let Some(rest) = writes
        .len()
        .checked_sub(WRITES_SHOWN)
        .filter(|rest| *rest > 0)
    {
        listing = listing.child(note(format!("and {rest} more")));
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

    div()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
        .child(named(shown, fields_of(write)))
}

fn fields_of(write: &Written) -> String {
    let mut named: Vec<&str> = write.edits.iter().map(|edit| edit.field.as_str()).collect();
    if write.picture.is_some() {
        named.push(COVER_ART);
    }

    named.join(", ")
}

fn so_far(stats: RetagStats, writing: bool) -> String {
    let counted = format!(
        "already said {} · passed over {} · walked {}",
        stats.unchanged, stats.passed_over, stats.walked
    );

    if writing {
        format!("written {} · {counted}", stats.written)
    } else {
        counted
    }
}

fn written(summary: &RetagSummary, applied: bool) -> String {
    let stats = summary.stats;
    let retagging = &summary.retagging;
    if retagging.writes.is_empty() && retagging.passed_over.is_empty() && stats.unchanged == 0 {
        return NOTHING_TO_TAG.to_owned();
    }

    let (leading, fields, pictures) = if applied {
        (
            format!("written {}", stats.written),
            stats.fields,
            stats.pictures,
        )
    } else {
        (
            format!("would write {}", retagging.writes.len()),
            retagging
                .writes
                .iter()
                .map(|write| write.edits.len() as u64)
                .sum(),
            retagging
                .writes
                .iter()
                .filter(|write| write.picture.is_some())
                .count() as u64,
        )
    };

    let told = format!(
        "{leading} · fields {fields} · pictures {pictures} · already said {} · passed over {}",
        stats.unchanged, stats.passed_over
    );

    let told = if applied {
        told
    } else {
        format!("{told} · nothing has been written")
    };

    if summary.cancelled {
        format!("{told} · stopped before it finished")
    } else {
        told
    }
}

fn going(stats: OrganiseStats, moving: bool) -> String {
    let counted = format!(
        "in place {} · unidentified {} · collided {} · failed {}",
        stats.unchanged, stats.unidentified, stats.collided, stats.failed
    );

    if moving {
        format!("moved {} · {counted}", stats.moved)
    } else {
        counted
    }
}

fn filed(summary: &OrganiseSummary, applied: bool) -> String {
    let stats = summary.stats;
    let (leading, folders) = if applied {
        (
            format!("moved {}", stats.moved),
            format!("folders pruned {}", stats.pruned),
        )
    } else {
        (
            format!("would move {}", summary.plan.files_moving()),
            format!("folders to empty {}", summary.plan.folders.len()),
        )
    };

    let told = format!(
        "{leading} · in place {} · unidentified {} · collided {} · failed {} · {folders}",
        stats.unchanged, stats.unidentified, stats.collided, stats.failed
    );

    let told = if applied {
        told
    } else {
        format!("{told} · nothing has moved")
    };

    if summary.cancelled {
        format!("{told} · stopped before it finished")
    } else {
        told
    }
}

const VAULT_NOTE: &str = "Decodes every scanned track the vault does not hold, throws its tags \
                          and its pictures away, keeps the smallest bit-exact copy it can make \
                          and points the catalog at it. The files it read are left exactly where \
                          they are.";

const NO_VAULT: &str = "No vault is open. Name one with --vault, or set the vault key in \
                        config.toml, and this build will keep what it imports there.";

const PREVIEW_IMPORT_FIRST: &str = "Preview first, so what would be kept is on screen before \
                                    anything is.";

const ASK_AGAIN_TO_KEEP: &str = "Press it again to keep the tracks. Nothing the vault reads is \
                                 moved, renamed or written to.";

const NOTHING_TO_KEEP: &str = "Nothing to import. The vault already holds every scanned track it \
                               can.";

impl RootView {
    pub(super) fn vault_group(&mut self, cx: &mut Context<Self>) -> Div {
        let armed = self.keeping_the_tracks;

        let library = self.library.read(cx);
        if !library.has_a_vault() {
            return kit::section_body().child(note(NO_VAULT));
        }

        let busy = library.is_busy();
        let importing = library.is_importing();
        let keeping = library.is_keeping();
        let previewed = library.previewed_import();
        let stopping = library.is_stopping_import();
        let stats = library.import_stats();
        let told = library
            .imported()
            .map(|(pass, summary)| vaulted(summary, pass.applies()));
        let listed = library
            .imported()
            .filter(|(pass, _)| !pass.applies())
            .map(|(_, summary)| wanted_rows(summary));

        kit::section_body()
            .child(note(VAULT_NOTE))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.preview_the_import(busy, importing && !keeping, cx))
                    .child(self.keep_the_tracks(busy || !previewed, keeping, armed, cx))
                    .when(importing, |row| row.child(self.stop_keeping(stopping, cx))),
            )
            .when(!previewed && !importing, |body| {
                body.child(note(PREVIEW_IMPORT_FIRST))
            })
            .when(armed, |body| body.child(note(ASK_AGAIN_TO_KEEP)))
            .when(importing && keeping, |body| {
                body.child(progress(kept_so_far(stats.unwrap_or_default())))
            })
            .when_some(listed, |body, listed| body.child(listed))
            .when_some(told, |body, told| body.child(note(told)))
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
                "Previewing…"
            } else {
                "Preview"
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
                "Keeping…"
            } else if armed {
                "Press again to keep the tracks"
            } else {
                "Apply"
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
            if stopping { "Stopping…" } else { "Stop" },
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
        self.keeping_the_tracks = true;
        cx.notify();
    }

    fn keep_them(&mut self, pass: Pass, cx: &mut Context<Self>) {
        self.keeping_the_tracks = false;
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

fn wanted_rows(summary: &ImportSummary) -> Div {
    let wanted = &summary.plan.wanted;
    if wanted.is_empty() {
        return div().child(note(NOTHING_TO_KEEP));
    }

    let mut listed = div().flex().flex_col().gap_1();
    for asked in wanted.iter().take(WRITES_SHOWN) {
        listed = listed.child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(rgb(theme::raised()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(theme::text_xs()))
                        .text_color(rgb(theme::text()))
                        .ends_in_an_ellipsis()
                        .child(named_file(&asked.from)),
                )
                .child(kit::figure(format::bytes(asked.bytes)))
                .child(kit::badge(kept_as(asked), theme::muted())),
        );
    }

    if wanted.len() > WRITES_SHOWN {
        listed = listed.child(note(format!("and {} more", wanted.len() - WRITES_SHOWN)));
    }
    listed
}

fn kept_as(asked: &Wanted) -> String {
    if asked.renewing {
        format!("{} again", asked.form.as_str())
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

fn vaulted(summary: &ImportSummary, applied: bool) -> String {
    let stats = summary.stats;
    let told = if applied {
        format!(
            "kept {} · already held {} · covers {} · passed over {} · saved {}",
            stats.vaulted,
            stats.deduped,
            stats.covers,
            stats.passed,
            format::bytes(stats.saved())
        )
    } else {
        format!("would keep {} · nothing has been written", stats.walked)
    };

    if summary.cancelled {
        format!("{told} · stopped before it finished")
    } else {
        told
    }
}

const NO_INBOX: &str = "No inbox is named, so nothing can fill a want. Choose the folder the \
                        files arrive in.";

const INBOX_NOTE: &str = "A file directly inside it named by the recording's MusicBrainz id, the \
                          track's or the ISRC fills the want it names. The folder is read and \
                          never written to.";

impl RootView {
    pub(super) fn inbox_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let inbox = library.inbox().map(Path::to_path_buf);
        let busy = library.is_busy();
        let polling = library.is_polling();
        let stopping = library.is_stopping_poll();
        let held_back = busy || !library.can_poll();
        let told = library.poll_stats().map(|stats| {
            asked_of_the_inbox(
                stats,
                library.polled().is_some_and(|summary| summary.cancelled),
            )
        });

        kit::section_body()
            .child(match &inbox {
                Some(folder) => div().child(inbox_row(folder)),
                None => div().child(note(NO_INBOX)),
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.choose_inbox(busy, cx))
                    .child(self.poll_now(held_back, polling, cx))
                    .when(polling, |row| row.child(self.stop_polling(stopping, cx))),
            )
            .when(inbox.is_some(), |body| body.child(note(INBOX_NOTE)))
            .when_some(told, |body, told| body.child(note(told)))
    }

    fn choose_inbox(&self, busy: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "choose-inbox",
            "Choose folder…",
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
            if polling { "Polling…" } else { "Poll now" },
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
            if stopping { "Stopping…" } else { "Stop" },
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
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::new_static("Choose")),
        });

        cx.spawn(async move |this, cx| {
            let chosen = match picked.await {
                Ok(Ok(Some(chosen))) => chosen,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::error!(%error, "the folder picker could not be opened");
                    let reported = this.update(cx, |this, cx| {
                        this.library.update(cx, |library, _| {
                            library.report(Notice::Trouble(error.to_string()));
                        });
                    });
                    let _ = reported;
                    return;
                }
            };
            let Some(folder) = chosen.into_iter().next() else {
                return;
            };

            let named = this.update(cx, |this, cx| this.set_inbox(folder, cx));
            let _ = named;
        })
        .detach();
    }

    fn set_inbox(&self, folder: PathBuf, cx: &mut Context<Self>) {
        self.store(&Setting::Inbox(folder.clone()), cx);
        self.library
            .update(cx, |library, cx| library.set_inbox(Some(folder), cx));
    }
}

fn inbox_row(folder: &Path) -> Div {
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded_lg()
        .bg(rgb(theme::raised()))
        .border_1()
        .border_color(rgb(theme::border()))
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
                .text_size(px(theme::text_sm()))
                .child(SharedString::from(folder.display().to_string())),
        )
}

fn asked_of_the_inbox(stats: PollStats, stopped: bool) -> String {
    let told = format!(
        "asked {} · kept {} · not kept {} · nothing {} · refused {} · late {}",
        stats.asked, stats.kept, stats.unkept, stats.nothing, stats.refused, stats.late
    );
    if stopped {
        format!("{told} · stopped before it finished")
    } else {
        told
    }
}
