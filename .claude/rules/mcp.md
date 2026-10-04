---
paths:
  - "crates/resonate-mcp/**/*.rs"
  - "crates/resonate/src/mcp.rs"
---

# The Model Context Protocol

`resonate mcp` is how a language model reaches the player and the catalog. `resonate-mcp` is on core,
the engine's vocabulary, the library, `resonate-mpris`, the `resonate-providers` seam a poll is handed
and `serde_json`; the binary reaches it behind the `mcp` feature, on by default. A build without it
keeps the subcommand in the grammar (`build.rs` reads `cli.rs` with no features) and answers
`Error::NoMcp`.

## The transport

- **Newline-delimited JSON-RPC 2.0 on stdin and stdout, no async runtime.** `Server::serve` reads a
  line at a time with `read_until`, so a non-UTF-8 line is a parse error rather than the session's
  end, and a blank line is passed over. A line over `LONGEST_MESSAGE` is answered `Refusal::TooLong`
  (`-32600`, the id unread), the rest of it is passed over a buffer at a time and the session goes on.
  The session ends with the input; a failed read or write is the one thing `serve` returns an `Error`
  for.
- **Stdout is the protocol.** Logs go to stderr (`binary.md`); a `println!` on the path of `mcp`
  corrupts the session.
- **Refused and failed are two different answers.** A line that is not JSON, a bad envelope or id, an
  unknown method, tool or resource, undeserialisable parameters and arguments a tool cannot take are a
  `Refusal`, answered as a JSON-RPC error with `Refusal::code`. A tool that ran and failed (no player
  on the bus, a missing playlist or track, the catalog or bus erroring) is a *successful* result with
  `isError` set and the error's whole `source` chain as text; `Tool::run` answers
  `Result<Result<Value>, Refusal>` to keep them apart. A resource and a prompt have no `isError`, so a
  failed read is a JSON-RPC error under `InternalError` with the same chain (`Unanswered`), and a URI
  naming no resource is `Refusal::UnknownResource` under the spec's `-32002`.
- **A notification is never answered**, nor is a message carrying a `result` or `error`, since this
  server sends no requests of its own. **A batch is answered as one array** in the order asked,
  whatever version was negotiated; an empty one is `Refusal::EmptyBatch`.
- `initialize` echoes the protocol version asked for where it is one of `PROTOCOLS`, else the latest.

## What is pushed

- **A resource can be subscribed to, and the resource list watched, where the session can push.**
  `serve_until_stopped` takes its input over a channel, so a third sender, the `resonate-mcp-tick`
  thread, puts a `Tick` on it every `LOOKED_OVER_EVERY`, answered between lines on the one thread.
  On a tick each subscribed URI is read again and, where the reading differs from the last (a failed
  read is a reading too), `notifications/resources/updated` names it; once the client has listed
  resources, a changed list is `notifications/resources/list_changed`. So a running pass is pushed
  through `resonate://library/passes`. A reading that moves with the clock is told as changed on every
  tick.
- **`serve` pushes nothing**, since it reads stdin on the thread answering: `initialize` offers
  `subscribe` and `listChanged` only where `pushing` was set, and a subscription asked of a session
  that cannot push is `UnknownMethod`. Subscriptions are keyed by the URI the client asked under.

## The resources, prompts and completions

- **A resource is a tool's reading under a URI, and the same reading.** `Resource::read` calls the
  functions the read-only tools do, at their defaults, shared rather than copied
  (`a_resource_reads_what_the_tool_of_the_same_reading_answers`). `resonate://player/…` reaches the
  running player and fails as the transport tools do where there is none; `resonate://library/…`
  reads the catalog with no player. `resonate://library/playlist/{name}` and
  `resonate://library/statistics/{window}` are templates; a name goes through core's `uri_escaped` /
  `uri_unescaped` and is found as `playlist_tracks` finds it. A read answers under the URI asked for,
  since that is what a client keys the reading to.
- **A prompt is an instruction with a resource embedded beside it.** The embedding is
  `Resource::embedded`, so a prompt cannot say what the resource would not, and the instruction names
  each tool through `Tool::name`, so a renamed tool cannot leave a prompt pointing at nothing. Refused
  as a tool is (`UnknownPrompt`, `BadPromptArguments`, `BlankArgument` under `InvalidParams`).
- **Every argument a client fills says how it completes, so none answers nothing.** A prompt's
  `Argument` and the URI `Template`s carry a `Completable`; `build_a_playlist`'s `brief` completes its
  last words through `Library::names_completing` (artists, albums and genres, never titles). At most a
  hundred values with the total, `hasMore` saying the rest were left out. An unoffered prompt, argument
  or template is refused (`UnknownPrompt`, `UnknownArgument`, `UnknownResource`).

## The tools

