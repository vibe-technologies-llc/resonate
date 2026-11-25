# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [S · 0.18.1] Hold the MusicBrainz download agent to its origin's redirects. **Low, builder.**
  `crates/resonate-musicbrainz/src/download.rs` builds on `configured()` and puts ureq's ten
  redirects back (`REDIRECTS_UREQ_FOLLOWS`), so the export and replication fetches still follow a
  redirect to any https host, while every provider now follows through `resonate_fetch::followed`
  within the origin asked. Find where MetaBrainz's servers redirect (the export mirror may hand off
  to another host), follow through `followed` or an allowlist of those hosts, and drop the setting.
  Done when a fake server's `302` to another loopback port is not followed, in a test.
- [M · 0.18.1] Harden the Windows Discord pipe. **Low, architect.** The pipe is opened with no
  security quality of service (`crates/resonate-discord/src/pipe.rs:44-51` through interprocess's
  `CreateFileW(..., FILE_FLAG_OVERLAPPED, ...)`), so a squatting server holding
  `SeImpersonatePrivilege` can impersonate the listener, beyond the learning of what plays that
  `windows.md` names; its `flush` is `FlushFileBuffers` with no deadline (`frame.rs:60-61`,
  `pipe.rs:101-103`), so a peer that never reads blocks the publisher thread for good; the reader's
  channel is unbounded (`pipe.rs:54-73`, `:118-131`); and the reader thread keeps the handle after
  the session drops, one leaked thread a retry against a silent pipe. Open with identification-level
  QoS, bound the write and flush as the reader is bounded, bound the channel and cancel the read on
  drop. Done when tests on Windows show a pipe that never reads ends the session within
  `ANSWERS_WITHIN` and leaves no thread behind.
- [S · 0.18.1] Keep Jellyfin's token out of the query where a header serves. **Low, builder.**
  `Session::url` puts `api_key` on every request (`crates/providers/jellyfin/src/lib.rs:111-118`),
  though only `streamed` reopens by URL, so proxy and request logs gather a full-access token;
  nothing logs a session out, and each password sign-in makes a fresh `DeviceId` (`lib.rs:165-167`),
  so every launch leaves a live token on the server. Send the token as a header except on the stream
  URL, reuse the device id and log out on forget. Done when the fake server takes the token from the
  header and sees `api_key` only on stream requests, in place of the tests that pin it on every
  query (`crates/providers/jellyfin/tests/server.rs:302`, `:386-399`, `:579-590`), and no error's
  text carries the token.
- [S · 0.18.1] Refuse a symlink in the inbox. **Low, builder.** `read` filters by extension and
  `settled` uses `fs::metadata`, which follows links (`crates/providers/inbox/src/lib.rs:143-148`,
  `:187-198`), so a link named `<mbid>.flac` delivers a file from elsewhere, retargetable between
  looks. Use `symlink_metadata` and pass links over. Done when a test's linked `<mbid>.flac` is not
  delivered.
- [S · 0.18.1] Open TIDAL's sign-in page only over https. **Low, builder.** `with_scheme` keeps
  whatever scheme `verification_uri_complete` names (`crates/providers/tidal/src/sign_in.rs:73-80`,
  `:255-260`), and the window and the terminal open it. Accept `https` on TIDAL's hosts alone and
  refuse the sign-in otherwise. Done when the fake's device code, today always `link.tidal.com`
  (`crates/providers/tidal/tests/server.rs:1100-1139`), names `file:///etc/passwd` or `http://` and
  is refused as unreadable.
- [S · 0.18.1] Serve a cue sheet's cut only from a regular file. **Low, builder.** The media server's
  `bytes_of` now opens only a regular file without following a link
  (`crates/resonate/src/offering.rs`, `regular_file`), but a cut of a cue sheet opens its file through
  `Shelves::cut_opened` and the codec's `Sources` (`LocalFiles`), which follow a link and block on a
  FIFO put in the file's place. Check the kind there too. Done when a FIFO behind a cut answers 404
  at once, in a test.
- [M · blocked: design] A device that joins a shared playlist after a hostile editor compacted it
  takes the forged member keys from the snapshot: the first key heard for a `MemberId` stands, so
  devices that already held the list keep the true keys, but a newcomer's first is the forger's, and
  a removal it later makes seals the other members' seats to them; closing it needs `Member` ops
  signed by the member's own key, a change of the op format
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address


