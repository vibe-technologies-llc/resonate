use std::{sync::Arc, time::Duration};

use gpui::{
    Context, Div, FontWeight, Role, SharedString, Stateful, Window, div, prelude::*, px, rgb,
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
        let room = (window.viewport_size().height
            - px(theme::header_height() + theme::transport_height()))
            * 0.45;
        let cards = self
            .remote_songs
            .iter()
            .map(|song| self.remote_card(song, cx));
        Some(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .py_2()
                .child(kit::eyebrow("Other devices").px_1())
                .child(
                    div()
                        .relative()
                        .child(
                            div()
                                .id("remote-devices")
                                .debug_selector(|| "remote-devices".to_owned())
                                .flex()
                                .flex_col()
                                .gap_2()
                                .max_h(room.min(px(theme::sidebar_width() * 2.0)))
                                .overflow_y_scroll()
                                .track_scroll(&self.remote_scroll)
                                .children(cards),
                        )
                        .child(
                            Scrollbars::of(cx)
                                .vertical("remote-devices-scrollbar", self.remote_scroll.clone()),
                        ),
                ),
        )
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
        let mut controls = div().flex().items_center().justify_center().gap_2();
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
                    .size(px(theme::button_height()))
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
            .gap_2p5()
            .p_3()
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
                    .child(remote_line(&device.name, theme::muted()).flex_1().min_w_0()),
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
                    .gap_1()
                    .child(
                        remote_line(&format!("{state} · {}", song.zone.name), colour)
                            .flex_1()
                            .min_w_0(),
                    )
                    .child(kit::figure(place).text_size(px(theme::text_xs()))),
            )
            .child(controls)
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
