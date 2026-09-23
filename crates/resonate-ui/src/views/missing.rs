use gpui::{AnyElement, Context, Div, SharedString, div, prelude::*, px, rgb, uniform_list};
use resonate_library::{MissingTrack, UnheldRelease};

use crate::{
    MissingRow, Selection, format,
    icons::Icon,
    theme,
    views::{
        browser::{
            OPEN_ALBUM_HINT, OPEN_ARTIST_HINT, Unheld, controls_place, run_heading, year_of,
        },
        kit::{self, KeepsItsWidth},
        listing,
        root::{RootView, empty, row},
    },
};

const NOTHING_MISSING: &str = "Nothing is missing.";

const NOTHING_MATCHES: &str = "Nothing missing matches.";

const LOOK_IT_UP: &str =
    "Look up the library from Settings › Online to learn what its releases are short of.";

impl RootView {
    pub(crate) fn missing_pane(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let library = self.library.read(cx);
        let counted = library.missing();
        let rows = library.missing_rows();
        let tracks = library.missing_tracks();
        let releases = library.unheld_releases();
        let can_enrich = library.can_enrich();
        let narrowed = library.narrowing().is_some();
        let reads = library.search().reads();
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
                        .child(kit::subtitle(format!(
                            "{} · {}",
                            format::counted(counted.tracks as usize, "track", "tracks"),
                            format::counted(counted.releases as usize, "release", "releases")
                        ))),
                ),
            )
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
                        "missing",
                        rows.len(),
                        cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                            let mut drawn = Vec::new();
                            for index in range {
                                let listed = match rows.get(index).copied() {
                                    Some(MissingRow::Album(first)) => tracks
                                        .get(first)
                                        .map(|track| this.album_heading(first, track, cx)),
                                    Some(MissingRow::Disc(first)) => tracks
                                        .get(first)
                                        .map(|track| run_heading(disc_of(track.disc))),
                                    Some(MissingRow::Track(track)) => tracks
                                        .get(track)
                                        .map(|row| this.unheld_row(track, Unheld::from(row), cx)),
                                    Some(MissingRow::Artist(first)) => releases
                                        .get(first)
                                        .map(|release| this.artist_heading(first, release, cx)),
                                    Some(MissingRow::Release(release)) => {
                                        releases.get(release).map(release_row)
                                    }
                                    None => None,
                                };
                                if let Some(listed) = listed {
                                    drawn.push(listed.into_any_element());
                                }
                            }
                            drawn
                        }),
                    )
                    .h_full()
                    .w_full(),
                )
            })
            .into_any_element()
    }

    fn album_heading(&self, index: usize, track: &MissingTrack, cx: &mut Context<Self>) -> Div {
        let billed = match track.owner.as_deref() {
            Some(owner) => format!("{} · {owner}", track.album_title),
            None => track.album_title.clone(),
        };

        run_heading(
            self.opens(
                ("missing-album", index),
                SharedString::from(billed),
                OPEN_ALBUM_HINT,
                Some(Selection::Album(track.album)),
                cx,
            )
            .keeps_its_width(),
        )
    }

    fn artist_heading(&self, index: usize, release: &UnheldRelease, cx: &mut Context<Self>) -> Div {
        run_heading(
            self.opens(
                ("unheld-artist", index),
                SharedString::from(release.artist_name.clone()),
                OPEN_ARTIST_HINT,
                Some(Selection::Artist(release.artist)),
                cx,
            )
            .keeps_its_width(),
        )
    }
}

fn disc_of(disc: u32) -> SharedString {
    SharedString::from(format!("Disc {disc}"))
}

fn release_row(release: &UnheldRelease) -> Div {
    let kind = SharedString::from(release.kind.clone().unwrap_or_default());
    let year = SharedString::from(
        release
            .first_released
            .as_deref()
            .map(year_of)
            .unwrap_or_default()
            .to_owned(),
    );

    row(false)
        .child(listing::number_cell(SharedString::new_static("")))
        .child(
            listing::title_cell(SharedString::from(release.title.clone()), Vec::new(), false)
                .text_color(rgb(theme::faint())),
        )
        .child(listing::artist_cell(kind).text_color(rgb(theme::faint())))
        .child(
            div()
                .w(px(theme::row_format()))
                .flex_none()
                .child(kit::figure(year).text_color(rgb(theme::faint()))),
        )
        .child(listing::unheard())
        .child(listing::length_cell(SharedString::new_static("")))
        .child(controls_place())
}
