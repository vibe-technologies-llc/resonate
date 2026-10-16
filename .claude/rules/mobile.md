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
`objc2-audio-toolbox`, and `output/remote_io_render.rs` reads the buffer list the system hands its
render callback. Domain, codec, DSP, engine and all other crates remain under `forbid`. New errors
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

`snapshot` consumes (it drains engine events, takes the failure and counts listening), so a caller that
only needs the transport asks `transport`, which reads the engine's state and nothing else.

## The catalog does not hold the transport

Nothing the transport needs waits on the catalog's one connection. `snapshot` copies the engine's
events, state and queue under the player lock and reads the catalog after releasing it (and
`store_counting` takes the connection only with something to store), so a long catalog hold delays
the poll alone, never play, pause or seek (`a_snapshot_waiting_on_the_catalog_does_not_hold_up_the_transport`).
The long holds themselves are short: the search index is read in pages of 2 000 rows with the lock
taken per page (stamped with the revision read before the first, so a write between pages makes the
next ask rebuild), and a playlist file or a link's matches take the lock per song over the
`folded(name)`, `folded(title)` and `recording` indexes
(`a_playlist_import_looks_each_song_up_by_index_not_by_scanning_the_catalog`).

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
included, is `InvalidUri`.

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

- **Android**: Oboe, shared stereo float at 48 kHz (`android.md`).
- **iOS**: RemoteIO at the session's hardware rate, interleaved Float32 stereo; the engine resamples
  into it. Swift reports the rate and the output latency (`outputLatency + ioBufferDuration`)
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

## Reading document rows

`catalog::read` takes a document row by position, the V1 and V2 columns in order; a migration only
appends columns, so `SELECT *`/`d.*` stays readable. A query adding its own columns after `d.*`
reads them by name (`position`, `listen_id`), never by index, since the next migration moves them.

## Checks

`cargo test -p resonate-mobile` runs on the host. The queue edit tests drive a real engine over
a backend with no device (a paused queue never binds a stream) and WAVE documents through the
`content` provider. The enrichment tests drive the pass over a fake `Reference` through `Asking`
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
