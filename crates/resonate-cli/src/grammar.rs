use std::{ffi::OsString, num::NonZeroUsize, ops::RangeInclusive, path::PathBuf};

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

use crate::words;

const MOST_LISTENED: NonZeroUsize = NonZeroUsize::new(10).unwrap();

pub const LISTENS_FOR_SECONDS: RangeInclusive<u64> = 4..=60;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum QualityArg {
    #[value(alias = "fast")]
    LowLatency,
    Balanced,
    #[default]
    High,
    VeryHigh,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum FilterPhaseArg {
    #[default]
    Linear,
    Intermediate,
    Minimum,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum DitherArg {
    None,
    Rectangular,
    #[default]
    Triangular,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum NoiseShapingArg {
    Flat,
    Lipshitz,
    #[default]
    Threshold,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum AbxOfArg {
    #[default]
    Equaliser,
    Restoration,
    Resampler,
    Dither,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum RestorationArg {
    Off,
    Repair,
    Extend,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Args)]
pub struct AbxArgs {
    #[arg(
        long,
        value_enum,
        value_name = "WHAT",
        default_value_t = AbxOfArg::Equaliser,
        help = words::grammar::abx_args_of()
    )]
    pub of: AbxOfArg,

    #[arg(
        long,
        value_enum,
        value_name = "LEVEL",
        help = words::grammar::abx_args_against_restoration()
    )]
    pub against_restoration: Option<RestorationArg>,

    #[arg(
        long,
        value_enum,
        value_name = "QUALITY",
        help = words::grammar::abx_args_against_quality()
    )]
    pub against_quality: Option<QualityArg>,

    #[arg(
        long,
        value_enum,
        value_name = "PHASE",
        help = words::grammar::abx_args_against_filter_phase()
    )]
    pub against_filter_phase: Option<FilterPhaseArg>,

    #[arg(
        long,
        value_enum,
        value_name = "KIND",
        help = words::grammar::abx_args_against_dither()
    )]
    pub against_dither: Option<DitherArg>,

    #[arg(
        long,
        value_enum,
        value_name = "CURVE",
        help = words::grammar::abx_args_against_noise_shaping()
    )]
    pub against_noise_shaping: Option<NoiseShapingArg>,
}

#[derive(Debug, Parser)]
#[command(name = "resonate", version, about = words::grammar::the_player())]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = words::grammar::cli_config()
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = words::grammar::cli_library()
    )]
    pub library: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help = words::grammar::cli_vault()
    )]
    pub vault: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "NAME",
        help = words::grammar::cli_sink()
    )]
    pub sink: Option<String>,

    #[arg(long, global = true, value_enum, help = words::grammar::cli_quality())]
    pub quality: Option<QualityArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = words::grammar::cli_filter_phase()
    )]
    pub filter_phase: Option<FilterPhaseArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = words::grammar::cli_dither()
    )]
    pub dither: Option<DitherArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = words::grammar::cli_noise_shaping()
    )]
    pub noise_shaping: Option<NoiseShapingArg>,

    #[arg(
        long,
        global = true,
        help = words::grammar::cli_bit_perfect()
    )]
    pub bit_perfect: bool,

    #[arg(
        long,
        global = true,
        help = words::grammar::cli_no_bit_perfect()
    )]
    pub no_bit_perfect: bool,

    #[command(subcommand)]
    pub command: Option<Sub>,

    #[arg(
        value_name = "FILE",
        help = words::grammar::cli_files()
    )]
    pub files: Vec<OsString>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum RepeatArg {
    #[default]
    Off,
    Track,
    Queue,
}

#[derive(Debug, Default, Args)]
pub struct TransportArgs {
    #[arg(
        long,
        help = words::grammar::transport_args_shuffle()
    )]
    pub shuffle: bool,

    #[arg(
        long,
        value_enum,
        value_name = "MODE",
        help = words::grammar::transport_args_repeat()
    )]
    pub repeat: Option<RepeatArg>,

    #[arg(
        long,
        value_name = "PERCENT",
        value_parser = clap::value_parser!(u8).range(0..=100),
        help = words::grammar::transport_args_volume()
    )]
    pub volume: Option<u8>,
}

