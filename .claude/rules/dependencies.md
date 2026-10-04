---
paths:
  - "**/Cargo.toml"
---

# Dependencies and manifests

- **Pin full versions and carry only the features used**:
  `foo = { version = "1.2.3", default-features = false, features = ["bar"] }`, never `"1"` or
  `"1.2"`. A declared dependency nothing imports is a defect, with the `libc` pin below as the one
  exception.
- **Shared dependencies live in `[workspace.dependencies]`**; members write `foo.workspace = true`.
  New code goes into `crates/<name>/`, added to `workspace.members` and, if anything depends on it,
  to `[workspace.dependencies]`.
- **Edition 2024, resolver 3**, set in `[workspace.package]` / `[workspace]`; members inherit with
  `edition.workspace = true` and `version.workspace = true`.
- **Every member opts into the workspace lints** with `[lints] workspace = true`; without it
  `unsafe_code = "forbid"` and `result_large_err = "deny"` do not apply.
- **`unsafe_code = "forbid"` cannot be downgraded locally.** A binding that seems to need `unsafe`
  is raised, not a weakened lint; it ruled out `pipewire::thread_loop::ThreadLoop`, whose
  constructor is an `unsafe fn`.

## The layering

A boundary that `cargo tree` can show is a `refuse` line in the CI's layering job and a `cargo tree`
line in `CLAUDE.md`'s commands, so a crossed layer fails a pull request; a crate or boundary added
here is added to both.

- `resonate-core` takes no symphonia, pipewire, gpui or serde: one would stop `resonate-dsp` being a
  leaf and lengthen the resampler's test cycle. It does take `encoding_rs` and `chardetng` (pure
  Rust, no default features) for the one text reader every crate reading a listener's files shares
  (`rust-style.md`), and `tz-rs` for `Calendar`, the listener's zone, which the statistics and the
  chart both bucket days by.
- `resonate-codec`, `resonate-dsp` and `resonate-pipewire` never depend on each other.
  `resonate-dsp` also stays off `resonate-eq` (the equaliser's arithmetic is in `resonate-core::eq`)
  and `resonate-analysis` reaches it only for `TruePeakMeter`, so a study's peak is read by the
  interpolator the playback guard uses.
- `resonate-lyrics`, `resonate-eq`, `resonate-listen` and `resonate-analysis` depend on none of
  gpui, the engine, the library or `ureq`; `resonate-listen` is on core and `resonate-pipewire`
  alone, so the window holds a `Listener` without an HTTP client and the recognisers that reach one
  are `resonate-online`'s, handed in by the binary. `resonate-mpris` depends on neither gpui nor the
  library.
- `resonate-ui` depends on neither `resonate-codec` nor any image decoding: `resonate-codec::Drawing`
  decodes and scales cover art into a `Raster`, reached through the engine's and library's
  re-exports, and gpui draws it.
- `resonate-online`, `resonate-mcp` and `resonate-discord` are reached by the binary alone, each
  optional behind its feature (`online`, `mcp`, `discord`), so `cargo tree -p resonate-library`
  stays free of `ureq`, `serde` and `serde_json` and the `--exclude resonate-ui
  --no-default-features` build links no HTTP client. `resonate-mcp` stays off `resonate-online`,
  gpui and the UI; `resonate-discord` reaches core, the engine's vocabulary and serde, no gpui,
  library or HTTP client.
- `resonate-codec` is a *dev*-dependency of `resonate-mpris` alone: `tests/bus.rs` implements a
  `MediaProvider`, which needs the codec's `Error`. The library half still sees the engine through
  `Player` alone.

## Feature flags that are not optional

`default-features = false` silently drops things that matter. Check before trimming:

- **`symphonia`** needs `opt-simd` (a default) for the SIMD FFT paths, `pcm` for `wav` to decode
  rather than demux, and `id3v1` / `id3v2` / `ape`, without which ordinary MP3s fail to probe (the
  probe uses the metadata readers to skip leading tags). `aiff` and `caf` are on because
  `Container::Aiff` and `Container::Caf` are vocabulary and a variant nothing produces is a defect.
  No flag gives WavPack, Monkey's Audio or DSD; hence `dsd/` is hand-rolled and the next two exist.
- **`symphonia-codec-wavpack`** is the WavPack reader and decoder, registered beside symphonia's
  and GPL-3.0-or-later (which the AGPL build takes). Its decoder misreads the extended bits a 32-bit
  or float stream keeps its low bits in, so it is never registered alone: `wavpack.rs` wraps it,
  and a version bump is weighed against the WavPack tests in `tests/encoded.rs`.
- **`ape-decoder`** is the Monkey's Audio decoder; only `format::parse` and `FrameDecoder` are
  used, `ape.rs` being the symphonia reader and decoder around them. Its arithmetic wraps by design,
  so its dev profile turns overflow checks off, and it narrows a 32-bit stereo stream's side channel
  before undoing it, so `Ape` refuses that one shape rather than decode it wrong.
- **`pipewire`** needs `v0_3_50`: `PW_KEY_NODE_RATE` (`v0_3_33`) and `PW_KEY_TARGET_OBJECT`
  (`v0_3_44`) serve the bit-perfect path, `pw_buffer.requested` (`v0_3_49`) lets the callback fill
  the quantum asked for, and `pw_time.buffered` (`v0_3_50`) reports the frames the stream still
  holds. The feature is a *minimum daemon* version, and `packaging/PKGBUILD` and the spec say so.
- **`ahash`** needs `std` for the `AHashMap` / `AHashSet` aliases and `runtime-rng` for the
  `Default` impl `AHashMap::new()` requires.
- **`tracing-subscriber`** needs `env-filter` (not default) and `tracing-log` (default, keep it):
  gpui logs through `log` and its diagnostics vanish without the bridge.
- **`unicode-normalization`** is core's (`folded_letters`, the one fold of a name into bare
  letters) and the library's (`playlist::folded`). Its `std` feature is not default.
