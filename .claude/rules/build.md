---
paths:
  - ".github/**"
  - ".cargo/**"
  - "fuzz/**"
  - "Cargo.toml"
  - "crates/*/benches/**"
  - "dsp-lab/**"
  - "docs/RESAMPLING.md"
  - "docs/DSP.md"
  - "crates/xtask/**"
---

# Build, CI and fuzzing


## The check runner

**`cargo xtask` runs the checks the way CI runs them**, so the lines are written once
(`crates/xtask`, a workspace member reaching no workspace crate and only `thiserror` outside it,
run through the `xtask` alias in `.cargo/config.toml`). `plan.rs` holds every line: `lint` is the
default-feature workspace, the headless workspace without `resonate-ui`, and the binary with
`BINARY_FEATURES_TOGETHER` and each of `BINARY_FEATURES_ALONE`, so a feature added to the binary is
added there; off desktop Linux (a `--target` that is not one, or a Windows or Mac host) `OFF_LINUX`
is left out, as CI's Windows and macOS jobs leave it. `test` is `cargo nextest run --workspace`,
wrapped in `dbus-run-session` on Linux where it is on the path; `check` is `fmt --check`, `lint`
and `test`; `android` and `ios` call the phones' own steps. CI's four clippy jobs run `cargo xtask
lint --locked --verbose` (the arm64 one with `--target aarch64-pc-windows-msvc`).

- **`--changed`** (`changed.rs`) takes the files differing from the merge base with `origin/master`
  (`--since <rev>`), the working tree and untracked files included, gives each to the member whose
  folder holds it, adds every workspace package depending on one (`cargo tree --invert`, dev
  edges included), and lints and tests those; `RULES_AND_COMMENTS` is always tested, since the
  comment and rules-name checks read the whole tree. A root manifest, the lock,
  `rust-toolchain.toml` or `.cargo/` is every package. The binary's feature lines come only with
  the binary.
- **Output** is one line a step, each kept whole in `target/xtask/NN-<step>.log` (cleared at the
  start of a run, under `CARGO_TARGET_DIR` where set); a failure shows its last
  `LINES_SHOWN_OF_A_FAILURE` lines with cargo's and nextest's progress taken out, and a passing
  nextest its summary line. `--verbose` streams instead; `--keep-going` runs every step though one
  fails. It speaks to whoever runs the checks, so its words are English literals, as an error's
  `Display` and a log line are (`language.md`).

## CI

