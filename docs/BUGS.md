# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

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

## Playback
- [S · 0.2.1] A seek back while the end-of-track sleep timer is inside its fade window
  (`SLEEP_FADES_OVER`, 10 s before the end) lifts the fade and begins it again once `quiet_within`
  (40 ms at least) has passed; where the new ring has not been heard by then (a slow decode after the
  seek, a starved engine thread), `fade_out` meets a consumer at `SILENT` after its discard, which
  hushes without taking frames, so the position never moves and the timer never fires: silence while
  `Playing`. `a_seek_back_under_an_end_of_track_timer_lifts_the_fade_it_had_begun` seeks from outside
  the window to stay clear of it
