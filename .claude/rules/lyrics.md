---
paths:
  - "crates/resonate-lyrics/**/*.rs"
  - "crates/resonate-ui/src/views/lyrics.rs"
---

# Lyrics

`resonate-lyrics` carries the vocabulary and the provider seam; `resonate-ui` draws it. The crate
takes no dependency on gpui, the engine or the library, so the whole of it is tested without a
window.

## The vocabulary

- **A `Lyrics` is a flat list of `LyricLine`s that is `Synced` or `Unsynced`, and `Timing` is the
  whole of the difference.** A karaoke-timed word, a translation beside the original and a second
  voice have no representation, so anything richer than a line with an optional moment is outside
  what the type can say. `Wanted` is the other half of the vocabulary — the track a provider is
  asked about — carrying the title, artist, album and length a provider would search on and the
  text the file itself holds as `Wanted::carried`.
- **`Lyrics::line_at` is what the pane reads at and `line_in_play` is what it lights, and they part
  company.** `line_at` is the last line whose moment has passed, and it is what the pane centres on,
  so a long instrumental holds its place rather than scrolling off it. `line_in_play` is that line
  only while it is still being sung, which `LIT_AT_MOST` caps at ten seconds, so the set goes dim
  where the pane stays put. The window turns on each with a clock of its own for exactly that
  reason, and it is `line_in_play` running out that takes the last line of a set away once it has
  had its word.
- **A `Credits` is what a sheet claims about its own making, and it rides on the set rather than
  beside it.** `Credits` carries `[al:]`, `[au:]`, `[by:]`, `[re:]`/`[tool:]` and `[ve:]` as
  `album`, `words_by`, `sheet_by`, `editor` and `version`, and `Lyrics::credited` is the one way one
  is attached — a builder rather than a fourth argument, so `Lyrics::synced` and `Lyrics::plain`
  stay the positional pair a provider that has never heard of an id tag calls. `Lyrics::credits` is
  what the window reads back. It is the sheet's claim about *itself* rather than about which track
  it is, which is why an embedded set carries its credits although it is checked against nothing:
  the text came out of the file, so who transcribed it and what laid it down are still the sheet's
  own word on itself.
- **`waiting_at` is what a flat set can say about a gap.** It answers only where nothing is in play:
  which line the wait is for and how far through it the transport is, counting from the start of the
  track before the first line and from the moment the last line went out after it. It lives here
  rather than in the window because it is the one arithmetic that has to agree with `LIT_AT_MOST`,
  and a constant duplicated in `resonate-ui` would be a second thing to keep in step. How far
  through a *line* the transport is has no reader: the set has no word timing, so it could only ever
  be the whole span pretending to be a karaoke sweep.

## Providers

- **Lyrics come out of the file, and one LRC reader is the whole of how.** `Sidecar` reads an `.lrc`
  or a `.txt` beside a local file or in a lyrics folder next to it; `Embedded` reads
  `Wanted::carried`, which is `TagSet::lyrics` —
  `USLT`, `LYRICS`, `UNSYNCEDLYRICS`, `UNSYNCED LYRICS` or `©lyr` — reaching the pane through the
  `StreamDigest` like every other tag. ID3's `SYLT` reaches symphonia as a raw frame, and
  `resonate-codec`'s `sylt.rs` writes it out as LRC text — a line per entry, or, where entries open
  with a newline, the syllables between two newlines gathered into one line at the first one's
  moment — so it is read by the same reader as everything else and outranks an unsynchronised frame
  beside it. A frame timed in MPEG frames rather than milliseconds is left unread, because nothing
  there says how long a frame is. Both parse through the same `lrc` reader, so a tag written
  with timestamps is a synced set exactly as a file would be, and a sidecar outranks what the file
  carries because it is the deliberate one. `read_lyrics(source, text)` is that reader's one
  public door — `lrc::read` with its `Sheet` folded to the `Lyrics` it holds — and `Embedded` and
  the online crate's `Lrclib` both go through it, so text out of a tag and text off a service are
  read under the same bounds and the `Sheet`'s `Declared` stays `Sidecar`'s alone.
- **A provider that reaches a network lives in `resonate-online`, and what it keeps lives in the
  library.** `Lrclib` is a `LyricProvider` like the two here, appended by the binary through
  `Lyricists::and` where `online` is on, and `online.md` has how it asks. What this crate carries
  for it is `Wanted::span`: a cue row is a span of a file, and the catalog's `lyrics_kept` is
  keyed by `(path, span_start)` the way `tracks` is, so a `Wanted` carries the `Option<FrameSpan>`
  the window fills from the queue item and a provider that keeps nothing ignores it. The kept row
  is read before any request and a miss is remembered for a week, so a track the service has no
  words for costs one request a week rather than one per play.
- **`Lyricists` walks the providers and a refusal is not an answer.** `Lyricists::local` registers
  the two behind `Unsourced`, which answers with nothing and is the one source `has_a_source`
  discounts, so a build with neither says a source is not configured rather than that the track has
  no words. A provider that refuses is logged and the walk continues; a refusal is reported only
  where nothing else answered.
- **A sidecar is checked against the track it sits beside; an embedded set never is.** `lrc::read`
  answers with a `Sheet` — the `Lyrics`, credits and all, and a `Declared` of what the file's own
  `[ti:]`, `[ar:]` and `[length:]` say it is about. `Sidecar` passes over a sheet declaring another
  track, folded to alphanumerics and case with either side allowed to contain the other, so
  "Echoes" still answers for "Echoes (Live at Pompeii)" while Jeff Buckley's "Hallelujah" does not
  answer for Leonard Cohen's. A sheet that declares none of the three is read as it always was, and
  so is one beside a row nothing has named yet. `Embedded` never checks, because the text came out
  of the file already.
- **A row cut out of a file is handed its own share of the file's words, on its own clock.**
  `Wanted` carries the row's `span` and the stream's `rate`, and `Wanted::cut_of_the_file` turns a
  set that belongs to the whole file into the row's: `Lyrics::within` keeps the synced lines that
  fall inside the span and shifts them by where it starts, so the row's first line is timed from
  the row's own zero. An unsynced set cannot be cut and is handed to no row, and a set whose lines
  all fall outside the span is none. What counts as the file's is a sidecar named after the file —
  `Rank::named_after` answers `NamedAfter::TheFile` for the two names it takes from the path — and
  whatever `Embedded` carries; a sidecar named after the row's own title is already the row's and
  is handed over whole. A cut with no rate to time it by is handed nothing rather than the album.
- **A declared length is weighed the same way, as a backstop rather than as a matcher.**
  `LENGTH_MAY_DIFFER_BY` is thirty seconds: a sheet rounds to the second and a rip's trim or a
  remaster moves it by a second or two, so anything tighter would throw away good sheets, while two
  different songs sit within half a minute of each other often enough that nothing looser would be
  worth having either. What it does catch is the single edit's sheet copied in beside the
  twenty-three minute album version. It can be that loose because the sidecar is already named after
  the file or the tags — the length is the last word against a sheet fetched for another pressing,
  not how one is found. A sheet that declares no length, and a track whose own length the engine has
  not published, are read exactly as they were before it existed, and `Wanted::duration` is what the
  window fills for it to weigh. `Embedded` is exempt for the same reason it is exempt from the
  title.
- **The sidecar walks folders rather than trying fixed names, and there are two of them.** `WITHIN`
  is the set of folder names it will descend into — `lyrics`, `lyric` and `lrc` — matched
  case-insensitively against the entries the parent walk already listed, so a `Lyrics/` under any
  casing is found without a stat per guess. `Rank` is the whole of the precedence and it is read in
  that order: `.lrc` over `.txt` first, then how the name was arrived at — the file's stem, its
  whole name, then `<artist> - <title>` and `<title>` off `Wanted` — and only then where it was
  found, so a sheet beside the track outranks the same name filed away in `Lyrics/` while a sheet
  named exactly after the file still outranks a tag-named one lying beside it. The tag spellings go
  through `lrc::folded`, the fold the `[ti:]` check already uses, and are compared for equality
  rather than containment: `pink_floyd echoes.lrc` answers for `01 Meddle side two.flac`, where
  containment would hand every sheet whose name holds the title to every track. A track nothing has
  named yet offers no tag spellings, so the walk there is exactly what it always was. It reads at most
  `lrc::LARGEST_SHEET` bytes of a candidate and keeps the whole lines that fitted, rather than
  weighing what `fs::metadata` reports and refusing the file: a sheet whose words are followed by a
  tail too long to hold is read as its words, where a stat-and-refuse passed the lot over. The half
  line the bound cut through is dropped, so a sheet whose first line outruns the reader is read as
  holding nothing at all. The one bound is written once — `LARGEST_SIDECAR` *is* the reader's own —
  because the file is read as far as the reader would take it and no further. A candidate it cannot
  read is logged and the walk carries on — the rule
  `Lyricists` follows for a provider — so an oversized `Echoes.lrc` does not stop `Echoes.txt` being
  tried, and a refusal is reported only where nothing else answered; a lyrics folder that cannot be
  read is logged and left at that, because the sheet beside the track must still be reached.
- **A folder is walked once and remembered, because a track change cost a directory read.** `Walked`
  sits behind a `parking_lot::Mutex` on `Sidecar` — `LyricProvider::lyrics` takes `&self`, so there
  is nowhere else to put it — and holds at most `FOLDERS_WALKED` folders' file names as
  `Arc<[PathBuf]>`, the one last looked at first, so playing an album costs one `read_dir` for the
  folder and one for its lyrics folder rather than two per track, and the cap is what stops a
  shuffle across a library from remembering all of it. What makes a walk safe to reuse is the
  folder's own modified time, taken in a single stat: a sheet dropped in beside a playing track
  moves it, and the walk is taken again. A timestamp alone is not enough, though — Linux stamps an
  inode from a coarse clock that ticks once a jiffy and a filesystem may round to the second, so a
  folder written to in the same tick it was walked in carries the very time the walk recorded.
  `TIMESTAMPS_SETTLE_IN` is the answer: a walk is read back only where the folder's stamp is more
  than a second older than the walk itself, so a folder still being written to is walked again and
  one that has been quiet since is not.

## The LRC reader

- **The ten LRC id tags are nine meanings, and the closed set a bracket must name to be a tag at
  all.** `Identified::NAMED` is the whole table, ten names against nine variants, because `re` and
  `tool` are one meaning written two ways. Every one of them lands somewhere: `ti`, `ar` and
  `length` are `Declared`, what the sheet says it is *about* and all `Sidecar` weighs; `al`, `au`,
  `by`, `re`/`tool` and `ve` are the `Credits` the set carries, what the sheet says about *itself*;
  and `offset` is the shift every moment is read through. Nothing is recognised and then dropped —
  that a bracket was not a line used to be the only use seven of them had. A bracket that is
  neither a moment nor one of the ten is the line it was written on, which is what prints a
  `[Chorus]` marker rather than swallowing it — and equally what prints a writer's own tag under a
  name the spec never defined. It is the same rule `Search::read` follows for a token naming no
  field: what cannot be read as structure is read as content, so nothing fails to parse.
- **A value that is not arithmetic is left as if it had not been written.** A blank tag is passed
  over, an `[offset:]` that will not parse leaves the shift where it was, and a `[length:]` that
  will not leaves the sheet declaring none rather than declaring zero — which would make every
  sheet name another track. `span` is what reads one: `mm:ss`, `mm:ss.xx` and `h:mm:ss`, told apart
  by counting colons rather than by trying `moment` and hoping, because `moment` reads the second
  colon of `[00:04:75]` as a fraction separator and would take `1:03:20` for a minute and three
  seconds. A *moment* is the other way round: a bracket is `moment` first, so `[00:04:75]` keeps
  its hundredths, and `span` only where that fails, so `[1:02:03.45]` — which `moment` cannot read,
  its fraction holding a dot — is sung an hour in rather than printed as text.
- **A line is repeated once per timestamp written on it**, which is what an LRC sheet means by
  giving one line several moments. That repetition is why the reader bounds itself rather than
  trusting its input: `LARGEST_SHEET` is what it will read at all, `LARGEST_SET` and `MOST_LINES`
  what the lines it yields may come to, and a sheet past any of the three is
  `Error::Unreadable { op: Parse }`. The bound lives in the reader because `Embedded` is handed a
  tag out of an untrusted file and `Sidecar` has already cut its read to the same length, so what
  reaches `LARGEST_SHEET` from a sidecar is only ever text a lossy decode expanded.
