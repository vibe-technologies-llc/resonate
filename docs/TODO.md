# Roadmap

Categories run from most to least important. Everything under a `Later:` heading is a nice-to-have
that no listener is waiting on, and is worked only once the categories above it are quiet. An item
marked **Blocked on …** waits on something outside this tree — hardware, an upstream crate, a
service or a format — and is not worked until that moves; everything else is open to be done.

## Defects
- A plain lyric sheet glides back to its first line six seconds after it is scrolled: `landing`
  answers the top wherever no line is being read, and `following` comes back after `HANDS_OFF`
- A press or an Enter on a Tracks row queues only the pages already loaded where the heading's
  *Play* queues the whole listing, so a 20 000-track library played from row 1 990 stops some
  two thousand rows later
- A *Listening history* chip ages the catalog the moment it is pressed, for good, while its note
  says listens are forgotten as the library opens; every other destructive setting is armed by a
  first press
- `resonate-online` declares `serde-saphyr` and uses nothing of it

## Privacy and security
- `config::laid_down` creates the staged file under the umask and copies a mode only from a file
  already there, so the first setting written makes a `config.toml` holding
  `subsonic-password`, `audd-token` and `listenbrainz-token` readable by every local user, and
  every later write holds them at the umask's mode until `set_permissions` runs
- The Subsonic password and the three tokens are drawn in plain text: `Field` has no masked mode
- `contact` rides on every request's User-Agent, Shazam, AudD, Spotify, Apple Music, Deezer,
  SoundCloud and GitHub included, where only MusicBrainz and the services it fronts ask for one
- Tag text reaches the terminal raw in every table but `info`'s — `stats`, `favourites`,
  `missing`, `playlists` and `play`'s readout — so a title carrying an OSC sequence or a newline
  retitles the terminal or forges a row
- `resonate mcp` reads a request with an unbounded `read_until`, so a client sending bytes with no
  newline grows the line until the process is killed
- `Vault::inside` and `holds` compare paths lexically, so `<root>/../x` passes; only `Vault::at`
  refuses a component that is not a plain name

## Robustness
- A release build aborts on a panic and nothing installs a hook, so a panic under `resonate play`
  on a terminal leaves it without echo or canonical mode until `reset`
- One unreadable value in `config.toml` — a pane renamed under `last-tab`, a `window-size` from
  another build — fails every command, `resonate sleep off` and `resonate mcp` included, where an
  unknown key only warns
- `resonate mcp` installs no signal handling, so a client ending a session with `SIGTERM` kills a
  scan or a lookup mid-file rather than stopping it at a file boundary as every headless pass does
- `resonate listen --seconds` is unbounded where `listen-for` is held to 4 to 60, and
  `Recording::holding` allocates the whole clip as atomics up front, so a large value aborts
- The PipeWire client needs the daemon at start: a failed first `connect` ends the thread and the
  binary exits, where a daemon lost mid-run is waited for and reconnected
- `Engine::fill` loops until the ring is full with no time slice, so a prime, a seek's refill or a
  large `SetBuffer` holds Pause and Stop behind it, and `wait_for_the_graph` enumerates the sinks
  inline for up to two seconds a pass
- A DSD read error or a DST frame that will not decode ends the track as though it had finished,
  because `fill` in `dsd/mod.rs` breaks on any error where every PCM codec raises one
- An undecodable packet is dropped with no silence in its place, so the output is short by the
  packet and a cut whose limit counts delivered frames runs that far into the next row
- The float WavPack restore calls `ones(shifted)` unmasked once `shifted` has climbed towards
  `max_exponent`, so a crafted `.wv` panics a debug or fuzz build on the shift; the extended path
  masks it
- The kept analysis cache trusts its own structure: `lanes` past `ENVELOPE_LANES` indexes out of
  bounds and a `frames_per_column` of nothing divides by zero in `Envelope::condensed`
- One NaN or infinite sample poisons the K-weighting filter's state for the rest of the track, so
  loudness and range come out `None` and the spectrum's sums go non-finite; the engine's
  equaliser already guards its state against the same
- The equaliser's one `_asked` task serves the import, the export, the catalogue read and the
  fetch, so starting one while another runs drops it and leaves `looking` set for the rest of the
  run, and the catalogue is never read again
- The vault import maps every error `keep` answers — a full disc, a failed rename — to
  `Passing::Unreadable` and goes on, so a full vault disc costs most of the work of every row left
  and bills each as unreadable; a cover with no format code ends the whole import where one
  `image` cannot read only warns
