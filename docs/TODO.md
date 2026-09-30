# Roadmap

Categories run from most to least important. Everything under a `Later:` heading is a nice-to-have
that no listener is waiting on, and is worked only once the categories above it are quiet. An item
marked **Blocked on …** waits on something outside this tree — hardware, an upstream crate, a
service or a format — and is not worked until that moves; everything else is open to be done.

## Playback and output
- **Blocked on hardware:** A device with no volume of its own is still turned by the stream, so
  anything under 100 % leaves bit-perfect there
- Changing the graph rate mid-track reopens the stream and costs the gap a sink switch does, and so
  does the rate policy, the buffer or DoP wherever the change moves the stream's format or the
  ring's depth
- **Blocked on pipewire-rs:** A stream's reported delay misses the frames in buffers it has already
  queued — `pw_time.queued` has no safe setter in pipewire-rs 0.10 — so the position and the
  visualiser's frame are short by up to one cycle
- **Blocked on PipeWire:** `SinkInfo::current_rate` is the graph-wide rate from the settings
  metadata, so every sink reports the same one; a per-device rate is the driver node's own clock,
  which the registry publishes nowhere
- The playback loop holds one playback stream and one capture stream; more than one concurrent
  playback stream is not supported
- **Blocked on hardware:** Nothing has proved a forced graph rate change against hardware — the only
  card here offers 48 kHz alone — nor DoP against a DAC that decodes it

## Formats
- **Blocked on `ape-decoder`:** A 32-bit stereo Monkey's Audio — integers or floats — is refused,
  because `ape-decoder` narrows the side channel to 32 bits before undoing it
- **Blocked on `symphonia-codec-wavpack`:** A `.wvc` correction file beside a hybrid WavPack is
  never opened, so the file plays and is billed as lossy: `symphonia-codec-wavpack` 0.1.1 reads a
  held zero's correction from the wrong range, and applying one waits on the crate being fixed
  upstream
- **Blocked on symphonia:** A file embedding a huge picture still costs one materialisation, because
  symphonia reads it into a buffer of its own before `probe_cover_art` can weigh it
- **Blocked on symphonia:** Opus mapping families 2, 3 and 255 are refused by symphonia's `OpusHead`
  reader

## Tagging
- On a filesystem that cannot clone a file — ext4 — a tag write that grows past the tag's room, and
  every write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) or an Ogg, still
  copies the whole file, and one with a second name twice

## Performance and scale
- Removing or moving a span near the top of a long playlist still holds every row after it for
  undo, the restore point being the tail from the edit's first row
- A duplicate of a WAVE, AIFF, WavPack or ALAC source still pays its whole encode before `Deduped`
  is known, none declaring the digest a FLAC's STREAMINFO carries
- **Blocked on gpui:** Every frame the visualiser or the lyrics pane asks for is a whole-window
  paint on the GPU; gpui draws the scene whole, so only a newer gpui avoids it

## Library
- **Blocked on the format:** A cue row exported to PLS is its whole file, the format having no word
  for a region
- A tidy keeps the rows of a deleted file until its emptied folder goes, and drops the rows of an
  unplugged drive never scanned whose mount point's parent still holds another drive
- A lyric in a dropped album's `lyrics/` folder keeps its name when the track it is named after
  lands as `name (2).ext`, so it is matched to nothing

## Search
- **Blocked on a service:** A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text

## Identification
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists, and no FhG encode
  was weighed
- **Blocked on a registered key:** AcoustID has never been reached with a real key; its fixture is
  written from the documentation

