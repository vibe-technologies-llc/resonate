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
- [M] Under shuffle a queue insert, a removal and a drag that moves rows between the play order and
  the rows played next walk `unshuffled` for where a row stands; a position index beside the vector
  (order-maintenance labels or a counted tree, neither in `imbl`) would make them cost what they
  move
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Android
- [M] `refresh()` and `reload()` hold the repository's `access` lock for the whole paged read, so the
  poll and every queued transport action wait behind a big library
