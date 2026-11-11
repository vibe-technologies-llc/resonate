---
paths:
  - "android/**"
  - "crates/resonate-mobile/**"
  - "crates/resonate-bindgen/**"
---

# Android

The Android front end is Jetpack Compose with Material 3 (`material3` 1.5, expressive components),
in the Gradle project under `android/`. Android Studio opens `android/` and runs the shared
`Resonate Android` configuration in `android/.run/` (or the generated `app` configuration), which
runs Gradle before deploying the default Activity, preserving app storage. Gradle builds Rust and
the generated Kotlin bindings before Kotlin. Android 13 (API 33) is the minimum. ARM64 phones and
x86_64 emulators share the debug APK; compile SDK 37 (the current Compose, Navigation 3 and
MaterialKolor releases require it), target SDK 36, AGP 9.4, Gradle 9.8, Kotlin 2.4 (built-in
Kotlin, with the Compose and serialization compiler plugins at the same version) and NDK
28.2.13676358 are pinned. Python 3, Cargo and both Rust Android targets are build prerequisites.
Host binding generation reads ELF metadata and needs no PipeWire. `versionName` is the root
`Cargo.toml`'s `version` read by Gradle, and `versionCode` is derived from it (a pre-release's
candidate digit, 9 for a final), so a release is versioned in one place.

**Every word the app shows is a resource** (`language.md`): `res/values/strings.xml`, `<plurals>` for
a count, the launcher's name `@string/app_name`; `generateLocaleConfig` and `res/resources.properties`
(`unqualifiedResLocale=en`) make the language the system's per-app setting, and `SpokenLanguages`
tells the boundary the configuration's locales as the player opens and on every configuration change
(`ResonateApplication.onConfigurationChanged`, once the engine is open). Composables read
`stringResource` and `pluralStringResource`; everything else says its words through the `Words` seam
(`Words.kt`: `string(id, args)` and `plural(id, count, args)`), `ResourceWords` over `Resources` in the
app (`LibraryRepository.words`, `words()` in a composable) and, in JVM tests, `English`, which reads
`res/values/*.xml` and formats each resource as Android does for English, so logic tested on the JVM
(`DeletionSentences`, `SyncWords`, `TagSentences` and the rest) takes a `Words` and its tests assert the
same English without Robolectric. `WordsTest` refuses a prose literal in any Kotlin source; `PROPER`
names the literals that are proper names.

`MainActivity` is a `ComponentActivity` that draws edge to edge and sets `ResonateApp`; it keeps
the `MediaController` connection (made again on start where none is, or the last one failed or was
lost) and implements `ui.Host` (folder and room-response pickers,
Downloads with its permission, notifications, sharing, opening a link) through activity-result
launchers. **ViewModels never hold an Activity**: `ResonateApplication.resonate` is one `ui.Resonate`
for the process, its host `CurrentActivity`, which forwards to the Activity attached in `onCreate` and
detached in `onDestroy` (none: a confirmation answers false), and its `Playback` reads the controller
the current Activity connected. Consent and microphone requests are asked one at a time under a
mutex, and one still open when its Activity is destroyed answers false, so none waits for ever. The
manifest's `configChanges` lists every change a Compose UI redraws for itself, so recreation is rare. The UI calls the repository and the generated bindings directly: there is no bridge,
so the compiler holds the contract. `ui.Resonate` carries the repository, `Playback` (controller
actions: play a list from a row, toggle, seek, jump, shuffle, repeat, volume) and the host, through
`LocalResonate`; `attempt` runs a native call and says its failure on the status banner, `read`
answers a `Result`. Each screen has a ViewModel (`resonateViewModel`, scoped to its navigation
entry) exposing typed `StateFlow`s; no screen reads an untyped map or names an action by string.
Kotlin retains SAF, encrypted accounts, workers, Media3 and services.
Compact and album home-screen widgets use RemoteViews with rounded,
artwork-tinted surfaces and system light/dark colors. Explicit, immutable PendingIntents send
transport actions through a short-lived Media3 controller, preserving audio focus and the
service lifecycle. Metadata and the bounded session thumbnail are kept in app preferences for
launcher refresh after process death; that fallback is paused until a live session publishes.
Widget updates deduplicate metadata, artwork and playing state instead of sending polling ticks
to the launcher; a drawing that fails (a launcher refusing the views, a cover too big for memory, the
cover decoded at most 512 px) is logged and never ends the process. No widget starts a second engine or decodes the original music file.

Each APK variant owns its native build task. Release uses Cargo's release profile, including
optimization and thin LTO, in a separate generated directory and packages ARM64 alone without
emulator binaries; it never packages
the debug variant's native libraries. Native symbols remain until binding generation finishes;
Android packaging strips the APK's libraries afterward. Release signing is supplied separately, with no signing
key or password stored in the repository: the `release` signing config exists only where Gradle
properties `resonate.signing.storeFile`, `.storePassword`, `.keyAlias` and `.keyPassword` (or the
environment variables `RESONATE_SIGNING_STORE_FILE`, `_STORE_PASSWORD`, `_KEY_ALIAS` and
`_KEY_PASSWORD`) name a keystore, and without them `assembleRelease` leaves the APK unsigned, never
signed with the debug key. `.github/workflows/release.yml` builds and verifies the signed release
APK from a version tag, the keystore and passwords coming from repository secrets. Release enables R8 and resource shrinking with
`proguard-android-optimize.txt` and `app/proguard-rules.pro`. Those rules preserve JNA and the generated `nativecore` boundary,
including names, fields, constructors and native entry points used through JNI and reflection.
The four absent `java.awt` types referenced by JNA's desktop-only helpers have explicit warning
suppressions; no missing Android or native-boundary type is suppressed.
Keeping only Java references is insufficient: renaming `Pointer.peer` makes JNA's `initIDs`
fail at startup with `UnsatisfiedLinkError`, before the Rust player opens. Debug startup cannot
verify these rules; CI checks the release APK's DEX for the kept names (`check-release.py`), and
release validation by hand cold-starts the signed release APK through native player initialization.

The native boundary is `resonate-mobile`, shared with iOS (`mobile.md`). On Android the owned
descriptor arrives detached from Kotlin's `ParcelFileDescriptor`. Kotlin bindings are generated
under `app/build/`, never checked in or edited; only the foreign presentation flattens error
display text for Kotlin.
Core validation failures retain their specific error text across this boundary so a settings
banner can name the rejected value instead of reporting a generic domain failure.

Handwritten Kotlin, Android tests and Gradle Kotlin scripts use ktfmt's Kotlin language style:
four-space indentation, wrapped calls, trailing commas and one statement per line. Generated
UniFFI code stays outside formatting passes.

The launcher uses the same five-bar blue-to-mint mark as iOS and `packaging/resonate.svg`.
Android vector foreground and monochrome layers share its paths and use adaptive-icon safe
padding over the same dark background. The launcher supplies its own mask, including Samsung's
shape; themed icons use the monochrome layer rather than a tinted full-color tile.

## Audio and lifecycle

`MediaLibraryService` owns a Media3 adapter over the Rust player. The UI reads published state
and sends controller actions; it performs no file, decode, catalog or network work on the main
thread. The adapter
publishes metadata and transport state to notifications, media keys and lock-screen controls.
The session activity opens or reuses the main Activity when its notification is tapped. Current
artwork is decoded off the main thread and encoded into a thumbnail bounded at 384 pixels and
128 KiB; only the current queue row carries it, so repeated songs cannot multiply image payloads
across a Binder timeline. Art revision and cache generation refresh the session after enrichment.
Dismissing the app task pauses every session player and stops its foreground service. Going Home
keeps playback running; an active download may retain the engine but never keeps music playing
across task dismissal. The media notification offers Close as a session command, including on
Android 13's system controls: it stops the player, abandons audio focus and stops the started
service. A stopped transport is Media3 idle and idle players show no notification, so the card
disappears even while an Activity or another controller remains bound. Playing again restores it;
pausing keeps the card. Stop clears pending audio-focus resumption so a later focus gain cannot
restart music after Close.
Controller actions enter one ordered worker, so load/prepare/play cannot race one another. The queue and
place are saved every fifth second of polling and also when the Activity stops and when the task is
dismissed (`LibraryRepository.saveNow`, through that worker), so a process kill while paused loses
nothing. Audio focus is given up when the engine stops or pauses itself (sleep timer, end of the
queue, a failure) unless a transient loss is waiting to resume.
`RustMediaPlayer` declares `COMMAND_RELEASE`, so `release()` reaches `handleRelease` once (media3
ignores a second): the service's retain is given back, its collectors and the widgets stop, and the
receiver and focus request are removed. Only the service's own controllers, the media
notification and controllers the platform trusts (`ControllerInfo.isTrusted`) may connect
(`TrustedControllers`); a controller's add, move and remove become engine queue edits
(`QueueEdits`: an append then moves, a range as moves of the rows it passes, a removal by queue id).
A headset, Bluetooth or lock-screen play reaches a service that is gone through the manifest's
`MediaButtonReceiver`, which starts it; a `setMediaItems` that names the queue the engine already holds
(`namesTheQueueHeld`, every item's media id in order, a served song's being its uri) only jumps to
its row and place and reloads nothing, so a queue of library and served songs resumed from a headset
loses no row (`ResumedQueueTest`), and a controller's `seekTo(index, ms)` is one boundary call
(`jump_to_at`) that seeks at the rate of the row it lands on; `onPlaybackResumption` waits at most 8 s for the engine, the
queue it resumed paused (`resume_saved`) and its first snapshot, and answers that queue at its row and
position (the engine's close clears both, so a service made again in a living process never answers the
last session's queue at row 0), so the key
plays where the listener left off (checked on the emulator by force-stopping the app and pressing
`KEYCODE_MEDIA_PLAY`).
**The library is browsable** (`LibraryBrowse`, the session's `onGetLibraryRoot`, `onGetChildren`,
`onGetItem`, `onSearch`, `onGetSearchResult`): a root of Albums, Artists, Playlists and Favourites over
`albums`, `artists`, `playlists`, `album_tracks`, `artist_tracks`, `playlist_tracks` and `filtered_tracks`
(at most 500 songs a folder). A song is listed as `<id>@<folder id>`, so one the car picks arrives with
the folder it was listed in and `onSetMediaItems` expands it to that folder's songs at its place, as
the app's own tap does; a plain catalog id passes through. Every id a controller adds is read
by `BrowseIds.trackOf` (a folder song or a plain id), and every id it sets by `BrowseIds.queueSongOf`
(a served song's uri too); anything else is left out and the start index
follows the song it named (`BrowseIds.playable`), and a song no longer in its folder (a search result
listed under the root, a playlist past row 500) plays alone. A voice request (`requestMetadata.searchQuery`,
or `MEDIA_PLAY_FROM_SEARCH` on `MainActivity`) plays `search_library`'s first 100 songs. Android Auto, the
Assistant, the car-assistant and the Auto simulator connect as controllers beside the trusted ones
(`BROWSING_HOSTS`), and `automotive_app_desc.xml` declares the media app
(`BrowseIdsTest`, `LibraryBrowseTest`). Every browse row wears a cover by `content://` URI, never bytes
(a Binder payload of 500 rows would be too large): `ArtworkProvider` (exported, read-only, answering only the app itself, `BROWSING_HOSTS` and a caller
holding `MEDIA_CONTENT_CONTROL`, `ArtworkReaders`, by the calling uid, so no other installed app reads
a cover, authority
`<applicationId>.art`, `ArtworkUris`) answers `/track/<id>` with the JPEG `artworkJpeg` encodes at
`ArtworkSizes.BROWSE_EDGE` through a pipe fed off the main thread, and a folder wears the cover of the
track the catalog names for it (`AlbumHit.artworkTrack`, `ArtistHit.artworkTrack`, a playlist's first
`covers`; none, none). `BrowseStyle.ofChildren` hints Albums, Artists and Playlists as grids and every
other level as lists in each folder's extras, and `onGetLibraryRoot` answers params that say content
styles are supported and carry the root's hints. The session's button preferences
are Shuffle and Close; a car host's own preferences add Repeat (session commands that toggle the player,
rebuilt per controller when the mode moves). The notification shows five actions, so a third overflow
button pushes Close out of it (`BackgroundPlaybackTest`). Tests: `ArtworkUrisTest`, `BrowseStyleTest`. A real car or the Desktop Head Unit is not checked here.
Audio focus loss and noisy-output events pause playback; a transient loss resumes on regaining
focus if the engine was playing or loading, and a duck is left to the platform
(`setWillPauseWhenDucked(false)`). The Media3 state is invalidated only when the transport, queue
place, repeat, shuffle, volume or song moved, or the position jumped more than a second from where
the last snapshot and the time since put it (`positionJumped`: a seek or a restart within the song,
which the system session would otherwise go on extrapolating past); the position is read live from
the latest snapshot, so *previous* past 3 s restarts the song and the notification's bar moves, and its playlist is
the window of the queue the repository read (`QueueRead`: `first`, `total` and the rows), its items
built off the main thread as each read lands and only the playing row's cover set on the main
thread, so a controller's index is the window's and `RustMediaPlayer` adds `first` back for a seek,
an add, a move or a jump onto the rows the controller handed back. An Activity's recreation does not own or close the
audio engine. Playback and download services each retain the repository; the last
release cancels its workers and saves and closes the engine off the main thread, then clears the
published snapshot, so a service made afterwards never draws the playing state the closed engine last
had (`PlaybackLifecycleTest`). A later service
waits for that cleanup before opening another. Closing replaces the engine's deferred with a fresh one,
so nothing awaits a closed native player; a native library that will not load fails it with an
`IllegalStateException` and the repository's scope logs anything else that escapes. A seek within
the current song only seeks, and a seek to another row compares against the place read on the main
thread when it was asked. `refresh()` reads on the IO dispatcher whoever calls it. The snapshot's `waiting` (`mobile.md`) becomes the status when it begins and is cleared when it
ends, if still shown; the expanded player shows the same timed, dismissible notice as every page,
with the full message wrapped above the bottom edge. It never replaces the format label between
the elapsed and remaining times, where a waiting sentence was clipped and stayed until the route
returned. Dismissing or timing out a notice leaves the engine's wait and recovery alone.
`LibraryRepository.place` is the position the poll last read with the time it was read at; the player's position (`PlayerViewModel.position`: scrubber, mini-player progress) runs from it every 50 ms while the transport plays, clamped to the duration, and is corrected by the next poll at a seek, a skip or a pause. The scrubber's draft and drag are keyed on the playing queue row and a release commits only for the song it began on.
The poll runs at the pace `PollPace` gives: 250 ms while the transport plays or loads, an analysis, a
download or a scan runs, or for two seconds after any `request` (so a press is answered at once);
once a second at rest in the foreground; every two seconds in the background. A request, a
foreground change and the boundary's `watch_player` (registered once the engine opens, told of a track
started or finished, an output change, a failure or a wait) wake it early, as on iOS. The poll catches a failing tick (a snapshot, queue read, save or listening count refused by
a full disk) and says it once per distinct message, so one error never ends the loop; a loop that
ends for any other reason clears `started`, so the next retain reopens the engine. The queue is read
off the poll and off `access`, by a reader of its own (`readQueues`, sent each tick's `QueueAsk` on a
conflated channel), so pause, seek and skip never wait behind it, and as a window (`queue_window`)
of `QueueShown`'s 50 rows heard and 200 ahead around the playing row, widened 200 at a time by the
queue sheet's *N earlier songs* and *N more songs* rows (`widenQueue`), as iOS's `QueueParts` shows
it; a whole queue is read only when asked (*Save queue as playlist*, `wholeQueue`). It is read
again only when the zone, `Snapshot.queue_revision`, the playing row or the window moved, or
`catalog_revision` moved and three seconds have passed since the last read (`QueueReads`, as iOS's
`catalogRereadAfter`), since every enrichment step, scan upsert and favourite moves the catalog.
**Nothing reaches a closed native object** (`OpenEngine`): every use of the engine holds it
(`OpenEngine.run` for a queued action, under `access`; `LibraryRepository.holding` for the poll, the
queue reader, a page walk, the lyric clock and every feature, tool and page), and the close takes
`access`, marks the engine closing inside it, shuts it down and asks `closed`, which closes the
native object at once where nobody holds it and otherwise when the last holder lets go. A holding
asked once the engine is closing throws `EngineClosed`, a `CancellationException`, so a late read
ends quietly instead of reporting a failure, and a queued action that lost the race is dropped.
Native shutdown stops sync, then the enrichment pass
(leaving its mark to carry on), analysis, downloads and the shared online client before releasing
playback; a failed final listen or save is answered after the player has stopped, never in its place. Long analysis, enrichment and HTTP work run on
separate workers without holding the controller lock; request generations refuse stale results.

