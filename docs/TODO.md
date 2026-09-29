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
- Undo snapshots every row of a playlist for each edit that moves rows, so one row added to a
  100 000-row playlist reads all of them under the write lock
- A duplicate FLAC still pays its whole encode before `Deduped` is known, the key being the MD5 the
  encode lays the samples into
- A vault row refused — `NoSmaller`, `NotValidated` — is weighed again at full cost by every
  import, with nothing stamping the refusal against `Encoding::OF_THIS_BUILD`
- `keep_cover` decodes a JXL whole to learn its size on every dedup hit, and `Unpacking` starts the
  stream again for any seek backwards
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
- There is no `--bit-perfect` to undo the key for one run, and `play` has no flag for shuffle,
  repeat or the volume
- `scan` is always incremental and never follows links, and a cancelled scan exits 0

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
