# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android

## Security

## Sync and together

## Playback and output

## Catalog
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

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

## The binary

## Equaliser
- [M · 0.12.0] A listening test of lossy restoration is level-matched only where its arms' chains
  begin on some frames of the song: over one MP3 of steady tones the extended arm settles from
  0.00 to 0.09 dB louder than `levels_of` measured from the song's start, by the frame its chain
  was built at (a whole number of 1 024-frame blocks in matched; 11 025 frames in, it stayed
  0.09 dB loud to the end), so a test begun mid-song can be heard unmatched and an ABX told apart
  by level; `restoration_on_and_off_are_heard_level_matched_and_switched_without_a_gap` begins its
  audition on a whole block for it
