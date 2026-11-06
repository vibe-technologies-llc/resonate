use std::{sync::Arc, time::Duration};

use gpui::{
    BoxShadow, Canvas, Context, Div, FontWeight, MouseButton, MouseDownEvent, Pixels, Role,
    SharedString, Stateful, Window, canvas, div, point, prelude::*, px, relative, rgb,
};
use resonate_core::Recording;
use resonate_library::{Library, Track};

use crate::{
    Drawn, RemoteKey, RemoteShown, RemoteZoneShown, ResonateApp, SyncedDevice, SyncedPlatform,
    icons::{self, Icon},
    theme,
    views::{
        kit::{self, EndsInAnEllipsis},
        listing::Pictured,
        root::{RootView, framed_cover},
        scrollbar::Scrollbars,
        spoken::Spoken,
    },
};

const READ_EVERY: Duration = Duration::from_secs(1);
const FLOATS_CLEAR_OF_THE_TOP_BY: Pixels = px(16.0);
const MINUTE: u64 = 60;

#[derive(Clone, PartialEq)]
pub(super) struct RemoteSong {
    device: SyncedDevice,
    zone: RemoteZoneShown,
    recording: Recording,
    track: Option<Track>,
    album: Option<String>,
}

impl RemoteSong {
    fn new(device: SyncedDevice, mut zone: RemoteZoneShown) -> Option<Self> {
        let song = zone.song.as_ref()?;
        let row = zone
            .playing
            .and_then(|playing| playing.checked_sub(zone.first_row))
            .and_then(|row| zone.rows.get(row));
        let recording = Recording {
            mbid: None,
            title: row.map_or_else(|| song.clone(), |row| row.title.clone()),
            artist: row.map_or_else(String::new, |row| row.artist.clone()),
            album: String::new(),
            length: zone.length,
        };
        zone.rows.clear();
        Some(Self {
            device,
            zone,
            recording,
            track: None,
            album: None,
        })
    }

    fn read_album(&mut self, library: &Library) -> resonate_library::Result<()> {
        let Some(cut) = library.resolve_recording(&self.recording)? else {
            return Ok(());
        };
        let Some(path) = cut.location.as_path() else {
            return Ok(());
        };
        self.track = library.track_at(path, cut.span)?;
        if let Some(album) = self.track.as_ref().and_then(|track| track.album_id) {
            self.album = library.album(album)?.map(|album| album.title);
        }
        Ok(())
    }
}

