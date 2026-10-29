---
paths:
  - "crates/resonate-upnp/**"
  - "crates/resonate/src/rendering.rs"
  - "crates/resonate/src/zoning.rs"
---

# Network renderers (UPnP AV)

`resonate-upnp` plays to a UPnP AV / DLNA MediaRenderer on the local network (a streamer, an
amplifier, upmpdcli in front of an OpenHome player): it finds one, describes it, offers it a stream
and steers it. It reaches core and `resonate-audio` alone (the `layering` job's `only` line, and a
`check` that its normal edges hold no PipeWire, gpui, `ureq`, serde or rustls); the engine is a
dev-dependency, for the end-to-end test.

## A renderer is a sink, not a zone kind

- **`Beside` is a `resonate-audio::Backend` that wraps another and adds the renderers to it.** Its
  surveyor answers the wrapped backend's sinks and then every renderer known (`Network::sinks`); its
  opener sends a request whose `target` a renderer owns (`Network::owns`: ids from
  `RENDERER_IDS_FROM`, `0xF000_0000`, above any PipeWire global) to `Network::open` and the rest to
  the wrapped one; sink changes from both arrive on one channel (a `resonate-upnp-sinks` thread
  selecting over the two). A survey failing in the wrapped backend fails whole, renderers and all:
  answering the renderers alone made a lost graph look like a list whose first sink was a renderer,
  which the engine then chose.
- **Why a backend and not a zone driven by URL:** the engine's queue, gapless handover, DSP, ring,
  volume and position all live on the far side of `Backend`, so a renderer as one more sink keeps
  every one of them, and a zone of the window binds one as it binds any device (`Command::SetSink`,
  `--sink`, the settings pane's device list). Handing the renderer the source file's URL instead
  would bypass all of that and give it a timeline of its own. What it costs: the renderer holds its
  own clock and buffer, so a seek, pause or volume change in the ring reaches the ear only after
  what the renderer already holds (`FEATURES.md`).
- **A renderer's node name is `upnp:` and its UDN** (`RENDERER_NODES`, `Renderer::node_name`), so a
  `sink` key or `--sink` names it across runs and replugs as a PipeWire `node.name` does; its
  `SinkInfo` is `is_hardware`, never default, `Plugged::Unsaid`, with `HardwareVolume::Yes` where it
  has RenderingControl (so `device-volume` sends `SetVolume`/`SetMute`, on a `resonate-upnp-turns`
  thread so the engine never waits on the network) and the volume `GetVolume` read at discovery.

## Finding and describing

- **SSDP `M-SEARCH` for `urn:schemas-upnp-org:device:MediaRenderer:1`**, sent twice to
  `239.255.255.250:1900` with two hops and `MX` the wait in whole seconds (1 to 5); every `200`
  answer whose `ST` names a MediaRenderer gives its `LOCATION` (`ssdp::search`, deduplicated). The
  address searched is `Searching::to`, so tests answer on loopback and nothing reaches the network.
- **The description is read as the device wrote it**: `URLBase` where present, else the description's
  own address, joins every `controlURL` (absolute, rooted or relative to the description's folder:
  `Located::join`); the renderer is the `device` (nested in a `deviceList` or not) whose `deviceType`
  names a MediaRenderer, else the first holding an AVTransport service. XML through `roxmltree`
  with no DTD and `NODES_AT_MOST` nodes; HTTP answers capped at `ANSWER_BYTES_AT_MOST` (1 MiB).
- **What it takes decides how it is fed** (`Carriage`, from `ConnectionManager`'s
  `GetProtocolInfo` `Sink`): `audio/wav` (or `x-wav`, `wave`, `vnd.wave`) is `Carriage::Wave`, offered
  S16 and S24 stereo at 44.1 to 192 kHz; `audio/L16` alone is `Carriage::L16` at the rates it lists
  (44.1 and 48 kHz where none), S16 stereo; neither is `Error::TakesNoPcm` and the device is not
  listed. A renderer with no ConnectionManager, or one that will not answer it, is offered WAVE.
- **Searched again every `Searching::every` (60 s)**; a renderer missing from
  `SEARCHES_MISSED_BEFORE_GONE` (2) searches in a row is `SinkChange::Removed`, a new UDN `Added`,
  a known UDN at a new address replaced under its id. `FirstSearch::Awaited` searches before
  `Network::start` answers, so a sink named at start is in the engine's first survey.

## HTTP, both ways, hand-written over `httparse`

A renderer speaks plain HTTP/1.1 on the local network, so no TLS and no `ureq`: the binary's
`refuse_headless` line keeps `ureq` out of a build without `online`, and renderers are wanted there
too. `http::exchange` sends `Connection: close` and `User-Agent: resonate/<version>` (nothing else
identifies the listener), reads a declared, chunked or close-delimited body, and gives up after
`ANSWERED_WITHIN` (3 s). SOAP (`soap::call`) posts an envelope to the control address with
`SOAPAction: "<service type>#<action>"`, every argument escaped; a `500` is
`Error::Fault { action, code }` from the body's `errorCode`.

## The stream

- **The renderer pulls a live WAVE (or L16) the player serves, paced at the ring's real time.** One
  `Server` per `Network` (`resonate-upnp-serve`, a `TcpListener` on `Searching::serving_on`, all
  addresses by default) offers each stream under `/resonate/<token>.<wav|pcm>`, the token a
  `RandomState` hash so another machine cannot guess one; the address handed to the renderer is the
  local address the system routes toward it (`address_toward`). `Network::open` refuses a spec the
  carriage does not carry (`FormatRefused`), offers the `Feed`, sends `SetAVTransportURI` with a
  DIDL-Lite item naming the title and `protocolInfo`, and answers a `SinkStream` with a
  `FormatChanged` already told (S24 is `Words::Packed`: three bytes on the wire).
- **A `Feed` is the ring's consumer behind an HTTP answer** (`resonate-upnp-feed`, one thread a
  fetch): a WAVE header with no length (`RIFF` and `data` sizes near `u32::MAX`), then quanta of a
  hundredth of a second pulled through `fill_stamped` no faster than the wall clock, the 24-bit ring
  word cut to three bytes and L16 turned big-endian (`wave::carried`). Pacing is the graph's job
  everywhere else and the ring pads an underrun with silence, so pulling at network speed would
  drain the ring and write that silence into the music. Where the renderer stops reading (its buffer
  full, a slower clock) writes block and the debt past one quantum is forgiven, so the renderer's
  clock governs and the pull never bursts to catch up. A second fetch of the same stream takes the
  source over and the first ends (renderers sniff an address before playing it). Inactive, nothing
  is pulled.
