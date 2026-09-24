# Roadmap

## Codec
- A gapless declaration is read from the boxes, so a source that cannot seek reaches it only where
  it lands inside `MAX_PRESCAN_HEAD`. An `.m4a` with a trailing `moov` piped in therefore plays its
  priming, which is the same head limit the `LIST INFO` note under Testing has
- A picture is weighed before `probe_cover_art` copies it, but symphonia has already read it into a
  buffer of its own by then, so a file embedding a 200 MB picture still costs one materialisation.
  Declining it before the reader allocates would mean a bound symphonia's own metadata readers take
- DST-compressed DSDIFF is refused rather than decoded, so a `.dff` whose `CMPR` chunk says `DST `
  names a file this build can see and not play. It is a decoder to write
- WavPack and Monkey's Audio have no decoder anywhere in the tree — symphonia carries neither, its
  `ape` feature being APEv2 metadata alone — so the desktop entry, the bus and the scan's extension
  list pass them over. Closing it is a decoder beside `opus-rs` in the codec crate's registry
- `opus-rs` runs SILK six samples early against libopus — bit-identical once shifted — and
  differs from it for a few hundred milliseconds wherever a stream moves between SILK, hybrid and
  CELT, which libopus's encoder does at the start of a low-bitrate stream. Music bitrates are CELT
  throughout and match libopus to 10⁻⁵; a speech-rate Opus file is the one that hears it. Both are
  the crate's to fix rather than this one's
- An Opus track in Matroska declares a length read off the segment's millisecond timestamps, up to
  1.5 ms off the exact count it decodes to. Exact would mean summing every block's Opus frame count
  out of its TOC byte in the prescan's cluster walk
- Opus mapping families 2, 3 and 255 — ambisonics and undefined layouts — are refused by
  symphonia's own `OpusHead` reader before the decoder is asked, so such a file does not open
- Nothing applies DSD's +6 dB modulation convention — its 0 dB reference is 50 % modulation — so a
  DSD master decimates about 6 dB quieter than the same music in PCM. Applying it would clip hot
  material, so it is a decision to take rather than an oversight
- DoP has never been proved against a DAC that decodes it. The markers are asserted byte for byte
  into the fake graph and the plan is asserted against every setting the pane can produce, but no
  hardware here takes a 176.4 kHz S24 stream

## The vault
- A kept object that is not FLAC — an MP3, a DSF — carries the tags its container was written
  with, because `bare_flac_head` only knows FLAC's metadata blocks and stripping the rest means a
  writer per format
- `flacenc` 0.5.1 caps the Rice parameter at 14 where the format's second partition method reaches
  30, which is why a 24-bit rip loses to `flac -8` by some 15 % and is kept rather than re-encoded.
  Closing it means the partitioned-rice-2 method in that crate or an encoder beside it
- `flacenc` also verifies `sample_rate <= 96_000` and `bits_per_sample <= 24` where the format
  holds 655 kHz and 32 bits, so a 192 kHz rip is weighed as a `Wave` and then usually kept
- A rip weighed as a `Wave` and then kept still pays most of a single-threaded zstd level-19 pass
  first, because `wave::compressed` gives up only once it has written what the source weighs: a
  five-minute 24/192 FLAC costs about 50 s of one core and is the whole of an import's tail.
  Spreading it over zstd's own workers changes the bytes and can cost ratio, and predicting the
  answer before the pass would be a guess about what the vault keeps

## Sources
- No provider reaches a network. `resonate-inbox` is the only one registered, so a want is filled
  only by a file someone put in the inbox under its MBID or ISRC, and the service links an
  `Identity` carries are read by nothing yet
- A delivered row belongs to no root, so nothing takes it away but deleting the catalog: no
  gesture forgets one, and a scan never prunes it. It carries no genre and no lyrics of its own,
  because the vault strips the tags the delivery came with and the row is written from the release
  track alone; its ReplayGain is what its study measured, so it is levelled only once studied
- A want the window's timer asked about waits out `POLL_AGAIN_AFTER` before the timer asks
  again, so a file dropped in the inbox an hour after lands five hours later unless *Poll now* or
  `resonate poll --again` is pressed. Nothing watches the inbox folder itself
- A provider or a stream left behind as late keeps its thread for as long as its `obtain` or its
  `read` runs. Nothing can take one back without the provider's help
- A provider is asked for bytes on the engine thread and on the tag reader, so a slow remote open
  stalls the track change it is part of, and nothing gives a `MediaProvider` a deadline. A row's
  name and its picture are one open only where the picture was already asked for when the tags
  are read, or where the file turns out to carry none; a picture asked for after its tags landed
  still opens the file again

## DSP
- The volume is the software gain stage alone, so anything under 100 % leaves bit-perfect. Whether
  a device has a hardware volume is read and drawn, and nothing drives it
