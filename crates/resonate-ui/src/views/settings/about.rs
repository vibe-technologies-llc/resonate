use std::{sync::Arc, time::Duration};

use gpui::{Context, Task, prelude::*};

use crate::{
    Newer, ResonateApp, Setting, fonts,
    icons::Icon,
    models::Notice,
    toast,
    views::{
        kit,
        root::RootView,
        settings::{Armed, Group, Rows, action, reading, switch_row},
    },
    words,
};

const VERSION: &str = env!("CARGO_PKG_VERSION");

const LOOKED_AT_AGAIN_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

impl RootView {
    pub(super) fn build_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let faces = fonts::drawn_in();
        let sinks = self.player.read(cx).sinks().len();
        let reference = self.library.read(cx).has_reference();

        kit::rows()
            .child(reading(words::settings_about::version(), VERSION))
            .child(reading(words::settings_about::body_face(), faces.body))
            .child(reading(words::settings_about::codes(), faces.monospace))
            .child(reading(
                words::settings_about::sinks(),
                words::settings_about::sinks_advertised(sinks),
            ))
            .child(reading(
                words::settings_about::reference(),
                if reference {
                    words::settings_about::musicbrainz_the_cover_art_archive_commons()
                } else {
                    words::settings_about::none_this_run_the_feature_is()
                },
            ))
            .note(words::settings_about::this_build())
    }

    pub(super) fn updates_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let app = cx.global::<ResonateApp>();
        let checks = app.check_for_updates;
        let can_ask = app.updates.is_some();

        let mut rows = kit::rows().child(self.in_the_ring(
            "check-for-updates",
            switch_row(
                words::settings_about::check_for_updates(),
                if checks {
                    words::settings_about::tell_me_when_a_newer_release()
                } else {
                    words::settings_about::off_nothing_is_asked()
                },
                checks,
                "check-for-updates",
            ),
            move |this, _, cx| this.check_for_updates(!checks, cx),
            cx,
        ));
        if let Some(newer) = self.newer.clone() {
            let page = newer.page.clone();
            let version = newer.version.clone();
            rows = rows.child(kit::row(
                words::settings_about::newer_is_out(newer.version.to_string()),
                words::settings_about::this_build_is(VERSION),
                gpui::div()
                    .flex()
                    .gap_2()
                    .child(action(
                        "open-the-release",
                        words::settings_about::open_the_release(),
                        Icon::Globe,
                        false,
                        move |_, _, cx| cx.open_url(&page),
                        self,
                        cx,
                    ))
                    .child(action(
                        "dismiss-the-release",
                        words::settings_about::dismiss(),
                        Icon::Close,
                        false,
                        move |this, _, cx| this.dismiss_the_release(&version, cx),
                        self,
                        cx,
                    )),
            ));
        }
        rows.note(if can_ask {
            words::settings_about::updates()
        } else {
            words::settings_about::nowhere_to_ask()
        })
    }

    pub(crate) fn check_for_updates(&mut self, checks: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.check_for_updates = checks);
        self.store(&Setting::CheckForUpdates(checks), cx);
        if checks {
            self.look_for_a_newer_release(cx);
        } else {
            self.looking_for_updates = Task::ready(());
            self.newer = None;
        }
        cx.notify();
    }

    pub(super) fn put_the_updates_back(&mut self, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.update_dismissed = None);
        self.check_for_updates(super::defaults::CHECK_FOR_UPDATES, cx);
    }

    fn dismiss_the_release(&mut self, version: &str, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.update_dismissed = Some(version.to_owned());
        });
        self.store(&Setting::UpdateDismissed(version.to_owned()), cx);
        self.newer = None;
        cx.notify();
    }

    pub(crate) fn look_for_a_newer_release(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(updates) = app.updates.clone().filter(|_| app.check_for_updates) else {
            self.looking_for_updates = Task::ready(());
            return;
        };

        self.looking_for_updates = cx.spawn(async move |this, cx| {
            loop {
                let reaching = cx.update(|cx| cx.global::<ResonateApp>().online.enabled);
                let asking = Arc::clone(&updates);
                let newer = cx
                    .background_executor()
                    .spawn(async move { asking.newer(reaching) })
                    .await;
                if this
                    .update(cx, |this, cx| this.heard_of_a_release(newer, cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(LOOKED_AT_AGAIN_EVERY).await;
            }
        });
    }

    pub(crate) fn heard_of_a_release(&mut self, newer: Option<Newer>, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let dismissed = app.update_dismissed.clone();
        let newer = newer
            .filter(|_| app.check_for_updates)
            .filter(|newer| dismissed.as_deref() != Some(newer.version.as_str()));
        let told_already = newer == self.newer;
        self.newer = newer;

        if let Some(newer) = self.newer.as_ref().filter(|_| !told_already) {
            toast::tell(
                Notice::Noted(words::settings_about::newer_is_out_toast(
                    newer.version.to_string(),
                )),
                cx,
            );
        }
        cx.notify();
    }

    pub(super) fn places_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let places = cx.global::<ResonateApp>().places.clone();

        kit::rows()
            .child(reading(
                words::settings_about::settings(),
                places.config.display().to_string(),
            ))
            .child(reading(
                words::settings_about::catalog(),
                places.library.display().to_string(),
            ))
            .note(words::settings_about::places())
    }

    pub(super) fn everything_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let armed = self.is_armed(&Armed::ResetEverything);

        kit::rows().child(kit::row(
            words::settings_about::every_setting(),
            if armed {
                words::settings_about::ask_again()
            } else if self.reset_everything_landed {
                words::settings_about::put_back()
            } else {
                words::settings_about::start_again()
            },
            action(
                "reset-everything",
                if armed {
                    words::settings_about::press_again_to_reset_everything()
                } else {
                    words::settings_about::reset_everything()
                },
                Icon::Undo,
                false,
                move |this, _, cx| {
                    if armed {
                        this.reset_everything(cx);
                    } else {
                        this.arm_the_reset(cx);
                    }
                },
                self,
                cx,
            ),
        ))
    }

    fn arm_the_reset(&mut self, cx: &mut Context<Self>) {
        self.arm(Armed::ResetEverything);
        self.reset_everything_landed = false;
        cx.notify();
    }

    fn reset_everything(&mut self, cx: &mut Context<Self>) {
        for group in Group::ALL {
            self.put_back(group, cx);
        }
        self.lower(&Armed::ResetEverything);
        self.reset_everything_landed = true;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gpui::TestAppContext;
    use resonate_library::Library;

    use super::*;
    use crate::{Updates, driven::Driven};

    #[derive(Default)]
    struct Released {
        asked: AtomicUsize,
    }

    impl Updates for Released {
        fn newer(&self, reaching: bool) -> Option<Newer> {
            assert!(reaching, "asked while the window was offline");
            self.asked.fetch_add(1, Ordering::Relaxed);
            Some(Newer {
                version: "99.0.0".to_owned(),
                page: "https://github.com/someone/resonate/releases/tag/v99.0.0".to_owned(),
            })
        }
    }

    fn opened_with(cx: &mut TestAppContext, released: &Arc<Released>) -> Driven {
        let mut driven = Driven::open(cx, Arc::new(Library::open_in_memory().expect("a catalog")));
        let seam = Arc::clone(released) as Arc<dyn Updates>;

        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|app, _| {
                app.updates = Some(seam);
                app.check_for_updates = true;
                app.online.enabled = true;
            });
        });
        driven
    }

    fn look(driven: &mut Driven) {
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.look_for_a_newer_release(cx)));
    }

    #[gpui::test]
    fn a_newer_release_is_told_once_offered_in_about_and_dismissed_for_good(
        cx: &mut TestAppContext,
    ) {
        let released = Arc::new(Released::default());
        let mut driven = opened_with(cx, &released);

        look(&mut driven);
        driven.until(|root, _| root.newer.is_some());
        let told = driven.read(|_, cx| toast::showing_text(cx));
        assert_eq!(
            told.as_deref(),
            Some("Resonate 99.0.0 is out · Settings, About has it")
        );

        driven.click("tab-settings");
        driven.focus(|root| &root.finding);
        driven.cx.simulate_input("updates");
        driven.settle();
        assert!(driven.cx.debug_bounds("open-the-release").is_some());
        driven.click("dismiss-the-release");
        driven.settle();

        assert!(driven.read(|root, _| root.newer.is_none()));
        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().update_dismissed.clone()),
            Some("99.0.0".to_owned())
        );
        look(&mut driven);
        driven.until(|_, _| released.asked.load(Ordering::Relaxed) == 2);
        driven.settle();
        assert!(driven.read(|root, _| root.newer.is_none()));
    }

    #[gpui::test]
    fn switching_the_check_off_forgets_the_release_and_asks_no_more(cx: &mut TestAppContext) {
        let released = Arc::new(Released::default());
        let mut driven = opened_with(cx, &released);

        look(&mut driven);
        driven.until(|root, _| root.newer.is_some());
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.check_for_updates(false, cx)));
        look(&mut driven);
        driven.settle();

        assert!(driven.read(|root, _| root.newer.is_none()));
        assert!(!driven.read(|_, cx| cx.global::<ResonateApp>().check_for_updates));
        assert_eq!(released.asked.load(Ordering::Relaxed), 1);
    }
}
