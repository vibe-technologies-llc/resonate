---
paths:
  - "crates/resonate-discord/**"
  - "crates/resonate/src/discord.rs"
  - "crates/resonate-core/src/presence.rs"
  - "crates/resonate-ui/src/views/settings/desktop.rs"
---

# Discord presence

`resonate-discord` shows a running Discord what plays (local IPC socket, *Listening* activity).
Binary only (`discord` feature); `cargo tree -p resonate-discord` stays free of gpui, the library,
`ureq`.

## Off means nothing runs

- **No application id in the build**: `discord` false, `discord-app` empty; `Presence::active` (on
  *and* an application named) is the one gate. The id is the listener's own, like `contact`: a
  request says what this build is, not who runs it.
- **Inactive: no thread, socket or probe. Active but idle: no socket.** `Discord::follow` spawns
  the `resonate-discord` thread on activation, stops it (clearing the activity) on deactivation; it
  connects only once there is something to show.

## The seam

- `resonate-core::presence`: vocabulary (`AppId`, `Icon`, `Shown`, `Pictured`, `Presence`) shared
  by config reader, window and crate, which cannot see each other.
- `resonate_ui::Present` keeps the settings pane off the publisher: the binary's
  `discord::Presenter` implements it, rides into the window on `Stored`; each Desktop group control
  stores its setting and calls `follow` (live). Featureless it is empty and warns while presence is
  active.
- `Releases` finds covers without the crate seeing the library: the binary's `Catalogued` answers
  from the library for the row's release or release group.

## What is sent

- **Frame**: little-endian opcode, length, JSON; at most `LARGEST_FRAME`. Handshake waits for
  `READY` (`READY_WITHIN`); `ERROR` instead is `Refused` at once. `Close` 4000 = unknown
  application: warned once, waited out until the id changes. `Ping` gets `Pong`. `ERROR` to an
  activity keeps the session, counts a refusal; `due` re-offers the same activity after
  `SENT_AGAIN_AFTER_A_REFUSAL` doubled per refusal up to `SENT_AGAIN_AT_MOST`; a changed activity
  goes on normal spacing (`refusals_carried`). Closing would reconnect every few seconds to be
  refused the same payload.
- **Socket search**: `$XDG_RUNTIME_DIR`, `$TMPDIR`, `/tmp`, each plain and under Flatpak, Snap,
  Vesktop sandboxes, `discord-ipc-0` to `-9`, last answerer first. `find_among` passes over a
  socket refusing the client (other `Close` code, handshake `ERROR`, unreadable frame), so a
  sibling or Vesktop beside a refusing Discord is reached; only 4000 ends the search (all Discords
  answer the same id alike). Nothing found: retry after `RETRY_AFTER_AT_FIRST`, doubling to
  `RETRY_AFTER_AT_MOST`; a session reached or lost restarts it.
- **Only the listener's own socket** (`/tmp` is shared; another user could bind `discord-ipc-0` and
  learn what plays): `Session::open` follows links (Flatpak, Vesktop may link), refuses with
  `NotOurs` unless the target is a socket owned by `rustix::process::getuid`; `find_among` warns,
  goes on.
- **`Activity::of` is the whole policy** (pure, tested without a socket).
  - `Shown::Application`: nothing about the track, no cover. `Track`: title over artist. `Album`:
    album as picture caption, or after the artist with no picture.
  - `Pictured::Cover`: `coverartarchive.org/release/<mbid>/front-500` (or group's), icon small
    beside it, icon alone where no release is known. A release the archive said has no front
    (`ReleaseDetail::may_have_a_front`) uses its group (else a blank picture).
    `Pictured::Nothing`: no assets.
  - Bar: `start = now - position`, `end = start + length`; absent when `discord-progress` is off or
    paused. Pause clears the activity unless `discord-paused`.
  - Text cut to `LONGEST_TEXT`, padded to Discord's two-character minimum.
- **Paced**: ticks every second; sends only on change (text, assets, bar appearing/going, start
  moving over `DRIFT_ALLOWED`: a seek), never within `SENDS_APART` of the last (Discord: five per
  twenty seconds); a change inside the window goes when it ends.
- **Release resolved once per track**, only where a cover is drawn: `MUSICBRAINZ_ALBUMID` tag, then
  release group, then `Releases`. Found covers cached by id, location and span together (unscanned
  row ids are re-minted by the next load; two cuts of one file share the rest); a miss retried
  after `COVER_REFRESH_AFTER` (enrichment may land the release mid-track).

## What it does not do

- **No local picture uploaded**: Discord draws only assets uploaded to the application or public
  URLs, so a track with no named release shows the icon; an image host would send the library
  somewhere the listener did not choose.
