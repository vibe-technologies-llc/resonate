---
paths:
  - "crates/resonate-mcp/**/*.rs"
  - "crates/resonate/src/mcp.rs"
---

# The Model Context Protocol

`resonate mcp` is what a language model reaches the player and the catalog through. The crate
behind it, `resonate-mcp`, is on core, the engine's vocabulary, the library, `resonate-mpris`, the
`resonate-providers` seam a poll is handed, `serde`, `serde_json`, `ahash`, `thiserror` and
`tracing`, and the binary reaches it behind the `mcp` feature, which is on by default the way `online` is. A build without it keeps the
subcommand in the grammar, because `build.rs` reads `cli.rs` with no features, and answers
`Error::NoMcp`.

## The transport

- **Newline-delimited JSON-RPC 2.0 on stdin and stdout, with no async runtime.** `Server::serve`
  reads a line at a time with `read_until`, not `lines`, so a line that is not UTF-8 is answered
  as a parse error rather than ending the session, and a blank line is passed over. The session
  ends when the input does; a failed read or write is the one thing `serve` returns an `Error`
  for.
- **Stdout is the protocol, so the logs go to stderr for this subcommand alone.**
  `main::logs_to` picks the writer from the parsed command, which is why `Cli::parse` now runs
  before `init_logging`. A `println!` anywhere on the path of `mcp` corrupts the session rather
  than printing a stray line.
- **What is refused and what fails are two different answers.** A line that is not JSON, an
  envelope that is not JSON-RPC 2.0, an id that is `null` or neither a string nor a number, an
  unknown method, parameters that do not deserialise, an unknown tool and arguments a tool
  cannot take are all a `Refusal`, answered as a JSON-RPC error with the code `Refusal::code`
  names. A tool that ran and failed — no player on the bus, a playlist or a track that is not
  there, the catalog or the bus erroring — is a *successful* result with `isError` set and the
  error's whole `source` chain as its text. Deciding which is which is the point of the two
  types: `Tool::run` answers `Result<Result<Value>, Refusal>`. A resource has no `isError`, so a
  read that ran and failed is a JSON-RPC error too, but under `Code::InternalError` with the same
  chain as its message — `Unanswered` is the two kinds of error answer — and a URI naming no
  resource is `Refusal::UnknownResource` under `ResourceNotFound`, the `-32002` the spec gives it.
- **A notification is never answered**, whatever its method, and neither is a message carrying
  a `result` or an `error`, because this server sends no requests of its own to be answered.
- `initialize` echoes the protocol version asked for where it is one of `PROTOCOLS` and answers
  the latest otherwise. The server offers tools, resources, prompts and completions;
  `listChanged` is false for the first three, because the tools are `Tool::ALL`, the prompts `Prompt::ALL`, and a client lists
  the resources again whenever it wants the playlists as they stand, and `subscribe` is false
  because the one thread that reads stdin has nothing to write an update from. The instructions
  say that the three long passes run in the background, how to ask after them, that the readings
  are resources too, what the prompts are for and that their arguments complete.

## The resources

- **A resource is a tool's reading under a URI, and it is the same reading.** `Resource::read`
  calls the very functions the read-only tools do — `transport::now_playing` and `queue`,
  `Passes::states`, and `catalog::playlists`, `playlist_tracks`, `favourites`, `statistics`,
  `suggestions` and `missing` — at the tools' own defaults, which are shared rather than copied,
  so `a_resource_reads_what_the_tool_of_the_same_reading_answers` holds each one to the tool
  called with no arguments. What a model gains is the client's choice: a resource can be put in
  front of it before it asks anything.
- **Two kinds of URI.** `resonate://player/…` reaches the running player and fails as the
  transport tools do where there is none; `resonate://library/…` reads the catalog with no player.
  `resources/list` is the eight fixed ones and then one per playlist the catalog holds, named as
  the playlist is, and `resonate://library/playlist/{name}` is also offered as a template, so a
  client that does not list can still name one. `resonate://library/statistics/{window}` is the
  second template, one per `Window` by the name the tool's `window` takes: `Resource::Statistics`
  carries its window, the month's is the fixed one listed under the bare URI, and
  `a_window_of_listening_is_a_resource_under_the_window_it_names` holds each to the tool. The name is written through core's
  `uri_escaped` — the same escaping a `file://` URI takes — and read back through
  `uri_unescaped`, and it is found the way `playlist_tracks` finds it, ignoring case. A name that
  is blank or does not unescape to text is no resource at all rather than a playlist nobody made.
