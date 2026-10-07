---
paths:
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate/src/mpris.rs"
  - "crates/resonate/src/starting.rs"
---

# The session bus

`resonate-mpris`: MPRIS, notifications, idle inhibit, playlists (`library.md` has the playlists
seam), and the client a second `resonate` reaches a first through.

## What it sees

- **Engine only via `Player`, front end only via `Host`.** No gpui or library dependency: one
  service serves `resonate play` and the window. Media keys are the desktop's to grab first (it
  calls MPRIS: the interface *is* the feature); the window binds them only as fallback (`ui.md`).
  Headset buttons come through BlueZ (*Headsets*). Started unconditionally; no session bus = warn,
  carry on.
- **A location whose source is not in `Host::sources` is refused**; `SupportedUriSchemes` is built
  from the same list; `SupportedMimeTypes` is the binary's `MIME_TYPES` (`binary.md`).
- **What MPRIS has no word for gets our own interface** (stretching a property is worse).
  `org.resonate.Player1`, same object path: `SetSleep`, `Sleep`, `PlayingNext`, `AddTracks`,
  `Relocate`. `Relocate(a(ss))` takes `(from, to)` URI pairs in landing order = one
  `Command::Relocate`; `Running::relocate` sends `MOVES_A_CALL` (4096) per call (moves compose
  across calls), so a whole library filed never exceeds the bus's message limit. The poll diffs the
  *published pair*, not the `Asleep` behind it (`left` ticks continuously: a change 5×/s).

## The track list

`TrackList` = the published queue read back: `Tracks` = `Player::queue` as object paths,
`AddTrack`/`RemoveTrack` = `Command::Insert`/`Remove`, four signals diffed from the 200 ms `POLL`
that also drives `Player` property changes. Ids: `/org/resonate/track/…`,
`/org/resonate/playlist/…` (spec reserves `/org/mpris`).

- **Moves announced edit by edit where possible.** `tracklist::change` keys rows by id, holds the
  longest in-order run still, announces other rows that left/moved as `TrackRemoved` and that
  arrived/moved as `TrackAdded`, ordered so every `AfterTrack` names an announced row.
  `TrackListReplaced` for: shuffle-sized reorder, no shared row, duplicate ids, a kept id whose row
  differs, more than `EDITS_ANNOUNCED_AT_MOST` (64) edits, and equal rows with a moved
  `Queued::revision` (`change_between`; the engine's only trace of a row added and removed within
  one poll).
- **`GetTracksMetadata` answers positionally**: one entry per id, in order; an id naming no row
  gets `mpris:trackid` alone (the only field keying reply to request). One `READ_BUDGET` (500 ms)
  per call: a row with unread tags answers with its file stem and `Owed` remembers it; the poll
  watches `Player::media_revision` and emits `TrackMetadataChanged` once the read settled.
  `Player::tags_read` answers `TagsRead`: a read that answered nothing is announced with the stem
  and dropped, not owed forever. A cover, and a `TrackAdded` that missed `ANNOUNCE_BUDGET`
  (100 ms), are owed likewise. A row leaving the queue is forgotten (owed bounded by the queue).
- **A method does what the spec says, including nothing.** `SetPosition` before start, past end or
  for another track: ignored. `Seek` past end = `Next`. Offsets via `unsigned_abs` (`i64::MIN`
  saturates). `SetRate(0.0)` pauses.
- **`Seeked` is the engine's seek count, not a jump read from the position.** `PlayerState::seeks`
  is stepped by `Engine::seek` only where the seek landed (refused: no announce; track change:
  `Metadata` alone). `skip` also steps it for a row heard again from its start (*repeat track*,
  the only row of a repeating queue): nothing else tells a client extrapolating the position that
  it went to zero.

## What the metadata carries

