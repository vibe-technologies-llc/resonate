use std::{ops::Range, sync::Arc};

use gpui::{AnyElement, Context, Div, div, prelude::*, px, uniform_list};
use resonate_core::TrackId;
use resonate_library::{Album, Artist, Track};

use crate::{
    Favourited, LibraryModel, format,
    icons::Icon,
    theme,
    views::{
        browser::{Chooses, Plays, Rowed, SHELF_AT_MOST, TRACK_CONTROLS},
        kit, listing,
        reorder::{Listed, Shift, Step},
        root::{RootView, empty},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_FAVOURITE: &str = "Nothing is a favourite yet.";

const NOTHING_MATCHES: &str = "No favourite matches.";

const HOW_TO_FAVOUR: &str = "Press the star on a track, an album or an artist to keep it here.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shelved {
    artists: usize,
    albums: usize,
    tracks: usize,
}

impl Shelved {
    const DRAWN: [Listed; 3] = [
        Listed::FavouriteArtists,
        Listed::FavouriteAlbums,
        Listed::Favourites,
    ];

    pub(crate) fn of(library: &LibraryModel) -> Self {
        Self {
            artists: library.favourite_artists().len().min(SHELF_AT_MOST),
            albums: library.favourite_albums().len().min(SHELF_AT_MOST),
            tracks: library.favourite_tracks().len(),
        }
    }

    pub(crate) const fn rows_of(self, listed: Listed) -> usize {
        match listed {
            Listed::FavouriteArtists => self.artists,
            Listed::FavouriteAlbums => self.albums,
            Listed::Favourites => self.tracks,
            _ => 0,
        }
    }

    pub(crate) fn opening(self) -> Option<Listed> {
        Self::DRAWN
            .into_iter()
            .rev()
            .find(|listed| self.rows_of(*listed) > 0)
    }

    pub(crate) fn beyond(self, from: Listed, step: Step) -> Option<Listed> {
        let at = Self::DRAWN.iter().position(|listed| *listed == from)?;
        let held = |listed: &Listed| self.rows_of(*listed) > 0;
        match step {
            Step::Above => Self::DRAWN[..at].iter().rev().copied().find(held),
            Step::Below => Self::DRAWN[at + 1..].iter().copied().find(held),
        }
    }

    pub(crate) const fn is_a_strip(listed: Listed) -> bool {
        matches!(listed, Listed::FavouriteArtists | Listed::FavouriteAlbums)
    }

    pub(crate) fn along_the_strip(
        self,
        listed: Listed,
        row: usize,
        step: Step,
    ) -> Option<Option<usize>> {
        if !Self::is_a_strip(listed) {
            return None;
        }
        Some(match step {
            Step::Below => (row + 1 < self.rows_of(listed)).then_some(row + 1),
            Step::Above => row.checked_sub(1),
        })
    }
}

impl RootView {
    pub(crate) fn favourites_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let held = library.favourited();
        let artists = library.favourite_artists();
        let albums = library.favourite_albums();
        let tracks = library.favourite_tracks();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
        let playing = self.playing_now(cx).track;
        let nothing = held.is_empty();

        let shelves = (!nothing).then(|| self.favourite_shelves(held, &artists, &albums, cx));
        let listed = (held.tracks > 0).then(|| self.favourite_rows(&tracks, playing, cx));

        kit::pane()
            .child(favourites_heading(held, &reads))
            .when(nothing, |pane| {
                pane.child(if narrowed {
                    empty(Icon::Favourite, NOTHING_MATCHES, None)
                } else {
                    empty(Icon::Favourite, NOTHING_FAVOURITE, Some(HOW_TO_FAVOUR))
                })
            })
            .children(shelves)
            .children(listed)
            .into_any_element()
    }

    fn favourite_shelves(
        &self,
        held: Favourited,
        artists: &[Artist],
        albums: &[Album],
        cx: &mut Context<Self>,
    ) -> Div {
        let mut stacked = div().flex().flex_col().flex_none();
        if held.artists > 0 {
            stacked = stacked.child(self.artist_shelf_of(
                "favourite-artists",
                format::counted(held.artists, "artist", "artists"),
                artists,
                Listed::FavouriteArtists,
                cx,
            ));
        }
        if held.albums > 0 {
            stacked = stacked.child(self.album_shelf(
                "favourite-albums",
                format::counted(held.albums, "album", "albums"),
                albums,
                Listed::FavouriteAlbums,
                cx,
            ));
        }

        stacked
    }

    fn favourite_rows(
        &self,
        tracks: &Arc<[Track]>,
        playing: Option<TrackId>,
        cx: &mut Context<Self>,
    ) -> Div {
        let rows = Arc::clone(tracks);
        let held = rows.len();

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h(px(0.0))
            .child(listing::columns(
                "#",
                true,
                TRACK_CONTROLS,
                sorting::favourites_sorted(self, cx),
                &self.columns_fit,
                cx,
            ))
            .child(
                Scrollbars::of(cx).around(
                    "favourites-scrollbar",
                    self.favourite_rows.clone(),
                    uniform_list(
                        "favourite-tracks",
                        held,
                        cx.processor(move |this, range: Range<usize>, _, cx| {
                            let mut drawn = Vec::new();
                            for index in range {
                                let Some(track) = rows.get(index) else {
                                    continue;
                                };
                                let shift = Shift::Listing(Listed::Favourites);
                                let row = this.track_row(
                                    &rows,
                                    index,
                                    track,
                                    Rowed {
                                        playing: playing == Some(track.id),
                                        plays: Plays::TheseRows,
                                        reach: this.at_the_reach(shift, index),
                                        chooses: Some(Chooses::among(Listed::Favourites, index)),
                                    },
                                    cx,
                                );
                                drawn.push(this.marked_where_reached(shift, index, row));
                            }
                            drawn
                        }),
                    )
                    .track_scroll(&self.favourite_rows)
                    .h_full()
                    .w_full(),
                ),
            )
    }
}

