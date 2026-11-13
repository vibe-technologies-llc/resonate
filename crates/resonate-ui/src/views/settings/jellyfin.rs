use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gpui::{
    BorrowAppContext as _, Context, Div, InteractiveElement as _, ParentElement as _, Styled as _,
    Task, div,
};

use crate::{
    JellyfinBegun, JellyfinSession, JellyfinSignIn, Notice, ResonateApp, Setting,
    icons::Icon,
    views::{
        kit,
        root::RootView,
        settings::{action, note},
    },
    words,
};

const EMBY_ROUTE: &str = "/emby";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    Idle,
    Asking,
    Waiting(String),
}

pub(crate) struct QuickConnecting {
    shown: Shown,
    stopped: Arc<AtomicBool>,
    task: Task<()>,
}

impl Default for QuickConnecting {
    fn default() -> Self {
        Self {
            shown: Shown::Idle,
            stopped: Arc::new(AtomicBool::new(false)),
            task: Task::ready(()),
        }
    }
}

impl QuickConnecting {
    #[cfg(test)]
    pub(crate) fn shown_waiting(&mut self, code: String) {
        self.shown = Shown::Waiting(code);
    }

    #[cfg(test)]
    pub(crate) fn stopping(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stopped)
    }
}

fn offers_quick_connect(address: &str) -> bool {
    let address = address.trim().trim_end_matches('/');
    !address.is_empty() && !address.to_ascii_lowercase().ends_with(EMBY_ROUTE)
}

impl RootView {
    fn sign_in_with_quick_connect(&mut self, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(signs_in) = app.signs_in_to_jellyfin.clone() else {
            return;
        };
        let address = app.online.jellyfin.trim().to_owned();
        if !offers_quick_connect(&address) {
            return;
        }

        let asked_at = address.clone();
        let stopped = Arc::new(AtomicBool::new(false));
        self.accounts.jellyfin_quick.stopped = Arc::clone(&stopped);
        self.accounts.jellyfin_quick.shown = Shown::Asking;
        cx.notify();
        self.accounts.jellyfin_quick.task = cx.spawn(async move |this, cx| {
            let beginning = Arc::clone(&signs_in);
            let begun = cx
                .background_executor()
                .spawn(async move { beginning.begun(&address) })
                .await;
            let code = match begun {
                JellyfinBegun::Code(code) => code,
                JellyfinBegun::Off | JellyfinBegun::Failed => {
                    let said = if begun == JellyfinBegun::Off {
                        Notice::Noted(words::settings_jellyfin::quick_connect_off().to_owned())
                    } else {
                        Notice::Trouble(words::settings_jellyfin::not_begun().to_owned())
                    };
                    let told = this.update(cx, |this, cx| {
                        this.accounts.jellyfin_quick.shown = Shown::Idle;
                        this.report(said, cx);
                        cx.notify();
                    });
                    let _ = told;
                    return;
                }
            };
            let waiting_on = code.clone();
            let shown = this.update(cx, |this, cx| {
                this.accounts.jellyfin_quick.shown = Shown::Waiting(code);
                cx.notify();
            });
            if shown.is_err() {
                return;
            }
            let answered = cx
                .background_executor()
                .spawn(async move { signs_in.approved(&waiting_on, &stopped) })
                .await;
            let told = this.update(cx, |this, cx| {
                this.accounts.jellyfin_quick.shown = Shown::Idle;
                let moved = cx.global::<ResonateApp>().online.jellyfin.trim() != asked_at;
                if !moved {
                    this.quick_connected(answered, cx);
                }
                cx.notify();
            });
            let _ = told;
        });
    }

    pub(super) fn stop_quick_connect(&mut self, cx: &mut Context<Self>) {
        self.accounts
            .jellyfin_quick
            .stopped
            .store(true, Ordering::SeqCst);
        self.accounts.jellyfin_quick.shown = Shown::Idle;
        self.accounts.jellyfin_quick.task = Task::ready(());
        cx.notify();
    }

