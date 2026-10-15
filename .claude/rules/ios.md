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
  A 250 ms poll reads the snapshot and saves every 20th tick, as Android does; the queue is read
  only when the snapshot's `queue_revision` or `catalog_revision` moved since the queue last shown
  (`QueueListed`), since reading it costs a catalog lookup and a record per row and a whole-library
  queue read four times a second kept the CPU busy and the main thread comparing thousands of rows.
  State a poll reads (`syncStatus`, `syncHandoffs`, `transferring`, enrichment's status) is assigned
  only when it changed, so an unchanged answer redraws nothing.
- **What moves every tick lives in `PlayClock`** (`library.clock`): position, duration, sleep time
  left, output rate and underruns. `snapshot` is assigned only when something else changed, and
  `currentId`/`currentQueueId` only when the song does, so a list that marks the playing row is not
  redrawn four times a second; only the views that show the position (Now Playing
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
  Sync reports once as well: songs landing from other devices are counted and told in one notice
  when the transfers stop, a landing failure is told once per distinct text, and a failing sync
  poll once until it recovers.
  A song whose file has gone is hidden, never deleted (`hide_missing_songs`, given the readable
  folders' prefixes; `documents.missing`) at launch, whenever the scene becomes active again, after a
  completed scan and on *Sync now*: song lists, search, albums, artists, statistics, top songs, a
  playlist's rows and the wants' *In your library* skip it, and its rows leave the queue
  (`left_the_queue`; the playing row too, which moves on), while its playlist rows and favourite
  stay so no removal reaches the sync ledger as the listener's edit. A playlist edit by shown index
  is mapped to the stored row (`shown_positions`). Indexing the file again or finding it readable
  restores it; a document indexed under another name that names the same song
  (`Recording::names_the_same_song_as`: a song fetched back from a device or a download) takes the
  hidden row's place (`take_the_place_of_missing`): its playlist rows, favourite, listens, lyrics,
  artwork and studies move over, the playlists' revisions step, and the hidden row is deleted.
  Becoming active with songs newly hidden while sync runs forces a sync, so they are fetched back.
- **Sync shows what it is doing.** `SyncStatus` carries `settled` (the service's `Shown.settled`)
  and `transfers` (title, artist, the sending device's name, bytes and `SyncTransferState`).
  `Features.syncActivity` is *fetching* while a transfer waits or moves or a received song is being
  written into the folder (`syncLanding`), with a fraction where every size is known, else
  *syncing* from *Sync now* (`syncAskedAt`) until the status settles (a notice says with how many
  devices), the server is refused or unreachable with nobody nearby (the standing as a notice), or
  45 s pass; the poll runs every second meanwhile. `SyncProgress` is a glass card in the status
  banner that opens the sync page. `transferring` (the background hold, the 2 s poll) counts only
  received songs that can land now (`landable`: a writable folder and no back-off), so a song with
  nowhere to go never holds the app awake or keeps `cameForward` from rebuilding sync.
- **Erase all data** (the last Settings section, confirmed) reads every song's URI (`song_uris`,
  hidden ones included), shuts the player down, deletes each of those files through `Folders.delete`
  (the songs in granted folders as well as the app's own; nothing else in those folders), then
  `Erasure.everything` empties Application Support, Caches, Documents and tmp, removes the
  `library-features`, `playback` and `provider-accounts` suites and the standard domain, and deletes
  every keychain class, then the app exits. With the pairing code gone, other devices hear nothing.
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
  `SyncConfig.nearby_only`; and the device name). `PairingSheet` shows the code with Copy and Share,
  joins another device's code (confirmed where a code is held), and *Make a new code…* (confirmed)
  is `Features.newSyncGroup`. The code is blurred and `privacySensitive` under *Tap to show* until
  tapped, and covered again whenever the sheet opens. **A computer is paired by the system
  camera**: `Info.plist` registers the `resonate` URL scheme, so the Camera app or the Control
  Centre scanner opens the desktop's QR code (`resonate://pair/<code>`) in Resonate; `Library.open`
  hands it to `Features.offerPairing` (`sync_code_in_link`), and the root view asks *Join these
  devices?* before `joinSync` (which also turns sync on) and opens the sync page. Resonate itself
  never touches the camera, so it asks for no camera permission. No page names the hosted server's domain. A device that is away and
  not this iPhone swipes (or long-presses) to *Remove*; the section's menu removes every such one
  (`forgetSyncDevices`, `sync_forget_device` by id); rows are keyed by `SyncedDevice.id`, since a
  reinstalled phone keeps its name. Landing a received song backs off per song
  (`landingRefused`), never pausing the rest (`android.md`).
- **A sync setting is kept before sync restarts.** `saveSync` writes the keychain and defaults and
  assigns `syncConfig` on the main actor at once, then restarts on `Work.inOrder`, so a second switch
  flipped straight after builds on the first; `applySync` puts a minted code into the *current*
  config only where its key is still the one that start used. `syncNetwork` runs on `Work.inOrder`
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
  awake, `NearbyDevices.restart` drops the listener and browser and `applySync` starts sync afresh
  (`sync_start` leaves the old one first); then missing songs are hidden and, where some were,
  sync is forced.
- **`Folders`** is the `Documents` callback. A chosen folder is a security-scoped bookmark kept in
  Application Support (`folders.plist`); the app's own Documents folder (`UIFileSharingEnabled`,
  `LSSupportsOpeningDocumentsInPlace`) is root `local`. A scan walks at most 32 levels, admits
  `audio_extensions()`, reads a folder's cover/folder/front JPEG or PNG within 8 MiB and a
  same-stem `.lrc` within 1 MiB. Fresh destinations are opened `O_CREAT | O_EXCL`. A file opened
  in Resonate from elsewhere is played where it already is when its resolved path lies in a granted
  folder (`Folders.granted`, the most specific root), else played from an identical copy already in
  `local` (`localCopy`: same name or `name (n)`, equal contents), else copied into `local` and
  played; files arriving together (`onOpenURL` fires once each; 250 ms settles a batch) are indexed
  in turn and queued in order, the first playing. A folder's access is measured, not
  assumed: `FolderAccess.of` reads `isReadableFile`/`isWritableFile` and the URL's
  `isReadable`/`isWritable` while the security scope is open, so a read-only grant is never offered
  as a destination for downloads or synced songs. Access is cached per folder: `folders()` resolves
  only a bookmark it has not measured, `refreshed()` re-measures every one on `Work.aside` (the
  folder and sync pages call it, and so does every hiding of missing songs, since a drive unplugged
  while the app was away still reads as readable from the cache), and a scan re-measures its own root. A folder removed hides its songs at once
  (`hide_songs_under` its prefix, the queue rows with them), though its files are still there, and
  its path is remembered (`forgotten-folders.plist`, path to root id) so granting it again gets the
  same root id: a rescan indexes the same URIs and restores the hidden rows with their favourites,
  playlist rows and listens, instead of minting a second copy of every song. A folder whose resolved,
  symlink-free path is already granted, or is the app's Documents, is refused
  (`FolderFailure.alreadyAdded`). A scan runs on a GCD queue where `Task` cancellation cannot reach,
  so it walks under a `ScanStop` flag; removing a folder stops its walk.
- **`NowPlaying`** owns the audio session (`.playback`, long-form), reports the route to Rust,
  pauses on an interruption (resuming when the system says so) and when the old route disappears,
  and drives the remote commands and now-playing info (artwork bounded at 384 px, rendered at scale 1 off
  the main actor). The category is set at start but the session is activated only on play
  (`activate()`), so opening Resonate never stops another app's audio. **The session is asked for
  the playing song's rate** (`preferRate`): on every song change and activation, while "Match the
  song's sample rate" is on (the output page; `follow-song-rate` in the `playback` suite, on by
  default), `setPreferredSampleRate(track.rate)`, `0` (no preference) while off, then the route is
  reported again, so a rate the route took moves `route_changed` and reopens RemoteIO at it; a route
  that keeps its own rate (the speaker, Bluetooth) changes nothing. Now-playing info is written
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
bottom ran through the title and artist, since the system fixes the accessory's height). Inline (the
tab bar minimised on scroll) it measures itself and keeps what fits: a 28 pt cover and the play
button always, the title from 150 pt, next from 200 pt (a minimised capsule beside the search
button is about 220 pt, and without next the title trailed into empty glass), tighter spacing and a
12 pt inset each side, so the square cover clears the capsule's rounded end; the title stays the
accessibility label. Expanded, the artist line is `Color.primary` at 60 % opacity, never the
hierarchical `.secondary`: that is drawn vibrant against the accessory's glass, which is not drawn
while Now Playing zooms back into it, so the artist showed only once the zoom ended. The output page holds `AVRoutePickerView`, since iOS owns the route.

- **Now Playing** is a full-screen cover zoomed out of the mini player's artwork
  (`matchedTransitionSource`, `.zoom`), dismissed by dragging down. Its slider commits one seek on
  release and holds the scrubbed value until the clock comes within 1.5 s of it or a second passes.
  Repeat cycles off, all, one. Volume is the system's, drawn as the scrubber is (`VolumeRow`: a
  capsule between the speaker icons in one `HStack`, so nothing can sit off their line): an
  `MPVolumeView` lays its slider out late and again on the first touch, which left it off centre
  until touched. `SystemVolumeLevel` reads `AVAudioSession.outputVolume` (KVO, built `nonisolated`)
  and sets the level through the slider of an `MPVolumeView` kept at 1 % opacity behind the capsule
  (`VolumeDriver`, its route button hidden), whose presence also keeps the system's volume HUD away.
  The glass row toggles two modes inside the cover: Lyrics (the compact `LyricsPane`) and Up next,
  each shrinking the artwork into a 56 pt header row. **Both are one layout** (`NowPlayingView.layout`):
  only the titles, the pane, the sleep countdown and the volume row come and go, while the scrubber,
  transport and glass mode row keep their identity and move, so no glass control crossfades with a
  copy of itself. **The cover is one view moved by a transform**:
  each layout holds an empty slot (`LargeArtwork`, `CompactTitles`) reporting its bounds through
  `CoverSlot` (an anchor preference keyed by `CoverPlace`, so the slot followed is the one the mode
  names; while one layout fades out beside the other both report, and whichever came last in the
  tree won, which left the cover in the header until the fade ended and then jumped it back
  unanimated), and `MovingCover`, an overlay above both, draws the artwork once
  at the large slot's edge and reaches the slot in force by `scaleEffect` and `offset`, so the spring
  re-lays out and re-measures nothing (two `ArtworkView`s under `matchedGeometryEffect` resized and
  re-measured a live image every frame). The pane or
  list is mounted only once that spring is logically complete (`NowPlayingView.settled`) and
  faded out in 0.12 s as the cover starts to grow back, so building or drawing a `List` or the lyric
  timeline barely overlaps the cover's frames; Lyrics to Up next crossfades at once, the cover staying put. Sleep,
  equaliser, visualizer and output sit in the More menu; no tab's toolbar carries the sleep timer
  (Settings has its row). At accessibility text sizes the artwork layout scrolls.
