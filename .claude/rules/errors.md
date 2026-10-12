# Error handling

Per crate: `thiserror` enum `Error` + `pub type Result<T>` (provider crates use
`resonate_providers::{Error, Result}`). Variants: typed domain values, plus for a foreign failure a
`#[source]` paired with a typed op enum.

## Rules

- **No stringly descriptions**: no `String`, `&str`, `Box<str>`, `Cow<str>`, `Box<dyn Error>`,
  `anyhow` as an *explanation*; callers must `match`. String newtypes only to *identify*
  (`NodeName`, `TagName`, `PathBuf`).
- **Opaque foreign error + op enum**: `Daemon { op: PwOp, #[source] source: pipewire::Error }` names
  the call.
- **Untypeable text goes to `tracing`** where a foreign API offers only `Display`
  (`gpui::App::open_window`'s `anyhow::Error`).
- **A field named `source` is the `#[source]`**; name other domain values for what they are
  (`provider`, `sink`, `track`).
- **`transparent` only where the layer adds no context; `#[from]` only where the lower error has
  every field the upper would add.** Else hand-write the wrapper with context:
  `engine::Error::Decode { track, source }` (`codec::Error` has no `TrackId`).
- **No `#[non_exhaustive]`**: internal, and it forces `_ =>` arms.
- **Remove unconstructed variants** and caller-less getters; a variant lands with the code raising it.
- **`size_of::<Error>() <= 128`**: `result_large_err` is denied workspace-wide and every crate has a
  guard test. Box the *named* foreign error (`Box<rusqlite::Error>`), never `Box<dyn Error>`.

## Structural enforcement

- `codec::Error::Symphonia` has no `#[from]`; `from_symphonia` is the only way in, so typed
  variants cannot rot into a catch-all.
- `engine::Error` cannot grow `Codec(#[from] codec::Error)` beside `Decode { track, .. }` (duplicate
  `From`); supply the `TrackId`.
