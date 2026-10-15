# Features

Gaps: what is not built, a limit by design, an unsupported format or a behaviour the listener cannot
reach. Format and ordering are in [ROADMAP.md](ROADMAP.md).

## Android parity
- [M] Android does not organise its folders or bind a curve to an output; iOS does both (moves in
  Swift over `plan_organise` and `documents_moved`; curves per route in Swift)
- [M] Android cannot edit tags: `edit_tags` writes at a filesystem path, which a SAF document does
  not give
- [S] Android has no room correction: `keep_room_correction` and `apply_room_correction` keep and
  play a response, and only iOS offers them
- [S] Android sends favourites only from its button: `tell_loves` loves and takes back on its own,
  and only iOS calls it
- [M] Android has no Listen: `recognise_heard` names a clip, but nothing records one from the
  microphone
- [S] Android does not start the background lookup (`start_enrichment`), follow
  `enrichment_status` or refresh pictures through `art_changed_since`; a song is still looked up
  only where the front end asks `enrich_track`
- [L] Android output is shared stereo float at 48 kHz; exclusive/bit-perfect output, per-route
  format enumeration and DoP need capability negotiation and physical-device validation
- [M] Android keeps one download account and runs one download at a time; the boundary races
  every account and runs jobs side by side, but Settings stores one and the screens show one job
- [M] Android does not ask for its wants on its own: `poll_wanted_due` and `next_wanted_due` say
  which are due and when, and only iOS calls them on a timer
- [S] A wanted row keeps no release, disc or position, so a want downloaded from the Wanted list is
  asked with less than the search result it was saved from
- [M] Android shapes the equaliser through ten octave sliders and a preamp: an AutoEq or imported
  curve plays and is kept, but its bands are edited only on iOS's graph, and moving a slider
  replaces it with ten bands
- [S] The phones apply one curve to every channel: a filter an imported profile limits to some
  channels is passed over (and counted), where the desktop keeps it on those channels

## iOS parity
- [L] Neither phone has the vault: the managed archive is built on desktop paths
- [M · blocked: xtool] iOS has no widget: a WidgetKit extension xtool builds is linked empty and,
  linked whole, traps in `ExtensionFoundation` before a widget registers (xtool issue 313)
- [M · blocked: Apple] iOS has no CarPlay: the CarPlay audio entitlement is granted by Apple to a
  paid team on request, and a free signing cannot carry it
- [M · blocked: rust nightly] Pointer authentication (an `arm64e` slice) is not adopted: the Rust
  library builds for `aarch64-apple-ios` on stable, and `arm64e-apple-ios` needs a nightly
  `build-std`

## Playback and output
- [M] Changing the graph rate mid-track reopens the stream, costing the gap a sink switch does; so
  do the rate policy, buffer or DoP wherever the change moves the stream's format or the ring's
  depth
- [M] Choosing the sink already bound, or changing quality, filter phase, dither, restoration, noise
  shaping, convolution or forced rate on a track whose plan they leave alone, still fades out and
  reopens: only `SetBitPerfect`, `SetDop` and `SetBuffer` ask `keeps_its_stream`
- [L] Changing the equaliser is heard up to the buffer's depth later (half a second by default): the
  filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- [L] One playback stream and one capture stream only; concurrent playback streams unsupported

## Formats
- [L] A mid-stream change of rate or channel count (chained Ogg Vorbis, an MP3 going mono to stereo)
  fails the track with `ResetRequired`; nothing reopens the stream at the change
- [M · blocked: ape-decoder] A 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- [M · blocked: symphonia-codec-wavpack] A `.wvc` correction file beside a hybrid WavPack is never
  opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held zero's
  correction from the wrong range
- [L · blocked: symphonia] Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- [M] A cue-cut row is never written; an album landed as a release group gets no totals
- [M · blocked: lofty] `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Catalog
- [M] Deleting a track is `fs::remove_file`: no trash, its `.cue`, `.lrc` and emptied folder left
  behind (organise prunes its own); its listens cascade away, shrinking statistics
- [M] A file whose name is not UTF-8 is counted `Unnamed` and never catalogued

## Providers and network
- [M] A TIDAL account is asked by link and code alone: its OpenAPI filter has no text search here,
  so a song TIDAL holds under a code MusicBrainz never lists is found by hifi-api and Monochrome
  only

## Device sync
- [M] A changed sync setting, or a code joined in the pane, takes effect only at the next start
- [M] Saved-query playlists stay on the device they were saved on; only lists travel
- [S] A handoff continues from the song the other device was on, not from its place in that song
- [S] A song from another device is refused where no music folder is set; an open vault does not
  keep it
- [M] Devices punch over IPv4 alone, so two devices reachable only over IPv6 never meet
- [L] Two devices both behind symmetric NATs cannot reach each other; nothing relays songs between
  them
- [M] Devices on one network meet over IPv4 alone; a network handing out only IPv6 leaves them
  apart
- [M · blocked: resonate.tecnio.dev] The hosted sync server is not live; until it is, devices on
  different networks meet only through a self-hosted `resonate-server`

## Command line and bus
- [M] The command line cannot mark a favourite, want a track or dismiss a missing one; only the
  window and MCP can
- [M] A setting a subcommand writes (`resonate eq --profile`, `--on`, `--unbind`) is not heard by a
  running window or player, and nothing says so
- [S] MCP tools answer `structuredContent` with no `outputSchema`
- [M] `config.toml`'s 89 keys are documented nowhere a hand-editor reads; man pages cover the CLI
  alone

## Equaliser and DSP
- [M] With `true-peak` off nothing weighs what the equaliser adds: a zero preamp under boosts
  reaches the hard clamp with no count or report
- [S] AutoEq's suggestion takes the first measurer `INDEX.md` lists for a device, none ranked

## Search
- [S] A phone's search holds nothing for `added:` (a document row keeps no time it was added) nor
  for `is:fake`, `is:suspect` and `is:misnamed` (studies stay outside the index and its revision)
- [M] A pasted TIDAL playlist link is taken as words; a Spotify playlist is read off its embed page
  and a YouTube one off its public page, each listing at most its first hundred songs, and an Apple
  Music one off its public page, none naming an ISRC
- [L · blocked: service] A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- [L] An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists; no FhG encode
  weighed

## Keyboard and accessibility
- [M] The sleep timer is a mouse-down handler outside the focus ring with no key: a keyboard-only
  session cannot set one; seeking to or copying a lyric line is pointer-only too
- [M] An icon-only control shows no name on focus (tooltips need a pointer) and the confirming toast
  leaves after 6 s; `TextSize::Large` is 1.15× the normal; no reduced-motion setting
- [M] A selection is a contiguous run reached by Shift; Tracks and Albums act on its first row
  alone; nothing adds a row with Control
- [L · blocked: gpui] Nothing is exposed to a screen reader: published gpui 0.2.2 carries no
  AccessKit, which only Zed's `main` has

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
