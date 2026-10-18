# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M] Between `param_changed` and the engine's rebuild (up to a poll tick) the callback writes the
  ring in the requested layout into buffers of the negotiated one; the negotiated layout is rebuilt
  by count (`from_count`), so a 5.0 or 6.1 stream reads as changed and is rebuilt once

## Formats
- [M] A spliced M4A whose edit list jumps in media time (a cut, a reorder) plays only its first
  stretch: the playable window is one `FrameSpan`, so the later edits have nowhere to go
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

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
- [S] A file a sheet cuts, followed as it moved (`moves::cuts_moved`), is not refreshed from the rows
  the scan wrote at its new path: a sheet edited as the file moved keeps the gone rows' titles and
  index entries until the sheet's mtime brings a reprobe

## Window
- [M] Typing anywhere to search drops text an input method composes until a field is focused; the
  field's preedit skips `one_line` and keeps its mark past the 16 KiB cut; a click can land the
  caret inside a grapheme cluster; selection is one quad across a mixed-direction run

## Equaliser and DSP
- [S] A profile of more than 32 filters keeps the ones past 32 in its text, so removing a band
  from it brings the 33rd into play at the next read

## Search
- [M] A song is ranked only among the first 25 recordings MusicBrainz answers, all scored alike for
  a title many artists recorded: *creep* lists Radiohead's 53rd of a hundred covers, and a typo
  (*bohemian rapsody*) lists covers of *Bohemian Rhapsody* without Queen's

## Keyboard and accessibility
- [M] Tab out of an Online or Library field (contact, AcoustID key, AudD or ListenBrainz token, typed
  folder, filing layout) commits nothing and leaves the typed text unsaved: only a press outside
  commits (`leave_contact` and kin answer while the field holds focus), and each `*_given` refocuses
  the window, so a `Blurred` handler needs a variant that settles without taking the caret back
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
