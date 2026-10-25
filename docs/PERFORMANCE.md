# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
