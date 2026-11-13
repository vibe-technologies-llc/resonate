use std::{
    ffi::OsString,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::Duration,
};

use crossbeam_channel::bounded;
use resonate_core::{MediaLocation, Resumption};
use resonate_engine::{Command, Opens, Placement, Player, Zones};
use resonate_library::Library;
use resonate_mpris::{Queueing, Running};

use crate::{
    arguments::queue_items_served,
    cli::Cli,
    config,
    config::Config,
    discord,
    engine_settings::engine_config,
    equaliser,
    error::Result,
    kinship, library_path, listen, measuring, mpris, online,
    online::Asking,
    playing_from, providers,
    rendering::Rendering,
    resuming::resuming,
    served::Served,
    settings_path,
    sinks::{confirm_sink_once_the_window_is_open, wanted_sink},
    submitting, sync, versions, words,
};

#[cfg(feature = "online")]
fn mirroring(config: &Config) -> Option<Arc<dyn resonate_ui::Mirroring>> {
    Some(crate::musicbrainz::Mirroring::of(config))
}

#[cfg(feature = "online")]
fn updates_checked(config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    crate::updates::checking(config)
}

#[cfg(not(feature = "online"))]
const fn updates_checked(_config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    None
}

#[cfg(not(feature = "online"))]
const fn mirroring(_config: &Config) -> Option<Arc<dyn resonate_ui::Mirroring>> {
    None
}