- A packaged build ships the `x86-64` baseline and reaches only SSE2, where a local build takes
  `target-cpu=native` from `.cargo/config.toml` and 1.4x to 1.7x on the resampler's High with it,
  the gap widening with the ratio. Closing that gap for a shipped binary needs runtime dispatch,
  and `#[target_feature]` is `unsafe` and forbidden workspace-wide
- A track the lookup has not studied yet is turned down by nothing but the peak its tags declare,
  so a boost with no peak tag still leans on the per-sample limiter until the study lands, and the
  guard catches only what the chain processes: a bit-perfect or repacked stream of a file that is
  itself over full scale reaches the device as the file has it
- The true peak is read at eight times the rate through a 32-tap interpolator, which under-reads a
  tone at three quarters of Nyquist by up to 0.04 dB; nothing oversamples further, so an over that
  small can still pass the guard's ceiling
- Lossy restoration's constants — the droop shelves, the hole depths, the extension's slope — were
  tuned on a handful of MP3s and synthetic walls; no AAC or Vorbis file was weighed, and nothing
  listens for whether the rebuilt band is heard as air or as hiss. A wall found as a track plays
  arrives a second and a half in, so the opening of a track the lookup has not studied plays
  unextended
- Restoration works on a hole no shorter than its 1 024-frame analysis window, so a band an MP3
  encoder switches off for a single 576-sample granule is below what it can see
- There is no convolution stage, so room correction and a measured impulse response have nowhere
  to go where a parametric equaliser now does, and a `Processor` is still the seam one would land
  behind
- A GraphicEQ import is fitted at 48 kHz and realised at whatever plays, so the fit is exact at
  the rate it was solved at and drifts a little at the edges of the band elsewhere. It is within
  0.035 dB at 48 kHz and is not measured at every rate
- Nothing measures what an equalised stream actually peaks at, so *Fit the preamp* is a model of
  the curve rather than a reading of the music; the true-peak guard rides down what the curve
  pushes over, which is a gain that moves where a fitted preamp would have held still
- AutoEq is the only correction source, and `Corrections` is the seam a second would fill. A
  measurement is fetched one device at a time; nothing walks the index for everything plugged in
- A downmix folds by position and never by measurement: `Remix` reads `ChannelLayout::positions`, so
  a `Discrete(n)` source is truncated one channel for one rather than folded, and nothing reads the
  `DIALNORM` or downmix coefficients a broadcast stream declares for itself
- The equaliser switched on or off in place changes the response between one block and the next:
  the new chain's biquads start from a silent history and nothing crossfades the two responses, so
  a correction switched on under a loud passage opens with the transient of a filter meeting a
  signal already running. Nothing has measured how much of that is heard

## Analysis
- The verdict's thresholds were measured on one library — a few dozen FLACs and a handful of LAME
  transcodes. No FhG, AAC, Opus or Vorbis transcode was weighed, and an encoder whose lowpass sits
  between 19.5 and 20.7 kHz can only ever read as suspect
- Nothing reads an MP3's other marks: the holes a lossy encoder leaves above 16 kHz from frame to
  frame, its scalefactor-band-21 content, or the pre-echo around a transient. The verdict rests on
  the lowpass wall, the upsampled wall and the padded bits alone
- The pane's waveform and spectrogram are held for the run and never kept; only the summary study
  is in the catalog, so a track analysed once is decoded whole again after a restart
- A disagreement is flagged and nothing more: no gesture takes the recognised title and artist as
  the row's, and nothing asks MusicBrainz about what the audio was heard as
- The recognition is AcoustID's alone, and it has never been reached from here — there is no key in
  this build and the fixture is written from the service's documentation rather than captured. The
  lookup is a GET with the print in the query string, where the service prefers a compressed POST
  for a long print
- Shazam is reached through an endpoint it does not document, so a change on its side stops the
  listener naming anything until this build follows it; AudD is the documented fallback and needs
  a token, and AcoustID rarely matches a clip from the middle of a song
- A microphone recording has never been proved here against sound reaching the microphone — the
  desktop's monitor has, with a real track recognised through it
- The Listen sheet has been run in the window end to end — a desktop recording named by Shazam and
  raised as a notification with its cover — but the screen was locked while it ran, so its card
  has not been seen drawn, nor the microphone chips, nor the Online category's *Listening* group
- Listen records one clip and asks once. Nothing listens again on its own when a clip is heard as
  nothing, and nothing keeps listening to name each song as a stream changes track

## PipeWire
- Nothing survives the daemon going away. The core listener registers `done` alone, with no
  `error` handler and no reconnect, so after a restart every `Sync` goes unanswered, every
  enumeration fails with `LoopStopped` and the engine binds to the stale list until the process is
  restarted
