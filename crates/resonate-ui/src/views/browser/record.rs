use std::cmp::Reverse;

use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, MouseButton, MouseDownEvent, Pixels, Point,
    SharedString, Stateful, anchored, deferred, div, prelude::*, px, rgb,
};
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{Genre, Link, Mbid, ReleaseDetail, Service};

use super::pressings::pressings;
use crate::{
    Selection, motion, theme,
    views::{
        hint::Names, kit, pointed::LitUnderThePointer, root::RootView, sheets, spoken::Spoken,
    },
    words,
};

const RECORD_LABEL: f32 = 112.0;

impl RootView {
    pub(super) fn show_the_record(
        &mut self,
        album: AlbumId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.record, Some(OpenedRecord::Album { album: open, .. }) if open == album) {
            self.record = None;
        } else {
            self.close_the_menu(cx);
            self.record = Some(OpenedRecord::Album {
                album,
                at,
                forgetting: false,
            });
        }
        cx.notify();
    }

    pub(super) fn show_the_artist(
        &mut self,
        artist: ArtistId,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if matches!(self.record, Some(OpenedRecord::Artist { artist: open, .. }) if open == artist)
        {
            self.record = None;
        } else {
            self.close_the_menu(cx);
            self.record = Some(OpenedRecord::Artist { artist, at });
        }
        cx.notify();
    }

    pub(crate) fn record_over_the_app(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (at, card) = {
            let opened = *self.record.as_ref()?;
            let library = self.library.read(cx);
            match opened {
                OpenedRecord::Album {
                    album,
                    at,
                    forgetting,
                } => {
                    if library.selection() != Selection::Album(album) {
                        return None;
                    }

                    let release = library.release();
                    let findable = library.can_enrich() && !release.is_some_and(is_matched);

                    let record = release
                        .and_then(record_of)
                        .or_else(|| findable.then(Record::bare))?;
                    let group = release
                        .and_then(|release| release.group.clone())
                        .filter(|_| library.can_enrich());
                    let current = release.and_then(|release| release.mbid.clone());
                    let pressings = self.pressings_of(album, cx).cloned();
                    let card = record_card(self, &record, cx);

                    let card = match (group, findable) {
                        (Some(group), _) => card.child(self.other_pressings(
                            album,
                            Asking::Group(group),
                            current,
                            pressings,
                            cx,
                        )),
                        (None, true) => card.child(self.other_pressings(
                            album,
                            Asking::Search,
                            current,
                            pressings,
                            cx,
                        )),
                        (None, false) => card,
                    };
                    let card = match record.matched {
                        true => card.child(self.not_this_record(album, at, forgetting, cx)),
                        false => card,
                    };
                    (at, card)
                }
                OpenedRecord::Artist { artist, at } => {
                    if library.selection() != Selection::Artist(artist) {
                        return None;
                    }

                    let genres = self
                        .around_the_rows(cx)
                        .artist
                        .as_ref()
                        .map(|detail| genre_names(&detail.genres))
                        .unwrap_or_default();

                    if genres.is_empty() {
                        return None;
                    }
                    (at, genre_card(&genres))
                }
            }
        };

        Some(self.detail_over(at, card, cx))
    }

    fn detail_over(
        &self,
        at: Point<Pixels>,
        card: Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.record = None;
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.record = None;
                    cx.notify();
                }),
            )
            .child(
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.0))
                        .child(motion::lifted_in(
                            card.track_focus(&self.sheet_panels.holding(sheets::RECORD_PANEL, cx)),
                            "record-arrives",
                        )),
                )
                .with_priority(1),
            )
            .into_any_element()
    }
}

pub(super) fn record_of(release: &ReleaseDetail) -> Option<Record> {
    let mut facts = Vec::new();

    if let Some(date) = release.date.clone() {
        facts.push(Fact {
            label: words::browser::released(),
            value: date,
        });
    }
    if let Some(format) = pressings(&release.media) {
        facts.push(Fact {
            label: words::browser::format(),
            value: format,
        });
    }
    if let Some(label) = release.label.clone() {
        facts.push(Fact {
            label: words::browser::label(),
            value: label,
        });
    }
    if let Some(number) = release.catalog_number.clone() {
        facts.push(Fact {
            label: words::browser::catalogue_number(),
            value: number,
        });
    }
    if let Some(barcode) = release.barcode.clone() {
        facts.push(Fact {
            label: words::browser::barcode(),
            value: barcode,
        });
    }
    if let Some(country) = release.country.clone() {
        facts.push(Fact {
            label: words::browser::country(),
            value: country,
        });
    }
    if let Some(kind) = release.kind.clone() {
        facts.push(Fact {
            label: words::browser::kind(),
            value: kind,
        });
    }

    let note = release
        .disambiguation
        .clone()
        .filter(|note| !note.trim().is_empty());
    let on = service_names(&release.links);

    let record = Record {
        facts,
        note,
        on,
        matched: is_matched(release),
    };

    (!record.is_empty()).then_some(record)
}

#[derive(Clone, Hash)]
pub(super) struct HeardOn {
    pub(super) name: &'static str,
    url: SharedString,
}

pub(super) struct Fact {
    pub(super) label: &'static str,
    pub(super) value: String,
}