pub(crate) fn launch(cli: Cli, config: Config, library: Arc<Library>) -> Result<()> {
    let starting = crate::starting::one_window_at_a_time();
    if settings_of_its_own(&cli).is_empty()
        && let Some(window) = Running::a_window().ok().flatten()
    {
        return handed_to(
            &window,
            &cli.files,
            &mpris::tuning_of(&cli),
            &Served::of(&config, settings_path(&cli).ok()),
        );
    }
    let places = resonate_ui::Places {
        config: settings_path(&cli)?,
        library: library_path(&cli, &config)?,
        equaliser: config::equaliser_dir()?,
    };
    let settings = crate::settings::File::at(places.config.clone());
    let served = Served::for_the_window(&config, settings_path(&cli).ok());
    let sources = playing_from(Some(&library), &served);
    let rendering = Rendering::for_the_window(&config, wanted_sink(&cli, &config).as_ref());
    let renderers = rendering.switch();
    let player = rendering.player(
        kinship::kin_from(engine_config(&cli, &config), Some(&library)),
        Arc::clone(&sources),
    )?;
    let player = Arc::new(match config::analyses_dir() {
        Some(dir) => player.keeping_analyses(resonate_engine::KeptAnalyses::at(dir)),
        None => player,
    });
    confirm_sink_once_the_window_is_open(&player, wanted_sink(&cli, &config));
    let keeps = resuming(&config, Some(&library));
    #[cfg(feature = "sync")]
    let files = crate::together::invitations_taken_out(&cli.files);
    #[cfg(not(feature = "sync"))]
    let files = cli.files.clone();
    queue(&player, &files, keeps.then(|| library.as_ref()), &served)?;

    let (asked_to_quit, quitting) = bounded(1);
    let (attending, attention) = crossbeam_channel::unbounded();
    let (asked_to_raise, raising) = bounded(1);
    let (follow, followed) = crossbeam_channel::unbounded();
    let notify = Arc::new(AtomicBool::new(config.notifies()));
    let by_sound = online::by_sound(&config);
    let lyrics_by_the_locale = online::lyrics_by_the_locale(&config);
    let opener = crate::zoning::Opener::new(
        kinship::kin_from(engine_config(&cli, &config), Some(&library)),
        &sources,
        rendering,
        mpris::Serving::new(
            &sources,
            Some(&library),
            Some(places.config.clone()),
            Some(asked_to_quit.clone()),
            Some(mpris::Window {
                attention,
                raise: asked_to_raise,
                followed: follow.clone(),
            }),
            &notify,
        ),
    );
    let kept_zones = crate::zoning::kept_zones(&library, keeps);
    let zones = Zones::start(
        Arc::clone(&player),
        crate::zoning::first_named(&kept_zones),
        Arc::clone(&opener) as Arc<dyn Opens>,
    );
    let silenced = Arc::downgrade(&zones);
    crate::signals::quit_when_told(
        asked_to_quit,
        crate::signals::silencing(move || {
            silenced.upgrade().map_or_else(Vec::new, |zones| {
                zones.listed().into_iter().map(|zone| zone.player).collect()
            })
        }),
    );
    drop(starting);
    crate::zoning::restore(&zones, &library, &kept_zones);
    let listens = listen::in_the_window(&config, opener.teller());
    let desktop = the_desktop_asks(&config);
    let (hears, heard) = crossbeam_channel::unbounded();
    watch_the_desktop(hears);
    let presenter = Arc::new(discord::start(&player, Some(&library), &config));
    #[cfg(any(windows, target_os = "macos"))]
    let _awake = {
        let zones = Arc::downgrade(&zones);
        crate::awake::start(move || {
            zones.upgrade().map_or_else(Vec::new, |zones| {
                zones.listed().into_iter().map(|zone| zone.player).collect()
            })
        })
    };
    let submitting = submitting::start(&config, &library, &player);
    #[cfg(feature = "online")]
    crate::musicbrainz::keep_fresh(&config);
    #[cfg(feature = "sync")]
    let together = crate::together::start(&config, &library).unwrap_or_else(|error| {
        tracing::warn!(%error, "shared playlists could not start");
        None
    });
    #[cfg(feature = "sync")]
    crate::party_room::in_the_window(crate::party_room::Seats {
        zones: Arc::clone(&zones),
        library: Arc::clone(&library),
        served: served.clone(),
        server: sync::server_of(&config),
        trusted: config.sync_server_certificate.clone(),
        name: config.shared_name.clone().unwrap_or_default(),
        told: follow,
    });
    let syncing = sync::start(&config, Some(&places.config), &library);
    #[cfg(feature = "sync")]
    syncing.drive(resonate_driven::Driven::zones(
        &zones,
        resonate_driven::ByTheirTags,
    ));
    let syncing_for_the_window = syncing.for_the_window();
    #[cfg(feature = "sync")]
    let together_for_the_window = together
        .as_ref()
        .map(|_| crate::together::for_the_window(&library));
    #[cfg(not(feature = "sync"))]
    let together_for_the_window = None;
    let (opening, opened) = crossbeam_channel::unbounded();
    queue_what_the_system_opens(opened, served.clone());
    let present = Arc::clone(&presenter) as Arc<dyn resonate_ui::Present>;
    let leaving: resonate_ui::Leaving = {
        let zones = Arc::clone(&zones);
        Box::new(move || {
            providers::stop_the_tools();
            drop(presenter);
            syncing.leave();
            #[cfg(feature = "sync")]
            crate::party_room::out_of_the_window();
            #[cfg(feature = "sync")]
            drop(together);
            submitting.leave();
            zones.leave();
        })
    };
    let outcome = resonate_ui::run(
        Arc::clone(&zones),
        Arc::clone(&library),
        resonate_ui::Lookups {
            lyricists: Arc::new(online::lyricists(
                &config,
                Some(Arc::clone(&library)),
                &lyrics_by_the_locale,
            )),
            fingerprinters: Arc::new(online::fingerprinters(
                &config,
                Arc::new(library.sources()),
                &by_sound,
                Asking::ForTheListener,
            )),
            reference: online::reference(&config, Asking::ForTheListener),
            for_the_pass: resonate_ui::Consulted {
                reference: online::reference(&config, Asking::InTheBackground),
                fingerprinters: Arc::new(online::fingerprinters(
                    &config,
                    Arc::new(library.sources()),
                    &by_sound,
                    Asking::InTheBackground,
                )),
            },
            scrobblers: online::scrobblers(&config),
            signs_in: providers::signs_in(),
            spotify: online::signs_in_to_spotify(&config),
            checks_youtube_music: providers::checks_youtube_music(settings_path(&cli).ok()),
            signs_in_to_youtube_music: providers::signs_in_to_youtube_music(),
            signs_in_to_jellyfin: providers::signs_in_to_jellyfin(),
            corrections: Arc::new(online::corrections(&config, Some(Arc::clone(&library)))),
            bindings: equaliser::bound(&config),
            measurers: equaliser::measured_by(&config),
            measures: Some(Arc::new(measuring::Sweeping)),
            online: resonate_ui::Online {
                enabled: config.online_enabled(),
                after_scan: config.enriches_after_scan(),
                enrich_on_metered: config.enriches_on_metered(),
                musicbrainz_mirror: config.mirrors_musicbrainz(),
                musicbrainz_replication_token: config
                    .musicbrainz_replication_token
                    .clone()
                    .unwrap_or_default(),
                stall_downloads_on_metered: config.stalls_downloads_on_metered(),
                studies: config.studies(),
                lyrics: config.fetches_lyrics(),
                contact: config.contact.clone().unwrap_or_default(),
                acoustid_key: config.acoustid_key.clone().unwrap_or_default(),
                audd_token: config.audd_token.clone().unwrap_or_default(),
                listenbrainz_token: config.listenbrainz_token.clone().unwrap_or_default(),
                lastfm_key: config.lastfm_key.clone().unwrap_or_default(),
                lastfm_secret: config.lastfm_secret.clone().unwrap_or_default(),
                lastfm_session: config.lastfm_session.clone().unwrap_or_default(),
                subsonic: config.subsonic.clone().unwrap_or_default(),
                subsonic_user: config.subsonic_user.clone().unwrap_or_default(),
                subsonic_password: config.subsonic_password.clone().unwrap_or_default(),
                jellyfin: config.jellyfin.clone().unwrap_or_default(),
                jellyfin_user: config.jellyfin_user.clone().unwrap_or_default(),
                jellyfin_password: config.jellyfin_password.clone().unwrap_or_default(),
                jellyfin_token: config.jellyfin_token.clone().unwrap_or_default(),
                jellyfin_user_id: config.jellyfin_user_id.clone().unwrap_or_default(),
                jellyfin_device: config.jellyfin_device.clone().unwrap_or_default(),
                tidal_client_id: config.tidal_client_id.clone().unwrap_or_default(),
                tidal_client_secret: config.tidal_client_secret.clone().unwrap_or_default(),
                tidal_refresh_token: config.tidal_refresh_token.clone().unwrap_or_default(),
                spotify_client_id: config.spotify_client_id.clone().unwrap_or_default(),
                spotify_refresh_token: config.spotify_refresh_token.clone().unwrap_or_default(),
                hifi_api: config.hifi_api.clone().unwrap_or_default(),
                hifi_secret: config.hifi_secret.clone().unwrap_or_default(),
                monochrome: config.monochrome.clone().unwrap_or_default(),
                hosted_services: config.hosted_services_allowed(),
                youtube_music: config.turns_youtube_music_on(),
                youtube_music_cookies: config.youtube_music_cookies.clone().unwrap_or_default(),
                yt_dlp: config.yt_dlp.clone().unwrap_or_default(),
                youtube_music_keep_order: config.keeps_youtubes_order(),
            },
            serving: served.serving(),
            sourcing: {
                let (register, unkept) = providers::sourced(settings_path(&cli).ok());
                resonate_ui::Sourcing {
                    inbox: config.inbox.clone(),
                    register,
                    unkept,
                }
            },
            listens,
            mirroring: mirroring(&config),
        },
        resonate_ui::Stored {
            settings: Arc::new(settings),
            caret: caret_as_the_desktop_blinks(),
            places,
            resume: keeps,
            history_kept: config.history_kept(),
            organise_as: config.organise_as().to_string(),
            notify,
            by_sound,
            lyrics_by_the_locale,
            flac_above_96k_for: config.flac_above_96k_for.clone(),
            convolution: config.convolution.clone(),
            speaker_angle: config.speaker_angle(),
            speaker_elevation: config.speaker_elevation(),
            head_yaw: config.head_yaw(),
            music_folder: config.music_folder.clone(),
            file_dropped: config.files_what_is_dropped(),
            window_buttons: config.window_buttons(),
            scroll_volume: config.scrolls_the_volume(),
            mouse_navigation: config.navigates_with_mouse_buttons(),
            reduced_motion: config.reduces_motion_where_the_desktop_asks(desktop.reduces_motion),
            scrollbars: config.scrollbars(),
            language: config.language(),
            spectral: config.spectral(),
            tabs: config.tabs(),
            remember_tab: config.remembers_tab(),
            last_tab: config.remembered_tab(),
            artists_drawn: config.artists_drawn(),
            remember_window_size: config.remembers_window_size(),
            window_size: config.window_size(),
            remember_settings_category: config.remembers_settings_category(),
            last_settings_category: config.remembered_settings_category(),
            presence: config.presence(),
            present,
            renderers,
            synchronised: resonate_ui::Synchronised {
                enabled: config.sync.unwrap_or(true),
                nearby_only: config.syncs_nearby_only(),
                server: config.sync_server.clone().unwrap_or_default(),
                device: config.sync_device.clone().unwrap_or_default(),
                shared_name: config.shared_name.clone().unwrap_or_default(),
                songs: config.sync_songs.unwrap_or_default(),
                control: config.sync_control.unwrap_or_default(),
            },
            syncing: syncing_for_the_window,
            together: together_for_the_window,
            reads_as_a_code: sync::code_reader(),
            launcher: Arc::new(crate::launcher::Icons::start()),
            updates: updates_checked(&config),
            check_for_updates: config.checks_for_updates(),
            update_dismissed: config.update_dismissed.clone(),
        },
        config.appearance_where_the_desktop_asks(desktop.more_contrast),
        resonate_ui::Bus {
            look: desktop.look,
            desktop: heard,
            quit: quitting,
            raise: raising,
            attention: attending,
            followed,
            opened: opening,
        },
        leaving,
    );
    outcome?;
    Ok(())
}

