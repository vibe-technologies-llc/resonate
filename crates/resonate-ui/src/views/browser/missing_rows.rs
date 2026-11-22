use std::time::Duration;

use gpui::{Context, Div, ElementId, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::ReleaseTrackId;
use resonate_library::{Column, Found, HeldReleaseTrack, Lit, Mbid, MissingTrack, Performer};

use super::{
    ROW_CONTROLS, TRACK_ADD_CONTROLS, TRACK_CONTROLS,
    menus::{MissingOnAnAlbum, found_menu, releases_to_want},
    rows::controls_place_of,
    tracks::run_heading,
};
use crate::{
    Drawn, Selection,
    downloads::Fetching,
    format, icons,
    icons::Icon,
    motion, theme,
    views::{
        hint::Names,
        kit,
        kit::{EndsInAnEllipsis, Press},
        listing, menu,
        playlists::ROW_GROUP,
        root::{RootView, cover_rounding, framed_cover, row},
        spoken,
        spoken::{Spelt, Spoken},
    },
    words,
};

pub(super) const UNHELD_COVER: f32 = 0.45;

impl RootView {
    pub(crate) fn unheld_row(
        &self,
        index: usize,
        unheld: Unheld,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let Unheld {
            asks,
            number,
            title,
            artist,
            length,
            beside,
        } = unheld;

        let download = match (&beside, &asks) {
            (Beside::AnAlbum, Asks::Row(release_track))
                if self.library.read(cx).wanted(*release_track).is_none() =>
            {
                Some(*release_track)
            }
            _ => None,
        };
        let dismissed = match (&beside, &asks) {
            (Beside::ARun, Asks::Row(release_track)) => Some(*release_track),
            _ => None,
        };
        let id = match &asks {
            Asks::Row(release_track) => {
                ElementId::NamedInteger("unheld-row".into(), release_track.get())
            }
            Asks::Found(found) => listing::keyed_by("found", &found.recording),
        };
        let fetching = match (&beside, &asks) {
            (Beside::AnAlbum, Asks::Row(release_track)) => {
                self.library.read(cx).fetching_want(*release_track)
            }
            _ => None,
        };
        let told = match &asks {
            Asks::Row(release_track) => self.library.read(cx).told_want(*release_track),
            Asks::Found(found) => self.library.read(cx).told_found(found),
        };
        let controls = match (&beside, self.adding_songs_to.is_some()) {
            (Beside::ARun, _) => ROW_CONTROLS,
            (_, true) => TRACK_ADD_CONTROLS,
            (_, false) => TRACK_CONTROLS,
        };
        let performer = match &asks {
            Asks::Found(found) => found.performer.clone(),
            Asks::Row(_) => None,
        };
        let spun = match (&beside, &asks) {
            (
                Beside::ASearch {
                    fetching: Some(fetching),
                    ..
                },
                Asks::Found(found),
            ) if fetching.is_underway() => Some(listing::keyed_by("fetching", &found.recording)),
            _ => None,
        };
        let missing = match (&beside, &asks) {
            (Beside::AnAlbum, Asks::Row(release_track)) => {
                let library = self.library.read(cx);
                Some(MissingOnAnAlbum {
                    release_track: *release_track,
                    want: library.wanted(*release_track),
                    recording: library.recording_of(*release_track),
                })
            }
            _ => None,
        };
        let mark = self.want_mark(asks, cx);
        let fitted = self.rooms.columns_fit.shown();
        let (title, lit_title, lit_artist) = match &beside {
            Beside::AnAlbum | Beside::ARun => (title, Lit::new(), Lit::new()),
            Beside::ASearch { .. } => {
                let search = self.library.read(cx).search();
                (
                    title.clone(),
                    search.lit(&title, Column::Title),
                    search.lit(&artist, Column::Artist),
                )
            }
        };
        let said = spoken::listening().then(|| unheld_said(&title, &artist));
        let artist = listing::matched(artist, lit_artist);

        let row = row(false)
            .child(listing::number_cell(number))
            .when_some(beside.pictured(), |row, sleeve| {
                let cover = match sleeve {
                    Sleeve::Released(release) => {
                        let art = self.library.update(cx, |library, cx| {
                            library.released_cover(release, Drawn::InARow, cx)
                        });
                        match art {
                            Some(art) => {
                                framed_cover(Some(art), theme::row_cover()).opacity(UNHELD_COVER)
                            }
                            None => unheld_cover(theme::row_cover()),
                        }
                    }
                    Sleeve::Unknown => unheld_cover(theme::row_cover()),
                };
                row.child(match spun {
                    Some(spun) => fetching_over(cover, spun),
                    None => cover,
                })
            })
            .child(
                listing::title_cell(title, lit_title, false)
                    .debug_selector(move || format!("unheld-title-{index}"))
                    .text_color(rgb(theme::faint())),
            )
            .child(
                listing::artist_cell(
                    self.performed_by(("missing-artist", index), artist, performer, cx)
                        .debug_selector(move || format!("unheld-artist-{index}"))
                        .flex_shrink(1.0)
                        .ends_in_an_ellipsis(),
                )
                .text_color(rgb(theme::faint())),
            )
            .when_some(
                match beside {
                    Beside::AnAlbum => Some(listing::format_cell(None).when_some(
                        fetching,
                        |cell, fetching| {
                            cell.child(
                                div()
                                    .text_size(px(theme::text_sm()))
                                    .text_color(rgb(fetching_colour(fetching)))
                                    .truncate()
                                    .ends_in_an_ellipsis()
                                    .child(told.clone().unwrap_or_else(|| fetching.saying())),
                            )
                        },
                    )),
                    Beside::ARun => None,
                    Beside::ASearch {
                        on, fetching: None, ..
                    } => Some(
                        listing::format_cell(None).child(
                            kit::figure(on)
                                .text_color(rgb(theme::faint()))
                                .truncate()
                                .ends_in_an_ellipsis(),
                        ),
                    ),
                    Beside::ASearch {
                        fetching: Some(fetching),
                        ..
                    } => Some(
                        listing::format_cell(None).child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(fetching_colour(fetching)))
                                .truncate()
                                .ends_in_an_ellipsis()
                                .child(told.unwrap_or_else(|| fetching.saying())),
                        ),
                    ),
                },
                |row, format| {
                    row.when(fitted.format, |row| row.child(format))
                        .when(fitted.heard, |row| row.child(listing::unheard()))
                },
            )
            .child(listing::length_cell(length).text_color(rgb(theme::faint())))
            .child(
                controls_place_of(controls)
                    .gap_1()
                    .when_some(dismissed, |controls, release_track| {
                        controls.child(self.in_the_pane_ring(
                            kit::icon_button(
                                ("dismiss-missing", release_track.get()),
                                Icon::Close,
                                words::browser::dismiss_missing_hint(),
                            ),
                            move |this, _, cx| {
                                cx.stop_propagation();
                                this.library.update(cx, |library, cx| {
                                    library.dismiss_missing(release_track, cx);
                                });
                            },
                            cx,
                        ))
                    })
                    .child(mark),
            );

        let row = row.id(id);
        let row = match missing {
            Some(missing) => menu::opens_a_menu(row, move |_, at, _| missing.menu(at), cx),
            None => row,
        };

        row.when_some(said, |row, said| row.listed_as(|| said))
            .when_some(download, |row, release_track| {
                row.cursor_pointer()
                    .hover(|row| row.bg(rgb(theme::hover())))
                    .names(words::browser::want_hint())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.library
                            .update(cx, |library, cx| library.want(release_track, cx));
                    }))
            })
    }

    fn performed_by(
        &self,
        id: impl Into<ElementId>,
        label: impl IntoElement + Spelt,
        performer: Option<Performer>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        match performer {
            Some(Performer::Held(artist)) => self.opens(
                id,
                label,
                words::browser::open_artist_hint(),
                Some(Selection::Artist(artist)),
                cx,
            ),
            Some(Performer::Elsewhere(found)) => self.pressed_to(
                id,
                label,
                words::browser::open_artist_found_hint(),
                Some(move |this: &mut Self, cx: &mut Context<Self>| {
                    this.open_artist_found(found.clone(), cx);
                }),
                cx,
            ),
            None => self.opens(id, label, words::browser::open_artist_hint(), None, cx),
        }
    }

    pub(crate) fn found_row(
        &self,
        index: usize,
        found: &Found,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let fetching = self.library.read(cx).fetching_found(found);
        let row = self
            .unheld_row(index, Unheld::found(found, fetching), cx)
            .debug_selector(move || format!("found-{index}"));
        let hint = match fetching {
            None => words::browser::fetch_found_hint(),
            Some(fetching) if fetching.can_be_asked_again() => {
                words::browser::fetch_found_again_hint()
            }
            Some(_) => return row,
        };

        let fetched = found.clone();
        let offered = found.clone();
        let row = menu::opens_a_menu(row, move |_, at, _| found_menu(at, &offered), cx);

        row.cursor_pointer()
            .hover(|row| row.bg(rgb(theme::hover())))
            .names(hint)
            .on_click(cx.listener(move |this, _, _, cx| {
                let wanted = fetched.clone();
                let zone = this.zone_acted_on();

                this.library
                    .update(cx, |library, cx| library.fetch_and_play(wanted, zone, cx));
            }))
    }

    pub(super) fn songs_not_held_heading(rows: usize) -> Div {
        run_heading(SharedString::from(words::browser::songs_not_held(rows)))
    }

    fn want_mark(&self, asks: Asks, cx: &mut Context<Self>) -> Stateful<Div> {
        match asks {
            Asks::Row(release_track) => match self.library.read(cx).wanted(release_track) {
                Some(want) => self.in_the_pane_ring(
                    kit::icon_button(
                        ("unwant", release_track.get()),
                        Icon::Wanted,
                        words::browser::unwant_hint(),
                    ),
                    move |this, _, cx| {
                        cx.stop_propagation();
                        this.library
                            .update(cx, |library, cx| library.unwant(want, cx));
                    },
                    cx,
                ),
                None => self.in_the_pane_ring(
                    kit::icon_button(
                        ("want", release_track.get()),
                        Icon::Want,
                        words::browser::want_hint(),
                    ),
                    move |this, _, cx| {
                        cx.stop_propagation();
                        this.library
                            .update(cx, |library, cx| library.want(release_track, cx));
                    },
                    cx,
                ),
            },
            Asks::Found(found) => {
                let fetching = self
                    .library
                    .read(cx)
                    .fetching_found(&found)
                    .filter(|fetching| !fetching.can_be_asked_again());

                if let Some(fetching) = fetching {
                    return kit::mark_when(
                        Press::Greyed,
                        listing::keyed_by("wanting-found", &found.recording),
                        Icon::Wanted,
                        fetching.saying(),
                        ROW_GROUP,
                    );
                }

                let offered = Found::clone(&found);
                let wanting = self.in_the_pane_ring(
                    kit::icon_button(
                        listing::keyed_by("want-found", &found.recording),
                        Icon::Want,
                        words::browser::want_found_hint(),
                    ),
                    move |this, _, cx| {
                        cx.stop_propagation();

                        let wanted = Found::clone(&found);

                        this.library
                            .update(cx, |library, cx| library.want_found(wanted, cx));
                    },
                    cx,
                );
                menu::opens_a_menu(wanting, move |_, at, _| releases_to_want(at, &offered), cx)
            }
        }
    }
}

