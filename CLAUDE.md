# CLAUDE.md

Guidance for Claude Code in this repository. Detailed rules live in `.claude/rules/` and load
automatically where their `paths:` match:

| File | Scope | Covers |
|---|---|---|
| `rust-style.md` | always | formatting, imports, the no-comment rule, collections, sync primitives, text read from files |
| `errors.md` | always | the `thiserror` architecture and its structural enforcement |
| `dependencies.md` | `**/Cargo.toml` | version pinning, crate layering, feature flags that are not optional |
| `binary.md` | the binary, the settings pane | the CLI grammar, file arguments and URIs, the MIME list, signals, the terminal, every config key, the settings file |
| `realtime.md` | engine, pipewire, dsp | the audio-callback contract |
| `audio.md` | engine, codec, dsp, pipewire | decode, the prescan guards, transport, queue, sleep, resumption, what is published, DSP, sink |
| `mpris.md` | resonate-mpris, the binary's `mpris.rs` | the bus, the track list, covers, the name, notifications, the idle inhibit, `Running` |
| `library.md` | library, mpris, the playlist panes | schema, scan, enrichment, tag writing, organise, search, playlists, undo, sheets, statistics, suggestions, share |
| `lyrics.md` | resonate-lyrics, the lyrics model and pane | the vocabulary, the provider seam, the LRC reader |
| `eq.md` | resonate-eq, `core::eq`, the DSP stage, the equaliser pane | the vocabulary, the biquads, the per-sink binding, the formats, AutoEq |
| `ui.md` | resonate-ui | chrome, input, drawing, the palettes, panes |
| `vault.md` | resonate-vault, `Library::import`, the Vault group | the forms, the keys, validation, what the catalog holds |
| `online.md` | resonate-online, the binary's `online.rs` | the paced client, the identity, what each service is asked and how its answer is read |
| `providers.md` | resonate-providers, providers/*, `Library::poll` | the seam, the registry, the inbox, what a delivery is and where it lands |
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

`docs/TODO.md` is the roadmap and holds **open work only** — defects, gaps, what is not built — as
`## Category` headings with `- Item` bullets, most to least important, `Defects` first, and
categories no listener is waiting on headed `Later:` below the rest. Keep it current: drop an item
as it lands, add what the work uncovers. What a landed item becomes is a record of the shipped
design and belongs in `.claude/rules/`, never left in the roadmap.

`AGENTS.md` is this guidance for agents that do not load `.claude/rules/` themselves: which files to
read, what is not negotiable, how the rules are kept true. It points here rather than restating the
design, so it changes only when a rules file or a check is added or a standing rule of how work is
done moves — and then in the same commit.

## Architecture

Twenty crates. `resonate-core` is the only universal dependency; `resonate-codec`, `resonate-dsp`
and `resonate-pipewire` never depend on each other, and `resonate-engine` joins them.

```
resonate            bin — CLI, tracing, wiring
  ├── resonate-ui         GPUI views, Wayland          [gated behind the `ui` feature]
  ├── resonate-mcp        the catalog and the running player, served to a language model
  │                       over the Model Context Protocol  [gated behind `mcp`]
  ├── resonate-mpris      the session bus: MPRIS, notifications, the idle inhibit, playlists,
  │                       the icon loader told the launcher icon moved
  ├── resonate-discord    what is playing, told to a running Discord  [gated behind `discord`]
  ├── resonate-engine     transport, ring, pipeline policy
  │     ├── resonate-codec    decode → PCM  (symphonia)
  │     ├── resonate-dsp      resample, gain, dither
  │     └── resonate-pipewire sink enumeration, negotiation, stream
  ├── resonate-online     MusicBrainz, covers, portraits, lyrics, AutoEq  [gated behind `online`]
  ├── resonate-library    scan, tags, persistence  → also on resonate-codec, resonate-vault
  ├── resonate-lyrics     lyric vocabulary and the provider seam  → drawn by resonate-ui
  ├── resonate-vault      the managed archive: FLAC, WAVE+zstd, JXL covers
  ├── resonate-analysis   one whole decode: waveform, spectrum, verdict, loudness, Chromaprint;
  │                       a clip's Shazam signature
  ├── resonate-listen     capture the desktop or a microphone, and the recogniser seam
  ├── resonate-eq         profile formats, the profile store, the AutoEq catalogue and its seam
  ├── resonate-providers  the provider seam: an identity in, media out  → filled by providers/*
  ├── providers/inbox     resonate-inbox, a folder of the listener's
  ├── providers/subsonic  resonate-subsonic, a Subsonic server of the listener's  [gated behind `online`]
  └── resonate-core       domain vocabulary
```

The tree says what each crate is *for*, not every edge (`cargo tree` is the authority). The binary
also reaches `resonate-codec`, `resonate-pipewire` and `resonate-vault`; `resonate-ui` the engine,
library, lyrics and `resonate-listen`; `resonate-online` the library, lyrics, codec and core, and only
the binary reaches it. `resonate-mcp` reaches the library, `resonate-mpris`, the engine's
vocabulary, the provider seam and core; `resonate-discord` core and the engine's vocabulary — both
the binary's alone. `resonate-library` reaches `resonate-providers` for the seam its poll walks and
`resonate-analysis` for the studies its enrichment takes; the engine reaches `resonate-analysis` to
hand the window `Player::analyse`; a provider crate reaches the seam and core and nothing else.
`resonate-listen` reaches core and `resonate-pipewire` alone — a PipeWire client of its own per
recording, so a capture never shares the engine's single-stream slot — and `resonate-analysis`
reaches `resonate-dsp` for the true-peak meter and the resampler a signature is taken at 16 kHz
through.

Invariants the layering protects; the rules files have the rest of each:

- **A playable item is named by a `MediaLocation`, never a path.** Core carries a `SourceId` and a
  `Locator` — a `PathBuf` for local files, an opaque key otherwise. The id is an `Arc<str>` and the
  local one interned, since a location is cloned per scanned file, queue row and drawn row (a
  `Box<str>` made `MediaLocation::local` a malloc for five bytes and a clone two allocations).
  `resonate-codec::Sources` turns one into bytes: `MediaProvider::open` answers a `Media`, and
  `LocalFiles` is the local source's provider (`VaultFiles` replacing it where a vault is open).
  `probe`, `probe_stream`, `probe_boxes` and `Decoder::open` take the registry and the location,
  and every `codec::Error` names the location, so a source with no filesystem still says what
  failed. `Player::with_sources` puts one registry behind engine and tag catalog, and
  `crates/resonate-engine/tests/transport.rs` serves a track from memory and asserts the graph got
  the bytes a local file gives. `MediaLocation::{from_uri, to_uri}` is in core because the bus and
  the command line both read URIs (`binary.md`).
- **`resonate-library` is the local source's catalog, not every source's.** It walks directories,
  stores paths and hands them back as local locations; nothing is keyed by source. Another source
  brings its own catalog, and its queue rows are read through `Player::media` like any unscanned
  row.
- **An artist is one artist however its name is spelled.** `folded_letters` (core, re-exported by
  the library's `store`) is what `artists.key`, `tracks_fts` and a typed search word are folded
  through; `store::reconcile_artists` folds an old catalog together in `Library::build`.
  `library.md` has it.
- **The engine reaches the graph through `Backend`, never `PipeWire` directly.** `Player::new`
  starts the real client; `Player::with_backend` takes any other, which lets `transport.rs` drive a
  whole transport with no daemon and assert on the bytes the graph got.
- **`resonate-mpris` sees the engine only through `Player` and the front end only through
  `Host`**, so one service serves `resonate play` and the window, and a second `resonate` reaches a
  first through it (`mpris.md`).
- **The CLI grammar is written once and read three times** — `cli.rs` is the clap derive alone,
  and `build.rs` writes the man pages and completions from it, so `cli.rs` names no workspace crate
  (`binary.md`). **What is offered is what decodes**: `MIME_TYPES`, the desktop entry, the
  metainfo and `AUDIO_EXTENSIONS` agree, held by tests.
- **The network is behind `Reference`, and the library owns the seam.** `reference.rs` carries the
  vocabulary and the trait, `Library::enrich` walks it, and `resonate-online`'s `Online` is the one
  implementation, reached only by the binary behind `online` — so `--exclude resonate-ui
  --no-default-features` builds with no HTTP client and `cargo tree -p resonate-library` stays free
  of `ureq` and `serde`. The same shape holds for every other service seam (`Scrobbler`,
  `Fingerprints`, `Corrections`, `LyricProvider`).
- **A request says what this build is and nothing else, unless the listener says otherwise.** The
  User-Agent is `resonate/<version>`; a contact, AcoustID key, AudD token or ListenBrainz token is
  sent only where the listener typed it into a setting, each empty by default, and `online = false`
  stops every request (`online.md`).
- **A guess is never written.** The enrichment takes only strict matches, and the grouping key
  never follows one (`library.md`).
- **Stored formats are migrated, not broken.** A schema change is a step appended to `MIGRATIONS`;
  `V1` and existing steps are never edited (`library.md`).
- **What identifies a recording is core vocabulary**, because a provider crate may not see the
  library: `Mbid`, `Isrc`, `Link`, `Relation` and `Service` (the host a link points at — *provider*
  means only a plugin that obtains media) live in core and the library re-exports them.
- **A run of rows is one `resonate-core::Span`** — a first and a last read either way round, never
  empty — and `Span::landing` is the one arithmetic of where a dropped span ends up: on the row
  going up, ending on it going down. `Library::remove_from_playlist`, `move_in_playlist`,
  `Command::Remove` and `Command::Move` take one, so removing thirty rows is one statement and one
  park-and-unpark pair, and `Queue::remove_rows` rebuilds `items` and `order` in one pass. In core
  because the library and the engine both need it. `resonate-ui`'s `edit::Span` is a different type
  naming a run of text edits.
- **Core also carries what two crates that may not see each other share**: `Resumption` (built by
  the engine, stored by the library — `audio.md`), `Reordered`, `Appearance` (read by the config
  reader, compiled without `ui`, and the window — `ui.md`), `CivilDate`, `Calendar` (the
  listener's zone, read through tz-rs, shared by the library's statistics and the window's chart),
  `eq`'s arithmetic (`eq.md`) and `presence` (`discord.md`).
- **Everything else is a crate behind a seam of its own**: tag writing through `TagSink`
  (`library.md`), the vault (`vault.md`), lyrics (`lyrics.md`), providers (`providers.md`),
  studies (`analysis.md`), the equaliser's I/O (`eq.md`), Discord (`discord.md`).

## Commands

```
cargo build --workspace --exclude resonate-ui --no-default-features   # audio stack only
cargo test  --workspace --exclude resonate-ui --no-default-features   # the usual inner loop
cargo build --workspace                    # everything, including gpui
cargo test  --workspace                    # everything
cargo test -p <crate> <test_name> -- --exact --nocapture
cargo test -p resonate-pipewire --test stream  # needs a live daemon; prints a skip without one
cargo test -p resonate-pipewire --test reconnect  # hosts and restarts a daemon; needs pipewire
cargo test -p resonate-listen --test reconnect    # the same under a recording; needs pipewire, pw-cli
cargo test -p resonate-mpris --test bus        # needs a session bus, else skips — but the
                                               #   notification press hosts its own bus under
                                               #   dbus-run-session
cargo test -p resonate-codec --test encoded    # needs ffmpeg, metaflac (embedded CUESHEET),
                                               #   wavpack and mac; skips each without
cargo test -p resonate-library --test library  # one embedded-sheet test needs ffmpeg and skips
cargo clippy --workspace --all-targets -- -D warnings
RESONATE_BENCH_CEILINGS=1 cargo bench -p resonate-dsp --bench stages   # every DSP stage under
                                                                       #   its ceiling
cargo tree -p resonate-core                # must stay free of symphonia, pipewire, gpui, serde
cargo tree -p resonate-library             # must stay free of ureq, serde, serde_json
cargo tree -p resonate-eq                  # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-vault               # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-codec               # must stay free of resonate-dsp, resonate-pipewire
cargo tree -p resonate-dsp                 # must stay free of resonate-eq, resonate-codec,
                                           #   resonate-pipewire
cargo tree -p resonate-pipewire            # must stay free of resonate-codec, resonate-dsp
cargo tree -p resonate-providers           # must stay free of the library, codec, vault, gpui
cargo tree -p resonate-analysis            # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-discord             # must stay free of gpui, the library, ureq
cargo tree -p resonate-listen              # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-lyrics              # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-mpris               # must stay free of gpui, the library
cargo tree -p resonate --no-default-features   # must stay free of serde_json, gpui, ureq
cargo test -p resonate-online --test live  # reaches the real services; skips unless
                                           #   RESONATE_ONLINE_TESTS is set
cargo test -p resonate-subsonic            # tests/server.rs serves a fake Subsonic server on
                                           #   loopback; no network needed
rust-formatter                             # format; never `cargo fmt`
rust-formatter --check                     # read-only; exits 1 with a diff
cd fuzz && cargo +nightly fuzz build       # the parsers' fuzz targets; needs cargo-fuzz
cd fuzz && cargo +nightly fuzz run probe corpus/probe seeds/probe -- -max_total_time=180 -timeout=15
```

**CI runs what the list runs, and nothing it does not.** `.github/workflows/ci.yml` takes every push
to `master` and every pull request through clippy with `-D warnings`, the headless and whole
workspace builds and tests, the DSP bench held to its ceilings, the `cargo tree` refusals and
`cargo +nightly fuzz build`, each in an `archlinux` container holding the PKGBUILD's dependencies
plus ffmpeg, `metaflac`, `wavpack` and `mac`, so the tests wanting them run. With no daemon or bus
there, the PipeWire and bus tests skip — but the two reconnect tests start a daemon of their own
and the notification press a bus under `dbus-run-session` (the container gets `dbus` for it).
`RUSTFLAGS` is emptied over `target-cpu=native`, because a restored cache may come from a runner
with another CPU and a native build faults on another. `rpm-release.yml` builds the Fedora 44 source
RPM and attaches the binary and source RPMs when a GitHub release is published. Formatting is the
one check CI cannot make — no hosted runner installs `rust-formatter` — so `rust-formatter --check`
stays local. A runnable command added above goes into the workflow too, and a crate added to the
layering refusals into its `refuse` lines.

**The hand-rolled parsers are fuzzed from outside the workspace.** `fuzz/`'s `[workspace]` table
detaches it, so `unsafe_code = "forbid"` and the workspace lints do not reach libfuzzer's macros.
Six targets: `probe` drives `probe`, `probe_stream`, `probe_cover_art`, `probe_span` and a bounded
`Decoder` over bytes a `MediaProvider` of its own serves from memory, reaching `riff.rs`,
`matroska.rs`, `flac.rs`, `text.rs` and `dsd/` through the public API rather than a widened one;
`boxes` and `cue` take the two readers that already answer to bytes; `lrc`, `lyricsfile` and
`playlist` need a seam, `#[cfg(fuzzing)]` rather than public — `resonate_lyrics::read_an_lrc_sheet`,
`resonate_lyrics::read_a_lyricsfile` and `resonate_library::read_a_playlist_sheet` compile only
under the cfg cargo-fuzz sets, so the normal build's surface is unchanged and nothing carries an
entry point with no caller (`sheet::parse` was split out of `sheet::read` for the playlist one, the
better factoring anyway). A run's grown corpus is ignored by `.gitignore`; the seed corpus is kept:
`fuzz/seeds/<target>` holds the smallest file of each thing a target reads, handed to a run as a
second corpus folder so growth lands in `corpus/` and the seeds stay. `probe` has one of every
container the scan takes — a 50 ms 8 kHz ffmpeg tone as WAVE, RF64 and Wave64, a three-channel
24-bit WAVE, FLAC carrying a Vorbis `CUESHEET`, MP3, ADTS, AAC and ALAC in MP4, FLAC and Vorbis in
Matroska, Vorbis, Opus in stereo and 5.1, AIFF, CAF, WavPack in integers and floats and Monkey's
Audio — beside a DSF,
a DSDIFF, a DST DSDIFF and an MP3 whose ID3v2 carries `SYLT`, `USLT` and a MusicBrainz `UFID`,
written by hand; seeded, a run starts at 11 733 edges where an empty one starts at 343. `boxes`
takes the two MP4s, and `cue`, `lrc` and `playlist` a sheet each. A seed is added by hand when a run
finds something worth starting from; nothing runs the targets but a person. What a run finds inside
symphonia is guarded against in the prescan (`audio.md`). `ape-decoder` and
`symphonia-codec-wavpack` wrap on malformed input by design, so their overflow checks are off in
`[profile.dev]` and a debug build behaves as a release one; the fuzz build's `RUSTFLAGS` checks reach
them anyway, so an overflow panic inside either is their wrapping, not a finding, and
`cargo +nightly fuzz run -O probe …` — no debug assertions or overflow checks — says what a release
build, which aborts on a panic, would do.

**A cost is measured, and the DSP's is held to a ceiling.**
`cargo bench -p resonate-dsp --bench stages [<words>]` runs every DSP stage and chain — two whole
chains, and the true-peak guard at half volume as a chain at each rate — over four seconds of audio
and prints each as a share of a core; words narrow it to runs whose names hold them. With
`RESONATE_BENCH_CEILINGS` set it weighs each against `benches/ceilings.tsv` — four times what the
run cost built for the baseline CPU here, never under 0.05 % of a core: room for a slower runner,
none for a stage made several times dearer — and exits 1 naming what went over (the CI's *DSP
costs* job). A stage added to the bench is added to the file; one made deliberately dearer moves its
line. It plays a hot signal through the true-peak guard too, since a guard that never limits never
pays for limiting. `cargo bench -p resonate-library --bench spelling` builds the search vocabulary
of a synthetic 500 000-track catalog and times its corrections and completions; it asserts nothing,
so CI runs it no more than a profile. `cargo build --profile profiling` is the release build keeping
the symbols and line tables `perf record` and `cargo flamegraph` need and `strip = "symbols"` takes.

**A debug build is optimised, because an unoptimised resampler cannot keep up with the music.** At
`opt-level = 0` a 96 kHz 24-bit source pegged a core to reach a 48 kHz sink and the ring starved:
the window wedged, and a `SIGTERM` asking a wedged front end to drain left the audio playing.
`[profile.dev]` is `opt-level = 1` for the workspace and its dependencies, with `resonate-dsp`,
`resonate-codec`, `resonate-analysis`, `resonate-vault` and their heavy dependencies (`rustfft`,
`rusty-chromaprint`, `flacenc`, `zstd-sys`, `zune-jpegxl`, `ape-decoder`) at 3 — the same file then
costs 1.5 % of a core instead of 100 %. Debug assertions and debuginfo stay on: the optimiser was
missing, not the checks.

`.cargo/config.toml` builds for `target-cpu=native`, so the resampler's taper and convolution
vectorise to what the machine has rather than the `x86-64` baseline's SSE2 — 1.4x to 1.7x on High,
more the further apart the rates. The Arch package keeps it; the Fedora RPM and the Flatpak build
for the architecture's baseline, and anything producing a binary for another machine, CI included,
sets `RUSTFLAGS` back over it (`packaging.md`).

`--exclude resonate-ui --no-default-features` is what keeps gpui out of a build: `resonate-ui`
depends on `gpui` unconditionally, so turning the binary's `ui` feature off is not enough.
`resonate-ui` stays in the default member set — decided, not deferred: dropping it would not skip
gpui while `ui` is a default feature, and making `ui` non-default would stop a bare `cargo run`
opening the window. The two flags together are the only lever, and already the documented one.

The binary doubles as the test harness for the audio stack, reaching layers a screenshot cannot:

```
cargo run -- sinks                    # the sinks PipeWire advertises; hits the real daemon
cargo run -- explain <file>           # the OutputPlan for a file; hits the real daemon
cargo run -- info <file> [--graph]    # tags, ReplayGain, box order and bitrate statistics
cargo run -- analyse <file>           # decodes it whole: the lossless verdict and what it rests
                                      #   on, levels, loudness, range and DR, the spectrum, the
                                      #   waveform's peak and RMS as text, the print; a .cue with
                                      #   --track <N> or a URI carrying #frames= analyses that cut;
                                      #   --recognise asks AcoustID what the audio is
cargo run -- listen                   # records twelve seconds of the desktop — or, with
                                      #   --microphone [<name>], a microphone — and names the song
                                      #   through Shazam or AudD; --seconds <n> sets the length,
                                      #   --microphones lists what there is
cargo run -- play <files>             # plays a queue of paths or file:// URIs, reading transport
                                      #   keys on stdin (? for help), a key at a time under a live
                                      #   position line on a terminal; --sleep <spec> sets the
                                      #   timer, --shuffle, --repeat off|track|queue and --volume
                                      #   <percent> the transport it starts with
cargo run -- queue <files>            # adds them to the player on the bus, at the end or, with
                                      #   --next, after the playing row; --play hears the first as
                                      #   it lands, --playlist <name> sends a playlist's rows,
                                      #   --player <name> picks one of several players
cargo run -- players                  # this build's players on the bus, what each plays, its rows
cargo run -- scan <roots>             # scans into the SQLite library, prints the counts and,
                                      #   with a reference, enriches what it holds; --full reads
                                      #   every file again, --follow-links walks into links; a
                                      #   pass cancelled by a signal exits 1
cargo run -- roots                    # the roots a bare `scan` walks
cargo run -- enrich                   # asks the reference about every album and artist not asked
                                      #   lately; --refresh asks the answered again, --albums <N>
                                      #   caps each
cargo run -- wants                    # wanted release tracks and where a delivery landed
cargo run -- missing                  # release tracks with no file and releases of held artists
                                      #   with none held, as a count line and two tables;
                                      #   --artist <NAME> one artist's, --read-the-rest first reads
                                      #   the next thousand releases of a discography cut short,
                                      #   --bring-back lists what the window dismissed again
cargo run -- poll                     # asks every provider for each want not tried lately and
                                      #   lands deliveries in the vault as rows; --again asks all
cargo run -- forget <roots>           # drops roots and their tracks; a path or URI `wants` lists
                                      #   a delivery under forgets that row, its want due again;
                                      #   a folder inside a root forgets the tracks under it whose
                                      #   files are gone, an unmounted drive's included
cargo run -- tag                      # the tags that would be written into every scanned file,
                                      #   one row per field, with the album's cover where the file
                                      #   has none, a favourite as a rating and plays as a count;
                                      #   --apply writes, reads back and has the catalog follow,
                                      #   --root <root> narrows it; --undo puts back the last
                                      #   applied run, printed until --apply
cargo run -- vault                    # what the vault holds by form, sources' weight and saving;
                                      #   --import previews and --apply keeps, --root <root> and
                                      #   --at-most <N> narrow; --verify decodes every object
                                      #   against what went in; --prune removes unnamed objects and
                                      #   covers; --release points vaulted rows whose file is
                                      #   still there back at it
cargo run -- organise                 # the moves filing every track under `organise-as`, each
                                      #   within its root, with refusals and why; --apply moves,
                                      #   rewrites the catalog and prunes emptied folders, --as
                                      #   <layout> for one run, --root <root> narrows; --undo puts
                                      #   back the last applied run, printed until --apply
cargo run -- playlists                # playlists with lengths and play counts; --order, --reverse,
                                      #   --named lists those whose name holds every word
cargo run -- playlist <name>          # plays one, taking play's --shuffle, --repeat and
                                      #   --volume; --add <files> appends, --into <other> copies
                                      #   rows (creating it), --export <file> writes M3U, PLS or
                                      #   XSPF by extension, --order <order> sorts by album,
                                      #   artist, title, length or file and --reverse turns it,
                                      #   --keep holds that order and --by-hand lets it go, --tidy
                                      #   drops rows whose files have gone and --fold rows a file
                                      #   is already named by, --query <text> saves one that fills
                                      #   itself with --sort <order> (relevance, album-then-track,
                                      #   title, artist, date-added, duration, plays, played,
                                      #   plays-this-month) and --limit <rows>, --matching <text>
                                      #   plays, copies or with --drop removes only matching rows,
                                      #   --rename <new>, --discard removes it for good, --pin
                                      #   stands it at the top of the listing and sidebar and
                                      #   --unpin lets it go
cargo run -- eq                       # whether the equaliser is on, each device's binding —
                                      #   a profile or its own curve — and the bands in force;
                                      #   --on/--off, --for <sink> picks a binding (none: the
                                      #   fallback), --profile <name> binds, --unbind unbinds,
                                      #   --own binds the device's own curve (shaped from a file or
                                      #   measurement beside --import or --fetch), --forget-own
                                      #   discards it, --list, --import <file> reads EqualizerAPO
                                      #   or AutoEq GraphicEQ, --export <file>, --forget <name>,
                                      #   --find <text> searches AutoEq, --fetch <device> keeps a
                                      #   measurement, --suggest weighs the sink's description
                                      #   against the catalogue
cargo run -- studies                  # what the studies found, with a count line; --fakes,
                                      #   --suspects, --misnamed narrow; --take <file> names that
                                      #   track by what its audio was heard as
cargo run -- stats                    # totals and the three most-heard tables over --window week,
                                      #   month, year or all, --top <N> rows each
cargo run -- favourites               # what is marked; --tracks, --albums, --artists narrow
cargo run -- suggest                  # the suggested playlists with their searches; --save <name>
cargo run -- share [<file>]           # the link for the file, or what the running player plays
cargo run -- sleep <spec>             # pauses the running player after minutes, or at `track`,
                                      #   `queue` or `off`; --player <name>
cargo run -- mcp                      # serves the catalog and running player over the Model
                                      #   Context Protocol on stdin/stdout; --player <name>
cargo run -- import <files>           # reads M3U, PLS and XSPF sheets in; --as <name> renames
cargo run -- <files>                  # opens the window with those queued (`Exec=resonate %U`)
cargo run                             # opens the window on the queue the last run left, paused
                                      #   where it stopped, unless `resume` is off
```

All twenty-nine subcommands run end to end, and so does the bare `resonate`, which opens the GPUI
window. No crate holds `todo!()`. Settings load from `config.toml` or `--config`; a flag outranks
the file, which outranks the defaults, and the settings pane writes back through the same file —
`binary.md` has every key. The agent shell does not inherit the desktop session, so a bare
`cargo run` cannot open a window; the `run-ui` skill in `.claude/skills` has the recipe.
