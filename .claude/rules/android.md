---
paths:
  - "android/**"
  - "crates/resonate-mobile/**"
  - "crates/resonate-bindgen/**"
---

# Android

The Android front end is Flutter, with Dart under `android/flutter/` and its native host in the
Gradle project under `android/`. Android Studio opens `android/` and runs the shared
`Resonate Android` configuration in `.run/` (or the generated `app` Android configuration).
The shared configuration targets `Resonate.app` and runs Gradle before deploying the default
Activity, preserving app storage. The nested Dart package is built through that host, not a
standalone Flutter Android runner; opening it and running `main.dart` searches for an absent
second Android project under `android/flutter/android/`. Flutter 3.47.7 is
pinned in CI. `flutter.sdk` in `android/local.properties` or `FLUTTER_ROOT` locates the SDK;
settings pass the resolved path to Flutter's loader so an environment-only build needs no local
properties file. Gradle builds Flutter assets, Rust and generated Kotlin bindings. Android 16
(API 36) is the minimum. ARM64 phones and x86_64 emulators share the debug APK; SDK 36,
build-tools 36 and NDK 28.2.13676358 are pinned. Python 3, Cargo and both Rust Android targets
are build prerequisites. Host binding generation reads ELF metadata and needs no PipeWire.

The Flutter Gradle plugin uses the legacy AGP DSL, built-in Kotlin and KGP 2.2.20 on the plugin
classpath. Flutter's automatic ABI filtering is disabled so the host's ARM64/x86_64 filters stay
authoritative; an ARM32 Flutter engine without its Rust library must not admit an ARM32 install.
`MainActivity` is a `FlutterActivity`. `FlutterBridge` exposes explicit method-channel actions
and state-flow patches; `FlutterWire` maps records to codec values without reflection. Unsigned
64-bit identities, positions and revisions travel as decimal text. The bridge qualifies
`kotlin.Exception`, since UniFFI exports an `Exception` that must not hide ordinary lookup or
argument failures. Dart owns the screens; Kotlin retains SAF, encrypted accounts, workers,
Media3 and services. Compact and album home-screen widgets use RemoteViews with rounded,
artwork-tinted surfaces and system light/dark colors. Explicit, immutable PendingIntents send
transport actions through a short-lived Media3 controller, preserving audio focus and the
service lifecycle. Metadata and the bounded session thumbnail are kept in app preferences for
launcher refresh after process death; that fallback is paused until a live session publishes.
Widget updates deduplicate metadata, artwork and playing state instead of sending polling ticks
to the launcher. No widget starts a second engine or decodes the original music file.
Compose dependencies and the old screens are removed together
with their Activity entry point and instrumentation selectors, so a build cannot mix the two UIs.

Each APK variant owns its native build task. Release uses Cargo's release profile, including
optimization and thin LTO, in a separate generated directory; it never packages
the debug variant's native libraries. Native symbols remain until binding generation finishes;
Android packaging strips the APK's libraries afterward. Release signing is supplied separately, with no signing
key or password stored in the repository. Flutter enables R8 for release and includes
`app/proguard-rules.pro`. Those rules preserve JNA and the generated `nativecore` boundary,
including names, fields, constructors and native entry points used through JNI and reflection.
The four absent `java.awt` types referenced by JNA's desktop-only helpers have explicit warning
suppressions; no missing Android or native-boundary type is suppressed.
Keeping only Java references is insufficient: renaming `Pointer.peer` makes JNA's `initIDs`
fail at startup with `UnsatisfiedLinkError`, before the Rust player opens. Debug startup cannot
verify these rules; release validation checks the R8 configuration and DEX field names, then
cold-starts the signed release APK through native player initialization.

The native boundary is `resonate-mobile`, shared with iOS (`mobile.md`). On Android the owned
descriptor arrives detached from Kotlin's `ParcelFileDescriptor`. Kotlin bindings are generated
under `app/build/`, never checked in or edited; only the foreign presentation flattens error
display text for Kotlin.

Handwritten Kotlin, Android tests and Gradle Kotlin scripts use ktfmt's Kotlin language style:
four-space indentation, wrapped calls, trailing commas and one statement per line. Generated
UniFFI code stays outside formatting passes.

