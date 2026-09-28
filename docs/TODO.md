# Roadmap

Categories run from most to least important. Everything under a `Later:` heading is a nice-to-have
that no listener is waiting on, and is worked only once the categories above it are quiet.

## Playback and output
- A device with no volume of its own is still turned by the stream, so anything under 100 % leaves
  bit-perfect there
- Changing the graph rate mid-track reopens the stream and costs the gap a sink switch does, and
  so does the rate policy, the buffer or DoP wherever the change moves the stream's format or the
  ring's depth
- A stream's reported delay misses the frames in buffers it has already queued — `pw_time.queued`
  has no safe setter in pipewire-rs 0.10 — so the position and the visualiser's frame are short by
  up to one cycle
- `SinkInfo::current_rate` is the graph-wide rate from the settings metadata, so every sink reports
  the same one; a per-device rate is the driver node's own clock, which the registry publishes
  nowhere
- The playback loop holds one playback stream and one capture stream; more than one concurrent
  playback stream is not supported
- Nothing has proved a forced graph rate change against hardware — the only card here offers 48 kHz
  alone — nor DoP against a DAC that decodes it

## Formats
- A 32-bit stereo Monkey's Audio — integers or floats — is refused, because `ape-decoder` narrows
  the side channel to 32 bits before undoing it
- A `.wvc` correction file beside a hybrid WavPack is never opened, so the file plays and is billed
  as lossy: `symphonia-codec-wavpack` 0.1.1 reads a held zero's correction from the wrong range,
  and applying one waits on the crate being fixed upstream
- A source that cannot seek is read whole before it plays, up to 256 MiB, so a slow remote stream
  waits for its download; past that cap it is prescanned only through its head
- A file embedding a huge picture still costs one materialisation, because symphonia reads it into
  a buffer of its own before `probe_cover_art` can weigh it
- Opus mapping families 2, 3 and 255 are refused by symphonia's `OpusHead` reader

## Performance and scale
- Every frame the visualiser or the lyrics pane asks for is a whole-window paint on the GPU;
  gpui draws the scene whole, so only a newer gpui avoids it
- gpui 0.2.2 rasterises every batch of vector paths — the inspector's bitrate graph, the scope, the
  analysis plots, the equaliser curve — through a window-sized 4× MSAA texture it clears and
  resolves each frame, which costs an integrated GPU far more than the paths themselves. Only a
  newer gpui or drawing those plots without `paint_path` avoids it
- A track is opened on the engine thread, so a provider that is not the filesystem can still hold
  a track change for up to `Sources::OPENED_WITHIN`; a stream's reads have no deadline; and a
  picture asked for after its tags landed opens the file a second time

## Library
- A file moved by hand and retagged past every name it had starts again at nothing unless a study
  kept its print, and a play of a file no scan has seen keeps no listening time
- A play count reaches a file only beside a favourite and only in an ID3v2 `POPM`: lofty's generic
  popularimeter has no word for an unrated row and no counter outside ID3v2, and an APE tag holds no
  rating at all
- A cue row exported to PLS is its whole file, the format having no word for a region

## Identification
- A track named by its audio is placed by a rule rather than by the listener: nothing offers the
  choice of release for a track already held
- A file with no title tag and no artist tag, whose stem is neither numbered nor separated, gives
  the search nothing to ask with, so without an `acoustid-key` it is identified only by an ISRC or
  recording id
- Past `GROUPS_AT_MOST` release groups an artist's discography is read no further; the count
  left unread is said, and nothing reads the rest
- The verdict reads only the lowpass wall, the upsampled wall and the padded bits — not an MP3's
  frame-to-frame holes, sfb21 content or pre-echo — so an encode with no lowpass a wall can find,
  ffmpeg's AAC at 256 kbps among them, reads as lossless; no FhG encode was weighed
- AcoustID has never been reached with a real key; its fixture is written from the documentation

## Search
- A lyric reaches only a row the catalog holds; no keyless service indexes lyric text

## Testing
- The prescan's reading of which container symphonia opens first counts two MPEG layer III frames
  in a row as a stream; symphonia's own scoring of a sync word, and of MPEG-2 and layer I and II
  frames, is not reproduced
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
  24-bit rip loses to `flac -8` by some 15 % and is kept, and a 192 kHz rip is never a FLAC
- A kept object in MP4 or Matroska keeps the tags its container holds inside its structure

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
- When a line goes out is guessed from how long its text is wherever the sheet gives it no end —
  an LRC never does, only a Lyricsfile — so a held note can be put out under the dots early
- `Lyrics` holds no translation beside the original, and no source says which singer owns a line:
  a second voice is read off overlapping lines alone, and a third is folded onto the two
- A `.lyricsfile.yaml` beside a file is not read: the Lyricsfile reader is `resonate-online`'s,
  so neither `Sidecar` nor a build without `online` has one
- The Lyricsfile reader is not among the fuzz targets, which reach no crate that parses with serde
- A word being sung is faded in whole rather than wiped across letter by letter
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
- Plays from before the token are never sent to ListenBrainz, and Last.fm is not reached at all

## Later: MPRIS
- A row added and removed inside one 200 ms poll is never announced, and a shuffle is announced as
  the whole list replaced

## Later: MCP
- Nothing is pushed: resources cannot be subscribed to and no notification is sent while a pass
  runs, because the server answers on the one thread reading stdin
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit

## Later: Packaging
- gpui pulls `stacksafe` and with it `proc-macro-error2`, whose `E0365` future-incompatibility
  warning becomes a hard error in a future rustc. Only a `[patch]` or a newer gpui fixes it

## Later: Polish
- A tooltip names its key in no one wording
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
