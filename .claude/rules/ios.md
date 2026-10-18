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
`zip` for `--ipa`, `rsvg-convert` (librsvg) for the accent icons, the `darwin` Swift SDK
`xtool setup` builds from Xcode's `.xip`, the `aarch64-apple-ios` Rust target, clang and `llvm-ar`.
None of this is in CI: the SDK cannot be fetched without the listener's Apple ID.

## The native library

`ios/build-native.py [--profile debug|release]` builds `resonate-mobile` as a staticlib for
`aarch64-apple-ios` into `ios/.cargo-target` (`RUSTFLAGS` reset, so no `target-cpu=native`;
`IPHONEOS_DEPLOYMENT_TARGET` 26.0; C dependencies compiled by clang against the SDK's
`iPhoneOS.sdk`), generates the Swift bindings, and lays out `ios/Generated/`: `ResonateNativeFFI` (the
header and a module map that links `resonate_mobile`), `ResonateNative` (the bindings) and `lib/`.
It also reads the Resonate palette's seven accents out of `resonate-ui`'s `theme.rs` (the `RESONATE`
flavour) and `launcher.rs`'s `LIFTED_TOWARD_WHITE`, writes them as `ResonateAccent.every`
(`Generated/ResonateNative/Accents.swift`), and draws `packaging/resonate.svg` recoloured as the
desktop's launcher does (light stop the accent lifted toward white, deep stop the accent) into
`Generated/icons/Icon<Accent>{,@2x,@3x}.png` (1024, 120, 180 px) with `rsvg-convert`; `xtool.yml`'s
`resources` copies them to the bundle root and `Info.plist`'s `CFBundleIcons` names each as an
alternate icon beside the primary `Icon`. `Package.swift` takes those as a system-library target and a Swift target and links the archive
from `Generated/lib` with AudioToolbox, CoreAudio, Security and SystemConfiguration. Everything under
`ios/Generated`, `ios/.cargo-target` and `ios/.build` is generated, never checked in or edited.

## App Intents metadata

