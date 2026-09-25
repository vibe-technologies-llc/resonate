use std::{
    ffi::OsString,
    num::{NonZeroU64, NonZeroUsize},
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand, ValueEnum};

const MOST_LISTENED: NonZeroUsize = NonZeroUsize::new(10).unwrap();

const SLEEP_SPEC_MEANS: &str = "How much longer to play: a bare number of minutes, track to stop \
                                at the end of the one playing, queue to stop at the end of the \
                                queue, or off to take the timer away";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum QualityArg {
    Fast,
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

#[derive(Debug, Parser)]
#[command(name = "resonate", version, about = "A high-fidelity music player")]
pub struct Cli {
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Read settings from FILE instead of $XDG_CONFIG_HOME/resonate/config.toml"
    )]
    pub config: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "FILE",
        help = "Use the library database FILE, which is created where it is not there"
    )]
    pub library: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "DIR",
        help = "Keep the managed vault at DIR"
    )]
    pub vault: Option<PathBuf>,

    #[arg(
        long,
        global = true,
        value_name = "NAME",
        help = "Target the sink with this node.name"
    )]
    pub sink: Option<String>,

    #[arg(long, global = true, value_enum, help = "Resampler quality")]
    pub quality: Option<QualityArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = "The resampler's phase: linear rings before a transient as well as after it, \
                minimum only after it, intermediate between the two"
    )]
    pub filter_phase: Option<FilterPhaseArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = "Which dither covers a word length the output is too short to hold"
    )]
    pub dither: Option<DitherArg>,

    #[arg(
        long,
        global = true,
        value_enum,
        help = "Which curve the dither noise is shaped by. threshold is designed at the output \
                rate and shapes at every rate; lipshitz runs at 44.1 and 48 kHz and is flat \
                elsewhere"
    )]
    pub noise_shaping: Option<NoiseShapingArg>,

    #[arg(
        long,
        global = true,
        help = "Stay on whatever rate the graph is already running at, rather than switching it \
                to match the source. Trades bit-accuracy for never interrupting another client"
    )]
    pub no_bit_perfect: bool,

    #[command(subcommand)]
    pub command: Option<Sub>,

    #[arg(
        value_name = "FILE",
        help = "Files to queue in the window this opens, each a path or the file:// URI a desktop \
                entry's %U passes"
    )]
    pub files: Vec<OsString>,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum PlaylistOrderArg {
    #[default]
    Name,
    Created,
    Modified,
    Played,
    Plays,
}

#[derive(Debug, Args)]
pub struct PlaylistArgs {
    #[arg(value_name = "NAME")]
    pub name: String,

