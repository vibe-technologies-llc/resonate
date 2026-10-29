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
(`ios.md`) from the same library metadata. Behaviour that is the boundary's (document catalog, playlists, sync bridge, downloads, online
orchestration, enrichment) is described once in `android.md` and holds for iOS unchanged; this file
holds what is the crate's alone.

The phones have no vault and none is planned: the boundary never opens one, and a song stays the
listener's own file.

## Unsafe

`resonate-mobile` is the sole exception to the workspace's unsafe prohibition, explicitly approved
for the native boundary. UniFFI generates the FFI scaffolding; the handwritten unsafe operations are
two. **An owned descriptor**: the front end hands Rust a descriptor it opened (Android detaches
`ParcelFileDescriptor`, iOS calls `open(2)`), transferred once and closed by Rust's `File`; a
negative descriptor is refused (`documents::owned`, for a document read through `Documents` and one replaced through `DocumentRewriting` alike). The same transfer (`downloads::fresh_destination`) owns a newly
created download or received-song destination, never an existing music document. **The iOS audio
unit**: `output/remote_io.rs` holds the AudioToolbox calls that create, configure and dispose a RemoteIO unit
(`objc2-audio-toolbox`), sequenced by `output/remote_io_unit.rs`, and `output/remote_io_render.rs` is the render callback, whose `output/remote_io_buffers.rs` reads the
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

**A measurement that will not fit says which file** (`measurement.rs`): `Error::Measurement { file,
source }` carries `MeasuredFile::{Measurement, Target}` and the boxed `resonate_eq::Error`, and
`failure::of_a_measurement` words each refusal (too large, no point, too few points, a span that
misses 500 Hz to 2 kHz, nothing to correct) as a sentence naming the file; a built-in target no one
is called is `UnknownTarget { name }`. `fit_measurement` takes the measurement's bytes and a
`MeasurementTarget` (a built-in's name or a document's bytes), is synchronous CPU work for the
caller to run off its main thread, and `measurement_offers()` hands over the built-ins and the byte
limit (`eq.md`'s *A measurement of your own*). Its result field is `imported`, not `import`, a Swift
keyword.

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

**A snapshot carries the open stream's figures** (`OutputFigures::of`): `output_rate`, `underruns`
and `overs`, the samples the chain clamped at full scale (the desktop inspector's *clamped
samples*, `audio.md`), all 0 with no stream open
(`a_snapshot_carries_the_clamped_samples_of_the_open_stream_and_none_without_one`). Android's
*Audio output* page shows `overs` as a *Clamped samples* row only once it is above 0
(`clampedSamplesWords`); iOS's output page shows the same in *Current stream*, read from `PlayClock`.

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
the opening lands (`a_row_still_opening_is_the_song_a_snapshot_names`). **A seek in that window is kept**:
`seek` sends `Command::Seek` at the opening row's catalog rate (`rate_to_seek_in`), which the engine holds as the
place the opening lands at (`a_seek_in_a_row_still_opening_is_sent_at_that_rows_rate`). **A save in that window
writes nothing** (`place_to_save`): a place of 0 would rewrite the queue over the place the previous song
reached, and the previous place stays true until the new row publishes
(`a_save_waits_for_the_row_opening_to_publish_its_place`).

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
archive). What only one platform uses is a no-op elsewhere: `select_output`/`output_device`,
`select_zone_output`'s device,
`set_follow_song_rate`, `output_offers_changed`, `output_stream` and `watch_output_mixer` act on
Android's Oboe routing (iOS asks its session for the song's rate in Swift, so its `Routing` follows
nothing and offers nothing); `output_route_changed(rate, latency_ms, rebind)` feeds iOS's route.
`audio_extensions` is core's `AUDIO_EXTENSIONS`, so a front end never keeps its own list. No
exported name is a Swift or Kotlin keyword: UniFFI backtick-escapes one in Swift and the backticks
leak into the generated C header, which then fails to compile (hence `Documents::open_document`).

## Granted documents

