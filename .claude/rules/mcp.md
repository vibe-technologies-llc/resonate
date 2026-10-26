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
  `read_until`: a non-UTF-8 line is a parse error, not session end; a blank line is skipped. A line
  over `LONGEST_MESSAGE` gets `Refusal::TooLong` (`-32600`, id unread). The session ends with input;
  only a failed read/write makes `serve` return `Error`.
- **Stdout is the protocol**: logs go to stderr (`binary.md`).
- **Refused vs failed.** `Refusal` is a JSON-RPC error via `Refusal::code`: non-JSON, bad
  envelope/id, unknown method/tool/resource, undeserialisable parameters, arguments a tool cannot
  take. A tool that ran and failed (no player, missing playlist/track, catalog/bus error) is a
  *successful* result with `isError` and the whole `source` chain as text; `Tool::run` returns
  `Result<Result<Value>, Refusal>` to keep them apart. Resources/prompts have no `isError`: a failed
  read is `InternalError` with the same chain (`Unanswered`); a URI naming no resource, or a
  playlist resource naming no playlist, is `Refusal::UnknownResource` (`-32002`).
- **Never answered**: notifications, messages with `result`/`error`. **A batch gets one array**, in
  order; empty is `Refusal::EmptyBatch`. `initialize` echoes the asked version if in `PROTOCOLS`,
  else the latest.

## Pushed

- **Subscriptions and list watching, where the session can push.** `serve_until_stopped` takes input
  over a channel; thread `resonate-mcp-tick` puts a `Tick` on it every `LOOKED_OVER_EVERY` (500 ms),
  answered between lines on the one thread. Each tick re-reads every subscribed URI; a differing
  reading (a failed read counts) sends `notifications/resources/updated`. Once the client has listed
  resources, a changed list sends `notifications/resources/list_changed`: a tick reads the playlist
  names only when `Library::catalog_revision` moved and compares a hash of them
  (`resources::fingerprint`), so a quiet catalog costs one counter read.
- **`serve` pushes nothing**: `initialize` offers `subscribe`/`listChanged` only where `pushing` was
  set; subscribing without it is `UnknownMethod`. Subscriptions are keyed by the URI as asked.
  Unsubscribing drops the URI unconditionally, so a playlist discarded or renamed since is still
  let go.

## Resources, prompts, completions

- **Every list is paged by `cursor`.** `tools/list`, `resources/list`, `resources/templates/list` and
  `prompts/list` answer `LISTED_A_PAGE` (100) entries and a `nextCursor`, the decimal offset of the
  next; a cursor that is not one is `Refusal::BadCursor` (`-32602`). The resource page reads only its
  own playlist names (`resources::page_from`, one extra to know whether more follow), never every
  playlist.

- **A resource is a tool's reading under a URI, the same reading**: `Resource::read` calls the
  read-only tools' functions at their defaults
  (`a_resource_reads_what_the_tool_of_the_same_reading_answers`). `resonate://player/…` reaches the
  running player; `resonate://library/…` reads the catalog, no player. Templates:
  `resonate://library/playlist/{name}`, `resonate://library/statistics/{window}`; names go through
  core's `uri_escaped`/`uri_unescaped`. A read answers under the URI asked.
- **A prompt is an instruction plus an embedded resource** (`Resource::embedded`); the instruction
  names tools via `Tool::name`, so a rename cannot dangle. Refusals: `UnknownPrompt`,
  `BadPromptArguments`, `BlankArgument`.
- **Every client-filled argument says how it completes**: prompt `Argument`s and URI `Template`s
  carry a `Completable`; `build_a_playlist`'s `brief` completes its last words via
  `Library::names_completing` (artists, albums, genres; never titles). At most a hundred values plus
  the total, `hasMore` for the rest. Unoffered ones: `UnknownPrompt`, `UnknownArgument`,
  `UnknownResource`.

## Tools

- **Catalog tools use a `Library` directly.** Transport tools find a player via `Reach` per call (one
  started mid-session is found; a missing one fails that tool alone). `OnTheBus` keeps one
  `resonate_mpris::Session` for the whole run and opens a new one only after a bus error, so a
  subscribed now-playing costs no connection a tick. `Controlling` is the seam over
  `Running`; tests use a fake, no bus.
- **`track_id` is the catalog's, `queue_id` the queue's.** `queue_id` is text: the queue mints ids
  down from `u64::MAX`, which clients reading JSON numbers as doubles would round to another row.
- **Id lists are held to `MOST_ROWS`** (1000): every id array's schema has `maxItems`; more is
  `Refusal::TooMany`.
- **A failed queueing says how far it got** (`Error::QueuedPartway`).
- **Results omit what nothing answered** (no `null`) and are one object twice: `structuredContent`
  and a text block of its JSON. Each tool lists an `outputSchema` (`outputs.rs`, `Tool::gives`)
  that the test helper `called` holds every answer to; a field a tool can leave out is not
  `required`.
- **Transport tools answer what the player reads back once the gesture landed** (`seek`: landed
  position and `moved_seconds`). Exception `play_playlist`: `ActivatePlaylist` is the front end's,
  so it answers once handed over, resolving the name via the playlists the *player* offers.