- What a stream reports is the delay to the device plus the frames its resampler still holds, and
  not the ones sitting in the buffers it has already queued: `pw_time.queued` is the sum of
  `pw_buffer.size` over those, pipewire-rs 0.10 exposes no setter for that field, and writing it
  would take `unsafe`, which is forbidden. The reading is therefore short by up to what one cycle
  queued, and so is the visualiser's idea of which frame is being heard, which is read off it
- Nothing says which of the two twenty-four bit words is on the wire. `SinkFormats` collapses
  `S24LE` and `S24_32LE` to one `SampleFormat::S24`, so `resonate sinks` cannot report which one a
  device asked for, and `resonate explain` builds its plan without opening a stream, so it cannot
  report which one the graph will settle on — only the `FormatChanged` the callback already acts on
  knows, and that is not published. The listing reads `S24` beside `S16LE` and `S32LE` for the same
  reason: the depth has two spa names and neither of them is the depth's own
- A forced graph *rate change* has never been proved against hardware. `NO_CONVERT` has: a real USB
  DAC takes it and `pw-dump` reports the node negotiated at exactly the spec the plan asked for. But
  the only card here advertises 48 kHz alone and the graph already runs there, so nothing has yet
  made the daemon switch the graph under a stream and shown `Engine::downgrade` riding the
  `StreamEvent::FormatChanged` that comes back. It wants a device offering two rates
- Support more than one concurrent stream; the loop thread holds a single slot today
- The port a sink comes out of, the profile its card is switched to and whether its volume is the
  hardware's own are drawn by `resonate sinks` and by the settings pane's device list, but the pane
  has only been seen drawing them for sinks staged in the window: the graph here offers
  `auto_null` alone, so no real port has reached it yet. Only the card's *current* `Profile` is
  read; `EnumProfile`, which is every profile a card offers, is
  still unread, so nothing can offer to switch one
- `SinkInfo::current_rate` comes from the settings metadata, which is graph-wide, so every sink
  reports the same rate. The `Device` above is not the answer: its four params are `EnumProfile`,
  `Profile`, `EnumRoute` and `Route`, and not one of them names a rate. A per-device rate is the
  driver node's own clock, which the registry publishes nowhere
- `LatencyRequest::Frames` and `LatencyRequest::Duration` are never constructed: the engine always
  asks for `Auto`, so `node.latency` is whatever the daemon picks and the buffer setting sizes only
  the engine's own ring. The settings pane calls it a buffer depth, which implies the graph's

## Engine
- A `SinkChange` while a stream is open refreshes the list only once the ring holds more audio than
  the enumeration is allowed to spend, so a device that appears while a buffer shorter than 100 ms
  is playing still waits for the next stream open. The refresh it does take is capped at 50 ms
  rather than the 2 s a startup enumeration may spend
- The place is written only where the row changed or the position moved `KEPT_EVERY`, so a run
  killed rather than closed loses up to five seconds of position. Writing it on the way out would
  mean the teardown path taking the SQLite writer, which is what `signals.rs` deliberately does not
  wait on

## Lyrics
- A sidecar's `[ti:]`, `[ar:]` and `[length:]` check is only as good as what the file says about
  itself: a title transliterated differently from the tag reads as a disagreement, and a sheet that
  declares none of the three is read as it always was
- `Lyrics` is a flat list of lines. A karaoke-timed word, a translation beside the original and a
  second voice have no representation, and `Timing` is the whole of what distinguishes a synced set
  from an unsynced one
- The ten seconds a line may be lit before the set goes dim is a constant rather than anything the
  set declares, and nothing says how far through a line the transport is, because there is no word
  timing to say it with — a karaoke reading wants a richer `Lyrics` first
- Every line of a set is laid out on every frame, including the ones the falloff has taken to
  nothing, because their heights are what the scroll positions are measured from. A set of a few
  hundred lines is a few hundred text layouts at 60 Hz; gpui's `list` is the virtualising
  variable-height element nothing here uses yet

## Search
- Nothing sorts on a term: `SortOrder` is the nine the saved query's chips offer, and a term
  narrows rather than orders. `is:favourite` is the one reading the catalog has that is both, and
  it is a `Shape` and a `Favourited` order written separately rather than one thing read two ways
- The kept vocabulary is dropped by a row written in `tracks`, `albums` or `artists`, because
  `update_hook` answers per table and not per column — so counting a play drops a vocabulary no
  name moved in, and a listener searching between tracks pays the read again. Narrowing it wants
  `sqlite3_preupdate_hook`, which is a compile-time flag on the bundled SQLite
- Nothing bounds what the vocabulary weighs. It holds every distinct word *and* every distinct
  multi-word name across the three columns, held for as long as no name moves, and `holds`,
  `names` and `nearest` each walk the whole of it — bearable for a personal library and untested
  against the 500k-track one the scan is written for
