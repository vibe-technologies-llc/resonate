---
paths:
  - "**/Cargo.toml"
---

# Dependencies and manifests

- **Full versions, only used features**:
  `foo = { version = "1.2.3", default-features = false, features = ["bar"] }`, never `"1"`/`"1.2"`.
  A declared dependency nothing imports is a defect (bar the `libc` pin).
- **Shared deps in `[workspace.dependencies]`**; members write `foo.workspace = true`. New code:
  `crates/<name>/`, in `workspace.members` and, if depended on, `[workspace.dependencies]`.
- **Edition 2024, resolver 3** in `[workspace.package]`/`[workspace]`; members inherit
  `edition.workspace`, `version.workspace`.
- **Every member: `[lints] workspace = true`** (else `unsafe_code = "forbid"`,
  `result_large_err = "deny"` do not apply).
- **`unsafe_code = "forbid"` is never downgraded locally**; a binding needing `unsafe` is raised
  (it ruled out `pipewire::thread_loop`: `ThreadLoopBox`/`ThreadLoopRc` constructors are
  `unsafe fn`).

## The layering

A `cargo tree`-visible boundary is a `refuse` line in the CI layering job (`CLAUDE.md`: `cargo tree`
is the authority); add a crate or boundary here, add it there.

- `resonate-core`: no symphonia, pipewire, gpui, serde (else `resonate-dsp` stops being a leaf and
  the resampler's test cycle lengthens). Has `encoding_rs`, `chardetng` (pure Rust, no default
  features; the shared text reader, `rust-style.md`) and `tz-rs` (`Calendar`, the listener's zone,
  which statistics and chart bucket days by).
- `resonate-codec`, `-dsp`, `-pipewire` never depend on each other. `-dsp` stays off `resonate-eq`
  (equaliser arithmetic is `resonate-core::eq`). `-analysis` reaches `-dsp` for `TruePeakMeter` (a
  study's peak uses the playback guard's interpolator) and the Shazam signature's `Resampler`.
- `resonate-lyrics`, `-eq`, `-listen`, `-analysis`: none of gpui, engine, library, `ureq`.
  `-listen` is core + `-pipewire` alone (window holds a `Listener` with no HTTP client; the
  recognisers that reach one are `resonate-online`'s, handed in by the binary). `resonate-mpris`:
  no gpui, no library.
- `resonate-ui`: no `resonate-codec`, no image decoding of its own; `resonate-codec::Drawing`
  decodes and scales cover art to a `Raster` (via engine/library re-exports), gpui draws it.
- `resonate-online`, `-mcp`, `-discord`: binary-only, each optional behind its feature (`online`,
  `mcp`, `discord`), so `cargo tree -p resonate-library` has no `ureq`/`serde`/`serde_json` and
  `--exclude resonate-ui --no-default-features` links no HTTP client. `-mcp`: off `-online`, gpui,
  UI. `-discord`: core, engine vocabulary, serde; no gpui, library, HTTP client.
- `resonate-codec` is a *dev*-dependency of `resonate-mpris` alone (`tests/bus.rs` implements a
  `MediaProvider`, needing the codec's `Error`); the library half sees the engine via `Player` only.

## Feature flags that are not optional

`default-features = false` silently drops things that matter; check before trimming.

- **`symphonia`**: `opt-simd` (default; SIMD FFT); `pcm` (`wav` decodes, not just demuxes);
  `id3v1`/`id3v2`/`ape` (else ordinary MP3s fail to probe: probe skips leading tags via the metadata
  readers); `aiff`/`caf` (`Container::Aiff`/`Caf` are vocabulary; a variant nothing produces is a
  defect). No flag gives WavPack, Monkey's Audio or DSD: `dsd/` is hand-rolled, next two exist.
- **`symphonia-codec-wavpack`**: WavPack beside symphonia's; GPL-3.0-or-later (AGPL build takes it).
  Misreads the extended bits holding a 32-bit/float stream's low bits, so never registered alone:
  `wavpack.rs` wraps it; version bumps weighed against the WavPack tests in `tests/encoded.rs`.
- **`ape-decoder`**: Monkey's Audio; only `format::parse`, `FrameDecoder` used, `ape.rs` wrapping
  them as symphonia reader/decoder. Wraps arithmetic by design (dev profile: overflow checks off).
  Narrows a 32-bit stereo side channel before undoing it, so `Ape` refuses that shape.
- **`pipewire`** `v0_3_50`: `PW_KEY_NODE_RATE` (`v0_3_33`), `PW_KEY_TARGET_OBJECT` (`v0_3_44`) for
  bit-perfect; `pw_buffer.requested` (`v0_3_49`) so the callback fills the quantum asked;
  `pw_time.buffered`, `queued_buffers` (`v0_3_50`) for frames held and buffers queued ahead of the
  graph. A *minimum daemon* version; `packaging/PKGBUILD` and the spec say so.
- **`ahash`**: `std` (`AHashMap`/`AHashSet` aliases), `runtime-rng` (`Default` for
  `AHashMap::new()`).
- **`tracing-subscriber`**: `env-filter` (not default), `tracing-log` (default, keep: gpui logs via
  `log`; diagnostics vanish without the bridge).
- **`unicode-normalization`**: core's (`folded_letters`, the one name fold) and library's
  (`playlist::folded`); `std` not default.
- **`rustix`** keeps `unsafe` out of OS calls: `termios`, `getpgrp` (binary, `resonate play`'s
  terminal); `fs` xattrs (`resonate-codec`, carried onto a staged tag write); `getuid`
  (`resonate-discord`, socket-owner check).
- **`unicode-width`**: `Table` (and the readout) measure with it (CJK = two columns). Not
  `unicode-segmentation` (graphemes; `resonate-ui`'s, text editing).
- **`image`**: exactly the five formats `CoverArt` names (`bmp`, `gif`, `jpeg`, `png`, `webp`).
  `resonate-codec`'s, `-vault`'s; `-ui` for the two container types above; `-library` dev-only;
  `-analysis` never.
- **`proptest`**: dev-dependency of `resonate-core`, `-library`, `-ui` alone; `std` only (defaults
  run each case in a child process, which no property needs).
- **`notify`**: library's; default set dropped (macOS FSEvents alone); Linux inotify needs no
  feature.
- **`wayland-protocols`**: `client`, `staging` (`ext-data-control-v1`, staging, sets the clipboard
  selection). With `wayland-client`: `resonate-ui` alone, at gpui's versions.
- **`ureq`**: `rustls`, `gzip`, not `json`. The one HTTP client, blocking (enrich pass is a thread,
  lyric look a background executor). `json` off on purpose: `Client::json` reads through a bounded
  reader (`TooLarge` past the limit) then `serde_json::from_slice`, so nothing uncapped is parsed.
  `gzip` only *reads*, so `flate2` (`rust_backend` alone, ureq's version) is `resonate-online`'s for
  the one packed request body, AcoustID's lookup.
- **`serde`** (`std`, `derive`), **`serde_json`** (`std`): `resonate-online`, `-mcp`, `-discord` and
  the Subsonic, TIDAL, Monochrome providers alone; `serde` alone in `resonate-lyrics`. Every struct
  deriving `Deserialize` is private (online: a `...Doc` mapped by hand to a library type; mcp: an
  argument struct made a domain value before a tool runs; discord: a frame doc; mcp's unit enums
  `Action`, `Pass` are the only public derives), so library and core stay free of both and a renamed
  field breaks one mapping in one crate. The layering job refuses `serde_json` with gpui and `ureq`
  in `cargo tree -p resonate --no-default-features`; `serde` is there via `zbus`.
- **`serde-saphyr`**: `resonate-lyrics`' (`deserialize` only), reads a Lyricsfile (nothing writes
  one); in lyrics so a build without `online` still reads a sidecar `.lyricsfile.yaml`. Refuses
  duplicate keys by default (as the draft asks); budget of nodes, depth, documents makes a network
  document parseable at all.
- **`rusqlite`**: `bundled`, `cache`, `hooks`, `functions`. `hooks` = `update_hook`, used by
  `db::watch_the_names` to know when a name could have moved (derived from SQLite, not a list of
  write paths, so a later pass cannot leave a stale spelling suggestion). `functions`:
  `schema::configure` registers the `words_of` fold SQL cannot spell.
- **`lofty`**: tag *writer*, `resonate-codec`'s alone; `id3v2_compression_support` required (0.25.4
  does not compile without it). Reading stays symphonia's (two disagreeing readers are worse than
  one).
- **`opus-rs`**: `resonate-codec`'s; `std` (runtime SIMD detection), `heap` (boxes the decoder's
  large state). Pure-Rust, not libopus: `unsafe_code = "forbid"` rules out writing bindings and a C
  dependency is one more thing a package must link.
