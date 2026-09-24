use std::ops::Range;

use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, div, prelude::*, px, rgb, uniform_list,
};
use resonate_library::{MissingTrack, UnheldRelease};

use crate::{
    MissingRow, Portrayed, Selection, format,
    icons::Icon,
    theme,
    views::{
        browser::{
            OPEN_ALBUM_HINT, OPEN_ARTIST_HINT, Unheld, controls_place, portrait_frame, year_of,
        },
        kit::{self, KeepsItsWidth},
        listing::{self, Pictured},
        root::{RootView, empty, row},
    },
};

const NOTHING_MISSING: &str = "Nothing is missing.";

const NOTHING_MATCHES: &str = "Nothing missing matches.";

const LOOK_IT_UP: &str =
    "Look up the library from Settings › Online to learn what its releases are short of.";

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

impl RootView {
    pub(crate) fn missing_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let counted = library.missing();
        let track_rows = library.missing_track_rows();
        let release_rows = library.unheld_release_rows();
        let tracks = library.missing_tracks();
        let releases = library.unheld_releases();
        let can_enrich = library.can_enrich();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
        let shows = self.missing_shows.within(tracks.len(), releases.len());
        let both = !tracks.is_empty() && !releases.is_empty();
        let summary = match shows {
            MissingShows::Tracks => format!(
                "{} missing from {}",
                format::counted(counted.tracks as usize, "track", "tracks"),
                format::counted(
                    headings(&track_rows, |row| matches!(row, MissingRow::Album(_))),
                    "album",
                    "albums"
                ),
            ),
            MissingShows::Releases => format!(
                "{} by {} not held",
                format::counted(counted.releases as usize, "release", "releases"),
                format::counted(
                    headings(&release_rows, |row| matches!(row, MissingRow::Artist(_))),
                    "artist",
                    "artists"
                ),
            ),
        };
        let rows = match shows {
            MissingShows::Tracks => track_rows,
            MissingShows::Releases => release_rows,
        };
        let nothing = rows.is_empty();

        let heading = kit::heading()
            .child(
                kit::heading_row().child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(px(0.0))
                        .gap_1()
                        .child(kit::eyebrow("COLLECTION"))
                        .child(kit::title("Missing"))
                        .when(!nothing, |column| column.child(kit::subtitle(summary))),
                ),
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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .child(heading)
            .when(nothing, |pane| {
                pane.child(if narrowed {
                    empty(Icon::Missing, NOTHING_MATCHES, None)
                } else {
                    empty(
                        Icon::Missing,
                        NOTHING_MISSING,
                        can_enrich.then_some(LOOK_IT_UP),
                    )
                })
            })
            .when(!nothing, |pane| {
                pane.child(
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
                                    Some(MissingRow::Album(first)) => tracks
                                        .get(first)
                                        .map(|track| this.album_heading(first, track, &tracks, cx)),
                                    Some(MissingRow::Disc(first)) => {
                                        tracks.get(first).map(|track| disc_heading(track.disc))
                                    }
                                    Some(MissingRow::Track(track)) => {
                                        tracks.get(track).map(|row| {
                                            this.unheld_row(track, Unheld::short_of(row), cx)
                                        })
                                    }
                                    Some(MissingRow::Artist(first)) => {
                                        releases.get(first).map(|release| {
                                            this.artist_heading(first, release, &releases, cx)
                                        })
                                    }
                                    Some(MissingRow::Release(release)) => {
                                        releases.get(release).map(release_row)
                                    }
                                    None => None,
                                };
                                if let Some(listed) = listed {
                                    drawn.push(
                                        in_a_card(listed, Place::of(&rows, index))
                                            .into_any_element(),
                                    );
                                }
                            }
                            drawn
                        }),
                    )
                    .h_full()
                    .w_full()
                    .pt_1p5()
                    .pb_4(),
                )
            })
            .into_any_element()
    }

    pub(crate) fn show_what_is_missing(&mut self, shows: MissingShows, cx: &mut Context<Self>) {
        self.missing_shows = shows;
        cx.notify();
    }

    fn missing_tabs(
        &self,
        shows: MissingShows,
        tracks: usize,
        releases: usize,
        cx: &mut Context<Self>,
    ) -> Div {
        let tab = |shown: MissingShows,
                   label: &'static str,
                   count: usize,
                   cx: &mut Context<Self>| {
            kit::segment(("missing-shows", shown as usize), label, shows == shown)
                .gap_2()
                .child(kit::figure(count.to_string()).text_color(rgb(theme::faint())))
                .on_click(cx.listener(move |this, _, _, cx| this.show_what_is_missing(shown, cx)))
        };

        div().flex().pt_1().child(
            kit::segmented()
                .child(tab(MissingShows::Tracks, "Tracks", tracks, cx))
                .child(tab(MissingShows::Releases, "Releases", releases, cx)),
        )
    }

    fn album_heading(
        &self,
        index: usize,
        track: &MissingTrack,
        tracks: &[MissingTrack],
        cx: &mut Context<Self>,
    ) -> Div {
        let short = tracks[index..]
            .iter()
            .take_while(|held| held.album == track.album)
            .count();

        run_band(
            self.cover(Pictured::Album(track.album), cx),
            self.opens(
                ("missing-album", index),
                SharedString::from(track.album_title.clone()),
                OPEN_ALBUM_HINT,
                Some(Selection::Album(track.album)),
                cx,
            )
            .keeps_its_width(),
            track.owner.clone().map(SharedString::from),
            format!("{short} missing"),
        )
    }

    fn artist_heading(
        &self,
        index: usize,
        release: &UnheldRelease,
        releases: &[UnheldRelease],
        cx: &mut Context<Self>,
    ) -> Div {
        let unheld = releases[index..]
            .iter()
            .take_while(|other| other.artist == release.artist)
            .count();
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
                OPEN_ARTIST_HINT,
                Some(Selection::Artist(release.artist)),
                cx,
            )
            .keeps_its_width(),
            None,
            format::counted(unheld, "release", "releases"),
        )
    }
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

fn in_a_card(listed: Div, place: Place) -> Div {
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
        .child(kit::eyebrow(format!("DISC {disc}")))
}

fn release_row(release: &UnheldRelease) -> Div {
    let year = release
        .first_released
        .as_deref()
        .map(year_of)
        .unwrap_or_default()
        .to_owned();

    row(false)
        .child(listing::number_cell(SharedString::new_static("")))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(theme::muted()))
                .child(release.title.clone()),
        )
        .when_some(release.kind.clone(), |row, kind| {
            row.child(kit::badge(kind, theme::muted()))
        })
        .child(listing::length_cell(SharedString::from(year)).text_color(rgb(theme::faint())))
        .child(controls_place())
}

#[cfg(test)]
mod tests {
    use super::{MissingShows, Place};
    use crate::MissingRow;

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
