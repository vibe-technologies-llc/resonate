---
paths:
  - "crates/resonate-mpris/**/*.rs"
  - "crates/resonate/src/mpris.rs"
  - "crates/resonate/src/starting.rs"
---

# The session bus

`resonate-mpris` also compiles for Windows, where there is no session bus: its portal trash and
the permission part of its cover folder are Unix, and the binary treats a missing bus
as it does on a Linux machine with none, except that a window is still served and reached on a
named pipe (*The client's second wire*, `windows.md`), song changes are told as toasts, the
connection's cost is read from `NetworkInformation` (*The desktop's calls*, *Is the connection
metered*) and the first zone is published to and commanded from the System Media Transport
Controls (*The media transport* below, `windows.md`). It is MPRIS, notifications, idle inhibit, playlists (`library.md` has the seam), and
the client a second `resonate` reaches a first through.

**It compiles for macOS too, where the bus is tried and not found.** A Mac's players are served and
reached on a Unix socket (`socket.rs`, *The client's second wire*) exactly as Windows's are on a
pipe: `Mpris::served` and `Session::open` fall back under `cfg(any(windows, target_os = "macos"))`,
so one launch finds the window another opened, hands it its files and raises it, and `queue`,
`players`, `sleep` and a subcommand's `Reload` reach it. The portal trash is not compiled there
(`rustix`'s `OFlags::PATH` is not Apple's; a Mac's Trash is the binary's, `binary.md`). A song
change is told through the system's notifications (`desk_notice.rs`, `macos.md`'s *Notifications*);
no idle inhibit is made (`desk_bus.rs`, `idle.rs` and `notifier.rs` are not compiled there), the
binary's `awake.rs` keeping the machine awake. Whether
the connection is metered is read from the Network framework's path monitor (*Is the connection
metered*).
The first zone is published to and commanded from the system's Now Playing (*The media transport*
below, `macos.md`).

## What it sees

- **zbus 6 has no blocking API; `blocking.rs` is ours.** It wraps the async `Connection`, `Proxy`,
  `ObjectServer`, `InterfaceRef`, the `fdo` proxies and their streams (`Arrivals` is the iterator)
  over its own parking `block_on`, never `zbus::block_on`: a connection built inside zbus's runs
  only while a thread is inside it, and handing it to a helper thread between calls lost signals
  under load (`a_row_whose_read_answered_nothing_is_told_so_rather_than_owed_for_ever`). Built
  outside, every connection is run by the backend's own helper thread. The integration tests
  include the file by `#[path]` (each `mod blocking` there allowed dead, since each uses a part), so
  the crate's own build is warned of every unused wrapper; a wrapper only tests call is
  `#[cfg(test)]`, and those the integration tests alone reach carry their own `allow(dead_code)`. An error reply whose name `fdo::Error` defines (`UnknownMethod`,
  `InvalidArgs`) reaches a caller as `zbus::Error::FDO`, not `MethodError`.
- **Engine only via `Player`, front end only via `Host`.** No gpui or library dependency: one
  service serves `resonate play` and the window. Media keys are the desktop's to grab (it calls
  MPRIS); the window binds them only as fallback (`ui.md`). Started unconditionally; no session bus
  = warn, carry on.
- **A location whose source is not in `Host::sources` is refused**; `SupportedUriSchemes` is built
  from the same list; `SupportedMimeTypes` is the binary's `MIME_TYPES` (`binary.md`).
- **What MPRIS has no word for gets our own interface** `org.resonate.Player1`, same object path:
  `SetSleep`, `Sleep`, `PlayingNext`, `PlayOrder`, `AddTracks`, `Relocate`, `Tune`, `Reload`, `HandOver`. `Relocate(a(ss))` takes `(from, to)`
  URI pairs in landing order = one `Command::Relocate`; `Running::relocate` sends `MOVES_A_CALL`
  (4096) per call (moves compose) to stay under the bus's message limit.
  The poll diffs the *published pair*, not the `Asleep` behind it (`left` ticks continuously).
  `Tune(a(ss))` hands a launch's settings flags to the window it hands files to: `Tunable::name`
  and a value spelled as the flag spells it, each asked of `Host::tune` (the binary maps it to the
  `Command::Set…` the pane would send, for this run only; the settings file is untouched); a name or
  value the host refuses is `InvalidArgs` and the settings before it stay applied.
  `Reload(ss)` tells a running player that a subcommand wrote the settings file at the path given:
  `Reloadable::name` of the group changed (`equaliser`, `submission`, `sync`, `tidal`, `spotify`, `renderers`) and the path, asked of
  `Host::reload`, which refuses (`InvalidArgs`) a file it does not read or a group it cannot take
  up (`Running::reload`).
  `HandOver(u)` moves what this zone plays onto the zone of that number (`Host::hand_over`, the
  binary's `Zones::hand_over` from the service's own zone; `audio.md`): answered once the handover
  has begun (`HandedOver::Moving`), its landing logged by the engine, never awaited on the bus
  thread; a host with no zones, an unknown number or a handover refused is `InvalidArgs`
  (`Running::hand_over`, `Running::zone_number`). Each zone's service then reads what its own
  player publishes, so the outgoing one falls to `Stopped` with an empty track list and the
  incoming one takes the song, position and tracks with no call of its own.

