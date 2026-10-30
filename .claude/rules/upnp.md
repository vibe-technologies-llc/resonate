---
paths:
  - "crates/resonate-upnp/**"
  - "crates/resonate/src/rendering.rs"
  - "crates/resonate/src/zoning.rs"
  - "crates/resonate-ui/src/views/settings/output.rs"
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
  own clock and buffer, so whatever is rendered into the ring reaches the ear only after what the
  renderer already holds; a seek, a pause, the volume and a change of what is rendered
  (equaliser, ReplayGain, the listening test) are steered past that buffer (*Past the renderer's
  buffer*).
- **Why not the untouched file:** `Backend::open` is handed an `AudioSource` of the ring's PCM, never
  the file, and everything that names what plays (the queue, the boundary between rows, position,
  sleep, the heard frame) is the engine's reading of that ring. Handing a renderer the file's own
  address where `GetProtocolInfo` lists its format would bit-perfectly play the file, but beside the
  engine rather than through it: the engine would go on pulling a ring nobody hears, and gapless
  rows, the equaliser and the zone handover would silently stop applying. A bit-perfect PCM stream
  reaches a renderer whenever the plan is `BitPerfect`, and where the renderer takes FLAC that PCM is
  encoded on the way out to spare the network (*The stream*), which fits the seam.
- **A renderer's node name is `upnp:` and its UDN** (`RENDERER_NODES`, `Renderer::node_name`), so a
  `sink` key or `--sink` names it across runs and replugs as a PipeWire `node.name` does; its
  `SinkInfo` is `is_hardware`, never default, `Plugged::Unsaid`, with `HardwareVolume::Always` where
  it has RenderingControl or OpenHome's `Volume` and the volume read at discovery.

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
  `GetProtocolInfo` `Sink`; a renderer holds a list of them, `Carriage::accepted_by`, in preference
  order): `audio/flac` (or `x-flac`) is `Wrapping::Flac`, S16 and S24 at 44.1 to 96 kHz where the
  protocol info states no rate, else at the rates it states (`rate=`) however high (*The stream*:
  FLAC above 96 kHz); `audio/wav`
  (or `x-wav`, `wave`, `vnd.wave`) is `Wrapping::Wave`, S16 and S24 at 44.1 to 192 kHz; `audio/L16`
  is `Wrapping::L16` at the rates it lists (44.1 and 48 kHz where none), S16. None of the three is
  `Error::TakesNoCarriage` and the device is not listed. A renderer with no ConnectionManager, or
  one that will not answer it, is offered WAVE. A rate is taken from the protocol info as
  channels are, never guessed: a renderer listing `audio/flac` alone is offered FLAC to 96 kHz and
  the engine resamples what is above to a rate it is offered, a lower-fidelity choice made only
  because the renderer did not say it decodes more, where one that also takes WAVE is sent
  the rate bit for bit as WAVE, and one stating `audio/flac;rate=192000` is sent 192 kHz FLAC.
  The listener can state the rate for a renderer that does not (*A renderer vouched for*). `Renderer::carriage_for(spec)` is the first carriage
  carrying a stream's rate, word and layout, so FLAC wins wherever it can carry what WAVE would,
  and `SinkInfo::formats` is the carriages' formats united.
- **A renderer vouched for is offered FLAC at every rate it does not state.** The `flac-above-96k-for`
  setting (`binary.md`) names renderers (`upnp:` and the UDN, as `sinks --renderers` lists them);
  `Network::vouch(Vouched::flac_at_any_rate(..))` hands the crate their UDNs (`Udn::from_node`), and
  `Renderer::reached` reads the protocol info with `FlacUnsaid::AnyRate` for those, so their
  `audio/flac` entry carries 44.1 to 192 kHz as one stating the rates would, and FLAC wins over WAVE
  there (`carriage_for`). It changes nothing else: stated rates stay as stated, channels are still
  only those stated, WAVE and L16 are untouched, and a renderer not named keeps the 96 kHz limit.
  `vouch` is read when a renderer is described, so it is called before `look` (the binary does) and
  does not re-describe a renderer already found. **Why a listener's word and not an attempt:** a
  renderer that cannot decode a rate usually accepts `SetAVTransportURI` and `Play` and then
  stops, errors or sits silent, with nothing a protocol answer distinguishes from a pause, and the
  sink's formats are fixed when it is listed, before any stream; a try-and-fall-back would put
  silence in front of the listener at the first 192 kHz song of every session and need the engine
  to reopen on another format. The setting is the listener stating what the protocol info does not
  (the same word `rate=` is), visible in the file, and removing the name restores the limit. No
  hardware has confirmed which renderers decode the rate: the proof is a fake renderer's, not a
  streamer's.
