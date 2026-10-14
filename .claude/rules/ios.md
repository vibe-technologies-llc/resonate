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
  same 30 s scrobbling loop, 2 s / 30 s sync polling, the sync-landing journal
  (`unfinished-sync-landing`, deleted on the next start). It owns `Enrichment` and `DownloadQueue`
  (below), each `@MainActor @Observable` and created lazily. Every busy flag
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
  `@Sendable` (`NowPlaying.artwork`, `Features.watchNetwork`, `BackgroundHold.expiry`);
  `MainActor.assumeIsolated` only where the framework delivers on the main thread (remote commands,
  notifications on `.main`, an expiration handler that finds itself on the main thread).
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

## Artwork and details

- **The background lookup starts itself** (`Enrichment.swift`): `Enrichment.begin` asks
  `start_enrichment` once the player opens (`Features.initialize`), after a scan
  (`Features.scanned`), after songs land from another device or a download, when online features
  turn on and when the network becomes unmetered. It is refused while "Fetch artwork and details
  automatically" is off (`enrich-automatically`, on by default) or, with "Only on Wi-Fi"
  (`enrich-unmetered`, off by default), while the path `NWPathMonitor` reports is expensive or
  constrained; a metered network then stops the pass. Both keys live in the `library-features`
  suite. Each start also seeks the playing song, and `Features.tick` hands every song change to
  `Enrichment.playing`, which calls `seek_enrichment` while a pass runs.
- **The status is polled every 2 s while a pass runs and the scene is active**: going inactive
  cancels the poll (the pass goes on in Rust), coming back reads the status once and resumes it.
  The line reads "Finding artwork and details · N of M", "Artwork and details up to date" or the
  `EnrichmentStop` in words; a finished pass refreshes the library once. Settings' *Artwork and
  details* group (shown while online) holds both toggles, "Look again for missing covers"
  (`ask_again_for_covers`, then a start), the line and Stop. Library tools' "Refresh artwork and
  details" confirms with the song count, stops any pass and starts one with `refresh`, and offers
  Stop while it runs. The Inspector's button is the one-song `enrich_track`; no lookup posts a
  banner per song.
- **Pictures refresh by the changed-art revision**: every status poll, scan, landing and
  single-song lookup asks `art_changed_since` from the last revision (the first answer only sets
  the baseline); the named songs alone are evicted from the cache and their entry in
  `Features.artGenerations` bumped, which `ArtworkView` and the backdrop key their `.task` on. A
  rise in `portraits` bumps `portraitGeneration` and forgets the kept portraits.
- **The cache decodes to the size drawn** (`ArtworkCache.swift`): `ArtworkView` measures itself
  (`onGeometryChange`), multiplies by the display scale and rounds up to a bucket of 96, 192, 384,
  768 or 1024 px; an entry is keyed by song (or portrait), art revision, generation and bucket.
  `CGImageSourceCreateThumbnailAtIndex` decodes on `Work.aside` with the bucket as its largest
  edge, at most 4 at once (`DecodeSlots`, an actor); requests for one key join one task, a view
  that disappears leaves its wait, and the last to leave cancels a decode still waiting for a
  slot. Pictures cost their bitmap bytes against 32 MiB (a 96 px row about 36 KiB); a song
  without a picture is remembered as missing until its art changes. A circular `ArtworkView`
  (`portraitOf:`, artist rows and headers) shows `artist_portrait` for the song's `artistMbid`
  first and falls back to the album's picture.

## Downloads

- **`DownloadQueue` runs jobs by the boundary's job number** (`DownloadQueue.swift`): `enqueue`
  begins one job per song not already queued (`begin_download`), two run at once and the rest
  wait. A job asks `find_download` with every ready account (the keychain keeps one today; a
  Subsonic account needs a server and user, TIDAL a client ID and refresh token), keeps a renewed
  TIDAL token, creates the destination with `Folders.fresh` on `Work.aside`, journals its URI,
  copies (`download_offer`), indexes (`index_download`), drops the journal entry, seeks the new
  song in the lookup and refreshes the library, the wants and the changed art. With no ready
  account nothing is begun and the downloads row offers "Set up a download provider".
- **Progress is polled every 250 ms while a job runs** (`download_progress`): a percentage where the
  provider gave a length, else the bytes received. Cancel calls `cancel_download` for that job
  alone; a failed or cancelled job deletes only the document it created and shows the boundary's
  sentence (`explained`) on its row, never in the banner, with "Try again"; a finished one offers
  Play and Go to album.
- **Saved for later queues into the same jobs**: "Download all missing" enqueues every want not
  held, each row has Download and Remove, a held row reads "In your library", and pulling to
  refresh reads the matches again. Online search's song rows enqueue one song.
- **The journal is a list**: `unfinished-download` holds the URIs of every job's document still
  being written, removed one by one as each lands or is deleted; on the next start each is deleted,
  and a single string left by an older build is read as a list of one.
- **The destination is remembered**: `download-folder` in `library-features`; the folder sheet
  appears only when none is kept or it is no longer readable and writable, with "Remember this
  folder" on by default; Downloads offers "Ask for a folder each time".
- **One background task is held while a job runs** (`BackgroundHold`, also the sync transfers'
  hold): its expiration handler, built `nonisolated`, cancels every running job and pauses the
  waiting ones until the scene is active again, so each cancelled job still deletes its document
  and its journal entry, or the next start does.

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build --ipa` must produce
`ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data race is
an error); with a
device attached, `xtool dev run` installs and launches it (the `run-ios` skill).
