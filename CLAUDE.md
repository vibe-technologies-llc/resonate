# CLAUDE.md

Guidance for Claude Code when working in this repository.

Detailed rules live in `.claude/rules/` and load automatically:

| File | Scope | Covers |
|---|---|---|
| `rust-style.md` | always | formatting, imports, the no-comment rule, collections, sync primitives |
| `errors.md` | always | the `thiserror` architecture and its structural enforcement |
| `dependencies.md` | `**/Cargo.toml` | version pinning, crate layering, feature flags that are not optional |
| `realtime.md` | engine, pipewire, dsp | the audio-callback contract |
| `audio.md` | engine, codec, dsp, pipewire | decode, transport, queue, what is published, DSP, sink |
| `library.md` | library, mpris, the playlist panes | schema, playlists, search grammar, undo, sheets |
| `lyrics.md` | resonate-lyrics, the lyrics pane | the vocabulary, the provider seam, the LRC reader |
| `eq.md` | resonate-eq, `core::eq`, the DSP stage, the equaliser pane | the vocabulary, the biquads, the per-sink binding, the formats, AutoEq |
| `ui.md` | resonate-ui | chrome, input, drawing, panes |
| `vault.md` | resonate-vault, `Library::import`, the settings pane's Vault group | the forms, the keys, validation, what the catalog holds |
| `online.md` | resonate-online, the binary's `online.rs` | the paced client, the identity, what each service is asked and how its answer is read |
| `providers.md` | resonate-providers, providers/*, `Library::poll` | the seam, the registry, the inbox, what a delivery is and where it lands |
| `analysis.md` | resonate-analysis, `studies.rs`, the analysis pane, `acoustid.rs` | the one decode pass, the fake-lossless heuristic, the print, the studies and recognition |
| `mcp.md` | resonate-mcp, the binary's `mcp.rs` | the transport, refusals against failures, the tools and the seam they reach the player through |
| `discord.md` | resonate-discord, `core::presence`, the binary's `discord.rs`, the Desktop groups | the gate, the seam, the frame, what an activity says and how often |
| `packaging.md` | `packaging/**` | the Arch, Fedora and Flatpak payloads and their build baseline |

## Project

Resonate is a music player for audio enthusiasts, targeting Linux first, with high-fidelity,
high-performance playback as the guiding constraint. Intended shape:

- A **native PipeWire client** (not via PulseAudio/ALSA compatibility layers) that reads the sink's
  advertised sample rates and performs **high-quality resampling itself** before handing the stream
  to the graph, so lossless sources stay bit-accurate where the hardware allows it.
- A **GPUI** front end running natively on Wayland.

`docs/TODO.md` is the roadmap and holds **open work only** — defects, gaps and what is not built
yet — written as `## Category` headings with `- Item` bullets. Keep it current: drop an item as it
lands, add what the work uncovers. What a landed item turns into is a record of the shipped design,
and that belongs in `.claude/rules/`, never left in the roadmap as something still to do.

`AGENTS.md` is the same guidance for agents that do not load `.claude/rules/` on their own: which
files to read, what is not negotiable and how the rules are kept true. It points here rather than
restating the design, so it changes only when a rules file is added, a check is added, or a
standing rule of how work is done moves — and then in the same commit.

## Architecture

Nineteen crates. `resonate-core` is the only universal dependency; `resonate-codec`, `resonate-dsp`
and `resonate-pipewire` never depend on each other, and `resonate-engine` is what joins them.

```
resonate            bin — CLI, tracing, wiring
  ├── resonate-ui         GPUI views, Wayland          [gated behind the `ui` feature]
  ├── resonate-mcp        the catalog and the running player, served to a language model
  │                       over the Model Context Protocol  [gated behind `mcp`]
  ├── resonate-mpris      the session bus: MPRIS, notifications, the idle inhibit, playlists,
  │                       the icon loader told the launcher icon moved
  ├── resonate-discord    what is playing, told to a running Discord  [gated behind `discord`]
  ├── resonate-engine     transport, ring, pipeline policy
  │     ├── resonate-codec    decode → PCM  (symphonia)
  │     ├── resonate-dsp      resample, gain, dither
  │     └── resonate-pipewire sink enumeration, negotiation, stream
  ├── resonate-online     MusicBrainz, covers, portraits, lyrics, AutoEq  [gated behind `online`]
  ├── resonate-library    scan, tags, persistence  → also on resonate-codec, resonate-vault
  ├── resonate-lyrics     lyric vocabulary and the provider seam  → drawn by resonate-ui
  ├── resonate-vault      the managed archive: FLAC, WAVE+zstd, JXL covers
  ├── resonate-analysis   one whole decode: waveform, spectrum, verdict, loudness, Chromaprint;
  │                       a clip's Chromaprint and Shazam signature
  ├── resonate-listen     capture the desktop or a microphone, and the recogniser seam
  ├── resonate-eq         profile formats, the profile store, the AutoEq catalogue and its seam
  ├── resonate-providers  the provider seam: an identity in, media out  → filled by providers/*
  ├── providers/inbox     resonate-inbox, the one provider this build registers
  └── resonate-core       domain vocabulary
```

The tree says what each crate is *for*, not the whole edge list: the binary also reaches
`resonate-codec`, `resonate-pipewire` and `resonate-vault` directly, `resonate-ui` reaches the
engine, the library, the lyrics and `resonate-listen`, and `resonate-online` reaches the library, the lyrics, the
codec and core while nothing but the binary reaches it. `resonate-mcp` reaches the library, `resonate-mpris`, the
engine's vocabulary and core, and nothing but the binary reaches it either; `resonate-discord` reaches core and the engine's
vocabulary alone, and is the binary's in the same way. `resonate-library` reaches
`resonate-providers` for the seam its poll walks and `resonate-analysis` for the studies its
enrichment takes, the engine reaches `resonate-analysis` to hand the window `Player::analyse`, and a provider crate reaches that seam and core
and nothing else of the workspace. `resonate-listen` reaches core and `resonate-pipewire` alone — it
starts a PipeWire client of its own for every recording, so a capture never shares the engine's
single-stream slot — and `resonate-analysis` reaches `resonate-dsp` for the true-peak meter and the
resampler a signature is taken at 16 kHz through. `cargo tree` is the authority.

Invariants the layering exists to protect:

- **A playable item is named by a `MediaLocation`, never by a path.** `resonate-core` carries the
  vocabulary: a `SourceId` and a `Locator` that is a `PathBuf` for local files and an opaque key for
  anything else. The id is an `Arc<str>` and the local one is interned, because a location is
  cloned once per scanned file, once per queue row and once per row a pane draws: a `Box<str>` made
  `MediaLocation::local` a malloc for the five bytes of *local* and a clone two allocations rather
  than one. `resonate-codec::Sources` is the registry that turns one into bytes —
  `MediaProvider::open` answers with a `Media`, and `LocalFiles` is the only provider this build
  registers. `probe`, `probe_stream`, `probe_boxes` and `Decoder::open` all take the registry and
  the location, and every `codec::Error` names the location rather than a path, so a failure from a
  source with no filesystem still says what failed. `Player::with_sources` puts one registry behind
  the engine and the tag catalog at once, and `crates/resonate-engine/tests/transport.rs` registers
  a provider serving a track out of memory and asserts the graph was handed the same bytes a local
  file would have produced.
- **An artist is one artist however its name is spelled.** `store::folded_letters` is what
  `artists.key` holds — the name lowercased, decomposed, stripped of its combining marks and with
  the letters Unicode does not decompose spelled out — so *Marcin Przybyłowicz* and *Marcin
  Przybylowicz* are one row billed under the marked spelling rather than two artists with half a
  discography each. `store::reconcile_artists` runs in `Library::build` and folds a catalog keyed
  the old way back together, keeping whichever row the enrichment hangs off. The same fold is what
  `tracks_fts` is written in and what a typed word is folded through, because SQLite's tokenizer
  folds `İ` to `i` and leaves the dotless `ı` alone, so *Kıskanç* and *KISKANÇ* were two different
  searches; an index written before the fold is a schema break with no migration behind it and
  wants the catalog deleted and scanned again. `library.md` has the rest, including why the rekey
  needs no sentinel.
- **`resonate-library` is the local source's catalog, not every source's.** It walks directories, so
  it stores paths and hands them back as local `MediaLocation`s; nothing in the schema is keyed by
  source. A source that is not the filesystem brings its own catalog, and a queue row from one is
  read through `Player::media` like any other unscanned row.
- **A URI names a source, not only a file, and one reader is the whole of how.**
  `resonate-core::MediaLocation::{from_uri, to_uri}` maps `file://` to the local source and any
  other scheme to a `SourceId` of that name; it lives in `resonate-core` because the bus and the
  command line both need it and neither may depend on the other. `resonate-mpris` refuses a
  location whose source is not in `Host::sources` — which is also what `SupportedUriSchemes` is
  built from, so the bus advertises exactly what this build can open rather than a hard-coded list.
  The binary reads every file argument through the same reader, so `Exec=resonate %U` hands
  `file:///music/Pink%20Floyd/Echoes.flac` over as the path it names rather than as a file of that
  literal name, and an argument that is a path is read as the file it names *from here* — the
  canonical path where it exists and the absolute one where it does not, so a file that is not
  there is still queued and still named. A queue row is published on the bus, kept for the next run
  and matched to a library row by what it holds, and none of the three can read a relative path:
  `to_uri` writes `file://track.wav`, which names a host rather than a file. A local path is escaped and
  unescaped as *bytes*, so a Latin-1 file name survives the URI it is written as rather than
  becoming U+FFFD on the way out and refusing to read back; only an opaque key is held to UTF-8.
  An argument is read as another source's URI only where its scheme names a source the build's
  `Sources` holds, so `01:intro.flac` is the file of that name here rather than a key under a
  source called `01`. **A row cut out of a
  file is named by its frames as well.** `to_uri_within` appends `#frames=START-END` — or
  `START-` for a cut that runs to the end — and `from_uri_within` reads it back; a `#` in a path
  is always escaped, so the fragment cannot be mistaken for a name. It is what `xesam:url` carries
  for a cue row, what `AddTrack` and `OpenUri` read, what `resonate queue` sends for a playlist's
  cue rows and for the rows a `.cue` it is handed cuts, and what `resonate share` looks the playing
  row up by — so a single-file rip reaches the bus as twelve tracks rather than one file twelve
  times. A `.cue` handed to `OpenUri` is read as its rows through the same `sheet_items` a
  `.cue` on the command line goes through.
- **The desktop entry, the bus and the scan advertise what this build can decode, and nothing
  else.** `MIME_TYPES` in the binary is what `SupportedMimeTypes` answers, what
  `packaging/resonate.desktop` declares and what the metainfo's `<provides>` names, held to both
  in both directions by a test each, and
  `resonate-library`'s `AUDIO_EXTENSIONS` names the same formats. Symphonia carries no WavPack, and
  its `ape` feature is APEv2 metadata rather than Monkey's Audio, so neither is offered; AAC is,
  bare ADTS included, because the `aac` feature registers `AdtsReader` as well as the decoder, and
  so is Opus — `audio/opus` and `audio/x-opus+ogg` — which symphonia demuxes and `opus-rs` decodes
  through a registry of the codec crate's own, `audio.md` having the rest.
- **The engine reaches the graph through `Backend`, never `PipeWire` directly.** `Player::new`
  starts the real client; `Player::with_backend` takes any other, which is what lets
  `crates/resonate-engine/tests/transport.rs` drive a whole transport with no daemon and assert on
  the bytes the graph was handed.
- **The CLI grammar is written once and read three times.** `crates/resonate/src/cli.rs` is the clap
  derive and nothing else, and `crates/resonate/build.rs` includes it as a module to write the man
  pages and the bash, fish and zsh completions into `OUT_DIR` at build time — so a flag added to the
  grammar is documented and completed with no second list to keep in step, and the Arch, Fedora
  and Flatpak packages install what they find under `target/release/build/resonate-*/out`. What it costs is a rule:
  `cli.rs` may name `std` and `clap` and no workspace crate, because the build script links neither,
  so `vocabulary.rs` is where a `QualityArg` becomes an `engine::Quality`.
- **`resonate-mpris` sees the engine only through `Player` and the front end only through `Host`.**
  It takes no dependency on gpui or the library, so the same service serves `resonate play` and the
  window. A media key is the desktop's to grab first: it calls MPRIS, so the interface *is* the
  feature, and the window binds the same keys only as a fallback for a session that does not grab
  them — see `ui.md`. It is started unconditionally, warns and carries on when there is no session
  bus, and falls back to `org.mpris.MediaPlayer2.resonate.instance<pid>` when another instance
  already owns the plain name. `TrackList` is the published queue read back: `Tracks` is
  `Player::queue` mapped to object paths — one path per row, because `Queue::load` and
  `Queue::insert` mint over any id a row arrives under that the queue already holds — `AddTrack`
  and `RemoveTrack` are `Command::Insert` and `Command::Remove` on the row that id names, and the four signals are diffed out of the same 200 ms
  poll that drives the `Player` property changes. `Tracks` is declared `invalidates`, as the spec
  asks, and the same poll invalidates it wherever the run of ids moved, so a client that caches
  properties reads the list again rather than holding the first one it saw. `GetTracksMetadata` answers *positionally* — one
  entry per id asked, in the order asked — and an id naming no queue row gets an entry carrying
  `mpris:trackid` and nothing else, because that field inside each entry is the only thing keying a
  reply to its request and a shorter list leaves the client unable to say which row went missing.
  A whole call is served under one `READ_BUDGET` of 500 ms, so a row whose tags have not been read
  by then answers with its file stem — and `Owed` is what remembers it: the poll watches
  `Player::media_revision` and, where a noted row's read has since *settled*, emits
  `TrackMetadataChanged` for it with what the call would answer now. Settled is the point:
  `Player::tags_read` answers `TagsRead`, the tri-state the catalog has always held and `media`
  flattened away, so a read that answered nothing — a file that has gone, a source that
  refused — is announced with the stem and taken off the list rather than owed for ever, and only
  a read still pending is waited on. A row that leaves the queue is forgotten rather than
  announced, so what is owed is bounded by the queue.
  What the spec says a method does is what it does, including where that is nothing: a `SetPosition`
  before the start of a track or past its end is ignored, a `Seek` past the end acts like `Next`,
  and an offset is read through `unsigned_abs` so `i64::MIN` saturates to the start rather than
  panicking. `mpris:artUrl` wants a URI where the catalog holds bytes, so `art::Pictures` lays the
  playing track's cover down under `resonate-art-<pid>-<n>/` in `$XDG_RUNTIME_DIR` — the
  temporary folder only where a session has none — named by a digest of its bytes so an album's
  tracks share one file, and hands back its `file://` URI. The folder is made 0700 with a
  `create` that refuses one already standing, so a predictable name in a shared folder cannot be
  taken first by another user to read what is playing or to aim a symlink, and each cover is
  opened `create_new` at 0600. Only the last
  `COVERS_KEPT` are held — a cover no kept track names any more is taken off the disc — and
  `Mpris::shutdown` takes the folder away again; a run that never got to, one the signal
  fallback's `process::exit` ended, is swept by the next run, which removes every
  `resonate-art-<pid>-*` whose process is no longer in `/proc`. Track ids are minted under
  `/org/resonate/track/` and playlists under `/org/resonate/playlist/`, because the spec reserves
  `/org/mpris` for its own paths, and `SetRate(0.0)` pauses, which is what the spec says a rate
  of nothing is. It is the playing
  track's alone: asking for every queued row would enqueue a cover read and a file write per row.