- **`rustix`** is how `unsafe` stays out of OS calls: `termios` and `getpgrp` in the binary
  (`resonate play`'s terminal), `fs` extended attributes in `resonate-codec` (carried onto a staged
  tag write) and `getuid` in `resonate-discord` (the socket-owner check).
- **`unicode-width`** is what `Table` measures a column with: a CJK character takes two terminal
  columns. Not `unicode-segmentation` (graphemes), which is `resonate-ui`'s for text editing.
- **`image`** carries exactly the five formats `CoverArt` can name: `bmp`, `gif`, `jpeg`, `png`,
  `webp`. It is `resonate-codec`'s and `resonate-vault`'s; `resonate-ui` names it for the two
  container types above, `resonate-library` only as a dev-dependency, `resonate-analysis` not at all.
- **`proptest`** is a dev-dependency of `resonate-core`, `resonate-library` and `resonate-ui` alone,
  with only `std`: the defaults run each case in a child process, which no property here needs.
- **`notify`** is the library's with its default set dropped (macOS's FSEvents alone); on Linux the
  inotify backend needs no feature.
- **`wayland-protocols`** needs `client` and `staging`: `ext-data-control-v1` is a staging protocol
  and is how the clipboard sets the selection. It and `wayland-client` are `resonate-ui`'s alone, at
  the versions gpui links.
- **`ureq`** carries `rustls` and `gzip`, not `json`. It is the one HTTP client and blocking, since
  the enrich pass is a thread and the lyric look runs on a background executor. `json` is off on
  purpose: `Client::json` reads the body through a bounded reader first, answering `TooLarge` past
  the limit, and hands the bounded bytes to `serde_json::from_slice`, so nothing uncapped is
  parsed. `gzip` only *reads*, so `flate2` (`rust_backend` alone, the version ureq links) is
  `resonate-online`'s for the one packed request body, AcoustID's lookup.
- **`serde`** (`std`, `derive`) and **`serde_json`** (`std`) are `resonate-online`'s, `resonate-mcp`'s,
  `resonate-discord`'s and the Subsonic and TIDAL providers' alone, and `serde` alone is
  `resonate-lyrics`'s. Every `#[derive(Deserialize)]` is private (in online a `...Doc` mapped by
  hand to a library type, in mcp an argument struct turned into a domain value before a tool runs,
  in discord a doc for a frame), so the library and core stay free of both and a renamed field
  breaks one mapping in one crate. The layering job refuses `serde_json` with gpui and `ureq` in
  `cargo tree -p resonate --no-default-features`; `serde` is there already through `zbus`.
