---
paths:
  - "**/Cargo.toml"
---

# Dependencies and manifests

- **Full versions, only used features**: `foo = { version = "1.2.3", default-features = false,
  features = ["bar"] }`, never `"1"`/`"1.2"`. A declared dependency nothing imports is a defect.
- **Shared deps in `[workspace.dependencies]`**; members write `foo.workspace = true`. New code goes
  in `crates/<name>/`, listed in `workspace.members`.
- **Edition 2024, resolver 3** in the workspace; members inherit `edition.workspace`,
  `version.workspace`.
- **Every member: `[lints] workspace = true`**, else `unsafe_code = "forbid"` and
  `result_large_err = "deny"` do not apply. `forbid` is never downgraded locally; a binding needing
  `unsafe` is raised (it ruled out `pipewire::thread_loop`, whose constructors are `unsafe fn`).

The listener approved one exception: `resonate-mobile` admits UniFFI scaffolding, the
owned-descriptor bridge and the iOS RemoteIO unit at the native boundary (`mobile.md`). Its explicit crate lints keep
`result_large_err` denied. No other crate weakens `unsafe_code = "forbid"`.

## gpui

- **gpui and `gpui_platform` are git dependencies of Zed's `main`, pinned by `rev`**, since the
  published 0.2.2 is long stale. `gpui_platform` carries the platform backends (`wayland` only);
  `resonate-ui` builds its `Application` through `gpui_platform::application()`. A bump moves both
  revs together, and `cargo test -p resonate-ui` is the check: the test scheduler refuses a wake
  from another thread, so `Driven::built_over` and the clipboard tests call
  `executor().allow_parking()`.

## The layering

A `cargo tree`-visible boundary is a `refuse` or `only` line in the CI layering job; add a boundary
here, add it there.

- `resonate-audio`: core, crossbeam-channel and thiserror alone; no PipeWire, codec, DSP or UI.
  The engine reexports these contracts; the Linux adapter implements them. `resonate-mobile` (Android and iOS) composes engine, codec, audio, library, analysis, lyrics, online and network providers,
  never PipeWire, GPUI, MPRIS or Discord IPC; PipeWire is a Linux-target dependency of the engine and `resonate-listen`. The listener/recogniser vocabulary builds on Android;
  PipeWire capture is compiled only for Linux.
- `resonate-core`: no symphonia, pipewire, gpui, serde (else `resonate-dsp` stops being a leaf).
  Has `encoding_rs`, `chardetng` (the shared text reader, `rust-style.md`) and `jiff` (`Calendar`).
- `resonate-codec`, `-dsp`, `-pipewire` never depend on each other. `-dsp` stays off `resonate-eq`
  (the arithmetic is `resonate-core::eq`). `-analysis` reaches `-dsp` for `TruePeakMeter` and the
  Shazam signature's `Resampler`.
- `resonate-lyrics`, `-eq`, `-listen`, `-analysis`: none of gpui, engine, library, `ureq`. `-listen`
  is core + `-audio` + `-pipewire` alone (recognisers that reach HTTP are `resonate-online`'s, handed in by the
  binary). `resonate-mpris`: no gpui, no library.
- `resonate-ui`: no `resonate-codec`, no image decoding of its own; `resonate-codec::Drawing`
  decodes and scales cover art to a `Raster`.
- `resonate-online`: composed only by the CLI/desktop binary and the mobile boundary.
  The phones need the paced metadata/lyric/scrobbling services but never a Linux audio dependency.
  `-mcp` and `-discord` remain binary-only. These are optional behind CLI features, so
  `cargo tree -p resonate-library` has no `ureq`/`serde`/`serde_json` and `cargo tree -p resonate --no-default-features` links no HTTP client. `-mcp`: off `-online`, gpui, UI. `-discord`: core,
  engine vocabulary, serde; no gpui, library, HTTP client.
- A provider crate reaches core, the seam and (a network one) `resonate-fetch` alone.
- `resonate-sync`: core alone among workspace crates; no library, engine, gpui, `ureq`, serde.
  Reached by the binary (behind `sync`), `resonate-mobile` and `resonate-server` alone; `resonate-server`
  reaches core and `resonate-sync` alone. `resonate-library` takes neither `quinn` nor `tokio`, and
  `cargo tree -p resonate --no-default-features` links neither.
- `resonate-codec` is a *dev*-dependency of `resonate-mpris` alone (`tests/bus.rs` implements a
  `MediaProvider`).

## Feature flags that are not optional

`default-features = false` silently drops things that matter; check before trimming.

- **`symphonia`**: `opt-simd` (SIMD FFT); `pcm` (`wav` decodes); `id3v1`/`id3v2`/`ape` (else
  ordinary MP3s fail to probe); `aiff`/`caf` (`Container::Aiff`/`Caf` are vocabulary). No flag gives
  WavPack, Monkey's Audio or DSD: `dsd/` is hand-rolled, the next two exist.
