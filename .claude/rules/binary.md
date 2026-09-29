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
`--dither`, `--noise-shaping` and `--no-bit-perfect` are `global`, reading the same before and
after the subcommand.

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
key is held to UTF-8. An argument is read as another source's URI only where its scheme names a
source this build's `Sources` holds, so `01:intro.flac` is a file here, not a key under a source
called `01`.

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
`.cue` on the command line goes through (`sheet_cuts` beside it).

## What this build advertises

**The desktop entry, the bus and the scan advertise what this build can decode, and nothing else.**
`MIME_TYPES` is what `SupportedMimeTypes` answers, what `packaging/resonate.desktop` declares and
what the metainfo's `<provides>` names, held to both in both directions by a test each;
`resonate-library`'s `AUDIO_EXTENSIONS` names the same formats. WavPack is offered through
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
- **`resonate play` takes a key at a time where it is typed at.** Where stdin is a terminal,
  `input::KeyAtATime` switches it out of canonical mode and echo through `rustix`'s safe `termios`
  (`rustix` was already in the tree under zbus and libspa, so the feature is the whole cost), keeps
  `ISIG` so an interrupt is still a signal, and restores the terminal on drop; the signal fallback's
  `process::exit` goes through `signals::leave`, which restores it first, since a wedged front
  end's exit runs no destructor. A panic runs none either — the release profile aborts — so the
  first `KeyAtATime` installs a panic hook, once, that puts the saved modes back (through
  `try_lock`, a panic under the lock not deadlocking it) before the hook it replaced reports the
  panic. `input::keys` reads bytes: a letter acts when pressed, the arrows
  seek and turn the volume, and a digit or `:` starts a line drawn in the readout and read through
  the same `parse` a piped line is (`90` enter seeks, `:z track` sets the timer). Where stdout is a
  terminal too, `readout::Readout` redraws one line — the transport's glyph, position out of
  length, volume, shuffle, repeat, the sleep timer and what is being typed — every 500 ms sample
  and after every key, clearing it before anything else prints so an event or refusal is a line
  of its own above. Piped input is read a line at a time with no readout, as a script sends it.
- **A headless pass stops at a file boundary on the first signal.** `resonate scan`, `enrich`,
  `poll`, `tag`, `organise` and `vault --import` run through `until_told`, which puts
  `signals::cancel_when_told` over the pass: the first `SIGINT` or `SIGTERM` calls the pass's
  `cancel`, so the file being written is finished, the catalog follows and the summary says
  `cancelled`; a second leaves at once. The six handles are one `resonate_library::PassHandle` over
  each pass's progress and summary (`ScanHandle` and the rest are aliases) whose progress is
  `Cancelling`, and a thread that dies answers `Error::Stopped { pass }` naming its `PassKind`;
  `until_told` is generic over it, so a seventh pass is an alias and a `PassKind` variant, not a
  struct, an error and a macro arm.

## Output

**Text from a tag, a sheet or a service reaches the terminal as plain text.** `Table::push` passes
every cell through `table::on_one_line`, which turns a control character — an escape opening an OSC
sequence, a newline forging a row — and a bidirectional override into a space, so every table
(`stats`, `favourites`, `missing`, `playlists`, `players`, `studies`, `info`'s) is safe whatever
the catalog holds; a line printed outside a table from such text, as `studies --take`'s, goes
through it too (`a_cell_carrying_a_control_or_a_reordering_mark_is_laid_on_one_plain_line`).

`resonate info` folds a raw tag value onto one line and cuts it at 72 characters
(`WIDEST_TAG_VALUE`), since a lyric tag runs to hundreds of lines and would wreck the table's
alignment; it reads a length through one clock that carries — rounded to the millisecond, so a
minute boundary rolls over rather than printing `1:60.000`, and saying so past an hour. A
*listening* total is drawn by a second clock, `stats::heard_for`, since a year rounded to the
millisecond is absurd; two readings, not one with a setting.

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
collects the refusals, so each reader's refusal is still asserted. Eight of
the seventy have a flag — `sink`, `library`, `vault`, `quality`, `filter-phase`, `dither`,
`noise-shaping` and `bit-perfect` (as `--no-bit-perfect`); the other sixty-two are set only by the
settings pane and the file:

- the output's `true-peak`, `restore-lossy`, `replay-gain`, `replay-gain-pre-amp`,
  `replay-gain-untagged`, `dop`, `dsd-like-pcm`, `force-graph-rate`, `bluetooth-wake`,
  `bluetooth-lead-ms`, `bluetooth-awake-s`, `device-volume`, `volume` and `buffer-ms`;
- the window's `theme`, `accent`, `text-size`, `minimise-button`, `maximise-button`,
  `scroll-volume`, `scrollbars`, `suggestions-tab`, `missing-tab`, `tab-counts`, `remember-tab`,
  `last-tab`, `remember-window-size`, `window-size`, `remember-settings-category` and
  `last-settings-category`, which no headless subcommand uses;
- the standing decisions: `online`, `enrich-after-scan`, `study`, `fetch-lyrics`,
  `identify-by-sound`, `contact`, `acoustid-key`, `equaliser`, `equaliser-for`,
  `equaliser-profile`, `convolution`, `resume`, `history-kept`, `skip-repeats-queue`,
  `previous-restarts`, `organise-as`, `notify`, `audd-token`, `listenbrainz-token`, `listen-from`,
  `listen-for`, `inbox` (the Library category's *The inbox* group, which polls from the window too),
  and `subsonic`, `subsonic-user`, `subsonic-password` (its *A Subsonic server* group, used from
  the next start);
- the seven shaping a Discord presence — `discord`, `discord-app`, `discord-shows`, `discord-art`,
  `discord-icon`, `discord-progress`, `discord-paused` — written by the Desktop category's two
  Discord groups and live at once.

What some of them mean:

- `listen-from` is `desktop`, `microphone` or a microphone's node name; `listen-for` a whole number
  of seconds. `resonate listen --microphone` and `--seconds` outrank them for one run; the Online
  category's *Listening* group and the Listen sheet's chips write them.
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
  exists, so a build nobody imported into creates nothing and carries no vault.

**The settings pane writes through `resonate_ui::Settings`**, which the binary fills with
`settings::File`. It edits the document with `toml_edit` rather than reserialising a parsed
`Config`, so a hand-written file keeps its comments and key order. Every writer — `config::store`,
`clear`, `store_in_table`, `clear_in_table` — goes through the private `edited`, which follows a symlinked
`config.toml` to its file, holds `config.toml.lock` for the whole read-edit-write (so a window and a
`resonate eq` beside it cannot write over each other), stages under a name of this process's own,
creates the staged file `0600` and narrows the standing file's mode to its owner's bits before a
byte is written — the file holds `subsonic-password`, `audd-token` and `listenbrainz-token`, and one
made under the umask, or kept at a wider mode an older build left, was readable by every local user
(`the_settings_file_is_its_owners_alone_from_the_first_write_and_after_a_wider_one`) — `sync_all`s
the staged file and the folder around the rename, and renames it over the target so a crash cannot
truncate it. A value with a flag is spelled as the
flag spells it — `quality`, `dither` and `noise-shaping` go through the `ValueEnum`s `cli.rs`
declares — so `--noise-shaping flat` is `noise-shaping = "flat"`. `Setting::Theme` and
`Setting::Accent` take the same seam and never reach the engine, as do `Setting::Online` and
`Setting::Contact` from the Online card: a cleared contact is `config::clear` on the key, not an
empty string, which keeps `Config::contact` `None` and the User-Agent bare.
