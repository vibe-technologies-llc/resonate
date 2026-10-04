---
paths:
  - "crates/resonate/**"
  - "crates/resonate-ui/src/settings.rs"
  - "crates/resonate-ui/src/views/settings/**"
---

# The binary: arguments, signals, the terminal and the settings

## The grammar

**Written once, read three times.** `crates/resonate/src/cli.rs` is the clap derive and nothing
else; `build.rs` includes it as a module to write the man pages and the bash, fish and zsh
completions into `OUT_DIR`, so a flag added is documented and completed with no second list, and
the Arch, Fedora and Flatpak packages install what they find under
`target/release/build/resonate-*/out`. The cost: `cli.rs` may name `std` and `clap` and no
workspace crate, since the build script links neither — `vocabulary.rs` turns a `QualityArg` into
an `engine::Quality`. `--config`, `--library`, `--vault`, `--sink`, `--quality`, `--filter-phase`,
`--dither`, `--noise-shaping`, `--bit-perfect` and `--no-bit-perfect` are `global`, reading the
same before and after the subcommand. The last two undo the `bit-perfect` key either way for one
run, and both at once is `Error::BitPerfectBothWays`, checked in `run` rather than by clap: a
`global` flag is propagated into the subcommand, where clap's `conflicts_with` and
`overrides_with` no longer see the one given before it
(`bit_perfect_is_asked_for_or_refused_for_one_run_and_never_both`).

**`resonate play` and `resonate playlist <name>` set the transport for their run.** `--shuffle`,
`--repeat off|track|queue` and `--volume <PERCENT>` are `cli::TransportArgs`, flattened into both;
under `playlist` they are a `transport` group conflicting with every flag that edits instead of
playing, so `--tidy --shuffle` is refused rather than shuffling nothing
(`a_playlist_played_takes_the_transport_and_one_edited_refuses_it`). The volume is a whole
percent from 0 to 100 — the position the readout draws and `+`/`-` step, not a gain — and
replaces the `volume` key in the engine's starting config; shuffle and repeat are sent before the
`Load`, so the queue loads already shuffled from a row picked anywhere in it, as the window's
*Shuffle* does, rather than always opening on the first file named
(`the_transport_is_set_before_the_queue_loads_so_the_first_track_is_already_shuffled`).

**`resonate scan` is incremental and stays off links unless told.** `--full` reads every file
again, whatever its size and time say; `--follow-links` walks into a link to a file or folder,
the walk's own `visited` set reading a folder two links reach once and leaving a link into a root
to that root's walk.

**`resonate forget` reads each argument as the first of three things it names**: a root, which
`Library::remove_root` drops with every track under it; the path or URI `resonate wants` lists a
delivery under, which `forget_delivered` drops and makes due again; or else a folder, which
`Library::retire` empties of every row whose file is gone — even on a drive that is not mounted,
the one place a scan keeps such rows — while a file still there keeps its row
(`forgetting_a_folder_inside_a_root_drops_the_tracks_gone_from_it_and_keeps_the_rest`). A folder
holding nothing gone says it named none of the three.

## Reading a file argument

**A URI names a source, not only a file, and one reader is the whole of how.**
`resonate-core::MediaLocation::{from_uri, to_uri}` maps `file://` to the local source and any other
scheme to a `SourceId` of that name; it lives in core because the bus and the command line both
need it and neither may depend on the other. Every file argument goes through it, so
`Exec=resonate %U` hands `file:///music/Pink%20Floyd/Echoes.flac` over as the path it names, not a
file of that literal name. An argument that is a path — or the path a `file://` URI decodes to — is
read as the file it names *from here*: the canonical path where it exists (so a URI through a
symlink is the row the scan stored) and the absolute one where not (so a missing file is still
queued and named). A queue row is published on the bus, kept for the next run and matched to a
library row by what it holds, and none of the three can read a relative path: `to_uri` writes
`file://track.wav`, which names a host. A local path is escaped and unescaped as *bytes*, so a
Latin-1 name survives its URI rather than becoming U+FFFD and refusing to read back; only an opaque
key is held to UTF-8. A `%00` decoding into a local path refuses the URI, no file name holding one. An argument is read as another source's URI only where its scheme names a
source this build's `Sources` holds, so `01:intro.flac` is a file here, not a key under a source
called `01`. The scheme and `localhost` are read in any case, as RFC 3986 has them — `FILE://`,
`file://LocalHost/`, `Subsonic:` (`a_scheme_and_localhost_are_read_in_any_case`). A command that
reads a local file alone — `info`, `explain`, `share <file>`, `playlist --add` — takes a `file://`
URI through `main::local_path`, the path it decodes to or the argument as written.

