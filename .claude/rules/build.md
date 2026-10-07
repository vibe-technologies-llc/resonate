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

- **`.github/workflows/ci.yml` runs what `CLAUDE.md`'s command list runs, and nothing it does not.**
  Every push to `master`, every pull request (and `workflow_dispatch`), in `archlinux` containers:
  clippy, the headless and whole-workspace builds and tests, the DSP bench under its ceilings, the
  layering refusals and the fuzz build. A runnable command added to `CLAUDE.md` goes into the
  workflow too; a crate added to the layering refusals gets a `refuse` line. `RUSTFLAGS` is emptied
  over `target-cpu=native`. The formatter is local only: a hosted runner cannot install
  `rust-formatter`.
- **`rpm-release.yml`** builds the Fedora binary and source RPMs (`fedora:44` container) when a
  release is published or the workflow is run manually against a selected branch; both are kept as
  a downloadable workflow artifact and, for a release, attached to it.

## Profiles and target

- **A debug build is optimised**, since an unoptimised resampler cannot keep up with the music:
  `[profile.dev]` is `opt-level = 1`, with the DSP, codec, analysis and vault crates and their heavy
  dependencies at 3. Debug assertions and debuginfo stay on.
- **`--profile profiling`** is release with the symbols `perf` and `cargo flamegraph` need.
- **`.cargo/config.toml` builds for `target-cpu=native`**, so the resampler vectorises to the
  machine. The Arch package keeps it; the Fedora RPM, the Flatpak and CI build for the
  architecture's baseline, and anything producing a binary for another machine sets `RUSTFLAGS` back
  over it (`packaging.md`).

## Fuzzing

- **The hand-rolled parsers are fuzzed from outside the workspace**: `fuzz/`'s `[workspace]` table
  detaches it, so the workspace lints do not reach libfuzzer's macros. Eleven targets: `probe`,
  `boxes`, `cue`, `lrc`, `lyricsfile`, `playlist`, `search`, `uri`, `decoded`, `equaliser`, `mcp`.
- `probe` reaches the container readers through the public API over bytes served by its own
  `MediaProvider` (`fuzz_targets/held.rs`, shared with `boxes` and `cue`). **The input is the file
  and its last three bytes are also the run's shape**, so a seed stays a plain file a player opens:
  the last byte's low bit serves it as a pipe (`held::Arrival::Piped`: no seek, no length, the
  spool path), the next asks DSD as samples rather than DoP, the six above hint an extension off
  `AUDIO_EXTENSIONS` (zero hints none); the two before it are two seeks, each a 256th of the length,
  decoded on after, then a seek back to the start and `settle_the_spool`. `cue` resolves every
  `FILE` line against names a folder could list (the name, its case, its stem with another
  extension) through `the_one_a_cue_names`, `the_best_a_cue_names` and `the_folder_a_cue_names`.
- **What a fuzz run found stays found**: an input that once failed a decode is copied into
  `crates/resonate-codec/tests/found/` and `tests/found.rs` decodes each, holding every block to
  what a decoder makes (a WavPack packet claiming hours of a hole once asked for 16 GiB in one
  block). `equaliser` reads an
  EqualizerAPO profile, its `GraphicEQ` line included, and holds that what it writes reads back as
  the same profile. `lrc`, `lyricsfile`, `playlist` and `mcp` use seams compiled only under
  `#[cfg(fuzzing)]`; `mcp` reads lines and envelopes and never dispatches, a tool being free to walk
  or write the filesystem.
- **`fuzz/seeds/<target>`** holds the smallest file of each thing a target reads, handed to a run as
  a second corpus folder; a grown `corpus/` is gitignored. A seed is added by hand when a run finds
  something worth starting from; nothing but a person runs the targets.
- **`ape-decoder` and `symphonia-codec-wavpack` wrap on malformed input by design**, so their
  overflow checks are off in `[profile.dev]`: an overflow panic inside either is their wrapping, not
  a finding; `fuzz run -O` says what a release build would do.
