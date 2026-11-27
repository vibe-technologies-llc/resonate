use std::{ops::Range, sync::Arc};

use gpui::{
    AnyElement, App, Context, Div, Entity, FontWeight, SharedString, Stateful, Subscription,
    UniformListScrollHandle, div, prelude::*, px, rgb, uniform_list,
};
use resonate_library::{Missing, MissingTrack, UnheldRelease};

use crate::{
    LibraryModel, MissingRow, Portrayed, Selection, format,
    icons::Icon,
    models::{missing_track_rows, renewed, unheld_release_rows},
    theme,
    views::{
        browser::{Unheld, controls_place, portrait_frame, year_of},
        kit::{self, EndsInAnEllipsis as _, KeepsItsWidth, Tone},
        listing::{self, Pictured},
        reorder::{self, Listed, Shift},
        root::{RootView, empty, row},
        scrollbar::Scrollbars,
        spoken::{self, Spoken as _},
    },
    words,
};

const HALF_BETWEEN_CARDS: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Head,
    Within,
    Last,
}

impl Place {
    fn of(rows: &[MissingRow], index: usize) -> Self {
        let heads = |row: &MissingRow| matches!(row, MissingRow::Album(_) | MissingRow::Artist(_));
        match (rows.get(index), rows.get(index + 1)) {
            (Some(row), _) if heads(row) => Self::Head,
            (_, Some(next)) if !heads(next) => Self::Within,
            _ => Self::Last,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum MissingShows {
    #[default]
    Tracks,
    Releases,
}

impl MissingShows {
    const fn within(self, tracks: usize, releases: usize) -> Self {
        match (self, tracks, releases) {
            (Self::Tracks, 0, 1..) => Self::Releases,
            (Self::Releases, 1.., 0) => Self::Tracks,
            (shows, _, _) => shows,
        }
    }
}

pub(crate) struct MissingPane {
    shows: MissingShows,
    rows: UniformListScrollHandle,
    tracks: Arc<[MissingTrack]>,
    releases: Arc<[UnheldRelease]>,
    counted: Missing,
    dismissed: Missing,
    track_rows: Arc<[MissingRow]>,
    release_rows: Arc<[MissingRow]>,
    _landing: Subscription,
}

impl MissingPane {
    pub(crate) fn new(library: &Entity<LibraryModel>, cx: &mut Context<Self>) -> Self {
        let mut pane = Self {
            shows: MissingShows::default(),
            rows: UniformListScrollHandle::default(),
            tracks: Arc::default(),
            releases: Arc::default(),
            counted: Missing::default(),
            dismissed: Missing::default(),
            track_rows: Arc::default(),
            release_rows: Arc::default(),
            _landing: cx.observe(library, |pane, library, cx| {
                pane.take_what_landed(&library, cx);
            }),
        };

        pane.take_what_landed(library, cx);

        pane
    }

    fn take_what_landed(&mut self, library: &Entity<LibraryModel>, cx: &mut Context<Self>) {
        let Some(short) = library.update(cx, |library, _| library.take_what_is_short()) else {
            return;
        };

        if renewed(&mut self.tracks, short.tracks) {
            self.track_rows =
                missing_track_rows(self.tracks.iter().map(|track| (track.album, track.disc)))
                    .into();
        }
        if renewed(&mut self.releases, short.releases) {
            self.release_rows =
                unheld_release_rows(self.releases.iter().map(|release| release.artist)).into();
        }
        self.counted = short.missing;
        self.dismissed = short.dismissed;
        cx.notify();
    }

    fn shown(&self) -> MissingShows {
        self.shows.within(self.tracks.len(), self.releases.len())
    }

    pub(crate) fn rows_shown(&self) -> Arc<[MissingRow]> {
        match self.shown() {
            MissingShows::Tracks => Arc::clone(&self.track_rows),
            MissingShows::Releases => Arc::clone(&self.release_rows),
        }
    }

    pub(crate) const fn counted(&self) -> Missing {
        self.counted
    }

    pub(crate) fn rows(&self) -> UniformListScrollHandle {
        self.rows.clone()
    }
}

impl RootView {
    pub(crate) fn missing_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let pane = self.missing.read(cx);
        let counted = pane.counted;
        let dismissed = pane.dismissed;
        let track_rows = Arc::clone(&pane.track_rows);
        let release_rows = Arc::clone(&pane.release_rows);
        let tracks = Arc::clone(&pane.tracks);
        let releases = Arc::clone(&pane.releases);
        let shows = pane.shown();
        let scroll = pane.rows.clone();
        let library = self.library.read(cx);
        let can_enrich = library.can_enrich();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();

        let both = !tracks.is_empty() && !releases.is_empty();
        let summary = match shows {
            MissingShows::Tracks => words::missing::TracksMissingFromAlbums {
                albums: headings(&track_rows, |row| matches!(row, MissingRow::Album(_))),
                tracks: counted.tracks,
            }
            .said(),
            MissingShows::Releases => words::missing::ReleasesByArtistsNotHeld {
                artists: headings(&release_rows, |row| matches!(row, MissingRow::Artist(_))),
                releases: counted.releases,
            }
            .said(),
        };
        let cut_short = match shows {
            MissingShows::Tracks => listed_short(tracks.len(), counted.tracks, shows),
            MissingShows::Releases => listed_short(releases.len(), counted.releases, shows),
        };
        let rows = match shows {
            MissingShows::Tracks => track_rows,
            MissingShows::Releases => release_rows,
        };
        let nothing = rows.is_empty();
        let bring_back = self.bring_back(dismissed, cx);

        let heading = kit::heading()
            .child(
                kit::heading_row()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w(px(theme::heading_name()))
                            .gap_1()
                            .child(kit::title(words::missing::missing()))
                            .when(!nothing, |column| column.child(kit::subtitle(summary)))
                            .when_some(cut_short, |column, said| {
                                column.child(
                                    kit::subtitle(said)
                                        .text_size(px(theme::text_xs()))
                                        .text_color(rgb(theme::faint())),
                                )
                            }),
                    )
                    .when_some(bring_back, |heading, offered| {
                        heading.child(kit::actions().child(offered))
                    }),
            )
            .when(both, |heading| {
                heading.child(self.missing_tabs(
                    shows,
                    counted.tracks as usize,
                    counted.releases as usize,
                    cx,
                ))
            })
            .when(!reads.is_empty(), |heading| {
                heading.child(listing::reads(&reads))
            });

        kit::pane()
            .child(heading)
            .when(nothing, |pane| {
                pane.child(if narrowed {
                    empty(
                        Icon::Missing,
                        words::missing::nothing_matches(),
                        Some(words::missing::nothing_short_matches()),
                    )
                } else {
                    empty(
                        Icon::Missing,
                        words::missing::nothing_missing(),
                        Some(if can_enrich {
                            words::missing::look_it_up()
                        } else {
                            words::missing::whole()
                        }),
                    )
                })
            })
            .when(!nothing, |pane| {
                pane.child(
                    Scrollbars::of(cx).around(
                        "missing-scrollbar",
                        scroll.clone(),
                        uniform_list(
                            match shows {
                                MissingShows::Tracks => "missing-tracks",
                                MissingShows::Releases => "unheld-releases",
                            },
                            rows.len(),
                            cx.processor(move |this, range: Range<usize>, _, cx| {
                                let mut drawn = Vec::new();

                                for index in range {
                                    let listed = match rows.get(index).copied() {
                                        Some(MissingRow::Album(first)) => {
                                            tracks.get(first).map(|track| {
                                                this.album_heading(first, track, &tracks, cx)
                                                    .id(("missing-row", index))
                                                    .listed_as(|| album_said(first, track, &tracks))
                                            })
                                        }
                                        Some(MissingRow::Disc(first)) => {
                                            tracks.get(first).map(|track| {
                                                disc_heading(track.disc)
                                                    .id(("missing-row", index))
                                                    .listed_as(|| disc_said(track.disc))
                                            })
                                        }
                                        Some(MissingRow::Track(track)) => {
                                            tracks.get(track).map(|row| {
                                                this.unheld_row(track, Unheld::short_of(row), cx)
                                            })
                                        }
                                        Some(MissingRow::Artist(first)) => {
                                            releases.get(first).map(|release| {
                                                this.artist_heading(first, release, &releases, cx)
                                                    .id(("missing-row", index))
                                                    .listed_as(|| {
                                                        artist_said(first, release, &releases)
                                                    })
                                            })
                                        }
                                        Some(MissingRow::Release(release)) => {
                                            releases.get(release).map(|held| {
                                                this.release_row(release, held, cx)
                                                    .id(("missing-row", index))
                                                    .listed_as(|| release_said(held))
                                            })
                                        }
                                        None => None,
                                    };
                                    if let Some(listed) = listed {
                                        let reached = this.reaches(
                                            Shift::Listing(Listed::Missing),
                                            index,
                                            cx,
                                        );

                                        drawn.push(
                                            in_a_card(
                                                reorder::marked(listed, reached),
                                                Place::of(&rows, index),
                                            )
                                            .into_any_element(),
                                        );
                                    }
                                }
                                drawn
                            }),
                        )
                        .track_scroll(&scroll)
                        .h_full()
                        .w_full()
                        .pt_1p5()
                        .pb_4(),
                    ),
                )
            })
            .into_any_element()
    }

    pub(crate) fn show_what_is_missing(&mut self, shows: MissingShows, cx: &mut Context<Self>) {
        if self.missing.read(cx).shows != shows {
            self.let_go_of_the_reach_in(Shift::Listing(Listed::Missing), cx);
        }
        self.missing.update(cx, |pane, cx| {
            pane.shows = shows;
            cx.notify();
        });
        cx.notify();
    }

    pub(crate) fn missing_rows_shown(&self, cx: &App) -> Arc<[MissingRow]> {
        self.missing.read(cx).rows_shown()
    }

    pub(crate) fn open_what_is_missing_at(&mut self, index: usize, cx: &mut Context<Self>) {
        let pane = self.missing.read(cx);
        let opened = match pane.rows_shown().get(index).copied() {
            Some(MissingRow::Album(at) | MissingRow::Track(at)) => pane
                .tracks
                .get(at)
                .map(|track| Selection::Album(track.album)),
            Some(MissingRow::Artist(at) | MissingRow::Release(at)) => pane
                .releases
                .get(at)
                .map(|release| Selection::Artist(release.artist)),
            Some(MissingRow::Disc(_)) | None => None,
        };
        if let Some(opened) = opened {
            self.opened(opened, cx);
        }
    }

    fn bring_back(&self, dismissed: Missing, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let many = dismissed.tracks + dismissed.releases;
        (many > 0).then(|| {
            self.in_the_pane_ring(
                kit::button(
                    "bring-back-dismissed",
                    Some(Icon::Undo),
                    words::missing::bring_back(many),
                    words::missing::bring_back_hint(),
                    Tone::Ghost,
                ),
                |this, _, cx| {
                    this.library
                        .update(cx, |library, cx| library.bring_back_dismissed(cx));
                },
                cx,
            )
        })
    }

    fn release_row(&self, index: usize, release: &UnheldRelease, cx: &mut Context<Self>) -> Div {
        let year = release
            .first_released
            .as_deref()
            .map(year_of)
            .unwrap_or_default()
            .to_owned();
        let artist = release.artist;
        let mbid = release.mbid.clone();

        row(false)
            .child(listing::number_cell(SharedString::new_static("")))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .truncate()
                    .ends_in_an_ellipsis()
                    .text_color(rgb(theme::muted()))
                    .child(release.title.clone()),
            )
            .when_some(release.kind.clone(), |row, kind| {
                row.child(kit::badge(kind, theme::muted()))
            })
            .child(listing::length_cell(SharedString::from(year)).text_color(rgb(theme::faint())))
            .child(controls_place().child(self.in_the_pane_ring(
                kit::icon_button(
                    ("dismiss-release", index),
                    Icon::Close,
                    words::missing::dismiss_release_hint(),
                ),
                move |this, _, cx| {
                    let release = mbid.clone();

                    this.library.update(cx, |library, cx| {
                        library.dismiss_release(artist, release, cx);
                    });
                },
                cx,
            )))
    }

