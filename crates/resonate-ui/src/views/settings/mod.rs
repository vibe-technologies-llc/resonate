mod about;
mod abx;
mod appearance;
pub(crate) mod curve;
mod defaults;
mod desktop;
mod equaliser;
mod filters;
mod find;
mod lastfm;
mod library;
mod online;
mod output;
mod processing;
pub(crate) mod spotify;
mod subsonic;
pub(crate) mod sync;
mod tidal;
mod youtube_music;

use std::{cell::Cell, path::PathBuf, rc::Rc, sync::atomic::Ordering};

pub(crate) use abx::{ListeningTest, Setup};
use gpui::{
    AnyElement, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, MouseButton, Pixels,
    Point, Role, SharedString, Stateful, Window, canvas, div, prelude::*, px, relative, rgb,
};
use resonate_core::{Appearance, Following, HeadYaw, SpeakerAngle, SpeakerElevation};
use resonate_library::{DEFAULT_LAYOUT, HistoryKept};
use resonate_listen::{CLIP_BY_DEFAULT, Listening};

pub(crate) use crate::views::{
    kit::{Rows, note, switch_row},
    settings::{
        curve::{HeldBand, Plotted},
        equaliser::{Fitting, marked_at},
        find::{Category, Group, Page},
        lastfm::{LastfmAccount, LastfmSigning},
        spotify::SpotifyState,
        subsonic::{Account, Service},
        sync::{
            SHARED_NAME_PLACEHOLDER, SYNC_CODE_PLACEHOLDER, SYNC_DEVICE_PLACEHOLDER,
            SYNC_SERVER_PLACEHOLDER,
        },
        tidal::{SigningIn, TidalAccount},
        youtube_music::{CheckingYoutubeMusic, SigningInToYoutubeMusic, YoutubeMusicField},
    },
};
use crate::{
    AppIcon, Followed, Measurers, ResonateApp, Setting, Synchronised,
    app::CONTROL_CONTEXT,
    icons::{self, Icon},
    motion, theme,
    views::{
        field::Field,
        focus::{self, by_the_pointer},
        hint::{self, Names},
        kit::{self, Press, Tone},
        root::RootView,
        scrollbar::Scrollbars,
        settings::{
            defaults::{Standing, can_be_put_back, differs, puts_back},
            find::{Drawn, Narrowing},
        },
        spoken::{self, Spoken},
    },
};

const CATEGORIES: &str = "Settings categories";

const BACK_HINT: &str = keyed!("Back to the rest of this category", key!(leave));

const CLEAR_THE_FILTER_HINT: &str = "Clear what is typed";

pub(crate) const FILTER_PLACEHOLDER: &str = "Find a setting…";

const REVERT_HINT: &str = "Put this back to what Resonate was built with, and take the key out of \
                           the settings file";

const EXPERIMENTAL: &str = "Experimental";

const RESET_HINT: &str = "Put every setting in this category back to what Resonate was built with";

const NOTHING_FOUND: &str = "No setting answers to that.";

type Place = (Category, Option<Page>);

const NOTHING_FOUND_MORE: &str = "A setting is found by its name, by the words its hint uses, or \
                                  by what other players call it.";

trait Choice: Copy + PartialEq + 'static {
    const ALL: &'static [Self];

    fn label(self) -> &'static str;

    fn detail(self) -> Option<SharedString> {
        None
    }

    fn in_force(self) -> SharedString {
        SharedString::new_static(self.label())
    }

    fn meaning(self) -> SharedString;
}

fn said_under<T: Choice>(held: T) -> SharedString {
    if T::ALL.contains(&held) {
        held.meaning()
    } else {
        SharedString::from(format!(
            "{} is in force, which is none of these.",
            held.in_force()
        ))
    }
}

impl RootView {
    pub(crate) fn follow(&mut self, followed: Followed, cx: &mut Context<Self>) {
        match followed {
            Followed::Bindings(bindings) => self.follow_the_bindings_read_again(bindings, cx),
            Followed::Tidal(signed_in) => {
                self.follow_the_tidal_sign_in_made_elsewhere(signed_in, cx)
            }
            Followed::Spotify(signed_in) => {
                self.follow_the_spotify_sign_in_made_elsewhere(signed_in, cx)
            }
            Followed::PartyOffered(link) => self.offer_a_party(link, cx),
        }
    }

    pub(crate) fn settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let narrowing = Narrowing::of(self.finding.read(cx).text());
        let category = self.settings_category;
        let page = self
            .settings_page
            .filter(|page| page.category() == category && !narrowing.narrows());
        let standing = self.standing(cx);
        let rail = self.categories(&narrowing, cx);
        let body = self.shown(&narrowing, category, page, &standing, cx);

