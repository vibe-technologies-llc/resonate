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

`- [S] text` or `- [M · blocked: gpui] text`: the bracket is the prefix, complexity first, blocker
after it when there is one.

| Complexity | Meaning |
|---|---|
| `S` | one function or file, a test beside it |
| `M` | several modules of one crate, or a design choice inside it |
| `L` | several crates, a migration, a new subsystem or open-ended work |

A blocker names the outside thing the entry waits on (hardware, an upstream crate, a service, a
format). Complexity is what the work costs once it moves.

## Ordering

- Categories run most to least important; entries likewise (prerequisite before dependent, easier
  first among equals). Blocked entries sit last in their category and are not worked until the
  blocker moves.
- `## Next` (work chosen to come next) is first when it exists; it lives here, in this file, in the
  same entry format. `## Later: Category` headings, which no listener waits on, come below the rest
  of their file.
- An entry lives in exactly one file. A category may appear in several files.

## Keeping it true

Drop an entry as it lands, add what the work uncovers to the file for its kind, and write what a
landed entry became into `.claude/rules/`.
