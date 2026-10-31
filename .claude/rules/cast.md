---
paths:
  - "crates/resonate-cast/**"
  - "crates/resonate/src/rendering.rs"
---

# Google Cast receivers

`resonate-cast` plays to a Google Cast receiver on the local network (a Chromecast, a Chromecast
Audio, a Nest or Home speaker, a speaker group, a TV with Cast built in): it finds one over mDNS,
speaks Cast v2 to it and has the Default Media Receiver load an address the player serves. It
reaches core, `resonate-audio` and `resonate-upnp` alone (the `layering` job's `only` line, and a
`check` that its normal edges hold no PipeWire, gpui, `ureq`, serde or `serde_json`); the engine
and codec are dev-dependencies, for the end-to-end tests.

## One more sink kind beside UPnP's

- **A receiver is a sink, never a zone kind, for the reasons a UPnP renderer is** (`upnp.md`'s *A
  renderer is a sink, not a zone kind*): the engine's queue, gapless handover, DSP, ring, volume
  and position stay on the far side of `Backend`, and a zone binds a receiver as it binds any
  device. `Casting` implements `resonate-upnp`'s `Neighbours`, the seam `Beside` composes, so the
  binary's backend is `Beside::over(pipewire, upnp).and(cast)` and both kinds share one sink
  stream, survey and opener.
