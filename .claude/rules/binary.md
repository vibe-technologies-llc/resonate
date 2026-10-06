---
paths:
  - "crates/resonate/**"
  - "crates/resonate-ui/src/settings.rs"
  - "crates/resonate-ui/src/views/settings/**"
---

# The binary: arguments, signals, the terminal and the settings

## The grammar

**Written once, read three times.** `crates/resonate/src/cli.rs` is the clap derive and nothing
else; `build.rs` includes it as a module to write the man pages and shell completions into
`OUT_DIR`, and the packages install what they find there. So `cli.rs` may name `std` and `clap` and
no workspace crate, since the build script links neither: `vocabulary.rs` turns a `QualityArg` into
an `engine::Quality`. The flags that mirror a config key are `global`. `--bit-perfect` and
`--no-bit-perfect` both given is `Error::BitPerfectBothWays`, checked in `run` rather than by clap,
because a `global` flag is propagated into the subcommand, where `conflicts_with` no longer sees
the one given before it.

**`play` and `playlist <name>` set the transport for their run** through `cli::TransportArgs`.
Under `playlist` they conflict with every flag that edits instead of playing. The volume is a whole
percent, the position the readout draws, not a gain. Shuffle and repeat are sent before the `Load`,
so the queue loads already shuffled from a row picked anywhere in it.

**`scan` is incremental and stays off links unless told** (`--full`, `--follow-links`; the walk's
`visited` set reads a folder two links reach once).

**`forget` reads each argument as the first of three things it names**: a root (`Library::remove_root`);
the path or URI `wants` lists a delivery under (`forget_delivered`, the want due again); or a
folder, whose rows with a gone file `Library::retire` drops, even on an unmounted drive, the one
place a scan keeps such rows. A folder holding nothing gone says it named none of the three.

## Reading a file argument

**A URI names a source, not only a file, and one reader is the whole of how.**
`resonate-core::MediaLocation::{from_uri, to_uri}` maps `file://` to the local source and any other
scheme to a `SourceId` of that name; it lives in core because the bus and the command line both
need it. Every file argument goes through it, so `Exec=resonate %U` hands over the path a
`file://` URI names.

- A path argument is read as the file it names *from here*: canonical where it exists (a URI
  through a symlink is the row the scan stored), absolute where not (a missing file is still
  queued and named). The queue row is published on the bus, kept for the next run and matched to a
  library row, and none can read a relative path.
- A local path is escaped and unescaped as *bytes*, so a Latin-1 name survives its URI; only an
  opaque key is held to UTF-8. A `%00` decoding into a local path refuses the URI.
- An argument is another source's URI only where its scheme names a source this build's `Sources`
  holds, so `01:intro.flac` is a file here. Scheme and `localhost` are case-insensitive (RFC 3986).
- A command reading a local file alone (`info`, `explain`, `share <file>`, `playlist --add`) goes
  through `main::local_path`.

**A row cut out of a file is named by its frames as well.** `to_uri_within` appends
`#frames=START-END` (or `START-`) and `from_uri_within` reads it back; a `#` in a path is always
escaped. It is what `xesam:url` carries for a cue row and what `AddTrack`, `OpenUri`, `queue`,
`share` and `play` read, so a single-file rip reaches the bus as twelve tracks. A `#frames=` that
is no span (`MediaLocation::claims_a_span` with nothing `from_uri_within` can read) is
`Error::UnreadableSpan` for `analyse` and a warning and no row for the queue, never the whole
file. A `.cue` on the command line or handed to `OpenUri` becomes its rows through `sheet_items`;
each `FILE` line is found as the scan finds it (`codec::the_file_a_cue_names`: folder, exact name,
case, then stem among audio), so `FILE "ALBUM.WAV"` reaches `album.flac`.

## What this build advertises

**The desktop entry, the bus and the scan advertise what this build can decode, and nothing else.**
`MIME_TYPES` is what `SupportedMimeTypes` answers, what `packaging/resonate.desktop` declares and
what the metainfo's `<provides>` names, held to both in both directions by a test each.
`resonate-library`'s `AUDIO_EXTENSIONS` names the same formats, with `.rf64` and `.w64` beside them
since shared-mime-info gives them no type. `audio.md` has the decoders.

## Signals and the terminal

- **A hang-up asks to leave, as an interrupt does.** `signals::ASKING_TO_LEAVE` is `SIGINT`,
  `SIGTERM` and `SIGHUP`, watched by `quit_when_told` and `cancel_when_told`; a hang-up also tells
  `said` nobody is reading where stdout is a terminal. `tests/signals.rs` hangs up a `resonate mcp`
  and asserts it left on its own.
- **One signal silences the graph even where the front end never answers.** `signals.rs` asks the
  front end to quit and leaves anyway after `DRAINS_WITHIN`; the farewell thread sends
  `Command::Stop` after `SILENCED_AFTER`, so a wedged window is not *heard* for the whole drain.
  `Player::send` only pushes onto a channel, safe from that thread.
- **What the binary prints goes through `said!`, `said_on!` and `told!`, never `println!` or
  `eprintln!`** (`main.rs` denies `clippy::print_stdout` and `print_stderr`). Each lays every
  control character but newline and tab, and the bidi overrides `table::TURNS_THE_READING` lists,
  as a space, so a title, a sheet's name or a service's answer cannot steer the terminal; the
  `play` readout's line clear is the one sequence written on purpose, through `said::raw`. On a
  broken pipe `said` notes nobody is reading and writes nothing more, without exiting, so a
  `tag --apply` printing as it goes finishes the files in hand.
- **Tables are plain text too.** `Table::push` passes every cell through `table::on_one_line`, so
  every table is safe whatever the catalog holds. `info` folds a raw tag value onto one line and
  cuts it at `WIDEST_TAG_VALUE`. A length reads through one clock (rounded to the millisecond, so
  a minute rolls over rather than printing `1:60.000`); a *listening* total is drawn by a second,
  `stats::heard_for`.
- **Logs go to stderr, whatever the subcommand**, so stdout (a table, a link, `mcp`'s protocol)
  holds no warning. `RESONATE_LOG` is read through `EnvFilter::try_new`: an unparsable filter warns
  and falls back to `DEFAULT_LOG`, the one setting where a silent fallback would hide the
  diagnostics being reached for.
- **`play` with nothing to play says so and exits 1** (`NoFileNamed`, `NothingPlayableNamed`)
  rather than opening an empty transport.
- **`play` takes a key at a time where it is typed at.** On a terminal, `input::KeyAtATime` leaves
  canonical mode and echo through `rustix`'s safe `termios`, keeps `ISIG`, and restores on drop. A
  wedged front end's `process::exit` and a panic (the release profile aborts) run no destructor,
  so the exit goes through `signals::leave`, which restores first, and the first `KeyAtATime`
  installs a panic hook that puts the modes back (through `try_lock`). `input::keys` reads what one
  terminal read holds, so an Escape ending a read is a lone Escape (cancels the typed line) while
  one with more behind it opens an arrow or Alt sequence. A digit or `:` starts a line read
  through the same `parse` a piped line is. With stdout a terminal too, `readout::Readout`
  redraws one line every 500 ms sample and after every key, cleared before anything else prints,
  and cut by display width, ending in `…`, to a column short of the terminal's width (asked at
  every draw), since a wrapped line leaves its first row behind. Piped input is read a line at a
  time with no readout.
- **A `play` in the background reads nothing.** `input::played_in_the_background` weighs the
  terminal's foreground group against the process's own; where they differ neither the modes nor
  stdin are touched, since either raises the `SIGTTOU`/`SIGTTIN` that stops the job. It says so
  on stderr and plays on.
