# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M · blocked: hardware] Android offers no DoP: a marked stream needs the USB bit-perfect mixer at a
  32-bit word (Oboe-rs has no packed 24-bit one) and a DAC that decodes it to prove the markers
  arrive untouched

## iOS parity
- [M · 0.2.0] The iOS icon ignores the system's icon appearances: the primary `Icon` and each accent
  alternate are one flat PNG (`ios/icons.py`, the gradient and the mark baked together), so a
  home screen set to Dark, Clear or Tinted derives its variant from that flat picture and the
  mark and ground come out muddy, inverted or in colours the theme never chose. Draw the layers
  apart (the mark alone on transparency, the accent gradient as the ground), a dark variant on a
  near-black ground, and a greyscale tinted variant whose mark is the bright layer on a black
  ground, so the system's tint lands on the mark and the ground stays dark; check whether xtool
  can carry them (an `Assets.car` or Icon Composer `.icon` is Xcode's `actool`, which Linux lacks;
  the plist's `CFBundleIcons` has no appearance keys), and if it cannot, write the catalog by hand
  the way `app-intents.py` writes Siri's metadata; each accent alternate takes its own variants
- [M · blocked: xtool] iOS has no widget: a WidgetKit extension xtool builds is linked empty and,
  linked whole, traps in `ExtensionFoundation` before a widget registers (xtool issue 313)
- [M · blocked: Apple] iOS has no CarPlay: the CarPlay audio entitlement is granted by Apple to a
  paid team on request, and a free signing cannot carry it
- [M · blocked: Apple] iOS zones share one output: `AVAudioSession` sends the app to one route at
  a time, so a second zone adds a queue of its own and a handover but no second room (both zones'
  RemoteIO units mix into that route) unless it plays on a network renderer, which the next entry
  keeps from a free signing
- [M · blocked: Apple] An iPhone signed for free finds no network renderer: the SSDP search is
  multicast from the library's own socket, which needs the `com.apple.developer.networking.multicast`
  entitlement Apple grants a paid team on request; a renderer given by the address of its
  description (`add_renderer_address`) is reached without it
- [M · blocked: rust nightly] Pointer authentication (an `arm64e` slice) is not adopted: the Rust
  library builds for `aarch64-apple-ios` on stable, and `arm64e-apple-ios` needs a nightly
  `build-std`

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

## Device sync

## Equaliser and DSP

## Search
- [M · blocked: service] A pasted Spotify, TIDAL or Apple Music playlist link lists only the songs its
  public page holds (TIDAL's embed page fifty, the others at most a hundred), since none offers a keyless
  route past it (their APIs want a registered client or the listener's sign-in); Spotify's and Apple
  Music's name no ISRC either, so each of their songs is found by the strict title and artist search
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [S · 0.3.0] libopus at 64 kbps is still missed on 12 of 426 library tracks (quiet masters, a
  significance of 1.6 to 11.3 against 12) with the bands weighed by their coherence under the
  rounding; a smaller `ROUNDING_NOISE_TURNS` than 0.5 is untried, and the sources of 397 of the
  426 tracks are not yet weighed with those weights (the 29 weighed read 3.9 at most)
- [M · blocked: encoders] The quantiser steps are weighed only on the encoders this machine has
  (ffmpeg's AAC and MP2, faac, LAME, libvorbis, ffmpeg's Vorbis, twolame and libopus; fdk AAC on an
  earlier one): no Apple or FhG AAC, no FhG or other MP3 encoder and no aoTuV Vorbis; fdk AAC at 320
  kbps missed five of 36 loud masters there with the cause not found (a power-law lattice per scale
  factor band, each band's scale factor searched, is untried without fdkaac to weigh it on)

## Later: Sources and providers
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
- [M] *Fit the preamp*'s solved bound prices every node under the whole signal's at its steady
  gain, so broadband music under a curve moving many bands still bounds one to three and a half
  decibels over its decode (3.6 dB on average over eight real songs, 0.2 to 5.9); a transient
  gain per node was measured too loose under a dispersive correction, a level of its own for every
  node of the tree and finer anchor levels moved nothing, and 64 moments in place of 8 moved the
  real songs 0.3 dB (`eq.md`), so a tighter bound likely needs a figure the study does not keep, one
  that says how far apart in time the bands' peaks fall (a stored format, so a migration), not a
  better reading of the peaks
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
- [S] The phones have no row for a renderer's address: `add_renderer_address`,
  `remove_renderer_address` and `renderer_addresses` are on the boundary (`upnp.md`), and neither
  the Compose settings nor the SwiftUI zones sheet calls them, so an iPhone signed for free and a
  network that drops multicast still find nothing until a screen takes an address and keeps the list
- [S] A network that drops multicast is still searched: the `renderers` key and `sinks --renderer-at`
  send the SSDP search before reaching an address, so no listener can be rid of multicast while
  keeping a renderer by address (`upnp.md`)
- [M] A Google Cast receiver cannot be added by host and port: its identity is the mDNS `id`
  and `ca` record, which a `GET_STATUS` over TLS does not carry, so `Casting` has no counterpart of
  `Network::locate` (`cast.md`)
- [S] A renderer named in the settings pane's *High-rate FLAC to renderers* group is sent FLAC above
  96 kHz from the next start, not at once: `Network::vouch` is read when a renderer is described and
  does not re-describe one already found, so the window cannot apply a switch live (`upnp.md`)
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

## Later: Listen and recognition
- [M · blocked: Shazam] Reached through an undocumented endpoint (`amp.shazam.com`); a change on its
  side stops recognition
