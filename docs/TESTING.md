# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [L · blocked: hardware] No forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

- [S · blocked: hardware] An iPhone following the song's rate (`follow-song-rate`) is not proved on
  a USB DAC: no device is attached here, so neither the route taking `preferredSampleRate` nor the
  gap of the RemoteIO reopen it causes has been heard

- [S · blocked: hardware] Android's unmixed output (`follow-song-rate`) is proved on the host and by
  the emulator's mixed fallback alone: no USB DAC or MMAP-capable phone is attached here, so neither
  `setPreferredMixerAttributes` taking a format, an exclusive open at the song's rate, nor the bytes
  reaching a DAC unaltered has been heard

## iOS
- [S · blocked: iPhone] The RemoteIO `CoreAudio` toolbox (the unsafe `AudioComponentInstanceNew`,
  `AudioUnitSetProperty`, `AudioOutputUnitStart` and sibling calls) and `render`'s `extern "C-unwind"`
  shell stay iOS-only; the unit's sequence, the output thread and the panic catch are proved on the
  host over fakes, so what no test has run is Apple's answer to the calls (that the format is
  accepted, the callback is called with the `Rendering`, a stop disposes cleanly)
- [S · blocked: Xcode's processor] `ios/app-intents.py` is held by a fixture of `Siri.swift`'s const
  values and its own output, never by Apple's: it was compared with `appintentsmetadataprocessor`
  by hand, on another xtool app's fixtures (a plain intent, a `String` parameter, App Shortcuts),
  and `AudioPlaybackIntent` as `AudioStarting` was never set beside Apple's output for it. The
  processor is an arm64 Mach-O

## Identification
- [S · blocked: registered key] AcoustID never reached with a real key; its fixture is written from
  the documentation and `tests/live.rs` has no AcoustID test

## Catalog
- [S · blocked: flatpak] Trashing through the portal is proved from a `bwrap` sandbox built to
  Flatpak's pattern (`.flatpak-info`, a filtering `xdg-dbus-proxy`, `bwrapinfo.json`), not from a
  `flatpak run` of the real manifest: no `flatpak` is installed here, so the document portal and the
  manifest's own permissions have never been in the path

## Providers and network
- [S · blocked: network] That the hosted hifi service still answers a request carrying neither
  `Origin` nor `Referer` is proved by its gated live test alone (`RESONATE_HIFI_LIVE_TESTS`), which
  nothing here has run

## Testing
- [S · 0.2.1] Timing tests flake under a loaded machine: `resonate-server --test hifi`'s
  `an_empty_answer_for_a_track_is_never_kept` failed once in a whole-workspace run (the second
  request's status, likely the one-download-at-a-time 503 before the first frees its slot), and
  `resonate-engine --test transport`'s
  `a_seek_back_under_an_end_of_track_timer_lifts_the_fade_it_had_begun` and
  `equaliser_changes_asked_for_faster_than_the_graph_reaches_them_end_on_the_last` failed once while
  other builds ran; each passes alone and on rerun
- [S · blocked: a second compositor] The drag test has run on KWin alone: no other compositor
  (`sway`, `weston`, `mutter`, `cage`, `labwc`) is installed here, so another's data-device and
  drag-icon behaviour is untried
- [M · blocked: network namespaces] Hole punching is proved over loopback alone; two devices
  behind real NATs, a phone on a mobile network and a Linux desktop, have not been driven against a
  live server. NATs built from user namespaces and nftables need a `veth` pair, which the running
  kernel here has no module for (no `/lib/modules`) and no root to load
- [S · blocked: network namespaces] Nearby meeting over IPv6 is proved over the loopback and the
  datagram path alone; the link local multicast group and a peer dialled by a link local address with
  its scope have not crossed a real interface (nor an IPv6-only network)
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone
- [S · blocked: screen reader] None of the cells, links, lyric lines, plots, bars, cards, the caret and
  selection or the menu a reader follows has been heard over AT-SPI with a screen reader (Orca), only
  built and read back in tests, since gpui's test platform never activates the tree

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone
