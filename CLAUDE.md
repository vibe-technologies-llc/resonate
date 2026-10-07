# CLAUDE.md

Rules live in `.claude/rules/` and load where their `paths:` match:

| File | Scope | Covers |
|---|---|---|
| `rust-style.md` | always | formatting, imports, the no-comment rule, collections, sync primitives, text read from files, names built around a file's |
| `errors.md` | always | the `thiserror` architecture and its structural enforcement |
| `build.md` | `.github`, `.cargo`, `fuzz`, the root manifest, benches | CI, the profiles and `target-cpu`, the fuzz targets |
| `dependencies.md` | `**/Cargo.toml` | version pinning, crate layering, feature flags that are not optional |
| `binary.md` | the binary, the settings pane | the CLI grammar, file arguments and URIs, the MIME list, signals, the terminal, the settings decisions, the settings file |
| `realtime.md` | engine, pipewire, dsp, `listen::recording`, `core::rt` | the audio-callback contract |
| `audio.md` | engine, codec, dsp, pipewire | decode, the prescan guards, transport, queue, sleep, resumption, what is published, DSP, sink |
| `mpris.md` | resonate-mpris, the binary's `mpris.rs`, `starting.rs` | the bus, the track list, covers, the name, notifications, the idle inhibit, `Running` |
| `library.md` | library, mpris, the playlist panes, the binary's `playlists.rs` | schema, scan, enrichment, tag writing, deleting, organise, search, playlists, undo, sheets, statistics, suggestions, share |
| `lyrics.md` | resonate-lyrics, the lyrics model and pane, `sylt.rs`, `lrclib.rs`, `sung.rs` | the vocabulary, the provider seam, the LRC reader |
| `eq.md` | resonate-eq, `core::eq`, the DSP stage and convolver, `impulse.rs`, `autoeq.rs`, the equaliser panes | the vocabulary, the biquads, the per-sink binding, the formats, AutoEq |
| `ui.md` | resonate-ui | chrome, input, drawing, the palettes, panes |
| `motion.md` | resonate-ui | the motion vocabulary, what arrives and flips, the song handover, the scrollbar fade |
| `vault.md` | resonate-vault, `Library::import`, `vaulted.rs`, `supply.rs`, the Vault group | the forms, the keys, validation, what the catalog holds |
| `online.md` | resonate-online, the binary's `online.rs` | the paced client, the identity, what each service is asked and how its answer is read |
| `providers.md` | resonate-providers, providers/*, the binary's `providers.rs`, `supply.rs`, `filed.rs` | the seam, the registry, the inbox, Subsonic, TIDAL, hifi-api, Monochrome, what a delivery is and where it lands |
| `analysis.md` | resonate-analysis, `studies.rs`, `fingerprint.rs`, the analysis pane, `acoustid.rs`, `analyse.rs` | the one decode pass, the fake-lossless heuristic, the print, the studies and recognition |
| `mcp.md` | resonate-mcp, the binary's `mcp.rs` | the transport, refusals against failures, the tools, the resources and their seam |
| `discord.md` | resonate-discord, `core::presence`, the binary's `discord.rs`, the Desktop groups | the gate, the seam, the frame, what an activity says and how often |
| `packaging.md` | `packaging/**` | the Arch, Fedora and Flatpak payloads and their build baseline |

## Project

Resonate: a music player for audio enthusiasts, Linux first; high-fidelity, high-performance
playback is the guiding constraint.

- A **native PipeWire client** (not PulseAudio/ALSA compatibility) that reads the sink's advertised
  rates and **resamples itself at high quality**, so lossless sources stay bit-accurate where the
  hardware allows.
- A **GPUI** front end, native on Wayland.

`docs/TODO.md` is the roadmap, **open work only** (defects, gaps, what is not built): `## Category`
headings of `- Item` bullets, most to least important; `Next` (work chosen to come next) first,
`Defects` after, categories no listener waits on headed `Later:` below the rest. An item waiting on
something outside this tree is **Blocked on …** the blocker. Drop an item as it lands, add what the
work uncovers; what a landed item became goes into `.claude/rules/`.

`AGENTS.md` is this guidance for agents that do not load `.claude/rules/`. It points here rather
than restating the design, so it changes only, in the same commit, when a rules file or check is
added or a standing rule of how work is done moves.

## Architecture

Twenty-two crates. `resonate-core` is the only universal dependency; `resonate-codec`,
`resonate-dsp` and `resonate-pipewire` never depend on each other; `resonate-engine` joins them.

```
resonate            bin: CLI, tracing, wiring
  ├── resonate-ui         GPUI views, Wayland          [gated behind the `ui` feature]
  ├── resonate-mcp        catalog and running player over the Model Context Protocol  [`mcp`]
  ├── resonate-mpris      session bus: MPRIS, notifications, idle inhibit, playlists
  ├── resonate-discord    what is playing, told to a running Discord  [`discord`]
  ├── resonate-engine     transport, ring, pipeline policy  (also on analysis)
  │     ├── resonate-codec    decode to PCM  (symphonia)
  │     ├── resonate-dsp      resample, gain, dither
  │     └── resonate-pipewire sink enumeration, negotiation, stream
  ├── resonate-online     MusicBrainz, covers, portraits, lyrics, AutoEq  [`online`]
  ├── resonate-library    scan, tags, persistence  (also on codec, vault, analysis, providers)
  ├── resonate-lyrics     lyric vocabulary and the provider seam
  ├── resonate-vault      the managed archive: FLAC, WAVE+zstd, JXL covers
  ├── resonate-analysis   one whole decode: waveform, spectrum, verdict, loudness, Chromaprint
  ├── resonate-listen     capture the desktop or a microphone, and the recogniser seam
  ├── resonate-eq         profile formats, the profile store, the AutoEq catalogue and its seam
  ├── resonate-providers  the provider seam: an identity in, media out  (filled by providers/*)
  ├── providers/inbox     resonate-inbox, a folder of the listener's
  ├── providers/monochrome resonate-monochrome, a Monochrome track streamer  [`online`]
  ├── providers/subsonic  resonate-subsonic, a Subsonic server  [`online`]
  ├── providers/tidal     resonate-tidal, the listener's TIDAL subscription  [`online`]
  └── resonate-core       domain vocabulary
```

The tree says what each crate is *for*, not every edge: `cargo tree` is the authority, and the
`layering` job in `.github/workflows/ci.yml` holds the refusals. Edges worth knowing: only the
binary (and the detached `fuzz/`, for `resonate-mcp`) reaches `resonate-online`, `resonate-mcp` and
`resonate-discord`; a provider crate reaches the seam and core alone; `resonate-listen` reaches core
and `resonate-pipewire` alone (its own PipeWire client per recording, never the engine's
single-stream slot); `resonate-analysis` reaches `resonate-dsp`.

Invariants the layering protects (the rules files have the rest):

- **A playable item is a `MediaLocation`, never a path.** Core carries a `SourceId` (interned
  `Arc<str>`) and a `Locator`. `resonate-codec::Sources` turns a location into bytes through
  `MediaProvider::open`; `LocalFiles` is the local provider (`VaultFiles` replacing it where a vault
  is open). `probe`, `probe_stream`, `probe_boxes` and `Decoder::open` take the registry and the
  location; every `codec::Error` about a file names the location (`Error::location`;
  `SeekOutOfRange` and `Domain` carry none). `Player::with_sources` puts one registry behind engine
  and tag catalog. `MediaLocation::{from_uri, to_uri}` are in core: the bus and the command line
  both read URIs.
- **`resonate-library` is the local source's catalog, not every source's.** Nothing is keyed by
  source; another source brings its own catalog, its queue rows read through `Player::media`.
- **An artist is one artist however spelled.** `folded_letters` (core) folds `artists.key`,
  `tracks_fts` and a typed search word (`library.md`).
- **The engine reaches the graph through `Backend`, never `PipeWire`.** `Player::new` starts the
  real client; `Player::with_backend` takes any other, so
  `crates/resonate-engine/tests/transport.rs` drives a whole transport with no daemon.
- **`resonate-mpris` sees the engine only through `Player` and the front end only through `Host`**,
  so one service serves `resonate play` and the window, and a second `resonate` reaches a first
  through it (`mpris.md`).
- **The CLI grammar is written once, read three times.** `cli.rs` is the clap derive alone and
  `build.rs` writes the man pages and completions from it, so `cli.rs` names no workspace crate.
  **What is offered is what decodes**: `MIME_TYPES`, the desktop entry and the metainfo agree, held
  by tests; `AUDIO_EXTENSIONS` (core) is what a scan admits (`binary.md`).
- **The network is behind `Reference`; the library owns the seam.** `Library::enrich` walks the
  trait, `resonate-online`'s `Online` is the one implementation, so `--exclude resonate-ui
  --no-default-features` builds with no HTTP client. Same shape for `Scrobbler`, `Fingerprints`,
  `Corrections` and `LyricProvider`.
- **A request says what this build is and nothing else unless the listener says otherwise.**
  User-Agent `resonate/<version>`; a contact, AcoustID key, AudD token or ListenBrainz token is sent
  only where typed into a setting, each empty by default; `online = false` stops every request
  (`online.md`).
- **A guess is never written.** Enrichment takes only strict matches; the grouping key never follows
  one (`library.md`).
- **Stored formats are migrated, not broken.** A schema change is a step appended to `MIGRATIONS`;
  `V1` and existing steps are never edited (`library.md`).
- **What identifies a recording is core vocabulary**, since a provider crate may not see the
  library: `Mbid`, `Isrc`, `Link`, `Relation`, `Service` (the host a link points at; *provider*
  means only a plugin that obtains media).
- **A run of rows is one `resonate-core::Span`**, never empty; `Span::landing` is the one arithmetic
  of where a dropped span lands. `Library::remove_from_playlist`, `move_in_playlist`,
  `Command::Remove` and `Command::Move` take one. `resonate-ui`'s `edit::Span` is a different type,
  a run of text edits.
- **Core also carries what two mutually blind crates share**: `Resumption`, `Reordered`,
  `Appearance`, `CivilDate`, `Calendar`, `eq`'s arithmetic, `presence`, `naming`.
- **Everything else is a crate behind its own seam**: tag writing through `TagSink`, the vault,
  lyrics, providers, studies, the equaliser's I/O, Discord.

## Commands

- `cargo build|test --workspace --exclude resonate-ui --no-default-features`: audio stack only, the
  usual inner loop; the only way to keep gpui out. `resonate-ui` stays a default member so a bare
  `cargo run` opens the window.
- `cargo build|test --workspace`: everything, gpui included.
- `cargo test -p <crate> <test_name> -- --exact --nocapture`: one test.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `rust-formatter` formats, `rust-formatter --check` checks; never `cargo fmt`. CI cannot run it.
- `RESONATE_BENCH_CEILINGS=1 cargo bench -p resonate-dsp --bench stages [<words>]`: every DSP stage
  against `benches/ceilings.tsv` (four times the baseline CPU's cost, never under 0.05 % of a core).
  A stage added to the bench is added to the file.
- `cargo bench -p resonate-library --bench spelling`: times the search vocabulary of a synthetic
  500 000-track catalog; asserts nothing.
- `cargo build --profile profiling`: release with symbols for `perf`/`cargo flamegraph`.
- Tests needing the outside world skip without it: `resonate-pipewire` (`stream`, `reconnect`,
  `formats`; the last needs `pw-cli`), `resonate-listen` (`capture`, `reconnect`, `microphone`; the
  last two need `pw-cli`), `resonate-mpris --test bus` (session bus), `resonate-codec --test
  encoded` (ffmpeg, `metaflac`, `wavpack`, `mac`), `resonate-library --test library` (ffmpeg),
  `resonate-online --test live` (`RESONATE_ONLINE_TESTS`), `resonate --test terminal` (`setsid`, a
  pseudo-terminal). The reconnect, `formats` and
  `microphone` tests host their own daemon; the mpris notification and headset presses run under
  `dbus-run-session`. `resonate-subsonic`, `resonate-tidal` and `resonate-monochrome` serve fake
  servers on loopback.
- `cd fuzz && cargo +nightly fuzz build`, and `cargo +nightly fuzz run <target> corpus/<target>
  seeds/<target> -- -max_total_time=180 -timeout=15` (`build.md`).

CI runs exactly this list; the debug profile is optimised and `target-cpu=native` is set (both in
`build.md`).

The binary doubles as the audio stack's test harness; `cargo run -- <subcommand> --help` is the
grammar (`binary.md`). By area:

- **Output:** `sinks`, `explain <file>` (the `OutputPlan`), `info <file>` (tags, ReplayGain, box
  order, bitrate), `analyse <file>` (whole-decode study; `--recognise` asks AcoustID), `listen`
  (records the desktop or a microphone and names the song), `eq`.
- **Playing:** `play <files>` (transport keys on stdin), `queue`, `players`, `sleep`, `share`,
  `mcp`.
- **Catalog:** `scan`, `roots`, `forget`, `enrich`, `tag`, `organise`, `vault`, `studies`, `stats`,
  `favourites`, `suggest`, `playlists`, `playlist`, `import`.
- **Wants:** `wants`, `missing`, `poll`, `tidal` (the device sign-in), `lastfm` (the scrobbling
  sign-in).
- `cargo run -- <files>` opens the window with those queued (`Exec=resonate %U`); a bare `cargo run`
  opens it on the queue the last run left, unless `resume` is off.

Settings load from `config.toml` or `--config`; a flag outranks the file, which outranks the
defaults; the settings pane writes back through the same file. The agent shell does not inherit the
desktop session, so a bare `cargo run` cannot open a window: the `run-ui` skill has the recipe.
