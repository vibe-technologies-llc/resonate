# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Tagging
- [M] On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name

## Window
- [M] Each reload empties the queue's name cache and the redraw then reads SQLite for every uncached
  row on the window's thread; Statistics' *play from here* reads a track a query

## Android
- [S] The lyrics pane builds every line in a non-lazy `Column` and rebuilds all of them on each
  4 Hz frame while it is open, a blur filter per line away from the read one

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
- [M] The convolver works complex FFTs on real input (twice the memory and multiply-adds), stores a
  mono response's spectrum once a channel, `%`s per partition, and `loudest_gain` allocates
  `next_pow2(frames × 4)` complex values; nothing bounds a long response at a high rate in bytes
- [M] Search over the unheld discography `instr`s a concatenation per row and `LIKE '%…%'`s
  `release_tracks`, `artist_releases` and `artists.key`, growing with every enriched artist
- [M] Retag's preview opens each file twice (`tags.read`, then `tags.rated`); the organise planner
  keeps every library path and folder listing for the whole run
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