iOS finds an app's intents and App Shortcuts through `Metadata.appintents` in the bundle, which Xcode
writes with `appintentsmetadataprocessor`, an arm64 Mach-O that cannot run on the Linux host; xtool
has no such step. So the `Resonate` target is compiled with `-emit-const-values` and
`-const-gather-protocols-file Resonate/AppIntentsProtocols.json` (an absolute path, since the
compiler resolves it from its own working directory; both flags, since the first alone gathers
nothing), which leaves a `.swiftconstvalues` per source under
`.build/out/Intermediates.noindex/Resonate.build/<Config>-iphoneos/Resonate-t.build/`, and
`ios/app-intents.py [-c debug|release] [--ipa]` writes `xtool/Resonate.app/Metadata.appintents`
(`extract.actionsdata`, `version.json` naming tools version 27A266a) from them after
`xtool dev build`, and with `--ipa` packs `xtool/Resonate.ipa`. `xtool dev run` and
`xtool dev build --ipa` rebuild the bundle without it, so the app is installed with
`xtool install xtool/Resonate.app`. `ios/install.py [-c debug|release] [--udid]` is that whole run
in one: it fetches the latest xtool AppImage from the GitHub release (refused unless it matches the
release's sha256 digest; kept in `$XDG_CACHE_HOME/resonate` and fetched again only when the digest
moves; asked as `resonate/<version>`), checks xtool's sign-in and the `darwin` SDK, then builds the
native library, the app and the metadata in the configuration asked (release by default) and
installs. The generator writes what Apple's processor writes for the
same input, field for field, for what Resonate uses alone: `AppIntent`s with literal titles and
descriptions and `String` parameters (`requestValueDialog` leaves no trace), a `perform()` returning
a result with or without a dialog, `AudioPlaybackIntent` as the system protocol
`com.apple.link.systemProtocol.AudioStarting` (the identifier Xcode's `LinkMetadataDT` pairs with
it), and one `AppShortcutsProvider` whose phrases interpolate only `.applicationName`. Anything else
(an entity, an enum, a query, a parameter summary, a phrase with a parameter) stops it, since iOS
ignores metadata it cannot match to the binary; a new construct is added only against Apple's
output for it. Without Xcode's second step (`nlu.appintents`) Siri matches the phrases as written,
not paraphrases.

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
  **Nothing slow shares the transport's lane**: a pause (the headphones leaving included) waits
  behind only the tick and other transport and queue edits. Playlist edits and `documents_moved`
  run on their own ordered lane (`Work.inLibrary`), `sync_start`, `sync_network` and
  `sync_continue_from` on another (`Work.inSync`, since `sync_start` joins the sync thread), and
  the queue read, which costs a record per row, is `Work.aside`, as is applying the room
  correction (decoding up to 64 MiB; choosing, applying, forgetting and restoring one response run
  one after another in a chain of their own, `RoomCorrectionState.queue`). **No `AVAudioSession` call runs on the main actor**: each is
  a call into the audio server that can take tens of milliseconds. Activation is queued straight
  onto the transport's lane (`Work.enqueueInOrder`, so it precedes the `play()` steered after it), as
  are the category (at launch, after a media-services reset and for Listen, whose begin awaits it)
  and the activation Listen's end gives up,
  and the preferred rate, the route, rate and latency readings and the volume are read on their own
  serial lane (`Work.inSession`); the main actor only applies the answer.
  The catalog is read in a task of its own beside the poll, so the restored mini player and the lock
  screen's info and commands never wait on a large library.
  The poll reads the snapshot every 250 ms while the scene is active and
  playing (or loading, or analysing, or for 2 s after a command: `nudgePoll` wakes it at once),
  500 ms playing in the background (100 ms in the last 1.5 s of a playing song, so an automatic change is seen, and its rate asked, at once), 1 s paused in the foreground and 2 s paused in the background,
  and saves every 5 s, as Android does; the queue is read
  only when the snapshot's `queue_revision` moved since the queue last shown (`QueueListed`), or
  its `catalog_revision` moved and 3 s have passed since the last read (`catalogRereadAfter`), since
  every enrichment step, scan upsert, favourite and landing moves the catalog and a whole-library
  queue read four times a second kept the CPU busy and the main thread comparing thousands of rows;
  the rows read are compared with those shown on `Work.aside`, and an equal read assigns nothing.
  `queue()` copies the rows under the player lock and reads their documents afterwards in one
  catalog lock (`tracks_at`).
  State a poll reads (`syncStatus`, `syncHandoffs`, `transferring`, enrichment's status) is assigned
  only when it changed, so an unchanged answer redraws nothing.
- **What moves every tick lives in `PlayClock`** (`library.clock`): position, duration, sleep time
  left, output rate and underruns. `snapshot` is assigned only when something else changed, and
  `currentId`/`currentQueueId` only when the song does, so a list that marks the playing row is not
  redrawn four times a second; `transport`, `playingTrack` and `playingNext` are derived from each
  snapshot assigned (`Library.derive`) and stored only when they move, so a view that needs the
  playing song or the transport is not rebuilt by a pause, a shuffle or a sleep timer, and a list
  never reads `snapshot` in the body that builds its rows (a row marks itself by `currentId`): the
  Songs list doing so re-diffed the whole library on every play and pause, in the frame Now
  Playing's cover began its spring; only the views that show the position (Now Playing
  slider and times, sleep countdowns, the output page) read the clock, each in a view of its own (Settings' sleep row is `SleepTimerRow`, so the list and its search filter are not rebuilt each tick).
  `snapshot.positionMs` is therefore the position when the snapshot last changed, not the live one.
- **Optimistic actions settle in order.** Play, pause, shuffle and repeat show at once (`Steered`)
  and queue edits change `queue` at once; a poll that began before such a command was answered
  keeps the steered values and leaves `queue` alone (`Settling` counts asked against answered); the steered values outlive their command's answer and are dropped only by a poll that began after it, so an answer landing during that poll's queue read never publishes the older snapshot raw. Every steer wakes the poll (`nudgePoll`).
  A favourite flips everywhere at once (catalog, browser, queue, snapshot, open playlist, top
  tracks, Inspector) and flips back with a notice if the write fails; hearts read
  `isFavourite(_:)`, the catalog's live answer by id, so a stale `Track` value never shows a stale
  heart. A catalog read that began before a favourite write was answered is published with that
  write laid over it (`favouriteWrites`, dropped once a read begun after the answer exists), so a
  heart tapped during a long refresh does not snap back. `Destination` carries song ids, never `Track` values, and `LiveSong` looks the song up
  when the page draws (`.lyrics(nil)` still means the playing song).
- **A view body does no whole-catalog work.** `Library` keeps what views derive from `allTracks`:
  `catalogIndex` (id to place), `favouriteIds` and `favouriteTracks` (in catalog order), so a heart
  changes one row in place and inserts or removes one favourite (`markFavourite`, `keepFavourite`;
  a heart never rewrites `allTracks` or `tracks`, whose rows' own `favourite` may lag until the next
  read, since writing one element of either redrew the whole Songs list and Library tab)
  and the Library tab and Favourites read `favouriteTracks` instead of filtering; `playingQueueRow`
  is found when the queue or the playing row changes; the queue's sections iterate slices, never
  copies; `Features.foldedName(of:)` holds each playlist name folded; the add-songs sheet folds
  every song once off the main thread (showing a spinner, not the empty-library message, until it has) (`SongKeys`, keyed by `catalogEpoch`) and filters there 150 ms
  after the last keystroke, as Favourites does with its own songs (keyed by `favouriteEpoch`, so a heart refolds nothing while no text is typed); the tag editor asks `tags_writable` once per song when it opens.
  Adding, removing and measuring a music folder (security-scoped bookmarks, a File Provider that may
  hang), the keychain reads at launch (`Features.readSecrets`), a landing's `Folders.fresh` and `delete`,
  the journals' recovery at launch and the joining of an enrichment pass or a download on erasing run on `Work.aside`, never the main actor.
- **Status** is one `StatusMessage` (notice or failure, announced to VoiceOver). A notice fades after
  6 s; a failure stays until dismissed, and no notice replaces it. The engine's `failure` (the boundary's cause sentence, `mobile.md`'s *A failure is told by its cause*) is shown once per distinct text per
  song, and a poll failure once until that part of polling recovers, so a dismissal holds.
  **A catalog that would not open is asked again, never given up on**: the failure's banner offers
  *Try again* (`StatusMessage.offer`, `.openCatalog`, a button beside Dismiss), and coming forward
  after `.background` and every Siri intent ask too (`Library.openAgain`, which does nothing while
  the player is open, an open is under way or erasing runs); each opens a fresh `MobilePlayer`
  (`Library.open`) with `openingFailure` cleared first, so `player()` callers wait for that attempt,
  and a success runs the whole start (preferences, Now Playing, `Features.initialize`, the catalog,
  the poll). Actions sent while it is closed are dropped, as before, since the banner says why. The
  snapshot, the queue read and the save are three parts of a tick, each failing and recovering on
  its own: a queue or save that fails never discards the snapshot already taken (which holds the
  engine's failure), a queue that cannot be read is not asked again until its revision moves, and
  a failing save is told once however many good ticks lie between its tries. A scan shows its own
  live line ("Reading music folder… N songs", the count posted at most every 200 ms) with Stop (`stopScanning`, `Folders.stopScanning`);
  its error is reported, never replaced by the tally, and removing a folder drops its queued scan.
  Sync reports once as well: songs landing from other devices are counted and told in one notice
  when the transfers stop, a landing failure is told once per distinct text, and a failing sync
  poll once until it recovers.
  A pass that fails (`hide_songs_under`, `hide_missing_songs`) is reported, once per distinct text for the pass that repeats at every foregrounding. A song whose file has gone is hidden, never deleted (`hide_missing_songs`, given the readable
  folders' prefixes; `documents.missing`) at launch, whenever the scene comes forward again after
  `.background` (an `.inactive` blip of Control Centre, a permission prompt or a call banner opens
  nothing: each pass opens every document and measures every folder), after a
  completed scan and on *Sync now*: song lists, search, albums, artists, statistics, top songs, a
  playlist's rows and the wants' *In your library* skip it, and its rows leave the queue
  (`left_the_queue`; the playing row too, which moves on), while its playlist rows and favourite
  stay so no removal reaches the sync ledger as the listener's edit. A playlist edit by shown index
  is mapped to the stored row (`shown_positions`). Indexing the file again or finding it readable
  restores it; a document indexed under another name that names the same song
  (`Recording::names_the_same_song_as`: a song fetched back from a device or a download) takes the
  hidden row's place (`take_the_place_of_missing`): its playlist rows, favourite, listens, lyrics,
  artwork and studies move over, the playlists' revisions step, and the hidden row is deleted.
  Coming forward with songs newly hidden while sync runs forces a sync, so they are fetched back.
- **Sync shows what it is doing.** `SyncStatus` carries `settled` (the service's `Shown.settled`)
  and `transfers` (title, artist, the sending device's name, bytes and `SyncTransferState`).
  `Features.syncActivity` is *fetching* while a transfer waits or moves or a received song is being
  written into the folder (`syncLanding`), with a fraction where every size is known, else
  *syncing* from *Sync now* (`syncAskedAt`) until the status settles (a notice says with how many
  devices), the server is refused or unreachable with nobody nearby (the standing as a notice), or
  45 s pass; the poll runs every second meanwhile. Songs arriving refresh the library and the
  playlists in a task of their own (`adoptArrivals`), and received songs are copied in another
  (`landing`, awaited on erasing), so neither a long refresh nor a large song holds the poll, the
  progress, `syncHandoffs` or `answerSyncAsked`. `SyncProgress` is a glass card in the status
  banner that opens the sync page. `transferring` (the background hold, the 2 s poll) counts only
  received songs that can land now (`landable`: a writable folder and no back-off), so a song with
  nowhere to go never holds the app awake or keeps `cameForward` from rebuilding sync.
- **Erase all data** (the last Settings section, confirmed) first reads every song's URI
  (`song_uris`, hidden ones included): where that fails nothing is erased and the failure is shown.
  It then stops what could write afterwards (the poll, the scan, sync, scrobbling, Listen,
  enrichment, and downloads, which are cancelled and waited for so each deletes its half-written
  document and journal entry), reads the URIs again, shuts the player down, returns the alternate
  icon to the primary, deletes each of those files through `Folders.delete` (the songs in granted
  folders as well as the app's own; nothing else in those folders), then `Erasure.everything`
  empties Application Support, Caches, Documents and tmp, removes the `library-features`,
  `playback`, `provider-accounts` and `appearance` suites and the standard domain, and deletes
  every keychain class, then the app exits. `Erasure.everything` counts what stays (a file or folder it could not remove, a keychain class refusing with a status other than success or not found) and returns it as `ErasureLeftovers` beside the songs `Folders.delete` could not remove: with anything left the app does not exit, and the overlay names each kind and asks to close Resonate and erase again, since the next launch would resume from the catalog or sync key that stayed. With the pairing code gone, other devices hear nothing. The sync poll and the arrivals task are awaited, not only cancelled, so a landing in progress is in the second read; the run holds a background task, and `ErasingOverlay` on the root view blocks every tap and closes the open sheets. When `Folders.delete` could not remove some song (a read-only grant, an offline cloud folder) the bookmarks (`Folders.kept`) are not erased, the app does not exit, and the overlay says how many songs stayed and to close Resonate.
- **The sync page leads with what is happening; setup sits a step away** (`SyncView.swift`). The
  page is the *Sync this iPhone* switch, then a status section (`SyncStateRow`: spinner, *Up to
  date* check, warning or the network mark, a headline and `syncStanding`; *Sync now* turning into
  *Syncing…*; with online off and a server in use, *Sync only on this network* in its place), then
  *Devices* (this iPhone first, then the others online first, each with its platform and *On this
  network*, *Online* or *Away*; *Add a device…* opens `PairingSheet`), *Continue listening*
  (`HandoffRows`, keyed and asked by `HandedOff.id`), the active then ten latest finished
  transfers, and *Options*: *Songs* (`SyncSongsPage`: the three follow switches, mobile data, the
  folder picker and the landing notice) and *Connection* (`SyncConnectionPage`: *Sync through*
  Resonate's server, your own server with its address, or *This network only*, which is
  `SyncConfig.nearby_only`; and the device name). `PairingSheet` shows the code with Copy (on the pasteboard `localOnly` and expiring after two minutes, so Universal Clipboard never hands it on) and Share,
  joins another device's code (confirmed where a code is held), and *Make a new code…* (confirmed)
  is `Features.newSyncGroup`. The code is blurred and `privacySensitive` under *Tap to show* until
  tapped, covered again whenever the sheet opens or the scene leaves `.active` (so the App Switcher
  snapshot never shows it), and not selectable: Copy is the only way onto the pasteboard, which is `localOnly`. **A computer is paired by the system
  camera**: `Info.plist` registers the `resonate` URL scheme, so the Camera app or the Control
  Centre scanner opens the desktop's QR code (`resonate://pair/<code>`) in Resonate; `Library.open`
  hands it to `Features.offerPairing` (`sync_code_in_link`), and the root view asks *Join these
  devices?*, naming the code the link holds, before `joinSync` (which also turns sync on) and opens the sync page; a join confirmed before the keychain read has landed waits (`joinWaiting`) and runs when it does. Resonate itself
  never touches the camera, so it asks for no camera permission. No page names the hosted server's domain. A device that is away and
  not this iPhone swipes (or long-presses) to *Remove*; the section's menu removes every such one
  (`forgetSyncDevices`, `sync_forget_device` by id); rows are keyed by `SyncedDevice.id`, since a
  reinstalled phone keeps its name, and a transfer's row by the sending device, artist and title
  (`SyncTransfers.keyed`, a repeat numbered), so one landing never re-binds the rows below it to other songs.
  The Inspector's artist portrait is decoded once off the main thread, at the 768 px page bucket, when it arrives. Landing a received song backs off per song
  (`landingRefused`), never pausing the rest (`android.md`).
- **A sync setting is kept before sync restarts.** `saveSync` writes the keychain and defaults and
  assigns `syncConfig` on the main actor at once, then restarts on `Work.inSync`, so a second switch
  flipped straight after builds on the first; `applySync` puts a minted code into the *current*
  config only where its key is still the one that start used. `syncNetwork` runs on `Work.inSync`
  after the start with the network as it is then, and `syncForce` on `Work.aside`, so nothing on
  the main actor waits on `sync_start`'s lock. Sync runs while switched on and online, or with
  *This network only* whatever online says (`Features.syncRuns`).
- **`Features`** mirrors Android's `LibraryFeatures`: request generations refuse stale results, the
  same 30 s scrobbling loop (which also asks `tell_loves`, so a favourite is loved on each saved
  account and taken back there when removed; the Scrobbling page says so instead of a send button), 2 s / 30 s sync polling, the sync-landing journal
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
  record (sync off, or transfers not allowed) withdraws both. It is told the record as soon as sync
  starts and whenever the network turns unmetered (`announceNearby`), not only by the poll.
  `Info.plist` lists the type under `NSBonjourServices` (`sync.md`'s *Nearby*).
- **Coming back from the background rebuilds sync.** iOS may reclaim a suspended app's sockets, which
  left the QUIC endpoint deaf until a setting restarted sync. So when the scene returns to active
  after `.background` (`Features.wentToBackground`/`cameForward`) and no transfer held the app
  awake (a transfers hold whose expiration handler ran counts as lost, `transferHoldLapsed`),
  `NearbyDevices.restart` drops the listener and browser and `applySync` starts sync afresh
  (`sync_start` leaves the old one first); then missing songs are hidden, whether or not the start
  failed, and, where some were and sync started, sync is forced.
- **`Folders`** is the `Documents` callback. A chosen folder is a security-scoped bookmark kept in
  Application Support (`folders.plist`); the app's own Documents folder (`UIFileSharingEnabled`,
  `LSSupportsOpeningDocumentsInPlace`) is root `local`. A scan walks at most 32 levels, takes each file's size and modification time from the listing's resource values (`FileStamp`) and hands them to `scan_document`, so an unchanged file is not probed again (`android.md`'s *Documents and catalog*), admits
  `audio_extensions()`, reads a folder's cover/folder/front JPEG or PNG within 8 MiB once for the
  whole folder (`FolderCovers` gathers the songs sharing it and keeps it in one `keep_folder_cover`
  call, stored once) and a same-stem `.lrc` within 1 MiB. Fresh destinations are opened `O_CREAT | O_EXCL` and held out of every scan until their writer says the file is whole (`Folders.written`; `delete` says it too), so a scan never indexes a half-written download, landing or copy. A file opened
  in Resonate from elsewhere is played where it already is when its resolved path lies in a granted
  folder (`Folders.granted`, the most specific root), else played from an identical copy already in
  `local` (`localCopy`: same name or `name (n)`, equal contents), else copied into `local` and
  played; files arriving together (`onOpenURL` fires once each; 250 ms settles a batch) are indexed
  in turn and queued in order, the first playing as soon as it is indexed and the rest appended to the queue as each is indexed, none waiting for the copies after it (the catalog refreshes
  after, never before). A folder's access is measured, not
  assumed: `FolderAccess.of` reads `isReadableFile`/`isWritableFile` and the URL's
  `isReadable`/`isWritable` while the security scope is open, so a read-only grant is never offered
  as a destination for downloads or synced songs. A bookmark is resolved once per folder and the URL kept (`resolvedRoots`; each use opens and closes its own scope), so a missing-song pass or a scan pays no `URL(resolvingBookmarkData:)` per song; measuring a folder (`refreshed()`, a scan) resolves it again, which is how a moved or stale folder is noticed. Access is cached per folder: `folders()` resolves
  only a bookmark it has not measured, `refreshed()` re-measures every one on `Work.aside` (the
  folder and sync pages call it, and so does every hiding of missing songs, since a drive unplugged
  while the app was away still reads as readable from the cache), and a scan re-measures its own root. A folder removed hides its songs at once
  (`hide_songs_under` its prefix, the queue rows with them), though its files are still there, and
  its path is remembered (`forgotten-folders.plist`, path to root id) so granting it again gets the
  same root id: a rescan indexes the same URIs and restores the hidden rows with their favourites,
  playlist rows and listens, instead of minting a second copy of every song. A folder whose resolved,
  symlink-free path is already granted, or is the app's Documents, is refused
  (`FolderFailure.alreadyAdded`), and so is one that holds or sits inside a granted folder or Documents (`FolderFailure.overlapping`), since a song in the overlap would be indexed under two root ids and split its plays and favourite. A scan runs on a GCD queue where `Task` cancellation cannot reach,
  so it walks under a `ScanStop` flag; removing a folder stops its walk. A root it cannot list fails the scan (`FolderFailure.unreadable`); a subfolder it cannot list is counted and named in the tally, and the missing-song pass after a scan runs only when the scan failed nowhere and read everything, so an offline File Provider folder never hides its songs as missing. The scan's notice
  gives its reasons: the first unreadable song and why, and the covers and lyrics sheets that
  could not be kept, with the first of those.
- **The folders are read again on their own** (`Library.rescanUnasked`): every granted folder and
  Documents once the catalog has been read at a launch in the foreground and whenever the scene comes
  forward after `.background`, and Documents alone 2 s after its listing last changed while the scene
  is active (`DocumentsWatch`, a vnode `.write` source on the folder, which a song dropped in over
  Files or Finder moves; Resonate's own writes there move it too and cost a walk of stamps). The
  scans join the listener's chain (`QueuedScan`, `asked: false`), so organise and the scans before
  them keep their order; one runs at a time, a folder already queued is not queued again, and asks
  arriving meanwhile coalesce into one more pass (`owedRescan`, `RescanScope`). Such a scan compares
  each answer with the catalog shown (`KnownDocuments`, built on `Work.aside`): an unchanged song
  keeps no cover or lyrics sheet again and counts for nothing, so a pass that finds nothing new shows
  no line, reads no catalog and tells nothing; the live line appears with the first new or changed
  song and a notice counts them (*Found N new or changed songs*). It never runs the missing-song pass,
  which coming forward already runs; a failing root is told once per distinct text. A listener's scan
  asked for a folder an unasked one holds turns that one into the listener's (queued), or queues
  behind it (running), so a rescan always keeps covers and sheets afresh.
- **`NowPlaying`** owns the audio session (`.playback`, long-form), off the main actor as above
  (a route report asked while another is reading is the one applied, carrying any rebind owed), reports the route to Rust,
  pauses on an interruption (resuming when the system says so, whether it began while playing or loading, as `Library.pause` pauses both) and when the old route disappears,
  and drives the remote commands and now-playing info (artwork bounded at 384 px, rendered at scale 1 off
  the main actor). The category is set at start but the session is activated only on play
  (`activate()`), so opening Resonate never stops another app's audio; it is deactivated with
  `.notifyOthersOnDeactivation` when the transport turns stopped (Stop, the queue's end) or a sleep
  timer paused it (`releaseSessionIfDone`, on the transport's lane and only if nothing plays by
  then), so the app audio it interrupted may resume. **The session is asked for
  the playing song's rate** (`preferRate`): on every song change and activation (a `play` or `jump` names the song it loads, `activate(loading:)`, not the one on the lock screen), while "Match the
  song's sample rate" is on (the output page; `follow-song-rate` in the `playback` suite, on by
  default), `setPreferredSampleRate(track.rate)`, `0` (no preference) while off, then the route is
  reported again, so a rate the route took moves `route_changed` and reopens RemoteIO at it; a route
  that keeps its own rate (the speaker, Bluetooth) changes nothing. A rate-only change posts no
  route notification, so the rate is read again 0.25, 1 and 3 s after asking and reported where it
  moved from the one last reported; the ask is once per song (`ratedTrack`), so a favourite flip or
  an artwork arrival rewrites the now-playing info without touching the session. A refused
  activation is shown at once, not left to surface as RemoteIO's `OSStatus`. Now-playing info is written
  only when the song, transport, artwork, favourite, queue place or count, shuffle or repeat change,
  or the position drifts more than 1.5 s from what the written rate predicts (a seek); between
  writes the system extrapolates elapsed time from the rate. Every command answers
  `.noSuchContent` with nothing loaded; next is enabled only before the last row or while the
  queue repeats; shuffle, repeat (`.one` is the engine's 1, `.all` its 2) and stop are offered.
- **The Processing page's *Player volume* is the engine's own trim, kept by `Library.setVolume` (`player-volume` in the `playback` suite) and sent again when the player opens; the slider reads `Library.volume`, which moves with the finger, not the polled snapshot.
- **Reduce Motion is honoured by every animation, not by the three motion-heavy views alone.** An
  implicit animation goes through `.motion(_:calm:value:)` (`Appearance.swift`: none, or the calm
  fade given, under Reduce Motion), an explicit one is `withAnimation(reduceMotion ? nil : …)`, a
  symbol effect is inactive and a content transition `.identity`, and a banner arrives by fading
  rather than moving. Now Playing's mode change under Reduce Motion swaps at once, with no spring and
  no settle wait.
- **An icon-only control is at least 44 pt** (`touchTarget()`: a minimum frame and a rectangle
  content shape, put inside a control's label so the hit area grows with it: the favourite heart, a *See all* link, a header's menu and the recent searches' *Clear*; a folder row's three actions are large bordered buttons that stack where they do not fit; the mini player's skips and play button and the transport row's skips, shuffle and
  repeat (the active mode also carries a dot, so colour is never the only sign), a filter token, the online song menu, *See all*). A slider speaks its value in its unit
  (`CommittingSlider.spoken`: decibels, milliseconds), not a percentage. Now Playing's secondary labels (artist, album, times, the quality line, the sleep countdown, the volume glyphs and the Up next list's headings and detail lines, the last through `secondaryInk()`, which reads the `liftedField` environment the player's `QueueList` sets) take `nowPlayingInk`, primary at 95 % and solid under Increase Contrast or Reduce Transparency, since the field behind them is lifted by the shader. A long status message opens in full on a tap and always at accessibility text sizes. What is lettered on the
  accent (Play, the active mode toggle, the swatch tick, a chosen handle) takes `Color.wornInk`,
  white or near-black, whichever contrasts more with the accent as it is drawn in the current
  appearance (`AppearanceState.ink(on:)`).
- **A closure a framework may call off the main thread is never formed on the main actor.** Swift 6
  makes a non-`@Sendable` closure written inside a `@MainActor` type main-isolated and asserts the
  queue when it runs: `MPMediaItemArtwork`'s image handler, called on a MediaPlayer queue, trapped in
  `_dispatch_assert_queue_fail`. Such a closure is built in a `nonisolated` function or marked
  `@Sendable` (`NowPlaying.artwork`, `Features.watchNetwork`, `BackgroundHold.expiry`, and the dynamic
  `UIColor`s of `AccentColours`, resolved by SwiftUI's render thread: formed inside the main-actor
  `AppearanceState` they trapped in the queue list);
  `MainActor.assumeIsolated` only where the framework delivers on the main thread (remote commands,
  notifications on `.main`, an expiration handler that finds itself on the main thread).
- Secrets (provider credentials, scrobbling, AcoustID key, sync key) live in the keychain,
  `AfterFirstUnlockThisDeviceOnly`. `Keychain.read` answers blank only for an item that is not
  there and throws on any other status (a launch before first unlock included), so a failed read
  never becomes a blank sync key that mints a new pairing code over the real one: `Features`
  keeps what it could not read unset, says so once, and reads again when the scene next becomes
  active. `write` updates an item in place and adds it only where none exists, so a refused write
  keeps the old secret. Every write runs on `Work.inSecrets`, a serial lane, never the main actor: a sync setting and a provider account are shown at once and put back if the write is refused, while a scrobbling account, the AcoustID key and the AudD token show only once written. Plain choices in `UserDefaults` suites named as Android's
  `SharedPreferences` files, with the same keys.

- **What can be rebuilt stays out of backups** (`Backup.leaveOutDerived`, at launch and again once the
  player is open): `sync-received` and `sync-staging` (songs other devices send, fetched again) and
  the catalog copy kept before a migration (`.before-v<N>`) are `isExcludedFromBackup`; the catalog
  itself (favourites, playlists, listens) travels with the device's backup.

The views follow Android's screens and wording (`android.md`): tabs Songs, Library, Queue, Settings
and Search (`Tab(role: .search)`, which iOS sets apart at the trailing edge and turns into the search
field; `Root.search` has its own stack); the mini player in the tab bar's bottom accessory (`tabViewBottomAccessory(isEnabled:)` from 26.1, enabled by `Library.loaded`, stored and flipped only when something is or stops being loaded so a song change never rebuilds the root view,
so no empty capsule shows with nothing loaded), with previous/play/next and a horizontal swipe to
skip; it shows no progress (a bar along the accessory's bottom ran through the title and artist,
since the system fixes the accessory's height, and a ring around the play button was dropped) and no
heart (the song's heart is Now Playing's). Opening Now Playing the first time in a launch built an
`AVRoutePickerView` and, 600 ms in, an `MPVolumeView`, each the first of its framework, so
`PlayerWarmup` builds one of each 2 s after launch and drops them, and the first opening costs no
more than the next. Inline (the
tab bar minimised on scroll) it measures itself and keeps what fits: a 28 pt cover and the play
button always, the title from 150 pt, next from 200 pt (a minimised capsule beside the search
button is about 220 pt, and without next the title trailed into empty glass), tighter spacing and a
12 pt inset each side, so the square cover clears the capsule's rounded end; the title stays the
accessibility label. Expanded, the artist line is `Color.primary` at 60 % opacity, never the
hierarchical `.secondary`: that is drawn vibrant against the accessory's glass, which is not drawn
while Now Playing zooms back into it, so the artist showed only once the zoom ended. The output page holds `AVRoutePickerView`, since iOS owns the route, and draws the session's route,
hardware rate and latency from `Library.routeReading`, which `NowPlaying.reportRoute` reads again on
every route change (a pair of headphones at the same rate included), not from the session in its body.

- **Now Playing** is a full-screen cover zoomed out of the mini player's artwork
  (`matchedTransitionSource`, `.zoom`), dismissed by dragging down. Its slider (like the volume row's) commits one seek on
  release, clears a drag the system cancelled through a `@GestureState` flag, and holds the scrubbed value until the clock comes within 1.5 s of it or a second passes.
  Repeat cycles off, all, one. Volume is the system's, drawn as the scrubber is (`VolumeRow`: a
  capsule between the speaker icons in one `HStack`, so nothing can sit off their line): an
  `MPVolumeView` lays its slider out late and again on the first touch, which left it off centre
  until touched. `SystemVolumeLevel` reads `AVAudioSession.outputVolume` (KVO registered and the first read made on `Work.inSession`, the level assigned back on the main actor, a registration that finishes after `stopWatching` invalidated at once; it reads 0 until the session is active, so the driver's slider seeds the level once it has laid out, but only where the session gave no level above 0: a freshly made `MPVolumeView`'s slider holds a stale value for a while, and seeding from it over the session's answer slid the bar from 50 to 40 each time the cover layout came back; a row opens on the last level shown, `lastShown`, not 0)
  and sets the level through the slider of an `MPVolumeView` kept at 1 % opacity behind the capsule
  (`VolumeDriver`, its route button hidden), whose presence also keeps the system's volume HUD away.
  The driver is mounted 600 ms after the row appears (or at the first touch), so building an
  `MPVolumeView` never lands in the zoom's first frames, and `SystemVolumeLevel` reads the session's
  volume only in `watch()`: a `@State` initial value is built on every parent body, and
  `outputVolume` is a call into the audio server.
  The glass row toggles two modes inside the cover: Lyrics (the compact `LyricsPane`) and Up next,
  each shrinking the artwork into a 56 pt header row. **Both are one layout** (`NowPlayingView.layout`):
  only the titles, the pane, the sleep countdown and the volume row come and go, while the scrubber,
  transport and glass mode row keep their identity and move, so no glass control crossfades with a
  copy of itself. **The cover is one view moved by a transform**:
  each layout holds an empty slot (`LargeArtwork`, `CompactTitles`) reporting its bounds through
  `CoverSlot` (an anchor preference keyed by `CoverPlace`, so the slot followed is the one the mode
  names; while one layout fades out beside the other both report, and whichever came last in the
  tree won, which left the cover in the header until the fade ended and then jumped it back
  unanimated), and `MovingCover`, an overlay above both, draws the artwork once (its shadow is
  `CoverShadow`, a shadow-only `Canvas` rasterised once at scale 1 behind it, never `.shadow` on the
  artwork, which is blurred again on every frame the field behind it moves)
  at the large slot's edge and reaches the slot in force by `scaleEffect` and `offset`, so the spring
  re-lays out and re-measures nothing (two `ArtworkView`s under `matchedGeometryEffect` resized and
  re-measured a live image every frame). The pane or
  list fades in over 0.2 s as the cover starts to shrink (`NowPlayingView.settled`), not once the
  spring has landed, and fades out in 0.12 s as the cover starts to grow back; both are cheap to
  build in the spring's first frame (the lyrics are layers, the queue a window); Lyrics to Up next crossfades at once, the cover staying put.
  **A mode change asks 120 Hz for its 0.8 s** (`FrameRateBoost`, a `CADisplayLink` that lets go once
  the time is up), and the field rests on the frame it shows while the cover moves
  (`BackdropView(resting:)`, `.dormant`, so the renderer's capped step carries on without a jump):
  left to the system, the spring ran at its default rate, and the field redrawing under the moving
  glass took the frames it needed. Sleep,
  equaliser, visualizer and output sit in the More menu; no tab's toolbar carries the sleep timer
  (Settings has its row). At accessibility text sizes the artwork layout scrolls.
- **The queue** (the tab and the player's Up next alike, `QueueList`) opens on Now playing at the top
  with History above it, reached by scrolling up and drawn faded, then *Playing next* (the rows the
  engine's `Queued::next` holds, `Snapshot.next_first`/`next_count`) and *Continue playing* (*Up
  next* where nothing was asked to play next), as the desktop's four parts (`QueueParts`), each headed by a row of its own
  (`QueueHeading`), never a `Section` header, which a plain list pins under a square blur that
  banded the player's list as it scrolled; it scrolls back to the playing row whenever the song
  changes. **The player's list is a window around the playing row** (`QueueParts`' `heardAtMost` 50
  and `aheadAtMost` 200), with a row naming the earlier and the further songs that opens the Queue
  tab, which lists them all: a whole library queued from Songs is thousands of rows, and a `List`
  holding them all set every one up and scrolled past the whole history to the playing row on the
  main thread, so the pane came in a beat after the cover landed (and, built earlier, swallowed
  taps). In the player the list takes the full width
  (the player's gutter undone), since its rows carry their own insets;
  empty, the tab says so (`EmptyQueue`, with *Browse songs*) and hides Clear and Edit.
  Rows reorder with `onMove` (SwiftUI's convention, which `move_in_queue` takes as is), swipe to
  remove by queue id, and Clear (confirmed) keeps the playing row. Play next and Add to queue are a
  song row's leading swipe and context-menu actions, and for a whole collection its toolbar's
  `CollectionMenu` (`CollectionQueueActions`: Play next, Add to queue, Add to playlist; inside the
  playlist's own options menu on a playlist).
- **A collection header** (`CollectionHeader`) is the artwork, the title, the subtitle (an album's
  artist a tinted link to the artist page), the summary, an optional quality capsule (an album whose
  every track shares one `Track.quality`), then Play and Shuffle as two equal-width glass
  capsules no wider than 440 pt (`CollectionAction`: a plain button under `glassEffect`, Play tinted
  with the accent and lettered white, Shuffle clear and lettered in the tint, 50 pt scaled); nothing
  else sits in that row. In a list row a `Label`'s icon takes the list's accent tint over any
  button style, so `.glassProminent` drew Play's triangle blue on its blue fill: the capsule draws
  its symbol and title as separate views, each given its colour, never a `Label`. The artwork's shadow is `HeaderShadow`, a shadow-only `Canvas` drawn once at scale 1 in the cover's outline, never `.shadow`, which was blurred again on every scrolled frame; a playlist's mosaic picks are kept per playlist and revision (`PlaylistCovers`). An album lists its tracks by disc,
  a *Disc N* header only where there is more than one, each row numbered with its length at the
  trailing edge and a second line only for a track artist other than the album's.
- **A failed read is never drawn as empty.** The album and artist pages, the online artist, album
  and release pages and the tag editor show the read's failure with *Try again* (`ReadFailure`)
  instead of "No songs…" or a blank editor (whose Save would overwrite the file's tags); a
  collection's context-menu action or a mix's Play reports a thrown read rather than saying
  "Nothing in X yet"; *Save all for later* stops at the first chunk that fails and says how many
  were saved.
- **Every album, artist and playlist row acts on the whole collection** (`collectionActions`, a
  `CollectionSource`): a context menu with Play, Shuffle, Play next, Add to queue, Add to playlist
  and an album's Go to artist, and the song row's leading Play next / Add to queue swipe, its songs
  read on `Work.aside` when the action is chosen (`album_tracks`, `artist_tracks`, every page
  of the playlist through `Library.everyPage`, as its page reads them). Lists, search results, shelves and the artist page's albums all carry it.
- **One row per kind**: a collection is a `CollectionRow` (lit title, optional chevron) and a song a
  `TrackRow` (lit by `query` in search results, `showsAlbum` on an artist page) wherever listed, so a
  search result swipes, marks the playing song and is felt as a list row is; only its `PlayableTrack` (the title's weight and colour, the playing mark) reads `currentId`, so a song change redraws that part of each row and not its lit subtitle; an album page folds its artist once (`AlbumCredit`); every artwork edge is a
  `@ScaledMetric`.
- **One song menu everywhere a song is listed** (`SongPages`: *Song details*, *Share link*, *Edit
  tags* where writable, *Lyrics*, *Analyse*, *Go to album*, *Go to artist*): `TrackMenu` carries it
  under its queue, favourite and playlist sections, a queue row (`QueueRow`) under its favourite and
  playlist entries with *Remove from queue* last (no Play next or Add to queue, which would only copy
  the row), a playlist row (`PlaylistRow`) under its own queue, favourite and *Add to another
  playlist* entries with *Move up*, *Move down* and *Remove from playlist* last, and Now Playing's
  `SongDestinations` without *Lyrics*, which the player's own mode shows. The Inspector's actions
  add *Edit tags*, *Go to album* and *Go to artist*. **The tag editor opens after Now Playing
  closes** (`Navigation.editTags`): asked inside the cover, it is held (`taggingAfterPlayer`), the
  cover dismissed, and the root's sheet presented from the cover's `onDismiss`
  (`Navigation.playerClosed`), since the root cannot present over the full-screen cover.
- **A finished download refreshes the library once for a run of them** (`DownloadQueue.landed`
  sets `landedSince` and starts one `settleLandings` task, which loops until no song landed during
  its refresh), so *Download all missing* of N songs reads the catalog and rebuilds the search index
  a few times, not N, as `landReceived` does once a batch.
- **A destination already on top of a stack is not pushed again** (`Navigation.open` switches the tab and returns), so *Go to album* from Now Playing on that album or *More › Equaliser* twice never leaves a copy for Back to land on.
- **The tab and every stack outlive the process** (`NavigationKeeper`, behind the `TabView`): the
  selected tab and each tab's path are `KeptNavigation` (`Root` and `Destination` are `Codable`),
  JSON in the scene's `@SceneStorage("navigation")`, written on every change once restored. They are
  restored on the root's first appearance, so the page shows from the first frame, and then checked
  once the player is open and the whole catalog read (`Library.wholeCatalog`): a song page whose id
  the catalog no longer holds, an album `album(key)` no longer finds, an artist with no songs or a
  playlist gone (`Library.gone`, on `Work.aside`) cuts its stack at that page, since every page above
  it was reached through it. A failed read cuts nothing. Now Playing and the sheets are not kept.
- **The Library tab's *All songs*** clears any filter or mix, sets the Songs tab to songs (it keeps
  whatever grouping it last showed otherwise) and pops its stack before switching to it.
- **The Library tab's All** shows albums and artists as the Search tab's carousels (`AlbumShelf`,
  `ArtistShelf`, twelve each) under a *See all* header that switches the picker to that shelf, where
  the full list is rows.
- **Collections** are titled by their entity and drawn over a still backdrop, and grouped in Rust,
  never matched in Swift. `Destination.album` carries the album key: a Library or search hit passes
  `AlbumHit.key`, a song passes `Track.albumPage` (`album_key` of its album, artist and album artist,
  the index's own arithmetic, `nil` without an album, which hides *Go to album*). `AlbumView` reads
  `album(key)` and `album_tracks(key)`, `ArtistView` `albums_by(name)` (newest first, year captions),
  `artist_tracks(name)` (folded, so *Bjork* and *Björk* are one page) and `most_played_by(name, 5)`,
  shown as *Top songs* (each row naming its album) until *See all* lists every song, each on `Work.aside` in a
  `.task` keyed by `Library.collectionGeneration`, which steps whenever the albums and artists
  reload, and `catalogEpoch`, which steps whenever a catalog read lands that differs from the one shown (the
  comparison, the index, the favourites and the letter runs are worked out on `Work.aside`, `CatalogChanges`,
  so an unchanged refresh after a download, landing or enrichment pass redraws nothing), keeping the last answer
  meanwhile. The key or name is state: when an album or artist reads empty, the page looks up where
  a song it showed went (`album_key` of that song's fresh row; the artist page asks `artist_tracks`
  for the song's artist and album artist) and follows it, or pops where the album is gone, so
  *Edit album tags* leaves the page on the album, not on "No songs from this album". A playlist's rows are keyed by song and occurrence, move and remove at once, and
  every edit reads the playlist's latest revision inside the library's ordered lane; a refused edit, a scan, and the hiding or restoring of missing songs or a folder's songs reload the open page, and a reload lands only if it is the latest asked (`Features.playlistRequest`); its rows survive a
  page pushed over it and are dropped when it is left; the page pops itself when the playlist
  disappears. A playlist's count is the rows its page shows (hidden songs are not counted), so a
  playlist of hidden songs reads as empty and offers *Your playlist starts here*. The playlist
  picker stays open until the edit has answered: it says "Added to X" and closes on success, and
  on a failure (a duplicate name in *Create and add*) shows it under the field; Now Playing's picker
  is made for the song playing when *Add to playlist* was chosen, not the one playing when a row is
  tapped.

## Playlist files and listening

- **A playlist file is read, never kept**: M3U, M3U8, PLS and XSPF
  (`playlist_file_extensions()`, declared in `Info.plist` as `org.resonate.playlist`) opened in
  Resonate or chosen from the playlists section's *Import playlist file* go to
  `import_playlist_file` (`android.md`'s *playlist file*), read at most one byte past the boundary's
  8 MiB so a huge pick is refused rather than loaded, a notice counts the songs found
  in the library, and `Features.playlistToShow` has the root view open the new playlist. A playlist's
  options menu exports M3U8, PLS or XSPF through `export_playlist` into a `fileExporter`
  (`PlaylistFile`).
- **The queue is saved as a playlist** from the Queue tab's toolbar (`createPlaylist` with every
  queue row, repeats kept).
- **Statistics is a page of its own** (`StatisticsView.swift`), a row beside Favourites, All songs and
  Suggestions on the Library tab as well as in Library tools. Under the period picker (the tiles and the chart take the period the loaded data was read for, `Features.statisticsDays`, so a pick that is loading or failed never divides one period's total by another's span): tiles
  (`StatisticsTiles`: time listened and plays, the daily average over the period or, for all time,
  since the first counted play, songs heard of the library, the busiest day, the library's size, albums
  and artists); a bar chart of listening (`ListeningChart`, drawn in SwiftUI: days for a week or month,
  weeks for a year, months for all time, the day cut at the midnight of the phone's zone, `TimeZone.current.identifier` with its offset as the fallback; all time reads every day
  heard, not the last year's, and a year's or all time's end labels and busiest day carry the year) that a tap or a drag
  selects a bar of to read its time and plays; most listened songs, albums and artists, five each
  and *Show more* for twenty-five (albums open `Destination.album(key)`, artists
  `Destination.artist(name)`, each a `CollectionRow` with the collection's menu and swipe, its cover
  found with the statistics on `Work.aside`, `ListenedCovers`: an album's `album(key)` artwork song,
  an artist's most played song); and the library in shares (`library_breakdown`: formats, sample
  rates, genres, decades, as bars of the library's songs, independent of the period). With no
  counted play the listening parts give way to *No plays yet* and the library shares remain.
- **Recently played** is a Suggestions mix and a *Last played* sort (`TrackOrder.played`).

## Sharing

- **A song is shared as a link, never a file** (`Features.share`, `Sharing.swift`): *Share link* in a
  song's menu (`TrackMenu`) and in Now Playing's song menu (`SongDestinations`) asks `share_link(id)` on
  `Work.aside`, then `Features.sharing` presents the system activity sheet (`ActivitySheet`, medium and
  large) from the root view with the URL alone. The latest ask wins (`shareRequest`); a song with no
  link says so in a notice. The link is the library's (`library.md`'s *A share is the link alone*),
  built from the row's recording id and ISRC; online asks the reference where the song streams only
  while it is on, else the MusicBrainz recording link is the answer.

## Tags

- **A song's or an album's tags are edited in a sheet** (`TagEditor.swift`, `Navigation.tagging`):
  *Edit tags* in a song's menu where `tags_writable` says its kind can be written and its folder (`MusicFolder.writable`) is, *Edit album tags*
  in an album page's menu. One song shows all eleven fields; several hide title and track number,
  show a field the songs share with its value and one they differ in as *Mixed* (changed only once
  typed into), and read at most 500 songs. *Save* sends only the fields that moved. `Features.editTags`
  resolves every URI to its path inside `Folders.withPaths`, which holds each folder's security
  scope open for the whole write, and runs `edit_tags` on `Work.aside`; a notice counts what was
  saved, a failure is reported with how many were left. Library tools offers *Undo tag edit* while
  `tags_to_undo` names a step (`Features.undoTags`, through the same paths).

- **A writable folder is organised from its row in Music folders** (`OrganiseView.swift`): a layout
  field (`organise-layout` in `library-features`, the desktop's `DEFAULT_LAYOUT` until changed)
  checked by `organise_layout_problem`, a preview 300 ms after the last keystroke (`plan_organise`
  on `Work.aside`: counts and the first fifty moves; a plan stands only for the layout it was made for, so
  *Organise* waits for the preview of what is typed and never runs, or counts, the previous
  layout's), and *Organise*, confirmed, which waits for any scan running or queued before it and holds the scans asked after it (`Library.afterScans`, the scans' own chain), so the new URIs are never indexed beside the old rows. `Folders.file` moves
  inside the root's security scope: a destination that exists, or a source outside the root, is
  counted failed; a same-stem `.lrc` moves along where its place is free; folders the moves left
  empty (a `.DS_Store` aside) are removed up to the root. The new URIs are `uri(root:relative:)`'s,
  the scan's own spelling, so a rescan finds the same rows; `documents_moved` runs on `Work.inOrder`.
  The moves back are kept for the session: Library tools' *Put files back* files them again the
  other way.

## Appearance

- **The accent is the desktop's, chosen on Settings › Appearance › Accent colour** (`Appearance.swift`):
  *Resonate* (the default: the system accent and the packaged icon) or one of `ResonateAccent.every`,
  kept as `accent` in the `appearance` suite. `AppearanceState.shared` (`@Observable`) is read by
  `.tint` at the root and the Now Playing cover and by `Color.worn`, which every view drawing the
  accent itself uses instead of `Color.accentColor` (that reads the asset catalog, never a tint set
  in code). An accent is drawn as its hex in dark mode and 28 % deeper in light, since the desktop's
  hexes are lit for dark grounds. Choosing one sets the alternate icon of its name
  (`setAlternateIconName`, `nil` for Resonate); the requests are chained, each asking for the accent
  held when its turn comes (the system refuses a second while one is pending), and the icon is
  reconciled with the kept accent at launch; a refusal is said under the swatches.

## Suggestions and generated art

- **A mix's art is drawn, never stored, as the desktop's** (`ui.md`'s mosaic): `MosaicArtwork`
  (`Mosaic.swift`) draws no cover as a 135° gradient between two `MixAccent`s with the mark (an SF
  Symbol, 22 % of the side) and, from 100 pt, the name lettered (bold, 11 %, at most three lines) in
  the first accent's ink; one cover whole; two to four as a 2×2, a missing tile filled with the
  accents in turn; continuous corners and a hairline. `MixAccent.color` is the system colour of the
  accent's hue (mauve purple, amber yellow, peach orange). A name's accents are `named_accents(name)`,
  core's `Accent::named_pair`, so a genre, artist or playlist wears the colours it wears on the
  desktop. A playlist is drawn the same way from its first four albums.
- **Suggestions is a grid of cards** (`SuggestionsView`, adaptive columns at least 160 pt): each
  `SuggestedMix` is a `TrackFilter` with a title, a line saying what it holds, an accent pair and a
  mark (Never heard teal/blue, Your favourites mauve/red, Most played red/peach, Recently added
  green/teal, High-resolution green/blue, the most listened artist's mix, `listening_detail`'s first artist, its named accents). *Most played* and *Recently played* set `TrackFilter.played`, so they hold the songs heard and never run on into unheard ones. Covers are
  `pictured_by(filter)`, read for every mix in one `Work.aside` keyed by
  `Library.collectionGeneration`: the albums the filter's first rows fall on in its own order, those
  holding a picture alone, at most four. Pressing the art opens the filtered Songs list; the round
  play button and the context menu (Play, Shuffle, Show songs) play every song it holds, read
  page by page (`Library.everyPage`), so *Never heard* shuffles the whole of it.
  `SuggestionsRow` wears a teal and blue sparkles tile.

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
  completions as a row of glass capsules heading the results (`SearchCompletions`, at most five,
  read from the answer on screen, so the row changes only as the results do); `.searchSuggestions`
  offers only, on an empty field, the last ten searches (`UserDefaults` suite `library-features`,
  key `recent-searches`, shared with the Search tab: a search kept merges into what is stored, and
  each tab re-reads it on appearing), kept when a search is submitted or one of its results opened.
  Completions are never `.searchSuggestions` while typing: that overlay sat over the results and
  moved with every answer. Searching spans the catalog: filters shape browsing
  only, and setting one clears the search. The page opens on the Songs/Albums/Artists picker; songs
  are filtered from the toolbar (`FilterButton`, filled while a filter is set, opening
  `FilterSheet`: genre, artist and album are pickers of the names the filter compares (the track's
  artist, not an album artist; an album's title, since the filter compares folded equality), Cancel
  closes it and *Reset* is a button in the form that clears the draft, applied with *Apply*), and each part of the filter set shows under the picker as a small capsule token
  that clears it. Playlists and Suggestions are reached from the Library tab (Suggestions is a row
  beside Favourites and All songs), online search from the Search tab.
- **The Search tab** (`SearchTab.swift`) has scopes Your library (the default) and Online
  (`Navigation.searchPlace`), one field for both: typing sets `tabSearch.text` always and
  `Features.onlineQuery` only in Online, and choosing Online hands it the text, so a library search
  never reaches the network. Your library shows the Songs results (Top, *See all* pushing the full
  list, *Did you mean*, reads, the completions' row) and, under five songs, "Search online for “…”",
  which switches scope; the tab has no `.searchSuggestions`. With no text the page lists the recent searches and a hint; Online with online
  features off shows why, with Open Settings. Library tools' Online search switches to this tab
  in Online (`Navigation.searchOnline`); there is no destination of its own.
- **Browsing** reads Rust too: songs through `filtered_tracks` pages of `Library.page`, the first
  page published at once where nothing is shown yet and the rest after; albums and artists through
  `albums(order)`/`artists(order)`, cached on `Library` and reloaded with the catalog. The ordered
  match of a filter is kept for the pages that follow it (`Ordered`: one filter, one index, 30 s),
  so paging 100 000 songs filters and sorts once; the index is built outside the catalog's lock
  and by one caller at a time (`Searching::index`). A list of 40
  rows or more ordered by name gets a section per letter run and the section index. `EmptyLibrary`
  shows only once the catalog was read empty and no scan runs; filters matching nothing offer Reset.
  The Library tab's search uses the same model for albums and artists and folds playlist names
  (`searchFolded`, which is core's `folded_letters` through `folded_text`, so *Lodz* finds *Łódź* as library search does), as the Favourites and Add-songs searches fold
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
`moving: false`: one still frame, rendered once into a `CGImage` (`StillBackdrop`,
`FieldPicture.rendered`, kept for the last eight palette, tone and size asked) and drawn as an
`Image`, so a still page holds no Metal layer or display link. A still page with no palette yet
draws nothing, so the app background behind it shows until the cover is sampled and its field fades
in; it never opens on another colour first.

- **The palette** (`Palette`, `Backdrop.palette` in `Artwork.swift`, off the main actor) is five
  linear-sRGB colours with weights: a 32 by 32 sample of the cover in Oklab, k-means seeded by
  lightness quantiles, six rounds. Near-black, near-white and grey pixels are dropped where at least
  a third of the cover is vivid; a cover less than a sixteenth vivid (black, grey, white) is
  clustered whole and keeps its own greys, its lightness held from 0.16 (`Palette.darkestGrey`) so a
  black cover gives a near-black field; only a song without a cover takes the palette built around
  the accent colour. A moving view whose cover is not yet sampled opens on that accent palette and crossfades to the cover's, never on black; the playing song's palette is sampled as soon as the song changes (`BackdropView.prepare`, from `Library.derive`), so Now Playing opens on its own field. Each colour is held to
  Oklab lightness 0.30 to 0.64 and chroma 0.07 to 0.19 (a grey stays grey), then pulled into gamut.
  A superseded sample is dropped at once (cancellation is checked as soon as the cover arrives), so it never assigns the accent palette after a newer song's. Palettes are remembered per song and art revision, so a page opens on its field at once.
- **The shader** is Metal Shading Language in a Swift string (`BackdropShader.source`), compiled once
  per process with `makeLibrary(source:)` on a utility task (`BackdropPipeline`): the offline
  `metal` compiler in the SDK is an arm64 Mach-O that cannot run on the Linux host, so the package
  carries no `.metal` file and no `.metallib`, and SwiftUI's `ShaderLibrary` is unusable. One
  fullscreen triangle; the uniforms (`FieldUniforms`, 192 bytes, mirrored by the shader's `Field`,
  checked before the pipeline is built) are passed with `setFragmentBytes`; each palette colour goes
  in already in Oklab (`FieldUniforms.packed`), never converted per pixel. Five Gaussian blobs
  drift on Lissajous paths, the domain warped by two octaves of value noise; colours mix in Oklab
  with their chroma partly kept, so complementary blobs do not meet in grey; then a lightness
  curve, a vignette and triangular dither on the sRGB-encoded output (`bgra8Unorm`). Every period divides 360 s, the
  length after which the clock wraps without a seam. `BackdropMetalView` is a `CAMetalLayer` driven
  by a `CAMetalDisplayLink` on the main run loop, which hands each frame a drawable already acquired
  (two frames queued ahead) and its presentation time: an `MTKView` takes its drawable on the main
  thread and blocks there whenever the GPU is behind, which stalled every animation on the screen.
  A view that enters a window (a tab switched to, a page popped back to) would show black, or the stale frame it last presented, until the link's first tick, so each time it enters a window with a size, while the application is active, it renders the field at 96 pixels across into a texture at the current time (`BackdropRenderer.placeholder`), lays that over the layer and removes it 0.15 s after its first live frame; the field is slow and blurred, so the two agree. `nextDrawable` is never asked for: a layer with a `CAMetalDisplayLink` raises an exception there, which no `catch` reaches. The link is paused when dormant, paused again after a still frame, and invalidated when the view
  leaves its window. The view draws at scale 1; the compositor
  scales the blurred field up.
- **A song change crossfades** the old field into the new over 1.2 s, eased in and out, in the shader;
  a change mid-fade starts from the blend then on screen.
- **Energy** is the RMS of the visualizer tap (`Features.heardRms`, worked out off the main thread
  with the read, `HeardSignal`), with a 120 ms attack and a 600 ms
  release, raising drift speed and blob size by at most a quarter. Paused, energy falls to zero and
  the drift slows to 18 %. The tap has two reference counts, `hearLoudness` (a backdrop) and
  `visualize` (the visualizer page), so neither switches off the other's; the observed
  `Features.samples` is assigned only while a visualizer page holds it, the scene is in the foreground and a song is playing (`visualizersWatching`; otherwise it is emptied and the page says to play a song), so thirty reads a second
  invalidate nothing behind Now Playing. A backdrop holds it only
  while visible, active, moving and playing, so a paused page leaves the tap off; a tap that fails
  is asked again after 2 s, not at once.
- **Reduce Motion** draws one still frame (time frozen, no energy, no fade) and redraws only when
  the palette changes.
- **Frame rate** follows what is moving (`BackdropView.rate`): 120 fps while playing, 60 paused,
  half that in Low Power Mode or a serious or critical thermal state (`ProcessInfo.sparesFrames`);
  an inactive or backgrounded scene, or a view out of its window, draws nothing. The glass over the
  field is composited again at every frame the field draws, which a 120 Hz frame holds only with the
  rest of Now Playing cheap: the Metal layer is opaque (the shader writes alpha 1, so the compositor
  blends nothing beneath it), the lyrics are layers set per frame rather than views rebuilt
  (*Lyrics*), and the field rests while the cover moves. Before those, 120 fps dropped frames and
  30 fps was kept, which read as plainly low beside everything else at the display's rate.
- **The app background is one field for the whole app** (`AppBackdrop`, the
  `containerBackground(for: .navigation)` of every page; a clear one is drawn as the system's own, so
  a single view behind the `TabView` never shows). Every page's `AmbientLayerView` shows the same
  `CGImage`, rendered by the one `AmbientField` at scale 1 for the largest page in a window, so a
  pushed page, a tab switched to and a page popped back to show exactly the frame the page before
  showed, from their first frame: one picture is drawn per tick however many pages hold it, a page
  joining a window takes the latest picture at once (rendering one synchronously only where none of
  its size exists), and no page holds a Metal layer or display link of its own. It ticks on one
  `CADisplayLink` at 10 fps (5 in Low Power Mode or a serious thermal state): the field drifts at
  18 % speed, under two pixels a frame, so a faster tick only made every glass surface over it
  composite again. It never reads the transport, so playing or pausing redraws no page's
  background. The palette is built around the accent colour (re-sampled when the accent changes,
  crossfaded at 30 fps for its 1.2 s), `BackdropTone.ambient` (dark floor 0.05 with a range of 0.10
  to 0.32, light floor 0.96 down to 0.80, chroma and range scaled by the strength, so system labels
  keep their contrast); a change of tone (light and dark) renders at once. The field's clock
  advances only while it flows: Now Playing's cover, an inactive scene and the background freeze it
  where it stands (`AmbientMotion.frozen`), so it neither jumps to another frame when Now Playing
  opens nor when it closes; *Moving background* off, or Reduce Motion, draws the still moment.
  `ThemedPage` on each stack's root and every `DestinationView` hides the scroll content background
  and sets that background. It stays frozen too (no display link, no render) while a page that draws its own backdrop is the top of the tab's stack (`Navigation.coversAmbient`: an album, artist, playlist, lyrics or visualizer page, as well as Now Playing). `AppearanceState` keeps three settings in the `appearance` defaults,
  edited in Settings › Appearance › Background (`BackgroundView`, `Destination.background`, a page of
  its own beside `AppearanceView`): `themedBackground` (on), `movingBackground` (on; off draws the
  still frame), `backgroundStrength` (0.4). A plain list's rows draw their own opaque fill, so each
  one is `themedRows()` (a clear `listRowBackground` while the background is on), keeping
  separators; a new plain list does the same. A grouped page (`ThemedList`, `ThemedForm`, which wrap
  `List` and `Form`) tints its cards instead, `themedCards()` filling each with `Color.themedCard`
  (white at 66 % in light, near-black at 62 % in dark), never a material: the field is already
  blurred, so a blur over it showed nothing, and one blur per row was recomputed on every scroll
  frame. Sheets keep the plain `List` and `Form`. Pages with their own backdrop cover it.
- **Failure falls back to the gradient**: no Metal device, a failed compile or pipeline, or a
  uniform size the shader does not expect is logged in full at `.error` under subsystem
  `org.resonate.ios`, category `backdrop` (a successful compile logs its time at `.notice`), and the
  view draws two radial gradients from the palette's first two colours.

## Lyrics

`LyricsPane(track:compact:)` (`LyricsPane.swift`) is the desktop pane ported (`ui.md`'s lyrics
entries, `mobile.md`'s *Lyrics*): the same `TURN`, `Glide`/`Leg`, falloffs, cascade, arrival,
breath dots, word sweep and *END OF LYRICS* mark, with the same constants, arithmetic drawn from
`lyric_frame`. `LyricStage` holds the motion outside observation (only `beat` is observed, bumped
where a resting pane must wake) and turns a moment into a `LyricScene` of plain values (a row's
standing, inset, inks and wipe; the sheet's offset), about 0.1 ms a frame.

**The pane is drawn by Core Animation, never by SwiftUI views per frame** (`LyricLayers.swift`).
`LyricLayers` is a `UIViewRepresentable` over `LyricLayerView`, whose own `CADisplayLink` asks the
stage for each frame's scene and sets layer properties inside one action-free transaction: each
line's text is typeset once (`LyricTypeset`, TextKit 1 at the column's width, bold at the pane's
size and leading) and drawn once into a glyph mask over an ink layer, so a turn changes the ink
layer's colour, the row's opacity and translation and the sheet layer's position, and the word sweep
is two `CAShapeLayer`s (sung, and the blending letter) covering `enumerateEnclosingRects` of the
wipe's UTF-16 range, rebuilt only when the range moves; the breath dots and the end mark are layers
of their own. Nothing is laid out, diffed or drawn again in a frame: as SwiftUI views (a
`TimelineView` rebuilding every row) the pane cost about 5 ms of a 120 Hz frame's 8 on the main
thread, though two or three rows' bodies changed. `LyricLayout` places the rows once per sheet,
width, height and measures (a row's breath room, padding, its side's column and the end mark), and
hands every row's frame to the stage as its measurement (`measured`, `measuredSheet`), so
`landing()` and the scroll bounds read the layer layout. A row more than a pane (at least 320 pt)
beyond the pane's edges is hidden, and twice that far its glyphs are released, so a long sheet
holds a few dozen masks at most. The edges dissolve through a `CAGradientLayer` mask; the
arrival fades the pane layer.

**The link's cadence follows the stage** (`LyricCadence`): while anything turns, a word-synced sheet
plays (its sweep) or a hand holds or flings the sheet, it asks 120 Hz (80 at least); a line-synced
sheet playing with nothing turning ticks at 20 Hz (`lineSyncedTick`) until a landed frame differs
from what the turns aim at (`unfollowed`, which `isMoving` includes), so a line change starts within
a tick; with nothing moving it pauses, and a `beat` bump (a frame landing, a drag beginning,
*Follow*) wakes it through `updateUIView`. The lyrics alone ask 120 Hz, only while they move: a whole
Now Playing held at 120 Hz (tried) dropped a frame at each of the field's redraws, since the field
and every glass control over it composited again fit a 60 Hz frame and not a 120 Hz one.

**Touch stays in SwiftUI**: a `DragGesture` (at least 10 pt, simultaneous with the cover's own
dismissal) and a `SpatialTapGesture` on the representable feed the stage; a tap is resolved to a
line by the layer layout through `LyricHits`. A `UIPanGestureRecognizer` on the layer view (tried)
never received the drag inside the full-screen cover. The sheet's offset is the stage's, not a
`ScrollView`'s: a vertical drag moves it (rubber-banded past the ends, released into a `Leg`
launched at the finger's pace), opens the falloff out, offers *Follow* (a glass button present only
while offered), and following returns `HANDS_OFF` after the last touch. VoiceOver reads one
`LyricLineElement` per line (its text, its time, selected while lit, activating it seeks), framed
where the line is drawn, and three-finger scrolling turns a page.

`LyricClock` steers wall time toward the snapshot's position projected from when it was heard (a
track change, pause or drift past 250 ms takes the position as is): the position is extrapolated
per display frame from the 250 ms poll's reading, never held between polls, and never read from
`Features.tick`. **The frame drawn is the frame of the tick drawing it**: `kept_lyric_frame` answers
from the sheet Rust already holds (a lock and the arithmetic, no SQLite) on the main actor inside the
tick, so the word sweep moves every display frame; only a sheet not yet held is asked on its own
serial queue (`LyricFrameSource`), never `Work.inOrder`, one in flight and the latest owed, landing on
the main actor through a closure built `nonisolated`, since that ask may read the catalog. An
asynchronous frame was one or two display frames old and landed on alternate frames at 120 Hz.
Lines seek only while the pane's song is the one playing; Reduce Motion snaps glide, rise and
cascade but keeps the brightening. `compact` is the pane embedded in Now Playing: smaller words,
tighter gutters, no chrome. Ink follows the system's legibility settings (`LyricLegibility`): under
Increase Contrast or Reduce Transparency the unlit ink is the lit ink, unsung words sit at 70 % and
no line stands below 85 %; otherwise a sheet with nothing lit (unsynced, or the song not playing)
stands at `ADRIFT` 0.8 over the 0.5 muted ink, since 0.45 over it read at 22 %. `LyricsView` adds
the backdrop, the toolbar's refresh and the `LyricAttribution` badge.

## Artwork and details

- **The background lookup starts itself** (`Enrichment.swift`): `Enrichment.begin` asks
  `start_enrichment` once the player opens (`Features.initialize`), after a scan
  (`Features.scanned`), after songs land from another device or a download, when online features
  turn on and when the network becomes unmetered. It is refused while "Fetch artwork and details
  automatically" is off (`enrich-automatically`, on by default) or, with "Only on Wi-Fi"
  (`enrich-unmetered`, off by default), while the path `NWPathMonitor` reports is expensive or
  constrained, for a song landing from a download or another device as for any other start; a
  metered network then stops the pass. Both keys live in the `library-features`
  suite. Each start also seeks the playing song, and `Features.tick` hands every song change to
  `Enrichment.playing`, which calls `seek_enrichment` while a pass runs.
- **The status is polled every 2 s while a pass runs and the scene is active**: going inactive
  cancels the poll (the pass goes on in Rust), coming back reads the status once and resumes it.
  The line reads "Finding artwork and details · N of M", "Artwork and details up to date" or the
  `EnrichmentStop` in words; a finished pass refreshes the library once, and one that ended while the scene was away is settled the same way when it comes forward (`Enrichment.read`, `unsettled`). Switching *Fetch automatically* off, or *Only on Wi-Fi* on while the path is metered, stops a running pass. Settings' *Artwork and
  details* group (shown while online) holds both toggles, "Look again for missing covers"
  (`ask_again_for_covers`, then a start), the line and Stop. Library tools' "Refresh artwork and
  details" confirms with the song count, stops any pass and starts one with `refresh`, and offers
  Stop while it runs. The Inspector's button is the one-song `enrich_track`; no lookup posts a
  banner per song.
- **Pictures refresh by the changed-art revision**: every status poll, scan, landing and
  single-song lookup asks `art_changed_since` from the last revision (the first answer only sets
  the baseline, judged by whether a revision was held when the ask was made, so two overlapping
  first asks both only set it); the named songs alone have their `ArtStamp` bumped, which
  `ArtworkView` and the backdrop key their `.task` on. Each song's stamp is an observed object of
  its own (`Features.artGeneration(of:)` reads only that song's), so a cover found for one song
  redraws that song's views, not every artwork on screen. A rise in `portraits` bumps
  `portraitGeneration`. Past 4096 changes in a session, or 20 000 stamps, the stamps are dropped and
  `artEpoch` stepped, which every song's generation includes.
- **The cache decodes to the size drawn** (`ArtworkCache.swift`): `ArtworkView` measures itself
  (`onGeometryChange`), multiplies by the display scale and rounds up to a bucket of 96, 192, 384,
  512, 768 or 1024 px; an entry is keyed by picture (or portrait) and bucket, so the twelve songs of an album decode one
  image: a song is first mapped to its picture id (`artwork_picture`, one cheap query, remembered per song,
  revision and generation in `PicturesOfSongs`, at most 20 000) and the bytes are read by picture id
  (`picture`) only when that picture is not decoded at the bucket. Nothing is tracked per subject:
  a changed picture is a new id and a changed song a new key, and `NSCache` evicts what is left.
  The picture lookup runs at most 4 at once (its own `DecodeSlots`), so a fast scroll never piles
  blocking catalog reads onto the aside queue, and `CGImageSourceCreateThumbnailAtIndex` decodes on
  `Work.onScreen` (user-initiated, apart from `Work.aside`'s utility work) with the bucket as its
  largest edge, at most 4 at once (`DecodeSlots`, an actor); requests for one key join one task, a view
  that disappears leaves its wait, and the last to leave cancels a decode still waiting for a
  slot. Pictures are decoded display-ready (`preparingForDisplay`), so nothing converts them on the
  main thread at first draw, and cost their bitmap bytes against 160 MiB (`DecodedPictures`; a 96 px
  row about 36 KiB, a 512 px shelf 1 MiB); a song without a picture is remembered as missing until
  its art changes. **A held picture shows in the frame the view appears**: `ArtworkView` asks
  `cachedArtwork`/`cachedPortrait` (a synchronous read of the same cache) as soon as it knows its
  bucket and before any await; only a picture that had to be decoded fades in (0.2 s), so rows
  scrolled back into view never pop. **A view's bucket only grows, and grows once it settles**: a
  smaller measure keeps the picture shown, a larger one already held swaps at once, and one that
  must be decoded waits 300 ms of steady size, so a cover zooming between Now Playing's layouts
  does not decode at every bucket it passes. `ShownArtwork` (the last picture per song, for a view's first
  frame before it is measured) keeps 64 MiB. **Every ask names its bucket**: the
  palette sample (32 by 32) reads the 192 px tile and the lock-screen artwork the 384 px card, never
  the 1024 px full size, which cost about 4 MiB cached and evicted row thumbnails. A circular `ArtworkView`
  (`ArtworkView(track:corner:circle:)`, artist rows and headers) shows `artist_portrait` for the song's `artistMbid`
  first and falls back to the album's picture. It clips with the concrete shape
  (`ArtworkClip`: `.circle` or `.rect(cornerRadius:style:)`), never an `AnyShape`, which hides the
  rounded rectangle from SwiftUI and turns the clip into a mask drawn offscreen (the online placeholder tile wears the same clip).

## Downloads

- **`DownloadQueue` runs jobs by the boundary's job number** (`DownloadQueue.swift`): `enqueue`
  begins one job per song not already queued (`begin_download`), two run at once and the rest
  wait. A job asks `find_download` with every account that is on and ready
  (`Features.downloadAccounts`, `ProviderAccounts.asked`), keeps a renewed TIDAL token on the TIDAL
  account (also from a find that failed, so the next job does not start from a token already spent), creates the destination with `Folders.fresh` on `Work.aside`, journals its URI, copies
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
  provider gave a length, else the bytes received. A job is assigned only when the poll changed it (`update`), so a job finding, validating or stalled redraws nothing. Cancel calls `cancel_download` for that job
  alone; a failed or cancelled job deletes only the document it created and shows the boundary's
  sentence (`explained`) on its row, never in the banner, with "Try again"; a finished one offers
  Play and Go to album.
- **Saved for later queues into the same jobs**: "Download all missing" enqueues every want not
  held, each row leads with a placeholder tile, keeps title and artist · album to a line each and
  ends in a Download icon (Remove is its swipe), a held row reads "In your library", and pulling to
  refresh reads the matches again. Online search's song rows enqueue one song.
- **Wants are asked on their own while the scene is active** (`DownloadQueue.askForWants`): the loop
  reads `next_wanted_due`, sleeps until then held to 30 s at least and 30 min at most (the
  desktop's `RETRIES_ASKED_AT_LEAST` and `ASKED_EVERY`), then enqueues `poll_wanted_due`'s songs into
  the remembered folder, only while online, on a network that is neither expensive nor constrained
  (Cellular and Low Data Mode wait), outside Low Power Mode, with a ready account and a remembered
  writable folder (`mayAskForWants`): it never raises the folder sheet or the provider notice. A song it asked is not asked again on its
  own for 30 min, so a provider that stays away cannot pile failed rows up. It ends, rather
  than waking every 30 s, when any of those conditions fails, nothing is due, the scene goes
  inactive or the switch is off, and restarts on any change of wants, accounts, providers, folder
  list, online, network cost or Low Power Mode. Each loop knows its generation, so a task that
  finishes after a restart never clears the loop that replaced it. "Download saved songs automatically" on Saved for later
  (`download-wants-automatically` in `library-features`, on by default) is the switch.
- **The journal is a list**: `unfinished-download` holds the URIs of every job's document still
  being written, removed one by one as each lands or is deleted (`Folders.delete` answers whether
  the file is gone, and an entry whose file could not be deleted stays); on the next start each is
  deleted unless the catalog already indexes it (a kill between `index_download` and the journal's
  removal left a song, not a fragment), and where the catalog cannot be read nothing is deleted.
  A single string left by an older build is read as a list of one. A song is held from the moment
  it is asked (`beginning`, before `begin_download` answers), so a double tap or *Download all
  missing* beside the wants loop makes one job.
- **The destination is remembered**: `download-folder` in `library-features`; the folder sheet
  appears only when none is kept or it is no longer readable and writable, with "Remember this
  folder" on by default; Downloads offers "Ask for a folder each time".
- **A running job continues in the background as a continued-processing task** (`BackgroundHold`,
  also the sync transfers' hold): `hold(true)` begins a plain background task and, while the app
  is active, submits a `BGContinuedProcessingTaskRequest` (`org.resonate.ios.downloads.<uuid>` or
  `.transfers.<uuid>`, strategy `.fail`; both wildcards are in `BGTaskSchedulerPermittedIdentifiers`
  and registered on the main queue in `ResonateApp.init`). Once the system starts it, the plain
  task ends and the system's progress shows the batch: each job a thousand steps, settled whole,
  running by its fraction, "N of M songs" (transfers: bytes received of the known sizes, "N songs
  left"). `hold(false)` completes it; a refused request leaves the plain task alone. Either
  expiration handler, built `nonisolated`, cancels every running job and pauses the waiting ones
  until the scene is active again, so each cancelled job still deletes its document and its journal
  entry, or the next start does, and the continued task completes unsuccessfully.

## Listen

- **Listen names the song playing around the iPhone** (`Listening.swift`, `ListenViews.swift`), the
  desktop's Listen sheet (`ui.md`) on the microphone: a sheet (`Navigation.listening`, medium and
  large) opened from the Search tab's ear and Library tools' *Listen*, recording at once; closing it
  or the scene going to the background stops a recording and drops the ask about it.
  `Listening.stage` walks `ListenStage` (recording, asking, then found, unknown, silent, unreached,
  offline, denied, failed); `ListenNext.after` is the desktop's `then`: a miss or a silent clip is
  recorded again up to `triesOnAMiss` (3), a service out of reach is said at once. *Keep listening*
  records another clip `followedEverySeconds` (20) after a named song or the last miss, keeping a
  song whose `HeardSong.key` (folded title and artist, made in Rust) matches quietly. A new run
  awaits the previous task first, so a cancelled recording's cleanup never lands inside the next
  one; the recognise call is raced against cancellation (`OnlyOnce`), so closing the sheet while
  *asking* frees the next run at once and the late answer is dropped, and the stage turns to
  recording on the tap itself, so extra taps start nothing. *Keep listening* ends where a clip
  cannot be asked (a service out of reach, a failure, offline), saying why, instead of recording
  every 20 s. Resonate pauses when it listens, since the microphone would only hear it. A found song is a card (cover decoded on `Work.aside`, title, artist, album · year, the
  naming service) with *Find it* (the Search tab, Your library, artist and title typed), *Open*
  where the service gave a link and *Listen again*; the last eight stay as *Heard earlier* for the
  run, and a new song is felt (`sensoryFeedback(.success)`).
- **`Microphone.record`** asks `AVAudioApplication` for the microphone (`NSMicrophoneUsageDescription`),
  switches the session to `.playAndRecord`, `.measurement` with A2DP kept, through
  `NowPlaying.beginListening` (an activation refused puts `.playback` back at once; a session
  interruption or an engine configuration change ends the clip with `MicrophoneFailure.interrupted`,
  never a short clip spending a try) (route changes are not reported to Rust meanwhile, so the rate the
  category moves costs no reopen of the output, only the one report on `endListening`),
  taps `AVAudioEngine`'s input as a mono mix into a `ClipSink` (a `Mutex`, its tap block built
  `nonisolated`) until 12 s are held or 3 s past that, and hands the samples and the input's rate to
  `recognise_heard`. `NowPlaying.endListening` puts back `.playback` and, with nothing playing,
  deactivates the session so another app's audio may resume.
- **The AudD token** is the keychain's `audd-token`, typed in the sheet's *Recognition* section;
  blank, only Shazam is asked (`online.md`).

## The equaliser

- **The curve is drawn and shaped on a graph** (`EqualiserGraph.swift`, in `SoundSettingsView`):
  log frequency 20 Hz to 20 kHz across, decibels up, gridlines every 5 dB with 0 dB marked and the
  edges labelled, 100, 1k and 10k labelled among fainter decade lines. The span is ±15 dB widened in
  5 dB steps to hold the response and the handles, at most ±30 (a pass filter's floor is clipped),
  and frozen while a handle is held, as on the desktop. The response is `response_curve` at 256
  points and the stream's rate, asked by `EqualiserResponse` on `Work.aside`: the latest curve asked
  wins and asks are at least 30 ms apart while one is pending, so a drag is followed without
  queueing; a curve asked that equals the one drawn clears what is pending, so a step back within the throttle never lets the older ask land. It is washed to the zero line.
- **A handle per band**, numbered, lifted by the preamp (a gainless kind sits at the preamp). A
  one-finger drag beginning within 32 pt of a handle moves it, frequency across and gain up (tidied
  to 0.1, 1 or 10 Hz by decade and to 0.1 dB); a pinch turns the Q of the held band, else the
  selected one; a tap on empty space adds a peaking band there (Q 1) and switches the equaliser on.
  Grab and release are felt (`sensoryFeedback`). A drag the system cancels (a call, an alert) never
  reaches `onEnded`; a `@GestureState` flag falling with a touch still held releases it and discards
  the draft, so the next touch starts clean. What a gesture moves is a draft drawn at once; the
  engine and the store hear the release, once, as the sliders did, and the draft gives way when the
  committed curve arrives, or a second after the release where none did, so a refused commit
  cannot linger.
- **The selected band's inspector** is a section below: type (all eight kinds), frequency, gain
  (gain kinds only) and Q, each typed (committed on return) or stepped (a semitone, 0.5 dB, a sixth
  of an octave of Q), and Delete. Then the preamp slider, *Fit the preamp* (`fitted_preamp` on
  `Work.aside` at the stream's rate, committed as a preamp move is; off with no bands), *Add a band*
  (1 kHz) and *Reset*, which
  confirms either removing every band or starting from ten flat octave bands. VoiceOver reads each
  handle as an adjustable element, labelled with its number, kind and frequency, valued with its
  gain and Q: swiping steps the gain 0.5 dB (Q, for a gainless kind), activating selects it.
- **Headphone correction is a section of the equaliser page** (`HeadphoneCorrectionSection`, then
  `ProfileImportSection`), not a page of its own: it searches AutoEq as the listener types (350 ms
  settle, `Features.correctionRequest` refusing a stale answer), marks the current preset, and loads a
  tapped headphone's correction as bands, confirming first where the curve has bands; EqualizerAPO or
  GraphicEQ text comes from the clipboard or a file (`fileImporter`, read on `Work.aside`, at most
  64 KiB and one byte, so the boundary's `TooLarge` speaks for a larger one). The notice after a
  load counts the bands and says what was fitted, passed over or held to the limits.
- **Stored as text**: `PlaybackPreferencesStore` keeps the curve as `equaliser_text` output under
  `equaliser-curve` (`eq.md`'s *The phones*), migrating the older keys once and writing the result
  back at once.
- **A curve may be kept for one output** (`OutputRoute`: the session route's first output, by port
  `uid`, named by `portName`). *Only for <output>* on the equaliser page binds the curve playing to
  that output (`Library.bindEqualiser`): `equaliser-curve-for` and `equaliser-curve-name-for` map a
  port to its text and name, `equaliser-route-names` to its name, all in the `playback` suite. While
  the output in use is bound, every edit (graph, inspector, AutoEq, import) is written to its curve,
  else to the shared `equaliser-curve`. `NowPlaying.reportRoute` hands each route to
  `Library.outputRouteChanged`, which sends the bound or shared curve where it differs from the one
  playing (`followRoute`); unbinding, or forgetting the output in use under *Curves kept for other
  outputs*, falls back to the shared curve at once. The switch stays one for every output, as on the
  desktop. The engine still gets one curve as its `Equalisation` fallback.
- **Room correction** is the equaliser page's last section (`RoomCorrection.swift`): choosing an
  audio file reads at most 64 MiB and one byte on `Work.aside` and keeps it through
  `keep_room_correction`, then switches it on; the switch, the response's channels, rate and length
  and *Forget the response* follow (`eq.md`'s *Room correction*). One response serves every output.

## Siri

- **Siri reaches the library through App Intents** (`Siri.swift`), all four `AudioPlaybackIntent`s,
  so they run in the background and may start audio from a locked phone, none opening the app: *Play*
  (`ResumePlaybackIntent`: resumes the restored queue, or shuffles the library where nothing is
  loaded), *Play from Your Library* (`PlayFromLibraryIntent`, a `String` Siri asks for: a playlist
  whose folded name is the answer, else `search_library`'s artist or album of that folded name,
  else its songs from the best), *Shuffle Your Library* and *Play Favourites*, each answering with
  a dialog that names what plays or why nothing does. `ResonateShortcuts` gives each its phrases
  (*Play Resonate*, *Play a playlist in Resonate*, *Shuffle Resonate*, *Play my favourites in
  Resonate*); a phrase holds no parameter, so the music is asked for, never spoken in the phrase.
  Siri's own *pause*, *skip* and *go back* reach the remote commands, so there is no intent for them.
- **An intent opens the player itself.** A background launch connects no scene, so no `onAppear`
  runs: `ResonateApp.init` makes the `Library` and registers it with `AppDependencyManager`, each
  intent takes it as a `@Dependency` and calls `start()` (which runs once) and `openAgain()` (where
  an earlier open failed) before
  `player()`, and plays through the same `Library.play`/`shuffle`/`resume` as a tap, so activation
  and the lock screen follow. Tracks are read whole, page by page, on `Work.aside`
  (`Library.everyPage`), the snapshot on `Work.inOrder`.

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build && python3 app-intents.py --ipa` must
produce `ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data
race is an error); with a device attached, `xtool install xtool/Resonate.app` installs it (the
`run-ios` skill).
