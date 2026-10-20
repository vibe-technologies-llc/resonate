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
- [M] More pure Swift is unproved beside the list above: polling (`NowPlaying.Written.expected`), sync (`syncStanding`, `SyncTransfers.keyed`, `syncActivity`'s fraction,
  `answerSyncAsked`, `landable`, `SyncRoute.init`), lists and search (`lit`, `LetterRun.runs`,
  `SearchModel`'s generations, `QueueParts`, `ListeningBar.grouped`, `PlaylistEntry.of`,
  `AlbumDisc.of`, `FilterChip`, `SongKeys.matching`), `OnlineAnswers.keep`, the tag editor's mixed fields, `AccentColours.ink` and `FieldUniforms`'
  192-byte stride against the shader's `Field`
- [M] `NowPlaying`'s interruption, route and release decisions, `BackgroundHold`'s hold, expire and
  launch machine and `landReceived`'s back-off are unproved; each calls `AVAudioSession`, `BGTaskScheduler` or
  `SecItem*` directly, so none can be driven without a seam
- [M] Pure Swift in the views is unproved too: `LyricClock`, `EqualiserScale` and
  `EqualiserResponse`'s coalescing, `LyricWords.cut` across surrogate pairs, `LyricTurn`/`LyricLeg`/`LyricGlide` against
  the desktop's lyrics tests, `OnlineFound.landing`/`awaited`, `FieldBlend.show` mid-fade,
  `SettingsView.matches`, and the playlist and queue `onMove` destination arithmetic, which differ

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
- [L] No frame is pulled through a stream in S16, S32, packed or padded S24, 5.1 or 7.1, or with
  `NO_CONVERT`, nor recorded from a microphone node: the desktop's sink is stereo F32 and a hosted
  daemon links nothing (no session manager; a hosted WirePlumber would reach the real cards)
- [M] The drop overlay never dragged onto on a real compositor from this tree: gpui's
  `ExternalPaths` is `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the
  platform's drag events are unproved here
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone
- [M · blocked: gpui] Gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc; `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
