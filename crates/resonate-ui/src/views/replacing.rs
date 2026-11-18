use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashSet;
use futures_channel::mpsc::unbounded;
use gpui::{Context, Div, SharedString, Stateful, div, prelude::*, px, rgb};
use resonate_core::{PlaylistId, TrackId};
use resonate_library::{Looked, LookingFor, Replaceable};

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
    pub(crate) looking: AHashSet<TrackId>,
    stop: Option<Arc<AtomicBool>>,
}

impl ReplacingSheet {
    pub(crate) fn asking(playlist: PlaylistId, name: String) -> Self {
        Self {
            playlist,
            name,
            rows: None,
            ticked: AHashSet::new(),
            looking: AHashSet::new(),
            stop: None,
        }
    }

    pub(crate) fn arrived(&mut self, rows: Vec<Replaceable>) {
        self.ticked = rows
            .iter()
            .filter(|row| row.can_be_ticked() && !row.is_offered_nowhere())
            .map(|row| row.track.id)
            .collect();
        self.rows = Some(rows);
    }

    fn lookable_rows(&self) -> Vec<Replaceable> {
        self.rows
            .iter()
            .flatten()
            .filter(|row| row.can_be_looked_for())
            .cloned()
            .collect()
    }

    fn anything_lookable(&self) -> bool {
        self.rows
            .iter()
            .flatten()
            .any(Replaceable::can_be_looked_for)
    }

    pub(crate) const fn is_looking(&self) -> bool {
        self.stop.is_some()
    }

    fn begin_looking(&mut self, stop: Arc<AtomicBool>, rows: &[Replaceable]) {
        self.looking = rows.iter().map(|row| row.track.id).collect();
        self.stop = Some(stop);
    }

