# Rust style

Strict: a violation is a defect.

## Formatting

- **`rust-formatter`, never `cargo fmt` or `rustfmt`.** A wrapper at `~/.cargo/bin/rust-formatter`
  driving nightly rustfmt with settings the plain tools lack; it formats `.toml` as well as `.rs`.
  `cargo fmt` produces *different* output. `rust-formatter --check` is read-only and exits 1 with a
  diff.
- **`rust-toolchain.toml` pins cargo to stable and `rust-formatter.toml` pins the formatter to
  nightly.** The wrapper honours a project's toolchain file, so stable alone would format with a
  rustfmt that has neither `StdExternalCrate` grouping nor crate-level import merging.
- Imports group `std`, then external crates, then `crate`, one `use` per root crate. The formatter
  reorders import lists: re-read a `use` line before string-matching against it.

## Comments

**Never write a comment.** No `//`, `///`, `//!` or `/* */`, in new code and edited code alike. What
a comment would have said goes into the code:

- a value needing explanation gets a name (a `const` called `SIXTEEN_BIT_FLOOR_DB`);
- a constraint gets a type (a newtype, an enum variant, a `NonZero`);
- a step gets extracted into a function whose name describes it;
- a fact about the wider design goes in `CLAUDE.md`, `.claude/rules/` or `docs/`.

This holds for representation invariants, hardware and protocol constraints and footguns too:
`SampleData::S24` holding sign-extended values in the low 24 bits of an `i32` belongs in the type's
name, a constructor enforcing it, or `CLAUDE.md`.

## Collections and sync

- **`ahash` replaces SipHash only where there is no DoS risk.** Keep the standard hasher wherever
  keys are attacker-controlled or come from untrusted input, and wherever an external API demands
  `RandomState`. For keys we produced, use `AHashMap` / `AHashSet`.
- **`BTreeMap` / `BTreeSet`** where ordering matters and hashing does not: sink discovery uses
  `BTreeMap` so the device list is stable between runs.
- **`parking_lot::{Mutex, RwLock}`, never `std::sync`'s.** No poisoning, so `lock()` / `read()` /
  `write()` return the guard with no `unwrap()`. `std::sync::{Arc, atomic}` are used normally.

## Text read from a file

**Text a file of the listener's holds is read through `resonate_core::text`, never assumed to be
UTF-8.** `text::decoded` weighs a byte-order mark first (UTF-8, UTF-16LE, UTF-16BE), then valid
UTF-8, and only then asks `chardetng` which legacy code page the bytes are in, answering the
`TextEncoding` beside the text so a writer can put it back as it was (`text::encoded`, `None` where
the code page has no letter for it). A cue sheet, a playlist sheet, a RIFF `INFO` list (weighed
whole, one guess for every value), an LRC or `.txt` lyric sidecar and an EqualizerAPO profile go
through it. Formats that define their encoding (XSPF, a Lyricsfile, an ID3 frame's encoding byte)
read as defined. It lives in core because the codec, library, lyrics and equaliser all read text
and none may see the others.
