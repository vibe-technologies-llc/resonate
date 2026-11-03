# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)

## Later: DSP
- [L] The resampler's High and Very high cost 3 to 22 times SoXR HQ's and VHQ's CPU at a deeper
  stopband (`docs/RESAMPLING.md`): every output frame is a dot product over the whole kernel,
  stretched by the ratio going down, so Very high at 192 to 48 kHz takes 1.87 % of a core against
  SoXR VHQ's 0.085 %, and intermediate phase up to 3 %; halving stages ahead of the polyphase one
  at integer factors, or partitioned FFT convolution for High and Very high, would close most of
  it (and lower `ceilings.tsv` with it)
