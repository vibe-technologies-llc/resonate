---
paths:
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate/src/mpris.rs"
  - "crates/resonate/src/starting.rs"
---

# The session bus

`resonate-mpris` serves MPRIS, notifications, the idle inhibit and the playlists (`library.md` has
the playlists seam), and is the client a second `resonate` reaches a first through.

## What it sees

- **The engine only through `Player`, the front end only through `Host`.** No gpui or library
  dependency, so one service serves `resonate play` and the window. A media key is the desktop's to
  grab first (it calls MPRIS, so the interface *is* the feature); the window binds the same keys only
  as a fallback (`ui.md`). Started unconditionally; with no session bus it warns and carries on.
- **A location whose source is not in `Host::sources` is refused**, and `SupportedUriSchemes` is built
  from the same list. `SupportedMimeTypes` answers the binary's `MIME_TYPES` (`binary.md`).
- **What MPRIS has no word for gets an interface of our own.** `org.resonate.Player1`, at the same
  object path, carries `SetSleep`, `Sleep`, `PlayingNext`, `AddTracks` and `Relocate`; stretching an
  MPRIS property would be worse than a name plainly ours. `Relocate(a(ss))` takes `(from, to)` URI
  pairs in landing order and is one `Command::Relocate`; `Running::relocate` sends them
  `MOVES_A_CALL` at a time, which moves compose across, so a whole library filed is never one message
  past the bus's limit. The poll diffs the *published pair*, not the `Asleep` behind it: `left` ticks
  continuously and would announce a change five times a second.

## The track list

`TrackList` is the published queue read back: `Tracks` is `Player::queue` mapped to object paths,
`AddTrack` and `RemoveTrack` are `Command::Insert` and `Command::Remove`, and the four signals are
diffed out of the same 200 ms `POLL` that drives the `Player` property changes. Track ids are minted
under `/org/resonate/track/` and playlists under `/org/resonate/playlist/`, the spec reserving
`/org/mpris`.

- **Moves are announced edit by edit where possible.** `tracklist::change` keys rows by id, holds
  still the longest run both samples hold in order, and announces every other row that left or moved
  as `TrackRemoved` and every row that arrived or moved as `TrackAdded`, ordered so every
  `AfterTrack` names a row already announced. A shuffle-sized reorder, a list sharing no row with the
  last, and more than `EDITS_ANNOUNCED_AT_MOST` edits are `TrackListReplaced`; so is a sample whose
  rows match but whose `Queued::revision` moved (`change_between`), the only trace the engine
  publishes of a row added and removed inside one poll.
- **`GetTracksMetadata` answers positionally**: one entry per id asked, in order, and an id naming no
  row gets an entry with `mpris:trackid` alone, the only field keying a reply to its request. A call
  is served under one `READ_BUDGET`, so a row whose tags are unread by then answers with its file
  stem and `Owed` remembers it: the poll watches `Player::media_revision` and, where the read has
  settled, emits `TrackMetadataChanged`. `Player::tags_read` answers `TagsRead`, so a read that
  answered nothing is announced with the stem and dropped rather than owed forever. A cover, and a
  `TrackAdded` that missed `ANNOUNCE_BUDGET`, are owed the same way. A row leaving the queue is
  forgotten, so what is owed is bounded by the queue.
- **What the spec says a method does is what it does, including nothing.** `SetPosition` before the
  start or past the end is ignored, `Seek` past the end acts like `Next`, an offset is read through
  `unsigned_abs` so `i64::MIN` saturates, and `SetRate(0.0)` pauses.
- **`Seeked` is the engine's own count of seeks, not a jump read out of the position.**
  `PlayerState::seeks` is stepped by `Engine::seek` only where the seek landed, so a refused one does
  not announce and a track change announces `Metadata` alone. `skip` also steps it for a row heard
  again from its start (*repeat track*, the only row of a repeating queue), since nothing else tells a
  client extrapolating the position that it went back to zero.

## What the metadata carries