- **A stream's commands go to its own `resonate-upnp-stream` thread** (`Watch`), never the
  engine's: `SetActive(true)` is `Play`, `SetActive(false)` `Pause` (only once played: a stopped
  renderer faults a pause), `Close` (or the `SinkStream` dropped) `Stop`, the token withdrawn and the
  feed closed. A refused `Play` is `StreamEvent::Refused` with the UPnP code, anything else failing
  `StateChanged` to `Failed`, so the engine waits for the device as for any sink gone.
- **The renderer's clock is read, not assumed:** every `POSITION_ASKED_EVERY` (500 ms) while it
  streams, `GetPositionInfo`'s `RelTime` against the frames served gives the frames still ahead of
  the ear, published through `StreamClock::note` as a `GraphTime` ticking at the stream's rate,
  which is what the engine's position, the heard frame and the drain fallback read.
  `POSITION_FAILURES_AT_MOST` (6) unanswered in a row is the renderer gone.
- **A drain ends the answer:** after `Drain`, the first short fill marks the frames served as the
  end and closes the connection, and `Drained` is told once `RelTime` is within
  `HEARD_TO_THE_SECOND` of it (renderers count whole seconds); a renderer that never says leaves the
  engine its wall-clock fallback.
- No reshape (`Unreshapable::Unsupported`): a new format is a new address and a new
  `SetAVTransportURI`.

## In the binary

`rendering::Rendering` holds the one `Network` a run uses. It searches only where the `renderers`
key is on (off by default: nothing is multicast unasked), `sinks --renderers` asks, or the sink
wanted names a renderer; the last two wait for the first search. `Rendering::player` is how `play`,
the window and every zone (`zoning::Opener`) build a `Player`: PipeWire inside `Beside` where a
network is held, `Player::with_sources` where not. `sinks` lists the renderers with *the network*
as what drives them.

## Proof

`crates/resonate-upnp/tests/renderer.rs` runs a fake renderer on loopback: an SSDP answer, a
chunked description, SOAP endpoints recording every call, and a fetcher that reads the offered
address once played and answers `RelTime` from what it read. It holds discovery and the sink
offered (`a_renderer_answering_the_search_is_described_and_listed_as_a_sink`), the call order and
escaping, a drain told, the volume turned and the served bytes exactly as written
(`a_stream_on_a_renderer_is_offered_by_address_played_paused_drained_and_stopped_and_fetched_as_written`),
and a WAVE played through a real `Player` reaching the renderer bit for bit
(`a_song_played_through_the_engine_to_a_renderer_reaches_it_bit_for_bit`).
