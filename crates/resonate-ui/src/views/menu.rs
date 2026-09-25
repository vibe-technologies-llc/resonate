use std::{path::Path, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Stateful, Window, anchored, deferred, div, hsla, point, prelude::*, px, rgb,
};
use resonate_core::{AlbumId, ArtistId, MediaLocation, TrackId};
use resonate_engine::Placement;
use resonate_library::{Favoured, PlaylistEntry, Track};

use crate::{
    Selection, clipboard, format,
    icons::{self, Icon},
    theme,
    views::{kit, playlists::Held, root::RootView},
};

const MENU_GROUP: &str = "menu-entry";

type Doing = dyn Fn(&mut RootView, &mut Window, &mut Context<RootView>);

#[derive(Clone)]
pub(crate) struct Menu {
    at: Point<Pixels>,
    entries: Vec<Entry>,
    reached: Option<usize>,
}

#[derive(Clone)]
struct Entry {
    icon: Option<Icon>,
    label: SharedString,
    key: Option<SharedString>,
    does: Option<Rc<Doing>>,
}

impl Menu {
    pub(crate) fn at(at: Point<Pixels>) -> Self {
        Self {
            at,
            entries: Vec::new(),
            reached: None,
        }
    }

    pub(crate) fn does(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        does: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        self.entries.push(Entry {
            icon: Some(icon),
            label: label.into(),
            key: None,
            does: Some(Rc::new(does)),
        });
        self
    }

    pub(crate) fn under(
        mut self,
        icon: Icon,
        label: impl Into<SharedString>,
        key: &'static str,
        does: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + 'static,
    ) -> Self {
        self = self.does(icon, label, does);
        if let Some(last) = self.entries.last_mut() {
            last.key = Some(SharedString::new_static(key));
        }
        self
    }

    pub(crate) fn when_some<T>(self, held: Option<T>, then: impl FnOnce(Self, T) -> Self) -> Self {
        match held {
            Some(held) => then(self, held),
            None => self,
        }
    }

    pub(crate) fn apart(mut self) -> Self {
        if self.entries.last().is_some_and(|last| last.does.is_some()) {
            self.entries.push(Entry {
                icon: None,
                label: SharedString::new_static(""),
                key: None,
                does: None,
            });
        }
        self
    }

    pub(crate) fn offers_anything(&self) -> bool {
        self.entries.iter().any(|entry| entry.does.is_some())
    }

    fn steps(&mut self, step: isize) {
        let taking: Vec<usize> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.does.is_some())
            .map(|(at, _)| at)
            .collect();
        let Some(last) = taking.len().checked_sub(1) else {
            return;
        };
        let at = self
            .reached
            .and_then(|reached| taking.iter().position(|entry| *entry == reached));
        let next = match (at, step) {
            (None, 1) => 0,
            (None, _) => last,
            (Some(at), 1) if at == last => 0,
            (Some(at), 1) => at + 1,
            (Some(0), _) => last,
            (Some(at), _) => at - 1,
        };

        self.reached = taking.get(next).copied();
    }

    fn reached(&self) -> Option<Rc<Doing>> {
        self.entries
            .get(self.reached?)
            .and_then(|entry| entry.does.clone())
    }
}

impl RootView {
    pub(crate) fn open_a_menu(&mut self, menu: Menu, cx: &mut Context<Self>) {
        if !menu.offers_anything() {
            return;
        }
        self.naming = None;
        self.adding = None;
        self.menu = Some(menu);
        cx.notify();
    }

    pub(crate) fn close_the_menu(&mut self, cx: &mut Context<Self>) -> bool {
        let opened = self.menu.take().is_some();
        if opened {
            cx.notify();
        }
        opened
    }

    pub(crate) fn step_the_menu(&mut self, step: isize, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        menu.steps(step);
        cx.notify();
        true
    }

