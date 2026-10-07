# Roadmap

Open work only. Categories run from most to least important, and within one the items run from
most important to least, a prerequisite before what depends on it, the easier first among equals.
Everything under a `Later:` heading is a nice-to-have no listener is waiting on. An item marked
**Blocked on …** waits on the named thing outside this tree (hardware, an upstream crate, a service
or a format) and sits at the end of its category; it is not worked until that moves. Everything
else is open to be done.

## Defects
- Closing a playback stream or a recording drops the `StreamRc` before its `StreamListener` (the
  tuple's first field goes first), so `pw_stream_destroy` frees the hook list and the listener's
  `spa_hook_remove` then writes two pointers into freed memory, on every track change, sink switch,
  reconnect and shutdown; AddressSanitizer shows it, the same tuples in the other order do not
- A layout rendering a folder or file name that begins with a dot (*...And Justice for All*, an
  artist called *.38 Special*) files the tracks where the scan passes over hidden names, so the next
  scan prunes them with their plays, listens and favourites; `as_one_component` trims trailing dots
  only, and a test pins the dot-led path as right
- A tag write landed back into the file's own inode (a file with a second name, a staged copy that
  cannot carry the owner or attributes, a folder nothing may be created in) that fails partway
  removes the staged copy, the only whole one, leaving a torn track; a crash mid-write-back leaves a
  copy the next write to that track sweeps as left over
- A tag run deletes the kept lyrics and lyric refusals of every file it writes: following the new
  size and mtime fires `lyrics_forget_a_changed_file`, and `StudiesKept` holds only the studies
  across it
- A row waiting for a device, or for the graph to come back, is stranded when a sink change is
  announced while the survey that would bind it is out: `bind` waits for the next survey and answers
  `Ok`, the caller takes that as bound and clears `unbound` and `waiting_for_a_device`, and the row
  sits silent in Loading, Play not reviving it
- A contact typed for MusicBrainz travels in the User-Agent through the Cover Art Archive's redirect
  to archive.org, a host that never asked who is asking: ureq keeps every header but the
  credentials across a redirect, and the identity test pins the first hop alone
- A room-correction response measured at a rate other than the stream's is redrawn shifted early by
  the resampler's reach (some 7 ms at 44.1 to 48 kHz) and loses its start, `resampled` skipping
  `latency_frames` from an output that is already time-aligned; the test at another rate checks only
  the length
- A pause pressed while the last buffer of a track drains leaves the transport `Draining`, which is
  published as Playing, so the window and the bus say Playing over silence
- A sleep timer that comes due while the next track is opening is cleared without pausing, because
  `doze` returns on an empty `track` before `pause` could note the opening, so the music plays on
- An open playlist marks the row `loaded_position` names by its place in the view, so narrowing it
  by a search, or moving, sorting or removing its rows while it plays, marks a different song than
  the one playing
- A search for an album artist stops finding that artist's tracks and albums once enrichment
  re-indexes them, because only the scan writes the album artist into `tracks_fts`'s artist column
  and every reindex and refold writes the track's artist alone
- The queue is kept for the next run by tasks that each replace the last in `keep_the_queue`, so a
  queue write still waiting for a thread is cancelled by the place write behind it, and the next run
  resumes an older queue
- MPRIS `Next` on the last row of a queue that does not repeat stops playback and ends the queue,
  though `CanGoNext` says false and the spec makes the call do nothing; `Previous` on the first row
  restarts the track the same way
- A name within some thirty bytes of the 255-byte limit cannot be written: a delivery's
  `.resonate-delivery` staging name, a dropped file's `.resonate-part`, its `" (2)"` candidates and
  a tag write's staged sibling outgrow it, so the want is counted unkept each poll and given up, the
  drop is refused and the tag write fails though it could be written in place
- A poll killed while filing a streamed delivery into the music folder leaves its hidden
  `.resonate-delivery` file behind, up to 4 GiB, and nothing sweeps it as the vault, organise and
  take-in do for theirs
- The queue's type-ahead matches the titles it read when the queue last changed, its names keyed by
  the queue's revision alone, so a retagged or enriched queued track is still found by its old name
- The analysis pane remembers a refused or unreachable recognition for the session, so a track
  looked up while offline is never asked again until 64 others push it out or the window restarts
- A `subsonic` setting written without a scheme makes ureq's bad-URI error, which prints the whole
  request URL with its token and salt, land in the debug log
- `play`'s key reader takes one byte after `ESC [`, so Ctrl or Shift with an arrow and F5 to F12
  leak their tail as keys and leave the prompt in a half-typed line such as `5C` until Enter or Esc
- An M3U written with bare carriage returns reads as one comment line and imports as an empty
  playlist without saying so