- A run of tokens is weighed as a name from every start position it could begin at, so a query of
  T tokens costs up to T runs of `nearest_name` over the whole named vocabulary where a word at a
  time costs one. It is bounded by `MOST_TOKENS_IN_A_NAME` and by the search having matched
  nothing to be asked at all, and untested against the 500k-track library the scan is written for
- A lyric reaches only a row the catalog holds a file or a kept fetch for. LRCLIB's search reads
  the title, the artist and the album and not the words, and no service that indexes lyric text
  answers without a key, so a line typed from a song nobody holds finds nothing elsewhere
- The search MusicBrainz is asked for a song not held goes through the paced client the
  enrichment shares, so a lookup running beside a search holds the answer behind its own queue.
  Nothing says why the *Asking MusicBrainz…* heading is standing longer than usual
- A found song is wanted from the earliest-dated release its recording sits on, which is often a
  single or a compilation rather than the album a listener means. Nothing offers the choice
- A found song names the release it will be wanted from but the pane draws no cover for it,
  because the art is fetched only for a release the catalog holds

## Suggestions
- A suggestion's mosaic weighs two sleeves the same only where their bytes begin alike, so one
  sleeve saved at two resolutions — an edition and its expanded edition — fills two tiles
- An opened suggestion lists its first `PREVIEWED_AT_MOST` rows; *Play* and the rest act on the
  whole search, but a list of thousands cannot be scrolled to its end in the pane
- Nothing drives the Suggestions pane's press on a card: the opened view was seen by seeding
  `open_suggestion` at load, the lettered art by withholding every cover

## Library
- M3U, PLS and XSPF have no vocabulary for a region, so a playlist of cue rows exports as one path
  per row and comes back as one whole-file row per path. XSPF's `<meta>` and `<extension>` are the
  only extension point any of the three offers, and the reader ignores both by design
- A root that is itself one album — a single album folder added as a library root — has no sleeve
  below it, so an album of several singers filed that way still splits per track artist. The guard
  that keeps two albums loose in a root apart is what costs it
- A disc numbered in words is composed only in English — `Disc Twenty One` and `Twenty-First
  Disc` — where every other language reads a flat `ELSEWHERE` table capped at twelve and only
  after the noun, so `Disque Vingt Et Un` and `Zweite CD` name no disc. A folder is a string with
  no language on it, so telling the tables apart would mean guessing one
- A track's count is kept against the path, so a file renamed or moved by anything but
  `resonate organise` — which rewrites the row rather than letting the next scan find a new file —
  starts again at nothing, and
  a file no scan has seen counts nothing at all — a queue of unscanned files plays and is forgotten
- No *listing* orders on a count inside a window, only narrows on one. The Statistics pane answers
  what was heard most in one, because `most_listened` is its own read and free to sort on a
  `count(*)`, but every `SortOrder` is a column read off an index and a correlated count is not
  one, so the tracks pane still cannot be put in that order. A playlist's own count is still one
  date and one total with no history behind it
- Listening time is the time inside plays that *counted*. A track skipped at twenty seconds
  records nothing at all, so a session spent skipping reads as a quiet evening. Recording every
  partial visit would mean a `listens` row per skip, and the sampler would have to answer for a
  visit it had already decided was not a play
- Nothing writes a favourite back into the file, for the same reason nothing writes a count: no
  tag vocabulary every format shares, and `ItemKey` offers no way to write a name lofty does not
  already know
- Nothing ages or bounds the play history. `listens` takes a row per play for as long as the
  library lives and only a track leaving the catalog takes its rows with it, so nothing weighs a
  play from last week against one from ten years ago except by being asked for a window
- Nothing writes a play back to the file, so a count is this library's alone and a move to another
  machine leaves it behind. The seam is there now — `resonate tag` writes what the catalog was
  told — but a count has no vocabulary every format shares: ID3v2's `POPM` carries one, a Vorbis
  comment has no standard name for it at all, and `ItemKey` offers no way to write a name lofty
  does not already know, so a FLAC library would get nothing. Scrobbling under MusicBrainz below
  is the other half of the same question

## Tagging
- Nothing undoes a run: the files are the record, and the way back is another lookup and another
  `--apply`. `resonate tag` also reads every scanned row into memory before it plans anything, the
  same shape `resonate organise` has
- `.caf`, `.mka`, `.oga` and the two DSD containers have no writer behind them at any price, and a
  WAV whose ID3v2 tag stands in front of its `RIFF` header is refused, because lofty does not
  recognise one as a WAV
- A row a cue sheet cut out of a file is passed over whole, because twelve rows share one set of
  tags, so a single-file rip identified track by track keeps that identification in the catalog
  alone
- A picture is written only where the file carries none, so a file holding a thumbnail a ripper
  embedded keeps it however much better the cover the archive gave is. Replacing one would mean a
  reading of which picture is worth more than the other, and the bytes say nothing about that
- The two totals are the release's rows and media, so an album landed as its release group alone
  is written neither: a group names no pressing, and therefore no track count and no disc count

## Organising
- A cycle — `A → B` standing beside `B → A` — is refused where a chain is ordered, because
  breaking one needs a temporary name and a crash between the renames would leave a file under a
  name nothing knows. It converges over runs the way a chain used to, with nothing saying so but
  the count of collisions
- A copy across a filesystem boundary is not resumable. Every failure `copying` can see takes the
  half-written file away again, but a run killed outright leaves one standing, and the next run
  reads it as a destination already held and refuses the move as `Collided` rather than writing
  over it — which is the safe reading and leaves a file to delete by hand
- A sheet is rewritten only where its own bytes name the audio exactly once, so a `.cue` naming
  the file in a `REM` comment as well as in its `FILE` line, or one a tagger wrote in an encoding
  the new name has no letters in, is left as it was and goes on naming a file that is not there
- `resonate organise` reads every scanned row into memory before it plans anything, and nothing
  undoes a run: the moves are the record, and the way back is another layout and another `--apply`
- The files a sheet names one each are filed only where every destination is free: a unit is
  never ordered into a chain, so one whose file would land where another source stands is refused
  rather than waiting for that source to go

## MusicBrainz
- An artist with no `image` and no `wikidata` relation has no portrait and no way to one: the
  release group's cover is not used as a stand-in, and nothing reads the Wikipedia page a
  `wikipedia` relation names. Measured on a real library that is half the artists still unpictured
- A strict match is written without a pane to confirm it in or a gesture to undo it: the rule is
  `STRICT_SCORE`, the declared count and the owner by id or by folded name, and an album it takes
  wrongly can only be put right by clearing `albums.mbid` and the rows by hand and asking again
- The pass asks the Cover Art Archive for a cover only in the pass that landed the release or its
  release group, and only where the album held none, so a cover the archive gained later is
  fetched only by a `--refresh`, and a file cover taken away by a rescan is not replaced from the
  archive until then
- A contact typed into the Online card or `config.toml` is read once at start, so the User-Agent
  carries it from the next run; nothing rebuilds the three clients under a running window
- A file whose tags name no title is identified by its file name or not at all. `stem.rs` reads
  `NN - Artist - Title` and fills what the tags left empty, so `tagged_title` and `tagged_artist`
  carry that reading and `Route::Search` has something to ask with; a stem with no separator in it
  leaves `tagged_title` NULL, which refuses the search outright, so without an `acoustid-key` such
  a file can only be identified by an ISRC or a recording id it does not have
- A stem-named row is identified against its own file name, so re-probing it after it has been
  renamed moves `tagged_title` and `tagged_artist`, nulls `answered` and asks again from the new
  name — overwriting what the last lookup wrote. `resonate organise` alone does not trip it, the
  size and the mtime being unchanged, so it takes a rename and an edit together
- The discography is capped at `GROUPS_AT_MOST` of 1 000 groups and `worth_keeping` keeps albums
  and EPs alone, so a single an artist released is never listed as not held, and an artist
  credited on more than a thousand groups has the rest passed over without anything saying so
- The pressings of a group are fetched with their titles and countries and not kept: `settle_group`
  lands one and drops the rest, so nothing lists a release's other editions or says which pressing
  the catalog holds against which it could
- Scrobbling is not written down anywhere yet. `Listening` and `COUNTS_AS_HEARD` are already the
  hook a ListenBrainz or Last.fm submitter needs — a track heard, named, at a moment — and the
  paced `Client`, its User-Agent and its per-host interval are the same ones it would want. A
  submission queue has to survive a restart, so it is a table beside the cache rather than a
  channel

## UI
- The whole window is one `Render`. `RootView::render` is the only `impl Render` outside `Field`,
  `Hint`, the drag `Ghost` and the visualiser's plot, so any notify lays out and paints the whole
  tree. `Grain` keeps the position from asking for frames nothing would show, which took a playing
  window on the tracks pane from 11.5 % of a core to 2.7 %, but the lyrics, the visualiser and the
  inspector follow every poll and still rebuild the whole window sixty times a second, and each of
  the frames `Grain` lets through lays out the pane as well as the playback bar. The answer is a
  view of its own for the playback bar and the panes gpui can cache, which is a structural change
  to a front end nothing drives under test
- A reached row in the tracks or artists pane is marked and moved to, but nothing scrolls the
  album grid from the keyboard and the Missing pane answers no reach at all: its rows are
  interleaved with headings, so a reach over it would have to skip them
- The context menus are built from `menu::opens_a_menu` on the element that answers the press, so
  a right press on a row's own ✕, arrow or + reaches that control and not the row's menu. What
  each of those marks does is on the row's menu anyway, so nothing is unreachable; it is a
  surprise rather than a gap
