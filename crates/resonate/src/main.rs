#![deny(clippy::print_stdout, clippy::print_stderr)]

#[macro_use]
mod said;

mod abx;
mod analyse;
#[cfg(any(windows, target_os = "macos"))]
mod awake;
mod catalog;
mod cli;
mod config;
mod discord;
mod equaliser;
mod error;
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
mod mpris;
#[cfg(feature = "online")]
mod musicbrainz;
mod online;
#[cfg(feature = "sync")]
mod party;
#[cfg(all(feature = "sync", feature = "ui"))]
mod party_room;
mod playlists;
mod providers;
mod readout;
mod reloading;
mod rendering;
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
#[cfg(feature = "sync")]
mod together;
mod trashing;
#[cfg(feature = "online")]
mod updates;
mod vault;
mod vocabulary;
mod wanting;
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
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
    fs,
    io::{self, IsTerminal as _},
    mem,
    num::NonZeroUsize,
    path::{self, Path, PathBuf},
    process::ExitCode,
    sync::{Arc, atomic::AtomicBool},
    thread,
    time::{Duration, SystemTime},
};

use clap::Parser;
use crossbeam_channel::{bounded, never, select, tick};
use resonate_cast::Given;
use resonate_codec::{
    CoverArt, Packing, Popularity, Sources, probe, read_cue_media, the_file_a_cue_names,
};
#[cfg(feature = "ui")]
use resonate_core::Resumption;
use resonate_core::{
    AlbumId, ArtistId, FrameSpan, Frames, MediaLocation, PlaylistId, SampleRate, Speakers,
    StreamSpec, Volume, ZoneId, names_audio,
};
use resonate_engine::{
    BluetoothWake, Command, Counting, Decoded, EngineConfig, Event, HardwareVolume, Impulse, Keep,
    Keeping, Levelling, Listening, NodeName, OutputPlan, Placement, Player, PlayerState, Plugged,
    QueueItem, Queued, RepeatMode, SinkError, SinkFormats, SinkInfo, Unclaimed, Until, Words,
    plan_output, read_impulse, resolve_replay_gain, stamp_of,
};
#[cfg(feature = "ui")]
use resonate_engine::{Opens, Zones};
use resonate_library::{
    Aged, Cancelling, Cut, DeliveryFolder, Direction, EnrichOptions, EnrichStats, EnrichSummary,
    Failure, Failures, FileTags, HistoryKept, Kept, Layout, Library, Listen, ListeningService,
    LookupOp, Missing, MissingTrack, Move, OrganiseOptions, OrganiseSummary, Outcome, PassHandle,
    PassKind, Playing, Playlist, PlaylistFormat, PlaylistName, PlaylistOrder, PollOptions, Refusal,
    Refused, RetagOptions, RetagSummary, RowOrder, SavedQuery, ScanOptions, Search, SheetScope,
    SortOrder, StudyFilter, TRIES_BEFORE_GIVING_UP, UnheldRelease, Upkeep, Vault, VaultFiles, Want,
    cuts_listed_in, folded_letters,
};
use resonate_mpris::{PlayerName, Queueing, Reloadable, Running, Standing, ZoneAsked};
use resonate_providers::Providers;
use resonate_upnp::Located;
use tracing_subscriber::{
    EnvFilter, filter::ParseError, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

use crate::{
    cli::{
        Cli, PlaylistArgs, PlaylistOrderArg, QueueArgs, ScanArgs, SharedArgs, Sub, SyncArgs,
        TransportArgs,
    },
    config::Config,
    error::{ArtistName, ConfigKey, Error, Result, ValueKind},
    info::bytes_text,
    input::{Action, Pressed},
    online::Asking,
    readout::Readout,
    rendering::{Asked, Rendering},
    served::Served,
    table::Table,
    words::columns,
};

const SINK_TIMEOUT: Duration = Duration::from_secs(2);

const HEARD_SAMPLE: Duration = Duration::from_millis(500);

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

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

fn wanted_sink(cli: &Cli, config: &Config) -> Option<NodeName> {
    cli.sink
        .as_deref()
        .map(NodeName::new)
        .or_else(|| config.sink.clone())
}

fn rates(rates: &[SampleRate]) -> String {
    if rates.is_empty() {
        return "none".to_owned();
    }
    rates
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" / ")
}

fn select_sink(sinks: Vec<SinkInfo>, wanted: Option<&NodeName>) -> Result<SinkInfo> {
    let Some(wanted) = wanted else {
        return sinks
            .into_iter()
            .find(|sink| sink.is_default)
            .ok_or(Error::Sink(SinkError::NoSink));
    };
    sinks
        .into_iter()
        .find(|sink| sink.is_named(wanted))
        .ok_or_else(|| Error::SinkNotFound(wanted.clone()))
}

fn confirm_sink(player: &Player, wanted: Option<&NodeName>) {
    let Some(wanted) = wanted else {
        return;
    };
    if player
        .sinks_once_surveyed(SINK_TIMEOUT)
        .iter()
        .any(|sink| sink.is_named(wanted))
    {
        return;
    }
    tracing::warn!(
        %wanted,
        "the chosen sink is not in the graph; the default device plays until it appears"
    );
}

fn broken_down(failed: Failures) -> String {
    let named: Vec<String> = Failure::ALL
        .into_iter()
        .filter(|failure| failed.counted(*failure) > 0)
        .map(|failure| format!("{} {}", failed.counted(failure), failure.as_str()))
        .collect();

    if named.is_empty() {
        String::new()
    } else {
        format!(" ({})", named.join(", "))
    }
}

fn with_receiver_addresses(
    config: &Config,
    renderers_at: &[String],
    cast_at: &[String],
) -> Result<Config> {
    let mut config = config.clone();
    for address in renderers_at {
        let located = Located::parse(address).map_err(|source| Error::RendererAddress {
            address: address.clone(),
            source: Box::new(source),
        })?;
        config.renderers_at.insert(located);
    }
    for address in cast_at {
        let receiver = Given::parse(address).ok_or_else(|| Error::CastAddress {
            address: address.clone(),
        })?;
        config.cast_at.insert(receiver);
    }
    Ok(config)
}

fn list_sinks(
    cli: &Cli,
    config: &Config,
    renderers: bool,
    renderers_at: &[String],
    cast_at: &[String],
) -> Result<()> {
    let mut sinks = surveyed::sinks()?;
    let config = with_receiver_addresses(config, renderers_at, cast_at)?;
    let asked = if renderers {
        Asked::Network
    } else if renderers_at.is_empty() && cast_at.is_empty() {
        Asked::Nothing
    } else {
        Asked::Addresses
    };
    sinks.extend(Rendering::of(&config, wanted_sink(cli, &config).as_ref(), asked).sinks());

    let mut table = Table::new(vec![
        "",
        columns::device(),
        columns::node_name(),
        columns::driven_by(),
        columns::profile(),
        columns::port(),
        columns::format(),
        columns::advertises(),
        columns::graph(),
        columns::now(),
    ]);

    for sink in &sinks {
        let marker = if sink.is_default { "*" } else { "" };
        let current = sink.current_rate.map_or_else(
            || words::main::unknown().to_owned(),
            |rate: SampleRate| rate.to_string(),
        );

        let mut formats = sink.formats.iter();
        let first = formats.next();
        table.push(vec![
            marker.to_owned(),
            sink.description.clone(),
            sink.name.to_string(),
            driven_by(sink).to_owned(),
            sink.profile.as_ref().map_or_else(
                || words::main::none().to_owned(),
                |profile| profile.description.clone(),
            ),
            comes_out_of(sink),
            first.map_or_else(|| words::main::none().to_owned(), SinkFormats::spelled),
            first.map_or_else(
                || words::main::none().to_owned(),
                |entry| rates(&entry.rates),
            ),
            rates(&sink.allowed_rates),
            current,
        ]);

        for entry in formats {
            table.push(vec![
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                entry.spelled(),
                rates(&entry.rates),
            ]);
        }
    }

    said_on!("{}", table.render());
    Ok(())
}

fn driven_by(sink: &SinkInfo) -> &'static str {
    if rendering::names_a_renderer(&sink.name) {
        words::main::the_network()
    } else if sink.is_hardware {
        words::main::a_device()
    } else {
        words::main::the_graph()
    }
}

fn comes_out_of(sink: &SinkInfo) -> String {
    let Some(port) = sink.port.as_ref() else {
        return words::main::none().to_owned();
    };
    let plugged = match port.plugged {
        Plugged::Unsaid => port.description.clone(),
        Plugged::No => words::main::nothing_plugged_in(&port.description),
        Plugged::Yes => words::main::plugged_in(&port.description),
    };
    let volume = match port.hardware_volume {
        HardwareVolume::Unsaid => plugged,
        HardwareVolume::No => words::main::volume_in_software(plugged),
        HardwareVolume::Yes => words::main::volume_on_the_hardware(plugged),
        HardwareVolume::Always => words::main::volume_always_on_the_device(plugged),
    };
    let level = match port.volume {
        Some(gain) => words::main::heard_at(Volume::heard_at(gain).to_string(), volume),
        None => volume,
    };
    if port.muted {
        words::main::muted(level)
    } else {
        level
    }
}

fn scan(library: &Library, config: &Config, wanted: &ScanArgs) -> Result<()> {
    let roots = &wanted.roots;
    if let Some(root) = roots.iter().find(|root| !root.is_dir()) {
        return Err(Error::MissingLibraryRoot { path: root.clone() });
    }

    let workers = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let handle = library.scan(scan_options(wanted, workers))?;

    let summary = until_told(handle)?;
    let stats = summary.stats;
    said!(
        "{}{}",
        words::main::scanned(
            stats.added,
            stats.discovered,
            stats.failed.total(),
            stats.moved,
            stats.removed,
            stats.updated,
        ),
        broken_down(stats.failed)
    );
    if summary.cancelled {
        said!("{}", words::main::cancelled());
    }
    finished(PassKind::Scan, summary.cancelled)?;
    let scanned = none_failed(PassKind::Scan, stats.failed.total());

    if config.enriches_after_scan()
        && let Some(reference) = online::reference(config, Asking::InTheBackground)
    {
        let summary = until_told(library.enrich(
            reference,
            Arc::new(online::fingerprinters(
                config,
                Arc::new(library.sources()),
                &online::by_sound(config),
                Asking::InTheBackground,
            )),
            carrying_on(
                library,
                EnrichOptions {
                    studies: config.studies(),
                    lyrics: config.fetches_lyrics(),
                    ..EnrichOptions::default()
                },
            )?,
        )?)?;
        said_on!("{}", enriched(&summary));
        finished(PassKind::Enrich, summary.cancelled)?;
    }
    scanned
}

fn scan_options(wanted: &ScanArgs, workers: NonZeroUsize) -> ScanOptions {
    ScanOptions {
        roots: wanted.roots.clone(),
        incremental: !wanted.full,
        follow_symlinks: wanted.follow_links,
        extract_cover_art: true,
        workers,
    }
}

fn enrich(library: &Library, config: &Config, options: EnrichOptions) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::InTheBackground)?;
    let options = carrying_on(library, options)?;
    let summary = until_told(library.enrich(
        reference,
        Arc::new(online::fingerprinters(
            config,
            Arc::new(library.sources()),
            &online::by_sound(config),
            Asking::InTheBackground,
        )),
        options,
    )?)?;
    said_on!("{}", enriched(&summary));
    finished(PassKind::Enrich, summary.cancelled)
}

fn carrying_on(library: &Library, options: EnrichOptions) -> Result<EnrichOptions> {
    if options.refresh {
        return Ok(options);
    }
    let Some(left) = library.unfinished_enrichment()? else {
        return Ok(options);
    };

    tracing::info!(
        began = ?left.began,
        refresh = left.refresh,
        "carrying on a lookup the last run left unfinished"
    );
    said!("{}", words::main::carrying_on());

    Ok(EnrichOptions {
        refresh: left.refresh,
        ..options
    })
}

