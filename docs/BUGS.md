# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M] An end-of-track or end-of-queue sleep on a DoP stream never fires: `fade_toward_sleep` fades a
  `Silence::Marked` ring, which hushes at once without draining (`ring.rs` `fades` is false), so the
  position freezes ten seconds short, the transport stays Playing all night (idle inhibit held) and
  every Play re-silences after `quiet_within`
- [M] A stream open that fails and parks its row (`land_the_stream` drops the output before `fail`,
  `unbound` unset) parks it at the decoder's position, a ring's depth past what was heard: when the
  device or daemon returns, the first half second of the track (or after a seek) is skipped
- [M] A row whose first bind fails (`NoSink`: headphones not yet connected) returns from `started`
  before `TrackStarted` and before `renegotiations` resets; parked and played later it is never
  announced, so `resonate play a b` with `b` failing exits 1 `NothingPlayed` though `a` played
- [M] Jumping to a queued row while another queued row plays restamps (`let_go_of_what_was_heard` ->
  `rows_changed`) while the chosen row is out of `next`, stamping it as part of `order`: the
  playlist badge and MPRIS `ActivePlaylist` go for that row's length
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
- [M] One DST frame that will not unpack fails the whole row as `Error::Io` rather than playing as a
  hole as every PCM codec's does; a `DSTF` chunk's length is bounded only by the file, so a hostile
  one reads the rest of it into memory on a seek
- [M] A fragmented MP4 sized from `sidx` counts the first top-level `sidx` alone (one a segment in
  DASH files), and only the first non-empty edit-list entry is honoured, so a spliced M4A stops at
  its first edit
- [S] A whole-file lyric sheet beside a cue-cut file is weighed on its title alone: `Wanted` has no
  whole-file length or artist, so its `[length:]` and `[ar:]` are not checked against the file
- [S] A `SYLT` frame stamped in MPEG frames counts from the first frame, not the first sample heard:
  lines land one encoder delay early
- [S] Matroska's trailing `DiscardPadding` is taken from the first `A_OPUS` track, not the track the
  decoder selected, in a file carrying several Opus tracks
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
- [M] A cue sheet the scan cannot read (`read_sheet` answers `None` for an `EIO`, `ESTALE`, a sheet
  read mid-rewrite as empty) claims nothing, so its file is probed whole and rows 2..N are pruned
  with their plays, listens and favourites: against *what the scan could not read, it keeps*
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
- [S] A cross-filesystem organise copy (`copied_whole`) keeps the mtime alone, dropping xattrs and
  ownership; a rewritten sheet or exported playlist (`staged_over`) is created with default mode and
  no folder fsync
- [M] A bind mount of the same filesystem is no volume (`st_dev` alone; `/proc/self/mounts` is read
  in `volumes.rs` but not for this): unmounted, its root reads as files moved out and is pruned
- [M] `listening_detail` cuts and buckets days at the phone's current UTC offset (`day * 86400 - offset`), and
  the phones regroup those instants in the local calendar: under summer time every winter day starts
  at 23:00 the day before, so the 1st of a month counts in the previous month and *Busiest day*
  reads a day early; the phones must pass their zone or their day boundaries across the boundary
- [S] The phones' `statistics` and `listening_detail` count a missing song's counted listens in the
  totals, where `android.md` leaves hidden songs out

## Providers and network
- [M] Last.fm is sent a batch in requests of 50 (`LastFm::submit`) and answers `Err` at the first
  that fails; the library marks nothing on such an error, so the chunks already taken are scrobbled
  again next pass (`library.md` says the mark moves past what was told)
- [S] A device sign-in's cancel is heard only between polls (each request is bounded by the API
  agent's 20 s)
- [M] The inbox takes a file as settled after two quiet seconds of mtime and ctime, no size
  compared: a torrent client writing sporadically delivers half a file, and the refusal burns the
  want's offer
- [M] `Refused` is collapsed into `Unreadable` at the equaliser and lyric seams, so a 429 from
  LRCLIB or GitHub cannot be backed off from
- [S] Threads of one `Client` do not share a backoff until a request gives up: on a 503 four cover
  threads climb their own 2-4-8 s ladders
- [S] A rotated TIDAL refresh token is kept in memory alone where there is no settings path, and a
  failed write only warns: the next run has lost the sign-in
- [S] The hosted hifi and Monochrome services are registered whenever `online` is on, with no opt-in
  of their own; the hifi token request claims another site's `Origin` and `Referer`

## Window
- [M] *Take out of the queue* on a track queued twice sends one `Command::Remove` a row under the
  same stale `queued.revision`: the first lands, the rest are refused `QueueChanged` into the log,
  the toast says one went and *Put back* is hidden behind steps still standing over queued rows
- [S] A playlist row's *Play* reached by keyboard (`play_reached_rows`) leaves shuffle on, where the
  playlist's own Play and its menu's turn it off
- [S] Playing a track row no longer clears `set_playing_playlist`: a playlist mark may stay after a
  track from another listing plays
- [S] More background reads and writes fail into the log alone (`models.rs`: the pressings read,
  cover reads, the play-count and listen-time writes)
- [S] A trouble toast repeated while the same one shows or waits is shown once, with no count
- [M] A key typed into a field arrives through `replace_text_in_range` as `Span::Composing`, which
  joins any next composing edit whatever the selection: undo takes back the whole session, and
  ctrl-a then a letter then undo empties the field with the old text out of reach
- [M] Typing anywhere to search drops text an input method composes until a field is focused; the
  field's preedit skips `one_line` and keeps its mark past the 16 KiB cut; a click can land the
  caret inside a grapheme cluster; selection is one quad across a mixed-direction run

## Command line and bus
- [S] `resonate play` keeps the queue only on the 500 ms sample (`keep_the_queue`, itself throttled
  to `KEPT_EVERY`) and not once more as it leaves: `n` then `q`, or a `resonate queue` just before
  Ctrl-C, resumes on the row before
- [S] A launch handing files to an open window (`handed_to`) drops `--sink`, `--bit-perfect`,
  `--quality`, `--dither`, `--library`, `--config` unsaid and exits 0
- [S] MCP `resources/unsubscribe` runs the subscribe check first, so a playlist discarded or renamed
  since is `UnknownResource` and its URI read every tick for the session
- [S] MCP `show_queue` and the queue resource promise play order and list MPRIS `Tracks`, the
  unshuffled list: with shuffle on a model names the wrong next song
- [S] The Discord socket search lacks `.flatpak/com.discordapp.DiscordPTB/xdg-run`, which
  `discord.md` lists
- [M] `resonate queue --playlist` sends one unchunked `AddTracks`, a catalog lookup a row behind the
  client's 2 s timeout: a large playlist reports failure, lands anyway and a retry doubles it

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
- [M] Android still groups albums and artists in Kotlin; `search_library`, `albums`, `artists`,
  `album_tracks`, `artist_tracks` and `highlights` answer all of it at the boundary and wait to be
  called there. Android opens an artist page by raw `artist == name || albumArtist == name`, so its
  empty *Unknown artist* lists nearly every song
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
