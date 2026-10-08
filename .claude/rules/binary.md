---
paths:
  - "crates/resonate/**"
  - "crates/resonate-ui/src/settings.rs"
  - "crates/resonate-ui/src/views/settings/**"
---

# The binary: arguments, signals, the terminal and the settings

## The grammar

- **Written once, read three times.** `cli.rs` is the clap derive alone; `build.rs` includes it to
  write man pages and completions into `OUT_DIR` (packages install them). Names `std` and `clap`,
  no workspace crate (the build script links neither); `vocabulary.rs` maps `QualityArg` to
  `engine::Quality`. Flags mirroring a config key are `global`. `--bit-perfect` with
  `--no-bit-perfect` is `Error::BitPerfectBothWays`, checked in `run` (a `global` flag is
  propagated into the subcommand, where `conflicts_with` misses the one given before it).
- **`play` and `playlist <name>` set the transport** via `cli::TransportArgs`; under `playlist` it
  conflicts with every editing flag. Volume: whole percent (the readout's position, not a gain).
  Shuffle and repeat are sent before the `Load` (queue loads shuffled from a row picked anywhere).
- **`scan`: incremental, off links unless told** (`--full`, `--follow-links`; the walk's `visited`
  set reads a folder two links reach once).
- **`forget` reads each argument as the first of three**: a root (`Library::remove_root`); the
  path/URI `wants` lists a delivery under (`forget_delivered`, want due again); a folder, whose
  rows with a gone file `Library::retire` drops, even on an unmounted drive (the one place a scan
  keeps them). A folder holding nothing gone says it named none of the three.

## Reading a file argument

One reader for URIs: `resonate-core::MediaLocation::{from_uri, to_uri}` maps `file://` to the local
source, any other scheme to a `SourceId` of that name (in core: bus and command line both need it).
Every file argument goes through it (`Exec=resonate %U` hands over a `file://` path).

- **Path = the file it names *from here***: canonical where it exists (a URI through a symlink is
  the scan's row), absolute where not (a missing file is still queued and named). The queue row is
  published on the bus, kept for the next run, matched to a library row; none read a relative path.
- **Local paths escape/unescape as *bytes*** (Latin-1 names survive); only an opaque key is held to
  UTF-8. `%00` decoding into a local path refuses the URI.
- **Another source's URI only where its scheme names a source this build's `Sources` holds**
  (`01:intro.flac` is a file here). Scheme and `localhost` are case-insensitive (RFC 3986).
- **`main::local_path`**: commands reading a local file alone (`info`, `explain`, `share <file>`,
  `playlist --add`).
- **A cut row is named by frames too.** `to_uri_within` appends `#frames=START-END` (or `START-`),
  `from_uri_within` reads it; `#` in a path is always escaped. It is `xesam:url` for a cue row and
  what `AddTrack`, `OpenUri`, `queue`, `share`, `play` read (a single-file rip reaches the bus as
  twelve tracks). A `#frames=` that is no span (`MediaLocation::claims_a_span`, nothing
  `from_uri_within` reads): `Error::UnreadableSpan` for `analyse`, warning and no row for the
  queue, never the whole file. A `.cue` argument or `OpenUri` becomes its rows via `sheet_items`;
  each `FILE` line is found as the scan finds it (`codec::the_file_a_cue_names`: folder, exact
  name, case, then stem among audio): `FILE "ALBUM.WAV"` reaches `album.flac`.

## What this build advertises

Desktop entry, bus and scan advertise what this build decodes, nothing else. `MIME_TYPES`
(`crates/resonate/src/mpris.rs`) = what `SupportedMimeTypes` answers, `packaging/resonate.desktop`
declares and the metainfo's `<provides>` names, held both ways by a test each
(`the_desktop_entry_and_the_bus_advertise_the_same_media_types`,
`the_metainfo_provides_the_media_types_the_bus_offers_and_no_others`). `resonate-core`'s
`AUDIO_EXTENSIONS` (the scan uses it) names the same formats plus `.rf64`, `.w64` (shared-mime-info
gives them no type). Decoders: `audio.md`.

## Signals and the terminal

- **Hang-up leaves, like an interrupt.** `signals::ASKING_TO_LEAVE` = `SIGINT`, `SIGTERM`,
  `SIGHUP`, watched by `quit_when_told` and `cancel_when_told`; a hang-up also tells `said` nobody
  reads where stdout is a terminal (`tests/signals.rs` hangs up a `resonate mcp`, asserts it left).
- **One signal silences the graph though the front end never answers.** `signals.rs` asks the
  front end to quit, leaves anyway after `DRAINS_WITHIN`; the farewell thread sends `Command::Stop`
  after `SILENCED_AFTER`, so a wedged window is not *heard* for the whole drain (`Player::send`
  only pushes onto a channel, safe from that thread).
- **Output via `said!`, `said_on!`, `told!`, never `println!`/`eprintln!`** (`main.rs` denies
  `clippy::print_stdout`, `print_stderr`). Control characters but newline/tab, and the bidi
  overrides in `table::TURNS_THE_READING`, become spaces (titles, sheet names, service answers
  cannot steer the terminal); the readout's line clear is the one deliberate sequence, via
  `said::raw`. On a broken pipe `said` notes nobody reads and writes no more, without exiting
  (`tag --apply` finishes the files in hand).
- **Tables are plain text:** `Table::push` passes every cell through `table::on_one_line`; `info`
  folds a raw tag value to one line, cut at `WIDEST_TAG_VALUE`. Lengths read through one clock
  (`info::clock`, rounded to the millisecond so a minute rolls over, not `1:60.000`); a *listening*
  total through `stats::heard_for`.
- **Logs to stderr** whatever the subcommand (stdout, a table, link or `mcp`'s protocol, holds no
  warning). `RESONATE_LOG` goes through `EnvFilter::try_new`; unparsable warns and falls back to
  `DEFAULT_LOG` (the one setting where silent fallback would hide the diagnostics sought).
- **`play` with nothing to play exits 1** (`NoFileNamed`, `NothingPlayableNamed`), no empty
  transport; nor does a queue that ran out with every track failed and none started
  (`Played`, `Error::NothingPlayed`; a signal or `q` ending it early is no failure). A key whose
  command fails ends the loop through the same farewell (last play counted, presenter and submitter
  left) before the error is answered. **It names what plays**: `readout::billing` = the digest's
  title and artist where it is the row's, else the file's stem, said on start, finish and failure
  and drawn at the readout's end.
- **A password prompt restores the terminal on a signal.** `resonate lastfm` reads the password
  under `signals::leave_when_told`: the first interrupt leaves through `signals::leave`, echo back
  on.
- **`play` takes a key at a time on a terminal.** `input::KeyAtATime` leaves canonical mode and
  echo via `rustix`'s safe `termios`, keeps `ISIG`, restores on drop. A wedged front end's
  `process::exit` and a panic (release aborts) run no destructor: exit goes through
  `signals::leave` (restores first), and the first `KeyAtATime` installs a panic hook restoring the
  modes (via `try_lock`). `tests/terminal.rs` drives it: `play` on a pseudo-terminal of its own
  (`setsid --ctty`; `unsafe_code` rules out a `pre_exec`), against a PipeWire runtime folder with
  no daemon so nothing sounds, is sent `SIGINT`, `SIGTERM` or `SIGHUP` once it has left canonical
  mode, and must leave, killed by none, with canonical mode and echo back. `input::keys` reads
  byte by byte with the escape state kept across reads (`Keyed::escaping`): an Escape cancels the typed line; `ESC [` reads a whole control sequence
  through its parameters to the final byte (Ctrl or Shift with an arrow is the arrow; F5 to F12,
  Delete and the rest are nothing), `ESC O` one byte more (F1 to F4); a key after a lone Escape is
  that key (`a_key_sending_a_longer_sequence_is_read_whole_and_leaves_nothing_typed`). A
  digit or `:` starts a line read through the `parse` a piped line uses. With stdout a terminal,
  `readout::Readout` redraws one line each 500 ms sample (`HEARD_SAMPLE`) and after every key,
  cleared before other output, cut by display width with `…` to a column short of the terminal's
  width (asked each draw; a wrapped line leaves its first row behind). Piped input: a line at a
  time, no readout.
- **`play` in the background reads nothing.** `input::played_in_the_background` compares the
  terminal's foreground group with the process's; where they differ neither modes nor stdin are
  touched (else `SIGTTOU`/`SIGTTIN` stops the job); says so on stderr, plays on.
- **One length reader** (`lasting::lasting`): bare number in the caller's unit (seconds for `play`
  seeks, minutes for sleep), clock (`1:30`, `1:02:03`, later fields under 60) or units coarsest
  first (`1h30m`); else `None`. `play`'s `f`/`r` take one leading sign's worth of slack; a seek past
  `i64::MAX` frames saturates; an unreadable override is refused as an unknown line, not the step.
  Sleep spec: `track`, `queue`, `off` or a non-zero length kept to the second.
- **A headless pass stops at a file boundary on the first signal.** `scan`, `enrich`, `poll`,
  `tag`, `organise`, `vault --import` run through `until_told` (`signals::cancel_when_told` over
  the pass): first signal calls the pass's `cancel` (file in hand finishes, catalog follows,
  summary says `cancelled`); a second leaves at once. Cancelled: `Error::Cancelled { pass }` via
  `finished`, exit 1. Ran out with failures: `Error::FilesFailed { pass, failed }` via
  `none_failed`, exit 1, after the summary (scan: after its lookup). `vault --verify`:
  `ObjectsUnverified { moved, unreached }`: an object that will not read back is noted unvalidated;
  one the vault cannot reach (`Vault::failed_itself`: the file gone as `ObjectGone`, the drive
  unmounted) is said apart and left as the catalog had it. Covers are weighed too
  (`Vault::verify_cover`: one kept as it came by its digest and a readable size, a JXL by decoding
  it whole). `--verify --apply` mends what did not read back: each track of such an object is
  pointed back at its own file where that is still there (`Library::release_the_rows_of`, weighed
  again by the next `--import`; one the vault holds the only copy of is counted and left), and every
  album naming such a cover lets it go (`Library::let_go_of_vault_covers`), then the run ends well
  unless something went unreached. A WAVE object that reads back but was packed as one zstd frame
  naming no length (`Vault::packed_before_the_frames`; an earlier build's) is counted, and with
  `--apply` repacked in frames from itself (`Vault::reframe`), whatever else failed. Writes `tag` passes over by design (`Unwritten::is_a_failure`) are not
  failures. A cancelled scan asks the reference nothing after. Handles are one
  `resonate_library::PassHandle` (`ScanHandle` etc. aliases); a dying thread answers
  `Error::Stopped { pass }` after the panic message goes to an error record (`pass::what_it_said`),
  so a new pass is an alias plus a `PassKind` variant, not a struct, error and macro arm.

## Settings

From `$XDG_CONFIG_HOME/resonate/config.toml` or `--config <FILE>` (must exist, unlike the XDG
path). CLI flag > file > `EngineConfig` defaults; an unknown key warns via `tracing`. Every key is a
`ConfigKey` variant (`ConfigKey::ALL`): a bad value names its key without prose in an error. Keys
with a flag: the `global` ones plus `volume` (`play --volume`); the rest only pane and file.
Defaults and meanings: `config.rs` and `views/settings/`; below are the decisions.

- **A value that will not read costs its key alone.** `Config::take` answers the typed
  `ConfigType`/`ConfigValue` refusal; `config::parse` warns and leaves the key at its default (a
  pane renamed under `last-tab`, or another build's `window-size`, fails no command); a non-TOML
  file still does.
- **`Config`'s `Debug` prints no secret:** hand-written over an exhaustive destructuring (a key
  added and left out fails to compile), `<withheld>` for every token, password, key and `contact`
  (`no_key_holding_a_secret_or_the_contact_is_printed_by_debug`).

**Keys whose rule is not obvious**

- **Live keys ride `Stored` onto a `ResonateApp` field** (`music_folder`, `file_dropped`,
  `scroll_volume`, `mouse_navigation`, `scrollbars`, `tabs`, `WindowButtons`), written by the
  Appearance and Library panes, live at once. `Config::window_buttons`/`Config::tabs` fold their
  switches; close is never a window button; an off tab leaves the sidebar and pane keys,
  `RootView::set_pane` lands on the tracks, the artist page's *not held* button is not drawn.
- `artists-drawn`: `resonate_core::ArtistsDrawn`, `grid` unless the file says `list`; set by the
  artists pane's *List*/*Grid*.
- `spectrum-tilt` (`flat`, `pink`, `steep`), `spectrum-floor` (`60`, `78`, `96`: dB under full
  scale), `spectrum-bands` (`thirds`, `sixths`, `twelfths`), `spectrum-falls` (`slowly`,
  `middling`, `quickly`): the live spectrum's reading, `resonate_core::Spectral`, each its default
  where absent.
- `scrollbars`: `resonate_core::ScrollbarMode`; the old switch still reads (`true` = `shown`,
  `false` = `hidden`).
- `remember-tab`, `remember-window-size`, `remember-settings-category` guard `last-tab`,
  `window-size`, `last-settings-category`; switching off removes only that value.
- `listen-for`: held to `cli::LISTENS_FOR_SECONDS`, shared with `--seconds`' parser
  (`Recording::holding` lays the clip out up front; unbounded aborts). `listen-from`: `desktop`,
  `microphone` or a node name; the flags outrank both for one run.
- `bluetooth-lead-ms`, `bluetooth-awake-s`: held to `BLUETOOTH_LEAD_MS`, `BLUETOOTH_AWAKE_S` via
  `At::within` (a slipped digit is refused at startup, else hours of silence or headphones awake
  for days).
- `music-extensions`: array of supported audio extensions, case-insensitive, leading dot optional;
  absent = every extension this build scans, empty = none. `minimum-length-s`: whole seconds 0-600,
  default 0. Both in Settings → Filters, change listings at once, keep excluded rows and files (no
  rescan to undo); they do not restrict explicit playback or file operations; a positive minimum
  passes over rows of unknown length.
- **A path key reads blank as unset and `~` as the home folder** (`config::a_path`: `library`,
  `vault`, `inbox`, `music-folder`, `convolution`); `~name` stays as written.
- `music-folder`: the one ordinary folder new songs are copied into (drops: `ui.md`, `library.md`'s
  *Taking files in*; a provider's delivery with no vault open: `providers.md`), filed by
  `organise-as` when `file-dropped` is on; never the vault. Choosing stores the canonical path,
  refuses a non-folder or a path inside the vault (`unusable_as_the_music_folder`), adds it to the
  roots unless one already reaches it.
- `hifi-api`: optional custom TIDAL server; blank/absent uses the hosted HiFi service while
  `online` is on (`providers.md`). `monochrome`: optional custom Monochrome streamer; blank/absent
  uses `tracks.monochrome.st` while `online` is on (`providers.md`).
- `lastfm-key`, `lastfm-secret`: an API account the listener registered with Last.fm (empty by
  default, like every key); `lastfm-session`: written by `resonate lastfm --user` or the Online
  card's *Last.fm* group, cleared by `--forget` or *Sign out of Last.fm*. All three are needed before anything is scrobbled there; `Config`'s `Debug` and
  `Accounts`' print none of them.
- `history-kept`: `forever` or whole days; every command opening the catalog first forgets listens
  and skipped time older than that.
- `enrich-after-scan` off leaves the reference to `enrich`, the Library card's *Enrich*, the Online
  card's *Look up*. `study` off starts no study pool from a lookup. `fetch-lyrics` off asks LRCLIB
  only as a track plays. `identify-by-sound`: `online::fingerprinters` always puts `ByEar` behind
  the AcoustID printer; it answers (twelve seconds of sound to Shazam) only while the switch is on
  (`Arc<AtomicBool>` shared with it, so live). `lyrics-language-from-locale` (off unless set): the
  sidecar reader chooses among language-named sheets by locale (`lyrics.md`);
  `online::lyrics_by_the_locale` is the same kind of shared flag, held by the `Sidecar` in
  `online::lyricists`, flipped by the pane's *Lyrics* group, so live.
- **A vault is made only where asked** (`made_where_asked`: default `$XDG_DATA_HOME/resonate/vault`,
  or a path `--vault` names on this command line) via `Vault::make`. The `vault` key's path opens
  with `Vault::open`, which refuses a missing root (`vault::Error::NotThere`) rather than making
  `audio/`, `covers/`, `staging/` under an empty mount point. Every other command opens through
  `vault_already_kept`, passing over a missing one: warning where the `vault` key named it, silence
  for the default place (a build nobody imported into). **The pane names a vault only where none
  is open** (Settings → Vault, `naming_a_vault`): the folder picker's choice is canonicalised,
  refused when it is not a folder or sits inside a library root or the music folder
  (`unusable_as_the_vault`: a scan would catalog its objects as songs), and stored as the `vault`
  key (`Setting::Vault`), opened by `Vault::open` from the next start; an open vault shows its root
  and is never swapped from the window (the rows it holds are keyed to it).

**The pane writes through `resonate_ui::Settings`**, filled by `settings::File`. It edits with
`toml_edit`, not by reserialising a `Config`: a hand-written file keeps comments and key order; a
rewritten value is replaced in place, decor carried over (`replaced_in_place`). The `f32` volume is
written as the figure it is (`settings::as_typed`): 70 % is `volume = 0.7`. `File::apply` takes a
batch of `SettingChange`s through one `config::edit`, writing only if something moved; a setting
the file cannot say (a non-UTF-8 folder) is `SettingNotStored` while the rest land.

Every writer (`config::store`, `clear`, `store_in_table`, `clear_in_table`, `edit`): follows a
symlinked `config.toml`; locks the containing folder for the whole read-edit-write (a window and
`resonate eq` cannot overwrite each other; no lock file left); sweeps what a killed writer left
(`config.toml.<pid>-<n>.new`, older builds' `config.toml.lock`, nothing else); stages under this
process's own name; creates the staged file `0600` and narrows the standing file to its owner's
bits before writing (it holds secrets); `sync_all`s file and folder; renames over the target (a
crash cannot truncate). A value with a flag is spelled as the flag spells it (`ValueEnum`s from
`cli.rs`). `Setting::Theme`, `Accent`, `Online`, `Contact` never reach the engine; a cleared
contact is `config::clear` on the key, not an empty string (`Config::contact` stays `None`, the
User-Agent bare).
