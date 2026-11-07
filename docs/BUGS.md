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

## Identification
- [M · 0.6.1] *Place on a release…* answers *nothing places it* for every track without a recording
  id: `Library::recording_of` reads `tracks.mbid` alone, and a track wanting placement rarely has
  one. Fall back to `Reference::recordings_of_isrc` on `tracks.isrc`, then `find_recording` on its
  title, artist and length (strict matches only), and offer each release beside the recording it is
  on, since `place_on` lands that recording on the track

## Providers and network
- [S · 0.6.1] A YouTube Music song kept before keeps carried their picture has none: its file holds a
  title, artist and album and no cover, and its `kept_songs` row holds no address to ask for one
  again, so the library draws a disc for it. A Matroska file with no `SeekHead`, or one that already
  holds an `Attachments` element, is left without a picture the same way (`library.md`)
