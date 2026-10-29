# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [S · 0.3.0] *Fit the preamp*'s solved bound costs about a millisecond a song where the partition cost
  microseconds: `Loudest::weigh` solves no moment that cannot raise the loudest yet, but a catalog
  fold of ten thousand songs as loud as one another can still take seconds, more on a phone
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
