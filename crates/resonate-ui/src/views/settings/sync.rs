use gpui::{Context, Div, Window, div, prelude::*, px, rgb};
use resonate_core::SyncedSongs;

use crate::{
    Notice, ResonateApp, Setting, SyncStanding, Synchronised, theme,
    views::{
        kit,
        root::RootView,
        settings::{note, switch_row},
    },
};

pub(crate) const SYNC_SERVER_PLACEHOLDER: &str =
    "resonate.tecnio.dev, or your own server's host:port";
pub(crate) const SYNC_DEVICE_PLACEHOLDER: &str = "What your other devices call this one";
pub(crate) const SYNC_CODE_PLACEHOLDER: &str = "Another device's pairing code, then press enter";

const SYNCING_NOTE: &str = "A change here is taken up when Resonate next starts. Joining another \
                            device's code leaves the devices this one synchronised with; the \
                            server never learns the code, only a number made from it.";

const SONGS_NOTE: &str = "A song arrives in the music folder and is read into the library like \
                          one dropped on the window. Any playlist's menu fetches its missing \
                          songs from a device on request, whatever is chosen here.";

const NOT_A_CODE: &str = "That is not a pairing code: it is 35 letters and digits, as another \
                          device's Device sync group shows it";

impl RootView {
    pub(super) fn syncing_group(&mut self, cx: &mut Context<Self>) -> Div {
        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown());
        let on = synchronised.enabled;
        let under = match (on, shown.as_ref().map(|shown| shown.standing)) {
            (false, _) => "Off, nothing leaves this device",
            (true, None) => "On from the next start",
            (true, Some(SyncStanding::Connecting)) => "Reaching the sync server",
            (true, Some(SyncStanding::Connected)) => "In step with your other devices",
            (true, Some(SyncStanding::Unreachable)) => {
                "The sync server cannot be reached; changes wait here until it can"
            }
            (true, Some(SyncStanding::Refused)) => "The sync server refused this device",
        };

