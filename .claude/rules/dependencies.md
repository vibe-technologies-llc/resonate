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

The listener approved three exceptions, each one crate whose explicit lints allow `unsafe_code` and
keep `result_large_err` denied: `resonate-mobile` admits UniFFI scaffolding and the owned-descriptor
bridge at the native boundary (`mobile.md`), `resonate-win32` holds the Win32 and COM calls no
reviewed safe crate wraps, behind a safe API (`windows.md`'s *The Win32 exception*), and
`resonate-apple` holds the Core Audio and AudioToolbox calls of iOS and macOS, behind a safe API
(below). No other crate weakens `unsafe_code = "forbid"`.

`resonate-apple` is shaped as `resonate-win32` is: it is empty on every target that is not Apple's
(each module is `cfg(target_vendor = "apple")` or narrower, the sequencing and pointer-handling
modules `cfg(any(target_vendor = "apple", test))` so their tests run on any host against a fake),
reaches no workspace crate, and every `unsafe` block is one call whose preconditions its types make
true (a unit is owned by `Unit`, which stops, uninitialises and disposes it on drop and frees what
its callback was handed exactly once; the raw `AudioUnit` never leaves the crate, so no caller can
hand a call a dangling one; a property is read into a `Plain` value of exactly its size; a hog is
an owned `Hog` released on drop only while this process still holds it; a listener is a `Listening`
whose callback is handed a number, never a pointer, looked up in a registry, so one firing after
its guard dropped finds nothing). It takes `objc2-audio-toolbox` and `objc2-core-audio-types` on Apple
targets and `thiserror`; on macOS alone `objc2-core-audio` 0.3.2 (`AudioHardware`, the device,
stream and property calls, with `objc2-core-audio-types` for the ranged format), `objc2-core-foundation`
0.3.2 (`CFString`, a device's name and UID; `CFRunLoop`, the main loop `resonate play` runs for Now Playing; the version gpui's tree already holds) and
`parking_lot` (the listeners' registry), and for `NowPlaying` (the system's Now Playing and its
remote commands, `MPNowPlayingInfoCenter` and `MPRemoteCommandCenter`) `objc2-media-player` 0.3.2
(the info center, the commands, their events and the artwork alone), `objc2`, `objc2-foundation`
(strings, numbers, data, the dictionary), `objc2-app-kit` (`NSImage` alone, the cover),
`block2` (the command handlers and the artwork's request) and `dispatch2` (every call is posted to
the main queue, never run synchronously), each at the version gpui's tree already holds, so no
second copy is built; for `Notices` `objc2-user-notifications` 0.3.2 (the center, the content,
request, category, action, attachment and response alone, with `block2`) and `objc2-foundation`'s
`NSArray`, `NSBundle`, `NSError`, `NSSet` and `NSURL`; for `DesktopTap` `objc2-core-audio`'s
`objc2` and `objc2-foundation` features (`CATapDescription`) and `objc2-foundation`'s `NSUUID`, the
tap's two calls found by a hand-declared `dlsym` since they are macOS 14.2's. `NowPlaying` holds no Objective-C object: each call fetches the shared center
inside its block, so it is `Send` and `Sync` and may be dropped on any thread. `mach_absolute_time` and `mach_timebase_info` are declared
by hand, since `libc` deprecates its own and `mach2` would be a second copy beside gpui's, and so
is the run-loop selector, the one constant behind `objc2-core-audio`'s deprecated feature, and so
are the Network framework's path monitor (`PathMonitor`, its update handler a `block2` block on a
`dispatch2` queue) and `clonefile` (`clone_file`, a tag write's staged copy cloned on APFS). It is
reached by `resonate-coreaudio`, on macOS `resonate-listen` (the microphone and the measuring
sweep), `resonate-mpris` (Now Playing and the path monitor, `macos.md`) and `resonate-codec` (the
clone, `library.md`) and, on iOS, `resonate-mobile` alone.

`resonate-win32` takes the `windows` crate (the `Win32_*` features it calls, listed in its manifest,
`Win32_System_Console` only under its `console` feature) and `windows-core` (`std`, for
`#[implement]` and `HSTRING`), at the `wasapi` crate's versions, so no second copy is built: `wasapi`,
`windows` (no features) and `windows-core` are written once in `[workspace.dependencies]`, and each
crate adds the features it calls, so a bump moves every user together. It
reaches no workspace crate and is reached by `resonate-wasapi`, `resonate-codec` and the binary
alone (the binary's tests take `console`); the layering job holds both. Its sequencing is compiled
for `test` on every host (`thiserror` and, for the tests, `parking_lot` are not Windows-only) and
its `windows` and `windows-core` stay `cfg(windows)`: `HResult` stands for `windows_core::Error`
in its errors.

Everywhere else Windows reaches its COM and Win32 calls through safe wrapper crates: `wasapi`
(`resonate-wasapi`'s device enumeration, endpoint notifications, render client and audio clock;
`resonate-listen`'s capture endpoints, loopback capture and measuring playback), `thread-priority`
(the fallback where MMCSS refuses the stream thread), `interprocess` (Discord's named pipe and the window's own, `resonate-mpris`), `keepawake` (the machine awake while playing), `windows` (`resonate-mpris`'s toasts, connection cost and media transport through the Windows Runtime's safe projections, eight features: `Data_Xml_Dom`, `Foundation`, `Media`, `Media_Playback`, `Networking_Connectivity`, `Storage`, `Storage_Streams`, `UI_Notifications`; the `wasapi` crate again for its safe apartment), `ctrlc` and `crossterm` (the binary's interrupts and key reading), `fs4` (the spool's free space), `trash` (the
recycle bin), `same-file` (core's same-entry test). Each is a `cfg(windows)` target dependency, so
none is built elsewhere, but for three a Mac takes as well, `cfg(any(windows, target_os = "macos"))`:
`keepawake` (an IOKit power assertion, idle sleep alone), `trash` (Finder's Trash through
`NSFileManager`'s `trashItemAtURL`, `DeleteMethod::NsFileManager`, since the crate's default there
scripts Finder and asks for the Automation permission; its `coinit_apartmentthreaded` feature
is Windows's and inert there) and `sys-locale` (the languages the listener put in order); the layering job refuses PipeWire, Wayland, `rustix` and `signal-hook`
from the Windows tree of the binary.

The binary's **build script** embeds the Windows version resource with `win-resource-builder`, a
`[target.'cfg(windows)'.build-dependencies]` entry (with `thiserror`) taken with
`default-features = false`: Cargo resolves a build dependency's `cfg` against the host, so only a
Windows host compiles it. It was chosen over `winresource`, `embed-resource` and `tauri-winres`,
which run `rc.exe` (`llvm-rc` or `windres` elsewhere) and so need a resource compiler on the runner
and a different one under `cargo xwin`, and over `embedinator`, which panics on bad input. The
crate writes a checked `.res` file in pure Rust, with golden tests against `llvm-rc`. **Its
`images` feature is off**: it brings `oxipng`, whose `libdeflater` builds C, and a C build is used
only where no Rust alternative covers the need. Without it the crate has no icon API and no public
way to insert a raw resource, so the icon is the binary's own `icon_resources.rs` (about 150 lines
with its error enum, no dependency): the committed `.ico`'s directory read field by field, each
image an `RT_ICON` resource numbered from 1, one `RT_GROUP_ICON` whose entries carry those numbers
where the file held offsets, in the same record layout. Its output was compared byte for byte with
`images`' own for this icon, once. The two parts are one `.res` file, which both `link.exe` and
`lld-link` take.

## gpui

- **gpui and `gpui_platform` are git dependencies of Zed's `main`, pinned by `rev`**, since the
  published 0.2.2 is long stale. `gpui_platform` carries the platform backends (`wayland` only on
  Linux; on macOS `resonate-ui` adds `font-kit`, without which `gpui_macos` draws no text, and
  `runtime_shaders`, which compiles the Metal shaders as the window opens so a build needs no Metal
  toolchain);
  `resonate-ui` builds its `Application` through `gpui_platform::application()`. A bump moves both
  revs together, and `cargo nextest run -p resonate-ui` is the check: the test scheduler refuses a wake
  from another thread, so `Driven::built_over` and the clipboard tests call
  `executor().allow_parking()`.

## The layering

A `cargo tree`-visible boundary is a `refuse` or `only` line in the CI layering job; add a boundary
here, add it there.

- `resonate-audio`: core, crossbeam-channel, `rtrb` (the handover) and thiserror alone; no PipeWire, codec, DSP or UI.
  The engine reexports these contracts; the Linux adapter implements them. `resonate-mobile` (Android and iOS) composes engine, codec, audio, library, analysis, lyrics, online, network providers, `resonate-upnp` and `resonate-cast`,
  never PipeWire, GPUI, MPRIS or Discord IPC; PipeWire is a Linux-target dependency of the engine and `resonate-listen`. The listener/recogniser vocabulary builds on Android;
  PipeWire capture is compiled only for Linux.
- `resonate-coreaudio`: core, `resonate-audio`, `resonate-words` (a hogged sink's name) and, on
  Apple targets, `resonate-apple` alone, with `crossbeam-channel`, `parking_lot` and, on macOS,
  `tracing`; no PipeWire, WASAPI, codec, DSP or gpui. Its portable halves build and are tested on
  every host. The engine takes it on macOS alone, and `resonate-mobile` on iOS (and as a
  dev-dependency, for its stream's host tests).
- `resonate-words`: no workspace crate; `fluent-bundle`, `unic-langid`, `tracing`, and `sys-locale`
  on Windows and macOS alone (`language.md`). `resonate-words-build` is a build dependency of each crate with
  words (and a dev-dependency, for the prose check), with `fluent-syntax` and `thiserror` alone. Every
  `only` line in the layering job admits `resonate-words`.
- `resonate-core`: no symphonia, pipewire, gpui, serde (else `resonate-dsp` stops being a leaf).
  Has `encoding_rs`, `chardetng` (the shared text reader, `rust-style.md`), `jiff` (`Calendar`),
  `parking_lot` (`Metering`, whose `Gate` a waiting thread sleeps on) and `resonate-words` (the
  labels, units, month names and Wrapped's sentences core hands every front end).
- `resonate-codec`, `-dsp`, `-pipewire` never depend on each other. `-dsp` stays off `resonate-eq`
  (the arithmetic is `resonate-core::eq`). `-analysis` reaches `-dsp` for `TruePeakMeter` and the
  Shazam signature's `Resampler`.
- `resonate-lyrics`, `-eq`, `-listen`, `-analysis`: none of gpui, engine, library, `ureq`. `-listen`
  is core + `-audio` + `-pipewire` alone on Linux and core + `-audio` + the `wasapi` crate on Windows,
  never `resonate-pipewire` or `resonate-wasapi` there (`resonate-wasapi` is its dev-dependency, which
  pins the endpoint naming the two agree on), and core + `-audio` + `resonate-apple` + `resonate-dsp`
  on macOS, never `resonate-coreaudio` there (its dev-dependency, pinning the node names the same
  way; `-dsp` because the HAL's input unit does not resample, so a microphone at another rate is
  brought to the clip's 48 kHz through `Resampler`; recognisers that reach HTTP are `resonate-online`'s, handed in by the
  binary). `resonate-mpris`: no gpui, no library; on macOS it reaches `resonate-apple`.
- `resonate-ui`: no `resonate-codec`, no image decoding of its own; `resonate-codec::Drawing`
  decodes and scales cover art to a `Raster`.
- `resonate-online`: composed only by the CLI/desktop binary and the mobile boundary.
  The phones need the paced metadata/lyric/scrobbling services but never a Linux audio dependency.
  `-mcp` and `-discord` remain binary-only. These are optional behind CLI features, so
  `cargo tree -p resonate-library` has no `ureq`/`serde`/`serde_json` and `cargo tree -p resonate --no-default-features` links no HTTP client. `-mcp`: off `-online`, gpui, UI. `-discord`: core,
  engine vocabulary, serde; no gpui, library, HTTP client.
- A provider crate reaches core, the seam and (a network one) `resonate-fetch` alone.
- `resonate-upnp`: core and `resonate-audio` alone, with `httparse`, `mio` (`os-poll` and `net`: the
  stream server's one thread woken by readiness rather than polling; the safe API alone, since the
  crate forbids `unsafe`), `roxmltree`, `flacenc` (the live
  FLAC stream, pure Rust, the vault's own encoder), `parking_lot`,
  `crossbeam-channel`, `thiserror` and `tracing`; no PipeWire, gpui, `ureq`, serde or rustls (a
  renderer is plain HTTP on the local network, `upnp.md`). The engine and the codec are its dev-dependencies alone.
- `resonate-cast`: core, `resonate-audio` and `resonate-upnp` (the paced stream, `cast.md`) alone, reached by the binary and `resonate-mobile` alone,
  with `rustls` (`ring`, `tls12`: a receiver's TLS), `mdns-sd`, `parking_lot`,
  `crossbeam-channel`, `thiserror` and `tracing`; no PipeWire, gpui, `ureq`, serde or `serde_json`
  (its protobuf and JSON are hand-written, so the binary without `online` still casts). The engine,
  codec, `rcgen` and `serde_json` are its dev-dependencies alone.
- `resonate-musicbrainz`: core and `resonate-library` among workspace crates, with `resonate-fetch`
  and `ureq` only under its `download` feature (the binary's `online` turns it on, the phones
  leave it off); no gpui, engine, `resonate-online`, `resonate-sync`, serde, quinn or tokio;
  reached by the binary and `resonate-mobile` alone. **`bzip2`** keeps its default features,
  which are its pure-Rust `libbz2-rs-sys` backend (the C `bzip2-sys` is the other, off); **`tar`**
  has no default features (`xattr` is not read). Both read the MusicBrainz export alone.
- `resonate-cli`: `resonate-words` alone among workspace crates, with `clap`; reached by the binary
  alone, as a dependency and as its build script's, so the grammar the man pages are written from is
  the one parsed.
- `resonate-signin`: core alone among workspace crates, reached by the binary alone, and only
  without its `window` feature; **`wry`** (`os-webview` alone) and **`tao`** (`rwh_06` alone: no
  `x11`, no `dbus`, since the window runs on Wayland through GTK) come in only under `window`, which
  the executable needs, so the binary's tree holds no browser engine (the layering job). **A
  `--workspace` build selects `resonate-signin` with its default `window` feature and Cargo merges
  it into the library the player links**, so every recipe builds the player as `--package resonate
  --bin resonate` and the sign-in program in a `cargo` run of its own
  (`no_recipe_builds_the_player_beside_the_sign_in_windows_engine`); the Flatpak's SDK has no
  WebKitGTK at all.
- `resonate-sync`: core alone among workspace crates; no library, engine, gpui, `ureq`, serde.
  Reached by the binary (behind `sync`), `resonate-driven`, `resonate-mobile` and `resonate-server` alone; `resonate-server`
  reaches core, `resonate-sync` and, for its hifi-api gateway, `resonate-tidal` with the seam and
  `resonate-fetch` it already reaches (so `ureq` and serde, which the sync server otherwise lacks). `resonate-library` takes neither `quinn` nor `tokio`, and
  `cargo tree -p resonate --no-default-features` links neither.
- `resonate-driven`: core, the engine and `resonate-sync` alone among what it names, the one crate
  seeing both sides of `Controlled`; no gpui, library, `ureq` or serde; reached by the binary (behind
  `sync`) and `resonate-mobile` alone.
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
- **`unicode-normalization`** (`std`): core's `folded_letters`, library's `playlist::folded` and the phones' boundary (`playlists.rs`, `sync.rs`).
- **`rustix`**: no `unsafe` in OS calls: `termios`, `getpgrp` (binary), `fs` xattrs
  (`resonate-codec`), `getuid` (`resonate-discord`), `time`'s `clock_gettime(Monotonic)`
  (`resonate-pipewire`'s callback and Oboe's, Android alone, to read how far a device timestamp
  lies from now; `realtime.md`), `system`'s `uname` (the binary alone: the host a sync device is
  named for), `fs::statfs` (the library on macOS: the type a root's volume says it is).
- **`unicode-width`**: `Table` and the readout (CJK = two columns). `unicode-segmentation` is
  `resonate-ui`'s, text editing.
- **`jxl-encoder`** (`std`, `jpeg-reencoding`; no `butteraugli-loop`): `resonate-vault`'s alone, the
  JPEG-to-JXL transcode with its `jbrd` reconstruction data (`vault.md`). Pure Rust (`zenjpeg`,
  `brotli`, `archmage`); licensed AGPL-3.0-only OR commercial, taken under the AGPL, which this project
  is. `jxl-oxide` reads it back and reconstructs the JPEG, so no second JXL decoder joins.
- **`image`**: exactly the five formats `CoverArt` names (`bmp`, `gif`, `jpeg`, `png`, `webp`).
  `resonate-codec`, `-vault`, `-ui`; `-library` dev-only; `-analysis` never.
- **`proptest`**: dev-dependency of `resonate-core`, `-library`, `-sync`, `-ui` alone; `std` only (defaults
  fork a child process per case).
- **`notify`**: library's; defaults dropped, Linux inotify needs no feature. On macOS the crate
  compiles its FSEvents watcher whatever the features say and takes that watcher's objc2 crates only
  under `macos_fsevent`, so the library names the feature as a `cfg(target_os = "macos")` target
  dependency (without it the crate does not build there); `macos_kqueue`, the other way out, holds
  a descriptor a watched file.
- **`wayland-protocols`**: `client`, `staging` (`ext-data-control-v1` sets the clipboard). With
  `wayland-client`: `resonate-ui` alone, at gpui's versions.
- **`ureq`**: `rustls`, `gzip`, not `json`; the one HTTP client, blocking. `json` is off because
  `Client::json` reads through a bounded reader (`TooLarge` past the limit) then
  `serde_json::from_slice`, so nothing uncapped is parsed. `gzip` only reads, so `flate2`
  (`rust_backend`) is `resonate-online`'s for AcoustID's packed request body and `resonate-sync`'s
  for a sealed record's deflate (`seal.rs`).
- **`serde`** (`std`, `derive`), **`serde_json`** (`std`): `resonate-online`, `-mcp`, `-discord` and
  the Subsonic, Jellyfin, TIDAL, Monochrome and YouTube Music providers alone (`serde_json` also a
  dev-dependency of `resonate-cast`); `serde` alone in `resonate-lyrics`. Every struct
  deriving `Deserialize` is private (online: a `...Doc` mapped by hand; mcp: an argument struct made
  a domain value before a tool runs; discord: a frame doc; mcp's unit enums `Action`, `Pass` are the
  only public derives), so a renamed field breaks one mapping in one crate. The layering job refuses
  `serde_json` beside gpui and `ureq` in `cargo tree -p resonate --no-default-features`; `serde` is
  there via `zbus`.
- **`serde-saphyr`**: `resonate-lyrics`' (`deserialize` only), reads a Lyricsfile, so a build
  without `online` still reads a sidecar `.lyricsfile.yaml`. Its node, depth and document budgets
  make a network document parseable at all. **`roxmltree`** (`std`) is `resonate-lyrics`' for TTML
  (`read_ttml`), with no DTD and a node limit, for the same reason.
- **`rusqlite`**: `bundled` (a C build of SQLite, kept under *C-backed crates kept*), `cache`, `hooks`, `functions`. `hooks` feeds `db::watch_the_names`
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
  behind a private CA (`providers.md`), and `resonate-sync`'s for the QUIC connection's roots
  (`quic.rs`). The first reads the system store; the second is the Mozilla
  roots as DER (`RootCerts::Specific` is the only way to add the built-in set beside the system's).
  `rustls` (`std` alone) downcasts a refused certificate out of ureq's `io::Error`, so matches
  ureq's version. Not ureq's `platform-verifier`, which fails where the system store is empty.
- **`resonate-subsonic`, `-jellyfin`, `-tidal`, `-monochrome`** take `ureq`, `serde`, `serde_json` themselves
  and `resonate-fetch` for shared plumbing, not `resonate-online`; the binary takes them only under
  `online`. `resonate-fetch` is core, the seam, `ureq` and the three certificate crates alone;
  `resonate-online` takes it for `retry_after`, `sha2` (pure Rust, no default features, also
  `resonate-musicbrainz`'s) and `base64` for a Spotify sign-in's PKCE challenge. `uuid`: Jellyfin's
  generated device id and `resonate-online`'s. `md-5`: Subsonic's token (and Last.fm signing in
  online). TIDAL: `base64` (`alloc`), `roxmltree` (`std`; DASH MPD), at 0.21 against gpui's 0.20:
  a second copy of a small crate, not a held-back pin.
- **`resvg`** (`text`, `system-fonts`): `resonate-ui`'s, for the Wrapped card (`wrapped_card.rs`), at gpui's 0.46 so one SVG stack (`usvg`, `fontdb`, `tiny-skia`, `rustybuzz`) is built and one system font database loaded; a newer `resvg` would build a second stack beside gpui's (`cargo tree -d -p resonate-ui` names none). Bump it with gpui.
- **`futures-channel`**: `resonate-ui`'s; the oneshot `Drawer::draw` returns a cover by.
- **`hdf5-reader`** (no default features, so no `rayon` and no `lz4`): `resonate-codec`'s SOFA reader alone (`eq.md`). Pure Rust, read only; its `memmap2` and `libc` are not used for a set (`from_vec`). **`hdf5-writer`**: dev-dependency of `resonate-codec`, `-engine` and `-mobile`, building the tests' sets. Both are versions of one family (`hdf5-core`), moved together.
- **`rustfft`**: `resonate-dsp`'s, `-analysis`', `-codec`'s (`sofa.rs`), library dev; at symphonia's `opt-simd` version.
  **`realfft`**: `resonate-dsp`'s convolver, over the same `rustfft`, with the same SIMD features.
  **`wide`** (no default features, pure Rust): `resonate-dsp`'s polyphase sum alone,
  its `f64x8` the lanes LLVM would not reliably pack (`audio.md`); it picks AVX-512, AVX or SSE2 by the
  build's target features, so the release baseline sums in AVX and the Flatpak's in SSE2.
  **`rusty-chromaprint`**: `resonate-analysis`'s, pure Rust, since AcoustID wants that exact print.
  **`rayon`**: `resonate-analysis`'s, pure Rust, no default features: the crews a pass fans out over (`analysis.md`), since a stage may wait on its own parallel search without holding a thread idle, and the forbidden `unsafe_code` rules out a scoped pool of our own.
- **`quinn`** (`runtime-tokio`, `rustls-ring`) and **`tokio`**: the sync stack alone
  (`resonate-sync`, `resonate-server`), not the HTTP client's way: hole punching needs UDP, so the
  server connection and peer transfers are QUIC. The client confines tokio to one current-thread
  runtime on its own thread (`rt`, `net`, `time`, `sync`, `macros`, `fs`, `io-util`); the server
  takes `rt-multi-thread` and `signal`. quinn's `platform-verifier`, `log` and `bloom` stay off.
  **`tokio-tungstenite`** (`handshake` only, pure Rust) and **`futures-util`** (`std`, `sink`) carry
  the same frames over a WebSocket for a server behind a proxy with no UDP (`server.md`); TLS for
  it is `tokio-rustls` on `ring`, and neither pulls serde into `resonate-sync`.
  `resonate-server` takes `tokio-tungstenite` as a dev-dependency alone, for the test that speaks
  WebSocket itself.
- **`ring`** (direct): sealing (ChaCha20-Poly1305), HKDF, SHA-256, randomness and the peer
  signature check in `resonate-sync`; the crate rustls already links. **`x25519-dalek`**
  (`static_secrets`, pure Rust) is `resonate-sync`'s Diffie-Hellman for sealing a removal's new
  code to each person who stays (`shared/moved.rs`), the one piece of its cryptography `ring` does
  not carry. **`rcgen`** (`ring`): the
  space's peer certificate (a dev-dependency of `resonate-server` and `resonate-cast`, for the tests'
  self-signed ones). **`rustls-pki-types`** (`std`):
  PEM reading (`sync-server-certificate`, the server's chain and key). `rustls` takes `ring` there
  for the provider QUIC needs. The wire is hand-encoded, so no serde.
- **`mdns-sd`** (`async`): pure Rust, its own daemon thread. `resonate-sync`'s Bonjour awaits its
  events on the sync runtime; `resonate-cast` browses `_googlecast._tcp` with blocking receives on
  a thread of its own, the `async` feature unused there but unified with sync's.
- **`jiff`**: `resonate-core`'s, the one zone database (`Calendar`): `std`, `tz-system` (the
  desktop's zone, `Calendar::local`) and `tzdb-zoneinfo` (a zone by name from `/usr/share/zoneinfo`:
  Linux, Flatpak, iOS). The phones add `tzdb-bundle-always`, and Android `tzdb-concatenated`, as
  target dependencies (`library.md`'s *A day is the listener's*); Windows has no zone database on disk,
  so its target dependency adds `tzdb-bundle-platform`, which bundles one there alone; Linux and
  macOS bundle nothing, since the system database is always there and updated with the system.
- **`toml_edit`**, not `toml` (`display`, `parse`): the binary's settings pane rewrites the listener's
  `config.toml`, and `resonate-server` reads its gateway's, since a value model loses comments, key
  order and spacing.

- **`uniffi`** 0.32.2: the mobile boundary's scaffolding (Kotlin and Swift), with `cli` only in
  `resonate-bindgen`.
  **`oboe`** 0.6.1 (`shared-stdcxx`): Android-target-only; NDK libc++ ships in the APK.
  **`objc2-audio-toolbox`** 0.3.2: `resonate-apple`'s on Apple targets alone, pure-Rust framework
  bindings for the RemoteIO and HAL units, only the features it calls. **`objc2-core-audio-types`**
  0.3.2 with `CoreAudioBaseTypes` alone is plain C structs and builds on every host, so
  `resonate-apple` takes it as a dev-dependency everywhere (the buffer list's and the unit's tests
  run on the host) and as a dependency on Apple targets; `objc2` itself, which refuses a non-Apple
  target, comes in only with the toolbox.

## C-backed crates kept, and duplicates third parties force

- **`rusqlite`** (`bundled`, via `libsqlite3-sys`): the catalog, the sync ledger, the server's and the phones' store.
  SQLite is built from source into every binary and the phones' library, so one version, with FTS5,
  triggers and the update hook the schema relies on, runs on every host and no system library is
  asked for; no pure-Rust engine carries those.
- **`zstd`** (via `zstd-sys`): `resonate-vault`'s, and `resonate-musicbrainz`'s with
  `zdict_builder`. The WAVE form packs at `ARCHIVED_AT` (19) in
  pledged frames, and `the_encoders_an_object_is_stamped_for_are_the_ones_this_build_runs` pins the
  encoder's output; the pure-Rust `ruzstd` encodes to roughly level 11 at best, so a switch would
  weigh every vaulted WAVE again at a worse ratio. The decoder side (`Unpacking`) is the same crate. The MusicBrainz copy packs its blocks with a
  dictionary trained per kind of document (`musicbrainz.md`), which `ruzstd` cannot write at all (it
  writes its fastest level alone, with no dictionary), and both the binary and the phones link the
  vault's copy already, so no second C library joins.
- **`ring`** (C and assembly): the provider rustls runs on for `ureq` (`resonate-fetch`), `quinn`
  and a Cast receiver's TLS (`resonate-cast`, so also in the binary without `online` or `sync`),
  linked whether or not `resonate-sync` names it; `resonate-sync` uses the same crate directly for
  sealing, HKDF, SHA-256, randomness and the peer signature check rather than link a second provider.
- **`parking_lot` 0.12** beside the workspace's 0.13 (with `lock_api` and `parking_lot_core`): gpui,
  `wgpu-sync`, `crossterm` and `hdf5-reader` (under `resonate-codec`'s SOFA reader) take 0.12, and
  `resonate-ui`'s `RecursiveRwLock` (`theme.rs`) exists from 0.13, so the workspace cannot come down
  to share one copy.
- **`base64` 0.22** beside the workspace's 0.23: `usvg` (under gpui) takes 0.22 while `ureq`,
  `ureq-proto` and `wry` take 0.23, so the workspace could not come down to 0.22 without still
  building 0.23 for the HTTP client every provider crate reaches. **`sha2` 0.10** beside 0.11 (with
  `digest`, `block-buffer` and `crypto-common`): `oo7` (gpui) takes 0.10 and `wry` too; the
  workspace's `sha2` and `md-5` are 0.11, and moving both to 0.10 would still build `digest` 0.11
  for the `sha1` `tungstenite` (under `resonate-sync`) takes, so a held-back pair would remove two
  crates and keep the third.
- **`md-5` 0.10** beside the workspace's 0.11: `ape-decoder`, `flacenc` and `oo7` take it; **`miniz_oxide`
  0.8** beside 0.9: `png` and `exr` (under `image`, and `gpui`'s) take it while `flate2` takes 0.9.
  None is ours to move; each goes when its dependent moves.
