use std::{
    rc::Rc,
    time::{Duration, SystemTime},
};

use gpui::{
    AnyElement, App, Context, Corners, Div, FontWeight, MouseButton, MouseDownEvent, ObjectFit,
    Pixels, Point, ScrollWheelEvent, SharedString, Stateful, Svg, Window, div, img, prelude::*, px,
    rgb,
};
use resonate_core::{AlbumId, ArtistId, Frames, MediaLocation, QueueStamp, SampleRate, TrackId};
use resonate_engine::{
    Asleep, Command, PlaybackState, PlayerState, QueueItem, RepeatMode, StreamDigest, Until,
};
use resonate_library::{Codec, Favoured};

use crate::{
    Drawn, ResonateApp, RootView, Selection,
    app::Grain,
    format,
    icons::{self, Icon},
    theme,
    views::{
        Pane,
        browser::{OPEN_ALBUM_HINT, OPEN_ARTIST_HINT},
        hint::Names,
        kit::{self, EndsInAnEllipsis, KeepsItsWidth},
        listing::Pictured,
        menu::{self, Menu},
        slider::Handle,
    },
};

const CONTROL_GROUP: &str = "transport-control";

const PREVIOUS_HINT: &str = "Previous track — ctrl-left";

const NEXT_HINT: &str = "Next track — ctrl-right";

const PLAY_HINT: &str = "Play — space";

const PAUSE_HINT: &str = "Pause — space";

const QUEUE_HINT: &str = "Show what is queued";

const QUEUE_OPEN_HINT: &str = "Back to the pane the queue covered";

pub(crate) const COVER_HINT: &str = "See the cover full size";

const VOLUME_HINT: &str = "Mute — click. Volume — ctrl-up and ctrl-down";

const VOLUME_HINT_WHEELED: &str = "Mute — click. Volume — the wheel, ctrl-up and ctrl-down";

const VOLUME_ICON_GROUP: &str = "volume-icon";

const TITLE_GAP: f32 = 4.0;

const UNMUTE_HINT: &str = "Muted — click, the wheel or ctrl-up to hear it again";

const VOLUME_A_NOTCH: f32 = 0.05;

const PIXELS_A_NOTCH: f32 = 24.0;

const SECONDS_A_MINUTE: u64 = 60;

const SLEEP_MINUTES: [u64; 5] = [15, 30, 45, 60, 90];

const END_OF_TRACK: &str = "End of track";

const END_OF_QUEUE: &str = "End of queue";

const SLEEP_OFF: &str = "Turn the sleep timer off";

const END_OF_TRACK_READING: &str = "track";

const END_OF_QUEUE_READING: &str = "queue";

const INSPECT_HINT: &str = "See the whole signal path in the inspector";

fn notches_turned(event: &ScrollWheelEvent) -> f32 {
    f32::from(event.delta.pixel_delta(px(PIXELS_A_NOTCH)).y) / PIXELS_A_NOTCH
}

fn inspects(sink: Option<&str>) -> SharedString {
    match sink {
        Some(sink) => SharedString::from(format!("Playing through {sink} · {INSPECT_HINT}")),
        None => SharedString::new_static(INSPECT_HINT),
    }
}

const DISMISS_HINT: &str = "click or escape to dismiss";

