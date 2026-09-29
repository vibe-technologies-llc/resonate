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

- **A `Lyrics` is a flat list of `LyricLine`s that is `Synced` or `Unsynced`.** A line carries an
  optional moment and a `Voice`: one by default, two where a timed LRC line names it. Two voices
  can overlap, and each line remains lit until the next line of its own voice or ten seconds,
  whichever comes first. A line may also carry its words, timed one by one — below — and a
  translation beside the original still has no representation. `Wanted` is the other half of the vocabulary — the track a provider is
  asked about — carrying the title, artist, album and length a provider would search on and the
  text the file itself holds as `Wanted::carried`.
- **`Lyrics::line_at` is what the pane reads at and `voices_in_play` is what it lights, and they
  part company.** `line_at` is the last line whose moment has passed, and it is what the pane
  centres on, so a long instrumental holds its place rather than scrolling off it. The two active
  voice slots stay lit until each voice's next line or `LIT_AT_MOST` at ten seconds, so one singer
  entering does not put out the other. `line_in_play` remains the latest active line, and when
  neither voice is active the set goes dim where the pane stays put.
- **A `Credits` is what a sheet claims about its own making, and it rides on the set rather than
  beside it.** `Credits` carries `[al:]`, `[au:]`, `[by:]`, `[re:]`/`[tool:]` and `[ve:]` as
  `album`, `words_by`, `sheet_by`, `editor` and `version`, and `Lyrics::credited` is the one way one
  is attached — a builder rather than a fourth argument, so `Lyrics::synced` and `Lyrics::plain`
  stay the positional pair a provider that has never heard of an id tag calls. `Lyrics::credits` is
  what the window reads back. It is the sheet's claim about *itself* rather than about which track
  it is, which is why an embedded set carries its credits although it is checked against nothing:
  the text came out of the file, so who transcribed it and what laid it down are still the sheet's
  own word on itself.
- **A line may carry its words, each with its own moment, and the end the sheet gave it.**
  `LyricLine::worded` takes `SungWord`s and makes the line's text their joining, so a word is a
  byte range of the line and no second copy of the text can disagree with the first;
  `LyricLine::ending` sets `until`, refused where it falls before the line starts, and
  `SungWord::ending` the same for a word. `Lyrics::detail` is `Unsynced`, `Lines` or `Words`, the
  order `Lyricists::find` ranks by and the badge the pane draws. `LyricLine::sweep_at` is what the
  pane lights a word-timed line by: a `Sweep` of how many bytes are sung and the `Singing` word
  with how far through it the transport is, a word lasting to its own end, else to the next
  word, else to the line's end — and one with none of the three counted sung once it starts.
  `Lyrics::within` shifts a line's end and its words onto a cue row's clock with the line.
- **The pane sweeps a word-timed line word by word.** Only a line `voices_in_play` names is
  swept: its unsung words are drawn at `UNSUNG_SHARE` of the lit colour, the sung ones at all of
  it and the word being sung mixed between the two by how far through it the transport is, as
  highlight runs over one `StyledText`, so the line wraps exactly as it would unswept. While a
  word is being sung the pane asks for a frame each frame, the position being read afresh every
  16 ms poll, so the sweep moves at the display's rate rather than the transport rail's step. The
  heading's badge says `WORD-SYNCED` for such a set.
- **A line goes out once it has been sung, and a blank line is a pause.** A synced set carries when
  a line starts and nothing about when it ends, so `span_of` guesses: `sung_for` is
  `SUNG_BEFORE_THE_WORDS` plus `SUNG_PER_LETTER` a letter, held between `SUNG_AT_LEAST` and
  `LIT_AT_MOST`, and a line goes out there only where the next line of its voice is at least
  `A_BREATH_AT_LEAST` further on — otherwise it stays lit until that line, as it always did, so a
  verse never flickers between its lines. **A line whose sheet gave it an end goes out exactly
  there** — its own `until`, or its last word's — and the guess is only for a line with neither.
  A timed blank line, which is how an `.lrc` marks the
  break between verses, ends the line before it and is never in play itself, so the pause after a
  verse counts down to the next written line instead of lighting nothing. Before this a line held
  for up to ten seconds whatever it said and a blank one counted as sung, so the dots were only
  ever seen before the first line. `has_ended` is the reading past the last written line.