## Playback and output
- [M · 0.18.1] Disconnect a PipeWire stream before its listener is dropped. **Medium, architect.**
  `Opened::drop` drops the listener, and with it the callback data the realtime thread reads,
  without `stream.disconnect()` (`crates/resonate-pipewire/src/client.rs:224-228`), and `Shutdown`
  (`:1123-1128`), the loop's tail (`:1151-1154`) and `lose_the_graph` (`:1158-1177`, reached from
  `Heard::Never` at `:1076-1087`) clear streams the same way, while `Close` (`:1059-1064`) and
  `StopCapture` (`:1025-1029`) disconnect first; a data thread inside `process` then reads freed
  memory. Disconnect inside `Opened::drop` ahead of the listener. Done when the hosted-daemon tests
  drop a streaming client during shutdown and graph loss under a sanitiser build without a fault.
- [S · 0.18.1] Bound a profile the phones import by its whole peak. **Low, builder.** The desktop's
  `Store::read_in` now refuses a profile whose preamp and stacked bands pass `BOOST_AT_MOST_DB`
  (`Error::PeakTooHigh`), but the phones' `parse_eq_profile` (`crates/resonate-mobile/src/equaliser.rs`)
  clamps each value to 24 dB alone, so stacked bells still sum past it. Weigh the whole peak there
  too and refuse through the mobile catalog. Done when a phone import of stacked +24 dB bells is
  refused, in a test.
- [S · 0.18.1] Keep a sink's bound profile, even when it cannot be read, from falling to the
  fallback curve. **Medium, builder.** `equaliser::resolved` leaves out a sink whose bound profile
  is missing or unreadable (`crates/resonate/src/equaliser.rs:938-968`), and
  `Equalisation::for_sink` then applies the fallback curve to it
  (`crates/resonate-engine/src/pipeline.rs:27-34`), while `Binding::Own` with no file plays flat;
  the window's gathering has the same shape
  (`crates/resonate-ui/src/views/settings/equaliser.rs:378-394`). Carry an unreadable binding as
  flat and say so. Done when a test binding a sink to a missing profile plays it flat beside a
  fallback curve.
- [S · 0.18.1] Bind `eq --measure`'s curve to the output it measured. **Medium, builder.** The sweep
  plays through `--sink` or the configured sink, but the fitted curve is bound to `--for`
  (`crates/resonate/src/equaliser.rs:57`, `:88-98`, `:347-414`), so one output's correction lands on
  another. Bind to the sink the sweep played, or refuse a `--for` naming another. Done when a test
  measuring through one sink and naming another is refused.
- [M · 0.18.1] Refuse a measurement that heard no sweep. **Medium, architect.** `Sweep::heard_as`
  takes the largest tap of the whole circular deconvolution and refuses only below `SILENT_BELOW`
  (`crates/resonate-dsp/src/sweep.rs:174-187`), so a recording of noise alone yields a response that
  `fit_recorded_sweep` fits and applies (`crates/resonate-mobile/src/measurement.rs:153-177`).
  Compare the peak with a robust noise floor and refuse with a typed reason. Done when a test
  recording of white noise is refused and a recorded sweep under noise is still heard.
- [S · 0.18.1] Describe a renderer again when it answers from another address. **Medium, builder.**
  `round` counts a hit on an alias as seen (`crates/resonate-upnp/src/network.rs:463-480`, aliases
  matched at `:610-616`), so a renderer that moved A to B and back stays at the dead B for good, and
  a spoofed UDN re-points the sink. Count an alias only when the current location is heard too, and
  describe the new address otherwise. Done when a test renderer moving A, B, A is followed back to
  A.
- [S · 0.18.1] Drop a capture the caller gave up on, and the PipeWire start thread. **Low,
  builder.** A capture answered after its five-second timeout stays in `heard` unnamed
  (`crates/resonate-pipewire/src/client.rs:801-803`, `:1001-1024`), and a start that times out
  leaves the loop thread running for ever (`:729-757`). Mark an abandoned request so the loop drops
  what it builds, and stop the thread on a start that failed. Done when hosted-daemon tests show
  neither stream nor thread outliving its timeout.