**A row cut out of a file is named by its frames as well.** `to_uri_within` appends
`#frames=START-END` — or `START-` for a cut running to the end — and `from_uri_within` reads it
back; a `#` in a path is always escaped, so the fragment cannot be mistaken for a name. It is what
`xesam:url` carries for a cue row, what `AddTrack` and `OpenUri` read, what `resonate queue` sends
for a playlist's cue rows and the rows a `.cue` it is handed cuts, and what `resonate share` looks
the playing row up by — so a single-file rip reaches the bus as twelve tracks, not one file twelve
times. `resonate play`, `resonate queue` and the window handed a cue row's URI queue that row; a
`#frames=` that is no span — `MediaLocation::claims_a_span` with nothing `from_uri_within` can read
— is `Error::UnreadableSpan` for `resonate analyse` and a warning and no row for the queue, never
the whole file. A `.cue` handed to `OpenUri` is read as its rows through the same `sheet_items` a
`.cue` on the command line goes through (`sheet_cuts` beside it). Each `FILE` line is found as the
scan finds it — `codec::the_file_a_cue_names`, the folder a name points into, then the exact name, its
case, then its stem among audio — so `play`, `queue`, `playlist --add`, `analyse --track` and `info`
reach `album.flac` for a `FILE "ALBUM.WAV"` and `CD1/01.wav` for `CD1\01.wav`
(`a_sheet_finds_its_audio_as_the_scan_does_whatever_case_or_extension_it_wrote`).

## What this build advertises

**The desktop entry, the bus and the scan advertise what this build can decode, and nothing else.**
`MIME_TYPES` is what `SupportedMimeTypes` answers, what `packaging/resonate.desktop` declares and
what the metainfo's `<provides>` names, held to both in both directions by a test each;
`resonate-library`'s `AUDIO_EXTENSIONS` names the same formats — with `.rf64` and `.w64` beside
them, which shared-mime-info gives no type of their own (an RF64 named `.wav` is `audio/x-wav`). WavPack is offered through
`symphonia-codec-wavpack` and a decoder of the codec crate's own around it; Monkey's Audio through a
reader and decoder of the codec crate's own over `ape-decoder` (symphonia's `ape` feature is APEv2
metadata, not the codec); AAC, bare ADTS included, because the `aac` feature registers `AdtsReader`
beside the decoder; Opus — `audio/opus` and `audio/x-opus+ogg` — demuxed by symphonia and decoded
by `opus-rs` through the codec crate's own registry. `audio.md` has the rest.

## Signals and the terminal

- **One signal silences the graph even where the front end never answers.** `signals.rs` asks the
  front end to quit and leaves anyway after `DRAINS_WITHIN` (5 s), which bounds how long a wedged
  window lives but not how long it is *heard*: the engine played on for those five seconds. The
  farewell thread sends `Command::Stop` after `SILENCED_AFTER` (1 s) and waits out the rest. A front
  end that will answer does so in ~250 ms, long before, so the polite drain is untouched; one that
  will not is quiet a second after the signal. The `Player` reaches the thread as an `Arc` from both
  call sites, and `Player::send` only pushes onto a channel, so it is safe from a signal handler's
  thread and answers `EngineStopped` where the engine has gone.
- **What the binary prints goes through `said!`, `said_on!` and `told!`, never `println!` or
  `eprintln!`.** Each lays every control character but a newline and a tab, and the bidi overrides
  `table::TURNS_THE_READING` lists, as a space, so a title, a sheet's name, a service's answer or a
  file name can neither steer the terminal nor read backwards — on every command, the tables having
  done it alone before (`a_name_cannot_steer_the_terminal_it_is_printed_on`); the `play` readout's
  line clear is the one sequence written on purpose, through `said::raw`. `said.rs` writes
  to the standard output and, on a broken pipe — a table piped into `head` or a pager quit early —
  notes that nobody is reading and writes nothing more, where `println!` panicked and a release build
  aborted. It does not exit: a `tag --apply` or an `organise --apply` printing as it goes finishes
  the files in hand and exits as it would have
  (`a_reader_gone_is_noted_once_and_nothing_more_is_written`); `main.rs` denies
  `clippy::print_stdout` and `clippy::print_stderr`, so a `println!` or `eprintln!` fails the lint.