- Text from a file or a service is read in quadratic time or without a bound in a few places: an
  enhanced-LRC line of thousands of `<` takes a minute to read (512 KiB, measured), an XSPF value of
  `&` with no `;` rescans the rest for each, a WAVE file's `LIST`/`INFO` ceiling of 1 MiB holds per
  list and not over the 4 096 chunks, and LRCLIB's plain lyrics and a Lyricsfile's `plain` text are
  split into lines with no `MOST_LINES`
- A UTF-8 lyric sidecar over 512 KiB is cut mid-character before its encoding is weighed, so it is
  guessed as a legacy code page and comes out as mojibake rather than the whole lines that fitted
- A TIDAL token answer with an enormous `expires_in`, or a DASH manifest with a `startNumber` near
  `u64::MAX`, panics the provider's thread through unchecked `Instant` and range arithmetic instead
  of being refused as unreadable
- A pod that fails to parse back from its own bytes is reported as `PodBuild` with a
  `GenError::NotYetImplemented` source that nothing raised

## Playback and output
- Changing the graph rate mid-track reopens the stream and costs the gap a sink switch does, and so
  does the rate policy, the buffer or DoP wherever the change moves the stream's format or the
  ring's depth
- Changing the equaliser is heard up to the buffer's depth later, half a second by default, because
  the filters run ahead of a ring kept full; only a level can be trimmed where the graph pulls
- The playback loop holds one playback stream and one capture stream; more than one concurrent
  playback stream is not supported
- **Blocked on hardware:** Nothing has proved a forced graph rate change against hardware (the only
  card here offers 48 kHz alone) nor DoP against a DAC that decodes it

## Formats
- **Blocked on `ape-decoder`:** A 32-bit stereo Monkey's Audio, integers or floats, is refused,
  because `ape-decoder` 0.3.2 narrows the side channel to 32 bits before undoing it
- **Blocked on `symphonia-codec-wavpack`:** A `.wvc` correction file beside a hybrid WavPack is
  never opened, so the file plays and is billed as lossy: 0.1.1, the only release, reads a held
  zero's correction from the wrong range
- **Blocked on symphonia:** A file embedding a huge picture still costs one materialisation, because
  symphonia 0.6.1 reads it into a buffer of its own before `probe_cover_art` can weigh it; its
  `limit_visual_bytes` option is honoured by no reader
- **Blocked on symphonia:** Opus mapping families 2, 3 and 255 are refused by symphonia's `OpusHead`
  reader, and families 2 and 3 would also need projection decoding

## Tagging
- The tracks pane files a track under its artist's sort name only where its own file carries one,
  so an artist sorted by MusicBrainz or by another file's `ARTISTSORT` still files its untagged
  tracks under *The*
- On a filesystem that cannot clone a file (ext4) a tag write that grows past the tag's room, and
  every write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) or an Ogg, still
  copies the whole file, and one with a second name twice
- A cue-cut row is never written, and an album landed as a release group gets no totals
- **Blocked on `lofty`:** `.caf`, `.mka` and the DSD containers have no writer, lofty 0.25.4 having
  no such file type; `.oga` alone could be mapped to Vorbis by hand

## Search
- A search typed with combining accents (`Mo` + U+0308 + `tley`) is split into two words at the
  mark, nothing normalising it first, and finds nothing where the same name precomposed finds the
  track; the hit highlight misses a decomposed title the same way
