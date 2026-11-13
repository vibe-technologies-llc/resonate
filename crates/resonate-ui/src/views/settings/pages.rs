use std::{
    sync::{Arc, atomic::Ordering},
    thread,
    time::{Duration, Instant},
};

use gpui::TestAppContext;
use parking_lot::Mutex;
use resonate_core::Switch;
use resonate_library::{Library, MusicExtensions};

use super::{Armed, Category, Group, Page};
use crate::{
    ConfigKey, ResonateApp, Setting, SettingChange,
    driven::{Driven, Folder},
    settings::{Settings, SettingsWriter},
};

const SCROLLED_PAST: f32 = 100_000.0;
const SCROLL_STEP: f32 = 300.0;
const SCROLLS_AT_MOST: usize = 60;
const FRAME: Duration = Duration::from_millis(16);
const PATIENCE: Duration = Duration::from_secs(10);

struct Recorded(Arc<Mutex<Vec<SettingChange>>>);

impl Settings for Recorded {
    fn apply(&self, changes: &[SettingChange]) -> crate::Result<()> {
        self.0.lock().extend_from_slice(changes);
        Ok(())
    }
}

struct Pane {
    driven: Driven,
    written: Arc<Mutex<Vec<SettingChange>>>,
}

fn leaked(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

impl Pane {
    fn open(cx: &mut TestAppContext) -> Self {
        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
        Self::over(cx, library)
    }

    fn over(cx: &mut TestAppContext, library: Arc<Library>) -> Self {
        let mut driven = Driven::open(cx, library);
        let written = Arc::new(Mutex::new(Vec::new()));
        let writer = SettingsWriter::over(Arc::new(Recorded(Arc::clone(&written))));
        driven.root.update(&mut driven.cx, |root, _| {
            root.settings_written = writer;
        });
        driven.click("tab-settings");
        Self { driven, written }
    }

    fn category(&mut self, category: Category) {
        let root = self.driven.root.clone();
        self.driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.open_the_place(category, None, cx));
        });
        self.driven.settle();
    }

    fn remembering_the_category(&mut self) {
        let root = self.driven.root.clone();
        self.driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.remember_settings_category(true, cx));
        });
    }

    fn reach(&mut self, selector: &'static str) {
        reach(&mut self.driven, selector);
    }

    fn forgetting_what_was_written(&mut self) {
        self.driven.settle();
        self.written.lock().clear();
    }

    fn written_after(&mut self, wanted: usize) -> Vec<SettingChange> {
        let began = Instant::now();
        loop {
            self.driven.settle();
            let written = self.written.lock().clone();
            if written.len() >= wanted {
                return written;
            }
            assert!(
                began.elapsed() < PATIENCE,
                "{wanted} settings were awaited and these came: {written:?}"
            );
            thread::sleep(FRAME);
        }
    }

    fn pressing(&mut self, selector: &'static str, wanted: usize) -> Vec<SettingChange> {
        self.reach(selector);
        self.forgetting_what_was_written();
        self.driven.click(selector);
        self.written_after(wanted)
    }

    fn typing_into(
        &mut self,
        field: impl FnOnce(
            &crate::views::root::RootView,
            &gpui::App,
        ) -> gpui::Entity<crate::views::field::Field>,
        text: &str,
        wanted: usize,
    ) -> Vec<SettingChange> {
        self.forgetting_what_was_written();
        self.driven.focus_on(field);
        self.driven.cx.simulate_input(text);
        self.driven.cx.simulate_keystrokes("enter");
        self.written_after(wanted)
    }

    fn app<R>(&mut self, read: impl FnOnce(&ResonateApp) -> R) -> R {
        self.driven.read(|_, cx| read(cx.global::<ResonateApp>()))
    }
}

