# Bugs

Defects: shipped behaviour that is wrong, lossy, unsafe or slower than it claims. Format and
ordering are in [ROADMAP.md](ROADMAP.md).

## Device sync

## Android

## Catalog

## Tagging

## Playback and output

## Formats
- [M · blocked: symphonia] A file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- [M · blocked: opus-rs] A stereo Opus stream's mono packets (libopus drops to mono near 16-24 kbps)
  go to a second `OpusDecoder` (`opus.rs` `other`) whose state is minutes stale or empty, and the
  main decoder misses them: a click, and band energies mispredicted for some frames, at every
  mono/stereo switch of a low-bitrate podcast; `opus-rs` 0.1.37 refuses a packet whose TOC channel
  bit differs from the decoder's (`Channel count mismatch`) and keeps no stream-channel state a
  mono packet could be decoded against, so the fix is a decoder that switches (upstream or vendored)

## Binary and desktop
- [S · 0.1.1] `file:/music/a.flac` (the single-slash form of RFC 8089, which Java's `File.toURI`
  writes) is not a local file: `MediaLocation::from_uri` takes only the `file://` prefix, so the
  URI falls to the source branch as a key on a source named `file`, and `resonate play`, `queue`
  and `OpenUri` fall back to the literal relative path and fail as a missing file. Run on
  `from_uri`
- [S · 0.1.1] MPRIS `xesam:contentCreated` can carry a date that does not exist: `iso_8601` checks
  the day against 1 to 31 for every month, so a `DATE` of `2021-02-30` is sent as
  `2021-02-30T00:00:00Z` where `mpris.md` says an out-of-range day falls to the first, and a strict
  client drops the date

## Providers and network
- [S · 0.1.1] A `Retry-After` in the asctime form with a huge or negative year panics on integer
  overflow in dev and test builds: the `[_, month, day, clock, year]` arm of `waited.rs` takes any
  `i64`, and `CivilDate::day_since_the_epoch` multiplies unchecked; a release build wraps and the
  callers clamp the wait, so only the thread of a dev build dies. Reproduced

## iOS
- [S · 0.1.1] A download whose title holds `? : * " < > |` cannot be created in a folder on exFAT
  or SMB: `file_stem_of` (`downloads.rs`) replaces control letters, `/` and `\` alone while the sync
  landing strips all nine (`UNFILEABLE`), so `Folders.fresh` fails on `open` with a refusal that
  only says the song could not be created. Read, not run
