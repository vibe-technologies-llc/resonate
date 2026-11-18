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
- [S · 0.13.1] An invitation link given to `resonate` while a window is open reaches the window
  spelt otherwise: `window.rs`'s `queued_in` turns it into a `MediaLocation`, whose `to_uri`
  escapes `?`, `=`, `:` and `#`, so a link naming `?server=` loses its server and an
  `https://<host>/s/#CODE` page link no longer reads as an invitation, and the bus's `add_tracks`
  (`interfaces.rs`) then fails the whole call, the files beside it unqueued
- [S · 0.13.1] A party guest whose song cannot start mid-way loses its own queue to the host:
  `Seat::take_up` (`resonate-engine/src/seat.rs`) sends `Resume` before it learns the row has no
  rate and answers `false`, `party/seat.rs` then leaves `rows` as they were, and `added_here`
  reads the host's songs now standing in the queue as songs the guest added, takes them out and
  sends each to the host as a request
- [S · 0.13.1] A phone moving its music folder (`organising.rs`'s `documents_moved`) resumes the
  saved queue even where `save_zone` skipped saving (a party being followed, a row still opening),
  so a guest is put back on its pre-party queue mid-party, and a buffering zone is resumed paused

## Playback and output
- [S · 0.13.1] Under a convolution impulse, a sleep timer ending at the end of the track or the queue
  never pauses: `sleep_is_due_in` (`dozing.rs`) sizes the fade by `position`, which takes off only
  the convolver's latency (`frames_it_holds`), while its flush hands the ring the whole response
  past the music, so the fade reaches silence with that tail unread, the quiet ring is pulled
  without being consumed and the track never ends; the zone shows playing all night and holds the
  idle inhibitor
- [S · 0.13.1] A gapless turn to a source of a much lower rate can leave the stream silent while it
  shows playing: `keeps_its_stream_for` (`binding.rs`) weighs the ring's capacity with the leaving
  chain's block, so the stream is kept though the entering chain's block (a high resampling ratio:
  8 kHz onto 96 kHz at a 100 ms buffer) is larger than the ring, and `fill` waits for room it can
  never have
- [S · 0.13.1] The UPnP media server and the renderer's stream stop for good on one unexpected
  accept error: `door.rs` ends its thread on anything but `WouldBlock` and `Interrupted`
  (`ECONNABORTED`, `ECONNRESET`, `EMFILE`), and `announce.rs` ends the SSDP loop on an error it does
  not list (Windows's `WSAEMSGSIZE`)
- [S · 0.13.1] A renderer that sends `HEAD` while it plays ends its own stream: `feed.rs` counts a
  `HEAD` as a connection, which supersedes the `GET` that is pouring
- [S · 0.13.1] Switching the catalog on the network off and on again within a quarter second can
  hide it for ten minutes: `Offered::share` (`offering.rs`) starts the new server at once while the
  old one's announcer has yet to hear its stop, so the old `ssdp:byebye` lands after the new
  `ssdp:alive` under the one UDN and control points forget a server that is running; `rename`
  already waits through `restarting`
- [S · 0.13.1] A playlist offered on the network that names a song twice pages wrongly:
  `played_from` (`offering.rs`) takes each track out of the map it read (`read.remove`), so a
  repeat on the page is dropped while `total` counts it, and a control point paging by
  `NumberReturned` sees rows twice and others never
- [S · 0.13.1] A seek on a row played in place from a service is dropped on the phones: `served.rs`
  builds such rows with a rate of 0 (`..Track::default()`), and `jump_to_at` (`lib.rs`) turns the
  position into frames by that rate, as does a seek asked while the row opens

## Catalog
- [S · 0.13.1] Reading a ListenBrainz export whose `listened_at` is huge panics:
  `listen_lines.rs` adds it to `UNIX_EPOCH` unchecked
- [S · 0.13.1] The first retag after a crash fails on the track a dead writer left: `FileTags::write`
  (`writing.rs`) stamps the file and reads its tags before `landed` mends it
  (`mend_what_a_dead_writer_left`), so the mend moves the stamp and the write is refused
  `ChangedWhileWritten`; the RIFF rechunk path never mends at all
- [S · 0.13.1] A phone's retag of a song with no title or artist tag writes the document's name,
  extension and all, as its title and an empty artist (`tagging.rs`'s `retagged`), where the desktop
  takes `named_by_the_file`
- [S · 0.13.1] The phones' statistics count albums by `album || album_artist` and artists by their
  raw spelling (`catalog_extras.rs`), so one album or one artist spelled two ways is counted twice,
  where the desktop counts by the folded key and `album_key`
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
- [S · 0.13.1] A WAVE whose ID3 tag stands in front of the RIFF header and whose RIFF size is odd
  has the tag moved into an `id3 ` chunk at an odd offset (`writing.rs`'s `chunked`), where neither
  `riff.rs` nor lofty finds it, so the retag loses every tag
- [S · 0.13.1] An SYLT stamp with a huge seconds field overflows (`sylt.rs`'s `stamped` multiplies
  unchecked beside a checked minutes term): a debug build panics writing such lyrics, a release
  build wraps and falls back to a plain `USLT`
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