**The route's formats.** By default Oboe opens a shared stereo float stream at 48 kHz in
`PerformanceMode::PowerSaving` with `ContentType::Music` (the engine's ring is the buffer, so low
latency buys nothing and costs power); the engine converts into it. *Match the song's sample rate*
on the output page (`follow-song-rate` in the `playback` preferences, iOS's name; **off by default
here**, since the only formats it adds are unmixed ones and an exclusive stream runs in low-latency
mode) has the surveyor offer, beside that mixer format, what the route takes unmixed, so the engine's
plan (`prefer_bit_perfect`, `best_spec_for`) picks the song's own rate and a word that holds it whole
(a 24-bit song in a 32-bit word is `Repacked`) where one is offered, and resamples into the mixer
where none is: **the bit-perfect mixer** (Android 14+, USB): `OutputWatch` reads the route's
`AudioManager.getSupportedMixerAttributes` of `MIXER_BEHAVIOR_BIT_PERFECT` (`bitPerfectFormats`, PCM
16, 32 and float, one or two channels) on every route change and hands them over with the route's
device id (`output_offers_changed`; used only while the system default or that device is chosen);
**the exclusive stream**: the surveyor probes the chosen device once per route with a callback-less
exclusive, low-latency stereo stream asking nothing else and no conversion, and offers its native
spec where Android answered exclusive. A stream opens by its `Way`: bit-perfect asks the
`OutputMixer` (`BitPerfectMixer`: `setPreferredMixerAttributes` for media on that device with the
matching attributes, under the install-time `MODIFY_AUDIO_SETTINGS`) and then opens shared, power-saving and unconverted on that device, falling to
mixed where Android declines; exclusive first opens a callback-less probe at exactly the spec (the
callback owns the engine's source, so a refused open could not be retried) and opens for real only
where it came back exclusive in that spec, else marks the route's exclusive offer refused (offered
no longer, announced, not probed again) and opens mixed; mixed opens shared and lets Oboe convert
(`SampleRateConversionQuality::Best`), and gives back any bit-perfect preference
(`clearPreferredMixerAttributes`), as the stream that held it closes (`Routing::closed`, only for the
stream still current, so a late close of an earlier stream leaves the next one's preference) and as
the backend shuts down. The callback is typed by the spec (`f32`, `i16`, `i32`, mono or
stereo); 24-bit packed has no Oboe-rs type, so it is never offered. `output_stream()` says what is
open (`OutputStream`), which the page shows as *Current stream* (`outputStreamWords`,
`outputPathWords`) under *This output takes* (the route's `AudioDeviceInfo` rates, depths and channel
counts, `outputTakes`); the switch and an offer that moved send `Command::FollowTheFormats`, so the
playing song reopens at once where its plan moves, and every later song opens at its own rate. The
emulator offers no exclusive stream and no bit-perfect mixer, so it proves the mixed fallback alone
(`TESTING.md`). DoP is not offered (`FEATURES.md`). The output thread waits on its commands as
`output/waits.rs` says: 10 ms while draining, 100 ms while streaming (the latency is read again
each time; the callback stamps each fill from Oboe's own `getTimestamp(CLOCK_MONOTONIC)` and
`getFramesWritten` instead, `realtime.md`) and 500 ms otherwise, in which a device error is still noticed. An open waits at most
`OPEN_WITHIN` (3 s) for the stream thread, so a stalled `open_stream` on a route change is a
timed-out open the engine parks, not a blocked engine thread, and the late stream closes itself
(`an_open_that_never_answers_times_out_instead_of_blocking`).
The output pane lists `AudioManager` devices and sends a chosen device id to Oboe before
rebinding; system default is id zero. **One output is offered a route** (`oneForEachRoute`, `offeredOutputs`), on this pane, in *Add a zone* and when a kept route is bound again: Android lists a USB DAC as a device and a headset, or as several PCM devices, and a Bluetooth headset once a profile, and all of them are one `OutputRoute`. Of such twins the call profile and the safe speaker come last, then the most rates wins. What is playing is still read off the whole list, since a stream can stand on a twin not offered, and a row stands for its twins (`standsFor`).
The pane shows the actual route in a status card and names a different requested route when one
is pending (`pendingOutput`); asking for the system default is never pending, since Android then
reports the device it chose. Android owns the physical route, and this is no promise of a DAC's rate or
bit-perfect delivery unless the current stream says so. Buffer length is validated from 50 to 2000 ms at the native boundary.
The native callback is `output/oboe_render.rs` over `oboe_frames.rs`, under the realtime lint set: preallocated bytes only,
no allocations, locks, filesystem, JNI or Kotlin calls. A short fill is padded with PCM silence.
Drain waits for the output latency after the ring ends before reporting completion. Device errors
are published from a worker through the existing stream events.

NDK libc++ is packaged beside the Rust library. Native linking rejects undefined symbols and
sets a 16 KB maximum page size; APK native libraries are uncompressed and aligned by AGP.
The emulator's native playback test must load and play audio, not merely install the APK.

With shuffle on, a song pressed in the Search tab's library results plays the whole catalog (`Playback.playFromSearch`) from that song, shuffle sent after the load (the load resumes with shuffle off) and so left on, the results themselves where the song is not in the catalog; with shuffle off the results are the queue.

## Documents and catalog

The user chooses folders through the Storage Access Framework or enables Downloads through
MediaStore. Persistable read/write grants survive restart; neither all-files access nor guessed
filesystem paths are used. Downloads asks for `READ_MEDIA_AUDIO` only when reading existing
music is requested. Its scan queries audio rows beneath `Download/` on the primary volume,
excluding pending and trashed files; it does not filter by `IS_MUSIC`, which misses unclassified
downloads. All MediaStore entries use their Files URI, including new deliveries, so rescanning
the same file preserves its catalog identity and favourites. Embedded artwork remains available;
the music permission does not grant access to separate picture or lyric sidecars.
The `content` media provider preserves the document URI's escaping exactly, including `%3A`,
`%2F` and `%25`, through a saved `MediaLocation`. Kotlin opens a descriptor on a worker; Rust
uses the same codec and transport as Linux. A forward-only document uses the codec's spool,
with its writable folder passed explicitly through `Sources::spooling_under`, never by mutating
the process environment.

The document source has its own SQLite catalog, `documents.sqlite` in app-private storage;
it does not put invented paths in the Linux catalog. Its schema is `V1` and a `MIGRATIONS` chain after it,
`user_version` the number of steps laid, all run in one immediate transaction (a fresh file lays them
all, a 0.1.0 file the steps after `V1`); there is no copy kept before one. A later change appends a
step and never edits `V1`; step 2 gave `listens` the `origin_device` and `origin_id` a remote listen
is credited by, unique together, and step 3 rebuilt `listens` with `AUTOINCREMENT`, as the desktop's is, so a deleted
song's listen ids are never handed to the next listen (the ledger and scrobbling read `id > mark`), seeding the
sequence past the highest `submissions.through`; step 4 gave `documents` an `added` (Unix seconds) that the `documents_dated` trigger sets on every insert, which the search index reads for `added:`; step 5 dates the rows step 4 found undated by their file's modification time (`modified` in ms, capped at now) and, for one never stamped, by the migration's own time, so no row is undated; step 6 gave `studies` an `agreement` (what `resonate_library::agreement` said of the audio against the song's name) that the `documents_forget_what_was_heard` trigger clears when a document's `size` or `modified` moves; step 7 made `playlist_queries` (`playlist` keyed to `playlists(id)` with `ON DELETE CASCADE`, `text`, `sort`, `reading`, `max_rows`: the desktop's columns and codes), the saved search a playlist fills itself from; step 8 made `unstudied` (`track` keyed to `documents(id)`, `under` the `JUDGED_UNDER` that failed it), the songs the enrichment pass could not decode, which the `documents_forget_what_would_not_decode` trigger clears when a document's `size` or `modified` moves; step 9 gave `studies` a `band_levels` (the 29 bytes `eq.md` describes, written by every study the pass and the analysis pane keep, and measured alone by the pass for a study kept before this step, `eq.md`) that the `documents_forget_where_the_music_sounded` trigger clears when a document's `size` or `modified` moves. Step 10 (`ANSWERS_TRAVEL`) gave `documents` the `packets` and `frames` that say which file a row is (cleared by the `documents_forget_which_file_they_were` trigger when `size` or `modified` moves) with the `documents_by_length` index, and `studies` the `studied`, `studied_under` and figures a study shared with another device carries (`mobile.md`'s *Answers from other devices*). Step 11 (`SONGS_RECEIVED_ARE_HELD_BY_WHAT_WAS_ASKED`) made `songs_received` (`recording` primary, `track` keyed to `documents(id)`), the recording keys (`Recording::key` and `named_key`) a landed song was fetched for. Step 12 (`DOCUMENTS_REMEMBER_WHEN_THEY_WERE_DOWNLOADED`) gave `documents` a `downloaded` (Unix seconds) that `index_download` sets on a song a download landed (search, wants, a playlist's missing songs; never a song from another device), which the enrichment pass reads to look those songs up first. Step 13 (`SONGS_OF_A_WITHDRAWN_FOLDER_STAY_HELD`) gave `documents` a `withdrawn` flag that `hide_songs_under` sets beside `missing` (a folder the listener removed, apart from a file found gone), which the `documents_return_from_a_withdrawn_folder` trigger clears whenever `missing` goes back to 0 (a scan's upsert, a restore). Step 14 (`SONGS_WAITING_KNOW_THEIR_FILE`) gave `received` the `packets`, `frames` and `rate` of the waiting file and made `received_also` (`received` keyed to `received(id)` with `ON DELETE CASCADE`, `recording`), the keys of a second spelling folded into a waiting song. Step 15 (`STUDIES_KEEP_THEIR_PRINT_AND_WHAT_IT_WAS_HEARD_AS`) gave `studies` the `print`, `print_length`, `recognised` and `heard_as`, `heard_score`, `heard_title`, `heard_artist` a study's print and recognition travel as, which the `documents_forget_what_was_printed` trigger clears when a document's `size` or `modified` moves (`mobile.md`'s *Answers from other devices*). Step 16 (`STUDIES_KEEP_WHERE_THEIR_PEAKS_GATHER`) gave `studies` a `band_peaks` (the figures the preamp fit bounds from, `eq.md`) that the `documents_forget_where_the_peaks_gathered` trigger clears when a document's `size` or `modified` moves. Step 17 (`PEAKS_KEPT_AS_ONE_MOMENT_ARE_MEASURED_AGAIN`) clears the 58-byte peaks of the layout that kept each band's peak as one moment, so the pass measures them again as the `BAND_PEAKS_BYTES` of up to eight moments of the band tree (`analysis.md`). Step 18 (`ZONES_KEEP_THEIR_OWN_QUEUES`) made `zones`, `zone_queue` and `zone_resume`, each zone's queue beside the first zone's `queue` and `resume` (`mobile.md`'s *Zones*). Step 19 (`ZONES_KEEP_THEIR_VOLUME`) gave `zones` a `volume`, each zone's own level (`mobile.md`'s *Zones*). Step 20 (`QUEUES_KEEP_SONGS_PLAYED_IN_PLACE`) made `served_rows` (zone, position, uri, the names the browse gave and the playing row's rate), the songs played in place a saved queue holds beside its catalog rows (`mobile.md`'s *Services played in place*). Step 21 (`LISTENS_NAME_THE_SERVED_SONG_THEY_HEARD`) gave `listens` a `served` (the uri of a song played in place, its `track` NULL) so statistics count it. Step 22 (`ZONES_KEEP_THEIR_RENDERER`) gave `zones` a `renderer` (the node) and `renderer_name`, the network renderer a zone is kept on (`upnp.md`'s *On the phones*). Step 23 (`SONGS_DELETED_HERE_ARE_REMEMBERED`) made `songs_deleted` (`recording` primary, `deleted` in nanoseconds), the keys of a song the listener deleted from the app, which `forget_deleted_songs` writes and the sync reads (`sync.md`). Step 24 (`PLAYLISTS_REMEMBER_THEIR_LINK`) made `playlist_links` (`playlist` primary, the link, `kept`, `refreshed`) with the `playlists_born_again_hold_no_link` trigger, the link a playlist was imported from (`mobile.md`'s *A playlist kept in step with its link*). Step 26 (`ALBUMS_KEEP_THE_SONGS_OF_THEIR_RELEASE`) made `release_songs`, the songs of the release an album landed on (`mobile.md`'s *The songs an album lacks*). Step 29 (`ARTISTS_KEEP_THEIR_DISCOGRAPHY`) made `artist_releases`, `artist_discographies`, `group_songs` and `groups_read`, the discography an artist's page greys in (`mobile.md`'s *The releases and songs an artist lacks*). Step 30 (`UNHELD_RELEASES_KEEP_THEIR_SLEEVE`) made `unheld_covers`, the sleeves of the releases it greys (`mobile.md`). Step 31 (`DOCUMENTS_ARE_FOUND_BY_THEIR_ISRC`) indexes `documents(isrc)` for a song a service names by its code (`mobile.md`'s *A Spotify account on a phone*). Step 32 (`A_YEAR_OVER_IS_KEPT_AS_IT_WAS_HEARD`) made `wrapped_years`, a finished year's Wrapped kept as it was heard (`library.md`, `mobile.md`). A version past the chain is `Error::CatalogVersion`. `V1` holds `documents` (the metadata, the lookup's clocks `asked`/`answered`
as nanosecond stamps with `asks` and `refusals`, `release_group`, `isrc` kept from tags on index,
`album_key`, `art_changed`, `missing`, and `size`, `modified` and `indexed_by`, the stamp a scan saw
the file by and the `INDEXING_REVISION` that read it), `resume`, `queue`, `lyrics`, `listens`
(`elsewhere` marks another device's listen), `studies` (summaries), `playlists` (`created` is 0 for
none, else a nanosecond stamp that undo restores) and `playlist_rows`, `submissions` (independent
successful-submission positions for ListenBrainz and Last.fm), `wanted` (saved recordings, separate
from playable documents, with `tried`, `misses`, `offered_at` and the `release`, `release_group`,
`artist_id`, `disc` and `position` a want downloaded from the Wanted list is asked with),
`received` (the songs another device sent that wait for a document), `album_art` (one release,
group, `picture` and cover per album key, with its own clocks and `cover_asked`), `artists` (a kept
profile and portrait per artist id, with its clocks and `portrait_asked`), the one-row `enrichment`
mark, `refused_offers` (identity, `taken_from`, `refused` stamp), `loves_told` (the loves told per
listening service) and `pictures` (id, a crc, the bytes), each picture kept once however many songs
and albums show it: `artwork` and `album_art` hold a `picture` id, not bytes (`kept_picture` finds
a picture by crc and then bytes, so a crc collision costs a comparison and never a wrong picture;
`unreferenced_picture_dropped` removes one nothing shows). `artwork_picture(id)` answers the picture
the song shows, its own or its album's, whichever `betters` says (the pair of ids decided once and
remembered, `Catalog::better_of`), `picture(id)` its bytes, and `keep_folder_cover(ids, bytes)` keeps
one folder cover for every song of a folder in one transaction. The indexes are those the scans and
imports look songs up by: a partial one on the hidden rows, and on `folded(name)`, `folded(title)`
and `recording`. The connection runs WAL with `synchronous = NORMAL`, as `resonate-library` does: a
scan's autocommits no longer sync the WAL to flash each, and a power loss costs the last
transactions, never the file. The album key is `folded(album) || char(31) || folded(album artist, else
artist)`, empty for a song without an album: the apps' grouping, folded. Triggers keep it on every
insert and on any write of `album`, `artist` or `album_artist`, and stamp `art_changed` with the
next catalog-wide number whenever `art_revision` moves; both call the connection's `folded`, which
`Catalog::open` registers before the schema runs, so a write from a connection without it fails.
Rescanning upserts by the full URI; it never deletes rows on an unavailable or revoked grant. A scan
asks `scan_document(uri, name, size, modified_ms)` (both phones, with what the folder listing already
holds: iOS's resource values, SAF's `COLUMN_SIZE` and `COLUMN_LAST_MODIFIED`, MediaStore's `SIZE`
and `DATE_MODIFIED` in seconds made milliseconds), which answers the
stored row untouched where its stamp and `INDEXING_REVISION` match and it is not hidden, so an
unchanged library is neither opened, probed nor written (the catalog revision stays, and the search
index with it) and an unknown size and time (both 0) always indexes; the folder's cover and sidecar
lyrics are still offered for the known row. A file changed from outside, a hidden row, a row with no stamp
and a bump of `INDEXING_REVISION` all index again, and `index_document` (no stamp) clears the stamp.
`take_the_place_of_missing` reads the hidden rows only where one exists (`any_missing`, a covering
partial index), so a scan is not quadratic in the library. **What it may take the place of depends on
who indexed** (`vanished::Absorbing`): a scan (`index_document`, `scan_document`) takes any hidden
row's place, a withdrawn folder's included, so a listener who removes one folder and adds another
holding the same songs keeps their favourites, playlists and history; a delivery (`indexed_download`:
a sync landing or a download) takes only the place of a row found gone, so a song landed while its
folder is withdrawn stays a row of its own and granting the folder again restores the withdrawn row
with its history (`a_song_of_a_withdrawn_folder_stays_held_and_only_a_scan_of_another_folder_takes_its_place`,
`a_folder_granted_again_restores_its_withdrawn_rows_with_their_history`).
Queries page at most 500 rows at a time. Folder traversal stops at depth 32 and admits the same
extensions as core. The queue resumes paused through the engine's `Resumption` command, so the
saved position is applied during opening rather than racing a subsequent seek. A newly prepared
paused queue uses the same command, so its selected song has opened metadata before being
published. Queue-row ids remain distinct from catalog ids: repeated songs have separate timeline
uids and shuffle/jump/save resolve the document through its raw URI. A save compares
queue revision and place: an unchanged save writes nothing, a moved place updates the singleton
row alone, and only a changed queue rewrites its entries.

The queue is edited in place by `queue_edit.rs`, every edit an engine queue command, so the playing
row keeps playing at its position and the revision moves for the next save. `play_next` inserts
catalog documents as `Placement::Next` (right after the playing row); `add_to_queue` appends them
(`Placement::At` the queue's length, after the rest of the loaded order, unlike the desktop's
`Queued`); both build rows as `load` does, and into an empty queue they arrive paused on its first
row through `Resumption`. `remove_from_queue` names the row by the queue snapshot's `queue_id`;
removing the playing row moves on as the engine's removal does. `move_in_queue(from, to)` takes
SwiftUI's `onMove` convention, `to` being the row it lands before counted before the move (the
queue's length for the end), turned into the engine's landing row. `clear_queue` keeps the playing
row alone (Apple Music's Clear empties Up Next), everything where nothing is current. Each edit
reads the queue's revision and is refused `QueueChanged` if it moved before the command lands; an
unknown queue id is `UnknownQueueRow`, a row or gap outside the queue `QueueRowOutOfRange`, a
closed player `Closed`. An edit waits for the engine's answer, so the next `queue()` shows it.
The snapshot's `current` carries the playing row's `queue_id`, as `queue()` does, so a front end
splits history from up next by row, a repeated song included
(`the_playing_song_names_its_queue_row_as_the_queue_does`).

## Appearance and playback tools

**Theme and accent are Settings › Appearance** (`Appearance.kt`, `AppearanceScreen.kt`): `ThemeMode`
(*System*, the default, *Light*, *Dark*), *Pitch black* (off by default; where the theme is dark it
asks MaterialKolor for its `isAmoled` scheme, black surfaces, and blacks the player's base) and an
`Accent` or none, kept as `theme`, `pitch-black` and `accent` in the `appearance` preferences and published by `AppearanceStore` (`LibraryFeatures.appearance`). With no
accent the seed is the wallpaper's (`wallpaperSeed`, `system_accent1_500`); an accent is one of the
desktop's seven (`resonate-ui`'s `RESONATE` flavour, held equal by `AppearanceTest`). The accent swatches are four to a row, spread across the page, and *Player background*'s two percentages are `CommitSlider`s with the player's 4 dp track. Either seeds a
MaterialKolor `TonalSpot` scheme (`schemeOf`), never the platform's `dynamic*ColorScheme`: One UI fills
the system surface-container roles with flat greys of its own, which greyed the mini player and the
scrolled top bars, so only the accent hue is taken from the system. **The ground is the desktop's, not the accent's**: `schemeOf` overrides MaterialKolor's neutral and neutral-variant palettes with a warm near-neutral (`WarmGround`, `WarmGroundVariant`; `Neutrals.WARM`), so every surface is a warm near-black or warm paper and the accent is left to controls, selection and text that means something, as `ui.md`'s Resonate and Resonate Light palettes are; the tone ladder is MaterialKolor's own, so the contrast bars hold as they did. A cover's own scheme keeps the seed's neutrals (`Neutrals.OF_THE_SEED`, what `rememberSchemeOf` asks for), so the player's ground is the cover's, and `ThemeContrastTest` walks both. Type is the system face with tabular figures in every style (`ResonateTypography`, `tnum`, as the desktop's `theme::ui`), so clocks, lengths and counts line up down a list; row titles are tracked at 0.15 sp and headings a little tight. `ResonateShapes` rounds `extraSmall` to 12 dp, which is what an outlined text field, a menu and a snackbar wear, so no field sets a shape of its own. `ResonateTheme` builds one scheme and one `MaterialExpressiveTheme`
whatever the choice, so changing it never rebuilds the content below; it provides `LocalDarkTheme`, which
`ArtworkTheme` reads in place of the system's, as it reads `LocalPitchBlack`, and re-applies `enableEdgeToEdge` so the system bars' icon
colours follow the chosen theme rather than the phone's. Widgets and the launcher keep following the
system.

**The shell** (`ResonateApp`): the roots are Home, Library and Search (`rootScreens`) in a
`NavigationSuiteScaffold`, a bottom bar on a phone and a rail on a wide window; Settings is a page,
opened from the gear (`SettingsAction`) in each root's header. Each root has its own Navigation 3
back stack (`Navigator`), kept across Activity recreation with the selected root: choosing the
current root pops it to its base, and Back at the base of a root other than Home returns to Home; *Find it*
and *Search online* open Search at its base (`Navigator.selectAtRoot`), the online trail rooted again by
`SearchViewModel.search`.
Home stays composed beside the selected root. A pushed page slides a sixth of the width and fades
(300 ms), and the pop reverses it; the system's back swipe (`predictivePopTransitionSpec`) slides the leaving page wholly off, opaque, over the one beneath shifting a sixth of the width, since a fading or shrinking leaving page shows the page beneath through it; each page is a `Page`, a Scaffold with a large, medium or small
flexible top bar that stays `surface` when content scrolls under it (no scrolled container colour). Above the navigation bar sit the banners and the mini-player, so they show
on every page. They overlay the pages in a transparent bottom-aligned column, so page content
continues behind the mini-player's outer padding instead of ending at an opaque strip. The footer draws the page's `surface` behind itself, clear at its top and whole from 70 % of its height down (`GROUND_WHOLE_FROM`), so a row passing under the mini-player dissolves instead of showing as a cut sliver between it and the navigation bar. The measured
banner and mini-player height supplies each `Page`'s bottom content inset and floating-action-button
clearance, so the last items can scroll above the controls. The page is edge to edge, so the window
never resizes for the keyboard: `Page` shrinks itself by the part of the IME inset that covers it and
lowers its bottom inset by as much, so a focused field low on a page scrolls above the keyboard and the
mini-player's room is not counted twice. A text field's value is Compose state (a view model's
`mutableStateOf`, read on as a `snapshotFlow` where typing drives work, or `rememberSaveable`), never a
`StateFlow` collected back into it, whose frame of lag drops keyboard edits and moves the cursor. Each banner carries its own 8 dp above it and the column none,
so with no banner shown the mini player's gap above equals the one below. The empty library's *Welcome* scrolls, so the banners
arriving on a first scan never squeeze its buttons.

