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
- [S · 0.18.1] Stage `history --export` before it replaces the target. **Medium, builder.**
  `File::create` truncates the target before the listens are read
  (`crates/resonate/src/history.rs:34-59`), so a failure leaves a partial file in place of whatever
  was there. Write to a held stage and rename. Done when a failing export leaves the old file byte
  for byte.
- [S · 0.18.1] Fold a recording key's artist and album as its title is folded. **Low, builder.**
  `crates/resonate-core/src/recorded.rs:30-38`, `:56-69` word-normalise the title but keep the
  artist's and album's punctuation and spacing, so one recording keys apart under `Guns N' Roses`
  and `Guns N Roses`. Fold all three alike. Done when both spellings key the same.
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
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader

## macOS
- [S · blocked: a Mac] The desktop capture reads the aggregate's first input stream, never checked
  to be the tap: `Heard::from` takes `streams(Direction::Input).next()`
  (`crates/resonate-listen/src/coreaudio_capture.rs:62-81`) of the aggregate over the default output
  and the tap (`crates/resonate-apple/src/process_tap.rs:137-170`), so an output with inputs of its
  own, a USB headset, may record its microphone when the listener chose the desktop and send it to a
  recogniser; settling it needs a Mac with such an output

## Windows
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
