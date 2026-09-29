---
paths:
  - "**/Cargo.toml"
---

# Dependencies and manifests

- **Pin full versions and carry only the features used**:
  `foo = { version = "1.2.3", default-features = false, features = ["bar"] }`, never `"1"` or
  `"1.2"`. A declared dependency nothing imports is a defect — remove it.
- **Shared dependencies live in `[workspace.dependencies]`**; members write `foo.workspace = true`.
- **Edition 2024, resolver 3**, set in `[workspace.package]` / `[workspace]`; members inherit with
  `edition.workspace = true` and `version.workspace = true`.
- **Every member opts into the workspace lints** with `[lints] workspace = true`; without it
  `unsafe_code = "forbid"` and `result_large_err = "deny"` do not apply.
- **`unsafe_code = "forbid"` cannot be downgraded locally.** A binding that seems to need `unsafe`
  is raised, not a weakened lint; it already ruled out `pipewire::thread_loop::ThreadLoop`, whose
  constructor is an `unsafe fn`.
- **New code goes into `crates/<name>/`**, added to `workspace.members`, and to
  `[workspace.dependencies]` if anything depends on it.
- **The layering:**
  - `resonate-core` takes no symphonia, pipewire, gpui or serde (`cargo tree -p resonate-core`);
    one would stop `resonate-dsp` being a leaf and grow the resampler's test cycle a full link.
  - `resonate-codec`, `resonate-dsp` and `resonate-pipewire` never depend on each other;
    `resonate-lyrics` and `resonate-eq` on none of gpui, the engine or the library; `resonate-dsp`
    on neither of those two, the equaliser's arithmetic being in `resonate-core::eq` so the
    resampler's test cycle stays short; `resonate-mpris` on neither gpui nor the library.
  - `resonate-ui` on neither `resonate-codec` nor any image decoding: `resonate-codec::Drawing`
    decodes and scales cover art into a `Raster`, reached through the engine's and library's
    re-exports, and gpui draws it. `resonate-ui` names `image` only for the `RgbaImage` and `Frame`
    gpui's `RenderImage` is built of; it is the workspace's one version, which gpui's requirement
    resolves to, so the type is the one gpui takes.
  - `resonate-online` is reached by the binary alone, optional behind `online`, so
    `cargo tree -p resonate-library` stays free of `ureq`, `serde` and `serde_json` and the
    `--exclude resonate-ui --no-default-features` build links no HTTP client. `resonate-mcp` is
    reached the same way behind `mcp`, off `resonate-online`, gpui and the UI; `resonate-discord`
    behind `discord`, on core, the engine's vocabulary and serde, reaching no gpui, library or HTTP
    client.
  - `resonate-listen` is on core, `resonate-pipewire`, `thiserror` and `tracing` alone, so the
    window and the binary hold a `Listener` and `Recognisers` without an HTTP client; the
    recognisers that reach one are `resonate-online`'s, handed in by the binary as the
    fingerprinters are. `cargo tree -p resonate-listen` stays free of gpui, the engine, the library
    and `ureq`.
  - `resonate-analysis` reaches `resonate-dsp` for `TruePeakMeter`, so a study's peak is read by
    the interpolator the playback guard rides the level down with; `resonate-dsp` still reaches no
    workspace crate but core.
  - `resonate-codec` is a *dev*-dependency of `resonate-mpris` alone: `tests/bus.rs` registers a
    `MediaProvider` holding a tag read open, and the trait the engine re-exports cannot be
    implemented without naming the codec's `Error`. The library half still sees the engine through
    `Player` alone, which `cargo tree -p resonate-mpris` is read for.

## Feature flags that are not optional

`default-features = false` silently drops things that matter. Check before trimming:

- **`symphonia`** needs `opt-simd` put back (default) for the SIMD FFT paths, and `pcm` for `wav`
  to decode rather than just demux. `aiff` and `caf` are on: `Container::Aiff` and `Container::Caf`
  are vocabulary, a variant nothing produces is a defect, and AIFF is a mainstream uncompressed
  container for this audience. No flag gives WavPack, Monkey's Audio or DSD — symphonia has no
  reader for any, hence `dsd/` is hand-rolled. `id3v1` / `id3v2` / `ape` are load-bearing for
  *demuxing*: the probe uses metadata readers to skip leading tags, and without them ordinary MP3s
  fail to probe.
- **`symphonia-codec-wavpack`** is the WavPack reader and decoder, registered on the codec crate's
  own registries beside symphonia's, default set empty, GPL-3.0-or-later (which the AGPL build
  takes). Its decoder misreads the extended bits a 32-bit or float stream keeps its low bits in,
  so it is never registered alone — `wavpack.rs` wraps it — and a version bump is weighed against
  the WavPack tests in `tests/encoded.rs`, which fail against the bare decoder.
