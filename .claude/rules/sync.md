---
paths:
  - "crates/resonate-sync/**"
  - "crates/resonate/src/sync.rs"
  - "crates/resonate-library/src/synced.rs"
  - "crates/resonate-library/tests/synced.rs"
  - "crates/resonate-ui/src/views/settings/sync.rs"
  - "crates/resonate-mobile/src/sync.rs"
  - "fuzz/fuzz_targets/sync.rs"
---

# Device sync

`resonate-sync` keeps a listener's devices in step: playlists and saved-query playlists, track favourites, listening history
and what each device was playing travel through a server that cannot read them, or straight between
devices on one network when the server is away; songs travel from one device to another directly, or through the server's relay where no direct path opens.
It reaches `resonate-core` alone among workspace crates (the layering job refuses the library,
engine, gpui, `ureq`, serde); the binary, the mobile boundary (Android and iOS) and
`resonate-server` reach it.

## The pairing key and what it derives

- **One random key is the account.** `SyncKey` = 20 bytes from `ring`'s `SystemRandom`; its
  code (`SyncKey::code`) is Crockford base32 (32 symbols) plus 3 check symbols (low five bits of the first
  three SHA-256 bytes), written in groups of five. Parsing drops dashes and spaces, ignores case and
  reads `I`/`L` as `1`, `O` as `0`; a wrong length, symbol or check is a typed `CodeRefused`.
  `Debug` prints `<withheld>`; `config.rs` withholds `sync-key`. **The code travels as a link
  too**: `SyncKey::link` is `PAIRING_LINK` (`resonate://pair/`) plus the code; `from_link` reads it
  or a bare code (`a_pairing_link_reads_back_as_its_key_and_a_bare_code_reads_too`). The
  desktop draws it as a QR code (`ui.md`); the iPhone and Android open it from the system camera through
  their `resonate` URL scheme (`ios.md`, `android.md`), so no app of ours asks for the camera.
- **HKDF-SHA256 (salt `resonate-sync/1`) derives everything, one `info` each**: `space` (16 bytes,
  the `SpaceId` the server files records under), `token` (32 bytes, the bearer the server keeps
  the SHA-256 of), `seal` (ChaCha20-Poly1305 key), `peer` (an Ed25519 seed). The server sees a
  space id, the token's digest, random `DeviceId`s, sealed blobs and addresses: no name, no code,
  no title. A new code is a new space; the old one ages out on the server.
- **A record is sealed after deflating**: `seal::sealed` deflates (flate2, pure Rust), seals under a
  random 96-bit nonce with the space id as associated data, and prefixes the nonce. `opened` caps
  the inflated size at `LARGEST_OPENED` (64 MiB). A blob that does not open is logged and passed
  over, its seq still counted.

## The wire

- **Hand-encoded, no serde** (`wire.rs`): `Writer`/`Reader` with LEB128 lengths and zigzag signed
  numbers; every length is weighed against what is left before anything is allocated, a count
  against the least each item takes (`Reader::count_of`), so a frame read before the token is checked
  cannot make the server hold many times its size; a sealed blob shorter than nonce and tag
  (`SEALED_AT_LEAST`, 28) is `Malformed::Short`; text is at most `LONGEST_TEXT` (`Writer::text` cuts a longer one on a letter, so what a device writes it can read back); a frame ends exactly
  where it says (`Malformed::Trailing`). Frames are a
  4-byte big-endian length and at most `LARGEST_FRAME` (32 MiB) on one bidirectional QUIC stream.
  `fuzz/fuzz_targets/sync.rs` decodes every frame and bundle and requires a decoded one to read back
  as itself.
- **A device left unnamed is called `<host> (<platform>)`** (`Me::called`: a chosen name wins, else the host and `Platform::label`, else the platform's own word). The desktop's host is the kernel's hostname, Android's the user's device name (`Settings.Global.device_name`, else `Build.MODEL`) through `sync_device_called`; iOS withholds the device's name, so an iPhone stays `iPhone`.
- **A device names its platform in one byte**: Linux 0, Android 1, iOS 2, other 255; a tag a later
  build adds reads as `Other` (`every_platform_reads_back_and_a_later_one_reads_as_other`), and the
  ledger keeps the same codes.
- **A bundle's changes are each length-prefixed**, so a change of a later build's tag is skipped,
  not refused (`a_bundle_of_every_change_reads_back_and_an_unknown_change_is_skipped`). **A saved
  query is `Change::Query` (wire tag 6)**: key, stamp, then a body (name, optional text, sort code,
  reading code, optional row cap) or none for a tombstone; a build without it skips it as it skips any
  later tag. The two codes are the library's `SortOrder::code` and `Direction::code` (`coded` reads
  them back; `store::sort_code` and `direction_code` are the same numbers), which the desktop and the
  phones both store and so are never renumbered. One cost of that tolerance: a build without tag 6 that compacts the
  server's log writes a snapshot lacking the queries. **A device hearing a `Compacted` it did not ask
  for owes every query again** (`Ledger::owe_queries`) and pushes at once, so the records after the
  snapshot carry them to a device joining from the server alone
  (`saved_queries_compacted_away_by_a_build_without_them_are_told_to_the_server_again`).
