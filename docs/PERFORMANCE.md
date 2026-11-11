# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## The window
- [S · 0.8.1] The stream digest is published again every second of decoded audio: `publish_digest`
  makes a new `Arc<StreamDigest>` whenever the bitrate profile gains a point (`engine.rs`), each
  `ProfileBuilder::finish` clones the series twice and sorts one copy (`stream.rs`, up to
  `MAX_WINDOWS` points), and the new pointer is `Moved::More` to the window (`app.rs`), which
  notifies `RootView` and redraws the header, sidebar, pane and transport (`zones.rs`, `part.rs`)
  once a second whatever is shown, against `ui.md`'s clock-only redraw; a ten-hour book sorts 36 000
  points a second on the engine thread
- [L · 0.8.1] A browse list's next page clones every row held to append 2 000 more (`models.rs`
  `grew`, on the UI thread) and is read with `LIMIT … OFFSET` (`db.rs`) though `alternative_of` and
  `hidden` are in no ordering index, so wheeling to the end of a 500 000-track list clones some sixty
  million rows and walks the table quadratically; rows held in chunks and paging by the order's
  last key (every order ends on the id) want both crates
- [S · 0.8.1] The queue pane's length reads every queued row again whenever the library model's
  revision moves (`queue.rs` `QueueLength`), which every read landing moves (a page, a search, a
  favourite, every 2 s of a scan); it wants the queue's revision and the catalog's stamps, as
  `names_in_the_queue` keys on, or one `sum` query
- [S · 0.8.1] Every playlists read asks each playlist's pictures one at a time (`models.rs`
  `pictures_of`, `playlist.rs` `playlist_pictures`), a saved query's through a `GROUP BY` over every
  track it matches (`db.rs` `pictured_by`) and a capped one's deduped with `Vec::contains`; nothing
  keeps them, though `measured_saved` beside them is kept under `rows_stamp`
- [S · 0.8.1] Every listing read measures the whole catalog (`models.rs` `browsed`, `db.rs`
  `measured`): a count and two sums over every `tracks` row with no index covering the filter, on
  every counted play and every 2 s of a scan; `KeptMeasurements` under `rows_stamp`, or a covering
  index
- [S · 0.8.1] Every `Everything` read aggregates the whole listening history about seven times,
  `quarters_heard` twice over the same window (`statistics.rs` `listening_by_day_in` and
  `habits_in`), whether or not Statistics is shown (`models.rs` `standing`)
- [S · 0.8.1] The now-playing bar deep-clones the playing `Track` and its `Album` on every transport
  render (`transport.rs`), and after each read lands an album not on the first page is read from
  SQLite on the UI thread (`models.rs` `album_anywhere`)

## Startup and idle
- [S · 0.8.1] The renderers' stream server polls its waiting request heads every 10 ms and its
  listener every 100 ms while idle (`resonate-upnp` `serve.rs`); a loop woken by readiness (`poll`)
  would not wake at all
- [S · 0.8.1] The window waits up to `SINK_TIMEOUT` (2 s) on the first sink survey before it opens
  where a sink is chosen (`main.rs` `launch`, `confirm_sink`, `sinks_once_surveyed`), only to log a
  warning, though `audio.md` says the window opens without waiting on the graph
- [S · 0.8.1] The window looks at the library's roots four times a second for its whole life
  (`ROOTS_LOOKED_AT_EVERY`, `models.rs` `watch_the_roots`): a catalog read and a `stat` of every root
  each tick, paused or minimised, reaching the server where a root is on NFS
- [S · 0.8.1] Every `Library::open` reads and folds every artist (`store::reconcile_artists`) to
  find nothing to do, where its sibling passes wait on a marker table (`refold_the_index`,
  `compose_the_album_keys`, `word_the_titles`); the marker wants making by the steps that change
  the fold
- [M · 0.8.1] (investigate) While a root is uncovered the recursive watch is built again over every
  root each five minutes, beside the old one (`models.rs` `following`, `watch.rs`); that is likely
  where `max_user_watches` ran out, so it may fail every time and walk the tree each try

## Android
- [M · 0.8.1] While enrichment writes, a walk of the library's pages rebuilds the whole search index
  for nearly every 500-row page: every `documents` write moves `Catalog::revision`, which throws the
  index away (`search.rs`), enrichment writes about once a second, `PagedSteadily` (and iOS's
  `Library.swift`) walk again up to three times on a moved revision, and `ArtworkEnrichment`
  refreshes every 2 s; an index pinned for a walk, or a revision only the index's own columns move
- [M · 0.8.1] Android reads the whole queue again every 3 s while the catalog moves (`QueueReads`),
  under `access` (`LibraryRepository.kt`), so pause, seek and skip wait behind it, at a catalog
  lookup a row (`lib.rs` `queue`), and rebuilds every `MediaItemData` on the main thread and the
  session's timeline (`RustMediaPlayer.kt`); a *Shuffle all* of 30 000 songs pays all of it each
  time, where iOS reads a window
- [S · 0.8.1] Android polls the native player every 250 ms whenever the engine is held, paused in
  the background included (`LibraryRepository.kt`, `LibraryFeatures.tick`), some twenty native calls
  a second; only iOS registers `watch_player`, and it slows its poll to 1 s at rest and 2 s in the
  background (`Polling.swift`)