    #[arg(
        long,
        value_name = "FILE",
        num_args = 1..,
        conflicts_with_all = ["export", "tidy", "fold", "query"],
        help = "Append these files to the playlist instead of playing it, creating it if it is new"
    )]
    pub add: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["tidy", "fold", "query"],
        help = "Write the playlist out instead of playing it, as M3U, PLS or XSPF by the file's \
                extension"
    )]
    pub export: Option<PathBuf>,

    #[arg(
        long,
        conflicts_with_all = ["fold", "query"],
        help = "Drop every row whose file is no longer on disk, instead of playing it"
    )]
    pub tidy: bool,

    #[arg(
        long,
        conflicts_with = "query",
        help = "Drop every row naming a file an earlier row already names, keeping the first of \
                each, instead of playing it"
    )]
    pub fold: bool,

    #[arg(
        long,
        requires = "matching",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand", "into"],
        help = "Take the rows --matching names out of the playlist, instead of playing it. The \
                rows it does not match stay, the files stay where they are, and there is no \
                --drop without a search, because a playlist holding nothing is one to discard"
    )]
    pub drop: bool,

    #[arg(
        long,
        value_name = "OTHER",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand"],
        help = "Copy this playlist's rows into OTHER instead of playing it, creating OTHER if \
                it is new. The rows stay here too, --matching copies only the ones that text \
                matches, and a playlist cannot be copied into itself"
    )]
    pub into: Option<String>,

    #[arg(
        long,
        value_enum,
        value_name = "ORDER",
        conflicts_with_all = ["add", "export", "tidy", "fold", "query"],
        help = "Put the rows in this order, instead of playing it. A row no scan has seen goes to \
                the end, except under file, which reads the path every row carries"
    )]
    pub order: Option<RowOrderArg>,

    #[arg(
        long,
        help = "Put them in that order the other way round, whichever of --order and --sort \
                named it"
    )]
    pub reverse: bool,

    #[arg(
        long,
        requires = "order",
        help = "Keep it in that order from now on, so a row added later lands where the order \
                puts it rather than at the end"
    )]
    pub keep: bool,

    #[arg(
        long,
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order"],
        help = "Stop keeping it in order and leave the rows where they stand, so a row added \
                later lands at the end again"
    )]
    pub by_hand: bool,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        help = "Start a playlist that fills itself with whatever this text matches, rather than \
                holding a fixed list, or change the search one already named fills itself from. \
                Give it empty to match the whole library. Words match a title, artist or album, \
                and terms like artist:\"pink floyd\", year:1970-1979, added:<30d, length:>5m, \
                rate:>=96k, depth:24, codec:flac and is:lossless narrow what they match. \
                plays:>5 counts every play a track ever had and plays:>5@30d only the ones \
                inside that span. A leading - takes one out, and or between two means either \
                will do"
    )]
    pub query: Option<String>,

    #[arg(
        long,
        value_enum,
        requires = "query",
        default_value_t,
        help = "The order a saved query hands its rows back in"
    )]
    pub sort: SortArg,

    #[arg(
        long,
        value_name = "ROWS",
        requires = "query",
        help = "Stop a saved query after this many rows"
    )]
    pub limit: Option<NonZeroUsize>,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        conflicts_with_all = ["add", "export", "tidy", "fold", "query", "order", "by_hand"],
        help = "Play only the rows this text matches, in the order the playlist holds them, or \
                copy only those where --into names another playlist, or take only those out \
                where --drop is given. It reads the way a saved query's own text does, and a \
                row no scan has seen matches nothing"
    )]
    pub matching: Option<String>,

    #[arg(
        long,
        value_name = "NEW",
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "discard"
        ],
        help = "Rename the playlist instead of playing it. A name is one name however it is \
                written, so a spelling another playlist already holds is refused"
    )]
    pub rename: Option<String>,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching"
        ],
        help = "Discard the playlist and every row in it instead of playing it. The files stay \
                where they are, and the command line has no undo to put it back with"
    )]
    pub discard: bool,

    #[arg(
        long,
        conflicts_with_all = [
            "unpin", "add", "export", "tidy", "fold", "query", "order", "by_hand", "into",
            "matching", "rename", "discard"
        ],
        help = "Pin the playlist instead of playing it, so it stands at the top of the listing \
                and of the window's sidebar"
    )]
    pub pin: bool,

    #[arg(
        long,
        conflicts_with_all = [
            "add", "export", "tidy", "fold", "query", "order", "by_hand", "into", "matching",
            "rename", "discard"
        ],
        help = "Unpin the playlist instead of playing it, so it takes its place in the order \
                the listing is read in"
    )]
    pub unpin: bool,
}

#[derive(Debug, Args)]
pub struct QueueArgs {
    #[arg(
        value_name = "FILE",
        required_unless_present = "playlist",
        help = "Files to queue, each a path or a file:// URI"
    )]
    pub files: Vec<OsString>,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with = "files",
        help = "Queue the rows of this playlist rather than files named here, reading them out \
                of the library this build holds and sending one a call. A playlist that fills \
                itself from a search is queued as whatever it matches now"
    )]
    pub playlist: Option<String>,

    #[arg(
        long,
        value_name = "PLAYER",
        help = "Queue onto this player, by the bus name `resonate players` prints or by the \
                instance under it. With none it is the one answering to the plain name, or the \
                lowest instance where another process holds that"
    )]
    pub player: Option<String>,

    #[arg(
        long,
        help = "Put them after the row being played rather than at the end of the queue"
    )]
    pub next: bool,

    #[arg(
        long,
        help = "Play the first of them as it lands, rather than leaving the transport on the \
                row it is on"
    )]
    pub play: bool,
}

