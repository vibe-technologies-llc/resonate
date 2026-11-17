# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · 0.12.1] A Keystore read of the sync settings that throws becomes a blank pairing code that mints a
  new group over the real one: `LibraryFeatures.kt` falls back to `SyncConfig(enabled = true,
  key = "")`, `syncWith` starts sync with it (`resonate-mobile`'s `sync.rs` mints where the key is
  blank) and, the code then differing, `accounts.writeSync` overwrites the kept key, so one
  `KeyStoreException` or `AEADBadTagException` at engine open leaves the phone in a group of its own
  with its devices lost; iOS refuses exactly this (`ios.md`): a failed read should leave sync unset
  and be read again, never started or written back
- [S · 0.12.1] An Oboe open that timed out still moves the shared `Routing` when it finishes late:
  `Device::open` gives up after `OPEN_WITHIN` but the `resonate-mobile-output` thread goes on to
  `prefer_bit_perfect`, `opened_on` (overwriting `actual`) and `routing.opened`, then, its `ready`
  dropped, `routing.closed` finds itself current and calls `mix_freely`, so a stream opened since
  loses its bit-perfect mixer preference, `output_stream()` says nothing is open and `zoning.rs`'s
  *held bit-perfect* refusal reads the late device; an abandoned opener should close its stream
  without touching `Routing` (`oboe.rs`, a host test in `oboe_state.rs`'s style)
- [S · 0.12.1] A bit-perfect preference granted before an open that then fails is never given back:
  `oboe.rs` returns on `FormatRefused` or an `open_stream` error after `prefer_bit_perfect` succeeded,
  with no `mix_freely`, so the USB device's mixer keeps Resonate's attributes until the next open or
  `shutdown`, against `android.md`'s *gives back any bit-perfect preference*
- [S · 0.12.1] `mobile.md` says Android's statistics read `statistics(days)` alone and call no
  day-cutting export, but `StatisticsScreen.kt` calls `listeningDetail` with the zone's name and
  offset as iOS does: the rule is stale
- [S · 0.12.1] Investigate: a `MulticastLock` may be taken again after `LibraryFeatures.stop()`: the
  settle paths of `NetworkRenderers.kt`, `CatalogOffering.kt` and `syncWith` call
  `hearSearches(true)` after a blocking native call cancellation cannot stop, and
  `if (hear && !lock.isHeld) lock.acquire()` is an unsynchronised check-then-act, so Wi-Fi may be
  held awake with no engine open

## iOS
- [S · 0.12.1] Investigate: a refused `setActive(false)` is swallowed (`try?`, `NowPlaying.swift`) and
  `release.owed(by:)` counts the release done, so where RemoteIO still runs as the transport stops
  (`IsBusy`) the session stays active and the audio Resonate interrupted is never told to resume

## Security
- [S · 0.12.1] One `Append` of many tiny blobs stalls the sync server for everyone: `Reader::blobs`
  (`wire.rs`) admits a count up to the frame over `SEALED_AT_LEAST + 1`, about 1.15 million in one
  32 MiB frame, and `Store::append` runs a `SELECT` and a linear `fresh.iter().any` per blob (O(n²))
  inside `spawn_blocking` holding the one `Mutex<Store>`, before `space_bytes` or `totals_in` is
  weighed; anyone may claim a space, so a stranger holds every other space's claim, catch-up and
  append for minutes. A blob count cap in decoding, a set for the repeats and the caps weighed
  first
- [M · 0.12.1] The web socket path, the one the hosted server is reached by, keeps no `--frame-bytes`
  budget before a message is whole: tungstenite assembles up to `LARGEST_FRAME` (`web.rs`'s
  `limits`) before `hold(length)` is asked, and a text, ping or pong message is read and passed over
  uncounted by the budget and by `--frames-per-minute`, so 2 048 sessions may each hold 32 MiB where
  `server.md` says the budget bounds them; draw on `FrameBytes` as bytes come off the socket, count
  every message, and say in `server.md` what the web path holds
- [S · 0.12.1] The server's QUIC transport keeps quinn's default stream counts and windows
  (`quic.rs`'s `transport()` sets keep-alive and the idle timeout alone): the server takes one bidi
  stream a connection, but a client may open the other streams and fill each one's receive window,
  bytes quinn buffers for streams never accepted and the frame budget never weighs; a server transport
  of one bidi stream, no uni streams and a window just over `LARGEST_FRAME` (check quinn-proto's
  defaults at the pinned version)
- [S · 0.12.1] A space's `Close` word is not weighed against the byte caps: `Store::close` writes up
  to `--record-bytes` into `spaces.closed` without touching `spaces.bytes`, so 10 000 spaces hold
  about 2.5 GiB beyond `--total-bytes` and `--space-bytes`
- [M · 0.12.1] One routed IPv6 prefix or a few IPv4 addresses fill the server-wide caps: `guard.rs`
  counts a /64 as one address with 5 spaces, 50 shared spaces and 50 rooms a day, against 10 000
  spaces and 1 024 rooms in all, and a space holding one record outlives the one-day unused sweep
  for `--idle-days`, so a /48 fills the spaces in a day for a year and about 21 IPv4 addresses hold
  every room; aggregate allowances at /56 and /48 (and /24) beside the /64's
- [S · 0.12.1] A device queues every frame the server sends without bound: `service.rs` reads into an
  `mpsc::unbounded_channel()` (`read_frames`) and never waits on the loop applying them, so a hostile
  or third-party server sending `Records` faster than the ledger takes them grows a phone's memory
  without end; a bounded channel lets flow control hold the server back
- [S · 0.12.1] A volume's `.Trash-$uid` is used as found: `trash.rs`'s `made()` creates it with
  `DirBuilder::recursive(true).mode(0o700)`, which follows a symlink standing there and takes an
  existing folder without asking its owner or mode, so on a shared or second disk another user who
  made the folder first receives every song deleted from that volume and its `.trashinfo`; refuse
  unless it and its `files` and `info` are real folders owned by the uid with no group or other bits
  (`Owner::keeps_alone`, as the spool does), which the *no trash is no deletion* rule then keeps
- [S · 0.12.1] The Last.fm API key is logged: `LastfmHistory::asked` (`history.rs`) puts `api_key` in
  the query, and `json_within` and the busy retry log the whole URL at debug (`client.rs`), so a log
  shared with a bug report carries it, where `online.md` withholds every `lastfm-` key; log the host
  and op alone
- [S · 0.12.1] A redirected POST carries the contact to another host: `Sending::Post` (`client.rs`)
  sends the introduced User-Agent (the contact, for MetaBrainz hosts such as ListenBrainz) with the
  agent's own redirects on, while only `got` follows redirects by hand and drops the contact off the
  host; `max_redirects(0)` on a post, as `upload_file_to` has, and a POST twin of
  `a_contact_is_not_carried_through_a_redirect_to_another_host`
- [S · 0.12.1] A staged copy holds a private file's bytes under the umask until its mode is copied
  over: `copied_into` (`writing.rs`) and organise's `copied_whole` write the whole track into a
  `Held::made` file made `0644` and only then `set_permissions`, so a `0600` song in a folder others
  may list is readable while it is copied, and a reader that opened it keeps it; make the stage
  `0600` and widen it after
- [S · 0.12.1] `carries_secret` (`providers/tidal/src/hifi.rs`) takes everything before the first `/`
  or `?` as the host, so `http://localhost:1@other.example/` reads as localhost and would send the
  bearer key in the clear to another host; latent while `with_secret` is handed only `OWN_SERVER`.
  Parse with `http::Uri`, refuse userinfo, compare the host exactly
- [S · 0.12.1] Investigate: the MusicBrainz export's download agent (`download.rs`) is not
  `https_only`, and `SHA256SUMS` comes over the same agent as the archives, so a redirect to
  `http://` would let the network replace both; the online client is `https_only` already
- [S · 0.12.1] Investigate: the shared-list page's CSP is `script-src 'unsafe-inline'` with no
  `frame-ancestors` (`pages.rs`), while the page holds the list's read key in `location.hash`; every
  output is `textContent` today, so nothing runs, but hashes of the fixed scripts and
  `frame-ancestors 'none'` would keep one future HTML sink from mattering
- [S · 0.12.1] Investigate: one host on the network can hold every media-server answer slot:
  `ANSWERS_AT_MOST` (16) with `A_LISTENER_WAITS_AT_MOST` (120 s) a write and no share a peer
  (`offered.rs`, `door.rs`), so a control point that asks for 16 songs and never reads leaves the
  rest `503`
- [S · 0.12.1] Investigate: the Spotify sign-in's loopback listener (`spotify_account.rs`) answers one
  connection at a time and waits `A_REQUEST_ARRIVES_WITHIN` (2 s) a read, so a local process or a
  browser's idle pre-opened socket can hold the redirect back until the sign-in lapses; PKCE keeps the
  token safe, this only delays it

## Sync and together
- [S · 0.12.1] The desktop names a party request by its position, not its `RequestId`:
  `Together::play_request_next(at)` and `dismiss_request(at)` (`resonate-ui`'s `together.rs`) carry
  the index the row was drawn at, *Play next* runs on the background executor (`party.rs`), and
  `party_room.rs` looks `asked_for.get(at)` up only then, so a request arriving or taken in between
  has the host play or dismiss another; `party.md` settles a request by its `RequestId`, never an
  index, as the phones do
- [S · 0.12.1] A kept MusicBrainz answer stamped in the future is never replaced: `Kept::take`
  (`kept.rs`) drops only answers too old and `written` refuses to overwrite a row whose `at` is as new,
  so one device whose clock runs ahead pins every answer it shares on every device, past the 30
  days `musicbrainz.md` allows and over this device's own fresh lookups; clamp `told.at` to now
- [M · 0.12.1] An answer consulted from another device is kept as this device's, fresh: `layered.rs`
  keeps a `peers.consult` answer through `kept_here` (`Kept::keep`, `at = now()`, no `came_from`), so a
  29-day-old answer becomes new here and is told back to the device it came from, whose older copy it
  replaces; two devices keep a stale answer alive for good, and the never-tell-back rule fails on this
  path. `consult` should answer the original `at` and the device
- [S · 0.12.1] The server's gateway cache drops a fill on a short first read: `gateway.rs` clears
  `source.fill` where `at == 0` and the chunk does not start with `FLAC_MARKER`, so a first read of one
  to three bytes leaves a valid FLAC uncached and fetched again on every ask
- [S · 0.12.1] Investigate: restarting sync can leave it off with nothing said: `released` waits
  `RELEASED_WITHIN` (5 s) for its own handle while `AskedOfDevices` and the window's `with` calls hold
  clones, so the old `Running` still holds `sync.lock`, the new start's `alone()` meets `WouldBlock`
  and answers `Ok(None)` logged at debug; `Syncing::leave` can also take the service while the watcher
  is inside `arranged`, which then installs a fresh `Running` after the leave (`sync.rs`)

## Playback and output
- [S · 0.12.1] A format change mid-track stalls playback for good when the chain holds more than the
  ring: `swap_chain` (`reshaping.rs`) parks in `settles_into` until `has_room_for_what_the_chain_holds`
  (`producer.free_frames() >= max_flush_frames`), the ring is sized from `max_process_frames` and the
  buffer (500 ms by default) and never from the flush, and a convolver's flush is its impulse plus a
  partition (up to `LONGEST_IMPULSE`, 10 s), so an MP3 turning mono to stereo, a chained Ogg or a
  radio stream under room correction or Very high stops writing until a seek; where the flush can
  never fit, drain as the gapless path does
- [M · 0.12.1] Across a gapless turn under a resampler or a carried convolver, the leaving track's last
  frames play at the entering track's ReplayGain: `retune_for_the_entering` (`boundary.rs`) sets the new
  gain at once, `turn_at` takes `frames_rendered_ahead_of_the_ring` (only what lies after the gain
  stage) while the boundary is at `written + chain.latency_frames()`, and the ring steps rendered and
  heard level on one frame, so what the resampler still held (about 300 ms at Very high) is heard at
  the next track's gain: a step on an album played at another rate under track gain
- [S · 0.12.1] A refused handover leaves the incoming zone's repeat changed: `Zones::hand_over` sends
  `SetRepeat(state.repeat)` and returns on the `Resume` request's `?` without `put_back`, which every
  later failure calls, against `audio.md`'s *as it stood before the handover*
- [S · 0.12.1] Investigate: PipeWire's reshape stores `taking` and calls `update_params` before
  `handing.hand(Feed::new(..))` (`client.rs`), so where `param_changed` settles `Taken::AsAsked` before
  the data thread takes the feed, one quantum of the old ring is written at the old stride into the new
  format with a wrong `chunk.size`
- [S · 0.12.1] Investigate: lossy restoration carries one track's learnt wall into the next across a
  gapless turn: two unstudied tracks of one codec tuning plan alike (`wall: None`), `same_shape_as`
  holds and only the gain is retuned, so a 128k MP3 after a 320k one is extended from the 20 kHz wall
  until `follow_the_wall` agrees; reset what `Restore` learnt on a turn, or never keep a restoring
  chain across rows

## Catalog
- [S · 0.12.1] One silent track keeps a whole album from a measured album gain: `album_loudness`
  (`studies.rs`) answers `None` where any row's `loudness` is NULL, and its `LEFT JOIN` cannot tell a
  track never studied (`s.track_id IS NULL`) from one studied whose every block was gated out, so a
  pregap or a hidden-track spacer leaves the album on track gain with nothing said, where `audio.md`
  asks only that every track be studied
- [S · 0.12.1] Organise across volumes unlinks the source before the folders it made are on disk:
  `renamed_onto` makes the destination's folders with `create_dir_all` and `settle` syncs only each
  file's own parent, so after a power cut the unlink can stand while `Artist/Album` does not, and the
  song is in neither place (`organise.rs`); sync the parent of every folder in `made`, deepest first,
  before `left_behind`
- [S · 0.12.1] A WAVE whose ID3 chunk sits before the RIFF is staged beside the file alone:
  `FileTags::write` (`writing.rs`) calls `Staged::beside` and answers `Error::Io` on a refusal, where
  every other path stages under `spool::kept_under` instead, so a writable song in a folder the
  listener may not create in is never tagged, against `library.md`'s *a folder the listener may not
  create in does not refuse a file they may write*
- [S · 0.12.1] The trash can overwrite a file already trashed and does not keep its record: `trash.rs`
  checks the landing name with `symlink_metadata`, then `fs::rename`s over whatever appeared there,
  and neither the `.trashinfo` nor the `files` and `info` folders are synced, so a crash can leave a
  song in the trash nothing restores; `unreplacing::renamed_over_nothing` and the syncs
- [S · 0.12.1] A drop-in's staging name has no counter: `staged_beside` (`take_in.rs`) names it
  `.<name>.<pid>.resonate-part` under `Held::taken_over`, so two drops of one name into one folder in
  one process collide (the second `Unwritable`), and on a filesystem refusing `flock` both write it
- [S · 0.12.1] A playlist exported over a dangling symlink replaces the link with a file: `landing_of`
  (`sheet.rs`) answers the path as given on `NotFound`, which a dangling link answers too, against
  *a symlinked `.m3u` stays a link*
- [M · 0.12.1] Investigate: a retag's follow takes its size and mtime from an `fs::metadata` after the
  read-back (`retag.rs`), so another program's edit landing in between is stored as seen and the next
  scan never reads it; `TagSink::write` could answer the stamp it landed
- [M · 0.12.1] Investigate: a same-length in-place tag write lands with no journal where the folder
  refuses one (`overlay.rs`: only a length change falls back to `TooWide`), so a crash part way leaves a
  torn header nothing mends; keep the journal under the spool as whole copies do, or decide the trade
- [S · 0.12.1] Investigate: the portal trash opens the file with `OFlags::PATH | OFlags::CLOEXEC`
  and no `NOFOLLOW` (`resonate-mpris`'s `trash.rs`), so trashing a song that is a symlink trashes its
  target
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

## Providers and network
- [S · 0.12.1] A long `Retry-After` is cut to 10 s before the pacer sees it: `named_cooling_off`
  (`client.rs`) caps it at `RETRY_AFTER_AT_MOST` before `heard` and `backed_off` record it, so the
  branch ending a call past `PACING_WITHIN` and keeping the host's cooldown (`online.md`'s *a later
  call never asks early*) is reached by its test alone, and a 429 asking for an hour is asked again
  every 10 s; keep the uncapped cooldown for the host and cap only the sleep in the call
- [S · 0.12.1] ListenBrainz history paging drops listens at a page's edge: `history.rs` sends a full
  page's earliest `listened_at` back as `max_ts`, which ListenBrainz reads as exclusive, so listens
  sharing that second past the 1 000th (a bulk import) are skipped
- [S · 0.12.1] TIDAL's device sign-in panics on a huge `interval`: `sign_in.rs` makes
  `Duration::from_secs(interval)` unchecked and `every += self.slowed_by` on `slow_down` overflows,
  against `providers.md`'s *a number the service gives is checked before it is added to*
- [S · 0.12.1] A MusicBrainz replication packet is read whole and cut silently: `download.rs` reads
  `take(LARGEST_PACKET)` (1 GiB) into a `Vec` with no read one byte past, so a packet too large
  surfaces as a bzip2 or parse error, after up to a gigabyte is held on the follow thread
- [S · 0.12.1] Investigate: a resumed or seeking download counts decoded bytes and asks for encoded
  ranges: every agent has ureq's `gzip`, and `ranged.rs` and `seeking.rs` send `Range: bytes=<read>-`
  from what was decoded, so a Subsonic or Jellyfin server (or a proxy) that gzips audio resumes at the
  wrong offset; ask `Accept-Encoding: identity` on a ranged request

## The window
- [M · 0.12.1] Playlist edits by position from the keys and a drag are not checked against what was
  drawn: key Delete (`acting.rs`), a drag's move and gather (`panes.rs`) and a drop (`landing.rs`) send
  positions down the `_edit` chain, run later against whatever the catalog then holds, where only the
  row menu's remove asks `entries_are` (`ui.md`), so a sync merge or an edit landing first moves or
  deletes the wrong rows; a check at the press, better a stamp the library refuses inside its
  transaction
- [S · 0.12.1] Sorting the queue sends its order without the engine's revision guard:
  `queue_put_in_order` (`queue.rs`) compares the polled revision, then plain `send`s `Command::Order`,
  so MPRIS, `resonate queue`, a device driving the zone or a party guest editing in between has the
  sort shuffle the wrong rows; `put_the_queue_back`'s `Insert` likewise; `send_by_position_at` is the
  guard
- [S · 0.12.1] A resize within `WINDOW_SIZE_SETTLE` (400 ms) of closing is not kept: the task waiting
  to store it is dropped with the root, and `on_release` and `on_app_quit` keep only the volume
  (`made.rs`)
- [S · 0.12.1] The sleep timer's countdown is `{minutes}:{ss}` (`format::counting_down`), so ninety
  minutes reads `90:00` where `ui.md` says every clock is `elapsed::clock` (`1:30:00`); the countdown
  or the rule moves

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

## The binary
- [S · 0.12.1] Investigate: a rotated TIDAL refresh token is written blind across processes:
  `Renewed::note` (`providers.rs`) overwrites `tidal-refresh-token` without reading the file again, so
  the window and `resonate poll` started with one token each rotate it and the last write wins;
  Spotify's holder reads the file again when its mtime moves (`binary.md`)

## macOS
- [S · 0.12.1] A panic in a press handler unwinds into Objective-C: `notices.rs`'s `did_receive` and
  `now_playing.rs`'s `RcBlock` handlers call the listener's closure with no `catch_unwind`, where
  `listening.rs` guards its nudge, so a panic there ends the window
- [S · 0.12.1] Investigate: `buffers.rs`'s `rendered` destructures `mBuffers` without asking
  `mNumberBuffers >= 1`, so a list of no buffers would be written through a stale pointer on the
  realtime thread
- [S · 0.12.1] Investigate: `NowPlaying::drop` removes every remote-command target in the process
  (`removeTarget(None)`, posted to the main queue), so a `NowPlaying` made before the last one's drop
  lands loses its handlers; one transport thread a service keeps it latent

## Equaliser
- [S · 0.12.1] One NaN or infinity silences crossfeed for the chain's life: `lowpassed` (`crossfeed.rs`)
  zeroes only values under the denormal floor, so a NaN state stays NaN and an infinity becomes one,
  mixed into both outputs; float sources are widened without cleaning (`buffer.rs`) and the chain is
  kept across gapless tracks, so one bad sample is silence until a seek. The equaliser's `usable` and
  `a_sample_that_is_not_a_number_does_not_poison_the_history` are the fix and its test
- [S · 0.12.1] A binaural pair is kept by `write_beside` (`views/settings/equaliser.rs`): no fsync, no
  folder sync and a `binaural.<pid>.new` left on a failed write or rename that nothing sweeps, where
  `config.rs` and `launcher.rs` hand-write the whole stage, sync and rename each; one
  `write_atomically` would serve the three
