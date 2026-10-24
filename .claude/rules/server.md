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
- **A space untouched for `--idle-days` (365) is swept** (checked hourly, or every thirtieth of the
  limit if shorter). **A space never written to is swept after a day** (`Store::sweep_unused`:
  `head` and `snapshot_through` both 0), so claims made by a script and abandoned do not pile up.

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
  and its sealed candidates. A device on a web socket has no observed UDP address: the server
  files it as `0.0.0.0:0`, the asked device skips an unspecified address, and the candidates (its
  interfaces and the address STUN gave it) are all it dials (`sync.md`).

## Limits

`--record-bytes` (`LARGEST_RECORD`, 256 KiB a blob, `TooLarge`; devices send nothing larger), `--space-bytes` (64 MiB a space, `SpaceFull`),
`--frames-per-minute` (240 a connection, `TooFast` and the session ends). Errors are typed
(`errors.md`); a session's failure is a debug line, never the server's.

There is no account or e-mail to tie a space to, so anyone can claim spaces; the limits are what
keeps a script from filling the store (`guard.rs`, `Store::limited`):

- **Per address** (an IPv6 `/64` counts as one, an IPv4-mapped IPv6 address as its IPv4):
  `--connections-per-address` (64 open at once), `--attempts-per-minute` (30 connections a minute,
  however briefly each lasts) and `--spaces-per-address-per-day` (5 new spaces, refused `TooFast`;
  a connection to a space that exists is never counted). A connection over either is dropped
  before a byte is read.
- **Everyone together**: `--connections` (2048 open), `--spaces` (10 000, refused `Busy`) and
  `--total-bytes` (16 GiB over all spaces, an append past it `SpaceFull`). The per-space limits
  above still hold under them.
- **QUIC proves the address first**: an `Incoming` whose address is not validated is sent a retry,
  so a spoofed source cannot spend an address's allowance or the server's connections.
- **A web socket's address is the proxy's header where one is trusted** (below), else the peer's.
  The address table holds at most 100 000 addresses and forgets a quiet one after a day; the
  counters live in memory, so a restart forgets them.

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

## The web socket listener (`web.rs`, `serve_web`)

QUIC cannot cross a proxy that carries only HTTP, so `--config` with a `[web]` table (`listen`, and
optionally `forwarded_by`, a header name) also accepts the same frames over WebSocket on TCP, one
binary message a frame (`resonate-sync`'s `Outgoing`/`Incoming`, which hide which of the two a
session runs on). The sessions, limits, store and hub are the same code for both. No table leaves
it off.

- **Plain TCP, no TLS of its own**: it sits behind Cloudflare Tunnel (`cloudflared` connects out
  to it, so no inbound port opens and the origin address is never in DNS) or another proxy that
  terminates TLS. Bind it to loopback. Devices use `sync-server = "wss://sync.example.org/"`.
- **`forwarded_by = "CF-Connecting-IP"`** makes the address the allowance counts the first one in
  that header, and a connection without a parseable one is dropped, so only the proxy's path works.
  Left out, the peer address is counted, which behind a proxy is the proxy itself: set it.
- At most 256 upgrades are in flight at once and the upgrade and `Hello` share the 10 s allowance.
  The server pings every 30 s and a reader silent for 90 s ends the session.
  `a_listener_behind_a_proxy_turns_away_a_connection_the_proxy_did_not_forward`.

## Deployment

**The hosted server is one hostname behind one tunnel** (`resonate.tecnio.dev`, `resonate_core::RESONATE_HOST`).
`cloudflared` ingress sends `^/(search|playback|file)/` to the gateway's TLS listener
(`https://127.0.0.1:443`, `originServerName` the host, so the Let's Encrypt chain is verified) and
every other path to the web socket listener on loopback, so sync and the hifi gateway share an
address and no inbound port is open. The old `sync.tecnio.dev` name is not served.


`packaging/server/resonate-server.service`: `DynamicUser`, `StateDirectory=resonate-server` (the
store), `CAP_NET_BIND_SERVICE` for UDP 443 and the usual hardening; the certificate chain and key
are handed in as systemd credentials from `/etc/resonate-server/fullchain.pem` and `privkey.pem`
(a root-owned Let's Encrypt key a dynamic user could not read); `resonate-server.env` sets `LISTEN`,
`RUST_LOG` and `SERVER_ARGS` (empty; `--config /etc/resonate-server/config.toml` turns the gateway on). SIGINT and SIGTERM close the endpoint and wait for it to go idle. The same directory holds the Arch and Fedora recipes that install the binary, the unit and
`resonate-server.env` (`packaging.md`).
