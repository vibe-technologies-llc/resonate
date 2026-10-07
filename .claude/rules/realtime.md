---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
  - "crates/resonate-listen/src/recording.rs"
  - "crates/resonate-core/src/rt.rs"
---

# Realtime discipline

PipeWire's `process` callback is a realtime thread: `StreamFlags::RT_PROCESS` puts it on the data
thread, not the loop thread (playback: from `StreamRequest::realtime`, which the engine sets;
capture: always). Its only job: ring → graph buffer. Decode, resample, dither stay on the engine
thread (a sinc resampler as long as the top quality level builds cannot fit a 256-frame callback
budget: forced by the quality goal, not hygiene). Exception: a track that cannot seek decodes a
block at a time on a worker, its reads possibly waiting on a pipe forever (`audio.md`). The engine
thread has no slack either (it must keep up with the music), hence the optimised `[profile.dev]`
(`CLAUDE.md`); costs measured on an unoptimised build mean nothing.

In the RT modules and everything they call:

- No allocation; no `Drop` that allocates or frees.
- No lock, **`parking_lot`'s included** (spins, then futex-waits). RT-visible state = atomics (a
  level is an `f32` stored via `f32::to_bits`).
- No `unwrap`, `expect`, `panic!`, `unreachable!`, `assert!`; `debug_assert!` only.
- No `[i]` / `[a..b]`: use `get`, `get_mut`, `chunks_exact`, `as_chunks_mut`.
- No division or `%` by a non-`NonZero`.
- Fallible ops return `RtFault` (`resonate-core::rt`): `Copy`, no heap pointer, no `Drop`; escalated
  off-thread via an SPSC queue whose atomic overflow counter surfaces in-band as
  `RtFault::Dropped`. Only `Underrun` and `Dropped` (what the ring raises), so `collect_faults`
  matches exhaustively, no arm for a fault that cannot arrive.

Three RT modules, each headed `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
clippy::unreachable, clippy::indexing_slicing)]` (allocation, locks, `assert!`, unguarded division
rest on this discipline alone):

- `resonate-engine/src/ring.rs`: what the graph pulls from. Fader and trim scale samples in place;
  fade length, fade target, level to be heard come from atomics, each rendered level's start from a
  preallocated SPSC queue.
- `resonate-pipewire/src/process.rs`: the callback: playback `Cycle`, capture `Hearing` (reads each
  quantum's chunk by its own offset and size, hands bytes to an `AudioSink`).
- `resonate-listen/src/recording.rs`: the recording's `AudioSink`: preallocated `AtomicU32` run of
  f32 bits + atomic count, `Relaxed` stores, one `Release` publish, `Acquire` reads.

Scope the lint list to the module, not the crate; callback work goes in `process.rs`, not the
closure `client.rs` registers (else the lints miss it). They cannot reach libspa (`Data::data`
`unwrap`s inside the dependency).

The callback writes what the source filled and sets `chunk.size` to exactly that: bytes past a short
read never reach the graph, nothing needs zeroing.

`panic = "abort"` is no substitute: pipewire-rs trampolines are `extern "C"`, which abort on unwind,
so a panic aborts in every profile; only this discipline turns a crash into a glitch. No
`catch_unwind`, so a malformed file returns `Err`, never panics (a corrupt track in a scanned
library must not abort the player).

## DSP stages

`Processor::process` returns `ProcessCount`, never `Result` (the error path is where allocation and
formatting live). Whatever can fail has failed in `prepare`, which allocates every buffer the stage
needs.

`Frames` is always at the decoded source rate; only `resonate-dsp` may reinterpret it at the sink
rate. `ProcessCount::frames_out` is the one crossing point and the only workspace value at the sink
rate; the engine never lets it escape into transport state.