#[derive(Debug, Subcommand)]
pub enum PartyWanted {
    #[command(about = words::grammar::party_wanted_start())]
    Start {
        #[arg(value_name = "FILES", required = true)]
        files: Vec<std::ffi::OsString>,
    },
    #[command(about = words::grammar::party_wanted_join())]
    Join {
        #[arg(value_name = "LINK")]
        link: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum YtmusicArgs {
    #[command(
        about = words::grammar::ytmusic_args_keep()
    )]
    Keep {
        #[arg(value_name = "URL")]
        url: String,
    },
    #[command(
        about = words::grammar::ytmusic_args_check()
    )]
    Check,
    #[command(
        about = words::grammar::ytmusic_args_sign_in()
    )]
    SignIn,
    #[command(
        about = words::grammar::ytmusic_args_sign_out()
    )]
    SignOut,
}

#[derive(Debug, Args)]
pub struct SharedArgs {
    #[arg(
        long,
        value_name = "PLAYLIST",
        conflicts_with_all = ["join", "leave", "remove_person", "reset"],
        help = words::grammar::shared_args_share()
    )]
    pub share: Option<String>,

    #[arg(
        long,
        value_name = "LINK",
        conflicts_with_all = ["leave", "remove_person", "reset"],
        help = words::grammar::shared_args_join()
    )]
    pub join: Option<String>,

    #[arg(
        long,
        value_name = "PLAYLIST",
        conflicts_with_all = ["remove_person", "reset"],
        help = words::grammar::shared_args_leave()
    )]
    pub leave: Option<String>,

    #[arg(
        long,
        value_name = "PERSON",
        requires = "from",
        conflicts_with = "reset",
        help = words::grammar::shared_args_remove_person()
    )]
    pub remove_person: Option<String>,

    #[arg(
        long,
        value_name = "PLAYLIST",
        help = words::grammar::shared_args_from()
    )]
    pub from: Option<String>,

    #[arg(
        long,
        value_name = "PLAYLIST",
        help = words::grammar::shared_args_reset()
    )]
    pub reset: Option<String>,

    #[arg(
        long,
        value_name = "NAME",
        help = words::grammar::shared_args_name()
    )]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct SyncArgs {
    #[arg(
        long,
        value_name = "CODE",
        conflicts_with_all = ["leave", "forget", "fetch", "handoff"],
        help = words::grammar::sync_args_join()
    )]
    pub join: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["forget", "fetch", "handoff"],
        help = words::grammar::sync_args_leave()
    )]
    pub leave: bool,

    #[arg(
        long,
        value_name = "DEVICE",
        conflicts_with_all = ["fetch", "handoff"],
        help = words::grammar::sync_args_forget()
    )]
    pub forget: Option<String>,

    #[arg(
        long,
        value_name = "PLAYLIST",
        requires = "from",
        conflicts_with = "handoff",
        help = words::grammar::sync_args_fetch()
    )]
    pub fetch: Option<String>,

    #[arg(
        long,
        value_name = "DEVICE",
        help = words::grammar::sync_args_from()
    )]
    pub from: Option<String>,

    #[arg(
        long,
        value_name = "DEVICE",
        help = words::grammar::sync_args_handoff()
    )]
    pub handoff: Option<String>,

    #[arg(
        long,
        value_name = "DEVICE",
        conflicts_with_all = ["join", "leave", "forget", "fetch", "handoff"],
        help = words::grammar::sync_args_control()
    )]
    pub control: Option<String>,

    #[arg(
        long,
        value_name = "N|NAME",
        requires = "control",
        help = words::grammar::sync_args_zone()
    )]
    pub zone: Option<String>,

    #[arg(
        long,
        value_enum,
        value_name = "KEY",
        requires = "control",
        group = "driving",
        help = words::grammar::sync_args_press()
    )]
    pub press: Option<PressArg>,

    #[arg(
        long,
        value_name = "TIME",
        requires = "control",
        group = "driving",
        help = words::grammar::sync_args_seek()
    )]
    pub seek: Option<String>,

    #[arg(
        long,
        value_name = "PERCENT",
        value_parser = clap::value_parser!(u8).range(0..=100),
        requires = "control",
        group = "driving",
        help = words::grammar::sync_args_set_volume()
    )]
    pub set_volume: Option<u8>,

    #[arg(
        long,
        value_name = "ROW",
        value_parser = clap::value_parser!(u32).range(1..),
        requires = "control",
        group = "driving",
        help = words::grammar::sync_args_jump()
    )]
    pub jump: Option<u32>,

    #[arg(
        long,
        value_name = "ROW",
        value_parser = clap::value_parser!(u32).range(1..),
        requires = "control",
        group = "driving",
        help = words::grammar::sync_args_remove()
    )]
    pub remove: Option<u32>,

    #[arg(
        long = "move",
        value_name = "ROW",
        value_parser = clap::value_parser!(u32).range(1..),
        requires_all = ["control", "to"],
        group = "driving",
        help = words::grammar::sync_args_move_row()
    )]
    pub move_row: Option<u32>,

    #[arg(
        long,
        value_name = "ROW",
        value_parser = clap::value_parser!(u32).range(1..),
        requires = "move_row",
        help = words::grammar::sync_args_to()
    )]
    pub to: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum PressArg {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

#[derive(Debug, Args)]
pub struct ScanArgs {
    #[arg(value_name = "ROOT")]
    pub roots: Vec<PathBuf>,

    #[arg(
        long,
        help = words::grammar::scan_args_full()
    )]
    pub full: bool,

    #[arg(
        long,
        help = words::grammar::scan_args_follow_links()
    )]
    pub follow_links: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum SortArg {
    #[default]
    Relevance,
    AlbumThenTrack,
    Title,
    Artist,
    DateAdded,
    Duration,
    Plays,
    Played,
    PlaysThisMonth,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum RowOrderArg {
    #[default]
    Album,
    Artist,
    Title,
    Length,
    File,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum WindowArg {
    Week,
    Month,
    Year,
    #[default]
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum UpkeepArg {
    Once,
    InStep,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum PlaylistOrderArg {
    #[default]
    Name,
    Created,
    Modified,
    Played,
    Plays,
    PlaysThisMonth,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("ordered").args(["order", "query"]).multiple(true)))]
#[command(group(
    ArgGroup::new("transport")
        .args(["shuffle", "repeat", "volume"])
        .multiple(true)
        .conflicts_with_all([
            "add", "export", "tidy", "fold", "drop", "into", "order", "by_hand", "query",
            "rename", "discard", "pin", "unpin", "refresh", "upkeep", "forget_link"
        ])
))]
pub struct PlaylistArgs {
    #[arg(value_name = "NAME")]
    pub name: String,

