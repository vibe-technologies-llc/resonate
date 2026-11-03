# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Later: DSP
- [M] The resampler's Very high still costs more than SoXR VHQ and the 32-bit recipe at the integer
  ratios (`docs/RESAMPLING.md`: 0.053 % of a core against 0.040 % and 0.046 % at 96 to 48 kHz, 0.054 %
  against 0.042 % and 0.046 % at 48 to 96 kHz, 0.095 % against VHQ's 0.084 % at 192 to 48 kHz): the
  forward transform runs at the input's length and most of its bins are dropped. A halving stage
  ahead of it at 4:1 and beyond, or a transform pruned to the bins kept, would close part of it
- [S] The resampler's Fast and Balanced now cost more than High and Very high (0.068 % and 0.097 %
  against 0.027 % and 0.039 % at 44.1 to 48 kHz, and they rise on a build without `target-cpu`, where
  `rustfft` does not): they stay polyphase for their latency under 1.1 ms; running them on the block
  route, for a listener who does not need that, is unmeasured
