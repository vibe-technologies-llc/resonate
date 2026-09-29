---
paths:
  - "crates/resonate-mcp/**/*.rs"
  - "crates/resonate/src/mcp.rs"
---

# The Model Context Protocol

`resonate mcp` is how a language model reaches the player and the catalog. `resonate-mcp` is on
core, the engine's vocabulary, the library, `resonate-mpris`, the `resonate-providers` seam a poll
is handed, `serde`, `serde_json`, `ahash`, `thiserror` and `tracing`; the binary reaches it behind
the `mcp` feature, on by default like `online`. A build without it keeps the subcommand in the
grammar (`build.rs` reads `cli.rs` with no features) and answers `Error::NoMcp`.

## The transport

- **Newline-delimited JSON-RPC 2.0 on stdin and stdout, no async runtime.** `Server::serve` reads a
  line at a time with `read_until`, not `lines`, so a non-UTF-8 line is a parse error rather than
  the session's end, and a blank line is passed over. `next_line` reads it through a `take` of
  `LONGEST_MESSAGE` (4 MiB) and one byte, so a client sending bytes with no newline cannot grow the
  line past that: a line running over is answered `Refusal::TooLong` (`-32600`, the id unread) and
  the rest of it is passed over a buffer at a time, the session going on with the next line
  (`a_line_longer_than_a_message_may_be_is_refused_and_passed_over_whole`). The session ends with the
  input; a failed read or write is the one thing `serve` returns an `Error` for.
- **Stdout is the protocol, so logs go to stderr for this subcommand alone.** `main::logs_to` picks
  the writer from the parsed command, which is why `Cli::parse` runs before `init_logging`. A
  `println!` on the path of `mcp` corrupts the session.
- **Refused and failed are two different answers.** A line that is not JSON, an envelope not
  JSON-RPC 2.0, an id `null` or neither string nor number, an unknown method, undeserialisable
  parameters, an unknown tool and arguments a tool cannot take are a `Refusal`, answered as a
  JSON-RPC error with the code `Refusal::code` names. A tool that ran and failed — no player on the
  bus, a missing playlist or track, the catalog or bus erroring — is a *successful* result with
  `isError` set and the error's whole `source` chain as text; `Tool::run` answers
  `Result<Result<Value>, Refusal>` to keep them apart. A resource has no `isError`, so a read that
  failed is a JSON-RPC error under `Code::InternalError` with the same chain (`Unanswered` is the
  two kinds of error answer), and a URI naming no resource is `Refusal::UnknownResource` under
  `ResourceNotFound`, the spec's `-32002`.
- **A notification is never answered**, whatever its method, nor is a message carrying a `result`
  or `error`, since this server sends no requests of its own.
- `initialize` echoes the protocol version asked for where it is one of `PROTOCOLS`, else the
  latest. It offers tools, resources, prompts and completions; `listChanged` is false for the first
  three (the tools are `Tool::ALL`, the prompts `Prompt::ALL`, and a client lists resources again
  whenever it wants the playlists as they stand) and `subscribe` false, since the one thread
  reading stdin has nothing to write an update from. The instructions say the three long passes run
  in the background and how to ask after them, that the readings are resources too, what the
  prompts are for and that their arguments complete.

## The resources

- **A resource is a tool's reading under a URI, and the same reading.** `Resource::read` calls the
  functions the read-only tools do — `transport::now_playing` and `queue`, `Passes::states`,
  `catalog::playlists`, `playlist_tracks`, `favourites`, `statistics`, `suggestions` and `missing`
  — at the tools' own defaults, shared rather than copied, and
  `a_resource_reads_what_the_tool_of_the_same_reading_answers` holds each to the tool called with
  no arguments. The model gains the client's choice: a resource can be put in front of it before
  it asks anything.
- **Two kinds of URI.** `resonate://player/…` reaches the running player and fails as the transport
  tools do where there is none; `resonate://library/…` reads the catalog with no player.
  `resources/list` is the eight fixed ones, then one per playlist, named as the playlist is;
  `resonate://library/playlist/{name}` is also a template, so a client that does not list can name
  one. `resonate://library/statistics/{window}` is the second template, one per `Window` by the
  tool's `window` names: `Resource::Statistics` carries its window, the month's being the fixed one
  under the bare URI (`a_window_of_listening_is_a_resource_under_the_window_it_names`). A name is
  written through core's `uri_escaped` (the escaping a `file://` URI takes), read back through
  `uri_unescaped` and found as `playlist_tracks` finds it, ignoring case; a blank name or one that
  does not unescape to text is no resource rather than a playlist nobody made.
