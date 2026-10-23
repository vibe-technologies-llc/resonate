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

- **SQLite, three tables** (`store.rs`, one `V1` in `MIGRATIONS`: editable until the 0.1.0 release, a step appended after it): `spaces` (id, the SHA-256 of
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

## The hifi-api gateway (`keys.rs`, `gateway.rs`)

`--config <TOML>` with a `[hifi]` table (`listen`, `upstream` as socket addresses, `keys` as SHA-256
hex digests) opens an HTTPS gateway on TCP, beside QUIC on UDP (same certificate, ALPN `http/1.1`),
in front of a hifi-api the operator runs on `upstream` (plain HTTP, expected on loopback). No table
leaves it off; a table with no key, or a key that is not a digest, is a startup error
(`ConfigMissing`/`ConfigInvalid`). `resonate-server --hifi-key` mints a secret (32 random bytes,
hex) and prints it with the digest to list: the file never holds a secret.

- **Everyone without a listed key is turned away alike**: no `Authorization: Bearer`, another scheme
  or an unlisted secret is 401 before method or route is looked at, so a stranger learns nothing of
  the routes (`a_request_without_a_secret_or_with_one_not_listed_is_turned_away_and_never_forwarded`).
- **A listed key reaches three routes**: `GET /search/`, `GET`/`DELETE
  /playback/requests/<letters, digits, dashes>` (forwarded to the hifi-api) and `GET /file/?id=<digits>`;
  else 404, another method 405. `/track/` is not one: the manifest names TIDAL's CDN, which a client
  never reaches itself (`a_listed_secret_reaches_only_the_hifi_routes_and_methods`).
- **The secret stops at the gateway**: a forwarded request goes upstream as HTTP/1.0 with the target
  and a `Host`, no `Authorization`, no client header; the answer's status, `Content-Type`, digit-only
  `Retry-After` and body (at most 1 MiB, so no chunking to read) go back. An upstream that is down,
  silent for 30 s or unreadable is 502. At most 64 connections are served at once, each one request
  (`Connection: close`).
- **The server downloads, the client is only sent the file.** `/file/` runs `HifiApi::track` against
  the upstream (manifest, DASH segments and remux are `resonate-tidal`'s, `providers.md`), then sends
  the FLAC with `Content-Length` where known, else chunked, 64 KiB at a time over a channel of four
  so a slow client holds the download back. Two downloads at once (`DOWNLOADS_AT_ONCE`), at most 1 GiB
  a track, 60 s for any one write. A failure after the head is sent drops the connection, so a short
  body is never taken for a whole file; a track the hifi-api lacks is 404, an upstream failure 502. The
  key check comes first: nothing is asked of the hifi-api for a request without a listed key
  (`a_track_is_never_downloaded_for_a_request_without_a_listed_key`,
  `a_track_is_downloaded_by_the_server_through_the_hifi_api_and_sent_to_a_listed_key`).

## Deployment

`packaging/server/resonate-server.service`: `DynamicUser`, `StateDirectory=resonate-server` (the
store), `CAP_NET_BIND_SERVICE` for UDP 443 and the usual hardening; the certificate chain and key
are handed in as systemd credentials from `/etc/resonate-server/fullchain.pem` and `privkey.pem`
(a root-owned Let's Encrypt key a dynamic user could not read); `resonate-server.env` sets `LISTEN`,
`RUST_LOG` and `SERVER_ARGS` (empty; `--config /etc/resonate-server/config.toml` turns the gateway on). SIGINT and SIGTERM close the endpoint and wait for it to go idle. The same directory holds the Arch and Fedora recipes that install the binary, the unit and
`resonate-server.env` (`packaging.md`).