- **The interface is what a second `resonate` reaches a first one through, and the crate that
  serves it is the one that speaks it.** `resonate queue <files>` is `resonate-mpris`'s `Running`:
  it lists the bus names, takes `org.mpris.MediaPlayer2.resonate` or, where that is not there, the
  first `instance<pid>` under it by name, and calls `AddTrack` a row at a time. Where a row lands is
  a `Placement` and `Placement::row` is the arithmetic, the same two the window and the service
  already use; what the client adds is the turn from a row into the *anchor* the spec asks for,
  which is the track before it, `NoTrack` being the front. A run of files is added back to front
  against one anchor, because each lands immediately after it, so what was named first arrives
  first with no read between the calls, and `--play` is `set_as_current` on the last call, which is
  the first file. Living beside the service is what keeps one notion of a track path, one of a
  location and one of where a row goes; a client written in the binary would have needed its own.
  **The window is single for the same reason.** The window's host answers `CanRaise` and `Raise`
  — a `Raise` reaches gpui over `resonate_ui::Bus` and activates the window — and a launch that
  finds a player of this build able to raise, `Running::a_window`, hands it the files it was given
  as `AddTrack`s placed next and heard now, raises it and leaves, rather than starting a second
  window, engine and stream under an `instance<pid>` name that writes the same resumption. A
  headless `resonate play` cannot raise, so it never swallows a window being opened.
- **Which player a call reaches is a name, and one reading of the bus answers for every way of
  choosing.** `ours` is that reading: the names the bus holds that are `org.mpris.MediaPlayer2.resonate`
  or an `instance` under it, sorted, which puts the plain name first because it is a prefix of every
  other. `Running::found` is its first — the same player `resonate queue` always reached —
  `Running::listed` is all of them and `Running::named` is the one that `answers_to` a `PlayerName`,
  which is the whole bus name or the `instance<pid>` alone, so what `resonate players` prints is
  what `--player` takes. `Standing` is what one answers about itself — the name, an
  `Option<PlaybackStatus>`, how many rows it holds and the title and artist of what it is playing —
  and it is `Option`s throughout because a player that answers nothing for a field and one that
  answers an empty field are different things; a player that stops answering between the listing and
  the read is left out of the table rather than drawn with empty cells, because it is gone.
  `PlaybackStatus` is the spec's three and lives in `track.rs` beside the mapping from
  `PlaybackState` that writes them, so the property is written and read back through one vocabulary
  rather than through a string at each end — which `repeat_mode` never needed, `RepeatMode` being a
  lossless counterpart where `PlaybackState` is not.
- **A playlist is handed over as its rows, because the bus has no call that adds one.**
  `ActivatePlaylist` replaces a queue rather than extending it, so `resonate queue --playlist <NAME>`
  reads the rows out of the catalog here and sends the `AddTrack` run every file argument already
  takes — which is why it is a flag on `queue` rather than on `playlist`: `--next`, `--play` and
  `--player` are the queueing vocabulary and are written once. A playlist that fills itself from a
  search is read through `Library::playlist_entries` like any other, so it is queued as whatever it
  matches at that moment. `queue` is the one subcommand that reaches the catalog without playing
  anything, and it opens the library only where `--playlist` named one.
