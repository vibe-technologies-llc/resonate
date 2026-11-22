---
name: measure-dsp
description: Measure every DSP stage and the chains the engine plans with dsp-lab, hold them to the committed baseline, and refresh docs/DSP.md and its plots. Use after any change to resonate-dsp, to the engine's planner (pipeline.rs) or to a stage's settings, before pushing it, or whenever DSP.md's figures need checking.
---

# Measure the DSP pipeline

The lab is `dsp-lab/`, a crate outside the workspace (`build.md`, *The DSP lab*). Its default
features need the system libsoxr; `--no-default-features` drops both references and still measures
every stage and chain.

1. **Hold the change to the baseline.** On every core, under any load, in about 25 s:

   ```sh
   cd dsp-lab
   cargo run --release -- measure --against baseline.tsv > /tmp/dsp.md
   ```

   It exits 1 naming each reading worse than the baseline by more than its metric's tolerance
   (`worse: SUITE SUBJECT at RATES: METRIC was … and is …`), and each kept reading no longer
   measured. Name suites to run fewer (`measure eq chain --against baseline.tsv`); `dsp-lab list`
   names them, with their subjects and rates.

2. **A worse reading is a finding.** Read the suite's table in `/tmp/dsp.md` beside the change: a
   chain's THD or SMPTE IMD moving under 10 dB is the engine's clock-seeded dither, anything else
   moved because the code did.

3. **Re-record only what a change means to move.** When the figures are the intended result:

   ```sh
   cargo run --release -- measure --record baseline.tsv > /tmp/dsp.md
   ```

   then `git diff --word-diff baseline.tsv` should show only the readings the change was meant to
   move; say which in the commit message.

4. **Refresh the report.** `cargo run --release -- measure --plots ../docs/dsp > /tmp/dsp.md` (the
   resample suite's plots belong in `../docs/resampling`, which `compare-resamplers` refreshes, so
   delete them from `docs/dsp/` or run the other suites by name). Each suite's tables replace those
   under its heading in `docs/DSP.md`; then check every claim in *What the figures show* against
   them. Look at a plot with `rsvg-convert docs/dsp/eq-curves-48.svg -o /tmp/eq.png` first.

5. **Timing, only on an idle machine.** Ask the listener first. `cargo run --release -- time
   SUITE..` times throughput and construction serially; the stages' ceilings in CI stay the
   `resonate-dsp` bench's (`benches/ceilings.tsv`).

6. **A new stage or chain** is a file in `src/suites/` with its `Metric`s and a line in
   `suites::all`, or a `Scenario` in `src/suites/chain.rs`; give it a test holding a known answer,
   then re-record the baseline.
