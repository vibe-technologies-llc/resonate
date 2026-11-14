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
nothing in it is ARM-only (`x86_64-apple-darwin` type-checks the same). **One Mac has run it**, a
MacBook Pro M4 under macOS 26 from the disk image (the window, drag and drop, playback, the
transport and the volume); `docs/FEATURES.md` holds what that run did not reach. The hosted runner
builds it and runs its tests, with no display and no output device. It is checked two ways:

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
  the Windows job's clippy and tests natively, and `cargo nextest run -p resonate -p resonate-ui`
  (`build.md`). The Core Audio tests skip where there is no output device
  (`RESONATE_COREAUDIO_TESTS=required` makes a skip a failure, for a machine that has one).

**What the runner refuses that Linux allows**, each met by its first run:

- **A Mac's temporary folder is under a symlink** (`/var` for `/private/var`), and the library
  names a root by its real path, so a test comparing a path it made with one the catalog answered
  disagrees. The job names `TMPDIR` by its real path before the tests; on a Mac,
  `TMPDIR=$(cd "$TMPDIR" && pwd -P) cargo nextest run`.
- **A process is refused threads past about two thousand**, where Linux allows tens of thousands:
  the window's harness lets go of its engine as each test ends (`ui.md`'s *Driven by tests*).
- **APFS refuses a name that is not UTF-8** (`EILSEQ`) and folds case: the tests of names that are
  not text are `cfg(all(unix, not(target_os = "macos")))`, and organise's test of a destination
  differing only in case takes the name it would link as already there.
- **A socket closed with bytes unread resets the connection and the reader loses what was sent**,
  where Linux still hands it over: the server reads a page's request out before closing
  (`server.md`), and a Cast test's fake keeps what it was fetching through a `STOP`.
- **A datagram to port 0 is sent**, so a test wanting a search that cannot be sent aims at the
  broadcast address from a socket never allowed to broadcast.
- **A watch on a Mac starts late and hears a little of the past.** FSEvents can miss what happens
  in the moments after a stream starts and deliver a folder made just before it, which reads as a
  change no test has settled, so `taken_away` answers nothing. A test acting on files made before
  its watch first proves the watch listens (`heard_a_write_first`).