        let mut body = kit::section_body().child(self.in_the_ring(
            "sync-on",
            switch_row("Synchronise this device", under, on, "sync-on"),
            move |this, _, cx| this.set_sync(!on, cx),
            cx,
        ));
        if let Some(shown) = shown.filter(|shown| !shown.code.is_empty()) {
            body = body.child(kit::field(
                "Pairing code",
                div()
                    .id("sync-code-shown")
                    .text_size(px(theme::text_lg()))
                    .text_color(rgb(theme::text()))
                    .child(shown.code.clone()),
            ));
            let others = shown
                .devices
                .iter()
                .filter(|device| !device.this)
                .map(|device| {
                    format!(
                        "{} ({})",
                        device.name,
                        if device.online { "online" } else { "away" }
                    )
                })
                .collect::<Vec<_>>();
            body = body.child(note(if others.is_empty() {
                "No other device shares this code yet: type it into another device's Device \
                 sync group"
                    .to_owned()
            } else {
                format!("Synchronised with {}", others.join(", "))
            }));
            if shown.moving > 0 {
                body = body.child(note(format!(
                    "{} songs on their way from another device",
                    shown.moving
                )));
            }
        }
        body.child(kit::field(
            "Join with a code",
            self.key_field(
                "sync-code",
                &self.sync_code,
                |this, window, cx| this.leave_sync_field(window, cx),
                cx,
            ),
        ))
        .child(kit::field(
            "Device name",
            self.key_field(
                "sync-device",
                &self.sync_device,
                |this, window, cx| this.leave_sync_field(window, cx),
                cx,
            ),
        ))
        .child(kit::field(
            "Server",
            self.key_field(
                "sync-server",
                &self.sync_server,
                |this, window, cx| this.leave_sync_field(window, cx),
                cx,
            ),
        ))
        .child(note(SYNCING_NOTE))
    }

    pub(super) fn synced_songs_group(&mut self, cx: &mut Context<Self>) -> Div {
        let songs = cx.global::<ResonateApp>().synchronised.songs;

        kit::section_body()
            .child(self.in_the_ring(
                "sync-favourites",
                switch_row(
                    "Fetch favourites this device lacks",
                    "A song favourited on another device is sent here while both are online",
                    songs.favourites,
                    "sync-favourites",
                ),
                move |this, _, cx| {
                    this.follow_songs(
                        SyncedSongs {
                            favourites: !songs.favourites,
                            ..songs
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(self.in_the_ring(
                "sync-playlists",
                switch_row(
                    "Fetch the songs of playlists kept here",
                    "A playlist whose menu says Keep its songs here is filled from another device",
                    songs.playlists,
                    "sync-playlists",
                ),
                move |this, _, cx| {
                    this.follow_songs(
                        SyncedSongs {
                            playlists: !songs.playlists,
                            ..songs
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(self.in_the_ring(
                "sync-everything",
                switch_row(
                    "Fetch everything",
                    "Every song another device holds and this one lacks is sent here",
                    songs.everything,
                    "sync-everything",
                ),
                move |this, _, cx| {
                    this.follow_songs(
                        SyncedSongs {
                            everything: !songs.everything,
                            ..songs
                        },
                        cx,
                    );
                },
                cx,
            ))
            .child(note(SONGS_NOTE))
    }

    fn synchronise(
        &mut self,
        change: impl FnOnce(&mut Synchronised),
        setting: &Setting,
        cx: &mut Context<Self>,
    ) {
        cx.update_global::<ResonateApp, _>(|global, _| change(&mut global.synchronised));
        self.store(setting, cx);
        cx.notify();
    }

    pub(crate) fn set_sync(&mut self, on: bool, cx: &mut Context<Self>) {
        self.synchronise(
            |synchronised| synchronised.enabled = on,
            &Setting::Sync(on),
            cx,
        );
    }

    pub(crate) fn follow_songs(&mut self, songs: SyncedSongs, cx: &mut Context<Self>) {
        self.synchronise(
            |synchronised| synchronised.songs = songs,
            &Setting::SyncSongs(songs),
            cx,
        );
    }

    pub(crate) fn sync_server_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.sync_server.read(cx).text().trim().to_owned();
        self.sync_server
            .update(cx, |field, cx| field.hold(typed.clone(), cx));
        let said = if typed.is_empty() {
            "Devices sync through resonate.tecnio.dev from the next start"
        } else {
            "Devices sync through that server from the next start"
        };
        self.synchronise(
            |synchronised| synchronised.server.clone_from(&typed),
            &Setting::SyncServer(typed.clone()),
            cx,
        );
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus);
    }

    pub(crate) fn sync_device_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.sync_device.read(cx).text().trim().to_owned();
        self.sync_device
            .update(cx, |field, cx| field.hold(typed.clone(), cx));
        self.synchronise(
            |synchronised| synchronised.device.clone_from(&typed),
            &Setting::SyncDevice(typed.clone()),
            cx,
        );
        self.report(
            Notice::Done("Your other devices learn the new name from the next start".to_owned()),
            cx,
        );
        window.focus(&self.focus);
    }

    pub(crate) fn sync_code_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.sync_code.read(cx).text().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let reads = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .is_none_or(|syncing| syncing.reads_as_a_code(&typed));
        if !reads {
            self.report(Notice::Trouble(NOT_A_CODE.to_owned()), cx);
            return;
        }
        self.sync_code
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.store(&Setting::SyncKey(typed), cx);
        self.report(
            Notice::Done("This device joins those devices from the next start".to_owned()),
            cx,
        );
        window.focus(&self.focus);
    }

    pub(crate) fn leave_sync_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = [&self.sync_code, &self.sync_device, &self.sync_server]
            .iter()
            .any(|field| field.read(cx).is_focused(window));
        if !focused {
            return;
        }
        window.focus(&self.focus);
        cx.notify();
    }

    pub(crate) fn put_syncing_back(&mut self, cx: &mut Context<Self>) {
        let built = Synchronised::default();
        self.sync_server
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.sync_device
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.set_sync(built.enabled, cx);
        self.synchronise(
            |synchronised| {
                synchronised.server = built.server;
                synchronised.device = built.device;
            },
            &Setting::SyncServer(String::new()),
            cx,
        );
        self.store(&Setting::SyncDevice(String::new()), cx);
    }
}