pub(crate) struct Playing {
    pub(crate) track: Option<TrackId>,
    pub(crate) favourite: bool,
    pub(crate) title: SharedString,
    pub(crate) artist: SharedString,
    pub(crate) artist_id: Option<ArtistId>,
    album: Option<SharedString>,
    codec: Option<Codec>,
    pub(crate) cover: Cover,
    pub(crate) heard: Option<Heard>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resolving {
    track: TrackId,
    row: Option<usize>,
    queue: QueueStamp,
    reads: u64,
    catalog: u64,
    tagged: bool,
}

pub(crate) struct Resolved {
    at: Resolving,
    playing: Rc<Playing>,
}

#[derive(Clone, Copy)]
pub(crate) struct PlayingNow {
    album: Option<AlbumId>,
    artist: Option<ArtistId>,
    pub(crate) track: Option<TrackId>,
    favourite: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct Heard {
    pub(crate) plays: u32,
    pub(crate) played: Option<SystemTime>,
    pub(crate) added: SystemTime,
}

#[derive(Default)]
pub(crate) struct Cover {
    pub(crate) album: Option<AlbumId>,
    file: Option<MediaLocation>,
}

impl Cover {
    fn pictured(&self) -> Option<Pictured<'_>> {
        match (self.album, self.file.as_ref()) {
            (album, Some(file)) => Some(Pictured::Track { album, file }),
            (Some(album), None) => Some(Pictured::Album(album)),
            (None, None) => None,
        }
    }
}

impl RootView {
    pub(crate) fn seek_by(&self, seconds: i64, cx: &mut Context<Self>) {
        let Some(track) = self.player.read(cx).state().current else {
            return;
        };
        let delta = seconds.saturating_mul(i64::from(track.source.rate.hz()));
        self.send(Command::SeekBy(delta), cx);
    }

    pub(crate) fn seek_to(&self, fraction: f32, cx: &mut Context<Self>) {
        let Some(duration) = self
            .player
            .read(cx)
            .state()
            .current
            .and_then(|track| track.duration)
        else {
            return;
        };
        self.send(Command::Seek(along(duration, fraction)), cx);
    }

    pub(crate) fn seek_to_moment(&self, at: Duration, cx: &mut Context<Self>) {
        let Some(track) = self.player.read(cx).state().current else {
            return;
        };
        let wanted = Frames::from_duration(at, track.source.rate);
        let landing = match track.duration {
            Some(duration) => wanted.min(duration),
            None => wanted,
        };

        self.send(Command::Seek(landing), cx);
    }

    pub(crate) fn grain(&self, window: &Window, cx: &App) -> Grain {
        let follows_every_poll =
            matches!(self.pane, Pane::Lyrics | Pane::Visualiser | Pane::Inspector);
        if follows_every_poll || self.grabbed_fraction(Handle::Seek).is_some() {
            return Grain::EveryPoll;
        }

        let duration = self
            .player
            .read(cx)
            .state()
            .current
            .and_then(|track| Some(track.duration?.to_duration(track.source.rate)));
        Grain::across(
            f32::from(self.seek_rail.width()) * window.scale_factor(),
            duration,
        )
    }

