use ahash::AHashSet;
use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::{PlaylistId, TrackId};
use resonate_library::Replaceable;

use crate::{
    icons::Icon,
    motion, theme, toast,
    views::{
        hint::Names as _,
        kit,
        root::RootView,
        scrollbar::Scrollbars,
        sheets,
        spoken::{self, Spoken as _},
    },
    words,
};

const ROWS_SHOWN_AT_MOST: f32 = 320.0;

#[derive(Clone, Debug)]
pub(crate) struct ReplacingSheet {
    pub(crate) playlist: PlaylistId,
    pub(crate) name: String,
    pub(crate) rows: Option<Vec<Replaceable>>,
    pub(crate) ticked: AHashSet<TrackId>,
}

impl ReplacingSheet {
    pub(crate) fn asking(playlist: PlaylistId, name: String) -> Self {
        Self {
            playlist,
            name,
            rows: None,
            ticked: AHashSet::new(),
        }
    }

    pub(crate) fn arrived(&mut self, rows: Vec<Replaceable>) {
        self.rows = Some(rows);
        self.ticked = self.tickable().collect();
    }

    fn tickable(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.rows
            .iter()
            .flatten()
            .filter(|row| row.can_be_ticked())
            .map(|row| row.track.id)
    }

    fn anything_tickable(&self) -> bool {
        self.tickable().next().is_some()
    }

    fn every_tickable_is_ticked(&self) -> bool {
        self.anything_tickable() && self.tickable().all(|track| self.ticked.contains(&track))
    }

    pub(crate) fn tick(&mut self, track: TrackId) {
        if !self.tickable().any(|tickable| tickable == track) {
            return;
        }
        if !self.ticked.remove(&track) {
            self.ticked.insert(track);
        }
    }

    pub(crate) fn tick_all(&mut self) {
        self.ticked = match self.every_tickable_is_ticked() {
            true => AHashSet::new(),
            false => self.tickable().collect(),
        };
    }

    pub(crate) fn ticked_rows(&self) -> Vec<Replaceable> {
        self.rows
            .iter()
            .flatten()
            .filter(|row| row.can_be_ticked() && self.ticked.contains(&row.track.id))
            .cloned()
            .collect()
    }
}

fn name_held_for(catalog: &resonate_library::Library, playlist: PlaylistId) -> Option<String> {
    match catalog.playlist(playlist) {
        Ok(held) => held.map(|held| held.name),
        Err(error) => {
            tracing::warn!(%error, "the name of a playlist could not be read");
            None
        }
    }
}

fn standing_of(row: &Replaceable) -> String {
    if row.pending {
        return words::replacing::pending().to_owned();
    }
    match (&row.held, row.release_track) {
        (Some(held), _) => words::replacing::held(held.codec.as_str()),
        (None, Some(_)) => words::replacing::wanted().to_owned(),
        (None, None) => words::replacing::not_matched().to_owned(),
    }
}

impl RootView {
    pub(crate) fn open_replacing(&mut self, playlist: PlaylistId, cx: &mut Context<Self>) {
        self.close_the_menu(cx);
        let pressed = self.press_a_lookup();
        let model = self.library.read(cx);
        let shelved = model.playlist_name(playlist);
        let unshelved = shelved.is_none();
        let catalog = model.catalog();
        self.replacing = Some(ReplacingSheet::asking(
            playlist,
            shelved.unwrap_or_default(),
        ));
        cx.notify();

        let reading = cx.background_executor().spawn(async move {
            let named = unshelved
                .then(|| name_held_for(&catalog, playlist))
                .flatten();
            (catalog.lossy_songs_of(playlist), named)
        });
        cx.spawn(async move |this, cx| {
            let (read, named) = reading.await;
            let _ = this.update(cx, |this, cx| {
                this.the_lossy_songs_read(playlist, pressed, read, named, cx);
            });
        })
        .detach();
    }