    fn missing_tabs(
        &self,
        shows: MissingShows,
        tracks: usize,
        releases: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let tab =
            |shown: MissingShows, label: &'static str, count: usize, cx: &mut Context<Self>| {
                self.in_the_pane_ring(
                    kit::segment(("missing-shows", shown as usize), label, shows == shown)
                        .gap_2()
                        .child(kit::figure(format::count(count)).text_color(rgb(theme::faint()))),
                    move |this, _, cx| this.show_what_is_missing(shown, cx),
                    cx,
                )
            };

        div().flex().pt_1().child(
            kit::segmented()
                .child(tab(
                    MissingShows::Tracks,
                    words::missing::tracks(),
                    tracks,
                    cx,
                ))
                .child(tab(
                    MissingShows::Releases,
                    words::missing::releases(),
                    releases,
                    cx,
                )),
        )
    }

    fn album_heading(
        &self,
        index: usize,
        track: &MissingTrack,
        tracks: &[MissingTrack],
        cx: &mut Context<Self>,
    ) -> Div {
        run_band(
            self.cover(Pictured::Album(track.album), cx),
            self.opens(
                ("missing-album", index),
                SharedString::from(track.album_title.clone()),
                words::browser::open_album_hint(),
                Some(Selection::Album(track.album)),
                cx,
            )
            .keeps_its_width(),
            track.owner.clone().map(SharedString::from),
            missing_from_album(index, track, tracks),
        )
    }

    fn artist_heading(
        &self,
        index: usize,
        release: &UnheldRelease,
        releases: &[UnheldRelease],
        cx: &mut Context<Self>,
    ) -> Div {
        let portrait = self
            .library
            .update(cx, |library, cx| {
                library.portrait(release.artist, Portrayed::InARow, cx)
            })
            .map_or_else(
                || {
                    kit::avatar(&release.artist_name, false)
                        .size(px(theme::row_cover()))
                        .text_size(px(theme::text_xs()))
                        .into_any_element()
                },
                |art| portrait_frame(art, theme::row_cover()).into_any_element(),
            );

        run_band(
            portrait,
            self.opens(
                ("unheld-artist", index),
                SharedString::from(release.artist_name.clone()),
                words::browser::open_artist_hint(),
                Some(Selection::Artist(release.artist)),
                cx,
            )
            .keeps_its_width(),
            None,
            unheld_by_artist(index, release, releases),
        )
    }
}

