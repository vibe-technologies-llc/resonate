# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [S] `Player::new` waits on the first sink survey (`SINK_TIMEOUT`, 2 s) before answering

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

## iOS
- [S] Applying room correction (`RoomCorrectionState.turn`, and `choose` after a pick) goes through
  `Library.request`, the transport's lane, and decodes the whole response there (up to 64 MiB):
  play, pause, seek and the activation ahead of `play()` wait behind it, where `ios.md` puts it on
  `Work.aside`
- [S] `AVAudioSession` calls still run on the main actor against `ios.md`: `routeReading`'s default
  (`RouteReading.current`, in `Library.init`), `OutputRoute.current` in `start`, `configureSession`
  at launch and on a media-services reset, and Listen's `setCategory`/`setActive` in both directions
  from the `@MainActor` `Microphone`
- [S] Folder and native calls that may hang run on the main actor: `Features.landReceived` calls
  `Folders.fresh` and `delete` per received song and `begin` deletes the landing journal's
  leftovers, `DownloadQueue.recover` calls `song_uris` and `Folders.delete`, and
  `Enrichment.stopForErasing` joins the pass; downloads do the same steps on `Work.aside`
- [S] `Library.start` awaits `refresh()` (every catalog page, then indexing on the main actor)
  before the first `poll`: on a large library the restored mini player and lock-screen info appear
  only once the whole catalog is read, and remote commands answer `.noSuchContent` meanwhile
- [S] `pollSync` awaits `landReceived` inline, which copies each song and awaits `library.refresh`,
  `refreshWanted` and `refreshChangedArt`: the fetching card's progress, `syncHandoffs` and
  `answerSyncAsked` stand still while a large song lands, unlike `adoptArrivals`, which has a task
  of its own
- [M] `Folders.root` resolves the bookmark and starts a scope on every call, once per
  `openDocument`, `companion`, `withPaths` element and organise move: each missing-song pass and
  scan pays a `URL(resolvingBookmarkData:)` per song
- [S] Every keychain write (`saveSync` on each sync switch, `saveScrobbling`'s four items,
  `saveRecognitionKey`, `keepProviderAccount`, the minted code, `signInLastFm`) runs
  `SecItemUpdate`/`SecItemAdd` on the main actor, though the launch reads moved off it
- [S] `SettingsView.soundEntries` reads `clock.sleepRemainingMs` in the root body: while a sleep
  timer runs, every tick rebuilds the whole Settings list and its search filter
- [S] `FavouritesView.body` folds title, artist and album of every favourite on the main actor per
  evaluation with text typed, at every keystroke and heart; *Add songs* folds once off the main
  thread (`SongKeys`) and filters after 150 ms
- [S] `DownloadQueue.follow` assigns each job's progress every 250 ms even unchanged (finding,
  validating, stalled), and `@Observable` notifies regardless: Online search, *Saved for later* and
  the Downloads summary redraw four times a second for the whole transfer
- [S] `VisualizerView` holds the tap from `onAppear` to `onDisappear`, and `tapHoldersChanged` reads
  neither `foreground` nor the transport: with the page open the 30 Hz read loop runs while paused
  and while playing in the background
- [S] `BackdropView.rate` reads Low Power Mode alone: on a serious or critical thermal state the
  full-screen shader keeps 60 fps, where the lyric timeline drops to 30
- [S] `remote_io::serve` wakes every 10 ms (`recv_timeout(COMMAND_POLL)`, `clock.note`) for as long
  as the stream is open, paused included; the latency moves only in `route_changed`
- [S] `OnlinePlaceholder` clips every online song, album, artist, release, want and download row
  with `AnyShape`, the offscreen mask `ArtworkClip` exists to avoid

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
