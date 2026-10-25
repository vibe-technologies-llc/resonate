# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Providers and network
- [M · 0.1.1] The listener's server downloads two songs at once while a poll asks it for three, so
  the third waits on a whole transfer before its first byte: the server should download one song at
  a time (`DOWNLOADS_AT_ONCE` in `resonate-server`'s `gateway.rs` at one, answering 503 with
  `Retry-After` while busy rather than holding the request) and a poll ask the keyed `resonate`
  provider for one want at a time
- [M · 0.1.1] A TIDAL link MusicBrainz holds that the listener's server finds gone or region-locked
  ends that provider's turn: `played::found` offers the linked track without asking its codes, the
  offer opens `Gone`, the next round passes the provider as heard and the want counts a miss though
  another listing under the song's codes may play; the provider should be told which offers failed
  and asked again without them

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
