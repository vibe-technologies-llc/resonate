# AGENTS.md

For coding agents other than Claude Code. Claude Code loads `CLAUDE.md` and `.claude/rules/` itself;
any other agent reads them itself, and this file says which, when, and what is not negotiable.

## Read before you touch anything

1. **`CLAUDE.md`**: the project, the crates and how they may depend on each other, the invariants
   the layering protects, the commands. With the rules it is the authority on the design; this file
   does not repeat it.
2. **`.claude/rules/rust-style.md`** and **`.claude/rules/errors.md`**: they apply to every Rust
   file.
3. **Every other `.claude/rules/*.md` whose scope covers a file you are about to change.** Each
   opens with a YAML `paths:` list of globs; a file you edit matching one makes that rules file
   required reading first. One with no `paths:` applies everywhere. `CLAUDE.md`'s table says what
   each covers.

The rules are dense prose written for this: most odd-looking choices in the code have their reason
there, and "simplifying" one away reintroduces the bug it was there for.

## Not negotiable

- **Never write a comment.** No `//`, `///`, `//!` or `/* */`, nor the equivalent in any other
  language, shell and YAML included; no exception for invariants, footguns or protocol quirks. What
  a comment would say goes into the code (a named `const`, a newtype, an enum variant, an extracted
  function) or, for the wider design, into `CLAUDE.md` or `.claude/rules/`. Editing a file that has
  comments, delete those in the code you touch.
- **Format with `rust-formatter`, never `cargo fmt` or `rustfmt`.** It drives nightly rustfmt with
  `StdExternalCrate` grouping and crate-level import merging, and formats `.toml` too; `cargo fmt`
  formats to different rules. `rust-formatter --check` is the read-only check. Where it is not
  installed, say so rather than reaching for `cargo fmt`.
- **Never put the maintainer's name, e-mail address or repository details anywhere**: not a request
  header or User-Agent, payload, URL, fixture, default value or generated code. Software identifies
  itself by name and version only (`resonate/<version>`); a contact is sent only where the listener
  typed one into the `contact` setting, empty by default.
- **Errors are typed.** One `thiserror` `Error` per crate; no `String`/`anyhow`/`Box<dyn Error>` as
  a description; opaque foreign errors paired with an operation enum; no `#[non_exhaustive]`; no
  variant nothing constructs; `size_of::<Error>() <= 128`. `errors.md` has the whole of it.
- **`parking_lot` locks, never `std::sync`'s.** `AHashMap`/`AHashSet` only where keys are our own;
  SipHash wherever they arrive from outside.
- **The crate layering is enforced.** `cargo tree -p <crate>` is the authority; the `layering` job
  in `.github/workflows/ci.yml` refuses the forbidden edges.
- **A stored format is migrated, not broken.** A SQLite schema change is a step appended to
  `MIGRATIONS`; `V1` and existing steps are never edited. Delete-and-rescan only for data that truly
  cannot be carried forward. The same default holds for profiles and the config file.
- **Never run a write pass against a real music library.** `organise --apply`, `tag --apply` and
  `vault --import --apply` work on a copy, never on the listener's own files.

## How work is done here

- **Do the work, not a document about it.** No plan files, design docs or handoff write-ups unless
  asked; the review happens in the diff and in git.
- **Anything that edits, commits or pushes runs one agent at a time.** Read-only work (audits,
  search, review) may fan out; work changing the tree does not. Where several agents are used, each
  result is verified by running the project's checks, not by trusting its report, and its diff is
  read for hunks that trace back to nothing asked for.
- **Commit messages**: one sentence-case imperative line saying what changed for the listener or the
  code (*Keep the Discord session over a refused activity*), with a body where the why is not
  obvious.
- **Playlists are finished.** Do not propose playlist work or reopen a playlist item; playlist code
  changes only because something else broke there.

## Checks

Run before committing; `.github/workflows/ci.yml` runs them, bar the formatter, on every push to
`master` and every pull request, beside the layering refusals (`build.md` has CI, the release
workflow, the profiles and the fuzz targets).

```
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace --exclude resonate-ui --no-default-features   # the usual inner loop
cargo test  --workspace                                               # everything, gpui included
RESONATE_BENCH_CEILINGS=1 cargo bench -p resonate-dsp --bench stages  # DSP costs under ceilings
rust-formatter --check                                                # local only; not in CI
cd fuzz && cargo +nightly fuzz build                                  # the parsers' fuzz targets
```

Tests needing a PipeWire daemon, a session bus, ffmpeg or the network skip where it is missing
(`CLAUDE.md` lists which, and which host their own).

## Keeping the rules true

The rules are worth reading only while they describe the code, so a change that makes one wrong
updates it **in the same commit**:

- A change to shipped behaviour or a design decision goes into the `.claude/rules/` file whose scope
  covers it, in the same register: what the thing is, the rule, and why (the failure it prevents,
  measured where it was measured).
- A new crate, subcommand or invariant goes into `CLAUDE.md`, moving its crate count; a config key
  into `ConfigKey::ALL`, with any decision behind it in `binary.md`. A new crate with rules of its
  own gets a `.claude/rules/<name>.md` with a `paths:` list and a row in `CLAUDE.md`'s table.
- `docs/TODO.md` holds **open work only**: `## Category` headings of `- Item` bullets by importance,
  nice-to-haves under `Later:` headings at the bottom, anything waiting outside this tree marked
  **Blocked on …**. Drop an item as it lands, add what the work uncovers, write what a landed item
  became into the rules.
- A new command worth running before a commit goes into `CLAUDE.md`'s commands, this file's checks
  and `.github/workflows/ci.yml` together.
- This file names the rules and says how to read them; it does not restate the design. Where it and
  `CLAUDE.md` or `.claude/rules/` disagree, the rules win and this file is fixed.
