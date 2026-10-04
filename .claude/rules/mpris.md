---
paths:
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate/src/mpris.rs"
---

# The session bus

`resonate-mpris` serves MPRIS, notifications, the idle inhibit and the playlists, and is the
client a second `resonate` reaches a first through. Its playlists seam is in `library.md`.

## What it sees

- **The engine only through `Player`, the front end only through `Host`.** No gpui or library
  dependency, so one service serves `resonate play` and the window. A media key is the desktop's
  to grab first — it calls MPRIS, so the interface *is* the feature — and the window binds the
  same keys only as a fallback (`ui.md`). Started unconditionally; with no session bus it warns
  and carries on; where another instance owns the plain name it falls back to
  `org.mpris.MediaPlayer2.resonate.instance<pid>`. The window and every playing command (`play`,
  `playlist <name>`) put `org.mpris.MediaPlayer2.resonate` on the bus; nothing in the workspace
  reads a key. `busctl --user introspect org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2`
  is the quickest look.
- **A location whose source is not in `Host::sources` is refused**, and `SupportedUriSchemes` is
  built from the same list, so the bus advertises what this build can open rather than a
  hard-coded list. `SupportedMimeTypes` answers the binary's `MIME_TYPES` (`binary.md`).
- **What MPRIS has no word for gets an interface of our own.** `org.resonate.Player1`, at the same
  object path as the four MPRIS interfaces, carries `SetSleep`, `Sleep`, `PlayingNext`, `AddTracks`
  and `Relocate`: the spec has no vocabulary for a sleep timer, for rows queued apart from what plays
  or for files that moved under the queue, and stretching one of its properties would be worse than a
  name plainly ours. `Relocate(a(ss))` takes `(from, to)` URI pairs in landing order and is one
  `Command::Relocate`; `Running::relocate` sends them `MOVES_A_CALL` (4 096) at a time, which a
  sequence of moves composes across, so a whole library filed is never one message past the bus's
  limit (`a_running_player_told_where_files_went_follows_its_queued_rows_there`). Its mode strings are one typed mapping
  in `track.rs` beside `PlaybackStatus`, written and read back through one vocabulary. The poll
  diffs the *published pair*, not the `Asleep` behind it: `left` ticks continuously, and diffing
  the domain value would announce a change five times a second while a timer ran.

## The track list

`TrackList` is the published queue read back. `Tracks` is `Player::queue` mapped to object paths,
one per row (`Queue::load` and `Queue::insert` mint over any id a row arrives under that the queue
already holds); `AddTrack` and `RemoveTrack` are `Command::Insert` and `Command::Remove` on the row
that id names; the four signals are diffed out of the same 200 ms `POLL` that drives the `Player`
property changes.

- **Moves are announced edit by edit where possible.** `tracklist::change` keys rows by id, holds
  still the longest run both samples hold in the same order, and announces every other row that
  left *or moved* as `TrackRemoved` and every row that arrived or moved as `TrackAdded`, ordered
  so every `AfterTrack` names a row already announced — two rows queued in one poll are two
  additions, a row dragged to the front is removed and added again. A reorder moving more rows
  than it leaves standing (a shuffle), a list sharing no row with the last, and more than
  `EDITS_ANNOUNCED_AT_MOST` edits are `TrackListReplaced`. So is a sample whose rows match the
  last but whose `Queued::revision` moved (`change_between`): a row added and removed inside one
  poll is in neither sample, and the revision is the only trace of it the engine publishes — it
  moves only where the drawn order does, so a quiet queue never announces. One sample's additions share one
  `ANNOUNCE_BUDGET` for their tags. `two_rows_added_between_two_polls_are_announced_as_two_additions`
  and `a_row_moved_is_announced_as_that_row_removed_and_added_again` are the claims.
- **`Tracks` is declared `invalidates`**, as the spec asks, and the poll invalidates it wherever
  the run of ids moved, so a caching client reads the list again.
