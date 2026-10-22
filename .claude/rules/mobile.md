---
paths:
  - "crates/resonate-mobile/**"
  - "crates/resonate-bindgen/**"
  - "android/build-native.py"
  - "ios/build-native.py"
---

# The mobile boundary

`resonate-mobile` is the one native boundary both phone front ends sit on: UniFFI proc-macros
export `MobilePlayer` and its records, and `resonate-bindgen` writes Kotlin (`android.md`) and Swift
(`ios.md`) from the same library metadata. Behaviour that is the boundary's (the document catalog,
playlists, sync bridge, downloads, online orchestration, the enrichment pass) is described once in `android.md`'s
sections and holds for iOS unchanged; this file holds what is the crate's alone.

## Unsafe

`resonate-mobile` is the sole exception to the workspace's unsafe prohibition, explicitly approved
for the native boundary. UniFFI generates the FFI scaffolding; the handwritten unsafe operations are
two. **An owned descriptor**: the front end hands Rust a descriptor it opened (Android detaches
`ParcelFileDescriptor`, iOS calls `open(2)`), transferred once and closed by Rust's `File`; a
negative descriptor is refused. The same transfer (`downloads::fresh_destination`) owns a newly
created download or received-song destination, never an existing music document. **The iOS audio
unit**: `output/remote_io.rs` creates, configures and disposes a RemoteIO unit through
`objc2-audio-toolbox`, and `output/remote_io_render.rs` is the render callback, whose `output/remote_io_buffers.rs` reads the
buffer list the system hands it and passes the bytes on to `Rendering::fill`. Domain, codec, DSP, engine and all other crates remain under `forbid`. New errors
are typed and bounded as in `errors.md`.

## A panic is an error on the phones

The workspace's release profile aborts on a panic, which is right for the desktop binary and wrong
across the boundary: UniFFI's `catch_unwind` turns a panic in an exported call (a listener's
corrupt file, a catalog row nobody foresaw) into an error the app shows, but only if the library
unwinds. Both `ios/build-native.py` and `android/build-native.py` therefore build `resonate-mobile`
with `CARGO_PROFILE_RELEASE_PANIC=unwind`, so a release build and a debug build fail alike. The
render callbacks abort on a panic, as an unwind may not cross them: `AURenderCallback`'s type is
`extern "C-unwind"`, so `remote_io_render::render` catches the panic itself and calls
`process::abort`.

## An error is the sentence the app shows

