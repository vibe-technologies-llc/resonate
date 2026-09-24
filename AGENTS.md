# AGENTS.md

Guidance for coding agents other than Claude Code working in this repository. Claude Code loads
`CLAUDE.md` and `.claude/rules/` on its own; any other agent has to read them itself, and this file
says which, when, and what is not negotiable.

## Read before you touch anything

1. **`CLAUDE.md`** — the project, the nineteen crates and how they may depend on each other, the
   invariants the layering protects, every command and every config key. It is the authority on
   the design; this file does not repeat it.
2. **`.claude/rules/rust-style.md`** and **`.claude/rules/errors.md`** — they apply to every Rust
   file.
3. **Every other `.claude/rules/*.md` whose scope covers a file you are about to change.** Each one
   opens with a YAML `paths:` list of globs; a file you edit that matches one of them means that
   rules file is required reading first. A rules file with no `paths:` list applies everywhere.
   `CLAUDE.md` has a table naming what each file covers.

The rules are dense prose written for exactly this: most of what looks like an odd choice in the
code has its reason stated there, and "simplifying" it away reintroduces the bug it was there for.

## Not negotiable

- **Never write a comment.** No `//`, `///`, `//!` or `/* */`, and the same in every other language,
  shell and YAML included. There is no exception for invariants, footguns or protocol quirks. What a
  comment would have said goes into the code — a named `const`, a newtype, an enum variant, an
  extracted function — or, where it is about the wider design, into `CLAUDE.md` or
  `.claude/rules/`. When editing a file that already has comments, delete the ones in the code you
  touch.
- **Format with `rust-formatter`, never `cargo fmt` or `rustfmt`.** It drives nightly rustfmt with
  `StdExternalCrate` import grouping and crate-level import merging, and formats `.toml` too;
  `cargo fmt` formats to different rules. `rust-formatter --check` is the read-only check. Where the
  tool is not installed, say so rather than reaching for `cargo fmt`.
- **Never put the maintainer's name, e-mail address or repository details anywhere** — not in a
  request header or User-Agent, a payload, a URL, a fixture, a default value or generated code.
  Software identifies itself by name and version only (`resonate/<version>`); a contact is sent
  only where the listener typed one into the `contact` setting, and it is empty by default.
- **Errors are typed.** One `thiserror` `Error` per crate, no `String`/`anyhow`/`Box<dyn Error>` as
  a description, opaque foreign errors paired with an operation enum, no `#[non_exhaustive]`, no
  variant nothing constructs, and `size_of::<Error>() <= 128`. `errors.md` has the whole of it.
- **`parking_lot` locks, never `std::sync`'s.** `AHashMap`/`AHashSet` only where keys are our own;
  SipHash wherever they arrive from outside.
- **The crate layering is enforced.** `cargo tree -p <crate>` is the authority, and the CI refuses
  the edges `CLAUDE.md` lists as forbidden.
- **A stored format is migrated, not broken.** A SQLite schema change is a new step appended to
  `MIGRATIONS`; `V1` and existing steps are never edited. Delete-and-rescan is only for data that
  genuinely cannot be carried forward. The same default holds for profiles and the config file.
- **Never run a write pass against a real music library.** `organise --apply`, `tag --apply` and
  `vault --import --apply` work on a copy, never on the listener's own files.

## How work is done here

- **Do the work, not a document about the work.** No plan files, design docs or handoff write-ups
  unless asked for; the review happens in the diff and in git.
- **Anything that edits, commits or pushes runs one agent at a time.** Read-only work — audits,
  search, review — may fan out; work that changes the tree does not. Where several agents are used,
  each one's result is verified by running the project's checks, not by trusting its report, and
  its diff is read for hunks that trace back to nothing asked for.
- **Commit messages** are one sentence-case imperative line saying what changed for the listener
  or the code — *Keep the Discord session over a refused activity* — with a body where the why is
  not obvious.
- **Playlists are finished.** Do not propose playlist work or reopen a playlist item; if playlist
  code has to change, it is because something else broke there.

## Checks

Run these before committing; the CI in `.github/workflows/ci.yml` runs the same set on every push
to `master` and every pull request, in an Arch Linux container because that is what the package
targets.

```
cargo clippy --workspace --all-targets -- -D warnings
cargo test  --workspace --exclude resonate-ui --no-default-features   # the usual inner loop
cargo test  --workspace                                               # everything, gpui included
rust-formatter --check                                                # local only; not in CI
cd fuzz && cargo +nightly fuzz build                                  # the parsers' fuzz targets
```

Formatting is checked locally only, because `rust-formatter` is not something a hosted runner can
install. Tests that need a PipeWire daemon, a session bus, ffmpeg or the network print a skip where
the thing is missing rather than failing; the CI has no daemon and no bus, so those skip there —
except `resonate-pipewire`'s reconnect test, which starts a daemon of its own.

`.cargo/config.toml` builds for `target-cpu=native`, and so does the package, which is built on the
machine it is for. Anything that produces a binary for another machine — the CI — sets `RUSTFLAGS`
back over it.

## Keeping the rules true

The rules are only worth reading while they describe the code, so a change that makes one of them
wrong updates it **in the same commit**:

- A change to shipped behaviour or to a design decision is written into the `.claude/rules/` file
  whose scope covers it, in the same prose register: what the thing is, the rule, and why — the
  failure it prevents, measured where it was measured.
- A new crate, subcommand, config key or invariant is added to `CLAUDE.md`, and the counts it
  states — crates, subcommands, config keys — are moved to match. A new crate that carries rules of
  its own gets a `.claude/rules/<name>.md` with a `paths:` list, and a row in `CLAUDE.md`'s table.
- `docs/TODO.md` holds **open work only**, as `## Category` headings with `- Item` bullets: drop an
  item as it lands, add what the work uncovers, and write what a landed item became into the rules
  rather than leaving it in the roadmap.
- A new command worth running before a commit goes into `CLAUDE.md`'s commands, this file's
  checks and `.github/workflows/ci.yml` together.
- This file names the rules and says how to read them; it does not restate the design. Where
  something here and `CLAUDE.md` or `.claude/rules/` disagree, the rules win, and this file is
  fixed.
