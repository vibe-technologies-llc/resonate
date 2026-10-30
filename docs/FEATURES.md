# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M · blocked: hardware] Android offers no DoP: a marked stream needs the USB bit-perfect mixer at a
  32-bit word (Oboe-rs has no packed 24-bit one) and a DAC that decodes it to prove the markers
  arrive untouched

## iOS parity
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
  entitlement Apple grants a paid team on request
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
- [S · 0.3.0] libopus misses one quiet choral master of 36 at 64, 128 and 192 kbps even with its
  pitch post-filter undone: 2 000 bands stand over the 16-bit rounding where a loud master has
  16 000, so its lattice (0.05 to 0.14) never reaches the significance of 12; reading quieter bands
  wants a weight that does not lift the genuine sources (4.1 at most now)
- [M · blocked: encoders] The quantiser steps are weighed only on the encoders this machine has
  (ffmpeg's AAC and MP2, faac, LAME, libvorbis, ffmpeg's Vorbis, twolame and libopus; fdk AAC on an
  earlier one): no Apple or FhG AAC, no FhG or other MP3 encoder and no aoTuV Vorbis; fdk AAC at 320
  kbps missed five of 36 loud masters there with the cause not found (a power-law lattice per scale
  factor band, each band's scale factor searched, is untried without fdkaac to weigh it on)

## Later: Sources and providers
- [M] A phone counts no listen of a song played in place and draws no star on one: the phones'
  `listens` name a document alone, statistics there read nothing else, and the service's own
  favourite mark (`providers.md`'s *A song played in place is a favourite in the service's own
  mark*, the desktop's window and `resonate served`) is not reached from the boundary
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
- [S] A phone refuses a SOFA set past 64 MiB (`IMPULSE_BYTES_AT_MOST`) though the desktop reads an
  HDF5 one of up to 8 GiB by the ranges it needs: the limit is the boundary's, a document read whole
  into a Kotlin `ByteArray` or Swift `Data`, copied across the boundary and staged as a file, and
  lifting it needs a chunked staging call and a document read in chunks in both pickers; a netCDF-3
  classic file is still read whole (up to 128 MiB) on every target, and a head's pitch and roll
  (`ListenerView` past its heading) and a `ListenerPosition` in units other than metres are not read
- [M] *Fit the preamp*'s solved bound prices every node under the whole signal's at its steady
  gain, so broadband music under a curve moving many bands still bounds one to three and a half
  decibels over its decode; weighing each node by its band's transient gain (the L1 of the near
  response filtered to it) was measured too loose under a dispersive correction (a random burst of
  taps 6 to 12 dB looser), and a transient gain read from what music a band holds, not the worst
  any music could, is not built
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
- [L] A stereo behind Chromecast cannot be played to: the desktop reaches UPnP AV renderers alone
  (`resonate-upnp`, a sink beside PipeWire's); Google Cast v2 (TLS on port 8009, protobuf
  `CastMessage` framing, mDNS `_googlecast._tcp`, the Default Media Receiver loading an address
  the player serves) would be one more sink kind beside them
- [M] A renderer is reached only through SSDP's multicast search: none can be added by the address
  of its description (`Located` handed to the `Network` and held whatever the searches miss), which
  an iPhone without the multicast entitlement and a network that drops multicast would need
- [S] A UPnP renderer is sent FLAC above 96 kHz only where its protocol info states the rate
  (`audio/flac;rate=192000`); one that says nothing is offered FLAC up to 96 kHz, so a streamer
  taking FLAC alone has 176.4 and 192 kHz material resampled to 96 kHz by the engine though it
  might decode the rate, and one taking WAVE is sent uncompressed PCM, about 9 Mbit/s at 24-bit
  192 kHz. Channels beyond stereo are offered only where the protocol info states `channels=N` (a
  renderer saying nothing is folded down), and handing the untouched file over does not fit the
  sink seam (`upnp.md`)
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