The launcher uses the same five-bar blue-to-mint mark as iOS and `packaging/resonate.svg`.
Android vector foreground and monochrome layers share its paths and use adaptive-icon safe
padding over the same dark background. The launcher supplies its own mask, including Samsung's
shape; themed icons use the monochrome layer rather than a tinted full-color tile.

## Audio and lifecycle

`MediaLibraryService` owns a Media3 adapter over the Rust player. Flutter reads published state
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
waits for that cleanup before opening another. Native shutdown stops sync, then the enrichment pass
(leaving its mark to carry on), analysis, downloads and the shared online client before releasing
playback. Long analysis, enrichment and HTTP work run on
separate workers without holding the controller lock; request generations refuse stale results.

Oboe opens a shared stereo float stream at 48 kHz; the existing Rust engine converts into it.
The output pane lists `AudioManager` devices and sends a chosen device id to Oboe before
rebinding; system default is id zero. The pane distinguishes a requested route from the actual
opened device. Android owns the physical route, and this is no promise of a DAC's rate or
bit-perfect delivery. Buffer length is validated from 50 to 2000 ms at the native boundary.
DoP, exclusive output and per-device format enumeration are not offered by this backend yet.
The native callback is `output/oboe_render.rs`, under the realtime lint set: preallocated bytes only,
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
An upgrade checks the existing catalog and keeps a synced `VACUUM INTO` backup
(`<catalog>.before-v<N>`, written as `.part` and renamed) before one immediate transaction applies
the remaining steps. The writer holds a `.lock` beside the copy, never the copy itself: SQLite opens
and locks the `VACUUM INTO` target, and on Darwin `flock` and SQLite's `fcntl` locks share one
table, so a held target refused the copy and every iOS upgrade failed. A `.part` a failed attempt
left is removed first (`a_copy_left_half_made_by_a_failed_migration_does_not_stop_the_next`). V2 adds metadata, cached
artwork/lyrics, listening history and study summaries; V3 adds document playlists; V4 stores
independent successful-submission positions for ListenBrainz and Last.fm. V5 adds saved wanted
recordings, separate from playable documents. V6 adds `listens.elsewhere` (another device's listen),
`playlists.created` (0 for playlists made before it, a nanosecond stamp after; undo restores it) and
`received`, the songs another device sent that wait for a document. V7 adds the lookup's columns:
`documents.asked`/`answered` (nanosecond stamps), `asks`, `refusals`, `release_group`, `isrc` (kept
from tags on index), `album_key` and `art_changed`; `album_art` (one release, group and cover per
album key, with its own clocks and `cover_asked`), `artists` (a kept profile and portrait per
artist id, with its clocks and `portrait_asked`), the one-row `enrichment` mark, and `wanted.tried`,
`misses`, `offered_at`. V8 adds `refused_offers` (identity, `taken_from`, `refused` stamp); V10 adds `loves_told`, the loves told per listening service; V11 adds `wanted.release`, `release_group`, `artist_id`, `disc` and `position`, so a want downloaded from the Wanted list is asked with the release, disc and position of the result it was saved from. V12 adds `pictures` (id, a crc, the bytes), each picture kept once however many songs and albums show it: `artwork` and `album_art` hold a `picture` id, not bytes (`kept_picture` finds a picture by crc and then bytes, so a crc collision costs a comparison and never a wrong picture; `unreferenced_picture_dropped` removes one nothing shows), and the migration folds equal blobs into one row; `picture_crc` is registered on the connection beside `folded`. `artwork_picture(id)` answers the picture the song shows, its own or its album's, whichever `betters` says (the pair of ids decided once and remembered, `Catalog::better_of`), `picture(id)` its bytes, and `keep_folder_cover(ids, bytes)` keeps one folder cover for every song of a folder in one transaction. The album key is `folded(album) || char(31) || folded(album artist, else
artist)`, empty for a song without an album: the apps' grouping, folded. Triggers keep it on every
insert and on any write of `album`, `artist` or `album_artist`, and stamp `art_changed` with the
next catalog-wide number whenever `art_revision` moves; both call the connection's `folded`, which
`Catalog::open` registers before migrating, so a write from a connection without it fails.
Rescanning upserts by the full URI; it never deletes rows on an unavailable or revoked grant.
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

Flutter uses Material 3 with a violet seed and system light/dark appearance. Glass navigation,
rounded covers and semantic surface colors follow iOS's cover-led hierarchy. The native host's
theme follows day/night. Android 16 is
the minimum app version, so there is no older-platform palette or notification-permission branch.
The native build retains NDK 28.2's API 31 symbol baseline; the APK's minimum API 36 governs
installation, and native code currently needs no newer platform symbols.

Settings use searchable groups for sound/playback, library sources and connected services.
The online switch lives there; the equaliser and processing have separate pages, and profile
import expands on request. Child-page Back restores its parent and settings search, including
after Activity recreation. Library tools use a separate grouped page. The floating mini-player
shows cover, title, artist, previous/play/next controls and a favourite heart in a floating pill
above navigation, with ambient tones matching the album. Its transport targets remain accessible
touch targets, and accessibility semantics retain the song progress node.
The expanded player links the tagged album and artist to their collection pages; route arguments
are URI-encoded and carried in saved navigation history, so punctuation and nested Back preserve
which collection was opened.

The navigation roots are Songs, Library, Queue and Settings. Library pins Favourites above playlists,
albums and artists, with collection search and category chips. Favourites and collection counts
read the full catalog separately from the song browser's search and filters. A completed favourite
write updates catalog, browser, queue, snapshot and open playlist state together, then refreshes
the active browser query. The native catalog remains the persisted source of truth. Liking a song
presents a non-blocking floating banner with an "Add to playlist" button that opens the playlist
picker without obstructing navigation or accessibility.


Collection pages share cover-led headers, song counts/duration and Play/Shuffle actions. Albums
keep disc/track ordering; artist pages expose their albums and songs. Playlist covers use at most
four existing track covers loaded off the main thread. Playlist detail adds a searchable multi-song
picker through the existing batched add seam; rename, revision-checked moves/removal, delete and
undo retain their existing native behavior. Row playback passes the explicit position, so repeated
songs select the row that was tapped.

Flutter motion reads `MediaQuery.disableAnimations`; reduced motion removes transitions and
stops decorative drift. Artwork is loaded through the existing native cache as JPEG bytes,
keyed in Dart by song, artwork revision and generation. The decoded Flutter cache is bounded at
64 MiB and encoded/in-flight requests at 48 entries. The expanded player samples two tones
from a 16 by 16 cover decode, falls back for neutral/dark/transparent art and draws radial washes
inside a repaint boundary. The shell and expanded player place a 128-pixel cover decode behind
glass surfaces; the expanded player uses a stronger wash. Blur sits inside a repaint boundary
while transforms drift, and decorative movement stops when paused, offstage or reduced
motion is requested. Paused artwork settles smaller. Artwork, lyrics, queue and the sleep timer
share one glass pill; narrow widths and large text use icon-only buttons with named tooltips.
Lyrics fade at the viewport edges, enlarge the active line and soften distant lines. Native
UTF-16 wipe ranges and letter blend fractions color word timing without a Dart clock. Manual
scroll pauses following for five seconds and offers an immediate Follow lyrics button. Restorable
routes preserve child pages and settings search on Activity recreation. Collections are grouped
and ordered through native `albums`, `artists`, `album_tracks` and `artist_tracks` queries;
open collection rows resolve current metadata by catalog id so favourite changes arrive at once.

The equaliser is the boundary's parametric curve (`eq.md`'s *The phones*): any number of bands up
to 32, through core's `Profile` and the engine's `Equalisation` fallback. One curve applies to all
Android routes; no UI implies a per-device binding. Android edits it through ten octave sliders
(31.25 Hz to 16 kHz, Q sqrt(2), via `ten_band_curve`/`ten_band_gains`) and the preamp: a curve of
any other layout (AutoEq, an import) plays and is kept, its sliders read flat, and moving one
replaces it with ten bands. Preamp and gains remain independent of the enable switch; reset
removes every band without changing that switch. The native boundary validates the whole curve
before sending any commands, and sends only fields that changed. ReplayGain, true-peak clip
prevention, resampling quality/phase and experimental lossy restoration use existing engine
commands, never a platform equaliser running beside the Rust chain.

Sound settings live in app-private `playback` SharedPreferences with named enum values, separate
from the unchanged document schema; the curve is EqualizerAPO text under `equaliser-curve`, the
older fixed band slots read once to migrate. Unknown enum values fall back to the native defaults;
an unreadable curve reads flat. The ordered repository worker restores these preferences before
allowing playback actions, sends edits and commits each completed gesture off the main thread.
Sliders keep an in-progress value locally and commit on release, avoiding a file write or stream
rebuild for each motion event.

The sleep timer is the engine's `Until`: elapsed time, end of song or end of queue, with cancellation
and the existing fade into a pause. Snapshot polling carries the remaining time; Flutter keeps no
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

Filters combine search text, folded artist/genre/album, minimum rate/depth, favourites and unplayed,
with seven stable orders (title, album, artist, duration, added, plays and last played, the latest
counted listen, the unheard last) and direction, read off the search index below. Suggestions are explicit mixes of the actual library and its
listening history. `Listening` counts heard time rather than sought position; its half-track or
four-minute threshold feeds the local history and statistics. History filters cover all time and
the last week/month/year. Document playlists preserve repeated songs, use revision checks for
position edits, and have bounded in-session undo; they never invent Linux paths.

**Statistics beyond the totals are `listening_detail(days, limit, utc_offset_seconds)`** (`catalog_extras.rs`):
the albums (by the catalog's `album_key`) and artists (by folded name) heard most in the window,
ranked by heard time then plays, at most `MOST_LISTENED_AT_MOST` (100), and the listening per day
(counted listens, a day cut at the caller's UTC offset, the latest `DAYS_SHOWN_AT_MOST` days oldest
first, every day heard for all time (`days` 0), `ListenedOnDay.starts` the unix second that day began). Hidden songs and uncounted listens
are left out, as in `statistics(days)`; `songs_heard` counts the distinct songs and `first_heard`
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

Lyrics prefer saved embedded/sidecar text, fetch LRCLIB only when online, and retain a richer saved
sheet over a poorer answer or failed refresh. The native `LyricFrame` (`mobile.md`'s *Lyrics*;
Flutter reads its `read_at`, `active` and `wipes`) drives line/word highlights, scrolling, scale, opacity
and blur; tapping a timed line seeks the player.
Analysis uses the shared whole-track pass for waveform, spectrum, lossless assessment, LUFS, DR,
true peak, clipping and bit use. Eight full results are cached in memory, invalidated on rescan;
summary studies are persisted. A `Watch` cancels decoding. The live visualizer subscribes to the
engine tap only while visible and samples on a worker; it never involves Kotlin in the callback.
The inspector probes the original document and displays both technical and tagged metadata.

**Tags are written through the desktop's writer, at a path the front end opens** (`tagging.rs`).
`FileTags` writes by path alone (staged copy, journal, read back: `library.md`'s *Writing the catalog
back into the files*), so `edit_tags(songs, change)` takes each document as a `SongFile` (its URI and
the path the front end resolved and holds open: iOS's security-scoped folder; Android's SAF gives no
path, so Android offers none). `SongTags` is eleven text fields (`EDITABLE`: title, artist, album,
album artist, genre, date, track and disc number and total, composer), each `None` to leave, blank to
clear, else the value; a number must be a whole number above zero (`TagNumber`). Only the fields
differing from what the file holds are written (`planned`); the file is read back and every field
weighed (`TagsUnconfirmed`), then the row's names and numbers are set to what it now reads, the
title falling back to the file name. Formats `FileTags::writes` refuses are `TagsUnwritable`
(`tags_writable(name)` asks first). Each call is one undo step of what every written field held
before, the last `UNDO_STEPS_KEPT` (16) kept for the session; `tags_to_undo` names its URIs and
`undo_tags(songs)` writes them back (`tags_written_reach_the_file_and_the_catalog_and_one_undo_puts_them_back`).
`song_tags(id)` reads a song's fields through the document provider. `TagsWritten` counts written,
unchanged and failed songs with the first failure's sentence.

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
saved queue at its row and position, playing again where it was playing.

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
lossy, mono, stereo, multichannel, hires, favourite. `added:` (a row keeps no added time) and
`is:hidden`/`fake`/`suspect`/`misnamed` hold nothing, so their denial holds everything, as the
desktop's `NOT coalesce(…, 0)` does for a row unable to answer. A text reading nothing (`!!!`)
matches nothing. A lone undenied word of three letters or more also matches inside a word, below
every other fit.

Ranking weighs each lone word's best fit, the worst word deciding: whole name, then name prefix,
then whole word, word prefix, inside a word; the words together as a whole name in any order (*floyd
pink* is Pink Floyd) lift a row to that fit. Ties go to the column (title, artist, album, genre,
lyrics), then favourites, then play count, then folded title and id. Exports:
`search_library(text, limit)` answers a `LibrarySearch` (ranked tracks to `limit`, at most 500, the
full `track_count`, the albums and artists holding them with those named by the words first, the
*Reads* chips, completions from `Spellings::completing`, and `did_you_mean` only where nothing
matched); `highlights(query, text, column)` is `Search::lit` in UTF-16 code units, as Swift and
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
`resonate-enrich-lyrics` asks LRCLIB for songs with no sheet, a miss older than
`MISSED_AGAIN_AFTER` or a fetched sheet without a Lyricsfile older than `BETTERED_AFTER`, keeping
the richer through `KeptLyrics::richer_of`. Every row backs off by the library's own `due_again`
clause and `WAITS`; refusals end the pass at `REFUSALS_THAT_END_A_PASS` in a row; it checks between
requests for `stop_enrichment`, offline and the network. `enrichment_status` counts what was due,
asked, matched, covers, portraits, lyrics and refusals and names how it stopped (`EnrichmentStop`).
The `enrichment` mark is removed when the pass finishes or the listener stops it; a pass ended by
the reference, the network or the app closing leaves it, and the next start carries its refresh,
skipping rows asked since it began. `seek_enrichment(track)` lifts the song's album, the song and
its artist to the front where still due. `enrich_track` is the same album, song and artist asks
forced for one row over the listener's client; its lyric lookup failing fails nothing. `artwork`
answers the song's own picture (embedded, folder, `set_artwork`) and falls back to its album key's
cover, or takes that cover where it betters a thumbnail; no per-song archive copy is written.
`art_changed_since(revision)` names the songs whose picture changed after a revision.
`artist_information` answers the kept profile and fetches and keeps one only where none is;
`artist_portrait(mbid)` reads the kept portrait; `ask_again_for_covers` clears `cover_asked` on
albums still without one.

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
naming the service); only iOS records one yet.

AutoEq and pasted EqualizerAPO/GraphicEQ profiles use the shared parser and load as the curve's
bands, every filter kept (`EqualiserImport`). One curve still applies to every route. ListenBrainz and Last.fm accounts are opt-in,
validate through their existing seams, and submit counted local listens in batches of 100 with
independent durable marks advanced only after success. Existing history is included only when
asked; Last.fm password sign-in saves the returned session, never that password. A separate
explicit action sends current identified favourites (`send_favourites`); it promises no remote
unlove. `tell_loves(account)` (`loves.rs`) is the desktop's `Library::tell_loves` over the document
catalog: V10's `loves_told` (service, recording) is weighed against the favourites with a recording,
at most `LOVES_TOLD_AT_ONCE` loves or take-backs per service and call; a love refused with a 4xx
other than 401, 403, 408 or 429 (`resonate_library::refused_for_good`) is noted told, any other
failure stops that service until the next call; one service failing tells the other.

`share_link(id)` (`sharing.rs`) is the desktop's share over a document row: `Shared` is built from the
row's title, artist, album, year, recording id, ISRC and length (a stored code that does not parse is
left out), the reference is asked where the song streams only while online is reaching, and the
answer is `Shared::written`, `None` where neither a service nor a recording gives a link; only iOS
calls it.

Monochrome, hifi-api, Subsonic and the listener's TIDAL account use the existing provider matching
and delivery code. Credentials and recognition keys are encrypted with AES-GCM under Android
Keystore. TIDAL device sign-in displays the code and authorization page, and renewed refresh
tokens are saved. The user chooses a write-granted music folder per download. A data-sync
foreground service retains the engine and offers cancellation.

**A download is a job** (`downloads.rs`), several at once: `begin_download(song)` answers its
number, `find_download(job, accounts)` races every account through `Providers::first` (registration
order, `TURNED_TO_APART`, the grace, `ANSWERS_WITHIN`) and opens the offer taken, falling to the
held-back offers in rank order where one is `Gone` or refused, so the `DownloadOffer` names the
extension, the answering `MusicProvider` and the `Opened::Reading` length before the front end
makes a document. `download_offer(job, descriptor)` copies into it; `index_download(job, uri,
name)` validates and indexes. `download_progress(job)` answers `DownloadProgress` (`DownloadPhase`
Finding, Downloading, Validating, Done, Failed, Cancelled; bytes received; the total where the
provider said one; the provider); `cancel_download(job)` is heard between provider answers, while an
offer opens (on its own thread, looked at every 50 ms) and between 64 KiB chunks, and a cancel
pressed before the find stays (nothing resets it). Shutdown cancels every job; at most 16 settled
jobs are kept. A failure is a typed error whose message is the sentence the app shows, naming the
provider and cause (`NoProvider`, `NotFound`, `Unreachable`, `ProviderAway` (also a provider left
behind unanswered), `ProviderRefused` with the status, `AccountRefused` with the code,
`ProviderUntrusted`, `ProviderMisaddressed`, `ProviderUnreadable`, `WrongSong`,
`DownloadCancelled`); with nothing found, the first provider failure is told before `NotFound`. A
delivery `index_download` refuses as `WrongSong` is declined by every later find of that song for
`REFUSALS_REMEMBERED_FOR` (30 days, the desktop's), across restarts: V8's `refused_offers` keeps it
by the song's wanted identity and the place it was taken from, and each refusal written sweeps the
rows past that age.

Only a fresh SAF document is opened for writing; its URI is journalled before streaming, bytes are
bounded at 8 GiB, and codec probe/duration validation (within `LENGTHS_AGREE_WITHIN`) precedes
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
desktop does (V7 `tried`, `misses`): an offer zeroes the misses, nothing heard from every provider
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
dialled, which runs with online off). A setting change restarts it; `sync_start` with a blank key
reuses the code of the sync it replaces, so two starts in a row never mint two codes.

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
waiting song as held so it is not asked for twice; a song `land` finds already held (indexed or
waiting) is answered landed and its staged copy dropped, never queued a second time. Kotlin polls `sync_received`, creates a document in
the chosen writable music folder (the first one unless chosen), journals its URI as
`unfinished-sync-landing`, and hands the fresh descriptor to `sync_receive`, which copies the bytes,
validates and indexes the document as a download is, and forgets the waiting song whether or not that
succeeded, telling the sync service of a failure (`Sync::refused`) so the song is not fetched again at
once. **A failed landing holds back that song alone**: it deletes only the new document and waits a
minute before that song is tried again, the rest landing meanwhile; a song refused
`LANDING_TRIES_AT_MOST` (3) times in a row is dropped with `sync_refuse_received`. A received song's
file name drops `/ \ ? : * " < > |` and control characters, so a FAT, exFAT or SMB folder takes it.
`SyncedDevice.id` names a device (the hex `DeviceId`; names repeat after a reinstall), which
`sync_forget_device` takes; `HandedOff.id` likewise, and `sync_continue_from` and
`sync_fetch_missing` take the id (a name is still read where no id matches). The sync screen's
pairing code stays hidden until *Show pairing code* is pressed; its text is absent from the
accessibility tree while hidden. `song_uris` lists every document's URI, hidden ones included.

`ACCESS_NETWORK_STATE` lets the default-network callback report `NET_CAPABILITY_NOT_METERED`; songs
move only on an unmetered network unless mobile data is allowed. While sync runs Kotlin holds a
`WifiManager.MulticastLock` (`CHANGE_WIFI_MULTICAST_STATE`), so beacons and Bonjour from devices on
the same network are heard (`sync.md`'s *Nearby*); the status text counts them when the server is away and a
playlist's menu offers fetching from a nearby device whatever the server's standing. Kotlin polls the status every two
seconds while the Activity is started (30 s otherwise), refreshes the library and playlists when
`SyncStatus.arrivals` moves, and keeps the data-sync foreground service while songs move or wait.
Playlist menus offer fetching their missing songs from each online device and keeping their songs
here; the queue offers continuing what another device was playing, starting it through the Media3
controller when that device was playing.

Remaining desktop capabilities are open work in `docs/FEATURES.md`.

## Checks

From `android/flutter/`, run `flutter pub get`, `flutter analyze` and `flutter test`. The Flutter
tests cover repeated rows, identities, favourite updates, feedback, restoration and large text.

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