const WHAT_IS_OPENED_WAITS_FOR_THE_WINDOW: Duration = Duration::from_secs(15);

const THE_WINDOW_IS_LOOKED_FOR_EVERY: Duration = Duration::from_millis(100);

fn queue_what_the_system_opens(opened: crossbeam_channel::Receiver<Vec<String>>, served: Served) {
    let spawned = thread::Builder::new()
        .name("resonate-opened".to_owned())
        .spawn(move || {
            for links in opened {
                let arguments: Vec<OsString> = links.into_iter().map(OsString::from).collect();
                let Some(window) = the_window_once_it_answers() else {
                    tracing::warn!("no window answered for what the system opened");
                    continue;
                };
                if let Err(error) = queued_in(&window, &arguments, &served) {
                    tracing::warn!(%error, "what the system opened was not queued");
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "nothing listens for what the system opens");
    }
}

fn the_window_once_it_answers() -> Option<Running> {
    let deadline = std::time::Instant::now() + WHAT_IS_OPENED_WAITS_FOR_THE_WINDOW;
    loop {
        if let Some(window) = Running::a_window().ok().flatten() {
            return Some(window);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        thread::sleep(THE_WINDOW_IS_LOOKED_FOR_EVERY);
    }
}

#[derive(Clone, Copy, Default)]
struct DesktopAsks {
    more_contrast: bool,
    reduces_motion: bool,
    look: resonate_core::DesktopLook,
}

fn the_desktop_asks(config: &Config) -> DesktopAsks {
    let follows = config.appearance_where_the_desktop_asks(false).follows;
    if config.theme.is_some() && config.reduced_motion.is_some() && !follows.scheme && !follows.text
    {
        return DesktopAsks::default();
    }

    let preferences = resonate_mpris::desktop_preferences();

    DesktopAsks {
        more_contrast: preferences.contrast == resonate_mpris::Contrast::More,
        reduces_motion: preferences.motion == resonate_mpris::Motion::Reduced,
        look: look_of(preferences),
    }
}

fn look_of(preferences: resonate_mpris::Preferences) -> resonate_core::DesktopLook {
    resonate_core::DesktopLook {
        ground: match preferences.scheme {
            resonate_mpris::Scheme::Dark => Some(resonate_core::Ground::Dark),
            resonate_mpris::Scheme::Light => Some(resonate_core::Ground::Light),
            resonate_mpris::Scheme::Unsaid => None,
        },
        text_scale: preferences.text_scale,
    }
}

fn watch_the_desktop(told: crossbeam_channel::Sender<resonate_core::DesktopLook>) {
    let watching = resonate_mpris::watch_the_desktop(move |preferences| {
        let _ = told.send(look_of(preferences));
    });
    if let Err(error) = watching {
        tracing::debug!(%error, "the desktop's colour scheme and text scale are read once, at start");
    }
}

fn caret_as_the_desktop_blinks() -> resonate_ui::CaretBlink {
    let caret = resonate_ui::CaretBlink::as_built();
    let told = caret.clone();
    let asked = thread::Builder::new()
        .name("resonate-caret".to_owned())
        .spawn(move || match resonate_mpris::caret_blinking() {
            resonate_mpris::Blinking::Steady => told.steady(),
            resonate_mpris::Blinking::Every(half) => told.every(half),
            resonate_mpris::Blinking::Unsaid => {}
        });
    if let Err(error) = asked {
        tracing::debug!(%error, "the desktop was not asked how its caret blinks");
    }
    caret
}

fn settings_of_its_own(cli: &Cli) -> Vec<&'static str> {
    [
        ("--config", cli.config.is_some()),
        ("--library", cli.library.is_some()),
        ("--vault", cli.vault.is_some()),
    ]
    .into_iter()
    .filter_map(|(flag, given)| given.then_some(flag))
    .collect()
}

fn handed_to(
    window: &Running,
    arguments: &[OsString],
    tuning: &[(resonate_mpris::Tunable, String)],
    served: &Served,
) -> Result<()> {
    if let Err(error) = window.tune(tuning) {
        tracing::warn!(%error, "the window already open did not take the settings given");
        told!("{}", words::main::untuned());
    }
    queued_in(window, arguments, served)?;
    said!("{}", words::main::handed(window.name().to_string()));
    Ok(())
}

fn queued_in(window: &Running, arguments: &[OsString], served: &Served) -> Result<()> {
    let (invitations, arguments): (Vec<&OsString>, Vec<&OsString>) = arguments
        .iter()
        .partition(|argument| names_an_invitation(argument));
    let arguments: Vec<OsString> = arguments.into_iter().cloned().collect();
    let mut rows: Vec<_> = queue_items_served(&arguments, served)
        .into_iter()
        .map(|item| (item.location, item.span))
        .collect();
    rows.extend(
        invitations
            .into_iter()
            .filter_map(|invitation| MediaLocation::from_uri_within(&invitation.to_string_lossy())),
    );
    if !rows.is_empty() {
        window.queue(
            &rows,
            Queueing {
                at: Placement::Next,
                play: true,
            },
        )?;
    }
    window.raise()?;
    Ok(())
}

#[cfg(feature = "sync")]
fn names_an_invitation(argument: &OsString) -> bool {
    resonate_sync::shared::invitation_in(&argument.to_string_lossy()).is_some()
}

#[cfg(not(feature = "sync"))]
fn names_an_invitation(_argument: &OsString) -> bool {
    false
}

pub(crate) fn queue(
    player: &Player,
    arguments: &[OsString],
    kept: Option<&Library>,
    served: &Served,
) -> Result<()> {
    if !arguments.is_empty() {
        player.send(Command::Load {
            items: versions::played_as_chosen(
                kept,
                mpris::claimed_by(kept, queue_items_served(arguments, served)),
            ),
            start_at: 0,
            autoplay: true,
        })?;
        return Ok(());
    }

    let Some(resumption) = resumed(kept) else {
        return Ok(());
    };
    player.send(Command::Resume(resumption))?;
    Ok(())
}

pub(crate) fn resumed(kept: Option<&Library>) -> Option<Resumption> {
    match kept?.resumption() {
        Ok(resumption) => resumption,
        Err(error) => {
            tracing::warn!(%error, "the queue kept from the last run could not be read");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[test]
    fn a_launch_naming_a_settings_file_catalog_or_vault_opens_a_window_of_its_own() {
        let own_settings = |arguments: &[&str]| {
            let cli = Cli::try_parse_from(arguments.iter().copied()).expect("a launch");
            settings_of_its_own(&cli)
        };

        assert!(own_settings(&["resonate", "a.flac"]).is_empty());
        assert!(own_settings(&["resonate", "--sink", "alsa_output.usb", "a.flac"]).is_empty());
        assert_eq!(
            own_settings(&[
                "resonate",
                "--config",
                "c.toml",
                "--library",
                "l.db",
                "--vault",
                "v"
            ]),
            ["--config", "--library", "--vault"]
        );
    }

    #[test]
    fn a_launch_handed_to_a_window_carries_the_settings_the_window_can_take() {
        use resonate_mpris::Tunable;

        let tuning = |arguments: &[&str]| {
            let cli = Cli::try_parse_from(arguments.iter().copied()).expect("a launch");
            mpris::tuning_of(&cli)
        };

        assert!(tuning(&["resonate", "a.flac"]).is_empty());
        assert_eq!(
            tuning(&[
                "resonate",
                "--sink",
                "alsa_output.usb",
                "--quality",
                "very-high",
                "--filter-phase",
                "minimum",
                "--dither",
                "none",
                "--noise-shaping",
                "lipshitz",
                "--no-bit-perfect",
                "a.flac"
            ]),
            [
                (Tunable::Sink, "alsa_output.usb".to_owned()),
                (Tunable::Quality, "very-high".to_owned()),
                (Tunable::FilterPhase, "minimum".to_owned()),
                (Tunable::Dither, "none".to_owned()),
                (Tunable::NoiseShaping, "lipshitz".to_owned()),
                (Tunable::BitPerfect, "false".to_owned()),
            ]
        );
        assert_eq!(
            tuning(&["resonate", "--bit-perfect"]),
            [(Tunable::BitPerfect, "true".to_owned())]
        );
    }
}
