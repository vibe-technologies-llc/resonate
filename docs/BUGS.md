# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Mobile
- [S · 0.7.1] Deleting or folding songs on a phone leaves dead rows in the live queue and then
  stops the queue from being kept: `drop_from_queue_in` sends one `Command::Remove` per run of gone
  rows and stops silently at `QUEUE_EDITS_AT_MOST` (64) runs or at the first error, `forget_deleted_songs`
  and `fold_identical_songs` then delete the `documents` rows, and `save_zone` fails on
  `ids_at` with `UnknownTrack` for any queue row whose document is gone, on every five-second tick and
  at `shutdown`, so after a large delete or fold of songs that sit apart in a queued library the
  queue and position are no longer written, a process death restores a stale queue and one failure
  stops `save` before the zones after it, with a vague toast once and then none (loop until none is
  gone, and let `save_zone` pass over a row the catalog lacks)
- [M · 0.7.1] A refresh of the phones' song list can duplicate rows and crash the list: Android's
  `pagedTracks` and iOS's `Library.pages` read 500 rows at a time by offset against the live
  catalog, `scanDocument` runs between pages without the lock the pages take, a new song sorting
  before the offset moves the last row of one page onto the next and a deletion skips one, and
  `SongsTab`, `AddSongsSheet` and `LibraryResults` key their lazy lists by `track.id`, so Compose
  throws *Key was already used* when the doubled row is composed; a pull-to-refresh, a sync arrival,
  artwork enrichment and `forgetMissing` all refresh while a scan runs, and `pagedFully` pages the
  same way with no lock (the boundary should hand back the revision it read and the pages restart
  when it moves, or slice one snapshot of the ordered ids)

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