- **`show_queue` describes only what it lists**: `GetTracksMetadata` for the first `limit` rows.
  The rows are the MPRIS `Tracks` list, the queue unshuffled, never play order (`mpris.md`), so the
  answer carries `shuffle` and the tool and resource descriptions say so. Under shuffle each row also
  carries `play_position`, its place from 0 in the order the player plays (the rows already heard
  first), read from `Controlling::play_order` (`org.resonate.Player1`'s `PlayOrder`); absent when
  unshuffled, and a player too old to have the property leaves it out rather than failing the
  listing (`a_shuffled_queue_gives_each_row_the_place_it_plays_at`).

## What a model may change

- **Edits are the library's calls, under its rules.** `mark_favourite` is `Library::favour_all`, one
  transaction: an unknown id anywhere answers `UnknownTrack`/`UnknownAlbum`/`UnknownArtist` and marks
  none. Playlist tools call `create_playlist`, `start_playlist`, `save_query` (with `fills_from`),
  `remove_playlist`; adding to a self-filling playlist is `NotAList`. **A model's playlist edit is
  on the window's *Undo* too**: `resonate mcp` calls `Library::offer_edits_as_a_model`, so every step
  it stacks is also written to the catalog for a window to take (`library.md`'s *Undo*); a
  favourite, a want or a pass has no undo in the window either and is offered nothing.
- **A playlist row is named by position**: `playlist_tracks` answers each `row`;
  `remove_from_playlist` takes `row`..`through_row` (one `Span`; a `through_row` before `row` is
  `Refusal::OutOfRange`) or every row a search matches; past the end is `Error::NotInThePlaylist`.
  **Only rows asked for are read**: `Library::playlist_entries_within` puts the limit in `LIMIT` and
  counts `matched` separately, so a huge or self-filling playlist costs a few hundred rows. The
  resource list uses `Library::playlist_names`, no row counted. `list_playlists` answers `limit`
  (default 200) playlists from `skip` with `matched` counting them all (`Library::playlists_within`);
  the playlists resource is its first page.
- **A missing track is named by `release_track_id`.** `want_tracks` is `Library::want_release_tracks`
  in one transaction: one unknown row wants none.
- **`Tool::destroys` is `destructiveHint`**: the removals, `forget_folder`, `undo_edit`,
  `play_playlist`. **`Tool::reaches_the_network` is `openWorldHint`**: `start_lookup`, `start_poll`.
- **A session undoes only its own edits.** `undo_edit` is `Library::undo` (`Library::redo` with
  `redo`) over this process's stack; a playlist since changed by anything else (a window that took
  the step and walked it back included) is `Error::PlaylistChanged`. Undoing withdraws the step's
  offer, redoing offers it again. Answers `edit`, `playlist`, `more_to_undo`, or `walked: null`.
- **Unreadable combinations are refusals**: `OneOf`, `AtLeastOneOf`, `AtMostOneOf`, `BlankField` (a
  blank `query`/`fills_from` would read as no condition and take the whole library;
  `a_blank_query_or_search_is_refused_rather_than_taken_as_the_whole_library`).

## Long passes

- **Scan, lookup, poll: started, then asked after.** `start_scan`/`start_lookup`/`start_poll` answer
  at once; `library_passes` gives each `idle`, `running` (progress), `finished` (summary) or
  `failed` (error chain); `stop_pass` cancels at the next file. `Passes` holds a `Slot` per pass in a
  `RefCell` (one answering thread). Starting a running pass is `Error::AlreadyRunning`.
- **Passes start as the CLI starts them.** `start_scan` takes `full` and `follow_links` like
  `resonate scan`. Where `Lookups::after_a_scan` (`enrich-after-scan`) holds and a reference is
  reached, `Passes::carry_on` (asked before every message and on every tick) starts a lookup once the
  scan settles uncancelled (`lookup_after` in the answer;
  `a_scan_run_to_its_end_hands_over_to_a_lookup_where_the_settings_ask_for_one`). A non-folder is
  `Error::NoSuchFolder` before anything starts; `Library::scan` registers the roots in one
  transaction once it holds the walk, so a refused scan keeps none
  (`a_scan_refused_before_its_walk_starts_keeps_none_of_its_roots`). **Admission first**: filesystem
  root is `Error::FilesystemRoot`; a list taking the library past `MOST_ROOTS` (64) is
  `Error::TooManyRoots`; `forget_folder` (`Library::remove_root` with its tracks; `Error::NotARoot`)
  is the way back. `Lookups` is filled by the binary; `Server::new` alone has `Lookups::none()`, so
  `start_lookup` with no network is `Error::NoReference` (a tool failure, not a refusal).
- **An ending session leaves no pass half-written.** `serve` drains passes when input ends (each
  running one cancelled and joined, all cancelled before any is joined); `Drop for Passes` on every
  other exit. **A signal ends it likewise**: `resonate mcp` serves through `serve_until_stopped` and
  hands `Stop::stop` to `signals::cancel_when_told`, so `SIGTERM` stops a scan at a file boundary,
  input still open; a second signal leaves at once. **`Stop::stop` never waits**: it sets a flag the
  loop reads before each message and only `try_send`s the wake-up, so the signal thread is free for
  the second signal however long a call runs
  (`a_stop_told_again_while_the_session_is_busy_never_holds_up_the_one_telling_it`).