    fn the_lossy_songs_read(
        &mut self,
        playlist: PlaylistId,
        pressed: crate::serial::Serial,
        read: resonate_library::Result<Vec<Replaceable>>,
        named: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let still_asked = self.attention == pressed
            && self
                .replacing
                .as_ref()
                .is_some_and(|sheet| sheet.playlist == playlist && sheet.rows.is_none());
        if !still_asked {
            return;
        }
        match read {
            Ok(rows) => {
                if let Some(sheet) = self.replacing.as_mut() {
                    sheet.arrived(rows);
                    if let Some(name) = named {
                        sheet.name = name;
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, "the lossy songs of a playlist could not be read");
                self.replacing = None;
                toast::tell(
                    toast::could_not(words::root::read_that_playlist(), &error),
                    cx,
                );
            }
        }
        cx.notify();
    }

    pub(crate) fn keep_the_lossy_songs(&mut self, cx: &mut Context<Self>) {
        self.replacing = None;
        cx.notify();
    }

    fn replace_the_ticked_songs(&mut self, cx: &mut Context<Self>) {
        let Some(sheet) = self.replacing.take() else {
            return;
        };
        let chosen = sheet.ticked_rows();
        cx.notify();
        self.library
            .update(cx, |library, cx| library.replace_lossy_songs(chosen, cx));
    }

    fn tick_a_lossy_song(&mut self, track: TrackId, cx: &mut Context<Self>) {
        if let Some(sheet) = self.replacing.as_mut() {
            sheet.tick(track);
            cx.notify();
        }
    }

    fn tick_every_lossy_song(&mut self, cx: &mut Context<Self>) {
        if let Some(sheet) = self.replacing.as_mut() {
            sheet.tick_all();
            cx.notify();
        }
    }

    pub(crate) fn replacing_sheet(&self, cx: &mut Context<Self>) -> Div {
        let Some(sheet) = self.replacing.as_ref() else {
            return div();
        };
        let heading = match sheet.name.as_str() {
            "" => words::replacing::heading_unnamed().to_owned(),
            name => words::replacing::heading(name),
        };

        let card = kit::sheet()
            .id("replacing-sheet")
            .debug_selector(|| "replacing-sheet".to_owned())
            .dialog(&heading)
            .track_focus(&self.sheet_panels.holding(sheets::REPLACING_PANEL, cx))
            .w(px(theme::confirm_width() * 1.4))
            .max_w_full()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.keep_the_lossy_songs(cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        kit::sheet_heading(
                            "replacing-heading",
                            Icon::Fold,
                            theme::accent(),
                            heading,
                        )
                        .flex_1()
                        .min_w_0(),
                    )
                    .child(self.tick_all_box(sheet, cx)),
            )
            .child(self.replacing_rows(sheet, cx))
            .child(
                div()
                    .id("replacing-trash-note")
                    .read_plainly(words::replacing::trash_note())
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(words::replacing::trash_note()),
            )
            .child(
                kit::sheet_actions()
                    .child(self.in_the_pane_ring(
                        kit::button(
                            sheets::KEEP_THE_LOSSY_SONGS,
                            None,
                            words::replacing::keep(),
                            words::replacing::keep_hint(key!(leave)),
                            kit::Tone::Outlined,
                        ),
                        |this, _, cx| this.keep_the_lossy_songs(cx),
                        cx,
                    ))
                    .child(self.replace_button(sheet, cx)),
            );

        kit::over_a_scrim(motion::lifted_in(card, "replacing-sheet-arrives"))
    }

    fn tick_all_box(&self, sheet: &ReplacingSheet, cx: &mut Context<Self>) -> Stateful<Div> {
        let press = match sheet.anything_tickable() {
            true => kit::Press::Takes,
            false => kit::Press::Greyed,
        };
        let tick_all = kit::checkbox_when(
            press,
            "replacing-tick-all",
            words::replacing::tick_all(),
            sheet.every_tickable_is_ticked(),
        )
        .names(words::replacing::tick_all_hint());
        match press {
            kit::Press::Takes => {
                self.in_the_pane_ring(tick_all, |this, _, cx| this.tick_every_lossy_song(cx), cx)
            }
            kit::Press::Greyed => tick_all,
        }
    }

    fn replace_button(&self, sheet: &ReplacingSheet, cx: &mut Context<Self>) -> Stateful<Div> {
        let count = sheet.ticked.len();
        let press = match count {
            0 => kit::Press::Greyed,
            _ => kit::Press::Takes,
        };
        let button = kit::button_when(
            press,
            "replace-the-lossy-songs",
            Some(Icon::Fold),
            words::replacing::replace(count),
            words::replacing::replace_hint(),
            kit::Tone::Primary,
        );
        match press {
            kit::Press::Takes => {
                self.in_the_pane_ring(button, |this, _, cx| this.replace_the_ticked_songs(cx), cx)
            }
            kit::Press::Greyed => button,
        }
    }

    fn replacing_rows(&self, sheet: &ReplacingSheet, cx: &mut Context<Self>) -> Div {
        let said = |words: &'static str| {
            div()
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .child(words)
        };
        let rows = match sheet.rows.as_deref() {
            None => return said(words::replacing::reading()),
            Some([]) => return said(words::replacing::nothing_lossy()),
            Some(rows) => rows,
        };