- **What the catalog has counted reaches the bus through `Host`, because the service may not see
  the library.** `xesam:useCount` and `xesam:lastUsed` come from `Host::heard`, which takes the
  row's `MediaLocation` *and its span* — a cue row is counted apart from the file it was cut out
  of, the same reason `Library::track_played` keys a play on the pair — and answers an
  `Option<Heard>`: `None` for a build with no catalog or a location it holds no row for, and
  `Some` with a count of zero for a row nothing has played, which are different answers and are
  written as different metadata. `xesam:lastUsed` is an ISO-8601 UTC stamp written by hand in
  `track.rs`, because no date crate is in the tree and one civil-from-days function is cheaper than
  one; it is total, so a `SystemTime` before the epoch converts rather than saturating. The playing
  track's reading is taken again the moment the row changes and otherwise at most once a second —
  `HEARD_READ_EVERY` — rather than on every 200 ms poll, which was a catalog read five times a
  second for a number that moves once a track; a client watching `Metadata` still sees the count
  move within a second of the play being counted, whichever process counted it —
  `a_play_counted_while_the_track_plays_is_announced_without_a_track_change`. The map the poll
  diffs is held behind an `Arc` and built again only where what it is made of moved — the row,
  its length, the `MediaInfo` the digest shares, the cover's URI and that reading — so an
  unchanged poll neither rebuilds nor compares it, and the playlists it lists are an
  `Arc<[PlaylistInfo]>` carried from poll to poll rather than cloned.
- **A play is still what was heard, and now it says how much.** `Listening` answers a `Counting`:
  `Counts` once a visit has earned its play, exactly where it always answered, and `Settles` once
  when a counted visit ends, carrying the whole time it was listened to. A visit that never
  counted answers neither, so listening time is the time inside plays that counted and a skip is
  not silently billed as one. `Library::track_played` hands back the `ListenId` it wrote and
  `Library::listened` spends it, so the two halves of one visit are the same row; a write that
  failed keeps no id, which is what stops a settle being attributed to the wrong track. The
  ending visit's row cannot be read off the queue, which has moved on by then, so `Listened`
  holds the `Played` it counted with and neither it nor `Listening` is `Copy`.
- **The sleep timer pauses, and what it is waiting for is a type.** `Command::SleepUntil(Option<Until>)`
  carries `After`, `EndOfTrack` or `EndOfQueue`, and `None` cancels. It pauses rather than stops
  because `Engine::stop` closes the stream and drops the decoder, and somebody who falls asleep
  should wake where they were; a timer that runs out against an idle transport clears itself
  rather than logging a transition it could never make. `budget()` is clamped to the deadline, or
  an idle transport's 100 ms park overshoots it by up to that much. The two end-of variants need
  no clock at all, only the edges in `skip` — and the wrap is the one worth naming, because under
  `RepeatMode::Queue` the queue never answers `None`, so `Queue::wraps_next` reads the same two
  fields `advance` decides the wrap from and `EndOfQueue` fires there rather than never.
  `PlayerState::sleeping` publishes what is left, so no front end keeps a clock of its own.
- **What MPRIS has no word for gets an interface of our own, not a stretched one.**
  `org.resonate.Player1` sits at the same object path as the four MPRIS interfaces and carries
  `SetSleep` and `Sleep`, because the spec has no vocabulary for a sleep timer and inventing a
  meaning for one of its properties would be worse than answering on a name that is plainly ours.
  The mode strings are one typed mapping in `track.rs` beside the one `PlaybackStatus` already
  is, so the property is written and read back through one vocabulary. The poll diffs the
  *published pair* rather than the `Asleep` behind it: `left` ticks continuously, so diffing the
  domain value would announce a property change five times a second for as long as a timer ran.
- **`Running` is a client, so a caller of it never learns D-Bus.** It knew only how to queue; it
  answers the transport now, and the queue, the playlists and the timer, each turned into a
  domain value on the way out — `seek` takes a `Seeking` and the two row calls take a `TrackId`,
  because an object path is `resonate-mpris`'s business and nobody else's. That is what a second
  remote control rests on rather than reimplementing the bus.
- **`Seeked` is the engine's own count of seeks moving, not a jump read out of the position.**
  `PlayerState::seeks` is a `Seeks` stepped by `Engine::seek` and only where the seek landed, so a
  refused one does not announce and a track change — which `start` reaches and `seek` does not —
  announces `Metadata` alone, which is what the spec says `Seeked` is not for. The poll emits it
  with the position out of the same sample the count came from. What it replaces is a heuristic that
  compared a 200 ms sample against the position extrapolated from the last one and called anything
  past 750 ms a jump: it missed every shorter seek, went blind whenever the rate changed, read a
  track change at the same rate as a jump, and read a paused transport sitting still as a jump
  backwards. It is also why a seek made from the window or the command line is announced at all.
- **The name is claimed with `AllowReplacement`, so losing it is a thing that happens, and the
  answer is the one the fallback already gives.** The connection subscribes to the bus's own
  `NameLost` *before* it claims anything, and a thread of its own reads the signal: on losing the
  name this process holds it does not ask for it back — `instances` claims
  `org.mpris.MediaPlayer2.resonate.instance<pid>` and the service goes on answering there, which is
  what `claim` already does for a name taken before we start. `Claimed` is the cell the two threads
  share, so `Mpris::name` answers with the name currently owned rather than the one first asked for.
  `Mpris::shutdown` hands the name back with `ReleaseName` and then closes the connection, which is
  also what ends the watch: the stop flag is set before the release, so the `NameLost` the release
  itself raises is read as the teardown it is rather than as a reason to take another name. All of
  it is idempotent, because `Drop` runs the same teardown a second time. `BusOp::ReleaseName` is the
  matchable half of `ClaimName`; `BusOp::Release` is the idle-inhibit portal handle and stays that.
  `crates/resonate-mpris/tests/name.rs` is a test binary of its own rather than part of `bus.rs`,
  because an instance name is keyed by the pid and a second process has a pool of its own — sixteen
  services in one process is what `cargo test` runs and nothing else does.
- **The desktop's two calls are a connection and a thread of their own, and the poll only leaves
  errands.** `notify.rs` raises `org.freedesktop.Notifications` on a track change and `idle.rs`
  holds the portal's `org.freedesktop.portal.Inhibit` while there is sound, and `desktop.rs` is
  where both are made: a second session connection carrying a 2 s `method_timeout` and a thread
  reading a bounded channel, so a notification daemon or a portal that does not answer stalls
  nothing queued behind it — the blocking proxy takes no per-call deadline, so on the poll thread
  one unanswered call held every property change for the bus's own 25 s. What the poll leaves for
  the inhibit is a *level* rather than an edge: `Errands::follow` writes whether the session should
  be held awake into an `AtomicBool` and nudges the thread, and the thread's `Gripping` weighs that
  level after every errand it runs, so a nudge dropped on a full channel is made good by whichever
  errand is behind it — a queued `LetGo` lost to a run of skips used to keep the session awake while
  paused, and a lost `Hold` let it suspend mid-play. `Awake::hold` refuses to ask again while a
  request is held, so none is overwritten and leaked. A failed `Inhibit` still counts as taken, so
  a session with no portal is asked once per play rather than five times a second, and a
  notification is asked for only when what it says changes. A
  notification is keyed on what it *says* rather than on the track: the cover lands a moment after a
  track starts, so the second telling replaces the first in place under the `replaces_id` the server
  handed back rather than raising a second popup, and `Mpris`'s own teardown closes it and waits
  `FAREWELL` for the thread to say it has rather than for a call that may never answer. The body is
  escaped only where `GetCapabilities` answers `body-markup`, because a server that does not read
  markup draws the escape itself wherever a name carries an ampersand. Neither is a setting — a
  desktop mutes an application by the `desktop-entry` hint the notification carries, and the idle
  inhibit is not a thing to want off — and a session offering neither service is a run of debug
  records rather than a failure, because a player that will not play is the only thing worth
  refusing to start over. **A notification is a setting all the same**, because a desktop's own
  mute is a blunt instrument for a player that is otherwise wanted: `Host::notifies` sits beside
  `Host::attended` and is weighed into one `Telling::of(attended, notifies)` the poll reads, so a
  track passed over is still *noted* as told either way and nothing pops the moment it is turned
  back on. The binary backs it with an `Arc<AtomicBool>` shared with the window — the mirror of
  how `Attention` carries the window's activation the other way — and `notify` is a key no CLI flag
  outranks, under the settings pane's *Desktop* category.
- **A notification carries the transport where the server draws buttons, and a press is answered on
  the connection that raised it.** `ACTIONS` is Previous, Play/Pause and Next, offered only where
  `GetCapabilities` answers `actions`, and `pressed` is the whole of what a key means. The listener
  is a thread on the desktop connection: `ActionInvoked` is subscribed against the notification
  server's own bus name and weighed against the id that server handed back, so another application's
  notification is not a command here. It ends when that connection closes at teardown, which is what
  `ManuallyDrop` beside the iterator is for — the subscription goes with the connection rather than
  being taken back over a socket that has already gone, which is a warning on every quit.
- **A notification is held back while the window is in front of the listener.** `Host::attended` is
  the reading the service has no other way to take: `resonate-ui` publishes the window's activation
  on a `Sender<bool>` the way it takes the quit `Receiver`, and the binary's `Attention` folds what
  arrives into the flag the host answers with. A track change passed over that way is still *noted*
  as told, so nothing pops the moment the window goes behind. A window is taken to be in front until
  it says otherwise — `OPENS_IN_FRONT` — because the window opens a second after the engine starts,
  so `resonate <files>` would otherwise pop a notification for the track it is opening to draw. A
  run with no window is never attended, so `resonate play` tells the desktop as it always did.
