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
  RemoteIO units mix into that route)
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
- [M · 0.3.0] The quantiser steps were weighed only on the encoders this machine has (ffmpeg's
  AAC, faac, fdk AAC, LAME, libvorbis and ffmpeg's Vorbis): no Apple or FhG AAC, no FhG or other
  MP3 encoder and no aoTuV Vorbis weighed; fdk AAC at 320 kbps and faac at `-q 100` still miss
  five to seven of 36 loud masters, whose lattice reads 0.008 at most (the cause is not found; at
  320 kbps large quantised values thin a log lattice, a power-law lattice per scale factor band
  would need each band's scale factor searched); a lossy decode downsampled below the rate it was coded at loses its grid; MP2 (twolame) is not
  weighed, its subband samples sitting on a linear grid per scale factor; and an Opus decode is
  only `Suspect`: CELT codes a band's energy as a residual from a prediction on the previous
  frame's, so the energies sit on no lattice of their own (a direct search of 960-line frames at
  every offset found none), and weighing the residual wants libopus's mean table, post-filter and
  stereo state

## Later: Sources and providers
- [L] TIDAL, hifi-api and Monochrome are played in place on the desktop alone (the window's Server
  pane and `tidal:`, `hifi-api:`, `resonate:`, `monochrome:` URIs): the phones' Server tab and
  `serve_from` still reach Subsonic only, and `resonate subsonic` is the only command that browses
  at the terminal
- [M] A song played in place is not a library row, so it is neither favourited, counted as a listen
  nor kept in a saved queue
- [M] TIDAL and hifi-api list nothing until searched: the account's favourite artists, albums and
  mixes are not offered as the pane's first page
- [S] A TIDAL song a DASH manifest delivers in segments seeks only once the codec has spooled it
  whole, since its remuxed byte offsets are unknown until every earlier segment is fetched
- [M · blocked: services] The Bandcamp and Discogs links an `Identity` carries are read by nothing;
  no provider asks either
- [M · blocked: servers] Subsonic matches a recording id or ISRC but not the release-track id the
  inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the release
  track

## Later: The vault
- [S] A JPEG cover kept as a JPEG before the transcode (`vault.md`) is not weighed again: a vault
  made by an older build keeps every one at its larger size until it is pruned and imported afresh
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
- [L] The phones reach no network renderer at all, though their zones open over the same `Backend`
  seam the desktop's renderers sit behind
- [M] An equaliser, ReplayGain or listening-test change reaches a UPnP renderer's ear only after
  what it already holds (seconds on many streamers): a seek, a pause and the volume skip that
  buffer (a fresh address after `Stop`, the renderer's own `Pause`, its own RenderingControl), but
  whatever is rendered into the ring plays out behind it. OpenHome's own `Playlist` and `Volume`
  services are not spoken (an OpenHome player is reached through the AVTransport upmpdcli puts in
  front of it)
- [S] A UPnP renderer is fed PCM alone: WAVE or L16 at 16 or 24 bits, stereo, so a multichannel
  source is folded down and a renderer taking FLAC is still sent uncompressed PCM, about 9 Mbit/s at
  24-bit 192 kHz; encoding the ring to FLAC on the way out is not done, and handing the untouched
  file over does not fit the sink seam (`upnp.md`)
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
