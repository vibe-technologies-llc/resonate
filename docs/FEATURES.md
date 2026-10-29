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
- [L] A Subsonic server is browsed and played in place only from the command line: the window has
  no pane to browse it (`resonate subsonic` lists, `play`/`queue` take its URIs, and a window launched
  or queued onto plays them, but nothing in the window lists the server), the phones reach it only
  to fetch wanted songs, its songs carry no cover beyond what the stream holds, and TIDAL, hifi-api
  and Monochrome are still only asked for a file to keep
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
- [S] A SOFA HRIR set is not read for a head turned, for a `MultiSpeakerBRIR` measured with the
  listener off the origin (`ListenerPosition` is ignored), from a netCDF-3 64-bit data file (CDF-5)
  or past the phones' 64 MiB response limit; the interpolation triangulates the sixteen nearest
  measurements within 30 degrees and is a true interpolation only where one triangle of them
  surrounds the speaker, so a set measured at one elevation alone, or asked for a direction past
  its outermost row, falls back to the inverse-distance blend of up to three measurements within 15
  degrees, and a set of speakers (`MultiSpeakerBRIR`) takes its nearest emitter and interpolates
  nothing; the interpolation blends in the time domain with the onsets aligned, not magnitude and
  delay apart, which a coarse set (a measurement every thirty degrees) smears
- [M] *Fit the preamp*'s solved bound leaves the whole signal's node to the level alone: with it,
  the tone model's solution passed the decode of transients through a phase-turning curve (an
  allpass pair over noise and over a kick, hat and pads, by up to 0.38 dB), so broadband music
  still bounds one to three and a half decibels over its decode; weighing each node by its own
  transient gain over its band (the L1 of the near response filtered to it) would let the whole
  node back in
- [M] *Fit the preamp*'s ring, what the chain's response holds outside the fifth of a second around
  its main lobe, is still weighed on the 2 048 points with the curve's fixed shares: an echo or a
  reverberant tail longer than that window has comb teeth between the points, which only the
  shares' slack covers (the near window is an envelope over every bin)
- [M] A measurement of the listener's own is taken live by `resonate eq --measure` and by
  *Measure* in the window's Equaliser pane, and the phones still fit a measurement from a file
  alone: each has a microphone path for Listen (Kotlin's `AudioRecord`, Swift's `AVAudioEngine`
  input) but neither has a playback of its own beside the engine to send a sweep through past the
  equaliser and the room correction in force (through the engine's output they would colour it),
  and the boundary has no call fitting a recorded sweep; the window's *Measure* cannot be stopped
  part way (eight seconds of sweep and two of tail) and takes no calibration file (the command's
  `--calibration`) nor a name for the profile (it keeps `measured`)
- [S] AutoEq is the only online correction source. The one keyless public source of measurements
  found, squig.link (`/data/phone_book.json` and REW text per device, the CrinGraph tool's
  convention every squig site copies), has no documented API or terms and names no rig or
  compensation, so a correction fitted toward a target from it would be a guess about the rig
  (`eq.md`); a download of its text already fits through `--fit-measurement`
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