## The track list

`Tracks` = `Queued::in_list_order` as object paths, `AddTrack`/`RemoveTrack` =
`Command::Insert`/`Remove`, four signals diffed from the 200 ms `POLL` that also drives `Player`
property changes. **The list is the queue unshuffled, not the play order** (`Queued::in_list_order`:
the unshuffled order with rows queued next after the row heard, worked out on the bus thread from
the vectors the publish shares), so shuffling moves nothing on the bus
and play-order positions never reach a client; ids are found by id, not position
(`a_shuffle_leaves_the_track_list_as_it_stood_and_announces_nothing_of_it`). The order it does play
in is `org.resonate.Player1`'s `PlayOrder` (`ao`, `Queued::rows` as paths, heard rows first), read on
demand and never announced. Ids:
`/org/resonate/track/…`, `/org/resonate/playlist/…` (spec reserves `/org/mpris`).

- **Moves announced edit by edit where possible.** `tracklist::change` keys rows by id, holds the
  longest in-order run still, announces the rest as `TrackRemoved`/`TrackAdded` ordered so every
  `AfterTrack` names an announced row. `TrackListReplaced` for: shuffle-sized reorder, no shared
  row, duplicate ids, a kept id whose row differs, more than `EDITS_ANNOUNCED_AT_MOST` (64) edits,
  and equal rows with a moved `Queued::rows_revision` (`change_between`; the only trace of a row
  added and removed within one poll, and unlike `Queued::revision` a reorder leaves it).
