# Roadmap

Categories run from most to least important. Everything under a `Later:` heading is a nice-to-have
that no listener is waiting on, and is worked only once the categories above it are quiet. An item
marked **Blocked on …** waits on something outside this tree — hardware, an upstream crate, a
service or a format — and is not worked until that moves; everything else is open to be done.

## Playback and output
- Moving the volume, muting, or changing ReplayGain or the equaliser is heard up to the buffer's
  depth later, half a second by default, because gain and filters run ahead of a ring kept full
- The PipeWire open and sink enumeration wait on the engine thread for seconds, so a daemon slow to
  answer freezes the transport and the window's close
- Nothing compares the graph's live rate, read every cycle, with the stream's, so a stream another
  client holds at a second rate is converted by the graph while the chip still says bit-perfect
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
- DSD128 and DSD256 are decimated through the same 512 taps as DSD64, so DSD256 falls 0.2 dB by
  10 kHz and 0.9 dB by 20 kHz, and DSD128 0.06 dB by 20 kHz
- A headerless VBR MP3's length is symphonia's bitrate guess held as exact, so its end can be
  unseekable and the bar fills early
- The ReplayGain reference loudness, iTunes Sound Check and the LAME header's gain are never read
- A DSF, DSDIFF or Monkey's Audio file on a source that cannot seek, too long to hold whole, is
  refused as wanting a seek rather than played as it arrives
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
- A file's own sort names — `ARTISTSORT`, `ALBUMARTISTSORT`, `TITLESORT`, `TSOP`, `soar` — are never
  read, so an unenriched artist never orders the way its tagger meant
- On a filesystem that cannot clone a file — ext4 — a tag write that grows past the tag's room, and
  every write to a tag at the end of a file (WAVE, AIFF, WavPack, Monkey's Audio) or an Ogg, still
  copies the whole file, and one with a second name twice

## Library
- Kept lyrics and lyric refusals are keyed by path and never swept, so a file replaced at the same
  path shows the old song's words and the tables only grow
- On a case-insensitive volume a name differing from the layout only in case is offered as a move
  and refused on apply as colliding with itself, every run
- Two cue sheets in one folder naming the same file cut it twice, and its rows flip between the two
  on every scan
- A steady writer under a root defers its rescan indefinitely, and a root the watch could not cover
  — the inotify limit reached — is never tried again
- A playlist undo restores rows under the paths they had when the edit was made, so after an
  organise it puts back dead paths
- A playlist undo rewrites the playlist from the window's memory without checking it is unchanged,
  so it destroys rows the command line, MCP or another window added meanwhile
- A playlist sheet from another machine or drive layout cannot be reconnected by its trailing path
  components, so every row of a Windows-written sheet imports as missing
- An organise cancel is heard only between batches of 256 moves, so a run of copies across devices
  cannot be stopped for minutes
- A tidy keeps the rows of a deleted file until its emptied folder goes, and drops the rows of an
  unplugged drive never scanned whose mount point's parent still holds another drive
- A lyric in a dropped album's `lyrics/` folder keeps its name when the track it is named after
  lands as `name (2).ext`, so it is matched to nothing
- **Blocked on the format:** A cue row exported to PLS is its whole file, the format having no word
  for a region

## Search
- A pasted link to an artist or a playlist is taken as words to search, not followed to what it
  names; only a link to a song or an album is downloaded
- A pasted album link is wanted from the pressing most of its release group's pressings share, not
  the one whose barcode the link named, so a deluxe edition linked downloads the standard track list
- An official music video longer than its song by more than five seconds names nothing when its
  page carries no ISRC, the title search holding a recording to the length the video ran
- The songs of an artist's releases not held are read only when the lookup pass reaches them, so an
  artist page opened before then lists the albums and none of their songs, and a large library's
  first pass runs on for an hour or more reading them, one release group a second
- **Blocked on a service:** A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text

## Identification
- With an AcoustID key set, a service refusing or unreachable is still asked about every remaining
  track of the study pool, one failed request after another
- A release too large for the 4 MiB document cap, a box set with per-recording relations, is never
  enriched and nothing says why
- A file with no measured length asked by an ISRC naming several recordings is identified as the
  first one, as exactly as a tagged id
- An album whose tracks name no album artist is taken by a release search on its title and track
  count alone
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists, and no FhG encode
  was weighed
- **Blocked on a registered key:** AcoustID has never been reached with a real key; its fixture is
  written from the documentation

## Performance and scale
- Every process opening the catalog rebuilds the collaboration credits and sweeps orphans under the
  write lock, each credit scanning the tracks by an artist index a plain `=` cannot use, so a
  read-only `resonate stats` also makes the window reload its vocabulary
- A scan that changed nothing still regroups alternatives, gathers loose files and prunes over the
  whole catalog, so one watched file added pays for the library
- Following or filing moved files updates playlist rows and resumption rows by an unindexed path
  once per file, N moves times M rows inside the scan's write
- The scan's 34-parameter track upsert, and the per-row reads beside it, are parsed again for every
  file rather than cached as `library.md` says every per-row write is
- Scrolling a large library to the end reads the whole prefix of tracks, albums and artists again at
  every page, quadratic in its length
- An incremental scan writes every unchanged row only to stamp it seen, and a change under one root
  stats every file of every other
- The picture likeness reads every embedded cover whole, in the orphan sweep and for each
  suggestion candidate
- Setting an AcoustID key or *Refresh all* reads every unrecognised print into memory before the
  pool starts, on the order of a gigabyte for a large library
- A row's tags and cover are looked up by statting its file under the catalog lock on the caller's
  thread, so a stalled network mount freezes the window's redraw and the bus
- One thread reads the tags of every queued row, so a remote source that does not answer holds back
  the local rows behind it five seconds a row
- Every backward seek in an MP3 or ADTS stream walks the frames from the first, and the Xing table
  of contents is never used
- Planning a tag run parses every file with lofty twice more than it needs, covers and audio
  properties included
- Every lookup pass re-pairs and rewrites each album short of a track though nothing moved, which
  also takes down an open tag or organise preview
- The study's true-peak meter interpolates every sample eight times over where the playback guard
  skips stretches that cannot pass
- The Analysis pane decodes a vaulted track whose file has gone on every visit, since a kept
  analysis is keyed on the file's size and time
- Each queue edit and each catalog revision re-reads every queued track while the queue pane is
  open, and each type-ahead key folds every row's name on the UI thread
- MCP's `playlist_tracks` and playlist resource read every row, or the whole catalog for a
  self-filling playlist, before taking a few hundred, and the resource list recounts every
  playlist twice a second
- The Missing pane's counts fold every track title of an artist for each release, on each narrowing
  key
- Dragging files from a file manager stats every path on entering and on dropping, and copies the
  paths on every pointer move
- Opening a DST DSDIFF reads one header per compressed frame through the whole file on every open
  and rebind
- Queueing tens of thousands of rows inserts each id into a sorted list, quadratic in the batch
- Cover decoding has no pixel limit beyond the image crate's, and a cover with transparency is
  shrunk without premultiplying its alpha
- One- and two-letter search prefixes enumerate every matching term, the full-text index declaring
  no prefix indexes
- The window polls the player every 16 ms for as long as it is open, paused or not
- **Blocked on gpui:** Every frame the visualiser or the lyrics pane asks for is a whole-window
  paint on the GPU; gpui draws the scene whole, so only a newer gpui avoids it

## Robustness
- Closing the window or pressing `ctrl-q` during a tag write, an organise, a vault import or a
  dropped file's copy neither cancels the pass nor waits for it
- Two lookups or polls in two processes ask the same rows at once, doubling the rate on MusicBrainz
  and the providers
- A host that stays busy is no longer waited on, so a `Retry-After` on a 429 is never read
- A service link of any scheme, or one whose authority hides another host behind a backslash, is
  opened if its host looks like a known service, and image host checks are string suffix tests
  a `#` or `?` passes
- A daemon connection that dies with a reset rather than a broken pipe, or hangs, is never taken as
  lost, so the client stays disconnected
- When WirePlumber restarts, the metadata objects that left keep their proxies and their values
  stand stale until new ones overwrite them
- MCP's `start_scan` keeps any folder it is given as a root for good, `/` included, with no cap and
  no tool to remove one
- During a long MCP call the first interrupt blocks the signal thread, so a second cannot leave at
  once, and a session's passes are cancelled and joined one after another

## Keyboard and accessibility
- Keys, tokens, the contact, the Subsonic account and the organise layout typed in Settings and left
  without Enter look saved and are not
- `End`, select-all and the scrollbar act on the 2 000 rows loaded so far, so `End` in a
  50 000-track library lands on row 2 000
- Word motions in a field split a decomposed accent from its letter, and take a whole CJK sentence
  as one word
- Only Settings and the search field take focus: the transport, the sidebar, the heading buttons,
  the seek and volume rails and every row control are reachable by the mouse alone
- A menu opens on the right button alone, so a row's *Add to playlist*, *Go to artist*, *Share* and
  *Favourite* have no key
- A selection is a contiguous run reached by Shift, and Tracks and Albums act on its first row
  alone; nothing adds a row with Control
- **Blocked on gpui:** Nothing is exposed to a screen reader; gpui carries no AccessKit

## Testing
- No transport test covers a failed rebind, a setting changed while a row is parked, a seek during
  the sleep fade, a track of unknown length, removing the playing row under repeat, or a
  reconnect at a track boundary
- Every test against a real PipeWire daemon opens stereo F32 at 48 kHz: the S16 and S32 words,
  packed and padded S24, 5.1 and 7.1 maps, `NO_CONVERT` and a sink leaving under an open stream
  are never run
- The `probe` fuzz target never seeks, opens a span, decodes DSD to samples, hints an extension or
  reads a stream that cannot seek, so the seeks of `ape.rs`, `matroska.rs` and `dsd/` and the whole
  spooled path are unfuzzed; its seeds also lack Matroska lacing and unknown-size clusters,
  fragmented MP4, m4b chapters, FLAC `CHAPTER` comments and a variable-packet CAF, and `cue`
  reaches none of the file resolution
- The search grammar, `MediaLocation::from_uri`, `text::decoded`, the EqualizerAPO and GraphicEQ
  readers and the MCP line reader have no fuzz target
- Scanning a non-UTF-8 file name, two sheets naming one file, a sheet with no audio track and the
  inotify limit are untested
- Nothing drives `play`'s signal paths or its terminal restore; only `mcp`'s hang-up is driven
- *Take this name* is checked by eye alone: driving it wants a recognition the catalog holds, which
  no fake fingerprinter hands the analysis pane yet
- The drop overlay has never been dragged onto on a real compositor from this tree: gpui's test
  support cannot build an `ExternalPaths`, so `driven.rs` calls `dragged_over` and `dropped` directly
  and the platform's own drag events are unproved here
- **Blocked on hardware:** A microphone recording has not been proved against real sound reaching a
  microphone

## Later: Sources and providers
- A want is carried onto whatever song sits at its disc and position after the release is chosen
  again or reordered
- A file the inbox delivers into the vault counts nothing in `PollProgress::received`, the vault
  reading it by its path
- Forgetting a delivered row remembers only the delivery its object was first noted from, so a
  second provider that delivered the same audio is fetched from again
- Subsonic matches a recording id or ISRC but not the release-track id the inbox accepts
- TIDAL's device sign-in is the window's alone, with no command-line way in
- **Blocked on the services:** The Bandcamp and Discogs links an `Identity` carries are read by
  nothing; no provider asks either

## Later: The vault
- `vault --verify` writes an object it could not open — an unmounted vault, a missing file — as one
  that did not read back, never checks covers, and offers nothing to mend one failing row
- `vault --prune` and `--release` act at once where every other pass previews until `--apply`
- A kept WAVE object reopened by path for a backward seek uses its old frame index against a renewal
  that replaced it
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
- The names derived beside a long destination — the staging and parked names, a sidecar's longer
  suffix — are not held to 255 bytes, so the move fails the same way every run
- A sidecar such as `Song.live.lrc` travels with `Song.flac` rather than `Song.live.flac`, the
  shorter stem claiming it first
- A root on a CIFS or SMB share is named as if it took any character, so a title with `?`, `:` or
  `"` fails on a share that refuses them
- A cue-cut row is never written, and an album landed as a release group gets no totals
- `.caf`, `.mka`, `.oga` and the DSD containers have no writer: lofty writes none of them

## Later: Equaliser and DSP extras
- An unsupported EqualizerAPO filter or a `Device:` scope is dropped without a word, and a
  `6dB`/`12dB` slope word on a pass or shelf filter is ignored
- A GraphicEQ line or measurement of more than 1 024 points loses its top frequencies rather than
  being thinned
- Choosing a room-correction file and then *Stop correcting* while it is read leaves correction on,
  and two picks landing out of order keep the first
- Mid-track digital silence through the equaliser reaches a 16-bit device as shaped hiss until the
  filter tail falls to −600 dB, the dither muting only on exact zeros
- *Millibels* means hundredths of a dB for the trim and thousandths for a band gain and the preamp
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
- **Blocked on gpui:** The sung line cannot grow as it lights: gpui on Linux draws a glyph on a whole
  pixel vertically and cosmic-text hints every size, so a type size in motion shimmers and hops, and
  the line only brightens
- **Blocked on the format:** When a line goes out is guessed from how long its text is wherever the
  sheet gives it no end — an LRC never does, only a Lyricsfile — so a held note can be put out under
  the dots early
- **Blocked on the sources:** `Lyrics` holds no translation beside the original, and no source says
  which singer owns a line: a second voice is read off overlapping lines alone, and a third is
  folded onto the two

## Later: Listen and recognition
- A clip with no peaks, a steady tone or sparse material, is still sent to Shazam
- Listen records one clip and asks once; nothing listens again on a miss or follows a stream from
  song to song
- **Blocked on Shazam:** Shazam is reached through an undocumented endpoint, so a change on its side
  stops recognition

## Later: Visualiser
- A track of four and a half hours or more gets no time marks on the analysis waveform
- The spectrum's tilt, floor, band width and fall rates are constants, and its axis stops at 20 kHz
  at every rate
- The scope has no level meters, correlation or goniometer, and triggers on the mid's rising edge
- The plot opens empty for up to a buffer's depth, because the tap runs only while the pane is in
  front

## Later: The window
- A track or album cannot be dragged from a listing into the queue or a playlist; only files from a
  file manager are taken
- A scoped album or artist whose rows vanish leaves *album 17* heading an empty list
- The queue's total length leaves out rows the catalog has not scanned, with no hint it is partial
- The by-line counts a character its face cannot draw — CJK, emoji — as no width, so the album clips
  with no ellipsis
- Queue edits from the window name rows by position, so an edit another client makes between the
  gesture and the click moves or removes the wrong rows
- The seek bar shows only the total length, with no remaining time and no time under the pointer
- Favourites cannot be sorted, by the date marked or otherwise
- *Listen for* offers 8, 12 and 20 seconds while the file takes 4 to 60, and another value selects
  no chip and is labelled 20
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
- A model has no tool to undo its own edits, so a discarded playlist or removed rows cannot be taken
  back by the session that made them
- Tools carry the read-only and destructive hints alone, so every read-only catalog tool reads as
  reaching an open world
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit

## Later: Packaging
- **Blocked on gpui:** gpui pulls `stacksafe` and with it `proc-macro-error2`, whose `E0365`
  future-incompatibility warning becomes a hard error in a future rustc. Only a `[patch]` or a newer
  gpui fixes it
