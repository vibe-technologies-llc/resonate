# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M · 0.2.0] Android does not organise its folders or bind a curve to an output; iOS does both (moves in
  Swift over `plan_organise` and `documents_moved`; curves per route in Swift)
- [M · 0.2.0] Android cannot edit tags: `edit_tags` writes at a filesystem path, which a SAF document does
  not give
- [L · 0.2.0] Android output is shared stereo float at 48 kHz; exclusive/bit-perfect output, per-route
  format enumeration and DoP need capability negotiation and physical-device validation
- [M · 0.2.0] Android keeps one download account: the boundary races every account, but Settings stores one
  and the screens show one

## iOS parity
- [L · 0.2.0] Neither phone has the vault: the managed archive is built on desktop paths
- [M · 0.2.0] iOS downloads cannot resume: any cancel, failure or background expiry deletes the partial
  file and the retry starts again, a large FLAC included
- [M · blocked: xtool] iOS has no widget: a WidgetKit extension xtool builds is linked empty and,
  linked whole, traps in `ExtensionFoundation` before a widget registers (xtool issue 313)
- [M · blocked: Apple] iOS has no CarPlay: the CarPlay audio entitlement is granted by Apple to a
  paid team on request, and a free signing cannot carry it
- [M · blocked: rust nightly] Pointer authentication (an `arm64e` slice) is not adopted: the Rust
  library builds for `aarch64-apple-ios` on stable, and `arm64e-apple-ios` needs a nightly
  `build-std`

## Playback and output
- [M · 0.2.0] Changing the graph rate mid-track reopens the stream, costing the gap a sink switch does; so
  do the rate policy, buffer or DoP wherever the change moves the stream's format or the ring's
  depth
- [L · 0.2.0] Changing the equaliser is heard up to the buffer's depth later (half a second by default): the
  filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- [L · 0.2.0] One playback stream and one capture stream only; concurrent playback streams unsupported

## Formats
- [L · 0.2.0] A mid-stream change of rate or channel count (chained Ogg Vorbis, an MP3 going mono to stereo)
  fails the track with `ResetRequired`; nothing reopens the stream at the change
- [M · blocked: ape-decoder] A 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- [M · blocked: symphonia-codec-wavpack] A `.wvc` correction file beside a hybrid WavPack is never
  opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held zero's
  correction from the wrong range
- [L · blocked: symphonia] Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- [M · 0.2.0] A cue-cut row is never written; an album landed as a release group gets no totals
- [M · blocked: lofty] `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Providers and network
- [M · 0.2.0] A TIDAL account is asked by link and code alone: its OpenAPI filter has no text search here,
  so a song TIDAL holds under a code MusicBrainz never lists is found by hifi-api and Monochrome
  only

## Device sync
- [L · 0.2.0] The phones carry saved-query playlists on to other devices but hold none themselves: their
  catalog has no saved queries and no search grammar to read one with
- [S · 0.2.0] A build without the saved-query record that compacts the server's log drops the saved queries
  from it; a device joining from the server alone misses them until one holding them edits them
- [S · 0.2.0] A song a vaulted device passes on has lost its tags to the vault, so the third device groups
  it by title and track artist alone unless the sender's file declared more; the wire's
  `Recording` carries no album artist, year or totals
- [M · 0.2.0] Devices on one network meet over IPv4 alone; a network handing out only IPv6 leaves them
  apart

## Equaliser and DSP
- [S · 0.2.0] The clamped-sample count is in the desktop inspector alone: the phones' `PlaybackState` does
  not carry it

## Search
- [S · 0.2.0] A phone's search holds nothing for `added:` (a document row keeps no time it was added) nor
  for `is:fake`, `is:suspect` and `is:misnamed` (studies stay outside the index and its revision)
- [M · 0.2.0] A pasted TIDAL playlist link is taken as words; a Spotify playlist is read off its embed page
  and a YouTube one off its public page, each listing at most its first hundred songs, and an Apple
  Music one off its public page, none naming an ISRC
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [L · 0.2.0] An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists; no FhG encode
  weighed

## Keyboard and accessibility
- [M · 0.2.0] An icon-only control shows no name on focus (tooltips need a pointer) and the confirming toast
  leaves after 6 s; `TextSize::Large` is 1.15× the normal; no reduced-motion setting
- [M · 0.2.0] A selection is a contiguous run reached by Shift; Tracks and Albums act on its first row
  alone; nothing adds a row with Control
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
- [M] *Fit the preamp* models the curve rather than measuring what the music peaks at
- [L] AutoEq is the only correction source, fetched one device at a time
- [M] A downmix folds by position, or unplaced channels in turn: a stream's own downmix coefficients
  are not read
- [L] Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- [M] A partly timed LRC drops its untimed lines (section heads, credits, translations) unsaid:
  placing one needs a moment that neither lights it nor ends the timed line beside it early
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

## Later: MCP
- [L] An edit a model makes is not on the window's *Undo*: undo stacks live in the process that made
  it