- **`ape-decoder`** is the Monkey's Audio decoder; only `format::parse` and `FrameDecoder` are
  used, `ape.rs` being the symphonia reader and decoder around them; default set empty. Its
  arithmetic wraps by design, so its dev profile turns overflow checks off, and it narrows a
  32-bit stereo stream's side channel to 32 bits before undoing it, so `Ape` refuses that one
  shape rather than decode it wrong.
- **`pipewire`** needs `v0_3_50`: `PW_KEY_NODE_RATE` is gated behind `v0_3_33`,
  `PW_KEY_TARGET_OBJECT` behind `v0_3_44` (the keys the bit-perfect path needs),
  `pw_buffer.requested` behind `v0_3_49` and `pw_time.buffered` behind `v0_3_50` — which let the
  callback fill the quantum asked for rather than the pool's whole `maxsize` and report the frames
  the stream still holds. The feature is a *minimum daemon* version: it refuses a PipeWire older
  than 0.3.50 (a 2022 release), and `packaging/PKGBUILD` says so.
- **`ahash`** needs `std` for the `AHashMap` / `AHashSet` aliases and `runtime-rng` for the
  `Default for RandomState` impl `AHashMap::new()` requires.
- **`tracing-subscriber`** needs `env-filter` (not default) and `tracing-log` (default, keep it):
  gpui logs through `log` and its diagnostics vanish without the bridge.
- **`unicode-normalization`** is core's and the library's. Core's is `folded_letters`, the one fold
  of a name into bare letters, which the catalog keys an artist and its search index by and the
  lyric sidecar weighs a declared title with; the library's is `playlist::folded`, a playlist name
  lowercased then composed so one letter has one spelling in `folded` whatever was typed. Its
  `std` feature is not default, and it brings `tinyvec` alone, keeping core a short link.
- **`rustix`** is the binary's, for `termios` alone (beside `std`) — `resonate play`'s terminal a
  key at a time with no `unsafe`. It was in the lockfile under zbus and libspa, so it adds a
  feature, not a crate.
- **`unicode-width`** is what `Table` measures a column with rather than `chars().count()`: a CJK
  character takes two terminal columns and a combining mark none. Not `unicode-segmentation`, which
  counts graphemes and would still leave a CJK name a column short per character.
- **`image`** carries exactly the five formats `CoverArt` can name — `bmp`, `gif`, `jpeg`, `png`,
  `webp`. It is `resonate-codec`'s, so the audio-only build compiles it, and `resonate-vault`'s,
  which decodes and re-encodes covers; `resonate-ui` names it for the two container types above
  and decodes nothing through it; `resonate-library` takes it only as a dev-dependency;
  `resonate-analysis` names it not at all, a spectrogram being painted straight into a `Raster`.
- **`proptest`** is a dev-dependency of `resonate-core`, `resonate-library` and `resonate-ui`
  alone, with the default set dropped for `std`: the defaults bring `fork` and `timeout`, pulling
  `rusty-fork`, `tempfile` and `wait-timeout` to run each case in a child process, and no property
  here needs to survive its own crash. It adds three crates to the lockfile.
- **`notify`** is the library's and drops its default set, macOS's FSEvents alone; on Linux the
  inotify backend needs no feature.
- **`wayland-protocols`** needs `client` and `staging`: `ext-data-control-v1` is a staging protocol
  and is how `resonate-ui`'s clipboard sets the selection. It and `wayland-client` are
  `resonate-ui`'s alone, at the versions gpui links, so the lockfile gains nothing.
- **`ureq`** carries `rustls` and `gzip`, not `json`. It is the one HTTP client and blocking — the
  enrich pass is a thread and the lyric look runs on a background executor, so nothing wants a
  runtime; `rustls` is its TLS and `gzip` lets a service answer compressed. `json` is off on
  purpose: `Client::json` reads the body through `as_reader().take(limit + 1)` first, answering
  `TooLarge` past the limit, and hands the bounded bytes to `serde_json::from_slice`, so nothing
  uncapped is parsed and no parser of ureq's is needed. `gzip` only *reads*, so `flate2` is
  `resonate-online`'s own too — the crate and version ureq links, `rust_backend` alone — for the
  one request body sent packed, AcoustID's lookup.
- **`serde`** with `std` and `derive`, and **`serde_json`** with `std`, are `resonate-online`'s,
  `resonate-mcp`'s, `resonate-discord`'s and `resonate-subsonic`'s alone — and `serde` alone in
  `resonate-lyrics`, deriving the private documents a Lyricsfile is read into. Every
  `#[derive(Deserialize)]` is private: in online a `…Doc` mapped by hand to a `resonate-library`
  type, in mcp an argument struct turned into a domain value before a tool runs, in discord a doc
  for a frame Discord's IPC speaks — so the library and core stay free of both and a renamed field
  breaks one mapping in one crate, not a public type. `serde` is in the `--exclude resonate-ui
  --no-default-features` build already, through `zbus`; `serde_json` is not, and
  `cargo tree -p resonate --no-default-features -i serde_json` finding nothing is the guard.