**The look is quiet chrome around the covers.** Colour comes from artwork and from the accent on what acts; chrome is neutral. A `Shortcut` tile (Home's *Favourites* and *Shuffle all*, Playlists' *Favourites* and *Party*, Statistics' year in review) and a Search browse tile are `surfaceContainerHigh` with the icon in `primary`, never one tinted container per tile; an `IconTile` standing where a row's cover would (*New playlist*, *Add songs*, a device that handed off) is the same, and only a tile that says something is tinted (a song already held, `primaryContainer`). Home's mixes are `CoverCard`s like every other shelf (the title and the detail on two lines), the reason's icon a small round badge at the cover's foot; *Your listening* is three figures set on the page between hairlines, its header's *Statistics* and the figures both opening the page. `EmptyState` is a bare icon over its words, with no tile behind it; *Welcome* leads with the five-bar mark (`ResonateMark`, the launcher's paths drawn in `primary`). No icon is a sparkle or a wand: statistics is a bar chart, a mix a queue, analysis a chart under a glass, restoration a plaster, *Never heard* a struck play. A sheet opened over the player (`SheetHost`) wears the player's scheme (`Sheets.worn`, set while `NowPlaying` is composed), so the song sheet matches the queue and zones sheets beside it. Song details are a specification sheet (`DetailRow`): the label in a 36 % column, the value beside it and a 48 dp slot for *Copy* in every row so the values line up, stacked above a font scale of 1.3. Every slider wears `CommitSlider`'s 4 dp track, the playback buffer's included.

**Settings is a page of navigation rows in six groups, the phones' shared root** (`SettingsScreen`,
`SettingsCatalog.kt`): a search field, then *Sound* (Equaliser, Sound processing, Audio output),
*Library* (Music folders, Library tools, Listening statistics, Made for you, Saved for later), *Online &
services* (Online features, the root's one switch; Download sources; Server, only while serving;
Scrobbling; Spotify), *Devices* (Device sync), *Appearance* (Appearance) and *App* (Playback
notifications, with *Allow* while refused, and the version). Every row is an icon, its page's title and
a one-line status, live where there is one to tell (the equaliser's *On · <curve>* or *Off*, the output
in use, the theme and accent). Search folds accents and matches word starts across a row's title, line,
group, the settings its page holds (`Setting.inner`, the one matched named in place of the line) and the
proper names it answers to (`Setting.names`), each row's words read from the resources first (`matchingSettings(words, query)`);
the equaliser's pages (*Headphone correction*, *Measure with a microphone*, *Room correction*,
*Listening test*, *Curves for other outputs* while one is kept) are rows only search lists
(`Setting.onRoot` false, their line led by *Equaliser ·*), so *AutoEq*, *room*, *ABX* or *microphone*
opens that page directly (`SettingsCatalogTest`). `UiSmokeTest` and `A11ySweepTest` find the list and
its rows by `settings-list` and `setting-<name>`.

**Banners** are three, all `LiveRegion`s: *Reading music folder*, *Syncing with your devices* or
*Fetching songs from your devices* (progress; a fixed label with the changing count and the bar
cleared from the semantics, so a count is never read again at every step) and the notice, which
says a failure or a status: a status fades after 6 s (`StatusNotice`, as iOS's notice), a failure stays until dismissed (`LibraryRepository.statusIsFailure`), and a later message replaces either. **What the library finds by itself is silent by default**: *Tell me what the library finds by itself* in Library tools (`tell-findings` in `library-features`, off by default, `LibraryFeatures.tellsFindings`) gates the two notices the app raises with nobody asking, the songs hidden because their files are gone and the songs a sync landing kept, through `FindingNotices.shown` (`FindingNoticesTest`). Everything the listener asked for (a scan's *Read N songs*, filing, tags, deletion) and every failure is shown whatever the switch says, as is the live *Reading music folder* progress; the app starts no scan of its own. A liked song
shows *Added to favourites* on the snackbar with *Add to playlist*; the song sheet (`SongSheet`)
holds the song's actions, adding Equaliser, Audio output and Visualizer when opened from the
player, and *Remove from queue* from a queue row.

**The player** is one `PlayerExpansion` (saved across recreation, reopening expanded), a single `Animatable` the mini-player's tap or upward drag
and the system's predictive back all drive: `NowPlaying` is drawn while it is above zero, so the
mini-player and the page beneath are never two routes with two exit animations. The mini-player is
a `surfaceContainerHighest` capsule (16 dp corners) with 48 dp artwork inset 8 dp at the start and top, its corners the capsule's less the inset so the two curves are concentric (4 dp and 36 dp in a short window), the title, artist, previous, play and next and
a line of progress, inset like the artwork, that says *Song progress* with its percentage. `NowPlaying` wears an `ArtworkTheme`
of the scheme of a 96-pixel sample of the cover (`rememberSeedOf`, `rememberSchemeOf`; a cover with no colour keeps the
app's primary). **A song change never passes through the app's own colour and recomposes the player once**: a seed outlasts its cover by `SEED_OUTLASTS_ITS_COVER_MS` (600 ms), since a cover still decoding reads as none; seeds and schemes are made off the main thread and kept (`LruCache`s), and the next song's cover, seed and scheme are made ahead, so a skip wears them on its first frame; the scheme steps with the cover and the title, and the ground alone glides, three `animateColorAsState`s read only in `drawBehind` (450 ms, `GROUND_GLIDES`), since a theme animated colour by colour changes a static composition local every frame and so recomposed the whole player (`ArtworkSeedTest`). Album, artist and playlist pages, which change colour once, keep the animated theme. It shows the cover or the lyrics, and opens the queue as a sheet. Its action row is *Lyrics*, *Queue* and *Zones* (the shown zone's name where there are several), each measured at its natural button width (`PlayerActions`), so a row that fits never cuts a label and one that does not wraps; proportional weights based on separately measured text can shrink a button below its content width and ellipsize Queue; the sleep timer is *Sleep timer* in the player's song sheet (`Sheets.sleeping`) and its chip shows in the row only while a timer runs. The title opens the album and the artist the artist; sliders draw a 4 dp track (`CommitSlider`'s `trackHeight`). The cover's gradient strength is *Player background* in Settings › Appearance (`backgroundStrength`, 0.7 by default, kept in the `appearance` preferences); at 0 the player has no coloured background, and with *Pitch black* it is black. *Now Playing intensity* (`nowPlayingIntensity`, 0.5 by default, `now-playing-intensity` in the same preferences, the slider under it with *Reset to default*) darkens or enriches that colour as iOS's does (`PlayerTone.top`, in HCT: at 0.5 the colour as it was, below it the tone dimmed by up to half in the dark theme and a quarter in the light and the chroma down to 0.6, above it the tone up 12 % to at most 95 and the chroma up 50 %), applied to the gradient's top before the strength sets its alpha (`PlayerToneTest`).
Album, artist and playlist pages wear the same theme from their first cover.

**Zones are the player's *Zones* sheet** (`ZonesSheet`, `PlayerZones`, `ZonePlan`; `mobile.md`'s
*Zones*): each zone a row with a radio (tap shows it), its output and what it plays
(`ZonePlan.line`), and a menu of *Hand the song over here* (another zone, while the shown one holds a
queue), *Rename* and *Remove* (not the first, confirmed); below, *Add a zone on <output>* for the
system default and every output offered, named after the output (`ZonePlan.nameFor`
numbers a taken name). A zone is added with the output's `AudioDeviceInfo` id and its route id
(`OutputRoute.id`), which is what is kept: `PlayerZones.bindRoutes` (at `initialize` and on every
device change) finds each kept route's current device id (`ZonePlan.deviceFor`) and binds it, since
Android hands ids out afresh. The *Audio output* page binds the shown zone (`selectOutput` is
`select_zone_output` with the route), so the first zone's output is kept too. `LibraryFeatures.tick`
reads the zones when `zone` or `zones_revision` moves and every eighth tick while there are several
(`ZonePlan.due`), and `QueueReads` reads the queue again when the shown zone moves. The media session,
notification and widgets read the snapshot, so they follow the zone shown (`ZonePlanTest`,
`QueueReadsTest`).