- **Covers.** `mpris:artUrl` wants a URI: `art::Pictures` lays the playing cover under
  `resonate-art-<pid>-<n>/` in `$XDG_RUNTIME_DIR` (temp dir if unset/relative), named by a digest of
  its bytes (an album's tracks share a file; same-size existing file reused). Folder 0700 via a
  `create` refusing an existing one (a predictable name in a shared folder can't be taken first by
  another user); covers written 0600 under a staging name, then renamed (URL never names an
  unfinished cover). A poll reuses a laid cover only where the row *and* the catalog's own `Arc` of
  the picture match (an unscanned row's id is minted again by the next load). Writes happen outside
  the lock every `Metadata` read takes (`Pictures::drawn`), under their own (`laying`). Last
  `COVERS_KEPT` (8) held; `Mpris::shutdown` removes the folder; the next run sweeps
  `resonate-art-<pid>-*` of dead processes. A queued row's cover is laid only when a client asks
  for that row, kept apart (`Shared::row_art`, `Pictures::queued_uri`), at most
  `QUEUED_COVERS_KEPT` (32): past that, no cover named rather than one a later row would remove.
- **Catalog counts reach the bus through `Host`** (service can't see the library).
  `xesam:useCount`/`xesam:lastUsed` come from `Host::heard`, taking the row's `MediaLocation` *and
  span* (a cue row counts apart from its file), answering `Option<Heard>`: `None` = no catalog or
  row, `Some` zero = unplayed. `lastUsed` = ISO-8601 UTC stamp written in `track.rs` over
  `resonate-core::CivilDate`. Reading retaken on row change, else every `HEARD_READ_EVERY` (1 s).
  The metadata map the poll diffs is an `Arc` rebuilt only where parts moved; listed playlists an
  `Arc<[PlaylistInfo]>` carried poll to poll (unchanged poll compares nothing).

## The name

**Claimed with `AllowReplacement`: losing it happens; the answer is the fallback's.** The
connection subscribes to `NameLost` *before* claiming; a thread of its own reads it. On losing the
name it doesn't ask back: `instances` claims `org.mpris.MediaPlayer2.resonate.instance<pid>` (then
`instance<pid>-<n>`, up to `INSTANCES` = 16, else `Error::NameTaken`), the service answers there,
as `claim` does for a name taken before start. `Claimed` is the cell both threads share
(`Mpris::name` = current name). `Mpris::shutdown`: stop flag, join poll thread, `ReleaseName`, close
connection (so the release's `NameLost` reads as teardown), remove the cover folder; idempotent
(`Drop` reruns it). `crates/resonate-mpris/tests/name.rs` is its own test binary: instance names
are keyed by pid and each process has its own `INSTANCES` pool.

**`DesktopEntry` = the installed entry, which Flatpak renames**: the binary's `Host` answers
`FLATPAK_ID` where set, for the bus property and a notification's `desktop-entry` hint
(`packaging.md`).

## The desktop's calls

- **Own connection and thread; the poll only leaves errands.** `notify.rs` raises
  `org.freedesktop.Notifications` on a track change; `idle.rs` holds the portal's
  `org.freedesktop.portal.Inhibit` while there is sound; `desktop.rs` makes both on a second session
  connection with `DEADLINE` (2 s) `method_timeout` and a thread reading a bounded channel (an
  unanswering daemon stalls nothing; on the poll thread one unanswered call held every property
  change 25 s, the bus's timeout). Inhibit is a *level*: `Errands::follow` writes whether the
  session should be awake into an `AtomicBool` and nudges the thread, whose `Gripping` weighs it
  after every errand (a nudge dropped on a full channel is made good by the next). A failed
  `Inhibit` still counts as taken (no portal = asked once per play). Either service missing = debug
  records, never a refusal to start.
- **A notification is keyed on what it *says*.** The cover lands a moment after the track starts:
  the second telling replaces the first under the server's `replaces_id`; teardown closes it and
  waits `FAREWELL` (500 ms). Body escaped only where `GetCapabilities` answers `body-markup`.
- **Idle inhibit is not a setting; a notification is.** A desktop mutes an app by the
  `desktop-entry` hint (blunt for a player otherwise wanted), so `Host::notifies` sits beside
  `Host::attended`, weighed into `Telling::of(attended, notifies)`; a track passed over is still
  *noted* as told (nothing pops when it is turned back on). `notify` is a key no flag outranks,
  under the settings pane's *Desktop* category.
- **Transport buttons, answered on the raising connection.** `ACTIONS` = Previous, Play/Pause,
  Next, offered only where `GetCapabilities` answers `actions`. `ActionInvoked` is subscribed
  against the notification server's bus name and weighed against the id it returned (another
  app's notification is no command)
  (`a_press_on_a_notification_button_reaches_the_transport`: reruns the binary under
  `dbus-run-session` with a stand-in notification server).
- **Held back while the window is in front.** `resonate-ui` publishes window activation; the
  binary's `Attention` folds it into `Host::attended`. A window counts as in front until it says
  otherwise (`OPENS_IN_FRONT`): it opens a second after the engine starts and `resonate <files>`
  would else notify for the track it opens to draw. No window = never attended.
- **What Listen named goes through the same notification, deciding nothing about it.**
  `Mpris::teller` hands out a `Teller`, a bounded (4) channel of `Told` the poll drains to the
  desktop thread as `Errand::Tell`, weighed by the same `Telling::of`, raised under its own
  `replaces_id` with no transport buttons (it names a song not playing). The window never sees the
  bus: `listen::in_the_window` (binary) builds `resonate_ui::Listens` with a `tell` closure over
  the `Teller`.

## Headsets

**A Bluetooth headset's buttons reach the player through BlueZ, not the desktop.** BlueZ hands an
AVRCP press (play, pause, stop, next, previous) only to a player registered via
`org.bluez.Media1`'s `RegisterPlayer`; AirPods also pick play vs pause from that player's reported
status. None registered (no `mpris-proxy`; PipeWire's dummy player off by default) = press lost.
`bluez.rs`'s `Headsets` serves a second `PlayerInterface` at the same object path on its own
system-bus connection (`DEADLINE` `method_timeout`); its thread registers it with every object
`org.bluez`'s ObjectManager lists as carrying `Media1`, then each `InterfacesAdded` announces
(adapter plugged in, `bluetoothd` restarting); objects asked only where the name has an owner
(never D-Bus-activates `bluetoothd`). `PlayerInterface::as_registered` is what `RegisterPlayer`
gets. BlueZ refuses calls while `CanControl` or the matching `Can*` is false: flags passed at
registration and kept current (the poll `publish`es and emits `Seeked` here beside the session
interface, so BlueZ hears every status change). `Headsets::rest` closes the connection; BlueZ drops
the player as its sender leaves. While Resonate runs, headset buttons control it, not whatever
else plays.

**Only a host that says so registers.** `Host::answers_headsets`: false by default, true for the
binary (no test touches the machine's BlueZ). Missing system bus or BlueZ = debug record. Flatpak
needs `--system-talk-name=org.bluez` (`packaging.md`)
(`bus.rs`'s `a_headsets_press_reaches_the_transport_through_the_player_registered_with_bluez`: under
`dbus-run-session`, `DBUS_SYSTEM_BUS_ADDRESS` at the session bus, stand-in `org.bluez`; registers,
follows a later adapter, tells BlueZ the status; a Pause and a Next called as BlueZ calls them reach
the transport).

## The client

**The interface is how a second `resonate` reaches a first, and the serving crate speaks it.**
`Running` is a client; callers never learn D-Bus: transport, queue, playlists and timer come back
as domain values (`Seeking`, `TrackId`).

- **`resonate queue <files>`**: whole run to `org.resonate.Player1`'s
  `AddTracks(uris, after_track, play_the_first)` (one `Command::Insert`, one settle), falling back
  to `AddTrack` per row only against an older build answering `UnknownMethod`. The client turns
  `Placement` into the spec's *anchor* (track before it; `NoTrack` = front): `Next` anchors on the
  playing row, `Queued` on the last row waiting after it (`PlayingNext` counts them). The fallback
  adds back to front against one anchor: first named arrives first, no read between calls.
- **`resonate queue --playlist <NAME>` hands a playlist over as its rows** (no bus call adds one;
  `ActivatePlaylist` replaces a queue). A search-filled playlist is read via
  `Library::playlist_entries`, so queued as whatever it matches then. `queue` opens the library only
  for `--playlist`.
- **The window is single.** The window's host answers `CanRaise`/`Raise`; a launch finding a
  player of this build able to raise (`Running::a_window`) hands it its files as `AddTrack`s placed
  next and heard now, raises it and leaves (else a second window, engine and stream write the same
  resumption). Headless `resonate play` can't raise. The name is claimed only after the engine
  starts, so `starting::one_window_at_a_time` takes an exclusive lock on `resonate-starting.lock`
  under `$XDG_RUNTIME_DIR` before asking the bus and holds it until `Mpris::start` claimed the
  name; a second launch waits at most `WAITS_AT_MOST` (15 s) for a wedged first; an untakeable lock
  starts the window as before.
- **A player is chosen by name; one reading of the bus serves every way of choosing.** `ours` =
  names that are `org.mpris.MediaPlayer2.resonate` or an `instance` under it, plain name first.
  `Running::found` = first, `listed` = all, `named` = the one that `answers_to` a `PlayerName`
  (whole bus name or `instance<pid>` alone), so `resonate players` prints what `--player` takes.
  `Standing` = what one answers about itself (`Option`s for playback, title, artist: nothing vs
  empty differ); a player that stops answering between listing and read is left out.
  `PlaybackStatus` lives in `track.rs` beside the (lossy) mapping from `PlaybackState`.
