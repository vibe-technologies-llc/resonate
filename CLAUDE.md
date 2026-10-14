# CLAUDE.md

Rules live in `.claude/rules/` and load where their `paths:` match:

| File | Scope | Covers |
|---|---|---|
| `rust-style.md` | always | formatting, imports, the no-comment rule, collections, sync primitives, text read from files, names built around a file's, a file another run may sweep |
| `errors.md` | always | the `thiserror` architecture and its structural enforcement |
| `build.md` | `.github`, `.cargo`, `fuzz`, the root manifest, benches | CI, the profiles and `target-cpu`, the fuzz targets |
| `android.md` | Android Gradle project, the mobile boundary and binding tool | Compose, native build, documents, service lifecycle, emulator checks, and the boundary behaviour both phones share |
| `mobile.md` | resonate-mobile, resonate-bindgen, both `build-native.py` | the unsafe exception, one surface on every target, granted-document schemes, the output per target |
| `ios.md` | `ios/**`, the RemoteIO output | the xtool toolchain, the native library and bindings, hardening, the shape of the SwiftUI app |
| `dependencies.md` | `**/Cargo.toml` | version pinning, crate layering, feature flags that are not optional |
| `binary.md` | the binary, the settings pane | the CLI grammar, file arguments and URIs, the MIME list, signals, the terminal, the settings decisions, the settings file |
| `realtime.md` | engine, pipewire, the Oboe and RemoteIO callbacks, dsp, `listen::recording`, `core::rt` | the audio-callback contract |
| `audio.md` | engine, codec, dsp, pipewire | decode, the prescan guards, transport, queue, sleep, resumption, what is published, DSP, sink |
| `mpris.md` | resonate-mpris, the binary's `mpris.rs`, `starting.rs` | the bus, the track list, covers, the name, notifications, the idle inhibit, `Running` |
| `library.md` | library, mpris, the playlist panes, the binary's `playlists.rs` | schema, scan, enrichment, tag writing, deleting, organise, search, playlists, undo, sheets, statistics, suggestions, share |
| `lyrics.md` | resonate-lyrics, the lyrics model and pane, `sylt.rs`, `lrclib.rs`, `sung.rs` | the vocabulary, the provider seam, the LRC reader |
| `eq.md` | resonate-eq, `core::eq`, the DSP stage and convolver, `impulse.rs`, `autoeq.rs`, the equaliser panes | the vocabulary, the biquads, the per-sink binding, the formats, AutoEq |
| `ui.md` | resonate-ui | chrome, input, drawing, the palettes, panes |
| `motion.md` | resonate-ui | the motion vocabulary, what arrives and flips, the song handover, the scrollbar fade |
| `vault.md` | resonate-vault, `Library::import`, `vaulted.rs`, `supply.rs`, the Vault group | the forms, the keys, validation, what the catalog holds |
| `online.md` | resonate-online, the binary's `online.rs` | the paced client, the identity, what each service is asked and how its answer is read |
| `providers.md` | resonate-providers, resonate-fetch, providers/*, the binary's `providers.rs`, `supply.rs`, `filed.rs` | the seam, the registry, the inbox, Subsonic, TIDAL, hifi-api, Monochrome, what a delivery is and where it lands |
| `analysis.md` | resonate-analysis, `studies.rs`, `fingerprint.rs`, the analysis pane, `acoustid.rs`, `analyse.rs` | the one decode pass, the fake-lossless heuristic, the print, the studies and recognition |
| `mcp.md` | resonate-mcp, the binary's `mcp.rs` | the transport, refusals against failures, the tools, the resources and their seam |
| `sync.md` | resonate-sync, the binary's `sync.rs`, `synced.rs`, the sync panes | the pairing key and what it derives, the wire, the ledger, the seam, peers and transfers, devices meeting on one network |
| `server.md` | resonate-server, `packaging/server/**` | what the server keeps, sessions, presence, punching, limits, deployment |
| `discord.md` | resonate-discord, `core::presence`, the binary's `discord.rs`, the Desktop groups | the gate, the seam, the frame, what an activity says and how often |
| `packaging.md` | `packaging/**` | the Arch, Fedora and Flatpak payloads and their build baseline |

## Project

Resonate: a music player for audio enthusiasts, Linux first; high-fidelity, high-performance
playback is the guiding constraint.

- A **native PipeWire client** (not PulseAudio/ALSA compatibility) that reads the sink's advertised
  rates and **resamples itself at high quality**, so lossless sources stay bit-accurate where the
  hardware allows.
- A **GPUI** front end, native on Wayland.
- An **Android 16+ Kotlin/Jetpack Compose** front end, with the same Rust playback engine,
  opened and run from the `android/` project in Android Studio.
- An **iOS 26+ SwiftUI** front end on the same engine and boundary, built, signed and installed from
  Linux with xtool from the `ios/` package.

`docs/ROADMAP.md` is the roadmap, **open work only** (defects, gaps, what is not built): it points
to `BUGS.md`, `FEATURES.md`, `PERFORMANCE.md` and `TESTING.md` beside it, each of `## Category`
headings of `- [S] Item` bullets, most to least important. The bracket prefix is the complexity
(`S`, `M`, `L`) and, where the entry waits on something outside this tree, `· blocked: <blocker>`;
blocked entries sit last in their category. `Next` (work chosen to come next) is first, in
`ROADMAP.md`; categories no listener waits on are headed `Later:` below the rest. Drop an item as it
lands, add what the work uncovers; what a landed item became goes into `.claude/rules/`.

`AGENTS.md` is this guidance for agents that do not load `.claude/rules/`. It points here rather
than restating the design, so it changes only, in the same commit, when a rules file or check is
added or a standing rule of how work is done moves.

## Architecture

Twenty-eight crates. `resonate-core` is the only universal dependency; `resonate-codec`,
`resonate-dsp` and `resonate-pipewire` never depend on each other; `resonate-engine` joins them.

```
resonate            bin: CLI, tracing, wiring
  ├── resonate-ui         GPUI views, Wayland          [`ui` feature]
  ├── resonate-mcp        catalog and player over MCP  [`mcp`]
  ├── resonate-mpris      session bus: MPRIS, notifications, idle inhibit, playlists
  ├── resonate-discord    what is playing, told to Discord  [`discord`]
  ├── resonate-sync       devices kept in step: sealed records, QUIC peers, LAN beacons  [`sync`]
  ├── resonate-engine     transport, ring, pipeline policy  (also on analysis)
  │     ├── resonate-codec    decode to PCM  (symphonia)
  │     ├── resonate-dsp      resample, gain, dither
  │     ├── resonate-audio    backend contracts, sink vocabulary and streams
  │     └── resonate-pipewire Linux sink enumeration, negotiation, stream
  ├── resonate-online     MusicBrainz, covers, portraits, lyrics, AutoEq  [`online`]
  ├── resonate-library    scan, tags, persistence  (also on codec, vault, analysis, providers)
  ├── resonate-lyrics     lyric vocabulary and the provider seam
  ├── resonate-vault      the managed archive: FLAC, WAVE+zstd, JXL covers
  ├── resonate-analysis   one whole decode: waveform, spectrum, verdict, loudness, Chromaprint
  ├── resonate-listen     capture the desktop or a microphone, and the recogniser seam
  ├── resonate-eq         profile formats, profile store, AutoEq catalogue and its seam
  ├── resonate-providers  the provider seam: an identity in, media out  (filled by providers/*)
  ├── resonate-fetch      what network providers share: agents, trust, Retry-After, resuming
  ├── providers/inbox     resonate-inbox, a folder of the listener's
  ├── providers/monochrome resonate-monochrome  [`online`]
  ├── providers/subsonic  resonate-subsonic  [`online`]
  ├── providers/tidal     resonate-tidal, the listener's subscription  [`online`]
  └── resonate-core       domain vocabulary

resonate-mobile      cdylib/staticlib: the phones' boundary (Kotlin and Swift) and document catalog, over playback, analysis, lyrics, library seams, online, providers and sync
resonate-bindgen     bin: host tool generating Kotlin and Swift from native metadata
resonate-server      bin: keeps sealed sync records and introduces devices, over resonate-sync's wire
```

`cargo tree` is the authority on edges and the `layering` job in `.github/workflows/ci.yml` holds
the refusals. Worth knowing: only the binary (and the detached `fuzz/`, for `resonate-mcp`) reaches
`resonate-mcp` and `resonate-discord`; `resonate-online` is composed by the binary and `resonate-mobile`; a provider crate reaches the seam, core and
(over the network) `resonate-fetch` alone; `resonate-listen` reaches core, `resonate-audio` and `resonate-pipewire`
alone (its own PipeWire client per recording; capture modules only on Linux); `resonate-analysis` reaches `resonate-dsp`;
`resonate-sync` reaches core alone and is reached by the binary, `resonate-mobile` and the server.

Invariants the layering protects (the rules files have the rest):

- **A playable item is a `MediaLocation`, never a path.** Core carries a `SourceId` and a `Locator`;
  `resonate-codec::Sources` turns a location into bytes through `MediaProvider::open` (`LocalFiles`,
  `VaultFiles` where a vault is open). `probe*` and `Decoder::open` take the registry and the
  location; every `codec::Error` about a file names it (`Error::location`). `Player::with_sources`
  puts one registry behind engine and tag catalog. `MediaLocation::{from_uri, to_uri}` are in core.
- **`resonate-library` is the local source's catalog, not every source's**: nothing is keyed by
  source; another source brings its own catalog, its queue rows read through `Player::media`.
- **An artist is one artist however spelled**: `folded_letters` (core) folds `artists.key`,
  `tracks_fts` and a typed search word.
- **The engine reaches the graph through `resonate-audio::Backend`, never `PipeWire`**: `Player::with_backend` takes
  any other, so `crates/resonate-engine/tests/transport.rs` drives a transport with no daemon.
- **`resonate-mpris` sees the engine only through `Player` and the front end only through `Host`**,
  so one service serves `resonate play` and the window (`mpris.md`).
- **The CLI grammar is written once, read three times**: `cli.rs` is the clap derive alone and
  `build.rs` writes man pages and completions from it. **What is offered is what decodes**:
  `MIME_TYPES`, the desktop entry and the metainfo agree, held by tests; `AUDIO_EXTENSIONS` (core)
  is what a scan admits.
- **The network is behind `Reference`; the library owns the seam**: `Online` implements it, so
  `cargo tree -p resonate --no-default-features` builds with no HTTP client. Same for `Scrobbler`,
  `Fingerprints`, `Corrections`, `LyricProvider`.
- **A request says what this build is and nothing else**: User-Agent `resonate/<version>`; a contact,
  AcoustID key, AudD or ListenBrainz token is sent only where typed into a setting, each empty by
  default; `online = false` stops every request (`online.md`).
- **A guess is never written**: enrichment takes strict matches only; the grouping key never follows
  one.
- **Stored formats are migrated, not broken**: a schema change appends a step to `MIGRATIONS`; `V1`
  and existing steps are never edited.
- **What identifies a recording is core vocabulary** (`Mbid`, `Isrc`, `Link`, `Relation`, `Service`)
  since a provider crate may not see the library; *provider* means only a plugin that obtains media.
- **A run of rows is one `resonate-core::Span`**, never empty; `Span::landing` is the one arithmetic
  of where a dropped span lands. `resonate-ui`'s `edit::Span` is a different type (text edits).
- **Core carries what two mutually blind crates share**: `Resumption`, `Reordered`, `Appearance`,
  `CivilDate`, `Calendar`, `eq`'s arithmetic, `presence`, `naming`, `titles`, `words_of_a_name`.
- **The sync server never reads what it keeps**: every record is sealed under keys the pairing
  code derives, songs travel device to device, and a device names itself only inside the sealed
  records (`sync.md`).
- **Everything else is a crate behind its own seam**: tag writing through `TagSink`, the vault,
  lyrics, providers, studies, the equaliser's I/O, Discord.

## Commands

- `android/gradlew -p android :app:assembleDebug :app:lintDebug`: Android APK and Kotlin/manifest
  checks; `python3 android/check-native.py --sdk "$ANDROID_HOME"` checks native alignment and
  Android Rust clippy; `android/gradlew -p android :app:connectedDebugAndroidTest`: native playback and
  resumption on an attached emulator/device. `:app:installDebug` installs the runnable app.

- `python3 ios/build-native.py`, then `cd ios && xtool dev build --ipa`: the iOS staticlib, Swift
  bindings and app package; `xtool dev run` installs on an attached iPhone (`run-ios` skill). Not in
  CI: the Darwin SDK comes from the listener's Xcode download.

- `cargo build|test --workspace --exclude resonate-ui --no-default-features`: the workspace without GPUI, the
  usual inner loop (the only way to keep gpui out); `resonate-ui` is a default member so a bare
  `cargo run` opens the window.
- `cargo build|test --workspace`: everything. `cargo test -p <crate> <test> -- --exact --nocapture`:
  one test. `cargo clippy --workspace --all-targets -- -D warnings`.
- `rust-formatter` formats, `rust-formatter --check` checks; never `cargo fmt`. CI cannot run it.
- `RESONATE_BENCH_CEILINGS=1 cargo bench -p resonate-dsp --bench stages [<words>]`: every DSP stage
  against `benches/ceilings.tsv`; a stage added to the bench is added to the file.
  `cargo bench -p resonate-library --bench spelling` times search on a synthetic 500 000-track
  catalog. `cargo build --profile profiling`: release with symbols.
- Tests needing the outside world skip without it: `resonate-pipewire` (`stream`, `reconnect`,
  `formats`), `resonate-listen` (`capture`, `reconnect`, `microphone`), the reconnect, `formats` and
  `microphone` tests need `pw-cli` and host their own daemon; `resonate-mpris --test bus` (session
  bus; notification and headset presses run under `dbus-run-session`); `resonate-codec --test
  encoded` (ffmpeg, `metaflac`, `wavpack`, `mac`, `lame`); `resonate-library --test library`
  (ffmpeg); `resonate-online --test live` (`RESONATE_ONLINE_TESTS`); `resonate --test terminal`
  (`setsid`, a pseudo-terminal). `resonate-subsonic`, `-tidal` and `-monochrome` serve fake servers
  on loopback.
- `cargo run -p resonate-server -- --help`: the sync server. `cargo test -p resonate-server` runs
  it in-process with two clients over loopback (playlists, favourites, a peer fetch, a stranger's
  key); `cargo test -p resonate-sync` holds the ledger, wire and sealing.
- `cd fuzz && cargo +nightly fuzz build`, then `cargo +nightly fuzz run <target> corpus/<target>
  seeds/<target> -- -max_total_time=180 -timeout=15` (`build.md`).

CI runs exactly this list, iOS aside; the debug profile is optimised and `target-cpu=native` is set
(`build.md`).

`cargo run -- <subcommand> --help` is the grammar (`binary.md`); the binary doubles as the audio
stack's test harness:

- **Output:** `sinks`, `explain`, `info`, `analyse` (`--recognise` asks AcoustID), `listen`, `eq`.
- **Playing:** `play <files>` (transport keys on stdin), `queue`, `players`, `sleep`, `share`, `mcp`.
- **Catalog:** `scan`, `roots`, `forget`, `enrich`, `tag`, `organise`, `vault`, `studies`, `stats`,
  `favourites`, `suggest`, `playlists`, `playlist`, `import`.
- **Wants:** `wants`, `missing`, `poll`, `tidal` (device sign-in), `lastfm` (scrobbling sign-in).
- **Devices:** `sync` (pairing code and devices; `--join`, `--leave`, `--fetch --from`, `--handoff`).
- `cargo run -- <files>` opens the window with those queued; a bare `cargo run` opens it on the
  last queue unless `resume` is off.

Settings load from `config.toml` or `--config`; a flag outranks the file, which outranks the
defaults; the settings pane writes back through the same file. The agent shell does not inherit the
desktop session, so a bare `cargo run` cannot open a window: the `run-ui` skill has the recipe.
