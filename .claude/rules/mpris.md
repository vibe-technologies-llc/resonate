---
paths:
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate/src/mpris.rs"
  - "crates/resonate/src/starting.rs"
---

# The session bus

`resonate-mpris`: MPRIS, notifications, idle inhibit, playlists (`library.md` has the seam), and
the client a second `resonate` reaches a first through.

## What it sees

- **zbus 6 has no blocking API; `blocking.rs` is ours.** It wraps the async `Connection`, `Proxy`,
  `ObjectServer`, `InterfaceRef`, the `fdo` proxies and their streams (`Arrivals` is the iterator)
  over its own parking `block_on`, never `zbus::block_on`: a connection built inside zbus's runs
  only while a thread is inside it, and handing it to a helper thread between calls lost signals
  under load (`a_row_whose_read_answered_nothing_is_told_so_rather_than_owed_for_ever`). Built
  outside, every connection is run by the backend's own helper thread. The integration tests
  include the file by `#[path]`. An error reply whose name `fdo::Error` defines (`UnknownMethod`,
  `InvalidArgs`) reaches a caller as `zbus::Error::FDO`, not `MethodError`.
- **Engine only via `Player`, front end only via `Host`.** No gpui or library dependency: one
  service serves `resonate play` and the window. Media keys are the desktop's to grab (it calls
  MPRIS); the window binds them only as fallback (`ui.md`). Started unconditionally; no session bus
  = warn, carry on.
- **A location whose source is not in `Host::sources` is refused**; `SupportedUriSchemes` is built
  from the same list; `SupportedMimeTypes` is the binary's `MIME_TYPES` (`binary.md`).
- **What MPRIS has no word for gets our own interface** `org.resonate.Player1`, same object path:
  `SetSleep`, `Sleep`, `PlayingNext`, `AddTracks`, `Relocate`. `Relocate(a(ss))` takes `(from, to)`
  URI pairs in landing order = one `Command::Relocate`; `Running::relocate` sends `MOVES_A_CALL`
  (4096) per call (moves compose) to stay under the bus's message limit. The poll diffs the
  *published pair*, not the `Asleep` behind it (`left` ticks continuously).

## The track list

`Tracks` = `Queued::in_list_order` as object paths, `AddTrack`/`RemoveTrack` =
`Command::Insert`/`Remove`, four signals diffed from the 200 ms `POLL` that also drives `Player`
property changes. **The list is the queue unshuffled, not the play order** (`Queued::listed`: the
unshuffled order with rows queued next after the row heard), so shuffling moves nothing on the bus
and play-order positions never reach a client; ids are found by id, not position
(`a_shuffle_leaves_the_track_list_as_it_stood_and_announces_nothing_of_it`). Ids:
`/org/resonate/track/…`, `/org/resonate/playlist/…` (spec reserves `/org/mpris`).

- **Moves announced edit by edit where possible.** `tracklist::change` keys rows by id, holds the
  longest in-order run still, announces the rest as `TrackRemoved`/`TrackAdded` ordered so every
  `AfterTrack` names an announced row. `TrackListReplaced` for: shuffle-sized reorder, no shared
  row, duplicate ids, a kept id whose row differs, more than `EDITS_ANNOUNCED_AT_MOST` (64) edits,
  and equal rows with a moved `Queued::rows_revision` (`change_between`; the only trace of a row
  added and removed within one poll, and unlike `Queued::revision` a reorder leaves it).
- **`GetTracksMetadata` answers positionally**: one entry per id; an unknown id gets
  `mpris:trackid` alone. One `READ_BUDGET` (500 ms) per call: a row with unread tags answers with
  its file stem and `Owed` remembers it; the poll watches `Player::media_revision` and emits
  `TrackMetadataChanged` once settled. `Player::tags_read` answers `TagsRead`: a read that answered
  nothing is announced with the stem and dropped. A cover, and a `TrackAdded` that missed
  `ANNOUNCE_BUDGET` (100 ms), are owed likewise; a cover whose read failed (`ArtRead::Failed`, which
  the window asks again) is no cover to the bus, announced at once, else a source refusing every
  open kept its row owed for good
  (`a_row_whose_read_answered_nothing_is_told_so_rather_than_owed_for_ever`). A row leaving the
  queue is forgotten.
- **A method does what the spec says, including nothing.** `SetPosition` before start, past end or
  for another track: ignored. `Seek`/`SetPosition` where `CanSeek` is false: ignored. `Seek` past
  end = `Next`. Offsets via `unsigned_abs`. `SetRate(0.0)` pauses. `Next`/`Previous` do nothing
  where `CanGoNext`/`CanGoPrevious` say false (`service::can_go_next`, `can_go_previous`: one
  arithmetic for property and method) (`next_and_previous_do_nothing_where_the_bus_was_told_they_could_not`).
- **`Seeked` is the engine's seek count, not a jump read from the position.** `PlayerState::seeks`
  is stepped by `Engine::seek` only where the seek landed; `skip` also steps it for a row heard
  again from its start (*repeat track*), which nothing else tells a client extrapolating position.