- **The queue** (the tab and the player's Up next alike, `QueueList`) opens on Now playing at the top
  with History above it, reached by scrolling up and drawn faded, then *Playing next* (the rows the
  engine's `Queued::next` holds, `Snapshot.next_first`/`next_count`) and *Continue playing* (*Up
  next* where nothing was asked to play next), as the desktop's four parts (`QueueParts`); it scrolls
  back to the playing row whenever the song changes. In the player the list takes the full width
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
  its symbol and title as separate views, each given its colour, never a `Label`. An album lists its tracks by disc,
  a *Disc N* header only where there is more than one, each row numbered with its length at the
  trailing edge and a second line only for a track artist other than the album's.
- **Every album, artist and playlist row acts on the whole collection** (`collectionActions`, a
  `CollectionSource`): a context menu with Play, Shuffle, Play next, Add to queue, Add to playlist
  and an album's Go to artist, and the song row's leading Play next / Add to queue swipe, its songs
  read on `Work.aside` when the action is chosen (`album_tracks`, `artist_tracks`, the playlist's
  first page). Lists, search results, shelves and the artist page's albums all carry it.
- **One row per kind**: a collection is a `CollectionRow` (lit title, optional chevron) and a song a
  `TrackRow` (lit by `query` in search results, `showsAlbum` on an artist page) wherever listed, so a
  search result swipes, marks the playing song and is felt as a list row is; every artwork edge is a
  `@ScaledMetric`.
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
  reload, keeping the last answer meanwhile. A playlist's rows are keyed by song and occurrence, move and remove at once, and
  every edit reads the playlist's latest revision inside the ordered queue; its rows survive a
  page pushed over it and are dropped when it is left; the page pops itself when the playlist
  disappears.

## Playlist files and listening

- **A playlist file is read, never kept**: M3U, M3U8, PLS and XSPF
  (`playlist_file_extensions()`, declared in `Info.plist` as `org.resonate.playlist`) opened in
  Resonate or chosen from the playlists section's *Import playlist file* go to
  `import_playlist_file` (`android.md`'s *playlist file*), a notice counts the songs found
  in the library, and `Features.playlistToShow` has the root view open the new playlist. A playlist's
  options menu exports M3U8, PLS or XSPF through `export_playlist` into a `fileExporter`
  (`PlaylistFile`).
- **The queue is saved as a playlist** from the Queue tab's toolbar (`createPlaylist` with every
  queue row, repeats kept).
- **Statistics is a page of its own** (`StatisticsView.swift`), a row beside Favourites, All songs and
  Suggestions on the Library tab as well as in Library tools. Under the period picker: tiles
  (`StatisticsTiles`: time listened and plays, the daily average over the period or, for all time,
  since the first counted play, songs heard of the library, the busiest day, the library's size, albums
  and artists); a bar chart of listening (`ListeningChart`, drawn in SwiftUI: days for a week or month,
  weeks for a year, months for all time, the day cut at the phone's offset) that a tap or a drag
  selects a bar of to read its time and plays; most listened songs, albums and artists, five each
  and *Show more* for twenty-five (albums open `Destination.album(key)`, artists
  `Destination.artist(name)`); and the library in shares (`library_breakdown`: formats, sample
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
  *Edit tags* in a song's menu where `tags_writable` says its kind can be written, *Edit album tags*
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
  on `Work.aside`: counts and the first fifty moves), and *Organise*, confirmed. `Folders.file` moves
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
  (`setAlternateIconName`, `nil` for Resonate); a refusal is said under the swatches.

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
  green/teal, High-resolution green/blue, the top artist's mix its named accents). Covers are
  `pictured_by(filter)`, read for every mix in one `Work.aside` keyed by
  `Library.collectionGeneration`: the albums the filter's first rows fall on in its own order, those
  holding a picture alone, at most four. Pressing the art opens the filtered Songs list; the round
  play button and the context menu (Play, Shuffle, Show songs) play its first page at once.
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
  a third of the cover is vivid; a cover less than a sixteenth vivid (black, grey, white) is
  clustered whole and keeps its own greys, its lightness held from 0.16 (`Palette.darkestGrey`) so a
  black cover gives a near-black field; only a song without a cover takes the palette built around
  the accent colour. Each colour is held to
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
- **The app background** is the same view with no track and an `ambience` strength
  (`BackdropView(track: nil, moving:, ambience:)`), as the `containerBackground(for: .navigation)` of every page (a clear one is drawn as the system's own, so a single view behind the `TabView` never shows; only pages in a window draw, and the clock is the wall clock so a pushed page starts in phase): the palette
  built around the accent colour, set synchronously in the view's `init` so a page or tab opens on
  its colour with no fade (re-sampled when the accent changes), `BackdropTone.ambient`
  (dark floor 0.05 with a range of 0.10 to 0.32, light floor 0.96 down to 0.80, chroma and range
  scaled by the strength, so system labels keep their contrast), 30 fps, and no hold on the
  visualizer tap or loudness. `ThemedPage` on each stack's root and every `DestinationView` hides
  the scroll content background and sets that background; Now
  Playing's cover freezes the app field to a still frame. `AppearanceState` keeps three settings
  in the `appearance` defaults, edited in Settings › Appearance › Background (`BackgroundView`, `Destination.background`, a page of its own beside `AppearanceView`): `themedBackground`
  (on), `movingBackground` (on; off draws the still frame), `backgroundStrength` (0.4). A plain list's rows
  draw their own opaque fill, so each one is `themedRows()` (a clear `listRowBackground` while the
  background is on), keeping separators; a new plain list does the same. A grouped page (`ThemedList`, `ThemedForm`, which
  wrap `List` and `Form`) frosts its cards instead, `themedCards()` filling each with
  `.regularMaterial` so the field shows through; sheets keep the plain `List` and `Form`. Pages with
  their own backdrop cover it.
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
  512, 768 or 1024 px; an entry is keyed by song (or portrait), art revision, generation and bucket.
  `CGImageSourceCreateThumbnailAtIndex` decodes on `Work.aside` with the bucket as its largest
  edge, at most 4 at once (`DecodeSlots`, an actor); requests for one key join one task, a view
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
  frame before it is measured) keeps 64 MiB. A circular `ArtworkView`
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
  held, each row leads with a placeholder tile, keeps title and artist · album to a line each and
  ends in a Download icon (Remove is its swipe), a held row reads "In your library", and pulling to
  refresh reads the matches again. Online search's song rows enqueue one song.
- **Wants are asked on their own while the scene is active** (`DownloadQueue.askForWants`): the loop
  reads `next_wanted_due`, sleeps until then held to 30 s at least and 30 min at most (the
  desktop's `RETRIES_ASKED_AT_LEAST` and `ASKED_EVERY`), then enqueues `poll_wanted_due`'s songs into
  the remembered folder, only while online, with a ready account and a remembered writable folder:
  it never raises the folder sheet or the provider notice. A song it asked is not asked again on its
  own for 30 min, so a provider that stays away cannot pile failed rows up. It stops with nothing
  due, going inactive or the switch off, and restarts on any change of wants, accounts, providers,
  folder or online. "Download saved songs automatically" on Saved for later
  (`download-wants-automatically` in `library-features`, on by default) is the switch.
- **The journal is a list**: `unfinished-download` holds the URIs of every job's document still
  being written, removed one by one as each lands or is deleted; on the next start each is deleted,
  and a single string left by an older build is read as a list of one.
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
  one. A found song is a card (cover decoded on `Work.aside`, title, artist, album · year, the
  naming service) with *Find it* (the Search tab, Your library, artist and title typed), *Open*
  where the service gave a link and *Listen again*; the last eight stay as *Heard earlier* for the
  run, and a new song is felt (`sensoryFeedback(.success)`).
- **`Microphone.record`** asks `AVAudioApplication` for the microphone (`NSMicrophoneUsageDescription`),
  switches the session to `.playAndRecord`, `.measurement`, through `NowPlaying.beginListening`,
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

## Checks

`python3 ios/build-native.py` then `cd ios && xtool dev build --ipa` must produce
`ios/xtool/Resonate.ipa` with no Swift warning (the package builds in Swift 6 mode, so a data race is
an error); with a
device attached, `xtool dev run` installs and launches it (the `run-ios` skill).
