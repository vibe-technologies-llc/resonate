# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Sync
- [M · 0.8.1] A device whose songs were deleted as a sync began fetched every song twice once
  another device spread them again (three devices holding everything, two cleared, one used as the
  source): the deletion was not yet registered when the pass decided what to fetch. Not reproduced
  yet by a loopback run of a phone syncing from two senders (clearing outside the app, deleting in
  the app, restarting the sync mid-transfer, a fresh launch); still to try is a delete landing while
  a pass is underway on each deletion path, and whether the double is two files (`Name (1)`), two
  rows for one file, or only the fetching count

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

## The window
- [S · 0.8.1] An album cell's artist cut by an ellipsis leaves a gap before the `·` and the year
  (`browser.rs` `album_cell_captioned`): the link keeps the room it was given while the cut text
  ends well short of it, so *Björk Ásgeirsdóttir…* stands some twenty pixels off its year
- [S · 0.8.1] A hint can come up over a sheet its own control opened: a press on the header's
  microphone with the pointer left at rest shows *Listen for a song…* above the Listen sheet's
  scrim until the pointer next moves, gpui scheduling the hint from the move it replays after the
  press and never weighing the scrim that then covers the control