    pub(crate) fn stop_looking(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Release);
        }
        self.looking.clear();
    }

    fn looks_with(&self, stop: &Arc<AtomicBool>) -> bool {
        self.stop
            .as_ref()
            .is_some_and(|held| Arc::ptr_eq(held, stop))
    }

    fn done_looking(&mut self) {
        self.stop = None;
        self.looking.clear();
    }

    pub(crate) fn told(&mut self, track: TrackId, looked: Looked) {
        self.looking.remove(&track);
        let Some(row) = self
            .rows
            .iter_mut()
            .flatten()
            .find(|row| row.track.id == track)
        else {
            return;
        };
        let found = matches!(looked, Looked::Offered { .. });
        let nowhere = looked == Looked::Unoffered;
        row.looked = Some(looked);
        if found && row.can_be_ticked() {
            self.ticked.insert(track);
        } else if nowhere {
            self.ticked.remove(&track);
        }
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

fn standing_of(row: &Replaceable, looking: bool) -> String {
    if row.pending {
        return words::replacing::pending().to_owned();
    }
    if let Some(held) = &row.held {
        return words::replacing::held(held.codec.as_str());
    }
    if looking {
        return words::replacing::looking().to_owned();
    }
    match (&row.looked, row.release_track) {
        (Some(Looked::Offered { by, .. }), _) => words::replacing::offered_by(by.as_str()),
        (Some(Looked::Unoffered), _) => words::replacing::offered_nowhere().to_owned(),
        (Some(Looked::Unheard), _) => words::replacing::unheard().to_owned(),
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
        if let Some(earlier) = self.replacing.as_mut() {
            earlier.stop_looking();
        }
        self.replacing = Some(ReplacingSheet::asking(
            playlist,
            shelved.unwrap_or_default(),
        ));
        cx.notify();

        let reading = cx.background_executor().spawn(async move {
            let named = unshelved
                .then(|| name_held_for(&catalog, playlist))
                .flatten();
            (catalog.songs_to_better_in(playlist), named)
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
        if let Some(mut sheet) = self.replacing.take() {
            sheet.stop_looking();
        }
        cx.notify();
    }

    fn replace_the_ticked_songs(&mut self, cx: &mut Context<Self>) {
        let Some(mut sheet) = self.replacing.take() else {
            return;
        };
        sheet.stop_looking();
        let chosen = sheet.ticked_rows();
        cx.notify();
        self.library
            .update(cx, |library, cx| library.replace_lossy_songs(chosen, cx));
    }

    pub(crate) fn look_for_lossless_copies(&mut self, cx: &mut Context<Self>) {
        if let Some(sheet) = self.replacing.as_mut().filter(|sheet| sheet.is_looking()) {
            sheet.stop_looking();
            cx.notify();
            return;
        }
        let model = self.library.read(cx);
        let Some(providers) = model.providers_to_look_through(cx) else {
            return;
        };
        let catalog = model.catalog();
        let Some(sheet) = self.replacing.as_mut() else {
            return;
        };
        let rows = sheet.lookable_rows();
        if rows.is_empty() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        sheet.begin_looking(Arc::clone(&stop), &rows);
        cx.notify();

        let (told, mut heard) = unbounded::<(TrackId, Looked)>();
        let stopped = Arc::clone(&stop);
        cx.spawn(async move |this, cx| {
            let looking = cx.background_executor().spawn(async move {
                let cancelled = || stopped.load(Ordering::Acquire);
                let tell = |track, looked| {
                    let _ = told.unbounded_send((track, looked));
                };
                catalog.look_for_replacements(
                    &providers,
                    &rows,
                    &LookingFor {
                        cancelled: &cancelled,
                        told: &tell,
                    },
                )
            });
            while let Ok(first) = heard.recv().await {
                let mut landed = vec![first];
                while let Ok(more) = heard.try_recv() {
                    landed.push(more);
                }
                let heard_by = this.update(cx, |this, cx| {
                    let Some(sheet) = this
                        .replacing
                        .as_mut()
                        .filter(|sheet| sheet.looks_with(&stop))
                    else {
                        return;
                    };
                    for (track, looked) in landed {
                        sheet.told(track, looked);
                    }
                    cx.notify();
                });
                if heard_by.is_err() {
                    return;
                }
            }
            let looked = looking.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = &looked {
                    tracing::warn!(%error, "the services could not be asked for lossless copies");
                    toast::tell(
                        toast::could_not(words::replacing::ask_the_services(), error),
                        cx,
                    );
                }
                if let Some(sheet) = this
                    .replacing
                    .as_mut()
                    .filter(|sheet| sheet.looks_with(&stop))
                {
                    sheet.done_looking();
                    cx.notify();
                }
            });
        })
        .detach();
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
                            Icon::Search,
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
                    .id("replacing-beside-note")
                    .read_plainly(words::replacing::beside_note())
                    .text_size(px(theme::text_sm()))
                    .text_color(rgb(theme::muted()))
                    .child(words::replacing::beside_note()),
            )
            .child(
                kit::sheet_actions()
                    .child(self.look_button(sheet, cx))
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

    fn look_button(&self, sheet: &ReplacingSheet, cx: &mut Context<Self>) -> Stateful<Div> {
        let (press, label, hint) = if sheet.is_looking() {
            (
                kit::Press::Takes,
                words::replacing::stop_looking(),
                words::replacing::stop_looking_hint(),
            )
        } else {
            let asked = self
                .library
                .read(cx)
                .providers_to_look_through(cx)
                .is_some();
            let press = match asked && sheet.anything_lookable() {
                true => kit::Press::Takes,
                false => kit::Press::Greyed,
            };
            let hint = match asked {
                true => words::replacing::look_hint(),
                false => words::replacing::look_unasked(),
            };
            (press, words::replacing::look(), hint)
        };
        let button = kit::button_when(
            press,
            "look-for-lossless-copies",
            Some(Icon::Search),
            label,
            hint,
            kit::Tone::Ghost,
        );
        match press {
            kit::Press::Takes => {
                self.in_the_pane_ring(button, |this, _, cx| this.look_for_lossless_copies(cx), cx)
            }
            kit::Press::Greyed => button,
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
            Some(Icon::Check),
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
                .track_scroll(&self.window_scrolls.replacing),
            |listed, row| listed.child(self.replacing_row(row, sheet, cx)),
        );
        div().relative().child(listed).child(
            Scrollbars::of(cx)
                .vertical("replacing-scrollbar", self.window_scrolls.replacing.clone()),
        )
    }

    fn replacing_row(
        &self,
        row: &Replaceable,
        sheet: &ReplacingSheet,
        cx: &mut Context<Self>,
    ) -> Div {
        let track = row.track.id;
        let ticked = sheet.ticked.contains(&track);
        let standing = standing_of(row, sheet.looking.contains(&track));
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
            .debug_selector(move || format!("replacing-line-{track}"))
            .flex()
            .flex_none()
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
            looked: None,
        }
    }

    #[test]
    fn a_row_says_why_it_stands_naming_the_codec_held_with_no_article() {
        let mut alac = row(1, true, false, false);
        if let Some(held) = alac.held.as_mut() {
            held.codec = Codec::Alac;
        }
        assert_eq!(standing_of(&alac, false), "you hold it in ALAC");
        assert_eq!(
            standing_of(&row(2, false, true, false), false),
            "wanted through MusicBrainz"
        );
        assert_eq!(
            standing_of(&row(3, false, true, true), false),
            "already being looked for"
        );
    }

    #[test]
    fn a_song_matched_nowhere_is_asked_about_and_ticked_once_a_service_offers_it() {
        let unmatched = row(5, false, false, false);
        let id = unmatched.track.id;
        let mut sheet =
            ReplacingSheet::asking(PlaylistId::new(1).expect("a valid id"), "Road trip".into());
        sheet.arrived(vec![unmatched]);
        assert!(sheet.ticked.is_empty());
        assert_eq!(sheet.lookable_rows().len(), 1);

        let by = resonate_core::SourceId::new("tidal").expect("a source name");
        sheet.told(
            id,
            Looked::Offered {
                at: resonate_core::MediaLocation::new(by.clone(), "track/9"),
                by,
            },
        );

        assert!(sheet.ticked.contains(&id));
        let rows = sheet.rows.clone().unwrap_or_default();
        assert_eq!(standing_of(&rows[0], false), "found lossless on tidal");
        assert_eq!(sheet.ticked_rows().len(), 1);
    }

    #[test]
    fn a_row_says_what_the_services_answered_and_one_offered_nowhere_is_left_unticked() {
        let found = row(1, false, true, false);
        let nowhere = row(2, false, true, false);
        let unheard = row(3, false, true, false);
        let ids = [found.track.id, nowhere.track.id, unheard.track.id];
        let mut kept_nowhere = row(4, false, true, false);
        kept_nowhere.looked = Some(Looked::Unoffered);
        let mut sheet =
            ReplacingSheet::asking(PlaylistId::new(1).expect("a valid id"), "Road trip".into());
        sheet.arrived(vec![found, nowhere, unheard, kept_nowhere]);
        assert_eq!(sheet.ticked, ids.into_iter().collect());

        sheet.begin_looking(Arc::new(AtomicBool::new(false)), &sheet.lookable_rows());
        let by = resonate_core::SourceId::new("shop").expect("a source name");
        let shop = Looked::Offered {
            at: resonate_core::MediaLocation::new(by.clone(), "track/1"),
            by,
        };
        assert_eq!(
            standing_of(&sheet.rows.clone().unwrap_or_default()[0], true),
            "checking…"
        );
        sheet.told(ids[0], shop);
        sheet.told(ids[1], Looked::Unoffered);
        sheet.told(ids[2], Looked::Unheard);

        let rows = sheet.rows.clone().unwrap_or_default();
        assert_eq!(standing_of(&rows[0], false), "found lossless on shop");
        assert_eq!(standing_of(&rows[1], false), "no lossless copy found");
        assert_eq!(standing_of(&rows[2], false), "not every service answered");
        assert_eq!(sheet.ticked, [ids[0], ids[2]].into_iter().collect());
        assert!(sheet.looking.contains(&rows[3].track.id));
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