- **What Listen named is told through the same notification, and the service decides nothing
  about it.** `Mpris::teller` hands out a `Teller`, a bounded channel of `Told` — a summary, a
  body and an optional picture — that the poll drains beside its own track changes and hands to
  the desktop thread as `Errand::Tell`. It is weighed by the same `Telling::of(attended, notifies)`,
  so a song named while the window is in front is drawn in the sheet alone, and it is raised under a
  `replaces_id` of its own with no transport buttons, because it names a song that is not the one
  playing. The window never sees the bus: the binary's `listen::in_the_window` builds the
  `resonate_ui::Listens` — the `Listener`, the `Recognisers` `online::recognisers` registers, the
  `listen-from` and `listen-for` it opens on and a `tell` closure over the `Teller` — and hands it in
  on `Lookups`, so `resonate-ui` reaches `resonate-listen` for the vocabulary and nothing that
  speaks HTTP or D-Bus.
- **The equaliser's arithmetic is in `resonate-core::eq` and only its I/O is a crate.** The DSP
  stage, `resonate explain`, `resonate eq` and the settings pane's response curve must not
  disagree about what a band is, which is the argument `AppliedGain` already makes, and putting
  it in core is what keeps `resonate-dsp` a leaf. `resonate-eq` mirrors `resonate-lyrics`: the
  EqualizerAPO and GraphicEQ readers, the on-disk profile store, the AutoEq catalogue with its
  search and suggestion rules, and the `Corrections` seam `resonate-online` fills — on
  `resonate-core`, `thiserror` and `tracing` alone. A band's fields are quantised integers, which
  is what keeps `OutputSettings`' `Eq` derive and makes the text round-trip exact; the binding is
  per sink and the engine resolves it, because only the engine knows which device a track will
  open on. A binding names a kept profile or the device's own curve — `resonate_eq::Binding`,
  written as a name or as `true` — and an own curve is a profile file under `own/` that needs no
  name, so switching the equaliser on and pressing the settings pane's curve is the whole of
  shaping the sound. The curve is shaped where it is drawn: a press adds a band, a handle drags,
  the wheel turns its Q, and `views/settings/curve.rs` is the pointer arithmetic, proved without a
  window. An equaliser in force makes the plan `Converted` and a DoP-packed stream refuses it
  outright. `eq.md` has the rest.
- **What the catalog was told is written back through a seam of its own, and this build writes
  only what it will read back.** `resonate-codec` carries the writing beside the reading:
  `TagField` is the vocabulary of a writable field, `TagEdit` is one of them with the value to
  write, `Writing` is the run of edits and the optional front cover that ride into one call,
  `TagSink` is the seam over `TagSource` and `FileTags` is the whole of what is behind it —
  lofty writes and symphonia still reads, because what a write is weighed against has to be what
  the rest of this build sees. A picture rides into the same call rather than costing a second pass
  over the file, and it is the catalog's cover offered only where the file carries none — the same
  rule `albums.cover_source` already states from the other side: the file's wins. A format whose
  tags this build would not read back is refused rather than written, which is the same promise the
  desktop entry makes about decoding: AAC, AIFF, FLAC, MP4, Ogg Vorbis, Opus and WAV are what
  `FileTags::writes` answers for — a WAV because `riff.rs` reads the `id3 ` chunk lofty writes
  into, which symphonia's reader skips — and `.caf`, `.mka`, `.oga` and the two DSD containers,
  which lofty has no writer for, are passed over. `Library::retag` is the pass
  behind `resonate tag`, a preview until `--apply` says otherwise the way `organise` is, and it
  takes the same `Walk` guard, because a file whose tags have moved is a file the next scan would
  otherwise re-probe against a stale size; the settings pane's *Tagging* group under Library is the
  window's way in and takes the same preview-then-arm shape *Organising* does. `library.md` has the
  rest, including why a name is written only where a lookup answered for it and why the catalog
  follows the file rather than the other way round.
- **The vault is a managed archive, and the library it imports from is read and never written
  to.** `resonate-vault` decodes a track, throws every tag and picture away, keeps the smallest
  bit-exact copy it can make and validates it by reading the PCM back through the same decoder the
  player uses; `Library::import` is the pass that walks it under the `Walk` guard and points
  `tracks.vault_path` at what landed, while `tracks.path` goes on naming the original — which is
  what keeps a rescan reading an untouched source as unchanged and lets `organise` go on filing
  the library it came from. Three forms: `Flac` for integer PCM a 24-bit, 96 kHz, 8-channel
  encoder holds, `Wave` for float and anything wider, zstd'd, and `Kept` for a source re-encoding
  would lose or bloat — a lossy codec, DSD, or a re-encode that came out no smaller, which is the
  rule that makes **nothing the vault writes larger than what it came from**. A kept FLAC still
  has its metadata blocks rewritten to STREAMINFO alone, so the promise about tags holds without
  touching an audio frame. Covers are lossless JXL, one per distinct picture rather than one per
  track, and `Vault::picture` hands them back as PNG so `resonate-ui` and `resonate-mpris` need
  know nothing about the format. A vaulted row is still named by its own file everywhere — its
  plays, its playlists, the bus and the resumption — and the vault stands in for it only when
  bytes are wanted: `Library::stand_in` is a `resonate_codec::StandIn` the player's `Sources`
  asks before it opens a row, which answers the object and the catalog's tags, ReplayGain and
  lyrics included, and `VaultFiles` replaces `LocalFiles` under `SourceId::local()` to decompress
  a `.wav.zst` on open. `Library::poll` lands a provider's delivery through the same
  keep, so what is obtained later arrives validated and deduped.
  `vault.md` has the rest, including why three of the encoder's bounds are flacenc's rather than
  FLAC's.
- **Lyrics come out of the file, and one LRC reader is the whole of how.** `resonate-lyrics` carries
  the vocabulary, the `LyricProvider` seam and the `Lyricists` registry that walks it, and takes no
  dependency on gpui, the engine or the library, so the whole of it is tested without a window.
  `lyrics.md` has the rest.
- **The network is behind `Reference`, and the library owns the seam.** `resonate-library`'s
  `reference.rs` carries the vocabulary — `Release`, `Medium`, `ReleaseTrack`, `ArtistProfile`,
  `LookupOp` — and the `Reference` trait, and `Library::enrich` is the
  pass that walks it, so the catalog knows what a release and an artist *are* without knowing
  where an answer came from. `resonate-online`'s `Online` is the one implementation, and only the
  binary depends on that crate, behind the `online` feature: `--exclude resonate-ui
  --no-default-features` therefore builds with no HTTP client in the tree, and
  `cargo tree -p resonate-library` stays free of `ureq` and `serde` the way `resonate-core` does.
  The pass is proved in `crates/resonate-library/tests/library.rs` through a `Fake` reference that
  answers canned releases and faults on the call it is told to; the online crate proves its
  mapping over captured fixtures and reaches the services only under `RESONATE_ONLINE_TESTS`.
  `library.md` and `online.md` have the rest.
- **A request says what this build is and nothing else, unless the listener says otherwise.**
  `Identity::user_agent` is `resonate/<version>`, and a contact is appended only where the
  `contact` key in `config.toml` holds one: `Identity::of_this_build` carries none, `Config::contact`
  is `None` for a blank value, and the settings pane's field writes the key through the same seam
  every other setting takes and clears it rather than writing an empty string. It is read once at
  start, so a change is sent from the next run. `online` is the switch beside it:
  `Config::online_enabled` defaults to true, `online::reference` answers `None` where it is off,
  and `Error::OnlineOff` is what `resonate enrich` says then, where a build without the feature
  says `Error::NoReference`. `acoustid-key` is the second thing a listener types in and the build never
  carries: AcoustID answers only a registered client key, so with the key empty no fingerprint
  leaves the machine and `online::fingerprinters` registers nothing but the stub. `audd-token` is
  the third, and AudD is sent a clip only where it is set; Shazam asks for no key and is sent only
  a signature — the peaks of what was heard — and only when a listener asks to listen.
- **A guess is never written; only a strict match is, and the grouping key never follows it.**
  `enrich.rs` takes a release search only where the top hit scores `STRICT_SCORE` or over,
  declares the count the files declared — `TRACKTOTAL`, or the rows held where none did — and,
  where the album has an owner, is credited to that owner by the id the tags gave or by a name
  that agrees with it; a hit strict in everything but the count names its release group instead,
  and the group's nearest pressing is what lands — the smallest wider than the rip, or the widest
  no wider where there is none — so the rows the rip is
  short of are listed as missing rather than guessed at. An artist is taken only at `EXACT_SCORE`
  with a name that agrees; anything short
  stamps `asked` and nothing else, so it is asked again once the wait its `asks` has earned is
  out — `RETRY_AFTER` doubled per ask nobody answered, capped at a month — and a landing puts that
  wait back to where it started. A refusal is not one of those asks: `albums.refusals` counts the
  asks in a row the service answered with its bad day rather than with a name nobody has heard of,
  so the row waits `REFUSED_AGAIN_AFTER` — an hour — doubled per refusal under the same cap, and
  the `asks` doubling stays where it was. A miss or a landing puts the count back to nothing.
  A name agrees in one of two folds — `folded_title`, which keeps its marks, and `stripped_title`, which does not — and
  `top_of` weighs the agreement above the score, so *Marcin Przybylowicz* is identified against
  the marked spelling MusicBrainz answers with while two artists differing only by a mark stay
  two. What a match writes is `albums.mbid`, the release's rows and, where two albums turn out to
  hold one release, a gathering: a grouping key is a *name* for an album rather than a column on
  it — `album_keys` holds any number per album — so `gather_under` hands the loser's tracks, rows
  and keys to the survivor and deletes it, and the next scan finds each folder's own key still
  naming the album it was gathered into. `tracks.album_id` is the only membership there is, which
  is what makes that three `UPDATE`s rather than a re-grouping.
