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
  between rows steps to a heard level held in a fixed array of `TURNS_HELD` atomics, one slot a
  turn by its `Turned` count, so up to `TURNS_WAITING_AT_MOST` turns wait at once each at its own
  level; every slot and the current level are movable by `hear_across` while turns wait, and a
  later `hear_at` outranks every step through an atomic turn count). A splice (an
  equaliser change, `audio.md`) arrives as two atomics behind a `Release`d count; the consumer pops
  to its point, commits the stale frames unread and stores the count back, all within the ring's
  own preallocated bytes.
- `resonate-pipewire/src/process.rs`: the callback: playback `Cycle`, capture `Hearing` (reads each
  quantum's chunk by its own offset and size, hands bytes to an `AudioSink`). A reshape's new
  `Feed` arrives through a preallocated `rtrb` queue and the old one leaves through another, so
  `Fed::run` swaps sources without dropping one (it takes nothing while the way back is full).
- `resonate-listen/src/recording.rs`: the recording's `AudioSink`: preallocated `AtomicU32` run of
  f32 bits + atomic count, `Relaxed` stores, one `Release` publish, `Acquire` reads.

**Every callback stamps what it fills, from the device's own timestamp.**
`AudioSource::fill_stamped(dst, Stamp)` (`resonate-audio`; `fill` is the default for a source that reads none) hands the source a `Moment` and the stream
frames between it and the fill's first being heard (`Moment`: nanoseconds on one process-wide monotonic clock, `Moment::start_the_clock` called by
`ring` so the clock's first read is never on the RT thread). **A callback runs some way into its cycle, so
the moment it ran is not when its cycle began**: each backend hands `StreamClock::stamp_by` a
`DeviceTime` (its own clock read now, the moment on that clock its reading names, the frames between
that moment and the fill's first being heard), which `DeviceTime::stamp` lands on `Moment`'s clock
by the distance between the two readings, so no clock is converted into another; a backend with no
reading falls back to `StreamClock::stamp` (now, plus the latency the clock last read). PipeWire's
`Cycle::run` reads `pw_time.now` (the driver's cycle start, `CLOCK_MONOTONIC`) against
`clock_gettime(Monotonic)` and the stream frames `note_latency` just published; Oboe's callback reads
`getTimestamp(CLOCK_MONOTONIC)` (a frame the device presented and when) and `getFramesWritten`,
landing the fill's first frame as far past that moment as it lies past that frame
(`oboe_frames::heard_by_the_device`), and `rendered` stamps each chunk with the frames of the chunks
before it added; RemoteIO's `render` reads the buffer's `mHostTime` against `mach_absolute_time`
and `Rendering::fill` stamps it with the route's latency less the buffer's frames. An Oboe stream
whose API gives no timestamp (OpenSL ES) or a RemoteIO stamp without a valid host time falls back. The ring consumer
(`filled_from`) writes a `RingStamp` (the moment the stamp was `taken`, the ring frame it took first, the moment that frame is heard,
the moment the next fill's first frame is, the frames pulled, whether music flowed from the fill's
start) into a seqlock of atomics (`StampSlot`, one writer, the reader retrying a torn read) under
its discard epoch; `RingProducer::stamp` answers only the current epoch's. A ring held in step
(`Entering::InStep`) hushes whole fills without consuming, drops the frames the engine asks
(`drop_ahead`, counted asked against done, cancelled by a discard) as they arrive, and sounds from
the frame its release moment lands on, rounding a moment's nanoseconds by at most
`A_MOMENT_ROUNDED_BY_FRAMES`; an unstamped fill releases it at once. No lock, no allocation:
counters and the slot are preallocated atomics.

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