- `Vault::open` makes `audio/`, `covers/` and `staging/` wherever it is pointed, so a vault on an
  unmounted drive becomes an empty vault on the wrong disc
- The Discord socket search gives up at the first socket that answers `Close`, whatever the code,
  so a sibling `discord-ipc-N` or Vesktop is never tried; an `ERROR` frame during the handshake is
  dropped and waited out, and a reconnect is a fixed 15 s across some 120 candidate paths with no
  memory of the one that worked
- The engine's tag and cover catalog is never invalidated and caches a failed read as nothing, so
  a row retagged, or read while its share was unmounted, stays wrong until the LRU evicts it
- Engine events share one 256-slot channel that drops when full, and a sustained underrun raises
  an `Underrun` every pass, so it can push out the `QueueFinished` the headless `play` waits on
- `config.toml.lock` is made and never removed, and a staged config a killed process left is never
  swept

## Playback and output
- **Blocked on hardware:** A device with no volume of its own is still turned by the stream, so
  anything under 100 % leaves bit-perfect there
- Changing the graph rate mid-track reopens the stream and costs the gap a sink switch does, and so
  does the rate policy, the buffer or DoP wherever the change moves the stream's format or the
  ring's depth
- Pause, stop, a seek and the sleep timer cut the waveform where it stands and resume from a sample
  that is not zero, so each can click; nothing fades, and the sleep timer does not fade out
- Under a convolver the ring is sized to twice the impulse's tail whatever `buffer-ms` says — past
  `LARGEST_RING` and past the visualiser's `LARGEST_TAP` — so every start and seek renders that
  much before there is sound, and a volume or ReplayGain change is heard that late
- Under a convolver a reshape flushes the impulse's whole tail into the ring ahead of the music,
  and a reshape deferred for room waits on `free_frames() >= tail`, which the pump never leaves,
  so an equaliser switched on mid-track may never apply
- `swap_chain` flushes and stages a chain that is already draining, overwriting the tail `drain`
  is still writing, so a volume or equaliser change just after a track ends cuts its tail
- A convolution impulse is applied at the level it was written, with no normalisation or headroom,
  so a boosted one clips at the dither's clamp or pumps the true-peak guard
- With `device-volume` on, the slider writes one gain into every channel and `mute = false`, so it
  unmutes a muted device and flattens its balance
- `Command::SeekBy` past the end answers `SeekOutOfRange` rather than moving on the way the bus's
  `Seek` does
- `Command::Load` with no rows and `autoplay` leaves `playing` set with nothing loaded, so every
  toggle answers `InvalidTransition`, and `resonate play` with no file, or with a sheet that
  yields no row, waits for keys rather than saying so
- Repeating the queue under shuffle can play the last track of one pass first in the next, because
  the wrap reshuffles with no guard
- **Blocked on pipewire-rs:** A stream's reported delay misses the frames in buffers it has already
  queued — `pw_time.queued` has no safe setter in pipewire-rs 0.10 — so the position and the
  visualiser's frame are short by up to one cycle
- **Blocked on PipeWire:** `SinkInfo::current_rate` is the graph-wide rate from the settings
  metadata, so every sink reports the same one; a per-device rate is the driver node's own clock,
  which the registry publishes nowhere
- A sink's formats are enumerated once, at bind: its params are not subscribed to and a stale
  index is never dropped, so a port or EDID change that keeps the node leaves the negotiation
  reading the old list
- `SinkInfo::supports` refuses a format entry naming no channels where `candidates` reads it as
  any layout, so such a sink is never offered DoP
- A device removed before any Route param arrived keeps its proxy until the client reconnects
- The playback loop holds one playback stream and one capture stream; more than one concurrent
  playback stream is not supported
- **Blocked on hardware:** Nothing has proved a forced graph rate change against hardware — the only
  card here offers 48 kHz alone — nor DoP against a DAC that decodes it

## Formats
- A `.cue`'s `track_total` counts the rows of each `FILE`, so a sheet of one `FILE` per track
  bills every row as one of one, and a data track is counted with the audio
- A cue track whose `INDEX 01` will not parse starts at nothing and overlaps track 1 rather than
  being refused, and `FrameSpan::between` reads a start past the next track's the other way round
- `INDEX 00` is thrown away, so audio before track 1's `INDEX 01` belongs to no row and a pregap
  plays as the tail of the row before it
- A quoted cue value is cut at its first inner quote, so EAC's `TITLE "The "Real" Thing"` reads
  as `The `
