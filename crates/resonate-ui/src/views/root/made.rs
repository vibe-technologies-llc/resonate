use std::{
    cell::{Cell, RefCell},
    future,
    rc::Rc,
    sync::Arc,
    time::SystemTime,
};

use ahash::AHashMap;
use gpui::{Context, Point, ScrollHandle, Task, UniformListScrollHandle, Window, prelude::*, px};
use resonate_core::ZoneId;
use resonate_engine::Command;
use resonate_library::{Direction, RowOrder, SortOrder, is_a_followed_link};

use super::{
    Accounts, Arrangement, Dropped, Following, Orders, Pane, Rails, Remembered, Remotely, Rooms,
    RootView, Scrolls, SettingsShown, Shows, Social, Typed, TypedAhead, WINDOW_SIZE_SETTLE,
};
use crate::{
    EqualiserModel, LibraryModel, LyricsModel, ResonateApp, Setting, WindowSize,
    analysis::AnalysisModel,
    app::attend,
    listening::ListenModel,
    models::Scale,
    serial::Serial,
    serving::ServerModel,
    settings::SettingsWriter,
    toast::{self, Toaster},
    together::is_an_invitation,
    views::{
        announcing,
        browser::ArtistShows,
        dropping::FolderStanding,
        field::{Caught, Field, Submitted},
        focus::Controls,
        inspector::InspectorPane,
        listing::{self},
        matching::{self},
        missing::MissingShows,
        part::Parts,
        party::PartyPane,
        playlists::PlaylistsDrawn,
        queue::{QueueMeasure, QueueNames, TakenBack},
        search::SearchShows,
        served::ServicesPane,
        settings::{
            Category, CheckingYoutubeMusic, Entered, Fitting, LastfmSigning, Left, QuickConnecting,
            SettingsFields, SigningIn, SigningInToYoutubeMusic, SpotifyState, Tending,
        },
        sheets::{self},
        slider::Rail,
        typing::TypeAhead,
        visualiser::Visualiser,
        zones::{Switcher, ZoneShown},
    },
    words,
};