A document is a URI the front end resolves to a descriptor through the `Documents` callback.
`GRANTED_SCHEMES` lists the schemes a `GrantedFiles` provider is registered for, one `SourceId` each:
`content` (Android's SAF URIs, kept as they are so saved locations still resolve) and
`resonate-folder` (iOS: `resonate-folder://<root>/<percent-encoded relative path>`, the root a
security-scoped bookmark the app keeps, or `local` for its own Documents folder). The key is the URI
after `scheme:` with its escaping kept exactly. Anything else, a bare path or a `file:` URI
included, is `InvalidUri`. A negative descriptor says why: `document_not_found()` (no such
document: Android's `FileNotFoundException`, iOS's unresolvable bookmark or `ENOENT`) reads as
`NotFound`, `document_refused()` (any other failure) as `PermissionDenied`, and
`hide_missing_songs` hides only a `NotFound`, so a provider's transient error leaves a song visible
(`only_a_document_that_is_not_found_reads_as_not_found`), and the sync catalog's `holds` reads only a
`NotFound` as not held, the same rule (`android.md`'s *Device sync*). `hide_unlisted_songs(prefixes, listed)` is
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

## The listening test

`abx.rs` hands the phones core's ABX session (`eq.md`'s *The listening test*): `ListeningTest` is a
UniFFI object holding the `Session`, the levelled `Arms` and the arm heard; `MobilePlayer` sends the
arms (`hear_in_listening_test`) and puts the preferences' equaliser back (`end_listening_test`), so
the object never touches the engine itself and a front end holds one per run. Refusals are
`Error::ListeningTest*` sentences; the core errors it cannot name pass as `Domain`.

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
`cached_lyrics` (behind `lyrics` and the keeping calls) reads the catalog again past `Wordless`. A catalog read that
fails is not kept: the frame is unlit for that call and the next frame asks again
(`a_failed_reread_is_asked_again_by_the_next_frame`).

## Answers from other devices

`DocumentCatalog` takes answers (`takes_answers` true) by the rules the desktop's `answers.rs`
keeps (`sync.md`), over the document catalog, in `sync/answers.rs`. A release (`Answer::Release`)
is settled and written nowhere, since the document catalog keeps no release a strict landing could
weigh; the ledger still passes it on in its snapshot.

- **Words.** `lyrics_to_share` lists every `lyrics` row with text of a visible document, in `asked`
  order (`at` is `asked` in nanoseconds), timed or not by `is_synced`, and only those `Answer::travels`
  admits. `take_lyrics` lands only on a document the recording is held strictly by (the in-memory
  `Indexed` resolves it, else `songs_received` does, through `resolved_in`, then the document's recording id is the recording's, or both lengths are
  known and within 2 s), parsed first, as a row with no Lyricsfile and origin `SHARED_LYRICS`, and
  never where the row keeps words as rich already (`LyricText::detail`). `SHARED_LYRICS` is due for
  the pass's Lyricsfile retry as `FETCHED_LYRICS` is. The kept sheet is dropped from `KeptSheets`
  (shared with the player as an `Arc`), so a pane never draws what landing replaced.
- **A study is the file it measured.** The catalog never knew a file by its packets, so
  `documents.packets` and `documents.frames` (the first-packets digest and frame count
  `probe_scanned` reads, the desktop's `tracks.packets` and `tracks.duration`) are kept by the
  study pass right after it studies (`Hearer::keep_identity`) and by landing, which probes a
  document only when its length (`documents_by_length`: rate and `duration_ms` within a
  millisecond) could be the study's and its identity is not yet kept; the identity goes with the
  file (`documents_forget_which_file_they_were` clears it when `size` or `modified` moves).
  `studies` gained `studied`, `studied_under` (`JUDGED_UNDER`) and the figures the catalog lacked
  (`cutoff_hz`, `cutoff_drop`, `lossy_guess`, `upsampled_from`, `declared_bits`,
  `loudness_range`, `mono_as_stereo`), all written by `keep_studied`; `keep_study` (the analysis
  pane, which has no such figures) leaves `studied_under` null. `studies_to_share` lists a row
  judged under this build with whole band levels (the file-change trigger clears them) on a
  document with a kept identity; a study from before the migration is listed once its song is
  studied again. `take_study` lands on every visible document whose rate, frame count and
  packets digest are the study's, never over a row `studied_under` this build (a row with no
  `studied_under` is a study from before and stays too), over one of another judgement it replaces and
  clears its `agreement`. A verdict, lossy guess, judgement or band layout this build cannot
  name is settled and written nowhere. **The band peaks travel as an answer of their own**
  (step 16's `band_peaks`, whole at `BAND_PEAKS_BYTES` since step 17 cleared the one-moment
  layout, `sync.md`): `band_peaks_to_share` lists a row judged under this build with whole peaks on a document with a kept identity, the same `WholeFile` as the study, and
  `take_band_peaks` writes them on every visible copy of that file studied under this judgement
  whose peaks are not already whole, never over peaks held; a judgement or length this build cannot
  read is settled and written nowhere, and a copy not yet studied takes nothing (the study is
  listed first, so it lands first; a phone that missed it measures its own peaks alone). A study
  landed without peaks reads as unmeasured to `studies_without_bands` until they arrive or the pass
  measures them (`the_peaks_of_a_study_land_on_every_studied_copy_of_the_same_file_and_never_over_peaks_held`,
  `peaks_of_another_encode_judgement_or_layout_are_written_nowhere`,
  `the_peaks_a_phone_kept_are_listed_beside_its_study_with_the_file_it_measured`).
  (`a_study_lands_on_every_copy_that_is_the_same_file_and_on_no_other_encode`,
  `a_study_of_another_encode_another_length_or_another_rate_is_never_written`,
  `a_study_never_lands_over_one_at_this_judgement_but_replaces_one_of_another`,
  `words_land_only_on_a_song_held_strictly_and_never_over_as_rich_words`,
  `landed_studies_are_listed_for_sending_exactly_as_they_arrived_beside_kept_words`,
  `a_study_the_phone_kept_itself_is_listed_with_the_file_it_measured`).
- **A print and its recognition are kept beside the study and asked of a device, not told.** The
  catalog's step 15 (`STUDIES_KEEP_THEIR_PRINT_AND_WHAT_IT_WAS_HEARD_AS`) gave `studies` the
  desktop's `print`, `print_length`, `recognised` (nanoseconds) and `heard_as`, `heard_score`,
  `heard_title`, `heard_artist`, all cleared by the `documents_forget_what_was_printed` trigger when a
  document's `size` or `modified` moves. The pass's `keep_studied` keeps the print its study made
  (the analysis pane's `keep_study` none, its row being judged under no build); `land_study`
  clears them where it replaces a study of another judgement. `recognised_through` (the pass and
  `recognise_track`) keeps the top match heard (`HeardAs::top_of`) and stamps `recognised` through
  `keep_recognition`; a study that made no print is `Unheard` with `recognised` left null, so a print
  landed later is still heard. The seam is the desktop's (`sync.md`), its shared halves
  `resonate-library`'s `KeptPrint::shared` and `weighed_again`: `prints_wanted` lists the
  `WholeFile`s (`documents.packets`, `frames`, `rate`) of visible documents studied under this
  `JUDGED_UNDER` whose `print` or `recognised` is null, one entry a file, the most lately studied
  first; `print` answers the print of such a copy, a recognised one first; `take_print` lands the
  print on each copy of that file studied under this judgement whose `print` is null, never over one,
  and the recognition on each whose `recognised` is null, weighed again against the copy's own name
  (`named_on_the_phone`). The service asks connected devices on the desktop's pacing. **The pass skips
  the second decode once a print is kept**: `Hearer::hear` recognises from `kept_print` without
  studying, and `tracks_to_hear` takes a row whose print is kept and recognition owed even where its
  own study left it `Unheard`
  (`a_print_and_its_recognition_from_another_device_land_on_every_copy_and_are_weighed_against_the_phones_own_names`,
  `a_print_and_recognition_the_phone_made_answer_another_device_lacking_them`,
  `a_print_alone_leaves_the_recognition_owed_and_one_that_heard_nothing_lands_as_unheard`,
  `a_print_kept_beside_a_study_is_forgotten_when_its_file_changes`,
  `a_print_landed_from_another_device_is_heard_without_decoding_the_song_again`).

## The equaliser

The phones' curve, its bounds, the drawn response, imports and stored form are `eq.md`'s *The
phones* (`equaliser.rs`); `playback.rs` sends it with the other sound preferences.

## Output

`output/mod.rs` picks one backend per target behind `output::player` and one `Routing`:

- **Android**: Oboe, music-typed, opened in whichever of three ways the plan's spec is offered by
  (`android.md`'s *The route's formats*). The boundary's vocabulary is `output/formats.rs`, on every
  target: `OutputFormat` (rate, `OutputEncoding` `Pcm16`/`Pcm32`/`Float`, one or two channels),
  `OutputPath` (`Mixed`, `Exclusive`, `BitPerfect`), `OutputStream` (what opened and how) and the
  `OutputMixer` callback (`prefer_bit_perfect(device, format)`, `mix_freely(device)`). What decides
  is compiled on the host as well, so its tests run there: `output/oboe_offers.rs` (`Offers`: the
  shared mixer's `MIXED`, 48 kHz stereo float, always, and while following the song the route's
  bit-perfect formats and its probed exclusive one, each its own `SinkFormats` entry so no rate and
  word are crossed; `Way` for a spec), `output/oboe_state.rs` (`Routing`: the requested and actual
  device, the switch, the offers, the probe made once per chosen device and route, the mixer
  preference held and given back, the stream told by a number so a late close never clears the
  next) and `output/oboe_frames.rs` (the callback's fill, realtime). The output thread's waits and
  the bounded open are `output/waits.rs`. **The stream is kept across a rebind** (`audio.md`):
  `output/oboe_reshape.rs` holds `Order` (what the output thread is sent: a `StreamCommand`, or
  `Reshaped`, which calls off a drain standing from before), `Opened` (the spec, the effective `Way`
  after the open's fallbacks and the device asked, sent back with the open) and `reshaper`, which
  keeps the stream only where the request is `Opened`'s spec, `Routing::requested` is its device and
  `Routing::way_for` its `Way`, handing the callback the new source through `output/handover.rs`;
  anything else is refused (`AnotherFormat`, `AnotherTarget`, `AnotherPath`) and the engine reopens
  (`a_stream_asked_for_the_shape_it_holds_is_given_the_new_source_and_kept`,
  `a_device_chosen_since_the_stream_opened_is_refused_so_the_stream_opens_on_it`,
  `a_route_that_now_offers_the_song_its_own_way_is_refused_so_the_stream_opens_that_way`).
- **iOS**: RemoteIO at the session's hardware rate, interleaved Float32 stereo; the engine resamples
  into it. **What does not touch CoreAudio is compiled on the host too, so its tests run there**:
  `output/remote_io_state.rs` holds `Routing` (the rate, latency and sink-change atomics and channels)
  and `Drain` (when a drain is over: the latency after the ring empties, never under 20 ms, restarted
  by a source that fills again), `output/remote_io_fill.rs` holds `Rendering::fill` (a short fill
  zeroed, the `empty` flag set exactly when the source gave nothing). `remote_io_buffers.rs` reads the system's buffer list
  (`objc2-core-audio-types`, plain C structs that build on the host, so the pointer handling is proved
  there: `the_systems_buffer_is_filled_up_to_the_bytes_it_declares_and_no_further`, a null list, a
  buffer with no memory, one declaring no bytes, the list's size and alignment). **The unit's sequence
  and the output thread are generic over what touches CoreAudio, so they run on the host too**:
  `remote_io_unit.rs` holds `Toolbox` (the nine AudioToolbox calls a RemoteIO unit takes: find the
  component, make an instance, set the format, set the render callback, initialize, start, stop,
  uninitialize, dispose) and `Unit<T: Toolbox>` (the open path: the format asked for is interleaved
  Float32 stereo at the route's rate, the callback is handed the `Rendering`, every refused step is
  an `Error::Backend` naming `Operation::Open` and the `OSStatus`, a unit made is taken down by
  `Drop` in the order stop, uninitialize, dispose and the `Rendering` is freed exactly once however
  far the open got); `remote_io_stream.rs` holds `Hardware` (start and stop), `launch` (the
  format check, the open thread, the `UNIT_WITHIN`-bounded wait for the unit, the close that waits
  for disposal) and `serve` (the commands, a refused start or stop told as `StreamEvent::Refused`
  with the status, the drain, the moved latency). Their tests drive a fake `Toolbox` and a fake
  `Hardware`. **The unit is kept across a rebind and a turn of the route's rate**: `Hardware` also
  has `reformat(rate)` (`Unit`: stop, uninitialize, set the format, initialize, stopping at the
  first step the system refuses and returning its status), and `launch` gives the stream a
  `Reshaper` that sends the output thread `Order::Reshape { rate }` (the route's rate now), waits at
  most `UNIT_WITHIN` for its status and only then hands the source to the render callback through
  `handover.rs`, so a refused reformat hands the source back and the engine closes the stream and
  opens another; the thread calls off a drain standing from before, reformats only where the rate
  moved and then announces `FormatChanged` (and the pause the reformat made)
  (`a_stream_asked_for_the_shape_it_holds_takes_the_new_source_and_the_unit_is_left_alone`,
  `a_drain_asked_before_a_reshape_is_called_off_so_the_new_source_is_not_drained`,
  `a_stream_that_drained_is_reshaped_and_started_again_rather_than_reopened`,
  `a_rate_the_route_took_since_the_unit_opened_reformats_the_unit_and_says_so`,
  `a_reformat_the_system_refuses_hands_the_source_back_and_announces_nothing`,
  `a_unit_reformatted_is_stopped_uninitialised_set_and_initialised_again_at_the_new_rate`; a unit
  reformatted on a device is not proved, below). The shape is still the route's alone
  (`a_shape_other_than_the_routes_is_refused_before_the_unit_is_touched`).
  Their other tests (`a_step_refused_after_the_instance_took_the_unit_down_and_frees_the_source_once`,
  `a_refused_start_is_told_with_its_status_and_leaves_the_stream_failed`,
  `an_open_that_never_answers_times_out_and_the_late_unit_is_disposed_by_its_own_thread`,
  `closing_returns_only_once_the_unit_is_disposed`). `remote_io_buffers::guarded` is the callback's
  panic catch, taking what to do on a panic (`process::abort` in `render`;
  `a_render_that_panics_is_caught_and_answered_by_the_abort_alone`). `remote_io.rs` and
  `remote_io_render.rs` are `cfg(target_os = "ios")` and keep only `CoreAudio`, the `Toolbox` whose
  methods are the unsafe AudioToolbox calls, and the callback's `extern "C-unwind"` shell
  (`a_short_fill_zeroes_the_rest_and_leaves_the_flag_down`,
  `a_drain_waits_the_latency_after_the_ring_empties_and_then_settles_once`,
  `a_moved_rate_is_announced_once_and_an_unmoved_one_is_not`). Swift reports the rate and the output latency (`outputLatency + ioBufferDuration`)
  through `output_route_changed` (after asking the session for the playing song's rate, `ios.md`'s
  `NowPlaying`); a moved rate announces `SinkChange::Reformatted` and sends
  `Command::FollowTheFormats`, so the engine reopens at the new rate once the survey has read it
  (`audio.md`), and `rebind` sends `SetSink(None)` (a media-services reset).
  A refused `AudioOutputUnitStart` or stop is a `StreamEvent::Refused` carrying the `OSStatus`, so the
  engine's failure names the operation and the status (an inactive session, an interruption not yet
  ended). The output thread sleeps until a command or a moved latency (`Routing::route_changed`) wakes it, and polls every 10 ms only while draining. The render callback writes the source's bytes straight into the system's buffer and zeroes a short
  fill; the stream clock is the reported latency, and a fill is stamped at the buffer's
  `AudioTimeStamp::mHostTime` (where `HostTimeValid`, read in nanoseconds through the
  `mach_timebase_info` read at open, against `mach_absolute_time` in the callback) plus that latency
  less the buffer's own frames (the `ioBufferDuration` the host time already counts; `realtime.md`).
  Drain waits that latency after the ring empties.
  `close` returns once the output thread has disposed the unit (waiting at most 3 s), so a rebind
  never opens the next unit beside a living one, and an open waits at most as long for the unit
  (`UNIT_WITHIN`): a media-services stall in `AudioComponentInstanceNew` is a timed-out open that
  the engine parks, not a blocked engine thread, and the late unit is disposed by its own thread.
- **Elsewhere** (host tests): PipeWire through `Player::with_sources`.

## Zones

**The boundary owns a `Zones`, never one player** (`zoning.rs`, `audio.md`'s zones). `Playing` holds
the `Zones`, the zone shown (`shown`, the first until another is chosen) and a `Zoned` per zone: its
own output `Routing`, failure, waiting, saved place and `Listening`, and the route string it was bound
for. Every call the phones made before addresses the zone shown (`Playing::player`, `send` through
`Zones::send`, so a setting, the equaliser and room correction reach every zone and a transport
command the shown one); the snapshot names it (`Snapshot.zone`) with `Zones::revision`
(`zones_revision`), so a front end reads a zone's queue again when another is shown, though its
revision may equal the last. `zones()` lists each zone as a `PlayerZone` (id, name, route, requested
device, shown, volume, transport and the title it plays), `show_zone`, `add_zone(name, device, route)` (a
blank name is *Zone N*), `rename_zone` (blank is `ZoneUnnamed`), `remove_zone` (the first is
`FirstZoneStays`; its listen is settled and its catalog rows go), `select_zone_output(zone, device,
route)` and `hand_over_to(zone)`, which hands the shown zone's song to another through
`Zones::hand_over` (`lockstep.rs`; the boundary does not wait on the handover thread) and shows that
zone. `select_output` and `output_device`/`output_stream` read the shown zone's `Routing`; follow-song-rate,
the route's bit-perfect offers, the mixer and iOS's route reports reach every zone's (`Outputs` keeps
the last of each for a zone opened later) and `FollowTheFormats` goes to the zones whose route moved.

**A zone opens over a `Routing` of its own** (`Opener`, the `Opens` the boundary hands `Zones`): the
first zone's sink is `OUTPUT`, a later one `Routing::serving(SinkId::new(zone))`, so the zones are
never one sink to `Zones::holder_of` (the phones' system mixes, and one zone's bit-perfect stream
refused every other zone a `SetSink`); its device is the id it was added or bound with (Oboe's
`setDeviceId`, 0 the system's choice), its engine the first zone's `OutputSettings::applied_to`, and
the room correction applied is sent to it once open. iOS has one route: every zone's RemoteIO unit
plays into it (`FEATURES.md`).

**A device another zone holds bit-perfect is refused, as the desktop refuses it** (`audio.md`'s
`Zones::holding`): each zone's sink being its own, `Zones::send` never sees two zones on one sink, so
the boundary compares the Android device ids. `Playing::holder_of(asking, device)` takes the zones
`Zones::holding` names (a zone whose settings prefer bit-perfect and whose output mode is
`BitPerfect` or `Repacked`) and finds one whose `Routing` requested `device`, or (for a device other
than 0, the system's choice) opened on it (`reaches`); `add_zone`, `select_zone_output`,
`select_output` and `hand_over_to` refuse it with `resonate_engine::Error::DeviceHeld { by }`
(`Cause::HeldByAnotherZone`'s sentence), a handover also where the holder is the zone handing over
and means to sound (`handover_clashes`, the engine's own rule). Two zones on a mixed device share it,
and a zone that turns bit-perfect after another chose the device is not undone, as on the desktop;
iOS holds no device per zone (one route) and refuses nothing (`OUTPUTS_ARE_HELD_PER_DEVICE`). The host's
`Routing` remembers the device selected so the rule is proved there
(`a_device_another_zone_holds_bit_perfect_is_refused_to_every_way_of_binding_a_zone_to_it`,
`a_device_is_reached_by_the_choice_made_or_the_one_the_system_gave`,
`a_handover_onto_a_held_device_is_refused_unless_the_holder_is_the_zone_handing_over_and_silent`).

**Each zone's queue is kept under its `ZoneId`**: the catalog's step 18
(`ZONES_KEEP_THEIR_OWN_QUEUES`) made `zones` (id, name, route), `zone_queue` and `zone_resume`
(keyed to `zones(id)` with `ON DELETE CASCADE`; `zone_queue.track` references `documents`, so
forgetting and handing over songs reach it as they reach `queue`). The first zone keeps `queue` and
`resume`, its `zones` row holding only a name, route or volume given;
`save_queue_of`, `save_place_of`, `resume_of` and `place_among_rows` read either through `QueueKept`.
`save` saves every zone; `MobilePlayer::new` resumes the first zone, then opens each kept zone at the
system's device with its kept queue paused and at its kept volume (`restore_zones`); the front end
binds a kept route to the device it now names (`select_zone_output`). A song gone or deleted leaves every zone's queue
(`drop_from_queue`), and `documents_moved` saves and resumes each zone holding a moved song. One
`resonate-watch` thread follows each zone's events, counting that zone's listening and telling the
watch (`watching::keep(player, zone)`; it ends when the zone's engine goes)
(`a_zone_added_keeps_a_queue_of_its_own_and_every_call_follows_the_zone_shown`,
`each_zones_queue_is_kept_under_its_id_and_goes_with_the_zone`,
`a_zone_renamed_keeps_its_new_name_and_a_blank_name_or_the_first_zones_removal_is_refused`,
`the_song_handed_to_another_zone_plays_there_and_leaves_the_zone_it_came_from`,
`a_route_serving_another_zone_announces_its_own_sink`).

**Each zone's volume is kept beside its device**, as the desktop's: the catalog's step 19
(`ZONES_KEEP_THEIR_VOLUME`) gave `zones` a `volume` (0 to 1, full by default). `save` keeps a zone's
volume (`keep_volume_of`) when the engine's has moved more than `VOLUME_KEPT_WITHIN` from the one
last kept, upserting the zone's row without touching its name or route (so the first zone, which has
no row until something is given, gets one only when its volume leaves full); `MobilePlayer::new`
starts the first zone's engine at it (`first_zones_volume`) and `zone_opened` opens each later zone
at its row's, so `set_volume`, which addresses the zone shown, is each zone's own and survives the
next start. `PlayerZone.volume` hands it to a front end; iOS reads its slider's first position from it and keeps no
volume preference of its own
(`each_zones_volume_is_kept_where_it_moved_and_a_zone_opens_at_the_volume_it_was_kept_at`,
`the_first_zone_reopens_at_the_volume_kept_beside_it_and_full_without_one`,
`a_catalog_of_before_zones_kept_their_volume_gains_a_full_one_for_each`).

**A handover to another device is sync's**: the phones already continue another device's queue
(`sync_continue_from`, into the zone shown) and offer the zone shown's queue and place
(`DocumentCatalog::resumption` reads the shown zone's `QueueKept` through the `shown` cell `Playing`
shares with it, the first zone's `queue` and `resume` while it is shown;
`a_handoff_offers_the_queue_and_place_of_the_zone_shown_and_not_the_first_zones`).

## TIDAL sign-in

`authorize_tidal` asks TIDAL for a device code and `finish_tidal_authorization` waits for it on the
caller's thread, as the seam's `SignsIn::authorized` does. Each `authorize_tidal` is an attempt
numbered before anything is asked (`Downloads::sign_ins`); `cancel_tidal_authorization` moves the
number on, so the wait's cancel (`Ok(None)`, heard within the seam's `LOOKED_AT_EVERY`) ends it, and
an attempt cancelled while its code was still being asked never stores the code a later attempt
would wait on (`a_sign_in_cancelled_and_asked_again_waits_for_the_new_code_alone`,
`a_cancel_ends_a_sign_in_waiting_for_its_code`).

## Downloads keep what they received

`downloads.rs` writes a resumable offer into `partials.rs`' folder (`download-partials/` under the
player's folder) before copying it to the descriptor, and a later job for the same want resumes it
with `Range`; `android.md`'s *A download that breaks off resumes* has the whole design. The sweep at
`MobilePlayer::new` is `Downloads::under`, and `Downloads::default()` (no folder) keeps nothing.
`Held::resumed` (core) is the lock on an existing partial that does not empty it.

## Reading document rows

`catalog::read` takes a document row by position, the first columns of `documents` in `V1`'s
order; a later step only appends columns, so `SELECT *`/`d.*` stays readable. A query adding its own
columns after `d.*` reads them by name (`position`, `listen_id`), never by index, since a later
step would move them.

## Checks

`cargo test -p resonate-mobile` runs on the host. The queue edit tests drive a real engine over
a backend with no device (a paused queue never binds a stream; `Harness::sounding` lists one sink
whose stream nothing pulls, for a queue that plays) and WAVE documents through the
`content` provider; that `Harness` (`harness.rs`, test-only) is shared by the organise, inspector,
analysis, snapshot and listening tests. A listen counted for a song the catalog no longer holds
stores nothing and settles nothing: `store_counting` keeps no visit unless the insert changed a row
(`a_listen_counted_for_a_song_the_catalog_lost_does_not_settle_over_another_listen`). The enrichment tests drive the pass over a fake `Reference` through `Asking`
(the reference and the client whose switch it reads): one cover per album, a miss left alone until
its wait is over, a prompt stop, the mark kept on closing, the album key and the artwork fallback.
Its documents tests cover both
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