`Error` is `flat_error`, so a front end sees its `Display` alone: a variant a listener can meet is
written as a capitalised sentence composed from its typed fields (`MusicProvider`'s `Display` names
the service, `DownloadOp`'s says what the download could not be), never a debug print of an op.
`downloads::failure_of` is the one reading of a provider's error into those variants, for a find, an
opening and the TIDAL sign-in alike.

## A failure is told by its cause

`failure.rs` reads `resonate_engine::Cause` (the desktop's reading of an engine error, with
`Cause::of_a_read` for a codec error) into the sentences the desktop's toast says, worded for songs:
`Snapshot.failure` is `would_not_play` (naming the song's title where the queue row and catalog still
know it) or `would_not_do` for a refused command, and `Error::Player` and `Error::Decode` display
`of_a_command` and `of_a_read`, so a missing file, an unsupported format, a damaged file and a refused
device read differently. `Error::Reference` displays `of_a_lookup` (what was asked, through
`LookupOp::asked_for`, and how it failed), and `StoreOp`, `SyncOp` and `ReceivedOp` implement `Display`,
so no variant prints an op with `{:?}`. The report text the phones show is `Finding::told` and
`Container`'s `Display`, never a debug print.

**A wait for a device is a state, not a failure.** `Event::Waiting` (the output went, the row is held
at its frame and plays on once a device is there) becomes `Snapshot.waiting`, the desktop's toast
sentence (`waits_for_a_device`: no speakers or headphones, or the device stopped playing, each
*playing on once one is there*). It stands in every snapshot, a pause included, until an output is
bound again or the transport stops (`waiting_is_over`), so a front end shows it while it is true and
takes it down when playback goes on
(`a_snapshot_says_there_is_no_device_to_play_through_until_the_transport_stops`).

`snapshot` consumes (it drains engine events and takes the failure), so a caller that
only needs the transport asks `transport`, which reads the engine's state and nothing else.

## The engine's events are heard on a thread of the boundary's

`watching.rs` keeps one thread per player (`resonate-watch`, holding a `Weak` so a dropped player
ends it, and ending when the engine's event channel closes), which owns two jobs a front end's poll
used to do. **It counts listening**: on each engine event and otherwise every second while playing
(every two seconds otherwise, so a resumed song's first step stays inside `A_SEEK`), it reads the
state and queue under the player lock and hands them to `Listening::heard`; a failure to store a
listen is warned once until counting recovers. `snapshot` does not count, so no front end's poll
rate decides what a listen is. **It tells the front end**: `watch_player(PlayerWatch)` registers a
callback whose `moved()` is called, off every lock, when an event that changes what is shown arrives
(`TrackStarted`, `TrackFinished`, `QueueFinished`, `OutputChanged`, `Failed`, `Waiting`,
`CommandFailed`, however many arrive at once: one call), so a front end need not poll fast to see a
song change the engine already knows. The thread and `snapshot` both drain the events (whichever
reads one stores its failure or wait through `take_in`); a poll that finds none was just told
(`engine_events_that_change_what_is_shown_tell_the_watch_once_however_many_arrive`,
`an_underrun_alone_does_not_wake_the_watch`).

**A song being opened is the song a snapshot names.** The engine opens every row off its thread and
publishes no `current` meanwhile (`audio.md`'s *Every track opens off the engine thread*), so a
poll landing in a skip read no song and the phones drew an empty player, then the next song with
nothing to animate from. Where the engine names no track while buffering or paused, `current` is the
queue row at `queue_position` (`row_being_opened`), at position 0 with the catalog's length, until
the opening lands (`a_row_still_opening_is_the_song_a_snapshot_names`).

## The catalog does not hold the transport

Nothing the transport needs waits on the catalog's one connection. `snapshot` copies the engine's
events, state and queue under the player lock and reads the catalog after releasing it (and
`store_counting` takes the connection only with something to store), so a long catalog hold delays
the poll alone, never play, pause or seek (`a_snapshot_waiting_on_the_catalog_does_not_hold_up_the_transport`).
A steady tick takes no catalog lock at all: the playing row's `Track` is kept with the URI and the
catalog revision it was read at (`Catalog::kept_track_uri`), read again only when either moves
(`a_snapshot_keeps_the_playing_song_until_the_catalog_moves`). `save` copies the place and rows
under the player lock and turns URIs into ids after releasing it. A run of rows by URI or id
(`tracks_at`, `ids_at`, `tracks_by_id`: the queue read, a save, `load`, the resumption at open) is
read `ROWS_READ_PER_HOLD` (256) to a hold, so a whole-library queue lets a snapshot or a favourite
in between (`rows_read_across_several_holds_keep_the_order_asked`).
The long holds themselves are short: the search index is read in pages of 2 000 rows with the lock
taken per page (stamped with the revision read before the first, so a write between pages makes the
next ask rebuild), and a playlist file or a link's matches take the lock per song over the
`folded(name)`, `folded(title)` and `recording` indexes
(`a_playlist_import_looks_each_song_up_by_index_not_by_scanning_the_catalog`).

## Logging

`logging.rs` is the phones' `tracing` subscriber, safe code only: `keep_log(logbook)` installs once
per process (`false` when a subscriber already stands) a `fmt` subscriber at `WARN` whose writer
hands each finished line, with its `LogLevel`, to the front end's `Logbook` callback, so there is no
`liblog` binding and no unsafe. Android installs it before the player opens (`Logcat.install`, tag
`ResonateNative`, mapped to `Log.e`/`w`/`i`/`d`), so an engine, library or sync warning reaches
logcat; iOS installs none yet
(`a_warning_reaches_the_logbook_with_its_level_and_a_debug_line_does_not`).

## The surface is the same on every target

Every export exists on every target, so bindings generated from the host library match the device
library (`ios/build-native.py` falls back to the host build where library mode cannot read the iOS
archive). What only one platform uses is a no-op elsewhere: `select_output`/`output_device` act on
Android's Oboe routing; `output_route_changed(rate, latency_ms, rebind)` feeds iOS's route.
`audio_extensions` is core's `AUDIO_EXTENSIONS`, so a front end never keeps its own list. No
exported name is a Swift or Kotlin keyword: UniFFI backtick-escapes one in Swift and the backticks
leak into the generated C header, which then fails to compile (hence `Documents::open_document`).

## Granted documents

A document is a URI the front end resolves to a descriptor through the `Documents` callback.
`GRANTED_SCHEMES` lists the schemes a `GrantedFiles` provider is registered for, one `SourceId` each:
`content` (Android's SAF URIs, unchanged since V1, so saved locations still resolve) and
`resonate-folder` (iOS: `resonate-folder://<root>/<percent-encoded relative path>`, the root a
security-scoped bookmark the app keeps, or `local` for its own Documents folder). The key is the URI
after `scheme:` with its escaping kept exactly. Anything else, a bare path or a `file:` URI
included, is `InvalidUri`. A negative descriptor says why: `document_not_found()` (no such
document: Android's `FileNotFoundException`, iOS's unresolvable bookmark or `ENOENT`) reads as
`NotFound`, `document_refused()` (any other failure) as `PermissionDenied`, and
`hide_missing_songs` hides only a `NotFound`, so a provider's transient error leaves a song visible
(`only_a_document_that_is_not_found_reads_as_not_found`). `hide_unlisted_songs(prefixes, listed)` is
the same pass given the URIs the front end saw listed: those are taken to exist and never opened
(`a_document_the_listing_shows_is_not_opened_and_the_rest_are`); `hide_missing_songs` is it with
none listed.

## A day is the phone's zone's

`listening_detail` takes the phone's IANA zone by name beside its current offset, and cuts each day
at that zone's own midnight through `resonate_core::Calendar::named`, so a winter day read in summer
still begins at its winter midnight; the offset is only the fallback for a name no zone database
holds. iOS passes `TimeZone.current.identifier`; Android's statistics read `statistics(days)` alone
and call no day-cutting export yet. Which database each target reads, and why the phones bundle one
behind the system's, is `library.md`'s *A day is the listener's*.

## Lyrics

`lyrics.rs` hands a phone what the desktop pane reads, so neither phone keeps lyric arithmetic of
its own. `LyricSheet` is the sheet once: `detail`, `two_voices`, `end` (the *END OF LYRICS* row,
`lines.len()`), `written_end`, and per row `blank`, `written` (written lines before it, the
falloff's ordinal) and `breathes` (`Lyrics::breathes_before`: room for the dots). `lyric_frame` is
asked at display rate, so it is a lock, a lookup among at most `SHEETS_KEPT` (16) and the
arithmetic: `line` (`line_at`) and `active` stay for Android; `voice_one`/`voice_two`
(`voices_in_play`); `read_at` (`waiting.next`, else `end` once `has_ended`, else `line_at`); `reads`
chosen exactly as `LyricsModel::follow_the_track` chooses (`Evenly` unsynced; `At` the latest lit
line, else the end, else, with no wait, `read_at`; else `Spent`); `waiting`; `ended`; and a
`LyricWipe` per lit line with words, `Sweep::wiped` turned from UTF-8 bytes into UTF-16 code units,
how Swift and Kotlin index a string (`a_wipe_counts_both_halves_of_a_surrogate_pair`).

`kept_lyric_frame` is the same frame answered only from a kept sheet (`None` where none is, unlit
for a `Wordless` song), never the catalog, so a front end may ask it inside a display tick
(`a_kept_frame_answers_only_from_a_sheet_already_held`).

The kept sheets are least recently *read* first out (`KeptSheets`; a frame reads, so the playing
sheet is never the one dropped: `a_sheet_read_lately_outlasts_the_sheets_remembered_after_it`). A
frame for a sheet not kept reads it from the catalog again rather than answering nothing; a song
the catalog holds no words for is kept `Wordless`, so its frames ask SQLite once, while
`cached_lyrics` (behind `lyrics` and the keeping calls) reads the catalog again past `Wordless`.

## The equaliser

The phones' curve, its bounds, the drawn response, imports and stored form are `eq.md`'s *The
phones* (`equaliser.rs`); `playback.rs` sends it with the other sound preferences.

## Output

`output/mod.rs` picks one backend per target behind `output::player` and one `Routing`:

- **Android**: Oboe, shared stereo float at 48 kHz, power-saving and music-typed; the output thread's
  waits and the bounded open are `output/waits.rs`, compiled on the host as well so its tests run
  there (`android.md`).
- **iOS**: RemoteIO at the session's hardware rate, interleaved Float32 stereo; the engine resamples
  into it. **What does not touch CoreAudio is compiled on the host too, so its tests run there**:
  `output/remote_io_state.rs` holds `Routing` (the rate, latency and sink-change atomics and channels)
  and `Drain` (when a drain is over: the latency after the ring empties, never under 20 ms, restarted
  by a source that fills again), `output/remote_io_fill.rs` holds `Rendering::fill` (a short fill
  zeroed, the `empty` flag set exactly when the source gave nothing). `remote_io_buffers.rs` reads the system's buffer list
  (`objc2-core-audio-types`, plain C structs that build on the host, so the pointer handling is proved
  there: `the_systems_buffer_is_filled_up_to_the_bytes_it_declares_and_no_further`, a null list, a
  buffer with no memory, one declaring no bytes, the list's size and alignment). `remote_io.rs` and
  `remote_io_render.rs` are `cfg(target_os = "ios")` and keep only the unit, the callback's panic
  catch and the output thread's loop (`a_short_fill_zeroes_the_rest_and_leaves_the_flag_down`,
  `a_drain_waits_the_latency_after_the_ring_empties_and_then_settles_once`,
  `a_moved_rate_is_announced_once_and_an_unmoved_one_is_not`). Swift reports the rate and the output latency (`outputLatency + ioBufferDuration`)
  through `output_route_changed` (after asking the session for the playing song's rate, `ios.md`'s
  `NowPlaying`); a moved rate announces `SinkChange::Reformatted` and rebinds with
  `SetSink(None)`, as does `rebind` (a media-services reset rebuilds the unit at an unchanged rate).
  A refused `AudioOutputUnitStart` or stop is a `StreamEvent::Refused` carrying the `OSStatus`, so the
  engine's failure names the operation and the status (an inactive session, an interruption not yet
  ended). The output thread sleeps until a command or a moved latency (`Routing::route_changed`) wakes it, and polls every 10 ms only while draining. The render callback writes the source's bytes straight into the system's buffer and zeroes a short
  fill; the stream clock is the reported latency. Drain waits that latency after the ring empties.
  `close` returns once the output thread has disposed the unit (waiting at most 3 s), so a rebind
  never opens the next unit beside a living one, and an open waits at most as long for the unit
  (`UNIT_WITHIN`): a media-services stall in `AudioComponentInstanceNew` is a timed-out open that
  the engine parks, not a blocked engine thread, and the late unit is disposed by its own thread.
- **Elsewhere** (host tests): PipeWire through `Player::with_sources`.

## TIDAL sign-in

`authorize_tidal` asks TIDAL for a device code and `finish_tidal_authorization` waits for it on the
caller's thread, as the seam's `SignsIn::authorized` does. Each `authorize_tidal` is an attempt
numbered before anything is asked (`Downloads::sign_ins`); `cancel_tidal_authorization` moves the
number on, so the wait's cancel (`Ok(None)`, heard within the seam's `LOOKED_AT_EVERY`) ends it, and
an attempt cancelled while its code was still being asked never stores the code a later attempt
would wait on (`a_sign_in_cancelled_and_asked_again_waits_for_the_new_code_alone`,
`a_cancel_ends_a_sign_in_waiting_for_its_code`).

## Reading document rows

`catalog::read` takes a document row by position, the V1 and V2 columns in order; a migration only
appends columns, so `SELECT *`/`d.*` stays readable. A query adding its own columns after `d.*`
reads them by name (`position`, `listen_id`), never by index, since the next migration moves them.

## Checks

`cargo test -p resonate-mobile` runs on the host. The queue edit tests drive a real engine over
a backend with no device (a paused queue never binds a stream; `Harness::sounding` lists one sink
whose stream nothing pulls, for a queue that plays) and WAVE documents through the
`content` provider; that `Harness` (`harness.rs`, test-only) is shared by the organise, inspector,
analysis, snapshot and listening tests. A listen counted for a song the catalog no longer holds
stores nothing and settles nothing: `store_counting` keeps no visit unless the insert changed a row
(`a_listen_counted_for_a_song_the_catalog_lost_does_not_settle_over_another_listen`). The enrichment tests drive the pass over a fake `Reference` through `Asking`
(the reference and the client whose switch it reads): one cover per album, a miss left alone until
its wait is over, a prompt stop, the mark kept on closing, the album key and the artwork fallback;
a V6 catalog migrates keeping ids, favourites and resumption. Its documents tests cover both
schemes and
`the_ios_document_types_admit_what_a_scan_admits` holds `ios/Resonate/Info.plist`'s declared
extensions to `AUDIO_EXTENSIONS`. `a_two_word_search_over_twenty_thousand_songs_answers_within_its_budget`
holds the search index (`android.md`) to 30 ms, the best of five, over 20 000 rows (150 ms with
debug assertions, where the workspace's optimised debug profile still runs it). Target clippy runs through `android/check-native.py` and, for
iOS, `cargo clippy -p resonate-mobile --target aarch64-apple-ios` with the environment
`ios/build-native.py` sets. The layering job refuses PipeWire, GPUI, MPRIS, Discord and Oboe in
`resonate-mobile`'s iOS tree.

## Hosted services on the phones

**`configure_hosted_services(enabled)` is the desktop's `hosted-services` on a phone** (default on, kept
by each phone beside its online switch and applied at start): `Downloads::find` drops a Monochrome or
hifi-api account whose server is blank while it is off, so a named server is still asked and nothing
else is, and an empty remainder is `NoProvider`
(`declined_hosted_services_leave_a_hosted_account_unasked_and_a_named_server_asked`). The switch is
*Ask the hosted services* on each phone's provider screen.
