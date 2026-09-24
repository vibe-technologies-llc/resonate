use std::{ops::Range, sync::Arc};

use gpui::{AnyElement, Context, Div, div, prelude::*, px, uniform_list};
use resonate_core::TrackId;
use resonate_library::{Album, Artist, Track};

use crate::{
    Favourited, format,
    icons::Icon,
    views::{
        kit, listing,
        root::{RootView, empty},
        scrollbar::Scrollbars,
        sorting,
    },
};

const NOTHING_FAVOURITE: &str = "Nothing is a favourite yet.";

const NOTHING_MATCHES: &str = "No favourite matches.";

const HOW_TO_FAVOUR: &str = "Press the star on a track, an album or an artist to keep it here.";

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

        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
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
                cx,
            ));
        }
        if held.albums > 0 {
            stacked = stacked.child(self.album_shelf(
                "favourite-albums",
                format::counted(held.albums, "album", "albums"),
                albums,
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
            .child(listing::columns("#", true, sorting::unsorted(), cx))
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
                                drawn.push(this.track_row(
                                    &rows,
                                    index,
                                    track,
                                    playing == Some(track.id),
                                    false,
                                    cx,
                                ));
                            }
                            drawn
                        }),
                    )
                    .track_scroll(self.favourite_rows.clone())
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
                    .min_w(px(0.0))
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