pub(crate) enum Asks {
    Row(ReleaseTrackId),
    Found(Box<Found>),
}

pub(crate) enum Beside {
    AnAlbum,
    ARun,
    ASearch {
        pictured: Sleeve,
        on: SharedString,
        fetching: Option<Fetching>,
    },
}

pub(crate) enum Sleeve {
    Released(Mbid),
    Unknown,
}

impl Beside {
    const fn pictured(&self) -> Option<&Sleeve> {
        match self {
            Self::AnAlbum | Self::ARun => None,
            Self::ASearch { pictured, .. } => Some(pictured),
        }
    }
}

pub(crate) struct Unheld {
    asks: Asks,
    number: SharedString,
    title: SharedString,
    pub(super) artist: SharedString,
    length: SharedString,
    beside: Beside,
}

impl Unheld {
    pub(super) fn of(
        release_track: ReleaseTrackId,
        number: Option<&str>,
        position: u32,
        title: &str,
        artist: Option<&str>,
        length: Option<Duration>,
    ) -> Self {
        Self {
            asks: Asks::Row(release_track),
            number: SharedString::from(
                number.map_or_else(|| position.to_string(), ToOwned::to_owned),
            ),
            title: SharedString::from(title.to_owned()),
            artist: SharedString::from(artist.unwrap_or_default().to_owned()),
            length: SharedString::from(length.map(format::spanned).unwrap_or_default()),
            beside: Beside::AnAlbum,
        }
    }

    pub(crate) fn short_of(row: &MissingTrack) -> Self {
        Self {
            beside: Beside::ARun,
            ..Self::from(row)
        }
        .credited_as(row.owner.as_deref())
    }

    pub(crate) fn credited_as(self, owner: Option<&str>) -> Self {
        match owner {
            Some(owner) if self.artist.is_empty() => Self {
                artist: SharedString::from(owner.to_owned()),
                ..self
            },
            _ => self,
        }
    }

    pub(super) fn found(found: &Found, fetching: Option<Fetching>) -> Self {
        Self {
            number: SharedString::new_static(""),
            title: SharedString::from(found.title.clone()),
            artist: SharedString::from(found.artist.clone()),
            length: SharedString::from(found.length.map(format::spanned).unwrap_or_default()),
            beside: Beside::ASearch {
                pictured: found.release.as_ref().map_or(Sleeve::Unknown, |release| {
                    Sleeve::Released(release.id.clone())
                }),
                on: SharedString::from(
                    found
                        .release
                        .as_ref()
                        .map(|release| release.title.clone())
                        .unwrap_or_default(),
                ),
                fetching,
            },
            asks: Asks::Found(Box::new(found.clone())),
        }
    }
}

pub(crate) fn fetching_colour(fetching: Fetching) -> u32 {
    match fetching {
        Fetching::Downloading { .. } => theme::accent(),
        Fetching::Downloaded => theme::done(),
        Fetching::GaveUp | Fetching::NoProvider | Fetching::Unwanted => theme::failure(),
        Fetching::Landing
        | Fetching::Queued
        | Fetching::Unreached { .. }
        | Fetching::Retrying { .. } => theme::muted(),
    }
}

fn fetching_over(cover: Div, spun: ElementId) -> Div {
    let side = theme::row_cover();

    div()
        .relative()
        .flex_none()
        .size(px(side))
        .child(cover)
        .child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(cover_rounding(side)))
                .bg(kit::cover_veil())
                .child(motion::spinning(
                    icons::icon(Icon::Spinner, side * 0.6, theme::accent()),
                    spun,
                )),
        )
}

pub(super) fn unheld_cover(side: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(side))
        .rounded(px(side / 4.0))
        .border_1()
        .border_dashed()
        .border_color(rgb(theme::border()))
        .child(icons::icon(Icon::Missing, side * 0.5, theme::faint()))
}

impl From<&HeldReleaseTrack> for Unheld {
    fn from(row: &HeldReleaseTrack) -> Self {
        Self::of(
            row.id,
            row.number.as_deref(),
            row.position,
            &row.title,
            row.artist.as_deref(),
            row.length,
        )
    }
}

impl From<&MissingTrack> for Unheld {
    fn from(row: &MissingTrack) -> Self {
        Self::of(
            row.release_track,
            row.number.as_deref(),
            row.position,
            &row.title,
            row.artist.as_deref(),
            row.length,
        )
    }
}

fn unheld_said(title: &str, artist: &str) -> SharedString {
    spoken::joined([
        spoken::song(title, Some(artist)).as_ref(),
        words::browser::not_in_the_library(),
    ])
}