pub(super) fn reach(driven: &mut Driven, selector: &'static str) {
    driven.scroll("settings", -SCROLLED_PAST);
    let body = driven.bounds_of("settings");
    for _ in 0..SCROLLS_AT_MOST {
        let reached = driven
            .cx
            .debug_bounds(selector)
            .is_some_and(|bounds| bounds.top() >= body.top() && bounds.bottom() <= body.bottom());
        if reached {
            return;
        }
        driven.scroll("settings", SCROLL_STEP);
    }
    panic!("{selector} could not be scrolled to");
}

pub(super) fn pressed(driven: &mut Driven, selector: &'static str) {
    reach(driven, selector);
    driven.click(selector);
}

fn stored(setting: Setting) -> SettingChange {
    SettingChange::Store(setting)
}

#[gpui::test]
fn each_category_opens_onto_the_controls_it_holds(cx: &mut TestAppContext) {
    const DRAWN: [(Category, &str); 14] = [
        (Category::Output, "follow-the-default"),
        (Category::Processing, "true-peak-track"),
        (Category::Equaliser, "equaliser-track"),
        (Category::Playback, "resume-the-queue-track"),
        (Category::Library, "choose-vault-folder"),
        (Category::Filters, "music-extension-aac"),
        (Category::Online, "reference-lookups-track"),
        (Category::Recognition, "studying-tracks-track"),
        (Category::Accounts, "subsonic-server"),
        (Category::Sync, "sync-on-track"),
        (Category::Desktop, "notify-on-a-track-change-track"),
        (Category::Appearance, "text-size-0"),
        (Category::Window, "show-the-minimise-button-track"),
        (Category::About, "check-for-updates-track"),
    ];
    let mut pane = Pane::open(cx);

    for (category, control) in DRAWN {
        pane.category(category);

        let opened = pane.driven.read(|root, _| root.settings_shown.category);
        assert_eq!(opened, category);
        pane.reach(control);
    }
    assert_eq!(
        DRAWN.map(|(category, _)| category),
        Category::ALL,
        "a category was added that no page here is opened onto"
    );
}

#[gpui::test]
fn every_page_opens_in_its_own_category_and_leaving_it_returns_to_the_category(
    cx: &mut TestAppContext,
) {
    let mut pane = Pane::open(cx);

    for page in Page::ALL {
        let root = pane.driven.root.clone();
        pane.driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.open_the_settings_page(page, cx)));
        pane.driven.settle();

        let (category, shown) = pane
            .driven
            .read(|root, _| (root.settings_shown.category, root.settings_shown.page));
        assert_eq!((category, shown), (page.category(), Some(page)));

        let left = pane
            .driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.leave_the_settings_page(cx)));
        assert!(left);
        assert_eq!(pane.driven.read(|root, _| root.settings_shown.page), None);
    }
}

#[gpui::test]
fn opening_a_category_remembers_it_unless_it_is_the_one_the_window_opens_on(
    cx: &mut TestAppContext,
) {
    let mut pane = Pane::open(cx);
    pane.remembering_the_category();

    pane.forgetting_what_was_written();
    pane.category(Category::Output);
    pane.category(Category::Online);
    let written = pane.written_after(1);

    assert_eq!(
        written,
        [stored(Setting::LastSettingsCategory(Category::Online))]
    );
}

#[gpui::test]
fn no_longer_remembering_the_category_forgets_the_one_kept(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.remembering_the_category();
    pane.category(Category::Online);
    pane.category(Category::Window);

    let written = pane.pressing("remember-the-settings-category-track", 2);

    assert_eq!(
        written,
        [
            stored(Setting::Switched(Switch::RememberSettingsCategory, false)),
            SettingChange::Forget(ConfigKey::LastSettingsCategory),
        ]
    );
}

#[gpui::test]
fn the_output_page_writes_a_switch_it_flips(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Output);
    let raised = pane
        .driven
        .read(|root, cx| root.player.read(cx).output_settings().dsd_like_pcm);

    let written = pane.pressing("dsd-like-pcm-track", 1);

    assert_eq!(written, [stored(Setting::DsdLikePcm(!raised))]);
}

