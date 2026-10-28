# Correctness audit

A reading of the playback, catalog and phone-sync paths for defects. Security, performance, tests, debt, dependencies, tooling, documentation and product direction were left alone. The two items already in [BUGS.md](BUGS.md) stay there: both wait on an upstream decoder.

Hotspots actually read: zone handover and the window, bus and command line that start it; the engine's stream reshape and the PipeWire callback that fills a quantum; phone sync landing on Android and iOS; MP4 duration arithmetic; a pass over organise, shared answers and listen credits.

## Findings

Ordered by leverage: what a listener loses, divided by the size of the fix, discounted where the window is narrow or the repair can race an edit.

### [BUG-01] Restore the target zone's queue when a handover is not heard

- **Evidence**: `crates/resonate-engine/src/zones.rs:266` — `hand_over` sends `SetRepeat`, then `Resume` of the outgoing zone's queue, then `Play` when the outgoing zone means to sound, and only afterwards spawns the thread that waits to hear the incoming zone.
- **Evidence**: `crates/resonate-engine/src/engine.rs:1491` — `Command::Resume` calls `stop` and `queue.restore` before `start`. `stop` (`engine.rs:2020`) clears the transport and leaves the queue where `restore` put it.
- **Evidence**: `crates/resonate-engine/src/zones.rs:305` — when `Play` is not answered, or the incoming zone never publishes `Playing` with an output inside the wait, `let_the_outgoing_go` sends `Stop` to the incoming zone and returns. The queue `Resume` installed stays. The outgoing zone is left playing.
- **Evidence**: `crates/resonate-engine/src/zones.rs:277` — a failed `spawn` of `resonate-handover` returns `HandoverUnheard` after those sends, so nothing stops the incoming zone. A `Play` request that returns an error (`zones.rs:269`) does the same: the `?` leaves before the thread exists.
- **Evidence**: `crates/resonate-ui/src/views/zones.rs:216` — the toast names the target zone and says the music stayed where it was. `move_playback_to` (`zones.rs:380`) shows the target at once and, on the error, shows the source again. `zone_moved` (`zones.rs:257`) calls `keep_the_queue`, and with resume on (`root.rs:1371`, `models.rs:4943`) that writes `keep_resumption` for the queue now published.
- **Evidence**: `crates/resonate/src/mpris.rs:300` — the bus method returns `HandedOver::Moving` and drops the `Handover`, so the waiter is detached and a later failure is only the warning at `zones.rs:282`. `queue --move-to` (`main.rs:2157`) prints that playback is moving as soon as the bus call returns.
- **Impact**: Moving playback onto a zone that does not reach `Playing` with an output within the wait (no device, a device that will not open) replaces that zone's queue and place with the source zone's, then stops it. The source keeps playing its own queue. With resume on, the replaced queue is what the next launch opens. An edit of the target during the wait edits the queue `Resume` already installed. A `Play` error or a failed spawn leaves both zones able to play that same queue, and the target's previous queue is already gone.
- **Effort**: M
- **Risk**: MED. The snapshot of the target has to be restored only for the queue `Resume` installed. An edit the listener makes after the failed handover's `Stop` has its own revision and must be left in place.
- **Confidence**: HIGH
- **Fix sketch**: Read the incoming zone's queue, row and position before any send. On every failure after those sends (`Play` refused, spawn refused, play unheard), `Stop` the incoming zone and `Resume` it from that snapshot, and leave the outgoing zone playing. Say the target did not start, and that its queue was put back. Add a transport test whose target already holds its own queue and position, force the incoming zone to stay short of `Playing` with an output, and hold both queues unchanged.

### [BUG-02] Settle a waiting sync song onto the catalog row that already holds its recording

- **Evidence**: `crates/resonate-mobile/src/sync.rs:1360` — `holds` treats a recording as held when `candidates` finds a visible row whose file is still there, or a withdrawn row. `candidates` (`sync.rs:853`) asks `Index::resolved` (`sync.rs:1569`): the recording id, or folded artist, title words, album when named, and length within two seconds.
- **Evidence**: `crates/resonate-mobile/src/sync.rs:689` — `held_already`, which `sync_received` (`sync.rs:384`) uses to drop a waiting song, unions `songs_received` keys and, when the waiting file was probed, documents of the same bytes. It does not ask `resolved`.
- **Evidence**: `crates/resonate-mobile/src/collections.rs:251` — indexing a download writes `documents.recording` and does not insert `songs_received`. A different encode of the same recording misses the byte comparison.
- **Evidence**: `crates/resonate-mobile/src/sync.rs:400` — `sync_receive` and `sync_receive_standing` (`sync.rs:412`) copy or adopt the file and index it, then `settled`, with no second held check. Android's poll (`LibraryFeatures.kt:606`) and iOS's (`Features.swift:650`) land every song `syncReceived` still returns.
- **Impact**: A song already sitting in `received` (the destination folder was out of reach, a landing was backing off, or the app was away) is copied again once a download or a scan has indexed that recording under its own row. The library then has two playable documents of one recording, and plays and favourites split across them. A later fetch is already blocked: `holds` is what stops a device asking for the song again.
- **Effort**: S
- **Risk**: MED. The check has to be the one `holds` already trusts. A looser name match would settle a waiting song onto a different recording.
- **Confidence**: MED
- **Fix sketch**: Inside `held_already`, resolve the waiting recording the way `holds` does. At the start of `sync_receive` and `sync_receive_standing`, run that check again: when a visible row already holds the recording, `settled` onto that track and drop the staged file before a document is created.

