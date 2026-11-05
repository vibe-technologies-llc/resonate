# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M · 0.5.0] Android's *Made for you* is four fixed mixes: the desktop's lists read from listening
  (*On repeat*, *More like*, *deep cuts*, *Discover*, *Old flames*) need a filter or saved search over
  the phones' own catalog, and the daily turn with them
- [M · blocked: hardware] Android offers no DoP: a marked stream needs the USB bit-perfect mixer at a
  32-bit word (Oboe-rs has no packed 24-bit one) and a DAC that decodes it to prove the markers
  arrive untouched

## iOS parity
- [M · 0.5.0] iOS's *Made for you* is six fixed mixes and one of the top artist: the desktop's lists
  read from listening (*On repeat*, *More like*, *deep cuts*, *Discover*, *Old flames*) need a filter or
  saved search over the phones' own catalog, and the daily turn with them
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
- [M · blocked: hardware] Nothing of the Windows build has been heard or seen on a Windows machine:
  it is cross-checked (`cargo xwin`), its portable halves are tested on Linux and a hosted runner
  builds and tests it, but WASAPI shared and exclusive playback at every rate and word, device
  hot-plug, the render cycle's timing, microphone and loopback capture (a loopback endpoint that
  stays silent while nothing plays, a microphone's autoconverted format), the window under
  per-monitor DPI and IME, the recycle bin, the terminal keys, the window's named pipe (its
  owner-only security descriptor, a client reaching it), the ARM64 files (cross-built on an x64
  runner, never started), the installer (compiled by Inno Setup for the first time at a release; its
  Start menu shortcut, file associations, the `resonate://` link handler and uninstall: whether a click on a shared or party link in a browser starts or reaches the window), a toast (whether one shows once a shortcut carries
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
- [S · blocked: certificate] The Windows archives and installers are not Authenticode signed, so
  SmartScreen warns on a first run: no code-signing certificate exists. `sign.ps1` and `release.yml`
  sign with `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD` once they are set

## macOS parity
- [L · blocked: hardware] There is no macOS desktop build: GPUI draws natively there and the iOS
  RemoteIO output is close to a CoreAudio sink beside PipeWire's and WASAPI's, but no Mac exists
  here to hear it on, so it would ship as Windows did, cross-checked and never played

## Playback and output
- [M · 0.5.0] Driving another device's zones (`sync.md`'s *One device drives another's zones*)
  stops short in four places: a phone cannot be driven (the boundary hands its `Sync` no
  `Controlled`, so a phone answers `Uncontrollable`; its `Zones` needs the binary's `Driven`
  reading over the boundary's `Playing`), the window and both phones drive transport and volume
  alone (seek, jump, move and remove are on the wire and in `resonate sync --control`, not yet
  drawn), `sync-control` has no row in the settings pane or on the phones, and `Driven` is held
  only by the sync crate's loopback test over a stand-in, never over a real engine
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

## Formats
- [M · blocked: ape-decoder] A 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- [M · blocked: symphonia-codec-wavpack] A `.wvc` correction file beside a hybrid WavPack is never
  opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held zero's
  correction from the wrong range
- [L · blocked: symphonia] Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- [M · blocked: lofty] `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Providers and network
- [L · blocked: Apple] An Apple Music library cannot be synced: reading a listener's library and
  playlists needs a MusicKit developer token, issued only to a paid Apple Developer membership, and
  a user token only Apple's own sign-in hands out, which neither the desktop nor Android can show;
  a public playlist's link is followed as it is

## Equaliser and DSP

## Search
- [M · blocked: service] A pasted Spotify, TIDAL or Apple Music playlist link lists only the songs its
  public page holds (TIDAL's embed page fifty, the others at most a hundred), since none offers a keyless
  route past it (their APIs want a registered client or the listener's sign-in); Spotify's and Apple
  Music's name no ISRC either, so each of their songs is found by the strict title and artist search
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [M · blocked: encoders] The quantiser steps are weighed only on the encoders this machine has
  (ffmpeg's AAC and MP2, faac, LAME, libvorbis, ffmpeg's Vorbis, twolame and libopus; fdk AAC on an
  earlier one): no Apple or FhG AAC, no FhG or other MP3 encoder and no aoTuV Vorbis; fdk AAC at 320
  kbps missed five of 36 loud masters there with the cause not found (a power-law lattice per scale
  factor band, each band's scale factor searched, is untried without fdkaac to weigh it on)

## Later: Together

- [M] Chat and reactions in a listening party.
- [S] Asking before a party's link takes over playback, as pairing asks before joining devices; a link joins at once today.
- [S] A followed list told apart on the phones while sync is off, so its edit controls hide instead of answering with a refusal.
- [S] A party on the phones reaching the listener's own server while sync is off; the boundary learns the server only as sync starts.
- [M] Handing the host's role to someone else in a party, when the host must go.
- [S] A frozen snapshot of a past Wrapped, kept once its year is over, rather than read afresh from what history keeps.
- [M] A browser view of a shared playlist's contents; today the page only hands its link to the app.
- [S] A guest's queue kept current as the host edits past the songs found ahead, rather than found again on reaching them.
- [S] Measuring a party's offset between two people in different places, past the bound half their round trip gives.

## Later: Sources and providers
- [L] A Spotify account cannot be synced: sign in with an app the listener registers at
  developer.spotify.com (its client ID typed into a setting, empty by default, as Last.fm's key is)
  through an authorisation-code sign-in with PKCE like TIDAL's, its refresh token kept in the
  settings; then read every playlist the account owns or follows, private ones included, and the
  liked songs through the Web API with each song's ISRC (past the embed page's hundred songs and
  its strict name search), and either keep them in step or move them to Resonate once. CLI,
  window, MCP and both phones
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
- [S] libopus at 64 kbps is still missed on 4 of 464 library tracks with the band
  energies and the intensity-stereo coupling together: two loud masters whose decode clips (the
  coupling skips a clipped frame, so too few frames are left), a quiet, sparse track with 78 cells
  over the rounding, and one whose highs are near mono in the source itself (coupling 0.42 against
  a control of 0.33, a significance of 9.1 against 12) (`analysis.md`)

## Later: Listen and recognition
- [M · blocked: Shazam] Reached through an undocumented endpoint (`amp.shazam.com`); a change on its
  side stops recognition