impl RootView {
    pub(super) fn watch_remote_playback(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let mut watched: Vec<String> = Vec::new();
            let mut previous_syncing: Option<Arc<dyn crate::Syncing>> = None;
            let mut previous_songs: Vec<RemoteSong> = Vec::new();
            let mut previous_stamp = None;
            loop {
                cx.background_executor().timer(READ_EVERY).await;
                let Ok((mut songs, library)) = this.update(cx, |_, cx| {
                    let syncing = cx.global::<ResonateApp>().syncing.clone();
                    if let Some(previous) = previous_syncing.as_ref()
                        && syncing
                            .as_ref()
                            .is_none_or(|current| !Arc::ptr_eq(previous, current))
                    {
                        for id in watched.drain(..) {
                            previous.unwatch(&id);
                        }
                    }
                    let mut songs = Vec::new();
                    if let Some(syncing) = syncing.as_ref() {
                        let mut current = Vec::new();
                        for device in syncing
                            .shown()
                            .devices
                            .into_iter()
                            .filter(|device| !device.this && device.online)
                        {
                            current.push(device.id.clone());
                            if let RemoteShown::Zones(zones) = syncing.watch(&device.id) {
                                songs.extend(
                                    zones
                                        .into_iter()
                                        .filter_map(|zone| RemoteSong::new(device.clone(), zone)),
                                );
                            }
                        }
                        for id in watched.iter().filter(|id| !current.contains(id)) {
                            syncing.unwatch(id);
                        }
                        watched = current;
                    }
                    previous_syncing = syncing;
                    (songs, Arc::clone(&cx.global::<ResonateApp>().library))
                }) else {
                    break;
                };
                let (enriched, stamp) = cx
                    .background_executor()
                    .spawn(async move {
                        let stamp = library.names_stamp();
                        for song in &mut songs {
                            if previous_stamp == Some(stamp)
                                && let Some(previous) = previous_songs
                                    .iter()
                                    .find(|previous| previous.recording == song.recording)
                            {
                                song.track.clone_from(&previous.track);
                                song.album.clone_from(&previous.album);
                            } else if let Err(error) = song.read_album(&library) {
                                tracing::warn!(%error, "a remote song's album could not be read");
                            }
                        }
                        (songs, stamp)
                    })
                    .await;
                previous_songs = enriched.clone();
                previous_stamp = Some(stamp);
                if this
                    .update(cx, |this, cx| {
                        if this.remote_songs != enriched {
                            this.remote_songs = enriched;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
            }
            if let Some(syncing) = previous_syncing {
                for id in watched {
                    syncing.unwatch(&id);
                }
            }
        })
        .detach();
    }

    pub(super) fn remote_playback(&self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        self.remote_controls.opening();
        if self.remote_songs.is_empty() {
            return None;
        }
        let floating = self.remote_floats(window);
        let room = (window.viewport_size().height
            - px(theme::header_height() + theme::transport_height()))
            * 0.45;
        let room = room.min(px(theme::sidebar_width() * 2.0));
        let room = if floating {
            room.min(
                (self.sidebar_scroll.bounds().size.height - FLOATS_CLEAR_OF_THE_TOP_BY)
                    .max(px(0.0)),
            )
        } else {
            room
        };
        let collapsed = self.remote_collapsed;
        let label = if collapsed {
            "Expand other devices"
        } else {
            "Collapse other devices"
        };
        let mut heading = kit::button(
            "toggle-other-devices",
            Some(if collapsed {
                Icon::ChevronRight
            } else {
                Icon::ChevronDown
            }),
            "Other devices",
            label,
            kit::Tone::Ghost,
        )
        .w_full()
        .font_weight(FontWeight::NORMAL)
        .when(super::spoken::listening(), |heading| {
            heading.aria_expanded(!collapsed)
        });
        self.remote_controls
            .captioned_by_its_name("toggle-other-devices", &mut heading);
        let handle = self.remote_controls.at("toggle-other-devices", cx);
        let heading = Self::ringed(
            &handle,
            heading,
            |this, _, cx| {
                this.remote_collapsed = !this.remote_collapsed;
                this.remote_expanded_by_hand = !this.remote_collapsed;
                cx.notify();
            },
            cx,
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, press: &MouseDownEvent, _, _| {
                this.remote_heading_pressed_at = Some(press.position);
            }),
        );
        Some(
            div()
                .relative()
                .flex()
                .flex_col()
                .gap_1()
                .py_1()
                .child(heading)
                .when(!collapsed, |panel| {
                    let cards: Vec<_> = self
                        .remote_songs
                        .iter()
                        .map(|song| self.remote_card(song, cx))
                        .collect();
                    let list = div()
                        .relative()
                        .child(
                            div()
                                .id("remote-devices")
                                .debug_selector(|| "remote-devices".to_owned())
                                .flex()
                                .flex_col()
                                .gap_1p5()
                                .max_h(room)
                                .overflow_y_scroll()
                                .track_scroll(&self.remote_scroll)
                                .children(cards),
                        )
                        .child(
                            Scrollbars::of(cx)
                                .vertical("remote-devices-scrollbar", self.remote_scroll.clone()),
                        );
                    if floating {
                        panel.child(Self::floating_over_the_categories(list, cx))
                    } else {
                        panel.child(list)
                    }
                }),
        )
    }