- **A server address of `ws://` or `wss://` runs the same frames over a WebSocket** (`web.rs`: TCP,
  TLS through `tokio-rustls` for `wss`, one binary message a frame, at most `LARGEST_FRAME`),
  for a server behind a proxy that carries no UDP (`server.md`). Any other address is QUIC. The
  link holds a `Carrier` (a QUIC connection, or a numbered web link) in place of a connection; the
  device pings every 25 s and a link silent for 90 s drops. A web link has no observed UDP address,
  so the server's `Welcome` and `Punched` carry `0.0.0.0:0`, which a device skips as a candidate.
- **To the server**: `Hello` (version, space, token, device, cursor), `Append` (sealed blobs),
  `Compact` (through, snapshot parts), `Punch` (to a device, sealed candidates), `Relaying`,
  `Relay` (to a device, one datagram of at most `LARGEST_DATAGRAM`, 2048 bytes), `Bye`. **From
  it**: `Welcome` (devices online, the address it saw, head), `Snapshot`, `Records` (a device's
  own records come back as a seq with no blob, so its cursor moves without the bytes), `Appended`,
  `Arrived`/`Left`, `Punched`, `Compacted`, `Refused`, `Relayed` (from a device, one datagram). ALPN `resonate-sync/1` to the server,
  `resonate-peer/1` between devices.
- **Between devices**, one request per stream: `Holdings` (answered `Held`… `HeldEnd`), `Fetch`
  (`Sending` then the bytes, or `Missing`), `Meet` (device, whether records travel; answered `Met`
  with the other's device), `Changes` (whether whole, a bundle; answered `Taken`) and `Tags` (a
  recording; answered `Tagged`, a `SongTags`). **What a song's file no longer carries travels beside
  it, not in it**: a `Recording` is never extended (every earlier build reads each frame to its end
  and refuses a trailing byte), so the album artist, date, track and disc numbers and totals and
  the compilation flag go in a request of their own, asked after a fetch lands (`peer::tags_from`)
  and handed to `Catalog::land`. A build that does not know the request (`ToPeer` tag 4) drops the
  stream, which reads as `Missing` and so as no tags, and a build asking an older sender fetches as
  it always did (`the_tags_of_a_song_travel_in_frames_an_earlier_build_refuses_whole_and_the_fetch_is_unchanged`,
  `a_song_fetched_from_a_device_arrives_with_the_tags_that_device_declares_for_it`). A `Beacon` is
  a device, its QUIC port, whether it answers another and the addresses it may be dialled at.

## The ledger

`ledger.rs` keeps `sync.sqlite` (the data folder on Linux, app-private storage on Android),
one `V1` in a `MIGRATIONS` list that holds it alone: until the 0.1.0 release `V1` may be edited, after it
a change appends a step and never edits `V1`; a stamp past the list is `Error::LedgerVersion`. All merging lives here, once for both
platforms; the catalog seam stays thin.

- **Stamps are last-writer-wins on a hybrid clock**: `Stamp { nanos, device }` ordered by both;
  `Clock::now` is never below the last stamp seen, so a device whose clock runs behind still writes
  after what it has read.
- **A look diffs the catalog against what was last synced, so no catalog mutator knows about
  sync.** Playlists: a local playlist is `(id, created)` (undo re-inserts under one id and SQLite
  reuses discarded ids); a digest of name and entries (`merge::projection_digest`) moved means an
  edit, stamped and owed; a mapped playlist gone locally is a tombstone. Favourites: `here` notes
  what the catalog showed at the last look or apply, so a change there is the listener's. Listens:
  past a mark, this device's own alone. The resume state: a digest of rows, place and position to
  the ten seconds.
- **An arrival first looks at what the ledger already holds.** `arrived` begins with a look limited
  to rows with a stored counterpart (`Looking::AtWhatIsHeld`: a mapped playlist or query edited or
  gone, a favourite changed), before the clock has seen any arriving stamp, so an edit made while
  the link was behind is stamped by its own time and competes with the older note instead of being
  overwritten by it. Nothing is minted or adopted by it: that stays the first full look, after
  catch-up (`a_playlist_edited_before_its_first_look_outranks_an_older_note_that_arrives`).
- **Songs a device lacks stay in the playlist it edits.** The ledger keeps every entry and whether
  it is held here; `merge::spliced` puts unheld entries back after the nearest earlier held one the
  edit kept (or where it stood, or at the head). A song arriving later is written in place by
  `settle`.
