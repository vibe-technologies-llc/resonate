# Roadmap

Open work only. Categories run most to least important; items likewise (prerequisite before
dependent, easier first among equals). `Later:` = nice-to-have no listener waits on.
**Blocked on …** = waits on the named outside thing (hardware, upstream crate, service, format),
sits last in its category, not worked until it moves. Everything else is open.

## Playback and output
- An end-of-track or end-of-queue sleep on a DoP stream never fires: `fade_toward_sleep` fades a
  `Silence::Marked` ring, which hushes at once without draining (`ring.rs` `fades` is false), so
  the position freezes ten seconds short, the transport stays Playing all night (idle inhibit
  held) and every Play re-silences after `quiet_within`
- A stream open that fails and parks its row (`land_the_stream` drops the output before `fail`,
  `unbound` unset) parks it at the decoder's position, a ring's depth past what was heard: when
  the device or daemon returns, the first half second of the track (or after a seek) is skipped
- An end-of-queue sleep under repeat-queue never fades (`sleep_is_due_in` asks
  `ends_with_this_row`, not `wraps_next`), and a queue ended by `remove` or `fail` emits
  `QueueFinished` leaving the timer armed: the next album loaded pauses at its end unasked
- A row whose first bind fails (`NoSink`: headphones not yet connected) returns from `started`
  before `TrackStarted` and before `renegotiations` resets; parked and played later it is never
  announced, so `resonate play a b` with `b` failing exits 1 `NothingPlayed` though `a` played
- `Insert { play: true }` at `Placement::Queued` lands the run at the end of the play-next block
  and `jump_to` moves only its first row to the front: `resonate queue a b c --play` over two
  queued rows plays a, the two queued rows, then b and c
- Jumping to a queued row while another queued row plays restamps (`let_go_of_what_was_heard` ->
  `rows_changed`) while the chosen row is out of `next`, stamping it as part of `order`: the
  playlist badge and MPRIS `ActivePlaylist` go for that row's length
- Changing the graph rate mid-track reopens the stream, costing the gap a sink switch does; so do
  the rate policy, buffer or DoP wherever the change moves the stream's format or the ring's depth
- Choosing the sink already bound, or changing quality, filter phase, dither, restoration, noise
  shaping, convolution or forced rate on a track whose plan they leave alone, still fades out and
  reopens: only `SetBitPerfect`, `SetDop` and `SetBuffer` ask `keeps_its_stream`
- Changing the equaliser is heard up to the buffer's depth later (half a second by default): the
  filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- A stream open the daemon never answers is `LoopStopped` (`Survey::open` ignores `unanswered`), not
  parked as the graph's: a hung daemon costs 5 s a queued row, and a late `Ok` leaves an orphan
  stream until the next open
- Between `param_changed` and the engine's rebuild (up to a poll tick) the callback writes the ring
  in the requested layout into buffers of the negotiated one; the negotiated layout is rebuilt by
  count (`from_count`), so a 5.0 or 6.1 stream reads as changed and is rebuilt once
- A device quantising its volume echoes a value outside `ONE_LEVEL_WITHIN` (1e-5) of what was sent,
  and a fast drag pushes the echo out of `TURNS_REMEMBERED`: read as turned elsewhere, the slider
  snaps and the mute clears
- The playback stream's own volume and mute are never read: one restored by the session manager
  scales a stream still called bit-perfect
- `Player::new` waits on the first sink survey (`SINK_TIMEOUT`, 2 s) before answering
- One playback stream and one capture stream only; concurrent playback streams unsupported
- **Blocked on hardware:** no forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