- A sheet that is neither UTF-8 nor UTF-16 with a mark is read as Windows-1252, so a CP1251, GBK,
  Shift-JIS or Big5 `.cue` arrives as mojibake; a UTF-16 playlist sheet is refused outright, and
  RIFF `INFO` strings, EqualizerAPO profiles and lyric sidecars are read as UTF-8 alone
- A cue's `FILE` is matched to the audio exactly and by case, so `FILE "ALBUM.WAV"` beside
  `album.flac` or a backslashed subfolder is dropped with a debug record, and `Album.flac.cue` or
  `Album.Cue` is never found
- A FLAC with an ID3v2 tag in front loses its `CUESHEET`, because `flac::scan` wants `fLaC` at the
  start where `riff.rs` and `caf.rs` step past a tag, and the vault's `bare_flac` keeps such a file
  whole, tags and picture included
- `REM DISCNUMBER`, `TOTALDISCS` and `COMPOSER` are ignored, an album-level `SONGWRITER` is not
  handed to the tracks, and a FLAC `CUESHEET`'s ISRC and catalogue number are never read
- RF64, BW64 and Wave64 are refused as unrecognised, so a WAVE past 4 GiB will not open; symphonia
  reads none of them, and a reader of the crate's own would, as it does for Monkey's Audio
- `Codec::from_id` names no unsigned big-endian or planar integer PCM, so such a CAF is
  `Codec::Unknown` and billed lossy; symphonia's `adpcm` feature is off, so an IMA or MS ADPCM
  WAVE is refused
- A DSF's or a DSDIFF's cover in its ID3 `APIC` is never offered, `MAX_METADATA_BYTES` cuts a large
  tag short, and a DSD file's channels — DSF's channel type, DSDIFF's `CHNL` — are read as a count
- A tag given twice collapses to its last value, so two `ARTIST` or several `GENRE` entries show
  one
- Chapters — an m4b's, MP3 `CHAP`, Matroska's — are never read, so only a `CUESHEET` cuts a file
  into rows
- The Matroska cluster walk stops at the first cluster of unknown size, so a live-recorded mkv or
  WebM has no length and no seek bar
- A RIFF `ITRK` of `3/12`, and the fallback Vorbis `TRACK` and `DISC` fields, drop the total
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
- Clearing a field in a WAV removes it from the ID3 chunk alone while the reader takes `INFO`
  first, so the old value comes back on the next read
- `unpictured` removes the front cover alone where the reader falls back to any picture, so a cover
  typed `Other` survives its removal, and the reader can pick an icon or a leaflet ahead of a cover
- Every tag write copies the whole file and renames the copy over it, so a symlinked track becomes
  a regular file, hard links, ownership and extended attributes are lost, and a multi-gigabyte
  file is copied for every edit
- `TagField` holds 19 fields: genre, composer and the other credits, comment, BPM, compilation,
  grouping, copyright and ReplayGain are read and cannot be written

## Performance and scale
- `browsed` reads albums, artists, tracks, three favourites lists, the statistics and the
  suggestions — some twenty-two queries — on every search keystroke, page grown and favourite,
  and `take` swaps every `Arc` whether it moved or not
- The queue pane's length and its sort chips read the catalog for every queue row on the UI thread,
  past a 4 096-row cache, so a 20 000-row queue is some 40 000 reads that freeze the window
- Every setting written is a locked read-modify-write with two `sync_all`s on the UI thread, so
  *Reset everything* is some fifty of them in a row
- Undo snapshots every row of a playlist for each edit that moves rows, so one row added to a
  100 000-row playlist reads all of them under the write lock
- Move detection fingerprints its candidates — a whole decode each — inside the scan's write
  transaction, so a bulk move holds every other writer past its busy timeout
- `retag` holds each picture it replaces in memory for the whole run and writes it per file into
  `retagged`, which undo then reads back whole
- `measured` sorts a suggestion's whole result to count it, for up to sixteen candidates in series
- Per-row statements go through `execute` rather than `prepare_cached` — `store::touch`, the
  playlist inserts, `keep_resumption`, `keep_order` — so an incremental scan of 500 000 tracks
  parses 500 000 `UPDATE`s
- A track that will not decode is decoded again by every lookup, because a failed study writes no
  row
- An unmeasured track is decoded whole again on every `SetReplayGain`, `SetLevelling` and
  `SetTruePeak`, because `Measured::Unmeasured` is not remembered
- `same_shape_as` compares a convolver's impulse tap by tap on every retune — a volume tick —
  because `Arc<Impulse>` equality is by value
