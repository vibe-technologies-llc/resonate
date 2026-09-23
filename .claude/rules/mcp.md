---
paths:
  - "crates/resonate-mcp/**/*.rs"
  - "crates/resonate/src/mcp.rs"
---

# The Model Context Protocol

`resonate mcp` is what a language model reaches the player and the catalog through. The crate
behind it, `resonate-mcp`, is on core, the engine's vocabulary, the library, `resonate-mpris`,
`serde`, `serde_json`, `ahash`, `thiserror` and `tracing`, and the binary reaches it behind the
`mcp` feature, which is on by default the way `online` is. A build without it keeps the
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
  types: `Tool::run` answers `Result<Result<Value>, Refusal>`.
- **A notification is never answered**, whatever its method, and neither is a message carrying
  a `result` or an `error`, because this server sends no requests of its own to be answered.
- `initialize` echoes the protocol version asked for where it is one of `PROTOCOLS` and answers
  the latest otherwise. The server offers tools and nothing else: no resources, no prompts, and
  `listChanged` is false because the list is `Tool::ALL`.

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
  `discard_playlist` and `play_playlist`, which replaces a queue. Nothing starts a scan or a
  lookup, which are long passes a session could end in the middle of.
- A combination that cannot be read is a refusal: `OneOf` where exactly one field must be given,
  `AtLeastOneOf` for a mark naming nothing and `AtMostOneOf` for a playlist started from two
  sources at once.