    pub(crate) fn transport(&self, corners: Corners<Pixels>, cx: &mut Context<Self>) -> AnyElement {
        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();
        let notice = model.notice().map(ToOwned::to_owned);
        let sink = state
            .output
            .and_then(|output| model.sinks().iter().find(|sink| sink.id == output.sink))
            .map(|sink| SharedString::from(sink.description.clone()));

        let now_playing = self.now_playing_panel(&state, digest.as_deref(), sink, cx);
        let (played, duration, rate) = match state.current {
            Some(track) => (track.position, track.duration, track.source.rate),
            None => (Frames::ZERO, None, SampleRate::HZ_44100),
        };
        let position = match (self.grabbed_fraction(Handle::Seek), duration) {
            (Some(fraction), Some(duration)) => along(duration, fraction),
            _ => played,
        };
        let playing = state.playback == PlaybackState::Playing;

        div()
            .flex()
            .flex_col()
            .flex_none()
            .border_t_1()
            .border_color(rgb(theme::border()))
            .rounded_bl(corners.bottom_left)
            .rounded_br(corners.bottom_right)
            .bg(rgb(theme::surface()))
            .when_some(notice, |bar, notice| bar.child(self.trouble(notice, cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    .h(px(theme::transport_height()))
                    .px_5()
                    .child(now_playing)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_basis(px(theme::transport_centre()))
                            .flex_shrink()
                            .min_w(px(300.0))
                            .items_center()
                            .gap_1p5()
                            .child(self.controls(playing, cx))
                            .child(self.seek_bar(position, duration, rate, cx)),
                    )
                    .child(self.status(&state, cx)),
            )
            .into_any_element()
    }

    fn controls(&self, playing: bool, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .child(self.step(
                "previous",
                Icon::Previous,
                PREVIOUS_HINT,
                Command::Previous,
                cx,
            ))
            .child(self.play_button(playing, cx))
            .child(self.step("next", Icon::Next, NEXT_HINT, Command::Next, cx))
    }

    fn trouble(&self, notice: String, cx: &mut Context<Self>) -> Stateful<Div> {
        let whole = SharedString::from(format!("{notice} — {DISMISS_HINT}"));

        div()
            .id("notice")
            .flex()
            .w_full()
            .items_center()
            .gap_2()
            .px_5()
            .py_1p5()
            .cursor_pointer()
            .bg(theme::tinted(theme::failure(), 0x14))
            .hover(|strip| strip.bg(theme::tinted(theme::failure(), 0x2a)))
            .border_b_1()
            .border_color(theme::tinted(theme::failure(), 0x33))
            .names(whole)
            .child(kit::mode_dot(theme::failure()))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::failure()))
                    .truncate()
                    .child(SharedString::from(notice)),
            )
            .child(icons::icon(
                Icon::Close,
                theme::row_control_icon(),
                theme::failure(),
            ))
            .on_click(cx.listener(|this, _, _, cx| this.dismiss_notice(cx)))
    }

    fn status(&self, state: &PlayerState, cx: &mut Context<Self>) -> Div {
        let repeat = match state.repeat {
            RepeatMode::Off | RepeatMode::Queue => Icon::Repeat,
            RepeatMode::Track => Icon::RepeatTrack,
        };
        let shuffle = state.shuffle;
        let cycled = match state.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        };

        div()
            .flex()
            .flex_1()
            .flex_basis(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .justify_end()
            .gap_1()
            .child(self.queue_button(cx))
            .child(
                self.toggle("shuffle", Icon::Shuffle, shuffling(shuffle), shuffle)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(Command::SetShuffle(!shuffle), cx);
                    })),
            )
            .child(
                self.toggle(
                    "repeat",
                    repeat,
                    repeating(state.repeat),
                    state.repeat != RepeatMode::Off,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.send(Command::SetRepeat(cycled), cx);
                })),
            )
            .child(self.sleep_button(state.sleeping, cx))
            .child(self.volume_bar(state.volume.get(), cx))
    }

    fn sleep_button(&self, sleeping: Option<Asleep>, cx: &mut Context<Self>) -> Stateful<Div> {
        let colour = match sleeping {
            Some(_) => theme::accent(),
            None => theme::muted(),
        };
        let control = div()
            .id("sleep")
            .group(CONTROL_GROUP)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1p5()
            .h(px(theme::toggle_control()))
            .min_w(px(theme::toggle_control()))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::hover())))
            .child(lit(Icon::Sleep, colour, sleeping.is_some()))
            .when_some(sleeping, |button, asleep| {
                button.bg(theme::tinted(theme::accent(), 0x1c)).child(
                    kit::figure(sleep_reading(asleep))
                        .w(px(theme::sleep_reading()))
                        .text_center()
                        .text_color(rgb(theme::accent())),
                )
            })
            .names(sleeping_says(sleeping))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    let menu = sleep_menu(event.position, sleeping);
                    this.open_a_menu(menu, cx);
                }),
            );

        menu::opens_a_menu(control, move |_, at, _| sleep_menu(at, sleeping), cx)
    }

    pub(crate) fn queued_row(&self, state: &PlayerState, cx: &Context<Self>) -> Option<QueueItem> {
        let current = state.current?;
        let queue = self.player.read(cx).queue();
        let item = queue.get(state.queue_position?)?;
        (item.id == current.id).then(|| item.clone())
    }

    fn now_playing_panel(
        &self,
        state: &PlayerState,
        digest: Option<&StreamDigest>,
        sink: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Div {
        let playing = self.playing(state, digest, cx);
        let cover = self.now_playing_cover(&playing.cover, cx);
        let idle = state.current.is_none();

        div()
            .flex()
            .flex_1()
            .flex_basis(px(0.0))
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .gap_3p5()
            .child(cover)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .relative()
                    .flex_1()
                    .min_w(px(0.0))
                    .gap_0p5()
                    .child(kit::measures_its_width(self.playing_room.clone()))
                    .child(self.played_title(&playing, idle, cx))
                    .child(self.by_line("playing-artist", "playing-album", &playing, cx))
                    .when(!idle, |panel| {
                        panel.child(self.signal_path(state, &playing, sink, cx))
                    }),
            )
    }

    fn played_title(&self, playing: &Playing, idle: bool, cx: &mut Context<Self>) -> Div {
        let favouring = playing.track.map(Favoured::Track);

        div()
            .flex()
            .items_center()
            .gap(px(TITLE_GAP))
            .min_w(px(0.0))
            .text_size(px(theme::text_base()))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(if idle { theme::muted() } else { theme::text() }))
            .child(
                self.opens(
                    "playing-title",
                    kit::cut_to_fit(
                        playing.title.clone(),
                        self.playing_room.get() - px(theme::row_control() + TITLE_GAP),
                        theme::ui(FontWeight::MEDIUM),
                        px(theme::text_base()),
                        cx,
                    ),
                    OPEN_ALBUM_HINT,
                    playing.cover.album.map(Selection::Album),
                    cx,
                )
                .keeps_its_width(),
            )
            .when_some(favouring, |line, what| {
                line.child(self.favour_mark("playing-favourite", what, playing.favourite, cx))
            })
    }

    pub(crate) fn by_line(
        &self,
        of_the_artist: &'static str,
        of_the_album: &'static str,
        playing: &Playing,
        cx: &mut Context<Self>,
    ) -> Div {
        let line = div()
            .flex()
            .items_center()
            .min_w(px(0.0))
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .child(
                self.opens(
                    of_the_artist,
                    playing.artist.clone(),
                    OPEN_ARTIST_HINT,
                    playing.artist_id.map(Selection::Artist),
                    cx,
                )
                .keeps_its_width(),
            );

        let Some(album) = playing.album.clone() else {
            return line;
        };

        line.child(div().flex_none().px_1p5().child("·")).child(
            self.opens(
                of_the_album,
                album,
                OPEN_ALBUM_HINT,
                playing.cover.album.map(Selection::Album),
                cx,
            )
            .flex_1()
            .ends_in_an_ellipsis(),
        )
    }

    fn signal_path(
        &self,
        state: &PlayerState,
        playing: &Playing,
        sink: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let source = state.current.map(|track| track.source);
        let mut path = div()
            .id("signal-path")
            .flex()
            .items_center()
            .gap_2()
            .mt_0p5()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded_sm()
            .cursor_pointer()
            .hover(|path| path.bg(rgb(theme::hover())))
            .names(inspects(sink.as_ref().map(SharedString::as_ref)))
            .on_click(cx.listener(|this, _, _, cx| this.set_pane(Pane::Inspector, cx)));

        if let (Some(codec), Some(source)) = (playing.codec, source) {
            path = path.child(kit::format_badge(codec, source));
        } else if let Some(source) = source {
            path = path.child(kit::figure(format::quality(source)));
        }

        let Some(output) = state.output else {
            return path;
        };
        let (label, colour) = format::mode(output.mode);

        path.child(kit::mode_dot(colour))
            .child(kit::figure(label).text_color(rgb(colour)))
    }

    fn now_playing_cover(&self, cover: &Cover, cx: &mut Context<Self>) -> AnyElement {
        let frame = div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::now_playing_cover()))
            .rounded_md()
            .overflow_hidden()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(theme::tinted(theme::text(), 0x0c));

        let drawn = cover
            .pictured()
            .and_then(|pictured| self.drawn_cover(pictured, Drawn::NowPlaying, cx));
        let Some((art, magnified)) = drawn else {
            return frame
                .child(icons::icon(Icon::Disc, 24.0, theme::faint()))
                .into_any_element();
        };

        let opened = magnified.clone();
        let cover = frame
            .id("magnify-cover")
            .cursor_pointer()
            .hover(|cover| cover.opacity(kit::LIT))
            .child(
                img(art)
                    .size(px(theme::now_playing_cover()))
                    .object_fit(ObjectFit::Cover)
                    .rounded_md(),
            )
            .names(COVER_HINT)
            .on_click(cx.listener(move |this, event, _, cx| {
                if !menu::pressed(event) {
                    return;
                }
                this.magnify(magnified.clone(), cx);
            }));

        menu::opens_a_menu(
            cover,
            move |this, at, cx| {
                let playing = this.playing_now(cx);

                Menu::at(at)
                    .does(Icon::Albums, menu::MAGNIFY, {
                        let seen = opened.clone();

                        move |this, _, cx| this.magnify(seen.clone(), cx)
                    })
                    .does(Icon::Inspector, menu::INSPECT, |this, _, cx| {
                        this.set_pane(Pane::Inspector, cx);
                    })
                    .reaches(playing.album, playing.artist)
                    .when_some(playing.track, |menu, track| {
                        menu.favours(Favoured::Track(track), playing.favourite)
                            .shares(track)
                    })
            },
            cx,
        )
        .into_any_element()
    }

    pub(crate) fn playing_now(&self, cx: &mut Context<Self>) -> PlayingNow {
        let model = self.player.read(cx);
        let state = model.state().clone();
        let digest = model.digest();
        let playing = self.playing(&state, digest.as_deref(), cx);

        PlayingNow {
            album: playing.cover.album,
            artist: playing.artist_id,
            track: playing.track,
            favourite: playing.favourite,
        }
    }

    pub(crate) fn playing(
        &self,
        state: &PlayerState,
        digest: Option<&StreamDigest>,
        cx: &mut Context<Self>,
    ) -> Rc<Playing> {
        let Some(current) = state.current else {
            return Rc::new(nothing_playing());
        };
        let at = Resolving {
            track: current.id,
            row: state.queue_position,
            queue: state.queue_stamp,
            reads: self.player.read(cx).reads(),
            catalog: self.library.read(cx).revision(),
            tagged: digest.is_some_and(|digest| digest.track == current.id),
        };

        if let Some(resolved) = self.resolved.borrow().as_ref()
            && resolved.at == at
        {
            return Rc::clone(&resolved.playing);
        }

        let playing = Rc::new(self.resolve(state, digest, cx));
        *self.resolved.borrow_mut() = Some(Resolved {
            at,
            playing: Rc::clone(&playing),
        });

        playing
    }

    fn resolve(
        &self,
        state: &PlayerState,
        digest: Option<&StreamDigest>,
        cx: &mut Context<Self>,
    ) -> Playing {
        let Some(current) = state.current else {
            return nothing_playing();
        };

        let row = self.queued_row(state, cx);
        let file = row.as_ref().map(|item| item.location.clone());
        let tags = digest
            .filter(|digest| digest.track == current.id)
            .map(|digest| &digest.info);
        let codec = tags.map(|info| Codec::from_id(info.codec));
        let scanned = row
            .as_ref()
            .and_then(|item| self.library.update(cx, |library, _| library.track_of(item)));

        if let Some(track) = scanned {
            let album = track.album_id.and_then(|album| {
                self.library
                    .update(cx, |library, _| library.album_title(album))
            });
            let heard = Some(Heard {
                plays: track.plays,
                played: track.played,
                added: track.added,
            });
            return Playing {
                track: Some(track.id),
                favourite: track.favourite.is_some(),
                title: SharedString::from(track.title),
                artist: SharedString::from(
                    track.artist.unwrap_or_else(|| "Unknown artist".to_owned()),
                ),
                artist_id: track.artist_id,
                album: album.map(SharedString::from),
                codec: codec.or(Some(track.codec)),
                cover: Cover {
                    album: track.album_id,
                    file,
                },
                heard,
            };
        }

        let tags = tags.map(|info| &info.tags);
        let stem = file.as_ref().map(format::stem);

        Playing {
            track: None,
            favourite: false,
            title: SharedString::from(
                tags.and_then(|tags| tags.title.clone())
                    .or(stem)
                    .unwrap_or_else(|| format!("track {}", current.id)),
            ),
            artist: SharedString::from(
                tags.and_then(|tags| tags.artist.clone())
                    .unwrap_or_else(|| "Unknown artist".to_owned()),
            ),
            artist_id: None,
            album: tags
                .and_then(|tags| tags.album.clone())
                .map(SharedString::from),
            codec,
            cover: Cover { album: None, file },
            heard: None,
        }
    }

    fn volume_bar(&self, level: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let level = self.grabbed_fraction(Handle::Volume).unwrap_or(level);
        let wheeled = cx.global::<ResonateApp>().scroll_volume;
        let muted = self.muted_at(cx).is_some();
        let hint = if wheeled {
            VOLUME_HINT_WHEELED
        } else {
            VOLUME_HINT
        };

        div()
            .id("volume-bar")
            .when(wheeled, |bar| {
                bar.on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                    cx.stop_propagation();
                    this.volume_by(notches_turned(event) * VOLUME_A_NOTCH, cx);
                }))
            })
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .ml_2()
            .child(
                div()
                    .id("volume-icon")
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .group(VOLUME_ICON_GROUP)
                    .child(icons::lit_on_hover(
                        icons::icon(
                            if muted { Icon::Muted } else { Icon::Volume },
                            theme::toggle_icon(),
                            if muted {
                                theme::accent()
                            } else {
                                theme::muted()
                            },
                        ),
                        VOLUME_ICON_GROUP,
                    ))
                    .names(if muted { UNMUTE_HINT } else { hint })
                    .on_click(cx.listener(|this, event, _, cx| {
                        if !menu::pressed(event) {
                            return;
                        }
                        cx.stop_propagation();
                        this.toggle_mute(cx);
                    })),
            )
            .child(self.rail(Handle::Volume, level, cx))
            .child(
                kit::readout(if muted {
                    "muted".to_owned()
                } else {
                    format!("{:.0}%", level * 100.0)
                })
                .w(px(theme::volume_reading()))
                .text_right(),
            )
    }

    fn seek_bar(
        &self,
        position: Frames,
        duration: Option<Frames>,
        rate: SampleRate,
        cx: &mut Context<Self>,
    ) -> Div {
        let elapsed = format::clock(position, rate);
        let left = duration.map(|duration| format::clock(duration, rate));

        div()
            .flex()
            .w_full()
            .items_center()
            .gap_3()
            .child(clock(elapsed).text_right())
            .child(self.rail(Handle::Seek, format::progress(position, duration), cx))
            .child(clock(left.unwrap_or_else(|| "–:––".to_owned())))
    }

    fn step(
        &self,
        id: &'static str,
        glyph: Icon,
        saying: &'static str,
        command: Command,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .group(CONTROL_GROUP)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::transport_step()))
            .rounded_full()
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::hover())))
            .child(icons::lit_on_hover(
                icons::icon(glyph, theme::transport_step_icon(), theme::muted()),
                CONTROL_GROUP,
            ))
            .names(saying)
            .on_click(cx.listener(move |this, _, _, cx| this.send(command.clone(), cx)))
    }

    fn play_button(&self, playing: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        let glyph = if playing { Icon::Pause } else { Icon::Play };

        div()
            .id("play")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::transport_play()))
            .rounded_full()
            .bg(rgb(theme::text()))
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::accent())))
            .child(icons::icon(
                glyph,
                theme::transport_play_icon(),
                theme::background(),
            ))
            .names(if playing { PAUSE_HINT } else { PLAY_HINT })
            .on_click(cx.listener(|this, _, _, cx| this.send(Command::TogglePlayPause, cx)))
    }

    fn queue_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.pane == Pane::Queue;

        self.toggle(
            "queue",
            Icon::Queue,
            if open { QUEUE_OPEN_HINT } else { QUEUE_HINT },
            open,
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_queue(cx)))
    }

    fn toggle(
        &self,
        id: &'static str,
        glyph: Icon,
        saying: &'static str,
        active: bool,
    ) -> Stateful<Div> {
        let colour = if active {
            theme::accent()
        } else {
            theme::muted()
        };

        div()
            .id(id)
            .group(CONTROL_GROUP)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(px(theme::toggle_control()))
            .rounded_md()
            .cursor_pointer()
            .when(active, |button| {
                button.bg(theme::tinted(theme::accent(), 0x1c))
            })
            .hover(|button| button.bg(rgb(theme::hover())))
            .child(lit(glyph, colour, active))
            .names(saying)
    }
}

