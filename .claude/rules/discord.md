---
paths:
  - "crates/resonate-discord/**"
  - "crates/resonate/src/discord.rs"
  - "crates/resonate-core/src/presence.rs"
  - "crates/resonate-ui/src/views/settings/desktop.rs"
---

# Discord presence

`resonate-discord` tells a running Discord client what is playing, over Discord's local IPC socket,
as a *Listening* activity. Only the binary reaches it, behind the `discord` feature;
`cargo tree -p resonate-discord` stays free of gpui, the library and `ureq`.

## Off means nothing runs

- **The build carries no application id and presence is off.** `discord` defaults to false and
  `discord-app` to nothing; `Presence::active` (switched on *and* naming an application) is the one
  gate. The id is the listener's own, for the reason `contact` and the service tokens are: a request
  says what this build is and nothing about who runs it.
- **Inactive is no thread, socket or probe, and active but idle is no socket either.**
  `Discord::follow` spawns the `resonate-discord` thread when presence becomes active and stops it,
  clearing the activity on the way out, when it stops. The thread connects only once there is
  something to show.

## The seam

- `resonate-core::presence` is the vocabulary (`AppId`, `Shown`, `Pictured`, `Presence`), since the
  config reader, the window and the crate all need it and none may reach the others.
- `resonate_ui::Present` is how the settings pane reaches the publisher without depending on it: the
  binary's `discord::Presenter` implements it and rides into the window on `Stored`; every control in
  the Desktop groups stores the setting and calls `follow`, so a change is live. Without the feature
  the `Presenter` is empty and warns once where the file asks for a presence the build cannot show.
- `Releases` finds a cover without the crate seeing the library: the binary's `Catalogued` answers
  from the library for the row's release or release group.

## What is sent

- **The frame is Discord's**: little-endian opcode and length, then JSON, capped at `LARGEST_FRAME`.
  The handshake waits for `READY` (`READY_WITHIN`); an `ERROR` instead is a `Refused` at once. A
  `Close` carrying 4000 is an application Discord does not know: warned about once and waited out
  until the id changes. A `Ping` is answered with a `Pong`. An `ERROR` reply to an activity keeps
  the session and counts a refusal; `due` offers the same activity again after
  `SENT_AGAIN_AFTER_A_REFUSAL` doubled per refusal up to `SENT_AGAIN_AT_MOST`, and a changed
  activity is sent on the usual spacing (`refusals_carried`). Closing the socket would reconnect
  every few seconds to be refused the same payload.
- **The socket is looked for where every Discord puts it**: `$XDG_RUNTIME_DIR`, `$TMPDIR` and
  `/tmp`, each plain and under the Flatpak, Snap and Vesktop sandboxes, `discord-ipc-0` to `-9`,
  the path that answered last first. `find_among` goes on past a socket that will not take the
  client (a `Close` of another code, an `ERROR` in the handshake, an unreadable frame), so a
  sibling or Vesktop beside a refusing Discord is still reached; only 4000 ends the search, every
  Discord answering the same id alike. A search that finds nothing asks again after
  `RETRY_AFTER_AT_FIRST`, doubling to `RETRY_AFTER_AT_MOST`; a session reached or lost restarts it.
- **Only a socket of the listener's own is connected to.** `/tmp` is shared, so another user could
  bind a `discord-ipc-0` and be told what is playing. `Session::open` follows links (Flatpak and
  Vesktop may link one) and refuses with `NotOurs` unless the target is a socket owned by
  `rustix::process::getuid`; `find_among` warns and goes on.
- **`Activity::of` is the whole policy**, pure and tested without a socket.
  - `Shown::Application` says nothing about the track and draws no cover; `Track` is the title over
    the artist; `Album` adds the album as the picture's caption, or after the artist with no picture.
  - `Pictured::Cover` is `coverartarchive.org/release/<mbid>/front-500`, or the release group's,
    with the icon small beside it, standing in where no release is known. A release the archive
    said holds no front (`ReleaseDetail::may_have_a_front`) is drawn from its group, since asking
    the release drew a blank picture. `Pictured::Nothing` sends no assets.
  - The progress bar is `start = now - position`, `end = start + length`, left out where
    `discord-progress` is off or the track is paused. A pause clears the activity unless
    `discord-paused` keeps it.
  - Text is cut to `LONGEST_TEXT` and padded to Discord's two-character minimum.
- **A send is paced.** The thread ticks every second but sends only when the activity changed (text,
  assets, the bar appearing or going, or a start moving more than `DRIFT_ALLOWED`, a seek) and never
  within `SENDS_APART` of the last, Discord limiting to five in twenty seconds. A change inside the
  window is sent when it ends.
- **The release is resolved once per track**: `MUSICBRAINZ_ALBUMID` from the tags, then the release
  group, then `Releases`, and only where a cover will be drawn. A found cover is held for the track
  keyed by id, location and span together (an unscanned row's id is minted again by the next load
  and two cuts of one file share the rest); a miss is asked again after `COVER_REFRESH_AFTER`,
  since enrichment may land the release while the track plays.

## What it does not do

- **No local picture is uploaded anywhere.** Discord draws only an asset uploaded to the application
  or a public URL, so a track whose release nobody named shows the icon. Sending covers to an image
  host would send the listener's library somewhere they did not choose.