- **Catalog tools read and write a `Library` directly and answer with no player running.** Transport
  tools reach a player through `Reach` per call, so a player started after the session began is found
  and a missing one fails that tool alone.
- **`Controlling` is the seam over `Running`, and `Reach` how one is found** (`Running::found`, or
  `Running::named` under `--player`), so the tests prove each tool with a fake and no bus.
- **A `track_id` is the catalog's and a `queue_id` the queue's**, different numbers. A `queue_id` is
  written as text: the queue mints ids down from `u64::MAX` and a client reading JSON numbers as
  doubles would round it to another row.
- **A list of ids is held to `MOST_ROWS`**, as a search's `limit` is: every id array's schema says
  `maxItems`, and more is `Refusal::TooMany`.
- **A queueing that fails says how far it got** (`Error::QueuedPartway`, the rows that landed),
  since `Running::queue` falls back to one `AddTrack` a row against an older player.
- **A result leaves out what nothing answered** rather than writing `null`, and is one object twice:
  `structuredContent` and a text block of its JSON.
- **A transport tool answers what the player reads back once the gesture landed**, `seek` the landed
  position and `moved_seconds` as the distance actually moved. `play_playlist` is the exception:
  `ActivatePlaylist` is the front end's, not the engine's, and answers once handed over; it resolves
  the name through the playlists the *player* offers, exact spelling first, then ignoring case.
- **`show_queue` describes only what it lists**: it asks `GetTracksMetadata` about the first `limit`
  alone, so a queue of thousands stays inside the service's read budget.

## What a model may change

- **The edits are the library's own calls, holding to its rules.** `mark_favourite` is
  `Library::favour_all`, every id in one transaction, so an unknown id anywhere answers
  `UnknownTrack`, `UnknownAlbum` or `UnknownArtist` and marks none. The playlist tools are the
  library's calls of those names (`create_playlist`, `start_playlist`, `save_query` with `fills_from`,
  `remove_playlist`), so refusals are the window's, and adding to a self-filling playlist is the
  library's `NotAList`. Their undo stacks are this process's, so a model's change is not on the
  window's *Undo*, though a window beside the session draws it (`Library::written_elsewhere`).
- **A playlist row is named by where it sits**: `playlist_tracks` answers each `row`, and
  `remove_from_playlist` takes a run from `row` to `through_row` (one `Span`, one statement, one undo
  step) or every row a search matches. A row past the end is `Error::NotInThePlaylist`.
- **A missing track is named by its `release_track_id`.** `want_tracks` is
  `Library::want_release_tracks` over the whole list in one transaction, so a list naming one unknown
  row wants none of it.
- **What a tool may destroy is said.** `Tool::destroys` is `destructiveHint` (the removals, and
  `play_playlist`, which replaces a queue); `Tool::reaches_the_network` is `openWorldHint`
  (`start_lookup` and `start_poll` alone).
- **An unreadable combination is a refusal**: `OneOf`, `AtLeastOneOf`, `AtMostOneOf`, and `BlankField`
  for a blank `query` or `fills_from`, which the grammar reads as no condition and would take the
  first rows of the whole library (`a_blank_query_or_search_is_refused_rather_than_taken_as_the_whole_library`).

## The long passes

- **A scan, a lookup and a poll are started, then asked after, each outlasting a call.** `start_scan`,
  `start_lookup` and `start_poll` answer at once; `library_passes` answers for each `idle`, `running`
  with the progress snapshot, `finished` with the summary, or `failed` with the error's chain.
  `stop_pass` cancels one at its next file. `Passes` holds one `Slot` per pass behind a `RefCell` (the
  server answers on one thread), and a finished pass is joined when first asked after. A second start
  of a running pass is `Error::AlreadyRunning`.
- **Passes start as the command line starts them.** A scan refuses a non-folder with
  `Error::NoSuchFolder` before anything starts, then hands the folders to `Library::scan`, which
  registers them as roots in one transaction once it holds the walk, so a refused scan keeps none of
  them (`a_scan_refused_before_its_walk_starts_keeps_none_of_its_roots`). What the passes need from
  outside arrives as `Lookups`, filled by the binary from what `resonate enrich` and `resonate poll`
  read; `Server::new` alone carries `Lookups::none()`, so a session with no network answers
  `start_lookup` with `Error::NoReference` as that tool's failure, not a refusal.
- **An ending session leaves no pass half-written.** `serve` drains the passes when the input ends
  (each running one cancelled and joined, so the catalog follows) and `Drop for Passes` drains them
  every other way out. **A signal ends it the same way**: `resonate mcp` serves through
  `serve_until_stopped`, stdin is read on a thread of its own, and the binary hands `Stop::stop` to
  `signals::cancel_when_told`, so `SIGTERM` stops a scan at a file boundary rather than killing it
  mid-file, with its input still open; a second signal leaves at once.