- **A length is read one way wherever one is typed.** `lasting::lasting` takes a bare number in
  the unit its caller names (seconds for `play`'s seeks, minutes for a sleep spec), a clock
  (`1:30`, `1:02:03`, fields after the first under 60) or units coarsest first (`1h30m`), and
  answers `None` for anything else. `play`'s `f`/`r` take one leading sign's worth of slack, a
  seek past `i64::MAX` frames saturates, and an override that will not read is refused as an
  unknown line rather than taking the step. A sleep spec is `track`, `queue`, `off` or a non-zero
  length kept to the second.
- **A headless pass stops at a file boundary on the first signal.** `scan`, `enrich`, `poll`,
  `tag`, `organise` and `vault --import` run through `until_told`, which puts
  `signals::cancel_when_told` over the pass: the first signal calls the pass's `cancel`, so the
  file in hand is finished, the catalog follows and the summary says `cancelled`; a second leaves
  at once. A cancelled pass answers `Error::Cancelled { pass }` through `finished`, exit 1; a pass
  that ran out with failures answers `Error::FilesFailed { pass, failed }` through `none_failed`,
  exit 1 too, after the summary (and for a scan after its lookup); `vault --verify` answers
  `ObjectsUnverified`. Writes `tag` passes over by design (`Unwritten::is_a_failure`) are not
  failures. A cancelled scan asks the reference nothing after it. The handles are one
  `resonate_library::PassHandle` (`ScanHandle` and the rest are aliases) and a thread that dies
  answers `Error::Stopped { pass }` after the panic's message goes to an error record
  (`pass::what_it_said`), so a new pass is an alias and a `PassKind` variant, not a struct, an
  error and a macro arm.

## Settings

Loaded from `$XDG_CONFIG_HOME/resonate/config.toml`, or `--config <FILE>`, which must exist where
the XDG path may not. A CLI flag outranks the file, the file outranks `EngineConfig`'s defaults,
and an unknown key warns through `tracing` rather than failing. Every key is a `ConfigKey` variant
(`ConfigKey::ALL`), so a bad value names the key without prose in an error. The keys with a flag
are the `global` ones plus `volume` (as `play --volume`); the rest are set only by the settings
pane and the file. Their defaults and meanings live in `config.rs` and the pane's `views/settings/`
modules; what follows are the decisions.

**A value that will not read costs its key alone.** `Config::take` reads one key and answers the
typed `ConfigType` or `ConfigValue` refusal; `Config::read_from` warns and leaves the key at its
default, so a pane renamed under `last-tab` or a `window-size` from another build does not fail
every command, while a file that is not TOML still does.

**`Config`'s `Debug` prints no secret.** It is written by hand over an exhaustive destructuring of
the struct (a key added and left out fails to compile) and prints `<withheld>` for every token,
password, key and the `contact` (`no_key_holding_a_secret_or_the_contact_is_printed_by_debug`).

**Keys whose rule is not obvious**

- **Live keys ride on `Stored` onto a `ResonateApp` field** (`music_folder`, `file_dropped`,
  `scroll_volume`, `mouse_navigation`, `scrollbars`, `tabs`, `WindowButtons`), written by the
  Appearance, Library and Desktop panes and live at once. `Config::window_buttons` and
  `Config::tabs` fold their switches; close is never a window button; a tab that is off is left
  out of the sidebar and the pane keys, `RootView::set_pane` lands on the tracks, and the artist
  page's *not held* button is not drawn.
- `artists-drawn` is a `resonate_core::ArtistsDrawn`, `grid` unless the file says `list`, written
  by the artists pane's *List* / *Grid* choice.
- `scrollbars` is a `resonate_core::ScrollbarMode`; a file from when it was a switch still reads
  (`true` as `shown`, `false` as `hidden`).
- `remember-tab`, `remember-window-size` and `remember-settings-category` each guard a saved value
  (`last-tab`, `window-size`, `last-settings-category`); turning a switch off removes only that
  value.
- `listen-for` is held to `cli::LISTENS_FOR_SECONDS`, which `--seconds`' value parser shares,
  because `Recording::holding` lays the clip out up front and an unbounded value aborts.
  `listen-from` is `desktop`, `microphone` or a node name; the flags outrank both for one run.
- `bluetooth-lead-ms` and `bluetooth-awake-s` are held to `BLUETOOTH_LEAD_MS` and
  `BLUETOOTH_AWAKE_S` through `At::within`, so a slipped digit is refused at startup rather than
  playing hours of silence or keeping headphones awake for days.
- `music-folder` is the one ordinary folder new songs are copied into (drops, `ui.md` and
  `library.md`'s *Taking files in*; a provider's delivery where no vault is open, `providers.md`),
  filed by `organise-as` when `file-dropped` is on; never the vault. Choosing one stores its
  canonical path, refuses a path that is not a folder or lies inside the vault
  (`unusable_as_the_music_folder`), and adds it to the roots unless a root already reaches it.
- `hifi-api` is an optional custom TIDAL server; blank or absent the hosted HiFi service is used
  while `online` is on (`providers.md`).
- `monochrome` is an optional custom Monochrome track streamer; blank or absent
  `tracks.monochrome.st` is used while `online` is on (`providers.md`).
- `history-kept` is `forever` or a whole number of days; every command opening the catalog first
  forgets listens and skipped time older than that.
- `enrich-after-scan` off leaves the reference to `enrich`, the Library card's *Enrich* and the
  Online card's *Look up*. `study` off starts no pool of studies from a lookup. `fetch-lyrics` off
  asks LRCLIB only as a track plays. `identify-by-sound` on registers `online::fingerprinters`'
  `ByEar` behind the AcoustID printer, sending twelve seconds of sound to Shazam; it is an
  `Arc<AtomicBool>` shared with the fingerprinter, so it is live. `lyrics-language-from-locale`,
  off unless set, has the sidecar lyric reader choose among sheets named for languages by the
  locale (`lyrics.md`); `online::lyrics_by_the_locale` is the same kind of shared flag, held by the
  `Sidecar` inside `online::lyricists` and flipped by the settings pane's *Lyrics* group, so it too
  is live.
- **A vault is made only where it is asked for** (`made_where_asked`: the default place
  `$XDG_DATA_HOME/resonate/vault`, or a path `--vault` names on this command line) through
  `Vault::make`. The `vault` key's path is opened with `Vault::open`, which refuses a missing root
  (`vault::Error::NotThere`) rather than making `audio/`, `covers/` and `staging/` under an empty
  mount point. Every other command opens through `vault_already_kept`, which passes over a missing
  one: a warning where the `vault` key named it, silence for the default place (a build nobody
  imported into).

**The settings pane writes through `resonate_ui::Settings`**, which the binary fills with
`settings::File`. It edits the document with `toml_edit` rather than reserialising a `Config`, so
a hand-written file keeps its comments and key order; a value written again is replaced in place
with its decor carried over (`replaced_in_place`). The `f32` volume is written as the figure it is
(`settings::as_typed`), so 70 % is `volume = 0.7`. `File::apply` takes a batch of `SettingChange`s
through one `config::edit` and writes only if something moved; a setting the file cannot say
(a folder not in UTF-8) is `SettingNotStored` while the rest land.

Every writer (`config::store`, `clear`, `store_in_table`, `clear_in_table`, `edit`) follows a
symlinked `config.toml` to its file, holds a lock on the containing folder for the whole
read-edit-write (so a window and a `resonate eq` cannot overwrite each other, and no lock file is
left), sweeps what a killed writer left (`config.toml.<pid>-<n>.new` and older builds'
`config.toml.lock`, nothing else), stages under a name of this process's own, creates the staged
file `0600` and narrows the standing file to its owner's bits before a byte is written (the file
holds secrets), `sync_all`s the file and the folder, and renames over the target so a crash cannot
truncate it. A value with a flag is spelled as the flag spells it (`ValueEnum`s from `cli.rs`).
`Setting::Theme`, `Accent`, `Online` and `Contact` never reach the engine; a cleared contact is
`config::clear` on the key, not an empty string, which keeps `Config::contact` `None` and the
User-Agent bare.