- A menu's entries are decided when the press lands and never again, so a menu left open while a
  scan or a lookup moves the catalog underneath it offers rows against what was there. Pressing
  one is still sound — every entry acts on ids rather than on positions — but a *Go to album* for
  an album since gathered into another opens nothing
- `Wayback` keeps the row a list was left on, not the pixel, so going back to a list whose rows
  have changed height — an album's, where a disc heading may have appeared — lands a row or two
  out
- A saved query's direction is written into the `sort` column above the nine order codes, so a
  catalog this build wrote is read by an older one as the wrong order rather than refused. Nothing
  else reads that column and nothing older is expected to run, but it is a format widened in place
  rather than broken
- Nothing has scrolled the settings body by hand since it stopped being `w_full` under a `max_w`.
  The fix is the one `wsg`'s own notes record for the same taffy behaviour — a column whose height
  depends on how its text wraps is given a pixel width — and every category now reaches past a
  screen, but a screenshot cannot scroll and KWin offers no synthetic input
- A ramped palette's seven accents are the same seven in Midnight, Graphite and Plum, so those three
  differ only in their surfaces. Giving a recipe its own accent hues would mean deciding what green
  means in a palette that leads with violet, and green is what says bit-perfect everywhere else
- A tooltip names the key a control answers to in no one way: *Pause — space* names the action
  and then the key, *Repeat is off — r repeats the queue* names the state and what the key does
  next, *A sleep timer is running — press to change it* names the gesture and no key at all, and
  a settings field says *then press enter*. One wording for a hint that carries a binding, and one
  place that writes it, would make them read alike
- The settings filter matches a group's whole hint, so a long hint is a wide net: *dither* finds the
  noise-shaping group because its hint names dither. That is usually what is wanted and occasionally
  a surprise, and nothing weighs a title match above a hint match
- A tooltip whose control has moved under a stationary pointer — a list scrolled, the notice strip
  appearing, a row's columns giving way to a longer name — is tested against the bounds the control
  had when the hint was raised, because gpui freezes them into the check. A press, a scroll or any
  further motion takes it down now that the hints are not hoverable, but a pointer held perfectly
  still over the rectangle the control has left keeps it up
- A row's controls are keyed `("remove", index)` rather than by what the row *is*, so editing a list
  hands row N's tooltip state to whatever row N becomes. That is a wrong hint rather than a stuck
  one, and the next press, scroll or motion clears it; keying them by identity touches the drag,
  drop and reach paths
- The search caret blinks on a 500 ms timer that the field restarts on every edit, so it is solid
  while a word is being typed and pulses once the typing stops. The period is the field's own:
  nothing reads the desktop's cursor-blink setting, so a session that has turned blinking off or
  slowed it down is not followed
- Only the window's own marks are not icons: the minimise bar and the maximise and restore boxes
  are bordered `div`s, because no box is in the primary UI face
- The application's mark is written twice — `Icon::Resonate` as a mask and `packaging/resonate.svg`
  in colour — and a test holds the two to the same paths rather than either being derived from the
  other. Only the five paths are weighed, so the ground, the gradient and the transform around them
  could still drift without anything saying so
- An icon under the application's name already standing in `$XDG_DATA_HOME/icons/hicolor` that
  this build did not draw — the packaged one copied there by hand, as the `run-ui` recipe does — is
  never written over, so on such a machine the launcher keeps it whatever accent is worn. Only
  KDE's caches are flushed by name; a desktop that keeps its own lookups and watches neither the
  theme folder's mtime nor `iconChanged` shows the new colour from its next login
- The album grid draws one frame at the column count the last width gave before the canvas under
  it reports the new one, so a resize is a cell or two out for a frame; the first open costs a
  frame with no cells at all instead, the list being held back until the width has landed.
  Neither has been seen by anything but the eye — a screenshot cannot catch one frame
- What the queue was cleared or dropped of can be put back only while nothing has taken its place:
  queue a row after the gesture and the whole walk goes, because the row the rows came out of is a
  place in a list that no longer exists. There is no redo either — a walk that has been taken back
  cannot be taken forward again, where a playlist's `Undoable` holds both stacks
- A name drawn inside a fixed-width cell is clipped by the cell rather than elided by the text
  system, so it can be sliced through a glyph with nothing saying so. `opens` is `truncate`, which
  elides where the link itself is the box that runs out of room; where an ancestor is, the text
  never learns it is short. `ends_in_an_ellipsis` — a wrapping measure clamped to one line — is
  what the album cell uses instead, and it costs the collapse a nowrap link must not have, so it
  cannot simply be put on every link
- A control's hit area and the icon inside it are still two `theme` constants that have to be kept
  in step. The icon is drawn to a box now, so the pair no longer has a glyph's baseline between
  them, but nothing stops one being changed without the other
