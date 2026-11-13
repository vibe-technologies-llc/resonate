use gpui::{Context, Task, Window, prelude::*};

use super::{RootView, token_notice};
use crate::{Notice, ResonateApp, Setting, words};

impl RootView {
    pub(super) fn contact_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_contact(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn contact_left(&mut self, cx: &mut Context<Self>) {
        if self.settings_fields.read(cx).contact.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.contact
        {
            self.take_the_contact(cx);
            cx.notify();
        }
    }

    pub(super) fn take_the_contact(&mut self, cx: &mut Context<Self>) {
        let given = self
            .settings_fields
            .read(cx)
            .contact
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.settings_fields
            .read(cx)
            .contact
            .clone()
            .update(cx, |contact, cx| contact.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = given.clone());

        let said = if given.is_empty() {
            words::root::contact_cleared()
        } else {
            words::root::contact_taken()
        };
        self.store(&Setting::Contact(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(super) fn acoustid_key_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_acoustid_key(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn acoustid_key_left(&mut self, cx: &mut Context<Self>) {
        if self
            .settings_fields
            .read(cx)
            .acoustid
            .read(cx)
            .text()
            .trim()
            != cx.global::<ResonateApp>().online.acoustid_key
        {
            self.take_the_acoustid_key(cx);
            cx.notify();
        }
    }

    pub(super) fn take_the_acoustid_key(&mut self, cx: &mut Context<Self>) {
        let given = self
            .settings_fields
            .read(cx)
            .acoustid
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.settings_fields
            .read(cx)
            .acoustid
            .clone()
            .update(cx, |key, cx| key.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.acoustid_key = given.clone());

        let said = if given.is_empty() {
            words::root::acoustid_key_cleared()
        } else {
            words::root::acoustid_key_taken()
        };
        self.store(&Setting::AcoustidKey(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn leave_acoustid_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .acoustid
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self
            .settings_fields
            .read(cx)
            .acoustid
            .read(cx)
            .text()
            .trim()
            != cx.global::<ResonateApp>().online.acoustid_key
        {
            self.acoustid_key_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_acoustid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.acoustid_key.to_owned();
        self.settings_fields
            .read(cx)
            .acoustid
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_acoustid_key(window, cx);
    }

    pub(super) fn replication_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_replication_token(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn replication_token_left(&mut self, cx: &mut Context<Self>) {
        if self
            .settings_fields
            .read(cx)
            .replication
            .read(cx)
            .text()
            .trim()
            != cx
                .global::<ResonateApp>()
                .online
                .musicbrainz_replication_token
        {
            self.take_the_replication_token(cx);
            cx.notify();
        }
    }

    pub(super) fn take_the_replication_token(&mut self, cx: &mut Context<Self>) {
        let given = self
            .settings_fields
            .read(cx)
            .replication
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.settings_fields
            .read(cx)
            .replication
            .clone()
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.musicbrainz_replication_token = given.clone();
        });

        let said = if given.is_empty() {
            words::root::replication_token_cleared()
        } else {
            words::root::replication_token_taken()
        };
        self.store(&Setting::MusicbrainzReplicationToken(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn leave_replication_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .replication
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self
            .settings_fields
            .read(cx)
            .replication
            .read(cx)
            .text()
            .trim()
            != cx
                .global::<ResonateApp>()
                .online
                .musicbrainz_replication_token
        {
            self.replication_token_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_replication_token(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let stored = cx
            .global::<ResonateApp>()
            .online
            .musicbrainz_replication_token
            .to_owned();
        self.settings_fields
            .read(cx)
            .replication
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_replication_token(window, cx);
    }

    pub(crate) fn clear_replication_token(&self, cx: &mut Context<Self>) {
        self.settings_fields
            .read(cx)
            .replication
            .clone()
            .update(cx, |token, cx| token.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.musicbrainz_replication_token = String::new();
        });
    }

    pub(super) fn audd_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_audd_token(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn audd_token_left(&mut self, cx: &mut Context<Self>) {
        if self.settings_fields.read(cx).audd.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.audd_token
        {
            self.take_the_audd_token(cx);
            cx.notify();
        }
    }

    pub(super) fn take_the_audd_token(&mut self, cx: &mut Context<Self>) {
        let given = self
            .settings_fields
            .read(cx)
            .audd
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.settings_fields
            .read(cx)
            .audd
            .clone()
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.audd_token = given.clone());

        let said = if given.is_empty() {
            words::root::audd_token_cleared()
        } else {
            words::root::audd_token_taken()
        };
        self.store(&Setting::AuddToken(given), cx);
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn leave_audd_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .audd
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self.settings_fields.read(cx).audd.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.audd_token
        {
            self.audd_token_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_audd(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.audd_token.to_owned();
        self.settings_fields
            .read(cx)
            .audd
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_audd_token(window, cx);
    }

    pub(super) fn listenbrainz_token_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.take_the_listenbrainz_token(cx);
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(super) fn listenbrainz_token_left(&mut self, cx: &mut Context<Self>) {
        if self
            .settings_fields
            .read(cx)
            .listenbrainz
            .read(cx)
            .text()
            .trim()
            != cx.global::<ResonateApp>().online.listenbrainz_token
        {
            self.take_the_listenbrainz_token(cx);
            cx.notify();
        }
    }

    pub(super) fn take_the_listenbrainz_token(&mut self, cx: &mut Context<Self>) {
        let given = self
            .settings_fields
            .read(cx)
            .listenbrainz
            .read(cx)
            .text()
            .trim()
            .to_owned();
        self.settings_fields
            .read(cx)
            .listenbrainz
            .clone()
            .update(cx, |token, cx| token.hold(given.clone(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.listenbrainz_token = given.clone();
        });

        let said = if given.is_empty() {
            words::root::listens_not_sent()
        } else {
            words::root::listens_sent()
        };
        self.store(&Setting::ListenbrainzToken(given.clone()), cx);
        self.report(Notice::Done(said.to_owned()), cx);
        self.check_the_listenbrainz_token(given, cx);
    }

    pub(super) fn check_the_listenbrainz_token(&mut self, token: String, cx: &mut Context<Self>) {
        let app = cx.global::<ResonateApp>();
        let Some(scrobblers) = app.scrobblers.clone().filter(|_| app.online.enabled) else {
            return;
        };
        if token.is_empty() {
            self.accounts._token_checked = Task::ready(());
            return;
        }

        self.accounts._token_checked = cx.spawn(async move |this, cx| {
            let held = cx
                .background_executor()
                .spawn(async move { scrobblers.under(token).token_held() })
                .await;
            let outcome = this.update(cx, |this, cx| this.report(token_notice(held), cx));
            let _ = outcome;
        });
    }

    pub(crate) fn leave_listenbrainz_token(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .listenbrainz
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self
            .settings_fields
            .read(cx)
            .listenbrainz
            .read(cx)
            .text()
            .trim()
            != cx.global::<ResonateApp>().online.listenbrainz_token
        {
            self.listenbrainz_token_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_listenbrainz(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx
            .global::<ResonateApp>()
            .online
            .listenbrainz_token
            .to_owned();
        self.settings_fields
            .read(cx)
            .listenbrainz
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_listenbrainz_token(window, cx);
    }

    pub(crate) fn clear_listenbrainz_token(&self, cx: &mut Context<Self>) {
        self.settings_fields
            .read(cx)
            .listenbrainz
            .clone()
            .update(cx, |token, cx| token.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.listenbrainz_token = String::new();
        });
    }

    pub(crate) fn clear_audd_token(&self, cx: &mut Context<Self>) {
        self.settings_fields
            .read(cx)
            .audd
            .clone()
            .update(cx, |token, cx| token.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.audd_token = String::new());
    }

    pub(crate) fn clear_acoustid_key(&self, cx: &mut Context<Self>) {
        self.settings_fields
            .read(cx)
            .acoustid
            .clone()
            .update(cx, |key, cx| key.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.acoustid_key = String::new());
    }

    pub(crate) fn leave_contact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .contact
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self.settings_fields.read(cx).contact.read(cx).text().trim()
            != cx.global::<ResonateApp>().online.contact
        {
            self.contact_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_contact(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().online.contact.to_owned();
        self.settings_fields
            .read(cx)
            .contact
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_contact(window, cx);
    }

    pub(crate) fn leave_typed_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .root
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn leave_organising(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self
            .settings_fields
            .read(cx)
            .organising
            .read(cx)
            .is_focused(window)
        {
            return;
        }
        if self
            .settings_fields
            .read(cx)
            .organising
            .read(cx)
            .text()
            .trim()
            != cx.global::<ResonateApp>().organise_as
        {
            self.layout_given(window, cx);
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_back_organising(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stored = cx.global::<ResonateApp>().organise_as.clone();
        self.settings_fields
            .read(cx)
            .organising
            .clone()
            .update(cx, |field, cx| field.hold(stored, cx));
        self.leave_organising(window, cx);
    }

    pub(crate) fn clear_contact(&self, cx: &mut Context<Self>) {
        self.settings_fields
            .read(cx)
            .contact
            .clone()
            .update(cx, |contact, cx| contact.hold(String::new(), cx));
        cx.update_global::<ResonateApp, _>(|global, _| global.online.contact = String::new());
    }
}