- **A read answers under the URI asked for**, not the one the playlist is listed under, since the
  URI in `contents` is what a client keys the reading to.

## The prompts

- **A prompt is an instruction with a resource embedded beside it, read when asked for.**
  `Prompt::ALL` is four: `build_a_playlist` takes a `brief` and optional `name` and embeds the
  playlists, so a taken name is in front of the model; `review_my_listening` takes an optional
  `window` and embeds `Resource::Statistics` for it; `complete_my_albums` embeds the missing tracks
  and asks for agreement before anything is wanted; `about_this_track` embeds what is playing. The
  embedding is `Resource::embedded` — the `read` a `resources/read` answers under the resource's
  own URI — so a prompt cannot say what the resource would not, and the instruction names each tool
  through `Tool::name`, so a renamed tool cannot leave a prompt pointing at nothing.
- **Refused as a tool is, failing as a resource does.** An unoffered name is
  `Refusal::UnknownPrompt`, undeserialisable arguments (a missing `brief`, a `window` that is not
  one) `BadPromptArguments`, and a `brief` of only space `BlankArgument`, all under
  `InvalidParams` as the spec gives a bad prompt name or argument. `Prompt::get` answers
  `Result<Result<Value>, Refusal>` like `Tool::run`, and a reading that failed —
  `about_this_track` with no player — is a JSON-RPC error under `InternalError`, a prompt having no
  `isError`.

## Completions

- **Every argument a client fills says how it completes, so none answers nothing.**
  `completion/complete` names a prompt or resource template and one argument; the answer is a
  `Completable`: a prompt's `Argument` carries one beside its name, and `Template` — the two URI
  templates `resources/templates/list` is written from — answers one for the argument its URI
  names. A playlist `name` offers the catalog's playlists and a `window` the four `Window` names,
  those beginning with what was typed before those merely holding it, ignoring case.
  `build_a_playlist`'s `name` offers suggestion names no playlist has (the prompt asks for a new
  one), and its `brief` completes its last words through `Library::names_completing` — the
  spelling vocabulary the search's *did you mean* reads, over artists, albums and genres, never
  titles — a whole name reached by the most words typed first, then a whole name before a single
  word of one, then the name more rows hold. A brief ending in anything but a letter is offered
  nothing, and what was typed is never offered back.