    pub(crate) fn press_the_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.menu.as_ref() else {
            return false;
        };
        let Some(does) = menu.reached() else {
            return self.close_the_menu(cx);
        };
        self.menu = None;
        does(self, window, cx);
        cx.notify();
        true
    }

    pub(crate) fn menu_over_the_app(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.clone()?;
        let mut panel = div()
            .id("menu")
            .flex()
            .flex_col()
            .w(theme::width(theme::menu_width()))
            .p_1()
            .gap_0p5()
            .rounded_lg()
            .bg(rgb(theme::raised()))
            .border_1()
            .border_color(rgb(theme::outline()))
            .shadow(vec![BoxShadow {
                color: hsla(0.0, 0.0, 0.0, 0.5),
                offset: point(px(0.0), px(6.0)),
                blur_radius: px(24.0),
                spread_radius: px(0.0),
            }])
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());

        for (at, entry) in menu.entries.iter().enumerate() {
            panel = panel.child(match entry.does.clone() {
                Some(does) => self.menu_entry(at, entry, &does, menu.reached == Some(at), cx),
                None => apart().into_any_element(),
            });
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.close_the_menu(cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.close_the_menu(cx);
                    }),
                )
                .child(
                    deferred(
                        anchored()
                            .position(menu.at)
                            .snap_to_window_with_margin(px(8.0))
                            .child(panel),
                    )
                    .with_priority(1),
                )
                .into_any_element(),
        )
    }

    fn menu_entry(
        &self,
        at: usize,
        entry: &Entry,
        does: &Rc<Doing>,
        reached: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let pressed = Rc::clone(does);

        div()
            .id(("menu-entry", at))
            .group(MENU_GROUP)
            .flex()
            .items_center()
            .gap_2p5()
            .h(px(theme::menu_row()))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .when(reached, |row| {
                row.bg(rgb(theme::hover())).text_color(rgb(theme::text()))
            })
            .hover(|row| row.bg(rgb(theme::hover())).text_color(rgb(theme::text())))
            .when_some(entry.icon, |row, icon| {
                row.child(icons::lit_on_hover(
                    icons::icon(icon, theme::menu_icon(), theme::faint()),
                    MENU_GROUP,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .child(entry.label.clone()),
            )
            .when_some(entry.key.clone(), |row, key| {
                row.child(kit::figure(key).text_color(rgb(theme::faint())))
            })
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.menu = None;
                pressed(this, window, cx);
                cx.notify();
            }))
            .into_any_element()
    }
}

fn apart() -> Div {
    div().my_1().mx_2().h(px(1.0)).bg(rgb(theme::border()))
}

pub(crate) fn opens_a_menu(
    element: Stateful<Div>,
    offers: impl Fn(&mut RootView, Point<Pixels>, &mut Context<RootView>) -> Menu + 'static,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    element.on_mouse_down(
        MouseButton::Right,
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            let menu = offers(this, event.position, cx);
            this.open_a_menu(menu, cx);
        }),
    )
}

pub(crate) const PLAY: &str = "Play";
pub(crate) const PLAY_NEXT: &str = "Play next";
pub(crate) const ADD_TO_QUEUE: &str = "Add to queue";
pub(crate) const ADD_TO_PLAYLIST: &str = "Add to playlist…";
pub(crate) const GO_TO_ARTIST: &str = "Go to artist";
pub(crate) const GO_TO_ALBUM: &str = "Go to album";
pub(crate) const COPY_PATH: &str = "Copy the file path";
pub(crate) const SHOW_IN_FOLDER: &str = "Show in the file manager";
pub(crate) const TAKE_OUT: &str = "Take out of the queue";
pub(crate) const REMOVE_ROW: &str = "Take out of the playlist";
pub(crate) const MAGNIFY: &str = "See the cover whole";
pub(crate) const INSPECT: &str = "Show in the inspector";
pub(crate) const FORGET_DELIVERY: &str = "Forget this delivery";
pub(crate) const OPEN_ALBUM: &str = "Show this album";
pub(crate) const OPEN_ARTIST: &str = "Show this artist";
pub(crate) const FAVOUR: &str = "Add to favourites";
pub(crate) const UNFAVOUR: &str = "Take out of favourites";
pub(crate) const SHARE: &str = "Share";
pub(crate) const PIN: &str = "Pin to the top";
pub(crate) const UNPIN: &str = "Stop pinning";
pub(crate) const RENAME: &str = "Rename";
pub(crate) const EXPORT: &str = "Export";
pub(crate) const DISCARD: &str = "Discard";

impl Menu {
    pub(crate) fn queues(self, rows: impl Fn() -> Arc<[PlaylistEntry]> + Clone + 'static) -> Self {
        let next = rows.clone();
        let last = rows.clone();

        self.under(Icon::Play, PLAY, "enter", move |this, _, cx| {
            this.play_entries(&rows(), cx);
        })
        .does(Icon::QueueNext, PLAY_NEXT, move |this, _, cx| {
            this.queue(&next(), Placement::Next, cx);
        })
        .does(Icon::QueueLast, ADD_TO_QUEUE, move |this, _, cx| {
            this.queue(&last(), Placement::Queued, cx);
        })
    }

    pub(crate) fn holds(self, rows: impl Fn() -> Held + 'static) -> Self {
        self.does(Icon::Plus, ADD_TO_PLAYLIST, move |this, window, cx| {
            this.hold_for_a_playlist(rows(), window, cx);
        })
    }

    pub(crate) fn reaches(self, album: Option<AlbumId>, artist: Option<ArtistId>) -> Self {
        if album.is_none() && artist.is_none() {
            return self;
        }

        let mut menu = self.apart();
        if let Some(artist) = artist {
            menu = menu.does(Icon::Artists, GO_TO_ARTIST, move |this, _, cx| {
                this.opened(Selection::Artist(artist), cx);
            });
        }
        if let Some(album) = album {
            menu = menu.does(Icon::Albums, GO_TO_ALBUM, move |this, _, cx| {
                this.opened(Selection::Album(album), cx);
            });
        }

        menu
    }

