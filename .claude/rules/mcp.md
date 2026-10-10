---
paths:
  - "crates/resonate-mcp/**/*.rs"
  - "crates/resonate/src/mcp.rs"
---

# The Model Context Protocol

`resonate mcp` gives a language model the player and catalog. `resonate-mcp` depends on core, engine
vocabulary, library, `resonate-mpris`, the `resonate-providers` seam (a poll takes it),
`serde_json`. The binary reaches it behind the default `mcp` feature; without it the subcommand
stays in the grammar (`build.rs` reads `cli.rs` featureless) and answers `Error::NoMcp`.

## Transport

- **Newline-delimited JSON-RPC 2.0 on stdin/stdout, no async runtime.** `Server::serve` uses
  `read_until`: non-UTF-8 line = parse error, not session end; blank line skipped. A line over
  `LONGEST_MESSAGE` gets `Refusal::TooLong` (`-32600`, id unread), the rest skipped a buffer at a
  time. Session ends with input; only a failed read/write makes `serve` return `Error`.
- **Stdout is the protocol**: logs go to stderr (`binary.md`); a `println!` corrupts it.
- **Refused vs failed.** `Refusal` = JSON-RPC error via `Refusal::code`: non-JSON, bad envelope/id,
  unknown method/tool/resource, undeserialisable parameters, arguments a tool cannot take. A tool
  that ran and failed (no player, missing playlist/track, catalog/bus error) = *successful* result
  with `isError` and the whole `source` chain as text; `Tool::run` returns `Result<Result<Value>,
  Refusal>` to keep them apart. Resources/prompts have no `isError`: a failed read is JSON-RPC
  `InternalError` with the same chain (`Unanswered`); a URI naming no resource is
  `Refusal::UnknownResource`, spec `-32002`, as is a playlist resource naming no playlist (read or
  subscribed: the template answers any name, the catalog decides).
- **Never answered**: notifications, messages with `result`/`error` (no requests of its own). **A
  batch gets one array**, in order, whatever version was negotiated; empty = `Refusal::EmptyBatch`.
- `initialize` echoes the asked version if in `PROTOCOLS`, else the latest.

## Pushed

- **Subscriptions and list watching, where the session can push.** `serve_until_stopped` takes input
  over a channel; a third sender, thread `resonate-mcp-tick`, puts a `Tick` on it every
  `LOOKED_OVER_EVERY` (500 ms), answered between lines on the one thread. Each tick re-reads every
  subscribed URI; a differing reading (a failed read counts) sends
  `notifications/resources/updated`. Once the client has listed resources, a changed list sends
  `notifications/resources/list_changed`. So a running pass is pushed via
  `resonate://library/passes`; a clock-moving reading is changed every tick.
- **`serve` pushes nothing** (stdin read on the answering thread): `initialize` offers `subscribe`/
  `listChanged` only where `pushing` was set; subscribing without it = `UnknownMethod`.
  Subscriptions are keyed by the URI as asked.

## Resources, prompts, completions

- **A resource is a tool's reading under a URI, the same reading**: `Resource::read` calls the
  read-only tools' functions at their defaults, shared not copied
  (`a_resource_reads_what_the_tool_of_the_same_reading_answers`). `resonate://player/…` reaches the
  running player (fails as transport tools do with none); `resonate://library/…` reads the catalog,
  no player. Templates: `resonate://library/playlist/{name}`,
  `resonate://library/statistics/{window}`; a name goes through core's
  `uri_escaped`/`uri_unescaped`, found as `playlist_tracks` finds it. A read answers under the URI
  asked (what clients key on).
- **A prompt = instruction + embedded resource** (`Resource::embedded`: cannot say what the resource
  would not); the instruction names tools via `Tool::name` (a rename cannot dangle). Refusals as
  tools': `UnknownPrompt`, `BadPromptArguments`, `BlankArgument` (`InvalidParams`).
- **Every client-filled argument says how it completes**: prompt `Argument`s and URI `Template`s
  carry a `Completable`; `build_a_playlist`'s `brief` completes its last words via
  `Library::names_completing` (artists, albums, genres; never titles). At most a hundred values plus
  the total, `hasMore` for the rest. Unoffered prompt/argument/template: `UnknownPrompt`,
  `UnknownArgument`, `UnknownResource`.

## Tools

- **Catalog tools use a `Library` directly**, no player needed. Transport tools find a player via
  `Reach` per call: one started mid-session is found; a missing one fails that tool alone.
  `Controlling` is the seam over `Running`, `Reach` finds one (`Running::found`, or `Running::named`
  under `--player`); tests use a fake, no bus.
- **`track_id` = catalog's, `queue_id` = queue's**, different numbers. `queue_id` is text: the queue
  mints ids down from `u64::MAX`, which JSON-numbers-as-doubles clients would round to another row.
- **Id lists held to `MOST_ROWS`** (1000) like a search's `limit`: every id array's schema has
  `maxItems`; more = `Refusal::TooMany`.
- **A failed queueing says how far it got** (`Error::QueuedPartway`, rows landed): `Running::queue`
  falls back to one `AddTrack` per row against an older player.
- **Results omit what nothing answered** (no `null`) and are one object twice: `structuredContent`
  and a text block of its JSON.