## Formats
- A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps) go to a second
  `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the main decoder
  misses them: a click at every mono/stereo switch of a low-bitrate podcast
- A whole-file lyric sheet beside a cue-cut file is checked against the cut row before it is cut
  (`Sidecar::held` reads with the row's `Wanted`, then `cut_of_the_file`): a sheet headed
  `[ti:Meddle][length:46:45]` names "another track" for every row and the pane shows nothing; the
  test's sheet has no header
- Matroska's trailing `DiscardPadding` is taken from whichever track's block ends the file
  (`read_cluster` sets `discarded` for every SimpleBlock and BlockGroup): a video frame last keeps
  the Opus padding, another audio track's padding trims this one
- A mid-stream change of rate or channel count (chained Ogg Vorbis, an MP3 going mono to stereo)
  fails the track with `ResetRequired`; nothing reopens the stream at the change
- One DST frame that will not unpack fails the whole row as `Error::Io` rather than playing as a
  hole as every PCM codec's does; a `DSTF` chunk's length is bounded only by the file, so a hostile
  one reads the rest of it into memory on a seek
- A fragmented MP4 sized from `sidx` counts the first top-level `sidx` alone (one a segment in DASH
  files), and only the first non-empty edit-list entry is honoured, so a spliced M4A stops at its
  first edit
- A Monkey's Audio file's last frame carries any trailing APE tag: copied each read, refused
  outright past `LARGEST_FRAME_BYTES` with the tag
- A `SYLT` frame stamped in MPEG frames is dropped (`as_lrc` reads milliseconds only); a UTF-16
  descriptor with no byte-order mark is read little-endian
- **Blocked on `ape-decoder`:** a 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- **Blocked on `symphonia-codec-wavpack`:** a `.wvc` correction file beside a hybrid WavPack is
  never opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held
  zero's correction from the wrong range
- **Blocked on symphonia:** a file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- **Blocked on symphonia:** Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- A write back staged in the spool folder (`written_back_from_elsewhere`, the track's folder
  refusing a new file) keeps no whole copy beside the track: a crash part way leaves the track torn
  with nothing to finish it, and the staged copy under `/var/tmp` swept by nothing
- A tag landed in place never asks `Taken::still_stands`, as the staged path does: a page another
  program changed between the overlay's read and `land` is put back
- A landing on the closest pressing (`Fit::Wider`, `Fit::Narrower`) is written into the files as a
  match: a 14-track rip landed on a 12-track pressing gets `TRACKTOTAL=12` in tracks 13 and 14;
  the fit is not stored
- The first of several ISRC takes is landed `Nearly` for a file of no length and its recording id
  written, then loved on Last.fm and ListenBrainz by it
- The tag undo record is keyed by path and does not follow organise or a hand-move, so *Put the last
  run back* answers `Unreadable` for the whole library after the usual tag-then-organise
- On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name
- A cue-cut row is never written; an album landed as a release group gets no totals
- **Blocked on `lofty`:** `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Catalog
- A cue sheet the scan cannot read (`read_sheet` answers `None` for an `EIO`, `ESTALE`, a sheet
  read mid-rewrite as empty) claims nothing, so its file is probed whole and rows 2..N are pruned
  with their plays, listens and favourites: against *what the scan could not read, it keeps*
- A scan naming some roots tidies the rest by forgetting every path gone (`tidy_the_roots_beside`)
  before any pairing could follow it: a hand move inside `~/Archive` is forgotten with its history
  by a poll's delivery scan, *Add folder* or `resonate scan ~/Music`, and refound as new
- A file retagged as it moved, paired by sound or packets, keeps the gone row's title, artist,
  size, mtime and index entry (`moves::follow` updates root and generation alone) while
  `settle_the_album` files it under the new album: unsearchable by its new names until reprobed
- An album's `tagged_tracks` is one file's raw `TRACKTOTAL`, a disc's count on a set (this build's
  own writer writes it per disc), weighed against a release's whole count: a 12 + 10 deluxe rip
  misses its 22-track pressing and lands strictly on the 12-track standard edition
- On a case-insensitive volume two destinations differing in case both preview and the second is
  refused `Collided` at apply (`claimed` is keyed by exact path)
