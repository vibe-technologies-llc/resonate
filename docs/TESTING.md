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

- [S · blocked: hardware] Android's unmixed output (`follow-song-rate`) is proved on the host and by
  the emulator's mixed fallback alone: no USB DAC or MMAP-capable phone is attached here, so neither
  `setPreferredMixerAttributes` taking a format, an exclusive open at the song's rate, nor the bytes
  reaching a DAC unaltered has been heard

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
- [S · blocked: iPhone] `resonate-apple`'s `AudioToolbox` (the unsafe `AudioComponentInstanceNew`,
  `AudioUnitSetProperty`, `AudioOutputUnitStart` and sibling calls) and `render`'s `extern "C-unwind"`
  shells are compiled for Apple targets alone; the unit's sequence, the output thread and the panic
  catch are proved on the host over fakes, so what no test has run is Apple's answer to the calls
  (that the format is accepted, the callback is called with the `Rendering`, a stop disposes
  cleanly), and the RemoteIO output has not been heard on an iPhone since those calls moved out of
  `resonate-mobile`
- [M · blocked: hardware] The Core Audio tests (`resonate-coreaudio --test stream`: a tone through
  the shared sink, a hog-mode rate switch and its restoration, a 24-bit ramp read back bit for bit
  through a loopback device) need an output device, and the hog and bit-exact halves a device named
  *BlackHole*; they skip without one, so they have run nowhere
- [S · blocked: Xcode's processor] `ios/app-intents.py` is held by a fixture of `Siri.swift`'s const
  values and its own output, never by Apple's: it was compared with `appintentsmetadataprocessor`
  by hand, on another xtool app's fixtures (a plain intent, a `String` parameter, App Shortcuts),
  and `AudioPlaybackIntent` as `AudioStarting` was never set beside Apple's output for it. The
  processor is an arm64 Mach-O
- [S · blocked: iPhone] Now Playing's motion is proved by its rules in tests and a clean build, never
  seen on an iPhone: the handover (cover crossfade, words, the held title height), the play and
  pause symbol replacing, the shuffle and repeat flips, the progress bar's flow, seek and song
  change, the mode change's pane fade, each under Reduce Motion, in the one column and in the two
  columns turned on its side, and the defects only a device shows (a dropped frame in a handover,
  the cover's decode landing late)
- [S · blocked: iPhone] The Settings pages and the equaliser page are proved by their rules in
  tests and a clean build, never run on an iPhone: a swipe that starts off a handle scrolling the
  form while one that starts on it drags the band, overlapping octave handles taken by the one on
  top, the band card fitting under the graph on one screen, the alert asking for an exact
  value, a settings stack kept by an earlier build reopening, and VoiceOver on the handles and the
  sliders
- [S · blocked: iPhone] The playing row's bars as layers, the lazy shelves and the filter sheet's
  name lists are proved by their rules, the bars' geometry tests and a clean build, never seen on an
  iPhone: the bars following the song and settling on a pause, their ink over a cover and in place
  of a number in both appearances, a shelf row holding its height as cards arrive on Home, Library
  and Search, the mix shelf's reserved detail line, a pushed name list popping on a choice, and a
  renderer address added while a song plays

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
- [S · 0.8.1] Two of the window's driven tests over rows chosen apart
  (`deleting_the_rows_chosen_apart_asks_once_for_all_of_them`,
  `dragging_a_playlist_row_chosen_apart_gathers_every_chosen_row_where_it_drops`) fail about one
  run in eight of the whole `resonate-ui` suite and pass alone, so something they read is shared
  with a test running beside them. The likely share is the theme: `theme::WORN` is one for the
  process, the theme tests guard it with a `WEARING` lock of their own while every `Driven` wears
  the default without it (`driven.rs`) and one wears a 1.5 text scale, so a theme test wearing
  Larger between a test reading a row's place and clicking it moves every row (`theme.rs`); a lock
  for the whole crate held by `Driven` for its life would settle it
- [S · 0.8.1] No fuzz target reaches the parsers facing the network: SSDP (`ssdp.rs`), a
  renderer's description and chunked bodies (`description.rs`, `http.rs`), Cast's protobuf and JSON
  (`frame.rs`, `json.rs`), and the server's page, gateway head and `Range` readers (`pages.rs`,
  `gateway.rs`, `ranges.rs`); `fuzz/Cargo.toml` reaches none of `resonate-upnp`, `resonate-cast` or
  `resonate-server`
- [S · 0.8.1] The no-comment rule is checked in `.ftl` alone (`resonate-words-build`); nothing
  refuses a comment in `.rs`, `.kt`, `.swift` or the workflows, and `rust-formatter --check` cannot
  run in CI, so the strictest rule of style rests on review. A walk beside `prose_in`, which already
  knows a string literal from code, would hold it
- [M · 0.8.1] `resonate-win32`'s unsafe calls (`endpoint.rs`, `console.rs`, `security.rs`,
  `streams.rs`, `gaming.rs`, some 43 sites) have four tests, where `resonate-apple` runs its
  sequencing against a fake `Toolbox` on every host; the Win32 crate has no such seam
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
- [S · 0.8.1] `resvg` 0.49 (`text`, `system-fonts`) is taken for the Wrapped card alone
  (`wrapped_card.rs`) beside gpui's 0.46, building a second SVG and font stack (`usvg`, `fontdb`
  0.24, `harfrust` 0.12, `skrifa`) and loading a second system font database at run time;
  `dependencies.md` takes gpui's version of a crate elsewhere and gives this one no reason
