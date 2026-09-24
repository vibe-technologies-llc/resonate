---
paths:
  - "**/Cargo.toml"
---

# Dependencies and manifests

- **Pin full versions and carry only the features actually used.** Write
  `foo = { version = "1.2.3", default-features = false, features = ["bar"] }`, never `"1"` or
  `"1.2"`. A declared dependency that nothing imports is a defect — remove it.
- **Shared dependencies live in `[workspace.dependencies]`**; members reference them with
  `foo.workspace = true`.
- **Edition 2024, resolver 3.** Set in `[workspace.package]` / `[workspace]`; members inherit with
  `edition.workspace = true` and `version.workspace = true`.
- **Every member opts into the workspace lints** with `[lints] workspace = true`. Without it,
  `unsafe_code = "forbid"` and `result_large_err = "deny"` do not apply to that crate.
- **`unsafe_code = "forbid"` cannot be downgraded locally.** If a binding appears to require
  `unsafe`, raise it rather than weakening the lint. It has already ruled out
  `pipewire::thread_loop::ThreadLoop`, whose constructor is an `unsafe fn`.
- **New code goes into `crates/<name>/`** and must be added to `workspace.members`, and to
  `[workspace.dependencies]` as well if anything else is to depend on it.
- **`resonate-core` takes no dependency on symphonia, pipewire, gpui or serde.** Guard with
  `cargo tree -p resonate-core`. If it acquires one, `resonate-dsp` stops being a leaf and the
  resampler's test cycle grows a full link. `resonate-codec`, `resonate-dsp` and
  `resonate-pipewire` likewise never depend on each other, `resonate-lyrics` and `resonate-eq` on
  none of gpui, the engine or the library — and `resonate-dsp` on neither of those two, because
  the equaliser's arithmetic is in `resonate-core::eq` precisely so the resampler's test cycle
  stays short — `resonate-mpris` on neither gpui nor the library, and `resonate-ui` on
  neither `resonate-codec` nor any image crate — gpui draws the cover art and
  `resonate-codec::Drawing` scales it, reached through the engine's and the library's re-exports.
  `resonate-online` is reached by the binary alone, as an optional dependency behind its `online`
  feature, so `cargo tree -p resonate-library` stays free of `ureq`, `serde` and `serde_json` and
  the `--exclude resonate-ui --no-default-features` build links no HTTP client. `resonate-mcp`
  is reached the same way, behind `mcp`, and stays off `resonate-online`, gpui and the UI.
  `resonate-discord` is the third, behind `discord`: core, the engine's vocabulary, serde and
  nothing that reaches gpui, the library or an HTTP client.

## Feature flags that are not optional

`default-features = false` silently drops things that matter. Check before trimming:

- `symphonia` needs `opt-simd` put back (it is in the default set) for the SIMD FFT paths, and
  `pcm` for `wav` to decode rather than just demux. `aiff` and `caf` are one flag each and both are
  on, because `Container::Aiff` and `Container::Caf` are vocabulary and a variant nothing can
  produce is a defect — AIFF especially, being a mainstream uncompressed container for this
  audience. There is no flag for WavPack or Monkey's Audio at any price: symphonia has no reader for
  either, which is why neither is in `Codec` any more, and none for DSD, which is why `dsd/` is
  hand-rolled. Its `id3v1` / `id3v2` / `ape` features are
  load-bearing for *demuxing*, not just tags: the probe uses metadata readers to skip leading tags,
  and without them ordinary MP3 files fail to probe.
- `pipewire` needs `v0_3_50`. `PW_KEY_NODE_RATE` is gated behind `v0_3_33` and
  `PW_KEY_TARGET_OBJECT` behind `v0_3_44` — the keys the bit-perfect path depends on —
  `pw_buffer.requested` behind `v0_3_49` and `pw_time.buffered` behind `v0_3_50`, which are what
  let the callback fill the quantum the graph asked for rather than the pool's whole `maxsize` and
  report the frames the stream is still holding. The feature is a *minimum daemon* version, so the
  cost is refusing to run against a PipeWire older than 0.3.50 — a 2022 release, and
  `packaging/PKGBUILD` says so.
