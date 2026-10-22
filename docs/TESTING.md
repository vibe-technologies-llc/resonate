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
- [S] RemoteIO's `Unit` open path and `render`'s pointer handling stay iOS-only and untested on the
  host; `Routing`, `Drain` and `fill` are proved
- [S] `ios/app-intents.py` was compared with Apple's processor by hand, on another xtool app's
  fixtures (a plain intent, a `String` parameter, App Shortcuts), and nothing compares it again;
  `AudioPlaybackIntent` as `AudioStarting` was never set beside Apple's output for it

## Identification
- [S · blocked: registered key] AcoustID never reached with a real key; its fixture is written from
  the documentation and `tests/live.rs` has no AcoustID test

## Catalog
- [S] Trashing through the portal (`trash_through_the_portal`) is unproved against a live portal
  under Flatpak; the agent shell's sandbox answered *refused* for a host file

## Providers and network
- [S · blocked: network] That the hosted hifi service still answers a request carrying neither
  `Origin` nor `Referer` is proved by its gated live test alone (`RESONATE_HIFI_LIVE_TESTS`), which
  nothing here has run

## Testing
- [M] Hole punching is proved over loopback alone; two devices behind real NATs, a phone on a
  mobile network and a Linux desktop, have not been driven against a live server
- [M] The drop overlay never dragged onto on a real compositor from this tree: gpui's
  `ExternalPaths` is `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the
  platform's drag events are unproved here
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone
