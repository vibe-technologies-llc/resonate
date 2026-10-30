---
paths:
  - "crates/resonate/**"
  - "crates/resonate-ui/src/settings.rs"
  - "crates/resonate-ui/src/views/settings/**"
---

# The binary: arguments, signals, the terminal and the settings

## The grammar

- **Written once, read three times.** `cli.rs` is the clap derive alone; `build.rs` includes it to
  write man pages and completions (it links no workspace crate, so `cli.rs` names `std` and `clap`
  only; `vocabulary.rs` maps `QualityArg` to `engine::Quality`). Flags mirroring a config key are
  `global`; `--bit-perfect` with `--no-bit-perfect` is `Error::BitPerfectBothWays`, checked in `run`
  (a `global` flag propagates into the subcommand, where `conflicts_with` misses the one given
  before it).
- **`play` and `playlist <name>` set the transport** via `cli::TransportArgs` (under `playlist` it
  conflicts with every editing flag). Volume is a whole percent. Shuffle and repeat are sent before
  the `Load`.
- **`abx FILE [--trials N] [--of equaliser|restoration|resampler|dither] [--against-…]` is the
  listening test at the terminal** (`abx.rs`, `eq.md`'s *The listening test*): the equaliser
  against its bypass by default, else the setting `--of` names as set against the `--against-…`
  arm (`cli::AbxArgs`, flattened; an `--against` flag of another test is refused in `run`, since
  clap cannot say a flag is allowed only beside one value): one song looped, keys `a` `b` `x` `1`
  `2` `q` `?` from `input::strokes`, which reads a byte at a time on a terminal (`KeyAtATime`, none in the background) and a line at a time
  piped, and ends at end of input. It prints through `said!` like `play`.
- **`scan` is incremental and off links unless told** (`--full`, `--follow-links`).
- **`forget` reads each argument as the first of three**: a root (`Library::remove_root`); a
  path/URI `wants` lists a delivery under (`forget_delivered`, want due again); a folder, whose rows
  with a gone file `Library::retire` drops even on an unmounted drive (the one place a scan keeps
  them). A folder holding nothing gone says it named none of the three.

- **The catalog's marks are named, not numbered.** `favourites --tracks|--albums|--artists --add|--remove
  <NAME>` (repeatable, one kind) marks through `Library::favour`: a track or album is found by the search
  on its name and kept only where its title folds to the name (`folded_name`), `--by <ARTIST>` narrows
  it, and a name two rows answer is `Error::SeveralNamed` rather than a guess; an artist is
  `artist_named`. `wants --add|--remove <TITLE>` and `missing --dismiss <TITLE>` /
  `--dismiss-release <TITLE>` (narrowed by `--artist` and `--album`) name a release track the same
  way, but act on every row the name folds to, since a want is undone by `--remove` and a dismissal by
  `--bring-back`; a name matching nothing is `Error::NothingNamed`. All live in `favourites.rs` and
  `wanting.rs`, over the calls MCP's edits make.

- **A launch handed to an open window carries its engine flags, and one naming its own world is not
  handed over.** `launch` finds the window by `Running::a_window`, sends `--sink`, `--quality`,
  `--filter-phase`, `--dither`, `--noise-shaping` and `--bit-perfect`/`--no-bit-perfect` over `Tune`
  (`mpris::tuning_of`, `Desktop::tune`) before the files; a window too old for `Tune` is said not to
  have taken the settings. A launch giving `--config`, `--library` or `--vault` (`settings_of_its_own`)
  asks the bus for no window at all and opens one of its own: the open window's settings file and
  catalog are its own and cannot be swapped from outside, so handing files over while ignoring those
  flags would play them from a catalog the listener did not name.

- **A zone is addressed where a running player is.** `queue` and `sleep` take `--zone <N|NAME>`
  (conflicting with `--player`): a number is the zone's (1 the first), anything else the name the
  window shows, case folded (`ZoneAsked`, `Running::zoned`, `Error::NoSuchZone`); with neither flag
  it is the first zone, as before. `queue --move-to <N|NAME>` queues nothing: it moves what the
  `--zone` zone plays onto the named one over `HandOver` (`move_the_playback`, the target's number
  read off its bus name by `Running::zone_number`), conflicting with files, `--playlist`,
  `--player`, `--next` and `--play`. `players` prints a `ZONE` column. `play` is one player and so the
  first zone. The window's zones are made in `zoning.rs`: `Opener` fills the engine's `Opens` seam
  (a player over the same `Sources`, its bus service through `mpris::start_zone`), `restore` opens
  every kept zone and resumes its queue; the signal farewell silences every zone. The settings file
  keeps no zone key: the first zone's device and volume are `sink` and `volume`, the rest live with
  the zones in the catalog (`library.md`).

## Reading a file argument

One reader for URIs: `resonate-core::MediaLocation::{from_uri, to_uri}` (in core because bus and
command line both need it) maps `file://` (and the single-slash `file:/path` that Java's `File.toURI` writes) to the local source, any other scheme to a `SourceId` of
that name. Every file argument goes through it.

- **Path = the file it names *from here***: canonical where it exists, absolute where not (a missing
  file is still queued and named). The queue row is published on the bus, kept for the next run and
  matched to a library row; none read a relative path.
- **Local paths escape/unescape as *bytes*** (Latin-1 names survive); only an opaque key is held to
  UTF-8. `%00` decoding into a local path refuses the URI.
- **Another source's URI only where its scheme names a source this build's `Sources` holds**
  (`01:intro.flac` is a file here). Scheme and `localhost` are case-insensitive.
- **A URI of a service played in place is the service's only where the settings set it**
  (`served.rs`, `providers.md`: `subsonic:`, `tidal:`, `hifi-api:`, `resonate:`, `monochrome:`, each
  a song, album or artist; Monochrome's a song alone): `queue_items_served` reads arguments
  through `Served::sources`, and an album or artist URI is queued as the songs the service lists
  for it. `resonate served [--service NAME]` browses a service (`cli::ServedArgs`: the name is a
  scheme, case folded; without it the first the settings set, in the registry's order): bare, its
  `first_page` (a Subsonic server's artists, a TIDAL account's favourites); `--artist ID`,
  `--album ID`, `--mix ID`, `--search WORDS` (`cli::BrowsingArgs`, one at a time), one table a kind
  found, each row with the URI `play` and `queue` take (`served::tables`); `--favour ID` and
  `--unfavour ID` (the same group) mark or unmark the song the service knows by that id in the
  service's own favourites (`PlaysInPlace::favour`) and print the song's URI; a service with none
  is `NotOffered`. `resonate subsonic` is the same with
  `--service subsonic`. With `online` off it is `OnlineOff`; no service set `NoServedService`; a
  name not set `ServiceNotServed`; `subsonic` without the account `NoSubsonicServer`; a build
  without `online` `NoServed`
  (`the_terminal_browses_the_service_named_and_a_tidal_accounts_first_page_is_its_favourites`).
- **`main::local_path`**: commands reading a local file alone (`info`, `explain`, `share <file>`,
  `playlist --add`).
- **A cut row is named by frames too.** `to_uri_within` appends `#frames=START-END` (or `START-`),
  `from_uri_within` reads it; `#` in a path is always escaped. It is `xesam:url` for a cue row. A
  `#frames=` that is no span (`MediaLocation::claims_a_span`): `Error::UnreadableSpan` for
  `analyse`, warning and no row for the queue, never the whole file. A `.cue` argument or `OpenUri`
  becomes its rows via `sheet_items`; each `FILE` line is found as the scan finds it
  (`codec::the_file_a_cue_names`), so `FILE "ALBUM.WAV"` reaches `album.flac`.
- **A folder or a playlist argument is what it holds** (`items_of`, for `play`, `queue` and a window
  launch): an M3U, PLS or XSPF becomes the rows it lists (`resonate_library::cuts_listed_in`); a
  folder its audio (`names_audio`, dot-names passed over) in name order, each `.cue` as the rows it
  cuts and the files a sheet cuts not again, then its folders the same way, at most `FOLDERS_DEEP`
  (32) down. A playlist inside a folder is not followed
  (`a_folder_is_queued_as_the_audio_inside_it_and_its_sheet_as_the_rows_it_cuts`,
  `a_playlist_is_queued_as_the_rows_it_lists`).

## What this build advertises

Desktop entry, bus and scan advertise what this build decodes, nothing else. `MIME_TYPES`
(`crates/resonate/src/mpris.rs`) = what `SupportedMimeTypes` answers, `packaging/resonate.desktop`
declares and the metainfo's `<provides>` names, held by
`the_desktop_entry_and_the_bus_advertise_the_same_media_types` and
`the_metainfo_provides_the_media_types_the_bus_offers_and_no_others`. `resonate-core`'s
`AUDIO_EXTENSIONS` (the scan's) names the same formats plus `.rf64`, `.w64` (shared-mime-info gives
them no type). Decoders: `audio.md`.

## Signals and the terminal

- **Hang-up leaves, like an interrupt.** `signals::ASKING_TO_LEAVE` = `SIGINT`, `SIGTERM`, `SIGHUP`,
  watched by `quit_when_told` and `cancel_when_told`; a hang-up also tells `said` nobody reads where
  stdout is a terminal (`tests/signals.rs`).
- **A headless pass's watch is `signals::cancelling_on`**, which takes the signals and what to do
  on a second one: `cancel_when_told` is that over `ASKING_TO_LEAVE` and `leave`, and the tests
  drive it with `SIGUSR1`, a stand-in `leaving` and `exit_code_after` (128 + the signal number); a
  dropped `Interrupting` closes the watch whether or not a signal came
  (`the_first_signal_cancels_the_pass_and_a_second_leaves_with_its_number`,
  `a_pass_no_signal_interrupts_answers_its_summary_through_until_told`).
- **One signal silences the graph though the front end never answers.** `signals.rs` asks the front
  end to quit, leaves anyway after `DRAINS_WITHIN`; the farewell thread sends `Command::Stop` after
  `SILENCED_AFTER`, so a wedged window is not *heard* for the whole drain. `relay` takes a
  `Farewell` (what silences the graph, what leaves, the two delays), so the tests drive every path
  with stand-ins and short delays
  (`the_first_signal_asks_to_quit_and_a_second_leaves_with_its_number`,
  `a_front_end_that_never_answers_is_silenced_and_then_left`,
  `a_signal_with_no_front_end_listening_leaves_at_once`).
- **Output via `said!`, `said_on!`, `told!`, never `println!`/`eprintln!`** (`main.rs` denies
  `clippy::print_stdout`, `print_stderr`). Control characters but newline/tab, and the bidi
  overrides in `table::TURNS_THE_READING`, become spaces so titles and service answers cannot steer
  the terminal; the readout's line clear is the one deliberate sequence, via `said::raw`. On a
  broken pipe `said` notes nobody reads and writes no more, without exiting (`tag --apply` finishes
  the files in hand).
- **Tables are plain text:** `Table::push` passes every cell through `table::on_one_line`; `info`
  cuts a raw tag value at `WIDEST_TAG_VALUE`. Lengths read through one clock (`info::clock`, rounded
  to the millisecond so a minute rolls over, not `1:60.000`); a *listening* total through
  `stats::heard_for`.
- **Logs go to stderr** whatever the subcommand (stdout holds tables, links, `mcp`'s protocol).
  `RESONATE_LOG` goes through `EnvFilter::try_new`; unparsable warns and falls back to
  `DEFAULT_LOG`.
- **`play` with nothing to play exits 1** (`NoFileNamed`, `NothingPlayableNamed`), as does a queue
  that ran out with every track failed and none started (`Played`, `Error::NothingPlayed`; a signal
  or `q` ending it early is no failure). A key whose command fails ends the loop through the same
  farewell before the error is answered. **It names what plays**: `readout::billing` = the digest's
  title and artist where it is the row's, else the file's stem.
- **A password prompt restores the terminal on a signal.** `resonate lastfm` reads the password
  under `signals::leave_when_told`: the first interrupt leaves through `signals::leave`, echo back
  on; `leaving_on` is that over any signals and any `leaving`
  (`a_signal_during_a_prompt_leaves_with_its_number`).
- **`play` takes a key at a time on a terminal.** `input::KeyAtATime` leaves canonical mode and echo
  via `rustix`'s safe `termios`, keeps `ISIG`, restores on drop. A wedged front end's
  `process::exit` and a panic (release aborts) run no destructor: exit goes through
  `signals::leave`, and the first `KeyAtATime` installs a panic hook restoring the modes (via
  `try_lock`). `tests/terminal.rs` drives `play` on a pseudo-terminal of its own (`setsid --ctty`;
  `unsafe_code` rules out a `pre_exec`) against a PipeWire runtime folder with no daemon, sends
  `SIGINT`, `SIGTERM` or `SIGHUP`, and requires it to leave with canonical mode and echo back.
  `input::keys` reads byte by byte with the escape state kept across reads (`Keyed::escaping`): an
  Escape cancels the typed line; `ESC [` reads a whole control sequence to its final byte (Ctrl or
  Shift with an arrow is the arrow; F5 to F12, Delete and the rest are nothing), `ESC O` one byte
  more (F1 to F4); a key after a lone Escape is that key
  (`a_key_sending_a_longer_sequence_is_read_whole_and_leaves_nothing_typed`). A digit or `:` starts a
  line read through the `parse` a piped line uses. With stdout a terminal, `readout::Readout`
  redraws one line each `HEARD_SAMPLE` (500 ms) and after every key, cleared before other output,
  cut by display width with `…` to a column short of the terminal's width (a wrapped line leaves its
  first row behind). Piped input: a line at a time, no readout.
- **`play` in the background reads nothing.** `input::played_in_the_background` compares the
  terminal's foreground group with the process's; where they differ neither modes nor stdin are
  touched (else `SIGTTOU`/`SIGTTIN` stops the job); says so on stderr, plays on.
- **One length reader** (`lasting::lasting`): bare number in the caller's unit (seconds for `play`
  seeks, minutes for sleep), clock (`1:30`, `1:02:03`, later fields under 60) or units coarsest
  first (`1h30m`); else `None`. A seek past `i64::MAX` frames saturates; an unreadable override is
  refused as an unknown line, not the step. Sleep spec: `track`, `queue`, `off` or a non-zero length
  kept to the second.
- **A headless pass stops at a file boundary on the first signal.** `scan`, `enrich`, `poll`, `tag`,
  `organise`, `vault --import` run through `until_told`: the first signal calls the pass's `cancel`
  (file in hand finishes, catalog follows, summary says `cancelled`); a second leaves at once.
  Cancelled: `Error::Cancelled { pass }` via `finished`, exit 1. Ran out with failures:
  `Error::FilesFailed { pass, failed }` via `none_failed`, exit 1, after the summary (scan: after
  its lookup); writes `tag` passes over by design (`Unwritten::is_a_failure`) are not failures. A
  cancelled scan asks the reference nothing after. Handles are one `resonate_library::PassHandle`
  (`ScanHandle` etc. aliases); a dying thread answers `Error::Stopped { pass }` after the panic
  message goes to an error record (`pass::what_it_said`), so a new pass is an alias plus a
  `PassKind` variant.
- **`vault --verify`** ends `ObjectsUnverified { moved, unreached }`: an object that will not read
  back is noted unvalidated; one the vault cannot reach (`Vault::failed_itself`) is said apart and
  left as the catalog had it. Covers are weighed too (`Vault::verify_cover`). `--verify --apply`
  mends what did not read back: each track of such an object is pointed back at its own file where
  that is still there (`Library::release_the_rows_of`; one the vault holds the only copy of is
  counted and left), every album naming such a cover lets it go
  (`Library::let_go_of_vault_covers`), and the run ends well unless something went unreached. A WAVE
  object packed as one zstd frame naming no length (`Vault::packed_before_the_frames`) is counted,
  and with `--apply` repacked in frames from itself (`Vault::reframe`), whatever else failed.
- **`vault --compact`** (conflicting with the other modes) swaps the JPEG covers an earlier build kept
  for byte-exact JXL transcodes where smaller; a preview until `--apply` (`vault.md`).

## Settings

From `$XDG_CONFIG_HOME/resonate/config.toml` or `--config <FILE>` (must exist, unlike the XDG path).
CLI flag > file > `EngineConfig` defaults; an unknown key warns via `tracing`. Every key is a
`ConfigKey` variant (`ConfigKey::ALL`): a bad value names its key without prose in an error. Keys
with a flag: the `global` ones plus `volume`; the rest only pane and file. Defaults and meanings:
`config.rs` and `views/settings/`; below are the decisions.

- **A value that will not read costs its key alone.** `Config::take` answers the typed
  `ConfigType`/`ConfigValue` refusal; `config::parse` warns and leaves the key at its default (a
  pane renamed under `last-tab`, or another build's `window-size`, fails no command); a non-TOML
  file still does.
- **`Config`'s `Debug` prints no secret:** hand-written over an exhaustive destructuring (a key added
  and left out fails to compile), `<withheld>` for every token, password, key and `contact`
  (`no_key_holding_a_secret_or_the_contact_is_printed_by_debug`).
- **A hand-editor reads `resonate-config.toml(5)`** (`packaging/resonate-config.toml.5`, installed by
  every package): one `.TP` entry per key with its type, accepted spellings and default. A key added
  to `ConfigKey` without an entry, or an entry for a key that is gone, fails
  `every_setting_the_file_reads_is_documented_in_its_man_page_and_nothing_else_is`; the entries for
  enum-valued keys must name every spelling the setting takes
  (`the_man_page_gives_every_spelling_a_setting_takes`,
  `the_man_page_names_every_tab_and_settings_category_the_file_remembers`). Defaults and wording are
  written by hand, so a changed default is changed there too.

**Keys whose rule is not obvious**

- **Live keys ride `Stored` onto a `ResonateApp` field** (`music_folder`, `file_dropped`,
  `scroll_volume`, `mouse_navigation`, `reduced_motion`, `scrollbars`, `tabs`, `WindowButtons`), written by the
  Appearance and Library panes, live at once. `Config::window_buttons`/`Config::tabs` fold their
  switches; close is never a window button; an off tab leaves the sidebar and pane keys,
  `RootView::set_pane` lands on the tracks, the artist page's *not held* button is not drawn.
- `artists-drawn`: `resonate_core::ArtistsDrawn`, `grid` unless the file says `list`.
- `spectrum-tilt` (`flat`, `pink`, `steep`), `spectrum-floor` (`60`, `78`, `96` dB under full
  scale), `spectrum-bands` (`thirds`, `sixths`, `twelfths`), `spectrum-falls` (`slowly`, `middling`,
  `quickly`): `resonate_core::Spectral`, each its default where absent.
- `scrollbars`: `resonate_core::ScrollbarMode`; the old switch still reads (`true` = `shown`,
  `false` = `hidden`).
- `remember-tab`, `remember-window-size`, `remember-settings-category` guard `last-tab`,
  `window-size`, `last-settings-category`; switching off removes only that value.
- `listen-for`: held to `cli::LISTENS_FOR_SECONDS`, shared with `--seconds`' parser (unbounded
  aborts `Recording::holding`). `listen-from`: `desktop`, `microphone` or a node name; the flags
  outrank both for one run.
- `bluetooth-lead-ms`, `bluetooth-awake-s`: held to `BLUETOOTH_LEAD_MS`, `BLUETOOTH_AWAKE_S` via
  `At::within` (a slipped digit is refused at startup, else hours of silence or headphones awake
  for days).
- `music-extensions`: array of supported audio extensions, case-insensitive, leading dot optional;
  absent = every extension this build scans, empty = none. `minimum-length-s`: whole seconds 0-600,
  default 0; a positive minimum passes over rows of unknown length. Both live in Settings → Filters,
  change listings at once, keep excluded rows and files (no rescan to undo) and do not restrict
  explicit playback or file operations.
- **A path key reads blank as unset and `~` as the home folder** (`config::a_path`: `library`,
  `vault`, `inbox`, `music-folder`, `convolution`); `~name` stays as written.
- `renderers` (off unless set): search the local network for UPnP media renderers and browse it
  for Google Cast receivers, and list each beside the sound server's sinks, for the window, its
  zones and `play` (`upnp.md`, `cast.md`). Off, nothing is multicast; a `sink` or `--sink` naming
  a renderer (`upnp:` and its UDN) or a receiver (`cast:` and its id) searches anyway, waiting
  for the first answer so the engine's first survey holds it, and so does `sinks --renderers`.
  The settings pane's device list switches it in the running window (`Setting::Renderers`,
  `Rendering::switch`), the window holding a resting network from the start.
