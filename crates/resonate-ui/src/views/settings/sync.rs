use std::sync::Arc;

use gpui::{
    Bounds, Context, Div, FontWeight, SharedString, Stateful, Window, canvas, div, fill, point,
    prelude::*, px, rgb, size,
};
use qrcode::{Color, EcLevel, QrCode};
use resonate_core::SyncedSongs;

use crate::{
    Notice, ResonateApp, Setting, SyncShown, SyncStanding, SyncedDevice, SyncedPlatform,
    Synchronised, clipboard,
    icons::{self, Icon},
    theme,
    views::{
        kit::{self, Tone},
        root::RootView,
        settings::{note, switch_row},
    },
};

pub(crate) const SYNC_SERVER_PLACEHOLDER: &str = "host or host:port of your own server";
pub(crate) const SYNC_DEVICE_PLACEHOLDER: &str = "What your other devices call this one";
pub(crate) const SYNC_CODE_PLACEHOLDER: &str = "Another device's pairing code, then press enter";

const SYNCING_NOTE: &str = "A change here is taken up when Resonate next starts.";

const PAIRING_NOTE: &str = "Scan the code with your phone's camera, or type it into Device sync \
                            on the other device. Anyone with it can join your devices, so keep \
                            it covered while others can see the screen.";

const JOINING_NOTE: &str = "Joining another device's code leaves the devices this one syncs with \
                            now; a server never learns the code, only a number made from it.";

const SONGS_NOTE: &str = "A song arrives in the music folder and is read into the library like \
                          one dropped on the window. Any playlist's menu fetches its missing \
                          songs from a device on request, whatever is chosen here.";

const COPY_CODE_HINT: &str = "Put this device's pairing code on the clipboard";

const SHOW_CODE_HINT: &str = "Show the pairing code and its QR code";

const HIDE_CODE_HINT: &str = "Cover the pairing code and its QR code again";

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

const NO_OTHER_DEVICE: &str = "No other device shares this code yet. Show the pairing code and \
                               scan it with your phone, or type it into another device.";

const NO_CODE_YET: &str = "The pairing code is made when this device first syncs: turn on \
                           Synchronise this device and restart";

const HOSTED_MEANS: &str = "Your devices meet through Resonate's server wherever they are, and \
                            directly on the same network. What travels is sealed with the \
                            pairing code, so the server cannot read it.";

const OWN_SERVER_MEANS: &str = "Your devices meet through a resonate-server you run, and \
                                directly on the same network.";

const NEARBY_MEANS: &str = "No server at all: your devices sync when they are on the same \
                            network, and nothing leaves it. This works with online features off.";

const COVERED_MARK: char = '•';
const STANDING_WASH: u8 = 0x24;
const DEVICE_WASH: u8 = 0x22;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Route {
    Hosted,
    Own,
    Nearby,
}

impl Route {
    const ALL: [Self; 3] = [Self::Hosted, Self::Own, Self::Nearby];

    const fn label(self) -> &'static str {
        match self {
            Self::Hosted => "Resonate's server",
            Self::Own => "Your own server",
            Self::Nearby => "This network only",
        }
    }

    const fn meaning(self) -> &'static str {
        match self {
            Self::Hosted => HOSTED_MEANS,
            Self::Own => OWN_SERVER_MEANS,
            Self::Nearby => NEARBY_MEANS,
        }
    }

    fn of(synchronised: &Synchronised, own_server_asked: bool) -> Self {
        if synchronised.nearby_only {
            Self::Nearby
        } else if own_server_asked || !synchronised.server.trim().is_empty() {
            Self::Own
        } else {
            Self::Hosted
        }
    }
}

#[derive(Clone, Copy)]
enum Mood {
    Fine,
    Busy,
    Trouble,
    Quiet,
}

impl Mood {
    fn colour(self) -> u32 {
        match self {
            Self::Fine => theme::done(),
            Self::Busy => theme::accent(),
            Self::Trouble => theme::failure(),
            Self::Quiet => theme::muted(),
        }
    }

    const fn icon(self) -> Icon {
        match self {
            Self::Fine => Icon::Check,
            Self::Trouble => Icon::Alert,
            Self::Busy | Self::Quiet => Icon::Sync,
        }
    }
}

struct Said {
    mood: Mood,
    headline: &'static str,
    under: String,
}

fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn standing_said(synchronised: &Synchronised, shown: Option<&SyncShown>) -> Said {
    let Some(shown) = shown.filter(|_| synchronised.enabled) else {
        return if synchronised.enabled {
            Said {
                mood: Mood::Quiet,
                headline: "On from the next start",
                under: "Restart Resonate to begin syncing this device".to_owned(),
            }
        } else {
            Said {
                mood: Mood::Quiet,
                headline: "Sync is off",
                under: "Nothing leaves this device".to_owned(),
            }
        };
    };
    let others = shown.devices.iter().filter(|device| !device.this);
    let nearby = others.clone().filter(|device| device.nearby).count();
    let online = others.filter(|device| device.online).count();
    let on_this_network = format!("{} on this network", counted(nearby, "device"));
    match shown.standing {
        SyncStanding::Connecting => Said {
            mood: Mood::Busy,
            headline: "Connecting",
            under: if nearby == 0 {
                "Reaching the sync server".to_owned()
            } else {
                format!("Reaching the sync server · {on_this_network}")
            },
        },
        SyncStanding::Connected => Said {
            mood: Mood::Fine,
            headline: "In step",
            under: if online == 0 {
                "No other device is online".to_owned()
            } else {
                format!("{} online", counted(online, "device"))
            },
        },
        SyncStanding::Unreachable => Said {
            mood: Mood::Trouble,
            headline: "The server is out of reach",
            under: if nearby == 0 {
                "Changes wait here until it can be reached".to_owned()
            } else {
                format!("In step with the {on_this_network} meanwhile")
            },
        },
        SyncStanding::Refused => Said {
            mood: Mood::Trouble,
            headline: "Refused",
            under: "The sync server refused this device".to_owned(),
        },
        SyncStanding::Nearby if nearby == 0 => Said {
            mood: Mood::Quiet,
            headline: "Looking on this network",
            under: "No other device is on this network yet".to_owned(),
        },
        SyncStanding::Nearby => Said {
            mood: Mood::Fine,
            headline: "In step on this network",
            under: on_this_network,
        },
    }
}

fn covered(code: &str) -> String {
    code.chars()
        .map(|glyph| if glyph == '-' { glyph } else { COVERED_MARK })
        .collect()
}

fn modules_of(link: &str) -> Option<(usize, Arc<[bool]>)> {
    let code = QrCode::with_error_correction_level(link.as_bytes(), EcLevel::M).ok()?;
    let dark = code
        .to_colors()
        .into_iter()
        .map(|colour| colour == Color::Dark)
        .collect::<Arc<[bool]>>();
    Some((code.width(), dark))
}

fn scanned(link: &str) -> Div {
    let modules = modules_of(link);
    div()
        .size_full()
        .p_4()
        .rounded_lg()
        .bg(rgb(theme::scanned_paper()))
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, (), window, _| {
                    let Some((width, dark)) = modules.as_ref() else {
                        return;
                    };
                    let across = *width as f32;
                    let wide = f32::from(bounds.size.width);
                    let tall = f32::from(bounds.size.height);
                    let module = (wide.min(tall) / across).floor().max(1.0);
                    let drawn = module * across;
                    let left = f32::from(bounds.origin.x) + (wide - drawn) / 2.0;
                    let top = f32::from(bounds.origin.y) + (tall - drawn) / 2.0;
                    for row in 0..*width {
                        let mut column = 0;
                        while column < *width {
                            if !dark[row * width + column] {
                                column += 1;
                                continue;
                            }
                            let start = column;
                            while column < *width && dark[row * width + column] {
                                column += 1;
                            }
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(
                                        px(left + start as f32 * module),
                                        px(top + row as f32 * module),
                                    ),
                                    size(px((column - start) as f32 * module), px(module)),
                                ),
                                rgb(theme::scanned_ink()),
                            ));
                        }
                    }
                },
            )
            .size_full(),
        )
}

fn device_mark(platform: SyncedPlatform, online: bool) -> Div {
    let icon = match platform {
        SyncedPlatform::Android | SyncedPlatform::Ios => Icon::Phone,
        SyncedPlatform::Linux | SyncedPlatform::Other => Icon::Computer,
    };
    let colour = if online {
        theme::accent()
    } else {
        theme::faint()
    };
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(theme::width(theme::device_mark()))
        .rounded_lg()
        .bg(theme::tinted(colour, DEVICE_WASH))
        .child(icons::icon(icon, theme::device_mark_icon(), colour))
}

const fn platform_name(platform: SyncedPlatform) -> Option<&'static str> {
    match platform {
        SyncedPlatform::Linux => Some("Linux"),
        SyncedPlatform::Android => Some("Android"),
        SyncedPlatform::Ios => Some("iOS"),
        SyncedPlatform::Other => None,
    }
}

