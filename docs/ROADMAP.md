# Roadmap

Open work only. The work is split by kind into the files below; each keeps the `## Category` headings
of `- Item` bullets.

| File | Holds |
|---|---|
| [BUGS.md](BUGS.md) | defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims |
| [FEATURES.md](FEATURES.md) | gaps: what is not built, a limit by design, an unsupported format, a behaviour the listener cannot reach |
| [PERFORMANCE.md](PERFORMANCE.md) | cost: work done more often, in more memory or on a thread it should not be on |
| [TESTING.md](TESTING.md) | proof and plumbing: untested paths, fuzz targets, hardware, dependencies, packaging |

## Reading an entry

`- [S · 0.2.0] text` or `- [M · blocked: gpui] text`: the bracket is the prefix, complexity first,
then the release the entry is targeted at, then the blocker when there is one. An entry with no
target is not scheduled: blocked entries and `Later:` categories carry none, and gain one when they
move.

| Complexity | Meaning |
|---|---|
| `S` | one function or file, a test beside it |
| `M` | several modules of one crate, or a design choice inside it |
| `L` | several crates, a migration, a new subsystem or open-ended work |

A blocker names the outside thing the entry waits on (hardware, an upstream crate, a service, a
format). Complexity is what the work costs once it moves.

## Releases

Versions are `MAJOR.MINOR.PATCH` with `-rcN` for candidates. A patch release carries fixes only; a
minor or major release carries features. Each `MAJOR.MINOR` line has a `release/MAJOR.MINOR` branch
cut from `master`, and each release is a `vMAJOR.MINOR.PATCH[-rcN]` tag with a GitHub release
(`.claude/rules/release.md`).

| Target | Kind | Draws from |
|---|---|---|
| `0.1.0` | first release | the first cut of `release/0.1` |
| `0.1.1` | patch | [BUGS.md](BUGS.md), [PERFORMANCE.md](PERFORMANCE.md), [TESTING.md](TESTING.md) |
| `0.2.0` | minor | [FEATURES.md](FEATURES.md) |

A fix found on a released line lands on `master` and is cherry-picked to its `release/` branch. A
target moves when the work does; the table grows a row when a release is planned.

## Ordering

- Categories run most to least important; entries likewise (prerequisite before dependent, easier
  first among equals). Blocked entries sit last in their category and are not worked until the
  blocker moves.
- `## Next` (work chosen for the coming release) is first when it exists; it lives here, in this
  file, in the same entry format, every entry carrying a target. `## Later: Category` headings, which no listener waits on, come below the rest
  of their file.
- An entry lives in exactly one file. A category may appear in several files.

## Keeping it true

Drop an entry as it lands, add what the work uncovers to the file for its kind, and write what a
landed entry became into `.claude/rules/`.
