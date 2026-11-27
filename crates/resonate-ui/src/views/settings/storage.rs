use std::path::Path;

use gpui::{Context, Div, SharedString, Stateful, div, prelude::*};
use resonate_core::TrackId;
use resonate_library::Ceiling;

use crate::{
    ResonateApp, Setting, format,
    icons::Icon,
    models::StoredFile,
    views::{
        browser::syncs_with_other_devices,
        kit::{self, Press},
        root::{Deleting, RootView},
        settings::{Armed, Choice, Rows, action, note, progress},
    },
    words,
};

pub(crate) const RECOMPRESS_TO_ID: &str = "recompress-to";
const SEPARATED_BY: &str = " \u{b7} ";

impl Choice for Ceiling {
    const ALL: &'static [Self] = &Ceiling::ALL;

    fn label(self) -> &'static str {
        match self {
            Self::Kept => words::settings_storage::as_it_is(),
            Self::Studio => words::settings_storage::studio(),
            Self::High => words::settings_storage::high(),
            Self::Cd => words::settings_storage::cd(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Kept => words::settings_storage::as_it_is_means(),
            Self::Studio => words::settings_storage::studio_means(),
            Self::High => words::settings_storage::high_means(),
            Self::Cd => words::settings_storage::cd_means(),
        })
    }
}

impl RootView {
    pub(super) fn recompressing_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let ceiling = cx.global::<ResonateApp>().recompress_to;
        let library = self.library.read(cx);
        let busy = library.is_busy();
        let running = library.is_recompressing_every_flac();
        let stopping = library.is_stopping_recompress();
        let stats = library.recompress_stats();
        let armed = self.is_armed(&Armed::RecompressEveryFlac, cx) && !busy;

        kit::rows()
            .child(kit::field(
                words::settings_storage::highest_quality_kept(),
                self.choices(RECOMPRESS_TO_ID, ceiling, cx, |this, ceiling, cx| {
                    this.recompress_to(ceiling, cx);
                }),
            ))
            .child(
                kit::action_row()
                    .child(self.recompress_every_flac_button(busy, running, armed, cx))
                    .when(running, |row| {
                        row.child(self.stop_recompressing_button(stopping, cx))
                    }),
            )
            .when(armed, |body| {
                body.child(note(words::settings_storage::ask_again_to_recompress()))
            })
            .when_some(stats.filter(|_| running), |body, stats| {
                let share = if stats.candidates == 0 {
                    0.0
                } else {
                    stats.weighed as f32 / stats.candidates as f32
                };

                body.child(progress(share)).child(note(
                    words::settings_storage::RecompressingSoFar {
                        candidates: format::count(stats.candidates),
                        saved: format::bytes(stats.saved()),
                        smaller: format::count(stats.smaller),
                        weighed: format::count(stats.weighed),
                    }
                    .said(),
                ))
            })
            .note(words::settings_storage::recompressing_note())
    }

    fn recompress_every_flac_button(
        &self,
        held_back: bool,
        running: bool,
        armed: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        action(
            "recompress-every-flac",
            if running {
                words::settings_storage::recompressing_every_flac()
            } else if armed {
                words::settings_storage::press_again_to_recompress()
            } else {
                words::settings_storage::recompress_every_flac()
            },
            Icon::Compress,
            held_back,
            move |this, _, cx| {
                if armed {
                    this.recompress_every_flac(cx);
                } else {
                    this.arm(Armed::RecompressEveryFlac, cx);
                    cx.notify();
                }
            },
            self,
            cx,
        )
    }

    fn stop_recompressing_button(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-recompressing",
            if stopping {
                words::settings_storage::stopping()
            } else {
                words::settings_storage::stop()
            },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_recompress(cx));
            },
            self,
            cx,
        )
    }

    pub(crate) fn recompress_to(&mut self, ceiling: Ceiling, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.recompress_to = ceiling);
        self.store(&Setting::RecompressTo(ceiling), cx);
        cx.notify();
    }

    fn recompress_every_flac(&mut self, cx: &mut Context<Self>) {
        let ceiling = cx.global::<ResonateApp>().recompress_to;

        self.lower(&Armed::RecompressEveryFlac, cx);
        self.library
            .update(cx, |library, cx| library.recompress_every_flac(ceiling, cx));
        cx.notify();
    }

    pub(crate) fn recompress_the_song(
        &mut self,
        track: TrackId,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let ceiling = cx.global::<ResonateApp>().recompress_to;

        self.close_the_menu(cx);
        self.library.update(cx, |library, cx| {
            library.recompress_the_file(track, title, ceiling, cx);
        });
        cx.notify();
    }

    pub(super) fn storage_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);

        if !library.has_read_the_files() && !library.is_reading_the_files() {
            let asked = self.library.clone();

            cx.defer(move |cx| {
                asked.update(cx, |library, cx| library.ask_for_the_largest_files(cx));
            });
        }

        let library = self.library.read(cx);
        let files = library.largest_files();
        let more = library.holds_more_files();
        let reading = library.is_reading_the_files();
        let busy = library.is_busy();
        let recompressing = library.recompressing_track();
        let everywhere = syncs_with_other_devices(cx);

        if files.is_empty() {
            return kit::rows()
                .child(note(if reading || !library.has_read_the_files() {
                    words::settings_storage::reading_the_files()
                } else {
                    words::settings_storage::no_files()
                }))
                .note(words::settings_storage::largest_files_note());
        }

        let mut listed = kit::rows();

        for stored in files.iter() {
            let Some(first) = stored.first() else {
                continue;
            };
            let at_work = recompressing == Some(first);

            listed = listed.child(kit::row(
                name_of(stored),
                under_it(stored),
                self.file_controls(
                    stored,
                    first,
                    FileGestures {
                        busy,
                        at_work,
                        everywhere,
                    },
                    cx,
                ),
            ));
        }

        listed
            .when(more, |body| {
                body.child(kit::action_row().child(action(
                    "storage-show-more",
                    words::settings_storage::show_more(),
                    Icon::ChevronDown,
                    reading,
                    |this, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.show_more_files(cx));
                    },
                    self,
                    cx,
                )))
            })
            .note(words::settings_storage::largest_files_note())
    }

    fn file_controls(
        &self,
        stored: &StoredFile,
        first: TrackId,
        gestures: FileGestures,
        cx: &mut Context<Self>,
    ) -> Div {
        let name = name_of(stored);
        let deleting = Deleting::each(&stored.songs);
        let gone_everywhere = deleting.clone().on_every_device();
        let id = first.get();
        let title = name.clone();

        let recompress = stored.recompresses().then(|| {
            let saying = if gestures.at_work {
                words::settings_storage::recompressing_the_file(name.as_str())
            } else {
                words::settings_storage::recompress_the_file(name.as_str())
            };
            let named = format!("storage-recompress-{id}");

            if gestures.busy {
                return kit::icon_button_when(Press::Greyed, named, Icon::Compress, saying);
            }
            self.in_the_ring_at(
                named.clone(),
                kit::icon_button(named, Icon::Compress, saying),
                move |this, _, cx| this.recompress_the_song(first, title.clone(), cx),
                cx,
            )
        });

        let delete_named = format!("storage-delete-{id}");
        let delete = self.in_the_ring_at(
            delete_named.clone(),
            kit::icon_button(
                delete_named,
                Icon::Delete,
                words::settings_storage::delete_the_file(name.as_str()),
            ),
            move |this, _, cx| this.ask_to_delete(deleting.clone(), cx),
            cx,
        );

        let everywhere = gestures.everywhere.then(|| {
            let named = format!("storage-delete-everywhere-{id}");

            self.in_the_ring_at(
                named.clone(),
                kit::icon_button(
                    named,
                    Icon::Sync,
                    words::settings_storage::delete_the_file_everywhere(name.as_str()),
                ),
                move |this, _, cx| this.ask_to_delete(gone_everywhere.clone(), cx),
                cx,
            )
        });

        div()
            .flex()
            .items_center()
            .gap_2()
            .child(kit::format_badge(stored.file.codec, stored.file.spec))
            .children(recompress)
            .child(delete)
            .children(everywhere)
    }
}

