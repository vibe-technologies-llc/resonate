# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · 0.13.1] Next from the notification, a headset or Android Auto at the last row is dropped by
  Media3 before it reaches `RustMediaPlayer.handleSeek` (repeat off or repeat-track: it reads no
  next item in the slice it was shown), where the player screen's button and the desktop finish
  the queue or wrap under skip-repeats-queue; Media3's skip methods cannot be overridden on a
  `SimpleBasePlayer`, so the session's player wants a `ForwardingPlayer` sending them to `next`

## Security
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address

## Sync and together

## Playback and output

## Catalog
- [S · blocked: CAP_LEASE] Lengthening in place a track another account owns (one this account may
  write through its group) still cannot see a reader root runs: the kernel grants a write lease only
  to the file's owner or a holder of `CAP_LEASE`, so such a track falls back to `held_open_elsewhere`
  (`insert.rs`), which counts an unreadable `/proc/<pid>/fd` as holding the file except for a
  process whose `status` names root

## Providers and network
- [S · 0.13.1] Phone scrobbling can stop for good: `online_tools.rs` submits up to 100 listens in
  one call (Last.fm takes 50), with no guard against an empty title or artist and no one-by-one
  retry of a malformed batch, and moves the mark only on success, so one bad listen holds every
  later one back; the desktop's `scrobble.rs` does all three (`android.md` still says batches of
  100)

## The window
- [S · 0.13.1] A new toast withdraws the one standing at once (`toast.rs`'s `tell`), so a trouble
  toast can be gone before it is read, against `ui.md`'s floor of 2 s and 10 s for trouble
- [S · 0.13.1] A failed move of playback to another zone shows the zone it came from
  (`views/zones.rs`'s `move_playback_to`) even where the listener has since turned to another
- [S · 0.13.1] A lyric nudge on the playing song is lost or flickers when the lyrics are asked
  again for the same track: `read_how_far_ahead` (`lyrics.rs`) sets the offset to zero before the
  kept one is read back
- [S · 0.13.1] A command to a running player over the Windows pipe or the Mac's socket that times
  out may still land: `exchange` (`resonate-mpris`'s `peer.rs`) gives up after `ANSWERS_WITHIN`, but
  the `converse` thread has already written the request and stays blocked reading its answer, so
  the terminal says the player did not answer and the track skips anyway, and the thread and its
  stream are kept

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

## Windows
- [M · 0.13.1] A playlist exported on Windows names its songs by odd absolute URIs: `sheet.rs`'s
  `reads_back_as_itself` is false for any row with a `\`, so no row is written relative, and
  `escaped` writes a verbatim path (`\\?\C:\…`) as `file:////%3F/C%3A/…`; `strip_prefix` fails
  between a verbatim and a plain prefix too

## macOS

## Equaliser