    #[arg(
        long,
        value_name = "FILE",
        num_args = 1..,
        conflicts_with_all = ["export", "tidy", "fold", "query"],
        help = words::grammar::playlist_args_add()
    )]
    pub add: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["tidy", "fold", "query"],
        help = words::grammar::playlist_args_export()
    )]
    pub export: Option<PathBuf>,

    #[arg(
        long,
        conflicts_with_all = ["fold", "query"],
        help = words::grammar::playlist_args_tidy()
    )]
    pub tidy: bool,

    #[arg(
        long,
        conflicts_with = "query",
        help = words::grammar::playlist_args_fold()
    )]
    pub fold: bool,

    #[arg(
        long,
        requires = "matching",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand", "into"],
        help = words::grammar::playlist_args_drop()
    )]
    pub drop: bool,

    #[arg(
        long,
        value_name = "OTHER",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand"],
        help = words::grammar::playlist_args_into()
    )]
    pub into: Option<String>,

    #[arg(
        long,
        value_enum,
        value_name = "ORDER",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query"],
        help = words::grammar::playlist_args_order()
    )]
    pub order: Option<RowOrderArg>,

    #[arg(
        long,
        requires = "ordered",
        help = words::grammar::playlist_args_reverse()
    )]
    pub reverse: bool,

    #[arg(
        long,
        requires = "order",
        help = words::grammar::playlist_args_keep()
    )]
    pub keep: bool,

    #[arg(
        long,
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order"],
        help = words::grammar::playlist_args_by_hand()
    )]
    pub by_hand: bool,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        help = words::grammar::playlist_args_query()
    )]
    pub query: Option<String>,

    #[arg(
        long,
        value_enum,
        requires = "query",
        default_value_t,
        help = words::grammar::playlist_args_sort()
    )]
    pub sort: SortArg,

    #[arg(
        long,
        value_name = "ROWS",
        requires = "query",
        help = words::grammar::playlist_args_limit()
    )]
    pub limit: Option<NonZeroUsize>,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand"],
        help = words::grammar::playlist_args_matching()
    )]
    pub matching: Option<String>,

    #[arg(
        long,
        value_name = "NEW",
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "discard"
        ],
        help = words::grammar::playlist_args_rename()
    )]
    pub rename: Option<String>,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching"
        ],
        help = words::grammar::playlist_args_discard()
    )]
    pub discard: bool,

    #[arg(
        long,
        conflicts_with_all = [
            "unpin", "add", "export", "tidy", "fold", "query", "order", "by_hand", "into",
            "matching", "rename", "discard"
        ],
        help = words::grammar::playlist_args_pin()
    )]
    pub pin: bool,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "rename", "discard"
        ],
        help = words::grammar::playlist_args_unpin()
    )]
    pub unpin: bool,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "rename", "discard", "pin", "unpin", "upkeep", "forget_link"
        ],
        help = words::grammar::playlist_args_refresh()
    )]
    pub refresh: bool,

    #[arg(
        long,
        value_enum,
        value_name = "UPKEEP",
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "rename", "discard", "pin", "unpin", "forget_link"
        ],
        help = words::grammar::playlist_args_upkeep()
    )]
    pub upkeep: Option<UpkeepArg>,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "rename", "discard", "pin", "unpin"
        ],
        help = words::grammar::playlist_args_forget_link()
    )]
    pub forget_link: bool,

    #[command(flatten)]
    pub transport: TransportArgs,
}

#[derive(Debug, Args)]
pub struct QueueArgs {
    #[arg(
        value_name = "FILE",
        required_unless_present_any = ["playlist", "move_to"],
        help = words::grammar::queue_args_files()
    )]
    pub files: Vec<OsString>,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "files",
        help = words::grammar::queue_args_playlist()
    )]
    pub playlist: Option<String>,

    #[arg(
        long,
        value_name = "PLAYER",
        help = words::grammar::queue_args_player()
    )]
    pub player: Option<String>,

    #[arg(
        long,
        value_name = "ZONE",
        conflicts_with = "player",
        help = words::grammar::queue_args_zone()
    )]
    pub zone: Option<String>,

    #[arg(
        long,
        help = words::grammar::queue_args_next()
    )]
    pub next: bool,

    #[arg(
        long,
        help = words::grammar::queue_args_play()
    )]
    pub play: bool,

    #[arg(
        long,
        value_name = "ZONE",
        conflicts_with_all = ["files", "playlist", "player", "next", "play"],
        help = words::grammar::queue_args_move_to()
    )]
    pub move_to: Option<String>,
}

#[derive(Debug, Args)]
pub struct VaultArgs {
    #[arg(
        long,
        conflicts_with_all = ["verify", "prune", "compact"],
        help = words::grammar::vault_args_import()
    )]
    pub import: bool,

    #[arg(
        long = "root",
        value_name = "ROOT",
        conflicts_with_all = ["verify", "prune", "compact"],
        help = words::grammar::vault_args_roots()
    )]
    pub roots: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["verify", "prune", "compact"],
        help = words::grammar::vault_args_at_most()
    )]
    pub at_most: Option<NonZeroUsize>,

    #[arg(
        long,
        help = words::grammar::vault_args_apply()
    )]
    pub apply: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "prune", "compact", "roots", "at_most"],
        help = words::grammar::vault_args_verify()
    )]
    pub verify: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "verify", "compact", "roots", "at_most", "release"],
        help = words::grammar::vault_args_prune()
    )]
    pub prune: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "verify", "prune", "compact", "at_most"],
        help = words::grammar::vault_args_release()
    )]
    pub release: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "verify", "prune", "release", "roots", "at_most"],
        help = words::grammar::vault_args_compact()
    )]
    pub compact: bool,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("looking").args(["find", "suggest"]).multiple(true)))]
#[command(group(ArgGroup::new("measuring").args(["fit_measurement", "measure"]).multiple(true)))]
pub struct EqArgs {
    #[arg(
        long,
        value_name = "SINK",
        conflicts_with_all = ["find", "list"],
        help = words::grammar::eq_args_pub()
    )]
    pub r#for: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["for", "off", "import", "export", "find", "fetch", "suggest", "list", "forget", "forget_own"],
        help = words::grammar::eq_args_on()
    )]
    pub on: bool,

    #[arg(
        long,
        conflicts_with_all = ["for", "on", "import", "export", "find", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_off()
    )]
    pub off: bool,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with_all = ["find", "suggest", "list", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_profile()
    )]
    pub profile: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["on", "profile", "import", "export", "find", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_unbind()
    )]
    pub unbind: bool,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["on", "off", "export", "find", "fetch", "suggest", "list", "forget", "forget_own"],
        help = words::grammar::eq_args_import()
    )]
    pub import: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["on", "off", "import", "export", "find", "fetch", "suggest", "list", "forget", "forget_own", "unbind"],
        help = words::grammar::eq_args_fit_measurement()
    )]
    pub fit_measurement: Option<PathBuf>,

    #[arg(
        long,
        conflicts_with_all = ["on", "off", "import", "export", "find", "fetch", "fetch_every", "suggest", "list", "forget", "forget_own", "unbind", "fit_measurement"],
        help = words::grammar::eq_args_measure()
    )]
    pub measure: bool,

    #[arg(
        long,
        value_name = "NAME",
        num_args = 0..=1,
        default_missing_value = "",
        requires = "measure",
        help = words::grammar::eq_args_microphone()
    )]
    pub microphone: Option<String>,

    #[arg(
        long,
        value_name = "FILE",
        requires = "measure",
        help = words::grammar::eq_args_keep_measurement()
    )]
    pub keep_measurement: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        requires = "measure",
        help = words::grammar::eq_args_calibration()
    )]
    pub calibration: Option<PathBuf>,

    #[arg(
        long,
        value_name = "NAME|FILE",
        requires = "measuring",
        help = words::grammar::eq_args_target()
    )]
    pub target: Option<String>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["on", "off", "import", "find", "fetch", "suggest", "list", "forget", "forget_own", "fit_measurement"],
        help = words::grammar::eq_args_export()
    )]
    pub export: Option<PathBuf>,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_find()
    )]
    pub find: Option<String>,

    #[arg(
        long,
        value_name = "DEVICE",
        conflicts_with_all = ["on", "off", "import", "export", "find", "suggest", "list", "forget", "forget_own"],
        help = words::grammar::eq_args_fetch()
    )]
    pub fetch: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "find", "fetch", "list", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_suggest()
    )]
    pub suggest: bool,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "find", "fetch", "suggest", "list", "forget", "own", "forget_own", "unbind", "fit_measurement", "measure"],
        help = words::grammar::eq_args_fetch_every()
    )]
    pub fetch_every: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "find", "fetch", "suggest", "forget", "own", "forget_own"],
        help = words::grammar::eq_args_list()
    )]
    pub list: bool,

    #[arg(
        long,
        value_name = "MEASURER",
        requires = "looking",
        help = words::grammar::eq_args_measurer()
    )]
    pub measurer: Option<String>,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with_all = ["on", "off", "profile", "import", "export", "find", "fetch", "suggest", "list", "own", "forget_own"],
        help = words::grammar::eq_args_forget()
    )]
    pub forget: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["off", "profile", "unbind", "find", "suggest", "list", "forget", "forget_own"],
        help = words::grammar::eq_args_own()
    )]
    pub own: bool,

    #[arg(
        long,
        conflicts_with_all = ["on", "off", "profile", "unbind", "import", "export", "find", "fetch", "suggest", "list", "forget", "own"],
        help = words::grammar::eq_args_forget_own()
    )]
    pub forget_own: bool,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("marking").args(["add", "remove"])))]
pub struct FavouritesArgs {
    #[arg(long, help = words::grammar::favourites_args_tracks())]
    pub tracks: bool,

    #[arg(long, help = words::grammar::favourites_args_albums())]
    pub albums: bool,

    #[arg(long, help = words::grammar::favourites_args_artists())]
    pub artists: bool,

    #[arg(
        long,
        value_name = "NAME",
        help = words::grammar::favourites_args_add()
    )]
    pub add: Vec<String>,

    #[arg(
        long,
        value_name = "NAME",
        help = words::grammar::favourites_args_remove()
    )]
    pub remove: Vec<String>,

    #[arg(
        long,
        value_name = "ARTIST",
        requires = "marking",
        help = words::grammar::favourites_args_by()
    )]
    pub by: Option<String>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("wanting").args(["add", "remove"])))]
pub struct WantsArgs {
    #[arg(
        long,
        value_name = "TITLE",
        help = words::grammar::wants_args_add()
    )]
    pub add: Option<String>,

    #[arg(
        long,
        value_name = "TITLE",
        help = words::grammar::wants_args_remove()
    )]
    pub remove: Option<String>,

    #[arg(
        long,
        value_name = "NAME",
        requires = "wanting",
        help = words::grammar::wants_args_artist()
    )]
    pub artist: Option<String>,

    #[arg(
        long,
        value_name = "TITLE",
        requires = "wanting",
        help = words::grammar::wants_args_album()
    )]
    pub album: Option<String>,
}

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("browsing").args(["artist", "album", "mix", "search", "favour", "unfavour"])))]
pub struct BrowsingArgs {
    #[arg(
        long,
        value_name = "ID",
        help = words::grammar::browsing_args_artist()
    )]
    pub artist: Option<String>,

    #[arg(
        long,
        value_name = "ID",
        help = words::grammar::browsing_args_album()
    )]
    pub album: Option<String>,

    #[arg(
        long,
        value_name = "ID",
        help = words::grammar::browsing_args_mix()
    )]
    pub mix: Option<String>,

    #[arg(
        long,
        value_name = "WORDS",
        help = words::grammar::browsing_args_search()
    )]
    pub search: Option<String>,

    #[arg(
        long,
        value_name = "ID",
        help = words::grammar::browsing_args_favour()
    )]
    pub favour: Option<String>,

    #[arg(
        long,
        value_name = "ID",
        help = words::grammar::browsing_args_unfavour()
    )]
    pub unfavour: Option<String>,
}

#[derive(Debug, Args)]
#[group(multiple = false)]
pub struct SpotifyArgs {
    #[arg(
        long,
        help = words::grammar::spotify_args_follow()
    )]
    pub follow: bool,

    #[arg(
        long = "move",
        help = words::grammar::spotify_args_move_once()
    )]
    pub move_once: bool,

    #[arg(
        long,
        help = words::grammar::spotify_args_forget()
    )]
    pub forget: bool,
}

#[derive(Debug, Args)]
pub struct ServedArgs {
    #[arg(
        long,
        value_name = "NAME",
        help = words::grammar::served_args_service()
    )]
    pub service: Option<String>,

    #[command(flatten)]
    pub browsing: BrowsingArgs,
}

#[derive(Debug, Subcommand)]
pub enum Sub {
    #[command(
        about = words::grammar::sub_sinks()
    )]
    Sinks {
        #[arg(
            long,
            help = words::grammar::sub_sinks_renderers()
        )]
        renderers: bool,

        #[arg(
            long = "renderer-at",
            value_name = "URL",
            help = words::grammar::sub_sinks_renderers_at()
        )]
        renderers_at: Vec<String>,

        #[arg(
            long = "cast-at",
            value_name = "HOST[:PORT]",
            help = words::grammar::sub_sinks_cast_at()
        )]
        cast_at: Vec<String>,
    },

    #[command(about = words::grammar::sub_scan())]
    Scan(ScanArgs),

    #[command(about = words::grammar::sub_roots())]
    Roots,

    #[command(
        about = words::grammar::sub_enrich()
    )]
    Enrich {
        #[arg(long, help = words::grammar::sub_enrich_refresh())]
        refresh: bool,

        #[arg(
            long,
            value_name = "N",
            help = words::grammar::sub_enrich_albums()
        )]
        albums: Option<NonZeroUsize>,
    },

    #[command(
        about = words::grammar::sub_wants()
    )]
    Wants(WantsArgs),

    #[command(
        about = words::grammar::sub_sync()
    )]
    Sync(SyncArgs),

    #[command(
        about = words::grammar::sub_shared()
    )]
    Shared(SharedArgs),

    #[command(
        about = words::grammar::sub_party(),
        subcommand
    )]
    Party(PartyWanted),

    #[command(
        about = words::grammar::sub_missing()
    )]
    Missing {
        #[arg(
            long,
            value_name = "NAME",
            help = words::grammar::sub_missing_artist()
        )]
        artist: Option<String>,

        #[arg(
            long,
            requires = "artist",
            help = words::grammar::sub_missing_read_the_rest()
        )]
        read_the_rest: bool,

        #[arg(
            long,
            help = words::grammar::sub_missing_bring_back()
        )]
        bring_back: bool,

        #[arg(
            long,
            value_name = "TITLE",
            conflicts_with_all = ["bring_back", "read_the_rest", "dismiss_release"],
            help = words::grammar::sub_missing_dismiss()
        )]
        dismiss: Option<String>,

        #[arg(
            long,
            value_name = "TITLE",
            conflicts_with_all = ["bring_back", "read_the_rest", "dismiss"],
            help = words::grammar::sub_missing_dismiss_release()
        )]
        dismiss_release: Option<String>,

        #[arg(
            long,
            value_name = "TITLE",
            requires = "dismiss",
            help = words::grammar::sub_missing_album()
        )]
        album: Option<String>,
    },

    #[command(
        about = words::grammar::sub_musicbrainz()
    )]
    Musicbrainz {
        #[arg(
            long,
            conflicts_with_all = ["remove", "search"],
            help = words::grammar::sub_musicbrainz_build()
        )]
        build: bool,

        #[arg(
            long,
            value_name = "FOLDER",
            requires = "build",
            help = words::grammar::sub_musicbrainz_from()
        )]
        from: Option<PathBuf>,

        #[arg(
            long,
            conflicts_with = "search",
            help = words::grammar::sub_musicbrainz_remove()
        )]
        remove: bool,

        #[arg(
            long,
            value_name = "WORDS",
            help = words::grammar::sub_musicbrainz_search()
        )]
        search: Option<String>,
    },

    #[command(
        about = words::grammar::sub_tidal()
    )]
    Tidal,

    #[command(
        about = words::grammar::sub_jellyfin()
    )]
    Jellyfin {
        #[arg(long, help = words::grammar::sub_jellyfin_forget())]
        forget: bool,
    },

    #[command(
        about = words::grammar::sub_spotify()
    )]
    Spotify(SpotifyArgs),

    #[command(
        about = words::grammar::sub_ytmusic(),
        subcommand
    )]
    Ytmusic(YtmusicArgs),

    #[command(
        about = words::grammar::sub_lastfm()
    )]
    Lastfm {
        #[arg(
            long,
            help = words::grammar::sub_lastfm_forget()
        )]
        forget: bool,
    },

    #[command(about = words::grammar::sub_poll())]
    Poll {
        #[arg(long, help = words::grammar::sub_poll_again())]
        again: bool,
    },

    #[command(
        about = words::grammar::sub_forget()
    )]
    Forget {
        #[arg(required = true, value_name = "ROOTS_DELIVERED_OR_FOLDERS")]
        roots: Vec<PathBuf>,
    },

    #[command(
        about = words::grammar::sub_tag()
    )]
    Tag {
        #[arg(
            long,
            value_name = "ROOT",
            help = words::grammar::sub_tag_root()
        )]
        root: Vec<PathBuf>,

        #[arg(
            long,
            help = words::grammar::sub_tag_apply()
        )]
        apply: bool,

        #[arg(
            long,
            conflicts_with = "root",
            help = words::grammar::sub_tag_undo()
        )]
        undo: bool,
    },

    #[command(
        about = words::grammar::sub_dedupe()
    )]
    Dedupe {
        #[arg(
            long,
            help = words::grammar::sub_dedupe_apply()
        )]
        apply: bool,
    },

    #[command(
        about = words::grammar::sub_vault()
    )]
    Vault(VaultArgs),

    #[command(
        about = words::grammar::sub_organise()
    )]
    Organise {
        #[arg(
            long = "as",
            value_name = "LAYOUT",
            help = words::grammar::sub_organise_layout()
        )]
        layout: Option<String>,

        #[arg(
            long,
            value_name = "ROOT",
            help = words::grammar::sub_organise_root()
        )]
        root: Vec<PathBuf>,

        #[arg(
            long,
            help = words::grammar::sub_organise_apply()
        )]
        apply: bool,

        #[arg(
            long,
            conflicts_with_all = ["layout", "root"],
            help = words::grammar::sub_organise_undo()
        )]
        undo: bool,
    },

    #[command(about = words::grammar::sub_play())]
    Play {
        #[arg(value_name = "FILE")]
        files: Vec<OsString>,

        #[arg(
            long,
            value_name = "SPEC",
            help = words::grammar::sleep_spec_means(),
        )]
        sleep: Option<String>,

        #[command(flatten)]
        transport: TransportArgs,
    },

    #[command(
        about = words::grammar::sub_queue()
    )]
    Queue(QueueArgs),

    #[command(
        about = words::grammar::sub_players()
    )]
    Players,

    #[command(
        about = words::grammar::sub_served()
    )]
    Served(ServedArgs),

    #[command(
        about = words::grammar::sub_subsonic()
    )]
    Subsonic(BrowsingArgs),

    #[command(about = words::grammar::sub_playlists())]
    Playlists {
        #[arg(
            long,
            value_enum,
            default_value_t,
            help = words::grammar::sub_playlists_order()
        )]
        order: PlaylistOrderArg,

        #[arg(
            long,
            help = words::grammar::sub_playlists_reverse()
        )]
        reverse: bool,

        #[arg(
            long,
            value_name = "WORDS",
            allow_hyphen_values = true,
            help = words::grammar::sub_playlists_named()
        )]
        named: Option<String>,

        #[arg(
            long,
            conflicts_with_all = ["order", "reverse", "named"],
            help = words::grammar::sub_playlists_refresh()
        )]
        refresh: bool,
    },

    #[command(
        about = words::grammar::sub_playlist()
    )]
    Playlist(PlaylistArgs),

    #[command(
        about = words::grammar::sub_eq()
    )]
    Eq(EqArgs),

    #[command(
        about = words::grammar::sub_abx()
    )]
    Abx {
        #[arg(value_name = "FILE")]
        file: OsString,

        #[arg(
            long,
            value_name = "N",
            default_value_t = 16,
            help = words::grammar::sub_abx_trials()
        )]
        trials: usize,

        #[command(flatten)]
        against: AbxArgs,
    },

    #[command(
        about = words::grammar::sub_import()
    )]
    Import {
        #[arg(
            required = true,
            value_name = "FILE",
            help = words::grammar::sub_import_files()
        )]
        files: Vec<PathBuf>,

        #[arg(
            long,
            value_enum,
            value_name = "UPKEEP",
            default_value = "once",
            help = words::grammar::sub_import_upkeep()
        )]
        upkeep: UpkeepArg,

        #[arg(
            long = "as",
            value_name = "NAME",
            help = words::grammar::sub_import_name()
        )]
        name: Option<String>,
    },

    #[command(
        about = words::grammar::sub_sleep()
    )]
    Sleep {
        #[arg(value_name = "SPEC", help = words::grammar::sleep_spec_means())]
        spec: String,

        #[arg(
            long,
            value_name = "PLAYER",
            help = words::grammar::sub_sleep_player()
        )]
        player: Option<String>,

        #[arg(
            long,
            value_name = "ZONE",
            conflicts_with = "player",
            help = words::grammar::sub_sleep_zone()
        )]
        zone: Option<String>,
    },

    #[command(
        about = words::grammar::sub_stats()
    )]
    Stats {
        #[arg(
            long,
            value_enum,
            default_value_t,
            help = words::grammar::sub_stats_window()
        )]
        window: WindowArg,

        #[arg(
            long,
            value_name = "N",
            default_value_t = MOST_LISTENED,
            help = words::grammar::sub_stats_top()
        )]
        top: NonZeroUsize,
    },

    #[command(
        about = words::grammar::sub_update()
    )]
    Update,

    #[command(
        about = words::grammar::sub_wrapped()
    )]
    Wrapped {
        #[arg(
            long,
            value_name = "YEAR",
            help = words::grammar::sub_wrapped_year()
        )]
        year: Option<i64>,

        #[arg(
            long,
            help = words::grammar::sub_wrapped_summary()
        )]
        summary: bool,
    },

    #[command(about = words::grammar::sub_favourites())]
    Favourites(FavouritesArgs),

    #[command(
        about = words::grammar::sub_suggest()
    )]
    Suggest {
        #[arg(
            long,
            value_name = "NAME",
            help = words::grammar::sub_suggest_save()
        )]
        save: Option<String>,
    },

    #[command(about = words::grammar::sub_share())]
    Share {
        #[arg(
            value_name = "FILE",
            help = words::grammar::sub_share_file()
        )]
        file: Option<PathBuf>,
    },

    #[command(
        about = words::grammar::sub_mcp()
    )]
    Mcp {
        #[arg(
            long,
            value_name = "PLAYER",
            help = words::grammar::sub_mcp_player()
        )]
        player: Option<String>,
    },

    #[command(about = words::grammar::sub_explain())]
    Explain { path: PathBuf },

    #[command(
        about = words::grammar::sub_info()
    )]
    Info {
        path: PathBuf,

        #[arg(long, help = words::grammar::sub_info_graph())]
        graph: bool,
    },

    #[command(
        about = words::grammar::sub_analyse()
    )]
    Analyse {
        #[arg(
            value_name = "FILE",
            help = words::grammar::sub_analyse_file()
        )]
        file: OsString,

        #[arg(
            long,
            value_name = "N",
            help = words::grammar::sub_analyse_track()
        )]
        track: Option<u32>,

        #[arg(
            long,
            help = words::grammar::sub_analyse_recognise()
        )]
        recognise: bool,
    },

    #[command(
        about = words::grammar::sub_listen()
    )]
    Listen {
        #[arg(
            long,
            value_name = "NAME",
            num_args = 0..=1,
            default_missing_value = "",
            help = words::grammar::sub_listen_microphone()
        )]
        microphone: Option<String>,

        #[arg(
            long,
            value_name = "SECONDS",
            value_parser = clap::value_parser!(u64).range(LISTENS_FOR_SECONDS),
            help = words::grammar::sub_listen_seconds()
        )]
        seconds: Option<u64>,

        #[arg(
            long,
            help = words::grammar::sub_listen_microphones()
        )]
        microphones: bool,
    },

    #[command(
        about = words::grammar::sub_studies()
    )]
    Studies {
        #[arg(
            long,
            help = words::grammar::sub_studies_fakes()
        )]
        fakes: bool,

        #[arg(long, help = words::grammar::sub_studies_suspects())]
        suspects: bool,

        #[arg(
            long,
            help = words::grammar::sub_studies_misnamed()
        )]
        misnamed: bool,

        #[arg(
            long,
            value_name = "FILE",
            conflicts_with_all = ["fakes", "suspects", "misnamed"],
            help = words::grammar::sub_studies_take()
        )]
        take: Option<OsString>,
    },
}
