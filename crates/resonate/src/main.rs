#![deny(clippy::print_stdout, clippy::print_stderr)]

#[macro_use]
mod said;

mod analyse;
mod cli;
mod config;
mod discord;
mod equaliser;
mod error;
mod favourites;
mod info;
mod input;
mod lasting;
#[cfg(feature = "ui")]
mod launcher;
mod listen;
mod mcp;
mod mpris;
mod online;
mod playlists;
mod providers;
mod readout;
#[cfg(feature = "ui")]
mod settings;
mod share;
mod signals;
mod sleep;
mod stats;
mod studies;
mod submitting;
mod suggest;
mod table;
mod vault;
mod vocabulary;

use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::{OsStr, OsString},
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
use resonate_codec::{CoverArt, Packing, Popularity, Sources, probe, read_cue_media};
#[cfg(feature = "ui")]
use resonate_core::Resumption;
use resonate_core::{
    AlbumId, FrameSpan, Frames, MediaLocation, PlaylistId, SampleRate, StreamSpec, Volume,
};
use resonate_engine::{
    BluetoothWake, Command, Counting, Decoded, EngineConfig, Event, Impulse, Keep, Keeping,
    Levelling, Listening, OutputPlan, Placement, Player, QueueItem, RepeatMode, Unclaimed, Until,
    plan_output, read_impulse, resolve_replay_gain, stamp_of,
};
use resonate_library::{
    Aged, Cancelling, Cut, Direction, EnrichOptions, EnrichSummary, Failure, Failures, FileTags,
    HistoryKept, Kept, Layout, Library, Listen, LookupOp, Missing, MissingTrack, Move,
    OrganiseOptions, OrganiseSummary, PassHandle, PassKind, Playing, Playlist, PlaylistName,
    PlaylistOrder, PollOptions, Refusal, Refused, RetagOptions, RetagSummary, RowOrder, SavedQuery,
    ScanOptions, Search, SortOrder, StudyFilter, UnheldRelease, Vault, VaultFiles, Want,
    folded_letters,
};
use resonate_mpris::{PlayerName, Queueing, Running, Standing};
use resonate_pipewire::{
    HardwareVolume, NodeName, PipeWire, Plugged, SinkFormats, SinkInfo, Words,
};
use resonate_providers::Providers;
use tracing_subscriber::{
    EnvFilter, filter::ParseError, fmt, layer::SubscriberExt as _, util::SubscriberInitExt as _,
};

use crate::{
    cli::{Cli, PlaylistArgs, PlaylistOrderArg, QueueArgs, ScanArgs, Sub, TransportArgs},
    config::Config,
    error::{ConfigKey, Error, Result, ValueKind},
    info::bytes_text,
    input::{Action, Pressed},
    readout::Readout,
    table::Table,
};

const SINK_TIMEOUT: Duration = Duration::from_secs(2);

const HEARD_SAMPLE: Duration = Duration::from_millis(500);

const COMMAND_TIMEOUT: Duration = Duration::from_secs(3);

const LOG_FILTER: &str = "RESONATE_LOG";

const DEFAULT_LOG: &str = "warn,resonate=info,symphonia=off";

const WITHIN_THE_MINUTE: &str = "just now";

const COVER_ART: &str = "cover art";
const TAKEN_AWAY: &str = "(taken away)";
const RATING: &str = "rating";

fn main() -> ExitCode {
    match run() {
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

fn init_logging() -> Result<()> {
    let (filter, refused) = wanted_filter();
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(io::stderr))
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
    init_logging()?;
    let config = config::load(cli.config.as_deref())?;

    match &cli.command {
        Some(Sub::Sinks) => list_sinks(),
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
        Some(Sub::Wants) => wants(&open_library(&cli, &config)?),
        Some(Sub::Missing {
            artist,
            read_the_rest,
            bring_back,
        }) => {
            let library = open_library(&cli, &config)?;
            if *bring_back {
                said!("{}", brought_back(library.bring_back_dismissed()?));
            }
            if *read_the_rest && let Some(named) = artist.as_deref() {
                read_the_rest_of(&library, &config, named)?;
            }
            missing(&library, artist.as_deref())
        }
        Some(Sub::Poll { again }) => {
            let held = vault_already_kept(&cli, &config);
            poll(
                &open_library_with(&cli, &config, held.as_ref())?,
                providers::registered(&config),
                *again,
            )
        }
        Some(Sub::Forget { roots }) => forget(&open_library(&cli, &config)?, roots),
        Some(Sub::Tag { root, apply, undo }) => {
            tag(&open_library(&cli, &config)?, root, *apply, *undo)
        }
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
        Some(Sub::Playlists {
            order,
            reverse,
            named,
        }) => list_playlists(
            &open_library(&cli, &config)?,
            *order,
            *reverse,
            named.as_deref(),
        ),
        Some(Sub::Playlist(wanted)) => playlist(&cli, &config, wanted),
        Some(Sub::Eq(wanted)) => equaliser::run(&cli, &config, wanted),
        Some(Sub::Import { files, name }) => {
            import(&open_library(&cli, &config)?, files, name.as_deref())
        }
        Some(Sub::Sleep { spec, player }) => sleep::set(spec, player.as_deref()),
        Some(Sub::Stats { window, top }) => {
            stats::print(&open_library(&cli, &config)?, *window, top.get())
        }
        Some(Sub::Favourites(wanted)) => favourites::print(&open_library(&cli, &config)?, wanted),
        Some(Sub::Suggest { save }) => {
            suggest::print(&open_library(&cli, &config)?, save.as_deref())
        }
        Some(Sub::Share { file }) => share::print(
            &open_library(&cli, &config)?,
            online::reference(&config).as_deref(),
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

fn playing_from(library: Option<&Library>) -> Arc<Sources> {
    let Some(library) = library else {
        return Arc::new(Sources::local());
    };
    Arc::new(library.sources().hinted_by(library.hinting()))
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
    age_the_history(&library, config.history_kept());
    Ok(library)
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
            .ok_or(Error::Sink(resonate_pipewire::Error::NoSink));
    };
    sinks
        .into_iter()
        .find(|sink| sink.name == *wanted)
        .ok_or_else(|| Error::SinkNotFound(wanted.clone()))
}

fn confirm_sink(player: &Player, wanted: Option<&NodeName>) {
    let Some(wanted) = wanted else {
        return;
    };
    if player.sinks().iter().any(|sink| sink.name == *wanted) {
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

fn list_sinks() -> Result<()> {
    let pipewire = PipeWire::start("Resonate")?;
    let sinks = pipewire.enumerate_sinks(SINK_TIMEOUT)?;

    let mut table = Table::new(vec![
        "",
        "DEVICE",
        "NODE NAME",
        "DRIVEN BY",
        "PROFILE",
        "PORT",
        "FORMAT",
        "ADVERTISES",
        "GRAPH",
        "NOW",
    ]);

    for sink in &sinks {
        let marker = if sink.is_default { "*" } else { "" };
        let current = sink
            .current_rate
            .map_or_else(|| "unknown".to_owned(), |rate: SampleRate| rate.to_string());

        let mut formats = sink.formats.iter();
        let first = formats.next();
        table.push(vec![
            marker.to_owned(),
            sink.description.clone(),
            sink.name.to_string(),
            driven_by(sink).to_owned(),
            sink.profile
                .as_ref()
                .map_or_else(|| "none".to_owned(), |profile| profile.description.clone()),
            comes_out_of(sink),
            first.map_or_else(|| "none".to_owned(), SinkFormats::spelled),
            first.map_or_else(|| "none".to_owned(), |entry| rates(&entry.rates)),
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
    pipewire.shutdown()?;
    Ok(())
}

const fn driven_by(sink: &SinkInfo) -> &'static str {
    if sink.is_hardware {
        "a device"
    } else {
        "the graph"
    }
}

fn comes_out_of(sink: &SinkInfo) -> String {
    let Some(port) = sink.port.as_ref() else {
        return "none".to_owned();
    };
    let plugged = match port.plugged {
        Plugged::Unsaid => port.description.clone(),
        Plugged::No => format!("{} (nothing plugged in)", port.description),
        Plugged::Yes => format!("{} (plugged in)", port.description),
    };
    let volume = match port.hardware_volume {
        HardwareVolume::Unsaid => plugged,
        HardwareVolume::No => format!("{plugged}, volume in software"),
        HardwareVolume::Yes => format!("{plugged}, volume on the hardware"),
    };
    let level = match port.volume {
        Some(gain) => format!("{volume} at {}", Volume::heard_at(gain)),
        None => volume,
    };
    if port.muted {
        format!("{level}, muted")
    } else {
        level
    }
}

fn scan(library: &Library, config: &Config, wanted: &ScanArgs) -> Result<()> {
    let roots = &wanted.roots;
    for root in roots {
        if !root.is_dir() {
            return Err(Error::MissingLibraryRoot { path: root.clone() });
        }
        library.add_root(root)?;
    }

    let workers = thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
    let handle = library.scan(scan_options(wanted, workers))?;

    let summary = until_told(handle)?;
    let stats = summary.stats;
    said!(
        "discovered {} | added {} | updated {} | moved {} | removed {} | failed {}{}",
        stats.discovered,
        stats.added,
        stats.updated,
        stats.moved,
        stats.removed,
        stats.failed.total(),
        broken_down(stats.failed)
    );
    if summary.cancelled {
        said!("cancelled");
    }
    finished(PassKind::Scan, summary.cancelled)?;

    if config.enriches_after_scan()
        && let Some(reference) = online::reference(config)
    {
        let summary = until_told(library.enrich(
            reference,
            Arc::new(online::fingerprinters(
                config,
                Arc::new(library.sources()),
                &online::by_sound(config),
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
    Ok(())
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
    let reference = online::reference_asked_for(config)?;
    let options = carrying_on(library, options)?;
    let summary = until_told(library.enrich(
        reference,
        Arc::new(online::fingerprinters(
            config,
            Arc::new(library.sources()),
            &online::by_sound(config),
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
    said!("carrying on the lookup a run left unfinished");

    Ok(EnrichOptions {
        refresh: left.refresh,
        ..options
    })
}

fn enriched(summary: &EnrichSummary) -> String {
    let stats = &summary.stats;
    let mut told = format!(
        "albums {} | releases {} | matched {} | covers {} | tracks {} | named {} | artists {} \
         | portraits {} | releases found {} | refused {}\n\
         studied {} | fakes {} | recognised {} | misnamed {} | lyrics {}\n",
        stats.albums,
        stats.releases,
        stats.matched,
        stats.covers,
        stats.tracks,
        stats.named,
        stats.artists,
        stats.portraits,
        stats.releases_found,
        stats.refused,
        stats.studied,
        stats.fakes,
        stats.recognised,
        stats.misnamed,
        stats.lyrics
    );
    if let Some(op) = summary.stopped_by {
        told.push_str(&format!(
            "stopped: the reference could not be reached while asking for {}\n",
            asked_for(op)
        ));
    }
    if summary.cancelled {
        told.push_str("cancelled\n");
    }
    told
}

const fn asked_for(op: LookupOp) -> &'static str {
    match op {
        LookupOp::Release => "a release",
        LookupOp::FindRelease => "a release search",
        LookupOp::Recording => "a recording",
        LookupOp::Isrc => "a recording by its ISRC",
        LookupOp::FindRecording => "a recording search",
        LookupOp::ReleaseGroup => "a release group",
        LookupOp::FindReleaseGroup => "a release group search",
        LookupOp::Artist => "an artist",
        LookupOp::FindArtist => "an artist search",
        LookupOp::ReleaseGroupsOfArtist => "the releases of an artist",
        LookupOp::Cover => "a cover",
        LookupOp::Portrait => "a portrait",
        LookupOp::Lyrics => "lyrics",
        LookupOp::Devices => "the measured devices",
        LookupOp::Correction => "a measured correction",
        LookupOp::Recognise => "a recognition",
        LookupOp::Submit => "a submission of what was heard",
        LookupOp::Love => "a favourite told to a listening service",
        LookupOp::Token => "a check of a listening service's token",
        LookupOp::StreamLink => "a look for where a track streams",
    }
}

fn wants(library: &Library) -> Result<()> {
    let wanted = library.wants()?;
    if wanted.is_empty() {
        said!("nothing is wanted");
        return Ok(());
    }

    let mut table = Table::new(vec![
        "TITLE", "ARTIST", "ALBUM", "WANTED", "TRIED", "OFFERED", "LINKS",
    ]);
    for want in &wanted {
        table.push(vec![
            want.title.clone(),
            want.artist.clone().unwrap_or_default(),
            want.album_title.clone(),
            ago(Some(want.wanted)),
            ago(want.tried),
            want.offered.clone().unwrap_or_else(|| "-".to_owned()),
            linked_through(want),
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
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

fn read_the_rest_of(library: &Library, config: &Config, named: &str) -> Result<()> {
    let reference = online::reference_asked_for(config)?;
    let Some(artist) = library.artist_named(named)? else {
        return Ok(());
    };
    let read = library.read_the_rest_of(artist, reference.as_ref())?;
    said!(
        "read {} more of {named}'s discography",
        counted(read as u64, "release", "releases")
    );
    Ok(())
}

fn dismissed_beside(dismissed: Missing) -> String {
    match dismissed.tracks + dismissed.releases {
        0 => String::new(),
        many => format!(" · {many} dismissed, --bring-back lists them again"),
    }
}

fn brought_back(brought: Missing) -> String {
    format!(
        "brought back {} and {}",
        counted(brought.tracks, "dismissed track", "dismissed tracks"),
        counted(brought.releases, "dismissed release", "dismissed releases")
    )
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
            None => said!("nothing is missing"),
            Some(named) => said!("nothing of {named} is missing"),
        }
        return Ok(());
    }

    let discs_of = discs_per_album(&tracks);
    let dismissed = match artist {
        None => library.dismissed()?,
        Some(_) => Missing::default(),
    };
    said!(
        "{} missing across {} · {} not held{}",
        counted(tracks_missing, "track", "tracks"),
        counted(discs_of.len() as u64, "album", "albums"),
        counted(releases_unheld, "release", "releases"),
        dismissed_beside(dismissed)
    );

    said!();
    said!("TRACKS MISSING");
    if tracks.is_empty() {
        said!("every release track the catalog knows of is held");
    } else {
        said_on!("{}", missing_tracks_table(&tracks, &discs_of).render());
    }

    said!();
    said!("RELEASES NOT HELD");
    if releases.is_empty() {
        said!("every release of a held artist is held");
    } else {
        said_on!("{}", unheld_releases_table(&releases).render());
    }
    if let Some(named) = artist
        && let Some(id) = library.artist_named(named)?
        && let Some(detail) = library.artist_detail(id)?
        && detail.releases_unread > 0
    {
        said!(
            "MusicBrainz credits {named} on {} more that were not read",
            counted(
                u64::from(detail.releases_unread),
                "release group",
                "release groups"
            )
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
        "ALBUM", "ARTIST", "DISC", "#", "TITLE", "LENGTH", "WANTED",
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
            if track.want.is_some() { "yes" } else { "-" }.to_owned(),
        ]);
    }
    table
}

fn unheld_releases_table(releases: &[UnheldRelease]) -> Table {
    let mut table = Table::new(vec!["ARTIST", "RELEASE", "KIND", "FIRST RELEASED"]);
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

fn poll(library: &Library, providers: Providers, again: bool) -> Result<()> {
    if !providers.has_a_source() {
        said!("no provider is registered");
    }
    let options = if again {
        PollOptions::ASKING_EVERY_WANT
    } else {
        PollOptions::default()
    };
    let summary = until_told(library.poll(Arc::new(providers), options)?)?;
    let stats = summary.stats;
    said!(
        "asked {} | offered {} | kept {} | unkept {} | nothing {} | refused {} | late {}",
        stats.asked,
        stats.offered,
        stats.kept,
        stats.unkept,
        stats.nothing,
        stats.refused,
        stats.late
    );
    if summary.cancelled {
        said!("cancelled");
    }
    finished(PassKind::Poll, summary.cancelled)
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
            Forgotten::Root => said!("forgot {}", argument.display()),
            Forgotten::Delivered(delivered) => {
                said!("forgot the delivered {}", delivered.display());
            }
            Forgotten::Gone(1) => said!("forgot 1 track gone from {}", argument.display()),
            Forgotten::Gone(tracks) => {
                said!("forgot {tracks} tracks gone from {}", argument.display());
            }
            Forgotten::Nothing => said!(
                "{} was neither a library root, a delivered track nor a folder holding a track \
                 that is gone",
                argument.display()
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
    finished(PassKind::Retag, summary.cancelled)
}

fn pictured(picture: &CoverArt) -> String {
    format!(
        "{} · {}",
        picture.format.extension().to_uppercase(),
        bytes_text(picture.bytes.len() as u64)
    )
}

fn rated(popularity: Popularity) -> String {
    let plays = match popularity.plays {
        1 => "1 play".to_owned(),
        plays => format!("{plays} plays"),
    };
    if popularity.favourite {
        format!("favourite · {plays}")
    } else {
        format!("not a favourite · {plays}")
    }
}

fn tagged(summary: &RetagSummary, roots: &[PathBuf], apply: bool) -> String {
    let retagging = &summary.retagging;
    if retagging.writes.is_empty() && retagging.passed_over.is_empty() {
        return "nothing to tag\n".to_owned();
    }

    let mut told = String::new();
    if !retagging.writes.is_empty() {
        let mut table = Table::new(vec!["FILE", "FIELD", "VALUE"]);
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
                    TAKEN_AWAY.to_owned(),
                ]);
            }
            if let Some(picture) = &write.picture {
                table.push(vec![
                    mem::take(&mut named),
                    COVER_ART.to_owned(),
                    pictured(picture),
                ]);
            }
            if write.unpictured {
                table.push(vec![
                    mem::take(&mut named),
                    COVER_ART.to_owned(),
                    TAKEN_AWAY.to_owned(),
                ]);
            }
            if let Some(popularity) = write.popularity {
                table.push(vec![
                    mem::take(&mut named),
                    RATING.to_owned(),
                    rated(popularity),
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
        format!(
            "written {} | fields {} | pictures {} | ratings {}",
            stats.written, stats.fields, stats.pictures, stats.ratings
        )
    } else {
        format!(
            "would write {} | fields {} | pictures {} | ratings {}",
            retagging.writes.len(),
            retagging
                .writes
                .iter()
                .map(|write| write.edits.len() + write.taken.len())
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
                .count()
        )
    };

    told.push_str(&format!(
        "{written} | already said {} | passed over {} | walked {}\n",
        stats.unchanged, stats.passed_over, stats.walked
    ));
    if !apply {
        told.push_str("nothing was written; pass --apply to do it\n");
    }
    if summary.cancelled {
        told.push_str("cancelled\n");
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
        said!("no applied run is kept to put back");
        return Ok(());
    }
    said_on!("{}", organised(&summary, &library.roots()?, how.apply));
    finished(PassKind::Organise, summary.cancelled)
}

fn until_told<Progress, Summary>(handle: PassHandle<Progress, Summary>) -> Result<Summary>
where
    Progress: Cancelling + 'static,
{
    let progress = Arc::clone(handle.progress());
    let _interrupting = signals::cancel_when_told(move || progress.cancel());
    Ok(handle.join()?)
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
        return "nothing to organise\n".to_owned();
    }

    let mut told = String::new();
    if !plan.moves.is_empty() {
        let mut table = Table::new(vec!["FROM", "TO"]);
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
            format!("moved {}", stats.moved),
            format!("folders pruned {}", stats.pruned),
        )
    } else {
        (
            format!("would move {}", plan.files_moving()),
            format!("folders to empty {}", plan.folders.len()),
        )
    };

    told.push_str(&format!(
        "{moved} | in place {} | unidentified {} | collided {} | failed {} | {folders}\n",
        stats.unchanged, stats.unidentified, stats.collided, stats.failed
    ));
    if !apply {
        told.push_str("nothing was moved; pass --apply to do it\n");
    }
    if summary.cancelled {
        told.push_str("cancelled\n");
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
            format!("{standing}: {why} {}", under_its_root(with, roots))
        }
        Refusal::Unmoved { kind } => format!("{standing}: {why} ({kind})"),
        _ => format!("{standing}: {why}"),
    }
}

const fn stands_because(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::Unidentified => "nothing names an album to file it under",
        Refusal::Loose => "it would land loose in the root",
        Refusal::Collided { .. } => "it would collide with",
        Refusal::SharesASheet { .. } => "it would be split from another file named by",
        Refusal::NamedFromAbove { .. } => "it is cut by a sheet in a folder above it,",
        Refusal::SourceGone => "it is no longer where the catalog says",
        Refusal::Unmoved { .. } => "the volume refused the move",
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

    let pipewire = PipeWire::start("Resonate")?;
    let wanted = wanted_sink(cli, config);
    let sink = select_sink(pipewire.enumerate_sinks(SINK_TIMEOUT)?, wanted.as_ref())?;
    let config = engine_config(cli, config);
    let replay_gain =
        resolve_replay_gain(config.replay_gain, config.levelling, &info.tags.replay_gain);
    let decoded = Decoded::of(&info, None);
    let plan = plan_output(decoded, &sink, &config, replay_gain);
    pipewire.shutdown()?;

    said!("source: {}", info.spec);
    if let Packing::DopMarked(rate) = info.packing {
        said!("dsd:    {rate} ({} Hz, 1-bit)", rate.hz());
    }
    said!("sink:   {} ({})", sink.description, sink.name);
    said!("output: {} [{:?}]", plan.stream, plan.mode);
    if let Some(words) = sink.words_for(plan.stream.format)
        && words != Words::Whole
    {
        said!(
            "words:  offered as {}; the device names {}",
            Words::offered_for(plan.stream.format).spelled(plan.stream.format),
            words.spelled(plan.stream.format)
        );
    }
    if let Some((from, to)) = plan.remix {
        said!("remix:    {from} -> {to}");
    }
    if let Some((from, to)) = plan.resample {
        said!(
            "resample: {from} -> {to} [{:?}, {:?} phase]",
            config.quality,
            config.filter_phase
        );
    }
    if let Some(profile) = plan.equalisation.as_ref() {
        said!(
            "eq:      {} bands, peak {:+.2} dB, preamp {}{}",
            profile.applied(plan.stream.rate),
            profile.peak_db(plan.stream.rate),
            profile.preamp(),
            above_nyquist(profile.passed_over(plan.stream.rate)),
        );
    } else if config.equaliser.enabled {
        said!("eq:      on, but nothing this device is bound to shapes the sound");
    }
    if let Some(impulse) = plan.convolution.as_ref() {
        said!(
            "room:     {} taps a channel over {} channels, measured at {}",
            impulse.frames(),
            impulse.channels(),
            impulse.rate()
        );
    }
    if let Some(restoring) = plan.restoration {
        let wall = restoring.wall.map_or_else(
            || "found as it plays".to_owned(),
            |wall| {
                format!(
                    "studied at {:.1} kHz",
                    f64::from(wall.centihertz()) / 100_000.0
                )
            },
        );
        said!(
            "restore: {} a {:?} source, its wall {wall}",
            restoring.restoration.as_str(),
            restoring.tuning
        );
    }
    if plan.true_peak {
        said!("guard:   true peaks held under -0.1 dBTP");
    }
    if let Some(depth) = plan.dither_to {
        said!("dither:  to {depth} [{:?}]", plan.shaping);
    }
    if let Packing::DopMarked(_) = info.packing {
        said!("dop:     {}", dop_reading(&plan, &sink, &config, info.spec));
    }
    Ok(())
}

fn above_nyquist(bands: usize) -> String {
    match bands {
        0 => String::new(),
        1 => " (1 band is above the output's Nyquist and is not applied)".to_owned(),
        many => format!(" ({many} bands are above the output's Nyquist and are not applied)"),
    }
}

fn dop_reading(
    plan: &OutputPlan,
    sink: &SinkInfo,
    config: &EngineConfig,
    source: StreamSpec,
) -> &'static str {
    if matches!(plan.packing, Packing::DopMarked(_)) {
        return "packed over PCM, untouched";
    }
    if !config.dop {
        return "decimated to PCM; dop is off";
    }
    if !sink.supports(source) {
        return "decimated to PCM; the sink does not take the carrier exactly";
    }
    "decimated to PCM; a gain stage would rewrite the markers"
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
        "PLAYLIST",
        "FILLS",
        "TRACKS",
        "LENGTH",
        "LAST PLAYED",
        "PLAYS",
        "PINNED",
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
        ]);
    }

    said_on!("{}", table.render());
    Ok(())
}

fn counted(count: u64, one: &str, many: &str) -> String {
    match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    }
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
            Some(kept) => format!("a list kept in {}", kept_order(kept)),
            None => "a list".to_owned(),
        };
    };

    match query.text.as_deref() {
        Some(text) => format!("itself, {text:?}"),
        None => "itself".to_owned(),
    }
}

fn kept_order(kept: Kept) -> String {
    match kept.reading {
        Direction::Ascending => format!("{} order", ordered_by(kept.order)),
        Direction::Descending => format!("reverse {} order", ordered_by(kept.order)),
    }
}

const fn ordered_by(order: RowOrder) -> &'static str {
    match order {
        RowOrder::Album => "album",
        RowOrder::Artist => "artist",
        RowOrder::Title => "title",
        RowOrder::Length => "length",
        RowOrder::File => "file name",
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
        return WITHIN_THE_MINUTE.to_owned();
    };

    let minutes = since.as_secs() / 60;
    let hours = minutes / 60;
    match (hours / 24, hours, minutes) {
        (0, 0, 0) => WITHIN_THE_MINUTE.to_owned(),
        (0, 0, minutes) => format!("{minutes}m ago"),
        (0, hours, _) => format!("{hours}h ago"),
        (days, _, _) => format!("{days}d ago"),
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
        said!("{} is called {into} now", found.name);
        return Ok(());
    }
    if wanted.discard {
        library.remove_playlist(found.id)?;
        said!("{}", discarded(&found));
        return Ok(());
    }
    if wanted.pin || wanted.unpin {
        library.pin_playlist(found.id, wanted.pin)?;
        said!("{}", pinning(&found.name, wanted.pin));
        return Ok(());
    }
    if let Some(target) = wanted.export.as_deref() {
        let written = library.export_playlist(found.id, target)?;
        said!(
            "wrote {} rows of {} to {} as {}",
            written.rows,
            found.name,
            target.display(),
            written.format.name()
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
        said!(
            "{} is back in hand, and a row added to it lands at the end",
            found.name
        );
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
        said!("{} holds nothing to play", found.name);
        return Ok(());
    }

    let items = playlists::queue_items(&entries);
    let whole = matching.is_none();
    play_queue(
        cli,
        config,
        items,
        Some(library),
        whole.then_some(found.id),
        None,
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

    said!(
        "copied {copied} of {} into {target}, which now holds {held}",
        found.name
    );
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

    let (id, said) = match library.playlist_named(name)? {
        Some(found) => {
            library.revise_query(found.id, &found.name, &query)?;
            (found.id, format!("{} now fills itself", found.name))
        }
        None => (
            library.save_query(name, &query)?,
            format!("{name} fills itself"),
        ),
    };
    let held = library.playlist(id)?.map_or(0, |found| found.entries);

    said!(
        "{said}{}, and holds {held} now",
        matching(query.text.as_deref())
    );
    if let Some(reads) = understood(query.text.as_deref()) {
        said!("it reads that as {reads}");
    }
    Ok(())
}

fn matching(text: Option<&str>) -> String {
    match text {
        Some(text) => format!(" with whatever matches {text:?}"),
        None => " with the whole library".to_owned(),
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

    said!("added {added} to {name}, which now holds {held}");
    Ok(())
}

fn import(library: &Library, files: &[PathBuf], name: Option<&str>) -> Result<()> {
    if name.is_some() && files.len() > 1 {
        return Err(Error::NameForOnePlaylistOnly { given: files.len() });
    }

    for file in files {
        let read = library.import_playlist(file, name)?;
        let held = library
            .playlist(read.id)?
            .map_or(read.added as u32, |playlist| playlist.entries);

        said!(
            "{}: read {} as {}, added {}, which now holds {held}{}{}{}{}",
            read.name,
            read.format.name(),
            read.encoding.name(),
            read.added,
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
        0 => format!("{name} was already in that order"),
        1 => format!("moved 1 row of {name} into that order"),
        moved => format!("moved {moved} rows of {name} into that order"),
    }
}

fn now_kept(moved: usize, name: &str, kept: Kept) -> String {
    let order = kept_order(kept);
    match moved {
        0 => format!("{name} is kept in {order}, and was already in it"),
        1 => format!("{name} is kept in {order}, and 1 row moved into it"),
        moved => format!("{name} is kept in {order}, and {moved} rows moved into it"),
    }
}

fn tidied(dropped: usize, name: &str) -> String {
    match dropped {
        0 => format!("every row of {name} still names a file that is there"),
        1 => format!("dropped 1 row of {name} whose file has gone"),
        dropped => format!("dropped {dropped} rows of {name} whose files have gone"),
    }
}

fn folded(dropped: usize, name: &str) -> String {
    match dropped {
        0 => format!("every row of {name} names a file no other row does"),
        1 => format!("folded 1 doubled row of {name} into the one above it"),
        dropped => format!("folded {dropped} doubled rows of {name} into the ones above them"),
    }
}

fn pinning(name: &str, pinned: bool) -> String {
    if pinned {
        format!("{name} is pinned, and stands at the top of the listing")
    } else {
        format!("{name} is unpinned, and takes its place in the order the listing reads in")
    }
}

fn discarded(found: &Playlist) -> String {
    let name = &found.name;
    if found.query.is_some() {
        return format!("discarded {name} and the search it filled itself from");
    }

    match found.entries {
        0 => format!("discarded {name}, which held nothing"),
        1 => format!("discarded {name} and the 1 row it held; the file stays where it is"),
        held => {
            format!("discarded {name} and the {held} rows it held; the files stay where they are")
        }
    }
}

fn taken_out(dropped: usize, name: &str, text: &str) -> String {
    match dropped {
        0 => format!("nothing in {name} matches {text:?}, so no row was taken out"),
        1 => format!("took 1 row matching {text:?} out of {name}"),
        dropped => format!("took {dropped} rows matching {text:?} out of {name}"),
    }
}

fn already_held(already: usize) -> String {
    match already {
        0 => String::new(),
        1 => " | 1 was already in it".to_owned(),
        already => format!(" | {already} were already in it"),
    }
}

fn passed_over(elsewhere: usize) -> String {
    match elsewhere {
        0 => String::new(),
        passed => format!(" | passed over {passed} naming no local file"),
    }
}

fn fell_short(short: usize) -> String {
    match short {
        0 => String::new(),
        1 => " | the sheet declared 1 row it did not hold".to_owned(),
        short => format!(" | the sheet declared {short} rows it did not hold"),
    }
}

fn not_on_disk(missing: usize) -> String {
    match missing {
        0 => String::new(),
        1 => " | 1 names a file that is not there".to_owned(),
        missing => format!(" | {missing} name files that are not there"),
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

    let items = mpris::claimed_by(library.as_deref(), queue_items(arguments));
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
        None,
        asleep.and_then(sleep::Sleep::until),
        transport,
    )
}

fn queue_onto_a_running_player(cli: &Cli, config: &Config, wanted: &QueueArgs) -> Result<()> {
    let running = reached(wanted.player.as_deref())?;
    let rows = match wanted.playlist.as_deref() {
        Some(name) => playlist_rows(cli, config, name)?,
        None => queue_items(&wanted.files)
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

    said!("queued {queued} onto {}", running.name());
    Ok(())
}

fn reached(player: Option<&str>) -> Result<Running> {
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
        said!("no player of this build is answering on the session bus");
        return Ok(());
    }

    let mut table = Table::new(vec!["PLAYER", "STATUS", "QUEUED", "TITLE", "ARTIST"]);
    for player in &standing {
        table.push(vec![
            player.name.to_string(),
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

fn play_queue(
    cli: &Cli,
    config: &Config,
    items: Vec<QueueItem>,
    library: Option<Arc<Library>>,
    playing: Option<PlaylistId>,
    asleep: Option<Until>,
    transport: &TransportArgs,
) -> Result<()> {
    let sources = playing_from(library.as_deref());
    let mut engine = engine_config(cli, config);
    if let Some(percent) = transport.volume {
        engine.volume = at_percent(percent)?;
    }
    let player = Arc::new(Player::with_sources(engine, Arc::clone(&sources))?);
    confirm_sink(&player, wanted_sink(cli, config).as_ref());
    let queue = stamp_of(&items);
    let start_at = if transport.shuffle {
        anywhere_in(items.len())
    } else {
        0
    };
    for command in set_out(transport, items, start_at) {
        player.send(command)?;
    }
    if let Some(library) = library.as_ref() {
        library.set_playing_playlist(playing.map(|playlist| Playing { playlist, queue }));
    }
    if asleep.is_some() {
        player.send(Command::SleepUntil(asleep))?;
    }
    let keeps = resuming(config, library.as_deref());

    let (asked_to_quit, quitting) = bounded(1);
    signals::quit_when_told(asked_to_quit.clone(), Arc::clone(&player));
    let mpris = mpris::start(
        &player,
        &sources,
        library.as_ref(),
        Some(asked_to_quit),
        None,
        &Arc::new(AtomicBool::new(config.notifies())),
    );
    let presenter = discord::start(&player, library.as_ref(), config);
    let submitting = library
        .as_ref()
        .map(|library| submitting::start(config, library, &player));

    let keyed = input::KeyAtATime::where_a_terminal();
    let help = if keyed.is_some() {
        input::KEYS
    } else {
        input::HELP
    };
    let mut readout = Readout::over(keyed.is_some() && io::stdout().is_terminal());
    said!("{help}");
    let events = player.events().clone();
    let mut keys = if keyed.is_some() {
        input::keys()
    } else {
        input::lines()
    };
    let sampled = tick(HEARD_SAMPLE);
    let mut listening = Listening::default();
    let mut counted: Option<Listen> = None;
    let mut keeping = Keeping::default();

    loop {
        select! {
            recv(events) -> event => {
                let Ok(event) = event else { break };
                readout.clear();
                if announce(event) {
                    break;
                }
                readout.draw(&player.state());
            }
            recv(sampled) -> _ => {
                count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
                if keeps {
                    keep_the_queue(&player, library.as_deref(), &mut keeping);
                }
                readout.draw(&player.state());
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
                        act(&player, action, help)?;
                    }
                    Pressed::Typing(typed) => readout.typing(typed),
                    Pressed::Unknown(line) => {
                        readout.clear();
                        told!("unknown key {line:?}; ? for help");
                    }
                }
                readout.draw(&player.state());
            }
            recv(quitting) -> _ => break,
        }
    }
    readout.clear();
    drop(keyed);

    count_a_play(&player, library.as_deref(), &mut listening, &mut counted);
    if let Some(library) = library.as_deref() {
        record_a_play(library, listening.leaves(), &mut counted);
    }
    presenter.leave();
    if let Some(submitting) = submitting {
        submitting.leave();
    }
    drop(mpris);
    drop(player);
    Ok(())
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

fn announce(event: Event) -> bool {
    match event {
        Event::TrackStarted(track) => said!("started  track {track}"),
        Event::TrackFinished(track) => said!("finished track {track}"),
        Event::OutputChanged(status) => said!(
            "output   {} [{mode:?}] on sink {sink}",
            status.negotiated,
            mode = status.mode,
            sink = status.sink
        ),
        Event::Underrun { missing } => told!("underrun {missing} frames"),
        Event::Failed { track, error } => {
            told!("track {track} failed");
            report(&error);
        }
        Event::Waiting { track, error } => {
            told!("track {track} waits for a device");
            report(&error);
        }
        Event::CommandFailed { command, error } => {
            told!("{command:?} rejected");
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
        told!("{kind:?} rejected");
        report(&error);
    }
    Ok(())
}

fn frames_in(span: Duration, rate: SampleRate) -> i64 {
    i64::try_from(Frames::from_duration(span, rate).get()).unwrap_or(i64::MAX)
}

pub(crate) fn impulse_at(path: &Path) -> Option<Arc<Impulse>> {
    match read_impulse(&Sources::local(), &MediaLocation::local(path)) {
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
        convolution: config.convolution.as_deref().and_then(impulse_at),
        skip_under_repeat: config.skip_under_repeat(),
        previous_restarts: config.previous_restarts(),
        ..defaults
    }
}

#[cfg(feature = "ui")]
fn launch(cli: Cli, config: Config, library: Arc<Library>) -> Result<()> {
    if let Some(window) = Running::a_window().ok().flatten() {
        return handed_to(&window, &cli.files);
    }
    let places = resonate_ui::Places {
        config: settings_path(&cli)?,
        library: library_path(&cli, &config)?,
        equaliser: config::equaliser_dir()?,
    };
    let settings = settings::File::at(places.config.clone());
    let sources = playing_from(Some(&library));
    let player = Player::with_sources(engine_config(&cli, &config), Arc::clone(&sources))?;
    let player = Arc::new(match config::analyses_dir() {
        Some(dir) => player.keeping_analyses(resonate_engine::KeptAnalyses::at(dir)),
        None => player,
    });
    confirm_sink(&player, wanted_sink(&cli, &config).as_ref());
    let keeps = resuming(&config, Some(&library));
    queue(&player, &cli.files, keeps.then(|| library.as_ref()))?;

    let (asked_to_quit, quitting) = bounded(1);
    signals::quit_when_told(asked_to_quit.clone(), Arc::clone(&player));
    let (attending, attention) = crossbeam_channel::unbounded();
    let (asked_to_raise, raising) = bounded(1);
    let notify = Arc::new(AtomicBool::new(config.notifies()));
    let by_sound = online::by_sound(&config);
    let mpris = mpris::start(
        &player,
        &sources,
        Some(&library),
        Some(asked_to_quit),
        Some(mpris::Window {
            attention,
            raise: asked_to_raise,
        }),
        &notify,
    );
    let listens = listen::in_the_window(&config, mpris.as_ref().map(resonate_mpris::Mpris::teller));
    let presenter = Arc::new(discord::start(&player, Some(&library), &config));
    let submitting = submitting::start(&config, &library, &player);
    let outcome = resonate_ui::run(
        Arc::clone(&player),
        Arc::clone(&library),
        resonate_ui::Lookups {
            lyricists: Arc::new(online::lyricists(&config, Some(Arc::clone(&library)))),
            fingerprinters: Arc::new(online::fingerprinters(
                &config,
                Arc::new(library.sources()),
                &by_sound,
            )),
            reference: online::reference(&config),
            scrobblers: online::scrobblers(&config),
            corrections: Arc::new(online::corrections(&config, Some(Arc::clone(&library)))),
            bindings: equaliser::bound(&config),
            online: resonate_ui::Online {
                enabled: config.online_enabled(),
                after_scan: config.enriches_after_scan(),
                studies: config.studies(),
                lyrics: config.fetches_lyrics(),
                contact: config.contact.clone().unwrap_or_default(),
                acoustid_key: config.acoustid_key.clone().unwrap_or_default(),
                audd_token: config.audd_token.clone().unwrap_or_default(),
                listenbrainz_token: config.listenbrainz_token.clone().unwrap_or_default(),
                subsonic: config.subsonic.clone().unwrap_or_default(),
                subsonic_user: config.subsonic_user.clone().unwrap_or_default(),
                subsonic_password: config.subsonic_password.clone().unwrap_or_default(),
            },
            sourcing: resonate_ui::Sourcing {
                inbox: config.inbox.clone(),
                register: providers::sourced(&config),
            },
            listens,
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
            convolution: config.convolution.clone(),
            music_folder: config.music_folder.clone(),
            file_dropped: config.files_what_is_dropped(),
            window_buttons: config.window_buttons(),
            scroll_volume: config.scrolls_the_volume(),
            scrollbars: config.scrollbars(),
            tabs: config.tabs(),
            remember_tab: config.remembers_tab(),
            last_tab: config.remembered_tab(),
            remember_window_size: config.remembers_window_size(),
            window_size: config.window_size(),
            remember_settings_category: config.remembers_settings_category(),
            last_settings_category: config.remembered_settings_category(),
            presence: config.presence(),
            present: Arc::clone(&presenter) as Arc<dyn resonate_ui::Present>,
            launcher: Arc::new(launcher::Icons::start()),
        },
        config.appearance(),
        resonate_ui::Bus {
            quit: quitting,
            raise: raising,
            attention: attending,
        },
    );
    drop(presenter);
    submitting.leave();
    drop(mpris);
    outcome?;
    Ok(())
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
fn handed_to(window: &Running, arguments: &[OsString]) -> Result<()> {
    let rows: Vec<_> = queue_items(arguments)
        .into_iter()
        .map(|item| (item.location, item.span))
        .collect();
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
    said!("handed to the window already open as {}", window.name());
    Ok(())
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
fn queue(player: &Player, arguments: &[OsString], kept: Option<&Library>) -> Result<()> {
    if !arguments.is_empty() {
        player.send(Command::Load {
            items: mpris::claimed_by(kept, queue_items(arguments)),
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
    let Some(library) = library else {
        return;
    };
    let kept = match keeping.kept(&player.state(), &player.queued()) {
        Some(Keep::Queue(resumption)) => library.keep_resumption(&resumption),
        Some(Keep::Order(reordered)) => library.keep_order(&reordered),
        Some(Keep::Place { row, at }) => library.keep_place(row, at),
        None => return,
    };

    if let Err(error) = kept {
        tracing::warn!(%error, "the queue was not kept for the next run");
    }
}

const SHEET_EXTENSION: &str = "cue";

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
        .or_else(|_| path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

fn queue_items(arguments: &[OsString]) -> Vec<QueueItem> {
    let sources = Sources::local();
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
        let sheet = names_a_sheet(&location)
            .then(|| location.as_path().map(Path::to_path_buf))
            .flatten();

        match sheet {
            Some(path) => items.extend(sheet_items(&sources, &path, &mut minting)),
            None => items.push(QueueItem::whole(minting.mint(), location)),
        }
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
        let Some(file) = path.parent().map(|folder| folder.join(&cut.named)) else {
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
    use std::{fs, os::unix::fs::symlink, process};

    use resonate_codec::{Media, MediaProvider};
    use resonate_core::SourceId;

    use super::*;
    use crate::cli::RepeatArg;

    fn located(argument: &str) -> Option<MediaLocation> {
        queue_items(&[OsString::from(argument)])
            .first()
            .map(|item| item.location.clone())
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
        assert!(parsed(&["resonate", "eq", "--off"]));
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
        assert_eq!(
            located("file:///music/Pink%20Floyd/Echoes.flac"),
            Some(MediaLocation::local("/music/Pink Floyd/Echoes.flac"))
        );
    }

    #[test]
    fn an_argument_that_is_a_path_rather_than_a_uri_is_read_as_the_path_it_is() {
        for path in ["/music/Pink Floyd/Echoes.flac", "/music/a:b.flac"] {
            assert_eq!(located(path), Some(MediaLocation::local(path)));
        }
    }

    #[test]
    fn an_argument_naming_a_file_from_here_is_read_as_the_file_it_names() {
        let here = env::current_dir().expect("a working directory");

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
        let queued = queue_items(&[OsString::from("file:///music/Meddle.flac#frames=588-44100")]);
        assert_eq!(queued.len(), 1);
        assert_eq!(
            queued[0].location,
            MediaLocation::local("/music/Meddle.flac")
        );
        assert_eq!(
            queued[0].span,
            Some(FrameSpan::between(Frames(588), Frames(44_100))),
            "the cut a URI names was queued as the whole file"
        );

        assert!(
            queue_items(&[OsString::from("file:///music/Meddle.flac#frames=100-50")]).is_empty(),
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

    fn silent_wave(seconds_tenths: u32) -> Vec<u8> {
        const CHANNELS: u16 = 2;
        const BITS: u16 = 16;
        const RATE: u32 = 44_100;

        let block_align = CHANNELS * BITS / 8;
        let frames = RATE / 10 * seconds_tenths;
        let data = vec![0_u8; frames as usize * usize::from(block_align)];
        let mut format = Vec::new();
        format.extend_from_slice(&1_u16.to_le_bytes());
        format.extend_from_slice(&CHANNELS.to_le_bytes());
        format.extend_from_slice(&RATE.to_le_bytes());
        format.extend_from_slice(&(RATE * u32::from(block_align)).to_le_bytes());
        format.extend_from_slice(&block_align.to_le_bytes());
        format.extend_from_slice(&BITS.to_le_bytes());

        let mut body = b"WAVE".to_vec();
        for (id, payload) in [(b"fmt ", format.as_slice()), (b"data", data.as_slice())] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            body.extend_from_slice(payload);
        }
        let mut file = b"RIFF".to_vec();
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
        file
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
