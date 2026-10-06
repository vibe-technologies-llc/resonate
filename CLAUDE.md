# CLAUDE.md

Guidance for Claude Code in this repository. Detailed rules live in `.claude/rules/` and load
automatically where their `paths:` match:

| File | Scope | Covers |
|---|---|---|
| `rust-style.md` | always | formatting, imports, the no-comment rule, collections, sync primitives, text read from files |
| `errors.md` | always | the `thiserror` architecture and its structural enforcement |
| `dependencies.md` | `**/Cargo.toml` | version pinning, crate layering, feature flags that are not optional |
| `binary.md` | the binary, the settings pane | the CLI grammar, file arguments and URIs, the MIME list, signals, the terminal, the config keys, the settings file |
| `realtime.md` | engine, pipewire, dsp | the audio-callback contract |
| `audio.md` | engine, codec, dsp, pipewire | decode, the prescan guards, transport, queue, sleep, resumption, what is published, DSP, sink |
| `mpris.md` | resonate-mpris, the binary's `mpris.rs` | the bus, the track list, covers, the name, notifications, the idle inhibit, `Running` |
| `library.md` | library, mpris, the playlist panes | schema, scan, enrichment, tag writing, deleting, organise, search, playlists, undo, sheets, statistics, suggestions, share |
| `lyrics.md` | resonate-lyrics, the lyrics model and pane | the vocabulary, the provider seam, the LRC reader |
| `eq.md` | resonate-eq, `core::eq`, the DSP stage, the equaliser pane | the vocabulary, the biquads, the per-sink binding, the formats, AutoEq |
| `ui.md` | resonate-ui | chrome, input, drawing, the palettes, panes |
| `motion.md` | resonate-ui | the motion vocabulary, what arrives and flips, the song handover, the scrollbar fade |
| `vault.md` | resonate-vault, `Library::import`, the Vault group | the forms, the keys, validation, what the catalog holds |
| `online.md` | resonate-online, the binary's `online.rs` | the paced client, the identity, what each service is asked and how its answer is read |
| `providers.md` | resonate-providers, providers/*, `Library::poll` | the seam, the registry, the inbox, Subsonic, TIDAL, what a delivery is and where it lands |
| `analysis.md` | resonate-analysis, `studies.rs`, the analysis pane, `acoustid.rs` | the one decode pass, the fake-lossless heuristic, the print, the studies and recognition |
| `mcp.md` | resonate-mcp, the binary's `mcp.rs` | the transport, refusals against failures, the tools, the resources and their seam |
| `discord.md` | resonate-discord, `core::presence`, the binary's `discord.rs`, the Desktop groups | the gate, the seam, the frame, what an activity says and how often |
| `packaging.md` | `packaging/**` | the Arch, Fedora and Flatpak payloads and their build baseline |

## Project

Resonate is a music player for audio enthusiasts, Linux first, with high-fidelity,
high-performance playback as the guiding constraint:

- A **native PipeWire client** (not via PulseAudio/ALSA compatibility) that reads the sink's
  advertised rates and **resamples itself at high quality** before handing the stream to the graph,
  so lossless sources stay bit-accurate where the hardware allows.
- A **GPUI** front end running natively on Wayland.

`docs/TODO.md` is the roadmap and holds **open work only** (defects, gaps, what is not built) as
`## Category` headings with `- Item` bullets, most to least important, `Defects` first, and
categories no listener is waiting on headed `Later:` below the rest. An item waiting on something
outside this tree is marked **Blocked on …** with the blocker. Drop an item as it lands and add what
the work uncovers; what a landed item became belongs in `.claude/rules/`.

`AGENTS.md` is this guidance for agents that do not load `.claude/rules/` themselves. It points here
rather than restating the design, so it changes only when a rules file or a check is added or a
standing rule of how work is done moves, in the same commit.

## Architecture

Twenty-two crates. `resonate-core` is the only universal dependency; `resonate-codec`, `resonate-dsp`
and `resonate-pipewire` never depend on each other, and `resonate-engine` joins them.

```
resonate            bin: CLI, tracing, wiring
  ├── resonate-ui         GPUI views, Wayland          [gated behind the `ui` feature]
  ├── resonate-mcp        catalog and running player over the Model Context Protocol  [`mcp`]
  ├── resonate-mpris      session bus: MPRIS, notifications, idle inhibit, playlists
  ├── resonate-discord    what is playing, told to a running Discord  [`discord`]
  ├── resonate-engine     transport, ring, pipeline policy
  │     ├── resonate-codec    decode to PCM  (symphonia)
  │     ├── resonate-dsp      resample, gain, dither
  │     └── resonate-pipewire sink enumeration, negotiation, stream
  ├── resonate-online     MusicBrainz, covers, portraits, lyrics, AutoEq  [`online`]
  ├── resonate-library    scan, tags, persistence  (also on codec, vault)
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

The tree says what each crate is *for*, not every edge. `cargo tree` is the authority, and the
`layering` job in `.github/workflows/ci.yml` holds the refusals (what each crate must stay free of).
Edges worth knowing: only the binary reaches `resonate-online`, `resonate-mcp` and
`resonate-discord`; a provider crate reaches the seam and core and nothing else; `resonate-listen`
reaches core and `resonate-pipewire` alone (a PipeWire client of its own per recording, never the
engine's single-stream slot); `resonate-library` reaches the provider seam and `resonate-analysis`;
`resonate-analysis` reaches `resonate-dsp`.

Invariants the layering protects; the rules files have the rest of each:

- **A playable item is named by a `MediaLocation`, never a path.** Core carries a `SourceId` (an
  interned `Arc<str>`) and a `Locator`. `resonate-codec::Sources` turns a location into bytes through
  `MediaProvider::open`; `LocalFiles` is the local provider (`VaultFiles` replacing it where a vault
  is open). `probe`, `probe_stream`, `probe_boxes` and `Decoder::open` take the registry and the
  location, and every `codec::Error` names the location. `Player::with_sources` puts one registry
  behind engine and tag catalog. `MediaLocation::{from_uri, to_uri}` is in core because the bus and
  the command line both read URIs.
- **`resonate-library` is the local source's catalog, not every source's.** Nothing is keyed by
  source; another source brings its own catalog and its queue rows are read through `Player::media`.
- **An artist is one artist however its name is spelled.** `folded_letters` (core) is what
  `artists.key`, `tracks_fts` and a typed search word are folded through (`library.md`).
- **The engine reaches the graph through `Backend`, never `PipeWire` directly.** `Player::new`
  starts the real client; `Player::with_backend` takes any other, which lets
  `crates/resonate-engine/tests/transport.rs` drive a whole transport with no daemon.
- **`resonate-mpris` sees the engine only through `Player` and the front end only through `Host`**,
  so one service serves `resonate play` and the window, and a second `resonate` reaches a first
  through it (`mpris.md`).
- **The CLI grammar is written once and read three times.** `cli.rs` is the clap derive alone and
  `build.rs` writes the man pages and completions from it, so `cli.rs` names no workspace crate.
  **What is offered is what decodes**: `MIME_TYPES`, the desktop entry, the metainfo and
  `AUDIO_EXTENSIONS` agree, held by tests (`binary.md`).
- **The network is behind `Reference`, and the library owns the seam.** `Library::enrich` walks the
  trait and `resonate-online`'s `Online` is the one implementation, so `--exclude resonate-ui
  --no-default-features` builds with no HTTP client. The same shape holds for `Scrobbler`,
  `Fingerprints`, `Corrections` and `LyricProvider`.
- **A request says what this build is and nothing else, unless the listener says otherwise.** The
  User-Agent is `resonate/<version>`; a contact, AcoustID key, AudD token or ListenBrainz token is
  sent only where typed into a setting, each empty by default, and `online = false` stops every
  request (`online.md`).
- **A guess is never written.** Enrichment takes only strict matches, and the grouping key never
  follows one (`library.md`).
- **Stored formats are migrated, not broken.** A schema change is a step appended to `MIGRATIONS`;
  `V1` and existing steps are never edited (`library.md`).
- **What identifies a recording is core vocabulary**, because a provider crate may not see the
  library: `Mbid`, `Isrc`, `Link`, `Relation`, `Service` (the host a link points at; *provider* means
  only a plugin that obtains media).
- **A run of rows is one `resonate-core::Span`**, never empty, whose `Span::landing` is the one
  arithmetic of where a dropped span ends up. `Library::remove_from_playlist`,
  `move_in_playlist`, `Command::Remove` and `Command::Move` take one. `resonate-ui`'s `edit::Span`
  is a different type naming a run of text edits.
- **Core also carries what two crates that may not see each other share**: `Resumption`,
  `Reordered`, `Appearance`, `CivilDate`, `Calendar`, `eq`'s arithmetic and `presence`.
- **Everything else is a crate behind a seam of its own**: tag writing through `TagSink`, the vault,
  lyrics, providers, studies, the equaliser's I/O, Discord.

## Commands

- `cargo build|test --workspace --exclude resonate-ui --no-default-features`: audio stack only, the
  usual inner loop. `--exclude resonate-ui` with `--no-default-features` is the only way to keep gpui
  out; `resonate-ui` stays a default member so a bare `cargo run` opens the window.
- `cargo build|test --workspace`: everything, gpui included.
- `cargo test -p <crate> <test_name> -- --exact --nocapture`: one test.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `rust-formatter` formats and `rust-formatter --check` checks; never `cargo fmt`. CI cannot run it.
- `RESONATE_BENCH_CEILINGS=1 cargo bench -p resonate-dsp --bench stages [<words>]`: every DSP stage
  weighed against `benches/ceilings.tsv`, four times what the baseline CPU costs, never under 0.05 %
  of a core. A stage added to the bench is added to the file.
- `cargo bench -p resonate-library --bench spelling`: times the search vocabulary of a synthetic
  500 000-track catalog; asserts nothing.
- `cargo build --profile profiling`: release with the symbols `perf` and `cargo flamegraph` need.
- Tests needing the outside world skip without it: `resonate-pipewire` (`stream`, `reconnect`),
  `resonate-listen` (`capture`, `reconnect`; the latter needs `pw-cli`), `resonate-mpris --test bus`
  (session bus), `resonate-codec --test encoded` and `resonate-library --test library` (ffmpeg, `metaflac`,
  `wavpack`, `mac`), `resonate-online --test live` (`RESONATE_ONLINE_TESTS`). The two reconnect tests
  host a daemon of their own and the mpris notification press a bus under `dbus-run-session`.
  `resonate-subsonic`, `resonate-tidal` and `resonate-monochrome` serve fake servers on loopback.
- `cd fuzz && cargo +nightly fuzz build`, and `cargo +nightly fuzz run <target> corpus/<target>
  seeds/<target> -- -max_total_time=180 -timeout=15`.

**CI runs what the list runs, and nothing it does not**: `.github/workflows/ci.yml` takes every push
to `master` and every pull request through clippy, the headless and whole-workspace builds and
tests, the DSP bench, the layering refusals and the fuzz build, in `archlinux` containers.
`RUSTFLAGS` is emptied over `target-cpu=native` there. `rpm-release.yml` builds the Fedora source
RPM when a release is published or the workflow is run manually against a selected branch; both
RPMs are kept as a downloadable workflow artifact. A runnable command added above goes into the
workflow too, and a crate added to the layering refusals into its `refuse` lines.

**The hand-rolled parsers are fuzzed from outside the workspace.** `fuzz/`'s `[workspace]` table
detaches it, so the workspace lints do not reach libfuzzer's macros. Eleven targets (`probe`,
`boxes`, `cue`, `lrc`, `lyricsfile`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`);
`probe` reaches the container readers through the public API over bytes a `MediaProvider` of its own
serves. `equaliser` reads an EqualizerAPO profile, its `GraphicEQ` line included, and holds what it
writes back to reading as the same profile. `lrc`, `lyricsfile`, `playlist` and `mcp` use seams
compiled only under `#[cfg(fuzzing)]`, `mcp` reading lines and envelopes and never dispatching, a
tool being free to walk or write the filesystem. `fuzz/seeds/<target>` holds the smallest file of
each thing a target reads and is handed to a run as a second corpus folder; a grown `corpus/` is
gitignored. A seed is added by hand when a run finds something worth starting from; nothing runs the
targets but a person. `ape-decoder` and `symphonia-codec-wavpack` wrap on malformed input by design,
so their overflow checks are off in `[profile.dev]`; an overflow panic inside either is their
wrapping, not a finding, and `fuzz run -O` says what a release build would do.

**A debug build is optimised**, because an unoptimised resampler cannot keep up with the music:
`[profile.dev]` is `opt-level = 1`, with the DSP, codec, analysis, vault crates and their heavy
dependencies at 3. Debug assertions and debuginfo stay on.

`.cargo/config.toml` builds for `target-cpu=native`, so the resampler vectorises to what the machine
has. The Arch package keeps it; the Fedora RPM and the Flatpak build for the architecture's baseline,
and anything producing a binary for another machine sets `RUSTFLAGS` back over it (`packaging.md`).

The binary doubles as the test harness for the audio stack. `cargo run -- <subcommand> --help` is the
grammar (`binary.md`); by area:

- **Output:** `sinks`, `explain <file>` (the `OutputPlan`), `info <file>` (tags, ReplayGain, box
  order, bitrate), `analyse <file>` (the whole-decode study; `--recognise` asks AcoustID), `listen`
  (records the desktop or a microphone and names the song), `eq`.
- **Playing:** `play <files>` (transport keys on stdin), `queue`, `players`, `sleep`, `share`, `mcp`.
- **Catalog:** `scan`, `roots`, `forget`, `enrich`, `tag`, `organise`, `vault`, `studies`, `stats`,
  `favourites`, `suggest`, `playlists`, `playlist`, `import`.
- **Wants:** `wants`, `missing`, `poll`, `tidal` (the device sign-in).
- `cargo run -- <files>` opens the window with those queued (`Exec=resonate %U`); a bare
  `cargo run` opens it on the queue the last run left, unless `resume` is off.

Settings load from `config.toml` or `--config`; a flag outranks the file, which outranks the
defaults, and the settings pane writes back through the same file. The agent shell does not inherit
the desktop session, so a bare `cargo run` cannot open a window; the `run-ui` skill in
`.claude/skills` has the recipe.
