# Roadmap

Open work only. Categories run most to least important; items likewise (prerequisite before
dependent, easier first among equals). `Later:` = nice-to-have no listener waits on.
**Blocked on …** = waits on the named outside thing (hardware, upstream crate, service, format),
sits last in its category, not worked until it moves. Everything else is open.

## Defects
- The queue is kept for the next run by tasks each replacing the last in `keep_the_queue`: a queue
  write still waiting for a thread is cancelled by the place write behind it; the next run resumes
  an older queue
- MPRIS `Next` on the last row of a non-repeating queue stops playback and ends the queue though
  `CanGoNext` says false and the spec makes the call do nothing; `Previous` on the first row
  restarts the track likewise
- A name within ~30 bytes of the 255-byte limit cannot be written: a delivery's
  `.resonate-delivery` staging name, a dropped file's `.resonate-part`, its `" (2)"` candidates and
  a tag write's staged sibling outgrow it; the want is counted unkept each poll and given up, the
  drop is refused, the tag write fails though writable in place
- A poll killed while filing a streamed delivery into the music folder leaves its hidden
  `.resonate-delivery` file (up to 4 GiB); nothing sweeps it as the vault, organise and take-in do
  for theirs
- The queue's type-ahead matches titles read when the queue last changed (names keyed by queue
  revision alone): a retagged or enriched queued track is still found by its old name
- The analysis pane remembers a refused or unreachable recognition for the session: a track looked
  up offline is never asked again until 64 others push it out or the window restarts
- A `subsonic` setting without a scheme makes ureq's bad-URI error, which prints the whole request
  URL with token and salt, land in the debug log
- `play`'s key reader takes one byte after `ESC [`: Ctrl/Shift with an arrow and F5 to F12 leak
  their tail as keys and leave the prompt half-typed (such as `5C`) until Enter or Esc
- An M3U written with bare carriage returns reads as one comment line and imports as an empty
  playlist without saying so
- Text from a file or service is read in quadratic time or unbounded in places: an enhanced-LRC
  line of thousands of `<` takes a minute (512 KiB, measured); an XSPF value of `&` with no `;`
  rescans the rest for each; a WAVE `LIST`/`INFO` ceiling of 1 MiB holds per list, not over the
  4 096 chunks; LRCLIB's plain lyrics and a Lyricsfile's `plain` text are split into lines with no
  `MOST_LINES`
- A UTF-8 lyric sidecar over 512 KiB is cut mid-character before its encoding is weighed: guessed as
  a legacy code page, mojibake instead of the whole lines that fitted
- A TIDAL token answer with an enormous `expires_in`, or a DASH manifest with `startNumber` near
  `u64::MAX`, panics the provider's thread through unchecked `Instant` and range arithmetic instead
  of being refused as unreadable
- A pod that fails to parse back from its own bytes is reported as `PodBuild` with a
  `GenError::NotYetImplemented` source nothing raised

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
- The tracks pane files a track under its artist's sort name only where its own file carries one: an
  artist sorted by MusicBrainz or another file's `ARTISTSORT` still files its untagged tracks under
  *The*
- On a filesystem that cannot clone a file (ext4), a tag write outgrowing the tag's room, every
  write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) and every Ogg write
  still copies the whole file, twice for one with a second name
- A cue-cut row is never written; an album landed as a release group gets no totals
- **Blocked on `lofty`:** `.caf`, `.mka` and the DSD containers have no writer (lofty 0.25.4 has no
  such file type); `.oga` alone could be mapped to Vorbis by hand

## Search
- A search typed with combining accents (`Mo` + U+0308 + `tley`) splits into two words at the mark
  (nothing normalises first) and finds nothing where the precomposed name finds the track; the hit
  highlight misses a decomposed title likewise
- A pasted link to a playlist is taken as words to search, not followed
- **Blocked on a service:** a lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists; no FhG encode
  weighed
- **Blocked on a registered key:** AcoustID never reached with a real key; its fixture is written
  from the documentation and `tests/live.rs` has no AcoustID test

## Performance and scale
- One watched file added still regroups the alternatives of the whole catalog and gathers the loose
  albums of its whole root; a scan that changed nothing does neither