- **`GetTracksMetadata` answers positionally**: one entry per id; an unknown id gets
  `mpris:trackid` alone. One `READ_BUDGET` (500 ms) per call: a row with unread tags answers with
  its file stem and `Owed` remembers it; the poll watches `Player::media_revision`, and reads the owed rows again on every poll while any is owed (a
  read can land before the call notes its row, and the revision then never moves again), and emits
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
  (`track::iso_8601`: `YYYY-MM-DDT00:00:00Z`; out-of-range month or day (one the month lacks, `2021-02-30`, included) falls to the first; no
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
  sweeps `resonate-art-<pid>-*` of dead processes (by `/proc/<pid>` on Linux alone; everywhere else,
  Windows and macOS having no `/proc` and a sweep that read its absence as every other player dead
  taking living players' covers, a folder holds a
  `.held` file under `writer::Held` for its life and is swept where `writer_of` reads it `Gone`). A queued row's cover is laid only when a client
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

**Every zone is a player of its own on the bus, under a name its number keeps.** The window's first
zone (and headless `resonate play`) claims `org.mpris.MediaPlayer2.resonate`; zone *N* claims
`org.mpris.MediaPlayer2.resonate.zone<N>` (`service::bus_name_of`), its fallback when that is taken
`…resonate.instance<pid>.zone<N>` (`instance_name`). Why `zone<N>` and not `instance<pid>`: the spec
lets a player append a dot and any identifier unique to it (an element not starting with a digit),
naming `instance<pid>` only as an example; a pid changes every run and says nothing of which zone,
while a zone's number is kept with it (`library.md`'s zones table) so a script or `--player zone2`
names the same zone each run. `Mpris::start` takes the `ZoneId`; each zone's service is its own
connection, poll thread, notifications and idle inhibit, and only the first registers with BlueZ
(`answers_headsets`). **`Identity` names the zone once there are several** (`Resonate (Study)`, the
binary's `identity_of`; `Resonate` alone with one zone) and `org.resonate.Player1`'s `Zone` (`s`)
always names it (`Host::zone`, the first zone's default name where the host has none); the poll diffs
both and announces a rename. `Device` (`s`, never announced, read when asked) names the device the
zone plays to: the binary's `Host::device` is the description of the sink its output sounds on, else
of the sink chosen, else of the default, and empty where none is known (`Running::device` answers
`None` for empty; `Call::Device` on a pipe or socket). The client finds a zone by number (`numbered`: the plain name and an
`instance` name are zone 1, a name ending `.zone<N>` is zone *N*) or by the name `Zone` reads,
case folded (`Running::zoned`, `ZoneAsked`); `Standing::zone` is what `resonate players` prints
(`a_second_zone_is_a_player_of_its_own_named_for_its_number_and_found_by_number_or_name`). The
crate still sees zones only through `Player` and `Host`: the binary's `Desktop` holds a `Weak<Zones>`
and its zone's id and sends every command through `Zones::send` (`audio.md`).

**`DesktopEntry` = the installed entry, which Flatpak renames**: the binary's `Host` answers
`FLATPAK_ID` where set, for the bus property and a notification's `desktop-entry` hint
(`packaging.md`).

## The desktop's calls

- **Own connection and thread; the poll only leaves errands.** `notify.rs` is what a notification
  says on every platform (`Shown`, `actions`, `pressed`); `desktop.rs`'s `Errands` run on a `Desk`,
  Linux's `desk_bus.rs` (`notifier.rs` raises `org.freedesktop.Notifications` on a track change),
  Windows's `desk_toast.rs` (toasts, `windows.md`: `toast.rs` writes the XML from the same `Shown`
  and `actions`, a press is `notify::pressed` again) or a Mac's `desk_notice.rs` (UserNotifications,
  `macos.md`); a pipe or socket service runs `toast_what_plays` on Windows and macOS alike; `idle.rs` holds the portal's
  `org.freedesktop.portal.Inhibit` while there is sound; `desk_bus.rs` makes both on a second
  connection with `DEADLINE` (2 s) `method_timeout` and a thread reading a bounded channel (on the
  poll thread one unanswered call held every property change 25 s). Inhibit is a *level*:
  `Errands::follow` writes whether the session should be awake into an `AtomicBool` and nudges the
  thread, whose `Gripping` weighs it after every errand (a Windows `Desk` ignores it; `awake.rs`
  of the binary holds the machine awake there). The poll's telling is one function,
  `tell_the_desktop`, for the bus loop and for Windows's pipe service, which has no bus to publish
  on and runs only that (`toast_what_plays`). A failed `Inhibit` still counts as taken.
  Either service missing = debug records, never a refusal to start. `trash.rs` is the same kind
  of errand for the catalog's deletes under Flatpak: `trash_through_the_portal` opens the file and
  passes the descriptor to `org.freedesktop.portal.Trash.TrashFile` on a connection of its own
  (`Trashing`: `Unopened`, `Unreached`, `Refused`), asked by the binary, never by this crate's poll.
  `tests/trash.rs` hosts the real `xdg-desktop-portal` binary on a bus of its own
  (`dbus-run-session`, a bus with nothing to activate, `XDG_DATA_HOME` in the test's folder) and
  trashes a file through it, skipping where no portal is installed. The portal trashes with GIO's own
  code, which answers `Refused` (result 0, the only trace being its debug log) where the trash
  folder is not mode 0700 (*Directory already exists with bad permissions*); the descriptor kind
  (`O_PATH`, read-write, read-only) makes no difference, and a run against a desktop's own portal
  says nothing of the code until that folder is 0700. **A sandboxed caller is proved by a sandbox
  built to Flatpak's pattern** (`a_file_is_taken_to_the_trash_from_a_sandbox_the_portal_knows_by_its_app_id`):
  `bwrap` with its own pid namespace and an empty root holding `/usr` and a `/.flatpak-info` naming
  an app id and an instance id, `xdg-dbus-proxy --filter` in a sandbox of the same file between it
  and the bus (the portal names the app by the `/proc/<pid>/root/.flatpak-info` of the connection's
  process, which is the proxy's), the portal given a runtime directory of its own holding
  `.flatpak/<instance>/bwrapinfo.json` as `bwrap --info-fd` writes it, and the test binary run inside
  to trash a file on a bound folder, the descriptor crossing the proxy. It skips without `bwrap`,
  `xdg-dbus-proxy` or a portal, or where the kernel refuses an unprivileged sandbox. It is not
  Flatpak itself: no `flatpak run`, no document portal, no permissions of a real manifest.
- **A notification is keyed on what it *says*.** The cover lands a moment after the track starts:
  the second telling replaces the first under the server's `replaces_id`; teardown closes it and
  waits `FAREWELL` (500 ms). Body escaped only where `GetCapabilities` answers `body-markup`.
- **Idle inhibit is not a setting; a notification is.** A desktop mutes an app by the
  `desktop-entry` hint (blunt for a player otherwise wanted), so `Host::notifies` sits beside
  `Host::attended`, weighed into `Telling::of(attended, notifies)`; a track passed over is still
  *noted* as told. `notify` is a key no flag outranks, under the settings pane's *Desktop* category.
- **Transport buttons, answered on the raising connection.** `actions()` = Previous, Play/Pause, Next (keys and the catalog's `notify` words),
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

## The media transport

**On Windows the media keys and the system's player overlay are SMTC, served from the same state and
the same methods as MPRIS** (`windows.md` has the Windows side). `Host::answers_media_keys` (false
by default; the binary's first zone, as `answers_headsets`) starts a `resonate-transport` thread
beside whichever service serves the zone. It reads `snapshot` as the bus poll does and maps it
through `controls_of` into `transport::Controls` (portable, tested on every host: `Status::of`,
`Buttons::of` over the `Can*` the bus publishes, `repeat_code`/`repeat_asked`, `ticks`,
`Timeline`, `Displayed`, `Moved::between`); every request Windows makes is a `PlayerInterface`
method (`Pressed::of` to `play`/`pause`/`stop`/`next`/`previous`, a position to `set_position`,
shuffle and repeat to their setters), so the rules of *A method does what the spec says* hold for a
media key too. Why the first zone and not the zone shown: it is the zone the plain bus name, BlueZ
and `resonate play` already mean, and following the window's choice would need the front end
through a second seam.

**On macOS the same thread tells the system's Now Playing** (`nowplaying.rs`, over
`resonate_apple::NowPlaying`; `macos.md` has the threads): the same `snapshot`, `controls_of` and
`Moved::between`, a press arriving as an `Asked` through a bounded channel the loop waits on in
place of its sleep, each answered by the same `PlayerInterface` methods. Now Playing has no call
for shuffle or repeat, so `transport.rs` compiles those and the Windows codes for Windows and the
tests alone. The crate reaches `resonate-apple` on macOS for this and nothing else.

## Is the connection metered

On Windows `costing.rs`'s `watch_the_meter` has the same signature and the same contract (the first
read made before it returns, `Unknown` where nothing is known) over
`NetworkInformation::GetInternetConnectionProfile().GetConnectionCost().NetworkCostType()` and its
`NetworkStatusChanged` event, mapped by `cost::meter_of_network_cost` (`windows.md`). On macOS
`pathing.rs`'s holds a `resonate_apple::PathMonitor` (the Network framework's `nw_path_monitor`, its
updates on a dispatch queue of its own) on `resonate-metered` for the life of the process, and
`route::meter_of_the_path` maps each `Route`: no satisfied path is `Unknown`, an expensive one
(cellular, a phone's hotspot) or a constrained one (Low Data Mode) `Metered`, any other `Unmetered`.
`Unknown` is reported before the monitor starts, and its first update comes within moments. The
rest of this section is NetworkManager's.

`metered.rs`'s `watch_the_meter(Metering)` reads NetworkManager's `Metered` property
(`org.freedesktop.NetworkManager` at `/org/freedesktop/NetworkManager`) over the system bus and
reports it into a `resonate_core::Metering`: 1 (yes) and 3 (guess yes) are `Meter::Metered`, 2 and
4 `Unmetered`, 0 and any other number `Unknown` (`meter_of_network_manager`). It subscribes to the
manager's `PropertiesChanged` before the first read, and does that first read on the caller's
thread, so a lookup made straight after is already gated; the rest runs on `resonate-metered`. A
change that names `Metered` is taken from the signal, one that only invalidates it is read again.
**No system bus ends the watch** (`Err` at once, the connection stays `Unknown`). **It is asked only
where `org.freedesktop.NetworkManager` has an owner** (`Error::NobodyOwns` otherwise), as BlueZ is,
because a call to the name would D-Bus-activate a NetworkManager the listener keeps stopped; a
manager that is not there or does not answer reports `Unknown` and is asked again after `ASKED_AGAIN_AFTER` (a minute), so one
that starts later is found. The crate only reports; what a metered connection stops is the
binary's (`binary.md`). Proved under `dbus-run-session` with `DBUS_SYSTEM_BUS_ADDRESS` at the
session bus and a stand-in manager (`bus.rs`'s `the_meter_follows_what_network_manager_says_of_the_connection`,
`a_connection_is_not_metered_where_no_network_manager_answers`); the values a real manager sends
for a hotspot have not been seen (`docs/TESTING.md`).

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
  `Session` is one bus connection (on Windows and macOS none where no bus answers, only the registry of players) a long-lived client (MCP) reuses through `found_in`, `listed_in`
  and `named_in`; `found`, `listed` and `named` open their own.
- **`resonate queue --playlist <NAME>` hands a playlist over as its rows** (`ActivatePlaylist`
  replaces a queue). A search-filled playlist is read via `Library::playlist_entries`, so queued as
  whatever it matches then. `queue` opens the library only for `--playlist`.
- **The window is single.** The window's host answers `CanRaise`/`Raise`; a launch finding a player
  of this build able to raise (`Running::a_window`) hands it its files as `AddTrack`s placed next
  and heard now, raises it and leaves; the engine flags it was given (`--sink`, `--quality`,
  `--filter-phase`, `--dither`, `--noise-shaping`, `--bit-perfect`, `--no-bit-perfect`) go over
  `Tune`, while one giving `--config`, `--library` or `--vault` (`settings_of_its_own`) never asks
  for a window and opens its own (`binary.md`). Headless `resonate play` can't raise. The name is claimed
  only after the engine starts, so `starting::one_window_at_a_time` takes an exclusive lock on
  `resonate-starting.lock` under `$XDG_RUNTIME_DIR` (none: `resonate-starting-<uid>.lock` in the
  temp folder) before asking the bus and holds it until `Mpris::start` claimed the name; a second
  launch waits at most `WAITS_AT_MOST` (15 s); an untakeable lock starts the window as before.
- **A player is chosen by name; one reading of the bus serves every way of choosing.** `ours` =
  names that are `org.mpris.MediaPlayer2.resonate` or anything under it (an `instance`, a
  `zone<N>`), plain name first, so `found` and `a_window` land on the first zone.
  `Running::found` = first, `listed` = all, `named` = the one that `answers_to` a `PlayerName`
  (whole bus name or `instance<pid>` alone), so `resonate players` prints what `--player` takes.
  `Standing` = what one answers about itself (`Option`s: nothing vs empty differ); a player that
  stops answering between listing and read is left out. `PlaybackStatus` lives in `track.rs` beside
  the (lossy) mapping from `PlaybackState`.

## The client's second wire

**`Running` is one API over two wires, as Discord's `Session` is** (`windows.md` for the pipe itself).
`peer.rs`'s `Peer` trait is every call a client makes, in domain values (`TrackId`, `Described`,
`Option<Asleep>`, micros as `i64`, `Reloadable`), and `Running` is the composition over it
(`queue`'s anchor arithmetic and chunking, `standing`, `a_window`, the `Placement` landing); no
caller learns which wire answered. `bus_peer.rs`'s `BusPeer` is the D-Bus implementation (the
`AddTracks` to `AddTrack` fallback is inside it); `PipePeer` speaks `wire.rs` over any
`Read + Write + Send` stream, so the host tests run it over a `UnixStream` pair. `Session` holds an
`Option<Connection>` and lists the players of every wire it has (the bus's names, on Windows the
registry's), sorted by name, so `found`, `listed`, `named` and `zoned` are the same on both.
`Running::through(stream, name)` is the seam a test or another transport enters by.

- **`wire.rs` is hand-written, with no serde.** A frame is a `u32` little-endian length (at most
  `FRAME_AT_MOST`, 64 MiB, refused before anything is read) and a body: a request is a `Call` tag
  and its fields, a reply a tag and its value; text is a `u32` length and UTF-8, a count is checked
  against the bytes left before anything is reserved, an id of zero is `WireFault::ZeroId`. Every
  `Request` and `Reply` is written and read back in `wire::tests`, and a test holds that each
  `Call` has a request. A pipe carries the **same calls** as the bus, not a second vocabulary:
  `Described` crosses as the URI (`to_uri_within`), title, artist, album, length and cover URL the
  bus's metadata map carries. A reply of the wrong shape is `WireFault::Unexpected { call }`, a
  refusal is `Error::Refused { call, fault }` with `Fault::{InvalidArgs, NotSupported, Failed}`
  (the three kinds of `fdo::Error` the bus answers), never text.
- **`answering.rs`'s `Answering` is the server end**, built over the same `Shared` and the same
  interface structs the bus serves (`PlayerInterface`, `TrackList`, `OwnInterface`, whose methods
  are `pub(crate)` for it), so a pipe call and a bus call run the same code: `can_go_next`,
  "ignored" seeks, `Host::tune`/`reload`/`hand_over` refusals, `Owed` and the read budget of
  `GetTracksMetadata`. `serve(stream)` answers requests until the peer leaves; a request it cannot
  read is `InvalidArgs` and an unknown tag `NotSupported` (an older server meeting a newer client),
  and `close` ends every connection at its next request. It is public so a test can serve a
  `UnixStream` (`bus.rs`'s `piped`: transport, queue, queueing of a run longer than one call,
  tune and reload refusals, each through `Running::through`).
- **A pipe link has a clock the bus has in its connection.** `Link` runs a stream's blocking I/O
  on a thread and a call waits `ANSWERS_WITHIN`; a timeout or a closed pipe marks the link broken
  for good, so no later call reads an earlier call's reply (`peer::tests`).
- **`registry.rs` is the portable discovery** (a held file per player in a folder, `Held` and
  `writer_of`, tested on every host) and `pipe.rs` the `interprocess` half, `cfg(windows)`.
- **`listening.rs` is the accept loop both transports share** (`accepting`, over a closure that
  accepts and a `Serves<S>` that `Answering` fills): at most `CONNECTIONS_AT_MOST` (32) clients at
  once, each on a thread of its own, a client refused costing the rest nothing and a listener that
  refuses `ACCEPT_FAILURES_AT_MOST` (5) times in a row given up on
  (`every_client_accepted_is_served_and_a_refused_one_costs_the_rest_nothing`,
  `a_listener_that_keeps_refusing_is_given_up_on`).
- **`socket.rs` is a Mac's transport, over `std::os::unix::net` and no further dependency**
  (compiled for macOS and, for its tests, on every Unix). The registry's folder there is
  `~/Library/Caches/resonate/players` (`registry::players_folder_of_a_mac`; the temporary folder's
  `resonate-players` only with no `HOME`), never `$TMPDIR`, whose files macOS sweeps after days
  unread. `kept_to_this_user` makes it `0700` and refuses to serve in one another user owns or can
  reach (`PermissionDenied` as `PipeOp::Claim`). **The claim is the authority, the socket only
  where it listens**: `Serving::start_in` takes the name's `Claim` first (the held file, so a
  second player moves on to its `instance` name), unlinks whatever socket stands at the name's
  path (stale, since the claim is held), binds and narrows it to `0600`; Windows listens first
  because a pipe's namespace is global, here the folder is private. A socket is named `socket` and
  what follows `org.mpris.MediaPlayer2.resonate` in the player's name (`socket`, `socket.zone2`,
  `socket.instance4242.zone2`), never the whole name, since `sun_path` holds 103 bytes on a Mac
  (`the_longest_name_a_player_is_likely_to_take_still_binds_under_a_long_home_on_a_mac`); a home
  long enough to pass it fails the bind as `PipeOp::Listen` and the player runs unserved. A client
  lists the registry and connects to each name's socket (`serving_in`); a player whose socket does
  not answer is passed over. `rest` wakes the accept by connecting to itself, removes the socket
  and lets the claim go; a Mac's `terminate:` drops nothing, so the next claimant's unlink is what
  clears a socket left behind (`a_player_is_reached_on_the_socket_beside_its_claim_and_leaves_neither_behind`,
  `a_socket_a_dead_player_left_is_taken_over_by_the_next_to_claim_its_name`,
  `a_folder_others_can_reach_is_refused_rather_than_served_in`).

- **The desktop's own preferences are read at start and watched after** (`preferences.rs`, `desktop_preferences`, `watch_the_desktop`): `org.freedesktop.appearance` `contrast`, `reduced-motion` and `color-scheme` over the portal, GNOME's `enable-animations` as the fallback for motion, the text scale from GNOME's `text-scaling-factor` else KDE's `font` point size over ten, each answered within 300 ms or `Unsaid`; the binary seeds High Contrast and Reduce motion from the start read only where the listener stored neither, and the colour scheme and text scale follow `SettingChanged` on a `resonate-desktop` thread (`ui.md`).

The Linux `watch_game_mode` worker polls the session bus every two seconds for `com.feralinteractive.GameMode.QueryStatus`, asking about this process: 1 and 2 mean active, 0 inactive. It checks the name owner first so an absent daemon is never activated. Calls time out after two seconds; a lost bus clears the gate and reconnects after ten seconds. It reports into `core::gaming`, never on the audio callback.
