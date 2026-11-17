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
- [S · 0.12.1] The server still counts connections and attempts per /64 alone (`guard.rs`), so one /48
  may open all 2 048 connections; the wider prefixes the daily allowances now weigh, for connections too
- [S · 0.12.1] The bytes read with a session's first `Hello` are not drawn from the frame budget: bounded
  by the first frame's ceiling and one 16 KiB read, so 2 048 half-greeted sessions hold that beside it
- [S · blocked: design] A host on the network may take several addresses and so pass the media server's
  per-neighbour share of its answers (`door.rs`); nothing on a LAN names a host better than its address

## Sync and together
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
- [S · 0.12.1] Lyrics on a UPnP renderer light up to a second late: the heard position follows the
  renderer's `RelTime`, which most renderers report in whole seconds, so a line waits for the next
  whole second; interpolate between reports from the clock the stream is paced by

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
- [S · 0.12.1] A whole-library lookup stopped by a metered connection tells "MusicBrainz couldn't be
  reached": the library's lookup summary says how many were answered and not why the pass stopped,
  so the window cannot tell a shut gate from an unreachable service
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

## Providers and network

## The window
- [S · 0.12.1] Lifted rows dropped into the queue at a row (`LiftedTo::Queue(Placement::At)`) send a
  plain insert, and a queue drag's carried rows are not checked against the revision they were lifted
  at, so an edit landing between the lift and the drop places or moves the wrong rows;
  `send_by_position_at` as the sort and put-back now use

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

## macOS
- [S · 0.12.1] `NowPlaying::drop` still clears the process's now-playing information and sets the state
  stopped (`now_playing.rs`), so a `NowPlaying` made before the last one's drop lands is shown as
  stopped with nothing playing; clear it only where this owner set it last

## Equaliser
- [S · 0.12.1] A binaural pair's stage left by a crash (`write_atomically`'s staged name beside the
  pair) is never swept: only the settings file's stage has a sweeper