## What the metadata carries

- **Fields are the spec's shapes, not the tags' text.** `xesam:contentCreated` is ISO 8601
  (`track::iso_8601`: `YYYY-MM-DDT00:00:00Z`; out-of-range month or day falls to the first; no
  leading year, no field). Artist, album artist, genre, composer and lyricist are lists split on
  `LISTED_APART_BY`. `xesam:userRating` is 1 for a catalog favourite, 0 for another catalog row
  (`Heard::favourite`), absent with no row; `xesam:asText` the tag's words.
- **Covers.** `mpris:artUrl` wants a URI: `art::Pictures` lays the playing cover under
  `resonate-art-<pid>-<n>/` in `$XDG_RUNTIME_DIR` (temp dir if unset/relative), named by a digest of
  its bytes. Folder 0700 via a `create` refusing an existing one (a predictable name in a shared
  folder can't be taken first); covers 0600 under a staging name, then renamed (URL never names an
  unfinished cover). A poll reuses a laid cover only where the row *and* the catalog's `Arc` of the
  picture match. Writes happen outside the lock every `Metadata` read takes (`Pictures::drawn`),
  under their own. Last `COVERS_KEPT` (8) held; `Mpris::shutdown` removes the folder; the next run
  sweeps `resonate-art-<pid>-*` of dead processes. A queued row's cover is laid only when a client
  asks for it, kept apart (`Shared::row_art`, `Pictures::queued_uri`), at most `QUEUED_COVERS_KEPT`
  (32): past that, no cover named rather than one a later row would remove.
- **Catalog counts reach the bus through `Host`.** `xesam:useCount`/`xesam:lastUsed` come from
  `Host::heard`, taking the row's `MediaLocation` *and span* (a cue row counts apart from its file),
  answering `Option<Heard>`: `None` = no catalog or row, `Some` zero = unplayed. `lastUsed` is an
  ISO-8601 UTC stamp over `resonate-core::CivilDate`. Retaken on row change, else every
  `HEARD_READ_EVERY` (1 s). The metadata map the poll diffs is an `Arc` rebuilt only where parts
  moved; listed playlists an `Arc<[PlaylistInfo]>` carried poll to poll.

## The name

