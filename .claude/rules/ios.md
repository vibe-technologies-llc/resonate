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
  (`unfinished-download`, `unfinished-sync-landing`, deleted on the next start).
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
  in Resonate from elsewhere is copied into `local` and played.
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
the mini player in the tab bar's bottom accessory; Now Playing as a full-screen cover over a backdrop
of two tones sampled from 16 by 16 cover pixels; the output page holds `AVRoutePickerView`, since
iOS owns the route.

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build --ipa` must produce
`ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data race is
an error); with a
device attached, `xtool dev run` installs and launches it (the `run-ios` skill).
