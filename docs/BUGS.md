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
