# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [S · 0.8.1] A `resonate://shared/…?server=` link opened from outside joins at once
  (`together.rs` `accepted`, a recorded decision) whatever server it names (`invitation.rs`), so a
  link clicked in a browser has the desktop reach a stranger's server every five minutes; ask before
  joining a server other than the default, a sheet of its own as `party_offer.rs` is
- [S · 0.8.1] A shared list's op log is unbounded in memory: `SharedList` keeps every `Added` row
  in its map though `rows()` shows `ROWS_AT_MOST` (10 000), so a hostile writer within the server's
  64 MiB space limit still costs every follower the whole map; drop the rows past the cap
  deterministically as the log is folded
- [S · 0.8.1] A guest from a build older than the party seat can still be taken over: the room binds
  a member's id to the seat it first joined with, and a member that never sends one is bound to
  nothing

## Sync and together
- [S · 0.8.1] A held listen's handle goes stale once its song is deleted mid-play: `deleted.rs`
  moves the song's listens into `unheld_listens` (keeping `listen_id`), but `Library::listened` with
  `Listen::Held(id)` updates `listens` alone, so the time heard after the delete is lost; it wants
  to fall back onto `unheld_listens.listen_id`
- [S · 0.8.1] A command sent to a driven device carries the queue revision of the controller's
  latest mirror when it leaves, not the one the front end drew (`resonate-driven`), so a mirror
  landing between a draw and a press lets a stale *Remove* through; the revision wants passing from
  the row drawn
- [S · 0.8.1] A shared playlist reads one clean close as `ServerTooOld`, as a party did:
  `shared/service.rs` ends on `Ok(Ok(None))` in the heads read and on a `None` in the close path,
  where a tunnel or a draining server sends a WebSocket `Close`; take several in a row before any
  answer, as `CLOSES_BEFORE_WELCOME_FOR_TOO_OLD` does
- [S · 0.8.1] Two presses of *Play next* on one party request while its search runs queue it twice:
  `take_a_request` takes the request named, but nothing marks it taken until the search lands

## Playback and output
- [S · 0.8.1] A row failing before its frames have passed the chain's latency still fades the
  leaving song's tail out: the gapless handover's failure waits for the graph only where the
  boundary is wholly written (`engine.rs`), so a song failing in its first milliseconds takes the
  old path
- [S · 0.8.1] A seek in a fetched song asks each range with `Ranged::opened_from` (`Seeking`), which
  carries no validator or total to weigh, so a file replaced between two seeks is joined old to new
  with no `If-Range` and no `Changed` guard; carry the first answer's validator and total into it

## Catalog
- [S · 0.8.1] Lengthening a file in place still cannot see a reader root runs: `held_open_elsewhere`
  (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding the file except for a process
  whose `status` names root, since every root process would otherwise end the in-place path; a write
  lease (`F_SETLEASE`) would see it, and wants a safe wrapper the codec may call

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

## The phones
- [S · 0.8.1] An Android read holding a `MobilePlayer` taken before `access` (`pagedTracks`, the
  poll, a `LibraryFeatures` `work` block) may still run against a closed native object; only the
  queued transport actions are guarded, by `OpenEngine` (`LibraryRepository.kt`)
- [S · 0.8.1] A mixed queue another Android controller sets still loses its served rows:
  `handleSetMediaItems` keeps the queue the engine holds when it names the same rows, but any other
  mixed list goes through `BrowseIds.playable`, which drops each served row; it wants a boundary
  call taking library and served rows together
- [L · 0.8.1] The phones keep no shuffle, list order or rows played next: `save_zone` keeps the
  rows, the row and the place, and `resume_saved_in` sends `..Resumption::default()`
  (`resonate-mobile` `lib.rs`), so a queue shuffled when the app was ended comes back in its
  shuffled order with shuffle off and the list order gone, where the desktop keeps all three; the
  phones' `resume` and `zone_resume` want them through a migration step
