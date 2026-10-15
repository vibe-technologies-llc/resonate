# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [S] `Player::new` waits on the first sink survey (`SINK_TIMEOUT`, 2 s) before answering

## iOS
- [S] The poll re-reads the queue on every tick while `catalog_revision` moves, and every enrichment
  step, scan upsert, favourite and landing moves it: `queue()` is a query per row under the player
  lock, then the main actor compares the arrays, on the ordered lane that carries Pause and Next
- [S] Each 500-row page of `filtered_tracks` filters and sorts all N rows again, and `Index::read`
  rebuilds the index under the catalog mutex whenever the revision moved: a `refresh()` at 100 000
  songs is seconds of CPU, stalling `snapshot()`, which needs the mutex for `track_uri`
- [S] `forgetMissing` opens every document on every foregrounding (the `.inactive` to `.active`
  blips of Control Centre, a permission prompt or a call banner included), each open resolving its
  folder's security-scoped bookmark again; it needs only `.background` (`leftAt`) and a cached root
- [S] A paused Now Playing, lyrics or album page keeps the backdrop at 60 fps and the visualizer tap
  polled every 33 ms (`wantsEnergy` ignores the transport; a failing `visualizer` retries at once),
  and the app field draws at 30 fps on every page, which keeps the display from idling
- [S] The palette sample (32 by 32) and the lock-screen artwork (384 px) are decoded from the 1024 px
  bucket (`artwork(_:)` defaults to `.full`), about 4 MiB cached each, evicting row thumbnails
- [M] A scan stores the folder's cover once per track (`artwork` is `track INTEGER PRIMARY KEY`),
  reading up to 8 MiB across FFI for each, and `VACUUM INTO` copies them all again before a
  migration; nothing is excluded from backup (no `isExcludedFromBackup`), so the catalog, that
  copy and `sync-received` travel in device and iCloud backups
- [M] The artwork cache is keyed by song: a 12-track album decodes and reads two blobs under the
  catalog lock twelve times (`the_better_picture`); an album key or content hash would share one
- [S] `ArtworkCache.kept` and `Features.artGenerations` only grow (NSCache evictions never prune the
  first; the second is copied whole on each refresh)
- [M] Whole-catalog work in view bodies and on the main actor: `LibraryView` filters `allTracks` for
  favourites and folds playlist names each body; `AddSongsSheet` refolds every title, artist and
  album on each toggle; `Library` rebuilds two `Set`s from `allTracks` on every heart; the queue
  list copies `rows[range]` and scans for the playing row each body; the tag editor asks
  `tags_writable` once per song per keystroke; `addFolder`, `removeFolder` and `folders()` measure
  and resolve bookmarks on the main actor (a hung File Provider hangs the UI), as do about eight
  keychain reads at launch
- [S] The poll runs at 4 Hz paused, locked and in the background, each tick a `snapshot()`, a
  `track_uri` query and a `nowPlaying.update`
- [M] The synced lyrics timeline runs at display rate while a sheet plays, building a `LyricRowLook`
  for every line and asking Rust for a frame whose position moved by milliseconds, with no throttle
  for a line-synced sheet where nothing turns and no Low Power Mode or thermal check
- [S] `SyncTransferRow`s are keyed by `\.offset`, so a transfer landing re-binds the rows below it to
  other songs; the Inspector decodes an artist portrait with `UIImage(data:)` on every body

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
- [S] Typing a folder into the library settings checks `is_dir` and `canonicalize`s on the window's
  thread, where dropping one does it on the background executor: a hung mount hangs the window

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
- [S] The true-peak guard oversamples 8× with 96 taps at every rate (BS.1770 uses 2× from 96 kHz); a
  shaped-phase design runs under the `DESIGNED` lock
- [M] Search over the unheld discography `instr`s a concatenation per row and `LIKE '%…%'`s
  `release_tracks`, `artist_releases` and `artists.key`, growing with every enriched artist
- [M] A playlist import missing a file rescans every catalog path and allocates a component vector
  each (`reconnected_by_trailing_components`); `Library::playlists` runs a whole-catalog count per
  smart playlist
- [M] Retag's preview opens each file twice (`tags.read`, then `tags.rated`); the organise planner
  keeps every library path and folder listing for the whole run
- [S] A cover decodes under `image`'s default 512 MiB allocation limit, any number at once; a
  vault's cover dedup reads the whole standing file to size it
- [S] A non-seekable source spools up to 8 GiB into `/var/tmp` with no free-space check
- [M] A WAVE import writes and `sync_all`s the whole uncompressed PCM before compressing and
  deleting it, a dedup hit included
- [S] `voices_in_play` walks every earlier line of a one-voice sheet each frame
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
