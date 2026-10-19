---
paths:
  - "crates/resonate-server/**"
  - "packaging/server/**"
---

# The sync server

`resonate-server` keeps what devices of one space append, hands it to the others and introduces
devices to each other. It reaches `resonate-sync` (the wire, framing, transport) and core alone,
and reads nothing it keeps: records are sealed on the devices (`sync.md`).

## What it keeps

- **SQLite, three tables** (`store.rs`, `MIGRATIONS` never edited): `spaces` (id, the SHA-256 of
  the token, created, touched, bytes, head, snapshot through), `records` (space, seq, device,
  blob), `snapshots` (space, part, blob). Every store call runs on `spawn_blocking` behind one lock.
- **The first token claims a space; any other is refused** (`Claim::Refused` → `Refusal::Token`).
  The digest is compared as stored: a digest of a 32-byte random token leaks nothing by timing.
- **A compaction replaces what it covers and only moves forward**: `through` must be past the
  snapshot held and within the head (`Compacted::Stale` otherwise); records at or below it and the
  old snapshot go in one transaction.
- **A space untouched for `--idle-days` (365) is swept** daily (sooner for shorter limits).

## Sessions

- **A `Hello` within 10 s or nothing**; a wrong version, token or a full space is a `Refused` frame,
  then the stream ends.
- **Catch-up is snapshot then records past the cursor**, in frames of at most 256 records or 4 MiB;
  a device's own records go back as seqs with no blob. Each frame is read through
  `Store::catch_up`, which under one lock answers the snapshot wherever what was sent lies behind the
  snapshot held, so a compaction landing between frames, or while a session is behind on its
  notices, never skips the records it replaced; `Compacted` is told only after the session has
  caught up (`a_reader_behind_a_compaction_is_handed_the_snapshot_rather_than_what_is_left`). After it, every `Append` anywhere in the
  space tells each session to send what lies past what it last sent, so ordering never depends on
  which notice arrived first.
- **Presence is the connections themselves** (`hub.rs`): joining tells the others `Arrived`,
  leaving `Left`; a device connecting again replaces its old session; at most `--devices` (16) at
  once.
- **A punch goes only to a device online in the same space**, carrying the asker's observed address
  and its sealed candidates.

## Limits

`--record-bytes` (`LARGEST_RECORD`, 256 KiB a blob, `TooLarge`; devices send nothing larger), `--space-bytes` (64 MiB a space, `SpaceFull`),
`--frames-per-minute` (240 a connection, `TooFast` and the session ends). Errors are typed
(`errors.md`); a session's failure is a debug line, never the server's.

## Deployment

`packaging/server/resonate-server.service`: `DynamicUser`, `StateDirectory=resonate-server` (the
store), `CAP_NET_BIND_SERVICE` for UDP 443 and the usual hardening; the certificate chain and key
are handed in as systemd credentials from `/etc/resonate-server/fullchain.pem` and `privkey.pem`
(a root-owned Let's Encrypt key a dynamic user could not read); `resonate-server.env` sets `LISTEN`
and `RUST_LOG`. SIGINT and SIGTERM close the endpoint and wait for it to go idle. The same directory holds the Arch and Fedora recipes that install the binary, the unit and
`resonate-server.env` (`packaging.md`).