- **`serde-saphyr`** is `resonate-lyrics`'s (`deserialize` only), for reading a Lyricsfile; nothing
  writes one. It lives in the lyrics crate so a build without `online` still reads a sidecar
  `.lyricsfile.yaml`. It refuses a duplicated key by default (as the draft asks) and takes a budget
  of nodes, depth and documents, which lets a document off the network be parsed at all.
- **`rusqlite`** needs `bundled`, `cache`, `hooks` and `functions`. `hooks` gives `update_hook`,
  which `db::watch_the_names` uses to know when a name could have moved; deriving that from SQLite
  rather than a list of write paths keeps a later pass from leaving a stale spelling suggestion.
  `functions` lets `schema::configure` register the `words_of` fold SQL cannot spell.
- **`lofty`** is the tag *writer* and `resonate-codec`'s alone. Its `id3v2_compression_support`
  feature is not optional: without it 0.25.4 does not compile. Reading stays symphonia's, since two
  readers disagreeing about a file is worse than one.
- **`opus-rs`** is `resonate-codec`'s with `std` (runtime SIMD detection) and `heap` (boxes a
  decoder's large working state). A pure-Rust decoder rather than a libopus binding, because
  `unsafe_code = "forbid"` rules out writing one and a C dependency is one more thing a package
  must link.
- **`smallvec`** (`union`, `const_generics`) is for collections built over and over that are nearly
  always small; a collection built once, or unbounded in ordinary use (a queue, a playlist, a
  catalog read), stays a `Vec`.
- **`rustls-native-certs`**, **`webpki-root-certs`** and **`rustls`** are the Subsonic and TIDAL
  providers', for a server of the listener's behind a private CA (`providers.md`): the first reads
  the system's store, the second is the Mozilla roots as DER, since ureq's `RootCerts::Specific`
  takes certificates and cannot be handed the built-in set beside the system's any other way.
  `rustls` (`std` alone) is named only to downcast a refused certificate out of the `io::Error`
  ureq hands back, so it must stay the version ureq links. Not ureq's `platform-verifier`, which on
  Linux reads the system's store alone and fails where that is empty.
- **`resonate-subsonic` and `resonate-tidal`** reach `ureq`, `serde` and `serde_json` themselves, as
  a network provider is told to rather than through `resonate-online`; the binary takes them only
  under `online`. Subsonic adds `md-5` for its token; TIDAL adds `base64` (`alloc`) for manifests
  answered base64-encoded and `roxmltree` (`std`, no `positions`) for a DASH MPD. gpui holds
  `roxmltree` 0.20 and this is 0.21, a second copy of a small crate rather than a held-back pin.
- **`futures-channel`** is `resonate-ui`'s (`alloc`), for the oneshot `Drawer::draw` hands a cover
  back through.
- **`rustfft`** is `resonate-dsp`'s and `resonate-analysis`'s, at the version symphonia's
  `opt-simd` locks with the same `avx`, `sse` and `neon` features, and a dev-dependency of the
  library. `rusty-chromaprint` is `resonate-analysis`'s: pure-Rust Chromaprint, since
  `unsafe_code = "forbid"` rules out binding libchromaprint and AcoustID wants that exact print.
- **`toml_edit`** rather than `toml`, with `display` as well as `parse`: the settings pane rewrites
  the user's `config.toml`, and a value model throws away the comments, key order and spacing the
  user wrote.

## Pins held against `cargo update`

- **`libc`** is held at `=0.2.189` on `resonate-ui`, which imports nothing from it: 0.2.190 took out
  `ENOATTR` on Linux, and `xattr` 0.2.3, under gpui, names `libc::ENOATTR` and stops compiling. The
  `=` requirement is what keeps `cargo update` from moving the lockfile past it. It goes when gpui
  no longer reaches `xattr` 0.2.