- [S · 0.8.1] (investigate) A song played in place over Wi-Fi takes no `WifiLock` on Android, so
  power saving with the screen off may starve a hi-res FLAC stream (`TESTING.md` names this for a
  renderer alone)

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## iOS
- [M] The equaliser page re-evaluates every section at each paced drag sample: `EqualiserView.body`
  reads the draft and hands every section closures made in that body, so a handle or slider drag
  rebuilds the band chips, the band card, the preamp and the presets twenty times a second; the
  draft wants an observable only the graph and the band card read, and the sections an action
  object that compares equal across bodies, as Now Playing's `PlayerAsks` is
- [M] Songs and the add-songs sheet hand SwiftUI the whole filtered library as one `ForEach`, whose
  ids are gathered and diffed on the main thread at every publish (several milliseconds past ten
  thousand songs); the queue is a window (`QueueParts`), these are not
- [S] An `.inactive` blip (Control Centre, a banner, the App Switcher) flips `Features.foreground`,
  which nudges the poll, restarts the enrichment watch, the wants loop and the linked-playlist loop
  (its ten-minute cadence starting over), toggles the visualizer tap and queues a save; they should
  act on `.background` and back alone, as `cameForward` does
- [S] The remote-device page rebuilds its list twice a second while the driven device plays:
  `sync_watch` answers the position played on to the moment asked, so the read never equals what is
  shown and every zone and visible row is evaluated again; the position wants an observable of its
  own, as `PlayClock` is, read by the place line and the seek slider. The party page does the same
  with the round trip it shows
- [S] `Features.syncStatus` is read whole by views that need one field of it (Home's devices, the
  pairing sheet's code, a playlist's options, the status banner), so each is evaluated again at
  every poll while a transfer's byte count moves, and the sync page sorts its devices and keys its
  transfers again each time
- [S] Every download job row holds three or four glass buttons, each a glass surface of its own in
  a scrolling list, and *Download all missing* makes hundreds of rows
- [S] The moving field behind Now Playing keeps drawing at its full rate under a sheet raised inside
  the cover (the sleep timer, zones, the playlist picker), beside the sheet's own presentation
- [S] `LyricLayerView.wake` draws a frame at every clock reading though its display link is already
  ticking, so a playing word-synced sheet is rendered two or three times a poll

## Later: Analysis
- [S] `TruePeakMeter` multiplies out the phase at whole samples (weights of one tap of 1 and the rest 0, so the
  answer is the sample itself) but skipping it saves nothing on a vector unit: the phases are lanes of one
  vector, so the lane is free beside the fractions and dropping it only narrows the array (measured, 60 s of
  stereo at 44.1 kHz, fifteen passes: 29.9 G instructions and 18.2 G cycles as it is; the same width with the
  lane left to the fractions and the centre read apart 32.4 G and 18.6 G; arrays of exactly the three
  fractions 60.3 G and 21.8 G, since LLVM does not vectorise three lanes; every read bit for bit the
  same). Only a stereo pair's six fractions packed into one 8-lane vector would save, on AVX-512 alone (the
  release baseline is x86-64-v3, four lanes), and a mono or odd channel has nothing to pack with
- [M] `workers_for` stayed at half the cores though each study now spreads over the crew: four
  workers keep the same throughput as eight at three quarters of the memory (442 MB against 582 MB
  on sixteen songs), but a worker also waits on recognition, which was not measured
- [M] `Bank`'s rows are read one chunk at a time on its own stage, and its forward pass cannot be parted, so it and
  `Metered`, 1 to 1.5 s each over 5 minutes at 44.1 kHz, are what a pass's wall time now waits for,
  96 kHz most of all (4.5 s for 5 minutes); `Metered`'s meter, which restarts its running maximum every tile,
  reads twice what one over the whole song does on a quiet master (158 to 200 ms against 74 to 96 ms for 60 s of
  stereo at 44.1 kHz) and a tenth more on a loud one (122 against 107 ms for 46 s)
- [M] `peak_through`, `print` and `excerpt` walk on one thread and the crew does not take them: it fans one
  decode out to independent stages, and each of these has one stateful consumer in series (decode, the room's
  convolver, the equaliser, the meter; decode, Chromaprint; decode, a mono mix), so a crew could only overlap
  the decode with it. That is a fifth of `peak_through` (177 to 205 ms for 47 s of 16/44.1 stereo FLAC, 297 ms
  for 40 s at 96 kHz, the decode alone 40 to 50 ms and 70 ms) and most of `print` (51 to 61 ms), and
  `loudest_through` already runs half the cores' worth of cuts at once, so the cores that overlap would buy
  are the ones the cuts use. A gain wants the chain parted, as a pipeline by stage or by channel, and the
  convolver's two-by-two response ties the channels together

## Later: DSP
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.041 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. A halving stage at 4:1 and beyond would close part of it; pruning the transform to the bins kept was
  worked through and saves one stage of fourteen; an own FFT was measured at parity
- [S] The resampler's Low latency costs four times Balanced (0.097 % against 0.023 % at 44.1 to 48 kHz, and it
  rises to 0.120 % on a build without `target-cpu`, where `rustfft` does not): it stays polyphase for its
  latency of 1.07 ms, and the kernel's symmetry is unused
