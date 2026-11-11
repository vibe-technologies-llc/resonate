---
paths:
  - ".github/**"
  - ".cargo/**"
  - "fuzz/**"
  - "Cargo.toml"
  - "crates/*/benches/**"
  - "soxr-comparison/**"
  - "docs/RESAMPLING.md"
---

# Build, CI and fuzzing

## CI

- **`.github/workflows/ci.yml` runs what `CLAUDE.md`'s command list runs, and nothing it does not**
  (push to `master`, pull requests, `workflow_dispatch`; `archlinux` containers; the token reads
  the repository and nothing more, `permissions: contents: read`): clippy (default
  features, the workspace headless, the binary without `ui` and with each feature alone, so code
  dead without a feature is warned), headless tests of every crate but the window (the job
  reruns the MPRIS bus tests under `dbus-run-session`, which a plain `cargo test` skips) and, in
  the whole-workspace job, only the tests of `resonate` and `resonate-ui`, the two crates whose
  default features the headless job does not build; no job runs `cargo build` before `cargo test`,
  which builds what it needs, and library crates set `doctest = false` since none holds a doctest, the DSP
  bench under its ceilings, the layering refusals, the fuzz build, the sync server's package
  (`server-package`), the `windows` job (`windows-latest`: clippy with `--all-targets` of the workspace but the phones'
  boundary, its binding tool and PipeWire, then the tests of every crate but the window, then the
  binary's `signals` test with `mcp` on, which `--no-default-features` leaves out; the library,
  codec, vault and MCP integration suites run there, gated per test; `windows.md`)
  and `windows-arm64` (`cargo check` of the same set for `aarch64-pc-windows-msvc`, never run), and the
  `macos` job (`macos-latest`, Apple Silicon: the clippy, the headless tests and the `signals` test
  the `windows` job runs, with its excludes, `resonate-engine`'s tests taken apart and run one at a time (`macos.md`), then the tests of `resonate` and `resonate-ui`, since
  the window builds for macOS on a Mac alone; its test steps run `--no-fail-fast` and each whatever
  the one before answered, so one run names every failure of a build no machine here can run;
  `macos.md`). A command added to `CLAUDE.md` goes into the workflow (iOS and the SoXR comparison aside); a crate added to the
  layering refusals gets a line. The layering job has six shapes: `refuse`, `refuse_headless` (no
  default features), `only` (the workspace crates a crate may reach), `only_on` (the same in one
  target's tree), `reached_by_the_binary_alone` (`cargo tree -i`; `online`, `mcp`, `discord`),
  `reached_directly_on` (the workspace crates that may name a crate, in one target's tree), and one `cargo tree --target x86_64-pc-windows-msvc`
  refusal that holds PipeWire, Wayland and `rustix` out of the binary's Windows tree; `resonate-mpris`'s direct normal edges are checked
  for `resonate-codec`, and `resonate-win32`'s Windows tree reaches no workspace crate while only the
  binary, `resonate-wasapi` and `resonate-codec` reach it directly. The macOS lines read
  `cargo tree --target aarch64-apple-darwin`: PipeWire, Wayland, WASAPI, `resonate-win32` and the
  `windows` crate stay out of the binary's tree; `resonate-apple` reaches no workspace crate there
  or on iOS, and is named by `resonate-coreaudio`, `resonate-listen` and `resonate-mpris` (on iOS by
  `resonate-coreaudio` and `resonate-mobile`) alone; `resonate-coreaudio` reaches core, audio, words
  and `resonate-apple` alone; `resonate-listen` reaches those and `resonate-dsp` there, never a
  backend crate. `RUSTFLAGS` is emptied over `target-cpu=native`. The formatter is local
  only: a hosted runner cannot install `rust-formatter`.
- **`release.yml` builds the release assets and runs on a `v*` tag push or a manual run** (input
  `tag`, empty builds artifacts only). The workflow holds `contents: read`; only `release` and `publish` raise it to `contents: write`, and they reach the API through `GH_TOKEN`, so every `actions/checkout` sets `persist-credentials: false` and no build step runs beside a token in `.git/config`. The `release` job holds the tag to
  `v` plus Cargo's version and the recipes' versions, and creates the GitHub release as a draft
  (pre-release when the tag has `-rc`, `-beta` or `-alpha`). The workflow sets `RESONATE_RELEASES` to `github.repository` for every job, so each release asset names the repository it asks about newer releases (`online.md`'s *The latest release*). Parallel jobs follow: `fedora-rpm` (`fedora:44`,
  player and server RPMs and SRPMs), `ubuntu-deb` (`ubuntu:24.04`, rustup stable,
  `packaging/deb/build.sh`), `arch-package` (`archlinux:latest`, `makepkg` as a build user from a
  `git archive` of the tag, the `packaging/release/` recipes, each set to the released version first so
  a recipe left behind still builds the tag), `windows-zip` (`windows-latest`, a matrix of
  `x86_64` and `aarch64`, each `packaging/windows/build.ps1 -Architecture`: the zip and the Inno Setup installer, signed where the certificate secrets exist), `macos-dmg` (`macos-latest`, `packaging/macos/build.sh`: the Apple Silicon application in a disk image, signed ad hoc, or with a Developer ID and notarised where the secrets exist) and `android-apk`; `publish` collects
  every artifact, fills the winget and Scoop manifests from the Windows files (`fill-manifests.sh`), writes `SHA256SUMS`, uploads all to the release and publishes it, since a published
  release is immutable; a job that failed leaves the others' assets, `publish` running whenever `release` succeeded. Everything but the APK, the Windows ARM64 archive and the macOS disk image is x86-64-v3
  (`packaging.md`). Secrets, set once in the repository: `ANDROID_KEYSTORE_BASE64` (the release
  keystore, base64), `RESONATE_SIGNING_STORE_PASSWORD`, `RESONATE_SIGNING_KEY_ALIAS`,
  `RESONATE_SIGNING_KEY_PASSWORD`; the Windows job reads `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD` when they exist and signs nothing when they do not; the macOS job reads `MACOS_CERTIFICATE_BASE64`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_NOTARY_KEY_BASE64`, `MACOS_NOTARY_KEY_ID` and `MACOS_NOTARY_ISSUER` when they exist and signs ad hoc when they do not (`packaging.md`). The APK job decodes the keystore to `$RUNNER_TEMP/release.jks`
  for the one build step, which exports the `RESONATE_SIGNING_*` variables Gradle reads, runs
  `apksigner verify` and `android/check-release.py`, and removes the keystore whatever happens. The three third-party actions beside the keystore (`setup-android`, `rust-cache`, `setup-gradle`) are pinned by full commit SHA of the release tag they were taken from, never a movable tag; a bump resolves the new tag with `gh api repos/<owner>/<repo>/git/ref/tags/<tag>` (dereferencing an annotated tag to its commit).

CI and the release jobs install WebKitGTK 4.1 (`webkit2gtk-4.1`, Fedora's `webkit2gtk4.1-devel`,
Ubuntu's `libwebkit2gtk-4.1-dev`) for `resonate-signin`'s window.

The CI image installs DejaVu fonts so the window's kerning and ellipsis regression can render
through the platform text system; GPUI's fixed-width mock cannot reproduce a name clipped at its
shaped width.

## Android

The Android job builds the Compose APK against compile SDK 37, runs Android lint and the JVM tests,
builds the release variant (R8, ARM64 alone, unsigned there) and holds it to
`android/check-release.py`, which takes the signed `app-release.apk` where it exists and the unsigned
one otherwise (16 KB zip and ELF alignment, no other ABI, the JNA and boundary classes R8 must keep
by name), then instruments x86_64 emulators at API 33, the minimum, and API 36, Compose UI tests
included, each run's report and result XML uploaded as `resonate-android-tests-api-<level>` whether or not it passed, since the log names no failing test. The Gradle task compiles ARM64 and x86_64 Rust libraries with baseline CPU settings, NDK
28.2 and 16 KB ELF alignment, running the host binding tool against Android ELF metadata, so no
Linux sound server or GUI libraries are needed.

## Faster local test runs

`.config/nextest.toml` configures `cargo nextest run --workspace --exclude resonate-ui
--no-default-features`, which runs each test as its own process across every test binary at once
where `cargo test` runs the binaries one after another; `dbus-run-session -- cargo nextest run -p
resonate-mpris --test bus` replaces the bus line. CI keeps `cargo test`, and nextest is installed by
the listener, never by a script.

## Profiles and target

- **A debug build is optimised**, since an unoptimised resampler cannot keep up with the music:
  `[profile.dev]` is `opt-level = 1`, the DSP, codec, analysis and vault crates and their heavy
  dependencies (`rustfft`, `realfft`, `flacenc`, `zstd-sys`...) at 3; debug assertions and debuginfo
  stay on.
- **`--profile profiling`** is release with symbols for `perf` and `cargo flamegraph`.
- **`.cargo/config.toml` sets `target-cpu=native`** so the resampler vectorises. The `-git` Arch packages
  keep it; the release assets build for `x86-64-v3` (the macOS disk image for `aarch64-apple-darwin`'s own baseline, `RUSTFLAGS` emptied), the Flatpak and CI for the baseline, and
  anything producing a binary for another machine sets `RUSTFLAGS` back over it (`packaging.md`).

## Fuzzing

- **The hand-rolled parsers are fuzzed from outside the workspace**: `fuzz/`'s `[workspace]` table
  detaches it, so workspace lints do not reach libfuzzer's macros. Twenty-nine targets: `together` (what a shared list and a party say, their reading back and that ops converge whichever order they cross), `probe`, `boxes`,
  `cue`, `lrc`, `lyricsfile`, `ttml`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`, `sync`,
  `rewriter` (the vault's `bare` over every container and `declared_digest`), `unpacking` (zstd frame
  indexing and the reader), `padding` (the tag writer's padded heads and its Matroska rewrite, kind 5), `journal` (an undo journal,
  sealed by the seam so the checksum is not the fuzzer's to guess), `overlay` (the write overlay
  driven against a byte-vector model, then landed), `cover` (`Drawing::of` and its scalings), `transcode` (the vault's JPEG-to-JXL transcode and its reconstruction),
  `pages` (the shared-page, Spotify, YouTube, TIDAL embed and song.link readers), `manifest` (TIDAL's BTS and
  DASH documents), `kept` (the analysis cache reader), `sofa` (an HRIR set through `hdf5-reader`), `musicbrainz`
  (a lookup and an answer read back as themselves, and a line of the export's COPY text), `upnp` (an SSDP
  answer, a renderer's description, a chunked body, a SOAP answer and an address, holding that no
  control address leaves the host that described the renderer), `cast` (a Cast frame's protobuf and the
  JSON in it, read back as itself) and `server` (the sync server's invitation page request, the
  gateway's request head and its `Range` header over a total, holding that a span lies within it; the
  input's first eight bytes are the total).
- **`probe`'s input is the file and its last three bytes are the run's shape**, so a seed stays a
  plain file a player opens (`fuzz_targets/held.rs`, its `MediaProvider`, is shared with `boxes` and
  `cue`): the last byte's low bit serves it as a pipe (`held::Arrival::Piped`: no seek, no length,
  the spool path), the next asks DSD as samples rather than DoP, the six above hint an extension off
  `AUDIO_EXTENSIONS` (zero hints none); the two before it are seeks, each a 256th of the length, then
  a seek back to the start and `settle_the_spool`. `cue` resolves every `FILE` line against names a
  folder could list through `the_one_a_cue_names`, `the_best_a_cue_names`, `the_folder_a_cue_names`.
- **What a fuzz run found stays found**: a failing input is copied into
  `crates/resonate-codec/tests/found/` and `tests/found.rs` probes and decodes each, holding every
  block to what a decoder makes (a WavPack packet claiming hours of a hole once asked for 16 GiB in
  one block); the same test replays `fuzz/seeds/probe`.
  `equaliser` holds that what it writes of an EqualizerAPO profile reads back the same. `lrc`,
  `lyricsfile`, `ttml`, `playlist`, `mcp` and the targets above that reach a private reader use seams compiled only under `#[cfg(fuzzing)]`; `mcp` reads
  lines and envelopes and never dispatches, a tool being free to touch the filesystem.
- **`fuzz/seeds/<target>`** holds the smallest file of each thing a target reads, a second corpus
  folder; a grown `corpus/` is gitignored. Seeds are added by hand; nothing but a person runs the
  targets.
- **`ape-decoder` and `symphonia-codec-wavpack` wrap on malformed input by design**, so their
  overflow checks are off in `[profile.dev]`: an overflow panic inside either is not a finding;
  `fuzz run -O` says what a release build would do.

## The resampler against SoXR

- **`soxr-comparison/` measures `resonate-dsp`'s resampler against libsoxr from outside the
  workspace**: its own `[workspace]` table, like `fuzz/`, so no workspace build, lint or `cargo
  tree` sees libsoxr. It links the system libsoxr through `pkg-config` (the `soxr` crate), the one
  C library wanted anywhere, since that library is what is measured; nothing in the workspace may
  depend on it.
- **One process, the same buffers**: eighteen settings (Resonate's four levels, High minimum and
  Very high intermediate and minimum; SoXR's eight recipes, VHQ steep, intermediate and minimum) at
  seven rate pairs, f64 interleaved in and out, SoXR on one thread. Tones sit on a 1/64 Hz grid so
  every phase reduces exactly (`signal::phase_at`) and the least-squares fit (`fit::Window`) reads
  levels down to the f64 floor, over the settled middle 0.6 s of 1.2 s.
- **Timing wants an idle machine**: throughput is the best of five runs over 4 s of stereo noise in
  1 024-frame blocks, construction the first build in the process and the best of nine after.
  `--no-timing` measures quality alone (on every core, under any load), `--timing-only` the costs.
  `--candidates FILE` measures and times a line each of kernels and recipes in one table, a rejection for
  each pair (`tuning.rs`, a `Contender::Tuned` through `Resampler::with_params`), which is how every
  preset was chosen. `design_window.py` (numpy alone) prints a level's window as `resample.rs` holds it, to the bit.
- **Not in CI**: a hosted runner has no libsoxr and its timings would mean nothing.
  `docs/RESAMPLING.md` is the report and `docs/resampling/` its plots; the `compare-resamplers`
  skill reruns both.
