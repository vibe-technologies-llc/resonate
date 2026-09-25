---
paths:
  - "crates/resonate-discord/**"
  - "crates/resonate/src/discord.rs"
  - "crates/resonate-core/src/presence.rs"
  - "crates/resonate-ui/src/views/settings/desktop.rs"
---

# Discord presence

`resonate-discord` tells a running Discord client what is playing, over Discord's own local IPC
socket, as a *Listening* activity. It reaches `resonate-core`, the engine's vocabulary,
`serde`/`serde_json`, `crossbeam-channel` and `parking_lot`, and only the binary reaches it,
behind the `discord` feature. `cargo tree -p resonate-discord` must stay free of gpui, the library
and `ureq`.

## Off means nothing runs

- **The build carries no application id and presence is off.** `discord` defaults to false and
  `discord-app` to nothing, and `Presence::active` — switched on *and* naming an application — is
  the one gate everything asks. The id is the listener's own, registered in Discord's developer
  portal, for the same reason `contact`, `acoustid-key`, `audd-token` and `listenbrainz-token` are: a request says what
  this build is and nothing about who runs it.
- **Inactive is no thread, no socket and no probe.** `Discord::new` starts nothing;
  `Discord::follow` spawns the `resonate-discord` thread when the presence becomes active and
  stops it — clearing the activity on the way out — when it stops being active. A run whose file
  leaves the keys alone never opens a socket or looks for one.
- **Active and idle is still no socket.** The thread connects only once there is something to show,
  so a window with nothing playing reaches for Discord no more than one with presence off.

## The seam

- `resonate-core::presence` is the vocabulary — `AppId`, `Icon`, `Shown`, `Pictured`, `Presence` —
  because the config reader, the window and the crate all need it and none may reach the others,
  which is the argument `Appearance` already makes.
- `resonate_ui::Present` is how the settings pane reaches the publisher without depending on it:
  the binary's `discord::Presenter` implements it, rides into the window on `Stored`, and every
  control in the two Desktop groups writes the `ResonateApp::presence` global, stores the setting
  and calls `follow`, so a change is live. Without the feature the `Presenter` is empty and warns
  once where the file asks for a presence the build cannot show.
- `Releases` is how a cover is found without the crate seeing the library: the binary's
  `Catalogued` reads `track_at` and `release_of` for the row's release, or its release group.

## What is sent

- **The frame is Discord's**: a little-endian opcode and length, then JSON, capped at
  `LARGEST_FRAME`. The handshake waits for `READY`; a `Close` carrying 4000 is an application
  Discord does not know, which is warned about once and then waited out until the id changes.
  A `Ping` is answered with a `Pong`, and an `ERROR` reply is a `Refused` warning that keeps the
  session: the `Sent` is marked `refused`, so `due` offers the same activity again once
  `RETRY_AFTER` has passed, and a change is sent on the usual spacing as ever. Closing the socket
  over it would reconnect every fifteen seconds to be refused the same payload.
- **The socket is looked for where every Discord puts it**: `$XDG_RUNTIME_DIR`, `$TMPDIR` and
  `/tmp`, each plain and under the Flatpak, Snap and Vesktop sandboxes, `discord-ipc-0` to `-9`.
  One that does not answer is a debug record and a retry after `RETRY_AFTER`.
- **`Activity::of` is the whole of the policy**, pure and tested without a socket.
  - `Shown::Application` says nothing about the track and draws no cover even where one is asked
    for; `Track` is the title over the artist; `Album` adds the album as the picture's caption, or
    after the artist where there is no picture.
  - `Pictured::Cover` is `coverartarchive.org/release/<mbid>/front-500` — or the release group's —
    with the icon small beside it; where no release is known the icon stands in, and
    `Pictured::Nothing` sends no assets.
  - The progress bar is `start = now − position` and `end = start + length`, left out where
    `discord-progress` is off or the track is paused. A pause clears the activity unless
    `discord-paused` keeps it.
  - Text is cut to 128 characters and padded to Discord's two.
- **A send is paced.** The thread ticks every second but sends only when the activity changed —
  its text or assets, the bar appearing or going, or a start that moved more than `DRIFT_ALLOWED`,
  which is what a seek is — and never within `SENDS_APART` of the last send, Discord's own limit
  being five in twenty seconds. A change inside the window is sent when it ends.
- **The release is resolved once per track**: `MUSICBRAINZ_ALBUMID` from the tags, then the
  release group, then `Releases`, and only where a cover is going to be drawn. A cover found is
  held for the track — its id, its location and its span together, because an unscanned row's
  id is minted again from `TrackId::MAX` by the next load and two cuts of one file share the
  rest; a miss is asked again after `COVER_REFRESH_AFTER`, because the enrichment
  may land the release while the track is still playing.

## What it does not do

- **No local picture is uploaded anywhere.** Discord draws only an asset uploaded to the
  application or a public URL, so a track whose release nobody has named shows the icon. Sending
  the listener's covers to an image host would be sending their library somewhere they did not
  choose.
