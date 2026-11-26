# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Startup and idle

## Android

## Performance and scale
- [M · 0.18.1] Count a playlist's songs without loading them for the media server. **Medium,
  builder.** `playlists()` calls `listed()` for each playlist's child count, loading every entry
  with its whole track row (`crates/resonate/src/offering.rs:204-231`); the cache of 32 lists is
  cleared wholesale (`:321-359`), so a 500-row page never hits it, and `played_from_matching` is not
  cached at all (`:256-286`). Count in SQL, keep counts per catalog revision and page a search in
  SQL. Done when a test of 500 playlists of 1 000 rows browses its root without reading an entry.
- [M · 0.18.1] Keep the media server from taking every catalog reader. **Medium, architect.** The
  shelves share the library's eight-reader pool with the window
  (`crates/resonate-library/src/db/mod.rs:79`, `:471-497`), whose checkout waits with no timeout,
  while neighbours may hold 16 answers and a root search makes up to four ranked FTS reads
  (`crates/resonate/src/offering.rs:554-604`, `:183-202`), and a one-letter prefix is accepted
  (`:804`). Cap the media server's concurrent reads below the pool, bound the checkout and ask for
  two letters at least. Done when a test of 16 concurrent root searches leaves a window read
  answered within its deadline.
- [S · 0.18.1] Unpack an amended MusicBrainz document at its own size. **Low, builder.** An amended
  read unpacks into a 16 MiB `MOST_UNPACKED_BLOCK` buffer
  (`crates/resonate-musicbrainz/src/mirror/mod.rs:695`, `:767-770`), up to 64 times a search. Store
  the unpacked length or read it from the frame. Done when an amended read allocates its document's
  size.
- [S · 0.18.1] Let the renderer subscriptions' forwarder go with its sink. **Low, builder.**
  `Beside::subscribe_sinks`' forwarder thread and the network's listener live until the next sink
  change (`crates/resonate-upnp/src/beside.rs:62-129`, `network.rs:344-349`, `:600-607`), and Cast
  keeps a link, thread, socket and pings per address for good
  (`crates/resonate-cast/src/casting.rs:811-862`). End each with its subscription or receiver. Done
  when tests count no thread left after unsubscribing.
- [S · 0.18.1] Tell MCP's now-playing subscribers only when the song or state changes. **Low,
  builder.** The compared reading includes `position_seconds`
  (`crates/resonate-mcp/src/transport.rs:152`), so every 500 ms tick notifies. Leave the position
  out of the comparison. Done when a playing song notifies once in a test over five seconds.
- [S · 0.18.1] Read each thing once at start and in studies. **Low, builder.** `engine_config` is
  evaluated twice, parsing the impulse twice (`crates/resonate/src/window.rs:82`, `:109`), and
  `studies` reads the catalog twice (`studies.rs:14`, `:42-46`). Reuse the first reading. Done when
  each is read once.
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Device sync

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
- [S] The band splitter's buffers sit outside the queue's 8 MiB bound: about 224 bytes a frame for
  1.7 s of each channel (`crates/resonate-analysis/src/split.rs:424-496`), about 440 MB for a 5.1
  file at 192 kHz

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