- **The hosted runner's sleeps overshoot and it stalls threads for tens of milliseconds**, which the
  engine's transport tests read as a device falling behind. The job runs `resonate-engine`'s tests
  one at a time (the `ci` profile's `engine-alone` group in `.config/nextest.toml`), on top of what `audio.md`'s *Fixtures* holds them to.
- `stat` is BSD's: a test reading a mode asks `stat -c %a`, then `stat -f %Lp`.

`docs/FEATURES.md`'s *macOS parity* lists what is not built and what only a Mac can settle.
`unsafe_code = "forbid"` holds everywhere but `resonate-apple` (and the two crates
`dependencies.md` names): an Apple framework is reached through a safe wrapper crate (`keepawake`,
`trash`, `sys-locale`, `notify`'s FSEvents, gpui, `wry`) or through that crate's safe API.

## The Apple exception

`resonate-apple` is the third crate whose lints allow `unsafe_code` (approved by the listener;
`dependencies.md` has its shape, crates and features). It is empty off Apple targets, reaches no
workspace crate and is reached by `resonate-coreaudio`, `resonate-listen`, `resonate-mpris` and
`resonate-codec` on macOS and by `resonate-coreaudio` and `resonate-mobile` on iOS, which the layering job holds.

- **One unit for both systems.** `Unit::{playing, hearing}` is the AudioUnit sequence iOS's RemoteIO
  output and macOS's HAL output and input share (`Component::{RemoteIo, HalOutput}`), tested on
  every host against a fake toolbox; the render and input callbacks are `extern "C-unwind"` shells
  over `trait Render` and `trait Hear` that abort on a panic (`realtime.md`).
- **The HAL is macOS's alone**: `Device`, `Stream`, `PhysicalFormat`, `Hog`, `Volume`, `Mute`,
  `Listening`, each an owned value over one `AudioObject` property call.
- **`NowPlaying`** is the system's Now Playing and remote commands (below).
- **`PathMonitor`** is the Network framework's path monitor, its C calls declared by hand (no
  `objc2` crate covers Network); the monitor is cancelled and released on drop and the handler
  block is kept beside it, so an update never outlives what it calls (`mpris.md`'s *Is the
  connection metered*).
- Errors are `Error::Refused { op: Op, source: OsStatus }` and the typed cases beside it; an
  `OsStatus` prints as its four-character code where it has one.

## Where the rest is written

A Mac's parts are described where their subject is, not here:

| Subject | Rule |
|---|---|
| The Core Audio backend: two sinks a device, hog mode, the stream, the clock, hot-plug, volume | `audio.md`'s *The sink*, `realtime.md` |
| Microphone capture | `audio.md`, `dependencies.md` |
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
- **Only a main loop answers.** A handler is called on the main thread's run loop, so on macOS
  `Desktop::answers_media_keys` is the first zone's only where the host can raise a window or was
  told it is heard there (`NOW_PLAYING_NEEDS_A_MAIN_LOOP`, `Serving::heard_on_the_main_loop`).
  `resonate play` runs the main loop itself: `play_queue` hands its work to
  `resonate_mpris::while_media_keys_are_heard` (`resonate_apple::while_the_main_loop_runs`: the
  work on a scoped thread, the main thread in `CFRunLoopRunInMode` a quarter second at a time until
  it ends, resting a moment where the loop has nothing to run), which elsewhere just runs it.

## Notifications

**A song change is told as Linux's notification and Windows's toast are, word for word**:
`desk_notice.rs` is a Mac's `Desk`, fed by the same `toast_what_plays` and `Telling::of` the
socket's service runs, over `resonate_apple::Notices` (UserNotifications). `Notices::open` refuses
`Error::NoBundle` where the process has no bundle identifier (a `cargo run` binary; the framework
raises an Objective-C exception there rather than answering), so only `Resonate.app`'s executable,
the window or `resonate play` through the link `README.txt` names, raises any. It asks the alert
permission once (the system asks the listener the first time; a refusal is the system's to keep),
sets the one category `resonate.transport` whose actions are `notify::actions`' Previous,
Play/Pause and Next, and sets a process-wide delegate once (`ResonateNoticeAnswerer`, kept for the
life of the process) which shows a notice while the app is in front too, as a banner and in the
list, and hands a press to the `Notices` that raised it: a notice's identifier is
`<asker>.<kind>` (`track`, or `listen` for what Listen named, with no buttons), so each zone's
presses reach its own transport through `notify::pressed`, and a second telling of the same kind
replaces the first in Notification Center. A cover is copied into the temporary folder as
`resonate-notice-<pid>-<n>` before it is attached, since the system moves an attachment's file into
its own store. Teardown removes the track's notice. Nothing here has run on a Mac.

## Recording what the Mac plays

**`Listening::Desktop` is a process tap heard through a private aggregate device**
(`resonate_apple::DesktopTap`, opened for the whole sitting by `coreaudio_listener.rs` and dropped
after the capture stops): a global stereo `CATapDescription` excluding no process (so, as Linux's
monitor of the default sink, what Resonate plays is heard too), private and unmuted, made by
`AudioHardwareCreateProcessTap`, then `AudioHardwareCreateAggregateDevice` over a private
aggregate whose main sub-device is the default output and whose one sub-tap is the tap (drift
compensated, started by the aggregate), so the HAL input unit the microphone uses
(`coreaudio_capture.rs`, `Capture::open` taking the `Device`) reads the aggregate as it would any
input and `Sitting::until_full` reopens it on the same tap. **The tap's calls are looked up when
asked, never linked**: they came with macOS 14.2 and the bundle runs from 13, so `dlsym` finds
`AudioHardwareCreateProcessTap` and `AudioHardwareDestroyProcessTap` and the class is looked up by
name; a system without them is `Error::NoProcessTap`, which `resonate-listen` answers as
`Error::DesktopUnheard`. The system asks the listener once (*System Audio Recording*, the bundle's
`NSAudioCaptureUsageDescription`); a refusal reads as silence, which the sitting answers as
`NothingHeard`. Dropping the tap destroys the aggregate, then the tap. Nothing here has run on a Mac.

## Not on macOS yet

Game Mode;
the launcher's icon following the accent (`plain_launcher.rs`
does nothing, as on Windows).

## Release

`packaging/macos/build.sh` builds `Resonate.app` for `aarch64-apple-darwin` or
`x86_64-apple-darwin` at `MACOSX_DEPLOYMENT_TARGET=13.0` and wraps it as
`resonate-<version>-aarch64-macos.dmg` or `resonate-<version>-x86_64_v3-macos.dmg`
(`packaging.md`, `release.md`); `release.yml`'s `macos-dmg` job runs it on `macos-latest` for each,
and `publish` fills the Homebrew cask over both. The
bundle is signed ad hoc always, since an Apple Silicon binary must carry a signature to run, and
with a Developer ID and the notary only where the repository holds the secrets; none exists, so a
first run meets Gatekeeper and `README.txt` says how to open it. **Nothing here has run**: the
scripts' first run is the release that carries them.
