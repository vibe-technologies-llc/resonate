use std::{sync::Arc, time::Duration};

use gpui::{
    AccessibleAction, Bounds, Context, Div, KeyContext, MouseButton, MouseDownEvent, Role,
    SharedString, Stateful, Window, canvas, div, fill, point, prelude::*, px, relative, rgb, size,
};
use qrcode::{Color, EcLevel, QrCode};
use resonate_core::{RemoteTrust, SyncedSongs, elapsed, remote_trust_asked, remote_trust_told};

use crate::{
    Notice, RemoteKey, RemoteRowShown, RemoteShown, RemoteZoneShown, ResonateApp, Setting,
    SyncShown, SyncStanding, SyncedDevice, SyncedPlatform, Synchronised,
    app::{CONTROL_CONTEXT, RAIL_CONTEXT, RailBack, RailOn},
    clipboard,
    icons::{self, Icon},
    theme,
    views::{
        focus,
        hint::Names,
        kit::{self, EndsInAnEllipsis, Found, Tone},
        root::RootView,
        settings::{Armed, Rows, note, switch_row},
        spoken::{self, Reading, Spoken},
    },
    words,
};

const DRIVEN_REDRAWN_EVERY: Duration = Duration::from_millis(500);

const VOLUME_STEP: u16 = 100;

const WHOLE_VOLUME: u16 = 1_000;

pub(crate) const DRIVEN_ROWS_AT_FIRST: usize = 12;

const DRIVEN_ROWS_MORE: usize = 50;

const DRIVEN_ROWS_BEFORE_THE_ONE_PLAYING: usize = 2;