- **Transport tools answer what the player reads back once the gesture landed**: `seek` the landed
  position and `moved_seconds` (distance actually moved). Exception `play_playlist`:
  `ActivatePlaylist` is the front end's, not the engine's, so it answers once handed over; it
  resolves the name via the playlists the *player* offers, exact spelling then ignoring case.
- **`show_queue` describes only what it lists**: `GetTracksMetadata` for the first `limit` rows, so
  thousands of rows stay in the service's read budget.

## What a model may change

- **Edits are the library's calls, under its rules.** `mark_favourite` = `Library::favour_all`, one
  transaction: an unknown id anywhere answers `UnknownTrack`/`UnknownAlbum`/`UnknownArtist`, marks
  none. Playlist tools call `create_playlist`, `start_playlist`, `save_query` (with `fills_from`),
  `remove_playlist`: refusals are the window's; adding to a self-filling playlist = `NotAList`. Undo
  stacks are this process's: a model's change is not on the window's *Undo*, though a window beside
  the session draws it (`Library::written_elsewhere`).
- **A playlist row is named by position**: `playlist_tracks` answers each `row`;
  `remove_from_playlist` takes `row`..`through_row` (one `Span`, statement and undo step; a
  `through_row` before `row` is `Refusal::OutOfRange`, never the range turned round) or every row a
  search matches; past the end = `Error::NotInThePlaylist`. **Only rows asked for are read**:
  `Library::playlist_entries_within` puts the limit in `LIMIT` (a saved query's cap lowered to it),
  counts `matched` separately, so a hundred-thousand-row or self-filling playlist costs a few
  hundred. The resource list uses `Library::playlist_names`, no row counted (read every tick).
- **A missing track is named by `release_track_id`.** `want_tracks` = `Library::want_release_tracks`
  over the list in one transaction: one unknown row wants none.
- **`Tool::destroys` = `destructiveHint`**: the removals, `forget_folder`, `undo_edit` (may discard
  a playlist it made), `play_playlist` (replaces a queue). **`Tool::reaches_the_network` =
  `openWorldHint`**: `start_lookup`, `start_poll` only.
- **A session undoes only its own edits.** `undo_edit` = `Library::undo` (`Library::redo` with
  `redo`) over this process's stack: the session tools' playlist edits, nothing from the window or
  another process; a playlist since changed by anything else = `Error::PlaylistChanged` (that tool's
  failure). Answers what was walked (`edit`, `playlist`, `more_to_undo`) or `walked: null`.
- **Unreadable combinations are refusals**: `OneOf`, `AtLeastOneOf`, `AtMostOneOf`, `BlankField` for
  a blank `query`/`fills_from` (read as no condition, it would take the first rows of the whole
  library; `a_blank_query_or_search_is_refused_rather_than_taken_as_the_whole_library`).

## Long passes

- **Scan, lookup, poll: started, then asked after.** `start_scan`/`start_lookup`/`start_poll` answer
  at once; `library_passes` gives each `idle`, `running` (progress snapshot), `finished` (summary)
  or `failed` (error chain); `stop_pass` cancels at the next file. `Passes` holds a `Slot` per pass
  in a `RefCell` (one answering thread); a finished pass is joined when first asked after. Starting
  a running pass = `Error::AlreadyRunning`.
- **Passes start as the CLI starts them.** `start_scan` takes `full` and `follow_links` as
  `resonate scan` takes `--full` and `--follow-links` (incremental, off links unless told). Where
  `Lookups::after_a_scan` (the `enrich-after-scan` key) holds and a reference is reached, a scan
  run to its end hands over to a lookup as the CLI's does: `Passes::carry_on`, asked before every
  message and on every tick, starts it once the scan slot settles uncancelled (`lookup_after` in the
  answer says it will;
  `a_scan_run_to_its_end_hands_over_to_a_lookup_where_the_settings_ask_for_one`). A scan refuses a
  non-folder (`Error::NoSuchFolder`) before anything starts; `Library::scan` registers the roots in one transaction once it holds the
  walk, so a refused scan keeps none
  (`a_scan_refused_before_its_walk_starts_keeps_none_of_its_roots`). **Admission first**: filesystem
  root = `Error::FilesystemRoot`; a list taking the library past `MOST_ROOTS` (64) =
  `Error::TooManyRoots`; `forget_folder` is the way back (`Library::remove_root` with its tracks;
  `Error::NotARoot` for a never-kept folder). Outside needs arrive as `Lookups`, filled by the
  binary from what `resonate enrich`/`resonate poll` read; `Server::new` alone has
  `Lookups::none()`, so `start_lookup` with no network = `Error::NoReference` (that tool's failure,
  not a refusal).
- **An ending session leaves no pass half-written.** `serve` drains passes when input ends (each
  running one cancelled and joined, so the catalog follows); `Drop for Passes` on every other exit.
  **A signal ends it likewise**: `resonate mcp` serves through `serve_until_stopped` (stdin on its
  own thread) and hands `Stop::stop` to `signals::cancel_when_told`, so `SIGTERM` stops a scan at a
  file boundary, input still open; a second signal leaves at once. **`Stop::stop` never waits**: it
  sets a flag the loop reads before each message and only `try_send`s the wake-up (channel holds
  one; a busy session leaves it full), so the signal thread is free for the second signal however
  long a call runs
  (`a_stop_told_again_while_the_session_is_busy_never_holds_up_the_one_telling_it`). A drain cancels
  every running pass before joining any, so they wind down together.