- **`GetTracksMetadata` answers positionally** — one entry per id asked, in order — and an id
  naming no row gets an entry with `mpris:trackid` alone, because that field is the only thing
  keying a reply to its request and a shorter list leaves the client unable to say which row went
  missing. A call is served under one `READ_BUDGET` (500 ms), so a row whose tags are unread by
  then answers with its file stem, and `Owed` remembers it: the poll watches
  `Player::media_revision` and, where a noted row's read has *settled*, emits
  `TrackMetadataChanged` with what the call would answer now. `Player::tags_read` answers
  `TagsRead`, the tri-state the catalog holds and `media` flattens, so a read that answered
  nothing (a file gone, a source that refused) is announced with the stem and dropped rather than
  owed forever; only a pending read is waited on. A row's cover is owed the same way, through
  `ArtRead`, and a `TrackAdded` whose tags or cover missed `ANNOUNCE_BUDGET` is owed too, so what it
  lacked follows as `TrackMetadataChanged`. A row leaving the queue is forgotten, so what
  is owed is bounded by the queue.
- **What the spec says a method does is what it does, including nothing.** A `SetPosition` before
  the start or past the end is ignored, a `Seek` past the end acts like `Next`, an offset is read
  through `unsigned_abs` so `i64::MIN` saturates to the start, and `SetRate(0.0)` pauses. Track
  ids are minted under `/org/resonate/track/` and playlists under `/org/resonate/playlist/`, the
  spec reserving `/org/mpris`.
- **`Seeked` is the engine's own count of seeks, not a jump read out of the position.**
  `PlayerState::seeks` is a `Seeks` stepped by `Engine::seek` only where the seek landed, so a
  refused one does not announce and a track change (which `start` reaches and `seek` does not)
  announces `Metadata` alone, which the spec says `Seeked` is not for. A row heard again from its
  start — a track under *repeat track*, the only row of a repeating queue — is the one move `skip`
  makes that keeps the track id, so `skip` steps the count there too: nothing else tells a client
  extrapolating the position that it went back to zero
  (`a_row_heard_again_from_the_start_is_counted_as_a_seek`). The poll emits it with the
  position from the same sample as the count. It replaced a heuristic calling any drift past
  750 ms from the extrapolated position a jump, which missed shorter seeks, went blind when the
  rate changed, read a track change as a jump and a paused transport as a jump backwards; it is
  also why a seek made from the window or the command line is announced at all.

## What the metadata carries

- **Covers.** `mpris:artUrl` wants a URI where the catalog holds bytes, so `art::Pictures` lays the
  playing track's cover under `resonate-art-<pid>-<n>/` in `$XDG_RUNTIME_DIR` (the temporary folder
  only where a session has none), named by a digest of its bytes so an album's tracks share one
  file, and hands back its `file://` URI. A poll reuses what is laid down without hashing again
  only where the row *and* the catalog's own `Arc` of the picture are those it was drawn for — a
  `Weak` beside the row, which also keeps the address from reuse — since an unscanned row's id is
  minted again from `TrackId::MAX` by the next load and an id alone published the last file's
  cover. The folder is made 0700 with a `create` refusing one already standing, so a predictable
  name in a shared folder cannot be taken first by another user to read what plays or aim a
  symlink; each cover is written at 0600 under a staging name and renamed into place, the
  staging file removed on any error, so `mpris:artUrl` never names an unfinished cover. It is not
  synced — a copy in a runtime folder a crash empties anyway, the rename being what makes it whole
  to a reader — and it is written outside the lock every `Metadata` read takes (`Pictures::drawn`),
  under one of its own (`laying`), so a read naming a cover already laid never waits on a write; a
  file
  already under the name counts as the picture only where its length matches the bytes just
  hashed. Only the last `COVERS_KEPT` (8) are held — a cover no kept track names is removed — and
  `Mpris::shutdown` removes the folder; a run that never got to (the signal fallback's
  `process::exit`) is swept by the next, which removes every `resonate-art-<pid>-*` whose process
  is gone from `/proc`. A queued row's cover is laid down when a client asks for that row —
  `GetTracksMetadata` or a `TrackAdded` — never for every row unasked, which would enqueue a cover
  read and a file write per row. Queued covers are kept apart from the playing ones
  (`Shared::row_art`, `Pictures::queued_uri`), held while their row is in the queue — the poll's
  `keep_rows` lets a row's go when it leaves — and at most `QUEUED_COVERS_KEPT` (32) files: a row
  past that is named no cover rather than one a later row would remove. A file is removed only
  where neither the playing covers nor a queued row names it, so the playing covers turning over
  never takes a cover a queued row still names.