- A cross-filesystem organise copy (`copied_whole`) keeps the mtime alone, dropping xattrs and
  ownership; a rewritten sheet or exported playlist (`staged_over`) is created with default mode
  and no folder fsync
- Deleting a track is `fs::remove_file`: no trash, its `.cue`, `.lrc` and emptied folder left
  behind (organise prunes its own); its listens cascade away, shrinking statistics
- A file whose name is not UTF-8 is counted `Unnamed` and never catalogued
- A bind mount of the same filesystem is no volume (`st_dev` alone; `/proc/self/mounts` is read in
  `volumes.rs` but not for this): unmounted, its root reads as files moved out and is pruned
- `filtered_counts` applies the music filters by replacing `t.hidden = 0` in SQL text

## Providers and network
- Last.fm is sent a batch in requests of 50 (`LastFm::submit`) and answers `Err` at the first that
  fails; the library marks nothing on such an error, so the chunks already taken are scrobbled
  again next pass (`library.md` says the mark moves past what was told)
- A TIDAL account is asked by link and code alone: its OpenAPI filter has no text search here, so a
  song TIDAL holds under a code MusicBrainz never lists is found by hifi-api and Monochrome only
- A device sign-in's cancel is heard only between polls (each request is bounded by the API
  agent's 20 s)
- The inbox takes a file as settled after two quiet seconds of mtime and ctime, no size compared:
  a torrent client writing sporadically delivers half a file, and the refusal burns the want's offer
- `Refused` is collapsed into `Unreadable` at the equaliser and lyric seams, so a 429 from LRCLIB
  or GitHub cannot be backed off from
- Threads of one `Client` do not share a backoff until a request gives up: on a 503 four cover
  threads climb their own 2-4-8 s ladders
- A rotated TIDAL refresh token is kept in memory alone where there is no settings path, and a
  failed write only warns: the next run has lost the sign-in
- The hosted hifi and Monochrome services are registered whenever `online` is on, with no opt-in of
  their own; the hifi token request claims another site's `Origin` and `Referer`

## Window
- *Forget* beside a library folder is one unarmed press (`forget_root` from a click or
  `PressControl`) deleting every row under it with plays, favourites and enrichment, no undo and
  no toast; *Reset everything* arms on a first press
- *Take out of the queue* on a track queued twice sends one `Command::Remove` a row under the same
  stale `queued.revision`: the first lands, the rest are refused `QueueChanged` into the log, the
  toast says one went and *Put back* is hidden behind steps still standing over queued rows
- The opened playlist's column header sorts by the window-wide `keeping`, `row_order` and
  `row_reading` (`show_playlist` resets none): after *From now on* on one playlist, a header press
  on another keeps it in order unasked; on a saved search or kept list it is a trouble toast
- A track row's menu *Play* (hinted `enter`) queues that row alone; a click or Enter plays the
  listing from it
- The playlist menu's *Play* (index card, row, pinned sidebar row, the ⋯ mark) leaves shuffle on;
  every other playlist *Play* sends `plays_in_order` first
- An album or artist cell's menu queues through `listing_whole`, the search text and the tracks
  pane's sort included: *Play* on an album the search listed for one track plays that track, in
  title order where the tracks pane sorts so; dragging the same cell queues the album whole
