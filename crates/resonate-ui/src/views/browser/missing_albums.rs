use gpui::{Context, Div, FontWeight, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_library::{AlbumFound, AlbumNotHeld, Mbid};

use super::{
    grid::CELL_GROUP,
    missing_rows::{UNHELD_COVER, fetching_colour, unheld_cover},
};
use crate::{
    Drawn, Selection, theme,
    views::{
        hint::Names,
        kit,
        kit::{Casts as _, EndsInAnEllipsis},
        listing,
        root::{RootView, framed_cover},
        spoken,
        spoken::Spoken,
    },
    words,
};

impl RootView {
    pub(super) fn album_not_held_cell(
        &self,
        album: &AlbumNotHeld,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.unheld_album_cell(
            UnheldAlbum {
                group: &album.release.mbid,
                pressing: album.pressing.as_ref(),
                title: &album.release.title,
                caption: described(&album.release.kind, album.release.first_released.as_deref()),
            },
            side,
            cx,
        )
    }

    pub(crate) fn album_found_cell(
        &self,
        album: &AlbumFound,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let by = SharedString::from(album.artist.clone());
        let described = described(&album.kind, album.first_released.as_deref());

        let caption = match described.is_empty() {
            true => by.to_string(),
            false => format!("{by} · {described}"),
        };

        let selector = format!("album-found-{}", album.group);

        self.unheld_album_cell(
            UnheldAlbum {
                group: &album.group,
                pressing: None,
                title: &album.title,
                caption,
            },
            side,
            cx,
        )
        .debug_selector(move || selector.clone())
    }

    fn unheld_album_cell(
        &self,
        album: UnheldAlbum<'_>,
        side: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let group = album.group.clone();
        let fetching = self.library.read(cx).fetching_album(&group);
        let told = self.library.read(cx).told_album(&group);
        let opening = self.library.read(cx).is_opening_album(&group);
        let can_ask = self.library.read(cx).can_enrich();
        let art = self.library.update(cx, |library, cx| {
            library.released_cover_with_group(
                album.pressing.unwrap_or(&group),
                Some(&group),
                Drawn::InAGrid,
                cx,
            )
        });
        let cover = match art {
            Some(art) => framed_cover(Some(art), side).opacity(UNHELD_COVER),
            None => unheld_cover(side),
        };

        let under = match fetching {
            Some(fetching) => div()
                .text_color(rgb(fetching_colour(fetching)))
                .child(told.unwrap_or_else(|| fetching.saying())),
            None if opening => div()
                .text_color(rgb(theme::accent()))
                .child(words::browser::opening()),
            None => div()
                .text_color(rgb(theme::faint()))
                .child(SharedString::from(album.caption)),
        };
        let pressable = can_ask;

        let cell = div()
            .id(listing::keyed_by("album-not-held", &group))
            .listed_as(|| spoken::song(album.title, Some(words::browser::not_in_the_library())))
            .group(CELL_GROUP)
            .flex()
            .flex_none()
            .flex_col()
            .gap_2p5()
            .w(px(side))
            .child(
                div()
                    .relative()
                    .rounded(px(8.0))
                    .when(pressable, |frame| {
                        frame.group_hover(CELL_GROUP, |frame| frame.casting(kit::Cast::Short))
                    })
                    .child(cover),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(px(theme::text_sm()))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(theme::faint()))
                            .truncate()
                            .ends_in_an_ellipsis()
                            .child(SharedString::from(album.title.to_owned())),
                    )
                    .child(
                        under
                            .text_size(px(theme::text_xs()))
                            .truncate()
                            .ends_in_an_ellipsis(),
                    ),
            );

        if !pressable {
            return cell;
        }

        cell.cursor_pointer()
            .names(words::browser::open_album_not_held_hint())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.open_album_not_held(group.clone(), cx);
            }))
    }

    pub(crate) fn open_album_not_held(&mut self, group: Mbid, cx: &mut Context<Self>) {
        let pressed = self.press_a_lookup();
        let landed = self
            .library
            .update(cx, |library, cx| library.land_album_not_held(group, cx));

        cx.spawn(async move |this, cx| {
            let Some(album) = landed.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if !this.window_has_moved_on_from(pressed, cx) {
                    this.opened(Selection::Album(album), cx);
                }
            });
        })
        .detach();
    }
}

pub(super) fn not_held_heading(albums: usize, under_the_held: bool) -> Div {
    div()
        .w_full()
        .when(under_the_held, |heading| heading.pt_4())
        .text_size(px(theme::text_xs()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(theme::faint()))
        .child(SharedString::from(
            words::browser::releases_not_in_your_library(albums),
        ))
}

struct UnheldAlbum<'a> {
    group: &'a Mbid,
    pressing: Option<&'a Mbid>,
    title: &'a str,
    caption: String,
}

fn described(kind: &Option<String>, first_released: Option<&str>) -> String {
    [
        kind.clone(),
        first_released
            .and_then(|date| date.get(..4))
            .map(str::to_owned),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}
