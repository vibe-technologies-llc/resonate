---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-mobile/src/output/oboe_render.rs"
  - "crates/resonate-mobile/src/output/oboe_frames.rs"
  - "crates/resonate-mobile/src/output/handover.rs"
  - "crates/resonate-mobile/src/output/remote_io_render.rs"
  - "crates/resonate-mobile/src/output/remote_io_buffers.rs"
  - "crates/resonate-mobile/src/output/remote_io_fill.rs"
  - "crates/resonate-pipewire/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
  - "crates/resonate-listen/src/recording.rs"
  - "crates/resonate-core/src/rt.rs"
---

# Realtime discipline

PipeWire's `process` callback runs on the data thread (`StreamFlags::RT_PROCESS`; playback via
`StreamRequest::realtime`, which the engine sets; capture always). Its only job: ring → graph
buffer. Decode, resample and dither stay on the engine thread, since a top-quality sinc resampler
cannot fit a 256-frame callback budget. Exception: a track that cannot seek decodes on a worker
(`audio.md`). The engine thread has no slack either, hence the optimised `[profile.dev]`; costs
measured on an unoptimised build mean nothing.

In the RT modules and everything they call:

- No allocation; no `Drop` that allocates or frees.
- No lock, **`parking_lot`'s included** (it spins, then futex-waits). RT-visible state is atomics
  (a level is an `f32` stored via `f32::to_bits`).
- No `unwrap`, `expect`, `panic!`, `unreachable!`, `assert!`; `debug_assert!` only.
- No `[i]` / `[a..b]`: use `get`, `get_mut`, `chunks_exact`, `as_chunks_mut`.
- No division or `%` by a non-`NonZero`.
- Fallible ops return `RtFault` (`resonate-core::rt`): `Copy`, no heap pointer, no `Drop`; escalated
  off-thread via an SPSC queue whose atomic overflow counter surfaces as `RtFault::Dropped`. Only
  `Underrun` and `Dropped` exist (what the ring raises), so `collect_faults` matches exhaustively.

Nine RT modules, each headed `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
clippy::unreachable, clippy::indexing_slicing)]` (allocation, locks, `assert!`, unguarded division
rest on this discipline alone):

- `resonate-mobile/src/output/oboe_render.rs` and `oboe_frames.rs`: preallocated ring bytes to
  Oboe frames of whichever shape the stream opened in (`f32`, `i16` or `i32`, mono or stereo:
  `rendered` over a `Frame`, compiled on the host too); short fills zero the rest and publish
  completion/failure through atomics.
- `resonate-mobile/src/output/handover.rs`: the phones' reshape. `Fed` is the callback's source and
  two preallocated one-way `rtrb` queues (one handed in, two spent), as `process::Fed` is
  PipeWire's: `Fed::source` pops a handed source first thing in each callback, swaps it in and
  pushes the old one onto the way back, so the callback never drops a source (it takes nothing while
  the way back is full). `Handing`, on the stream's side, lets the spent go and reports room.
  Oboe's `Rendering` and RemoteIO's `Rendering` both read `Fed::source`.
- `resonate-mobile/src/output/remote_io_render.rs`, `remote_io_buffers.rs` and `remote_io_fill.rs`:
  the RemoteIO render callback (`render`, which catches a panic through `guarded` and aborts) hands the system's own
  interleaved float buffer (`rendered`, reading the buffer list) to `Rendering::fill`, which fills it
  from the source, zeroes a short fill and publishes emptiness through an atomic; it allocates
  nothing and touches no lock. The buffer handling and the fill are on the host as well, so their
  tests run there.
- `resonate-engine/src/ring.rs`: what the graph pulls from; fader and trim scale in place, fade
  state from atomics, each rendered level's start from a preallocated SPSC queue (a gapless turn
  between rows steps to a heard level held in an atomic beside the current one, both movable by
  `hear_across` while the turn waits, and a later `hear_at` outranks the step through an atomic
  turn count). A splice (an
  equaliser change, `audio.md`) arrives as two atomics behind a `Release`d count; the consumer pops
  to its point, commits the stale frames unread and stores the count back, all within the ring's
  own preallocated bytes.
- `resonate-pipewire/src/process.rs`: the callback: playback `Cycle`, capture `Hearing` (reads each
  quantum's chunk by its own offset and size, hands bytes to an `AudioSink`). A reshape's new
  `Feed` arrives through a preallocated `rtrb` queue and the old one leaves through another, so
  `Fed::run` swaps sources without dropping one (it takes nothing while the way back is full).
- `resonate-listen/src/recording.rs`: the recording's `AudioSink`: preallocated `AtomicU32` run of
  f32 bits + atomic count, `Relaxed` stores, one `Release` publish, `Acquire` reads.

Scope the lint list to the module, not the crate; callback work goes in `process.rs`, not the
closure `client.rs` registers (else the lints miss it). They cannot reach libspa. The callback sets
`chunk.size` to exactly what the source filled, so bytes past a short read never reach the graph.

**A ring laid out for one format never reaches buffers of another.** `param_changed` stores what the
graph settled on against what was asked (`Negotiated::taken_against`) in one `AtomicU32`
(`process::Taking`, so word and layout cannot tear): `Taken::AsAsked(word)` hands the ring over as
ever; `Taken::Otherwise { frame }` (the engine's rebuild is owed, up to a poll tick away) writes
zeros for the quantum at the graph's own frame and stride, never reading the ring. Silence, not a
map: every `SampleFormat` is signed or float, so zero is silence in all of them; a remix here would
need the matrix and a scratch buffer on the RT thread, for audio the rebuild discards anyway
(`a_ring_laid_out_for_another_layout_is_never_written_into_the_graphs_buffer`).

`panic = "abort"` is no substitute: pipewire-rs trampolines are `extern "C"`, so a panic aborts in
every profile; only this discipline turns a crash into a glitch. No `catch_unwind`: a malformed
file returns `Err`, never panics.

## DSP stages

`Processor::process` returns `ProcessCount`, never `Result` (the error path is where allocation
lives). Whatever can fail has failed in `prepare`, which allocates every buffer the stage needs.

`Frames` is always at the decoded source rate; only `resonate-dsp` may reinterpret it at the sink
rate. `ProcessCount::frames_out` is the one crossing point and the only workspace value at the sink
rate; the engine never lets it escape into transport state.
