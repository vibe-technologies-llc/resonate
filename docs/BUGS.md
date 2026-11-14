# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · 0.11.1] `ArtworkProvider` can leave a cover's pipe unanswered for good: `openFile` hands back the
  read end at once and calls `artworkJpeg` outside its `try`, which catches only `IOException`, so a
  `CancellationException` (`EngineClosed` from `PollPace.kt`) or a `holding` that waits on an engine
  nobody `retain`s (a host asking for covers while no engine is open) never closes the write end:
  the reader blocks and a coroutine and a descriptor leak per request
- [S · 0.11.1] A Kotlin callback that throws panics the Rust thread that called it: UniFFI 0.32.2
  panics on a failed callback with no `Result` (`Documents::open_document`, `OutputMixer`,
  `PlayerWatch::moved`, `Logbook`, `DocumentRewriting`), these run on the decoder, scan, output and
  watch threads where no `catch_unwind` stands, and release builds unwind; `OpenedFiles.descriptor`
  catches only `FileNotFoundException` and `OutputFormats.kt`'s `deviceOf` and
  `bitPerfectAttributes` run outside their `try`, so an unexpected exception silently ends an output
  or listening thread

## Security
- [M · 0.11.1] A scan or a tag write finishes a `.resonate-torn` or `.resonate-whole` copy found in
  `/var/tmp` or `/tmp` whoever made it: `spool::spooled_under` mends in shared folders,
  `journal::mend_what_was_left_in` checks no owner, and the track it writes over is the path the
  copy's own trailer names (`bound_of`, a hash anyone can compute), so another local user can have
  the listener's next scan overwrite any file the listener may write with bytes of their choosing
  (the page record passes too, since the planter hashes the readable target). Stage and mend under a
  per-user `0700` folder, and finish only a copy whose owner is the effective uid; Windows and macOS
  temp folders are the user's own
- [S · 0.11.1] The iOS folder URI check passes a `/` percent-encoded inside a segment:
  `FolderFiling.components(of:)` splits on `/` before decoding and refuses only a segment that is
  exactly `..`, so `a%2F..%2F..%2Fx` decodes to a climbing path that `Folders.resolved`'s
  `appendingPathComponent` takes whole, and `open`, `delete` and `fileNumber` then reach outside the
  granted folder; `documents::location` checks only the scheme. No reachable hostile URI was found,
  but one from an imported list or another device would do it
- [S · 0.12.0] A server can still make a device dial an address, against `sync.md`: the sealed punch
  candidates (`wire.rs`) carry addresses alone, no sender, target or time, and
  `service::punched_candidates` puts the server's `address` first once any old blob opens, so a
  server, or anyone holding an earlier blob, replays it under any device with any address and the
  device sends QUIC Initials there. Seal the asker, the target and a coarse time, or correct the rule
- [S · 0.12.0] A nearby `Beacon` opens for ever and is answered to whatever source it came from: it
  holds no time or counter, and `beacon_heard` puts the datagram's source first among the candidates,
  answers it by unicast and dials it for up to a minute, so one beacon heard on the network and
  replayed with a forged source makes every device of the space send datagrams and Initials to an
  address of the replayer's choosing
- [M · 0.12.0] The server bounds the frames it reads per session, not across them: `read_frame_at_most`
  allocates the announced length (up to `LARGEST_FRAME`, 32 MiB) before any body arrives and
  `reading.rs`'s `Budget` is made per session, so 2 048 connections hold tens of GiB in frame
  buffers with no ceiling but the machine's memory; one shared byte semaphore taken before the
  allocation, refusing with `Busy`, would bound it
- [S · 0.12.0] A `ws://` sync server on another host sends the space's token in the clear:
  `web::web_address` takes `ws` for any host and invitation links can name one, so anyone on the
  path learns a token that may `Append`, `Compact` or `Close`; the hifi secret is refused off
  `https://` or loopback, sync has no such guard
- [S · 0.12.0] A peer's `FromPeer::Sending` size is believed: `peer.rs` stages the file under
  `sync-staging` up to whatever size it announced, with no ceiling like the vault's
  `LARGEST_DELIVERY`, so a paired device (an old phone never forgotten) can fill the disc
- [S · 0.12.0] A renderer's control or description URL keeps any control characters:
  `located::parse` and `join` keep the path as given and `http::request` writes it into the request
  line, so a CR or LF a description encodes injects headers into the hand-written HTTP (to the
  renderer's own host alone, which `is_on_host_of` pins)
- [S · 0.12.0] The window's start-up lock falls to a predictable name in the shared temp folder where
  `XDG_RUNTIME_DIR` is unset (`starting::lock_path`, `resonate-starting-<uid>.lock`, opened with no
  `O_NOFOLLOW` or owner check), so another user creating it first loses the single window or makes
  each launch wait `WAITS_AT_MOST`; it wants the per-user `0700` folder check `socket.rs`'s
  `kept_to_this_user` makes
- [S · 0.12.0] Subsonic's token salt is predictable (`subsonic/src/lib.rs` `salt`: an MD5 of the
  time, a counter and the pid), where the scheme assumes a random one; draw it from the OS
- [S · 0.12.0] `forwarded_by` is honoured whoever connects: `serve.rs` skips the per-address `admit`
  and reads the counted address from the configured header, so a web listener bound beyond loopback
  (`server.md` says to bind it there; nothing holds it) lets every client choose the address its
  limits count. Refuse a non-loopback `listen` with `forwarded_by`, or honour the header only from a
  loopback peer
- [S · 0.12.0] `documents::owned` is a safe function around `File::from_raw_fd` that refuses nothing,
  though `mobile.md` says it refuses a negative descriptor: every caller checks first today
  (`downloads.rs`'s `fresh_destination` checks inside), so the one approved unsafe call relies on
  each caller remembering

## Sync and together

## Playback and output
- [S · 0.11.1] A refused seek still takes the gapless handover back: `Engine::seek` (and `SeekBy` in
  `commands.rs`) calls `take_the_boundary_back` before `SeekOutOfRange`, `NotSeekable` and the seek
  to where it already is, so a refused seek in a song's last ring of frames plays the next song's
  opening from the ring, drains, and starts that song again from its first frame while the bus still
  names the old one; `tests/transport.rs` tries neither refusal
- [S · 0.12.0] WASAPI and Core Audio keep a replaced ring until the next reshape:
  `Handing::let_go_of_the_spent` is called only inside the reshaper closure (`serving.rs` in both
  crates), never from the service loop as PipeWire's heartbeat does, so the old consumer and its
  buffer (up to twice `LARGEST_RING`) stay allocated until another reshape or the stream closes
- [S · 0.12.0] A packed 24-bit PipeWire quantum over 8 192 frames may be filled short: `process.rs`'s
  `widened` scratch is sized for `QUANTUM_LIMIT_FRAMES` and the fill takes `wanted.min` of it, with the
  shortfall counted as no starvation; a graph whose `quantum-limit` is raised and whose buffer is
  sized exactly for packed words would starve each quantum (unconfirmed: whether such a buffer is
  handed out is unread)

## Catalog
- [S · 0.11.1] A scan of a walked root forgets the listens of the rows it prunes: `store::prune` deletes
  unreached rows with no `deleted::keep_listens_as_unheld` first, so `listens` cascade away, while the
  tidy of a root *not* walked (`forget_paths`) keeps them; a file deleted or moved out by hand under a
  walked root (every root, for a bare `resonate scan`) loses its plays from Statistics, Wrapped and
  unsent scrobbles and has nothing credited back if it returns, against *Every forgetting keeps the
  listens* (`library.md`)
- [S · 0.11.1] `forget_delivered` deletes the delivery's row with no `keep_listens_as_unheld`
  (`db/roots.rs`), so forgetting a delivery that was played takes its plays from the totals
- [M · 0.11.1] A folder or file renamed only in case on a case-folding volume (APFS, NTFS, a casefold
  folder) loses its rows' history: `moves::follow_the_moved` takes a row as gone only where
  `path.exists()` is false, which the old spelling never is there, so the new spelling's rows are
  added, the old ones go unpaired and the prune takes plays, favourites, enrichment, the vault link
  and studies; organise handles case (`listed_as_named`), the scan's move pairing does not
- [S · 0.11.1] Undoing a playlist edit after organise or a followed move puts the old paths back:
  `organise::files_moved` rewrites `playlist_entries` of playlists still holding the path but neither
  `offered_step_rows` nor the steps in memory (`undo.rs`'s `Held.rows`), so a row removed, then
  organised, then undone comes back under a path no file or row carries
- [S · 0.11.1] A deletion is noted in `songs_deleted` before anything is trashed, in a transaction of
  its own (`db/roots.rs` `remove_tracks`, `note_before_removing`), so a file the trash refused
  (`Removal::Kept`) is still marked deleted here and never synced back, as is every file of a run
  that fails before the rows go; `library.md` puts the mark in the transaction that drops the rows
- [S · 0.11.1] The write-time mend of a dead writer's whole or torn copy finishes it over whichever
  track is being written: `mend_what_a_dead_writer_left` matches a cut name by prefix
  (`Kept::names`) and calls `finished_writing_back` with the track in hand, where
  `mend_a_cut_short_write` resolves the track through `track_named_by` and leaves an ambiguous
  prefix `Unplaced` (`rust-style.md`); two tracks in a folder sharing a cut name lose the one torn
  copy that could mend the first, or, with no page record, have its bytes laid over the second
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

## Formats
- [S · 0.11.1] A Matroska `Tags` or `SeekHead` followed by a large `Void` makes a retag allocate and
  write that many zeros: `matroska_tags::void_of` builds a Void's whole body (`void.resize(bytes)`)
  for every room `room_of` counts, rebuilt on every turn of `laid_in_the_room`, so a sparse file with
  a multi-gigabyte Void aborts the process and an ordinary large one pushes an in-place write past
  `MOST_BYTES_WRITTEN_IN_PLACE` for nothing; a Void's header alone needs writing
- [S · 0.12.0] A truncated WAV whose ID3v2 tag stands before its `RIFF` may have the tag laid inside its
  audio: `writing::chunked` copies what the `RIFF` size declares, which stops short at the file's
  end, then appends the `id3 ` chunk within the `data` chunk's declared span, played as noise and
  never read as a tag; whether lofty refuses such a copy before the rename is unchecked
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

## The window
- [S · 0.11.1] Enter on a row of a search-narrowed playlist marks the whole playlist as playing:
  `acting.rs` passes `true` for `whole` under `Shift::Playlist` where every pointer path passes
  `!narrowed`, so the queue's *Continue playing* heading, the playlists' badge and the playlist's
  last-played stamp name a list of which only the matching rows were queued
- [S · 0.12.0] A queue or playlist row menu acts on the index it was opened at: `queue.rs` captures
  the row's position when the menu is built and *Play* and *Take out of the queue* send it by
  position, guarded by the revision at the press rather than the one the menu was built from, so a
  queue moved underneath (a party request, a device driving the zone, a handover, a download playing
  on arrival) has the wrong song jumped to or taken out; the playlist row menu removes by position
  the same way
- [S · 0.12.0] An analysis of no frames reports its progress as NaN: `resonate-analysis`'s
  `watch.rs` divides by the length with no zero guard and `clamp` lets NaN through

## The binary
- [M · 0.12.0] The window's pipe and socket keep a client's thread after its player stops serving:
  `Answering::serve` reads `closed` only after a frame arrives and `listening.rs` runs each client on
  a detached thread that `rest` never wakes or joins, so a connected client (a long `resonate mcp`
  session) holds a thread and the zone's `Shared` (player and host) until it next speaks or leaves
- [S · 0.12.0] The settings writer replaces a dangling `config.toml` symlink instead of following
  it: `config::edit` falls back to the link's own path where `canonicalize` fails, so a dotfile link
  to a file not yet made becomes a plain file on the first write, against *Every writer follows a
  symlinked `config.toml`* (`binary.md`)
- [S · 0.12.0] The MusicBrainz copy follows default settings when the file will not read again after
  a write: `settings.rs` re-reads with `config::load(..).unwrap_or_default()` for the mirror switch
  and the replication token, so a hand-edit's syntax error there starts or stops the copy as if
  under defaults, silently; `musicbrainz.rs` falls back to the config already held

## Equaliser
- [S · 0.12.0] A profile name Windows or a FAT volume refuses is taken as usable:
  `ProfileName::new` refuses only `/`, `\`, NUL, control characters, `.` and `..`, and `path_of`
  writes `<name>.txt`, so a name holding `: * ? " < > |`, a trailing dot or space or a device name
  (an AutoEq label through `ProfileName::after`) fails the save as a store error, and two names apart
  only in case are one file on macOS and Windows (unconfirmed which labels reach it)
