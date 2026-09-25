# Roadmap

Categories run from most to least important. Everything under a `Later:` heading is a nice-to-have
that no listener is waiting on, and is worked only once the categories above it are quiet.

## Playback and output
- The volume is the software gain stage alone, so anything under 100 % leaves bit-perfect. Whether
  a device has a hardware volume is read and drawn, and nothing drives it
- Changing the rate policy, the graph rate, the buffer or DoP mid-track reopens the stream and
  costs the gap a sink switch does, and so does switching the equaliser while a resampler runs,
  because `OutputPlan::becomes_on_the_same_stream` will not carry a resampler's history into a new
  chain. Handing the new resampler the old one's state would close the equaliser case
- The place is written only where the row changed or the position moved `KEPT_EVERY`, so a run
  killed rather than closed loses up to five seconds of position
- A track the lookup has not studied is turned down only by the peak its tags declare, so a boost
  with no peak tag leans on the per-sample limiter until the study lands, and a bit-perfect stream
  of a file that is itself over full scale reaches the device as the file has it
- A stream's reported delay misses the frames in buffers it has already queued — `pw_time.queued`
  has no safe setter in pipewire-rs 0.10 — so the position and the visualiser's frame are short by
  up to one cycle
- `resonate sinks` and `resonate explain` cannot say whether a device takes `S24LE` or `S24_32LE`:
  `SinkFormats` folds both into `SampleFormat::S24`, and only the unpublished `FormatChanged` knows
  which the graph settled on
- `SinkInfo::current_rate` is the graph-wide rate from the settings metadata, so every sink reports
  the same one; a per-device rate is the driver node's own clock, which the registry publishes
  nowhere
- Only a card's current `Profile` is read. `EnumProfile` is not, so nothing can offer to switch one
- The playback loop holds one playback stream and one capture stream; more than one concurrent
  playback stream is not supported
- A capture running when the daemon restarts is dropped and not started again
- Nothing has proved a forced graph rate change against hardware — the only card here offers 48 kHz
  alone — nor DoP against a DAC that decodes it

## Formats
- WavPack and Monkey's Audio have no decoder anywhere in the tree, so the desktop entry, the bus
  and the scan pass them over. Closing it is a decoder beside `opus-rs` in the codec crate's
  registry
- DST-compressed DSDIFF is refused rather than decoded
- A source that cannot seek is prescanned only through its first `MAX_PRESCAN_HEAD` bytes, so over
  a pipe an `.m4a` with a trailing `moov` plays its priming, a WAV with `LIST INFO` after `data`
  loses those tags, and an Opus or FLAC track in Matroska falls back to millisecond timestamps
- A Matroska `Duration` longer than the file's clusters is caught only for Opus and FLAC; Vorbis
  would need its block sizes out of the setup header
- A file embedding a huge picture still costs one materialisation, because symphonia reads it into
  a buffer of its own before `probe_cover_art` can weigh it
- Opus mapping families 2, 3 and 255 are refused by symphonia's `OpusHead` reader
- Nothing applies DSD's +6 dB modulation convention, so a DSD master decimates about 6 dB quieter
  than the same music in PCM. Applying it would clip hot material; it is a decision to take

## Performance and scale
- The whole window is one `Render`, so any notify lays out and paints the whole tree, and the
  lyrics, the visualiser and the inspector rebuild it sixty times a second while playing. The answer
  is views of their own for the playback bar and the panes, which gpui can cache — and which would
  also let the visualiser repaint at the display's rate
- A picture the caches let go of leaves its tile in gpui's sprite atlas, so GPU memory grows with
  every distinct cover drawn in a run. Freeing it means drawing covers from a `RenderImage` the
  caches own rather than from encoded bytes
- Every line of a lyric set is laid out on every frame, including the ones faded to nothing; gpui's
  variable-height `list` is the virtualising element that would fix it
- The search vocabulary is unbounded — every distinct word and multi-word name across titles,
  artists, albums and genres — and a query weighs a run of tokens as a name from every start and
  length up to `MOST_TOKENS_IN_A_NAME`. Neither has been measured against the 500k-track library
  the scan is written for
- `names_in_the_queue` resolves the whole queue on the render thread on the first keystroke of a
  jump, up to two SQLite reads a row
- `resonate tag` and `resonate organise` read every scanned row into memory before planning
- A cover is decoded once and resized to all three drawn sizes whether or not the grid is opened,
  at a fixed twice-the-cell that is exact only at a scale factor of 1 or 2