- [S · 0.18.1] Keep `Spanned::holds` from overflowing on a node's range. **Low, builder.** `value -
  self.min` overflows for a node advertising `min = i32::MIN`
  (`crates/resonate-pipewire/src/format.rs:173-177`) and panics inside an `extern "C"` callback in a
  checked build. Use checked or widened arithmetic. Done when a unit test with `i32::MIN` bounds
  answers.
- [S · 0.18.1] Report the frames `Restore` holds. **Low, builder.** `Restore` claims `size - hop`
  frames of latency (`crates/resonate-dsp/src/restore.rs:1428`) but holds up to a hop more and
  implements neither `frames_held` nor `frames_short_of_its_latency`, which only `TruePeak` does
  (`peak.rs:667`, `:671`), so `Chain::frames_it_holds` (`chain.rs:160-167`) is off by up to 5.3 ms
  at the gapless boundary, the published position and `HeardAt`, and claims 768 held after a reset.
  Implement both. Done when a chain test with restoration reports the frames held after a push and
  after a reset.
- [S · 0.18.1] Pair a renderer's rates with its channel counts per protocolInfo entry. **Low,
  builder.** `crates/resonate-upnp/src/carriage.rs:228-293` crosses rates and channel counts across
  entries, so a renderer offering 96 kHz stereo and 48 kHz six-channel is sent 96 kHz six-channel.
  Read each entry whole. Done when that pair of entries plans only what each offers, in a test.
- [S · 0.18.1] Release the renderer round's lock between exchanges. **Low, builder.** The round
  holds its lock across serial three-second exchanges with no total deadline
  (`crates/resonate-upnp/src/network.rs:439-501`, `servers.rs:379-420`), so resting or stopping
  multicast waits out every dead renderer. Exchange outside the lock and bound the round. Done when
  a test round with five silent renderers rests within its deadline.
- [S · 0.18.1] Cap a renderer's name and UDN. **Low, builder.** friendlyName and UDN are taken whole
  with control characters kept (`crates/resonate-upnp/src/description.rs:123-124`, `:153-154`). Cut
  and clean both. Done when a description with a 1 MiB name holding newlines lists a short clean
  one.
- [S · 0.18.1] Keep UPnP renderers when Cast fails to start. **Low, builder.** `Kinds::resting`
  drops the UPnP kind it built when Cast fails (`crates/resonate/src/rendering.rs:29-60`). Keep each
  kind that started. Done when a test with Cast refused still lists UPnP renderers.
- [S · 0.18.1] Release an idle inhibit whose request timed out. **Low, builder.** `hold` keeps the
  request only on success (`crates/resonate-mpris/src/idle.rs:76-112`), so an `Inhibit` the portal
  granted after the two-second deadline is never closed and the session cannot idle until the
  process exits. Retry with a longer deadline or reconcile on release. Done when a stand-in portal
  answering after three seconds holds no inhibition after `release`.
- [S · 0.18.1] Correct the equaliser's rule about bands past Nyquist. **Low, builder.** `angle_at`'s
  clamp to π never fires, since `design` passes a band over below `applies_at`
  (`crates/resonate-core/src/eq.rs:61-63`, `:543-558`), and `.claude/rules/eq.md:74-75` says every
  kind is identity at π, which is false for high-pass and band-pass (silence) and low-shelf (a flat
  gain). Drop the clamp and say a band at or past Nyquist is passed over. Done when the rule matches
  `design` and the dead clamp is gone.
- [S · 0.18.1] Clamp a fitted band to the gain range instead of flattening it. **Low, builder.**
  `third_octave_bank` turns a gain past ±40 dB into `FLAT`
  (`crates/resonate-core/src/eq.rs:1330-1342`), and `Target::fitted` keeps updating it unclamped
  (`:1234-1286`), so a deep null in a measured target silently loses its band while its neighbours
  are fitted around it. Clamp in the bank and the update. Done when fitting alternating ±40 dB
  targets keeps every band at its bound.
- [S · 0.18.1] Say why `eq` replaced a kept profile, after it did. **Low, builder.** `eq --import`,
  `--fetch` and `--fit-measurement` replace a kept profile of the same name before reporting it
  (`crates/resonate/src/equaliser.rs:222-226`, `:265-266`, `:331-334`, `:888-894`), and
  `--keep-measurement` writes in place (`:384-395`). Refuse a taken name unless asked to replace it,
  and stage the measurement. Done when importing over a kept name is refused without `--replace` and
  a failed measurement write leaves the old file.
- [S · 0.18.1] Warn once while the machine may not be kept awake. **Low, builder.** `awake.rs:43-65`
  retries every second and warns each time while refused. Warn once and retry quietly. Done when a
  refusing stand-in logs one warning over ten seconds.
- [M · 0.18.1] Reconnect the MPRIS connection the session bus closed. **Low, architect.** The desk
  thread now notices a closed connection and opens notifications and the idle inhibit again
  (`crates/resonate-mpris/src/desk_bus.rs`), but the player's own connection is only logged once and
  left (`service.rs`), so media keys and `Metadata` stay gone for the session. Claim the name again
  and rebuild the object server, the name watch and the shared connection fields. Done when a bus
  test that closes the player's connection finds it answering `Metadata` again.

## Device sync
- [S · 0.18.1] Report a shared playlist as shared only once the server has it. **Medium, builder.**
  `settled` waits twenty seconds on any list owing and returns nothing
  (`crates/resonate/src/together.rs:580-595`), and `--share`, `--join` and `--leave` then print the
  link or success and exit 0 whatever the standing, the server down or the request turned away
  included (`:674-709`). Answer the standing and fail when it is not settled. Done when a test
  against a stopped server exits non-zero with no link printed.
- [S · 0.18.1] Judge a launch's invitations once the home server is known. **Low, builder.**
  Invitations from the launch's arguments are judged at `crates/resonate/src/window.rs:94`, before
  `together::start` sets `HOME_SERVER` at `:167` (`together.rs:51`, `:203-204`, `:574-578`). Judge
  them after. Done when a launch with a link naming the default server offers it as such.
- [S · 0.18.1] Serialise beginning and joining a party. **Low, builder.** `party_room` begins and
  joins unserialised and drops the old room under the `ROOM` lock
  (`crates/resonate/src/party_room.rs:152-228`), contrary to `party.md`. Take one lock across the
  whole change. Done when concurrent begin and join leave one room in a test.
- [S · 0.18.1] Start a party only once its settings are known. **Low, builder.** `party start` sends
  `Load` with autoplay before `settings_for` can fail with `PartyNeedsAServer`
  (`crates/resonate/src/party.rs:390-409`). Read the settings first. Done when a party start with no
  server plays nothing.
- [S · 0.18.1] Keep a guest's request when it could not be queued. **Low, builder.** `added_here`
  removes a row by a stale index even when `song_of` is `None`
  (`crates/resonate/src/party.rs:145-157`), and `play_a_request_next` takes the request though the
  insert failed (`party_room.rs:309-324`). Remove by identity and only after the insert. Done when a
  failed insert leaves the request waiting, in a test.
- [S · 0.18.1] Tell why Together could not start. **Low, builder.** Any `try_lock` error is a silent
  `None` (`crates/resonate/src/together.rs:183-185`), so the window gets neither Together nor
  parties (`window.rs:195-198`). Tell a held lock from another failure and log the latter. Done when
  a test with an unreadable state folder logs the reason.

## Catalog
- [M · 0.18.1] Stop calling a band-limited lossless file Fake on a lone wall. **Medium, architect.**
  `judged` answers Fake for a lone wall at or below `LOSSY_CEILING_HZ` (19.5 kHz) or an upsampling
  match, excused only under 44.1 kHz (`crates/resonate-analysis/src/verdict.rs:341-408`, `:455-457`,
  `:565-567`), and says "it is not the lossless master it claims to be" (`words/en/verdict.ftl:3`),
  so a genuine band-limited source (an FM air-check, a 32 kHz DAT master at 44.1, a lathe-filtered
  needle-drop) reads as fraud. Answer a lone wall as suspect with the wording of a doubt, keeping
  Fake for a lossy signature. Done when lossless and lossy pairs walled at 18, 19, 19.5 and 20.5
  kHz, at 44.1, 48 and 96 kHz, are judged as intended in
  `crates/resonate-analysis/tests/analysis.rs`, whose fixtures today sit only at 16 kHz and 21.7
  kHz.
- [M · 0.18.1] Check the MusicBrainz export's schema before reading its columns. **Medium,
  builder.** `imported` parses `SCHEMA_SEQUENCE` but never compares it with 31, the schema the
  column positions follow (`crates/resonate-musicbrainz/src/mirror/import.rs:432-433`, `:460-465`,
  `:509-519`, positions in `copy.rs:45-361`), and drops the derived archive's sequences (`:239`);
  only a short row refuses, so a schema that inserts or moves a column is read into the wrong fields
  and served, though `.claude/rules/musicbrainz.md:120-122` says a schema change refuses the build.
  Pin the schema read and refuse another, and compare the two archives' sequences. Done when a test
  export stamped 32 is refused with a typed error.
- [M · 0.18.1] Sync a finished MusicBrainz build before it replaces the copy. **Medium, architect.**
  The build writes with `journal_mode = OFF` and `synchronous = OFF`
  (`crates/resonate-musicbrainz/src/mirror/import.rs:341-352`, `:1055-1056`) and nothing under
  `mirror/` calls `sync_all`, then `settled` removes the amendments and renames the tables and the
  copy as three steps with no marker (`import.rs:304-318`), and `received_copy` removes the
  amendments before a rename that may fail (`mirror/offered.rs:242-248`). A power loss after the
  rename leaves a torn copy trusted once its `made` row reads, with the export already forgotten,
  and a failed step leaves amendments gone beside a copy that needs them. Sync the files and the
  folder before and after the renames, rename the copy first and remove the old amendments last.
  Done when a test failing each step in turn leaves either the old copy with its amendments or the
  new one whole.
- [S · 0.18.1] Count the rows a media server's page returns. **Medium, builder.** `songs_of` and the
  album shelf drop rows with `filter_map(song_of)`, a song with no local file, an unknown extension
  or a cut with no duration (`crates/resonate/src/offering.rs:142-158`, `:491-524`, `:633-638`),
  while `TotalMatches` and `childCount` come from `tracks_counted`, so a control point is answered
  fewer rows than asked and reads rows twice across pages. Count what is offered, or page over what
  can be offered. Done when a test catalog with unplayable rows pages without gaps or repeats.
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
- [S · 0.18.1] Refuse a `wrapped` year no calendar holds. **Low, builder.** `wrapped --year` takes
  any `i64` (`crates/resonate-cli/src/grammar.rs:1765`), as does the phones' `wrapped(year)`, and
  `Calendar::year_bounds` (`crates/resonate-core/src/wrapped.rs:690-703`) feeds it to
  `day_since_the_epoch`, whose arithmetic is unchecked (`date.rs:50-62`): a checked build panics and
  a release reviews garbage. Make the arithmetic checked and bound the year to what `Timestamp`
  holds. Done when `--year 9223372036854775807` is refused.
- [S · 0.18.1] Choose Wrapped's default year among years not ahead of today. **Low, builder.**
  `year_worth_showing` opens on the latest year heard
  (`crates/resonate-core/src/wrapped.rs:273-285`), so one listen stamped ahead by a wrong clock or a
  bad import shows that year for good. Leave out years past today. Done when `[2030, 2025]` in 2026
  opens on 2025.
- [S · 0.18.1] Take "with" as a guest credit only before a name. **Low, builder.** `FEATURING` holds
  `with` (`crates/resonate-core/src/titles.rs:131`, `:153-158`), so `bare("Stay (With Me)")` is
  `Stay` and other songs of the artist group as its versions. Accept `with` only where
  `without_guests` needs it, not in the grouping key. Done when `bare("Stay (With Me)")` keeps its
  bracket.
- [S · 0.18.1] Fold a recording key's artist and album as its title is folded. **Low, builder.**
  `crates/resonate-core/src/recorded.rs:30-38`, `:56-69` word-normalise the title but keep the
  artist's and album's punctuation and spacing, so one recording keys apart under `Guns N' Roses`
  and `Guns N Roses`. Fold all three alike. Done when both spellings key the same.
