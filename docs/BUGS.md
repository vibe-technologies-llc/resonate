# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M] An end-of-track or end-of-queue sleep on a DoP stream never fires: `fade_toward_sleep` fades a
  `Silence::Marked` ring, which hushes at once without draining (`ring.rs` `fades` is false), so the
  position freezes ten seconds short, the transport stays Playing all night (idle inhibit held) and
  every Play re-silences after `quiet_within`
- [M] A stream open the daemon never answers is `LoopStopped` (`Survey::open` ignores `unanswered`),
  not parked as the graph's: a hung daemon costs 5 s a queued row, and a late `Ok` leaves an orphan
  stream until the next open
- [M] Between `param_changed` and the engine's rebuild (up to a poll tick) the callback writes the
  ring in the requested layout into buffers of the negotiated one; the negotiated layout is rebuilt
  by count (`from_count`), so a 5.0 or 6.1 stream reads as changed and is rebuilt once
- [M] A device quantising its volume echoes a value outside `ONE_LEVEL_WITHIN` (1e-5) of what was
  sent, and a fast drag pushes the echo out of `TURNS_REMEMBERED`: read as turned elsewhere, the
  slider snaps and the mute clears
- [M] The playback stream's own volume and mute are never read: one restored by the session manager
  scales a stream still called bit-perfect

