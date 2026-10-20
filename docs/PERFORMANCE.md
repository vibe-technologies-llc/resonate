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

## iOS
- [M] Playing in the background polls `snapshot` twice a second (ten times near a song's end) only
  to catch a song change the engine knows; `count_listening` rides on that poll, so an engine event
  needs the counting moved first

## Performance and scale
- [M] Every open of a Matroska file still reads every block header of it (up to
  `MAX_ELEMENTS_COUNTED`), and of a VBR MP3 naming no length every frame header (up to
  `MOST_FRAMES_WALKED`), scan and playback alike; a count persisted beside the catalog's duration
  would spare playback the walk
- [L] Queue edits are O(n) on the engine thread: `one_id_each` hashes every id an insert,
  `rows_changed` rehashes every location, a consumed play-next row is three `retain` passes, and
  `publish_queue` clones every row (a `PathBuf` allocation each) whenever the revision moves. The
  publish dominates and cannot shrink while `Queued::rows` is an owned `Vec<QueueItem>` in play
  order: rows shared across publishes (an `Arc` a row, or a persistent vector) reach every reader of
  `Queued::rows` in resonate-ui, -mpris, -mcp, -mobile and the binary; the stamps are order-dependent
  SipHash, so they cannot be kept incrementally either
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