- **`breathes_before` is `waiting_at` read ahead of time.** It answers whether any position could
  ever wait on a line, by the same arithmetic — the first written line always, and any other
  where the written line before it goes out `A_BREATH_AT_LEAST` or more ahead of it — so the pane
  can give that line its room once, when the sheet is laid out, rather than as the wait begins.
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
  moment with each syllable written after an inline `<mm:ss.xxx>` stamp of its own, so a frame
  timed syllable by syllable is a line timed word by word — so it is read by the same reader as everything else and outranks an unsynchronised frame
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
  words for costs one request a week rather than one per play. The Lyricsfile reader is this
  crate's — `read_lyricsfile`, over `serde-saphyr` — so LRCLIB's answer and a sidecar beside the
  file are read by one reader, and a build without `online` reads the sidecar all the same.
- **`Lyricists` walks the providers and a refusal is not an answer.** `Lyricists::local` registers
  the two behind `Unsourced`, which answers with nothing and is the one source `has_a_source`
  discounts, so a build with neither says a source is not configured rather than that the track has
  no words. A provider that refuses is logged and the walk continues; a refusal is reported only
  where nothing else answered. **The walk hands over the most finely timed set, not the first**:
  every provider is asked and an answer replaces the one held only where its `Detail` is higher,
  so order breaks a tie — a sidecar still outranks the tags where both are line-synced — and a
  file's own plain words give way to a synced set fetched for it. A set timed word by word ends
  the walk, there being nothing finer to find.
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
- **A `.lyricsfile.yaml` beside a file is a sidecar, and the richest one.** `BESIDE` pairs each
  ending the walk takes with how it is `Written` — `.lyricsfile.yaml` and `.lyricsfile.yml` ahead
  of `.lrc` and `.txt` — and an ending is stripped whole, so `Echoes.lyricsfile.yaml` is named
  after `echoes` like `Echoes.lrc` is. A Lyricsfile is read to `LARGEST_LYRICSFILE` and refused
  past it rather than cut at a line, YAML being no use in halves; its `metadata` gives the
  `title`, `artist` and `duration_ms` the `[ti:]`, `[ar:]` and `[length:]` check weighs, so one
  declaring another song gives way to the `.lrc` beside it, and so does one that does not parse.
  `a_lyricsfile_beside_the_track_is_read_word_by_word_ahead_of_an_lrc` is the claim.
- **A name is weighed in the letters `folded_letters` spells, and a transliteration is no
  disagreement.** `lrc::folded` folds through `resonate_core::folded_letters` — the catalog's own
  fold, moved into core for this — so `Przybylowicz` agrees with `Przybyłowicz` and `KISKANC`
  with `Kıskanç`; where one side folds to ASCII and the other does not, the two are written in
  different scripts and nothing here can say whether *Kukla* is *Кукла*, so the check stands
  aside rather than calling it another song. Two names in the same script are weighed as before.
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
- **A stamp inside a line times the word after it — enhanced LRC.** `Stamped::read` splits a
  line's text on every `<…>` that reads as a moment, and whatever does not — `I <3 you`, `<b>` —
  stays text. Each stamp opens a word that runs to the next; text before the first stamp is folded
  into the first word, a stamp with nothing after it is the line's end, and a space written on
  both sides of a stamp is kept once, so the words always join to the line as it reads. A line of
  stamps with no `[mm:ss]` of its own is sung from its first word. The words go through `[offset:]`
  with the line, and a stamped line given several moments carries its words to each by the
  distance from the first. It lives in the one reader, so a sidecar, a tag, LRCLIB's synced text
  and the catalog's kept text all yield word timing alike; the search index already strips the
  stamps, so `lyrics:` never reaches one.
- **A timed line may name one of two voices with `[v1: words]` or `[v2: words]` after its
  timestamp.** The marker is removed from the text and stays with each repeated timestamp and
  with a cue row's shifted line. A line without one belongs to voice one, and an unrecognised
  bracket remains text. This extension uses the existing LRC reader for sidecars, embedded tags
  and fetched sets alike.