fn sleep_menu(at: Point<Pixels>, sleeping: Option<Asleep>) -> Menu {
    let mut menu = Menu::at(at);
    for minutes in SLEEP_MINUTES {
        menu = menu.does(
            Icon::Sleep,
            format::counted(minutes as usize, "minute", "minutes"),
            move |this, _, cx| {
                this.send(
                    Command::SleepUntil(Some(Until::After(Duration::from_secs(
                        minutes * SECONDS_A_MINUTE,
                    )))),
                    cx,
                );
            },
        );
    }

    menu = menu
        .apart()
        .does(Icon::Tracks, END_OF_TRACK, |this, _, cx| {
            this.send(Command::SleepUntil(Some(Until::EndOfTrack)), cx);
        })
        .does(Icon::Queue, END_OF_QUEUE, |this, _, cx| {
            this.send(Command::SleepUntil(Some(Until::EndOfQueue)), cx);
        });

    if sleeping.is_none() {
        return menu;
    }

    menu.apart().does(Icon::Close, SLEEP_OFF, |this, _, cx| {
        this.send(Command::SleepUntil(None), cx);
    })
}

fn sleep_reading(asleep: Asleep) -> SharedString {
    match asleep.until {
        Until::After(_) => {
            SharedString::from(format::counting_down(asleep.left.unwrap_or_default()))
        }
        Until::EndOfTrack => SharedString::new_static(END_OF_TRACK_READING),
        Until::EndOfQueue => SharedString::new_static(END_OF_QUEUE_READING),
    }
}

