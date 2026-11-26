# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M · 0.18.1] Drive the resampler over every rate pair with audio. **Medium, builder.** The lab's
  pairs stop at 44.1, 48, 96 and 192 kHz (`dsp-lab/src/suites/resample/mod.rs:31-39`), the bench at
  a few (`crates/resonate-dsp/benches/stages.rs:40-47`), and the one grid test only builds a
  resampler and passes over any `Err` (`crates/resonate-dsp/src/resample.rs:2226-2248`), so the
  staged compaction's underflow at 1/29 to 1/32 is reached by nothing. Add a property test over rate
  pair, quality, phase, block size and reset that asserts finite output and the expected frames, and
  fail the grid on an unexpected `Err`. Done when the property test finds the compaction underflow
  before its fix.
- [M · 0.18.1] Build and run the equaliser a fuzzed profile describes. **Medium, builder.** The
  `equaliser` target stops at a text round trip (`fuzz/fuzz_targets/equaliser.rs:7-11`),
  `measurement` never fits (`measurement.rs:12-22`), and `Store::read_in` and `targets::parse` have
  no target. Build the stage at 44.1, 96 and 192 kHz, call `fitted_preamp` and process an impulse,
  asserting finite bounded output. Done when a profile of 32 bands at +40 dB fails the target before
  its fix.
- [L · blocked: hardware] No forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

- [S · blocked: hardware] A zone handover's alignment is proved against a fake graph whose
  callbacks run late by varying amounts while their device timestamps stay true (within two
  frames); no two devices here are heard side by side, so how closely two real sound cards meet
  (how true their own timestamps and latency readings are, and clock drift over the overlap) has
  not been measured

- [S · blocked: hardware] A phone driven from another device is proved on the host over a real engine
  and the peer wire alone: audio focus asked for a play pressed elsewhere (`soundBegan`) and the
  iPhone's session activated for one (`SessionAsked`) have not been heard on a device

- [S · blocked: hardware] Android zones on two outputs are proved on the host alone: no USB DAC or
  Bluetooth device is attached here, so whether Android plays two Oboe streams asking two device ids
  (`setDeviceId`) on those two devices at once, rather than routing media to one, has not been heard

- [S · blocked: hardware] Android's *Curves for other outputs* page (`OutputCurvesScreen`) has not
  been seen: the emulator offers one output, so no other output keeps a curve and the equaliser
  never shows the row; its arithmetic is held by `OutputCurvesTest` alone

- [S · blocked: hardware] An iPhone following the song's rate (`follow-song-rate`) is not proved on
  a USB DAC: no device is attached here, so neither the route taking `preferredSampleRate` nor the
  gap of the RemoteIO reopen it causes has been heard


- [S · blocked: hardware] Learning what a device plays is proved against loopback fakes alone
  (`resonate-upnp/tests/renderer.rs`, `resonate-cast/tests/receiver.rs`): no streamer or receiver
  has been tried, so whether a real one that cannot decode a rate says `ERROR_OCCURRED` or
  `IDLE`/`ERROR`, stops fetching or lets its clock stand within the 10 s a trial lasts is unheard
- [S · blocked: hardware] No Google Cast receiver has been played to: the Cast stream is proved
  against a loopback fake alone (`resonate-cast/tests/receiver.rs`), and no Chromecast, Nest
  speaker or speaker group is on the network here, so whether the Default Media Receiver takes an
  endless `LIVE` FLAC from the player's address, how `currentTime` runs on it, and the TLS 1.2
  handshake of an older receiver have not been heard

- [S · blocked: hardware] Cast on a phone has never been heard against a real receiver: the
  phones' receivers are proved on the host by what `resonate-mobile` holds of them (an address
  parsed, held, refused when it is neither an address nor a name, listed with the renderers and
  forgotten) and by `resonate-cast`'s loopback fake alone, so the mDNS browse over Wi-Fi (Android's
  `MulticastLock` held), a receiver given by name from a phone, a Default Media Receiver fetching
  the phone's stream across the network and an iPhone's browse being refused without the multicast
  entitlement have not been seen