- [S · 0.18.1] Write a cut that holds frames, and name a zone only with letters left. **Low,
  builder.** `to_uri_within` writes `#frames=S-S` for an empty span
  (`crates/resonate-core/src/time.rs:78-120`), which `from_uri_within` refuses (`media.rs:188-217`),
  as a cue sheet with equal `INDEX 01` stamps produces; and `ZoneName::given` tests emptiness before
  filtering control characters (`zone.rs:46-60`), so `"\u{7}"` names a zone with nothing. Refuse an
  empty span where it is made and test emptiness after filtering. Done when both round trips are
  tested.
- [S · 0.18.1] Say "a year ago" past 360 days. **Low, builder.**
  `crates/resonate-core/src/elapsed.rs:5-92` counts months of 30 days and years of 365, so 361 to
  364 days read "12 months ago". Promote twelve months to a year. Done when 362 days reads a year
  ago.
- [S · 0.18.1] Tell the media server when music folders change. **Low, builder.** `filter_music`
  does not move `catalog_revision` (`crates/resonate-library/src/db/mod.rs:1040-1046`), so the
  offered totals, cursors and lists and `SystemUpdateID` stay stale. Move the revision. Done when a
  test sees `SystemUpdateID` change after a folder is filtered out.
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
- [S · 0.18.1] Key the analysis cache by the bytes decoded. **Low, builder.** `KeptAnalyses::keep`
  stats the file again after the decode (`crates/resonate-analysis/src/kept.rs:82-122`,
  `crates/resonate-engine/src/player.rs:303-313`), so a file changed mid-decode keeps the old bytes'
  analysis under the new key; and `recalled` deletes any entry it cannot read, another version's
  included (`kept.rs:62-80`), so a dev build and the release erase each other's cache. Take the key
  before the decode and pass another version's entry over. Done when tests show both.
