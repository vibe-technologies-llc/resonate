# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M · blocked: hardware] Android offers no DoP: a marked stream needs the USB bit-perfect mixer at a
  32-bit word (Oboe-rs has no packed 24-bit one) and a DAC that decodes it to prove the markers
  arrive untouched

## iOS parity
- [L · blocked: xtool] The iOS icon has no appearance variants of its own: Dark, Clear and Tinted
  come from a compiled `Assets.car` (an `AppIcon` set with dark and tinted renditions, or an Icon
  Composer `.icon`, whichever `actool` compiles, which only Xcode has), and xtool 1.24 turns
  `iconPath` into loose PNGs and compiles no catalog, so the system derives every look from the
  flat drawing (full-bleed, mark bright on a near-black ground, so the derived looks read; the
  tint lands on the greyscale and loses each accent's hue). A real variant set needs a catalog
  written by hand the way `app-intents.py` writes Siri's metadata (the CoreUI format is
  undocumented and nothing here can read one back without a device), each accent alternate
  carrying its own variants
- [M · blocked: xtool] iOS has no widget: a WidgetKit extension xtool builds is linked empty and,
  linked whole, traps in `ExtensionFoundation` before a widget registers (xtool issue 313)
- [M · blocked: Apple] iOS has no CarPlay: the CarPlay audio entitlement is granted by Apple to a
  paid team on request, and a free signing cannot carry it
- [M · blocked: Apple] iOS zones share one output: `AVAudioSession` sends the app to one route at
  a time, so a second zone adds a queue of its own and a handover but no second room (both zones'
  RemoteIO units mix into that route) unless it plays on a network renderer, which the next entry
  keeps from a free signing
- [M · blocked: Apple] An iPhone signed for free finds no network renderer and no Cast receiver:
  the SSDP search and the mDNS browse are multicast from the library's own sockets, which needs the
  `com.apple.developer.networking.multicast` entitlement Apple grants a paid team on request; a
  renderer given by the address of its description and a receiver given by its address or name (the
  zones sheet's *Add a renderer by address* and *Add a receiver by address*) are reached without it
- [M · blocked: rust nightly] Pointer authentication (an `arm64e` slice) is not adopted: the Rust
  library builds for `aarch64-apple-ios` on stable, and `arm64e-apple-ios` needs a nightly
  `build-std`

## Windows parity
- [M · blocked: hardware] Physical Windows devices still need WASAPI shared and exclusive playback
  at every rate and word (`windows.md` records what has run on Windows 11 under QEMU/KVM), device
  hot-plug, the render cycle's timing, microphone and loopback capture (a loopback endpoint that
  stays silent while nothing plays, a microphone's autoconverted format), the window under
  per-monitor DPI and IME, the recycle bin, the terminal keys, the window's named pipe (its
  owner-only security descriptor against another account), the ARM64 files (cross-built on an x64
  runner, never started), the installer's Start menu shortcut, the default-app selection and the
  `resonate://` link handler (whether a browser's shared or party link starts or reaches the window),
  a toast (whether one shows once a shortcut carries
  `org.resonate.Resonate`, its image from a `file:///` cover, its three buttons reaching the
  transport, a replacement when the cover lands), the connection's `NetworkCostType` (a metered
  hotspot, the change event) and the System Media Transport Controls (whether a `MediaPlayer` with its
  command manager off claims the session for an unpackaged process, the media keys and the overlay's
  buttons, seek bar, shuffle and repeat reaching the transport, the cover read through `StorageFile`,
  the title, artist and album on the overlay and the lock screen), the stream thread in MMCSS's
  `Pro Audio` class, the endpoint volume's change notification and its scalar level, the console
  tests under ConPTY, a tag write carrying NTFS alternate streams and the DACL and owner, and a toast
  shown from the zip under the process's own AppUserModelID have never run on real hardware or a
  real audio endpoint
- [S · blocked: hardware] The console tests (`tests/console.rs`) skip where `resonate sinks`
  lists no audio endpoint, which is every hosted runner: `play` answers `NoSink` to its first load
  with no device, so it leaves before the console can be driven, and a runner cannot be given an
  endpoint without a driver
