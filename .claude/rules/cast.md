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
`check` that its normal edges hold no PipeWire, gpui, `ureq`, serde or `serde_json`) and is reached
by the binary and `resonate-mobile` alone (the job's loop over `cargo tree -i`); the engine and
codec are dev-dependencies, for the end-to-end tests.

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
  across runs; where the receiver was given by address and the browse has not heard it, the id is
  not known and the name is the address's (*A receiver may be given by its address or its name*). Its `SinkInfo` is `is_hardware`, never default, `Plugged::Unsaid`, with
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
- **A receiver may be given by its address or its name** (`Casting::locate(impl Into<Given>)`,
  `unlocate`, `located`; the binary's `cast-at`). `Given` is `Address(SocketAddr)` or `Named { host:
  Host, port }`: `Given::parse` takes an IP, `ip:port`, `[v6]`, `[v6]:port`, a host name or
  `host:port` (port 8009 when none), a host being 1 to 253 bytes of letters, digits, `-` and `_` in
  labels of at most 63, lowercased, whose last label is not all digits (so `192.168.1.300` is
  refused rather than looked up) and anything else `None`. **A name is resolved when it is reached,
  never when it is held** (`Given::resolve`, `Host::resolve`): `to_socket_addrs` on the
  `resonate-cast-browse` thread, the IPv4 answer preferred, and, only under `Finding::Bonjour`
  and for a name ending in `.local`, a short-lived `ServiceDaemon::resolve_hostname` (3 s) where
  the system resolver lacks mDNS; a name nothing resolves is `Error::Resolve { host, source }`,
  logged at debug and tried again with the next round. `locate` on a casting already reaching
  spawns a `resonate-cast-resolve` thread for a name (an address is reached on the caller's
  thread, as before), so no window, audio or UI thread blocks on a resolver. The address a name
  last resolved to is kept (`Located::resolved`) and is what counts as held; a name that moves
  drops the receiver at its old address unless another given or the browse holds it, and
  reaches the new one. The mDNS `id` is not on the wire (`GET_STATUS` does not carry it), so a
  receiver reached by address is first named by where it is, `CastId::at` (`at-192-168-1-20-8009`,
  so the sink `cast:at-192-168-1-20-8009`) and `Found::at` titles it with the IP.
  `Casting::look_at_addresses` starts the stream server and a `resonate-cast-browse` thread with
  no daemon, which reaches every held given at once and again every `ADDRESSES_TRIED_AGAIN_EVERY`
  (60 s), so a receiver powered off then appears when it is on and a name is looked up again; a
  held given is never dropped for being quiet. `look` on such a casting starts the browse beside
  it, `stop_browsing` stops the browse and drops the receivers only it heard (or rests where
  nothing is held), `is_looking` is the browse and `is_reaching` either.
- **One receiver, one sink, named by its mDNS id once heard.** An address is skipped where a
  receiver is already held at it. A record the browse resolves (or a `Finding::Listed` one) at the
  address of a receiver reached by address, whatever name the record gives, **renames that sink
  to `cast:<id>` under the same `SinkId`** (`Shared::relabel`, `Device::renamed`, told as
  `SinkChange::Reformatted`; the title becomes the record's `fn`), so a `sink` key naming the
  mDNS id finds it while the address is held, and the `SinkInfo` keeps the address's name in
  `aliases` for as long as the address is held (`Casting::sinks`, `Device::address_node_name`),
  so a stored `cast:at-...` still finds it (`SinkInfo::is_named`, `audio.md`). Which sinks were
  reached by address is `Known::addressed`, and the browse's record names (`Known::named`, a
  fullname to a `CastId`; a `Listed` receiver is named by its id) say which are heard. A record
  removed from the browse **keeps** an addressed receiver's name and its alias while the address
  is held (nothing is told; a name that went to a window's binding must not be lost to a browse
  that fell quiet) and removes any other; forgetting the address removes a receiver only the address
  held (`forget_at`: addressed and not heard) and leaves one the browse hears, its alias gone.
  A browse that stops keeps the name it gave. A receiver the browse listed first and an address
  names later is one sink under the id. Locks and turns key by address (`Lanes`, the volume
  links), not by id, so a rename does not split a receiver's lane. The browse's record is not
  asked again to prove the rename: the address was reached already. **A name stored elsewhere by
  node name follows the alias** (`audio.md`, `eq.md`): the engine resolves an equaliser binding
  through every name the sink answers to, and the window carries a binding, an own curve and a
  measurer stored under `cast:at-...` to `cast:<id>` once the sink is listed so. The rename lasts as long
  as the address is held, past the browse hearing the record or stopping, so a binding stored under
  `cast:<id>` keeps finding the receiver; a record heard again under the same id is the same name
  and sink, told nothing.
  (`a_receiver_given_by_address_is_reached_without_a_browse_and_named_by_where_it_is`,
  `a_receiver_given_by_host_name_is_resolved_reached_and_named_by_where_it_is`,
  `a_host_name_given_to_a_reaching_casting_is_resolved_off_the_callers_thread_and_forgetting_it_removes_the_receiver`,
  `a_receiver_given_by_address_and_then_heard_by_the_browse_is_named_by_its_mdns_id_under_the_same_sink`,
  `an_address_given_to_a_receiver_already_listed_makes_no_second_sink`,
  `a_casting_that_stops_browsing_keeps_the_receivers_given_by_address_and_drops_the_rest`,
  and in `casting.rs` `a_receiver_the_browse_stops_hearing_keeps_the_name_it_was_given_while_its_address_is_held`,
  `a_receiver_the_browse_stopped_hearing_is_heard_again_under_the_same_name_and_sink`,
  `a_receiver_the_browse_stopped_hearing_goes_with_its_address`,
  `a_receiver_only_the_browse_held_goes_when_the_browse_stops_hearing_it`).
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
  `GET_STATUS`'s `currentTime` (fractional seconds, through `Duration::try_from_secs_f64`, so an oversized figure is no reading) against the frames served is the delay
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

## On the phones

`resonate-mobile` holds one `Casting` beside its one `Network` (`rendering::Kinds`, resting from the
start, `Searching::on_the_network`), and `output::beside` composes them exactly as the binary does,
`Beside::over(oboe or remote_io, upnp).and(cast)`, so a receiver is one more sink to every zone
with the queue, handover, DSP, clock and volume it has on the desktop. A receiver is listed in the
zones sheet beside the renderers: `renderers()` answers a `NetworkRenderer` per renderer and per
receiver, its `kind` (`RendererKind::Upnp` or `Cast`) telling them apart, and a receiver's `node`
is its current `cast:` name (`Casting::sinks`), its `host` the address it is reached at.
`select_zone_renderer` takes either kind, and a node that is an alias of a listed receiver (a zone
kept on `cast:at-...` once the browse has named it) is kept under its current name.

- **`look_for_renderers(on)` is one switch for both searches**: SSDP and the mDNS browse, each
  started or stopped (`stop_multicasting`, `stop_browsing`) and each failing apart
  (`RendererSearch`, `ReceiverSearch`; the second is attempted whichever the first answered).
  Android holds the `MulticastLock` it holds for SSDP across the browse, mDNS being multicast too.
- **A receiver by address or name** (`add_receiver_address`, `remove_receiver_address`,
  `receiver_addresses`) is `Casting::locate` with `Given::parse` (`ReceiverAddress` where the text
  is neither an address nor a name), starting `look_at_addresses` when nothing reaches yet, so no
  mDNS is needed; a receiver that could not be reached stays held, and the call answers
  `ReceiverUnreached` once a casting is reaching (the first address is reached on the thread
  `look_at_addresses` starts, as a renderer's is). Each phone keeps the list (`receiver-addresses`) beside `renderer-addresses`
  and adds it again at start, and the zones sheet has an *Add a receiver by address* row beside
  *Add a renderer by address* (`mobile.md`'s *Zones*).
- **An iPhone signed for free browses nothing**: the mDNS browse is multicast from the library's own
  socket, which needs `com.apple.developer.networking.multicast`, so only the address route works
  there, as for a renderer (`ios.md`, `FEATURES.md`). The browse's failure is logged and the
  addresses are still reached.

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
carries the song from where it landed without launching again. A receiver given by a host name is resolved and reached (`localhost` and the fake's port),
listed under its address's name, added and forgotten with its name, and a name nothing resolves lists
nothing and is kept; a receiver given by address and then heard by the browse is renamed under its
own sink id with the address's name as an alias, told `Reformatted`, and stays listed under its id when
the address is forgotten. A record advertised through a
second `mdns-sd` daemon is browsed, reached and listed, and removed once withdrawn
(`a_receiver_advertised_over_mdns_is_found_and_gone_once_withdrawn`, skipped where that daemon
cannot resolve its own record, which is a host with no multicast). The unit tests hold the protobuf
bytes, the JSON reader's escapes, surrogates and depth limit, the mDNS record's reading and the
media status entries. The phones' side is proved on the host in
`crates/resonate-mobile/src/rendering.rs` (`a_receiver_given_by_address_is_held_with_no_search_and_forgotten`,
`a_receiver_address_that_is_neither_an_address_nor_a_name_is_refused_and_a_name_is_held`,
`one_switch_looks_for_renderers_and_receivers_and_turning_it_off_keeps_the_addresses`), the stream
itself by this crate's fake. No real receiver has been played to, from a desktop or a phone
(`docs/TESTING.md`).
