# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Later: DSP
- [M] The resampler's Very high is level with SoXR VHQ at 2:1, not ahead (`docs/RESAMPLING.md`: 0.041 % of a
  core against 0.040 % at 96 to 48 kHz, 0.043 % against 0.041 % at 48 to 96 kHz), and pays for it in latency
  (300 ms, against 2.8 ms): the forward transform runs at the input's length and most of its bins are
  dropped. A halving stage at 4:1 and beyond would close part of it; pruning the transform to the bins kept was
  worked through and saves one stage of fourteen; an own FFT was measured at parity
- [S] The resampler's Low latency costs four times Balanced (0.097 % against 0.023 % at 44.1 to 48 kHz, and it
  rises to 0.120 % on a build without `target-cpu`, where `rustfft` does not): it stays polyphase for its
  latency of 1.07 ms, and the kernel's symmetry is unused
