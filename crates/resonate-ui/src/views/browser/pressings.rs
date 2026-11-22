use gpui::{Context, Div, Pixels, Point, Role, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::AlbumId;
use resonate_library::{GroupRelease, HeldMedium, Mbid};

use super::record::{Asking, OpenedRecord};
use crate::{
    Pressings,
    icons::Icon,
    theme,
    views::{
        hint::Names,
        kit,
        kit::{EndsInAnEllipsis, Tone},
        root::RootView,
        spoken::Spoken,
    },
    words,
};

const PRESSINGS_SHOWN: usize = 12;

impl RootView {
    pub(super) fn other_pressings(
        &self,
        album: AlbumId,
        asking: Asking,
        current: Option<Mbid>,
        pressings: Option<Pressings>,
        cx: &mut Context<Self>,
    ) -> Div {
        let (label, hint, waiting, none) = match asking {
            Asking::Group(_) => (
                words::browser::other_pressings(),
                words::browser::other_pressings_hint(),
                words::browser::asking_for_pressings(),
                words::browser::no_pressings(),
            ),
            Asking::Search => (
                words::browser::find_the_record(),
                words::browser::find_the_record_hint(),
                words::browser::asking_for_releases(),
                words::browser::no_releases(),
            ),
        };

        let listed = match pressings {
            None => {
                return div().child(self.in_the_pane_ring(
                    kit::button(
                        "other-pressings",
                        Some(Icon::Disc),
                        label,
                        hint,
                        Tone::Ghost,
                    ),
                    move |this, _, cx| {
                        let asking = asking.clone();
                        let asked = this.library.update(cx, |library, cx| match asking {
                            Asking::Group(group) => library.pressings_asked(group, cx),
                            Asking::Search => library.releases_asked(album, cx),
                        });

                        this.ask_for_pressings(album, asked, cx);
                    },
                    cx,
                ));
            }
            Some(Pressings::Asking) => return pressing_note(waiting),
            Some(Pressings::Unanswered) => return pressing_note(none),
            Some(Pressings::Listed(listed)) => listed,
        };

        let mut column = div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(kit::eyebrow(words::browser::pressings(listed.len())));

        for (at, pressing) in listed.iter().take(PRESSINGS_SHOWN).enumerate() {
            let in_use = current.as_ref() == Some(&pressing.id);
            let chosen = pressing.id.clone();
            let line = pressing_line(pressing);
            let hint = if in_use {
                words::browser::pressing_in_use_hint()
            } else {
                words::browser::take_the_pressing_hint()
            };

            let row = div()
                .id(("pressing", at))
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_1p5()
                .py_0p5()
                .rounded_md()
                .names(hint)
                .child(kit::figure(line.clone()).truncate().ends_in_an_ellipsis())
                .when(in_use, |row| {
                    row.child(kit::badge(words::browser::in_use(), theme::accent()))
                });

            column = column.child(match in_use {
                true => row.named_as(Role::ListItem, &line, hint).chosen(true),
                false => self.in_the_pane_ring(
                    row.named_as(Role::Button, &line, hint)
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(theme::hover()))),
                    move |this, _, cx| {
                        this.record = None;

                        let taken = chosen.clone();
                        let took = this
                            .library
                            .update(cx, |library, cx| library.take_pressing(album, taken, cx));

                        if took {
                            this.let_go_of_the_pressings(cx);
                        }
                        cx.notify();
                    },
                    cx,
                ),
            });
        }
        if listed.len() > PRESSINGS_SHOWN {
            column = column.child(pressing_note(&words::browser::and_more(
                listed.len() - PRESSINGS_SHOWN,
            )));
        }
        column
    }

    pub(super) fn not_this_record(
        &self,
        album: AlbumId,
        at: Point<Pixels>,
        forgetting: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (label, hint) = match forgetting {
            true => (
                words::browser::forget_the_match_armed(),
                words::browser::forget_the_match_armed_hint(),
            ),
            false => (
                words::browser::forget_the_match(),
                words::browser::forget_the_match_hint(),
            ),
        };

        self.in_the_pane_ring(
            kit::button(
                "not-this-record",
                Some(Icon::Discard),
                label,
                hint,
                Tone::Ghost,
            ),
            move |this, _, cx| {
                if forgetting {
                    this.record = None;
                    this.library
                        .update(cx, |library, cx| library.forget_the_match(album, cx));
                } else {
                    this.record = Some(OpenedRecord::Album {
                        album,
                        at,
                        forgetting: true,
                    });
                }
                cx.notify();
            },
            cx,
        )
    }
}

fn pressing_line(pressing: &GroupRelease) -> String {
    let mut parts: Vec<String> = Vec::new();

    parts.push(
        pressing
            .date
            .clone()
            .unwrap_or_else(|| words::browser::undated().to_owned()),
    );
    if let Some(country) = &pressing.country {
        parts.push(country.clone());
    }
    if let Some(tracks) = pressing.track_count {
        parts.push(words::browser::pressing_tracks(tracks));
    }
    parts.join(" · ")
}

fn pressing_note(said: &str) -> Div {
    div()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child(SharedString::from(said.to_owned()))
}

pub(super) fn pressings(media: &[HeldMedium]) -> Option<String> {
    let format = media.first()?.format.as_deref()?;

    if media
        .iter()
        .any(|medium| medium.format.as_deref() != Some(format))
    {
        return Some(words::browser::discs(media.len()));
    }

    Some(match media.len() {
        1 => format.to_owned(),
        held => format!("{held} × {format}"),
    })
}
