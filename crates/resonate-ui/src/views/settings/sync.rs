use gpui::{Context, Div, SharedString, Stateful, Window, div, prelude::*, px, rgb};
use resonate_core::SyncedSongs;

use crate::{
    Notice, ResonateApp, Setting, SyncStanding, SyncedDevice, Synchronised, clipboard,
    icons::Icon,
    theme,
    views::{
        kit::{self, Tone},
        root::RootView,
        settings::{note, reading, switch_row},
    },
};

pub(crate) const SYNC_SERVER_PLACEHOLDER: &str =
    "Blank for Resonate's server, or your own server's host:port";
pub(crate) const SYNC_DEVICE_PLACEHOLDER: &str = "What your other devices call this one";
pub(crate) const SYNC_CODE_PLACEHOLDER: &str = "Another device's pairing code, then press enter";

const SYNCING_NOTE: &str = "A change here is taken up when Resonate next starts. Joining another \
                            device's code leaves the devices this one synchronised with; a \
                            server never learns the code, only a number made from it.";

const NEARBY_ONLY_UNDER: &str = "No server: devices sync when they are on the same network, and \
                                 nothing leaves it";

const THROUGH_A_SERVER_UNDER: &str = "Devices sync through a server wherever they are, and \
                                      straight to each other on the same network";

const SONGS_NOTE: &str = "A song arrives in the music folder and is read into the library like \
                          one dropped on the window. Any playlist's menu fetches its missing \
                          songs from a device on request, whatever is chosen here.";

const COPY_CODE_HINT: &str = "Put this device's pairing code on the clipboard";

const FORCE_HINT: &str = "Reach your other devices again now, and ask them for any song this \
                          device lacks";

const CODE_COPIED: &str = "The pairing code is on the clipboard";

const FORCED: &str = "Reaching your other devices again";

const NOT_SYNCING: &str = "Sync is off; turn on Synchronise this device and restart first";

const REGENERATE_HINT: &str = "Make a new pairing code at the next start, so a code someone else saw \
                               reaches nothing; your other devices then join the new one";

const ARMED_TO_REGENERATE: &str = "Press again to make a new code: every device using this one stops \
                                   syncing with this device until the new code is typed into it";

const REGENERATED: &str = "A new pairing code is made when Resonate next starts; type it into your \
                           other devices then";