        let listed = rows.iter().fold(
            div()
                .id("replacing-rows")
                .flex()
                .flex_col()
                .gap_0p5()
                .max_h(px(ROWS_SHOWN_AT_MOST))
                .overflow_y_scroll()
                .track_scroll(&self.scrolls.replacing),
            |listed, row| {
                listed.child(self.replacing_row(row, sheet.ticked.contains(&row.track.id), cx))
            },
        );
        div().relative().child(listed).child(
            Scrollbars::of(cx).vertical("replacing-scrollbar", self.scrolls.replacing.clone()),
        )
    }

    fn replacing_row(&self, row: &Replaceable, ticked: bool, cx: &mut Context<Self>) -> Div {
        let track = row.track.id;
        let standing = standing_of(row);
        let press = match row.can_be_ticked() {
            true => kit::Press::Takes,
            false => kit::Press::Greyed,
        };
        let ticking = kit::checkbox_when(
            press,
            SharedString::from(format!("replacing-row-{track}")),
            spoken::song(&row.track.title, row.track.artist.as_deref()),
            ticked,
        )
        .names(standing.clone())
        .flex_1()
        .min_w_0();
        let ticking = match press {
            kit::Press::Takes => self.in_the_pane_ring(
                ticking,
                move |this, _, cx| this.tick_a_lossy_song(track, cx),
                cx,
            ),
            kit::Press::Greyed => ticking,
        };

        div()
            .flex()
            .items_center()
            .gap_3()
            .min_w_0()
            .overflow_hidden()
            .child(ticking)
            .child(kit::format_badge(row.track.codec, row.track.spec))
            .child(
                div()
                    .debug_selector(move || format!("replacing-standing-{track}"))
                    .flex_none()
                    .text_size(px(theme::text_xs()))
                    .text_color(rgb(match press {
                        kit::Press::Takes => theme::muted(),
                        kit::Press::Greyed => theme::faint(),
                    }))
                    .child(SharedString::from(standing)),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use resonate_core::{
        ChannelLayout, MediaLocation, PlaylistId, ReleaseTrackId, SampleFormat, SampleRate,
        StreamSpec, TrackId,
    };
    use resonate_library::{Codec, Replaceable, Track};

    use super::*;

    fn track(id: u64, codec: Codec) -> Track {
        Track {
            explicit: None,
            id: TrackId::new(id).expect("a valid id"),
            location: MediaLocation::local(format!("/music/{id}.opus")),
            title: format!("Song {id}"),
            artist: Some("Pink Floyd".to_owned()),
            artist_id: None,
            album_id: None,
            track_number: None,
            disc_number: None,
            duration: None,
            spec: StreamSpec::new(
                SampleRate::HZ_48000,
                ChannelLayout::Stereo,
                SampleFormat::F32,
            ),
            codec,
            replay_gain: Default::default(),
            file_size: 0,
            modified: SystemTime::UNIX_EPOCH,
            added: SystemTime::UNIX_EPOCH,
            plays: 0,
            played: None,
            span: None,
            favourite: None,
            genre: None,
            hidden: false,
            alternatives: 0,
            delivered: false,
        }
    }

    fn row(id: u64, held: bool, matched: bool, pending: bool) -> Replaceable {
        Replaceable {
            track: track(id, Codec::Opus),
            held: held.then(|| track(id + 100, Codec::Flac)),
            release_track: matched.then(|| ReleaseTrackId::new(id).expect("a valid row")),
            pending,
        }
    }

    #[test]
    fn a_row_says_why_it_stands_naming_the_codec_held_with_no_article() {
        let mut alac = row(1, true, false, false);
        if let Some(held) = alac.held.as_mut() {
            held.codec = Codec::Alac;
        }
        assert_eq!(standing_of(&alac), "you hold it in ALAC");
        assert_eq!(
            standing_of(&row(2, false, true, false)),
            "wanted through MusicBrainz"
        );
        assert_eq!(
            standing_of(&row(3, false, true, true)),
            "already being replaced"
        );
    }

    #[test]
    fn every_row_that_can_be_ticked_starts_ticked_and_tick_all_turns_them_together() {
        let held = row(1, true, false, false);
        let wanted = row(2, false, true, false);
        let pending = row(3, false, true, true);
        let ids = [held.track.id, wanted.track.id, pending.track.id];
        let mut sheet =
            ReplacingSheet::asking(PlaylistId::new(1).expect("a valid id"), "Road trip".into());
        sheet.arrived(vec![held, wanted, pending]);

        assert_eq!(sheet.ticked, [ids[0], ids[1]].into_iter().collect());

        sheet.tick_all();
        assert!(sheet.ticked.is_empty());

        sheet.tick_all();
        assert_eq!(sheet.ticked, [ids[0], ids[1]].into_iter().collect());

        sheet.tick(ids[2]);
        assert_eq!(sheet.ticked, [ids[0], ids[1]].into_iter().collect());

        sheet.tick(ids[0]);
        assert_eq!(
            sheet
                .ticked_rows()
                .iter()
                .map(|row| row.track.id)
                .collect::<Vec<_>>(),
            [ids[1]]
        );
    }
}