    fn quick_connected(&mut self, answered: JellyfinSignIn, cx: &mut Context<Self>) {
        let said = match answered {
            JellyfinSignIn::SignedIn(session) => {
                let said = words::settings_jellyfin::signed_in_as(session.name.as_str());
                self.keep_the_jellyfin_session(session, cx);
                Notice::Done(said)
            }
            JellyfinSignIn::Lapsed => Notice::Noted(words::settings_jellyfin::lapsed().to_owned()),
            JellyfinSignIn::Denied => {
                Notice::Trouble(words::settings_jellyfin::denied().to_owned())
            }
            JellyfinSignIn::Failed => {
                Notice::Trouble(words::settings_jellyfin::unreached().to_owned())
            }
            JellyfinSignIn::Stopped => return,
        };
        self.report(said, cx);
    }

    fn keep_the_jellyfin_session(&mut self, session: JellyfinSession, cx: &mut Context<Self>) {
        let name = session.name.clone();
        self.settings_fields.read(cx).jellyfin[1]
            .clone()
            .clone()
            .update(cx, |field, cx| field.hold(name.clone(), cx));
        self.settings_fields.read(cx).jellyfin[2]
            .clone()
            .clone()
            .update(cx, |field, cx| field.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            let online = &mut global.online;
            online.jellyfin_user.clone_from(&name);
            online.jellyfin_password.clear();
            online.jellyfin_token.clone_from(&session.token);
            online.jellyfin_user_id.clone_from(&session.user_id);
            online.jellyfin_device.clone_from(&session.device);
        });
        for setting in [
            Setting::JellyfinToken(session.token),
            Setting::JellyfinUserId(session.user_id),
            Setting::JellyfinDevice(session.device),
            Setting::JellyfinUser(name),
            Setting::JellyfinPassword(String::new()),
        ] {
            self.store(&setting, cx);
        }
        self.the_sources_moved(cx);
    }

    pub(super) fn let_go_of_the_jellyfin_session(&mut self, cx: &mut Context<Self>) {
        let online = &cx.global::<ResonateApp>().online;
        let held = [
            &online.jellyfin_token,
            &online.jellyfin_user_id,
            &online.jellyfin_device,
        ]
        .iter()
        .any(|held| !held.is_empty());
        if !held {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| {
            let online = &mut global.online;
            online.jellyfin_token.clear();
            online.jellyfin_user_id.clear();
            online.jellyfin_device.clear();
        });
        for setting in [
            Setting::JellyfinToken(String::new()),
            Setting::JellyfinUserId(String::new()),
            Setting::JellyfinDevice(String::new()),
        ] {
            self.store(&setting, cx);
        }
    }

    pub(super) fn quick_connect_row(&self, cx: &mut Context<Self>) -> Div {
        let app = cx.global::<ResonateApp>();
        if app.signs_in_to_jellyfin.is_none() || !offers_quick_connect(&app.online.jellyfin) {
            return div();
        }
        let reachable = app.online.enabled;
        match self.accounts.jellyfin_quick.shown.clone() {
            Shown::Idle => kit::action_row().child(action(
                "jellyfin-quick-connect",
                words::settings_jellyfin::sign_in_with_quick_connect(),
                Icon::Link,
                !reachable,
                |this, _, cx| this.sign_in_with_quick_connect(cx),
                self,
                cx,
            )),
            Shown::Asking => kit::action_row().child(action(
                "jellyfin-quick-connect",
                words::settings_jellyfin::asking_the_server(),
                Icon::Link,
                true,
                |_, _, _| {},
                self,
                cx,
            )),
            Shown::Waiting(code) => div()
                .flex()
                .flex_col()
                .gap_2()
                .child(kit::field(
                    words::settings_jellyfin::code(),
                    div()
                        .debug_selector(|| "jellyfin-quick-connect-code".to_owned())
                        .child(kit::figure(code)),
                ))
                .child(kit::action_row().child(action(
                    "jellyfin-quick-connect-cancel",
                    words::settings_jellyfin::cancel(),
                    Icon::Close,
                    false,
                    |this, _, cx| this.stop_quick_connect(cx),
                    self,
                    cx,
                )))
                .child(note(words::settings_jellyfin::approve_it())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_connect_is_offered_for_a_jellyfin_address_and_not_for_emby_or_none() {
        assert!(offers_quick_connect("http://music.local:8096"));
        assert!(!offers_quick_connect("https://music.local/EMBY/"));
        assert!(!offers_quick_connect("  "));
    }
}
