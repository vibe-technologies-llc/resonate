# Testing and build

Proof and plumbing: what no test, fuzz target, hardware or CI step yet covers, and dependencies and
packaging that need a decision. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Playback and output
- [M · 0.17.1] Prove the playback state before exercising a waiting boundary or a waiting row.
  **Medium, builder.** The boundary tests can finish preparing the next row after the fixed sleep,
  and the waiting-row settings test discards both command outcomes
  (`crates/resonate-engine/tests/transport.rs:7314`, `crates/resonate-engine/tests/transport.rs:5378`).
  Wait for the ring or fake sink to observe the precise boundary state, and assert both setting
  requests landed before checking the position. Done when the tests cover the intended state
  under delayed preparation and fail if either setting command is dropped.
- [S · 0.17.1] Exercise levelling in the published output-settings guard. **Low, builder.**
  The list of mutations omits levelling, so dropping its comparison would leave the guard green
  (`crates/resonate-engine/src/state.rs:147`, `crates/resonate-engine/src/state.rs:365`,
  `crates/resonate-engine/src/state.rs:471`). Done when a levelling-only change is published by the
  test and removing its comparison makes that test fail.
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
- [S · 0.17.1] Test iOS file removal errors with overlapping numeric codes. **Low, builder.**
  The negatives use different numbers from the positive cases, so ignoring the error
  domain would pass (`ios/Tests/ResonateLogicTests/FileRemovalTests.swift:41`,
  `ios/Sources/ResonateLogic/FileRemoval.swift:22`). Add interrupted POSIX I/O and a
  wrong-domain match. Done when those errors keep the catalog row eligible and
  a real not-found result under a reachable folder still counts as gone.
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
- [M · 0.17.1] Exercise tag-write failures after staging and during recovery. **Medium, builder.**
  The failed staged-write fixture is rejected before staging, and the recovery fixtures begin
  with an already torn file (`crates/resonate-codec/src/writing.rs:1790`,
  `crates/resonate-codec/src/writing.rs:2238`, `crates/resonate-codec/src/writing.rs:2945`).
  Inject failures after creating a rechunked WAVE stage, after a file changes during a staged
  save, and during whole-copy write-back. Done when the tests prove ordinary failures leave the
  original intact, discard their stage and retain a usable recovery copy for a torn write-back.
- [M · 0.17.1] Prove a retag writes only the chosen roots and only on apply. **Medium, builder.**
  Retag validates and filters the roots and guards its staging sweep with `apply`, but the
  retag tests exercise all roots together and do not check the preview sweep
  (`crates/resonate-library/src/retag.rs:229`, `crates/resonate-library/src/retag.rs:300`,
  `crates/resonate-library/tests/retag.rs:267`). Done when a two-root fixture proves the other
  root's bytes stay identical, an unknown root is refused, and preview leaves files and stages
  untouched.
- [M · 0.17.1] Prove interrupted retag undo keeps the original tags available. **Medium, builder.**
  Cancellation and unreadable files can leave an undo unfinished. The current partial-undo
  fixture depends on its runner being unable to write to a directory
  (`crates/resonate-library/src/retag.rs:269`, `crates/resonate-library/src/retag.rs:878`,
  `crates/resonate-library/src/retag.rs:1471`, `crates/resonate-library/tests/retag.rs:805`).
  Use a gated tag sink and a temporarily missing file in the test. Done when cancellation,
  refusal and absence preserve the notes for unfinished files and a later undo restores them.
- [S · 0.17.1] Cover removing an unterminated final cue-sheet line. **Low, builder.**
  The existing test inserts at the end, leaving the removal and mixed-edit paths without a
  regression check (`crates/resonate-codec/src/sheet.rs:319`, `crates/resonate-codec/src/sheet.rs:598`).
  Done when removing the final line alone and with an insertion preserves the preceding lines,
  the chosen line ending and a legacy sheet's encoding.
- [M · 0.17.1] Test the exact history retention cutoff with two scrobblers.
  **Medium, builder.** The ageing tests use fresh or 400-day-old samples and one
  service, so they miss a shorter retention window or a lagging second service
  (`crates/resonate-library/tests/history.rs:403`, `:468`,
  `crates/resonate-library/tests/support/fakes.rs:89`). Use a fixed clock and different
  submission cursors. Done when the test proves the cutoff boundary, ageing of unheld
  listens, and protection for rows the slower service still needs.
