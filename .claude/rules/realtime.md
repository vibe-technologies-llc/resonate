---
paths:
  - "crates/resonate-engine/**/*.rs"
  - "crates/resonate-pipewire/**/*.rs"
  - "crates/resonate-dsp/**/*.rs"
---

# Realtime discipline

The PipeWire `process` callback is a realtime thread. With `StreamFlags::RT_PROCESS` it runs on
PipeWire's data thread, not the loop thread. Its only job is to move bytes out of the ring and into
the graph buffer. Decode, resample and dither all run on the engine thread — a sinc resampler of
the length the top quality level builds cannot fit in a 256-frame callback budget, so this split is
forced by the quality goal, not merely by hygiene.

The engine thread is therefore where the music is kept up with, and it has no slack to give away:
a 96 kHz 24-bit source reaching a 48 kHz sink took a whole core at `opt-level = 0` and starved the
ring. That is why `[profile.dev]` optimises the workspace's own crates — see `CLAUDE.md`. Anything
measured for cost here is meaningless against an unoptimised build.

Inside the RT module and everything it calls:

- No allocation, and no `Drop` that allocates or frees.
- No lock acquisition, **including `parking_lot`'s**, which may spin then futex-wait. RT-visible
  state must be atomics — encode a `Gain` with `f32::to_bits`.
- No `unwrap`, `expect`, `panic!`, `unreachable!` or `assert!`; `debug_assert!` only.
- No `[i]` indexing or `[a..b]` slicing — use `get`, `get_mut`, `chunks_exact`.
- No division or `%` by a value that is not a `NonZero`.
- Fallible operations return `RtFault`: `Copy`, no heap pointer, no `Drop`, escalated off-thread
  through an SPSC queue with an atomic overflow counter surfaced in-band as `RtFault::Dropped`.
  It holds `Underrun` and `Dropped` and nothing else, because those are the two the ring raises and
  a variant nothing constructs is a variant to take out — so `collect_faults` matches exhaustively
  and has no arm for a fault that cannot arrive.

Three modules are the RT path and each carries that `#![deny(...)]` list at its head:
`resonate-engine/src/ring.rs`, which is what the graph pulls from,
`resonate-pipewire/src/process.rs`, which is the callback itself — the playback `Cycle` and the
capture `Hearing`, which reads each quantum's chunk by its own offset and size and hands the bytes
to an `AudioSink` — and `resonate-listen/src/recording.rs`, the `AudioSink` a recording is kept in:
a preallocated run of `AtomicU32` holding f32 bits and an atomic count, filled with `Relaxed`
stores and published with one `Release`, so taking a quantum neither locks nor allocates and the
listener's thread reads it back with `Acquire`. Scope the list to the module
rather than the crate — the rest of the engine should not have to fight those lints — and put
anything the callback does inside `process.rs` rather than in the closure `client.rs` registers,
which is a closure precisely so that the lints reach its body. What the list cannot reach is
libspa's own code: `Data::data` is an `unwrap` inside the dependency, and no scoping of ours
changes that.

The callback writes what the source filled and sets `chunk.size` to exactly that, so the bytes past
a short read never reach the graph and there is nothing to zero. A memset there is work the graph
was already told to ignore.

`panic = "abort"` is not a substitute. Since Rust 1.81 `extern "C"` functions abort on unwind
anyway, and pipewire-rs's trampolines are `extern "C"`, so a panic aborts in every profile. The
profile converts undefined behaviour into a crash; only this discipline converts a crash into a
glitch. It also means `catch_unwind` is unavailable, so a malformed file must return `Err` rather
than panic — a corrupt track in a scanned library must not be able to abort the player.

## DSP stages

`Processor::process` returns `ProcessCount`, never `Result`. An RT function must have no fallible
path, because the error path is where allocation and formatting live; everything that can fail has
already failed in `prepare`. `prepare` allocates every buffer the stage will ever need.

`Frames` is always at the decoded source rate, and `resonate-dsp` is the only crate permitted to
reinterpret it at the sink rate. `ProcessCount::frames_out` is the one crossing point — the one
value in the workspace expressed at the sink rate — and the engine must never let it escape into
transport state.
