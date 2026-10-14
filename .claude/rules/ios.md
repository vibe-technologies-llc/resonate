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
- **What moves every tick lives in `PlayClock`** (`library.clock`): position, duration, sleep time
  left, output rate and underruns. `snapshot` is assigned only when something else changed, and
  `currentId`/`currentQueueId` only when the song does, so a list that marks the playing row is not
  redrawn four times a second; only the views that show the position (mini player ring, Now Playing
  slider and times, sleep countdowns, the output page) read the clock, each in a view of its own.
  `snapshot.positionMs` is therefore the position when the snapshot last changed, not the live one.
- **Optimistic actions settle in order.** Play, pause, shuffle and repeat show at once (`Steered`)
  and queue edits change `queue` at once; a poll that began before such a command was answered
  keeps the steered values and leaves `queue` alone (`Settling` counts asked against answered).
  A favourite flips everywhere at once (catalog, browser, queue, snapshot, open playlist, top
  tracks, Inspector) and flips back with a notice if the write fails; hearts read
  `isFavourite(_:)`, the catalog's live answer by id, so a stale `Track` value never shows a stale
  heart. `Destination` carries song ids, never `Track` values, and `LiveSong` looks the song up
  when the page draws (`.lyrics(nil)` still means the playing song).
- **Status** is one `StatusMessage` (notice or failure, announced to VoiceOver). A notice fades after
  6 s; a failure stays until dismissed. The engine's `failure` is shown once per distinct text per
  song, and a poll failure once until polling recovers, so a dismissal holds. A scan shows its own
  live line ("Reading music folder… N songs") with Stop (`stopScanning`, `Folders.stopScanning`);
  its error is reported, never replaced by the tally, and removing a folder drops its queued scan.