- `ahash` needs `std` for the `AHashMap` / `AHashSet` aliases and `runtime-rng` for the
  `Default for RandomState` impl that `AHashMap::new()` requires.
- `tracing-subscriber` needs `env-filter` (not default) and `tracing-log` (default, keep it) —
  gpui logs through the `log` crate and its diagnostics vanish without the bridge.
- `unicode-normalization` is `resonate-library`'s, and only `playlist::folded` reaches it: a
  playlist name is lowercased and then composed, so one letter has one spelling in the `folded`
  column whatever was typed. Its `std` feature is not in the default set.
- `rustix` is the binary's, for `termios` alone — the feature that puts `resonate play`'s terminal
  a key at a time with no `unsafe` — beside `std`. The crate was in the lockfile already, under
  zbus and libspa, so what the dependency adds is the feature and not a crate.
- `unicode-width` is what `Table` measures a column with, rather than `chars().count()`: a CJK name
  takes two terminal columns a character and a combining mark takes none, and neither is one `char`
  worth of room. It is not `unicode-segmentation`, which counts graphemes and would still put a
  CJK name one column short per character.
- `image` carries exactly the five formats `CoverArt` can name — `bmp`, `gif`, `jpeg`, `png`,
  `webp` — and `png` is load-bearing twice over, because a scaled cover is written back as one. It
  is `resonate-codec`'s, so the audio-only build compiles it; `resonate-ui` has none of its own.
- `proptest` is a dev-dependency of `resonate-core`, `resonate-library` and `resonate-ui` and of
  nothing else, and it drops the default set for `std` alone: the defaults bring `fork` and
  `timeout`, which pull `rusty-fork`, `tempfile` and `wait-timeout` in to run each case in a child
  process, and nothing here needs a property that survives its own crash. What it adds to the
  lockfile is three crates.
- `resonate-codec` is a *dev*-dependency of `resonate-mpris` and nothing more: `tests/bus.rs`
  registers a `MediaProvider` that holds a tag read open, and the trait `resonate-engine`
  re-exports cannot be implemented without naming the codec's own `Error`. The library half of the
  crate still sees the engine through `Player` alone, which is what `cargo tree -p resonate-mpris`
  is read for.
- `notify` is `resonate-library`'s and drops its default set, which is macOS's FSEvents alone; on
  Linux the inotify backend needs no feature.
- `wayland-protocols` needs `client` and `staging`: `ext-data-control-v1` is a staging protocol
  and is what `resonate-ui`'s clipboard sets the selection through. It and `wayland-client` are
  `resonate-ui`'s alone and are the versions gpui already links, so the lockfile gains nothing.
- `ureq` carries `rustls` and `gzip` and not `json`. It is the one HTTP client in the tree and it
  is blocking, because the enrich pass is a thread and the lyric look runs on a background
  executor, so nothing here wants a runtime behind it; `rustls` is the TLS it is built with, and
  `gzip` is what lets a service answer compressed. `json` is left off on purpose: `Client::json`
  reads the body through `Body::with_config().limit(…)` first and hands the bounded bytes to
  `serde_json::from_slice`, so the crate parses nothing it has not already capped and needs no
  parser of ureq's. `gzip` only ever *reads*, so `flate2` is `resonate-online`'s own as well —
  the same crate and version ureq already links, with `rust_backend` alone — for the one request
  body that is sent packed, AcoustID's lookup.
- `serde` with `std` and `derive`, and `serde_json` with `std`, are `resonate-online`'s,
  `resonate-mcp`'s and `resonate-discord`'s and nobody else's. Every `#[derive(Deserialize)]` in
  the first is a private `…Doc` mapped by hand to a `resonate-library` type, in the second a
  private argument struct turned into a domain value before a tool runs, and in the third a
  private doc for a frame Discord's IPC speaks, so the library and `resonate-core` stay free of
  both and a renamed field breaks one mapping in one crate rather than a public type.
  `serde` itself is in the `--exclude resonate-ui --no-default-features` build already, through
  `zbus`; `serde_json` is not, and `cargo tree -p resonate --no-default-features -i serde_json`
  finding nothing is the guard.
