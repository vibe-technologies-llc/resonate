---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-mobile/src/output/oboe_render.rs"
  - "crates/resonate-mobile/src/output/remote_io_render.rs"
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

Five RT modules, each headed `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic,
clippy::unreachable, clippy::indexing_slicing)]` (allocation, locks, `assert!`, unguarded division
rest on this discipline alone):

- `resonate-mobile/src/output/oboe_render.rs`: preallocated ring bytes to Oboe stereo float
  frames; short fills zero the rest and publish completion/failure through atomics.
- `resonate-mobile/src/output/remote_io_render.rs` and `remote_io_fill.rs`: the RemoteIO render
  callback hands the system's own interleaved float buffer to `Rendering::fill`, which fills it from
  the source, zeroes a short fill and publishes emptiness through an atomic; it allocates nothing and
  touches no lock. The fill is on the host as well, so its tests run there.
- `resonate-engine/src/ring.rs`: what the graph pulls from; fader and trim scale in place, fade
  state from atomics, each rendered level's start from a preallocated SPSC queue.
- `resonate-pipewire/src/process.rs`: the callback: playback `Cycle`, capture `Hearing` (reads each
  quantum's chunk by its own offset and size, hands bytes to an `AudioSink`).
- `resonate-listen/src/recording.rs`: the recording's `AudioSink`: preallocated `AtomicU32` run of
  f32 bits + atomic count, `Relaxed` stores, one `Release` publish, `Acquire` reads.

Scope the lint list to the module, not the crate; callback work goes in `process.rs`, not the
closure `client.rs` registers (else the lints miss it). They cannot reach libspa.

The callback sets `chunk.size` to exactly what the source filled: bytes past a short read never
reach the graph, nothing needs zeroing.

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
