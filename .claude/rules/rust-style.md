# Rust style

Strict. Treat a violation as a defect.

## Formatting

- **`rust-formatter`, never `cargo fmt` or `rustfmt`.** A wrapper at `~/.cargo/bin/rust-formatter`
  driving nightly rustfmt with settings the plain tools do not carry; it formats `.toml` as well as
  `.rs`. `cargo fmt` produces *different* output and must not be run here.
- `rust-formatter --check` is read-only and exits 1 with a diff.
- **`rust-toolchain.toml` pins cargo to stable and `rust-formatter.toml` pins the formatter to
  nightly.** The wrapper honours a project's toolchain file, so pinning stable alone would quietly
  format with stable rustfmt, which carries neither `StdExternalCrate` grouping nor crate-level
  import merging. The two files together are what keep `cargo` on the toolchain the package is built
  with and `rust-formatter` on the one this style needs.
- Imports group `std` → external crates → `crate`, one `use` per root crate, matching the
  formatter's `StdExternalCrate` grouping with crate-level merging.
- The formatter reorders import lists. Re-read a `use` line before string-matching against it.

## Comments

**Never write a comment.** No `//`, no `///`, no `//!`, no `/* */`. There is no case that earns
one, and no judgment call to make: a comment in a diff is a defect, in new code and in code being
edited alike.

What a comment would have said goes into the code instead:

- A value that needs explaining gets a name — a `const` called `SIXTEEN_BIT_FLOOR_DB` or
  `LIPSHITZ_1992_E_WEIGHTED`.
- A constraint that needs stating gets a type — a newtype, an enum variant, a `NonZero`.
- A step that needs describing gets extracted into a function whose name describes it.
- A fact about the wider design goes in `CLAUDE.md`, `.claude/rules/` or `docs/`, where it is
  read once rather than skipped a thousand times.

This holds for representation invariants, hardware and protocol constraints and footguns too.
`SampleData::S24` holding sign-extended values in the low 24 bits of an `i32` is a fact about the
type: it belongs in the type's name, in a constructor that enforces it, or in `CLAUDE.md`. It does
not belong beside the field.

## Collections and sync

- **`ahash` replaces std's SipHash only where there is no DoS risk.** The decision is about the
  hasher, not the container: SipHash is there to resist hash flooding, so keep it (the
  `std::collections::HashMap` default) wherever keys are attacker-controlled or reach us from
  untrusted input, and wherever an external API demands `RandomState`. Everywhere else — keys we
  produced ourselves — `AHashMap` / `AHashSet` are the aliases to reach for.
- **`BTreeMap` / `BTreeSet` are fine** where ordering matters and hashing is beside the point. Sink
  discovery uses `BTreeMap` so the enumerated device list is stable between runs.
- **`parking_lot::{Mutex, RwLock}`, never `std::sync`'s.** No poisoning, so `lock()` / `read()` /
  `write()` return the guard directly with no `unwrap()`. `std::sync::{Arc, atomic}` are unaffected
  and used normally.