#[gpui::test]
fn the_processing_page_writes_a_switch_it_flips(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Processing);
    let guarded = pane
        .driven
        .read(|root, cx| root.player.read(cx).output_settings().true_peak);

    let written = pane.pressing("true-peak-track", 1);

    assert_eq!(written, [stored(Setting::TruePeak(!guarded))]);
}

#[gpui::test]
fn the_equaliser_page_writes_the_switch_it_flips(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Equaliser);
    let on = pane
        .driven
        .read(|root, cx| root.equaliser.read(cx).bindings().enabled);

    let written = pane.pressing("equaliser-track", 1);

    assert_eq!(written, [stored(Setting::Switched(Switch::Equaliser, !on))]);
}

#[gpui::test]
fn the_playback_page_writes_the_resume_switch_and_the_shuffle_style(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Playback);
    let resumes = pane.driven.read(|root, cx| root.library.read(cx).resumes());

    let written = pane.pressing("resume-the-queue-track", 1);
    assert_eq!(
        written,
        [stored(Setting::Switched(Switch::Resume, !resumes))]
    );

    let written = pane.pressing("shuffle-style-0", 1);
    assert!(matches!(
        written.as_slice(),
        [SettingChange::Store(Setting::ShuffleStyle(_))]
    ));
}

#[gpui::test]
fn the_library_page_writes_the_vault_a_folder_is_made_into(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Library);
    let folder = Folder::new();
    let named = folder.path().to_path_buf();
    pane.forgetting_what_was_written();

    let root = pane.driven.root.clone();
    pane.driven.cx.update(|_, cx| {
        root.update(cx, |root, cx| root.name_the_vault(named.clone(), cx));
    });
    let written = pane.written_after(1);

    assert_eq!(written, [stored(Setting::Vault(named.clone()))]);
    assert_eq!(
        pane.driven
            .read(|root, _| root.settings_shown.vault_named.clone()),
        Some(named)
    );
}

#[gpui::test]
fn the_library_page_refuses_a_vault_inside_a_folder_the_library_scans(cx: &mut TestAppContext) {
    let music = Folder::new();
    let inside = music.path().join("vault");
    std::fs::create_dir_all(&inside).expect("a folder inside");
    let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));
    Driven::scanned(&library, &music);
    let mut pane = Pane::over(cx, library);
    pane.category(Category::Library);
    pane.forgetting_what_was_written();

    let root = pane.driven.root.clone();
    pane.driven.cx.update(|_, cx| {
        root.update(cx, |root, cx| root.name_the_vault(inside.clone(), cx));
    });
    pane.driven.settle();

    assert_eq!(*pane.written.lock(), Vec::new());
    assert_eq!(
        pane.driven
            .read(|root, _| root.settings_shown.vault_named.clone()),
        None
    );
}

#[gpui::test]
fn the_filters_page_writes_the_extensions_left_after_one_is_dropped(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Filters);

    let written = pane.pressing("music-extension-aac", 1);

    let [SettingChange::Store(Setting::MusicExtensions(left))] = written.as_slice() else {
        panic!("one set of extensions was awaited, not {written:?}");
    };
    assert!(left.selected().all(|extension| extension != "aac"));
    assert_ne!(*left, MusicExtensions::default());
}

#[gpui::test]
fn the_online_page_writes_the_network_switch(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Online);
    let on = pane.app(|app| app.online.enabled);

    let written = pane.pressing("reference-lookups-track", 1);

    assert_eq!(written, [stored(Setting::Switched(Switch::Online, !on))]);
}

#[gpui::test]
fn the_recognition_page_writes_the_study_switch(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Recognition);
    let studies = pane.driven.read(|root, cx| root.library.read(cx).studies());

    let written = pane.pressing("studying-tracks-track", 1);

    assert_eq!(
        written,
        [stored(Setting::Switched(Switch::Study, !studies))]
    );
}