- **Channels are offered where the protocol info states them, never guessed.** A `channels=N`
  parameter on any entry of a wrapping (1 to 8) adds that count's layouts (`layouts_of`: the named
  layout and, where the count has a different unnamed one, `Discrete(n)`); a wrapping naming none
  is stereo alone, so a renderer that says nothing about channels is folded down by the engine's
  matrix as before rather than sent six channels it may refuse (a refused stream fails the sink).
  A stereo-only soundbar behind `audio/wav` therefore never sees 5.1, and an AV receiver that
  lists `audio/flac;channels=6` is sent all six. The ring's channel order is the order of the
  layouts' positions, which is both FLAC's order for 1 to 8 channels and ascending
  `WAVE_FORMAT_EXTENSIBLE` mask bits, so no channel is moved on the way out.
- **A `Network` may rest.** `Network::resting` binds nothing and sends nothing (only the
  `resonate-upnp-turns` thread runs); `Network::look` starts the stream server once and a
  `resonate-upnp-search` thread, `Network::rest` stops searching and forgets every renderer, each
  told `SinkChange::Removed`. `Network::start` is `resting` and `look` together. So the window can
  hold a network behind every zone's `Beside` from the start and the settings pane switch it.
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

- **The renderer pulls a live FLAC, WAVE or L16 the player serves, paced at the ring's real time.** One
  `Server` per `Network` (`resonate-upnp-serve`, a `TcpListener` on `Searching::serving_on`, all
  addresses by default) offers each stream under `/resonate/<token>.<flac|wav|pcm>`, the token a
  `RandomState` hash so another machine cannot guess one; the address handed to the renderer is the
  local address the system routes toward it (`address_toward`). `Network::open` refuses a spec the
  carriage does not carry (`FormatRefused`), offers the `Feed`, sends `SetAVTransportURI` with a
  DIDL-Lite item naming the title and `protocolInfo`, and answers a `SinkStream` with a
  `FormatChanged` already told (S24 is `Words::Packed`: three bytes on the wire).
- **A `Feed` is the ring's consumer behind an HTTP answer** (`resonate-upnp-feed`, one thread a
  fetch): a head with no length, then quanta of a hundredth of a second pulled through
  `fill_stamped` no faster than the wall clock and carried by the fetch's `Wire`. WAVE is a
  `RIFF`/`data` header with sizes near `u32::MAX` (the 44-byte header up to two channels, the
  68-byte `WAVE_FORMAT_EXTENSIBLE` one with the layout's speaker mask above), the 24-bit ring word
  cut to three bytes and L16 turned big-endian (`wave::carried`). Pacing is the graph's job
  everywhere else and the ring pads an underrun with silence, so pulling at network speed would
  drain the ring and write that silence into the music. Where the renderer stops reading (its buffer
  full, a slower clock) writes block and the debt past one quantum is forgiven, so the renderer's
  clock governs and the pull never bursts to catch up. A second fetch of the same stream takes the
  source over and the first ends (renderers sniff an address before playing it). Inactive, nothing
  is pulled.
- **FLAC is encoded live from the ring, bit for bit** (`flac::Live`, over `flacenc`, which exposes
  what a stream with no end needs: the STREAMINFO is written first with an unknown total
  (`total_samples` 0, no MD5), then one fixed-size frame per block as the ring fills it, 4096 frames
  up to 48 kHz and 8192 above, default compression and no threads, so a block costs a fraction of
  its own duration). Every fetch begins a stream of its own (`fLaC`, frame numbers from zero), so a
  renderer that sniffs and refetches gets a valid stream either time. The encoder holds a partial
  block until the next quantum completes it, so `served`, which the renderer's clock is read
  against, counts only frames written; a drain flushes the short last block (the one frame whose
  size differs, which FLAC allows) before the end is marked, so `Drained` still waits on the
  renderer's clock for every frame. An encoder failure ends that fetch (`Error::Encoding { op }`
  with the foreign error in `tracing`). Pause costs nothing extra: nothing is pulled, so the held
  block waits with the ring.
- **FLAC above 96 kHz is the frame writer's, not the encoder's.** flacenc refuses a STREAMINFO
  over 96 kHz in `Stream::new`, though its frame encoder writes any rate. `flac::Live` builds the
  stream at `min(rate, 96 kHz)`, then names the real rate: the 20-bit STREAMINFO field in the
  head, and in each frame the header's rate code (`0010` for 176.4 kHz, `0011` for 192 kHz,
  `0000`, taken from STREAMINFO, for any other) with the header's CRC-8 and the frame's CRC-16
  written again over the changed bytes (`name_the_rate_in_the_frame`; the header is
  `FRAME_NUMBER_AT` bytes, the frame number's UTF-8 bytes and the block size's 0 to 2). Blocks stay
  8192 frames above 48 kHz.
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

