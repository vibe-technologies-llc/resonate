# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Tagging
- [M] A tag write that moves the audio still copies the whole file where the filesystem can neither
  clone nor insert a range (FAT, exFAT, NTFS, network shares) and a head tag outgrows its room; so
  does an Ogg comment that needs another page (the audio's sequence numbers and CRCs change) and an
  Ogg head another muxer laid out unlike lofty, once each (the copy leaves padding). Each writes
  the copy back over the inode when the file has a second name (only the pages that differ, which
  for moved audio is all of them)

## Performance and scale
- [M] Queue edits still walk the queue on the engine thread, though none allocates a row: a
  publish copies a pointer a row into a fresh `Arc<[Arc<QueueItem>]>` and rebuilds `loaded_at` and
  `listed`; a removal, and a heard play-next row leaving, rehash every location for `Queued::stamp`
  (order-dependent SipHash over the load order, so only an append extends it); a heard play-next row
  shifts every index above it in `order`, `unshuffled` and `next`. Edits costing what they move need
  rows keyed stably (a slab, not positions) and a persistent vector for the published play order
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
