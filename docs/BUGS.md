# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Security
- [S · 0.8.1] A shared playlist's editor or a party guest can make every follower or host inflate
  64 MiB a record: `seal::opened` caps every sealed blob at `LARGEST_OPENED` whatever it carries,
  though an honest bundle is at most `LARGEST_BUNDLE` and `BUNDLE_OPS_AT_MOST` ops, and
  `shared/service.rs` clones the whole list (up to 40 000 entries) for each record it opens; the
  party host opens every guest's `RoomHeard` under the same cap. `opened` wants a ceiling per
  caller, the ops of a record capped, and a change flag from `apply` in place of the clone
- [S · 0.8.1] A host on the network can wedge renderer discovery: `http::exchange`'s 3 s is a
  timeout per read, so an answer dripping a byte every 2.9 s holds a round until the 1 MiB cap,
  though `upnp.md` says an exchange gives up after 3 s; `ssdp::search` keeps every location an
  answer names on its sender's address, uncapped and deduplicated by a linear `contains`, and
  `network.rs` describes them one after another under the round lock; Cast admits every resolved
  record with no cap either (`casting.rs`). One deadline an exchange, and caps on the locations
  kept a sender and on the renderers and receivers admitted
- [S · 0.8.1] The Discord socket is checked by path and then connected by path: `socket.rs`'s
  `open` asks `ours` (`fs::metadata`, which follows a link) before `UnixStream::connect`, and
  `/tmp` is searched, so another local user's `/tmp/discord-ipc-N` link swapped between the two
  reads what plays (`discord.md`'s *Only the listener's own socket*); the peer's uid wants reading
  after the connect (`SO_PEERCRED`)
