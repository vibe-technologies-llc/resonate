use std::time::Duration;

use gpui::{
    ClickEvent, Context, Div, Entity, SharedString, Stateful, Window, div, prelude::*, rgb,
};
use resonate_library::EnrichStats;
use resonate_listen::Listening;

use crate::{
    ResonateApp, Setting,
    icons::Icon,
    theme,
    views::{
        field::Field,
        kit, listing,
        root::RootView,
        settings::{Choice, action, note, switch_row},
    },
};

const LOOKUPS_NOTE: &str = "MusicBrainz for what a release and an artist are, the Cover Art \
                            Archive for a cover, Wikimedia Commons for a portrait and LRCLIB for \
                            lyrics. Nothing sent identifies you unless a contact is given below.";

const NEXT_START_NOTE: &str = "Turned on for the next start; nothing is reached until then.";

const KEY_NOTE: &str = "Used from the next start by the lookup, which names by ear what nothing \
                        else could, and by the Analysis pane. Leave it empty to recognise nothing.";

const TOKEN_NOTE: &str = "Used from the next start by Listen, which asks AudD beside Shazam. \
                          Leave it empty to ask Shazam alone.";

const LISTENING_NOTE: &str = "Listen names a song playing on the desktop or into a microphone, \
                              whether or not the library holds it. A microphone named in the \
                              Listen sheet is kept here too.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeardFrom {
    Desktop,
    Microphone,
}

impl HeardFrom {
    const fn of(from: &Listening) -> Self {
        match from {
            Listening::Desktop => Self::Desktop,
            Listening::Microphone(_) => Self::Microphone,
        }
    }
}

impl Choice for HeardFrom {
    const ALL: &'static [Self] = &[Self::Desktop, Self::Microphone];

    fn label(self) -> &'static str {
        match self {
            Self::Desktop => "The desktop",
            Self::Microphone => "A microphone",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Desktop => {
                "Listen names what this machine is playing — a video, a stream, another player \
                 — by recording the desktop's own output."
            }
            Self::Microphone => {
                "Listen names what is playing in the room, heard through the microphone chosen \
                 below."
            }
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClipLength {
    Brief,
    Usual,
    Long,
}

impl ClipLength {
    const fn held(self) -> Duration {
        Duration::from_secs(match self {
            Self::Brief => 8,
            Self::Usual => 12,
            Self::Long => 20,
        })
    }

    fn of(held: Duration) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|length| length.held() == held)
    }
}

impl Choice for ClipLength {
    const ALL: &'static [Self] = &[Self::Brief, Self::Usual, Self::Long];

    fn label(self) -> &'static str {
        match self {
            Self::Brief => "8 s",
            Self::Usual => "12 s",
            Self::Long => "20 s",
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Brief => "Quickest to answer; enough for a clear recording of a well-known song.",
            Self::Usual => "Enough for most songs, even over a little noise.",
            Self::Long => "The best chance with a noisy room, a quiet passage or a rarer song.",
        })
    }
}

const CONTACT_NOTE: &str = "Sent in the User-Agent from the next start. Leave it empty to send \
                            nothing.";

impl RootView {
    pub(super) fn lookups_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let on = library.is_online();
        let reachable = library.has_reference();

        kit::section_body()
            .child(self.in_the_ring(
                "reference-lookups",
                switch_row(
                    "Reach the network",
                    "Off, the library is what the files say",
                    on,
                    "reference-lookups",
                ),
                move |this, _, cx| this.set_online(!on, cx),
                cx,
            ))
            .child(note(LOOKUPS_NOTE))
            .when(on && !reachable, |body| body.child(note(NEXT_START_NOTE)))
    }

    pub(super) fn after_scan_group(&mut self, cx: &mut Context<Self>) -> Div {
        let after_scan = self.library.read(cx).enriches_after_scan();

        kit::section_body().child(self.in_the_ring(
            "after-a-scan",
            switch_row(
                "Ask as soon as a scan ends",
                "Off, the reference is asked only when Enrich is pressed",
                after_scan,
                "after-a-scan",
            ),
            move |this, _, cx| this.set_after_scan(!after_scan, cx),
            cx,
        ))
    }

    pub(super) fn studies_group(&mut self, cx: &mut Context<Self>) -> Div {
        let studies = self.library.read(cx).studies();

        kit::section_body().child(self.in_the_ring(
            "studying-tracks",
            switch_row(
                "Study every track as a lookup runs",
                "Off, a track is decoded only to be heard or named",
                studies,
                "studying-tracks",
            ),
            move |this, _, cx| this.set_studies(!studies, cx),
            cx,
        ))
    }

    pub(super) fn contact_group(&mut self, cx: &mut Context<Self>) -> Div {
        kit::section_body()
            .child(self.contact_field(cx))
            .child(note(CONTACT_NOTE))
    }

    pub(super) fn recognition_group(&mut self, cx: &mut Context<Self>) -> Div {
        kit::section_body()
            .child(kit::field(
                "AcoustID",
                self.key_field(
                    "acoustid-key",
                    &self.acoustid,
                    |this, window, cx| this.leave_acoustid_key(window, cx),
                    cx,
                ),
            ))
            .child(note(KEY_NOTE))
            .child(kit::field(
                "AudD",
                self.key_field(
                    "audd-token",
                    &self.audd,
                    |this, window, cx| this.leave_audd_token(window, cx),
                    cx,
                ),
            ))
            .child(note(TOKEN_NOTE))
    }

    pub(super) fn listening_group(&mut self, cx: &mut Context<Self>) -> Div {
        let listen = self.listen.read(cx);
        let from = HeardFrom::of(listen.from());
        let held = listen.length();
        let length = ClipLength::of(held);

        kit::section_body()
            .child(kit::field(
                "Listen to",
                self.choices(
                    "listen-from",
                    Some(from),
                    cx,
                    |this, from: HeardFrom, cx| {
                        let listening = match from {
                            HeardFrom::Desktop => Listening::Desktop,
                            HeardFrom::Microphone => Listening::Microphone(None),
                        };
                        this.listen_from(listening, cx);
                    },
                ),
            ))
            .child(kit::field(
                "For",
                self.choices("listen-for", length, cx, |this, length: ClipLength, cx| {
                    this.listen_for(length.held(), cx);
                }),
            ))
            .when(length.is_none(), |body| {
                body.child(note(format!(
                    "{} s is in force, which is none of these.",
                    held.as_secs()
                )))
            })
            .child(note(LISTENING_NOTE))
    }

    pub(super) fn key_field(
        &self,
        id: &'static str,
        field: &Entity<Field>,
        leave: fn(&mut Self, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let taking = field.clone();
        div()
            .id(id)
            .flex()
            .items_center()
            .gap_2()
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(gpui::px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .on_mouse_down_out(cx.listener(move |this, _, window, cx| leave(this, window, cx)))
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                taking.update(cx, |key, _| key.take_focus(window));
                cx.notify();
            }))
            .child(field.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    fn contact_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("contact")
            .flex()
            .items_center()
            .gap_2()
            .h(theme::width(theme::search_height()))
            .px_3()
            .rounded_lg()
            .bg(rgb(theme::background()))
            .border_1()
            .border_color(rgb(theme::border()))
            .text_size(gpui::px(theme::text_sm()))
            .cursor_text()
            .hover(|field| field.border_color(theme::tinted(theme::accent(), 0x99)))
            .on_mouse_down_out(cx.listener(|this, _, window, cx| this.leave_contact(window, cx)))
            .on_click(cx.listener(|this, _, window, cx| {
                this.contact
                    .update(cx, |contact, _| contact.take_focus(window));
                cx.notify();
            }))
            .child(self.contact.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    pub(super) fn look_up_group(&mut self, cx: &mut Context<Self>) -> Div {
        let library = self.library.read(cx);
        let on = library.is_online();
        let reachable = library.has_reference();
        let enriching = library.is_enriching();
        let stopping = library.is_stopping_enrich();
        let stats = library.enrich_stats();
        let notice = library.notice().cloned();
        let held_back = enriching || !(on && reachable);

        kit::section_body()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.look_up(held_back, enriching, cx))
                    .child(self.refresh_all(held_back, cx))
                    .when(enriching, |row| row.child(self.stop_lookup(stopping, cx))),
            )
            .when_some(stats, |body, stats| body.child(note(asked_so_far(stats))))
            .when_some(notice, |body, notice| body.child(listing::noticed(&notice)))
    }

    fn look_up(&self, held_back: bool, enriching: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "look-up",
            if enriching {
                "Looking up…"
            } else {
                "Look up"
            },
            Icon::Globe,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.enrich(false, cx));
            },
            self,
            cx,
        )
    }

    fn refresh_all(&self, held_back: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "refresh-all",
            "Refresh all",
            Icon::Redo,
            held_back,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.enrich(true, cx));
            },
            self,
            cx,
        )
    }

    fn stop_lookup(&self, stopping: bool, cx: &mut Context<Self>) -> Stateful<Div> {
        action(
            "stop-lookup",
            if stopping { "Stopping…" } else { "Stop" },
            Icon::Stop,
            stopping,
            |this, _, cx| {
                this.library
                    .update(cx, |library, cx| library.stop_enrich(cx));
            },
            self,
            cx,
        )
    }
}

impl RootView {
    pub(crate) fn set_online(&self, on: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.enabled = on);
        self.library
            .update(cx, |library, cx| library.set_online(on, cx));
        self.store(&Setting::Online(on), cx);
    }

    pub(crate) fn set_after_scan(&self, after_scan: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.after_scan = after_scan);
        self.library
            .update(cx, |library, cx| library.set_after_scan(after_scan, cx));
        self.store(&Setting::EnrichAfterScan(after_scan), cx);
    }

    pub(crate) fn set_studies(&self, studies: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.studies = studies);
        self.library
            .update(cx, |library, cx| library.set_studies(studies, cx));
        self.store(&Setting::Study(studies), cx);
    }
}

fn asked_so_far(stats: EnrichStats) -> String {
    format!(
        "albums {} · covers {} · tracks {} · named {} · artists {} · portraits {} · releases \
         found {} · refused {}",
        stats.albums,
        stats.covers,
        stats.tracks,
        stats.named,
        stats.artists,
        stats.portraits,
        stats.releases_found,
        stats.refused
    )
}