- **What the catalog counted reaches the bus through `Host`**, since the service may not see the
  library. `xesam:useCount` and `xesam:lastUsed` come from `Host::heard`, taking the row's
  `MediaLocation` *and span* — a cue row is counted apart from its file, as `Library::track_played`
  keys a play on the pair — and answering `Option<Heard>`: `None` for no catalog or no row,
  `Some` with zero for an unplayed row, written as different metadata. `xesam:lastUsed` is an
  ISO-8601 UTC stamp written by hand in `track.rs` over `resonate-core::CivilDate` (a UTC stamp
  wanting no zone, one civil-from-days function is all it needs; `Calendar` is for the listener's
  days); it is total, so a `SystemTime` before the epoch converts rather than saturating.
  The reading is taken again the moment the row changes and otherwise at most every
  `HEARD_READ_EVERY` (1 s), not every 200 ms poll (a catalog read five times a second for a number
  that moves once a track); a `Metadata` watcher still sees a count move within a second whichever
  process counted it (`a_play_counted_while_the_track_plays_is_announced_without_a_track_change`).
  The map the poll diffs is an `Arc`, rebuilt only where its parts moved — the row, its length, the
  `MediaInfo` the digest shares, the cover URI and that reading — so an unchanged poll neither
  rebuilds nor compares it, and the listed playlists are an `Arc<[PlaylistInfo]>` carried poll to
  poll: `publish_playlists` compares none of them where the poll holds the very `Arc` it held
  before, and where the listing moved, `moved` weighs it against a map of the last one's names
  in one pass rather than every playlist against every other.

## The name

**Claimed with `AllowReplacement`, so losing it happens, and the answer is the fallback's.** The
connection subscribes to the bus's `NameLost` *before* claiming, and a thread of its own reads it:
losing the held name, it does not ask for it back — `instances` claims
`org.mpris.MediaPlayer2.resonate.instance<pid>` and the service answers there, as `claim` does for a
name taken before start. `Claimed` is the cell the two threads share, so `Mpris::name` answers the
name currently owned. `Mpris::shutdown` hands the name back with `ReleaseName` then closes the
connection, which ends the watch: the stop flag is set before the release, so the release's own
`NameLost` reads as teardown. All idempotent, since `Drop` runs the teardown again.
`BusOp::ReleaseName` is the matchable half of `ClaimName`; `BusOp::Release` is the idle-inhibit
portal handle. `crates/resonate-mpris/tests/name.rs` is its own test binary rather than part of
`bus.rs`, because an instance name is keyed by the pid and a second process has its own pool of
`INSTANCES` (16) — sixteen services in one process is what `cargo test` runs.

**`DesktopEntry` names the entry installed, which the Flatpak renames.** The binary's `Host`
answers `FLATPAK_ID` where the sandbox sets it and `resonate` elsewhere, so the bus property and a
notification's `desktop-entry` hint both find `org.resonate.Resonate.desktop` under Flatpak
(`packaging.md`).

## The desktop's calls

- **Two calls on their own connection and thread; the poll only leaves errands.** `notify.rs`
  raises `org.freedesktop.Notifications` on a track change and `idle.rs` holds the portal's
  `org.freedesktop.portal.Inhibit` while there is sound; `desktop.rs` makes both, on a second
  session connection with a 2 s `method_timeout` (`DEADLINE`) and a thread reading a bounded
  channel, so an unanswering daemon or portal stalls nothing behind it — the blocking proxy takes
  no per-call deadline, and on the poll thread one unanswered call held every property change for
  the bus's own 25 s. The inhibit is left as a *level*: `Errands::follow` writes whether the
  session should be held awake into an `AtomicBool` and nudges the thread, whose `Gripping` weighs
  that level after every errand, so a nudge dropped on a full channel is made good by whichever
  errand follows — a lost `LetGo` used to keep the session awake while paused, a lost `Hold` let it
  suspend mid-play. `Awake::hold` refuses to ask again while a request is held, so none is
  overwritten and leaked. A failed `Inhibit` still counts as taken, so a session with no portal is
  asked once per play, not five times a second, and a notification is asked for only when what it
  says changes. Neither is a failure on a session lacking the services — a run of debug records;
  only a player that will not play is worth refusing to start over.