- A provider is asked for bytes on the engine thread and the tag reader, and `MediaProvider` has
  no deadline, so a slow remote open stalls the track change; a provider or stream left behind as
  late keeps its thread for as long as it runs. A picture asked for after its tags landed opens the
  file a second time

## Library
- A file moved by hand is followed only where a vanished row and a new one agree in size, length,
  codec and tagged names, so a file retagged as it moved and a cue-cut file start again at nothing.
  A file no scan has seen counts nothing — a queue of unscanned files plays and is forgotten
- A root that is itself one album folder has no sleeve above it, so a various-artists album filed
  that way splits per track artist
- Nothing writes a favourite or a play count back into the file, so both stay with this catalog.
  lofty's `ItemKey::Popularimeter` maps to `POPM`, Vorbis `RATING`, MP4 `rate` and RIFF `IRTD` and
  carries a counter, so the seam `resonate tag` already is could write them
- Listening time counts only plays that counted, so a session spent skipping reads as quiet
- Nothing ages or bounds the play history
- No listing orders on a count inside a window, only the Statistics pane's own reads; a playlist's
  count is one date and one total with no history
- A playlist of cue rows exports as one path per row and imports as whole files, because M3U, PLS
  and XSPF have no vocabulary for a region
- A delivered row can only be forgotten with `resonate forget`; the window offers no gesture. It
  carries no genre or lyrics, and is levelled only once studied

## Identification
- A strict match is written without a pane to confirm it or a gesture to undo it: an album taken
  wrongly is put right only by clearing `albums.mbid` and its rows by hand
- A found song or a track named by its audio is placed on the earliest-dated release its recording
  sits on, often a single or compilation rather than the album meant. Nothing offers the choice
- A file with no title tag and a stem that is neither numbered nor separated gives the search
  nothing to ask with, so without an `acoustid-key` it is identified only by an ISRC or recording id
- A stem-named row renamed and edited together is asked again from its new name, overwriting what
  the last lookup wrote
- Half the artists of a real library have no portrait: nothing falls back to the release group's
  cover or reads the page a `wikipedia` relation names
- The discography asks for albums and EPs alone, so singles are never listed as not held, and past
  `GROUPS_AT_MOST` the rest are passed over with only a log line
- A group's other pressings are fetched and dropped; nothing lists a release's editions
- The verdict's thresholds were measured on one library of FLACs and LAME transcodes; no FhG, AAC,
  Opus or Vorbis transcode was weighed, and it reads only the lowpass wall, the upsampled wall and
  the padded bits — not an MP3's frame-to-frame holes, sfb21 content or pre-echo
- The Analysis pane's waveform and spectrogram are not kept, so a track is decoded whole again
  after a restart
- AcoustID has never been reached with a real key; its fixture is written from the documentation

## Search
- A lyric reaches only a row the catalog holds; no keyless service indexes lyric text
- The MusicBrainz search for a song not held shares the enrichment's paced client, so a running
  lookup holds it behind its queue, and nothing says why *Asking MusicBrainz…* stands so long
- A found song draws no cover, because art is fetched only for releases the catalog holds
- An opened suggestion lists only its first `PREVIEWED_AT_MOST` rows

## UI
- The Missing, Favourites, Statistics and Suggestions panes answer no keyboard reach and no
  type-ahead
- The playback bar keeps its controls centred by clipping its side columns, so a narrow window
  loses the end of the signal path or the notice; nothing drops an item before it is clipped
- A session with no XDG portal can add a library root only with `resonate scan`, and a scan blocks
  a second settings edit until it finishes
- A band on the equaliser's curve has no key to nudge it and its handles are not in the focus ring;
  a device's own curve cannot be kept as a named profile or discarded from the pane
- What the queue was cleared of can be put back only while nothing has been queued since, and the
  queue has no redo
- A name clipped by a fixed-width cell is sliced through a glyph rather than elided wherever an
  ancestor, not the text, is the box that runs out of room
- A menu's entries are decided when it opens, so one left open across a scan can offer a *Go to
  album* for an album since gathered away
- A row's controls are keyed by index, so editing a list can hand row N's tooltip to the new row N

## Testing
- Nothing drives `resonate-ui`'s panes: KWin offers no synthetic input without the remote-desktop
  portal, so a pane is seen only by making it the default and rebuilding. Everything that appears
  only under a pointer — the menus, drags, the pickers, the equaliser curve, *Take this name*, a
  suggestion card, the Listen card and its microphone chips, the Scope segment, scrolling the
  settings body — has been checked by eye alone