**Claimed with `AllowReplacement`: losing it happens; the answer is the fallback's.** The connection
subscribes to `NameLost` *before* claiming; a thread of its own reads it. On losing, `instances`
claims `org.mpris.MediaPlayer2.resonate.instance<pid>` (then `instance<pid>-<n>`, up to `INSTANCES`
= 16, else `Error::NameTaken`), as `claim` does for a name taken before start. `Claimed` is the cell
both threads share (`Mpris::name` = current name). `Mpris::shutdown`: stop flag, join poll thread,
`ReleaseName`, close connection (so the release's `NameLost` reads as teardown), remove the cover
folder; idempotent (`Drop` reruns it). `crates/resonate-mpris/tests/name.rs` is its own test binary:
instance names are keyed by pid and each process has its own `INSTANCES` pool.

**`DesktopEntry` = the installed entry, which Flatpak renames**: the binary's `Host` answers
`FLATPAK_ID` where set, for the bus property and a notification's `desktop-entry` hint
(`packaging.md`).

## The desktop's calls

- **Own connection and thread; the poll only leaves errands.** `notify.rs` raises
  `org.freedesktop.Notifications` on a track change; `idle.rs` holds the portal's
  `org.freedesktop.portal.Inhibit` while there is sound; `desktop.rs` makes both on a second
  connection with `DEADLINE` (2 s) `method_timeout` and a thread reading a bounded channel (on the
  poll thread one unanswered call held every property change 25 s). Inhibit is a *level*:
  `Errands::follow` writes whether the session should be awake into an `AtomicBool` and nudges the
  thread, whose `Gripping` weighs it after every errand. A failed `Inhibit` still counts as taken.
  Either service missing = debug records, never a refusal to start.
- **A notification is keyed on what it *says*.** The cover lands a moment after the track starts:
  the second telling replaces the first under the server's `replaces_id`; teardown closes it and
  waits `FAREWELL` (500 ms). Body escaped only where `GetCapabilities` answers `body-markup`.
- **Idle inhibit is not a setting; a notification is.** A desktop mutes an app by the
  `desktop-entry` hint (blunt for a player otherwise wanted), so `Host::notifies` sits beside
  `Host::attended`, weighed into `Telling::of(attended, notifies)`; a track passed over is still
  *noted* as told. `notify` is a key no flag outranks, under the settings pane's *Desktop* category.
- **Transport buttons, answered on the raising connection.** `ACTIONS` = Previous, Play/Pause, Next,
  offered only where `GetCapabilities` answers `actions`. `ActionInvoked` is weighed against the id
  the notification returned (another app's is no command)
  (`a_press_on_a_notification_button_reaches_the_transport`: reruns the binary under
  `dbus-run-session` with a stand-in notification server).
- **Held back while the window is in front.** `resonate-ui` publishes window activation; the
  binary's `Attention` folds it into `Host::attended`. A window counts as in front until it says
  otherwise (`OPENS_IN_FRONT`): it opens a second after the engine starts and `resonate <files>`
  would else notify for the track it opens to draw. No window = never attended.
- **What Listen named goes through the same notification, deciding nothing about it.**
  `Mpris::teller` hands out a `Teller`, a bounded (4) channel of `Told` the poll drains to the
  desktop thread as `Errand::Tell`, weighed by the same `Telling::of`, raised under its own
  `replaces_id` with no transport buttons. The window never sees the bus: `listen::in_the_window`
  (binary) builds `resonate_ui::Listens` with a `tell` closure over the `Teller`.

## Headsets

**A Bluetooth headset's buttons reach the player through BlueZ, not the desktop.** BlueZ hands an
AVRCP press only to a player registered via `org.bluez.Media1`'s `RegisterPlayer` (AirPods also pick
play vs pause from its reported status); none registered = press lost. `bluez.rs`'s `Headsets`
serves a second `PlayerInterface` at the same object path on its own system-bus connection
(`DEADLINE`); its thread registers it with every object `org.bluez`'s ObjectManager lists as
carrying `Media1`, then each `InterfacesAdded`; asked only where the name has an owner (never
D-Bus-activates `bluetoothd`). `PlayerInterface::as_registered` is what `RegisterPlayer` gets. BlueZ
refuses calls while `CanControl` or the matching `Can*` is false: flags passed at registration and
kept current (the poll `publish`es and emits `Seeked` here beside the session interface).
`Headsets::rest` closes the connection.

**Only a host that says so registers.** `Host::answers_headsets`: false by default, true for the
binary (no test touches the machine's BlueZ). Missing system bus or BlueZ = debug record. Flatpak
needs `--system-talk-name=org.bluez` (`packaging.md`)
(`bus.rs`'s `a_headsets_press_reaches_the_transport_through_the_player_registered_with_bluez`: under
`dbus-run-session` with `DBUS_SYSTEM_BUS_ADDRESS` at the session bus and a stand-in `org.bluez`).

## The client

**The interface is how a second `resonate` reaches a first, and the serving crate speaks it.**
`Running` is a client; callers never learn D-Bus: results come back as domain values (`Seeking`,
`TrackId`).

- **`resonate queue <files>`**: whole run to `AddTracks(uris, after_track, play_the_first)` (one
  `Command::Insert`, one settle), falling back to `AddTrack` per row only against an older build
  answering `UnknownMethod`. The client turns `Placement` into the spec's *anchor* (track before it;
  `NoTrack` = front): `Next` anchors on the playing row, `Queued` on the last row waiting after it
  (`PlayingNext` counts them). A run goes in calls of `ROWS_A_CALL` (128) rows, back to front
  against one anchor, so each stays inside the client's 2 s timeout; a failure after some landed is
  `Error::QueuedPartway { landed, asked }`. The fallback adds back to front against one anchor.
  `Session` is one bus connection a long-lived client (MCP) reuses through `found_in`, `listed_in`
  and `named_in`; `found`, `listed` and `named` open their own.
- **`resonate queue --playlist <NAME>` hands a playlist over as its rows** (`ActivatePlaylist`
  replaces a queue). A search-filled playlist is read via `Library::playlist_entries`, so queued as
  whatever it matches then. `queue` opens the library only for `--playlist`.
- **The window is single.** The window's host answers `CanRaise`/`Raise`; a launch finding a player
  of this build able to raise (`Running::a_window`) hands it its files as `AddTrack`s placed next
  and heard now, raises it and leaves; the settings flags it was given (`--config`, `--library`,
  `--vault`, `--sink`, `--quality`, `--filter-phase`, `--dither`, `--noise-shaping`,
  `--bit-perfect`, `--no-bit-perfect`) are not handed over, so `settings_left_behind` names them on
  stderr. Headless `resonate play` can't raise. The name is claimed
  only after the engine starts, so `starting::one_window_at_a_time` takes an exclusive lock on
  `resonate-starting.lock` under `$XDG_RUNTIME_DIR` (none: `resonate-starting-<uid>.lock` in the
  temp folder) before asking the bus and holds it until `Mpris::start` claimed the name; a second
  launch waits at most `WAITS_AT_MOST` (15 s); an untakeable lock starts the window as before.
- **A player is chosen by name; one reading of the bus serves every way of choosing.** `ours` =
  names that are `org.mpris.MediaPlayer2.resonate` or an `instance` under it, plain name first.
  `Running::found` = first, `listed` = all, `named` = the one that `answers_to` a `PlayerName`
  (whole bus name or `instance<pid>` alone), so `resonate players` prints what `--player` takes.
  `Standing` = what one answers about itself (`Option`s: nothing vs empty differ); a player that
  stops answering between listing and read is left out. `PlaybackStatus` lives in `track.rs` beside
  the (lossy) mapping from `PlaybackState`.
