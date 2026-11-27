# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
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
- [S · blocked: registered key] AcoustID never reached with a real key; its fixture is written from
  the documentation and `tests/live.rs` has no AcoustID test

## Catalog
- [S · 0.18.1] Replace the inline lines of a real song in tests. **Low, builder.** The lyric
  fixtures are invented now, but tests across `resonate-codec` (`sylt.rs`, `decoder.rs`,
  `writing.rs`), `resonate-library` (`tests/finding.rs`, `stored.rs`, `retag.rs`, `synced.rs`,
  `organise.rs`, `src/elsewhere.rs`), `resonate-lyrics` (`lrc.rs`, `sidecar.rs`), `resonate-sync`
  (`ledger`, `wire.rs`, `tests/nearby.rs`), `resonate-online/tests/live.rs` and
  `fuzz/seeds/lrc/sheet.lrc` still write a real song's line, title and artist inline. Write invented
  ones of the same shape. Done when no test or seed quotes a real song.
- [M · 0.18.1] Fuzz the lyric readers at the sizes their costs appear. **Medium, builder.** `lrc`,
  `ttml` and `lyricsfile` check only for a panic (`fuzz/fuzz_targets/lrc.rs:5-7`), their seeds are
  under 500 bytes and the documented run leaves `-max_len` at libFuzzer's 4 KiB, where neither the
  quadratic LRC scan nor deep TTML nesting can appear. Document `-max_len=262144`, add a dictionary
  of stamps and elements, and assert words within the input's length. Done when the target finds the
  TTML depth overflow before its fix.
- [S · 0.18.1] Run AutoEq's index matching by default. **Low, builder.** The only integration test
  of `resonate-eq` skips unless `RESONATE_AUTOEQ_INDEX` names the whole index
  (`crates/resonate-eq/tests/whole_index.rs:5-11`). Commit a few hundred index lines with the misses
  and run them by default. Done when the suggestion test runs without the variable.
- [S · blocked: flatpak] Trashing through the portal is proved from a `bwrap` sandbox built to
  Flatpak's pattern (`.flatpak-info`, a filtering `xdg-dbus-proxy`, `bwrapinfo.json`), not from a
  `flatpak run` of the real manifest: no `flatpak` is installed here, so the document portal and the
  manifest's own permissions have never been in the path

## Mobile catalog

## Device sync

## Providers and network
- [S · 0.18.1] Fuzz the media server's Host reader. **Low, builder.** `offered::addressed` reads the
  `Host` header the media server now answers only when it names the address a request arrived at
  (`crates/resonate-upnp/src/offered.rs`), and the `upnp` fuzz target never reaches it. Feed it the
  request heads the target already builds, holding that a name is never admitted. Done when the
  target calls it and its seeds pass.
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
- [S · 0.18.1] Give the server's page tests and the packaging unit's systemd check a required
  switch. **Low, builder.** `resonate-server`'s `shared::` page tests (`node`, a headless `firefox`)
  and `packaging.rs`'s systemd skip still print a skip and pass with no `RESONATE_*_TESTS=required`
  switch, as every other suite needing the outside world now has (`build.md`). Add one each and set
  it where CI provides the tool. Done when a required run without `node` fails.
- [S · 0.18.1] Walk a folder's songs in name order. **Low, builder.** The scan sorts a folder's cue
  sheets but sends its audio files in `read_dir` order (`crates/resonate-library/src/scan.rs`), so
  the order a walk reaches songs, and what a `ScanObserver` hears, differs between filesystems and
  platforms. Sort them as the sheets are. Done when a test reaches a folder's songs in name order.
- [S · 0.18.1] Fail the layering job on a misspelled banned dependency. **Low, builder.** `check`
  greps the captured tree for each name it refuses (`.github/workflows/ci.yml`, the `layering` job),
  so `refuse resonate-core nosuchdep` passes: a typo or a renamed crate in a refusal enforces
  nothing. Hold each refused name to a package `cargo metadata` knows. Done when a refusal naming no
  package fails the job.
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
- [S · 0.18.1] Walk every commented config the tree holds for comments. **Low, builder.** `ROOTS`
  leaves out `.cargo/` and `.config/` (`crates/resonate-words-build/tests/comments.rs:3-12`), and
  `Tongue::of` knows no `.pro`, `.properties`, `.install`, `.service`, `.hook` or `.rb`
  (`crates/resonate-words-build/src/comments.rs:26-38`). Add them and update `rust-style.md`'s list.
  Done when `# x` in `android/gradle.properties` fails the test.
- [S · 0.18.1] Catch a catalog call that passes its values in the sentence's order. **Low,
  builder.** `resonate-words-build` writes each message's function with its parameters in
  alphabetical order, so a call passing them in the order the sentence says them still compiles
  when the types agree: `history --import-file` said *brought in FILE from 1* until
  `cli::` caught it. Name the arguments at the call (a struct of the message's variables, or a
  builder), or refuse a call whose arguments are named otherwise than its parameters. Done when
  swapping two arguments of the same type fails to compile or fails a test.
- [S · 0.18.1] Refuse a lone lowercase word in the catalog walks. **Low, builder.** `prose_in` reads a
  literal as words only when it is capitalised or holds a space (`resonate-words-build`), so a
  shown `none` or `faststart` passed the binary's catalog test until moved by hand. Weigh a lone
  lowercase word where it reaches a listener (a `println!`/`said` argument, a table cell) without
  flagging keys and identifiers. Done when a printed literal `none` fails the binary's catalog
  test.
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
- [S · 0.18.1] The signing jobs have not run. `release.yml`, `android-apk.yml` and `macos-dmg.yml`
  build on one runner and sign on another (`release.md`'s *Signing apart from the build*), and a
  run that releases no tag names the environment `${{ ... || '' }}`, an empty name, which GitHub's
  documentation does not say is read as no environment; nor has `build.ps1 -Stage`, the macOS
  `bundle` and `image` stages on a Mac or `apksigner sign` with the release keystore run. Dispatch
  `android-apk.yml` and `macos-dmg.yml` on a branch and on a `v*` tag, and `release.yml` on a
  branch, once the `release-signing` environment is set. Done when each leg has run and the APK it
  signs installs over the last release.

## Dependencies
- [S · blocked: flacenc] `heapless` 0.8.0, which `flacenc` 0.5.1 (the latest, and upstream's
  `main`) brings, carries nine unsound advisories fixed in 0.9.4 (RUSTSEC-2026-0574 to 0582), ignored
  by id in `.cargo/audit.toml` until a `flacenc` on `heapless` 0.9 is published
- [S · blocked: gpui] `cargo audit`'s other warnings are other crates' trees, each ignored by id in
  `.cargo/audit.toml`: zbus 5 under gpui's accesskit and ashpd, beside `resonate-mpris`'s zbus 6 (two
  built), with `zvariant`, `zvariant_derive` and `zbus_names` unmaintained since they were merged into
  zbus 6; `ttf-parser` under gpui's `usvg`; `chrono` 0.4.45 unmaintained under gpui's `scheduler`;
  and the build-time `paste` (lofty) and `basic-toml` (uniffi's askama)

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