- **`serde-saphyr`** is `resonate-lyrics`'s, with `deserialize` and none of its default set (its
  serialiser): it reads the YAML of a Lyricsfile — LRCLIB's answer or a sidecar's — and nothing
  writes one. It sits in the lyrics crate, not the online one, which reaches it through
  `read_lyricsfile` alone — so a build without `online` still reads a `.lyricsfile.yaml` beside a
  file. Pure Rust over its own `granit-parser`, it refuses a
  duplicated key by default (as the Lyricsfile draft asks a reader to) and takes a budget of nodes,
  depth and documents, which lets a document off the network be parsed at all. `serde_yaml` was
  archived and `yaml-rust2` has no serde side, which is the whole of why this one.
- **`rusqlite`** needs `bundled`, `cache`, `hooks` and `functions`. `hooks` gives
  `Connection::update_hook`, which `db::watch_the_names` registers to know when a name in `tracks`,
  `albums` or `artists` could have moved — what the kept spelling vocabulary is stamped against;
  deriving it from SQLite rather than a list of write paths keeps a later pass from silently
  leaving a stale suggestion. `functions` lets `schema::configure` register `words_of` on every
  connection, the one fold of a title SQL cannot spell, which weighs a single against the songs an
  artist's tracks carry.
- **`lofty`** is the tag *writer* and `resonate-codec`'s alone, and its
  `id3v2_compression_support` feature is not optional: with nothing put back 0.25.4 does not
  compile (`handle_compression`'s `#[cfg(not(...))]` twin names `Id3v2Error` and `Id3v2ErrorKind`
  without importing either), so it is carried and `flate2` with it. Reading stays symphonia's: two
  readers disagreeing about a file is worse than one, and a write is weighed against what the rest
  of the build sees.
- **`opus-rs`** is `resonate-codec`'s, with `std` and `heap` — the default set, named so nothing
  unused is carried. `std` is its runtime SIMD detection; `heap` boxes a decoder's large working
  state, otherwise several kilobytes of stack inline. Pure Rust with no crates of its own (one
  lockfile entry), and a decoder rather than a libopus binding because `unsafe_code = "forbid"`
  rules out writing one here and a C dependency is one more thing a package must link.
- **`smallvec`** is shared, with `union` and `const_generics` and none of its default set, for
  collections built over and over that are nearly always small: a cell's highlight runs and the
  folded words they match, a clause's alternatives and conditions, the peaks one Shazam frame
  finds, a PipeWire format's offered values, the events one engine pass drains, the visualiser's
  bars and traces and the window's heading parts. `union` makes the inline form a word smaller and
  `const_generics` lets an inline length be any constant. symphonia, `parking_lot` and gpui link
  it already, so it adds edges, no crate. A collection built once, or with no bound a listener
  could not exceed in ordinary use — a queue, a playlist, a catalog read — stays a `Vec`.
- **`resonate-subsonic`** reaches `ureq`, `serde`, `serde_json` and `md-5` itself, as a network
  provider is told to rather than through `resonate-online`; `md-5` was in the lockfile under
  lofty, so the token costs an edge, and the binary takes the provider only under `online`.
- **`parking_lot`** is `resonate-codec`'s too, for the lock a `Deadlined` stream holds its answers
  behind (`MediaStream` is `Sync`, `std`'s receiver is not); already in the lockfile, so an edge.
- **`futures-channel`** is `resonate-ui`'s, with `alloc` alone, for the oneshot `Drawer::draw`
  hands a cover back through from a thread gpui does not own; gpui links it, so an edge.
- **`rustfft`** is `resonate-dsp`'s, for the cepstrum a minimum- or intermediate-phase resampler is
  designed through, with `parking_lot` for the cell keeping each designed prototype for the run —
  two edges, no crate. It is `resonate-analysis`'s too, at the version symphonia's `opt-simd`
  locks and with the same `avx`, `sse` and `neon` features (an edge), and a dev-dependency of the
  library for the band-limited noise the studies' tests are made of. `rusty-chromaprint` is
  `resonate-analysis`'s — Chromaprint in pure Rust, MIT, bringing `rubato` — because
  `unsafe_code = "forbid"` rules out binding libchromaprint and an AcoustID lookup wants that
  algorithm's print exactly.
- **`toml_edit`** rather than `toml`, with `display` as well as `parse`. The settings pane rewrites
  the user's `config.toml`; a value model round-trips the *settings* but throws away the comments,
  key order and spacing the user wrote, and `display` emits the edited document.