**A zone may play on a network renderer** (`upnp.md`'s *On the phones*). The sheet's *Network
renderers* section is a *Find renderers* row with a switch (`NetworkRenderers.look`, kept as the
`renderers` preference, off by default, and applied at `initialize` by `begin`, which never turns a
search off: a zone kept on a renderer had the boundary search already); its supporting line counts
those found, read every two seconds while the switch is on and the sheet open. Each zone's menu has
*Play on a renderer* (a dialog of the renderers found, a radio on the one it plays to, each with its
host) and, on one, *Play on this phone*; the row names the renderer before the device
(`ZonePlan.heardOn`). The *Audio output* page takes its zone off a renderer before binding the device.
Kotlin holds a `WifiManager.MulticastLock` (`resonate-renderers`, apart from sync's) exactly while the
boundary searches, released at `stop`. A song played in place holds a `WifiLock` (`StreamingWifi`,
`resonate-streaming`, under `WAKE_LOCK`) while the transport plays or loads it, released at pause, stop
and `handleRelease`, so power saving with the screen off does not starve a hi-res stream
(`StreamingWifiTest`). Below the switch, *Add a renderer by address*
(`RendererAddressDialog`) takes the address of a renderer's description and lists those added, each
with *Remove*; `NetworkRenderers.add` and `forget` keep the boundary's list in the `renderers`
preferences as `renderer-addresses` (a string set) and `begin` adds it again, so a renderer
given by address is listed and offered to a zone with the switch off. **Cast receivers are in the
same section** (`cast.md`'s *On the phones*): *Find renderers* looks for them too (the same
`MulticastLock`), the dialog of renderers lists them with their host under *Google Cast ·*
(`ZonePlan.rendererLine`), and *Add a receiver by address* (the same dialog, an address or host
name) is kept by `NetworkRenderers.addReceiver` and `forgetReceiver` as `receiver-addresses` and
added again at `begin`.

**The queue sheet** lists the devices that handed off (one row), History, the playing row and Up
next, the window the repository read (`QueueView.first`, `total`; the header counts the whole queue
and a jump or move adds `first`), each end a row counting the rest (`queue_earlier`, `queue_more`)
that widens it; a handle drags a row (`Reorder`, its rows counted from Up next's first row as it stood when the
drag began, so a song ending mid-drag moves the row dragged; a drop over a list that changed meanwhile
shows the engine's order), sent as `queueLanding(from, to)` since the engine's
`move_in_queue` counts the landing row before the move, so a downward drag adds one and the end is
the queue's length. *Save queue as playlist* and *Clear* sit in its header; *Clear* asks first (history and up next go, the playing song stays), and a swipe the engine refuses puts its row back after `UNREMOVED_AFTER_MS`, as a playlist row does.

**Library** is a pager of Songs, Albums, Artists and Playlists under a tab row (scrollable past a
font scale of 1.3), *New playlist* a floating `+` on Playlists, named so to TalkBack (the heading carries no second one), **Playlists** opening on two `Shortcut` tiles, *Favourites* and *Party* (stacked above a font scale of 1.3, the same `ShortcutPair` as Home's), then *Your playlists* with *Paste a link* and a grid of `PlaylistCard`s as wide as Albums' (a shared, followed or saved-search list says so before its count; none is *No playlists yet* over the button), Songs opening on full-width *Play* and *Shuffle* with the count at the start and sort and filters at the end of one row, as Albums and Artists put their sort, the active filters on a row beneath, *More* the folder, rescan and
Library tools entries. **A sort menu lists only the orders**, in compact 48 dp rows (`MenuEntry.Choice`): choosing the order already held flips its direction, which the sort chip's arrow shows (a *Descending* switch under seven rows made the popup overrun the list and the notice). A catalog read (`catalogRead`) never emits a spinner over a result it held, so a page subscribed again
keeps its rows, and the Albums and Artists tabs keep their scroll across a reload. Favourites and the counts read the repository's whole catalog, never the
song browser's filtered page. A completed favourite write updates the catalog, the browser, the
queue, the snapshot and the open playlist in place (`setFavourite`) before the catalog is read
again. A playlist is a spinner until its rows arrive (`playlistLoading`; selecting one clears the
previous rows at once); a read that throws is `Loaded.Failed` with *Try again* (`playlistUnread`), never an
empty playlist offering *Add songs*. Likewise a catalog whose first `refresh()` failed (`LibraryRepository.unread`)
draws Home and Library as unreadable with *Try again* (`reread`), and *Welcome* only once a read has succeeded with no songs.

**Search is one page of the library and online**, as iOS's: the library's answer (songs, albums and
artists, `search_library`, at most 500, the *Songs* chip saying how many it lists, *Show more* reading on,
one page at a time behind a spinner, rows kept once by id)
then an *Online* section of the best five songs and three albums and artists, each with *See all*
(`OnlineView.Found`, a nested page with Back) and a line naming what is still awaited. Typing asks the three
online kinds in a chain after 450 ms (`SearchViewModel.askTop`, an answer per spelling kept); a link shows
the online page alone; its header stays `surface` under a scrolled list, as a `Page`'s bar does, and its actions wear the same `onSurfaceVariant`; with online off the section, and a link's page, is one card that turns it on. Each
level of the online trail keeps its own scroll, so Back returns to where that page was. The last ten queries are
kept in `library-features` as `recent-searches` (`keepRecentSearches`, `keptSearch` puts a typed one first
and never a link) unless *Remember recent searches* in Library tools is off (`remember-searches`, on by
default; turning it off clears them); a library search asks `highlights` once for every title, artist, album
and artist name it shows, waited for with the answer so rows never light late, and `lit` draws the
UTF-16 ranges emphasised over the rest.

**Artwork** is asked at the size drawn (a bitmap already decoded is drawn on the first frame, `cachedArtwork`, so a cover scrolled back into view never flashes its placeholder): `ArtworkSizes.edgeFor` picks the bucket (96, 192, 384, 512,
768 or 1024 px) holding the composable's size in pixels, and `LibraryFeatures.artworkBitmap` decodes
by *picture* and bucket into a 32 MiB `LruCache` (JPEGs for the session thumbnail in 16 MiB), so
the twelve songs of an album decode one image and a picture never goes stale. A cover redraws when
its song, art revision, the generation or the song's stamp moves (`ArtworkClock`): the stamp moves
for the songs `art_changed_since` names, so an enrichment finding one cover refetches that song's
alone, and the generation moves only where everything may have changed (a scan, an explicit
refresh). Missing art is a quiet `surfaceContainerHighest` square with a note.

**Collection pages** (album, artist, playlist, mix, favourites) share cover-led headers with the song
count and length and Play and Shuffle. Albums keep disc and track order (`album_tracks`); an artist
page lists their albums and songs, its profile read by its own `ArtistViewModel` (never the shared
`LibraryFeatures.artist` that Details reads, so another artist's page cannot lend it theirs) and each link
listed once, its album cards saying the year alone, since the page names the artist. **What the artist's discography holds that the library lacks follows, greyed** (`mobile.md`'s *The releases and songs an artist lacks*): `ArtistViewModel` asks `learn_artist_discography` and then `learn_artist_songs` until it answers 0, then `learn_artist_covers` the same way, reading the page again (`Retry`) after each batch, and the page follows the albums with *Not in your library · N releases* (`UnheldReleaseCard`, the release's sleeve (`unheldReleaseCover`, read when `ReleaseNotHeld.covered`) at half strength, a placeholder until it arrives or where none was found; a press opens `UnheldReleaseSheet`, which reads the release's songs and offers *Download N songs*, *Save N songs for later* and a row for each) and the songs with *Not in your library · N songs* (`NotHeldSongRow`, the first twenty and *Show all*, each a press to download through `Downloads.request`, its *More* the online song sheet). Play, Shuffle, the summary and the selection read the songs held alone; offline the lists kept are drawn and nothing is asked (`firstOf`, `releaseLine`). An album's rows are a tracklist:
the number, the title and the length at the trailing edge (`TrackRow`'s `note`), a second line only for a song billed to someone other than the album (a favourite's heart then sits beside the length). **The songs of its release the album lacks stand at their places, greyed** (`mobile.md`'s *The songs an album lacks*): `AlbumViewModel` reads `album_songs_not_held` with the album's songs and, once for the page it opens (`init`), asks `learn_album_songs`, reading again (`Retry`) where songs landed; `discsOf(tracks, notHeld)` lays each in by disc and position as an `AlbumRow.NotHeld` among the `AlbumRow.Held` ones (a disc holding no song yet standing in its order, and the headings shown once the release has more than one disc, `LibraryLogicTest`), and `NotHeldSongRow` is the number, the title and its length in `onSurfaceVariant`, a download button and *More* (the online song sheet: *Save for later*, *Download…*); a press downloads through `Downloads.request` as an online row does, the row says *Finding the song…* or *Downloading…* while its job stands (`DownloadPlan.standing`) and TalkBack hears *Not in your library*. Play, Shuffle, the summary and the selection read the songs held alone; offline, or for an album no strict lookup matched, the page is as it was (`NotHeldRowsTest`). A playlist cover is `MusicPlaylist.covers`: none shows an icon on `secondaryContainer`, a designed tile rather than missing art,
one to three the first song's picture, four a 2 by 2 (`PlaylistCover`). A playlist adds songs through
a searchable multi-song picker over the batched add seam, opened from an *Add songs* row aligned with the songs under Play and Shuffle, or from the button in the empty playlist's words; rename, revision-checked moves and removal,
delete and undo are the native calls. A row plays its list from the position tapped
(`Playback.play(tracks, index)`), so a repeated song selects the row pressed.

**Songs are multi-selected from every list of songs** with one helper, `Selection<K>` (`Selection.kt`):
a set of row keys, `begin`, `toggle`, `selectAll`, `retain` (rows that left the list are dropped, so a
delete or a filter never leaves a ghost), `picked(items, keyOf)` (the chosen items in the order the
list shows them) and `row(key)`, the `RowSelection` a `TrackRow` takes. A screen owns its `Selection`
(`rememberSelection`) because it must swap its own top bar; the key is the track id, except where a
song can repeat (a playlist's `PlaylistEntry.key`, the queue's `rowKey`). A long press on a row
(haptic `LongPress`) selects it, a tap toggles while any row is selected, and toggling the last row off
leaves selection; the row's trailing *More* button is hidden while selecting and is how a row's menu
stays reachable otherwise. A selected row draws a check over its cover (or number) on `secondaryContainer`
and says `selected` to TalkBack, which also gets a custom action *Select <title>*. While selecting,
`Page` (and `CollectionPage`, the Search scaffold and the queue sheet's header) show `SongSelectionBar`
in place of the title bar: *N selected* (a polite live region), *Clear selection* (Back does the
same), *Select all* (of what the list shows: the artist page's folded five, Search's songs of the
chosen scope), *Play* (the selection, in list order), *Add to playlist*, *Add to favourites* or
*Remove from favourites* (the latter when every selected song is one), and in *More* *Play next*, *Add
to queue*, the list's own removal (*Remove from playlist* on a list playlist, *Remove from queue* in
the queue; `SelectionRemoval`) and *Delete N songs* where every song's folder is writable
(`songDeletable`; confirmed by the one `DeleteSongsDialog`). Every action clears the selection. The
songs are re-read from the catalog (`findTrack`) first so the favourite state is live. The boundary has
no batched favourite or removal, so `LibraryRepository.setFavourites`, `PlaylistViewModel.removeAll`
(positions from the last, each against the revision the one before made) and `PlayerViewModel.removeAll`
loop the single native call inside one worker request; queue inserts, add-to-playlist and deletion
are the existing batches. Songs, Album, Artist, Favourites, mixes, a playlist, Search's songs and the
queue select; Home, Statistics and Identical songs do not. The Library pager clears the selection
when the page changes (`SelectionTest`).

**Lyrics** are a `LazyColumn` of `LyricLine`s following the native `LyricFrame` (`mobile.md`'s
*Lyrics*; the frame is computed only while a lyrics pane is open, `watchLyrics`). The active line is
full size and ink, the rest 0.94 scale at 38 % alpha with the distant ones blurred, a wipe colouring
each line's words by their UTF-16 ranges, the edges faded. A drag pauses following for five seconds
and offers *Follow lyrics*; tapping a timed line seeks. Blur is dropped where animations are off
(`ANIMATOR_DURATION_SCALE` zero) or TalkBack is exploring (`rememberCalm`). A pane with no lyrics scrolls its empty state (`NoLyrics`), since large text makes it taller than the room above the controls and an unscrolled column squeezes its last child, the button; `EmptyState` itself cannot scroll, as many callers put it in a `LazyColumn` item.

**The equaliser is one page, the curve drawn and shaped where it is drawn, the phones' shared page**
(`EqualiserScreen`, `eq.md`'s *The phones*): the boundary's parametric curve, any number of bands up to
32, through core's `Profile` and the engine's `Equalisation` fallback. The engine holds one curve at a
time; which one is the route's business (below). From the top: *Equaliser* (the switch, its line the
curve's name and *Every output* or the output keeping it, *Off · Original sound* when off) and *Only for
<output>* while the route is known (`SwitchRow`s); the graph, always shown (`EqualiserGraph` in an
`EqualiserCard`, its height 10/16 of its width held to 180 to 340 dp, `graphHeightDp`, the level labels
in a gutter of their own, `GraphFrame.gutter`, so no handle covers them); the band chips `1 … n`
(`BandChips`, a `LazyRow` keeping the chosen chip in view, faded over its last 48 dp while it scrolls on, `fadedTowardTheEnd`, so a chip cut by the edge reads as more to come) with *Add band* fixed at its end, a tonal `AssistChip` as tall as the band chips; the chosen
band's card docked under them (`BandCard`); *Preamp* (*Level*, its value, *Fit* and the note *Fit brings
the queue's loudest peak to full scale.*); *Presets* (a `LazyRow` of `FilterChip`s opening on the chosen
one, its line beneath, or *Your own sound, shaped on the graph above.*); *Corrections*, rows to the
pages `Screen.HeadphoneCorrection`, `Measurement`, `RoomCorrection`, `ListeningTest` and, only while
another output keeps a curve, `OutputCurves`, each with a line (room correction's `roomStatus`: *No
response chosen*, *On · <name>*, *Off · <name>*). *Reset curve* is the top bar's overflow menu,
confirmed (`ConfirmDialog`). Every page is a `SettingsList` of `SettingsParts.kt`'s rows, and each
sub-page has its own ViewModel scoped to its entry, so a measurement or a listening test ends when its
page is left. On the medium phone the graph and the band card fit one screen above the mini-player once
the top bar has collapsed.

**A drag that starts on a handle edits it; one that starts anywhere else scrolls the page**
(`Graph.kt`). A handle's hit area is the 48 dp square around it (`GraphFrame.held`, the nearest where
squares overlap, as the ten octave handles of a 411 dp phone do; `EqualiserScalesTest`). A press inside
one consumes the gesture and chooses the band, and a move past 2 dp places it (`GraphFrame.placed`:
sideways the frequency, up and down the gain, a gainless filter keeping its gain), shown as a draft
(`hold`) with the drawn range frozen at the press; a press anywhere else consumes nothing, so the list
scrolls, and nothing is added by a tap. Release commits; an up the gesture did not see (the synthetic
cancel, which arrives consumed), a pointer gone or the gesture's coroutine cancelled releases the draft
instead (`release`). **Every gesture has an accessibility equivalent**: each handle is a 48 dp
semantics node (`band-handle-N`, *Band N*, its type, frequency and gain) whose click chooses it and
whose custom actions *Raise the gain* and *Lower the gain* (`GAIN_STEP_DB`, half a decibel, for a
filter that carries gain) and *Raise the frequency* and *Lower the frequency* (`HERTZ_STEP_OCTAVES`, a
third of an octave) commit at once (`withNudgedGain`, `withNudgedHertz`), so TalkBack and Switch Access
reach what a drag does; the chips, *Add band* and the card's sliders are the rest of the way in.

**The band card is the exact way in** (`BandEditor.kt`, `Controls.kt`, `Entry.kt`): *Band N · <type
▾>* (a menu of the eight filters) and *Remove*, wrapping under the title where the line is too narrow,
then *Gain*, *Frequency* and *Width* as `MeasureControl`s: a label, the value as a `TextButton` and the
shared `components.CommitSlider` with its 4 dp track (`centred` for gain and the preamp, filled from
0 dB; frequency and Q on a log scale, `LogScale`), whose inactive part is `outline`, 3:1 against the
card; *Gain* is off and reads *Not used by notch* (and so on) for a filter that carries none. Pressing
a value opens `ExactEntry`: the value selected, its unit, its reach (*From -24.0 dB to +24.0 dB*), a ±
button for a signed value, since a decimal keyboard need not offer a minus, and *Set* only while the
text reads inside the bounds (`Measure.accepted`; `typedNumber` takes a sign, `dB` and either decimal
mark, `typedHertz` also `Hz`, `kHz` and `k`; `EntryTest`). *Add band* puts a peaking band at 0 dB and
Q 1 in the middle, on a log scale, of the widest gap between the bands and the 20 Hz and 20 kHz ends,
the gap nearest 1 kHz and then the lower winning a tie (`freeHertz`: 1 kHz on an empty curve, 707 Hz
beside the octave layout), appends it and chooses it; *Remove* chooses its neighbour
(`chosenAfterRemoving`). A band is always chosen while the curve shows any (`chosenBand`), so the card
stands from the first frame (`EqualiserCurvesTest`).

Selecting one of the boundary's named presets (`equaliser_presets`, handed over in `equaliserInfo`;
`eq.md`'s *The phones*) enables the equaliser and stores its name and curve through the existing
preferences; boosted presets lower the preamp to leave headroom, except Bass only: it raises the three
lowest octave bands, leaves every other band flat and the preamp at zero, so the rest is never lowered.
Editing a band or the preamp clears that preset name. An empty flat curve shows ten octave handles and
chips (31.25 Hz to 16 kHz, Q sqrt(2), `editable`), creating the bands only on a committed edit. An
AutoEq or imported profile keeps its filters, frequencies, Q and preamp; its bands are the handles as
they are, never converted to the octave layout. *Fit* beside the preamp's level sends the shown curve to
the player's `fitted_preamp` at the stream rate, through the room correction applied (`fittedPreamp`,
off the main thread) and commits the answer as a preamp edit, clearing the preset name; it is off with
no bands. Every edit of a band or the preamp (a handle, the card, an accessibility action, an added or
removed band, an exact value, *Fit*) switches the equaliser on, as a preset does (`withCurve`); only the
switch turns it off, and confirmed *Reset curve* removes every band and returns the preamp to zero
without changing that switch. An edit is shown at once as a draft over the stored curve; the draft goes
once the repository has tried it, so a rejected curve falls back to the stored one. The drawn response is
the stage's arithmetic at the stream rate (48 kHz with nothing open, so the graph never reads empty);
the native boundary validates the whole curve before sending any commands, and sends only fields
that changed. ReplayGain, true-peak clip
prevention, resampling quality/phase and experimental lossy restoration use existing engine
commands, never a platform equaliser running beside the Rust chain.

Sound settings live in app-private `playback` SharedPreferences with named enum values, separate
from the document schema; the curve is EqualizerAPO text under `equaliser-curve`. Unknown enum values fall back to the native defaults;
an unreadable curve reads flat. The ordered repository worker restores these preferences before
allowing playback actions, sends edits and commits each completed gesture off the main thread.
Sliders keep an in-progress value locally and commit on release, avoiding a file write or stream
rebuild for each motion event.

**A curve may be kept for one output, as iOS's** (`OutputRoutes.kt`, `OutputCurves.kt`). The route in
use is `OutputRoute(id, name)`: the output the Oboe stream opened on (`output_device`), else the one
`AudioManager.getAudioDevicesForAttributes` says media would take (so the right curve is read before
the first song opens and while paused), found among `audioOutputs` by device id. The id is versioned
(`v1:<kind>:<which>`) and never holds an `AudioDeviceInfo.id`, which Android hands out afresh: `kind` is
speaker (both speaker types), earpiece, wired headphones, wired headset, bluetooth (A2DP, SCO, BLE, hearing aid, so
a headset on either profile is one route), usb or hdmi, and `which` is the Bluetooth address, else the
product name (a USB card number moves between plugs; neither needs a permission). The name shown is
the output page's title (`outputName`). `OutputWatch` (started by `LibraryFeatures.initialize`, stopped
by `stop`) registers an `AudioDeviceCallback` and is told the stream's device every tick; each change
reaches `LibraryRepository.outputRouteChanged` on the ordered worker, which sends the bound curve, else
the shared one, where it differs from the one playing (`followRoute`), writing nothing.
`PlaybackPreferencesStore` keeps a bound curve in `playback` beside the shared one:
`equaliser-curve-for.<id>` (EqualizerAPO text), `equaliser-curve-name-for.<id>` and
`equaliser-route-names.<id>`. While the output in use is bound every edit (graph, band card, AutoEq,
import, measurement, reset) writes its curve and the other choices still go to the shared keys
(`write(preferences, route)`); an unreadable bound curve reads as the shared one. *Only for <output>*
on the equaliser page binds the curve playing to that output or unbinds it, falling back to the shared
curve at once; *Curves for other outputs* (`OutputCurvesScreen`, a Corrections row shown only while
another output keeps one) lists the rest, each *Forget* confirmed and told in a notice (Android's idiom, where iOS offers Undo). The equaliser switch and
room correction stay one for every output, as on the desktop and iOS
(`OutputRoutesTest`, `OutputCurvesTest`, `PlaybackPreferencesTest`).

**A measurement of the listener's own is fitted on *Measure with a microphone*** (`MeasurementScreen`,
its own `MeasurementViewModel`, `MeasurementAim.kt`): *Fit toward* is a row opening a `ChoiceDialog`
of the boundary's built-in targets (`measurement_offers`, the recommended one chosen first) and *A file
of your own…* (`TargetChoice.File`), which opens `ActivityResultContracts.OpenDocument` and reads the
target through `ContentResolver.readUpTo` (`DocumentReads.kt`, at most the offers' `bytes_at_most` and
one byte, so the boundary's refusal speaks for a larger one); *A measurement file* holds *Fit a
measurement…*, which opens the picker for the measurement itself. `MeasurementViewModel.fitFrom` reads
on `Dispatchers.IO`, fits on `Dispatchers.Default` through `fit_measurement`, then commits the fitted
curve as an import does (it switches the equaliser on, binds to the output in use) and says
`fitSentence`: the bands, the target, the error before and after and any lines passed over, kept at the
page's foot under a progress bar while it runs. `MeasurementView` holds the offers, the aim
(`Aim.BuiltIn` or `Aim.Document`), whether a fit is running and the last sentence; a second fit is
refused while one runs, and a refusal reaches the status line as the boundary's sentence
(`MeasurementAimTest`). The page draws every row before the offers are read (*Reading the targets…*,
the buttons off), so nothing shifts when they arrive.

**The room is measured from the same page** (*The microphone*'s *Measure*, `MeasurementViewModel.measure`,
`SweepRecorder.kt`; `eq.md`'s *A measurement of your own*): it asks `Host.allowMicrophone` (refused:
the section says so and offers *Open Settings*, `MeasurementView.denied`), pauses a playing engine
(`LibraryRepository.pauseForRecording`, which Listen shares), asks `measurement_rate_for` of
`AudioTrack.getNativeOutputSampleRate` and `measurement_sweep` of that, and `SweepRecorder.measure`
plays the sweep through a short-lived `AudioTrack` of its own (`MODE_STATIC`, float mono, media
usage) while an `AudioRecord` (`UNPROCESSED` where `PROPERTY_SUPPORT_AUDIO_SOURCE_UNPROCESSED` says
the phone has it, else `MIC`; float mono at the same rate) fills `recorded_frames` on the IO
dispatcher, the sweep never passing the engine and so the equaliser and room correction in force.
Both are released in `finally`, so *Stop* (which cancels the job; the read loop checks every tenth of
a second), leaving the page (`onCleared` cancels it too) and a failure leave nothing open. The recording goes to `fit_recorded_sweep` on
`Dispatchers.Default` with the aim and the optional *Microphone calibration* file (a row that
chooses one, with a button that forgets it; `Calibration`, kept in `MeasurementView` until forgotten), and the fitted curve is committed as a
fitted measurement is, named `Measured`, with `fitSentence`. A second *Measure* is refused while one
runs (`MeasurementView.sounding`, `MeasurementAimTest`).

**Room correction is a page of its own** (`RoomCorrectionScreen`, `RoomCorrectionViewModel`,
`RoomCorrectionState.kt`): *Correct for the room* (a `SwitchRow`, off until a response is kept, its line
the response's name and `describeRoom`), then *The response*: *Choose a response* opens the system document picker for audio (`ACTION_OPEN_DOCUMENT`), the chosen
file is read in `room_correction_chunk_bytes()` chunks off the main thread and staged through
`begin_room_correction`, `stage_room_correction` and `finish_room_correction` (a failure calls
`abandon_room_correction`), never held whole, then switched on; the switch, the response's channels, rate and length and
*Forget the response* (confirmed) follow, as `eq.md`'s *Room correction* says. `room-correction` (on)
and `room-correction-name` live in the `playback` preferences, as iOS's suite; the kept response is
applied at start (`initialize`), a missing file turning correction off with a notice. *Choose a
pair* opens `ActivityResultContracts.OpenDocument` of audio for the left speaker's file, kept in
`RoomView.left` (`RoomHalf`: its URI and display name) while the page shows it with *Choose the
right speaker's response* and *Cancel*; the right file chosen, both are read whole, up to 64 MiB and one byte, and
kept through `keep_binaural_room_correction` (`RoomCorrectionState.chooseRight`, `eq.md`'s *Crossfeed
and binaural responses*; `RoomPairNamesTest`). Choosing,
applying, forgetting and restoring run one at a time under one mutex. **The page ends in *The
speakers*, shown only once a response is kept**: the speaker angle, elevation and the head turned
(`SpeakerSetting`, a `SettingRow` with its reading and steppers, `RoomView.angle`, `elevation` and `yaw`,
bounds from `speaker_limits()`), each with the desktop's chips beneath (`ANGLE_PRESETS`,
`ELEVATION_PRESETS`, `YAW_PRESETS`), sent as
`set_speaker_angle`, `set_speaker_elevation` and `set_head_yaw` (a set that will not read there
leaves the trio standing and the failure on the banner) and kept in `playback` as `speaker-angle`,
`speaker-elevation` and `head-yaw`, sent at start before the kept response is read (`eq.md`).

**The listening test tests a setting as well as the equaliser** (`ListeningTestScreen`, its own
`ListeningTestViewModel`, `ListeningTestView.kt`; `eq.md`'s *The listening test*): `FilterChip`s choose
`Testing` (*Equaliser*, *Restoration*, *Resampler*, *Dither*) and, for a setting,
`ListeningTestViewModel.chooseTesting` asks `settingTestByDefault(kind, preferences)` for the
`TestedSetting` to begin from, drawn as *A is …* with chips for B (`Against`; the labels are
Processing's `restorationChoices`, `qualityChoices` and `phaseChoices` and the section's
`ditherChoices`, A left out of B's restoration and dither choices). `beginTest` calls
`beginSettingTest` on `Dispatchers.Default` where a setting is chosen, the begin buttons reading
*Measuring…* (restoration) or *Starting…* while it runs, and an engine refusal is `Resonate.attempt`'s
banner carrying the boundary's sentence. The equaliser is tested on the stored curve, and the test
ends when the page is left (`onCleared` calls `endTest`). The words are `ListeningTestView.kt`'s, the
levels sentence from `TestLevels` (`ListeningTestViewTest`).

The sleep timer is the engine's `Until`: elapsed time, end of song or end of queue, with cancellation
and the existing fade into a pause. Snapshot polling carries the remaining time; the UI keeps no
timer, so the countdown and pause continue with the Activity in the background. A timer is not
restored into a later engine session.

## Library tools

Scanning keeps album artist, genre, date, track/disc numbers, depth, channel count and tagged
MusicBrainz ids. Embedded pictures precede the folder's cover/folder/front JPEG or PNG, bounded at
8 MiB; a matching-basename LRC is bounded at 1 MiB and decoded through core's text reader.
The boundary refuses past the same bound itself (`keep_sidecar_lyrics`, `keep_lyric_text`:
`LyricsTooLarge`), as `keep_folder_cover` does past 8 MiB and `recognise_heard` past 30 s of
samples at 192 kHz (`ClipTooLong`), so a front end's own cap is not the only one. A bad
companion does not refuse playable audio. A directory's cover crosses the boundary once, for every
song the scan indexed there (`keep_folder_cover`), not once a song. Albums group by title and album artist, falling back
to the tagged artist. Songs resolve by id through the repository's index (`findTrack`), not a scan of the catalog per
request. `refresh()` and `reload()` take the repository's `access` lock one 500-row page at a time
(`pagedTracks`), so the poll and queued transport actions run between the pages of a big library's
read, and a `reload` a newer request overtook stops at its next page. **A walk of the library's pages is read from one index** (`pagedPinned`, also `filteredFully`): each page after the first asks `filtered_page` with the revision the first answered, so the boundary reads it from the index the walk began on however the catalog moves meanwhile (`mobile.md`), and only a page whose pin was let go starts the walk over, at most `PAGE_WALKS_AT_MOST` (3) times; each song is kept once by id, so a scan or sync landing between pages neither skips a row nor doubles one into a lazy list's key. A saved search's pages (`pagedFully`) still read `catalog_revision` before and after and walk again on a moved one (`pagedSteadily`) (`PagedSteadilyTest`). A song's album page opens by `album_key` (`albumKey`), so a song with
another spelling, no album artist or a shared title opens its own album, and the song sheet's
heart reads the live favourite, not the map the row was fetched with. Folder names and grants are listed independently of the catalog, including
unavailable grants; removing access revokes a grant after confirmation, leaving music files
untouched, and hides the songs under a tree as withdrawn (`hide_songs_under` its prefix, `MusicDocuments.prefix`:
`<tree>/document/`) while Downloads keeps its entries.

**A song whose file has gone is hidden, as on iOS** (`LibraryFeatures.forgetMissing`): when the
Activity comes forward (at most every fifteen seconds, so a trip to a file manager and back finds
the deletions, and not during a scan) and after a scan that
failed on no song, `hide_unlisted_songs` is given the prefixes of the readable folders (a tree whose
root answers a query, and MediaStore's Files collection while Downloads is on), so an unreadable
grant never hides its songs, and the documents those folders list (`MusicDocuments.listed`: one
query a directory, not one open a song): only a song the listing does not show is opened, so
`NotFound` still decides (a deleted document under a tree grant fails to open with an
`IllegalArgumentException`, from `FileSystemProvider.isChildDocument`, as well as a
`FileNotFoundException`, and `openGranted` answers both `NotFound`; a `SecurityException` is a refusal),
and a listing that fails part-way costs opens, never a wrong hiding. A pass that hid songs reads the catalog again, reloads the open
playlist and, only when *Tell me what the library finds by itself* is on, says *N songs whose files are gone were hidden*; one that fails is reported once per
distinct text. The queue drops hidden rows through the boundary's own `left_the_queue`.

**A file opened from elsewhere is played at once** (`OpenedFiles`): `MainActivity` takes `ACTION_VIEW` of
a `content:` audio URI (`audio/*`, `application/ogg`), as iOS's `onOpenURL` does, and once the controller is
connected plays it through `Playback`. A document that lies in a granted tree (the same authority and a
tree document id holding it, the most specific tree winning, `OpenedNames.withinTree`) is indexed at the
URI that tree reaches it by and played where it is; any other is copied into `files/opened` under its
display name made safe (`safeName`; `name (2).ext` where taken) and indexed as
`resonate-folder://local/<escaped name>`, a URI `Documents.openDocument` resolves to that file before it
asks the resolver (iOS's `local` folder). The copy is kept as a song of the library; a temporary grant
would not outlive the process.

The song sheet holds *Share link* (`shareLink`: `share_link(id)` on the
IO dispatcher, then the system `ACTION_SEND` chooser with the link as text; no link is a notice) and,
from a queue row's *More* button, *Remove from queue* by `queueId`; the swipe stays.

**A playlist is a list or a saved search, as on the desktop** (`saved_searches.rs`). A saved search
is a `playlists` row with a `playlist_queries` row beside it, holding no `playlist_rows`; the boundary's
`SavedSearch` is its text (blank asks every song), a `SearchOrder` (the desktop's ten `SortOrder`s),
`descending` and an optional row cap, and `MusicPlaylist.search` carries it (`None` for a list). It is
read when asked, never stored as rows: `playlist_tracks`, `export_playlist` and the listing's `songs`
and `covers` run it through the search index (`Searching::saved`: the grammar's every clause, then
`SortOrder::Relevance` ranks as `search_library` does where the text has lone words and is the album
order otherwise, the other orders read the index's columns with the desktop's tie on the title, the
reading reverses, the cap cuts), so it answers differently once the catalog or the clock moves; a
page's `queue_id` is the row's place in the answer. `save_search(name, search)` makes one (one undo
step) and `revise_search(id, revision, name, search)` rewrites its name, text, order, direction and cap
in place as one revision-checked, undoable step (`Error::NotASavedSearch` for a list,
`PlaylistChanged` for a stale revision, `DuplicatePlaylistName` for a taken name; the next sync offers
the new body as the desktop's `revise_query` is offered, `a_saved_search_revised_on_the_phone_is_offered_to_the_other_devices_in_its_new_form`);
a cap of 0 is no cap, since a search asking no row would only be empty
(`a_saved_search_is_revised_in_text_order_cap_and_name_as_one_undoable_step`,
`a_revision_of_a_stale_a_taken_or_a_list_playlist_is_refused_and_changes_nothing`,
`a_cap_of_none_or_zero_asks_every_row`); rename, delete and undo are a list's calls (undo restores the query with the row); `add_to_playlist`,
`remove_from_playlist` and `move_in_playlist` refuse it with `NotAList`
(`a_saved_search_answers_from_the_catalog_in_its_order_and_under_its_cap`,
`a_saved_search_refuses_list_edits_and_is_renamed_deleted_and_restored_whole`).
`search_order_reads_descending(order)` is the desktop's `SortOrder::reads`. Android saves a search from
Search's Songs scope (*Save as playlist*) in `SavedSearchDialog`: a name, the search text, an order
menu of the ten `SearchOrder`s (choosing one takes `search_order_reads_descending`), a *Descending*
switch and a *Limit* (`SavedSearchDraft`: blank for every row, else a whole number above zero, which
`capped` holds); its playlist page shows *Saved search*, the text and the order, direction and cap
(`SavedSearch.reading`), plain rows without swipe, handle or *Add songs*, and its menu offers *Edit
search* (the same dialog, `revise_search`) beside rename, undo and delete; the *Add to playlist*
picker lists lists alone (`SavedSearchDraftTest`).

Filters combine search text, folded artist/genre/album, minimum rate/depth, favourites and unplayed,
with seven stable orders (title, album, artist, duration, added, plays and last played, the latest
counted listen, the unheard last) and direction, read off the search index below. Suggestions are the
desktop's lists (`mobile.md`'s *Made for you*, `suggested_lists`): *On repeat*, *More like*, *deep cuts*,
*Discover*, *Back to*, *Old flames* and the rest read from the actual library and its listening history,
turning daily. Home's *Made for you* shelf shows up to twelve of the first three shelves
(`SuggestedLists.onHome`, `HomeViewModel.mixes`), a card wearing its reason's icon (`SuggestionReason.icon`)
and the cover mosaic the boundary measured, and the Made for you page (`SuggestionsScreen`) lists every
shelf under its name after the *Your favourites* card. A card opens `Screen.Suggested`, which carries the
list's name, detail and search as plain values (`SuggestedSearch`, so a restored back stack needs no
catalog) and reads its songs with `saved_search_page` and its covers with `measure_saved_search`
(`SuggestedViewModel`); its overflow menu offers *Keep as a playlist that fills itself*
(`LibraryFeatures.keepSearchAsPlaylist`, `keep_search_as_playlist`: keeping the same list twice says
*Already kept as ...* and makes no second playlist). The four filter `Mix`es (*Never heard*, *Most played*, *Recently added*, *High
resolution*) remain the Search tab's browse tiles. `Listening` counts heard time rather than sought position; its half-track or
four-minute threshold feeds the local history and statistics. History filters cover all time and
the last week/month/year. Document playlists preserve repeated songs, use revision checks for
position edits, and have bounded in-session undo; they never invent Linux paths.

**A playlist pasted as a link is imported once or kept in step with it** (`playlist_links.rs`). The
shared link's page in Search offers, where `playlist_link_service` (the boundary's own
`PlaylistLink::read`, so no host list is kept in Kotlin) names the text's service, *Import once* and *Keep in step with <service>* beside *Save
all for later*, each naming the playlist first and calling `import_linked_playlist(link, name, songs,
kept_in_step)` (`LibraryFeatures.importLinkedPlaylist`); a song link's songs keep *Import as playlist*
over `import_matched_playlist`. Kept in step, the wants saved read the Saved for later list again and
nudge the automatic downloads (`DownloadQueue.wantsChanged`). A playlist whose `MusicPlaylist.link` is
set shows *Imported from <service>* or *Kept in step with <service>* under its header
(`LinkedPlaylistWords.line`) with a refresh button (a spinner while `refreshingPlaylists` holds it,
off while offline), and its menu *Keep in step with <service>* (`keep_playlist_in_step`) and *Forget
the link* (`forget_playlist_link`). `refresh_linked_playlist` runs on `LibraryFeatures`' worker, one
refresh per playlist at a time (`claimRefresh`), reads the playlists and the open playlist again only
when something moved and says it (`LinkedPlaylistWords.refreshed`: added, taken out, saved for later).
While the Activity is visible and online is on, `keepLinkedPlaylistsInStep` looks a minute after
start and then every ten minutes at `playlists_due_a_refresh` (six hours since the last read) and
refreshes each; a quiet refresh says nothing, a moved one is a finding (`tellFinding`), and a failure
is said once per distinct message (`LinkedPlaylistWordsTest`).
**A public profile is followed the same way** (`profile_links.rs`). Where `profile_link_service` names
the pasted text's service (a Deezer profile), Search shows a profile card in place of the empty
collection (the online-off card while online is off) with *Follow <service> profile* and *Move to
Resonate once*, a spinner while `followingProfiles` holds the link; both call `follow_profile(link,
kept_in_step)` on `LibraryFeatures`' worker (`followProfile`, one follow per link at a time,
`claimProfile`), then read the playlists and the open playlist again, read Saved for later again and
nudge the downloads where an in-step playlist misses songs, and say `FollowedProfileWords.followed`
(*Following <name> on Deezer · N playlists*, with the let-go, deleted and unreadable counts where not
nought) or `moved` (*Moved N playlists of <name> to Resonate*). The same ten-minute loop first follows
each of `profiles_due_a_refresh` kept in step, reading the playlists again only where
`FollowedProfileWords.changes` (the playlists before and after, by id and revision, and `gone`) is not
quiet, and saying `refreshed` as a finding. Library tools lists `profiles_followed` under *Profiles
followed*, each with *Stop following* (`unfollow_profile`, confirmed; its playlists stay)
(`FollowedProfileWordsTest`).

**A Spotify account is a page of Settings** (`SpotifyScreen`, `SpotifyWords`; `mobile.md`'s *A Spotify account on a phone*): a client ID, kept encrypted beside the other keys (`spotify-client-id`, `spotify-refresh-token`, `ProviderAccountStore.readSpotify`) and handed over with `configure_spotify` at start and on a change. *Sign in to Spotify* is off without one or offline; `LibraryFeatures.signInToSpotify` asks `authorize_spotify` on its worker, the screen opens the address in the browser (`spotifyAddress`, `Host.open`, with *Open the page* and *Cancel* while it waits on `finish_spotify_authorization`). Signed in, *Follow the account* and *Move to Resonate once* are `followProfile(spotify_account_link(), ...)`, the profile's path, its timers and its *Profiles followed* row included; a rotated token is written after every read that may have rotated one (`keepRenewedSpotifyToken`); *Sign out* clears the token and calls `sign_out_of_spotify`. Whether a backgrounded process hears the browser's answer has not been seen on a device (`SpotifyWordsTest`; `docs/TESTING.md`).

**Statistics beyond the totals are `listening_detail(days, limit, time_zone, utc_offset_seconds)`** (`catalog_extras.rs`):
the albums (by the catalog's `album_key`) and artists (by folded name) heard most in the window,
ranked by heard time then plays, at most `MOST_LISTENED_AT_MOST` (100), and the listening per day
(counted listens, a day cut at the local midnight of the caller's IANA zone, each day as long as
the clocks make it, the offset only where no database names the zone (`library.md`'s *A day is the
listener's*), the latest `DAYS_SHOWN_AT_MOST` days oldest
first, every day heard for all time (`days` 0), `ListenedOnDay.starts` the unix second that day began). Hidden songs and uncounted listens
are left out of every total and day, as in `statistics(days)` (`plays`, `listened_ms`); `songs_heard` counts the distinct songs and `first_heard`
is the earliest counted listen in the window. `library_breakdown()` is what the library holds
regardless of listening: the eight most common formats (codec), sample rates (`44.1 kHz`), genres
(folded) and decades (from `date`'s first four digits), each with songs and length, hidden songs
left out. `statistics(days)` ranks the top 25 songs. The page leads with *Time listening* as one wide `primaryContainer` tile above *Listens* and *Different songs*; the year in review opens from a `Shortcut` tile under the periods.

**A playlist file is read and written over the document catalog** (`playlist_files.rs`).
`import_playlist_file(bytes, file_name)` decodes the text through core's `text::decoded` (UTF-16
and code pages read), reads M3U, PLS or XSPF with the library's own `read_sheet`, and matches each
entry to a visible document by file name; a name held twice goes to the document sharing most
trailing folders with the entry and stays unmatched on a tie, and a URL or an unmatched path counts
as missing. The playlist takes the sheet's declared name, else the file's stem, numbered
(`Name (2)`) where the name is taken, in one transaction and one undo step; a file listing nothing
is `PlaylistFileEmpty`, one over 8 MiB `PlaylistFileTooLarge`, one matching nothing `NoLocalMatches`.
`export_playlist(id, format)` writes the visible rows with titles, artists and lengths through the
library's `written_sheet`, each path relative to its folder (the root's id left out, since it names
a bookmark, not a place). `playlist_file_extensions()` is what `ios/Resonate/Info.plist` declares
as `org.resonate.playlist` (held by a test).

Lyrics prefer saved embedded/sidecar text, fetch from LRCLIB and Better Lyrics (scored per song,
`online.md`) only when online, and retain a richer saved sheet over a poorer answer or failed
refresh; the pane reads the native `LyricFrame`'s `read_at`, `active` and `wipes`.
Analysis uses the shared whole-track pass for waveform, spectrum, lossless assessment, LUFS, DR,
true peak, clipping and bit use. Eight full results are cached in memory, invalidated on rescan;
summary studies are persisted. A `Watch` cancels decoding. The live visualizer subscribes to the
engine tap only while visible and samples on a worker; it never involves Kotlin in the callback.
The inspector probes the original document and displays both technical and tagged metadata; each
Details page reads its own (`InspectorState`), offering *Try again* where the file could not be read.
The analysis pane shows the pass running only for the song it was started on (`analysisTrack`), and
the lyrics view keeps the sheet it was given for its song, asking again only where the shared sheet
moved to another song before it saw one. **A lookup that throws is not asked again by itself**:
`lyricsUnread` holds the song's id, the pane says *Lyrics could not be read* with *Try again*, and a refresh
of a song that already has its sheet keeps that sheet. The lyric frame is not computed in the poll's
`access` lock: `followLyrics` (a child of the playback job, every 33 ms while a pane is open and the
Activity is forward) asks `keptLyricFrame` and only on a miss `lyricFrame`, at the position the
`Place` clock reads.

**Tags are written through the desktop's writer, on a copy of the document, then the document is replaced** (`tagging.rs`, `rewriting.rs`).
`FileTags` writes by path alone (staged copy, journal, read back: `library.md`'s *Writing the catalog
back into the files*), so `edit_tags(songs, change)` takes each song as a `SongFile` (its URI and
a path): iOS passes the path its security-scoped folder resolved, and Android passes one only for a
copy in `files/opened` (`TagFiles.songFile`), which the writer then edits in place like any file. A
SAF or MediaStore document has no path, so Android passes an empty one and the boundary does the
staging. **The document is replaced only by bytes that were read back whole.** `Stage` is a folder of
its own under `tag-staging` beside `documents.sqlite`, named `<pid>-<n>` and holding a `held` file
under `writer::Held` (a sweeper, run by the next stage made, asks `writer::writer_of` and removes
a folder whose writer is gone, so a killed run leaves nothing for ever), an `original/` and a
`working/` folder, each holding the song under its own name cut by `naming::named_within`. The steps, each
undone by the next if it fails: the document is copied through the `content` provider into `working/`
(`CopiedAside`), so every failure up to here has touched nothing; the tags are read and planned there
(`planned`), and an edit that changes no field ends as *unchanged* having read the document once and
written nothing; the working copy is copied to `original/` and its CRC-32 taken; `FileTags` writes the
working copy, with its own staged copy and journal beside it, and reads it back (`TagsUnconfirmed`
as on a path), so a write the desktop's writer or its read-back refuses never reaches the document;
then `DocumentRewriting::rewrite_document(uri)` hands back a descriptor opened `"wt"`, which truncates
the document at once, and `put_back` copies the working file into it and flushes. The document is
then read again through the provider and its CRC-32 compared with the working copy's: only a document
that holds the very bytes verified is accepted, and only then is the catalog row set to what the
working copy reads, the analysis dropped and an undo step kept, so **the row never claims tags the
file does not hold**. A descriptor refused (`TagsDocument { Opened }`: a revoked grant, a MediaStore
row the app does not own) has truncated nothing and ends the song as failed. Any later failure (a write
cut short, a provider keeping nothing, a CRC that differs) has already emptied the document, so the
original is written back the same way and its CRC checked; a restored document is a failed song with
its first error, nothing changed. **An original that cannot be written back is never deleted**:
`keep_original` moves it to `tag-unrestored/` (never swept) and `TagsUnrestored { kept }` says so,
since the stage's folder is otherwise removed whatever happened. A song costs up to three copies of
its file on the app's storage at once (`original/`, `working/` and the writer's own staged copy) and
one read of the document to verify, one song at a time; a full disk is `CopiedAside` before the
document is touched. **A MediaStore row the app does not own is written only after the listener
allows it**: `TagEditing` probes each Files URI with a read-write descriptor (never `"w"`, which
truncates) and, for the ones that throw, asks `Host.confirmWrite` once for the batch
(`MediaStore.createWriteRequest`, as `confirmDelete` asks `createDeleteRequest`); a request over a
Files URI is refused as not a media item, so the probe, the request and the `"wt"` descriptor all
use the same row's audio URI (`TagFiles.writeUri`); songs the listener
refuses are not sent to the boundary and are told as *Writing to N songs was not allowed*, and a
SAF tree's documents need no such step. The rewriting callback is not part of `Documents`, so iOS's implementation of that
interface is untouched: `rewrite_documents_with(rewriting)` is a separate exported method Android
calls once after the player opens (`DocumentRewrites`: `openFileDescriptor(uri, "wt")`, detached;
`document_not_found()` for a `FileNotFoundException`, else `document_refused()`), and iOS, which always
passes a path, never calls it, so the surface is the same on every target; without it a pathless
song is `TagsUnwritable`. The one `unsafe` descriptor transfer is `documents::owned`, for reading and
writing alike. A file changed by another program between the copy and the replacement is overwritten
by the edited copy; a scan that opens the document during the replacement can fail on that song for one
pass. `SongTags` is eleven text fields (`EDITABLE`: title, artist, album,
album artist, genre, date, track and disc number and total, composer), each `None` to leave, blank to
clear, else the value; a number must be a whole number above zero (`TagNumber`). Only the fields
differing from what the file holds are written (`planned`). Formats `FileTags::writes` refuses are
`TagsUnwritable` (`tags_writable(name)` asks first). Each call is one undo step of what every written
field held before, the last `UNDO_STEPS_KEPT` (16) kept for the session; `tags_to_undo` names its URIs
and `undo_tags(songs)` writes them back through the same route; a song the call left out or could not write goes back on the stack as a step of its own, so a later undo retries it (`an_undo_that_leaves_a_song_out_keeps_its_tags_for_the_next_undo`). `song_tags(id)` reads a song's fields
through the document provider. `TagsWritten` counts written, unchanged and failed songs with the
first failure's sentence
(`tags_written_to_a_document_reach_it_through_a_staged_copy_and_one_undo_puts_them_back`,
`a_write_back_cut_short_puts_the_original_bytes_back_and_the_row_keeps_its_tags`,
`a_document_that_does_not_hold_what_was_written_is_put_back_as_it_was`,
`an_original_that_cannot_be_put_back_is_kept_whole`,
`a_document_refused_for_writing_is_left_untouched`,
`a_stage_a_dead_writer_left_is_swept_and_a_living_one_is_not`).

**The sheet is iOS's** (`TagEditor.kt`, `Sheets.editingTags`, `TagEditing`): *Edit tags* in the song
sheet where `TagEditing.writable` says its kind is writable (`audioExtensions()` asked once of
`tags_writable` at start) and its folder is (`songDeletable`), *Edit album tags* in an album page's menu.
One song shows all eleven fields; several hide title and track number, show a field the songs share
with its value and one they differ in as *Mixed* (changed only once typed into, or cleared on every
song by the field's button), read at most 500 songs, and say how many cannot be written. *Save* sends
only the fields that moved (`TagDiff`, the Swift `TagDiff`'s arithmetic held by `TagEditingTest`), a
sheet with unsaved edits asks before it closes (its state vetoes the hide and raises *Discard your
changes?*, so *Keep editing* leaves it open), and the notice counts saved, unchanged and failed songs
(`TagSentences`). Library tools offers *Undo tag edit* while `tags_to_undo` names a step.

**A song is deleted by the front end, then forgotten; identical files fold into one** (`deleting.rs`).
The front end removes the file (Rust holds only a descriptor), then `forget_deleted_songs(ids)` drops,
of the songs whose documents no longer open, the queue rows naming each (the playing one moving on), steps the revision of every playlist holding
it and deletes its row with every row referring to it (playlist rows, listens, lyrics, artwork,
studies; a picture nothing else shows goes too), answering `SongsForgotten` (rows forgotten, queue
rows left); its listens leave the totals, as a hidden song's already do, and the keys of the song and of its `songs_received` rows go into `songs_deleted` in the same transaction, so a follow never fetches it back (`sync.md`); `fold_identical_songs` marks nothing. **A document that opens again is not forgotten**: a sync pass may fetch a song between its file going and the forgetting, and its landing, made at the freed name, is the same URI and so upserts onto the very row being deleted; forgetting it would leave the file standing unindexed and the song marked deleted, to be fetched again beside that file as a `name (1)` (`a_song_landed_again_where_it_was_deleted_before_the_deletion_was_forgotten_stays_held`). The queue rows it deletes leave
holes in `queue.position`, so the saved place is read by position, not by index (`place_among_rows`, for
resume and the sync handoff alike): a position still standing is its row at its time, one that is gone
is the next remaining row from its start
(`deleting_a_song_before_the_one_heard_resumes_the_same_song_at_its_place`,
`deleting_the_song_heard_resumes_the_next_one_from_its_start`). `identical_songs()` answers
`IdenticalSongs` (the copy kept and the copies) for files holding exactly the same bytes: held
songs grouped by size (the stamp a scan kept, else measured through the document provider, since a
download, a landing or an opened file is indexed with none), then by a CRC-32 of the whole file,
then confirmed byte for byte, so equal sizes or equal CRCs alone never match
(`songs_whose_files_hold_the_same_bytes_are_found_and_one_alike_in_size_alone_is_not`). The copy
playing is kept, else the first indexed (`the_copy_playing_is_the_one_kept`); an unreadable file is
left out. `fold_identical_songs(kept, copies)`, called after the copies' files are gone, hands each
copy's favourite, playlist rows, listens, lyrics and studies to the one kept (`handed_over`, the
missing-song merge) and drops its queue rows
(`folding_a_copy_hands_its_favourite_and_playlist_rows_to_the_one_kept`,
`a_deleted_song_leaves_the_catalog_the_queue_and_its_playlists`). Both phones call them. **Android deletes
through `SongDeletion`**: the song sheet's *Delete song* (shown where the song's folder is writable or it is
a copy in `files/opened`, `songDeletable`; confirmed) and Library tools' *Identical songs* (a tap
chooses the copy kept, *Delete copies*, confirmed) remove each file (`MusicDocuments.delete`: a SAF
`deleteDocument`, a MediaStore delete, or the opened copy), and only the songs whose files went are
forgotten or folded; a file the app does not own in MediaStore (a `SecurityException`) is collected and
asked of the listener once with `MediaStore.createDeleteRequest` (`Host.confirmDelete`), and one that
stays is told as *N files could not be deleted* (`DeletionSentences`, `SongDeletionTest`).

**A folder is organised by the desktop's layout, its files moved by the front end** (`organising.rs`).
`plan_organise(folder, layout)` reads the layout through `resonate_library::Layout` (refused as
`LayoutUnreadable`; `organise_layout_problem` says why before asking, `default_organise_layout` is
`DEFAULT_LAYOUT`) and renders every unhidden document whose URI lies under the folder's prefix
through `Layout::render` with `Named` filled from its row (album artist, artist and album where
given, title, the year off the date's first four digits, disc, the album's disc count, track,
the file name's extension) under `Naming::Anything`. A song rendering nothing is `unplaced`, one
already at its place `unchanged`, one whose place another song stands at or claimed first `taken`;
the rest are `FiledSong`s (id, URI, the path's components). The front end moves each file and
reports the new URIs in one `documents_moved`: where any moved song is in the queue it saves the
queue first (by the old URIs, which the catalog still names), rewrites every moved row's URI in one
transaction (ids, and with them favourites, plays, playlists and lyrics, stay), then resumes the
saved queue at its row and position, playing again where it was playing. A destination another row
still holds (the front end refuses a file that exists, so that row's file is gone) never fails the
batch: where it names the same song as the moved one its history is handed over and it is deleted
(`take_the_place_of_missing`'s merge), else it stays hidden under a `resonate-displaced:<id>` URI no
scan can reach, keeping its favourite and playlist rows for a song fetched back under another name.
A `content:` folder is planned beneath `<folder>%2F` (`within_of`), not `<folder>/`: SAF encodes the
slash between the components of a document id, so a song's place below the root is its id read
back through the same unescaping (`a_granted_tree_holds_its_songs_beneath_the_encoded_id_of_their_folder`).

**Android organises a writable tree from its row in Music folders** (`OrganiseScreen`, `FolderOrganising`,
`FolderFiling`; iOS's `OrganiseView.swift`, `ios.md`): the row's *Organise* button (shown where the
grant is writable, never for Downloads) opens a page with a layout field (`organise-layout` in
`library-features`, `default_organise_layout` until changed, `organise_layout_problem` under the field
and *Organise* off while it speaks), a preview 300 ms after the last keystroke (`plan_organise` on the IO
dispatcher given `MusicDocuments.rootDocument`, the root's document URI as the scan spells it: the
counts and the first fifty moves; a plan stands only for the layout it was made for, so *Organise*
never runs, or counts, the previous layout's) and *Organise*, confirmed. `FolderOrganising.file` holds
`LibraryRepository.afterScans`, the scans' own mutex, for the whole run, so it waits for a scan running or
queued before it and holds the scans asked after, and the new URIs are never indexed beside the old rows
(*Filing songs once the scan ends* where one runs; the missing-song pass is skipped while it runs);
`FolderFiling` moves over `DocumentTree` (`SafTree`: `DocumentsContract` children, `createDocument`,
`moveDocument`, `renameDocument`, `deleteDocument`), so the logic is held on the JVM against a
fake tree (`FolderFilingTest`). A document id is read as a path under the tree's root id, as the plan
reads it; a source that is not beneath the root or of another tree, one that is not there, a name that
is not a plain component and a destination that exists (case ignored) count as failed, and a folder
that cannot be listed fails the song instead of looking empty. Folders are found case-insensitively
before one is made, a different directory moves and then renames (renaming first where the destination
already holds the old name), a rename refused after the move puts the file back, a same-stem `.lrc`
(any case) moves along where its place is free, and the folders the moves left empty, and those made
for a failed move, are removed from the deepest up to the root, never above it and never one whose
listing is empty only because it failed. The new URI is built from the id the provider answers
(`buildDocumentUriUsingTree`), the scan's own spelling, so a rescan finds the same rows; one
`documents_moved` on the repository's ordered lane (`LibraryRepository.inOrder`) reports them, the
catalog, playlists and queue are read again and a notice counts what was *Filed* and what could not be
moved. A provider without `moveDocument` fails every move. The moves back are kept for the session
(`FolderOrganising.undo`): Library tools' *Put files back* files them the other way through the same
code, with the old places as the destinations.

## Searching the documents

`search.rs` keeps every document row in memory, folded through `folded_letters` (the desktop's
fold, so *bjork* finds *Björk*): title, artist, album artist, album, genre, a year read off the
first four digits of `date`, codec, rate, bits, channels, length, favourite and each counted
listen's time; lyrics' kept `text` (never a Lyricsfile document) is folded with `[…]`/`<…>` stamps
stripped. The index sits behind `Services::searching` and is rebuilt on the next ask once
`Catalog::revision` moves; lyrics are a separate map read only for a `lyrics:` word and rebuilt when
the `lyrics` table's count, `asked` sum or text length moves (lyrics are outside the update hook).
`Spellings` is filled from the same rows on first need and kept with the index.

`Search::read` is the grammar, the desktop's whole: every clause holds, so words match in any order
(*floyd pink*), each piece of a bare word beginning a word of title, artist, album artist, album or
genre, never lyrics; `title:`, `artist:` (artist or album artist), `album:`, `genre:`, `lyrics:`
scope; quotes ask a run of whole words inside one name; `artist:=Air` asks the whole name (not
*Airiel*); `-` denies one token; `or` alternates. Terms answered: `year:`, `length:`, `rate:`,
`depth:` (`bits`), `codec:`, `plays:` (with `@` windows over listens), `played:`, `is:` lossless,
lossy, mono, stereo, multichannel, hires, favourite. `added:` (a row's `added` second, `Row::added`; an undated row answers neither way), and `is:fake`/`suspect` (the
`studies` verdict, read apart from the index as `Studied`, stamped by the counts and id sums of its
fake, suspect and `disagrees` rows and held beside the cached order, so a study written without moving the catalog
revision is seen by the next search). `is:misnamed` is a `studies.agreement` of `disagrees`, written by
`recognise_track` and by the enrichment pass (below) from the recognition's matches weighed by the library's
own `agreement` against the song's name (a title equal to the file name is untagged, so `Unnamed`); a song not
yet recognised is not misnamed, so its denial holds. `is:hidden` holds nothing (a phone keeps no hidden songs), so its denial holds everything, as the desktop's
`NOT coalesce(…, 0)` does for a row unable to answer. A text reading nothing (`!!!`)
matches nothing. A lone undenied word of three letters or more also matches inside a word, below
every other fit.

Ranking weighs each lone word's best fit, the worst word deciding: whole name, then name prefix,
then whole word, word prefix, inside a word; the words together as a whole name in any order (*floyd
pink* is Pink Floyd) lift a row to that fit. Ties go to the column (title, artist, album, genre,
lyrics), then favourites, then play count, then folded title and id. Exports:
`search_library(text, limit)` answers a `LibrarySearch` (ranked tracks to `limit`, at most 500, the
full `track_count`, the albums and artists holding them with those named by the words first, the
*Reads* chips, completions from `Spellings::completing`, and `did_you_mean` only where nothing
matched), and `search_tracks(text, offset, limit)` the same ranking a page at a time (Search's *Show more* in the Songs scope reads the next 500 and lights them); `highlights(query, text, column)` is `Search::lit` in UTF-16 code units, as Swift and
Kotlin index. `albums(order)`/`artists(order)` (`CollectionOrder`: alphabetical, artist, year newest
first, recently added, plays) group in Rust: an album by folded title plus folded album artist,
else artist (an empty album title is no album), and then, as the desktop's `grouping_keys` gather,
keys meeting in a release id, or in one title and one sleeve (`sleeve_of`: the document's folder
read off its URI, a disc folder `scan::disc_in_folder` names answering its parent, a song loose in a
granted root or SAF tree in none) are one album (`albums_of`, a union of the provisional keys, the
largest lending its key); every member key opens it, and one whose songs bill different owners is
*Various artists* with `AlbumHit.various_artists`, so a front end links no artist
(`an_album_split_by_its_credits_or_discs_is_gathered_by_the_folder_holding_it`). Its `key` is what
`album_tracks` and `album(key)` take back, in disc
and track order with unnumbered rows last, and the free `album_key(album, artist, album_artist)`
the same key for one song (`None` without an album), so a song's page needs no lookup;
`albums_by(name)` lists an artist's albums, newest first, and `most_played_by(name, limit)` its
songs by counted plays; `pictured_by(filter)` answers the
artwork songs of up to four pictured albums the filter's rows fall on, first rows first; an artist by folded name, holding every song whose
artist or album artist folds to it. `artist_tracks("")` is the songs naming no artist alone, the
*Unknown artist* group, never every song lacking an album artist. `filtered_tracks` reads the same
index: its text is the grammar, its album, artist and genre compare folded, and a page costs a sort
of the matches rather than an `OFFSET` walk.

## Online and downloads

Online starts enabled when no preference is stored; Settings persists an explicit offline choice across upgrades. A single paced `Online` client serves
MusicBrainz search/enrichment, covers, portraits, LRCLIB, AutoEq, recognition and scrobbling.

Enrichment is the desktop's pass on the document catalog (`enrichment.rs`), weighed by
`resonate-library`'s own strict rules (`top_release`, `top_group`, `closest_release`,
`the_strict_recording`, `best_recording`, `best_release`, `recording_asked`), never a copy of them.
`start_enrichment(EnrichmentOptions { refresh, at_most })` runs it on a `resonate-enrich` thread
over the shared client's yielding twin, so a listener's request never waits behind it; it answers
false while offline or while a pass runs. Albums go first (one release lookup and at most one cover
per album key, the group's cover where the release has none, asked only where the album shows no
picture or one under `A_THUMBNAIL_BELOW`), then songs (ISRC, recording id, then a strict search;
`learned` fills only empty identities and never moves the album key), then artists (profile and
portrait into `artists`); `resonate-enrich-pictures` fetches pictures beside the pass and
`resonate-enrich-lyrics` asks LRCLIB and Better Lyrics for songs with no sheet, a miss older than
`MISSED_AGAIN_AFTER` or a fetched sheet without a Lyricsfile older than `BETTERED_AFTER`, keeping
the richer through `KeptLyrics::richer_of`. A fetched row's origin stays `FETCHED_LYRICS`; the
sheet's shown origin is `resonate_online::lyric_source_of` its document. Every row backs off by the library's own `due_again`
clause and `WAITS`; refusals end the pass at `REFUSALS_THAT_END_A_PASS` in a row; it checks between
requests for `stop_enrichment`, offline and the network. `enrichment_status` counts what was due,
asked, matched, covers, portraits, lyrics and refusals and names how it stopped (`EnrichmentStop`).
The `enrichment` mark is removed when the pass finishes or the listener stops it; a pass ended by
the reference, the network, the app closing or `pause_enrichment` (iOS, on going to the background,
`ios.md`) leaves it, and the next start carries its refresh, skipping rows asked since it began. **A download is looked up first**: every pass puts the album and
song asks of each `downloaded` document still due ahead of the rest, the newest download first, and
orders the artists, the lyric reader and the study worker the same way
(`a_downloaded_song_is_looked_up_before_the_songs_held_before_it`); `index_download` also seeks the
song it landed. The metadata pass keeps serving sought songs while its picture, lyric and study workers
finish, waking on a seek, cancellation or worker completion; a download landing in that interval gets its album and cover in
that pass instead of waiting for another start
(`a_download_landing_while_lyrics_finish_gets_its_cover_in_the_running_pass`).
`match_local_song` returns the visible document matched by the same `local_match` used for wants,
so an online result plays the local recording without depending on the search answer's stale held flag.
`seek_enrichment(track)` lifts the song's album, the song and
its artist to the front where still due, and the song to the front of the lyric reader's list, and
a lifted or downloaded ask fetches its cover or portrait on the pass's own thread rather than behind
the pictures already waiting (`a_song_sought_has_its_album_and_words_asked_before_the_rest`). `enrich_track` is the same album, song and artist asks
forced for one row over the listener's client; its lyric lookup failing fails nothing. `artwork`
answers the song's own picture (embedded, folder, `set_artwork`) and falls back to its album key's
cover, or takes that cover where it betters a thumbnail; no per-song archive copy is written.
`art_changed_since(revision)` names the songs whose picture changed after a revision.
`artist_information` answers the kept profile and fetches and keeps one only where none is;
`artist_portrait(mbid)` reads the kept portrait; `ask_again_for_covers` clears `cover_asked` on
albums still without one, and `asked` on an album with no picture, no artwork and no release or
group ever answered, so a release added to MusicBrainz since is asked at once rather than after
the doubled wait its misses earned.

**The pass studies and recognises beside it** when it is given an AcoustID key
(`EnrichmentOptions::acoustid_key`, the key both phones keep encrypted and read at each start; a blank key
studies, measures bands and recognises nothing, so a phone with no key never decodes its library in the background, and
saving a key starts a pass). `resonate-study` is one thread (the desktop's pool is half the cores; a phone
keeps one) over `tracks_to_hear`: visible documents with no `studies.agreement`, and those studied under this
`JUDGED_UNDER` keeping a print with no `recognised` (every visible one under
`refresh`), cut to `at_most`, downloads first and then in id order. A song whose study keeps a print
(`kept_print`, its own or one landed from another device) is not decoded again; any other is studied by `resonate_analysis::study` (one decode, the
levels, verdict and print, stopped within a decode by `stop_enrichment` through `Hearing`), kept with its print through
`keep_studied`. The print is then asked of `AcoustId` over the pass's yielding client through
`recognised_through`, the one reading `recognise_track` shares: matches weighed by `agreement`, kept as
`studies.agreement` beside the top match heard and `recognised` (`keep_recognition`), so a song is asked once until its file changes (`documents_forget_what_was_heard`) or a
refresh. A song whose study has no print is kept `Unheard`, not studied again, until a print lands from another device. A refusal or unreadable
answer stamps nothing (asked again by the next pass) and `RECOGNITIONS_REFUSED_BEFORE_GIVING_UP` in a row
ends the recognition only, the rest of the pass going on; an unreachable service or going offline ends it
too. **A study that fails is kept as failed, as the desktop's `unstudied`**: a file that will not decode is noted
in `unstudied` under `JUDGED_UNDER` (`note_unstudied`), and `tracks_to_hear` passes it over until its `size`
or `modified` moves (the trigger), `JUDGED_UNDER` moves, or `refresh` asks again; a study kept
(`keep_figures`) clears the mark. A stopped study and a file out of reach
(`Error::is_out_of_reach`) leave no mark and are tried again by the next pass, and a song forgotten or
handed over takes its mark with it (`referring_tables`). The study thread counts into
`due` and `asked` and the pass is `running` until it has finished
(`a_pass_given_a_key_studies_and_hears_every_song_so_is_misnamed_fills_unasked`,
`a_pass_with_no_key_studies_and_hears_nothing`, `a_refusal_leaves_the_song_unheard_for_the_next_pass_to_ask`,
`recognition_is_given_up_after_a_run_of_refusals_while_the_pass_goes_on`,
`a_song_heard_is_not_asked_again_unless_the_pass_refreshes`, `a_pass_told_to_stop_studies_nothing`,
`a_song_that_will_not_decode_is_not_decoded_again_until_its_file_changes`,
`a_song_that_will_not_decode_is_studied_again_when_the_pass_refreshes`,
`a_song_marked_under_an_earlier_judgement_is_studied_again`, `a_song_forgotten_takes_its_mark_with_it`,
`a_catalog_of_before_the_marks_gains_them_and_forgets_a_mark_with_the_file`).
The pass is gated as it is, so the study runs only where automatic lookup, online and (if chosen) an
unmetered network permit.

Android's Artwork and details section uses this same pass. Automatic lookup defaults on;
Only on Wi-Fi defaults off and uses the unmetered-network signal. Both choices persist in
preferences. Initialization, scans and new downloads or sync arrivals start an automatic pass
only when those choices and online permit it. Restricting an automatic pass to Wi-Fi stops it
when the network becomes metered. Explicit refresh confirms the library count, restarts with
refresh enabled and offers Stop; retrying missing covers clears the cover retry marks and
the wait of an album nothing matched. Kotlin reads progress and artwork revisions every two seconds while the Activity is
visible and a pass runs, updates the artwork generation the UI keys covers by and refreshes completed catalog
metadata. Leaving the Activity stops observation; native shutdown owns stopping the pass.
The controller awaits the engine before taking its operation mutex, and repeated foreground
notifications do nothing: the first Activity start can precede the service's engine creation,
so waiting there under the mutex would block every later artwork action.

A chosen tree that holds or sits inside one already granted is refused with *holds or sits inside a
folder already in your library* (`OpenedNames.treesOverlap`, as iOS's `FolderFailure.overlapping`): a file
under both would be indexed at two URIs, and deleting identical copies would delete the only file.
The SAF folder chooser starts at Music. Android refuses a tree grant for the storage root and
Downloads itself; Settings offers Use Downloads and the download chooser offers Downloads
directly. This uses MediaStore rather than sending the listener back to the restricted picker.
New deliveries insert a pending row into the Downloads collection with `Download/` as their
relative path, pass its detached descriptor through the existing validation/indexing flow, then
publish it. Duplicate names create separate files. Cleanup and interrupted-delivery recovery
delete only the journalled new row through MediaStore; SAF destinations retain document deletion
and the exact granted URI. Downloads is saved among the chosen sources and rescanned with them;
removing it keeps files and catalog entries. Sync uses Downloads only when explicitly chosen as
its destination, so enabling the source never changes the previous automatic destination.

Search covers songs, albums, artists, release editions and songs within a release; artist
discography pages by identity. Shared song, album, artist and playlist links use the library's
existing resolvers; remote playlist order and repeats are retained, within the service's own page
limits. Importing local matches creates a document playlist in one transaction and one undo step;
uncoded name/artist/length matches must be unique, so an ambiguous row stays missing. Recordings saved for later persist independently of files, deduplicate by recording id or folded identity,
and distinguish held from missing against the current catalog; downloads refresh that view. Recognition
is explicitly asked from Analysis: a supplied AcoustID key sends the stored whole-track print,
otherwise `ByEar` sends a Shazam signature. Candidates are shown, never written as a guess.
`recognise_heard(samples, rate, audd_token)` names a mono clip the front end recorded (`listening.rs`):
Shazam, then AudD where a token is given, answering `HeardAnswer` (`Named` with a `HeardSong` whose
`by` is the service's name and `key` its folded title and artist, `Missed`, `Silent`, `Unreached`
naming the service); both phones record one.

**Listen names the song playing around the phone** (`Listening`, `ListenPlan`, `ListenSheet`): the ear in
Search's header and Library tools' *Listen* open a sheet that records at once. `Host.allowMicrophone` asks
for `RECORD_AUDIO` (refused: the sheet says so and offers the app's settings), playback is paused, and
`Microphone` reads 12 s of mono float PCM at 44.1 kHz from `AudioRecord` on the IO dispatcher
(cancelled with the sheet) into `recognise_heard` with the AudD token. `ListenPlan.after` is iOS's
`ListenNext`: a miss or silence is recorded again up to three times (*Try 2 of 3*), a service out of reach
is said at once, and a found song is a card with *Find it* (the Search tab's library search, typed as title
and artist through `searchAsked`), *Open* where the service gave a link and *Again*. *Keep listening*
records another clip 20 s after a named song or the last miss and keeps a song whose `key` matches quietly;
it ends where a clip cannot be asked. The last eight songs heard stay as *Heard earlier* for the run. The
AudD token is kept encrypted beside the other keys (`audd-token`) and typed in Library tools'
*Recognition* section; blank, only Shazam is asked (`ListenPlanTest`).

AutoEq and pasted EqualizerAPO/GraphicEQ profiles use the shared parser and load as the curve's
bands, every filter kept (`EqualiserImport`), into the curve in use (the bound one on a bound output). ListenBrainz and Last.fm accounts are opt-in,
validate through their existing seams, and submit counted local listens in batches of 100 with
independent durable marks advanced only after success. Existing history is included only when
asked; Last.fm is signed in to on its own page (`authorize_lastfm` answers it, the screen opens it
in the browser and offers *Cancel* and *Open the page again* while `finish_lastfm_authorization`
waits), and the session it answers is saved into the accounts with the key and secret it was made
under, leaving every other unsaved field as typed; *Sign out* saves the account without its
session. *Headphone correction* (`HeadphoneCorrectionScreen`) opens on the curve in use (read before
its first frame, `currentOf`), searches AutoEq as typing settles or on the keyboard's search, once per
text (offline, a card says so and the field is off); emptying the field clears the results, applying
one is confirmed (`ConfirmDialog`), shows its own progress and says when AutoEq gave no profile.
*Import a profile* follows on the same page, online or not: a name, the pasted text and *Apply
profile*, which commits the parsed curve and says `importSentence` (the bands, the name, a GraphicEQ
fit, and what was held, approximated or passed over). Favourites
are told by `tell_loves(account)` (`loves.rs`), which both phones' 30 s scrobbling loop calls after
the listens (the Scrobbling page says so; there is no send button, and a favourite taken back is
unloved on each saved account). It is the desktop's `Library::tell_loves` over the document
catalog: `loves_told` (service, recording) is weighed against the favourites with a recording,
at most `LOVES_TOLD_AT_ONCE` loves or take-backs per service and call; a love refused with a 4xx
other than 401, 403, 408 or 429 (`resonate_library::refused_for_good`) is noted told, any other
failure stops that service until the next call; one service failing tells the other.

`share_link(id)` (`sharing.rs`) is the desktop's share over a document row: `Shared` is built from the
row's title, artist, album, year, recording id, ISRC and length (a stored code that does not parse is
left out), the reference is asked where the song streams only while online is reaching, and the
answer is `Shared::written`, `None` where neither a service nor a recording gives a link; both phones
call it.

Monochrome, hifi-api, Subsonic and the listener's TIDAL account use the existing provider matching
and delivery code. Credentials and recognition keys are encrypted with AES-GCM under Android
Keystore. TIDAL device sign-in displays the code and authorization page, and the renewed refresh
token is saved on the TIDAL account. The user chooses a write-granted music folder per download. A data-sync
foreground service retains the engine and offers cancellation; a download waits at most ten seconds for
that service to start and then fails with a notice, so one that never starts cannot hold the
download slot (`DownloadQueue.SERVICE_STARTS_WITHIN_MS`). Choosing Downloads writes its preference on the IO
dispatcher, never the main thread. Background tasks of `LibraryFeatures` share a
`CoroutineExceptionHandler` that logs and says the failure, and `Resonate.attempt` catches every
native failure, a `LinkageError` included, so a screen never waits for ever.

**Download accounts are a list** (`ProviderAccountBook`, `ProviderAccountRecords`, `ProviderAccountStore`):
any number of Monochrome, hifi-api, Subsonic and Jellyfin accounts, each its own row with its own switch, and at
most one TIDAL account, because the boundary hands back one renewed refresh token
(`refreshed_tidal_token`) that cannot say which account it renews. An account is asked when it is on
and ready (`Readiness`, iOS's `DownloadAccount.readiness`: Monochrome and hifi-api always, Subsonic and Jellyfin
with a server, user and password, TIDAL with a client ID and refresh token; a Jellyfin account is
browsed and played in place and never asked for a want, the boundary leaving it out of `find_download`), and `find_download`
takes every such account in list order, which only breaks ties, the boundary racing them; there is
no order of the listener's, as on iOS and the desktop. The stored form is version 2, in the
`provider-accounts` preferences: `accounts-version`, `accounts-ids` (the ids in order) and per id
`account-<id>-provider`, `-server`, `-on` in plain text and `-user`, `-password`, `-client-id`,
`-client-secret`, `-refresh-token` each sealed with AES-GCM under Android Keystore as before. Every
write replaces all `account-` keys in one commit, so a removed account leaves nothing behind. The
first read of an earlier store migrates it in one commit: the single `provider`, `server` and sealed
fields become account 1, copied without being opened, and a store with no account at all starts as
one hosted Monochrome, as the single account's default always asked it; an empty list written later
stays empty (`ProviderAccountRecordsTest`, `ProviderAccountStoreTest`). Every change is
`ProviderAccountStore.change`, a read, an edit and a write under one lock, whether the listener's
(`LibraryFeatures.keepProviderAccount`, `turnProviderAccount`, `removeProviderAccount`) or a
renewed token's. Settings › Download sources lists the accounts with their switches and what each
needs (`accountSummary`) and offers *Add a source* per kind; an account opens in its own page
(`Screen.ProviderAccount`: an id, or none for a new one that is stored only on *Add*) with its
fields, TIDAL sign-in and *Remove* (confirmed). *Ask the hosted services* stays one switch for all. A renewed refresh token reaches an open account page as that field alone once the listener has typed into the draft; a draft still equal to the stored account follows it whole.

**A download is a job** (`downloads.rs`), several at once: `begin_download(song)` answers its
number, `find_download(job, accounts)` races every account it is given through `Providers::first` (registration
order, `TURNED_TO_APART`, the grace, `ANSWERS_WITHIN`) and opens the offer taken, falling to the
held-back offers in rank order where one is `Gone` or refused, so the `DownloadOffer` names the
extension, the answering `MusicProvider` and the `Opened::Reading` length before the front end
makes a document. `download_offer(job, descriptor)` copies into it; `index_download(job, uri,
name)` validates and indexes. `download_progress(job)` answers `DownloadProgress` (`DownloadPhase`
Finding, Downloading, Validating, Done, Failed, Cancelled; bytes received; the total where the
provider said one; the provider); `cancel_download(job)` is heard between provider answers, while an
offer opens (on its own thread, looked at every 50 ms) and between 64 KiB chunks, and a cancel
pressed before the find stays (nothing resets it). Shutdown cancels every job; at most 16 settled
jobs are kept. **A found offer waits for its destination at most `OFFER_HELD_FOR` (2 min)**: a
watchdog thread per offer drops the provider's reader (and its connection) and fails the job if
`download_offer` has not taken it, so a front end that cannot create a document leaks neither
(`download_offer` then answers `DownloadExpired`; `an_offer_nobody_takes_is_let_go_and_its_job_fails`).
Taking the offer, a cancel or a settled job ends the watchdog at once. A failure is a typed error whose message is the sentence the app shows, naming the
provider and cause (`NoProvider`, `NotFound`, `Unreachable`, `ProviderAway` (also a provider left
behind unanswered), `ProviderRefused` with the status, `AccountRefused` with the code,
`ProviderUntrusted`, `ProviderMisaddressed`, `ProviderUnreadable`, `WrongSong`,
`DownloadCancelled`); with nothing found, the first provider failure is told before `NotFound`. A
delivery `index_download` refuses as `WrongSong` is declined by every later find of that song for
`REFUSALS_REMEMBERED_FOR` (30 days, the desktop's), across restarts: `refused_offers` keeps it
by the song's wanted identity and the place it was taken from, and each refusal written sweeps the
rows past that age.

**Android runs the jobs as a queue** (`DownloadQueue`, its arithmetic `DownloadPlan`): `download(songs,
folder)` adds a job a song at a time (a song already waiting or running is not added again) and at most
`AT_ONCE` (2) run together (every change to the jobs is one `update`, and a pump claims the jobs it
starts as *Finding* in that same update, `DownloadPlan.claimed`, so two pumps never start one job), each its own `begin_download`, `find_download`, document, `download_offer`,
`index_download` and publish; the first to start asks for the foreground service and every job waits
for it (10 s). A cancel stops one job, Cancel on the card all: a waiting job is cancelled at once and a
running one is marked `cancelling` (`DownloadPlan.cancelled`), which its run checks after the service
starts and right after `begin_download`. The service clears `transferServiceRunning` before it asks
to stop with the latest start's id, so a job asking afterwards starts it again rather than trusting
a service on its way out. The card, notification and `downloading`
read the jobs: one says *Finding*, *Downloading* or *Downloaded name · KiB*, a batch *Downloading 2 of 3 ·
…* and then *Downloaded 3 of 3 songs*, naming the first failure. Each created document is journalled in
`unfinished-downloads` (a set; the old single `unfinished-download` is still recovered) until its job
is done, so a crash deletes each half-made one; `recover` reads `song_uris` first (and touches nothing if that fails) and deletes only a journalled
document the catalog does not hold, publishing one it does, since an indexed song is a finished one: the job drops its
"created" handle the moment `index_download` returns, so a failed publish cannot delete it, and the sync landing's
`unfinished-sync-landing` is settled the same way (`DownloadPlanTest`). *Remember this folder* in the chooser keeps a folder
(`download-folder`), after which a download goes straight there; **Saved for later** offers *Download N missing
songs* and *Download saved songs automatically* (on unless switched off, `download-wants-automatically`).
Automatic asking runs only while the Activity is visible, online is on and the network unmetered,
a provider is set up and the remembered folder is readable and writable: it waits for
`next_wanted_due` (30 s at least, 30 min at most), asks `poll_wanted_due`, and enqueues what is not already
held, not asking one song again for 30 minutes (`DownloadPlanTest`, `DownloadQueueTest`).

**A download that breaks off resumes from the bytes it kept, on both phones** (`partials.rs`, `Downloads::under`).
A stream offer whose `Opening` is resumable (Subsonic, Monochrome and a keyed hifi-api, whose servers answer
`Range`; TIDAL's segments and a remux cannot, and stream straight into the document as before) is written
to `download-partials/` beside `documents.sqlite` as it arrives, and only copied into the destination
document once whole, then removed, so a cancel, a failure, a lost network or a killed app costs the
document and nothing else (and a destination that refuses the copy keeps the bytes for the retry). The
partial is `<file stem>.<crc32 of wanted_key>.resonate-part` (`naming::named_within`) with a
`.resonate-note` beside it (same length of suffix, so both cut alike) holding the wanted key, the offer's
`taken_from`, the whole's length and its validator (ETag, else Last-Modified); a note that does not read
means no partial. **`find_download` claims the partial under `Held::resumed` for the whole job** (a second job for the
same want finds it held and streams straight, the sweep finds it living), and opens the offer with
`Opening::open_resuming(Held { bytes, total, validator })`: a partial is asked for only if it is of the same
offer (`taken_from`) and can be checked, by a strong validator (sent as `If-Range`) or a known length
(a `206` must start at the byte held and name that total). A `200` (a server ignoring `Range`, or a validator
that no longer matches), a mismatched `Content-Range` or a `412`/`416` begins again from byte 0 and empties the
partial; `DownloadOffer.total` is always the whole's length and `DownloadProgress.received` starts at the
bytes kept. The offer found `Gone` and a download over the eight GiB limit delete the partial; a stream
ending short of the known length is a failed read, not a finished file. A partial untouched for
`STALE_AFTER` (7 days) with no living writer is swept when the player opens
(`a_download_cut_off_part_way_resumes_with_a_range_and_yields_the_same_bytes`,
`a_server_ignoring_range_restarts_cleanly_from_the_start`,
`an_offer_that_changed_discards_the_partial_and_starts_over`,
`a_partial_of_another_offer_is_never_ranged_into`). Android has no front-end change: its document is still
created, journalled and deleted as before, and a retry (a new job) finds the partial.

Only a fresh SAF document or pending Downloads row is opened for writing; its URI is journalled
before streaming, bytes are bounded at 8 GiB, and codec probe/duration validation (within
`LENGTHS_AGREE_WITHIN`) precedes
catalog insertion. Failed or interrupted deliveries delete only that newly created document,
including journal recovery on the next start. Validated deliveries fill the song's recording,
release, artist id, disc and position where the document names none, and missing catalog names,
without writing tags, so an untagged file is still recognised as a held want; the wanted row it
answered is stamped `offered_at`. A rescan preserves those names when the document supplies none.

**Text searches share the desktop's ranking before any limit**: `searched_songs` hands every
recording returned by `find_songs` to `resonate_library::ranked_songs`, including the title/artist
reading, grouping, original-versus-variant fit, distinct release counts and compatible ISRCs.
Only then are the best twelve mapped to `SearchSong` and their local holding checked. Taking
fifty in service order discarded originals further down and let remixes lead on both phones.
`an_online_search_ranks_every_answer_before_choosing_the_songs_shown_on_the_phone` puts the
original at position ninety among a hundred variants and checks its identity, code and place.

**Found songs carry their place and whether they are held.** `SearchSong` holds release, release
group, artist id, disc and position beside the recording; `search_song` places a found recording on
the release whose title holds the typed words the title and credit do not, else the first
`in_the_order_worth_offering`. Every online song listing marks `held` through the wanted rows'
`local_match`; a text or song-link search keeps one of the takes sharing a folded title and artist
within `A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY`, the one naming most identity, in first place; release
and playlist listings keep their repeats. A find for a wanted song notes the try on its row as the
desktop does (`tried`, `misses`): an offer zeroes the misses, nothing heard from every provider
counts one, a refusal or cancel neither; `poll_wanted_due` answers the unheld wants due by the
desktop's `Want::due_at` (`RETRY_WAITS`, `POLL_AGAIN_AFTER`, given up at `TRIES_BEFORE_GIVING_UP`),
and `next_wanted_due` the earliest of those times among them (unix seconds), for a front end's timer.

## Services played in place

**The Server page browses and plays every service that plays in place** (Subsonic, Jellyfin or Emby, TIDAL,
the keyed Resonate server, hifi-api and Monochrome; `mobile.md`'s *Services played in place*; `Served.kt`,
`ui/server/`). `Served` (`LibraryFeatures.served`) derives the accounts from the download accounts,
the online switch and *Ask the hosted services*: every account that is on and ready
(`ProviderAccountBook.asked`, the list `find_download` is given), less a Monochrome or hifi-api
account with no server while hosted services are declined (so a toggle changes the list handed
over), and none while online is off (`servedAccounts`). `Served.begin` hands each change to
`serve_from` (so the slot the player's own `Sources` read on every open follows the accounts) and
publishes `served_services` as `Served.services`; every browse binds again first (`Resonate.asked`,
`serve_from` being idempotent for the same list), so a read never races the collector. **It is a page of Settings, not a root, as
on iOS and the desktop**: `Setting.SERVER` (*Online & services*) opens `Screen.Server` and is listed
only while `Served.serving` (at least one service; `SettingsScreen` filters the matches), and when the
last service goes `ResonateApp` cuts every stack at its first served page (`Navigator.leave`,
`Screen.isServed`: the Server page and the served artist and album pages). **The page is one service at a time**:
`FilterChip`s under the search field choose it, shown only where more than one is served;
`ServerViewModel` keeps the scheme picked and `chosenService` answers it while that scheme is still
served, else the first (`ServedChoiceTest`). The root shows `served_first_page` of the chosen one
(Subsonic's artists, TIDAL's favourite artists, albums and songs, nothing for the rest, which then
says to search) in sections as search results are (artists, albums, songs), and the field asks
`served_search(chosen, words)` on the keyboard's search, both read again when the choice moves. Its
pages are `Screen.ServedArtist` and `Screen.ServedAlbum`, each a `CollectionPage` over a `Loaded`
read (`servedRead`: loading, ready, or the failure reported on the notice with *Try again*, the
boundary's sentence, which also says a browse a service does not offer; changed accounts read
again); an artist page is its header (Play and Shuffle over the artist's songs) and a shelf of
album cards; an album page is the library album's, numbered by the service's track number, a row
playing the album from it. Monochrome and the keyed Resonate server are searched for songs alone.
Every row, card and page has *Play*, *Play next* and *Add to queue* in a menu; an album or artist
is expanded to its songs off the main thread (`served_song_uris`, in `ServerActions`) before the
controller or the queue is given them, since a network call must not hold the ordered worker. **A
served song plays through the controller as a library one does**: `Playback.playServed` sets media
items whose id is the song's uri, `RustMediaPlayer.handleSetMediaItems` reads each id as a
`QueueSong` (`BrowseIds.queueSongOf`: a served uri, `servedMediaId`, else a library id,
`BrowseIds.trackOf`) and calls `load_songs`, so a list another controller sets keeps its served
rows beside its library ones. **A served row has
id 0**, told by `Track.served` (id 0 and a uri of one of `SERVED_SCHEMES`: `subsonic:`, `jellyfin:`, `tidal:`,
`resonate:`, `hifi-api:`, `monochrome:`); everything keyed by song id keys it by `Track.pictureKey`
(its uri) or `Track.rowKey` (its queue row) instead, and `findTrack(0)` is nothing. A queue holding
served rows is saved and resumed by the boundary like any other. Its cover is `Served.bitmap`
(`served_cover`, four at a time, decoded at the bucket drawn into a 24 MiB `LruCache` keyed by uri
and bucket, a failure (Monochrome has no covers) remembered until the accounts move and leaving
the placeholder) drawn by `Artwork`, `WideArtwork` and Now Playing through `rememberCover`, and the
session's thumbnail is the same bitmap (`artworkBitmap`), so the media notification, the lock
screen, the mini-player and the widgets show it. A served row's song sheet offers only the player's
own entries and *Remove from queue*; the player's lyrics button, favourite heart and the title links
are off; *Save queue as playlist* leaves served rows out (`ServedTest`, `ServerWordsTest`). **A served
song is counted and starred** (`mobile.md`): its listens are in the statistics with no change here, and
a song row whose `ServedSong.favourite` is not null draws `ServedStar`, a star toggled at once and put
back with the notice if `favour_served_song` fails (`ServerActions.favour`); Now Playing's heart is on
for a served row only where `marksFavouritesOf` finds its service's `ServedService.favourites`, and
`Resonate.favourite` sends a served row to `favour_served_song` instead of the catalog, with no *Add to
playlist* offer (`ServedTest`).

## Device sync

`sync.rs` is the bridge to `resonate-sync`: `DocumentCatalog` implements its `Catalog` over the
document catalog, and `MobilePlayer` holds at most one running `Sync`, started and stopped as a
whole (`sync_start` leaves the previous one first; `shutdown` stops it before anything else). Its
ledger is `sync.sqlite` and its staging folder `sync-staging`, both beside `documents.sqlite`.
A blank key mints one; `SyncStatus.code` hands it back and Kotlin stores it encrypted beside the
provider accounts, with the other choices (enabled, server, device name, the three follow switches,
mobile data, `sync-nearby-only`) in plain preferences. Sync is on unless switched off and runs
only while online is, unless `nearby_only` (*Only this network*: `Server::Nowhere`, no server
dialled, which runs with online off); `sync-nearby-only` defaults off, so the hosted server is dialled. A setting change restarts it; `sync_start` with a blank key
reuses the code of the sync it replaces, so two starts in a row never mint two codes.

A song is found by recording id, else by folded artist, title words, album when named and a length
within 2 s, the desktop rule; the lookup reads an in-memory index rebuilt whenever the catalog
revision moves. That revision is an update hook over `documents`, `playlists`, `playlist_rows`,
`playlist_queries`, `received` and inserted or deleted `listens`. **The phones keep saved queries**
(`keeps_queries`): `DocumentCatalog::playlists` lists only the playlists with no `playlist_queries`
row and `queries` the rest, `write_query` refuses a sort or reading code `SortOrder::coded` or
`Direction::coded` cannot read (`Refused::Unwritable`) and makes a new playlist where `at` names a
list, as `write_playlist` does where it names a saved search
(`a_saved_query_from_another_device_is_held_as_a_saved_search_apart_from_the_lists`). A synced playlist write replaces its rows, takes a free
name and steps `playlists.revision`, so an undo step from before it is refused as changed. Only this
device's counted listens are offered, the newest one only once 15 minutes old; another device's
listen is kept with `elsewhere = 1`, counts in history and statistics and is never scrobbled.
A holding's size is reported as 0, since a document row carries none; a song is served through the
`content` media provider's own descriptor.

A song from another device cannot be written through SAF from Rust: `land` moves the staged file into
app-private `sync-received`, notes it in `received` and answers that it landed, and `holds` counts a
waiting song as held so it is not asked for twice; a song `land` finds already held (indexed or
waiting) is answered landed and its staged copy dropped, never queued a second time. **A song landed is held by what was asked for, never fetched again**: `sync_receive` notes each key of the recording the sender announced against the document it indexed (`note_received_as`), and `holds` and `open` look a recording up by name first, then through `songs_received` (`DocumentCatalog::candidates`; a synced playlist write, a favour, a credit, kept words and `held_as` resolve the same way, `resolved_in`, so such a song is written into its playlists and keeps its favourite, `a_song_landed_under_the_names_of_its_own_file_stays_in_its_playlist_and_favourites_and_settles_once`), so a file whose own tags name the song otherwise than the sender's catalog does (a corrected title, a missing album) is neither fetched on every follow nor refused as `Missing` when that sender asks for it back (`a_song_landed_under_the_names_of_its_own_file_is_held_by_what_was_asked_for`). **Every row a recording resolves to is weighed** (`Index::resolved` answers each MBID and name match, `received_as` each noted row), and a row holds the song unless its document reads `NotFound`: one refused or out of reach (an SD card or USB drive away, an offline File Provider, a transient `document_refused()`) is held, as `hide_missing_songs` keeps it visible, and a row hidden because its folder was withdrawn is held without being opened, so neither an unreadable copy hiding a readable one nor a folder away has its songs fetched again into another (`a_song_out_of_reach_for_now_is_held_and_only_one_not_found_is_fetched_again`, `every_copy_a_song_resolves_to_is_tried_so_an_unreadable_one_hides_no_readable_one`). **A song is known by its file, however it was asked for**: `land` probes the received file (`probe_scanned`'s packets digest, frames and rate) and, where a visible document whose file stands is that file (`answers::the_same_file`, probing a document of that length whose identity is not kept, as `take_study` does), notes the asked keys in `songs_received` against it and lands nothing; where a waiting song is that file, the keys go into `received_also` and the second copy is dropped, so a renamed request or a second sender's spelling neither fetches nor queues the song again, and `holds` counts those keys as waiting (`a_song_asked_for_under_another_spelling_is_known_by_its_file_and_not_landed_again`). **A landing is one transaction**: noting every key of the waiting song (its own and `received_also`'s) and deleting its `received` row commit together, and `sync_received` first drops a waiting song already held through `songs_received`, by its file, or by a visible row its recording resolves to (`Index::resolved`, read once a poll, the match `holds` trusts), so a kill between indexing and forgetting never lands it twice and a download or scan of another encode indexed while it waited is settled onto rather than copied beside (`a_song_indexed_before_its_waiting_row_was_forgotten_is_not_landed_twice`, `a_waiting_song_another_encode_of_which_was_indexed_meanwhile_settles_onto_that_row`). `sync_receive` itself does not check again: the phones create the destination document before calling it, so settling there would leave an empty document. **A file of the same bytes or the same audio already standing at the landing name is the song**, as the desktop's `filed::Landed::Standing`: the front end hands `sync_receive_standing(id, standing)` the documents of the destination folder whose names begin with the song's stem (`StandingDocument`, its URI and name); the boundary keeps those named as the landing name or a numbered copy of it (`name (n).ext`, `named_alike`), compares each byte for byte with the waiting file through the document provider, else probes it for the waiting file's packets digest, frames and rate (a sender whose copy carries other tags), and indexes the first alike as `sync_receive` would, answering `None` where none is, after which the front end creates a document as before (`a_song_whose_bytes_stand_at_its_name_already_is_taken_as_that_file_and_not_copied_again`, `a_song_whose_audio_stands_at_its_name_in_other_bytes_is_taken_as_that_file`), so a reinstall that lost the catalog but kept the files lands nothing beside them. A document made for a landing, a download or a sync is created under the mime type the platform maps its name's extension to (`MusicDocuments.mimeOfName`), since a provider given another type for the same extension appends its own (`song.flac.flac`). Kotlin polls `sync_received`, asks `sync_receive_standing` with `MusicDocuments.standing` (a SAF tree's root children, or Downloads' rows), and only then creates a document in
the chosen writable music folder (the first writable one other than Downloads unless chosen; `SyncLanding.destination`, `SyncLandingTest`), **never another folder where that one is out of reach** (the songs wait, the notice saying so). **A destination opening lands the songs held back at once and says so**: the verdict that held them (`Away` or `Nowhere`) is kept, and when choosing a folder on the sync page (Downloads among them) or a folder list read again makes the destination reachable (`SyncLanding.opened`), `landingMayHaveOpened` clears the notice, wakes the poll (`pollSoon`) and says *Fetching N songs from your devices into <folder>* (`SyncLanding.beginning`, a plural); where the sync is quiet (`SyncLanding.followsAgain`: on, nothing moving, settled) it also asks `sync_force`, so songs refused earlier are followed again, and a sync running is left to carry on, never started twice. While songs land, `syncKeeping` counts those left and the sync page shows *Keeping N songs from your devices* under Transfers with the folder, and the *Kept N songs* that ends a landing the listener opened is said whatever *Tell me what the library finds by itself* says (`SyncLandingTest`). It journals its URI as
`unfinished-sync-landing`, and hands the fresh descriptor to `sync_receive`, which copies the bytes,
validates and indexes the document as a download is, and forgets the waiting song once it landed; a
failure leaves it waiting (`a_song_that_fails_to_land_waits_for_the_front_end_to_try_again_or_give_it_up`).
**A failed landing holds back that song alone**: it deletes only the new document and waits a
minute before that song is tried again, the rest landing meanwhile; a song refused
`LANDING_TRIES_AT_MOST` (3) times in a row is dropped with `sync_refuse_received`, which tells the
sync service (`Sync::refused`) so the song is not fetched again at once. A received song's
file name drops `/ \ ? : * " < > |` and control characters, so a FAT, exFAT or SMB folder takes it. A
download's file name does too (`downloads::file_stem_of`, the one stem both use).
`SyncedDevice.id` names a device (the hex `DeviceId`; names repeat after a reinstall), which
`sync_forget_device` takes; `HandedOff.id` likewise, and `sync_continue_from` and
`sync_fetch_missing` take the id (a name is still read where no id matches). The sync screen's
pairing code stays hidden until *Show pairing code* is pressed; its text is absent from the
accessibility tree while hidden. **A computer is paired by the system camera or any QR scanner**: the manifest's `MainActivity` filter takes `VIEW` of `resonate://pair` (`DEFAULT` and `BROWSABLE`, which a scanner needs), `opening` hands the link to `LibraryFeatures.offerPairing` (`sync_code_in_link`), and `PairingOffer` asks *Join these devices?*, naming the code, before `joinSync` (which also turns sync on; one made before the settings are read waits in `joinWaiting`) and opens the sync page, as iOS does. Resonate never touches the camera. `song_uris` lists every document's URI, hidden ones included.

`ACCESS_NETWORK_STATE` lets the default-network callback report `NET_CAPABILITY_NOT_METERED`; songs
move only on an unmetered network unless mobile data is allowed. The same callback
(`LibraryFeatures.watchNetwork`, registered at start and unregistered in `stop`) tells the boundary
the network through `ConnectionMeters.of` and `report_connection`; *Look up on a metered connection*
(`enrich-on-metered`, Library tools) and *Wait on a metered connection* (`stall-downloads-on-metered`,
Download sources) are handed to `configureEnrichOnMetered` and `configureStallDownloadsOnMetered` at
start and on a change, and `tick` reads `downloadsWaitOnTheMeter()` while a download runs so the
download card says *Waiting for a network that is not metered* (`DownloadPlan.meterLine`). While sync runs Kotlin holds a
`WifiManager.MulticastLock` (`CHANGE_WIFI_MULTICAST_STATE`, released when sync stops and when the
repository closes), so beacons and Bonjour from devices on the same network are heard (`sync.md`'s *Nearby*); the status text counts them when the server is away and a
playlist's menu offers fetching from a nearby device whatever the server's standing. Kotlin polls the status every two
seconds while the Activity is started (30 s otherwise), refreshes the library and playlists when
`SyncStatus.arrivals` moves, and keeps the data-sync foreground service while songs move or wait. A landing that kept songs says *Kept N songs from your devices* only when that same switch is on.

**Sync goes on with the app closed** (`BackgroundSync.kt`). The engine lives only while something
retains `LibraryRepository` (playback, a download, a party), so once the last owner lets go sync
stops with it. `SyncJobService`, a `JobService`, is that owner in the background: it retains the
library (starting the process and engine where none runs, and marking the features not in the
foreground where no Activity is attached), runs `LibraryFeatures.syncUntilQuiet`, then releases it
and calls `jobFinished`. Two jobs start it. **The wake** (`PERIODIC_JOB`) runs every 15 minutes
within 5 (`WAKES_EVERY_MS`, `WAKES_WITHIN_MS`) on any network with the battery not low, persisted
across a reboot (`RECEIVE_BOOT_COMPLETED`); `syncWith` keeps it scheduled while *Sync in the
background* (`sync-in-background` in the `library-features` preferences, on by default, the sync
page's switch), sync and online (or *Only this network*) are on, and cancels both jobs otherwise. A
wake already scheduled with the same period is left alone, since scheduling again would restart its
clock. **The carry-on** (`CARRY_ON_JOB`, one-off, any network) is scheduled by
`MainActivity.onStop` through `carryOnInBackground` when sync is not quiet or songs wait to land, so
leaving the app mid-transfer neither freezes nor ends it; one already pending is never replaced,
which would stop it running. A pass makes `pollSync` poll every two seconds (and at once, through
`pollSoon`) and publishes each poll's status after landing what arrived (`SyncPass`, stamped when
the status was read, so the pass reads no status from before it began). **A pass ends when sync is
quiet** (`BackgroundSync.quiet`: off, or nothing moving and either `settled` or nobody to meet,
neither dialling nor connected and no other device online or nearby) for `QUIET_FOR_MS` (6 s), and
it has listened at least `HEARD_FOR_AT_LEAST_MS` (20 s, long enough for beacons and Bonjour); it
gives up at `RUNS_AT_MOST_MS` (8 minutes, inside the system's 10), asking to be rescheduled only
while songs still move. A pass starts no data-sync foreground service, since the job already holds
the process, and never forces sync, since *force* forgives deletions (`sync.md`'s *The service*)
(`BackgroundSyncTest`). On the emulator a forced run (`adb shell cmd jobscheduler run -f
org.resonate.android 7101`) cold-starts the process, dials and finishes alone after 20 s.
Playlist menus offer fetching their missing songs from each online device and keeping their songs
here; the queue offers continuing what another device was playing, starting it through the Media3
controller when that device was playing.

**Another device is driven from its row** (*Drive*, `DrivenDevice.kt`: transport, volume, seek, and jump, move and remove by row) **and this one is driven by another** as far as *What your other devices may do here* lets it (`sync-control`, `SyncControls`; a sound begun from elsewhere asks audio focus, `soundBegan`; `sync.md`'s *One device drives another's zones*).

Remaining desktop capabilities are open work in `docs/FEATURES.md`.

## Checks

From the repository root, `android/gradlew -p android :app:assembleDebug :app:lintDebug` builds
both native ABIs and checks Kotlin and the manifest, and `:app:testDebugUnitTest` runs the plain
JVM tests (`android/app/src/test`: queue edit arithmetic, queue read pacing, the loves sentence, the settings search, the search words, the lyric lines, the accents, the organising of a tree against a fake one).
`python3 android/check-native.py --sdk "$ANDROID_HOME"` checks ELF/APK alignment and runs
clippy for both native targets. With an emulator or device connected,
`android/gradlew -p android :app:connectedDebugAndroidTest` exercises native loading, document
identity, decode, playback, seeking, resumption, sound-setting restoration, sleep timers,
metadata/artwork/sidecar lyrics, analysis, filtering, duplicate playlist/queue rows, listening
thresholds, offline enrichment, wants and playlist import, a loopback provider download,
MediaStore Downloads publication and stable rescan identity, encrypted account storage,
favourites, playlist edit/undo, navigation, Activity recreation, Media3 background controls and
notification, task-dismissal pause, audio focus and noisy-output events, a controller's queue
edits, the service's destruction giving the repository back and a failing poll tick said once. CI uses API 33 (the minimum) and API 36 x86_64
emulators; the local Pixel_10_Pro AVD additionally exercises 16 KB pages. Real hardware
remains necessary for Bluetooth, USB DAC and device-fidelity claims. UI checks clear the
accessibility cache before each poll (`forgetTheCachedTree`): an API 36 screenshot can show current controls while the
cached tree still reports the preceding value. `UiAutomation.clearCache` is API 34, so below it the
cache is cleared by setting the automation's own `serviceInfo` again, which clears it first; left
alone on API 33, the settings list's cached node stopped reporting a row once the tree moved under
a swipe and `UiSmokeTest` scrolled past *Online features* for good. All write tests use copies.

## Together

`Together.kt`, `PartyService.kt`, `InvitationLinks.kt`, `ui/together/` and `ui/wrapped/` hold shared playlists,
parties and the deck. `party_shown` is polled off the main thread, since a press on the boundary may wait
on it. `PartyService` keeps a party in the foreground whatever playback does; dismissing the task leaves
it, since a guest would otherwise go on playing unseen. `resonate://shared`, `resonate://party` and App Links
for `https://resonate.tecnio.dev/s/` and `/p/` all reach `accept_invitation`; the links verify against
`[web] android_certificates` on the server, which names the release key, so a debug build shows them
unverified. The QR is `com.google.zxing:core` (pure Java), since pairing draws none. The typed name is
`shared-name`. The card is drawn through a graphics layer to a PNG for the system's sheet
(`shared-playlists.md`, `party.md`).

## Accessibility

- **One set of helpers, in `ui/components/Accessibility.kt`.** `rememberCalm()` re-reads on resume and on
  touch-exploration changes (animator scale zero or a reader exploring means calm); `rememberTitleMaxLines()`
  gives titles two or three lines above font scale 1.3 (Now Playing's action row wraps to two columns
  there); `recommendedTimeoutMs` runs a notice's time through `AccessibilityManager`; `outlinedTrackColors()`
  and `bannerTrackOf` draw inactive tracks at 3:1.
- **A gesture always has a switch-access equivalent.** `Modifier.reorderActions` (`Reorder.kt`) gives the
  queue and playlist rows Remove, Move up and Move down; the drag handle is a button named *Reorder
  <title>*. The scrubber and volume take `stateText` from `CommitSlider` (*1:23 of 4:56*, *40 percent*) and
  the scrubber ten second skips; `PlayerKeys.kt` binds Space, media play-pause and the arrows on Now Playing.
- **Now Playing hides what is under it.** While open it has a pane title, the screens behind are hidden from
  accessibility, and the snackbar host is drawn above it. Pages carry a pane title and a heading.
- **Contrast follows the system.** `schemeOf` and `ArtworkTheme` take `UiModeManager.contrast` (API 34) joined
  with the stored *High contrast* switch (the higher wins); Now Playing drops its cover wash at high contrast.
  Lyrics' quiet ink is `QUIET_ALPHA` 0.85 with the active line bold, held by the contrast test.
- **Held by** `ThemeContrastTest` (JVM: every accent in light, dark and pitch black, standard and high contrast;
  text pairs 4.5:1, non-text pairs 3:1, quiet lyric ink 3:1 on four grounds, body text 7:1 at high contrast) and
  the Library shelf tabs end in an ellipsis rather than a clip (the sweep reads a `maxLines` cut with no ellipsis as clipped, and the row scrolls above font scale 1.3), `A11ySweepTest` (instrumented: the Compose accessibility checks over the root tabs and settings, a
  `fontScale = 2` pass for clipped text and a pass for unnamed clickables; the sweep needs an emulator). **Clipped is a line wider than its box or lines cut off with no ellipsis** (`isClipped`, `ClippedTextTest`): `didOverflowWidth` weighs the box against the paragraph's layout width, so it reads every label narrower than its container as overflowing.

Share lookup failures appear as a notice; coroutine cancellation still propagates.

Artwork lookup without a recognition key never repairs missing audio measurements. Band measurements
are decoded only by an opted-in recognition pass or an explicit analysis or equaliser fit, so opening
the app to fetch covers does not spend battery decoding a previously studied library. Metadata progress
counts an item when it starts, including a slow first album, rather than leaving the row at zero
until every request for that album has completed.

A metadata pass that fails cancels its picture, lyric and study workers before joining them,
keeping the original failure and its resumable mark. A network failure must not leave the pass
shown as running while an otherwise unrelated library decode continues. Cleanup only supplies
its own cancellation when the caller has not already stopped the pass; a listener's Stop removes
the mark, while shutdown preserves it. Overriding that choice made every explicit Stop resume.