## Past the renderer's buffer

- **A renderer's stream says it holds seconds** (`SinkStream::holding(Downstream::Seconds)`, a
  `resonate-audio` vocabulary every other backend leaves at `Moments`), and the engine reads it two
  three ways (`audio.md`): a seek while playing rebinds on a fresh stream instead of discarding the
  ring in place, a pause stands the stream down at once with no fade written into the ring, and a
  change of what is rendered restarts the stream. A discard in place would only change what the
  renderer is fed next, after the seconds it holds; a fade before a pause would sit in its buffer
  and be heard as a dip once it resumed; a splice of the equaliser into the ring would play out
  behind the buffer.
- **An equaliser, ReplayGain or listening-test change restarts the stream, once, after the change
  has been quiet for `DOWNSTREAM_RESTARTED_AFTER_QUIET_FOR` (300 ms).** The sink cannot see
  settings, so the engine does (`Engine::retune_heard_past_the_buffer`): on `SetEqualisation`,
  `SetReplayGain` and `SetLevelling`, and on a listening-test arm switch, where the output holds
  seconds and the plan it would now render differs from the one it renders (`OutputPlan::renders_alike`;
  a volume change does not, the volume being the renderer's), the usual change in the ring is made
  (so a non-seekable track or a refused rebind still plays the change eventually) and a restart
  is owed. Each further change pushes the due time out, so a slider drag restarts once after it
  settles, not at every step; the due restart is `rebind_where_it_stands` at the heard position,
  which is a fresh stream: `Stop`, a new address, `Play`, and the renderer's buffer of the old
  sound dropped. A rebind or a track change in between builds the output from the settings as
  they stand and cancels the owed restart; one waits while a boundary between rows is pending. The
  cost is a gap while the renderer reloads, the length of its own start-up, which is why it waits
  for the drag to end and why the volume is never restarted.
- **A fresh stream stops the renderer first.** Each stream takes the renderer's `Lane` (one per
  UDN, `Holders`): a `Mutex` holding which stream steers it. `stream::open` takes it, sends `Stop`
  where another stream held it (the renderer drops its buffer) and `SetAVTransportURI` with the new
  address under the lock; a `Watch` sends `Play`, `Pause` and `GetPositionInfo` only while its
  stream still holds the lane (`Hold::steer`), and its closing `Stop` only where it lets go of a
  lane it held (`Hold::let_go`). So a retiring stream's late pause or stop never reaches the stream
  that replaced it, whichever closes first.
- **A pause is the renderer's own `Pause`**, heard at once; the feed stops pulling, so on `Play` the
  renderer resumes the very buffer it held and the ring continues it frame for frame.
- **The volume is always the renderer's** (`HardwareVolume::Always`, which `Attenuator::of` hands
  the volume whatever `device-volume` says), through OpenHome's `Volume` service where the device
  offers it, else RenderingControl: `SetVolume`/`SetMute` go out on the
  `resonate-upnp-turns` thread the moment the slider moves, never waiting on the network, and the
  stream stays at unity (`BitPerfect` where the plan is).

## OpenHome

A renderer whose description lists `urn:av-openhome-org:service:Volume:1` is turned through it
(`Renderer::openhome`, preferred over RenderingControl where both are listed, because it is the
volume the OpenHome player itself keeps, where RenderingControl is a bridge onto it): `Characteristics` gives
`VolumeMax` once at discovery (100 where it does not answer), `Volume`'s `Value` over it is the
volume read at discovery, and `SetVolume`/`SetMute` carry `Value` in steps (`volume * VolumeMax`,
rounded) and `true`/`false`, on the same `resonate-upnp-turns` thread. Nothing else of OpenHome is
spoken, on purpose:

- **`Playlist` (`Insert`, `SeekId`, `Play`) gives no gain over AVTransport for this design.** The
  engine owns the queue and the gapless handover, and the renderer is handed one endless live
  address per stream, so a playlist of one that never changes is all `Playlist` could hold. Using it
  would mean emptying the listener's own OpenHome playlist (`DeleteAll`) or leaving our entry in it,
  switching the Product's source to Playlist (`Product.SetSourceIndex`) out from under whatever
  else plays, reading the position from the separate `Time` service, and keeping a second
  transport in step with the first. An OpenHome player that offers AVTransport (upmpdcli, Linn's
  bridge) already maps `SetAVTransportURI` and `Play` onto it, and a device that offers
  OpenHome alone is not found by the MediaRenderer search this crate sends, so `Playlist` would
  not widen the renderers reached. Seeking, pausing and the fresh address after `Stop` (*Past the renderer's buffer*)
  are the same moves either way.
- **`Product` is not asked** (no standby or source handling): a renderer asleep or on another
  source refuses `Play` and that is `StreamEvent::Refused`, as for any renderer.

## In the binary

`rendering::Rendering` holds the one `Network` a run uses. It searches only where the `renderers`
key is on (off by default: nothing is multicast unasked), `sinks --renderers` asks, or the sink
wanted names a renderer; the last two wait for the first search. The window always holds one
(`Rendering::for_the_window`, resting where none of those asks), so the settings pane's device
list can switch it: `Rendering::switch` hands `resonate-ui` a `Renderers` (`looking`, `look`), the
switch row under the devices calls it and stores `Setting::Renderers`, and renderers appear in
or leave the list through the sink changes `Beside` forwards. `Rendering::player` is how `play`,
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
a WAVE played through a real `Player` reaching the renderer bit for bit
(`a_song_played_through_the_engine_to_a_renderer_reaches_it_bit_for_bit`), a pause and a volume
change reaching it within `SOONER_THAN` (400 ms) and the song resuming bit for bit after the pause
(`a_pause_and_a_volume_change_reach_a_renderer_at_once_and_the_song_resumes_bit_for_bit`), an
equaliser dragged through six steps restarting the stream exactly once (one `Stop`, two addresses)
within `RESTARTED_WITHIN` of the last step with the fresh address carrying the song at the new
level and a volume change after it leaving it alone
(`an_equaliser_change_dragged_on_a_renderer_restarts_its_stream_once_from_where_it_is_heard`), the
same for a ReplayGain mode and a trim
(`a_replay_gain_change_on_a_renderer_restarts_its_stream_and_the_fresh_address_carries_it`), and a
seek sending `Stop`, `SetAVTransportURI` with a fresh address and `Play` within it, the new fetch
carrying the song from where it landed and nothing before
(`a_seek_on_a_renderer_stops_it_and_offers_a_fresh_address_starting_where_it_landed`). The fake
stops reading a fetch on `Stop` and fetches each new address afresh, as a renderer does.

The fake takes a `Profile`: the `Sink` string `GetProtocolInfo` answers and whether the description
adds OpenHome's `Product`, `Playlist` and `Volume` (answered under `/oh/`, recorded apart from
AVTransport's calls). With FLAC offered, a song played through a real `Player` arrives as `fLaC`
smaller than its PCM, offered as `audio/flac`, and decodes through the codec crate (and through the
`flac` tool where installed) to bytes containing the song
(`a_song_played_to_a_renderer_taking_flac_is_sent_flac_that_decodes_to_the_samples_played`); a 5.1
S24 stream to a renderer stating `channels=6` comes back from FLAC sample for sample and arrives
as a `WAVE_FORMAT_EXTENSIBLE` stream with mask `0x3F` and packed words
(`a_renderer_that_states_channels_is_sent_all_six_of_a_surround_stream_in_flac_and_in_wave`); a
renderer stating nothing is offered stereo alone; S24 stereo at 44.1, 88.2 and 96 kHz returns from
FLAC bit for bit and 192 kHz is offered as WAVE
(`flac_carries_twenty_four_bits_up_to_ninety_six_kilohertz_bit_for_bit_and_wave_the_rates_above`,
`a_rate_flac_cannot_carry_is_sent_as_wave_to_a_renderer_taking_both`); a renderer stating
`audio/flac;rate=` 176.4, 192 and 352.8 kHz is sent FLAC that returns bit for bit through the codec
crate, whose STREAMINFO names the rate, and through the `flac` tool (CRCs included) where installed,
a short last block of 100 frames and one of 16-bit size included
(`flac_above_ninety_six_kilohertz_is_sent_where_the_renderer_states_the_rate_and_comes_back_bit_for_bit`); a renderer taking FLAC alone and stating no rate is offered up to 96 kHz, a renderer named to
`Network::vouch` is sent 192 kHz FLAC that returns bit for bit, a vouch for another UDN changes
nothing, and a vouched renderer taking FLAC and WAVE is sent FLAC where it would be sent WAVE
(`a_renderer_taking_flac_alone_is_offered_it_above_ninety_six_kilohertz_only_when_the_listener_vouches`,
`a_vouched_renderer_taking_flac_and_wave_is_sent_flac_at_a_rate_it_would_otherwise_be_sent_as_wave`); an OpenHome renderer reads
its volume in steps of `VolumeMax` and is turned and muted through `Volume` alone, with no
RenderingControl call and no `Playlist` call
(`a_renderer_offering_openhome_volume_is_turned_through_it_and_its_playlist_is_left_alone`).
