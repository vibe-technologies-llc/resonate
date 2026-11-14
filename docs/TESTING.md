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
- [S · blocked: flatpak] Trashing through the portal is proved from a `bwrap` sandbox built to
  Flatpak's pattern (`.flatpak-info`, a filtering `xdg-dbus-proxy`, `bwrapinfo.json`), not from a
  `flatpak run` of the real manifest: no `flatpak` is installed here, so the document portal and the
  manifest's own permissions have never been in the path

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
- [S · 0.12.0] Nothing holds the phones' released schema: `catalog::FIRST_VERSION` is today's `V1`
  and the `laid_before` tests replay each step's present text, so an edit to `V1` or a shipped step
  passes every test while phones that ran the old text keep it; a fixture of `V1` and each released
  step as tagged (or their hashes), and a `documents.sqlite` laid out at 0.1.0, would hold it
- [S · 0.12.0] No driven test holds that the window stops redrawing once a pane or sheet is left:
  five timers (`party.rs`, `settings/sync.rs`, `sharing.rs`, `settings/online.rs`, `analysis.rs`)
  each end themselves by hand, and the party pane's never does (`PERFORMANCE.md`)
- [M · 0.12.0] Parsers of untrusted bytes no fuzz target reaches: `SheetText::read` and `edited`
  (the `cue` target drives `read_cue` and `renamed_cue` alone), `matroska_tags::attached` (`padding`
  drives `rewritten`), the whole copy's trailer and record (`bound_of`, `Record::read`, which read
  files found in the shared temp folders), the lofty reads every retag preview makes
  (`rating_in`, `Counted::read_in`), the streamed SOFA path (`hdf5_streamed`), the vault's
  `standing::of_a_flac` and `of_a_packed_wave`, and the UPnP stream server's request head
  (`serve.rs`'s `judged` and `token_in`)
- [S · 0.12.0] The hand-written SYLT reader (`sylt.rs`'s `as_lrc`) is fuzzed only if the `probe`
  target invents an ID3v2 header and a SYLT frame from nothing: no seed in `fuzz/seeds/probe` holds
  one, in either timestamp format
- [S · 0.12.0] `resonate-mobile`'s `sync::copied::tests::a_phone_keeping_a_copy_fetches_it_from_a_desk_and_serves_lookups_from_it`
  fails here at its 40 s wait (*the copy kept on the phone never happened*), alone and in a whole
  run, before the move to nextest and after it
- [M · 0.12.0] `resonate-analysis` is most of a test run's work: its tests sum to about 800 s of the
  headless run's 2 560 s (`steps::tests::every_grid_reads_the_same_alone_as_on_a_crew` takes 16 s
  alone and 81 s among the rest, each process's rayon crews contending for the same cores);
  sharing one decode or encode between the `references::` and `analysis::` cases that make the same
  file, or shorter signals where a case does not need seconds, would shorten every run
- [S · 0.12.0] The phones' `enrichment::tests::hearing::` tests take about 21 s each and the
  library's study and gathering tests 15 to 43 s; whether they wait out a timer a test could shorten
  is unread
- [S · 0.12.0] CI builds and runs each test line in one job; `cargo nextest archive` and
  `--partition` would build once and spread the headless tests over runners, and a faster linker
  (`wild`, pure Rust, or `mold`) is untried beside the default `rust-lld`
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
- [S · 0.12.0] Two SIMD stacks are built under the engine: `imbl` 7.0.2 depends on `wide` 0.7 (and
  `safe_arch` 0.7) unconditionally beside the workspace's `wide` 1.7.1, a duplicate
  `dependencies.md`'s forced list does not name
- [S · 0.12.0] A Mac build compiles two `objc2` majors: 0.5 (with `block2`, `objc2-foundation`,
  `objc2-app-kit` 0.2) under gpui's `accesskit_macos` beside the 0.6 line `resonate-apple` takes,
  which `dependencies.md` reads as no second copy; `thiserror` 1 beside 2 under glib, `jni` and `ndk`
  is unnamed too (each wants `cargo tree -d` per target)
- [S · 0.12.0] `image` resolves with `exr`, `qoi`, `ravif` (and so `rav1e`), `tiff` and `rayon` beside
  the five formats `dependencies.md` names, unified in from gpui's tree; whether `Drawing` then
  accepts a format the vocabulary does not is unchecked (`cargo tree -e features -i image`)
- [S · 0.12.0] `same-file`, `wasapi` and `hdf5-writer` are the workspace's only dependencies written
  without `default-features = false` and a feature list (`wasapi`'s defaults bring `log` and
  `num-integer`)
- [S · blocked: gpui] `cargo audit`'s other warnings are other crates' trees: `glib` 0.18.5 unsound
  (RUSTSEC-2024-0429) under `tao`, `webkit2gtk` and wry, for `resonate-signin`'s window alone; zbus
  5 under gpui's accesskit and ashpd, beside `resonate-mpris`'s zbus 6 (two built), with
  `zvariant` and `zbus_names` unmaintained since they were merged into zbus 6; `rustybuzz` and
  `ttf-parser` under gpui's `usvg`; and the build-time `paste` (lofty), `proc-macro-error` (glib)
  and `basic-toml` (uniffi's askama)

## Docs
- [S · 0.12.0] `README.md` and `CLAUDE.md` say the macOS build has never run on a Mac, where
  `macos.md` and `FEATURES.md` record a run on an M4 under macOS 26
- [S · 0.12.0] `README.md`'s facts that no longer hold: `man 5 resonate-config` (the page is
  `resonate-config.toml(5)`), Android's SDK 36 (`compileSdk = 37`), macOS listed for Apple Silicon
  alone (`release.yml` builds an x86-64-v3 image too), and macOS called unsigned (it is signed ad hoc,
  not notarised)
- [S · 0.12.0] `android.md`'s list of the catalog's steps passes over steps 25, 27, 28 and 33
  (`PROFILES_ARE_FOLLOWED_WHOLE`, `LISTENS_ARE_READ_BY_WHEN_THEY_BEGAN`,
  `A_PLAYLIST_MAY_BE_SHARED_WITH_OTHER_PEOPLE`, `QUEUES_KEEP_THEIR_SHUFFLE_LIST_ORDER_AND_ROWS_PLAYED_NEXT`),
  and the step check does not ask that every step be named somewhere

## Later: Structure
- [M] Seven places delete `tracks` rows (`deleted.rs` twice, `store.rs`'s `prune`,
  `superseded_in_the_vault` and `forget_paths`, `db/roots.rs`'s `forget_delivered`, `organise.rs`),
  each repeating some of keeping the listens, noting `songs_deleted`, dropping `wants`,
  `sweep_orphans` and `settle_if_owed`; the listens two of them lose (`BUGS.md`) are that drift. One
  `forget_rows` every removal goes through, and a test refusing a raw `DELETE FROM tracks` elsewhere,
  would hold it
- [L] The party, inspector and services panes and the settings pane's fields are entities owning
  their state and subscriptions (`ui.md`), but `RootView` (77 fields, 133 more in its groups) still
  carries the rest: the settings pane's own state (`SettingsShown`, `Accounts`, `Remotely`), the
  browse panes' scrolls, orders and reach, and the queue, statistics, suggestions, missing, lyrics
  and analysis panes, with `RootView::new` some 470 lines making 23 subscriptions; `LibraryModel`
  (48) still holds each pane's catalog state (`Charts`, `Gaps`, `Shelf`, `Detail`, …). The missing,
  suggestions and statistics panes want a driven test pinning them before they move, and a pane's
  drawing stays on the root while its controls live in the root's focus ring
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
