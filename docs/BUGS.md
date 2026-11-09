# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Library and files
- [S · 0.7.1] Queuing a folder follows symlinked directories with no memory of where it has
  been: `folder_items` splits entries with `Path::is_dir`, which follows links, and recurses into
  each, bounded only by `FOLDERS_DEEP` (32), so a link to the folder or an ancestor
  (`~/Music/Backup -> ..`, a sync tool's link) queues every song about 33 times, and two such links
  branch at every level (2^32 walks, a hang and unbounded memory before the window opens); the
  library scan's `Visited::first_time_through` and `Walking::reaches_a_root` have no counterpart
  here, no test has a link loop, and a link to a sibling folder queues its songs twice
- [S · 0.7.1] A trashed file's `DeletionDate` is wrong on the two days a clock changes:
  `trash::deletion_date` reads the time of day as the time elapsed since `Calendar::midnight_of`,
  which is the wall clock only on a 24-hour day, so on the day clocks go forward everything after the
  gap is stamped an hour early and on the day they go back a deletion between 23:00 and 24:00 is
  written `T24:30:00`, which is no `.trashinfo` date and which a strict trash viewer or restore tool
  shows undated or refuses (`Calendar::minute_of_the_day` already honours the offset at that moment;
  the seconds are the epoch's modulo 60)

## Tags in the files
- [M · 0.7.1] A tag write that outgrows its head lengthens the file under a decoder that has it
  open: `Saving::lengthened_in_place` and `insert::lengthened` open whole filesystem blocks at
  offset 0 of the live inode with `FALLOC_FL_INSERT_RANGE` (ext4, XFS, F2FS; FLAC, MP3 and Ogg heads),
  so every byte behind the head moves forward by the inserted length, and a decoder reading the
  playing track from its own handle takes its next refill from `shifted` bytes earlier (repeated
  audio, lost frame sync, FLAC CRC errors, a seek table and first-frame offset cached at open now
  wrong) until the track is opened again; the staged-copy landing renames a new inode and leaves
  the old one to its readers, and `taken.still_stands` guards only against another writer; first
  find whether a tag write can reach the song playing at all (neither `audio.md` nor `library.md`
  says), then keep the lengthening for a file nothing holds open or take the staged copy
- [S · 0.7.1] Adding a picture to a chained Ogg file renumbers the later links' pages:
  `ogg::renumbered` rewrites the sequence number and checksum of every consecutive page after the
  header with no look at the serial, from `laid_head` and `copied_with_head` whenever the header's
  page count changes (a cover added to the comment header), so a second link, whose pages restart at
  0, and any stream interleaved with the first, take the first's counter and a libogg-based player
  reports a hole at each link; the vault's renumbering keeps to `renumbering.serial`, and
  `ogg::chain` and the decoder's link following say chained Opus and Vorbis are read

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