#[derive(Debug, Args)]
pub struct VaultArgs {
    #[arg(
        long,
        conflicts_with_all = ["verify", "prune"],
        help = "Re-encode every scanned track the vault does not already hold, strip its tags \
                and its pictures, validate what came back and point the catalog at it. \
                Nothing is written until --apply says so"
    )]
    pub import: bool,

    #[arg(
        long = "root",
        value_name = "ROOT",
        conflicts_with_all = ["verify", "prune"],
        help = "Import or release only the tracks scanned from this library root. Repeat it to \
                name more than one; with none, every root is taken"
    )]
    pub roots: Vec<PathBuf>,

    #[arg(
        long,
        value_name = "N",
        conflicts_with_all = ["verify", "prune"],
        help = "Import at most N tracks in this run"
    )]
    pub at_most: Option<NonZeroUsize>,

    #[arg(
        long,
        conflicts_with_all = ["verify", "prune"],
        help = "Make the import rather than printing it"
    )]
    pub apply: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "prune", "apply", "roots", "at_most"],
        help = "Decode every object the vault holds and weigh what comes back against what \
                went in"
    )]
    pub verify: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "verify", "apply", "roots", "at_most", "release"],
        help = "Take away the objects no track and no album names any more"
    )]
    pub prune: bool,

    #[arg(
        long,
        conflicts_with_all = ["import", "verify", "apply", "at_most"],
        help = "Point every vaulted track whose own file is still there back at that file. A \
                track the vault holds the only copy of stays in it, and the objects released \
                are left for --prune to take away"
    )]
    pub release: bool,
}

#[derive(Debug, Args)]
pub struct EqArgs {
    #[arg(
        long,
        value_name = "SINK",
        conflicts_with_all = ["find", "list"],
        help = "Which device's binding the rest of this command reads or writes, by the \
                node.name `resonate sinks` prints. With none it is the binding every device \
                that names none of its own falls back to. With --import or --fetch the profile \
                kept is bound to it"
    )]
    pub r#for: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["off", "import", "export", "find", "fetch", "suggest", "list", "forget", "forget_own"],
        help = "Turn the equaliser on. It is a filter, so the stream stops being bit-perfect \
                and `resonate explain` says so"
    )]
    pub on: bool,

    #[arg(
        long,
        conflicts_with_all = ["on", "import", "export", "find", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = "Turn the equaliser off and leave the samples untouched. Every binding stays, so \
                --on puts them back"
    )]
    pub off: bool,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with_all = ["find", "suggest", "list", "forget", "own", "forget_own"],
        help = "Bind this kept profile to the device --for names and turn the equaliser on. \
                With --import or --fetch it names what the new profile is kept as, and with \
                --export it names which one is written out"
    )]
    pub profile: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["on", "profile", "import", "export", "find", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = "Take the binding away from the device --for names, or from the fallback where \
                it names none"
    )]
    pub unbind: bool,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["on", "off", "export", "find", "fetch", "suggest", "list", "forget", "forget_own"],
        help = "Read a profile in and keep it. An EqualizerAPO ParametricEQ file is read as the \
                bands it names; an AutoEq GraphicEQ file is kept as its curve and fitted onto \
                third-octave bands at the rate each stream plays at"
    )]
    pub import: Option<PathBuf>,

    #[arg(
        long,
        value_name = "FILE",
        conflicts_with_all = ["on", "off", "import", "find", "fetch", "suggest", "list", "forget", "forget_own"],
        help = "Write a profile out as an EqualizerAPO ParametricEQ file — the one --profile \
                names, or the one bound to the device --for names"
    )]
    pub export: Option<PathBuf>,

    #[arg(
        long,
        value_name = "TEXT",
        allow_hyphen_values = true,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "fetch", "suggest", "list", "forget", "own", "forget_own"],
        help = "List the measured devices whose name holds every one of these words, with who \
                measured each and on what rig. Nothing is fetched and nothing is kept"
    )]
    pub find: Option<String>,

    #[arg(
        long,
        value_name = "DEVICE",
        conflicts_with_all = ["on", "off", "import", "export", "find", "suggest", "list", "forget", "forget_own"],
        help = "Fetch the correction measured for this device and keep it, named by --profile \
                or by the device. It is the path --find prints"
    )]
    pub fetch: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["on", "off", "profile", "import", "export", "find", "fetch", "list", "forget", "own", "forget_own"],
        help = "Name the measured device the chosen sink looks like, or say that nothing \
                answers to it clearly enough to be worth guessing at"
    )]
    pub suggest: bool,

    #[arg(
        long,
        conflicts_with_all = ["for", "on", "off", "profile", "import", "export", "find", "fetch", "suggest", "forget", "own", "forget_own"],
        help = "List the profiles kept here, with how many bands each holds and which devices \
                each is bound to"
    )]
    pub list: bool,

    #[arg(
        long,
        value_name = "NAME",
        conflicts_with_all = ["on", "off", "profile", "import", "export", "find", "fetch", "suggest", "list", "own", "forget_own"],
        help = "Discard a kept profile and unbind every device bound to it. The file is taken \
                away, and the command line has no undo to put it back with"
    )]
    pub forget: Option<String>,

    #[arg(
        long,
        conflicts_with_all = ["off", "profile", "unbind", "find", "suggest", "list", "forget", "forget_own"],
        help = "Bind the device --for names to a curve of its own and turn the equaliser on. \
                With --import or --fetch what is read becomes that curve rather than a kept \
                profile, and with --export that curve is what is written out, bound or not"
    )]
    pub own: bool,

    #[arg(
        long,
        conflicts_with_all = ["on", "off", "profile", "unbind", "import", "export", "find", "fetch", "suggest", "list", "forget", "own"],
        help = "Discard the curve of its own the device --for names holds, or every other \
                device's where it names none, and take the binding away where it was bound to \
                that curve. It is how a device gone for good stops leaving its curve behind"
    )]
    pub forget_own: bool,
}

#[derive(Debug, Args)]
pub struct FavouritesArgs {
    #[arg(long, help = "List the favourite tracks and nothing else")]
    pub tracks: bool,

    #[arg(long, help = "List the favourite albums and nothing else")]
    pub albums: bool,

    #[arg(long, help = "List the favourite artists and nothing else")]
    pub artists: bool,
}

#[derive(Debug, Subcommand)]
pub enum Sub {
    #[command(
        about = "Print the sinks PipeWire advertises, and the rates the graph will switch to"
    )]
    Sinks,

    #[command(about = "Scan library roots and exit. With no paths, rescans every registered root")]
    Scan { roots: Vec<PathBuf> },

    #[command(about = "Print the library roots a bare `scan` will walk")]
    Roots,

    #[command(
        about = "Ask the reference about every album and artist not asked lately, and keep what \
                 it answers"
    )]
    Enrich {
        #[arg(long, help = "Ask again about albums and artists already answered")]
        refresh: bool,

        #[arg(
            long,
            value_name = "N",
            help = "Ask about at most N albums and N artists this run"
        )]
        albums: Option<NonZeroUsize>,
    },

    #[command(about = "List the release tracks marked wanted, with what a provider has delivered")]
    Wants,

    #[command(
        about = "List the release tracks the catalog holds no file for, and the releases of held \
                 artists the catalog holds none of"
    )]
    Missing {
        #[arg(
            long,
            value_name = "NAME",
            help = "List only the tracks and releases of the artist with this name"
        )]
        artist: Option<String>,
    },

    #[command(about = "Ask every registered provider for each wanted track not tried lately")]
    Poll {
        #[arg(long, help = "Ask about the wanted tracks tried lately as well")]
        again: bool,
    },

    #[command(
        about = "Drop library roots, and every track scanned from them, or a track a provider \
                 delivered, named by the path or URI `resonate wants` lists it under"
    )]
    Forget {
        #[arg(required = true, value_name = "ROOTS_OR_DELIVERED")]
        roots: Vec<PathBuf>,
    },

    #[command(
        about = "Print the tags that would be written into every scanned file to say what the \
                 catalog was told about it. Nothing is written until --apply says so"
    )]
    Tag {
        #[arg(
            long,
            value_name = "ROOT",
            help = "Write into the files scanned from this library root alone. Repeat it to \
                    name more than one; with none, every root is written"
        )]
        root: Vec<PathBuf>,

        #[arg(
            long,
            help = "Write the tags rather than printing them. Each file is read back \
                    afterwards and the catalog follows what it now says"
        )]
        apply: bool,
    },

    #[command(
        about = "Say what the managed vault holds. The vault keeps a bit-exact copy of every \
                 track imported into it, stripped of its tags and re-encoded, and the library \
                 it was imported from is read and never touched"
    )]
    Vault(VaultArgs),

    #[command(
        about = "Print the moves that would file every scanned track under the layout the \
                 organise-as setting names. Nothing is moved until --apply says so"
    )]
    Organise {
        #[arg(
            long = "as",
            value_name = "LAYOUT",
            help = "File the tracks under this layout rather than the one the organise-as \
                    setting names, for this run alone"
        )]
        layout: Option<String>,

        #[arg(
            long,
            value_name = "ROOT",
            help = "File only the tracks scanned from this library root. Repeat it to name \
                    more than one; with none, every root is filed"
        )]
        root: Vec<PathBuf>,

        #[arg(
            long,
            help = "Make the moves rather than printing them. Every file is renamed where it \
                    stands, the catalog follows it, a file beside it sharing its name travels \
                    with it, and a folder the moves leave empty is taken away"
        )]
        apply: bool,
    },

    #[command(about = "Play files through the engine and print what the transport does")]
    Play {
        #[arg(value_name = "FILE")]
        files: Vec<OsString>,

        #[arg(
            long,
            value_name = "SPEC",
            help = SLEEP_SPEC_MEANS,
        )]
        sleep: Option<String>,
    },

    #[command(
        about = "Queue files or a playlist onto a player already running, through the interface \
                 it puts on the session bus"
    )]
    Queue(QueueArgs),

    #[command(
        about = "Print the players of this build answering on the session bus, with what each \
                 is playing"
    )]
    Players,

    #[command(about = "Print the playlists the library holds")]
    Playlists {
        #[arg(
            long,
            value_enum,
            default_value_t,
            help = "List them in this order, newest or most played first for all but the name"
        )]
        order: PlaylistOrderArg,

        #[arg(
            long,
            help = "Turn the listing around from the way that order reads, oldest or least first"
        )]
        reverse: bool,

        #[arg(
            long,
            value_name = "WORDS",
            allow_hyphen_values = true,
            help = "List only the playlists whose name holds every one of these words. A playlist \
                    has only a name to answer with, so terms like year: and is: are passed over"
        )]
        named: Option<String>,
    },

    #[command(
        about = "Play a playlist, add files to it, copy it into another, drop what a search \
                 matched, save or change a query, write it out, put it in order or keep it in \
                 one, tidy it or fold its doubles"
    )]
    Playlist(PlaylistArgs),

    #[command(
        about = "Print the equaliser in force, turn it on or off, bind profiles to devices, \
                 keep and fetch them, and find the device a correction was measured for"
    )]
    Eq(EqArgs),

    #[command(
        about = "Read playlists in from M3U, PLS or XSPF files, appending to one already named"
    )]
    Import {
        #[arg(required = true, value_name = "FILE")]
        files: Vec<PathBuf>,

        #[arg(
            long = "as",
            value_name = "NAME",
            help = "Name the playlist this, rather than taking the name the file declares"
        )]
        name: Option<String>,
    },

    #[command(
        about = "Set the sleep timer on a player already running, and print what it reads back as"
    )]
    Sleep {
        #[arg(value_name = "SPEC", help = SLEEP_SPEC_MEANS)]
        spec: String,

        #[arg(
            long,
            value_name = "PLAYER",
            help = "Set it on this player, by the bus name `resonate players` prints or by the \
                    instance under it. With none it is the one answering to the plain name, or \
                    the lowest instance where another process holds that"
        )]
        player: Option<String>,
    },

    #[command(
        about = "Print what the catalog has counted: how much was played, and what was listened \
                 to most"
    )]
    Stats {
        #[arg(
            long,
            value_enum,
            default_value_t,
            help = "Count only the plays inside this span"
        )]
        window: WindowArg,

        #[arg(
            long,
            value_name = "N",
            default_value_t = MOST_LISTENED,
            help = "List this many tracks, albums and artists in each table"
        )]
        top: NonZeroUsize,
    },

    #[command(about = "Print the tracks, albums and artists marked a favourite")]
    Favourites(FavouritesArgs),

    #[command(
        about = "Print the playlists the catalog suggests making out of what it holds, with what \
                 each would fill itself from"
    )]
    Suggest {
        #[arg(
            long,
            value_name = "NAME",
            help = "Save the suggestion of this name as a playlist that fills itself from the \
                    search behind it"
        )]
        save: Option<String>,
    },

    #[command(
        about = "Print what a track reads as when it is passed on: who made it, what it is from \
                 and where it can be heard"
    )]
    Share {
        #[arg(
            value_name = "FILE",
            help = "The file to write a share for. With none, it is whatever the player already \
                    running is playing"
        )]
        file: Option<PathBuf>,
    },

    #[command(
        about = "Serve the catalog and the running player to a language model over the Model \
                 Context Protocol, as newline-delimited JSON-RPC on stdin and stdout"
    )]
    Mcp {
        #[arg(
            long,
            value_name = "PLAYER",
            help = "Reach this player, by the bus name `resonate players` prints or by the \
                    instance under it. With none it is the one answering to the plain name, or \
                    the lowest instance where another process holds that"
        )]
        player: Option<String>,
    },

    #[command(about = "Print the output plan for a file against the selected sink")]
    Explain { path: PathBuf },

    #[command(
        about = "Print everything known about a file: tags, ReplayGain, container layout, bitrate"
    )]
    Info {
        path: PathBuf,

        #[arg(long, help = "Draw the bitrate over time")]
        graph: bool,
    },

    #[command(
        about = "Decode a file end to end and say whether its lossless claim holds, how loud it \
                 is and what its audio prints as"
    )]
    Analyse {
        #[arg(
            value_name = "FILE",
            help = "A file, a file:// URI whose #frames=START-END names one cut of it, or a .cue \
                    sheet with --track"
        )]
        file: OsString,

        #[arg(
            long,
            value_name = "N",
            help = "The track of the .cue sheet to analyse, by the number the sheet gives it"
        )]
        track: Option<u32>,

        #[arg(
            long,
            help = "Ask the recognition service what the audio is. Needs `acoustid-key` in \
                    config.toml"
        )]
        recognise: bool,
    },

    #[command(
        about = "Listen to what the desktop is playing, or to a microphone, and name the song — \
                 in the library or not"
    )]
    Listen {
        #[arg(
            long,
            value_name = "NAME",
            num_args = 0..=1,
            default_missing_value = "",
            help = "Listen to a microphone rather than the desktop: the one of that node.name, or \
                    the default where none is named"
        )]
        microphone: Option<String>,

        #[arg(
            long,
            value_name = "SECONDS",
            help = "How long to listen for; twelve seconds is what the signature is taken over"
        )]
        seconds: Option<NonZeroU64>,

        #[arg(
            long,
            help = "List the microphones there are to listen to, and listen to nothing"
        )]
        microphones: bool,
    },

    #[command(
        about = "List what the studies the lookup takes of every track found: the fakes, the \
                 suspects and the tracks whose audio is another song"
    )]
    Studies {
        #[arg(
            long,
            help = "Only the tracks judged fake: a lossy transcode, an upsample or padded bits"
        )]
        fakes: bool,

        #[arg(long, help = "Only the tracks whose spectrum makes them suspect")]
        suspects: bool,

        #[arg(
            long,
            help = "Only the tracks whose audio was recognised as another song"
        )]
        misnamed: bool,

        #[arg(
            long,
            value_name = "FILE",
            conflicts_with_all = ["fakes", "suspects", "misnamed"],
            help = "Name a track by what its audio was recognised as: the title, the artist and \
                    the recording, in place of what its file says. A file, or a file:// URI whose \
                    #frames=START-END names one cut of it"
        )]
        take: Option<OsString>,
    },
}
