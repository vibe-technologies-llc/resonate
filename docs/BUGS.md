# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [M · blocked: design] A device that joins a shared playlist after a hostile editor compacted it
  takes the forged member keys from the snapshot: the first key heard for a `MemberId` stands, so
  devices that already held the list keep the true keys, but a newcomer's first is the forger's, and
  a removal it later makes seals the other members' seats to them; closing it needs `Member` ops
  signed by the member's own key, a change of the op format
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address


## Playback and output

## Catalog
- [S · 0.18.1] Report organise's refusals after `--undo --apply`. **Medium, builder.**
  `organising.rs:43` returns "nothing to put back" when the plan's moves are empty after applying,
  though refusals or a cancel emptied them (`crates/resonate-library/src/organise.rs:820`, `:2350`,
  `:2375`), so the refusals go unprinted and a cancel exits 0. Print what was refused and exit
  non-zero on a cancel. Done when a test with a refused move prints it and exits non-zero.
- [S · 0.18.1] Stage `history --export` before it replaces the target. **Medium, builder.**
  `File::create` truncates the target before the listens are read
  (`crates/resonate/src/history.rs:34-59`), so a failure leaves a partial file in place of whatever
  was there. Write to a held stage and rename. Done when a failing export leaves the old file byte
  for byte.
- [S · 0.18.1] Fold a recording key's artist and album as its title is folded. **Low, builder.**
  `crates/resonate-core/src/recorded.rs:30-38`, `:56-69` word-normalise the title but keep the
  artist's and album's punctuation and spacing, so one recording keys apart under `Guns N' Roses`
  and `Guns N Roses`. Fold all three alike. Done when both spellings key the same.
- [S · 0.18.1] Make a bare `resonate served` ask which service. **Low, builder.** Online, a bare
  `served` always picks dlna (`crates/resonate/src/providers.rs:125-133`, `served.rs:496-498`), so
  `NoServedService` never fires. Pick only a configured service. Done when a test with none
  configured gets `NoServedService`.
- [S · 0.18.1] Refuse `playlist --export` over a file that is not a playlist. **Low, builder.**
  `PlaylistFormat::of` defaults to M3U (`crates/resonate/src/model.rs:474-476`,
  `playlist.rs:161-173`), so `--export notes.txt` writes M3U over it. Refuse an unknown extension.
  Done when exporting to `.txt` is refused and the file is untouched.
- [S · 0.18.1] Refuse a folder or unreadable sheet in `playlist --add`. **Low, builder.** A folder
  or a non-audio file is stored as an unplayable row, and an unreadable cue sheet makes an empty
  playlist with exit 0 (`crates/resonate/src/playlist.rs:313-348`). Refuse each with its reason.
  Done when each case exits non-zero and stores nothing.
- [S · 0.18.1] Exit non-zero when `dedupe --apply` could not trash. **Low, builder.** A trash
  refusal still exits 0 (`crates/resonate/src/tagging.rs:183-198`). Count refusals into the exit.
  Done when a refused trash exits non-zero in a test.
- [S · 0.18.1] Name an album exactly in `favourites --add`. **Low, builder.** `favourites --add
  NAME` goes through the search grammar with no album disambiguator
  (`crates/resonate/src/favourites.rs:74-124`), so the wrong one of two same-named albums is marked.
  Take an artist with the album, or refuse an ambiguous name. Done when two same-named albums refuse
  a bare name.
- [S · 0.18.1] Keep a run with another config from clearing the window's scrobble mark. **Low,
  builder.** `open_library_with` stops telling any service whose token this run's config lacks
  (`crates/resonate/src/main.rs:676`, `:690-711`), so a run with `--config` naming another file
  against the shared catalog deletes the window's mark
  (`crates/resonate-library/src/scrobble.rs:610-628`) and its unsent listens are skipped. Stop only
  on an explicit sign-out. Done when a test run with a token-less config leaves the mark.
- [S · 0.18.1] Hold listen submission to one process. **Low, builder.** The window and `resonate
  play` both submit from one catalog with no hold between reading the mark and `mark_through`
  (`crates/resonate/src/submitting.rs:139-170`, `crates/resonate-library/src/scrobble.rs:291-320`),
  so listens are sent twice. Take a held claim around a submission round. Done when two submitters
  in a test send each listen once.
- [S · 0.18.1] Check an imported listen's stamp from a service. **Low, builder.**
  `crates/resonate-online/src/history.rs:340-346` adds a service's `u64` seconds to `UNIX_EPOCH`
  unchecked, and `:210`'s `earliest + 1` overflows, so one bad stamp aborts the import. Check both
  and pass the listen over. Done when a page with `u64::MAX` imports the rest.
- [S · 0.18.1] Check MusicBrainz's track offset. **Low, builder.**
  `crates/resonate-online/src/musicbrainz.rs:1321` adds one to a server's `track_offset` unchecked.
  Use a checked add and pass the medium over. Done when an answer with `u32::MAX` is read without a
  panic.
- [S · 0.18.1] Correct `forget_folder`'s description. **Low, builder.** MCP says listens and
  playlist rows go (`crates/resonate-mcp/src/tools.rs:401-402`), but `forgetting.rs:30` keeps
  listens and playlist entries survive. Say what is kept. Done when the description matches the
  tool's test.
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Formats
- [S · 0.18.1] Keep a SYLT line led by a bracket text when it is written as LRC. **Low, builder.**
  `crates/resonate-codec/src/sylt.rs` writes timed words as LRC without `escaped_for_lrc`, so a
  line opening on `[offset:…]` or a stamp reads back as a tag, as Better Lyrics' did. Escape it the
  same way. Done when such a line round-trips, in a test.
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS
- [S · 0.18.1] Restore a Mac output's settings loudly, and let the hog go when verification fails.
  **Low, builder.** `Restoring::drop` ignores every restore error with no log
  (`crates/resonate-coreaudio/src/opening.rs:129-141`), and `Hog::taken` keeps the hog when the read
  that verifies it fails after the toggle (`crates/resonate-apple/src/hog.rs:42-46`). Log each
  failure and release on a failed verification. Done when the toolbox fakes show both.
- [S · 0.18.1] Count a Mac capture slice that was dropped. **Low, builder.** A slice above
  `FRAMES_A_SLICE_AT_MOST` (8192) or a failed pull is answered silently
  (`crates/resonate-apple/src/buffers.rs:67-76`), so a recording is silence and reads
  `NothingHeard`. Count drops and fail a sitting that heard only drops. Done when a fake capture of
  16 384-frame slices records them.
- [S · 0.18.1] Guard the path monitor and artwork handlers against a panic. **Low, builder.** The
  path monitor's handler (`crates/resonate-apple/src/path_monitor.rs:69`) and the artwork handler
  (`now_playing.rs:204`) are not run through `guarded`, so a checked build unwinds into libdispatch,
  which `macos.md` says no handler does. Wrap both. Done when both handlers go through `guarded`.
- [S · 0.18.1] Join a lost Mac capture's thread before reopening. **Low, builder.** The Mac
  `Capture` has no `Drop` (`crates/resonate-listen/src/coreaudio_capture.rs:106-110`, `:183-192`),
  so a lost capture's last `carry` races the reopened recorder's `silence_for` (`session.rs:79-105`,
  `recording.rs:64-90`), whose `filled` is a load then a store. Join on drop as
  `wasapi_capture.rs:115-122` does. Done when a test drops a capture and opens another on the same
  recording.
- [S · blocked: a Mac] The desktop capture reads the aggregate's first input stream, never checked
  to be the tap: `Heard::from` takes `streams(Direction::Input).next()`
  (`crates/resonate-listen/src/coreaudio_capture.rs:62-81`) of the aggregate over the default output
  and the tap (`crates/resonate-apple/src/process_tap.rs:137-170`), so an output with inputs of its
  own, a USB headset, may record its microphone when the listener chose the desktop and send it to a
  recogniser; settling it needs a Mac with such an output

## Windows
- [S · 0.18.1] Keep the old window icons until the new ones are worn. **Low, builder.**
  `WindowIcons::wear` destroys the replaced icons after a `WM_SETICON` that timed out
  (`crates/resonate-win32/src/icons.rs:61-69`, `:151-168`), so a hung window keeps a destroyed icon.
  Destroy only once every window answered. Done when the fake's timed-out send keeps the old pair
  alive.
- [S · 0.18.1] Check the stop before starting a late WASAPI capture, and pad a discontinuity. **Low,
  builder.** A capture whose open timed out is detached and later starts the client before seeing
  `stop` (`crates/resonate-listen/src/wasapi_capture.rs:92-95`, `:251-277`), flashing the microphone
  indicator; and `drained` ignores `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY` (`:279-305`), so a stall
  loses time from the clip silently. Check `stop` before `start_stream`, and pad the elapsed time on
  a discontinuity. Done when tests with a fake show both.
- [S · 0.18.1] Bound a WASAPI survey by the time the engine gives it. **Low, builder.**
  `Survey::enumerate_sinks` ignores its timeout (`crates/resonate-wasapi/src/backend.rs:166-173`),
  and the engine surveys in line at start, with no stream open and in `select_sink`
  (`crates/resonate-engine/src/engine.rs:741`, `engine/sinks.rs:47`, `:251`), so an endpoint whose
  `Activate` or `IsFormatSupported` stalls freezes the engine thread, and `shutdown` joins the
  watcher unbounded (`backend.rs:204-211`); PipeWire honours the timeout. Survey on a worker and
  answer the last list past the timeout. Done when a fake survey that never returns leaves
  `enumerate_sinks` answering within its timeout.
- [S · 0.18.1] Type a WASAPI open's failures and publish latency after the write. **Low, builder.**
  `device_for` and `get_iaudioclient` failures become `Backend { op: Open }`
  (`crates/resonate-wasapi/src/device.rs:222-234`), which the engine bills to the row, so a DAC
  unplugged between survey and open fails the row instead of waiting; `initialize_error` maps only
  `UnsupportedFormat`, which `Initialize` never raises (`stream.rs:80-122`, `:174-179`), and there
  is no retry on `AUDCLNT_E_BUFFER_SIZE_NOT_ALIGNED`; and the latency noted each cycle leaves out
  the frames that cycle writes (`render.rs:57-77`, `:89-108`), about one period low. Map a vanished
  or held endpoint to `SinkGone` or `StreamRefused`, retry at the aligned size, and note latency
  after the write. Done when the serving fakes show an unplug at open waiting for the device and the
  latency matching the queue.

## Command line