fn missing_from_album(index: usize, track: &MissingTrack, tracks: &[MissingTrack]) -> String {
    let short = tracks[index..]
        .iter()
        .take_while(|held| held.album == track.album)
        .count();

    words::missing::missing_from_album(short)
}

fn unheld_by_artist(index: usize, release: &UnheldRelease, releases: &[UnheldRelease]) -> String {
    let unheld = releases[index..]
        .iter()
        .take_while(|other| other.artist == release.artist)
        .count();

    words::missing::releases_unheld(unheld)
}

fn album_said(index: usize, track: &MissingTrack, tracks: &[MissingTrack]) -> SharedString {
    spoken::joined([
        track.album_title.as_str(),
        track.owner.as_deref().unwrap_or_default(),
        missing_from_album(index, track, tracks).as_str(),
    ])
}

fn artist_said(index: usize, release: &UnheldRelease, releases: &[UnheldRelease]) -> SharedString {
    spoken::joined([
        release.artist_name.as_str(),
        unheld_by_artist(index, release, releases).as_str(),
    ])
}

fn release_said(release: &UnheldRelease) -> SharedString {
    let year = release
        .first_released
        .as_deref()
        .map(year_of)
        .unwrap_or_default();

    spoken::joined([
        release.title.as_str(),
        release.kind.as_deref().unwrap_or_default(),
        year,
    ])
}

