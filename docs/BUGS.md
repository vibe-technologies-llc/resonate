# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Sync and sharing
- [M · 0.7.1] The server reads a session's frames faster than it handles them: each session and
  room spawns `read_frames` into an `unbounded_channel`, the frame cap, record limit and store work
  run in the select loop, and while that loop waits on a write to a client that reads nothing the
  reader keeps draining the stream, so a client with one space of its own appends a few MiB, reconnects
  from the start and stops reading, then floods `Relaying` and `Relay` frames (neither counts toward
  `frames_per_minute`, a zero-length datagram costs the byte bucket nothing and adds nothing to the
  target's backlog) and server memory grows at its upload rate while QUIC keep-alives hold the
  connection; the hub's one-mebibyte bound for what a client does not read has no counterpart on the
  way in (a bounded channel for the reader, a write timeout, and a cost for every relayed frame)

## Network devices
- [S · 0.7.1] A UPnP renderer or Cast receiver can abort the player with one number: `clock_of`
  in `resonate-upnp` parses `RelTime`'s seconds as an `f64`, refuses only the non-finite and
  negative, and hands it to `Duration::from_secs_f64`, which panics from about 1.8e19 (`0:00:1e30`),
  as an `hours` near the limit overflows its `+`; `resonate-cast`'s `currentTime` reaches
  `from_secs_f64` the same way (`Json::number` takes `1e30`); both streams then add a constant to
  `heard` (`+ HEARD_TO_THE_SECOND`, `+ HEARD_WITHIN`), which overflows near `Duration::MAX`; the
  release profile is `panic = "abort"`, so a device on the network that answers a position poll with
  an oversized figure ends the process (`try_from_secs_f64`, `checked_add` and `saturating_add`, as
  the AcoustID, LRCLIB and TIDAL readers already do)

## Downloads
- [S · 0.7.1] A MusicBrainz export whose download is whole but unverified can never resume: a
  cancel during the checksum (`summed` → `looked_at`), a kill before the rename, or one between
  `rename(partial, whole)` and the `.verified` write leaves a complete `.part` (the next run renames
  a `whole` with no marker back to `partial`), and `downloaded` then asks `Range: bytes=<length>-`
  at the end of the file, which a server answers 416 and `Ranged` turns into `Refused(416)`, so every
  later build and background `keep_fresh` fails until `resonate musicbrainz --remove` (the Subsonic,
  Monochrome and hifi readers treat 412 and 416 as start over; here a partial at or past the listed
  size should go straight to verification, else start again)
- [S · 0.7.1] A hifi-api server that answers 206 to a request with no `Range` makes
  `relayed_download` call itself without end: with nothing held a 206 matches neither the `200`
  arm nor `continuing_held`, `ranged` is `None` and the fall-through is `relayed_download(…, None)`,
  the same request again, until the stack overflows and the release build aborts (Subsonic and
  Monochrome read any 2xx through `Ranged::continuing`)

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
