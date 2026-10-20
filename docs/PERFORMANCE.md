# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Tagging
- [M] On a filesystem that cannot clone a file (ext4), a tag write that moves the audio (a head tag
  outgrowing its room) and every Ogg write still copy the whole file, and write it back over the
  inode when the file has a second name (only the pages that differ, which for moved audio is all
  of them)

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
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
