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
- [S] A queue drag (`Queue::move_rows`) rebuilds `order`, `next` and `unshuffled` from every row
  (`flagged`, `split`) where a span could be spliced out of the persistent vectors and in again, and
  under shuffle an insert or removal walks `unshuffled` to find where a row stands; a place-to-row
  index beside the vectors would make both cost what they move
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