- **`resonate play` with nothing to play says so and exits 1** — `NoFileNamed` for no argument,
  `NothingPlayableNamed` where every argument was passed over (a sheet with no row, a cut that
  would not read) — rather than opening a transport with no rows and waiting for keys.
- **`resonate play` takes a key at a time where it is typed at.** Where stdin is a terminal,
  `input::KeyAtATime` switches it out of canonical mode and echo through `rustix`'s safe `termios`
  (`rustix` was already in the tree under zbus and libspa, so the feature is the whole cost), keeps
  `ISIG` so an interrupt is still a signal, and restores the terminal on drop; the signal fallback's
  `process::exit` goes through `signals::leave`, which restores it first, since a wedged front
  end's exit runs no destructor. A panic runs none either — the release profile aborts — so the
  first `KeyAtATime` installs a panic hook, once, that puts the saved modes back (through
  `try_lock`, a panic under the lock not deadlocking it) before the hook it replaced reports the
  panic. `input::keys` reads bytes a terminal read at a time, so an Escape ending a read is a lone
  Escape — the typed line cancelled that moment — while one with more behind it in the same read
  opens an arrow's sequence or an Alt chord
  (`a_lone_escape_cancels_the_line_before_the_next_key_arrives`). A letter acts when pressed, the
  arrows seek and turn the volume, and a digit or `:` starts a line drawn in the readout and read through
  the same `parse` a piped line is (`90` enter seeks, `:z track` sets the timer). Where stdout is a
  terminal too, `readout::Readout` redraws one line — the transport's glyph, position out of
  length, volume, shuffle, repeat, the sleep timer and what is being typed — every 500 ms sample
  and after every key, clearing it before anything else prints so an event or refusal is a line
  of its own above. Piped input is read a line at a time with no readout, as a script sends it.
- **A length is read one way wherever one is typed.** `lasting::lasting` takes a bare number in
  the unit its caller names — seconds for `play`'s seeks, minutes for a sleep spec — a clock
  (`1:30`, `1:02:03`, every field after the first under 60) or units coarsest first (`1h30m`,
  `45s`, `10mins`), and answers `None` for anything else, a sign or a count past `u64` included.
  `play`'s `f` and `r` take the key's direction and one leading sign's worth of slack (`f -45` is
  still forward), a seek past `i64::MAX` frames saturates rather than wrapping backwards, and an
  override that will not read — `f abc`, `+ 5%` — is refused as an unknown line rather than taking
  the step (`an_override_that_cannot_be_read_is_refused_rather_than_taking_the_step`). A sleep spec
  is `track`, `queue`, `off` or a non-zero length, kept to the second.
- **A headless pass stops at a file boundary on the first signal.** `resonate scan`, `enrich`,
  `poll`, `tag`, `organise` and `vault --import` run through `until_told`, which puts
  `signals::cancel_when_told` over the pass: the first `SIGINT` or `SIGTERM` calls the pass's
  `cancel`, so the file being written is finished, the catalog follows and the summary says
  `cancelled`; a second leaves at once. A cancelled pass then answers `Error::Cancelled { pass }`
  through `finished`, so the command exits 1 and a script can tell a pass cut short from one that
  ran out (`a_cancelled_pass_answers_an_error_so_the_command_exits_1`); a scan cancelled asks the
  reference nothing after it. The six handles are one `resonate_library::PassHandle` over
  each pass's progress and summary (`ScanHandle` and the rest are aliases) whose progress is
  `Cancelling`, and a thread that dies answers `Error::Stopped { pass }` naming its `PassKind`;
  `until_told` is generic over it, so a seventh pass is an alias and a `PassKind` variant, not a
  struct, an error and a macro arm.

## Output