        kit::pane()
            .child(self.settings_heading(&narrowing, category, page, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.0))
                    .child(
                        div()
                            .relative()
                            .flex_none()
                            .w(theme::width(theme::settings_rail()))
                            .min_h(px(0.0))
                            .child(rail)
                            .child(Scrollbars::of(cx).vertical(
                                "settings-categories-scrollbar",
                                self.settings_rail_scroll.clone(),
                            )),
                    )
                    .child(Scrollbars::of(cx).around(
                        "settings-scrollbar",
                        self.settings_scroll.clone(),
                        body,
                    )),
            )
            .into_any_element()
    }

    fn settings_heading(
        &self,
        narrowing: &Narrowing,
        category: Category,
        page: Option<Page>,
        cx: &mut Context<Self>,
    ) -> Div {
        let found = narrowing.anywhere().count();
        let (title, about) = match (narrowing.narrows(), page) {
            (true, _) => (
                SharedString::new_static("Found"),
                SharedString::from(match found {
                    0 => "No setting anywhere answers to that".to_owned(),
                    1 => "One setting, wherever it is kept".to_owned(),
                    many => format!("{many} settings, wherever they are kept"),
                }),
            ),
            (false, Some(page)) => (
                SharedString::new_static(page.title()),
                SharedString::new_static(page.about()),
            ),
            (false, None) => (
                SharedString::new_static(category.label()),
                SharedString::new_static(category.about()),
            ),
        };
        let above = match page {
            Some(_) => self
                .in_the_ring(
                    "settings-way-back",
                    kit::way_back("settings-way-back", category.label(), BACK_HINT),
                    |this, window, cx| {
                        this.leave_the_settings_page(cx);
                        window.focus(&this.focus, cx);
                    },
                    cx,
                )
                .into_any_element(),
            None => kit::eyebrow("Settings").into_any_element(),
        };

        kit::heading().child(
            kit::heading_row()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w(theme::width(theme::heading_name()))
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .min_h(px(theme::way_back_height()))
                                .child(above),
                        )
                        .child(kit::title(title))
                        .child(kit::subtitle(about)),
                )
                .child(kit::actions().child(self.filter_field(cx))),
        )
    }

    fn filter_field(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let typed = !self.finding.read(cx).text().is_empty();

        kit::framed_field("find-a-setting", self.finding_holds_the_caret)
            .w(theme::width(theme::picker_width()))
            .max_w_full()
            .child(icons::icon(
                Icon::Search,
                theme::search_icon(),
                theme::faint(),
            ))
            .child(self.finding.clone())
            .when_else(
                typed,
                |field| field.child(self.clear_filter(cx)),
                |field| field.child(kit::figure("ctrl-,").text_color(rgb(theme::faint()))),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.finding
                    .update(cx, |finding, cx| finding.take_focus(window, cx));
                cx.notify();
            }))
    }

    fn clear_filter(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let clear = div()
            .id("clear-the-filter")
            .said_as(Role::Button, CLEAR_THE_FILTER_HINT)
            .group("clear-filter")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(theme::width(theme::filter_clear_control()))
            .rounded_full()
            .cursor_pointer()
            .hover(|mark| mark.bg(rgb(theme::hover())))
            .names(CLEAR_THE_FILTER_HINT)
            .child(icons::lit_on_hover(
                icons::icon(Icon::Close, theme::filter_clear_mark(), theme::muted()),
                "clear-filter",
            ));

        self.in_the_pane_ring(
            clear,
            |this, window, cx| {
                this.finding.update(cx, |finding, cx| finding.clear(cx));
                this.finding
                    .update(cx, |finding, cx| finding.take_focus(window, cx));
                cx.notify();
            },
            cx,
        )
    }

    fn categories(&self, narrowing: &Narrowing, cx: &mut Context<Self>) -> Stateful<Div> {
        let chosen = self.settings_category;
        let mut rail = div()
            .id("settings-categories")
            .named_as(Role::TabList, CATEGORIES, "")
            .flex()
            .flex_col()
            .flex_none()
            .gap_0p5()
            .p_2()
            .pr_4()
            .w(theme::width(theme::settings_rail()))
            .bg(rgb(theme::surface()))
            .border_r_1()
            .border_color(rgb(theme::border()))
            .overflow_y_scroll()
            .track_scroll(&self.settings_rail_scroll)
            .h_full();

        for category in Category::ALL {
            let selected = !narrowing.narrows() && category == chosen;
            let found = narrowing.narrows().then(|| narrowing.found(category));
            let ink = if selected {
                theme::text()
            } else {
                theme::muted()
            };

            rail = rail.child(
                self.in_the_ring(
                    category.label(),
                    div()
                        .id(SharedString::new_static(category.label()))
                        .debug_selector(|| format!("category-{}", category.label()))
                        .named_as(Role::Tab, category.label(), category.about())
                        .chosen(selected)
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .text_size(px(theme::text_sm()))
                        .whitespace_nowrap()
                        .cursor_pointer()
                        .relative()
                        .text_color(rgb(ink))
                        .child(motion::shown_while(
                            focus::beneath_the_ring(div())
                                .rounded_lg()
                                .bg(rgb(theme::hover())),
                            "chosen",
                            selected,
                        ))
                        .when_else(
                            selected,
                            |row| row.font_weight(FontWeight::MEDIUM),
                            |row| row.hover(|row| row.bg(rgb(theme::hover()))),
                        )
                        .names(category.about())
                        .child(icons::icon(category.icon(), theme::root_icon(), ink))
                        .child(div().flex_1().min_w(px(0.0)).child(category.label()))
                        .when_some(found, |row, found| {
                            row.child(kit::figure(found.to_string()).text_color(rgb(
                                if found == 0 {
                                    theme::faint()
                                } else {
                                    theme::accent()
                                },
                            )))
                        }),
                    move |this, _, cx| this.show_settings(category, cx),
                    cx,
                ),
            );
        }

        rail
    }

    fn shown(
        &mut self,
        narrowing: &Narrowing,
        category: Category,
        page: Option<Page>,
        standing: &Standing,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut column = div()
            .flex()
            .flex_col()
            .flex_none()
            .gap_6()
            .w(theme::width(theme::settings_column()))
            .max_w_full();

        if narrowing.narrows() {
            let mut places: Vec<(Place, Vec<Group>)> = Vec::new();
            for group in narrowing.anywhere() {
                let place = (group.category(), group.page());
                match places.last_mut() {
                    Some((held, groups)) if *held == place => groups.push(group),
                    _ => places.push((place, vec![group])),
                }
            }
            if places.is_empty() {
                column = column.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .py_8()
                        .child(
                            div()
                                .text_size(px(theme::text_base()))
                                .text_color(rgb(theme::muted()))
                                .child(NOTHING_FOUND),
                        )
                        .child(
                            div()
                                .text_size(px(theme::text_sm()))
                                .text_color(rgb(theme::faint()))
                                .child(NOTHING_FOUND_MORE),
                        ),
                );
            }
            for ((category, page), groups) in places {
                let mut place = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().flex().child(self.place_link(category, page, cx)));
                for group in groups {
                    place = place.child(self.group(group, standing, cx));
                }
                column = column.child(place);
            }
        } else {
            match page {
                Some(page) => {
                    for group in category.shown_on(Some(page)) {
                        column = column.child(self.group(group, standing, cx));
                    }
                }
                None => {
                    for drawn in category.drawn() {
                        column = column.child(match drawn {
                            Drawn::Group(group) => self.group(group, standing, cx),
                            Drawn::Pages(pages) => self.pages(&pages, cx),
                        });
                    }
                    if category.groups().any(can_be_put_back) {
                        column = column.child(self.reset(category, cx));
                    }
                }
            }
        }

        div()
            .id("settings")
            .debug_selector(|| "settings".to_owned())
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if !*hovered && this.holds_a_press_armed() {
                    this.lower_every_armed_press();
                    cx.notify();
                }
            }))
            .flex()
            .flex_col()
            .flex_1()
            .min_w(px(0.0))
            .min_h(px(0.0))
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .child(motion::faded_in(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .items_center()
                    .p_6()
                    .child(column),
                SharedString::from(match (narrowing.narrows(), page) {
                    (true, _) => "settings-found".to_owned(),
                    (false, Some(page)) => format!("settings-{}-{}", category.as_str(), page.id()),
                    (false, None) => format!("settings-{}", category.as_str()),
                }),
                motion::ARRIVES_OVER,
            ))
    }

    fn place_link(
        &self,
        category: Category,
        page: Option<Page>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = format!(
            "settings-place-{}-{}",
            category.as_str(),
            page.map_or("main", Page::id)
        );
        let label = match page {
            Some(page) => format!("{} · {}", category.label(), page.title()),
            None => category.label().to_owned(),
        };

        self.in_the_ring_at(
            id.clone(),
            kit::place_link(
                SharedString::from(id),
                label.clone(),
                format!("Open {label}, where these settings are kept"),
            ),
            move |this, window, cx| {
                this.open_the_place(category, page, cx);
                window.focus(&this.focus, cx);
            },
            cx,
        )
    }

    fn pages(&self, pages: &[Page], cx: &mut Context<Self>) -> Div {
        let gathered = pages.first().map_or("", |page| page.gathered_under());
        let mut listed = kit::rows();
        for page in pages.iter().copied() {
            let id = format!("settings-page-{}", page.id());
            listed = listed.child(self.in_the_ring_at(
                id.clone(),
                kit::nav_row(
                    SharedString::from(id),
                    page.icon(),
                    page.title(),
                    page.about(),
                ),
                move |this, window, cx| {
                    this.open_the_settings_page(page, cx);
                    window.focus(&this.focus, cx);
                },
                cx,
            ));
        }

        kit::group(
            kit::group_heading().child(kit::group_title(gathered)),
            listed,
        )
    }

    pub(crate) fn open_the_settings_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.show_settings(page.category(), cx);
        self.settings_page = Some(page);
        self.settings_scroll.set_offset(Point::default());
        cx.notify();
    }

    pub(crate) fn leave_the_settings_page(&mut self, cx: &mut Context<Self>) -> bool {
        if self.settings_page.take().is_none() {
            return false;
        }
        self.settings_scroll.set_offset(Point::default());
        self.disarm();
        cx.notify();
        true
    }

    fn open_the_place(&mut self, category: Category, page: Option<Page>, cx: &mut Context<Self>) {
        self.finding.update(cx, |finding, cx| finding.clear(cx));
        match page {
            Some(page) => self.open_the_settings_page(page, cx),
            None => {
                self.show_settings(category, cx);
                self.settings_scroll.set_offset(Point::default());
            }
        }
    }

    fn group(&mut self, group: Group, standing: &Standing, cx: &mut Context<Self>) -> Div {
        let body = match group {
            Group::Device => self.device_group(cx),
            Group::FlacRenderers => self.flac_renderers_group(cx),
            Group::DeviceVolume => self.device_volume_group(cx),
            Group::SampleRate => self.sample_rate_group(cx),
            Group::GraphRate => self.graph_rate_group(cx),
            Group::Buffer => self.buffer_group(cx),
            Group::Dop => self.dop_group(cx),
            Group::Bluetooth => self.bluetooth_group(cx),
            Group::Resampler => self.resampler_group(cx),
            Group::Dither => self.dither_group(cx),
            Group::NoiseShaping => self.noise_shaping_group(cx),
            Group::Crossfeed => self.crossfeed_group(cx),
            Group::ReplayGain => self.replay_gain_group(cx),
            Group::TruePeak => self.true_peak_group(cx),
            Group::LossySources => self.lossy_sources_group(cx),
            Group::Equalising => self.equalising_group(cx),
            Group::BoundTo => self.bound_to_group(cx),
            Group::Bands => self.bands_group(cx),
            Group::AutoEq => self.autoeq_group(cx),
            Group::Measurers => self.measurers_group(cx),
            Group::OwnMeasurement => self.own_measurement_group(cx),
            Group::RoomMeasurement => self.room_measurement_group(cx),
            Group::RoomCorrection => self.room_group(cx),
            Group::Speakers => self.speakers_group(cx),
            Group::Abx => self.abx_group(cx),
            Group::Hidden => self.hidden_group(cx),
            Group::MusicExtensions => self.music_extensions_group(cx),
            Group::MinimumLength => self.minimum_length_group(cx),
            Group::Folders => self.folders_group(cx),
            Group::Rescanning => self.rescanning_group(cx),
            Group::LookingUp => self.looking_up_group(cx),
            Group::Retagging => self.retagging_group(cx),
            Group::Organising => self.organising_group(cx),
            Group::Vault => self.vault_group(cx),
            Group::Inbox => self.inbox_group(cx),
            Group::YoutubeMusic => self.youtube_music_group(cx),
            Group::Subsonic => self.account_group(Service::Subsonic, cx),
            Group::Jellyfin => self.account_group(Service::Jellyfin, cx),
            Group::ResonateKey => self.resonate_key_group(cx),
            Group::Tidal => self.tidal_group(cx),
            Group::Spotify => self.spotify_group(cx),
            Group::Resuming => self.resuming_group(cx),
            Group::History => self.history_group(cx),
            Group::Shuffling => self.shuffling_group(cx),
            Group::Repeating => self.repeating_group(cx),
            Group::PreviousButton => self.previous_group(cx),
            Group::Lookups => self.lookups_group(cx),
            Group::AfterScan => self.after_scan_group(cx),
            Group::Metered => self.metered_group(cx),
            Group::Studies => self.studies_group(cx),
            Group::Lyrics => self.lyrics_group(cx),
            Group::Contact => self.contact_group(cx),
            Group::Recognition => self.recognition_group(cx),
            Group::Listening => self.listening_group(cx),
            Group::Submitting => self.submitting_group(cx),
            Group::Lastfm => self.lastfm_group(cx),
            Group::Syncing => self.syncing_group(cx),
            Group::Pairing => self.pairing_group(cx),
            Group::SyncDevices => self.sync_devices_group(cx),
            Group::SyncConnection => self.sync_connection_group(cx),
            Group::SyncedSongs => self.synced_songs_group(cx),
            Group::SyncControl => self.sync_control_group(cx),
            Group::Notifications => self.notifications_group(cx),
            Group::Discord => self.discord_group(cx),
            Group::DiscordShows => self.discord_shows_group(cx),
            Group::Colour => self.colour_group(cx),
            Group::Layout => self.layout_group(cx),
            Group::Motion => self.motion_group(cx),
            Group::WindowButtons => self.window_buttons_group(cx),
            Group::VolumeWheel => self.volume_wheel_group(cx),
            Group::MouseNavigation => self.mouse_navigation_group(cx),
            Group::Scrollbars => self.scrollbars_group(cx),
            Group::LiveSpectrum => self.live_spectrum_group(cx),
            Group::Tabs => self.tabs_group(cx),
            Group::WindowState => self.window_state_group(cx),
            Group::Build => self.build_group(cx),
            Group::Updates => self.updates_group(cx),
            Group::Places => self.places_group(cx),
            Group::Everything => self.everything_group(cx),
        };

        self.grouped(group, standing, body, cx)
    }

    fn grouped(
        &self,
        group: Group,
        standing: &Standing,
        body: Rows,
        cx: &mut Context<Self>,
    ) -> Div {
        let moved = can_be_put_back(group) && differs(group, standing);

        kit::group(
            kit::group_heading()
                .child(kit::group_title(group.title()))
                .when(group.is_experimental(), |heading| {
                    heading.child(kit::badge(EXPERIMENTAL, theme::lossy()))
                })
                .when(moved, |heading| heading.child(self.revert(group, cx)))
                .child(hint::explains(group.title(), group.hint())),
            body,
        )
    }

    fn revert(&self, group: Group, cx: &mut Context<Self>) -> Stateful<Div> {
        self.in_the_ring(
            group.title(),
            kit::icon_button(
                SharedString::from(format!("revert-{}", group.title())),
                Icon::Undo,
                REVERT_HINT,
            ),
            move |this, _, cx| this.put_back(group, cx),
            cx,
        )
    }

    fn reset(&self, category: Category, cx: &mut Context<Self>) -> Div {
        div().flex().justify_end().child(self.in_the_ring(
            "reset-the-category",
            kit::button(
                "reset-the-category",
                Some(Icon::Undo),
                format!("Reset {}", category.label().to_lowercase()),
                RESET_HINT,
                Tone::Ghost,
            ),
            move |this, _, cx| {
                for group in category.groups().filter(|group| can_be_put_back(*group)) {
                    this.put_back(group, cx);
                }
            },
            cx,
        ))
    }

    pub(crate) fn put_back(&mut self, group: Group, cx: &mut Context<Self>) {
        for command in puts_back(group) {
            self.send(command, cx);
        }

        match group {
            Group::Colour => self.dress(
                Appearance {
                    theme: Appearance::DEFAULT.theme,
                    accent: Appearance::DEFAULT.accent,
                    follows: Following {
                        scheme: Appearance::DEFAULT.follows.scheme,
                        ..theme::chosen().follows
                    },
                    ..theme::chosen()
                },
                cx,
            ),
            Group::Layout => self.dress(
                Appearance {
                    text_size: Appearance::DEFAULT.text_size,
                    follows: Following {
                        text: Appearance::DEFAULT.follows.text,
                        ..theme::chosen().follows
                    },
                    ..theme::chosen()
                },
                cx,
            ),
            Group::Equalising => self.switch_the_equaliser(false, cx),
            Group::BoundTo => self.unbind_every_device(cx),
            Group::Lookups => {
                self.set_online(defaults::ONLINE, cx);
                self.set_musicbrainz_mirror(defaults::MUSICBRAINZ_MIRROR, cx);
            }
            Group::AfterScan => self.set_after_scan(defaults::ENRICH_AFTER_SCAN, cx),
            Group::Metered => {
                self.set_enrich_on_metered(defaults::ENRICH_ON_METERED, cx);
                self.set_stall_downloads_on_metered(defaults::STALL_DOWNLOADS_ON_METERED, cx);
            }
            Group::FlacRenderers => self.vouch_for_none(cx),
            Group::Studies => {
                self.set_studies(defaults::STUDY, cx);
                self.set_by_sound(defaults::BY_SOUND, cx);
            }
            Group::Lyrics => {
                self.set_lyrics(defaults::FETCH_LYRICS, cx);
                self.set_lyrics_by_the_locale(defaults::LYRICS_BY_THE_LOCALE, cx);
            }
            Group::Resuming => self.set_resume(defaults::RESUME, cx),
            Group::History => self.keep_the_history(defaults::HISTORY_KEPT, cx),
            Group::MusicExtensions => self.set_music_extensions(Default::default(), cx),
            Group::MinimumLength => self.set_minimum_length(Default::default(), cx),
            Group::Device => self.look_for_renderers(false, cx),
            Group::Notifications => self.set_notify(defaults::NOTIFY, cx),
            Group::Updates => self.put_the_updates_back(cx),
            Group::Discord => self.put_discord_back(cx),
            Group::Syncing => self.set_sync(Synchronised::default().enabled, cx),
            Group::SyncConnection => self.put_the_connection_back(cx),
            Group::SyncedSongs => self.follow_songs(resonate_core::SyncedSongs::default(), cx),
            Group::SyncControl => {
                self.trust_other_devices(resonate_core::RemoteTrust::default(), cx);
            }
            Group::DiscordShows => self.put_discord_shows_back(cx),
            Group::Contact => self.clear_contact(cx),
            Group::Measurers => self.wear_the_measurers(Measurers::default(), cx),
            Group::Recognition => {
                self.clear_acoustid_key(cx);
                self.clear_audd_token(cx);
            }
            Group::Submitting => self.clear_listenbrainz_token(cx),
            Group::Lastfm => self.forget_the_lastfm_account(cx),
            Group::Listening => {
                self.listen.update(cx, |listen, cx| {
                    listen.choose(Listening::Desktop, cx);
                    listen.listen_for(CLIP_BY_DEFAULT, cx);
                });
            }
            Group::Inbox => self
                .library
                .update(cx, |library, cx| library.set_inbox(None, cx)),
            Group::Folders => {
                self.clear_the_music_folder(cx);
                self.file_what_is_dropped(defaults::FILE_DROPPED, cx);
            }
            Group::Subsonic => self.forget_the_account(Service::Subsonic, cx),
            Group::Jellyfin => self.forget_the_account(Service::Jellyfin, cx),
            Group::ResonateKey => self.forget_the_resonate_key(cx),
            Group::Tidal => self.forget_the_tidal_account(cx),
            Group::Spotify => self.forget_the_spotify_account(cx),
            Group::YoutubeMusic => self.forget_youtube_music(cx),
            Group::RoomCorrection => self.correct_the_room(None, cx),
            Group::Speakers => {
                self.set_the_speaker_angle(SpeakerAngle::STANDARD, cx);
                self.set_the_speaker_elevation(SpeakerElevation::LEVEL, cx);
                self.set_the_head_yaw(HeadYaw::AHEAD, cx);
            }
            Group::Organising => self.set_organise_as(DEFAULT_LAYOUT.to_owned(), cx),
            Group::WindowButtons => self.show_window_buttons(defaults::WINDOW_BUTTONS, cx),
            Group::VolumeWheel => self.wheel_the_volume(defaults::SCROLL_VOLUME, cx),
            Group::MouseNavigation => {
                self.navigate_with_mouse_buttons(defaults::MOUSE_NAVIGATION, cx);
            }
            Group::Motion => self.reduce_motion(defaults::REDUCED_MOTION, cx),
            Group::Scrollbars => self.draw_scrollbars(defaults::SCROLLBARS, cx),
            Group::LiveSpectrum => self.read_the_spectrum(defaults::SPECTRAL, cx),
            Group::Tabs => self.show_tabs(defaults::TABS, cx),
            Group::WindowState => self.put_window_state_back(cx),
            _ => {}
        }

        for key in group.keys() {
            self.forget(*key, cx);
        }
        cx.notify();
    }

    fn standing(&self, cx: &mut Context<Self>) -> Standing {
        let library = self.library.read(cx);
        let online = library.is_online();
        let after_scan = library.enriches_after_scan();
        let studies = library.studies();
        let lyrics = library.fetches_lyrics();
        let resume = library.resumes();
        let history_kept = cx.global::<ResonateApp>().history_kept;
        let skip_under_repeat = self.player.read(cx).state().skip_under_repeat;
        let previous_restarts = self.player.read(cx).state().previous_restarts;
        let smart_shuffle = self.player.read(cx).state().smart_shuffle;
        let notify = cx.global::<ResonateApp>().notify.load(Ordering::Acquire);
        let check_for_updates = cx.global::<ResonateApp>().check_for_updates;
        let renderers = cx
            .global::<ResonateApp>()
            .renderers
            .as_ref()
            .is_some_and(|renderers| renderers.looking());
        let enrich_on_metered = cx.global::<ResonateApp>().online.enrich_on_metered;
        let musicbrainz_mirror = cx.global::<ResonateApp>().online.musicbrainz_mirror;
        let stall_downloads_on_metered =
            cx.global::<ResonateApp>().online.stall_downloads_on_metered;
        let flac_renderers_named = !cx.global::<ResonateApp>().flac_above_96k_for.is_empty();
        let by_sound = cx.global::<ResonateApp>().by_sound.load(Ordering::Acquire);
        let lyrics_by_the_locale = cx
            .global::<ResonateApp>()
            .lyrics_by_the_locale
            .load(Ordering::Acquire);
        let window_buttons = cx.global::<ResonateApp>().window_buttons;
        let scroll_volume = cx.global::<ResonateApp>().scroll_volume;
        let mouse_navigation = cx.global::<ResonateApp>().mouse_navigation;
        let reduced_motion = cx.global::<ResonateApp>().reduced_motion;
        let scrollbars = cx.global::<ResonateApp>().scrollbars;
        let spectral = cx.global::<ResonateApp>().spectral;
        let tabs = cx.global::<ResonateApp>().tabs;
        let remember_tab = cx.global::<ResonateApp>().remember_tab;
        let remember_window_size = cx.global::<ResonateApp>().remember_window_size;
        let remember_settings_category = cx.global::<ResonateApp>().remember_settings_category;
        let presence = cx.global::<ResonateApp>().presence.clone();
        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let measurer_given = !cx.global::<ResonateApp>().measurers.is_empty();
        let contact_given = !self.contact.read(cx).text().trim().is_empty();
        let key_given = !self.acoustid.read(cx).text().trim().is_empty();
        let token_given = !self.audd.read(cx).text().trim().is_empty();
        let submitting_given = !self.listenbrainz.read(cx).text().trim().is_empty();
        let lastfm_given = {
            let online = &cx.global::<ResonateApp>().online;
            [
                &online.lastfm_key,
                &online.lastfm_secret,
                &online.lastfm_session,
            ]
            .iter()
            .any(|given| !given.trim().is_empty())
        };
        let listen = self.listen.read(cx);
        let listening_from = listen.from().clone();
        let listening_for = listen.length();
        let template_given = self.organising.read(cx).text().trim() != DEFAULT_LAYOUT;
        let inbox_given = self.library.read(cx).inbox().is_some();
        let music_folder_given = cx.global::<ResonateApp>().music_folder.is_some();
        let file_dropped = cx.global::<ResonateApp>().file_dropped;
        let convolving = cx.global::<ResonateApp>().convolution.is_some();
        let speakers_moved = cx.global::<ResonateApp>().speaker_angle != SpeakerAngle::STANDARD
            || cx.global::<ResonateApp>().speaker_elevation != SpeakerElevation::LEVEL
            || cx.global::<ResonateApp>().head_yaw != HeadYaw::AHEAD;
        let subsonic_given = self
            .subsonic
            .iter()
            .any(|field| !field.read(cx).text().trim().is_empty());
        let jellyfin_given = self
            .jellyfin
            .iter()
            .any(|field| !field.read(cx).text().trim().is_empty());
        let tidal_given = TidalAccount::ALL
            .into_iter()
            .filter(|account| account.is_tidals())
            .any(|account| !self.tidal[account.nth()].read(cx).text().trim().is_empty())
            || cx.global::<ResonateApp>().online.hosted_services;
        let resonate_key_given = !self.tidal[TidalAccount::HifiSecret.nth()]
            .read(cx)
            .text()
            .trim()
            .is_empty();

        let spotify_given = !self.spotify_client.read(cx).text().trim().is_empty()
            || !cx
                .global::<ResonateApp>()
                .online
                .spotify_refresh_token
                .trim()
                .is_empty();
        let youtube_music_given = {
            let online = &cx.global::<ResonateApp>().online;
            online.youtube_music
                || !online.youtube_music_keep_order
                || self
                    .youtube_music
                    .iter()
                    .any(|field| !field.read(cx).text().trim().is_empty())
        };

        Standing {
            output: self.player.read(cx).output_settings().clone(),
            appearance: theme::chosen(),
            online,
            musicbrainz_mirror,
            after_scan,
            enrich_on_metered,
            stall_downloads_on_metered,
            flac_renderers_named,
            studies,
            by_sound,
            lyrics,
            lyrics_by_the_locale,
            contact_given,
            measurer_given,
            key_given,
            token_given,
            submitting_given,
            lastfm_given,
            listening_from,
            listening_for,
            resume,
            history_kept,
            music_filters: cx.global::<ResonateApp>().library.music_filters(),
            skip_under_repeat,
            previous_restarts,
            smart_shuffle,
            notify,
            check_for_updates,
            renderers,
            window_buttons,
            scroll_volume,
            mouse_navigation,
            reduced_motion,
            scrollbars,
            spectral,
            tabs,
            remember_tab,
            remember_window_size,
            remember_settings_category,
            presence,
            synchronised,
            template_given,
            inbox_given,
            music_folder_given,
            file_dropped,
            subsonic_given,
            jellyfin_given,
            tidal_given,
            spotify_given,
            resonate_key_given,
            youtube_music_given,
            convolving,
            speakers_moved,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.lower_every_armed_press();
        self.reset_everything_landed = false;
    }

    fn holds_a_press_armed(&self) -> bool {
        self.armed.is_some()
    }

    pub(super) fn lower_every_armed_press(&mut self) {
        self.armed = None;
        self.pairing_shown = false;
    }

    pub(crate) fn is_armed(&self, armed: &Armed) -> bool {
        self.armed.as_ref() == Some(armed)
    }

    pub(crate) fn arm(&mut self, armed: Armed) {
        self.armed = Some(armed);
    }

    pub(crate) fn lower(&mut self, armed: &Armed) {
        if self.is_armed(armed) {
            self.armed = None;
        }
    }

    pub(crate) const fn history_armed(&self) -> Option<HistoryKept> {
        match &self.armed {
            Some(Armed::AgeTheHistory(kept)) => Some(*kept),
            _ => None,
        }
    }

    pub(crate) fn folder_armed(&self) -> Option<&PathBuf> {
        match &self.armed {
            Some(Armed::ForgetTheFolder(root)) => Some(root),
            _ => None,
        }
    }

    pub(crate) fn in_the_ring_at(
        &self,
        named: impl Into<String>,
        mut control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let named = named.into();
        let clicked = press.clone();
        self.controls
            .captioned_by_its_name(named.clone(), &mut control);

        control
            .track_focus(&self.controls.at(&named, cx))
            .key_context(CONTROL_CONTEXT)
            .focus_visible(focus::lit)
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, click: &ClickEvent, window, cx| {
                if by_the_pointer(click) {
                    clicked(this, window, cx);
                }
            }))
    }

    pub(crate) fn in_the_ring(
        &self,
        named: &'static str,
        mut control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let handle = self.controls.at(named, cx);
        self.controls.captioned_by_its_name(named, &mut control);
        Self::ringed(&handle, control, press, cx)
    }

    pub(crate) fn in_the_pane_ring(
        &self,
        mut control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let handle = self.the_pane_handle_of(&mut control, cx);
        Self::ringed(&handle, control, press, cx)
            .focus(|control| control.opacity(1.0))
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    }

    pub(crate) fn in_the_pane_ring_for_keys_at(
        &self,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, Point<Pixels>, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (mut control, corner) = Self::marking_its_corner(control);
        let handle = self.the_pane_handle_of(&mut control, cx);

        Self::ringed_for_keys(
            &handle,
            control,
            move |this, window, cx| {
                this.remember_the_opener(window, cx);
                window.focus(&this.focus, cx);
                press(this, corner.get(), cx);
            },
            cx,
        )
        .focus(|control| control.opacity(1.0))
        .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
    }

    fn the_pane_handle_of(
        &self,
        control: &mut Stateful<Div>,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        let named = control
            .interactivity()
            .element_id
            .as_ref()
            .map_or_else(SharedString::default, |id| {
                SharedString::from(id.to_string())
            });
        self.controls.captioned_by_its_name(named.clone(), control);
        self.controls.at(named, cx)
    }

    pub(crate) fn in_the_pane_ring_at(
        &self,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, Point<Pixels>, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (control, corner) = Self::marking_its_corner(control);

        self.in_the_pane_ring(
            control,
            move |this, window, cx| {
                this.remember_the_opener(window, cx);
                window.focus(&this.focus, cx);
                press(this, corner.get(), cx);
            },
            cx,
        )
    }

    pub(crate) fn in_the_standing_ring_at(
        &self,
        named: &'static str,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, Point<Pixels>, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (control, corner) = Self::marking_its_corner(control);

        self.in_the_standing_ring(
            named,
            control,
            move |this, window, cx| {
                this.remember_the_opener(window, cx);
                window.focus(&this.focus, cx);
                press(this, corner.get(), cx);
            },
            cx,
        )
    }

    fn marking_its_corner(control: Stateful<Div>) -> (Stateful<Div>, Rc<Cell<Point<Pixels>>>) {
        let corner = Rc::new(Cell::new(Point::default()));
        let held = Rc::clone(&corner);
        let control = control.relative().child(
            canvas(
                move |bounds, _, _| held.set(bounds.bottom_left()),
                |_, (), _, _| {},
            )
            .absolute()
            .inset_0(),
        );

        (control, corner)
    }

    pub(crate) fn ringed(
        handle: &FocusHandle,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = press.clone();

        Self::ringed_for_keys(handle, control, press, cx).on_click(cx.listener(
            move |this, click: &ClickEvent, window, cx| {
                if by_the_pointer(click) {
                    clicked(this, window, cx);
                }
            },
        ))
    }

    pub(crate) fn ringed_for_keys(
        handle: &FocusHandle,
        control: Stateful<Div>,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        control
            .track_focus(handle)
            .key_context(CONTROL_CONTEXT)
            .focus_visible(focus::lit)
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
    }

    fn choices<T: Choice>(
        &self,
        id: &'static str,
        held: T,
        cx: &mut Context<Self>,
        taken: impl Fn(&mut Self, T, &mut Context<Self>) + Copy + 'static,
    ) -> Div {
        let mut row = kit::segmented();

        for (index, value) in T::ALL.iter().enumerate() {
            let value = *value;
            row = row.child(
                self.option_in_the_ring(
                    kit::segment((id, index), value.label(), held == value)
                        .when_some(value.detail(), |option, detail| option.names(detail)),
                    id,
                    index,
                    move |this, _, cx| taken(this, value, cx),
                    cx,
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(div().flex().child(row))
            .child(note(said_under(held)))
    }

    fn option_in_the_ring(
        &self,
        control: Stateful<Div>,
        id: &'static str,
        index: usize,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        self.option_named(control, format!("{id}-{index}"), press, cx)
    }

    fn option_named(
        &self,
        mut control: Stateful<Div>,
        named: String,
        press: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let clicked = press.clone();
        self.controls
            .captioned_by_its_name(named.clone(), &mut control);

        control
            .track_focus(&self.controls.at(named, cx))
            .key_context(CONTROL_CONTEXT)
            .focus_visible(focus::picked)
            .on_action(
                cx.listener(move |this, _: &crate::app::PressControl, window, cx| {
                    press(this, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_click(cx.listener(move |this, click: &ClickEvent, window, cx| {
                if by_the_pointer(click) {
                    clicked(this, window, cx);
                }
            }))
    }

    pub(crate) fn dress(&self, dressed: Appearance, cx: &mut Context<Self>) {
        theme::wear(dressed);
        cx.global::<ResonateApp>()
            .launcher
            .show(AppIcon::of(theme::worn()));
        cx.refresh_windows();
    }

    pub(crate) fn wear(&mut self, dressed: Appearance, setting: &Setting, cx: &mut Context<Self>) {
        self.dress(dressed, cx);
        self.store(setting, cx);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Armed {
    ResetEverything,
    RegenerateTheCode,
    MoveTheFiles,
    WalkTheFilingBack,
    WalkTheTagsBack,
    WriteTheTags,
    KeepTheTracks,
    DiscardTheCurve,
    TellTheEarlierPlays,
    AgeTheHistory(HistoryKept),
    ForgetTheFolder(PathBuf),
}

impl RootView {
    pub(super) fn text_field(
        &self,
        id: &'static str,
        field: &Entity<Field>,
        leave: fn(&mut Self, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let taking = field.clone();
        let held = field.clone();

        kit::framed_field(id, false)
            .on_mouse_down_out(cx.listener(move |this, _, window, cx| {
                if held.read(cx).is_focused(window) {
                    leave(this, window, cx);
                }
            }))
            .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {
                taking.update(cx, |typed, cx| typed.take_focus(window, cx));
                cx.notify();
            }))
            .child(field.clone())
            .child(kit::figure("enter").text_color(rgb(theme::faint())))
    }

    pub(super) fn let_go_of_the_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

fn reading(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Stateful<Div> {
    let label = label.into();
    let value = value.into();

    div()
        .id(label.clone())
        .read_plainly_as(|| spoken::joined([label.as_ref(), value.as_ref()]))
        .flex()
        .items_start()
        .justify_between()
        .gap_4()
        .child(kit::row_name(label).flex_none())
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .text_size(px(theme::text_sm()))
                .text_color(rgb(theme::muted()))
                .text_right()
                .child(value),
        )
}

fn progress(share: f32) -> Div {
    div()
        .w_full()
        .h(theme::width(theme::SCAN_BAR))
        .rounded_full()
        .overflow_hidden()
        .bg(rgb(theme::soft_edge()))
        .child(
            div()
                .h_full()
                .w(relative(share.clamp(0.0, 1.0)))
                .rounded_full()
                .bg(rgb(theme::accent())),
        )
}

fn action(
    id: &'static str,
    label: impl Into<SharedString>,
    icon: Icon,
    busy: bool,
    press: impl Fn(&mut RootView, &mut Window, &mut Context<RootView>) + Clone + 'static,
    this: &RootView,
    cx: &mut Context<RootView>,
) -> Stateful<Div> {
    let label = label.into();

    if busy {
        return kit::button_when(
            Press::Greyed,
            id,
            Some(icon),
            label.clone(),
            label,
            Tone::Outlined,
        );
    }

    this.in_the_ring(
        id,
        kit::button(id, Some(icon), label.clone(), label, Tone::Outlined),
        press,
        cx,
    )
}
