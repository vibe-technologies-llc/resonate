# Error handling

Every crate exposes a `thiserror` enum named `Error` and
`pub type Result<T> = std::result::Result<T, Error>`; a provider crate uses the seam's
`resonate_providers::Error` rather than its own.

> Every variant is a product of typed domain values and, where a foreign library failed, a
> `#[source]` foreign error paired with a typed **operation** discriminant.

## Rules

- **No `String`, `&str`, `Box<str>`, `Cow<str>`, `Box<dyn Error>` or `anyhow` as an error
  *description*.** A caller must be able to `match` and recover the specifics.
- **A string newtype is allowed only where the string *identifies* something** (`NodeName`,
  `TagName`, a `PathBuf`), never where it *explains*. `SinkInfo`'s `name` is a `NodeName` and may
  appear in an error; its `description` is a bare `String` and may not.
- **Pair an opaque foreign error with an op enum.** `pipewire::Error` wraps
  `spa::utils::result::Error`, which hides its `Errno`, so `Daemon { op: PwOp, #[source] source }`
  supplies which call failed as matchable data.
- **Text that cannot be typed goes to `tracing`.** The error stays matchable; prose is a log
  record. This holds wherever a foreign API offers only `Display`, including
  `gpui::App::open_window`, whose `anyhow::Error` cannot even be a `#[source]`.
- **A variant field named `source` is the `#[source]`, whatever its type**, so name a domain value
  for what it is (`provider`, `sink`, `track`) and leave `source` to the foreign error.
- **`#[error(transparent)]` only where the layer adds no context; `#[from]` only where the lower
  error already carries every field the upper layer would add.** Otherwise write the wrapper by
  hand so the context is mandatory: `engine::Error::Decode { track, source }` exists because
  `codec::Error` has no notion of a `TrackId`.
- **No `#[non_exhaustive]`.** These crates are workspace-internal; it would force `_ =>` arms in the
  matches that should be exhaustive.
- **A variant nothing constructs is taken out, not left for later**, and so is a getter with no
  caller and the state it reads. An enum claims what can happen; write the variant when the code
  raising it lands.
- **Keep `size_of::<Error>()` at or below 128 bytes.** `result_large_err` is denied workspace-wide
  and every crate with an `Error` has a guard test. When one grows, box the *named* foreign error
  (`Box<rusqlite::Error>`), never `Box<dyn Error>`.

## Enforcement that is structural

Prefer a violation that fails to compile over one that is documented.

- `codec::Error::Symphonia` has no `#[from]`, so `?` on a `symphonia::Result` does not compile and
  the `from_symphonia` classifier is the only way in, which keeps the typed variants from rotting
  into an unused catch-all.
- `engine::Error` cannot grow `Codec(#[from] codec::Error)` beside `Decode { track, .. }` (a
  duplicate `From` impl); supply the `TrackId`.
