---
paths:
  - ".github/**"
  - ".cargo/**"
  - "fuzz/**"
  - "Cargo.toml"
  - "crates/*/benches/**"
---

# Build, CI and fuzzing

## CI

- **`.github/workflows/ci.yml` runs what `CLAUDE.md`'s command list runs, and nothing it does not**
  (push to `master`, pull requests, `workflow_dispatch`; `archlinux` containers): clippy (default
  features, the workspace headless, the binary without `ui` and with each feature alone, so code
  dead without a feature is warned), headless and whole-workspace builds and tests (the headless job
  reruns the MPRIS bus tests under `dbus-run-session`, which a plain `cargo test` skips), the DSP
  bench under its ceilings, the layering refusals, the fuzz build, the sync server's package
  (`server-package`). A command added to `CLAUDE.md` goes into the workflow; a crate added to the
  layering refusals gets a line. The layering job has four shapes: `refuse`, `refuse_headless` (no
  default features), `only` (the workspace crates a crate may reach), `reached_by_the_binary_alone`
  (`cargo tree -i`; `online`, `mcp`, `discord`); `resonate-mpris`'s direct normal edges are checked
  for `resonate-codec`. `RUSTFLAGS` is emptied over `target-cpu=native`. The formatter is local
  only: a hosted runner cannot install `rust-formatter`.
- **`release.yml` builds the release assets and runs on a `v*` tag push or a manual run** (input
  `tag`, empty builds artifacts only; `contents: write`). The `release` job holds the tag to
  `v` plus Cargo's version and the recipes' versions, and creates the GitHub release (pre-release
  when the tag has `-rc`, `-beta` or `-alpha`). Parallel jobs follow: `fedora-rpm` (`fedora:44`,
  player and server RPMs and SRPMs), `ubuntu-deb` (`ubuntu:24.04`, rustup stable,
  `packaging/deb/build.sh`), `arch-package` (`archlinux:latest`, `makepkg` as a build user from a
  `git archive` of the tag, the `packaging/release/` recipes) and `android-apk`; `publish` collects
  every artifact, writes `SHA256SUMS` and uploads all to the release. All of it is x86-64-v3
  (`packaging.md`). Secrets, set once in the repository: `ANDROID_KEYSTORE_BASE64` (the release
  keystore, base64), `RESONATE_SIGNING_STORE_PASSWORD`, `RESONATE_SIGNING_KEY_ALIAS`,
  `RESONATE_SIGNING_KEY_PASSWORD`. The APK job decodes the keystore to `$RUNNER_TEMP/release.jks`
  for the one build step, which exports the `RESONATE_SIGNING_*` variables Gradle reads, runs
  `apksigner verify` and `android/check-release.py`, and removes the keystore whatever happens.

## Android

The Android job builds the Compose APK against compile SDK 37, runs Android lint and the JVM tests,
builds the release variant (R8, ARM64 alone, unsigned there) and holds it to
`android/check-release.py`, which takes the signed `app-release.apk` where it exists and the unsigned
one otherwise (16 KB zip and ELF alignment, no other ABI, the JNA and boundary classes R8 must keep
by name), then instruments x86_64 emulators at API 33, the minimum, and API 36, Compose UI tests
included. The Gradle task compiles ARM64 and x86_64 Rust libraries with baseline CPU settings, NDK
28.2 and 16 KB ELF alignment, running the host binding tool against Android ELF metadata, so no
Linux sound server or GUI libraries are needed.

## Profiles and target

- **A debug build is optimised**, since an unoptimised resampler cannot keep up with the music:
  `[profile.dev]` is `opt-level = 1`, the DSP, codec, analysis and vault crates and their heavy
  dependencies (`rustfft`, `realfft`, `flacenc`, `zstd-sys`...) at 3; debug assertions and debuginfo
  stay on.
- **`--profile profiling`** is release with symbols for `perf` and `cargo flamegraph`.
- **`.cargo/config.toml` sets `target-cpu=native`** so the resampler vectorises. The `-git` Arch packages
  keep it; the release assets build for `x86-64-v3`, the Flatpak and CI for the baseline, and
  anything producing a binary for another machine sets `RUSTFLAGS` back over it (`packaging.md`).

## Fuzzing

- **The hand-rolled parsers are fuzzed from outside the workspace**: `fuzz/`'s `[workspace]` table
  detaches it, so workspace lints do not reach libfuzzer's macros. Twenty-two targets: `probe`, `boxes`,
  `cue`, `lrc`, `lyricsfile`, `ttml`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`, `sync`,
  `rewriter` (the vault's `bare` over every container and `declared_digest`), `unpacking` (zstd frame
  indexing and the reader), `padding` (the tag writer's padded heads), `journal` (an undo journal,
  sealed by the seam so the checksum is not the fuzzer's to guess), `overlay` (the write overlay
  driven against a byte-vector model, then landed), `cover` (`Drawing::of` and its scalings),
  `pages` (the shared-page, Spotify, YouTube and song.link readers), `manifest` (TIDAL's BTS and
  DASH documents), `kept` (the analysis cache reader).
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