- **A pass says it is running, and the next window carries on what was left.** The `enrichment`
  table holds one row while `Library::enrich` walks its queue, carrying the `refresh` it was
  started with; a pass that reaches the end of its queue or is stopped by the listener takes it
  away, and one the reference ended — or one the process never got to finish — leaves it standing
  for `Library::unfinished_enrichment` to read back. `LibraryModel::new` asks at start, so a
  lookup killed halfway through a library goes on where it stopped rather than waiting for a
  gesture. Pictures are fetched beside the pass rather than in it: `Pictures` is two threads
  reading a channel of covers and portraits, so an archive that answers slowly costs the
  MusicBrainz queue nothing, that service's one request a second being what a pass is really made
  of.
- **A cover says where it came from, and the file's wins.** `albums.cover_source` is
  `CoverSource::File` or `Archive`: `land_archive_cover` writes only where `cover_art IS NULL`,
  and `store::cover` on a rescan probes again wherever the held picture is not the file's, so
  one the archive gave is replaced by one a file turns out to carry and never the other way
  round. The pass asks the archive only in the pass that landed the release or its release group,
  and only where the album held none.
- **A track is asked about in its own right, and what a lookup may overwrite is a type.** A file
  carrying no `ALBUM` tag is under no album, so the enrichment reached it through nothing at all
  until `Ask::Track` joined the queue between the albums and the artists. `Pass::track` walks four
  routes, most exact first — an ISRC, a recording id, a text search, a fingerprint — and
  `Certainty::{Exactly, Nearly}` is what each of them may write: a name the file never gave is
  always filled, and a name it *did* give is corrected only on the strength of an identifier the
  file itself carried, never on a score. `tagged_title` and `tagged_artist` hold what the scan read
  the file as, which is how a rescan tells a retagging apart from an identification; they and
  what the tags declare about a release — the barcode, the catalogue number, the label and
  `TRACKTOTAL` — are the only enrichment columns a scan writes. `Fingerprints` is the fourth
  route's seam, filled by `AcoustId` where an `acoustid-key` is set and read from the print the
  track's study took; an album no pressing is as wide as is landed as its release group, which is weighed
  without a track count because a group has none. `library.md` and `online.md` have the rest.
- **`resonate organise` plans before it moves, and the preview is the plan.** A `Layout` read from
  the `organise-as` key, or from `--as` for one run, is segments split on `/` *before* the pieces
  inside them are read, so a separator can never reach a value and neither a template nor a tag can
  name a path outside the root the file came from — which is why `Refusal` has no `Escapes`
  variant, nothing being able to construct one. One `Plan` is built and then handed to the apply or
  not, so a preview and an `--apply` cannot disagree about what would happen; the moves are ordered
  so that a chain lands in one run and only a cycle is `Collided`, the files move in batches of 256
  with a rollback that renames back what was renamed and removes what a cross-device copy wrote,
  and the catalog follows each batch, because a moved file the catalog has not followed is what the
  next scan already reconciles while the reverse is a catalog naming files that are not there.
  A rename across a filesystem is a copy whose source goes only once that write has committed, and
  a `.cue` cutting one row out of a file has its `FILE` line rewritten after the batch, in the
  encoding the sheet was written in. `Library::scan` and `Library::organise` take the same `Walk`
  guard and a second caller gets `Error::AlreadyWalking`: a scan's snapshot of what the catalog
  holds, with a pass rewriting paths underneath it, silently destroyed rows. `library.md` has the
  rest.
- **What identifies a recording is core vocabulary, because a provider crate may not see the
  library.** `Mbid`, `Isrc`, `Link`, `Relation` and `Service` live in `resonate-core` and the
  library re-exports them, so every `resonate_library::Mbid` path still reads. `Service` is what
  was the library's `Provider` enum — the host a link points at, Tidal or Bandcamp or Discogs —
  renamed so that *provider* means one thing in this tree: a plugin that obtains media. The link
  tables keep their `provider` column, which is a stored name rather than a word anyone reads.
- **A provider is a crate that turns an identity into media, and everything around that is the
  poll's.** `resonate-providers` is the seam, on `resonate-core`, `thiserror` and `tracing` alone:
  `Identity` is everything the catalog knows about a wanted row — the recording, track and release
  MBIDs, the ISRC, the title, artist, album, length, disc and position, and the service links the
  enrichment stored for the track and for its release, read through `track_on` and `release_on` —
  `Provider::obtain` answers an `Obtained`, and a `Delivery` is a `File` already on disk or a
  `Stream` of bytes with the provider's own key and an `Extension`. `Providers` is the registry,
  `Providers::none()` holding the `Unprovided` stub and `and` registering one per name the way
  `Lyricists::and` does. A plugin is a crate under `crates/providers/` that depends on the seam and
  core, and the binary's `providers::registered` is the one place any is registered — in code, from
  `Config` — so adding one is a crate, a workspace member, a dependency of the binary and one
  `.and(..)`. `Library::poll` does the rest: asks each want's `Want::identity` of every provider in
  turn, counts a refusal and carries on, lands a delivery in the vault — a file through
  `Vault::keep`, a stream through `Vault::keep_delivered`, which stages it under a cap and throws
  the staging away whatever happens — makes what landed a track row paired with the release
  track it was wanted for, and stamps the want with where the bytes now are. **A delivered row
  belongs to no root**: `tracks.root_id` is nullable, the row is named by the vault object's own
  path, and every pass that walks the user's files joins `roots`, so none of them moves, retags,
  re-imports or releases what only the vault holds. `resonate-inbox` is the only provider there is: a folder named by the `inbox` key, read and
  never written to, answering a file whose stem is the recording MBID, the track MBID or the ISRC
  and never one that merely shares a title. `providers.md` has the rest.
- **A favourite is a timestamp, a genre is a folded column, and a pin leads every order.** All
  three are columns on tables that already existed, so all three were a schema break and the
  catalog was deleted and scanned again — the rule then; a change now is a step in `MIGRATIONS`
  that carries the catalog forward, as `library.md` says. A
  `favourite` is nullable and holds *when*, not whether, because it costs the same and orders
  "recently favourited" for nothing; `Library::favour` takes one typed `Favoured` rather than
  three functions, `is:favourite` narrows on it and a `Favourited` joins each of the three order
  enums — the track one needing `tracks_by_favourite` declared exactly as its `ORDER BY` reads,
  because `every_order_the_panes_offer_is_read_off_an_index_either_way_round` refuses a plan that
  sorts. `tracks.genre` is the tag the scan always read and threw away, and the fourth
  `tracks_fts` column folds it together with the genres MusicBrainz gave the artist, so
  `genre:rock` reaches a file whose tagger named none and landing an artist re-indexes its
  tracks. `playlists.pinned` is prefixed onto every `PlaylistOrder`, and the place it has to be
  threaded through that is easy to miss is `undo.rs`, which re-creates the whole row: a column
  missed in `Held`, `held_in` and `rewritten` is lost the first time somebody undoes an edit.
  `store::READ_BACKWARDS` went from 8 to 16 in the same pass, because it was exactly the count of
  sort orders and a ninth would have been read back as the direction bit.
- **What was listened to is read back rather than kept.** `listens` grew a `heard` — the
  nanoseconds of that visit actually listened to — and `listens_by_time`, and `statistics.rs` is
  three reads over them, each bounded by `listens.at >= ?` so the index serves it. A day is
  bucketed by dividing the stamp, because there is no date crate in the tree and one
  civil-from-days is cheaper than one; a day nothing was played on is written as a zero row, so a
  chart has no gaps to draw around. Nothing is stored that the history does not already say.
- **A suggestion is a search, which is why it costs almost nothing.** `suggest.rs` builds a name,
  a reason, a row count and a `SavedQuery` **in the existing grammar, written through `Display`**
  — so a suggestion and the search box cannot drift, saving one is `Library::save_query` and no
  new code, and nothing is persisted until it is. Nothing is offered that cannot be filled, so a
  thin catalog offers few rather than nonsense. Two things the real data settled: MusicBrainz
  hands out `2010s` as a *genre*, which stood beside the decade built from real years and said
  the same thing worse, so a genre that only names a decade is left to the decades; and a genre
  arrives lower-cased, so one is billed with its words capitalised after any mark rather than
  only after a space, or `contemporary r&b` reads as *Contemporary R&b*.
- **A suggestion is drawn as well as counted, and opens onto what it holds.** Each carries its
  length and `pictured_by`, the covered albums its rows fall on most, one per distinct picture, so
  the pane draws a card's art as a mosaic of real sleeves over a gradient of two palette accents
  — or the reason's icon and the name where nothing is covered — rather than keeping an image
  anywhere. The cards are shelved by `SuggestionKind`, and pressing one opens its rows with
  *Shuffle*, *Play next* and *Search for these* beside *Play*, *Add to queue* and *Save*, which
  greys to *Saved* once a playlist fills itself from the same search.
- **A search reaches what the library does not hold, greyed and wantable like any missing row.**
  After the held rows the tracks pane lists the release rows the catalog knows it lacks, read off
  `release_tracks.folded`, and then the songs `Reference::find_songs` found on MusicBrainz that
  the catalog names nowhere. Wanting a found song lands its first release as an album stamped
  `found_elsewhere`, which the orphan sweep spares only while a want stands on it, so the want is
  an ordinary one the providers fill. What a track *sings* is the fifth `tracks_fts` column —
  the file's lyrics or the fetched ones, timestamps stripped — reached by `lyrics:` and never by a
  bare word, and a plain-word search some track sings is offered as `lyrics:"…"`.
