---
paths:
  - "crates/resonate-discord/**"
  - "crates/resonate/src/discord.rs"
  - "crates/resonate-core/src/presence.rs"
  - "crates/resonate-ui/src/views/settings/desktop.rs"
---

# Discord presence

`resonate-discord` tells a running Discord client what is playing, over Discord's local IPC socket,
as a *Listening* activity. It reaches `resonate-core`, the engine's vocabulary,
`serde`/`serde_json`, `crossbeam-channel` and `parking_lot`; only the binary reaches it, behind
the `discord` feature. `cargo tree -p resonate-discord` stays free of gpui, the library and `ureq`.

## Off means nothing runs

- **The build carries no application id and presence is off.** `discord` defaults to false and
  `discord-app` to nothing; `Presence::active` — switched on *and* naming an application — is the
  one gate everything asks. The id is the listener's own, registered in Discord's developer
  portal, for the reason `contact`, `acoustid-key`, `audd-token` and `listenbrainz-token` are: a
  request says what this build is and nothing about who runs it.
- **Inactive is no thread, socket or probe.** `Discord::new` starts nothing; `Discord::follow`
  spawns the `resonate-discord` thread when presence becomes active and stops it — clearing the
  activity on the way out — when it stops. A run whose file leaves the keys alone never looks for a
  socket.
- **Active and idle is still no socket.** The thread connects only once there is something to
  show, so a window with nothing playing reaches for Discord no more than one with presence off.

## The seam

- `resonate-core::presence` is the vocabulary — `AppId`, `Icon`, `Shown`, `Pictured`, `Presence`
  — since the config reader, the window and the crate all need it and none may reach the others
  (the argument `Appearance` makes).
- `resonate_ui::Present` is how the settings pane reaches the publisher without depending on it
  (as it reaches the file through `Settings`): the binary's `discord::Presenter` implements it and
  rides into the window on `Stored`, and every control in the two Desktop groups writes the
  `ResonateApp::presence` global, stores the setting and calls `follow`, so a change is live.
  Without the feature the `Presenter` is empty and warns once where the file asks for a presence
  the build cannot show.
- `Releases` finds a cover without the crate seeing the library: the binary's `Catalogued` reads
  `track_at` and `release_of` for the row's release or its release group.

## What is sent

- **The frame is Discord's**: a little-endian opcode and length, then JSON, capped at
  `LARGEST_FRAME`. The handshake waits for `READY`; a `Close` carrying 4000 is an application
  Discord does not know, warned about once and waited out until the id changes. A `Ping` is
  answered with a `Pong`; an `ERROR` reply is a `Refused` warning that keeps the session — the
  `Sent` is marked `refused`, so `due` offers the same activity again after `RETRY_AFTER`, and a
  change is sent on the usual spacing. Closing the socket over it would reconnect every fifteen
  seconds to be refused the same payload.
- **The socket is looked for where every Discord puts it**: `$XDG_RUNTIME_DIR`, `$TMPDIR` and
  `/tmp`, each plain and under the Flatpak, Snap and Vesktop sandboxes, `discord-ipc-0` to `-9`.
  One that does not answer is a debug record and a retry after `RETRY_AFTER`.
- **`Activity::of` is the whole policy**, pure and tested without a socket.
  - `Shown::Application` says nothing about the track and draws no cover even where asked; `Track`
    is the title over the artist; `Album` adds the album as the picture's caption, or after the
    artist where there is no picture.
  - `Pictured::Cover` is `coverartarchive.org/release/<mbid>/front-500` — or the release group's —
    with the icon small beside it, the icon standing in where no release is known;
    `Pictured::Nothing` sends no assets.
  - The progress bar is `start = now − position`, `end = start + length`, left out where
    `discord-progress` is off or the track is paused. A pause clears the activity unless
    `discord-paused` keeps it.
  - Text is cut to 128 characters and padded to Discord's two.
- **A send is paced.** The thread ticks every second but sends only when the activity changed —
  text or assets, the bar appearing or going, or a start moving more than `DRIFT_ALLOWED` (a seek)
  — and never within `SENDS_APART` of the last, Discord's own limit being five in twenty seconds. A
  change inside the window is sent when it ends.
- **The release is resolved once per track**: `MUSICBRAINZ_ALBUMID` from the tags, then the
  release group, then `Releases`, and only where a cover will be drawn. A found cover is held for
  the track — id, location and span together, since an unscanned row's id is minted again from
  `TrackId::MAX` by the next load and two cuts of one file share the rest; a miss is asked again
  after `COVER_REFRESH_AFTER`, since the enrichment may land the release while the track plays.

## What it does not do

- **No local picture is uploaded anywhere.** Discord draws only an asset uploaded to the
  application or a public URL — a Cover Art Archive address it fetches itself — so a track whose
  release nobody named shows the icon. Sending covers to an image host would send the listener's
  library somewhere they did not choose.
