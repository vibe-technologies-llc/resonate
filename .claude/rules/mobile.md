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
playlists, sync bridge, downloads, online orchestration) is described once in `android.md`'s
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

## Output

`output/mod.rs` picks one backend per target behind `output::player` and one `Routing`:

- **Android**: Oboe, shared stereo float at 48 kHz (`android.md`).
- **iOS**: RemoteIO at the session's hardware rate, interleaved Float32 stereo; the engine resamples
  into it. Swift reports the rate and the output latency (`outputLatency + ioBufferDuration`)
  through `output_route_changed`; a moved rate announces `SinkChange::Reformatted` and rebinds with
  `SetSink(None)`, as does `rebind` (a media-services reset rebuilds the unit at an unchanged rate).
  The render callback writes the source's bytes straight into the system's buffer and zeroes a short
  fill; the stream clock is the reported latency. Drain waits that latency after the ring empties.
- **Elsewhere** (host tests): PipeWire through `Player::with_sources`.

## Checks

`cargo test -p resonate-mobile` runs on the host. The queue edit tests drive a real engine over
a backend with no device (a paused queue never binds a stream) and WAVE documents through the
`content` provider. Its documents tests cover both schemes and
`the_ios_document_types_admit_what_a_scan_admits` holds `ios/Resonate/Info.plist`'s declared
extensions to `AUDIO_EXTENSIONS`. Target clippy runs through `android/check-native.py` and, for
iOS, `cargo clippy -p resonate-mobile --target aarch64-apple-ios` with the environment
`ios/build-native.py` sets. The layering job refuses PipeWire, GPUI, MPRIS, Discord and Oboe in
`resonate-mobile`'s iOS tree.