- **A share is text and one link, and the catalog already held the link.** `Shared::written` is
  the whole of it, in `resonate-library` because the window, `resonate share` and anything else
  that wants it must say the same thing. song.link takes the service URL appended whole — it
  answers 308 and lands on its own shortcode form — and the URL comes from the
  `release_track_links` and `album_links` MusicBrainz enrichment already wrote, a recording's
  preferred over its release's and the providers weighed in a declared order so one track shares
  the same way twice running. Where no service is linked it is the MusicBrainz recording, and
  where there is neither it is the text alone.
- **Every major listing has an order, and no index was added to give it one.** `SortOrder` grew a
  `Direction` beside it and `AlbumOrder` and `ArtistOrder` joined `PlaylistOrder` and `RowOrder`,
  so the tracks, albums and artists panes each sort and reverse where only the playlists pane
  could. A reversed order reads off the index it always did, SQLite scanning one backwards, so
  `order_by` is the natural spelling and its term-by-term mirror rather than a second index;
  `library.md` has why the two new enums are deliberately unguarded and why a saved query's
  direction rides in the `sort` column it already had. `Command::Order` is what puts the *queue*
  in order — a permutation of the play order applied in one pass, with the cursor re-seated, so a
  sort mid-track reopens no stream — and `resonate playlist --reverse` now turns `--sort` round as
  well as `--order`. There is no `resonate albums` or `resonate artists`, so neither new enum has
  a CLI arg: a variant nothing constructs is one to leave out.
- **A run of rows is one `Span`, and every list edit takes one.** `resonate-core::Span` is a first
  and a last row read either way round, so it can never be empty, and `Span::landing` is the single
  piece of arithmetic that says where a span ends up when it is dropped on a row: on that row going
  up, ending on it going down, which for one row is what a single move always meant.
  `Library::remove_from_playlist`, `Library::move_in_playlist`, `Command::Remove` and
  `Command::Move` all carry one, so removing thirty rows is one SQL statement and one
  park-and-unpark pair rather than thirty edits, and `Queue::remove_rows` rebuilds `items` and
  `order` in one pass rather than shifting the play order per row. It lives in `resonate-core`
  because `resonate-library` and `resonate-engine` both need it and neither may depend on the other.
  `resonate-ui`'s `edit::Span` is a different type with the same name, naming a run of text edits to
  coalesce.
- **The transport outlives the run because the catalog keeps it, and only a run naming no files
  resumes.** `resonate-core::Resumption` is the vocabulary — the queue's rows as `Resumable`s in the
  order they were *loaded*, an `order` naming which of them each playing position holds, the row
  the queue was on, the frame into it and whether it was shuffled — and it is in core because
  `resonate-engine` builds one and `resonate-library` stores one and neither may depend on the
  other, which `Reordered` beside it makes the same argument for. `Keeping` is the sampler,
  read off the same `PlayerState` and published queue that `Listening` is, so the window's observer
  and `resonate play`'s 500 ms tick each keep what they are playing; `resume`, `resume_rows` and
  `resume_order` are what hold it, `library.md` has why that is three tables and `audio.md` what
  the sampler writes when.
  Coming back is `Command::Resume`, which takes the whole resumption: the queue opens paused on the
  row it was left, at the frame it was left at, playing the order it was playing with the order it
  was loaded in underneath — so unshuffling gives back the album rather than the shuffle — and plays
  only when told to. What is *not* kept is the volume and the repeat mode, which are settings a run
  makes rather than a place it reached. Ids are not kept either — a `QueueItem` pairs a
  `MediaLocation` with a `TrackId` that `unclaimed_id` minted for that run — so the locations are
  stored and `Queue::restore` mints again on the way back. Only the bare `resonate` resumes, because
  every other way in names what to play; and turning `resume` off discards what was kept rather than
  merely ceasing to add to it, in the settings pane at once and on the next run that reads the key.
- **A track is studied whole, once, and what the study says is kept.** `resonate-analysis` decodes
  a file end to end through the same `Decoder` the player uses and feeds every block to one pass
  of builders: a bounded waveform envelope, a long-term average spectrum and a spectrogram, sample
  levels and the bits in use, BS.1770 loudness and a DR reading, and a Chromaprint of the first two
  minutes. `verdict.rs` reads the spectrum, the bits and the codec into a `Verdict` — `Genuine`,
  `Suspect`, `Fake`, `Lossy` or `NotJudged` — with typed `Finding`s: a lowpass wall where a lossy
  encoder cuts, a wall where a lower rate ended, bits that never move. The pane draws the whole
  analysis; the enrichment keeps the summary in `track_studies`, studies every track beside the
  pass on a pool of its own, and recognises each print through the `Fingerprints` seam, which
  `resonate-online`'s `AcoustId` fills where an `acoustid-key` is set. What the audio is heard as
  is weighed against what the row is called, so `is:fake`, `is:suspect` and `is:misnamed` narrow
  the catalog and `Route::Fingerprint` names a track nothing else could. `analysis.md` has the rest.
- **Discord is told nothing unless the listener says so, and under an application of their own.**
  `resonate-discord` is behind the `discord` feature, but the feature being in the build is not
  presence: `discord` defaults to false, `discord-app` to nothing, and `Presence::active` wanting
  both is the one gate — inactive, no thread is started and no socket is looked for. The window
  reaches the publisher through `resonate_ui::Present` the way it reaches the file through
  `Settings`, so every switch is live, and a cover is a Cover Art Archive address Discord fetches
  itself, never a picture of the listener's sent anywhere. `discord.md` has the rest.
- **What the window is painted in is a name in `resonate-core` and a palette in `resonate-ui`.**
  `Appearance` is a `Theme` and an `Option<Accent>`, both of them names with a text form and
  nothing else, and it lives in core because the config reader and the window both need it and the
  reader is compiled without the `ui` feature — the same reason `MediaLocation::from_uri` is there.
  The accent is optional because a palette declares the one it was built around and that is what a
  fresh install wears, so `None` is *the palette's own* rather than an absence, and the settings
  pane clears the `accent` key for it the way it clears `contact`. Not one
  hex is here: `resonate-ui`'s `theme.rs` owns the `Flavour` each theme resolves to, along with
  which of its seven accents is native to it, so core stays free of anything that can be drawn.
  `ui.md` has what a palette holds and what holds it to being readable.

## Commands

```
cargo build --workspace --exclude resonate-ui --no-default-features   # audio stack only
cargo test  --workspace --exclude resonate-ui --no-default-features   # the usual inner loop
cargo build --workspace                    # everything, including gpui
cargo test  --workspace                    # everything
cargo test -p <crate> <test_name> -- --exact --nocapture
cargo test -p resonate-pipewire --test stream  # needs a live daemon; prints a skip without one
cargo test -p resonate-pipewire --test reconnect  # hosts a daemon of its own and restarts it;
                                                  #   needs the pipewire binary
cargo test -p resonate-mpris --test bus        # needs a session bus; prints a skip without one
cargo test -p resonate-codec --test encoded    # needs ffmpeg, and metaflac for the embedded
                                               #   CUESHEET block; prints a skip without either
cargo test -p resonate-library --test library  # one embedded-sheet test needs ffmpeg and skips
cargo clippy --workspace --all-targets -- -D warnings
cargo tree -p resonate-core                # must stay free of symphonia, pipewire, gpui, serde
cargo tree -p resonate-library             # must stay free of ureq, serde
cargo tree -p resonate-eq                  # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-vault               # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-dsp                 # must stay free of resonate-eq
cargo tree -p resonate-providers           # must stay free of the library, codec, vault, gpui
cargo tree -p resonate-analysis            # must stay free of gpui, the engine, the library, ureq
cargo tree -p resonate-discord             # must stay free of gpui, the library, ureq
cargo test -p resonate-online --test live  # reaches the real services; prints a skip unless
                                           #   RESONATE_ONLINE_TESTS is set
rust-formatter                             # format; never `cargo fmt`
rust-formatter --check                     # read-only; exits 1 with a diff
cd fuzz && cargo +nightly fuzz build       # the parsers' fuzz targets; needs cargo-fuzz
cd fuzz && cargo +nightly fuzz run probe corpus/probe seeds/probe -- -max_total_time=180 -timeout=15
```

**The CI runs what the list above runs, and nothing the list does not.** `.github/workflows/ci.yml`
takes every push to `master` and every pull request through clippy with `-D warnings`, the headless
build and tests, the whole workspace's build and tests, the `cargo tree` refusals above and
`cargo +nightly fuzz build`, each in an `archlinux` container holding the PKGBUILD's dependencies
plus ffmpeg and `metaflac`, so the tests that want them run rather than skip. There is no daemon and
no session bus there, so the PipeWire and bus tests print their skip — all but the reconnect test,
which starts a daemon of its own. `RUSTFLAGS` is emptied over `target-cpu=native`, because the cache
a job restores may have been built on a runner with another CPU, and a native build from one faults
on another. The separate Fedora 44 workflow builds the source RPM and attaches the binary and
source RPMs when a GitHub release is published. Formatting is the one check CI cannot
make: `rust-formatter` is not something a hosted runner installs, so `rust-formatter --check` stays
local. A command added to the list above that a runner can run is added to the workflow too, and a
crate added to the layering refusals is added to the workflow's `refuse` lines.

