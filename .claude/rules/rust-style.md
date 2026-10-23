# Rust style

Strict: a violation is a defect.

## Formatting

- **`rust-formatter`, never `cargo fmt`/`rustfmt`**: it drives nightly rustfmt with extra settings (`cargo fmt` output differs). `--check` is read-only, exit 1 with a diff.
- `rust-toolchain.toml` keeps cargo on stable; `rust-formatter.toml` puts the formatter on nightly, since stable rustfmt lacks `StdExternalCrate` grouping and import merging.
- Imports: `std`, external, `crate`; one `use` per root crate. The formatter reorders, so re-read a `use` line before string-matching.

## Comments

**Never write a comment** (`//`, `///`, `//!`, `/* */`), invariants, hardware or protocol constraints and footguns included. Instead:

- name the value (`const SIXTEEN_BIT_FLOOR_DB`);
- type the constraint (newtype, variant, `NonZero`), as `SampleData::S24`'s sign-extended low-24-bit `i32` belongs in a type name or enforcing constructor;
- extract the step into a named function;
- put design facts in `CLAUDE.md`, `.claude/rules/`, `docs/`.

## Collections and sync

- **`ahash` only without DoS risk**: standard hasher for untrusted keys and where an API demands `RandomState`; `AHashMap`/`AHashSet` for keys we produced.
- **`BTreeMap`/`BTreeSet`** where order matters and hashing does not (sink discovery: stable device list).
- **`parking_lot::{Mutex, RwLock}`, never `std::sync`'s** (no poisoning, no `unwrap()`); `std::sync::{Arc, atomic}` as usual.

## Text read from a file

**A listener's file text goes through `resonate_core::text`, never assumed UTF-8.** `text::decoded` takes a UTF-16 byte-order mark, else valid UTF-8, else `chardetng`'s code-page guess, and returns the `TextEncoding` so a writer restores it (`text::encoded`, `None` where the code page lacks a letter). Users: cue sheet, playlist sheet, RIFF `INFO` list (one guess for all values), LRC/`.txt` lyric sidecar, EqualizerAPO profile, DSDIFF edited-master text. Self-defining formats (XSPF, Lyricsfile, ID3 frame encoding byte) read as defined. It lives in core because codec, library, lyrics and equaliser all read text and none may see the others.

## A name made from a listener's name

**A staging, journal, copy or candidate name built around a file's name goes through `resonate_core::naming::named_within`**, which cuts the name on a letter to keep the whole within `NAME_BYTES_AT_MOST` (255); a near-limit name plus a stamp and suffix would be refused by the filesystem, failing a write the file itself allows. Readers of such names (`journal::kept_by`, `writing::staged_by`) take a stem that is a prefix of the track's as naming it only where the name sits at the limit; a cut whole copy whose prefix names more than one file stays put (`Mended::Unplaced`).

## A file another run may sweep

**A staging file, journal or whole copy another run may take away is held under `resonate_core::writer::Held` for its whole life, and a sweeper asks `writer::writer_of`.** A held `flock` means a living writer in any pid namespace; a `/proc` pid cannot, because Flatpak's sandbox has pids of its own. A file nobody holds is gone once untouched for `UNHELD_AND_UNTOUCHED_FOR` (10 s, covering a writer between creating and locking), sooner where its stamped pid has no `/proc` entry; where the filesystem refuses locks the stamp alone decides. `Held::made` creates a fresh name; `Held::taken_over` reuses a fixed one, refusing `WouldBlock` while a living writer holds it. The pid stays in a name only to keep names apart and for that fallback. Users: the tag writer's staged copies, undo journals and whole copies, beside the track or in the spool (`resonate-codec`), the vault's staging, organise's and a delivery's staged files, a drop-in's `.resonate-part` and the analysis cache's `.staged` writes (`a_held_file_is_living_whatever_process_its_stamp_names`, `an_unheld_file_left_alone_is_gone_though_its_stamp_names_a_running_process`).
