---
name: compare-resamplers
description: Rerun the measurement of Resonate's resampler against libsoxr, try a changed kernel or window against it, and refresh docs/RESAMPLING.md and its plots. Use after any change to resonate-dsp's resample.rs, spectral.rs or phase.rs, a new Quality level or SincParams, or whenever the report's figures need checking.
---

# Compare the resampler against SoXR

The harness is `soxr-comparison/`, a crate outside the workspace (`build.md`). It needs the
system libsoxr and its headers (`pkg-config --modversion soxr` answers).

1. **Quality, under any load.** On every core, in about 20 s:

   ```sh
   cd soxr-comparison
   cargo run --release -- --no-timing --plots ../docs/resampling > /tmp/quality.md
   ```

   It writes `response-<pair>.svg` and `passband-<pair>.svg` for each rate pair, and the frequency
   and time domain tables on stdout.

2. **Timing, only on an idle machine.** Ask the listener first: another build, agent or player
   running at the same time skews every figure. It runs serially, about a minute:

   ```sh
   cargo run --release -- --timing-only > /tmp/cost.md
   ```

3. **Refresh the report.** In `docs/RESAMPLING.md`, the quality output replaces the tables under
   `## Every measurement` and the timing output those under `### Throughput and construction`.
   Then check the prose against the new figures: each claim there names a number from these
   tables, and the summary tables under `## Results` repeat some of them.

4. **A changed kernel starts from `--candidates`.** `python3 design_window.py NAME WIDTH_HZ REACH RING_TAPS TERMS`
   prints a window (`docs/RESAMPLING.md`, *Reproducing*, has the three held now); write its numbers to a file, one
   line per try in a candidates file (`very-high 1200 32.4845 0 3300 16000 window.txt`, `soxr-steep` for a recipe),
   and `cargo run --release -- --candidates FILE` tables them side by side.

5. **Look at the plots** with `rsvg-convert docs/resampling/response-44.1-to-48.svg -o /tmp/r.png`
   before committing: a level clamped at the plot's floor or overlapping labels mean the frame needs
   widening in `plot.rs`.