**The hand-rolled parsers are fuzzed from outside the workspace.** `fuzz/` is a crate of its own —
its `[workspace]` table detaches it, so `unsafe_code = "forbid"` and the workspace's lints do not
reach libfuzzer's macros. Five targets: `probe` drives `probe`, `probe_stream`, `probe_cover_art`,
`probe_span` and a bounded `Decoder` over bytes served from memory by a `MediaProvider` of its own,
which is how `riff.rs`, `matroska.rs`, `flac.rs`, `text.rs` and `dsd/` are reached through the
public API rather than through a widened one; `boxes` and `cue` take the two readers that already
answer to bytes. The other two need a seam, and it is `#[cfg(fuzzing)]` rather than public:
`resonate_lyrics::read_an_lrc_sheet` and `resonate_library::read_a_playlist_sheet` are compiled
only under the cfg cargo-fuzz sets, so the normal build's surface is unchanged and nothing in the
tree carries an entry point with no caller. `sheet::parse` was split out of `sheet::read` for the
second, which is the better factoring anyway: the reading of a sheet no longer needs a file.
The corpus a run grows is not kept — `.gitignore` has it — but a seed corpus is:
`fuzz/seeds/<target>` holds the smallest file of each thing a target reads, handed to a run as a
second corpus folder so what the run grows lands in the ignored `corpus/` and the seeds stay as
they were. `probe` has one of every container the scan takes — a 50 ms 8 kHz tone ffmpeg writes as
WAVE, a three-channel 24-bit WAVE, FLAC carrying a Vorbis `CUESHEET`, MP3, ADTS, AAC and ALAC in
MP4, FLAC in Matroska, Vorbis, Opus in stereo and in 5.1, AIFF and CAF — beside a DSF, a DSDIFF and an MP3 whose ID3v2 carries
`SYLT`, `USLT` and a MusicBrainz `UFID`, written by hand to the formats' own layouts; seeded, a run
starts at 11 733 edges where an empty one starts at 343. `boxes` takes the two MP4s, and `cue`,
`lrc` and `playlist` a sheet each. A seed is added by hand when a run finds something worth
starting from, and nothing runs the targets but a person.

**A cost is measured rather than guessed, and neither measure is a check.**
`cargo bench -p resonate-dsp --bench stages [<words>]` runs every DSP stage, and two whole chains,
over four seconds of audio and prints each as a share of a core; any word narrows it to the runs
whose names hold it. It plays a hot signal through the true-peak guard as well as the usual one,
because a guard that never limits never pays for limiting. `cargo build --profile profiling` is
the release build with its symbols and line tables kept, which `perf record` and
`cargo flamegraph` need and `strip = "symbols"` takes away. Neither asserts anything, so the CI
runs neither.

**A debug build is optimised, because an unoptimised resampler cannot keep up with the music.** At
`opt-level = 0` a 96 kHz 24-bit source pegged a whole core to reach a 48 kHz sink and the ring
starved: the window wedged, and a `SIGTERM` asking a wedged front end to drain politely left the
audio playing. `[profile.dev]` is `opt-level = 1` for the workspace's own crates and the same for
its dependencies, with `resonate-dsp` and `resonate-codec` at 3 — the same file then costs 1.5 % of
a core instead of 100 %. Debug assertions and debuginfo stay on; it is the optimiser that was
missing, not the checks.

**One signal silences the graph even where the front end never answers.** `signals.rs` asks the
front end to quit and then leaves anyway after `DRAINS_WITHIN`, which bounds how long a wedged
window can live but not how long it can be *heard* — for those five seconds the engine was still
playing, because nothing was stopping it. The farewell thread sends `Command::Stop` after
`SILENCED_AFTER` and then waits out the rest. A front end that is going to answer answers in about
250 ms, so it is gone long before that and the polite drain is untouched; one that is not is quiet
a second after the signal rather than five. The `Player` reaches the thread as an `Arc` from both
call sites, and `Player::send` only pushes onto a channel, so it is safe from a signal handler's
thread and answers `EngineStopped` where the engine has already gone.

**A headless pass stops at a file boundary on the first signal.** `resonate scan`, `enrich`,
`poll`, `tag`, `organise` and `vault --import` each run through `until_told`, which puts
`signals::cancel_when_told` over the pass for as long as it runs: the first `SIGINT` or `SIGTERM`
calls the pass's own `cancel`, so the file being written is finished, the catalog follows it and
the summary says `cancelled`, and a second leaves at once. The six handles are one
`resonate_library::PassHandle` over each pass's progress and summary — `ScanHandle` and the rest
are aliases of it — whose progress is `Cancelling`, and a thread that dies answers
`Error::Stopped { pass }` naming its `PassKind`; `until_told` is generic over it, so a seventh
pass is an alias and a `PassKind` variant rather than a struct, an error and a macro arm.

`.cargo/config.toml` builds for `target-cpu=native`, so the resampler's taper and convolution
vectorise to whatever the machine has rather than to the `x86-64` baseline's SSE2, which is worth
1.4x to 1.7x on High, more the further the rates are apart. The Arch package keeps it:
`packaging/PKGBUILD` appends `-C target-cpu=native` to whatever `RUSTFLAGS` makepkg hands it in
`build` and `check`. Appending is the point — any `RUSTFLAGS` in the environment replaces
`build.rustflags` outright, and makepkg always sets one — so an AUR build is made for the machine
that builds it and is for that machine alone. Anything that produces a binary for another machine,
the CI among them, sets `RUSTFLAGS` back over it.

`packaging/resonate.spec` builds the default-feature binary and generated CLI material on Fedora
44 from a source archive of the committed tree, tests the headless workspace in `%check`, and
installs the desktop payload beside it. `packaging/org.resonate.Resonate.yml` builds the same
binary from the checkout with the Freedesktop 25.08 SDK, for the architecture's baseline CPU.
The packaging rule covers that baseline and why the Arch package instead keeps this checkout's
`target-cpu=native`.

`--exclude resonate-ui --no-default-features` is what keeps gpui out of a build. Excluding the crate
is what does the work: `resonate-ui` depends on `gpui` unconditionally, so the binary's `ui` feature
alone is not enough.

`resonate-ui` stays in the default member set — decided, not deferred. Dropping it would not skip
gpui, because the binary's `ui` feature is on by default and pulls the crate in as a dependency
anyway; turning that feature off by default would stop a bare `cargo run` opening the window. The
two flags together are the only lever, and they are already the documented one.

The binary doubles as the test harness for the audio stack, reaching layers a screenshot cannot:

```
cargo run -- sinks                    # the sinks PipeWire advertises; hits the real daemon
cargo run -- explain <file>           # the OutputPlan for a file; hits the real daemon
cargo run -- info <file> [--graph]    # tags, ReplayGain, box order and bitrate statistics
cargo run -- analyse <file>           # decodes the file whole: the verdict on its lossless claim
                                      #   and what it rests on, the levels, the loudness, its range
                                      #   and DR, the spectrum, the waveform's peak and RMS as text
                                      #   and the print; a .cue with --track <N> or a URI carrying
                                      #   #frames= analyses that one cut, and --recognise asks
                                      #   AcoustID what the audio is
cargo run -- listen                   # records twelve seconds of what the desktop plays — or, with
                                      #   --microphone [<name>], of a microphone — and names the song
                                      #   through Shazam, AudD or AcoustID; --seconds <n> listens
                                      #   longer or shorter and --microphones lists what there is
cargo run -- play <files>             # plays a queue; each file is a path or a file:// URI, and
                                      #   it reads transport keys on stdin (? for help)
cargo run -- queue <files>            # adds them to the player already on the session bus, at the
                                      #   end or, with --next, after the row being played; --play
                                      #   hears the first of them as it lands, --playlist <name>
                                      #   sends a playlist's rows instead of files, and --player
                                      #   <name> says which player where a session runs more than
                                      #   one
cargo run -- players                  # the players of this build answering on the session bus,
                                      #   with what each is playing and how many rows it holds
cargo run -- scan <roots>             # scans into the SQLite library, prints the counts and,
                                      #   where there is a reference, enriches what it holds
cargo run -- roots                    # the roots a bare `scan` walks
cargo run -- enrich                   # asks the reference about every album and artist not
                                      #   asked lately; --refresh asks again about those already
                                      #   answered and --albums <N> caps how many of each
cargo run -- wants                    # the release tracks marked wanted, with where what a
                                      #   provider delivered landed
cargo run -- missing                  # the release tracks the catalog holds no file for, and
                                      #   the releases of held artists it holds none of, as a
                                      #   count line and two tables; --artist <NAME> lists one
                                      #   artist's alone
cargo run -- poll                     # asks every registered provider for each want not tried
                                      #   lately and lands what they deliver in the vault as
                                      #   track rows; --again asks about those tried lately too
cargo run -- forget <roots>           # drops roots and every track scanned from them
cargo run -- tag                      # the tags that would be written into every scanned file to
                                      #   say what the catalog was told about it, one row per
                                      #   field, with the album's cover where the file carries
                                      #   none; --apply writes them, reads each file back and has
                                      #   the catalog follow what it now says, and --root <root>
                                      #   writes only the files scanned from that root
cargo run -- vault                    # what the managed vault holds, by form, with what its
                                      #   sources weigh and what was saved; --import prints what
                                      #   would be kept and --apply keeps it, --root <root> takes
                                      #   only the tracks scanned from that root and --at-most <N>
                                      #   caps the run; --verify decodes every object and weighs
                                      #   it against what went in; --prune takes away the objects
                                      #   and covers nothing names any more, and --release points
                                      #   every vaulted row whose file is still there back at it
cargo run -- organise                 # the moves that would file every scanned track under the
                                      #   layout `organise-as` names, each written against its own
                                      #   root, with what is refused and why; --apply makes them,
                                      #   rewrites the catalog to follow and prunes the folders
                                      #   they empty, --as <layout> files them under another shape
                                      #   for this run alone and --root <root> files only the
                                      #   tracks scanned from that root
cargo run -- playlists                # the playlists the library holds, with their lengths and
                                      #   play counts; --order names which way to list them,
                                      #   --reverse turns that order around and --named lists only
                                      #   the ones whose name holds every one of those words
cargo run -- playlist <name>          # plays one; --add <files> appends, --into <other> copies its
                                      #   rows into another playlist, creating it if new,
                                      #   --export <file> writes M3U, PLS or XSPF by its extension,
                                      #   --order <order> puts the rows in album, artist, title,
                                      #   length or file order and --reverse turns that round,
                                      #   --keep holds it in that order and --by-hand lets it go,
                                      #   --tidy drops rows whose files have gone and --fold drops
                                      #   the rows a file is already named by, --query <text>
                                      #   saves one that fills itself, with --sort <order> for the
                                      #   order it hands rows back in — relevance, album, title,
                                      #   artist, added, length, plays or played — and --limit
                                      #   <rows> for its cap, --matching <text> plays only the
                                      #   rows that search matches, copies only those, or with
                                      #   --drop takes only those out, --rename <new> renames it
                                      #   and --discard takes it and its rows away for good
cargo run -- eq                       # the equaliser: whether it is on, what is bound to each
                                      #   device — a profile or its own curve — and the bands of
                                      #   the curve in force. --on and
                                      #   --off switch it, --for <sink> says which binding the
                                      #   rest reads or writes and with none it is the fallback,
                                      #   --profile <name> binds one and --unbind takes it away,
                                      #   --list names what is kept, --import <file> reads an
                                      #   EqualizerAPO or AutoEq GraphicEQ file in and --export
                                      #   <file> writes one out, --forget <name> discards one,
                                      #   --find <text> searches AutoEq, --fetch <device> keeps a
                                      #   measurement and --suggest weighs the chosen sink's own
                                      #   description against the catalogue
cargo run -- studies                  # what the lookup's studies of every track found, with a
                                      #   count line; --fakes, --suspects and --misnamed narrow it
cargo run -- stats                    # what was listened to: the totals and the three tables of
                                      #   what was heard most, over --window week, month, year or
                                      #   all, with --top <N> rows in each
cargo run -- favourites               # what is marked, all three kinds or whichever of --tracks,
                                      #   --albums and --artists is asked for
cargo run -- suggest                  # the playlists the catalog suggests to itself, each with
                                      #   the search it fills from; --save <name> keeps one
cargo run -- share [<file>]           # the track, its album and one link, for the file named or
                                      #   for whatever the running player is playing
cargo run -- sleep <spec>             # stops the running player after so many minutes, or at
                                      #   `track`, `queue` or `off`; --player <name> says which
cargo run -- mcp                      # serves the catalog and the running player to a language
                                      #   model over the Model Context Protocol, on stdin and
                                      #   stdout; --player <name> says which player
cargo run -- import <files>           # reads M3U, PLS and XSPF sheets in; --as <name> overrides
                                      #   what one declares
cargo run -- <files>                  # opens the window with those files queued, as `Exec=resonate %U` does
cargo run                             # opens the window on the queue the last run left, paused
                                      #   where it stopped, unless `resume` is off in config.toml
```

All twenty-nine subcommands run end to end, and so does the bare `resonate`, which opens the GPUI
window. No crate holds `todo!()`. `resonate info` folds a raw tag value onto one line and cuts it at 72
characters, because a lyric tag runs to hundreds of lines and would otherwise take the table's
alignment with it, and it reads a length back through one clock that carries — rounded to the
millisecond, so a minute boundary rolls over rather than printing `1:60.000`, and past an hour it
says so. A *listening* total is drawn by a second clock, `stats::heard_for`, because a year of it
rounded to the millisecond is absurd; the two are separate readings rather than one with a
setting. `--config`, `--library`, `--vault`, `--sink`, `--quality`, `--filter-phase`, `--dither`,
`--noise-shaping` and `--no-bit-perfect` are `global`, so they
read the same before the subcommand and after it.

`RESONATE_LOG` is read through `EnvFilter::try_new` rather than `try_from_env`, so a filter that
cannot be parsed warns and falls back to `DEFAULT_LOG` where an unset variable simply takes it —
the one setting where a silent fallback would hide the diagnostics being reached for.

The window and every command that plays — `resonate play` and `resonate playlist <name>` — put
`org.mpris.MediaPlayer2.resonate` on the session bus, which is how media keys reach the transport —
nothing in the workspace reads a key. `busctl --user
introspect org.mpris.MediaPlayer2.resonate /org/mpris/MediaPlayer2` is the quickest look at it.

Settings load from `$XDG_CONFIG_HOME/resonate/config.toml`, or from `--config <FILE>`, which must
exist where the XDG path may not. A CLI flag outranks the file, the file outranks `EngineConfig`'s
defaults, and an unknown key warns through `tracing` rather than failing the run. Every key is a
`ConfigKey` variant, so a bad value names the key without putting prose in an error. Eight of the
fifty-two have a flag — `sink`, `library`, `vault`, `quality`, `filter-phase`, `dither`,
`noise-shaping` and `bit-perfect`, the last as `--no-bit-perfect` — and the other forty-four have none, so the
settings pane and the file are the whole of how any of them is set: the output's `true-peak`,
`restore-lossy`, `replay-gain`,
`replay-gain-pre-amp`, `replay-gain-untagged`, `dop`, `force-graph-rate`, `bluetooth-wake`,
`bluetooth-lead-ms`, `bluetooth-awake-s`, `volume` and `buffer-ms`; the window's `theme`, `accent`, `text-size`, `minimise-button`,
`maximise-button`, `scroll-volume`, `scrollbars`, `suggestions-tab` and `missing-tab`, which every headless subcommand has no use for; and the standing decisions
rather than per-run ones — `online`, `enrich-after-scan`, `study`, `contact`, `acoustid-key`, `equaliser`,
`equaliser-for`, `equaliser-profile`, `resume`, `skip-repeats-queue`, `organise-as`, `notify`, `audd-token`,
`listen-from`, `listen-for` and `inbox`, the last
chosen with the Library category's *The inbox* group, which polls from the window as well — and
the seven that shape a Discord presence, `discord`, `discord-app`, `discord-shows`, `discord-art`,
`discord-icon`, `discord-progress` and `discord-paused`, written by the Desktop category's two
Discord groups and live the moment they are.
`listen-from` is `desktop`, `microphone` or a microphone's node name and `listen-for` a whole
number of seconds; `resonate listen --microphone` and `--seconds` outrank them for one run, and
the Online category's *Listening* group and the Listen sheet's own chips write them.
`ConfigKey::ALL` is the list. `enrich-after-scan` defaults to true and is
read by `resonate scan` and the window's scan alike; off, the reference is asked only by
`resonate enrich`, the Library card's *Enrich* and the Online card's *Look up*. `study` defaults
to true and is read by every lookup, from the command line or the window; off, no lookup starts
the pool of studies, and a track is decoded only where the pane analyses it or the fingerprint
route needs its print.
`minimise-button` and `maximise-button` both default to true and are read by the window alone:
`Config::window_buttons` folds them into a `resonate_ui::WindowButtons` that rides into `run` on
`Stored` and lives on `ResonateApp`, so a switch in the Appearance category hides the button on
the next frame and close is never one of them. `scroll-volume` defaults to true and is the
window's too: it rides on `Stored` onto `ResonateApp::scroll_volume`, the Appearance category's
*The volume wheel* switch writes it, and while it is on a wheel over the volume slider moves the
volume a notch — five per cent — at a time. `scrollbars` defaults to true and rides the same
way onto `ResonateApp::scrollbars`, written by the Appearance category's *Scrollbars* switch; off,
no pane draws a bar and every region still scrolls. `suggestions-tab` defaults to true and
`missing-tab` to false, and `Config::tabs` folds them into a `resonate_ui::Tabs` that rides the
same way onto `ResonateApp::tabs`, written by the Appearance category's *Sidebar tabs* switches:
a tab that is off is left out of the sidebar and of the pane keys, `RootView::set_pane` lands on
the tracks rather than on it wherever it is asked for, and the artist page's *not held* button
into Missing is not drawn.
`vault` has a flag —
`--vault` outranks it the way `--library` outranks `library` — and it defaults to
`$XDG_DATA_HOME/resonate/vault`. A run that does not name one opens the vault only where that
folder is already there, so a build nobody has imported into creates nothing and carries no vault
at all.

The settings pane writes back through `resonate_ui::Settings`, a seam the binary fills with
`settings::File`. It edits the document with `toml_edit` rather than reserialising a parsed
`Config`, so a hand-written file keeps its comments and its key order, and renames a staged file
over the target so a crash mid-write cannot truncate it. `config::edited` is the one way in: it
follows a symlinked `config.toml` to the file it names, holds `config.toml.lock` for the whole
read-edit-write so a window and a `resonate eq` beside it cannot each write over the other's
change, stages under a name of this process's own, carries the file's mode across, and
`sync_all`s the staged file and the folder around the rename. A value that has a flag spells it
the way the flag does — `quality`, `dither` and `noise-shaping` are read and written through the
same `ValueEnum`s `cli.rs` declares — so `--noise-shaping flat` is `noise-shaping = "flat"`.
`Setting::Theme` and `Setting::Accent` go through the same seam and nowhere near the engine,
which never hears about either, and so do
`Setting::Online` and `Setting::Contact` from the Online card: a cleared contact is
`config::clear` on the key rather than an empty string, which is what keeps `Config::contact`
`None` and the User-Agent bare.

The agent shell does not inherit the desktop session, so a bare `cargo run` cannot open a window.
The `run-ui` skill in `.claude/skills` has the recipe.