## Formats
- [M] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps) go to a second
  `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the main decoder
  misses them: a click at every mono/stereo switch of a low-bitrate podcast
- [M] A fragmented MP4 sized from `sidx` counts the first top-level `sidx` alone (one a segment in
  DASH files), and only the first non-empty edit-list entry is honoured, so a spliced M4A stops at
  its first edit
- [S] A whole-file lyric sheet beside a cue-cut file is weighed on its title alone: `Wanted` has no
  whole-file length or artist, so its `[length:]` and `[ar:]` are not checked against the file
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## Tagging
- [M] A write back staged in the spool folder (`written_back_from_elsewhere`, the track's folder
  refusing a new file) keeps no whole copy beside the track: a crash part way leaves the track torn
  with nothing to finish it, and the staged copy under `/var/tmp` swept by nothing
- [M] A landing on the closest pressing (`Fit::Wider`, `Fit::Narrower`) is written into the files as
  a match: a 14-track rip landed on a 12-track pressing gets `TRACKTOTAL=12` in tracks 13 and 14;
  the fit is not stored
- [M] The tag undo record is keyed by path and does not follow organise or a hand-move, so *Put the
  last run back* answers `Unreadable` for the whole library after the usual tag-then-organise

## Catalog
- [M] A scan naming some roots tidies the rest by forgetting every path gone
  (`tidy_the_roots_beside`) before any pairing could follow it: a hand move inside `~/Archive` is
  forgotten with its history by a poll's delivery scan, *Add folder* or `resonate scan ~/Music`, and
  refound as new
- [M] A file retagged as it moved, paired by sound or packets, keeps the gone row's title, artist,
  size, mtime and index entry (`moves::follow` updates root and generation alone) while
  `settle_the_album` files it under the new album: unsearchable by its new names until reprobed
- [M] An album's `tagged_tracks` is one file's raw `TRACKTOTAL`, a disc's count on a set (this
  build's own writer writes it per disc), weighed against a release's whole count: a 12 + 10 deluxe
  rip misses its 22-track pressing and lands strictly on the 12-track standard edition
- [M] A bind mount of the same filesystem is no volume (`st_dev` alone; `/proc/self/mounts` is read
  in `volumes.rs` but not for this): unmounted, its root reads as files moved out and is pruned
- [M] `listening_detail` cuts and buckets days at the phone's current UTC offset (`day * 86400 - offset`), and
  the phones regroup those instants in the local calendar: under summer time every winter day starts
  at 23:00 the day before, so the 1st of a month counts in the previous month and *Busiest day*
  reads a day early; the phones must pass their zone or their day boundaries across the boundary
- [S] A cue sheet the scan cannot read keeps only the cut rows of files beside it
  (`Known::cuts_in`): rows of a file it cut from a folder below are pruned with their history
  until the sheet reads again

## Window
- [M] A key typed into a field arrives through `replace_text_in_range` as `Span::Composing`, which
  joins any next composing edit whatever the selection: undo takes back the whole session, and
  ctrl-a then a letter then undo empties the field with the old text out of reach
- [M] Typing anywhere to search drops text an input method composes until a field is focused; the
  field's preedit skips `one_line` and keeps its mark past the 16 KiB cut; a click can land the
  caret inside a grapheme cluster; selection is one quad across a mixed-direction run

## Phones
- [S] `documents_moved` rekeys a whole batch in one transaction on the `UNIQUE` `documents.uri`
  after Swift has moved the files: a destination held by a hidden missing row fails the batch,
  leaving the moved files under their old URIs, re-indexed by the next scan as new songs
  (favourites, plays and playlists lost) and with no *Put files back* record
- [M] A phone's playback failure says nothing of its cause: `Snapshot.failure` is
  `error.to_string()` ("decoding track 12 failed", source chain dropped), `Error::Player` and
  `Error::Decode` flatten the engine and codec errors to one sentence each, and `Store`,
  `Reference`, `Sync` and `Received` print their op with `{:?}`; the desktop reads
  `engine::Error::cause`, which the boundary never maps
- [S] `send_favourites` loves every favourite on every press, stops at the first refused love and
  records no `loves_told`: one recording a service refuses for good blocks every later favourite on
  Android, and `tell_loves` (which skips `refused_for_good`) later sends them all again
- [S] `snapshot` consumes state (drains events, `take`s `failure`, counts listening), so the two
  `snapshot()` calls `NowPlaying.swift` makes only to read the transport on releasing the session
  can swallow a playback failure the poll never shows
- [S] A found download whose destination cannot be created leaks its job: `find_download` parks the
  provider's reader in `Standing.opened`, neither phone calls `cancel_download` on that path, and
  `begin` evicts settled jobs only, so the job and its connection stay open
- [S] The phones show Rust debug prints as report text: `analyse_track` builds `findings` with
  `{finding:?}` and `inspector` builds `container` with `{:?}` (`IsoMp4`), where `Finding::told`
  and `Container`'s `Display` are the sentences the desktop shows
- [S] Rust caps what a phone hands it unevenly: `keep_sidecar_lyrics`, `keep_lyric_text` and
  `recognise_heard` take any size where `import_playlist_file`, `keep_folder_cover` and
  `keep_room_correction` refuse past a bound (both phones cap at the source today)

## Android
- [S] `RustMediaPlayer`'s available commands omit `COMMAND_RELEASE`, so `SimpleBasePlayer.release()`
  returns before `handleRelease`: the service's retain on `LibraryRepository` is never given back,
  the engine is never saved or closed, the noisy receiver, focus request and widgets outlive a
  dismissed task, and every restarted service adds three more live collectors
- [S] The poll loop in `LibraryRepository.start` has no per-tick catch: one `Error` from
  `snapshot`, `queue`, `save` or `tick` (a full disk refusing `count_listening`) leaves the loop,
  its `finally` shuts the engine down while `started` stays true, and nothing reopens it until the
  service is destroyed
- [S] `RustMediaPlayer` declares `COMMAND_CHANGE_MEDIA_ITEMS` and overrides only
  `handleSetMediaItems`, so a controller's add, move or remove throws `IllegalStateException` on the
  main thread; the exported session's default callback accepts every package
  (`isTrustedForMediaControl` unasked)
- [S] `MainActivity`'s controller listener calls `pending.get()` uncaught, so a session connection
  that fails is an `ExecutionException` on the main thread at launch (`ResonateWidgets.control`
  catches it)
- [S] `oboe.rs` `Opener::open` waits on the stream thread with `started.recv()` and no bound, so a
  stalled `open_stream` on a Bluetooth or USB route change blocks the engine thread; `remote_io`
  bounds the same wait with `UNIT_WITHIN`
- [S] `RustMediaPlayer` pauses on `AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK`, though the platform ducks a
  request without `setWillPauseWhenDucked`, and resumes only if the transport was Playing, so a
  loss during Loading never resumes
- [S] A queue drag lands one row short: `QueuePage` sends `onReorderItem`'s already-adjusted index
  to `moveInQueue`, which counts the row it lands before as before the move (`landing_before`), so
  a downward drag stops a row early and no row can be dropped last
- [M] Online search dead-ends: `SearchPage` renders neither `onlineReleases` (`releaseSongs` is
  never called), nor `artistAlbumsNext`, nor a pasted link's songs while the field is empty, so an
  album result does nothing and an artist shows one page of albums; the older `OnlinePage` is
  unreachable
- [M] Notices (`failure`, `status`) render only on root tabs and the player: a profile import,
  AutoEq apply, playlist edit or folder removal failing on a pushed page shows nothing there, and
  `CollectionPage` spins for ever when `collectionTracks` answers nothing
- [S] `PlayerModel.call` clears `failure` on any success, so a background call (`collections` on
  every catalog revision, `lyrics`, `enrichmentStatus`) erases the error of the action that failed
  within a tick
- [S] Statistics' *Your most played* finds the tapped row with `rows(stats['topTracks']).indexOf`
  over freshly built maps: the index is -1 and every row plays from the default start
- [S] The empty queue's *Browse songs* opens a `collection` of kind `all` that `CollectionPage` does
  not know: `collectionTracks` answers `artist_tracks("")`, the unknown-artist group, titled *Songs*
- [S] Search results and Statistics rows keep the `favourite` of the map they were fetched with: the
  song sheet offers *Add to favourites* again and cannot take one back until a new search
  (`CollectionPage`'s `trackById` merge does it right)
- [S] The Analysis pane lists the last `recognition` for whatever song it shows; `loadInspector` and
  `searchCorrections` carry no request generation, and `applyCorrection` takes a list index, so an
  overlapping reply can show another song's result or apply another headphone's profile
- [S] The *Device sync* switch and every *Sync options* switch edit a draft only *Save sync
  settings*, in another card above, sends; nothing marks it unsaved and leaving the page drops it
- [S] *Made for you* applies its mix as the global `library.filter`: back on the Songs tab, it is
  still filtered to the mix; the mix needs its own query (`filtered_tracks`)
- [S] Search lists at most 500 of `trackCount` songs under a *Songs (N)* chip and plays only those
- [S] Playlist rows read `artworkTrack`, which `MusicPlaylist` and its wire map never carry, so
  every playlist shows the generic icon where `android.md` promises up to four covers
- [S] Search carries a *Listen & recognise* button whose sheet only describes recognition and has
  nothing behind it
- [S] A playlist page shows the empty state and a disabled Play between `selectPlaylist` (which
  sets `playlistRows` to an empty list) and its rows: loading cannot be told from empty
- [S] The light theme's *Search online* card draws its second line in `Colors.white70` on the
  near-white `surfaceContainer`
- [S] `LibraryFeatures.stop` never releases the `MulticastLock` `hearBeacons` acquired, so it stays
  held, multicast filtering off, for the life of the process
- [S] `LibraryRepository.scanFolder` admits files by its own extension and cover-name lists, never
  `audio_extensions()`, so a format added to core is scanned on desktop and iOS and skipped here
- [S] The release build type signs with the debug keystore and reads no supplied signing config

## iOS
- [S] `Erasure.everything` discards every failure (`try?` on each directory listing and
  `removeItem`, an unchecked `SecItemDelete` per class) and `eraseEverything` then calls `exit(0)`:
  a catalog, history file or sync key that survives is never reported and the next launch resumes
  from it, rejoining sync with the old key
- [S] `PairingSheet` keeps the pairing code revealed when the scene leaves `.active`, so the App
  Switcher snapshot shows the sync key, and `.textSelection(.enabled)` on the blurred text lets the
  system Copy put it on the general pasteboard, past the `localOnly` one
- [S] `Enrichment.read` on coming forward publishes the status and changed art but never `settle`s
  a pass that ended while the scene was inactive or backgrounded, so `Library.allTracks` keeps rows
  without its `artist_mbid` and album artist, and portraits stay absent until a scan or launch
- [S] `NowPlaying.interrupted` marks a resume only for `.playing` while `Library.pause` also pauses
  `.loading`, so an interruption beginning during a song change pauses and never resumes
- [S] `Navigation.open` appends a destination already on top of the stack (*Go to album* from Now
  Playing on that album, *More › Equaliser* twice), so Back lands on a copy; only `SyncProgress`
  guards it
- [S] `SystemVolumeLevel.watch` registers its `outputVolume` KVO synchronously on the main actor
  (`observing` is `nonisolated` but called from `watch()`), the one `AVAudioSession` call left off
  `Work.inSession`
- [S] Lyrics read at about 22 % alpha when nothing is lit (an unsynced sheet, a song not playing):
  `LyricInks.muted` (0.5) is drawn under `.opacity(ADRIFT 0.45)`, and `LyricsPane` has no Increase
  Contrast or Reduce Transparency branch
- [S] Now Playing's sleep countdown, volume glyphs and Up next list (`QueueHeading`, the track
  detail line) are `.secondary` over the lifted field, not `nowPlayingInk`
- [S] `SyncDevices`' header menu, `SeeAllLink` and the recent searches' *Clear* skip
  `touchTarget()`, and `FolderRow`'s three actions are `.controlSize(.small)`: 20 to 28 pt targets
  where `ios.md` fixes 44 pt

## Equaliser and DSP
- [M] An equaliser cell opens holding `{:.0}` hertz and `{:.2}` dB: Enter on it untouched moves a
  1059.46 Hz band to 1059 Hz; the one shared figure `Field` is reloaded through `set_content`, which
  pushes an undo step, so ctrl-z in a cell reaches the last cell's value and Enter writes it
- [M] Tab out of an open equaliser cell moves the caret on, commits nothing and leaves the cell
  drawn open (`leave_figure` answers only while the figure holds focus); a press outside no longer
  closes it; the Online and Library fields share the shape
- [M] Saving or exporting a profile rewrites it from the parsed model: comments, `Include:`,
  `Device:`, `If:`, filters past 32 and the original text encoding are lost; `export` is a bare
  `fs::write`; two writers stage to one fixed `.txt.new`

## Search
- [S] Android's *Go to album* finds the album by raw title and album artist, the player's album
  link by title alone: a song with no album, another spelling or a *Various artists* release does
  nothing and a shared title opens the wrong album; `album_key` is exported and uncalled, as is
  `highlights`
- [M] A multi-valued genre is one `tracks.genre` (`House; Disco`): `genre:=House` misses it, and
  suggestions tally whole strings (a *House; Disco* card); a one-word genre card is a prefix
  (`Reach::Begins`), so *Rock* holds *Rockabilly* and *Rocksteady* (artist mixes were made whole)
- [M] A song is ranked only among the first 25 recordings MusicBrainz answers, all scored alike for
  a title many artists recorded: *creep* lists Radiohead's 53rd of a hundred covers, and a typo
  (*bohemian rapsody*) lists covers of *Bohemian Rhapsody* without Queen's

## Keyboard and accessibility
- [M] Enter or Space on a ringed control presses it twice: `PressControl` fires on key-down and gpui
  0.2.2 turns the key-up on a focused element with click listeners into a `ClickEvent::Keyboard` for
  `on_click` (`div.rs`); Next skips two, a switch flips back. The driven tests dispatch key-down
  only
- [M] The keyboard reach keeps its row through `opened` and an order or reading change: after
  reaching row 19, *Go to album* on a 10-track album and Enter plays its last track with nothing
  marked; after a header press the edge stays on another song

## Later: The vault
- [M] A cancel is not heard while a row is read back, decoded for dedup, or kept as it came
  (`pcm_of` and the `copied` loop take no `Halt`): a multi-gigabyte DSD keep runs to its end
- [M] A cover's lossless JXL is read back from memory, not the staged file; a 16-bit PNG is
  quantised to 8 bits, an animated picture keeps its first frame and an ICC profile is dropped
  (compared against the same lossy decode, so it passes)
- [S] A folder sync failing after the rename reports the keep failed though the object landed,
  unreferenced until `--prune`
- [L] The key is the PCM alone: two tracks of the same samples at another rate, count or mask
  (digital silence) share an object, and a standing object is taken on its size alone; needs a
  migration
- [M] Blanking a Matroska file stops at an element of unknown size or after 65 536 elements and
  lands what it blanked so far as stripped; a `TrackEntry` name, chapters and an MP4's track-level
  `meta` are never blanked; an MP4 past 4 096 boxes keeps every tag

## Later: Listen and recognition
- [M] A capture stream failing without the daemon going (`StateChanged` to `Failed`) is never
  reopened: `was_lost` reads only a closed channel, so an unplugged microphone records nothing till
  the deadline; a reopened capture splices onto the clip with no gap mark, and a failing stop
  discards a full clip
