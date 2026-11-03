# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Later: DSP
- [M] The resampler's High and Very high still cost 2.5 to 4 times SoXR HQ's and VHQ's CPU at a
  deeper stopband (`docs/RESAMPLING.md`: Very high 0.165 % of a core against VHQ's 0.054 % at 44.1
  to 48 kHz, 0.332 % against 0.084 % at 192 to 48 kHz): stage two's interpolator spends 29 to 37
  taps a frame through the general polyphase loop, about a third of the cost, and stage one's
  partitions stay at 256 and 512 samples to hold the latency; a loop of its own for the short
  interpolator, or halving stages ahead of stage one at 4:1 and beyond, would close part of it