const fn sleeping_says(sleeping: Option<Asleep>) -> &'static str {
    match sleeping {
        None => "Set a sleep timer",
        Some(Asleep {
            until: Until::After(_),
            ..
        }) => "A sleep timer is running — press to change it or turn it off",
        Some(Asleep {
            until: Until::EndOfTrack,
            ..
        }) => "Playing stops at the end of this track — press to change it or turn it off",
        Some(Asleep {
            until: Until::EndOfQueue,
            ..
        }) => "Playing stops at the end of the queue — press to change it or turn it off",
    }
}

const fn shuffling(shuffle: bool) -> &'static str {
    if shuffle {
        "Shuffle is on — h plays the queue in its load order again"
    } else {
        "Shuffle the play order — h"
    }
}

const fn repeating(repeat: RepeatMode) -> &'static str {
    match repeat {
        RepeatMode::Off => "Repeat is off — r repeats the queue",
        RepeatMode::Queue => "Repeating the queue — r repeats the track",
        RepeatMode::Track => "Repeating the track — r turns repeat off",
    }
}

fn lit(glyph: Icon, colour: u32, active: bool) -> Svg {
    let drawn = icons::icon(glyph, theme::toggle_icon(), colour);
    if active {
        drawn
    } else {
        icons::lit_on_hover(drawn, CONTROL_GROUP)
    }
}

fn clock(reading: String) -> Div {
    kit::figure(reading)
        .flex_none()
        .w(px(theme::clock_width()))
        .text_color(rgb(theme::muted()))
}

fn along(duration: Frames, fraction: f32) -> Frames {
    Frames((duration.get() as f32 * fraction) as u64)
}

fn nothing_playing() -> Playing {
    Playing {
        track: None,
        favourite: false,
        title: SharedString::new_static("Nothing playing"),
        artist: SharedString::new_static("Pick a track, or press play on an album"),
        artist_id: None,
        album: None,
        codec: None,
        cover: Cover::default(),
        heard: None,
    }
}