- **At most a hundred values, with the total** (the spec's cap), `hasMore` saying the rest were
  left out. An unoffered prompt or argument is refused — `UnknownPrompt`, or `UnknownArgument`
  under `InvalidParams` — an unoffered template is `UnknownResource` under `ResourceNotFound` as a
  read of one is, and a failing catalog `InternalError`, a completion having no `isError` either.

## The tools

- **Catalog tools read and write a `Library` directly and answer with no player running**, so a
  question about the library is never refused over the bus. Transport tools reach a player through
  `Reach` per call, so a player started after the session began is found and a missing one fails
  that tool alone.
- **`Controlling` is the seam over `Running`, and `Reach` how one is found.** `OnTheBus` answers
  `Running::found` or, under `--player`, `Running::named`; the tests register a fake recording
  every call and moving its own state as a player would, proving each tool and its read-back with
  no bus. `playback` was split from `standing` so a status is one property read, and `queued` and
  `described` from `rows` so the queue's ids and tags are two reads, not one fetching everything.
- **A `track_id` is the catalog's and a `queue_id` the queue's** — different numbers.
  `add_to_queue` takes the first, or a search queued in album order through the search box's
  grammar; `remove_from_queue` the second.
- **A `queue_id` is written as text.** The queue mints ids down from `u64::MAX`, and a client
  reading JSON numbers as doubles rounds `18446744073709551615` to another row.
- **A result leaves out what nothing answered** rather than writing `null`, and is one object
  twice: `structuredContent` and a text block of its JSON, for a client reading only text.
- **A transport tool answers what the player reads back once the gesture landed.** The bus settles
  every call moving the engine, so `control_playback` answers the following status and track,
  `seek` the landed position, `set_volume` the volume read back, `add_to_queue` and
  `remove_from_queue` the queue's row count now. `play_playlist` is the exception:
  `ActivatePlaylist` is the front end's, not the engine's, and answers once handed over.
- **`show_queue` describes only what it lists.** `Controlling` reads the queue's ids (one property)
  and asks `GetTracksMetadata` about the first `limit` alone, so a queue of thousands costs one
  list of paths, not every row's tags under the service's 500 ms budget; `remove_from_queue` weighs
  a `queue_id` against the ids alone likewise.
- `play_playlist` resolves the name through the playlists the *player* offers on the bus, not the
  catalog's, so the id it activates is one that player can open — exact spelling first, then
  ignoring case.

## What a model may change

- **The edits are the library's own calls, holding to its rules.** `mark_favourite` is
  `Library::favour` per id, counting the marks that moved; `create_playlist` is `create_playlist`,
  `start_playlist` or — with `fills_from` — `save_query` in relevance order; `add_to_playlist`,
  `remove_from_playlist` and `rename_playlist` are the library's calls of those names and
  `discard_playlist` is `Library::remove_playlist`, so a duplicate or blank name and a row another
  source names are refused as the window refuses them. A self-filling playlist is not a list, so
  adding to or removing from one is the library's `NotAList` failure, not a refusal. Those calls'
  undo stacks are this process's, so a model's change is not on the window's *Undo* — though a
  window beside the session draws it once the writes settle, through `Library::written_elsewhere`.
- **A playlist row is named by where it sits**: `playlist_tracks` answers each row's `row`, and
  `remove_from_playlist` takes one, a run from `row` to `through_row` — one `Span`, one statement,
  one undo step — or every row a search matches. A row past the end is `Error::NotInThePlaylist`.
- **A missing track is named by its `release_track_id`.** `list_missing` is
  `Library::missing_tracks` under the search narrowing `resonate missing` takes, and `want_tracks`
  is `Library::want` for each, standing a want the providers fill like any other.
- **What a tool may destroy is said.** `Tool::destroys` is `destructiveHint`: the removals
  `remove_from_playlist`, `discard_playlist` and `remove_from_queue`, and `play_playlist`, which
  replaces a queue.
- An unreadable combination is a refusal: `OneOf` where exactly one field must be given,
  `AtLeastOneOf` for a mark naming nothing, `AtMostOneOf` for a playlist started from two sources.

## The long passes

- **A scan, a lookup and a poll are started, then asked after, each outlasting a call.**
  `start_scan`, `start_lookup` and `start_poll` start the library's pass and answer at once;
  `library_passes` answers for each `idle`, `running` with the progress snapshot, `finished` with
  the summary's stats and whether it was cancelled — and, for a lookup the reference ended, what it
  was asking — or `failed` with the error's chain. `stop_pass` cancels one at its next file and
  answers where it stands. `Passes` holds one `Slot` per pass behind a `RefCell` (the server answers
  on one thread), and a finished pass is joined when first asked after, its summary read once and
  kept. A second start of a running pass is `Error::AlreadyRunning`; a scan beside another
  process's walk is the library's `AlreadyWalking`.
- **Passes start as the command line starts them.** A scan adds each folder given as a root —
  refusing a non-folder with `Error::NoSuchFolder` before anything is kept, every path weighed
  before the first is added, since `add_root` commits each alone — and walks every root where given
  none, incrementally, with covers, on the machine's parallelism; a lookup carries on one a run
  left unfinished unless asked to refresh, honouring `study`; a poll asks every registered
  provider. What the passes need from outside arrives as `Lookups` — reference, fingerprinters,
  providers, the study switch — filled by the binary from the `online::` and `providers::`
  functions `resonate enrich` and `resonate poll` read; `Server::new` alone carries
  `Lookups::none()`, so a build or session with no network answers `start_lookup` with
  `Error::NoReference` as that tool's failure, not a refusal.
- **An ending session leaves no pass half-written.** `serve` drains the passes when the input ends
  — each running one cancelled and joined, so the file being written is finished and the catalog
  follows — and `Drop for Passes` drains them every other way out
  (`a_session_that_ends_under_a_running_scan_waits_for_it_to_stop`).