- The vault's FLAC path has no early out: a 24-bit rip that will lose is encoded whole, read back
  whole and then decoded twice more to be kept, and a duplicate pays the encode and the read-back
  before `Deduped` is known
- A vault row refused — `NoSmaller`, `NotValidated` — is weighed again at full cost by every
  import, with nothing stamping the refusal against `Encoding::OF_THIS_BUILD`
- `keep_cover` decodes a JXL whole to learn its size on every dedup hit, and `Unpacking` starts the
  stream again for any seek backwards
- MCP's `add_to_queue` costs one `AddTrack` round trip — and a settle of up to 500 ms — per row,
  a hundred by default
- The MPRIS playlists poll compares every playlist with every other every 200 ms, even where the
  `Arc` it reads is the one it read last time
- A cover for `mpris:artUrl` is written and `sync_all`ed on the poll thread, under the lock every
  `Metadata` read takes
- `submitting.rs` builds a new ListenBrainz client every two seconds, throwing its connections and
  its pacing away
- The equaliser's `peak_db` designs every band again at each of 256 points on every render of the
  Bands group
- **Blocked on gpui:** Every frame the visualiser or the lyrics pane asks for is a whole-window
  paint on the GPU; gpui draws the scene whole, so only a newer gpui avoids it

## Library
- A symlink to a folder inside the same root is walked twice under `follow_symlinks`, because only
  a link's target goes into `visited`, so every file behind it is a second row
- `tidy` and `prune_playlist` read a file on an unmounted drive as gone and drop its rows, and
  stat every file inside the write transaction
- The statistics bucket days in UTC, so *today* and *yesterday* are wrong everywhere else
- A resumption holding one row whose URI will not open is dropped whole, and its four reads run
  outside one transaction, so another process's keep can tear it
- The XSPF export escapes the five entities and passes control characters through, so a title
  carrying U+0001 writes a sheet VLC and Kodi refuse
- A timed M3U row probes its file again for every row
- A playlist export stages beside its target as `.new` and leaves it there on a failure
- `duplicate` picks a free name outside the write, so two processes copying one playlist collide
- A playlist sheet's path is canonicalised through links where a scan under `follow_symlinks`
  stores the link, so the imported row matches no track
- An artist suggestion is an FTS phrase, so a mix for *Air* holds *Air Supply*
- Every order ends on a column that is not unique, so paging with `LIMIT` and `OFFSET` can repeat
  or drop rows among ties
- The Missing pane stops at 5 000 rows without saying so, beside a count of every missing track,
  and a missing row or unheld release cannot be dismissed
- A volume retired for good keeps its rows: nothing forgets a folder inside a root, only a whole
  root
- **Blocked on the format:** A cue row exported to PLS is its whole file, the format having no word
  for a region

## Search
- A search made only of punctuation — *!!!*, *+/-* — matches nothing, and so does an empty saved
  query
- `length:3:30` is a float equality that matches only a track exactly 210.000 s long, where
  `added:` and `played:` read an exact value as a range
- `year:2000-1990` is taken as written and matches nothing, and `rate:44.1` means 44 Hz
- **Blocked on a service:** A lyric reaches only a row the catalog holds; no keyless service indexes
  lyric text

## Identification
- `analysis::print` stops decoding once the print is full, so a stream whose container declares
  no length is sent to AcoustID as two minutes long
- A clip Listen heard is sent to AcoustID with its own twelve seconds as `duration`, which the
  service reads as the whole recording's; its prints are of whole tracks, so a clip rarely matches
  at all
- A kept analysis is weighed against `WRITTEN_AS` alone, so a bump of `JUDGED_UNDER` judges the
  verdict again and leaves the loudness, the true peak and the print as they were
- `mix_down` sums every channel at equal weight, the LFE included, so a stereo file near −1
  correlation mixes to near silence and is `NotJudged`
- An encode with no lowpass a wall can find reads as lossless: ffmpeg's AAC at 256 and 320 kbps
  measured the same as its source by every spectral feature `analysis.md` lists, and no FhG encode
  was weighed
- **Blocked on a registered key:** AcoustID has never been reached with a real key; its fixture is
  written from the documentation

## Online services
- `online::client` builds a `Client` per call and the pacing lives on the client, so the
  reference, `ByEar`, `AcoustId` and the recognisers each keep a one-a-second slot of their own and
  together ask MusicBrainz faster than it allows