    fn floating_over_the_categories(list: Div, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("remote-devices-floating")
            .debug_selector(|| "remote-devices-floating".to_owned())
            .absolute()
            .left_0()
            .right_0()
            .bottom(relative(1.0))
            .p_1p5()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::soft_edge()))
            .shadow(vec![BoxShadow {
                color: theme::shadow(0.5),
                offset: point(px(0.0), px(12.0)),
                blur_radius: px(32.0),
                spread_radius: px(0.0),
                inset: false,
            }])
            .occlude()
            .on_mouse_down_out(cx.listener(|_, press: &MouseDownEvent, window, cx| {
                let at = press.position;
                cx.defer_in(window, move |this, _, cx| {
                    let on_the_heading = this.remote_heading_pressed_at.take() == Some(at);
                    if !on_the_heading {
                        this.remote_collapsed = true;
                        this.remote_expanded_by_hand = false;
                        cx.notify();
                    }
                });
            }))
            .child(list)
    }

    pub(super) fn remote_floats(&self, window: &Window) -> bool {
        self.remote_fits_from
            .is_some_and(|tall| window.viewport_size().height < tall)
    }

    pub(super) fn float_remote_when_categories_scroll(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> Canvas<()> {
        let open_in_flow =
            !self.remote_collapsed && !self.remote_songs.is_empty() && !self.remote_floats(window);
        let by_hand = self.remote_expanded_by_hand;
        let scroll = self.sidebar_scroll.clone();
        let this = cx.entity().downgrade();
        canvas(
            |_, _, _| (),
            move |_, _, window, cx| {
                if !open_in_flow {
                    return;
                }
                let overflow = scroll.max_offset().y;
                if overflow > px(0.0) {
                    let fits_from = window.viewport_size().height + overflow;
                    cx.defer(move |cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.remote_fits_from = Some(fits_from);
                            this.remote_collapsed = !this.remote_expanded_by_hand;
                            this.remote_expanded_by_hand = false;
                            cx.notify();
                        });
                    });
                } else if by_hand {
                    cx.defer(move |cx| {
                        let _ = this.update(cx, |this, _| this.remote_expanded_by_hand = false);
                    });
                }
            },
        )
        .absolute()
        .inset_0()
    }

    fn remote_card(&self, song: &RemoteSong, cx: &mut Context<Self>) -> Stateful<Div> {
        let at = song.zone.zone;
        let device = &song.device;
        let id = format!("remote-{}-{at}", device.id);
        let state = if song.zone.sounding {
            "Playing"
        } else {
            "Paused"
        };
        let mark = match device.platform {
            SyncedPlatform::Android | SyncedPlatform::Ios => Icon::Phone,
            SyncedPlatform::Linux | SyncedPlatform::Windows | SyncedPlatform::Other => {
                Icon::Computer
            }
        };
        let colour = if song.zone.sounding {
            theme::accent()
        } else {
            theme::muted()
        };
        let cover = song.track.as_ref().map_or_else(
            || framed_cover(None, theme::now_playing_cover()),
            |track| {
                self.cover_sized(
                    Pictured::Track {
                        album: track.album_id,
                        file: &track.location,
                    },
                    Drawn::NowPlaying,
                    theme::now_playing_cover(),
                    cx,
                )
            },
        );
        let mut words = div()
            .id("remote-song-details")
            .read_plainly_as(|| {
                SharedString::from(format!(
                    "{}, {}, {state}",
                    song.recording.title, song.recording.artist
                ))
            })
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .child(
                remote_line(&song.recording.title, theme::text())
                    .text_size(px(theme::text_sm()))
                    .font_weight(FontWeight::MEDIUM),
            );
        if !song.recording.artist.is_empty() {
            words = words.child(remote_line(&song.recording.artist, theme::muted()));
        }
        if let Some(album) = &song.album {
            words = words.child(remote_line(album, theme::faint()));
        }
        let mut controls = div()
            .flex()
            .flex_none()
            .items_center()
            .h(px(theme::row_control()))
            .gap_1();
        for (which, icon, label, key) in [
            (
                "previous",
                Icon::Previous,
                "Previous song",
                RemoteKey::Previous,
            ),
            (
                "toggle",
                if song.zone.sounding {
                    Icon::Pause
                } else {
                    Icon::Play
                },
                if song.zone.sounding { "Pause" } else { "Play" },
                RemoteKey::Toggle,
            ),
            ("next", Icon::Next, "Next song", RemoteKey::Next),
        ] {
            let named = format!("{id}-{which}");
            let saying = format!("{label} on {} · {}", device.name, song.zone.name);
            let button = if key == RemoteKey::Toggle {
                kit::lit_mark(SharedString::from(named.clone()), icon, saying)
                    .rounded_full()
                    .bg(theme::tinted(theme::accent(), 0x18))
            } else {
                kit::icon_button(SharedString::from(named.clone()), icon, saying)
            };
            let handle = self.remote_controls.at(&named, cx);
            let mut button = button;
            self.remote_controls
                .captioned_by_its_name(&named, &mut button);
            let device = device.id.clone();
            controls = controls.child(Self::ringed(
                &handle,
                button,
                move |this, _, cx| this.press_on_the_device(&device, at, key, cx),
                cx,
            ));
        }
        let place = song.zone.length.map_or_else(
            || clock(song.zone.position),
            |length| format!("{} / {}", clock(song.zone.position), clock(length)),
        );
        div()
            .id(SharedString::from(id))
            .named_as(Role::Group, &device.name, &song.zone.name)
            .flex()
            .flex_none()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::soft_edge()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(mark, theme::text_sm(), colour))
                    .child(remote_line(&device.name, theme::muted()).flex_1().min_w_0())
                    .child(remote_line(state, colour).flex_none()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(cover)
                    .child(words),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(theme::row_control()))
                    .w_full()
                    .gap_1()
                    .child(
                        kit::figure(place)
                            .flex()
                            .items_center()
                            .h(px(theme::row_control())),
                    )
                    .child(controls),
            )
    }
}

