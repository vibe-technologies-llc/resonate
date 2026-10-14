---
paths:
  - "android/**"
  - "crates/resonate-android/**"
  - "crates/resonate-bindgen/**"
---

# Android

The Android front end is Kotlin and Jetpack Compose, in the Gradle project under `android/`.
Android 16 (API 36) is the minimum. ARM64 phones and x86_64 emulators share the same debug APK;
SDK 36, build-tools 36 and NDK 28.2.13676358 are pinned. Android Studio opens that directory and
runs `app`; Gradle builds the Rust library and Kotlin bindings as generated sources before
compilation. Python 3, Cargo and both Rust Android targets must be installed on the build host.
`resonate-bindgen` runs on the host and reads metadata from the Android library without loading
it; building the Android app needs no host PipeWire installation.

Each APK variant owns its native build task. Release uses Cargo's release profile, including
optimization and thin LTO, in a separate generated directory; it never packages
the debug variant's native libraries. Native symbols remain until binding generation finishes;
Android packaging strips the APK's libraries afterward. Release signing is supplied separately, with no signing
key or password stored in the repository.

`resonate-android` is the sole exception to the workspace's unsafe prohibition, explicitly
approved for the native boundary. UniFFI generates the FFI scaffolding; the handwritten unsafe
operation takes ownership of the file descriptor detached by Kotlin's `ParcelFileDescriptor`.
A descriptor is transferred once and closed by Rust's `File`; a negative descriptor is refused.
The same transfer (`downloads::fresh_destination`) owns a newly created download or received-song
destination, never an existing music document.
Domain, codec, DSP, engine and all other workspace crates remain under `forbid`. Kotlin bindings
are generated under `app/build/`, never checked in or edited. New errors are typed and bounded as
in `errors.md`; only the foreign presentation flattens their display text for Kotlin.

Handwritten Kotlin, Android tests and Gradle Kotlin scripts use ktfmt's Kotlin language style:
four-space indentation, wrapped calls, trailing commas and one statement per line. Generated
UniFFI code stays outside formatting passes.

## Audio and lifecycle

`MediaLibraryService` owns a Media3 adapter over the Rust player. Compose reads published state
and sends controller actions; it performs no file, decode, catalog or network work. The adapter
publishes metadata and transport state to notifications, media keys and lock-screen controls.
The session activity opens or reuses the main Activity when its notification is tapped. Current
artwork is decoded off the main thread and encoded into a thumbnail bounded at 384 pixels and
128 KiB; only the current queue row carries it, so repeated songs cannot multiply image payloads
across a Binder timeline. Art revision and cache generation refresh the session after enrichment.
Dismissing the app task pauses every session player and stops its foreground service. Going Home
keeps playback running; an active download may retain the engine but never keeps music playing
across task dismissal.
Controller actions enter one ordered worker, so load/prepare/play cannot race one another.
Audio focus loss and noisy-output events pause playback. An Activity's recreation does not own
or close the audio engine. Playback and download services each retain the repository; the last
release cancels its workers and saves and closes the engine off the main thread. A later service
waits for that cleanup before opening another. Native shutdown stops analysis, downloads and the
shared online client before releasing playback. Long analysis, enrichment and HTTP work run on
separate workers without holding the controller lock; request generations refuse stale results.

Oboe opens a shared stereo float stream at 48 kHz; the existing Rust engine converts into it.
The output pane lists `AudioManager` devices and sends a chosen device id to Oboe before
rebinding; system default is id zero. The pane distinguishes a requested route from the actual
opened device. Android owns the physical route, and this is no promise of a DAC's rate or
bit-perfect delivery. Buffer length is validated from 50 to 2000 ms at the native boundary.
DoP, exclusive output and per-device format enumeration are not offered by this backend yet.
The native callback is `process.rs`, under the realtime lint set: preallocated bytes only,
no allocations, locks, filesystem, JNI or Kotlin calls. A short fill is padded with PCM silence.
Drain waits for the output latency after the ring ends before reporting completion. Device errors
are published from a worker through the existing stream events.

NDK libc++ is packaged beside the Rust library. Native linking rejects undefined symbols and
sets a 16 KB maximum page size; APK native libraries are uncompressed and aligned by AGP.
The emulator's native playback test must load and play audio, not merely install the APK.

## Documents and catalog

The user chooses folders through the Storage Access Framework. Persistable read/write grants
survive restart; neither broad storage permission nor guessed filesystem paths are used.
The `content` media provider preserves the document URI's escaping exactly, including `%3A`,
`%2F` and `%25`, through a saved `MediaLocation`. Kotlin opens a descriptor on a worker; Rust
uses the same codec and transport as Linux. A forward-only document uses the codec's spool,
with its writable folder passed explicitly through `Sources::spooling_under`, never by mutating
the process environment.