- `cargo bench -p resonate-dsp --bench stages` is run by nobody but a person, so a regression in a
  figure the rules quote is noticed only when somebody looks
- A microphone recording has not been proved against real sound reaching a microphone

## Later: Sources and providers
- No provider reaches a network: `resonate-inbox` is the only one, registered only where an inbox
  folder is set, and the service links an `Identity` carries are read by nothing

## Later: The vault
- `flacenc` 0.5.1 caps the Rice parameter at 14, the rate at 96 kHz and the depth at 24 bits, so a
  24-bit rip loses to `flac -8` by some 15 % and is kept, and a 192 kHz rip is weighed as a `Wave`
  first — which pays most of a single-threaded zstd level-19 pass, about 50 s of a core for five
  minutes of 24/192, before it is kept anyway
- A kept object whose container holds tags inside its structure — MP4, DSDIFF, Matroska, WAVE,
  AIFF and CAF — keeps them

## Later: Tagging and organising
- Nothing undoes a `resonate tag` or `resonate organise` run; the way back is another run
- A cue-cut row is never written, a thumbnail a ripper embedded is never replaced by a better
  cover, and an album landed as a release group gets no totals
- `.caf`, `.mka`, `.oga` and the DSD containers have no writer, and a WAV with ID3v2 before `RIFF`
  is refused
- A move cycle is refused rather than broken through a temporary name
- A disc numbered in words is composed only in English beyond the flat tables of twelve

## Later: Equaliser and DSP extras
- There is no convolution stage for room correction or a measured impulse response
- *Fit the preamp* models the curve rather than measuring what the music peaks at
- AutoEq is the only correction source, fetched one device at a time
- A downmix folds by position alone: a `Discrete(n)` source is truncated one for one, and a
  stream's own downmix coefficients are not read
- Lossy restoration was tuned on a few MP3s and synthetic walls, misses a hole shorter than its
  1 024-frame window, and leaves the first second and a half of an unstudied track unextended

## Later: Lyrics
- `Lyrics` is a flat list of lines: no word timing, no translation beside the original, and no
  source says which singer owns a line
- A sidecar's `[ti:]`, `[ar:]` and `[length:]` check reads a differently transliterated title as a
  disagreement

## Later: Listen and recognition
- Listen records one clip and asks once; nothing listens again on a miss or follows a stream from
  song to song
- Shazam is reached through an undocumented endpoint, so a change on its side stops recognition

## Later: Visualiser
- The spectrum's tilt, floor, band width and fall rates are constants, and its axis stops at
  20 kHz at every rate
- The scope has no level meters, correlation or goniometer, and triggers on the mid's rising edge
- The plot opens empty for up to a buffer's depth, because the tap runs only while the pane is in
  front

## Later: Scrobbling
- ListenBrainz gets no `playing_now`, a listen is stamped when it counted rather than when it
  began, and plays from before the token are never sent. Last.fm is not reached at all

## Later: MPRIS
- A row added and removed inside one 200 ms poll is never announced, and a shuffle is announced as
  the whole list replaced
- An id `unclaimed_id` minted for a file outside the library is never reconciled with the library
  row for the same file

## Later: MCP
- Nothing is pushed: resources cannot be subscribed to and no notification is sent while a pass
  runs, because the server answers on the one thread reading stdin
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit

## Later: Packaging
- gpui pulls `stacksafe` and with it `proc-macro-error2`, whose `E0365` future-incompatibility
  warning becomes a hard error in a future rustc. Only a `[patch]` or a newer gpui fixes it

## Later: Polish
- A tooltip names its key in no one wording, and the settings filter weighs a hint match the same
  as a title match
- A tooltip whose control moved under a perfectly still pointer stays up
- The search caret's blink ignores the desktop's cursor-blink setting
- `Wayback` restores a row rather than a pixel, so a list whose rows changed height lands a row out
- Midnight, Graphite and Plum share the same seven accents
- The minimise and maximise marks are `div`s rather than icons, and the application mark is
  written twice with only its paths held equal
- An icon already in `$XDG_DATA_HOME/icons/hicolor` that this build did not draw is never
  replaced, and only KDE's caches are flushed
- The album grid draws one frame at the old column count on a resize
- The statistics chart reads dates relatively, because nothing in the window draws a real date
- A saved query's direction is written into the `sort` column above the order codes, so an older
  build reads it as the wrong order rather than refusing it
- A drag on the equaliser's curve retunes the engine on every pointer move, uncoalesced