- The seek rail's `along` multiplies in `f32`: a release at the rail's end on a track past 2^24
  frames (6:20 at 44.1 kHz) rounds a few frames past the end about half the time, refused
  `SeekOutOfRange` with a toast (the analysis waveform's edge too)
- The play button draws *Play* while a row opens (`Buffering`), and pressing it pauses
- Time left floors to the second: `-0:00` through the last second, elapsed and left a second short
  of the length
- The visualiser's hints say sixth-octave bands tilted 3 dB an octave whatever `spectrum-bands`
  and `spectrum-tilt` hold
- A key typed into a field arrives through `replace_text_in_range` as `Span::Composing`, which
  joins any next composing edit whatever the selection: undo takes back the whole session, and
  ctrl-a then a letter then undo empties the field with the old text out of reach
- *Play*, *Shuffle*, *Next* or *Last* on a playlist from the index reads it whole on the window's
  thread (`entries_of` → `Library::playlist_entries`), a smart one running its query over the
  catalog; the tracks heading already does this on the background executor
- A library read that fails is a log record only (`landed`), and the pane then advises *No albums
  yet. Add a music folder*
- Heading Play and Shuffle, a suggestion row, a scoped listing, a dragged album and a downloaded
  song to play fail into the log alone: the press does nothing visible
- The sidebar does not scroll: at the 520 px minimum, or with pinned playlists piling up, Settings
  and the download and enrichment status fall under the playback bar
- A fifth toast is dropped while four wait (`WAITING_AT_MOST`), errors included, with no way to
  see a missed one
- A settings write whose writer died (a cancelled oneshot) is never told
- Every visible track row copies the whole selected run each frame (`Lifted::tracks` deep-copies
  into an `Arc` and formats): after Ctrl-A on a 2 000-track page scrolling stutters; the queue
  shares an `Arc<[Cut]>`
- Each reload empties the queue's name cache and the redraw then reads SQLite for every uncached
  row on the window's thread; Statistics' *play from here* reads a track a query
- Typing a folder into the library settings checks `is_dir` and `canonicalize`s on the window's
  thread, where dropping one does it on the background executor: a hung mount hangs the window
- Typing anywhere to search drops text an input method composes until a field is focused; the
  field's preedit skips `one_line` and keeps its mark past the 16 KiB cut; a click can land the
  caret inside a grapheme cluster; selection is one quad across a mixed-direction run

## Command line and bus
- `resonate play` keeps the queue only on the 500 ms sample (`keep_the_queue`, itself throttled to
  `KEPT_EVERY`) and not once more as it leaves: `n` then `q`, or a `resonate queue` just before
  Ctrl-C, resumes on the row before
- A launch handing files to an open window (`handed_to`) drops `--sink`, `--bit-perfect`,
  `--quality`, `--dither`, `--library`, `--config` unsaid and exits 0
- MCP `resources/unsubscribe` runs the subscribe check first, so a playlist discarded or renamed
  since is `UnknownResource` and its URI read every tick for the session
- MCP `show_queue` and the queue resource promise play order and list MPRIS `Tracks`, the
  unshuffled list: with shuffle on a model names the wrong next song
- The Discord socket search lacks `.flatpak/com.discordapp.DiscordPTB/xdg-run`, which
  `discord.md` lists
- The command line cannot mark a favourite, want a track or dismiss a missing one; only the window
  and MCP can
- A setting a subcommand writes (`resonate eq --profile`, `--on`, `--unbind`) is not heard by a
  running window or player, and nothing says so
- `resonate queue --playlist` sends one unchunked `AddTracks`, a catalog lookup a row behind the
  client's 2 s timeout: a large playlist reports failure, lands anyway and a retry doubles it
- MCP opens a fresh D-Bus connection for every player call, a subscribed now-playing resource
  twice a second on the one answering thread
- MCP lists are unpaginated (`cursor` ignored), `list_playlists` and the resource list are
  unbounded, rebuilt every 500 ms once listed
- MCP tools answer `structuredContent` with no `outputSchema`
- `config.toml`'s 89 keys are documented nowhere a hand-editor reads; man pages cover the CLI alone

## Equaliser and DSP
- An EqualizerAPO profile's `Preamp:` lines replace one another (`apo.rs` keeps the last) where
  EqualizerAPO sums them: an AutoEq preamp plus a trim plays louder by the first, uncounted, and a
  save writes the last alone
- An equaliser cell opens holding `{:.0}` hertz and `{:.2}` dB: Enter on it untouched moves a
  1059.46 Hz band to 1059 Hz; the one shared figure `Field` is reloaded through `set_content`,
  which pushes an undo step, so ctrl-z in a cell reaches the last cell's value and Enter writes it
- Tab out of an open equaliser cell moves the caret on, commits nothing and leaves the cell drawn
  open (`leave_figure` answers only while the figure holds focus); a press outside no longer
  closes it; the Online and Library fields share the shape
- Saving or exporting a profile rewrites it from the parsed model: comments, `Include:`, `Device:`,
  `If:`, filters past 32 and the original text encoding are lost; `export` is a bare `fs::write`;
  two writers stage to one fixed `.txt.new`
- With `true-peak` off nothing weighs what the equaliser adds: a zero preamp under boosts reaches
  the hard clamp with no count or report
- The pane writes a gain into a gainless band (`EqualiserModel::take`) and stores it un-normalised,
  so equal notches compare unequal
- Selecting another profile while playing keeps the old filters' history in the new ones
- AutoEq's suggestion takes the first measurer `INDEX.md` lists for a device, none ranked

## Search
- The albums and artists panes match a search over hidden rows and other copies
  (`Matching::grouped` takes neither `THE_BEST_COPY` nor `A_SHOWN_TRACK`): a hidden *Seamus* lists
  *Meddle* and Pink Floyd for `seamus` while the tracks pane lists nothing
- A multi-valued genre is one `tracks.genre` (`House; Disco`): `genre:=House` misses it, and
  suggestions tally whole strings (a *House; Disco* card); a one-word genre card is a prefix
  (`Reach::Begins`), so *Rock* holds *Rockabilly* and *Rocksteady* (artist mixes were made whole)
- An album order by year read the other way flips `a.year IS NULL` too: the artist page and the
  albums pane's default *Year* reading list undated rips first
- A song is ranked only among the first 25 recordings MusicBrainz answers, all scored alike for a
  title many artists recorded: *creep* lists Radiohead's 53rd of a hundred covers, and a typo
  (*bohemian rapsody*) lists covers of *Bohemian Rhapsody* without Queen's
- A pasted TIDAL playlist link is taken as words; a Spotify playlist is read off its embed page and
  a YouTube one off its public page, each listing at most its first hundred songs, and an Apple
  Music one off its public page, none naming an ISRC
- **Blocked on a service:** a lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists; no FhG encode
  weighed
- **Blocked on a registered key:** AcoustID never reached with a real key; its fixture is written
  from the documentation and `tests/live.rs` has no AcoustID test

## Performance and scale
- Every track open opens its source twice, once more for the inspector's box layout
  (`Unwrapped::inspected` → `probe_boxes`; `probe_stream` likewise), on the path before the first
  sample: a second request a skip for a network provider
- Every open of a Matroska file walks every block of it (`count_clusters` caps per cluster and in
  clusters, not in total), and of a VBR MP3 naming no length every frame header (`mpa::walked`),
  scan and playback alike
- Queue edits are O(n) on the engine thread: `one_id_each` hashes every id an insert,
  `rows_changed` rehashes every location, a consumed play-next row is three `retain` passes, and
  `publish_queue` clones every row whenever the revision moves
- A non-local source's read allocates and zeroes up to 1 MiB, then copies it twice
  (`serve_the_reads`); vault playback goes through it
- The convolver works complex FFTs on real input (twice the memory and multiply-adds), stores a
  mono response's spectrum once a channel, `%`s per partition, and `loudest_gain` allocates
  `next_pow2(frames × 4)` complex values; nothing bounds a long response at a high rate in bytes
- The true-peak guard oversamples 8× with 96 taps at every rate (BS.1770 uses 2× from 96 kHz); a
  shaped-phase design runs under the `DESIGNED` lock
- Search over the unheld discography `instr`s a concatenation per row and `LIKE '%…%'`s
  `release_tracks`, `artist_releases` and `artists.key`, growing with every enriched artist
- A playlist import missing a file rescans every catalog path and allocates a component vector
  each (`reconnected_by_trailing_components`); `Library::playlists` runs a whole-catalog count per
  smart playlist
- Retag's preview opens each file twice (`tags.read`, then `tags.rated`); the organise planner keeps
  every library path and folder listing for the whole run
- A cover decodes under `image`'s default 512 MiB allocation limit, any number at once; a vault's
  cover dedup reads the whole standing file to size it
- A non-seekable source spools up to 8 GiB into `/var/tmp` with no free-space check
- A WAVE import writes and `sync_all`s the whole uncompressed PCM before compressing and deleting
  it, a dedup hit included
- `voices_in_play` walks every earlier line of a one-voice sheet each frame
- **Blocked on gpui:** every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream
  (zed #62455)

## Keyboard and accessibility
- Enter or Space on a ringed control presses it twice: `PressControl` fires on key-down and gpui
  0.2.2 turns the key-up on a focused element with click listeners into a `ClickEvent::Keyboard`
  for `on_click` (`div.rs`); Next skips two, a switch flips back. The driven tests dispatch
  key-down only
- The keyboard reach keeps its row through `opened` and an order or reading change: after reaching
  row 19, *Go to album* on a 10-track album and Enter plays its last track with nothing marked;
  after a header press the edge stays on another song
- The sleep timer is a mouse-down handler outside the focus ring with no key: a keyboard-only
  session cannot set one; seeking to or copying a lyric line is pointer-only too
- An icon-only control shows no name on focus (tooltips need a pointer) and the confirming toast
  leaves after 6 s; `TextSize::Large` is 1.15× the normal; no reduced-motion setting
- A selection is a contiguous run reached by Shift; Tracks and Albums act on its first row alone;
  nothing adds a row with Control
- **Blocked on gpui:** nothing is exposed to a screen reader: published gpui 0.2.2 carries no
  AccessKit, which only Zed's `main` has

## Build and checks
- No fuzz target reaches the vault's rewriters (`blanks`, `ogg`, `chunks`, `bare`,
  `unpacking::frames_in`), tag writing (`padded`, `Overlay`, `journal::decoded`), cover decoding
  (`artwork::Drawing::of`), the remote document readers (`shared`, `spotify`, `youtube`, `linked`,
  the DASH manifest) or the analysis cache reader
- Twelve of thirteen inputs that crashed `probe` sit only in the ignored `fuzz/artifacts/`, not in
  `tests/found/`; `fuzz/seeds/` is never replayed by `cargo test`
- `Encoding::OF_THIS_BUILD` is bumped by hand: a `flacenc` upgrade or a changed `TUKEY_ALPHA` or
  `ARCHIVED_AT` leaves old objects called current
- `zstd` (via `zstd-sys`) and `ring` (via `rustls`) are C-backed with no reason recorded in
  `dependencies.md`; `md-5` 0.10 and `miniz_oxide` 0.8 build beside the workspace's versions

## Testing
- The MPRIS bus tests skip without a session bus and CI's `cargo test` gives them none, so all but
  the `dbus-run-session` ones pass having run nothing
- `downgrade`, `MAX_RENEGOTIATIONS` and `Error::Renegotiation` are untested (the only
  `FormatChanged` test answers the spec asked for)
- Loudness and the print have no reference: no test against `ffmpeg -af ebur128` or EBU 3341/3342,
  K-weighting checked at 48 kHz alone, the print asserted only to begin `AQA`
- Nothing races two imports of one key, or a dedup hit or renewal against a sweep, though
  `landed_or_standing` exists for it
- TIDAL's `slow_down` is untested;
  the hosted hifi path's hosts are constants no fake server can stand in for, so it runs only live
- Discord's `Publisher::show` (backoff, application change, reconnect) and the signal paths beyond
  `play` and `mcp`'s SIGHUP (`until_told`, a second signal's 128+n) are untested
