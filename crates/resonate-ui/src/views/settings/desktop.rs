use std::sync::{Arc, atomic::Ordering};

use gpui::{Context, SharedString, Window, prelude::*};
use resonate_core::{AppId, Pictured, Presence, Shown};

use crate::{
    Notice, ResonateApp, Setting,
    views::{
        kit,
        root::RootView,
        settings::{Choice, Rows, switch_row},
    },
    words,
};

impl Choice for Shown {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Application => words::settings_desktop::discord_says_only_that_resonate_is(),
            Self::Track => words::settings_desktop::discord_shows_the_title_and_the(),
            Self::Album => words::settings_desktop::discord_shows_the_title_the_artist(),
        })
    }
}

impl Choice for Pictured {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Cover => words::settings_desktop::the_albums_cover_from_the_cover(),
            Self::Icon => words::settings_desktop::the_icon_named_below_whatever_is(),
            Self::Nothing => words::settings_desktop::no_picture_at_all(),
        })
    }
}

impl RootView {
    pub(super) fn notifications_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let tells = cx.global::<ResonateApp>().notify.load(Ordering::Acquire);

        kit::rows()
            .child(self.in_the_ring(
                "notify-on-a-track-change",
                switch_row(
                    words::settings_desktop::tell_the_desktop_when_a_track(),
                    words::settings_desktop::off_nothing_pops_up_when_the(),
                    tells,
                    "notify-on-a-track-change",
                ),
                move |this, _, cx| this.set_notify(!tells, cx),
                cx,
            ))
            .note(words::settings_desktop::notifications_note())
    }

    pub(crate) fn set_notify(&mut self, notify: bool, cx: &mut Context<Self>) {
        cx.global::<ResonateApp>()
            .notify
            .store(notify, Ordering::Release);
        self.store(&Setting::Notify(notify), cx);
        cx.notify();
    }

    pub(super) fn discord_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let presence = cx.global::<ResonateApp>().presence.clone();
        let on = presence.enabled;
        let under = match (on, presence.app.is_some()) {
            (false, _) => words::settings_desktop::off_nothing_reaches_discord(),
            (true, false) => words::settings_desktop::on_and_waiting_for_an_application(),
            (true, true) => words::settings_desktop::discord_is_told_what_is_playing(),
        };

        kit::rows()
            .child(self.in_the_ring(
                "discord-on",
                switch_row(
                    words::settings_desktop::show_what_is_playing_on_discord(),
                    under,
                    on,
                    "discord-on",
                ),
                move |this, _, cx| {
                    this.present(
                        |presence| presence.enabled = !on,
                        &Setting::Discord(!on),
                        cx,
                    );
                },
                cx,
            ))
            .child(kit::field(
                words::settings_desktop::application_id(),
                self.text_field(
                    "discord-app",
                    &self.typed.discord_app,
                    |this, window, cx| this.leave_discord_app(window, cx),
                    cx,
                ),
            ))
            .note(words::settings_desktop::discord_note())
    }

    pub(super) fn discord_shows_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let presence = cx.global::<ResonateApp>().presence.clone();
        let progress = presence.progress;
        let while_paused = presence.while_paused;

        kit::rows()
            .child(kit::field(
                words::settings_desktop::shows(),
                self.choices(
                    "discord-shows",
                    presence.shown,
                    cx,
                    |this, shown: Shown, cx| {
                        this.present(
                            |presence| presence.shown = shown,
                            &Setting::DiscordShows(shown),
                            cx,
                        );
                    },
                ),
            ))
            .child(kit::field(
                words::settings_desktop::picture(),
                self.choices(
                    "discord-art",
                    presence.pictured,
                    cx,
                    |this, pictured: Pictured, cx| {
                        this.present(
                            |presence| presence.pictured = pictured,
                            &Setting::DiscordArt(pictured),
                            cx,
                        );
                    },
                ),
            ))
            .child(kit::field(
                words::settings_desktop::icon(),
                self.text_field(
                    "discord-icon",
                    &self.typed.discord_icon,
                    |this, window, cx| this.leave_discord_icon(window, cx),
                    cx,
                ),
            ))
            .child(self.in_the_ring(
                "discord-progress",
                switch_row(
                    words::settings_desktop::show_how_far_into_the_track(),
                    words::settings_desktop::off_no_bar_and_no_time(),
                    progress,
                    "discord-progress",
                ),
                move |this, _, cx| {
                    this.present(
                        |presence| presence.progress = !progress,
                        &Setting::DiscordProgress(!progress),
                        cx,
                    );
                },
                cx,
            ))
            .child(self.in_the_ring(
                "discord-paused",
                switch_row(
                    words::settings_desktop::stay_while_paused(),
                    words::settings_desktop::off_the_status_clears_when_the(),
                    while_paused,
                    "discord-paused",
                ),
                move |this, _, cx| {
                    this.present(
                        |presence| presence.while_paused = !while_paused,
                        &Setting::DiscordPaused(!while_paused),
                        cx,
                    );
                },
                cx,
            ))
            .note(words::settings_desktop::discord_art_note())
    }

    pub(crate) fn discord_app_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.discord_app.read(cx).text().trim().to_owned();
        let app = AppId::parse(&typed);
        if app.is_none() && !typed.is_empty() {
            self.report(
                Notice::Trouble(words::settings_desktop::not_an_application().to_owned()),
                cx,
            );
            return;
        }
        let held = app.map(|app| app.to_string()).unwrap_or_default();
        self.typed
            .discord_app
            .update(cx, |field, cx| field.hold(held, cx));
        self.present(|presence| presence.app = app, &Setting::DiscordApp(app), cx);

        let said = if app.is_some() {
            words::settings_desktop::discord_is_told_under_that_application()
        } else {
            words::settings_desktop::discord_is_told_nothing_until_an()
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus, cx);
    }

    pub(crate) fn leave_discord_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed.discord_app.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn discord_icon_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.discord_icon.read(cx).text().trim().to_owned();
        let icon = resonate_core::Icon::parse(&typed);
        if icon.is_none() && !typed.is_empty() {
            self.report(
                Notice::Trouble(words::settings_desktop::not_an_icon().to_owned()),
                cx,
            );
            return;
        }
        self.typed
            .discord_icon
            .update(cx, |field, cx| field.hold(typed, cx));
        self.present(
            |presence| presence.icon.clone_from(&icon),
            &Setting::DiscordIcon(icon.clone()),
            cx,
        );

        let said = if icon.is_some() {
            words::settings_desktop::discord_draws_that_icon_where_no()
        } else {
            words::settings_desktop::discord_draws_no_icon()
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus, cx);
    }

    pub(crate) fn leave_discord_icon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.typed.discord_icon.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_discord_back(&mut self, cx: &mut Context<Self>) {
        self.typed
            .discord_app
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.presented(
            |presence| {
                presence.enabled = Presence::OFF.enabled;
                presence.app = Presence::OFF.app;
            },
            cx,
        );
    }

    pub(crate) fn put_discord_shows_back(&mut self, cx: &mut Context<Self>) {
        self.typed
            .discord_icon
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.presented(
            |presence| {
                let off = Presence::OFF;
                presence.shown = off.shown;
                presence.pictured = off.pictured;
                presence.icon = off.icon;
                presence.progress = off.progress;
                presence.while_paused = off.while_paused;
            },
            cx,
        );
    }

    fn present(
        &mut self,
        change: impl FnOnce(&mut Presence),
        setting: &Setting,
        cx: &mut Context<Self>,
    ) {
        self.presented(change, cx);
        self.store(setting, cx);
    }

    fn presented(&self, change: impl FnOnce(&mut Presence), cx: &mut Context<Self>) {
        let (presence, present) = cx.update_global::<ResonateApp, _>(|global, _| {
            change(&mut global.presence);
            (global.presence.clone(), Arc::clone(&global.present))
        });
        present.follow(&presence);
        cx.notify();
    }
}