- **A notification is keyed on what it *says*.** The cover lands a moment after a track starts, so
  the second telling replaces the first in place under the `replaces_id` the server handed back
  rather than raising a second popup; `Mpris`'s teardown closes it and waits `FAREWELL` for the
  thread to say so, not for a call that may never answer. The body is escaped only where
  `GetCapabilities` answers `body-markup`, since a server not reading markup draws the escape
  wherever a name holds an ampersand.
- **The idle inhibit is not a setting; a notification is.** A desktop mutes an application by the
  `desktop-entry` hint, a blunt instrument for a player otherwise wanted, so `Host::notifies` sits
  beside `Host::attended`, weighed into one `Telling::of(attended, notifies)` the poll reads; a
  track passed over is still *noted* as told either way, so nothing pops the moment it is turned
  back on. The binary backs it with an `Arc<AtomicBool>` shared with the window (the mirror of how
  `Attention` carries activation the other way); `notify` is a key no flag outranks, under the
  settings pane's *Desktop* category.
- **Transport buttons, answered on the connection that raised them.** `ACTIONS` is Previous,
  Play/Pause and Next, offered only where `GetCapabilities` answers `actions`; `pressed` is the
  whole of what a key means. The listener is a thread on the desktop connection: `ActionInvoked`
  is subscribed against the notification server's own bus name and weighed against the id it
  handed back, so another application's notification is no command. It ends when that connection
  closes at teardown — `ManuallyDrop` beside the iterator lets the subscription go with the
  connection rather than be taken back over a closed socket, a warning every quit.
  `a_press_on_a_notification_button_reaches_the_transport` is the claim; it cannot run on the
  desktop's bus, whose notification name is the desktop's, so it reruns the test binary under
  `dbus-run-session` with a configuration activating nothing, owns `org.freedesktop.Notifications`
  there with a stand-in offering `actions` and one id, and sends `ActionInvoked`: a press on another
  id moves nothing, Next moves the row and Play/Pause pauses.
- **Held back while the window is in front.** `Host::attended` is a reading the service has no
  other way to take: `resonate-ui` publishes the window's activation on a `Sender<bool>` (as it
  takes the quit `Receiver`) and the binary's `Attention` folds it into the flag. A change passed
  over is still *noted* as told, so nothing pops when the window goes behind. A window counts as in
  front until it says otherwise — `OPENS_IN_FRONT` — since it opens a second after the engine
  starts and `resonate <files>` would otherwise notify for the track it opens to draw. A run with
  no window is never attended, so `resonate play` tells the desktop as always.
- **What Listen named is told through the same notification, deciding nothing about it.**
  `Mpris::teller` hands out a `Teller`, a bounded channel of `Told` (summary, body, optional
  picture) the poll drains beside its track changes and hands to the desktop thread as
  `Errand::Tell`. Weighed by the same `Telling::of`, so a song named with the window in front is
  drawn in the sheet alone; raised under a `replaces_id` of its own with no transport buttons,
  since it names a song that is not playing. The window never sees the bus: the binary's
  `listen::in_the_window` builds the `resonate_ui::Listens` — the `Listener`, the `Recognisers`
  `online::recognisers` registers, the `listen-from` and `listen-for` it opens on and a `tell`
  closure over the `Teller` — and hands it in on `Lookups`, so `resonate-ui` reaches
  `resonate-listen` for the vocabulary and nothing speaking HTTP or D-Bus.

## The client