- [M · 0.17.1] Verify that a failed desktop upgrade rolls back and retries on disk.
  **Low, builder.** The released-stamp sweep lays out empty in-memory catalogs and checks
  the final stamp (`crates/resonate-library/src/schema_released_tests.rs:70`).
  Exercise a populated file catalog with an injected migration failure. Done when the
  failure leaves the old schema, rows and stamp intact, and retry reaches the current
  stamp with the rows preserved.
- [M · 0.17.1] Carry populated mobile state through the full released migration chain.
  **Low, builder.** The released-version sweep uses an empty catalog, and focused tests
  populate only individual steps (`crates/resonate-mobile/src/catalog_released_tests.rs:111`).
  Add a populated old-version fixture with listening history, saved queue and favourites.
  Done when an upgrade keeps the identities and saved values across the full chain.
- [S · 0.17.1] Pin down desktop resumption when the last saved row is unreadable.
  **Low, builder.** The test for dropping the selected row has a successor, while the
  last-row case clamps to the preceding song at its start
  (`crates/resonate-library/src/resumed.rs:49`, `:160`, `.claude/rules/library.md:79`).
  Decide and document the intended fallback. Done when a regression test covers a
  saved queue whose selected last URI cannot be read.
- [S · 0.17.1] Give same-size rescan fixtures a known modification time. **Low, builder.**
  The incremental test writes the same bytes twice and expects a new timestamp, which
  a coarse filesystem can round to the same value
  (`crates/resonate-library/tests/stored.rs:415`, `:436`,
  `crates/resonate-library/src/scan.rs:1380`). Set distinct fixture timestamps where
  the test needs a changed stamp. Done when the enrichment assertions run reliably
  with both fixture writes in the same clock tick.
- [M · 0.17.1] Exercise unreadable-folder scans regardless of process privileges. **Low, builder.**
  The permission cases return successfully without checking scan behavior when the runner
  can read or search the fixture anyway (`crates/resonate-library/tests/scanning.rs:1659`,
  `:1698`). Inject the denied listing or metadata read into the existing scan cases.
  Done when the tests assert the protected rows stay even when the runner can bypass
  filesystem mode bits.
- [M · 0.17.1] Prove which catalog row each restored desktop queue entry names. **Low, builder.**
  The test for a catalog-backed queue entry only asserts that some track ID was found
  (`crates/resonate-library/tests/history.rs:121`,
  `crates/resonate-library/src/db/resuming.rs:57`). Assert the actual ID and add two cue
  cuts of the same file. Done when the test catches assigning either cut to its neighbour
  and a pruned row keeps its saved location without a catalog ID.
- [S · blocked: flatpak] Trashing through the portal is proved from a `bwrap` sandbox built to
  Flatpak's pattern (`.flatpak-info`, a filtering `xdg-dbus-proxy`, `bwrapinfo.json`), not from a
  `flatpak run` of the real manifest: no `flatpak` is installed here, so the document portal and the
  manifest's own permissions have never been in the path

## Mobile catalog
- [M · 0.17.1] Exercise phone restores through a new player on the saved catalog.
  **Medium, builder.** The save and queue tests use an already-open harness and the
  released-step tests reopen only the catalog (`crates/resonate-mobile/src/harness.rs:309`,
  `crates/resonate-mobile/src/lib.rs:434`, `crates/resonate-mobile/src/zoning.rs:696`).
  Reopen through the same path the apps use. Done when tests observe each zone's queue,
  shuffle order, play-next span, volume and restore-off behavior after relaunch.
- [M · 0.17.1] Test Android queue restoration from a window with a nonzero start.
  **Medium, builder.** The current positive test builds media IDs with the helper it
  checks. It never exercises conversion between a saved queue position and a Media3
  window index (`android/app/src/test/java/org/resonate/android/ResumedQueueTest.kt:35`,
  `android/app/src/main/java/org/resonate/android/PlaybackService.kt:48`,
  `android/app/src/main/java/org/resonate/android/RustMediaPlayer.kt:279`). Assert fixed
  IDs for library and served items and exercise both index conversions. Done when a
  test restores a mixed window starting after row zero and lands on the original song.

## Providers and network
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

## Dependencies
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
