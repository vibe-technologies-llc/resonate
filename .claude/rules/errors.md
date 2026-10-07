# Error handling

Per crate: `thiserror` enum `Error` + `pub type Result<T> = std::result::Result<T, Error>`
(providers use `resonate_providers::Error`). Variants: typed domain values, plus for a foreign
failure a `#[source]` paired with a typed op enum.

## Rules

- **No stringly descriptions**: no `String`, `&str`, `Box<str>`, `Cow<str>`, `Box<dyn Error>`,
  `anyhow` as an *explanation* (callers must `match`). String newtypes only to *identify*
  (`NodeName`, `TagName`, `PathBuf`): `SinkInfo.name` may appear, `.description` (`String`) not.
- **Opaque foreign error + op enum**: `pipewire::Error` wraps `spa::utils::result::Error` (hides its
  `Errno`); `Daemon { op: PwOp, #[source] source }` names the call.
- **Untypeable text goes to `tracing`**: the error stays matchable, the prose is a log record,
  wherever a foreign API offers only `Display` (`gpui::App::open_window`'s `anyhow::Error` cannot
  even be a `#[source]`).
- **A field named `source` is the `#[source]`**, any type; name domain values for what they are
  (`provider`, `sink`, `track`).
- **`transparent` only where the layer adds no context; `#[from]` only where the lower error has
  every field the upper would add.** Else hand-write the wrapper, context mandatory:
  `engine::Error::Decode { track, source }` (`codec::Error` has no `TrackId`).
- **No `#[non_exhaustive]`** (internal; forces `_ =>` arms in exhaustive matches).
- **Remove unconstructed variants**, caller-less getters and their state; a variant lands with the
  code raising it.
- **`size_of::<Error>() <= 128`**: `result_large_err` denied workspace-wide; guard test per crate.
  Box the *named* foreign error (`Box<rusqlite::Error>`), never `Box<dyn Error>`.

## Structural enforcement

Prefer failing to compile over documenting.

- `codec::Error::Symphonia` has no `#[from]`: `?` on `symphonia::Result` fails; `from_symphonia` is
  the only way in (typed variants cannot rot into a catch-all).
- `engine::Error` cannot grow `Codec(#[from] codec::Error)` beside `Decode { track, .. }` (duplicate
  `From`); supply the `TrackId`.