const FORGET_HINT: &str = "Take this device off every device's list; one that syncs again comes \
                           back";

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
        let nearby_only = synchronised.nearby_only;
        let nearby = shown.as_ref().map_or(0, |shown| {
            shown
                .devices
                .iter()
                .filter(|device| device.nearby && !device.this)
                .count()
        });
        let under = match (on, shown.as_ref().map(|shown| shown.standing), nearby) {
            (false, _, _) => "Off, nothing leaves this device",
            (true, None, _) => "On from the next start",
            (true, Some(SyncStanding::Connecting), 0) => "Reaching the sync server",
            (true, Some(SyncStanding::Connecting), _) => {
                "Reaching the sync server; in step with the devices on this network"
            }
            (true, Some(SyncStanding::Connected), _) => "In step with your other devices",
            (true, Some(SyncStanding::Unreachable), 0) => {
                "The sync server cannot be reached; changes wait here until it can"
            }
            (true, Some(SyncStanding::Unreachable), _) => {
                "The sync server cannot be reached; in step with the devices on this network"
            }
            (true, Some(SyncStanding::Refused), _) => "The sync server refused this device",
            (true, Some(SyncStanding::Nearby), 0) => {
                "Looking for your devices on this network; no server is used"
            }
            (true, Some(SyncStanding::Nearby), _) => {
                "In step with the devices on this network; no server is used"
            }
        };

        let mut body = kit::section_body()
            .child(self.in_the_ring(
                "sync-on",
                switch_row("Synchronise this device", under, on, "sync-on"),
                move |this, _, cx| this.set_sync(!on, cx),
                cx,
            ))
            .child(self.in_the_ring(
                "sync-nearby-only",
                switch_row(
                    "Only this network",
                    if nearby_only {
                        NEARBY_ONLY_UNDER
                    } else {
                        THROUGH_A_SERVER_UNDER
                    },
                    nearby_only,
                    "sync-nearby-only",
                ),
                move |this, _, cx| this.set_sync_nearby_only(!nearby_only, cx),
                cx,
            ));
        if let Some(shown) = shown.filter(|shown| !shown.code.is_empty()) {
            let code = shown.code.clone();
            body = body.child(kit::field(
                "Pairing code",
                div()
                    .id("sync-code-shown")
                    .text_size(px(theme::text_lg()))
                    .text_color(rgb(theme::text()))
                    .child(shown.code.clone()),
            ));
            body = body.child(
                kit::actions()
                    .child(self.in_the_ring(
                        "sync-copy-code",
                        kit::button(
                            "sync-copy-code",
                            Some(Icon::Export),
                            "Copy pairing code",
                            COPY_CODE_HINT,
                            Tone::Outlined,
                        ),
                        move |this, _, cx| {
                            clipboard::copy(code.clone(), cx);
                            this.report(Notice::Done(CODE_COPIED.to_owned()), cx);
                        },
                        cx,
                    ))
                    .child(self.in_the_ring(
                        "sync-force",
                        kit::button(
                            "sync-force",
                            Some(Icon::Redo),
                            "Sync now",
                            FORCE_HINT,
                            Tone::Outlined,
                        ),
                        move |this, _, cx| this.force_sync(cx),
                        cx,
                    ))
                    .child(self.regenerate_button(cx)),
            );
            if self.regenerating_the_code {
                body = body.child(note(ARMED_TO_REGENERATE));
            } else if self.code_regenerated {
                body = body.child(note(REGENERATED));
            }
            let mut others = shown
                .devices
                .iter()
                .filter(|device| !device.this)
                .peekable();
            if others.peek().is_none() {
                body = body.child(note(
                    "No other device shares this code yet: type it into another device's Device \
                     sync group",
                ));
            }
            for device in others {
                body = body.child(self.device_row(device, cx));
            }
            if shown.moving > 0 {
                body = body.child(note(format!(
                    "{} songs on their way from another device",
                    shown.moving
                )));
            }
        }
        body = body
            .child(kit::field(
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
            ));
        if !nearby_only {
            body = body.child(kit::field(
                "Server",
                self.key_field(
                    "sync-server",
                    &self.sync_server,
                    |this, window, cx| this.leave_sync_field(window, cx),
                    cx,
                ),
            ));
        }
        body.child(note(SYNCING_NOTE))
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

    fn regenerate_button(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let armed = self.regenerating_the_code;
        self.in_the_ring(
            "sync-regenerate",
            kit::button(
                "sync-regenerate",
                Some(Icon::Undo),
                if armed {
                    "Press again to regenerate"
                } else {
                    "Regenerate pairing code"
                },
                REGENERATE_HINT,
                if armed {
                    Tone::Destructive
                } else {
                    Tone::Outlined
                },
            ),
            move |this, _, cx| {
                if armed {
                    this.regenerate_the_code(cx);
                } else {
                    this.regenerating_the_code = true;
                    cx.notify();
                }
            },
            cx,
        )
    }

    fn device_row(&mut self, device: &SyncedDevice, cx: &mut Context<Self>) -> Div {
        let standing = match (device.nearby, device.online) {
            (true, _) => "On this network",
            (false, true) => "Online",
            (false, false) => "Away",
        };
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(reading(device.name.clone(), standing));
        if device.online {
            return row;
        }
        let id = device.id.clone();
        let name = device.name.clone();
        let control = format!("sync-forget-{}", device.id);
        row.child(self.in_the_ring_at(
            control.clone(),
            kit::button(
                SharedString::from(control),
                Some(Icon::Delete),
                "Remove",
                FORGET_HINT,
                Tone::Outlined,
            ),
            move |this, _, cx| this.forget_sync_device(&id, &name, cx),
            cx,
        ))
    }

    fn regenerate_the_code(&mut self, cx: &mut Context<Self>) {
        self.regenerating_the_code = false;
        self.code_regenerated = true;
        self.store(&Setting::SyncKey(String::new()), cx);
        self.report(Notice::Done(REGENERATED.to_owned()), cx);
        cx.notify();
    }

    fn forget_sync_device(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            self.report(Notice::Trouble(NOT_SYNCING.to_owned()), cx);
            return;
        };
        let notice = if syncing.forget(id) {
            Notice::Done(format!("{name} is off the list of your devices"))
        } else {
            Notice::Trouble(format!("{name} could not be taken off the list"))
        };
        self.report(notice, cx);
        cx.notify();
    }

    pub(crate) fn set_sync(&mut self, on: bool, cx: &mut Context<Self>) {
        self.synchronise(
            |synchronised| synchronised.enabled = on,
            &Setting::Sync(on),
            cx,
        );
    }

    pub(crate) fn set_sync_nearby_only(&mut self, nearby_only: bool, cx: &mut Context<Self>) {
        self.synchronise(
            |synchronised| synchronised.nearby_only = nearby_only,
            &Setting::SyncNearbyOnly(nearby_only),
            cx,
        );
        let said = if nearby_only {
            "Devices sync only on this network from the next start"
        } else {
            "Devices sync through the server again from the next start"
        };
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn force_sync(&mut self, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            self.report(Notice::Trouble(NOT_SYNCING.to_owned()), cx);
            return;
        };
        syncing.force();
        self.report(Notice::Done(FORCED.to_owned()), cx);
        cx.notify();
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
            "Devices sync through Resonate's server from the next start"
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
            .reads_as_a_code
            .is_none_or(|reads| reads(&typed));
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
        self.store(&Setting::SyncNearbyOnly(built.nearby_only), cx);
        self.synchronise(
            |synchronised| {
                synchronised.nearby_only = built.nearby_only;
                synchronised.server = built.server;
                synchronised.device = built.device;
            },
            &Setting::SyncServer(String::new()),
            cx,
        );
        self.store(&Setting::SyncDevice(String::new()), cx);
    }
}
