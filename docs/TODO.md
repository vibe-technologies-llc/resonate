# Roadmap

Open work only. Categories run most to least important; items likewise (prerequisite before
dependent, easier first among equals). `Later:` = nice-to-have no listener waits on.
**Blocked on …** = waits on the named outside thing (hardware, upstream crate, service, format),
sits last in its category, not worked until it moves. Everything else is open.

## Playback and output
- Changing the graph rate mid-track reopens the stream, costing the gap a sink switch does; so do
  the rate policy, buffer or DoP wherever the change moves the stream's format or the ring's depth
- Changing the equaliser is heard up to the buffer's depth later (half a second by default): the
  filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- One playback stream and one capture stream only; concurrent playback streams unsupported
- **Blocked on hardware:** no forced graph rate change proved against hardware (the only card here
  offers 48 kHz alone), nor DoP against a DAC that decodes it

## Formats
- **Blocked on `ape-decoder`:** a 32-bit stereo Monkey's Audio, integer or float, is refused:
  `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- **Blocked on `symphonia-codec-wavpack`:** a `.wvc` correction file beside a hybrid WavPack is
  never opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held
  zero's correction from the wrong range
- **Blocked on symphonia:** a file embedding a huge picture still costs one materialisation:
  symphonia 0.6.1 reads it into its own buffer before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` is honoured by no reader
- **Blocked on symphonia:** Opus mapping families 2, 3 and 255 are refused by its `OpusHead` reader;
  families 2 and 3 would also need projection decoding

## Tagging
- On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name
- A cue-cut row is never written; an album landed as a release group gets no totals
- **Blocked on `lofty`:** `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Search
- A pasted Spotify, Apple Music, TIDAL or YouTube playlist link is taken as words: only Deezer and
  ListenBrainz answer a playlist without an account
- **Blocked on a service:** a lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists; no FhG encode
  weighed
- **Blocked on a registered key:** AcoustID never reached with a real key; its fixture is written
  from the documentation and `tests/live.rs` has no AcoustID test

## Performance and scale
- **Blocked on gpui:** every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream
  (zed #62455)

## Keyboard and accessibility
- The marks opening a card or menu where pressed (an album's and artist's info mark, a playlist's
  ⋯), the order chips under a heading and the inspector's graph switch are no tab stops
- A selection is a contiguous run reached by Shift; Tracks and Albums act on its first row alone;
  nothing adds a row with Control
- **Blocked on gpui:** nothing is exposed to a screen reader: published gpui 0.2.2 carries no
  AccessKit, which only Zed's `main` has

## Testing
- `transport.rs`'s `turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame`
  failed once when the whole workspace's tests ran at once, passed alone eight times after; its
  frame counts assume the engine thread keeps pace with the test's pulls
- No frame is pulled through a stream in S16, S32, packed or padded S24, 5.1 or 7.1, or with
  `NO_CONVERT`, nor recorded from a microphone node: the desktop's sink is stereo F32 and a hosted
  daemon links nothing (no session manager; a hosted WirePlumber would reach the real cards)
- The drop overlay never dragged onto on a real compositor from this tree: gpui's `ExternalPaths` is
  `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the platform's drag
  events are unproved here
- **Blocked on hardware:** a microphone recording not proved against real sound reaching a
  microphone

## Later: Sources and providers
- **Blocked on the services:** the Bandcamp and Discogs links an `Identity` carries are read by
  nothing; no provider asks either
- **Blocked on the servers:** Subsonic matches a recording id or ISRC but not the release-track id
  the inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the
  release track

## Later: The vault
- A kept WAVE object reopened by path for a backward seek uses its old frame index against a renewal
  that replaced it. Do it with the next two items
- A renewal landing under a new key is weighed against the source, not the object it replaces: the
  vault can grow
- A WAVE object packed before the frames came is one zstd frame naming no length: a backward seek in
  it still restarts the stream
- `vault --verify` never checks covers and offers nothing to mend one failing row. After the
  renewal items
- The key is the PCM alone: two tracks of the same samples at another rate, count or mask (digital
  silence) share an object, and a standing object is taken on its size alone; needs a migration
- A JPEG cover is kept byte for byte where its lossless JXL is larger, never smaller than it: a
  byte-exact JPEG transcode (`jxl-encoder` 0.3, AGPL, opt-in) is an unproven spike
- Blanking a Matroska file stops at an element of unknown size or after 65 536 elements and lands
  what it blanked so far as stripped; a `TrackEntry` name, chapters and an MP4's track-level `meta`
  are never blanked; an MP4 past 4 096 boxes keeps every tag
- **Blocked on `flacenc`:** `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at
  96 kHz and the depth at 24 bits: a 24-bit rip loses to `flac -8` by ~15 % and is kept; a 192 kHz
  rip is never a FLAC

## Later: Equaliser and DSP extras
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position alone: a `Discrete(n)` source is truncated one for one; a stream's own
  downmix coefficients are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- **Blocked on gpui:** the sung line cannot grow as it lights: gpui 0.2.2 on Linux draws a glyph on
  a whole pixel vertically (`SUBPIXEL_VARIANTS_Y = 1`) and cosmic-text hints every size, so a type
  size in motion shimmers and hops; the line only brightens
- **Blocked on the format:** where a sheet gives a line no end (a plain LRC never; only an enhanced
  one with a closing stamp, or a Lyricsfile), when it goes out is guessed from its text length: a
  held note can be put out under the dots early
- **Blocked on the sources:** `Lyrics` holds no translation beside the original. Only an LRC line
  marked `[v1:]` or `[v2:]` names its singer; elsewhere a second voice is read off overlapping lines
  alone and a third is folded onto the two

## Later: Listen and recognition
- **Blocked on Shazam:** reached through an undocumented endpoint (`amp.shazam.com`); a change on
  its side stops recognition

## Later: Visualiser
- The live spectrum's tilt, floor, band width and fall rates are constants; its axis stops at 20 kHz
  at every rate
- The scope has no level meters, correlation or goniometer, and triggers on the mid's rising edge
- The plot opens empty for up to a buffer's depth: the tap runs only while the pane is in front
- The tap records frames as rendered, before the ring trims them: a muted stream still draws, a
  turned volume shows a buffer late

## Later: The window
- A track or album cannot be dragged from a listing into the queue or a playlist; only files from a
  file manager are taken. A drop is a positional insert (queue edits are now guarded by
  `Queued::revision`)

## Later: Scrobbling
- Last.fm is not reached at all; only ListenBrainz is told what was heard

## Later: MCP
- An edit a model makes is not on the window's *Undo*: undo stacks live in the process that made it

## Later: Packaging
- **Blocked on gpui:** gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc; `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