- The Output and Processing settings panes have no test pressing their controls
- The scan is never fed a non-UTF-8 name
- `transport.rs`'s `turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame`
  failed once when the whole workspace's tests ran at once, passed alone eight times after; its
  frame counts assume the engine thread keeps pace with the test's pulls
- No frame is pulled through a stream in S16, S32, packed or padded S24, 5.1 or 7.1, or with
  `NO_CONVERT`, nor recorded from a microphone node: the desktop's sink is stereo F32 and a hosted
  daemon links nothing (no session manager; a hosted WirePlumber would reach the real cards)
- The drop overlay never dragged onto on a real compositor from this tree: gpui's `ExternalPaths` is
  `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the platform's drag
  events are unproved here
- **Blocked on hardware:** a microphone recording not proved against real sound reaching a
  microphone

## Later: Sources and providers
- **Blocked on the services:** the Bandcamp and Discogs links an `Identity` carries are read by
  nothing; no provider asks either
- **Blocked on the servers:** Subsonic matches a recording id or ISRC but not the release-track id
  the inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the
  release track

## Later: The vault
- A cancel is not heard while a row is read back, decoded for dedup, or kept as it came (`pcm_of`
  and the `copied` loop take no `Halt`): a multi-gigabyte DSD keep runs to its end
- A cover's lossless JXL is read back from memory, not the staged file; a 16-bit PNG is quantised to
  8 bits, an animated picture keeps its first frame and an ICC profile is dropped (compared against
  the same lossy decode, so it passes)
- A folder sync failing after the rename reports the keep failed though the object landed,
  unreferenced until `--prune`
- The key is the PCM alone: two tracks of the same samples at another rate, count or mask (digital
  silence) share an object, and a standing object is taken on its size alone; needs a migration
- A JPEG cover is kept byte for byte where its lossless JXL is larger, never smaller than it: a
  byte-exact JPEG transcode (`jxl-encoder` 0.3, AGPL, opt-in) is an unproven spike
- Blanking a Matroska file stops at an element of unknown size or after 65 536 elements and lands
  what it blanked so far as stripped; a `TrackEntry` name, chapters and an MP4's track-level `meta`
  are never blanked; an MP4 past 4 096 boxes keeps every tag
- **Blocked on `flacenc`:** `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at
  96 kHz and the depth at 24 bits: a 24-bit rip loses to `flac -8` by ~15 % and is kept; a 192 kHz
  rip is never a FLAC