- [S · 0.18.1] Keep the analysis crew's lane from hanging on a panic. **Low, builder.**
  `Lane::drain` leaves `draining` set when `note` unwinds
  (`crates/resonate-analysis/src/crew.rs:182-218`) and `wait_for_room` checks no stop, so a checked
  build's pass hangs; and `loudest_through` maps a worker's panic to `Stopped` while the others run
  on (`headroom.rs:247`). Clear the flag on unwind and stop the others. Done when a test worker that
  panics ends the pass with an error.
- [S · 0.18.1] Judge padding behind the same evidence gate. **Low, builder.** Padding answers Fake
  on any effective length short of the declared, ORed before `heard_enough`
  (`crates/resonate-analysis/src/verdict.rs:569-582`), and the wall's wording is fixed to 20.7 kHz
  (`:217-223`, `:459-467`), so a sub-44.1 kHz file's anti-alias wall reads as a lossy encode beside
  Genuine. Gate padding with the rest and word the wall by the rate. Done when a short clip is not
  judged and a 32 kHz file's wall is worded as its rate's.
- [S · 0.18.1] Walk the MusicBrainz documents by id, not by id span. **Low, builder.** The chunk
  count comes from `max(id) - min(id)` unchecked
  (`crates/resonate-musicbrainz/src/mirror/import.rs:1174-1189`), and `placed` sizes a `Vec` by an
  id (`mirror/looked_up.rs:109-124`), so one stray id near 2^62 asks 10^15 empty chunks or aborts
  the build on a terabyte allocation. Walk by the last id seen and refuse an id past `u32::MAX`.
  Done when a test export with one huge id is refused quickly.
