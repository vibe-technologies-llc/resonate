# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [L · 0.2.0] Android output is shared stereo float at 48 kHz; exclusive/bit-perfect output, per-route
  format enumeration and DoP need capability negotiation and physical-device validation

## iOS parity
- [M · blocked: xtool] iOS has no widget: a WidgetKit extension xtool builds is linked empty and,
  linked whole, traps in `ExtensionFoundation` before a widget registers (xtool issue 313)
- [M · blocked: Apple] iOS has no CarPlay: the CarPlay audio entitlement is granted by Apple to a
  paid team on request, and a free signing cannot carry it
- [M · blocked: rust nightly] Pointer authentication (an `arm64e` slice) is not adopted: the Rust
  library builds for `aarch64-apple-ios` on stable, and `arm64e-apple-ios` needs a nightly
  `build-std`

## Playback and output
- [M · 0.2.0] A mid-track change still reopens the PipeWire stream where it moves what the stream was
  connected with: its rate while `force-graph-rate` pins it (on by default; moving `node.rate` needs
  `pw_stream_update_properties`, which pipewire-rs does not wrap), the pin itself, or `NO_CONVERT`
  (the rate policy or DoP turning the plan between converted and not); a turn of rate inside a row
  reopens after its drain, and Oboe and RemoteIO reshape nothing
- [L · 0.2.0] Changing the equaliser is heard up to the buffer's depth later (half a second by default): the
  filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- [L · 0.2.0] One playback stream and one capture stream only; concurrent playback streams unsupported

## Formats
- [M · blocked: ape-decoder] A 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- [M · blocked: symphonia-codec-wavpack] A `.wvc` correction file beside a hybrid WavPack is never
  opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held zero's
  correction from the wrong range
- [L · blocked: symphonia] Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- [S · 0.2.0] An album landed as its release group alone gets no totals, by design: a group names no
  pressing and so no count, and a closest pressing's are withheld as a guess; only a count the
  group's pressings agree on could be written, which `settle_group` already lands as an exact fit
- [M · blocked: lofty] `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Providers and network

## Device sync
- [S · 0.2.0] A phone saves a search only in its relevance order with no row cap, and cannot revise a
  saved search's text, order or cap once made, though a revision made on the desktop arrives

## Equaliser and DSP

## Search
- [S · 0.2.0] A phone's `is:misnamed` holds only the songs a listener has had recognised one at a time
  (`recognise_track`), since a phone studies and recognises no song in the background as the desktop's
  enrichment pass does
- [M · 0.2.0] A pasted Spotify, TIDAL, YouTube or Apple Music playlist link lists only the songs its public
  page holds (TIDAL's embed page fifty, the others at most a hundred), none naming an ISRC, so each
  song is found by the strict title and artist search
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [M · 0.2.0] AAC's quantiser steps were weighed only on ffmpeg's encoder (every decode at 256 and
  320 kbps found) and faac's (18 of 31 at 160 kbps): no Apple, FhG or fdk encode weighed, and a
  lossy decode with neither a wall nor AAC's frames (a resampled AAC, an MP3 or Vorbis with no
  lowpass) still reads as lossless

## Keyboard and accessibility
- [S · 0.2.0] The Favourites shelves (strips no reach key walks) take a plain click alone: a choice
  apart cannot be made or extended there by key, so Control-click is left out until the shelves
  join the reach
- [L · 0.2.0] Nothing is exposed to a screen reader: gpui now carries AccessKit, which the views
  do not yet use

## Later: Sources and providers
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
- [M] *Fit the preamp* measures the queue's first 32 songs by decoding them on the press: a fit over
  the whole catalog would need a per-band measure kept in `track_studies`, and room correction and
  the volume stage stand outside what it runs
- [L] AutoEq is the only correction source, fetched one device at a time
- [L] Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, leaves the first second and a half of an unstudied track unextended
- [M · blocked: symphonia] A downmix folds by position, or unplaced channels in turn: a stream's own
  downmix coefficients are not read, because the only decoded stream that could carry them is a
  multichannel AAC whose program config element (`matrix_mixdown_idx`, `pseudo_surround`)
  symphonia-codec-aac 0.6.1 refuses (`aac: program config`, and more than two channels is `aac too
  complex`)

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