pub(super) struct Record {
    pub(super) facts: Vec<Fact>,
    pub(super) note: Option<String>,
    pub(super) on: Vec<HeardOn>,
    matched: bool,
}

#[derive(Clone)]
pub(super) enum Asking {
    Group(Mbid),
    Search,
}

pub(super) fn is_matched(release: &ReleaseDetail) -> bool {
    release.mbid.is_some() || release.group.is_some()
}

impl Record {
    const fn bare() -> Self {
        Self {
            facts: Vec::new(),
            note: None,
            on: Vec::new(),
            matched: false,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.facts.is_empty() && self.note.is_none() && self.on.is_empty()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum OpenedRecord {
    Album {
        album: AlbumId,
        at: Point<Pixels>,
        forgetting: bool,
    },
    Artist {
        artist: ArtistId,
        at: Point<Pixels>,
    },
}

fn detail_card(title: &'static str) -> Stateful<Div> {
    kit::floating()
        .id("record")
        .dialog(title)
        .w(px(theme::record_width()))
        .p_3()
        .gap_2()
        .child(
            div()
                .id("record-title")
                .headed(2, title)
                .text_size(px(theme::text_sm()))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::text()))
                .child(title),
        )
}

fn record_card(root: &RootView, record: &Record, cx: &mut Context<RootView>) -> Stateful<Div> {
    let mut card = detail_card(words::browser::about_this_record());

    for fact in &record.facts {
        card = card.child(fact_row(fact));
    }
    if let Some(note) = &record.note {
        card = card.child(
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(note.clone()),
        );
    }
    if !record.on.is_empty() {
        card = card.child(record_services(root, &record.on, cx));
    }
    card
}

fn genre_card(genres: &[String]) -> Stateful<Div> {
    let mut tags = div().flex().flex_wrap().gap_1p5();

    for genre in genres {
        tags = tags.child(kit::tag(genre.clone()));
    }
    detail_card(words::browser::genres()).child(tags)
}

pub(super) fn service_names(links: &[Link]) -> Vec<HeardOn> {
    let mut named: Vec<HeardOn> = Vec::new();

    for link in links {
        if link.service == Service::Other {
            continue;
        }

        let name = link.service.title();
        if !named.iter().any(|held| held.name == name) {
            named.push(HeardOn {
                name,
                url: SharedString::from(link.url.clone()),
            });
        }
    }
    named
}

const SERVICES_SHOWN: usize = 4;

pub(super) fn heard_on(
    root: &RootView,
    id: &'static str,
    services: Vec<HeardOn>,
    room: Pixels,
    cx: &mut Context<RootView>,
) -> Option<Div> {
    if services.is_empty() {
        return None;
    }

    let mut line = kit::wraps_within(room)
        .items_center()
        .text_size(px(theme::text_sm()))
        .text_color(rgb(theme::faint()))
        .child(words::browser::on_services());

    for (at, heard) in services.into_iter().take(SERVICES_SHOWN).enumerate() {
        if at > 0 {
            line = line.child(div().px_1().child("·"));
        }
        line = line.child(service_link(
            root,
            ElementId::NamedInteger(id.into(), at as u64),
            heard,
            cx,
        ));
    }
    Some(line)
}

fn fact_row(fact: &Fact) -> Div {
    div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
            div()
                .w(px(RECORD_LABEL))
                .flex_none()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(fact.label),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::text()))
                .child(fact.value.clone()),
        )
}

fn record_services(root: &RootView, services: &[HeardOn], cx: &mut Context<RootView>) -> Div {
    let mut links = div()
        .flex()
        .flex_1()
        .min_w(px(0.0))
        .flex_wrap()
        .items_center()
        .gap_x_2()
        .gap_y_1();

    for (at, heard) in services.iter().cloned().enumerate() {
        links = links.child(service_link(
            root,
            ElementId::NamedInteger("record-service".into(), at as u64),
            heard,
            cx,
        ));
    }

    div()
        .flex()
        .items_baseline()
        .gap_3()
        .child(
            div()
                .w(px(RECORD_LABEL))
                .flex_none()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::faint()))
                .child(words::browser::on()),
        )
        .child(links)
}

fn service_link(
    root: &RootView,
    id: ElementId,
    heard: HeardOn,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let url = heard.url;
    let hint = words::browser::open_on(heard.name);

    let link = div()
        .id(id.clone())
        .linked_as(&heard.name, &hint)
        .cursor_pointer()
        .lit_under_the_pointer(id, |link| {
            link.text_color(rgb(theme::accent()))
                .text_decoration_1()
                .text_decoration_color(rgb(theme::accent()))
        })
        .names(hint)
        .child(heard.name);

    root.in_the_pane_ring(
        link,
        move |_, _, cx| {
            cx.stop_propagation();
            cx.open_url(&url);
        },
        cx,
    )
}

pub(super) fn genre_names(genres: &[Genre]) -> Vec<String> {
    let mut weighed: Vec<&Genre> = genres.iter().collect();

    weighed.sort_by_key(|genre| Reverse(genre.weight));
    weighed
        .into_iter()
        .map(|genre| genre.name.clone())
        .collect()
}