- A refused search is folded into nothing, so `album` goes on to the release group, the phrase
  and the words — each retried three times — before stamping the refusal, nothing stops a pass
  after refusals in a row, and a 502 or 504 is not retried at all
- A row answered more than a month ago is due again on every pass whatever came of the last ask,
  because `due_again` reads neither `asks` nor `refusals` for it
- A portrait nobody holds is asked for again on every pass — nothing remembers the miss — and a
  refusal from Commons, Wikidata or Wikipedia other than a 404 ends the walk before Apple, Spotify,
  Deezer or SoundCloud are tried
- A discography refused as its artist landed stays empty for `REFRESH_AFTER`, a month
- The poll stamps a want tried when every provider refused or ran late, so a Subsonic server that
  was down, or a wrong password, costs six hours after it is put right
- The poll does not stop asking a provider that is not there, so an unreachable Subsonic host
  costs its connect timeout for every want
- Subsonic is asked for 40 songs by title alone with no paging; a download is streamed without
  checking it is audio, so an error document lands as a song; its ISRC compare keeps the dashes;
  and it neither paces nor retries a 429 or 503
- The inbox delivers the first file whose stem matches whatever its extension, so `<mbid>.cue` or
  `<mbid>.jpg` is offered ahead of the audio beside it
- LRCLIB's search takes the first acceptable hit rather than a synced one or the nearest in length,
  and a lyric LRCLIB refuses is asked for again on every pass
- `has_front_cover` is read from MusicBrainz and never consulted, so a release it says has no cover
  is asked of the archive every month
- Wikidata's first P18 is taken whatever its rank, out of the whole entity
- ListenBrainz is sent neither the ISRC nor the release group the catalog holds, a favourite is
  never told as feedback, and a token is not checked when it is typed

## The window
- Deleting the last rows of the queue leaves the reach on a row that is gone, so the next Delete
  removes a row that was never highlighted
- Backspace removes the reached queue row once the type-ahead lapses, where a typo was meant
- The playlist picker, the magnified cover and the Listen sheet do not gate Delete, Enter or
  typing, which act on the pane behind wherever focus returns to it
- A second edit spawned into `_edit` before the first has run drops the first, its write and its
  toast with it, and `_passed` drops a pass the same way
- The queue jumps to the playing row whenever its index moves, so editing rows above it throws the
  view away from them
- Escape dismisses a toast before it closes an open menu
- An edit that changes nothing is kept in a field's history and clears its redo
- A copy blocks the UI thread up to 250 ms, and a copy thread that answers late can overwrite the
  fallback's clipboard
- The Listen sheet's source stays pressable while it records and then names the wrong one, and it
  says Online is off after Online was switched on in this run
- The Peak readout and *Fit the preamp* are worked at 48 kHz while the curve beside them is drawn at
  the stream's rate
- A frequency typed with a thousands comma is read as a decimal one, so *1,000 Hz* is 1 Hz, and
  *KHz* is not a unit it knows
- `Profile::magnitude_db` ignores a band's channels, so a band shaping the left alone is drawn as
  shaping both
- A value off a segmented control's table — a `bluetooth-lead-ms`, a pre-amp — lights nothing and
  says nothing, where `buffer-ms` and `listen-for` say what is in force
- The Online card says `contact` is used from the next start, where it is live
- `faint` text fails 4.5:1 against the panes in most themes and `muted` fails it against a raised
  surface in Frappé and Macchiato, which the palette test never weighs
- *Suspect* and *Lossy* are drawn in one colour

## Keyboard and accessibility
- Only Settings and the search field take focus: the transport, the sidebar, the heading buttons,
  the seek and volume rails and every row control are reachable by the mouse alone
- A menu opens on the right button alone, so a row's *Add to playlist*, *Go to artist*, *Share* and
  *Favourite* have no key
- A selection is a contiguous run reached by Shift, and Tracks and Albums act on its first row
  alone; nothing adds a row with Control
- Nothing mutes, seeks further than five seconds or opens the queue from the keyboard
- **Blocked on gpui:** Nothing is exposed to a screen reader; gpui carries no AccessKit

## Command line
- Logs go to stdout for every subcommand but `mcp`, so a warning lands in `resonate share >
  link.txt` and in a piped table
- Escape on `play`'s line is read only when the next key arrives, which is then taken as a command
- `play`'s input reads `f -9223372036854775808` through an overflowing `abs`, turns a seek past
  `i64::MAX` frames backwards, and seeks ten seconds on a number it could not read
- `resonate playlist --reverse` is ignored everywhere but `--order` and `--query`, and `resonate eq
  --off --for <sink>` switches the equaliser off for every device
