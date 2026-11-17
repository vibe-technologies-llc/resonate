# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Android
- [S · 0.12.1] A phone whose Keystore key is lost for good (an `AEADBadTagException` on every read)
  keeps sync unset forever: `SyncSettingsReading` answers `null`, and `newSyncGroup` and `saveSync`
  return on a `null` config, so nothing short of clearing the app's data starts sync again; offer
  starting over with a new group where the settings cannot be read

## iOS
- [S · 0.12.1] Investigate: a refused `setActive(false)` is swallowed (`try?`, `NowPlaying.swift`) and
  `release.owed(by:)` counts the release done, so where RemoteIO still runs as the transport stops
  (`IsBusy`) the session stays active and the audio Resonate interrupted is never told to resume

## Security
- [M · 0.12.1] The web socket path, the one the hosted server is reached by, keeps no `--frame-bytes`
  budget before a message is whole: tungstenite assembles up to `LARGEST_FRAME` (`web.rs`'s
  `limits`) before `hold(length)` is asked, and a text, ping or pong message is read and passed over
  uncounted by the budget and by `--frames-per-minute`, so 2 048 sessions may each hold 32 MiB where
  `server.md` says the budget bounds them; draw on `FrameBytes` as bytes come off the socket, count
  every message, and say in `server.md` what the web path holds
- [M · 0.12.1] One routed IPv6 prefix or a few IPv4 addresses fill the server-wide caps: `guard.rs`
  counts a /64 as one address with 5 spaces, 50 shared spaces and 50 rooms a day, against 10 000
  spaces and 1 024 rooms in all, and a space holding one record outlives the one-day unused sweep
  for `--idle-days`, so a /48 fills the spaces in a day for a year and about 21 IPv4 addresses hold
  every room; aggregate allowances at /56 and /48 (and /24) beside the /64's
- [S · 0.12.1] `carries_secret` (`providers/tidal/src/hifi.rs`) takes everything before the first `/`
  or `?` as the host, so `http://localhost:1@other.example/` reads as localhost and would send the
  bearer key in the clear to another host; latent while `with_secret` is handed only `OWN_SERVER`.
  Parse with `http::Uri`, refuse userinfo, compare the host exactly
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
- [S · 0.12.1] Investigate: restarting sync can leave it off with nothing said: `released` waits
  `RELEASED_WITHIN` (5 s) for its own handle while `AskedOfDevices` and the window's `with` calls hold
  clones, so the old `Running` still holds `sync.lock`, the new start's `alone()` meets `WouldBlock`
  and answers `Ok(None)` logged at debug; `Syncing::leave` can also take the service while the watcher
  is inside `arranged`, which then installs a fresh `Running` after the leave (`sync.rs`)
- [S · 0.12.1] A shared playlist's client sends every owed op in one `Append` (`shared/service.rs`)
  without bounding its blob count by `BLOBS_A_FRAME_AT_MOST` (4 096), so an owner owing about a
  million ops has every append refused; split it as the sync service's compaction is

## Playback and output
- [S · 0.12.1] Investigate: PipeWire's reshape stores `taking` and calls `update_params` before
  `handing.hand(Feed::new(..))` (`client.rs`), so where `param_changed` settles `Taken::AsAsked` before
  the data thread takes the feed, one quantum of the old ring is written at the old stride into the new
  format with a wrong `chunk.size`
- [S · 0.12.1] Under true peak, a gapless turn carrying the resampler or convolver into a new chain lands
  its boundary a guard delay late: the fresh true-peak guard swallows its delay rather than holding
  it, so `written + latency` (and the entering gain step that follows it) is late by that delay
- [S · 0.12.1] An equaliser change spliced in while a gain step still waits (`GainStage`'s waiting
  steps, a few milliseconds after a gapless turn) builds the new chain at the entering gain and drops
  the waiting step, so the leaving tail is heard at the entering track's gain

## Catalog
- [S · 0.12.1] One silent track keeps a whole album from a measured album gain: `album_loudness`
  (`studies.rs`) answers `None` where any row's `loudness` is NULL, and its `LEFT JOIN` cannot tell a
  track never studied (`s.track_id IS NULL`) from one studied whose every block was gated out, so a
  pregap or a hidden-track spacer leaves the album on track gain with nothing said, where `audio.md`
  asks only that every track be studied
- [M · 0.12.1] Investigate: a retag's follow takes its size and mtime from an `fs::metadata` after the
  read-back (`retag.rs`), so another program's edit landing in between is stored as seen and the next
  scan never reads it; `TagSink::write` could answer the stamp it landed
- [M · 0.12.1] Investigate: a same-length in-place tag write lands with no journal where the folder
  refuses one (`overlay.rs`: only a length change falls back to `TooWide`), so a crash part way leaves a
  torn header nothing mends; keep the journal under the spool as whole copies do, or decide the trade
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

## Providers and network
- [S · 0.12.1] TIDAL's device sign-in panics on a huge `interval`: `sign_in.rs` makes
  `Duration::from_secs(interval)` unchecked and `every += self.slowed_by` on `slow_down` overflows,
  against `providers.md`'s *a number the service gives is checked before it is added to*
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
- [S · 0.12.1] A binaural pair is kept by `write_beside` (`views/settings/equaliser.rs`): no fsync, no
  folder sync and a `binaural.<pid>.new` left on a failed write or rename that nothing sweeps, where
  `config.rs` and `launcher.rs` hand-write the whole stage, sync and rename each; one
  `write_atomically` would serve the three