- [S · blocked: hardware] The YouTube Music sign-in window (`resonate-signin`) on WebView2 is
  cross-checked only: whether Google lets the sign-in through there, whether its HttpOnly cookies
  are handed over and whether `yt-dlp` reads the jar as it did on Linux; where it fails the window
  is not offered on Windows (`providers.md`'s *Signing in to YouTube Music in a window*)
- [S · blocked: certificate] The Windows archives and installers are not Authenticode signed, so
  SmartScreen warns on a first run: no code-signing certificate exists. `sign.ps1` and `release.yml`
  sign with `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD` once they are set

## macOS parity
- [L · blocked: hardware] The macOS build has never run on a Mac: it is type-checked from Linux and
  built and tested on a hosted runner (`macos.md`). Only hardware settles hog mode (taking a
  device, moving its nominal rate and then its physical format, each read back within 1.5 s, and
  putting all three back), whether the HAL unit takes 16-bit and packed 32-bit client words and
  carries 24 bits through its Float32 path exactly, the callback's host time as the moment a buffer
  reaches the device, a device's volume and mute (element 0 or the stereo pair, and whether a
  settable scalar is the device's own), listeners for hot-plug and a rate another program moved, a
  microphone through the HAL input unit and its permission prompt, the measuring sweep, Now Playing
  (whether setting the playback state alone makes an unbundled process the Now Playing app, the
  media keys, the cover, the seek bar), the window (shaders compiled at start, the header beside the
  traffic lights and their size on macOS 26, dragging and double click on it, the bundle's fonts
  found by family, the menu's key equivalents while a field holds the caret, Control-Space against
  the system's input-source switch), files and `resonate://` links from Finder at a cold start, the
  socket a second launch reaches the window on (and its path under an unusually long home), Finder's
  Trash through `trashItemAtURL` and its *Put Back*, the machine kept awake, the language list,
  FAT and exFAT volumes named portably, the YouTube Music sign-in window on WKWebView, and the disk
  image (Gatekeeper on an ad-hoc signature, the document types and the link scheme registering,
  the icon)
- [M] macOS tells nothing of a song change: no notification is raised (UserNotifications needs a
  bundle and the listener's permission, and nothing calls it)
- [M] The desktop's sound cannot be recorded on macOS (`Listening::Desktop` is
  `Error::DesktopUnheard`, the Listen pane offers the microphone alone): a Core Audio process tap
  (macOS 14.2) is not built
- [S] The exclusive sink on macOS offers no 32-bit integer word and no layout past stereo, and asks
  for no buffer size: the HAL unit's Float32 path holds 24 bits, a true integer mode (an IOProc on
  an unmixable format) is not built, and a device of more channels plays as its preferred pair
- [S] Closing the window on macOS quits; playing on with it closed, as a Mac's players do, is not
  built, and `resonate play` answers no media key there (it runs no main loop)
- [S] There is no Homebrew cask and no Intel build: the release is one Apple Silicon disk image
- [S · blocked: certificate] The macOS disk image is signed ad hoc and not notarised, so Gatekeeper
  refuses a first run until it is opened from *Privacy & Security*: no Developer ID exists.
  `sign.sh` and `release.yml` sign and notarise once `MACOS_CERTIFICATE_BASE64`,
  `MACOS_CERTIFICATE_PASSWORD`, `MACOS_NOTARY_KEY_BASE64`, `MACOS_NOTARY_KEY_ID` and
  `MACOS_NOTARY_ISSUER` are set

## Playback and output
- [M · blocked: pipewire-rs] A rebind or turn inside a row that moves the rate while
  `force-graph-rate` pins it (on by default), or switches the pin, still reopens the PipeWire stream:
  the pin is the node's `node.rate` property, which only `pw_stream_update_properties` moves, and
  pipewire-rs 0.10.1 (and its main branch) does not wrap it; no param, `Props` key or per-node
  metadata reaches a node's properties, and the `settings` metadata's `clock.force-rate` would force
  every client's graph and outlive a crash
- [M · blocked: Oboe] A turn of rate, word or channel count inside a row (the song's own rate while
  *Match the song's sample rate* is on, or a route moving between mixed, exclusive and bit-perfect)
  still closes the Oboe stream and opens another: an open stream's rate, format and channel count
  are fixed, and the callback is typed by them

## The window

## Sync and together

## Formats
- [M · blocked: ape-decoder] A 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- [M · blocked: symphonia-codec-wavpack] A `.wvc` correction file beside a hybrid WavPack is never
  opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held zero's
  correction from the wrong range
- [L · blocked: symphonia] Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- [M · blocked: lofty] `.caf` and the DSD containers have no writer (lofty 0.25.4 has no such file
  type); `.oga` alone could be mapped to Vorbis by hand; `.mka` is written by hand (`library.md`) and
  has no way to take a picture back out

## Providers and network
- [L · blocked: Apple] An Apple Music library cannot be synced: reading a listener's library and
  playlists needs a MusicKit developer token, issued only to a paid Apple Developer membership, and
  a user token only Apple's own sign-in hands out, which neither the desktop nor Android can show;
  a public playlist's link is followed as it is

## Equaliser and DSP

## Search
- [M · blocked: service] A pasted Spotify, TIDAL or Apple Music playlist link lists only the songs its
  public page holds (TIDAL's embed page fifty, the others at most a hundred), since none offers a keyless
  route past it (their APIs want a registered client or the listener's sign-in; a Spotify account signed
  in reads its own and its collaborative playlists whole); Spotify's and Apple Music's name no ISRC either,
  so each of their songs is found by the strict title and artist search
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [M · blocked: encoders] The quantiser steps are weighed only on the encoders this machine has
  (ffmpeg's AAC and MP2, faac, LAME, libvorbis, ffmpeg's Vorbis, twolame and libopus; fdk AAC on an
  earlier one): no Apple or FhG AAC, no FhG or other MP3 encoder and no aoTuV Vorbis; fdk AAC at 320
  kbps missed five of 36 loud masters there with the cause not found (a power-law lattice per scale
  factor band, each band's scale factor searched, is untried without fdkaac to weigh it on)

## Later: Language

- [L] The first language beside English: a `Language` variant, every crate's `words/<tag>/`, Android's `values-<tag>/` and iOS's `<tag>.lproj` (`language.md`).
- [M] Numbers, dates and month names in the listener's locale: figures are not grouped, and `CivilDate`'s months and the weekdays are English words in a catalog, not the locale's.
- [M] Right-to-left languages: Fluent's isolation marks are off, and the window's layout is never mirrored.
- [M] The window speaking a newly chosen language at once, without a restart.
- [S] The CLI's help, the man pages, the desktop entry and the metainfo in the listener's language; `build.rs` writes them in English at build time.


- [M] Chat and reactions in a listening party.
- [M] Handing the host's role to someone else in a party, when the host must go.
- [M] A browser view of a shared playlist's contents; today the page only hands its link to the app.
- [S] Measuring a party's offset between two people in different places, past the bound half their round trip gives.

## Later: Sources and providers
- [M · blocked: service] A Spotify playlist the account only follows is read off its public page,
  at most a hundred songs with no ISRC and nothing where it is private: Spotify's development mode
  answers `403` on `/playlists/<id>/items` unless the account owns or collaborates on it, and extended
  quota, which lifts that, is open only to organisations with 250 000 monthly listeners
- [S · blocked: TIDAL's CDN] A TIDAL song a DASH manifest delivers in segments seeks only once the
  codec has spooled it whole: the codec seeks by byte, and the remuxed FLAC's offsets are the sum of
  every earlier segment's `mdat`, which the manifest's timeline (times alone) does not give; asking
  each segment's head with a `Range` would map them, but whether TIDAL's segment hosts answer one is
  not known from here, and without it the map costs fetching every earlier segment, as the spool
  already does
- [M · blocked: services] The Bandcamp and Discogs links an `Identity` carries are read by nothing;
  no provider asks either
- [M · blocked: servers] Subsonic matches a recording id or ISRC but not the release-track id the
  inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the release
  track

## Later: The vault
- [L · blocked: flacenc] `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at 96
  kHz and the depth at 24 bits: a 24-bit rip loses to `flac -8` by ~15 % and is kept; a 192 kHz rip
  is never a FLAC
- [M · blocked: jxl-encoder] A progressive JPEG cover, and one with a `COM` segment, stays a JPEG in
  the vault: `jxl-encoder` 0.3.4, the latest, refuses the first and rebuilds the second one byte long

## Later: Equaliser and DSP extras
- [M] *Fit the preamp*'s solved bound still sits 2.67 dB over the decode on average over twenty-three
  real songs under the bench's six curves (0.45 to 4.65; `RESONATE_BENCH_SONGS`), down from 3.55 by
  keeping what came together at one instant (`eq.md`); that figure's own rectified limit is about
  0.6 dB, and what keeps it from there is what makes it sound against a turned phase, so more
  needs a joint figure that knows the groups' phases rather than their envelopes, each group's real
  and imaginary parts apart, in a space too large for this few figures. A finer reading of the nodes
  reaching above 10 kHz, whose tone shortfall costs at most 0.63 dB of the old bound, was not measured
  again since the figures read those bands by envelope
- [M] A song built as the exact inverse of a chain of all-passes, heard through that chain, passes
  under *Fit the preamp*'s bound by 3.65 dB (`eq.md`), since each narrow node is priced at its steady
  gain and only the whole signal's at its transient gain: a chain refused the figures could be priced
  rigorously, but a dispersive correction then bounds 6 to 12 dB looser, so the price wants a test
  that tells such a song from music
- [S · blocked: hardware] The phones' *Measure* (a sweep through an `AudioTrack` or an `AVAudioPlayerNode` of its
  own beside the engine, recorded by the microphone and fitted by `fit_recorded_sweep`; `eq.md`) is
  proved on the host against a known filter, and its Kotlin on the JVM, but not heard through a real
  phone's speaker and microphone: the iOS path is built and not run, the Android path was run end to end on an
  emulator, whose virtual microphone proves the path and says nothing of the sound, and neither phone corrects its own microphone or speaker
  except by a calibration file the listener has
- [M] Lossy restoration was tuned and tested on ffmpeg's own MP3, AAC and Vorbis encodes of
  synthetic tones, walls and music-like notes at several bitrates, not on a breadth of real music
  per codec and bitrate and not on other encoders (Fraunhofer, Apple, Nero, libfdk); the thresholds
  of its short-hole reader and of the wall it follows were set on those encodes and need a corpus
  of real files to be trusted
- [S · blocked: service] AutoEq is the only online correction source. The one keyless public source of measurements
  found, squig.link (`/data/phone_book.json` and REW text per device, the CrinGraph tool's
  convention every squig site copies), has no documented API or terms and names no rig or
  compensation, so a correction fitted toward a target from it would be a guess about the rig
  (`eq.md`); a download of its text already fits through `--fit-measurement`
- [M · blocked: symphonia] A downmix folds by position, or unplaced channels in turn: a stream's own
  downmix coefficients are not read, because the only decoded stream that could carry them is a
  multichannel AAC whose program config element (`matrix_mixdown_idx`, `pseudo_surround`)
  symphonia-codec-aac 0.6.1 refuses (`aac: program config`, and more than two channels is `aac too
  complex`)

## Later: Network output
- [S] Nothing learns which renderers decode 176.4 and 192 kHz FLAC, since the protocol info does not say
  and a renderer that cannot play a stream gives no answer a try could read, so the default stays 96 kHz
  (the fake renderer's tests are all the proof the vouched rates have, no streamer having confirmed any);
  one taking WAVE is sent uncompressed PCM, about 9 Mbit/s at 24-bit 192 kHz. Channels beyond stereo are
  offered only where the protocol info states `channels=N` (a renderer saying nothing is folded down),
  and handing the untouched file over does not fit the sink seam (`upnp.md`)
- [S] A Google Cast receiver is offered stereo FLAC up to 96 kHz alone, the Default Media Receiver's
  published support: Cast asks no device what it decodes, so a TV or soundbar with Cast built in
  has surround folded down to stereo and anything above 96 kHz resampled (`cast.md`)
- [L · blocked: Apple] AirPlay 2 cannot be played to: its pairing, encryption and buffered audio
  are Apple's own, with no published specification to build against

## Later: Lyrics
- [L · blocked: gpui] The sung line cannot grow as it lights: gpui 0.2.2 on Linux draws a glyph on a
  whole pixel vertically (`SUBPIXEL_VARIANTS_Y = 1`) and cosmic-text hints every size, so a type
  size in motion shimmers and hops; the line only brightens
- [M · blocked: format] Where a sheet gives a line no end (a plain LRC never; only an enhanced one
  with a closing stamp, or a Lyricsfile), when it goes out is guessed from its text length: a held
  note can be put out under the dots early
- [L · blocked: sources] `Lyrics` holds no translation beside the original. Only an LRC line marked
  `[v1:]` or `[v2:]` names its singer; elsewhere a second voice is read off overlapping lines alone
  and a third is folded onto the two

## Later: Identification
- [M] The local copy of MusicBrainz is brought up to date only by a whole new export, at most once
  a month: MusicBrainz's hourly replication packets (`replication-sequence`, the
  `mbdump-*.tar.bz2` change files) could keep it within the hour, applied to the documents they
  touch (`musicbrainz.md`)
- [S] A phone cannot keep a copy of MusicBrainz of its own: the build wants some 50 GB of room, a
  gigabyte of memory and an hour; it asks a desktop holding one instead (`musicbrainz.md`)
- [S] libopus at 64 kbps is still missed on 4 of 464 library tracks with the band
  energies and the intensity-stereo coupling together: two loud masters whose decode clips (the
  coupling skips a clipped frame, so too few frames are left), a quiet, sparse track with 78 cells
  over the rounding, and one whose highs are near mono in the source itself (coupling 0.42 against
  a control of 0.33, a significance of 9.1 against 12) (`analysis.md`)

## Later: Listen and recognition
- [M · blocked: Shazam] Reached through an undocumented endpoint (`amp.shazam.com`); a change on its
  side stops recognition