- `speaker-angle`: whole degrees 5 to 90 (`resonate_core::SpeakerAngle`, 30 unless set; held by
  `At::within`, a value outside it costs the key alone): how far to either side of the front the
  speakers stand when `convolution` names a SOFA set (`eq.md`); the start reads the set at it
  (`impulse_at`) and the Room correction group writes it as `Setting::SpeakerAngle`.
- `speaker-elevation`: whole degrees -30 to 60 (`resonate_core::SpeakerElevation`, 0 unless set; a
  value outside it costs the key alone): how far above the ears the speakers stand for the same
  set; read, written and worn with `speaker-angle` as `Config::speakers` and
  `Setting::SpeakerElevation`.
- `head-yaw`: whole degrees -90 to 90 (`resonate_core::HeadYaw`, 0 unless set, positive to the left;
  a value outside it costs the key alone): how far the listener's head is turned from the front for
  the same set, the third of `Config::speakers` and `Setting::HeadYaw`.
- `music-folder`: the one ordinary folder new songs are copied into (drops: `ui.md`, `library.md`;
  a provider's delivery with no vault open: `providers.md`), filed by `organise-as` when
  `file-dropped` is on; never the vault. Choosing stores the canonical path, refuses a non-folder or
  a path inside the vault (`unusable_as_the_music_folder`), adds it to the roots unless one already
  reaches it. The pane chooses it from the *Music folders* box, where it is the root whose circle is
  filled (`ui.md`).
- `hifi-secret`: the key the Resonate server (`resonate_core::RESONATE_WEB_ADDRESS`) lists. It
  registers the `resonate` provider on its own, beside `hifi-api` and whatever `hifi-api` and
  `hosted-services` say; it is never sent to a server `hifi-api` names (`providers.md`).
- **`sync` (on) and the seven `sync-*` keys** (`sync.md`): `sync-nearby-only` (off by default) uses no server at all (`Server::Nowhere`): devices sync only on the same network,
  and sync then runs with `online` off too; off, `sync-server` or the hosted one is dialled.
  `sync-server` blank = the hosted `wss://resonate.tecnio.dev/`; `sync-server-certificate` a PEM trusted
  beside the roots; `sync-stun` a STUN server that tells the device its public address for
  punching, blank (none) by default, needed to reach other devices through a `ws://`/`wss://`
  `sync-server`; `sync-key` the pairing code, minted and written back on the first start that
  syncs, withheld by `Debug`, joined by `resonate sync --join` or the pane, cleared by `--leave` or
  the pane's *Regenerate pairing code* (two presses; the sync watcher mints a new one at once);
  `resonate sync --forget <device>` takes a device that is away off every list
  (`Error::SyncDeviceOnline` for one online); `sync-device` the name other devices see (default
  `<hostname> (Linux)`, from `/proc/sys/kernel/hostname`, else `Linux`); `sync-songs` an array of
  `favourites`, `playlists`, `everything` (empty = only on request). Off whenever `online` is, unless
  `sync-nearby-only` (`Config::syncs`). The pane writes them and the running service follows the file
  (`sync.md`). **One process syncs a catalog**: `sync.lock` in the data folder is `try_lock`ed; a
  window holding it leaves `resonate play` and `resonate sync` unsynced (`Error::SyncBusy` for the
  latter).
- `hifi-api`, `monochrome`: optional custom TIDAL and Monochrome servers, asked while `online` is
  on. `hosted-services` (default true) lets blank/absent fall back to the hosted services
  (`tracks.monochrome.st`); with it off nothing hosted is registered (`providers.md`).
- **A rotated TIDAL refresh token that could not be written is told, not lost quietly.**
  `providers::Keeping::unkept` holds the last `TidalTokenNotKept` (the write failed) or
  `TidalTokenHasNoSettings` (no config directory and no `--config`); `resonate poll` ends with it
  as its error after the pass, `resonate mcp` logs it at error, and the window logs it and toasts it
  once when the pass ends (`Sourcing::unkept`, `providers::sourced`'s second half). The settings path
  is already the default config file, so no further location is tried.
- `equaliser-measurer`: the measurer whose AutoEq correction is preferred where several measured a
  device (for every device without one of its own); `[equaliser-measurer-for]` maps a sink's
  `node.name` to the measurer preferred for it alone (a blank or non-text entry is passed over with a
  warning), read as `Config::equaliser_measurer_for(sink)`. `eq --find`/`--suggest` take `--measurer`
  over both and the equaliser pane's search and suggestion take the device in use's over the fixed
  ranking (`EqualiserModel::prefer(Measurers)`); the Equaliser category's *Measured corrections* group
  chooses them as chips, one field for every device and one for the device in use (*Best ranked* /
  *As for the rest*, `resonate_eq::MEASURERS_BEST_FIRST`, and a held name outside them), written as
  `Setting::EqualiserMeasurer` and `Setting::EqualiserMeasurerFor` and worn at once on
  `ResonateApp::measurers` and the model (`RootView::prefer_a_measurer`, `prefer_a_measurer_for`), and
  the group's mark removes both keys.
- `lastfm-key`, `lastfm-secret`: an API account the listener registered (empty by default);
  `lastfm-session`: written by `resonate lastfm --user` or the Scrobbling category's *Last.fm* group,
  cleared by `--forget` or *Sign out of Last.fm*. All three are needed before anything is
  scrobbled; `Config`'s `Debug` and `Accounts`' print none of them.
- `history-kept`: `forever` or whole days; every command opening the catalog first forgets listens
  and skipped time older than that.
- `enrich-after-scan` off leaves the reference to `enrich`, the Library card's *Look up*. `study` off starts no study pool from a lookup. `fetch-lyrics` off asks LRCLIB
  only as a track plays. `identify-by-sound`: `online::fingerprinters` always puts `ByEar` behind
  the AcoustID printer; it answers only while the switch is on (an `Arc<AtomicBool>` shared with it,
  so live). `lyrics-language-from-locale` (off unless set): the sidecar reader chooses among
  language-named sheets by locale (`lyrics.md`); `online::lyrics_by_the_locale` is the same kind of
  shared flag, flipped by the pane's *Lyrics* group, so live.
- **A vault is made only where asked** (`made_where_asked`: default `$XDG_DATA_HOME/resonate/vault`,
  or a path `--vault` names on this command line) via `Vault::make`. The `vault` key's path opens
  with `Vault::open`, which refuses a missing root (`vault::Error::NotThere`) rather than making
  `audio/`, `covers/`, `staging/` under an empty mount point. Every other command opens through
  `vault_already_kept`, passing over a missing one: warning where the `vault` key named it, silence
  for the default place. **The pane names a vault only where none is open** (Settings → Vault,
  `naming_a_vault`): the folder picker's choice is canonicalised, refused when it is not a folder or
  sits inside a library root or the music folder (`unusable_as_the_vault`: a scan would catalog its
  objects as songs), stored as the `vault` key (`Setting::Vault`) and opened from the next start; an
  open vault is never swapped from the window (its rows are keyed to it).

**The pane writes through `resonate_ui::Settings`**, filled by `settings::File`. It edits with
`toml_edit`, not by reserialising a `Config`: a hand-written file keeps comments and key order; a
rewritten value is replaced in place, decor carried over (`replaced_in_place`). The `f32` volume is
written as the figure it is (`settings::as_typed`): 70 % is `volume = 0.7`. `File::apply` takes a
batch of `SettingChange`s through one `config::edit`, writing only if something moved; a setting the
file cannot say (a non-UTF-8 folder) is `SettingNotStored` while the rest land.

**A subcommand's write is delivered to the players already running.** `reloading::delivering` wraps
the write of `resonate eq`, `tidal`, `lastfm` and `sync`: it compares the settings file before and
after and, when it moved, lists the players on the bus (`Running::listed`) and calls `Reload` on each
with the group the command changed (`Reloadable`) and the file's path. A player that took it is
said to take it up; one that refused, or is too old to know `Reload`, is said to keep the settings it
started with and to hear the change only if it reads the file written once started again; with none
running, or no bus, nothing is said. `Desktop::reload` (`mpris.rs`) answers only for the file it
reads (`same_file`, so a `--config` elsewhere is a refusal, **by design**: a player on another
settings file lives in another world, its catalog and accounts included, and one file's equaliser,
account or sync key applied to it would mix two): `Equaliser` reads the file again and sends
`Command::SetEqualisation` from `equaliser::resolved` and, for a window, sends the window the bindings
read again (`resonate_ui::Followed::Bindings`); `Submission` and `Sync` are `Applied` where
the player has a catalog, since `submitting`'s token and the sync watcher (`sync.md`) follow the file
themselves; `Tidal` (named by `resonate tidal`) reads the account again and hands a window
`Followed::Tidal`, which the window writes into its own `Online` and TIDAL fields before asking
every unheld want again, and a windowless player refuses it (it registers no providers). The
window drains `Bus::followed` on the foreground (`follow_when_told`, as it drains `raise`).

Every writer (`config::store`, `clear`, `store_in_table`, `clear_in_table`, `edit`): follows a
symlinked `config.toml`; locks the containing folder for the whole read-edit-write (a window and
`resonate eq` cannot overwrite each other; no lock file left); sweeps what a killed writer left
(`config.toml.<pid>-<n>.new`, older builds' `config.toml.lock`, nothing else); stages under this
process's own name; creates the staged file `0600` and narrows the standing file to its owner's bits
before writing (it holds secrets); `sync_all`s file and folder; renames over the target. A value
with a flag is spelled as the flag spells it (`ValueEnum`s from `cli.rs`). `Setting::Theme`,
`Accent`, `Online`, `Contact` never reach the engine; a cleared contact is `config::clear` on the
key, not an empty string (`Config::contact` stays `None`, the User-Agent bare).