    pub(crate) fn favours(self, what: Favoured, already: bool) -> Self {
        let (icon, label) = match already {
            true => (Icon::Favourited, UNFAVOUR),
            false => (Icon::Favourite, FAVOUR),
        };

        self.apart().does(icon, label, move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.favour(what, !already, cx));
        })
    }

    pub(crate) fn shares(self, track: TrackId) -> Self {
        self.does(Icon::Share, SHARE, move |this, _, cx| {
            this.library
                .update(cx, |library, cx| library.share(track, cx));
        })
    }

    pub(crate) fn offers_the_other_copies(self, (own, copies): (Track, Vec<Track>)) -> Self {
        let mut menu = self.apart();
        let labels = copy_labels(&own, &copies);
        for (copy, label) in copies.into_iter().zip(labels) {
            let playing: Arc<[Track]> = Arc::from([copy]);
            menu = menu.does(Icon::Play, label, move |this, _, cx| {
                this.play(&playing, 0, cx);
            });
        }
        menu
    }

    pub(crate) fn offers_the_file(self, location: MediaLocation) -> Self {
        let Some(path) = location.locator().as_path() else {
            return self;
        };
        let written = path.to_string_lossy().into_owned();
        let shown = path.to_path_buf();

        self.apart()
            .does(Icon::Folder, SHOW_IN_FOLDER, move |_, _, cx| {
                cx.reveal_path(&shown);
            })
            .does(Icon::Rename, COPY_PATH, move |_, _, cx| {
                clipboard::copy(written.clone(), cx);
            })
    }
}

fn kind_of(copy: &Track) -> String {
    format!("{} {}", copy.codec.as_str(), format::quality(copy.spec))
}

fn where_it_is(copy: &Track) -> Option<String> {
    let path = copy.location.locator().as_path()?;
    let file = path.file_name()?.to_string_lossy();
    Some(match path.parent().and_then(Path::file_name) {
        Some(folder) => format!("{}/{file}", folder.to_string_lossy()),
        None => file.into_owned(),
    })
}

fn copy_labels(own: &Track, copies: &[Track]) -> Vec<String> {
    let kinds: Vec<String> = copies.iter().map(kind_of).collect();
    let own_kind = kind_of(own);

    copies
        .iter()
        .zip(&kinds)
        .map(|(copy, kind)| {
            let shared =
                *kind == own_kind || kinds.iter().filter(|other| *other == kind).count() > 1;
            match where_it_is(copy).filter(|_| shared) {
                Some(place) => format!("Play the {kind} copy in {place}"),
                None => format!("Play the {kind} copy"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};
    use resonate_library::Codec;

    use super::*;

    fn copy(id: u64, path: &str, codec: Codec, format: SampleFormat) -> Track {
        Track {
            id: TrackId::new(id).expect("a valid id"),
            location: MediaLocation::local(path),
            title: "Echoes".to_owned(),
            artist: None,
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: None,
            spec: StreamSpec::new(SampleRate::HZ_44100, ChannelLayout::Stereo, format),
            codec,
            replay_gain: Default::default(),
            file_size: 0,
            modified: SystemTime::UNIX_EPOCH,
            added: SystemTime::UNIX_EPOCH,
            plays: 0,
            played: None,
            span: None,
            favourite: None,
            genre: None,
            hidden: false,
            alternatives: 0,
            delivered: false,
        }
    }

    #[test]
    fn a_copy_of_a_kind_nothing_else_shares_is_named_by_its_kind_alone() {
        let own = copy(
            1,
            "/music/hires/06 Echoes.flac",
            Codec::Flac,
            SampleFormat::S24,
        );
        let other = copy(
            2,
            "/music/cd/06 Echoes.flac",
            Codec::Flac,
            SampleFormat::S16,
        );

        let labels = copy_labels(&own, std::slice::from_ref(&other));
        assert_eq!(labels, vec![format!("Play the {} copy", kind_of(&other))]);
    }

    #[test]
    fn copies_of_one_kind_are_told_apart_by_where_they_are() {
        let own = copy(1, "/music/a/06 Echoes.flac", Codec::Flac, SampleFormat::S16);
        let twin = copy(2, "/music/b/06 Echoes.flac", Codec::Flac, SampleFormat::S16);
        let lossy = copy(3, "/music/c/06 Echoes.mp3", Codec::Mp3, SampleFormat::F32);
        let lossy_again = copy(4, "/music/d/06 Echoes.mp3", Codec::Mp3, SampleFormat::F32);

        let labels = copy_labels(&own, &[twin.clone(), lossy.clone(), lossy_again.clone()]);
        assert_eq!(
            labels,
            vec![
                format!("Play the {} copy in b/06 Echoes.flac", kind_of(&twin)),
                format!("Play the {} copy in c/06 Echoes.mp3", kind_of(&lossy)),
                format!("Play the {} copy in d/06 Echoes.mp3", kind_of(&lossy_again)),
            ]
        );
    }
}
