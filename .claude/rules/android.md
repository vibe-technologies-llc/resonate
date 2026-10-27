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
`MediaButtonReceiver`, which starts it; `onPlaybackResumption` waits at most 8 s for the engine, the
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
the app's own tap does; a plain catalog id passes through. Every id a controller sets or adds is read
by `BrowseIds.trackOf` (a folder song or a plain id; anything else is left out and the start index
follows the song it named, `BrowseIds.playable`), and a song no longer in its folder (a search result
listed under the root, a playlist past row 500) plays alone. A voice request (`requestMetadata.searchQuery`,
or `MEDIA_PLAY_FROM_SEARCH` on `MainActivity`) plays `search_library`'s first 100 songs. Android Auto, the
Assistant, the car-assistant and the Auto simulator connect as controllers beside the trusted ones
(`BROWSING_HOSTS`), and `automotive_app_desc.xml` declares the media app
(`BrowseIdsTest`, `LibraryBrowseTest`).
Audio focus loss and noisy-output events pause playback; a transient loss resumes on regaining
focus if the engine was playing or loading, and a duck is left to the platform
(`setWillPauseWhenDucked(false)`). The Media3 state is invalidated only when the transport, queue
place, repeat, shuffle, volume or song moved, or the position jumped more than a second from where
the last snapshot and the time since put it (`positionJumped`: a seek or a restart within the song,
which the system session would otherwise go on extrapolating past); the position is read live from
the latest snapshot, so *previous* past 3 s restarts the song and the notification's bar moves, and its playlist is
rebuilt only when the queue or the cover moved. An Activity's recreation does not own or close the
audio engine. Playback and download services each retain the repository; the last
release cancels its workers and saves and closes the engine off the main thread, then clears the
published snapshot, so a service made afterwards never draws the playing state the closed engine last
had (`PlaybackLifecycleTest`). A later service
waits for that cleanup before opening another. Closing replaces the engine's deferred with a fresh one,
so nothing awaits a closed native player; a native library that will not load fails it with an
`IllegalStateException` and the repository's scope logs anything else that escapes. A seek within
the current song only seeks, and a seek to another row compares against the place read on the main
thread when it was asked. `refresh()` reads on the IO dispatcher whoever calls it. The snapshot's `waiting` (`mobile.md`) becomes the status when it begins and is cleared when it
ends, if still shown; the expanded player shows it in place of the format line.
`LibraryRepository.place` is the position the poll last read with the time it was read at; the player's position (`PlayerViewModel.position`: scrubber, mini-player progress) runs from it every 50 ms while the transport plays, clamped to the duration, and is corrected by the next poll at a seek, a skip or a pause. The scrubber's draft and drag are keyed on the playing queue row and a release commits only for the song it began on.
The 250 ms poll catches a failing tick (a snapshot, queue read, save or listening count refused by
a full disk) and says it once per distinct message, so one error never ends the loop; a loop that
ends for any other reason clears `started`, so the next retain reopens the engine. The queue is read
only when `Snapshot.queue_revision` moved, or `catalog_revision` moved and three seconds have passed
since the last read (`QueueReads`, as iOS's `catalogRereadAfter`), since every enrichment step, scan
upsert and favourite moves the catalog.
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
(`clearPreferredMixerAttributes`). The callback is typed by the spec (`f32`, `i16`, `i32`, mono or
stereo); 24-bit packed has no Oboe-rs type, so it is never offered. `output_stream()` says what is
open (`OutputStream`), which the page shows as *Current stream* (`outputStreamWords`,
`outputPathWords`) under *This output takes* (the route's `AudioDeviceInfo` rates, depths and channel
counts, `outputTakes`); the switch and an offer that moved send `Command::FollowTheFormats`, so the
playing song reopens at once where its plan moves, and every later song opens at its own rate. The
emulator offers no exclusive stream and no bit-perfect mixer, so it proves the mixed fallback alone
(`TESTING.md`). DoP is not offered (`FEATURES.md`). The output thread waits on its commands as
`output/waits.rs` says: 10 ms while draining, 100 ms while streaming (the latency is read again
each time) and 500 ms otherwise, in which a device error is still noticed. An open waits at most
`OPEN_WITHIN` (3 s) for the stream thread, so a stalled `open_stream` on a route change is a
timed-out open the engine parks, not a blocked engine thread, and the late stream closes itself
(`an_open_that_never_answers_times_out_instead_of_blocking`).
The output pane lists `AudioManager` devices and sends a chosen device id to Oboe before
rebinding; system default is id zero. Device type distinguishes outputs that share a product name.
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
sequence past the highest `submissions.through`; step 4 gave `documents` an `added` (Unix seconds) that the `documents_dated` trigger sets on every insert, which the search index reads for `added:`; step 5 dates the rows step 4 found undated by their file's modification time (`modified` in ms, capped at now) and, for one never stamped, by the migration's own time, so no row is undated; step 6 gave `studies` an `agreement` (what `resonate_library::agreement` said of the audio against the song's name) that the `documents_forget_what_was_heard` trigger clears when a document's `size` or `modified` moves; step 7 made `playlist_queries` (`playlist` keyed to `playlists(id)` with `ON DELETE CASCADE`, `text`, `sort`, `reading`, `max_rows`: the desktop's columns and codes), the saved search a playlist fills itself from; step 8 made `unstudied` (`track` keyed to `documents(id)`, `under` the `JUDGED_UNDER` that failed it), the songs the enrichment pass could not decode, which the `documents_forget_what_would_not_decode` trigger clears when a document's `size` or `modified` moves; step 9 gave `studies` a `band_levels` (the 29 bytes `eq.md` describes, written by every study the pass and the analysis pane keep, and measured alone by the pass for a study kept before this step, `eq.md`) that the `documents_forget_where_the_music_sounded` trigger clears when a document's `size` or `modified` moves. Step 10 (`ANSWERS_TRAVEL`) gave `documents` the `packets` and `frames` that say which file a row is (cleared by the `documents_forget_which_file_they_were` trigger when `size` or `modified` moves) with the `documents_by_length` index, and `studies` the `studied`, `studied_under` and figures a study shared with another device carries (`mobile.md`'s *Answers from other devices*). Step 11 (`SONGS_RECEIVED_ARE_HELD_BY_WHAT_WAS_ASKED`) made `songs_received` (`recording` primary, `track` keyed to `documents(id)`), the recording keys (`Recording::key` and `named_key`) a landed song was fetched for. Step 12 (`DOCUMENTS_REMEMBER_WHEN_THEY_WERE_DOWNLOADED`) gave `documents` a `downloaded` (Unix seconds) that `index_download` sets on a song a download landed (search, wants, a playlist's missing songs; never a song from another device), which the enrichment pass reads to look those songs up first. A version past the chain is `Error::CatalogVersion`. `V1` holds `documents` (the metadata, the lookup's clocks `asked`/`answered`
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
partial index), so a scan is not quadratic in the library.
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
desktop's seven (`resonate-ui`'s `RESONATE` flavour, held equal by `AppearanceTest`). Either seeds a
MaterialKolor `TonalSpot` scheme (`schemeOf`), never the platform's `dynamic*ColorScheme`: One UI fills
the system surface-container roles with flat greys of its own, which greyed the mini player and the
scrolled top bars, so only the accent hue is taken from the system. `ResonateTheme` builds one scheme and one `MaterialExpressiveTheme`
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
continues behind the mini-player's outer padding instead of ending at an opaque strip. The measured
banner and mini-player height supplies each `Page`'s bottom content inset and floating-action-button
clearance, so the last items can scroll above the controls. The page is edge to edge, so the window
never resizes for the keyboard: `Page` shrinks itself by the part of the IME inset that covers it and
lowers its bottom inset by as much, so a focused field low on a page scrolls above the keyboard and the
mini-player's room is not counted twice. A text field's value is Compose state (a view model's
`mutableStateOf`, read on as a `snapshotFlow` where typing drives work, or `rememberSaveable`), never a
`StateFlow` collected back into it, whose frame of lag drops keyboard edits and moves the cursor. Each banner carries its own 8 dp above it and the column none,
so with no banner shown the mini player's gap above equals the one below. The empty library's *Welcome* scrolls, so the banners
arriving on a first scan never squeeze its buttons.

**Banners** are three, all `LiveRegion`s: *Reading music folder*, *Syncing with your devices* or
*Fetching songs from your devices* (progress; a fixed label with the changing count and the bar
cleared from the semantics, so a count is never read again at every step) and the notice, which
says a failure or a status and stays until dismissed, whatever call succeeds after it. **What the library finds by itself is silent by default**: *Tell me what the library finds by itself* in Library tools (`tell-findings` in `library-features`, off by default, `LibraryFeatures.tellsFindings`) gates the two notices the app raises with nobody asking, the songs hidden because their files are gone and the songs a sync landing kept, through `FindingNotices.shown` (`FindingNoticesTest`). Everything the listener asked for (a scan's *Read N songs*, filing, tags, deletion) and every failure is shown whatever the switch says, as is the live *Reading music folder* progress; the app starts no scan of its own. A liked song
shows *Added to favourites* on the snackbar with *Add to playlist*; the song sheet (`SongSheet`)
holds the song's actions, adding Equaliser, Audio output and Visualizer when opened from the
player, and *Remove from queue* from a queue row.

**The player** is one `PlayerExpansion` (saved across recreation, reopening expanded), a single `Animatable` the mini-player's tap or upward drag
and the system's predictive back all drive: `NowPlaying` is drawn while it is above zero, so the
mini-player and the page beneath are never two routes with two exit animations. The mini-player is
a `surfaceContainerHighest` capsule (16 dp corners) with 48 dp artwork inset 8 dp at the start and top, its corners the capsule's less the inset so the two curves are concentric (4 dp and 36 dp in a short window), the title, artist, previous, play and next and
a line of progress, inset like the artwork, that says *Song progress* with its percentage. `NowPlaying` wears an `ArtworkTheme`
seeded from a 96-pixel sample of the cover (`rememberSeedOf`; a cover with no colour keeps the
app's primary), shows the cover or the lyrics, and opens the queue as a sheet. Its action row is *Lyrics* and *Queue*; the sleep timer is *Sleep timer* in the player's song sheet (`Sheets.sleeping`) and its chip shows in the row only while a timer runs. The title opens the album and the artist the artist; sliders draw a 4 dp track (`CommitSlider`'s `trackHeight`). The cover's gradient strength is *Player background* in Settings › Appearance (`backgroundStrength`, 0.7 by default, kept in the `appearance` preferences); at 0 the player has no coloured background, and with *Pitch black* it is black.
Album, artist and playlist pages wear the same theme from their first cover.

**The queue sheet** lists the devices that handed off (one row), History, the playing row and Up
next; a handle drags a row (`Reorder`, its rows counted from Up next's first row as it stood when the
drag began, so a song ending mid-drag moves the row dragged; a drop over a list that changed meanwhile
shows the engine's order), sent as `queueLanding(from, to)` since the engine's
`move_in_queue` counts the landing row before the move, so a downward drag adds one and the end is
the queue's length. *Save queue as playlist* and *Clear* sit in its header; *Clear* asks first (history and up next go, the playing song stays), and a swipe the engine refuses puts its row back after `UNREMOVED_AFTER_MS`, as a playlist row does.

**Library** is a pager of Songs, Albums, Artists and Playlists under a tab row (scrollable past a
font scale of 1.3), *New playlist* a floating button on Playlists (the heading carries no second one), Songs opening on full-width *Play* and *Shuffle* with the count at the start and sort and filters at the end of one row, as Albums and Artists put their sort, the active filters on a row beneath, *More* the folder, rescan and
Library tools entries. **A sort menu lists only the orders**, in compact 40 dp rows (`MenuEntry.Choice`): choosing the order already held flips its direction, which the sort chip's arrow shows (a *Descending* switch under seven rows made the popup overrun the list and the notice). A catalog read (`catalogRead`) never emits a spinner over a result it held, so a page subscribed again
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
the online page alone; with online off the section, and a link's page, is one card that turns it on. Each
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
listed once. A playlist cover is `MusicPlaylist.covers`: none shows an icon,
one to three the first song's picture, four a 2 by 2 (`PlaylistCover`). A playlist adds songs through
a searchable multi-song picker over the batched add seam; rename, revision-checked moves and removal,
delete and undo are the native calls. A row plays its list from the position tapped
(`Playback.play(tracks, index)`), so a repeated song selects the row pressed.

**Lyrics** are a `LazyColumn` of `LyricLine`s following the native `LyricFrame` (`mobile.md`'s
*Lyrics*; the frame is computed only while a lyrics pane is open, `watchLyrics`). The active line is
full size and ink, the rest 0.94 scale at 38 % alpha with the distant ones blurred, a wipe colouring
each line's words by their UTF-16 ranges, the edges faded. A drag pauses following for five seconds
and offers *Follow lyrics*; tapping a timed line seeks. Blur is dropped where animations are off
(`ANIMATOR_DURATION_SCALE` zero) or TalkBack is exploring (`rememberCalm`).

The equaliser is the boundary's parametric curve (`eq.md`'s *The phones*): any number of bands up
to 32, through core's `Profile` and the engine's `Equalisation` fallback. The engine holds one curve at
a time; which one is the route's business (below). Android opens on the boundary's nine named
presets (`equaliser_presets`, handed over in `equaliserInfo`; `eq.md`'s *The phones*) and
frequency sliders labelled by their audible range, with live signed gain values. Selecting a
preset enables the equaliser and stores its name and curve through the existing preferences;
boosted presets lower the preamp to leave headroom, except Bass only: it raises the three lowest
octave bands, leaves every other band flat and the preamp at zero, so the rest is never lowered.
Editing a band or the preamp clears that preset name. An empty flat curve shows ten octave sliders
(31.25 Hz to 16 kHz, Q sqrt(2)), creating
the bands only on a completed edit. An AutoEq or imported profile keeps its filters, frequencies,
Q and preamp; its gain-bearing bands get sliders without being converted to the octave layout.
Advanced exposes the interactive graph, accessible band selectors and per-band type, frequency,
gain and Q. *Fit the preamp* under the preamp slider sends the shown curve to the boundary's
`fitted_preamp` at the stream rate (`fittedPreamp`, off the main thread) and commits
the answer as a preamp edit, clearing the preset name; it is off with no bands. Every edit of a band
or the preamp (slider, graph, band editor, added or deleted band, fit) switches the equaliser on, as a
preset does (`withCurve`); only the switch turns it off, and confirmed reset removes
every band and returns the preamp to zero without changing that switch. An edit is shown at once as a
draft over the stored curve; the draft goes once the repository has tried it, so a rejected curve falls
back to the stored one, and a graph drag the gesture loses (cancelled, scrolled away) is released, not kept. The drawn response is the
stage's arithmetic at the stream rate (48 kHz with nothing open, so the graph never reads empty);
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
`equaliser-route-names.<id>`. While the output in use is bound every edit (sliders, graph, AutoEq,
import, reset) writes its curve and the other choices still go to the shared keys
(`write(preferences, route)`); an unreadable bound curve reads as the shared one. *Only for <output>*
on the equaliser page (`OutputCurveSection`) binds the curve playing to that output or unbinds it,
falling back to the shared curve at once; *Curves kept for other outputs* lists the rest, each *Forget*
confirmed and told in a notice (Android's idiom, where iOS offers Undo). The equaliser switch and
room correction stay one for every output, as on the desktop and iOS
(`OutputRoutesTest`, `OutputCurvesTest`, `PlaybackPreferencesTest`).

**Room correction is the equaliser page's last section** (`RoomCorrectionSection`, `RoomCorrectionState.kt`):
*Choose a response…* opens the system document picker for audio (`ACTION_OPEN_DOCUMENT`), the chosen
file is read up to 64 MiB and one byte (the boundary refuses the byte) and kept through
`keep_room_correction`, then switched on; the switch, the response's channels, rate and length and
*Forget the response* (confirmed) follow, as `eq.md`'s *Room correction* says. `room-correction` (on)
and `room-correction-name` live in the `playback` preferences, as iOS's suite; the kept response is
applied at start (`initialize`), a missing file turning correction off with a notice. Choosing,
applying, forgetting and restoring run one at a time under one mutex.

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
read, and a `reload` a newer request overtook stops at its next page. A song's album page opens by `album_key` (`albumKey`), so a song with
another spelling, no album artist or a shared title opens its own album, and the song sheet's
heart reads the live favourite, not the map the row was fetched with. Folder names and grants are listed independently of the catalog, including
unavailable grants; removing access revokes a grant after confirmation, leaving music files
untouched, and hides the songs under a tree (`hide_songs_under` its prefix, `MusicDocuments.prefix`:
`<tree>/document/`) while Downloads keeps its entries.

**A song whose file has gone is hidden, as on iOS** (`LibraryFeatures.forgetMissing`): when the
Activity comes forward (at most every five minutes, and not during a scan) and after a scan that
failed on no song, `hide_unlisted_songs` is given the prefixes of the readable folders (a tree whose
root answers a query, and MediaStore's Files collection while Downloads is on), so an unreadable
grant never hides its songs, and the documents those folders list (`MusicDocuments.listed`: one
query a directory, not one open a song): only a song the listing does not show is opened, so
`NotFound` still decides, and a listing that fails part-way costs opens, never a wrong hiding. A pass that hid songs reads the catalog again, reloads the open
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
counted listen, the unheard last) and direction, read off the search index below. Suggestions are explicit mixes of the actual library and its
listening history. `Listening` counts heard time rather than sought position; its half-track or
four-minute threshold feeds the local history and statistics. History filters cover all time and
the last week/month/year. Document playlists preserve repeated songs, use revision checks for
position edits, and have bounded in-session undo; they never invent Linux paths.

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
left out. `statistics(days)` ranks the top 25 songs.

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
The front end removes the file (Rust holds only a descriptor), then `forget_deleted_songs(ids)` drops
the queue rows naming each (the playing one moving on), steps the revision of every playlist holding
it and deletes its row with every row referring to it (playlist rows, listens, lyrics, artwork,
studies; a picture nothing else shows goes too), answering `SongsForgotten` (rows forgotten, queue
rows left); its listens leave the totals, as a hidden song's already do. The queue rows it deletes leave
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
the reference, the network or the app closing leaves it, and the next start carries its refresh,
skipping rows asked since it began. **A download is looked up first**: every pass puts the album and
song asks of each `downloaded` document still due ahead of the rest, the newest download first, and
orders the artists, the lyric reader and the study worker the same way
(`a_downloaded_song_is_looked_up_before_the_songs_held_before_it`); `index_download` also seeks the
song it landed. `seek_enrichment(track)` lifts the song's album, the song and
its artist to the front where still due, and the song to the front of the lyric reader's list, and
a lifted or downloaded ask fetches its cover or portrait on the pass's own thread rather than behind
the pictures already waiting (`a_song_sought_has_its_album_and_words_asked_before_the_rest`). `enrich_track` is the same album, song and artist asks
forced for one row over the listener's client; its lyric lookup failing fails nothing. `artwork`
answers the song's own picture (embedded, folder, `set_artwork`) and falls back to its album key's
cover, or takes that cover where it betters a thumbnail; no per-song archive copy is written.
`art_changed_since(revision)` names the songs whose picture changed after a revision.
`artist_information` answers the kept profile and fetches and keeps one only where none is;
`artist_portrait(mbid)` reads the kept portrait; `ask_again_for_covers` clears `cover_asked` on
albums still without one.

**The pass studies and recognises beside it** when it is given an AcoustID key
(`EnrichmentOptions::acoustid_key`, the key both phones keep encrypted and read at each start; a blank key
studies and recognises nothing, so a phone with no key never decodes its library in the background, and
saving a key starts a pass). `resonate-study` is one thread (the desktop's pool is half the cores; a phone
keeps one) over `tracks_to_hear`: visible documents with no `studies.agreement` (every visible one under
`refresh`), cut to `at_most`, downloads first and then in id order. Each is studied by `resonate_analysis::study` (one decode, the
levels, verdict and print, stopped within a decode by `stop_enrichment` through `Hearing`), kept through
`keep_studied`, then its print asked of `AcoustId` over the pass's yielding client through
`recognised_through`, the one reading `recognise_track` shares: matches weighed by `agreement` and kept as
`studies.agreement`, so a song is asked once until its file changes (`documents_forget_what_was_heard`) or a
refresh. A song whose study has no print is kept `Unheard`, not studied again. A refusal or unreadable
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
refresh enabled and offers Stop; retrying missing covers clears only the existing cover retry
marks. Kotlin reads progress and artwork revisions every two seconds while the Activity is
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
asked; Last.fm password sign-in saves the returned session, never that password, into the saved
accounts with the key and secret it was made under, leaving every other unsaved field as typed; the
password field clears only once the sign-in succeeds, and *Sign out* saves the account without its
session. *Headphone correction* searches as typing settles or on the keyboard's search, once per text;
emptying the field clears the results, and applying one shows its own progress and says when AutoEq
gave no profile. Favourites
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
any number of Monochrome, hifi-api and Subsonic accounts, each its own row with its own switch, and at
most one TIDAL account, because the boundary hands back one renewed refresh token
(`refreshed_tidal_token`) that cannot say which account it renews. An account is asked when it is on
and ready (`Readiness`, iOS's `DownloadAccount.readiness`: Monochrome and hifi-api always, Subsonic
with a server, user and password, TIDAL with a client ID and refresh token), and `find_download`
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
waiting) is answered landed and its staged copy dropped, never queued a second time. **A song landed is held by what was asked for, never fetched again**: `sync_receive` notes each key of the recording the sender announced against the document it indexed (`note_received_as`), and `holds` and `open` look a recording up by name first, then through `songs_received` (`DocumentCatalog::candidates`), so a file whose own tags name the song otherwise than the sender's catalog does (a corrected title, a missing album) is neither fetched on every follow nor refused as `Missing` when that sender asks for it back (`a_song_landed_under_the_names_of_its_own_file_is_held_by_what_was_asked_for`). A document made for a landing, a download or a sync is created under the mime type the platform maps its name's extension to (`MusicDocuments.mimeOfName`), since a provider given another type for the same extension appends its own (`song.flac.flac`). Kotlin polls `sync_received`, creates a document in
the chosen writable music folder (the first one unless chosen), journals its URI as
`unfinished-sync-landing`, and hands the fresh descriptor to `sync_receive`, which copies the bytes,
validates and indexes the document as a download is, and forgets the waiting song whether or not that
succeeded, telling the sync service of a failure (`Sync::refused`) so the song is not fetched again at
once. **A failed landing holds back that song alone**: it deletes only the new document and waits a
minute before that song is tried again, the rest landing meanwhile; a song refused
`LANDING_TRIES_AT_MOST` (3) times in a row is dropped with `sync_refuse_received`. A received song's
file name drops `/ \ ? : * " < > |` and control characters, so a FAT, exFAT or SMB folder takes it. A
download's file name does too (`downloads::file_stem_of`, the one stem both use).
`SyncedDevice.id` names a device (the hex `DeviceId`; names repeat after a reinstall), which
`sync_forget_device` takes; `HandedOff.id` likewise, and `sync_continue_from` and
`sync_fetch_missing` take the id (a name is still read where no id matches). The sync screen's
pairing code stays hidden until *Show pairing code* is pressed; its text is absent from the
accessibility tree while hidden. **A computer is paired by the system camera or any QR scanner**: the manifest's `MainActivity` filter takes `VIEW` of `resonate://pair` (`DEFAULT` and `BROWSABLE`, which a scanner needs), `opening` hands the link to `LibraryFeatures.offerPairing` (`sync_code_in_link`), and `PairingOffer` asks *Join these devices?*, naming the code, before `joinSync` (which also turns sync on; one made before the settings are read waits in `joinWaiting`) and opens the sync page, as iOS does. Resonate never touches the camera. `song_uris` lists every document's URI, hidden ones included.

`ACCESS_NETWORK_STATE` lets the default-network callback report `NET_CAPABILITY_NOT_METERED`; songs
move only on an unmetered network unless mobile data is allowed. While sync runs Kotlin holds a
`WifiManager.MulticastLock` (`CHANGE_WIFI_MULTICAST_STATE`, released when sync stops and when the
repository closes), so beacons and Bonjour from devices on the same network are heard (`sync.md`'s *Nearby*); the status text counts them when the server is away and a
playlist's menu offers fetching from a nearby device whatever the server's standing. Kotlin polls the status every two
seconds while the Activity is started (30 s otherwise), refreshes the library and playlists when
`SyncStatus.arrivals` moves, and keeps the data-sync foreground service while songs move or wait. A landing that kept songs says *Kept N songs from your devices* only when that same switch is on.
Playlist menus offer fetching their missing songs from each online device and keeping their songs
here; the queue offers continuing what another device was playing, starting it through the Media3
controller when that device was playing.

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
accessibility cache before each poll: an API 36 screenshot can show current controls while the
cached tree still reports the preceding value. All write tests use copies.