- The playback bar keeps its controls centred by clipping its side columns, so a narrow window
  loses the end of the signal path or the notice rather than moving the buttons. Nothing drops an
  item from the bar before it is clipped. The width it starts clipping at was about 1 200 px idle
  and 1 280 px with a sleep countdown drawn, and has not been measured since the signal path lost
  its repeated depth and rate and the queue button its count; at `WINDOW_MIN_WIDTH` the cluster
  overflowed by some 160 px
- The type-ahead answers in the queue and nowhere else. Statistics and Suggestions have no
  keyboard cursor at all — `reachable` answers `None` for both and neither is a `uniform_list`,
  one being charts over a scrolling div and the other a wrapping row of cards — so there is
  nothing in either to jump to. Giving them one means a `Listed` variant each, a
  `UniformListScrollHandle` each and arms in `reach_at`, `show_row` and `rows_a_page`
- `names_in_the_queue` resolves the whole queue on every keystroke of a jump, which is up to two
  SQLite reads a row the first time, on the render thread. A 4 096-entry `Recent` holds them after
  that, so it is one cold walk per queue rather than one per letter, and it is untested against a
  queue of thousands of rows no scan has seen
- The statistics chart folds days into wider bars past `BARS_AT_MOST`, so an *all time* window over
  years is several days to a bar. Its axis and each bar's hint read relatively — *today*,
  *9 days ago to 7 days ago* — because there is no date crate in the tree and nothing in the
  window draws a real date anywhere
- Nothing drives the panes under test. KWin exposes no synthetic input without the remote-desktop
  portal, so a pane can only be seen by temporarily making it `Pane::default()` and rebuilding, and
  a search only by seeding `LibraryModel::set_query` the same way — which is how the inspector's,
  the settings categories', a narrowed playlist's and the browse panes' *Reads* rows and empty
  messages were checked, the settings filter by seeding `RootView::finding` the same way, and the
  Tagging group's preview list by seeding `LibraryModel::retag` beside the reload. Anything that
  only appears on a pointer — the settings hints' tooltips, the folder picker, the playlists pane's
  Import, Export and Tidy controls, the file picker each of the first two opens, the second press
  *Apply* wants under Organising and Tagging alike, a span under a drag with the ghost saying how
  many rows it carries, the list
  scrolling out from under one and going on scrolling while the pointer is held at the edge, the
  magnified cover of a file the library never scanned, and a band pressed onto the equaliser's
  curve, dragged, turned by the wheel or taken out by a secondary press — has been seen by nobody
  but the person running it. The queue and an opened playlist were both seen by starting the run
  on them; `Span`, `Reach` and `Step::landing` are the arithmetic under the arrows, the drop and
  the keys alike, and `curve.rs`'s `Plot` is the arithmetic under the equaliser's handles, and all
  four are covered without a window
- A band on the equaliser's curve is reached by the keyboard only through its row's cells: the
  chosen band has no key that nudges its frequency or gain the way the pointer does, and the
  handles themselves are not in the focus ring
- A device's own equaliser curve can be exported but not kept as a named profile from the pane,
  and `resonate eq` prints, exports and unbinds one but cannot bind a device to its own curve or
  shape it. An own curve outlives its binding on purpose, so switching back finds it, but nothing
  ever removes the file of a device that is gone for good
- A drag on the equaliser's curve tells the engine once per pointer move that lands the band
  somewhere new, and nothing coalesces those to the rate the engine publishes at; each is a
  retune rather than a rebuild, but a high-rate pointer sends more of them than can be heard
- The sample rate policy, the graph rate, the buffer and DoP each reopen the stream, so changing
  one mid-track costs the gap a sink switch does. Switching the equaliser on or off while a
  resampler runs costs the same, because `OutputPlan::becomes_on_the_same_stream` refuses to carry a
  resampler's history and phase into a new chain and `retune` falls into a full `rebind`; handing
  the new resampler the old one's state would close that case
- A preview under Organising or Tagging is not taken down by anything that moves the catalog under
  it, so a scan or a lookup leaves a plan on screen that no longer describes what a press would do.
  Nothing wrong is written — *Apply* re-runs the pass rather than applying the plan it is standing
  beside — but the rows and the counts are the ones from before
- A session with no XDG portal has only `resonate scan` to add a library root — the settings pane
  reports the refusal and does nothing else about it — and a scan still blocks a second edit until
  it finishes, because one task slot carries both. `resonate scan` on the command line still says
  nothing until it ends
- Nothing asks the window for its scale factor, so the twice-the-cell a cover is resampled to is a
  guess that is exact at a scale of 1 or 2 and neither above that. A cover is decoded once and
  resized to all three drawn sizes whether or not the grid is ever opened, so the row and
  now-playing sizes pay for the grid's