- **Ids from `RECEIVER_IDS_FROM` (`0xE000_0000`) up to UPnP's `0xF000_0000`** (`Casting::owns`),
  above any PipeWire global and apart from the renderers'. The node name is `cast:` and the
  receiver's mDNS `id` (`CAST_NODES`, `Device::node_name`), so a `sink` key or `--sink` names one
  across runs. Its `SinkInfo` is `is_hardware`, never default, `Plugged::Unsaid`, with
  `HardwareVolume::Always` and the volume read at discovery (`RECEIVER_STATUS`'s `volume.level`).
- **The stream reuses UPnP's, unchanged**: `resonate-upnp`'s `Server` (one per `Casting`) offers a
  `Feed` under a token address, and the feed is the same paced live FLAC, encoded bit for bit from
  the ring, pulled no faster than the wall clock and forgiving a stalled reader's debt. That is why
  the crate reaches `resonate-upnp`: the paced feed and its FLAC encoder are the hard part and
  belong to no protocol. The address handed over is the one the system routes toward the receiver
  (`Server::address_toward`).
- **What a receiver takes is fixed, not asked**: Cast has no protocol-info call, and the Default
  Media Receiver's published support is FLAC up to 96 kHz and 24 bits. `Device::carriage` is
  `Wrapping::Flac` at 44.1, 48, 88.2 and 96 kHz, S16 and S24, stereo alone; the engine resamples
  above and folds surround down, as for a UPnP renderer that states nothing.

## Finding

- **mDNS browse of `_googlecast._tcp.local.`** (`CAST_SERVICE`) through `mdns-sd`, its own daemon
  thread and a `resonate-cast-browse` thread taking its events. A resolved record is a receiver
  where its TXT `id` is 1 to 64 letters, digits or dashes and its `ca` capabilities, where stated,
  hold audio out (bit `0x04`); it is named by `fn`, else `md`, else the id, and reached at its IPv4
  address where it has one and the record's port (a group answers on a port of its own). A record
  removed (`ServiceRemoved`, or its TTL run out) removes the sink. A record seen again at the same
  address is not reached again.
- **A record is reached before it is listed** (`Device::reached`): a TLS connection and a receiver
  `GET_STATUS`, which reads the volume and proves it speaks Cast; one that fails is left out.
- **`Finding::Listed` reaches a fixed list instead of browsing**, which is how the tests name a
  receiver on the loopback without multicast. `Searching::within` (2 s) is how long
  `FirstSearch::Awaited` collects records before `Casting::start` answers.
- **A receiver may be given by its address** (`Casting::locate(SocketAddr)`, `unlocate`, `located`;
  the binary's `cast-at`): its mDNS `id` is not on the wire (`GET_STATUS` does not carry it), so
  the receiver is named by where it is, `CastId::at` (`at-192-168-1-20-8009`, so the sink
  `cast:at-192-168-1-20-8009`) and `Found::at` titles it with the IP. `Casting::look_at_addresses`
  starts the stream server and a `resonate-cast-browse` thread with no daemon, which reaches every
  held address at once and again every `ADDRESSES_TRIED_AGAIN_EVERY` (60 s), so a receiver
  powered off then appears when it is on; a held address is never dropped for being quiet.
  `look` on such a casting starts the browse beside it, `stop_browsing` stops the browse and
  drops the receivers only it heard (or rests where no address is held), `is_looking` is the browse
  and `is_reaching` either. **One receiver, one sink:** an address is skipped where a receiver is
  already held at it, and a browsed record at a held address is skipped, so a receiver the mDNS
  browse names and an address names is listed once, under the name the address gives it; adding an
  address for a receiver the browse already lists changes nothing
  (`a_receiver_given_by_address_is_reached_without_a_browse_and_named_by_where_it_is`,
  `an_address_given_to_a_receiver_already_listed_makes_no_second_sink`,
  `a_casting_that_stops_browsing_keeps_the_receivers_given_by_address_and_drops_the_rest`).
  No Cast receiver has been addressed so (`TESTING.md`'s Cast entry).
- **`Casting` rests like `Network`**: `resting` runs only the `resonate-cast-turns` thread;
  `look` starts the stream server once, the daemon and the browse thread; `rest` stops browsing,
  shuts the daemon down and forgets every receiver, each told `SinkChange::Removed`.

## The wire, hand-written

- **TLS to the receiver's port (8009 for a device) over rustls on `ring`, with TLS 1.2 kept for
  older receivers.** A receiver presents a self-signed certificate of its own, so the chain is not
  checked (`TheReceiversOwn`), only the handshake's signature against the certificate offered.
  The TLS hides the stream's address and the commands from a passive listener; it proves nothing
  about who answers, since Cast's own device authentication (`deviceauth`, a challenge signed by
  Google's chain) is optional for a sender and not spoken.
- **A `CastMessage` is protobuf, written and read by hand** (`frame.rs`): a four-byte big-endian
  length, then `protocol_version` `CASTV2_1_0`, `source_id`, `destination_id`, `namespace`,
  `payload_type` and `payload_utf8` (or `payload_binary`); unknown fields are stepped over, a
  message over `MESSAGE_BYTES_AT_MOST` (64 KiB) ends the link. No protobuf crate: seven fields do
  not earn a code generator.
- **The payloads are JSON, written and read by hand** (`json.rs`, depth at most 32), because the
  binary without `online` must not link `serde_json` (`refuse_headless`), and receivers are wanted
  there as UPnP renderers are.
- **A `Link` is one TLS connection and its `resonate-cast-link` thread.** It `CONNECT`s to
  `receiver-0` as `sender-0` with `userAgent` `resonate/<version>` and nothing else identifying
  the listener, `PING`s every 5 s, answers the receiver's `PING` with `PONG`, and counts the
  receiver gone after `SILENT_AT_MOST` (16 s) with nothing heard or a `CLOSE` from `receiver-0`.
  An ask carries a fresh `requestId` and waits for the answer bearing it (`ANSWERED_WITHIN`, 5 s;
  `LAUNCH` and `LOAD` 20 s); an answer of a refusing type (`LAUNCH_ERROR`, `LOAD_FAILED`,
  `LOAD_CANCELLED`, `INVALID_REQUEST`, `INVALID_PLAYER_STATE`) is `Error::Refused`. Anything said
  unasked goes to the link's `told` queue, bounded and dropping when full.

## A stream

- **`Neighbours::open` connects, nothing more**: the spec must be one the carriage carries
  (`FormatRefused` otherwise, before any connection), a `Link` is opened (so an unreachable
  receiver fails the open), the feed is offered and the stream answers with `FormatChanged`
  already told and `Downstream::Seconds`, so a seek, pause or change of what is rendered is
  handled as for a UPnP renderer (`upnp.md`'s *Past the renderer's buffer*). Its commands go to
  its own `resonate-cast-stream` thread.
- **The first `SetActive(true)` launches and loads**: a receiver `GET_STATUS` takes the Default
  Media Receiver (`CC1AD845`) where it already runs, else `LAUNCH` starts it; the stream connects
  to its `transportId` and sends a media `LOAD` with the address as `contentId`, `audio/flac`,
  `streamType` `LIVE`, `MusicTrackMediaMetadata` (type 3) titled with the request's media name, and
  `autoplay`. The load waits for its play, so the feed is already pulled when the receiver fetches.
  Loading at open instead would have the receiver fetch a feed nobody pulls and wait on it.
  Later `SetActive(true)` is `PLAY`, `SetActive(false)` `PAUSE` (heard at once; the receiver
  resumes its own buffer and the ring continues it), both naming the `mediaSessionId` the load
  answered.
- **One stream steers a receiver at a time** (`Lanes`, one lane per receiver id, as UPnP's
  `Holders`): a new stream takes the lane at open, and a stream no longer holding it sends nothing
  and reports no failure. A new `LOAD` replaces what the receiver plays, dropping its buffer, so no
  `STOP` is needed before it; a stream closing while it holds the lane sends media `STOP` for its
  own session. The receiver application is never stopped, so the next stream needs no launch.
- **The receiver's clock is read**: every `STATUS_ASKED_EVERY` (500 ms) while streaming, a media
  `GET_STATUS`'s `currentTime` (fractional seconds) against the frames served is the delay
  published through `Feed::heard_through`; `STATUS_FAILURES_AT_MOST` (6) unanswered in a row is
  the receiver gone. The same `MEDIA_STATUS` arriving unasked is read the same way.
- **A drain is told once**: after `Drain` the feed marks the end, and `Drained` is sent when the
  receiver reports the session `IDLE` with `idleReason` `FINISHED`, or `currentTime` within
  `HEARD_WITHIN` (250 ms) of the end, or the session gone once the feed has ended. The session
  `IDLE` for any other reason, a `CLOSE` from its transport, a dead link or a refused `LOAD` is
  the stream failed (`StreamEvent::Refused` for a refused launch, load or play), so the engine
  waits for the device as for any sink gone.
- **The volume is always the receiver's**: `SET_VOLUME` with `volume.level` (the `Volume` as is)
  or `volume.muted` on the `resonate-cast-turns` thread, which keeps a `Link` per receiver and
  sends only the last of the turns queued for each, so a dragged slider costs one round trip per
  wake, not one per step.

## In the binary

`rendering::Rendering` holds both kinds (`Kinds`): the `renderers` key, `sinks --renderers` and a
sink named `upnp:` or `cast:` (`names_a_renderer`) look for both, the two first searches run side
by side, and the settings pane's one switch looks or rests for both. `sinks` lists receivers with
*the network* as what drives them.

## Proof

`crates/resonate-cast/tests/receiver.rs` runs a fake receiver on loopback: TLS under an `rcgen`
certificate, framing and JSON written in the test apart from the crate's, a `PING` sent at each
connection, the receiver and media namespaces answered as the Default Media Receiver does, and the
loaded address fetched and recorded, its playing time counted on the wall clock and reported
`IDLE`/`FINISHED` once the fetch has ended. It holds a listed receiver reached and offered as a
stereo FLAC sink up to 96 kHz with its volume (`a_receiver_listed_is_reached_and_offered_as_a_sink`),
the asks in order, the `LOAD`'s fields, the fetched FLAC decoding to the samples poured, a drain
told, a pause, the volume and the mute turned, the receiver's ping answered and a stop on close
(`a_stream_to_a_receiver_launches_it_loads_flac_by_address_and_is_paused_drained_and_stopped_its_pings_answered`), a
refused load told as refused, a rate the receiver cannot take refused before it is reached, a song
played through a real `Player` arriving bit for bit, a pause and a volume change reaching it within
400 ms and the song resuming bit for bit, and a seek loading a fresh address within 400 ms that
carries the song from where it landed without launching again. A record advertised through a
second `mdns-sd` daemon is browsed, reached and listed, and removed once withdrawn
(`a_receiver_advertised_over_mdns_is_found_and_gone_once_withdrawn`, skipped where that daemon
cannot resolve its own record, which is a host with no multicast). The unit tests hold the protobuf
bytes, the JSON reader's escapes, surrogates and depth limit, the mDNS record's reading and the
media status entries. No real receiver has been played to (`docs/TESTING.md`).