- **Covers.** `mpris:artUrl` wants a URI, so `art::Pictures` lays the playing track's cover under
  `resonate-art-<pid>-<n>/` in `$XDG_RUNTIME_DIR`, named by a digest of its bytes so an album's tracks
  share one file. The folder is 0700, made with a `create` refusing one already standing, so a
  predictable name in a shared folder cannot be taken first by another user; each cover is written
  0600 under a staging name and renamed into place, so the URL never names an unfinished cover. A poll
  reuses what is laid down only where the row *and* the catalog's own `Arc` of the picture are those it
  was drawn for, since an unscanned row's id is minted again by the next load. Writes happen outside
  the lock every `Metadata` read takes (`Pictures::drawn`), under one of their own (`laying`). Only the
  last `COVERS_KEPT` are held; `Mpris::shutdown` removes the folder and the next run sweeps any
  `resonate-art-<pid>-*` whose process is gone. A queued row's cover is laid only when a client asks
  for that row, kept apart from the playing ones (`Shared::row_art`, `Pictures::queued_uri`) and at
  most `QUEUED_COVERS_KEPT`: a row past that is named no cover rather than one a later row would
  remove.
- **What the catalog counted reaches the bus through `Host`**, since the service may not see the
  library. `xesam:useCount` and `xesam:lastUsed` come from `Host::heard`, taking the row's
  `MediaLocation` *and span* (a cue row is counted apart from its file) and answering
  `Option<Heard>`: `None` for no catalog or no row, `Some` with zero for an unplayed row.
  `xesam:lastUsed` is an ISO-8601 UTC stamp written in `track.rs` over `resonate-core::CivilDate`. The
  reading is retaken when the row changes and otherwise at most every `HEARD_READ_EVERY`. The metadata
  map the poll diffs is an `Arc` rebuilt only where its parts moved, and the listed playlists an
  `Arc<[PlaylistInfo]>` carried poll to poll, so an unchanged poll compares nothing.

## The name

**Claimed with `AllowReplacement`, so losing it happens, and the answer is the fallback's.** The
connection subscribes to `NameLost` *before* claiming and a thread of its own reads it: losing the
held name, it does not ask for it back; `instances` claims
`org.mpris.MediaPlayer2.resonate.instance<pid>` and the service answers there, as `claim` does for a
name taken before start. `Claimed` is the cell the two threads share, so `Mpris::name` answers the
name currently owned. `Mpris::shutdown` sets the stop flag, then `ReleaseName`, then closes the
connection, so the release's own `NameLost` reads as teardown; all idempotent, since `Drop` runs it
again. `crates/resonate-mpris/tests/name.rs` is its own test binary because instance names are keyed
by pid and a process has its own pool of `INSTANCES`.

**`DesktopEntry` names the entry installed, which the Flatpak renames**: the binary's `Host` answers
`FLATPAK_ID` where the sandbox sets it, for the bus property and a notification's `desktop-entry` hint
alike (`packaging.md`).

## The desktop's calls