## Later: Equaliser and DSP extras
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position, or unplaced channels in turn: a stream's own downmix coefficients
  are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- A partly timed LRC drops its untimed lines (section heads, credits, translations) unsaid: placing
  one needs a moment that neither lights it nor ends the timed line beside it early
- **Blocked on gpui:** the sung line cannot grow as it lights: gpui 0.2.2 on Linux draws a glyph on
  a whole pixel vertically (`SUBPIXEL_VARIANTS_Y = 1`) and cosmic-text hints every size, so a type
  size in motion shimmers and hops; the line only brightens
- **Blocked on the format:** where a sheet gives a line no end (a plain LRC never; only an enhanced
  one with a closing stamp, or a Lyricsfile), when it goes out is guessed from its text length: a
  held note can be put out under the dots early
- **Blocked on the sources:** `Lyrics` holds no translation beside the original. Only an LRC line
  marked `[v1:]` or `[v2:]` names its singer; elsewhere a second voice is read off overlapping lines
  alone and a third is folded onto the two

## Later: Listen and recognition
- A capture stream failing without the daemon going (`StateChanged` to `Failed`) is never reopened:
  `was_lost` reads only a closed channel, so an unplugged microphone records nothing till the
  deadline; a reopened capture splices onto the clip with no gap mark, and a failing stop discards
  a full clip
- **Blocked on Shazam:** reached through an undocumented endpoint (`amp.shazam.com`); a change on
  its side stops recognition

## Later: MCP
- An edit a model makes is not on the window's *Undo*: undo stacks live in the process that made it

## Later: Packaging
- **Blocked on gpui:** gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc; `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
