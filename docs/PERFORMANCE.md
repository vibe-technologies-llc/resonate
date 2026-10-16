# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M] `Player::new` waits on the first sink survey (`SINK_TIMEOUT`, 2 s) before answering: the binary's
  `confirm_sink` and the window's sink list read it once at start, so answering early needs a
  survey-settled signal they wait on

## Tagging
- [M] On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name

## Window
- [M] *Play*, *Shuffle*, *Next* or *Last* on a playlist from the index reads it whole on the
  window's thread (`entries_of` → `Library::playlist_entries`), a smart one running its query over
  the catalog; the tracks heading already does this on the background executor
- [M] Every visible track row copies the whole selected run each frame (`Lifted::tracks` deep-copies
  into an `Arc` and formats): after Ctrl-A on a 2 000-track page scrolling stutters; the queue
  shares an `Arc<[Cut]>`
- [M] Each reload empties the queue's name cache and the redraw then reads SQLite for every uncached
  row on the window's thread; Statistics' *play from here* reads a track a query

## Command line and bus
- [M] MCP opens a fresh D-Bus connection for every player call, a subscribed now-playing resource
  twice a second on the one answering thread
- [M] MCP lists are unpaginated (`cursor` ignored), `list_playlists` and the resource list are
  unbounded, rebuilt every 500 ms once listed

## Performance and scale
- [M] Every track open opens its source twice, once more for the inspector's box layout
  (`Unwrapped::inspected` → `probe_boxes`; `probe_stream` likewise), on the path before the first
  sample: a second request a skip for a network provider
- [M] Every open of a Matroska file walks every block of it (`count_clusters` caps per cluster and
  in clusters, not in total), and of a VBR MP3 naming no length every frame header (`mpa::walked`),
  scan and playback alike
- [M] Queue edits are O(n) on the engine thread: `one_id_each` hashes every id an insert,
  `rows_changed` rehashes every location, a consumed play-next row is three `retain` passes, and
  `publish_queue` clones every row whenever the revision moves
- [M] A non-local source's read allocates and zeroes up to 1 MiB, then copies it twice
  (`serve_the_reads`); vault playback goes through it
- [M] The convolver works complex FFTs on real input (twice the memory and multiply-adds), stores a
  mono response's spectrum once a channel, `%`s per partition, and `loudest_gain` allocates
  `next_pow2(frames × 4)` complex values; nothing bounds a long response at a high rate in bytes
- [M] Search over the unheld discography `instr`s a concatenation per row and `LIKE '%…%'`s
  `release_tracks`, `artist_releases` and `artists.key`, growing with every enriched artist
- [M] A playlist import missing a file rescans every catalog path and allocates a component vector
  each (`reconnected_by_trailing_components`); `Library::playlists` runs a whole-catalog count per
  smart playlist
- [M] Retag's preview opens each file twice (`tags.read`, then `tags.rated`); the organise planner
  keeps every library path and folder listing for the whole run
- [S] The analysis study's `TruePeakMeter` oversamples 8× at every rate: it is given no rate, where
  the guard now folds 4×, 2× or not at all by rate
- [S] A cover decodes under `image`'s default 512 MiB allocation limit, any number at once; a
  vault's cover dedup reads the whole standing file to size it
- [S] A non-seekable source spools up to 8 GiB into `/var/tmp` with no free-space check
- [M] A WAVE import writes and `sync_all`s the whole uncompressed PCM before compressing and
  deleting it, a dedup hit included
- [S] `voices_in_play` walks every earlier line of a one-voice sheet each frame
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