- A pasted link to a playlist is taken as words to search, not followed to what it names
- **Blocked on a service:** A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text (LRCLIB's text search finds none)

## Identification
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists, and no FhG encode
  was weighed
- **Blocked on a registered key:** AcoustID has never been reached with a real key; its fixture is
  written from the documentation and `tests/live.rs` has no AcoustID test

## Performance and scale
- One watched file added still regroups the alternatives of the whole catalog and gathers the loose
  albums of its whole root; a scan that changed nothing does neither
- A change under one root stats every file of every other, the tidy reading each other root's paths
- Scrolling a large library to the end reads the whole prefix of tracks, albums and artists again at
  every page, quadratic in its length; design it with the `End` item under Keyboard
- Each queue edit and each catalog revision re-reads every queued track while the queue pane is
  open, and each type-ahead key folds every row's name on the UI thread
- Taking a large run out of a long queue freezes the queue pane for as long as it can be put back,
  because each frame checks every taken row against every queued row
- Dragging files from a file manager stats every path on entering and on dropping, and the drop
  overlay and the Library settings stat the music folder on every frame, so a folder on a stalled
  network mount freezes the window
- A first scan reads each album's embedded cover inside the transaction that holds the catalog's
  write lock, so every other write waits behind file reads
- A picture's identity is taken from the length and first 256 bytes of each embedded cover, which
  SQLite reads whole, in every orphan sweep and for each suggestion candidate
- The Missing pane's counts, listing and due lookups fold every track title of an artist for each
  release, on each narrowing key
- Every backward seek in an MP3 or ADTS stream walks the frames from the first, and the Xing table
  of contents is never used
- **Blocked on gpui:** Every frame the visualiser or the lyrics pane asks for is a whole-window
  paint on the GPU; gpui 0.2.2, the latest release, draws the scene whole and its damage tracking is
  open upstream (zed #62455)

## Keyboard and accessibility
- The album and artist strips on a search's *Top results*, held and found, are not reached by the
  keys; only its songs are
- `End`, select-all and the scrollbar act on the 2 000 rows loaded so far, so `End` in a
  50 000-track library lands on row 2 000
- Only Settings and the search field are tab stops: the transport buttons, the sidebar entries, the
  heading buttons, the seek and volume rails and every row control cannot be focused or pressed from
  the keyboard
- A menu opens on the right button alone, so a row's *Add to playlist*, *Go to artist*, *Share* and
  *Favourite* have no key. After the focus item
- A selection is a contiguous run reached by Shift, and Tracks and Albums act on its first row
  alone; nothing adds a row with Control. After the `End` item
- **Blocked on gpui:** Nothing is exposed to a screen reader; the published gpui 0.2.2 carries no
  AccessKit, which only Zed's `main` has

## Testing
- `resonate-mpris`'s `bus.rs` tests fail now and then, a different `set_position_*` each time, when
  the whole workspace's tests run at once, and pass alone; the fixed 300 ms wait in `set_position_*`,
  the 500 ms `SETTLE` and the `RealtimeSink` thread paced by `thread::sleep` are the candidates
- `transport.rs`'s `turning_a_bit_perfect_track_down_and_back_up_keeps_its_stream_and_every_frame` failed once
  when the whole workspace's tests ran at once and passed alone eight times after; its frame counts
  assume the engine thread keeps pace with the test's pulls
- Every test against a real PipeWire daemon opens stereo F32 at 48 kHz: the S16 and S32 words,
  packed and padded S24, 5.1 and 7.1 maps, `NO_CONVERT` and a sink leaving under an open stream
  are never run
- The inotify limit is untested
- The `probe` fuzz target never seeks, decodes DSD to samples, hints an extension or reads a stream
  that cannot seek, so the seeks of `ape.rs`, `matroska.rs` and `dsd/` and the whole spooled path are
  unfuzzed; its seeds also lack Matroska lacing and unknown-size clusters, fragmented MP4, m4b
  chapters, FLAC `CHAPTER` comments and a variable-packet CAF, and `cue` reaches none of the file
  resolution
- No test records from a microphone node, though the reconnect test's hosted daemon could serve a
  virtual source
- Nothing drives `play`'s signal paths or its terminal restore; only `mcp`'s hang-up is driven
- *Take this name* is checked by eye alone: `driven.rs` hands the analysis pane
  `Fingerprinters::none()`, and driving it wants a fake that returns a recognition the catalog holds
- The drop overlay has never been dragged onto on a real compositor from this tree: gpui's
  `ExternalPaths` is `pub(crate)`, so `driven.rs` calls `dragged_over` and `dropped` directly and the
  platform's own drag events are unproved here
- **Blocked on hardware:** A microphone recording has not been proved against real sound reaching a
  microphone

## Later: Sources and providers
- Forgetting a delivered row remembers only the delivery its object was first noted from, so a
  second provider that delivered the same audio is fetched from again
- **Blocked on the services:** The Bandcamp and Discogs links an `Identity` carries are read by
  nothing; no provider asks either
- **Blocked on the servers:** Subsonic matches a recording id or ISRC but not the release-track id the
  inbox accepts, the API's `musicBrainzId` naming the recording and nothing in a song naming the
  release track

## Later: The vault
- A source kept whole (lossy, DSD, past eight channels, or a FLAC or WAVE that would not shrink)
  whose read fails mid-decode during an import, a share that dropped or a read that timed out, is
  stamped refused as not read back, so no later `--import` tries it until its file or the encoder
  changes; the FLAC and WAVE paths pass such a row over unstamped, as `vault.md` says
- A read error on the source while a kept copy is made carries the staging file's path, so
  `failed_itself` takes it for the vault failing and stops the whole import instead of passing the
  row over
- A kept WAVE object reopened by path for a backward seek uses its old frame index against a renewal
  that replaced it. Do it with the next two items
- A renewal that lands under a new key is weighed against the source rather than the object it
  replaces, so the vault can grow
- A WAVE object packed before the frames came is one zstd frame naming no length, so a backward
  seek in it still restarts the stream
- `vault --verify` writes an object it could not open (an unmounted vault, a missing file) as one
  that did not read back, never checks covers, and offers nothing to mend one failing row. After
  the renewal items
- The stand-in's `TagSet` is built from seventeen catalog columns, so a vaulted row loses its
  composer, lyricist, comment, label, totals and grouping on the bus and in the inspector;
  `Kept::declared` holds them at import and nothing keeps them
- A cancel is heard only between rows, so one long WAVE pass or FLAC encode cannot be interrupted,
  and the window imports on every core with no regard for what is playing
- The key is the PCM alone, so two tracks of the same samples at another rate, count or mask
  (digital silence) share an object, and a standing object is taken on its size alone; it needs a
  migration
- A cover is kept as lossless pixels whatever it cost as a JPEG, so a 200 KB cover can land several
  times larger with its bytes dropped; keeping the original where it is smaller needs no new codec,
  and a byte-exact JPEG transcode (`jxl-encoder` 0.3, AGPL, opt-in) is an unproven spike
- Blanking a Matroska file stops at an element of unknown size or after 65 536 elements and lands
  what it blanked so far as stripped; a `TrackEntry` name, chapters and an MP4's track-level `meta`
  are never blanked, and an MP4 past 4 096 boxes keeps every tag
- A 24-bit hi-res source is written as a 32-bit WAVE, and a 20-bit one stored as a 24-bit FLAC, so
  the stand-in reports the container's width where the catalog says the source's, and a study or
  `analyse` through the object judges the genuine master Fake for padded bits
- **Blocked on `flacenc`:** `flacenc` 0.5.1, the latest, caps the Rice parameter at 14, the rate at
  96 kHz and the depth at 24 bits, so a 24-bit rip loses to `flac -8` by some 15 % and is kept, and a
  192 kHz rip is never a FLAC

## Later: Equaliser and DSP extras
- A binding for a device not plugged in is shown by the window for plugged devices only, so it is
  seen only through `resonate eq` and cannot be taken away alone there
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position alone: a `Discrete(n)` source is truncated one for one, and a stream's
  own downmix coefficients are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, and leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- An unrelated `<stem>.txt` is read as words
- There is no offset of the listener's own for a sheet that runs early or late
- **Blocked on gpui:** The sung line cannot grow as it lights: gpui 0.2.2 on Linux draws a glyph on a
  whole pixel vertically (`SUBPIXEL_VARIANTS_Y = 1`) and cosmic-text hints every size, so a type size
  in motion shimmers and hops, and the line only brightens
- **Blocked on the format:** When a line goes out is guessed from how long its text is wherever the
  sheet gives it no end (a plain LRC never does, only an enhanced one with a closing stamp or a
  Lyricsfile), so a held note can be put out under the dots early
- **Blocked on the sources:** `Lyrics` holds no translation beside the original. Only an LRC line
  marked `[v1:]` or `[v2:]` names its singer; elsewhere a second voice is read off overlapping lines
  alone, and a third is folded onto the two

## Later: Listen and recognition
- *Open* on a recognised song hands the desktop whatever link Shazam or AudD answered, with no check
  that it is an `https://` page, where Deezer's is held to its track pages
- A clip that is not silent but leaves no peaks, a steady tone or sparse material, is still sent to
  Shazam
- Listen records one clip and asks each service once; nothing listens again on a miss or follows a
  stream from song to song. After the item above
- **Blocked on Shazam:** Shazam is reached through an undocumented endpoint (`amp.shazam.com`), so a
  change on its side stops recognition

## Later: Visualiser
- The live spectrum's tilt, floor, band width and fall rates are constants, and its axis stops at
  20 kHz at every rate
- The scope has no level meters, correlation or goniometer, and triggers on the mid's rising edge
- The plot opens empty for up to a buffer's depth, because the tap runs only while the pane is in
  front
- The tap records frames as rendered, before the ring trims them, so a muted stream still draws and
  a turned volume shows a buffer late

## Later: The window
- A track or album cannot be dragged from a listing into the queue or a playlist; only files from a
  file manager are taken. After the queue-position defect, a drop being a positional insert
- The `vault` key has no field, so a vault is opened only by `--vault` or by editing `config.toml`

## Later: Scrobbling
- Plays from before the token are never sent to ListenBrainz (favourites are, as loves), and Last.fm
  is not reached at all

## Later: MPRIS
- A shuffle is announced as the whole list replaced, because `Tracks` is built from the play order
  alone; `Queued::loaded_at` carries the order rows were added in, which the track list does not
  read, and `AddTrack` and `RemoveTrack` would then map back to play-order positions

## Later: MCP
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit

## Later: Packaging
- **Blocked on gpui:** gpui 0.2.2 pulls `stacksafe` 0.1 and with it `proc-macro-error2`, whose
  `E0365` future-incompatibility warning becomes a hard error in a future rustc. `stacksafe` 1.x
  dropped it and Zed's `main` uses that, so only a git gpui or a vendored `[patch]` of
  `stacksafe-macro` fixes it