- [S · blocked: hardware] A phone's zone on a network renderer is proved on the host alone, over a
  fake renderer on loopback: no renderer is on this network and neither phone ran it, so the
  search over Wi-Fi (Android's `MulticastLock` held), a renderer fetching the phone's stream across
  the network, playback with the screen off (no Wi-Fi lock is taken) and an iPhone's search being
  refused without the multicast entitlement have not been seen

## iOS
- [M · blocked: hardware] The Core Audio tests (`resonate-coreaudio`'s `stream`: a tone through
  the shared sink, a hog-mode rate switch and its restoration, a 24-bit ramp read back bit for bit
  through a loopback device) need an output device, and the hog and bit-exact halves a device named
  *BlackHole*; they skip without one, so they have run nowhere
- [S · blocked: Xcode's processor] `ios/app-intents.py` is held by a fixture of `Siri.swift`'s const
  values and its own output, never by Apple's: it was compared with `appintentsmetadataprocessor`
  by hand, on another xtool app's fixtures (a plain intent, a `String` parameter, App Shortcuts),
  and `AudioPlaybackIntent` as `AudioStarting` was never set beside Apple's output for it. The
  processor is an arm64 Mach-O

## Identification
- [M · 0.18.1] Check the Chromaprint print against a committed one. **Medium, builder.** The one
  correctness test compares with `fpcalc` and skips without it
  (`crates/resonate-analysis/tests/references.rs:389-435`), which CI does not install
  (`.github/workflows/ci.yml:21-24`); the others are self-consistency or `starts_with("AQA")`
  (`crates/resonate-analysis/src/print.rs:168-169`), and only an 11 025 Hz input is compared. Commit
  a golden print of a seeded signal at 44.1, 48 and 96 kHz and install `chromaprint` where the tests
  run. Done when a change to the filterbank fails without `fpcalc`.
- [S · blocked: registered key] AcoustID never reached with a real key; its fixture is written from
  the documentation and `tests/live.rs` has no AcoustID test

## Catalog
- [M · 0.18.1] Build the MusicBrainz copy from a hostile or failing export. **Medium, builder.**
  `crates/resonate-musicbrainz/tests/mirror.rs:112-160` always writes a valid schema and well-formed
  COPY text, and no test builds over an existing copy with a cut archive, a wrong `SCHEMA_SEQUENCE`
  or an oversized row, or shows a failed build leaves the old copy answering and nothing staged. Add
  those builds. Done when each fails with its error and the old copy still answers.
- [M · 0.18.1] Fuzz the lyric readers at the sizes their costs appear. **Medium, builder.** `lrc`,
  `ttml` and `lyricsfile` check only for a panic (`fuzz/fuzz_targets/lrc.rs:5-7`), their seeds are
  under 500 bytes and the documented run leaves `-max_len` at libFuzzer's 4 KiB, where neither the
  quadratic LRC scan nor deep TTML nesting can appear. Document `-max_len=262144`, add a dictionary
  of stamps and elements, and assert words within the input's length. Done when the target finds the
  TTML depth overflow before its fix.
- [S · 0.18.1] Replace the lyric fixtures holding real songs. **Low, builder.**
  `crates/resonate-lyrics/tests/fixtures/ttml_duet.ttml` and `ttml_worded.ttml` are verbatim service
  answers carrying a real song's whole lyrics and its songwriters' names. Write invented lyrics and
  names of the same structure. Done when no fixture names a real songwriter.
- [S · 0.18.1] Run AutoEq's index matching by default. **Low, builder.** The only integration test
  of `resonate-eq` skips unless `RESONATE_AUTOEQ_INDEX` names the whole index
  (`crates/resonate-eq/tests/whole_index.rs:5-11`). Commit a few hundred index lines with the misses
  and run them by default. Done when the suggestion test runs without the variable.
- [S · 0.18.1] Prove that a write against a broken settings file leaves it untouched. **Low,
  builder.** No test of `crates/resonate/src/config/tests/writing.rs` writes a setting against a
  file that does not parse. Add it. Done when the file is byte for byte the same after the refused
  write.
- [S · blocked: flatpak] Trashing through the portal is proved from a `bwrap` sandbox built to
  Flatpak's pattern (`.flatpak-info`, a filtering `xdg-dbus-proxy`, `bwrapinfo.json`), not from a
  `flatpak run` of the real manifest: no `flatpak` is installed here, so the document portal and the
  manifest's own permissions have never been in the path

## Mobile catalog

## Device sync

## Providers and network
- [M · 0.18.1] Hold the providers' server-driven limits in tests. **Medium, builder.** No fake keeps
  answering full pages or claims a huge total, so `PAGES_AT_MOST`
  (`crates/providers/subsonic/src/lib.rs:35`) and Jellyfin's caps
  (`crates/providers/jellyfin/src/lib.rs:38-41`) are unproved; every `Retry-After` is `0` or `1`, so
  the four `LONGEST_RETRY_AFTER` clamps never act; and Jellyfin's fake cannot cut or stall a body.
  Add an endless-pages fake, make the retry ceiling a field a test can lower, and give Jellyfin's
  fake the `Sent` modes. Done when removing each cap fails a test.
- [S · 0.18.1] Fuzz the media server's Host reader. **Low, builder.** `offered::addressed` reads the
  `Host` header the media server now answers only when it names the address a request arrived at
  (`crates/resonate-upnp/src/offered.rs`), and the `upnp` fuzz target never reaches it. Feed it the
  request heads the target already builds, holding that a name is never admitted. Done when the
  target calls it and its seeds pass.
- [M · 0.18.1] Let the fake renderer fault, stall and lie. **Medium, builder.** The renderer fake
  always answers 200 with a fixed name (`crates/resonate-upnp/tests/renderer.rs:245`, `:271-420`),
  and `ANSWER_BYTES_AT_MOST` and `HEAD_BYTES_AT_MOST` (`crates/resonate-upnp/src/http.rs:11-15`) are
  referenced by no test. Give `Profile` a fault on a named action, an answer that never comes, an
  oversized body and a long name. Done when each ends the stream cleanly or drops the sink in a
  test.
- [S · blocked: a listener's account] `resonate-ytmusic`'s gated live test (`tests/live.rs`,
  `RESONATE_YTMUSIC_TESTS`, with `RESONATE_YTMUSIC_COOKIES`, `RESONATE_YTMUSIC_SONG`,
  `RESONATE_YTMUSIC_PLAYLIST`) has never been run: it lists an album song, reads the head of its best
  tier and reads a playlist of the account, and nothing here holds an account or has run it at all
- [S · blocked: NetworkManager] `resonate-mpris`'s `metered.rs` has only been run against a
  stand-in `org.freedesktop.NetworkManager` on a private bus: the `Metered` values a real manager
  sends for a phone hotspot or a connection the listener marked metered, and that it announces a
  change as a `PropertiesChanged` on the manager object, were taken from its documentation
- [S · blocked: network] That the hosted hifi service still answers a request carrying neither
  `Origin` nor `Referer` is proved by its gated live test alone (`RESONATE_HIFI_LIVE_TESTS`), which
  nothing here has run
- [S · blocked: an account without Opus 774] A kept song's AAC tier (`Aac141`, `.m4a`) has not been
  measured: nothing here has downloaded one, so whether the tag writer takes DASH's fragmented MP4 and
  the decoder probes it is unproved; it is reached only where `774` is absent
- [S · blocked: registered app] No real Spotify account has been read: the sign-in and the reads are
  proved against a fake server on loopback whose fixtures are written from the published Web API
  reference, and `resonate-online`'s gated live test (`RESONATE_ONLINE_TESTS`,
  `RESONATE_SPOTIFY_CLIENT_ID`, `RESONATE_SPOTIFY_TOKEN_FILE`) has never run; a registered app's
  development mode (`403` on a playlist the account only follows), `SPOTIFY_REDIRECT` accepted as
  registered and the rotation of its refresh tokens are as Spotify documents them, unmeasured; on a
  phone, the browser reaching the boundary's loopback listener (an `ASWebAuthenticationSession` on
  iOS, a browser in front of a backgrounded process on Android) has not been seen on a device

## Testing
- [M · 0.18.1] Run the binary's subcommands as processes in a test. **Medium, builder.**
  `crates/resonate/tests` spawns the binary only for `play` and `mcp` (`terminal.rs:109`,
  `signals.rs:37`), and `organising.rs`, `kinship.rs`, `share.rs`, `sync.rs`, `vault.rs`,
  `studies.rs`, `playlists.rs`, `served.rs`, `suggest.rs` and `updates.rs` have no test module, so
  no exit status, output or file written by a subcommand is asserted, which let every filed
  command-line bug through. Add `tests/cli.rs` running each mutating subcommand's failing and
  succeeding case under the `Home` that `signals.rs` builds. Done when `dedupe --apply` with a
  refused trash fails it before the fix.
- [M · 0.18.1] Make a skipped test fail where its requirement is meant to be present. **Medium,
  builder.** The session-bus, pseudo-terminal, portal, live service, PipeWire stream, capture, Cast
  multicast and reference-tool suites all print a skip and pass
  (`crates/resonate-mpris/tests/bus.rs:586`, `crates/resonate/tests/terminal.rs:197-210`,
  `crates/resonate-online/tests/live.rs:32-43`, `crates/resonate-pipewire/tests/stream.rs:48-69`,
  `crates/resonate-dsp/tests/restoration.rs:236-262`,
  `crates/resonate-analysis/tests/references.rs`), with no `RESONATE_*_TESTS=required` switch as
  `coreaudio` and `nearby::` have, and the child-run helpers name their test by a typed string, so a
  rename runs nothing and passes. Add the switch to each, set it where CI provides the requirement,
  and take the child's name from `module_path!()` and fail on zero tests run. Done when CI without a
  session bus fails the bus suite.
- [M · 0.18.1] Test the portal's trash where it can fail. **Medium, builder.** Both portal tests
  skip without `xdg-desktop-portal`, `bwrap` and `xdg-dbus-proxy`, none of which CI installs
  (`crates/resonate-mpris/tests/trash.rs:85-89`, `:152-157`, `.github/workflows/ci.yml:19-22`), and
  they assert only success. Install them in CI and add a refusal case asserting the `Trashing`
  variant. Done when a refused trash is tested.
- [M · 0.18.1] Stand in for the portal's inhibit and record what notifications say. **Medium,
  builder.** No test stands in for `org.freedesktop.portal.Desktop`, so holding and releasing the
  idle inhibit is proved only by `Gripping::toward` (`crates/resonate-mpris/src/idle.rs:115-128`);
  the notification stand-in ignores the summary, body, hints and replacement
  (`crates/resonate-mpris/tests/bus.rs:3605-3634`); and no fixture in the MCP or MPRIS suites holds
  hostile tag text, a NUL, markup, a newline or 10 kB (`crates/resonate-mcp/tests/server.rs:72-90`,
  `bus.rs:148-180`). Record both stand-ins' calls and parameterise the fixtures. Done when pause
  releases the inhibit and a NUL title is notified, in tests.
- [M · 0.18.1] Test MCP's `refresh_playlist` and pin each tool's hints. **Medium, builder.** No test
  calls `refresh_playlist` (`crates/resonate-mcp/src/tools.rs:174-213`, `:902-907`), and the
  annotation test compares `destroys()` with itself (`crates/resonate-mcp/tests/server.rs:881-923`).
  Test a refresh, a list with no link and a failed fetch leaving rows, and pin hints by a table of
  names. Done when a tool losing its destructive hint fails.
- [M · 0.18.1] Make the layering job fail when `cargo tree` does. **Medium, builder.** Its loops end
  `|| true` (`.github/workflows/ci.yml:144`, `:153`, `:163`, `:173`, `:217`, `:232`, `:239`, `:251`,
  `:258`, `:276`, `:280`), which under `pipefail` swallows a failing `cargo tree`, and `check
  "$(held_by ...)"` (`:207`, `:212`, `:215`, `:230`, `:288-304`) is not caught by `set -e`, so a
  renamed crate or a target that stops resolving enforces nothing and passes. Capture `held_by` into
  a variable first and keep `|| true` on the `grep` alone. Done when a nonexistent crate fails the
  job.
- [M · 0.18.1] Select the tests that read files outside their crate in `--changed`. **Medium,
  builder.** `owner()` maps a changed file to the member holding it
  (`crates/xtask/src/changed.rs:112-146`), so a change under `packaging/`, `.github/`, `fuzz/seeds/`
  or `docs/` selects only `resonate-words-build`, while `resonate`, `resonate-server`,
  `resonate-codec`, `resonate-sync` and `resonate-musicbrainz` have tests reading them. Map those
  paths to their readers, or treat an unowned file as workspace-wide. Done when touching
  `packaging/resonate.spec` selects `resonate`.
- [S · 0.18.1] Stop a session in MCP's drain test while its scan still runs. **Low, builder.**
  `crates/resonate-mcp/tests/server.rs:1949-1984` stops 200 ms into a one-file scan that has
  finished by then, so it passes whether or not passes drain. Use the gated 40-file tree. Done when
  the test fails with draining removed.
- [S · 0.18.1] Walk every commented config the tree holds for comments. **Low, builder.** `ROOTS`
  leaves out `.cargo/` and `.config/` (`crates/resonate-words-build/tests/comments.rs:3-12`), and
  `Tongue::of` knows no `.pro`, `.properties`, `.install`, `.service`, `.hook` or `.rb`
  (`crates/resonate-words-build/src/comments.rs:26-38`). Add them and update `rust-style.md`'s list.
  Done when `# x` in `android/gradle.properties` fails the test.
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
- [S · blocked: a phone] Android's background sync is proved by its rules in tests and by forced
  job runs on the emulator alone: when a real phone's Doze, standby buckets and maker's battery
  savers let the 15-minute wake run, and a carry-on finishing songs mid-transfer after the app is
  left, have not been seen
- [S · blocked: a device] The listening test is proved on the host alone (`resonate abx`'s session and
  keys, the boundary's object, the window's group under the test platform and the phones' wording): the commands it sends are checked at the
  engine's published settings, but no run has been heard on PipeWire with a real song, nor the
  sections driven on an emulator or an iPhone
- [M · blocked: hardware] A microphone recording not proved against real sound reaching a microphone
- [M · blocked: screen reader] No front end has been heard with a screen reader: the desktop's cells,
  links, lyric lines, plots, bars, cards, headings, dialogs, the caret and selection and the menu a
  reader follows are built and read back in tests only (Orca over AT-SPI; gpui's test platform never
  activates the tree), and neither TalkBack on the Compose screens nor VoiceOver on the SwiftUI app
  has read the accessibility work of 0.4.0 (the lyric elements, the adjustable bars, the hidden
  background under Now Playing, the announcements), and the iOS landscape and iPad layouts (Now
  Playing's two columns, the Library tab's split view, the sheets' form and page sizes, and the
  accessibility-size layouts in each) have been proved by their rules in tests and a clean build,
  never seen on an iPhone turned on its side or an iPad
- [M · blocked: hardware] The iOS Home feed and the Library's four shelves are proved by their logic
  in tests and a clean build, and the Library's tab row was rebuilt after it juddered on an iPhone;
  still to be seen there: the row's one capsule sliding across after a shelf's scroll view is
  swapped and following a drag, the Library's own search field hidden until a shelf scrolls up or is
  pulled down at its top, under a bar of one height, its recent searches while focused, the thumb
  appearing from `onScrollGeometryChange` on a `List` and the frame after a drag moves the Songs
  window, the selection bar against the minimising tab bar and the mini
  player, each shelf's toolbar, *Jump back in*'s tiles sizing themselves in their row, the
  *Recently added* carousel coming to rest on a cover and its play button over a bright one, and
  the album and playlist grids' columns at each width
- [S · blocked: hardware] Now Playing's opening rest ending as `PresentationProbe` hears the zoom
  end (and resting again as the cover closes), the cover's bucket estimated before the first
  opening and the volume driver mounted after the zoom are proved by a clean build alone: whether a
  representable's controller is told `viewDidAppear` as the full-screen cover's zoom ends, and the
  frame rate of the zoom with them, are still to be seen in Instruments on an iPhone
- [S · blocked: hardware] The lyrics moving Now Playing's controls away (`ControlsTuck`) is proved by
  `LyricFootTests` and a clean build: the slide and fade, the pane reaching under the band, the
  fade mask moving with them and the controls coming back after `HANDS_OFF` are still to be seen on
  an iPhone
- [S · blocked: hardware] The sideloaded IPA and its AltStore source are proved by their shape in
  tests: no release has yet been added as a source to SideStore or AltStore, signed with a free
  Apple ID and installed, so the declared permissions matching what AltStore reads from the IPA and
  the Enhanced Security entitlements passing a free profile are unseen
- [S · blocked: hardware] The iPhone Duo's poses are proved by their rules in tests and a clean
  build against the iOS 27.2 SDK, never seen on a Duo or the Duo simulator (Xcode's Device Hub needs a
  Mac): Now Playing's columns meeting at the fold held as a book, the cover above the fold and the
  controls below it on a table, Up next stopping short of the fold, and the inner screen edge to edge
- [S · blocked: a desktop] The desktop's colour scheme and text scale are proved on how a
  value is read alone: no GNOME or KDE session is reachable from the agent shell, so the portal's
  `color-scheme` and `SettingChanged`, GNOME's `text-scaling-factor` and KDE's `font` point size
  (taken against an ordinary ten) have not been read from a live portal, and the light palettes have
  been held by the contrast tests but not looked at in a window

## Release
- [M · 0.18.1] Sign apart from the build. **Medium, architect.** Each signing secret is set on the
  one step that runs `build.ps1`, `build.sh` or `gradlew assembleRelease`
  (`.github/workflows/release.yml:219-224`, `:249-258`, `:291-300`, `android-apk.yml:34-43`,
  `macos-dmg.yml:37-45`), so every build script and proc-macro of about 1 086 packages, gpui's git
  tree included, can read them, and the Android keystore is on disk before Gradle starts; the
  secrets are repository-wide and `android-apk.yml` and `macos-dmg.yml` run from any branch. Build
  with no secrets, sign in a following step, put the secrets in an environment limited to `v*` tags
  and give the dispatch workflows the same. Done when no build step's environment names a signing
  secret.

## Dependencies
- [S · 0.18.1] Gate on advisories in the local check. **Low, builder.** `heapless` 0.8.0, which
  `flacenc` brings (`Cargo.toml:174`), carries ten unsound advisories fixed in 0.9.4, beyond the
  trees the blocked entry below names, and nothing in the tree gates on an advisory: there is no
  `deny.toml` or `audit.toml` and no step runs `cargo audit`. Run it as an `xtask` step with the
  entries below ignored by name, and move to a `flacenc` on `heapless` 0.9. Done when a new advisory
  fails `cargo xtask check`.
- [S · blocked: gpui] `cargo audit`'s other warnings are other crates' trees: `glib` 0.18.5 unsound
  (RUSTSEC-2024-0429) under `tao`, `webkit2gtk` and wry, for `resonate-signin`'s window alone; zbus
  5 under gpui's accesskit and ashpd, beside `resonate-mpris`'s zbus 6 (two built), with
  `zvariant` and `zbus_names` unmaintained since they were merged into zbus 6; `rustybuzz` and
  `ttf-parser` under gpui's `usvg`; and the build-time `paste` (lofty), `proc-macro-error` (glib)
  and `basic-toml` (uniffi's askama)

## Later: Structure
- [L] The party, inspector, services, queue, lyrics, analysis, missing, statistics, suggestions
  and settings panes (the settings' fields and its own state) and the browse panes' scrolls,
  orders, reach and what they draw around their rows and on the record card (`BrowsePanes`) are
  entities owning their state and subscriptions (`ui.md`), but `RootView` still has 73 fields (58
  more in its groups), with `RootView::new` some 400 lines making 27 subscriptions. Each wants a
  driven test pinning it before it moves, as the panes moved so far got, and a pane's drawing stays
  on the root while its controls live in the root's focus ring
- [M · blocked: jxl-oxide reading jxl-encoder's ANS-coded output] The vault builds two JPEG XL
  encoders, `zune-jpegxl` for a cover's pixels and `jxl-encoder` for a JPEG's transcode. Measured on
  600 to 1400 px covers, `jxl-encoder` 0.3.4's lossless effort 7 is about half zune's size (0.6 to
  2.6 s a cover; effort 9 a further 20 % for ten times the time) and would let a cover with an ICC
  profile become JXL too, and libjxl's `djxl` reads it back bit for bit; but `jxl-oxide` 0.12.6, which
  validates every cover and draws it back, fails from effort 3 up (`UnexpectedEof` rendering ANS
  entropy coding), and without ANS the output is no smaller than zune's and twenty times slower

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone

## Later: Together on the phones

- [S] Trying Wrapped, shared playlists and parties on a real iPhone: they build clean and their logic is tested, but the views have never run.
- [S] A party between two real devices in different places, measuring the offset by ear or by loopback.
