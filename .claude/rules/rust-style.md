# Rust style

Strict: a violation is a defect.

## Formatting

- **`rust-formatter`, never `cargo fmt`/`rustfmt`** (`~/.cargo/bin/rust-formatter`: nightly rustfmt
  plus extra settings, also formats `.toml`; `cargo fmt` output differs). `--check`: read-only, exit
  1 with a diff.
- `rust-toolchain.toml`: cargo on stable; `rust-formatter.toml`: formatter on nightly (the wrapper
  honours toolchain files; stable rustfmt lacks `StdExternalCrate` grouping and import merging).
- Imports: `std`, external, `crate`; one `use` per root crate. The formatter reorders: re-read a
  `use` line before string-matching.

## Comments

**Never write a comment** (`//`, `///`, `//!`, `/* */`), new or edited code, invariants, hardware or
protocol constraints and footguns included. Instead:

- name the value (`const SIXTEEN_BIT_FLOOR_DB`);
- type the constraint (newtype, variant, `NonZero`); `SampleData::S24`'s sign-extended low-24-bit
  `i32` belongs in a type name, enforcing constructor, or `CLAUDE.md`;
- extract the step into a named function;
- design facts: `CLAUDE.md`, `.claude/rules/`, `docs/`.

## Collections and sync

- **`ahash` only without DoS risk**: standard hasher for attacker-controlled/untrusted keys and
  where an API demands `RandomState`; `AHashMap`/`AHashSet` for keys we produced.
- **`BTreeMap`/`BTreeSet`** where order matters, hashing does not (sink discovery: stable device
  list).
- **`parking_lot::{Mutex, RwLock}`, never `std::sync`'s** (no poisoning: `lock()`/`read()`/`write()`
  return the guard, no `unwrap()`); `std::sync::{Arc, atomic}` as usual.

## Text read from a file

**A listener's file text goes through `resonate_core::text`, never assumed UTF-8.** `text::decoded`:
UTF-16LE/BE byte-order mark, else valid UTF-8 (UTF-8 mark stripped), else `chardetng`'s legacy
code-page guess; returns the `TextEncoding` so a writer restores it (`text::encoded`, `None` where
the code page lacks a letter). Users: cue sheet, playlist sheet, RIFF `INFO` list (weighed whole,
one guess for all values), LRC/`.txt` lyric sidecar, EqualizerAPO profile, DSDIFF edited-master
text. Self-defining formats (XSPF, Lyricsfile, ID3 frame encoding byte) read as defined. In core
because codec, library, lyrics, equaliser all read text and none may see the others.

## A name made from a listener's name

**A staging, journal, copy or candidate name built around a file's name goes through
`resonate_core::naming::named_within`**, which cuts the name (on a letter) so the whole stays
within `NAME_BYTES_AT_MOST` (255): a name near the limit gains `.`, a stamp and a suffix and would
otherwise be refused by the filesystem, failing a write the file itself allows. A reader of such
names (`journal::kept_by`, `writing::staged_by`) takes a stem that is a prefix of the track's as
naming it only where the name sits at the limit; a cut whole copy whose prefix names more than one
file is left where it is (`Mended::Unplaced`).
