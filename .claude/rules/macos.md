---
paths:
  - "crates/resonate-apple/**"
  - "crates/resonate-coreaudio/**"
  - "crates/resonate-listen/src/coreaudio_*.rs"
  - "crates/resonate-listen/src/microphone.rs"
  - "crates/resonate-mpris/src/socket.rs"
  - "crates/resonate-mpris/src/listening.rs"
  - "crates/resonate-mpris/src/nowplaying.rs"
  - "crates/resonate-mpris/src/transport.rs"
  - "crates/resonate/src/awake.rs"
  - "crates/resonate/src/trashing.rs"
  - "crates/resonate/src/plain_launcher.rs"
  - "crates/resonate/tests/bundle.rs"
  - "crates/resonate-ui/src/app_menu.rs"
  - "crates/resonate-ui/src/keys.rs"
  - "crates/resonate-ui/src/system.rs"
  - "crates/resonate-ui/src/views/chrome.rs"
  - "packaging/macos/**"
---

# macOS

The whole workspace but the phones' boundary and PipeWire builds for `aarch64-apple-darwin`, and
nothing in it is ARM-only (`x86_64-apple-darwin` type-checks the same). **No Mac has run any of it.**
It is checked two ways, neither of which hears a note:

- **From Linux, types alone.** `rustup target add aarch64-apple-darwin`, a macOS SDK (the one
  `xtool setup` builds for iOS holds `MacOSX.sdk` beside `iPhoneOS.sdk`) and clang:
  `RUSTFLAGS= SDKROOT=<MacOSX.sdk> CC_aarch64_apple_darwin=clang AR_aarch64_apple_darwin=llvm-ar
  CFLAGS_aarch64_apple_darwin="--target=arm64-apple-macos13.0 -isysroot <MacOSX.sdk>"
  cargo clippy --target aarch64-apple-darwin --workspace --exclude resonate-mobile --exclude
  resonate-bindgen --exclude resonate-pipewire --all-targets -- -D warnings`. `gpui_apple`'s build
  script writes its Metal shader source only on a macOS host (its body is gated on the host's
  `cfg`), so the first run stops at `include_str!` of a missing `stitched_shaders.metal`; an empty
  file of that name in `target/aarch64-apple-darwin/debug/build/gpui_apple-*/out/` lets the check
  go on, and is made again after a `cargo clean` or a gpui bump. Nothing links.
- **On the hosted runner, built and tested.** CI's `macos` job (`macos-latest`, Apple Silicon) runs
  the Windows job's clippy and tests natively, and `cargo test -p resonate -p resonate-ui`
  (`build.md`). The Core Audio tests skip where there is no output device
  (`RESONATE_COREAUDIO_TESTS=required` makes a skip a failure, for a machine that has one).

