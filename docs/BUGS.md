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
- [M] An album's `tagged_tracks` is one file's raw `TRACKTOTAL`, a disc's count on a set (this
  build's own writer writes it per disc), weighed against a release's whole count: a 12 + 10 deluxe
  rip misses its 22-track pressing and lands strictly on the 12-track standard edition
- [M] A bind mount of the same filesystem is no volume (`st_dev` alone; `/proc/self/mounts` is read
  in `volumes.rs` but not for this): unmounted, its root reads as files moved out and is pruned
- [S] A file a sheet cuts, followed as it moved (`moves::cuts_moved`), is not refreshed from the rows
  the scan wrote at its new path: a sheet edited as the file moved keeps the gone rows' titles and
  index entries until the sheet's mtime brings a reprobe

## Window
- [M] A key typed into a field arrives through `replace_text_in_range` as `Span::Composing`, which
  joins any next composing edit whatever the selection: undo takes back the whole session, and
  ctrl-a then a letter then undo empties the field with the old text out of reach
- [M] Typing anywhere to search drops text an input method composes until a field is focused; the
  field's preedit skips `one_line` and keeps its mark past the 16 KiB cut; a click can land the
  caret inside a grapheme cluster; selection is one quad across a mixed-direction run

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