- **A saved query travels as its text, evaluated by each device against its own catalog.** Its own
  `queries` table beside `playlists`, same key type, stamp, alias and tombstone rules, looked at
  the same way: a digest of the body (`merge::query_digest`) moved means an edit, stamped and owed; a
  mapped query gone locally is a tombstone. A query has no entries, so there is no `here` and nothing
  to splice or fetch. Two queries of one folded name meeting for the first time become one with the
  **synced body standing** (the local text is replaced, not stamped: the account's definition was
  there first). A list and a query of one name stay two, the catalog's free alias keeping them
  apart. A body the receiving catalog cannot read (a sort or reading code a later build added) is
  kept in the ledger and passed on, written nowhere (`Refused::Unwritable`), and tried again when a
  newer note arrives and at every `settle`. **A catalog that keeps no queries**
  (`Catalog::keeps_queries`, false by default; the desktop and both phones keep them) never looks at
  queries and stores and forwards every note untouched, so a snapshot through it still carries them.
  **A note held but written nowhere is written by `settle`** (`settle_queries`: every query row with
  no local id and no tombstone, once the catalog keeps queries), adopting an unmapped local query of
  the same folded name with the synced body standing, as the first look would, so a phone that
  passed queries on before it kept them holds them after the upgrade without waiting for an edit
  (`a_device_that_begins_keeping_queries_takes_up_those_it_passed_on_and_adopts_its_own_namesake`).
- **Two playlists of one folded name meeting for the first time become one** (adopted, the local
  entries the other lacks appended). An arriving name another playlist holds lands as the catalog's
  free alias (`Mix (2)`); the alias is never sent back as a rename.
- **A tombstone never takes a favourite the ledger had not met**: the first word of a favourite on
  a device is its own.
- **A favourite is one song however each device names it.** Rows are keyed by `Recording::key()`
  (`mbid:` or `named:`), but a look matches the catalog's favourites to held rows by
  `names_the_same_song_as` (the exact key first, then the named key): a copy with an MBID on one
  device and without on another is one favourite, and a held row is tombstoned only where no
  favourite here names its song
  (`a_favourite_on_a_copy_known_by_its_mbid_and_one_known_by_its_tags_stays_favoured`).
- **Remote listens are credited where the song is held** (`Catalog::credit`), else kept and
  credited when it arrives; they are never sent on to a scrobbling service (the library's
  `listens.elsewhere`, Android's likewise). The ledger ages listens past `LISTENS_KEPT_FOR`, and a
  listen older than that arriving again (an old snapshot) is passed over, never credited twice.
  **A credit is idempotent by the listen's origin**: `RemoteListen` carries the sending device and the
  listen's id, and each catalog keeps them with the row it writes under a unique index
  (`origin_device`, `origin_id`), so `Catalog::credit` of a listen already credited changes nothing and
  answers true; the ledger's `credited` mark may therefore lag the catalog (a crash between the two) and
  `settle` only repeats a no-op (`a_listen_credited_before_the_ledger_could_say_so_is_credited_once`).
- **A snapshot is the merged state**, cut into bundles of at most `BUNDLE_BYTES`; a device compacts
  the server's log past `COMPACT_PAST_RECORDS` (512) or `COMPACT_PAST_BYTES` (4 MiB) since the last
  snapshot, and a `Compacted` from any device resets every device's count.
- **Owed is cleared by the server's `Appended`, or by a look where there is no server.** With no
  server (`Server::Nowhere`), what a look told its neighbours is marked sent, since a device met
  later is told the whole snapshot; `state.serverless` remembers it, and the first start with a
  server again owes everything (`Ledger::through_a_server`), so the server is given the whole state.
  A `Welcome` whose head is below this device's cursor (a space swept and made anew) resets the
  cursor and owes everything (`space_made_anew`, `Error::SpaceMadeAnew`), and the link is made again.
- **Entering another space forgets the last one** (`enter_space`), so everything local is owed again;
  *regenerate* is just a new pairing code: each front end mints or clears the key and starts again.
- **A device is forgotten by a tombstone, last writer wins like any note.** `Ledger::forget_device`
  (`Sync::forget`, refused for this device and for one already gone) sets `devices.gone`,
  restamps the row, owes it and drops the device's handoff; it travels as `Change::Forgotten` (wire
  tag 5, which a build without it skips), and a snapshot carries every tombstone, so neither an old
  note nor another device's snapshot brings the device back. `devices()` and `handoffs()` leave the
  forgotten out. **A forgotten device that still syncs speaks up**: hearing its own tombstone it
  restamps and owes its own note, which outranks the tombstone, so only a device that is truly gone
  stays off the list
  (`a_forgotten_device_leaves_every_list_and_its_old_note_does_not_bring_it_back`,
  `a_device_forgotten_while_it_still_syncs_speaks_up_and_is_listed_again`). The front ends offer it
  only for a device that is not online.

