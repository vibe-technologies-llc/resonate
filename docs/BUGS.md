# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [S · 0.8.1] The sync server queues without bound for a session that reads nothing: every `Close`
  of a space already closed sends the stored `Closed` blob again (up to 256 KiB; `store.rs`'s
  `coalesce` keeps the first, so a one-byte `Close` still costs the whole blob) to every device
  (`serve.rs` `close`), and `Hub::punch` queues a `Punched` of any size up to `LARGEST_FRAME` with
  no backlog charge (`hub.rs`), each into the session's unbounded channel. A space claimed for free,
  eight devices that never read and eight sending small `Close` frames queue gigabytes a minute,
  where `server.md` promises a slow reader a bounded queue: tell `Closed` only when a blob was
  stored, cap a punch's `sealed`, charge every notice to the `backlog` relays already keep
- [S · 0.8.1] The sync server's address table refuses every new address once full: past
  `MOST_ADDRESSES_KEPT` (100 000, one per IPv6 `/64`) `guard.rs` `admit` lets go only of accounts
  quiet for a day, so one connection from each `/64` of an ordinary hosting `/47` refuses every new
  device and every listener idle a day, for a day; evict the account seen least lately with nothing
  open
- [S · 0.8.1] `shared.sqlite` and `sync.sqlite` are readable by other local accounts: the data
  folder is made by a plain `create_dir_all` (`together.rs`, `sync.rs`, `config.rs`) and SQLite
  makes the files and their `-wal` and `-shm` 0644, though they hold shared-playlist edit codes and
  the member's x25519 private key (`shared/store.rs`, `ledger.rs`), and `config.toml` is narrowed to
  0600 for less. Make the folder 0700 and the files 0600, narrowing those already made
- [S · 0.8.1] Android's `ArtworkProvider` is exported with no permission (`AndroidManifest.xml`), so
  any installed app can step through `content://<pkg>.art/track/<n>` and read every cover in the
  library, past the media session's trusted-controller check; grant each URI to the controllers it
  is handed to, or guard the provider with a signature permission beside `BROWSING_HOSTS`
- [S · 0.8.1] The sync server reads a frame of up to `LARGEST_FRAME` (32 MiB) before `Hello` is
  checked (`quic.rs` `read_frame`, `web.rs`, `serve.rs`'s first read) and holds up to
  `FRAMES_READ_AHEAD` more in a stalled session or room, so each admitted connection pins some 160
  MiB having proved nothing; the first frame and every room frame want a ceiling of a few KiB
- [S · 0.8.1] The renderers' stream server spawns a thread for every connection it accepts and
  reads a request's head with a 5 s timeout on each `read` and no deadline over all
  (`resonate-upnp` `serve.rs`), so any host on the network (a café's Wi-Fi) holds thousands of
  threads at a byte every few seconds, until a plain `thread::spawn` elsewhere panics; bound the
  fetches, put a deadline on the head and refuse a path that is no `/resonate/<token>.<ext>` before
  spawning
- [S · 0.8.1] A UPnP description's `controlURL`, and an SSDP `LOCATION`, may name any host
  (`description.rs` through `Located::join`, `ssdp.rs`), so a device answering the search points the
  SOAP posts (the stream's token and the song's title among them) at another host or a loopback
  service; keep control on the host that described the renderer
- [S · 0.8.1] A party guest can take another guest's place: `rooms.rs` protects the host's
  `DeviceId` alone, so a guest joining under another's id (every member's is in `RoomWelcome`)
  replaces them (`RoomNotice::Replaced`) and its requests are heard as theirs; bind a member's id to
  a secret of its own on its first join, as the host token is
- [L · 0.8.1] A view link can claim a shared playlist before its owner does: `share` hands out the
  links while the space still waits (sharing offline is allowed), the first token to arrive claims
  the space and sets its viewer digest (`serve.rs`), and the view key derives the sealer, so whoever
  holds the view link and reaches the server first writes records the viewers take for the owner's,
  and the owner is refused `Refusal::Token`; a party room left past its grace is open the same way.
  The space id wants to commit to the write token, or the records to be signed by a key the edit
  key derives, with a step for the links already handed out
- [S · 0.8.1] (investigate) A `resonate://shared/…?server=` link opened from outside joins at once
  (`together.rs` `accepted`, a recorded decision) whatever server it names (`invitation.rs`); the
  hosted `/s/` page copies its fragment into the link unfiltered (`pages.rs`), and nothing bounds the
  rows a shared list receives, so a link clicked in a browser has the desktop reach a stranger's
  server every five minutes, which may fill a catalog playlist with millions of rows. Ask before
  joining a server other than the default, drop the fragment's `?` part on the page, cap a shared
  list's rows

## Sync and together
- [S · 0.8.1] A listen is offered to the other devices once its `heard` leaves zero, not once it has
  settled: `listens_after` (`synced.rs`) holds back only a listen at `heard == 0` past
  `settled_before`, while `Counting::Hears` raises `heard` every `TOLD_EVERY` (30 s) as the song
  plays, and the ledger and the remote `credit` are `INSERT OR IGNORE`, so a pass caught mid-song
  sends the time heard so far for good and every other device's statistics and Wrapped count short
- [S · 0.8.1] A playlist sync makes inherits a deleted playlist's link: `synced::made` clears
  `playlist_plays` for the id SQLite hands out again but not `playlist_sources`, which
  `playlist::created` clears (`forget_the_source_of_a_discarded_holder`) and a removal keeps for
  undo, so a synced playlist landing on a deleted linked one's id shows linked and its next refresh
  rewrites it with the link's songs and spreads that to every device
- [S · 0.8.1] A guest's request, a host's dismissal and a turn-out made while a party reconnects are
  thrown away (`party/service.rs` `remember` keeps nothing of `Ask`, `Kick` or `TakeAsk`), and the
  guest's song has already left its own queue (`seat.rs`), so it is lost twice with nothing said;
  keep them in a short queue and send them after `RoomWelcome`
- [S · 0.8.1] *Play next* on a party request removes it by index after a search that may take
  seconds (`party_room.rs` `play_a_request_next`, the phones' `together.rs`), so two pressed in
  quick succession queue one request and take another out unplayed; `TakeAsk` wants the request's
  sender and row
- [M · 0.8.1] The desktop's deletion is open to the race 90936028 closed on the phones:
  `remove_tracks` trashes every file and only then writes `forget_files` and its `songs_deleted`
  marks (`db.rs`), `holds` reads a row whose file is gone as not held (`synced.rs`), and
  `fetch_claimed` checks `holds` but never `deleted_here` before landing (`resonate-sync`
  `service.rs`), so a follow inside a large deletion fetches the songs back to the freed names, the
  forgetting leaves those files without a row, and the next scan brings them back. Write the marks
  before trashing, or keep a path whose file stands again as the phones do, and weigh
  `deleted_here` before a landing
- [L · 0.8.1] A device driven from another removes, moves or jumps by bare row index
  (`resonate-driven` `command_for`, sent with a plain `send`), so an edit made on the driven device
  meanwhile has a remote *Remove* take another song; the wire carries no queue revision for
  `send_if_the_queue_is_still` (`audio.md`'s *An edit by position names its queue*) to weigh, and
  older peers must keep working
- [L · 0.8.1] An unheld listen's handle goes stale once a scan credits it: `unheld_listens` keeps a
  plain rowid (`schema.rs`), `credit_the_unheld` (`history.rs`) moves the row into `listens`
  mid-song and deletes it, and the window goes on calling `listened` with that rowid, so the
  credited listen keeps the time heard at the scan, and a rowid handed out again has another
  listen's `heard` overwritten; an `AUTOINCREMENT` id and a note of what each unheld row became want
  a migration step
- [L · 0.8.1] A kept MusicBrainz answer's `seq` is `max(seq) + 1` (`resonate-musicbrainz`
  `kept.rs`), so once the 30-day sweep at open removes the newest answer the next one takes its
  number, under a mark a peer has already passed, and never reaches that peer; a counter that never
  goes back wants a migration step
- [S · 0.8.1] (investigate) One clean close before the welcome ends a party for good as
  `ServerTooOld`: `party/service.rs` reads a first `None` so and stops retrying, and `web.rs` reads
  any WebSocket `Close` as `None`, which a tunnel or a server draining its connections may send

## Playback and output
- [L · 0.8.1] A 3.0, 2.1, 3.1, 4.0 with a centre, 4.1, 6.0 or 7.0 source is mixed down by index
  though its container placed every channel: `positioned_layout` (`container.rs`) names Quad, 5.0,
  5.1, 6.1 and 7.1 alone and calls any other mask `Discrete`, which `Remix` routes in turn
  (`remix.rs`), so a 3.0 DSF (a three-channel SACD) on stereo plays its centre in the left speaker
  alone, a 2.1 sends its LFE right and a 7.0 crosses its sides; `audio.md` routes by index only
  where positions are unknown. The layouts (or a placed mask) want adding to `resonate-audio`'s
  vocabulary and PipeWire's matcher
- [S · 0.8.1] A seek into the second or a later link of a chained Ogg Opus takes no pre-roll and no
  `start_ts` floor: only `seek_the_first_link` takes `pre_roll` (`decoder.rs`), and
  `seek_along_the_chain` seeks to the link's exact timestamp, so `audio.md`'s *Seeks start early*
  holds for the first link alone and a seek into a stream rip's later link lands on CELT's burst
- [M · 0.8.1] A song failing just after a gapless handover fades the leaving song's tail out
  unheard: `fail` passes the boundary and `skip` reaches `start`, which retires the output still
  holding up to a ring (500 ms) of the leaving row before `boundary.at` (`engine.rs`), and
  `TrackFinished` and `TrackStarted` are told that much early; drop only what lies past `at` and let
  the graph reach it before moving on
- [S · 0.8.1] A UPnP renderer or Cast receiver fetching its stream again counts the first fetch's
  frames as heard: `served` is one counter for the `Feed`'s life though each fetch begins a stream
  of its own from frame zero (`feed.rs`), so the delay and position read behind for the rest of the
  song, and past `HEARD_TO_THE_SECOND` the drain is never told; reset it where a fetch takes the
  source over (the fake renderer fetches once, so whether a real one fetches again after `Play` is
  unseen)
- [S · 0.8.1] A Cast receiver's lane is held through `GET_STATUS`, `LAUNCH` and `LOAD`
  (`resonate-cast` `stream.rs`, some 45 s on a slow receiver), so a seek or an equaliser change that
  rebinds meanwhile waits that long, silent, in `Lanes::taken`; weigh ownership under the lock and
  launch outside it
- [S · 0.8.1] A resume after a break sends a bare `Range` with no `If-Range` and takes any `206`
  starting at the byte held, or a `200` with the head skipped (`resonate-fetch` `ranged.rs`
  `resumed_after`), though the validator is known and the total in `Content-Range` is not weighed,
  so a file a server re-transcoded or replaced between the break and the resume is joined old head
  to new tail
- [S · 0.8.1] (investigate) The ring's callback stores `quiet` from a read of `silenced` a whole
  `take_into` old (`ring.rs` `pulled`), so a pause then play landing in that window leaves `quiet`
  set over music, `is_sounding` false, and the next pause or track change stops the stream without
  its fade
- [S · 0.8.1] (investigate) An equaliser splice point is chosen 10 ms or two pulls ahead and
  published only after the whole recall is rendered again (`splicing.rs`), so a slow re-render (a
  long room response, Very high) lets the graph pass the point on the old curve and the 20 ms blend
  is lost
- [S · 0.8.1] (investigate) A packed 24-bit (`S24_3LE`) fill sizes the quantum in padded frames and
  caps it at the packed buffer's length (`resonate-pipewire` `process.rs` `write`, `asked_for`), so
  a quantum reaching the buffer's capacity is filled three quarters, every cycle

## Catalog
- [S · 0.8.1] Lengthening a file in place may shift its bytes under a reader it cannot see:
  `held_open_elsewhere` (`insert.rs`) reads a `/proc/<pid>/fd` it cannot open as that process not
  holding the file, so MPD or a media server under another account, every host process seen from
  inside Flatpak and a reader holding only an `mmap` have `FALLOC_FL_INSERT_RANGE` move the audio
  under them, where `library.md` promises no reader sees it; an unreadable folder should count as
  held, or the kernel be asked for a write lease
- [S · 0.8.1] `dedupe` adds a copy's plays to the kept row on every run the copy survives:
  `handed_over` (`copies.rs`) sums `plays` without zeroing the copy's, and a copy the trash refuses
  (`Removal::Kept`) keeps its row, so each run finds the pair again and the count grows
- [S · 0.8.1] Organise cuts its staging name across devices by bytes (`organise.rs`
  `noted_staging`), not through `naming::named_within` as `rust-style.md` asks, so a long name of
  multibyte letters makes a staging name that is not UTF-8, which exFAT and APFS refuse, failing the
  move
- [S · 0.8.1] A partial walk-back keeps a unit it did put back in the note where one of its
  sidecars had gone: `still_to_walk_back` compares whole `Move`s, sidecars included, against the
  trimmed one it landed (`organise.rs`), so *Put the last run back* stays offered and refuses that
  unit every time

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

## The window
- [S · 0.8.1] The window writes `last-tab = "server"`, which the loader refuses: the Server pane is
  remembered like any tab (`root.rs` `set_pane`), and the binary's `tab()` (`config.rs`) has no
  `server` arm though the man page lists it, so the next start opens on Tracks and every command
  warns of a setting it cannot read until another tab is opened; the man page's test lists the tabs
  by hand rather than walking `Pane`
- [S · 0.8.1] A volume change is kept 400 ms later for whichever zone is shown then (`root.rs`
  `set_volume`; `store` reads `switcher.selected()` as it writes), and `show_zone` does not cancel
  the wait, so a second zone's volume changed just before switching is kept as the first zone's, and
  one changed just before quitting is lost
- [S · 0.8.1] A queue selection outlives a change of zone (`zones.rs` `show_zone` keeps `reach` and
  `landed_by_a_jump`), so Delete, Enter or a move key after switching acts on the other zone's rows
  at the same positions, under that queue's own revision
- [S · 0.8.1] An album cell's artist cut by an ellipsis leaves a gap before the `·` and the year
  (`browser.rs` `album_cell_captioned`): the link keeps the room it was given while the cut text
  ends well short of it, so *Björk Ásgeirsdóttir…* stands some twenty pixels off its year
- [S · 0.8.1] A hint can come up over a sheet its own control opened: a press on the header's
  microphone with the pointer left at rest shows *Listen for a song…* above the Listen sheet's
  scrim until the pointer next moves, gpui scheduling the hint from the move it replays after the
  press and never weighing the scrim that then covers the control

## The phones
- [S · 0.8.1] A headset or lock-screen play after Android ended the app loses a queue's songs
  played in place and starts the wrong song part-way: `onPlaybackResumption` hands Media3 the whole
  saved queue (`PlaybackService.kt`), Media3 sets it on the player, and `handleSetMediaItems`
  (`RustMediaPlayer.kt`) loads served ids only where every item is served, so a mixed queue goes
  through `BrowseIds.playable`, which drops each served row and moves the start to the next library
  song, at the served song's position
- [L · 0.8.1] The phones keep no shuffle, list order or rows played next: `save_zone` keeps the
  rows, the row and the place, and `resume_saved_in` sends `..Resumption::default()`
  (`resonate-mobile` `lib.rs`), so a queue shuffled when the app was ended comes back in its
  shuffled order with shuffle off and the list order gone, where the desktop keeps all three; the
  phones' `resume` and `zone_resume` want them through a migration step
- [S · 0.8.1] One saved queue row whose URI does not read stops the phone's player opening:
  `resume_saved_in` applies `location(&track.uri)?` to every row, so a `resonate-displaced:` row
  organise left in a saved queue (`vanished.rs`) fails `MobilePlayer::new` with `InvalidUri` at
  every start; pass over it as `save_zone` passes over a row gone
- [S · 0.8.1] One queue row whose catalog row is gone makes the phones' whole queue unreadable:
  `queue()` reads through `tracks_at`, which answers `UnknownTrack` for any missing row, so the
  `break` written for a row not held never runs (`lib.rs`); `each_in_may_hold` was meant
- [S · 0.8.1] An Android controller's `seekTo(index, ms)` sends `jumpTo` and `seek` apart
  (`RustMediaPlayer.kt`), and `seek` converts at the rate of the song still current (`lib.rs`), so
  a car's seek into a song of another rate lands at about half or twice the place
- [S · 0.8.1] A transport action queued on Android while the engine closes runs against the closed
  player: the worker takes `native` before `access` and the close's `finally` takes `access` first
  (`LibraryRepository.kt`), so the action is lost and the banner shows the native object's internal
  message
- [S · 0.8.1] (investigate) `MobilePlayer::shutdown` reports a zone stopped where
  `Arc::try_unwrap` fails because the listening thread holds it that moment (`zoning.rs`), so the
  engine stops later on that thread and a service made at once may open a second engine beside it
- [S · 0.8.1] (investigate) Android never gives the bit-perfect mixer preference back when its
  stream closes or the app shuts down (`oboe_state.rs`'s `mix_freely` is reached only by the next
  open), which may leave a USB DAC's mixer bit-perfect and other apps silent after Resonate leaves