- [S · blocked: hdf5-reader] `cargo audit` calls `lru` 0.16.4 unsound (RUSTSEC-2026-0253, fixed from
  0.18.2), reached through `hdf5-reader` 0.12.1, the latest, under `resonate-codec`'s SOFA reader;
  `hdf5-reader` never calls the `LruCache::pop` the advisory names
- [S · blocked: gpui] `cargo audit`'s other warnings are other crates' trees: `glib` 0.18.5 unsound
  (RUSTSEC-2024-0429) under `tao`, `webkit2gtk` and wry, for `resonate-signin`'s window alone; zbus
  5 under gpui's accesskit and ashpd, beside `resonate-mpris`'s zbus 6 (two built), with
  `zvariant` and `zbus_names` unmaintained since they were merged into zbus 6; `rustybuzz` and
  `ttf-parser` under gpui's `usvg`; and the build-time `paste` (lofty), `proc-macro-error` (glib)
  and `basic-toml` (uniffi's askama)

## Docs and agents
- [S · 0.8.1] `README.md` is wrong in four places: it says no release is published (the tags reach
  v0.8.0); it builds with `cargo build --release --workspace`, which turns on `resonate-signin`'s
  `window` feature and links WebKitGTK into the player (`dependencies.md`); it lists the binary's
  default features without `sync`; and it says media keys, notifications, capture, Discord and the
  window's remote control are not on Windows, which `windows.md` describes as shipped
- [S · 0.8.1] `dependencies.md`'s lists of who takes a crate are stale: `proptest` is
  `resonate-sync`'s too, `serde` and `serde_json` `resonate-ytmusic`'s, `rustfft`
  `resonate-codec`'s (`sofa.rs`), `sha2` `resonate-musicbrainz`'s, `uuid` `resonate-online`'s and
  `toml_edit` `resonate-server`'s
- [S · 0.8.1] `language.md` leaves out `resonate-coreaudio`'s prose check; CLAUDE.md's *CI runs
  exactly this list* is not so (the benches, nextest and the profiling build are not in `ci.yml`);
  `AGENTS.md`'s checks leave out the bus tests under `dbus-run-session`, `android/check-release.py`
  and the iOS Python checks
- [S · 0.8.1] `docs/superpowers/` keeps four plans and two specs of work since landed (the YouTube
  Music provider and its sign-in window, Jellyfin Quick Connect, Last.fm's browser approval), each
  opening on a directive to an agent and citing a `.superpowers/` file that does not exist, against
  `AGENTS.md`'s *no plan files*; what they hold beyond the rules wants moving into `providers.md` and
  `online.md`, and the folder goes
- [S · 0.8.1] `.claude/settings.json` allows `cargo build`, `test`, `clippy` and `tree` but none of
  the other routine checks CLAUDE.md lists (`dbus-run-session -- cargo test`, `cargo nextest run`,
  `cargo xwin check`, `cargo bench`, `python3 -m unittest discover -s ios`, `ios/check-build.py`,
  `ios/test-logic.py`, `android/check-release.py`), so each asks first

## Later: Structure
- [L] `LibraryModel` holds 174 fields and 357 methods in a 9 423-line `models.rs`, and `RootView`
  215 fields with a 671-line `new` making 39 subscriptions (`root.rs`), the two files changed most
  since August, and every pane's feature passes through both: panes as entities owning their fields
  and subscriptions, once the `driven.rs` harness pins what they do
- [L] The phones' enrichment pass copies the desktop's orchestration: `resonate-mobile`'s
  `enrichment.rs` (4 100 lines: `Sought`, `Pictures`, `Pass`, the lyric reader, and constants such as
  `PICTURES_WAITING_AT_MOST` and `PERFORMERS_WEIGHED_AT_MOST`) beside `resonate-library`'s
  `enrich.rs`; the strict rules are shared, the queueing, backoff and stopping are not, so a fix to
  one is made twice. One pass over a small catalog trait both catalogs fill

## Later: Packaging
- [S · blocked: tooling] The sync server's RPM spec has never run: no `rpmbuild` here, and its
  workflow job (`fedora-server-rpm`) fires on a release or a manual dispatch alone

## Later: Together on the phones

- [S] Trying Wrapped, shared playlists and parties on a real iPhone: they build clean and their logic is tested, but the views have never run.
- [S] On Android, driving what the emulator run did not reach: the online-off banner, a list's songs not held, Remove in the sharing sheet, Paste a link, saving a typed name, leaving by dismissing the task and from the notification, and the service held through a pause.
- [S] A party between two real devices in different places, measuring the offset by ear or by loopback.