## The seam

`Catalog` is what a platform implements: playlists read and written (no undo step), saved queries
read and written (`queries`, `write_query`; removal is `remove_playlist`, one id space), favourites,
listens after a mark, a remote listen credited, the resume state, whether and how a song is held,
opening one to send and landing one received, and a `revision` that moves with any of these.
Failures are a `Refused` the implementation logs (errors.md: no boxed error). The binary's
`Catalogued` sits over `Library` (`resonate-library`'s `synced.rs`: `synced_playlists` reads lists
only and `synced_queries` the saved queries, a query written over a list or the reverse made anew
rather than overwritten; a recording resolves by MBID, else folded artist, title words and album with lengths within
2 s; a remote playlist write moves `modified` past any undo step's `left_at` and drops those steps,
`undo::overtaken`). **A song from another device lands where a provider's delivery does: in the open
vault, else the music folder.** With a vault open (`SongFiled::Vaulted`, `synced::vaulted`, even
where a music folder is set): `Vault::keep_delivered`, then `Library::note_received` writes the
`vault_objects` row (`taken_from` = `devices:<recording key>`) and a rootless `tracks` row named by
the recording (title, artist, MBID, `track_total`) with what the file declared (genre, lyrics,
ReplayGain, numbers), as a provider's delivery lands (`providers.md`). **Its album is grouped as a
scan would group the declared tags** (`store::album_received_in`, over the `grouping_keys` the scan
uses): an `ALBUMARTIST` the file declares (unless a compilation) owns the album and keys it, else
the recording's artist; year, barcode, catalogue number, label and `TRACKTOTAL` fill the album as
a scan fills one, a differing owner disowns it
(`songs_from_another_device_group_by_the_album_artist_and_year_their_files_declare`). **A vaulted
sender says what its vault dropped**: `Library::song_tags` reads the row as the stand-in does
(`vaulted::declared_by`: the catalog's album artist, date and numbers, the `kept_tags` totals and
compilation flag), `Catalog::tags` hands it over the wire, and the receiver's `HeldTags::fill`
lays it under what the delivered file declares itself (a declared tag wins), so a third device
groups the song as the first did
(`songs_passed_on_by_a_vaulted_device_keep_the_album_artist_year_and_totals_the_vault_dropped`).
An unvaulted sender says nothing, its file carrying the tags. Audio the vault holds already under another recording is `Refused`. With no vault, a song is
filed in the music folder under `organise-as` like a delivery (`filed::landed_beside`) and its
root scanned. **A song filed is held by what was asked for, never landed twice**: the catalog's
`songs_received` keeps each recording key a song was filed for (`key` and `named_key`) against its
path, and `synced::resolved` asks it after the MBID and before the tag match, so a sender whose
catalog names the song otherwise than its file does is not fetched again on every follow; a file
of the same bytes already standing at the name (or a numbered one) is the song
(`filed::Landed::Standing`), not a ` (2)` beside it; the scan after filing waits out another pass
(`scanned_once_free`: `SCANS_TRIED_AT_MOST` tries `SCAN_TRIED_AGAIN_AFTER` apart) rather than leave
the song uncatalogued to be fetched again
(`a_song_from_another_device_is_held_by_what_was_asked_and_never_filed_twice`). Copies such a
fetch left are folded by `resonate dedupe` (`library.md`). With neither it is refused (`SongFiled::Nowhere`), and one that does not probe (or the
vault refuses) is deleted and answered `false` (`SongFiled::Refused`), so a damaged file is never
filed to be fetched again. **A vaulted song is offered onward**: `Library::held_songs` lists the
rootless vaulted rows beside the rooted unvaulted ones, and `Library::open_song` opens either;
`Vault::offer` gives the object's bytes (`Offered`, `Read + Seek`) with their size and extension, a
`.wav.zst` object as the WAVE it decodes to (`Unpacking`, extension `wav`), so the receiver's vault
keys the same PCM
(`a_vaulted_song_is_passed_on_to_a_third_device_as_the_bytes_it_decodes_to`). A vaulted object that
will not open is left out of the holdings.

## The service

- **One thread, one current-thread tokio runtime, two UDP sockets** (`service.rs`): the quinn
  endpoint, client to the server and server to peers at once, and the beacon socket (`Nearby`).
  Where Bonjour is on, `mdns-sd` runs a daemon thread of its own beside them.
  `Syncing` is the one state for the whole run; the server is a `Link` in it that comes and goes,
  so looks, peers and beacons carry on while the server is away. Connecting runs as a task handing
  back a `Link`. The server connection keeps alive every 25 s, so presence and the NAT mapping hold
  for a few dozen bytes; nothing is polled: the server pushes what other devices append.