`docs/FEATURES.md`'s *macOS parity* lists what is not built and what only a Mac can settle.
`unsafe_code = "forbid"` holds everywhere but `resonate-apple` (and the two crates
`dependencies.md` names): an Apple framework is reached through a safe wrapper crate (`keepawake`,
`trash`, `sys-locale`, `notify`'s FSEvents, gpui, `wry`) or through that crate's safe API.

## The Apple exception

`resonate-apple` is the third crate whose lints allow `unsafe_code` (approved by the listener;
`dependencies.md` has its shape, crates and features). It is empty off Apple targets, reaches no
workspace crate and is reached by `resonate-coreaudio`, `resonate-listen` and `resonate-mpris` on
macOS and by `resonate-coreaudio` and `resonate-mobile` on iOS, which the layering job holds.

- **One unit for both systems.** `Unit::{playing, hearing}` is the AudioUnit sequence iOS's RemoteIO
  output and macOS's HAL output and input share (`Component::{RemoteIo, HalOutput}`), tested on
  every host against a fake toolbox; the render and input callbacks are `extern "C-unwind"` shells
  over `trait Render` and `trait Hear` that abort on a panic (`realtime.md`).
- **The HAL is macOS's alone**: `Device`, `Stream`, `PhysicalFormat`, `Hog`, `Volume`, `Mute`,
  `Listening`, each an owned value over one `AudioObject` property call.
- **`NowPlaying`** is the system's Now Playing and remote commands (below).
- Errors are `Error::Refused { op: Op, source: OsStatus }` and the typed cases beside it; an
  `OsStatus` prints as its four-character code where it has one.

## Where the rest is written

A Mac's parts are described where their subject is, not here:

| Subject | Rule |
|---|---|
| The Core Audio backend: two sinks a device, hog mode, the stream, the clock, hot-plug, volume | `audio.md`'s *The sink*, `realtime.md` |
| Microphone capture; the desktop cannot be recorded (`Error::DesktopUnheard`) | `audio.md`, `dependencies.md` |
| The window's socket, the registry folder, the bus tried first | `mpris.md`'s *The client's second wire* |
| Folders, Finder's Trash, keep-awake, the host's name, `open` | `binary.md`, `library.md` |
| The language | `language.md` |
| Command and Control, the menu bar, the header beside the traffic lights, fonts, leaving, files from Finder | `ui.md` |
| The platform tag on the sync wire | `sync.md` |
| The disk image, the bundle, the icon, signing | `packaging.md`, `release.md` |
| CI's `macos` job and the layering lines | `build.md` |

## Now Playing and media keys

**A Mac's media keys never reach the window as keys** (gpui's macOS backend reads no system-defined
events), so the system's Now Playing is the only way they arrive, and `ui.md` binds no media key
there. `resonate-mpris`'s `nowplaying.rs` is `smtc.rs`'s twin: `Host::answers_media_keys` starts
the same `resonate-transport` thread, which reads `snapshot`, maps it through `controls_of` into
`transport::Controls` and tells `resonate_apple::NowPlaying` what moved (`Moved::between`): what is
offered (`Offered`, seeking only where the timeline says so), the state, the song (its cover read
from the `file://` URI `art::Pictures` laid, as bytes) and the clock (elapsed, the length where the
stream has one, a rate of one while playing and nought while held, told again every
`TIMELINE_EVERY`). `transport.rs`'s Windows codes (`Status::code`, `Pressed`, the repeat codes,
shuffle and repeat) are compiled for Windows and the tests alone, since Now Playing has no call
for them.

- **A press is queued, never settled where it lands.** MediaPlayer calls its handlers on the main
  thread, which is gpui's; the handler only `try_send`s the `Asked` into a channel of
  `PRESSES_WAITING` (16) and answers at once (`Failed` where it is full), and the transport thread
  takes it within its `POLL`, so a press is acted on in milliseconds and the window's thread never
  waits on the engine. Every request is a `PlayerInterface` method (`play`, `pause`, `play_pause`,
  `stop`, `next`, `previous`, a place asked for to `set_position`), so `mpris.md`'s *A method does
  what the spec says* holds for a media key here too.
- **Every MediaPlayer call is posted to the main queue and none is waited for**: teardown joins
  the transport thread from the main thread as the application ends, and a synchronous call back
  onto it would never return.
- **Only a window answers.** `resonate play` runs no main loop, so a handler registered there
  would never be called: the binary's `Desktop::answers_media_keys` is the first zone's and, on
  macOS, only where the host can raise a window (`NOW_PLAYING_NEEDS_A_WINDOW`).

## Not on macOS yet

Notifications (no bus `Desk` is opened and none is made for a Mac); whether the connection is
metered (it stays `Unknown`, so nothing is held back); recording the desktop's sound; Game Mode;
playing on with the window closed; the launcher's icon following the accent (`plain_launcher.rs`
does nothing, as on Windows); a Homebrew cask.

## Release

`packaging/macos/build.sh` builds `Resonate.app` for `aarch64-apple-darwin` at
`MACOSX_DEPLOYMENT_TARGET=13.0` and wraps it as `resonate-<version>-aarch64-macos.dmg`
(`packaging.md`, `release.md`); `release.yml`'s `macos-dmg` job runs it on `macos-latest`. The
bundle is signed ad hoc always, since an Apple Silicon binary must carry a signature to run, and
with a Developer ID and the notary only where the repository holds the secrets; none exists, so a
first run meets Gatekeeper and `README.txt` says how to open it. **Nothing here has run**: the
scripts' first run is the release that carries them.