- **`.github/workflows/ci.yml` runs what `CLAUDE.md`'s command list runs, and nothing it does not**
  (push to `master`, pull requests, `workflow_dispatch`; `archlinux` containers; the token reads
  the repository and nothing more, `permissions: contents: read`): clippy through `cargo xtask lint
  --locked --verbose` (default features, the workspace headless, the binary without `ui` and with each feature alone, so code
  dead without a feature is warned), headless tests of every crate but the window (built and run
  by `headless` under `dbus-run-session` so the MPRIS bus tests run) and, in
  the whole-workspace job, only the tests of `resonate` and `resonate-ui`, two of the three crates whose
  default features the headless job does not build (the third, `resonate-signin`, whose `window`
  feature is a default, is linted by the full clippy and tested by no job); no job runs `cargo build` before `cargo nextest run`,
  which builds what it needs, and library crates set `doctest = false` since none holds a doctest, the DSP
  bench under its ceilings, the layering refusals, the fuzz build, the sync server's package
  (`server-package`), on Windows (`windows-latest`) the `windows` job (clippy with `--all-targets` of the workspace but the phones'
  boundary, its binding tool and PipeWire, then the same headless and per-feature clippy lines as
  Linux) beside `windows-tests` (the tests of every crate but the window, then the
  binary's `signals` test with `mcp` on, which `--no-default-features` leaves out, whatever the first
  answered; the library, codec, vault and MCP integration suites run there, gated per test; `windows.md`)
  and `windows-arm64` (the same clippy lines for `aarch64-pc-windows-msvc`, never run), and on macOS
  (`macos-latest`, Apple Silicon) the `macos` job (the same clippy lines) beside `macos-tests` (the headless tests and the `signals` test
  `windows-tests` runs, with its excludes, `resonate-engine`'s tests taken apart and run one at a time, `macos.md`) and `macos-window` (the tests of `resonate` and `resonate-ui`, since
  the window builds for macOS on a Mac alone), each test step run whatever the one before
  answered, so one run names every failure of a build no machine here can run (`macos.md`). A command added to `CLAUDE.md` goes into the workflow (iOS and the DSP lab aside); a crate added to the
  layering refusals gets a line. The layering job has six shapes: `refuse`, `refuse_headless` (no
  default features), `only` (the workspace crates a crate may reach), `only_on` (the same in one
  target's tree), `reached_by_the_binary_alone` (`cargo tree -i`; `online`, `mcp`, `discord`),
  `reached_directly_on` (the workspace crates that may name a crate, in one target's tree), and one `cargo tree --target x86_64-pc-windows-msvc`
  refusal that holds PipeWire, Wayland and `rustix` out of the binary's Windows tree; `resonate-mpris`'s direct normal edges are checked
  for `resonate-codec`, and `resonate-win32`'s Windows tree reaches no workspace crate while only the
  binary, `resonate-wasapi`, `resonate-codec` and `resonate-mpris` reach it directly; `resonate-linux`
  reaches no workspace crate and only `resonate-codec` reaches it directly, and it stays out of the
  binary's Windows and macOS trees. The macOS lines read
  `cargo tree --target aarch64-apple-darwin`: PipeWire, Wayland, WASAPI, `resonate-win32` and the
  `windows` crate stay out of the binary's tree; `resonate-apple` reaches no workspace crate there
  or on iOS, and is named by `resonate-coreaudio`, `resonate-listen`, `resonate-mpris` and `resonate-codec` (on iOS by
  `resonate-coreaudio` and `resonate-mobile`) alone; `resonate-coreaudio` reaches core, audio, words
  and `resonate-apple` alone; `resonate-listen` reaches those and `resonate-dsp` there, never a
  backend crate. `RUSTFLAGS` is emptied over `target-cpu=native` in the Linux and macOS jobs; the
  Windows jobs cannot empty it, since a variable set to the empty string is removed there and the
  checkout's `target-cpu=native` would apply, so each sets the flags its release asset builds with
  (`windows.md`'s *Release*): `-C target-cpu=x86-64-v3` for `windows` and `-C target-cpu=generic` for
  `windows-arm64`, both with `-C target-feature=+crt-static`. Every Windows job links with
  `rust-lld.exe` (`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER`, the host's build scripts included
  in the ARM64 one), since `link.exe` spent most of a test build linking the sixty-odd test
  binaries, and works on a Dev Drive (`samypr100/setup-dev-drive`: ReFS, trusted so Defender does
  not scan each file written, the checkout copied there and every step run in
  `DEV_DRIVE_WORKSPACE`). The `fuzz` job installs `cargo-fuzz` with `cargo install`, and `layering`
  installs `git` and `rustup` alone, the only things `cargo tree` needs. The formatter is local
  only: a hosted runner cannot install `rust-formatter`.
- **A push to `master` never cancels the run before it; a pull request's does**
  (`cancel-in-progress` only for `pull_request`): GitHub keeps one run going and the newest one
  waiting, so a run always finishes and saves its cache: a cancelled job saves none, and the
  Windows, macOS and Android builds, longer than the gap between pushes, would compile cold every
  time.
- **The Rust caches fit the repository's 10 GB, which is all GitHub gives without billing**, so
  only the jobs whose build is the longest keep one, each saved by one job on `master` alone
  (`save-if`) and whatever the tests answered (`cache-on-failure`), and read by the others of its
  platform through `shared-key` (the registry, build scripts and proc-macros, which `cargo check`
  and `cargo build` share): `linux`, saved by `workspace` and read by `clippy`; `headless`'s
  own; `windows`, saved by `windows-tests` and read by `windows`; `macos`, saved by `macos-tests` and
  read by `macos` and `macos-window`; `android`, saved by `android` (both ABIs, debug) and read by
  `android-emulator`. `costs`, `layering`, `fuzz`, `windows-arm64` and `android-release` keep none:
  each builds cold in less time than the slowest job takes warm. Each cache also holds about
  0.7 GB of registry beside `target`, and CI sets `CARGO_PROFILE_DEV_DEBUG=0`, since line tables
  only serve a backtrace and cost every link and cache. A new cache job is weighed against this sum.
- **`release.yml` builds the release assets and runs on a `v*` tag push or a manual run** (input
  `tag`, empty builds artifacts only). The workflow holds `contents: read`; only `release` and `publish` raise it to `contents: write`, and they reach the API through `GH_TOKEN`, so every `actions/checkout` sets `persist-credentials: false` and no build step runs beside a token in `.git/config`. The `release` job holds the tag to
  `v` plus Cargo's version and the recipes' versions, and creates the GitHub release as a draft
  (pre-release when the tag has `-rc`, `-beta` or `-alpha`). The workflow sets `RESONATE_RELEASES` to `github.repository` for every job, so each release asset names the repository it asks about newer releases (`online.md`'s *The latest release*). Parallel jobs follow: `fedora-rpm` (`fedora:44`,
  player and server RPMs and SRPMs), `ubuntu-deb` (`ubuntu:24.04`, rustup stable,
  `packaging/deb/build.sh`), `arch-package` (`archlinux:latest`, `makepkg` as a build user from a
  `git archive` of the tag, the `packaging/release/` recipes, each set to the released version first so
  a recipe left behind still builds the tag), `windows-zip` (`windows-latest`, a matrix of
  `x86_64` and `aarch64`, each `packaging/windows/build.ps1 -Architecture`: the zip and the Inno Setup installer, signed where the certificate secrets exist), `macos-dmg` (`macos-latest`, `packaging/macos/build.sh` once for `aarch64` and once for `x86_64`: the application in a disk image each, signed ad hoc, or with a Developer ID and notarised where the secrets exist) and `android-apk`; `publish` collects
  every artifact, fills the winget and Scoop manifests from the Windows files (`fill-manifests.sh`) and the Homebrew cask from the two disk images (`fill-cask.sh`), writes `SHA256SUMS`, uploads all to the release and publishes it, since a published
  release is immutable; a job that failed leaves the others' assets, `publish` running whenever `release` succeeded. Everything but the APK, the Windows ARM64 archive and the Apple Silicon disk image is x86-64-v3
  (`packaging.md`). Secrets, set once in the repository: `ANDROID_KEYSTORE_BASE64` (the release
  keystore, base64), `RESONATE_SIGNING_STORE_PASSWORD`, `RESONATE_SIGNING_KEY_ALIAS`,
  `RESONATE_SIGNING_KEY_PASSWORD`; the Windows job reads `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD` when they exist and signs nothing when they do not; the macOS job reads `MACOS_CERTIFICATE_BASE64`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_NOTARY_KEY_BASE64`, `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER` when they exist and signs ad hoc when they do not (`packaging.md`). The APK job decodes the keystore to `$RUNNER_TEMP/release.jks`
  for the one build step, which exports the `RESONATE_SIGNING_*` variables Gradle reads, runs
  `apksigner verify` and `android/check-release.py`, and removes the keystore whatever happens. The three third-party actions beside the keystore (`setup-android`, `rust-cache`, `setup-gradle`) are pinned by full commit SHA of the release tag they were taken from, never a movable tag; a bump resolves the new tag with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` (dereferencing an annotated tag to its commit).

- **`macos-dmg.yml` builds the macOS disk images alone, on a manual run** (input `architecture`:
  `both`, `aarch64` or `x86_64`), so a Mac can try the branch before any release: the
  `macos-dmg` job of `release.yml` over the checked-out branch, the version the manifest names,
  each image uploaded as the artifact `resonate-macos-<architecture>` and nothing published.
  `gh workflow run macos-dmg.yml --ref <branch>` starts it.

- **`android-apk.yml` builds the signed Android APK alone, on a manual run**: the `android-apk`
  job of `release.yml` over the checked-out branch (the same keystore secrets, `apksigner verify`
  and `android/check-release.py`, the keystore removed whatever happens), named by the version
  the root manifest names and uploaded as the artifact `resonate-android-apk`, nothing published.
  `gh workflow run android-apk.yml --ref <branch>` starts it.

- **One runner builds and runs the headless suite.** The `headless` job runs
  `dbus-run-session -- cargo nextest run --locked --profile ci --workspace --exclude resonate-ui --no-default-features`
  in the checkout with the pinned toolchain and its own cargo cache.

CI and the release jobs install WebKitGTK 4.1 (`webkit2gtk-4.1`, Fedora's `webkit2gtk4.1-devel`,
Ubuntu's `libwebkit2gtk-4.1-dev`) for `resonate-signin`'s window.

The CI image installs DejaVu fonts so the window's kerning and ellipsis regression can render
through the platform text system; GPUI's fixed-width mock cannot reproduce a name clipped at its
shaped width.

## Android

Three Android jobs run side by side. `android` builds the Compose APK against compile SDK 37.1, runs
Android lint, the JVM tests and `check-native.py`; `android-release` builds the release variant (R8, ARM64 alone, unsigned there) and holds it to
`android/check-release.py`, which takes the signed `app-release.apk` where it exists and the unsigned
one otherwise (16 KB zip and ELF alignment, no other ABI, the JNA and boundary classes R8 must keep
by name); `android-emulator`, a matrix of two, instruments an x86_64 emulator at API 33, the
minimum, or API 36, Compose UI tests included, its debug APK built for x86_64 alone
(`-Presonate.debugAbis=x86_64`, a comma-separated list the debug variant's ABIs default to both
without), each run's report and result XML uploaded as `resonate-android-tests-api-<level>` whether or not it passed, since the log names no failing test. The Gradle task compiles ARM64 and x86_64 Rust libraries with baseline CPU settings, NDK
r30 LTS and 16 KB ELF alignment, running the host binding tool against Android ELF metadata, so no
Linux sound server or GUI libraries are needed.

## Tests run through nextest

- **`cargo nextest run` is the one test runner**, locally, in every CI job and in the PKGBUILDs'
  `check()` (`checkdepends=('cargo-nextest')`); nothing runs `cargo test`. Each test is a process of
  its own, every binary's tests at once, so a test's globals, threads and environment never meet
  another's (a Mac refuses a process past about two thousand threads, `macos.md`). No crate holds a
  doctest (every library sets `doctest = false`), which nextest would not run. The Arch containers
  install it with `pacman` (`cargo-nextest`, in `ARCH_PACKAGES`), the Windows and macOS jobs with
  `taiki-e/install-action`.
- **`.config/nextest.toml`**: `nextest-version` holds the oldest version the config parses under.
  The default profile never fails fast and calls a test slow at 30 s and kills it at five minutes,
  so a hung test fails its job long before the job's timeout. The CPU-heavy packages start first
  (`priority`: `resonate-analysis` at 100, the library, the phones' boundary, UPnP, Cast and the
  engine at 50), so the long tests are not the tail of a run whose sum of test time is about sixteen
  times its length on sixteen threads. The `ci` profile, which every CI line names (`--profile ci`),
  prints each failure again at the end and on macOS puts `resonate-engine`'s tests in the
  `engine-alone` group (one at a time, `macos.md`).
- **A crate's integration tests are one binary, `suite`**: `autotests = false` and one `[[test]]`
  at `tests/suite.rs`, which names each `tests/*.rs` as a module (and holds a module two files share,
  `support`, `shelf`, `blocking`), since every test binary links the whole dependency tree again;
  merging the 66 binaries into 28 roughly halved the time a change in `resonate-core` takes to
  relink every test. A crate with a single file keeps Cargo's inferred binary.
  `every_crate_links_its_integration_tests_into_one_suite_that_names_each_file`
  (`resonate-words-build`'s `suites.rs`) refuses a second binary and a file the suite never
  compiles. A test runs within its file's module, so its name is `bus::…`, `keeps::…`; a test
  that starts its own binary again (a hosted daemon, a bus of its own, a process killed part way)
  names the test through `in_this_suite`, which prefixes `module_path!()`'s module, since a bare
  name under `--exact` runs nothing and the parent would read the child's emptiness as a pass.
  A lib or bin target holding no test sets `test = false` (`resonate-driven`, the server's and the
  sign-in window's binaries, `resonate-bindgen`), so no empty harness is linked.

## Profiles and target

- **A debug build is optimised**, since an unoptimised resampler cannot keep up with the music:
  `[profile.dev]` is `opt-level = 1`, the DSP, codec, analysis and vault crates and their heavy
  dependencies (`rustfft`, `realfft`, `flacenc`, `zstd-sys`...) at 3; debug assertions stay on.
  **Debuginfo is `line-tables-only`**: a panic's backtrace still names file and line, while full
  DWARF made every test binary over half a gigabyte, most of a cold build's 31 GB of `target` and
  most of each link. A debugger session that needs variables builds with
  `CARGO_PROFILE_DEV_DEBUG=full`.
- **The linker stays `rust-lld`, rustc's default on x86-64 Linux.** Measured over the headless test
  build (`cargo test --no-run --workspace --exclude resonate-ui --no-default-features`, 61 links,
  16 threads, every link timed through a wrapper, one target directory each): cold build `rust-lld`
  190.6 s, `wild` 0.11.0 193.0 s, `mold` 3.0.0 182.8 s; after touching `resonate-core`'s `lib.rs`,
  `rust-lld` 33.8 and 33.3 s, `wild` 37.3 and 51.9 s (summed link time 108 s against 191 s), `mold`
  14.6 s (one run). `wild` is slower than the default here and is not pursued. `mold` shortens a
  relink after a change in core by more than half and a cold build by 4 per cent, but it is a
  system package none of the Fedora, DEB, Flatpak and Arch recipes carry and no crate brings, and a
  `.cargo/config.toml` naming it would break every build without it, so a build that wants it
  sets it in a `RUSTFLAGS` of its own (the measurement turned the default off with `-C linker-features=-lld` and put a directory holding `mold` as `ld` first on the linker's search path with `-C link-arg=-B`).
- **`--profile profiling`** is release with symbols for `perf` and `cargo flamegraph`.
- **`.cargo/config.toml` sets `target-cpu=native`** so the resampler vectorises. The `-git` Arch packages
  keep it; the release assets build for `x86-64-v3` (the macOS disk image for `aarch64-apple-darwin`'s own baseline, `RUSTFLAGS` emptied), the Flatpak and CI for the baseline, and
  anything producing a binary for another machine sets `RUSTFLAGS` back over it (`packaging.md`).

## Fuzzing

- **The hand-rolled parsers are fuzzed from outside the workspace**: `fuzz/`'s `[workspace]` table
  detaches it, so workspace lints do not reach libfuzzer's macros. Thirty-eight targets: `listens` (the ListenBrainz JSON lines `history --import-file` reads, holding that each listen kept names a song and an artist and a refusal names a line the input has), `together` (what a shared list and a party say, their reading back and that ops converge whichever order they cross), `probe`, `boxes`,
  `cue` (beside the sheet's reading, `SheetText`'s `own` and `edited` over every field and scope, the edit read back), `rating` (the lofty reads a retag preview makes, `rating_in` and `Counted::read_in`, the input's first byte choosing the extension), `lrc`, `lyricsfile`, `ttml`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`, `sync`,
  `rewriter` (the vault's `bare` over every container and `declared_digest`), `unpacking` (zstd frame
  indexing and the reader, and the vault's `standing::of_a_flac` and `of_a_packed_wave`), `padding` (the tag writer's padded heads and its Matroska rewrite, then a picture attached to what the rewrite laid, held byte for byte to the tags and the picture laid together as a keep writes them, kind 5), `journal` (an undo journal,
  sealed by the seam so the checksum is not the fuzzer's to guess, and a whole copy's trailer and record read off a file), `overlay` (the write overlay
  driven against a byte-vector model, then landed), `cover` (`Drawing::of` and its scalings), `transcode` (the vault's JPEG-to-JXL transcode and its reconstruction),
  `pages` (the shared-page, Spotify, YouTube, TIDAL embed, song.link and Apple Music readers), `manifest` (TIDAL's BTS and
  DASH documents), `kept` (the analysis cache reader), `sofa` (an HRIR set through `hdf5-reader`, whole and streamed from a held source), `musicbrainz`
  (a lookup and an answer read back as themselves, and a line of the export's COPY text), `replication`
  (an input opening as bzip2 read whole as a replication packet through `packet_read`, bzip2 and
  tar, holding that one read is the sequence asked, and every input as `pending_data` rows and their
  JSON alone, seeded with the packets `tests/mirror.rs` replicates; `mibz2` sets up its decoder for
  each packet, so a packet runs at about a dozen inputs a second), `bzipped` (any bytes through
  the crate's two `mibz2` readers, `bzip2_decoded` and `bzip2_decoded_across`, beside
  `libbz2-rs-sys`: the two agree, and agree with the reference wherever it reads, but for a
  randomised block, `dependencies.md`; seeded with small streams, one of blocks of 1, several one
  after another, cut, flipped and trailed ones), `upnp` (an SSDP
  answer, a renderer's description, a chunked body, a SOAP answer, an address and the stream server's request head (`serve::read_a_request_head`), holding that no
  control address leaves the host that described the renderer; and what a control point asks the
  media server: an `M-SEARCH`, a request head and path, a GENA head, a `SearchCriteria`, an object
  id, a `Range` span and a SOAP request answered against an empty catalog, the answer reading as XML),
  `dlna` (a media server's DIDL-Lite listing, bare and inside a SOAP answer to Browse, holding every entry
  to an id and a title and every address to the server's host, `upnp.md`), `cast` (a Cast frame's protobuf and the
  JSON in it, read back as itself), `server` (the sync server's invitation page request, the
  gateway's request head and its `Range` header over a total, holding that a span lies within it; the
  input's first eight bytes are the total), `ipc` (the frames `resonate-mpris`'s `wire.rs` carries
  over a Windows pipe or a Mac's socket, read as requests and as replies and held to re-encode to
  the same bytes), `stun` (an answer on the QUIC socket, `binding_answer`), `measurement`
  (`read_response`: a measurement or calibration file, holding its points ascending and every
  interpolation finite) and `catalogue` (the AutoEq index, holding that no `DeviceId` leaves its
  folder).
- **`probe`'s input is the file and its last three bytes are the run's shape**, so a seed stays a
  plain file a player opens (`fuzz_targets/held.rs`, its `MediaProvider`, is shared with `boxes` and
  `cue`): the last byte's low bit serves it as a pipe (`held::Arrival::Piped`: no seek, no length,
  the spool path), the next asks DSD as samples rather than DoP, the six above hint an extension off
  `AUDIO_EXTENSIONS` (zero hints none); the two before it are seeks, each a 256th of the length, then
  a seek back to the start and `settle_the_spool`. Beside `probe_stream`, `probe_cover_art` and `probe_span` the target runs `probe_scanned` and `probe_pictured` (both pictures). `cue` resolves every `FILE` line against names a
  folder could list through `the_one_a_cue_names`, `the_best_a_cue_names`, `the_folder_a_cue_names`.
- **What a fuzz run found stays found**: a failing input is copied into
  `crates/resonate-codec/tests/found/` and `tests/found.rs` probes and decodes each, holding every
  block to what a decoder makes (a WavPack packet claiming hours of a hole once asked for 16 GiB in
  one block); the same test replays `fuzz/seeds/probe`. **The hand-written SYLT reader has a seed in
  each stamp format**: `tone-id3.mp3` times its words in milliseconds (Latin-1), `tone-sylt-frames.mp3`
  in MPEG frames (UTF-16 with a byte-order mark, no USLT beside it), and
  `the_probe_seeds_reach_the_timed_lyrics_reader_in_either_stamp_format` holds both to reading as
  timed words.
  `equaliser` holds that what it writes of an EqualizerAPO profile reads back the same. `lrc`,
  `lyricsfile`, `ttml`, `playlist`, `mcp`, `ipc`, `stun` and the targets above that reach a private reader use seams compiled only under `#[cfg(fuzzing)]`; `mcp` reads
  lines and envelopes and never dispatches, a tool being free to touch the filesystem.
- **`fuzz/seeds/<target>`** holds the smallest file of each thing a target reads, a second corpus
  folder; a grown `corpus/` is gitignored. Seeds are added by hand; nothing but a person runs the
  targets.
- **`ape-decoder` and `symphonia-codec-wavpack` wrap on malformed input by design**, so their
  overflow checks are off in `[profile.dev]`: an overflow panic inside either is not a finding;
  `fuzz run -O` says what a release build would do.

## The DSP lab

- **`dsp-lab/` measures `resonate-dsp`'s stages and the engine's whole chains for quality and cost,
  from outside the workspace**: its own `[workspace]` table, like `fuzz/`, so no workspace build,
  lint or `cargo tree` sees its references. It reaches `resonate-core`, `resonate-dsp` and
  `resonate-engine` by path, `realfft` for spectra and `clap` for its grammar. Its two references are
  features, both on by default: `soxr` links the system libsoxr through `pkg-config` (the one C
  library wanted anywhere, since that library is what is measured; nothing in the workspace may
  depend on it) and `rubato` is pure Rust. `--no-default-features` builds and tests with neither.
  `cargo xtask fmt` formats it beside `fuzz/` (`DETACHED` in `plan.rs`).
- **The seams.** A `Subject` names a setting and opens a `Running` (f64 interleaved in, pushed then
  finished, with the latency it claims), so every implementation pays the same buffers. `Stage`
  makes one of any `Processor` and its `Driven` drives it block by block. `Resonate` covers the
  resampler's levels, phases and `Tuned` candidates. `Recipe` (SoXR) and `Rubato` (FFT, two sincs,
  cubic polynomial) are the references. `Played` is a whole chain: a source, a sink's
  `SinkFormats` and an `EngineConfig` go through `plan_output` and `build_chain` with the engine's
  block, the input is rounded to the source's words and the output to the sink's (`in_words`), so a
  chain measured is the chain played. A `Running` that can be reset says so (`starts_over`: `Driven`
  resets its `Processor`, `Playing` its `Chain`, a reference cannot), which
  `ran_again_after_a_reset` reads. A `Suite` names its subjects, its rate `Pair`s, its `Metric`s
  and how it measures one subject at one pair. Each `Metric` carries the way that is better and a
  `Tolerance`, and `suites::all` is the one list a suite is added to. `measured_everywhere` shares
  the subject-pair cases among every core; `timed` runs them one at a time.
- **The suites.** `resample` is the resampler against SoXR and rubato (below). `dither-16` and
  `dither-24` cover each kind and shaping: noise to Nyquist and A-weighted, where shaping moves it,
  the harmonics of a one-step tone, noise modulation across levels, the grid and the frames to
  silence. `eq` covers single bands of every kind and whole curves against
  `Profile::magnitude_db_on`, and the impulse against the design. `gain` is the gain heard against
  the gain asked and the overs a hot input makes. `peak` is the true-peak guard against `Overs`
  (an inter-sample over, a +1 dBFS tone, a null under the ceiling), each output's peak read whole,
  never from a cut that would itself ring past the ceiling. `remix` is every matrix heard
  tone by tone against `Remix::gain`. `crossfeed` covers the direct and crossed paths, the crossed
  delay and a centred source's spread. `convolve` is against direct convolution at the response's
  rate and redrawn. `restore` covers repair and extension on noise walled at 16 kHz. `chain` is
  eleven planned chains, from bit-perfect to room correction, with THD, THD+N, A-weighted dynamic
  range, SMPTE IMD, the response's spread, the gain asked and heard, latency and a null where the
  rate and layout do not change. `stress` borrows every subject of ours the other suites hold, at
  each one's own rates among eight (`Borrowed`, named `<suite>: <subject>`), and feeds each the
  same generated inputs: full-scale noise, a 0 dBFS square, Nyquist, a DC step, impulses, a sweep,
  a tone under every word, a NaN and an infinity, silence and 12 s of silence after noise. It
  counts what is not finite and what falls under f32's normal range, reads the loudest sample,
  what silence and a tail leave, what a NaN leaves a second on against a clean run, the null
  across blocks of 1, 37, 1 024 and 4 096 frames and after a flush and a reset, and the frames
  beyond the input times the ratio. Two bugs it found when first run are held by tests in
  `resonate-dsp` as well (`blocks_shorter_than_the_filter_hand_on_the_track_whole_and_alike`,
  `a_convolver_reset_after_its_flush_flushes_the_whole_tail_again`). Its readings are bit for bit
  but a chain's and a dither's reset, which read the dither's own noise.
- **The measures** (`analysis/`): tones sit on whole FFT bins (`on_the_bins`) at 2^16 frames
  widened with the rate (`analysed_frames`), analysed after 0.75 s of settling, so a rectangular
  window leaks nothing. The response is a multitone of 48 log-spaced bins at random phases, THD
  counts harmonics 2 to 10, and THD+N everything from 20 Hz to 20 kHz but the fundamental. A tone
  fed to one channel is read on that channel alone (`heard_where_fed`), since a downmix leaves the
  other near silent.
- **A claimed latency is not where the samples land.** The true-peak guard, the resampler and
  restoration hold their delay back and hand the samples on aligned (measured 0). The convolver
  hands them on a partition late (measured as claimed). So every null is aligned by the delay an
  impulse measures (`held_back`), never by the claim, and both are tabled.
- **Readings become a baseline.** `measure --tsv FILE` writes every reading as `suite`, rates,
  subject, metric key and value apart by tabs, and `--record FILE` writes the same as a baseline.
  `--against FILE` names each reading worse than the baseline by more than its tolerance, and each
  kept reading no longer measured, then fails. `dsp-lab/baseline.tsv` is the committed baseline,
  recorded on this machine with the default features. Everything is seeded but the engine's
  dither, which is seeded from the clock, so a chain's THD and SMPTE IMD (a few bins at the dither
  floor, which moved 6.8 and 4 dB over six runs) are held within `AMONG_DITHER` (10 dB). Re-record
  only where a change is meant to move a figure, and say which in the commit.
- **The resample suite is the old comparison, unchanged in arithmetic**: nineteen settings
  (Resonate's four levels, High and Very high each intermediate and minimum, SoXR's eight recipes,
  VHQ steep, intermediate and minimum) plus rubato's four, at seven rate pairs. SoXR runs on one
  thread. Tones sit on a 1/64 Hz grid so every phase reduces exactly (`signal::phase_at`), and the
  least-squares fit (`fit::Window`) reads levels down to the f64 floor over the settled middle
  0.6 s of 1.2 s. rubato is queued chunk by chunk. Its `output_delay` is trimmed and its tail
  pumped with empty partial chunks until it has given `ceil(ratio × input)` frames, as SoXR does,
  and a chunk may give none, so only `EMPTY_PUMPS_AT_MOST` in a row is a stall. Its report and
  plots are the old tool's to the byte for the rows it had.
- **Timing wants an idle machine**: `time [SUITE..]` times throughput as the best of five runs over
  4 s of noise in 1 024-frame blocks, and construction as the first build in the process and the
  best of nine after, in each subject's own layout (stereo by default). `candidates FILE
  [--no-timing]` measures and times one candidate per line, kernels and recipes, in one table: a
  rejection, a ringing, a pre-echo and a latency for each pair (`tuning.rs`, a `Resonate::Tuned`
  through `Resampler::with_params`). That is how every preset was chosen: a window that wins at
  44.1 to 48 kHz can lose going up. `design_window.py` (numpy alone) prints a level's window as
  `resample.rs` holds it, to the bit.
- **Not in CI**: a hosted runner has no libsoxr and its timings would mean nothing.
  `docs/DSP.md` is the report of every suite, `docs/dsp/` its plots, and `docs/RESAMPLING.md` the
  resampler's own report with `docs/resampling/` its plots. The `measure-dsp` skill runs the lab
  and holds it to the baseline after a DSP change; the `compare-resamplers` skill refreshes the
  resampler's report.