- **`Features`** mirrors Android's `LibraryFeatures`: request generations refuse stale results, the
  same 30 s scrobbling loop, 2 s / 30 s sync polling, the sync-landing journal
  (`unfinished-sync-landing`, deleted on the next start). It owns `Enrichment` and `DownloadQueue`
  (below), each `@MainActor @Observable` and created lazily. Every busy flag
  is cleared in `work`'s `finally`, which runs on every path, a failed `library.player()` included;
  it clears only while its own generation is current. The Inspector's artist lookup
  (`inspectorArtist`, an `ArtistLookup`) is its own state, apart from the online artist page's,
  and survives the Inspector reappearing for the same song. Scrobbling accounts reach the
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
  the main actor). The category is set at start but the session is activated only on play
  (`activate()`), so opening Resonate never stops another app's audio. Now-playing info is written
  only when the song, transport, artwork, favourite, queue place or count, shuffle or repeat change,
  or the position drifts more than 1.5 s from what the written rate predicts (a seek); between
  writes the system extrapolates elapsed time from the rate. Every command answers
  `.noSuchContent` with nothing loaded; next is enabled only before the last row or while the
  queue repeats; shuffle, repeat (`.one` is the engine's 1, `.all` its 2) and stop are offered.
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

The views follow Android's screens and wording (`android.md`): tabs Songs, Library, Queue, Settings
and Search (`Tab(role: .search)`, which iOS sets apart at the trailing edge and turns into the search
field; `Root.search` has its own stack); the mini player in the tab bar's bottom accessory (`tabViewBottomAccessory(isEnabled:)` from 26.1,
so no empty capsule shows with nothing loaded), with previous/play/next and a horizontal swipe to
skip; its progress is a ring around the play button in either placement (a bar along the accessory's
bottom ran through the title and artist, since the system fixes the accessory's height); the output page holds `AVRoutePickerView`, since iOS owns the route.

- **Now Playing** is a full-screen cover zoomed out of the mini player's artwork
  (`matchedTransitionSource`, `.zoom`), dismissed by dragging down. Its slider commits one seek on
  release and holds the scrubbed value until the clock comes within 1.5 s of it or a second passes.
  Repeat cycles off, all, one. Volume is the system's (`MPVolumeView`, its route button hidden).
  The glass row toggles two modes inside the cover: Lyrics (`NowPlayingLyrics`, the compact
  `LyricsPane`) and Up next, each shrinking the artwork into a 56 pt header row through
  `matchedGeometryEffect`; sleep, equaliser, visualizer and output sit in the More menu. At
  accessibility text sizes the artwork layout scrolls.
- **The queue** splits History, Now playing and Up next around the playing row and scrolls to it.
  Rows reorder with `onMove` (SwiftUI's convention, which `move_in_queue` takes as is), swipe to
  remove by queue id, and Clear (confirmed) keeps the playing row. Play next and Add to queue are a
  song row's leading swipe and context-menu actions and a collection header's buttons.
- **Collections** are titled by their entity and drawn over a still backdrop, and grouped in Rust,
  never matched in Swift. `Destination.album` carries the album key: a Library or search hit passes
  `AlbumHit.key`, a song passes `Track.albumPage` (`album_key` of its album, artist and album artist,
  the index's own arithmetic, `nil` without an album, which hides *Go to album*). `AlbumView` reads
  `album(key)` and `album_tracks(key)`, `ArtistView` `albums_by(name)` (newest first, year captions)
  and `artist_tracks(name)` (folded, so *Bjork* and *Björk* are one page), each on `Work.aside` in a
  `.task` keyed by `Library.collectionGeneration`, which steps whenever the albums and artists
  reload, keeping the last answer meanwhile. A playlist's rows are keyed by song and occurrence, move and remove at once, and
  every edit reads the playlist's latest revision inside the ordered queue; its rows survive a
  page pushed over it and are dropped when it is left; the page pops itself when the playlist
  disappears.

## Search

- **The library is searched in Rust, never filtered in Swift.** `SearchModel` (`SearchModel.swift`;
  `Library.songSearch` for Songs, `Library.shelfSearch` for the Library tab, `Library.tabSearch` for
  the Search tab, so each tab's text is its own) asks `search_library`
  on `Work.aside` 150 ms after the last keystroke (the desktop's `SEARCH_SETTLE`), cancels the
  settle before, refuses a stale answer by generation and keeps the last answer on screen until the
  next lands; blank text asks nothing. Rows light matched text through `highlights`, its UTF-16
  ranges turned into `String` indices by `NSRange` and widened to whole characters (`lit`). A
  catalog change (`Library.refresh`) or a favourite asks again without the settle.
- **Songs** searches with scopes Top (five songs, six albums, six artists, each with *See all*),
  Songs, Albums and Artists; *Did you mean* where nothing matched; the grammar's `reads` as chips;
  `.searchSuggestions` offers completions while typing and, on an empty field, the last ten searches
  (`UserDefaults` suite `library-features`, key `recent-searches`, shared with the Search tab: a
  search kept merges into what is stored, and each tab re-reads it on appearing), kept when a search
  is submitted or one of its results opened. Searching spans the catalog: filters shape browsing
  only, and setting one clears the search. The page opens on the Songs/Albums/Artists picker; songs
  are filtered from the toolbar (`FilterButton`, filled while a filter is set, opening
  `FilterSheet`), and each part of the filter set shows under the picker as a small capsule token
  that clears it. Playlists and Suggestions are reached from the Library tab (Suggestions is a row
  beside Favourites and All songs), online search from the Search tab.
- **The Search tab** (`SearchTab.swift`) has scopes Your library (the default) and Online
  (`Navigation.searchPlace`), one field for both: typing sets `tabSearch.text` always and
  `Features.onlineQuery` only in Online, and choosing Online hands it the text, so a library search
  never reaches the network. Your library shows the Songs results (Top, *See all* pushing the full
  list, *Did you mean*, reads, completions) and, under five songs, "Search online for “…”", which
  switches scope. With no text the page lists the recent searches and a hint; Online with online
  features off shows why, with Open Settings. `Navigation.open(.online)` and Library tools' Online
  search switch to this tab in Online (`searchOnline`); `Destination.online` still draws a page of
  its own (`OnlineView`) over the same state.
- **Browsing** reads Rust too: songs through `filtered_tracks` pages of `Library.page`, the first
  page published at once where nothing is shown yet and the rest after; albums and artists through
  `albums(order)`/`artists(order)`, cached on `Library` and reloaded with the catalog. A list of 40
  rows or more ordered by name gets a section per letter run and the section index. `EmptyLibrary`
  shows only once the catalog was read empty and no scan runs; filters matching nothing offer Reset.
  The Library tab's search uses the same model for albums and artists and folds playlist names
  (`searchFolded`: case- and diacritic-insensitive), as the Favourites and Add-songs searches fold
  a song's title, artist and album.
- **Online** search asks as the listener types: 450 ms settle, three letters at least, one request
  generation (`Features.searchRequest`) and an in-memory answer per spelling, the last 64 per mode
  (`OnlineAnswers`). MusicBrainz is paced at about a request a second, so one spelling is a chain
  (`OnlineAsked.chain`): songs, then albums, then artists, one after another on `Work.aside`, each
  landing in `Features.onlineFound` as it arrives, a cached answer without asking; new text cancels
  the chain and its generation refuses whatever was still in flight. The Top view shows the best
  five songs (download, save, *In your library*), six albums and six artists, each with *See all*,
  and a progress row naming the section still awaited; the previous spelling's sections stay until
  the new one's first lands. Text starting `http://`, `https://` or `spotify:` is one request,
  opened as a link. Nothing is asked while online features are off; turning them on asks again.
  The query and the last answer live on `Features`, so they outlive the page. An artist, an
  album and a release are pages of their own (`OnlineArtistPage` with *More releases*,
  `OnlineAlbumPage`, `OnlineReleasePage`), each holding its own state; song rows are keyed by
  position, since a shared playlist repeats songs.

## The backdrop

`BackdropView(track:moving:)` (`Backdrop.swift`) is the flowing field behind Now Playing, the
lyrics, the visualizer and the album, artist and playlist pages. The collection pages pass
`moving: false`: one still frame, as under Reduce Motion, and no hold on the visualizer tap.

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
  wait. A job asks `find_download` with every account that is on and ready
  (`Features.downloadAccounts`, `ProviderAccounts.asked`), keeps a renewed TIDAL token on the TIDAL
  account, creates the destination with `Folders.fresh` on `Work.aside`, journals its URI, copies
  (`download_offer`), indexes (`index_download`), drops the journal entry, seeks the new song in the
  lookup and refreshes the library, the wants and the changed art. With no ready account nothing is
  begun and the downloads row offers "Set up a download provider" until one is ready; a job's row
  names the provider that delivered.
- **Every provider is configured at once, as on the desktop** (`ProviderView`): one section per
  `MusicProvider.every`, each with its switch, its own credentials and Save, and a status
  (`DownloadAccount.readiness`): Monochrome and hifi-api are always ready (a blank server is the
  hosted service); Subsonic needs a server, user and password (the desktop's three); TIDAL a client
  ID and refresh token. Every provider is on until switched off. **The boundary races them**, so
  `every` is the desktop's registration order (Subsonic, TIDAL, hifi-api, Monochrome) and accounts
  are passed in it: the desktop has no order of the listener's, so neither does the phone.
- **Accounts are one keychain item**: `download-accounts`, a JSON list of every provider's account
  (provider name, server, user, password, client ID, secret, refresh token); the switched-off
  providers are `download-providers-off` in `library-features`. The single account older builds
  kept (`provider` and `server` in the `provider-accounts` suite, `user`, `password`, `client-id`,
  `client-secret`, `refresh-token` in the keychain) is read once by `AccountStore.readProviders`
  while `download-accounts` is absent, written as the list, and only then deleted.
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

## The equaliser

- **The curve is drawn and shaped on a graph** (`EqualiserGraph.swift`, in `SoundSettingsView`):
  log frequency 20 Hz to 20 kHz across, decibels up, gridlines every 5 dB with 0 dB marked and the
  edges labelled, 100, 1k and 10k labelled among fainter decade lines. The span is ±15 dB widened in
  5 dB steps to hold the response and the handles, at most ±30 (a pass filter's floor is clipped),
  and frozen while a handle is held, as on the desktop. The response is `response_curve` at 256
  points and the stream's rate, asked by `EqualiserResponse` on `Work.aside`: the latest curve asked
  wins and asks are at least 30 ms apart while one is pending, so a drag is followed without
  queueing. It is washed to the zero line.
- **A handle per band**, numbered, lifted by the preamp (a gainless kind sits at the preamp). A
  one-finger drag beginning within 32 pt of a handle moves it, frequency across and gain up (tidied
  to 0.1, 1 or 10 Hz by decade and to 0.1 dB); a pinch turns the Q of the held band, else the
  selected one; a tap on empty space adds a peaking band there (Q 1) and switches the equaliser on.
  Grab and release are felt (`sensoryFeedback`). What a gesture moves is a draft drawn at once; the
  engine and the store hear the release, once, as the sliders did, and the draft gives way when the
  committed curve arrives, or a second after the release where none did, so a refused commit
  cannot linger.
- **The selected band's inspector** is a section below: type (all eight kinds), frequency, gain
  (gain kinds only) and Q, each typed (committed on return) or stepped (a semitone, 0.5 dB, a sixth
  of an octave of Q), and Delete. Then the preamp slider, *Add a band* (1 kHz) and *Reset*, which
  confirms either removing every band or starting from ten flat octave bands. VoiceOver reads each
  handle as an adjustable element, labelled with its number, kind and frequency, valued with its
  gain and Q: swiping steps the gain 0.5 dB (Q, for a gainless kind), activating selects it.
- **Headphone correction** (`AutoEqView`) searches AutoEq as the listener types (350 ms settle,
  `Features.correctionRequest` refusing a stale answer), shows the current preset, and loads a tapped
  headphone's correction as bands, confirming first where the curve has bands; EqualizerAPO or
  GraphicEQ text comes from the clipboard or a file (`fileImporter`, read on `Work.aside`, at most
  64 KiB and one byte, so the boundary's `TooLarge` speaks for a larger one). The notice after a
  load counts the bands and says what was fitted, passed over or held to the limits.
- **Stored as text**: `PlaybackPreferencesStore` keeps the curve as `equaliser_text` output under
  `equaliser-curve` (`eq.md`'s *The phones*), migrating the older keys once and writing the result
  back at once.

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build --ipa` must produce
`ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data race is
an error); with a
device attached, `xtool dev run` installs and launches it (the `run-ios` skill).