**Text from a tag, a sheet or a service reaches the terminal as plain text.** `Table::push` passes
every cell through `table::on_one_line`, which turns a control character — an escape opening an OSC
sequence, a newline forging a row — and a bidirectional override into a space, so every table
(`stats`, `favourites`, `missing`, `playlists`, `players`, `studies`, `info`'s) is safe whatever
the catalog holds (`a_cell_carrying_a_control_or_a_reordering_mark_is_laid_on_one_plain_line`); every
line printed outside a table goes through `said!`'s plain text, which does the same but keeps the
newlines and tabs the line's own format wrote.

`resonate info` folds a raw tag value onto one line and cuts it at 72 characters
(`WIDEST_TAG_VALUE`), since a lyric tag runs to hundreds of lines and would wreck the table's
alignment; it reads a length through one clock that carries — rounded to the millisecond, so a
minute boundary rolls over rather than printing `1:60.000`, and saying so past an hour. A
*listening* total is drawn by a second clock, `stats::heard_for`, since a year rounded to the
millisecond is absurd; two readings, not one with a setting.

**Logs go to stderr, whatever the subcommand.** Stdout is what a subcommand answers — a table, a
link, `mcp`'s protocol — so `resonate share > link.txt` or a piped table holds no warning.

`RESONATE_LOG` is read through `EnvFilter::try_new` rather than `try_from_env`, so an unparsable
filter warns and falls back to `DEFAULT_LOG` where an unset variable silently takes it — the one
setting where a silent fallback would hide the diagnostics being reached for.

## Settings

Loaded from `$XDG_CONFIG_HOME/resonate/config.toml`, or `--config <FILE>`, which must exist where
the XDG path may not. A CLI flag outranks the file, the file outranks `EngineConfig`'s defaults,
and an unknown key warns through `tracing` rather than failing. Every key is a `ConfigKey` variant
(`ConfigKey::ALL` is the list), so a bad value names the key without prose in an error. **A value
that will not read costs its key alone.** `Config::take` reads one key and answers the typed
`ConfigType` or `ConfigValue` refusal; `parse` hands each to the caller's `refused`, and
`Config::read_from`'s warns and leaves the key at its default, so a pane renamed under `last-tab`
or a `window-size` from another build no longer fails every command — `sleep off` and `mcp`
included — while a file that is not TOML still does
(`a_value_that_will_not_read_is_left_at_its_default_and_the_rest_are_read`). The tests' `read`
collects the refusals, so each reader's refusal is still asserted. Nine of
the seventy-seven have a flag — `sink`, `library`, `vault`, `quality`, `filter-phase`, `dither`,
`noise-shaping`, `bit-perfect` (as `--bit-perfect` and `--no-bit-perfect`) and `volume` (as
`play --volume`, a percent); the other sixty-eight are set only by the settings pane and the file:

- the output's `true-peak`, `restore-lossy`, `replay-gain`, `replay-gain-pre-amp`,
  `replay-gain-untagged`, `dop`, `dsd-like-pcm`, `force-graph-rate`, `bluetooth-wake`,
  `bluetooth-lead-ms`, `bluetooth-awake-s`, `device-volume` and `buffer-ms`;
- the window's `theme`, `accent`, `text-size`, `minimise-button`, `maximise-button`,
  `scroll-volume`, `mouse-navigation`, `scrollbars`, `suggestions-tab`, `missing-tab`,
  `tab-counts`, `remember-tab`, `last-tab`, `remember-window-size`, `window-size`,
  `remember-settings-category` and
  `last-settings-category`, which no headless subcommand uses;
- the standing decisions: `online`, `enrich-after-scan`, `study`, `fetch-lyrics`,
  `identify-by-sound`, `contact`, `acoustid-key`, `equaliser`, `equaliser-for`,
  `equaliser-profile`, `convolution`, `resume`, `history-kept`, `skip-repeats-queue`,
  `previous-restarts`, `organise-as`, `notify`, `audd-token`, `listenbrainz-token`, `listen-from`,
  `listen-for`, `music-folder` and `file-dropped` (the Library category's *Primary music folder*
  group),
  `inbox` (the Library category's *The inbox* group, which polls from the window too),
  `subsonic`, `subsonic-user`, `subsonic-password` (its *A Subsonic server* group, used from
  the next start) and `tidal-client-id`, `tidal-client-secret`, `tidal-refresh-token` and
  `hifi-api` (its *A TIDAL account* group, likewise);
- the seven shaping a Discord presence — `discord`, `discord-app`, `discord-shows`, `discord-art`,
  `discord-icon`, `discord-progress`, `discord-paused` — written by the Desktop category's two
  Discord groups and live at once.

What some of them mean:

- `listen-from` is `desktop`, `microphone` or a microphone's node name; `listen-for` a whole number
  of seconds from 4 to 60, `cli::LISTENS_FOR_SECONDS`, which `--seconds`' value parser holds it to
  as well — `Recording::holding` lays the whole clip out up front, so an unbounded `--seconds`
  aborted on a large value (`a_length_to_listen_for_is_held_to_what_the_setting_takes`).
  `resonate listen --microphone` and `--seconds` outrank them for one run; the Online
  category's *Listening* group and the Listen sheet's chips write them.
- `bluetooth-lead-ms` is a whole number of milliseconds from 0 to 5 000 and `bluetooth-awake-s` of
  seconds from 0 to three hours (`BLUETOOTH_LEAD_MS`, `BLUETOOTH_AWAKE_S`), read through
  `At::within` as `listen-for` is, so a slipped digit is refused at startup rather than playing hours
  of silence before the first track or keeping headphones awake for days.
- `music-folder` is the one folder new songs are copied into — what is dragged onto the window
  (`ui.md`, `library.md`'s *Taking files in*), and what a provider delivers where no vault is open
  (`providers.md`), filed by `organise-as` — an ordinary folder of files and never the vault. It rides on `Stored` onto `ResonateApp::music_folder`, written by
  Library's *Primary music folder* and live at once; blank or absent it is `None`. Choosing one
  stores its canonical path, refuses a path that is not a folder or lies inside the vault
  (`unusable_as_the_music_folder`), and adds it to the roots through `add_roots` unless a root
  already reaches it, so what lands there is scanned like any other folder.
- `hifi-api` is an optional custom server address. Blank or absent, the hosted TIDAL HiFi service
  is used while `online` is on; a custom address replaces it from the next start. The *A TIDAL
  account* group's field writes it (`providers.md`).
- `file-dropped` (default true): what is dropped on the window is filed by `organise-as` once the
  scan has it, rather than left under the names and folders it came with. It rides on `Stored` onto
  `ResonateApp::file_dropped`, and the *Primary music folder* group's switch writes it.
- `history-kept` is `forever` (default) or a whole number of days; every command opening the
  catalog first forgets listens and skipped time older than that, and the Library category's
  *Listening history* chips write it and age the catalog, a shorter span on a second press.
- `previous-restarts` (default true): previous restarts the song once the heard position is past
  its first three seconds and goes back a track within them. Library's *The previous button*; live.
- `enrich-after-scan` (default true), read by `resonate scan` and the window's scan; off, the
  reference is asked only by `resonate enrich`, the Library card's *Enrich* and the Online card's
  *Look up*.
- `identify-by-sound` (default false): on, `online::fingerprinters` registers `ByEar` behind the
  AcoustID printer, so a lookup, `resonate analyse --recognise` and the Analysis pane name an
  otherwise unnameable track by twelve seconds of sound sent to Shazam as a signature. An
  `Arc<AtomicBool>` shared with the fingerprinter, written by Online's *Studying tracks*; live.
- `study` (default true), read by every lookup; off, no lookup starts the pool of studies, and a
  track is decoded only where the pane analyses it or the fingerprint route needs its print.
- `fetch-lyrics` (default true), read likewise; off, no lookup asks LRCLIB and a track's words are
  asked for only as it plays. Online's *Fetching lyrics*.
- `minimise-button` and `maximise-button` (default true), the window's alone:
  `Config::window_buttons` folds them into a `resonate_ui::WindowButtons` riding into `run` on
  `Stored` and living on `ResonateApp`, so an Appearance switch hides the button next frame; close
  is never one of them.
- `scroll-volume` (default true) rides on `Stored` onto `ResonateApp::scroll_volume`, written by
  Appearance's *The volume wheel*; while on, a wheel over the volume slider moves it a notch (5 %).
- `mouse-navigation` (default true) rides on `Stored` onto `ResonateApp::mouse_navigation`, written
  by Appearance's *Mouse navigation*; while on, the mouse side buttons move backward and forward
  through the album and artist pages opened in the window.
- `scrollbars` is a `resonate_core::ScrollbarMode` — `shown` (default), `auto-hide` or `hidden` —
  riding onto `ResonateApp::scrollbars`, written by Appearance's *Scrollbars*; hidden, no pane draws
  a bar and every region still scrolls. A file from when the key was a switch still reads, `true`
  as `shown`, `false` as `hidden`.
- `suggestions-tab` (default true), `missing-tab` and `tab-counts` (default false):
  `Config::tabs` folds them into a `resonate_ui::Tabs` riding onto `ResonateApp::tabs`, written by
  Appearance's *Sidebar tabs*. A tab that is off is left out of the sidebar and the pane keys,
  `RootView::set_pane` lands on the tracks wherever it is asked for, and the artist page's *not
  held* button into Missing is not drawn; with `tab-counts` off no tab draws its figure.
- `remember-tab`, `remember-window-size`, `remember-settings-category` (default true) each have a
  switch in Appearance's *Window state*. `last-tab` keeps the pane, `window-size` the windowed
  restore size as `widthxheight` (written after resizing settles), `last-settings-category` the
  category to reopen; turning a switch off removes only its saved value.
- `vault` has `--vault` (as `--library` outranks `library`) and defaults to
  `$XDG_DATA_HOME/resonate/vault`. A run not naming one opens the vault only where that folder
  exists, so a build nobody imported into creates nothing and carries no vault. **A vault is made
  only where it is asked for** — `made_where_asked`: the default place, or a path `--vault` names
  on this command line — through `Vault::make`; the `vault` key's path is opened with `Vault::open`,
  which refuses a root that is not there (`vault::Error::NotThere`) rather than making `audio/`,
  `covers/` and `staging/` under an empty mount point, so `resonate vault --import` against a vault
  on an unmounted drive says so instead of filling the wrong disc
  (`a_vault_opened_where_none_is_makes_nothing_there_and_one_made_is_opened_after`).

**The settings pane writes through `resonate_ui::Settings`**, which the binary fills with
`settings::File`. It edits the document with `toml_edit` rather than reserialising a parsed
`Config`, so a hand-written file keeps its comments and key order. A value written again is replaced
in place with its old decor carried over (`replaced_in_place`), so the comment after it on its line and
one above an `[equaliser-for]` entry stay, where replacing the item whole dropped them
(`a_value_written_again_keeps_the_comments_around_it`). `settings::File::apply` takes a
whole batch of `SettingChange`s through one `config::edit`, whose `Editing` stores and clears keys and
table entries on the one document and writes only if something moved; a setting the file cannot say
— a folder not in UTF-8 — is refused as `SettingNotStored` while the rest of its batch lands
(`a_batch_lands_whole_in_one_write_and_a_refused_setting_leaves_the_rest`). Every writer —
`config::store`, `clear`, `store_in_table`, `clear_in_table`, `edit` — follows a symlinked
`config.toml` to its file, holds a lock on the folder the file sits in for the whole
read-edit-write (so a window and a `resonate eq` beside it cannot write over each other, and no lock
file is left beside it), sweeps what a killed writer left there — a `config.toml.<pid>-<n>.new` and
the `config.toml.lock` older builds made, nothing else
(`what_a_killed_writer_left_is_swept_by_the_next_and_nothing_else`) — stages under a name of this
process's own,
creates the staged file `0600` and narrows the standing file's mode to its owner's bits before a
byte is written — the file holds `subsonic-password`, `tidal-client-secret`, `tidal-refresh-token`,
`audd-token` and `listenbrainz-token`, and one
made under the umask, or kept at a wider mode an older build left, was readable by every local user
(`the_settings_file_is_its_owners_alone_from_the_first_write_and_after_a_wider_one`) — `sync_all`s
the staged file and the folder around the rename, and renames it over the target so a crash cannot
truncate it. A value with a flag is spelled as the
flag spells it — `quality`, `dither` and `noise-shaping` go through the `ValueEnum`s `cli.rs`
declares — so `--noise-shaping flat` is `noise-shaping = "flat"`. `Setting::Theme` and
`Setting::Accent` take the same seam and never reach the engine, as do `Setting::Online` and
`Setting::Contact` from the Online card: a cleared contact is `config::clear` on the key, not an
empty string, which keeps `Config::contact` `None` and the User-Agent bare.