- `info`, `explain`, `share <file>` and `playlist --add` take no `file://` URI where `play` and
  `queue` do, and `MediaLocation::from_uri` reads the scheme and `localhost` by case
- There is no `--bit-perfect` to undo the key for one run, and `play` has no flag for shuffle,
  repeat or the volume
- `scan` is always incremental and never follows links, and a cancelled scan exits 0
- A seek takes seconds alone, never `1:30`, and the sleep timer takes minutes alone, never `30m`

## Packaging and CI
- The Flatpak installs `resonate.desktop`, which flatpak-builder exports only under a name
  prefixed with `org.resonate.Resonate`, so the package has no launcher and no MIME types; renaming
  it breaks the `StartupWMClass` the window's `APP_ID` is tested against
- `packaging/.SRCINFO` still says `MIT`, an old `pkgver` and `pkgrel`, and lacks
  `libxkbcommon-x11`, where the PKGBUILD says `AGPL-3.0-or-later`
- The refusals stop at eight crates: `resonate-listen`, `resonate-lyrics` and `resonate-mpris` are
  held to nothing, `resonate-codec`, `resonate-dsp` and `resonate-pipewire` are never checked
  against each other, and the headless binary's `serde_json` guard in `dependencies.md` is run
  neither by the CI nor from the command list

## Testing
- The `probe` fuzz target never seeks, opens a span, decodes DSD to samples, hints an extension or
  reads a stream that cannot seek, so the seeks of `ape.rs`, `matroska.rs` and `dsd/` and the whole
  spooled path are unfuzzed
- *Take this name* is checked by eye alone: driving it wants a recognition the catalog holds, which
  no fake fingerprinter hands the analysis pane yet
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
- `vault_the_cover` clears `cover_art` without checking it is the picture it encoded, so a better
  archive cover landed during the encode is replaced by the old one
- A cancel is heard only between rows, so one long WAVE pass or FLAC encode cannot be interrupted,
  and the window imports on every core with no regard for what is playing
- A 24-bit hi-res source is written as a 32-bit WAVE, so the stand-in reports 32 bits where the
  catalog says 24
- A crashed import's staging is swept by `--prune` alone though its pid is in the name, and a
  landing does not sync the folder after the rename
- `Drawings::note` forgets and locks again, so two threads noting one key both push and a pruned
  cover can still be answered from memory
- **Blocked on `flacenc`:** `flacenc` 0.5.1 caps the Rice parameter at 14, the rate at 96 kHz and
  the depth at 24 bits, so a 24-bit rip loses to `flac -8` by some 15 % and is kept, and a 192 kHz
  rip is never a FLAC

## Later: Tagging and organising
- A cue-cut row is never written, and an album landed as a release group gets no totals
- `.caf`, `.mka`, `.oga` and the DSD containers have no writer: lofty writes none of them

## Later: Equaliser and DSP extras
- A profile name past 32 CJK letters falls back to `profile`, and a second such import overwrites
  the first, because `ProfileName::after` cuts by letters and `new` weighs bytes
- `ProfileStore::names` stops at 256 before it sorts, so which profiles are listed depends on the
  folder's order
- A 33rd EqualizerAPO filter refuses the whole file where every other unreadable line is passed
  over
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
- A row added and removed inside one 200 ms poll is never announced, and a shuffle is announced as
  the whole list replaced
- `PlaylistChanged` is raised for a playlist just made, where the spec keeps it for a name or icon
  changed
- A queued row carries no `mpris:artUrl`, so a playlist widget draws no cover for what is next

## Later: MCP
- Nothing is pushed: resources cannot be subscribed to and no notification is sent while a pass
  runs, because the server answers on the one thread reading stdin
- An edit a model makes is not on the window's *Undo*, since undo stacks live in the process that
  made the edit
- A JSON-RPC batch is answered with one `-32600`, though `2025-03-26` is negotiated and asks for
  batches
- `seek` answers the distance asked for rather than where the transport landed
- `start_scan` keeps its roots before the walk is known to start, so a refused scan leaves them
  standing
- `track_ids` has no cap where a search is held to `MOST_ROWS`, and a queue that fails partway
  says nothing of the rows that landed

## Later: Packaging
- **Blocked on gpui:** gpui pulls `stacksafe` and with it `proc-macro-error2`, whose `E0365`
  future-incompatibility warning becomes a hard error in a future rustc. Only a `[patch]` or a newer
  gpui fixes it
