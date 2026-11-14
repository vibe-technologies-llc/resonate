# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## The window
- [S · 0.11.1] Opening the party pane once redraws the whole window twice a second for the rest of
  the run: `party.rs`'s `keep_redrawing` starts a `PANE_REDRAWN_EVERY` loop that nothing stops
  (`redraws` is never set back to `None`), the root observes the party and each `Part` the root, so
  every pane, idle or not, is painted at 2 Hz; `ui.md` says the timer redraws the pane while it is
  shown
- [M · 0.12.0] Every `Load` and `Insert` asks the catalog for chosen versions on the window's thread:
  `RootView::send` calls `with_the_versions_chosen`, which reads `chosen_among` synchronously (a
  fresh statement per 500 ids), about two hundred queries to play a 100 000-track listing even
  where no version was ever chosen, and a wait on the reader pool while a scan holds it; an
  in-memory count of `versions_chosen` rows would skip it, else it belongs on the background
  executor
- [S · 0.12.0] The lyrics timing nudge writes the catalog on the window's thread:
  `nudge_the_lyrics` calls `Library::hold_lyrics_ahead`, an `IMMEDIATE` write under a 10 s busy
  timeout, at each press of the steppers, and `lyrics_ahead` is read inside the pane's render; every
  other edit goes through `edited_then`
- [S · 0.12.0] Every automatic and inbox poll reads and decodes the whole `wants` table on the
  window's thread (`keeping.rs`'s `worth_asking_on_its_own`, `Library::is_a_want_due`), thousands of
  rows for a listener following artists or profiles; `next_want_due` in `supply.rs` could answer it
  from what is held, or the poll could weigh it in its task
- [M · 0.12.0] Opening a track's menu runs catalog queries on the window's thread:
  `browser.rs` asks `alternatives_of` and `has_a_version_chosen`, the playlist row menu
  `has_a_version_chosen`, and *Find another version*'s rating route a full-text `tracks_named`, each
  waiting on the reader pool while a scan or lookup holds it
- [S · 0.12.0] A folder picked or typed as the music folder or the vault is canonicalised off the
  window's thread and then probed again on it: `unusable_as_the_music_folder` and
  `unusable_as_the_vault` call `is_dir`, so a stalled network mount or a sleeping disc blocks the
  window where `ui.md` says the check is made off it
- [S · 0.12.0] Driving another device's zones redraws the whole window twice a second until *Drive* is
  pressed again: `driving_redraws` in `settings/sync.rs` runs while `remotely.driving` is set, which
  leaving the page or the pane does not clear (`sync.md` says *while driving*; whether the watch
  should outlive the page wants deciding)
- [S · 0.12.0] The identical-songs page regroups every copy on each frame it draws
  (`identical.rs`'s `grouped`: a `BTreeMap` over `copies.identical` and a sort of each group, to draw
  `GROUPS_SHOWN`), the caret's blink included; grouping once as `Copies` lands would do

## Catalog
- [S · 0.12.0] Deleting or organising many files compiles statements per path and scans two tables
  per path: `deleted::forget_files` `prepare`s (not cached) in its loop, as do
  `keep_listens_as_unheld` and the `wants` delete; `files_of` queries a track at a time; `wants` has
  no index on `offered` and `retagged_sheets` none on `file`, though `forget_files` and
  `sheeted::notes_follow_the_file` filter on them once a path (the indexes are a migration step)

## Startup and idle

## Android
- [S · 0.12.0] Some native calls still run on Android's main thread where iOS runs them aside:
  `ListeningTestScreen`'s `hearInListeningTest` (iOS's `Work.aside`), and `serveFrom` through
  `served.bind(native)` in `ServerReads.kt`, `Sheets.kt`, `VersionsSheet.kt` and `ServerActions.kt`,
  each taking a Rust lock (the zones, the served `RwLock`) that readers on other threads hold; how
  long they wait is unmeasured

## Performance and scale
- [S · 0.12.0] The true-peak measurement opens and decodes a network song a second time:
  `Measuring::wanted` weighs neither the source nor whether it seeks, and every lossy codec without a
  declared depth is `F32`, so with `true_peak` on by default each never-studied MP3, AAC, Vorbis or
  Opus song from Subsonic, TIDAL or YouTube Music is downloaded again whole, and a live stream is
  decoded on a second connection for as long as it plays; measuring local sources alone would do
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## iOS

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
  on sixteen songs), but a worker also waits on recognition, which was not measured (retried on a machine
  at a load of 35, where a throughput comparison between four and eight workers means nothing)
- [M] `Bank`'s forward pass, and `Metered` with it, are what a pass's wall time waits for. The reading of the
  settled rows is parted across the crew now (`analysis.md`), which took a bank's stage from about 330 to
  220 ms of 60 s of stereo at 44.1 kHz and `band_figures` from 0.31 to 0.24 s; what stands beside it is the
  forward pass (143 ms of one thread, which cannot be parted in time), `Metered` (160 ms) and the two
  `Coinciding` stages (100 ms each), and a lossless study waits for its grids instead (the weighing stages, 430
  to 830 ms, are its longest lanes). Left: the forward pass across edge groups, the 28 edges being independent
  lanes. Not built: the planes are written by every edge at once, so lanes parted into tasks either write
  buffers of their own that are gathered into the planes (the pass's whole output copied once more, 10 MB for
  each second of audio and bank) or the planes are kept group-major through the settling and the reading too,
  which is a rewrite of the layout, and what it could gain is bounded by `Metered`'s 160 ms, the next lane.
  `Metered`'s tile-at-a-time meter reads more than one
  over the whole song does because a tile's exact maximum is lower than the song's, so what the song's
  skips a tile's must read (a bound of every window by its largest taps cut it by a fifth on a quiet master
  and left a loud one as it was: `analysis.md`, *Loudness*); a read that costs less per window, not a
  better skip, is what is left of it, and parting its tiles across the crew would want each tile's meter
  primed by the samples before it without moving a read
- [S] `excerpt` walks on one thread: its decode is the 14 to 20 ms of it (20 s of 16/44.1 stereo FLAC) and
  the mono mix is two multiply-adds a frame, so handing slabs to another thread, as `peak_through` and `print`
  do now (`analysis.md`), could hide only the mix; not built

## Later: DSP
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.041 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. Pruning the transform to the bins kept was worked through and saves one stage of fourteen; an own
  FFT was measured at parity; a half-band halving stage cannot reach 2:1, and at 4:1 it was built and cost
  more than it saved (Very high 192 to 48 kHz 24 % dearer, 11 % with every load aligned)