- `rusqlite` needs `hooks` beside `bundled` and `cache`. `Connection::update_hook` is what
  `db::watch_the_names` registers to know when a name in `tracks`, `albums` or `artists` could
  have moved, which is what the kept spelling vocabulary is stamped against; deriving that from
  SQLite rather than from a list of write paths is what keeps a pass added later from silently
  leaving a stale suggestion behind.
- `lofty` is the tag *writer* and `resonate-codec`'s alone, and its
  `id3v2_compression_support` feature is not optional: with `default-features = false` and nothing
  put back, 0.25.4 does not compile — `handle_compression`'s `#[cfg(not(...))]` twin names
  `Id3v2Error` and `Id3v2ErrorKind` without importing either — so the feature is carried and
  `flate2` with it. Reading stays symphonia's: two readers that disagree about what a file says is
  worse than one, and what a write is weighed against has to be what the rest of the build sees.
- `opus-rs` is `resonate-codec`'s, with `std` and `heap` put back — the default set, named so the
  crate carries nothing it does not use. `std` is its runtime SIMD detection and `heap` boxes a
  decoder's large working state, which otherwise sits inline and makes one several kilobytes of
  stack. It is pure Rust and brings no crate of its own, so the lockfile gains one entry, and it is
  the decoder rather than a binding to libopus because `unsafe_code = "forbid"` would rule out
  writing one here and a C dependency is one more thing a package has to link.
- `smallvec` is shared, with `union` and `const_generics` and nothing of its default set, for a
  collection built over and over that is nearly always small: a cell's search highlight runs and
  the folded words they are matched against, a clause's alternatives and conditions, the peaks one
  frame of a Shazam signature finds, a PipeWire format's offered values, the events one engine pass
  drains, and the visualiser's bars and traces and the window's heading parts. `union` makes the
  inline form one word smaller and `const_generics` lets an inline length be any constant rather
  than one of the sizes the crate lists. symphonia, `parking_lot` and gpui already link it, so the
  lockfile gains edges and no crate. A collection that is built once, or that has no bound a
  listener could not exceed in ordinary use — a queue, a playlist, a catalog read — stays a `Vec`.
- `futures-channel` is `resonate-ui`'s, with `alloc` alone, for the oneshot `Drawer::draw` hands a
  cover back through from a thread gpui does not own. gpui already links it, so the lockfile gains
  an edge and no crate.
- `resonate-listen` is on core, `resonate-pipewire`, `thiserror` and `tracing` alone, so the window
  and the binary can both hold a `Listener` and a `Recognisers` without reaching an HTTP client;
  the recognisers that do reach one are `resonate-online`'s, handed in by the binary the way the
  fingerprinters are. `cargo tree -p resonate-listen` must stay free of gpui, the engine, the
  library and `ureq`.
- `resonate-analysis` reaches `resonate-dsp`, for `TruePeakMeter`, so what a study says a track
  peaks at is read by the same interpolator the playback guard rides the level down with. The
  edge runs one way: `resonate-dsp` still reaches no workspace crate but core.
- `rustfft` is `resonate-dsp`'s as well, for the cepstrum a minimum- or intermediate-phase
  resampler is designed through, and `parking_lot` with it for the cell that keeps each designed
  prototype for the rest of the run. Both were in the lockfile already, so `resonate-dsp` gains
  two edges and the tree no crate, and `resonate-dsp` still reaches no workspace crate but core.
- `rustfft` is `resonate-analysis`'s, at the version symphonia's `opt-simd` already locks and with
  the same `avx`, `sse` and `neon` features, so it costs the lockfile an edge; it is a
  dev-dependency of `resonate-library` for the band-limited noise the studies' tests are made of.
  `rusty-chromaprint` is `resonate-analysis`'s too — Chromaprint in pure Rust, MIT, bringing
  `rubato` with it — because `unsafe_code = "forbid"` rules out binding libchromaprint and an
  AcoustID lookup wants that algorithm's print exactly. `resonate-analysis` also takes `image`
  with the workspace's features, for the PNG a spectrogram is painted as.
- `toml_edit` rather than `toml`, and it needs `display` as well as `parse`. The settings pane
  rewrites the user's `config.toml`; a value model round-trips the *settings* but throws away the
  comments, key order and spacing the user wrote, and `display` is what emits the edited document.