### [BUG-03] Keep a reshaped stream's format and volume events until the engine is polling it again

- **Evidence**: `crates/resonate-engine/src/engine.rs:1946` — `retire` moves a quiet stream straight into `kept` and a sounding one into `retiring` until the fade ends. The run loop (`engine.rs:1113`) calls `finish_fading` before `poll_stream`. `poll_stream` (`engine.rs:3763`) reads events only from the output that is current, so a stream in `retiring` or `kept` is unread.
- **Evidence**: `crates/resonate-engine/src/streaming.rs:76` — `has_stood_up_to_now` drains the whole event queue with `try_iter` and keeps the stream when none of those events is `Failed` or `Refused`. `FormatChanged` and `VolumeChanged` are discarded there.
- **Evidence**: `crates/resonate-pipewire/src/client.rs:1748` — `param_changed` stores `taken_against` the spec the stream asked for and sends `FormatChanged`. `taken_against` (`format.rs:284`) is `Taken::Otherwise` whenever the settled spec differs from the asked spec. The callback (`process.rs:232`) fills that quantum with zeros and reports the quantum filled.
- **Evidence**: `crates/resonate-pipewire/src/client.rs:151` — `reshape` stores `taken_against` and calls `update_params` only when the requested spec differs from the spec asked last time. A same-spec reshape leaves a standing `Taken::Otherwise` where it is.
- **Evidence**: `crates/resonate-engine/src/engine.rs:852` — `take_what_the_stream_was_handed` arms the one-second reopen timer only when the parked snapshot's spec differs from the plan. A same-spec reshape therefore neither provokes a new settlement nor reopens.
- **Impact**: A format settlement that arrives during the fade, or while a paused rebind holds the stream in `kept`, is dropped. The following reshape that asks for the same rate, word and layout leaves the callback writing silence, and nothing later asks the graph to settle, so the zone stays silent until some other rebind changes the spec or reopens the stream. A volume change in that same window is dropped, and the published stream volume stays at the snapshot taken when the stream was parked. The window is the fade plus however long the stream sits in `kept`, and it takes a graph format change inside it followed by a same-spec reshape.
- **Effort**: M
- **Risk**: MED. Replaying a `FormatChanged` onto the engine can start a `downgrade` rebind. The test has to show a same-spec reshape converging on the spec asked for, rather than renegotiating in a loop.
- **Confidence**: MED
- **Fix sketch**: `has_stood_up_to_now` should count failures and leave every other event queued for `poll_stream`. On a same-spec reshape, when the settled format is still `Taken::Otherwise` against the spec in force, store `taken_against` of the asked spec and provoke a settlement (or arm `format_owed_since`) so the ring is filled again once the graph's format is the one asked for.

## Considered and rejected

- A second gapless boundary applying the entering row's level to the leaving tail. `pass_the_boundary` calls `hear_at`, which bumps the ring's turn before another boundary can be announced, so `wanted` keeps reading the level heard now while the next boundary waits. The volume path during a waiting boundary is `hear_across`.
- MP4 `decoded_frames` multiplying sample count by the longest `stts` delta. The boxes fixtures pin that product's remainder as encoder padding. A closed edit window plays its own `playable` length, and duration is taken from that window. The inspector and `resonate info` can show the phantom tail; playback does not trim it.
- `Zones::send` of a command meant for every zone folding with `Result::and`. Every zone still receives the command; the first error is the one returned.
- `organise::files_moved` deleting the destination row before retargeting the source. The planner orders a chain so a destination vacates first, and the scan's move pairing calls it only after whole files are paired.
- `take_band_peaks` marking the file taken when any same-file copy exists. The study is listed at the same stamp ahead of the peaks, and a copy whose peaks are still missing is measured on its own.
- A deleted track's listens being credited again. `forget_files` copies them into `unheld_listens` in the same transaction that deletes the track, and `credit_the_unheld` inserts under the old id when it is free. A remote listen uses `INSERT OR IGNORE` and leaves `plays` unchanged when the insert changes nothing.