fn remote_line(text: &str, colour: u32) -> Div {
    div()
        .text_size(px(theme::text_xs()))
        .text_color(rgb(colour))
        .truncate()
        .ends_in_an_ellipsis()
        .child(SharedString::from(text.to_owned()))
}

fn clock(at: Duration) -> String {
    format!("{}:{:02}", at.as_secs() / MINUTE, at.as_secs() % MINUTE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RemoteRowShown,
        driven::{Driven, Folder},
    };

    fn song() -> RemoteSong {
        RemoteSong::new(
            SyncedDevice {
                id: "phone".to_owned(),
                name: "Phone".to_owned(),
                platform: SyncedPlatform::Android,
                online: true,
                nearby: true,
                this: false,
            },
            RemoteZoneShown {
                zone: 7,
                name: "Headphones".to_owned(),
                song: Some("Echoes — Pink Floyd".to_owned()),
                playing: Some(501),
                first_row: 500,
                length: Some(Duration::from_secs(1)),
                rows: vec![
                    RemoteRowShown {
                        title: "Time".to_owned(),
                        ..RemoteRowShown::default()
                    },
                    RemoteRowShown {
                        title: "Echoes".to_owned(),
                        artist: "Pink Floyd".to_owned(),
                        ..RemoteRowShown::default()
                    },
                ],
                ..RemoteZoneShown::default()
            },
        )
        .expect("a song in a mirrored window")
    }

    #[gpui::test]
    fn other_devices_can_be_collapsed_and_expanded(cx: &mut gpui::TestAppContext) {
        let mut driven = Driven::open(cx, Arc::new(Library::open_in_memory().expect("a catalog")));
        driven.root.update(&mut driven.cx, |root, cx| {
            root.remote_songs = vec![song()];
            cx.notify();
        });
        driven.settle();
        assert!(driven.cx.debug_bounds("remote-devices").is_some());

        driven.click("toggle-other-devices");
        assert!(driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices").is_none());

        let handle = driven.read(|root, cx| root.remote_controls.at("toggle-other-devices", cx));
        driven.cx.update(|window, cx| handle.focus(window, cx));
        driven.cx.simulate_keystrokes("enter");
        driven.settle();
        assert!(!driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices").is_some());
    }

    #[gpui::test]
    fn devices_without_room_float_over_the_categories_until_pressed_away_and_return_once_they_fit(
        cx: &mut gpui::TestAppContext,
    ) {
        let mut driven = Driven::open(cx, Arc::new(Library::open_in_memory().expect("a catalog")));
        driven.root.update(&mut driven.cx, |root, cx| {
            root.remote_songs = vec![song()];
            cx.notify();
        });
        driven.settle();
        assert!(!driven.read(|root, _| root.remote_collapsed));
        let available = driven.read(|root, _| root.sidebar_scroll.bounds().size.height);
        driven.click("toggle-other-devices");
        let categories = driven.read(|root, _| root.sidebar_scroll.bounds().size.height);
        let cards = categories - available;
        assert!(cards > px(0.0));
        driven.click("toggle-other-devices");
        let content = driven.read(|root, _| {
            root.sidebar_scroll
                .bounds_for_item(0)
                .expect("the categories")
                .size
                .height
        });
        let height = crate::driven::TALL - f32::from(available - content) - f32::from(cards) / 2.0;
        driven
            .cx
            .simulate_resize(gpui::size(px(crate::driven::WIDE), px(height)));
        driven.settle();

        assert!(driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices").is_none());
        assert_eq!(
            driven.read(|root, _| root.sidebar_scroll.max_offset().y),
            px(0.0)
        );
        assert!(driven.bounds_of("tab-settings").bottom() <= px(height));

        driven.click("toggle-other-devices");
        driven.settle();
        driven.settle();

        assert!(!driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices-floating").is_some());
        assert!(
            driven.bounds_of("remote-devices-floating").bottom()
                <= driven.bounds_of("toggle-other-devices").top()
        );
        assert_eq!(
            driven.read(|root, _| root.sidebar_scroll.max_offset().y),
            px(0.0)
        );

        driven.click("tab-settings");
        driven.settle();

        assert!(driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices").is_none());

        driven.click("toggle-other-devices");
        driven.settle();
        assert!(!driven.read(|root, _| root.remote_collapsed));
        driven.click("toggle-other-devices");
        driven.settle();
        assert!(driven.read(|root, _| root.remote_collapsed));

        driven
            .cx
            .simulate_resize(gpui::size(px(crate::driven::WIDE), px(crate::driven::TALL)));
        driven.settle();
        driven.click("toggle-other-devices");
        driven.settle();
        driven.settle();

        assert!(!driven.read(|root, _| root.remote_collapsed));
        assert!(driven.cx.debug_bounds("remote-devices").is_some());
        assert!(driven.cx.debug_bounds("remote-devices-floating").is_none());
    }

    #[test]
    fn a_remote_song_reads_the_playing_row_relative_to_the_mirrored_window() {
        let song = song();
        assert_eq!(song.recording.title, "Echoes");
        assert_eq!(song.recording.artist, "Pink Floyd");
        let mut outside = song.zone.clone();
        outside.playing = Some(499);
        let outside = RemoteSong::new(song.device.clone(), outside).expect("a billed song");
        assert_eq!(outside.recording.title, "Echoes — Pink Floyd");
        assert!(outside.recording.artist.is_empty());
        let mut idle = song.zone;
        idle.song = None;
        assert!(RemoteSong::new(song.device, idle).is_none());
    }

    #[test]
    fn a_remote_song_finds_its_local_album_and_leaves_an_unheld_recording_unpictured() {
        let library = Library::open_in_memory().expect("a catalog");
        let folder = Folder::new();
        folder.tagged(
            "echoes.wav",
            1,
            &[
                (b"INAM", "Echoes"),
                (b"IART", "Pink Floyd"),
                (b"IPRD", "Meddle"),
            ],
        );
        Driven::scanned(&library, &folder);
        let mut held = song();
        held.read_album(&library).expect("the matched album");
        assert_eq!(held.album.as_deref(), Some("Meddle"));
        assert!(held.track.is_some());

        let mut unheld = song();
        unheld.recording.artist = "Another artist".to_owned();
        unheld.read_album(&library).expect("an unheld recording");
        assert!(unheld.album.is_none());
        assert!(unheld.track.is_none());
    }
}
