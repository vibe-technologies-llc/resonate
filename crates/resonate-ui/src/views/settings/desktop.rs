use std::sync::{Arc, atomic::Ordering};

use gpui::{Context, Div, SharedString, Window, prelude::*};
use resonate_core::{AppId, Pictured, Presence, Shown};

use crate::{
    Notice, ResonateApp, Setting,
    views::{
        kit,
        root::RootView,
        settings::{Choice, note, switch_row},
    },
};

const NOTIFICATIONS_NOTE: &str = "A popup is never raised while this window is the one in front, \
                                  and a session running no notification server sees none either \
                                  way. What the desktop draws on one — a cover, the transport \
                                  buttons, how long it stays — is the server's own to decide.";

const DISCORD_NOTE: &str = "Register an application in Discord's developer portal, name it what \
                            the status should say, and paste its application id here. Discord \
                            has to be running on this machine: it is told over its own local \
                            socket and nothing else is asked.";

const DISCORD_ART_NOTE: &str = "A cover is Discord fetching the album's front from the Cover Art \
                                Archive by the release's MusicBrainz id, so only a track whose \
                                tags or lookup named its release has one. Where none is known the \
                                icon stands in — an asset key uploaded to your application, or an \
                                image address. No picture on this machine is sent anywhere.";

const NOT_AN_APPLICATION: &str =
    "That is not a Discord application id; it is a run of 17 to 20 digits";

const NOT_AN_ICON: &str = "An icon is one asset key or one image address, with no spaces";

impl Choice for Shown {
    const ALL: &'static [Self] = &Self::ALL;

    fn label(self) -> &'static str {
        Self::label(self)
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Application => "Discord says only that Resonate is running, and nothing of what.",
            Self::Track => "Discord shows the title and the artist of what is playing.",
            Self::Album => "Discord shows the title, the artist and the album it is on.",
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
            Self::Cover => {
                "The album's cover from the Cover Art Archive, which Discord fetches itself; \
                 nothing on this machine is sent. An album the archive holds none for shows the \
                 icon below."
            }
            Self::Icon => "The icon named below, whatever is playing.",
            Self::Nothing => "No picture at all.",
        })
    }
}

impl RootView {
    pub(super) fn notifications_group(&mut self, cx: &mut Context<Self>) -> Div {
        let tells = cx.global::<ResonateApp>().notify.load(Ordering::Acquire);

        kit::section_body()
            .child(self.in_the_ring(
                "notify-on-a-track-change",
                switch_row(
                    "Tell the desktop when a track starts",
                    "Off, nothing pops up when the track changes",
                    tells,
                    "notify-on-a-track-change",
                ),
                move |this, _, cx| this.set_notify(!tells, cx),
                cx,
            ))
            .child(note(NOTIFICATIONS_NOTE))
    }

    pub(crate) fn set_notify(&self, notify: bool, cx: &mut Context<Self>) {
        cx.global::<ResonateApp>()
            .notify
            .store(notify, Ordering::Release);
        self.store(&Setting::Notify(notify), cx);
        cx.notify();
    }

    pub(super) fn discord_group(&mut self, cx: &mut Context<Self>) -> Div {
        let presence = cx.global::<ResonateApp>().presence.clone();
        let on = presence.enabled;
        let under = match (on, presence.app.is_some()) {
            (false, _) => "Off, nothing reaches Discord",
            (true, false) => "On, and waiting for an application id",
            (true, true) => "Discord is told what is playing",
        };

        kit::section_body()
            .child(self.in_the_ring(
                "discord-on",
                switch_row("Show what is playing on Discord", under, on, "discord-on"),
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
                "Application id",
                self.key_field(
                    "discord-app",
                    &self.discord_app,
                    |this, window, cx| this.leave_discord_app(window, cx),
                    cx,
                ),
            ))
            .child(note(DISCORD_NOTE))
    }

    pub(super) fn discord_shows_group(&mut self, cx: &mut Context<Self>) -> Div {
        let presence = cx.global::<ResonateApp>().presence.clone();
        let progress = presence.progress;
        let while_paused = presence.while_paused;

        kit::section_body()
            .child(kit::field(
                "Shows",
                self.choices(
                    "discord-shows",
                    Some(presence.shown),
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
                "Picture",
                self.choices(
                    "discord-art",
                    Some(presence.pictured),
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
                "Icon",
                self.key_field(
                    "discord-icon",
                    &self.discord_icon,
                    |this, window, cx| this.leave_discord_icon(window, cx),
                    cx,
                ),
            ))
            .child(self.in_the_ring(
                "discord-progress",
                switch_row(
                    "Show how far into the track",
                    "Off, no bar and no time is drawn",
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
                    "Stay while paused",
                    "Off, the status clears when the music pauses",
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
            .child(note(DISCORD_ART_NOTE))
    }

    pub(crate) fn discord_app_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.discord_app.read(cx).text().trim().to_owned();
        let app = AppId::parse(&typed);
        if app.is_none() && !typed.is_empty() {
            self.report(Notice::Trouble(NOT_AN_APPLICATION.to_owned()), cx);
            return;
        }
        let held = app.map(|app| app.to_string()).unwrap_or_default();
        self.discord_app
            .update(cx, |field, cx| field.hold(held, cx));
        self.present(|presence| presence.app = app, &Setting::DiscordApp(app), cx);

        let said = if app.is_some() {
            "Discord is told under that application"
        } else {
            "Discord is told nothing until an application id is given"
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
    }

    pub(crate) fn leave_discord_app(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.discord_app.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn discord_icon_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.discord_icon.read(cx).text().trim().to_owned();
        let icon = resonate_core::Icon::parse(&typed);
        if icon.is_none() && !typed.is_empty() {
            self.report(Notice::Trouble(NOT_AN_ICON.to_owned()), cx);
            return;
        }
        self.discord_icon
            .update(cx, |field, cx| field.hold(typed, cx));
        self.present(
            |presence| presence.icon.clone_from(&icon),
            &Setting::DiscordIcon(icon.clone()),
            cx,
        );

        let said = if icon.is_some() {
            "Discord draws that icon where no cover is known"
        } else {
            "Discord draws no icon"
        };
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
    }

    pub(crate) fn leave_discord_icon(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.discord_icon.read(cx).is_focused(window) {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn put_discord_back(&self, cx: &mut Context<Self>) {
        self.discord_app
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.presented(
            |presence| {
                presence.enabled = Presence::OFF.enabled;
                presence.app = Presence::OFF.app;
            },
            cx,
        );
    }

    pub(crate) fn put_discord_shows_back(&self, cx: &mut Context<Self>) {
        self.discord_icon
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
        &self,
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
