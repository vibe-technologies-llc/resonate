# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Tagging
- [M] On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name

## iOS
- [M] Playing in the background polls `snapshot` twice a second (ten times near a song's end) only
  to catch a song change the engine knows; `count_listening` rides on that poll, so an engine event
  needs the counting moved first

## Performance and scale
- [M] Every open of a Matroska file still reads every block header of it (up to
  `MAX_ELEMENTS_COUNTED`), and of a VBR MP3 naming no length every frame header (up to
  `MOST_FRAMES_WALKED`), scan and playback alike; a count persisted beside the catalog's duration
  would spare playback the walk
- [M] Queue edits still walk the queue on the engine thread, though none allocates a row: a
  publish copies a pointer a row into a fresh `Arc<[Arc<QueueItem>]>` and rebuilds `loaded_at` and
  `listed`; a removal, and a heard play-next row leaving, rehash every location for `Queued::stamp`
  (order-dependent SipHash over the load order, so only an append extends it); a heard play-next row
  shifts every index above it in `order`, `unshuffled` and `next`. Edits costing what they move need
  rows keyed stably (a slab, not positions) and a persistent vector for the published play order
- [M] The convolver works complex FFTs on real input (twice the memory and multiply-adds), stores a
  mono response's spectrum once a channel, `%`s per partition, and `loudest_gain` allocates
  `next_pow2(frames × 4)` complex values; nothing bounds a long response at a high rate in bytes
- [M] Search over the unheld discography `instr`s a concatenation per row and `LIKE '%…%'`s
  `release_tracks`, `artist_releases` and `artists.key`, growing with every enriched artist
- [M] The organise planner keeps every library path and folder listing for the whole run
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
