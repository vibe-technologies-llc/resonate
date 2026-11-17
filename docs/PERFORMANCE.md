# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Startup and idle
- [S · 0.12.1] Four channels are polled every `QUIT_POLL` (100 ms) while the window is idle:
  `follow_the_desktop_when_told`, `follow_when_told` and `raise_when_asked` wake the foreground ten
  times a second for a `try_recv`, and `close_when_asked` the background executor (`app.rs`), some
  forty wake-ups a second beside the resting 64 ms poll; one thread blocking on the receivers, or an
  async channel awaited, wakes only when something comes

## Android

## Performance and scale
- [S · 0.12.1] Every sync look scans `track_studies` twice and `lyrics_kept` whole: `studies_after`,
  `band_peaks_after` and `lyrics_after` (`answers.rs`) filter and order by `studied` and `taken`, which
  no index holds (`track_studies` is indexed on `verdict` and `agreement` alone, `lyrics_kept` on its
  key), and `band_peaks` and `taken` sit after the large `print` and `text`; a look follows every
  revision change by 300 ms, so on a 500 000-song catalog each play costs a read and sort of both
  tables. A `MIGRATIONS` step indexing the two, with an `EXPLAIN QUERY PLAN` test
- [S · 0.12.1] `credit_the_unheld` walks every track after a scan that brought one file:
  `COUNT_THE_CREDITED_AND_LET_THEM_GO` (`history.rs`) is an `UPDATE tracks … WHERE EXISTS` over
  `unheld_listens`, which SQLite does not turn into a join for an UPDATE, run whenever `arrived > 0`
  inside the scan's write lock; and `CREDIT_AN_UNHELD` and `NOTE_AN_UNHELD_CREDITED` are prepared
  once a listen, not through `store::cached` as `library.md` asks. Restrict to the ids the join
  names
- [S · 0.12.1] The ledger ages listens and the kept answers sweep their freshness by scanning whole
  tables: `DELETE FROM listens WHERE at < ?1 AND owed = 0` runs on every look with no index on `at`
  (`ledger.rs`), and `DELETE FROM answers WHERE at < ?1` at every open of `kept.sqlite` likewise
  (`kept.rs`); an index on each as a migration step, or aging once a day
- [M · 0.12.1] The search box's discography lookup scans every `discography_songs` row with `instr`:
  `songs_kept_for` (`db/following.rs`) matches each word by `instr(s.folded, ?)`, run for every search
  read while Online is on, where `library.md` says a piece of a word is looked up in a trigram index and
  `albums_kept_for` goes through `Infix`; a trigram table kept by triggers (the table is `WITHOUT
  ROWID`) and `Infix::holding`
- [M · 0.12.1] Opening a playlist's menu reads the sync ledger on the window's thread: `synced_entries`
  (`views/playlists.rs`) asks `missing_from` and `keeps_songs_of`, which take the ledger's lock and
  decode every synced playlist's entries to find one (`ledger.rs`), and wait on the lock while the
  worker merges, against `ui.md`'s *a menu opens at once and never reads the catalog on the window's
  thread*; read it on the background executor as `open_once_the_copies_are_read` does, and ask the
  ledger by `local_id`
- [S · 0.12.1] The sharing sheet reads the catalog on every render: `together.seen(playlist)`
  (`views/sharing.rs`) does a `playlist_stamp` read and clones the whole `Vec<SharedShown>` each frame
  the root draws, the root notified every second while it is open, and the playlists pane clones the
  same snapshot each render for `(id, may_edit)` pairs; keep what the 1 s tick read on `Social`
- [M · 0.12.1] A pane's own pulse redraws all four regions: the root observes each pane entity with
  `cx.notify()` and every `Part` observes the root, so the party pane's and the driven zones' 500 ms
  pulses and the mirror's 1 s one rebuild the header, the sidebar and the bar too, against `ui.md`'s
  *a frame asked inside a region notifies it alone*; a `Parts::the_pane_moved` beside
  `the_clock_moved`
- [M · 0.12.1] A paired phone offers every answer it does not hold to its catalog again on each
  revision: `settle_answers` (`ledger.rs`) reads every `here = 0` row into one `Vec`, decoding each
  blob, and calls `take_answer` for each, skipped only where `answers_tried_at` (held in memory, so
  lost on every start) equals the revision; a phone paired with a large desktop holds most of its
  answers unheld, so every play re-resolves them. Offer only what became held since, in pages
- [M · 0.12.1] The mirror's song, album and artist searches rank by bm25 over every row holding a common
  word: `find_songs`, `find_albums` and `find_artist` (`mirror/search.rs`) call `matched` with every
  word, without the `bare_title_token` narrowing `asked_queries` tries first, though `musicbrainz.md`
  measured *love* at 2.5 s that way; the local score re-ranks the candidates anyway, so an unranked
  wider set or the narrowing first
- [L · 0.12.1] Sync stops compacting once the state passes 16 MiB and rebuilds the snapshot on every
  batch thereafter: `compact()` (`service.rs`) seals the whole `snapshot()` (every answer decoded,
  `answer_notes("1")`) and returns `None` past `SNAPSHOT_FRAME_SHARE`, while `since_snapshot` never
  resets, so it runs again after every `Records` frame at `Compression::best()`, the server's log
  grows past `sync.md`'s compaction for good and a new device replays all of it; answers are never
  pruned. Back off once refused, then a snapshot of several parts or answers kept out of it
- [S · 0.12.1] Investigate: an orphan sweep scans `lyrics_kept`, `lyrics_refused` and `lyrics_ahead`
  whole (`store.rs`'s `ORPHANS`, a `NOT EXISTS` probe a row) beside the credit rebuild, inside the
  write lock; measure on a large synthetic catalog, then delete with the track by trigger if it
  matters
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
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.039 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. Pruning the transform to the bins kept was worked through and saves one stage of fourteen; splitting
  it into its two polyphase branches saves half a stage of fifteen, the join being the radix-2 stage it
  removes; an own FFT was measured at parity; a half-band halving stage cannot reach 2:1, and at 4:1 it was
  built and cost more than it saved (Very high 192 to 48 kHz 24 % dearer, 11 % with every load aligned)
- [S] High and Balanced cost 4 to 5 % more at 2:1 and steeper since their kernels grew (High 9 % at 176.4 to
  48 kHz, `docs/RESAMPLING.md`'s *Deeper windows*): the overlap is a larger share of a block whose steep hop
  was held for latency. A hop swept again for each, or a hop of its own for each phase, since a shaped
  phase's latency moved both ways with the kernels (High minimum 57.89 ms for 34.60 ms at 192 to 48 kHz)