#[derive(Clone, Copy)]
struct FileGestures {
    busy: bool,
    at_work: bool,
    everywhere: bool,
}

fn name_of(stored: &StoredFile) -> String {
    match stored.songs.as_slice() {
        [only] if !stored.is_a_sheets() => only.title.clone(),
        _ => file_name(&stored.file.path),
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn under_it(stored: &StoredFile) -> String {
    let size = format::bytes(stored.file.bytes);
    let whose = match stored.songs.as_slice() {
        [only] if !stored.is_a_sheets() => only.artist.clone().unwrap_or_default(),
        songs => words::settings_storage::songs_in_the_file(songs.len()),
    };
    let vaulted = if stored.file.vaulted {
        words::settings_storage::in_the_vault()
    } else {
        ""
    };

    [size.as_str(), whose.as_str(), vaulted]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(SEPARATED_BY)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::TestAppContext;
    use resonate_library::Library;

    use super::*;
    use crate::{
        Continuing, HandedOff, SyncShown, Syncing,
        driven::{Driven, Folder},
        models::FILES_A_PAGE,
        views::settings::{Page, pages::reach},
    };

    struct Paired;

    impl Syncing for Paired {
        fn shown(&self) -> SyncShown {
            SyncShown::default()
        }

        fn force(&self) {}

        fn forget(&self, _: &str) -> bool {
            false
        }

        fn missing_from(&self, _: resonate_core::PlaylistId) -> usize {
            0
        }

        fn fetch_missing(&self, _: resonate_core::PlaylistId, _: &str) {}

        fn keeps_songs_of(&self, _: resonate_core::PlaylistId) -> bool {
            false
        }

        fn keep_songs_of(&self, _: resonate_core::PlaylistId, _: bool) {}

        fn handed_off(&self) -> Vec<HandedOff> {
            Vec::new()
        }

        fn continue_from(&self, _: &str) -> Continuing {
            Continuing::default()
        }
    }

    fn leaked(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    fn scanned(folder: &Folder) -> Arc<Library> {
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

        Driven::scanned(&library, folder);
        library
    }

    fn on_the_page(cx: &mut TestAppContext, folder: &Folder) -> Driven {
        let library = scanned(folder);
        let mut driven = Driven::opened_in(cx, library, folder);

        driven.root.update(&mut driven.cx, |root, cx| {
            root.set_pane(crate::Pane::Settings, cx);
            root.open_the_settings_page(Page::Storage, cx);
        });
        driven.until(|root, cx| root.library.read(cx).has_read_the_files());
        driven
    }

    fn listed(driven: &mut Driven) -> Arc<[StoredFile]> {
        driven.read(|root, cx| root.library.read(cx).largest_files())
    }

    #[gpui::test]
    fn the_page_lists_the_files_largest_first_and_shows_more_a_page_at_a_time(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();

        folder.tone("Long.wav", 3);
        folder.tone("Short.wav", 1);
        for filler in 0..FILES_A_PAGE {
            folder.tone(&format!("Filler {filler:02}.wav"), 2);
        }

        let mut driven = on_the_page(cx, &folder);

        let first = listed(&mut driven);
        let more = driven.read(|root, cx| root.library.read(cx).holds_more_files());

        assert_eq!(first.len(), FILES_A_PAGE);
        assert!(more);
        assert!(first[0].file.path.ends_with("Long.wav"));
        assert!(
            first
                .windows(2)
                .all(|pair| pair[0].file.bytes >= pair[1].file.bytes)
        );

        driven.root.update(&mut driven.cx, |root, cx| {
            root.library
                .update(cx, |library, cx| library.show_more_files(cx));
        });
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library.has_read_the_files() && library.largest_files().len() == FILES_A_PAGE + 2
        });

        let every = listed(&mut driven);
        let more = driven.read(|root, cx| root.library.read(cx).holds_more_files());
        let mut paths: Vec<_> = every
            .iter()
            .map(|stored| stored.file.path.clone())
            .collect();

        paths.sort();
        paths.dedup();

        assert!(!more);
        assert!(every[FILES_A_PAGE + 1].file.path.ends_with("Short.wav"));
        assert_eq!(paths.len(), FILES_A_PAGE + 2);
    }

    #[gpui::test]
    fn a_file_deleted_from_the_page_is_asked_about_first_and_every_device_only_while_syncing(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let doomed = folder.tone("Doomed.wav", 3);
        let kept = folder.tone("Kept.wav", 1);

        let mut driven = on_the_page(cx, &folder);

        let largest = listed(&mut driven)[0].clone();
        let id = largest.first().expect("a song in the file").get();
        let delete = leaked(format!("storage-delete-{id}"));
        let everywhere = leaked(format!("storage-delete-everywhere-{id}"));

        reach(&mut driven, delete);

        assert!(driven.cx.debug_bounds(everywhere).is_none());

        driven.click(delete);

        let asked = driven
            .read(|root, _| root.deleting.clone())
            .expect("the deletion is asked about");

        assert!(!asked.everywhere);
        assert_eq!(asked.tracks, largest.file.tracks);

        driven.click("keep-the-track");

        assert!(doomed.exists());

        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|app, _| {
                app.syncing = Some(Arc::new(Paired));
                app.synchronised.enabled = true;
            });
        });
        driven.settle();
        reach(&mut driven, everywhere);
        driven.click(everywhere);

        let asked = driven
            .read(|root, _| root.deleting.clone())
            .expect("the deletion is asked about");

        assert!(asked.everywhere);

        driven.click("delete-the-track");
        driven.until(|_, _| !doomed.exists());
        driven.until(|root, cx| root.library.read(cx).largest_files().len() == 1);

        assert!(kept.exists());
    }

    #[gpui::test]
    fn a_flac_on_the_page_is_re_compressed_and_told_and_a_wave_is_not_offered(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let wave = folder.tone("Plain.wav", 1);

        let Some(flac) = folder.encoded_titled(
            "Loose.flac",
            &["-c:a", "flac", "-compression_level", "0"],
            "Loose",
        ) else {
            return;
        };

        let mut driven = on_the_page(cx, &folder);

        let files = listed(&mut driven);
        let of = |ending: &str| {
            files
                .iter()
                .find(|stored| stored.file.path.ends_with(ending))
                .and_then(StoredFile::first)
                .expect("the file is listed")
                .get()
        };
        let recompress = leaked(format!("storage-recompress-{}", of("Loose.flac")));
        let not_offered = leaked(format!("storage-recompress-{}", of("Plain.wav")));

        reach(&mut driven, recompress);

        assert!(driven.cx.debug_bounds(not_offered).is_none());

        driven.click(recompress);
        driven.until_said(|said| said.contains("Loose"));
        driven.until(|root, cx| !root.library.read(cx).is_busy());

        assert!(flac.exists());
        assert!(wave.exists());
    }
}