- [S · 0.8.1] A sync server can make every device dial any address: the observed address a
  `Punched` frame carries joins the candidates before the sealed ones are opened and stays where
  they do not open (`service.rs`'s answer to a punch), and `punched_back` keeps an entry for every
  `DeviceId` the server names; nothing wants dialling unless the candidates open, and only devices
  the ledger knows answering
- [S · 0.8.1] A party guest can drain the host's server allowance: every guest `Hello` has the host
  tell the whole `People` list to everyone (`party/service.rs`), a name may be `LONGEST_TEXT`
  (4 KiB), and the server drops a host's `RoomSay` past 512 KiB a second, so a guest repeating
  `Hello` under up to 32 ids starves the room of its `Now` frames; `People` wants telling again only
  where a name changed, at most every few seconds, and a name cutting short where it is read

## Sync and together
- [S · 0.8.1] A party host on a phone dismisses or plays next by a row's place in a list that
  moves: `party_play_next` and `party_pass_over` take a `u32` and read it against `asked_for` as it
  stands at the press (`resonate-mobile`'s `together.rs`), while iOS's rows are `enumerated()`
  offsets refreshed by a poll (`TogetherViews.swift`, `Together.swift`) and Android's are the same
  (`Together.kt`), so a second dismissal right after a first, or a request arriving between a read
  and a press, acts on another guest's request; `party.md` has a dismissal name its request, never
  an index
- [S · 0.8.1] A party seat keeping its queue ahead edits by place without naming the queue:
  `Seat::keep_ahead` reads `state()` and `queued()` apart and sends `Remove` and `Insert` at
  `first_unlike` through a plain `request` (`seat.rs`), so a skip or an edit landing between the
  reads and the removal takes the wrong rows (after a *Next*, the song now playing and everything
  after it); `request_if_the_queue_is_still` with the revision read, `QueueChanged` read as not kept
- [S · 0.8.1] The relay's synthetic address table grows without end and then shares addresses:
  `relay.rs`'s `address_of` numbers each new `DeviceId` the server names `len + 1`, saturating at
  `u16::MAX`, after which every new device gets one address and datagrams are told as another's;
  only devices the ledger knows or that are online want mapping, the least recently used evicted
  past a cap

## Playback and output

## Catalog
- [M · 0.8.1] Forgetting a folder deletes its vaulted rows and their listens: `remove_root` deletes
  every `tracks` row of the root (`db.rs`), where every other removal leaves a row holding a
  `vault_key` (`store.rs`'s `rooted_paths_at`, `forget_paths`), so the next `vault --prune --apply`
  deletes audio the vault held as its only copy (`vault.md`: a vaulted row outlives its file), and
  the listens cascade with the rows without the `unheld_listens` copy `deleted.rs` makes, leaving
  Statistics, the unfrozen Wrapped years and the scrobbles not yet sent. The window's *Forget*, the
  CLI's `forget` and MCP's `forget_folder` all reach it
- [S · 0.8.1] Online off does not stop a build of the local MusicBrainz copy: the binary's
  `musicbrainz::build` asks `online_enabled` once before a 7.5 GB download, `Mirroring` holds the
  config cloned as the window opened (so *Build it now* reads Online as it stood at start),
  `online::reach(false)` and `follow(false)` cancel nothing, a build landing after the mirror was
  switched off holds it again from its own forced config, and `keep_fresh` asks `LATEST` past the
  metered lookup gate; `online.md` has `online = false` stop every request. `musicbrainz.rs` holds
  no test
- [S · 0.8.1] A moved file can be left under two names: `linked_over_nothing` (`unreplacing.rs`:
  every rename on Windows, and on a filesystem refusing `RENAME_NOREPLACE`) hard-links and then
  unlinks, and an unlink refused (another program holding the file without delete sharing) returns
  its error with the link left; organise's `put_back` covers only the steps pushed, so the next scan
  catalogues a second row. The link wants removing before the error is answered
- [S · 0.8.1] Exporting a playlist over a symlinked `.m3u` or `.xspf` replaces the link with a
  file: `sheet.rs`'s staging renames over the path as given and is held by no `writer::Held`, so a
  stage left by a kill is never swept; the cue-sheet writer canonicalises and holds its stage
  (`sheeted.rs`), and organise's cue-sheet rewrite (`organise.rs`'s `staged_over`) stages with
  `fs::write`, though `rust-style.md` holds every staging file under `writer::Held`
- [S · 0.8.1] A resumed download of the MusicBrainz export shows more read than there is:
  `download.rs` takes the total from `source.length()`, which on a `206` is the bytes still to
  come, while `written` starts at what the part held, so the settings pane reads past 100 %; the
  total is `source.total()`, else the length and what was held
- [S · blocked: a Linux unsafe exception] Lengthening a file in place still cannot see a reader
  root runs: `held_open_elsewhere` (`insert.rs`) counts an unreadable `/proc/<pid>/fd` as holding
  the file except for a process whose `status` names root. A write lease (`F_SETLEASE` with
  `F_WRLCK`, then `F_GETLEASE` after the insert to see whether a break released it) would see it, but
  neither `rustix` nor `nix` wraps leases and the codec forbids `unsafe`, so it wants a small crate
  under an exception of its own beside `resonate-win32` and `resonate-apple`, which is the
  listener's to grant

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
- [S · 0.8.1] Late network answers act on a window that has moved on: *Find another version*
  opens its menu at the press's place whenever the services answer (`better.rs`), as *Place on a
  release…* does (`browser.rs`), and `open_a_menu` clears `naming`, `adding` and `record`, so an
  answer landing while a playlist is named or picked drops it and its held rows; an album or artist
  not held opens its page whenever its lookup lands (`open_album_not_held`, `root.rs`), so of two
  pressed, whichever answers last is shown. A serial taken at the press and moved by every pane,
  sheet and lookup
- [S · 0.8.1] The Match sheet takes an answer asked by an earlier opening: `ask_to_match` starts
  `asked` at 0 each time the sheet opens (`matching.rs`) and a landing compares the track and that
  count alone, so a sheet reopened on the same song before MusicBrainz answers takes the first ask's
  recordings (for the words typed then), and they overwrite the newer where they land after
- [S · 0.8.1] A remote zone's clock shows no hours: `remote.rs`'s `clock` prints `75:03` where the
  Device sync pane prints `1:15:03` for the same zone. The window, the panes and the terminal hold
  some eight clock formatters of their own (`format.rs`'s `whole_seconds` and `spanned`, the
  binary's `played`, `readout.rs`, `sync.rs`, `settings/sync.rs`, `analysis_plot.rs`), and the
  terminal's `ago` stops at days (*400 days ago*) where the window says years (`format.rs`'s
  `since`); one formatter in core would serve both
- [S · 0.8.1] Forgetting the served covers lets fetches already running land: `serving.rs`'s
  `forget_the_covers` clears `fetching` and the cache while detached fetches run, and each landing
  removes a key a newer fetch may hold and keeps a cover from before the reset
