---
paths:
  - "crates/resonate-sync/**"
  - "crates/resonate/src/sync.rs"
  - "crates/resonate-library/src/synced.rs"
  - "crates/resonate-library/tests/synced.rs"
  - "crates/resonate-ui/src/views/settings/sync.rs"
  - "crates/resonate-android/src/sync.rs"
  - "fuzz/fuzz_targets/sync.rs"
---

# Device sync

`resonate-sync` keeps a listener's devices in step: playlists, track favourites, listening history
and what each device was playing travel through a server that cannot read them; songs travel from
one device to another directly. It reaches `resonate-core` alone among workspace crates (the
layering job refuses the library, engine, gpui, `ureq`, serde); the binary, Android and
`resonate-server` reach it.

## The pairing key and what it derives

- **One random key is the account.** `SyncKey` = 20 bytes from `ring`'s `SystemRandom`; its
  `PairingCode` is Crockford base32 (32 symbols) plus 3 check symbols (low five bits of the first
  three SHA-256 bytes), written in groups of five. Parsing drops dashes and spaces, ignores case and
  reads `I`/`L` as `1`, `O` as `0`; a wrong length, symbol or check is a typed `CodeRefused`.
  `Debug` prints `<withheld>`; `config.rs` withholds `sync-key`.
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
  numbers; every length is weighed against what is left before anything is allocated; text is at
  most `LONGEST_TEXT`; a frame ends exactly where it says (`Malformed::Trailing`). Frames are a
  4-byte big-endian length and at most `LARGEST_FRAME` (32 MiB) on one bidirectional QUIC stream.
  `fuzz/fuzz_targets/sync.rs` decodes every frame and bundle and requires a decoded one to read back
  as itself.
- **A bundle's changes are each length-prefixed**, so a change of a later build's tag is skipped,
  not refused (`a_bundle_of_every_change_reads_back_and_an_unknown_change_is_skipped`).
- **To the server**: `Hello` (version, space, token, device, cursor), `Append` (sealed blobs),
  `Compact` (through, snapshot parts), `Punch` (to a device, sealed candidates), `Bye`. **From
  it**: `Welcome` (devices online, the address it saw, head), `Snapshot`, `Records` (a device's
  own records come back as a seq with no blob, so its cursor moves without the bytes), `Appended`,
  `Arrived`/`Left`, `Punched`, `Compacted`, `Refused`. ALPN `resonate-sync/1` to the server,
  `resonate-peer/1` between devices.

## The ledger

`ledger.rs` keeps `sync.sqlite` (the data folder on Linux, app-private storage on Android),
migrated like the catalogs (`MIGRATIONS`, never edited). All merging lives here, once for both
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
- **Songs a device lacks stay in the playlist it edits.** The ledger keeps every entry and whether
  it is held here; `merge::spliced` puts unheld entries back after the nearest earlier held one the
  edit kept (or where it stood, or at the head). A song arriving later is written in place by
  `settle`.
- **Two playlists of one folded name meeting for the first time become one** (adopted, the local
  entries the other lacks appended). An arriving name another playlist holds lands as the catalog's
  free alias (`Mix (2)`); the alias is never sent back as a rename.
- **A tombstone never takes a favourite the ledger had not met**: the first word of a favourite on
  a device is its own.
- **Remote listens are credited where the song is held** (`Catalog::credit`), else kept and
  credited when it arrives; they are never sent on to a scrobbling service (the library's
  `listens.elsewhere`, Android's likewise). The ledger ages listens past `LISTENS_KEPT_FOR`.
- **A snapshot is the merged state**, cut into bundles of at most `BUNDLE_BYTES`; a device compacts
  the server's log past `COMPACT_PAST_RECORDS` (512) or `COMPACT_PAST_BYTES` (4 MiB) since the last
  snapshot, and a `Compacted` from any device resets every device's count.
- **Entering another space forgets the last one** (`enter_space`), so everything local is owed again.

## The seam

`Catalog` is what a platform implements: playlists read and written (no undo step), favourites,
listens after a mark, a remote listen credited, the resume state, whether and how a song is held,
opening one to send and landing one received, and a `revision` that moves with any of these.
Failures are a `Refused` the implementation logs (errors.md: no boxed error). The binary's
`Catalogued` sits over `Library` (`resonate-library`'s `synced.rs`: lists only, saved queries stay
local; a recording resolves by MBID, else folded artist, title words and album with lengths within
2 s; a remote playlist write moves `modified` past any undo step's `left_at` and drops those steps,
`undo::overtaken`). A song from another device is filed in the music folder under `organise-as`
like a delivery (`filed::landed_beside`) and its root scanned; with no music folder it is refused.

## The service

- **One thread, one current-thread tokio runtime, one UDP socket** (`service.rs`). The quinn
  endpoint is client to the server and server to peers at once. The server connection keeps alive
  every 25 s, so presence and the NAT mapping hold for a few dozen bytes; nothing is polled: the
  server pushes what other devices append.
- **Unreachable is quiet**: backoff from 5 s doubling to 30 minutes with jitter, logged at debug;
  a refusal waits the longest. `Shown` (standing, devices, transfers, owed) is a snapshot the
  window and Android read without asking the thread.
- **A look runs a second after the catalog's revision moves, three seconds settled, and every five
  minutes regardless**; it settles held entries first, then pushes what is owed, one `Append` in
  flight. Catch-up comes before the first look, so a device joining adopts before it mints.
- **Server**: `HOSTED_SERVER` (`resonate.tecnio.dev`, port 443) unless `sync-server` names
  another; `sync-server-certificate` adds a PEM to the system and built-in roots (a private CA, the
  tests' self-signed server).

## Peers

- **The server introduces, the devices connect.** `Punch` carries the asker's LAN candidates sealed;
  the server hands the asked device the asker's observed address and those candidates; the asked
  device dials them and punches back with its own; both dial every candidate at once and the first
  connection wins. A loopback candidate is offered only where the server itself is on loopback.
- **Every device of a space presents one certificate, and a handshake is verified against the
  space's key, never the certificate.** `PeerIdentity` builds a self-signed Ed25519 certificate
  from the `peer` seed (rcgen, `ring`); `SpaceVerifier` accepts any presented certificate but checks
  the TLS 1.3 signature with the space's public key, both ways (mutual TLS). A device without the
  code cannot complete a handshake.
- **Songs move only when asked or as chosen**: `fetch` (a playlist's missing songs from one device,
  from the CLI, the playlist menu or Android), or `sync-songs` (`favourites`, `playlists` the
  listener marked *Keep its songs here*, `everything`) when a device arrives, at most every 30
  minutes per device. Only whole-file, unvaulted songs are offered. A transfer is staged under
  `sync-staging` and resumes from what it holds; the catalog lands it once whole.
- **Android sends and fetches songs only on an unmetered network** unless mobile data is allowed
  (`allow_transfers`); incoming peers are refused meanwhile.