- **A read answers under the URI it was asked for**, not the one the playlist would be listed
  under, because the URI inside `contents` is what a client keys the reading to.

## The prompts

- **A prompt is an instruction and a resource embedded beside it, read when it is asked for.**
  `Prompt::ALL` is four: `build_a_playlist` takes a `brief` and an optional `name` and embeds the
  playlists, so a name already taken is in front of the model; `review_my_listening` takes an
  optional `window` and embeds `Resource::Statistics` for it; `complete_my_albums` embeds the
  missing tracks and asks for agreement before anything is wanted; `about_this_track` embeds what
  is playing. The embedding is `Resource::embedded`, the same `read` a `resources/read` answers
  with under the resource's own URI, so a prompt cannot say something the resource would not, and
  the instruction names each tool through `Tool::name` rather than spelling it, so a renamed tool
  cannot leave a prompt pointing at nothing.
- **A prompt is refused the way a tool is and fails the way a resource does.** A name nobody
  offers is `Refusal::UnknownPrompt`, arguments that do not deserialise — a missing `brief`, a
  `window` that is not one — are `BadPromptArguments` and a `brief` of nothing but space is
  `BlankArgument`, all under `InvalidParams`, which is what the spec gives a bad prompt name or
  argument. `Prompt::get` answers `Result<Result<Value>, Refusal>` like `Tool::run`, and a
  reading that ran and failed — `about_this_track` with no player — is a JSON-RPC error under
  `InternalError`, because a prompt, like a resource, has no `isError` to carry it.

## Completions

- **Every argument a client fills in says how it completes, so none is left answering nothing.**
  `completion/complete` names a prompt or a resource template and one of its arguments, and the
  answer is a `Completable`: a prompt's `Argument` carries one beside its name, and `Template` —
  the two URI templates, which `resources/templates/list` is written from — answers one for
  the argument its URI names. A playlist's `name` offers the playlists the catalog holds and a
  `window` the four `Window` names, those beginning with what was typed before those merely
  holding it, ignoring case. `build_a_playlist`'s `name` offers the suggestions' names no playlist
  already has, because the prompt asks for a new one, and its `brief` completes its last words
  through `Library::names_completing`: the spelling vocabulary the search's *did you mean* reads,
  over the artists, albums and genres and never the titles, a whole name reached by the most words
  typed first, then a whole name before a single word of one, then the name more rows hold. A brief
  that ends in anything but a letter is offered nothing, and what was typed is never offered back.
- **At most a hundred values, with the total**, which is the spec's cap, and `hasMore` says the
  rest were left out. A prompt or an argument nobody offers is refused — `UnknownPrompt`, or
  `UnknownArgument` under `InvalidParams` — and a template nobody offers is `UnknownResource`
  under `ResourceNotFound`, the way a read of one is; a catalog that fails is `InternalError`,
  because a completion has no `isError` either.

## The tools

- **The catalog tools read and write a `Library` directly and answer with no player
  running**, so a question about the library is never refused over the bus. The transport tools
  reach a player through `Reach`, per call, so a player started after the session began is
  found, and one that is not there is a failure of that tool alone.
- **`Controlling` is the seam over `Running`, and `Reach` is how one is found.** `OnTheBus`
  answers `Running::found` or, under `--player`, `Running::named`; the tests register a fake
  that records every call and moves its own state the way a player would, which is what lets
  each tool and what it reads back be proved with no bus. `playback` was split out of `standing`
  so a status is one property read, and `queued` and `described` out of `rows` so the queue's ids
  and its tags are two reads rather than one that fetches everything.
- **A `track_id` is the catalog's and a `queue_id` the queue's**, and the two are not the same
  numbers. `add_to_queue` takes the first — or a search, queued in album order through the same
  grammar the search box reads — and `remove_from_queue` the second.
- **A `queue_id` is written as text.** The queue mints its ids down from `u64::MAX`, and a client
  that reads JSON numbers as doubles rounds `18446744073709551615` to a different row.
- **A result leaves out what nothing answered** rather than writing `null`, and it is the same
  object twice: `structuredContent` and a text block carrying its JSON, which is what a client
  that reads only the text still understands.