fn enriched(summary: &EnrichSummary) -> String {
    let mut told = counted_by_section(&summary.stats);
    if let Some(op) = summary.stopped_by {
        told.push_str(&words::main::stopped_unreached(asked_for(op)));
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

fn counted_by_section(stats: &EnrichStats) -> String {
    if stats.is_blank() {
        return format!("{}\n", words::main::nothing_asked());
    }
    let widest = Outcome::ALL
        .into_iter()
        .map(|outcome| stats.of(outcome).to_string().len())
        .max()
        .unwrap_or(1);

    let mut told = String::new();
    for (section, counted) in stats.told_by_section() {
        told.push_str(section.title());
        told.push('\n');
        for (outcome, count) in counted {
            told.push_str(&format!("  {count:>widest$}  {}\n", outcome.label()));
        }
    }
    told
}

fn asked_for(op: LookupOp) -> &'static str {
    match op {
        LookupOp::Release => words::main::asked_release(),
        LookupOp::FindRelease => words::main::asked_find_release(),
        LookupOp::Recording => words::main::asked_recording(),
        LookupOp::Isrc => words::main::asked_isrc(),
        LookupOp::FindRecording => words::main::asked_find_recording(),
        LookupOp::ReleaseGroup => words::main::asked_release_group(),
        LookupOp::FindReleaseGroup => words::main::asked_find_release_group(),
        LookupOp::Artist => words::main::asked_artist(),
        LookupOp::FindArtist => words::main::asked_find_artist(),
        LookupOp::ReleaseGroupsOfArtist => words::main::asked_release_groups_of_artist(),
        LookupOp::ReleasesOfGroup => words::main::asked_releases_of_group(),
        LookupOp::ReleasesOfArtist => words::main::asked_releases_of_artist(),
        LookupOp::Cover => words::main::asked_cover(),
        LookupOp::Portrait => words::main::asked_portrait(),
        LookupOp::Lyrics => words::main::asked_lyrics(),
        LookupOp::Devices => words::main::asked_devices(),
        LookupOp::Correction => words::main::asked_correction(),
        LookupOp::Recognise => words::main::asked_recognise(),
        LookupOp::Submit => words::main::asked_submit(),
        LookupOp::Love => words::main::asked_love(),
        LookupOp::Token => words::main::asked_token(),
        LookupOp::StreamLink => words::main::asked_stream_link(),
        LookupOp::FollowLink => words::main::asked_follow_link(),
        LookupOp::ShareFile => words::main::asked_share_file(),
        LookupOp::LatestRelease => words::main::asked_latest_release(),
        LookupOp::SpotifyAccount => words::main::asked_spotify_account(),
    }
}

fn wants(library: &Library) -> Result<()> {
    let wanted = library.wants()?;
    if wanted.is_empty() {
        said!("{}", words::main::nothing_wanted());
        return Ok(());
    }

    let mut table = Table::new(vec![
        columns::title(),
        columns::artist(),
        columns::album(),
        columns::state(),
        columns::wanted(),
        columns::tried(),
        columns::offered(),
        columns::links(),
    ]);
    let now = SystemTime::now();
    for want in &wanted {
        table.push(vec![
            want.title.clone(),
            want.artist.clone().unwrap_or_default(),
            want.album_title.clone(),
            standing_of(want, now),
            ago(Some(want.wanted)),
            ago(want.tried),
            want.offered.clone().unwrap_or_else(|| "-".to_owned()),
            linked_through(want),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

fn standing_of(want: &Want, now: SystemTime) -> String {
    if want.held.is_some() {
        return words::main::held().to_owned();
    }
    if want.gave_up() {
        return words::main::gave_up(TRIES_BEFORE_GIVING_UP);
    }
    let due = want
        .due_at()
        .and_then(|at| at.duration_since(now).ok())
        .filter(|left| !left.is_zero());
    let waiting = |said: String| match due {
        Some(left) => words::main::again_in(lasting_for(left), said),
        None => words::main::due_again(said),
    };

    match (want.offered.is_some(), want.misses) {
        (true, _) => waiting(words::main::offered().to_owned()),
        (false, 0) if want.tried.is_none() => words::main::due().to_owned(),
        (false, 0) => waiting(words::main::unanswered().to_owned()),
        (false, misses) => waiting(words::main::no_match(misses, TRIES_BEFORE_GIVING_UP)),
    }
}

fn lasting_for(left: Duration) -> String {
    let minutes = left.as_secs().div_ceil(60);
    match (minutes / 60, minutes % 60) {
        (0, minutes) => words::main::lasting_minutes(minutes),
        (hours, 0) => words::main::lasting_hours(hours),
        (hours, minutes) => words::main::lasting_hours_and_minutes(hours, minutes),
    }
}

fn linked_through(want: &Want) -> String {
    let mut named: Vec<&str> = Vec::with_capacity(want.links.len());
    for link in &want.links {
        let name = link.service.name();
        if !named.contains(&name) {
            named.push(name);
        }
    }
    named.join(", ")
}

fn held_artist(library: &Library, named: &str) -> Result<ArtistId> {
    library
        .artist_named(named)?
        .ok_or_else(|| Error::NoSuchArtist(ArtistName::new(named)))
}

fn read_the_rest_of(
    library: &Library,
    config: &Config,
    named: &str,
    artist: ArtistId,
) -> Result<()> {
    let reference = online::reference_asked_for(config, Asking::ForTheListener)?;
    let read = library.read_the_rest_of(artist, reference.as_ref())?;
    said!("{}", words::main::read_the_rest(named, read));
    Ok(())
}

fn dismissed_beside(dismissed: Missing) -> String {
    match dismissed.tracks + dismissed.releases {
        0 => String::new(),
        many => format!(" · {}", words::main::dismissed_beside(many)),
    }
}

fn brought_back(brought: Missing) -> String {
    words::main::brought_back(brought.releases, brought.tracks)
}

fn missing(library: &Library, artist: Option<&str>) -> Result<()> {
    let mut tracks = library.missing_tracks(None, None)?;
    let mut releases = library.unheld_releases(None, None)?;
    let (tracks_missing, releases_unheld) = match artist {
        None => {
            let counted = library.missing_counted(None)?;
            (counted.tracks, counted.releases)
        }
        Some(named) => {
            let wanted = folded_name(named);
            tracks.retain(|track| {
                track
                    .owner
                    .as_deref()
                    .is_some_and(|owner| folded_name(owner) == wanted)
            });
            releases.retain(|release| folded_name(&release.artist_name) == wanted);
            (tracks.len() as u64, releases.len() as u64)
        }
    };

    if tracks.is_empty() && releases.is_empty() {
        match artist {
            None => said!("{}", words::main::nothing_missing()),
            Some(named) => said!("{}", words::main::nothing_of_missing(named)),
        }
        return Ok(());
    }

    let discs_of = discs_per_album(&tracks);
    let dismissed = match artist {
        None => library.dismissed()?,
        Some(_) => Missing::default(),
    };
    said!(
        "{}{}",
        words::main::missing_across(discs_of.len(), releases_unheld, tracks_missing),
        dismissed_beside(dismissed)
    );

    said!();
    said!("{}", words::main::tracks_missing());
    if tracks.is_empty() {
        said!("{}", words::main::every_track_held());
    } else {
        said_on!("{}", missing_tracks_table(&tracks, &discs_of).render());
    }

    said!();
    said!("{}", words::main::releases_not_held());
    if releases.is_empty() {
        said!("{}", words::main::every_release_held());
    } else {
        said_on!("{}", unheld_releases_table(&releases).render());
    }
    if let Some(named) = artist
        && let Some(id) = library.artist_named(named)?
        && let Some(detail) = library.artist_detail(id)?
        && detail.releases_unread > 0
    {
        said!(
            "{}",
            words::main::credits_unread(named, detail.releases_unread)
        );
    }
    Ok(())
}

fn folded_name(name: &str) -> String {
    folded_letters(name.trim())
}

fn discs_per_album(tracks: &[MissingTrack]) -> BTreeMap<AlbumId, BTreeSet<u32>> {
    let mut discs: BTreeMap<AlbumId, BTreeSet<u32>> = BTreeMap::new();
    for track in tracks {
        discs.entry(track.album).or_default().insert(track.disc);
    }
    discs
}

fn missing_tracks_table(
    tracks: &[MissingTrack],
    discs_of: &BTreeMap<AlbumId, BTreeSet<u32>>,
) -> Table {
    let mut table = Table::new(vec![
        columns::album(),
        columns::artist(),
        columns::disc(),
        "#",
        columns::title(),
        columns::length(),
        columns::wanted(),
    ]);
    for track in tracks {
        let on_several_discs = discs_of
            .get(&track.album)
            .is_some_and(|discs| discs.len() > 1);
        table.push(vec![
            track.album_title.clone(),
            track
                .artist
                .clone()
                .or_else(|| track.owner.clone())
                .unwrap_or_else(|| "-".to_owned()),
            if on_several_discs {
                track.disc.to_string()
            } else {
                String::new()
            },
            track
                .number
                .clone()
                .unwrap_or_else(|| track.position.to_string()),
            track.title.clone(),
            track
                .length
                .map_or_else(|| "-".to_owned(), |length| played(Some(length))),
            if track.want.is_some() {
                words::main::yes()
            } else {
                "-"
            }
            .to_owned(),
        ]);
    }
    table
}

fn unheld_releases_table(releases: &[UnheldRelease]) -> Table {
    let mut table = Table::new(vec![
        columns::artist(),
        columns::release(),
        columns::kind(),
        columns::first_released(),
    ]);
    for release in releases {
        table.push(vec![
            release.artist_name.clone(),
            release.title.clone(),
            release.kind.clone().unwrap_or_else(|| "-".to_owned()),
            release
                .first_released
                .clone()
                .unwrap_or_else(|| "-".to_owned()),
        ]);
    }
    table
}

fn poll(
    library: &Library,
    providers: Providers,
    keeping: &providers::Keeping,
    again: bool,
) -> Result<()> {
    if !providers.has_a_source() {
        said!("{}", words::main::no_provider());
    }
    let options = if again {
        PollOptions::ASKING_EVERY_WANT
    } else {
        PollOptions::default()
    };
    let summary = until_told(library.poll(Arc::new(providers), options)?)?;
    let stats = summary.stats;
    said!(
        "{}",
        words::main::polled(
            stats.asked,
            stats.kept,
            stats.late,
            stats.nothing,
            stats.offered,
            stats.refused,
            stats.unkept,
        )
    );
    if summary.cancelled {
        said!("{}", words::main::cancelled());
    }
    finished(PassKind::Poll, summary.cancelled)?;
    keeping.unkept().map_or(Ok(()), Err)
}

fn roots(library: &Library) -> Result<()> {
    for root in library.roots()? {
        said!("{}", root.display());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum Forgotten {
    Root,
    Delivered(PathBuf),
    Gone(u64),
    Nothing,
}

fn forget(library: &Library, named: &[PathBuf]) -> Result<()> {
    for argument in named {
        match forgotten(library, argument)? {
            Forgotten::Root => said!("{}", words::main::forgot(argument.display().to_string())),
            Forgotten::Delivered(delivered) => {
                said!(
                    "{}",
                    words::main::forgot_delivered(delivered.display().to_string())
                );
            }
            Forgotten::Gone(tracks) => {
                said!(
                    "{}",
                    words::main::forgot_gone(argument.display().to_string(), tracks)
                );
            }
            Forgotten::Nothing => said!(
                "{}",
                words::main::forgot_nothing(argument.display().to_string())
            ),
        }
    }
    Ok(())
}

fn forgotten(library: &Library, argument: &Path) -> Result<Forgotten> {
    if library.remove_root(argument)? {
        return Ok(Forgotten::Root);
    }
    let named = location_of_argument(argument.as_os_str(), &Sources::local());
    let path = named.as_path().unwrap_or(argument);
    if library.forget_delivered(path)? {
        return Ok(Forgotten::Delivered(path.to_path_buf()));
    }

    Ok(match library.retire(path)? {
        0 => Forgotten::Nothing,
        tracks => Forgotten::Gone(tracks),
    })
}

fn tag(library: &Library, roots: &[PathBuf], apply: bool, undo: bool) -> Result<()> {
    let summary = until_told(library.retag(
        Arc::new(FileTags::default()),
        RetagOptions {
            roots: filed_from(roots),
            apply,
            undo,
        },
    )?)?;

    said_on!("{}", tagged(&summary, &library.roots()?, apply));
    finished(PassKind::Retag, summary.cancelled)?;
    let failed = summary
        .retagging
        .passed_over
        .iter()
        .filter(|passed| passed.why.is_a_failure())
        .count();
    none_failed(PassKind::Retag, if apply { failed as u64 } else { 0 })
}

fn dedupe(library: &Library, apply: bool) -> Result<()> {
    let copies = library.numbered_copies()?;
    for copy in &copies.numbered {
        said!(
            "{}",
            words::main::copies(
                copy.copy.display().to_string(),
                copy.kept.display().to_string()
            )
        );
    }
    for differing in &copies.differing {
        said!("{}", words::main::differs(differing.display().to_string()));
    }
    if !apply {
        said!("{}", words::main::to_fold(copies.numbered.len()));
        return Ok(());
    }
    let deleted = library.fold_copies(&copies.numbered)?;
    said!("{}", words::main::folded(deleted.files));
    if deleted.kept > 0 {
        said!("{}", words::main::trash_refused(deleted.kept));
    }
    Ok(())
}

fn pictured(picture: &CoverArt) -> String {
    format!(
        "{} · {}",
        picture.format.extension().to_uppercase(),
        bytes_text(picture.bytes.len() as u64)
    )
}

fn rated(popularity: Popularity) -> String {
    let plays = words::main::plays(popularity.plays);
    if popularity.favourite {
        words::main::favourite_plays(plays)
    } else {
        words::main::not_favourite_plays(plays)
    }
}

fn tagged(summary: &RetagSummary, roots: &[PathBuf], apply: bool) -> String {
    let retagging = &summary.retagging;
    if retagging.writes.is_empty()
        && retagging.sheets.is_empty()
        && retagging.passed_over.is_empty()
    {
        return format!("{}\n", words::main::nothing_to_tag());
    }

    let mut told = String::new();
    if !retagging.writes.is_empty() || !retagging.sheets.is_empty() {
        let mut table = Table::new(vec![columns::file(), columns::field(), columns::value()]);
        for write in &retagging.writes {
            let mut named = under_its_root(&write.path, roots);
            for edit in &write.edits {
                table.push(vec![
                    mem::take(&mut named),
                    edit.field.to_string(),
                    edit.value.clone(),
                ]);
            }
            for field in &write.taken {
                table.push(vec![
                    mem::take(&mut named),
                    field.to_string(),
                    words::main::taken_away().to_owned(),
                ]);
            }
            if let Some(picture) = &write.picture {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::cover_art().to_owned(),
                    pictured(picture),
                ]);
            }
            if write.unpictured {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::cover_art().to_owned(),
                    words::main::taken_away().to_owned(),
                ]);
            }
            if let Some(popularity) = write.popularity {
                table.push(vec![
                    mem::take(&mut named),
                    words::main::rating().to_owned(),
                    rated(popularity),
                ]);
            }
        }
        for write in &retagging.sheets {
            let mut named = under_its_root(&write.sheet, roots);
            for edit in &write.edits {
                let field = match edit.scope {
                    SheetScope::Disc => edit.field.to_string(),
                    SheetScope::Track(number) => {
                        words::main::sheet_track(edit.field.to_string(), format!("{number:02}"))
                    }
                };
                table.push(vec![
                    mem::take(&mut named),
                    field,
                    edit.value
                        .clone()
                        .unwrap_or_else(|| words::main::taken_away().to_owned()),
                ]);
            }
        }
        told.push_str(&table.render());
    }

    for over in &retagging.passed_over {
        told.push_str(&format!(
            "{}: {}\n",
            under_its_root(&over.path, roots),
            over.why.as_str()
        ));
    }

    let stats = &summary.stats;
    let written = if apply {
        words::main::written(stats.fields, stats.pictures, stats.ratings, stats.written)
    } else {
        words::main::would_write(
            retagging
                .writes
                .iter()
                .map(|write| write.edits.len() + write.taken.len())
                .chain(retagging.sheets.iter().map(|write| write.edits.len()))
                .sum::<usize>(),
            retagging
                .writes
                .iter()
                .filter(|write| write.picture.is_some())
                .count(),
            retagging
                .writes
                .iter()
                .filter(|write| write.popularity.is_some())
                .count(),
            retagging.writes.len() + retagging.sheets.len(),
        )
    };

    told.push_str(&words::main::tagged(
        stats.passed_over,
        stats.unchanged,
        stats.walked,
        written,
    ));
    told.push('\n');
    if !apply {
        told.push_str(words::main::nothing_written());
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

#[derive(Clone, Copy)]
struct Organising {
    apply: bool,
    walk_back: bool,
}

fn organise(
    library: &Library,
    config: &Config,
    named: Option<&str>,
    roots: &[PathBuf],
    how: Organising,
) -> Result<()> {
    let layout = match named {
        Some(template) => Layout::read(template)?,
        None => config.organise_as(),
    };
    let summary = until_told(library.organise(OrganiseOptions {
        layout,
        roots: filed_from(roots),
        only: Vec::new(),
        apply: how.apply,
        walk_back: how.walk_back,
    })?)?;

    if how.walk_back && summary.plan.moves.is_empty() {
        said!("{}", words::main::nothing_to_put_back());
        return Ok(());
    }
    said_on!("{}", organised(&summary, &library.roots()?, how.apply));
    if how.apply {
        tell_the_players_where_files_went(&summary);
    }
    finished(PassKind::Organise, summary.cancelled)?;
    none_failed(
        PassKind::Organise,
        if how.apply { summary.stats.failed } else { 0 },
    )
}

fn tell_the_players_where_files_went(summary: &OrganiseSummary) {
    let moved: Vec<(MediaLocation, MediaLocation)> = summary
        .plan
        .moves
        .iter()
        .flat_map(|planned| planned.files())
        .map(|(from, to)| (MediaLocation::local(from), MediaLocation::local(to)))
        .collect();
    if moved.is_empty() {
        return;
    }

    let running = match Running::listed() {
        Ok(running) => running,
        Err(error) => {
            tracing::debug!(%error, "no session bus to tell a running player where files went");
            return;
        }
    };
    for player in running {
        if let Err(error) = player.relocate(&moved) {
            tracing::warn!(
                %error,
                name = %player.name(),
                "a running player was not told where its queued files went"
            );
        }
    }
}

#[cfg(all(feature = "ui", feature = "online"))]
fn mirroring(config: &Config) -> Option<Arc<dyn resonate_ui::Mirroring>> {
    Some(musicbrainz::Mirroring::of(config))
}

#[cfg(all(feature = "ui", feature = "online"))]
fn updates_checked(config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    updates::checking(config)
}

#[cfg(all(feature = "ui", not(feature = "online")))]
const fn updates_checked(_config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    None
}

#[cfg(all(feature = "ui", not(feature = "online")))]
const fn mirroring(_config: &Config) -> Option<Arc<dyn resonate_ui::Mirroring>> {
    None
}

fn until_told<Progress, Summary>(handle: PassHandle<Progress, Summary>) -> Result<Summary>
where
    Progress: Cancelling + 'static,
{
    let progress = Arc::clone(handle.progress());
    let _interrupting = signals::cancel_when_told(move || progress.cancel());
    Ok(handle.join()?)
}

const fn none_failed(pass: PassKind, failed: u64) -> Result<()> {
    if failed > 0 {
        Err(Error::FilesFailed { pass, failed })
    } else {
        Ok(())
    }
}

const fn finished(pass: PassKind, cancelled: bool) -> Result<()> {
    if cancelled {
        Err(Error::Cancelled { pass })
    } else {
        Ok(())
    }
}

fn filed_from(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots
        .iter()
        .map(|root| root.canonicalize().unwrap_or_else(|_| root.clone()))
        .collect()
}

fn organised(summary: &OrganiseSummary, roots: &[PathBuf], apply: bool) -> String {
    let plan = &summary.plan;
    if plan.moves.is_empty() && plan.refused.is_empty() {
        return format!("{}\n", words::main::nothing_to_organise());
    }

    let mut told = String::new();
    if !plan.moves.is_empty() {
        let mut table = Table::new(vec![columns::from(), columns::to()]);
        for (from, to) in plan.moves.iter().flat_map(Move::files) {
            table.push(vec![under_its_root(from, roots), under_its_root(to, roots)]);
        }
        told.push_str(&table.render());
    }

    for refused in &plan.refused {
        told.push_str(&left_standing(refused, roots));
        told.push('\n');
    }

    let stats = &summary.stats;
    let (moved, folders) = if apply {
        (
            words::main::moved(stats.moved),
            words::main::folders_pruned(stats.pruned),
        )
    } else {
        (
            words::main::would_move(plan.files_moving()),
            words::main::folders_to_empty(plan.folders.len()),
        )
    };

    told.push_str(&words::main::organised(
        stats.collided,
        stats.failed,
        folders,
        moved,
        stats.unchanged,
        stats.unidentified,
    ));
    told.push('\n');
    if !apply {
        told.push_str(words::main::nothing_moved());
        told.push('\n');
    }
    if summary.cancelled {
        told.push_str(words::main::cancelled());
        told.push('\n');
    }
    told
}

fn left_standing(refused: &Refused, roots: &[PathBuf]) -> String {
    let standing = under_its_root(&refused.from, roots);
    let why = stands_because(&refused.refusal);

    match &refused.refusal {
        Refusal::Collided { with }
        | Refusal::SharesASheet { sheet: with }
        | Refusal::NamedFromAbove { sheet: with } => {
            words::main::refused_with(standing, why, under_its_root(with, roots))
        }
        Refusal::Unmoved { kind } => words::main::refused_kind(kind.to_string(), standing, why),
        _ => words::main::refused(standing, why),
    }
}

fn stands_because(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::Unidentified => words::main::unidentified(),
        Refusal::Loose => words::main::loose(),
        Refusal::Collided { .. } => words::main::collided(),
        Refusal::SharesASheet { .. } => words::main::shares_a_sheet(),
        Refusal::NamedFromAbove { .. } => words::main::named_from_above(),
        Refusal::OutOfReach => words::main::out_of_reach(),
        Refusal::SourceGone => words::main::source_gone(),
        Refusal::Unmoved { .. } => words::main::unmoved(),
    }
}

fn under_its_root(path: &Path, roots: &[PathBuf]) -> String {
    roots
        .iter()
        .find_map(|root| path.strip_prefix(root).ok())
        .unwrap_or(path)
        .display()
        .to_string()
}

fn explain(cli: &Cli, config: &Config, path: &Path) -> Result<()> {
    let sources = sources_over(vault_already_kept(cli, config).as_ref());
    let info = resonate_codec::probe(&sources, &MediaLocation::local(path))?;

    let wanted = wanted_sink(cli, config);
    let sink = select_sink(surveyed::sinks()?, wanted.as_ref())?;
    let config = engine_config(cli, config);
    let replay_gain =
        resolve_replay_gain(config.replay_gain, config.levelling, &info.tags.replay_gain);
    let decoded = Decoded::of(&info, None);
    let plan = plan_output(decoded, &sink, &config, replay_gain);

    said!("{}", words::main::explain_source(info.spec.to_string()));
    if let Packing::DopMarked(rate) = info.packing {
        said!("{}", words::main::explain_dsd(rate.hz(), rate.to_string()));
    }
    said!(
        "{}",
        words::main::explain_sink(&sink.description, sink.name.as_str())
    );
    said!(
        "{}",
        words::main::explain_output(format!("{:?}", plan.mode), plan.stream.to_string())
    );
    if let Some(framing) = sink.words_for(plan.stream.format)
        && framing != Words::Whole
    {
        said!(
            "{}",
            words::main::explain_words(
                framing.spelled(plan.stream.format),
                Words::offered_for(plan.stream.format).spelled(plan.stream.format),
            )
        );
    }
    if let Some((from, to)) = plan.remix {
        said!(
            "{}",
            words::main::explain_remix(from.to_string(), to.to_string())
        );
    }
    if let Some((from, to)) = plan.resample {
        said!(
            "{}",
            words::main::explain_resample(
                from.to_string(),
                format!("{:?}", config.filter_phase),
                format!("{:?}", config.quality),
                to.to_string(),
            )
        );
    }
    if let Some(profile) = plan.equalisation.as_ref() {
        said!(
            "{}{}",
            words::main::explain_eq(
                profile.applied(plan.stream.rate),
                format!("{:+.2}", profile.peak_db(plan.stream.rate)),
                profile.preamp().to_string(),
            ),
            above_nyquist(profile.passed_over(plan.stream.rate)),
        );
    } else if config.equaliser.enabled {
        said!("{}", words::main::explain_eq_unbound());
    }
    if let Some(impulse) = plan.convolution.as_ref() {
        said!(
            "{}",
            words::main::explain_room(
                impulse.channels(),
                impulse.rate().to_string(),
                impulse.frames(),
            )
        );
    }
    if let Some(restoring) = plan.restoration {
        let wall = restoring.wall.map_or_else(
            || words::main::found_as_it_plays().to_owned(),
            |wall| {
                words::main::studied_at(format!("{:.1}", f64::from(wall.centihertz()) / 100_000.0))
            },
        );
        said!(
            "{}",
            words::main::explain_restore(
                restoring.restoration.as_str(),
                format!("{:?}", restoring.tuning),
                wall,
            )
        );
    }
    if let Some(crossfeed) = plan.crossfeed {
        said!("{}", words::main::explain_crossfeed(crossfeed.as_str()));
    }
    if plan.true_peak {
        said!("{}", words::main::explain_guard());
    }
    if let Some(depth) = plan.dither_to {
        said!(
            "{}",
            words::main::explain_dither(depth.to_string(), format!("{:?}", plan.shaping))
        );
    }
    if let Packing::DopMarked(_) = info.packing {
        said!(
            "{}",
            words::main::explain_dop(dop_reading(&plan, &sink, &config, info.spec))
        );
    }
    Ok(())
}

fn above_nyquist(bands: usize) -> String {
    match bands {
        0 => String::new(),
        bands => format!(" ({})", words::main::above_nyquist(bands)),
    }
}

fn dop_reading(
    plan: &OutputPlan,
    sink: &SinkInfo,
    config: &EngineConfig,
    source: StreamSpec,
) -> &'static str {
    if matches!(plan.packing, Packing::DopMarked(_)) {
        return words::main::dop_packed();
    }
    if !config.dop {
        return words::main::dop_off();
    }
    if !sink.supports(source) {
        return words::main::dop_inexact();
    }
    words::main::dop_gain()
}

fn list_playlists(
    library: &Library,
    order: PlaylistOrderArg,
    reverse: bool,
    named: Option<&str>,
) -> Result<()> {
    let order = PlaylistOrder::from(order);
    let reading = read_as(order.reads(), reverse);
    let listed = library.playlists(order, reading, named)?;

    let mut table = Table::new(vec![
        columns::playlist(),
        columns::fills(),
        columns::tracks(),
        columns::length(),
        columns::last_played(),
        columns::plays(),
        columns::pinned(),
        columns::from(),
    ]);
    for playlist in listed {
        table.push(vec![
            playlist.name.clone(),
            fills(&playlist),
            playlist.entries.to_string(),
            played(playlist.duration),
            ago(playlist.played),
            times(playlist.plays),
            ago(playlist.pinned),
            followed::came_from(playlist.source.as_ref()),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

fn times(plays: u32) -> String {
    match plays {
        0 => String::new(),
        plays => plays.to_string(),
    }
}

fn fills(playlist: &Playlist) -> String {
    let Some(query) = playlist.query.as_ref() else {
        return match playlist.kept {
            Some(kept) => words::main::a_list_kept_in(kept_order(kept)),
            None => words::main::a_list().to_owned(),
        };
    };

    match query.text.as_deref() {
        Some(text) => words::main::itself_with(format!("{text:?}")),
        None => words::main::itself().to_owned(),
    }
}

fn kept_order(kept: Kept) -> String {
    match kept.reading {
        Direction::Ascending => words::main::in_order(ordered_by(kept.order)),
        Direction::Descending => words::main::in_reverse_order(ordered_by(kept.order)),
    }
}

fn ordered_by(order: RowOrder) -> &'static str {
    match order {
        RowOrder::Album => words::main::by_album(),
        RowOrder::Artist => words::main::by_artist(),
        RowOrder::Title => words::main::by_title(),
        RowOrder::Length => words::main::by_length(),
        RowOrder::File => words::main::by_file_name(),
    }
}

fn played(duration: Option<Duration>) -> String {
    let Some(duration) = duration else {
        return String::new();
    };
    let seconds = duration.as_secs();
    let minutes = seconds / 60;

    match minutes / 60 {
        0 => format!("{minutes}:{:02}", seconds % 60),
        hours => format!("{hours}:{:02}:{:02}", minutes % 60, seconds % 60),
    }
}

fn ago(played: Option<SystemTime>) -> String {
    let Some(played) = played else {
        return String::new();
    };
    let Ok(since) = played.elapsed() else {
        return words::main::just_now().to_owned();
    };

    let minutes = since.as_secs() / 60;
    let hours = minutes / 60;
    match (hours / 24, hours, minutes) {
        (0, 0, 0) => words::main::just_now().to_owned(),
        (0, 0, minutes) => words::main::minutes_ago(minutes),
        (0, hours, _) => words::main::hours_ago(hours),
        (days, _, _) => words::main::days_ago(days),
    }
}

fn playlist(cli: &Cli, config: &Config, wanted: &PlaylistArgs) -> Result<()> {
    let library = Arc::new(open_library(cli, config)?);
    let name = wanted.name.as_str();
    if let Some(text) = wanted.query.as_deref() {
        return save_query(&library, name, text, wanted);
    }
    if !wanted.add.is_empty() {
        return extend(&library, name, &wanted.add);
    }

    let found = library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;
    if let Some(into) = wanted.rename.as_deref() {
        library.rename_playlist(found.id, into)?;
        said!("{}", words::main::renamed(into, &found.name));
        return Ok(());
    }
    if wanted.discard {
        library.remove_playlist(found.id)?;
        said!("{}", discarded(&found));
        return Ok(());
    }
    if wanted.refresh {
        return followed::refresh(&library, config, &found);
    }
    if let Some(upkeep) = wanted.upkeep {
        return followed::set_upkeep(&library, &found, upkeep.into());
    }
    if wanted.forget_link {
        return followed::forget_link(&library, &found);
    }
    if wanted.pin || wanted.unpin {
        library.pin_playlist(found.id, wanted.pin)?;
        said!("{}", pinning(&found.name, wanted.pin));
        return Ok(());
    }
    if let Some(target) = wanted.export.as_deref() {
        let written = library.export_playlist(found.id, target)?;
        said!(
            "{}",
            words::main::exported(
                written.format.name(),
                &found.name,
                target.display().to_string(),
                written.rows,
            )
        );
        return Ok(());
    }
    if let Some(target) = wanted.into.as_deref() {
        return copy_into(&library, &found, target, wanted.matching.as_deref());
    }
    if let Some(order) = wanted.order {
        let kept = Kept {
            order: order.into(),
            reading: read_as(Direction::Ascending, wanted.reverse),
        };
        if wanted.keep {
            let moved = library.keep_playlist_in_order(found.id, Some(kept))?;
            said!("{}", now_kept(moved, &found.name, kept));
            return Ok(());
        }
        let moved = library.sort_playlist(found.id, kept.order, kept.reading)?;

        said!("{}", reordered(moved, &found.name));
        return Ok(());
    }
    if wanted.by_hand {
        library.keep_playlist_in_order(found.id, None)?;
        said!("{}", words::main::back_in_hand(&found.name));
        return Ok(());
    }
    if wanted.tidy {
        said!("{}", tidied(library.prune_playlist(found.id)?, &found.name));
        return Ok(());
    }
    if wanted.fold {
        said!("{}", folded(library.fold_doubles(found.id)?, &found.name));
        return Ok(());
    }
    if let Some(text) = wanted.matching.as_deref().filter(|_| wanted.drop) {
        let dropped = library.remove_matching(found.id, text)?;
        said!("{}", taken_out(dropped, &found.name, text));
        return Ok(());
    }

    let matching = wanted.matching.as_deref();
    let entries = library.playlist_entries(found.id, matching)?;
    if entries.is_empty() {
        said!("{}", words::main::holds_nothing(&found.name));
        return Ok(());
    }

    let items = playlists::queue_items(&entries);
    let whole = matching.is_none();
    play_queue(
        cli,
        config,
        items,
        Some(library),
        Opening {
            playing: whole.then_some(found.id),
            ..Opening::default()
        },
        &wanted.transport,
    )
}

fn copy_into(
    library: &Library,
    found: &Playlist,
    target: &str,
    matching: Option<&str>,
) -> Result<()> {
    let into = match library.playlist_named(target)? {
        Some(held) => held.id,
        None => library.create_playlist(target)?,
    };
    let copied = library.copy_playlist(found.id, into, matching)?;
    let held = library
        .playlist(into)?
        .map_or(copied as u32, |playlist| playlist.entries);

    said!("{}", words::main::copied(copied, held, &found.name, target));
    Ok(())
}

const fn read_as(reads: Direction, reversed: bool) -> Direction {
    if reversed { reads.flipped() } else { reads }
}

fn save_query(library: &Library, name: &str, text: &str, wanted: &PlaylistArgs) -> Result<()> {
    let sort = SortOrder::from(wanted.sort);
    let query = SavedQuery {
        text: (!text.trim().is_empty()).then(|| text.trim().to_owned()),
        sort,
        reading: read_as(sort.reads(), wanted.reverse),
        limit: wanted.limit.map(NonZeroUsize::get),
    };

    let (id, revised) = match library.playlist_named(name)? {
        Some(found) => {
            library.revise_query(found.id, &found.name, &query)?;
            (found.id, Some(found.name))
        }
        None => (library.save_query(name, &query)?, None),
    };
    let held = library.playlist(id)?.map_or(0, |found| found.entries);
    let matched = matching(query.text.as_deref());

    said!(
        "{}",
        match revised {
            Some(revised) => words::main::query_revised(held, matched, revised),
            None => words::main::query_saved(held, matched, name),
        }
    );
    if let Some(reads) = understood(query.text.as_deref()) {
        said!("{}", words::main::reads_as(reads));
    }
    Ok(())
}

fn matching(text: Option<&str>) -> String {
    match text {
        Some(text) => format!(" {}", words::main::whatever_matches(format!("{text:?}"))),
        None => format!(" {}", words::main::the_whole_library()),
    }
}

fn understood(text: Option<&str>) -> Option<String> {
    let reads = Search::read(text?).reads();

    (!reads.is_empty()).then(|| reads.join(" · "))
}

fn extend(library: &Library, name: &str, paths: &[PathBuf]) -> Result<()> {
    let sources = Sources::local();
    let mut wanted = Vec::with_capacity(paths.len());
    for path in paths.iter().map(|path| local_path(path)) {
        let location =
            MediaLocation::local(
                path.canonicalize()
                    .map_err(|source| Error::UnresolvedFile {
                        path: path.clone(),
                        source,
                    })?,
            );
        match location.as_path().filter(|_| names_a_sheet(&location)) {
            Some(sheet) => wanted.extend(sheet_cuts(&sources, sheet).into_iter().map(
                |(location, span)| Cut {
                    location,
                    span: Some(span),
                },
            )),
            None => wanted.push(Cut::whole(location)),
        }
    }

    let (id, added) = match library.playlist_named(name)? {
        Some(found) => (found.id, library.add_to_playlist(found.id, &wanted)?),
        None => (library.start_playlist(name, &wanted)?, wanted.len()),
    };
    let held = library
        .playlist(id)?
        .map_or(added as u32, |playlist| playlist.entries);

    said!("{}", words::main::added(added, held, name));
    Ok(())
}

fn import(
    library: &Library,
    config: &Config,
    files: &[PathBuf],
    name: Option<&str>,
    upkeep: Upkeep,
) -> Result<()> {
    if name.is_some() && files.len() > 1 {
        return Err(Error::NameForOnePlaylistOnly { given: files.len() });
    }

    for file in files {
        if let Some(profile) = followed::profile_named_by(file) {
            followed::follow_profile(library, config, &profile, upkeep)?;
            continue;
        }
        if let Some(link) = followed::link_named_by(file) {
            followed::import(library, config, &link, name, upkeep)?;
            continue;
        }
        if upkeep == Upkeep::InStep {
            return Err(Error::KeptInStepOnlyWithALink { file: file.clone() });
        }
        let read = library.import_playlist(file, name)?;
        let held = library
            .playlist(read.id)?
            .map_or(read.added as u32, |playlist| playlist.entries);

        said!(
            "{}{}{}{}{}",
            words::main::imported(
                read.added,
                read.encoding.name(),
                read.format.name(),
                held,
                &read.name,
            ),
            already_held(read.already),
            passed_over(read.elsewhere),
            not_on_disk(read.missing),
            fell_short(read.short)
        );
    }
    Ok(())
}

fn reordered(moved: usize, name: &str) -> String {
    match moved {
        0 => words::main::already_in_order(name),
        moved => words::main::reordered(moved, name),
    }
}

fn now_kept(moved: usize, name: &str, kept: Kept) -> String {
    let order = kept_order(kept);
    match moved {
        0 => words::main::kept_already(name, order),
        moved => words::main::now_kept(moved, name, order),
    }
}

fn tidied(dropped: usize, name: &str) -> String {
    match dropped {
        0 => words::main::all_there(name),
        dropped => words::main::tidied(dropped, name),
    }
}

fn folded(dropped: usize, name: &str) -> String {
    match dropped {
        0 => words::main::none_doubled(name),
        dropped => words::main::folded_doubles(dropped, name),
    }
}

fn pinning(name: &str, pinned: bool) -> String {
    if pinned {
        words::main::pinned(name)
    } else {
        words::main::unpinned(name)
    }
}

fn discarded(found: &Playlist) -> String {
    let name = &found.name;
    if found.query.is_some() {
        return words::main::discarded_search(name);
    }

    match found.entries {
        0 => words::main::discarded_empty(name),
        held => words::main::discarded(held, name),
    }
}

fn taken_out(dropped: usize, name: &str, text: &str) -> String {
    match dropped {
        0 => words::main::none_match(name, format!("{text:?}")),
        dropped => words::main::took_out(dropped, name, format!("{text:?}")),
    }
}

fn already_held(already: usize) -> String {
    match already {
        0 => String::new(),
        already => format!(" | {}", words::main::already_in_it(already)),
    }
}

fn passed_over(elsewhere: usize) -> String {
    match elsewhere {
        0 => String::new(),
        passed => format!(" | {}", words::main::passed_over(passed)),
    }
}

fn fell_short(short: usize) -> String {
    match short {
        0 => String::new(),
        short => format!(" | {}", words::main::fell_short(short)),
    }
}

fn not_on_disk(missing: usize) -> String {
    match missing {
        0 => String::new(),
        missing => format!(" | {}", words::main::not_on_disk(missing)),
    }
}

fn play(
    cli: &Cli,
    config: &Config,
    arguments: &[OsString],
    spec: Option<&str>,
    transport: &TransportArgs,
) -> Result<()> {
    let asleep = spec.map(sleep::Sleep::read).transpose()?;
    let library = catalog(cli, config);

    let items = mpris::claimed_by(
        library.as_deref(),
        queue_items_served(arguments, &Served::of(config, settings_path(cli).ok())),
    );
    if items.is_empty() {
        return Err(if arguments.is_empty() {
            Error::NoFileNamed
        } else {
            Error::NothingPlayableNamed
        });
    }
    play_queue(
        cli,
        config,
        items,
        library,
        Opening {
            asleep: asleep.and_then(sleep::Sleep::until),
            ..Opening::default()
        },
        transport,
    )
}

fn queue_onto_a_running_player(cli: &Cli, config: &Config, wanted: &QueueArgs) -> Result<()> {
    let running = reached(wanted.player.as_deref(), wanted.zone.as_deref())?;
    if let Some(to) = wanted.move_to.as_deref().map(ZoneAsked::read) {
        return move_the_playback(&running, to);
    }
    let rows = match wanted.playlist.as_deref() {
        Some(name) => playlist_rows(cli, config, name)?,
        None => queue_items_served(&wanted.files, &Served::of(config, settings_path(cli).ok()))
            .into_iter()
            .map(|item| (item.location, item.span))
            .collect(),
    };
    let queued = running.queue(
        &rows,
        Queueing {
            at: if wanted.next {
                Placement::Next
            } else {
                Placement::Queued
            },
            play: wanted.play,
        },
    )?;

    said!(
        "{}",
        words::main::queued_onto(running.name().to_string(), queued)
    );
    Ok(())
}

fn move_the_playback(running: &Running, to: ZoneAsked) -> Result<()> {
    let target = Running::zoned(&to)?
        .and_then(|target| target.zone_number())
        .ok_or(Error::NoSuchZone { zone: to })?;
    running.hand_over(target)?;

    said!(
        "{}",
        words::main::moving_to(running.name().to_string(), target.to_string())
    );
    Ok(())
}

fn reached(player: Option<&str>, zone: Option<&str>) -> Result<Running> {
    if let Some(zone) = zone.map(ZoneAsked::read) {
        return Running::zoned(&zone)?.ok_or(Error::NoSuchZone { zone });
    }
    let Some(name) = player.map(PlayerName::new) else {
        return Running::found()?.ok_or(Error::NothingRunning);
    };
    Running::named(&name)?.ok_or(Error::NoSuchPlayer { name })
}

fn playlist_rows(
    cli: &Cli,
    config: &Config,
    name: &str,
) -> Result<Vec<(MediaLocation, Option<FrameSpan>)>> {
    let library = open_library(cli, config)?;
    let found = library
        .playlist_named(name)?
        .ok_or_else(|| Error::NoSuchPlaylist(PlaylistName::new(name)))?;

    Ok(library
        .playlist_entries(found.id, None)?
        .into_iter()
        .map(|entry| (entry.cut.location, entry.cut.span))
        .collect())
}

fn players() -> Result<()> {
    let standing: Vec<Standing> = Running::listed()?
        .iter()
        .filter_map(|player| match player.standing() {
            Ok(standing) => Some(standing),
            Err(error) => {
                tracing::debug!(
                    %error,
                    name = %player.name(),
                    "a player stopped answering while it was being listed"
                );
                None
            }
        })
        .collect();

    if standing.is_empty() {
        said!("{}", words::main::no_player());
        return Ok(());
    }

    let mut table = Table::new(vec![
        columns::player(),
        columns::zone(),
        columns::status(),
        columns::queued(),
        columns::title(),
        columns::artist(),
    ]);
    for player in &standing {
        table.push(vec![
            player.name.to_string(),
            player.zone.clone().unwrap_or_default(),
            player
                .playback
                .map_or_else(String::new, |status| status.as_str().to_owned()),
            player.queued.to_string(),
            player.title.clone().unwrap_or_default(),
            player.artist.clone().unwrap_or_default(),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

#[cfg(feature = "sync")]
fn synced(cli: &Cli, config: &Config, args: &SyncArgs) -> Result<()> {
    let settings = settings_path(cli)?;
    let asked = match (
        &args.join,
        &args.forget,
        &args.fetch,
        &args.from,
        &args.handoff,
    ) {
        _ if args.control.is_some() => sync::Asked::Control {
            device: args.control.as_deref().unwrap_or_default(),
            zone: args.zone.as_deref().map(ZoneAsked::read),
            command: remote_command(args)?,
        },
        (Some(code), ..) => sync::Asked::Join(code),
        _ if args.leave => sync::Asked::Leave,
        (_, Some(device), ..) => sync::Asked::Forget(device),
        (_, _, Some(playlist), Some(from), _) => sync::Asked::Fetch { playlist, from },
        (.., Some(from)) => sync::Asked::Handoff { from },
        _ => sync::Asked::Standing,
    };
    let library = match asked {
        sync::Asked::Join(_) | sync::Asked::Leave => None,
        _ => Some(Arc::new(open_library(cli, config)?)),
    };
    let Some(handed) = reloading::delivering(&settings, Some(Reloadable::Sync), || {
        sync::asked(config, &settings, library.clone(), asked)
    })?
    else {
        return Ok(());
    };
    let transport = TransportArgs {
        shuffle: false,
        repeat: None,
        volume: None,
    };
    play_queue(
        cli,
        config,
        handed.items,
        library,
        Opening {
            place: handed.at,
            ..Opening::default()
        },
        &transport,
    )
}

#[cfg(feature = "sync")]
fn remote_command(args: &SyncArgs) -> Result<Option<resonate_sync::RemoteCommand>> {
    use resonate_sync::{Permille, RemoteCommand};

    let row = |given: u32| usize::try_from(given.saturating_sub(1)).unwrap_or(usize::MAX);
    if let Some(key) = args.press {
        return Ok(Some(match key {
            cli::PressArg::Play => RemoteCommand::Play,
            cli::PressArg::Pause => RemoteCommand::Pause,
            cli::PressArg::Toggle => RemoteCommand::Toggle,
            cli::PressArg::Next => RemoteCommand::Next,
            cli::PressArg::Previous => RemoteCommand::Previous,
        }));
    }
    if let Some(given) = args.seek.as_deref() {
        let at =
            lasting::lasting(given, lasting::Bare::Seconds).ok_or_else(|| Error::NotAPlace {
                given: given.trim().into(),
            })?;
        return Ok(Some(RemoteCommand::Seek(at)));
    }
    if let Some(percent) = args.set_volume {
        return Ok(Some(RemoteCommand::Volume(Permille::new(
            u16::from(percent) * 10,
        ))));
    }
    Ok(args
        .jump
        .map(|given| RemoteCommand::Jump(row(given)))
        .or_else(|| args.remove.map(|given| RemoteCommand::Remove(row(given))))
        .or_else(|| {
            args.move_row
                .zip(args.to)
                .map(|(from, to)| RemoteCommand::Move {
                    from: row(from),
                    to: row(to),
                })
        }))
}

#[cfg(not(feature = "sync"))]
fn synced(_cli: &Cli, _config: &Config, _args: &SyncArgs) -> Result<()> {
    Err(Error::SyncOff)
}

#[cfg(feature = "sync")]
fn shared(cli: &Cli, config: &Config, args: &SharedArgs) -> Result<()> {
    together::run(
        config,
        &settings_path(cli)?,
        Arc::new(open_library(cli, config)?),
        args,
    )
}

#[cfg(not(feature = "sync"))]
fn shared(_cli: &Cli, _config: &Config, _args: &SharedArgs) -> Result<()> {
    Err(Error::SyncOff)
}

#[cfg(feature = "sync")]
fn party_asked(cli: &Cli, config: &Config, wanted: &cli::PartyWanted) -> Result<()> {
    party::run(cli, config, wanted)
}

#[cfg(not(feature = "sync"))]
fn party_asked(_cli: &Cli, _config: &Config, _wanted: &cli::PartyWanted) -> Result<()> {
    Err(Error::SyncOff)
}

#[derive(Default)]
struct Opening {
    playing: Option<PlaylistId>,
    asleep: Option<Until>,
    place: Frames,
}

fn play_queue(
    cli: &Cli,
    config: &Config,
    items: Vec<QueueItem>,
    library: Option<Arc<Library>>,
    opening: Opening,
    transport: &TransportArgs,
) -> Result<()> {
    let Opening {
        playing,
        asleep,
        place,
    } = opening;
    let sources = playing_from(
        library.as_deref(),
        &Served::of(config, settings_path(cli).ok()),
    );
    let mut engine = kinship::kin_from(engine_config(cli, config), library.as_ref());
    if let Some(percent) = transport.volume {
        engine.volume = at_percent(percent)?;
    }
    let wanted = wanted_sink(cli, config);
    let player = Arc::new(
        Rendering::of(config, wanted.as_ref(), Asked::Nothing)
            .player(engine, Arc::clone(&sources))?,
    );
    confirm_sink(&player, wanted.as_ref());
    let queue = stamp_of(&items);
    let start_at = if transport.shuffle {
        anywhere_in(items.len())
    } else {
        0
    };
    for command in set_out(transport, items, start_at) {
        player.send(command)?;
    }
    if place != Frames::default()
        && let Err(error) = player.send(Command::Seek(place))
    {
        tracing::warn!(%error, "the place the other device was at could not be reached");
    }
    if let Some(library) = library.as_ref() {
        library.set_playing_playlist(playing.map(|playlist| Playing { playlist, queue }));
    }
    if asleep.is_some() {
        player.send(Command::SleepUntil(asleep))?;
    }
    let keeps = resuming(config, library.as_deref());

    let (asked_to_quit, quitting) = bounded(1);
    let silenced = Arc::clone(&player);
    signals::quit_when_told(
        asked_to_quit.clone(),
        signals::silencing(move || vec![Arc::clone(&silenced)]),
    );
    let mpris = mpris::start(
        &mpris::Serving::new(
            &sources,
            library.as_ref(),
            settings_path(cli).ok(),
            Some(asked_to_quit),
            None,
            &Arc::new(AtomicBool::new(config.notifies())),
        ),
        &player,
    );
    let presenter = discord::start(&player, library.as_ref(), config);
    #[cfg(any(windows, target_os = "macos"))]
    let _awake = {
        let playing = Arc::clone(&player);
        awake::start(move || vec![Arc::clone(&playing)])
    };
    let submitting = library
        .as_ref()
        .map(|library| submitting::start(config, library, &player));
    let syncing = library
        .as_ref()
        .map(|library| sync::start(config, settings_path(cli).ok().as_deref(), library));
    #[cfg(feature = "sync")]
    if let Some(syncing) = syncing.as_ref() {
        syncing.drive(resonate_driven::Driven::player(&player));
    }

    let behind = input::played_in_the_background();
    let keyed = (!behind)
        .then(input::KeyAtATime::where_a_terminal)
        .flatten();
    let help = if keyed.is_some() {
        input::keys_said()
    } else {
        input::help_said()
    };
    let mut readout = Readout::over(keyed.is_some() && io::stdout().is_terminal());
    if behind {
        told!("{}", words::main::in_the_background());
    } else {
        said!("{help}");
    }
    let events = player.events().clone();
    let mut keys = if behind {
        never()
    } else if keyed.is_some() {
        input::keys()
    } else {
        input::lines()
    };
    let sampled = tick(HEARD_SAMPLE);
    let mut listening = Listening::default();
    let mut counted: Option<Listen> = None;
    let mut keeping = Keeping::default();
    let mut outcome = Played::default();
    let mut acted = Ok(());

    loop {
        select! {
            recv(events) -> event => {
                let Ok(event) = event else { break };
                readout.clear();
                outcome.hears(&event);
                if announce(event, &player) {
                    break;
                }
                readout.draw(&player);
            }
            recv(sampled) -> _ => {
                count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
                if keeps {
                    keep_the_queue(&player, library.as_deref(), &mut keeping);
                }
                readout.draw(&player);
            }
            recv(keys) -> pressed => {
                let Ok(pressed) = pressed else {
                    keys = never();
                    continue;
                };
                match pressed {
                    Pressed::Acted(Action::Quit) => break,
                    Pressed::Acted(action) => {
                        readout.clear();
                        acted = act(&player, action, &help);
                        if acted.is_err() {
                            break;
                        }
                    }
                    Pressed::Typing(typed) => readout.typing(typed),
                    Pressed::Unknown(line) => {
                        readout.clear();
                        told!("{}", words::main::unknown_key(format!("{line:?}")));
                    }
                }
                readout.draw(&player);
            }
            recv(quitting) -> _ => break,
        }
    }
    readout.clear();
    drop(keyed);

    if keeps {
        keep_the_queue_on_leaving(&player, library.as_deref(), &mut keeping);
    }
    count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
    if let Some(library) = library.as_deref() {
        record_a_play(library, listening.leaves(), &mut counted);
    }
    presenter.leave();
    if let Some(syncing) = syncing {
        syncing.leave();
    }
    if let Some(submitting) = submitting {
        submitting.leave();
    }
    drop(mpris);
    drop(player);
    acted?;
    outcome.played_anything()
}

#[derive(Default)]
struct Played {
    started: bool,
    failed: usize,
    finished: bool,
}

impl Played {
    const fn hears(&mut self, event: &Event) {
        match event {
            Event::TrackStarted(_) => self.started = true,
            Event::Failed { .. } => self.failed += 1,
            Event::QueueFinished => self.finished = true,
            _ => {}
        }
    }

    const fn played_anything(&self) -> Result<()> {
        if !self.finished || self.started || self.failed == 0 {
            Ok(())
        } else {
            Err(Error::NothingPlayed {
                failed: self.failed,
            })
        }
    }
}

fn at_percent(percent: u8) -> Result<Volume> {
    Ok(Volume::new(f32::from(percent) / 100.0)?)
}

fn anywhere_in(rows: usize) -> usize {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as usize)
        .unwrap_or_default();

    now.checked_rem(rows).unwrap_or_default()
}

fn set_out(transport: &TransportArgs, items: Vec<QueueItem>, start_at: usize) -> Vec<Command> {
    let shuffle = transport.shuffle.then_some(Command::SetShuffle(true));
    let repeat = transport
        .repeat
        .map(|repeat| Command::SetRepeat(repeat.into()));
    let load = Command::Load {
        items,
        start_at,
        autoplay: true,
    };

    shuffle.into_iter().chain(repeat).chain([load]).collect()
}

fn count_a_play(
    player: &Player,
    library: Option<&Library>,
    listening: &mut Listening,
    counted: &mut Option<Listen>,
) {
    let Some(library) = library else {
        return;
    };
    record_a_play(
        library,
        listening.heard(&player.state(), &player.queue()),
        counted,
    );
}

fn record_a_play(library: &Library, counting: Option<Counting>, counted: &mut Option<Listen>) {
    let Some(counting) = counting else {
        return;
    };

    match counting {
        Counting::Counts(played) => {
            *counted = match library.track_played(&played.location, played.span, played.heard) {
                Ok(row) => row.map(|row| row.listen),
                Err(error) => {
                    tracing::warn!(%error, "a play was not counted");
                    None
                }
            };
        }
        Counting::Hears(played) => {
            if let Some(listen) = *counted
                && let Err(error) = library.listened(listen, played.heard)
            {
                tracing::warn!(%error, "how long a play has been heard for was not kept");
            }
        }
        Counting::Settles(played) => {
            if let Some(listen) = counted.take()
                && let Err(error) = library.listened(listen, played.heard)
            {
                tracing::warn!(%error, "how long a play was heard for was not kept");
            }
        }
        Counting::Passes(heard) => {
            if let Err(error) = library.passed(heard) {
                tracing::warn!(%error, "how long a passing listen lasted was not kept");
            }
        }
    }
}

fn announce(event: Event, player: &Player) -> bool {
    match event {
        Event::TrackStarted(track) => {
            said!("{}", words::main::started(readout::billing(player, track)))
        }
        Event::TrackFinished(track) => {
            said!("{}", words::main::finished(readout::billing(player, track)))
        }
        Event::OutputChanged(status) => said!(
            "{}",
            words::main::output(
                format!("{:?}", status.mode),
                status.sink.to_string(),
                status.negotiated.to_string(),
            )
        ),
        Event::Underrun { missing } => told!("{}", words::main::underrun(missing.to_string())),
        Event::Failed { track, error } => {
            told!("{}", words::main::failed(readout::billing(player, track)));
            report(&error);
        }
        Event::Waiting { track, error } => {
            told!("{}", words::main::waits(readout::billing(player, track)));
            report(&error);
        }
        Event::CommandFailed { command, error } => {
            told!("{}", words::main::rejected(format!("{command:?}")));
            report(&error);
        }
        Event::QueueFinished => return true,
    }
    false
}

fn act(player: &Player, action: Action, help: &str) -> Result<()> {
    let state = player.state();
    let rate = state.current.map(|track| track.source.rate);

    let command = match action {
        Action::Toggle => Command::TogglePlayPause,
        Action::Next => Command::Next,
        Action::Previous => Command::Previous,
        Action::Stop => Command::Stop,
        Action::SeekTo(moment) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::Seek(Frames::from_duration(moment, rate))
        }
        Action::SeekForward(span) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::SeekBy(frames_in(span, rate))
        }
        Action::SeekBack(span) => {
            let Some(rate) = rate else { return Ok(()) };
            Command::SeekBy(-frames_in(span, rate))
        }
        Action::VolumeBy(step) => {
            let percent = (state.volume.get() * 100.0).round() as i32;
            let wanted = percent.saturating_add(step).clamp(0, 100);
            Command::SetVolume(Volume::new(wanted as f32 / 100.0)?)
        }
        Action::ToggleShuffle => Command::SetShuffle(!state.shuffle),
        Action::CycleRepeat => Command::SetRepeat(match state.repeat {
            RepeatMode::Off => RepeatMode::Queue,
            RepeatMode::Queue => RepeatMode::Track,
            RepeatMode::Track => RepeatMode::Off,
        }),
        Action::Sleep(wanted) => Command::SleepUntil(wanted.until()),
        Action::Help => {
            said!("{help}");
            return Ok(());
        }
        Action::Quit => return Ok(()),
    };

    let kind = command.kind();
    if let Err(error) = player.request(command)?.wait_for(COMMAND_TIMEOUT) {
        told!("{}", words::main::rejected(format!("{kind:?}")));
        report(&error);
    }
    Ok(())
}

fn frames_in(span: Duration, rate: SampleRate) -> i64 {
    i64::try_from(Frames::from_duration(span, rate).get()).unwrap_or(i64::MAX)
}

pub(crate) fn impulse_at(path: &Path, speakers: Speakers) -> Option<Arc<Impulse>> {
    match read_impulse(&Sources::local(), &MediaLocation::local(path), speakers) {
        Ok(Some(impulse)) => Some(Arc::new(impulse)),
        Ok(None) => {
            tracing::warn!(path = %path.display(), "the room's response holds no taps, so nothing is corrected");
            None
        }
        Err(error) => {
            tracing::warn!(%error, "the room's response could not be read, so nothing is corrected");
            None
        }
    }
}

const fn one_way_on_bit_perfect(cli: &Cli) -> Result<()> {
    if cli.bit_perfect && cli.no_bit_perfect {
        Err(Error::BitPerfectBothWays)
    } else {
        Ok(())
    }
}

const fn bit_perfect_asked(cli: &Cli) -> Option<bool> {
    match (cli.bit_perfect, cli.no_bit_perfect) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        (false, false) => None,
    }
}

fn engine_config(cli: &Cli, config: &Config) -> EngineConfig {
    let defaults = EngineConfig::default();
    EngineConfig {
        sink: wanted_sink(cli, config),
        quality: cli
            .quality
            .map(Into::into)
            .or(config.quality)
            .unwrap_or(defaults.quality),
        filter_phase: cli
            .filter_phase
            .map(Into::into)
            .or(config.filter_phase)
            .unwrap_or(defaults.filter_phase),
        dither: cli
            .dither
            .map(Into::into)
            .or(config.dither)
            .unwrap_or(defaults.dither),
        noise_shaping: cli
            .noise_shaping
            .map(Into::into)
            .or(config.noise_shaping)
            .unwrap_or(defaults.noise_shaping),
        replay_gain: config.replay_gain.unwrap_or(defaults.replay_gain),
        levelling: Levelling {
            pre_amp: config.pre_amp.unwrap_or(defaults.levelling.pre_amp),
            untagged: config.untagged.unwrap_or(defaults.levelling.untagged),
        },
        prefer_bit_perfect: bit_perfect_asked(cli)
            .or(config.bit_perfect)
            .unwrap_or(defaults.prefer_bit_perfect),
        dop: config.dop.unwrap_or(defaults.dop),
        dsd_like_pcm: config.dsd_like_pcm.unwrap_or(defaults.dsd_like_pcm),
        device_volume: config.device_volume.unwrap_or(defaults.device_volume),
        true_peak: config.true_peak.unwrap_or(defaults.true_peak),
        restoration: config.restoration.unwrap_or(defaults.restoration),
        crossfeed: config.crossfeed.unwrap_or(defaults.crossfeed),
        force_graph_rate: config.force_graph_rate.unwrap_or(defaults.force_graph_rate),
        bluetooth: BluetoothWake {
            on: config.bluetooth_wake.unwrap_or(defaults.bluetooth.on),
            lead: config.bluetooth_lead.unwrap_or(defaults.bluetooth.lead),
            awake_for: config
                .bluetooth_awake
                .unwrap_or(defaults.bluetooth.awake_for),
        },
        volume: config.volume.unwrap_or(defaults.volume),
        buffer: config.buffer.unwrap_or(defaults.buffer),
        equaliser: Arc::new(equaliser::resolved(config)),
        convolution: config
            .convolution
            .as_deref()
            .and_then(|path| impulse_at(path, config.speakers())),
        skip_under_repeat: config.skip_under_repeat(),
        previous_restarts: config.previous_restarts(),
        smart_shuffle: config.smart_shuffle(),
        ..defaults
    }
}

#[cfg(feature = "ui")]
fn launch(cli: Cli, config: Config, library: Arc<Library>) -> Result<()> {
    let starting = starting::one_window_at_a_time();
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
    let settings = settings::File::at(places.config.clone());
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
    confirm_sink(&player, wanted_sink(&cli, &config).as_ref());
    let keeps = resuming(&config, Some(&library));
    #[cfg(feature = "sync")]
    let files = together::invitations_taken_out(&cli.files);
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
    let opener = zoning::Opener::new(
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
    let kept_zones = zoning::kept_zones(&library, keeps);
    let zones = Zones::start(
        Arc::clone(&player),
        zoning::first_named(&kept_zones),
        Arc::clone(&opener) as Arc<dyn Opens>,
    );
    let silenced = Arc::downgrade(&zones);
    signals::quit_when_told(
        asked_to_quit,
        signals::silencing(move || {
            silenced.upgrade().map_or_else(Vec::new, |zones| {
                zones.listed().into_iter().map(|zone| zone.player).collect()
            })
        }),
    );
    drop(starting);
    zoning::restore(&zones, &library, &kept_zones);
    let listens = listen::in_the_window(&config, opener.teller());
    let desktop = the_desktop_asks(&config);
    let (hears, heard) = crossbeam_channel::unbounded();
    watch_the_desktop(hears);
    let presenter = Arc::new(discord::start(&player, Some(&library), &config));
    #[cfg(any(windows, target_os = "macos"))]
    let _awake = {
        let zones = Arc::downgrade(&zones);
        awake::start(move || {
            zones.upgrade().map_or_else(Vec::new, |zones| {
                zones.listed().into_iter().map(|zone| zone.player).collect()
            })
        })
    };
    let submitting = submitting::start(&config, &library, &player);
    #[cfg(feature = "online")]
    musicbrainz::keep_fresh(&config);
    #[cfg(feature = "sync")]
    let together = together::start(&config, &library).unwrap_or_else(|error| {
        tracing::warn!(%error, "shared playlists could not start");
        None
    });
    #[cfg(feature = "sync")]
    party_room::in_the_window(party_room::Seats {
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
        .map(|_| together::for_the_window(&library));
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
            party_room::out_of_the_window();
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
            launcher: Arc::new(launcher::Icons::start()),
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

#[cfg(feature = "ui")]
const WHAT_IS_OPENED_WAITS_FOR_THE_WINDOW: Duration = Duration::from_secs(15);

#[cfg(feature = "ui")]
const THE_WINDOW_IS_LOOKED_FOR_EVERY: Duration = Duration::from_millis(100);

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
#[derive(Clone, Copy, Default)]
struct DesktopAsks {
    more_contrast: bool,
    reduces_motion: bool,
    look: resonate_core::DesktopLook,
}

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
fn watch_the_desktop(told: crossbeam_channel::Sender<resonate_core::DesktopLook>) {
    let watching = resonate_mpris::watch_the_desktop(move |preferences| {
        let _ = told.send(look_of(preferences));
    });
    if let Err(error) = watching {
        tracing::debug!(%error, "the desktop's colour scheme and text scale are read once, at start");
    }
}

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
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

#[cfg(feature = "ui")]
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

#[cfg(all(feature = "ui", feature = "sync"))]
fn names_an_invitation(argument: &OsString) -> bool {
    resonate_sync::shared::invitation_in(&argument.to_string_lossy()).is_some()
}

#[cfg(all(feature = "ui", not(feature = "sync")))]
fn names_an_invitation(_argument: &OsString) -> bool {
    false
}

#[cfg(not(feature = "ui"))]
fn launch(cli: Cli, config: Config, _library: Arc<Library>) -> Result<()> {
    if !cli.files.is_empty() {
        return play(&cli, &config, &cli.files, None, &TransportArgs::default());
    }
    let player = Player::new(engine_config(&cli, &config))?;
    confirm_sink(&player, wanted_sink(&cli, &config).as_ref());
    player.shutdown()?;
    Ok(())
}

#[cfg(feature = "ui")]
fn queue(
    player: &Player,
    arguments: &[OsString],
    kept: Option<&Library>,
    served: &Served,
) -> Result<()> {
    if !arguments.is_empty() {
        player.send(Command::Load {
            items: mpris::claimed_by(kept, queue_items_served(arguments, served)),
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

#[cfg(feature = "ui")]
fn resumed(kept: Option<&Library>) -> Option<Resumption> {
    match kept?.resumption() {
        Ok(resumption) => resumption,
        Err(error) => {
            tracing::warn!(%error, "the queue kept from the last run could not be read");
            None
        }
    }
}

fn resuming(config: &Config, library: Option<&Library>) -> bool {
    if config.resumes() {
        return true;
    }
    if let Some(library) = library
        && let Err(error) = library.forget_resumption()
    {
        tracing::warn!(%error, "what was kept of a queue could not be discarded");
    }
    false
}

fn keep_the_queue(player: &Player, library: Option<&Library>, keeping: &mut Keeping) {
    keep_the_queue_by(player, library, keeping, Keeping::kept);
}

fn keep_the_queue_on_leaving(player: &Player, library: Option<&Library>, keeping: &mut Keeping) {
    keep_the_queue_by(player, library, keeping, Keeping::kept_on_leaving);
}

fn keep_the_queue_by(
    player: &Player,
    library: Option<&Library>,
    keeping: &mut Keeping,
    keep: fn(&mut Keeping, &PlayerState, &Queued) -> Option<Keep>,
) {
    let Some(library) = library else {
        return;
    };
    let kept = match keep(keeping, &player.state(), &player.queued()) {
        Some(Keep::Queue(resumption)) => library.keep_resumption(ZoneId::FIRST, &resumption),
        Some(Keep::Order(reordered)) => library.keep_order(ZoneId::FIRST, &reordered),
        Some(Keep::Place { row, at }) => library.keep_place(ZoneId::FIRST, row, at),
        None => return,
    };

    if let Err(error) = kept {
        tracing::warn!(%error, "the queue was not kept for the next run");
    }
}

const SHEET_EXTENSION: &str = "cue";
const FOLDERS_DEEP: usize = 32;

fn local_path(argument: &Path) -> PathBuf {
    argument
        .to_str()
        .and_then(MediaLocation::from_uri)
        .and_then(|named| named.as_path().map(Path::to_path_buf))
        .unwrap_or_else(|| argument.to_path_buf())
}

fn location_of_argument(argument: &OsStr, sources: &Sources) -> MediaLocation {
    argument
        .to_str()
        .and_then(MediaLocation::from_uri)
        .filter(|named| held_by(named, sources))
        .map(settled_here)
        .unwrap_or_else(|| MediaLocation::local(from_here(Path::new(argument))))
}

fn cut_of_argument(
    argument: &OsStr,
    sources: &Sources,
) -> Result<(MediaLocation, Option<FrameSpan>)> {
    if let Some(uri) = argument
        .to_str()
        .filter(|uri| MediaLocation::claims_a_span(uri))
    {
        if let Some((location, span)) =
            MediaLocation::from_uri_within(uri).filter(|(named, _)| held_by(named, sources))
        {
            return Ok((settled_here(location), span));
        }
        if let Some(location) = MediaLocation::from_uri(uri).filter(|named| held_by(named, sources))
        {
            return Err(Error::UnreadableSpan {
                location: settled_here(location),
            });
        }
    }
    Ok((location_of_argument(argument, sources), None))
}

fn held_by(named: &MediaLocation, sources: &Sources) -> bool {
    named.as_path().is_some() || sources.provider(named.source()).is_some()
}

fn settled_here(location: MediaLocation) -> MediaLocation {
    match location.as_path() {
        Some(path) => MediaLocation::local(from_here(path)),
        None => location,
    }
}

fn from_here(path: &Path) -> PathBuf {
    path.canonicalize()
        .or_else(|_| {
            path::absolute(path).map(|absolute| resonate_core::path_text::verbatim(&absolute))
        })
        .unwrap_or_else(|_| path.to_path_buf())
}

fn queue_items(arguments: &[OsString]) -> Vec<QueueItem> {
    queue_items_served(arguments, &Served::default())
}

fn queue_items_served(arguments: &[OsString], served: &Served) -> Vec<QueueItem> {
    let sources = served.sources(Sources::local());
    let mut minting = Unclaimed::beside(&[]);
    let mut items = Vec::with_capacity(arguments.len());

    for argument in arguments {
        let (location, span) = match cut_of_argument(argument, &sources) {
            Ok(cut) => cut,
            Err(error) => {
                tracing::warn!(%error, "an argument naming a cut was not queued");
                continue;
            }
        };
        if span.is_some() {
            items.push(QueueItem {
                id: minting.mint(),
                location,
                span,
            });
            continue;
        }
        if let Some(songs) = served.songs_at(&location) {
            items.extend(
                songs
                    .into_iter()
                    .map(|song| QueueItem::whole(minting.mint(), song)),
            );
            continue;
        }
        items.extend(items_of(&sources, location, &mut minting));
    }
    items
}

fn items_of(sources: &Sources, location: MediaLocation, minting: &mut Unclaimed) -> Vec<QueueItem> {
    let Some(path) = location.as_path().map(Path::to_path_buf) else {
        return vec![QueueItem::whole(minting.mint(), location)];
    };
    if path.is_dir() {
        return folder_items(sources, &path, minting, FOLDERS_DEEP, &mut BTreeSet::new());
    }
    if names_a_sheet(&location) {
        return sheet_items(sources, &path, minting);
    }
    if PlaylistFormat::named_by(&path).is_some() {
        return listed_items(&path, minting);
    }
    vec![QueueItem::whole(minting.mint(), location)]
}

fn listed_items(path: &Path, minting: &mut Unclaimed) -> Vec<QueueItem> {
    match cuts_listed_in(path) {
        Ok(cuts) => cuts
            .into_iter()
            .map(|cut| QueueItem {
                id: minting.mint(),
                location: cut.location,
                span: cut.span,
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a playlist could not be read");
            Vec::new()
        }
    }
}

fn listed_in_order(folder: &Path) -> Vec<PathBuf> {
    let mut listed: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| !entry.file_name().as_encoded_bytes().starts_with(b"."))
            .map(|entry| entry.path())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, folder = %folder.display(), "a folder could not be read");
            Vec::new()
        }
    };
    listed.sort();
    listed
}

fn folder_items(
    sources: &Sources,
    folder: &Path,
    minting: &mut Unclaimed,
    deeper: usize,
    walked: &mut BTreeSet<PathBuf>,
) -> Vec<QueueItem> {
    let Some(deeper) = deeper.checked_sub(1) else {
        tracing::warn!(folder = %folder.display(), "a folder nested too deep was not queued");
        return Vec::new();
    };
    let first_time_through = folder
        .canonicalize()
        .map_or(true, |canonical| walked.insert(canonical));
    if !first_time_through {
        tracing::debug!(folder = %folder.display(), "a folder reached again through a link was queued once");
        return Vec::new();
    }
    let (folders, files): (Vec<PathBuf>, Vec<PathBuf>) = listed_in_order(folder)
        .into_iter()
        .partition(|path| path.is_dir());
    let sheets: BTreeMap<&Path, Vec<(MediaLocation, FrameSpan)>> = files
        .iter()
        .filter(|file| names_a_sheet(&MediaLocation::local(file)))
        .map(|sheet| (sheet.as_path(), sheet_cuts(sources, sheet)))
        .collect();
    let cut_from: BTreeSet<PathBuf> = sheets
        .values()
        .flatten()
        .filter_map(|(location, _)| location.as_path().map(from_here))
        .collect();

    let mut items = Vec::new();
    for file in &files {
        if let Some(cuts) = sheets.get(file.as_path()) {
            items.extend(cuts.iter().map(|(location, span)| QueueItem {
                id: minting.mint(),
                location: location.clone(),
                span: Some(*span),
            }));
        } else if names_audio(file) && !cut_from.contains(&from_here(file)) {
            items.push(QueueItem::whole(minting.mint(), MediaLocation::local(file)));
        }
    }
    for inside in &folders {
        items.extend(folder_items(sources, inside, minting, deeper, walked));
    }
    items
}

fn names_a_sheet(location: &MediaLocation) -> bool {
    location
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(SHEET_EXTENSION))
}

fn sheet_items(sources: &Sources, path: &Path, minting: &mut Unclaimed) -> Vec<QueueItem> {
    sheet_cuts(sources, path)
        .into_iter()
        .map(|(location, span)| QueueItem {
            id: minting.mint(),
            location,
            span: Some(span),
        })
        .collect()
}

fn sheet_cuts(sources: &Sources, path: &Path) -> Vec<(MediaLocation, FrameSpan)> {
    let sheet = match read_cue_media(sources, &MediaLocation::local(path)) {
        Ok(sheet) => sheet,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "a cue sheet could not be read");
            return Vec::new();
        }
    };

    let mut cuts = Vec::new();
    for cut in &sheet.files {
        let Some(file) = path
            .parent()
            .and_then(|folder| the_file_a_cue_names(folder, &cut.named))
        else {
            tracing::warn!(sheet = %path.display(), file = %cut.named, "a cue sheet names a file that is not where it says");
            continue;
        };
        let location = MediaLocation::local(from_here(&file));
        let Ok(info) = probe(sources, &location) else {
            tracing::warn!(path = %file.display(), "a cue sheet names a file that will not probe");
            continue;
        };

        for (index, _) in cut.audio_tracks() {
            let Some(span) = cut.span_of(index, info.spec.rate, info.duration) else {
                continue;
            };
            cuts.push((location.clone(), span));
        }
    }
    cuts
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::{fs, process};

    use resonate_codec::{Media, MediaProvider};
    use resonate_core::SourceId;

    use super::*;
    use crate::{cli::RepeatArg, fixtures::silent_wave};

    #[test]
    fn a_pass_is_told_by_section_with_what_it_counted_and_nothing_it_did_not() {
        let summary = EnrichSummary {
            stats: EnrichStats {
                albums: 12,
                releases: 8,
                albums_unmatched: 3,
                by_isrc: 40,
                refused: 2,
                ..EnrichStats::default()
            },
            ..EnrichSummary::default()
        };

        assert_eq!(
            enriched(&summary),
            "Albums\n\
             \x20 12  asked about\n\
             \x20  8  matched to a release\n\
             \x20  3  with no strict match\n\
             Tracks\n\
             \x20 40  identified by their ISRC\n\
             Trouble\n\
             \x20  2  requests a service refused or answered unreadably\n"
        );
    }

    #[test]
    fn a_pass_that_asked_nothing_says_so_and_a_cancelled_one_says_that_too() {
        let summary = EnrichSummary {
            cancelled: true,
            ..EnrichSummary::default()
        };

        assert_eq!(enriched(&summary), "nothing was asked\ncancelled\n");
    }

    #[test]
    fn a_queue_that_finished_with_every_track_failed_is_no_success() {
        let track = resonate_core::TrackId::new(1).expect("an id");
        let failed = || Event::Failed {
            track,
            error: resonate_engine::Error::QueueEmpty,
        };

        let mut every_one_failed = Played::default();
        for event in [failed(), failed(), Event::QueueFinished] {
            every_one_failed.hears(&event);
        }
        let mut one_played = Played::default();
        for event in [failed(), Event::TrackStarted(track), Event::QueueFinished] {
            one_played.hears(&event);
        }
        let mut left_early = Played::default();
        left_early.hears(&failed());

        assert!(matches!(
            every_one_failed.played_anything(),
            Err(Error::NothingPlayed { failed: 2 })
        ));
        assert!(one_played.played_anything().is_ok());
        assert!(left_early.played_anything().is_ok());
    }

    fn wanted_at(tried: Option<SystemTime>, misses: u32) -> Want {
        Want {
            id: resonate_core::WantId::new(1).expect("an id"),
            release_track: resonate_core::ReleaseTrackId::new(1).expect("an id"),
            album: resonate_core::AlbumId::new(1).expect("an id"),
            album_title: "Heroes Tonight".to_owned(),
            title: "Heroes Tonight".to_owned(),
            artist: Some("Janji".to_owned()),
            recording: None,
            track: None,
            release: None,
            isrcs: Vec::new(),
            length: None,
            disc: 1,
            position: 1,
            wanted: SystemTime::UNIX_EPOCH,
            tried,
            offered: None,
            misses,
            held: None,
            links: Vec::new(),
            release_links: Vec::new(),
        }
    }

    #[test]
    fn a_want_says_how_it_is_getting_on_and_when_it_is_asked_again() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let tried = Some(now - Duration::from_secs(60));

        assert_eq!(standing_of(&wanted_at(None, 0), now), "due");
        assert_eq!(
            standing_of(&wanted_at(tried, 2), now),
            "no match 2 of 6, again in 1m"
        );
        assert_eq!(
            standing_of(&wanted_at(tried, 1), now),
            "no match 1 of 6, due"
        );
        assert_eq!(
            standing_of(&wanted_at(tried, TRIES_BEFORE_GIVING_UP), now),
            "gave up after 6 tries"
        );
        assert_eq!(
            standing_of(
                &Want {
                    offered: Some("file:///music/a.flac".to_owned()),
                    ..wanted_at(tried, 0)
                },
                now
            ),
            "offered, again in 5h 59m"
        );
        assert_eq!(
            standing_of(
                &Want {
                    held: Some(resonate_core::TrackId::new(3).expect("an id")),
                    ..wanted_at(tried, 0)
                },
                now
            ),
            "held"
        );
    }

    fn located(argument: &str) -> Option<MediaLocation> {
        queue_items(&[OsString::from(argument)])
            .first()
            .map(|item| item.location.clone())
    }

    #[cfg(feature = "sync")]
    fn driving(arguments: &[&str]) -> Option<resonate_sync::RemoteCommand> {
        let parsed = Cli::try_parse_from(
            ["resonate", "sync", "--control", "Desk"]
                .iter()
                .chain(arguments)
                .copied(),
        );
        let Ok(Cli {
            command: Some(Sub::Sync(args)),
            ..
        }) = parsed
        else {
            panic!("{arguments:?} was refused: {parsed:?}");
        };
        remote_command(&args).expect("a command")
    }

    #[cfg(feature = "sync")]
    #[test]
    fn another_device_is_driven_by_one_flag_at_a_time_and_rows_are_counted_from_one() {
        use resonate_sync::{Permille, RemoteCommand};

        let two_at_once = Cli::try_parse_from([
            "resonate",
            "sync",
            "--control",
            "Desk",
            "--jump",
            "2",
            "--press",
            "next",
        ]);
        let a_zone_without_a_device = Cli::try_parse_from(["resonate", "sync", "--zone", "2"]);

        assert_eq!(driving(&[]), None);
        assert_eq!(driving(&["--press", "toggle"]), Some(RemoteCommand::Toggle));
        assert_eq!(
            driving(&["--seek", "1:30"]),
            Some(RemoteCommand::Seek(Duration::from_secs(90)))
        );
        assert_eq!(
            driving(&["--set-volume", "40"]),
            Some(RemoteCommand::Volume(Permille::new(400)))
        );
        assert_eq!(driving(&["--jump", "1"]), Some(RemoteCommand::Jump(0)));
        assert_eq!(driving(&["--remove", "3"]), Some(RemoteCommand::Remove(2)));
        assert_eq!(
            driving(&["--move", "4", "--to", "1"]),
            Some(RemoteCommand::Move { from: 3, to: 0 })
        );
        assert!(two_at_once.is_err());
        assert!(a_zone_without_a_device.is_err());
    }

    #[test]
    fn a_profile_imported_for_a_device_is_a_command_the_grammar_takes() {
        let parsed = Cli::try_parse_from([
            "resonate",
            "eq",
            "--import",
            "profile.txt",
            "--for",
            "alsa_output.usb",
        ]);

        let Ok(Cli {
            command: Some(Sub::Eq(wanted)),
            ..
        }) = parsed
        else {
            panic!("--import beside --for was refused: {parsed:?}");
        };
        assert_eq!(wanted.r#for.as_deref(), Some("alsa_output.usb"));
    }

    #[cfg(feature = "ui")]
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

    #[cfg(feature = "ui")]
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

    #[test]
    fn a_file_uri_names_the_file_a_command_reads_and_a_path_is_taken_as_written() {
        assert_eq!(
            local_path(Path::new("file:///music/Pink%20Floyd/Echoes.flac")),
            Path::new("/music/Pink Floyd/Echoes.flac")
        );
        assert_eq!(
            local_path(Path::new("FILE://localhost/music/a.flac")),
            Path::new("/music/a.flac")
        );
        assert_eq!(
            local_path(Path::new("music/a.flac")),
            Path::new("music/a.flac")
        );
        assert_eq!(
            local_path(Path::new("subsonic:track/1")),
            Path::new("subsonic:track/1")
        );
    }

    #[test]
    fn a_flag_that_would_do_nothing_or_more_than_it_says_is_refused() {
        let parsed = |arguments: &[&str]| Cli::try_parse_from(arguments.iter().copied()).is_ok();

        assert!(!parsed(&["resonate", "playlist", "Road", "--reverse"]));
        assert!(!parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--export",
            "a.m3u",
            "--reverse"
        ]));
        assert!(parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--order",
            "title",
            "--reverse"
        ]));
        assert!(parsed(&[
            "resonate",
            "playlist",
            "Road",
            "--query",
            "floyd",
            "--reverse"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--off",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--on",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(!parsed(&[
            "resonate",
            "eq",
            "--suggest",
            "--for",
            "alsa_output.usb"
        ]));
        assert!(parsed(&["resonate", "eq", "--off"]));
        assert!(parsed(&["resonate", "eq", "--on"]));
    }

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

    #[test]
    fn a_uri_a_file_manager_passes_names_the_file_it_points_at() {
        let (uri, path) = if cfg!(windows) {
            (
                "file:///C:/music/Pink%20Floyd/Echoes.flac",
                r"\\?\C:\music\Pink Floyd\Echoes.flac",
            )
        } else {
            (
                "file:///music/Pink%20Floyd/Echoes.flac",
                "/music/Pink Floyd/Echoes.flac",
            )
        };
        assert_eq!(located(uri), Some(MediaLocation::local(path)));
    }

    #[test]
    fn an_argument_that_is_a_path_rather_than_a_uri_is_read_as_the_path_it_is() {
        let paths = if cfg!(windows) {
            [
                (
                    r"C:\music\Pink Floyd\Echoes.flac",
                    r"\\?\C:\music\Pink Floyd\Echoes.flac",
                ),
                (r"C:\music\a.flac", r"\\?\C:\music\a.flac"),
            ]
        } else {
            [
                (
                    "/music/Pink Floyd/Echoes.flac",
                    "/music/Pink Floyd/Echoes.flac",
                ),
                ("/music/a:b.flac", "/music/a:b.flac"),
            ]
        };
        for (argument, path) in paths {
            assert_eq!(located(argument), Some(MediaLocation::local(path)));
        }
    }

    #[test]
    fn an_argument_naming_a_file_from_here_is_read_as_the_file_it_names() {
        let here = env::current_dir().expect("a working directory");
        let here = resonate_core::path_text::verbatim(&here);

        assert_eq!(
            located("a.flac"),
            Some(MediaLocation::local(here.join("a.flac")))
        );
        assert_eq!(
            located("./sheets/../a.flac"),
            Some(MediaLocation::local(here.join("sheets/../a.flac"))),
            "a path was resolved past a folder that is not there"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_uri_names_the_same_file_a_path_through_the_same_link_does() {
        let folder = env::temp_dir().join(format!("resonate-uri-{}", process::id()));
        let real = folder.join("real");
        fs::create_dir_all(&real).expect("a scratch folder");
        fs::write(real.join("a.flac"), b"").expect("a file");
        let linked = folder.join("linked");
        symlink(&real, &linked).expect("a link to the folder");

        let through_the_link = linked.join("../linked/a.flac");
        let uri = MediaLocation::local(&through_the_link).to_uri();
        assert_eq!(
            located(&uri),
            located(through_the_link.to_str().expect("a UTF-8 path"))
        );
        assert_eq!(
            located(&uri),
            Some(MediaLocation::local(
                real.canonicalize().expect("the folder").join("a.flac")
            )),
            "a file:// argument kept the link it was named through"
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_uri_naming_frames_queues_that_cut_and_one_naming_no_span_queues_nothing() {
        let (uri, path, unreadable) = if cfg!(windows) {
            (
                "file:///C:/music/Meddle.flac#frames=588-44100",
                r"\\?\C:\music\Meddle.flac",
                "file:///C:/music/Meddle.flac#frames=100-50",
            )
        } else {
            (
                "file:///music/Meddle.flac#frames=588-44100",
                "/music/Meddle.flac",
                "file:///music/Meddle.flac#frames=100-50",
            )
        };
        let queued = queue_items(&[OsString::from(uri)]);
        assert_eq!(queued.len(), 1);
        assert_eq!(queued[0].location, MediaLocation::local(path));
        assert_eq!(
            queued[0].span,
            Some(FrameSpan::between(Frames(588), Frames(44_100))),
            "the cut a URI names was queued as the whole file"
        );

        assert!(
            queue_items(&[OsString::from(unreadable)]).is_empty(),
            "a cut that is no span was queued as the whole file"
        );
    }

    #[test]
    fn a_sheet_added_to_a_playlist_is_added_as_the_rows_it_cuts() {
        let folder = env::temp_dir().join(format!("resonate-extend-{}", process::id()));
        fs::create_dir_all(&folder).expect("a scratch folder");
        fs::write(folder.join("whole.wav"), analyse::tests::silent_wave()).expect("a wave file");
        let sheet = folder.join("sheet.cue");
        fs::write(
            &sheet,
            "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 \
             AUDIO\n    INDEX 01 00:00:30\n",
        )
        .expect("a sheet");
        let library = Library::open(&folder.join("library.db")).expect("a catalog");

        extend(&library, "Mixtape", &[sheet]).expect("the sheet is added");

        let playlist = library
            .playlist_named("Mixtape")
            .expect("a read")
            .expect("the playlist was started");
        let cuts = library.playlist_cuts(playlist.id).expect("a read");
        assert_eq!(cuts.len(), 2, "a sheet was stored as one row");
        assert!(cuts.iter().all(|cut| cut.span.is_some()
            && cut.location.as_path() == Some(from_here(&folder.join("whole.wav")).as_path())));

        drop(library);
        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_folder_is_queued_as_the_audio_inside_it_and_its_sheet_as_the_rows_it_cuts() {
        let folder = env::temp_dir().join(format!("resonate-queued-folder-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("Album/CD2")).expect("a scratch folder");
        for name in [
            "Album/02.wav",
            "Album/01.wav",
            "Album/CD2/01.wav",
            "Album/.hidden.wav",
        ] {
            fs::write(folder.join(name), analyse::tests::silent_wave()).expect("a wave file");
        }
        fs::write(folder.join("Album/notes.txt"), b"words").expect("a note");
        fs::create_dir_all(folder.join("Rip")).expect("a scratch folder");
        fs::write(folder.join("Rip/whole.wav"), analyse::tests::silent_wave()).expect("a wave");
        fs::write(
            folder.join("Rip/whole.cue"),
            "FILE \"whole.wav\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n  TRACK 02 \
             AUDIO\n    INDEX 01 00:00:30\n",
        )
        .expect("a sheet");
        let named = |items: Vec<QueueItem>| {
            items
                .into_iter()
                .map(|item| {
                    (
                        item.location.as_path().map(Path::to_path_buf),
                        item.span.is_some(),
                    )
                })
                .collect::<Vec<_>>()
        };

        let album = named(queue_items(&[folder.join("Album").into_os_string()]));
        let rip = named(queue_items(&[folder.join("Rip").into_os_string()]));

        let at = |name: &str| Some(from_here(&folder.join(name)));
        assert_eq!(
            album,
            [
                (at("Album/01.wav"), false),
                (at("Album/02.wav"), false),
                (at("Album/CD2/01.wav"), false)
            ]
        );
        assert_eq!(
            rip,
            [(at("Rip/whole.wav"), true), (at("Rip/whole.wav"), true)]
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_linked_back_to_itself_or_to_a_sibling_is_queued_once() {
        let folder = env::temp_dir().join(format!("resonate-queued-loop-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("Music/Album")).expect("a scratch folder");
        fs::write(
            folder.join("Music/Album/01.wav"),
            analyse::tests::silent_wave(),
        )
        .expect("a wave");
        std::os::unix::fs::symlink("..", folder.join("Music/Album/Backup")).expect("a link up");
        std::os::unix::fs::symlink("Album", folder.join("Music/Again")).expect("a sibling link");
        std::os::unix::fs::symlink(".", folder.join("Music/Here")).expect("a link to itself");

        let queued = queue_items(&[folder.join("Music").into_os_string()]);

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
        assert_eq!(queued.len(), 1);
    }

    #[test]
    fn a_playlist_is_queued_as_the_rows_it_lists() {
        let folder = env::temp_dir().join(format!("resonate-queued-list-{}", process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("a scratch folder");
        let folder = folder
            .canonicalize()
            .expect("the scratch folder is canonical");
        for name in ["one.wav", "two.wav"] {
            fs::write(folder.join(name), analyse::tests::silent_wave()).expect("a wave file");
        }
        fs::write(folder.join("mix.m3u8"), "#EXTM3U\ntwo.wav\none.wav\n").expect("a list");

        let queued: Vec<_> = queue_items(&[folder.join("mix.m3u8").into_os_string()])
            .into_iter()
            .map(|item| item.location.as_path().map(Path::to_path_buf))
            .collect();

        assert_eq!(
            queued,
            [Some(folder.join("two.wav")), Some(folder.join("one.wav"))]
        );

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_sheet_finds_its_audio_as_the_scan_does_whatever_case_or_extension_it_wrote() {
        let folder = env::temp_dir().join(format!("resonate-sheet-names-{}", process::id()));
        fs::create_dir_all(folder.join("CD1")).expect("a scratch folder");
        fs::write(folder.join("album.wav"), analyse::tests::silent_wave()).expect("a wave file");
        fs::write(folder.join("CD1/01.wav"), analyse::tests::silent_wave()).expect("a wave file");
        let one_track = |named: &str| {
            format!("FILE \"{named}\" WAVE\n  TRACK 01 AUDIO\n    INDEX 01 00:00:00\n")
        };

        for (written, found) in [
            ("ALBUM.WAV", "album.wav"),
            ("album.flac", "album.wav"),
            ("CD1\\01.wav", "CD1/01.wav"),
        ] {
            let sheet = folder.join("sheet.cue");
            fs::write(&sheet, one_track(written)).expect("a sheet");

            let cuts = sheet_cuts(&Sources::local(), &sheet);

            assert_eq!(cuts.len(), 1, "{written} found nothing");
            assert_eq!(
                cuts[0].0.as_path(),
                Some(from_here(&folder.join(found)).as_path()),
                "{written}"
            );
        }

        fs::remove_dir_all(&folder).expect("the scratch folder goes");
    }

    #[test]
    fn a_queued_row_is_named_by_a_uri_that_reads_back_as_the_same_row() {
        let queued = located("a.flac").expect("one argument makes one row");

        assert_eq!(MediaLocation::from_uri(&queued.to_uri()), Some(queued));
    }

    #[test]
    fn an_argument_naming_a_source_this_build_does_not_hold_is_a_file_from_here() {
        let here = env::current_dir().expect("a working directory");
        let here = resonate_core::path_text::verbatim(&here);

        for named in ["01:intro.flac", "subsonic:track/1"] {
            assert_eq!(located(named), Some(MediaLocation::local(here.join(named))));
        }
    }

    struct Elsewhere(SourceId);

    impl MediaProvider for Elsewhere {
        fn source(&self) -> &SourceId {
            &self.0
        }

        fn open(&self, location: &MediaLocation) -> resonate_codec::Result<Media> {
            Err(resonate_codec::Error::LocatorNotUsable {
                location: location.clone(),
            })
        }
    }

    #[test]
    fn a_uri_naming_a_source_this_build_holds_keeps_the_source_that_named_it() {
        let subsonic = SourceId::new("subsonic").expect("a source name");
        let sources = Sources::local().and(Arc::new(Elsewhere(subsonic)));

        assert_eq!(
            Some(location_of_argument(
                OsStr::new("subsonic:track/1"),
                &sources
            )),
            MediaLocation::from_uri("subsonic:track/1")
        );
    }

    fn parsed(arguments: &[&str]) -> Cli {
        Cli::try_parse_from(arguments.iter().copied())
            .unwrap_or_else(|error| panic!("{arguments:?} was refused: {error}"))
    }

    fn transport_of(arguments: &[&str]) -> TransportArgs {
        match parsed(arguments).command {
            Some(Sub::Play { transport, .. }) => transport,
            other => panic!("{arguments:?} is not play: {other:?}"),
        }
    }

    fn playlist_transport_of(arguments: &[&str]) -> TransportArgs {
        match parsed(arguments).command {
            Some(Sub::Playlist(wanted)) => wanted.transport,
            other => panic!("{arguments:?} is not playlist: {other:?}"),
        }
    }

    fn scan_of(arguments: &[&str]) -> ScanOptions {
        match parsed(arguments).command {
            Some(Sub::Scan(wanted)) => scan_options(&wanted, NonZeroUsize::MIN),
            other => panic!("{arguments:?} is not scan: {other:?}"),
        }
    }

    #[test]
    fn bit_perfect_is_asked_for_or_refused_for_one_run_and_never_both() {
        let asked = |arguments: &[&str]| bit_perfect_asked(&parsed(arguments));
        let refused = |arguments: &[&str]| {
            matches!(
                one_way_on_bit_perfect(&parsed(arguments)),
                Err(Error::BitPerfectBothWays)
            )
        };

        assert_eq!(asked(&["resonate", "sinks"]), None);
        assert_eq!(asked(&["resonate", "--bit-perfect", "sinks"]), Some(true));
        assert_eq!(asked(&["resonate", "sinks", "--bit-perfect"]), Some(true));
        assert_eq!(
            asked(&["resonate", "sinks", "--no-bit-perfect"]),
            Some(false)
        );
        assert!(!refused(&["resonate", "sinks", "--bit-perfect"]));
        assert!(refused(&[
            "resonate",
            "--no-bit-perfect",
            "--bit-perfect",
            "sinks"
        ]));
        assert!(refused(&[
            "resonate",
            "--no-bit-perfect",
            "sinks",
            "--bit-perfect"
        ]));
        assert!(refused(&[
            "resonate",
            "--bit-perfect",
            "sinks",
            "--no-bit-perfect"
        ]));
    }

    #[test]
    fn play_takes_shuffle_repeat_and_a_volume_for_the_run() {
        let bare = transport_of(&["resonate", "play", "a.flac"]);
        let set = transport_of(&[
            "resonate",
            "play",
            "a.flac",
            "--shuffle",
            "--repeat",
            "track",
            "--volume",
            "40",
        ]);
        let refused = |volume: &str| {
            Cli::try_parse_from(["resonate", "play", "a.flac", "--volume", volume]).is_err()
        };

        assert!(!bare.shuffle);
        assert_eq!(bare.repeat, None);
        assert_eq!(bare.volume, None);
        assert!(set.shuffle);
        assert_eq!(set.repeat, Some(RepeatArg::Track));
        assert_eq!(set.volume, Some(40));
        assert!(refused("101"));
        assert!(refused("-1"));
        assert!(!refused("0"));
        assert!(!refused("100"));
        assert_eq!(at_percent(40).ok(), Volume::new(0.4).ok());
        assert_eq!(at_percent(100).ok(), Some(Volume::MAX));
    }

    #[test]
    fn a_playlist_played_takes_the_transport_and_one_edited_refuses_it() {
        let set = playlist_transport_of(&[
            "resonate",
            "playlist",
            "Evening",
            "--matching",
            "floyd",
            "--shuffle",
            "--repeat",
            "queue",
            "--volume",
            "25",
        ]);
        let refused = |arguments: &[&str]| Cli::try_parse_from(arguments.iter().copied()).is_err();

        assert!(set.shuffle);
        assert_eq!(set.repeat, Some(RepeatArg::Queue));
        assert_eq!(set.volume, Some(25));
        assert!(refused(&[
            "resonate",
            "playlist",
            "Evening",
            "--tidy",
            "--shuffle"
        ]));
        assert!(refused(&[
            "resonate", "playlist", "Evening", "--pin", "--volume", "10"
        ]));
        assert!(refused(&[
            "resonate",
            "playlist",
            "Evening",
            "--matching",
            "x",
            "--drop",
            "--repeat",
            "track"
        ]));
        assert!(!refused(&["resonate", "playlist", "Evening", "--shuffle"]));
    }

    #[test]
    fn the_transport_is_set_before_the_queue_loads_so_the_first_track_is_already_shuffled() {
        let set = transport_of(&[
            "resonate",
            "play",
            "a.flac",
            "--shuffle",
            "--repeat",
            "queue",
        ]);
        let bare = transport_of(&["resonate", "play", "a.flac"]);

        let commands = set_out(&set, Vec::new(), 3);
        let loaded = set_out(&bare, Vec::new(), 0);

        assert_eq!(
            commands,
            vec![
                Command::SetShuffle(true),
                Command::SetRepeat(RepeatMode::Queue),
                Command::Load {
                    items: Vec::new(),
                    start_at: 3,
                    autoplay: true,
                },
            ]
        );
        assert_eq!(
            loaded,
            vec![Command::Load {
                items: Vec::new(),
                start_at: 0,
                autoplay: true,
            }]
        );
        assert_eq!(anywhere_in(0), 0);
        assert!(anywhere_in(7) < 7);
    }

    #[test]
    fn a_scan_reads_every_file_and_follows_links_only_where_asked() {
        let usual = scan_of(&["resonate", "scan", "/music"]);
        let asked = scan_of(&["resonate", "scan", "--full", "--follow-links", "/music"]);

        assert!(usual.incremental);
        assert!(!usual.follow_symlinks);
        assert_eq!(usual.roots, vec![PathBuf::from("/music")]);
        assert!(!asked.incremental);
        assert!(asked.follow_symlinks);
        assert_eq!(asked.roots, vec![PathBuf::from("/music")]);
    }

    #[test]
    fn forgetting_a_folder_inside_a_root_drops_the_tracks_gone_from_it_and_keeps_the_rest() {
        let root = env::temp_dir().join(format!("resonate-forget-{}", process::id()));
        let retired = root.join("retired");
        let kept = root.join("kept");
        fs::create_dir_all(&retired).expect("a scratch folder");
        fs::create_dir_all(&kept).expect("a scratch folder");
        fs::write(retired.join("a.wav"), silent_wave(1)).expect("a track");
        fs::write(retired.join("b.wav"), silent_wave(1)).expect("a track");
        fs::write(kept.join("c.wav"), silent_wave(1)).expect("a track");
        let library = Library::open_in_memory().expect("a catalog");
        library.add_root(&root).expect("a root");

        let scanned = library
            .scan(ScanOptions {
                roots: vec![root.clone()],
                incremental: true,
                follow_symlinks: false,
                extract_cover_art: false,
                workers: NonZeroUsize::MIN,
            })
            .and_then(PassHandle::join)
            .expect("a scan");
        fs::remove_file(retired.join("a.wav")).expect("a track taken away");
        fs::remove_file(kept.join("c.wav")).expect("a track taken away");
        let from_the_folder = forgotten(&library, &retired).expect("a forget");
        let from_nowhere = forgotten(&library, &root.join("never")).expect("a forget");
        let left = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the tracks");
        let the_root = forgotten(&library, &root).expect("a forget");
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(scanned.stats.added, 3);
        assert_eq!(from_the_folder, Forgotten::Gone(1));
        assert_eq!(from_nowhere, Forgotten::Nothing);
        assert_eq!(
            left.len(),
            2,
            "a present file or one outside the folder lost its row"
        );
        assert_eq!(the_root, Forgotten::Root);
    }

    #[test]
    fn a_pass_no_signal_interrupts_answers_its_summary_through_until_told() {
        let root = fixtures::scratch("until-told");
        fs::write(root.join("a.wav"), silent_wave(1)).expect("a track");
        let library = Library::open_in_memory().expect("a catalog");
        library.add_root(&root).expect("a root");

        let summary = until_told(
            library
                .scan(fixtures::scan_of(&root))
                .expect("a scan under way"),
        )
        .expect("a summary");
        fs::remove_dir_all(&root).expect("the scratch folder removed");

        assert_eq!(summary.stats.added, 1);
        assert!(!summary.cancelled);
    }

    #[test]
    fn a_pass_whose_files_failed_answers_an_error_so_the_command_exits_1() {
        assert!(none_failed(PassKind::Organise, 0).is_ok());
        assert!(matches!(
            none_failed(PassKind::Retag, 3),
            Err(Error::FilesFailed {
                pass: PassKind::Retag,
                failed: 3
            })
        ));
    }

    #[test]
    fn a_cancelled_pass_answers_an_error_so_the_command_exits_1() {
        assert!(finished(PassKind::Scan, false).is_ok());
        assert!(matches!(
            finished(PassKind::Scan, true),
            Err(Error::Cancelled {
                pass: PassKind::Scan
            })
        ));
        assert!(matches!(
            finished(PassKind::Import, true),
            Err(Error::Cancelled {
                pass: PassKind::Import
            })
        ));
    }
}