const DRIVEN_SEEK_STEP: Duration = Duration::from_secs(5);

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

    fn label(self) -> &'static str {
        match self {
            Self::Hosted => words::settings_sync::resonates_server(),
            Self::Own => words::settings_sync::your_own_server(),
            Self::Nearby => words::settings_sync::this_network_only(),
        }
    }

    fn meaning(self) -> &'static str {
        match self {
            Self::Hosted => words::settings_sync::hosted_means(),
            Self::Own => words::settings_sync::own_server_means(),
            Self::Nearby => words::settings_sync::nearby_means(),
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

fn on_their_way(downloading: usize, waiting: usize) -> Option<String> {
    let total = downloading + waiting;
    if total == 0 {
        return None;
    }
    let mut parts = vec![words::settings_sync::songs_on_their_way(total)];
    if downloading > 0 {
        parts.push(words::settings_sync::downloading_count(downloading));
    }
    if waiting > 0 {
        parts.push(words::settings_sync::waiting_count(waiting));
    }
    Some(parts.join(" · "))
}

fn standing_said(synchronised: &Synchronised, shown: Option<&SyncShown>) -> Said {
    let Some(shown) = shown.filter(|_| synchronised.enabled) else {
        return if synchronised.enabled {
            Said {
                mood: Mood::Quiet,
                headline: words::settings_sync::not_in_this_build(),
                under: words::settings_sync::this_build_carries_no_device_sync().to_owned(),
            }
        } else {
            Said {
                mood: Mood::Quiet,
                headline: words::settings_sync::sync_is_off(),
                under: words::settings_sync::nothing_leaves_this_device().to_owned(),
            }
        };
    };
    let others = shown.devices.iter().filter(|device| !device.this);
    let nearby = others.clone().filter(|device| device.nearby).count();
    let online = others.filter(|device| device.online).count();
    let on_this_network = words::settings_sync::devices_on_this_network(nearby);
    match shown.standing {
        SyncStanding::Waiting => Said {
            mood: Mood::Busy,
            headline: words::settings_sync::starting(),
            under: words::settings_sync::this_device_begins_syncing_in_a().to_owned(),
        },
        SyncStanding::Connecting => Said {
            mood: Mood::Busy,
            headline: words::settings_sync::connecting(),
            under: if nearby == 0 {
                words::settings_sync::reaching_the_sync_server().to_owned()
            } else {
                words::settings_sync::reaching_the_server_beside(&on_this_network)
            },
        },
        SyncStanding::Connected => Said {
            mood: Mood::Fine,
            headline: words::settings_sync::in_step(),
            under: if online == 0 {
                words::settings_sync::no_other_device_is_online().to_owned()
            } else {
                words::settings_sync::devices_online(online)
            },
        },
        SyncStanding::Unreachable => Said {
            mood: Mood::Trouble,
            headline: words::settings_sync::the_server_is_out_of_reach(),
            under: if nearby == 0 {
                words::settings_sync::changes_wait_here_until_it_can().to_owned()
            } else {
                words::settings_sync::in_step_with_the_network_meanwhile(&on_this_network)
            },
        },
        SyncStanding::Refused => Said {
            mood: Mood::Trouble,
            headline: words::settings_sync::refused(),
            under: words::settings_sync::the_sync_server_refused_this_device().to_owned(),
        },
        SyncStanding::Nearby if nearby == 0 => Said {
            mood: Mood::Quiet,
            headline: words::settings_sync::looking_on_this_network(),
            under: words::settings_sync::no_other_device_is_on_this().to_owned(),
        },
        SyncStanding::Nearby => Said {
            mood: Mood::Fine,
            headline: words::settings_sync::in_step_on_this_network(),
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

pub(crate) fn scanned(link: &str) -> Div {
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
        SyncedPlatform::Linux
        | SyncedPlatform::Windows
        | SyncedPlatform::MacOs
        | SyncedPlatform::Other => Icon::Computer,
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
        SyncedPlatform::Windows => Some("Windows"),
        SyncedPlatform::MacOs => Some("macOS"),
        SyncedPlatform::Other => None,
    }
}

impl RootView {
    pub(super) fn syncing_group(&mut self, cx: &mut Context<Self>) -> Rows {
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
            .gap_3()
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
            .child(kit::named(said.headline, said.under));
        if on && shown.is_some() {
            standing = standing.child(self.in_the_ring(
                "sync-force",
                kit::button(
                    "sync-force",
                    Some(Icon::Sync),
                    words::settings_sync::sync_now(),
                    words::settings_sync::force_hint(),
                    Tone::Outlined,
                ),
                move |this, _, cx| this.force_sync(cx),
                cx,
            ));
        }

        let mut body = kit::rows()
            .child(self.in_the_ring(
                "sync-on",
                switch_row(
                    words::settings_sync::synchronise_this_device(),
                    words::settings_sync::playlists_favourites_history_and_what_is(),
                    on,
                    "sync-on",
                ),
                move |this, _, cx| this.set_sync(!on, cx),
                cx,
            ))
            .child(standing);
        if let Some(on_their_way) = shown
            .as_ref()
            .and_then(|shown| on_their_way(shown.downloading, shown.waiting))
        {
            body = body.child(note(on_their_way));
        }
        body.note(words::settings_sync::syncing_note())
    }

    pub(super) fn pairing_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown())
            .filter(|shown| !shown.code.is_empty());
        let revealed = self.settings_shown.pairing_shown;

        let mut body = kit::rows();
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
                            .child(words::settings_sync::click_to_show()),
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
                .child(kit::eyebrow(words::settings_sync::pairing_code()))
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
                                if revealed {
                                    words::settings_sync::hide()
                                } else {
                                    words::settings_sync::show()
                                },
                                if revealed {
                                    words::settings_sync::hide_code_hint()
                                } else {
                                    words::settings_sync::show_code_hint()
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
                                words::settings_sync::copy(),
                                words::settings_sync::copy_code_hint(),
                                Tone::Outlined,
                            ),
                            move |this, _, cx| {
                                clipboard::copy(copying.clone(), cx);
                                this.report(
                                    Notice::Done(words::settings_sync::code_copied().to_owned()),
                                    cx,
                                );
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
                        .py_1()
                        .child(tile)
                        .child(column),
                )
                .note(words::settings_sync::pairing_note());
            if self.is_armed(&Armed::RegenerateTheCode) {
                body = body.child(note(words::settings_sync::armed_to_regenerate()));
            } else if self.settings_shown.code_regenerated {
                body = body.child(note(words::settings_sync::regenerated()));
            }
        } else {
            body = body.child(note(words::settings_sync::no_code_yet()));
        }
        body.child(
            kit::field(
                words::settings_sync::join_another_devices_code(),
                self.text_field(
                    "sync-code",
                    &self.typed.sync_code,
                    |this, window, cx| this.leave_sync_field(window, cx),
                    cx,
                ),
            )
            .child(note(words::settings_sync::joining_note())),
        )
    }

    pub(super) fn sync_devices_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map(|syncing| syncing.shown());
        let Some(shown) = shown.filter(|shown| !shown.devices.is_empty()) else {
            return kit::rows().child(note(
                words::settings_sync::your_devices_are_listed_here_once(),
            ));
        };
        let mut devices = shown.devices;
        devices.sort_by(|first, second| {
            second
                .this
                .cmp(&first.this)
                .then(second.online.cmp(&first.online))
                .then_with(|| first.name.to_lowercase().cmp(&second.name.to_lowercase()))
        });
        let mut body = kit::rows();
        for device in &devices {
            body = body.child(self.device_row(device, cx));
        }
        if devices.iter().all(|device| device.this) {
            body = body.child(note(words::settings_sync::no_other_device()));
        }
        if let Some((id, name)) = self.remotely.driving.clone() {
            body = body.child(self.driven_zones(&id, &name, cx));
        }
        body
    }

    fn drive_the_device(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let syncing = cx.global::<ResonateApp>().syncing.clone();
        if let Some((held, _)) = self.remotely.driving.take() {
            if let Some(syncing) = syncing.as_ref() {
                syncing.unwatch(&held);
            }
            self.remotely.driving_redraws = None;
            if held == id {
                cx.notify();
                return;
            }
        }
        if syncing.is_none() {
            self.report(
                Notice::Trouble(words::settings_sync::not_syncing().to_owned()),
                cx,
            );
            return;
        }
        self.remotely.driving = Some((id.to_owned(), name.to_owned()));
        self.remotely.driving_rows = DRIVEN_ROWS_AT_FIRST;
        self.remotely.driving_redraws = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(DRIVEN_REDRAWN_EVERY).await;
                let driving = this.update(cx, |this, cx| {
                    cx.notify();
                    this.remotely.driving.is_some()
                });
                if !driving.unwrap_or(false) {
                    return;
                }
            }
        }));
        cx.notify();
    }

    fn driven_zones(&mut self, id: &str, name: &str, cx: &mut Context<Self>) -> Div {
        let shown = cx
            .global::<ResonateApp>()
            .syncing
            .as_ref()
            .map_or(RemoteShown::Refused, |syncing| syncing.watch(id));
        let mut panel = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(kit::row_name(words::settings_sync::playing_on(name)));
        match shown {
            RemoteShown::Waiting => panel.child(note(words::settings_sync::driving_waits())),
            RemoteShown::Refused => panel.child(note(words::settings_sync::driving_refused())),
            RemoteShown::Zones(zones) if zones.is_empty() => {
                panel.child(note(words::settings_sync::driving_waits()))
            }
            RemoteShown::Zones(zones) => {
                for zone in zones {
                    panel = panel.child(self.driven_zone(id, &zone, cx));
                }
                panel
            }
        }
    }

    fn driven_zone(&mut self, id: &str, zone: &RemoteZoneShown, cx: &mut Context<Self>) -> Div {
        let place = zone.length.map_or_else(
            || elapsed::clock(zone.position),
            |length| {
                format!(
                    "{} / {}",
                    elapsed::clock(zone.position),
                    elapsed::clock(length)
                )
            },
        );
        let song = zone
            .song
            .clone()
            .unwrap_or_else(|| words::settings_sync::nothing_playing().to_owned());
        let louder = zone.volume.saturating_add(VOLUME_STEP).min(WHOLE_VOLUME);
        let quieter = zone.volume.saturating_sub(VOLUME_STEP);
        let keys = [
            (
                "previous",
                Icon::Previous,
                words::settings_sync::previous_song(),
                RemoteKey::Previous,
            ),
            (
                "toggle",
                if zone.sounding {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                if zone.sounding {
                    words::settings_sync::pause()
                } else {
                    words::settings_sync::play()
                },
                RemoteKey::Toggle,
            ),
            (
                "next",
                Icon::Next,
                words::settings_sync::next_song(),
                RemoteKey::Next,
            ),
            (
                "quieter",
                Icon::Minus,
                words::settings_sync::quieter(),
                RemoteKey::Volume(quieter),
            ),
            (
                "louder",
                Icon::Plus,
                words::settings_sync::louder(),
                RemoteKey::Volume(louder),
            ),
        ];
        let mut transport = div().flex().items_center().gap_1();
        for (which, icon, saying, key) in keys {
            let control = format!("sync-drive-{which}-{}", zone.zone);
            let device = id.to_owned();
            let at = zone.zone;
            transport = transport.child(self.in_the_ring_at(
                control.clone(),
                kit::icon_button(SharedString::from(control), icon, saying),
                move |this, _, cx| this.press_on_the_device(&device, at, key, None, cx),
                cx,
            ));
        }
        let heading = div()
            .flex()
            .items_center()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(0.0))
                    .child(
                        div()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .child(format!(
                                "{} · {}%",
                                zone.name,
                                (u32::from(zone.volume) + 5) / 10
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .text_color(rgb(theme::text()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(song),
                    )
                    .child(kit::figure(place)),
            )
            .child(transport);
        let rail = self.driven_rail(id, zone, cx);
        let rows = self.driven_rows(id, zone, cx);

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(heading)
            .children(rail)
            .child(rows)
    }

    fn driven_rail(
        &mut self,
        id: &str,
        zone: &RemoteZoneShown,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let length = zone.length.filter(|length| !length.is_zero())?;
        let filled = (zone.position.as_secs_f32() / length.as_secs_f32()).clamp(0.0, 1.0);
        let rail = self
            .remotely
            .driving_rails
            .entry(zone.zone)
            .or_default()
            .clone();
        let painted = rail.painted_as();
        let (position, at) = (zone.position, zone.zone);
        let control = format!("sync-drive-seek-{at}");
        let element = SharedString::from(control.clone()).into();
        let focused = self.controls.at(control, cx);
        let device = id.to_owned();
        let (forward, back, pressed, set) = (
            device.clone(),
            device.clone(),
            device.clone(),
            device.clone(),
        );
        let reading = spoken::listening().then(|| Reading {
            share: filled,
            said: words::settings_sync::position_of_length(
                elapsed::clock(length),
                elapsed::clock(position),
            )
            .into(),
        });

        Some(
            div()
                .id(SharedString::from(format!("sync-drive-seek-{at}")))
                .found_as(&element)
                .track_focus(&focused)
                .key_context(
                    KeyContext::parse(&format!("{CONTROL_CONTEXT} {RAIL_CONTEXT}"))
                        .unwrap_or_default(),
                )
                .on_action(cx.listener(move |this, _: &RailOn, _, cx| {
                    let to = stepped_on(position, length);
                    this.press_on_the_device(&forward, at, RemoteKey::Seek(to), None, cx);
                }))
                .on_action(cx.listener(move |this, _: &RailBack, _, cx| {
                    let to = position.saturating_sub(DRIVEN_SEEK_STEP);
                    this.press_on_the_device(&back, at, RemoteKey::Seek(to), None, cx);
                }))
                .when_some(reading, |control, reading| {
                    let entity = cx.entity().downgrade();
                    let (on, off) = (entity.clone(), entity.clone());
                    let (toward, away) = (device.clone(), device);
                    control
                        .read_as(words::settings_sync::driven_position(), reading)
                        .on_a11y_action(AccessibleAction::SetValue, move |asked, _, cx| {
                            let Some(share) = spoken::share_asked(asked) else {
                                return;
                            };
                            entity
                                .update(cx, |this, cx| {
                                    let to = length.mul_f32(share.clamp(0.0, 1.0));
                                    this.press_on_the_device(
                                        &set,
                                        at,
                                        RemoteKey::Seek(to),
                                        None,
                                        cx,
                                    );
                                })
                                .ok();
                        })
                        .on_a11y_action(AccessibleAction::Increment, move |_, _, cx| {
                            on.update(cx, |this, cx| {
                                let to = stepped_on(position, length);
                                this.press_on_the_device(
                                    &toward,
                                    at,
                                    RemoteKey::Seek(to),
                                    None,
                                    cx,
                                );
                            })
                            .ok();
                        })
                        .on_a11y_action(AccessibleAction::Decrement, move |_, _, cx| {
                            off.update(cx, |this, cx| {
                                let to = position.saturating_sub(DRIVEN_SEEK_STEP);
                                this.press_on_the_device(&away, at, RemoteKey::Seek(to), None, cx);
                            })
                            .ok();
                        })
                })
                .rounded_md()
                .focus_visible(focus::lit)
                .flex()
                .items_center()
                .h(theme::width(theme::RAIL_HEIGHT))
                .px(theme::width(theme::RAIL_THUMB / 2.0))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        if let Some(fraction) = rail.fraction_at(event.position) {
                            let to = length.mul_f32(fraction);
                            this.press_on_the_device(&pressed, at, RemoteKey::Seek(to), None, cx);
                        }
                    }),
                )
                .child(
                    div()
                        .relative()
                        .h(theme::width(theme::RAIL_TRACK))
                        .w_full()
                        .rounded_full()
                        .bg(rgb(theme::soft_edge()))
                        .child(
                            div()
                                .h_full()
                                .w(relative(filled))
                                .rounded_full()
                                .bg(rgb(theme::accent())),
                        )
                        .child(canvas(
                            move |bounds, _, _| painted.set(bounds),
                            |_, _, _, _| {},
                        )),
                ),
        )
    }

    fn driven_rows(&mut self, id: &str, zone: &RemoteZoneShown, cx: &mut Context<Self>) -> Div {
        let shown = rows_shown(zone, self.remotely.driving_rows);
        let more = zone.rows.len().saturating_sub(shown.len());
        let mut list = div().flex().flex_col();
        for (row, song) in shown {
            list = list.child(self.driven_row(id, zone, row, song, cx));
        }
        if more > 0 {
            let control = format!("sync-drive-more-{}", zone.zone);
            list = list.child(self.in_the_ring_at(
                control.clone(),
                kit::button(
                    SharedString::from(control),
                    None,
                    words::settings_sync::more_zones(more),
                    words::settings_sync::show_more_hint(),
                    Tone::Ghost,
                ),
                move |this, _, cx| {
                    this.remotely.driving_rows =
                        this.remotely.driving_rows.saturating_add(DRIVEN_ROWS_MORE);
                    cx.notify();
                },
                cx,
            ));
        }
        list
    }

    fn driven_row(
        &mut self,
        id: &str,
        zone: &RemoteZoneShown,
        row: usize,
        song: &RemoteRowShown,
        cx: &mut Context<Self>,
    ) -> Div {
        let at = zone.zone;
        let drawn = zone.queue_revision;
        let playing = zone.playing == Some(row);
        let saying = if song.artist.is_empty() {
            song.title.clone()
        } else {
            format!("{} — {}", song.title, song.artist)
        };
        let jump = format!("sync-drive-jump-{at}-{row}");
        let device = id.to_owned();
        let named = SharedString::from(jump.clone()).into();
        let pressed = div()
            .id(SharedString::from(jump.clone()))
            .found_as(&named)
            .named_as(Role::Button, &saying, words::settings_sync::jump_hint())
            .names(words::settings_sync::jump_hint())
            .flex()
            .flex_1()
            .min_w(px(0.0))
            .items_center()
            .gap_2()
            .px_2()
            .py_0p5()
            .rounded_md()
            .cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(if playing {
                        theme::accent()
                    } else {
                        theme::text()
                    }))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .child(saying),
            )
            .children(
                song.length
                    .map(|length| kit::figure(elapsed::clock(length))),
            );
        let mut line = div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.in_the_ring_at(
                jump,
                pressed,
                move |this, _, cx| {
                    this.press_on_the_device(&device, at, RemoteKey::Jump(row), drawn, cx);
                },
                cx,
            ));
        let moves = [
            (
                "up",
                Icon::ChevronUp,
                words::settings_sync::up_hint(),
                row.checked_sub(1)
                    .map(|to| RemoteKey::Move { from: row, to }),
            ),
            (
                "down",
                Icon::ChevronDown,
                words::settings_sync::down_hint(),
                (row + 1 < zone.queue_length).then_some(RemoteKey::Move {
                    from: row,
                    to: row + 1,
                }),
            ),
            (
                "remove",
                Icon::Close,
                words::settings_sync::remove_hint(),
                Some(RemoteKey::Remove(row)),
            ),
        ];
        for (which, icon, hint, key) in moves {
            let Some(key) = key else { continue };
            let control = format!("sync-drive-{which}-{at}-{row}");
            let device = id.to_owned();
            line = line.child(self.in_the_ring_at(
                control.clone(),
                kit::icon_button(SharedString::from(control), icon, hint),
                move |this, _, cx| this.press_on_the_device(&device, at, key, drawn, cx),
                cx,
            ));
        }
        line
    }

    pub(crate) fn press_on_the_device(
        &mut self,
        id: &str,
        zone: u32,
        key: RemoteKey,
        drawn: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            self.report(
                Notice::Trouble(words::settings_sync::not_syncing().to_owned()),
                cx,
            );
            return;
        };
        let device = id.to_owned();
        let pressing = cx
            .background_executor()
            .spawn(async move { syncing.press(&device, zone, key, drawn) });
        cx.spawn(async move |this, cx| {
            if !pressing.await {
                let _ = this.update(cx, |this, cx| {
                    this.report(
                        Notice::Trouble(words::settings_sync::not_driven().to_owned()),
                        cx,
                    );
                });
            }
        })
        .detach();
    }

    pub(super) fn sync_connection_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let chosen = Route::of(&synchronised, self.settings_shown.own_server_asked);

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
        let mut body = kit::rows().child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(div().flex().child(routes))
                .child(note(chosen.meaning())),
        );
        if chosen == Route::Own {
            body = body.child(kit::field(
                words::settings_sync::server(),
                self.text_field(
                    "sync-server",
                    &self.typed.sync_server,
                    |this, window, cx| this.leave_sync_field(window, cx),
                    cx,
                ),
            ));
        }
        body.child(kit::field(
            words::settings_sync::device_name(),
            self.text_field(
                "sync-device",
                &self.typed.sync_device,
                |this, window, cx| this.leave_sync_field(window, cx),
                cx,
            ),
        ))
        .child(kit::field(
            words::settings_sync::your_name_on_shared_playlists_and(),
            self.text_field(
                "shared-name",
                &self.typed.shared_name,
                |this, window, cx| this.leave_sync_field(window, cx),
                cx,
            ),
        ))
        .note(words::settings_sync::syncing_note())
    }

    pub(super) fn synced_songs_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let songs = cx.global::<ResonateApp>().synchronised.songs;

        kit::rows()
            .child(self.in_the_ring(
                "sync-favourites",
                switch_row(
                    words::settings_sync::fetch_favourites_this_device_lacks(),
                    words::settings_sync::a_song_favourited_on_another_device(),
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
                    words::settings_sync::fetch_the_songs_of_playlists_kept(),
                    words::settings_sync::a_playlist_whose_menu_says_keep(),
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
                    words::settings_sync::fetch_everything(),
                    words::settings_sync::every_song_another_device_holds_and(),
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
            .note(words::settings_sync::songs_note())
    }

    pub(super) fn sync_control_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let chosen = cx.global::<ResonateApp>().synchronised.control;

        let mut choices = kit::segmented();
        for (index, trust) in RemoteTrust::ALL.into_iter().enumerate() {
            choices = choices.child(self.option_in_the_ring(
                kit::segment(("sync-control", index), trust.label(), trust == chosen),
                "sync-control",
                index,
                move |this, _, cx| this.trust_other_devices(trust, cx),
                cx,
            ));
        }
        kit::rows()
            .child(kit::stacked_row(
                remote_trust_asked(),
                "",
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(div().flex().child(choices))
                    .child(note(chosen.meaning())),
            ))
            .note(remote_trust_told())
            .note(words::settings_sync::syncing_note())
    }

    pub(crate) fn trust_other_devices(&mut self, control: RemoteTrust, cx: &mut Context<Self>) {
        self.synchronise(
            |synchronised| synchronised.control = control,
            &Setting::SyncControl(control),
            cx,
        );
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
        self.settings_shown.pairing_shown = shown;
        cx.notify();
    }

    fn regenerate_button(&mut self, cx: &mut Context<Self>) -> Stateful<Div> {
        let armed = self.is_armed(&Armed::RegenerateTheCode);
        self.in_the_ring(
            "sync-regenerate",
            kit::button(
                "sync-regenerate",
                Some(Icon::Undo),
                if armed {
                    words::settings_sync::press_again_to_make_a_new()
                } else {
                    words::settings_sync::new_code()
                },
                words::settings_sync::regenerate_hint(),
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
                    this.arm(Armed::RegenerateTheCode);
                    cx.notify();
                }
            },
            cx,
        )
    }

    fn device_row(&mut self, device: &SyncedDevice, cx: &mut Context<Self>) -> Div {
        let state = match (device.this, device.nearby, device.online) {
            (true, _, _) => words::settings_sync::this_device(),
            (false, true, _) => words::settings_sync::on_this_network(),
            (false, false, true) => words::settings_sync::online(),
            (false, false, false) => words::settings_sync::away(),
        };
        let under = platform_name(device.platform).map_or_else(
            || state.to_owned(),
            |platform| format!("{platform} · {state}"),
        );
        let row = div()
            .flex()
            .items_center()
            .gap_3()
            .child(device_mark(device.platform, device.online))
            .child(kit::named(device.name.clone(), under));
        if device.online {
            if device.this {
                return row;
            }
            let id = device.id.clone();
            let name = device.name.clone();
            let driven = self
                .remotely
                .driving
                .as_ref()
                .is_some_and(|(held, _)| *held == device.id);
            let control = format!("sync-drive-{}", device.id);
            let (label, hint) = if driven {
                (
                    words::settings_sync::stop_driving(),
                    words::settings_sync::stop_driving_hint(),
                )
            } else {
                (
                    words::settings_sync::drive(),
                    words::settings_sync::drive_hint(),
                )
            };
            return row.child(self.in_the_ring_at(
                control.clone(),
                kit::button(
                    SharedString::from(control),
                    Some(Icon::Play),
                    label,
                    hint,
                    Tone::Ghost,
                ),
                move |this, _, cx| this.drive_the_device(&id, &name, cx),
                cx,
            ));
        }
        let id = device.id.clone();
        let name = device.name.clone();
        let control = format!("sync-forget-{}", device.id);
        row.child(self.in_the_ring_at(
            control.clone(),
            kit::button(
                SharedString::from(control),
                Some(Icon::Delete),
                words::settings_sync::remove(),
                words::settings_sync::forget_hint(),
                Tone::Ghost,
            ),
            move |this, _, cx| this.forget_sync_device(&id, &name, cx),
            cx,
        ))
    }

    fn regenerate_the_code(&mut self, cx: &mut Context<Self>) {
        self.lower(&Armed::RegenerateTheCode);
        self.settings_shown.code_regenerated = true;
        self.settings_shown.pairing_shown = false;
        self.store(&Setting::SyncKey(String::new()), cx);
        self.report(
            Notice::Done(words::settings_sync::regenerated().to_owned()),
            cx,
        );
        cx.notify();
    }

    fn forget_sync_device(&mut self, id: &str, name: &str, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            self.report(
                Notice::Trouble(words::settings_sync::not_syncing().to_owned()),
                cx,
            );
            return;
        };
        let notice = if syncing.forget(id) {
            Notice::Done(words::settings_sync::off_the_list(name))
        } else {
            Notice::Trouble(words::settings_sync::not_taken_off_the_list(name))
        };
        self.report(notice, cx);
        cx.notify();
    }

    fn take_the_route(&mut self, route: Route, cx: &mut Context<Self>) {
        self.settings_shown.own_server_asked = route == Route::Own;
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
            self.typed
                .sync_server
                .update(cx, |field, cx| field.hold(String::new(), cx));
            self.synchronise(
                |synchronised| synchronised.server.clear(),
                &Setting::SyncServer(String::new()),
                cx,
            );
            self.report(
                Notice::Done(
                    words::settings_sync::devices_sync_through_resonates_server().to_owned(),
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
            words::settings_sync::devices_sync_only_on_this_network()
        } else {
            words::settings_sync::devices_sync_through_a_server_again()
        };
        self.report(Notice::Done(said.to_owned()), cx);
    }

    pub(crate) fn force_sync(&mut self, cx: &mut Context<Self>) {
        let Some(syncing) = cx.global::<ResonateApp>().syncing.clone() else {
            self.report(
                Notice::Trouble(words::settings_sync::not_syncing().to_owned()),
                cx,
            );
            return;
        };
        syncing.force();
        self.report(Notice::Done(words::settings_sync::forced().to_owned()), cx);
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
        let typed = self.typed.sync_server.read(cx).text().trim().to_owned();
        self.typed
            .sync_server
            .update(cx, |field, cx| field.hold(typed.clone(), cx));
        let said = if typed.is_empty() {
            words::settings_sync::devices_sync_through_resonates_server()
        } else {
            words::settings_sync::devices_sync_through_that_server()
        };
        self.settings_shown.own_server_asked = !typed.is_empty();
        self.synchronise(
            |synchronised| synchronised.server.clone_from(&typed),
            &Setting::SyncServer(typed.clone()),
            cx,
        );
        self.report(Notice::Done(said.to_owned()), cx);
        window.focus(&self.focus, cx);
    }

    pub(crate) fn shared_name_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.shared_name.read(cx).text().trim().to_owned();
        self.typed
            .shared_name
            .update(cx, |field, cx| field.hold(typed.clone(), cx));
        self.synchronise(
            |synchronised| synchronised.shared_name.clone_from(&typed),
            &Setting::SharedName(typed.clone()),
            cx,
        );
        if let Some(together) = cx.global::<ResonateApp>().together.clone() {
            together.named(&typed);
        }
        window.focus(&self.focus, cx);
    }

    pub(crate) fn sync_device_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.sync_device.read(cx).text().trim().to_owned();
        self.typed
            .sync_device
            .update(cx, |field, cx| field.hold(typed.clone(), cx));
        self.synchronise(
            |synchronised| synchronised.device.clone_from(&typed),
            &Setting::SyncDevice(typed.clone()),
            cx,
        );
        self.report(
            Notice::Done(words::settings_sync::your_other_devices_learn_the_new().to_owned()),
            cx,
        );
        window.focus(&self.focus, cx);
    }

    pub(crate) fn sync_code_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let typed = self.typed.sync_code.read(cx).text().trim().to_owned();
        if typed.is_empty() {
            return;
        }
        let reads = cx
            .global::<ResonateApp>()
            .reads_as_a_code
            .is_none_or(|reads| reads(&typed));
        if !reads {
            self.report(
                Notice::Trouble(words::settings_sync::not_a_code().to_owned()),
                cx,
            );
            return;
        }
        self.typed
            .sync_code
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.store(&Setting::SyncKey(typed), cx);
        self.report(
            Notice::Done(words::settings_sync::this_device_joins_those_devices().to_owned()),
            cx,
        );
        window.focus(&self.focus, cx);
    }

    pub(crate) fn leave_sync_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let focused = [
            &self.typed.sync_code,
            &self.typed.sync_device,
            &self.typed.shared_name,
            &self.typed.sync_server,
        ]
        .iter()
        .any(|field| field.read(cx).is_focused(window));
        if !focused {
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn put_the_connection_back(&mut self, cx: &mut Context<Self>) {
        let built = Synchronised::default();
        self.settings_shown.own_server_asked = false;
        self.typed
            .sync_server
            .update(cx, |field, cx| field.hold(String::new(), cx));
        self.typed
            .sync_device
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

fn stepped_on(position: Duration, length: Duration) -> Duration {
    (position + DRIVEN_SEEK_STEP).min(length)
}

fn rows_shown(zone: &RemoteZoneShown, most: usize) -> Vec<(usize, &RemoteRowShown)> {
    let from = zone
        .playing
        .map_or(zone.first_row, |playing| {
            playing.saturating_sub(DRIVEN_ROWS_BEFORE_THE_ONE_PLAYING)
        })
        .max(zone.first_row);

    zone.rows
        .iter()
        .enumerate()
        .map(|(offset, row)| (zone.first_row + offset, row))
        .filter(|(row, _)| *row >= from)
        .take(most)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use gpui::TestAppContext;
    use parking_lot::Mutex;
    use resonate_core::PlaylistId;
    use resonate_library::Library;

    use super::*;
    use crate::{Continuing, HandedOff, Syncing, driven::Driven};

    type Pressed = (String, u32, RemoteKey, Option<u64>);

    const ROWS: usize = 3;
    const DRAWN_REVISION: u64 = 12;

    #[derive(Default)]
    struct Stage {
        pressed: Mutex<Vec<Pressed>>,
        unwatched: Mutex<Vec<String>>,
        second: AtomicBool,
        away: AtomicBool,
        refused: AtomicBool,
        paused: AtomicBool,
    }

    fn row(title: &str) -> RemoteRowShown {
        RemoteRowShown {
            title: title.to_owned(),
            artist: "Pink Floyd".to_owned(),
            length: Some(Duration::from_secs(200)),
        }
    }

    fn a_zone() -> RemoteZoneShown {
        RemoteZoneShown {
            zone: 1,
            name: "Living room".to_owned(),
            song: Some("Time".to_owned()),
            sounding: true,
            position: Duration::from_secs(50),
            length: Some(Duration::from_secs(200)),
            volume: 500,
            playing: Some(1),
            queue_length: ROWS,
            first_row: 0,
            queue_revision: Some(DRAWN_REVISION),
            rows: vec![row("Echoes"), row("Time"), row("Money")],
        }
    }

    impl Syncing for Stage {
        fn shown(&self) -> SyncShown {
            let desk = SyncedDevice {
                id: "desk".to_owned(),
                name: "Desk".to_owned(),
                platform: SyncedPlatform::Linux,
                online: !self.away.load(Ordering::Relaxed),
                nearby: false,
                this: false,
            };
            let mut devices = vec![desk.clone()];
            if self.second.load(Ordering::Relaxed) {
                devices.push(SyncedDevice {
                    id: "phone".to_owned(),
                    name: "Phone".to_owned(),
                    platform: SyncedPlatform::Android,
                    ..desk
                });
            }
            SyncShown {
                standing: SyncStanding::Connected,
                devices,
                ..SyncShown::default()
            }
        }

        fn force(&self) {}

        fn forget(&self, _: &str) -> bool {
            false
        }

        fn missing_from(&self, _: PlaylistId) -> usize {
            0
        }

        fn fetch_missing(&self, _: PlaylistId, _: &str) {}

        fn keeps_songs_of(&self, _: PlaylistId) -> bool {
            false
        }

        fn keep_songs_of(&self, _: PlaylistId, _: bool) {}

        fn handed_off(&self) -> Vec<HandedOff> {
            Vec::new()
        }

        fn continue_from(&self, _: &str) -> Continuing {
            Continuing::default()
        }

        fn watch(&self, device: &str) -> RemoteShown {
            if device == "phone" && self.refused.load(Ordering::Relaxed) {
                return RemoteShown::Refused;
            }
            let mut zone = a_zone();
            zone.sounding = !self.paused.load(Ordering::Relaxed);
            if device == "phone" {
                zone.zone = 7;
            }
            RemoteShown::Zones(vec![zone])
        }

        fn unwatch(&self, device: &str) {
            self.unwatched.lock().push(device.to_owned());
        }

        fn press(&self, device: &str, zone: u32, key: RemoteKey, drawn: Option<u64>) -> bool {
            self.pressed
                .lock()
                .push((device.to_owned(), zone, key, drawn));
            if key == RemoteKey::Toggle {
                self.paused.fetch_xor(true, Ordering::Relaxed);
            }
            true
        }
    }

    fn opened_on_the_sync_group(cx: &mut TestAppContext) -> (Driven, Arc<Stage>) {
        let mut driven = Driven::open(cx, Arc::new(in_memory()));
        let stage = Arc::new(Stage::default());
        let seam = Arc::clone(&stage) as Arc<dyn Syncing>;

        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|app, _| app.syncing = Some(seam));
        });
        driven.click("tab-settings");
        driven.focus(|root| &root.typed.finding);
        driven.cx.simulate_input("your devices");
        driven.settle();
        (driven, stage)
    }

    fn in_memory() -> Library {
        Library::open_in_memory().expect("a catalog")
    }

    #[gpui::test]
    fn sidebar_players_drive_the_named_device_across_panes_and_follow_its_permission(
        cx: &mut TestAppContext,
    ) {
        let (mut driven, stage) = opened_on_the_sync_group(cx);
        stage.second.store(true, Ordering::Relaxed);
        driven.until(|root, _| root.remotely.songs.len() == 2);

        driven.click("remote-desk-1-previous");
        driven.click("remote-phone-7-toggle");
        driven.until(|_, _| stage.pressed.lock().len() == 2);
        driven.click("tab-albums");
        let root = driven.root.clone();
        driven.cx.update(|window, cx| {
            let next = root
                .read(cx)
                .remotely
                .controls
                .handle("remote-phone-7-next")
                .expect("the remote controls survive a pane change");
            window.focus(&next, cx);
        });
        driven.cx.simulate_keystrokes("enter");
        driven.until(|_, _| stage.pressed.lock().len() == 3);
        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        let root = driven.root.clone();
        assert!(
            driven
                .cx
                .update(|window, cx| !root.read(cx).remotely.controls.holds_the_caret(window))
        );
        assert_eq!(
            *stage.pressed.lock(),
            vec![
                ("desk".to_owned(), 1, RemoteKey::Previous, None),
                ("phone".to_owned(), 7, RemoteKey::Toggle, None),
                ("phone".to_owned(), 7, RemoteKey::Next, None),
            ]
        );
        assert!(driven.read(|root, cx| root.player.read(cx).state().current.is_none()));

        stage.refused.store(true, Ordering::Relaxed);
        driven.until(|root, _| root.remotely.songs.len() == 1);
        driven.settle();
        assert!(driven.cx.debug_bounds("remote-phone-7-toggle").is_none());
        stage.away.store(true, Ordering::Relaxed);
        driven.until(|root, _| root.remotely.songs.is_empty());
        assert!(
            stage
                .unwatched
                .lock()
                .iter()
                .any(|device| device == "phone")
        );
        assert!(stage.unwatched.lock().iter().any(|device| device == "desk"));
        assert!(driven.bounds_of("tab-settings").size.height > px(0.0));
    }

    #[gpui::test]
    fn a_driven_device_is_sought_jumped_moved_and_emptied_from_its_rows(cx: &mut TestAppContext) {
        let (mut driven, stage) = opened_on_the_sync_group(cx);

        driven.click("sync-drive-desk");
        driven.settle();
        driven.click("sync-drive-seek-1");
        driven.click("sync-drive-jump-1-2");
        driven.click("sync-drive-up-1-2");
        driven.click("sync-drive-down-1-0");
        driven.click("sync-drive-remove-1-1");
        driven.until(|_, _| stage.pressed.lock().len() == 5);

        let pressed = stage.pressed.lock().clone();

        assert!(
            pressed
                .iter()
                .all(|(device, zone, _, _)| device == "desk" && *zone == 1)
        );
        let RemoteKey::Seek(sought) = pressed[0].2 else {
            panic!("a seek, not {:?}", pressed[0].2);
        };
        assert!(sought > Duration::from_secs(80) && sought < Duration::from_secs(120));
        assert_eq!(pressed[1].2, RemoteKey::Jump(2));
        assert_eq!(pressed[2].2, RemoteKey::Move { from: 2, to: 1 });
        assert_eq!(pressed[3].2, RemoteKey::Move { from: 0, to: 1 });
        assert_eq!(pressed[4].2, RemoteKey::Remove(1));
        assert_eq!(pressed[0].3, None);
        assert!(
            pressed[1..]
                .iter()
                .all(|(_, _, _, drawn)| *drawn == Some(DRAWN_REVISION))
        );
    }

    #[test]
    fn only_a_rows_real_neighbours_are_offered_and_the_shown_rows_start_about_the_one_playing() {
        let many = RemoteZoneShown {
            playing: Some(20),
            queue_length: 40,
            first_row: 5,
            rows: (5..40).map(|at| row(&format!("{at}"))).collect(),
            ..a_zone()
        };

        let shown = rows_shown(&many, 4);

        assert_eq!(
            shown.iter().map(|(at, _)| *at).collect::<Vec<_>>(),
            [18, 19, 20, 21]
        );
        assert_eq!(shown[2].1.title, "20");
        assert_eq!(
            rows_shown(&a_zone(), 12)
                .iter()
                .map(|(at, _)| *at)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(
            stepped_on(Duration::from_secs(198), Duration::from_secs(200)),
            Duration::from_secs(200)
        );
    }

    #[gpui::test]
    fn what_other_devices_may_do_is_chosen_in_the_pane_and_worn_at_once(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, Arc::new(in_memory()));

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().synchronised.control),
            RemoteTrust::Everything
        );

        driven.click("tab-settings");
        driven.focus(|root| &root.typed.finding);
        driven.cx.simulate_input("driven by your other devices");
        driven.settle();
        driven.click("sync-control-1");
        driven.settle();

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().synchronised.control),
            RemoteTrust::Transport
        );
    }

    #[test]
    fn the_songs_on_their_way_say_how_many_are_downloading_and_how_many_wait() {
        assert_eq!(on_their_way(0, 0), None);
        assert_eq!(
            on_their_way(2, 340).as_deref(),
            Some("342 songs on their way from another device · 2 downloading · 340 waiting")
        );
        assert_eq!(
            on_their_way(1, 0).as_deref(),
            Some("1 song on their way from another device · 1 downloading")
        );
        assert_eq!(
            on_their_way(0, 3).as_deref(),
            Some("3 songs on their way from another device · 3 waiting")
        );
    }
}