- [S · 0.18.1] Accept a MusicBrainz table only as one regular entry. **Low, builder.** Tar entries
  are matched by name alone (`crates/resonate-musicbrainz/src/mirror/import.rs:420-454`), so a link
  named `mbdump/artist` counts as a present, empty table and a repeated entry loads twice. Require a
  file entry and refuse a repeat. Done when an archive with a linked table is refused.
- [S · 0.18.1] Lay the amendments file in one transaction and check it against the tables. **Low,
  builder.** `made_at` lays the schema and inserts `amended_from` as two statements and never
  compares an existing row with the tables' dump and sequence
  (`crates/resonate-musicbrainz/src/mirror/amended.rs:68-94`), and `sequenced_at` ignores the rows
  it changed (`:261-272`), so an interrupted creation hides every amendment while replication
  reports success. Lay it in one transaction, compare, and check the rows changed. Done when a test
  amendments file with only the schema is laid again.
- [S · 0.18.1] Let a staged write's own failures decide its outcome. **Low, builder.**
  `write_atomically` answers `Unwritten { Place }` when the folder sync after a rename fails, though
  the target was replaced (`crates/resonate-core/src/file_facts.rs:171-174`, `:224-241`), as the
  sign-in jar does (`crates/resonate-signin/src/jar.rs:147-166`, so the window exits failed beside a
  good jar); and `claimed` reads a lock the filesystem refuses as held and `taken_over` truncates
  (`crates/resonate-core/src/writer.rs:127-137`), so two writers on a lockless mount both win.
  Report a failed folder sync apart, and refuse to truncate an unlockable claim. Done when tests
  show both.
