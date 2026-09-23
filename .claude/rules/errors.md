# Error handling

Every crate exposes a `thiserror` enum named `Error` and
`pub type Result<T> = std::result::Result<T, Error>`.

The governing rule:

> Every variant is a product of typed domain values and, where a foreign library failed, a
> `#[source]` foreign error paired with a typed **operation** discriminant.

## Rules

- **No `String`, `&str`, `Box<str>`, `Cow<str>`, `Box<dyn Error>` or `anyhow` as an error
  *description*.** Treat one as a defect. A caller must be able to `match` on an error and recover
  the specifics programmatically.
- **A string newtype is allowed only when the string *identifies* something** — `NodeName`,
  `TagName`, a `PathBuf`. Forbidden when the string *explains* something. `SinkInfo` holds both:
  `name` is a `NodeName` and may appear in an error; `description` is a bare `String` and may not.
- **When a foreign error is opaque, pair it with an op enum.** `spa::utils::result::Error` hides its
  `Errno` behind a private field with no getter, so `Daemon { op: PwOp, #[source] source }` supplies
  what the errno would have told us — which call failed — as matchable data.
- **Text that cannot be typed goes to `tracing`.** The error value stays matchable; prose is a log
  record. This is the answer wherever a foreign API offers only `Display`, including
  `gpui::App::open_window`, whose `anyhow::Error` cannot even be a `#[source]`.
- **A variant field named `source` is the `#[source]`, whatever its type.** thiserror claims the
  name, so `Unreadable { source: SourceId, op: LyricOp }` fails to compile on `as_dyn_error`. Name
  the domain value for what it is — `provider`, `sink`, `track` — and leave `source` to the foreign
  error.
- **`#[error(transparent)]` only where the layer adds no context**, and **`#[from]` only when the
  lower error already carries every field the upper layer would add.** Otherwise write the wrapper
  by hand so the context is mandatory — `engine::Error::Decode { track, source }` exists because
  `codec::Error` has no notion of a `TrackId`.
- **No `#[non_exhaustive]`.** These crates are workspace-internal; it would force `_ =>` arms in
  exactly the matches that should be exhaustive.
- **A variant nothing constructs is taken out, not left for later.** An enum is a claim about what
  can happen, so a variant no code path reaches is a claim the crate cannot make good on, and a
  `match` that handles it is an arm nothing exercises. The same holds for a getter with no caller
  and for the state it reads: `SinkStream` lost its `state` and `negotiated` slots because the two
  `Arc<Mutex<_>>` the PipeWire callbacks wrote on every state and format change were read by
  nothing, and what the engine needed was already on `StreamEvent`. Write the variant when the code
  that raises it lands.
- **Keep `size_of::<Error>()` at or below 128 bytes.** `result_large_err` is denied workspace-wide
  and every crate has a guard test. When one grows, box the *named* foreign error —
  `Box<rusqlite::Error>` is still statically known and still matchable — never `Box<dyn Error>`.

## Enforcement that is structural

Prefer making a violation fail to compile over documenting it.

- `codec::Error::Symphonia` has no `#[from]`, so `?` on a `symphonia::Result` does not compile and
  the `from_symphonia` classifier is the only way in. That is what stops the typed variants rotting
  into an unused catch-all.
- `engine::Error` cannot grow `Codec(#[from] codec::Error)` beside `Decode { track, .. }` — a
  duplicate `From` impl. The answer when you hit it is to supply the `TrackId`.
