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
- [M] Every repository refresh sends the whole `tracks` and `allTracks` lists across the event
  channel, `refreshCollections` re-reads every album and artist on each catalog revision, and
  `AppShell` rebuilds every mounted root on each notification, so a large library pays for it on
  every enrichment step
- [M] `refresh()` and `reload()` hold the repository's `access` lock for the whole paged read, so the
  poll and every queued transport action wait behind a big library
- [S] A folder scan reads up to 8 MB of cover per directory and hands it to `set_artwork` for every
  song of that directory
- [S] `hide_missing_songs` opens every document under the granted folders (one IPC each) on every
  foreground, at most every five minutes