- [S · 0.18.1] Round-trip legacy text byte for byte, and write WTF-8 as Windows reads it. **Low,
  builder.** `text::decoded` then `encoded` changes an odd UTF-16 tail and Shift_JIS duplicate
  codes, and the UTF-8 mark is not part of `TextEncoding`, so each writer restores it by hand
  (`crates/resonate-core/src/text.rs:100-147`); and `wide_of_wtf8` accepts four-byte forms above
  U+10FFFF and overlongs, writing lone surrogates (`os_bytes.rs:92-133`). Carry the mark in
  `TextEncoding`, refuse a re-encoding that differs, and replace an invalid sequence. Done when
  `b"\x87\x90"` as Shift_JIS round-trips and `F4 90 80 80` writes U+FFFD.
- [M · blocked: platform] On macOS and Windows a retag that grows a hard-linked track's head while it
  plays still shifts the bytes under the decoder: neither can tell who holds a file
  (`Shifting::readers_can_be_asked` is false), so the copy is written back over the same inode as
  before rather than refusing every such write; Linux and Android wait for the readers under a lease

## Formats
- [S · 0.18.1] Keep a SYLT line led by a bracket text when it is written as LRC. **Low, builder.**
  `crates/resonate-codec/src/sylt.rs` writes timed words as LRC without `escaped_for_lrc`, so a
  line opening on `[offset:…]` or a stamp reads back as a tag, as Better Lyrics' did. Escape it the
  same way. Done when such a line round-trips, in a test.
- [S · 0.18.1] Refuse `..` in a sheet's file URI on Unix. **Low, builder.** `sheet::local_file`
  still normalises `file:///a/../b` lexically (`crates/resonate-library/src/sheet.rs`), though
  `MediaLocation::from_uri` now refuses dot segments. Read such a row as missing. Done when a sheet
  row naming `file:///music/../etc/a.flac` reads as missing, in a test.
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
- [S · 0.18.1] Give `abx` a verdict only on a finished test. **Medium, builder.** The conclusion
  reads `heard_it()` or `luck()` off the answered prefix of an unfinished test
  (`crates/resonate/src/abx.rs:235-258`), so stopping at 5 of 5 says the listener can hear it, which
  optional stopping makes meaningless. Say only the tally for an unfinished test. Done when a test
  leaving after five right answers prints no verdict.
- [S · 0.18.1] Hold the terminal's lines to one line and say every word through the catalog. **Low,
  builder.** `said::plain` keeps `\n` and `\t` (`crates/resonate/src/said.rs:9`, `:83-94`), so a
  title holding a newline scrolls the readout on every redraw (`readout.rs:67-125`, `:153-175`), and
  `info` and `listen` skip `on_one_line`; `sinks.rs:37-40` prints a literal `none` beside
  `words::main::none`, `info.rs:261` a literal `faststart`, and `playing.rs:626`, `info.rs:204` the
  `Debug` of engine enums; table counts are not grouped (`stats.rs:53-57`, `suggest.rs:51`,
  `playing.rs:285`). Fold control characters, speak each word and group each count. Done when a
  title holding a newline draws on one line and the catalog test refuses the literals.
- [S · 0.18.1] Restore the terminal on SIGQUIT and fail on a write error. **Low, builder.** Ctrl-\
  leaves the terminal raw, since `ISIG` stays on and SIGQUIT is not in `ASKING_TO_LEAVE`
  (`crates/resonate/src/input.rs:118-125`, `signals.rs:17`); and `said` swallows a write error other
  than a closed pipe at debug (`said.rs:73-81`, `:96-105`), so `resonate sinks > /dev/full` exits 0.
  Restore on SIGQUIT and exit non-zero on a failed write. Done when
  `crates/resonate/tests/terminal.rs` sends QUIT beside INT, TERM and HUP and asserts the mode
  restored and the exit code, and `/dev/full` fails.
- [S · 0.18.1] Print each of `Config`'s fields once. **Low, builder.** `Debug` prints `offer_name`
  twice (`crates/resonate/src/config.rs:367-368`). Print it once. Done when the `Debug` test names
  each field once.
