use std::sync::Arc;

use gpui::{
    Context, Div, Entity, FontWeight, MouseButton, SharedString, Stateful, Window, div, prelude::*,
    px, rgb,
};
use resonate_core::{Isrc, TrackId};
use resonate_library::{Recording, in_the_order_worth_offering};

use crate::{
    Notice, format,
    icons::Icon,
    motion, theme,
    views::{
        field::{Field, Submitted},
        kit,
        root::RootView,
        scrollbar::Scrollbars,
        sheets,
        spoken::Spoken as _,
    },
    words,
};

const LIKELY_LIST_AT_MOST: f32 = 280.0;

#[derive(Clone, Debug, Default)]
pub(crate) enum Likely {
    #[default]
    Unasked,
    Offline,
    Asking,
    Found(Arc<[Arc<Recording>]>),
    Unreached,
}

pub(crate) struct Matching {
    pub(crate) track: TrackId,
    likely: Likely,
    asked: u64,
}

pub(crate) fn matching_fields(
    window: &mut Window,
    cx: &mut Context<RootView>,
) -> (Entity<Field>, Entity<Field>) {
    let words = cx.new(|cx| Field::new(words::matching::words_placeholder(), window, cx));
    cx.subscribe_in(&words, window, |this, _, _: &Submitted, _, cx| {
        this.look_for_likely_recordings(cx);
    })
    .detach();
    let isrc = cx.new(|cx| Field::new(words::matching::isrc_placeholder(), window, cx));
    cx.subscribe_in(&isrc, window, |this, _, _: &Submitted, window, cx| {
        this.isrc_given(window, cx);
    })
    .detach();

    (words, isrc)
}

fn credit_of(recording: &Recording) -> String {
    recording
        .credit
        .iter()
        .map(|credit| format!("{}{}", credit.name, credit.joined_by))
        .collect()
}

fn likely_line(recording: &Recording) -> String {
    let release = in_the_order_worth_offering(&recording.releases)
        .first()
        .map(
            |release| match release.date.as_deref().and_then(|date| date.get(..4)) {
                Some(year) => format!("{} ({year})", release.title),
                None => release.title.clone(),
            },
        );

    [
        Some(recording.title.clone()),
        Some(credit_of(recording)).filter(|credit| !credit.is_empty()),
        recording.length.map(format::spanned),
        release,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ")
}

impl RootView {
    pub(crate) fn ask_to_match(
        &mut self,
        track: TrackId,
        title: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_the_menu(cx);
        self.matching = Some(Matching {
            track,
            likely: Likely::Unasked,
            asked: 0,
        });
        self.typed_isrc.update(cx, |typed, cx| typed.clear(cx));
        self.typed_words.update(cx, |typed, cx| {
            typed.open_with(title.to_string(), cx);
            typed.take_focus(window, cx);
        });
        self.look_for_likely_recordings(cx);
    }

    pub(crate) fn leave_the_matching(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.matching = None;
        self.typed_isrc.update(cx, |typed, cx| typed.clear(cx));
        self.typed_words.update(cx, |typed, cx| typed.clear(cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn look_for_likely_recordings(&mut self, cx: &mut Context<Self>) {
        let Some(matching) = self.matching.as_mut() else {
            return;
        };
        let track = matching.track;
        let words = self.typed_words.read(cx).text().trim().to_owned();
        let asking = self
            .library
            .update(cx, |library, cx| library.recordings_like(track, words, cx));
        let Some(asking) = asking else {
            if let Some(matching) = self.matching.as_mut() {
                matching.likely = Likely::Offline;
            }
            cx.notify();
            return;
        };
        let Some(matching) = self.matching.as_mut() else {
            return;
        };
        matching.asked += 1;
        matching.likely = Likely::Asking;
        let asked = matching.asked;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let answer = asking.await;
            let _ = this.update(cx, |this, cx| {
                let Some(matching) = this
                    .matching
                    .as_mut()
                    .filter(|matching| matching.track == track && matching.asked == asked)
                else {
                    return;
                };
                matching.likely = match answer {
                    Ok(found) => Likely::Found(found.into_iter().map(Arc::new).collect()),
                    Err(error) => {
                        tracing::warn!(%error, "MusicBrainz was not asked for recordings like the song");
                        Likely::Unreached
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn take_the_likely(
        &mut self,
        recording: Arc<Recording>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(track) = self.matching.as_ref().map(|matching| matching.track) else {
            return;
        };
        self.library.update(cx, |library, cx| {
            library.take_recording(track, recording, cx)
        });
        self.leave_the_matching(window, cx);
    }

    fn isrc_given(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(track) = self.matching.as_ref().map(|matching| matching.track) else {
            return;
        };
        let typed = self.typed_isrc.read(cx).text().trim().to_owned();
        let Ok(isrc) = Isrc::new(&typed) else {
            self.report(
                Notice::Trouble(words::matching::not_an_isrc().to_owned()),
                cx,
            );
            return;
        };
        self.library
            .update(cx, |library, cx| library.give_an_isrc(track, isrc, cx));
        self.leave_the_matching(window, cx);
    }

    fn likely_rows(&self, likely: &Likely, cx: &mut Context<Self>) -> Div {
        let said = |words: &'static str| {
            div()
                .id(words)
                .read_plainly(words)
                .py_2()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .child(words)
        };

        let rows = div().flex().flex_col().gap_1();
        match likely {
            Likely::Unasked => rows,
            Likely::Offline => rows.child(said(words::matching::offline())),
            Likely::Asking => rows.child(said(words::matching::asking())),
            Likely::Unreached => rows.child(said(words::matching::unreached())),
            Likely::Found(found) if found.is_empty() => {
                rows.child(said(words::matching::nothing_like_it()))
            }
            Likely::Found(found) => {
                rows.children(found.iter().enumerate().map(|(at, recording)| {
                    let taken = Arc::clone(recording);

                    self.in_the_pane_ring(
                        kit::button(
                            ("likely-recording", at),
                            Some(Icon::Disc),
                            likely_line(recording),
                            words::matching::take_hint(),
                            kit::Tone::Ghost,
                        )
                        .w_full()
                        .justify_start(),
                        move |this, window, cx| {
                            this.take_the_likely(Arc::clone(&taken), window, cx)
                        },
                        cx,
                    )
                }))
            }
        }
    }

    fn typed_into(field: &Entity<Field>, id: &'static str) -> Stateful<Div> {
        kit::framed_field(id, false)
            .flex_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(field.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    pub(crate) fn matching_sheet(&self, cx: &mut Context<Self>) -> Div {
        let likely = self
            .matching
            .as_ref()
            .map(|matching| matching.likely.clone())
            .unwrap_or_default();

        let heading = kit::sheet_heading(
            "matching-sheet-asking",
            Icon::Globe,
            theme::accent(),
            words::matching::matching_label(),
        );
        let saying = div()
            .id("matching-sheet-saying")
            .read_plainly(words::matching::matching_saying())
            .text_size(px(theme::text_sm()))
            .text_color(rgb(theme::muted()))
            .child(words::matching::matching_saying());
        let searching = div()
            .flex()
            .items_center()
            .gap_2()
            .child(Self::typed_into(&self.typed_words, "matching-words"))
            .child(self.in_the_pane_ring(
                kit::button(
                    "search-for-likely-recordings",
                    Some(Icon::Search),
                    words::matching::search(),
                    words::matching::search_hint(),
                    kit::Tone::Outlined,
                ),
                |this, _, cx| this.look_for_likely_recordings(cx),
                cx,
            ));
        let found = div()
            .id("likely-recordings")
            .max_h(px(LIKELY_LIST_AT_MOST))
            .overflow_y_scroll()
            .track_scroll(&self.likely_scroll)
            .child(self.likely_rows(&likely, cx));
        let found = div()
            .relative()
            .child(found)
            .child(Scrollbars::of(cx).vertical("likely-scrollbar", self.likely_scroll.clone()));
        let isrc_heading = div()
            .id("matching-isrc-heading")
            .headed(3, words::matching::isrc_heading())
            .pt_2()
            .text_size(px(theme::text_sm()))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(rgb(theme::text()))
            .child(words::matching::isrc_heading());
        let coding = div()
            .flex()
            .items_center()
            .gap_2()
            .child(Self::typed_into(&self.typed_isrc, "isrc-field"))
            .child(self.in_the_pane_ring(
                kit::button(
                    "give-the-isrc",
                    Some(Icon::Check),
                    words::matching::give_it(),
                    words::matching::give_the_isrc_hint(),
                    kit::Tone::Outlined,
                ),
                |this, window, cx| this.isrc_given(window, cx),
                cx,
            ));
        let leaving = kit::sheet_actions().child(self.in_the_pane_ring(
            kit::button(
                "leave-the-matching",
                None,
                words::matching::cancel(),
                words::matching::leave_hint(key!(leave)),
                kit::Tone::Outlined,
            ),
            |this, window, cx| this.leave_the_matching(window, cx),
            cx,
        ));

        let card = kit::sheet()
            .id("matching-sheet")
            .dialog(words::matching::matching_label())
            .track_focus(&self.sheet_panels.holding(sheets::MATCHING_PANEL, cx))
            .gap_3()
            .w(px(theme::confirm_width() * 1.4))
            .max_w_full()
            .on_mouse_down_out(
                cx.listener(|this, _, window, cx| this.leave_the_matching(window, cx)),
            )
            .child(heading)
            .child(saying)
            .child(searching)
            .child(found)
            .child(isrc_heading)
            .child(coding)
            .child(leaving);

        kit::over_a_scrim(motion::lifted_in(card, "matching-sheet-arrives"))
    }
}

#[cfg(test)]
mod tests {
    use resonate_core::Mbid;
    use resonate_library::{Credit, Issued, RecordingRelease};

    use super::*;

    #[test]
    fn a_likely_recording_says_its_title_credit_length_and_the_release_it_is_on() {
        let recording = Recording {
            id: Mbid::new("b1a9c0de-1111-4222-8333-444455556666").expect("an mbid"),
            title: "Echoes".to_owned(),
            credit: vec![Credit {
                name: "Pink Floyd".to_owned(),
                joined_by: String::new(),
                mbid: None,
            }],
            length: Some(std::time::Duration::from_secs(1411)),
            isrcs: Vec::new(),
            releases: vec![RecordingRelease {
                id: Mbid::new("c2b8d1ef-2222-4333-8444-555566667777").expect("an mbid"),
                title: "Meddle".to_owned(),
                date: Some("1971-10-30".to_owned()),
                disc: Some(1),
                position: Some(6),
                issued: Issued::default(),
            }],
        };

        assert_eq!(
            likely_line(&recording),
            "Echoes · Pink Floyd · 23:31 · Meddle (1971)"
        );
    }
}