- A settings control that has focus takes `space` and `enter` from the transport until escape lets
  go, which is the same bargain the search field has always made. Nothing on screen says the caret
  is in the pane rather than on a row, beyond the focused control's own accent border
- The visualiser opens on an empty plot for up to a buffer's depth, because the engine taps only
  while the pane is in front and what the ring already held when it opened was written unlistened.
  Tapping always would close it for about 0.006 % of a core at 48 kHz; the switch was taken
  because it cost nothing to arrange, not because the always-on cost was measured to matter
- The visualiser's frames are the whole window's frames. gpui dirties every ancestor of a notified
  view, so the pane's repaint is a `RootView::render` like any other, and it is cheap only because
  it rides the poll the window already redraws on while playing. Were the rest of the window cached
  views — the structural change the first item in this section asks for — the pane could repaint
  at the display's own rate for what its transform and its quads cost
- The spectrum's tilt, floor, band width and fall rates are constants rather than settings, and its
  axis is 20 Hz to 20 kHz at every rate, so a 96 kHz stream's ultrasonic content is analysed and
  never drawn. The levels carry no figures because a tilted reading is dBFS only at the pivot
- The scope draws left over right and nothing else: there are no stereo level meters, no
  correlation reading and no goniometer, and its trigger is the mid's rising edge, so a signal
  whose channels are out of phase triggers on whichever side the sum follows
- A `buffer-ms` written by hand past what the tap holds — about 21 s at 48 kHz, 2.1 s at 384 kHz,
  `LARGEST_TAP` frames less the slack and the widest window — leaves the frame being heard already
  gone round, so the visualiser reads silence. Every depth the settings pane offers fits
- Nothing has pressed the visualiser's *Scope* segment: the scope was seen by making it the default
  `Showing`, the same way a pane is seen by making it the default `Pane`

## Packaging
- gpui pulls `stacksafe` and with it `proc-macro-error2`, which re-exports the private `proc_macro`
  crate and triggers `E0365` as a future-incompatibility warning today and a hard error in a future
  rustc. Nothing here can fix it but a `[patch]` or a gpui that has moved on, so it is worth knowing
  about before a toolchain bump stops the build

## Tooling
- `resonate play` reads whole lines, so a key only lands on Enter and there is no live position
  readout. Raw mode would need a terminal dependency the workspace does not carry

## Testing
- Nothing covers `resonate-ui`'s panes; see the note under UI about driving them. What is tested is
  what needs no window to run — `Recent`, the search field's `Edit`, the curve's `Plot`, the
  spectrum, the reorder arithmetic, the settings filter, the sort chips and the type-ahead.
  What `Edit` cannot cover is the half that needs one: the shaped line, the caret's pixel position
  and everything the platform's input method hands to `EntityInputHandler`
- `cargo bench -p resonate-dsp --bench stages` prints what every stage costs, but nothing runs it
  but a person, so a figure the rules quote going away is noticed only when somebody looks. The
  packaged build's cost is `RUSTFLAGS="-C target-cpu=x86-64"` with a target directory of its own,
  and it is worth reading beside the native one, because the two do not rank the stages alike
- A Matroska `Duration` longer than the file's own clusters is not caught: `matroska.rs` counts where
  the last block starts, which is a lower bound, so blocks past the declared end prove a short
  declaration wrong while nothing proves a long one wrong. Catching it needs the last block's own
  length, which means the codec's frame size rather than the container's timestamps
- A prescan over a source that cannot seek reaches only the first `MAX_PRESCAN_HEAD` bytes, so a WAV
  whose writer put its `LIST INFO` after the `data` chunk still loses those tags over a pipe where a
  file on disc keeps them
- Nothing proves a press on a notification's button reaches the transport. `ActionInvoked` is
  matched on the notification server's own bus name, so nothing in the workspace can stand in for
  the desktop's daemon and send one; `pressed` and what the buttons are named are covered without a
  session, and the rest — the rule registered against the real server, a press landing on the id it
  handed back — has been seen by hand under plasmashell and by nobody else

## MPRIS
- `TrackAdded`, `TrackRemoved` and `TrackListReplaced` are all diffed from a 200 ms poll of the
  published queue, so two edits inside one sample coalesce into one announcement
- Nothing reconciles an id minted by `unclaimed_id` for a file outside the library with the library
  row for the same file, and a row the queue renamed to keep its ids apart is read back by its path
  rather than its id for as long as it is queued

## MCP
- The server is tools alone: no resource carries the catalog or the queue and no prompt is
  offered. No tool starts a scan, a lookup or a poll, which run long enough that a session could
  end in the middle of one
- An edit a model makes is not on the window's *Undo*: the library keeps its undo stacks in the
  process that made the edit, and `resonate mcp` is another process. The window draws the edit
  within a few seconds, but its own undo stack is not told, so undoing past it acts on rows the
  model has since moved