- **`symphonia-codec-wavpack`**: GPL-3.0-or-later. Misreads the extended bits of a 32-bit/float
  stream, so never registered alone: `wavpack.rs` wraps it; bumps weighed against the WavPack tests
  in `tests/encoded.rs`.
- **`ape-decoder`**: only `format::parse` and `FrameDecoder`, wrapped by `ape.rs`. Wraps arithmetic
  by design; narrows a 32-bit stereo side channel before undoing it, so `Ape` refuses that shape.
- **`pipewire`** `v0_3_50`: `PW_KEY_NODE_RATE`, `PW_KEY_TARGET_OBJECT` (bit-perfect),
  `pw_buffer.requested` (fill the quantum asked), `pw_time.buffered`/`queued_buffers`. A *minimum
  daemon* version; `packaging/PKGBUILD` says so.
- **`ahash`**: `std`, `runtime-rng` (`Default` for `AHashMap::new()`).
- **`tracing-subscriber`**: `env-filter`, and default `tracing-log` kept: gpui logs via `log`. `resonate-mobile` takes it for the `fmt` writer of its phone log (`mobile.md`'s *Logging*).
- **`unicode-normalization`** (`std`): core's `folded_letters` and library's `playlist::folded`.
- **`rustix`**: no `unsafe` in OS calls: `termios`, `getpgrp` (binary), `fs` xattrs
  (`resonate-codec`), `getuid` (`resonate-discord`).
- **`unicode-width`**: `Table` and the readout (CJK = two columns). `unicode-segmentation` is
  `resonate-ui`'s, text editing.
- **`image`**: exactly the five formats `CoverArt` names (`bmp`, `gif`, `jpeg`, `png`, `webp`).
  `resonate-codec`, `-vault`, `-ui`; `-library` dev-only; `-analysis` never.
- **`proptest`**: dev-dependency of `resonate-core`, `-library`, `-ui` alone; `std` only (defaults
  fork a child process per case).
- **`notify`**: library's; defaults dropped (macOS FSEvents alone), Linux inotify needs no feature.
- **`wayland-protocols`**: `client`, `staging` (`ext-data-control-v1` sets the clipboard). With
  `wayland-client`: `resonate-ui` alone, at gpui's versions.
- **`ureq`**: `rustls`, `gzip`, not `json`; the one HTTP client, blocking. `json` is off because
  `Client::json` reads through a bounded reader (`TooLarge` past the limit) then
  `serde_json::from_slice`, so nothing uncapped is parsed. `gzip` only reads, so `flate2`
  (`rust_backend`) is `resonate-online`'s for AcoustID's packed request body.
- **`serde`** (`std`, `derive`), **`serde_json`** (`std`): `resonate-online`, `-mcp`, `-discord` and
  the Subsonic, TIDAL, Monochrome providers alone; `serde` alone in `resonate-lyrics`. Every struct
  deriving `Deserialize` is private (online: a `...Doc` mapped by hand; mcp: an argument struct made
  a domain value before a tool runs; discord: a frame doc; mcp's unit enums `Action`, `Pass` are the
  only public derives), so a renamed field breaks one mapping in one crate. The layering job refuses
  `serde_json` beside gpui and `ureq` in `cargo tree -p resonate --no-default-features`; `serde` is
  there via `zbus`.
- **`serde-saphyr`**: `resonate-lyrics`' (`deserialize` only), reads a Lyricsfile, so a build
  without `online` still reads a sidecar `.lyricsfile.yaml`. Its node, depth and document budgets
  make a network document parseable at all. **`roxmltree`** (`std`) is `resonate-lyrics`' for TTML
  (`read_ttml`), with no DTD and a node limit, for the same reason.
- **`rusqlite`**: `bundled`, `cache`, `hooks`, `functions`. `hooks` feeds `db::watch_the_names`
  (derived from SQLite's `update_hook`, so no later write path leaves a stale spelling suggestion);
  `functions` lets `schema::configure` register the `words_of` fold.
- **`lofty`**: tag *writer*, `resonate-codec`'s alone; `id3v2_compression_support` required (0.25.4
  does not compile without it). Reading stays symphonia's: two disagreeing readers are worse.
- **`opus-rs`**: `resonate-codec`'s; `std`, `heap`. Pure Rust: `forbid` rules out bindings and a C
  dependency is one more thing a package must link.
- **`imbl`** (no default features): the engine's queue alone. `order`, `next` and
  the published `QueueRows` are its persistent `Vector`s, `unshuffled` its persistent `OrdMap`s, so a publish shares the queue in
  O(log n) and an edit copies only the path it touches (`audio.md`'s *A queue edit costs what it
  moves*).
- **`smallvec`** (`union`, `const_generics`): collections built repeatedly and nearly always small;
  queue, playlist and catalog reads stay `Vec`.
- **`rustls-native-certs`**, **`webpki-root-certs`**, **`rustls`**: `resonate-fetch`'s, for a server
  behind a private CA (`providers.md`). The first reads the system store; the second is the Mozilla
  roots as DER (`RootCerts::Specific` is the only way to add the built-in set beside the system's).
  `rustls` (`std` alone) downcasts a refused certificate out of ureq's `io::Error`, so matches
  ureq's version. Not ureq's `platform-verifier`, which fails where the system store is empty.
- **`resonate-subsonic`, `-tidal`, `-monochrome`** take `ureq`, `serde`, `serde_json` themselves
  and `resonate-fetch` for shared plumbing, not `resonate-online`; the binary takes them only under
  `online`. `resonate-fetch` is core, the seam, `ureq` and the three certificate crates alone;
  `resonate-online` takes it for `retry_after`. `md-5`: Subsonic's token (and Last.fm signing in
  online). TIDAL: `base64` (`alloc`), `roxmltree` (`std`; DASH MPD), at 0.21 against gpui's 0.20:
  a second copy of a small crate, not a held-back pin.
- **`futures-channel`**: `resonate-ui`'s; the oneshot `Drawer::draw` returns a cover by.
- **`rustfft`**: `resonate-dsp`'s, `-analysis`', library dev; at symphonia's `opt-simd` version.
  **`realfft`**: `resonate-dsp`'s convolver, over the same `rustfft`, with the same SIMD features.
  **`rusty-chromaprint`**: `resonate-analysis`'s, pure Rust, since AcoustID wants that exact print.
- **`quinn`** (`runtime-tokio`, `rustls-ring`) and **`tokio`**: the sync stack alone
  (`resonate-sync`, `resonate-server`), not the HTTP client's way: hole punching needs UDP, so the
  server connection and peer transfers are QUIC. The client confines tokio to one current-thread
  runtime on its own thread (`rt`, `net`, `time`, `sync`, `macros`, `fs`, `io-util`); the server
  takes `rt-multi-thread` and `signal`. quinn's `platform-verifier`, `log` and `bloom` stay off.
- **`ring`** (direct): sealing (ChaCha20-Poly1305), HKDF, SHA-256, randomness and the peer
  signature check in `resonate-sync`; the crate rustls already links. **`rcgen`** (`ring`): the
  space's peer certificate, and the server tests' self-signed one. **`rustls-pki-types`** (`std`):
  PEM reading (`sync-server-certificate`, the server's chain and key). `rustls` takes `ring` there
  for the provider QUIC needs. The wire is hand-encoded, so no serde.
- **`jiff`**: `resonate-core`'s, the one zone database (`Calendar`): `std`, `tz-system` (the
  desktop's zone, `Calendar::local`) and `tzdb-zoneinfo` (a zone by name from `/usr/share/zoneinfo`:
  Linux, Flatpak, iOS). The phones add `tzdb-bundle-always`, and Android `tzdb-concatenated`, as
  target dependencies (`library.md`'s *A day is the listener's*); the desktop bundles nothing, since
  its system database is always there and updated with the system.
- **`toml_edit`**, not `toml` (`display`, `parse`): the settings pane rewrites the listener's
  `config.toml`, and a value model loses comments, key order and spacing.

- **`uniffi`** 0.32.2: the mobile boundary's scaffolding (Kotlin and Swift), with `cli` only in
  `resonate-bindgen`.
  **`oboe`** 0.6.1 (`shared-stdcxx`): Android-target-only; NDK libc++ ships in the APK.
  **`objc2-audio-toolbox`** 0.3.2: iOS-target-only, pure-Rust framework bindings for the RemoteIO
  output, only the features it calls. **`objc2-core-audio-types`** 0.3.2 with `CoreAudioBaseTypes`
  alone is plain C structs and builds on every host, so `resonate-mobile` takes it everywhere (the
  buffer list's tests run on the host); `objc2` itself, which refuses a non-Apple target, comes in
  only with the toolbox.

## C-backed crates kept, and duplicates third parties force

- **`zstd`** (via `zstd-sys`): `resonate-vault`'s alone. The WAVE form packs at `ARCHIVED_AT` (19) in
  pledged frames, and `the_encoders_an_object_is_stamped_for_are_the_ones_this_build_runs` pins the
  encoder's output; the pure-Rust `ruzstd` encodes to roughly level 11 at best, so a switch would
  weigh every vaulted WAVE again at a worse ratio. The decoder side (`Unpacking`) is the same crate.
- **`ring`** (C and assembly): the provider rustls runs on for `ureq` (`resonate-fetch`) and `quinn`,
  linked whether or not `resonate-sync` names it; `resonate-sync` uses the same crate directly for
  sealing, HKDF, SHA-256, randomness and the peer signature check rather than link a second provider.
- **`md-5` 0.10** beside the workspace's 0.11: `ape-decoder` and `flacenc` take it; **`miniz_oxide`
  0.8** beside 0.9: `png` and `exr` (under `image`, and `gpui`'s) take it while `flate2` takes 0.9.
  Neither is ours to move; each goes when its dependent moves.