fn favourites_heading(held: Favourited, reads: &[String]) -> Div {
    let counted = held.counted();

    kit::heading()
        .child(
            kit::heading_row().child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w(px(theme::heading_name()))
                    .gap_1()
                    .child(kit::eyebrow("COLLECTION"))
                    .child(kit::title("Favourites"))
                    .when(!counted.is_empty(), |column| {
                        column.child(kit::subtitle(counted))
                    }),
            ),
        )
        .when(!reads.is_empty(), |heading| {
            heading.child(listing::reads(reads))
        })
}

#[cfg(test)]
mod tests {
    use super::Shelved;
    use crate::views::reorder::{Listed, Step};

    fn shelved(artists: usize, albums: usize, tracks: usize) -> Shelved {
        Shelved {
            artists,
            albums,
            tracks,
        }
    }

    #[test]
    fn the_keys_open_on_the_songs_and_fall_back_to_the_shelves_nearest_them() {
        assert_eq!(shelved(2, 3, 4).opening(), Some(Listed::Favourites));
        assert_eq!(shelved(2, 3, 0).opening(), Some(Listed::FavouriteAlbums));
        assert_eq!(shelved(2, 0, 0).opening(), Some(Listed::FavouriteArtists));
        assert_eq!(shelved(0, 0, 0).opening(), None);
    }

    #[test]
    fn a_step_leaves_a_section_for_the_nearest_one_drawn_with_something_in_it() {
        let all = shelved(2, 3, 4);
        let no_albums = shelved(2, 0, 4);

        assert_eq!(
            all.beyond(Listed::FavouriteArtists, Step::Below),
            Some(Listed::FavouriteAlbums)
        );
        assert_eq!(
            all.beyond(Listed::Favourites, Step::Above),
            Some(Listed::FavouriteAlbums)
        );
        assert_eq!(
            no_albums.beyond(Listed::FavouriteArtists, Step::Below),
            Some(Listed::Favourites)
        );
        assert_eq!(
            no_albums.beyond(Listed::Favourites, Step::Above),
            Some(Listed::FavouriteArtists)
        );
        assert_eq!(all.beyond(Listed::FavouriteArtists, Step::Above), None);
        assert_eq!(all.beyond(Listed::Favourites, Step::Below), None);
        assert_eq!(all.beyond(Listed::Albums, Step::Below), None);
    }

    #[test]
    fn the_cells_of_a_shelf_are_stepped_along_and_the_songs_are_not_one() {
        let run = shelved(3, 2, 4);

        assert_eq!(
            run.along_the_strip(Listed::FavouriteArtists, 0, Step::Below),
            Some(Some(1))
        );
        assert_eq!(
            run.along_the_strip(Listed::FavouriteArtists, 2, Step::Below),
            Some(None)
        );
        assert_eq!(
            run.along_the_strip(Listed::FavouriteAlbums, 0, Step::Above),
            Some(None)
        );
        assert_eq!(
            run.along_the_strip(Listed::FavouriteAlbums, 1, Step::Above),
            Some(Some(0))
        );
        assert_eq!(
            run.along_the_strip(Listed::Favourites, 1, Step::Below),
            None
        );
    }
}
