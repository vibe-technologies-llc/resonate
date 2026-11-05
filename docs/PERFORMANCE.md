# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Later: Analysis
- [S] A study's two true-peak meters read one signal: `Metering`'s whole-song maximum is `Metered`'s
  tiles' (each is the exact maximum over every read it could not rule out), so the first could go
- [S] `TruePeakMeter` multiplies out the phase at whole samples, whose weights are one tap of 1 and the rest 0; it is
  a quarter of a study's meter and of the guard's, and the answer is the sample itself
- [S] `Mdct::coefficients` turns and writes every line while a grid reads only its band's (AAC's 4..16 kHz is
  half of them); it was about a tenth of the CPU on a minute of FLAC
- [M] `workers_for` stayed at half the cores though each study now spreads over the crew: four
  workers keep the same throughput as eight at three quarters of the memory (442 MB against 582 MB
  on sixteen songs), but a worker also waits on recognition, which was not measured
- [M] `Bank`'s rows are read one chunk at a time on its own stage, and its forward pass cannot be parted, so it and
  `Metered`, 1 to 1.5 s each over 5 minutes at 44.1 kHz, are what a pass's wall time now waits for,
  96 kHz most of all (4.5 s for 5 minutes)
- [M] `peak_through`, `print` and `excerpt` still walk on one thread: *Fit the preamp to the music* over songs no
  study has measured runs its chain in line

## Later: DSP
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.041 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. A halving stage at 4:1 and beyond would close part of it; pruning the transform to the bins kept was
  worked through and saves one stage of fourteen; an own FFT was measured at parity
- [S] The resampler's Low latency costs four times Balanced (0.097 % against 0.023 % at 44.1 to 48 kHz, and it
  rises to 0.120 % on a build without `target-cpu`, where `rustfft` does not): it stays polyphase for its
  latency of 1.07 ms, and the kernel's symmetry is unused