- **`smallvec`** (`union`, `const_generics`): collections built repeatedly and nearly always small;
  built once or unbounded in ordinary use (queue, playlist, catalog read) stays `Vec`.
- **`rustls-native-certs`**, **`webpki-root-certs`**, **`rustls`**: Subsonic, TIDAL, Monochrome
  providers', for a server behind a private CA (`providers.md`). First reads the system store;
  second is the Mozilla roots as DER (`RootCerts::Specific` takes certificates; no other way to add
  the built-in set beside the system's). `rustls` (`std` alone) only downcasts a refused
  certificate out of ureq's `io::Error`, so matches ureq's version. Not ureq's `platform-verifier`
  (on Linux reads the system store alone, fails where empty).
- **`resonate-subsonic`, `-tidal`, `-monochrome`** take `ureq`, `serde`, `serde_json` themselves
  (network providers are told to), not via `resonate-online`; binary takes them only under
  `online`. Subsonic: `md-5` (token). TIDAL: `base64` (`alloc`; base64 manifests), `roxmltree`
  (`std`, no `positions`; DASH MPD). gpui holds `roxmltree` 0.20, this is 0.21: a second copy of a
  small crate, not a held-back pin.
- **`futures-channel`**: `resonate-ui`'s (`alloc`); the oneshot `Drawer::draw` returns a cover by.
- **`rustfft`**: `resonate-dsp`'s, `-analysis`', at symphonia's `opt-simd` version with the same
  `avx`/`sse`/`neon`; library dev-dependency. `rusty-chromaprint`: `resonate-analysis`'s, pure-Rust
  (`unsafe_code = "forbid"` rules out libchromaprint bindings; AcoustID wants that exact print).
- **`toml_edit`**, not `toml`, with `display` and `parse`: the settings pane rewrites the user's
  `config.toml`; a value model loses comments, key order, spacing.

## Pins held against `cargo update`

- **`libc`** `=0.2.189` on `resonate-ui` (imports nothing from it): 0.2.190 removed `ENOATTR` on
  Linux; `xattr` 0.2.3 (under gpui) names `libc::ENOATTR`, so fails to compile. `=` stops
  `cargo update` moving the lockfile past it. Goes when gpui no longer reaches `xattr` 0.2.