- A change under one root stats every file of every other; the tidy reads each other root's paths
- Scrolling a large library to the end re-reads the whole prefix of tracks, albums and artists at
  every page (quadratic); design with the `End` item under Keyboard
- Each queue edit and catalog revision re-reads every queued track while the queue pane is open;
  each type-ahead key folds every row's name on the UI thread
- Taking a large run out of a long queue freezes the queue pane for as long as it can be put back:
  each frame checks every taken row against every queued row
- Dragging files from a file manager stats every path on entering and dropping; the drop overlay and
  Library settings stat the music folder every frame: a folder on a stalled network mount freezes
  the window
- A first scan reads each album's embedded cover inside the transaction holding the catalog's write
  lock: every other write waits behind file reads
- A picture's identity is the length and first 256 bytes of each embedded cover, which SQLite reads
  whole, in every orphan sweep and per suggestion candidate
- The Missing pane's counts, listing and due lookups fold every track title of an artist for each
  release, on each narrowing key
- Every backward seek in an MP3 or ADTS stream walks the frames from the first; the Xing table of
  contents is never used
- **Blocked on gpui:** every frame the visualiser or lyrics pane asks for is a whole-window GPU
  paint: gpui 0.2.2, the latest release, draws the scene whole; damage tracking open upstream
  (zed #62455)

## Keyboard and accessibility
- The album and artist strips on a search's *Top results*, held and found, are not reached by the
  keys; only its songs are
- `End`, select-all and the scrollbar act on the 2 000 rows loaded so far: `End` in a 50 000-track
  library lands on row 2 000
- Only Settings and the search field are tab stops: transport buttons, sidebar entries, heading
  buttons, seek and volume rails and every row control cannot be focused or pressed by keyboard
- A menu opens on the right button alone: a row's *Add to playlist*, *Go to artist*, *Share* and
  *Favourite* have no key. After the focus item
- A selection is a contiguous run reached by Shift; Tracks and Albums act on its first row alone;
  nothing adds a row with Control. After the `End` item
- **Blocked on gpui:** nothing is exposed to a screen reader: published gpui 0.2.2 carries no
  AccessKit, which only Zed's `main` has

## Testing
- `resonate-mpris`'s `bus.rs` tests fail now and then, a different `set_position_*` each time, when
  the whole workspace's tests run at once, and pass alone; candidates: the fixed 300 ms wait in
  `set_position_*`, the 500 ms `SETTLE`, the `RealtimeSink` thread paced by `thread::sleep`
- `transport.rs`'s `turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame`
  failed once when the whole workspace's tests ran at once, passed alone eight times after; its
  frame counts assume the engine thread keeps pace with the test's pulls
- Every test against a real PipeWire daemon opens stereo F32 at 48 kHz: S16 and S32 words, packed
  and padded S24, 5.1 and 7.1 maps, `NO_CONVERT` and a sink leaving under an open stream never run
- The inotify limit is untested
- The `probe` fuzz target never seeks, decodes DSD to samples, hints an extension or reads a
  non-seekable stream: the seeks of `ape.rs`, `matroska.rs` and `dsd/` and the whole spooled path
  are unfuzzed; seeds also lack Matroska lacing and unknown-size clusters, fragmented MP4, m4b
  chapters, FLAC `CHAPTER` comments and a variable-packet CAF; `cue` reaches no file resolution
- No test records from a microphone node, though the reconnect test's hosted daemon could serve a
  virtual source
- Nothing drives `play`'s signal paths or terminal restore; only `mcp`'s hang-up is driven
- *Take this name* is checked by eye alone: `driven.rs` hands the analysis pane
  `Fingerprinters::none()`; driving it needs a fake returning a recognition the catalog holds
- The drop overlay never dragged onto on a real compositor from this tree: gpui's `ExternalPaths` is
  `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly; the platform's drag
  events are unproved here
- **Blocked on hardware:** a microphone recording not proved against real sound reaching a
  microphone

## Later: Sources and providers
- Forgetting a delivered row remembers only the delivery its object was first noted from: a second
  provider that delivered the same audio is fetched from again
- **Blocked on the services:** the Bandcamp and Discogs links an `Identity` carries are read by
  nothing; no provider asks either
- **Blocked on the servers:** Subsonic matches a recording id or ISRC but not the release-track id
  the inbox accepts: the API's `musicBrainzId` names the recording; nothing in a song names the
  release track

## Later: The vault
- A source kept whole (lossy, DSD, past eight channels, or a FLAC/WAVE that would not shrink) whose
  read fails mid-decode during an import (dropped share, timed-out read) is stamped refused as not
  read back, so no later `--import` tries it until its file or the encoder changes; the FLAC and
  WAVE paths pass such a row over unstamped, as `vault.md` says
- A read error on the source while a kept copy is made carries the staging file's path, so
  `failed_itself` takes it for the vault failing and stops the whole import instead of passing the
  row over
- A kept WAVE object reopened by path for a backward seek uses its old frame index against a renewal
  that replaced it. Do it with the next two items
- A renewal landing under a new key is weighed against the source, not the object it replaces: the
  vault can grow
- A WAVE object packed before the frames came is one zstd frame naming no length: a backward seek in
  it still restarts the stream
- `vault --verify` writes an object it could not open (unmounted vault, missing file) as one that
  did not read back, never checks covers, offers nothing to mend one failing row. After the renewal
  items
- The stand-in's `TagSet` fills seventeen fields from the catalog: a vaulted row loses its composer,
  lyricist, comment, label, totals and grouping on the bus and in the inspector; `Kept::declared`
  holds them at import and nothing keeps them
- A cancel is heard only between rows: one long WAVE pass or FLAC encode cannot be interrupted; the
  window imports on every core regardless of what is playing
- The key is the PCM alone: two tracks of the same samples at another rate, count or mask (digital
  silence) share an object, and a standing object is taken on its size alone; needs a migration
- A cover is kept as lossless pixels whatever it cost as a JPEG: a 200 KB cover can land several
  times larger with its bytes dropped; keeping the original where smaller needs no new codec; a
  byte-exact JPEG transcode (`jxl-encoder` 0.3, AGPL, opt-in) is an unproven spike
- Blanking a Matroska file stops at an element of unknown size or after 65 536 elements and lands
  what it blanked so far as stripped; a `TrackEntry` name, chapters and an MP4's track-level `meta`
  are never blanked; an MP4 past 4 096 boxes keeps every tag
- A 24-bit hi-res source is written as a 32-bit WAVE and a 20-bit one stored as a 24-bit FLAC: the
  stand-in reports the container's width where the catalog says the source's, and a study or
  `analyse` through the object judges the genuine master Fake for padded bits
- **Blocked on `flacenc`:** `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at
  96 kHz and the depth at 24 bits: a 24-bit rip loses to `flac -8` by ~15 % and is kept; a 192 kHz
  rip is never a FLAC

## Later: Equaliser and DSP extras
- A binding for a device not plugged in is shown by the window for plugged devices only: seen only
  through `resonate eq`, cannot be taken away alone there
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position alone: a `Discrete(n)` source is truncated one for one; a stream's own
  downmix coefficients are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- An unrelated `<stem>.txt` is read as words
- No listener-set offset for a sheet that runs early or late
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
- *Open* on a recognised song hands the desktop whatever link Shazam or AudD answered, unchecked as
  an `https://` page (Deezer's is held to its track pages)
- A clip not silent but leaving no peaks (steady tone, sparse material) is still sent to Shazam
- Listen records one clip and asks each service once; nothing listens again on a miss or follows a
  stream from song to song. After the item above
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
- The `vault` key has no field: a vault is opened only by `--vault` or by editing `config.toml`

## Later: Scrobbling
- Plays from before the token are never sent to ListenBrainz (favourites are, as loves); Last.fm is
  not reached at all

## Later: MPRIS
- A shuffle is announced as the whole list replaced: `Tracks` is built from the play order alone;
  `Queued::loaded_at` carries the order rows were added in, which the track list does not read, and
  `AddTrack`/`RemoveTrack` would then map back to play-order positions

## Later: MCP
- An edit a model makes is not on the window's *Undo*: undo stacks live in the process that made it

## Later: Packaging
- **Blocked on gpui:** gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc; `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