- **Two calls on their own connection and thread; the poll only leaves errands.** `notify.rs` raises
  `org.freedesktop.Notifications` on a track change and `idle.rs` holds the portal's
  `org.freedesktop.portal.Inhibit` while there is sound; `desktop.rs` makes both on a second session
  connection with a `DEADLINE` `method_timeout` and a thread reading a bounded channel, so an
  unanswering daemon stalls nothing (on the poll thread one unanswered call held every property change
  for the bus's 25 s). The inhibit is a *level*: `Errands::follow` writes whether the session should be
  awake into an `AtomicBool` and nudges the thread, whose `Gripping` weighs that level after every
  errand, so a nudge dropped on a full channel is made good by the next. A failed `Inhibit` still
  counts as taken, so a session with no portal is asked once per play. A session lacking either
  service is a run of debug records, never a refusal to start.
- **A notification is keyed on what it *says*.** The cover lands a moment after a track starts, so the
  second telling replaces the first under the `replaces_id` the server handed back; teardown closes it
  and waits `FAREWELL`. The body is escaped only where `GetCapabilities` answers `body-markup`.
- **The idle inhibit is not a setting; a notification is.** A desktop mutes an application by the
  `desktop-entry` hint, a blunt instrument for a player otherwise wanted, so `Host::notifies` sits
  beside `Host::attended`, weighed into `Telling::of(attended, notifies)`; a track passed over is still
  *noted* as told, so nothing pops the moment it is turned back on. `notify` is a key no flag
  outranks, under the settings pane's *Desktop* category.
- **Transport buttons, answered on the connection that raised them.** `ACTIONS` is Previous,
  Play/Pause and Next, offered only where `GetCapabilities` answers `actions`. `ActionInvoked` is
  subscribed against the notification server's own bus name and weighed against the id it handed back,
  so another application's notification is no command. The test
  (`a_press_on_a_notification_button_reaches_the_transport`) reruns the binary under
  `dbus-run-session` with a stand-in notification server.
- **Held back while the window is in front.** `resonate-ui` publishes the window's activation and the
  binary's `Attention` folds it into `Host::attended`. A window counts as in front until it says
  otherwise (`OPENS_IN_FRONT`), since it opens a second after the engine starts and `resonate <files>`
  would otherwise notify for the track it opens to draw. A run with no window is never attended.
- **What Listen named is told through the same notification, deciding nothing about it.**
  `Mpris::teller` hands out a `Teller`, a bounded channel of `Told` the poll drains and hands to the
  desktop thread as `Errand::Tell`, weighed by the same `Telling::of` and raised under a `replaces_id`
  of its own with no transport buttons, since it names a song that is not playing. The window never
  sees the bus: the binary's `listen::in_the_window` builds the `resonate_ui::Listens` with a `tell`
  closure over the `Teller`.

## The client

**The interface is how a second `resonate` reaches a first, and the crate serving it speaks it.**
`Running` is a client, so its callers never learn D-Bus: it answers the transport, the queue, the
playlists and the timer as domain values (`Seeking`, `TrackId`).

- **`resonate queue <files>`** hands the whole run to `org.resonate.Player1`'s
  `AddTracks(uris, after_track, play_the_first)`, one `Command::Insert` and one settle for every row,
  falling back to `AddTrack` a row at a time only against an older build answering `UnknownMethod`.
  The client turns the `Placement` into the spec's *anchor*, the track before it, `NoTrack` being the
  front: `Next` anchors on the playing row and `Queued` on the last row waiting after it, which
  `PlayingNext` counts. The fallback adds back to front against one anchor so the first named arrives
  first with no read between calls.
- **`resonate queue --playlist <NAME>` hands a playlist over as its rows**, since the bus has no call
  adding one and `ActivatePlaylist` replaces a queue. A search-filled playlist is read through
  `Library::playlist_entries`, so it is queued as whatever it matches then. `queue` opens the library
  only for `--playlist`.
- **The window is single.** The window's host answers `CanRaise` and `Raise`; a launch that finds a
  player of this build able to raise (`Running::a_window`) hands it its files as `AddTrack`s placed
  next and heard now, raises it and leaves, rather than starting a second window, engine and stream
  writing the same resumption. A headless `resonate play` cannot raise. The name is claimed only once
  the engine has started, so `starting::one_window_at_a_time` takes an exclusive lock on
  `resonate-starting.lock` under `$XDG_RUNTIME_DIR` before asking the bus and holds it until
  `Mpris::start` has claimed the name; a second launch waits at most `WAITS_AT_MOST` for a first that
  wedged, and a lock that cannot be taken starts the window as before.
- **Which player a call reaches is a name, and one reading of the bus answers every way of
  choosing.** `ours` is the names that are `org.mpris.MediaPlayer2.resonate` or an `instance` under
  it, the plain name first. `Running::found` is the first, `Running::listed` all, and
  `Running::named` the one that `answers_to` a `PlayerName` (the whole bus name or `instance<pid>`
  alone), so what `resonate players` prints is what `--player` takes. `Standing` is what one answers
  about itself, `Option`s throughout because answering nothing and answering empty differ; a player
  that stops answering between the listing and the read is left out. `PlaybackStatus` lives in
  `track.rs` beside the mapping from `PlaybackState`, which is lossy.
