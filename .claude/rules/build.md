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
  (push to `master`, pull requests, `workflow_dispatch`; `archlinux` containers): clippy (the default
  features, the workspace headless, and the binary without `ui` and with each feature alone, so code
  dead without a feature is warned), headless and whole-workspace builds and tests (the headless job runs the MPRIS bus tests again under
  `dbus-run-session`, which a plain `cargo test` skips for want of a session bus), the DSP bench under its ceilings, the layering refusals, the
  fuzz build, the sync server's package (`server-package`). A command added to `CLAUDE.md` goes into the workflow; a crate added to the layering
  refusals gets a line. The layering job has four shapes: `refuse`, `refuse_headless` (no default
  features), `only` (the workspace crates a crate may reach), `reached_by_the_binary_alone`
  (`cargo tree -i`; `online`, `mcp`, `discord`); `resonate-mpris`'s direct normal edges are checked
  for `resonate-codec`. `RUSTFLAGS` is emptied over `target-cpu=native`. The formatter is local
  only: a hosted runner cannot install `rust-formatter`.
- **`rpm-release.yml`** builds the Fedora RPMs (`fedora:44`) on a published release or manual run,
  kept as a workflow artifact and attached to a release.

## Android

The Android job builds the Compose APK against compile SDK 37, runs Android lint and the JVM tests,
builds the release variant (R8, ARM64 alone) and holds it to `android/check-release.py` (16 KB zip
and ELF alignment, no other ABI, the JNA and boundary classes R8 must keep by name), then instruments
x86_64 emulators at API 33, the minimum, and API 36, Compose UI tests included. The Gradle task compiles ARM64 and x86_64 Rust libraries with baseline CPU
settings, NDK 28.2 and 16 KB ELF alignment. It runs the host binding tool against Android ELF
metadata, so no Linux sound server or GUI libraries are needed for the Android build.

## Profiles and target

- **A debug build is optimised**, since an unoptimised resampler cannot keep up with the music:
  `[profile.dev]` is `opt-level = 1`, the DSP, codec, analysis and vault crates and their heavy
  dependencies at 3; debug assertions and debuginfo stay on.
- **`--profile profiling`** is release with symbols for `perf` and `cargo flamegraph`.
- **`.cargo/config.toml` sets `target-cpu=native`** so the resampler vectorises. The Arch package
  keeps it; the Fedora RPM, Flatpak and CI build for the baseline, and anything producing a binary
  for another machine sets `RUSTFLAGS` back over it (`packaging.md`).

## Fuzzing

- **The hand-rolled parsers are fuzzed from outside the workspace**: `fuzz/`'s `[workspace]` table
  detaches it, so workspace lints do not reach libfuzzer's macros. Twenty-one targets: `probe`, `boxes`,
  `cue`, `lrc`, `lyricsfile`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`, `sync`,
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
  `lyricsfile`, `playlist`, `mcp` and the targets above that reach a private reader use seams compiled only under `#[cfg(fuzzing)]`; `mcp` reads
  lines and envelopes and never dispatches, a tool being free to touch the filesystem.
- **`fuzz/seeds/<target>`** holds the smallest file of each thing a target reads, a second corpus
  folder; a grown `corpus/` is gitignored. Seeds are added by hand; nothing but a person runs the
  targets.
- **`ape-decoder` and `symphonia-codec-wavpack` wrap on malformed input by design**, so their
  overflow checks are off in `[profile.dev]`: an overflow panic inside either is not a finding;
  `fuzz run -O` says what a release build would do.