#[gpui::test]
fn the_accounts_page_writes_a_user_typed_and_refuses_a_server_that_is_not_one(
    cx: &mut TestAppContext,
) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Accounts);

    let written = pane.typing_into(
        |root, cx| root.settings_fields.read(cx).subsonic[1].clone(),
        "alice",
        1,
    );
    assert_eq!(written, [stored(Setting::SubsonicUser("alice".to_owned()))]);

    pane.forgetting_what_was_written();
    pane.driven
        .focus_on(|root, cx| root.settings_fields.read(cx).subsonic[0].clone());
    pane.driven.cx.simulate_input("nonsense");
    pane.driven.cx.simulate_keystrokes("enter");
    pane.driven.settle();
    assert_eq!(*pane.written.lock(), Vec::new());
}

#[gpui::test]
fn the_sync_page_writes_the_switch_it_flips(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Sync);
    let on = pane.app(|app| app.synchronised.enabled);

    let written = pane.pressing("sync-on-track", 1);

    assert_eq!(written, [stored(Setting::Switched(Switch::Sync, !on))]);
}

#[gpui::test]
fn the_desktop_page_writes_the_notification_switch(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Desktop);
    let tells = pane.app(|app| app.notify.load(Ordering::Acquire));

    let written = pane.pressing("notify-on-a-track-change-track", 1);

    assert_eq!(written, [stored(Setting::Switched(Switch::Notify, !tells))]);
}

#[gpui::test]
fn the_appearance_page_writes_the_text_size_chosen_and_stops_following_the_desktop(
    cx: &mut TestAppContext,
) {
    let mut pane = Pane::open(cx);
    pane.driven.wearing_alone();
    pane.category(Category::Appearance);

    let written = pane.pressing("text-size-0", 2);

    assert!(matches!(
        written.as_slice(),
        [
            SettingChange::Store(Setting::TextSize(_)),
            SettingChange::Store(Setting::FollowDesktopTextSize(false)),
        ]
    ));
}

#[gpui::test]
fn the_window_page_writes_the_button_switch(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Window);
    let shown = pane.app(|app| app.window_buttons.minimise);

    let written = pane.pressing("show-the-minimise-button-track", 1);

    assert_eq!(written, [stored(Setting::MinimiseButton(!shown))]);
}

#[gpui::test]
fn the_about_page_writes_the_update_switch(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::About);
    let checks = pane.app(|app| app.check_for_updates);

    let written = pane.pressing("check-for-updates-track", 1);

    assert_eq!(
        written,
        [stored(Setting::Switched(Switch::CheckForUpdates, !checks))]
    );
}

#[gpui::test]
fn a_group_moved_off_its_default_offers_to_be_put_back_and_writes_the_default_when_asked(
    cx: &mut TestAppContext,
) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Online);
    if !pane.app(|app| app.online.enabled) {
        pane.pressing("reference-lookups-track", 1);
    }
    pane.pressing("reference-lookups-track", 1);

    let written = pane.pressing(leaked(format!("revert-{}", Group::Lookups.title())), 1);

    assert!(written.contains(&stored(Setting::Switched(
        Switch::Online,
        Switch::Online.on_unless_said()
    ))));
}

#[gpui::test]
fn resetting_a_category_puts_each_of_its_groups_back(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::Online);
    if !pane.app(|app| app.online.enabled) {
        pane.pressing("reference-lookups-track", 1);
    }
    pane.pressing("reference-lookups-track", 1);

    let written = pane.pressing("reset-the-category", 1);

    assert!(written.contains(&stored(Setting::Switched(
        Switch::Online,
        Switch::Online.on_unless_said()
    ))));
}

#[gpui::test]
fn leaving_the_category_lowers_a_press_armed_in_it(cx: &mut TestAppContext) {
    let mut pane = Pane::open(cx);
    pane.category(Category::About);
    pane.reach("reset-everything");
    pane.driven.click("reset-everything");
    assert!(
        pane.driven
            .read(|root, _| root.is_armed(&Armed::ResetEverything))
    );

    pane.category(Category::Online);

    assert!(
        !pane
            .driven
            .read(|root, _| root.is_armed(&Armed::ResetEverything))
    );
}
