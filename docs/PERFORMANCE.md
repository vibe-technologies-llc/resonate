# Performance

Cost: work done more often, in more memory or on a thread it should not be on. Format and ordering
are in [ROADMAP.md](ROADMAP.md).

## Performance and scale
- [M · 0.1.1] A song pressed in a search is not asked of a provider until its release has been read
  from MusicBrainz (one paced request, one to three seconds, more under a 503), though the `Found`
  already carries the title, credit, length and codes a provider searches with: the poll could start
  from them while the release lands (`LibraryModel::want_found`, `Library::want_found_uncovered`)
- [L · blocked: gpui] Every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream (zed
  #62455)