impl RootView {
    pub(super) fn syncing_group(&mut self, cx: &mut Context<Self>) -> Div {
        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown());
        let on = synchronised.enabled;
        let said = standing_said(&synchronised, shown.as_ref());
        let colour = said.mood.colour();

        let mut standing = div()
            .flex()
            .items_center()
            .gap_4()
            .p_4()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(theme::width(theme::standing_mark()))
                    .rounded_full()
                    .bg(theme::tinted(colour, STANDING_WASH))
                    .child(icons::icon(
                        said.mood.icon(),
                        theme::standing_mark_icon(),
                        colour,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(theme::text_base()))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::text()))
                            .child(said.headline),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::muted()))
                            .child(said.under),
                    ),
            );
        if on && shown.is_some() {
            standing = standing.child(self.in_the_ring(
                "sync-force",
                kit::button(
                    "sync-force",
                    Some(Icon::Sync),
                    "Sync now",
                    FORCE_HINT,
                    Tone::Outlined,
                ),
                move |this, _, cx| this.force_sync(cx),
                cx,
            ));
        }

        let mut body = kit::section_body()
            .child(self.in_the_ring(
                "sync-on",
                switch_row(
                    "Synchronise this device",
                    "Playlists, favourites, history and what is playing follow you",
                    on,
                    "sync-on",
                ),
                move |this, _, cx| this.set_sync(!on, cx),
                cx,
            ))
            .child(standing);
        if let Some(moving) = shown
            .as_ref()
            .map(|shown| shown.moving)
            .filter(|moving| *moving > 0)
        {
            body = body.child(note(format!(
                "{} on their way from another device",
                counted(moving, "song")
            )));
        }
        body.child(note(SYNCING_NOTE))
    }

    pub(super) fn pairing_group(&mut self, cx: &mut Context<Self>) -> Div {
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown())
            .filter(|shown| !shown.code.is_empty());
        let revealed = self.pairing_shown;

        let mut body = kit::section_body();
        if let Some(shown) = shown {
            let tile = if revealed {
                div()
                    .id("sync-qr")
                    .flex_none()
                    .size(theme::width(theme::pairing_tile()))
                    .child(scanned(&shown.link))
            } else {
                div()
                    .id("sync-qr")
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .size(theme::width(theme::pairing_tile()))
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(rgb(theme::surface()))
                    .border_1()
                    .border_color(rgb(theme::border()))
                    .hover(|tile| tile.bg(rgb(theme::hover())))
                    .child(icons::icon(
                        Icon::Reveal,
                        theme::standing_mark_icon(),
                        theme::muted(),
                    ))
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::muted()))
                            .child("Click to show"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.show_the_pairing(true, cx)))
            };
            let code = if revealed {
                shown.code.clone()
            } else {
                covered(&shown.code)
            };
            let copying = shown.code.clone();
            let column = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(0.0))
                .gap_3()
                .child(kit::eyebrow("PAIRING CODE"))
                .child(
                    div()
                        .id("sync-code-shown")
                        .font_family(theme::mono_face())
                        .text_size(px(theme::text_lg()))
                        .text_color(rgb(theme::text()))
                        .child(code),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_2()
                        .child(self.in_the_ring(
                            "sync-show-code",
                            kit::button(
                                "sync-show-code",
                                Some(if revealed {
                                    Icon::Conceal
                                } else {
                                    Icon::Reveal
                                }),
                                if revealed { "Hide" } else { "Show" },
                                if revealed {
                                    HIDE_CODE_HINT
                                } else {
                                    SHOW_CODE_HINT
                                },
                                Tone::Outlined,
                            ),
                            move |this, _, cx| this.show_the_pairing(!revealed, cx),
                            cx,
                        ))
                        .child(self.in_the_ring(
                            "sync-copy-code",
                            kit::button(
                                "sync-copy-code",
                                Some(Icon::Export),
                                "Copy",
                                COPY_CODE_HINT,
                                Tone::Outlined,
                            ),
                            move |this, _, cx| {
                                clipboard::copy(copying.clone(), cx);
                                this.report(Notice::Done(CODE_COPIED.to_owned()), cx);
                            },
                            cx,
                        ))
                        .child(self.regenerate_button(cx)),
                );
            body = body
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_6()
                        .p_4()
                        .rounded_lg()
                        .bg(rgb(theme::raised()))
                        .border_1()
                        .border_color(rgb(theme::border()))
                        .child(tile)
                        .child(column),
                )
                .child(note(PAIRING_NOTE));
            if self.regenerating_the_code {
                body = body.child(note(ARMED_TO_REGENERATE));
            } else if self.code_regenerated {
                body = body.child(note(REGENERATED));
            }
        } else {
            body = body.child(note(NO_CODE_YET));
        }
        body.child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .pt_4()
                .border_t_1()
                .border_color(rgb(theme::border()))
                .child(kit::field(
                    "Join another device's code",
                    self.key_field(
                        "sync-code",
                        &self.sync_code,
                        |this, window, cx| this.leave_sync_field(window, cx),
                        cx,
                    ),
                ))
                .child(note(JOINING_NOTE)),
        )
    }

    pub(super) fn sync_devices_group(&mut self, cx: &mut Context<Self>) -> Div {
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown());
        let Some(shown) = shown else {
            return kit::section_body()
                .child(note("Your devices are listed here once this device syncs"));
        };
        let mut devices = shown.devices;
        devices.sort_by(|first, second| {
            second
                .this
                .cmp(&first.this)
                .then(second.online.cmp(&first.online))
                .then_with(|| first.name.to_lowercase().cmp(&second.name.to_lowercase()))
        });
        let mut body = kit::section_body().gap_2();
        for device in &devices {
            body = body.child(self.device_row(device, cx));
        }
        if devices.iter().all(|device| device.this) {
            body = body.child(note(NO_OTHER_DEVICE));
        }
        body
    }

    pub(super) fn sync_connection_group(&mut self, cx: &mut Context<Self>) -> Div {
        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let chosen = Route::of(&synchronised, self.own_server_asked);

        let mut routes = kit::segmented();
        for (index, route) in Route::ALL.into_iter().enumerate() {
            routes = routes.child(self.option_in_the_ring(
                kit::segment(("sync-route", index), route.label(), route == chosen),
                "sync-route",
                index,
                move |this, _, cx| this.take_the_route(route, cx),
                cx,
            ));
        }
        let mut body = kit::section_body().child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().flex().child(routes))
                .child(note(chosen.meaning())),
        );
        if chosen == Route::Own {
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
        body.child(kit::field(
            "Device name",
            self.key_field(
                "sync-device",
                &self.sync_device,
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

    fn show_the_pairing(&mut self, shown: bool, cx: &mut Context<Self>) {
        self.pairing_shown = shown;
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
                    "Press again to make a new code"
                } else {
                    "New code"
                },
                REGENERATE_HINT,
                if armed {
                    Tone::Destructive
                } else {
                    Tone::Ghost
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
        let state = match (device.this, device.nearby, device.online) {
            (true, _, _) => "This device",
            (false, true, _) => "On this network",
            (false, false, true) => "Online",
            (false, false, false) => "Away",
        };
        let under = platform_name(device.platform).map_or_else(
            || state.to_owned(),
            |platform| format!("{platform} · {state}"),
        );
        let row = div()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::border()))
            .child(device_mark(device.platform, device.online))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::text()))
                            .child(device.name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .child(under),
                    ),
            );
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
                Tone::Ghost,
            ),
            move |this, _, cx| this.forget_sync_device(&id, &name, cx),
            cx,
        ))
    }

    fn regenerate_the_code(&mut self, cx: &mut Context<Self>) {
        self.regenerating_the_code = false;
        self.code_regenerated = true;
        self.pairing_shown = false;
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

    fn take_the_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.own_server_asked = route == Route::Own;
        let nearby_only = route == Route::Nearby;
        if cx.global::<ResonateApp>().synchronised.nearby_only != nearby_only {
            self.set_sync_nearby_only(nearby_only, cx);
        }
        let given = !cx
            .global::<ResonateApp>()
            .synchronised
            .server
            .trim()
            .is_empty();
        if route == Route::Hosted && given {
            self.sync_server
                .update(cx, |field, cx| field.hold(String::new(), cx));
            self.synchronise(
                |synchronised| synchronised.server.clear(),
                &Setting::SyncServer(String::new()),
                cx,
            );
            self.report(
                Notice::Done(
                    "Devices sync through Resonate's server from the next start".to_owned(),
                ),
                cx,
            );
        }
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
            "Devices sync through a server again from the next start"
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
        self.own_server_asked = !typed.is_empty();
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

    pub(crate) fn put_the_connection_back(&mut self, cx: &mut Context<Self>) {
        let built = Synchronised::default();
        self.own_server_asked = false;
        self.sync_server
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.sync_device
            .update(cx, |field, cx| field.hold(String::new(), cx));
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