impl RootView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let global = cx.global::<ResonateApp>();
        let tabs = global.tabs;
        let remember_tab = global.remember_tab;
        let pane = global
            .last_tab
            .filter(|pane| remember_tab && pane.is_shown(tabs) && *pane != Pane::Server)
            .unwrap_or_default();
        let remember_window_size = global.remember_window_size;
        let remember_settings_category = global.remember_settings_category;
        let settings_category = if remember_settings_category {
            global.last_settings_category
        } else {
            Category::default()
        };
        let last_window_size = WindowSize::from_pixels(window.window_bounds().get_bounds().size);
        let zones = Arc::clone(&global.zones);
        let library = Arc::clone(&global.library);
        let settings = Arc::clone(&global.settings);
        let lyricists = Arc::clone(&global.lyricists);
        let attention = global.attention.clone();
        let online = global.online.clone();
        let reference = global.reference.clone();
        let for_the_pass = global.for_the_pass.clone();
        let corrections = Arc::clone(&global.corrections);
        let bindings = global.bindings.clone();
        let measurers = global.measurers.clone();
        let places = global.places.clone();
        let resume = global.resume;
        let sourcing = global.sourcing.clone();
        let engine = Arc::clone(&global.zones.first().player);
        let catalog = Arc::clone(&global.library);
        let fingerprinters = Arc::clone(&global.fingerprinters);

        let library = cx.new(|cx| {
            LibraryModel::new(
                library,
                reference,
                for_the_pass,
                &online,
                resume,
                sourcing,
                cx,
            )
        });
        let arranged_as = Arrangement {
            selection: library.read(cx).selection(),
            sorting: library.read(cx).sorting(),
            playlists: (
                library.read(cx).playlist_order(),
                library.read(cx).playlist_reading(),
            ),
            opened_rows: (RowOrder::default(), Direction::default()),
            search: (SortOrder::default(), SortOrder::default().reads()),
            shelves: (
                library.read(cx).favourited().artists,
                library.read(cx).favourited().albums,
            ),
        };
        let lyrics = cx.new(|_| LyricsModel::new(lyricists));
        let held_zones = zones.listed();
        let switcher = Switcher::of(held_zones.iter().map(|zone| (zone.id, zone.name.clone())));
        let shown_zones: Vec<ZoneShown> = held_zones
            .iter()
            .map(|zone| Self::watch_the_zone(zone, cx))
            .collect();
        let player = shown_zones
            .iter()
            .find(|zone| zone.id == ZoneId::FIRST)
            .map(|zone| zone.player.clone())
            .unwrap_or_else(|| unreachable!("the first zone is always held"));
        let visualiser = cx.new(|cx| Visualiser::new(player.clone(), cx));
        let reaching = cx.global::<ResonateApp>().online.enabled;
        let analysis =
            cx.new(|_| AnalysisModel::new(engine, catalog, Arc::clone(&fingerprinters), reaching));
        cx.observe(&analysis, |_, _, cx| cx.notify()).detach();
        let listens = cx.global::<ResonateApp>().listens.clone();
        let listen = cx.new(|_| ListenModel::new(listens));
        cx.observe(&listen, |_, _, cx| cx.notify()).detach();
        let party = cx.new(|cx| PartyPane::new(window, cx));
        cx.observe(&party, |_, _, cx| cx.notify()).detach();
        let inspector = cx.new(|_| InspectorPane::default());
        cx.observe(&inspector, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<Toaster>(|_, cx| cx.notify()).detach();
        player.read(cx).listen_in(true);
        cx.observe_in(&library, window, |this, library, window, cx| {
            this.forget_what_has_gone(&library, cx);
            this.let_go_of_a_reach_the_rows_left(cx);
            let (relocated, arrived) = library.update(cx, |model, _| {
                (model.take_relocations(), model.take_arrival())
            });
            if !relocated.is_empty() {
                this.send(Command::Relocate(relocated), cx);
            }
            if let Some(track) = arrived {
                this.play_what_was_downloaded(track, cx);
            }
            if this.typing.waiting_for_the_results && !library.read(cx).is_reading_the_search() {
                this.typing.waiting_for_the_results = false;
                this.reach_the_first_result(window, cx);
            }
            cx.notify();
        })
        .detach();
        cx.observe(&lyrics, |_, _, cx| cx.notify()).detach();

        attend(&attention, window.is_window_active());
        cx.observe_window_activation(window, move |_, window, _| {
            attend(&attention, window.is_window_active());
        })
        .detach();

        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let menu_focus = cx.focus_handle();

        let search = cx.new(|cx| {
            Field::new(words::root::search_placeholder, window, cx)
                .catching(|text| is_a_followed_link(text) || is_an_invitation(text))
                .searching()
        });
        cx.observe(&search, |this, search, cx| {
            let typed_anew = {
                let typed = search.read(cx).text();
                this.library.read(cx).narrowing().unwrap_or_default() != typed
            };
            if !typed_anew {
                return;
            }

            this.typing.waiting_for_the_results = false;
            let query = search.read(cx).text().to_owned();
            this.set_query(query, cx);
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, Caught(link), window, cx| {
            if is_an_invitation(link) {
                this.accept_an_invitation(link, cx);
            } else {
                this.follow_link(link, window, cx);
            }
        })
        .detach();

        cx.subscribe_in(&search, window, |this, _, _: &Submitted, window, cx| {
            this.library
                .update(cx, |library, cx| library.ask_elsewhere_now(cx));
            this.go_to_the_results(window, cx);
        })
        .detach();

        let zone_name = cx.new(|cx| Field::new(words::zones::name_this_zone, window, cx));
        cx.subscribe_in(&zone_name, window, |this, _, _: &Submitted, window, cx| {
            this.zone_named(window, cx);
        })
        .detach();

        let name = cx.new(|cx| Field::new(words::root::name_placeholder, window, cx));
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
            this.name_given(window, cx);
        })
        .detach();

        let settings_fields = cx.new(|cx| SettingsFields::new(window, cx));
        cx.subscribe_in(
            &settings_fields,
            window,
            |this, _, entered: &Entered, window, cx| this.entered(*entered, window, cx),
        )
        .detach();
        cx.subscribe_in(
            &settings_fields,
            window,
            |this, _, left: &Left, window, cx| this.left(*left, window, cx),
        )
        .detach();

        let (typed_words, typed_isrc) = matching::matching_fields(window, cx);

        let server = cx.new(ServerModel::new);
        cx.observe(&server, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<ResonateApp>(|this, cx| {
            this.server.update(cx, |server, cx| server.refresh(cx));
            if this.pane == Pane::Server && !this.server.read(cx).is_open() {
                this.set_pane(Pane::default(), cx);
            }
        })
        .detach();

        let services = cx.new(|cx| ServicesPane::new(server.clone(), focus.clone(), window, cx));
        cx.observe(&services, |_, _, cx| cx.notify()).detach();

        let equaliser = cx.new(|cx| {
            let mut model = EqualiserModel::saved_on_leaving(
                places.equaliser.clone(),
                Arc::clone(&corrections),
                bindings,
                cx,
            );
            model.prefer(measurers);
            model
        });
        cx.observe(&equaliser, |this, equaliser, cx| {
            let (notice, untold) =
                equaliser.update(cx, |model, _| (model.take_notice(), model.take_untold()));
            if let Some(notice) = notice {
                toast::tell(notice, cx);
            }
            if untold {
                this.tell_the_engine(cx);
            }
        })
        .detach();

        Self::watch_remote_playback(cx);

        let mut view = Self {
            player,
            library,
            lyrics,
            visualiser,
            analysis,
            listen,
            party,
            inspector,
            services,
            settings_fields,
            listening_open: false,
            downloads_open: false,
            downloads_pressed_off: None,
            deleting: None,
            matching: None,
            likely_asked: Serial::default(),
            attention: Serial::default(),
            settings_written: SettingsWriter::over(settings),
            pane,
            behind_queue: Pane::default(),
            naming: None,
            adding: None,
            adding_songs_to: None,
            finding_holds_the_caret: false,
            server,
            equaliser,
            controls: Controls::default(),
            standing_controls: Controls::default(),
            sheet_panels: Controls::default(),
            sheets: sheets::Standing::default(),
            announced: announcing::Announcer::default(),
            announcing_again: None,
            took_out: TakenBack::default(),
            queue_length: QueueMeasure::default(),
            queue_ordered: None,
            earlier_plays_asked: false,
            zones: shown_zones,
            switcher,
            naming_the_zone: None,
            following: Following::default(),
            came_from: Vec::new(),
            goes_forward: Vec::new(),
            titled: None,
            landing_on: None,
            under_the_pointer: None,
            menu: None,
            record: None,
            reach: None,
            arranged_as,
            creeping: None,
            creeping_on: Task::ready(()),
            listing_whole: Task::ready(()),
            magnified: None,
            pointer_inside: true,
            beneath_closes: Task::ready(()),
            queue_names: QueueNames::default(),
            drawn_at: SystemTime::now(),
            scale: Scale::ONE,
            resolved: RefCell::new(None),
            shown_cover: RefCell::new(None),
            handovers: RefCell::default(),
            focus,
            menu_focus,
            parts: Parts::of(&cx.entity(), cx),
            scrolls: Scrolls {
                settings: ScrollHandle::new(),
                settings_rail: ScrollHandle::new(),
                sidebar: ScrollHandle::new(),
                statistics: ScrollHandle::new(),
                analysis: ScrollHandle::new(),
                suggestions: ScrollHandle::new(),
                artist_records: ScrollHandle::new(),
                ordering: ScrollHandle::new(),
                downloads: ScrollHandle::new(),
                heard_earlier: ScrollHandle::new(),
                likely: ScrollHandle::new(),
                remote: ScrollHandle::new(),
                search: ScrollHandle::default(),
                queue_rows: UniformListScrollHandle::default(),
                playlist_rows: UniformListScrollHandle::default(),
                track_rows: UniformListScrollHandle::default(),
                artist_rows: UniformListScrollHandle::default(),
                album_rows: UniformListScrollHandle::default(),
                all_playlist_rows: UniformListScrollHandle::default(),
                favourite_rows: UniformListScrollHandle::default(),
                missing_rows: UniformListScrollHandle::default(),
                suggestion_rows: UniformListScrollHandle::default(),
                picker_rows: UniformListScrollHandle::default(),
                shelves: RefCell::new(AHashMap::new()),
                left_at: AHashMap::new(),
            },
            rooms: Rooms {
                grid_width: Rc::default(),
                hero_width: Rc::new(Cell::new(px(0.0))),
                hero_height: Rc::new(Cell::new(px(0.0))),
                playing: Rc::new(Cell::new(px(0.0))),
                status: Rc::new(Cell::new(px(0.0))),
                heading: Rc::new(Cell::new(px(0.0))),
                columns_fit: listing::Fitting::default(),
                queue_height: Rc::default(),
                reached_unseen: Rc::default(),
            },
            remotely: Remotely {
                songs: Vec::new(),
                collapsed: false,
                fits_from: None,
                expanded_by_hand: false,
                heading_pressed_at: None,
                controls: Controls::default(),
                driving: None,
                driving_redraws: None,
                mirror_redraws: None,
                driving_rails: std::collections::BTreeMap::new(),
                driving_rows: crate::views::settings::sync::DRIVEN_ROWS_AT_FIRST,
            },
            accounts: Accounts {
                spotify: SpotifyState::default(),
                youtube_music_check: CheckingYoutubeMusic::default(),
                youtube_music_sign_in: SigningInToYoutubeMusic::default(),
                jellyfin_quick: QuickConnecting::default(),
                signing_in: SigningIn::default(),
                lastfm: LastfmSigning::default(),
                _token_checked: Task::ready(()),
            },
            rails: Rails {
                seek: Rail::default(),
                seek_pointed: None,
                volume: Rail::default(),
                grabbed: None,
                volume_settled: Task::ready(()),
                volume_unkept: None,
                volume_aimed: None,
                muted_from: None,
                band_retuning: Task::ready(()),
                band_untold: None,
                held_band: None,
                curve_plotted: Rc::default(),
            },
            orders: Orders {
                queue_order: RowOrder::default(),
                queue_reading: Direction::default(),
                row_order: RowOrder::default(),
                row_reading: Direction::default(),
                keeping: false,
                query_sort: SortOrder::default(),
                query_reading: SortOrder::default().reads(),
                query_cap: None,
                ordering: None,
                ordering_at: Point::default(),
                ordering_typed: TypeAhead::default(),
            },
            remembered: Remembered {
                tab: remember_tab,
                window_size: remember_window_size,
                settings_category: remember_settings_category,
                last_window_size,
                _window_size_settled: Task::ready(()),
            },
            dropped: Dropped {
                incoming: None,
                taking_in: None,
                watching_the_drag: Task::ready(()),
                weighing_the_drag: Task::ready(()),
                _taking_in: Task::ready(()),
            },
            typed: Typed {
                words: typed_words,
                isrc: typed_isrc,
                name,
                zone_name,
                search,
            },
            settings_shown: SettingsShown {
                category: settings_category,
                page: None,
                music_folder_standing: FolderStanding::default(),
                looking_at_the_music_folder: Task::ready(()),
                reading_the_room: Task::ready(()),
                looking_at_the_room: Task::ready(()),
                room_shape: None,
                audition: None,
                test_setup: crate::views::settings::Setup::default(),
                fitting_the_preamp: Fitting::idle(),
                making_the_vault: Task::ready(()),
                tending: Tending::default(),
                looking_at_the_typed_root: Task::ready(()),
                armed: None,
                code_regenerated: false,
                pairing_shown: false,
                own_server_asked: false,
                reset_everything_landed: false,
                vault_named: None,
                newer: None,
                looking_for_updates: Task::ready(()),
                trying: None,
                heard: None,
            },
            social: Social {
                wrapped: None,
                reading_wrapped: None,
                sharing: None,
                shared_lists: Vec::new(),
                sharing_redraws: None,
                party_offered: None,
                list_offered: None,
            },
            shows: Shows {
                artist: ArtistShows::default(),
                search: SearchShows::default(),
                artists: cx.global::<ResonateApp>().artists_drawn,
                missing: MissingShows::default(),
                playlists: PlaylistsDrawn::default(),
                time_left: false,
            },
            typing: TypedAhead {
                ahead: TypeAhead::default(),
                _stops: Task::ready(()),
                last_typed: None,
                landed_by_a_jump: None,
                waiting_for_the_results: false,
                left_for_the_search: None,
            },
        };
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_bounds_changed(window, cx);
        })
        .detach();
        cx.observe_window_visibility(window, |this, visibility, _, cx| {
            this.library.update(cx, |library, _| {
                library.window_seen(visibility.is_visible())
            });
        })
        .detach();
        cx.on_app_quit(|this, cx| {
            this.keep_the_volume(cx);
            future::ready(())
        })
        .detach();
        cx.on_release(|this, _| this.keep_the_volume_as_the_window_leaves())
            .detach();
        view.look_for_a_newer_release(cx);
        let opened_on_statistics = view.pane == Pane::Statistics;
        view.library.update(cx, |library, cx| {
            library.statistics_shown(opened_on_statistics, cx);
        });
        view
    }

    pub(crate) const fn drawn_at(&self) -> SystemTime {
        self.drawn_at
    }

    pub(super) fn window_bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(size) = WindowSize::from_pixels(window.window_bounds().get_bounds().size) else {
            return;
        };
        if self.remembered.last_window_size == Some(size) {
            return;
        }
        self.remembered.last_window_size = Some(size);
        if !self.remembered.window_size {
            return;
        }
        cx.update_global::<ResonateApp, _>(|global, _| global.window_size = Some(size));

        self.remembered._window_size_settled = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(WINDOW_SIZE_SETTLE).await;
            let stored = this.update(cx, |this, cx| {
                if this.remembered.window_size && this.remembered.last_window_size == Some(size) {
                    this.store(&Setting::WindowSize(size), cx);
                }
            });
            let _ = stored;
        });
    }
}
