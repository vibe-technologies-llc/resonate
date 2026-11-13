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
        field::{Blurred, Caught, Field, Submitted},
        focus::Controls,
        listing::{self},
        matching::{self},
        missing::MissingShows,
        part::Parts,
        playlists::PlaylistsDrawn,
        queue::{QueueMeasure, QueueNames, TakenBack},
        search::SearchShows,
        settings::{
            Account, Category, CheckingYoutubeMusic, Fitting, LastfmAccount, LastfmSigning,
            QuickConnecting, Service, SigningIn, SigningInToYoutubeMusic, SpotifyState, Tending,
            TidalAccount, YoutubeMusicField, spotify,
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
        let presence = global.presence.clone();
        let reference = global.reference.clone();
        let for_the_pass = global.for_the_pass.clone();
        let corrections = Arc::clone(&global.corrections);
        let bindings = global.bindings.clone();
        let measurers = global.measurers.clone();
        let places = global.places.clone();
        let resume = global.resume;
        let organise_as = global.organise_as.clone();
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
            Field::new(words::root::search_placeholder(), window, cx)
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

        let zone_name = cx.new(|cx| Field::new(words::zones::name_this_zone(), window, cx));
        cx.subscribe_in(&zone_name, window, |this, _, _: &Submitted, window, cx| {
            this.zone_named(window, cx);
        })
        .detach();

        let name = cx.new(|cx| Field::new(words::root::name_placeholder(), window, cx));
        cx.subscribe_in(&name, window, |this, _, _: &Submitted, window, cx| {
            this.name_given(window, cx);
        })
        .detach();

        let contact = cx.new(|cx| {
            let mut field = Field::new(words::root::contact_placeholder(), window, cx);
            field.hold(online.contact.clone(), cx);
            field
        });
        cx.subscribe_in(&contact, window, |this, _, _: &Submitted, window, cx| {
            this.contact_given(window, cx);
        })
        .detach();
        cx.subscribe(&contact, |this, _, _: &Blurred, cx| this.contact_left(cx))
            .detach();

        let acoustid = cx.new(|cx| {
            let mut field = Field::new(words::root::acoustid_placeholder(), window, cx).masked();
            field.hold(online.acoustid_key.clone(), cx);
            field
        });
        cx.subscribe_in(&acoustid, window, |this, _, _: &Submitted, window, cx| {
            this.acoustid_key_given(window, cx);
        })
        .detach();
        cx.subscribe(&acoustid, |this, _, _: &Blurred, cx| {
            this.acoustid_key_left(cx)
        })
        .detach();

        let replication = cx.new(|cx| {
            let mut field =
                Field::new(words::root::replication_token_placeholder(), window, cx).masked();
            field.hold(online.musicbrainz_replication_token.clone(), cx);
            field
        });
        cx.subscribe_in(
            &replication,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.replication_token_given(window, cx);
            },
        )
        .detach();
        cx.subscribe(&replication, |this, _, _: &Blurred, cx| {
            this.replication_token_left(cx)
        })
        .detach();

        let audd = cx.new(|cx| {
            let mut field = Field::new(words::root::audd_placeholder(), window, cx).masked();
            field.hold(online.audd_token.clone(), cx);
            field
        });
        cx.subscribe_in(&audd, window, |this, _, _: &Submitted, window, cx| {
            this.audd_token_given(window, cx);
        })
        .detach();
        cx.subscribe(&audd, |this, _, _: &Blurred, cx| this.audd_token_left(cx))
            .detach();

        let listenbrainz = cx.new(|cx| {
            let mut field =
                Field::new(words::root::listenbrainz_placeholder(), window, cx).masked();
            field.hold(online.listenbrainz_token.clone(), cx);
            field
        });
        cx.subscribe_in(
            &listenbrainz,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.listenbrainz_token_given(window, cx);
            },
        )
        .detach();
        cx.subscribe(&listenbrainz, |this, _, _: &Blurred, cx| {
            this.listenbrainz_token_left(cx)
        })
        .detach();

        let subsonic = Account::fields(Service::Subsonic, &online, window, cx);
        let jellyfin = Account::fields(Service::Jellyfin, &online, window, cx);
        let tidal = TidalAccount::fields(&online, window, cx);
        let lastfm = LastfmAccount::fields(&online, window, cx);
        let spotify_client = spotify::field(&online, window, cx);
        let youtube_music = YoutubeMusicField::fields(&online, window, cx);

        let discord_app = cx.new(|cx| {
            let mut field = Field::new(words::root::discord_app_placeholder(), window, cx);
            field.hold(
                presence.app.map(|app| app.to_string()).unwrap_or_default(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &discord_app,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.discord_app_given(window, cx);
            },
        )
        .detach();

        let discord_icon = cx.new(|cx| {
            let mut field = Field::new(words::root::discord_icon_placeholder(), window, cx);
            field.hold(
                presence
                    .icon
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &discord_icon,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.discord_icon_given(window, cx);
            },
        )
        .detach();

        let synchronised = cx.global::<ResonateApp>().synchronised.clone();
        let sync_server = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::sync_server_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.server.clone(), cx);
            field
        });
        cx.subscribe_in(
            &sync_server,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.sync_server_given(window, cx);
            },
        )
        .detach();
        let sync_device = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::sync_device_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.device.clone(), cx);
            field
        });
        cx.subscribe_in(
            &sync_device,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.sync_device_given(window, cx);
            },
        )
        .detach();
        let shared_name = cx.new(|cx| {
            let mut field = Field::new(
                crate::words::settings_sync::shared_name_placeholder(),
                window,
                cx,
            );
            field.hold(synchronised.shared_name.clone(), cx);
            field
        });
        cx.subscribe_in(
            &shared_name,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.shared_name_given(window, cx);
            },
        )
        .detach();
        let party_chat = cx.new(|cx| Field::new(crate::words::party::say_something(), window, cx));
        cx.subscribe_in(&party_chat, window, |this, _, _: &Submitted, window, cx| {
            this.say_in_the_party(window, cx);
        })
        .detach();
        let sync_code = cx.new(|cx| {
            Field::new(
                crate::words::settings_sync::sync_code_placeholder(),
                window,
                cx,
            )
        });
        cx.subscribe_in(&sync_code, window, |this, _, _: &Submitted, window, cx| {
            this.sync_code_given(window, cx);
        })
        .detach();

        let minimum_length = cx.new(|cx| {
            let mut field = Field::new(words::root::seconds_from_0_to_600(), window, cx);
            field.hold(
                cx.global::<ResonateApp>()
                    .library
                    .music_filters()
                    .minimum_length
                    .seconds()
                    .to_string(),
                cx,
            );
            field
        });
        cx.subscribe_in(
            &minimum_length,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.minimum_length_given(window, cx);
            },
        )
        .detach();

        let organising = cx.new(|cx| {
            let mut field = Field::new(words::root::organising_placeholder(), window, cx);
            field.hold(organise_as, cx);
            field
        });
        cx.subscribe_in(&organising, window, |this, _, _: &Submitted, window, cx| {
            this.layout_given(window, cx);
        })
        .detach();
        cx.subscribe(&organising, |this, _, _: &Blurred, cx| this.layout_left(cx))
            .detach();

        let (typed_words, typed_isrc) = matching::matching_fields(window, cx);
        let typed_root = cx.new(|cx| Field::new(words::root::typed_root_placeholder(), window, cx));
        cx.subscribe_in(&typed_root, window, |this, _, _: &Submitted, window, cx| {
            this.root_typed(window, cx);
        })
        .detach();
        cx.subscribe_in(&typed_root, window, |this, _, _: &Blurred, window, cx| {
            this.root_typed(window, cx);
        })
        .detach();

        let finding = cx.new(|cx| {
            Field::new(
                crate::words::settings_pane::filter_placeholder(),
                window,
                cx,
            )
        });
        cx.subscribe_in(&finding, window, |this, _, _: &Submitted, window, cx| {
            this.leave_filter(window, cx);
        })
        .detach();

        let server = cx.new(ServerModel::new);
        cx.observe(&server, |_, _, cx| cx.notify()).detach();
        cx.observe_global::<ResonateApp>(|this, cx| {
            this.server.update(cx, |server, cx| server.refresh(cx));
            if this.pane == Pane::Server && !this.server.read(cx).is_open() {
                this.set_pane(Pane::default(), cx);
            }
        })
        .detach();

        let server_search =
            cx.new(|cx| Field::new(words::served::search_placeholder(), window, cx));
        cx.subscribe_in(
            &server_search,
            window,
            |this, _, _: &Submitted, window, cx| {
                this.search_the_server(window, cx);
            },
        )
        .detach();

        let figure = cx.new(|cx| Field::new(words::root::figure_placeholder(), window, cx));
        cx.subscribe_in(&figure, window, |this, _, _: &Submitted, window, cx| {
            this.figure_given(window, cx);
        })
        .detach();
        cx.subscribe(&figure, |this, _, _: &Blurred, cx| this.figure_left(cx))
            .detach();

        let looking = cx.new(|cx| Field::new(words::root::looking_placeholder(), window, cx));
        cx.subscribe_in(&looking, window, |this, _, _: &Submitted, _window, cx| {
            this.look_for_a_device(cx);
        })
        .detach();

        let measured_as =
            cx.new(|cx| Field::new(words::root::measured_as_placeholder(), window, cx));

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
                inspector: ScrollHandle::new(),
                statistics: ScrollHandle::new(),
                party: ScrollHandle::new(),
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
                server_rows: UniformListScrollHandle::new(),
                shelves: RefCell::new(AHashMap::new()),
                left_at: AHashMap::new(),
            },
            rooms: Rooms {
                grid_width: Rc::default(),
                hero_width: Rc::new(Cell::new(px(0.0))),
                hero_height: Rc::new(Cell::new(px(0.0))),
                playing: Rc::new(Cell::new(px(0.0))),
                status: Rc::new(Cell::new(px(0.0))),
                inspected: Rc::new(Cell::new(px(0.0))),
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
                contact,
                acoustid,
                replication,
                audd,
                listenbrainz,
                subsonic,
                jellyfin,
                tidal,
                lastfm,
                spotify_client,
                youtube_music,
                discord_app,
                discord_icon,
                sync_server,
                sync_device,
                shared_name,
                party_chat,
                sync_code,
                organising,
                minimum_length,
                root: typed_root,
                finding,
                server_search,
                figure,
                looking,
                measured_as,
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
                party_redraws: None,
                party_offered: None,
                list_offered: None,
                handing_armed: None,
            },
            shows: Shows {
                artist: ArtistShows::default(),
                search: SearchShows::default(),
                artists: cx.global::<ResonateApp>().artists_drawn,
                missing: MissingShows::default(),
                playlists: PlaylistsDrawn::default(),
                bitrate_graph: false,
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