- **A transport tool answers with what the player reads back once the gesture has landed.** The
  bus settles every call that moves the engine, so `control_playback` answers the status and the
  track that follow, `seek` the position it landed on, `set_volume` the volume read back and
  `add_to_queue` and `remove_from_queue` how many rows the queue holds now. `play_playlist` is the
  exception: `ActivatePlaylist` is the front end's, not the engine's, and answers as soon as it is
  handed over.
- **`show_queue` describes only what it lists.** `Controlling` reads the queue's ids — one
  property — and asks `GetTracksMetadata` about the first `limit` of them alone, so a queue of
  thousands costs one list of paths rather than every row's tags under the service's 500 ms
  budget. `remove_from_queue` weighs a `queue_id` against the ids alone for the same reason.
- `play_playlist` resolves the name through the playlists the *player* offers on the bus rather
  than the catalog's own, so the id it activates is one that player can open, exact spelling first
  and then ignoring case.

## What a model may change

- **The edits are the library's own calls, so they hold to its rules.** `mark_favourite` is
  `Library::favour` per id and counts the marks that moved; `create_playlist` is
  `create_playlist`, `start_playlist` or — with `fills_from` — `save_query` in relevance order;
  `add_to_playlist`, `remove_from_playlist` and `rename_playlist` are the library's calls of the
  same names and `discard_playlist` is `Library::remove_playlist`, so a duplicate name, a blank
  one and a row another source names are refused the way the window refuses them. A playlist that fills itself is not a list, and adding to or
  removing from one is the library's `NotAList` failure rather than a refusal. The undo stacks
  those calls push onto are this process's, so what a model changed is not on the window's
  *Undo* — though a window running beside the session draws it once the writes settle, through
  `Library::written_elsewhere`.
- **A row of a playlist is named by where it sits**: `playlist_tracks` answers each row's `row`,
  and `remove_from_playlist` takes one, a run from `row` to `through_row` — one `Span`, one
  statement and one undo step — or every row a search matches. A row past the end is
  `Error::NotInThePlaylist` rather than nothing.
- **A missing track is named by its `release_track_id`.** `list_missing` is
  `Library::missing_tracks` under the same search narrowing `resonate missing` takes, and
  `want_tracks` is `Library::want` for each, which stands a want the providers fill as any other.
- **What a tool may destroy is said.** `Tool::destroys` is `destructiveHint`: the two removals,
  `discard_playlist` and `play_playlist`, which replaces a queue.
- A combination that cannot be read is a refusal: `OneOf` where exactly one field must be given,
  `AtLeastOneOf` for a mark naming nothing and `AtMostOneOf` for a playlist started from two
  sources at once.

## The long passes

- **A scan, a lookup and a poll are started and then asked after, because each outlasts a call.**
  `start_scan`, `start_lookup` and `start_poll` start the library's own pass and answer at once;
  `library_passes` answers, for each of the three, `idle`, `running` with the progress's snapshot,
  `finished` with the summary's stats and whether it was cancelled — and, for a lookup the
  reference ended, what it was asking for — or `failed` with the error's chain. `stop_pass`
  cancels one at its next file and answers where it stands. `Passes` holds one `Slot` per pass
  behind a `RefCell`, because the server answers on one thread, and a finished pass is joined
  the first time it is asked after, so its summary is read once and kept. A second start of a
  pass the session is still running is `Error::AlreadyRunning`; a scan beside another process's
  walk is the library's own `AlreadyWalking`.
- **The passes are started the way the command line starts them.** A scan adds each folder it is
  given as a root — refusing one that is not a folder, `Error::NoSuchFolder`, before anything is
  kept, so every path in the list is weighed before the first is added, because `add_root`
  commits each on its own — and walks every root where it is given none, incrementally and with covers, on the
  machine's parallelism; a lookup carries on one a run left unfinished unless asked to refresh,
  and honours the `study` setting; a poll asks every registered provider. What the passes need
  from outside the crate arrives as `Lookups` — the reference, the fingerprinters, the providers
  and the study switch — which the binary fills from the same `online::` and `providers::`
  functions `resonate enrich` and `resonate poll` read, and `Server::new` alone carries
  `Lookups::none()`, so a build or a session with no network answers `start_lookup` with
  `Error::NoReference` as a failure of that tool rather than a refusal.
- **A session that ends does not leave a pass half-written.** `serve` drains the passes when the
  input ends — each running one is cancelled and joined, so the file being written is finished
  and the catalog follows it — and `Drop for Passes` drains them on every other way out.
  `a_session_that_ends_under_a_running_scan_waits_for_it_to_stop` is the claim.