The document source has its own SQLite catalog, `documents.sqlite` in app-private storage;
it does not put invented paths in the Linux catalog. Its first schema is `V1`, version 1.
Further schema changes append migrations and preserve document ids, favourites and resumption.
An upgrade checks the existing catalog and keeps a synced `VACUUM INTO` backup under a held staging
file before one immediate transaction applies the remaining steps. V2 adds metadata, cached
artwork/lyrics, listening history and study summaries; V3 adds document playlists; V4 stores
independent successful-submission positions for ListenBrainz and Last.fm. V5 adds saved wanted
recordings, separate from playable documents. V6 adds `listens.elsewhere` (another device's listen),
`playlists.created` (0 for playlists made before it, a nanosecond stamp after; undo restores it) and
`received`, the songs another device sent that wait for a document.
Rescanning upserts by the full URI; it never deletes rows on an unavailable or revoked grant.
Queries page at most 500 rows at a time. Folder traversal stops at depth 32 and admits the same
extensions as core. The queue resumes paused through the engine's `Resumption` command, so the
saved position is applied during opening rather than racing a subsequent seek. A newly prepared
paused queue uses the same command, so its selected song has opened metadata before being
published. Queue-row ids remain distinct from catalog ids: repeated songs have separate timeline
uids and shuffle/jump/save resolve the document through its raw URI. A save compares
queue revision and place: an unchanged save writes nothing, a moved place updates the singleton
row alone, and only a changed queue rewrites its entries.

## Appearance and playback tools

Compose uses Material 3 dynamic color schemes from the Android system, selected with the system's
light/dark appearance. Surfaces, controls and artwork fallbacks use semantic theme roles; the
platform theme follows day/night and its accent comes from `system_accent1_500`. Android 16 is
the minimum app version, so there is no older-platform palette or notification-permission branch.
The native build retains NDK 28.2's API 31 symbol baseline; the APK's minimum API 36 governs
installation, and native code currently needs no newer platform symbols.

Settings use searchable groups for sound/playback, library sources and connected services.
The online switch lives there; the equaliser and processing have separate pages, and profile
import expands on request. Child-page Back restores its parent and settings search, including
after Activity recreation. Library tools use a separate grouped page. The floating mini-player
shows cover, title, artist, progress/time, previous/play/next controls and a favourite heart above
navigation. Its metadata and heart have their own row so all transport targets remain 48 dp wide.
The expanded player links the tagged album and artist to their collection pages; route arguments
are URI-encoded and carried in saved navigation history, so punctuation and nested Back preserve
which collection was opened.

The navigation roots are Songs, Library, Queue and Settings. Library pins Favourites above playlists,
albums and artists, with collection search and category chips. Favourites and collection counts
read the full catalog separately from the song browser's search and filters. A completed favourite
write updates catalog, browser, queue, snapshot and open playlist state together, then refreshes
the active browser query. The native catalog remains the persisted source of truth.

Collection pages share cover-led headers, song counts/duration and Play/Shuffle actions. Albums
keep disc/track ordering; artist pages expose their albums and songs. Playlist covers use at most
four existing track covers loaded off the main thread. Playlist detail adds a searchable multi-song
picker through the existing batched add seam; rename, revision-checked moves/removal, delete and
undo retain their existing native behavior. Row playback passes the explicit position, so repeated
songs select the row that was tapped.

Motion uses Compose's duration-scaled animation primitives, following Android's animation scale.
Page reveals animate one active destination rather than keeping two service-facing screens alive.
Artwork and player labels crossfade, hearts use a spring response, and keyed collection rows
animate placement and removal. Decorative crossfade icons expose one current label on their
parent control, so accessibility never announces both transport or favourite states.

Player and collection backdrops derive two tones from a bounded 16 by 16 sample of cached cover
art on a worker. Dark or unsaturated covers fall back to the system palette. Tone changes blend
through cached radial brushes; drawing and artwork-scale animations read their state in draw
layers rather than recomposing the library on every frame. The backgrounds do not run perpetual
animation loops, decode their own full-size images or alter the audio callback. The expanded
player keeps a square, shadowed cover, compact album/artist links and a larger central transport
control; pause settles the cover to a smaller scale.

The ten-band equaliser uses octave-spaced peaking bands from 31.25 Hz to 16 kHz with Q sqrt(2),
through core's `Profile` and the engine's `Equalisation` fallback. One curve applies to all Android
routes; no UI implies a per-device binding. Preamp and gains remain independent of the enable
switch; reset flattens the curve without changing that switch. The native boundary validates the
whole curve before sending any commands, and sends only fields that changed. ReplayGain, true-peak
clip prevention, resampling quality/phase and experimental lossy restoration use existing engine
commands, never a platform equaliser running beside the Rust chain.

Sound settings live in app-private `playback` SharedPreferences with named enum values and fixed
band slots, separate from the unchanged document schema. Unknown enum values fall back to the
native defaults; nonfinite levels use the default and out-of-slider levels are bounded. The ordered
repository worker restores these preferences before allowing playback actions, sends edits and
commits each completed gesture off the main thread. Sliders keep an in-progress value locally and
commit on release, avoiding a file write or stream rebuild for each motion event.

The sleep timer is the engine's `Until`: elapsed time, end of song or end of queue, with cancellation
and the existing fade into a pause. Snapshot polling carries the remaining time; Compose keeps no
timer, so the countdown and pause continue with the Activity in the background. A timer is not
restored into a later engine session.

## Library tools

Scanning keeps album artist, genre, date, track/disc numbers, depth, channel count and tagged
MusicBrainz ids. Embedded pictures precede the folder's cover/folder/front JPEG or PNG, bounded at
8 MiB; a matching-basename LRC is bounded at 1 MiB and decoded through core's text reader. A bad
companion does not refuse playable audio. Bitmap caches use 32 MiB and decode to at most 1024
pixels per side after checking dimensions. Albums group by title and album artist, falling back
to the tagged artist. Folder names and grants are listed independently of the catalog, including
unavailable grants; removing access revokes a grant after confirmation, leaving music untouched.

Filters combine folded text/artist/genre, album, minimum rate/depth, favourites and unplayed, with
six stable orders and direction. Suggestions are explicit mixes of the actual library and its
listening history. `Listening` counts heard time rather than sought position; its half-track or
four-minute threshold feeds the local history and statistics. History filters cover all time and
the last week/month/year. Document playlists preserve repeated songs, use revision checks for
position edits, and have bounded in-session undo; they never invent Linux paths.

Lyrics prefer saved embedded/sidecar text, fetch LRCLIB only when online, and retain a richer saved
sheet over a poorer answer or failed refresh. Native `line_at` and `voices_in_play` drive Compose
line/word highlights, scrolling, scale, opacity and blur; tapping a timed line seeks the player.
Analysis uses the shared whole-track pass for waveform, spectrum, lossless assessment, LUFS, DR,
true peak, clipping and bit use. Eight full results are cached in memory, invalidated on rescan;
summary studies are persisted. A `Watch` cancels decoding. The live visualizer subscribes to the
engine tap only while visible and samples on a worker; it never involves Kotlin in the callback.
The inspector probes the original document and displays both technical and tagged metadata.

## Online and downloads

Online starts enabled when no preference is stored; Settings persists an explicit offline choice across upgrades. A single paced `Online` client serves
MusicBrainz search/enrichment, covers, portraits, LRCLIB, AutoEq, recognition and scrobbling.
Enrichment admits only a unique strict title/artist/length match, or an existing recording id;
it fills learned identities/album artist and cached pictures/lyrics without writing source tags.
Search covers songs, albums, artists, release editions and songs within a release; artist
discography pages by identity. Shared song, album, artist and playlist links use the library's
existing resolvers; remote playlist order and repeats are retained, within the service's own page
limits. Importing local matches creates a document playlist in one transaction and one undo step;
uncoded name/artist/length matches must be unique, so an ambiguous row stays missing. Recordings saved for later persist independently of files, deduplicate by recording id or folded identity,
and distinguish held from missing against the current catalog; downloads refresh that view. Recognition
is explicitly asked from Analysis: a supplied AcoustID key sends the stored whole-track print,
otherwise `ByEar` sends a Shazam signature. Candidates are shown, never written as a guess.

AutoEq and pasted EqualizerAPO/GraphicEQ profiles use the shared parser. A chosen parametric
profile is retained intact as canonical text; manual gain/preamp edits switch to the ten-band
curve. One curve still applies to every route. ListenBrainz and Last.fm accounts are opt-in,
validate through their existing seams, and submit counted local listens in batches of 100 with
independent durable marks advanced only after success. Existing history is included only when
asked; Last.fm password sign-in saves the returned session, never that password. A separate
explicit action sends current identified favourites; it promises no automatic remote unlove.

Monochrome, hifi-api, Subsonic and the listener's TIDAL account use the existing provider matching
and delivery code. Credentials and recognition keys are encrypted with AES-GCM under Android
Keystore. TIDAL device sign-in displays the code and authorization page, and renewed refresh
tokens are saved. The user chooses a write-granted music folder per download. A data-sync
foreground service retains the engine and offers cancellation. Only a fresh SAF document is
opened for writing; its URI is journalled before streaming, bytes are bounded at 8 GiB, and codec
probe/duration validation precedes catalog insertion. Failed or interrupted deliveries delete
only that newly created document, including journal recovery on the next start. Validated
deliveries retain the selected recording identity and fill missing catalog names without writing
tags, so an untagged file is still recognised as a held want. A rescan preserves those names
when the document supplies none.

## Device sync

`sync.rs` is the bridge to `resonate-sync`: `DocumentCatalog` implements its `Catalog` over the
document catalog, and `AndroidPlayer` holds at most one running `Sync`, started and stopped as a
whole (`sync_start` leaves the previous one first; `shutdown` stops it before anything else). Its
ledger is `sync.sqlite` and its staging folder `sync-staging`, both beside `documents.sqlite`.
A blank key mints one; `SyncStatus.code` hands it back and Kotlin stores it encrypted beside the
provider accounts, with the other choices (enabled, server, device name, the three follow switches,
mobile data) in plain preferences. Sync is on unless switched off and runs only while online is.
A setting change restarts it.

A song is found by recording id, else by folded artist, title words, album when named and a length
within 2 s, the desktop rule; the lookup reads an in-memory index rebuilt whenever the catalog
revision moves. That revision is an update hook over `documents`, `playlists`, `playlist_rows`,
`received` and inserted or deleted `listens`. A synced playlist write replaces its rows, takes a free
name and steps `playlists.revision`, so an undo step from before it is refused as changed. Only this
device's counted listens are offered, the newest one only once 15 minutes old; another device's
listen is kept with `elsewhere = 1`, counts in history and statistics and is never scrobbled.
A holding's size is reported as 0, since a document row carries none; a song is served through the
`content` media provider's own descriptor.

A song from another device cannot be written through SAF from Rust: `land` moves the staged file into
app-private `sync-received`, notes it in `received` and answers that it landed, and `holds` counts a
waiting song as held so it is not asked for twice. Kotlin polls `sync_received`, creates a document in
the chosen writable music folder (the first one unless chosen), journals its URI as
`unfinished-sync-landing`, and hands the fresh descriptor to `sync_receive`, which copies the bytes,
validates and indexes the document as a download is, and forgets the waiting song whether or not that
succeeded; a failed landing deletes only the new document and pauses landing for a minute.

`ACCESS_NETWORK_STATE` lets the default-network callback report `NET_CAPABILITY_NOT_METERED`; songs
move only on an unmetered network unless mobile data is allowed. Kotlin polls the status every two
seconds while the Activity is started (30 s otherwise), refreshes the library and playlists when
`SyncStatus.arrivals` moves, and keeps the data-sync foreground service while songs move or wait.
Playlist menus offer fetching their missing songs from each online device and keeping their songs
here; the queue offers continuing what another device was playing, starting it through the Media3
controller when that device was playing.

Remaining desktop capabilities are open work in `docs/FEATURES.md`.

## Checks

From the repository root, `android/gradlew -p android :app:assembleDebug :app:lintDebug` builds
both native ABIs and checks Kotlin and the manifest.
`python3 android/check-native.py --sdk "$ANDROID_HOME"` checks ELF/APK alignment and runs
clippy for both native targets. With an emulator or device connected,
`android/gradlew -p android :app:connectedDebugAndroidTest` exercises native loading, document
identity, decode, rate conversion, playback, pause, seeking, resumption, sound-setting restoration,
sleep timers, metadata/artwork/sidecar lyrics, analysis, filtering, duplicate playlist/queue rows,
listening thresholds, saved wants and local playlist import, a loopback provider download and
cancellation, encrypted account storage, pinned favourites and player hearts, collection links,
playlist picker/edit/undo and repeated-row playback, tool navigation, Activity recreation and
background controls through Media3, notification artwork and opening, and task-dismissal pause. CI uses an API 36 x86_64
emulator; the local Pixel_10_Pro AVD additionally exercises 16 KB pages. Real hardware
remains necessary for Bluetooth, USB DAC and device-fidelity claims. UI checks clear the
accessibility cache before each poll: an API 36 screenshot can show current controls while the
cached tree still reports the preceding value. All write tests use copies.
