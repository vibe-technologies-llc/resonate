---
paths:
  - "ios/**"
  - "crates/resonate-mobile/src/output/remote_io.rs"
  - "crates/resonate-mobile/src/output/remote_io_render.rs"
---

# iOS

The iOS front end is SwiftUI, a SwiftPM package under `ios/` built, signed and installed from Linux
with xtool (`xtool.yml`: bundle `org.resonate.ios`, `Resonate/Info.plist`, `Resonate/Icon.png`,
`Resonate/Resonate.entitlements`). iOS 26 is the minimum, so the app draws with Liquid Glass and
needs no older-platform branch. The host needs Swift 6.4, the xtool AppImage (1.22.2 or later; older builds miss Xcode 27's SDK),
`zip` for `--ipa`, the `darwin` Swift SDK
`xtool setup` builds from Xcode's `.xip`, the `aarch64-apple-ios` Rust target, clang and `llvm-ar`.
None of this is in CI: the SDK cannot be fetched without the listener's Apple ID.

## The native library

`ios/build-native.py [--profile debug|release]` builds `resonate-mobile` as a staticlib for
`aarch64-apple-ios` into `ios/.cargo-target` (`RUSTFLAGS` reset, so no `target-cpu=native`;
`IPHONEOS_DEPLOYMENT_TARGET` 26.0; C dependencies compiled by clang against the SDK's
`iPhoneOS.sdk`), generates the Swift bindings, and lays out `ios/Generated/`: `ResonateNativeFFI` (the
header and a module map that links `resonate_mobile`), `ResonateNative` (the bindings) and `lib/`.
`Package.swift` takes those as a system-library target and a Swift target and links the archive
from `Generated/lib` with AudioToolbox, CoreAudio, Security and SystemConfiguration. Everything under
`ios/Generated`, `ios/.cargo-target` and `ios/.build` is generated, never checked in or edited.

## Hardening

The entitlements opt into Enhanced Security: `hardened-process`, `enhanced-security-version-string`
2 (guard objects), `checked-allocations` (Memory Integrity Enforcement: hardware memory tagging on
A19 and later, no effect before) enforced rather than soft, and `dyld-ro`. They sit in the
unrestricted `com.apple.security.` namespace, so no provisioning-profile capability is needed.
Pointer authentication is not adopted: it needs an `arm64e` slice, and the Rust library builds for
`aarch64-apple-ios` on stable.

## Shape of the app

- **`Library`** (`@MainActor @Observable`) owns the one `MobilePlayer`. Transport, favourite and
  preference actions enter one ordered queue (`Work.inOrder`, an `AsyncStream` consumed in order);
  long native calls (analysis, search, downloads, sync) run on `Work.aside`, never the main actor.
  A 250 ms poll reads the snapshot and queue and saves every 20th tick, as Android does.
- **`Features`** mirrors Android's `LibraryFeatures`: request generations refuse stale results, the
  same 30 s scrobbling loop, 2 s / 30 s sync polling, the same download and sync-landing journal
  (`unfinished-download`, `unfinished-sync-landing`, deleted on the next start). Every busy flag
  is cleared in `work`'s `finally`, which runs on every path, a failed `library.player()` included;
  it clears only while its own generation is current. The Inspector's artist lookup
  (`inspectorArtist`, an `ArtistLookup`) is its own state, apart from the online search page's
  `artist`, and survives the Inspector reappearing for the same song. Scrobbling accounts reach the
  keychain before scrobbling starts, so a refused start keeps what was typed.
- **Analysis runs while its page is open**: `.task(id: track.id)` starts it unless a report for
  that song is already held, leaving the page cancels it (`cancelAnalysis`), and progress is
  indeterminate until the engine reports a fraction. A silent level reads "Silent", never
  `-inf`; the spectrum arrives in dB and is drawn from a 120 dB floor, as Android does.
- **`NearbyDevices`** (`Nearby.swift`) is how an iPhone meets devices on its network without a
  server: sync's own beacons are multicast and broadcast, which need the
  `com.apple.developer.networking.multicast` entitlement a free signing cannot carry, so it
  advertises the record `syncNearbyRecord` hands out as `_resonate-sync._udp` with an `NWListener`
  (a random instance name, never the device's) and passes every record its `NWBrowser` finds to
  `syncNearbyHeard`. All of its state lives on its own queue; the sync poll drives it, and a `nil`
  record (sync off, or transfers not allowed) withdraws both. `Info.plist` lists the type under
  `NSBonjourServices` (`sync.md`'s *Nearby*).
- **`Folders`** is the `Documents` callback. A chosen folder is a security-scoped bookmark kept in
  Application Support (`folders.plist`); the app's own Documents folder (`UIFileSharingEnabled`,
  `LSSupportsOpeningDocumentsInPlace`) is root `local`. A scan walks at most 32 levels, admits
  `audio_extensions()`, reads a folder's cover/folder/front JPEG or PNG within 8 MiB and a
  same-stem `.lrc` within 1 MiB. Fresh destinations are opened `O_CREAT | O_EXCL`. A file opened
  in Resonate from elsewhere is copied into `local` and played. A folder's access is measured, not
  assumed: `FolderAccess.of` reads `isReadableFile`/`isWritableFile` and the URL's
  `isReadable`/`isWritable` while the security scope is open, so a read-only grant is never offered
  as a destination for downloads or synced songs. Access is cached per folder: `folders()` resolves
  only a bookmark it has not measured, `refreshed()` re-measures every one on `Work.aside` (the
  folder and sync pages call it), and a scan re-measures its own root. A folder whose resolved,
  symlink-free path is already granted, or is the app's Documents, is refused
  (`FolderFailure.alreadyAdded`). A scan runs on a GCD queue where `Task` cancellation cannot reach,
  so it walks under a `ScanStop` flag; removing a folder stops its walk.
- **`NowPlaying`** owns the audio session (`.playback`, long-form), reports the route to Rust,
  pauses on an interruption (resuming when the system says so) and when the old route disappears,
  and drives the remote commands and now-playing info (artwork bounded at 384 px, rendered at scale 1 off
  the main actor).
- **A closure a framework may call off the main thread is never formed on the main actor.** Swift 6
  makes a non-`@Sendable` closure written inside a `@MainActor` type main-isolated and asserts the
  queue when it runs: `MPMediaItemArtwork`'s image handler, called on a MediaPlayer queue, trapped in
  `_dispatch_assert_queue_fail`. Such a closure is built in a `nonisolated` function or marked
  `@Sendable` (`NowPlaying.artwork`, `Features.watchNetwork`); `MainActor.assumeIsolated` only where
  the framework delivers on the main thread (remote commands, notifications on `.main`).
- Secrets (provider credentials, scrobbling, AcoustID key, sync key) live in the keychain,
  `AfterFirstUnlockThisDeviceOnly`; plain choices in `UserDefaults` suites named as Android's
  `SharedPreferences` files, with the same keys.

The views follow Android's screens and wording (`android.md`): tabs Songs, Library, Queue, Settings;
the mini player in the tab bar's bottom accessory; Now Playing as a full-screen cover over the
backdrop; the output page holds `AVRoutePickerView`, since iOS owns the route.

## The backdrop

`BackdropView(track:)` (`Backdrop.swift`) is the flowing field behind Now Playing, the lyrics, the
visualizer and the album and playlist pages.

- **The palette** (`Palette`, `Backdrop.palette` in `Artwork.swift`, off the main actor) is five
  linear-sRGB colours with weights: a 32 by 32 sample of the cover in Oklab, k-means seeded by
  lightness quantiles, six rounds. Near-black, near-white and grey pixels are dropped where at least
  a third of the cover is vivid; a cover less than a sixteenth vivid is colourless and takes the
  palette built around the accent colour, as does a song without a cover. Each colour is held to
  Oklab lightness 0.30 to 0.64 and chroma 0.07 to 0.19 (a grey stays grey), then pulled into gamut.
  Palettes are remembered per song and art revision, so a page opens on its field at once.
- **The shader** is Metal Shading Language in a Swift string (`BackdropShader.source`), compiled once
  per process with `makeLibrary(source:)` on a utility task (`BackdropPipeline`): the offline
  `metal` compiler in the SDK is an arm64 Mach-O that cannot run on the Linux host, so the package
  carries no `.metal` file and no `.metallib`, and SwiftUI's `ShaderLibrary` is unusable. One
  fullscreen triangle; the uniforms (`FieldUniforms`, 192 bytes, mirrored by the shader's `Field`,
  checked before the pipeline is built) are passed with `setFragmentBytes`. Five Gaussian blobs
  drift on Lissajous paths, the domain warped by two octaves of value noise; colours mix in Oklab
  with their chroma partly kept, so complementary blobs do not meet in grey; then a lightness
  curve, a vignette and triangular dither on the sRGB-encoded output (`bgra8Unorm`). Every period divides 360 s, the
  length after which the clock wraps without a seam. The view draws at scale 1; the compositor
  scales the blurred field up.
- **A song change crossfades** the old field into the new over 1.2 s, eased in and out, in the shader;
  a change mid-fade starts from the blend then on screen.
- **Energy** is the RMS of the visualizer tap (`Features.samples`), with a 120 ms attack and a 600 ms
  release, raising drift speed and blob size by at most a quarter. Paused, energy falls to zero and
  the drift slows to 18 %. `Features.visualize` is reference-counted, so a backdrop and the
  visualizer page each hold the tap without switching off the other's; a backdrop holds it only
  while visible, active and moving.
- **Reduce Motion** draws one still frame (time frozen, no energy, no fade) and redraws only when
  the palette changes; **Low Power Mode** caps at 30 fps; an inactive or backgrounded scene, or a
  view out of its window, draws nothing.
- **Failure falls back to the gradient**: no Metal device, a failed compile or pipeline, or a
  uniform size the shader does not expect is logged in full at `.error` under subsystem
  `org.resonate.ios`, category `backdrop` (a successful compile logs its time at `.notice`), and the
  view draws two radial gradients from the palette's first two colours.

## Lyrics

`LyricsPane(track:compact:)` (`LyricsPane.swift`) is the desktop pane ported (`ui.md`'s lyrics
entries, `mobile.md`'s *Lyrics*): the same `TURN`, `Glide`/`Leg`, falloffs, cascade, arrival,
breath dots, word sweep and *END OF LYRICS* mark, with the same constants, arithmetic drawn from
`lyric_frame`. `LyricStage` holds the motion outside observation (only `beat` is observed, bumped
where a paused timeline must wake) and turns each `TimelineView(.animation)` tick into a
`LyricScene`; the timeline runs while the pane is visible and a synced sheet plays or anything
still turns. The sheet's offset is a transform the stage owns, not a `ScrollView`: a vertical drag
moves it (rubber-banded past the ends, released into a `Leg` launched at the finger's pace), opens
the falloff out, offers *Follow*, and following returns `HANDS_OFF` after the last touch.
`LyricClock` steers wall time toward the snapshot's position projected from when it was heard (a
track change, pause or drift past 250 ms takes the position as is). Frames are asked on their own
serial queue (`LyricFrameSource`), never `Work.inOrder`, one in flight and the latest owed, and
land on the main actor through a closure built `nonisolated`; `Features.tick` no longer polls
them. Lines seek only while the pane's song is the one playing; Reduce Motion snaps glide, rise and
cascade but keeps the brightening. `compact` is the pane embedded in Now Playing: smaller words,
tighter gutters, no chrome; `LyricsView` adds the backdrop, the toolbar's refresh and the
`LyricAttribution` badge.

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build --ipa` must produce
`ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data race is
an error); with a
device attached, `xtool dev run` installs and launches it (the `run-ios` skill).