**The interface is how a second `resonate` reaches a first, and the crate serving it speaks it.**
`Running` is a client, so its callers never learn D-Bus: it answers the transport, the queue, the
playlists and the timer, each turned into a domain value — `seek` takes a `Seeking`, the row calls
a `TrackId`, an object path being this crate's business alone. A second remote control rests on it
rather than reimplementing the bus.

- **`resonate queue <files>`** lists the bus names, takes the plain name or else the first
  `instance<pid>` under it by name, and hands the whole run to `org.resonate.Player1`'s
  `AddTracks(uris, after_track, play_the_first)`, one `Command::Insert` and one settle for every row
  — MCP's `add_to_queue` of a hundred rows was a hundred round trips and up to a hundred 500 ms
  settles — falling back to `AddTrack` a row at a time only where the running player is an older
  build answering `UnknownMethod`. Where a row lands is the
  `Placement` the window and service use; the client turns it into the spec's *anchor*, the track
  before it, `NoTrack` being the front: `Next` anchors on the playing row and `Queued` on the last
  row waiting after it, which `org.resonate.Player1`'s `PlayingNext` counts, so an `AddTrack` there
  joins what is queued rather than landing after the whole playlist. `AddTracks` lands the run in
  order right after its anchor, `play_the_first` hearing its first row; the `AddTrack` fallback adds
  it back to front against one anchor (each lands right after it), so the first named arrives first
  with no read between calls, and `--play` is `set_as_current` on the last call, the first file. Living
  beside the service keeps one notion of a track path, a location and where a row goes.
- **`resonate queue --playlist <NAME>` hands a playlist over as its rows**, since the bus has no
  call adding one and `ActivatePlaylist` replaces a queue: it reads the rows from the catalog here
  and sends the same `AddTrack` run, which is why it is a flag on `queue` (`--next`, `--play`,
  `--player` are the queueing vocabulary, written once). A search-filled playlist is read through
  `Library::playlist_entries`, so it is queued as whatever it matches then. `queue` is the one
  subcommand reaching the catalog without playing, and opens the library only for `--playlist`.
- **The window is single for the same reason.** The window's host answers `CanRaise` and `Raise`
  (reaching gpui over `resonate_ui::Bus` and activating the window); a launch that finds a player
  of this build able to raise (`Running::a_window`) hands it its files as `AddTrack`s placed next
  and heard now, raises it and leaves, rather than starting a second window, engine and stream
  under an `instance<pid>` name writing the same resumption. A headless `resonate play` cannot
  raise, so it never swallows a window being opened. The name is claimed only once the engine has
  started, so two launches a moment apart would both find no window: `starting::one_window_at_a_time`
  takes an exclusive lock on `resonate-starting.lock` under `$XDG_RUNTIME_DIR` before asking the bus,
  and the launch holds it until `Mpris::start` has claimed the name, so the second waits for the first
  to be reachable and hands its files over. It waits at most `WAITS_AT_MOST` (15 s) for a first that
  wedged while starting, and a lock that cannot be taken at all starts the window as before
  (`a_second_launch_waits_for_the_first_to_be_reachable`).
- **Which player a call reaches is a name, and one reading of the bus answers every way of
  choosing.** `ours` is the names that are `org.mpris.MediaPlayer2.resonate` or an `instance` under
  it, sorted (the plain name first, being a prefix of every other). `Running::found` is its first —
  the player `resonate queue` always reached — `Running::listed` is all, and `Running::named` is the
  one that `answers_to` a `PlayerName`, the whole bus name or `instance<pid>` alone, so what
  `resonate players` prints is what `--player` takes. `Standing` is what one answers about itself —
  name, `Option<PlaybackStatus>`, rows held, the playing title and artist — `Option`s throughout
  because answering nothing and answering empty differ; a player that stops answering between the
  listing and the read is left out of the table, being gone. `PlaybackStatus` is the spec's three,
  in `track.rs` beside the mapping from `PlaybackState`, so the property is written and read back
  through one vocabulary (`repeat_mode` never needed this, `RepeatMode` being lossless where
  `PlaybackState` is not).