- **Unreachable is quiet**: backoff from 5 s doubling to 30 minutes with jitter, logged at debug;
  a refusal waits the longest; a link made resets it, so a drop after a long outage retries in 5 s.
  `Shown` (standing, devices, transfers, owed) is a snapshot the window and the phones read without
  asking the thread; `owed` is read again after every `Appended`.
- **A record the server would refuse is never sent to it.** A bundle sealed past `LARGEST_RECORD`
  (256 KiB, the server's default `--record-bytes`) is marked sent with a warning and reaches devices
  on the network in the snapshot; a `TooLarge` refusal from a server with a lower limit marks the
  largest bundle of that append sent and pushes again, so one large playlist never holds up the rest.
- **A look runs `LOOK_AFTER` (300 ms) after the catalog's revision moves, watched every
  `WATCH_EVERY` (500 ms), and every five minutes regardless**, and at once whenever one falls due (a
  neighbour's whole snapshot, a link caught up, *force*): the loop sleeps until the next look or
  follow falls due (`next_due`) and checks after every event; it settles held entries first, then tells neighbours and pushes what is
  owed, one `Append` in flight. Catch-up comes before the first look, so a device joining adopts
  before it mints: caught up with the server (this link, or an earlier one while none stands) or
  with a whole snapshot from a neighbour. A device that has met nobody does not look; there is
  nobody to tell. `Shown.settled` asks the same, and nothing in flight to the server.
- **The server never holds the loop.** Its connection is bounded: resolving, connecting, opening the
  stream and the `Welcome` each get `SERVER_ANSWERS_WITHIN` (15 s, else `ServerSilent`), and every
  frame to it goes through a writer task fed by a channel (`Link::say`), so a server that stops
  reading stalls nothing: neighbours, beacons and looks carry on, and a failed write comes back as
  `Event::Dropped` and unlinks.
- **Force** (`Sync::force`, *Sync now* on the desktop pane and the iPhone's sync page, `sync_force`
  on the boundary) resets the backoff, drops and redials the server, clears the throttles on
  dialling and on following a device, dials every device heard, tells each neighbour the whole
  snapshot again and looks at once.
- **A song deleted by hand is not held.** `Catalog::holds` is false for a row whose file is gone
  (the desktop stats the path; the phones open the document), so the next follow or force fetches
  it again. `Ledger::wanted` therefore asks `holds` of every entry of a followed playlist and every
  favoured song, never trusting `here`, which only says the catalog listed the song at the last look
  (`a_song_deleted_by_hand_is_wanted_again_while_its_playlist_and_favourite_stay`).
- **`Shown.transfers` keeps at most `TRANSFERS_SHOWN_AT_MOST` (200)**, the oldest landed or
  failed dropped first. **A follow lists every song it will fetch as `Waiting` at once**
  (`Running::queued`, a finished or failed row of the same song becoming `Waiting` again), not as
  each one is reached, so `Shown::waiting` is how many are queued and `Shown::downloading` how many
  move; the apps say both (`SyncStatus::downloading`, `waiting`). A connection that closes turns the
  rest to `Failed`, as before.
- **Server**: `Settings::server` is a `Server`: `Hosted` (`HOSTED_SERVER`, `wss://resonate.tecnio.dev/`, a web socket through Cloudflare Tunnel,
  **the default on every front end**; the one host `resonate_core::RESONATE_HOST` names serves the hifi gateway too, `server.md`),
  `At` another (`sync-server`; blank is the hosted one; `ws://`/`wss://` for a web
  socket), or `Nowhere`, the listener's
  *This network only* (`sync-nearby-only` on; off by default): nothing is dialled, the standing is `Standing::Nearby` from the start, and
  devices meet only as *Nearby* says. `sync-server-certificate` adds a PEM to the system and built-in
  roots (a private CA, the tests' self-signed server).

## The desktop continues a handoff

- **A handoff names the song and the place in it** (`Handoff::{rows, at, position}`: the queue, the
  row playing and the time into it). The desktop (`continuing`) resolves the rows from `at` on and,
  where the row playing is held here, turns `position` into frames at that file's rate
  (`Frames::from_duration`); a row not held here, or no rate for it, continues from the start of
  the first song that is. The window (`Syncing::continue_from` answers `Continuing { cuts, at }`)
  and `resonate sync --handoff` (`Handed`, `Opening::place`) load the songs, autoplaying, and then
  send `Command::Seek(at)`, which the engine keeps as the place to open at; a seek that fails
  is logged and the song plays from its start. The phones do the same through `Command::Resume`
  (`resonate-mobile`'s `continue_from`).
- **A handoff is cut from the play order, in a window holding the row heard.** Both devices
  hand off the rows as they play (the desktop's load order mapped through `Resumption::in_play_order`,
  the phones' queue as saved), at most `HANDED_OFF_AT_MOST` (500) of them, chosen by
  `Resumed::window_around` so the playhead is inside however far down the queue it is
  (`a_shuffled_queue_is_handed_off_in_the_order_it_plays_at_the_song_heard`,
  `a_queue_longer_than_a_handoff_hands_off_the_songs_around_the_playhead`).

## The desktop follows its settings

- **The settings file is the one way to reconfigure.** `sync::start` returns a `Syncing` over a
  `Service` (the running `Running`, swapped under a lock) and a watcher thread
  (`resonate-sync-settings`) that looks at the file's modified time every `SETTINGS_LOOKED_AT_EVERY`
  (2 s) and, when it moved, reads it again with `config::load`. An `Arrangement` (whether it syncs,
  the code as `SyncKey::code` spells it, the server, certificate, device name and `sync-songs`) is
  compared with the one running; only a difference stops the running service (`released`, waiting
  up to `RELEASED_WITHIN` for a call in flight to let go of `sync.lock`) and starts another. A device
  that is off holds one arrangement whatever else is set, and a key minted at start is kept in the
  arrangement (`Arrangement::keyed`), so the write that records it is not a change. A file that will
  not read leaves what runs as it is. So the pane, `resonate sync --join` and `--leave` from another
  process, and a hand edit take effect within seconds, and *New code* (the pane clears `sync-key`)
  mints and joins a new space at once.
- **The window's seam never goes absent.** `Service` implements `resonate_ui::Syncing` over
  whatever runs, answering `SyncStanding::Waiting` and nothing else while none does (just switched
  on, or another Resonate holds `sync.lock`). It also serves headless `resonate play` with a catalog,
  which starts the same watcher.

## Peers

- **`sync-stun` names a STUN server** (`host` or `host:port`, 3478; blank asks none, the default,
  since a request names this build and nothing else). The peer endpoint's socket is wrapped
  (`stun::Reflector`, an `AsyncUdpSocket` over the runtime's) so a binding request goes out of the
  very port peers are dialled on and its answer is taken off the stream before QUIC sees it (a
  datagram whose top two bits are 00 and carries the cookie is STUN; QUIC's fixed bit makes its
  first byte at least `0x40`). The address it names joins the candidates of every punch, so a
  device reached through a server it cannot see UDP from (a web link) still offers its public
  address; on a QUIC link it is added beside what the server saw. A web link's local candidates are
  the interfaces' IPv4 addresses (`addresses_here`) at the peer port.
- **The server introduces, the devices connect.** `Punch` carries the asker's LAN candidates sealed;
  the server hands the asked device the asker's observed address and those candidates; the asked
  device dials them and punches back with its own; both dial every candidate at once and the first
  connection wins. A loopback candidate is offered only where the server itself is on loopback. A
  punch is answered only where no live connection to that device stands and none was answered
  within `PUNCH_ANSWERED_AT_MOST_EVERY` (10 s), so two devices never punch each other in a loop. A
  punched connection is watched for closing as a met one is, so a dead one leaves `peers`.
- **Every device of a space presents one certificate, and a handshake is verified against the
  space's key, never the certificate.** `PeerIdentity` builds a self-signed Ed25519 certificate
  from the `peer` seed (rcgen, `ring`); `SpaceVerifier` accepts any presented certificate but checks
  the TLS 1.3 signature with the space's public key, both ways (mutual TLS). A device without the
  code cannot complete a handshake.
- **Peers are dialled over IPv4 and IPv6 alike.** The peer endpoint's socket is dual stack
  (`service::peer_socket`: `[::]` with `IPV6_V6ONLY` off, falling back to IPv4 alone where the host
  has no IPv6), so one port serves both and quinn maps an IPv4 address to its IPv6 form. A punch
  offers, beside the IPv4 candidates, up to `GLOBAL_ADDRESSES_OFFERED_AT_MOST` (4) IPv6 addresses
  the interfaces hold (`nearby::global_addresses_here`: up, not loopback, not point to point, not
  link local, not multicast; a unique local address counts, like a private IPv4), at the peer port,
  and only where the endpoint took IPv6. `routed_candidates` probes the route to the server in the
  server's own family, and a server seen over IPv6 offers the address it saw. Nearby beacons run over IPv6 too (`Nearby`). The STUN reflector converts an IPv4 server to its mapped form before it
  sends from a dual stack socket (`a_dual_stack_endpoint_is_dialled_over_ipv6_and_over_ipv4_alike`).
- **A device a direct dial cannot reach is reached through the server's relay, as QUIC.** The peer
  protocol is untouched: `Reflector` (the endpoint's socket wrapper) owns a `Relay` that gives each
  device a synthetic IPv4 address in `240.82.0.0/16` (reserved, never routed) and, in `try_send`,
  turns a datagram to such an address into `ToServer::Relay` instead of a UDP send; a
  `FromServer::Relayed` is queued by `read_frames` and handed to quinn by `poll_recv` as received
  from that address. So the handshake, mutual TLS against the space's key, fetch, resume and the
  rest run over the server's link exactly as over UDP, **end to end: the server relays ciphertext it
  holds no key to and cannot impersonate a device** (`SpaceVerifier`). `Event::Punched(device, None)`
  (every direct candidate failed) dials the relay address within `RELAY_DIAL_WITHIN` (20 s) only
  where a song is waiting on the device and a server link stands (`Event::Relayed` then lands as a
  punch does); each side dials for its own fetches, as it does directly. Datagrams leave through a
  bounded queue (`RELAYED_DATAGRAMS_QUEUED_AT_MOST`, 256) beside the link's writer, which serves the
  ordinary queue first (`biased`), and a full queue drops: QUIC reads that as loss and slows. A
  device sends `Relaying` after every `Welcome`; the server relays only to a device that did, so an
  older build is never sent a frame it cannot read. `Settings::relay_only` clears every direct
  candidate (the tests' way to take the relay; false in the front ends)
  (`a_song_reaches_a_device_the_other_can_only_reach_through_the_servers_relay`,
  `two_endpoints_reach_each_other_over_relayed_datagrams_alone_and_pass_a_song_between_them`).
- **Songs move only when asked or as chosen**: `fetch` (a playlist's missing songs from one device,
  from the CLI, the playlist menu or a phone), or `sync-songs` (`favourites`, `playlists` the
  listener marked *Keep its songs here*, `everything`) when a device arrives, at most every 30
  minutes per device, **and again `FOLLOW_AGAIN_AFTER` (250 ms) after records arrive** from the
  server or a neighbour, since a device meeting another asks before that one's favourites and
  playlists have reached it (`a_favourite_learned_after_meeting_is_fetched_without_a_restart`). A
  look follows again too, at most every 60 s. Following again asks only connected devices, works out
  what is wanted here before listing the other's holdings (nothing wanted, nothing asked), and under
  `everything` runs at most every 10 s, since that lists the whole of the other's holdings. A song is
  fetched by one transfer at a time (`Running::fetching`), whichever device and follow asked, and
  **asks `Catalog::holds` again once claimed**: follows overlap, each listing what was wanted when
  it began, so a song an earlier follow landed meanwhile is passed over (`held_already`), never
  fetched twice (on a phone a second landing took `Name (1)` beside the first); the passed-over run's
  `Waiting` row is taken off the list (`Running::unlisted_waiting`), the claiming one reporting the
  song. Only whole-file songs are offered (a rooted file, or a vaulted object as `Vault::offer` gives it). A transfer is staged under
  `sync-staging` and resumes from what it holds; the catalog lands it once whole.
- **One broken song never holds the others up.** Every wait of a fetch (the stream, the header, each
  chunk) gives up after `A_FETCH_GOES_QUIET_AFTER` (30 s), so a sender stuck on an evicted file fails
  that song and the run goes on to the next. **A partial is resumed only from the file it began as**:
  a `.from` note beside it names the sending device and the size it announced; another device, or a
  size the header no longer names, starts the song over (`peer::resumable`, `fetched_from`). **A song
  the catalog refused waits before a follow asks for it again**: a landing answered `false` or
  refused, and a phone's refused landing (`Sync::refused`), go into `Running::failed_lately`, and
  follows pass the song over for `A_FAILED_SONG_WAITS` (30 min); an asked fetch (`Work::Fetch`)
  tries it at once and *Sync now* forgets them all. **A fetch cut off by the network is no verdict
  on the song**: it never enters `failed_lately`, and a run whose connection closes stops there
  (the songs it had not reached are not failed one by one) and sends `Event::CutShort`, which lets
  the next meeting with that device follow at once rather than after `FOLLOW_A_DEVICE_EVERY`.
- **Android and iOS send and fetch songs only on an unmetered network** unless mobile data is allowed
  (`allow_transfers`); incoming peers are refused meanwhile, a run under way stops before its next
  song as a cut-short run does, and a `Fetch` asked meanwhile is answered `Missing`
  (`Answering::transfers_allowed`, the flag the service holds).

## Nearby

The server is the way across networks; on one network devices find each other themselves
(`nearby.rs`), so a listener with no server reachable, or none at all, still syncs at home. Two ways
announce a device, both carrying the same sealed beacon, and either finds the others:

- **A beacon is sealed like a record.** `RSN1` then `seal::sealed` of a `Beacon` under the space's
  key: only a device of the code opens it, and a fresh nonce each time leaves an onlooker nothing to
  follow. It names the QUIC port and the addresses `beacon_addresses_here` reads off the interfaces that
  are up, not loopback, not point to point: at most `ADDRESSES_BEACONED_AT_MOST` (6) IPv4 and
  `IPV6_ADDRESSES_BEACONED_AT_MOST` (2) of the global or unique local IPv6 ones, which together still
  fit one Bonjour text string even when distinct
  (`the_fullest_record_of_distinct_addresses_still_fits_one_bonjour_text_string`). A link local
  address is never named: it means nothing without the interface, which the datagram it came in
  (or the Bonjour answer) carries.
- **Broadcast** (`Broadcast`): sent at start and every `BEACON_EVERY` (15 s, jittered) to
  `NEARBY_GROUP` (239.255.41.73, one hop), the broadcast address and, over IPv6, `NEARBY_GROUP_V6`
  (`ff02::7273:a306`, link local scope), port `NEARBY_PORT` (41734). **IPv6 has a socket of its
  own** (`bound_v6`: `IPV6_V6ONLY`, the IPv4 socket's port where free, joined to the group on every
  interface holding an IPv6 address; none where the host has no IPv6, and IPv4 carries on alone), and
  a group is told once per such interface with its index as the scope (`told_over_ipv6`), so a network
  handing out IPv6 alone is found. A beacon heard over IPv6 is dialled where it came from with its
  scope kept, which is how a link local peer is reached, and an IPv6 candidate is dropped where the
  peer endpoint took IPv4 alone. Where the port is taken the socket binds anywhere: it still sends,
  and hears answers; an answer goes out of the socket of the family it was heard on
  (`two_devices_told_of_each_other_over_ipv6_alone_meet_and_sync`). **An unmet
  device answers by unicast**, so one that cannot hear multicast (a taken port, an Android without
  its lock) is still found.
- **Bonjour** (`Bonjour`, `mdns-sd`): the device advertises `_resonate-sync._udp` under a random
  instance name for the run, the hex of the sealed beacon as TXT key `b` (`RECORD_KEY`; the fullest
  record fits one TXT string, `the_fullest_record_fits_one_bonjour_text_string_and_reads_back`), and
  browses the same type, taking every address an answer resolves (IPv4 and IPv6, a link local one
  with its interface index as scope). The record is resealed only where the beacon it carries moved
  (`Running::record`), checked at every beacon tick; withdrawn while transfers are not allowed.
- **An iPhone announces through Apple's Bonjour, not ours.** Multicast and broadcast from an app's
  own sockets need Apple's `com.apple.developer.networking.multicast` entitlement; Bonjour through
  Network.framework does not (`NSBonjourServices` in `Info.plist` and the Local Network permission).
  iOS runs with `Nearby::told_by_the_platform()` (no broadcast, no `mdns-sd`); Swift's
  `NearbyDevices` advertises `Sync::nearby_record` with an `NWListener` and hands every record its
  `NWBrowser` finds to `Sync::nearby_heard`, again every 5 s while it stands.
- **The lower `DeviceId` dials at once, the higher after `DIAL_BACK_AFTER` (500 ms) if they have
  not met by then**, so a device only one side can hear (Bonjour seen one way, an iPhone's record
  handed to one device) is still met (`a_device_meets_another_that_only_it_has_heard_of_whichever_dials_first`).
  Every address heard is dialled at once (where a datagram came from, where Bonjour resolved it,
  what the beacon names). A failed dial is tried again by itself after `FIRST_REDIAL` (1 s),
  doubling to `LAST_REDIAL` (30 s), while the device was heard within `HEARD_FOR` (60 s); a
  neighbour whose connection goes is dialled again at once. A want for a device heard within
  `HEARD_FOR` dials it too. Nothing is sent, heard or dialled while transfers are not allowed.
- **A dial is mutual TLS like any peer, then `Meet`**: the certificate is the space's, so it says
  nothing of which device is there. Both sides then serve the connection (fetch either way), call it
  nearby (`SyncDevice::nearby`, also `online`), and each sends its whole `Ledger::snapshot` as one
  `Changes { whole }`; the other applies it through `arrived`, as a server snapshot. A punched
  connection does not meet and carries no records.
- **After every look, what is owed goes to each neighbour**, less what it was told on this
  connection (`told`: SHA-256 of each encoded change). `owed` is cleared only by the server's
  `Appended`, so the server still gets all of it once back. Nothing is relayed: what came from the
  server or from one neighbour is not passed to another, since each device on the network meets each
  other one.
- **Android holds a `WifiManager.MulticastLock` while sync runs** (`CHANGE_WIFI_MULTICAST_STATE`),
  for the beacons and `mdns-sd` alike.
- `Settings::nearby`: `Nearby::on_the_network()` on Linux and Android (broadcast and Bonjour),
  `told_by_the_platform()` on iOS; `None` meets nobody here (the server's tests).
  `crates/resonate-sync/tests/nearby.rs` pairs two devices over loopback beacons with no server
  (records both ways, a song fetched, a rename back), and two told `Server::Nowhere` (standing
  `Nearby`, owing nothing once told), over records handed across as Swift hands them,
  and over real Bonjour; a beacon or record of another code meets nobody. The last two skip on a
  machine with no IPv4 interface but loopback.