## The window
- The equaliser curve draws one line for every channel, so a band shaping the left alone is drawn
  as the loudest channel's rather than beside the right's (`Profile::response_on` and
  `channels_apart` answer each channel's)

## Keyboard and accessibility
- Only Settings and the search field take focus: the transport, the sidebar, the heading buttons,
  the seek and volume rails and every row control are reachable by the mouse alone
- A menu opens on the right button alone, so a row's *Add to playlist*, *Go to artist*, *Share* and
  *Favourite* have no key
- A selection is a contiguous run reached by Shift, and Tracks and Albums act on its first row
  alone; nothing adds a row with Control
- **Blocked on gpui:** Nothing is exposed to a screen reader; gpui carries no AccessKit

## Testing
- The `probe` fuzz target never seeks, opens a span, decodes DSD to samples, hints an extension or
  reads a stream that cannot seek, so the seeks of `ape.rs`, `matroska.rs` and `dsd/` and the whole
  spooled path are unfuzzed
- *Take this name* is checked by eye alone: driving it wants a recognition the catalog holds, which
  no fake fingerprinter hands the analysis pane yet
- The drop overlay has never been dragged onto on a real compositor from this tree: gpui's test
  support cannot build an `ExternalPaths`, so `driven.rs` calls `dragged_over` and `dropped` directly
  and the platform's own drag events are unproved here
- **Blocked on hardware:** A microphone recording has not been proved against real sound reaching a
  microphone

## Later: Sources and providers
- **Blocked on the services:** The service links an `Identity` carries are read by nothing: no
  provider asks Tidal, Bandcamp or Discogs, whose pages the links name

## Later: The vault
- The stand-in's `TagSet` carries some twenty-one fields, so a vaulted row loses its composer,
  lyricist, comment, label and totals on the bus and in the inspector
- Blanking a Matroska file stops at an element of unknown size and lands what it blanked so far as
  stripped; a `TrackEntry` name, chapters and an MP4's track-level `meta` are never blanked, and an
  MP4 past 4 096 boxes keeps every tag
- A cover is kept as lossless pixels whatever it cost as a JPEG, so a 200 KB cover can land
  several times larger with its bytes dropped. **Blocked on `zune-jpegxl`** for recompressing the
  JPEG itself; keeping the original where it is smaller is not
- A renewal that lands under a new key is weighed against the source rather than the object it
  replaces, so the vault can grow
- The key is the PCM alone, so two tracks of the same samples at another rate, count or mask —
  digital silence — share an object, and a standing object is taken on its size alone
- A cancel is heard only between rows, so one long WAVE pass or FLAC encode cannot be interrupted,
  and the window imports on every core with no regard for what is playing
- A 24-bit hi-res source is written as a 32-bit WAVE, so the stand-in reports 32 bits where the
  catalog says 24
- **Blocked on `flacenc`:** `flacenc` 0.5.1 caps the Rice parameter at 14, the rate at 96 kHz and
  the depth at 24 bits, so a 24-bit rip loses to `flac -8` by some 15 % and is kept, and a 192 kHz
  rip is never a FLAC
- A WAVE object packed before the frames came is one zstd frame naming no length, so a backward
  seek in it still restarts the stream

## Later: Tagging and organising
- A cue-cut row is never written, and an album landed as a release group gets no totals
- `.caf`, `.mka`, `.oga` and the DSD containers have no writer: lofty writes none of them

## Later: Equaliser and DSP extras
- A binding for a device not plugged in cannot be seen or taken away alone
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position alone: a `Discrete(n)` source is truncated one for one, and a stream's
  own downmix coefficients are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its 1
  024-frame window, and leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- A sidecar's declared title is weighed by substring, so `[ti:It]` agrees with any title holding
  *it*, and a lone `\r` ending reads a sheet as one line
- `Song.en.lrc` and `Song.ja.lrc` are never offered, and an unrelated `<stem>.txt` is read as words
- A lookup still running when its row leaves the queue lands the last track's words over *none*
- There is no offset of the listener's own for a sheet that runs early or late
- **Blocked on the format:** When a line goes out is guessed from how long its text is wherever the
  sheet gives it no end — an LRC never does, only a Lyricsfile — so a held note can be put out under
  the dots early
- **Blocked on the sources:** `Lyrics` holds no translation beside the original, and no source says
  which singer owns a line: a second voice is read off overlapping lines alone, and a third is
  folded onto the two

## Later: Listen and recognition
- Listen records one clip and asks once; nothing listens again on a miss or follows a stream from
  song to song
- **Blocked on Shazam:** Shazam is reached through an undocumented endpoint, so a change on its side
  stops recognition

## Later: Visualiser
- The spectrum's tilt, floor, band width and fall rates are constants, and its axis stops at 20 kHz
  at every rate
- The scope has no level meters, correlation or goniometer, and triggers on the mid's rising edge
- The plot opens empty for up to a buffer's depth, because the tap runs only while the pane is in
  front

## Later: The window
- Nothing dropped from a file manager is taken, and a track or album cannot be dragged into the
  queue or a playlist
- The window's title never names what is playing
- The statistics count the albums heard and never draw them
- The `vault` key has no field, so the Vault group is reached only by editing `config.toml`

## Later: Scrobbling
- Plays from before the token are never sent to ListenBrainz, and Last.fm is not reached at all
- *Playing now* is told again only when the row changes, so a pause and a resume, or a track on
  repeat, lets it lapse

## Later: MPRIS
- A shuffle is announced as the whole list replaced, because `Tracks` is the play order and
  `Queued` publishes no other; listing rows in the order they were added, with the play order
  apart, needs the engine to publish both

## Later: MCP
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit

## Later: Packaging
- **Blocked on gpui:** gpui pulls `stacksafe` and with it `proc-macro-error2`, whose `E0365`
  future-incompatibility warning becomes a hard error in a future rustc. Only a `[patch]` or a newer
  gpui fixes it
