# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [S] `transport.rs`'s `a_steady_level_playing` drops its temporary tree as it returns, so a test
  that makes the engine reopen the file (a repeat wrap) fails intermittently
- [L · blocked: hardware] No forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

- [S · blocked: hardware] An iPhone following the song's rate (`follow-song-rate`) is not proved on
  a USB DAC: no device is attached here, so neither the route taking `preferredSampleRate` nor the
  gap of the RemoteIO reopen it causes has been heard

## iOS
- [M] `ios/Package.swift` has no test target: `Folders.uri` and `resolved` (the `..` check),
  `Settling` and `Steered`, `AccountStore` and `PlaybackPreferencesStore` migrations, `Erasure`
  against the suites in use (it misses `appearance`), `ListenNext.after`, `ArtworkBucket.fitting`,
  `DecodeSlots`, `LyricClock`, `DownloadJob.follow` and the journal are unproved
- [S] `remote_io.rs` and `remote_io_render.rs` are `cfg(target_os = "ios")` alone: no host test of
  `render` (a short fill, zero length, the `empty` flag), `Routing::route_changed` or the drain
  wait, though `Routing` is atomics and could leave the cfg
- [S] `build-native.py` reads the accents and `LIFTED_TOWARD_WHITE` out of Rust sources by regex and
  recolours the SVG with a silent `replace(..., 1)`, and nothing asserts that the twenty-one PNGs
  match `xtool.yml` and `Info.plist`
- [S] The build is checked for Swift warnings by hand only (`ios.md`'s *Checks*); no check reads
  `xtool dev build`'s output
- [S] `explained` digs the sentence out of the generated `ResonateNative.Error` with `Mirror` (one
  level for a `String` payload, two for `message:`), and nothing holds the shape: a UniFFI upgrade
  changing the flat error falls back to `String(reflecting:)`, and every banner shows a debug print
- [M] More pure Swift is unproved beside the list above: `Folders.file` (the root-prefix guard,
  existing-destination refusal, the `.lrc` move, `prune`, the *Put files back* list), polling
  (`QueueListed.isDue`, `pollInterval`, `Library.steady`, `NowPlaying.Written.expected`,
  `Tally.summary`), sync (`syncStanding`, `SyncTransfers.keyed`, `syncActivity`'s fraction,
  `answerSyncAsked`, `landable`, `SyncRoute.init`), lists and search (`lit`, `LetterRun.runs`,
  `SearchModel`'s generations, `QueueParts`, `ListeningBar.grouped`, `PlaylistEntry.of`,
  `AlbumDisc.of`, `FilterChip`, `SongKeys.matching`), `ArtworkCache`'s flights,
  `OnlineAnswers.keep`, the tag editor's mixed fields, `AccentColours.ink` and `FieldUniforms`'
  192-byte stride against the shader's `Field`

## Identification
- [S · blocked: registered key] AcoustID never reached with a real key; its fixture is written from
  the documentation and `tests/live.rs` has no AcoustID test

## Build and checks
- [M] No fuzz target reaches the vault's rewriters (`blanks`, `ogg`, `chunks`, `bare`,
  `unpacking::frames_in`), tag writing (`padded`, `Overlay`, `journal::decoded`), cover decoding
  (`artwork::Drawing::of`), the remote document readers (`shared`, `spotify`, `youtube`, `linked`,
  the DASH manifest) or the analysis cache reader
- [S] Twelve of thirteen inputs that crashed `probe` sit only in the ignored `fuzz/artifacts/`, not
  in `tests/found/`; `fuzz/seeds/` is never replayed by `cargo test`
- [S] `Encoding::OF_THIS_BUILD` is bumped by hand: a `flacenc` upgrade or a changed `TUKEY_ALPHA` or
  `ARCHIVED_AT` leaves old objects called current
- [M] `zstd` (via `zstd-sys`) and `ring` (via `rustls`) are C-backed with no reason recorded in
  `dependencies.md`; `md-5` 0.10 and `miniz_oxide` 0.8 build beside the workspace's versions

## Testing
- [M] Hole punching is proved over loopback alone; two devices behind real NATs, a phone on a
  mobile network and a Linux desktop, have not been driven against a live server
- [M] The MPRIS bus tests skip without a session bus and CI's `cargo test` gives them none, so all
  but the `dbus-run-session` ones pass having run nothing
- [S] `downgrade`, `MAX_RENEGOTIATIONS` and `Error::Renegotiation` are untested (the only
  `FormatChanged` test answers the spec asked for)
- [M] Loudness and the print have no reference: no test against `ffmpeg -af ebur128` or EBU
  3341/3342, K-weighting checked at 48 kHz alone, the print asserted only to begin `AQA`
- [M] Nothing races two imports of one key, or a dedup hit or renewal against a sweep, though
  `landed_or_standing` exists for it
- [S] TIDAL's `slow_down` is untested; the hosted hifi path's hosts are constants no fake server can
  stand in for, so it runs only live
- [M] Discord's `Publisher::show` (backoff, application change, reconnect) and the signal paths
  beyond `play` and `mcp`'s SIGHUP (`until_told`, a second signal's 128+n) are untested
- [M] The Output and Processing settings panes have no test pressing their controls
- [S] The scan is never fed a non-UTF-8 name
- [M] `transport.rs`'s
  `turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame` failed once when
  the whole workspace's tests ran at once, passed alone eight times after; its frame counts assume
  the engine thread keeps pace with the test's pulls
- [L] No frame is pulled through a stream in S16, S32, packed or padded S24, 5.1 or 7.1, or with
  `NO_CONVERT`, nor recorded from a microphone node: the desktop's sink is stereo F32 and a hosted
  daemon links nothing (no session manager; a hosted WirePlumber would reach the real cards)
- [M] The drop overlay never dragged onto on a real compositor from this tree: gpui's
  `ExternalPaths` is `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the
  platform's drag events are unproved here
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone

## Later: Packaging
- [S] No package builds `resonate-server`; `packaging/server/` holds only its systemd unit
- [M · blocked: gpui] Gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc; `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