fn disc_said(disc: u32) -> SharedString {
    SharedString::from(words::missing::disc(disc))
}

fn headings(rows: &[MissingRow], heads: fn(&MissingRow) -> bool) -> usize {
    rows.iter().filter(|row| heads(row)).count()
}

fn run_band(
    picture: impl IntoElement,
    named: impl IntoElement,
    by: Option<SharedString>,
    count: String,
) -> Div {
    row(false)
        .child(
            div()
                .flex()
                .flex_none()
                .w(px(theme::row_number()))
                .child(picture),
        )
        .child(
            div()
                .flex()
                .flex_1()
                .min_w(px(0.0))
                .items_baseline()
                .gap_2()
                .overflow_hidden()
                .child(
                    div()
                        .flex()
                        .min_w(px(0.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(theme::text()))
                        .child(named),
                )
                .when_some(by, |line, by| {
                    line.child(
                        div()
                            .flex_none()
                            .text_size(px(theme::text_xs()))
                            .text_color(rgb(theme::muted()))
                            .whitespace_nowrap()
                            .child(by),
                    )
                }),
        )
        .child(
            kit::figure(count)
                .flex_none()
                .text_color(rgb(theme::faint())),
        )
        .child(controls_place())
}

fn in_a_card(listed: Stateful<Div>, place: Place) -> Div {
    let slice = div()
        .flex()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .border_l_1()
        .border_r_1()
        .border_b_1()
        .border_color(rgb(theme::border()))
        .child(listed.w_full().h_full().px_3());
    let (slice, row) = match place {
        Place::Head => (
            slice
                .h(px(theme::row_height() - HALF_BETWEEN_CARDS))
                .border_t_1()
                .rounded_t_xl()
                .bg(rgb(theme::raised())),
            row(false).items_end(),
        ),
        Place::Within => (slice.h_full().bg(rgb(theme::surface())), row(false)),
        Place::Last => (
            slice
                .h(px(theme::row_height() - HALF_BETWEEN_CARDS))
                .rounded_b_xl()
                .bg(rgb(theme::surface())),
            row(false).items_start(),
        ),
    };

    row.px_3().child(slice)
}

fn disc_heading(disc: u32) -> Div {
    row(false)
        .child(listing::number_cell(SharedString::new_static("")))
        .child(kit::eyebrow(words::missing::disc_eyebrow(disc)))
}

fn listed_short(listed: usize, counted: u64, shows: MissingShows) -> Option<String> {
    ((listed as u64) < counted).then(|| match shows {
        MissingShows::Tracks => words::missing::TracksListedShort { counted, listed }.said(),
        MissingShows::Releases => words::missing::ReleasesListedShort { counted, listed }.said(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gpui::{BorrowAppContext as _, TestAppContext};
    use resonate_core::{AlbumId, Mbid};
    use resonate_library::{
        AlbumQuery, ArtistQuery, ArtistRelease, Library, Medium, Release, ReleaseTrack,
    };

    use super::{MissingShows, Place, listed_short};
    use crate::{
        MissingRow, Pane, ResonateApp, Selection,
        driven::{Driven, Folder},
        views::reorder::{Listed, Shift},
    };

    const THE_RELEASE: &str = "6f8d5c3a-1b2c-4d3e-8f40-000000000001";

    const THE_EP: &str = "6f8d5c3a-1b2c-4d3e-8f40-000000000002";

    const HELD: [&str; 2] = ["One", "Two"];

    const RELEASED: [&str; 4] = ["One", "Two", "Three", "Four"];

    fn mbid(text: &str) -> Mbid {
        Mbid::new(text).expect("a well formed MBID")
    }

    fn released(position: u32, title: &str) -> ReleaseTrack {
        ReleaseTrack {
            position,
            number: position.to_string(),
            title: title.to_owned(),
            artist: None,
            recording: None,
            track: None,
            length: None,
            isrcs: Vec::new(),
            links: Vec::new(),
        }
    }

    fn orbits() -> Release {
        Release {
            id: mbid(THE_RELEASE),
            group: None,
            title: "Orbits".to_owned(),
            credit: Vec::new(),
            date: None,
            country: None,
            label: None,
            catalog_number: None,
            barcode: None,
            kind: Some("Album".to_owned()),
            disambiguation: None,
            has_front_cover: false,
            links: Vec::new(),
            media: vec![Medium {
                position: 1,
                format: None,
                title: None,
                tracks: RELEASED
                    .iter()
                    .zip(1..)
                    .map(|(title, position)| released(position, title))
                    .collect(),
            }],
        }
    }

    fn short_of_two_songs_and_an_ep(folder: &Folder) -> (Arc<Library>, AlbumId) {
        for title in HELD {
            folder.tagged(
                &format!("{title}.wav"),
                1,
                &[
                    (b"IART", "The Orbiters"),
                    (b"INAM", title),
                    (b"IPRD", "Orbits"),
                ],
            );
        }

        let library = Arc::new(Library::open_in_memory().expect("a catalog in memory"));

        Driven::scanned(&library, folder);

        let album = library
            .albums(&AlbumQuery::default())
            .expect("the scanned albums")
            .remove(0)
            .id;
        let artist = library
            .artists(&ArtistQuery::default())
            .expect("the scanned artists")
            .remove(0)
            .id;

        library
            .land_release(album, &orbits())
            .expect("the release lands");
        library.rematch(album).expect("the release is matched");
        library
            .land_artist_releases(
                artist,
                &[ArtistRelease {
                    mbid: mbid(THE_EP),
                    title: "Hours".to_owned(),
                    kind: Some("EP".to_owned()),
                    secondary: Vec::new(),
                    first_released: Some("1996-05-06".to_owned()),
                }],
                (0, 1),
            )
            .expect("the discography lands");

        (library, album)
    }

    fn missing_shown(cx: &mut TestAppContext, folder: &Folder) -> (Driven, AlbumId, Arc<Library>) {
        let (library, album) = short_of_two_songs_and_an_ep(folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), folder);

        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|app, _| app.tabs.missing = true);
        });
        driven.settle();
        driven.click("tab-missing");
        driven.until(|root, cx| root.missing_rows_shown(cx).len() == 3);

        (driven, album, library)
    }

    fn shown(driven: &mut Driven) -> Vec<MissingRow> {
        driven.read(|root, cx| root.missing_rows_shown(cx).to_vec())
    }

    fn reached_at(driven: &mut Driven) -> Option<(Shift, usize)> {
        driven.read(|root, cx| root.reached(cx).map(|reach| (reach.shift, reach.row)))
    }

    fn tabbed_onto(driven: &mut Driven, named: &'static str) -> bool {
        const TAB_STOPS_AT_MOST: usize = 64;

        let root = driven.root.clone();
        (0..TAB_STOPS_AT_MOST).any(|_| {
            driven.cx.simulate_keystrokes("tab");
            driven
                .cx
                .update(|window, cx| root.read(cx).controls.at(named, cx).is_focused(window))
        })
    }

    #[gpui::test]
    fn the_missing_pane_heads_the_songs_an_album_is_short_of_with_the_album_and_draws_each(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _, _) = missing_shown(cx, &folder);

        assert_eq!(driven.read(|root, _| root.pane), Pane::Missing);
        assert_eq!(
            shown(&mut driven),
            [
                MissingRow::Album(0),
                MissingRow::Track(0),
                MissingRow::Track(1)
            ]
        );
        assert!(driven.cx.debug_bounds("unheld-title-0").is_some());
        assert!(driven.cx.debug_bounds("unheld-title-1").is_some());
        assert!(driven.cx.debug_bounds("missing-shows-0").is_some());
        assert!(driven.cx.debug_bounds("missing-shows-1").is_some());
        assert!(driven.cx.debug_bounds("bring-back-dismissed").is_none());
    }

    #[gpui::test]
    fn the_releases_tab_shows_what_an_artist_has_out_unheld_and_the_tracks_tab_shows_the_songs_again(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _, _) = missing_shown(cx, &folder);

        driven.click("missing-shows-1");

        assert_eq!(
            shown(&mut driven),
            [MissingRow::Artist(0), MissingRow::Release(0)]
        );
        assert!(driven.cx.debug_bounds("dismiss-release-0").is_some());
        assert!(driven.cx.debug_bounds("unheld-title-0").is_none());

        driven.click("missing-shows-0");

        assert_eq!(shown(&mut driven).len(), 3);
        assert!(driven.cx.debug_bounds("unheld-title-0").is_some());
    }

    #[gpui::test]
    fn a_song_dismissed_leaves_the_pane_until_the_heading_brings_it_back(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _, library) = missing_shown(cx, &folder);
        let first = library
            .missing_tracks(None, None)
            .expect("the songs missing")
            .first()
            .map(|track| track.release_track)
            .expect("a song is missing");

        driven.click(Box::leak(
            format!("dismiss-missing-{}", first.get()).into_boxed_str(),
        ));
        driven.until(|root, cx| root.missing_rows_shown(cx).len() == 2);

        assert_eq!(
            shown(&mut driven),
            [MissingRow::Album(0), MissingRow::Track(0)]
        );
        assert!(driven.cx.debug_bounds("bring-back-dismissed").is_some());

        driven.click("bring-back-dismissed");
        driven.until(|root, cx| root.missing_rows_shown(cx).len() == 3);

        assert!(driven.cx.debug_bounds("bring-back-dismissed").is_none());
    }

    #[gpui::test]
    fn a_release_dismissed_leaves_its_tab_and_the_pane_hands_over_to_the_songs(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _, _) = missing_shown(cx, &folder);

        driven.click("missing-shows-1");

        driven.click("dismiss-release-0");
        driven.until(|root, cx| root.missing_rows_shown(cx).len() == 3);

        assert_eq!(shown(&mut driven)[0], MissingRow::Album(0));
        assert!(driven.cx.debug_bounds("missing-shows-1").is_none());
        assert!(driven.cx.debug_bounds("bring-back-dismissed").is_some());
    }

    #[gpui::test]
    fn the_arrows_walk_the_missing_rows_and_enter_on_a_heading_opens_its_album(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, album, _) = missing_shown(cx, &folder);

        driven.cx.simulate_keystrokes("down");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::Missing), 0))
        );

        driven.cx.simulate_keystrokes("down down");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::Missing), 2))
        );

        driven.cx.simulate_keystrokes("up up enter");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            Selection::Album(album)
        );
    }

    #[gpui::test]
    fn tab_reaches_the_releases_tab_and_enter_shows_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _, _) = missing_shown(cx, &folder);

        assert!(
            tabbed_onto(&mut driven, "missing-shows-1"),
            "tab never reached the releases tab"
        );

        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert_eq!(
            shown(&mut driven),
            [MissingRow::Artist(0), MissingRow::Release(0)]
        );
        assert!(
            tabbed_onto(&mut driven, "dismiss-release-0"),
            "tab never reached the release's dismissal"
        );
    }

    #[test]
    fn a_list_cut_short_of_its_count_says_so_and_a_whole_one_says_nothing() {
        assert_eq!(listed_short(12, 12, MissingShows::Tracks), None);
        assert_eq!(
            listed_short(5_000, 7_214, MissingShows::Tracks).as_deref(),
            Some("5,000 of 7,214 tracks listed; type to narrow to the rest")
        );
    }

    #[test]
    fn a_run_is_one_card_opened_by_its_heading_and_closed_by_its_last_row() {
        let rows = [
            MissingRow::Album(0),
            MissingRow::Disc(0),
            MissingRow::Track(0),
            MissingRow::Track(1),
            MissingRow::Album(2),
            MissingRow::Track(2),
        ];

        let places: Vec<Place> = (0..rows.len())
            .map(|index| Place::of(&rows, index))
            .collect();

        assert_eq!(
            places,
            vec![
                Place::Head,
                Place::Within,
                Place::Within,
                Place::Last,
                Place::Head,
                Place::Last,
            ]
        );
    }

    #[test]
    fn the_half_that_holds_nothing_hands_over_to_the_one_that_does() {
        assert_eq!(MissingShows::Tracks.within(0, 4), MissingShows::Releases);
        assert_eq!(MissingShows::Releases.within(3, 0), MissingShows::Tracks);
    }

    #[test]
    fn a_half_chosen_by_hand_stands_while_it_holds_something() {
        assert_eq!(MissingShows::Releases.within(3, 4), MissingShows::Releases);
        assert_eq!(MissingShows::Tracks.within(3, 4), MissingShows::Tracks);
        assert_eq!(MissingShows::Tracks.within(0, 0), MissingShows::Tracks);
    }
}
