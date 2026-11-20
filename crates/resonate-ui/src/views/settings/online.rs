use std::{
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui::{Context, SharedString, div, prelude::*};
use resonate_core::Switch;
use resonate_listen::Listening;

use crate::{
    MirrorStep, MirrorTold, ResonateApp, Setting,
    icons::Icon,
    views::{
        kit,
        root::RootView,
        settings::{
            Armed, Choice, Rows, SettingsPane, action, note, progress, reading, switch_row,
        },
    },
    words,
};

const MIRROR_REDRAWN_EVERY: Duration = Duration::from_secs(1);

const BYTES_IN_A_GIGABYTE: f64 = 1_000_000_000.0;
const WHOLE_PERCENT: f64 = 100.0;

fn step_said(step: MirrorStep) -> &'static str {
    match step {
        MirrorStep::Waiting => words::settings_online::mirror_step_waiting(),
        MirrorStep::Downloading => words::settings_online::mirror_step_downloading(),
        MirrorStep::Verifying => words::settings_online::mirror_step_verifying(),
        MirrorStep::Reading => words::settings_online::mirror_step_reading(),
        MirrorStep::Indexing => words::settings_online::mirror_step_indexing(),
        MirrorStep::Placing => words::settings_online::mirror_step_placing(),
        MirrorStep::Writing => words::settings_online::mirror_step_writing(),
        MirrorStep::Settling => words::settings_online::mirror_step_settling(),
    }
}

fn gigabytes(bytes: u64) -> String {
    resonate_words::decimal(bytes as f64 / BYTES_IN_A_GIGABYTE, 1)
}

fn step_standing(taken: MirrorStep, step: MirrorStep, done: Option<(u64, u64)>) -> (String, f32) {
    let (place, at) = (taken.place(), step.place());
    if place < at {
        return (words::settings_online::mirror_step_done().to_owned(), 1.0);
    }
    if place > at {
        return (
            words::settings_online::mirror_step_to_come().to_owned(),
            0.0,
        );
    }
    if step == MirrorStep::Waiting {
        return (words::settings_online::mirror_step_waits().to_owned(), 0.0);
    }
    match done {
        Some((read, of)) if of > 0 => {
            let read = read.min(of);
            let share = read as f64 / of as f64;
            let said = if taken.counts_bytes() {
                words::settings_online::mirror_step_bytes(gigabytes(of), gigabytes(read))
            } else {
                words::settings_online::mirror_step_share((share * WHOLE_PERCENT).floor() as u64)
            };
            (said, share as f32)
        }
        _ => (
            words::settings_online::mirror_step_under_way().to_owned(),
            0.0,
        ),
    }
}

fn mirror_steps(step: MirrorStep, done: Option<(u64, u64)>) -> gpui::Div {
    MirrorStep::TAKEN
        .into_iter()
        .fold(div().flex().flex_col().gap_2(), |steps, taken| {
            let (said, share) = step_standing(taken, step, done);
            steps.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(reading(step_said(taken), said))
                    .child(progress(share)),
            )
        })
}

fn mirror_said(told: &MirrorTold) -> String {
    match told {
        MirrorTold::Absent => words::settings_online::mirror_absent().to_owned(),
        MirrorTold::Building { step, .. } => {
            words::settings_online::mirror_building(step_said(*step))
        }
        MirrorTold::Held {
            dump,
            bytes,
            amended_at: None,
        } => words::settings_online::mirror_held(
            dump.to_string(),
            resonate_words::decimal(*bytes as f64 / BYTES_IN_A_GIGABYTE, 1),
        ),
        MirrorTold::Held {
            dump,
            bytes,
            amended_at: Some(at),
        } => words::settings_online::mirror_held_and_followed(
            dump.to_string(),
            resonate_words::decimal(*bytes as f64 / BYTES_IN_A_GIGABYTE, 1),
            resonate_core::elapsed::since(UNIX_EPOCH + Duration::from_secs(*at), SystemTime::now()),
        ),
        MirrorTold::Stopped => words::settings_online::mirror_stopped().to_owned(),
        MirrorTold::Failed(why) => words::settings_online::mirror_failed(why.to_string()),
    }
}

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
            Self::Desktop => words::settings_online::the_desktop(),
            Self::Microphone => words::settings_online::a_microphone(),
        }
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self {
            Self::Desktop => words::settings_online::listen_names_what_this_machine_is(),
            Self::Microphone => words::settings_online::listen_names_what_is_playing_in(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClipLength(Duration);

impl Choice for ClipLength {
    const ALL: &'static [Self] = &[
        Self(Duration::from_secs(8)),
        Self(Duration::from_secs(12)),
        Self(Duration::from_secs(20)),
    ];

    fn label(self) -> &'static str {
        match self.0.as_secs() {
            8 => "8 s",
            12 => "12 s",
            20 => "20 s",
            _ => words::settings_online::other(),
        }
    }

    fn in_force(self) -> SharedString {
        SharedString::from(format!("{} s", self.0.as_secs()))
    }

    fn meaning(self) -> SharedString {
        SharedString::new_static(match self.0.as_secs() {
            8 => words::settings_online::quickest_to_answer_enough_for_a(),
            12 => words::settings_online::enough_for_most_songs_even_over(),
            20 => words::settings_online::the_best_chance_with_a_noisy(),
            _ => words::settings_online::a_length_of_your_own_set(),
        })
    }
}

impl RootView {
    pub(super) fn lookups_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let library = self.library.read(cx);
        let on = library.is_online();
        let reachable = library.has_reference();
        let explore = !cx.global::<ResonateApp>().online.explore_disabled;

        let rows = kit::rows()
            .child(self.in_the_ring(
                "reference-lookups",
                switch_row(
                    words::settings_online::reach_the_network(),
                    words::settings_online::off_the_library_is_what_the(),
                    on,
                    "reference-lookups",
                ),
                move |this, _, cx| this.set_online(!on, cx),
                cx,
            ))
            .child(self.in_the_ring(
                "show-explore",
                switch_row(
                    words::suggestions::show_explore(),
                    words::suggestions::show_explore_detail(),
                    explore,
                    "show-explore",
                ),
                move |this, _, cx| this.set_explore(!explore, cx),
                cx,
            ))
            .when(on && !reachable, |body| {
                body.child(note(words::settings_online::next_start_note()))
            })
            .note(words::settings_online::lookups_note());

        self.mirror_rows(rows, cx)
    }

    fn mirror_rows(&mut self, rows: Rows, cx: &mut Context<Self>) -> Rows {
        let mirrors = cx.global::<ResonateApp>().online.musicbrainz_mirror;
        let offers = cx.global::<ResonateApp>().online.offer_musicbrainz_copy;
        let told = cx
            .global::<ResonateApp>()
            .mirroring
            .as_ref()
            .map(|mirroring| mirroring.told());
        let building = matches!(told, Some(MirrorTold::Building { .. }));
        if building {
            self.settings_pane.update(cx, |pane, cx| {
                pane.remotely.mirror_redraws.beats_while_drawn(
                    MIRROR_REDRAWN_EVERY,
                    |pane: &mut SettingsPane| &mut pane.remotely.mirror_redraws,
                    cx,
                );
            });
        }
        let held = matches!(told, Some(MirrorTold::Held { .. }));

        rows.child(self.in_the_ring(
            "musicbrainz-mirror",
            switch_row(
                words::settings_online::keep_a_copy_of_musicbrainz_here(),
                words::settings_online::off_musicbrainz_is_asked_over_the(),
                mirrors,
                "musicbrainz-mirror",
            ),
            move |this, _, cx| this.set_musicbrainz_mirror(!mirrors, cx),
            cx,
        ))
        .when_some(told.filter(|_| mirrors), |body, told| {
            body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(note(mirror_said(&told)))
                    .when_some(
                        match told {
                            MirrorTold::Building { step, done } => Some((step, done)),
                            _ => None,
                        },
                        |body, (step, done)| body.child(mirror_steps(step, done)),
                    )
                    .child(
                        kit::action_row()
                            .child(action(
                                "musicbrainz-mirror-build",
                                if held {
                                    words::settings_online::build_it_again()
                                } else {
                                    words::settings_online::build_it_now()
                                },
                                Icon::Download,
                                building,
                                |_, _, cx| {
                                    if let Some(mirroring) =
                                        cx.global::<ResonateApp>().mirroring.clone()
                                    {
                                        mirroring.build();
                                    }
                                    cx.notify();
                                },
                                self,
                                cx,
                            ))
                            .when(building, |row| {
                                row.child(action(
                                    "musicbrainz-mirror-stop",
                                    words::settings_online::stop_building(),
                                    Icon::Stop,
                                    false,
                                    |_, _, cx| {
                                        if let Some(mirroring) =
                                            cx.global::<ResonateApp>().mirroring.clone()
                                        {
                                            mirroring.cancel();
                                        }
                                        cx.notify();
                                    },
                                    self,
                                    cx,
                                ))
                            })
                            .when(held, |row| {
                                row.child(action(
                                    "musicbrainz-mirror-remove",
                                    words::settings_online::remove_the_copy(),
                                    Icon::Delete,
                                    building,
                                    |_, _, cx| {
                                        if let Some(mirroring) =
                                            cx.global::<ResonateApp>().mirroring.clone()
                                        {
                                            mirroring.remove();
                                        }
                                        cx.notify();
                                    },
                                    self,
                                    cx,
                                ))
                            }),
                    ),
            )
        })
        .when(mirrors, |body| {
            body.child(self.in_the_ring(
                "offer-musicbrainz-copy",
                switch_row(
                    words::settings_online::offer_the_copy_to_your_phones(),
                    words::settings_online::off_a_phone_asks_this_copy_one(),
                    offers,
                    "offer-musicbrainz-copy",
                ),
                move |this, _, cx| this.set_offer_musicbrainz_copy(!offers, cx),
                cx,
            ))
        })
        .when(mirrors, |body| {
            body.child(
                kit::field(
                    "MetaBrainz",
                    self.text_field(
                        "musicbrainz-replication-token",
                        &self.settings_fields.read(cx).replication.clone(),
                        |this, window, cx| this.leave_replication_token(window, cx),
                        cx,
                    ),
                )
                .child(note(words::settings_online::replication_note())),
            )
        })
        .note(words::settings_online::mirror_note())
    }

    pub(crate) fn set_musicbrainz_mirror(&mut self, mirrors: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.musicbrainz_mirror = mirrors);
        self.store(&Setting::Switched(Switch::MusicbrainzMirror, mirrors), cx);
        cx.notify();
    }

    pub(crate) fn set_offer_musicbrainz_copy(&mut self, offers: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.offer_musicbrainz_copy = offers;
        });
        self.store(&Setting::Switched(Switch::OfferMusicbrainzCopy, offers), cx);
        cx.notify();
    }

    pub(super) fn after_scan_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let after_scan = self.library.read(cx).enriches_after_scan();

        kit::rows().child(self.in_the_ring(
            "after-a-scan",
            switch_row(
                words::settings_online::ask_as_soon_as_a_scan(),
                words::settings_online::off_the_reference_is_asked_only(),
                after_scan,
                "after-a-scan",
            ),
            move |this, _, cx| this.set_after_scan(!after_scan, cx),
            cx,
        ))
    }

    pub(super) fn metered_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let online = &cx.global::<ResonateApp>().online;
        let enriches = online.enrich_on_metered;
        let downloads = online.download_on_metered;
        let waiting = self.library.read(cx).is_waiting_on_the_meter();

        kit::rows()
            .child(self.in_the_ring(
                "enrich-on-metered",
                switch_row(
                    words::settings_online::look_things_up_on_a_metered(),
                    words::settings_online::off_the_reference_is_not_asked(),
                    enriches,
                    "enrich-on-metered",
                ),
                move |this, _, cx| this.set_enrich_on_metered(!enriches, cx),
                cx,
            ))
            .child(self.in_the_ring(
                "download-on-metered",
                switch_row(
                    words::settings_online::download_on_a_metered_connection(),
                    words::settings_online::off_a_providers_downloads_wait_for(),
                    downloads,
                    "download-on-metered",
                ),
                move |this, _, cx| this.set_download_on_metered(!downloads, cx),
                cx,
            ))
            .when(waiting, |body| {
                body.child(note(words::settings_online::waiting_on_the_meter()))
            })
            .note(words::settings_online::metered_note())
    }

    pub(crate) fn set_enrich_on_metered(&mut self, enriches: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.enrich_on_metered = enriches);
        self.store(&Setting::Switched(Switch::EnrichOnMetered, enriches), cx);
        cx.notify();
    }

    pub(crate) fn set_download_on_metered(&mut self, downloads: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| {
            global.online.download_on_metered = downloads;
        });
        self.store(&Setting::Switched(Switch::DownloadOnMetered, downloads), cx);
        cx.notify();
    }

    pub(super) fn studies_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let studies = self.library.read(cx).studies();
        let by_sound = cx.global::<ResonateApp>().by_sound.load(Ordering::Acquire);

        kit::rows()
            .child(self.in_the_ring(
                "studying-tracks",
                switch_row(
                    words::settings_online::study_every_track_as_a_lookup(),
                    words::settings_online::off_a_track_is_decoded_only(),
                    studies,
                    "studying-tracks",
                ),
                move |this, _, cx| this.set_studies(!studies, cx),
                cx,
            ))
            .child(self.in_the_ring(
                "identifying-by-sound",
                switch_row(
                    words::settings_online::name_a_track_nothing_else_can(),
                    words::settings_online::off_no_tracks_sound_is_sent(),
                    by_sound,
                    "identifying-by-sound",
                ),
                move |this, _, cx| this.set_by_sound(!by_sound, cx),
                cx,
            ))
            .note(words::settings_online::by_sound_note())
    }

    pub(crate) fn set_by_sound(&mut self, by_sound: bool, cx: &mut Context<Self>) {
        cx.global::<ResonateApp>()
            .by_sound
            .store(by_sound, Ordering::Release);
        self.store(&Setting::Switched(Switch::IdentifyBySound, by_sound), cx);
        cx.notify();
    }

    pub(super) fn lyrics_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let lyrics = self.library.read(cx).fetches_lyrics();
        let by_the_locale = cx
            .global::<ResonateApp>()
            .lyrics_by_the_locale
            .load(Ordering::Acquire);

        kit::rows()
            .child(self.in_the_ring(
                "fetching-lyrics",
                switch_row(
                    words::settings_online::fetch_lyrics_for_every_track_as(),
                    words::settings_online::off_words_are_asked_for_only(),
                    lyrics,
                    "fetching-lyrics",
                ),
                move |this, _, cx| this.set_lyrics(!lyrics, cx),
                cx,
            ))
            .child(self.in_the_ring(
                "lyrics-by-the-locale",
                switch_row(
                    words::settings_online::choose_a_lyric_sheet_in_the(),
                    words::settings_online::off_of_sheets_in_several_languages(),
                    by_the_locale,
                    "lyrics-by-the-locale",
                ),
                move |this, _, cx| this.set_lyrics_by_the_locale(!by_the_locale, cx),
                cx,
            ))
    }

    pub(crate) fn set_lyrics_by_the_locale(&mut self, by_the_locale: bool, cx: &mut Context<Self>) {
        cx.global::<ResonateApp>()
            .lyrics_by_the_locale
            .store(by_the_locale, Ordering::Release);
        self.store(
            &Setting::Switched(Switch::LyricsLanguageFromLocale, by_the_locale),
            cx,
        );
        cx.notify();
    }

    pub(super) fn contact_group(&mut self, cx: &mut Context<Self>) -> Rows {
        kit::rows()
            .child(self.text_field(
                "contact",
                &self.settings_fields.read(cx).contact.clone(),
                |this, window, cx| this.leave_contact(window, cx),
                cx,
            ))
            .note(words::settings_online::contact_note())
    }

    pub(super) fn recognition_group(&mut self, cx: &mut Context<Self>) -> Rows {
        kit::rows()
            .child(
                kit::field(
                    "AcoustID",
                    self.text_field(
                        "acoustid-key",
                        &self.settings_fields.read(cx).acoustid.clone(),
                        |this, window, cx| this.leave_acoustid_key(window, cx),
                        cx,
                    ),
                )
                .child(note(words::settings_online::key_note())),
            )
            .child(
                kit::field(
                    "AudD",
                    self.text_field(
                        "audd-token",
                        &self.settings_fields.read(cx).audd.clone(),
                        |this, window, cx| this.leave_audd_token(window, cx),
                        cx,
                    ),
                )
                .child(note(words::settings_online::token_note())),
            )
    }

    pub(super) fn submitting_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let given = !self
            .settings_fields
            .read(cx)
            .listenbrainz
            .read(cx)
            .text()
            .trim()
            .is_empty();
        let armed = self.is_armed(&Armed::TellTheEarlierPlays, cx);
        let asked = self.earlier_plays_asked;

        kit::rows()
            .child(kit::field(
                words::settings_online::user_token(),
                self.text_field(
                    "listenbrainz-token",
                    &self.settings_fields.read(cx).listenbrainz.clone(),
                    |this, window, cx| this.leave_listenbrainz_token(window, cx),
                    cx,
                ),
            ))
            .when(given, |body| {
                body.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(kit::action_row().child(action(
                            "send-the-earlier-plays",
                            if armed {
                                words::settings_online::press_again_to_send_every_earlier()
                            } else {
                                words::settings_online::send_earlier_plays()
                            },
                            Icon::Export,
                            asked,
                            move |this, _, cx| {
                                if armed {
                                    this.send_the_earlier_plays(cx);
                                } else {
                                    this.arm(Armed::TellTheEarlierPlays, cx);
                                    cx.notify();
                                }
                            },
                            self,
                            cx,
                        )))
                        .child(note(if asked {
                            words::settings_online::earlier_plays_asked()
                        } else {
                            words::settings_online::earlier_plays_note()
                        })),
                )
            })
            .note(words::settings_online::submitting_note())
    }

    fn send_the_earlier_plays(&mut self, cx: &mut Context<Self>) {
        self.lower(&Armed::TellTheEarlierPlays, cx);
        self.earlier_plays_asked = self
            .library
            .update(cx, |library, cx| library.tell_earlier_listens(cx));
        cx.notify();
    }

    pub(super) fn listening_group(&mut self, cx: &mut Context<Self>) -> Rows {
        let listen = self.listen.read(cx);
        let from = HeardFrom::of(listen.from());
        let length = ClipLength(listen.length());

        kit::rows()
            .child(kit::field(
                words::settings_online::listen_to(),
                self.choices("listen-from", from, cx, |this, from: HeardFrom, cx| {
                    let listening = match from {
                        HeardFrom::Desktop => Listening::Desktop,
                        HeardFrom::Microphone => Listening::Microphone(None),
                    };
                    this.listen_from(listening, cx);
                }),
            ))
            .child(kit::field(
                words::settings_online::r#for(),
                self.choices("listen-for", length, cx, |this, length: ClipLength, cx| {
                    this.listen_for(length.0, cx);
                }),
            ))
            .note(words::settings_online::listening_note())
    }
}

impl RootView {
    pub(crate) fn set_explore(&mut self, enabled: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.explore_disabled = !enabled);
        self.clear_explore(cx);
        self.store(&Setting::Switched(Switch::Explore, enabled), cx);
        cx.refresh_windows();
    }

    pub(crate) fn set_online(&mut self, on: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.enabled = on);
        self.library
            .update(cx, |library, cx| library.set_online(on, cx));
        self.analysis.update(cx, |analysis, cx| {
            analysis.reach(on);
            cx.notify();
        });
        self.store(&Setting::Switched(Switch::Online, on), cx);
    }

    pub(crate) fn set_after_scan(&mut self, after_scan: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.after_scan = after_scan);
        self.library
            .update(cx, |library, cx| library.set_after_scan(after_scan, cx));
        self.store(&Setting::Switched(Switch::EnrichAfterScan, after_scan), cx);
    }

    pub(crate) fn set_studies(&mut self, studies: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.studies = studies);
        self.library
            .update(cx, |library, cx| library.set_studies(studies, cx));
        self.store(&Setting::Switched(Switch::Study, studies), cx);
    }

    pub(crate) fn set_lyrics(&mut self, lyrics: bool, cx: &mut Context<Self>) {
        cx.update_global::<ResonateApp, _>(|global, _| global.online.lyrics = lyrics);
        self.library
            .update(cx, |library, cx| library.set_lyrics(lyrics, cx));
        self.store(&Setting::Switched(Switch::FetchLyrics, lyrics), cx);
    }
}
