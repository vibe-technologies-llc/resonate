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
- [S · 0.4.0] A phone offers its other devices only its first zone's queue to continue: sync's
  handoff reads the first zone's saved place (`DocumentCatalog::resumption` over `queue` and
  `resume`), never the zone shown, so a song handed to a second zone cannot be continued elsewhere
- [S · 0.4.0] A phone keeps no zone's volume: a zone reopens at full volume at the next start, where
  the desktop keeps each zone's volume beside its device
- [S · 0.4.0] Two Android zones are never refused one output: each zone reports a sink of its own,
  so two zones bound to one USB DAC both ask the bit-perfect mixer for it and Android decides,
  where the desktop refuses a device another zone holds bit-perfect
- [S · 0.4.0] A handover between zones is aligned on the moment each callback ran plus the latency
  its stream clock last read, not on the device's own timestamp (`pw_time.now` with its `delay`,
  Oboe's `getTimestamp`, RemoteIO's `AudioTimeStamp`), so two zones meet only as closely as a
  callback runs after its cycle began and that latency reading is true; the graph's timestamps
  would take the callback's lateness out
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
- [L] A server's library cannot be browsed or played, only wanted songs fetched: Subsonic, TIDAL,
  hifi-api and Monochrome deliver a file to keep, and the catalog is the local source's alone, though
  the playing side is ready (`Sources` opens any other provider off the engine thread under
  `OPENED_WITHIN` and `READ_WITHIN`, `Player::media` reads a queue row of any source). A Subsonic
  (Navidrome, Jellyfin's Subsonic API) source with its own catalog, browsed and played in place, is
  what a listener with a home server would reach first
- [M · blocked: services] The Bandcamp and Discogs links an `Identity` carries are read by nothing;
  no provider asks either
- [M · blocked: servers] Subsonic matches a recording id or ISRC but not the release-track id the
  inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the release
  track

## Later: The vault
- [L] A JPEG cover is kept byte for byte where its lossless JXL is larger, never smaller than it: a
  byte-exact JPEG transcode (`jxl-encoder` 0.3, AGPL, opt-in) is an unproven spike
- [L · blocked: flacenc] `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at 96
  kHz and the depth at 24 bits: a 24-bit rip loses to `flac -8` by ~15 % and is kept; a 192 kHz rip
  is never a FLAC

## Later: Equaliser and DSP extras
- [S] A SOFA HRIR set is read at the fixed speaker angles (30 degrees either side of the front, no
  elevation) from the nearest measurement of a `SimpleFreeFieldHRIR`-shaped set: another angle
  for the speakers, an interpolation between measurements, a head turned, a multi-emitter
  (`MultiSpeakerBRIR`) or netCDF-3 classic set, and a set past the phones' 64 MiB response limit
  are not read
- [M] *Fit the preamp*'s kept-figure bound still sits 2 to 6 dB over the decode for broadband
  music under a curve that moves many bands, and a band whose skirt reaches a narrow boost pays a
  little of it: each node's share of a frequency is fixed by the curve, not fitted to the song's
  peaks (solving the tone model's dual per song would let a quiet band carry a boost it borders)
- [M] *Fit the preamp*'s kept-figure bound pays a linear-phase room correction whole from the
  song's peaks, because a moment holds only a tenth of a second each way and the response's main
  lobe sits half its length later (shifting each moment by the response's delay would let the
  anchors and the nodes meet in one moment again); the fit runs the response's own taps against
  every point it weighs, about a second for a ten-second response on this machine
- [M] AutoEq is the only online correction source, fetched one device at a time; a measurement of
  the listener's own is fitted to a target on every front end, but only from a file
- [M] Lossy restoration was tuned and tested on ffmpeg's own MP3, AAC and Vorbis encodes of
  synthetic tones, walls and music-like notes at several bitrates, not on a breadth of real music
  per codec and bitrate and not on other encoders (Fraunhofer, Apple, Nero, libfdk); the thresholds
  of its short-hole reader and of the wall it follows were set on those encodes and need a corpus
  of real files to be trusted
- [M · blocked: symphonia] A downmix folds by position, or unplaced channels in turn: a stream's own
  downmix coefficients are not read, because the only decoded stream that could carry them is a
  multichannel AAC whose program config element (`matrix_mixdown_idx`, `pseudo_surround`)
  symphonia-codec-aac 0.6.1 refuses (`aac: program config`, and more than two channels is `aac too
  complex`)

## Later: Network output
- [L] Output reaches PipeWire, Oboe and RemoteIO alone: a stereo behind a network streamer (UPnP
  and OpenHome renderers, AirPlay, Chromecast) cannot be played to, though `resonate-audio::Backend`
  is the seam the engine reaches every sink through and a zone is a `Player` of its own, so a
  renderer would be one more backend and a room one more zone. Such a sink holds its own clock and
  takes a URL or a stream rather than a ring, so the plan's bit-perfect modes would not apply to it

## Later: Equaliser and DSP extras
- [M] The listening test toggles the equaliser alone: restoration, the resampler's quality and
  phase and noise shaping each rebind the stream (a gap) on a change, so they cannot be A and B of a
  test without a splice for them, and each would need its own level match (the resampler's and
  dither's are flat in level, restoration's is not)

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
