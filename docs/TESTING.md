# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [L · blocked: hardware] No forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

- [S · blocked: hardware] An iPhone following the song's rate (`follow-song-rate`) is not proved on
  a USB DAC: no device is attached here, so neither the route taking `preferredSampleRate` nor the
  gap of the RemoteIO reopen it causes has been heard

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
- [S · 0.1.1] Trashing through the portal (`trash_through_the_portal`) is proved against a hosted
  `xdg-desktop-portal` but not from inside a Flatpak sandbox, where the portal reads the caller's
  app id and the descriptor crosses the sandbox

## Providers and network
- [S · blocked: network] That the hosted hifi service still answers a request carrying neither
  `Origin` nor `Referer` is proved by its gated live test alone (`RESONATE_HIFI_LIVE_TESTS`), which
  nothing here has run

## Testing
- [S · 0.1.1] The drop overlay is driven by a real drag only by hand, on a hosted KWin (the `drive-a-drag`
  skill, which proves the overlay and the copy there): gpui's `ExternalPaths` is `pub(crate)`, so
  `driven.rs` still calls `dragged_over` and `dropped` directly and no test or CI step feeds the
  platform's events; no other compositor has been tried
- [M · blocked: network namespaces] Hole punching is proved over loopback alone; two devices
  behind real NATs, a phone on a mobile network and a Linux desktop, have not been driven against a
  live server. NATs built from user namespaces and nftables need a `veth` pair, which the running
  kernel here has no module for (no `/lib/modules`) and no root to load
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone
