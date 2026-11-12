#![deny(clippy::print_stdout, clippy::print_stderr)]

#[macro_use]
mod said;

mod abx;
mod analyse;
mod arguments;
#[cfg(any(windows, target_os = "macos"))]
mod awake;
mod catalog;
mod cli;
mod config;
mod connected;
mod discord;
mod engine_settings;
mod equaliser;
mod error;
mod explain;
mod favourites;
#[cfg(test)]
mod fixtures;
mod followed;
mod gaming;
mod info;
mod input;
#[cfg(feature = "online")]
mod jellyfin;
mod kinship;
mod lastfm;
mod lasting;
#[cfg(all(feature = "ui", unix, not(target_os = "macos")))]
mod launcher;
#[cfg(all(feature = "ui", any(windows, target_os = "macos")))]
#[path = "plain_launcher.rs"]
mod launcher;
mod listen;
mod mcp;
mod measuring;
mod metered;
mod missing;
mod mpris;
#[cfg(feature = "online")]
mod musicbrainz;
mod online;
mod organising;
#[cfg(feature = "sync")]
mod party;
#[cfg(all(feature = "sync", feature = "ui"))]
mod party_room;
mod passes;
mod playing;
mod playlist;
mod playlists;
mod providers;
mod readout;
mod reloading;
mod rendering;
mod resuming;
mod roots;
mod scanning;
mod served;
#[cfg(feature = "ui")]
mod settings;
mod share;
#[cfg(unix)]
mod signals;
#[cfg(windows)]
#[path = "interrupts.rs"]
mod signals;
#[cfg(feature = "online")]
mod signing_in;
mod sinks;
mod sleep;
mod spotify;
#[cfg(feature = "ui")]
mod starting;
mod stats;
mod studies;
mod submitting;
mod suggest;
mod surveyed;
mod sync;
mod table;
mod tagging;
#[cfg(feature = "sync")]
mod together;
mod trashing;
#[cfg(feature = "online")]
mod updates;
mod vault;
mod versions;
mod vocabulary;
mod wanting;
#[cfg(feature = "ui")]
mod window;
mod wrapped;
#[cfg(feature = "online")]
mod ytmusic;
#[cfg(feature = "ui")]
mod zoning;

#[cfg_attr(
    not(all(
        feature = "ui",
        feature = "online",
        feature = "sync",
        feature = "mcp",
        feature = "discord"
    )),
    allow(dead_code)
)]
mod words {
    include!(concat!(env!("OUT_DIR"), "/words.rs"));
}

use std::{
    env,
    io::{self},
    path::PathBuf,
    process::ExitCode,
    sync::Arc,
};

use clap::Parser;
use resonate_codec::Sources;
use resonate_library::{
    Aged, DeliveryFolder, EnrichOptions, HistoryKept, Library, ListeningService, StudyFilter,
    Upkeep, Vault, VaultFiles,
};
use resonate_mpris::Reloadable;
use tracing_subscriber::{
    EnvFilter, filter::ParseError, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

#[cfg(not(feature = "ui"))]
use crate::playing::launch;
#[cfg(feature = "ui")]
use crate::window::launch;
use crate::{
    arguments::{cut_of_argument, local_path},
    cli::{Cli, Sub},
    config::Config,
    connected::{party_asked, shared, synced},
    engine_settings::{engine_config, one_way_on_bit_perfect},
    error::{ConfigKey, Error, Result, ValueKind},
    explain::explain,
    missing::{brought_back, held_artist, missing, poll, read_the_rest_of},
    online::Asking,
    organising::{Organising, organise},
    playing::{play, players, queue_onto_a_running_player},
    playlist::{import, list_playlists, playlist},
    roots::{forget, roots},
    scanning::{enrich, scan},
    served::Served,
    sinks::list_sinks,
    tagging::{dedupe, tag},
};

const LOG_FILTER: &str = "RESONATE_LOG";

const DEFAULT_LOG: &str = "warn,resonate=info,symphonia=off";

fn main() -> ExitCode {
    gaming::watch();
    let ran = run();
    providers::stop_the_tools();
    match ran {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            report(&error);
            ExitCode::FAILURE
        }
    }
}

fn report(error: &(dyn std::error::Error + 'static)) {
    told!("resonate: {error}");
    let mut source = error.source();
    while let Some(cause) = source {
        told!("  caused by: {cause}");
        source = cause.source();
    }
}

#[cfg(windows)]
fn name_the_process() {
    if let Err(error) = resonate_win32::name_the_process(resonate_mpris::APP_USER_MODEL_ID) {
        tracing::warn!(%error, "the process could not take the identity its toasts are raised under");
    }
}

#[cfg(all(windows, feature = "ui"))]
fn leave_a_console_of_its_own() {
    if let Err(error) = resonate_win32::leave_a_console_of_its_own() {
        told!("resonate: the console the window was started with stays open: {error}");
    }
}

fn init_logging() -> Result<()> {
    let (filter, refused) = wanted_filter();
    let written = fmt::layer().with_writer(io::stderr);
    #[cfg(windows)]
    let written = written.with_ansi(readout::understands_escapes());
    tracing_subscriber::registry()
        .with(filter)
        .with(written)
        .try_init()
        .map_err(|_| Error::LoggingAlreadyInstalled)?;

    if let Some(refused) = refused {
        tracing::warn!(
            %refused,
            filter = DEFAULT_LOG,
            "RESONATE_LOG is not a filter this reads; the default one is in force"
        );
    }
    Ok(())
}

fn wanted_filter() -> (EnvFilter, Option<ParseError>) {
    let Ok(wanted) = env::var(LOG_FILTER) else {
        return (EnvFilter::new(DEFAULT_LOG), None);
    };

    match EnvFilter::try_new(wanted) {
        Ok(filter) => (filter, None),
        Err(refused) => (EnvFilter::new(DEFAULT_LOG), Some(refused)),
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    one_way_on_bit_perfect(&cli)?;
    #[cfg(all(windows, feature = "ui"))]
    if cli.command.is_none() {
        leave_a_console_of_its_own();
    }
    init_logging()?;
    #[cfg(windows)]
    name_the_process();
    let config = config::load(cli.config.as_deref())?;
    resonate_words::speak(config.language_spoken());

    match &cli.command {
        Some(Sub::Sinks {
            renderers,
            renderers_at,
            cast_at,
        }) => list_sinks(&cli, &config, *renderers, renderers_at, cast_at),
        Some(Sub::Explain { path }) => explain(&cli, &config, &local_path(path)),
        Some(Sub::Info { path, graph }) => info::print(
            &local_path(path),
            &sources_over(vault_already_kept(&cli, &config).as_ref()),
            &engine_config(&cli, &config),
            *graph,
        ),
        Some(Sub::Listen {
            microphone,
            seconds,
            microphones,
        }) => listen::run(&config, microphone.as_deref(), *seconds, *microphones),
        Some(Sub::Analyse {
            file,
            track,
            recognise,
        }) => {
            let sources = sources_over(vault_already_kept(&cli, &config).as_ref());
            analyse::print(
                &analyse::Analysed::named(file, *track, &sources)?,
                &sources,
                (*recognise).then(|| {
                    online::fingerprinters(
                        &config,
                        sources_over(vault_already_kept(&cli, &config).as_ref()),
                        &online::by_sound(&config),
                        Asking::ForTheListener,
                    )
                }),
            )
        }
        Some(Sub::Studies {
            take: Some(file), ..
        }) => {
            let (location, span) = cut_of_argument(file, &sources_over(None))?;
            studies::take(&open_library(&cli, &config)?, &location, span)
        }
        Some(Sub::Studies {
            fakes,
            suspects,
            misnamed,
            take: None,
        }) => studies::print(
            &open_library(&cli, &config)?,
            StudyFilter {
                fakes: *fakes,
                suspects: *suspects,
                misnamed: *misnamed,
            },
        ),
        Some(Sub::Scan(wanted)) => scan(&open_library(&cli, &config)?, &config, wanted),
        Some(Sub::Roots) => roots(&open_library(&cli, &config)?),
        Some(Sub::Enrich { refresh, albums }) => enrich(
            &open_library(&cli, &config)?,
            &config,
            EnrichOptions {
                refresh: *refresh,
                at_most: *albums,
                studies: config.studies(),
                lyrics: config.fetches_lyrics(),
                ..EnrichOptions::default()
            },
        ),
        Some(Sub::Wants(args)) => wanting::run(&open_library(&cli, &config)?, args),
        Some(Sub::Sync(args)) => synced(&cli, &config, args),
        Some(Sub::Shared(args)) => shared(&cli, &config, args),
        Some(Sub::Party(wanted)) => party_asked(&cli, &config, wanted),
        Some(Sub::Missing {
            artist,
            read_the_rest,
            bring_back,
            dismiss,
            dismiss_release,
            album,
        }) => {
            let library = open_library(&cli, &config)?;
            if let Some(title) = dismiss {
                let narrowing = wanting::Narrowing {
                    artist: artist.as_deref(),
                    album: album.as_deref(),
                };
                return wanting::dismiss(&library, title, narrowing);
            }
            if let Some(title) = dismiss_release {
                return wanting::dismiss_release(&library, title, artist.as_deref());
            }
            if *bring_back {
                said!("{}", brought_back(library.bring_back_dismissed()?));
            }
            let held = match artist.as_deref() {
                Some(named) => Some((named, held_artist(&library, named)?)),
                None => None,
            };
            if *read_the_rest && let Some((named, id)) = held {
                read_the_rest_of(&library, &config, named, id)?;
            }
            missing(&library, artist.as_deref())
        }
        Some(Sub::Poll { again }) => {
            let held = vault_already_kept(&cli, &config);
            let (registered, keeping) =
                providers::registered_keeping(&config, settings_path(&cli).ok());
            poll(
                &open_library_with(&cli, &config, held.as_ref())?,
                registered,
                &keeping,
                *again,
            )
        }
        Some(Sub::Tidal) => {
            let settings = settings_path(&cli)?;
            reloading::delivering(&settings, Some(Reloadable::Tidal), || {
                providers::sign_in_to_tidal(&config, &settings)
            })
        }
        #[cfg(feature = "online")]
        Some(Sub::Jellyfin { forget }) => {
            jellyfin::sign_in(&config, &settings_path(&cli)?, *forget)
        }
        #[cfg(not(feature = "online"))]
        Some(Sub::Jellyfin { .. }) => Err(Error::NoSignIn),
        Some(Sub::Spotify(args)) => {
            let settings = settings_path(&cli)?;
            reloading::delivering(&settings, Some(Reloadable::Spotify), || {
                match (args.follow, args.move_once, args.forget) {
                    (true, ..) => {
                        spotify::follow(&open_library(&cli, &config)?, &config, Upkeep::InStep)
                    }
                    (_, true, _) => {
                        spotify::follow(&open_library(&cli, &config)?, &config, Upkeep::Once)
                    }
                    (.., true) => spotify::sign_out(&open_library(&cli, &config)?, &settings),
                    _ => spotify::sign_in(&config, &settings),
                }
            })
        }
        #[cfg(feature = "online")]
        Some(Sub::Ytmusic(wanted)) => {
            ytmusic::run(&config, settings_path(&cli).ok(), wanted, || {
                open_library(&cli, &config)
            })
        }
        #[cfg(not(feature = "online"))]
        Some(Sub::Ytmusic(_)) => Err(Error::NoServed),
        #[cfg(feature = "online")]
        Some(Sub::Musicbrainz {
            build,
            from,
            remove,
            search,
        }) => musicbrainz::run(&config, *build, from.as_deref(), *remove, search.as_deref()),
        #[cfg(not(feature = "online"))]
        Some(Sub::Musicbrainz { .. }) => Err(Error::NoMirror),
        Some(Sub::Lastfm { forget }) => {
            let settings = settings_path(&cli)?;
            reloading::delivering(&settings, Some(Reloadable::Submission), || {
                lastfm::sign_in(&config, &settings, *forget)
            })
        }
        Some(Sub::Forget { roots }) => forget(&open_library(&cli, &config)?, roots),
        Some(Sub::Tag { root, apply, undo }) => {
            tag(&open_library(&cli, &config)?, root, *apply, *undo)
        }
        Some(Sub::Dedupe { apply }) => dedupe(&open_library(&cli, &config)?, *apply),
        Some(Sub::Vault(args)) => {
            let held = vault_asked_for(&cli, &config)?;
            vault::run(&open_library_with(&cli, &config, Some(&held))?, &held, args)
        }
        Some(Sub::Organise {
            layout,
            root,
            apply,
            undo,
        }) => organise(
            &open_library(&cli, &config)?,
            &config,
            layout.as_deref(),
            root,
            Organising {
                apply: *apply,
                walk_back: *undo,
            },
        ),
        Some(Sub::Play {
            files,
            sleep,
            transport,
        }) => play(&cli, &config, files, sleep.as_deref(), transport),
        Some(Sub::Queue(wanted)) => queue_onto_a_running_player(&cli, &config, wanted),
        Some(Sub::Players) => players(),
        Some(Sub::Served(wanted)) => served::browse(
            &config,
            settings_path(&cli).ok(),
            wanted.service.as_deref(),
            &wanted.browsing,
        ),
        Some(Sub::Subsonic(wanted)) => {
            served::browse_subsonic(&config, settings_path(&cli).ok(), wanted)
        }
        Some(Sub::Playlists { refresh: true, .. }) => {
            followed::refresh_every_kept(&open_library(&cli, &config)?, &config)
        }
        Some(Sub::Playlists {
            order,
            reverse,
            named,
            refresh: false,
        }) => list_playlists(
            &open_library(&cli, &config)?,
            *order,
            *reverse,
            named.as_deref(),
        ),
        Some(Sub::Playlist(wanted)) => playlist(&cli, &config, wanted),
        Some(Sub::Eq(wanted)) => equaliser::run(&cli, &config, wanted),
        Some(Sub::Abx {
            file,
            trials,
            against,
        }) => abx::run(&cli, &config, file, *trials, *against),
        Some(Sub::Import {
            files,
            upkeep,
            name,
        }) => import(
            &open_library(&cli, &config)?,
            &config,
            files,
            name.as_deref(),
            (*upkeep).into(),
        ),
        Some(Sub::Sleep { spec, player, zone }) => {
            sleep::set(spec, player.as_deref(), zone.as_deref())
        }
        Some(Sub::Stats { window, top }) => {
            stats::print(&open_library(&cli, &config)?, *window, top.get())
        }
        #[cfg(feature = "online")]
        Some(Sub::Update) => updates::run(&config),
        #[cfg(not(feature = "online"))]
        Some(Sub::Update) => Err(Error::NoUpdates),
        Some(Sub::Wrapped { year, summary }) => wrapped::print(
            &open_library(&cli, &config)?,
            *year,
            *summary,
            config.history_kept(),
        ),
        Some(Sub::Favourites(wanted)) => favourites::run(&open_library(&cli, &config)?, wanted),
        Some(Sub::Suggest { save }) => {
            suggest::print(&open_library(&cli, &config)?, save.as_deref())
        }
        Some(Sub::Share { file }) => share::print(
            &open_library(&cli, &config)?,
            online::reference(&config, Asking::ForTheListener).as_deref(),
            file.as_deref().map(local_path).as_deref(),
        ),
        Some(Sub::Mcp { player }) => mcp::serve(&cli, &config, player.as_deref()),
        None => {
            let library = Arc::new(open_library(&cli, &config)?);
            launch(cli, config, library)
        }
    }
}

fn library_path(cli: &Cli, config: &Config) -> Result<PathBuf> {
    match cli.library.clone().or_else(|| config.library.clone()) {
        Some(path) => Ok(path),
        None => config::library_path(),
    }
}

fn settings_path(cli: &Cli) -> Result<PathBuf> {
    cli.config.clone().map_or_else(config::config_path, Ok)
}

fn vault_path(cli: &Cli, config: &Config) -> Result<PathBuf> {
    match cli.vault.clone().or_else(|| config.vault.clone()) {
        Some(path) => Ok(path),
        None => config::vault_dir(),
    }
}

fn vault_asked_for(cli: &Cli, config: &Config) -> Result<Arc<Vault>> {
    let path = vault_path(cli, config)?;
    Ok(Arc::new(if made_where_asked(cli, config) {
        Vault::make(path)?
    } else {
        Vault::open(path)?
    }))
}

const fn made_where_asked(cli: &Cli, config: &Config) -> bool {
    cli.vault.is_some() || config.vault.is_none()
}

fn vault_already_kept(cli: &Cli, config: &Config) -> Option<Arc<Vault>> {
    let path = vault_path(cli, config).ok()?;
    if !path.is_dir() && cli.vault.is_none() {
        if config.vault.is_some() {
            tracing::warn!(
                path = %path.display(),
                "the vault the settings name is not there, an unmounted drive perhaps; \
                 what it holds will not play until it is"
            );
        }
        return None;
    }
    let opened = if cli.vault.is_some() {
        Vault::make(path)
    } else {
        Vault::open(path)
    };
    match opened {
        Ok(vault) => Some(Arc::new(vault)),
        Err(error) => {
            tracing::warn!(%error, "the vault could not be opened; nothing will be kept in one");
            None
        }
    }
}

fn sources_over(vault: Option<&Arc<Vault>>) -> Arc<Sources> {
    Arc::new(held_over(vault))
}

fn held_over(vault: Option<&Arc<Vault>>) -> Sources {
    let mut sources = Sources::local();
    if let Some(vault) = vault {
        sources = sources.and(Arc::new(VaultFiles::over(vault)));
    }
    sources
}

fn playing_from(library: Option<&Library>, served: &Served) -> Arc<Sources> {
    let Some(library) = library else {
        return Arc::new(served.sources(Sources::local()));
    };
    Arc::new(served.sources(library.sources().hinted_by(library.hinting())))
}

fn open_library(cli: &Cli, config: &Config) -> Result<Library> {
    open_library_with(cli, config, vault_already_kept(cli, config).as_ref())
}

fn open_library_with(cli: &Cli, config: &Config, vault: Option<&Arc<Vault>>) -> Result<Library> {
    let path = library_path(cli, config)?;
    let library = match vault {
        Some(vault) => Library::open_with_vault(&path, Arc::clone(vault))?,
        None => Library::open(&path)?,
    };
    library.filter_music(config.music_filters());
    stop_telling_what_has_no_account(&library, config);
    age_the_history(&library, config.history_kept());
    library.deliver_into(delivery_folder(config));
    library.stall_downloads_by(metered::for_downloads(config));
    library.trash_through(trashing::the_desktops());
    Ok(library)
}

fn delivery_folder(config: &Config) -> Option<DeliveryFolder> {
    Some(DeliveryFolder {
        path: config.music_folder.clone()?,
        layout: config.organise_as(),
    })
}

fn stop_telling_what_has_no_account(library: &Library, config: &Config) {
    let unaccounted = [
        (
            ListeningService::ListenBrainz,
            config.listenbrainz_token.is_none(),
        ),
        (ListeningService::LastFm, config.lastfm_session.is_none()),
    ];
    for (service, unaccounted) in unaccounted {
        if !unaccounted {
            continue;
        }
        match library.stop_telling(service) {
            Ok(true) => tracing::info!(
                service = service.name(),
                "a service no longer signed in to holds the history back no longer"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(%error, "a service's mark could not be let go of"),
        }
    }
}

fn age_the_history(library: &Library, kept: HistoryKept) {
    match library.age_the_history(kept) {
        Ok(aged) if aged != Aged::default() => tracing::info!(
            listens = aged.listens,
            passes = aged.passes,
            "the history older than it is kept for was forgotten"
        ),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "the history could not be aged"),
    }
}

fn catalog(cli: &Cli, config: &Config) -> Option<Arc<Library>> {
    match open_library(cli, config) {
        Ok(library) => Some(Arc::new(library)),
        Err(error) => {
            tracing::warn!(%error, "playlists are unavailable without a library");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_length_to_listen_for_is_held_to_what_the_setting_takes() {
        let listened = |seconds: &str| {
            Cli::try_parse_from(["resonate", "listen", "--seconds", seconds]).map(|cli| {
                match cli.command {
                    Some(Sub::Listen { seconds, .. }) => seconds,
                    _ => None,
                }
            })
        };

        assert_eq!(listened("12").ok().flatten(), Some(12));
        assert_eq!(listened("4").ok().flatten(), Some(4));
        assert_eq!(listened("60").ok().flatten(), Some(60));
        assert!(listened("3").is_err());
        assert!(listened("61").is_err());
        assert!(listened("18446744073709551615").is_err());
    }
}
