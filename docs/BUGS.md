# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · 0.11.1] A Kotlin callback that throws panics the Rust thread that called it: UniFFI 0.32.2
  panics on a failed callback with no `Result` (`Documents::open_document`, `OutputMixer`,
  `PlayerWatch::moved`, `Logbook`, `DocumentRewriting`), these run on the decoder, scan, output and
  watch threads where no `catch_unwind` stands, and release builds unwind; `OpenedFiles.descriptor`
  catches only `FileNotFoundException` and `OutputFormats.kt`'s `deviceOf` and
  `bitPerfectAttributes` run outside their `try`, so an unexpected exception silently ends an output
  or listening thread

## Security
- [S · 0.12.0] A Bonjour record still opens for ever: `record_of` seals a bare `Beacon` (no time,
  resealed only as it moves, and iOS hands the same record every 5 s), so one record seen on the
  network and advertised again under addresses of the replayer's choosing makes every device of the
  space dial them for `HEARD_FOR`; a broadcast datagram is held to `BEACON_SENT_WITHIN` and heard
  once (`heard_in`), a record wants the same with resealing every window and a cache that tells the
  same record resolved again from a replay

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
- [S · 0.12.0] The window's pipe and socket leave a connected client's thread blocked after the
  player stops serving: `listening::serve_while_held` lets go of the zone's `Shared` at once, but the
  thread sits in `read_frame` until the client next speaks or leaves (a Windows named pipe has no
  cross-thread wake; a socket could be shut down from `rest`)

## Equaliser
