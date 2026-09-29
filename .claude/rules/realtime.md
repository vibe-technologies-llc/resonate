---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
---

# Realtime discipline

The PipeWire `process` callback is a realtime thread: with `StreamFlags::RT_PROCESS` it runs on
PipeWire's data thread, not the loop thread (playback sets the flag from `StreamRequest::realtime`,
which the engine sets; capture always sets it). Its only job is to move bytes from the ring into
the graph buffer. Decode, resample and dither run on the engine thread — a sinc resampler as long
as the top quality level builds cannot fit a 256-frame callback budget, so the split is forced by
the quality goal, not hygiene.

The engine thread is where the music is kept up with, with no slack to give away: a 96 kHz 24-bit
source reaching a 48 kHz sink took a whole core at `opt-level = 0` and starved the ring, which is
why `[profile.dev]` optimises the workspace (`CLAUDE.md`). A cost measured against an unoptimised
build means nothing.

Inside the RT module and everything it calls:

- No allocation, and no `Drop` that allocates or frees.
- No lock, **including `parking_lot`'s**, which may spin then futex-wait. RT-visible state is
  atomics — encode a `Gain` with `f32::to_bits`.
- No `unwrap`, `expect`, `panic!`, `unreachable!` or `assert!`; `debug_assert!` only.
- No `[i]` indexing or `[a..b]` slicing — use `get`, `get_mut`, `chunks_exact`.
- No division or `%` by a value that is not a `NonZero`.
- Fallible operations return `RtFault`: `Copy`, no heap pointer, no `Drop`, escalated off-thread
  through an SPSC queue whose atomic overflow counter surfaces in-band as `RtFault::Dropped`. It
  holds `Underrun` and `Dropped` alone — the two the ring raises — so `collect_faults` matches
  exhaustively with no arm for a fault that cannot arrive.

Three modules are the RT path, each with that `#![deny(...)]` list at its head:
`resonate-engine/src/ring.rs`, what the graph pulls from — its fader scales samples in place through
`as_chunks_mut`, never indexing, and takes its fade length and target from atomics;
`resonate-pipewire/src/process.rs`, the callback itself — the playback `Cycle` and the capture `Hearing`, which reads each quantum's chunk by
its own offset and size and hands the bytes to an `AudioSink`; and `resonate-listen/src/recording.rs`,
the `AudioSink` a recording is kept in — a preallocated run of `AtomicU32` holding f32 bits and an
atomic count, filled with `Relaxed` stores and published with one `Release`, so a quantum is taken
without locking or allocating and the listener reads it back with `Acquire`. Scope the list to the
module, not the crate — the rest of the engine should not fight those lints — and put whatever the
callback does in `process.rs` rather than the closure `client.rs` registers, which is a closure so
the lints reach its body. The list cannot reach libspa's own code: `Data::data` is an `unwrap`
inside the dependency.

The callback writes what the source filled and sets `chunk.size` to exactly that, so bytes past a
short read never reach the graph and nothing needs zeroing; a memset there is work the graph was
already told to ignore.

`panic = "abort"` is no substitute. Since Rust 1.81 `extern "C"` functions abort on unwind, and
pipewire-rs's trampolines are `extern "C"`, so a panic aborts in every profile. The profile turns
undefined behaviour into a crash; only this discipline turns a crash into a glitch. `catch_unwind`
is therefore unavailable, so a malformed file must return `Err` rather than panic — a corrupt track
in a scanned library must not abort the player.

## DSP stages

`Processor::process` returns `ProcessCount`, never `Result`: an RT function has no fallible path,
the error path being where allocation and formatting live; everything that can fail has failed in
`prepare`, which allocates every buffer the stage will need.

`Frames` is always at the decoded source rate, and only `resonate-dsp` may reinterpret it at the
sink rate. `ProcessCount::frames_out` is the one crossing point — the one value in the workspace at
the sink rate — and the engine never lets it escape into transport state.
